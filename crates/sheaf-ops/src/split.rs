//! Split a document into several files (Document > Split Document…,
//! `sheaf split`): every N pages, by page ranges, at each top-level
//! bookmark, or into files no bigger than a size. Each part is an extract
//! ([`Export::extract`]): it keeps its pages' markup and the bookmarks that
//! point into it.

use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use in_progress::{Level, NoProgress, Progress, Unit};
use serde::{Deserialize, Serialize};
use sheaf_doc::export::{Export, ExportFrom, Out};
use sheaf_doc::materialize::AskAnswers;
use sheaf_doc::{PAGES, PageId, Snapshot};

use crate::session::Decided;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SplitBy {
    /// Parts of this many pages (the last may be shorter).
    EveryN(usize),
    /// One part per range, e.g. ["1-3", "4-10", "11-"].
    Ranges(Vec<String>),
    /// A part per top-level bookmark, from its page to the next one's.
    TopBookmarks,
    /// Parts as large as fit in this many bytes.
    MaxBytes(u64),
}

/// One file to write: its name (no extension) and pages.
#[derive(Debug, Clone, PartialEq)]
pub struct Part {
    pub name: String,
    pub pages: Vec<PageId>,
}

/// The parts `by` makes of `snap`, named after `stem`. Size splits need
/// to measure, so they're planned by [`split`] instead.
pub fn plan(snap: &Snapshot, by: &SplitBy, stem: &str) -> Result<Vec<Part>> {
    let pages: Vec<PageId> = snap.state.order().iter().copied().collect();
    if pages.is_empty() {
        bail!("the document has no pages");
    }
    let numbered = |groups: Vec<Vec<PageId>>| {
        let width = groups.len().to_string().len();
        groups
            .into_iter()
            .enumerate()
            .map(|(i, pages)| Part {
                name: format!("{stem}_Part_{:0width$}", i + 1),
                pages,
            })
            .collect::<Vec<_>>()
    };
    Ok(match by {
        SplitBy::EveryN(n) => {
            if *n == 0 {
                bail!("parts need at least one page");
            }
            numbered(pages.chunks(*n).map(|c| c.to_vec()).collect())
        }
        SplitBy::Ranges(ranges) => {
            let mut groups = Vec::new();
            for r in ranges.iter().filter(|r| !r.trim().is_empty()) {
                let idx = crate::pages::parse(r, pages.len()).map_err(anyhow::Error::msg)?;
                groups.push(idx.into_iter().map(|i| pages[i]).collect());
            }
            if groups.is_empty() {
                bail!("no page ranges given");
            }
            numbered(groups)
        }
        SplitBy::TopBookmarks => by_bookmarks(snap, &pages, stem)?,
        SplitBy::MaxBytes(_) => bail!("a size split is planned while it's written"),
    })
}

fn by_bookmarks(snap: &Snapshot, pages: &[PageId], stem: &str) -> Result<Vec<Part>> {
    // (first page index, title) per top-level bookmark with a target.
    let st = &snap.state;
    let mut starts: Vec<(usize, String)> = (st.outline_roots().iter())
        .filter_map(|&id| {
            let b = st.bookmark(id)?;
            let at = pages.iter().position(|p| Some(*p) == b.page())?;
            Some((at, b.title.to_string()))
        })
        .collect();
    starts.sort_by_key(|(at, _)| *at);
    starts.dedup_by_key(|(at, _)| *at);
    if starts.is_empty() {
        bail!("the document has no top-level bookmarks to split at");
    }
    let mut parts = Vec::new();
    let mut used: Vec<String> = Vec::new();
    let mut name = |title: &str| {
        let base = format!("{stem}_{}", file_safe(title));
        let mut n = base.clone();
        let mut k = 2;
        while used.iter().any(|u| u.eq_ignore_ascii_case(&n)) {
            n = format!("{base}_{k}");
            k += 1;
        }
        used.push(n.clone());
        n
    };
    if starts[0].0 > 0 {
        parts.push(Part {
            name: name("Start"),
            pages: pages[..starts[0].0].to_vec(),
        });
    }
    for (i, (at, title)) in starts.iter().enumerate() {
        let end = starts.get(i + 1).map(|(e, _)| *e).unwrap_or(pages.len());
        parts.push(Part {
            name: name(title),
            pages: pages[*at..end].to_vec(),
        });
    }
    Ok(parts)
}

