//! Ops in words, for the History panel: "Delete pages 4–6", "Add a
//! highlight on page 3" (M10.5 §8.3). An op is described as it's lowered,
//! against the document just before it, so a transaction's later ops count
//! pages after its earlier ones. Page numbers come from the state's
//! position index, not a walk of the pages. A run of ops of one kind is
//! one line ([`lines`]): a batch of 7,000 is "Recognize text on 7000
//! pages".

use sheaf_edit::MarkKind;

use crate::effects::Flags;
use crate::model::{AnnotBody, DocState, PageId, Step};
use crate::tx::{Derive, ResolvedOp};

/// A line of the History panel: what some steps did, and whether they
/// removed content that was there (its badge).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub text: String,
    pub removes: bool,
}

/// What `op` does to `before`, the document it's applied to, in a short
/// line.
pub fn describe(op: &ResolvedOp, before: &DocState) -> String {
    use ResolvedOp as R;
    let pages = |ids: &[PageId]| pages_text(ids, before);
    match op {
        R::MovePages { pages: ids, .. } => format!("Move {}", pages(ids)),
        R::RotatePages { pages: ids, by } => {
            format!("Rotate {} {}°", pages(ids), by.rotation().degrees())
        }
        R::DeletePages { pages: ids } => format!("Delete {}", pages(ids)),
        R::DuplicatePages { pages: ids, .. } => format!("Duplicate {}", pages(ids)),
        R::InsertPages { pages: from, .. } => format!("Insert {}", count(from.len(), "page")),
        R::ReplacePages {
            pages: ids, with, ..
        } => format!("Replace {} with {}", pages(ids), count(with.len(), "page")),
        R::SetPageLabels { .. } => "Change page labels".into(),
        R::AddBookmark { title, .. } => format!("Add bookmark “{title}”"),
        R::PatchBookmarks { bookmarks, patch } => {
            if bookmarks.len() > 1 {
                return format!("Change {}", count(bookmarks.len(), "bookmark"));
            }
            let mut what = Vec::new();
            if let Some(t) = &patch.title {
                what.push(format!("rename it “{t}”"));
            }
            if !patch.target.is_keep() || !patch.view.is_keep() {
                what.push("change where it goes".into());
            }
            match patch.color {
                sheaf_edit::Patch::Keep => {}
                sheaf_edit::Patch::Clear => what.push("clear its color".into()),
                sheaf_edit::Patch::Set(_) => what.push("color it".into()),
            }
            if patch.style.is_some() {
                what.push("change its style".into());
            }
            match what.as_slice() {
                [] => "Change a bookmark".into(),
                _ => format!("Bookmark: {}", what.join(", ")),
            }
        }
        R::MoveBookmarks {
            bookmarks,
            with_pages,
            ..
        } => {
            let what = match bookmarks.len() {
                1 => "a bookmark".to_string(),
                n => format!("{n} bookmarks"),
            };
            match with_pages {
                true => format!("Move {what} with its pages"),
                false => format!("Move {what}"),
            }
        }
        R::DeleteBookmarks { bookmarks } => match bookmarks.len() {
            1 => "Delete a bookmark".into(),
            n => format!("Delete {n} bookmarks"),
        },
        R::AddMark { page, mark, .. } => {
            let on = on_page(*page, before);
            match &mark.kind {
                MarkKind::Reply {} => format!("Reply{on}"),
                MarkKind::Status { model, state } if model == crate::annot::MARKED => {
                    let verb = if state == "Marked" {
                        "Check"
                    } else {
                        "Uncheck"
                    };
                    format!("{verb} a comment{on}")
                }
                MarkKind::Status { state, .. } => format!("Set status {state}{on}"),
                kind => format!("Add {}{on}", noun(kind)),
            }
        }
        R::AddConnector { label, .. } => format!("Add connector pair {label}"),
        R::PatchMarks { marks, .. } => match marks.as_slice() {
            [one] => format!("Change {}", annot_text(*one, before)),
            _ => format!("Change {}", count(marks.len(), "mark")),
        },
        R::MoveMarks { marks, .. } => match marks.as_slice() {
            [one] => format!("Move {}", annot_text(*one, before)),
            _ => format!("Move {}", count(marks.len(), "mark")),
        },
        R::DeleteAnnotations { annots } => match annots.as_slice() {
            [one] => format!("Delete {}", annot_text(*one, before)),
            _ => format!("Delete {}", count(annots.len(), "comment")),
        },
        R::AdoptAnnotations { annots } => match annots.as_slice() {
            [one] => {
                let on = (before.annot(*one)).map_or(String::new(), |a| on_page(a.page, before));
                format!("Make a comment editable{on}")
            }
            _ => format!("Make {} editable", count(annots.len(), "comment")),
        },
        R::SetOcrText { page, .. } => format!("Recognize text on {}", pages(&[*page])),
        R::ClearOcrText { pages: ids } => {
            format!("Remove recognized text from {}", pages(ids))
        }
        R::SetHeaderFooter { .. } => "Set the header and footer".into(),
        R::RemoveHeaderFooter { .. } => "Remove the header and footer".into(),
        R::SetPageBase { pages: list, why } => {
            let ids: Vec<PageId> = list.iter().map(|&(id, ..)| id).collect();
            derived(*why, &pages(&ids))
        }
    }
}

