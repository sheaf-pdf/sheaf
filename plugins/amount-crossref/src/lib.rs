//! Cross-reference amounts that appear on more than one page. See README.md.

use std::collections::BTreeMap;
use std::sync::Mutex;

use sheaf_plugin_api::sheaf_edit::Box2;
use sheaf_plugin_api::types::{AnnotKind, AnnotView, PageInfo};
use sheaf_plugin_api::ui::{self, PanelRow, RowAction};
use sheaf_plugin_api::{FormBuilder, Ops, Plugin, document, text, view};

/// Money as written in returns and statements: "$1,234.56", "(1,234)",
/// "12,500", "980.10". Bare integers without a separator are too noisy
/// (line numbers, years) to count.
const AMOUNT: &str = r"\(?\$?\s?\d{1,3}(?:,\d{3})+(?:\.\d{2})?\)?|\(?\$?\s?\d+\.\d{2}\)?";

const PANEL: &str = "pairs";
/// Connector side, as sheaf's own connector tool draws them.
const CONNECTOR: f32 = 22.0;
/// Why pairs found before the document was opened again aren't linked:
/// its pages' ids aren't theirs.
const REOPENED: &str = "the document was opened again; find again";

/// An amount in cents, from its text. Parentheses mean negative.
pub fn parse_amount(s: &str) -> Option<i64> {
    let negative = s.contains('(') && s.contains(')');
    let digits: String = s
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    let (whole, cents) = match digits.split_once('.') {
        Some((w, c)) if c.len() == 2 => (w, c.parse::<i64>().ok()?),
        Some(_) => return None,
        None => (digits.as_str(), 0),
    };
    let v = whole
        .parse::<i64>()
        .ok()?
        .checked_mul(100)?
        .checked_add(cents)?;
    Some(if negative { -v } else { v })
}

pub fn format_amount(cents: i64) -> String {
    let abs = cents.unsigned_abs();
    let whole = (abs / 100).to_string();
    let mut grouped = String::new();
    for (i, ch) in whole.chars().enumerate() {
        if i > 0 && (whole.len() - i).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(ch);
    }
    let s = format!("${grouped}.{:02}", abs % 100);
    if cents < 0 { format!("({s})") } else { s }
}

/// Where an amount was seen.
#[derive(Debug, Clone, PartialEq)]
pub struct Seen {
    pub page: u64,
    /// Display index, for labels.
    pub index: usize,
    /// In PageSpace, as its text was found.
    pub rect: Box2,
}

/// A candidate: the same amount on two pages.
#[derive(Debug, Clone, PartialEq)]
pub struct Pair {
    pub cents: i64,
    pub a: Seen,
    pub b: Seen,
}

impl Pair {
    pub fn key(&self) -> String {
        format!("{}:{}:{}", self.cents, self.a.page, self.b.page)
    }
}

/// Pair each amount's first sighting on a page with its first sighting on
/// the next page it appears on, in page order. Amounts below `min_cents`,
/// and multiples of $100 when `skip_round`, are left out.
pub fn find_pairs(seen: &[(i64, Seen)], min_cents: i64, skip_round: bool) -> Vec<Pair> {
    let mut by_amount: BTreeMap<i64, Vec<&Seen>> = BTreeMap::new();
    for (cents, s) in seen {
        if cents.abs() < min_cents || (skip_round && cents % 10_000 == 0) {
            continue;
        }
        let list = by_amount.entry(*cents).or_default();
        if !list.iter().any(|x| x.page == s.page) {
            list.push(s);
        }
    }
    let mut pairs = Vec::new();
    for (cents, mut list) in by_amount {
        list.sort_by_key(|s| s.index);
        for w in list.windows(2) {
            pairs.push(Pair {
                cents,
                a: w[0].clone(),
                b: w[1].clone(),
            });
        }
    }
    pairs.sort_by_key(|p| (p.a.index, p.b.index, p.cents));
    pairs
}

fn overlaps(a: &Box2, b: &Box2) -> bool {
    a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h
}

/// The connector square just right of an amount on `page`, as the page
/// shows: both in PageSpace.
pub fn connector_rect(page: &PageInfo, amount: &Box2) -> Box2 {
    let shown = view::box_to_view(page, *amount);
    let square = Box2 {
        x: shown.right() + 3.0,
        y: shown.y + shown.h / 2.0 - CONNECTOR / 2.0,
        w: CONNECTOR,
        h: CONNECTOR,
    };
    view::box_from_view(page, square)
}

/// Pairs waiting in the panel, and the document they were found in: its
/// path, and the instance of it open then, whose pages their ids name.
struct Waiting {
    path: Option<String>,
    instance: Option<u64>,
    pairs: Vec<Pair>,
}

