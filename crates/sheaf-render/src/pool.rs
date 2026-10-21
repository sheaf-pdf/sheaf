//! One worker thread for every engine (M10.5 §10.3).
//!
//! An [`EnginePool`] owns the engines its requests ask for, each under a
//! key its caller chooses (sheaf-ops' `Views` uses the realizer's
//! `EngineKey`). A request names its engine, how to open it ([`Opener`])
//! and whose budget it opens in ([`Budget`]); the worker opens an engine
//! when a request first needs it, outside PDFium's lock, and closes engines
//! by these rules:
//!
//! - **Pinned** engines, those serving what each pane and the thumbnail
//!   strip show ([`EnginePool::pin`]), are the screen's, and no budget
//!   closes them.
//! - **The screen's** group and patch engines are at most
//!   [`SCREEN_ENGINES`], pinned ones included, and all its engines, base
//!   engines too, weigh at most [`SCREEN_BYTES`]; past either, the least
//!   recently used unpinned engine closes first. A base engine is outside
//!   the count but not the bytes, and grows with every page it loads
//!   (slice 2: 91 MB on the synthetic 7k after previewing every page, 338
//!   MB on the heavy): once the screen's engines weigh more than the cap
//!   with no unpinned engine left to close, the heaviest base engine
//!   heavier than [`ENGINE_BYTES`] closes, pinned or not, and opens again
//!   at its next request, whoever's request took it past.
//! - **Background** requests (OCR, blank detection, Trim White, plugins'
//!   text and page scans) use any open engine, leaving the screen's order
//!   alone, and otherwise open one of their own, at most
//!   [`BACKGROUND_ENGINES`], so they never close one of the screen's to
//!   make room. Their group and patch engines weigh at most
//!   [`BACKGROUND_BYTES`]; their base engines grow into what the screen's
//!   engines leave of [`SCREEN_BYTES`], and past it the heaviest heavier
//!   than [`ENGINE_BYTES`] closes, so a walk over a 7k document opens its
//!   base engine again about as often as the screen's does (not every 400
//!   pages). A screen request for one of theirs makes it the screen's.
//! - **Walks** hold the engine they read their last page on: neither
//!   budget's count or bytes close it, as if pinned, until the walk reads
//!   on another or ends, so a walk past pinned engines filling
//!   [`SCREEN_ENGINES`] doesn't open its group's engine again for each
//!   page.
//! - **Liveness:** an engine whose [`Opener`] says its source is gone
//!   closes at the worker's next turn.
//! - **Failures:** an engine that doesn't open isn't tried again while its
//!   source lives: its requests fail with why it didn't. An open does what
//!   it did before (a key names what its engine opens), and a damaged
//!   file's repair takes tens of milliseconds a try. An opener that asks
//!   is told when PDFium refuses its engine's bytes ([`OpenError::Refused`],
//!   [`Opener::on_refused`]), and a render or a query that says where else
//!   its page is ([`Req::fallback`], [`EnginePool::query`]) reads it from
//!   there instead.
//!
//! PDFium tells nothing of an engine's memory, so engines are weighed by
//! an estimate: [`OPEN_BYTES`], the bytes of the segments the engine alone
//! holds (a section, or a file of its own; not a source's bytes, which its
//! document holds), and [`PAGE_BYTES`] for each page it has loaded.
//!
//! Requests keep the old worker's ways: lower priorities run first in a
//! batch (0 previews, 1 visible tiles, 2–3 prefetch, `u8::MAX` background);
//! renders of one page of one engine by one job, drawing and format in a
//! batch render once, and each is answered with the image, whoever asked
//! (the pool is the whole process's); a render whose receivers are all
//! dropped is skipped; queries run in the order they came, before the
//! batch's renders; and a walk over a document's pages (Find's, Patterns';
//! [`EnginePool::walk`]) reads them `WALK_CHUNK` at a time between
//! renders, in their order, each on its own engine and falling back as a
//! query does. The worker blocks on its channel when no walk is pending,
//! and ends when the last handle is dropped.
//!
//! **Panics:** an opener, a fallback, a render, a query or a walk's page
//! that panics (a malformed file) fails its own request, or page, with
//! why, as an engine that doesn't open does, and the worker goes on
//! serving every other.

use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::io::{self, Read, Seek, SeekFrom};
use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, mpsc};
use std::time::Instant;

use anyhow::{Context as _, Result, anyhow};
use futures::channel::{mpsc as stream, oneshot};
use sheaf_image::raster::PixelFormat;

use crate::pdfium::PdfiumEngine;
use crate::{Draw, FormType, Job, OpenError, PageIndex, RenderRequest, Rendered, Renderer};

