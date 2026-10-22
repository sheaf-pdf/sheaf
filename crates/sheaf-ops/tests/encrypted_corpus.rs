//! Every encrypted corpus file that opens with the empty user password
//! saves encrypted by both strategies (M10.5 slice 7): its first page
//! turned, each output opens with no password, still encrypted with the
//! same `/P` and `/ID[0]`, with as many pages and the first one turned;
//! that page's content and strings and the `/Info` strings decrypt to
//! what the file's do; and PDFium opens it where it opens the file,
//! reading that page's characters as it read the file's. Skips when
//! there's no corpus (see `sheaf_doc::forms::corpus`).
//!
//! Needs libpdfium (`scripts/fetch-pdfium.sh`), like the app itself.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use in_progress::NoProgress;
use lopdf::Object;
use sheaf_doc::forms::corpus::{Corpus, each_entry};
use sheaf_doc::materialize::{Came, How};
use sheaf_doc::serialize::crypt::Crypt;
use sheaf_doc::store::BlobKind;
use sheaf_doc::tx::fixture::rotate;
use sheaf_doc::{Document, FixedCount, Source, Store};
use sheaf_edit::{Rotation, Turn};
use sheaf_render::PageIndex;
use sheaf_render::Renderer;
use sheaf_render::pdfium::PdfiumEngine;

/// What a save `how` writes of `doc`, or why it didn't, and what the
/// save is called.
fn saved(doc: &Document, how: How) -> (&'static str, anyhow::Result<Vec<u8>>) {
    match how {
        How::Rewritten => ("Rewrite", sheaf_doc::save::rewrite(&doc.snapshot())),
        How::Appended => {
            let appended = sheaf_doc::materialize::appended(doc);
            ("Append", appended.map_err(anyhow::Error::from))
        }
    }
}

/// What a save of a corpus file came to.
enum Saved {
    /// Written, and read back as it should be.
    Kept,
    /// The strategy declined, as it does a plain file the same way: an
    /// append to a file whose cross-references are damaged.
    Declined(String),
    /// Written, or not, otherwise than it should be.
    Failed(String),
}