/// What a derived flow did to `pages`, the pages it names.
fn derived(why: Derive, pages: &str) -> String {
    let layer = |behind: bool| if behind { "background" } else { "watermark" };
    match why {
        Derive::Crop => format!("Crop {pages}"),
        Derive::Watermark { behind, .. } => format!("Add a {} to {pages}", layer(behind)),
        Derive::RemoveWatermark { behind } => format!("Remove the {} from {pages}", layer(behind)),
        Derive::StripHeaderFooter => format!("Remove the header and footer from {pages}"),
    }
}

/// `steps`' lines, a run of steps of one kind as one: what the History
/// panel and a commit say they did.
pub fn lines(steps: &[Step]) -> Vec<Line> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < steps.len() {
        let name = steps[i].op.name();
        let n = 1
            + (steps[i + 1..].iter())
                .take_while(|s| s.op.name() == name)
                .count();
        let run = &steps[i..i + n];
        out.push(Line {
            text: match run {
                [one] => one.line.clone(),
                _ => run_line(run),
            },
            removes: (run.iter()).any(|s| s.effects.effects.flags.contains(Flags::REMOVES)),
        });
        i += n;
    }
    out
}

/// What a run of steps of one kind did, in one line: how many of what.
fn run_line(run: &[Step]) -> String {
    use ResolvedOp as R;
    let sum = |of: fn(&ResolvedOp) -> usize| run.iter().map(|s| of(&s.op)).sum::<usize>();
    let pages = || count(sum(page_count), "page");
    let n = run.len();
    match &*run[0].op {
        R::MovePages { .. } => format!("Move {}", pages()),
        R::RotatePages { .. } => format!("Rotate {}", pages()),
        R::DeletePages { .. } => format!("Delete {}", pages()),
        R::DuplicatePages { .. } => format!("Duplicate {}", pages()),
        R::InsertPages { .. } => format!("Insert {}", pages()),
        R::ReplacePages { .. } => format!("Replace {}", pages()),
        R::ClearOcrText { .. } => format!("Remove recognized text from {}", pages()),
        R::SetOcrText { .. } => format!("Recognize text on {}", pages()),
        R::SetPageBase { why, .. } => derived(*why, &pages()),
        R::AddBookmark { .. } => format!("Add {}", count(n, "bookmark")),
        R::PatchBookmarks { .. } => format!("Change {}", count(sum(target_count), "bookmark")),
        R::MoveBookmarks { .. } => format!("Move {}", count(sum(target_count), "bookmark")),
        R::DeleteBookmarks { .. } => format!("Delete {}", count(sum(target_count), "bookmark")),
        R::AddMark { .. } => format!("Add {}", count(n, "mark")),
        R::AddConnector { .. } => format!("Add {}", count(n, "connector pair")),
        R::PatchMarks { .. } => format!("Change {}", count(sum(target_count), "mark")),
        R::MoveMarks { .. } => format!("Move {}", count(sum(target_count), "mark")),
        R::DeleteAnnotations { .. } => format!("Delete {}", count(sum(target_count), "comment")),
        R::AdoptAnnotations { .. } => {
            format!("Make {} editable", count(sum(target_count), "comment"))
        }
        // Each replaces the one before: the last says what's there.
        R::SetPageLabels { .. } | R::SetHeaderFooter { .. } | R::RemoveHeaderFooter { .. } => {
            run[n - 1].line.clone()
        }
    }
}

