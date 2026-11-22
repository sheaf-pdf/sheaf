//! The content interpreter: what content draws, an operator at a time, in
//! the graphics and text state it's drawn in, as PDFium interprets it, and
//! content written again from what it draws. It is pdf-gfx's (M18 slice
//! 23; it began as sheaf's own, M11 slice 3), and its names are the
//! door's ([`crate::pdf::gfx`]), re-exported here: each operator, string
//! shown, path, image, shading, form and marked range handed to a visitor
//! as an [`Event`] in the [`State`] it's drawn in; what a visitor reads
//! ([`Opts`], [`Wants`]), and whether forms are drawn where they're used
//! or each judged on its own ([`Forms`]); what it came to ([`Summary`])
//! or why pdf-gfx stopped ([`GfxError`]).
//!
//! What sheaf adds is where its sources, caps and rule for an object that
//! doesn't read meet the family's: [`interpret`] and [`encode`] read a
//! page from any of sheaf's sources ([`ReadObjects`], as [`Objects`]
//! gives them, with the reason each object that doesn't read gives),
//! what the source doesn't keep (an XObject's dictionary, a font's
//! stream's) held for the call alone ([`Unkept`]), so that interpreting
//! pages, however many, keeps nothing more of the source, under sheaf's
//! caps ([`Limits::gfx`]). An object the source can't read
//! for a reason that isn't the file's (I/O, a password, a cap) stops
//! either, as pdf-gfx's own error ([`Error::Gfx`]); one that is damaged,
//! which pdf-gfx reads as none and never fails for, stops one that draws
//! forms ([`Forms::Descend`]), where text is to be taken out of what's
//! drawn through it, as an error of sheaf's own ([`Error::Damaged`]), and
//! is none where each form is judged on its own ([`Forms::Skip`]), as
//! sheaf read it before.

use std::ops::ControlFlow;

use crate::pdf::fault::{Place, Sink};
use crate::pdf::gfx::{self, Fault};
use crate::pdf::{Object, ObjectId};

pub use crate::pdf::gfx::{
    Cap, EncodedPage, Error as GfxError, Event, Forms, Glyph, ImageOf, Kind, Mark, MarkProps,
    Matrix, Opts, Rect, Refused, Seg, State, Summary, Unwritable, Wants,
};

use super::Fonts;
use super::page::{Concat, Objects, ReadObjects, Unkept};
use crate::source::Limits;

/// Why an interpretation, or content written again, stopped: pdf-gfx's
/// reason, or sheaf's rule for a damaged object where forms are drawn.
/// The two are told apart by the variant: pdf-gfx never fails for the
/// file's damage (its [`GfxError::Unreadable`] is a caller's reason alone:
/// I/O, a password, a cap), and sheaf's rule is never one of its values.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// pdf-gfx stopped, and why.
    #[error(transparent)]
    Gfx(#[from] GfxError),
    /// An object pdf-gfx was told is damaged, the first, which it read as
    /// none: content that draws forms ([`Forms::Descend`], and
    /// [`encode`]) is read through no damaged object. Never where each
    /// form is judged on its own ([`Forms::Skip`]).
    #[error(
        "object {} {} R is damaged, and content that draws forms is read through no damaged object",
        .0.number, .0.generation
    )]
    Damaged(ObjectId),
}

/// Interprets `content`, drawn with `resources` (a page's or a form's
/// `/Resources` as written), its objects read from `objects` and its
/// fonts through `fonts`, a source's, within `lim`, handing each event to
/// `visit` in order, until the end or until `visit` breaks: pdf-gfx's
/// [`interpret_with`](crate::pdf::gfx::interpret_with), what `objects`
/// doesn't keep held until it ends ([`Unkept`]). Where forms are
/// drawn ([`Forms::Descend`]), an object pdf-gfx is told is damaged, and
/// reads as none, fails it ([`Error::Damaged`]), after the events it
/// handed.
pub fn interpret<R: ReadObjects + ?Sized>(
    objects: &R,
    content: &Concat,
    resources: Option<&Object>,
    lim: &Limits,
    fonts: &Fonts,
    opts: Opts,
    visit: impl FnMut(Event<'_>) -> ControlFlow<()>,
) -> Result<Summary, Error> {
    let unkept = Unkept::default();
    let (objects, lim) = (Objects::new(objects, &unkept), lim.gfx());
    match opts.forms {
        Forms::Skip => Ok(gfx::interpret_with(
            objects,
            content,
            resources,
            &lim,
            fonts,
            opts,
            &mut (),
            visit,
        )?),
        Forms::Descend => {
            let mut sink = FailClosed::default();
            let read = gfx::interpret_with(
                objects, content, resources, &lim, fonts, opts, &mut sink, visit,
            );
            sink.failed(read)
        }
    }
}

/// `content`, drawn with `resources` from `objects`' objects in `fonts`
/// within `lim`, and each form it draws, written again so no line depends
/// on another: pdf-gfx's [`encode_with`](crate::pdf::gfx::encode_with),
/// what `objects` doesn't keep held until it ends, as in [`interpret`].
/// It draws forms, so an object it's told is damaged fails it
/// ([`Error::Damaged`]).
pub fn encode<R: ReadObjects + ?Sized>(
    objects: &R,
    content: &Concat,
    resources: Option<&Object>,
    lim: &Limits,
    fonts: &Fonts,
) -> Result<EncodedPage, Error> {
    let (unkept, mut sink) = (Unkept::default(), FailClosed::default());
    let written = gfx::encode_with(
        Objects::new(objects, &unkept),
        content,
        resources,
        &lim.gfx(),
        fonts,
        &mut sink,
    );
    sink.failed(written)
}

/// sheaf's rule for an object that doesn't read where forms are drawn:
/// the first that pdf-gfx is told is damaged ([`Fault::Damaged`]) is kept,
/// and fails what was read ([`FailClosed::failed`]). It wants every fault,
/// as a sink that counts them does, so pdf-gfx looks up what it looks up
/// only for a sink (named color spaces and patterns) and reads the same.
#[derive(Debug, Default)]
struct FailClosed {
    damaged: Option<ObjectId>,
}

impl Sink<Fault> for FailClosed {
    fn fault(&mut self, _: Place, fault: Fault) {
        if let Fault::Damaged { id } = fault {
            self.damaged.get_or_insert(id);
        }
    }
}

impl FailClosed {
    /// `read`, failed for the damaged object it was told of, where it was
    /// told of one: that came first, since pdf-gfx tells nothing once a
    /// run has failed, and stops at what else fails it.
    fn failed<T>(self, read: Result<T, GfxError>) -> Result<T, Error> {
        match self.damaged {
            Some(id) => Err(Error::Damaged(id)),
            None => Ok(read?),
        }
    }
}
