//! Edited pages render from their realizations as the saved file shows
//! them (M10.5 §10.1, slice 13): two pages of a group sharing a font and a
//! duplicate, their section's new numbers past everything the base uses;
//! encrypted bases, the section encrypted with their key; a page turned
//! with recognized text, and one with a header, turned once by the job; a
//! damaged base, as a file of its own; a base PDFium refuses, every page of
//! it so, untouched or not, once the pool has found out, whichever engine
//! over its bytes found it, and its print file, which PDFium reads; a layer
//! turned off, by a patch of the catalog; a comment hidden on its page and
//! not its duplicate; a crop's comments, link and widget, and its content
//! alone for OCR; sheaf's marks, renamed at open or not, left to the
//! screen; and watermarks and backgrounds not yet saved (slice 16).
//!
//! Each realization is the engine's segments read one after another, the
//! base's bytes and the section after them. Pages are drawn through the
//! engine pool, live, and from the segments loaded alone, appearances
//! only, and compare with the saved Rewrite's drawn the same way. The
//! pool counts the views' group engines against the screen's six, and not
//! the base engine. OCR reads pages through [`ViewPages`], as they show,
//! their content alone, and its run closes none of the engines the screen
//! shows.
//!
//! Needs libpdfium (`scripts/fetch-pdfium.sh`), like the app itself.

mod common;

use std::collections::HashSet;
use std::sync::Arc;

use futures::StreamExt;
use futures::executor::block_on;

use lopdf::xref::XrefType;
use lopdf::{
    Dictionary, Document as Pdf, LazyPdf, Object, ObjectId, ReadLimits, Stream, dictionary,
};
use sheaf_doc::export::{Export, print_preview};
use sheaf_doc::forms::fixture::{Form, every_mark_kind, write_marks};
use sheaf_doc::layers::LayerId;
use sheaf_doc::model::Mark;
use sheaf_doc::realize::{EngineKey, GROUP, OcgOverride, PageView, Realization, ViewInputs};
use sheaf_doc::serialize::fixture::{
    USER, encrypted, form, holds, rc4_aes_128_and_aes_256, revision_cut_out,
};
use sheaf_doc::store::BlobKind;
use sheaf_doc::tx::fixture as fx;
use sheaf_doc::{Actor, Document, PageId, TextLayer};
use sheaf_edit::{BlobSel, GridPos, Margins, Rotation, Turn, WatermarkContent, WatermarkSpec, Z};
use sheaf_image::raster::{PixelFormat, Raster};
use sheaf_ocr::job::{EngineFactory, HAS_TEXT_MIN_CHARS};
use sheaf_ocr::{EngineKind, OcrConfig, OcrEvent, OcrRequest, PageSource};
use sheaf_ops::views::{ViewPages, Views};
use sheaf_render::pdfium::{PdfiumEngine, render_appearances};
use sheaf_render::pool::BACKGROUND_ENGINES;
use sheaf_render::{Budget, Draw, Job, OpenError, PageIndex, Renderer};

use common::{Blind, crop, file, opened, views};

/// Renders are this wide.
const WIDTH: u32 = 512;

/// The pixels a page drawn from another writing of it may differ by from
/// the Rewrite's: PDFium's anti-aliasing at an edge, as realize_corpus
/// allows.
const ANTI_ALIASING_PX: usize = 16;

/// What page `page` of `d` renders from under `inputs`: its view, and
/// what its engine opens.
fn realized(
    views: &Views,
    d: &Document,
    page: PageId,
    inputs: &ViewInputs,
) -> (PageView, Realization) {
    let st = d.state();
    let view = views.view(st, page, inputs).unwrap();
    let r = views.realizer().open(st, view.engine, inputs).unwrap();
    (view, r)
}

/// Page `page` of `d` under `inputs`, appearances only, from an engine of
/// its own on its realization's segments.
fn appearances_of(views: &Views, d: &Document, page: PageId, inputs: &ViewInputs) -> Raster {
    let (view, r) = realized(views, d, page, inputs);
    let engine = PdfiumEngine::open_segments(r.segments, r.password.as_deref()).unwrap();
    (engine.render_appearances(PageIndex(view.index), WIDTH)).unwrap()
}

/// Page `page` of `d` under `inputs`, drawn live by the pool, its pane
/// turned by `rotation`.
fn drawn(
    views: &Views,
    d: &Document,
    page: PageId,
    inputs: &ViewInputs,
    rotation: Rotation,
) -> Raster {
    let job = Job::Page {
        width: WIDTH,
        rotation,
    };
    let rx = views.render(
        d.state(),
        page,
        inputs,
        job,
        PixelFormat::Rgba,
        0,
        Budget::Screen,
    );
    block_on(rx).unwrap().unwrap().raster
}

/// `d` as a Rewrite saves it.
fn saved(d: &Document) -> Vec<u8> {
    sheaf_doc::save::rewrite(d).unwrap()
}

/// Page `index` of `bytes`, appearances only.
fn appearances(bytes: &[u8], password: Option<&str>, index: u32) -> Raster {
    render_appearances(bytes, password, PageIndex(index), WIDTH).unwrap()
}

/// Page `index` of `bytes`, drawn live and turned by `rotation`.
fn live(bytes: &[u8], password: Option<&str>, index: u32, rotation: Rotation) -> Raster {
    let engine = PdfiumEngine::open_bytes(bytes.into(), password).unwrap();
    let job = Job::Page {
        width: WIDTH,
        rotation,
    };
    (engine.render(PageIndex(index), job, PixelFormat::Rgba)).unwrap()
}

/// How many pixels of two renders differ: all of them when their sizes
/// do.
fn differing(a: &Raster, b: &Raster) -> usize {
    if (a.width, a.height) != (b.width, b.height) {
        return (a.width * a.height).max(b.width * b.height) as usize;
    }
    (a.data.chunks(4).zip(b.data.chunks(4)))
        .filter(|(a, b)| a != b)
        .count()
}

/// Asserts page `page` of `d` renders from its realization for a plugin
/// as page `at` of `saved`, a Rewrite: appearances only, and live.
fn renders_as_saved(views: &Views, d: &Document, page: PageId, saved: &[u8], at: u32) {
    let password = d.primary().password();
    let ours = appearances_of(views, d, page, &ViewInputs::plugin());
    let theirs = appearances(saved, password, at);
    assert_eq!(differing(&ours, &theirs), 0, "{page:?}, appearances");
    let ours = drawn(views, d, page, &ViewInputs::plugin(), Rotation::R0);
    let theirs = live(saved, password, at, Rotation::R0);
    assert_eq!(differing(&ours, &theirs), 0, "{page:?}, live");
}

/// The objects `section`, an update in a cross-reference table or stream,
/// writes, in order: each `n g obj` it starts.
fn numbered(section: &[u8]) -> Vec<u32> {
    let re = regex::bytes::Regex::new(r"(?m)^(\d+) 0 obj").unwrap();
    (re.captures_iter(section))
        .map(|c| std::str::from_utf8(&c[1]).unwrap().parse().unwrap())
        .collect()
}

