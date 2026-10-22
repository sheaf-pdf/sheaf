//! The realizer over real files (M10.5 slice 13): over every form of the
//! corpus and a sample of its pool, each file edited (a header on its
//! first, middle and last page, from slice 17 with live page numbers and
//! Bates numbers, and from slice 16 a watermark there too, each replacing
//! others' of its kind on them; another tool's comment deleted and a
//! field's value set where it has them; nothing turned). Two tiers:
//!
//! - **The predicate:** on every page, under each consumer's inputs (the
//!   screen's, a plugin's, print preview's with and without markup, and
//!   OCR's), the realizer's model of an untouched page agrees with what an
//!   overlay of it alone writes ([`emission_is_base`]). A wrong "untouched"
//!   would show the base page in place of the edit.
//! - **The pixels:** each edited page and one page left alone, realized for
//!   a plugin's view (sheaf's marks drawn by PDFium) and loaded from its
//!   segments as the engine pool loads them, render as the saved Rewrite's
//!   at 1,024 px, appearances only: pixel for pixel, or within
//!   [`ANTI_ALIASING_PX`] pixels, each page that differs at all printed. A
//!   base PDFium refuses is told so when an engine over its bytes fails to
//!   open, as the pool tells it, and its pages are viewed again, every one
//!   in its group (Greg, 2026-10-20); a section refused over a base PDFium
//!   reads fails.
//!
//! The field's value only checks that a value change leaves its pages on
//! their base: no save writes `/V` from the model until fills do (M10
//! slice 9), so the predicate ignores it and the Rewrite draws the file's.
//!
//! The pool's sample is slice 2's spikes': the pool sample
//! (`bench/pool-sample.txt`); spike A's encrypted files, the first 75
//! encrypted rows of the pool's inventory, at most 15 an algorithm; and
//! spike B's top-up, the first 50 pool files outside the sample whose
//! header is past offset 0 and the first 50 whose `startxref` leads to no
//! cross-reference section, in inventory order. A damaged base, its
//! cross-references rebuilt, realizes as a file of its own. Counts are
//! printed per class; a difference past the allowance or a disagreement
//! fails, and no file is left out but one sheaf doesn't open.
//!
//! Minutes long, so ignored by default: `cargo test -p sheaf-ops --test
//! realize_corpus -- --ignored --nocapture`. Skips when there's no corpus
//! (see `sheaf_doc::forms::corpus`). Needs libpdfium
//! (`scripts/fetch-pdfium.sh`). To look at a few files by hand, name them
//! in [`ONLY`]; the tiers' own runs leave it unset.

mod common;

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::io::{BufRead, BufReader, Read as _};
use std::path::Path;
use std::sync::Arc;

use lopdf::Object;
use sheaf_doc::forms::corpus::{Corpus, Entry, each_entry};
use sheaf_doc::forms::{FieldKind, Value};
use sheaf_doc::model::{FieldNode, Terminal};
use sheaf_doc::prim::{Key, Prim, Val};
use sheaf_doc::realize::{EngineKey, PageView, Realizer, ViewInputs, emission_is_base, untouched};
use sheaf_doc::serialize::crypt::Crypt;
use sheaf_doc::tx::fixture as fx;
use sheaf_doc::{DocState, Document, PageId};
use sheaf_edit::{BatesSpec, Z};
use sheaf_render::pdfium::{PdfiumEngine, render_appearances};
use sheaf_render::{OpenError, PageIndex};

/// Renders are this wide, as the spikes' were.
const WIDTH: u32 = 1024;

/// The pixels a page's realization may differ by from the Rewrite and
/// pass: PDFium's anti-aliasing at a picture's or a shape's edge. Slice 13
/// found 1 to 9 such pixels on 6 or 7 of 13,289 pages, ≤ 14 levels each,
/// on pages whose dictionary and content streams are the same in both
/// files, and which pages differ changes with what the process drew before
/// (PDFium's process-wide caches), so it isn't sheaf's output. Each page
/// within it is still printed and counted (Greg, 2026-10-20).
const ANTI_ALIASING_PX: usize = 16;

/// Corpus paths, comma-separated, to run a tier on alone
/// (`SHEAF_REALIZE_ONLY=forms/safedocs/4422502.pdf,…`): for looking at a
/// few files by hand. Unset, a tier checks every file it picks.
const ONLY: &str = "SHEAF_REALIZE_ONLY";

/// Spike A's encrypted files: of the inventory's first encrypted rows,
/// as many of each algorithm.
const ENCRYPTED_ROWS: usize = 75;
const PER_ALGORITHM: usize = 15;

