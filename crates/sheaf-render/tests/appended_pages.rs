//! An Append splices the pages into the saved file's own tree (M10.5
//! §11.6), so a page moved under a node that turns and crops its pages,
//! and one moved out from under it, carry what they inherited on
//! themselves: after a file written with a cross-reference table and one
//! written with a stream, each page shows in PDFium as it did, size, turn,
//! crop and all, and the file reads back sound. Three Appends in a row
//! reopen in PDFium and `LazyPdf` with every page as saved.
//!
//! Needs libpdfium (`scripts/fetch-pdfium.sh`), like the app itself.

use std::path::PathBuf;
use std::sync::Arc;

use lopdf::xref::XrefType;
use lopdf::{Dictionary, Document as Pdf, LazyPdf, Object, Stream, dictionary};
use sheaf_core::{PageIndex, Rotation};
use sheaf_doc::Edit;
use sheaf_image::raster::{PixelFormat, Raster};
use sheaf_render::pdfium::PdfiumEngine;
use sheaf_render::{Job, Renderer};

/// A box as written.
fn rect(r: [i64; 4]) -> Object {
    r.map(Object::from).to_vec().into()
}

/// A file whose root boxes its pages, with a node under it turned 90° and
/// cropped holding two pages, then a page of the root's own; each page
/// draws a blue block at its bottom left and a red one where it says.
/// Written by lopdf in `kind`.
fn file(kind: XrefType) -> Vec<u8> {
    let mut pdf = Pdf::with_version("1.7");
    let root = pdf.new_object_id();
    let node = pdf.new_object_id();
    let page = |pdf: &mut Pdf, parent, [x, y]: [i64; 2]| {
        let content = format!("0 0 1 rg 46 82 60 40 re f 1 0 0 rg {x} {y} 30 30 re f");
        let content = pdf.add_object(Stream::new(dictionary! {}, content.into_bytes()));
        let page = dictionary! { "Type" => "Page", "Parent" => parent, "Contents" => content };
        Object::Reference(pdf.add_object(page))
    };
    let under = vec![
        page(&mut pdf, node, [400, 500]),
        page(&mut pdf, node, [100, 600]),
    ];
    pdf.objects.insert(
        node,
        Object::Dictionary(dictionary! {
            "Type" => "Pages", "Parent" => root, "Kids" => under, "Count" => 2,
            "Rotate" => 90, "CropBox" => rect([36, 72, 576, 720]),
        }),
    );
    let own = page(&mut pdf, root, [300, 300]);
    pdf.objects.insert(
        root,
        Object::Dictionary(dictionary! {
            "Type" => "Pages", "Kids" => vec![node.into(), own], "Count" => 3,
            "MediaBox" => rect([0, 0, 612, 792]), "Resources" => Dictionary::new(),
        }),
    );
    let catalog = pdf.add_object(dictionary! { "Type" => "Catalog", "Pages" => root });
    pdf.trailer.set("Root", catalog);
    pdf.reference_table.cross_reference_type = kind;
    let mut bytes = Vec::new();
    pdf.save_to(&mut bytes).unwrap();
    bytes
}

/// Page `i` of `engine`: its size as shown, and a render 150 pixels wide.
fn shown(engine: &PdfiumEngine, i: u32) -> ((f32, f32), Raster) {
    let size = engine.page_size(PageIndex(i)).unwrap();
    let job = Job::Page {
        width: 150,
        rotation: Rotation::R0,
    };
    let raster = engine.render(PageIndex(i), job, PixelFormat::Rgba).unwrap();
    ((size.width, size.height), raster)
}

/// A scratch file of this test's own, `name`.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("sheaf-appended-pages-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

