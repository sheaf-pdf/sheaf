//! Pages holding recognized text sheaf wrote, for tests here and in other
//! crates (the `fixtures` feature). The OCR layer an older sheaf wrote
//! before M10.5 slice 18 ([`legacy_page`]), its bytes frozen as its
//! installer wrote them: a `q` stream before the page's content and the
//! layer after it, which starts with the `Q` that ends it, both marked
//! `/SheafOcrLayer`, its text in a glyphless font named `/SheafOcr`. An
//! OCR block of sheaf's, frozen as `text_layer::block` writes it
//! ([`OCR_BLOCK`]), a stream of its own after the page's isolated content
//! ([`block_page`]), and the same block merged into the page's one stream
//! by another tool ([`merged_page`]). Each page's own content is
//! [`BASE`], which leaves a `q` open, and its recognized text says
//! [`WORDS`]. And pages drawing contents of a test's own
//! ([`pages_drawing`]).

use lopdf::{Dictionary, Document as Pdf, Object, ObjectId, Stream, dictionary};
use sheaf_edit::OcrWord;

use super::TextLayer;
use crate::annot::Box2;
use crate::content::{Font, Glyphless};
use crate::geom::Geom;
use crate::serialize::built::ResObjects;

/// The page's own content: text in Helvetica named `/F1`, moved by a
/// `cm` in a `q` it leaves open, as a careless writer leaves it.
pub const BASE: &[u8] = b"q 1 0 0 1 20 20 cm\nBT /F1 12 Tf 72 700 Td (Body text) Tj ET\n";

/// What the page's own content says.
pub const BASE_TEXT: &str = "Body text";

/// The words the recognized text says.
pub const WORDS: [&str; 2] = ["Saved", "words"];

/// The words as the glyphless font encodes them: UTF-16 code units, in
/// hex, as a layer writes them.
pub const WORDS_HEX: [&str; 2] = ["<00530061007600650064>", "<0077006F007200640073>"];

/// The recognized text the fixtures' layers say: [`WORDS`], on one line
/// of an upright Letter page.
pub fn layer() -> TextLayer {
    let word = |text: &str, x: f32| OcrWord {
        text: text.into(),
        rect: Box2 {
            x,
            y: 100.0,
            w: 40.0,
            h: 12.0,
        },
        confidence: 0.9,
    };
    let words = vec![word(WORDS[0], 72.0), word(WORDS[1], 120.0)];
    super::from_engine(words, &Geom::read(None, None, 0), "test")
}

/// The layer an older sheaf wrote of [`layer`], as its installer wrote it
/// after the page's content: it ends the `q` before the content, and its
/// text is invisible, in the glyphless font named `/SheafOcr`.
pub const LEGACY_LAYER: &[u8] = b"Q\nBT\n3 Tr\n/SheafOcr 12 Tf\n1 0 0 1 72 680 Tm\n133.3333 Tz\n\
    <00530061007600650064> Tj\n1 0 0 1 112 680 Tm\n133.3333 Tz\n<0020> Tj\n\
    1 0 0 1 120 680 Tm\n133.3333 Tz\n<0077006F007200640073> Tj\nET\n";

/// The marker on each of the legacy layer's two streams.
pub const LEGACY_MARKER: &[u8] = b"SheafOcrLayer";

/// The id of [`block_page`]'s and [`merged_page`]'s block.
pub const BLOCK_ID: u64 = 0x2a;

/// An OCR block of [`layer`], block [`BLOCK_ID`]'s, as `text_layer::block`
/// writes it, frozen: its invisible text in a `q` of its own, in the
/// glyphless font named [`OCR_FONT`], marked `/ShOCR` with the list named
/// [`OCR_LIST`], which says its block (`/SHEF_OcrBlock (b2a)`).
pub const OCR_BLOCK: &[u8] = b"/ShOCR /ShX256IW BDC\nq\nBT\n3 Tr\n/ShOANHIY 12 Tf\n\
    1 0 0 1 72 680 Tm\n133.3333 Tz\n<00530061007600650064> Tj\n1 0 0 1 112 680 Tm\n133.3333 Tz\n\
    <0020> Tj\n1 0 0 1 120 680 Tm\n133.3333 Tz\n<0077006F007200640073> Tj\nET\nQ\nEMC\n";

/// The name [`OCR_BLOCK`] draws its font by.
pub const OCR_FONT: &[u8] = b"ShOANHIY";

/// The name [`OCR_BLOCK`] names its property list by.
pub const OCR_LIST: &[u8] = b"ShX256IW";

/// A one-page document whose page draws [`BASE`] and then the layer an
/// older sheaf wrote of [`layer`], as it wrote it: `/Contents [q, base,
/// layer]`, the `q` and the layer marked [`LEGACY_MARKER`] and compressed,
/// and its resources a dictionary of its own binding the glyphless font
/// as `/SheafOcr` beside Helvetica as `/F1`.
pub fn legacy_page() -> Pdf {
    let mut pdf = Pdf::with_version("1.7");
    let font = glyphless(&mut pdf);
    let marked = || dictionary! { LEGACY_MARKER => true };
    let q = compressed(&mut pdf, marked(), b"q\n");
    let base = pdf.add_object(Stream::new(dictionary! {}, BASE.to_vec()));
    let layer = compressed(&mut pdf, marked(), LEGACY_LAYER);
    let fonts = dictionary! { "F1" => helvetica(&mut pdf), "SheafOcr" => font };
    let resources = dictionary! { "Font" => fonts };
    one_page(pdf, vec![q.into(), base.into(), layer.into()], resources)
}

