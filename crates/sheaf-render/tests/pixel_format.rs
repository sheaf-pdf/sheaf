//! Rasters come back in the byte order they're asked for: a red page is
//! B=0,G=0,R=255 as BGRA and R=255,G=0,B=0 as RGBA.
//!
//! Needs libpdfium (`scripts/fetch-pdfium.sh`), like the app itself.

use sheaf_doc::pdf::{Document as Pdf, Object, Stream, dictionary};
use sheaf_edit::Rotation;
use sheaf_image::raster::PixelFormat;
use sheaf_render::PageIndex;
use sheaf_render::pdfium::PdfiumEngine;
use sheaf_render::{Job, Renderer};

fn red_page() -> Vec<u8> {
    page_drawing(b"1 0 0 rg 0 0 100 100 re f")
}

/// A page 100 pt square whose content is `content`.
fn page_drawing(content: &[u8]) -> Vec<u8> {
    let mut pdf = Pdf::with_version("1.5");
    let pages_id = pdf.new_object_id();
    let content = pdf.add_object(Stream::new(dictionary! {}, content.to_vec()));
    let page = pdf.add_object(dictionary! {
        "Type" => Object::name("Page"), "Parent" => pages_id, "Contents" => content,
        "MediaBox" => vec![0.into(), 0.into(), 100.into(), 100.into()],
    });
    pdf.objects.insert(
        pages_id,
        dictionary! {
            "Type" => Object::name("Pages"), "Kids" => vec![page.into()], "Count" => 1,
        }
        .into(),
    );
    let catalog =
        pdf.add_object(dictionary! { "Type" => Object::name("Catalog"), "Pages" => pages_id });
    pdf.trailer.set("Root", catalog);
    let mut bytes = Vec::new();
    pdf.save_to(&mut bytes).unwrap();
    bytes
}

#[test]
fn bgra_and_rgba_are_what_they_say() {
    let engine = PdfiumEngine::open_bytes(red_page().into(), None).unwrap();
    let page = Job::Page {
        width: 20,
        rotation: Rotation::R0,
    };
    let tile = Job::Tile {
        page_width: 20,
        page_height: 20,
        x: 5,
        y: 5,
        w: 4,
        h: 4,
        rotation: Rotation::R0,
    };
    for job in [page, tile] {
        let bgra = engine.render(PageIndex(0), job, PixelFormat::Bgra).unwrap();
        assert_eq!(&bgra.data[..4], &[0, 0, 255, 255], "{job:?} as BGRA");
        let rgba = engine.render(PageIndex(0), job, PixelFormat::Rgba).unwrap();
        assert_eq!(&rgba.data[..4], &[255, 0, 0, 255], "{job:?} as RGBA");
    }
}

#[test]
fn a_transparent_render_is_rgba_and_clear_where_nothing_is_drawn() {
    // Red in the page's lower left quarter, nothing elsewhere.
    let bytes = page_drawing(b"1 0 0 rg 0 0 50 50 re f");
    let engine = PdfiumEngine::open_bytes(bytes.into(), None).unwrap();
    let stamp = engine.render_transparent(PageIndex(0), 20).unwrap();
    assert_eq!((stamp.width, stamp.height), (20, 20));
    assert_eq!(stamp.format, PixelFormat::Rgba);
    let at = |x: usize, y: usize| {
        let i = (y * 20 + x) * 4;
        [
            stamp.data[i],
            stamp.data[i + 1],
            stamp.data[i + 2],
            stamp.data[i + 3],
        ]
    };
    assert_eq!(at(4, 15), [255, 0, 0, 255], "inside the red, as RGBA");
    assert_eq!(at(15, 4)[3], 0, "outside it, clear");
    assert_eq!(at(4, 4)[3], 0);
    assert_eq!(at(15, 15)[3], 0);

    // The page view's render of the same page is white there, and opaque.
    let page = Job::Page {
        width: 20,
        rotation: Rotation::R0,
    };
    let view = engine
        .render(PageIndex(0), page, PixelFormat::Rgba)
        .unwrap();
    assert_eq!(&view.data[(4 * 20 + 15) * 4..][..4], &[255, 255, 255, 255]);
}
