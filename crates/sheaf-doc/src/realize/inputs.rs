//! What a consumer asks a page to draw (M10.5 §10.1, §10.4): each passes
//! its own inputs, so hiding a comment on screen changes neither print
//! preview nor what OCR reads.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use crate::annot::AnnotId;
use crate::layers::LayerId;
use crate::store::BlobId;

/// Who draws sheaf's marks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MarkDraw {
    /// GPUI draws them over the page, so PDFium leaves them out (until
    /// M10 slice 12).
    ByHost,
    /// PDFium draws them from their appearances, as the file will.
    ByEngine,
    /// Nobody does.
    Off,
}

/// What a page is drawn with, beside its own content.
#[derive(Debug, Clone, PartialEq)]
pub struct ViewInputs {
    pub marks: MarkDraw,
    /// What generators draw (the header and footer); false for analysis.
    pub generators: bool,
    /// Recognized text, which is invisible: it matters to text reads.
    pub ocr_over: bool,
    /// The form's widgets.
    pub widgets: bool,
    /// Other tools' comments and links.
    pub foreign: bool,
    /// Annotations hidden by the comment filter or Hide Comments.
    pub hidden: Arc<im::HashSet<AnnotId>>,
    /// Marks being dragged: GPUI draws their ghost, and the realization
    /// leaves the originals out.
    pub dragging: Arc<im::HashSet<AnnotId>>,
    /// The layers panel's choice of layers, a view of each source's
    /// catalog.
    pub ocg_off: Option<Arc<OcgOverride>>,
}

/// The layers the layers panel turns off, by the source whose they are:
/// a source with an entry shows every layer but those, whatever its file's
/// default configuration says. Never saved.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct OcgOverride {
    pub off: BTreeMap<BlobId, BTreeSet<LayerId>>,
}

impl ViewInputs {
    /// The screen's: everything, sheaf's marks by GPUI, nothing hidden.
    pub fn screen() -> ViewInputs {
        ViewInputs {
            marks: MarkDraw::ByHost,
            generators: true,
            ocr_over: true,
            widgets: true,
            foreign: true,
            hidden: Arc::default(),
            dragging: Arc::default(),
            ocg_off: None,
        }
    }

    /// A plugin's render, and the reads it records: the screen's, with
    /// sheaf's marks drawn by PDFium, as the saved file will draw them
    /// (§14.3). One constructor for both, so a plugin's reads are of what
    /// it was shown.
    pub fn plugin() -> ViewInputs {
        ViewInputs {
            marks: MarkDraw::ByEngine,
            ..ViewInputs::screen()
        }
    }

    /// Print preview's: with `markup` its comments and marks, and without,
    /// neither; the form's widgets either way.
    pub fn print(markup: bool) -> ViewInputs {
        ViewInputs {
            marks: match markup {
                true => MarkDraw::ByHost,
                false => MarkDraw::Off,
            },
            foreign: markup,
            ..ViewInputs::screen()
        }
    }

    /// The page's own content alone, as OCR and blank detection read it:
    /// no generators, recognized text, widgets or annotations, so an
    /// unsaved header is never read as the page's. What it draws is what
    /// the page shows ([`Key::Page`](crate::prim::Key::Page)) alone.
    pub fn content_only() -> ViewInputs {
        ViewInputs {
            marks: MarkDraw::Off,
            generators: false,
            ocr_over: false,
            widgets: false,
            foreign: false,
            ..ViewInputs::screen()
        }
    }

    /// Whether PDFium draws annotations other than widgets: others' comments
    /// and links, or sheaf's marks.
    pub fn draws_annots(&self) -> bool {
        self.foreign || self.marks == MarkDraw::ByEngine
    }
}
