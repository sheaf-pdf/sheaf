//! The content toolkit (M10.5 §13), pulled forward from M11: page
//! content read as tokens, without `Content::decode`, which
//! re-encodes what it reads, loses comments and spacing, and reads inline
//! images by the first `EI` it meets.
//!
//! - [`tokens`] and [`operations`]: a content stream's tokens, each with
//!   where it is and its bytes as written, an inline image one token found
//!   by PDFium's rules, and its operations, each operator with its
//!   operands ([`Tok`], [`Operation`], [`Operand`]); an operand's [`name`]
//!   and [`object`]; and what stops them, [`SyntaxError`]. They're
//!   pdf-content-streams' (M18 slice 17; sheaf's own lexer until then),
//!   which reads content as PDFium does.
//! - [`page`]: a page's `/Contents` as one, the concatenation PDFium reads,
//!   read within [`ContentLimits`](crate::source::ContentLimits), and
//!   places in it mapped back to a stream ([`Concat`], [`Pos`] and
//!   [`SpanAcross`], which are pdf-content-streams'); and what of its
//!   resources pdf-content-streams reads it with, read from sheaf's
//!   sources: its color spaces ([`res_cx`], a [`ResCx`]) and property
//!   lists ([`properties`], [`Properties`]).
//! - [`balance`] and [`names_used`]: its `q`/`Q` balance and the resource
//!   names it uses; [`kept`], the two of what's left once ranges are taken
//!   out. pdf-content-streams', as are its marked ranges, which
//!   [`PageBlocks`] keeps of its own ([`Marked`]).
//! - [`splice`] and [`splice_each`]: its streams with ranges taken out,
//!   byte for byte, when that's safe, and refused when it isn't; and
//!   [`without_keys`], a marked range's property list written again
//!   without some keys. They're pdf-content-streams', which has the
//!   rules, called with the [`ResCx`] content was read with and
//!   [`ContentLimits::family`](crate::source::ContentLimits::family):
//!   the ranges are found by its tokens, and judged by them.
//! - [`PageBlocks`]: its pagination artifacts and who drew them, and
//!   sheaf's own blocks, sheaf's classification of the marked ranges
//!   pdf-content-streams finds.
//! - [`text_bag`]: the words it draws, as [`interp`] reads it, decoded
//!   through its fonts.
//! - [`DrawFont`]: what sheaf draws text in, the PDF layer's [`Std14`]
//!   and sheaf's [`Glyphless`], each a page resource ([`Res`]) named by
//!   its [`ResId`]; [`Font`], a font a file draws in, as read, and
//!   [`Fonts`], a source's.
//! - [`interp`]: what it draws, an operator at a time, in the graphics
//!   and text state it's drawn in: each glyph and where it is, each
//!   path, image and shading, the clip and the marked content each is
//!   in, and each form it draws, interpreted where it's drawn; content
//!   written again from what it draws ([`interp::encode`]). The
//!   interpreter is pdf-gfx's (M18 slice 23; sheaf's own from M11 slice
//!   3), read from sheaf's sources ([`Objects`]) under sheaf's caps, an
//!   object that doesn't read stopping it as its reason says; and
//!   [`fixture`], pages built to test it, for other crates' tests too.
//! - [`ContentWriter`]: content written, its resources ([`Res`]) named at
//!   the end ([`Reserved`], [`ResourceNeeds`]): pdf-content-streams', as
//!   are the types its calls take. Text goes in through [`DrawText`], in a
//!   [`DrawFont`], and a form by [`place_form`], in sheaf's geometry.
//! - [`sink`]: where content's resources are written: the emitter, or a
//!   PDF built in memory ([`serialize::built`](crate::serialize::built)).

mod blocks;
#[cfg(test)]
mod blocks_tests;
#[cfg(test)]
mod corpus;
#[cfg(any(test, feature = "fixtures"))]
pub mod fixture;
mod font;
pub mod interp;
#[cfg(test)]
mod interp_tests;
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
    Balance, Blend, Color, ContentOperation as Operation, ContentWriter, FillRule, GState, Kept,
    LineCap, LineJoin, NameSet, Need, Operand, Paint, Properties, Res, ResBody, ResCx, ResId,
    ResKind, Reserved, ResourceNeeds, Rewrite, SpliceError, Spliced, SyntaxError, SyntaxErrorKind,
    TextMode, Tok, balance, kept, name, names_used, object, operations, splice, splice_each,
    tokens, without_keys,
};
pub use blocks::{
    BlockAt, BlockId, ContentError, DrawnForms, GenId, Marked, Marker, PagOrigin, PageBlocks,
    Pagination, PaginationKind, Props, SheafBlock, SheafBlockRef, isolates,
};
pub use font::{
    Descendant, DrawFont, DrawText, Font, FontApprox, FontKind, FontMetrics, FontProgram, Fonts,
    Glyphless, Std14, Unencoded, VerticalMetrics, std14_of, wrap_with,
};
pub use page::{
    Concat, ContentFault, Objects, PageContent, Pos, ReadObjects, SpanAcross, Unkept, contents,
    properties, res_cx,
};
pub use place::place_form;
pub use text_bag::{TextBag, text_bag};
