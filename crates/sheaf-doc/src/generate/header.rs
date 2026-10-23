//! The header and footer (M10.5 §3.5): six slots of text, left, center
//! and right of the header and of the footer, whose tokens read the page
//! they're on. `<<1>>`, `<<N>>` and `<<label>>` are live, read from the
//! document as it's written, a print's pages where the document has them
//! (`DocState::numbered_in`); `<<date>>`, `<<file>>` and `<<bates>>` say
//! what its generator fixed when it was set ([`Frozen`]). Each slot is laid
//! out on the page as it shows, the header's tops `top` below its top edge
//! and the footer's baselines `bottom` above its bottom edge, at the left
//! margin, centered, or flush to the right margin. A top is placed by
//! Helvetica's cap height whatever the face: near enough for the others.

use std::collections::BTreeSet;
use std::sync::Arc;

use sheaf_edit::{BatesSpec, HeaderFooterSpec};

use crate::content::{Font, PaginationKind, Std14};
use crate::geom::Geom;
use crate::model::{CtxNeeds, Frozen, PageCtx, PageId};

use super::CAP_HEIGHT;

/// The tokens a slot's text can use, and what each says.
pub const TOKENS: [(&str, &str); 6] = [
    ("<<1>>", "page number"),
    ("<<N>>", "page count"),
    ("<<label>>", "page label"),
    ("<<date>>", "date"),
    ("<<file>>", "file name"),
    ("<<bates>>", "Bates number"),
];

/// One slot's text, placed: its baseline's left end in the page's shown
/// space (top-left origin, points).
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    /// In the header (else the footer).
    pub header: bool,
    pub text: String,
    pub x: f32,
    pub baseline: f32,
    pub size: f32,
    pub width: f32,
}

/// The kinds of pagination artifact a header and footer draws, and
/// replaces others' of on its pages (§11.4): headers and footers.
pub fn kinds() -> BTreeSet<PaginationKind> {
    BTreeSet::from([PaginationKind::Header, PaginationKind::Footer])
}

/// Whether `spec` draws nothing: every slot is blank.
pub fn is_empty(spec: &HeaderFooterSpec) -> bool {
    spec.texts.iter().all(|t| t.trim().is_empty())
}

/// Why a header and footer can't draw `spec`, when it can't: some text, a
/// size above 0, margins that are numbers. What an op is refused for, and
/// what's read passed over for.
pub(crate) fn unfit(spec: &HeaderFooterSpec) -> Option<String> {
    if is_empty(spec) {
        return Some("every slot is blank".into());
    }
    if !(spec.size.is_finite() && spec.size > 0.0) {
        return Some(format!("the size is {}, not above 0", spec.size));
    }
    let m = &spec.margins;
    if ![m.top, m.bottom, m.left, m.right]
        .iter()
        .all(|v| v.is_finite())
    {
        return Some("a margin isn't a number".into());
    }
    None
}

/// What `spec`'s tokens read of each page it's on: `<<1>>` its position,
/// `<<N>>` how many pages there are, `<<label>>` its label, and
/// `<<bates>>`, when it numbers them, its Bates number.
pub fn needs(spec: &HeaderFooterSpec) -> CtxNeeds {
    let uses = |token: &str| spec.texts.iter().any(|t| t.contains(token));
    CtxNeeds {
        index: uses("<<1>>"),
        count: uses("<<N>>"),
        label: uses("<<label>>"),
        bates: spec.bates.is_some() && uses("<<bates>>"),
    }
}

/// Bates number `n` as `bates` writes it: its prefix, `n` zero-padded to
/// its digits, and its suffix.
pub fn bates_text(bates: &BatesSpec, n: u64) -> String {
    format!(
        "{}{:0width$}{}",
        bates.prefix,
        n,
        bates.suffix,
        width = bates.digits as usize
    )
}

/// Bates numbers for `pages`, in order, from `start`: what a header and
/// footer fixes when it's set ([`Frozen::bates`]). None when they'd run
/// past the largest number there is.
pub fn numbered(
    pages: impl IntoIterator<Item = PageId>,
    start: u64,
) -> Option<Arc<im::OrdMap<PageId, u64>>> {
    let numbers = (0..).map(|i| start.checked_add(i));
    let numbered = pages.into_iter().zip(numbers);
    (numbered.map(|(p, n)| Some((p, n?))))
        .collect::<Option<_>>()
        .map(Arc::new)
}

