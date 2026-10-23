//! The intake (M5 proposal 1): pages a page at a time into a document, in
//! batches that make the pages one batch would, as one undo step however
//! many batches it took, two around an edit between them, and stopped
//! once it's undone; a TIFF's pages each a page. And the scanner profile
//! (proposal 3), which keeps a white page on a white lid whole.
//!
//! Every page is cleaned with orientation off: a workspace run has the
//! orientation models and a run of this crate alone doesn't. The intake's
//! pages are cleaned with finding the page, evening out the light and
//! straightening off too, which leaves what the mode does, so a page takes
//! milliseconds in a debug build; the flatbed scan is cleaned as each
//! profile starts it. OCR is a fake engine's, which reads a word a line of
//! "text". Needs libpdfium (`scripts/fetch-pdfium.sh`), like the app
//! itself.

// A test crate of its own, whose scratch files go in the temp directory
// clippy.toml bans outside tests.
#![allow(clippy::disallowed_methods)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use image::{GrayImage, Luma, Rgb, RgbImage};
use lopdf::Document as Pdf;
use sheaf_doc::materialize::{AskAnswers, StrategyKind};
use sheaf_doc::recovery;
use sheaf_doc::{Actor, EditError, JobKind, TextLayer};
use sheaf_edit::{Op, PageRef, PageSel, PageSize, PageSource, Rotation, Turn};
use sheaf_image::clean::Mode;
use sheaf_image::raster::PixelFormat;
use sheaf_ocr::{EngineInfo, EngineKind, InputKind, OcrEngine, OcrImage, OcrWord};
use sheaf_ops::images::Paper;
use sheaf_ops::intake::{Batch, Batching, Files, Landing, Pages, RawPage, SessionLanding, intake};
use sheaf_ops::scan::{Profile, ScanOptions, ScanReport, clean_image, scan};
use sheaf_ops::session::{SaveTo, Session};
use sheaf_ops::{NoProgress, Progress, Tracker, was_cancelled};
use sheaf_render::pdfium::PdfiumEngine;
use sheaf_render::{PageIndex, Renderer};

/// A page of lines of "words" on off-white paper with a little sensor
/// noise, and a blue mark: the words' lengths and gaps, and the lines'
/// lengths, come from `seed`, so no two pages are alike.
fn page(seed: u64) -> RgbImage {
    let mut rng = fastrand::Rng::with_seed(seed);
    let mut img = RgbImage::from_fn(850, 1100, |_, _| {
        let v = 236 + rng.u8(0..9);
        Rgb([v, v, v.saturating_sub(4)])
    });
    let mut fill = |x0: u32, y0: u32, w: u32, h: u32, c: Rgb<u8>| {
        for y in y0..y0 + h {
            for x in x0..(x0 + w).min(img.width()) {
                img.put_pixel(x, y, c);
            }
        }
    };
    for line in 0..24 {
        let (y, end) = (120 + line * 36, rng.u32(400..760));
        let mut x = 90;
        while x < end {
            let w = rng.u32(20..90);
            fill(x, y, w, 12, Rgb([30, 30, 30]));
            x += w + rng.u32(12..24);
        }
    }
    fill(rng.u32(500..700), 1020, 80, 30, Rgb([40, 60, 200]));
    img
}

/// Reads a word a line of "text": each run of rows with dark pixels in
/// it, its box around them, named by its width.
struct Lines;

impl OcrEngine for Lines {
    fn input(&self) -> InputKind {
        InputKind::Gray
    }

    fn recognize(&mut self, img: OcrImage, _dpi: u32) -> Result<Vec<OcrWord>> {
        let OcrImage::Gray(img) = img else {
            unreachable!("asked for gray")
        };
        // Each row's dark pixels, from the first to past the last, when
        // there are enough of them.
        let rows: Vec<Option<(u32, u32)>> = (0..img.height())
            .map(|y| {
                let dark: Vec<u32> = (0..img.width())
                    .filter(|&x| img.get_pixel(x, y).0[0] < 128)
                    .collect();
                (dark.len() >= 20).then(|| (dark[0], dark[dark.len() - 1] + 1))
            })
            .collect();
        let mut words = Vec::new();
        let mut y = 0;
        while y < rows.len() {
            if rows[y].is_none() {
                y += 1;
                continue;
            }
            let (top, mut l, mut r) = (y, u32::MAX, 0);
            while let Some(Some((a, b))) = rows.get(y) {
                (l, r) = (l.min(*a), r.max(*b));
                y += 1;
            }
            words.push(OcrWord {
                text: format!("w{}", (r - l) / 10),
                bbox_px: [l as f32, top as f32, r as f32, y as f32],
                confidence: 0.9,
            });
        }
        Ok(words)
    }

