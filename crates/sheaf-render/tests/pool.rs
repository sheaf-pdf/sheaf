//! The engine pool (M10.5 §10.3): one thread serves every engine; the
//! screen's group engines are capped, the least recently used unpinned
//! closing first, and pinned ones never close; background walks open their
//! own engines and never close the screen's; what the engines weigh stays
//! under the cap, and a base engine is opened again past its bound; an
//! engine whose source is gone closes, and one that doesn't open is tried
//! once; one PDFium refuses tells its opener, and a render or a query with
//! a fallback reads its page from there; a dropped receiver skips its
//! render and a burst renders once;
//! encrypted files open from segments; a walk reads its pages in their
//! order, each on its own engine or its fallback's, and ends when its
//! receiver is dropped; the worker sleeps when there's nothing to do; and
//! a render's flags draw the form's widgets alone, or nothing but the
//! page.
//!
//! Checks of the whole process (its threads, its memory) run in a process
//! of their own, the test binary run again for that one test.
//!
//! Needs libpdfium (`scripts/fetch-pdfium.sh`), like the app itself.

use std::collections::{BTreeSet, HashMap};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};

use futures::StreamExt;
use futures::channel::mpsc;
use futures::executor::block_on;
use lopdf::{Document as Pdf, Object, ObjectId, Stream, dictionary};
use sheaf_doc::forms::fixture::{Form, row};
use sheaf_doc::serialize::fixture::{USER, encrypted, form, rc4_aes_128_and_aes_256};
use sheaf_edit::Rotation;
use sheaf_image::raster::{PixelFormat, Raster};
use sheaf_render::pdfium::PdfiumEngine;
use sheaf_render::pool::{
    BACKGROUND_BYTES, BACKGROUND_ENGINES, OPEN_BYTES, Opened, PAGE_BYTES, SCREEN_BYTES,
    SCREEN_ENGINES,
};
use sheaf_render::{
    Budget, Draw, EnginePool, Fallback, Job, OpenError, Opener, PageIndex, Rendered, Renderer, Req,
    Target, Walked,
};

/// Set in a process that runs one test alone.
const ALONE: &str = "SHEAF_POOL_ALONE";

/// Whether this process is to do test `name`'s work: in the one the test
/// harness started, `name` is run again in a process of its own, which
/// must pass, and this one does nothing more.
fn alone(name: &str) -> bool {
    if std::env::var_os(ALONE).is_some() {
        return true;
    }
    let exe = std::env::current_exe().unwrap();
    let out = std::process::Command::new(exe)
        .args([name, "--exact", "--nocapture", "--test-threads=1"])
        .env(ALONE, "1")
        .output()
        .unwrap();
    let said = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
    eprint!("{}", said(&out.stderr));
    assert!(
        out.status.success(),
        "{name} alone: {}\n{}{}",
        out.status,
        said(&out.stdout),
        said(&out.stderr)
    );
    false
}

/// A file of `n` Letter pages, each drawing a blue box and "page i".
fn pdf(n: usize) -> Arc<[u8]> {
    let mut pdf = Pdf::with_version("1.7");
    let root = pdf.new_object_id();
    let font = pdf.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
    });
    let kids: Vec<Object> = (0..n)
        .map(|i| {
            let content =
                format!("0 0 1 rg 60 600 200 120 re f BT /F1 28 Tf 72 400 Td (page {i}) Tj ET");
            let content = pdf.add_object(Stream::new(dictionary! {}, content.into_bytes()));
            let page = dictionary! { "Type" => "Page", "Parent" => root, "Contents" => content };
            pdf.add_object(page).into()
        })
        .collect();
    pdf.objects.insert(
        root,
        Object::Dictionary(dictionary! {
            "Type" => "Pages", "Kids" => kids, "Count" => n as i64,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "Resources" => dictionary! { "Font" => dictionary! { "F1" => font } },
        }),
    );
    let catalog = pdf.add_object(dictionary! { "Type" => "Catalog", "Pages" => root });
    pdf.trailer.set("Root", catalog);
    let mut bytes = Vec::new();
    pdf.save_to(&mut bytes).unwrap();
    bytes.into()
}

/// A file of `n` Letter pages, each drawing a picture of its own, 400 ×
/// 400 pixels stored as they are, which its engine keeps once drawn.
fn scans(n: usize) -> Arc<[u8]> {
    let mut pdf = Pdf::with_version("1.7");
    let root = pdf.new_object_id();
    let kids: Vec<Object> = (0..n)
        .map(|i| {
            let pixels: Vec<u8> = (0..400 * 400 * 3)
                .map(|j| ((i * 7 + j * 13) % 251) as u8)
                .collect();
            let image = pdf.add_object(Stream::new(
                dictionary! {
                    "Type" => "XObject", "Subtype" => "Image", "Width" => 400, "Height" => 400,
                    "ColorSpace" => "DeviceRGB", "BitsPerComponent" => 8,
                },
                pixels,
            ));
            let content = b"q 500 0 0 500 56 146 cm /Im0 Do Q".to_vec();
            let content = pdf.add_object(Stream::new(dictionary! {}, content));
            let page = dictionary! {
                "Type" => "Page", "Parent" => root, "Contents" => content,
                "Resources" => dictionary! { "XObject" => dictionary! { "Im0" => image } },
            };
            pdf.add_object(page).into()
        })
        .collect();
    pdf.objects.insert(
        root,
        Object::Dictionary(dictionary! {
            "Type" => "Pages", "Kids" => kids, "Count" => n as i64,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        }),
    );
    let catalog = pdf.add_object(dictionary! { "Type" => "Catalog", "Pages" => root });
    pdf.trailer.set("Root", catalog);
    let mut bytes = Vec::new();
    pdf.save_to(&mut bytes).unwrap();
    bytes.into()
}

