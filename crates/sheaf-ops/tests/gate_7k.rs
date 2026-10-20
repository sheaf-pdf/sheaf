//! The 7k gates (M10.5's "Tests, fixtures and gates"): what today's code
//! takes on the three 7,000-page fixtures, beside slice 2's baselines, and
//! what history costs at that scale. Every test is ignored. Run them in
//! release and one at a time, since peak RSS is the whole process's and
//! the timings would share the machine:
//!
//! ```text
//! cargo test -p sheaf-ops --release --test gate_7k -- --ignored --nocapture --test-threads=1
//! ```
//!
//! The fixtures are `bench/gen.py`'s synthetic, heavy and scanned 7k
//! (`bench/README.md`), kept in `$SHEAF_GATE_DIR`, else `target/gate-7k/`,
//! as `<kind>-7000.pdf`. One that's missing, older than gen.py or not the
//! length slice 2 measured is made again, with python3; the heavy one
//! needs DejaVuSans.ttf (gen.py's `--font`). Beside them go the copy a
//! fixture's document is opened from and saved to, the synthetic's and
//! the heavy's AES-256 copies (`<kind>-7000-aes256.pdf`), and the cache
//! sheaf's generated files would go to, in `cache/`.
//!
//! Each fixture runs in two processes of its own, the second for the
//! flatten, and each prints a table: every row's number now, slice 2's
//! baseline ([`SYNTHETIC`], [`HEAVY`], [`SCANNED`], measured on an AMD
//! EPYC 7302P), the ratio, and the gate a later slice sets on the row.
//! Times are medians. RSS rows are Linux only (`/proc/self`) and read n/a
//! elsewhere; each measurement gives its peak and what it added to the
//! RSS it started from.
//!
//! The rows that run the writer (Rewrite, Bates' Rewrite, flatten and
//! crop's copy) have run up to 1.46× faster in another build of this file,
//! with nothing they run changed, and hold steady within a build. That is
//! code placement: a Rewrite of the heavy 7k is mostly sheaf-doc's own
//! loops around lopdf's load and the writer. The baselines are this file's.
//! From slice 7 the Rewrite, Bates' Rewrite, the flatten and the
//! incremental save write through sheaf-doc's `serialize/` rather than
//! lopdf's writer (crop's copy still goes through lopdf's until slice 15),
//! and the incremental save and the flatten replace their file durably,
//! fsynced with its directory. From slice 9 the Rewrite, Bates' Rewrite
//! and the flatten are the new writer's (`materialize`), the flatten a
//! post-pass on its graph. From slice 10 the incremental save is one mark
//! saved as the app saves it: planned, an Append onto the file in place
//! (the stamp checked again, the journal, the section, four syncs), then
//! abandoned and the file cut back to its length and time, so the next
//! run's record holds; its rows say how much of it is the section, made
//! in memory with every check it has, and how much writing it in place.
//! Beside it, with neither a baseline nor a gate, the same save of a
//! one-page form inserted, whose form check reads the whole file.
//!
//! A gate is asserted from the slice that changes its path: the geometry
//! walk (slice 3), ready (4), Rewrite (9), the incremental save of one mark
//! (10), the history budget (12), realize + render of an edited page,
//! engines after 200 groups and the thumbnail strip (13), crop commit
//! (15), Bates apply and the live-token reorder (17), and the flattened
//! export's RSS (19). The rows today's code has no counterpart for come
//! with those slices; idle CPU is the app's (12, 13). Slice 6's rows are
//! every page's content classified cold (`Source::blocks`), with neither a
//! baseline nor a gate, and a Rewrite of 20,000 of sheaf's marks, some of
//! them text, beside the writer from before marks were drawn through the
//! content toolkit, with no gate; from slice 6 Bates' apply classifies
//! every page, cold, to find an old header to strip. Slice 7's are the
//! incremental save of one mark and the Rewrite again on an AES-256 copy
//! of the synthetic and heavy fixtures, made by lopdf beside them, with
//! neither a baseline nor a gate. Slice 8's is 1,000 mark adds on the
//! heavy 7k, whose annotations and widgets are records from then on,
//! beside slice 1's AnnotAdd and the same adds on blank pages, so an add
//! that scans the records shows. Slice 9's are the Rewrite by
//! `materialize`, from its plan and with its post-conditions, in a process
//! of its own on the synthetic and heavy fixtures, beside slice 8's
//! writer's numbers ([`WriterBaseline`], taken from that slice's build,
//! since slice 9 deleted the writer), and alone in
//! `rewrites_on_the_synthetic_and_heavy_7k`; and, with neither a baseline
//! nor a gate, the opened file's record (part of ready, timed apart), and
//! on every fixture the recovery sidecar of the marked document, from
//! slice 10 an Append section, with the bytes it writes. From slice 11
//! every edit here is ops in a transaction, committed; its rows are
//! `Tx::check` of deleting and of duplicating every page by id, which
//! draw no id, and in the history bench one transaction of 7,000 ops of
//! one kind, lowered and committed, with neither a baseline nor a gate.
//! Slice 12's are a snapshot of each fixture as opened, which planners,
//! dialogs and plugin calls read, and the crop's edit landed by value on
//! the version its dialog opened on after the user deleted a page, and an
//! incremental save of every page turned, each holding a connector of one
//! group; in the history bench, an OCR run as the app makes one, 700 flushes of 10 pages
//! in its group, each landed by value and amending its version, and an
//! insert lowered again after the user moved a page, with neither a
//! baseline nor a gate; and history kept to its budget.
//! So far slice 1's history bench, slice 3's geometry walk, slice 4's
//! ready, slice 5's ready on 20,000 of sheaf's marks, slice 8's mark adds,
//! slice 9's Rewrite, slice 10's incremental save on the scanned 7k
//! ([`INCREMENTAL_GATE_MS`]), slice 11's check of a delete on every
//! fixture ([`CHECK_GATE`]), and slice 12's snapshot ([`SNAPSHOT_GATE`]),
//! flat pushes ([`FLAT`]) and 200 OCR versions within the history budget
//! ([`HISTORY_BUDGET`]) assert, after their tables print.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

use lopdf::Object;
use sheaf_doc::materialize::{AskAnswers, Profile, SaveInput, SaveTarget, materialize};
use sheaf_doc::open::{Facts, FileAnnot, FileAnnots, FileBody};
use sheaf_doc::prim::Key;
use sheaf_doc::serialize::fixture::{USER, encrypt_dict, rc4_aes_128_and_aes_256};
use sheaf_doc::stamps::{Bates, HeaderFooter};
use sheaf_doc::store::BlobKind;
use sheaf_doc::tx::fixture::{add, delete, duplicate, insert, move_pages, ocr, rotate, set_header};
use sheaf_doc::{
    Actor, Annot, AnnotId, AnnotKind, Base, Box2, CommitOpts, Derive, Document, GroupKind, JobKind,
    OcrWord, PageId, Src, Stale, Store, TextLayer,
};
use sheaf_edit::{Anchor, Op, PageRef, Rotation, Style, Turn};
use sheaf_image::raster::PixelFormat;
use sheaf_ops::NoProgress;
use sheaf_ops::crop::{CropSpec, Margins};
use sheaf_ops::derive::DerivedSource;
use sheaf_render::PageIndex;
use sheaf_render::pdfium::PdfiumEngine;
use sheaf_render::{Job, RenderHandle, Renderer};

/// Every fixture's pages.
const PAGES: u32 = 7000;
/// Timed runs of an operation on the whole file, and of a quicker one.
const RUNS: usize = 3;
const QUICK_RUNS: usize = 5;
/// Timed runs of the incremental save of one mark: a sync can take a
/// hundred milliseconds now and then, and the median is the gate's.
const INCREMENTAL_RUNS: usize = 9;
/// The incremental save of one mark on the scanned 7k, at most (slice 10).
const INCREMENTAL_GATE_MS: f64 = 50.0;
/// Timed runs of `Tx::check`, and what one of deleting every page may
/// take (slice 11).
const CHECK_RUNS: usize = 21;
const CHECK_GATE: Duration = Duration::from_millis(1);
/// What a snapshot of the document may take, the median of
/// [`CHECK_RUNS`] (slice 12): O(1), its state shared.
const SNAPSHOT_GATE: Duration = Duration::from_micros(50);
/// Previews rendered in a row from one engine.
const FILL: u32 = 200;
/// Set in the processes a fixture's gates run in (see [`run`]): the file
/// each writes once its rows are done, and which rows those are.
const OWN_PROCESS: &str = "SHEAF_GATE_FIXTURE_PROCESS";
const PART: &str = "SHEAF_GATE_PART";
/// The fixture a process of its own is for, by gen.py's name.
const KIND: &str = "SHEAF_GATE_KIND";
/// What the default policy keeps history to: 200 OCR versions at 1,000
/// pages hold no more (slice 12).
const HISTORY_BUDGET: usize = 256 << 20;

/// A fixture's numbers on slice 2's code, each the median of three runs
/// of the harness: milliseconds, megabytes (MiB) and pages a second. An
/// RSS measurement has its peak (`_rss_mb`) and what it added to the RSS
/// it started from (`_added_mb`).
struct Baseline {
    walk_ms: f64,
    ready_ms: f64,
    ready_rss_mb: f64,
    ready_added_mb: f64,
    cold_render_ms: f64,
    warm_render_ms: f64,
    fill_pages_per_s: f64,
    incremental_ms: f64,
    incremental_rss_mb: f64,
    incremental_added_mb: f64,
    rewrite_ms: f64,
    rewrite_rss_mb: f64,
    rewrite_added_mb: f64,
    rewrite_out_mb: f64,
    crop_ms: f64,
    crop_edit_ms: f64,
    bates_apply_ms: f64,
    bates_rewrite_ms: f64,
    flatten_ms: f64,
    flatten_rss_mb: f64,
    flatten_added_mb: f64,
}

