//! Codes in a page image: barcodes and Kodak patch codes read, to find
//! separator sheets (`sheaf_ops::separate`), and a QR code's modules, for
//! sheaf's own sheet to draw. Pixels only, as the rest of the crate: which
//! page an image is and what a code means are the caller's. Patch codes
//! are sheaf's own reader's ([`patches`]).
//!
//! Barcodes are read through rxing behind [`Reader`], so a second engine
//! (zxing-cpp) can stand beside it later. A page is binarized once and
//! decoded for each symbology asked for alone, for the first code the
//! caller takes:
//! - with `TryHarder` on: without it rxing was twice as fast in the M5
//!   bench and missed every Data Matrix and small 1D code;
//! - with its formats limited to those asked for: searching the page for
//!   every code of every format was 5 to 6 times slower and read junk
//!   retail codes (UPC-E, EAN-8, ITF, Codabar) off about 15% of the
//!   corpus's real scans, where the limited formats read none;
//! - in a fixed order, QR first: sheaf's sheet comes before any other
//!   software's code on the same page, and a page reads the same way every
//!   time;
//! - going on past a code the caller turns down (a code counts only if its
//!   text matches, proposal 15): a vendor's QR code or a form's Code 39 id
//!   never hides a separator's code of another symbology on the same page.
//!   rxing gives one code a decode, so a second code of the same symbology
//!   is still hidden by the first.
//!
//! [`read`] reads a page as a separator is found: its patch code, then
//! its barcode.

use std::collections::HashSet;

use anyhow::{Context, Result, anyhow};
use image::GrayImage;
use rxing::Reader as _;
use rxing::common::HybridBinarizer;
use rxing::qrcode::common::ErrorCorrectionLevel;
use rxing::qrcode::encoder::qrcode_encoder;
use rxing::{BarcodeFormat, BinaryBitmap, DecodeHints, EncodeHints, Luma8Source};
use rxing::{MultiFormatReader, RXingResult};

#[cfg(any(test, feature = "fixtures"))]
pub mod fixture;
mod patch;
#[cfg(test)]
mod tests;

pub use patch::{Edge, Patch, PatchCode, patch, patches};

/// The barcodes sheaf reads: its own sheet's (QR), and those other
/// software prints on separator sheets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Symbology {
    Qr,
    DataMatrix,
    Pdf417,
    Code39,
    Code128,
}

impl Symbology {
    /// Every one, in the order a page is decoded for them: sheaf's QR
    /// code, the other 2D codes, then the 1D ones in rxing's own order.
    pub const ALL: [Symbology; 5] = [
        Symbology::Qr,
        Symbology::DataMatrix,
        Symbology::Pdf417,
        Symbology::Code39,
        Symbology::Code128,
    ];

    fn format(self) -> BarcodeFormat {
        match self {
            Symbology::Qr => BarcodeFormat::QR_CODE,
            Symbology::DataMatrix => BarcodeFormat::DATA_MATRIX,
            Symbology::Code39 => BarcodeFormat::CODE_39,
            Symbology::Code128 => BarcodeFormat::CODE_128,
            Symbology::Pdf417 => BarcodeFormat::PDF_417,
        }
    }
}

/// A barcode read in a page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Code {
    pub symbology: Symbology,
    pub text: String,
}

/// Reads barcodes. Object-safe, so a second engine can come in beside
/// [`Rxing`] (ask 7).
pub trait Reader: Send + Sync {
    /// The first code of `symbologies` in `page` that `accept` takes, the
    /// page binarized once and decoded for each symbology alone in a fixed
    /// order ([`Symbology::ALL`]'s), a code turned down letting the next
    /// symbology's decode run. None asked, nothing read.
    fn read(
        &self,
        page: &GrayImage,
        symbologies: &[Symbology],
        accept: &dyn Fn(&Code) -> bool,
    ) -> Option<Code>;
}

/// rxing, a pure-Rust port of ZXing.
pub struct Rxing;

