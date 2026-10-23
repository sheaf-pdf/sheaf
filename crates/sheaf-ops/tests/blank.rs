//! Finding blank pages in a mixed document, each page read as the
//! document's views draw it with its content alone (M10.5 §10.4): a header
//! not yet saved leaves a blank page blank, recognized text not yet saved
//! is the page's, and a source PDFium refuses is read through its groups,
//! its blank pages found and removed, and OCR's check for pages that have
//! text already reading them there too.
//!
//! Needs libpdfium (`scripts/fetch-pdfium.sh`), like the app itself.

// A test crate of its own, whose scratch files go in the temp directory
// clippy.toml bans outside tests.
#![allow(clippy::disallowed_methods)]

mod common;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use futures::StreamExt;
use futures::executor::block_on;
use lopdf::{Document as Pdf, Object, Stream, dictionary};
use sheaf_doc::materialize::AskAnswers;
use sheaf_doc::realize::ViewInputs;
use sheaf_doc::serialize::fixture::revision_cut_out;
use sheaf_doc::store::BlobKind;
use sheaf_doc::tx::fixture as fx;
use sheaf_doc::{Box2, Document, OcrWord, PageId, TextOpts, TextService};
use sheaf_edit::{BlobSel, Fit, Op, PageSize, Rotation};
use sheaf_image::blank::{Sensitivity, Verdict, is_blank};
use sheaf_image::raster::PixelFormat;
use sheaf_ocr::job::{EngineFactory, HAS_TEXT_MIN_CHARS};
use sheaf_ocr::{EngineKind, OcrConfig, OcrEvent, OcrRequest, PageSource};
use sheaf_ops::NoProgress;
use sheaf_ops::blank::{BlankParams, Candidate, Reason};
use sheaf_ops::session::Session;
use sheaf_ops::text::Text;
use sheaf_ops::views::{ViewPages, Views};
use sheaf_render::pdfium::PdfiumEngine;
use sheaf_render::{Budget, Job, OpenError, PageIndex, Renderer};

use common::Blind;

/// Pages: 0 text, 1 empty, 2 "intentionally left blank", 3 a link on an
/// empty page, 4 a lone signature stroke, 5 another empty page.
fn mixed() -> Vec<u8> {
    let mut pdf = Pdf::with_version("1.5");
    let pages_id = pdf.new_object_id();
    let font = pdf.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
    });
    let contents = [
        "BT /F1 12 Tf 72 700 Td (Statement of account for the year) Tj 0 -16 Td (More lines of real content here) Tj ET".to_string(),
        String::new(),
        "BT /F1 12 Tf 200 400 Td (This page intentionally left blank) Tj ET".to_string(),
        String::new(),
        "0 0 0 RG 2 w 380 120 m 520 128 l S".to_string(),
        String::new(),
    ];
    let kids: Vec<Object> = contents
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let content = pdf.add_object(Stream::new(dictionary! {}, c.clone().into_bytes()));
            let mut page = dictionary! {
                "Type" => "Page", "Parent" => pages_id, "Contents" => content,
                "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
                "Resources" => dictionary! { "Font" => dictionary! { "F1" => font } },
            };
            if i == 3 {
                let link = pdf.add_object(dictionary! {
                    "Type" => "Annot", "Subtype" => "Link",
                    "Rect" => vec![100.into(), 100.into(), 200.into(), 120.into()],
                });
                page.set("Annots", vec![link.into()]);
            }
            pdf.add_object(page).into()
        })
        .collect();
    let n = kids.len() as i64;
    pdf.objects.insert(
        pages_id,
        dictionary! { "Type" => "Pages", "Kids" => kids, "Count" => n }.into(),
    );
    let catalog = pdf.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    pdf.trailer.set("Root", catalog);
    let mut bytes = Vec::new();
    pdf.save_to(&mut bytes).unwrap();
    bytes
}

/// The mixed document's blank pages, as it's opened.
const MIXED: [(usize, Reason); 4] = [
    (1, Reason::Blank),
    (2, Reason::IntentionallyBlank),
    (4, Reason::NearlyBlank),
    (5, Reason::Blank),
];