const SYNTHETIC: Baseline = Baseline {
    walk_ms: 46.6,
    ready_ms: 77.1,
    ready_rss_mb: 47.2,
    ready_added_mb: 40.4,
    cold_render_ms: 30.0,
    warm_render_ms: 5.26,
    fill_pages_per_s: 522.0,
    incremental_ms: 56.6,
    incremental_rss_mb: 61.5,
    incremental_added_mb: 35.5,
    rewrite_ms: 300.0,
    rewrite_rss_mb: 78.0,
    rewrite_added_mb: 50.5,
    rewrite_out_mb: 3.27,
    crop_ms: 445.0,
    crop_edit_ms: 1.65,
    bates_apply_ms: 114.0,
    bates_rewrite_ms: 750.0,
    flatten_ms: 336.0,
    flatten_rss_mb: 74.2,
    flatten_added_mb: 63.3,
};

const HEAVY: Baseline = Baseline {
    walk_ms: 59.4,
    ready_ms: 724.0,
    ready_rss_mb: 320.0,
    ready_added_mb: 312.0,
    cold_render_ms: 733.0,
    warm_render_ms: 4.95,
    fill_pages_per_s: 171.0,
    incremental_ms: 166.0,
    incremental_rss_mb: 374.0,
    incremental_added_mb: 216.0,
    rewrite_ms: 3082.0,
    rewrite_rss_mb: 417.0,
    rewrite_added_mb: 248.0,
    rewrite_out_mb: 17.7,
    crop_ms: 2326.0,
    crop_edit_ms: 1.70,
    bates_apply_ms: 270.0,
    bates_rewrite_ms: 3979.0,
    flatten_ms: 3489.0,
    flatten_rss_mb: 420.0,
    flatten_added_mb: 281.0,
};

const SCANNED: Baseline = Baseline {
    walk_ms: 53.0,
    ready_ms: 853.0,
    ready_rss_mb: 1011.0,
    ready_added_mb: 1005.0,
    cold_render_ms: 365.0,
    warm_render_ms: 16.5,
    fill_pages_per_s: 152.0,
    incremental_ms: 742.0,
    incremental_rss_mb: 1032.0,
    incremental_added_mb: 1004.0,
    rewrite_ms: 1201.0,
    rewrite_rss_mb: 1078.0,
    rewrite_added_mb: 1021.0,
    rewrite_out_mb: 473.0,
    crop_ms: 2256.0,
    crop_edit_ms: 1.71,
    bates_apply_ms: 428.0,
    bates_rewrite_ms: 1708.0,
    flatten_ms: 1495.0,
    flatten_rss_mb: 1057.0,
    flatten_added_mb: 1041.0,
};

/// A Rewrite of a fixture as opened on slice 8's writer (`save::rewrite`
/// at that slice's commit, 0fa5cf1), which slice 9 deleted: its time, in
/// milliseconds, and its peak and added RSS, in megabytes, on the AMD
/// EPYC 7302P, each the median of seven processes of its own, each
/// process's time the median of three runs. They come from that commit's
/// release build, its harness given a part that times the old writer as
/// [`rewrite_part`] times `materialize`, run alternately with this file's
/// part. Timed beside `materialize` in one build while both writers were
/// there, the old writer took 133 and 712 ms, so code placement (up to
/// 1.46×, slice 2) moved it under 1% here. The new writer's Rewrite is
/// held to 1.3× this time (flagged past 1.2×) and 1.5× the RSS it adds.
struct WriterBaseline {
    ms: f64,
    rss_mb: f64,
    added_mb: f64,
}

const SLICE8_REWRITE_SYNTHETIC: WriterBaseline = WriterBaseline {
    ms: 134.0,
    rss_mb: 96.5,
    added_mb: 61.7,
};

const SLICE8_REWRITE_HEAVY: WriterBaseline = WriterBaseline {
    ms: 707.0,
    rss_mb: 468.0,
    added_mb: 251.0,
};

/// What the gate allows `materialize`'s Rewrite beside slice 8's writer:
/// its time and the RSS it adds (M10.5 slice 9). The plan's 1.2–1.3× for
/// the time: past 1.2× is flagged, past 1.3× fails, so a check that makes
/// saves sounder can cost a little time.
const REWRITE_TIME_GATE: f64 = 1.3;
const REWRITE_TIME_FLAG: f64 = 1.2;
const REWRITE_ADDED_GATE: f64 = 1.5;

/// Slice 1's history bench on slice 2's code, in milliseconds.
struct MarksBaseline {
    add_ms: f64,
    reorder_ms: f64,
}

const MARKS_7K: MarksBaseline = MarksBaseline {
    add_ms: 0.686,
    reorder_ms: 0.945,
};

/// Ready on the history bench's document saved, its 20,000 marks sheaf's
/// own, on slice 5's code, in milliseconds: the median of three runs of
/// the harness.
const MARKS_7K_READY_MS: f64 = 499.0;

/// A Rewrite of those marks, a fifth of them FreeText and a fifth text
/// stamps, on slice 6's writer before marks were drawn through the content
/// toolkit: the median of three runs of the harness, in milliseconds, and
/// what it added to RSS, in megabytes. Much of the time was lopdf's prune
/// and renumber, which grew with the square of the objects until slice 6
/// patched them (`vendor/lopdf/SHEAF.md`).
const MARKS_7K_REWRITE_MS: f64 = 2314.0;
const MARKS_7K_REWRITE_ADDED_MB: f64 = 346.0;

/// The marks those two save, and how many of them are copies.
const SHEAF_MARKS: u32 = 20_000;
const SHEAF_COPIES: u32 = 10_000;

/// 200 OCR versions of 1,000 pages on slice 2's code: what history holds,
/// by its estimate and by RSS, in megabytes.
struct OcrBaseline {
    versions: f64,
    snapshots: f64,
    snapshot_mb: f64,
    step_mb: f64,
    rss_growth_mb: f64,
}

const OCR_200: OcrBaseline = OcrBaseline {
    versions: 201.0,
    snapshots: 2.0,
    snapshot_mb: 0.016,
    step_mb: 4076.0,
    rss_growth_mb: 4738.0,
};

#[derive(Debug, Clone, Copy, PartialEq)]
enum Kind {
    Synthetic,
    Heavy,
    Scanned,
}

impl Kind {
    /// gen.py's name for it.
    fn name(self) -> &'static str {
        match self {
            Kind::Synthetic => "synthetic",
            Kind::Heavy => "heavy",
            Kind::Scanned => "scanned",
        }
    }

    /// Slice 8's writer's Rewrite (synthetic and heavy only).
    fn slice8_rewrite(self) -> Option<&'static WriterBaseline> {
        match self {
            Kind::Synthetic => Some(&SLICE8_REWRITE_SYNTHETIC),
            Kind::Heavy => Some(&SLICE8_REWRITE_HEAVY),
            Kind::Scanned => None,
        }
    }

    fn baseline(self) -> &'static Baseline {
        match self {
            Kind::Synthetic => &SYNTHETIC,
            Kind::Heavy => &HEAVY,
            Kind::Scanned => &SCANNED,
        }
    }

    /// The fixture's length in bytes when slice 2 measured it (gen.py's
    /// docstring). The heavy's depends on the font gen.py embeds.
    fn bytes(self) -> u64 {
        match self {
            Kind::Synthetic => 3_510_145,
            Kind::Heavy => 14_364_550,
            Kind::Scanned => 496_665_679,
        }
    }
}

#[test]
#[ignore = "gate: cargo test -p sheaf-ops --release --test gate_7k -- --ignored --nocapture --test-threads=1"]
fn gates_on_the_synthetic_7k() {
    run(Kind::Synthetic);
}

#[test]
#[ignore = "gate: cargo test -p sheaf-ops --release --test gate_7k -- --ignored --nocapture --test-threads=1"]
fn gates_on_the_heavy_7k() {
    run(Kind::Heavy);
}

#[test]
#[ignore = "gate: cargo test -p sheaf-ops --release --test gate_7k -- --ignored --nocapture --test-threads=1"]
fn gates_on_the_scanned_7k() {
    run(Kind::Scanned);
}

/// `kind`'s gates, in processes of their own: the test runs its binary
/// again for just itself, for every row but the flatten's, then for
/// those. The allocator keeps some of what a process freed, fragmented,
/// and later peaks would count it: a fixture's after another fixture's,
/// and the flatten's after the edits undone before it, even on the file
/// opened again. Each child writes a file once its rows are done, so a
/// filter that names no test fails rather than passing with nothing run.
fn run(kind: Kind) {
    let test = format!("gates_on_the_{}_7k", kind.name());
    let mut parts = vec!["gates", "flatten"];
    if kind != Kind::Scanned {
        parts.extend(REWRITE_PARTS);
    }
    run_parts(&test, kind, &parts);
}

/// The Rewrite's part, in a process of its own (slice 9's row): the new
/// writer beside slice 8's.
const REWRITE_PARTS: [&str; 1] = ["rewrite"];

/// Each of `parts` of `kind`'s rows in a process of its own, the binary
/// run again for `test` alone (see [`run`]); in such a process, the one
/// part it's for.
fn run_parts(test: &str, kind: Kind, parts: &[&str]) {
    if let Some(done) = std::env::var_os(OWN_PROCESS) {
        if std::env::var(KIND).is_ok_and(|k| k != kind.name()) {
            return;
        }
        match std::env::var(PART).as_deref() {
            Ok("flatten") => flatten(kind),
            Ok("rewrite") => rewrite_part(kind),
            _ => gates(kind),
        }
        std::fs::write(done, "").unwrap();
        return;
    }
    let exe = std::env::current_exe().unwrap();
    for &part in parts {
        let done =
            std::env::temp_dir().join(format!("sheaf-{test}-{part}-{}.done", std::process::id()));
        let status = Command::new(&exe)
            .args([test, "--exact", "--ignored", "--nocapture", "--quiet"])
            .env(OWN_PROCESS, &done)
            .env(PART, part)
            .env(KIND, kind.name())
            .status()
            .unwrap_or_else(|e| panic!("running {test} in its own process: {e}"));
        assert!(status.success(), "{test} ({part}) failed ({status})");
        assert!(
            std::fs::remove_file(&done).is_ok(),
            "{test} ran nothing in its own process: does `{test} --exact` still name it?"
        );
    }
}

