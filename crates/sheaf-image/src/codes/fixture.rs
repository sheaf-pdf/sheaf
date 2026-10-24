//! Codes drawn on synthetic pages, and the pages scanned badly, for this
//! crate's tests and other crates' (the `fixtures` feature). Everything is
//! seeded, so a page and its degradations are the same on every run.
//!
//! The pages are the M5 bench's, 300 dpi Letter: text, rules and a table,
//! the text drawn as glyph-like strokes (stems, bars, bowls and diagonals
//! at an 11 pt x-height), since this crate has no font. Real glyphs come
//! from PDFium in sheaf-ops' tests. Patch codes are drawn as Kodak's label
//! and sheets and Alliance's sheet lay them out, from their measurements
//! (no PDFs are kept).

use image::imageops::{self, FilterType};
use image::{GrayImage, ImageFormat, Luma};
use imageproc::geometric_transformations::{Border, Interpolation, rotate_about_center};
use rxing::{EncodeHints, MultiFormatWriter, Writer};

use super::{Edge, Modules, PatchCode, Symbology, qr};
use Degradation::*;

/// A Letter page at 300 dpi, in pixels.
pub const LETTER_300: (u32, u32) = (2550, 3300);

/// The gray a code's dark modules and a page's strokes are drawn in:
/// printer black.
const INK: Luma<u8> = Luma([15]);

/// The gray patch codes' bars are drawn in: under A-61599's 20%
/// reflectance.
const BAR: Luma<u8> = Luma([10]);

/// Where [`code_page`] puts a code's top left corner.
const CODE_AT: (u32, u32) = (450, 700);

/// A sheet's code text, as proposal 13 spells it.
pub const SHEET: &str = "SHEAFSEP1|a=split|l=1|p=drop|t=W-2 Acme Corp";

/// Each symbology as the codes' tests draw it at 300 dpi ([`code_page`]):
/// its text, its modules' width and its rows' height in pixels. sheaf's
/// sheet's QR modules (0.75 mm), the bench's Data Matrix (0.68 mm) and
/// PDF417 (4 pixels, its rows 4 modules tall), and 1D codes of 20 mil
/// bars half an inch tall, which the bench found the least that reads in
/// 1-bit at 200 dpi.
pub const SAMPLES: [(Symbology, &str, u32, u32); 5] = [
    (Symbology::Qr, SHEET, 9, 9),
    (Symbology::DataMatrix, "DOC=W-2;CLIENT=1042;N=0001", 8, 8),
    (Symbology::Code39, "SEP-W2 ACME", 6, 150),
    // SurePrep's style: the form's name.
    (Symbology::Code128, "Schedule A", 6, 150),
    (
        Symbology::Pdf417,
        "Form W-2 2025|Copy B|Acme Corp|12-3456789",
        4,
        4,
    ),
];

/// A white page of `w` × `h` pixels.
pub fn white_page(w: u32, h: u32) -> GrayImage {
    GrayImage::from_pixel(w, h, Luma([255]))
}

/// A white Letter page at 300 dpi.
pub fn blank_page() -> GrayImage {
    white_page(LETTER_300.0, LETTER_300.1)
}

/// `text` as `symbology`'s modules: a QR code as sheaf's sheet makes it
/// ([`qr`]); the others as rxing's writer draws them, one module a pixel,
/// cropped to their dark modules. A 1D code is one row; a PDF417 code
/// stays unturned, its rows each four modules tall as the writer makes
/// them.
pub fn encode(symbology: Symbology, text: &str) -> Modules {
    if symbology == Symbology::Qr {
        return qr(text).expect("a QR code");
    }
    let hints = EncodeHints {
        Margin: Some("0".into()),
        ..Default::default()
    };
    // A size of 0 × 0 draws one pixel a module, and keeps a PDF417 code
    // wider than tall unturned.
    let matrix = MultiFormatWriter
        .encode_with_hints(text, &symbology.format(), 0, 0, &hints)
        .expect("a code rxing can write");
    let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0, 0);
    for y in 0..matrix.getHeight() {
        for x in 0..matrix.getWidth() {
            if matrix.get(x, y) {
                (x0, y0) = (x0.min(x), y0.min(y));
                (x1, y1) = (x1.max(x), y1.max(y));
            }
        }
    }
    assert!(x0 <= x1, "a code with no dark modules");
    let dark = (y0..=y1)
        .flat_map(|y| (x0..=x1).map(move |x| (x, y)))
        .map(|(x, y)| matrix.get(x, y))
        .collect();
    Modules {
        width: x1 - x0 + 1,
        height: y1 - y0 + 1,
        dark,
    }
}

