//! What an unsigned Save As writes, by size and by layout (the object
//! streams plan, `plan/complete/object-streams.md`): every IRS form
//! (`forms/irs/`) and every file of the pool's sample
//! (`bench/pool-sample.txt`), each opened as a session opens it and saved
//! to a new file as `sheaf edit -o` saves it, a Rewrite, what it asks
//! answered as a reader who breaks the signatures and drops the XFA
//! values would. Beside it, qpdf's reference: the same file with its
//! object streams unpacked (`--object-streams=disable`) and packed again
//! (`=generate`), qpdf's other defaults as they are (it recompresses
//! streams, so its file can be smaller than one whose streams are copied
//! byte for byte, as sheaf's are).
//!
//! It prints a row per file, tab-separated, then each writer's growth
//! (the output's size over the source's, less one) by group, IRS and
//! pool, and within each by whether the source used object streams as
//! decision 1 has it (its newest cross-reference section a stream, or a
//! hybrid's table): the median and the 90th percentile, by nearest rank,
//! and the worst file.
//!
//! Each output is laid out as decision 1 has it ([`laid_out`]): a source
//! that used object streams writes a cross-reference stream, at version
//! 1.5 or later, with object streams whenever the source held one (a
//! cross-reference stream without any may write none; one whose
//! encryption doesn't pack writes none and keeps its kind, a hybrid
//! written as a table, by decision 4; each is printed, with why); one that
//! didn't writes a table and no object stream. Run on every file it picks,
//! sheaf's growth is held to [`BOUNDS`] by group;
//! each bound is printed, held or not. And every file picked opens but those
//! [`UNOPENED`] names, every save writes, and every output reopens in
//! sheaf with the source's pages, not linearized; and with a qpdf, `qpdf
//! --check` finds every output no worse than its source (it passes where
//! the source passes, and never fails). Without one, it says so and skips
//! qpdf's parts. No file is left out of a group for its size or its
//! layout.
//!
//! Ignored, as a corpus tier (`just tier-size`). Skips when there's no
//! corpus (see `sheaf_doc::forms::corpus`).

mod common;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use common::scratch;
use lopdf::{XrefFacts, XrefKind};
use sheaf_doc::forms::corpus::{Corpus, Entry, each_entry};
use sheaf_doc::materialize::emit::PdfVersion;
use sheaf_doc::materialize::{Ask, AskAnswers, SaveStop, StrategyKind};
use sheaf_doc::serialize::fixture::object_streams_of;
use sheaf_ops::NoProgress;
use sheaf_ops::session::{SaveTo, Session};

/// Corpus paths, comma-separated, to run
/// [`an_unsigned_save_as_s_size_against_its_source`] on alone
/// (`SHEAF_SIZE_ONLY=forms/irs/fw4.pdf,…`): for looking at a few files by
/// hand. Unset, it takes every file it picks.
const SIZE_ONLY: &str = "SHEAF_SIZE_ONLY";

/// Where the IRS forms are, and how many there are, and the pool sample's
/// length: a corpus with other counts isn't laid out as the one the tier
/// was written for.
const IRS_DIR: &str = "forms/irs/";
const IRS: usize = 70;
const POOL: usize = 402;

/// The files picked that sheaf doesn't open, in path order, which the
/// tier leaves out: none, on the corpus it was written for.
const UNOPENED: &[&str] = &[];