/// Every row on the `kind` fixture but the flatten's, printed beside its
/// baseline. Each edit is undone after its rows, so each row edits the
/// file as opened.
fn gates(kind: Kind) {
    let path = fixture(kind);
    let dir = path.parent().expect("the fixture is in the gate directory");
    let cache = dir.join("cache");
    // SAFETY: every test here sets the same value before anything reads it.
    unsafe { std::env::set_var("SHEAF_CACHE_DIR", &cache) };
    // The document is opened from a copy, so the incremental save can
    // write to its own file.
    let work = dir.join(format!("{}-{PAGES}-work.pdf", kind.name()));
    work_copy(&path, &work);
    let base = kind.baseline();
    let file_mb = mb(std::fs::metadata(&path).unwrap().len());
    let middle = PAGES / 2;
    let page = |width| Job::Page {
        width,
        rotation: Rotation::R0,
    };
    let mut rows = Vec::new();

    // The geometry walk: the cross-reference sections and the page table,
    // from bytes already interned, parsed again each run (slice 3; slice
    // 2's baseline is PDFium's page sizes).
    let bytes = sheaf_util::read_file(&path).unwrap();
    let file = || BlobKind::file(&path, None);
    let store = Store::new();
    let blob = store.intern(bytes.clone(), file()).unwrap();
    let (walk, _) = measure(RUNS, || timed(|| store.source(&blob).unwrap()).0);
    drop((blob, store));
    let walk_gate = match kind {
        Kind::Synthetic => Some(Duration::from_millis(100)),
        Kind::Heavy => Some(Duration::from_millis(250)),
        Kind::Scanned => None,
    };
    let gate = walk_gate.map_or(String::new(), |g| {
        format!("< {} ms, asserted", g.as_millis())
    });
    rows.push(Row::time("geometry walk", walk, base.walk_ms, gate));

    // Every page's content classified, as Remove Header/Footer and the
    // watermark pick their pages: each page's streams read, not kept,
    // lexed and its pagination artifacts found, on a source parsed afresh
    // each run, so nothing is remembered (slice 6). No baseline or gate.
    let (classify, _) = measure(RUNS, || {
        let store = Store::new();
        let blob = store.intern(bytes.clone(), file()).unwrap();
        let src = store.source(&blob).unwrap();
        timed(|| {
            for i in 0..src.page_count() {
                src.blocks(i).unwrap();
            }
        })
        .0
    });
    drop(bytes);
    rows.push(Row::time(
        "classify every page (blocks), cold",
        classify,
        f64::NAN,
        "",
    ));

    // Ready: open to edits enabled, as the app opens a file and every
    // headless caller does: the source read, the document started on its
    // pages, the facts read and the opened version made of them (slice 4).
    // The app's PDFium engine opens beside it for the first paint, and
    // isn't counted.
    let mut opened = None;
    let (ready, ready_mem) = measure(RUNS, || {
        drop(opened.take());
        let (t, o) = timed(|| sheaf_doc::open::file(&work, None).unwrap());
        opened = Some(o.doc);
        t
    });
    let mut doc = opened.expect("opened above");
    let ready_gate = (kind == Kind::Heavy).then_some(Duration::from_secs(1));
    let gate = ready_gate.map_or(String::new(), |g| format!("< {} s, asserted", g.as_secs()));
    rows.push(Row::time("ready", ready, base.ready_ms, gate));
    rows.push(Row::peak("ready", ready_mem, base.ready_rss_mb, ""));
    rows.push(Row::added("ready", ready_mem, base.ready_added_mb, ""));
    // The opened file's record, which the open synthesizes (slice 9): part
    // of ready, shown on its own.
    let primary = doc.primary().clone();
    let opened = doc.saved_record().expect("an opened file's record");
    let (file, stamp) = (opened.path.clone(), opened.stamp);
    let version = doc.current_version();
    let (record, _) = measure(QUICK_RUNS, || {
        let file = file.clone();
        timed(|| sheaf_doc::materialize::SaveRecord::opened(version, &primary, file, stamp)).0
    });
    rows.push(Row::time(
        "opened file's record (in ready)",
        record,
        f64::NAN,
        "",
    ));
    // A snapshot of the document, as a planner, a dialog or a plugin call
    // takes one, and the app at every change for plugins (slice 12): its
    // state's tables shared, not copied.
    let (snapshot, _) = measure(CHECK_RUNS, || timed(|| doc.snapshot()).0);
    rows.push(Row {
        what: "snapshot".into(),
        now: Some(snapshot.as_secs_f64() * 1e6),
        unit: "µs",
        baseline: f64::NAN,
        gate: format!("< {} µs, asserted", SNAPSHOT_GATE.as_micros()),
    });

    // `Tx::check` of deleting every page, each named by its id, as a menu
    // asks before it offers an op (§6.3): every reference resolved and the
    // actor's capability checked, nothing lowered (slice 11). Duplicating
    // them is checked too: lowered, it would draw 7,000 page ids, and the
    // check draws none.
    let every: Vec<PageId> = doc.state().order().iter().copied().collect();
    let (delete_every, duplicate_every) = (delete(&every), duplicate(&every));
    let next = doc.ids_next();
    let tx = doc.tx(Actor::User);
    let (check, _) = measure(CHECK_RUNS, || timed(|| tx.check(&delete_every).unwrap()).0);
    let (check_duplicate, _) = measure(CHECK_RUNS, || {
        timed(|| tx.check(&duplicate_every).unwrap()).0
    });
    assert_eq!(doc.ids_next(), next, "Tx::check drew ids");
    drop((tx, delete_every, duplicate_every, every));
    rows.push(Row::time(
        "Tx::check, delete every page by id",
        check,
        f64::NAN,
        format!("< {} ms, asserted", CHECK_GATE.as_millis()),
    ));
    rows.push(Row::time(
        "Tx::check, duplicate every page by id",
        check_duplicate,
        f64::NAN,
        "no ids drawn, asserted",
    ));

    // The middle page at 1024 px: on a new engine, then again.
    let mut engine = None;
    let (cold, _) = measure(RUNS, || {
        drop(engine.take());
        let (t, e) = timed(|| {
            let e = PdfiumEngine::open(&path, None).unwrap();
            e.render(PageIndex(middle), page(1024), PixelFormat::Bgra)
                .unwrap();
            e
        });
        engine = Some(e);
        t
    });
    let engine = engine.expect("opened above");
    let (warm, _) = measure(QUICK_RUNS, || {
        timed(|| {
            engine
                .render(PageIndex(middle), page(1024), PixelFormat::Bgra)
                .unwrap()
        })
        .0
    });
    drop(engine);
    rows.push(Row::time(
        "render 1024 px, cold (open + render)",
        cold,
        base.cold_render_ms,
        "",
    ));
    rows.push(Row::time(
        "render 1024 px, warm",
        warm,
        base.warm_render_ms,
        "",
    ));

    // Previews of the pages from the middle on, as the thumbnail strip
    // renders them (320 px, the app's PREVIEW_W).
    let engine = PdfiumEngine::open(&path, None).unwrap();
    let (fill, ()) = timed(|| {
        for i in middle..middle + FILL {
            engine
                .render(PageIndex(i), page(320), PixelFormat::Bgra)
                .unwrap();
        }
    });
    drop(engine);
    rows.push(Row {
        what: format!("{FILL} previews at 320 px"),
        now: Some(f64::from(FILL) / fill.as_secs_f64()),
        unit: "pages/s",
        baseline: base.fill_pages_per_s,
        gate: String::new(),
    });

    // An incremental save of one mark, as the app saves to the file it
    // opened (slice 10): planned, an Append onto the file in place, then
    // abandoned and the file cut back. Asserted on the scanned fixture.
    let middle_id = doc.page(middle as usize).unwrap();
    doc.commit_ops(vec![add(middle_id, &square(0))], "mark")
        .unwrap();
    let gate = match kind {
        Kind::Scanned => format!("< {INCREMENTAL_GATE_MS} ms, asserted"),
        _ => String::new(),
    };
    let (incremental, incremental_rows) = incremental_rows(&mut doc, &work, Some(base), gate);
    rows.extend(incremental_rows);
    doc.undo().unwrap();

    // An incremental save that writes form keys: a one-page form inserted
    // at the end, which an Append writes with the form, so its form check
    // reads the whole file and the section (`forms::check_latest`, slice
    // 10's review). Informational: the plan gates only one mark.
    let form = dir.join("one-field-form.pdf");
    let mut f = sheaf_doc::forms::fixture::Form::new(1);
    f.text("field", 0, sheaf_doc::forms::fixture::row(0));
    std::fs::write(&form, f.bytes()).unwrap();
    let (src, _) = doc.store().read_file(&form, None).unwrap();
    let one = insert(&src, [0], "one-field-form.pdf", None);
    doc.commit_ops(vec![one], "form").unwrap();
    let (with_form, with_form_mem) = measure(RUNS, || planned_and_put_back(&mut doc, &work));
    doc.undo().unwrap();
    rows.push(Row::time(
        "incremental save, a one-page form",
        with_form,
        f64::NAN,
        "",
    ));
    rows.push(Row::added(
        "incremental save, a one-page form",
        with_form_mem,
        f64::NAN,
        "",
    ));

    // An incremental save of every page turned, each page holding a
    // connector of one group, so each page's turn takes the group into the
    // section (slice 12's review: once for the group, not once a page).
    // Informational.
    let ids: Vec<PageId> = doc.state().order().iter().copied().collect();
    let pairs = ids.as_chunks::<2>().0.iter().map(|&[a, b]| connector(a, b));
    doc.commit_ops(pairs.collect(), "connectors").unwrap();
    doc.commit_ops(vec![rotate(&ids, Turn::Quarter)], "turn")
        .unwrap();
    let (turned, _) = measure(RUNS, || planned_and_put_back(&mut doc, &work));
    doc.undo().unwrap();
    doc.undo().unwrap();
    rows.push(Row::time(
        "incremental save, all turned, a connector each",
        turned,
        f64::NAN,
        "",
    ));

    // The recovery sidecar of one mark, an Append section against the
    // file. Informational.
    doc.commit_ops(vec![add(middle_id, &square(1))], "mark")
        .unwrap();
    let input = doc.recovery_input();
    let (file, stamp) = (doc.file()).map(|(f, s)| (f.clone(), *s)).unwrap();
    let mut sidecar = 0;
    let (recovery, recovery_mem) = measure(RUNS, || {
        let (t, n) =
            timed(|| sheaf_doc::recovery::write(&input, &file, &stamp, &NoProgress).unwrap());
        sidecar = n;
        t
    });
    sheaf_doc::recovery::discard(&work);
    drop(input);
    doc.undo().unwrap();
    rows.push(Row::time(
        "recovery sidecar, one mark (Append section)",
        recovery,
        f64::NAN,
        "",
    ));
    rows.push(Row::peak("recovery sidecar", recovery_mem, f64::NAN, ""));
    rows.push(Row {
        what: "recovery sidecar written".into(),
        now: Some(sidecar as f64 / 1024.0),
        unit: "KB",
        baseline: f64::NAN,
        gate: String::new(),
    });

    // The writer, `materialize` from slice 9, beside slice 2's.
    let snap = doc.snapshot();
    let mut written = 0;
    let (rewrite, rewrite_mem) = measure(RUNS, || {
        let (t, bytes) = timed(|| sheaf_doc::save::rewrite(&snap).unwrap());
        written = bytes.len();
        t
    });
    drop(snap);
    // Its gate is asserted against slice 8's writer in a process of its
    // own (`rewrite_part`), where nothing run before it holds memory.
    let gate = if kind == Kind::Scanned {
        ""
    } else {
        "slice 9's: in its own process"
    };
    rows.push(Row::time("Rewrite", rewrite, base.rewrite_ms, gate));
    rows.push(Row::peak("Rewrite", rewrite_mem, base.rewrite_rss_mb, ""));
    rows.push(Row::added(
        "Rewrite",
        rewrite_mem,
        base.rewrite_added_mb,
        gate,
    ));
    rows.push(Row {
        what: "Rewrite, the file written".into(),
        now: Some(mb(written as u64)),
        unit: "MB",
        baseline: base.rewrite_out_mb,
        gate: String::new(),
    });

    // Crop every page by half an inch a side, as the app does: the cropped
    // copy made in memory and interned, an engine opened on its bytes, then
    // the edit onto it.
    let ids: Vec<PageId> = doc.state().order().iter().copied().collect();
    let half_inch = CropSpec::Margins(Margins {
        top: 36.0,
        right: 36.0,
        bottom: 36.0,
        left: 36.0,
    });
    let mut edits = Vec::new();
    let (crop, _) = measure(RUNS, || {
        let snap = doc.snapshot();
        let (t, (edit, engines)) = timed(|| {
            let plan =
                sheaf_ops::crop::plan(doc.store(), &snap, &ids, half_inch, &NoProgress).unwrap();
            let engines = engines_on(&plan);
            let edit = timed(|| {
                let pages = (plan.iter())
                    .flat_map(|copy| {
                        copy.note(&mut doc);
                        copy.bases()
                    })
                    .collect();
                doc.commit_page_base(pages, Derive::Crop, "Crop").unwrap();
            })
            .0;
            (edit, engines)
        });
        drop(engines);
        edits.push(edit);
        doc.undo().unwrap();
        t
    });
    rows.push(Row::time(
        "crop all pages",
        crop,
        base.crop_ms,
        "< 200 ms, slice 15",
    ));
    rows.push(Row::time(
        "crop all pages, the edit",
        median(edits),
        base.crop_edit_ms,
        "",
    ));
    // The crop's edit as its dialog lands it (slice 12): made on the
    // version the dialog opened on, and committed after the user deleted
    // the last page, by value (`Stale::Validate`): each slot it changes
    // checked against the version it lands on, then the post-merge check.
    // Every page but the deleted one is cropped.
    let opened = doc.snapshot();
    let plan = sheaf_ops::crop::plan(doc.store(), &opened, &ids, half_inch, &NoProgress).unwrap();
    let last = *ids.last().expect("the fixture has pages");
    let bases: Vec<(PageId, Src, u32)> = (plan.iter())
        .flat_map(|copy| {
            copy.note(&mut doc);
            copy.bases()
        })
        .filter(|&(page, ..)| page != last)
        .collect();
    let (validated, _) = measure(RUNS, || {
        doc.commit_ops(vec![delete(&[last])], "Delete page")
            .unwrap();
        let pages = bases.clone();
        let (t, ()) = timed(|| {
            let mut tx = Document::tx_at(&opened, doc.lower_cx(Actor::User));
            tx.set_page_base(pages, Derive::Crop).unwrap();
            let opts = CommitOpts::based(opened.version, Stale::Validate, "Crop");
            doc.commit(tx, opts).unwrap();
        });
        doc.undo().unwrap();
        doc.undo().unwrap();
        t
    });
    drop((plan, bases, opened));
    rows.push(Row::time(
        "crop all pages, validated after a delete",
        validated,
        base.crop_edit_ms,
        "< 200 ms, slice 15",
    ));

    // Bates numbers on every page, as the app applies them, and the
    // Rewrite that draws them. Each apply is a first one, on the file
    // opened afresh (untimed): it classifies every page cold, where
    // another on the same document would find their blocks known.
    let mut texts: [String; 6] = Default::default();
    texts[5] = "<<bates>>".into();
    let bates = HeaderFooter {
        texts,
        bates: Some(Bates {
            prefix: "GATE".into(),
            ..Default::default()
        }),
        ..Default::default()
    };
    let (apply, _) = measure(RUNS, || {
        let mut fresh = sheaf_doc::open::file(&work, None).unwrap().doc;
        let (t, engines) = timed(|| set_header_footer(&mut fresh, &bates));
        drop((engines, fresh));
        t
    });
    let engines = set_header_footer(&mut doc, &bates);
    let snap = doc.snapshot();
    let (drawn, _) = measure(RUNS, || {
        timed(|| sheaf_doc::save::rewrite(&snap).unwrap()).0
    });
    drop((snap, engines, doc));
    rows.push(Row::time(
        "Bates apply, all pages",
        apply,
        base.bates_apply_ms,
        "< 50 ms, slice 17",
    ));
    rows.push(Row::time(
        "Bates, the Rewrite that draws it",
        drawn,
        base.bates_rewrite_ms,
        "",
    ));

    std::fs::remove_file(&work).unwrap();
    if kind != Kind::Scanned {
        rows.extend(aes_256_rows(kind, &path));
    }

    print_table(
        &format!("{} 7k: {}, {file_mb:.1} MB", kind.name(), path.display()),
        "slice 2",
        &rows,
    );
    if let Some(gate) = walk_gate {
        assert!(
            walk < gate,
            "the geometry walk took {walk:?}, over {gate:?}"
        );
    }
    if let Some(gate) = ready_gate {
        assert!(ready < gate, "ready took {ready:?}, over {gate:?}");
    }
    assert!(
        check < CHECK_GATE,
        "Tx::check of deleting every page took {check:?}, over {CHECK_GATE:?}"
    );
    assert!(
        snapshot < SNAPSHOT_GATE,
        "a snapshot took {snapshot:?}, over {SNAPSHOT_GATE:?}"
    );
    if kind == Kind::Scanned {
        assert!(
            ms(incremental) < INCREMENTAL_GATE_MS,
            "the incremental save of one mark took {incremental:?}, over {INCREMENTAL_GATE_MS} ms"
        );
    }
}