/// Draw `modules`' dark ones in `img` with their top left at (`x`, `y`),
/// each `module_px_x` × `module_px_y` pixels, clipped to the image.
pub fn paste(
    img: &mut GrayImage,
    modules: &Modules,
    x: u32,
    y: u32,
    module_px_x: u32,
    module_px_y: u32,
) {
    for row in 0..modules.height() {
        for col in 0..modules.width() {
            if !modules.dark(col, row) {
                continue;
            }
            let (left, top) = (x + col * module_px_x, y + row * module_px_y);
            for py in top..(top + module_px_y).min(img.height()) {
                for px in left..(left + module_px_x).min(img.width()) {
                    img.put_pixel(px, py, INK);
                }
            }
        }
    }
}

/// A text page ([`text_page`], seed 42) with `text` drawn as `symbology`
/// at (450, 700), each module `module_px` wide and each row `row_px` tall
/// (a 1D code's one row is its bars' height), and its quiet zone left
/// white: 4 modules round a 2D code and 10 round a 1D one, at least 40
/// and 60 pixels, as the bench's.
pub fn code_page(symbology: Symbology, text: &str, module_px: u32, row_px: u32) -> GrayImage {
    let modules = encode(symbology, text);
    let quiet = if matches!(symbology, Symbology::Code39 | Symbology::Code128) {
        (10 * module_px).max(60)
    } else {
        (4 * module_px).max(40)
    };
    let (x, y) = CODE_AT;
    let clear = (
        x - quiet,
        y - quiet,
        modules.width() * module_px + 2 * quiet,
        modules.height() * row_px + 2 * quiet,
    );
    let mut img = text_page(42, Some(clear));
    paste(&mut img, &modules, x, y, module_px, row_px);
    img
}

/// A busy 300 dpi Letter page: a header with a rule under it, lines of
/// words, a ruled table of numbers and a footer. `seed` varies the words
/// and numbers; nothing is drawn in `clear` (x, y, w, h), where a code
/// and its quiet zone go.
pub fn text_page(seed: u64, clear: Option<(u32, u32, u32, u32)>) -> GrayImage {
    let mut pen = Pen {
        img: blank_page(),
        clear,
    };
    let mut rng = Rng::new(seed);
    // The header, at about twice the body's size.
    let mut x = 300;
    for _ in 0..5 {
        x += pen.word(&mut rng, x, 290, 40) + 24;
    }
    pen.rule(300, 320, 1950, 6);
    // Body lines: 11 pt at 300 dpi, an x-height of 22 pixels, a leading
    // of 62.
    for top in (380..2250).step_by(62) {
        let mut x = 300;
        while x < 2200 {
            let w = pen.word(&mut rng, x, top + 34, 22);
            if w == 0 {
                break;
            }
            x += w + 22;
        }
    }
    // A 5 × 9 table: rules and numbers.
    let (tx, ty, cols, rows, cw, rh) = (300, 2350, 5, 9, 390, 70);
    for r in 0..=rows {
        pen.rule(tx, ty + r * rh, cols * cw, 3);
    }
    for c in 0..=cols {
        pen.rule(tx + c * cw, ty, 3, rows * rh);
    }
    for r in 0..rows {
        for c in 0..cols {
            let v = rng.below(9_999_999);
            let number = format!("{}.{:02}", v / 100, v % 100);
            pen.number(&number, tx + c * cw + 20, ty + r * rh + 50, 30);
        }
    }
    // The footer.
    let mut x = 300;
    for _ in 0..12 {
        x += pen.word(&mut rng, x, 3080, 18) + 18;
    }
    pen.img
}

