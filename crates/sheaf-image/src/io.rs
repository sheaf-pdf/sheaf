//! Image files in: decoded, turned upright, with their resolution, and
//! kept as compact as they allow. A JPEG keeps its bytes (and its EXIF
//! turn), a black-and-white image is held as CCITT G4, a multi-page TIFF
//! gives one image per page.

use std::fs::File;
use std::io::{BufReader, Cursor, Read, Seek};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use image::{
    DynamicImage, ImageDecoder, ImageFormat, ImageReader, RgbImage, metadata::Orientation,
};
use tiff::tags::Tag;

/// Resolution assumed when an image doesn't record one.
pub const DEFAULT_DPI: f32 = 96.0;

/// One image, ready to become a page.
#[derive(Clone)]
pub struct PageImage {
    pub width: u32,
    pub height: u32,
    /// Pixels per inch, horizontally and vertically.
    pub dpi: (f32, f32),
    /// Clockwise quarter turns to show it upright (JPEGs only; the rest are
    /// turned when decoded).
    pub rotate: u16,
    pub pixels: Pixels,
}

#[derive(Clone)]
pub enum Pixels {
    /// A JPEG file's bytes and its component count (1 gray, 3 color).
    Jpeg { bytes: Vec<u8>, components: u8 },
    /// 8-bit gray, row by row.
    Gray(Vec<u8>),
    /// 8-bit RGB, row by row.
    Rgb(Vec<u8>),
    /// Black and white, CCITT Group 4 as [`crate::encode::g4`] writes it
    /// (K -1, `width` columns, `height` rows): encoded once, when the page
    /// is made.
    G4(Vec<u8>),
}

impl PageImage {
    /// Pixels the caller already has (a cleaned-up scan, say). Gray
    /// images whose pixels are all 0 or 255 are stored as black and white,
    /// encoded as G4 at once.
    pub fn from_image(image: DynamicImage, dpi: f32) -> PageImage {
        let (width, height) = (image.width(), image.height());
        let pixels = if image.color().has_color() {
            Pixels::Rgb(flatten_alpha(image).into_rgb8().into_raw())
        } else {
            let gray = flatten_alpha(image).into_luma8().into_raw();
            if gray.iter().all(|&v| v == 0 || v == 255) {
                Pixels::G4(crate::encode::g4(&gray, width, height))
            } else {
                Pixels::Gray(gray)
            }
        };
        PageImage {
            width,
            height,
            dpi: (dpi, dpi),
            rotate: 0,
            pixels,
        }
    }

    /// Pixels encoded as JPEG at `quality` (1–100): far smaller than Flate
    /// for photos of paper. Gray stays one component.
    pub fn jpeg(image: &DynamicImage, dpi: f32, quality: u8) -> Result<PageImage> {
        use image::codecs::jpeg::JpegEncoder;
        let mut bytes = Vec::new();
        let mut enc = JpegEncoder::new_with_quality(&mut bytes, quality.clamp(1, 100));
        let components = if image.color().has_color() {
            enc.encode_image(&flatten_alpha(image.clone()).into_rgb8())?;
            3
        } else {
            enc.encode_image(&flatten_alpha(image.clone()).into_luma8())?;
            1
        };
        Ok(PageImage {
            width: image.width(),
            height: image.height(),
            dpi: (dpi, dpi),
            rotate: 0,
            pixels: Pixels::Jpeg { bytes, components },
        })
    }

    /// Size in points as shown, after `rotate`.
    pub fn size_pt(&self) -> (f32, f32) {
        let w = self.width as f32 * 72.0 / self.dpi.0;
        let h = self.height as f32 * 72.0 / self.dpi.1;
        if self.rotate % 2 == 1 { (h, w) } else { (w, h) }
    }
}

/// The pages in an image file: one, or one per image in a TIFF.
pub fn read(path: &Path) -> Result<Vec<PageImage>> {
    let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    read_bytes(bytes).with_context(|| format!("reading {}", path.display()))
}

pub fn read_bytes(bytes: Vec<u8>) -> Result<Vec<PageImage>> {
    let format = sniff(&bytes).context("not an image sheaf knows")?;
    match format {
        ImageFormat::Jpeg => Ok(vec![jpeg(bytes)?]),
        ImageFormat::Tiff => tiff_pages(&bytes),
        _ => {
            let dpi = match format {
                ImageFormat::Png => png_dpi(&bytes),
                _ => None,
            };
            Ok(vec![decoded(&bytes, format, dpi)?])
        }
    }
}

/// Decode, turn upright, and keep the pixels.
fn decoded(bytes: &[u8], format: ImageFormat, dpi: Option<(f32, f32)>) -> Result<PageImage> {
    let mut decoder = ImageReader::with_format(Cursor::new(bytes), format).into_decoder()?;
    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
    let mut image = DynamicImage::from_decoder(decoder)?;
    image.apply_orientation(orientation);
    let dpi = dpi.unwrap_or((DEFAULT_DPI, DEFAULT_DPI));
    let mut page = PageImage::from_image(image, dpi.0);
    page.dpi = if matches!(
        orientation,
        Orientation::Rotate90
            | Orientation::Rotate270
            | Orientation::Rotate90FlipH
            | Orientation::Rotate270FlipH
    ) {
        (dpi.1, dpi.0)
    } else {
        dpi
    };
    Ok(page)
}