/// The incremental save of one mark and the Rewrite on [`aes_256`]'s copy
/// of the `kind` fixture, opened with its password, so every object they
/// write is encrypted, an AES IV a keyed hash of the object's key (slices
/// 7 and 10). Neither has a baseline or a gate: the plain rows are what
/// they cost beside.
fn aes_256_rows(kind: Kind, fixture: &Path) -> Vec<Row> {
    let encrypted = aes_256(kind, fixture);
    let work = fixture.with_file_name(format!("{}-{PAGES}-aes256-work.pdf", kind.name()));
    work_copy(&encrypted, &work);
    let mut doc = sheaf_doc::open::file(&work, Some(USER)).unwrap().doc;
    let snap = doc.snapshot();
    assert!(snap.primary.crypt().is_some(), "the copy isn't encrypted");
    let page = doc.page(PAGES as usize / 2).unwrap();
    doc.commit_ops(vec![add(page, &square(0))], "mark").unwrap();
    let (_, mut rows) = incremental_rows(&mut doc, &work, None, String::new());
    let (rewrite, _) = measure(RUNS, || {
        timed(|| sheaf_doc::save::rewrite(&snap).unwrap()).0
    });
    drop((snap, doc));
    std::fs::remove_file(&work).unwrap();
    rows.push(Row::time("Rewrite, AES-256", rewrite, f64::NAN, ""));
    rows
}

/// The incremental save's rows for `doc`, a document with one mark more
/// than its file at `work` (slice 10), beside `base`'s when it's slice 2's
/// fixture, `gate` on its time: the save as the app makes it
/// ([`planned_and_put_back`]), the median of [`INCREMENTAL_RUNS`], with its
/// peak and added RSS; then the same save's section made in memory
/// ([`section_in_memory`]), and the rest, writing it in place: the stamp
/// checked again, the journal written, the section appended, the journal
/// removed, and the four syncs of the file, the journal and their
/// directory between. The save's median time too, for the gate.
fn incremental_rows(
    doc: &mut Document,
    work: &Path,
    base: Option<&Baseline>,
    gate: String,
) -> (Duration, Vec<Row>) {
    let (saved, mem) = measure(INCREMENTAL_RUNS, || planned_and_put_back(doc, work));
    let (section, _) = measure(INCREMENTAL_RUNS, || section_in_memory(doc, work));
    let writing = saved.saturating_sub(section);
    let what = match base {
        Some(_) => "incremental save, one mark",
        None => "incremental save, one mark, AES-256",
    };
    let baseline = |b: fn(&Baseline) -> f64| base.map_or(f64::NAN, b);
    let mut rows = vec![Row::time(what, saved, baseline(|b| b.incremental_ms), gate)];
    if base.is_some() {
        rows.push(Row::peak(
            "incremental save",
            mem,
            baseline(|b| b.incremental_rss_mb),
            "",
        ));
        rows.push(Row::added(
            "incremental save",
            mem,
            baseline(|b| b.incremental_added_mb),
            "",
        ));
    }
    rows.push(Row::time("  its section, in memory", section, f64::NAN, ""));
    rows.push(Row::time(
        "  writing it in place (journal, syncs)",
        writing,
        f64::NAN,
        "",
    ));
    rows.push(Row {
        what: "  writing it in place, of the save".into(),
        now: Some(100.0 * writing.as_secs_f64() / saved.as_secs_f64()),
        unit: "%",
        baseline: f64::NAN,
        gate: String::new(),
    });
    (saved, rows)
}