/// Two edited pages of one group, their headers drawn in one font, and a
/// duplicate of one of them render as the Rewrite does, from one engine;
/// the section writes each number once, a page at its own object and
/// everything new past the base's `/Size`. The pages left alone render
/// from the base as they are.
#[test]
fn two_edited_pages_sharing_a_font_and_a_duplicate_render_as_saved() {
    let mut d = opened(file(4, XrefType::CrossReferenceTable), "a.pdf", None);
    let [p0, p1, p2, p3] = [0, 1, 2, 3].map(|i| d.page(i).unwrap());
    let header = fx::set_header(fx::header("Draft <<1>>"), Some(&[p0, p2]));
    d.commit_ops(vec![header, fx::duplicate(&[p0])], "Edit")
        .unwrap();
    let copy = d.page(1).unwrap();
    let saved = saved(&d);
    let views = views();
    let st = d.state();
    let mut engines = HashSet::new();
    for page in [p0, copy, p2] {
        let view = views.view(st, page, &ViewInputs::plugin()).unwrap();
        assert!(matches!(view.engine, EngineKey::Group(..)), "{view:?}");
        engines.insert(view.engine);
        renders_as_saved(&views, &d, page, &saved, st.pos_of(page).unwrap() as u32);
    }
    assert_eq!(engines.len(), 1);
    for page in [p1, p3] {
        let view = views.view(st, page, &ViewInputs::plugin()).unwrap();
        assert!(matches!(view.engine, EngineKey::Base(_)), "{view:?}");
        renders_as_saved(&views, &d, page, &saved, st.pos_of(page).unwrap() as u32);
    }
    let (_, r) = realized(&views, &d, p0, &ViewInputs::plugin());
    let src = d.primary();
    assert_eq!(&r.segments[0], src.bytes());
    let section = &r.segments[1];
    let written = numbered(section);
    let unique: HashSet<u32> = written.iter().copied().collect();
    assert_eq!(unique.len(), written.len(), "{written:?}");
    let pages: HashSet<u32> = src.pages().iter().map(|p| p.id.0).collect();
    let size = src.xref().size;
    let catalog = src
        .pdf()
        .trailer()
        .get(b"Root")
        .unwrap()
        .as_reference()
        .unwrap();
    for n in written {
        assert!(
            n >= size || pages.contains(&n) || n == catalog.0,
            "{n} under {size}"
        );
    }
    // The two headers' font is one object.
    let font = b"/BaseFont/Helvetica";
    let fonts = (section.windows(font.len())).filter(|w| w == font).count();
    assert_eq!(fonts, 1);
}

/// A page with a header in each of seven groups, and one left alone, drawn
/// on the screen: the pool holds the base engine and six group engines,
/// the screen's count of them.
#[test]
fn group_engines_count_against_the_screen_s_six() {
    let n = 7 * GROUP as usize;
    let mut d = opened(file(n, XrefType::CrossReferenceTable), "a.pdf", None);
    let edited: Vec<PageId> = (0..7)
        .map(|g| d.page(g * GROUP as usize).unwrap())
        .collect();
    let header = fx::set_header(fx::header("Draft"), Some(&edited));
    d.commit_ops(vec![header], "Header").unwrap();
    let views = views();
    let alone = d.page(1).unwrap();
    for &page in edited.iter().chain([&alone]) {
        drawn(&views, &d, page, &ViewInputs::screen(), Rotation::R0);
    }
    let open = views.pool().open_engines();
    let groups = (open.iter())
        .filter(|e| matches!(e, EngineKey::Group(..)))
        .count();
    assert_eq!((groups, open.len()), (6, 7), "{open:?}");
}

/// Over RC4-128, AES-128 and AES-256 bases, with a cross-reference table
/// and a stream, an edited page renders as the Rewrite does with the
/// password, and the section holds nothing of the edit in the clear.
#[test]
fn encrypted_bases_render() {
    let plain = form();
    for (name, state) in rc4_aes_128_and_aes_256(&plain) {
        for bytes in encrypted(&plain, &state, |_| {}) {
            let mut d = opened(bytes, "encrypted.pdf", Some(USER));
            let p0 = d.page(0).unwrap();
            let header = fx::set_header(fx::header("SECRET EDIT"), Some(&[p0]));
            d.commit_ops(vec![header], "Header").unwrap();
            let saved = saved(&d);
            let views = views();
            let (_, r) = realized(&views, &d, p0, &ViewInputs::plugin());
            assert_eq!(r.password.as_deref(), Some(USER), "{name}");
            assert!(!holds(&r.segments[1], b"SECRET"), "{name}");
            let ours = appearances_of(&views, &d, p0, &ViewInputs::plugin());
            let theirs = appearances(&saved, Some(USER), 0);
            assert_eq!(differing(&ours, &theirs), 0, "{name}");
            let base = appearances(d.primary().bytes(), Some(USER), 0);
            assert_ne!(differing(&ours, &base), 0, "{name}: the edit shows");
            let ours = drawn(&views, &d, p0, &ViewInputs::plugin(), Rotation::R0);
            let theirs = live(&saved, Some(USER), 0, Rotation::R0);
            assert_eq!(differing(&ours, &theirs), 0, "{name}, live");
        }
    }
}

/// A page turned with recognized text not yet saved is realized with its
/// base page's `/Rotate`, and its job turns it once: as the base page
/// turned by the job, and as the Rewrite's page, turned there.
#[test]
fn a_page_turned_with_unsaved_ocr_renders_once_turned() {
    let mut d = opened(file(2, XrefType::CrossReferenceTable), "a.pdf", None);
    let p0 = d.page(0).unwrap();
    let word = sheaf_doc::OcrWord {
        text: "word".into(),
        rect: sheaf_doc::Box2 {
            x: 72.0,
            y: 72.0,
            w: 60.0,
            h: 20.0,
        },
        confidence: 0.9,
    };
    let layer = Arc::new(TextLayer {
        words: vec![word],
        lines: std::iter::once(0..1).collect(),
        upright: Rotation::R0,
        engine: "ocr".into(),
    });
    d.commit_ops(fx::ocr([(p0, layer)]), "OCR").unwrap();
    d.commit_ops(vec![fx::rotate(&[p0], Turn::Quarter)], "Turn")
        .unwrap();
    let views = views();
    let view = views.view(d.state(), p0, &ViewInputs::screen()).unwrap();
    assert!(matches!(view.engine, EngineKey::Group(..)));
    assert_eq!(view.job.rotation, Rotation::R90);
    let ours = drawn(&views, &d, p0, &ViewInputs::screen(), Rotation::R0);
    let base = live(d.primary().bytes(), None, 0, Rotation::R90);
    let theirs = live(&saved(&d), None, 0, Rotation::R0);
    assert_eq!(ours.width, theirs.width);
    assert_eq!(differing(&ours, &base), 0);
    assert_eq!(differing(&ours, &theirs), 0);
}