/// A JPEG goes in as is unless it's mirrored or CMYK.
fn jpeg(bytes: Vec<u8>) -> Result<PageImage> {
    let info = jpeg_info(&bytes).context("not a readable JPEG")?;
    let orientation = ImageReader::with_format(Cursor::new(&bytes), ImageFormat::Jpeg)
        .into_decoder()?
        .orientation()
        .unwrap_or(Orientation::NoTransforms);
    let rotate = match orientation {
        Orientation::NoTransforms => Some(0),
        Orientation::Rotate90 => Some(1),
        Orientation::Rotate180 => Some(2),
        Orientation::Rotate270 => Some(3),
        _ => None,
    };
    let dpi = info.dpi.unwrap_or((DEFAULT_DPI, DEFAULT_DPI));
    match (rotate, info.components) {
        (Some(rotate), c @ (1 | 3)) => Ok(PageImage {
            width: info.width,
            height: info.height,
            dpi,
            rotate,
            pixels: Pixels::Jpeg {
                bytes,
                components: c,
            },
        }),
        _ => decoded(&bytes, ImageFormat::Jpeg, info.dpi),
    }
}

struct JpegInfo {
    width: u32,
    height: u32,
    components: u8,
    dpi: Option<(f32, f32)>,
}

/// Size, components and JFIF density, from the markers.
fn jpeg_info(b: &[u8]) -> Option<JpegInfo> {
    if b.get(..2)? != [0xFF, 0xD8] {
        return None;
    }
    let mut dpi = None;
    let mut i = 2;
    while i + 4 <= b.len() {
        if b[i] != 0xFF {
            return None;
        }
        let marker = b[i + 1];
        if marker == 0xFF {
            i += 1;
            continue;
        }
        let len = u16::from_be_bytes([b[i + 2], b[i + 3]]) as usize;
        let seg = b.get(i + 4..i + 2 + len)?;
        match marker {
            // APP0 JFIF: units, x density, y density.
            0xE0 if seg.starts_with(b"JFIF\0") && seg.len() >= 12 => {
                let x = u16::from_be_bytes([seg[8], seg[9]]) as f32;
                let y = u16::from_be_bytes([seg[10], seg[11]]) as f32;
                dpi = match seg[7] {
                    1 if x > 0.0 && y > 0.0 => Some((x, y)),
                    2 if x > 0.0 && y > 0.0 => Some((x * 2.54, y * 2.54)),
                    _ => None,
                };
            }
            // Start of frame (any but DHT, JPG and DAC).
            0xC0..=0xCF if !matches!(marker, 0xC4 | 0xC8 | 0xCC) => {
                return Some(JpegInfo {
                    height: u16::from_be_bytes([*seg.get(1)?, *seg.get(2)?]) as u32,
                    width: u16::from_be_bytes([*seg.get(3)?, *seg.get(4)?]) as u32,
                    components: *seg.get(5)?,
                    dpi,
                });
            }
            _ => {}
        }
        i += 2 + len;
    }
    None
}

/// The `pHYs` resolution, when it's in pixels per meter.
fn png_dpi(b: &[u8]) -> Option<(f32, f32)> {
    let mut i = 8;
    while i + 8 <= b.len() {
        let len = u32::from_be_bytes(b[i..i + 4].try_into().ok()?) as usize;
        let kind = &b[i + 4..i + 8];
        if kind == b"pHYs" {
            let d = b.get(i + 8..i + 17)?;
            let x = u32::from_be_bytes(d[0..4].try_into().ok()?) as f32;
            let y = u32::from_be_bytes(d[4..8].try_into().ok()?) as f32;
            return (d[8] == 1 && x > 0.0 && y > 0.0).then_some((x * 0.0254, y * 0.0254));
        }
        if kind == b"IDAT" {
            return None;
        }
        i += 12 + len;
    }
    None
}

/// Every page in a TIFF.
fn tiff_pages(bytes: &[u8]) -> Result<Vec<PageImage>> {
    let mut dec = tiff_decoder(Cursor::new(bytes))?;
    let mut out = Vec::new();
    loop {
        let (image, dpi) = tiff_image(&mut dec, out.len() + 1)?;
        let mut page = PageImage::from_image(image, dpi.0);
        page.dpi = dpi;
        out.push(page);
        if !next_page(&mut dec)? {
            return Ok(out);
        }
    }
}

/// How much a TIFF's decode may allocate: image's own default limit, which
/// [`load`] reads within (tiff's own stops at 256 MiB, under a 1200 dpi
/// Letter page in color).
const TIFF_LIMIT: usize = 512 << 20;