/// How often an engine was opened, and whether its source is still there.
#[derive(Clone, Default)]
struct Probe {
    opens: Arc<AtomicUsize>,
    gone: Arc<AtomicBool>,
}

impl Probe {
    fn opens(&self) -> usize {
        self.opens.load(Ordering::SeqCst)
    }

    /// An opener of `segments`, a base engine's or an overlay's, which
    /// counts its opens.
    fn opener(&self, segments: Vec<Arc<[u8]>>, base: bool) -> Opener {
        let (opens, gone) = (self.opens.clone(), self.gone.clone());
        let open = move || {
            opens.fetch_add(1, Ordering::SeqCst);
            Ok((segments.clone(), None))
        };
        let alive = move || !gone.load(Ordering::SeqCst);
        match base {
            true => Opener::base(open, alive),
            false => Opener::overlay(open, alive),
        }
    }
}

/// A pool of engines keyed by number, and each one's probe.
struct Pool {
    pool: EnginePool<u32>,
    probes: Mutex<HashMap<u32, Probe>>,
}

impl Pool {
    fn new() -> Pool {
        Pool {
            pool: EnginePool::new().unwrap(),
            probes: Mutex::default(),
        }
    }

    fn probe(&self, key: u32) -> Probe {
        self.probes.lock().unwrap().entry(key).or_default().clone()
    }

    /// Engine `key` on `bytes`, as a group's engine or a base engine, in
    /// `budget`.
    fn target(&self, key: u32, bytes: &Arc<[u8]>, base: bool, budget: Budget) -> Target<u32> {
        Target {
            engine: key,
            open: self.probe(key).opener(vec![bytes.clone()], base),
            budget,
        }
    }

    /// Page `page` of `target` drawn `width` pixels wide.
    fn draw(&self, target: Target<u32>, page: u32, width: u32) -> Rendered {
        let req = Req {
            target,
            page: PageIndex(page),
            job: Job::Page {
                width,
                rotation: Rotation::R0,
            },
            draw: Draw::ALL,
            format: PixelFormat::Rgba,
            priority: 0,
            fallback: None,
        };
        block_on(self.pool.render(req)).unwrap().unwrap()
    }

    /// Group engine `key`'s page 0 drawn, on the screen.
    fn group(&self, key: u32, bytes: &Arc<[u8]>) {
        self.draw(self.target(key, bytes, false, Budget::Screen), 0, 32);
    }

    fn open(&self) -> BTreeSet<u32> {
        self.pool.open_engines().into_iter().collect()
    }
}

/// The threads of this process.
#[cfg(target_os = "linux")]
fn tasks() -> usize {
    std::fs::read_dir("/proc/self/task").unwrap().count()
}

/// Opening 20 engines one after another, each serving a render, starts
/// one thread, the pool's, whatever the count of engines.
#[cfg(target_os = "linux")]
#[test]
fn one_thread_serves_every_engine() {
    if !alone("one_thread_serves_every_engine") {
        return;
    }
    let bytes = pdf(2);
    // PDFium bound, so the count is the pool's alone.
    PdfiumEngine::open_bytes(bytes.clone(), None).unwrap();
    let before = tasks();
    let pool = Pool::new();
    let mut after = Vec::new();
    for key in 0..20 {
        pool.draw(pool.target(key, &bytes, true, Budget::Screen), 0, 32);
        after.push(tasks());
    }
    assert_eq!(pool.open().len(), 20);
    assert!(after.iter().all(|&n| n == after[0]), "{after:?}");
    assert!(after[0] <= before + 1, "{before} then {after:?}");
}

/// The seventh group engine on the screen closes the one used longest
/// ago, not the first opened.
#[test]
fn the_seventh_group_engine_closes_the_least_recently_used_unpinned() {
    let bytes = pdf(1);
    let pool = Pool::new();
    for key in 0..SCREEN_ENGINES as u32 {
        pool.group(key, &bytes);
    }
    pool.group(0, &bytes);
    assert_eq!(pool.open(), (0..6).collect());
    pool.group(6, &bytes);
    assert_eq!(pool.open(), [0, 2, 3, 4, 5, 6].into());
    assert_eq!(pool.probe(0).opens(), 1);
}

/// Engines pinned stay open past the count, and engines opened beside
/// them close at once; unpinned, the least recently used close down to
/// the count.
#[test]
fn pinned_engines_are_never_closed() {
    let bytes = pdf(1);
    let pool = Pool::new();
    let pane = pool.pool.owner();
    pool.pool.pin(pane, (0..8).collect());
    for key in 0..8 {
        pool.group(key, &bytes);
    }
    for key in 8..11 {
        pool.group(key, &bytes);
        assert_eq!(pool.open(), (0..8).collect());
    }
    for key in 0..8 {
        pool.group(key, &bytes);
        assert_eq!(pool.probe(key).opens(), 1);
    }
    pool.pool.pin(pane, Vec::new());
    assert_eq!(pool.open(), (2..8).collect());
}

