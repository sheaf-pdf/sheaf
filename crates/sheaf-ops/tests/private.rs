//! A clean copy holds none of sheaf's private data (M10.5 §15), and what
//! an older sheaf drew, or sheaf drew as blocks of its own, draws in it
//! as in the file, pixel for pixel, its text read alike: a header in the
//! font it named `/SheafHF`, its settings in the catalog; the recognized
//! text an older sheaf wrote in a stream marked `/SheafOcrLayer`, in the
//! font it named `/SheafOcr`; sheaf's own OCR blocks, in streams of their
//! own or merged into the page's; a watermark whose list is named in the
//! page's `/Properties`; and what an older flatten drew as `/SheafFlat1`.
//! (An older sheaf's watermark and background are watermark.rs's.)
//!
//! Needs libpdfium (`scripts/fetch-pdfium.sh`), like the app itself.

mod common;

use std::path::Path;

use common::{open, scratch};
use lopdf::{Dictionary, Document as Pdf, Object, Stream, dictionary};
use sheaf_doc::export::Export;
use sheaf_doc::generate::fixture as older;
use sheaf_doc::text_layer::fixture as saved;
use sheaf_image::raster::{PixelFormat, Raster};
use sheaf_render::pdfium::PdfiumEngine;
use sheaf_render::{PageIndex, Renderer};

/// `bytes` on PDFium, its first page drawn 612 pixels wide.
fn drawn(bytes: &[u8]) -> Raster {
    let engine = PdfiumEngine::open_bytes(bytes.to_vec().into(), None).unwrap();
    engine
        .render_page(PageIndex(0), 612, PixelFormat::Rgba)
        .unwrap()
}

/// PDFium's text of the first page of `bytes`, each character with its
/// box.
fn read(bytes: &[u8]) -> Vec<(char, Option<sheaf_doc::Box2>)> {
    let engine = PdfiumEngine::open_bytes(bytes.to_vec().into(), None).unwrap();
    let chars = engine.page_chars(PageIndex(0)).unwrap();
    chars.into_iter().map(|c| (c.ch, c.rect)).collect()
}

/// `pdf`'s bytes.
fn bytes_of(pdf: &mut Pdf) -> Vec<u8> {
    let mut bytes = Vec::new();
    pdf.save_to(&mut bytes).unwrap();
    bytes
}

/// A document of one Letter page drawing `contents`, each a stream, with
/// `resources`, Helvetica as `/F1` added to them.
fn one_page(pdf: &mut Pdf, contents: &[&[u8]], mut resources: Dictionary) -> Dictionary {
    let font = pdf.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
    });
    let mut fonts = (resources.get(b"Font").and_then(Object::as_dict).cloned()).unwrap_or_default();
    fonts.set("F1", font);
    resources.set("Font", fonts);
    let contents: Vec<Object> = (contents.iter())
        .map(|c| {
            pdf.add_object(Stream::new(dictionary! {}, c.to_vec()))
                .into()
        })
        .collect();
    dictionary! {
        "Type" => "Page", "Contents" => contents, "Resources" => resources,
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
    }
}

