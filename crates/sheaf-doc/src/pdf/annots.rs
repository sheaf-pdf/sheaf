//! Annotations through `pdf-annots` (M18 slice 24): an appearance as the
//! parts of the form XObject that holds it ([`SynthesizedAp`]: its box,
//! its matrix, its content and what that draws with, by what each is),
//! whether the family drew it or sheaf did, on pdf-gfx's
//! [`Canvas`](super::gfx::Canvas) ([`SynthesizedAp::drawn`]). A mark's
//! appearance reaches the writer as one, whoever drew it, and the writer
//! makes its form from those parts (`crate::content::sink::form`).

pub use pdf_annots::SynthesizedAp;

/// What only tests name yet, setting the family's reading and drawing
/// beside sheaf's (M18 slice 24): an annotation as the family reads it
/// ([`Annot`], [`TypedAnnot`] and their parts) under PDFium's rules
/// ([`Rules`]) and its caps ([`Limits`]), a page's annotations read whole
/// ([`page_annots_with`]), what its appearance is ([`Appearances`]) and
/// where its form is placed ([`stream_placement_matrix`]), and the
/// appearances the family draws (`synth_*`).
#[cfg(any(test, feature = "fixtures"))]
pub use pdf_annots::{
    Annot, AnnotColor, AnnotFlags, AnnotSubtype, AppearanceEntry, Appearances, AttachmentIcon,
    Border, BorderStyle, Cap, Error, FileAttachmentAnnot, FreeTextIntent, Limits, LineEnding,
    LineIntent, PaintStyle, PolyIntent, PolyKind, QuadPoints, ReplyKind, ShapeKind, StampName,
    TextMarkupKind, TypedAnnot, page_annots_with, placement_matrix, stream_placement_matrix,
    synth_ink, synth_poly, synth_shape, synth_text_markup,
};

/// A point of the family's geometry, as a `synth_*` takes a path's.
#[cfg(any(test, feature = "fixtures"))]
pub use pdf_annots::pdf_gfx::Point;

/// The rules a family reader reads by: ISO 32000-2's, or PDFium's, which
/// sheaf reads annotations by (see [`Annot`]).
#[cfg(any(test, feature = "fixtures"))]
pub use pdf_objects::pdf_tokenizer::Rules;
