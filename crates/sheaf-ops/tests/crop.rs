//! Cropping a page: the edit, a box of the page's alone, and what a save
//! writes: the marks, OCR words and bookmarks on the page keep their
//! values in PageSpace and land on the same content as PDFium draws it,
//! saved both ways, and an Append writes the page's dictionary alone; and
//! Trim White, which measures each page through the document's views as
//! its file crops it, and commits its crops as a planner does.
//!
//! Needs libpdfium (`scripts/fetch-pdfium.sh`), like the app itself.

mod common;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use common::open;
use lopdf::{Dictionary, Document as Pdf, Stream, dictionary};
use sheaf_doc::content::{PagOrigin, PaginationKind};
use sheaf_doc::geom::Rect;
use sheaf_doc::materialize::{How, StrategyKind};
use sheaf_doc::prim::Key;
use sheaf_doc::realize::{EngineKey, ViewInputs};
use sheaf_doc::serialize::fixture::revision_cut_out;
use sheaf_doc::tx::fixture::{
    add, crop_to, header, ocr, rotate, set_header, set_watermark, watermark,
};
use sheaf_doc::{
    Actor, Annot, AnnotId, AnnotKind, Box2, Commit, CommitOpts, DestView, Document, EditError,
    JobKind, OcrWord, PageChars, PageId, Pt, Query, Snapshot, Stale,
};
use sheaf_edit::{Margins, Op, PageRef, PageSel, Patch, Rotation, Sibling, Turn, Z};
use sheaf_image::raster::{PixelFormat, content_bounds};
use sheaf_ops::NoProgress;
use sheaf_ops::crop::{self, Trimmed};
use sheaf_ops::views::Views;
use sheaf_render::pdfium::PdfiumEngine;
use sheaf_render::{Job, OpenError, PageIndex, Renderer};

/// A black square at user (300..400, 300..400).
const SQUARE: &[u8] = b"0 0 0 rg 300 300 100 100 re f";

/// The square, in PageSpace.
const SQUARE_BOX: Box2 = Box2 {
    x: 300.0,
    y: 300.0,
    w: 100.0,
    h: 100.0,
};

/// A Letter page's keys.
fn letter() -> Dictionary {
    dictionary! {
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
    }
}

/// A Letter page with a black square at user (300..400, 300..400).
fn pdf(dir: &Path) -> PathBuf {
    one_page(dir, "square.pdf", letter(), SQUARE)
}

/// A Letter-sized page whose media box is off the origin, with a line of
/// Helvetica at user (100, 500).
fn text_pdf(dir: &Path) -> PathBuf {
    let font = dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica" };
    let page = dictionary! {
        "MediaBox" => vec![(-37).into(), 52.into(), 575.into(), 844.into()],
        "Resources" => dictionary! { "Font" => dictionary! { "F1" => font } },
    };
    let text = b"BT /F1 24 Tf 100 500 Td (Glyphs under sheaf ink) Tj ET";
    one_page(dir, "text.pdf", page, text)
}

/// `dir/name`: one page, `page`'s keys and `content`.
fn one_page(dir: &Path, name: &str, page: Dictionary, content: &[u8]) -> PathBuf {
    on_disk(dir, name, file_of(vec![(page, content)]))
}