/// A TIFF decoder over `r`, at its first image, within [`TIFF_LIMIT`].
fn tiff_decoder<R: Read + Seek>(r: R) -> tiff::TiffResult<tiff::decoder::Decoder<R>> {
    let mut limits = tiff::decoder::Limits::default();
    limits.decoding_buffer_size = TIFF_LIMIT;
    limits.intermediate_buffer_size = TIFF_LIMIT;
    limits.ifd_value_size = TIFF_LIMIT;
    Ok(tiff::decoder::Decoder::new(r)?.with_limits(limits))
}

/// `dec` moved on to its TIFF's next page, past reduced-resolution images
/// (NewSubfileType bit 0: a thumbnail, a pyramid's smaller levels); false
/// when there's none. The first image is always a page, as [`load`] reads
/// it.
fn next_page<R: Read + Seek>(dec: &mut tiff::decoder::Decoder<R>) -> tiff::TiffResult<bool> {
    while dec.more_images() {
        dec.next_image()?;
        let kind = (dec.find_tag(Tag::NewSubfileType).ok().flatten())
            .and_then(|v| v.into_u32().ok())
            .unwrap_or(0);
        if kind & 1 == 0 {
            return Ok(true);
        }
    }
    Ok(false)
}

/// The TIFF image `dec` is at, image `number` (from 1) of its file: its
/// pixels as stored, not turned, and its resolution.
fn tiff_image<R: Read + Seek>(
    dec: &mut tiff::decoder::Decoder<R>,
    number: usize,
) -> Result<(DynamicImage, (f32, f32))> {
    use tiff::ColorType as C;
    use tiff::decoder::{Decoder, DecodingResult as D};
    let (w, h) = dec.dimensions()?;
    let color = dec.colortype()?;
    let res = |dec: &mut Decoder<R>, tag| {
        dec.find_tag(tag)
            .ok()
            .flatten()
            .and_then(resolution)
            .filter(|v| *v > 0.0)
    };
    let unit = (dec.find_tag(Tag::ResolutionUnit).ok().flatten())
        .and_then(|v| v.into_u16().ok())
        .unwrap_or(2);
    let dpi = match (res(dec, Tag::XResolution), res(dec, Tag::YResolution)) {
        (Some(x), y) => {
            let y = y.unwrap_or(x);
            match unit {
                3 => (x * 2.54, y * 2.54),
                2 => (x, y),
                _ => (DEFAULT_DPI, DEFAULT_DPI),
            }
        }
        _ => (DEFAULT_DPI, DEFAULT_DPI),
    };
    let n = w as usize * h as usize;
    let image = match (color, dec.read_image()?) {
        (C::Gray(1), D::U8(packed)) => {
            let row = w.div_ceil(8) as usize;
            let mut gray = Vec::with_capacity(n);
            for r in packed.chunks(row).take(h as usize) {
                gray.extend((0..w as usize).map(|x| {
                    if r[x / 8] & (0x80 >> (x % 8)) != 0 {
                        255
                    } else {
                        0
                    }
                }));
            }
            DynamicImage::ImageLuma8(image::GrayImage::from_raw(w, h, gray).context("size")?)
        }
        (C::Gray(8), D::U8(v)) => {
            DynamicImage::ImageLuma8(image::GrayImage::from_raw(w, h, v).context("size")?)
        }
        (C::Gray(16), D::U16(v)) => {
            DynamicImage::ImageLuma16(image::ImageBuffer::from_raw(w, h, v).context("size")?)
        }
        (C::GrayA(8), D::U8(v)) => {
            DynamicImage::ImageLumaA8(image::ImageBuffer::from_raw(w, h, v).context("size")?)
        }
        (C::RGB(8), D::U8(v)) => {
            DynamicImage::ImageRgb8(image::RgbImage::from_raw(w, h, v).context("size")?)
        }
        (C::RGB(16), D::U16(v)) => {
            DynamicImage::ImageRgb16(image::ImageBuffer::from_raw(w, h, v).context("size")?)
        }
        (C::RGBA(8), D::U8(v)) => {
            DynamicImage::ImageRgba8(image::RgbaImage::from_raw(w, h, v).context("size")?)
        }
        (C::RGBA(16), D::U16(v)) => {
            DynamicImage::ImageRgba16(image::ImageBuffer::from_raw(w, h, v).context("size")?)
        }
        (C::CMYK(8), D::U8(v)) => {
            let rgb: Vec<u8> = v
                .as_chunks::<4>()
                .0
                .iter()
                .flat_map(|p| {
                    let k = 255 - p[3] as u16;
                    [0, 1, 2].map(|c| ((255 - p[c] as u16) * k / 255) as u8)
                })
                .collect();
            DynamicImage::ImageRgb8(image::RgbImage::from_raw(w, h, rgb).context("size")?)
        }
        (c, _) => bail!("TIFF image {number} is {c:?}, which sheaf can't read"),
    };
    Ok((image, dpi))
}