/// A header not yet saved shows on a turned page: the job turns the page
/// its realization keeps unturned, header and all, as the Rewrite shows
/// it, and turns it further with its pane.
#[test]
fn an_unsaved_header_shows_under_view_rotation() {
    let mut d = opened(file(2, XrefType::CrossReferenceTable), "a.pdf", None);
    let p1 = d.page(1).unwrap();
    let header = fx::set_header(fx::header("Draft <<1>> of <<n>>"), Some(&[p1]));
    d.commit_ops(vec![header, fx::rotate(&[p1], Turn::Quarter)], "Edit")
        .unwrap();
    let views = views();
    let view = views.view(d.state(), p1, &ViewInputs::screen()).unwrap();
    assert_eq!(view.job.rotation, Rotation::R90);
    let saved = saved(&d);
    for pane in [Rotation::R0, Rotation::R90] {
        let ours = drawn(&views, &d, p1, &ViewInputs::screen(), pane);
        let theirs = live(&saved, None, 1, pane);
        assert_eq!(differing(&ours, &theirs), 0, "{pane:?}");
    }
    let ours = drawn(&views, &d, p1, &ViewInputs::screen(), Rotation::R0);
    let base = live(d.primary().bytes(), None, 1, Rotation::R90);
    assert_ne!(differing(&ours, &base), 0, "the header shows");
}

/// Watermarks not yet saved show as the saved file draws them: a text
/// watermark over two pages, an image background under two, one page with
/// both turned a quarter, each drawn from its group's overlay as the
/// Rewrite draws it, the turned one turned once by its job, in a pane
/// turned or not, the group's section writing what they share once. The
/// page under neither draws from the base, and OCR's inputs, which draw no
/// generator, view every page there.
#[test]
fn unsaved_watermarks_render_as_saved() {
    let mut d = opened(file(4, XrefType::CrossReferenceTable), "a.pdf", None);
    let [p0, p1, p2, p3] = [0, 1, 2, 3].map(|i| d.page(i).unwrap());
    let rgb = image::RgbImage::from_fn(8, 4, |x, _| image::Rgb([30 * x as u8, 90, 160]));
    let png = sheaf_image::encode::png(&rgb, 72).unwrap();
    let blob = (d.store())
        .intern(png.as_slice().into(), BlobKind::image(&png).unwrap())
        .unwrap();
    let image = WatermarkSpec {
        content: WatermarkContent::Image {
            image: Some(BlobSel::Id(blob.id())),
            scale: 0.5,
            opacity: 0.6,
        },
        position: GridPos::BottomRight,
    };
    let ops = vec![
        fx::set_watermark(fx::watermark("Draft"), Z::Over, Some(&[p0, p2])),
        fx::set_watermark(image, Z::Under, Some(&[p1, p2])),
        fx::rotate(&[p2], Turn::Quarter),
    ];
    d.commit_ops(ops, "Watermarks").unwrap();
    let saved = saved(&d);
    let views = views();
    let st = d.state();
    for page in [p0, p1] {
        let view = views.view(st, page, &ViewInputs::plugin()).unwrap();
        assert!(matches!(view.engine, EngineKey::Group(..)), "{view:?}");
        renders_as_saved(&views, &d, page, &saved, st.pos_of(page).unwrap() as u32);
    }
    let view = views.view(st, p2, &ViewInputs::plugin()).unwrap();
    assert!(matches!(view.engine, EngineKey::Group(..)), "{view:?}");
    assert_eq!(view.job.rotation, Rotation::R90);
    for pane in [Rotation::R0, Rotation::R90] {
        let ours = drawn(&views, &d, p2, &ViewInputs::plugin(), pane);
        let theirs = live(&saved, None, 2, pane);
        assert_eq!(differing(&ours, &theirs), 0, "{pane:?}");
    }
    let view = views.view(st, p3, &ViewInputs::plugin()).unwrap();
    assert!(matches!(view.engine, EngineKey::Base(_)), "{view:?}");
    for page in [p0, p1, p2, p3] {
        let view = views.view(st, page, &ViewInputs::content_only()).unwrap();
        assert!(matches!(view.engine, EngineKey::Base(_)), "{view:?}");
    }
    let plain = appearances(d.primary().bytes(), None, 0);
    let ours = appearances_of(&views, &d, p0, &ViewInputs::plugin());
    assert_ne!(differing(&ours, &plain), 0, "the watermark shows");
    // The pages' deltas share what they draw with: the section writes each
    // number once, and the watermark's font once.
    let (_, r) = realized(&views, &d, p0, &ViewInputs::plugin());
    let section = &r.segments[1];
    let written = numbered(section);
    let unique: HashSet<u32> = written.iter().copied().collect();
    assert_eq!(unique.len(), written.len(), "{written:?}");
    let font = b"/BaseFont/Helvetica-Bold";
    let fonts = (section.windows(font.len())).filter(|w| w == font).count();
    assert_eq!(fonts, 1);
}

/// `bytes` with its `startxref` pointing nowhere, so a reader rebuilds
/// its cross-references.
fn damaged(mut bytes: Vec<u8>) -> Vec<u8> {
    let at = (bytes.windows(9).rposition(|w| w == b"startxref")).unwrap() + 10;
    let end = at
        + bytes[at..]
            .iter()
            .position(|b| !b.is_ascii_digit())
            .unwrap();
    let wrong = format!("{:0width$}", 7, width = end - at);
    bytes.splice(at..end, wrong.into_bytes());
    bytes
}

/// A damaged base, plain or encrypted, realizes an edited page as a file
/// of its own, unencrypted, which renders as the Rewrite does; its
/// untouched pages render from the base.
#[test]
fn a_damaged_base_realizes_standalone() {
    let plain = form();
    let [(_, rc4), ..] = rc4_aes_128_and_aes_256(&plain);
    let [encrypted_table, _] = encrypted(&plain, &rc4, |_| {});
    let bases = [
        (damaged(file(2, XrefType::CrossReferenceTable)), None),
        (damaged(encrypted_table), Some(USER)),
    ];
    for (bytes, password) in bases {
        let mut d = opened(bytes, "damaged.pdf", password);
        assert!(!d.primary().base_sound());
        let last = d.page(d.state().page_count() - 1).unwrap();
        let header = fx::set_header(fx::header("Draft"), Some(&[last]));
        d.commit_ops(vec![header], "Header").unwrap();
        let views = views();
        let st = d.state();
        let (_, r) = realized(&views, &d, last, &ViewInputs::plugin());
        assert_eq!((r.segments.len(), r.password.as_deref()), (1, None));
        let at = st.pos_of(last).unwrap() as u32;
        renders_as_saved(&views, &d, last, &saved(&d), at);
        if st.page_count() > 1 {
            let first = d.page(0).unwrap();
            let view = views.view(st, first, &ViewInputs::plugin()).unwrap();
            assert!(matches!(view.engine, EngineKey::Base(_)));
        }
    }
}