/// With six group engines pinned for the screen, two background walks
/// taking turns over ten groups each open two engines at most at once and
/// none twice, and close none of the screen's; nor do they with two of the
/// six unpinned, and a walk's use of one leaves the screen's order as it
/// was. A walk's engine the screen asks for becomes the screen's, and the
/// screen's count then closes its least recently used unpinned engine.
#[test]
fn background_walks_never_close_screen_engines() {
    let bytes = pdf(3);
    let pool = Pool::new();
    let screen: BTreeSet<u32> = (0..SCREEN_ENGINES as u32).collect();
    let pane = pool.pool.owner();
    pool.pool.pin(pane, screen.iter().copied().collect());
    for &key in &screen {
        pool.group(key, &bytes);
    }
    let background = |key| pool.target(key, &bytes, false, Budget::Background);
    let walks = |groups: std::ops::Range<u32>| {
        for group in groups.clone() {
            for page in 0..3 {
                for walk in [100, 200] {
                    pool.draw(background(walk + group), page, 64);
                    let open = pool.open();
                    assert!(open.is_superset(&screen), "{open:?}");
                    assert!(open.len() - screen.len() <= BACKGROUND_ENGINES, "{open:?}");
                }
            }
        }
        for key in groups.flat_map(|group| [100 + group, 200 + group]) {
            assert_eq!(pool.probe(key).opens(), 1, "{key}");
        }
    };
    walks(0..10);
    // 4 and 5 unpinned, 4 the less recently used.
    pool.pool.pin(pane, (0..4).collect());
    walks(10..20);
    pool.draw(background(4), 1, 64);
    assert_eq!(pool.probe(4).opens(), 1);
    pool.draw(pool.target(219, &bytes, false, Budget::Screen), 0, 64);
    let open = pool.open();
    assert!(open.contains(&219) && open.contains(&5), "{open:?}");
    assert!(!open.contains(&4), "{open:?}");
    let walk_left = OPEN_BYTES + 3 * PAGE_BYTES;
    assert_eq!(pool.pool.weighed(Budget::Background), walk_left);
}

/// The resident memory of this process, in bytes.
#[cfg(target_os = "linux")]
fn rss() -> usize {
    let status = std::fs::read_to_string("/proc/self/status").unwrap();
    let line = status.lines().find(|l| l.starts_with("VmRSS:")).unwrap();
    let kb: usize = line.split_whitespace().nth(1).unwrap().parse().unwrap();
    kb << 10
}

/// Visiting 200 groups, eight pages of each previewed, leaves what the
/// screen's engines weigh under its cap after every group, and the
/// process's memory no more than the cap and the file above where it
/// started. (Each engine keeps the pictures it drew: with none closed, the
/// process grew 759 MB.)
#[test]
fn engine_bytes_stay_under_the_cap_after_200_groups() {
    if !alone("engine_bytes_stay_under_the_cap_after_200_groups") {
        return;
    }
    let bytes = scans(128);
    let warm = PdfiumEngine::open_bytes(bytes.clone(), None).unwrap();
    warm.render_page(PageIndex(0), 320, PixelFormat::Bgra)
        .unwrap();
    drop(warm);
    #[cfg(target_os = "linux")]
    let before = rss();
    let pool = Pool::new();
    for key in 0..200 {
        let target = pool.target(key, &bytes, false, Budget::Screen);
        for page in (key % 16) * 8..(key % 16) * 8 + 8 {
            pool.draw(target.clone(), page, 320);
        }
        let weighed = pool.pool.weighed(Budget::Screen);
        assert!(weighed <= SCREEN_BYTES, "{weighed} after group {key}");
    }
    assert_eq!(pool.open().len(), SCREEN_ENGINES);
    #[cfg(target_os = "linux")]
    {
        let added = rss().saturating_sub(before);
        eprintln!("{} MB added", added >> 20);
        assert!(added <= SCREEN_BYTES + bytes.len(), "{added}");
    }
}

/// Holds the worker inside an opener, or a walk's work, until the test
/// lets it go.
#[derive(Default)]
struct Gate {
    /// Whether the worker is held, and whether it may go on.
    state: Mutex<(bool, bool)>,
    changed: Condvar,
}

impl Gate {
    fn opener(self: &Arc<Gate>, bytes: &Arc<[u8]>) -> Opener {
        let (gate, bytes) = (self.clone(), bytes.clone());
        let open = move || {
            gate.hold();
            Ok((vec![bytes.clone()], None))
        };
        Opener::overlay(open, || true)
    }

    /// Holds the calling thread until the test lets it go.
    fn hold(&self) {
        let mut state = self.state.lock().unwrap();
        state.0 = true;
        self.changed.notify_all();
        while !state.1 {
            state = self.changed.wait(state).unwrap();
        }
    }

    fn entered(&self) {
        let mut state = self.state.lock().unwrap();
        while !state.0 {
            state = self.changed.wait(state).unwrap();
        }
    }

    fn release(&self) {
        self.state.lock().unwrap().1 = true;
        self.changed.notify_all();
    }
}

/// A render of `target`'s page 0.
fn first(target: Target<u32>) -> Req<u32> {
    Req {
        target,
        page: PageIndex(0),
        job: Job::Page {
            width: 40,
            rotation: Rotation::R0,
        },
        draw: Draw::ALL,
        format: PixelFormat::Rgba,
        priority: 0,
        fallback: None,
    }
}