static PENDING: Mutex<Option<Waiting>> = Mutex::new(None);

/// The document the call reads: its path and instance.
fn this_document() -> (Option<String>, Option<u64>) {
    (document::path(), document::version().map(|v| v.instance))
}

/// The pairs waiting for this document: none for another, and refused
/// when it was opened again since they were found.
fn pending() -> Result<Vec<Pair>, String> {
    let (path, instance) = this_document();
    match &*PENDING.lock().unwrap() {
        Some(w) if w.path == path && w.instance == instance => Ok(w.pairs.clone()),
        Some(w) if w.path == path && !w.pairs.is_empty() => Err(REOPENED.into()),
        _ => Ok(Vec::new()),
    }
}

fn set_pending(pairs: Vec<Pair>) {
    let rows: Vec<PanelRow> = pairs
        .iter()
        .map(|p| PanelRow {
            id: p.key(),
            label: format_amount(p.cents),
            detail: format!("page {} ⇄ page {}", p.a.index + 1, p.b.index + 1),
            badge: None,
            icon: ui::Icon::Link,
            depth: 0,
            page: Some(p.a.page),
            actions: vec![
                RowAction {
                    id: "link".into(),
                    label: "Link".into(),
                },
                RowAction {
                    id: "dismiss".into(),
                    label: "Dismiss".into(),
                },
            ],
        })
        .collect();
    ui::set_panel(PANEL, &rows);
    let (path, instance) = this_document();
    *PENDING.lock().unwrap() = Some(Waiting {
        path,
        instance,
        pairs,
    });
}

/// The connectors on `page`: their labels and squares.
fn connectors(page: u64) -> Vec<(String, Box2)> {
    (document::annotations(page).into_iter())
        .filter_map(|a| match a {
            AnnotView::Mark(m) => match m.kind {
                AnnotKind::Connector(c) => Some((c.label, c.rect.into())),
                _ => None,
            },
            AnnotView::Comment(_) => None,
        })
        .collect()
}

/// The next free connector number among `pages`.
fn next_label(pages: &[PageInfo]) -> u32 {
    (pages.iter())
        .flat_map(|p| connectors(p.id))
        .filter_map(|(label, _)| label.trim().parse::<u32>().ok())
        .max()
        .unwrap_or(0)
        + 1
}

/// One connector pair per pair, numbered from the next free label,
/// submitted as `label`.
fn link(label: &str, pairs: &[Pair]) -> Result<(), String> {
    let pages = document::pages();
    let end = |s: &Seen| {
        let gone = "a page of that match is gone; find again";
        let page = (pages.iter().find(|p| p.id == s.page)).ok_or(gone)?;
        Ok::<_, String>((s.page, connector_rect(page, &s.rect)))
    };
    let first = next_label(&pages);
    let mut ops = Ops::new();
    for (i, p) in pairs.iter().enumerate() {
        ops = ops.connector(end(&p.a)?, end(&p.b)?, &(first + i as u32).to_string());
    }
    ops.submit(label).map(|_| ()).map_err(|f| f.message)
}

fn find() -> Result<(), String> {
    let pages = document::pages();
    if pages.is_empty() {
        return Err("open a document first".into());
    }
    let Some(a) = FormBuilder::new("Find Matching Amounts")
        .number_in("min", "Smallest amount", 100.0, 0.0, 1e12)
        .pages("pages", "Pages")
        .checkbox("round", "Skip round hundreds", true)
        .submit_label("Find")
        .show()
    else {
        return Ok(());
    };
    let min_cents = (a.number("min").unwrap_or(100.0) * 100.0).round() as i64;
    let skip_round = a.checked("round").unwrap_or(true);
    let scan: Vec<u64> = match a.pages("pages") {
        Some(p) => p.to_vec(),
        None => pages.iter().map(|p| p.id).collect(),
    };
    let mut seen = Vec::new();
    for (index, p) in pages.iter().enumerate() {
        if !scan.contains(&p.id) {
            continue;
        }
        let there = connectors(p.id);
        for m in text::pattern_search(p.id, AMOUNT)? {
            let (Some(cents), Some(rect)) = (parse_amount(&m.text), m.rects.first().copied())
            else {
                continue;
            };
            let rect = rect.into();
            // Already cross-referenced: a connector sits where ours would.
            let ours = connector_rect(p, &rect);
            if there.iter().any(|(_, c)| overlaps(c, &ours)) {
                continue;
            }
            seen.push((
                cents,
                Seen {
                    page: p.id,
                    index,
                    rect,
                },
            ));
        }
    }
    let pairs = find_pairs(&seen, min_cents, skip_round);
    let n = pairs.len();
    set_pending(pairs);
    ui::notice(&match n {
        0 => "No matching amounts found".to_string(),
        1 => "1 matching pair — see Amount Matches".to_string(),
        n => format!("{n} matching pairs — see Amount Matches"),
    });
    Ok(())
}

