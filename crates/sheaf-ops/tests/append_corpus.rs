//! Appends over Greg's corpus of real forms (M10.5 slice 10), each on a
//! copy of its file, which an Append writes in place:
//! - every file takes one mark saved as the app saves it, planned, and
//!   reopens with it: an Append keeps the file's bytes as its first; where
//!   the plan rewrites or asks, why is counted; the removal check never
//!   finds a derived object's text gone (a `RemovalMisclassified`), and
//!   its vetoes on the files' own objects are counted, for tuning;
//! - a crop is the page's boxes alone (M10.5 slice 15; ignored, as a
//!   corpus tier): every file's first page cropped and saved as planned
//!   reopens cropped, an Append writing that page's dictionary and
//!   nothing else, and the form check finds nothing in the file it didn't
//!   find before; where the plan rewrites or asks, why is counted;
//! - deleted means deleted, both ways, on every file (ignored, as it
//!   takes about seven minutes; each slice's Verify and commit run it): a
//!   page, a mark, a reply, a mark's text, another tool's comment or a
//!   bookmark saved by a Rewrite or an Append, then deleted (or replaced),
//!   is a Rewrite as planned, and what it said can't be read in the file
//!   after: its text, in any of the ways a file holds it, and a run of the
//!   page's content only it had; an Append the file can't take is counted
//!   by why;
//! - the removal check as a test oracle on the Rewrite that deletes a
//!   file's first page (§11.9): run over what the Rewrite wrote, as though
//!   it were a section written over the file, it finds what the page took
//!   with it, and never a derived object's text;
//! - signed and certified files ([`SIGNED`]): a comment appends, or asks
//!   first, as §9.3's table says, and appended (anyway, where it asks),
//!   the file's bytes stay its first, every signature's byte range still
//!   within them.
//!
//! The counts are asserted against their baselines ([`MARKED`],
//! [`DELETED`], [`ORACLE`]) when the corpus is the one they were counted on (its
//! manifest's length): a later slice that moves one says why in the plan's
//! slice notes, and moves the baseline. Skips when there's no corpus (see
//! `sheaf_doc::forms::corpus`).

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use in_progress::NoProgress;
use lopdf::{Document as Pdf, Object, ObjectId};
use sheaf_doc::forms::corpus::{Corpus, Entry, each_entry};
use sheaf_doc::forms::sigs::Verdict;
use sheaf_doc::materialize::{
    Ask, AskAnswers, How, Onto, SaveStop, Strategy, StrategyKind, Why, forced_append, materialize,
    plan, removal, save, save_rewritten, write_file,
};
use sheaf_doc::tx::fixture::{add, bookmark, crop, delete, delete_annots, delete_bookmark, update};
use sheaf_doc::{Annot, AnnotId, AnnotKind, Document, PageId, Pt};
use sheaf_edit::{Margins, Rotation};

mod common;

