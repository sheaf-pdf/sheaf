//! The new writer over Greg's corpus of real forms (M10.5 slice 9):
//! - an identity Rewrite keeps every surviving primary object at its
//!   number, the same object there, and copies each it left alone byte for
//!   byte (when the file's cross-references are sound);
//! - the same document written twice, and from two threads at once, is
//!   the same bytes;
//! - with a page deleted, every form writes, and the form check finds no
//!   widget, field or font the file's own check didn't name, but values
//!   with no appearance and what save policy decides (asked about first);
//! - with every page duplicated, every file writes; and the pages of every
//!   fourth file (and of those that once failed) inserted twice into the
//!   1040 write too: a copy never brings its file's catalog or form along,
//!   nor stops on a signature it can't decrypt;
//! - `pool/safedocs/2932/2932995.pdf`, whose first page's logo lopdf's
//!   writer recolours, renders the same written.
//!
//! The identity Rewrite's counts are asserted against the baseline
//! ([`IDENTITY`]), when the corpus is the one they were counted on (its
//! manifest's length): a later step that moves one says why in the plan's
//! slice notes, and moves the baseline.
//!
//! Skips when there's no corpus (see `sheaf_doc::forms::corpus`). Needs
//! libpdfium (`scripts/fetch-pdfium.sh`) for the logo, like the app.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use lopdf::{LazyPdf, Object, ObjectId};
use sheaf_doc::Document;
use sheaf_doc::forms::corpus::{Corpus, Entry, each_entry};
use sheaf_doc::forms::{Finding, Report};
use sheaf_doc::materialize::{
    Output, Profile, SaveInput, SaveTarget, materialize, plan as save_plan,
};
use sheaf_doc::source::Limits;
use sheaf_doc::tx::fixture::{delete, duplicate, file};
use sheaf_edit::{Bring, FormBring, Op, OutlineBring, Rotation};
use sheaf_image::raster::PixelFormat;
use sheaf_ops::NoProgress;
use sheaf_render::PageIndex;
use sheaf_render::pdfium::PdfiumEngine;
use sheaf_render::{Job, Renderer};

/// `doc` Rewritten by the new writer, as a save to another file plans it.
fn written(doc: &Document) -> anyhow::Result<Output> {
    let input = SaveInput::detached(&doc.snapshot());
    let target = SaveTarget {
        path: Path::new("out.pdf").into(),
        same_file: false,
        profile: Profile::default(),
        stamp: None,
    };
    let plan = save_plan(&input, &target);
    Ok(materialize(&input, &plan, target, &NoProgress)?)
}

/// The corpus file at `path`, opened as the app opens it; None when it
/// doesn't open without a password.
fn opened(path: &Path) -> Option<Document> {
    sheaf_doc::open::file(path, None).ok().map(|o| o.doc)
}

/// Whether `a` and `b` are the same object as values: strings by their
/// bytes, whatever their form.
fn same(a: &Object, b: &Object) -> bool {
    match (a, b) {
        (Object::String(x, _), Object::String(y, _)) => x == y,
        (Object::Array(x), Object::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(x, y)| same(x, y))
        }
        (Object::Dictionary(x), Object::Dictionary(y)) => {
            x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_ok_and(|w| same(v, w)))
        }
        (Object::Stream(x), Object::Stream(y)) => {
            let dict = |d: &lopdf::Dictionary| {
                let mut d = d.clone();
                d.remove(b"Length");
                Object::Dictionary(d)
            };
            same(&dict(&x.dict), &dict(&y.dict)) && x.content == y.content
        }
        (x, y) => x == y,
    }
}

/// What an object is, for telling whether a number still names it: its
/// kind and, for a dictionary, its `/Type` and `/Subtype`.
fn kind(obj: &Object) -> (u8, Option<Vec<u8>>, Option<Vec<u8>>) {
    let dict = match obj {
        Object::Dictionary(d) => Some(d),
        Object::Stream(s) => Some(&s.dict),
        _ => None,
    };
    let name =
        |k: &[u8]| -> Option<Vec<u8>> { dict?.get(k).ok()?.as_name().ok().map(<[u8]>::to_vec) };
    let tag = match obj {
        Object::Dictionary(_) => 0,
        Object::Stream(_) => 1,
        Object::Array(_) => 2,
        _ => 3,
    };
    (tag, name(b"Type"), name(b"Subtype"))
}