/// What became of one corpus file: None when it isn't encrypted or
/// doesn't open without a password; else how it's encrypted, and the
/// save.
type Outcome = Option<(String, Vec<(&'static str, Saved)>)>;

/// How `crypt` encrypts: its version and revision, and its streams'
/// method (`V2` is RC4, `AESV2` AES-128, `AESV3` AES-256).
fn how(crypt: &Crypt) -> String {
    let get = |key: &[u8]| crypt.dict().get(key).and_then(Object::as_i64).unwrap_or(0);
    let method = crypt
        .state()
        .get_stream_filter()
        .map(|filter| String::from_utf8_lossy(filter.method()).into_owned())
        .unwrap_or_default();
    format!("V{} R{} {method}", get(b"V"), get(b"R"))
}

/// `crypt`'s `/P` and `/ID[0]`.
fn kept(crypt: &Crypt) -> (Option<Object>, Option<&[u8]>) {
    (crypt.dict().get(b"P").ok().cloned(), crypt.first_id())
}

/// What a file decrypts to: its first page's `/Rotate`, that page's
/// content streams' data as stored (None for one that can't be read) and
/// the strings its dictionary holds, and the strings `/Info` holds. The
/// strings are sorted, as a rewrite may order keys anew.
#[derive(Debug, PartialEq)]
struct Decrypted {
    rotate: Rotation,
    content: Vec<Option<Vec<u8>>>,
    page: Vec<Vec<u8>>,
    info: Vec<Vec<u8>>,
}

fn decrypted(src: &Source) -> Decrypted {
    let pdf = src.pdf();
    let read = |obj: &Object| pdf.dereference(obj).ok().map(|(_, obj)| obj.clone());
    let data = |obj: Option<Object>| Some(obj?.as_stream().ok()?.content.clone());
    let (mut content, mut page) = (Vec::new(), Vec::new());
    if let Some(facts) = src.page(0) {
        for &id in facts.contents.iter() {
            match src.object(id).as_deref() {
                // A reference to an array of streams.
                Ok(Object::Array(streams)) => {
                    content.extend(streams.iter().map(|stream| data(read(stream))));
                }
                Ok(stream) => content.push(data(Some(stream.clone()))),
                Err(_) => content.push(None),
            }
        }
        if let Ok(dict) = src.object(facts.id) {
            strings(&dict, &mut page);
        }
    }
    let mut info = Vec::new();
    if let Some(dict) = pdf.trailer().get(b"Info").ok().and_then(read) {
        strings(&dict, &mut info);
    }
    page.sort();
    info.sort();
    Decrypted {
        rotate: src.geom(0).rotate,
        content,
        page,
        info,
    }
}

/// Every string `obj` holds, in its arrays and dictionaries too, but not
/// in what it refers to.
fn strings(obj: &Object, out: &mut Vec<Vec<u8>>) {
    match obj {
        Object::String(text, _) => out.push(text.clone()),
        Object::Array(items) => items.iter().for_each(|item| strings(item, out)),
        Object::Dictionary(dict) => dict.iter().for_each(|(_, v)| strings(v, out)),
        Object::Stream(stream) => stream.dict.iter().for_each(|(_, v)| strings(v, out)),
        _ => {}
    }
}

/// What PDFium shows of a file it opens: its pages, and its first page's
/// characters when it reads them. PDFium orders them, puts white space
/// between them and marks a hyphen ending a line (U+0002) by where they
/// are, which a turn changes: so they're sorted, without the white space,
/// a marked hyphen read as a hyphen.
#[derive(Debug, PartialEq)]
struct Shown {
    pages: u32,
    chars: Option<Vec<char>>,
}

fn shown(bytes: Arc<[u8]>) -> anyhow::Result<Shown> {
    let engine = PdfiumEngine::open_bytes(bytes, None)?;
    let chars = engine.page_chars(PageIndex(0)).ok().map(|chars| {
        let mut chars: Vec<char> = chars
            .into_iter()
            .map(|c| if c.ch == '\u{2}' { '-' } else { c.ch })
            .filter(|c| !c.is_whitespace())
            .collect();
        chars.sort_unstable();
        chars
    });
    Ok(Shown {
        pages: engine.page_count(),
        chars,
    })
}

/// A corpus file as read before its first page was turned: how it's
/// encrypted, how many pages it has, what it decrypts to, and what PDFium
/// shows of it when PDFium opens it.
struct Before<'a> {
    path: &'a Path,
    crypt: Arc<Crypt>,
    pages: u32,
    decrypted: Decrypted,
    shown: Option<Shown>,
}

/// `bytes` saved from the file `was` is of, read back: encrypted as it
/// was, with its pages, the first turned a quarter and all decrypting to
/// what they did, and shown by PDFium as the file was when PDFium opened
/// it.
fn check(was: &Before, bytes: Vec<u8>) -> Saved {
    let bytes: Arc<[u8]> = bytes.into();
    let src = match Store::new().open(bytes.clone(), BlobKind::file(was.path, None)) {
        Ok(src) => src,
        Err(e) => return Saved::Failed(format!("doesn't reopen: {e}")),
    };
    let Some(crypt) = src.crypt() else {
        return Saved::Failed("not encrypted".into());
    };
    let (now, then) = (kept(crypt), kept(&was.crypt));
    if now.0 != then.0 {
        return Saved::Failed(format!("/P {:?}, was {:?}", now.0, then.0));
    }
    if then.1.is_some() && now.1 != then.1 {
        return Saved::Failed("/ID[0] changed".into());
    }
    if src.page_count() != was.pages {
        return Saved::Failed(format!("{} pages, was {}", src.page_count(), was.pages));
    }
    let got = decrypted(&src);
    let turned = was.decrypted.rotate.plus(Rotation::R90);
    if got.rotate != turned {
        return Saved::Failed(format!("page 1 at {:?}, not {turned:?}", got.rotate));
    }
    let differs = [
        ("page 1's content", got.content != was.decrypted.content),
        ("page 1's strings", got.page != was.decrypted.page),
        ("/Info's strings", got.info != was.decrypted.info),
    ];
    if let Some((what, _)) = differs.iter().find(|(_, differs)| *differs) {
        return Saved::Failed(format!("{what} decrypt otherwise"));
    }
    if let Some(want) = &was.shown {
        match shown(bytes) {
            Ok(got) if got.pages != want.pages => {
                return Saved::Failed(format!("PDFium: {} pages", got.pages));
            }
            Ok(got) if want.chars.is_some() && got.chars != want.chars => {
                return Saved::Failed("PDFium: page 1 reads otherwise".into());
            }
            Ok(_) => {}
            Err(e) => return Saved::Failed(format!("PDFium: {e:#}")),
        }
    }
    Saved::Kept
}

/// The file at `path`, its first page turned, saved `save`.
fn save_encrypted(path: &Path, save: How) -> Outcome {
    let store = Store::new();
    let (primary, stamp) = store.read_file(path, None).ok()?;
    let src = primary.clone();
    let was = Before {
        path,
        crypt: src.crypt()?.clone(),
        pages: src.page_count(),
        decrypted: decrypted(&src),
        shown: shown(src.bytes().clone()).ok(),
    };
    let came = Came::Read {
        path: path.into(),
        stamp,
    };
    let mut doc = sheaf_doc::open::start(
        store,
        primary,
        sheaf_doc::store::file_name(path),
        Some(came),
        Box::new(FixedCount(2)),
    );
    let opened = doc
        .ready(sheaf_doc::open::finish(&src, &NoProgress))
        .is_ok();
    let turned = match doc.page(0) {
        Some(first) if opened => {
            let turn = rotate(&[first], Turn::Quarter);
            doc.commit_ops(vec![turn], "turn").is_ok()
        }
        _ => false,
    };
    if !turned {
        let failed = Saved::Failed("doesn't open to edit".into());
        return Some((how(&was.crypt), vec![("Open", failed)]));
    }
    let (strategy, written) = saved(&doc, save);
    let saved = match written {
        Ok(bytes) => check(&was, bytes),
        Err(e) if save == How::Appended && !src.base_sound() => Saved::Declined(format!("{e:#}")),
        Err(e) => Saved::Failed(format!("doesn't save: {e:#}")),
    };
    Some((how(&was.crypt), vec![(strategy, saved)]))
}

sheaf_doc::both_ways!(
    encrypted_corpus_files_save_encrypted_by_both_strategies,
    encrypted_corpus_files_save_encrypted_by_both_strategies_appended: save_encrypted_corpus
);

fn save_encrypted_corpus(save: How) {
    let Some(corpus) = Corpus::or_skip() else {
        return;
    };
    let entries = corpus.forms().unwrap();
    let outcomes = each_entry(&entries, |entry| save_encrypted(&corpus.path(entry), save));
    let mut by_kind: BTreeMap<String, usize> = BTreeMap::new();
    let mut by_strategy: BTreeMap<(&str, &str), usize> = BTreeMap::new();
    let mut failed = Vec::new();
    for (entry, outcome) in entries.iter().zip(outcomes) {
        let Some((kind, saves)) = outcome else {
            continue;
        };
        *by_kind.entry(kind).or_default() += 1;
        for (strategy, saved) in saves {
            let what = match saved {
                Saved::Kept => "kept",
                Saved::Declined(why) => {
                    println!("{}: {strategy} declined: {why}", entry.path);
                    "declined"
                }
                Saved::Failed(why) => {
                    failed.push(format!("{}: {strategy}: {why}", entry.path));
                    "failed"
                }
            };
            *by_strategy.entry((strategy, what)).or_default() += 1;
        }
    }
    println!(
        "{} files, {} encrypted and open with no password: {by_kind:?}",
        entries.len(),
        by_kind.values().sum::<usize>()
    );
    println!("saves: {by_strategy:?}");
    assert!(failed.is_empty(), "{failed:#?}");
}
