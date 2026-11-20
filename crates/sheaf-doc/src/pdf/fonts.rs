//! Fonts through `pdf-fonts` and `pdf-gfx` (M18 slice 22): a file's font
//! as read ([`Font`]), under its caps ([`Limits`]), the standard 14
//! ([`Std14`]) with their metrics ([`FontMetrics`], [`GlyphBox`]), and
//! what a font text is laid out in gives of itself ([`LayoutFont`]): a
//! character's codes and how far it advances. `crate::content`
//! re-exports what sheaf draws and reads with.
//!
//! A font is read from a source's objects through [`Resolve`] and
//! [`Streams`]; `crate::content::Objects` is any of sheaf's sources as
//! both.
//!
//! [`Resolve`]: super::Resolve
//! [`Streams`]: super::Streams

pub use pdf_fonts::{
    Descendant, Error, Font, FontApprox, FontId, FontKind, FontMetrics, FontProgram, FontScope,
    Fonts, GlyphBox, Limits, Std14, VerticalMetrics,
};
pub use pdf_gfx::LayoutFont;

/// What a code of a standard encoding draws: asked by tests alone.
#[cfg(test)]
pub use pdf_fonts::Encoding;
