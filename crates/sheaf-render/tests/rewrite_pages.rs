//! A Rewrite places every page in a new tree, so a page that took its
//! `/Rotate` and `/CropBox` from a node above it writes them on itself
//! (M10.5 §11.3): a page under a node turned a quarter and cropped, a page
//! from another file under such a node, and a duplicate of one show in
//! PDFium as they did in their own files, size, turn, crop and all.
//!
//! Needs libpdfium (`scripts/fetch-pdfium.sh`), like the app itself.

use std::path::Path;
use std::sync::Arc;

use in_progress::NoProgress;
use lopdf::{Dictionary, Document as Pdf, Object, Stream, dictionary};
use sheaf_doc::materialize::{
    Profile, SaveInput, SavePlan, SaveTarget, Strategy, Why, materialize,
};
use sheaf_doc::store::BlobKind;
use sheaf_doc::tx::fixture::{duplicate, insert};
use sheaf_doc::{Document, FixedCount, Src, Store};
use sheaf_edit::Rotation;
use sheaf_image::raster::{PixelFormat, Raster};
use sheaf_render::PageIndex;
use sheaf_render::pdfium::PdfiumEngine;
use sheaf_render::{Job, Renderer};

/// A box as written.
fn rect(r: [i64; 4]) -> Object {
    r.map(Object::from).to_vec().into()
}

/// A file whose root gives `media`, with a node under it turned 90° and
/// cropped to `crop`, holding a page per entry of `marks` (each drawing a
/// blue block at its bottom left and a red one where its entry says);
/// and after the node, a page of the root's own.
fn file(media: [i64; 4], crop: [i64; 4], marks: &[[i64; 2]]) -> Vec<u8> {
    let mut pdf = Pdf::with_version("1.7");
    let root = pdf.new_object_id();
    let node = pdf.new_object_id();
    let page = |pdf: &mut Pdf, parent, [x, y]: [i64; 2]| {
        let content = format!(
            "0 0 1 rg {} {} 60 40 re f 1 0 0 rg {x} {y} 30 30 re f",
            crop[0] + 10,
            crop[1] + 10
        );
        let content = pdf.add_object(Stream::new(dictionary! {}, content.into_bytes()));
        let page = dictionary! { "Type" => "Page", "Parent" => parent, "Contents" => content };
        Object::Reference(pdf.add_object(page))
    };
    let under: Vec<Object> = marks.iter().map(|&m| page(&mut pdf, node, m)).collect();
    let count = under.len() as i64;
    pdf.objects.insert(
        node,
        Object::Dictionary(dictionary! {
            "Type" => "Pages", "Parent" => root, "Kids" => under, "Count" => count,
            "Rotate" => 90, "CropBox" => rect(crop),
        }),
    );
    let own = page(&mut pdf, root, [300, 300]);
    pdf.objects.insert(
        root,
        Object::Dictionary(dictionary! {
            "Type" => "Pages", "Kids" => vec![node.into(), own], "Count" => count + 1,
            "MediaBox" => rect(media), "Resources" => Dictionary::new(),
        }),
    );
    let catalog = pdf.add_object(dictionary! { "Type" => "Catalog", "Pages" => root });
    pdf.trailer.set("Root", catalog);
    let mut bytes = Vec::new();
    pdf.save_to(&mut bytes).unwrap();
    bytes
}

/// `bytes`, read into `store` as `name`.
fn source(store: &Store, bytes: &[u8], name: &str) -> Src {
    (store.open(Arc::from(bytes), BlobKind::file(Path::new(name), None))).unwrap()
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

#[test]
fn pages_under_a_turned_cropped_node_keep_how_they_show() {
    let primary = file(
        [0, 0, 612, 792],
        [36, 72, 576, 720],
        &[[400, 500], [100, 600]],
    );
    let other = file([0, 0, 595, 842], [50, 100, 500, 700], &[[300, 200]]);
    let store = Store::new();
    let a = source(&store, &primary, "a.pdf");
    let mut doc = Document::new(
        store,
        a,
        "a.pdf".into(),
        None,
        &[],
        Box::new(FixedCount(10)),
    );
    let b = source(doc.store(), &other, "b.pdf");
    let second = doc.page(1).unwrap();
    doc.commit_ops(vec![insert(&b, [0], "b.pdf", Some(second))], "insert")
        .unwrap();
    doc.commit_ops(vec![duplicate(&[second])], "dup").unwrap();
    let plan = SavePlan {
        strategy: Strategy::Rewrite {
            why: vec![Why::OtherTarget, Why::NoSaveRecord],
        },
        asks: Vec::new(),
        transforms: Vec::new(),
        notes: Vec::new(),
    };
    let target = SaveTarget {
        path: Path::new("out.pdf").into(),
        same_file: false,
        profile: Profile::default(),
        stamp: None,
    };
    let input = SaveInput::detached(&doc.snapshot());
    let out = materialize(&input, &plan, target, &NoProgress).unwrap();

    let a = PdfiumEngine::open_bytes(Arc::from(primary), None).unwrap();
    let b = PdfiumEngine::open_bytes(Arc::from(other), None).unwrap();
    let saved = PdfiumEngine::open_bytes(Arc::from(out.bytes), None).unwrap();
    // A's first page, B's turned page, A's second and its duplicate, and
    // A's page of the root's own.
    let want = [(&a, 0), (&b, 0), (&a, 1), (&a, 1), (&a, 2)];
    assert_eq!(saved.page_count(), want.len() as u32);
    for (i, (engine, page)) in want.into_iter().enumerate() {
        let (size, raster) = shown(engine, page);
        let (now, again) = shown(&saved, i as u32);
        assert_eq!(now, size, "page {i}");
        assert_eq!(
            (again.width, again.height),
            (raster.width, raster.height),
            "page {i}"
        );
        assert!(again.data == raster.data, "page {i} shows otherwise");
    }
    // Turned and cropped, A's pages show 648 wide and 540 tall.
    assert_eq!(shown(&saved, 0).0, (648.0, 540.0));
}
