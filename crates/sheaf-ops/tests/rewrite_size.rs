//! What an unsigned Save As writes, by size (object streams' slice 1):
//! every IRS form (`forms/irs/`) and every file of the pool's sample
//! (`bench/pool-sample.txt`), each opened as a session opens it and saved
//! to a new file as `sheaf edit -o` saves it, a Rewrite, what it asks
//! answered as a reader who breaks the signatures and drops the XFA
//! values would. Beside it, qpdf's reference: the same file with its
//! object streams unpacked (`--object-streams=disable`) and packed again
//! (`=generate`), qpdf's other defaults as they are (it recompresses
//! streams, so its unpacked file can be smaller than one unpacked byte
//! for byte).
//!
//! It prints a row per file, tab-separated, then each writer's growth
//! (the output's size over the source's, less one) by group, IRS and
//! pool, and within each by whether the source used object streams as
//! decision 1 has it (its newest cross-reference section a stream, or a
//! hybrid's table): the median and the 90th percentile, by nearest rank,
//! and the worst file. Nothing about size is asserted: slice 2 sets the
//! bound from what packing achieves. What is: every file picked opens but
//! those [`UNOPENED`] names, every save writes, and every output reopens
//! in sheaf with the source's pages, not linearized; and with a qpdf,
//! `qpdf --check` finds every output no worse than its source (it passes
//! where the source passes, and never fails). Without one, it says so and
//! skips qpdf's parts.
//!
//! Ignored, as a corpus tier (`just tier-size`). Skips when there's no
//! corpus (see `sheaf_doc::forms::corpus`).

mod common;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use common::scratch;
use lopdf::XrefKind;
use sheaf_doc::forms::corpus::{Corpus, Entry, each_entry};
use sheaf_doc::materialize::{Ask, AskAnswers, SaveStop, StrategyKind};
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

/// The tier's group a corpus path is in.
fn group(path: &str) -> &'static str {
    match path.starts_with(IRS_DIR) {
        true => "IRS",
        false => "pool",
    }
}

/// What one file's Save As came to.
struct Measured {
    /// Its newest cross-reference section's kind, `stream`, `hybrid` (a
    /// table with an `/XRefStm`) or `table`: the first two used object
    /// streams, as decision 1 has it.
    class: &'static str,
    /// Whether the source's bytes hold `/ObjStm`.
    objstm: bool,
    /// The source's length, and sheaf's output's.
    source: u64,
    sheaf: u64,
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
    fn packs(&self) -> bool {
        self.class != "table"
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
    let xref = session.doc.primary().xref();
    let class = match (xref.kind, xref.hybrid) {
        (XrefKind::Stream, _) => "stream",
        (XrefKind::Table, true) => "hybrid",
        (XrefKind::Table, false) => "table",
    };
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
    if again.primary().xref().linearized {
        return Err("the output says it's linearized".into());
    }
    let mut m = Measured {
        class,
        objstm,
        source: bytes.len() as u64,
        sheaf,
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

/// `growths`' median, 90th percentile and worst, each by nearest rank: one
/// of the files' own.
fn summary(mut growths: Vec<Growth<'_>>) -> String {
    if growths.is_empty() {
        return "no files".into();
    }
    growths.sort_by(|a, b| a.0.total_cmp(&b.0));
    let rank = |p: f64| growths[((p * growths.len() as f64).ceil() as usize).max(1) - 1].0;
    let pct = |g: f64| format!("{:+.1}%", g * 100.0);
    let (worst, path, source, out) = growths[growths.len() - 1];
    format!(
        "of {}: median {}, p90 {}, worst {} {path} ({source} -> {out})",
        growths.len(),
        pct(rank(0.5)),
        pct(rank(0.9)),
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
        let growths: Vec<Growth<'_>> = (rows.iter())
            .filter_map(|&&(path, ref m)| {
                let out = size(m)?;
                Some((out as f64 / m.source as f64 - 1.0, path, m.source, out))
            })
            .collect();
        println!("  {writer:<14} {}", summary(growths));
    }
}

/// Every IRS form and every file of the pool's sample given an unsigned
/// Save As through a session, the path a user takes, and its output's
/// size held beside the source's and qpdf's: a row per file, then the
/// growth by group and by whether the source used object streams. It
/// asserts nothing about size (slice 2 sets the bound), but that every
/// output reopens with the source's pages and, with a qpdf, `qpdf
/// --check` finds it no worse than its source.
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
    for (entry, outcome) in entries.iter().zip(outcomes) {
        let path = entry.path.as_str();
        match outcome {
            Outcome::Unopened(why) => {
                unopened.insert(path, why);
            }
            Outcome::Measured(m) => rows.push((path, m)),
            Outcome::Failed(e) => {
                failed.insert(path, e);
            }
        }
    }
    println!(
        "path\tgroup\tclass\t/ObjStm\tsource\tsheaf\tqpdf unpacked\tqpdf repacked\task\t\
         qpdf --check"
    );
    for (path, m) in &rows {
        let size = |s: Option<u64>| s.map_or("-".into(), |s| s.to_string());
        let check = (m.checked).map_or("-".into(), |[s, o]| format!("{} -> {}", exit(s), exit(o)));
        let asked = match m.asked.is_empty() {
            true => "-".into(),
            false => m.asked.join(", "),
        };
        println!(
            "{path}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{asked}\t{check}",
            group(path),
            m.class,
            if m.objstm { "yes" } else { "no" },
            m.source,
            m.sheaf,
            size(m.unpacked),
            size(m.repacked),
        );
    }
    for g in ["IRS", "pool"] {
        let of: Vec<&(&str, Measured)> = rows.iter().filter(|(path, _)| group(path) == g).collect();
        let packs =
            |p: bool| -> Vec<_> { of.iter().copied().filter(|(_, m)| m.packs() == p).collect() };
        print_growths(&format!("{g}, all"), &of);
        print_growths(&format!("{g}, used object streams"), &packs(true));
        print_growths(&format!("{g}, didn't"), &packs(false));
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
        let listed: Vec<&str> = unopened.keys().copied().collect();
        assert_eq!(listed, UNOPENED, "the files that don't open");
    }
}
