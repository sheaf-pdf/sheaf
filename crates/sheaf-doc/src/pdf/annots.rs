//! Annotations through `pdf-annots` (M18 slice 24).
//!
//! **Reading.** An annotation as the family reads it ([`Annot`], and what
//! it is by its subtype, [`TypedAnnot`]), each dictionary read alone by
//! [`read`] under PDFium's rules, or its appearances alone
//! ([`appearances`]: `/AP` and `/AS` and nothing else of it, so no cap on
//! the rest can refuse them). What a flatten draws of one is the form its
//! appearances ([`Appearances`]) name, placed on its box by the family's
//! placement ([`placement`]). These take any resolver of the family's and
//! the caps to read under; which objects of a source they read, and
//! sheaf's caps, are `crate::source::annots`.
//!
//! **Appearances.** An appearance as the parts of the form XObject that
//! holds it ([`SynthesizedAp`]: its box, its matrix, its content and what
//! that draws with, by what each is), whether the family drew it (its
//! `synth_*`, in place) or sheaf did, on pdf-gfx's
//! [`Canvas`](super::gfx::Canvas) ([`SynthesizedAp::drawn`]). A mark's
//! appearance reaches the writer as one, whoever drew it, and the writer
//! makes its form from those parts (`crate::content::sink::form`).
//!
//! **What stays sheaf's.** The walk of a page's `/Annots` and which of its
//! entries are sheaf's own marks (`crate::file`), so none of those reaches
//! the family's reader; the mark another tool's annotation becomes
//! (`crate::import`); a mark's dictionary, its layout, and the drawing of
//! the kinds whose look in the family isn't sheaf's (`crate::annot`); and
//! which state a flatten draws where none is named (`source::showing`).
//! The family's reading of a page's every entry (`page_annots`), which
//! ends the page at one entry's error, and its flatten of a page
//! dictionary (`plan_flatten`) aren't used.

use super::gfx::{Matrix, Rect};
use crate::pdf::{Dictionary, ObjectId, Resolve};

pub use pdf_annots::{
    Annot, AnnotColor, AnnotFlags, AnnotSubtype, AppearanceEntry, Appearances, AttachmentIcon,
    BorderStyle, Cap, Error, FileAttachmentAnnot, FreeTextIntent, Limits, LineEnding, LineIntent,
    PolyIntent, PolyKind, ReplyKind, ShapeKind, StampName, SynthesizedAp, TextMarkupKind,
    TypedAnnot,
};

/// The appearances the family draws of sheaf's marks (`synth_*`), in place
/// ([`ApRect::InPlace`]), and what they take ([`Border`], [`PaintStyle`],
/// [`QuadPoints`]); and the shapes they draw by, which the screen paints
/// from too ([`caret_outline`], [`attachment_icon_strokes`],
/// [`cloud_outline`], [`cloud_step`]).
pub use pdf_annots::{
    ApRect, Border, PaintStyle, QuadPoints, attachment_icon_strokes, caret_outline, cloud_outline,
    cloud_step, synth_caret, synth_cloud, synth_file_attachment, synth_ink, synth_poly,
    synth_shape, synth_text_markup,
};

/// The rules sheaf reads an annotation under: PDFium's, the page view's
/// engine.
const RULES: pdf_objects::pdf_tokenizer::Rules = pdf_objects::pdf_tokenizer::Rules::Pdfium;

/// Annotation `dict`, object `id` (None for one written in a page's
/// `/Annots`), as the family reads it whole from `objects` under PDFium's
/// rules and `limits`: an error where what it reads is past one of the
/// caps, or a string it reads is still encrypted.
pub fn read<'o, R: Resolve<'o>>(
    objects: R,
    dict: &'o Dictionary,
    id: Option<ObjectId>,
    limits: &Limits,
) -> Result<Annot<'o>, Error> {
    Annot::from_dict_with(objects, dict, id, RULES, limits, &mut ())
}

/// Annotation `dict`'s appearances alone (`/AP` and `/AS`), as [`read`]
/// reads them with the rest: none where it has neither. Nothing else of
/// the annotation is read, so only an object that can't be read is an
/// error.
pub fn appearances<'o, R: Resolve<'o>>(
    objects: R,
    dict: &'o Dictionary,
    id: Option<ObjectId>,
    limits: &Limits,
) -> Result<Option<Appearances>, Error> {
    Appearances::read(objects, dict, id, RULES, limits)
}

/// Form `form`, an annotation's appearance, placed on `rect` as ISO
/// 32000-2 §12.5.5 places one, by the family's reading
/// (`stream_placement_matrix`, under PDFium's rules and `limits`): the
/// form's `/BBox`, its `/Matrix` (the identity where it has none), and the
/// matrix that takes the box the second makes of the first onto `rect`.
/// None where it isn't a form with a box, or that box has no area.
pub fn placement<'o, R: Resolve<'o>>(
    objects: R,
    form: ObjectId,
    rect: Rect,
    limits: &Limits,
) -> Option<(Rect, Matrix, Matrix)> {
    pdf_annots::stream_placement_matrix(objects, form, rect, limits)
        .ok()
        .flatten()
}