/// How many pages a page op names.
fn page_count(op: &ResolvedOp) -> usize {
    use ResolvedOp as R;
    match op {
        R::MovePages { pages, .. }
        | R::RotatePages { pages, .. }
        | R::DeletePages { pages }
        | R::DuplicatePages { pages, .. }
        | R::ClearOcrText { pages }
        | R::RemoveHeaderFooter { pages } => pages.len(),
        R::InsertPages { pages, .. } => pages.len(),
        R::ReplacePages { with, .. } => with.len(),
        R::SetPageBase { pages, .. } => pages.len(),
        R::SetOcrText { .. } => 1,
        _ => 0,
    }
}

/// How many bookmarks or annotations an op names.
fn target_count(op: &ResolvedOp) -> usize {
    use ResolvedOp as R;
    match op {
        R::PatchBookmarks { bookmarks, .. }
        | R::MoveBookmarks { bookmarks, .. }
        | R::DeleteBookmarks { bookmarks } => bookmarks.len(),
        R::PatchMarks { marks, .. } | R::MoveMarks { marks, .. } => marks.len(),
        R::DeleteAnnotations { annots } | R::AdoptAnnotations { annots } => annots.len(),
        _ => 0,
    }
}

/// Annotation `id` in `st`, and its page: "a highlight on page 3", "a
/// comment on page 2".
fn annot_text(id: crate::annot::AnnotId, st: &DocState) -> String {
    let Some(rec) = st.annot(id) else {
        return "a mark".into();
    };
    let what = match &rec.body {
        AnnotBody::Mark(m) => noun(&crate::tx::mark_kind(&m.annot.kind)),
        AnnotBody::Foreign(f) if f.summary.is_link() => "a link".into(),
        AnnotBody::Foreign(_) => "a comment".into(),
        AnnotBody::Widget(_) => "a form field".into(),
    };
    format!("{what}{}", on_page(rec.page, st))
}

/// "a highlight", "a sticky note", "tickmark ✓".
fn noun(kind: &MarkKind) -> String {
    match kind {
        MarkKind::Highlight { .. } => "a highlight".into(),
        MarkKind::Note { .. } => "a sticky note".into(),
        MarkKind::FreeText { .. } => "a text box".into(),
        MarkKind::Square { .. } => "a rectangle".into(),
        MarkKind::Ellipse { .. } => "an ellipse".into(),
        MarkKind::Line { arrow: true, .. } => "an arrow".into(),
        MarkKind::Line { .. } => "a line".into(),
        MarkKind::Ink { .. } => "a drawing".into(),
        MarkKind::Stamp { glyph, .. } => format!("tickmark {glyph}"),
        MarkKind::Connector { label, .. } => format!("connector {label}"),
        MarkKind::Tape { .. } => "a calculator tape".into(),
        MarkKind::Underline { .. } => "an underline".into(),
        MarkKind::StrikeOut { .. } => "a strikethrough".into(),
        MarkKind::Squiggly { .. } => "a squiggly underline".into(),
        MarkKind::Replace { .. } => "a text replacement".into(),
        MarkKind::Insert { .. } => "a text insertion".into(),
        MarkKind::PageStamp { name, .. } => format!("stamp {name}"),
        MarkKind::TextStamp { text, .. } => {
            format!("stamp {}", text.lines().next().unwrap_or_default())
        }
        MarkKind::PageBorder { status, .. } => format!("a {status} border"),
        MarkKind::Polygon { .. } => "a polygon".into(),
        MarkKind::Polyline { .. } => "a polyline".into(),
        MarkKind::Callout { .. } => "a callout".into(),
        MarkKind::FileAttachment { name, .. } => format!("attachment {name}"),
        MarkKind::Measure { what, .. } => format!("a {} measurement", what.label().to_lowercase()),
        MarkKind::Reply {} => "a reply".into(),
        MarkKind::Status { .. } => "a status".into(),
    }
}

