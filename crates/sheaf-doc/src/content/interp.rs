//! The content interpreter: what content draws, an operator at a time, in
//! the graphics and text state it's drawn in, as PDFium interprets it, and
//! content written again from what it draws. It is pdf-gfx's (M18 slice
//! 23; it began as sheaf's own, M11 slice 3), and its names are the
//! door's ([`crate::pdf::gfx`]), re-exported here: each operator, string
//! shown, path, image, shading, form and marked range handed to a visitor
//! as an [`Event`] in the [`State`] it's drawn in; what a visitor reads
//! ([`Opts`], [`Wants`]), and whether forms are drawn where they're used
//! or each judged on its own ([`Forms`]); what it came to ([`Summary`])
//! or why it stopped ([`Error`]).
//!
//! What sheaf adds is where its sources, caps and rule for an object that
//! doesn't read meet the family's: [`interpret`] and [`encode`] read a
//! page from any of sheaf's sources ([`ReadObjects`], as [`Objects`]
//! gives them, with the reason each object that doesn't read gives),
//! under sheaf's caps ([`Limits::gfx`]). An object the source can't read
//! for a reason that isn't the file's (I/O, a password, a cap) stops
//! either; one that is damaged, which pdf-gfx reads as none, stops one
//! that draws forms ([`Forms::Descend`]), where text is to be taken out of
//! what's drawn through it, and is none where each form is judged on its
//! own ([`Forms::Skip`]), as sheaf read it before.

use std::ops::ControlFlow;

use crate::pdf::fault::{Place, Sink};
use crate::pdf::gfx::{self, Fault};
use crate::pdf::{Object, ObjectId, Unreadable, Why};

pub use crate::pdf::gfx::{
    Cap, EncodedPage, Error, Event, Forms, Glyph, ImageOf, Kind, Mark, MarkProps, Matrix, Opts,
    Rect, Refused, Seg, State, Summary, Unwritable, Wants,
};

use super::Fonts;
use super::page::{Concat, Objects, ReadObjects};
use crate::source::Limits;

/// Interprets `content`, drawn with `resources` (a page's or a form's
/// `/Resources` as written), its objects read from `objects` and its
/// fonts through `fonts`, a source's, within `lim`, handing each event to
/// `visit` in order, until the end or until `visit` breaks: pdf-gfx's
/// [`interpret_with`](crate::pdf::gfx::interpret_with). Where forms are
/// drawn, an object pdf-gfx is told is damaged, and reads as none, fails
/// it ([`Error::Unreadable`], [`Why::Damaged`]), after the events it
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
    let (objects, lim) = (Objects(objects), lim.gfx());
    match opts.forms {
        Forms::Skip => gfx::interpret_with(
            objects,
            content,
            resources,
            &lim,
            fonts,
            opts,
            &mut (),
            visit,
        ),
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
/// on another: pdf-gfx's [`encode_with`](crate::pdf::gfx::encode_with).
/// It draws forms, so an object it's told is damaged fails it, as in
/// [`interpret`].
pub fn encode<R: ReadObjects + ?Sized>(
    objects: &R,
    content: &Concat,
    resources: Option<&Object>,
    lim: &Limits,
    fonts: &Fonts,
) -> Result<EncodedPage, Error> {
    let mut sink = FailClosed::default();
    let written = gfx::encode_with(
        Objects(objects),
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
    fn failed<T>(self, read: Result<T, Error>) -> Result<T, Error> {
        match self.damaged {
            Some(id) => Err(Error::Unreadable(Unreadable::new(
                id,
                Why::Damaged,
                "damaged, and content that draws forms is read through no damaged object",
            ))),
            None => read,
        }
    }
}
