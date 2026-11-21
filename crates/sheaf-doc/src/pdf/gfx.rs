//! Content interpreted and written again through `pdf-gfx` (M18 slice
//! 23): a page's content walked an operator at a time
//! ([`interpret_with`]) under its caps ([`Limits`]), what each step does
//! ([`Event`], [`Kind`]) in the state it's done in ([`State`]), what it
//! came to ([`Summary`]) or why it stopped ([`Error`]), what it read past
//! ([`Fault`], told to a [`Sink`](super::fault::Sink)); and the content
//! written again so no line depends on another ([`encode_with`],
//! [`EncodedPage`]). The events carry the family's geometry ([`Rect`],
//! [`Matrix`]; a point and a quad are fields' types nothing of sheaf's
//! names yet).
//!
//! A page is read from a source's objects through [`Resolve`] and
//! [`Streams`]; `crate::content::Objects` is any of sheaf's sources as
//! both, as for fonts ([`super::fonts`]).
//!
//! [`Resolve`]: super::Resolve
//! [`Streams`]: super::Streams

pub use pdf_gfx::{
    Cap, EncodedPage, Error, Event, Fault, Forms, Glyph, ImageOf, Kind, Limits, Mark, MarkProps,
    Matrix, Opts, Rect, Refused, Seg, State, Summary, Unwritable, Wants, encode_with,
    interpret_with,
};

/// What an event carries that only tests name: a clip's parts, an image
/// and a shading drawn, the marked ranges an event is in, a point, a
/// glyph's quad, and which use of a form is drawn.
#[cfg(test)]
pub use pdf_gfx::{ClipPart, ImageDrawn, Marks, Point, Quad, ShadingDrawn, UseId};