impl Reader for Rxing {
    fn read(
        &self,
        page: &GrayImage,
        symbologies: &[Symbology],
        accept: &dyn Fn(&Code) -> bool,
    ) -> Option<Code> {
        if symbologies.is_empty() {
            return None;
        }
        let source =
            Luma8Source::new_with_slice(page.as_raw(), page.width(), page.height()).ok()?;
        let mut bitmap = BinaryBitmap::new(HybridBinarizer::new(source));
        // rxing walks its formats in a HashSet's order, which is random for
        // each reader, so a page with two codes would read as either one;
        // and it stops at the first code it finds, which the caller may
        // turn down. Each symbology is decoded alone instead, so the calls
        // share one binarization (the bitmap caches it).
        let passes = Symbology::ALL
            .into_iter()
            .filter(|s| symbologies.contains(s));
        for symbology in passes {
            let hints = DecodeHints {
                TryHarder: Some(true),
                PossibleFormats: Some(HashSet::from([symbology.format()])),
                ..Default::default()
            };
            if let Ok(found) = MultiFormatReader::default().decode_with_hints(&mut bitmap, &hints)
                && let Some(code) = of(&found, symbology)
                && accept(&code)
            {
                return Some(code);
            }
        }
        None
    }
}

/// What a page carries that could make it a separator sheet: its patch
/// code and the first barcode the caller took.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageCodes {
    pub patch: Option<PatchCode>,
    pub code: Option<Code>,
}

/// The codes on `page`, `dpi` its resolution, read as separators are
/// (proposal 15): the patch bars along its four edges first (about 5 ms),
/// then `reader`'s first code of `symbologies` that `accept` takes (a
/// code counts only if its text matches). Both always run: a sheaf sheet
/// may carry patch T bars beside its QR code, and which one a separator
/// counts by is the caller's. So is the resolution: 150 dpi in gray reads
/// sheaf's QR modules (4.4 pixels) and patch bars, and a rule that reads
/// 1D codes wants 200 (a 20 mil bar is 4 pixels).
pub fn read(
    page: &GrayImage,
    dpi: f32,
    symbologies: &[Symbology],
    accept: &dyn Fn(&Code) -> bool,
    reader: &dyn Reader,
) -> PageCodes {
    PageCodes {
        patch: patch(page, dpi),
        code: reader.read(page, symbologies, accept),
    }
}

/// `found` as a [`Code`] if it is of `symbology`.
fn of(found: &RXingResult, symbology: Symbology) -> Option<Code> {
    (*found.getBarcodeFormat() == symbology.format()).then(|| Code {
        symbology,
        text: found.getText().to_string(),
    })
}

/// A code's modules, row by row, with no quiet zone: whoever draws it adds
/// its own white.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Modules {
    width: u32,
    height: u32,
    dark: Vec<bool>,
}

impl Modules {
    /// How many modules across.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// How many modules down.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Whether the module in column `x` of row `y` is dark.
    pub fn dark(&self, x: u32, y: u32) -> bool {
        assert!(
            x < self.width && y < self.height,
            "module ({x}, {y}) of {} × {}",
            self.width,
            self.height
        );
        self.dark[y as usize * self.width as usize + x as usize]
    }
}

/// The modules of a QR code of `text`, for sheaf's sheet (proposal 13):
/// error correction level M, the text in UTF-8 (with its ECI, so every
/// reader takes it as UTF-8). The same text makes the same modules, so a
/// sheet made again is the same sheet.
pub fn qr(text: &str) -> Result<Modules> {
    let hints = EncodeHints {
        CharacterSet: Some("UTF-8".into()),
        ..Default::default()
    };
    let code = qrcode_encoder::encode_with_hints(text, ErrorCorrectionLevel::M, &hints)
        .map_err(|e| anyhow!("a QR code of {} bytes: {e}", text.len()))?;
    let matrix = code
        .getMatrix()
        .as_ref()
        .context("a QR code with no modules")?;
    let (width, height) = (matrix.getWidth(), matrix.getHeight());
    let dark = (0..height)
        .flat_map(|y| (0..width).map(move |x| matrix.get(x, y) == 1))
        .collect();
    Ok(Modules {
        width,
        height,
        dark,
    })
}