/// A render whose receiver is dropped while the worker is busy never
/// opens its engine.
#[test]
fn a_dropped_receiver_skips_its_render() {
    let bytes = pdf(1);
    let pool = Pool::new();
    let gate = Arc::new(Gate::default());
    let held = Target {
        engine: 0,
        open: gate.opener(&bytes),
        budget: Budget::Screen,
    };
    let busy = pool.pool.render(first(held));
    gate.entered();
    drop(
        pool.pool
            .render(first(pool.target(1, &bytes, false, Budget::Screen))),
    );
    gate.release();
    block_on(busy).unwrap().unwrap();
    assert_eq!(pool.open(), [0].into());
    assert_eq!(pool.probe(1).opens(), 0);
}

/// Five requests for one render of a page (its job, drawing and format)
/// queued while the worker is busy, as two documents showing one file
/// would ask, render once, and each is answered with the image; one drawn
/// otherwise, and one in another format, render on their own.
#[test]
fn renders_asked_for_together_render_once_for_each() {
    let bytes = pdf(1);
    let pool = Pool::new();
    let gate = Arc::new(Gate::default());
    let held = Target {
        engine: 0,
        open: gate.opener(&bytes),
        budget: Budget::Screen,
    };
    let busy = pool.pool.render(first(held));
    gate.entered();
    let burst: Vec<_> = (0..5)
        .map(|_| {
            let target = pool.target(1, &bytes, false, Budget::Screen);
            pool.pool.render(first(target))
        })
        .collect();
    let mut widgets_alone = first(pool.target(1, &bytes, false, Budget::Screen));
    widgets_alone.draw.annots = false;
    let other = pool.pool.render(widgets_alone);
    let mut bgra = first(pool.target(1, &bytes, false, Budget::Screen));
    bgra.format = PixelFormat::Bgra;
    let bgra = pool.pool.render(bgra);
    gate.release();
    block_on(busy).unwrap().unwrap();
    let done: Vec<Rendered> = (burst.into_iter())
        .map(|rx| block_on(rx).unwrap().unwrap())
        .collect();
    for rendered in &done[1..] {
        assert_eq!(rendered.render_time, done[0].render_time, "one render");
        assert_eq!(rendered.raster.data, done[0].raster.data);
    }
    let bgra = block_on(bgra).unwrap().unwrap();
    assert_eq!(bgra.raster.format, PixelFormat::Bgra);
    assert_eq!(done[0].raster.format, PixelFormat::Rgba);
    block_on(other).unwrap().unwrap();
    assert_eq!(pool.probe(1).opens(), 1);
}

/// An opener that panics, as one over a malformed file might, fails its
/// own requests with why, a render and a query alike, and isn't tried
/// again; the worker goes on serving every other engine.
#[test]
fn a_panic_fails_only_its_own_requests() {
    let bytes = pdf(1);
    let pool = Pool::new();
    let panics = Target {
        engine: 0,
        open: Opener::overlay(
            || -> anyhow::Result<Opened> { panic!("a malformed file") },
            || true,
        ),
        budget: Budget::Screen,
    };
    let failed = block_on(pool.pool.render(first(panics.clone())));
    let failed = failed.unwrap().err().expect("it panicked");
    assert!(
        format!("{failed:#}").contains("a malformed file"),
        "{failed:#}"
    );
    let asked = block_on(pool.pool.form_type(panics)).unwrap();
    let asked = asked.expect_err("nor for a query");
    assert_eq!(format!("{failed:#}"), format!("{asked:#}"));
    pool.group(1, &bytes);
    assert_eq!(pool.open(), [1].into());
}

/// An engine whose source is gone closes at the worker's next turn, and
/// lets go of its bytes.
#[test]
fn an_engine_whose_source_is_gone_closes() {
    let bytes = pdf(1);
    let pool = Pool::new();
    pool.group(0, &bytes);
    pool.group(1, &pdf(1));
    assert_eq!(pool.open(), [0, 1].into());
    assert!(Arc::strong_count(&bytes) > 1);
    pool.probe(0).gone.store(true, Ordering::SeqCst);
    assert_eq!(pool.open(), [1].into());
    assert_eq!(Arc::strong_count(&bytes), 1);
}

/// An engine that doesn't open is tried once while its source lives:
/// asked for again, by a render or a query, it fails as it did, a wrong
/// password still told as one, and isn't opened. Once its source is gone
/// it's forgotten, and its key opens anew.
#[test]
fn an_engine_that_fails_to_open_is_tried_once() {
    let pool = Pool::new();
    let junk: Arc<[u8]> = Arc::from(&b"%PDF-1.7 and nothing more"[..]);
    let render = |key, bytes: &Arc<[u8]>| {
        let target = pool.target(key, bytes, true, Budget::Screen);
        block_on(pool.pool.render(first(target))).unwrap().err()
    };
    let failed = render(0, &junk).expect("junk doesn't open");
    let again = render(0, &junk).expect("nor again");
    let asked = block_on((pool.pool).form_type(pool.target(0, &junk, true, Budget::Screen)));
    let asked = asked.unwrap().expect_err("nor for a query");
    assert_eq!(pool.probe(0).opens(), 1);
    assert_eq!(format!("{failed:#}"), format!("{again:#}"));
    assert_eq!(format!("{failed:#}"), format!("{asked:#}"));
    let plain = form();
    let [(_, state), ..] = rc4_aes_128_and_aes_256(&plain);
    let [locked, _] = encrypted(&plain, &state, |_| {});
    let locked: Arc<[u8]> = locked.into();
    for _ in 0..2 {
        let err = render(1, &locked).expect("no password");
        assert!(OpenError::is_password(&err), "{err:#}");
    }
    assert_eq!(pool.probe(1).opens(), 1);
    pool.probe(0).gone.store(true, Ordering::SeqCst);
    assert!(pool.open().is_empty());
    pool.probe(0).gone.store(false, Ordering::SeqCst);
    assert!(render(0, &pdf(1)).is_none());
    assert_eq!((pool.probe(0).opens(), pool.open()), (2, [0].into()));
}