/// [`text_page`] (seed 7) as a feeder scans it a little askew: a black
/// border along all four edges, 0.1 in wide at one end of each and 0.3 in
/// at the other.
pub fn bordered_page() -> GrayImage {
    let mut img = text_page(7, None);
    let (w, h) = (img.width() as i32, img.height() as i32);
    for y in 0..h {
        let (left, right) = (30 + 60 * y / h, 90 - 60 * y / h);
        fill(&mut img, (0, y, left, 1), INK);
        fill(&mut img, (w - right, y, right, 1), INK);
    }
    for x in 0..w {
        let (top, bottom) = (90 - 60 * x / w, 30 + 60 * x / w);
        fill(&mut img, (x, 0, 1, top), INK);
        fill(&mut img, (x, h - bottom, 1, bottom), INK);
    }
    img
}

/// [`text_page`] (seed 8) under a letterhead's rule: across the page,
/// 0.3 in from the top, 0.05 in thick.
pub fn edge_rule_page() -> GrayImage {
    let mut img = text_page(8, None);
    let w = img.width() as i32;
    fill(&mut img, (0, 90, w, 15), INK);
    img
}

/// A 300 dpi Letter page of rules alone, as a form's: a border, a header
/// rule, two columns of boxes, a table and lines to write on.
pub fn rules_page() -> GrayImage {
    let mut pen = Pen {
        img: blank_page(),
        clear: None,
    };
    for (x, y, w, h) in [
        (150, 150, 2250, 4),
        (150, 3146, 2250, 4),
        (150, 150, 4, 3000),
        (2396, 150, 4, 3000),
    ] {
        pen.rule(x, y, w, h);
    }
    pen.rule(150, 400, 2250, 8);
    // Boxes, as a W-2's.
    for r in 0..=10 {
        pen.rule(150, 450 + r * 95, 2250, 3);
    }
    for x in [1275, 1840] {
        pen.rule(x, 450, 3, 950);
    }
    // A table of uneven columns.
    let columns = [300, 620, 980, 1200, 1560, 1900, 2250];
    for r in 0..=12 {
        pen.rule(300, 1500 + r * 80, 1950, 2);
    }
    for x in columns {
        pen.rule(x, 1500, 2, 960);
    }
    // Lines to write on.
    for r in 0..6 {
        pen.rule(300, 2650 + r * 90, 1200, 3);
        pen.rule(1700, 2650 + r * 90, 550, 3);
    }
    pen.img
}

/// Where a patch code's bars go, as printed labels and sheets put them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatchLayout {
    /// Kodak A-61599's label: along the top edge alone, the first bar
    /// 0.5 in in, 2.5 in long from 3 in along, on a page of text whose top
    /// 1.5 in is clear.
    KodakLabel,
    /// Kodak's "Patches for Printing" sheets: along all four edges, the
    /// first bar 0.62 in in, from 0.5 to 10.5 in down the sides and from
    /// 1.5 to 7 in along the top and bottom; a title, text and the type as
    /// Code 39 in the middle.
    KodakSheet,
    /// Alliance's patch T sheet (patch_t_us-letter.pdf, measured at 100
    /// dpi): along all four edges, the first bar 0.63 in in, from 0.63 to
    /// 9.5 in down the sides and from 1.55 to 6.96 in along the top and
    /// bottom; the middle as Kodak's. A T only.
    AllianceT,
}

impl PatchLayout {
    /// Where the bars go: the edge, the first bar's inset, and from and to
    /// how far along the edge (from the left or the top), in inches.
    fn places(self) -> &'static [(Edge, f32, f32, f32)] {
        match self {
            PatchLayout::KodakLabel => &[(Edge::Top, 0.5, 3.0, 5.5)],
            PatchLayout::KodakSheet => &[
                (Edge::Top, 0.62, 1.5, 7.0),
                (Edge::Bottom, 0.62, 1.5, 7.0),
                (Edge::Left, 0.62, 0.5, 10.5),
                (Edge::Right, 0.62, 0.5, 10.5),
            ],
            PatchLayout::AllianceT => &[
                (Edge::Top, 0.63, 1.55, 6.96),
                (Edge::Bottom, 0.63, 1.55, 6.96),
                (Edge::Left, 0.63, 0.63, 9.5),
                (Edge::Right, 0.63, 0.63, 9.5),
            ],
        }
    }
}

