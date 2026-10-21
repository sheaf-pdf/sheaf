//! The OCR text layer `sheaf-doc` writes must be real text to PDFium: this
//! saves a layer and checks Find, over the characters the engine reads,
//! hits the words where the glyphs are, for every combination of the
//! file's own `/Rotate` and the turn the page was shown with when it was
//! recognized, on a page whose crop box is off the origin.
//!
//! Needs libpdfium (`scripts/fetch-pdfium.sh`), like the app itself; it
//! fails rather than skips without it so CI can't silently lose it.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use lopdf::{Document as Pdf, Object, Stream, dictionary};
use sheaf_doc::materialize::Came;
use sheaf_doc::tx::fixture::{self, ocr};
use sheaf_doc::{Box2, Document, FixedCount, OcrWord, PageChars, Pt, Query, Store};
use sheaf_edit::{Rotation, Turn};
use sheaf_render::PageIndex;
use sheaf_render::Renderer;
use sheaf_render::pdfium::PdfiumEngine;

fn tempdir() -> PathBuf {
    // Counted, not timed: the tests run at once, and macOS's clock only
    // has microseconds, so two of them could be given the same directory.
    static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let d = std::env::temp_dir().join(format!(
        "sheaf-render-ocr-{}-{}",
        std::process::id(),
        N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A one-page Letter PDF with no content, a crop box off the origin and
/// the given `/Rotate`.
fn blank_pdf(path: &Path, rotate: i64) {
    let mut pdf = Pdf::with_version("1.5");
    let pages_id = pdf.new_object_id();
    let content = pdf.add_object(Stream::new(dictionary! {}, Vec::new()));
    let mut page = dictionary! {
        "Type" => "Page", "Parent" => pages_id, "Contents" => content,
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        "CropBox" => vec![20.into(), 30.into(), 600.into(), 770.into()],
    };
    if rotate != 0 {
        page.set("Rotate", rotate);
    }
    let page_id = pdf.add_object(page);
    pdf.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages", "Kids" => vec![Object::Reference(page_id)], "Count" => 1,
        }),
    );
    let catalog = pdf.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    pdf.trailer.set("Root", catalog);
    pdf.save(path).unwrap();
}

fn word(text: &str, b: Box2) -> OcrWord {
    OcrWord {
        text: text.into(),
        rect: b,
        confidence: 1.0,
    }
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() <= 2.0
}

fn close_box(a: Box2, b: Box2) -> bool {
    close(a.x, b.x)
        && close(a.y, b.y)
        && close(a.right(), b.right())
        && close(a.bottom(), b.bottom())
}

