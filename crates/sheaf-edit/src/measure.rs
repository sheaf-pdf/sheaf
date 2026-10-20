//! What a measurement measures, and the scale it's read at ("1 in = 10
//! ft"), as Acrobat's Measure tools make them. sheaf-doc works out the
//! values and writes them as dimension annotations.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// What a measurement measures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum Measured {
    /// Between two points.
    Distance,
    /// Along an open path.
    Perimeter,
    /// Inside a closed shape.
    Area,
}

impl Measured {
    pub fn label(self) -> &'static str {
        match self {
            Measured::Distance => "Distance",
            Measured::Perimeter => "Perimeter",
            Measured::Area => "Area",
        }
    }

    /// `/IT` as the PDF has it.
    pub fn intent(self) -> &'static [u8] {
        match self {
            Measured::Distance => b"LineDimension",
            Measured::Perimeter => b"PolyLineDimension",
            Measured::Area => b"PolygonDimension",
        }
    }
}

/// Points in one of a unit.
pub fn points_per(unit: &str) -> Option<f64> {
    Some(match unit {
        "pt" => 1.0,
        "in" => 72.0,
        "ft" => 72.0 * 12.0,
        "yd" => 72.0 * 36.0,
        "mi" => 72.0 * 63_360.0,
        "mm" => 72.0 / 25.4,
        "cm" => 72.0 / 2.54,
        "m" => 72.0 / 0.0254,
        "km" => 72.0 / 0.0000254,
        _ => return None,
    })
}

/// A scale: `page` `page_unit` on the page stands for `real` `unit`, and
/// values show `precision` decimals.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Scale {
    pub page: f64,
    pub page_unit: String,
    pub real: f64,
    pub unit: String,
    pub precision: u8,
}

impl Default for Scale {
    /// Actual size, in inches.
    fn default() -> Self {
        Scale {
            page: 1.0,
            page_unit: "in".into(),
            real: 1.0,
            unit: "in".into(),
            precision: 2,
        }
    }
}

impl Scale {
    /// Real units for each point on the page.
    pub fn per_point(&self) -> f64 {
        let page_pt = points_per(&self.page_unit).unwrap_or(72.0) * self.page;
        if page_pt <= 0.0 {
            return 0.0;
        }
        self.real / page_pt
    }

    /// "1 in = 10 ft".
    pub fn ratio(&self) -> String {
        format!(
            "{} {} = {} {}",
            trim(self.page),
            self.page_unit,
            trim(self.real),
            self.unit
        )
    }

    /// Calibrated: a line `points` long on the page is `real` `unit`. Kept
    /// as so much per inch of page.
    pub fn calibrated(points: f64, real: f64, unit: &str, precision: u8) -> Scale {
        let per_point = if points > 0.0 { real / points } else { 1.0 };
        Scale {
            page: 1.0,
            page_unit: "in".into(),
            real: per_point * 72.0,
            unit: unit.into(),
            precision,
        }
    }

    /// `v` with the scale's decimals and `unit`: "12.50 ft".
    pub fn show(&self, v: f64, unit: &str) -> String {
        let p = self.precision.min(6) as usize;
        format!("{v:.p$} {unit}")
    }
}

/// A number without trailing zeros: 10, 0.25.
fn trim(v: f64) -> String {
    let s = format!("{v:.6}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() { "0".into() } else { s.into() }
}