/// How long one save of `doc`'s current version to `work`, its file, takes
/// as the app saves (`save_input`, `plan`, `write_file`): planned, which
/// must be an Append onto the file, and written so. The save is then
/// abandoned, and the file cut back to what it was, its time too, so its
/// record's stamp holds again.
fn planned_and_put_back(doc: &mut Document, work: &Path) -> Duration {
    use sheaf_doc::materialize::{Onto, Strategy, plan, write_file};
    let meta = std::fs::metadata(work).unwrap();
    let (len, mtime) = (meta.len(), meta.modified().unwrap());
    let v = doc.current_version();
    let (t, written) = timed(|| {
        let input = doc.save_input(v).unwrap();
        let target = doc.save_target(work);
        let plan = plan(&input, &target);
        assert_eq!(plan.strategy, Strategy::Append { onto: Onto::Saved });
        let answers = AskAnswers::default();
        write_file(&input, &plan, target, &answers, &NoProgress).unwrap()
    });
    assert!(written.wrote, "the save wrote nothing");
    assert_eq!(written.strategy, Strategy::Append { onto: Onto::Saved });
    doc.abandon_save(v);
    let file = std::fs::File::options().write(true).open(work).unwrap();
    file.set_len(len).unwrap();
    file.set_modified(mtime).unwrap();
    file.sync_all().unwrap();
    t
}

/// How long the section of that save takes to make in memory, every check
/// it has included (`save_input`, `plan`, then `append::append` over the
/// file on disk, as `write_file` reads it), the save then abandoned: the
/// save of [`planned_and_put_back`] but for writing it in place.
fn section_in_memory(doc: &mut Document, work: &Path) -> Duration {
    use sheaf_doc::materialize::{append, plan, removal::Mode};
    let v = doc.current_version();
    let (t, ()) = timed(|| {
        let input = doc.save_input(v).unwrap();
        let plan = plan(&input, &doc.save_target(work));
        let file = lopdf::FileBytes::new(std::fs::File::open(work).unwrap()).unwrap();
        let out = append::append(&input, &plan, Arc::new(file), Mode::Veto, &NoProgress).unwrap();
        assert!(!out.bytes.is_empty(), "the section is empty");
    });
    doc.abandon_save(v);
    t
}

/// The flatten's rows on the `kind` fixture, from the file as opened:
/// every markup annotation flattened into its page. Today's flatten keeps
/// links and widgets, so slice 19's `FlattenAnnotations { All }` flattens
/// the heavy's 7,000 widgets beyond what this baseline covers.
fn flatten(kind: Kind) {
    let path = fixture(kind);
    let dir = path.parent().expect("the fixture is in the gate directory");
    let base = kind.baseline();
    let file_mb = mb(std::fs::metadata(&path).unwrap().len());
    let doc = sheaf_doc::open::file(&path, None).unwrap().doc;
    let snap = doc.snapshot();
    let out = dir.join(format!("{}-flattened.pdf", kind.name()));
    let (flatten, mem) = measure(RUNS, || {
        let t = timed(|| sheaf_doc::save::export_flattened(&snap, &out).unwrap()).0;
        std::fs::remove_file(&out).unwrap();
        t
    });
    let limit = 1.5 * file_mb + 200.0;
    let rss_gate = if kind == Kind::Heavy {
        format!("≤ {limit:.0} MB (1.5× file + 200 MB), slice 19")
    } else {
        format!("(1.5× file + 200 MB = {limit:.0} MB)")
    };
    print_table(
        &format!("{} 7k, flatten in a process of its own", kind.name()),
        "slice 2",
        &[
            Row::time("flatten", flatten, base.flatten_ms, ""),
            Row::peak("flatten", mem, base.flatten_rss_mb, rss_gate),
            Row::added("flatten", mem, base.flatten_added_mb, ""),
        ],
    );
}

/// The Rewrite of the `kind` fixture as opened, by `materialize` from its
/// plan, its post-conditions checked, in a process of its own: its time,
/// peak and added RSS, beside slice 8's writer's ([`WriterBaseline`]),
/// asserted within [`REWRITE_TIME_GATE`] of its time, flagged past
/// [`REWRITE_TIME_FLAG`], and within [`REWRITE_ADDED_GATE`] of the RSS it
/// added (slice 9).
fn rewrite_part(kind: Kind) {
    let path = fixture(kind);
    let doc = sheaf_doc::open::file(&path, None).unwrap().doc;
    let snap = doc.snapshot();
    let input = SaveInput::detached(&snap);
    let target = SaveTarget {
        path: path.as_path().into(),
        same_file: false,
        profile: Profile::default(),
        stamp: None,
    };
    let plan = sheaf_doc::materialize::plan(&input, &target);
    let mut written = 0;
    // As slice 8's writer was timed: its output is dropped outside the
    // time, and a save keeps the record.
    let (time, mem) = measure(RUNS, || {
        let (t, out) = timed(|| materialize(&input, &plan, target.clone(), &NoProgress).unwrap());
        written = out.bytes.len();
        t
    });
    let base = kind.slice8_rewrite().expect("synthetic or heavy");
    let what = "Rewrite, materialize";
    let time_gate = REWRITE_TIME_GATE * base.ms;
    let time_flag = REWRITE_TIME_FLAG * base.ms;
    let added_gate = REWRITE_ADDED_GATE * base.added_mb;
    print_table(
        &format!("{} 7k, {what} in a process of its own", kind.name()),
        "slice 8",
        &[
            Row::time(
                what,
                time,
                base.ms,
                format!(
                    "≤ {REWRITE_TIME_GATE}× ({time_gate:.0} ms), asserted; flagged past \
                     {REWRITE_TIME_FLAG}× ({time_flag:.0} ms)"
                ),
            ),
            Row::peak(what, mem, base.rss_mb, ""),
            Row::added(
                what,
                mem,
                base.added_mb,
                format!("≤ {REWRITE_ADDED_GATE}× ({added_gate:.0} MB), asserted"),
            ),
            Row {
                what: format!("{what}, the file written"),
                now: Some(mb(written as u64)),
                unit: "MB",
                baseline: f64::NAN,
                gate: String::new(),
            },
        ],
    );
    if ms(time) > time_flag {
        println!(
            "flag: the Rewrite took {time:?}, past {REWRITE_TIME_FLAG}× slice 8's {} ms",
            base.ms
        );
    }
    assert!(
        ms(time) <= time_gate,
        "the Rewrite took {time:?}, over {REWRITE_TIME_GATE}× slice 8's {} ms",
        base.ms
    );
    if let Some(mem) = mem {
        let added = mb(mem.peak.saturating_sub(mem.start));
        assert!(
            added <= added_gate,
            "the Rewrite added {added:.1} MB, over {REWRITE_ADDED_GATE}× slice 8's {} MB",
            base.added_mb
        );
    }
}

/// The Rewrite's rows alone, the new writer's beside slice 8's, on the
/// synthetic and heavy fixtures, each in a process of its own (see
/// [`run`]): what the gate holds the new writer to.
#[test]
#[ignore = "gate: cargo test -p sheaf-ops --release --test gate_7k -- --ignored --nocapture --test-threads=1"]
fn rewrites_on_the_synthetic_and_heavy_7k() {
    for kind in [Kind::Synthetic, Kind::Heavy] {
        run_parts(
            "rewrites_on_the_synthetic_and_heavy_7k",
            kind,
            &REWRITE_PARTS,
        );
    }
}

/// A render engine on each derived copy, as the app opens one before it
/// takes the copy in as a source.
fn engines_on(copies: &[DerivedSource]) -> Vec<RenderHandle> {
    copies
        .iter()
        .map(|c| RenderHandle::open_bytes(c.src.bytes().clone(), None).unwrap())
        .collect()
}

/// Header & Footer as the app applies it: every source loaded to find the
/// pages whose file already draws one, those moved onto stripped copies,
/// then `hf` set, in one transaction. Gives the engines opened on the
/// copies.
fn set_header_footer(doc: &mut Document, hf: &HeaderFooter) -> Vec<RenderHandle> {
    let snap = doc.snapshot();
    let copies = sheaf_ops::derive::strip_stamps(doc.store(), &snap, &NoProgress).unwrap();
    let engines = engines_on(&copies);
    let pages: Vec<_> = (copies.iter())
        .flat_map(|copy| {
            copy.note(doc);
            copy.bases()
        })
        .collect();
    let mut tx = doc.tx(Actor::User);
    if !pages.is_empty() {
        tx.set_page_base(pages, Derive::StripHeaderFooter).unwrap();
    }
    let set = set_header(hf.spec(), hf.pages.as_deref());
    tx.apply(set, &NoProgress).unwrap();
    doc.commit(tx, CommitOpts::labelled("Header & footer"))
        .unwrap();
    engines
}