/// A bookmark title as a file name: no path separators or characters
/// Windows refuses, spaces kept, at most 80 characters.
pub fn file_safe(title: &str) -> String {
    let s: String = title
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '-',
            c if c.is_control() => ' ',
            c => c,
        })
        .collect();
    let s = s.trim().trim_matches('.').trim();
    let s: String = s.chars().take(80).collect();
    if s.is_empty() {
        "Untitled".into()
    } else {
        s.trim_end().to_string()
    }
}

/// Split `from`'s version into files in `dir` named after `stem`, each an
/// extract, which stops at what any part asks that `answers` don't answer
/// ([`NeedsDecision`]) before anything is written. Returns them in order.
/// With `force`, existing files are replaced; otherwise that's an error
/// before anything is written.
///
/// [`NeedsDecision`]: crate::session::NeedsDecision
pub fn split(
    from: &ExportFrom,
    by: &SplitBy,
    dir: &Path,
    stem: &str,
    force: bool,
    answers: &AskAnswers,
    progress: &dyn Progress,
) -> Result<Vec<PathBuf>> {
    std::fs::create_dir_all(dir)?;
    let parts = match by {
        SplitBy::MaxBytes(max) => {
            let measuring = progress.child("measuring", 0.5);
            by_size(from, *max, stem, answers, &*measuring)?
        }
        _ => plan(&from.snap, by, stem)?,
    };
    let paths: Vec<PathBuf> = parts
        .iter()
        .map(|p| dir.join(format!("{}.pdf", p.name)))
        .collect();
    if !force && let Some(p) = paths.iter().find(|p| p.exists()) {
        bail!("{} already exists", p.display());
    }
    let extracts: Vec<Export> = (parts.iter().zip(&paths))
        .map(|(part, path)| {
            let out = Out::File {
                path: path.clone(),
                overwrite: force,
            };
            Export::extract(from.clone(), &part.pages, out)
        })
        .collect();
    // What any part asks stops the split before one is written: each is
    // checked first, but a size split's, asked about as they were measured.
    if !matches!(by, SplitBy::MaxBytes(_)) {
        let step = progress.child("checking", 0.25);
        step.set_total(Some(parts.len() as u64), Unit::ITEMS);
        for (i, extract) in extracts.iter().enumerate() {
            step.check()?;
            sheaf_doc::export::check(extract, answers, &NoProgress).decided()?;
            step.set(i as u64 + 1);
        }
    }
    let step = progress.child("writing", 1.0);
    step.set_total(Some(parts.len() as u64), Unit::ITEMS);
    for (i, (part, extract)) in parts.iter().zip(extracts).enumerate() {
        step.check()?;
        // Its count is the parts': the part only stops.
        let part_of = step.child(&part.name, 0.0);
        sheaf_doc::export::run(extract, answers, &*part_of).decided()?;
        drop(part_of);
        step.set(i as u64 + 1);
    }
    Ok(paths)
}

