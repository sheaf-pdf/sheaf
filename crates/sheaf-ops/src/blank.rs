//! Find blank pages (Document > Find Blank Pages…, `sheaf remove-blanks`):
//! duplex backs, scanner fillers, empty separators.
//!
//! Content is checked before pixels, since it's cheap and certain:
//! - a page with sheaf's marks, other annotations, links or form fields
//!   isn't blank;
//! - a page with text isn't either, unless the text says it's
//!   intentionally blank ("This page intentionally left blank", perhaps
//!   with a page number or header).
//!
//! The rest are rendered at about 60 dpi and judged by their pixels with
//! [`sheaf_image::blank`], which the scan pipeline also uses on cleaned
//! photos.
//!
//! A page is read as it's drawn, through the document's views, with its
//! content alone (M10.5 §10.4): a header or watermark sheaf draws, saved
//! or not, never makes a page content. Its text is read with its
//! recognized text, saved or not, which is the page's too, and its pixels
//! without, recognized text being invisible. An edited, inserted or
//! duplicated page is read from its realization, and so is every page of
//! a source PDFium refuses.
//! The engines open in the background's budget, so a search never closes
//! one the screen shows.

use std::collections::HashSet;
use std::path::Path;
use std::sync::LazyLock;

use anyhow::{Context, Result};
use futures::executor::block_on;
use in_progress::{NoProgress, Progress};
use serde::{Deserialize, Serialize};
use sheaf_doc::export::{Export, Out};
use sheaf_doc::materialize::AskAnswers;
use sheaf_doc::realize::ViewInputs;
use sheaf_doc::{PAGES, PageId, Snapshot, TextOpts, TextService};
use sheaf_edit::Rotation;
use sheaf_image::blank::{Sensitivity, Verdict, is_blank};
use sheaf_image::raster::PixelFormat;
use sheaf_render::{Budget, Job};

use crate::session::{Decided, Session};
use crate::text::Text;
use crate::views::Views;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Reason {
    Blank,
    /// A little ink in a few places: worth a look, not deleted by default.
    NearlyBlank,
    /// Says it's intentionally left blank.
    IntentionallyBlank,
}