/// AddMark (slice 1's AnnotAdd) and a full reorder at 7,000 pages with
/// 20,000 marks, each a transaction committed: medians under 1 ms and 2 ms
/// (M10.5 slice 1). Beside them, with neither a baseline nor a gate, one
/// transaction of 7,000 ops of one kind, an OCR run's layer set on every
/// page, lowered and then committed as an OCR group's version, which
/// History shows as one line (slice 11); an OCR run as the app makes one
/// ([`ocr_run`]): 700 flushes, each amending the group's version, the
/// last ones joining a version of thousands of steps; and an insert of 10
/// pages made on a version, then lowered again on the current one after
/// the user moved a page (slice 12).
#[test]
#[ignore = "gate: cargo test -p sheaf-ops --release --test gate_7k -- --ignored --nocapture --test-threads=1"]
fn history_at_7k_pages_and_20k_marks() {
    let mut d = marked(square);
    let mut adds = Vec::new();
    for i in 0..21 {
        let page = d.page(i * 333).unwrap();
        let start = Instant::now();
        d.commit_ops(vec![add(page, &square(0))], "mark").unwrap();
        adds.push(start.elapsed());
    }
    let mut reorders = Vec::new();
    for _ in 0..5 {
        // The second half before the first page: every page moves.
        let ids: Vec<PageId> = d
            .state()
            .order()
            .iter()
            .skip(PAGES as usize / 2)
            .copied()
            .collect();
        let before = Some(d.page(0).unwrap());
        let start = Instant::now();
        d.commit_ops(vec![move_pages(&ids, before)], "reorder")
            .unwrap();
        reorders.push(start.elapsed());
    }
    let layer = Arc::new(ocr_layer(0));
    let ids: Vec<PageId> = d.state().order().iter().copied().collect();
    let (mut lowered, mut committed) = (Vec::new(), Vec::new());
    for _ in 0..QUICK_RUNS {
        let ops = ocr(ids.iter().map(|&id| (id, layer.clone())));
        let mut tx = d.tx(Actor::Job(JobKind::Ocr));
        let (t, made) = timed(|| tx.apply_batch(ops, &NoProgress).unwrap());
        assert_eq!(made.len(), PAGES as usize);
        lowered.push(t);
        let g = d.begin_group(GroupKind::Ocr);
        let opts = CommitOpts {
            group: Some(g),
            ..CommitOpts::labelled("OCR")
        };
        let (t, c) = timed(|| d.commit(tx, opts).unwrap());
        d.end_group(g);
        committed.push(t);
        let line = format!("Recognize text on {PAGES} pages");
        let lines: Vec<&str> = c.lines.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(lines, [line.as_str()]);
        assert_eq!(d.timeline().pop().unwrap().edits, c.lines);
        d.undo().unwrap();
    }
    let flushes = ocr_run(&mut d, &ids, &layer);
    let run: Duration = flushes.iter().sum();
    let last = median(flushes[flushes.len() - 50..].to_vec());
    let amend = median(flushes[1..].to_vec());
    // An insert planned off the UI thread on the version it began on,
    // landed after the user moved the first page to the end: lowered again
    // on the current version (`Stale::Relower`), drawing the ids it drew.
    let ten = sheaf_doc::store::blank(d.store(), 10);
    let mut relowered = Vec::new();
    for _ in 0..QUICK_RUNS {
        let planned = d.snapshot();
        let before = d.page(PAGES as usize / 2).unwrap();
        let mut tx = Document::tx_at(&planned, d.lower_cx(Actor::Job(JobKind::Insert)));
        tx.apply(insert(&ten, 0..10, "ten.pdf", Some(before)), &NoProgress)
            .unwrap();
        let drew = tx.preview().created;
        let first = d.page(0).unwrap();
        d.commit_ops(vec![move_pages(&[first], None)], "Move page 1")
            .unwrap();
        let opts = CommitOpts::based(planned.version, Stale::Relower, "Insert ten.pdf");
        let (t, c) = timed(|| d.commit(tx, opts).unwrap());
        assert_eq!(c.created, drew);
        relowered.push(t);
        d.undo().unwrap();
        d.undo().unwrap();
    }
    let (add, reorder) = (median(adds), median(reorders));
    print_table(
        "history: 7,000 pages, 20,000 marks",
        "slice 2",
        &[
            Row::time("AddMark", add, MARKS_7K.add_ms, "< 1 ms, asserted"),
            Row::time(
                "a full reorder",
                reorder,
                MARKS_7K.reorder_ms,
                "< 2 ms, asserted",
            ),
            Row::time(
                "7,000 SetOcrText in one Tx, lowered",
                median(lowered),
                f64::NAN,
                "",
            ),
            Row::time(
                "  committed in a group, one History line",
                median(committed),
                f64::NAN,
                "",
            ),
            Row::time("an OCR run, 700 flushes of 10 pages", run, f64::NAN, ""),
            Row::time("  a flush that amends, the median", amend, f64::NAN, ""),
            Row::time("  a flush, the last 50's median", last, f64::NAN, ""),
            Row::time(
                "an insert relowered after a move, the commit",
                median(relowered),
                f64::NAN,
                "",
            ),
        ],
    );
    assert!(add < Duration::from_millis(1), "AddMark took {add:?}");
    assert!(
        reorder < Duration::from_millis(2),
        "a full reorder took {reorder:?}"
    );
}

/// An OCR run over `ids`, as the app makes one: flushes of 10 pages,
/// each made on the version the run began on, deciding by its pages as
/// they were rendered, and landed by value in the run's group, every one
/// after the first amending its version. Each flush's time; the run is
/// undone.
fn ocr_run(d: &mut Document, ids: &[PageId], layer: &Arc<TextLayer>) -> Vec<Duration> {
    let g = d.begin_group(GroupKind::Ocr);
    let base = d.snapshot();
    let mut times = Vec::new();
    for pages in ids.chunks(10) {
        let start = Instant::now();
        let mut tx = Document::tx_at(&base, d.lower_cx(Actor::Job(JobKind::Ocr)));
        for &page in pages {
            tx.read(Key::Page(page));
        }
        let layers = ocr(pages.iter().map(|&page| (page, layer.clone())));
        tx.apply_batch(layers, &NoProgress).unwrap();
        let opts = CommitOpts {
            base: Base::At(base.version),
            stale: Stale::Validate,
            group: Some(g),
            ..CommitOpts::labelled("Recognize text")
        };
        let c = d.commit(tx, opts).unwrap();
        times.push(start.elapsed());
        assert_eq!(c.amended, times.len() > 1);
    }
    d.end_group(g);
    d.undo().unwrap();
    times
}

/// 1,000 AddMarks spread over the heavy 7k's pages, opened as the app
/// opens it, its annotations and widgets records, and the same adds on
/// 7,000 blank pages, which hold none: the heavy's median add under 1 ms,
/// and its medians, over all the adds and over the last 100, within twice
/// the blank's, so no add scans the records (M10.5 slice 8). Each add is
/// a version, and pushing one doesn't walk history (slice 12): on blank
/// pages the last 100 adds' median, with history at the policy's count
/// and weighed at every push, is within [`FLAT`] of the first 100's,
/// with none to drop (slice 8's took 6×). The baseline is slice 1's
/// AnnotAdd on blank pages.
#[test]
#[ignore = "gate: cargo test -p sheaf-ops --release --test gate_7k -- --ignored --nocapture --test-threads=1"]
fn mark_adds_on_the_heavy_7k() {
    let path = fixture(Kind::Heavy);
    let mut heavy = sheaf_doc::open::file(&path, None).unwrap().doc;
    let held = heavy.state().annot_count();
    // A scan shows only against a large table.
    assert!(held >= 20_000, "the heavy 7k holds {held} annotations");
    let heavy = Adds::on(&mut heavy);
    let blank = Adds::on(&mut blank(PAGES));
    let rows = |what: &str, adds: &Adds, gates: [&str; 2]| {
        [
            Row::time(what, adds.median, MARKS_7K.add_ms, gates[0]),
            Row::time(
                &format!("{what}, the first {ENDS}"),
                adds.first,
                MARKS_7K.add_ms,
                "",
            ),
            Row::time(
                &format!("{what}, the last {ENDS}"),
                adds.last,
                MARKS_7K.add_ms,
                gates[1],
            ),
        ]
    };
    let twice = "≤ 2× the blank's, asserted";
    let mut table = Vec::from(rows(
        &format!("AddMark, heavy ({held} held)"),
        &heavy,
        ["< 1 ms and ≤ 2× the blank's, asserted", twice],
    ));
    let flat = format!("≤ {FLAT}× the first {ENDS}'s, asserted");
    table.extend(rows("AddMark, blank (none held)", &blank, ["", &flat]));
    print_table(
        &format!("history: {ADDS} AddMarks on the heavy 7k and on 7,000 blank pages"),
        "slice 2",
        &table,
    );
    let add = heavy.median;
    assert!(add < Duration::from_millis(1), "AddMark took {add:?}");
    assert!(
        add <= blank.median * 2,
        "AddMark took {add:?} on the heavy 7k, {:?} on blank pages",
        blank.median
    );
    assert!(
        heavy.last <= blank.last * 2,
        "the last {ENDS} adds took {:?} on the heavy 7k, {:?} on blank pages",
        heavy.last,
        blank.last
    );
    assert!(
        blank.last <= blank.first.mul_f64(FLAT),
        "on blank pages the last {ENDS} adds took {:?}, the first {:?}",
        blank.last,
        blank.first
    );
}

/// The AddMarks [`mark_adds_on_the_heavy_7k`] makes on each document,
/// and how many at either end it takes a median of.
const ADDS: usize = 1000;
const ENDS: usize = 100;
/// How much more the last [`ENDS`] adds may take than the first, at most
/// (slice 12).
const FLAT: f64 = 1.5;

/// What [`ADDS`] AddMarks took: the median of all, of the first
/// [`ENDS`] and of the last.
struct Adds {
    median: Duration,
    first: Duration,
    last: Duration,
}

impl Adds {
    /// [`ADDS`] squares added to `d`, each its own version, on pages
    /// spread over the document.
    fn on(d: &mut Document) -> Adds {
        let held = d.state().annot_count();
        let step = d.state().page_count() / ADDS;
        let mut adds = Vec::with_capacity(ADDS);
        for i in 0..ADDS {
            let page = d.page(i * step).unwrap();
            let start = Instant::now();
            d.commit_ops(vec![add(page, &square(0))], "mark").unwrap();
            adds.push(start.elapsed());
        }
        assert_eq!(d.state().annot_count(), held + ADDS);
        Adds {
            first: median(adds[..ENDS].to_vec()),
            last: median(adds[ADDS - ENDS..].to_vec()),
            median: median(adds),
        }
    }
}