/// A TIFF resolution tag's value. TIFF stores it as a RATIONAL, which
/// `tiff`'s `get_tag_f32` won't convert.
fn resolution(v: tiff::decoder::ifd::Value) -> Option<f32> {
    use tiff::decoder::ifd::Value as V;
    match v {
        V::Rational(n, d) if d != 0 => Some(n as f32 / d as f32),
        V::Float(f) => Some(f),
        V::Double(f) => Some(f as f32),
        V::Short(n) => Some(n as f32),
        V::Unsigned(n) => Some(n as f32),
        V::List(mut l) if !l.is_empty() => resolution(l.swap_remove(0)),
        _ => None,
    }
}

/// Alpha composited onto white: pages are paper.
fn flatten_alpha(image: DynamicImage) -> DynamicImage {
    if !image.color().has_alpha() {
        return image;
    }
    let mut rgba = image.into_rgba8();
    for p in rgba.pixels_mut() {
        let a = p[3] as u16;
        for c in 0..3 {
            p[c] = ((p[c] as u16 * a + 255 * (255 - a)) / 255) as u8;
        }
        p[3] = 255;
    }
    DynamicImage::ImageRgba8(rgba).into_rgb8().into()
}

/// Whether `path`'s extension is one of the image types `read` takes.
pub fn is_image_path(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .and_then(ImageFormat::from_extension)
        .is_some_and(taken)
}

/// The type of image `bytes` are, by how they start: None when it isn't
/// one `read` takes.
pub fn sniff(bytes: &[u8]) -> Option<ImageFormat> {
    image::guess_format(bytes).ok().filter(|&f| taken(f))
}

/// Whether `read` takes images of type `f`.
fn taken(f: ImageFormat) -> bool {
    matches!(
        f,
        ImageFormat::Jpeg
            | ImageFormat::Png
            | ImageFormat::Tiff
            | ImageFormat::Bmp
            | ImageFormat::Gif
            | ImageFormat::WebP
    )
}

/// An image file as RGB, turned upright per its EXIF orientation.
pub fn load(path: &Path) -> Result<RgbImage> {
    let mut decoder = ImageReader::open(path)
        .with_context(|| format!("opening {}", path.display()))?
        .with_guessed_format()?
        .into_decoder()
        .with_context(|| format!("reading {}", path.display()))?;
    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
    let mut image = DynamicImage::from_decoder(decoder)
        .with_context(|| format!("decoding {}", path.display()))?;
    image.apply_orientation(orientation);
    Ok(image.into_rgb8())
}

/// The pages in an image file, in order: a TIFF's read one at a time from
/// the file, which is never held whole, and any other file's one image.
/// The first is [`load`]'s image, byte for byte, whatever kind of TIFF it
/// is. A TIFF's later pages are decoded as images-to-pdf decodes them
/// (CMYK made RGB by its integer sums, which can come out one below
/// `load`'s; a kind it can't read, planar or CMYK16 say, an error), each
/// turned upright by its Orientation tag as `load` turns the first;
/// reduced-resolution images (thumbnails) aren't pages. The type is told
/// by the file's first bytes.
pub fn frames(path: &Path) -> Result<Frames> {
    let opening = || format!("opening {}", path.display());
    let mut file = File::open(path).with_context(opening)?;
    let mut head = Vec::with_capacity(16);
    (&mut file)
        .take(16)
        .read_to_end(&mut head)
        .with_context(opening)?;
    let next = if sniff(&head) == Some(ImageFormat::Tiff) {
        file.rewind().with_context(opening)?;
        let dec = tiff_decoder(BufReader::new(file))
            .with_context(|| format!("reading {}", path.display()))?;
        Next::Tiff {
            dec: Box::new(dec),
            given: 0,
        }
    } else {
        Next::Load
    };
    Ok(Frames {
        path: path.to_path_buf(),
        next,
    })
}

/// How many pages [`frames`] gives for `path`: a TIFF's, counted from its
/// images' headers without decoding them, or 1.
pub fn count_frames(path: &Path) -> Result<usize> {
    let Next::Tiff { mut dec, .. } = frames(path)?.next else {
        return Ok(1);
    };
    let mut n = 1;
    while next_page(&mut dec).with_context(|| format!("reading {}", path.display()))? {
        n += 1;
    }
    Ok(n)
}

/// An image file's pages, read as they're asked for: see [`frames`].
pub struct Frames {
    path: PathBuf,
    next: Next,
}

enum Next {
    /// Not a TIFF: [`load`]'s image, not read yet.
    Load,
    /// A TIFF, `given` of its pages given so far, `dec` at the last.
    Tiff {
        dec: Box<tiff::decoder::Decoder<BufReader<File>>>,
        given: usize,
    },
    Done,
}

impl Iterator for Frames {
    type Item = Result<RgbImage>;

