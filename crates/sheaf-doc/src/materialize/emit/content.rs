//! What sheaf draws over a page (M10.5 §11.4), in the legacy shape until
//! the content stack's blocks and the generators (slices 17 and 18), drawn
//! through the installers the old writers used, with the emitter as their
//! [`Sink`](crate::content::sink::Sink):
//! - **OCR text** through `text_layer::install`: `/Contents` becomes `[q,
//!   base…, layer]`, the layer's streams marked `/SheafOcrLayer` and its
//!   text in `/SheafOcr`. A layer the page's content holds already (a
//!   re-OCR over a saved one) is taken out first, so the page has one.
//! - **The header and footer** through `stamps::install`, on each page it
//!   covers whose base doesn't draw it already (`stamps::drawn_over`): a
//!   page whose file sheaf saved with it isn't drawn on twice. Bates
//!   numbers count the pages it covers, in order.
//!
//! An overlay draws each only when its view asks for it, and finds where
//! a page is, whether the header and footer is on it and its Bates number
//! from the state's own tables, never by a walk of the pages.
//!
//! The page's own streams are referenced, never re-encoded. What the
//! installers add is keyed by digest: a resource ([`ObjKey::Res`], its
//! other objects [`ObjKey::ResPart`]) is written once a file, so every OCR
//! page draws in one font, and identical blocks ([`ObjKey::Block`]) are
//! one object.
//!
//! [`ObjKey::Res`]: crate::materialize::ObjKey::Res
//! [`ObjKey::ResPart`]: crate::materialize::ObjKey::ResPart
//! [`ObjKey::Block`]: crate::materialize::ObjKey::Block

use lopdf::Dictionary;

use super::{Cx, PdfVersion};
use crate::geom::Geom;
use crate::model::{DocState, PageDef, PageId};
use crate::stamps;

/// The PDF version what sheaf draws over a page showing `def` needs, the
/// header and footer's aside (a Rewrite draws it, until slice 17): its OCR
/// text's fonts. The planner reads it too, as an Append that needs a later
/// version than its file says so in the catalog (§11.7), which a
/// signature counts as any other change (§9.3).
pub(crate) fn needs(def: &PageDef) -> Option<PdfVersion> {
    def.content.ocr_text().map(|_| PdfVersion::CID_FONTS)
}

/// Whether what sheaf draws over `st`'s pages needs a later PDF version
/// than `version` ([`needs`]): nothing needs a later one than OCR text's
/// fonts.
pub(crate) fn needs_after(st: &DocState, version: PdfVersion) -> bool {
    version < PdfVersion::CID_FONTS
        && (st.pages()).any(|(_, def, _)| needs(def).is_some_and(|v| v > version))
}

/// What sheaf draws over page `id`, which shows `def` with `geom` at
/// place `pos`, drawn into `page`, its dictionary as emitted.
pub(crate) fn page(
    cx: &mut Cx,
    id: PageId,
    def: &PageDef,
    geom: &Geom,
    pos: usize,
    page: &mut Dictionary,
) {
    let view = cx.overlay.map(|o| o.inputs);
    if view.is_none_or(|v| v.ocr_over) {
        if let Some(layer) = def.content.ocr_text() {
            crate::text_layer::install(cx, page, layer);
        }
        if let Some(version) = needs(def) {
            cx.needs(version);
        }
    }
    let st = cx.st;
    // A derived copy's page draws what the copy says it drew.
    let (hf, bates) = match view {
        None => (
            stamps::drawn_over(st, id),
            cx.bates.get(pos).copied().flatten(),
        ),
        Some(v) => {
            let hf = (st.header_footer()).filter(|hf| {
                v.generators
                    && st.header_covers(id)
                    && !stamps::base_draws(&def.base.src, def.base.index, hf)
            });
            (hf, st.bates_of(id))
        }
    };
    let Some(hf) = hf else {
        return;
    };
    let facts = stamps::PageFacts {
        index: pos,
        count: st.page_count(),
        bates,
    };
    let placed = hf.layout(geom.shown(), &facts);
    if !placed.is_empty() {
        stamps::install(cx, page, &placed, hf.color, geom);
        cx.needs(PdfVersion::ARTIFACTS);
    }
}