/// Ready on the history bench's 7,000 pages and 20,000 marks written as
/// sheaf writes them, so they're sheaf's own in the file, the last 10,000
/// copies that say the first 10,000's ids on other pages: each mark's
/// payload read and its digest checked, and the copies given new ids
/// (M10.5 slice 5). The file is written to the gate directory each run,
/// by today's writer. Under the ready gate, 1 s.
#[test]
#[ignore = "gate: cargo test -p sheaf-ops --release --test gate_7k -- --ignored --nocapture --test-threads=1"]
fn ready_at_7k_pages_and_20k_sheaf_marks() {
    let dir = gate_dir();
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("marks-{PAGES}.pdf"));
    std::fs::write(&path, sheaf_marks(square)).unwrap();
    let mut opened = None;
    let (ready, _) = measure(RUNS, || {
        drop(opened.take());
        let (t, o) = timed(|| sheaf_doc::open::file(&path, None).unwrap());
        opened = Some(o);
        t
    });
    let opened = opened.expect("opened above");
    let read = opened.doc.state().marks().count();
    assert_eq!(read, SHEAF_MARKS as usize, "every mark is sheaf's");
    assert_eq!(opened.view.renamed.len(), SHEAF_COPIES as usize);
    print_table(
        &format!(
            "ready: 7,000 pages, 20,000 sheaf marks, {:.1} MB",
            mb(std::fs::metadata(&path).unwrap().len())
        ),
        "slice 5",
        &[Row::time(
            "ready",
            ready,
            MARKS_7K_READY_MS,
            "< 1 s, asserted",
        )],
    );
    assert!(ready < Duration::from_secs(1), "ready took {ready:?}");
}

/// A Rewrite of the history bench's 7,000 pages and 20,000 marks, with
/// every fifth a FreeText and every fifth a text stamp, so the writer
/// draws text as well as squares: each mark's appearance written. The
/// baseline is slice 6's writer before marks were drawn through the
/// content toolkit; nothing is asserted.
#[test]
#[ignore = "gate: cargo test -p sheaf-ops --release --test gate_7k -- --ignored --nocapture --test-threads=1"]
fn rewrite_of_7k_pages_and_20k_sheaf_marks() {
    let d = marked(|id| match id % 5 {
        1 => free_text(id),
        2 => text_stamp(id),
        _ => square(id),
    });
    let snap = d.snapshot();
    let (rewrite, mem) = measure(RUNS, || {
        timed(|| sheaf_doc::save::rewrite(&snap).unwrap()).0
    });
    print_table(
        "Rewrite: 7,000 pages, 20,000 sheaf marks, a fifth FreeText and a fifth text stamps",
        "slice 6",
        &[
            Row::time(
                "Rewrite, 20,000 sheaf marks",
                rewrite,
                MARKS_7K_REWRITE_MS,
                "",
            ),
            Row::added(
                "Rewrite, 20,000 sheaf marks",
                mem,
                MARKS_7K_REWRITE_ADDED_MB,
                "",
            ),
        ],
    );
}

/// The history bench: [`PAGES`] blank pages, ready, with [`SHEAF_MARKS`]
/// marks on them in turn, each `mark(id)` under an id of its own.
fn marked(mark: impl Fn(u64) -> Annot) -> Document {
    let mut annots = FileAnnots::default();
    for i in 0..SHEAF_MARKS {
        annots.pages.entry(i % PAGES).or_default().push(FileAnnot {
            object: None,
            slot: 0,
            body: FileBody::Mark {
                annot: Box::new(mark(u64::from(i) + 1)),
                held: None,
            },
        });
    }
    let store = Store::new();
    let primary = sheaf_doc::store::blank(&store, PAGES);
    let mut d = sheaf_doc::open::start(
        store,
        primary,
        "a.pdf".into(),
        None,
        sheaf_doc::history::default_policy(),
    );
    d.ready(Facts {
        annots: Arc::new(annots),
        ..Default::default()
    })
    .unwrap();
    d
}

/// The history bench's marks on [`PAGES`] blank pages, as a file: each
/// `mark(id)` as sheaf writes it, [`SHEAF_MARKS`] of them, the last
/// [`SHEAF_COPIES`] copies on other pages that say the first ones' ids,
/// as Acrobat's Insert Pages copies them.
fn sheaf_marks(mark: impl Fn(u64) -> Annot) -> Vec<u8> {
    let blank = sheaf_doc::store::blank(&Store::new(), PAGES);
    let mut pdf = lopdf::Document::load_mem(blank.bytes()).unwrap();
    let pages: Vec<lopdf::ObjectId> = pdf.page_iter().collect();
    let mut annots: HashMap<lopdf::ObjectId, Vec<Object>> = HashMap::new();
    for i in 0..SHEAF_MARKS {
        let id = i % (SHEAF_MARKS - SHEAF_COPIES);
        let written = sheaf_doc::annot::to_pdf(&mark(u64::from(id) + 1), &mut pdf);
        let main = pdf.add_object(written.main);
        let page = pages[(i % PAGES) as usize];
        annots.entry(page).or_default().push(main.into());
    }
    for (page, list) in annots {
        pdf.get_dictionary_mut(page).unwrap().set("Annots", list);
    }
    let mut bytes = Vec::new();
    pdf.save_to(&mut bytes).unwrap();
    bytes
}

/// 200 OCR runs over a 1,000-page scan, each a version: what history
/// holds, by its own estimate and by RSS, beside slice 2's. The default
/// policy keeps the estimate (snapshots, steps and undone sources) within
/// [`HISTORY_BUDGET`], pruning versions as runs come (slice 12): asserted
/// after every run.
#[test]
#[ignore = "gate: cargo test -p sheaf-ops --release --test gate_7k -- --ignored --nocapture --test-threads=1"]
fn history_of_200_ocr_versions_at_1000_pages() {
    let mut d = blank(1000);
    let ids: Vec<PageId> = d.state().order().iter().copied().collect();
    let mut most = 0;
    let ((), mem) = with_mem(|| {
        for run in 0..200 {
            let layers = ids
                .iter()
                .enumerate()
                .map(|(i, &id)| (id, Arc::new(ocr_layer(run + i))));
            d.commit_ops(ocr(layers), "OCR").unwrap();
            most = most.max(d.stats().total_bytes());
        }
    });
    let stats = d.stats();
    print_table(
        "history: 200 OCR versions of 1,000 pages, 300 words a page",
        "slice 2",
        &[
            Row {
                what: "versions".into(),
                now: Some(d.version_count() as f64),
                unit: "",
                baseline: OCR_200.versions,
                gate: String::new(),
            },
            Row {
                what: "snapshots kept".into(),
                now: Some(stats.versions as f64),
                unit: "",
                baseline: OCR_200.snapshots,
                gate: String::new(),
            },
            Row {
                what: "snapshot bytes (estimate)".into(),
                now: Some(mb(stats.snapshot_bytes.iter().sum::<usize>() as u64)),
                unit: "MB",
                baseline: OCR_200.snapshot_mb,
                gate: String::new(),
            },
            Row {
                what: "step bytes (estimate)".into(),
                now: Some(mb(stats.step_bytes as u64)),
                unit: "MB",
                baseline: OCR_200.step_mb,
                gate: String::new(),
            },
            Row {
                what: "history (estimate), the most after a run".into(),
                now: Some(mb(most as u64)),
                unit: "MB",
                baseline: OCR_200.snapshot_mb + OCR_200.step_mb,
                gate: "≤ 256 MB, asserted".into(),
            },
            Row {
                what: "RSS growth".into(),
                now: mem.map(|m| mb(m.end.saturating_sub(m.start))),
                unit: "MB",
                baseline: OCR_200.rss_growth_mb,
                gate: String::new(),
            },
        ],
    );
    assert!(
        most <= HISTORY_BUDGET,
        "history held {:.1} MB",
        mb(most as u64)
    );
}

/// The workspace's root: this crate is two below it.
fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/sheaf-ops is two below the workspace")
        .to_path_buf()
}

/// The `kind` fixture, made by `bench/gen.py` when it's missing, older
/// than the generator or not [`Kind::bytes`] long. A synthetic or scanned
/// fixture gen.py makes at another length isn't slice 2's, and fails; a
/// heavy one is another font's, and only warns.
fn fixture(kind: Kind) -> PathBuf {
    let path = gate_dir().join(format!("{}-{PAGES}.pdf", kind.name()));
    let generator = workspace().join("bench/gen.py");
    let len = |p: &Path| std::fs::metadata(p).map(|m| m.len()).ok();
    let modified = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    let stale = match (modified(&path), modified(&generator)) {
        (Some(made), Some(changed)) => made < changed || len(&path) != Some(kind.bytes()),
        _ => true,
    };
    if stale {
        let out = Command::new("python3")
            .arg("-I")
            .arg(&generator)
            .args([kind.name(), &PAGES.to_string()])
            .arg(&path)
            .output()
            .unwrap_or_else(|e| panic!("running python3 for {}: {e}", generator.display()));
        assert!(
            out.status.success(),
            "bench/gen.py {} failed ({}):\n{}",
            kind.name(),
            out.status,
            String::from_utf8_lossy(&out.stderr)
        );
        print!("{}", String::from_utf8_lossy(&out.stdout));
    }
    let made = len(&path).unwrap();
    if made != kind.bytes() {
        let why = format!(
            "{} is {made} bytes, not the {} slice 2 measured",
            path.display(),
            kind.bytes()
        );
        assert!(
            kind == Kind::Heavy,
            "{why}: gen.py makes another fixture now; take the baselines again"
        );
        println!("warning: {why}: gen.py's --font isn't DejaVuSans.ttf, so HEAVY doesn't apply");
    }
    path
}

