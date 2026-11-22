//! A source's annotations as `pdf-annots` reads them (M18 slice 24): the
//! family's readers ([`crate::pdf::annots`]) over a source's objects as
//! sheaf reads an annotation's parts, under sheaf's caps ([`limits`]).
//! What the source keeps is lent from it, a stream's dictionary is held by
//! an [`Unkept`] of the caller's and its data never read, and an object
//! that doesn't read is absent ([`Parts`]).

use super::Limits;
use crate::content::{Objects, ReadObjects, Unkept};
use crate::pdf::annots::{self, Annot, Appearances, Error};
use crate::pdf::gfx::{Matrix, Rect};
use crate::pdf::{Dictionary, Object, ObjectId, Resolve};

/// The caps an annotation is read under: the family's, but for the ones
/// sheaf has none of (a page's annotations, a markup's quadrilaterals, a
/// path's points), past reach, and references followed as deep as
/// sheaf's reader follows them by default (an annotation is read from any
/// of sheaf's sources, a file built in memory too, which has no caps of
/// its own). What's left of the family's own (a `/DA`'s tokens, an action
/// chain, a destination's name tree) refuses an annotation read whole
/// past it.
pub fn limits() -> annots::Limits {
    let mut limits = annots::Limits::DEFAULT;
    limits.hops = Limits::DEFAULT.reader.max_depth;
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
/// page's `/Annots`), as the family reads it whole under PDFium's rules
/// and [`limits`], each stream it reads held by `unkept`: an error where
/// what it reads is past one of the family's caps it keeps, or a string
/// it reads is still encrypted.
pub fn read<'o, R: ReadObjects + ?Sized>(
    source: &'o R,
    unkept: &'o Unkept,
    dict: &'o Dictionary,
    id: Option<ObjectId>,
) -> Result<Annot<'o>, Error> {
    annots::read(Parts(Objects::new(source, unkept)), dict, id, &limits())
}

/// Annotation `dict`'s appearances alone (`/AP`, `/AS`), read as [`read`]
/// reads them and nothing else of it: no cap on the rest of the
/// annotation (its actions, its `/DA`) can refuse them.
pub fn appearances<'o, R: ReadObjects + ?Sized>(
    source: &'o R,
    unkept: &'o Unkept,
    dict: &'o Dictionary,
    id: Option<ObjectId>,
) -> Result<Option<Appearances>, Error> {
    annots::appearances(Parts(Objects::new(source, unkept)), dict, id, &limits())
}

/// Form `form` of `source`, an annotation's appearance, placed on `rect`
/// as ISO 32000-2 §12.5.5 places one, by the family's reading under
/// [`limits`], what it reads held by `unkept` as [`read`] holds it: the
/// form's `/BBox`, its `/Matrix` (the identity where it has none), and the
/// matrix that takes the box the second makes of the first onto `rect`.
/// None where it isn't a form with a box, or that box has no area.
pub fn placement<'o, R: ReadObjects + ?Sized>(
    source: &'o R,
    unkept: &'o Unkept,
    form: ObjectId,
    rect: Rect,
) -> Option<(Rect, Matrix, Matrix)> {
    annots::placement(Parts(Objects::new(source, unkept)), form, rect, &limits())
}