/// `bytes`, written to `dir` as `name`, opened as the app opens it.
fn opened(dir: &Path, name: &str, bytes: Vec<u8>) -> Document {
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    sheaf_doc::open::file(&path, None).unwrap().doc
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("sheaf-blank-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The blank pages among `pages` of `doc` (none: every page), each read
/// as views of the test's own draw it.
fn found(doc: &Document, pages: Option<&[usize]>, params: BlankParams) -> Vec<Candidate> {
    let views = Views::start().unwrap();
    sheaf_ops::blank::find(&doc.snapshot(), &views, pages, params, &NoProgress).unwrap()
}

/// Where each page found is, and why.
fn reasons(found: &[Candidate]) -> Vec<(usize, Reason)> {
    found.iter().map(|c| (c.index, c.reason)).collect()
}

#[test]
fn finds_blanks_and_spares_content() {
    let dir = scratch("mixed");
    let doc = opened(&dir, "mixed.pdf", mixed());
    let all = found(&doc, None, BlankParams::default());
    assert_eq!(reasons(&all), MIXED);
    assert!(!all[2].drop_by_default());
    // Keeping intentional ones, and a range.
    let params = BlankParams {
        keep_intentional: true,
        ..Default::default()
    };
    let some = found(&doc, Some(&[0, 1, 2, 3]), params);
    assert_eq!(some.iter().map(|c| c.index).collect::<Vec<_>>(), vec![1]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_noisy_scanned_blank_is_blank() {
    let dir = scratch("scan");
    // An off-white "scan" with noise and faint bleed-through.
    let mut img = image::GrayImage::from_pixel(1275, 1650, image::Luma([228]));
    let mut s = 3u32;
    for p in img.pixels_mut() {
        s = s.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        p[0] = (p[0] as i32 + ((s >> 16) % 13) as i32 - 6) as u8;
    }
    for line in 0..40u32 {
        for x in 150..1100 {
            for y in 0..10 {
                img.put_pixel(x, 150 + line * 34 + y, image::Luma([200]));
            }
        }
    }
    let page = sheaf_image::io::PageImage::from_image(image::DynamicImage::ImageLuma8(img), 150.0);
    let bytes = sheaf_doc::images::to_pdf(&[page], sheaf_edit::Fit::Original).unwrap();
    let doc = opened(&dir, "scan.pdf", bytes);
    let found = found(&doc, None, BlankParams::default());
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].reason, Reason::Blank);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Blank pages and an image's, inserted and saved, write no file but the
/// one saved: they're made in memory (M10.5 §3.9), and nothing goes to the
/// temp, cache or home directories ([`common::own_dirs`]).
#[test]
fn inserted_blank_and_image_pages_write_nothing_else() {
    common::own_dirs("inserted_blank_and_image_pages_write_nothing_else", || {
        let dir = common::scratch("memory");
        let path = dir.join("mixed.pdf");
        std::fs::write(&path, mixed()).unwrap();
        let mut doc = common::open(&path);
        let red = image::RgbImage::from_pixel(40, 20, image::Rgb([200, 30, 30]));
        let png = sheaf_image::encode::png(&red, 72).unwrap();
        let kind = BlobKind::image(&png).unwrap();
        let image = doc.store().intern(png.into(), kind).unwrap();
        let insert = |from| Op::InsertPages {
            from,
            pages: None,
            before: None,
            bring: Default::default(),
            locals: Vec::new(),
        };
        let letter = PageSize {
            width: 612.0,
            height: 792.0,
        };
        let images = vec![BlobSel::Id(image.id())];
        let ops = vec![
            insert(sheaf_edit::PageSource::Blank {
                size: letter,
                count: 2,
            }),
            insert(sheaf_edit::PageSource::Images {
                images,
                fit: Fit::LETTER,
            }),
        ];
        doc.commit_ops(ops, "insert").unwrap();
        drop(image);
        let (_, bytes) = common::planned(&mut doc, &path);
        assert_eq!(Pdf::load_mem(&bytes).unwrap().get_pages().len(), 6 + 3);
        let _ = std::fs::remove_dir_all(&dir);
    });
}

/// A header and footer not yet saved, on every page, are no page's
/// content (§10.4): the screen shows them on an empty page, its ink more
/// than a blank page's, and reads their text, yet the blank pages found
/// are the file's, for the same reasons.
#[test]
fn an_unsaved_header_leaves_a_blank_page_blank() {
    let dir = scratch("header");
    let mut doc = opened(&dir, "mixed.pdf", mixed());
    let mut header = fx::header("Smith v. Jones");
    header.texts[1] = "CONFIDENTIAL".into();
    header.texts[2] = "Page <<1>> of <<N>>".into();
    header.texts[4] = "Exhibit 12".into();
    doc.commit_ops(vec![fx::set_header(header, None)], "Header")
        .unwrap();
    let views = Views::start().unwrap();
    let (st, empty, screen) = (doc.state(), doc.page(1).unwrap(), ViewInputs::screen());
    let job = Job::Page {
        width: 510,
        rotation: Rotation::R0,
    };
    let (format, budget) = (PixelFormat::Rgba, Budget::Screen);
    let rx = views.render(st, empty, &screen, job, format, 0, budget);
    let gray = block_on(rx).unwrap().unwrap().raster.to_gray().unwrap();
    let verdict = is_blank(&gray, Sensitivity::default());
    assert!(!matches!(verdict, Verdict::Blank { .. }), "{verdict:?}");
    let text = Text::new(views, Budget::Screen);
    let o = TextOpts { inputs: screen };
    let shown = TextService::chars(&text, &doc.snapshot(), empty, &o).unwrap();
    let shown = shown.text(0, shown.len());
    for said in ["CONFIDENTIAL", "Page 2 of 6", "Exhibit 12"] {
        assert!(shown.contains(said), "{shown}");
    }
    assert_eq!(reasons(&found(&doc, None, BlankParams::default())), MIXED);
    let _ = std::fs::remove_dir_all(&dir);
}

/// `words` recognized on a line of page `page` of `doc`, not yet saved.
fn recognized(doc: &mut Document, page: PageId, words: &[&str]) {
    let mut x = 72.0;
    let seen = (words.iter())
        .map(|w| {
            let width = 10.0 * w.len() as f32;
            let rect = Box2 {
                x,
                y: 300.0,
                w: width,
                h: 14.0,
            };
            x += width + 10.0;
            OcrWord {
                text: w.to_string(),
                rect,
                confidence: 0.9,
            }
        })
        .collect();
    let layer = sheaf_doc::text_layer::from_engine(seen, &doc.geom(page), "test");
    let ops = fx::ocr([(page, Arc::new(layer))]);
    doc.commit_ops(ops, "OCR").unwrap();
}

/// Recognized text not yet saved is the page's, as it was before the text
/// service: an empty page recognized as saying it's intentionally blank is
/// that, and one recognized as an invoice isn't blank.
#[test]
fn unsaved_ocr_text_still_counts() {
    let dir = scratch("ocr");
    let mut doc = opened(&dir, "mixed.pdf", mixed());
    let [empty, last] = [1, 5].map(|i| doc.page(i).unwrap());
    let blank = ["This", "page", "intentionally", "left", "blank"];
    recognized(&mut doc, empty, &blank);
    recognized(&mut doc, last, &["Invoice", "1234", "total", "56.00"]);
    assert_eq!(
        reasons(&found(&doc, None, BlankParams::default())),
        [
            (1, Reason::IntentionallyBlank),
            (2, Reason::IntentionallyBlank),
            (4, Reason::NearlyBlank),
        ]
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A file PDFium refuses (as it does the corpus's 4422502) has its pages
/// read from its groups: Remove Blank Pages finds the blank pages of the
/// file it was cut from and writes the rest, which PDFium reads, and the
/// pages it took out to a file of their own, each with a bookmark saying
/// what it was, the document left as it was.
#[test]
fn remove_blank_pages_works_on_a_source_pdfium_refuses() {
    let dir = scratch("refused");
    let bytes = revision_cut_out(mixed());
    let refused = PdfiumEngine::open_bytes(bytes.clone().into(), None).err();
    assert!(OpenError::is_refused(&refused.expect("PDFium refuses it")));
    let doc = opened(&dir, "refused.pdf", bytes);
    let all = found(&doc, None, BlankParams::default());
    assert_eq!(reasons(&all), MIXED);
    assert!(doc.primary().base_refused());
    let drop: Vec<Candidate> = all.into_iter().filter(|c| c.drop_by_default()).collect();
    let (kept, rejects) = (dir.join("kept.pdf"), dir.join("rejects.pdf"));
    let session = Session::from(doc);
    let answers = AskAnswers::default();
    let n = sheaf_ops::blank::write(&session, &drop, &kept, Some(&rejects), false, &answers);
    assert_eq!(n.unwrap(), 3);
    let engine = PdfiumEngine::open_bytes(std::fs::read(&kept).unwrap().into(), None).unwrap();
    assert_eq!(engine.page_count(), 3);
    let text = engine.page_text(PageIndex(0)).unwrap();
    assert!(text.contains("Statement of account"), "{text}");
    let rejected = sheaf_doc::open::file(&rejects, None).unwrap().doc;
    assert_eq!(rejected.page_count(), drop.len());
    let titles: Vec<String> = (rejected.state().bookmarks_preorder().into_iter())
        .map(|(_, _, b)| b.title.to_string())
        .collect();
    let want: Vec<String> = (drop.iter())
        .map(|c| format!("p. {} — {}", c.index + 1, c.reason.label()))
        .collect();
    assert_eq!(titles, want);
    assert_eq!(session.doc.version_count(), 1, "the document is as it was");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Remove Blank Pages over a signed form whose signature is on the page it
/// takes out: the rejects file would break the signature, so the two asks
/// before either file is written; answered, both are.
#[test]
fn a_removal_that_asks_writes_neither_file() {
    use sheaf_doc::forms::fixture::{Form, row};
    let dir = scratch("signed");
    let mut form = Form::new(2);
    let sig = form.signature("sig", 1, row(2));
    form.sign(sig);
    let doc = opened(&dir, "signed.pdf", form.bytes());
    let drop = vec![Candidate {
        index: 1,
        id: doc.page(1).unwrap(),
        ink: 0.0,
        reason: Reason::Blank,
    }];
    let (kept, rejects) = (dir.join("kept.pdf"), dir.join("rejects.pdf"));
    let session = Session::from(doc);
    let write = |answers: &AskAnswers| {
        sheaf_ops::blank::write(&session, &drop, &kept, Some(&rejects), false, answers)
    };
    let asked = write(&AskAnswers::default()).unwrap_err();
    assert!(asked.is::<sheaf_ops::session::NeedsDecision>(), "{asked:#}");
    assert!(!kept.exists() && !rejects.exists());
    let answers = AskAnswers {
        break_signatures: true,
        ..AskAnswers::default()
    };
    assert_eq!(write(&answers).unwrap(), 1);
    assert!(kept.is_file() && rejects.is_file());
    let _ = std::fs::remove_dir_all(&dir);
}

/// OCR's check for text a page already has reads a file PDFium refuses
/// through its groups, its content alone: a run that skips pages with
/// text skips the statement and the "intentionally left blank" page and
/// reads the other four, none failing; and each page's text is as long as
/// the saved file's.
#[test]
fn ocr_skips_text_on_a_source_pdfium_refuses() {
    let dir = scratch("ocr-refused");
    let doc = opened(&dir, "refused.pdf", revision_cut_out(mixed()));
    let pages = Arc::new(ViewPages::new(Views::start().unwrap(), doc.snapshot()));
    let order = doc.state().order().clone();
    let req = OcrRequest {
        pages: order.iter().map(|&p| pages.page(p).unwrap()).collect(),
        skip_pages_with_text: true,
        engine: EngineKind::Tesseract,
        config: OcrConfig {
            dpi: 150,
            ..OcrConfig::default()
        },
    };
    let factory: EngineFactory = Arc::new(|| Ok(Box::new(Blind) as _));
    let (_job, rx) = sheaf_ocr::job::start_with(req, pages.clone(), factory, 1);
    let events: Vec<OcrEvent> = block_on(rx.collect());
    let skipped: Vec<PageId> = (events.iter())
        .filter_map(|e| match e {
            OcrEvent::Skipped { id } => Some(*id),
            _ => None,
        })
        .collect();
    assert_eq!(skipped, [order[0], order[2]]);
    let all_read = matches!(
        events.last(),
        Some(OcrEvent::Finished {
            done: 4,
            skipped: 2,
            failed: 0,
            cancelled: false
        })
    );
    assert!(all_read, "{events:?}");
    assert!(doc.primary().base_refused());
    let saved = sheaf_doc::save::rewrite(&doc).unwrap();
    let engine = PdfiumEngine::open_bytes(saved.into(), None).unwrap();
    for (at, &id) in order.iter().enumerate() {
        let len = pages.text_len(&pages.page(id).unwrap()).unwrap();
        assert_eq!(len, engine.page_text_len(PageIndex(at as u32)).unwrap());
    }
    let first = pages.text_len(&pages.page(order[0]).unwrap()).unwrap();
    assert!(first >= HAS_TEXT_MIN_CHARS, "{first}");
    let _ = std::fs::remove_dir_all(&dir);
}