#[test]
fn pages_moved_across_nodes_show_as_they_did() {
    for (kind, name) in [
        (XrefType::CrossReferenceTable, "table.pdf"),
        (XrefType::CrossReferenceStream, "stream.pdf"),
    ] {
        let bytes = file(kind);
        let path = scratch(name);
        std::fs::write(&path, &bytes).unwrap();
        let mut doc = sheaf_doc::open::file(&path, None).unwrap().doc;
        let pages: Vec<_> = (0..3).map(|i| doc.page(i).unwrap()).collect();
        // The root's own page first, under the turned node; that node's
        // second page last, under the root.
        let first = Edit::MovePages {
            ids: vec![pages[2]],
            before: Some(pages[0]),
        };
        doc.apply(first, "move").unwrap();
        let last = Edit::MovePages {
            ids: vec![pages[1]],
            before: None,
        };
        doc.apply(last, "move").unwrap();
        sheaf_doc::materialize::save_appended(&mut doc, &path).unwrap();
        let appended = std::fs::read(&path).unwrap();
        assert!(appended.starts_with(&bytes), "{name}");
        let lazy = LazyPdf::open(
            Arc::<[u8]>::from(&appended[..]),
            None,
            &sheaf_doc::source::Limits::DEFAULT.reader,
        )
        .unwrap();
        assert!(lazy.base_sound(), "{name}");
        assert_eq!(lazy.xref().sections, 2, "{name}");

        let was = PdfiumEngine::open_bytes(Arc::from(bytes), None).unwrap();
        let now = PdfiumEngine::open_bytes(Arc::from(appended), None).unwrap();
        assert_eq!(now.page_count(), 3, "{name}");
        for (i, page) in [2, 0, 1].into_iter().enumerate() {
            let (size, raster) = shown(&was, page);
            let (at, again) = shown(&now, i as u32);
            assert_eq!(at, size, "{name}: page {i}");
            assert_eq!(
                (again.width, again.height),
                (raster.width, raster.height),
                "{name}: page {i}"
            );
            assert!(
                again.data == raster.data,
                "{name}: page {i} shows otherwise"
            );
        }
        // The turned, cropped pages show 648 wide and 540 tall; the root's
        // own, 612 by 792.
        assert_eq!(shown(&now, 0).0, (612.0, 792.0), "{name}");
        assert_eq!(shown(&now, 1).0, (648.0, 540.0), "{name}");
    }
}

/// Three Appends in a row, each turning a page and moving another, each
/// after the last: the file reopens, through `LazyPdf` and PDFium, with
/// every page there, in the order saved, turned as saved.
#[test]
fn three_appends_in_a_row_reopen() {
    for (kind, name) in [
        (XrefType::CrossReferenceTable, "three-table.pdf"),
        (XrefType::CrossReferenceStream, "three-stream.pdf"),
    ] {
        let bytes = file(kind);
        let path = scratch(name);
        std::fs::write(&path, &bytes).unwrap();
        let mut doc = sheaf_doc::open::file(&path, None).unwrap().doc;
        let pages: Vec<_> = (0..3).map(|i| doc.page(i).unwrap()).collect();
        for &page in &pages {
            let turn = Edit::RotatePages {
                ids: vec![page],
                by: Rotation::R90,
            };
            doc.apply(turn, "turn").unwrap();
            let last = Edit::MovePages {
                ids: vec![doc.page(0).unwrap()],
                before: None,
            };
            doc.apply(last, "move").unwrap();
            sheaf_doc::materialize::save_appended(&mut doc, &path).unwrap();
        }
        let appended = std::fs::read(&path).unwrap();
        assert!(appended.starts_with(&bytes), "{name}");
        let lazy = LazyPdf::open(
            Arc::<[u8]>::from(&appended[..]),
            None,
            &sheaf_doc::source::Limits::DEFAULT.reader,
        )
        .unwrap();
        assert!(lazy.base_sound(), "{name}");
        assert_eq!(lazy.xref().sections, 4, "{name}");
        let reopened = sheaf_doc::open::file(&path, None).unwrap().doc;
        let st = reopened.state();
        assert_eq!(st.order().len(), 3, "{name}");
        let engine = PdfiumEngine::open_bytes(Arc::from(appended), None).unwrap();
        assert_eq!(engine.page_count(), 3, "{name}");
        for i in 0..3 {
            let (at, was) = (reopened.page(i).unwrap(), doc.page(i).unwrap());
            assert_eq!(reopened.geom(at), doc.geom(was), "{name}: page {i}");
            let (size, raster) = shown(&engine, i as u32);
            let want = doc.page_size(was);
            assert_eq!(size, (want.width, want.height), "{name}: page {i}");
            assert!(
                raster.data.iter().any(|&b| b != 255),
                "{name}: page {i} draws"
            );
        }
    }
}