/// The screen's group and patch engines at most, pinned ones included:
/// two groups for each pane of a split window and two for the thumbnail
/// strip (slice 2's count).
pub const SCREEN_ENGINES: usize = 6;

/// What the screen's engines weigh at most, its base engines included:
/// [`SCREEN_ENGINES`] at slice 2's per-engine cap, [`ENGINE_BYTES`].
pub const SCREEN_BYTES: usize = 96 << 20;

/// Background consumers' own engines at most: one for each of two walks
/// taking turns (slice 2's rule).
pub const BACKGROUND_ENGINES: usize = 2;

/// What background consumers' own engines weigh at most:
/// [`BACKGROUND_ENGINES`] at the per-engine cap.
pub const BACKGROUND_BYTES: usize = 32 << 20;

/// Slice 2's per-engine cap, which a group's engine fits with every page
/// of its group loaded. A base engine is closed for the screen's bytes
/// only once it weighs more, so one just opened again isn't closed at once.
pub const ENGINE_BYTES: usize = 16 << 20;

/// Pages a walk reads a turn, between renders.
const WALK_CHUNK: usize = 4;

/// What an engine weighs open, by estimate: slice 2's scoped group engines
/// measured 0.8, 3.6 and 1.2 MB before their pages (the synthetic, heavy
/// and scanned 7k's, from 8 and 128 pages loaded), the heavy's rounded up.
pub const OPEN_BYTES: usize = 4 << 20;

/// What each page an engine has loaded adds, by estimate: slice 2's
/// scoped engines grew 15, 50 and 74 KB a page (synthetic, heavy, scanned)
/// and its base engines 13 and 50 KB; the scanned's, rounded up. A group
/// of 128 then weighs 13.4 MB, inside [`ENGINE_BYTES`].
pub const PAGE_BYTES: usize = 75 << 10;

/// Bytes read one segment after another as one file: a source's bytes and
/// a section after them, with no copy of either.
pub(crate) struct Segments {
    parts: Vec<Arc<[u8]>>,
    /// Where each part starts.
    starts: Vec<u64>,
    len: u64,
    pos: u64,
}

impl Segments {
    pub(crate) fn new(segments: Vec<Arc<[u8]>>) -> Segments {
        let parts: Vec<Arc<[u8]>> = segments.into_iter().filter(|s| !s.is_empty()).collect();
        let mut starts = Vec::with_capacity(parts.len());
        let mut len = 0;
        for part in &parts {
            starts.push(len);
            len += part.len() as u64;
        }
        Segments {
            parts,
            starts,
            len,
            pos: 0,
        }
    }
}

impl Read for Segments {
    /// Fills `buf` as far as the bytes go, across the segments'
    /// boundaries: pdfium-render hands each of PDFium's block requests to
    /// one read, and PDFium takes any count but zero for the whole block
    /// (slice 2).
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut n = 0;
        while n < buf.len() && self.pos < self.len {
            let i = self.starts.partition_point(|&s| s <= self.pos) - 1;
            let part = &self.parts[i][(self.pos - self.starts[i]) as usize..];
            let k = part.len().min(buf.len() - n);
            buf[n..n + k].copy_from_slice(&part[..k]);
            n += k;
            self.pos += k as u64;
        }
        Ok(n)
    }
}

impl Seek for Segments {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        let pos = match to {
            SeekFrom::Start(pos) => Some(pos),
            SeekFrom::End(by) => self.len.checked_add_signed(by),
            SeekFrom::Current(by) => self.pos.checked_add_signed(by),
        };
        self.pos =
            pos.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "before the start"))?;
        Ok(self.pos)
    }
}

/// What an engine opens: its segments, and the password they're read
/// with.
pub type Opened = (Vec<Arc<[u8]>>, Option<String>);

type Open = dyn Fn() -> Result<Opened> + Send + Sync;
type Alive = dyn Fn() -> bool + Send + Sync;
type Refused = dyn Fn() + Send + Sync;

/// How the pool opens an engine it hasn't got open, and tells when to
/// close it. `open` runs on the worker, and again whenever the engine has
/// been closed since; `alive` says whether what the engine shows is still
/// a document's; `refused`, when there's one, is told that PDFium refused
/// the engine's bytes ([`Opener::on_refused`]).
#[derive(Clone)]
pub struct Opener {
    open: Arc<Open>,
    alive: Arc<Alive>,
    refused: Option<Arc<Refused>>,
    base: bool,
}

impl Opener {
    /// A source's own bytes: a base engine, outside the screen's count.
    pub fn base(
        open: impl Fn() -> Result<Opened> + Send + Sync + 'static,
        alive: impl Fn() -> bool + Send + Sync + 'static,
    ) -> Opener {
        Opener {
            open: Arc::new(open),
            alive: Arc::new(alive),
            refused: None,
            base: true,
        }
    }

