//! A flattened page draws what its annotations drew (M10.5 slice 19):
//! saved with its annotations, PDFium renders them over the page; saved
//! flattened, it renders the page's layer of placed forms, and the two
//! are the same pixels. Another tool's comment, sheaf's marks (a
//! replacement's caret and struck text both, and text in a box), a check
//! box that's on and one that's off, a filled text field, and a hidden
//! comment, which neither draws. (A check box whose state names no
//! appearance draws nothing flattened, M10 bug 10, where PDFium's form
//! filler draws a box of its own: not compared.)
//!
//! Needs libpdfium (`scripts/fetch-pdfium.sh`), like the app itself.

// A test crate of its own, whose scratch files go in the temp directory
// clippy.toml bans outside tests.
#![allow(clippy::disallowed_methods)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use lopdf::{Object, Stream, dictionary};
use sheaf_doc::export::{Export, printed};
use sheaf_doc::forms::fixture::Form;
use sheaf_doc::tx::Actor;
use sheaf_doc::tx::fixture::{add, flatten, set_watermark, watermark};
use sheaf_doc::{Annot, AnnotId, AnnotKind, Box2};
use sheaf_edit::{FlattenSet, Rotation, WatermarkContent, Z};
use sheaf_image::raster::{PixelFormat, Raster};
use sheaf_render::pdfium::PdfiumEngine;
use sheaf_render::{Job, PageIndex, Renderer};