/// Spike B's top-up: as many files of each thin class.
const TOP_UP: usize = 50;

/// A file of the sample, and the rules that picked it.
struct Pick {
    path: String,
    why: BTreeSet<&'static str>,
}

/// The pool's inventory, a row a file: its path and whether it's
/// encrypted, in order.
fn inventory(corpus: &Corpus) -> Vec<(String, bool)> {
    let path = corpus.root.join("index/safedocs-inventory.tsv.gz");
    let file = std::fs::File::open(&path).unwrap();
    let mut lines = BufReader::new(flate2::read::GzDecoder::new(file)).lines();
    let header = lines.next().unwrap().unwrap();
    let column = |name: &str| header.split('\t').position(|c| c == name).unwrap();
    let (path, encrypted) = (column("file"), column("encrypted"));
    lines
        .map(|line| {
            let line = line.unwrap();
            let cols: Vec<&str> = line.split('\t').collect();
            (cols[path].to_string(), cols[encrypted] == "1")
        })
        .collect()
}

/// How `crypt` encrypts: its version and revision, and its streams'
/// method (`V2` is RC4, `AESV2` AES-128, `AESV3` AES-256).
fn algorithm(crypt: &Crypt) -> String {
    let get = |key: &[u8]| crypt.dict().get(key).and_then(Object::as_i64).unwrap_or(0);
    let method = crypt
        .state()
        .get_stream_filter()
        .map(|filter| String::from_utf8_lossy(filter.method()).into_owned())
        .unwrap_or_default();
    let bits = get(b"Length");
    format!("V{} R{} {method} {bits}", get(b"V"), get(b"R"))
}

/// The first kilobyte of the file at `path`.
fn head(path: &Path) -> Option<Vec<u8>> {
    let mut head = Vec::new();
    let file = std::fs::File::open(path).ok()?;
    file.take(1024).read_to_end(&mut head).ok()?;
    Some(head)
}

/// Whether the file at `path`'s `startxref` leads to no cross-reference
/// section: neither `xref` nor an object is where it says, counted from
/// the file's start or its header's.
fn leads_nowhere(path: &Path) -> bool {
    let Ok(bytes) = std::fs::read(path) else {
        return false;
    };
    let header = find(&bytes[..bytes.len().min(1024)], b"%PDF-").unwrap_or(0);
    let tail = bytes.len().saturating_sub(2048);
    let Some(at) = rfind(&bytes[tail..], b"startxref").map(|i| tail + i + 9) else {
        return true;
    };
    let digits: String = (bytes[at..].iter())
        .skip_while(|b| b.is_ascii_whitespace())
        .take_while(|b| b.is_ascii_digit())
        .map(|&b| b as char)
        .collect();
    let Ok(start) = digits.parse::<usize>() else {
        return true;
    };
    let section = |pos: usize| {
        let Some(rest) = bytes.get(pos..) else {
            return false;
        };
        let rest = &rest[..rest.len().min(64)];
        let object = regex::bytes::Regex::new(r"^\d+\s+\d+\s+obj").unwrap();
        rest.starts_with(b"xref") || object.is_match(rest)
    };
    !section(start) && !section(start + header)
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

fn rfind(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).rposition(|w| w == needle)
}

/// Every form of the corpus, and the pool's sample: each file once, with
/// every rule that picked it.
fn files(corpus: &Corpus) -> Vec<Pick> {
    let list = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../bench/pool-sample.txt");
    let pool: Vec<String> = (std::fs::read_to_string(list).unwrap().lines())
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect();
    let mut picks: BTreeMap<String, BTreeSet<&'static str>> = BTreeMap::new();
    for entry in corpus.manifest().unwrap() {
        picks.entry(entry.path).or_default().insert("corpus form");
    }
    for path in &pool {
        picks.entry(path.clone()).or_default().insert("pool sample");
    }
    let rows = inventory(corpus);
    // Spike A's: the first encrypted rows, so many an algorithm.
    let mut by_algorithm: BTreeMap<String, usize> = BTreeMap::new();
    for (path, _) in rows.iter().filter(|(_, e)| *e).take(ENCRYPTED_ROWS) {
        let Ok(opened) = sheaf_doc::open::file(&corpus.root.join(path), None) else {
            continue;
        };
        let Some(crypt) = opened.doc.primary().crypt() else {
            continue;
        };
        let taken = by_algorithm.entry(algorithm(crypt)).or_default();
        if *taken < PER_ALGORITHM {
            *taken += 1;
            picks.entry(path.clone()).or_default().insert("encrypted");
        }
    }
    // Spike B's top-up, outside the pool sample.
    let sampled: HashSet<&String> = pool.iter().collect();
    let (mut junk, mut damaged) = (0, 0);
    for (path, _) in &rows {
        if junk >= TOP_UP && damaged >= TOP_UP {
            break;
        }
        if sampled.contains(path) {
            continue;
        }
        let at = corpus.root.join(path);
        let Some(head) = head(&at) else {
            continue;
        };
        if junk < TOP_UP && find(&head, b"%PDF-").is_some_and(|i| i > 0) {
            junk += 1;
            picks
                .entry(path.clone())
                .or_default()
                .insert("leading junk");
        }
        if damaged < TOP_UP && leads_nowhere(&at) {
            damaged += 1;
            picks
                .entry(path.clone())
                .or_default()
                .insert("startxref nowhere");
        }
    }
    (picks.into_iter())
        .map(|(path, why)| Pick { path, why })
        .collect()
}