/// A bound on sheaf's growth in one of the tier's groups.
struct Bound {
    /// The group, IRS or pool, and whether its sources used object
    /// streams.
    group: &'static str,
    used: bool,
    /// Where in the group, by nearest rank: the median at 0.5, the worst
    /// file at 1.0.
    rank: (&'static str, f64),
    /// The most an output may grow there over its source.
    most: f64,
}

/// sheaf's growth, held where the plan asked (a median under +10% on the
/// IRS forms, no form more than +25% bigger) and tightened toward what
/// packing achieves, with headroom; measured in slice 2's notes.
const BOUNDS: [Bound; 5] = [
    // The IRS forms all used object streams, and each is linearized and
    // carries usage rights (a `/UR3` signature of 13 KB), which a Rewrite
    // drops. Written whole and packed again, the median shrinks 12.9%,
    Bound {
        group: "IRS",
        used: true,
        rank: ("median", 0.5),
        most: -0.05,
    },
    // and the worst, fw2, shrinks 0.9%.
    Bound {
        group: "IRS",
        used: true,
        rank: ("worst", 1.0),
        most: 0.05,
    },
    // The pool's files that used them shrink 0.4% at the median.
    Bound {
        group: "pool",
        used: true,
        rank: ("median", 0.5),
        most: 0.01,
    },
    // The worst, 2705083, grows 2.8%: its twelve signature dictionaries,
    // 13 KB each, which the source packed, stay on their own (decision 3)
    // and uncompressed. Slice 1's worst, 6335597, +114.4% with its object
    // streams unpacked, now shrinks 1.8%: its growth was the unpacking,
    // not its streams copied as they are.
    Bound {
        group: "pool",
        used: true,
        rank: ("worst", 1.0),
        most: 0.05,
    },
    // A table file isn't packed, and writes the size it wrote before
    // packing, file by file: at worst 7439653, +0.4%.
    Bound {
        group: "pool",
        used: false,
        rank: ("worst", 1.0),
        most: 0.005,
    },
];

/// The tier's group a corpus path is in.
fn group(path: &str) -> &'static str {
    match path.starts_with(IRS_DIR) {
        true => "IRS",
        false => "pool",
    }
}

/// A file's newest cross-reference section's kind, `stream`, `hybrid` (a
/// table with an `/XRefStm`) or `table`: the first two used object
/// streams, as decision 1 has it.
fn class(xref: &XrefFacts) -> &'static str {
    match (xref.kind, xref.hybrid) {
        (XrefKind::Stream, _) => "stream",
        (XrefKind::Table, true) => "hybrid",
        (XrefKind::Table, false) => "table",
    }
}

/// What one file's Save As came to.
struct Measured {
    /// The source's [`class`].
    class: &'static str,
    /// Whether the source's bytes hold `/ObjStm`.
    objstm: bool,
    /// Whether its encryption, which the save keeps, lets objects go in
    /// object streams (decision 4: not an Identity `/StmF` under an
    /// encrypting `/StrF`): true when it isn't encrypted.
    packs: bool,
    /// The source's length, and sheaf's output's.
    source: u64,
    sheaf: u64,
    /// The output's [`class`], its header's version, and the object
    /// streams it holds, read as objects.
    written: &'static str,
    version: PdfVersion,
    streams: usize,
    /// qpdf's, unpacked and packed again: None without qpdf, or where it
    /// wrote nothing.
    unpacked: Option<u64>,
    repacked: Option<u64>,
    /// What the save asked, each answered.
    asked: Vec<&'static str>,
    /// How `qpdf --check` exited on the source and on the output (None
    /// when a signal ended it): None without qpdf.
    checked: Option<[Option<i32>; 2]>,
    /// The warnings it gave of the output, its path taken out.
    warned: Vec<String>,
}

impl Measured {
    /// Whether the source used object streams (decision 1).
    fn used(&self) -> bool {
        self.class != "table"
    }
}

/// Whether `m`'s output is laid out as decision 1 has it, an error saying
/// how it isn't: a source that used object streams writes a
/// cross-reference stream at version 1.5 or later, with object streams
/// whenever its bytes held `/ObjStm`; one that didn't writes a table and
/// no object stream. A source that used them and held no `/ObjStm` (a
/// cross-reference stream alone) may write none, and one whose encryption
/// doesn't pack (decision 4) writes none, a stream as a stream and a
/// hybrid as a table: why, to print, when it doesn't.
fn laid_out(m: &Measured) -> Result<Option<&'static str>, String> {
    let (written, version, streams) = (m.written, m.version, m.streams);
    let table = "where its source's cross-references are a table";
    let kept = if m.class == "stream" {
        "stream"
    } else {
        "table"
    };
    match m.used() {
        true if !m.packs && written != kept => Err(format!(
            "a {written}, where its source, a {} whose encryption doesn't pack, is written a {kept}",
            m.class
        )),
        true if !m.packs && streams > 0 => Err(format!(
            "{streams} object streams, where its source's encryption doesn't pack"
        )),
        true if !m.packs => Ok(Some(
            "its encryption, an Identity /StmF under an encrypting /StrF, doesn't pack",
        )),
        true if written != "stream" => {
            Err(format!("a {written}, where its source used object streams"))
        }
        true if version < PdfVersion::XREF_STREAMS => {
            Err(format!("a cross-reference stream at version {version}"))
        }
        true if m.objstm && streams == 0 => {
            Err("no object stream, where its source held some".into())
        }
        true if streams == 0 => Ok(Some(
            "its source, a cross-reference stream, held none either",
        )),
        false if written != "table" => Err(format!("a {written}, {table}")),
        false if streams > 0 => Err(format!("{streams} object streams, {table}")),
        _ => Ok(None),
    }
}