fn count(n: usize, what: &str) -> String {
    if n == 1 {
        format!("1 {what}")
    } else {
        format!("{n} {what}s")
    }
}

/// " on page 3", or nothing when the page isn't in the document.
fn on_page(page: PageId, st: &DocState) -> String {
    st.pos_of(page)
        .map(|i| format!(" on page {}", i + 1))
        .unwrap_or_default()
}

/// "page 3", "pages 2, 5", "pages 4–6", or a count when one of them isn't
/// in the document.
fn pages_text(ids: &[PageId], st: &DocState) -> String {
    let mut wanted = ids.to_vec();
    wanted.sort_unstable();
    wanted.dedup();
    let mut numbers: Vec<usize> = wanted
        .iter()
        .filter_map(|&id| st.pos_of(id))
        .map(|i| i + 1)
        .collect();
    if numbers.len() != wanted.len() || numbers.is_empty() {
        return count(wanted.len(), "page");
    }
    numbers.sort_unstable();
    // Runs: 4–6, 9.
    let mut runs: Vec<String> = Vec::new();
    let mut i = 0;
    while i < numbers.len() {
        let mut j = i;
        while j + 1 < numbers.len() && numbers[j + 1] == numbers[j] + 1 {
            j += 1;
        }
        runs.push(match j - i {
            0 => numbers[i].to_string(),
            1 => format!("{}, {}", numbers[i], numbers[j]),
            _ => format!("{}–{}", numbers[i], numbers[j]),
        });
        i = j + 1;
    }
    let word = if numbers.len() == 1 { "page" } else { "pages" };
    // A long list is just its count.
    if runs.len() > 4 {
        return count(numbers.len(), "page");
    }
    format!("{word} {}", runs.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tx::tests::doc_of;
    use sheaf_edit::Turn;

    #[test]
    fn pages_read_as_numbers_and_runs() {
        let d = doc_of(10, &[]);
        let id = |i: usize| d.page(i).unwrap();
        let st = d.state();
        assert_eq!(pages_text(&[id(2)], st), "page 3");
        assert_eq!(pages_text(&[id(5), id(4), id(3)], st), "pages 4–6");
        assert_eq!(pages_text(&[id(1), id(4)], st), "pages 2, 5");
        assert_eq!(pages_text(&[id(0), id(1), id(8)], st), "pages 1, 2, 9");
        let every_other: Vec<PageId> = (0..10).step_by(2).map(id).collect();
        assert_eq!(pages_text(&every_other, st), "5 pages", "a long list");
        assert_eq!(
            pages_text(&[id(3), PageId(999)], st),
            "2 pages",
            "one isn't there"
        );
        assert_eq!(pages_text(&[id(2), id(2)], st), "page 3", "named twice");
        assert_eq!(
            pages_text(&[id(3), id(3), PageId(999)], st),
            "2 pages",
            "named twice, and one isn't there"
        );
    }

    #[test]
    fn ops_read_plainly() {
        let d = doc_of(6, &[]);
        let id = |i: usize| d.page(i).unwrap();
        let st = d.state();
        let delete = ResolvedOp::DeletePages {
            pages: vec![id(3), id(4), id(5)],
        };
        assert_eq!(describe(&delete, st), "Delete pages 4–6");
        let rotate = ResolvedOp::RotatePages {
            pages: vec![id(0)],
            by: Turn::Quarter,
        };
        assert_eq!(describe(&rotate, st), "Rotate page 1 90°");
    }

    /// A derived flow's line names it (M10.5 bug 8), not "change the
    /// content".
    #[test]
    fn a_page_base_line_names_its_flow() {
        let d = doc_of(6, &[]);
        let id = |i: usize| d.page(i).unwrap();
        let on = |i: usize, why| ResolvedOp::SetPageBase {
            pages: vec![(id(i), d.primary().clone(), i as u32)],
            why,
        };
        let st = d.state();
        assert_eq!(describe(&on(2, Derive::Crop), st), "Crop page 3");
        assert_eq!(
            describe(&on(0, Derive::StripHeaderFooter), st),
            "Remove the header and footer from page 1"
        );
        let behind = Derive::Watermark {
            behind: true,
            unencodable: false,
        };
        assert_eq!(describe(&on(1, behind), st), "Add a background to page 2");
    }
}