/// A base PDFium refuses (as it does the corpus's 4422502: a revision cut
/// out of the file) opens, and every page of it draws from its
/// realization, a file of its own, untouched or not (Greg, 2026-10-20). An
/// untouched page is viewed on the base engine until an engine has tried
/// the bytes: the pool's failed open tells the source, the render already
/// asked of the base engine draws from the page's group instead, and every
/// view after is its group's. A second document on the same bytes learns
/// it from its first request, told of the failure the pool remembers.
#[test]
fn a_base_pdfium_refuses_draws_every_page_from_its_realization() {
    let bytes = revision_cut_out(file(3, XrefType::CrossReferenceTable));
    let refused = PdfiumEngine::open_bytes(bytes.clone().into(), None).err();
    assert!(OpenError::is_refused(&refused.expect("PDFium refuses it")));
    let mut d = opened(bytes.clone(), "refused.pdf", None);
    let last = d.page(2).unwrap();
    let header = fx::set_header(fx::header("Draft"), Some(&[last]));
    d.commit_ops(vec![header], "Header").unwrap();
    let (views, plugin) = (views(), ViewInputs::plugin());
    let (st, first) = (d.state(), d.page(0).unwrap());
    let view = views.view(st, first, &plugin).unwrap();
    assert!(matches!(view.engine, EngineKey::Base(_)));
    assert!(!d.primary().base_refused());
    let job = Job::Page {
        width: WIDTH,
        rotation: Rotation::R0,
    };
    let (format, budget) = (PixelFormat::Rgba, Budget::Screen);
    let rx = (views.draw(st, first, &view, &plugin, job, format, 0, budget)).unwrap();
    let ours = block_on(rx).unwrap().unwrap().raster;
    assert!(d.primary().base_refused());
    let saved = saved(&d);
    assert_eq!(differing(&ours, &live(&saved, None, 0, Rotation::R0)), 0);
    for at in 0..3 {
        let page = d.page(at).unwrap();
        let (view, r) = realized(&views, &d, page, &plugin);
        assert!(matches!(view.engine, EngineKey::Group(..)), "{at}");
        assert_eq!((r.segments.len(), r.password.as_deref()), (1, None));
        renders_as_saved(&views, &d, page, &saved, at as u32);
    }
    let other = opened(bytes, "refused.pdf", None);
    let again = Views::new(views.pool().clone());
    let asked = block_on(views.pool().form_type(again.base(other.primary(), budget)));
    assert!(OpenError::is_refused(&asked.unwrap().unwrap_err()));
    assert!(other.primary().base_refused());
}

/// A file PDFium refuses prints (slice 14). Its print file is sheaf's own
/// writing, which never asks PDFium of the source: opened from memory, as
/// GDI opens it, it has every page, an unsaved header and a mark (drawn
/// into its page) included, and each draws as the Rewrite's does but for
/// anti-aliasing (realize_corpus's allowance, [`ANTI_ALIASING_PX`]).
#[test]
fn a_source_pdfium_refuses_prints() {
    let bytes = revision_cut_out(file(3, XrefType::CrossReferenceTable));
    let refused = PdfiumEngine::open_bytes(bytes.clone().into(), None).err();
    assert!(OpenError::is_refused(&refused.expect("PDFium refuses it")));
    let mut d = opened(bytes, "refused.pdf", None);
    let [first, last] = [0, 2].map(|i| d.page(i).unwrap());
    let ops = vec![
        fx::set_header(fx::header("Draft"), Some(&[last])),
        fx::add(first, &every_mark_kind()[3]),
    ];
    d.commit_ops(ops, "Edit").unwrap();
    let pages: Vec<PageId> = d.state().order().iter().copied().collect();
    let print = |markup| {
        let printed = d.export_bytes(|f, _| Export::print(f, &pages, markup));
        PdfiumEngine::open_bytes(printed.into(), None).unwrap()
    };
    let job = Job::Page {
        width: WIDTH,
        rotation: Rotation::R0,
    };
    let page = |engine: &PdfiumEngine, at| engine.render(PageIndex(at), job, PixelFormat::Rgba);
    let (marked, bare) = (print(true), print(false));
    assert_eq!(marked.page_count(), 3);
    let saved = saved(&d);
    for at in 0..3 {
        let theirs = live(&saved, None, at, Rotation::R0);
        let differ = differing(&page(&marked, at).unwrap(), &theirs);
        assert!(differ <= ANTI_ALIASING_PX, "page {at}: {differ} px differ");
    }
    let [with, without] = [&marked, &bare].map(|e| page(e, 0).unwrap());
    assert!(differing(&with, &without) > 0, "the mark prints");
}

/// Print preview draws what prints (§16 row 28): what it realizes of a
/// page ([`print_preview`]) under print's inputs ([`ViewInputs::print`]),
/// with markup and without, draws as the print file ([`Export::print`])
/// draws the page. On the page another tool's comment that prints, one
/// hidden, one never shown on screen (NoView), one that doesn't print, a
/// filled field, and a check box whose state names no appearance, which
/// the print file draws nothing of with markup (M10 bug 10) where PDFium's
/// form filler draws a box of its own: the preview draws it flattened.
#[test]
fn print_preview_draws_what_prints() {
    let mut f = Form::new(1);
    let name = f.text("Name", 0, [72.0, 700.0, 272.0, 720.0]);
    f.fill(name, "Ann");
    let boxed = f.check_box("Box", 0, [300.0, 700.0, 320.0, 720.0], true);
    f.dict(boxed).remove(b"AS");
    let page = f.pages[0];
    let mut annots = f.annots(0);
    for (flags, y) in [(4, 100), (4 | 2, 200), (4 | 32, 300), (0, 400)] {
        let form = f.pdf.add_object(Stream::new(
            dictionary! {
                "Type" => "XObject", "Subtype" => "Form",
                "BBox" => vec![0.into(), 0.into(), 100.into(), 40.into()],
            },
            b"0 0 1 rg 0 0 100 40 re f".to_vec(),
        ));
        annots.push(f.pdf.add_object(dictionary! {
            "Type" => "Annot", "Subtype" => "Square", "P" => page, "F" => flags,
            "Rect" => vec![72.into(), y.into(), 172.into(), (y + 40).into()],
            "AP" => dictionary! { "N" => form },
        }));
    }
    f.set_annots(0, &annots);
    let d = opened(f.bytes(), "form.pdf", None);
    let first = d.page(0).unwrap();
    let views = views();
    let job = Job::Page {
        width: WIDTH,
        rotation: Rotation::R0,
    };
    for markup in [true, false] {
        let st = print_preview(&d.export_from(Actor::User), first, markup).unwrap();
        let (inputs, format) = (ViewInputs::print(markup), PixelFormat::Rgba);
        let rx = views.render(&st, first, &inputs, job, format, 0, Budget::Screen);
        let preview = block_on(rx).unwrap().unwrap().raster;
        let printed = d.export_bytes(|f, _| Export::print(f, &[first], markup));
        let printed = live(&printed, None, 0, Rotation::R0);
        let differ = differing(&preview, &printed);
        assert_eq!(differ, 0, "markup {markup}: {differ} px differ");
    }
}