/// Save a layer recognized with the page shown turned `by` beyond a file
/// whose page has `/Rotate rotate`, then search the saved file.
fn check(rotate: i64, by: Rotation) {
    let dir = tempdir();
    let src = dir.join("src.pdf");
    blank_pdf(&src, rotate);
    let store = Store::new();
    let (primary, stamp) = store.read_file(&src, None).unwrap();
    let came = Came::Read {
        path: src.as_path().into(),
        stamp,
    };
    let mut model = Document::new(
        store,
        primary,
        sheaf_doc::store::file_name(&src),
        Some(came),
        &[],
        Box::new(FixedCount(10)),
    );
    let page = model.page(0).unwrap();
    if let Some(by) = Turn::by(by) {
        model
            .commit_ops(vec![fixture::rotate(&[page], by)], "rotate")
            .unwrap();
    }
    // Two words on a line, as the engine would report them in the frame
    // the page was shown in.
    let shown = model.geom(model.state().order()[0]);
    let hello = Box2 {
        x: 72.0,
        y: 100.0,
        w: 60.0,
        h: 12.0,
    };
    let world = Box2 {
        x: 140.0,
        y: 100.0,
        w: 60.0,
        h: 12.0,
    };
    let layer = sheaf_doc::text_layer::from_engine(
        vec![word("Hello", hello), word("World", world)],
        &shown,
        "test",
    );
    let layers = [(page, Arc::new(layer.clone()))];
    model.commit_ops(ocr(layers), "ocr").unwrap();
    let out = dir.join("ocr.pdf");
    sheaf_doc::save::rewrite_to_path(&model.snapshot(), &out).unwrap();

    // The saved page's /Rotate is the page's rotation, so the page shows
    // as it did when its words were recognized.
    let base = sheaf_doc::forms::fixture::source(std::fs::read(&out).unwrap()).geom(0);
    assert_eq!(base.rotate, shown.rotate);
    let engine = PdfiumEngine::open_bytes(sheaf_util::read_file(&out).unwrap(), None).unwrap();
    let ctx = format!("rotate {rotate}, by {by:?}");
    assert_eq!(engine.page_text_len(PageIndex(0)).unwrap(), 10, "{ctx}");
    let read = engine.page_chars(PageIndex(0)).unwrap();
    let chars = PageChars::from_engine(read.into_iter().map(|c| (c.ch, c.rect, c.angle)));
    // Find hits a word on its box in PageSpace, where it shows as it was
    // recognized; and the gap between the words became a space, so a
    // phrase matches too.
    let found = |query: &str| {
        let q = Query::literal(query, false).unwrap();
        let hits = chars.find(&q);
        assert_eq!(hits.len(), 1, "{ctx}: {query}: {hits:?}");
        hits[0].rects.iter().copied().reduce(Box2::union).unwrap()
    };
    let (hello_at, world_at) = (layer.words[0].rect, layer.words[1].rect);
    let r = found("hello");
    assert!(
        close_box(r, hello_at),
        "{ctx}: hit {r:?} vs word {hello_at:?}"
    );
    let shows = base.to_view().apply_box(r);
    assert!(
        close_box(shows, hello),
        "{ctx}: shows {shows:?} vs {hello:?}"
    );
    let r = found("Hello World");
    let both = hello_at.union(world_at);
    assert!(close_box(r, both), "{ctx}: phrase hit {r:?} vs {both:?}");

    // Selecting text: the saved page's characters read the same, upright
    // in the frame the layer was recognized in (the turn PDFium says they
    // are drawn at), and a point on "W" is just before it.
    assert_eq!(chars.text(0, chars.len()).trim(), "Hello World", "{ctx}");
    assert_eq!(chars.upright, layer.upright, "{ctx}");
    let on_w = shown.from_view().apply(Pt {
        x: world.x + 2.0,
        y: world.y + 6.0,
    });
    let at = chars.boundary_at(on_w, 0.0).expect("on the text");
    assert_eq!(chars.text(at, chars.len()).trim(), "World", "{ctx}");
    // Each word PDFium reads is the layer's, on its box in PageSpace.
    let words = chars.words();
    assert_eq!(words.len(), layer.words.len(), "{ctx}: {words:?}");
    for ((text, _, saved), w) in words.iter().zip(&layer.words) {
        assert_eq!(text, &w.text, "{ctx}");
        let unsaved = w.rect;
        assert!(
            close_box(*saved, unsaved),
            "{ctx}: {text} at {saved:?} saved, {unsaved:?} in the layer"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn pdfium_finds_saved_layer_unrotated() {
    check(0, Rotation::R0);
}

#[test]
fn pdfium_finds_saved_layer_on_rotated_source() {
    check(90, Rotation::R0);
    check(270, Rotation::R0);
}

#[test]
fn pdfium_finds_saved_layer_recognized_in_rotated_frame() {
    check(0, Rotation::R90);
    check(0, Rotation::R180);
    check(90, Rotation::R90);
}

#[test]
fn image_only_page_has_no_text() {
    let dir = tempdir();
    let src = dir.join("blank.pdf");
    blank_pdf(&src, 0);
    let engine = PdfiumEngine::open_bytes(sheaf_util::read_file(&src).unwrap(), None).unwrap();
    assert_eq!(engine.page_text_len(PageIndex(0)).unwrap(), 0);
    let _ = std::fs::remove_dir_all(&dir);
}
