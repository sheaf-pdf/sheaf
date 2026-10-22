//! A page's size, rotation and boxes, where a bookmark lands on it, and
//! how a bookmark's title is drawn.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::geom::Box2;

/// A size in PDF points (1/72 inch).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PageSize {
    pub width: f32,
    pub height: f32,
}

impl PageSize {
    /// The size after rotating the page by `r` (quarter turns swap axes).
    pub fn rotated(self, r: Rotation) -> Self {
        if r.is_quarter() {
            Self {
                width: self.height,
                height: self.width,
            }
        } else {
            self
        }
    }
}

/// A clockwise quarter-turn rotation, as in the `/Rotate` entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, JsonSchema)]
pub enum Rotation {
    #[default]
    R0,
    R90,
    R180,
    R270,
}

impl Rotation {
    pub fn from_degrees(deg: i32) -> Self {
        match deg.rem_euclid(360) {
            90 => Self::R90,
            180 => Self::R180,
            270 => Self::R270,
            _ => Self::R0,
        }
    }

    pub fn degrees(self) -> i32 {
        match self {
            Self::R0 => 0,
            Self::R90 => 90,
            Self::R180 => 180,
            Self::R270 => 270,
        }
    }

    /// Rotate a further 90° clockwise (the PDFlyer "rotate this page" action).
    pub fn cw(self) -> Self {
        Self::from_degrees(self.degrees() + 90)
    }

    pub fn ccw(self) -> Self {
        Self::from_degrees(self.degrees() - 90)
    }

    pub fn plus(self, other: Rotation) -> Self {
        Self::from_degrees(self.degrees() + other.degrees())
    }

    pub fn minus(self, other: Rotation) -> Self {
        Self::from_degrees(self.degrees() - other.degrees())
    }

    /// 90° or 270°: width and height swap.
    pub fn is_quarter(self) -> bool {
        matches!(self, Self::R90 | Self::R270)
    }
}

/// Distances in points from each side of a page as it shows, turned by its
/// `/Rotate`: what a crop takes off, what a header and footer keep clear.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Margins {
    pub top: f32,
    pub bottom: f32,
    pub left: f32,
    pub right: f32,
}

/// One of a page's boxes, as an op sets it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub enum BoxSpec {
    /// Points off each side of the box the page shows, as it shows after
    /// its `/Rotate`.
    Margins(Margins),
    /// A box in PageSpace, within the page's media box.
    Rect(Box2),
}

/// Where a bookmark lands on its page.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub enum DestView {
    /// PDF `/XYZ`: show this point at the window's top-left, at this zoom
    /// (1.0 = 100%). Coordinates are in PageSpace, the page's default user
    /// space before `/Rotate`, as the file has them. `None` keeps the
    /// viewer's current value.
    Xyz {
        left: Option<f32>,
        top: Option<f32>,
        zoom: Option<f32>,
    },
    /// Any other kind (`Fit`, `FitH`, `FitR`, …), kept as the file had it:
    /// its name and its numbers in user space (`None` for null), written
    /// back unchanged.
    Other {
        kind: String,
        params: Vec<Option<f32>>,
    },
}

/// How a bookmark's title is drawn (its `/F`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TextStyle {
    #[serde(default)]
    pub bold: bool,
    #[serde(default)]
    pub italic: bool,
}

impl TextStyle {
    pub fn is_plain(&self) -> bool {
        !self.bold && !self.italic
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotation_wraps() {
        assert_eq!(Rotation::R270.cw(), Rotation::R0);
        assert_eq!(Rotation::R0.ccw(), Rotation::R270);
        assert_eq!(Rotation::from_degrees(-90), Rotation::R270);
        assert_eq!(Rotation::R90.plus(Rotation::R270), Rotation::R0);
        assert_eq!(Rotation::R0.minus(Rotation::R90), Rotation::R270);
    }

    #[test]
    fn size_rotates() {
        let s = PageSize {
            width: 612.0,
            height: 792.0,
        };
        assert_eq!(s.rotated(Rotation::R90).width, 792.0);
        assert_eq!(s.rotated(Rotation::R180).width, 612.0);
    }
}