/// What an identity Rewrite of one file came to.
#[derive(Default)]
struct Identity {
    /// Primary objects still in the file, and those copied as they were.
    survived: usize,
    raw: usize,
    /// Survivors `keep_only` changed in place.
    pruned: usize,
    /// What's wrong with it.
    failed: Vec<String>,
}

fn identity(entry: &Entry, corpus: &Corpus) -> Option<Identity> {
    let path = corpus.path(entry);
    let doc = opened(&path)?;
    let mut out = Identity::default();
    let written = match written(&doc) {
        Ok(w) => w,
        Err(e) => {
            out.failed.push(format!("not written: {e:#}"));
            return Some(out);
        }
    };
    let src = doc.primary();
    let before = src.pdf();
    let after = match LazyPdf::open(
        Arc::from(written.bytes.as_slice()),
        src.password(),
        &Limits::DEFAULT.reader,
    ) {
        Ok(a) => a,
        Err(e) => {
            out.failed.push(format!("doesn't read back: {e}"));
            return Some(out);
        }
    };
    let mut listed: HashSet<ObjectId> = written.record.map.keys.values().copied().collect();
    // The encryption dictionary is written from the file's keys.
    listed.extend(src.crypt().map(|c| c.at()));
    let now: HashSet<ObjectId> = after.ids().collect();
    let sound = src.base_sound();
    for id in before.ids() {
        if !now.contains(&id) || listed.contains(&id) {
            continue;
        }
        let (Ok(was), Ok(is)) = (before.get(id), after.get(id)) else {
            continue;
        };
        out.survived += 1;
        if kind(was) != kind(is) {
            out.failed
                .push(format!("{} {} is another object", id.0, id.1));
            continue;
        }
        if !same(was, is) {
            out.pruned += 1;
            continue;
        }
        let spans = (before.raw_span_whole(id), after.raw_span(id));
        if let (Some(a), Some(b)) = spans {
            if src.bytes()[a] == written.bytes[b] {
                out.raw += 1;
            } else if sound {
                out.failed
                    .push(format!("{} {} was left alone and written anew", id.0, id.1));
            }
        }
    }
    Some(out)
}

/// The identity Rewrite's counts over the corpus of 4,392 files (slice
/// 9): files written, primary objects surviving at their numbers but those
/// the emitter writes (whose numbers the record lists), those copied as
/// they were, and those `keep_only` changed. Lower counts (higher for the
/// last) are a regression unless the slice notes say why they're right.
/// Slice 9's review moved them from 2,270,468 and 919,554: 746 objects
/// (75 of them copied raw) that the unbuilt fast paths copied as they
/// were, where the built path writes them otherwise, are written anew:
/// fields with an empty `/Kids` (614 of 0982696's, 11 of 1210120's), and
/// an outline root with no `/Type` in each of 121 other files. Slice 18's
/// wrap-up moved them from 2,269,722, 919,479 and 436 (Greg, 2026-10-22):
/// 0982696's 19 nodes whose `/Kids` another node holds aren't written, 3
/// of them written changed before; and 0092321's fields no longer copy
/// its two catalogs as kids, nor the usage rights (`/Perms`, `/UR3`)
/// only those copies reached, which a Rewrite's own catalog drops: 15
/// objects, 14 of them copied as they were, 1 changed.
const IDENTITY: Baseline = Baseline {
    manifest: 4392,
    files: 4389,
    survived: 2_269_688,
    raw: 919_465,
    changed: 432,
};

struct Baseline {
    manifest: usize,
    files: usize,
    survived: usize,
    raw: usize,
    changed: usize,
}

