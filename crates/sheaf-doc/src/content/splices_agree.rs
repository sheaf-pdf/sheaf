//! Temporary (M18 slice 19): sheaf's splice and pdf-content-streams' set
//! side by side over the corpus, before sheaf splices through the
//! family's. For every page and every form XObject of every corpus form
//! and of the pool's sample, each of these is asked of both, and whether
//! it was made or refused, what was taken, and every stream's bytes (and
//! whether a stream is the file's own or a copy) are compared:
//!
//! - (a) a splice of nothing;
//! - (b) each pagination artifact's cuts alone, as a strip takes it;
//! - (c) every artifact of a kind set a filter or a generator's
//!   `replaces` selects, together;
//! - (d) every top-level marked range alone;
//! - (e) `splice_each` over (b)'s groups, its `taken`, its streams, and
//!   `takes` of every marked range;
//! - (f) `without_keys` of every top-level marked range and pagination
//!   artifact, with `/Type` and with sheaf's private list keys, each made
//!   alone, and all made with what (e) takes out.
//!
//! Every disagreement is a row of `target/s19/splice-diff.tsv`. It
//! asserts only that it ran over both groups whole.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fmt::Write as _;
use std::path::Path;
use std::sync::Mutex;
use std::time::Instant;

use super::xobject;
use crate::content::*;
use crate::forms::corpus::{Corpus, Entry};
use crate::pdf::content as fam;
use crate::pdf::{Object, StreamExt};
use crate::private::LIST_KEYS;
use crate::source::Limits;
use crate::store::{BlobKind, Store};

const LIM: &Limits = &Limits::DEFAULT;

/// One disagreement.
struct Row {
    at: String,
    case: &'static str,
    /// What was asked: the range, the kinds.
    detail: String,
    ours: String,
    theirs: String,
    /// The family's error by its variant, where it refused.
    error: String,
    /// The content at the range.
    excerpt: String,
}

/// What one file came to.
#[derive(Default)]
struct Diff {
    missing: bool,
    unopened: bool,
    pages: usize,
    forms: usize,
    /// Pages and forms sheaf can't read or lex, which get (a) alone.
    unread: Vec<String>,
    /// Those whose marked content doesn't balance, which get (a) alone.
    unbalanced: usize,
    /// How each comparison came out, by case.
    tallies: BTreeMap<(&'static str, String), usize>,
    rows: Vec<Row>,
}

/// The family's error by its variant.
fn class(e: &fam::SpliceError) -> String {
    match e {
        fam::SpliceError::Read(e) => format!("Read({:?})", e.kind),
        fam::SpliceError::OutOfRange(_) => "OutOfRange".into(),
        fam::SpliceError::Overlap(_) => "Overlap".into(),
        fam::SpliceError::Unsafe(_) => "Unsafe".into(),
        fam::SpliceError::List(_) => "List".into(),
        fam::SpliceError::Capped(_) => "Capped".into(),
        fam::SpliceError::Reads { .. } => "Reads".into(),
        _ => "other".into(),
    }
}

/// `bytes` on one line.
fn shown(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|&c| match c {
            b'\t' | b'\r' | b'\n' => ' ',
            0x20..=0x7e => c as char,
            _ => '.',
        })
        .collect()
}

/// The content at `span`, with what's before and after it.
fn excerpt(concat: &Concat, span: SpanAcross) -> String {
    let s = concat.bytes();
    let Some(r) = concat.offsets(span) else {
        return String::new();
    };
    let before = &s[r.start.saturating_sub(120)..r.start];
    let after = &s[r.end..(r.end + 160).min(s.len())];
    let inside = &s[r.clone()];
    let inside = if inside.len() <= 600 {
        shown(inside)
    } else {
        format!(
            "{} [...{} bytes...] {}",
            shown(&inside[..360]),
            inside.len() - 560,
            shown(&inside[inside.len() - 200..])
        )
    };
    format!("{} [[ {inside} ]] {}", shown(before), shown(after))
}

type Streams<'a> = Option<Vec<Cow<'a, [u8]>>>;

/// One content, a page's or a form's, with what both are asked with.
struct Side<'a> {
    at: &'a str,
    concat: &'a Concat<'a>,
    res: &'a ResCx,
    lim: fam::Limits,
    out: &'a mut Diff,
}