/// `code`'s bars laid out as `layout` on a 300 dpi Letter page: for
/// Kodak's label, [`text_page`] (seed 99) with its top 1.5 in clear; for
/// the sheets, a sheet's title, text and Code 39.
pub fn patch_page(code: PatchCode, layout: PatchLayout) -> GrayImage {
    let page = match layout {
        PatchLayout::KodakLabel => text_page(99, Some((0, 0, LETTER_300.0, 450))),
        PatchLayout::KodakSheet | PatchLayout::AllianceT => patch_sheet(code),
    };
    with_patch_bars(page, code, layout)
}

/// `page`, at 300 dpi, with `code`'s bars on it as `layout` puts them,
/// from the edge inward: wide bars 0.20 in, narrow ones and the spaces
/// 0.08 in.
pub fn with_patch_bars(mut page: GrayImage, code: PatchCode, layout: PatchLayout) -> GrayImage {
    assert!(
        layout != PatchLayout::AllianceT || code == PatchCode::T,
        "Alliance's sheet is a patch T"
    );
    let px = |inches: f32| (inches * 300.0).round() as i32;
    let (w, h) = (page.width() as i32, page.height() as i32);
    for &(edge, inset, from, to) in layout.places() {
        let (a0, a1) = (px(from), px(to));
        let mut depth = inset;
        for wide in code.bars() {
            let width = if wide { 0.20 } else { 0.08 };
            let (d0, d1) = (px(depth), px(depth + width));
            let bar = match edge {
                Edge::Top => (a0, d0, a1 - a0, d1 - d0),
                Edge::Bottom => (a0, h - d1, a1 - a0, d1 - d0),
                Edge::Left => (d0, a0, d1 - d0, a1 - a0),
                Edge::Right => (w - d1, a0, d1 - d0, a1 - a0),
            };
            fill(&mut page, bar, BAR);
            depth += width + 0.08;
        }
    }
    page
}

/// A patch sheet's middle, as Kodak's and Alliance's print it, clear of
/// the bars: a title, lines of text, and the type as Code 39 (`PATCH1` …
/// `PATCHT`), which some software reads instead of the bars.
fn patch_sheet(code: PatchCode) -> GrayImage {
    let mut pen = Pen {
        img: blank_page(),
        clear: None,
    };
    let mut rng = Rng::new(5);
    pen.word(&mut rng, 750, 700, 100);
    for base in (850..1550).step_by(62) {
        let mut x = 600;
        while x < 1750 {
            x += pen.word(&mut rng, x, base, 22) + 22;
        }
    }
    let mark = match code {
        PatchCode::One => '1',
        PatchCode::Two => '2',
        PatchCode::Three => '3',
        PatchCode::Four => '4',
        PatchCode::Six => '6',
        PatchCode::T => 'T',
    };
    let text = encode(Symbology::Code39, &format!("PATCH{mark}"));
    paste(&mut pen.img, &text, 900, 1950, 6, 150);
    pen.img
}

/// What a scan does to a page, applied in order by [`degrade`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Degradation {
    /// Turned by a few degrees about the center, clockwise for a positive
    /// angle: bilinear, white at the corners, the page's size kept.
    Skew(f32),
    /// Turned a quarter, a half or three quarters clockwise: 90, 180 or
    /// 270.
    Turn(u16),
    /// Gaussian noise of `sigma` gray levels.
    Noise { sigma: f64, seed: u64 },
    /// A `rate` of the pixels set black or white.
    SaltPepper { rate: f64, seed: u64 },
    /// Gaussian blur of a sigma in pixels.
    Blur(f32),
    /// Gray squeezed to 100..=215 with a little noise: a dull scan.
    LowContrast,
    /// Saved as a JPEG of this quality and read back.
    Jpeg(u8),
    /// Black and white: black below this gray.
    Threshold(u8),
    /// Resampled to this resolution (a Triangle filter).
    Dpi(u32),
}