    /// This opener, `refused` run on the worker when PDFium refuses its
    /// engine's bytes ([`OpenError::Refused`]): at the failed open, and at
    /// each request after that's told of it, so a document whose source
    /// another one's request found refused learns it too.
    pub fn on_refused(self, refused: impl Fn() + Send + Sync + 'static) -> Opener {
        Opener {
            refused: Some(Arc::new(refused)),
            ..self
        }
    }

    /// The error a request of this opener's engine fails with, it having
    /// failed to open for `why`; telling the opener first when PDFium
    /// refused it.
    fn unopened(&self, why: Arc<anyhow::Error>) -> anyhow::Error {
        if let Some(refused) = &self.refused
            && OpenError::is_refused(&why)
        {
            refused();
        }
        Unopened(why).into()
    }

    /// An overlay: a group's, or a source's view patch.
    pub fn overlay(
        open: impl Fn() -> Result<Opened> + Send + Sync + 'static,
        alive: impl Fn() -> bool + Send + Sync + 'static,
    ) -> Opener {
        Opener {
            base: false,
            ..Opener::base(open, alive)
        }
    }
}

/// Whose engines a request may open, and close to make room.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Budget {
    /// The page view's and the thumbnail strip's, dialogs' previews, and
    /// Find's and selection's text.
    Screen,
    /// Walks over a document's pages: OCR, blank detection, Trim White,
    /// plugins' text and scans.
    Background,
}

/// An engine to use: its key, how to open it, and whose budget it opens
/// in.
#[derive(Clone)]
pub struct Target<K> {
    pub engine: K,
    pub open: Opener,
    pub budget: Budget,
}

/// Where else a render's or a query's page is, should PDFium refuse its
/// engine's bytes: an engine and the page's index there. Run on the
/// worker, once.
pub type Fallback<K> = Box<dyn FnOnce() -> Result<(Target<K>, PageIndex)> + Send>;

/// How far a walk over a document's pages has got ([`EnginePool::walk`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Walked {
    /// The pages at the first `n` positions are read.
    Scanned(usize),
    /// Every page is read.
    Done,
}

/// A render of a page of an engine.
pub struct Req<K> {
    pub target: Target<K>,
    pub page: PageIndex,
    pub job: Job,
    pub draw: Draw,
    pub format: PixelFormat,
    /// Lower runs sooner within a batch: 0 previews, 1 visible tiles, 2–3
    /// prefetch, `u8::MAX` background.
    pub priority: u8,
    /// Where the page renders from, by the same job and drawing, when
    /// PDFium refuses `target`'s bytes (sheaf-ops' `Views` gives a page
    /// viewed over its source's bytes its group's, a file of its own).
    pub fallback: Option<Fallback<K>>,
}

/// Who pins engines: a pane, the thumbnail strip. Each owner's pins
/// replace its last.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Owner(u64);

/// The pool's worker, and the engines it holds. Cheap to clone; the worker
/// ends when the last clone is dropped.
pub struct EnginePool<K> {
    tx: mpsc::Sender<Msg<K>>,
}

impl<K> Clone for EnginePool<K> {
    fn clone(&self) -> Self {
        EnginePool {
            tx: self.tx.clone(),
        }
    }
}

/// Why an engine didn't open, as its requests are told: its first
/// failure, kept.
#[derive(Debug)]
struct Unopened(Arc<anyhow::Error>);

impl std::fmt::Display for Unopened {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the engine didn't open")
    }
}

impl std::error::Error for Unopened {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&**self.0)
    }
}

/// One failure told to each of the requests a render answered: its
/// message and what it came of.
#[derive(Debug)]
struct Shared(Arc<anyhow::Error>);

impl std::fmt::Display for Shared {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for Shared {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.0.source()
    }
}

/// `f`'s result, a panic in it an error saying `what` panicked: one
/// malformed file or query fails its own request, and the worker goes on.
fn guarded<T>(what: &str, f: impl FnOnce() -> Result<T>) -> Result<T> {
    std::panic::catch_unwind(AssertUnwindSafe(f)).unwrap_or_else(|panic| {
        let why = (panic.downcast_ref::<&str>().copied())
            .or_else(|| panic.downcast_ref::<String>().map(String::as_str))
            .unwrap_or("no message");
        Err(anyhow!("{what} panicked: {why}"))
    })
}