/// An engine PDFium refuses tells its opener, at its failed open and at
/// each request after, and a render with a fallback draws its page from
/// there instead; a failure that isn't a refusal (a password) tells
/// nothing and falls back to nothing, and a render without one fails.
#[test]
fn a_refused_engine_tells_its_opener_and_renders_from_the_fallback() {
    let pool = Pool::new();
    let junk: Arc<[u8]> = Arc::from(&b"%PDF-1.7 and nothing more"[..]);
    let told = Arc::new(AtomicUsize::new(0));
    let target = |key, bytes: &Arc<[u8]>| {
        let told = told.clone();
        let mut target = pool.target(key, bytes, true, Budget::Screen);
        target.open = target.open.on_refused(move || {
            told.fetch_add(1, Ordering::SeqCst);
        });
        target
    };
    let bytes = pdf(3);
    let fallen = |key, bytes: &Arc<[u8]>| {
        let instead = pool.target(9, &pdf(3), false, Budget::Screen);
        Req {
            fallback: Some(Box::new(move || Ok((instead, PageIndex(2))))),
            ..first(target(key, bytes))
        }
    };
    let drawn = block_on(pool.pool.render(fallen(0, &junk)))
        .unwrap()
        .unwrap();
    assert_eq!(told.load(Ordering::SeqCst), 1);
    assert_eq!(drawn.request.page, PageIndex(2));
    let want = pool.draw(pool.target(1, &bytes, true, Budget::Screen), 2, 40);
    assert_eq!(drawn.raster.data, want.raster.data);
    let failed = block_on(pool.pool.render(first(target(0, &junk)))).unwrap();
    assert!(OpenError::is_refused(&failed.err().expect("no fallback")));
    let asked = block_on(pool.pool.form_type(target(0, &junk))).unwrap();
    assert!(OpenError::is_refused(&asked.unwrap_err()));
    assert_eq!((told.load(Ordering::SeqCst), pool.probe(0).opens()), (3, 1));
    let plain = form();
    let [(_, state), ..] = rc4_aes_128_and_aes_256(&plain);
    let [locked, _] = encrypted(&plain, &state, |_| {});
    let locked: Arc<[u8]> = locked.into();
    let err = block_on(pool.pool.render(fallen(2, &locked)))
        .unwrap()
        .err();
    let err = err.expect("no fallback for a password");
    assert!(OpenError::is_password(&err), "{err:#}");
    assert_eq!(told.load(Ordering::SeqCst), 3);
}

/// A query of an engine PDFium refuses reads its page from its fallback
/// instead, run once, as a render does, and is given the page it reads
/// there; without a fallback, it fails as refused. A fallback that panics
/// fails its own query alone, and a failure that isn't a refusal (a
/// password) falls back to nothing.
#[test]
fn a_refused_query_reads_from_its_fallback() {
    let pool = Pool::new();
    let junk: Arc<[u8]> = Arc::from(&b"%PDF-1.7 and nothing more"[..]);
    let refused = || pool.target(0, &junk, true, Budget::Screen);
    let runs = Arc::new(AtomicUsize::new(0));
    let instead = |runs: &Arc<AtomicUsize>| -> Fallback<u32> {
        let (runs, there) = (runs.clone(), pool.target(9, &pdf(3), false, Budget::Screen));
        Box::new(move || {
            runs.fetch_add(1, Ordering::SeqCst);
            Ok((there, PageIndex(2)))
        })
    };
    let text = |e: &PdfiumEngine, page: PageIndex| Ok((page, e.page_text(page)?));
    let ask = |target, fallback| block_on(pool.pool.query(target, PageIndex(0), fallback, text));
    let read = ask(refused(), Some(instead(&runs))).unwrap().unwrap();
    assert_eq!(read, (PageIndex(2), "page 2".to_string()));
    assert_eq!(runs.load(Ordering::SeqCst), 1);
    let failed = ask(refused(), None).unwrap().expect_err("no fallback");
    assert!(OpenError::is_refused(&failed), "{failed:#}");
    let panics: Fallback<u32> = Box::new(|| panic!("a malformed fallback"));
    let failed = ask(refused(), Some(panics))
        .unwrap()
        .expect_err("it panicked");
    assert!(
        format!("{failed:#}").contains("a malformed fallback"),
        "{failed:#}"
    );
    assert_eq!(pool.probe(0).opens(), 1);
    let plain = form();
    let [(_, state), ..] = rc4_aes_128_and_aes_256(&plain);
    let [locked, _] = encrypted(&plain, &state, |_| {});
    let locked = pool.target(1, &locked.into(), true, Budget::Screen);
    let failed = ask(locked, Some(instead(&runs))).unwrap();
    let failed = failed.expect_err("no fallback for a password");
    assert!(OpenError::is_password(&failed), "{failed:#}");
    assert_eq!(runs.load(Ordering::SeqCst), 1);
    let read = ask(pool.target(2, &pdf(1), true, Budget::Screen), None);
    assert_eq!(read.unwrap().unwrap(), (PageIndex(0), "page 0".to_string()));
}

