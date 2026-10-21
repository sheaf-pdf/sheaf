//! OCR text: the words an engine recognized on a page. sheaf-doc builds a
//! layer from what an engine returns and writes it into the page as
//! invisible text.

use std::ops::Range;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::geom::Box2;
use crate::page::Rotation;

/// One recognized word.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OcrWord {
    pub text: String,
    /// PageSpace.
    pub rect: Box2,
    /// 0 to 1.
    pub confidence: f32,
}

/// The OCR result for one page.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TextLayer {
    /// Sorted by line (top to bottom in the upright frame), then left to right.
    pub words: Vec<OcrWord>,
    /// Index ranges into `words`, one per line.
    pub lines: Vec<Range<usize>>,
    /// The frame the text reads upright in: the page's rotation, absolute,
    /// when it was recognized.
    pub upright: Rotation,
    /// Which engine produced it, e.g. "tesseract 5.5.2".
    pub engine: String,
}

impl TextLayer {
    pub fn is_empty(&self) -> bool {
        self.words.is_empty()
    }

    /// Rough heap bytes, for the history memory policy.
    pub fn byte_estimate(&self) -> usize {
        self.words.iter().map(|w| 64 + w.text.len()).sum::<usize>() + 16 * self.lines.len()
    }
}