/// Print preview of pages printed from the middle draws what prints (Greg,
/// 2026-10-22): pages 5 and 6 of 10 under a header of their labels, "Page
/// <<1>> of <<N>>" and Bates numbers, a mark on page 5, print as the
/// preview shows them, "Page 5 of 10" and "Page 6 of 10", with markup and
/// without: each previewed page draws as the print file's page does.
#[test]
fn print_preview_of_pages_from_the_middle_draws_what_prints() {
    let mut d = opened(file(10, XrefType::CrossReferenceTable), "ten.pdf", None);
    let (fifth, sixth) = (d.page(4).unwrap(), d.page(5).unwrap());
    let mut spec = fx::header("<<label>>");
    spec.texts[2] = "<<bates>>".into();
    spec.texts[4] = "Page <<1>> of <<N>>".into();
    spec.bates = Some(sheaf_edit::BatesSpec {
        prefix: "SHF".into(),
        suffix: String::new(),
        digits: 4,
        start: 1,
    });
    let labels = sheaf_edit::Op::SetPageLabels {
        ranges: Some(vec![fx::label(
            d.page(0).unwrap(),
            sheaf_edit::LabelStyle::LowerRoman,
            "",
            1,
        )]),
    };
    let ops = vec![
        fx::set_header(spec, None),
        labels,
        fx::add(fifth, &every_mark_kind()[3]),
    ];
    d.commit_ops(ops, "Edit").unwrap();
    let views = views();
    let job = Job::Page {
        width: WIDTH,
        rotation: Rotation::R0,
    };
    for markup in [true, false] {
        let printed = d.export_bytes(|f, _| Export::print(f, &[fifth, sixth], markup));
        for (at, page) in [fifth, sixth].into_iter().enumerate() {
            let st = print_preview(&d.export_from(Actor::User), page, markup).unwrap();
            let (inputs, format) = (ViewInputs::print(markup), PixelFormat::Rgba);
            let rx = views.render(&st, page, &inputs, job, format, 0, Budget::Screen);
            let preview = block_on(rx).unwrap().unwrap().raster;
            let print = live(&printed, None, at as u32, Rotation::R0);
            let differ = differing(&preview, &print);
            assert_eq!(differ, 0, "markup {markup}, page {at}: {differ} px differ");
        }
    }
}

/// `bytes` behind 2,000 bytes of junk: sheaf finds the header wherever it
/// is and reads the file as sound, where PDFium looks for it in the first
/// 1,024 bytes alone and refuses the file.
fn behind_junk(bytes: Vec<u8>) -> Vec<u8> {
    let mut junk = vec![b'x'; 2000];
    junk.extend(bytes);
    junk
}

/// A sound base PDFium refuses draws from its groups whatever asks first
/// (Greg, 2026-10-20). An edited page viewed in its group, a section over
/// the refused bytes, before anything has asked the base engine: the
/// section's failed open tells the source, and the render draws from the
/// page's group found again, a file of its own; the page left alone,
/// viewed on the base engine before that, draws from its group too. And a
/// page whose view turns a layer off, viewed on the source's patch before
/// the base engine's render told the source: the patch fails as refused
/// without opening, and the page draws from its group, the layer off.
#[test]
fn a_sound_base_pdfium_refuses_draws_from_its_groups() {
    let bytes = behind_junk(file(2, XrefType::CrossReferenceTable));
    let refused = PdfiumEngine::open_bytes(bytes.clone().into(), None).err();
    assert!(OpenError::is_refused(&refused.expect("PDFium refuses it")));
    let mut d = opened(bytes, "junk.pdf", None);
    assert!(d.primary().base_sound());
    let [alone, edited] = [0, 1].map(|i| d.page(i).unwrap());
    let header = fx::set_header(fx::header("Draft"), Some(&[edited]));
    d.commit_ops(vec![header], "Header").unwrap();
    let (views, plugin, st) = (views(), ViewInputs::plugin(), d.state());
    let [first, second] = [edited, alone].map(|p| views.view(st, p, &plugin).unwrap());
    assert!(matches!(first.engine, EngineKey::Group(..)));
    assert!(matches!(second.engine, EngineKey::Base(_)));
    let job = Job::Page {
        width: WIDTH,
        rotation: Rotation::R0,
    };
    let (format, budget) = (PixelFormat::Rgba, Budget::Screen);
    let saved = saved(&d);
    for (page, view, at) in [(edited, first, 1), (alone, second, 0)] {
        let rx = (views.draw(st, page, &view, &plugin, job, format, 0, budget)).unwrap();
        let ours = block_on(rx).unwrap().unwrap().raster;
        assert!(d.primary().base_refused());
        assert_eq!(differing(&ours, &live(&saved, None, at, Rotation::R0)), 0);
        let again = views.view(st, page, &plugin).unwrap();
        assert!(matches!(again.engine, EngineKey::Group(..)));
    }
    let again = views.view(st, edited, &plugin).unwrap();
    assert_ne!(again.engine, first.engine, "a file of its own");

    let (bytes, [blue, _]) = layered(false);
    let d = opened(behind_junk(bytes), "layers.pdf", None);
    let (p0, blob) = (d.page(0).unwrap(), d.primary().id());
    let inputs = ViewInputs {
        ocg_off: Some(Arc::new(OcgOverride {
            off: [(blob, [LayerId::new(blue)].into())].into(),
        })),
        ..ViewInputs::screen()
    };
    let (views, st) = (Views::new(views.pool().clone()), d.state());
    let patched = views.view(st, p0, &inputs).unwrap();
    assert!(matches!(patched.engine, EngineKey::SourcePatch(..)));
    drawn(&views, &d, p0, &ViewInputs::screen(), Rotation::R0);
    assert!(d.primary().base_refused());
    let rx = (views.draw(st, p0, &patched, &inputs, job, format, 0, budget)).unwrap();
    let ours = block_on(rx).unwrap().unwrap().raster;
    let (hidden, _) = layered(true);
    assert_eq!(differing(&ours, &live(&hidden, None, 0, Rotation::R0)), 0);
    let open = views.pool().open_engines();
    assert!(!open.contains(&patched.engine), "{open:?}");
}

