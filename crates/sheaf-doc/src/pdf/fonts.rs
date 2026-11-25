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
//! And a TrueType program embedded (M10 slice 7, pdf-fonts' `embed`): a
//! [`Program`] opened once, the glyphs a file draws in it, each with the
//! text it stands for ([`Glyphs`]), and the font written of them
//! ([`embed_write`], under [`EmbedOptions`]): a Type 0 font whose codes
//! are each glyph's number in the whole program, its program cut to the
//! glyphs drawn behind a `/CIDToGIDMap` stream. `crate::content::Fallback`
//! is the program sheaf bundles.
//!
//! [`Resolve`]: super::Resolve
//! [`Streams`]: super::Streams

pub use pdf_fonts::embed::{Glyphs, Options as EmbedOptions, Program, write as embed_write};
pub use pdf_fonts::{
    Descendant, Font, FontApprox, FontKind, FontMetrics, FontProgram, Fonts, GlyphBox, Limits,
    Std14, VerticalMetrics,
};

/// Why a font didn't read: asked by tests alone.
#[cfg(test)]
pub use pdf_fonts::Error;
pub use pdf_gfx::LayoutFont;

/// What a code of a standard encoding draws: asked by tests alone.
#[cfg(test)]
pub use pdf_fonts::Encoding;
