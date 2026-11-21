//! Content interpreted and written again through `pdf-gfx` (M18 slice
//! 23): a page's content walked an operator at a time ([`interpret`],
//! [`interpret_with`]) under its caps ([`Limits`]), what each step does
//! ([`Event`], [`Kind`]) in the state it's done in ([`State`]), what it
//! came to ([`Summary`]) or why it stopped ([`Error`]), what it read past
//! ([`Fault`], told to a [`Sink`](super::fault::Sink)); and the content
//! written again so no line depends on another ([`encode`],
//! [`encode_with`], [`EncodedPage`]). The events carry the family's
//! geometry ([`Point`], [`Rect`], [`Quad`], [`Matrix`]).
//!
//! A page is read from a source's objects through [`Resolve`] and
//! [`Streams`]; `crate::content::Objects` is any of sheaf's sources as
//! both, as for fonts ([`super::fonts`]).
//!
//! [`Resolve`]: super::Resolve
//! [`Streams`]: super::Streams

pub use pdf_gfx::{
    Cap, ClipPart, EncodedPage, Error, Event, Fault, Forms, ImageDrawn, ImageOf, Kind, Limits,
    Mark, MarkProps, Marks, Matrix, Opts, Point, Quad, Rect, Refused, Seg, ShadingDrawn, State,
    Summary, Unwritable, UseId, Wants, encode, encode_with, interpret, interpret_with,
};