/// Over a base whose newest cross-reference stream is past its `/Size`'s
/// other objects (as lopdf writes one, and as slice 2's 5801985 is), the
/// section numbers what's new past `/Size` and the stream, which PDFium
/// would otherwise read as the stream; the page renders as saved.
#[test]
fn allocator_numbers_above_size_and_xref_streams() {
    let mut d = opened(file(2, XrefType::CrossReferenceStream), "a.pdf", None);
    let src = d.primary().clone();
    let stream = src.xref().max_stream.unwrap();
    assert!(
        stream + 1 >= src.xref().size,
        "lopdf numbers its stream last"
    );
    let p0 = d.page(0).unwrap();
    let header = fx::set_header(fx::header("Draft"), Some(&[p0]));
    d.commit_ops(vec![header], "Header").unwrap();
    let views = views();
    let (_, r) = realized(&views, &d, p0, &ViewInputs::plugin());
    let bytes = r.segments.concat();
    let pdf = LazyPdf::open(Arc::from(bytes.as_slice()), None, &ReadLimits::DEFAULT).unwrap();
    let written = numbered(&r.segments[1]);
    let own: HashSet<u32> = src.pdf().ids().map(|id| id.0).collect();
    let fresh: Vec<u32> = written.into_iter().filter(|n| !own.contains(n)).collect();
    assert!(!fresh.is_empty());
    assert!(fresh.iter().all(|&n| n > stream), "{fresh:?} past {stream}");
    assert!(pdf.xref().max_stream.unwrap() > stream);
    renders_as_saved(&views, &d, p0, &saved(&d), 0);
}

/// One page drawing a blue square in one layer and a red one in another,
/// both shown by default: the bytes, and the two layers. With `off`, the
/// first layer is off in the file's default configuration.
fn layered(off: bool) -> (Vec<u8>, [ObjectId; 2]) {
    let mut pdf = Pdf::with_version("1.7");
    let root = pdf.new_object_id();
    let ocg = |pdf: &mut Pdf, name: &str| {
        pdf.add_object(dictionary! { "Type" => "OCG", "Name" => Object::string_literal(name) })
    };
    let ids = [ocg(&mut pdf, "Blue"), ocg(&mut pdf, "Red")];
    let content = b"/OC /L0 BDC 0 0 1 rg 100 500 200 200 re f EMC \
                    /OC /L1 BDC 1 0 0 rg 300 100 150 150 re f EMC";
    let content = pdf.add_object(Stream::new(dictionary! {}, content.to_vec()));
    let page = pdf.add_object(dictionary! {
        "Type" => "Page", "Parent" => root, "Contents" => content,
        "Resources" => dictionary! { "Properties" => dictionary! { "L0" => ids[0], "L1" => ids[1] } },
    });
    pdf.objects.insert(
        root,
        Object::Dictionary(dictionary! {
            "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        }),
    );
    let mut config = dictionary! { "Order" => ids.map(Object::from).to_vec() };
    if off {
        config.set("OFF", vec![ids[0].into()]);
    }
    let props = dictionary! { "OCGs" => ids.map(Object::from).to_vec(), "D" => config };
    let catalog = pdf.add_object(dictionary! {
        "Type" => "Catalog", "Pages" => root, "OCProperties" => props,
    });
    pdf.trailer.set("Root", catalog);
    let mut bytes = Vec::new();
    pdf.save_to(&mut bytes).unwrap();
    (bytes, ids)
}

/// A layer the view turns off is a patch of its source's catalog: an
/// untouched page renders from the source and the patch, without that
/// layer, as the file with the layer off in its default configuration
/// does; an edited page's group carries the patch too.
#[test]
fn a_hidden_layer_s_patch_hides_its_content() {
    let (bytes, [blue, _]) = layered(false);
    let mut d = opened(bytes, "layers.pdf", None);
    let p0 = d.page(0).unwrap();
    let blob = d.primary().id();
    let off = OcgOverride {
        off: [(blob, [LayerId::new(blue)].into())].into(),
    };
    let inputs = ViewInputs {
        ocg_off: Some(Arc::new(off)),
        ..ViewInputs::screen()
    };
    let views = views();
    let view = views.view(d.state(), p0, &inputs).unwrap();
    assert!(matches!(view.engine, EngineKey::SourcePatch(b, _) if b == blob));
    let (hidden, _) = layered(true);
    let want = live(&hidden, None, 0, Rotation::R0);
    let patched = drawn(&views, &d, p0, &inputs, Rotation::R0);
    assert_eq!(differing(&patched, &want), 0);
    let all = live(d.primary().bytes(), None, 0, Rotation::R0);
    assert_ne!(differing(&all, &want), 0, "the layer draws");
    // An edited page's group: the header drawn, the layer still off.
    let header = fx::set_header(fx::header("Draft"), Some(&[p0]));
    d.commit_ops(vec![header], "Header").unwrap();
    let view = views.view(d.state(), p0, &inputs).unwrap();
    assert!(matches!(view.engine, EngineKey::Group(..)));
    let ours = drawn(&views, &d, p0, &inputs, Rotation::R0);
    let unpatched = drawn(&views, &d, p0, &ViewInputs::screen(), Rotation::R0);
    assert_eq!(differing(&ours, &unpatched), differing(&all, &want));
}

/// A square another tool drew, with its appearance, at `x`.
fn their_square(pdf: &mut Pdf, name: &str, x: i64) -> ObjectId {
    let ap = pdf.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject", "Subtype" => "Form",
            "BBox" => vec![0.into(), 0.into(), 80.into(), 80.into()],
        },
        b"0 0.6 0 rg 0 0 80 80 re f".to_vec(),
    ));
    pdf.add_object(dictionary! {
        "Type" => "Annot", "Subtype" => "Square", "NM" => Object::string_literal(name),
        "Rect" => vec![x.into(), 300.into(), (x + 80).into(), 380.into()],
        "C" => vec![0.into(), 0.6.into(), 0.into()], "AP" => dictionary! { "N" => ap },
    })
}

/// A comment hidden on its page is left out of that page's realization
/// and not its duplicate's: the page renders as the Rewrite with the
/// comment deleted from it does, and the duplicate with it.
#[test]
fn hiding_a_comment_hides_it_on_its_page_not_its_duplicate() {
    let mut f = Form::new(1);
    let square = their_square(&mut f.pdf, "theirs", 200);
    f.set_annots(0, &[square]);
    let mut d = opened(f.bytes(), "comment.pdf", None);
    let p0 = d.page(0).unwrap();
    d.commit_ops(vec![fx::duplicate(&[p0])], "Duplicate")
        .unwrap();
    let copy = d.page(1).unwrap();
    let (comment, _) = d.state().foreign_on(p0).next().unwrap();
    let hidden = ViewInputs {
        hidden: Arc::new([comment].into_iter().collect()),
        ..ViewInputs::plugin()
    };
    let views = views();
    let [page, dup] = [p0, copy].map(|p| appearances_of(&views, &d, p, &hidden));
    let [page_live, dup_live] = [p0, copy].map(|p| drawn(&views, &d, p, &hidden, Rotation::R0));
    d.commit_ops(vec![fx::delete_annots(&[comment])], "Delete")
        .unwrap();
    let saved = saved(&d);
    assert_eq!(differing(&page, &appearances(&saved, None, 0)), 0);
    assert_eq!(differing(&dup, &appearances(&saved, None, 1)), 0);
    assert_ne!(differing(&page, &dup), 0, "the duplicate shows it");
    assert_eq!(
        differing(&page_live, &live(&saved, None, 0, Rotation::R0)),
        0
    );
    assert_eq!(
        differing(&dup_live, &live(&saved, None, 1, Rotation::R0)),
        0
    );
}