#[test]
fn an_identity_rewrite_keeps_every_number_and_copies_what_it_left_alone() {
    let Some(corpus) = Corpus::or_skip() else {
        return;
    };
    let entries = corpus.forms().unwrap();
    let outcomes = each_entry(&entries, |entry| identity(entry, &corpus));
    let (mut files, mut survived, mut raw, mut pruned) = (0, 0, 0, 0);
    let mut failed = BTreeMap::new();
    for (entry, outcome) in entries.iter().zip(outcomes) {
        let Some(o) = outcome else {
            continue;
        };
        files += 1;
        (survived, raw, pruned) = (survived + o.survived, raw + o.raw, pruned + o.pruned);
        if !o.failed.is_empty() {
            failed.insert(entry.path.clone(), o.failed);
        }
    }
    println!(
        "{files} of {} files written; {survived} primary objects survived, {raw} copied as \
         they were, {pruned} changed by keep_only; {} files failed",
        entries.len(),
        failed.len()
    );
    for (path, why) in &failed {
        println!("{path}: {}", why[..why.len().min(3)].join("; "));
    }
    assert!(failed.is_empty());
    if entries.len() == IDENTITY.manifest {
        let b = &IDENTITY;
        assert!(files >= b.files, "{files} files written, from {}", b.files);
        assert!(
            survived >= b.survived,
            "{survived} survived, from {}",
            b.survived
        );
        assert!(raw >= b.raw, "{raw} copied as they were, from {}", b.raw);
        assert!(pruned <= b.changed, "{pruned} changed, from {}", b.changed);
    }
}

#[test]
fn the_same_document_writes_the_same_bytes_twice_and_from_two_threads() {
    let Some(corpus) = Corpus::or_skip() else {
        return;
    };
    let entries = corpus.forms().unwrap();
    let outcomes = each_entry(&entries, |entry| {
        let doc = opened(&corpus.path(entry))?;
        let first = written(&doc).ok()?.bytes;
        let again = written(&doc).ok()?.bytes;
        let [a, b] = std::thread::scope(|s| {
            let one = s.spawn(|| written(&doc).ok().map(|o| o.bytes));
            let two = s.spawn(|| written(&doc).ok().map(|o| o.bytes));
            [one.join().unwrap(), two.join().unwrap()]
        });
        let alike = again == first && a.as_ref() == Some(&first) && b.as_ref() == Some(&first);
        Some(alike)
    });
    let mut files = 0;
    let mut differ = Vec::new();
    for (entry, alike) in entries.iter().zip(outcomes) {
        let Some(alike) = alike else {
            continue;
        };
        files += 1;
        if !alike {
            differ.push(entry.path.clone());
        }
    }
    println!("{files} files written four times, {} differ", differ.len());
    assert_eq!(differ, Vec::<String>::new());
}

/// What each of `report`'s findings names, by kind: its widgets', fields'
/// or fonts' names, each on its own; a damaged tree, nothing.
fn named(report: &Report) -> Vec<(&'static str, Option<String>)> {
    let mut out = Vec::new();
    for f in &report.findings {
        let names: Vec<Option<String>> = match f {
            Finding::Orphans(n)
            | Finding::OffPage(n)
            | Finding::SharedWidgets(n)
            | Finding::WrongPage(n)
            | Finding::NullPage(n)
            | Finding::WrongParent(n)
            | Finding::DuplicateNames(n)
            | Finding::NoAppearance(n) => n.iter().cloned().map(Some).collect(),
            Finding::MissingFonts(fonts) => fonts.iter().map(|(f, _)| Some(f.clone())).collect(),
            _ => vec![None],
        };
        out.extend(names.into_iter().map(|n| (f.kind(), n)));
    }
    out
}

/// Whether `finding`'s kind is one save policy decides, and asks about
/// first, or a value with no appearance, which a page delete leaves where
/// the chosen button's widget went: what a written form may have new.
fn allowed(kind: &str) -> bool {
    let allowed = [
        Finding::NoAppearance(Vec::new()),
        Finding::EarlierValues {
            revision: 0,
            names: Vec::new(),
        },
        Finding::XfaMismatch {
            xfa_only: Vec::new(),
            acroform_only: Vec::new(),
        },
        Finding::StaleSignature {
            what: String::new(),
            covers: 0,
            size: 0,
        },
        Finding::StaleSigFlags { flags: 0, why: "" },
    ];
    allowed.iter().any(|f| f.kind() == kind)
}

