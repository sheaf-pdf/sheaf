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
use crate::model::{PageDef, PageId};
use crate::stamps;

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
    if let Some(layer) = def.content.ocr_text() {
        crate::text_layer::install(cx, page, layer);
        cx.needs(PdfVersion::CID_FONTS);
    }
    let input = cx.input;
    let st = &input.target;
    // A derived copy's page draws what the copy says it drew.
    let Some(hf) = stamps::drawn_over(st, id) else {
        return;
    };
    let facts = stamps::PageFacts {
        index: pos,
        count: st.page_count(),
        bates: cx.bates.get(pos).copied().flatten(),
    };
    let placed = hf.layout(geom.shown(), &facts);
    if !placed.is_empty() {
        stamps::install(cx, page, &placed, hf.color, geom);
        cx.needs(PdfVersion::ARTIFACTS);
    }
}