/// `img`, at `from_dpi`, through `steps`, and its resolution after them.
pub fn degrade(img: &GrayImage, from_dpi: u32, steps: &[Degradation]) -> (GrayImage, u32) {
    let mut out = img.clone();
    let mut dpi = from_dpi;
    for step in steps {
        out = match *step {
            Skew(degrees) => rotate_about_center(
                &out,
                degrees.to_radians(),
                Interpolation::Bilinear,
                Border::Constant(Luma([255])),
            ),
            Turn(90) => imageops::rotate90(&out),
            Turn(180) => imageops::rotate180(&out),
            Turn(270) => imageops::rotate270(&out),
            Turn(t) => panic!("a turn of {t}°"),
            Noise { sigma, seed } => noise(out, sigma, seed),
            SaltPepper { rate, seed } => {
                let mut rng = Rng::new(seed);
                for p in out.pixels_mut() {
                    if rng.unit() < rate {
                        p.0[0] = if rng.below(2) == 0 { 0 } else { 255 };
                    }
                }
                out
            }
            Blur(sigma) => imageproc::filter::gaussian_blur_f32(&out, sigma),
            LowContrast => {
                for p in out.pixels_mut() {
                    p.0[0] = (100.0 + f32::from(p.0[0]) * (115.0 / 255.0)).round() as u8;
                }
                noise(out, 6.0, 77)
            }
            Jpeg(quality) => {
                let mut bytes = Vec::new();
                let encoder =
                    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, quality);
                out.write_with_encoder(encoder).expect("a JPEG");
                image::load_from_memory_with_format(&bytes, ImageFormat::Jpeg)
                    .expect("a JPEG read back")
                    .into_luma8()
            }
            Threshold(t) => {
                for p in out.pixels_mut() {
                    p.0[0] = if p.0[0] < t { 0 } else { 255 };
                }
                out
            }
            Dpi(to) => {
                let from = std::mem::replace(&mut dpi, to);
                if from == to {
                    out
                } else {
                    let (w, h) = (out.width() * to / from, out.height() * to / from);
                    imageops::resize(&out, w, h, FilterType::Triangle)
                }
            }
        };
    }
    (out, dpi)
}

/// `img` with Gaussian noise of `sigma` gray levels.
fn noise(mut img: GrayImage, sigma: f64, seed: u64) -> GrayImage {
    let mut rng = Rng::new(seed);
    for p in img.pixels_mut() {
        let v = f64::from(p.0[0]) + rng.normal() * sigma;
        p.0[0] = v.round().clamp(0.0, 255.0) as u8;
    }
    img
}

/// The bench's 17 scans of a 300 dpi page (proposal 22): turned, skewed,
/// noisy, blurred, speckled, dull, JPEG, black and white, at 300, 200 and
/// 150 dpi, and an office scanner's mix of blur, skew, noise and 1-bit.
pub const BENCH: [(&str, &[Degradation]); 17] = [
    ("clean", &[]),
    ("rot+1.5", &[Skew(1.5)]),
    ("rot-3", &[Skew(-3.0)]),
    ("rot90", &[Turn(90)]),
    ("rot180", &[Turn(180)]),
    (
        "noise25",
        &[Noise {
            sigma: 25.0,
            seed: 11,
        }],
    ),
    ("blur1.5", &[Blur(1.5)]),
    (
        "saltpep2%",
        &[SaltPepper {
            rate: 0.02,
            seed: 13,
        }],
    ),
    ("lowcontrast", &[LowContrast]),
    ("jpeg40", &[Jpeg(40)]),
    ("bw300", &[Threshold(128)]),
    ("gray200", &[Dpi(200)]),
    ("bw200", &[Dpi(200), Threshold(128)]),
    ("gray150", &[Dpi(150)]),
    ("bw150", &[Dpi(150), Threshold(128)]),
    ("office200bw", &office(200)),
    ("office150bw", &office(150)),
];

