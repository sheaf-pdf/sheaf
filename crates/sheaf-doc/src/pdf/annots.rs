//! Annotations through `pdf-annots` (M18 slice 24).
//!
//! **Reading.** An annotation as the family reads it ([`Annot`], and what
//! it is by its subtype, [`TypedAnnot`]), each dictionary read alone by
//! [`read`] under PDFium's rules and sheaf's caps ([`limits`]), from a
//! source's objects as sheaf reads an annotation's parts: what the source
//! keeps lent from it, a stream's dictionary held by an [`Unkept`] of the
//! caller's and its data never read, and an object that doesn't read
//! absent.
//!
//! **Appearances.** An appearance as the parts of the form XObject that
//! holds it ([`SynthesizedAp`]: its box, its matrix, its content and what
//! that draws with, by what each is), whether the family drew it (its
//! `synth_*`, in place) or sheaf did, on pdf-gfx's
//! [`Canvas`](super::gfx::Canvas) ([`SynthesizedAp::drawn`]). A mark's
//! appearance reaches the writer as one, whoever drew it, and the writer
//! makes its form from those parts (`crate::content::sink::form`).

use crate::content::{Objects, ReadObjects, Unkept};
use crate::pdf::{Dictionary, Object, ObjectId, Resolve};

pub use pdf_annots::{
    Annot, AnnotColor, AnnotFlags, AnnotSubtype, AppearanceEntry, AttachmentIcon, BorderStyle,
    Error, FileAttachmentAnnot, FreeTextIntent, Limits, LineEnding, LineIntent, PolyIntent,
    PolyKind, ReplyKind, ShapeKind, StampName, SynthesizedAp, TextMarkupKind, TypedAnnot,
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

/// The caps an annotation is read under: the family's, but for the ones
/// sheaf has none of (a page's annotations, a markup's quadrilaterals, a
/// path's points), past reach, and references followed as deep as
/// sheaf's reader follows them. What's left of the family's own (a `/DA`'s
/// tokens, an action chain, a destination's name tree) refuses an
/// annotation past it.
pub fn limits() -> Limits {
    let mut limits = Limits::DEFAULT;
    limits.hops = crate::source::Limits::DEFAULT.reader.max_depth;
    limits.annots_per_page = usize::MAX;
    limits.quads = usize::MAX;
    limits.points = usize::MAX;
    limits
}

/// A source's objects as an annotation's parts are read from them:
/// [`Objects`]' (what the source keeps lent from it, a stream's dictionary
/// held by the caller's [`Unkept`], its data unread), and one that doesn't
/// read, whatever the reason, absent, as sheaf has always read an
/// annotation's parts.
struct Parts<'a, R: ?Sized>(Objects<'a, R>);

impl<R: ?Sized> Clone for Parts<'_, R> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<R: ?Sized> Copy for Parts<'_, R> {}

impl<'a, R: ReadObjects + ?Sized> Resolve<'a> for Parts<'a, R> {
    fn resolve(&self, id: ObjectId) -> Option<&'a Object> {
        self.0.resolve(id)
    }
}

/// Annotation `dict` of `source`, object `id` (None for one written in a
/// page's `/Annots`), as the family reads it under PDFium's rules and
/// [`limits`], each stream it reads held by `unkept`: an error where what
/// it reads is past one of the family's caps it keeps, or a string it
/// reads is still encrypted.
pub fn read<'o, R: ReadObjects + ?Sized>(
    source: &'o R,
    unkept: &'o Unkept,
    dict: &'o Dictionary,
    id: Option<ObjectId>,
) -> Result<Annot<'o>, Error> {
    let parts = Parts(Objects::new(source, unkept));
    let rules = pdf_objects::pdf_tokenizer::Rules::Pdfium;
    Annot::from_dict_with(parts, dict, id, rules, &limits(), &mut ())
}