/// A base engine, pinned, grows with every page it loads until the
/// screen's engines weigh more than the cap: then it closes, and opens
/// again at the next request.
#[test]
fn a_base_engine_is_reopened_past_its_bound() {
    let bound = (SCREEN_BYTES - OPEN_BYTES) / PAGE_BYTES + 1;
    let bytes = pdf(bound + 4);
    let pool = Pool::new();
    pool.pool.pin(pool.pool.owner(), vec![0]);
    let len = |page: usize| {
        let target = pool.target(0, &bytes, true, Budget::Screen);
        let count = |e: &PdfiumEngine, page| e.page_text_len(page);
        block_on(pool.pool.query(target, PageIndex(page as u32), None, count))
            .unwrap()
            .unwrap()
    };
    for page in 0..bound - 1 {
        assert_eq!(len(page), 4 + page.to_string().len(), "page {page}");
    }
    assert_eq!((pool.probe(0).opens(), pool.open()), (1, [0].into()));
    assert!(pool.pool.weighed(Budget::Screen) <= SCREEN_BYTES);
    len(bound - 1);
    assert!(pool.open().is_empty());
    len(bound);
    assert_eq!((pool.probe(0).opens(), pool.open()), (2, [0].into()));
}

/// A background base engine isn't held to the background's bytes: it
/// grows into what the screen's engines leave of their cap, a group's
/// engine of the screen's open beside it, and past that closes and opens
/// again at the next request, as the screen's own does. It never closes
/// the screen's engine to make room.
#[test]
fn a_background_base_engine_grows_into_what_the_screen_leaves() {
    let screen = OPEN_BYTES + PAGE_BYTES;
    let bound = (SCREEN_BYTES - screen - OPEN_BYTES) / PAGE_BYTES + 1;
    assert!(OPEN_BYTES + bound * PAGE_BYTES > 2 * BACKGROUND_BYTES);
    let bytes = pdf(bound + 4);
    let pool = Pool::new();
    pool.group(9, &bytes);
    let len = |page: usize| {
        let target = pool.target(0, &bytes, true, Budget::Background);
        let count = |e: &PdfiumEngine, page| e.page_text_len(page);
        block_on(pool.pool.query(target, PageIndex(page as u32), None, count))
            .unwrap()
            .unwrap()
    };
    for page in 0..bound - 1 {
        assert_eq!(len(page), 4 + page.to_string().len(), "page {page}");
    }
    assert_eq!((pool.probe(0).opens(), pool.open()), (1, [0, 9].into()));
    assert_eq!(pool.pool.weighed(Budget::Screen), screen);
    len(bound - 1);
    assert_eq!(pool.open(), [9].into());
    len(bound);
    assert_eq!((pool.probe(0).opens(), pool.open()), (2, [0, 9].into()));
    assert_eq!(pool.probe(9).opens(), 1);
}

/// A walk keeps the engine it reads on while it goes on reading there:
/// with the screen's count filled by pinned engines, a walk over three
/// groups' pages, each group's in a run, opens each group's engine once,
/// and lets the last go when it ends.
#[test]
fn a_walk_keeps_its_engine_past_pinned_ones() {
    let bytes = pdf(3);
    let pool = Pool::new();
    let pinned: BTreeSet<u32> = (0..SCREEN_ENGINES as u32).collect();
    pool.pool
        .pin(pool.pool.owner(), pinned.iter().copied().collect());
    for &key in &pinned {
        pool.group(key, &bytes);
    }
    let targets: Vec<Target<u32>> = (100..103)
        .map(|key| pool.target(key, &bytes, false, Budget::Screen))
        .collect();
    let item = move |at: usize| Ok((targets[at / 9].clone(), PageIndex(at as u32 % 3), None));
    let work = |_: &PdfiumEngine, _, _| Ok(());
    let (tx, rx) = mpsc::unbounded::<Walked>();
    pool.pool.walk(27, item, work, tx);
    let told: Vec<Walked> = block_on(rx.collect());
    assert_eq!(told.last(), Some(&Walked::Done));
    let opens = [100, 101, 102].map(|key| pool.probe(key).opens());
    assert_eq!(opens, [1; 3]);
    assert_eq!(pool.open(), pinned);
}

/// Encrypted files, RC4-128, AES-128 and AES-256, with a cross-reference
/// table and a stream, cut into segments anywhere, open with their
/// password through the pool and draw as the whole file does; without
/// it, they ask for it.
#[test]
fn two_segments_open_encrypted_bases() {
    let plain = form();
    let pool = EnginePool::new().unwrap();
    let mut key = 0;
    for (name, state) in rc4_aes_128_and_aes_256(&plain) {
        for bytes in encrypted(&plain, &state, |_| {}) {
            let whole = PdfiumEngine::open_bytes(bytes.clone().into(), Some(USER)).unwrap();
            let want = whole
                .render_page(PageIndex(0), 300, PixelFormat::Rgba)
                .unwrap();
            let n = bytes.len();
            let cuts = [
                vec![1],
                vec![n / 2],
                vec![n - 1],
                vec![n / 3, 2 * n / 3],
                vec![7, 8],
            ];
            for cut in cuts {
                let mut segments: Vec<Arc<[u8]>> = Vec::new();
                let mut from = 0;
                for at in cut.iter().copied().chain([n]) {
                    segments.push(bytes[from..at].into());
                    from = at;
                }
                key += 1;
                let opened = (segments, Some(USER.to_string()));
                let open = Opener::overlay(move || Ok(opened.clone()), || true);
                let target = Target {
                    engine: key,
                    open,
                    budget: Budget::Screen,
                };
                let mut req = first(target);
                req.job = Job::Page {
                    width: 300,
                    rotation: Rotation::R0,
                };
                let got = block_on(pool.render(req)).unwrap().unwrap().raster;
                assert_eq!(differing(&got, &want), 0, "{name} cut at {cut:?}");
            }
            let segments: Vec<Arc<[u8]>> = vec![bytes[..n / 2].into(), bytes[n / 2..].into()];
            let err = PdfiumEngine::open_segments(segments, None).err().unwrap();
            assert!(OpenError::is_password(&err), "{name}: {err:#}");
        }
    }
}