/// Answers each of `replies` with `done`: the image to each, or the
/// failure, shared when there's more than one.
fn answer(mut replies: Vec<Reply>, done: Result<Rendered>) {
    match done {
        Ok(rendered) => {
            let last = replies.pop();
            for reply in replies {
                let _ = reply.send(Ok(rendered.clone()));
            }
            if let Some(last) = last {
                let _ = last.send(Ok(rendered));
            }
        }
        Err(e) if replies.len() == 1 => {
            let _ = replies.remove(0).send(Err(e));
        }
        Err(e) => {
            let why = Arc::new(e);
            for reply in replies {
                let _ = reply.send(Err(Shared(why.clone()).into()));
            }
        }
    }
}

/// Where a render's answer goes.
type Reply = oneshot::Sender<Result<Rendered>>;

/// A query's work, given its engine and the page it loads there (its
/// fallback's, should it have fallen back), or why there's no engine.
type Work = Box<dyn FnOnce(Result<(&PdfiumEngine, Option<PageIndex>)>) + Send>;

/// Where a walk reads the page at a position: its engine, its index there,
/// and where else it is should PDFium refuse that engine's bytes.
type Item<K> = dyn Fn(usize) -> Result<(Target<K>, PageIndex, Option<Fallback<K>>)> + Send;

/// A walk's work on the page at a position, given the engine and the page
/// it reads.
type Step = dyn Fn(&PdfiumEngine, PageIndex, usize) -> Result<()> + Send;

/// Who a walk tells how far it has got: a channel's sender, whatever it
/// carries besides.
trait Listener: Send {
    fn tell(&self, walked: Walked);
    /// Whether the receiver is dropped.
    fn gone(&self) -> bool;
}

impl<T: From<Walked> + Send> Listener for stream::UnboundedSender<T> {
    fn tell(&self, walked: Walked) {
        let _ = self.unbounded_send(walked.into());
    }

    fn gone(&self) -> bool {
        self.is_closed()
    }
}

/// Where a request whose engine didn't open, for `e`, reads its page
/// instead: its fallback's engine and page, the fallback run once, when
/// PDFium refused the engine's bytes; otherwise it fails, for `e`.
fn fall_back<K>(e: anyhow::Error, fallback: Option<Fallback<K>>) -> Result<(Target<K>, PageIndex)> {
    let fallback = fallback.filter(|_| OpenError::is_refused(&e)).ok_or(e)?;
    guarded("a fallback", fallback)
}

enum Msg<K> {
    Render(Req<K>, Reply),
    Ask {
        target: Target<K>,
        /// The page it loads.
        page: Option<PageIndex>,
        /// Where else that page is, should PDFium refuse `target`'s bytes.
        fallback: Option<Fallback<K>>,
        work: Work,
    },
    Walk(Walk<K>),
    Pin(Owner, Vec<K>),
    Look(mpsc::Sender<Look<K>>),
}

/// What the worker holds: each engine, its budget and its weight; how
/// many times it has opened one; and its thread, as Linux numbers it.
struct Look<K> {
    engines: Vec<(K, Budget, usize)>,
    opened: usize,
    task: Option<u32>,
}