/// What the tier made of a file.
enum Outcome {
    /// sheaf doesn't open it: why.
    Unopened(String),
    Measured(Measured),
    /// What went wrong.
    Failed(String),
}

/// `entry`'s file opened as a session opens it and saved to a new file in
/// `dir` by a Rewrite, and qpdf's two files beside it, each measured, then
/// removed.
fn measured(entry: &Entry, corpus: &Corpus, dir: &Path, qpdf: Option<&Path>) -> Outcome {
    let source = corpus.path(entry);
    let mut session = match Session::open(&source, None, &NoProgress) {
        Ok(session) => session,
        Err(e) => return Outcome::Unopened(format!("{e:#}")),
    };
    let name = entry.path.replace('/', "-");
    let outs = ["sheaf", "unpacked", "repacked"].map(|tag| dir.join(format!("{tag}-{name}")));
    let outcome = measure(&mut session, &source, &outs, qpdf);
    for out in &outs {
        let _ = std::fs::remove_file(out);
    }
    match outcome {
        Ok(m) => Outcome::Measured(m),
        Err(e) => Outcome::Failed(e),
    }
}

/// [`measured`]'s save of `session`, opened from `source`, to `outs[0]`,
/// and qpdf's unpacked and repacked files, to `outs[1]` and `outs[2]`.
fn measure(
    session: &mut Session,
    source: &Path,
    outs: &[PathBuf; 3],
    qpdf: Option<&Path>,
) -> Result<Measured, String> {
    let kind = class(session.doc.primary().xref());
    let packs = (session.doc.primary().crypt()).is_none_or(|c| c.packs());
    let bytes = std::fs::read(source).map_err(|e| format!("reading the source: {e}"))?;
    let objstm = bytes.windows(7).any(|w| w == b"/ObjStm");
    let pages = session.doc.page_count();
    let asked = saved_as(session, &outs[0])?;
    let sheaf = std::fs::metadata(&outs[0])
        .map_err(|e| format!("the output: {e}"))?
        .len();
    let again = (sheaf_doc::open::file(&outs[0], None))
        .map_err(|e| format!("the output doesn't reopen: {e:#}"))?
        .doc;
    if again.page_count() != pages {
        let now = again.page_count();
        return Err(format!("the output has {now} pages, the source {pages}"));
    }
    let out = again.primary();
    if out.xref().linearized {
        return Err("the output says it's linearized".into());
    }
    let version = (PdfVersion::parse(out.pdf().version().as_bytes()))
        .ok_or("the output's header gives no version")?;
    let mut m = Measured {
        class: kind,
        objstm,
        packs,
        source: bytes.len() as u64,
        sheaf,
        written: class(out.xref()),
        version,
        streams: object_streams_of(out.pdf()).len(),
        unpacked: None,
        repacked: None,
        asked,
        checked: None,
        warned: Vec::new(),
    };
    if let Some(qpdf) = qpdf {
        let check = |path: &Path| common::qpdf_said(Command::new(qpdf).arg("--check").arg(path));
        let (was, _) = check(source);
        let (is, said) = check(&outs[0]);
        no_worse([was, is])?;
        m.checked = Some([was, is]);
        let out = outs[0].display().to_string();
        m.warned = (said.lines())
            .filter(|l| l.starts_with("WARNING: "))
            .map(|l| l.replace(&out, "the output"))
            .collect();
        m.unpacked = qpdf_wrote(qpdf, "--object-streams=disable", source, &outs[1]);
        m.repacked = qpdf_wrote(qpdf, "--object-streams=generate", source, &outs[2]);
    }
    Ok(m)
}