/// A one-page document whose page draws [`BASE`], isolated by the `q`
/// and the two `Q`s its balance asks for, each a stream marked
/// `/SHEF_Block << /K /Isolate >>`, and then [`OCR_BLOCK`], a stream
/// marked `/SHEF_Block << /K /Ocr /B (b2a) >>`.
pub fn block_page() -> Pdf {
    let mut pdf = Pdf::with_version("1.7");
    let isolate = || dictionary! { "SHEF_Block" => dictionary! { "K" => "Isolate" } };
    let open = pdf.add_object(Stream::new(isolate(), b"q\n".to_vec()));
    let base = pdf.add_object(Stream::new(dictionary! {}, BASE.to_vec()));
    let close = pdf.add_object(Stream::new(isolate(), b"Q\nQ\n".to_vec()));
    let block = dictionary! {
        "SHEF_Block" => dictionary! { "K" => "Ocr", "B" => block_id() },
    };
    let ocr = pdf.add_object(Stream::new(block, OCR_BLOCK.to_vec()));
    let contents = vec![open.into(), base.into(), close.into(), ocr.into()];
    let resources = block_resources(&mut pdf);
    one_page(pdf, contents, resources)
}

/// A one-page document whose page draws [`BASE`] and [`OCR_BLOCK`] in
/// one stream, as another tool that merged its streams writes it, its
/// list and font named as [`block_page`]'s.
pub fn merged_page() -> Pdf {
    let mut pdf = Pdf::with_version("1.7");
    let merged = pdf.add_object(Stream::new(dictionary! {}, [BASE, OCR_BLOCK].concat()));
    let resources = block_resources(&mut pdf);
    one_page(pdf, vec![merged.into()], resources)
}

/// [`BLOCK_ID`] as a block writes it.
fn block_id() -> Object {
    Object::string_literal(format!("b{BLOCK_ID:x}"))
}

/// The resources [`OCR_BLOCK`] draws with, made in `pdf`: Helvetica as
/// `/F1`, the glyphless font as [`OCR_FONT`] and the block's list as
/// [`OCR_LIST`].
fn block_resources(pdf: &mut Pdf) -> Dictionary {
    let fonts = dictionary! { "F1" => helvetica(pdf), OCR_FONT => glyphless(pdf) };
    let list = dictionary! { "SHEF_OcrBlock" => block_id() };
    dictionary! { "Font" => fonts, "Properties" => dictionary! { OCR_LIST => list } }
}

/// The glyphless font's objects, made in `pdf`: its Type0's number.
fn glyphless(pdf: &mut Pdf) -> ObjectId {
    ResObjects::default().object(pdf, &Glyphless.resource().into())
}

/// Helvetica, made in `pdf`.
fn helvetica(pdf: &mut Pdf) -> ObjectId {
    pdf.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
    })
}

/// A stream of `content` with `dict`, compressed as a save compressed it,
/// made in `pdf`.
fn compressed(pdf: &mut Pdf, dict: Dictionary, content: &[u8]) -> ObjectId {
    let mut stream = Stream::new(dict, content.to_vec());
    let _ = stream.compress();
    pdf.add_object(stream)
}

/// A document of Letter pages, each drawing one of `contents`, with no
/// resources.
pub fn pages_drawing(contents: &[&[u8]]) -> Pdf {
    let mut pdf = Pdf::with_version("1.7");
    let pages = (contents.iter())
        .map(|&c| {
            let content = pdf.add_object(Stream::new(dictionary! {}, c.to_vec()));
            dictionary! { "Contents" => content }
        })
        .collect();
    with_pages(pdf, pages)
}

/// `pdf` with one Letter page drawing `contents` with `resources`.
fn one_page(pdf: Pdf, contents: Vec<Object>, resources: Dictionary) -> Pdf {
    let page = dictionary! { "Contents" => contents, "Resources" => resources };
    with_pages(pdf, vec![page])
}

/// `pdf` with a Letter page of each of `pages`, page dictionaries but for
/// their type and parent.
fn with_pages(mut pdf: Pdf, pages: Vec<Dictionary>) -> Pdf {
    let parent = pdf.new_object_id();
    let kids: Vec<Object> = (pages.into_iter())
        .map(|page| {
            let mut dict = dictionary! { "Type" => "Page", "Parent" => parent };
            dict.extend(&page);
            pdf.add_object(dict).into()
        })
        .collect();
    let count = kids.len() as i64;
    pdf.objects.insert(
        parent,
        Object::Dictionary(dictionary! {
            "Type" => "Pages", "Kids" => kids, "Count" => count,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        }),
    );
    let catalog = pdf.add_object(dictionary! { "Type" => "Catalog", "Pages" => parent });
    pdf.trailer.set("Root", catalog);
    pdf
}