impl Side<'_> {
    fn tally(&mut self, case: &'static str, verdict: &str) {
        *(self.out.tallies)
            .entry((case, verdict.to_owned()))
            .or_default() += 1;
    }

    fn row(
        &mut self,
        case: &'static str,
        detail: &str,
        span: Option<SpanAcross>,
        got: (&str, &str, &str),
    ) {
        let (ours, theirs, error) = got;
        let verdict = match error {
            "" => format!("DIFFER: sheaf {ours}, the family {theirs}"),
            error => format!("DIFFER: sheaf {ours}, the family refused: {error}"),
        };
        self.tally(case, &verdict);
        self.out.rows.push(Row {
            at: self.at.to_owned(),
            case,
            detail: detail.to_owned(),
            ours: ours.to_owned(),
            theirs: theirs.to_owned(),
            error: error.to_owned(),
            excerpt: span.map_or_else(String::new, |s| excerpt(self.concat, s)),
        });
    }

    /// The two outcomes of one question compared: made or refused, each
    /// stream's bytes, and whether each is the file's own or a copy.
    fn compare(
        &mut self,
        case: &'static str,
        detail: &str,
        span: Option<SpanAcross>,
        ours: Result<Streams<'_>, SpliceError>,
        theirs: Result<Streams<'_>, fam::SpliceError>,
    ) {
        match (ours, theirs) {
            (Err(_), Err(e)) => self.tally(case, &format!("both refused ({})", class(&e))),
            (Ok(_), Err(e)) => {
                let theirs = format!("refused: {e}");
                self.row(case, detail, span, ("made", &theirs, &class(&e)));
            }
            (Err(e), Ok(_)) => {
                let ours = format!("refused: {e}");
                self.row(case, detail, span, (&ours, "made", ""));
            }
            (Ok(None), Ok(None)) => self.tally(case, "both changed nothing"),
            (Ok(Some(_)), Ok(None)) => self.row(case, detail, span, ("changed", "unchanged", "")),
            (Ok(None), Ok(Some(_))) => self.row(case, detail, span, ("unchanged", "changed", "")),
            (Ok(Some(a)), Ok(Some(b))) => {
                let bytes = (a.iter().zip(&b)).position(|(a, b)| **a != **b);
                let copies = (a.iter().zip(&b))
                    .position(|(a, b)| matches!(a, Cow::Owned(_)) != matches!(b, Cow::Owned(_)));
                if a.len() != b.len() {
                    let (a, b) = (a.len().to_string(), b.len().to_string());
                    self.row(case, detail, span, (&a, &b, ""));
                } else if let Some(n) = bytes {
                    let theirs = format!("made, stream {n}'s bytes differ");
                    self.row(case, detail, span, ("made", &theirs, ""));
                } else if let Some(n) = copies {
                    let theirs = format!("made, stream {n} a copy in one, the file's in the other");
                    self.row(case, detail, span, ("made", &theirs, ""));
                } else {
                    self.tally(case, "both made, the same bytes");
                }
            }
        }
    }

    /// `cuts` spliced out by both.
    fn splice(
        &mut self,
        case: &'static str,
        detail: &str,
        span: Option<SpanAcross>,
        cuts: &[SpanAcross],
    ) {
        let ours = splice(self.concat, self.res, LIM, cuts).map(Some);
        let theirs = fam::splice(self.concat, self.res.family(), &self.lim, cuts).map(Some);
        self.compare(case, detail, span, ours, theirs);
    }

    /// Every case, over the top-level marked ranges `tops` and, for a
    /// page, its `blocks`.
    fn cases(&mut self, tops: &[Marked], blocks: Option<&PageBlocks>) {
        let (concat, res) = (self.concat, self.res);
        let lim = self.lim;
        let pagination: &[Pagination] = blocks.map_or(&[], |b| &b.pagination);
        // (b) each pagination artifact's cuts alone.
        let mut groups = Vec::new();
        for p in pagination {
            let Ok(cuts) = p.cuts(concat, res, LIM) else {
                self.tally("b artifact alone", "cuts not found");
                continue;
            };
            let detail = format!("{:?} by {:?} at {}", p.kind, p.origin, p.span);
            self.splice("b artifact alone", &detail, Some(p.span), &cuts);
            if !groups.contains(&cuts) {
                groups.push(cuts);
            }
        }
        // (c) what a filter or a generator's `replaces` selects.
        if let Some(blocks) = blocks {
            let origins = BTreeSet::from(PagOrigin::REPLACED);
            for kinds in [
                BTreeSet::from([PaginationKind::Header, PaginationKind::Footer]),
                BTreeSet::from([PaginationKind::Watermark]),
                BTreeSet::from([PaginationKind::Background]),
            ] {
                let selected: Vec<&Pagination> = blocks.selected(&kinds, &origins).collect();
                let cuts: Vec<SpanAcross> = (selected.iter())
                    .filter_map(|p| p.cuts(concat, res, LIM).ok())
                    .flatten()
                    .collect();
                if cuts.is_empty() {
                    continue;
                }
                let detail = format!("{kinds:?}: {} ranges", selected.len());
                self.splice("c kinds together", &detail, Some(selected[0].span), &cuts);
            }
        }
        // (d) every top-level marked range alone.
        for m in tops {
            let detail = format!("/{} at {}", shown(&m.tag), m.span);
            self.splice("d marked range alone", &detail, Some(m.span), &[m.span]);
        }
        // (e) as many of (b)'s groups as can be.
        let ours = splice_each(concat, res, LIM, &groups);
        let theirs = fam::splice_each(concat, res.family(), &lim, &groups);
        let detail = format!("{} groups", groups.len());
        let bits = |taken: &[bool]| -> String {
            taken.iter().map(|&t| if t { '1' } else { '0' }).collect()
        };
        let theirs = match theirs {
            Ok(theirs) => {
                if ours.taken == theirs.taken {
                    self.tally("e splice_each taken", "both the same");
                } else {
                    let got = (
                        &*format!("took {}", bits(&ours.taken)),
                        &*format!("took {}", bits(&theirs.taken)),
                        "",
                    );
                    self.row("e splice_each taken", &detail, None, got);
                }
                let marked = blocks.map_or(&[][..], |b| &b.marked);
                for m in marked.iter().chain(tops) {
                    if ours.takes(concat, m.span) != theirs.takes(concat, m.span) {
                        let detail = format!("/{} at {}", shown(&m.tag), m.span);
                        let got = (
                            &*ours.takes(concat, m.span).to_string(),
                            &*theirs.takes(concat, m.span).to_string(),
                            "",
                        );
                        self.row("e takes", &detail, Some(m.span), got);
                    }
                }
                let made = theirs.streams(concat, res.family(), &lim, &[]);
                self.compare(
                    "e splice_each streams",
                    &detail,
                    None,
                    Ok(ours.streams(concat, &[])),
                    made,
                );
                Some(theirs)
            }
            Err(e) => {
                let got = (
                    &*format!("took {}", bits(&ours.taken)),
                    &*format!("refused: {e}"),
                    &*class(&e),
                );
                self.row("e splice_each taken", &detail, None, got);
                None
            }
        };
        // (f) lists written again without keys: each alone, then all with
        // what (e) takes out.
        let mut spans: Vec<(SpanAcross, String)> = Vec::new();
        for (span, tag) in (tops.iter().map(|m| (m.span, shown(&m.tag))))
            .chain(pagination.iter().map(|p| (p.span, "Artifact".to_owned())))
        {
            if !spans.iter().any(|(s, _)| *s == span) {
                spans.push((span, tag));
            }
        }
        let nothing = splice_each(concat, res, LIM, &[]);
        let fam_nothing = fam::Spliced::default();
        let type_key: [&[u8]; 1] = [b"Type"];
        for (case, alone, together, keys) in [
            (
                "f without /Type",
                "f without /Type, made alone",
                "f without /Type, made with (e)",
                &type_key[..],
            ),
            (
                "f without private keys",
                "f without private keys, made alone",
                "f without private keys, made with (e)",
                &LIST_KEYS[..],
            ),
        ] {
            let (mut rewrites, mut fam_rewrites) = (Vec::new(), Vec::new());
            for (span, tag) in &spans {
                let detail = format!("/{tag} at {span}");
                let ours = without_keys(concat, res, LIM, *span, keys);
                let theirs = fam::without_keys(concat, res.family(), &lim, *span, keys);
                match (ours, theirs) {
                    (Ok(None), Ok(None)) => self.tally(case, "both: none of the keys"),
                    (Err(_), Err(e)) => self.tally(case, &format!("both refused ({})", class(&e))),
                    (Ok(Some(ours)), Ok(Some(theirs))) => {
                        self.tally(case, "both written again");
                        let made = fam_nothing.streams(
                            concat,
                            res.family(),
                            &lim,
                            std::slice::from_ref(&theirs),
                        );
                        self.compare(
                            alone,
                            &detail,
                            Some(*span),
                            Ok(nothing.streams(concat, std::slice::from_ref(&ours))),
                            made,
                        );
                        rewrites.push(ours);
                        fam_rewrites.push(theirs);
                    }
                    (ours, theirs) => {
                        let say = |made: Option<bool>| match made {
                            Some(true) => "written again",
                            Some(false) => "none of the keys",
                            None => "refused",
                        };
                        let error = theirs.as_ref().err().map(class).unwrap_or_default();
                        let theirs = match &theirs {
                            Ok(r) => say(Some(r.is_some())).to_owned(),
                            Err(e) => format!("refused: {e}"),
                        };
                        let ours = match &ours {
                            Ok(r) => say(Some(r.is_some())).to_owned(),
                            Err(e) => format!("refused: {e}"),
                        };
                        self.row(case, &detail, Some(*span), (&ours, &theirs, &error));
                    }
                }
            }
            if let Some(theirs) = &theirs
                && !rewrites.is_empty()
            {
                let detail = format!("{} lists, {} groups", rewrites.len(), groups.len());
                let made = theirs.streams(concat, res.family(), &lim, &fam_rewrites);
                self.compare(
                    together,
                    &detail,
                    None,
                    Ok(ours.streams(concat, &rewrites)),
                    made,
                );
            }
        }
    }

    /// One content whole: (a), then the rest where sheaf reads it.
    fn content(&mut self, props: &PropertiesDict, blocks: impl FnOnce() -> Option<PageBlocks>) {
        self.splice("a nothing", "", None, &[]);
        let lexes = lex(self.concat.bytes(), self.res, LIM).all(|t| t.is_ok());
        if !lexes {
            self.out.unread.push(format!("{}: doesn't lex", self.at));
            return;
        }
        let Ok(marked) = marked(self.concat, props, self.res, LIM) else {
            self.out.unbalanced += 1;
            return;
        };
        let tops: Vec<Marked> = marked.into_iter().filter(|m| m.depth == 0).collect();
        self.cases(&tops, blocks().as_ref());
    }
}