/// The bench's office scanner at `dpi`: blurred, skewed a degree, noisy,
/// then 1-bit.
const fn office(dpi: u32) -> [Degradation; 5] {
    [
        Blur(0.8),
        Skew(1.0),
        Noise {
            sigma: 8.0,
            seed: 17,
        },
        Dpi(dpi),
        Threshold(128),
    ]
}

/// The ways the codes' tests scan a page, each at 300 dpi before it's
/// resampled: as it is, turned, skewed ±3°, with a scanner's noise (σ 10;
/// the bench's office mix has 8) and blurred (σ 1.5 pixels).
pub const VARIATIONS: [(&str, &[Degradation]); 8] = [
    ("as is", &[]),
    ("turned 90°", &[Turn(90)]),
    ("turned 180°", &[Turn(180)]),
    ("turned 270°", &[Turn(270)]),
    ("skewed +3°", &[Skew(3.0)]),
    ("skewed -3°", &[Skew(-3.0)]),
    (
        "noise σ 10",
        &[Noise {
            sigma: 10.0,
            seed: 11,
        }],
    ),
    ("blur σ 1.5", &[Blur(1.5)]),
];

/// A page as [`scanned_every_way`] scans it.
pub struct Scan {
    /// How: "150 dpi 1-bit, skewed +3°".
    pub label: String,
    pub dpi: u32,
    pub image: GrayImage,
}

/// `page`, at 300 dpi, scanned every way the codes' tests read it: each
/// of [`VARIATIONS`], drawn at 300, 200 and 150 dpi, each in gray and in
/// black and white (thresholded at 128). 48 scans.
pub fn scanned_every_way(page: &GrayImage) -> Vec<Scan> {
    let mut scans = Vec::with_capacity(48);
    for (variation, steps) in VARIATIONS {
        let (varied, _) = degrade(page, 300, steps);
        for dpi in [300, 200, 150] {
            let (gray, _) = degrade(&varied, 300, &[Dpi(dpi)]);
            let (bw, _) = degrade(&gray, dpi, &[Threshold(128)]);
            for (tone, image) in [("gray", gray), ("1-bit", bw)] {
                scans.push(Scan {
                    label: format!("{dpi} dpi {tone}, {variation}"),
                    dpi,
                    image,
                });
            }
        }
    }
    scans
}

/// A small seeded generator (xorshift64*), so pages and noise are the same
/// on every run. sheaf's own rather than `fastrand`'s, as other tests'
/// are: the codes' tests were settled on these exact pages (every cell of
/// the matrix read, every negative none), and slices 4 and 5 build on
/// them, so a dependency update mustn't draw different ones.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Below `n`.
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    /// In [0, 1).
    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// A standard normal, by Box-Muller.
    fn normal(&mut self) -> f64 {
        let u1 = self.unit().max(1e-12);
        let u2 = self.unit();
        (-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos()
    }
}

/// The letters' shapes, as their letters: stems, arches, bowls, bars and
/// diagonals.
const LETTERS: &[u8] = b"lihnmuobdpqaecvxyktrzs";

/// Digits drawn at a capital's height: shapes with no ascender or
/// descender.
const DIGITS: &[u8; 10] = b"oIzseuacnx";

/// Draws a page's strokes, leaving `clear` white.
struct Pen {
    img: GrayImage,
    clear: Option<(u32, u32, u32, u32)>,
}

impl Pen {
    /// Whether the box at (x, y), w × h, stays out of `clear`.
    fn free(&self, x: i32, y: i32, w: i32, h: i32) -> bool {
        self.clear.is_none_or(|(cx, cy, cw, ch)| {
            let (cx, cy, cw, ch) = (cx as i32, cy as i32, cw as i32, ch as i32);
            x + w <= cx || x >= cx + cw || y + h <= cy || y >= cy + ch
        })
    }

