//! The new writer over Greg's corpus of real forms (M10.5 slice 9):
//! - an identity Rewrite keeps every surviving primary object at its
//!   number, the same object there, and copies each it left alone byte for
//!   byte (when the file's cross-references are sound); an object
//!   stream's number holds one of the Rewrite's own, or nothing;
//! - the same document written twice, and from two threads at once, is
//!   the same bytes;
//! - with a page deleted, every form writes, and the form check finds no
//!   widget, field or font the file's own check didn't name, but values
//!   with no appearance and what save policy decides (asked about first);
//! - with every page duplicated, every file writes; and the pages of every
//!   fourth file (and of those that once failed) inserted twice into the
//!   1040 write too: a copy never brings its file's catalog or form along,
//!   nor stops on a signature it can't decrypt;
//! - `pool/safedocs/2932/2932995.pdf`, whose first page's logo an older
//!   writer recoloured, renders the same written;
//! - a Rewrite under each profile that renumbers (dense, a clean copy's,
//!   print's) writes every form so it reads back, no copy refused, the
//!   same bytes twice under dense (an AES file the same objects), and
//!   counts the primary objects whose bytes are their source's, their
//!   header and references aside (M18 slice 14).
//!
//! The identity Rewrite's counts are asserted against the baseline
//! ([`IDENTITY`]), and the renumbered ones against theirs
//! ([`RENUMBERED`]), when the corpus is the one they were counted on (its
//! manifest's length): a later step that moves one says why in the plan's
//! slice notes, and moves the baseline.
//!
//! `SHEAF_RENUMBER_SUMS=<file>` has the renumbered test write a line per
//! file and profile, the default's too, to `<file>`: what tells a change
//! of the bytes written from one of the objects they hold
//! ([`renumbered`]).
//!
//! Skips when there's no corpus (see `sheaf_doc::forms::corpus`). Needs
//! libpdfium (`scripts/fetch-pdfium.sh`) for the logo, like the app.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use sheaf_doc::forms::corpus::{Corpus, Entry, each_entry};
use sheaf_doc::forms::{Finding, Report};
use sheaf_doc::materialize::{
    Encryption, Numbering, ObjKey, Output, Private, Profile, SaveInput, SaveTarget, Unheld,
    materialize, plan as save_plan,
};
use sheaf_doc::pdf::{Dictionary, Document as Pdf, LazyPdf, Object, ObjectId, StreamExt, Value};
use sheaf_doc::source::Limits;
use sheaf_doc::tx::fixture::{delete, duplicate, file};
use sheaf_doc::{Document, Source};
use sheaf_edit::{Bring, FormBring, Op, OutlineBring, Rotation};
use sheaf_image::raster::PixelFormat;
use sheaf_ops::NoProgress;
use sheaf_render::PageIndex;
use sheaf_render::pdfium::PdfiumEngine;
use sheaf_render::{Job, Renderer};
use sheaf_util::{Checksum, checksum_hex};

/// `doc` Rewritten by the new writer, as a save to another file plans it.
fn written(doc: &Document) -> anyhow::Result<Output> {
    written_as(doc, Profile::default())
}

/// [`written`] under `profile`.
fn written_as(doc: &Document, profile: Profile) -> anyhow::Result<Output> {
    let input = SaveInput::detached(&doc.snapshot());
    let target = SaveTarget {
        path: Path::new("out.pdf").into(),
        same_file: false,
        profile,
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
    match (a.value(), b.value()) {
        (Value::String(x, _), Value::String(y, _)) => x.as_bytes() == y.as_bytes(),
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(x, y)| same(x, y))
        }
        (Value::Dict(x), Value::Dict(y)) => {
            x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| same(v, w)))
        }
        (Value::Stream(x), Value::Stream(y)) => {
            let dict = |d: &Dictionary| {
                let mut d = d.clone();
                d.remove(b"Length");
                Object::from(d)
            };
            same(&dict(&x.dict), &dict(&y.dict)) && x.raw_bytes() == y.raw_bytes()
        }
        _ => a.eq_value(b),
    }
}

