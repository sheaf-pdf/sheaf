//! Watermarks as PDFium draws them from a saved file (M10.5 slice 16),
//! saved both ways: text in its face's encoding, upright on a turned page,
//! over a page whose content leaves its graphics state changed, over an
//! OCR layer sheaf wrote before, and on a form's pages. Acrobat's
//! watermark replaced by one of the document's, or taken off, leaves the
//! rest of the page as it drew, and corpus files with others' watermarks
//! keep the author's. A clean copy of another file's generator's
//! watermark draws it as it was, without what marks it as sheaf's.
//!
//! Needs libpdfium (`scripts/fetch-pdfium.sh`), like the app itself.

mod common;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use common::{
    HALF_A_ROW, blue_in, clean, corpus_form, holds, open, scratch, two_page_form, values, write,
    written,
};
use lopdf::{Document as Pdf, Stream, dictionary};
use sheaf_doc::content::{PagOrigin, PaginationKind};
use sheaf_doc::export::Export;
use sheaf_doc::materialize::How;
use sheaf_doc::text_layer::fixture as saved;
use sheaf_doc::tx::fixture as fx;
use sheaf_doc::{OcrWord, TextLayer};
use sheaf_edit::{FontChoice, GridPos, Turn, WatermarkContent, WatermarkSpec, Z};
use sheaf_image::raster::{PixelFormat, Raster};
use sheaf_render::pdfium::PdfiumEngine;
use sheaf_render::{PageIndex, Renderer};