/// `session` saved to `out` as Save As saves to another file, a Rewrite:
/// what it asked, each kind answered once (a signature ask by breaking
/// them, an XFA part's values by dropping them) and the save made again.
/// Any other stop is a failure.
fn saved_as(session: &mut Session, out: &Path) -> Result<Vec<&'static str>, String> {
    let mut answers = AskAnswers::default();
    let mut asked = Vec::new();
    loop {
        let to = SaveTo::File {
            path: out.into(),
            append: false,
        };
        match session.save(to, &answers, &NoProgress) {
            Ok(report) if report.strategy == Some(StrategyKind::Rewrite) => return Ok(asked),
            Ok(report) => return Err(format!("saved as {:?}", report.strategy)),
            Err(SaveStop::Ask(Ask::Signatures { .. })) if !answers.break_signatures => {
                answers.break_signatures = true;
                asked.push("signatures");
            }
            Err(SaveStop::Ask(Ask::XfaHoldsValues { .. })) if !answers.drop_xfa_values => {
                answers.drop_xfa_values = true;
                asked.push("XFA values");
            }
            Err(stop) => return Err(format!("the save stopped: {stop}")),
        }
    }
}

/// An error unless the output's `qpdf --check` is no worse than the
/// source's: it exits 0 where the source does, and 3 (warnings) only where
/// the source warns or fails; never 2 (an error), nor by a signal.
fn no_worse([source, output]: [Option<i32>; 2]) -> Result<(), String> {
    let sound = match output {
        Some(0) => true,
        Some(3) => source != Some(0),
        _ => false,
    };
    match sound {
        true => Ok(()),
        false => Err(format!(
            "qpdf --check exits {} on the output, {} on the source",
            exit(output),
            exit(source)
        )),
    }
}

/// How a process exited: its code, or "signal".
fn exit(code: Option<i32>) -> String {
    code.map_or("signal".into(), |c| c.to_string())
}

/// The size of what `qpdf <flag> source to` wrote: None where it wrote
/// nothing (it exits 0, or 3 when it only warns, and writes either way).
fn qpdf_wrote(qpdf: &Path, flag: &str, source: &Path, to: &Path) -> Option<u64> {
    let (code, _) = common::qpdf_said(Command::new(qpdf).arg(flag).arg(source).arg(to));
    match code {
        Some(0 | 3) => std::fs::metadata(to).ok().map(|m| m.len()),
        _ => None,
    }
}

/// A writer's size of a file's output: None where it wrote none.
type Size = fn(&Measured) -> Option<u64>;

/// The writers measured, and each's [`Size`].
const WRITERS: [(&str, Size); 3] = [
    ("sheaf", |m| Some(m.sheaf)),
    ("qpdf unpacked", |m| m.unpacked),
    ("qpdf repacked", |m| m.repacked),
];

/// A file's growth by one writer, its output's size over the source's
/// less one, with the file and the two sizes.
type Growth<'a> = (f64, &'a str, u64, u64);

/// Each of `rows`' growth by the writer of `size`, least first, but where
/// it wrote nothing.
fn growths<'a>(rows: &[&(&'a str, Measured)], size: Size) -> Vec<Growth<'a>> {
    let mut growths: Vec<Growth<'a>> = (rows.iter())
        .filter_map(|&&(path, ref m)| {
            let out = size(m)?;
            Some((out as f64 / m.source as f64 - 1.0, path, m.source, out))
        })
        .collect();
    growths.sort_by(|a, b| a.0.total_cmp(&b.0));
    growths
}