impl<K: Clone + Eq + Hash + Send + 'static> EnginePool<K> {
    /// Starts the worker.
    pub fn new() -> Result<Self> {
        let (tx, rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("sheaf-render".into())
            .spawn(move || Worker::new().run(rx))
            .context("spawning the render thread")?;
        Ok(EnginePool { tx })
    }

    /// A new owner of pins.
    pub fn owner(&self) -> Owner {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        Owner(NEXT.fetch_add(1, Ordering::Relaxed))
    }

    /// Pins `keys` for `owner`, in place of what it pinned before: the
    /// engines serving what it shows. No keys unpins.
    pub fn pin(&self, owner: Owner, keys: Vec<K>) {
        self.send(Msg::Pin(owner, keys));
    }

    /// Queues a render. Requests for the same render of the same page
    /// queued together share it. Dropping the receiver before the worker
    /// gets to it skips the render, unless another receiver waits for it.
    pub fn render(&self, req: Req<K>) -> oneshot::Receiver<Result<Rendered>> {
        let (tx, rx) = oneshot::channel();
        self.send(Msg::Render(req, tx));
        rx
    }

    /// `f` run on page `page` of `target`'s engine, on the worker; or, when
    /// PDFium refuses that engine's bytes, on the page `fallback` says is
    /// where else it is, the fallback run once, as a render's is. `f` is
    /// given the page it reads. Queries run in the order they came, before
    /// their batch's renders; one that panics fails alone.
    pub fn query<T: Send + 'static>(
        &self,
        target: Target<K>,
        page: PageIndex,
        fallback: Option<Fallback<K>>,
        f: impl FnOnce(&PdfiumEngine, PageIndex) -> Result<T> + Send + 'static,
    ) -> oneshot::Receiver<Result<T>> {
        // A query that loads a page is given one, its own or its fallback's.
        self.ask(target, Some(page), fallback, move |e, at| {
            f(e, at.unwrap_or(page))
        })
    }

    /// The kind of form the engine's document has. With no fallback: a
    /// document asks it of its file's base engine, and is told so when
    /// PDFium refuses the file.
    pub fn form_type(&self, target: Target<K>) -> oneshot::Receiver<Result<FormType>> {
        self.ask(target, None, None, |e, _| Ok(e.form_type()))
    }

    /// Walks the pages at `count` positions, in their order,
    /// `WALK_CHUNK` a turn between renders: `item`, run on the worker as
    /// the walk reaches a position, says where its page is read (its
    /// engine, its index there, and where else it is should PDFium refuse
    /// that engine's bytes, as a query's fallback does), and `work` reads
    /// it there, sending what it finds where it likes. A page whose item
    /// or work fails or panics, or whose engine doesn't open, is passed
    /// over, logged. The engine it read its last page on stays open while
    /// it goes on. `tx` is told how far the walk has got after each turn,
    /// and when it's done; dropping its receiver ends the walk at its next
    /// turn.
    pub fn walk<T: From<Walked> + Send + 'static>(
        &self,
        count: usize,
        item: impl Fn(usize) -> Result<(Target<K>, PageIndex, Option<Fallback<K>>)> + Send + 'static,
        work: impl Fn(&PdfiumEngine, PageIndex, usize) -> Result<()> + Send + 'static,
        tx: stream::UnboundedSender<T>,
    ) {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        self.send(Msg::Walk(Walk {
            id: NEXT.fetch_add(1, Ordering::Relaxed),
            count,
            next: 0,
            item: Box::new(item),
            work: Box::new(work),
            tx: Box::new(tx),
        }));
    }

    /// How many times the worker has opened an engine, one opened again
    /// counted again.
    pub fn opened(&self) -> usize {
        self.look().opened
    }

    /// The engines open.
    pub fn open_engines(&self) -> Vec<K> {
        self.look().engines.into_iter().map(|(k, _, _)| k).collect()
    }

    /// What the engines open in `budget` weigh, by the pool's estimate.
    pub fn weighed(&self, budget: Budget) -> usize {
        (self.look().engines.into_iter())
            .filter(|&(_, b, _)| b == budget)
            .map(|(_, _, bytes)| bytes)
            .sum()
    }

    /// The worker's thread as Linux numbers it, `/proc/self/task/<n>`;
    /// None elsewhere.
    pub fn worker_task(&self) -> Option<u32> {
        self.look().task
    }

    fn look(&self) -> Look<K> {
        let (tx, rx) = mpsc::channel();
        self.send(Msg::Look(tx));
        rx.recv().unwrap_or(Look {
            engines: Vec::new(),
            opened: 0,
            task: None,
        })
    }

    fn ask<T: Send + 'static>(
        &self,
        target: Target<K>,
        page: Option<PageIndex>,
        fallback: Option<Fallback<K>>,
        f: impl FnOnce(&PdfiumEngine, Option<PageIndex>) -> Result<T> + Send + 'static,
    ) -> oneshot::Receiver<Result<T>> {
        let (tx, rx) = oneshot::channel();
        let work: Work = Box::new(move |asked| {
            let _ = tx.send(asked.and_then(|(e, page)| guarded("a query", || f(e, page))));
        });
        self.send(Msg::Ask {
            target,
            page,
            fallback,
            work,
        });
        rx
    }

    fn send(&self, msg: Msg<K>) {
        if self.tx.send(msg).is_err() {
            log::error!("render thread is gone");
        }
    }
}

/// An open engine.
struct Slot {
    engine: PdfiumEngine,
    alive: Arc<Alive>,
    base: bool,
    budget: Budget,
    /// The worker's turn it was last used by its budget.
    used: u64,
    /// The pages it has loaded.
    pages: HashSet<u32>,
    /// The bytes of the segments only it holds.
    held: usize,
}

impl Slot {
    /// What it weighs, by estimate.
    fn weight(&self) -> usize {
        OPEN_BYTES + self.held + self.pages.len() * PAGE_BYTES
    }
}

/// A walk over a document's pages in progress, resumed between renders so
/// it never delays a visible page.
struct Walk<K> {
    /// Which walk it is, for what it holds.
    id: u64,
    count: usize,
    /// The position it reads next.
    next: usize,
    item: Box<Item<K>>,
    work: Box<Step>,
    tx: Box<dyn Listener>,
}