/// The CPU time thread `task` of this process has had, in clock ticks.
#[cfg(target_os = "linux")]
fn cpu(task: u32) -> u64 {
    let stat = std::fs::read_to_string(format!("/proc/self/task/{task}/stat")).unwrap();
    let fields: Vec<&str> = stat.rsplit_once(") ").unwrap().1.split(' ').collect();
    // utime and stime, the 14th and 15th fields, past the command's.
    fields[11].parse::<u64>().unwrap() + fields[12].parse::<u64>().unwrap()
}

/// What a walk's work and the walk itself tell, in the order told.
#[derive(Debug, PartialEq)]
enum Told {
    /// The page read at a position, and its text.
    Read(usize, PageIndex, String),
    Walked(Walked),
}

impl From<Walked> for Told {
    fn from(walked: Walked) -> Told {
        Told::Walked(walked)
    }
}

/// A walk reads the pages at its positions in their order, four a turn,
/// each on the engine its item names: a base engine's, a group's, and one
/// PDFium refuses, whose pages are read from their fallback's engine and
/// page, a fallback run for each; a page whose item or work panics is
/// passed over alone. It tells how far it has got after each turn, in
/// order with what its work tells, and last that it's done. Then the worker
/// spends no CPU while nothing is asked of it: a tick at most, of its way
/// back to its channel after answering where it is (a busy or polling
/// worker takes about 30).
#[test]
fn a_walk_reads_pages_of_many_engines_in_order_and_then_idles() {
    let bytes = pdf(40);
    let junk: Arc<[u8]> = Arc::from(&b"%PDF-1.7 and nothing more"[..]);
    let pool = Pool::new();
    let targets = [
        pool.target(0, &bytes, true, Budget::Screen),
        pool.target(1, &bytes, false, Budget::Screen),
        pool.target(2, &junk, true, Budget::Screen),
    ];
    let instead = pool.target(3, &bytes, false, Budget::Screen);
    let runs = Arc::new(AtomicUsize::new(0));
    let fallen = runs.clone();
    // Position `at` reads page 39 - at.
    let item = move |at: usize| {
        if at == 7 {
            panic!("a malformed page's view");
        }
        let page = PageIndex(39 - at as u32);
        let (instead, fallen) = (instead.clone(), fallen.clone());
        let fallback: Fallback<u32> = Box::new(move || {
            fallen.fetch_add(1, Ordering::SeqCst);
            Ok((instead, page))
        });
        Ok((targets[at % 3].clone(), page, Some(fallback)))
    };
    let (tx, rx) = mpsc::unbounded::<Told>();
    let read = tx.clone();
    let work = move |e: &PdfiumEngine, page: PageIndex, at: usize| {
        if at == 11 {
            panic!("a malformed page");
        }
        let _ = read.unbounded_send(Told::Read(at, page, e.page_text(page)?));
        Ok(())
    };
    pool.pool.walk(30, item, work, tx);
    let told: Vec<Told> = block_on(rx.collect());
    let mut want = Vec::new();
    for turn in (0..30).step_by(4) {
        for at in (turn..turn + 4).take_while(|&at| at < 30) {
            if at != 7 && at != 11 {
                let page = 39 - at;
                want.push(Told::Read(
                    at,
                    PageIndex(page as u32),
                    format!("page {page}"),
                ));
            }
        }
        want.push(Told::Walked(Walked::Scanned((turn + 4).min(30))));
    }
    want.push(Told::Walked(Walked::Done));
    assert_eq!(told, want);
    // Every third position from 2 is on the refused engine, opened once.
    assert_eq!(runs.load(Ordering::SeqCst), 10);
    let opens = [0, 1, 2, 3].map(|key| pool.probe(key).opens());
    assert_eq!(opens, [1; 4]);
    #[cfg(target_os = "linux")]
    {
        let task = pool.pool.worker_task().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(50));
        let was = cpu(task);
        std::thread::sleep(std::time::Duration::from_millis(300));
        let took = cpu(task) - was;
        assert!(took <= 1, "{took} ticks");
    }
}

