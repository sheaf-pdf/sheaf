//! A clean copy holds none of sheaf's private data (M10.5 §15), and what
//! sheaf drew as blocks of its own draws in it as in the file, pixel for
//! pixel, its text read alike: sheaf's OCR blocks, in streams of their own
//! or merged into the page's; and a generator's watermark whose list is
//! named in the page's `/Properties`.
//!
//! Needs libpdfium (`scripts/fetch-pdfium.sh`), like the app itself.

mod common;

use std::path::Path;

use common::{open, scratch};
use lopdf::{Dictionary, Document as Pdf, Object, Stream, dictionary};
use sheaf_doc::export::Export;
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

/// A watermark another file's generator drew, merged into the page's
/// content, whose list, marked as the generator's, is named in the page's
/// `/Properties`: the list in the copy says its kind alone.
#[test]
fn a_generator_s_named_list() {
    let dir = scratch("private-named");
    let mut pdf = Pdf::with_version("1.7");
    let list = dictionary! {
        "Type" => "Pagination", "Subtype" => "Watermark",
        "SHEF_Gen" => Object::string_literal("g1a"),
    };
    let resources = dictionary! { "Properties" => dictionary! { "P0" => list } };
    let mark: &[u8] = b"/Artifact /P0 BDC q 1 0 0 rg 100 100 200 50 re f Q EMC";
    let page = one_page(&mut pdf, &[BODY, mark], resources);
    let clean = cleaned(&dir, "named.pdf", &finished(pdf, page, Dictionary::new()));
    assert!(!common::holds(&clean, b"SHEF_"));
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