struct Worker<K> {
    engines: HashMap<K, Slot>,
    /// The engines that didn't open, why, and whether their source lives.
    failed: HashMap<K, (Arc<anyhow::Error>, Arc<Alive>)>,
    pins: HashMap<Owner, Vec<K>>,
    pinned: HashSet<K>,
    walks: Vec<Walk<K>>,
    /// The engine each walk read its last page on, and its budget, by the
    /// walk's id: closed by no count until the walk moves on.
    held: HashMap<u64, (K, Budget)>,
    /// Counts uses, for the least recently used.
    tick: u64,
    /// Engines opened, one opened again counted again.
    opened: usize,
    task: Option<u32>,
}

impl<K: Clone + Eq + Hash> Worker<K> {
    fn new() -> Worker<K> {
        Worker {
            engines: HashMap::new(),
            failed: HashMap::new(),
            pins: HashMap::new(),
            pinned: HashSet::new(),
            walks: Vec::new(),
            held: HashMap::new(),
            tick: 0,
            opened: 0,
            task: task(),
        }
    }

    fn run(mut self, rx: mpsc::Receiver<Msg<K>>) {
        loop {
            // Block only when there's no walk to resume.
            let first = match self.walks.is_empty() {
                true => match rx.recv() {
                    Ok(m) => Some(m),
                    Err(_) => return,
                },
                false => match rx.try_recv() {
                    Ok(m) => Some(m),
                    Err(mpsc::TryRecvError::Empty) => None,
                    Err(mpsc::TryRecvError::Disconnected) => return,
                },
            };
            // Drain whatever else is queued so requests for the same render
            // (two views of one file, rapid flipping) render once.
            let mut batch: Vec<Msg<K>> = first.into_iter().collect();
            batch.extend(rx.try_iter());
            self.sweep();
            let mut renders: Vec<(Req<K>, Vec<Reply>)> = Vec::new();
            let mut looks = Vec::new();
            for msg in batch {
                match msg {
                    Msg::Render(req, reply) => {
                        let same = |r: &Req<K>| {
                            (r.target.engine == req.target.engine && r.page == req.page)
                                && (r.job == req.job && r.draw == req.draw)
                                && r.format == req.format
                        };
                        match renders.iter_mut().find(|(r, _)| same(r)) {
                            // One render answers both, as soon as either
                            // asks, by the newest's opener.
                            Some((r, replies)) => {
                                let priority = r.priority.min(req.priority);
                                *r = Req { priority, ..req };
                                replies.push(reply);
                            }
                            None => renders.push((req, vec![reply])),
                        }
                    }
                    Msg::Ask {
                        target,
                        page,
                        fallback,
                        work,
                    } => self.ask(target, page, fallback, work),
                    Msg::Walk(walk) => self.walks.push(walk),
                    Msg::Pin(owner, keys) => self.pin(owner, keys),
                    Msg::Look(reply) => looks.push(reply),
                }
            }
            renders.sort_by_key(|(r, _)| r.priority);
            for (req, mut replies) in renders {
                // The viewer drops receivers for pages that scrolled out of
                // view; don't waste the worker on them.
                replies.retain(|reply| !reply.is_canceled());
                if replies.is_empty() {
                    continue;
                }
                answer(replies, guarded("a render", || self.render(req)));
            }
            self.walk();
            for reply in looks {
                let _ = reply.send(self.look());
            }
        }
    }

    /// `target`'s engine, opened if it isn't, and noted as used by its
    /// budget; or why it didn't open, the first time it was asked for.
    fn engine(&mut self, target: &Target<K>) -> Result<&mut Slot> {
        self.tick += 1;
        if let Some((why, _)) = self.failed.get(&target.engine) {
            return Err(target.open.unopened(why.clone()));
        }
        if !self.engines.contains_key(&target.engine) {
            let slot = match self.open(target) {
                Ok(slot) => slot,
                Err(e) => {
                    let why = Arc::new(e);
                    let alive = target.open.alive.clone();
                    self.failed
                        .insert(target.engine.clone(), (why.clone(), alive));
                    return Err(target.open.unopened(why));
                }
            };
            self.engines.insert(target.engine.clone(), slot);
            self.opened += 1;
        }
        let pinned = self.pinned.contains(&target.engine);
        let slot = (self.engines.get_mut(&target.engine)).expect("an engine just opened");
        if target.budget == Budget::Screen || pinned {
            slot.budget = Budget::Screen;
        }
        // A background request on one of the screen's engines leaves the
        // screen's order as it was.
        if slot.budget == target.budget {
            slot.used = self.tick;
        }
        // The engine lives as long as its newest user's source.
        slot.alive = target.open.alive.clone();
        Ok(slot)
    }