    /// The box at (x, y), w × h, in ink, clipped to the page.
    fn fill(&mut self, x: i32, y: i32, w: i32, h: i32) {
        fill(&mut self.img, (x, y, w, h), INK);
    }

    /// A rule, unless it would cross `clear`.
    fn rule(&mut self, x: u32, y: u32, w: u32, h: u32) {
        let (x, y, w, h) = (x as i32, y as i32, w as i32, h as i32);
        if self.free(x, y, w, h) {
            self.fill(x, y, w, h);
        }
    }

    /// An ellipse's ring in the box at (x, y), w × h, `s` thick; `open`
    /// leaves its right side out, as a c's.
    fn bowl(&mut self, x: i32, y: i32, w: i32, h: i32, s: i32, open: bool) {
        let (rx, ry) = (w as f32 / 2.0, h as f32 / 2.0);
        let (cx, cy) = (x as f32 + rx, y as f32 + ry);
        let (ix, iy) = (rx - s as f32, ry - s as f32);
        for py in y..y + h {
            for px in x..x + w {
                let (dx, dy) = (px as f32 + 0.5 - cx, py as f32 + 0.5 - cy);
                let outside = (dx / rx).powi(2) + (dy / ry).powi(2) > 1.0;
                let inside = (dx / ix).powi(2) + (dy / iy).powi(2) < 1.0;
                let gap = open && dx > 0.0 && dy.abs() < ry / 3.0;
                if !outside && !inside && !gap {
                    self.fill(px, py, 1, 1);
                }
            }
        }
    }

    /// A stroke `s` thick from `from` to `to`.
    fn diagonal(&mut self, from: (i32, i32), to: (i32, i32), s: i32) {
        let steps = (to.0 - from.0).abs().max((to.1 - from.1).abs()).max(1);
        for i in 0..=steps {
            let x = from.0 + (to.0 - from.0) * i / steps;
            let y = from.1 + (to.1 - from.1) * i / steps;
            self.fill(x - s / 2, y - s / 2, s, s);
        }
    }

    /// A word of 2 to 9 letters with its left at `x` and its baseline at
    /// `base`, at an x-height of `xh`, and its width; 0, drawing nothing,
    /// if it would pass the body's right margin.
    fn word(&mut self, rng: &mut Rng, x: u32, base: u32, xh: i32) -> u32 {
        let letters: Vec<u8> = (0..2 + rng.below(8))
            .map(|_| LETTERS[rng.below(LETTERS.len() as u64) as usize])
            .collect();
        let width: i32 = letters.iter().map(|&l| advance(l, xh)).sum();
        let (x, base) = (x as i32, base as i32);
        if x + width > 2250 {
            return 0;
        }
        let (top, bottom) = (base - xh - xh / 2, base + xh * 2 / 5);
        if self.free(x, top, width, bottom - top) {
            let mut at = x;
            for l in letters {
                self.letter(l, at, base, xh);
                at += advance(l, xh);
            }
        }
        width as u32
    }

    /// `number`'s digits and points with its left at `x` and its baseline
    /// at `base`, `cap` pixels tall.
    fn number(&mut self, number: &str, x: u32, base: u32, cap: i32) {
        let (mut at, base) = (x as i32, base as i32);
        let width: i32 = number.bytes().map(|_| advance(b'o', cap)).sum();
        if !self.free(at, base - cap, width, cap) {
            return;
        }
        let s = (cap / 5).max(2);
        for b in number.bytes() {
            if b == b'.' {
                self.fill(at, base - s, s, s);
                at += s * 3;
            } else {
                let shape = DIGITS[usize::from(b - b'0')];
                self.letter(shape, at, base, cap);
                at += advance(shape, cap);
            }
        }
    }