/// A file of `pages`, each its keys and its content.
fn file_of(pages: Vec<(Dictionary, &[u8])>) -> Vec<u8> {
    let mut pdf = Pdf::with_version("1.5");
    let pages_id = pdf.new_object_id();
    let kids: Vec<lopdf::Object> = (pages.into_iter())
        .map(|(mut page, content)| {
            let content = pdf.add_object(Stream::new(dictionary! {}, content.to_vec()));
            page.set("Type", "Page");
            page.set("Parent", pages_id);
            page.set("Contents", content);
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

/// A box's numbers, as a page's keys hold them.
fn numbers(v: [i64; 4]) -> Vec<lopdf::Object> {
    v.map(Into::into).to_vec()
}

/// `bytes`, written to `dir/name`.
fn on_disk(dir: &Path, name: &str, bytes: Vec<u8>) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("sheaf-crop-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // SAFETY: each test sets the same value before any reads it.
    unsafe {
        std::env::set_var(
            "SHEAF_CACHE_DIR",
            std::env::temp_dir().join("sheaf-crop-cache"),
        )
    };
    dir
}

/// Margins off every side, none alike.
const UNEVEN: Margins = Margins {
    top: 50.0,
    right: 30.0,
    bottom: 20.0,
    left: 72.0,
};

/// A red mark of `kind` for the document to give an id, reading upright.
fn mark(kind: AnnotKind) -> Annot {
    Annot {
        id: AnnotId(0),
        kind,
        color: [255, 0, 0],
        opacity: 1.0,
        stroke: 1.0,
        author: String::new(),
        contents: String::new(),
        modified: String::new(),
        created: String::new(),
        fill: None,
        dashed: false,
        cloudy: false,
        turn: Rotation::R0,
        name: String::new(),
    }
}

/// An engine on the file at `path`.
fn engine_on(path: &Path) -> PdfiumEngine {
    PdfiumEngine::open_bytes(sheaf_util::read_file(path).unwrap(), None).unwrap()
}

/// PDFium's characters on the first page as [`PageChars::from_engine`]
/// takes them: boxes in PageSpace, and the angles they're drawn at.
fn engine_chars(engine: &PdfiumEngine) -> Vec<(char, Option<Box2>, f32)> {
    let chars = engine.page_chars(PageIndex(0)).unwrap();
    chars.into_iter().map(|c| (c.ch, c.rect, c.angle)).collect()
}

/// PDFium's glyph boxes for `word` on the first page, in PageSpace.
fn glyphs(engine: &PdfiumEngine, word: &str) -> Vec<Box2> {
    let chars = engine_chars(engine);
    let n = word.chars().count();
    let at = chars
        .windows(n)
        .position(|w| w.iter().map(|(c, ..)| *c).eq(word.chars()))
        .unwrap_or_else(|| panic!("{word:?} isn't on the page"));
    chars[at..at + n]
        .iter()
        .map(|(_, r, _)| r.unwrap())
        .collect()
}

fn crop_box(path: &Path) -> Vec<f64> {
    let pdf = Pdf::load(path).unwrap();
    let id = *pdf.get_pages().values().next().unwrap();
    pdf.get_dictionary(id)
        .unwrap()
        .get(b"CropBox")
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .map(|o| o.as_float().unwrap())
        .collect()
}

sheaf_doc::both_ways!(
    margins_crop_and_marks_follow,
    margins_crop_and_marks_follow_appended: marks_follow
);

/// A crop is a box edit: the mark over the square, in PageSpace, is what
/// it was, and the cropped page, saved `how` and drawn by PDFium, shows the
/// square where the page's frame puts the mark; reopened, the mark is
/// where it was.
fn marks_follow(how: How) {
    let dir = scratch(&format!("margins-{how:?}"));
    let src = pdf(&dir);
    let mut doc = open(&src);
    let page = doc.state().order()[0];
    let over = Box2 {
        x: 325.0,
        y: 325.0,
        w: 50.0,
        h: 50.0,
    };
    let annot = mark(AnnotKind::Square { rect: over });
    doc.commit_ops(vec![add(page, &annot)], "mark").unwrap();
    let m = Margins {
        top: 72.0,
        left: 72.0,
        right: 0.0,
        bottom: 0.0,
    };
    common::crop(&mut doc, &[page], m);
    let p = doc.state().order()[0];
    let size = doc.page_size(p);
    assert_eq!((size.width, size.height), (540.0, 720.0));
    let kept = doc.state().marks_on(page).next().unwrap();
    assert_eq!(kept.kind, AnnotKind::Square { rect: over });
    let shown = doc.geom(p).to_view().apply_box(over);
    assert_eq!((shown.x, shown.y), (253.0, 345.0));
    // Saved, the page's crop box is the shown area, and the mark is where
    // it was.
    let out = dir.join("out.pdf");
    std::fs::write(&out, how.write(&doc)).unwrap();
    assert_eq!(crop_box(&out), vec![72.0, 0.0, 612.0, 720.0], "{how:?}");
    let saved = open(&out);
    let back = saved.state().order()[0];
    let marks: Vec<&Annot> = saved.state().marks_on(back).collect();
    assert_eq!(marks.len(), 1, "{how:?}");
    assert_eq!(marks[0].kind, AnnotKind::Square { rect: over }, "{how:?}");
    let engine = engine_on(&out);
    let job = Job::Page {
        width: 540,
        rotation: Rotation::R0,
    };
    let raster = engine.render(PageIndex(0), job, PixelFormat::Rgba).unwrap();
    let (cx, cy) = (shown.x + shown.w / 2.0, shown.y + shown.h / 2.0);
    let i = ((cy as u32 * raster.width + cx as u32) * 4) as usize;
    let px = &raster.data[i..i + 3];
    assert!(
        px.iter().all(|&c| c < 60),
        "{how:?}: the square under the mark: {px:?}"
    );
    // Undo puts it back.
    doc.undo().unwrap().unwrap();
    assert_eq!(doc.page_size(doc.state().order()[0]).width, 612.0);
    let _ = std::fs::remove_dir_all(&dir);
}

sheaf_doc::both_ways!(
    a_cropped_rotated_highlight_still_covers_its_glyphs,
    a_cropped_rotated_highlight_still_covers_its_glyphs_appended: highlight_covers_its_glyphs
);

/// A highlight made over a word as the app makes one, from PDFium's glyph
/// boxes, still covers the word once its page is cropped, turned and
/// saved either way: reopened, its rects are as they were, and PDFium's
/// boxes for the word on the saved page, in PageSpace as before, lie
/// inside them.
fn highlight_covers_its_glyphs(how: How) {
    let dir = scratch(&format!("highlight-{how:?}"));
    let src = text_pdf(&dir);
    let mut doc = open(&src);
    let p = doc.state().order()[0];
    let engine = engine_on(&src);
    let chars = PageChars::from_engine(engine_chars(&engine));
    let words = chars.words();
    let (_, (a, b), _) = words.iter().find(|(w, ..)| w == "sheaf").unwrap();
    let rects = chars.rects(*a, *b);
    let annot = Annot {
        opacity: 0.6,
        turn: chars.upright,
        ..mark(AnnotKind::Highlight {
            rects: rects.clone(),
        })
    };
    doc.commit_ops(vec![add(p, &annot)], "highlight").unwrap();
    common::crop(&mut doc, &[p], UNEVEN);
    let held = doc.state().marks_on(p).next().unwrap();
    assert_eq!(
        held.kind,
        AnnotKind::Highlight {
            rects: rects.clone()
        }
    );
    let turn = rotate(&[p], Turn::Quarter);
    doc.commit_ops(vec![turn], "rotate").unwrap();
    let now = doc.geom(doc.state().order()[0]);
    let shown = (now.visible(), now.rotate);
    assert_eq!(shown.1, Rotation::R90);
    let bytes = how.write(&doc);
    let out = dir.join(format!("{how:?}.pdf"));
    std::fs::write(&out, bytes).unwrap();
    let saved = open(&out);
    let page = saved.state().order()[0];
    let geom = saved.geom(page);
    assert_eq!((geom.visible(), geom.rotate), shown, "{how:?}");
    let marks: Vec<&Annot> = saved.state().marks_on(page).collect();
    let AnnotKind::Highlight { rects: kept } = &marks[0].kind else {
        panic!("{how:?}: {:?}", marks[0].kind);
    };
    assert_eq!(kept.len(), rects.len(), "{how:?}");
    for (k, r) in kept.iter().zip(&rects) {
        assert!(close(*k, *r, 1.0 / 16.0), "{how:?}: {k:?} was {r:?}");
    }
    // PDFium's boxes on the saved page fall inside, within 1/16 pt.
    let engine = engine_on(&out);
    for g in glyphs(&engine, "sheaf") {
        assert!(
            kept.iter().any(|k| close(k.union(g), *k, 1.0 / 16.0)),
            "{how:?}: {g:?} outside {kept:?}"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// What Trim White finds on pages `pages` of `snap`, padded `pad`, each
/// drawn by `views`, for a job of `doc`'s, as the app's job resolves it.
fn trimmed(doc: &Document, snap: &Snapshot, views: &Views, pages: &[PageId], pad: f32) -> Trimmed {
    let lower = doc.lower_cx(Actor::Job(JobKind::Trim));
    let urgency = crop::Urgency::Background;
    crop::trim_white(views, snap, lower, pages, pad, urgency, &NoProgress).unwrap()
}

/// `found`'s crops, measured at `snap`, committed onto `doc` as the app's
/// job commits them: in a transaction made there, under Validate.
fn landed(
    doc: &mut Document,
    snap: &Snapshot,
    found: &Trimmed,
    label: &str,
) -> Result<Commit, EditError> {
    let mut tx = Document::tx_at(snap, doc.lower_cx(Actor::Job(JobKind::Trim)));
    found.lower(&mut tx)?;
    doc.commit(tx, CommitOpts::based(snap.version, Stale::Validate, label))
}

/// Whether `a` and `b` are within `by` points on every side.
fn close(a: Box2, b: Box2, by: f32) -> bool {
    (a.x - b.x).abs() <= by
        && (a.y - b.y).abs() <= by
        && (a.right() - b.right()).abs() <= by
        && (a.bottom() - b.bottom()).abs() <= by
}

/// A conflict on `key`, which the version labelled `by` changed.
fn conflict(key: Key, by: &str) -> EditError {
    EditError::Conflict {
        key,
        by: Some(by.into()),
    }
}

/// Page `page`'s crop box in `doc`, in PageSpace.
fn crop_of(doc: &Document, page: PageId) -> Box2 {
    let crop = doc.geom(page).crop;
    Box2 {
        x: crop.x0,
        y: crop.y0,
        w: crop.width(),
        h: crop.height(),
    }
}

/// Trim White crops a page down to its content: the square, within a
/// point, or [`crop::PAD`] more on each side padded as the app pads it.
/// Committed, the page shows the square alone.
#[test]
fn trim_white_finds_the_content() {
    let dir = scratch("trim");
    let mut doc = open(&pdf(&dir));
    let p = doc.state().order()[0];
    let (snap, views) = (doc.snapshot(), common::views());
    let found = trimmed(&doc, &snap, &views, &[p], 0.0);
    assert_eq!((found.measured.as_slice(), found.left.len()), (&[p][..], 0));
    assert_eq!(found.crops.len(), 1, "{found:?}");
    let (at, to) = found.crops[0];
    assert!(at == p && close(to, SQUARE_BOX, 1.0), "{to:?}");
    let padded = trimmed(&doc, &snap, &views, &[p], crop::PAD);
    let grown = Box2 {
        x: 300.0 - crop::PAD,
        y: 300.0 - crop::PAD,
        w: 100.0 + 2.0 * crop::PAD,
        h: 100.0 + 2.0 * crop::PAD,
    };
    assert!(close(padded.crops[0].1, grown, 1.0), "{padded:?}");
    landed(&mut doc, &snap, &found, "Trim White").unwrap();
    assert!(close(crop_of(&doc, p), SQUARE_BOX, 1.0));
    let size = doc.page_size(p);
    let square = (size.width - 100.0).abs() <= 1.0 && (size.height - 100.0).abs() <= 1.0;
    assert!(square, "{size:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Trim White measures a page as its file crops it, so a crop made in
/// sheaf never changes what it finds: with the page's top and the
/// square's left half cropped away, it still finds the whole square, in
/// PageSpace. The page, whose only change is its crop, is drawn there from
/// the file's own bytes, where the document's own view of it is its
/// group's: no group's engine opens.
#[test]
fn trim_white_measures_the_page_as_its_file_crops_it() {
    let dir = scratch("trim-cropped");
    let mut doc = open(&pdf(&dir));
    let p = doc.state().order()[0];
    let half = Box2 {
        x: 350.0,
        y: 0.0,
        w: 262.0,
        h: 700.0,
    };
    doc.commit_ops(vec![crop_to(p, half)], "Crop").unwrap();
    let views = common::views();
    let shown = (views.view(doc.state(), p, &ViewInputs::content_only())).unwrap();
    assert!(matches!(shown.engine, EngineKey::Group(..)), "{shown:?}");
    let found = trimmed(&doc, &doc.snapshot(), &views, &[p], 0.0);
    assert_eq!(found.crops.len(), 1, "{found:?}");
    assert!(close(found.crops[0].1, SQUARE_BOX, 1.0), "{found:?}");
    let open = views.pool().open_engines();
    assert_eq!(open, [EngineKey::Base(doc.primary().id())]);
    assert_eq!(views.pool().opened(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Trim White looks within the box a file crops a page to, never at what
/// its `/CropBox` hides: printer's marks in the corners outside it aren't
/// content, so the crop is the square's; and padded past that box, the
/// crop is the box, as the page is already, so there's nothing to crop.
#[test]
fn trim_white_stays_within_the_file_s_crop_box() {
    let dir = scratch("trim-marks");
    let mut page = letter();
    page.set("CropBox", numbers([36, 36, 576, 756]));
    let content = b"0 0 0 rg 300 300 100 100 re f 4 4 24 24 re f 584 764 24 24 re f";
    let doc = open(&one_page(&dir, "marks.pdf", page, content));
    let p = doc.state().order()[0];
    let (snap, views) = (doc.snapshot(), common::views());
    let found = trimmed(&doc, &snap, &views, &[p], 0.0);
    assert_eq!(found.crops.len(), 1, "{found:?}");
    assert!(close(found.crops[0].1, SQUARE_BOX, 1.0), "{found:?}");
    let padded = trimmed(&doc, &snap, &views, &[p], 400.0);
    assert!(padded.crops.is_empty(), "{padded:?}");
    assert_eq!(padded.measured, [p]);
    let _ = std::fs::remove_dir_all(&dir);
}

/// On a page its file turns a quarter, its media box off the origin, and
/// on it turned a quarter more in sheaf, Trim White's crop is the
/// square's in PageSpace: PDFium draws the saved page as the square alone.
#[test]
fn trim_white_crops_a_turned_page_in_page_space() {
    let dir = scratch("trim-turned");
    let page = dictionary! {
        "MediaBox" => vec![(-37).into(), 52.into(), 575.into(), 844.into()],
        "Rotate" => 90,
    };
    let path = one_page(&dir, "turned.pdf", page, b"0 0 0 rg 100 300 100 100 re f");
    let square = Box2 {
        x: 100.0,
        y: 300.0,
        w: 100.0,
        h: 100.0,
    };
    for turn in [None, Some(Turn::Quarter)] {
        let mut doc = open(&path);
        let p = doc.state().order()[0];
        if let Some(by) = turn {
            doc.commit_ops(vec![rotate(&[p], by)], "rotate").unwrap();
        }
        let snap = doc.snapshot();
        let found = trimmed(&doc, &snap, &common::views(), &[p], 0.0);
        assert_eq!(found.crops.len(), 1, "{turn:?}: {found:?}");
        assert!(close(found.crops[0].1, square, 1.0), "{turn:?}: {found:?}");
        landed(&mut doc, &snap, &found, "Trim White").unwrap();
        let out = dir.join("out.pdf");
        sheaf_doc::save::rewrite_to_path(&doc.snapshot(), &out).unwrap();
        let size = doc.page_size(p);
        let job = Job::Page {
            width: size.width.round() as u32,
            rotation: Rotation::R0,
        };
        let raster = engine_on(&out).render(PageIndex(0), job, PixelFormat::Rgba);
        let gray = raster.unwrap().to_gray().unwrap();
        let ink = gray.pixels().filter(|px| px[0] < 128).count();
        let all = (gray.width() * gray.height()) as usize;
        assert!(ink * 100 >= all * 95, "{turn:?}: {ink} of {all} pixels");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// What Trim White leaves as it was: a blank page; one whose content, a
/// dot, is too small to crop to; and one whose file shows less than a
/// quarter inch of it, which isn't drawn. A page it trimmed already is
/// measured with nothing to crop, so trimming again commits nothing.
#[test]
fn trim_white_leaves_what_it_can_t_crop_and_commits_nothing_more() {
    let dir = scratch("trim-left");
    let mut small = letter();
    small.set("CropBox", numbers([300, 300, 310, 310]));
    let bytes = file_of(vec![
        (letter(), b"".as_slice()),
        (letter(), b"0 0 0 rg 300 300 2 2 re f"),
        (small, SQUARE),
        (letter(), SQUARE),
    ]);
    let mut doc = open(&on_disk(&dir, "left.pdf", bytes));
    let ids: Vec<PageId> = doc.state().order().iter().copied().collect();
    let views = common::views();
    let snap = doc.snapshot();
    let found = trimmed(&doc, &snap, &views, &ids, 0.0);
    assert_eq!(found.measured, [ids[0], ids[1], ids[3]]);
    assert_eq!(found.left, [ids[0], ids[1], ids[2]]);
    let cropped: Vec<PageId> = found.crops.iter().map(|&(id, _)| id).collect();
    assert_eq!(cropped, [ids[3]]);
    let first = landed(&mut doc, &snap, &found, "Trim White").unwrap();
    assert!(first.version.is_some());
    let snap = doc.snapshot();
    let again = trimmed(&doc, &snap, &views, &ids, 0.0);
    assert_eq!((again.crops.len(), &again.measured), (0, &found.measured));
    let commit = landed(&mut doc, &snap, &again, "Trim White").unwrap();
    assert_eq!(commit.version, None);
    assert_eq!(doc.snapshot().version, snap.version);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A file PDFium refuses (as it does the corpus's 4422502) is measured
/// from its groups: Trim White crops its page to the square, and the
/// saved file's `/CropBox` is the square's.
#[test]
fn trim_white_works_on_a_source_pdfium_refuses() {
    let dir = scratch("trim-refused");
    let bytes = revision_cut_out(file_of(vec![(letter(), SQUARE)]));
    let refused = PdfiumEngine::open_bytes(bytes.clone().into(), None).err();
    assert!(OpenError::is_refused(&refused.expect("PDFium refuses it")));
    let mut doc = open(&on_disk(&dir, "refused.pdf", bytes));
    let p = doc.state().order()[0];
    let snap = doc.snapshot();
    let found = trimmed(&doc, &snap, &common::views(), &[p], 0.0);
    assert_eq!(found.crops.len(), 1, "{found:?}");
    assert!(close(found.crops[0].1, SQUARE_BOX, 1.0), "{found:?}");
    assert!(doc.primary().base_refused());
    landed(&mut doc, &snap, &found, "Trim White").unwrap();
    let out = dir.join("out.pdf");
    sheaf_doc::save::rewrite_to_path(&doc.snapshot(), &out).unwrap();
    let saved = crop_box(&out);
    let square = [300.0, 300.0, 400.0, 400.0];
    let near = saved.iter().zip(square).all(|(a, b)| (a - b).abs() <= 1.0);
    assert!(saved.len() == 4 && near, "{saved:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The corpus's form PDFium refuses, 4422502: every page is measured from
/// its group and trimmed, none failing or left as it was (each draws
/// content), each crop within the box the file crops its page to; and the
/// crops commit. Skipped without the corpus.
#[test]
fn trim_white_on_the_corpus_form_pdfium_refuses() {
    let Some(bytes) = common::corpus_form("forms/safedocs/4422502.pdf") else {
        return;
    };
    let dir = scratch("trim-4422502");
    let mut doc = open(&on_disk(&dir, "4422502.pdf", bytes));
    let ids: Vec<PageId> = doc.state().order().iter().copied().collect();
    let snap = doc.snapshot();
    let found = trimmed(&doc, &snap, &common::views(), &ids, crop::PAD);
    assert!(doc.primary().base_refused());
    assert_eq!(found.measured, ids, "{found:?}");
    assert!(found.left.is_empty(), "{found:?}");
    assert_eq!(found.crops.len(), ids.len(), "{found:?}");
    for &(id, to) in &found.crops {
        let own = crop_of(&doc, id);
        let inside = close(to.union(own), own, 1.0 / 64.0);
        assert!(inside, "{id:?}: {to:?} outside {own:?}");
    }
    landed(&mut doc, &snap, &found, "Trim White").unwrap();
    for &(id, to) in &found.crops {
        assert!(close(crop_of(&doc, id), to, 1.0 / 16.0), "{id:?}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// Of two Trim White jobs on overlapping pages, planned on one version,
/// the second to land conflicts on the first page they share, which the
/// first cropped, and changes nothing. Planned again on the version the
/// first made, it crops only the page the first didn't, and lands: every
/// page holds its trim, the shared ones the first's.
#[test]
fn of_two_overlapping_trims_the_second_conflicts_and_re_plans() {
    let dir = scratch("trim-overlap");
    let squares: Vec<Vec<u8>> = (0..4)
        .map(|i| format!("0 0 0 rg {} 300 100 100 re f", 100 + 50 * i).into_bytes())
        .collect();
    let pages = squares.iter().map(|c| (letter(), c.as_slice())).collect();
    let mut doc = open(&on_disk(&dir, "four.pdf", file_of(pages)));
    let ids: Vec<PageId> = doc.state().order().iter().copied().collect();
    let views = common::views();
    let snap = doc.snapshot();
    let a = trimmed(&doc, &snap, &views, &ids[..3], 0.0);
    let b = trimmed(&doc, &snap, &views, &ids[1..], 0.0);
    landed(&mut doc, &snap, &a, "Trim White A").unwrap();
    let after_a = doc.snapshot();
    let shared = [ids[1], ids[2]].map(|id| doc.geom(id).crop);
    let e = landed(&mut doc, &snap, &b, "Trim White B").unwrap_err();
    assert_eq!(e, conflict(Key::Geom(ids[1]), "Trim White A"));
    assert_eq!(doc.snapshot().version, after_a.version);
    let again = trimmed(&doc, &after_a, &views, &ids[1..], 0.0);
    let cropped: Vec<PageId> = again.crops.iter().map(|&(id, _)| id).collect();
    assert_eq!(cropped, [ids[3]]);
    landed(&mut doc, &after_a, &again, "Trim White B").unwrap();
    for (i, &id) in ids.iter().enumerate() {
        let square = Box2 {
            x: 100.0 + 50.0 * i as f32,
            y: 300.0,
            w: 100.0,
            h: 100.0,
        };
        assert!(close(crop_of(&doc, id), square, 1.0), "page {i}");
    }
    assert_eq!([ids[1], ids[2]].map(|id| doc.geom(id).crop), shared);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Trim White decides by what each page shows: recognized text set on a
/// page after it was measured changes that, so the trim's commit
/// conflicts on the page, though nothing else changed its boxes.
#[test]
fn a_trim_conflicts_with_a_change_of_what_its_page_shows() {
    let dir = scratch("trim-ocr");
    let mut doc = open(&pdf(&dir));
    let p = doc.state().order()[0];
    let snap = doc.snapshot();
    let found = trimmed(&doc, &snap, &common::views(), &[p], 0.0);
    let word = OcrWord {
        text: "square".into(),
        rect: SQUARE_BOX,
        confidence: 1.0,
    };
    let layer = sheaf_doc::text_layer::from_engine(vec![word], &doc.geom(p), "test");
    doc.commit_ops(ocr([(p, Arc::new(layer))]), "Recognize text")
        .unwrap();
    let e = landed(&mut doc, &snap, &found, "Trim White").unwrap_err();
    assert_eq!(e, conflict(Key::Page(p), "Recognize text"));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A page Trim White finds already cropped as it would crop it takes no
/// op, but its crop decided that: a trim planned then, which lands after
/// the crop was cleared, conflicts on the page's boxes, as it would had it
/// cropped the page.
#[test]
fn a_trim_that_leaves_a_trimmed_page_conflicts_with_a_change_of_its_crop() {
    let dir = scratch("trim-again");
    let mut doc = open(&pdf(&dir));
    let p = doc.state().order()[0];
    let views = common::views();
    let snap = doc.snapshot();
    let found = trimmed(&doc, &snap, &views, &[p], 0.0);
    landed(&mut doc, &snap, &found, "Trim White").unwrap();
    let snap = doc.snapshot();
    let again = trimmed(&doc, &snap, &views, &[p], 0.0);
    assert_eq!(
        (again.measured.as_slice(), again.crops.len()),
        (&[p][..], 0)
    );
    let clear = Op::SetPageBoxes {
        pages: PageSel::ids([p]),
        crop: Patch::Clear,
        trim: Patch::Keep,
        bleed: Patch::Keep,
        art: Patch::Keep,
    };
    doc.commit_ops(vec![clear], "Uncrop").unwrap();
    let e = landed(&mut doc, &snap, &again, "Trim White").unwrap_err();
    assert_eq!(e, conflict(Key::Geom(p), "Uncrop"));
    let _ = std::fs::remove_dir_all(&dir);
}

sheaf_doc::both_ways!(
    a_bookmark_on_a_cropped_page_lands_on_the_same_content,
    a_bookmark_on_a_cropped_page_lands_on_the_same_content_appended: bookmark_lands
);

/// A bookmark's `/XYZ`, in PageSpace, stays on the same content through a
/// crop: unchanged by it, saved `how` and reopened it's unchanged, and the
/// cropped page's frame puts it on the square's top-left corner as PDFium
/// draws the page.
fn bookmark_lands(how: How) {
    let dir = scratch(&format!("bookmark-{how:?}"));
    let src = pdf(&dir);
    let mut doc = open(&src);
    let page = doc.state().order()[0];
    // The square's top-left in user space.
    let view = DestView::Xyz {
        left: Some(300.0),
        top: Some(400.0),
        zoom: None,
    };
    let insert = Op::AddBookmark {
        parent: None,
        at: Sibling::First,
        title: "Square".into(),
        target: Some(PageRef::Id(page)),
        view: Some(view.clone()),
        color: None,
        style: Default::default(),
        local: None,
    };
    doc.commit_ops(vec![insert], "bookmark").unwrap();
    common::crop(&mut doc, &[page], UNEVEN);
    let st = doc.state();
    assert_eq!(
        st.bookmark(st.outline_roots()[0]).unwrap().view(),
        Some(&view)
    );
    let out = dir.join("out.pdf");
    std::fs::write(&out, how.write(&doc)).unwrap();
    let saved = open(&out);
    let st = saved.state();
    let first = st.bookmark(st.outline_roots()[0]).unwrap();
    assert_eq!(first.view(), Some(&view), "{how:?}");
    let p = saved.state().order()[0];
    let at = saved.geom(p).to_view().apply(Pt { x: 300.0, y: 400.0 });
    // Where PDFium draws the square's top-left on the saved page.
    let size = saved.page_size(p);
    let engine = engine_on(&out);
    let job = Job::Page {
        width: size.width.round() as u32,
        rotation: Rotation::R0,
    };
    let raster = engine.render(PageIndex(0), job, PixelFormat::Rgba).unwrap();
    let (sx, sy) = (
        size.width / raster.width as f32,
        size.height / raster.height as f32,
    );
    let (x0, y0, _, _) = content_bounds(&raster.to_gray().unwrap(), 128).unwrap();
    let corner = Pt {
        x: x0 as f32 * sx,
        y: y0 as f32 * sy,
    };
    assert!(
        (corner.x - at.x).abs() <= 1.0 && (corner.y - at.y).abs() <= 1.0,
        "{how:?}: the bookmark shows at {at:?}, the square at {corner:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

sheaf_doc::both_ways!(
    a_crop_leaves_ocr_text_on_its_words,
    a_crop_leaves_ocr_text_on_its_words_appended: ocr_text_stays
);

/// A crop leaves the page's OCR text as it was, in PageSpace, and PDFium
/// finds the text saved `how` on the words it was recognized on.
fn ocr_text_stays(how: How) {
    let dir = scratch(&format!("ocr-{how:?}"));
    let src = pdf(&dir);
    let mut doc = open(&src);
    let p = doc.state().order()[0];
    // A word along the square's top as shown.
    let word = OcrWord {
        text: "square".into(),
        rect: Box2 {
            x: 300.0,
            y: 392.0,
            w: 100.0,
            h: 20.0,
        },
        confidence: 1.0,
    };
    let layer = Arc::new(sheaf_doc::text_layer::from_engine(
        vec![word],
        &doc.geom(p),
        "test",
    ));
    doc.commit_ops(ocr([(p, layer.clone())]), "ocr").unwrap();
    common::crop(&mut doc, &[p], UNEVEN);
    assert!(Arc::ptr_eq(doc.state().ocr_text(p).unwrap(), &layer));
    let out = dir.join("out.pdf");
    std::fs::write(&out, how.write(&doc)).unwrap();
    let engine = engine_on(&out);
    let chars = PageChars::from_engine(engine_chars(&engine));
    let hits = chars.find(&Query::literal("square", false).unwrap());
    assert_eq!(hits.len(), 1, "{how:?}: {hits:?}");
    let hit = hits[0].rects.iter().copied().reduce(Box2::union).unwrap();
    let (got, want) = (hit, layer.words[0].rect);
    let near = |a: f32, b: f32| (a - b).abs() <= 2.0;
    assert!(
        near(got.x, want.x)
            && near(got.y, want.y)
            && near(got.right(), want.right())
            && near(got.bottom(), want.bottom()),
        "{how:?}: found at {got:?}, recognized at {want:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

sheaf_doc::both_ways!(
    generators_keep_the_crop,
    generators_keep_the_crop_appended: generators_keep_crop
);

/// A page keeps its boxes, which are the page's, not its base's, through
/// the generators drawn on it: cropped pages, a watermark on the first and
/// a header over both, which replaces the one an older sheaf drew on the
/// second, are cropped as they are and show their file's pages, saved
/// `how` with that `/CropBox`, the mark in the middle of the crop and the
/// header in its top left corner, inside the margins. A watermark set on a
/// version before a crop made since lands, and sits in the middle of that
/// crop: it's laid out on each page's own boxes as a save writes it, and
/// reads none when it's set. Cropped again, the mark and the header are
/// laid out on the new crop.
fn generators_keep_crop(how: How) {
    let dir = scratch(&format!("generators-{how:?}"));
    let older = b"/Artifact <</Type /Pagination /Subtype /Header /SheafStamp true>> BDC (h) Tj EMC";
    let headed = [SQUARE, b"\n", older].concat();
    let file = file_of(vec![(letter(), SQUARE), (letter(), &headed)]);
    let mut doc = open(&on_disk(&dir, "two.pdf", file));
    let pages: Vec<PageId> = doc.state().order().iter().copied().collect();
    common::crop(&mut doc, &pages, UNEVEN);
    let shown = |doc: &Document| pages.iter().map(|&p| doc.geom(p)).collect::<Vec<_>>();
    // Set, then the page cropped again before it lands.
    let snap = doc.snapshot();
    let mut tx = Document::tx_at(&snap, doc.lower_cx(Actor::User));
    let set = set_watermark(watermark("DRAFT"), Z::Over, Some(&pages[..1]));
    tx.apply(set, &NoProgress).unwrap();
    common::crop(&mut doc, &pages[..1], UNEVEN);
    let cropped = shown(&doc);
    let opts = CommitOpts::based(snap.version, Stale::Validate, "Watermark");
    doc.commit(tx, opts).unwrap();
    (doc.commit_ops(vec![set_header(header("HEAD"), None)], "Header")).unwrap();
    for &p in &pages {
        let base = &doc.state().def(p).unwrap().base;
        assert_eq!(base.src, *doc.primary(), "no copy is made");
    }
    assert_eq!(shown(&doc), cropped, "the boxes are the page's");
    // Saved, each page has its crop, only the document's header, and the
    // mark in the middle of the first's, the header in its corner.
    let saved = |doc: &Document, name: &str| {
        let out = dir.join(name);
        std::fs::write(&out, how.write(doc)).unwrap();
        out
    };
    let in_the_middle = |out: &Path, c: Rect| {
        let drawn = glyphs(&engine_on(out), "DRAFT");
        let drawn = (drawn.into_iter()).reduce(Box2::union).unwrap();
        let off = (
            drawn.x + drawn.w / 2.0 - (c.x0 + c.x1) / 2.0,
            drawn.y + drawn.h / 2.0 - (c.y0 + c.y1) / 2.0,
        );
        assert!(
            off.0.abs() <= 12.0 && off.1.abs() <= 12.0,
            "{how:?}: {off:?} off the crop's middle"
        );
    };
    let in_the_corner = |out: &Path, c: Rect| {
        let drawn = glyphs(&engine_on(out), "HEAD");
        let drawn = (drawn.into_iter()).reduce(Box2::union).unwrap();
        let off = (drawn.x - (c.x0 + 72.0), drawn.y + drawn.h - (c.y1 - 36.0));
        assert!(
            off.0.abs() <= 3.0 && off.1.abs() <= 3.0,
            "{how:?}: {off:?} off the crop's corner, inside its margins"
        );
    };
    let out = saved(&doc, "out.pdf");
    let written = sheaf_doc::forms::fixture::source(std::fs::read(&out).unwrap());
    let (mark, head) = (PaginationKind::Watermark, PaginationKind::Header);
    for (n, want) in [vec![mark, head], vec![head]].into_iter().enumerate() {
        let blocks = written.blocks(n as u32).unwrap();
        let found: Vec<_> = (blocks.pagination.iter())
            .map(|p| (p.kind, p.origin))
            .collect();
        let want: Vec<_> = want.into_iter().map(|k| (k, PagOrigin::Sheaf)).collect();
        assert_eq!(found, want, "{how:?}: page {n}");
    }
    let file = Pdf::load(&out).unwrap();
    for (n, (_, id)) in file.get_pages().into_iter().enumerate() {
        let crop = file.get_dictionary(id).unwrap().get(b"CropBox").unwrap();
        let crop: Vec<f64> = (crop.as_array().unwrap().iter())
            .map(|o| o.as_float().unwrap())
            .collect();
        let c = cropped[n].crop;
        let want = [c.x0, c.y0, c.x1, c.y1].map(f64::from);
        assert_eq!(crop, want, "{how:?}: page {n}");
    }
    in_the_middle(&out, cropped[0].crop);
    in_the_corner(&out, cropped[0].crop);
    // Cropped again, the mark moves to the middle of the new crop, and the
    // header to its corner.
    common::crop(&mut doc, &pages[..1], UNEVEN);
    let again = doc.geom(pages[0]).crop;
    assert_ne!(again, cropped[0].crop);
    let out = saved(&doc, "again.pdf");
    in_the_middle(&out, again);
    in_the_corner(&out, again);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Two pages alike, cropped alike in two edits, are two pages still: a
/// crop is a box of the page's, so no copy is made of either, and a save
/// writes each as its own page, so a link to either still leads to it.
#[test]
fn pages_alike_cropped_apart_stay_apart() {
    let dir = scratch("alike");
    let mut pdf = Pdf::with_version("1.5");
    let pages_id = pdf.new_object_id();
    let content = pdf.add_object(Stream::new(
        dictionary! {},
        b"0 g 300 300 100 100 re f".to_vec(),
    ));
    let page = |annots: Vec<lopdf::Object>| {
        dictionary! {
            "Type" => "Page", "Parent" => pages_id, "Contents" => content, "Annots" => annots,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        }
    };
    let alike = [page(Vec::new()), page(Vec::new())].map(|p| pdf.add_object(p));
    let links = alike.map(|to| {
        let rect: Vec<lopdf::Object> = vec![10.into(), 10.into(), 30.into(), 30.into()];
        let dest: Vec<lopdf::Object> = vec![to.into(), "Fit".into()];
        let link =
            dictionary! { "Type" => "Annot", "Subtype" => "Link", "Rect" => rect, "Dest" => dest };
        pdf.add_object(link).into()
    });
    let linking = pdf.add_object(page(links.to_vec()));
    let kids: Vec<lopdf::Object> = [alike[0], alike[1], linking].map(Into::into).to_vec();
    pdf.objects.insert(
        pages_id,
        dictionary! { "Type" => "Pages", "Kids" => kids, "Count" => 3 }.into(),
    );
    let catalog = pdf.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    pdf.trailer.set("Root", catalog);
    let path = dir.join("alike.pdf");
    pdf.save(&path).unwrap();
    let mut doc = open(&path);
    for i in 0..2 {
        let page = doc.state().order()[i];
        common::crop(&mut doc, &[page], UNEVEN);
    }
    let primary = doc.primary().clone();
    for &p in doc.state().order().iter().take(2) {
        assert_eq!(doc.state().def(p).unwrap().base.src, primary);
    }
    for how in [How::Rewritten, How::Appended] {
        let saved = Pdf::load_mem(&how.write(&doc)).unwrap();
        let pages: Vec<lopdf::ObjectId> = saved.get_pages().into_values().collect();
        let annots = saved.get_page_annotations(pages[2]).unwrap();
        let to: Vec<lopdf::ObjectId> = (annots.iter())
            .map(|link| {
                link.get(b"Dest").unwrap().as_array().unwrap()[0]
                    .as_reference()
                    .unwrap()
            })
            .collect();
        assert_eq!(to, pages[..2], "{how:?}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// A crop of a form's page saves as planned by an Append whose section is
/// that page's dictionary alone (proposal 27), and the form stays clean:
/// its fields and their values as they were, each widget's `/P` the page
/// that lists it, and reopened, the page cropped.
#[test]
fn a_crop_appends_only_page_dicts_and_forms_stay_clean() {
    let dir = scratch("form-append");
    let path = common::write(&dir, "form.pdf", &mut common::two_page_form());
    let input = std::fs::read(&path).unwrap();
    let mut doc = open(&path);
    let page = doc.state().order()[0];
    common::crop(&mut doc, &[page], UNEVEN);
    let (strategy, bytes) = common::planned(&mut doc, &path);
    assert_eq!(strategy, StrategyKind::Append);
    assert!(
        bytes.starts_with(&input),
        "an Append keeps the file's bytes"
    );
    let first = Pdf::load_mem(&input).unwrap().get_pages()[&1];
    let wrote = common::appended_objects(input.len(), &bytes);
    assert_eq!(wrote, BTreeMap::from([(first, true)]), "the page alone");
    let report = common::clean(&bytes, "the Append");
    assert_eq!((report.fields, report.widgets), (4, 4));
    assert_eq!(common::values(&bytes), common::values(&input));
    let saved = Pdf::load_mem(&bytes).unwrap();
    let mut widgets = 0;
    for at in saved.get_pages().into_values() {
        let Ok(annots) = saved.get_dictionary(at).unwrap().get(b"Annots") else {
            continue;
        };
        for annot in saved.dereference(annots).unwrap().1.as_array().unwrap() {
            let widget = saved.get_dictionary(annot.as_reference().unwrap()).unwrap();
            assert_eq!(widget.get(b"P").unwrap().as_reference().unwrap(), at);
            widgets += 1;
        }
    }
    assert_eq!(widgets, 4);
    let back = open(&path);
    assert_eq!(back.geom(back.state().order()[0]), doc.geom(page));
    let _ = std::fs::remove_dir_all(&dir);
}