/// `text` with each token replaced by what it reads of the page `ctx`
/// says, or of what `frozen` fixed, in one pass, so a value (a file name,
/// a date, a label) is never read for tokens again: what [`needs`] says of
/// the texts alone holds.
fn expand(spec: &HeaderFooterSpec, frozen: &Frozen, ctx: &PageCtx, text: &str) -> String {
    let value = |token: &str| match token {
        "<<1>>" => (u64::from(ctx.position) + u64::from(spec.first_number)).to_string(),
        "<<N>>" => ctx.count.to_string(),
        "<<label>>" => ctx.label.to_string(),
        "<<date>>" => frozen.date.to_string(),
        "<<file>>" => frozen.file.to_string(),
        _ => match (&spec.bates, ctx.bates) {
            (Some(bates), Some(n)) => bates_text(bates, n),
            _ => String::new(),
        },
    };
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("<<") {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        match TOKENS.iter().find(|(t, _)| rest.starts_with(t)) {
            Some((token, _)) => {
                out.push_str(&value(token));
                rest = &rest[token.len()..];
            }
            None => {
                out.push('<');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Each slot's text on the page `ctx` says, by slot, the blank ones left
/// out: what [`layout`] places and [`unencodable`] reads.
pub fn slots<'a>(
    spec: &'a HeaderFooterSpec,
    frozen: &'a Frozen,
    ctx: &'a PageCtx,
) -> impl Iterator<Item = (usize, String)> + 'a {
    (spec.texts.iter().enumerate())
        .map(|(slot, text)| (slot, expand(spec, frozen, ctx, text)))
        .filter(|(_, text)| !text.trim().is_empty())
}

/// Whether some of the text `spec` draws, with what `frozen` fixed, can't
/// be drawn in `face`: it shows as `?`. Its tokens draw the same
/// characters on every page but the digits, which every face has, and a
/// page's label, which [`label_unencodable`] reads: so a page labeled by
/// its number answers for all.
pub fn unencodable(spec: &HeaderFooterSpec, frozen: &Frozen, face: &Std14) -> bool {
    let first = PageCtx {
        geom: Geom::read(None, None, 0),
        position: 0,
        count: 1,
        label: "1".into(),
        bates: spec.bates.as_ref().map(|b| b.start),
    };
    slots(spec, frozen, &first).any(|(_, text)| face.encode(&text).1.0 > 0)
}

/// Whether `spec` draws page label `label` (`<<label>>`), and `face` can't
/// draw all of it.
pub fn label_unencodable(spec: &HeaderFooterSpec, label: &str, face: &Std14) -> bool {
    needs(spec).label && face.encode(label).1.0 > 0
}

/// The slots' texts on the page `ctx` says, placed on it as it shows, in
/// `face`.
pub fn layout(spec: &HeaderFooterSpec, frozen: &Frozen, ctx: &PageCtx, face: &Std14) -> Vec<Line> {
    let size = ctx.geom.shown();
    let fs = spec.size;
    let margins = &spec.margins;
    let mut out = Vec::new();
    for (slot, text) in slots(spec, frozen, ctx) {
        let width = face.width(&text, fs);
        let x = match slot % 3 {
            0 => margins.left,
            1 => (size.width - width) / 2.0,
            _ => size.width - margins.right - width,
        };
        let baseline = if slot < 3 {
            margins.top + fs * CAP_HEIGHT
        } else {
            size.height - margins.bottom
        };
        out.push(Line {
            header: slot < 3,
            text,
            x,
            baseline,
            size: fs,
            width,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use sheaf_edit::Margins;

    use super::*;

    fn spec() -> HeaderFooterSpec {
        HeaderFooterSpec {
            texts: [
                "<<file>>".into(),
                String::new(),
                "<<date>>".into(),
                "<<label>>".into(),
                "Page <<1>> of <<N>>".into(),
                "<<bates>>".into(),
            ],
            font: sheaf_edit::FontChoice::Helvetica,
            size: 10.0,
            color: [0, 0, 0],
            margins: Margins {
                top: 36.0,
                bottom: 36.0,
                left: 72.0,
                right: 72.0,
            },
            first_number: 1,
            bates: Some(BatesSpec {
                prefix: "ABC".into(),
                digits: 6,
                start: 100,
                ..Default::default()
            }),
        }
    }

    fn frozen() -> Frozen {
        Frozen {
            date: "10/14/2026".into(),
            file: "Return.pdf".into(),
            bates: None,
        }
    }

    fn ctx(position: u32, count: u32, label: &str, bates: Option<u64>) -> PageCtx {
        PageCtx {
            geom: Geom::read(None, None, 0),
            position,
            count,
            label: label.into(),
            bates,
        }
    }

    #[test]
    fn tokens_and_layout() {
        let lines = layout(
            &spec(),
            &frozen(),
            &ctx(2, 10, "iii", Some(101)),
            &Std14::Helvetica,
        );
        let texts: Vec<&str> = lines.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(
            texts,
            [
                "Return.pdf",
                "10/14/2026",
                "iii",
                "Page 3 of 10",
                "ABC000101"
            ]
        );
        // Left at the margin, centred, right flush to its margin.
        assert_eq!((lines[0].x, lines[2].x), (72.0, 72.0));
        assert!((lines[3].x + lines[3].width / 2.0 - 306.0).abs() < 0.01);
        assert!((lines[1].x + lines[1].width - 540.0).abs() < 0.01);
        assert!((lines[4].x + lines[4].width - 540.0).abs() < 0.01);
        // The header's tops `top` below the edge, the footer's baselines
        // `bottom` above it.
        assert_eq!(lines[0].baseline, 36.0 + 10.0 * CAP_HEIGHT);
        assert_eq!(lines[4].baseline, 792.0 - 36.0);
    }

    /// Tokens are read from the texts alone: a file name, a date or a label
    /// holding one is drawn as it is, so what a header needs is what its
    /// texts use.
    #[test]
    fn a_token_in_a_value_is_drawn_as_it_is() {
        let spec = HeaderFooterSpec {
            texts: ["<<file>>", "<<date>>", "<<<1>>", "<<label>>", "", ""].map(String::from),
            ..spec()
        };
        let frozen = Frozen {
            file: "Exhibit <<bates>> of <<N>>.pdf".into(),
            date: "<<1>>".into(),
            bates: None,
        };
        let ctx = ctx(2, 10, "<<file>>", Some(101));
        let texts: Vec<String> = slots(&spec, &frozen, &ctx).map(|(_, text)| text).collect();
        assert_eq!(
            texts,
            ["Exhibit <<bates>> of <<N>>.pdf", "<<1>>", "<3", "<<file>>"]
        );
        let needs = needs(&spec);
        assert!(
            needs.index && needs.label && !needs.count && !needs.bates,
            "{needs:?}"
        );
    }

    /// `<<bates>>` reads the page's number, and says nothing without Bates
    /// numbering or on a page without a number.
    #[test]
    fn bates_text_pads_to_its_digits() {
        let bates = spec().bates.unwrap();
        assert_eq!(bates_text(&bates, 7), "ABC000007");
        let only = HeaderFooterSpec {
            texts: ["<<bates>>", "", "", "", "", ""].map(String::from),
            ..spec()
        };
        let draws =
            |spec: &HeaderFooterSpec, ctx: &PageCtx| slots(spec, &frozen(), ctx).next().is_some();
        assert!(!draws(&only, &ctx(0, 1, "1", None)));
        let unnumbered = HeaderFooterSpec {
            bates: None,
            ..only.clone()
        };
        assert!(!draws(&unnumbered, &ctx(0, 1, "1", Some(3))));
        assert!(draws(&only, &ctx(0, 1, "1", Some(3))));
    }

    /// A slot is as wide as its face draws it, whatever its characters.
    #[test]
    fn slots_are_as_wide_as_helvetica_draws_them() {
        let spec = HeaderFooterSpec {
            texts: ["Il “€”", "Œuvre", "Ω", "", "", ""].map(String::from),
            ..spec()
        };
        let lines = layout(&spec, &frozen(), &ctx(0, 1, "1", None), &Std14::Helvetica);
        let widths: Vec<f32> = lines.iter().map(|l| l.width).collect();
        // I l space “ € ” at 10 pt; Œ u v r e; Ω draws `?`.
        let at_10 = |w: f32| w * 10.0 / 1000.0;
        assert_eq!(
            widths[0],
            at_10(278.0 + 222.0 + 278.0 + 333.0 + 556.0 + 333.0)
        );
        assert_eq!(widths[1], at_10(1000.0 + 556.0 + 500.0 + 333.0 + 556.0));
        assert_eq!(widths[2], at_10(556.0));
        assert!(unencodable(&spec, &frozen(), &Std14::Helvetica));
        let fine = HeaderFooterSpec {
            texts: ["Œuvre <<1>>", "", "", "", "", ""].map(String::from),
            ..spec
        };
        assert!(!unencodable(&fine, &frozen(), &Std14::Helvetica));
    }

    /// A label is checked apart, and only when a text draws it.
    #[test]
    fn a_label_is_unencodable_only_where_it_is_drawn() {
        let face = Std14::Helvetica;
        assert!(label_unencodable(&spec(), "Ω-1", &face));
        assert!(!label_unencodable(&spec(), "é-1", &face));
        let unlabeled = HeaderFooterSpec {
            texts: ["<<1>>", "", "", "", "", ""].map(String::from),
            ..spec()
        };
        assert!(!label_unencodable(&unlabeled, "Ω-1", &face));
    }
}