struct AmountCrossref;

impl Plugin for AmountCrossref {
    fn run_command(id: String, _args_json: Option<String>) -> Result<(), String> {
        match id.as_str() {
            "find" => find(),
            "link-all" => {
                let pairs = pending()?;
                if pairs.is_empty() {
                    return Err("nothing to link; run Find Matching Amounts first".into());
                }
                let n = pairs.len();
                let label = format!("Link {n} amount{}", if n == 1 { "" } else { "s" });
                link(&label, &pairs)?;
                set_pending(Vec::new());
                Ok(())
            }
            other => Err(format!("unknown command {other}")),
        }
    }

    fn panel_action(panel: String, row: String, action: String) -> Result<(), String> {
        if panel != PANEL {
            return Err(format!("unknown panel {panel}"));
        }
        let mut pairs = pending()?;
        let at = pairs
            .iter()
            .position(|p| p.key() == row)
            .ok_or("that match is gone; run Find Matching Amounts again")?;
        let pair = pairs.remove(at);
        match action.as_str() {
            "link" => link(
                &format!("Link {}", format_amount(pair.cents)),
                std::slice::from_ref(&pair),
            )?,
            "dismiss" => {}
            other => return Err(format!("unknown action {other}")),
        }
        set_pending(pairs);
        Ok(())
    }
}

sheaf_plugin_api::export!(AmountCrossref);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn amounts_parse_and_format() {
        assert_eq!(parse_amount("$1,234.56"), Some(123456));
        assert_eq!(parse_amount("(1,234)"), Some(-123400));
        assert_eq!(parse_amount("12,500"), Some(1250000));
        assert_eq!(parse_amount("980.10"), Some(98010));
        assert_eq!(parse_amount("1.5"), None);
        assert_eq!(format_amount(123456), "$1,234.56");
        assert_eq!(format_amount(-1250000), "($12,500.00)");
        assert_eq!(format_amount(5), "$0.05");
    }

    fn seen(page: u64, index: usize) -> Seen {
        Seen {
            page,
            index,
            rect: Box2 {
                x: 0.0,
                y: 0.0,
                w: 10.0,
                h: 10.0,
            },
        }
    }

    #[test]
    fn pairs_chain_across_pages_once_per_page() {
        let s = vec![
            (123456, seen(10, 0)),
            (123456, seen(10, 0)), // twice on one page: one sighting
            (123456, seen(30, 2)),
            (123456, seen(20, 1)),
            (5000, seen(10, 0)), // below the minimum
            (5000, seen(20, 1)),
            (2000000, seen(10, 0)), // round
            (2000000, seen(20, 1)),
            (777777, seen(40, 3)), // only one page
        ];
        let pairs = find_pairs(&s, 10_000, true);
        let got: Vec<(i64, usize, usize)> = pairs
            .iter()
            .map(|p| (p.cents, p.a.index, p.b.index))
            .collect();
        assert_eq!(got, [(123456, 0, 1), (123456, 1, 2)]);
        let with_round = find_pairs(&s, 10_000, false);
        assert_eq!(with_round.len(), 3);
    }

    /// A Letter page as sheaf describes one, upright or turned 90°: its
    /// view matrices.
    fn letter(turned: bool) -> PageInfo {
        use sheaf_plugin_api::types::{Affine, Box2, Rotation};
        let (m, rotate) = match turned {
            false => ([1.0, 0.0, 0.0, -1.0, 0.0, 792.0], Rotation::R0),
            true => ([0.0, 1.0, 1.0, 0.0, 0.0, 0.0], Rotation::R90),
        };
        let [a, b, c, d, e, f] = m;
        let m = Affine { a, b, c, d, e, f };
        let letter = Box2 {
            x: 0.0,
            y: 0.0,
            w: 612.0,
            h: 792.0,
        };
        PageInfo {
            id: 1,
            media: letter,
            crop: letter,
            rotate,
            label: "1".into(),
            to_view: m,
            from_view: m,
        }
    }

    /// The connector sits right of the amount as its page shows: up the
    /// page in PageSpace on an upright one, and along it on a turned one.
    #[test]
    fn connectors_sit_right_of_the_amount_as_shown() {
        let amount = seen(1, 0).rect;
        let r = connector_rect(&letter(false), &amount);
        assert_eq!((r.x, r.y, r.w, r.h), (13.0, -6.0, CONNECTOR, CONNECTOR));
        let r = connector_rect(&letter(true), &amount);
        assert_eq!((r.x, r.y, r.w, r.h), (-6.0, 13.0, CONNECTOR, CONNECTOR));
    }
}