fn tempdir() -> PathBuf {
    static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let d = std::env::temp_dir().join(format!(
        "sheaf-render-flatten-{}-{}",
        std::process::id(),
        N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Page 1 of `bytes`, 612 pixels wide: a point a pixel.
fn render(bytes: Vec<u8>) -> Raster {
    let engine = PdfiumEngine::open_bytes(Arc::from(bytes), None).unwrap();
    let job = Job::Page {
        width: 612,
        rotation: Rotation::R0,
    };
    engine.render(PageIndex(0), job, PixelFormat::Rgba).unwrap()
}

/// A Letter form whose page holds a check box that's on and one that's
/// off, a filled text field, and another tool's square, printed, and a
/// hidden one.
fn annotated(path: &Path) {
    let mut f = Form::new(1);
    f.check_box("On", 0, [72.0, 600.0, 92.0, 620.0], true);
    f.check_box("Off", 0, [100.0, 600.0, 120.0, 620.0], false);
    let text = f.text("Name", 0, [150.0, 600.0, 300.0, 620.0]);
    f.fill(text, "Ada Lovelace");
    let page = f.pages[0];
    for (name, flags, x) in [("printed", 4, 72), ("hidden", 6, 300)] {
        let form = f.pdf.add_object(Stream::new(
            dictionary! {
                "Type" => "XObject", "Subtype" => "Form",
                "BBox" => vec![0.into(), 0.into(), 50.into(), 25.into()],
            },
            b"0 0 1 rg 0 0 50 25 re f 1 0 0 RG 4 w 10 10 m 40 15 l S".to_vec(),
        ));
        let square = f.pdf.add_object(dictionary! {
            "Type" => "Annot", "Subtype" => "Square", "P" => page, "F" => flags,
            "Rect" => vec![x.into(), 400.into(), (x + 100).into(), 450.into()],
            "NM" => Object::string_literal(name), "AP" => dictionary! { "N" => form },
        });
        let mut annots = f.annots(0);
        annots.push(square);
        f.set_annots(0, &annots);
    }
    std::fs::write(path, f.bytes()).unwrap();
}

/// A mark of `kind`, red, saying "Look".
fn mark(kind: AnnotKind) -> Annot {
    Annot {
        id: AnnotId(0),
        kind,
        color: [200, 0, 0],
        opacity: 1.0,
        stroke: 2.0,
        author: "Me".into(),
        contents: "Look".into(),
        modified: String::new(),
        created: String::new(),
        fill: Some([255, 230, 0]),
        dashed: false,
        cloudy: false,
        turn: Rotation::R0,
        name: String::new(),
    }
}

#[test]
fn a_flattened_page_draws_what_its_annotations_drew() {
    let dir = tempdir();
    let path = dir.join("form.pdf");
    annotated(&path);
    let mut doc = sheaf_doc::open::file(&path, None).unwrap().doc;
    let page = doc.page(0).unwrap();
    let square = mark(AnnotKind::Square {
        rect: Box2 {
            x: 300.0,
            y: 100.0,
            w: 80.0,
            h: 40.0,
        },
    });
    let words = Box2 {
        x: 100.0,
        y: 200.0,
        w: 60.0,
        h: 12.0,
    };
    let replace = mark(AnnotKind::Replace {
        rects: vec![words],
        struck: "old".into(),
        at: sheaf_doc::annot::caret_box(160.0, 200.0, 12.0),
    });
    let note = mark(AnnotKind::FreeText {
        rect: Box2 {
            x: 72.0,
            y: 250.0,
            w: 200.0,
            h: 40.0,
        },
        size: 12.0,
    });
    let ops = [square, replace, note].map(|m| add(page, &m)).to_vec();
    doc.commit_ops(ops, "marks").unwrap();
    let annotated = render(sheaf_doc::save::rewrite(&doc).unwrap());
    doc.commit_ops(vec![flatten(&[page], FlattenSet::All)], "flatten")
        .unwrap();
    let flattened = render(sheaf_doc::save::rewrite(&doc).unwrap());
    // Something was drawn: the square's blue, the mark's yellow. Each
    // pixel's channels within 2 of the other render's: a mark's `/Rect` is
    // snapped to a grid its appearance's box isn't, which PDFium maps it
    // onto.
    let drawn = |r: &Raster, rgb: [u8; 3]| r.data.chunks(4).any(|p| p[..3] == rgb);
    assert!(drawn(&flattened, [0, 0, 255]) && drawn(&flattened, [255, 230, 0]));
    assert_alike(&annotated, &flattened, "flattened");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Whether `a` and `b` are the same size and each pixel's channels are
/// within 2 of the other's (see above), `what` saying which.
fn assert_alike(a: &Raster, b: &Raster, what: &str) {
    assert_eq!((a.width, a.height), (b.width, b.height), "{what}");
    let off: Vec<usize> = (a.data.chunks(4).zip(b.data.chunks(4)))
        .enumerate()
        .filter(|(_, (a, b))| a.iter().zip(*b).any(|(a, b)| a.abs_diff(*b) > 2))
        .map(|(i, _)| i)
        .collect();
    let first = off.first().map(|i| (i % 612, i / 612));
    assert!(
        off.is_empty(),
        "{what}: {} pixels differ, the first at {first:?}",
        off.len()
    );
}

/// An annotation shows over everything on its page, a watermark over the
/// content included, and what a flatten draws of it stays there: under an
/// opaque watermark, a page flattened, exported flattened and printed with
/// markup is the same pixels as the page with its annotation.
#[test]
fn flattened_annotations_draw_over_the_watermark() {
    let dir = tempdir();
    let path = dir.join("form.pdf");
    std::fs::write(&path, Form::new(1).bytes()).unwrap();
    let mut doc = sheaf_doc::open::file(&path, None).unwrap().doc;
    let page = doc.page(0).unwrap();
    let mut spec = watermark("MMMM");
    if let WatermarkContent::Text {
        size,
        color,
        opacity,
        angle,
        ..
    } = &mut spec.content
    {
        (*size, *color, *opacity, *angle) = (150.0, [0, 0, 0], 1.0, 0.0);
    }
    let square = mark(AnnotKind::Square {
        rect: Box2 {
            x: 156.0,
            y: 346.0,
            w: 300.0,
            h: 100.0,
        },
    });
    let ops = vec![set_watermark(spec, Z::Over, None), add(page, &square)];
    doc.commit_ops(ops, "marked").unwrap();
    let annotated = render(sheaf_doc::save::rewrite(&doc).unwrap());
    let from = doc.export_from(Actor::User);
    let print = render(printed(from, &[page], true, &in_progress::NoProgress).unwrap());
    let exported = render(doc.export_bytes(Export::flattened));
    doc.commit_ops(vec![flatten(&[page], FlattenSet::All)], "flatten")
        .unwrap();
    let flattened = render(sheaf_doc::save::rewrite(&doc).unwrap());
    // The watermark's black and the square's yellow both show.
    let drawn = |r: &Raster, rgb: [u8; 3]| r.data.chunks(4).any(|p| p[..3] == rgb);
    assert!(drawn(&annotated, [0, 0, 0]) && drawn(&annotated, [255, 230, 0]));
    assert_alike(&annotated, &flattened, "flattened");
    assert_alike(&annotated, &exported, "exported flattened");
    assert_alike(&annotated, &print, "printed with markup");
    let _ = std::fs::remove_dir_all(&dir);
}