fn read_file(corpus: &Corpus, entry: &Entry) -> Diff {
    let mut out = Diff::default();
    let Ok(bytes) = std::fs::read(corpus.path(entry)) else {
        out.missing = true;
        return out;
    };
    let kind = BlobKind::file(Path::new(&entry.path), None);
    let Ok(src) = Store::new().open(bytes.into(), kind) else {
        out.unopened = true;
        return out;
    };
    let lim = LIM.content.family();
    let mut forms_seen = HashSet::new();
    let known = DrawnForms::default();
    for (i, page) in src.pages().iter().enumerate() {
        out.pages += 1;
        let at = format!("{}\tpage {}", entry.path, i + 1);
        let content = match PageContent::read(src.pdf(), &page.contents, LIM) {
            Ok(content) => content,
            Err(fault) => {
                out.unread.push(format!("{at}: {fault}"));
                continue;
            }
        };
        let resources = page.resources.as_deref();
        let res = ResCx::read(src.pdf(), resources);
        let concat = content.concat();
        let props = PropertiesDict::read(src.pdf(), resources);
        let mut side = Side {
            at: &at,
            concat,
            res: &res,
            lim,
            out: &mut out,
        };
        side.content(&props, || {
            PageBlocks::of(src.pdf(), &content, resources, LIM, &known).ok()
        });
        // The forms it draws, each once a file.
        let Ok(names) = names_used(concat, &res, LIM, &[]) else {
            continue;
        };
        for name in names.of(ResKind::XObject) {
            let Some(id) = xobject(&src, resources, name) else {
                continue;
            };
            if !forms_seen.insert(id) {
                continue;
            }
            let form = src.pdf().get_object(id).ok().and_then(Object::as_stream);
            let Some(form) =
                form.filter(|f| f.dict.get(b"Subtype").and_then(Object::as_name) == Some(b"Form"))
            else {
                continue;
            };
            out.forms += 1;
            let at = format!("{at}, form {} {} R", id.number, id.generation);
            let Ok(data) = form.decompressed_content_with_limit(LIM.content.decompressed) else {
                out.unread.push(format!("{at}: won't decode"));
                continue;
            };
            let own = form.dict.get(b"Resources").or(resources);
            let res = ResCx::read(src.pdf(), own);
            let props = PropertiesDict::read(src.pdf(), own);
            let one = Concat::new(&[&data]);
            let mut side = Side {
                at: &at,
                concat: &one,
                res: &res,
                lim,
                out: &mut out,
            };
            side.content(&props, || None);
        }
    }
    out
}