/// Greedy parts no bigger than `max` bytes: each takes as many pages as
/// fit, found by doubling then bisecting. A page too big on its own is a
/// part by itself, with a warning.
fn by_size(
    from: &ExportFrom,
    max: u64,
    stem: &str,
    answers: &AskAnswers,
    progress: &dyn Progress,
) -> Result<Vec<Part>> {
    let pages: Vec<PageId> = from.snap.state.order().iter().copied().collect();
    if pages.is_empty() {
        bail!("the document has no pages");
    }
    if max == 0 {
        bail!("the size limit must be more than zero");
    }
    progress.set_total(Some(pages.len() as u64), PAGES);
    let fits = |first: usize, end: usize| -> Result<bool> {
        progress.check()?;
        let extract = Export::extract(from.clone(), &pages[first..end], Out::Bytes);
        let made = sheaf_doc::export::run(extract, answers, &NoProgress).decided()?;
        let bytes = made.bytes.expect("an extract to bytes is its bytes");
        Ok(bytes.len() as u64 <= max)
    };
    let mut groups = Vec::new();
    let mut start = 0;
    while start < pages.len() {
        let n = pages.len() - start;
        let take = if !fits(start, start + 1)? {
            progress.message(
                Level::Warn,
                &format!("page {} alone is over the limit", start + 1),
            );
            1
        } else if fits(start, pages.len())? {
            n
        } else {
            // good fits, bad doesn't.
            let (mut good, mut bad) = (1, 2.min(n));
            while bad < n && fits(start, start + bad)? {
                good = bad;
                bad = (bad * 2).min(n);
            }
            while bad - good > 1 {
                let mid = (good + bad) / 2;
                if fits(start, start + mid)? {
                    good = mid;
                } else {
                    bad = mid;
                }
            }
            good
        };
        groups.push(pages[start..start + take].to_vec());
        start += take;
        progress.set(start as u64);
    }
    let width = groups.len().to_string().len();
    Ok(groups
        .into_iter()
        .enumerate()
        .map(|(i, pages)| Part {
            name: format!("{stem}_Part_{:0width$}", i + 1),
            pages,
        })
        .collect())
}