/// A walk whose receiver is dropped ends at its next turn: the turn it was
/// dropped in reads its pages, and no page after; and the walk lets go of
/// what it holds.
#[test]
fn a_dropped_receiver_ends_its_walk() {
    let bytes = pdf(1);
    let pool = Pool::new();
    let gate = Arc::new(Gate::default());
    let (held, reads) = (Arc::new(()), Arc::new(AtomicUsize::new(0)));
    let target = pool.target(0, &bytes, true, Budget::Screen);
    // The walk holds what its item does, as a search holds its state.
    let kept = held.clone();
    let item = move |_| {
        let _ = &kept;
        Ok((target.clone(), PageIndex(0), None))
    };
    let (counted, waits) = (reads.clone(), gate.clone());
    let work = move |_: &PdfiumEngine, _, at: usize| {
        counted.fetch_add(1, Ordering::SeqCst);
        if at == 5 {
            waits.hold();
        }
        Ok(())
    };
    let (tx, rx) = mpsc::unbounded::<Walked>();
    pool.pool.walk(40, item, work, tx);
    gate.entered();
    drop(rx);
    gate.release();
    let text = |e: &PdfiumEngine, page| e.page_text(page);
    for _ in 0..2 {
        let target = pool.target(0, &bytes, true, Budget::Screen);
        block_on(pool.pool.query(target, PageIndex(0), None, text))
            .unwrap()
            .unwrap();
        assert_eq!(reads.load(Ordering::SeqCst), 8, "the turn of 4 to 7");
    }
    assert_eq!(Arc::strong_count(&held), 1);
}

/// What a page's `/Annots` lists in [`annotated`].
#[derive(Clone, Copy)]
enum Annots {
    /// A text field's widget, another tool's square and a link.
    All,
    Widget,
    /// The square and the link.
    Theirs,
    None,
}

/// Annotation `d` at `rect`, its appearance a box of `color` filling it.
fn with_appearance(f: &mut Form, d: lopdf::Dictionary, rect: [i64; 4], color: &str) -> ObjectId {
    let [x0, y0, x1, y1] = rect;
    let ap = f.pdf.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject", "Subtype" => "Form",
            "BBox" => vec![0.into(), 0.into(), (x1 - x0).into(), (y1 - y0).into()],
        },
        format!("{color} rg 0 0 {} {} re f", x1 - x0, y1 - y0).into_bytes(),
    ));
    let mut d = d;
    d.set("Rect", rect.map(Object::from).to_vec());
    d.set("AP", dictionary! { "N" => ap });
    f.pdf.add_object(d)
}

/// A `width` × `height` page with a filled text field, another tool's
/// square and a link with its appearance, `annots` of them listed in its
/// `/Annots`.
fn annotated(annots: Annots, width: f32, height: f32) -> Arc<[u8]> {
    let mut f = Form::new(1);
    let media = [0.0, 0.0, width, height].map(Object::from).to_vec();
    f.dict(f.pages[0]).set("MediaBox", media);
    let field = f.text("name", 0, row(1));
    f.fill(field, "Ann");
    let square = dictionary! { "Type" => "Annot", "Subtype" => "Square" };
    let square = with_appearance(&mut f, square, [300, 300, 380, 380], "0 0.6 0");
    let link = dictionary! { "Type" => "Annot", "Subtype" => "Link" };
    let link = with_appearance(&mut f, link, [300, 500, 400, 520], "1 0 1");
    let listed = match annots {
        Annots::All => vec![field, square, link],
        Annots::Widget => vec![field],
        Annots::Theirs => vec![square, link],
        Annots::None => Vec::new(),
    };
    f.set_annots(0, &listed);
    f.bytes().into()
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

/// With annotations off and forms on, a page draws its widget alone, as
/// the page listing only it does with everything on; with forms off too,
/// it draws as the page with no annotations; with annotations on and
/// forms off, as the page without its widget: a whole page and a tile,
/// turned each way, of a Letter page and an A4 one, whose size isn't whole
/// points.
#[test]
fn annotation_flags_draw_widgets_alone() {
    let pool = Pool::new();
    let kinds = [Annots::All, Annots::Widget, Annots::Theirs, Annots::None];
    for (size, (width, height)) in [(612.0, 792.0), (595.3, 841.9)].into_iter().enumerate() {
        let files = kinds.map(|annots| annotated(annots, width, height));
        let full = (816, (816.0 * height / width).round() as u32);
        let rotations = [Rotation::R0, Rotation::R90, Rotation::R180, Rotation::R270];
        let jobs = rotations.into_iter().flat_map(|rotation| {
            let page = Job::Page {
                width: 400,
                rotation,
            };
            let (page_width, page_height) = match rotation.is_quarter() {
                true => (full.1, full.0),
                false => full,
            };
            // The page but a tenth at each side, where all three are.
            let tile = Job::Tile {
                page_width,
                page_height,
                x: page_width / 10,
                y: page_height / 10,
                w: page_width * 8 / 10,
                h: page_height * 8 / 10,
                rotation,
            };
            [page, tile]
        });
        for job in jobs {
            let shot = |file: usize, draw: Draw| {
                let key = (4 * size + file) as u32;
                let req = Req {
                    target: pool.target(key, &files[file], true, Budget::Screen),
                    page: PageIndex(0),
                    job,
                    draw,
                    format: PixelFormat::Rgba,
                    priority: 0,
                    fallback: None,
                };
                block_on(pool.pool.render(req)).unwrap().unwrap().raster
            };
            let [all, widget, theirs, none] = [0, 1, 2, 3].map(|file| shot(file, Draw::ALL));
            let flags = |annots, forms| shot(0, Draw { annots, forms });
            let at = format!("{width} x {height}, {job:?}");
            assert_eq!(differing(&flags(false, true), &widget), 0, "{at}");
            assert_eq!(differing(&flags(false, false), &none), 0, "{at}");
            assert_eq!(differing(&flags(true, false), &theirs), 0, "{at}");
            assert_eq!(differing(&flags(true, true), &all), 0, "{at}");
            for (a, b) in [(&all, &widget), (&widget, &none), (&theirs, &none)] {
                assert_ne!(differing(a, b), 0, "{at}: each draws something");
            }
        }
    }
}