/// A crop of a page with another tool's comment, a link with its
/// appearance and a widget renders each once, as the Rewrite does; and
/// for OCR, its content alone, as the Rewrite's page drawn without them.
#[test]
fn a_cropped_page_s_comments_render_once_as_saved() {
    let mut f = Form::new(2);
    common::blue(&mut f, "name", 0, 2, "Ann");
    let square = their_square(&mut f.pdf, "theirs", 300);
    let ap = f.pdf.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject", "Subtype" => "Form",
            "BBox" => vec![0.into(), 0.into(), 100.into(), 20.into()],
        },
        b"1 0 1 RG 2 w 1 1 98 18 re S".to_vec(),
    ));
    let link = f.pdf.add_object(dictionary! {
        "Type" => "Annot", "Subtype" => "Link",
        "Rect" => vec![300.into(), 500.into(), 400.into(), 520.into()],
        "Dest" => vec![f.pages[1].into(), Object::Name(b"Fit".to_vec())],
        "AP" => dictionary! { "N" => ap },
    });
    let mut annots = f.annots(0);
    annots.extend([square, link]);
    f.set_annots(0, &annots);
    let mut d = opened(f.bytes(), "crop.pdf", None);
    let p0 = d.page(0).unwrap();
    let margins = Margins {
        top: 40.0,
        right: 30.0,
        bottom: 50.0,
        left: 20.0,
    };
    crop(&mut d, &[p0], margins);
    let views = views();
    let view = views.view(d.state(), p0, &ViewInputs::plugin()).unwrap();
    assert!(matches!(view.engine, EngineKey::Group(..)));
    let saved = saved(&d);
    renders_as_saved(&views, &d, p0, &saved, 0);
    let ours = drawn(&views, &d, p0, &ViewInputs::content_only(), Rotation::R0);
    let page = Job::Page {
        width: WIDTH,
        rotation: Rotation::R0,
    };
    let nothing = Draw {
        annots: false,
        forms: false,
    };
    let engine = PdfiumEngine::open_bytes(saved.as_slice().into(), None).unwrap();
    let theirs = (engine.render_with(PageIndex(0), page, PixelFormat::Rgba, nothing)).unwrap();
    assert_eq!(differing(&ours, &theirs), 0);
    let all = live(&saved, None, 0, Rotation::R0);
    assert_ne!(differing(&ours, &all), 0, "they're left out");
}

/// sheaf's marks on a page, and copies another tool made of them on the
/// next, which the open gives new ids and names: the screen draws them
/// itself, so their pages' realizations leave every one out, as the
/// Rewrite with the marks deleted shows the pages; PDFium draws them for a
/// plugin, as the Rewrite does.
#[test]
fn renamed_marks_are_left_out_under_by_host() {
    let every = every_mark_kind();
    let marks = [every[1].clone(), every[3].clone()].map(Mark::from);
    let mut f = Form::new(2);
    let mut page = f.dict(f.pages[0]).clone();
    write_marks(&mut f.pdf, &mut page, &marks);
    *f.dict(f.pages[0]) = page;
    let copies: Vec<ObjectId> = (f.annots(0).into_iter())
        .map(|id| {
            let copy: Dictionary = f.pdf.get_dictionary(id).unwrap().clone();
            f.pdf.add_object(copy)
        })
        .collect();
    f.set_annots(1, &copies);
    let mut d = opened(f.bytes(), "marks.pdf", None);
    let [p0, p1] = [0, 1].map(|i| d.page(i).unwrap());
    let st = d.state();
    let ids: Vec<_> = [p0, p1]
        .iter()
        .flat_map(|&p| st.marks_on(p).map(|a| a.id))
        .collect();
    assert_eq!(ids.len(), 4);
    let views = views();
    let with = saved(&d);
    let screen = ViewInputs::screen();
    let mut shown = Vec::new();
    for p in [p0, p1] {
        let view = views.view(st, p, &screen).unwrap();
        assert!(matches!(view.engine, EngineKey::Group(..)), "{view:?}");
        let live = drawn(&views, &d, p, &screen, Rotation::R0);
        shown.push((appearances_of(&views, &d, p, &screen), live));
    }
    for (i, p) in [p0, p1].into_iter().enumerate() {
        renders_as_saved(&views, &d, p, &with, i as u32);
    }
    d.commit_ops(vec![fx::delete_annots(&ids)], "Delete")
        .unwrap();
    let without = saved(&d);
    for (i, (ours, ours_live)) in shown.iter().enumerate() {
        let theirs = appearances(&without, None, i as u32);
        assert_eq!(differing(ours, &theirs), 0, "page {i}");
        let theirs = live(&without, None, i as u32, Rotation::R0);
        assert_eq!(differing(ours_live, &theirs), 0, "page {i}, live");
    }
}

/// OCR reads a page as it shows, its content alone ([`ViewPages`]): a
/// page turned a quarter, with an unsaved header and a mark, and its
/// duplicate, draw as the file's page does turned, without either or the
/// page's other tool's comment, the copy from its group's engine; and their
/// text is the file's page's. A run that skips pages with text reads both:
/// the header, long enough to count as text, isn't read.
#[test]
fn ocr_reads_a_turned_page_as_it_shows_its_content_alone() {
    const HEADER: &str = "Draft, for the board's review";
    let mut f = Form::new(1);
    let square = their_square(&mut f.pdf, "theirs", 200);
    f.set_annots(0, &[square]);
    let bytes = f.bytes();
    let mut d = opened(bytes.clone(), "turned.pdf", None);
    let p0 = d.page(0).unwrap();
    let ops = vec![
        fx::set_header(fx::header(HEADER), Some(&[p0])),
        fx::add(p0, &every_mark_kind()[3]),
        fx::rotate(&[p0], Turn::Quarter),
        fx::duplicate(&[p0]),
    ];
    d.commit_ops(ops, "Edit").unwrap();
    let copy = d.page(1).unwrap();
    let views = views();
    let content = ViewInputs::content_only();
    let view = |p| views.view(d.state(), p, &content).unwrap().engine;
    assert!(matches!(view(p0), EngineKey::Base(_)));
    assert!(matches!(view(copy), EngineKey::Group(..)));
    let engine = PdfiumEngine::open_bytes(bytes.into(), None).unwrap();
    let turned = Job::Page {
        width: WIDTH,
        rotation: Rotation::R90,
    };
    let nothing = Draw {
        annots: false,
        forms: false,
    };
    let theirs = (engine.render_with(PageIndex(0), turned, PixelFormat::Rgba, nothing)).unwrap();
    assert!(theirs.width > theirs.height, "turned");
    let len = engine.page_text_len(PageIndex(0)).unwrap();
    let pages = ViewPages::new(views, d.snapshot());
    for p in [p0, copy] {
        let page = pages.page(p).unwrap();
        assert_eq!(differing(&pages.render(&page, WIDTH).unwrap(), &theirs), 0);
        assert_eq!(pages.text_len(&page).unwrap(), len);
    }
    assert!(len < HAS_TEXT_MIN_CHARS && HEADER.len() >= HAS_TEXT_MIN_CHARS);
    let req = OcrRequest {
        pages: [p0, copy].map(|p| pages.page(p).unwrap()).into(),
        skip_pages_with_text: true,
        engine: EngineKind::Tesseract,
        config: OcrConfig {
            dpi: 150,
            ..OcrConfig::default()
        },
    };
    let factory: EngineFactory = Arc::new(|| Ok(Box::new(Blind) as _));
    let (_job, rx) = sheaf_ocr::job::start_with(req, Arc::new(pages), factory, 1);
    let events: Vec<OcrEvent> = block_on(rx.collect());
    let both_read = matches!(
        events.last(),
        Some(OcrEvent::Finished {
            done: 2,
            skipped: 0,
            failed: 0,
            cancelled: false
        })
    );
    assert!(both_read, "{events:?}");
}