/// A directory of its own for `test`'s copies, empty.
fn scratch(test: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("sheaf-append-corpus-{test}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// `entry`'s file copied into `dir`, named for it and `tag`: None when it
/// can't be read.
fn copy_of(entry: &Entry, corpus: &Corpus, dir: &Path, tag: &str) -> Option<PathBuf> {
    let to = dir.join(format!("{tag}{}", entry.path.replace('/', "-")));
    std::fs::copy(corpus.path(entry), &to).ok()?;
    Some(to)
}

/// `path` and whatever an Append left beside it, removed.
fn remove(path: &Path) {
    let _ = std::fs::remove_file(path);
    let mut journal = path.as_os_str().to_owned();
    journal.push(".sheaf-append");
    let _ = std::fs::remove_file(journal);
}

/// The file at `path`, opened as the app opens it; None when it doesn't
/// open without a password.
fn opened(path: &Path) -> Option<Document> {
    sheaf_doc::open::file(path, None).ok().map(|o| o.doc)
}

/// A note saying `text`.
fn note(text: &str) -> Annot {
    Annot {
        id: AnnotId(0),
        kind: AnnotKind::Note {
            pos: Pt { x: 72.0, y: 72.0 },
        },
        color: [255, 230, 0],
        opacity: 1.0,
        stroke: 1.0,
        author: "sheaf".into(),
        contents: text.into(),
        modified: String::new(),
        created: String::new(),
        fill: None,
        dashed: false,
        cloudy: false,
        turn: Rotation::R0,
        name: String::new(),
    }
}

/// What a save's reasons are, by kind: each's name.
fn reasons(why: &[Why]) -> String {
    let name = |w: &Why| {
        let w = format!("{w:?}");
        let end = w.find(['(', ' ', '{']).unwrap_or(w.len());
        w[..end].to_string()
    };
    why.iter().map(name).collect::<Vec<_>>().join(", ")
}

// ----- One mark, planned ----------------------------------------------------

/// What one mark saved as planned onto a copy of a file came to.
enum Marked {
    /// Appended, and reopened with the mark, the file's bytes its first.
    Appended,
    /// Rewritten, for these reasons, and reopened with the mark.
    Rewritten(String),
    /// The plan asked about the file's signatures first.
    Asked,
}

/// The text of the mark every file takes.
const MARK: &str = "SHEAF-CORPUS-MARK";

/// A copy of `entry`'s file with a note on its first page, saved to itself
/// as planned, asking nothing, then reopened: None when it doesn't open.
fn marked(entry: &Entry, corpus: &Corpus, dir: &Path) -> Option<Result<Marked, String>> {
    let path = copy_of(entry, corpus, dir, "")?;
    let outcome = (|| {
        let mut doc = opened(&path)?;
        let before = std::fs::read(&path).unwrap();
        let page = doc.page(0)?;
        if let Err(e) = doc.commit_ops(vec![add(page, &note(MARK))], "mark") {
            return Some(Err(format!("no mark: {e}")));
        }
        Some(saved_with_a_mark(&mut doc, &path, &before))
    })();
    remove(&path);
    outcome
}

/// [`marked`]'s save of `doc` to `path`, whose bytes were `before`, and
/// its reopen.
fn saved_with_a_mark(doc: &mut Document, path: &Path, before: &[u8]) -> Result<Marked, String> {
    let pages = doc.state().page_count();
    let report = match save(doc, path, &AskAnswers::default(), &NoProgress) {
        Ok(report) => report,
        Err(SaveStop::Ask(Ask::Signatures { .. })) => return Ok(Marked::Asked),
        Err(stop) => return Err(format!("stopped: {stop}")),
    };
    let after = std::fs::read(path).unwrap();
    let marked = match report.strategy {
        Some(StrategyKind::Append) if !after.starts_with(before) => {
            return Err("an Append that doesn't keep the file's bytes first".into());
        }
        Some(StrategyKind::Append) => Marked::Appended,
        Some(StrategyKind::Rewrite) => Marked::Rewritten(reasons(&report.why)),
        other => return Err(format!("saved as {other:?}")),
    };
    let again = opened(path).ok_or("the saved file doesn't open")?;
    if again.state().page_count() != pages {
        return Err(format!(
            "{} pages, from {pages}",
            again.state().page_count()
        ));
    }
    let first = again.page(0).ok_or("no first page")?;
    if !again.state().marks_on(first).any(|a| a.contents == MARK) {
        return Err("the mark didn't read back".into());
    }
    Ok(marked)
}

/// One mark saved as planned onto every file (slice 10): files marked, and
/// those appended. Lower counts are a regression unless the slice notes say
/// why they're right. Of the 4,389, 360 asked about their signatures first
/// (a certification, or a `/Lock`, with `/P` 1 or 2, and 1263925's that
/// can't be read), and 259 were rewritten: 211 for the removal check, every
/// one a file whose usage rights a comment is outside, which save policy
/// drops (its `/Perms /UR3` signature unlinked); 34 whose first page shows a
/// widget that is its field's object, though the field has others or the
/// widget shows twice, which a save writes apart, so the file hasn't got
/// it (`BaseObjectsMissing`); and 14 whose cross-reference table was
/// rebuilt to read them (`BaseUnsound`).
const MARKED: MarkedBaseline = MarkedBaseline {
    manifest: 4392,
    files: 4389,
    appended: 3770,
};

struct MarkedBaseline {
    manifest: usize,
    files: usize,
    appended: usize,
}

#[test]
fn every_file_appends_a_mark_as_planned_and_reopens_with_it() {
    let Some(corpus) = Corpus::or_skip() else {
        return;
    };
    let dir = scratch("marked");
    let entries = corpus.manifest().unwrap();
    let outcomes = each_entry(&entries, |entry| marked(entry, &corpus, &dir));
    let (mut files, mut appended, mut asked) = (0, 0, 0);
    let mut rewritten: BTreeMap<String, Vec<&str>> = BTreeMap::new();
    let mut failed = BTreeMap::new();
    for (entry, outcome) in entries.iter().zip(outcomes) {
        let Some(outcome) = outcome else {
            continue;
        };
        files += 1;
        match outcome {
            Ok(Marked::Appended) => appended += 1,
            Ok(Marked::Rewritten(why)) => rewritten.entry(why).or_default().push(&entry.path),
            Ok(Marked::Asked) => asked += 1,
            Err(e) => {
                failed.insert(entry.path.clone(), e);
            }
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    let vetoes = removal::vetoes();
    println!(
        "{files} of {} files marked: {appended} appended, {} rewritten, {asked} asked about \
         their signatures first; {} failed; removal check vetoes: {} derived, {} on the files' \
         own objects",
        entries.len(),
        rewritten.values().map(Vec::len).sum::<usize>(),
        failed.len(),
        vetoes.derived,
        vetoes.source,
    );
    for (why, paths) in &rewritten {
        let some = paths[..paths.len().min(4)].join(", ");
        println!("  rewritten for {why}: {} ({some})", paths.len());
    }
    for (path, why) in &failed {
        println!("{path}: {why}");
    }
    assert!(failed.is_empty());
    assert_eq!(vetoes.derived, 0, "a derived object's text was found gone");
    if entries.len() == MARKED.manifest {
        assert!(
            files >= MARKED.files,
            "{files} files, from {}",
            MARKED.files
        );
        assert!(
            appended >= MARKED.appended,
            "{appended} appended, from {}",
            MARKED.appended
        );
    }
}

// ----- A crop appends page dicts alone --------------------------------------

/// What a crop of a copy's first page, saved as planned, came to.
enum Cropped {
    /// Appended, the section the page's dictionary alone.
    Appended,
    /// Rewritten, for these reasons.
    Rewritten(String),
    /// The plan asked about the file's signatures first.
    Asked,
    /// The page shows too little to crop by [`CROP_BY`] a side.
    TooSmall,
}

/// Points off each side of the first page as it shows.
const CROP_BY: f32 = 18.0;

/// Corpus paths, comma-separated, to run [`crop_appends_only_page_dicts`]
/// on alone (`SHEAF_CROP_ONLY=forms/irs/f1040.pdf,…`): for looking at a few
/// files by hand. Unset, it crops every file.
const CROP_ONLY: &str = "SHEAF_CROP_ONLY";

/// A copy of `entry`'s file with its first page cropped by [`CROP_BY`] a
/// side, saved to itself as planned, asking nothing, then reopened: None
/// when it doesn't open.
fn cropped(entry: &Entry, corpus: &Corpus, dir: &Path) -> Option<Result<Cropped, String>> {
    let path = copy_of(entry, corpus, dir, "crop-")?;
    let outcome = (|| {
        let mut doc = opened(&path)?;
        let before = std::fs::read(&path).unwrap();
        let page = doc.page(0)?;
        let shown = doc.geom(page).shown();
        let least = 2.0 * CROP_BY + sheaf_doc::geom::MIN_SIDE;
        if shown.width < least || shown.height < least {
            return Some(Ok(Cropped::TooSmall));
        }
        let by = Margins {
            top: CROP_BY,
            bottom: CROP_BY,
            left: CROP_BY,
            right: CROP_BY,
        };
        if let Err(e) = doc.commit_ops(vec![crop(&[page], by)], "crop") {
            return Some(Err(format!("no crop: {e}")));
        }
        Some(saved_cropped(&mut doc, page, &path, &before))
    })();
    remove(&path);
    outcome
}

/// [`cropped`]'s save of `doc`, its page `page` cropped, to `path`, whose
/// bytes were `before`, and its reopen.
fn saved_cropped(
    doc: &mut Document,
    page: PageId,
    path: &Path,
    before: &[u8],
) -> Result<Cropped, String> {
    let report = match save(doc, path, &AskAnswers::default(), &NoProgress) {
        Ok(report) => report,
        Err(SaveStop::Ask(Ask::Signatures { .. })) => return Ok(Cropped::Asked),
        Err(stop) => return Err(format!("stopped: {stop}")),
    };
    let after = std::fs::read(path).unwrap();
    let cropped = match report.strategy {
        Some(StrategyKind::Append) if !after.starts_with(before) => {
            return Err("an Append that doesn't keep the file's bytes first".into());
        }
        Some(StrategyKind::Append) => {
            let wrote = common::appended_objects(before.len(), &after);
            let others: Vec<&ObjectId> = (wrote.iter())
                .filter(|(_, page)| !**page)
                .map(|(id, _)| id)
                .collect();
            if !others.is_empty() || wrote.is_empty() {
                return Err(format!(
                    "the Append wrote {} objects, {others:?} not pages",
                    wrote.len()
                ));
            }
            Cropped::Appended
        }
        Some(StrategyKind::Rewrite) => Cropped::Rewritten(reasons(&report.why)),
        other => return Err(format!("saved as {other:?}")),
    };
    // What the form check finds after that it didn't before.
    if let Ok(was) = sheaf_doc::forms::check(before, None) {
        let now = sheaf_doc::forms::check(&after, None).map_err(|e| format!("check: {e:#}"))?;
        let added: Vec<_> = (now.findings.iter())
            .filter(|f| !was.findings.contains(f))
            .collect();
        if !added.is_empty() {
            return Err(format!("the form check finds {added:?}"));
        }
    }
    let again = opened(path).ok_or("the saved file doesn't open")?;
    let first = again.page(0).ok_or("no first page")?;
    let (shows, showed) = (again.geom(first).visible(), doc.geom(page).visible());
    if shows != showed {
        return Err(format!(
            "reopened, it shows {shows:?}, cropped to {showed:?}"
        ));
    }
    Ok(cropped)
}

/// Every file's first page cropped, saved as planned (slice 15): an Append
/// writes the page's dictionary alone, never another object, and no save
/// gives the form check a finding the file didn't have.
#[test]
#[ignore = "corpus tier: cargo test -p sheaf-ops --test append_corpus -- --ignored --nocapture"]
fn crop_appends_only_page_dicts() {
    let Some(corpus) = Corpus::or_skip() else {
        return;
    };
    let dir = scratch("cropped");
    let entries = common::narrowed(CROP_ONLY, corpus.manifest().unwrap(), |e| &e.path);
    let outcomes = each_entry(&entries, |entry| cropped(entry, &corpus, &dir));
    let (mut files, mut appended, mut asked, mut small) = (0, 0, 0, 0);
    let mut rewritten: BTreeMap<String, Vec<&str>> = BTreeMap::new();
    let mut failed = BTreeMap::new();
    for (entry, outcome) in entries.iter().zip(outcomes) {
        let Some(outcome) = outcome else {
            continue;
        };
        files += 1;
        match outcome {
            Ok(Cropped::Appended) => appended += 1,
            Ok(Cropped::Rewritten(why)) => rewritten.entry(why).or_default().push(&entry.path),
            Ok(Cropped::Asked) => asked += 1,
            Ok(Cropped::TooSmall) => small += 1,
            Err(e) => {
                failed.insert(entry.path.clone(), e);
            }
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    println!(
        "{files} of {} files cropped: {appended} appended with page dictionaries alone, {} \
         rewritten, {asked} asked about their signatures first, {small} too small to crop; {} \
         failed",
        entries.len(),
        rewritten.values().map(Vec::len).sum::<usize>(),
        failed.len(),
    );
    for (why, paths) in &rewritten {
        let some = paths[..paths.len().min(4)].join(", ");
        println!("  rewritten for {why}: {} ({some})", paths.len());
    }
    for (path, why) in &failed {
        println!("{path}: {why}");
    }
    assert!(failed.is_empty());
}

// ----- Deleted means deleted, both ways --------------------------------------

/// What's deleted from a saved file in [`deleted`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Feature {
    /// The last page, with a note on it.
    Page,
    Mark,
    /// A reply to a note, the note kept.
    Reply,
    /// A note's text, replaced.
    MarkText,
    /// The first comment of another tool's in the file whose text no other
    /// object of it holds, a note beside it saved first.
    Theirs,
    Bookmark,
}

const FEATURES: [Feature; 6] = [
    Feature::Page,
    Feature::Mark,
    Feature::Reply,
    Feature::MarkText,
    Feature::Theirs,
    Feature::Bookmark,
];

/// The bytes `text` may be written as: itself, and in UTF-16 (a text
/// string's other encoding), each also as a hex string's digits, in either
/// case.
fn written_as(text: &str) -> Vec<Vec<u8>> {
    let utf16: Vec<u8> = text.encode_utf16().flat_map(u16::to_be_bytes).collect();
    let mut out = Vec::new();
    for bytes in [text.as_bytes().to_vec(), utf16] {
        let hex: String = bytes.iter().map(|b| format!("{b:02X}")).collect();
        out.push(hex.to_lowercase().into_bytes());
        out.push(hex.into_bytes());
        out.push(bytes);
    }
    out
}

/// Whether any of `forms` can be read in the file `bytes`: in its bytes as
/// they are, in a stream decoded (an object stream's too) or in a string
/// any object holds, decrypted. Every object of the file's newest
/// revision, which is all of a Rewrite's.
fn readable(bytes: &[u8], forms: &[Vec<u8>]) -> bool {
    let found = |hay: &[u8]| (forms.iter()).any(|f| hay.windows(f.len()).any(|w| w == &f[..]));
    found(bytes) || (Pdf::load_mem(bytes).unwrap().objects.values()).any(|o| holds_in(o, &found))
}

/// Whether `found` finds what it looks for in `o`: in its strings, and its
/// stream's data decoded.
fn holds_in(o: &Object, found: &dyn Fn(&[u8]) -> bool) -> bool {
    match o {
        Object::String(s, _) => found(s),
        Object::Array(items) => items.iter().any(|o| holds_in(o, found)),
        Object::Dictionary(d) => d.iter().any(|(_, o)| holds_in(o, found)),
        Object::Stream(s) => {
            let data = (s.decompressed_content()).unwrap_or_else(|_| s.content.clone());
            found(&data) || s.dict.iter().any(|(_, o)| holds_in(o, found))
        }
        _ => false,
    }
}

/// Bytes only page `index` of the file `bytes` holds: 48 from the middle of
/// its first content stream, decoded, which no other object of the file
/// holds, from a stream nothing else refers to. None when there are none.
fn page_probe(bytes: &[u8], index: usize) -> Option<Vec<u8>> {
    let pdf = Pdf::load_mem(bytes).ok()?;
    let page = *pdf.get_pages().get(&(u32::try_from(index).ok()? + 1))?;
    let first = match pdf.get_dictionary(page).ok()?.get(b"Contents").ok()? {
        Object::Reference(id) => *id,
        Object::Array(items) => items.first()?.as_reference().ok()?,
        _ => return None,
    };
    let mut referred: HashMap<ObjectId, usize> = HashMap::new();
    for obj in pdf.objects.values() {
        count_refs(obj, &mut referred);
    }
    if referred.get(&first) != Some(&1) {
        return None;
    }
    let data = pdf.get_object(first).ok()?.as_stream().ok()?;
    let data = data.decompressed_content().ok()?;
    let window = data.get(data.len() / 2..data.len() / 2 + 48)?.to_vec();
    let found = |hay: &[u8]| hay.windows(window.len()).any(|w| w == &window[..]);
    let elsewhere = (pdf.objects.iter()).any(|(&id, o)| id != first && holds_in(o, &found));
    (!elsewhere).then_some(window)
}

/// Adds each reference `obj` holds to `counts`.
fn count_refs(obj: &Object, counts: &mut HashMap<ObjectId, usize>) {
    match obj {
        Object::Reference(id) => *counts.entry(*id).or_default() += 1,
        Object::Array(items) => items.iter().for_each(|o| count_refs(o, counts)),
        Object::Dictionary(d) => d.iter().for_each(|(_, o)| count_refs(o, counts)),
        Object::Stream(s) => s.dict.iter().for_each(|(_, o)| count_refs(o, counts)),
        _ => {}
    }
}

/// What one of [`deleted`]'s runs came to: the first save, an Append,
/// rewritten instead for a file that can't take one (why), or what can
/// still be read that went, and whether the page had a run of its content
/// to look for.
enum Deleted {
    Declined(String),
    Gone { probed: bool },
}

/// A copy of `entry`'s file given `feature`, saved `how`, then the feature
/// deleted (or its text replaced) and the file saved as planned,
/// signatures and an XFA part's values given up if it asks: a Rewrite,
/// after which what the feature said can't be read in the file. None when
/// the file doesn't open, the page has no page after it to keep, or it has
/// no comment of another tool's to delete.
fn deleted(
    entry: &Entry,
    corpus: &Corpus,
    dir: &Path,
    how: How,
    feature: Feature,
) -> Option<Result<Deleted, String>> {
    let path = copy_of(entry, corpus, dir, &format!("{how:?}-{feature:?}-"))?;
    let outcome = (|| {
        let mut doc = opened(&path)?;
        let pages = doc.state().page_count();
        if matches!(feature, Feature::Page) && pages < 2 {
            return None;
        }
        let theirs = match feature {
            Feature::Theirs => Some(theirs(&doc, &std::fs::read(&path).ok()?)?),
            _ => None,
        };
        Some(saved_then_deleted(&mut doc, &path, how, feature, theirs))
    })();
    remove(&path);
    outcome
}

/// Another tool's comment of `doc`, opened from the file `bytes`, to
/// delete: its page, its name and its text, at least eight characters,
/// which no object of the file holds but the comment's own (its pop-up and
/// appearances). None when there's none.
fn theirs(doc: &Document, bytes: &[u8]) -> Option<(PageId, String, String)> {
    let pdf = Pdf::load_mem(bytes).ok()?;
    let st = doc.state();
    (st.order().iter()).find_map(|&page| {
        st.foreign_on(page).find_map(|(_, f)| {
            let summary = &f.summary;
            let text = summary.contents.trim();
            let comment = !matches!(summary.subtype.as_str(), "Link" | "Widget" | "Popup");
            let at = f.src.as_ref()?.id;
            if !comment || text.chars().count() < 8 {
                return None;
            }
            let forms = written_as(text);
            let found =
                |hay: &[u8]| (forms.iter()).any(|f| hay.windows(f.len()).any(|w| w == &f[..]));
            let own = own_objects(&pdf, at);
            let elsewhere =
                (pdf.objects.iter()).any(|(id, o)| !own.contains(id) && holds_in(o, &found));
            let there = pdf.objects.get(&at).is_some_and(|o| holds_in(o, &found));
            (there && !elsewhere).then(|| (page, summary.name.clone(), text.to_string()))
        })
    })
}

/// Annotation `at` of `pdf` and what's its own: its pop-up, and the
/// appearance streams its `/AP` names.
fn own_objects(pdf: &Pdf, at: ObjectId) -> Vec<ObjectId> {
    let mut own = vec![at];
    let Ok(annot) = pdf.get_dictionary(at) else {
        return own;
    };
    own.extend(annot.get(b"Popup").and_then(Object::as_reference));
    let mut refs = HashMap::new();
    if let Ok(ap) = annot.get(b"AP") {
        let ap = pdf.dereference(ap).map_or(ap, |(_, o)| o);
        count_refs(ap, &mut refs);
        for &id in refs.clone().keys() {
            if let Ok(Object::Dictionary(states)) = pdf.get_object(id) {
                count_refs(&Object::Dictionary(states.clone()), &mut refs);
            }
        }
    }
    own.extend(refs.into_keys());
    own
}

/// `doc` appended to its file at `path`, as "Append anyway" appends
/// (whatever its signatures allow, what it removes reported): a Rewrite
/// instead when the section can't follow the file, as any save makes it.
/// How it wrote.
fn appended_or_not(doc: &mut Document, path: &Path) -> Result<Strategy, SaveStop> {
    let v = doc.current_version();
    let input = doc
        .save_input(v)
        .map_err(|e| SaveStop::Failed(format!("{e:#}")))?;
    let plan = forced_append(&input, Onto::Saved);
    let answers = AskAnswers {
        append_anyway: true,
        stays_readable: true,
        found: true,
        break_signatures: true,
        drop_xfa_values: true,
        ..Default::default()
    };
    match write_file(&input, &plan, doc.save_target(path), &answers, &NoProgress) {
        Ok(written) => {
            let strategy = written.strategy.clone();
            (written.record_in(doc)).map_err(|e| SaveStop::Failed(format!("{e:#}")))?;
            Ok(strategy)
        }
        Err(stop) => {
            doc.abandon_save(v);
            Err(stop)
        }
    }
}

/// [`deleted`]'s edits and saves of `doc`, opened from `path`; `theirs`
/// another tool's comment to delete.
fn saved_then_deleted(
    doc: &mut Document,
    path: &Path,
    how: How,
    feature: Feature,
    theirs: Option<(PageId, String, String)>,
) -> Result<Deleted, String> {
    let ours = format!("SHEAF-{feature:?}-GONE");
    let text = theirs
        .as_ref()
        .map_or(ours.clone(), |(_, _, text)| text.clone());
    let last = doc.state().page_count() - 1;
    let on = match feature {
        Feature::Page => doc.page(last).unwrap(),
        _ => doc.page(0).unwrap(),
    };
    let probe = match feature {
        Feature::Page => page_probe(&std::fs::read(path).unwrap(), last),
        _ => None,
    };
    let put = |doc: &mut Document, annot: Annot| {
        doc.commit_ops(vec![add(on, &annot)], "add")
            .map(drop)
            .map_err(|e| format!("adding: {e}"))
    };
    match feature {
        Feature::Page | Feature::Mark | Feature::MarkText => put(doc, note(&ours))?,
        Feature::Reply => {
            put(doc, note("SHEAF-Reply-PARENT"))?;
            let parent = (doc.state().marks_on(on))
                .find(|a| a.contents == "SHEAF-Reply-PARENT")
                .ok_or("the note isn't on its page")?
                .name();
            let reply = Annot {
                kind: AnnotKind::Reply { parent },
                ..note(&ours)
            };
            put(doc, reply)?;
        }
        Feature::Theirs => put(doc, note("SHEAF-Theirs-OURS"))?,
        Feature::Bookmark => {
            let insert = bookmark(None, None, &ours, Some(on));
            doc.commit_ops(vec![insert], "add")
                .map_err(|e| format!("adding: {e}"))?;
        }
    }
    let first = match how {
        How::Rewritten => save_rewritten(doc, path).map(|_| None),
        How::Appended => appended_or_not(doc, path).map(Some),
    };
    match first {
        Ok(None | Some(Strategy::Append { .. })) => {}
        Ok(Some(Strategy::Rewrite { why }))
            if matches!(why[..], [Why::BaseUnsound] | [Why::BaseObjectsMissing]) =>
        {
            return Ok(Deleted::Declined(reasons(&why)));
        }
        Ok(Some(other)) => return Err(format!("the first save was {other:?}")),
        Err(stop) => return Err(format!("the first save stopped: {stop}")),
    }
    let forms = written_as(&text);
    if !readable(&std::fs::read(path).unwrap(), &forms) {
        return Err("the first save doesn't hold the text".into());
    }
    let mark = |doc: &Document| {
        (doc.state().marks_on(on))
            .find(|a| a.contents == ours)
            .cloned()
            .ok_or("the note isn't on its page")
    };
    let gone = match feature {
        Feature::Page => delete(&[on]),
        Feature::Mark | Feature::Reply => delete_annots(&[mark(doc)?.id]),
        Feature::MarkText => update(&Annot {
            contents: "SHEAF-MarkText-NEW".into(),
            ..mark(doc)?
        }),
        Feature::Theirs => {
            // Every comment of that name on its page, as Delete Comment
            // deletes them: a file may name two alike.
            let (page, name, _) = theirs.ok_or("no comment of theirs")?;
            let named: Vec<AnnotId> = (doc.state().foreign_on(page))
                .filter(|(_, f)| f.summary.name == name)
                .map(|(id, _)| id)
                .collect();
            delete_annots(&named)
        }
        Feature::Bookmark => {
            let id = (doc.state().bookmarks_preorder().into_iter())
                .find(|(_, _, b)| *b.title == *text)
                .ok_or("the bookmark isn't there")?
                .1;
            delete_bookmark(id)
        }
    };
    doc.commit_ops(vec![gone], "delete")
        .map_err(|e| format!("deleting: {e}"))?;
    let answers = AskAnswers {
        break_signatures: true,
        drop_xfa_values: true,
        ..Default::default()
    };
    let report = save(doc, path, &answers, &NoProgress).map_err(|e| format!("saving: {e}"))?;
    if report.strategy != Some(StrategyKind::Rewrite) {
        return Err(format!("saved as {:?}, not a Rewrite", report.strategy));
    }
    let bytes = std::fs::read(path).unwrap();
    if readable(&bytes, &forms) {
        return Err(format!("{text} can still be read"));
    }
    if let Some(probe) = &probe
        && readable(&bytes, std::slice::from_ref(probe))
    {
        return Err("the deleted page's content can still be read".into());
    }
    Ok(Deleted::Gone {
        probed: probe.is_some(),
    })
}

/// What deleted means deleted found over the corpus (slice 10's review):
/// each feature's first Appends rewritten instead, as the file can't take
/// one, an upper bound. 30 files' cross-reference tables were rebuilt to
/// read them (`BaseUnsound`; a page's 17 of them have more than one page),
/// and 38 first pages (11 last pages) show a widget that is its field's
/// object, though the field has others or the widget shows twice, which a
/// save writes apart, so the file hasn't got it (`BaseObjectsMissing`); a
/// bookmark writes no page, and another tool's comment's 30 files decline
/// none. More is a regression unless the slice notes say why it's right.
const DELETED: DeletedBaseline = DeletedBaseline {
    manifest: 4392,
    declined: [28, 68, 68, 68, 0, 30],
};

struct DeletedBaseline {
    manifest: usize,
    /// By feature, as [`FEATURES`] lists them.
    declined: [usize; 6],
}

/// Every file, each feature deleted after a first save each way.
#[test]
#[ignore = "corpus tier: cargo test -p sheaf-ops --test append_corpus -- --ignored --nocapture"]
fn deleted_means_deleted_both_ways() {
    let Some(corpus) = Corpus::or_skip() else {
        return;
    };
    let dir = scratch("deleted");
    let entries = corpus.manifest().unwrap();
    let mut failed = BTreeMap::new();
    let mut declined = [0; 6];
    for how in [How::Rewritten, How::Appended] {
        for (f, feature) in FEATURES.into_iter().enumerate() {
            let outcomes = each_entry(&entries, |e| deleted(e, &corpus, &dir, how, feature));
            let (mut files, mut probed) = (0, 0);
            let mut why: BTreeMap<String, usize> = BTreeMap::new();
            for (entry, outcome) in entries.iter().zip(outcomes) {
                let Some(outcome) = outcome else {
                    continue;
                };
                files += 1;
                match outcome {
                    Ok(Deleted::Declined(reason)) => *why.entry(reason).or_default() += 1,
                    Ok(Deleted::Gone { probed: p }) => probed += usize::from(p),
                    Err(e) => {
                        failed.insert(format!("{how:?} {feature:?} {}", entry.path), e);
                    }
                }
            }
            if how == How::Appended {
                declined[f] = why.values().sum();
            }
            let probes = match feature {
                Feature::Page => format!(", {probed} with a run of the page's content"),
                _ => String::new(),
            };
            println!(
                "{feature:?} deleted after a first save {how:?}: {files} files, {} declined \
                 that save {why:?}{probes}",
                why.values().sum::<usize>()
            );
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    for (what, why) in &failed {
        println!("{what}: {why}");
    }
    assert!(failed.is_empty());
    if entries.len() == DELETED.manifest {
        for (f, feature) in FEATURES.into_iter().enumerate() {
            assert!(
                declined[f] <= DELETED.declined[f],
                "{feature:?}: {} declined, from {}",
                declined[f],
                DELETED.declined[f]
            );
        }
    }
}

// ----- The removal check as an oracle on a Rewrite ---------------------------

/// What the oracle found over the corpus's page-delete Rewrites (slice 10):
/// files with a page after the first, and those whose Rewrite the check
/// finds took text out. Lower counts are a regression unless the slice
/// notes say why they're right. It finds none in two, 0016064 and 0112654,
/// whose cover pages' words their other pages say too (0016064's logo is
/// on its other pages as well).
const ORACLE: OracleBaseline = OracleBaseline {
    manifest: 4392,
    files: 2901,
    found: 2899,
};

struct OracleBaseline {
    manifest: usize,
    files: usize,
    found: usize,
}

/// `entry`'s file with its first page deleted, planned as a save to itself
/// (a Rewrite, for what went) and written in memory, then the removal check
/// run over what it wrote against the file: whether it found text gone.
/// None when the file doesn't open or has one page.
fn oracle(entry: &Entry, corpus: &Corpus) -> Option<Result<bool, String>> {
    let path = corpus.path(entry);
    let mut doc = opened(&path)?;
    if doc.state().page_count() < 2 {
        return None;
    }
    let first = doc.page(0).unwrap();
    doc.commit_ops(vec![delete(&[first])], "delete").ok()?;
    let v = doc.current_version();
    let input = doc.save_input(v).ok()?;
    let target = doc.save_target(&path);
    let plan = plan(&input, &target);
    let outcome = (|| {
        if !matches!(plan.strategy, Strategy::Rewrite { .. }) {
            return Err(format!("planned {:?}", plan.strategy));
        }
        let out = materialize(&input, &plan, target, &NoProgress).map_err(|e| e.to_string())?;
        let bytes: Arc<[u8]> = std::fs::read(&path).unwrap().into();
        match removal::of_rewrite(&input, &out, bytes) {
            Ok(Some(found)) if found.derived => Err(format!("derived: {:?}", found.key)),
            Ok(found) => Ok(found.is_some()),
            Err(e) => Err(format!("{e:#}")),
        }
    })();
    doc.abandon_save(v);
    Some(outcome)
}

#[test]
fn the_removal_check_finds_what_a_page_delete_rewrite_took_out() {
    let Some(corpus) = Corpus::or_skip() else {
        return;
    };
    let entries = corpus.manifest().unwrap();
    let outcomes = each_entry(&entries, |entry| oracle(entry, &corpus));
    let mut files = 0;
    let (mut found, mut not) = (0, Vec::new());
    let mut failed = BTreeMap::new();
    for (entry, outcome) in entries.iter().zip(outcomes) {
        let Some(outcome) = outcome else {
            continue;
        };
        files += 1;
        match outcome {
            Ok(true) => found += 1,
            Ok(false) => not.push(entry.path.as_str()),
            Err(e) => {
                failed.insert(entry.path.clone(), e);
            }
        }
    }
    println!(
        "{files} files without their first page: the removal check finds text gone from {found}, \
         none from {}; {} failed",
        not.len(),
        failed.len()
    );
    println!("  none found: {}", not[..not.len().min(12)].join(", "));
    for (path, why) in &failed {
        println!("{path}: {why}");
    }
    assert!(failed.is_empty());
    if entries.len() == ORACLE.manifest {
        assert!(
            files >= ORACLE.files,
            "{files} files, from {}",
            ORACLE.files
        );
        assert!(
            found >= ORACLE.found,
            "{found} found, from {}",
            ORACLE.found
        );
    }
}

// ----- Signed and certified files -------------------------------------------

/// What a file's signatures allow after them: a certification's DocMDP
/// `/P` (or a field's `/Lock /P`), or only approval signatures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Allows {
    P(u8),
    Approval,
}

/// Signed and certified corpus files, picked by what their signatures allow
/// (`Source::signatures`, over the 1,121 files with a signature) and none
/// with Reader's usage rights: for each class the first two in the
/// manifest's order, an encrypted one where there's one, and the first
/// whose approval signature's `/Lock` sets `/P` 1.
const SIGNED: [(&str, Allows); 9] = [
    ("forms/safedocs/0000772.pdf", Allows::P(1)),
    ("forms/safedocs/0043679.pdf", Allows::P(1)),
    ("forms/safedocs/0001207.pdf", Allows::P(1)),
    ("forms/safedocs/0000823.pdf", Allows::P(2)),
    ("forms/safedocs/0101803.pdf", Allows::P(2)),
    ("forms/safedocs/0008830.pdf", Allows::P(3)),
    ("forms/safedocs/0338205.pdf", Allows::P(3)),
    ("forms/safedocs/0000581.pdf", Allows::Approval),
    ("forms/safedocs/0023352.pdf", Allows::Approval),
];

/// Every signature's `/ByteRange` in the file `bytes`.
fn byte_ranges(bytes: &[u8]) -> Vec<[usize; 4]> {
    let pdf = Pdf::load_mem(bytes).unwrap();
    let mut out: Vec<[usize; 4]> = (pdf.objects.values())
        .filter_map(|o| o.as_dict().ok()?.get(b"ByteRange").ok()?.as_array().ok())
        .filter_map(|range| {
            let n: Vec<usize> = (range.iter())
                .filter_map(|n| usize::try_from(n.as_i64().ok()?).ok())
                .collect();
            n.try_into().ok()
        })
        .collect();
    out.sort();
    out
}

/// A comment on the first page of each of [`SIGNED`]: an Append within
/// what the signatures allow (§9.3: a certification with `/P` 3, approval
/// signatures) asks nothing; one outside it (`/P` 1 or 2) asks, offering
/// Append anyway, which it's then answered. The file's bytes are the
/// appended file's first, and every signature's byte range ends within
/// them.
#[test]
fn a_comment_on_a_signed_file_appends_or_asks_by_what_its_signatures_allow() {
    let Some(corpus) = Corpus::or_skip() else {
        return;
    };
    let dir = scratch("signed");
    for (path, allows) in SIGNED {
        let Some(entry) = corpus.entry(path).unwrap() else {
            eprintln!("skipped: the corpus has no {path}");
            continue;
        };
        let copy = copy_of(&entry, &corpus, &dir, "").unwrap();
        let mut doc = opened(&copy).unwrap_or_else(|| panic!("{path} opens"));
        let sigs = (doc.primary().signatures()).clone();
        let p = sigs.signed.iter().filter_map(|s| s.p).min();
        let class = match p {
            Some(p) => Allows::P(p),
            None => Allows::Approval,
        };
        assert_eq!(class, allows, "{path}: {:?}", sigs.signed);
        assert!(sigs.usage_rights.is_none(), "{path}");
        let before = std::fs::read(&copy).unwrap();
        let ranges = byte_ranges(&before);
        let page = doc.page(0).unwrap();
        doc.commit_ops(vec![add(page, &note("a comment"))], "comment")
            .unwrap();

        let v = doc.current_version();
        let input = doc.save_input(v).unwrap();
        let planned = plan(&input, &doc.save_target(&copy));
        doc.abandon_save(v);
        let want = match allows {
            Allows::P(1 | 2) => Some(Verdict::Invalid),
            Allows::P(_) => None,
            Allows::Approval => None,
        };
        let asked = planned.asks.iter().find_map(|ask| match ask {
            Ask::Signatures {
                impact,
                append_anyway,
                ..
            } => Some((impact, append_anyway.is_some())),
            _ => None,
        });
        assert_eq!(
            planned.strategy,
            Strategy::Append {
                onto: sheaf_doc::materialize::Onto::Saved
            },
            "{path}"
        );
        match (want, asked) {
            (None, None) => {}
            (Some(verdict), Some((impact, true))) => {
                assert!(
                    impact.iter().any(|i| i.verdict == verdict),
                    "{path}: {impact:?}"
                );
            }
            (want, asked) => panic!("{path}: wanted {want:?}, asked {asked:?}"),
        }

        let answers = AskAnswers {
            append_anyway: want.is_some(),
            ..Default::default()
        };
        let report = save(&mut doc, &copy, &answers, &NoProgress).unwrap();
        assert_eq!(report.strategy, Some(StrategyKind::Append), "{path}");
        let after = std::fs::read(&copy).unwrap();
        assert!(after.starts_with(&before), "{path}: the signed bytes moved");
        assert_eq!(byte_ranges(&after), ranges, "{path}");
        for [a, b, c, d] in &ranges {
            assert!(a + b <= *c && c + d <= before.len(), "{path}: {ranges:?}");
        }
        let again = opened(&copy).unwrap();
        let reread = again.primary().signatures();
        assert_eq!(reread.signed.len(), sigs.signed.len(), "{path}");
        println!(
            "{path} ({allows:?}): {}appended, {} signatures' bytes intact",
            if want.is_some() { "asked, then " } else { "" },
            ranges.len()
        );
        remove(&copy);
    }
    let _ = std::fs::remove_dir_all(&dir);
}