    /// `target`'s engine opened.
    fn open(&self, target: &Target<K>) -> Result<Slot> {
        let (segments, password) = guarded("opening the engine", || (target.open.open)())?;
        let held = (segments.iter())
            .filter(|s| Arc::strong_count(s) == 1)
            .map(|s| s.len())
            .sum();
        let engine = guarded("opening the engine", || {
            PdfiumEngine::open_segments(segments, password.as_deref())
        })?;
        Ok(Slot {
            engine,
            alive: target.open.alive.clone(),
            base: target.open.base,
            budget: target.budget,
            used: self.tick,
            pages: HashSet::new(),
            held,
        })
    }

    /// Runs a query's `work` on `target`'s engine, which loads `page`; or
    /// on its fallback's engine and page, when PDFium refuses `target`'s
    /// bytes.
    fn ask(
        &mut self,
        mut target: Target<K>,
        mut page: Option<PageIndex>,
        fallback: Option<Fallback<K>>,
        work: Work,
    ) {
        let slot = match self.engine(&target) {
            Ok(slot) => Ok(slot),
            Err(e) => match fall_back(e, fallback) {
                Ok((instead, at)) => {
                    (target, page) = (instead, Some(at));
                    self.engine(&target)
                }
                Err(e) => Err(e),
            },
        };
        match slot {
            Ok(slot) => {
                work(Ok((&slot.engine, page)));
                slot.pages.extend(page.map(|p| p.0));
                self.settle(target.budget);
            }
            Err(e) => work(Err(e)),
        }
    }

    fn render(&mut self, mut req: Req<K>) -> Result<Rendered> {
        let slot = match self.engine(&req.target) {
            Ok(slot) => slot,
            Err(e) => {
                (req.target, req.page) = fall_back(e, req.fallback.take())?;
                self.engine(&req.target)?
            }
        };
        let t = Instant::now();
        let raster = slot
            .engine
            .render_with(req.page, req.job, req.format, req.draw);
        let render_time = t.elapsed();
        slot.pages.insert(req.page.0);
        self.settle(req.target.budget);
        Ok(Rendered {
            request: RenderRequest {
                page: req.page,
                job: req.job,
            },
            raster: raster?,
            render_time,
        })
    }

    /// Closes what's past `budget`'s caps after a request of its, the
    /// least recently used engine neither pinned nor held by a walk first,
    /// and then a base engine past its bound (the module's rules).
    fn settle(&mut self, budget: Budget) {
        match budget {
            Budget::Screen => {
                while self.count(budget) > SCREEN_ENGINES && self.close_lru(budget, false) {}
                while self.weight(budget, true) > SCREEN_BYTES && self.close_lru(budget, true) {}
            }
            Budget::Background => {
                while self.count(budget) > BACKGROUND_ENGINES && self.close_lru(budget, true) {}
                while self.weight(budget, false) > BACKGROUND_BYTES && self.close_lru(budget, false)
                {
                }
                // Its base engines, in what the screen's leave.
                let left = SCREEN_BYTES.saturating_sub(self.weight(Budget::Screen, true));
                while self.weight(budget, true) - self.weight(budget, false) > left
                    && self.close_heaviest_base(budget)
                {}
            }
        }
        while self.weight(Budget::Screen, true) > SCREEN_BYTES
            && self.close_heaviest_base(Budget::Screen)
        {}
    }

    /// The engines `budget` counts: the screen's group and patch engines,
    /// or every one of the background's.
    fn count(&self, budget: Budget) -> usize {
        (self.engines.values())
            .filter(|s| s.budget == budget && !(budget == Budget::Screen && s.base))
            .count()
    }

    /// What `budget`'s engines weigh, its base engines only with `bases`.
    fn weight(&self, budget: Budget, bases: bool) -> usize {
        (self.engines.values())
            .filter(|s| s.budget == budget && (bases || !s.base))
            .map(Slot::weight)
            .sum()
    }

    /// Closes `budget`'s least recently used engine neither pinned nor
    /// held by a walk, a base engine only when `bases`: whether there was
    /// one.
    fn close_lru(&mut self, budget: Budget, bases: bool) -> bool {
        let held = |k: &K| self.held.values().any(|(h, _)| h == k);
        let lru = (self.engines.iter())
            .filter(|(k, s)| s.budget == budget && !self.pinned.contains(*k) && !held(k))
            .filter(|(_, s)| bases || !s.base)
            .min_by_key(|(_, s)| s.used)
            .map(|(k, _)| k.clone());
        lru.is_some_and(|k| self.engines.remove(&k).is_some())
    }