/// `pdf` with `page` its one page, its catalog's keys `catalog`'s.
fn finished(mut pdf: Pdf, mut page: Dictionary, catalog: Dictionary) -> Vec<u8> {
    let pages = pdf.new_object_id();
    page.set("Parent", pages);
    let page = pdf.add_object(page);
    pdf.objects.insert(
        pages,
        dictionary! { "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1 }.into(),
    );
    let mut root = dictionary! { "Type" => "Catalog", "Pages" => pages };
    for (key, value) in catalog.iter() {
        root.set(key.clone(), value.clone());
    }
    let root = pdf.add_object(root);
    pdf.trailer.set("Root", root);
    bytes_of(&mut pdf)
}

/// The page's own text.
const BODY: &[u8] = b"BT /F1 24 Tf 72 600 Td (body) Tj ET";

/// `file`'s clean copy, made in `dir`, holds none of sheaf's private data
/// and draws its first page as `file` does, pixel for pixel, with the same
/// text: the copy.
fn cleaned(dir: &Path, name: &str, file: &[u8]) -> Vec<u8> {
    let path = dir.join(name);
    std::fs::write(&path, file).unwrap();
    let doc = open(&path);
    let out = dir.join(format!("clean-{name}"));
    let clean = common::exported_to(&doc, Export::clean_copy, &out);
    let pdf = Pdf::load_mem(&clean).unwrap();
    assert!(!sheaf_doc::private::found(&Pdf::load_mem(file).unwrap()).is_empty());
    assert_eq!(sheaf_doc::private::found(&pdf), [], "{name}");
    assert!(
        drawn(file).data == drawn(&clean).data,
        "{name} draws as it was"
    );
    assert_eq!(read(file), read(&clean), "{name} reads as it did");
    clean
}

/// A header an older sheaf drew, in Helvetica named `/SheafHF`, its list
/// marked `/SheafStamp` and its settings in the catalog's
/// `/SheafHeaderFooter`: the copy draws it, in a font named anew.
#[test]
fn an_older_sheaf_s_header() {
    let dir = scratch("private-header");
    let mut pdf = Pdf::with_version("1.7");
    let mut page = one_page(&mut pdf, &[BODY], Dictionary::new());
    older::legacy_header(&mut pdf, &mut page);
    let settings = Object::string_literal(older::HEADER_SETTINGS);
    let file = finished(pdf, page, dictionary! { "SheafHeaderFooter" => settings });
    let clean = cleaned(&dir, "header.pdf", &file);
    assert!(common::holds(&clean, b"(Draft) Tj"));
    let _ = std::fs::remove_dir_all(&dir);
}

/// The recognized text an older sheaf wrote after the page's content,
/// with the `q` before it, both marked `/SheafOcrLayer`, in the glyphless
/// font named `/SheafOcr`: the copy reads its words.
#[test]
fn an_older_sheaf_s_recognized_text() {
    let dir = scratch("private-ocr");
    let clean = cleaned(&dir, "ocr.pdf", &bytes_of(&mut saved::legacy_page()));
    let text: String = read(&clean).into_iter().map(|(c, _)| c).collect();
    assert!(text.contains(saved::WORDS[0]), "{text}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// sheaf's recognized text, a block in a stream of its own after the
/// page's isolated content, and the same block merged into the page's one
/// stream by another tool: its words read in the copy, which marks no
/// stream and names no list as sheaf's.
#[test]
fn sheaf_s_blocks() {
    let dir = scratch("private-blocks");
    let pages = [
        ("block.pdf", saved::block_page()),
        ("merged.pdf", saved::merged_page()),
    ];
    for (name, mut pdf) in pages {
        let clean = cleaned(&dir, name, &bytes_of(&mut pdf));
        let text: String = read(&clean).into_iter().map(|(c, _)| c).collect();
        assert!(text.contains(saved::WORDS[1]), "{name}: {text}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// A watermark an older sheaf drew whose list is named in the page's
/// `/Properties`, keeping its settings and an image's path: the list in
/// the copy says its kind alone.
#[test]
fn an_older_sheaf_s_named_list() {
    let dir = scratch("private-named");
    let mut pdf = Pdf::with_version("1.7");
    let list = dictionary! {
        "Type" => "Pagination", "Subtype" => "Watermark", "SheafStamp" => true,
        "SheafSettings" => Object::string_literal(r#"{"path":"/home/someone/logo.png"}"#),
    };
    let resources = dictionary! { "Properties" => dictionary! { "P0" => list } };
    let mark: &[u8] = b"/Artifact /P0 BDC q 1 0 0 rg 100 100 200 50 re f Q EMC";
    let page = one_page(&mut pdf, &[BODY, mark], resources);
    let clean = cleaned(&dir, "named.pdf", &finished(pdf, page, Dictionary::new()));
    assert!(!common::holds(&clean, b"/home/someone"));
    let pdf = Pdf::load_mem(&clean).unwrap();
    let page = pdf
        .get_dictionary(*pdf.get_pages().get(&1).unwrap())
        .unwrap();
    let res = pdf.dereference(page.get(b"Resources").unwrap()).unwrap().1;
    let lists = res.as_dict().unwrap().get(b"Properties").unwrap();
    let lists = pdf.dereference(lists).unwrap().1.as_dict().unwrap();
    let p0 = pdf.dereference(lists.get(b"P0").unwrap()).unwrap().1;
    let want = dictionary! { "Type" => "Pagination", "Subtype" => "Watermark" };
    assert_eq!(p0.as_dict().unwrap(), &want);
    let _ = std::fs::remove_dir_all(&dir);
}

/// What an older flatten drew: an annotation's form, named `/SheafFlat1`
/// in the page's resources, drawn after its content: the copy draws it
/// by a name of its own.
#[test]
fn an_older_flatten_s_form() {
    let dir = scratch("private-flat");
    let mut pdf = Pdf::with_version("1.7");
    let form = pdf.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject", "Subtype" => "Form",
            "BBox" => vec![0.into(), 0.into(), 100.into(), 50.into()],
        },
        b"0 0 1 rg 0 0 100 50 re f".to_vec(),
    ));
    let resources = dictionary! { "XObject" => dictionary! { "SheafFlat1" => form } };
    let content: &[u8] = b"q\nBT /F1 24 Tf 72 600 Td (body) Tj ET\nQ\n \
        q 2.0000 0 0 2.0000 100.000 300.000 cm /SheafFlat1 Do Q";
    let page = one_page(&mut pdf, &[content], resources);
    let clean = cleaned(&dir, "flat.pdf", &finished(pdf, page, Dictionary::new()));
    assert!(!common::holds(&clean, b"SheafFlat"));
    let _ = std::fs::remove_dir_all(&dir);
}