impl Reason {
    pub fn label(self) -> &'static str {
        match self {
            Reason::Blank => "blank",
            Reason::NearlyBlank => "nearly blank",
            Reason::IntentionallyBlank => "intentionally blank",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    /// 0-based position in the document.
    pub index: usize,
    pub id: PageId,
    /// Share of the page that's ink (0 for an intentionally blank one).
    pub ink: f32,
    pub reason: Reason,
}

impl Candidate {
    /// Whether it's deleted unless someone says otherwise.
    pub fn drop_by_default(&self) -> bool {
        self.reason != Reason::NearlyBlank
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct BlankParams {
    pub sensitivity: Sensitivity,
    /// Keep pages that say they're intentionally blank.
    pub keep_intentional: bool,
}

static INTENTIONAL: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(
        r"(?i)(this\s+page\s+(is\s+|has\s+been\s+|was\s+)?)?(intentionally|deliberately|purposely|purposefully)\s+(left\s+)?blank|(this\s+)?page\s+left\s+blank",
    )
    .expect("valid")
});

/// What a page's text says about it.
#[derive(Debug, PartialEq)]
enum Says {
    /// No real text: the pixels decide.
    Nothing,
    IntentionallyBlank,
    Content,
}

fn text_says(text: &str) -> Says {
    let letters = |s: &str| s.chars().filter(|c| c.is_alphanumeric()).count();
    if letters(text) < 3 {
        return Says::Nothing;
    }
    if INTENTIONAL.is_match(text) {
        // Room for a page number, a header or a footer, not a paragraph.
        let rest = INTENTIONAL.replace_all(text, "");
        if letters(&rest) <= 60 {
            return Says::IntentionallyBlank;
        }
    }
    Says::Content
}

/// The blank pages among `indexes` (0-based; none: every page), in order,
/// each read as `views` draw it.
pub fn find(
    snap: &Snapshot,
    views: &Views,
    indexes: Option<&[usize]>,
    params: BlankParams,
    progress: &dyn Progress,
) -> Result<Vec<Candidate>> {
    let st = &snap.state;
    let wanted: Vec<usize> = match indexes {
        Some(ix) => ix
            .iter()
            .copied()
            .filter(|&i| i < st.page_count())
            .collect(),
        None => (0..st.page_count()).collect(),
    };
    progress.set_total(Some(wanted.len() as u64), PAGES);
    let pages: Vec<_> = st.pages().collect();
    let text = Text::new(views.clone(), Budget::Background);
    let content = ViewInputs::content_only();
    let read = TextOpts {
        inputs: ViewInputs {
            ocr_over: true,
            ..content.clone()
        },
    };
    let mut out = Vec::new();
    for (done, i) in wanted.into_iter().enumerate() {
        progress.check()?;
        progress.set(done as u64 + 1);
        let (id, def, _) = pages[i];
        // sheaf's marks, others' annotations, links, fields.
        if st.annots_on(id).next().is_some() {
            continue;
        }
        let base = &def.base;
        let has_annots = (base.src.page(base.index)).is_some_and(|page| !page.annots.is_empty());
        if has_annots {
            continue;
        }
        // Text, the file's and any recognized, never a header's.
        let says = (TextService::chars(&text, snap, id, &read))
            .map(|chars| text_says(&chars.text(0, chars.len())))
            .unwrap_or(Says::Nothing);
        match says {
            Says::IntentionallyBlank => {
                if !params.keep_intentional {
                    out.push(Candidate {
                        index: i,
                        id,
                        ink: 0.0,
                        reason: Reason::IntentionallyBlank,
                    });
                }
                continue;
            }
            Says::Content => continue,
            Says::Nothing => {}
        }
        let shown = snap.geom(id).shown();
        let job = Job::Page {
            width: (shown.width * 60.0 / 72.0).round().max(16.0) as u32,
            rotation: Rotation::R0,
        };
        let (format, budget) = (PixelFormat::Rgba, Budget::Background);
        let rx = views.render(st, id, &content, job, format, u8::MAX, budget);
        let raster = block_on(rx).context("the render thread is gone")??.raster;
        let gray = raster.to_gray().context("short render")?;
        let (reason, ink) = match is_blank(&gray, params.sensitivity) {
            Verdict::Blank { ink } => (Reason::Blank, ink),
            Verdict::NearlyBlank { ink } => (Reason::NearlyBlank, ink),
            Verdict::NotBlank { .. } => continue,
        };
        out.push(Candidate {
            index: i,
            id,
            ink,
            reason,
        });
    }
    Ok(out)
}

/// Write `session`'s current version without the `drop` pages to `kept`,
/// and, with `rejects`, the dropped pages to a file of their own, each
/// bookmarked with where it was and why ("p. 4 — blank"), so nothing is
/// lost for good: two exports ([`Export::extract`] and
/// [`Export::rejects`]), which replace files there when `overwrite` and
/// stop at what either asks that `answers` don't answer ([`NeedsDecision`])
/// before either is written. The document is left as it was. Returns how
/// many pages `kept` has.
///
/// [`NeedsDecision`]: crate::session::NeedsDecision
pub fn write(
    session: &Session,
    drop: &[Candidate],
    kept: &Path,
    rejects: Option<&Path>,
    overwrite: bool,
    answers: &AskAnswers,
) -> Result<usize> {
    let from = session.export_from();
    let gone: HashSet<PageId> = drop.iter().map(|c| c.id).collect();
    let keep: Vec<PageId> = (from.snap.state.order().iter())
        .copied()
        .filter(|id| !gone.contains(id))
        .collect();
    if keep.is_empty() {
        anyhow::bail!("every page is blank; nothing to keep");
    }
    let to = |path: &Path| Out::File {
        path: path.into(),
        overwrite,
    };
    let titled: Vec<(PageId, String)> = (drop.iter())
        .map(|c| (c.id, format!("p. {} — {}", c.index + 1, c.reason.label())))
        .collect();
    let rejected = (rejects.filter(|_| !drop.is_empty()))
        .map(|rejects| Export::rejects(from.clone(), &titled, to(rejects)));
    // What the second file asks stops both before the first is written.
    if let Some(rejected) = &rejected {
        sheaf_doc::export::check(rejected, answers, &NoProgress).decided()?;
    }
    let extract = Export::extract(from, &keep, to(kept));
    session.export(extract, answers, &NoProgress).decided()?;
    if let Some(rejected) = rejected {
        session.export(rejected, answers, &NoProgress).decided()?;
    }
    Ok(keep.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intentionally_blank_phrases() {
        let says = |t: &str| text_says(t);
        assert_eq!(
            says("This page intentionally left blank"),
            Says::IntentionallyBlank
        );
        assert_eq!(
            says("Form 1040 (2025)   Page 4\nTHIS PAGE HAS BEEN INTENTIONALLY LEFT BLANK"),
            Says::IntentionallyBlank
        );
        assert_eq!(says("Intentionally blank"), Says::IntentionallyBlank);
        assert_eq!(says("  . , - "), Says::Nothing);
        // A paragraph that mentions it is content.
        let long = "Schedule B instructions: if a line does not apply, leave it blank. \
                    Lines intentionally left blank are not errors and need no explanation \
                    from the preparer or the taxpayer when filing.";
        assert_eq!(says(long), Says::Content);
        assert_eq!(says("Invoice 1234 total 56.00"), Says::Content);
    }
}