/// Each consumer's inputs, named.
fn every_input() -> [(&'static str, ViewInputs); 5] {
    [
        ("screen", ViewInputs::screen()),
        ("plugin", ViewInputs::plugin()),
        ("print", ViewInputs::print(true)),
        ("print without markup", ViewInputs::print(false)),
        ("OCR", ViewInputs::content_only()),
    ]
}

/// Which of the tiers a run checks.
#[derive(Clone, Copy, PartialEq)]
enum Tier {
    Predicate,
    Pixels,
}

/// What became of one file.
#[derive(Default)]
struct Outcome {
    /// The classes its base is in.
    classes: BTreeSet<&'static str>,
    /// Why sheaf doesn't open it, when it doesn't.
    unopened: Option<String>,
    /// Why it couldn't be edited, saved, realized or rendered.
    failed: Option<String>,
    /// Pages compared, those that differ past [`ANTI_ALIASING_PX`], and
    /// those that differ within it, with how.
    compared: usize,
    differing: Vec<String>,
    within: Vec<String>,
    /// Pages checked against the emission under each consumer's inputs
    /// (a page once for each), and those it disagrees on.
    checked: usize,
    disagreeing: Vec<String>,
    /// Its groups were files of their own.
    standalone: bool,
    /// PDFium refused its base, so every page was drawn from its group.
    refused: bool,
    /// What it holds that the edits reached: a field whose value changed
    /// and another tool's comment deleted.
    field: bool,
    comment: bool,
}

/// `d`'s state with the first text field that shows on a page holding
/// `value`, as a fill would put it: the pages it shows on, or none when
/// there's no such field.
fn filled(st: &mut DocState, value: &str) -> Vec<PageId> {
    let found = (st.terminals())
        .filter(|(_, _, t)| t.kind == FieldKind::Text && !t.widgets.is_empty())
        .map(|(id, node, _)| (id, Arc::new(node.clone())))
        .next();
    let Some((field, node)) = found else {
        return Vec::new();
    };
    let term = node.term.as_ref().unwrap();
    let pages = (term.widgets.iter())
        .filter_map(|&w| Some(st.annot(w)?.page))
        .collect();
    let filled = FieldNode {
        term: Some(Arc::new(Terminal {
            value: Some(Value::Text(value.into())),
            ..Terminal::clone(term)
        })),
        ..FieldNode::clone(&node)
    };
    let prim = Prim {
        key: Key::Field(field),
        before: Some(Val::Field(node)),
        after: Some(Val::Field(Arc::new(filled))),
    };
    st.apply(&prim).unwrap();
    pages
}

/// The file at `path`: opened, edited, and checked by `tier`.
fn check(path: &Path, tier: Tier) -> Outcome {
    let mut out = Outcome::default();
    let mut d: Document = match sheaf_doc::open::file(path, None) {
        Ok(opened) => opened.doc,
        Err(e) => {
            out.unopened = Some(format!("{e:#}"));
            return out;
        }
    };
    let src = d.primary().clone();
    let xref = src.xref();
    out.classes.insert(match xref.kind {
        lopdf::XrefKind::Table => "classic table",
        lopdf::XrefKind::Stream => "xref stream",
    });
    for (class, is) in [
        ("hybrid", xref.hybrid),
        ("linearized", xref.linearized),
        ("damaged", !src.base_sound()),
        ("leading junk", xref.header_offset > 0),
        ("encrypted", src.crypt().is_some()),
    ] {
        if is {
            out.classes.insert(class);
        }
    }
    if let Err(e) = edit_and_check(&mut d, tier, &mut out) {
        out.failed = Some(format!("{e:#}"));
    }
    out.refused = src.base_refused();
    out.standalone = !src.base_sound() || out.refused;
    out
}

/// Page `page` of `st` viewed under `inputs` by `realizer`, and an engine
/// of its own on its realization, loaded from its segments as the pool
/// loads them. When PDFium refuses an engine over the source's bytes (its
/// base engine, or a group's section over a sound base), its source is
/// told, as the pool's openers tell it, and the page is viewed again, now
/// in its group; but a section refused over a base PDFium reads fails, so
/// a section sheaf wrote wrong isn't drawn standalone unseen.
fn engine_of(
    realizer: &Realizer,
    st: &DocState,
    page: PageId,
    inputs: &ViewInputs,
) -> anyhow::Result<(PageView, PdfiumEngine)> {
    let view = realizer.view(st, page, inputs)?;
    let r = realizer.open(st, view.engine, inputs)?;
    let src = &st.def(page).expect("a page").base.src;
    let over = matches!(view.engine, EngineKey::Base(_)) || src.base_sound();
    match PdfiumEngine::open_segments(r.segments, r.password.as_deref()) {
        Err(e) if OpenError::is_refused(&e) && over && !src.base_refused() => {
            let base = PdfiumEngine::open_bytes(src.bytes().clone(), src.password());
            if !base.is_err_and(|e| OpenError::is_refused(&e)) {
                return Err(e.context("PDFium refuses the section over a base it reads"));
            }
            src.refuse_base();
            engine_of(realizer, st, page, inputs)
        }
        engine => Ok((view, engine?)),
    }
}

/// [`check`]'s work on the opened document `d`.
fn edit_and_check(d: &mut Document, tier: Tier, out: &mut Outcome) -> anyhow::Result<()> {
    let n = d.state().page_count();
    let order: Vec<PageId> = d.state().order().iter().copied().collect();
    let mut covered: Vec<PageId> = [0, n / 2, n - 1].map(|i| order[i]).to_vec();
    covered.dedup();
    let mut spec = fx::header("Realized <<1>> of <<N>>");
    spec.texts[5] = "<<bates>>".into();
    spec.bates = Some(BatesSpec {
        prefix: "R".into(),
        suffix: String::new(),
        digits: 4,
        start: 1,
    });
    let header = fx::set_header(spec, Some(&covered));
    let watermark = fx::set_watermark(fx::watermark("REALIZED"), Z::Over, Some(&covered));
    d.commit_ops(vec![header, watermark], "Header and watermark")?;
    let mut edited: BTreeSet<PageId> = covered.iter().copied().collect();
    let comment = (order.iter()).find_map(|&p| {
        let (id, _) = (d.state().foreign_on(p)).find(|(_, f)| !f.summary.is_link())?;
        Some((p, id))
    });
    if let Some((page, id)) = comment {
        d.commit_ops(vec![fx::delete_annots(&[id])], "Delete comment")?;
        edited.insert(page);
        out.comment = true;
    }
    let mut st = d.state().clone();
    let field = filled(&mut st, "sheaf");
    out.field = !field.is_empty();
    edited.extend(field);
    if tier == Tier::Predicate {
        for &page in &order {
            for (name, inputs) in &every_input() {
                out.checked += 1;
                let model = untouched(&st, page, inputs);
                let emitted = emission_is_base(&st, page, inputs)?;
                if model != emitted {
                    let at = st.pos_of(page).expect("a page");
                    out.disagreeing.push(format!(
                        "page {at}, {name}: the model says {model}, the emission {emitted}"
                    ));
                }
            }
        }
        return Ok(());
    }
    let mut snap = d.snapshot();
    snap.state = st.clone();
    let saved = sheaf_doc::save::rewrite(&snap)?;
    let password = d.primary().password().map(String::from);
    let inputs = ViewInputs::plugin();
    let realizer = Realizer::new();
    let left = order.iter().copied().find(|p| !edited.contains(p));
    for page in edited.iter().copied().chain(left) {
        let (view, engine) = engine_of(&realizer, &st, page, &inputs)?;
        let ours = engine.render_appearances(PageIndex(view.index), WIDTH)?;
        let at = st.pos_of(page).expect("a page") as u32;
        let theirs = render_appearances(&saved, password.as_deref(), PageIndex(at), WIDTH)?;
        out.compared += 1;
        let differ = match (ours.width, ours.height) == (theirs.width, theirs.height) {
            true => (ours.data.chunks(4).zip(theirs.data.chunks(4)))
                .filter(|(a, b)| a != b)
                .count(),
            false => usize::MAX,
        };
        if differ > 0 {
            let edit = if edited.contains(&page) {
                "edited"
            } else {
                "left alone"
            };
            let how = format!("page {at} ({edit}, {:?}): {differ} px", view.engine);
            match differ <= ANTI_ALIASING_PX {
                true => out.within.push(how),
                false => out.differing.push(how),
            }
        }
    }
    Ok(())
}

/// Every page of every file, under each consumer's inputs: the model of
/// an untouched page agrees with the emission.
#[test]
#[ignore = "corpus tier: cargo test -p sheaf-ops --test realize_corpus -- --ignored --nocapture"]
fn the_predicate_agrees_with_the_emission_on_every_page() {
    run(Tier::Predicate);
}

/// Every file: its edited pages and one left alone render from their
/// realizations as the Rewrite does, each within [`ANTI_ALIASING_PX`]
/// pixels of it; a base PDFium refuses (4422502's) draws every page from
/// its group.
#[test]
#[ignore = "corpus tier: cargo test -p sheaf-ops --test realize_corpus -- --ignored --nocapture"]
fn realizations_render_as_saved() {
    run(Tier::Pixels);
}

/// `tier` over every file, its counts printed by class.
fn run(tier: Tier) {
    let Some(corpus) = Corpus::or_skip() else {
        return;
    };
    let picks = common::narrowed(ONLY, files(&corpus), |pick| &pick.path);
    let entries: Vec<Entry> = (picks.iter())
        .map(|pick| Entry {
            path: pick.path.clone(),
            ..Entry::default()
        })
        .collect();
    let started = std::time::Instant::now();
    let outcomes = each_entry(&entries, |e| check(&corpus.path(e), tier));
    let name = match tier {
        Tier::Predicate => "realize_corpus predicate",
        Tier::Pixels => "realize_corpus pixels",
    };
    // Per class: files, pages compared, differing past the allowance and
    // within it, pages checked, disagreeing.
    let mut classes: BTreeMap<&str, [usize; 6]> = BTreeMap::new();
    let mut picked: BTreeMap<&str, usize> = BTreeMap::new();
    let (mut unopened, mut failed, mut standalone, mut refused) = (0, 0, 0, 0);
    let (mut fields, mut comments) = (0, 0);
    let mut problems = Vec::new();
    for (pick, o) in picks.iter().zip(&outcomes) {
        for why in &pick.why {
            *picked.entry(why).or_default() += 1;
        }
        if let Some(e) = &o.unopened {
            unopened += 1;
            eprintln!("doesn't open: {} ({e})", pick.path);
            continue;
        }
        if let Some(e) = &o.failed {
            failed += 1;
            problems.push(format!("{}: {e}", pick.path));
        }
        for d in o.differing.iter().chain(&o.disagreeing) {
            problems.push(format!("{}: {d}", pick.path));
        }
        for w in &o.within {
            eprintln!("{}: {w}, within the anti-aliasing allowance", pick.path);
        }
        standalone += usize::from(o.standalone);
        refused += usize::from(o.refused);
        fields += usize::from(o.field);
        comments += usize::from(o.comment);
        for class in o.classes.iter().copied().chain(["all"]) {
            let counts = classes.entry(class).or_default();
            counts[0] += 1;
            counts[1] += o.compared;
            counts[2] += o.differing.len();
            counts[3] += o.within.len();
            counts[4] += o.checked;
            counts[5] += o.disagreeing.len();
        }
    }
    eprintln!("{name}: {} files picked: {picked:?}", picks.len());
    eprintln!(
        "{name}: {unopened} don't open, {failed} failed; {standalone} realized standalone, \
         {refused} of them refused by PDFium; another tool's comment deleted in {comments}; \
         a field's value changed in {fields} (its pages left on their base: no save writes /V \
         until M10 slice 9)"
    );
    for (class, [files, compared, differing, within, checked, disagreeing]) in &classes {
        match tier {
            Tier::Predicate => eprintln!(
                "{name}: {class}: {files} files, the model disagrees on {disagreeing} of \
                 {checked} pages and inputs"
            ),
            Tier::Pixels => eprintln!(
                "{name}: {class}: {files} files, {} of {compared} pages differ, {within} of them \
                 within the anti-aliasing allowance ({ANTI_ALIASING_PX} px)",
                differing + within
            ),
        }
    }
    eprintln!("{name}: {:.0} s", started.elapsed().as_secs_f64());
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}