/// What a Rewrite with the first page deleted came to: None when the file
/// doesn't open or has one page; else whether it wrote, its form finding
/// nothing the file's didn't (by name, or by kind where the file's tree is
/// damaged), and why not; and how many values it left with no
/// appearance.
fn without_the_first_page(entry: &Entry, corpus: &Corpus) -> Option<Result<usize, String>> {
    let mut doc = opened(&corpus.path(entry))?;
    if doc.state().page_count() < 2 {
        return None;
    }
    let first = doc.page(0).unwrap();
    doc.commit_ops(vec![delete(&[first])], "delete").ok()?;
    let out = match written(&doc) {
        Ok(out) => out,
        Err(e) => return Some(Err(format!("{e:#}"))),
    };
    let src = &doc.primary();
    let checked = |bytes: &[u8]| sheaf_doc::forms::check(bytes, src.password());
    let (was, now) = match (checked(src.bytes()), checked(&out.bytes)) {
        (Ok(was), Ok(now)) => (was, now),
        (_, Err(e)) => return Some(Err(format!("the written form doesn't check: {e:#}"))),
        (Err(_), Ok(now)) => (Report::default(), now),
    };
    let damaged = was
        .findings
        .iter()
        .any(|f| matches!(f, Finding::Damaged(_)));
    let had = named(&was);
    let mut unseen = 0;
    let mut new = Vec::new();
    for (kind, name) in named(&now) {
        let had = had
            .iter()
            .any(|(k, n)| *k == kind && (damaged || *n == name));
        match (had, allowed(kind)) {
            (true, _) => {}
            (false, true) => unseen += 1,
            (false, false) => new.push(format!("{kind}: {name:?}")),
        }
    }
    Some(match new.is_empty() {
        true => Ok(unseen),
        false => Err(new[..new.len().min(5)].join("; ")),
    })
}

#[test]
fn every_form_writes_without_a_page_its_form_checked() {
    let Some(corpus) = Corpus::or_skip() else {
        return;
    };
    let entries = corpus.forms().unwrap();
    let outcomes = each_entry(&entries, |entry| without_the_first_page(entry, &corpus));
    let (mut files, mut unseen) = (0, 0);
    let mut failed = BTreeMap::new();
    for (entry, outcome) in entries.iter().zip(outcomes) {
        let Some(outcome) = outcome else {
            continue;
        };
        files += 1;
        match outcome {
            Ok(n) => unseen += n,
            Err(e) => {
                failed.insert(entry.path.clone(), e);
            }
        }
    }
    println!(
        "{files} forms written without their first page, {} failed; {unseen} new findings \
         save policy decides or of values with no appearance",
        failed.len()
    );
    for (path, why) in &failed {
        println!("{path}: {why}");
    }
    assert!(failed.is_empty());
}

/// Whether every outcome wrote, printing each that didn't: `what` says
/// what was written.
fn all_written(entries: &[Entry], outcomes: Vec<Option<Result<(), String>>>, what: &str) {
    let mut files = 0;
    let mut failed = BTreeMap::new();
    for (entry, outcome) in entries.iter().zip(outcomes) {
        let Some(outcome) = outcome else {
            continue;
        };
        files += 1;
        if let Err(e) = outcome {
            failed.insert(entry.path.clone(), e);
        }
    }
    println!("{files} {what}, {} failed", failed.len());
    for (path, why) in &failed {
        println!("{path}: {why}");
    }
    assert!(failed.is_empty());
}