/// The growth at `p` of `growths`, least first and not empty, by nearest
/// rank: one of the files' own.
fn at_rank(growths: &[Growth<'_>], p: f64) -> f64 {
    growths[((p * growths.len() as f64).ceil() as usize).max(1) - 1].0
}

/// `g` as a percentage, its sign shown.
fn pct(g: f64) -> String {
    format!("{:+.1}%", g * 100.0)
}

/// `growths`' median, 90th percentile and worst, each by nearest rank.
fn summary(growths: &[Growth<'_>]) -> String {
    let Some(&(worst, path, source, out)) = growths.last() else {
        return "no files".into();
    };
    format!(
        "of {}: median {}, p90 {}, worst {} {path} ({source} -> {out})",
        growths.len(),
        pct(at_rank(growths, 0.5)),
        pct(at_rank(growths, 0.9)),
        pct(worst),
    )
}

/// One of the summary's lines, `what`, over `rows`: how many hold
/// `/ObjStm`, and each writer's growth.
fn print_growths(what: &str, rows: &[&(&str, Measured)]) {
    let objstm = rows.iter().filter(|(_, m)| m.objstm).count();
    println!(
        "{what}: {} of the files, {objstm} holding /ObjStm",
        rows.len()
    );
    for (writer, size) in WRITERS {
        println!("  {writer:<14} {}", summary(&growths(rows, size)));
    }
}

/// The rows of `rows` in group `g` whose sources used object streams, or
/// didn't, as `used` says.
fn of_group<'r, 'a>(
    rows: &'r [(&'a str, Measured)],
    g: &str,
    used: bool,
) -> Vec<&'r (&'a str, Measured)> {
    (rows.iter())
        .filter(|(path, m)| group(path) == g && m.used() == used)
        .collect()
}

/// `bound` over `rows`: a line to print, saying sheaf's growth where it's
/// set, and whether it holds, which it doesn't for a group with no files.
fn bounded(bound: &Bound, rows: &[(&str, Measured)]) -> (String, bool) {
    let of = of_group(rows, bound.group, bound.used);
    let growths = growths(&of, |m| Some(m.sheaf));
    let (rank, p) = bound.rank;
    let used = if bound.used {
        "used object streams"
    } else {
        "didn't"
    };
    let at = format!(
        "{}, {used}, {rank}, at most {}",
        bound.group,
        pct(bound.most)
    );
    if growths.is_empty() {
        return (format!("{at}: no files"), false);
    }
    let growth = at_rank(&growths, p);
    let holds = growth <= bound.most;
    let line = format!(
        "{at}: {}, {}",
        pct(growth),
        if holds { "held" } else { "MISSED" }
    );
    (line, holds)
}

