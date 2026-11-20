//! The content toolkit (M10.5 §13), pulled forward from M11: page
//! content read as tokens, without `Content::decode`, which
//! re-encodes what it reads, loses comments and spacing, and reads inline
//! images by the first `EI` it meets.
//!
//! - [`lex`]: a content stream's tokens, each with where it is and its
//!   bytes as written, an inline image one token found by PDFium's rules.
//! - [`page`]: a page's `/Contents` as one, the concatenation PDFium lexes,
//!   read within [`ContentLimits`](crate::source::ContentLimits), and
//!   places in it mapped back to a stream ([`Concat`], [`Pos`] and
//!   [`SpanAcross`], which are pdf-content-streams').
//! - [`marked`], [`balance`] and [`names_used`]: its marked ranges, its
//!   `q`/`Q` balance and the resource names it uses; [`kept`], the last
//!   two of what's left once ranges are taken out.
//! - [`splice`] and [`splice_each`]: its streams with ranges taken out,
//!   byte for byte, when that's safe, and refused when it isn't; and
//!   [`without_keys`], a marked range's property list written again
//!   without some keys. They're pdf-content-streams', which has the
//!   rules, called with [`ResCx::family`] and
//!   [`ContentLimits::family`](crate::source::ContentLimits::family):
//!   the ranges are found by sheaf's lexer, and judged by its tokens.
//! - [`PageBlocks`]: its pagination artifacts and who drew them, and
//!   sheaf's own blocks.
//! - [`text_bag`]: the words it draws, as [`interp`] reads it, decoded
//!   through its fonts.
//! - [`Font`]: what sheaf draws text in, [`Std14`] and [`Glyphless`], and
//!   [`FontRes`], a font as a page resource named by its [`ResId`];
//!   [`Foreign`], a font a file draws in, as read, and [`Fonts`], a
//!   source's.
//! - [`interp`]: what it draws, an operator at a time, in the graphics
//!   and text state it's drawn in: each glyph and where it is, each
//!   path, image and shading, the clip and the marked content each is
//!   in, and each form it draws, interpreted where it's drawn (M11 slice
//!   3); content written again from what it draws ([`interp::encode`]);
//!   and [`fixture`], pages built to test it, for other crates' tests
//!   too.
//! - [`ContentWriter`]: content written, its resources ([`Res`]) named at
//!   the end ([`Reserved`], [`ResourceNeeds`]): pdf-content-streams', as
//!   are the types its calls take. Text goes in through [`DrawText`], in a
//!   [`Font`], and a form by [`place_form`], in sheaf's geometry.
//! - [`sink`]: where content's resources are written: the emitter, or a
//!   PDF built in memory ([`serialize::built`](crate::serialize::built)).

mod balance;
mod blocks;
#[cfg(test)]
mod blocks_tests;
#[cfg(test)]
mod corpus;
#[cfg(any(test, feature = "fixtures"))]
pub mod fixture;
mod font;
pub mod interp;
mod lex;
mod marked;
mod names;
mod page;
mod place;
pub mod sink;
#[cfg(test)]
mod tests;
mod text_bag;
#[cfg(test)]
mod text_bag_tests;
#[cfg(test)]
mod written_golden;

pub use crate::pdf::content::{
    Blend, Color, ContentWriter, FillRule, GState, LineCap, LineJoin, Need, Paint, Res, ResBody,
    ResId, ResKind, Reserved, ResourceNeeds, Rewrite, SpliceError, Spliced, TextMode, splice,
    splice_each, without_keys,
};
pub use balance::{Balance, balance};
pub use blocks::{
    BlockAt, BlockId, ContentError, DrawnForms, GenId, Marker, PagOrigin, PageBlocks, Pagination,
    PaginationKind, SheafBlock, SheafBlockRef, isolates,
};
pub use font::{
    Descendant, DrawText, Font, FontApprox, FontId, FontKind, FontMetrics, FontProgram, FontRes,
    FontScope, Fonts, Foreign, Glyphless, Std14, TooLarge, Unencoded, VerticalMetrics, wrap_with,
};
pub use lex::{LexError, LexErrorKind, ResCx, Tok, lex, name, object};
pub use marked::{Marked, PropertiesDict, Props, marked};
pub use names::{Kept, NameSet, kept, names_used};
pub use page::{Concat, ContentFault, PageContent, Pos, ReadObjects, SpanAcross, contents};
pub use place::place_form;
pub use text_bag::{TextBag, text_bag};