/// "10MB", "500 kB", "2.5M", or plain bytes.
pub fn parse_size(s: &str) -> Result<u64, String> {
    let t = s.trim().to_ascii_lowercase().replace(' ', "");
    let digits = t
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(t.len());
    let (num, unit) = t.split_at(digits);
    let n: f64 = num.parse().map_err(|_| format!("'{s}' isn't a size"))?;
    let mult = match unit {
        "" | "b" => 1.0,
        "k" | "kb" => 1e3,
        "m" | "mb" => 1e6,
        "g" | "gb" => 1e9,
        "kib" => 1024.0,
        "mib" => 1024.0 * 1024.0,
        "gib" => 1024.0 * 1024.0 * 1024.0,
        _ => return Err(format!("'{s}': use B, KB, MB or GB")),
    };
    let bytes = (n * mult).round();
    if bytes < 1.0 {
        return Err(format!("'{s}' is too small"));
    }
    Ok(bytes as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sheaf_doc::tx::fixture::bookmark;
    use sheaf_doc::{Document, Store};

    fn doc(n: u32) -> Document {
        let store = Store::new();
        let primary = sheaf_doc::store::blank(&store, n);
        Document::new(
            store,
            primary,
            "x.pdf".into(),
            None,
            &[],
            sheaf_doc::history::default_policy(),
        )
    }

    fn sizes(parts: &[Part]) -> Vec<usize> {
        parts.iter().map(|p| p.pages.len()).collect()
    }

    #[test]
    fn every_n_and_ranges() {
        let d = doc(10);
        let s = d.snapshot();
        let parts = plan(&s, &SplitBy::EveryN(4), "Return").unwrap();
        assert_eq!(sizes(&parts), vec![4, 4, 2]);
        assert_eq!(parts[0].name, "Return_Part_1");
        let parts = plan(
            &s,
            &SplitBy::Ranges(vec!["1-2".into(), "5, 9-".into()]),
            "R",
        )
        .unwrap();
        assert_eq!(sizes(&parts), vec![2, 3]);
        assert!(plan(&s, &SplitBy::Ranges(vec!["11".into()]), "R").is_err());
        let parts = plan(&doc(12).snapshot(), &SplitBy::EveryN(1), "R").unwrap();
        assert_eq!(parts[0].name, "R_Part_01");
    }

    #[test]
    fn top_bookmarks() {
        let mut d = doc(6);
        let ids: Vec<_> = d.state().order().iter().copied().collect();
        for (title, page) in [("Form 1040", 1), ("Schedule A/B", 3), ("Form 1040", 4)] {
            let after = d.state().outline_roots().last().copied();
            let add = bookmark(None, after, title, Some(ids[page]));
            d.commit_ops(vec![add], "b").unwrap();
        }
        let parts = plan(&d.snapshot(), &SplitBy::TopBookmarks, "Ret").unwrap();
        let names: Vec<_> = parts.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "Ret_Start",
                "Ret_Form 1040",
                "Ret_Schedule A-B",
                "Ret_Form 1040_2"
            ]
        );
        assert_eq!(sizes(&parts), vec![1, 2, 1, 2]);
    }

    #[test]
    fn sizes_parse() {
        assert_eq!(parse_size("10MB"), Ok(10_000_000));
        assert_eq!(parse_size("2.5 m"), Ok(2_500_000));
        assert_eq!(parse_size("1MiB"), Ok(1_048_576));
        assert_eq!(parse_size("500"), Ok(500));
        assert!(parse_size("ten").is_err());
        assert!(parse_size("5 TB").is_err());
    }

    /// A size split's parts, as their extracts measure them: pages that
    /// carry more marks than others, bookmarks and labels.
    #[test]
    fn a_size_split_makes_the_parts_it_did() {
        use sheaf_doc::annot::{Annot, AnnotId, AnnotKind, Box2};
        use sheaf_doc::tx::fixture::{add, label};
        use sheaf_edit::{LabelStyle, Rotation};
        let mut d = doc(12);
        let ids: Vec<_> = d.state().order().iter().copied().collect();
        let mut ops = Vec::new();
        for (i, &page) in ids.iter().enumerate() {
            for k in 0..(i * 7) % 5 {
                let kind = AnnotKind::Square {
                    rect: Box2 {
                        x: 20.0 + 30.0 * k as f32,
                        y: 40.0 + 10.0 * i as f32,
                        w: 25.0,
                        h: 15.0,
                    },
                };
                let a = Annot {
                    id: AnnotId(0),
                    kind,
                    color: [200, 20, 20],
                    opacity: 1.0,
                    stroke: 1.0,
                    author: "t".into(),
                    contents: format!("mark {k} on page {}", i + 1).repeat(k + 1),
                    modified: String::new(),
                    created: String::new(),
                    fill: None,
                    dashed: false,
                    cloudy: false,
                    turn: Rotation::R0,
                    name: String::new(),
                };
                ops.push(add(page, &a));
            }
        }
        d.commit_ops(ops, "marks").unwrap();
        for (title, page) in [("One", 0), ("Two", 4), ("Three", 9)] {
            let after = d.state().outline_roots().last().copied();
            let add = bookmark(None, after, title, Some(ids[page]));
            d.commit_ops(vec![add], "b").unwrap();
        }
        let ranges = vec![
            label(ids[0], LabelStyle::LowerRoman, "", 1),
            label(ids[3], LabelStyle::Decimal, "A-", 1),
        ];
        let labels = sheaf_edit::Op::SetPageLabels {
            ranges: Some(ranges),
        };
        d.commit_ops(vec![labels], "labels").unwrap();
        let from = d.export_from(sheaf_doc::Actor::User);
        let tracker = crate::Tracker::new();
        // The parts the split made before size splits were exports, each
        // row run against it (main at 3c72df2, ten runs alike). An
        // extract's length varies by a few bytes with its marks' random
        // ids, so each limit is some hundreds of bytes from every extract
        // measured against it, then and now (pages 1-7 are 11,431-11,443
        // bytes then and 11,998-12,008 now, pages 8-10 7,655-7,663 and
        // 8,011-8,017).
        for (max, parts) in [
            (3_000, &[2, 1, 1, 1, 2, 1, 1, 1, 2][..]),
            (7_500, &[4, 3, 2, 3]),
            (12_500, &[7, 5]),
        ] {
            let made = by_size(&from, max, "S", &AskAnswers::default(), &tracker).unwrap();
            assert_eq!(sizes(&made), parts, "{max}");
        }
    }
}