/// Every IRS form and every file of the pool's sample given an unsigned
/// Save As through a session, the path a user takes, and its output's
/// size held beside the source's and qpdf's: a row per file, then the
/// growth by group and by whether the source used object streams. Every
/// output is laid out as decision 1 has it, reopens with the source's
/// pages and, with a qpdf, `qpdf --check` finds it no worse than its
/// source; run on every file, sheaf's growth is within [`BOUNDS`].
#[test]
#[ignore = "corpus tier: cargo test -p sheaf-ops --test rewrite_size -- --ignored --nocapture"]
fn an_unsigned_save_as_s_size_against_its_source() {
    let Some(corpus) = Corpus::or_skip() else {
        return;
    };
    let qpdf = common::qpdf();
    let started = Instant::now();
    let irs: Vec<Entry> = (corpus.forms().unwrap().into_iter())
        .filter(|e| e.path.starts_with(IRS_DIR))
        .collect();
    assert_eq!(
        irs.len(),
        IRS,
        "forms in {IRS_DIR}: a corpus laid out otherwise?"
    );
    let pool: Vec<Entry> = (common::pool_sample().into_iter())
        .map(|path| Entry {
            path,
            ..Entry::default()
        })
        .collect();
    assert_eq!(pool.len(), POOL, "files in the pool's sample");
    let picks = [irs, pool].concat();
    let whole = picks.len();
    let entries = common::narrowed(SIZE_ONLY, picks, |e| &e.path);
    let dir = scratch("rewrite-size");
    let outcomes = each_entry(&entries, |e| measured(e, &corpus, &dir, qpdf.as_deref()));
    let _ = std::fs::remove_dir_all(&dir);
    let (mut rows, mut unopened, mut failed) = (Vec::new(), BTreeMap::new(), BTreeMap::new());
    let mut unpacked = BTreeMap::new();
    for (entry, outcome) in entries.iter().zip(outcomes) {
        let path = entry.path.as_str();
        match outcome {
            Outcome::Unopened(why) => {
                unopened.insert(path, why);
            }
            Outcome::Measured(m) => {
                match laid_out(&m) {
                    Ok(None) => {}
                    Ok(Some(why)) => {
                        unpacked.insert(path, why);
                    }
                    Err(e) => {
                        failed.insert(path, format!("the output is laid out otherwise: {e}"));
                    }
                }
                rows.push((path, m));
            }
            Outcome::Failed(e) => {
                failed.insert(path, e);
            }
        }
    }
    println!(
        "path\tgroup\tclass\t/ObjStm\tsource\tsheaf\twritten\tversion\tobject streams\t\
         qpdf unpacked\tqpdf repacked\task\tqpdf --check"
    );
    for (path, m) in &rows {
        let size = |s: Option<u64>| s.map_or("-".into(), |s| s.to_string());
        let check = (m.checked).map_or("-".into(), |[s, o]| format!("{} -> {}", exit(s), exit(o)));
        let asked = match m.asked.is_empty() {
            true => "-".into(),
            false => m.asked.join(", "),
        };
        println!(
            "{path}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{asked}\t{check}",
            group(path),
            m.class,
            if m.objstm { "yes" } else { "no" },
            m.source,
            m.sheaf,
            m.written,
            m.version,
            m.streams,
            size(m.unpacked),
            size(m.repacked),
        );
    }
    for g in ["IRS", "pool"] {
        let of: Vec<&(&str, Measured)> = rows.iter().filter(|(path, _)| group(path) == g).collect();
        print_growths(&format!("{g}, all"), &of);
        print_growths(
            &format!("{g}, used object streams"),
            &of_group(&rows, g, true),
        );
        print_growths(&format!("{g}, didn't"), &of_group(&rows, g, false));
    }
    let mut missed = Vec::new();
    for bound in &BOUNDS {
        let (line, holds) = bounded(bound, &rows);
        println!("bound: {line}");
        if !holds {
            missed.push(line);
        }
    }
    for (path, why) in &unpacked {
        println!("no object stream written: {path}: {why}");
    }
    let mut asked: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for (path, m) in &rows {
        for &ask in &m.asked {
            asked.entry(ask).or_default().push(path);
        }
    }
    for (ask, paths) in &asked {
        println!(
            "asked about {ask}, answered: {} ({})",
            paths.len(),
            paths.join(", ")
        );
    }
    match &qpdf {
        None => println!("no qpdf: no qpdf --check, and no qpdf sizes"),
        Some(_) => {
            let mut checks: BTreeMap<[Option<i32>; 2], usize> = BTreeMap::new();
            for (_, m) in &rows {
                *checks.entry(m.checked.unwrap()).or_default() += 1;
            }
            for ([source, output], n) in &checks {
                let [source, output] = [exit(*source), exit(*output)];
                println!("qpdf --check exits {source} on the source, {output} on the output: {n}");
            }
            for (path, m) in rows.iter().filter(|(_, m)| !m.warned.is_empty()) {
                let first = &m.warned[0];
                println!("  {path} warns ({}): {first}", m.warned.len());
            }
            for (writer, size) in &WRITERS[1..] {
                let unwritten: Vec<&str> = (rows.iter())
                    .filter(|(_, m)| size(m).is_none())
                    .map(|(path, _)| *path)
                    .collect();
                println!("{writer}: no file for {} {unwritten:?}", unwritten.len());
            }
        }
    }
    for (path, why) in &unopened {
        println!("doesn't open: {path}: {why}");
    }
    for (path, why) in &failed {
        println!("{path}: {why}");
    }
    println!(
        "{} of {} files measured, {} don't open, {} failed, in {:.0?}",
        rows.len(),
        entries.len(),
        unopened.len(),
        failed.len(),
        started.elapsed(),
    );
    assert!(failed.is_empty(), "{} files failed", failed.len());
    let unlisted: Vec<&str> = (unopened.keys().copied())
        .filter(|p| !UNOPENED.contains(p))
        .collect();
    assert!(unlisted.is_empty(), "files that don't open: {unlisted:?}");
    if entries.len() == whole {
        assert!(missed.is_empty(), "bounds missed: {missed:#?}");
        let listed: Vec<&str> = unopened.keys().copied().collect();
        assert_eq!(listed, UNOPENED, "the files that don't open");
    }
}
