//! Image in, image out: sheaf's pixel work, with no PDFs, OCR or windows.
//!
//! The boundary is the point of the crate. It may depend on image codecs,
//! `imageproc` and a barcode library (rxing, which reads and writes
//! pixels only), never on `lopdf`, pdfium, an OCR engine or model, or
//! gpui. Code that needs a document page, a model or a window belongs in
//! `sheaf-ops`, `sheaf-ocr` or the app, which call in here.
//!
//! - [`clean`]: scans and phone photos of paper made into clean pages.
//! - [`blank`]: whether a page image is blank.
//! - [`codes`]: barcodes and Kodak patch codes read in a page image, and a
//!   QR code's modules to draw.
//! - [`raster`]: rendered pages as pixels, and their conversions.
//! - [`io`]: image files in, as [`io::PageImage`]s or upright RGB (a
//!   TIFF's pages one at a time, [`io::frames`]).
//! - [`encode`]: images out: PNG, JPEG and TIFF with their resolution, and
//!   CCITT G4.

pub mod blank;
pub mod clean;
pub mod codes;
pub mod encode;
pub mod io;
pub mod raster;