/// `read` of each of `entries`, in their order, on twelve threads at
/// most.
fn sweep(entries: &[Entry], read: impl Fn(&Entry) -> Diff + Sync) -> Vec<Diff> {
    let outcomes = Mutex::new(BTreeMap::new());
    let queue = Mutex::new(entries.iter().enumerate());
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get().min(12));
    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| {
                loop {
                    let Some((i, entry)) = queue.lock().unwrap().next() else {
                        break;
                    };
                    let outcome = read(entry);
                    outcomes.lock().unwrap().insert(i, outcome);
                }
            });
        }
    });
    outcomes.into_inner().unwrap().into_values().collect()
}

pub(super) fn run() {
    let Some(corpus) = Corpus::or_skip() else {
        return;
    };
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let pool: Vec<Entry> = std::fs::read_to_string(root.join("bench/pool-sample.txt"))
        .expect("bench/pool-sample.txt")
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|path| Entry {
            path: path.trim().into(),
            ..Entry::default()
        })
        .collect();
    let forms = corpus.forms().unwrap();
    let mut tsv =
        String::from("group\tfile\tpage\tcase\tasked\tsheaf\tfamily\tfamily_error\tcontent\n");
    let mut all_rows = 0;
    let mut by_error: BTreeMap<String, usize> = BTreeMap::new();
    for (group, entries) in [("forms", &forms), ("pool", &pool)] {
        let started = Instant::now();
        let diffs = sweep(entries, |entry| read_file(&corpus, entry));
        let sum = |f: fn(&Diff) -> usize| diffs.iter().map(f).sum::<usize>();
        let rows = sum(|d| d.rows.len());
        let files = diffs.iter().filter(|d| !d.rows.is_empty()).count();
        let pages: BTreeSet<&str> = (diffs.iter().flat_map(|d| &d.rows))
            .map(|r| r.at.split(", form").next().unwrap_or(&r.at))
            .collect();
        let missing = diffs.iter().filter(|d| d.missing).count();
        let unopened: Vec<&str> = (entries.iter().zip(&diffs))
            .filter(|(_, d)| d.unopened)
            .map(|(e, _)| &*e.path)
            .collect();
        eprintln!(
            "{group}: {} files listed, {} read ({} not opened), {} pages and {} forms, in {:?}: \
             {} that sheaf can't read or lex, {} with marked content unbalanced; \
             {rows} disagreements, in {files} files and {} pages",
            entries.len(),
            entries.len() - missing - unopened.len(),
            unopened.len(),
            sum(|d| d.pages),
            sum(|d| d.forms),
            started.elapsed(),
            sum(|d| d.unread.len()),
            sum(|d| d.unbalanced),
            pages.len(),
        );
        for path in &unopened {
            eprintln!("  {group} not opened: {path}");
        }
        for line in diffs.iter().flat_map(|d| &d.unread).take(40) {
            eprintln!("  {group} unread: {}", line.replace('\t', " "));
        }
        let mut tallies: BTreeMap<(&str, &str), usize> = BTreeMap::new();
        for d in &diffs {
            for ((case, verdict), n) in &d.tallies {
                *tallies.entry((case, verdict)).or_default() += n;
            }
            for r in &d.rows {
                if !r.error.is_empty() {
                    *by_error.entry(r.error.clone()).or_default() += 1;
                }
                let line = [
                    group, &r.at, r.case, &r.detail, &r.ours, &r.theirs, &r.error, &r.excerpt,
                ]
                .map(|field| match field == r.at {
                    true => field.to_owned(),
                    false => field.replace('\t', " "),
                });
                writeln!(tsv, "{}", line.join("\t")).unwrap();
            }
        }
        for ((case, verdict), n) in &tallies {
            eprintln!("  {group} ({case}) {verdict}: {n}");
        }
        all_rows += rows;
        assert_eq!(missing, 0, "{group}: files that aren't in the corpus");
        assert!(sum(|d| d.pages) > 0, "{group}: no pages");
    }
    assert_eq!(pool.len(), 402, "the pool's sample");
    eprintln!("{all_rows} disagreements in all; where the family refused what sheaf made:");
    for (error, n) in &by_error {
        eprintln!("  the family's {error}: {n}");
    }
    let dir = root.join("target/s19");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("splice-diff.tsv"), tsv).unwrap();
}
