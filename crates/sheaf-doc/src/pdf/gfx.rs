//! Content interpreted and written again through `pdf-gfx` (M18 slice
//! 23): a page's content walked an operator at a time
//! ([`interpret_with`]) under its caps ([`Limits`]), what each step does
//! ([`Event`], [`Kind`]) in the state it's done in ([`State`]), what it
//! came to ([`Summary`]) or why it stopped ([`Error`]), what it read past
//! ([`Fault`], told to a [`Sink`](super::fault::Sink)); and the content
//! written again so no line depends on another ([`encode_with`],
//! [`EncodedPage`]). The events carry the family's geometry ([`Rect`],
//! [`Matrix`], [`Point`]; a quad is a field's type nothing of sheaf's
//! names yet). And content drawn on a [`Canvas`], over pdf-content-streams'
//! writer: an annotation's appearance (M18 slice 24, [`super::annots`]),
//! the family's geometry its points; and a text field's, from a border and
//! a default appearance string (M10 slice 6, [`super::forms`]).
//!
//! A page is read from a source's objects through [`Resolve`] and
//! [`Streams`]; `crate::content::Objects` is any of sheaf's sources as
//! both, as for fonts ([`super::fonts`]).
//!
//! [`Resolve`]: super::Resolve
//! [`Streams`]: super::Streams

pub use pdf_gfx::{
    Canvas, Cap, EncodedPage, Error, Event, Fault, Forms, Glyph, ImageOf, Kind, Limits, Mark,
    MarkProps, Matrix, Opts, Point, Rect, Refused, Seg, State, Summary, Unwritable, Wants,
    encode_with, interpret_with,
};

/// Text in a font and a fallback for what it can't draw (M10 slice 7): the
/// two as a set of fonts, each face drawn under its resource
/// ([`WithFallback`]), and the face that draws a grapheme cluster of text,
/// the first that has all of it ([`face_for`]), or the character none
/// draws ([`UnencodableChar`]). An embedded program is a
/// [`LayoutFont`](super::fonts::LayoutFont) too
/// ([`Program`](super::fonts::Program)).
pub use pdf_gfx::{UnencodableChar, WithFallback, face_for};

/// A box's border as a [`Canvas`] draws it ([`BoxBorder`], its
/// [`BoxBorderStyle`]), and a default appearance string parsed
/// ([`DefaultAppearance`]): what a text field's appearance is made with
/// (M10 slice 6, [`super::forms`]).
pub use pdf_gfx::{BoxBorder, BoxBorderStyle, DefaultAppearance};

/// What an event carries that only tests name: a clip's parts, an image
/// and a shading drawn, the marked ranges an event is in, a glyph's quad,
/// and which use of a form is drawn.
#[cfg(test)]
pub use pdf_gfx::{ClipPart, ImageDrawn, Marks, Quad, ShadingDrawn, UseId};

/// What a [`State`] holds that only the corpus's events dump names: its
/// color spaces, rendering intent, blend mode and soft mask.
#[cfg(test)]
pub use pdf_gfx::{ActiveColorSpace, BlendState, RenderingIntent, SoftMask, SoftMaskKind};