    /// The next page; after an error, none.
    fn next(&mut self) -> Option<Result<RgbImage>> {
        match std::mem::replace(&mut self.next, Next::Done) {
            Next::Done => None,
            Next::Load => Some(load(&self.path)),
            Next::Tiff { mut dec, given } => {
                let frame = if given == 0 {
                    load(&self.path)
                } else {
                    match next_page(&mut dec) {
                        Ok(false) => return None,
                        Ok(true) => tiff_frame(&mut dec, given + 1),
                        Err(e) => Err(e.into()),
                    }
                    .with_context(|| format!("reading {}, page {}", self.path.display(), given + 1))
                };
                if frame.is_ok() {
                    self.next = Next::Tiff {
                        dec,
                        given: given + 1,
                    };
                }
                Some(frame)
            }
        }
    }
}

/// The TIFF image `dec` is at, image `number` of its file, as RGB, turned
/// upright by its Orientation tag; a tag that isn't one value from 1 to 8
/// is ignored, as `load` ignores it.
fn tiff_frame<R: Read + Seek>(
    dec: &mut tiff::decoder::Decoder<R>,
    number: usize,
) -> Result<RgbImage> {
    let orientation = (dec.find_tag(Tag::Orientation).ok().flatten())
        .and_then(|v| v.into_u16().ok())
        .and_then(|v| Orientation::from_exif(v.min(255) as u8))
        .unwrap_or(Orientation::NoTransforms);
    let (mut image, _) = tiff_image(dec, number)?;
    image.apply_orientation(orientation);
    Ok(image.into_rgb8())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gradient(w: u32, h: u32) -> DynamicImage {
        DynamicImage::ImageRgb8(image::RgbImage::from_fn(w, h, |x, y| {
            image::Rgb([(x * 255 / w) as u8, (y * 255 / h) as u8, 128])
        }))
    }

    fn encode(image: &DynamicImage, format: ImageFormat) -> Vec<u8> {
        let mut out = Cursor::new(Vec::new());
        image.write_to(&mut out, format).unwrap();
        out.into_inner()
    }

    #[test]
    fn jfif_density_sets_the_size() {
        let mut bytes = encode(&gradient(300, 600), ImageFormat::Jpeg);
        // The encoder writes a JFIF APP0 right after SOI: set 300 dpi.
        assert_eq!(&bytes[6..11], b"JFIF\0");
        bytes[13] = 1;
        bytes[14..16].copy_from_slice(&300u16.to_be_bytes());
        bytes[16..18].copy_from_slice(&300u16.to_be_bytes());
        let pages = read_bytes(bytes).unwrap();
        assert_eq!(pages[0].size_pt(), (72.0, 144.0));
    }

    #[test]
    fn multi_page_tiff() {
        use tiff::encoder::{TiffEncoder, colortype};
        let mut out = Cursor::new(Vec::new());
        let mut enc = TiffEncoder::new(&mut out).unwrap();
        let a = gradient(30, 20).into_rgb8();
        enc.write_image::<colortype::RGB8>(30, 20, a.as_raw())
            .unwrap();
        let b = vec![200u8; 50 * 40];
        enc.write_image::<colortype::Gray8>(50, 40, &b).unwrap();
        let pages = read_bytes(out.into_inner()).unwrap();
        assert_eq!(pages.len(), 2);
        assert_eq!((pages[1].width, pages[1].height), (50, 40));
    }

    #[test]
    fn a_black_and_white_page_is_held_as_g4() {
        // Letter at 300 dpi: white, with black bars where lines of text go.
        let (w, h) = (2550, 3300);
        let gray = image::GrayImage::from_fn(w, h, |x, y| {
            let ink = (300..2250).contains(&x) && (300..3000).contains(&y) && y % 100 < 30;
            image::Luma([if ink { 0 } else { 255 }])
        });
        let page = PageImage::from_image(DynamicImage::ImageLuma8(gray.clone()), 300.0);
        let Pixels::G4(bytes) = &page.pixels else {
            panic!("a black and white page should be held as G4");
        };
        assert_eq!(*bytes, crate::encode::g4(gray.as_raw(), w, h));
        assert!(
            bytes.len() * 100 < (w * h) as usize,
            "{} bytes of G4",
            bytes.len()
        );
    }

    fn write(dir: &tempfile::TempDir, name: &str, bytes: Vec<u8>) -> PathBuf {
        let path = dir.path().join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    #[test]
    fn a_tiff_gives_its_pages_one_at_a_time() {
        use tiff::encoder::{TiffEncoder, colortype};
        let dir = tempfile::tempdir().unwrap();
        let mut out = Cursor::new(Vec::new());
        let mut enc = TiffEncoder::new(&mut out).unwrap();
        let a = gradient(30, 20).into_rgb8();
        enc.write_image::<colortype::RGB8>(30, 20, a.as_raw())
            .unwrap();
        let b = image::GrayImage::from_fn(50, 40, |x, y| image::Luma([(x * 5 + y) as u8]));
        enc.write_image::<colortype::Gray8>(50, 40, b.as_raw())
            .unwrap();
        let path = write(&dir, "two.tif", out.into_inner());
        let pages: Vec<RgbImage> = frames(&path).unwrap().collect::<Result<_>>().unwrap();
        assert_eq!(pages.len(), 2);
        assert_eq!(pages[0], a);
        assert_eq!(pages[1], DynamicImage::ImageLuma8(b).into_rgb8());
    }

    /// An image's tags: tag, type, values.
    type Tags = Vec<(u16, u16, Vec<u32>)>;

    /// A little-endian TIFF made by hand, for what tiff's encoder won't
    /// write: each image its tags (tag, type, values; 3 SHORT, 4 LONG, 8
    /// SSHORT) and its strips, whose offsets and byte counts are added.
    fn by_hand(images: &[(Tags, Vec<Vec<u8>>)]) -> Vec<u8> {
        let mut out = b"II*\0\0\0\0\0".to_vec();
        let mut link = 4;
        for (tags, strips) in images {
            let mut tags = tags.clone();
            let mut offsets = Vec::new();
            for strip in strips {
                offsets.push(out.len() as u32);
                out.extend(strip);
            }
            tags.push((273, 4, offsets));
            tags.push((279, 4, strips.iter().map(|s| s.len() as u32).collect()));
            tags.sort_by_key(|t| t.0);
            let mut entries = Vec::new();
            for (tag, kind, values) in tags {
                let bytes: Vec<u8> = (values.iter())
                    .flat_map(|&v| match kind {
                        3 | 8 => (v as u16).to_le_bytes().to_vec(),
                        _ => v.to_le_bytes().to_vec(),
                    })
                    .collect();
                let field = if bytes.len() <= 4 {
                    let mut b = bytes;
                    b.resize(4, 0);
                    b
                } else {
                    out.resize(out.len().next_multiple_of(2), 0);
                    let at = out.len() as u32;
                    out.extend(bytes);
                    at.to_le_bytes().to_vec()
                };
                entries.push((tag, kind, values.len() as u32, field));
            }
            out.resize(out.len().next_multiple_of(2), 0);
            let ifd = out.len() as u32;
            out[link..link + 4].copy_from_slice(&ifd.to_le_bytes());
            out.extend((entries.len() as u16).to_le_bytes());
            for (tag, kind, count, field) in entries {
                out.extend(tag.to_le_bytes());
                out.extend(kind.to_le_bytes());
                out.extend(count.to_le_bytes());
                out.extend(field);
            }
            link = out.len();
            out.extend([0; 4]);
        }
        out
    }

    /// An uncompressed image's tags made by hand: `w` x `h`, `bits` a
    /// sample, as `photometric` says, its samples `planar` or interleaved.
    fn hand_tags(w: u32, h: u32, bits: &[u32], photometric: u32, planar: bool) -> Tags {
        vec![
            (256, 4, vec![w]),
            (257, 4, vec![h]),
            (258, 3, bits.to_vec()),
            (259, 3, vec![1]),
            (262, 3, vec![photometric]),
            (277, 3, vec![bits.len() as u32]),
            (278, 4, vec![h]),
            (284, 3, vec![if planar { 2 } else { 1 }]),
        ]
    }

    /// TIFFs of each kind `load` reads that the tests try, in `dir`: one
    /// image, or `first` and then the kind's image when there's a `first`.
    fn kinds(dir: &tempfile::TempDir, first: Option<&image::GrayImage>) -> Vec<PathBuf> {
        use crate::encode::{TiffPage, TiffWriter};
        use tiff::encoder::{DirectoryEncoder, TiffEncoder, TiffKindStandard, colortype};
        type Enc<'a> = TiffEncoder<&'a mut Cursor<Vec<u8>>>;
        type Dir<'a, 'b> = DirectoryEncoder<'a, &'b mut Cursor<Vec<u8>>, TiffKindStandard>;
        let prefix = if first.is_some() { "two-" } else { "" };
        let rgb = gradient(40, 30).into_rgb8();
        let gray = DynamicImage::ImageRgb8(rgb.clone()).into_luma8();
        let bw = image::GrayImage::from_fn(40, 30, |x, y| {
            image::Luma([if (x * y) % 7 < 3 { 0 } else { 255 }])
        });
        let mut files = Vec::new();
        for (name, page) in [
            ("rgb.tif", TiffPage::Rgb(&rgb)),
            ("gray.tif", TiffPage::Gray(&gray)),
            ("bw.tif", TiffPage::Bilevel(&bw)),
        ] {
            let mut out = Cursor::new(Vec::new());
            let mut w = TiffWriter::new(&mut out, 300, 1 + first.is_some() as usize).unwrap();
            if let Some(first) = first {
                w.add(TiffPage::Gray(first)).unwrap();
            }
            w.add(page).unwrap();
            files.push(write(dir, &format!("{prefix}{name}"), out.into_inner()));
        }
        let encoded = |name: &str, f: &dyn Fn(&mut Enc)| {
            let mut out = Cursor::new(Vec::new());
            let mut enc = TiffEncoder::new(&mut out).unwrap();
            if let Some(first) = first {
                (enc.write_image::<colortype::Gray8>(
                    first.width(),
                    first.height(),
                    first.as_raw(),
                ))
                .unwrap();
            }
            f(&mut enc);
            write(dir, &format!("{prefix}{name}"), out.into_inner())
        };
        let wide: Vec<u16> = (0..40 * 30).map(|i| (i * 53) as u16).collect();
        files.push(encoded("gray16.tif", &|enc| {
            enc.write_image::<colortype::Gray16>(40, 30, &wide).unwrap();
        }));
        // Stored on its side; Orientation 6 shows it turned a quarter
        // clockwise. Then the tag as two values, and as a signed one,
        // which aren't an orientation.
        let tagged = |name: &str, tag: &dyn Fn(&mut Dir)| {
            encoded(name, &|enc| {
                let mut im = enc.new_image::<colortype::RGB8>(40, 30).unwrap();
                tag(im.encoder());
                im.write_data(rgb.as_raw()).unwrap();
            })
        };
        let turned = tagged("turned.tif", &|d| {
            d.write_tag(Tag::Orientation, 6u16).unwrap()
        });
        if first.is_none() {
            assert_eq!(load(&turned).unwrap().dimensions(), (30, 40));
        }
        files.push(turned);
        files.push(tagged("orientation-pair.tif", &|d| {
            d.write_tag(Tag::Orientation, &[6u16, 1][..]).unwrap()
        }));
        files.push(tagged("signed-orientation.tif", &|d| {
            d.write_tag(Tag::Orientation, 6i16).unwrap()
        }));
        let cmyk: Vec<u8> = (0..40 * 30 * 4).map(|i| (i * 37 % 256) as u8).collect();
        files.push(encoded("cmyk.tif", &|enc| {
            enc.write_image::<colortype::CMYK8>(40, 30, &cmyk).unwrap();
        }));
        let cmyk16: Vec<u16> = (0..40 * 30 * 4).map(|i| (i * 4099) as u16).collect();
        files.push(encoded("cmyk16.tif", &|enc| {
            enc.write_image::<colortype::CMYK16>(40, 30, &cmyk16)
                .unwrap();
        }));
        let float: Vec<f32> = rgb.as_raw().iter().map(|&v| v as f32 / 255.0).collect();
        files.push(encoded("float.tif", &|enc| {
            enc.write_image::<colortype::RGB32Float>(40, 30, &float)
                .unwrap();
        }));
        // RGB stored a plane a strip.
        let planes: Vec<Vec<u8>> = (0..3)
            .map(|c| rgb.pixels().map(|p| p[c]).collect())
            .collect();
        let mut images = Vec::new();
        if let Some(first) = first {
            let tags = hand_tags(first.width(), first.height(), &[8], 1, false);
            images.push((tags, vec![first.as_raw().clone()]));
        }
        images.push((hand_tags(40, 30, &[8, 8, 8], 2, true), planes));
        files.push(write(dir, &format!("{prefix}planar.tif"), by_hand(&images)));
        files
    }

    /// The kinds images-to-pdf can't read, which a TIFF's later page is
    /// refused for.
    const FIRST_ONLY: [&str; 3] = ["cmyk16.tif", "float.tif", "planar.tif"];

    /// A TIFF's first page is `load`'s image, whatever its kind: those
    /// images-to-pdf can't read (CMYK16, float RGB,
    /// planar), CMYK whose integer sums differ, an Orientation tag that
    /// isn't one value; and nothing follows it.
    #[test]
    fn a_tiff_s_first_page_is_load_s_image() {
        let dir = tempfile::tempdir().unwrap();
        for path in kinds(&dir, None) {
            let mut pages = frames(&path).unwrap();
            let page = pages.next().unwrap().unwrap();
            assert!(page == load(&path).unwrap(), "{}", path.display());
            assert!(pages.next().is_none());
            assert_eq!(count_frames(&path).unwrap(), 1);
        }
    }

    /// A TIFF's later pages read as `load` reads each alone: CMYK within 1
    /// of it, by `tiff_image`'s integer sums, and the rest exactly, an
    /// Orientation tag that isn't one value ignored as `load` ignores it;
    /// the kinds images-to-pdf can't read an error, never another image.
    #[test]
    fn a_tiff_s_later_pages_read_as_load_reads_them() {
        let dir = tempfile::tempdir().unwrap();
        let first = image::GrayImage::from_fn(20, 10, |x, y| image::Luma([(x * 9 + y) as u8]));
        let alone = kinds(&dir, None);
        for (path, alone) in kinds(&dir, Some(&first)).iter().zip(&alone) {
            let name = alone.file_name().unwrap().to_str().unwrap();
            assert_eq!(count_frames(path).unwrap(), 2, "{name}");
            let mut pages = frames(path).unwrap();
            let page = pages.next().unwrap().unwrap();
            assert!(page == DynamicImage::ImageLuma8(first.clone()).into_rgb8());
            let page = pages.next().unwrap();
            if FIRST_ONLY.contains(&name) {
                assert!(page.is_err(), "{name}");
                continue;
            }
            let (page, loaded) = (page.unwrap(), load(alone).unwrap());
            assert!(pages.next().is_none());
            assert_eq!(page.dimensions(), loaded.dimensions(), "{name}");
            if name == "cmyk.tif" {
                let mut off = page.iter().zip(loaded.iter());
                assert!(off.all(|(a, b)| a.abs_diff(*b) <= 1), "{name}");
            } else {
                assert!(page == loaded, "{name}");
            }
        }
    }

    /// An image marked reduced-resolution (a thumbnail) isn't a page, in
    /// `frames` or in images-to-pdf's read.
    #[test]
    fn a_tiff_s_thumbnail_is_not_a_page() {
        use tiff::encoder::{TiffEncoder, colortype};
        let dir = tempfile::tempdir().unwrap();
        let rgb = gradient(40, 30).into_rgb8();
        let gray = image::GrayImage::from_fn(50, 40, |x, y| image::Luma([(x * 5 + y) as u8]));
        let mut out = Cursor::new(Vec::new());
        let mut enc = TiffEncoder::new(&mut out).unwrap();
        enc.write_image::<colortype::RGB8>(40, 30, rgb.as_raw())
            .unwrap();
        let small = image::imageops::thumbnail(&rgb, 10, 8);
        let mut im = enc.new_image::<colortype::RGB8>(10, 8).unwrap();
        im.encoder().write_tag(Tag::NewSubfileType, 1u32).unwrap();
        im.write_data(small.as_raw()).unwrap();
        enc.write_image::<colortype::Gray8>(50, 40, gray.as_raw())
            .unwrap();
        let bytes = out.into_inner();
        let path = write(&dir, "thumbnail.tif", bytes.clone());
        let pages: Vec<RgbImage> = frames(&path).unwrap().collect::<Result<_>>().unwrap();
        assert_eq!(pages.len(), 2);
        assert_eq!(count_frames(&path).unwrap(), 2);
        assert!(pages[1] == DynamicImage::ImageLuma8(gray).into_rgb8());
        let read = read_bytes(bytes).unwrap();
        assert_eq!((read.len(), read[1].width), (2, 50));
    }

    /// A page bigger than tiff's own 256 MiB limit and inside image's 512,
    /// as `load` reads it: a white 9500 x 9500 RGB page, 271 MB, second in
    /// its file (the first is `load`'s own).
    #[test]
    fn a_tiff_page_past_256_mib_is_read() {
        let dir = tempfile::tempdir().unwrap();
        let (w, h) = (9500u32, 9500u32);
        // PackBits: each row 222 runs of 128 bytes of 255, then 84.
        let mut row = Vec::new();
        for _ in 0..222 {
            row.extend([0x81, 0xFF]);
        }
        row.extend([(1i8 - 84) as u8, 0xFF]);
        let packed = row.repeat(h as usize);
        let mut big = hand_tags(w, h, &[8, 8, 8], 2, false);
        big[3].2 = vec![32773];
        let path = write(
            &dir,
            "big.tif",
            by_hand(&[
                (hand_tags(2, 2, &[8], 1, false), vec![vec![0; 4]]),
                (big, vec![packed]),
            ]),
        );
        let mut pages = frames(&path).unwrap();
        pages.next().unwrap().unwrap();
        let page = pages.next().unwrap().unwrap();
        assert_eq!(page.dimensions(), (w, h));
        assert!(page.iter().all(|&v| v == 255));
    }

    /// A page too big for any limit is an error, not a multiply that
    /// overflows: 70000 x 70000 gray, its header alone.
    #[test]
    fn a_tiff_page_too_big_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let bytes = by_hand(&[
            (hand_tags(2, 2, &[8], 1, false), vec![vec![0; 4]]),
            (hand_tags(70000, 70000, &[8], 1, false), vec![vec![0; 4]]),
        ]);
        let path = write(&dir, "huge.tif", bytes.clone());
        let mut pages = frames(&path).unwrap();
        pages.next().unwrap().unwrap();
        assert!(pages.next().unwrap().is_err());
        assert!(read_bytes(bytes).is_err());
    }

    #[test]
    fn any_other_image_is_one_frame_as_load_reads_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(
            &dir,
            "page.png",
            encode(&gradient(40, 30), ImageFormat::Png),
        );
        let mut pages = frames(&path).unwrap();
        assert!(pages.next().unwrap().unwrap() == load(&path).unwrap());
        assert!(pages.next().is_none());
    }

    #[test]
    fn image_paths_by_extension() {
        assert!(is_image_path(Path::new("a/b.JPG")));
        assert!(is_image_path(Path::new("scan.tif")));
        assert!(!is_image_path(Path::new("doc.pdf")));
    }
}
