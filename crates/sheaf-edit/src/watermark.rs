//! Watermarks and backgrounds, as an op sets them: text, an image or a
//! PDF's page, drawn on each page in scope, over its content or under it.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::mark::{Color, FontChoice};
use crate::refs::BlobSel;

/// Where a watermark is drawn: over the page's content (a watermark) or
/// under it (a background).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
pub enum Z {
    Under,
    Over,
}

/// A cell of a 3×3 grid over the page as it shows (its visible box,
/// turned as it's turned): where a watermark's middle goes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum GridPos {
    TopLeft,
    Top,
    TopRight,
    Left,
    #[default]
    Center,
    Right,
    BottomLeft,
    Bottom,
    BottomRight,
}

/// A watermark: what it draws, and where on each page.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WatermarkSpec {
    pub content: WatermarkContent,
    /// The middle of the page when none is said.
    #[serde(default)]
    pub position: GridPos,
}

/// What a watermark draws.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "kebab-case", deny_unknown_fields)]
pub enum WatermarkContent {
    /// `text`, `size` points tall.
    Text {
        text: String,
        #[serde(default)]
        font: FontChoice,
        size: f32,
        color: Color,
        /// 0–1.
        opacity: f32,
        /// Degrees counterclockwise, on the page as it shows.
        #[serde(default)]
        angle: f32,
    },
    /// An image file (PNG, JPEG, TIFF, BMP, GIF or WebP), `scale` of the
    /// page's width wide (above 0, up to 2), its height in proportion.
    /// With no `image`, the image the watermark at its z draws now, as the
    /// document has it: a file saved with one draws it again unchanged.
    Image {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[schemars(with = "BlobSel")]
        image: Option<BlobSel>,
        scale: f32,
        /// 0–1.
        opacity: f32,
    },
    /// Page `page` (1-based, 1 when none is said) of a PDF, `scale` of the
    /// page's width wide (above 0, up to 2), its height in proportion.
    /// With no `pdf`, the page the watermark at its z draws now, as the
    /// document has it, and `page` isn't read.
    Page {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[schemars(with = "BlobSel")]
        pdf: Option<BlobSel>,
        #[serde(default = "first_page")]
        page: u32,
        scale: f32,
        /// 0–1.
        opacity: f32,
    },
}

/// A PDF's page a watermark draws when none is said.
fn first_page() -> u32 {
    1
}