    /// The letter shaped as `shape` in the cell at `x`, its baseline at
    /// `base`, `xh` pixels tall without its ascender or descender.
    fn letter(&mut self, shape: u8, x: i32, base: i32, xh: i32) {
        let s = (xh / 5).max(2);
        let w = xh * 9 / 10;
        let (top, mid) = (base - xh, base - xh / 2);
        let (ascender, descender) = (top - xh / 2, base + xh * 2 / 5);
        let (left, right) = (x, x + w - s);
        match shape {
            b'l' => self.fill(x + (w - s) / 2, ascender, s, base - ascender),
            b'I' => self.fill(x + (w - s) / 2, top, s, xh),
            b'i' => {
                self.fill(x + (w - s) / 2, top, s, xh);
                self.fill(x + (w - s) / 2, top - 2 * s, s, s);
            }
            b'h' | b'n' => {
                let from = if shape == b'h' { ascender } else { top };
                self.fill(left, from, s, base - from);
                self.fill(left, top, w, s);
                self.fill(right, top, s, xh);
            }
            b'm' => {
                let w = w * 3 / 2;
                for at in [left, x + (w - s) / 2, x + w - s] {
                    self.fill(at, top, s, xh);
                }
                self.fill(left, top, w, s);
            }
            b'u' => {
                self.fill(left, top, s, xh);
                self.fill(right, top, s, xh);
                self.fill(left, base - s, w, s);
            }
            b'o' | b'e' | b'c' => {
                self.bowl(x, top, w, xh, s, shape == b'c');
                if shape == b'e' {
                    self.fill(x, mid - s / 2, w, s);
                }
            }
            b'b' | b'd' | b'p' | b'q' | b'a' => {
                self.bowl(x, top, w, xh, s, false);
                let at = if matches!(shape, b'b' | b'p') {
                    left
                } else {
                    right
                };
                let (from, to) = match shape {
                    b'b' | b'd' => (ascender, base),
                    b'p' | b'q' => (top, descender),
                    _ => (top, base),
                };
                self.fill(at, from, s, to - from);
            }
            b'v' | b'y' => {
                let foot = if shape == b'y' {
                    (x + w / 4, descender)
                } else {
                    (x + w / 2, base)
                };
                self.diagonal((x + s / 2, top), (x + w / 2, base), s);
                self.diagonal((x + w - s / 2, top), foot, s);
            }
            b'x' => {
                self.diagonal((x + s / 2, top), (x + w - s / 2, base), s);
                self.diagonal((x + w - s / 2, top), (x + s / 2, base), s);
            }
            b'k' => {
                self.fill(left, ascender, s, base - ascender);
                self.diagonal((x + w - s / 2, top), (x + s, mid), s);
                self.diagonal((x + s, mid), (x + w - s / 2, base), s);
            }
            b't' => {
                self.fill(x + w / 3, top - xh / 4, s, base - top + xh / 4);
                self.fill(left, top, w * 3 / 4, s);
            }
            b'r' => {
                self.fill(left, top, s, xh);
                self.fill(left, top, w * 2 / 3, s);
            }
            b'z' => {
                self.fill(left, top, w, s);
                self.fill(left, base - s, w, s);
                self.diagonal((x + w - s / 2, top + s), (x + s / 2, base - s), s);
            }
            b's' => {
                for y in [top, mid - s / 2, base - s] {
                    self.fill(left, y, w, s);
                }
                self.fill(left, top, s, mid - top);
                self.fill(right, mid, s, base - mid);
            }
            other => unreachable!("no letter shaped {}", other as char),
        }
    }
}

/// The box (x, y, w, h) in `ink`, clipped to `img`.
fn fill(img: &mut GrayImage, (x, y, w, h): (i32, i32, i32, i32), ink: Luma<u8>) {
    let (iw, ih) = (img.width() as i32, img.height() as i32);
    for py in y.max(0)..(y + h).min(ih) {
        for px in x.max(0)..(x + w).min(iw) {
            img.put_pixel(px as u32, py as u32, ink);
        }
    }
}

/// How far the letter shaped as `shape` moves the pen at an x-height of
/// `xh`: its cell and the gap after it.
fn advance(shape: u8, xh: i32) -> i32 {
    let w = xh * 9 / 10;
    let w = if shape == b'm' { w * 3 / 2 } else { w };
    w + xh / 4
}