    /// Closes `budget`'s heaviest base engine heavier than
    /// [`ENGINE_BYTES`], pinned or held or not: whether there was one.
    fn close_heaviest_base(&mut self, budget: Budget) -> bool {
        let heaviest = (self.engines.iter())
            .filter(|(_, s)| s.base && s.budget == budget)
            .filter(|(_, s)| s.weight() > ENGINE_BYTES)
            .max_by_key(|(_, s)| s.weight())
            .map(|(k, _)| k.clone());
        heaviest.is_some_and(|k| self.engines.remove(&k).is_some())
    }

    fn pin(&mut self, owner: Owner, keys: Vec<K>) {
        match keys.is_empty() {
            true => self.pins.remove(&owner),
            false => self.pins.insert(owner, keys),
        };
        self.pinned = self.pins.values().flatten().cloned().collect();
        for (k, slot) in &mut self.engines {
            if self.pinned.contains(k) {
                slot.budget = Budget::Screen;
            }
        }
        self.settle(Budget::Screen);
    }

    /// Closes the engines whose source is gone, and forgets those that
    /// didn't open.
    fn sweep(&mut self) {
        self.engines.retain(|_, s| (s.alive)());
        self.failed.retain(|_, (_, alive)| alive());
    }

    /// Takes each walk whose receiver is still there a turn further: its
    /// next `WALK_CHUNK` pages, each read on its own engine. A walk that
    /// ends lets go of the engine it held.
    fn walk(&mut self) {
        for mut w in std::mem::take(&mut self.walks) {
            if !w.tx.gone() {
                let to = (w.next + WALK_CHUNK).min(w.count);
                for at in w.next..to {
                    if let Err(e) = self.read(&w, at) {
                        log::warn!("a walk's page {at}: {e:#}");
                    }
                }
                w.next = to;
                w.tx.tell(Walked::Scanned(to));
                if to < w.count {
                    self.walks.push(w);
                    continue;
                }
                w.tx.tell(Walked::Done);
            }
            if let Some((_, budget)) = self.held.remove(&w.id) {
                self.settle(budget);
            }
        }
    }

    /// Runs `w`'s work on the page at position `at`, on the engine its item
    /// names; or on its fallback's engine and page, when PDFium refuses
    /// that engine's bytes.
    fn read(&mut self, w: &Walk<K>, at: usize) -> Result<()> {
        let (mut target, mut page, fallback) = guarded("a walk's page", || (w.item)(at))?;
        let slot = match self.engine(&target) {
            Ok(slot) => slot,
            Err(e) => {
                (target, page) = fall_back(e, fallback)?;
                self.engine(&target)?
            }
        };
        let read = guarded("a walk's page", || (w.work)(&slot.engine, page, at));
        slot.pages.insert(page.0);
        self.held.insert(w.id, (target.engine, target.budget));
        self.settle(target.budget);
        read
    }

    fn look(&self) -> Look<K> {
        Look {
            engines: (self.engines.iter())
                .map(|(k, s)| (k.clone(), s.budget, s.weight()))
                .collect(),
            opened: self.opened,
            task: self.task,
        }
    }
}

/// The calling thread as Linux numbers it.
#[cfg(target_os = "linux")]
fn task() -> Option<u32> {
    let link = std::fs::read_link("/proc/thread-self").ok()?;
    link.file_name()?.to_str()?.parse().ok()
}

#[cfg(not(target_os = "linux"))]
fn task() -> Option<u32> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A read that crosses a boundary is filled from both sides of it, and
    /// any count of segments, empty ones too, reads as one file.
    #[test]
    fn segments_fill_a_read_across_the_boundary() {
        let parts: Vec<Arc<[u8]>> = [&b"abc"[..], b"", b"defg", b"h"].map(Arc::from).to_vec();
        let mut s = Segments::new(parts);
        assert_eq!(s.seek(SeekFrom::End(0)).unwrap(), 8);
        s.seek(SeekFrom::Start(1)).unwrap();
        let mut buf = [0; 6];
        assert_eq!(s.read(&mut buf).unwrap(), 6);
        assert_eq!(&buf, b"bcdefg");
        let mut rest = [0; 4];
        assert_eq!(s.read(&mut rest).unwrap(), 1);
        assert_eq!(rest[0], b'h');
        assert_eq!(s.read(&mut rest).unwrap(), 0);
        s.seek(SeekFrom::Current(-6)).unwrap();
        let mut all = Vec::new();
        s.read_to_end(&mut all).unwrap();
        assert_eq!(all, b"cdefgh");
        assert!(s.seek(SeekFrom::Current(-9)).is_err());
        let mut whole = [0; 8];
        s.seek(SeekFrom::Start(0)).unwrap();
        assert_eq!(s.read(&mut whole).unwrap(), 8);
        assert_eq!(&whole, b"abcdefgh");
        assert_eq!(Segments::new(Vec::new()).read(&mut whole).unwrap(), 0);
    }
}