/// What an object is, for telling whether a number still names it: its
/// kind and, for a dictionary, its `/Type` and `/Subtype`.
fn kind(obj: &Object) -> (u8, Option<Vec<u8>>, Option<Vec<u8>>) {
    let dict = obj.as_dict();
    let name = |k: &[u8]| -> Option<Vec<u8>> { dict?.get(k)?.as_name().map(<[u8]>::to_vec) };
    let tag = match obj.value() {
        Value::Dict(_) => 0,
        Value::Stream(_) => 1,
        Value::Array(_) => 2,
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
        Arc::<[u8]>::from(written.bytes.as_slice()),
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
        // An object stream held the file's objects: a packed Rewrite gives
        // its number to an object stream of its own, or to nothing.
        let objstm =
            |obj: &Object| obj.as_dict().and_then(Dictionary::type_name) == Some(b"ObjStm");
        if objstm(was) {
            if !objstm(is) {
                out.failed.push(format!("{id} was an object stream"));
            }
            continue;
        }
        out.survived += 1;
        if kind(was) != kind(is) {
            out.failed.push(format!("{id} is another object"));
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
                    .push(format!("{id} was left alone and written anew"));
            }
        } else if let (Some((_, a)), Some((_, b))) =
            (before.member_bytes(id), after.member_bytes(id))
            && a == b
        {
            out.raw += 1;
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
/// objects, 14 of them copied as they were, 1 changed. M18 slice 8 moved
/// them to 4,388, 2,267,675, 921,586 and 431: 4422502's damaged trailer
/// has no `/Root` under `pdf-structure` (closing F29), incremental `type
/// 0` free entries (0150632, 3170869, …) drop freed objects the old reader
/// kept,
/// and 2,121 streams with multi-EOL whitespace before `endstream` copy
/// verbatim. M18 slice 9 moved `raw` to 2,246,369 (99.1% of surviving
/// primary objects): unchanged object-stream members copy into new
/// containers as their decoded bytes without re-serialization.
const IDENTITY: Baseline = Baseline {
    manifest: 4392,
    files: 4388,
    survived: 2_267_675,
    raw: 2_246_369,
    changed: 431,
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
        let alike = if doc.primary().crypt().is_some_and(|c| c.is_aes()) {
            // Production AES saves draw fresh 16-byte OS CSPRNG IVs per
            // ISO 32000-2 §7.6.3.2 & §7.6.3.3; all four writes must decrypt
            // to semantically equal object graphs.
            let (Some(a), Some(b)) = (a, b) else {
                return Some(false);
            };
            let Ok(base_pdf) = Pdf::load_mem(&first) else {
                return Some(false);
            };
            let non_xref = |obj: &Object| {
                !obj.as_stream()
                    .is_some_and(|s| s.dict.type_name() == Some(b"XRef"))
            };
            let base_objs: std::collections::BTreeMap<_, _> = base_pdf
                .objects
                .iter()
                .filter(|(_, o)| non_xref(o))
                .collect();
            [&again, &a, &b].into_iter().all(|bytes| {
                Pdf::load_mem(bytes).is_ok_and(|pdf| {
                    let objs: Vec<_> = pdf.objects.iter().filter(|(_, o)| non_xref(o)).collect();
                    objs.len() == base_objs.len()
                        && objs
                            .into_iter()
                            .all(|(id, obj)| base_objs.get(id).is_some_and(|o| o.eq_value(obj)))
                })
            })
        } else {
            again == first && a.as_ref() == Some(&first) && b.as_ref() == Some(&first)
        };
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

/// An export's profile with nothing else asked: from 1, encryption and
/// sheaf's private data kept (an extract's, a flattened copy's).
const DENSE: Profile = Profile {
    encryption: Encryption::Keep,
    numbering: Numbering::Dense,
    private: Private::Keep,
    unheld: Unheld::Keep,
};

/// The profiles that renumber, written as identity exports (no
/// transform): dense; a clean copy's, without sheaf's private data; and
/// print's, in the clear too.
const RENUMBERING: [(&str, Profile); 3] = [
    ("dense", DENSE),
    (
        "clean",
        Profile {
            private: Private::Strip,
            ..DENSE
        },
    ),
    (
        "print",
        Profile {
            encryption: Encryption::None,
            private: Private::Scrub,
            ..DENSE
        },
    ),
];

/// What the writes under one profile came to: files written, primary
/// objects still in them, those whose bytes are their source's (but
/// their header and references, [`shaped`]), and the bytes written, but
/// AES files' (`aes_bytes`), whose length varies from write to write:
/// a string's ciphertext, as fresh IVs make it, is written with as many
/// bytes escaped as it happens to hold.
#[derive(Debug, Clone, Copy, Default)]
struct Tally {
    files: usize,
    survived: usize,
    copied: usize,
    bytes: usize,
    aes_bytes: usize,
}

/// Whether `written` is encrypted with AES: its strings and streams
/// under IVs drawn as it was written.
fn aes(written: &Output) -> bool {
    written.record.crypt.as_ref().is_some_and(|c| c.is_aes())
}

/// The renumbered Rewrites' counts over the corpus of 4,392 files (M18
/// slice 14), in [`RENUMBERING`]'s order, as [`Tally`] counts them: AES
/// files' bytes aren't held. Lower counts, or more bytes, are a
/// regression unless the slice notes say why they're right. First counted
/// before a renumbered write copied anything: 1,399,877 under dense (61%),
/// objects the writer wrote anew as their source had them, in 1,706,068,369
/// bytes (print 1,399,105 and 1,932,696,841). Slice 14 moved them to
/// 2,201,569 (96.0%) when a renumbered write came to copy what it left
/// alone, its header and moved references written anew in place. What
/// isn't copied is an object at an offset whose number moves under RC4 or
/// AES-128, whose strings its number keys (73,876), a stream whose
/// `/Length` is a reference (8,692 in the clear), an unsound file's objects
/// (8,094), and what the Rewrite changed. The bytes grew by 0.86%: a copy at
/// an offset is written on its own, where written anew it was packed into
/// an object stream. Print copies 51,694 fewer than dense: it writes an
/// encrypted file's objects in the clear, and their bytes as read are
/// ciphertext. M10 slice 9 moved them to 2,293,342 survived and 2,201,676
/// copied in 1,720,675,424 bytes (print 2,149,982 and 1,947,227,562): a
/// field whose own dictionary is a widget beside kid widgets (36 fields in
/// 34 forms) is copied with its kids as its file has them, where it was
/// written apart from that widget, each anew, packed into an object
/// stream: 107 objects more copied, 4,865 bytes more (137 in 32 of them,
/// 488 and -7 in the other two).
const RENUMBERED: Renumbering = Renumbering {
    manifest: 4392,
    profiles: [
        Held {
            files: 4389,
            survived: 2_293_342,
            copied: 2_201_676,
            bytes: 1_720_675_424,
        },
        Held {
            files: 4389,
            survived: 2_293_342,
            copied: 2_201_676,
            bytes: 1_720_675_424,
        },
        Held {
            files: 4389,
            survived: 2_293_342,
            copied: 2_149_982,
            bytes: 1_947_227_562,
        },
    ],
};

struct Renumbering {
    manifest: usize,
    profiles: [Held; 3],
}

/// What [`RENUMBERED`] holds of a profile's [`Tally`].
struct Held {
    files: usize,
    survived: usize,
    copied: usize,
    bytes: usize,
}

/// Whether `b` is PDF whitespace (ISO 32000-1 7.2.2).
fn white(b: u8) -> bool {
    matches!(b, b'\0' | b'\t' | b'\n' | b'\x0c' | b'\r' | b' ')
}

/// Whether `b` is a regular character: neither whitespace nor a
/// delimiter.
fn regular(b: u8) -> bool {
    !white(b) && !b"()<>[]{}/%".contains(&b)
}

/// The end of the run of `bytes` from `at` that `pick` takes, where it
/// takes one.
fn run(bytes: &[u8], at: usize, pick: impl Fn(u8) -> bool) -> Option<usize> {
    let n = bytes[at..].iter().take_while(|&&b| pick(b)).count();
    (n > 0).then_some(at + n)
}

/// The end of the reference, `n g R`, at `at` in `bytes`, where one is.
fn reference(bytes: &[u8], at: usize) -> Option<usize> {
    let at = run(bytes, at, |b| b.is_ascii_digit())?;
    let at = run(bytes, at, white)?;
    let at = run(bytes, at, |b| b.is_ascii_digit())?;
    let at = run(bytes, at, white)?;
    let ends = |at: usize| bytes.get(at).is_none_or(|&b| !regular(b));
    (bytes.get(at) == Some(&b'R') && ends(at + 1)).then_some(at + 1)
}

/// `bytes`, an object's as a file has them, as a copy under a
/// renumbering keeps them: its header, `n g obj` and the whitespace after
/// it, cut where `header` says it has one, and each reference and each
/// `null` one token, so that an object whose references a renumbering
/// rewrote, `null` where they name nothing now, is its source's bytes.
fn shaped(bytes: &[u8], header: bool) -> Vec<u8> {
    let mut i = 0;
    if header {
        let at = run(bytes, 0, white).unwrap_or(0);
        let body = (run(bytes, at, |b| b.is_ascii_digit()))
            .and_then(|at| run(bytes, at, white))
            .and_then(|at| run(bytes, at, |b| b.is_ascii_digit()))
            .and_then(|at| run(bytes, at, white))
            .filter(|&at| bytes[at..].starts_with(b"obj"))
            .map(|at| at + 3);
        if let Some(at) = body {
            i = run(bytes, at, white).unwrap_or(at);
        }
    }
    let start = i;
    let mut out = Vec::with_capacity(bytes.len() - i);
    while i < bytes.len() {
        // A token starts here: not within a number, a name or a word.
        if i == start || (!regular(bytes[i - 1]) && bytes[i - 1] != b'/') {
            let null = || {
                let ends = bytes.get(i + 4).is_none_or(|&b| !regular(b));
                (bytes[i..].starts_with(b"null") && ends).then_some(i + 4)
            };
            if let Some(end) = reference(bytes, i).or_else(null) {
                out.extend_from_slice(b"\0R");
                i = end;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    out
}

/// Whether `obj` is an object or cross-reference stream: a file's
/// structure, which no write keeps.
fn structural(obj: &Object) -> bool {
    obj.as_stream()
        .is_some_and(|s| matches!(s.dict.type_name(), Some(b"ObjStm" | b"XRef")))
}

/// `written`, read back with `password`: the file, and the checksum of
/// its objects but its object and cross-reference streams, each read
/// (decrypted) and written as sheaf's writer writes it in the clear, at
/// its number, in order: a reference to object 0, which a stable save's
/// copy keeps as its source had it, as `null`.
fn read_back(written: &Output, password: Option<&str>) -> Result<(LazyPdf, u128), String> {
    use sheaf_doc::pdf::write::{Refused, indirect_renumbered, non_zero_ref};
    let password = written.record.crypt.as_ref().and(password);
    let bytes = Arc::<[u8]>::from(written.bytes.as_slice());
    let pdf = LazyPdf::open(bytes, password, &Limits::DEFAULT.reader)
        .map_err(|e| format!("doesn't read back: {e}"))?;
    let mut sum = Checksum::new();
    let mut out = Vec::new();
    for id in pdf.ids() {
        let obj = (pdf.object_unkept(id)).map_err(|e| format!("{id} doesn't read back: {e}"))?;
        if structural(&obj) {
            continue;
        }
        out.clear();
        indirect_renumbered(&mut out, id, &obj, &(), &non_zero_ref).map_err(|e| match e {
            Refused::Write(e) => format!("{id} doesn't write: {e}"),
            Refused::Encrypt { why, .. } => match why {},
        })?;
        sum.write(&out);
    }
    Ok((pdf, sum.finish()))
}

/// What `written`, `src`'s document renumbered, read back as `after`,
/// copied: each primary object the record puts at a number of the file
/// (but an object stream's), and whether its bytes are its source's,
/// [`shaped`], where the source and the file both have it at an offset,
/// or both in an object stream.
fn copies(src: &Source, written: &Output, after: &LazyPdf) -> Tally {
    let before = src.pdf();
    let now: HashSet<ObjectId> = after.ids().collect();
    let peeked =
        |pdf: &LazyPdf, id: ObjectId| pdf.peek(id).ok().flatten().is_some_and(|o| structural(&o));
    let mut out = Tally {
        files: 1,
        ..Tally::default()
    };
    match aes(written) {
        true => out.aes_bytes = written.bytes.len(),
        false => out.bytes = written.bytes.len(),
    }
    for (key, &at) in &written.record.map.keys {
        let ObjKey::Base { blob, obj, copy: 0 } = *key else {
            continue;
        };
        if blob != src.id() || !now.contains(&at) || peeked(before, obj) || peeked(after, at) {
            continue;
        }
        out.survived += 1;
        let copied = match (before.raw_span_whole(obj), after.raw_span(at)) {
            (Some(a), Some(b)) => shaped(&src.bytes()[a], true) == shaped(&written.bytes[b], true),
            _ => match (before.member_bytes(obj), after.member_bytes(at)) {
                (Some((_, a)), Some((_, b))) => shaped(&a, false) == shaped(&b, false),
                _ => false,
            },
        };
        out.copied += usize::from(copied);
    }
    out
}

/// What one file's renumbered Rewrites came to.
struct Renumbered {
    /// Under each of [`RENUMBERING`]'s profiles.
    copies: [Tally; 3],
    /// `SHEAF_RENUMBER_SUMS`'s lines, when asked: a profile's each, the
    /// default's first.
    sums: Vec<String>,
    /// What's wrong with it.
    failed: Vec<String>,
}

/// The form `entry` written under the default profile (when `sums`
/// asks) and each of [`RENUMBERING`]'s, read back; twice under dense,
/// the same bytes (an AES file, whose IVs are fresh, the same objects).
/// A line of `sums` is the file's path, the profile, how many bytes were
/// written and their checksum (each `-` for AES, as [`Tally`] says why),
/// and the objects' checksum ([`read_back`]): a copy and its object
/// written anew are one object, so that sum is the same whichever is
/// written.
fn renumbered(entry: &Entry, corpus: &Corpus, sums: bool) -> Option<Renumbered> {
    let doc = opened(&corpus.path(entry))?;
    let src = doc.primary();
    let mut out = Renumbered {
        copies: [Tally::default(); 3],
        sums: Vec::new(),
        failed: Vec::new(),
    };
    let default = sums.then_some((None, "default", Profile::default()));
    let renumbering = (0..)
        .zip(RENUMBERING)
        .map(|(i, (name, p))| (Some(i), name, p));
    for (i, name, profile) in default.into_iter().chain(renumbering) {
        let written = match written_as(&doc, profile) {
            Ok(w) => w,
            Err(e) => {
                out.failed.push(format!("{name}: not written: {e:#}"));
                continue;
            }
        };
        if let Some(why) = &written.copy_refused {
            out.failed
                .push(format!("{name}: a copy was refused, nothing copied: {why}"));
        }
        let (after, objects) = match read_back(&written, src.password()) {
            Ok(back) => back,
            Err(e) => {
                out.failed.push(format!("{name}: {e}"));
                continue;
            }
        };
        let in_aes = aes(&written);
        if profile == DENSE {
            let alike = written_as(&doc, profile).is_ok_and(|again| match in_aes {
                true => read_back(&again, src.password()).is_ok_and(|(_, sum)| sum == objects),
                false => again.bytes == written.bytes,
            });
            if !alike {
                out.failed.push(format!("{name}: a second write differs"));
            }
        }
        if let Some(i) = i {
            out.copies[i] = copies(src, &written, &after);
        }
        if sums {
            let (len, file) = match in_aes {
                true => ("-".to_owned(), "-".to_owned()),
                false => (
                    written.bytes.len().to_string(),
                    checksum_hex(&written.bytes),
                ),
            };
            let path = &entry.path;
            let line = format!("{path}\t{name}\t{len}\t{file}\t{objects:032x}");
            out.sums.push(line);
        }
    }
    Some(out)
}

#[test]
fn a_renumbered_rewrite_copies_what_it_left_alone() {
    let Some(corpus) = Corpus::or_skip() else {
        return;
    };
    let entries = corpus.forms().unwrap();
    let sums = std::env::var_os("SHEAF_RENUMBER_SUMS");
    let outcomes = each_entry(&entries, |entry| renumbered(entry, &corpus, sums.is_some()));
    let mut totals = [Tally::default(); 3];
    let mut lines = Vec::new();
    let mut failed = BTreeMap::new();
    for (entry, outcome) in entries.iter().zip(outcomes) {
        let Some(o) = outcome else {
            continue;
        };
        for (total, c) in totals.iter_mut().zip(o.copies) {
            total.files += c.files;
            total.survived += c.survived;
            total.copied += c.copied;
            total.bytes += c.bytes;
            total.aes_bytes += c.aes_bytes;
        }
        lines.extend(o.sums.into_iter().map(|line| (entry.path.clone(), line)));
        if !o.failed.is_empty() {
            failed.insert(entry.path.clone(), o.failed);
        }
    }
    if let Some(file) = &sums {
        // By path; a file's profiles in the order written.
        lines.sort_by(|a, b| a.0.cmp(&b.0));
        let text: String = lines.iter().map(|(_, line)| format!("{line}\n")).collect();
        std::fs::write(file, text).unwrap();
    }
    for ((name, _), c) in RENUMBERING.iter().zip(&totals) {
        println!(
            "{name}: {} of {} files written; {} primary objects survived, {} copied as they \
             were; {} bytes, and {} of AES files",
            c.files,
            entries.len(),
            c.survived,
            c.copied,
            c.bytes,
            c.aes_bytes
        );
    }
    println!("{} files failed", failed.len());
    for (path, why) in &failed {
        println!("{path}: {}", why[..why.len().min(3)].join("; "));
    }
    assert!(failed.is_empty());
    if entries.len() == RENUMBERED.manifest {
        for ((name, _), (c, b)) in RENUMBERING
            .iter()
            .zip(totals.iter().zip(&RENUMBERED.profiles))
        {
            assert!(
                c.files >= b.files,
                "{name}: {} files, from {}",
                c.files,
                b.files
            );
            assert!(
                c.survived >= b.survived,
                "{name}: {} survived, from {}",
                c.survived,
                b.survived
            );
            assert!(
                c.copied >= b.copied,
                "{name}: {} copied, from {}",
                c.copied,
                b.copied
            );
            assert!(
                c.bytes <= b.bytes,
                "{name}: {} bytes, from {}",
                c.bytes,
                b.bytes
            );
        }
    }
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
