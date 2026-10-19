//! OCR text: the words an engine recognized on a page. sheaf-doc builds a
//! layer from what an engine returns, searches it, and writes it into the
//! page as invisible text.

use std::ops::Range;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::geom::{Box2, Pt};
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

    /// The recognized text: words joined by spaces, lines by newlines.
    pub fn text(&self) -> String {
        let line = |words: &Range<usize>| {
            let words = self.words[words.clone()].iter().map(|w| w.text.as_str());
            words.collect::<Vec<_>>().join(" ")
        };
        self.lines.iter().map(line).collect::<Vec<_>>().join("\n")
    }

    /// Rough heap bytes, for the history memory policy.
    pub fn byte_estimate(&self) -> usize {
        self.words.iter().map(|w| 64 + w.text.len()).sum::<usize>() + 16 * self.lines.len()
    }

    /// Boxes of the words whose centre lies inside `rect`, merged per line
    /// (the shape the highlight tool wants). PageSpace.
    pub fn boxes_in_rect(&self, rect: Box2) -> Vec<Box2> {
        let mut out = Vec::new();
        for line in &self.lines {
            let mut merged: Option<Box2> = None;
            for w in &self.words[line.clone()] {
                let c = Pt {
                    x: w.rect.x + w.rect.w / 2.0,
                    y: w.rect.y + w.rect.h / 2.0,
                };
                if rect.contains(c) {
                    merged = Some(merged.map_or(w.rect, |m| m.union(w.rect)));
                }
            }
            out.extend(merged);
        }
        out
    }
}
