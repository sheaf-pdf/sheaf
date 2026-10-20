//! Page labels as an op sets them: the "iv" or "A-3" a page shows instead
//! of its position.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::refs::PageRef;

/// How a range numbers its pages (`/S`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum LabelStyle {
    Decimal,
    UpperRoman,
    LowerRoman,
    UpperLetters,
    LowerLetters,
    /// No number, only the prefix.
    None,
}

/// A label range: pages from `start` on, until the next range, numbered
/// in `style` from `first`, after `prefix`. It stays with the page it
/// starts at when pages move.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PageLabelRange {
    pub start: PageRef,
    pub style: LabelStyle,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub prefix: String,
    /// The number of the range's first page (`/St`).
    #[serde(default = "one")]
    pub first: u32,
}

fn one() -> u32 {
    1
}