/// Every file with each of its pages duplicated writes: a duplicate's
/// copies follow no reference into the file's catalog or form (a
/// signature's `/Reference /Data`, a stray key naming the form), whose
/// copies held the form's widgets on no page.
#[test]
fn every_file_writes_with_its_pages_duplicated() {
    let Some(corpus) = Corpus::or_skip() else {
        return;
    };
    let entries = corpus.forms().unwrap();
    let outcomes = each_entry(&entries, |entry| {
        let mut doc = opened(&corpus.path(entry))?;
        let ids: Vec<_> = doc.state().order().iter().copied().collect();
        doc.commit_ops(vec![duplicate(&ids)], "dup").ok()?;
        Some(written(&doc).map(drop).map_err(|e| format!("{e:#}")))
    });
    all_written(
        &entries,
        outcomes,
        "files written with their pages duplicated",
    );
}

/// Files whose pages, inserted, once stopped the write: a signature
/// whose `/Reference /Data` named the catalog, and an encrypted file's
/// signature with no `/Type`, whose `/Contents` isn't encrypted.
const ONCE_FAILED: [&str; 5] = [
    "forms/safedocs/0110336.pdf",
    "forms/safedocs/0218989.pdf",
    "forms/safedocs/0636951.pdf",
    "forms/safedocs/1263925.pdf",
    "forms/safedocs/1296627.pdf",
];

/// The pages of every fourth file, and of [`ONCE_FAILED`]'s, inserted
/// twice at the end of the 1040, write.
#[test]
fn every_fourth_file_s_pages_inserted_twice_into_a_form_write() {
    let Some(corpus) = Corpus::or_skip() else {
        return;
    };
    let base = corpus.root.join("forms/irs/f1040.pdf");
    if !base.is_file() {
        eprintln!("skipped: no {}", base.display());
        return;
    }
    let entries = corpus.forms().unwrap();
    let picked: Vec<Entry> = (entries.iter().enumerate())
        .filter(|(i, e)| i % 4 == 0 || ONCE_FAILED.contains(&e.path.as_str()))
        .map(|(_, e)| e.clone())
        .collect();
    assert!(
        picked.iter().any(|e| e.path == ONCE_FAILED[3]),
        "1263925 is in"
    );
    let outcomes = each_entry(&picked, |entry| {
        let mut doc = opened(&base).expect("the 1040 opens");
        let path = corpus.path(entry);
        let (src, _) = doc.store().read_file(&path, None).ok()?;
        for _ in 0..2 {
            // Its pages and their form, without its marks and bookmarks.
            let insert = Op::InsertPages {
                from: file(&src, &sheaf_doc::store::file_name(&path)),
                pages: None,
                before: None,
                bring: Bring {
                    marks: false,
                    form: FormBring::New,
                    outline: OutlineBring::None,
                },
                locals: Vec::new(),
            };
            doc.commit_ops(vec![insert], "insert").ok()?;
        }
        Some(written(&doc).map(drop).map_err(|e| format!("{e:#}")))
    });
    all_written(
        &picked,
        outcomes,
        "files' pages inserted twice into the 1040",
    );
}

#[test]
fn the_2932995_logo_renders_as_it_did() {
    let Some(corpus) = Corpus::or_skip() else {
        return;
    };
    let path = corpus.root.join("pool/safedocs/2932/2932995.pdf");
    if !path.is_file() {
        eprintln!("skipped: no {}", path.display());
        return;
    }
    let doc = opened(&path).expect("it opens");
    let out = written(&doc).unwrap();
    let job = Job::Page {
        width: 600,
        rotation: Rotation::R0,
    };
    let render = |bytes: Vec<u8>| {
        let engine = PdfiumEngine::open_bytes(bytes.into(), None).unwrap();
        engine.render(PageIndex(0), job, PixelFormat::Bgra).unwrap()
    };
    let before = render(std::fs::read(&path).unwrap());
    let after = render(out.bytes);
    assert_eq!((before.width, before.height), (after.width, after.height));
    let differ = (before.data.iter().zip(&after.data))
        .filter(|(a, b)| a != b)
        .count();
    println!("{differ} of {} bytes differ", before.data.len());
    assert_eq!(differ, 0);
}