/// A file of one Letter page for each of `contents`, each its content,
/// Helvetica its `/F1`.
fn pages_pdf(dir: &Path, name: &str, contents: &[&[u8]]) -> PathBuf {
    let mut pdf = Pdf::with_version("1.7");
    let pages = pdf.new_object_id();
    let font = dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica" };
    let resources = pdf.add_object(dictionary! { "Font" => dictionary! { "F1" => font } });
    let kids: Vec<lopdf::Object> = (contents.iter())
        .map(|content| {
            let content = pdf.add_object(Stream::new(dictionary! {}, content.to_vec()));
            let page = dictionary! {
                "Type" => "Page", "Parent" => pages, "Contents" => content,
                "Resources" => resources,
                "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            };
            pdf.add_object(page).into()
        })
        .collect();
    let n = kids.len() as i64;
    pdf.objects.insert(
        pages,
        dictionary! { "Type" => "Pages", "Kids" => kids, "Count" => n }.into(),
    );
    let catalog = pdf.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
    pdf.trailer.set("Root", catalog);
    let path = dir.join(name);
    pdf.save(&path).unwrap();
    path
}

/// A watermark of `text` in black, wholly opaque, in the middle of every
/// page.
fn opaque(text: &str) -> sheaf_edit::Op {
    let spec = WatermarkSpec {
        content: WatermarkContent::Text {
            text: text.into(),
            font: FontChoice::HelveticaBold,
            size: 60.0,
            color: [0, 0, 0],
            opacity: 1.0,
            angle: 0.0,
        },
        position: GridPos::Center,
    };
    fx::set_watermark(spec, Z::Over, None)
}

/// `bytes` on PDFium, its first page drawn 612 pixels wide.
fn drawn(bytes: Vec<u8>) -> Raster {
    let engine = PdfiumEngine::open_bytes(bytes.into(), None).unwrap();
    engine
        .render_page(PageIndex(0), 612, PixelFormat::Rgba)
        .unwrap()
}

/// PDFium's characters on the first page of `bytes`: each, its box in
/// PageSpace and its angle.
fn chars(bytes: Vec<u8>) -> Vec<(char, Option<sheaf_doc::Box2>, f32)> {
    let engine = PdfiumEngine::open_bytes(bytes.into(), None).unwrap();
    let chars = engine.page_chars(PageIndex(0)).unwrap();
    chars.into_iter().map(|c| (c.ch, c.rect, c.angle)).collect()
}

sheaf_doc::both_ways!(
    cafe_reads_back_as_written,
    cafe_reads_back_as_written_appended: cafe
);

/// "Café" is drawn in the face's WinAnsi, which PDFium reads back as it
/// was typed.
fn cafe(how: How) {
    let dir = scratch(&format!("watermark-cafe-{how:?}"));
    let path = pages_pdf(&dir, "a.pdf", &[b"BT /F1 12 Tf 72 72 Td (body) Tj ET"]);
    let mut doc = open(&path);
    doc.commit_ops(vec![opaque("Café")], "watermark").unwrap();
    let read: String = chars(written(&doc, how)).iter().map(|(c, ..)| *c).collect();
    assert!(read.contains("Café"), "{how:?}: {read:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

sheaf_doc::both_ways!(
    a_turned_page_s_watermark_reads_upright,
    a_turned_page_s_watermark_reads_upright_appended: upright
);

/// A watermark on a page turned a quarter after it was set reads left to
/// right on the page as it shows: PDFium draws its characters up the page
/// in PageSpace, as a turned page's upright text runs, each to the right
/// of the one before as the page shows, along one line.
fn upright(how: How) {
    let dir = scratch(&format!("watermark-upright-{how:?}"));
    let path = pages_pdf(&dir, "a.pdf", &[b""]);
    let mut doc = open(&path);
    let page = doc.page(0).unwrap();
    doc.commit_ops(vec![opaque("DRAFT")], "watermark").unwrap();
    doc.commit_ops(vec![fx::rotate(&[page], Turn::Quarter)], "turn")
        .unwrap();
    let view = doc.geom(page).to_view();
    let read = chars(written(&doc, how));
    let letters: Vec<_> = read.iter().filter(|(c, ..)| c.is_alphabetic()).collect();
    let word: String = letters.iter().map(|(c, ..)| *c).collect();
    assert_eq!(word, "DRAFT", "{how:?}");
    let mut last: Option<sheaf_doc::Pt> = None;
    for (c, rect, angle) in letters {
        assert!((angle - 90.0).abs() < 1.0, "{how:?}: {c} at {angle}");
        let r = rect.unwrap();
        let middle = view.apply(sheaf_doc::Pt {
            x: r.x + r.w / 2.0,
            y: r.y + r.h / 2.0,
        });
        if let Some(last) = last {
            assert!(middle.x > last.x, "{how:?}: {c} isn't after the last");
            assert!(
                (middle.y - last.y).abs() < 2.0,
                "{how:?}: {c} isn't on its line"
            );
        }
        last = Some(middle);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// A front watermark over a page whose content leaves a `q` open with its
/// `cm`, ends a `q` it never began, or both, lands as it does on a page
/// whose content is balanced: no graphics state the page left reaches it.
/// PDFium draws all four alike, pixel for pixel, both ways.
#[test]
fn a_page_s_unbalanced_state_never_reaches_its_watermark() {
    let dir = scratch("watermark-balance");
    let clean: &[u8] = b"q 1 0 0 1 200 200 cm 0 0 1 rg 0 0 50 50 re f Q";
    let leaky: [&[u8]; 3] = [
        b"q 1 0 0 1 200 200 cm 0 0 1 rg 0 0 50 50 re f",
        b"Q q 1 0 0 1 200 200 cm 0 0 1 rg 0 0 50 50 re f",
        b"Q 1 0 0 1 200 200 cm 0 0 1 rg 0 0 50 50 re f",
    ];
    for how in [How::Rewritten, How::Appended] {
        let watermarked = |name: &str, content: &[u8]| {
            let path = pages_pdf(&dir, name, &[content]);
            let mut doc = open(&path);
            doc.commit_ops(vec![opaque("DRAFT")], "watermark").unwrap();
            drawn(written(&doc, how))
        };
        let want = watermarked("clean.pdf", clean);
        let dark = (want.data.chunks(4)).filter(|px| px[..3].iter().all(|&c| c < 60));
        assert!(dark.count() > 1000, "{how:?}: the watermark is drawn");
        for (i, content) in leaky.into_iter().enumerate() {
            let got = watermarked(&format!("leaky{i}.pdf"), content);
            let what = String::from_utf8_lossy(content);
            assert!(got.data == want.data, "{how:?}: {what}");
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// OCR text saying `text`, over the page's body.
fn layer(text: &str) -> std::sync::Arc<TextLayer> {
    let word = OcrWord {
        text: text.into(),
        rect: sheaf_doc::Box2 {
            x: 72.0,
            y: 100.0,
            w: 60.0,
            h: 12.0,
        },
        confidence: 0.9,
    };
    let geom = sheaf_doc::geom::Geom::read(None, None, 0);
    std::sync::Arc::new(sheaf_doc::text_layer::from_engine(
        vec![word],
        &geom,
        "test",
    ))
}

/// A page with an OCR layer sheaf wrote before, in an earlier save or as a
/// block on a page whose own content leaves a `q` open
/// (`text_layer::fixture::block_page`), given new text and a watermark,
/// draws as the page with the watermark alone: the old layer goes, and
/// nothing of it shows; PDFium reads the new words and not the old. Both
/// ways.
#[test]
fn a_watermark_over_an_old_ocr_layer_draws_as_over_the_page() {
    let dir = scratch("watermark-ocr");
    let body: &[u8] = b"BT /F1 24 Tf 72 700 Td (body) Tj ET";
    let path = pages_pdf(&dir, "ocr.pdf", &[body]);
    let mut doc = open(&path);
    let page = doc.page(0).unwrap();
    doc.commit_ops(fx::ocr([(page, layer("before"))]), "ocr")
        .unwrap();
    How::Rewritten.save(&mut doc, &path);
    let block = dir.join("block.pdf");
    saved::block_page().save(&block).unwrap();
    for (old, body, was) in [(&path, body, "before"), (&block, saved::BASE, "Saved")] {
        let plain = pages_pdf(&dir, "plain.pdf", &[body]);
        let mut plain = open(&plain);
        plain
            .commit_ops(vec![opaque("DRAFT")], "watermark")
            .unwrap();
        for how in [How::Rewritten, How::Appended] {
            let mut doc = open(old);
            let page = doc.page(0).unwrap();
            let mut ops = fx::ocr([(page, layer("after"))]);
            ops.push(opaque("DRAFT"));
            doc.commit_ops(ops, "ocr and watermark").unwrap();
            let bytes = written(&doc, how);
            let read: String = chars(bytes.clone()).into_iter().map(|c| c.0).collect();
            assert!(
                read.contains("after") && !read.contains(was),
                "{how:?}: {read}"
            );
            let got = drawn(bytes);
            let want = drawn(written(&plain, how));
            assert!(got.data == want.data, "{was} {how:?}");
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

sheaf_doc::both_ways!(
    a_form_s_watermarked_pages_check_clean,
    a_form_s_watermarked_pages_check_clean_appended: watermarked_form
);

/// A watermark on every page of a form: the file checks clean, its values
/// are kept, and PDFium draws its fields.
fn watermarked_form(how: How) {
    let dir = scratch(&format!("watermark-form-{how:?}"));
    let path = write(&dir, "form.pdf", &mut two_page_form());
    let mut doc = open(&path);
    let set = fx::set_watermark(fx::watermark("DRAFT"), Z::Over, None);
    doc.commit_ops(vec![set], "watermark").unwrap();
    let bytes = written(&doc, how);
    let what = format!("{how:?}");
    let report = clean(&bytes, &what);
    assert_eq!((report.fields, report.widgets), (4, 4), "{what}");
    let v = values(&bytes);
    assert_eq!(
        (v["a"].as_str(), v["b"].as_str()),
        ("Alpha", "Beta"),
        "{what}"
    );
    let engine = PdfiumEngine::open_bytes(bytes.clone().into(), None).unwrap();
    for page in 0..2 {
        let r = engine
            .render_page(PageIndex(page), 612, PixelFormat::Rgba)
            .unwrap();
        assert!(blue_in(&r, 0, 792.0) > HALF_A_ROW, "{what}: page {page}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// A watermark and a background, saved both ways and written again by
/// PDFium as another tool built on it would write them
/// (`FPDF_SaveAsCopy`), are still found when the file opens: the same
/// generators, drawing the same, on the same pages. Ignored, as a check
/// of PDFium's writer rather than of sheaf's.
#[test]
#[ignore = "PDFium's SaveAsCopy: cargo test -p sheaf-ops --test watermark -- --ignored"]
fn a_pdfium_resave_keeps_the_watermark_found() {
    let dir = scratch("watermark-resave");
    let page: &[u8] = b"BT /F1 24 Tf 72 72 Td (page) Tj ET";
    let path = pages_pdf(&dir, "a.pdf", &[page; 3]);
    let mut doc = open(&path);
    let second = doc.page(1).unwrap();
    let under = fx::set_watermark(fx::watermark("under"), Z::Under, Some(&[second]));
    doc.commit_ops(vec![opaque("DRAFT"), under], "watermark")
        .unwrap();
    let said = |d: &sheaf_doc::Document, z| {
        let st = d.state();
        let g = st.watermark(z).unwrap();
        let wm = g.spec.watermark().unwrap();
        let on = match &g.scope {
            sheaf_doc::model::Scope::All => None,
            sheaf_doc::model::Scope::Pages(pages) => Some(
                pages
                    .iter()
                    .map(|&p| st.pos_of(p).unwrap())
                    .collect::<Vec<_>>(),
            ),
        };
        (g.id, format!("{:?}", wm.content), wm.position, on)
    };
    for how in [How::Rewritten, How::Appended] {
        let engine = PdfiumEngine::open_bytes(written(&doc, how).into(), None).unwrap();
        let again = dir.join(format!("{how:?}.pdf"));
        std::fs::write(&again, engine.save_copy().unwrap()).unwrap();
        let found = open(&again);
        for z in [Z::Over, Z::Under] {
            assert_eq!(said(&found, z), said(&doc, z), "{how:?}, {z:?}");
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// A watermark found when the file opens again, over content that leaves
/// its graphics state changed, then changed, draws as the same one set on
/// the file as it was first; removed, the page draws as it did before it
/// had one. PDFium draws them alike, pixel for pixel, both ways.
#[test]
fn a_found_watermark_changed_or_removed_draws_as_set_afresh() {
    let dir = scratch("watermark-found");
    let leaky: &[u8] = b"Q q 1 0 0 1 200 200 cm 0 0 1 rg 0 0 50 50 re f";
    let path = pages_pdf(&dir, "a.pdf", &[leaky]);
    let mut doc = open(&path);
    doc.commit_ops(vec![opaque("DRAFT")], "watermark").unwrap();
    How::Rewritten.save(&mut doc, &path);
    let plain = open(&pages_pdf(&dir, "plain.pdf", &[leaky]));
    for how in [How::Rewritten, How::Appended] {
        let mut want = open(&pages_pdf(&dir, "want.pdf", &[leaky]));
        want.commit_ops(vec![opaque("FINAL")], "watermark").unwrap();
        let mut found = open(&path);
        found
            .commit_ops(vec![opaque("FINAL")], "watermark")
            .unwrap();
        let got = drawn(written(&found, how));
        assert!(got.data == drawn(written(&want, how)).data, "{how:?}");
        let page = found.page(0).unwrap();
        let remove = fx::remove_watermark(None, &[page]);
        found.commit_ops(vec![remove], "remove").unwrap();
        let got = drawn(written(&found, how));
        assert!(got.data == drawn(written(&plain, how)).data, "{how:?}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// Where [`acrobat_pdf`]'s watermark draws, in pixels of a page drawn 612
/// wide from its top left: its form's box, placed at (150, 350).
const ACROBAT_BOX: [u32; 4] = [150, 382, 450, 442];

/// The page's own content: text above and below the watermark, and a red
/// box.
const BODY: &[u8] = b"BT /F1 24 Tf 72 700 Td (Body above) Tj ET \
    BT /F1 24 Tf 72 200 Td (Body below) Tj ET q 1 0 0 rg 400 100 60 40 re f Q";

/// Acrobat's watermark, its form placed in [`ACROBAT_BOX`].
const ACROBAT: &[u8] = b"/Artifact <</Type /Pagination /Subtype /Watermark>> BDC q \
    1 0 0 1 150 350 cm /Fm0 Do Q EMC";

/// A file of one Letter page drawing `streams`, Helvetica its `/F1`, and a
/// watermark Acrobat would add its `/Fm0`: a form whose `/PieceInfo` says
/// so, drawing "ACROBAT" in blue.
fn acrobat_pdf(dir: &Path, name: &str, streams: &[&[u8]]) -> PathBuf {
    let mut pdf = Pdf::with_version("1.7");
    let pages = pdf.new_object_id();
    let font = pdf.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
    });
    let piece = dictionary! { "ADBE_CompoundType" => dictionary! {
        "DocSettings" => dictionary! {}, "Private" => "Watermark",
    } };
    let form = pdf.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject", "Subtype" => "Form", "PieceInfo" => piece,
            "BBox" => vec![0.into(), 0.into(), 300.into(), 60.into()],
            "Resources" => dictionary! { "Font" => dictionary! { "F1" => font } },
        },
        b"0 0 1 rg BT /F1 48 Tf 4 12 Td (ACROBAT) Tj ET".to_vec(),
    ));
    let contents: Vec<lopdf::Object> = (streams.iter())
        .map(|s| {
            pdf.add_object(Stream::new(dictionary! {}, s.to_vec()))
                .into()
        })
        .collect();
    let page = pdf.add_object(dictionary! {
        "Type" => "Page", "Parent" => pages, "Contents" => contents,
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        "Resources" => dictionary! {
            "Font" => dictionary! { "F1" => font },
            "XObject" => dictionary! { "Fm0" => form },
        },
    });
    pdf.objects.insert(
        pages,
        dictionary! { "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1 }.into(),
    );
    let catalog = pdf.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
    pdf.trailer.set("Root", catalog);
    let path = dir.join(name);
    pdf.save(&path).unwrap();
    path
}

/// A page's four streams.
type Streams = [&'static [u8]; 4];

/// The page's streams with Acrobat's watermark after its content, and as
/// Acrobat lays it out: a `q` before the content, which the watermark's
/// `Q` ends, and more drawn after it. Each with the watermark's place
/// empty: the same streams without it.
fn acrobat_layouts() -> [(Streams, Streams); 2] {
    const SCALED: &[u8] = b"BT /F1 24 Tf 72 700 Td (Body above) Tj ET 0.5 0 0 0.5 0 0 cm \
        BT /F1 48 Tf 144 400 Td (Body below) Tj ET";
    const AFTER: &[u8] = b"q 1 0 0 rg 400 100 60 40 re f Q";
    const OPENING_Q: &[u8] = b"/Artifact <</Type /Pagination /Subtype /Watermark>> BDC Q q \
        1 0 0 1 150 350 cm /Fm0 Do Q EMC";
    [
        ([BODY, ACROBAT, b"", b""], [BODY, b"", b"", b""]),
        (
            [b"q\n", SCALED, OPENING_Q, AFTER],
            [b"q\n", SCALED, b"Q", AFTER],
        ),
    ]
}

/// Acrobat's watermark taken off: PDFium draws every pixel outside its
/// box as before, the page's own content as it was, and nothing of the
/// watermark in its box. After the page's content and as Acrobat lays it
/// out, both ways.
#[test]
fn acrobat_s_watermark_removed_leaves_the_rest_of_the_page() {
    let dir = scratch("watermark-acrobat-removed");
    for (i, (streams, _)) in acrobat_layouts().into_iter().enumerate() {
        let path = acrobat_pdf(&dir, &format!("{i}.pdf"), &streams);
        let before = drawn(std::fs::read(&path).unwrap());
        for how in [How::Rewritten, How::Appended] {
            let mut doc = open(&path);
            let page = doc.page(0).unwrap();
            let remove = fx::remove_watermark(Some(Z::Over), &[page]);
            let c = doc.commit_ops(vec![remove], "remove").unwrap();
            assert_eq!(
                c.lines[0].text,
                "Remove the watermark from page 1 (1 made in Acrobat)"
            );
            let after = drawn(written(&doc, how));
            let [left, top, right, bottom] = ACROBAT_BOX;
            let inside = |x: u32, y: u32| (left..right).contains(&x) && (top..bottom).contains(&y);
            let pixel = |r: &Raster, x: u32, y: u32| {
                let i = ((y * r.width + x) * 4) as usize;
                [r.data[i], r.data[i + 1], r.data[i + 2]]
            };
            let blue = |p: [u8; 3]| p[2] > 150 && p[0] < 100;
            let mut was_blue = 0;
            for y in 0..before.height {
                for x in 0..before.width {
                    let (was, is) = (pixel(&before, x, y), pixel(&after, x, y));
                    if inside(x, y) {
                        was_blue += usize::from(blue(was));
                        assert!(!blue(is), "{i}, {how:?}: the watermark at ({x}, {y})");
                    } else {
                        assert_eq!(was, is, "{i}, {how:?}: ({x}, {y}) changed");
                    }
                }
            }
            assert!(was_blue > 500, "{i}: the watermark drew {was_blue} pixels");
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// A watermark of the document's on a page with Acrobat's draws as it does
/// on the page without Acrobat's, pixel for pixel: Acrobat's is replaced,
/// as Acrobat lays it out too. Both ways.
#[test]
fn acrobat_s_watermark_replaced_draws_as_ours_alone() {
    let dir = scratch("watermark-acrobat-replaced");
    for (i, (streams, without)) in acrobat_layouts().into_iter().enumerate() {
        for how in [How::Rewritten, How::Appended] {
            let watermarked = |name: &str, streams: &[&[u8]]| {
                let mut doc = open(&acrobat_pdf(&dir, name, streams));
                doc.commit_ops(vec![opaque("DRAFT")], "watermark").unwrap();
                drawn(written(&doc, how))
            };
            let got = watermarked(&format!("{i}.pdf"), &streams);
            let want = watermarked(&format!("{i}-without.pdf"), &without);
            assert!(got.data == want.data, "{i}, {how:?}");
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// What `doc`'s file, written `how` and opened again, has of others'
/// watermarks and backgrounds: each kind and origin, how many times.
fn found_in(bytes: Vec<u8>, name: &str) -> BTreeMap<(PaginationKind, PagOrigin), usize> {
    let dir = scratch(&format!("watermark-found-in-{name}"));
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    let doc = open(&path);
    let src = doc.primary();
    let mut found = BTreeMap::new();
    for i in 0..src.page_count() {
        for p in src.blocks(i).unwrap().pagination.iter() {
            *found.entry((p.kind, p.origin)).or_default() += 1;
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    found
}

/// Corpus files with others' watermarks (skipped without the corpus): a
/// watermark of the document's replaces Acrobat's, and RemoveWatermark
/// takes it off, saying so; on a Word file, the author's running heads,
/// footers and watermark all stay under it, and RemoveWatermark leaves the
/// author's watermark, saying so, but takes those a splice can when asked.
#[test]
fn corpus_files_with_others_watermarks() {
    use PagOrigin::{Acrobat, Authored};
    use PaginationKind::{Footer, Header, Watermark};
    let dir = scratch("watermark-corpus");
    let Some(acrobat) = corpus_form("forms/safedocs/0043199.pdf") else {
        return;
    };
    let path = dir.join("acrobat.pdf");
    std::fs::write(&path, &acrobat).unwrap();
    assert!(found_in(acrobat.clone(), "a0.pdf").contains_key(&(Watermark, Acrobat)));
    for how in [How::Rewritten, How::Appended] {
        let mut doc = open(&path);
        doc.commit_ops(vec![opaque("DRAFT")], "watermark").unwrap();
        let found = found_in(written(&doc, how), "a1.pdf");
        assert!(
            !found.contains_key(&(Watermark, Acrobat)),
            "{how:?}: {found:?}"
        );
        let mut doc = open(&path);
        let all: Vec<_> = doc.state().order().iter().copied().collect();
        let c = (doc.commit_ops(vec![fx::remove_watermark(None, &all)], "remove")).unwrap();
        assert!(
            c.lines[0].text.contains("made in Acrobat"),
            "{}",
            c.lines[0].text
        );
        let found = found_in(written(&doc, how), "a2.pdf");
        assert!(
            !found.contains_key(&(Watermark, Acrobat)),
            "{how:?}: {found:?}"
        );
    }
    let word = corpus_form("forms/safedocs/0128827.pdf").unwrap();
    let path = dir.join("word.pdf");
    std::fs::write(&path, &word).unwrap();
    let authored = found_in(word, "w0.pdf");
    for kind in [Header, Footer, Watermark] {
        assert!(authored.contains_key(&(kind, Authored)), "{authored:?}");
    }
    for how in [How::Rewritten, How::Appended] {
        let mut doc = open(&path);
        doc.commit_ops(vec![opaque("DRAFT")], "watermark").unwrap();
        let found = found_in(written(&doc, how), "w1.pdf");
        for kind in [Header, Footer, Watermark] {
            assert_eq!(
                found.get(&(kind, Authored)),
                authored.get(&(kind, Authored)),
                "{how:?}"
            );
        }
        let all: Vec<_> = doc.state().order().iter().copied().collect();
        let left = (doc.commit_ops(vec![fx::remove_watermark(None, &all)], "remove")).unwrap();
        let n = authored[&(Watermark, Authored)];
        let line = &left.lines[0].text;
        assert!(
            line.ends_with(&format!("({n} the author's left in place)")),
            "{line}"
        );
        let asked = sheaf_edit::Op::RemoveWatermark {
            z: None,
            pages: fx::ids(&all),
            include_authored: true,
        };
        let c = doc.commit_ops(vec![asked], "remove the author's").unwrap();
        let line = &c.lines[0].text;
        let taken = |line: &str| {
            let at = line
                .find(" the author's;")
                .or_else(|| line.find(" the author's)"))?;
            let digits = line[..at].rsplit(['(', ' ']).next()?;
            digits.parse::<usize>().ok()
        };
        let found = found_in(written(&doc, how), "w2.pdf");
        let now = found.get(&(Watermark, Authored)).copied().unwrap_or(0);
        assert_eq!(now, n - taken(line).unwrap_or(0), "{how:?}: {line}");
        for kind in [Header, Footer] {
            assert_eq!(
                found.get(&(kind, Authored)),
                authored.get(&(kind, Authored))
            );
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// A page another file's generators drew a background and a watermark on,
/// merged into its streams, each list marked as its generator's
/// (`/SHEF_Gen`): an image in its own graphics state; then "DRAFT" in red
/// over the page's content.
fn merged_pdf(dir: &Path, name: &str) -> PathBuf {
    let mut pdf = Pdf::with_version("1.7");
    let pages = pdf.new_object_id();
    let font = pdf.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
    });
    let bold = pdf.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica-Bold",
    });
    let gs = pdf.add_object(dictionary! { "Type" => "ExtGState", "ca" => 0.3, "CA" => 0.3 });
    let image = pdf.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject", "Subtype" => "Image", "Width" => 2, "Height" => 2,
            "ColorSpace" => "DeviceRGB", "BitsPerComponent" => 8,
        },
        vec![255, 0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0],
    ));
    let resources = dictionary! {
        "Font" => dictionary! { "F1" => font, "F2" => bold },
        "ExtGState" => dictionary! { "GS0" => gs },
        "XObject" => dictionary! { "Im0" => image },
    };
    let streams: [&[u8]; 3] = [
        b"/Artifact <</Type/Pagination/Subtype/Background/SHEF_Gen (g1b)>> BDC\nq\n/GS0 gs \
          306 0 0 306 153 243 cm /Im0 Do\nQ\nEMC\n",
        b"BT /F1 24 Tf 72 700 Td (body) Tj ET",
        b"/Artifact <</Type/Pagination/Subtype/Watermark/SHEF_Gen (g1a)>> BDC\nq\n/GS0 gs \
          0.7071 0.7071 -0.7071 0.7071 200 300 cm BT 0.784 0.118 0.118 rg /F2 72 Tf 0 0 Td \
          (DRAFT) Tj ET\nQ\nEMC\n",
    ];
    let contents: Vec<lopdf::Object> = (streams.iter())
        .map(|s| {
            pdf.add_object(Stream::new(dictionary! {}, s.to_vec()))
                .into()
        })
        .collect();
    let page = pdf.add_object(dictionary! {
        "Type" => "Page", "Parent" => pages, "Contents" => contents, "Resources" => resources,
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
    });
    pdf.objects.insert(
        pages,
        dictionary! { "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1 }.into(),
    );
    let catalog = pdf.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
    pdf.trailer.set("Root", catalog);
    let path = dir.join(name);
    pdf.save(&path).unwrap();
    path
}

/// A clean copy of a page another file's generators drew a watermark and
/// a background on draws them as the file did, pixel for pixel, and holds
/// nothing that marks them as sheaf's: none of sheaf's private data (§15).
#[test]
fn a_clean_copy_draws_another_file_s_watermark_as_it_was() {
    let dir = scratch("watermark-merged-clean");
    let path = merged_pdf(&dir, "merged.pdf");
    let original = std::fs::read(&path).unwrap();
    let doc = open(&path);
    let out = dir.join("clean.pdf");
    let clean = common::exported_to(&doc, Export::clean_copy, &out);
    assert!(holds(&original, b"SHEF_Gen"));
    assert!(!holds(&clean, b"SHEF_Gen"));
    let found = sheaf_doc::private::found(&Pdf::load_mem(&clean).unwrap());
    assert_eq!(found, []);
    let (before, after) = (drawn(original), drawn(clean));
    let tinted = |more: usize, less: usize| {
        (before.data.chunks(4))
            .filter(|px| px[more] > px[less].saturating_add(40))
            .count()
    };
    let (red, green) = (tinted(0, 1), tinted(1, 0));
    assert!(red > 500 && green > 500, "both are drawn: {red}, {green}");
    assert!(
        before.data == after.data,
        "the clean copy draws as the file"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