    fn describe(&self) -> EngineInfo {
        EngineInfo {
            kind: EngineKind::Tesseract,
            name: "lines 1".into(),
            languages: Vec::new(),
        }
    }
}

/// Pages made from seeds, one at a time, counting how many it gave.
struct Made {
    seeds: std::ops::Range<u64>,
    given: usize,
}

fn made(n: u64) -> Made {
    Made {
        seeds: 0..n,
        given: 0,
    }
}

impl Pages for Made {
    fn next(&mut self, _: &dyn Progress) -> Result<Option<RawPage>> {
        let Some(seed) = self.seeds.next() else {
            return Ok(None);
        };
        self.given += 1;
        Ok(Some(RawPage {
            image: page(seed),
            name: format!("page {seed}"),
        }))
    }

    fn count(&self) -> Option<usize> {
        Some(self.seeds.end as usize)
    }
}

/// `mode`, each page its own size at 100 dpi, with none of cleanup's
/// steps but the mode's.
fn opts(mode: Mode) -> ScanOptions {
    let mut opts = ScanOptions {
        page: Paper::Original,
        dpi: 100,
        ..Default::default()
    };
    opts.clean.mode = mode;
    opts.clean.crop = false;
    opts.clean.orient = false;
    opts.clean.deskew = false;
    opts.clean.flatten = false;
    opts
}

/// Landings at most `pages` pages each, as the command line watches no
/// clock.
fn every(pages: usize) -> Batching {
    Batching { pages, wait: None }
}

/// A session's landing that, once its `at`th batch has landed, does
/// `then` to the session.
struct After<'a> {
    inner: SessionLanding<'a>,
    landed: usize,
    at: usize,
    then: fn(&mut Session),
}

impl Landing for After<'_> {
    fn land(&mut self, batch: Batch, p: &dyn Progress) -> Result<()> {
        self.inner.land(batch, p)?;
        self.landed += 1;
        if self.landed == self.at {
            (self.then)(self.inner.session());
        }
        Ok(())
    }
}

/// `pages` taken into `session` as `opts` says, recognized by the fake
/// engine when `ocr`, landed as `batching` says.
fn take(
    session: &mut Session,
    pages: &mut dyn Pages,
    opts: &ScanOptions,
    ocr: bool,
    batching: Batching,
) -> Result<ScanReport> {
    let mut engine = Lines;
    let engine = ocr.then_some(&mut engine as &mut dyn OcrEngine);
    let mut landing = SessionLanding::new(session, "Scan");
    let report = intake(pages, opts, engine, &mut landing, batching, &NoProgress)?;
    landing.finish();
    Ok(report)
}

/// A scratch folder of the test's own.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("sheaf-intake-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// `session` written whole to `path`, as `sheaf scan` writes it.
fn save(session: &mut Session, path: &Path) -> Vec<u8> {
    let to = SaveTo::File {
        path: path.to_path_buf(),
        append: false,
    };
    session
        .save(to, &AskAnswers::default(), &NoProgress)
        .unwrap();
    std::fs::read(path).unwrap()
}

/// Each page's recognized text in `session`, in order.
fn layers(session: &Session) -> Vec<Option<Arc<TextLayer>>> {
    let state = session.doc.state();
    (state.pages())
        .map(|(id, _, _)| state.ocr_text(id).cloned())
        .collect()
}

/// A saved file as PDFium reads it: each page rendered at 50 dpi, with
/// its text and its characters' boxes.
fn read(bytes: &[u8]) -> Vec<(Vec<u8>, String, String)> {
    let engine = PdfiumEngine::open_bytes(bytes.to_vec().into(), None).unwrap();
    (0..engine.page_count())
        .map(|i| {
            let page = PageIndex(i);
            let raster = engine.render_page(page, 425, PixelFormat::Rgba).unwrap();
            let text = engine.page_text(page).unwrap();
            let chars = format!("{:?}", engine.page_chars(page).unwrap());
            (raster.data, text, chars)
        })
        .collect()
}

