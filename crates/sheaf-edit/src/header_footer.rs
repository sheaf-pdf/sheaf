//! The header and footer, and Bates numbers, as an op sets them.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::mark::Color;

/// A header and footer: six slots of text, drawn on every page in scope.
/// What `<<date>>` and `<<file>>` say is fixed when it's set, and Bates
/// numbers are counted then (M10.5 §3.5).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HeaderFooterSpec {
    /// Header left, center, right; footer left, center, right. Each can
    /// use `<<1>>` (the page number), `<<N>>` (the page count),
    /// `<<date>>`, `<<file>>` and `<<bates>>`.
    pub texts: [String; 6],
    /// The text's size, in points.
    pub size: f32,
    pub color: Color,
    pub margins: Margins,
    /// What `<<1>>` is on the document's first page.
    pub first_number: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bates: Option<BatesSpec>,
}

/// Distances from the page's edges to the text, in points: from the top
/// to the header, from the bottom to the footer, and from the sides.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Margins {
    pub top: f32,
    pub bottom: f32,
    pub left: f32,
    pub right: f32,
}

/// Bates numbering: `<prefix><number, zero-padded to digits><suffix>`,
/// counting pages in scope in document order from `start`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BatesSpec {
    pub prefix: String,
    pub suffix: String,
    pub digits: u8,
    pub start: u64,
}

impl Default for BatesSpec {
    fn default() -> Self {
        Self {
            prefix: String::new(),
            suffix: String::new(),
            digits: 6,
            start: 1,
        }
    }
}