/// A page's own recognized text counts as text whether it's saved or not
/// ([`ViewPages`]): a run that skips pages with text skips a page whose
/// text was recognized before, in this session and on the file saved and
/// opened again, which read the same.
#[test]
fn ocr_skips_a_page_recognized_before_saved_or_not() {
    const TEXT: &str = "Recognized_before_and_long_enough";
    let mut d = opened(file(1, XrefType::CrossReferenceTable), "a.pdf", None);
    let p0 = d.page(0).unwrap();
    let views = views();
    let before = ViewPages::new(views.clone(), d.snapshot());
    let len = before.text_len(&before.page(p0).unwrap()).unwrap();
    assert!(len < HAS_TEXT_MIN_CHARS && TEXT.len() >= HAS_TEXT_MIN_CHARS);
    let word = sheaf_doc::OcrWord {
        text: TEXT.into(),
        rect: sheaf_doc::Box2 {
            x: 72.0,
            y: 72.0,
            w: 300.0,
            h: 20.0,
        },
        confidence: 0.9,
    };
    let layer = Arc::new(TextLayer {
        words: vec![word],
        lines: std::iter::once(0..1).collect(),
        upright: Rotation::R0,
        engine: "ocr".into(),
    });
    d.commit_ops(fx::ocr([(p0, layer)]), "OCR").unwrap();
    let reopened = opened(saved(&d), "a.pdf", None);
    let mut lens = Vec::new();
    for d in [&d, &reopened] {
        let p0 = d.page(0).unwrap();
        let pages = ViewPages::new(views.clone(), d.snapshot());
        lens.push(pages.text_len(&pages.page(p0).unwrap()).unwrap());
        let req = OcrRequest {
            pages: vec![pages.page(p0).unwrap()],
            skip_pages_with_text: true,
            engine: EngineKind::Tesseract,
            config: OcrConfig {
                dpi: 150,
                ..OcrConfig::default()
            },
        };
        let factory: EngineFactory = Arc::new(|| Ok(Box::new(Blind) as _));
        let (_job, rx) = sheaf_ocr::job::start_with(req, Arc::new(pages), factory, 1);
        let events: Vec<OcrEvent> = block_on(rx.collect());
        let skipped = matches!(
            events.last(),
            Some(OcrEvent::Finished {
                done: 0,
                skipped: 1,
                failed: 0,
                cancelled: false
            })
        );
        assert!(skipped, "{events:?}");
    }
    assert!(lens[0] >= len + TEXT.len(), "{lens:?}");
    assert_eq!(lens[0], lens[1], "read the same, saved or not");
}

/// OCR in the background closes none of the screen's engines (M10.5
/// §10.3): with six group engines open for a pane, four pinned for what it
/// shows and two it prefetched, a run over copies in three other groups,
/// each drawn, and its text read, with its content alone by its group's
/// engine through [`ViewPages`], ends with every one of the six open, and
/// no more engines of its own than the background's.
#[test]
fn background_ocr_doesn_t_evict_visible_engines() {
    let n = 9 * GROUP as usize;
    let mut d = opened(file(n, XrefType::CrossReferenceTable), "a.pdf", None);
    let firsts: Vec<PageId> = (0..9)
        .map(|g| d.page(g * GROUP as usize).unwrap())
        .collect();
    let (shown, read) = firsts.split_at(6);
    let header = fx::set_header(fx::header("Draft"), Some(shown));
    d.commit_ops(vec![header, fx::duplicate(read)], "Edit")
        .unwrap();
    let st = d.state();
    let copies: Vec<PageId> = (st.pages())
        .filter(|(_, def, _)| def.copy > 0)
        .map(|(id, _, _)| id)
        .collect();
    assert_eq!(copies.len(), 3);
    let views = views();
    let screen = ViewInputs::screen();
    let engines: Vec<EngineKey> = (shown.iter())
        .map(|&p| views.view(st, p, &screen).unwrap().engine)
        .collect();
    let screen_s: HashSet<EngineKey> = engines.iter().copied().collect();
    assert_eq!(screen_s.len(), 6);
    assert!(screen_s.iter().all(|e| matches!(e, EngineKey::Group(..))));
    let pane = views.pool().owner();
    views.pool().pin(pane, engines[..4].to_vec());
    for &page in shown {
        drawn(&views, &d, page, &screen, Rotation::R0);
    }
    let open: HashSet<EngineKey> = views.pool().open_engines().into_iter().collect();
    assert_eq!(open, screen_s);
    for &page in &copies {
        let view = views.view(st, page, &ViewInputs::content_only()).unwrap();
        assert!(matches!(view.engine, EngineKey::Group(..)), "{view:?}");
    }
    let pages = ViewPages::new(views.clone(), d.snapshot());
    let req = OcrRequest {
        pages: copies.iter().map(|&p| pages.page(p).unwrap()).collect(),
        skip_pages_with_text: true,
        engine: EngineKind::Tesseract,
        config: OcrConfig {
            dpi: 150,
            ..OcrConfig::default()
        },
    };
    let factory: EngineFactory = Arc::new(|| Ok(Box::new(Blind) as _));
    let (_job, rx) = sheaf_ocr::job::start_with(req, Arc::new(pages), factory, 1);
    let events: Vec<OcrEvent> = block_on(rx.collect());
    let all_read = matches!(
        events.last(),
        Some(OcrEvent::Finished {
            done: 3,
            skipped: 0,
            failed: 0,
            cancelled: false
        })
    );
    assert!(all_read, "{events:?}");
    let after: HashSet<EngineKey> = views.pool().open_engines().into_iter().collect();
    assert!(after.is_superset(&screen_s), "{after:?}");
    let theirs = after.len() - screen_s.len();
    assert!(theirs <= BACKGROUND_ENGINES, "{after:?}");
}