/// The `kind` fixture encrypted with AES-256, its user password
/// [`USER`], as `<kind>-7000-aes256.pdf` beside it: made by lopdf
/// (`Document::encrypt`, then its writer, which writes no object streams)
/// when it's missing or older than the fixture. A fixture with no `/ID` is
/// given one, as a file with `/Encrypt` must have.
fn aes_256(kind: Kind, fixture: &Path) -> PathBuf {
    let path = fixture.with_file_name(format!("{}-{PAGES}-aes256.pdf", kind.name()));
    let modified = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    if modified(&path) < modified(fixture) {
        let mut pdf = lopdf::Document::load(fixture).unwrap();
        if !pdf.trailer.has(b"ID") {
            let id = Object::string_literal("sheaf 7k gate id");
            pdf.trailer.set("ID", vec![id.clone(), id]);
        }
        let [.., (_, state)] = rc4_aes_128_and_aes_256(&pdf);
        pdf.encrypt(&state).unwrap();
        let at = pdf.trailer.get(b"Encrypt").unwrap().as_reference().unwrap();
        *pdf.get_dictionary_mut(at).unwrap() = encrypt_dict(&state);
        pdf.save(&path).unwrap();
    }
    path
}

/// `fixture` copied to `work`, for a document to be opened from and saved
/// to: synced, so a save's sync doesn't write the copy out too.
fn work_copy(fixture: &Path, work: &Path) {
    std::fs::copy(fixture, work).unwrap();
    std::fs::File::open(work).unwrap().sync_all().unwrap();
}

/// Where the fixtures go: `$SHEAF_GATE_DIR`, else `target/gate-7k/`.
fn gate_dir() -> PathBuf {
    std::env::var_os("SHEAF_GATE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace().join("target/gate-7k"))
}

/// A document of `pages` blank Letter pages, in a store of its own.
fn blank(pages: u32) -> Document {
    let store = Store::new();
    let primary = sheaf_doc::store::blank(&store, pages);
    Document::new(
        store,
        primary,
        "a.pdf".into(),
        None,
        &[],
        sheaf_doc::history::default_policy(),
    )
}

/// A connector pair labelled "1" from page `a` to page `b`: every pair is
/// of one group.
fn connector(a: PageId, b: PageId) -> Op {
    let at = |page| Anchor {
        page: PageRef::Id(page),
        rect: Box2 {
            x: 10.0,
            y: 10.0,
            w: 22.0,
            h: 22.0,
        },
    };
    Op::AddConnector {
        a: at(a),
        b: at(b),
        label: "1".into(),
        style: Style {
            color: [255, 0, 0],
            opacity: 1.0,
            stroke: 1.0,
            fill: None,
            dashed: false,
            cloudy: false,
        },
        locals: Vec::new(),
    }
}

/// A small red square, `id` 0 for the document to assign one.
fn square(id: u64) -> Annot {
    Annot {
        id: AnnotId(id),
        kind: AnnotKind::Square {
            rect: Box2 {
                x: 1.0,
                y: 1.0,
                w: 5.0,
                h: 5.0,
            },
        },
        color: [255, 0, 0],
        opacity: 1.0,
        stroke: 1.0,
        author: "t".into(),
        contents: String::new(),
        modified: String::new(),
        created: String::new(),
        fill: None,
        dashed: false,
        cloudy: false,
        turn: Rotation::R0,
        name: String::new(),
    }
}

/// A FreeText of two short lines, `id` as [`square`]'s.
fn free_text(id: u64) -> Annot {
    Annot {
        kind: AnnotKind::FreeText {
            rect: Box2 {
                x: 20.0,
                y: 20.0,
                w: 120.0,
                h: 40.0,
            },
            size: 10.0,
        },
        contents: "Checked against the ledger, see exhibit 4".into(),
        ..square(id)
    }
}

/// A text stamp, `id` as [`square`]'s.
fn text_stamp(id: u64) -> Annot {
    Annot {
        kind: AnnotKind::TextStamp {
            rect: Box2 {
                x: 200.0,
                y: 20.0,
                w: 110.0,
                h: 36.0,
            },
            text: "APPROVED\nby t".into(),
            status: false,
        },
        ..square(id)
    }
}

/// A Letter page's OCR text: 30 lines of 10 words down the page, picked
/// by `seed`.
fn ocr_layer(seed: usize) -> TextLayer {
    const WORDS: [&str; 8] = [
        "the", "witness", "said", "exhibit", "of", "page", "2024", "counsel",
    ];
    let mut words = Vec::with_capacity(300);
    let mut lines = Vec::with_capacity(30);
    for line in 0..30 {
        let first = words.len();
        for i in 0..10 {
            words.push(OcrWord {
                text: WORDS[(seed + line * 10 + i) % WORDS.len()].into(),
                rect: Box2 {
                    x: 72.0 + 46.0 * i as f32,
                    y: 708.0 - 20.0 * line as f32,
                    w: 42.0,
                    h: 12.0,
                },
                confidence: 0.9,
            });
        }
        lines.push(first..words.len());
    }
    TextLayer {
        words,
        lines,
        upright: Rotation::R0,
        engine: "gate".into(),
    }
}

/// One line of a table.
struct Row {
    what: String,
    /// None where it can't be measured here (RSS off Linux).
    now: Option<f64>,
    unit: &'static str,
    baseline: f64,
    gate: String,
}

impl Row {
    fn time(what: &str, now: Duration, baseline_ms: f64, gate: impl Into<String>) -> Row {
        Row {
            what: what.into(),
            now: Some(ms(now)),
            unit: "ms",
            baseline: baseline_ms,
            gate: gate.into(),
        }
    }

    /// The peak RSS over `what`.
    fn peak(what: &str, mem: Option<Mem>, baseline_mb: f64, gate: impl Into<String>) -> Row {
        Row {
            what: format!("{what}, peak RSS"),
            now: mem.map(|m| mb(m.peak)),
            unit: "MB",
            baseline: baseline_mb,
            gate: gate.into(),
        }
    }

    /// What `what` added to the RSS the process held when it started.
    fn added(what: &str, mem: Option<Mem>, baseline_mb: f64, gate: impl Into<String>) -> Row {
        Row {
            what: format!("{what}, RSS added"),
            now: mem.map(|m| mb(m.peak.saturating_sub(m.start))),
            unit: "MB",
            baseline: baseline_mb,
            gate: gate.into(),
        }
    }
}

/// `rows` under `title`, beside the baselines `against` names.
fn print_table(title: &str, against: &str, rows: &[Row]) {
    println!("\n{title}");
    println!(
        "  {:<44} {:>14} {:>14} {:>6}  gate",
        "", "now", against, "ratio"
    );
    for r in rows {
        let now = r.now.map_or("n/a".into(), |v| amount(v, r.unit));
        let ratio = match r.now {
            Some(v) if r.baseline > 0.0 => format!("{:.2}", v / r.baseline),
            _ => "n/a".into(),
        };
        let baseline = match r.baseline {
            b if b.is_nan() => "n/a".into(),
            b => amount(b, r.unit),
        };
        println!(
            "  {:<44} {:>14} {:>14} {:>6}  {}",
            r.what, now, baseline, ratio, r.gate
        );
    }
}

/// `v` in `unit`, to about three figures; a count (no unit) whole.
fn amount(v: f64, unit: &str) -> String {
    let digits = match v {
        _ if unit.is_empty() => 0,
        v if v < 1.0 => 3,
        v if v < 10.0 => 2,
        v if v < 100.0 => 1,
        _ => 0,
    };
    format!("{v:.digits$} {unit}").trim_end().to_string()
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1e3
}

fn mb(bytes: u64) -> f64 {
    bytes as f64 / f64::from(1 << 20)
}

/// How long `f` takes, and what it returns.
fn timed<T>(f: impl FnOnce() -> T) -> (Duration, T) {
    let start = Instant::now();
    let out = f();
    (start.elapsed(), out)
}

fn median(mut times: Vec<Duration>) -> Duration {
    times.sort();
    times[times.len() / 2]
}

/// `f` run `n` times: the median of the times it returns, and the memory
/// over its first run. `f` times what it measures itself, so it can set
/// up and clean up around it.
fn measure(n: usize, mut f: impl FnMut() -> Duration) -> (Duration, Option<Mem>) {
    let (first, mem) = with_mem(&mut f);
    let mut times = vec![first];
    times.extend((1..n).map(|_| f()));
    (median(times), mem)
}

/// The process's resident memory over a run, in bytes.
#[derive(Debug, Clone, Copy)]
struct Mem {
    start: u64,
    /// The most at any point in the run.
    peak: u64,
    end: u64,
}

/// What `f` returns, and the memory it took: the peak is `VmHWM`, reset
/// to the resident set at the start (`clear_refs` 5). None off Linux.
fn with_mem<T>(f: impl FnOnce() -> T) -> (T, Option<Mem>) {
    release_free_memory();
    let reset = std::fs::write("/proc/self/clear_refs", "5").is_ok();
    let start = status("VmRSS");
    let out = f();
    let mem = match (reset, start, status("VmHWM"), status("VmRSS")) {
        (true, Some(start), Some(peak), Some(end)) => Some(Mem { start, peak, end }),
        _ => None,
    };
    (out, mem)
}

/// Hand the allocator's free pages back to the system, so a run starts
/// from what is live. Without it, what earlier rows and tests freed stays
/// resident, and a run reuses it without its peak moving.
fn release_free_memory() {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    {
        unsafe extern "C" {
            fn malloc_trim(pad: usize) -> std::ffi::c_int;
        }
        // SAFETY: glibc's malloc_trim only releases memory nothing holds.
        unsafe { malloc_trim(0) };
    }
}

/// A field of `/proc/self/status` given in kB, in bytes.
fn status(field: &str) -> Option<u64> {
    kb_field("/proc/self/status", field)
}

/// A field of a `/proc` file that gives it in kB, in bytes.
fn kb_field(file: &str, field: &str) -> Option<u64> {
    let text = std::fs::read_to_string(file).ok()?;
    text.lines().find_map(|line| {
        let kb = line
            .strip_prefix(field)?
            .strip_prefix(':')?
            .trim()
            .strip_suffix(" kB")?;
        kb.parse::<u64>().ok().map(|kb| kb * 1024)
    })
}