/// Ten pages taken in batches of `per`, recognized when `ocr`, and saved.
struct Saved {
    per: usize,
    ocr: bool,
    layers: Vec<Option<Arc<TextLayer>>>,
    read: Vec<(Vec<u8>, String, String)>,
    bytes: Vec<u8>,
}

/// In each mode, ten pages landed a page at a time, three, eight, or all
/// at once (the one batch `sheaf scan` made before the intake) make the
/// same pages: the same recognized text, and saved, the same as PDFium
/// reads them, rendered, as text and as character boxes. The bytes differ:
/// a Rewrite numbers what an insert brought by its source, each batch's
/// own, so other batches write the same objects in another order. The same
/// batches write the same bytes, without text (a layer's block id is drawn
/// at random).
#[test]
fn batches_make_the_pages_one_batch_makes() {
    let dir = scratch("batches");
    for mode in [Mode::Color, Mode::Gray, Mode::Bw] {
        let opts = opts(mode);
        let mut saves = Vec::new();
        for per in [usize::MAX, 1, 3, 8] {
            for ocr in [false, true] {
                let mut session = Session::new_empty();
                let report = take(&mut session, &mut made(10), &opts, ocr, every(per)).unwrap();
                assert_eq!((report.pages, report.words > 0), (10, ocr), "{mode:?}");
                let layers = layers(&session);
                assert!(layers.iter().all(|l| l.is_some() == ocr), "{mode:?}");
                let bytes = save(&mut session, &dir.join(format!("{per}-{ocr}.pdf")));
                let read = read(&bytes);
                assert_eq!(read.len(), 10);
                saves.push(Saved {
                    per,
                    ocr,
                    layers,
                    read,
                    bytes,
                });
            }
        }
        for s in &saves[2..] {
            let all = saves.iter().find(|a| a.per == usize::MAX && a.ocr == s.ocr);
            let all = all.expect("the one batch's");
            let what = format!("{mode:?}: batches of {}, text {}", s.per, s.ocr);
            assert_eq!(s.layers, all.layers, "{what}");
            for (i, (page, one)) in s.read.iter().zip(&all.read).enumerate() {
                assert!(page == one, "{what}, page {i}");
            }
        }
        let mut again = Session::new_empty();
        take(&mut again, &mut made(10), &opts, false, every(8)).unwrap();
        let eight = saves.iter().find(|s| s.per == 8 && !s.ocr).unwrap();
        let bytes = save(&mut again, &dir.join("again.pdf"));
        assert!(
            bytes == eight.bytes,
            "{mode:?}: the same batches, other bytes"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// Twenty pages in batches of three are one version after the empty
/// document, the scan job's, which one undo takes back whole.
#[test]
fn an_intake_is_one_undo_step() {
    let mut session = Session::new_empty();
    take(&mut session, &mut made(20), &opts(Mode::Bw), true, every(3)).unwrap();
    assert_eq!(session.doc.page_count(), 20);
    let timeline = session.doc.timeline();
    assert_eq!(timeline.len(), 2, "the empty document and the scan");
    assert_eq!(timeline[1].label, "Scan");
    assert_eq!(timeline[1].actor, Actor::Job(JobKind::Scan));
    session.doc.undo().unwrap();
    assert_eq!(session.doc.page_count(), 0);
    assert!(!session.doc.can_undo());
}

/// An intake's document no file holds, written as an untitled sidecar
/// (M5 proposal 2), is found in its folder once its claim is let go and
/// comes back as the app opens it, under its name: never saved, so dirty
/// with nothing to undo, its first save a Rewrite that leaves it clean,
/// and its pages the intake's, as PDFium reads them, rendered, as text and
/// as character boxes.
#[test]
fn an_intake_s_untitled_sidecar_comes_back() {
    let dir = scratch("untitled");
    let mut session = Session::new_empty_named("Untitled scan 1");
    let gray = opts(Mode::Gray);
    take(&mut session, &mut made(10), &gray, true, every(3)).unwrap();
    let folder = dir.join("recovery");
    let claim = recovery::claim_untitled(&folder, "Untitled scan 1")
        .unwrap()
        .unwrap();
    let input = session.doc.recovery_input();
    recovery::write_untitled(&input, &claim, &dir, &NoProgress).unwrap();
    drop(claim);
    let found = recovery::find_untitled(&folder);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].folder, dir);
    let store = sheaf_doc::Store::new();
    let src = recovery::restore_untitled(&store, &found[0].claim).unwrap();
    let name = found[0].claim.name().into();
    let policy = sheaf_doc::history::default_policy();
    let mut doc = sheaf_doc::open::start(store, src.clone(), name, None, policy);
    doc.ready(sheaf_doc::open::finish(&src, &NoProgress))
        .unwrap();
    doc.never_saved();
    let mut back = Session::from(doc);
    assert_eq!(**back.doc.name(), *"Untitled scan 1");
    assert_eq!(back.doc.page_count(), 10);
    assert!(back.doc.is_dirty() && !back.doc.can_undo());
    let to = SaveTo::File {
        path: dir.join("recovered.pdf"),
        append: false,
    };
    let report = (back.save(to, &AskAnswers::default(), &NoProgress)).unwrap();
    assert_eq!(report.strategy, Some(StrategyKind::Rewrite));
    assert!(!back.doc.is_dirty());
    let recovered = read(&std::fs::read(dir.join("recovered.pdf")).unwrap());
    let scanned = read(&save(&mut session, &dir.join("scanned.pdf")));
    assert_eq!(recovered.len(), 10);
    for (i, (page, scanned)) in recovered.iter().zip(&scanned).enumerate() {
        assert!(page == scanned, "page {i}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// The user's page 1 turned a quarter.
fn turn_page_1(session: &mut Session) {
    let rotate = Op::RotatePages {
        pages: PageSel::Ids(vec![PageRef::Pos(1)]),
        by: Turn::Quarter,
    };
    session.commit(vec![rotate], "Rotate").unwrap();
}

/// An edit between batches leaves the intake two undo steps around it,
/// "Scan" and "Scan (continued)"; undoing all of the scan takes both back
/// and keeps the edit.
#[test]
fn an_edit_between_batches_makes_two_undo_steps() {
    let mut session = Session::new_empty();
    let blank = PageSource::Blank {
        size: PageSize {
            width: 612.0,
            height: 792.0,
        },
        count: 1,
    };
    let insert = Op::InsertPages {
        from: blank,
        pages: None,
        before: None,
        bring: Default::default(),
        locals: Vec::new(),
    };
    session.commit(vec![insert], "Blank").unwrap();
    let mut landing = After {
        inner: SessionLanding::new(&mut session, "Scan"),
        landed: 0,
        at: 2,
        then: turn_page_1,
    };
    let opts = opts(Mode::Bw);
    intake(
        &mut made(10),
        &opts,
        None,
        &mut landing,
        every(3),
        &NoProgress,
    )
    .unwrap();
    landing.inner.finish();
    assert_eq!(session.doc.page_count(), 11);
    let timeline = session.doc.timeline();
    let labels: Vec<&str> = timeline.iter().map(|v| v.label.as_str()).collect();
    assert_eq!(labels[1..], ["Blank", "Scan", "Rotate", "Scan (continued)"]);
    let (g, _) = timeline[2].group.clone().expect("the scan's group");
    assert_eq!(timeline[4].group.as_ref().map(|(g, _)| *g), Some(g));
    session.doc.revert_group(g, "Undo all of Scan").unwrap();
    assert_eq!(session.doc.page_count(), 1);
    let p1 = session.doc.page(0).unwrap();
    assert_eq!(session.doc.geom(p1).rotate, Rotation::R90, "the edit stays");
}

/// Undoing the intake's edits after its second batch stops it at its
/// third: that batch's commit is GroupEnded, no page is read after it,
/// and nothing it scanned is put back.
#[test]
fn undoing_an_intake_stops_it() {
    let mut session = Session::new_empty();
    let mut landing = After {
        inner: SessionLanding::new(&mut session, "Scan"),
        landed: 0,
        at: 2,
        then: |s| {
            s.doc.undo().unwrap();
        },
    };
    let mut pages = made(20);
    let opts = opts(Mode::Bw);
    let e = intake(&mut pages, &opts, None, &mut landing, every(3), &NoProgress).unwrap_err();
    assert_eq!(e.downcast_ref::<EditError>(), Some(&EditError::GroupEnded));
    assert_eq!(pages.given, 9, "read up to the third batch, and no further");
    assert_eq!(landing.inner.session().doc.page_count(), 0);
}

/// `Made`'s pages until its `at`th, which instead of a page is an error.
struct FailsAt {
    inner: Made,
    at: usize,
}

impl Pages for FailsAt {
    fn next(&mut self, p: &dyn Progress) -> Result<Option<RawPage>> {
        if self.inner.given + 1 == self.at {
            anyhow::bail!("page {} can't be read", self.at);
        }
        self.inner.next(p)
    }

    fn count(&self) -> Option<usize> {
        self.inner.count()
    }
}

/// A page that can't be read stops the intake, and the pages before it
/// stay: in batches of three, page 5's error lands page 4 with the three
/// before it, as the one undo step.
#[test]
fn a_page_that_can_t_be_read_keeps_the_pages_before_it() {
    let mut session = Session::new_empty();
    let mut landing = SessionLanding::new(&mut session, "Scan");
    let mut pages = FailsAt {
        inner: made(10),
        at: 5,
    };
    let opts = opts(Mode::Bw);
    let mut engine = Lines;
    let e = intake(
        &mut pages,
        &opts,
        Some(&mut engine),
        &mut landing,
        every(3),
        &NoProgress,
    )
    .unwrap_err();
    assert_eq!(e.to_string(), "page 5 can't be read");
    assert_eq!(session.doc.page_count(), 4);
    assert!(
        layers(&session).iter().all(Option::is_some),
        "with their text"
    );
    assert_eq!(session.doc.timeline().len(), 2, "one undo step");
    session.doc.undo().unwrap();
    assert_eq!(session.doc.page_count(), 0);
}

/// `Made`'s pages, cancelling `tracker` as the one after its `at`th
/// comes, as Escape would.
struct CancelsAfter {
    inner: Made,
    at: usize,
    tracker: Tracker,
}

impl Pages for CancelsAfter {
    fn next(&mut self, p: &dyn Progress) -> Result<Option<RawPage>> {
        if self.inner.given == self.at {
            self.tracker.cancel();
        }
        self.inner.next(p)
    }

    fn count(&self) -> Option<usize> {
        self.inner.count()
    }
}

/// A cancel stops the intake, and the pages so far stay: in batches of
/// three, a cancel as page 5 comes keeps the four before it, page 4 in a
/// batch of its own. The page it stops is left out, its cleanup cut
/// short.
#[test]
fn a_cancel_keeps_the_pages_so_far() {
    let mut session = Session::new_empty();
    let tracker = Tracker::new();
    let mut pages = CancelsAfter {
        inner: made(10),
        at: 4,
        tracker: tracker.clone(),
    };
    let mut landing = SessionLanding::new(&mut session, "Scan");
    let opts = opts(Mode::Bw);
    let e = intake(&mut pages, &opts, None, &mut landing, every(3), &tracker).unwrap_err();
    assert!(was_cancelled(&e), "{e:#}");
    assert_eq!(pages.inner.given, 5);
    assert_eq!(session.doc.page_count(), 4);
    assert_eq!(session.doc.timeline().len(), 2, "one undo step");
}

/// The pages of each batch landed.
#[derive(Default)]
struct Counts(Vec<usize>);

impl Landing for Counts {
    fn land(&mut self, batch: Batch, _: &dyn Progress) -> Result<()> {
        self.0.push(batch.pages.len());
        Ok(())
    }
}

/// Small pages that take `pause` to come before pages 1 and 5, and none
/// before the rest.
struct Slow {
    given: usize,
    pause: Duration,
}

impl Pages for Slow {
    fn next(&mut self, _: &dyn Progress) -> Result<Option<RawPage>> {
        if self.given == 8 {
            return Ok(None);
        }
        self.given += 1;
        if matches!(self.given, 1 | 5) {
            std::thread::sleep(self.pause);
        }
        let image = image::imageops::thumbnail(&page(self.given as u64), 85, 110);
        Ok(Some(RawPage {
            image,
            name: format!("page {}", self.given),
        }))
    }

    fn count(&self) -> Option<usize> {
        Some(8)
    }
}

/// A batch lands once the wait has passed since the last landing, counted
/// again from each: with a wait of 0.3 s and room for 100 pages, page 1,
/// 0.9 s in coming, lands alone; pages 2 to 4 come at once and wait for
/// page 5, 0.9 s in coming, which takes them with it; the last three land
/// at the end.
#[test]
fn a_batch_lands_once_its_wait_has_passed() {
    let wait = Duration::from_millis(300);
    let mut pages = Slow {
        given: 0,
        pause: wait * 3,
    };
    let mut landing = Counts::default();
    let batching = Batching {
        pages: 100,
        wait: Some(wait),
    };
    let opts = opts(Mode::Gray);
    intake(&mut pages, &opts, None, &mut landing, batching, &NoProgress).unwrap();
    assert_eq!(landing.0, [1, 4, 3]);
}

/// In `dir`, a TIFF of two pages, the second turned on its side, and a
/// PNG of a third page a little smaller, all at 100 dpi.
fn tiff_and_png(dir: &Path) -> Vec<PathBuf> {
    let tall = page(1);
    let wide = image::imageops::rotate90(&page(2));
    let tiff = dir.join("two.tif");
    let mut w = sheaf_image::encode::TiffWriter::new(std::fs::File::create(&tiff).unwrap(), 100, 2)
        .unwrap();
    for img in [&tall, &wide] {
        let gray = image::DynamicImage::ImageRgb8(img.clone()).into_luma8();
        w.add(sheaf_image::encode::TiffPage::Gray(&gray)).unwrap();
    }
    drop(w);
    let png = dir.join("three.png");
    image::imageops::crop_imm(&page(3), 0, 0, 800, 1000)
        .to_image()
        .save(&png)
        .unwrap();
    vec![tiff, png]
}

/// A TIFF of two pages and a PNG scan as three pages, the TIFF's in
/// order.
#[test]
fn a_multi_page_tiff_scans_page_by_page() {
    let dir = scratch("tiff");
    let opts = opts(Mode::Gray);
    let out = dir.join("out.pdf");
    let files = tiff_and_png(&dir);
    let report = scan(&files, &opts, &Default::default(), &out, &NoProgress).unwrap();
    assert_eq!(report.pages, 3);
    let pdf = Pdf::load(&out).unwrap();
    let sizes: Vec<_> = (pdf.get_pages().values())
        .map(|&id| {
            let page = pdf.get_dictionary(id).unwrap();
            let media = page.get(b"MediaBox").unwrap().as_array().unwrap();
            let n = |i: usize| media[i].as_float().unwrap().round() as i64;
            (n(2), n(3))
        })
        .collect();
    assert_eq!(sizes, [(612, 792), (792, 612), (576, 720)]);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Files' pages, with the intake's progress as each is asked for.
struct Watched {
    files: Files,
    tracker: Tracker,
    seen: Vec<(u64, Option<f32>)>,
}

impl Pages for Watched {
    fn next(&mut self, p: &dyn Progress) -> Result<Option<RawPage>> {
        let s = self.tracker.snapshot();
        self.seen.push((s.done, s.fraction));
        self.files.next(p)
    }

    fn count(&self) -> Option<usize> {
        self.files.count()
    }
}

/// Progress counts pages, a TIFF's each one: over a two-page TIFF and a
/// PNG, a third of the way after each page, all of it at the end.
#[test]
fn progress_counts_a_tiff_s_pages() {
    let dir = scratch("progress");
    let files = Files::new(&tiff_and_png(&dir)).unwrap();
    assert_eq!(files.count(), Some(3));
    let tracker = Tracker::new();
    let mut pages = Watched {
        files,
        tracker: tracker.clone(),
        seen: Vec::new(),
    };
    let mut session = Session::new_empty();
    let mut landing = SessionLanding::new(&mut session, "Scan");
    let opts = opts(Mode::Gray);
    intake(&mut pages, &opts, None, &mut landing, every(8), &tracker).unwrap();
    landing.finish();
    assert_eq!(pages.seen.len(), 4, "three pages and the end");
    for (k, &(done, fraction)) in pages.seen.iter().enumerate() {
        let want = k as f32 / 3.0;
        let near = fraction.is_some_and(|f| (f - want).abs() < 0.01);
        assert!(
            done == k as u64 && near,
            "after {k} pages: {done}, {fraction:?}"
        );
    }
    let s = tracker.snapshot();
    assert_eq!((s.done, s.total, s.fraction), (3, Some(3), Some(1.0)));
    let _ = std::fs::remove_dir_all(&dir);
}

/// The flatbed page's lines of "words", at 150 dpi: how many, the first's
/// top, from one line's top to the next's, and how tall each is; and
/// where each line's words start.
const LINES: u32 = 28;
const LINE_TOP: u32 = 180;
const LINE_STEP: u32 = 48;
const LINE_HEIGHT: u32 = 12;
const LINE_LEFT: u32 = 150;
/// The page, Letter at 150 dpi, and the lid around it.
const LETTER_150: (u32, u32) = (1275, 1650);
const LID: (u32, u32) = (1400, 1800);

/// A flatbed scan at 150 dpi: a Letter page of paper about 247 carrying
/// lines of "words", turned 3° clockwise, on a white lid of about 252 a
/// little larger than it, both with a little sensor noise; and where the
/// longest line ends on the page.
fn flatbed() -> (RgbImage, u32) {
    let mut rng = fastrand::Rng::with_seed(7);
    let (pw, ph) = LETTER_150;
    let mut page = GrayImage::from_fn(pw, ph, |_, _| Luma([246 + rng.u8(0..3)]));
    let mut right = 0;
    for line in 0..LINES {
        let (y, end) = (LINE_TOP + line * LINE_STEP, rng.u32(800..1120));
        let mut x = LINE_LEFT;
        while x < end {
            let w = rng.u32(20..90);
            for yy in y..y + LINE_HEIGHT {
                for xx in x..x + w {
                    page.put_pixel(xx, yy, Luma([30]));
                }
            }
            right = right.max(x + w);
            x += w + rng.u32(12..24);
        }
    }
    let (lw, lh) = LID;
    let (sin, cos) = 3f32.to_radians().sin_cos();
    let scan = RgbImage::from_fn(lw, lh, |x, y| {
        let (dx, dy) = (
            x as f32 + 0.5 - lw as f32 / 2.0,
            y as f32 + 0.5 - lh as f32 / 2.0,
        );
        // From the lid back to the page, turned the other way.
        let (u, v) = (
            dx * cos + dy * sin + pw as f32 / 2.0,
            -dx * sin + dy * cos + ph as f32 / 2.0,
        );
        let tone = if u >= 0.0 && v >= 0.0 && (u as u32) < pw && (v as u32) < ph {
            page.get_pixel(u as u32, v as u32)[0]
        } else {
            251 + rng.u8(0..3)
        };
        Rgb([tone, tone, tone])
    });
    (scan, right)
}

/// Where `gray` has ink: each run of rows with a line's worth of dark
/// pixels (its first row and past its last), and the columns the dark
/// pixels span.
fn ink(gray: &GrayImage) -> (Vec<(u32, u32)>, (u32, u32)) {
    let (mut runs, mut cols) = (Vec::<(u32, u32)>::new(), (u32::MAX, 0));
    for y in 0..gray.height() {
        let dark: Vec<u32> = (0..gray.width())
            .filter(|&x| gray.get_pixel(x, y)[0] < 128)
            .collect();
        if dark.len() < 20 {
            continue;
        }
        cols = (cols.0.min(dark[0]), cols.1.max(dark[dark.len() - 1] + 1));
        match runs.last_mut() {
            Some(run) if run.1 == y => run.1 = y + 1,
            _ => runs.push((y, y + 1)),
        }
    }
    (runs, cols)
}

/// The skew left in `gray`, measured as cleanup measures it, on a copy
/// about 1000 pixels long.
fn skew_left(gray: &GrayImage) -> f32 {
    let s = 1000.0 / gray.width().max(gray.height()) as f32;
    let small = image::imageops::resize(
        gray,
        (gray.width() as f32 * s).round() as u32,
        (gray.height() as f32 * s).round() as u32,
        image::imageops::FilterType::Triangle,
    );
    sheaf_image::clean::skew_angle(&small)
}

/// The middle of `gray`'s values over `xs` × `ys`.
fn median(gray: &GrayImage, xs: std::ops::Range<u32>, ys: std::ops::Range<u32>) -> u8 {
    let mut v: Vec<u8> = ys
        .flat_map(|y| xs.clone().map(move |x| (x, y)))
        .map(|(x, y)| gray.get_pixel(x, y)[0])
        .collect();
    v.sort_unstable();
    v[v.len() / 2]
}

/// The flatbed scan cleaned as `profile` starts it, orientation off; and
/// the whole scan's size fitted to the profile's page.
fn clean_flatbed(scan: &RgbImage, profile: Profile) -> (sheaf_image::clean::Cleaned, (u32, u32)) {
    let mut opts = profile.options();
    opts.clean.orient = false;
    let params = opts.params();
    let cleaned = clean_image(scan.clone(), &params, &NoProgress, &mut |_, _| {}).unwrap();
    let (fw, fh) = params.fit.unwrap();
    let k = (fw as f32 / LID.0 as f32).min(fh as f32 / LID.1 as f32);
    let fit = (
        (LID.0 as f32 * k).round() as u32,
        (LID.1 as f32 * k).round() as u32,
    );
    (cleaned, fit)
}

/// A white page on a flatbed's white lid has no edge to find: the scanner
/// profile keeps the whole scan, straightened, every line where it was
/// drawn, and its light as the scanner made it. The photo profile finds
/// no page there either, keeps the whole scan, straightened, and every
/// line, and evens the light out (M5 finding 6).
#[test]
fn the_scanner_profile_keeps_a_white_page_on_a_white_lid_whole() {
    let (scan, right) = flatbed();
    let (cleaned, fit) = clean_flatbed(&scan, Profile::Scanner);
    assert!(cleaned.quad.is_none(), "found a page: {:?}", cleaned.quad);
    let gray = cleaned.image.to_luma8();
    assert_eq!(gray.dimensions(), fit, "the whole scan, fitted");
    let skew = skew_left(&gray);
    assert!(skew.abs() < 0.3, "{skew}° of skew left");
    // A point on the page, straightened about the scan's middle, scaled.
    let k = fit.0 as f32 / LID.0 as f32;
    let at = |page: u32, half_page: u32, half_out: u32| {
        half_out as f32 + k * (page as f32 - half_page as f32)
    };
    let (x, y) = (
        |page: u32| at(page, LETTER_150.0 / 2, fit.0 / 2),
        |page: u32| at(page, LETTER_150.1 / 2, fit.1 / 2),
    );
    let near = |got: u32, want: f32| (got as f32 - want).abs() <= 3.0 * k;
    let (runs, cols) = ink(&gray);
    assert_eq!(runs.len(), LINES as usize, "{runs:?}");
    for (i, &(top, end)) in runs.iter().enumerate() {
        let line = LINE_TOP + i as u32 * LINE_STEP;
        let (want_top, want_end) = (y(line), y(line + LINE_HEIGHT));
        assert!(
            near(top, want_top) && near(end, want_end),
            "line {i}: rows {top}..{end}, drawn at {want_top}..{want_end}"
        );
    }
    let (want_left, want_right) = (x(LINE_LEFT), x(right));
    assert!(
        near(cols.0, want_left) && near(cols.1, want_right),
        "ink across {cols:?}, drawn across {want_left}..{want_right}"
    );
    // The page's top margin, and the lid beside the page halfway down.
    let paper = |gray: &GrayImage| {
        median(
            gray,
            x(300) as u32..x(1000) as u32,
            y(40) as u32..y(140) as u32,
        )
    };
    let lid = |gray: &GrayImage| median(gray, 30..90, fit.1 / 2 - 300..fit.1 / 2 + 300);
    let tones = (paper(&gray), lid(&gray));
    assert!(
        tones.0.abs_diff(247) <= 3 && tones.1.abs_diff(252) <= 3 && tones.0 < tones.1,
        "paper and lid {tones:?}, scanned 247 and 252"
    );

    // The photo profile finds no page either: it keeps the whole scan,
    // straightened, every line, and evens the light out.
    let (cleaned, fit) = clean_flatbed(&scan, Profile::Photo);
    assert!(cleaned.quad.is_none(), "found a page: {:?}", cleaned.quad);
    let gray = cleaned.image.to_luma8();
    assert_eq!(gray.dimensions(), fit, "the whole scan, fitted");
    let skew = skew_left(&gray);
    assert!(skew.abs() < 0.3, "{skew}° of skew left");
    assert_eq!(ink(&gray).0.len(), LINES as usize);
    let tones = (paper(&gray), lid(&gray));
    assert!(
        tones.0 >= 250 && tones.1 >= 250,
        "paper and lid {tones:?}, evened out"
    );
}
