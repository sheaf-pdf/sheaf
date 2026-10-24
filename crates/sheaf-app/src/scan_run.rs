//! Scanning pages into one document (M5 proposals 1 and 2). The intake
//! (`sheaf_ops::intake`) runs as a job on a background thread: a page at a
//! time, each cleaned up, recognized when that was asked, and made a page,
//! and every batch of them lands at once, so they show as they come and
//! the user can keep working meanwhile. A batch is made one source in the
//! document's store and planned on that thread, on the version the view
//! hands it, then committed on the UI thread with that version as its
//! base, lowered again on the current one if the user edited since
//! (`Stale::Relower`): its pages go at the end, which names no position,
//! so no edit of the user's conflicts with it. The batches are the scan's
//! group, one undo step (or a fragment each side of the user's edits).
//! Each batch that lands starts the recovery sidecar's write at once, or
//! as soon as the write in flight ends, so a crash loses at most the
//! pages landed since the last write finished: an untitled document's is
//! offered back at the next launch.
//! The status bar shows the page being scanned, and Escape stops the scan,
//! keeping the pages it brought in; undoing it stops it at its next batch,
//! and so does closing the view, which answers nothing more. Saving to
//! close stops it and waits for it to end ([`DocView::stop_scan`]).

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::time::Instant;

use anyhow::anyhow;
use futures::StreamExt;
use futures::channel::mpsc::{UnboundedSender, unbounded};
use futures::channel::oneshot;
use gpui::{Context, Task};
use sheaf_doc::tx::LowerCx;
use sheaf_doc::{
    Actor, Base, CommitOpts, Document, EditError, GroupId, GroupKind, Identity, JobKind, Snapshot,
    Stale, Store, Tx, VersionId,
};
use sheaf_ocr::{OcrConfig, OcrEngine};
use sheaf_ops::intake::{self, Batch, Batching, Files, Landing, Pages};
use sheaf_ops::scan::{ScanOptions, ScanReport};
use sheaf_ops::{NoProgress, Progress, ProgressSnapshot, Tracker};

use crate::docview::{DocView, State};
use crate::untitled::Untitled;

/// What a scan says once the user undid what it brought in, which stops
/// it.
const UNDONE: &str = "Scan stopped: its pages were undone";

/// A scan streaming pages into the view's document.
pub(crate) struct ScanRun {
    /// The intake's progress, which Escape cancels.
    tracker: Tracker,
    /// The group its batches are committed in.
    group: GroupId,
    /// The task answering the intake. Dropped with the view, it leaves
    /// what the intake asks unanswered, which stops it.
    _answers: Task<()>,
    /// Who waits for it to end: told as it's dropped.
    ended: Vec<oneshot::Sender<()>>,
}

/// What the intake asks of the view, which answers on the UI thread.
enum Request {
    /// The version to plan a batch on, with the scan job's lowering on it.
    Base(mpsc::Sender<(Snapshot, Arc<LowerCx>)>),
    /// A batch planned on version `on`, to commit: whether it landed.
    Commit {
        tx: Box<Tx>,
        on: VersionId,
        landed: mpsc::Sender<anyhow::Result<()>>,
    },
}

/// The view's landing, on the intake's thread: each batch made one source
/// in the document's store and planned on the version the view gives, for
/// the view to commit. What the view doesn't answer (it closed) stops the
/// intake.
struct ViewLanding {
    store: Arc<Store>,
    view: UnboundedSender<Request>,
}

impl ViewLanding {
    /// The view's answer to the request `ask` makes of the sender to answer
    /// on.
    fn ask<T>(&self, ask: impl FnOnce(mpsc::Sender<T>) -> Request) -> anyhow::Result<T> {
        let (answer, answered) = mpsc::channel();
        let closed = || anyhow!("the document was closed");
        let sent = self.view.unbounded_send(ask(answer));
        sent.map_err(|_| closed())?;
        answered.recv().map_err(|_| closed())
    }
}

impl Landing for ViewLanding {
    fn land(&mut self, batch: Batch, _: &dyn Progress) -> anyhow::Result<()> {
        let src = intake::batch_source(&self.store, &batch.pages, batch.fit)?;
        let (snap, lower) = self.ask(Request::Base)?;
        let mut tx = Document::tx_at(&snap, lower);
        tx.pin(src.blob().clone());
        // Not cut short by a cancel, which stops the intake once this
        // batch has landed: the pages scanned are kept.
        intake::apply_batch(&mut tx, &src, batch.words, &batch.engine, &NoProgress)?;
        let (tx, on) = (Box::new(tx), snap.version);
        self.ask(|landed| Request::Commit { tx, on, landed })?
    }
}

/// The image files `files`' pages, read as the intake asks for them.
fn reading(
    files: Vec<PathBuf>,
) -> impl FnOnce() -> anyhow::Result<Box<dyn Pages + Send>> + Send + 'static {
    move || Ok(Box::new(Files::new(&files)?))
}

impl DocView {
    /// A view of `u`, a new untitled document, that `files`' pages come
    /// into as they're cleaned up as `opts` says (Create from Scans, M5
    /// proposal 2), recognized with `ocr` when `opts` asks, landing as
    /// `batching` says; its marks are made by the user `who`, measured at
    /// `scale`.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn scan_untitled(
        u: Untitled,
        files: Vec<PathBuf>,
        opts: ScanOptions,
        ocr: OcrConfig,
        batching: Batching,
        who: Identity,
        scale: sheaf_doc::measure::Scale,
        timing: bool,
        launched: Instant,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut view = Self::untitled(u, who, scale, timing, launched, cx);
        view.start_scan(reading(files), opts, ocr, batching, cx);
        view
    }

    /// Scan the pages `open` gives, on a background thread, into the
    /// document: cleaned up as `opts` says, recognized with `ocr` when it
    /// asks, and inserted at the end as `batching` lands them, in a group
    /// of the scan job's. The status bar counts the pages; it ends saying
    /// what it brought in, or why it stopped, and the group ends with it,
    /// whatever stopped it. Refused while another scan runs.
    pub(crate) fn start_scan(
        &mut self,
        open: impl FnOnce() -> anyhow::Result<Box<dyn Pages + Send>> + Send + 'static,
        opts: ScanOptions,
        ocr: OcrConfig,
        batching: Batching,
        cx: &mut Context<Self>,
    ) {
        if self.scan.is_some() {
            self.set_notice("A scan is already running — Esc stops it", cx);
            return;
        }
        let State::Ready(doc) = &mut self.state else {
            return;
        };
        let group = doc.model.begin_group(GroupKind::Scan);
        let store = doc.model.store().clone();
        let (view, mut asked) = unbounded();
        let answers = cx.spawn(async move |this, cx| {
            while let Some(request) = asked.next().await {
                if this.update(cx, |v, cx| v.answer(request, cx)).is_err() {
                    break;
                }
            }
        });
        let tracker = Tracker::new();
        self.scan = Some(ScanRun {
            tracker: tracker.clone(),
            group,
            _answers: answers,
            ended: Vec::new(),
        });
        self.set_notice("Scanning…", cx);
        // How many pages there are, once the stream is open and knows.
        let count = Arc::new(AtomicU64::new(0));
        let work = {
            let count = count.clone();
            move |p: &Tracker| {
                let mut pages = open()?;
                let n = pages.count().unwrap_or(0) as u64;
                count.store(n, Ordering::Relaxed);
                let mut engine = sheaf_ops::scan::engine(&opts, &ocr)?;
                let engine = engine.as_deref_mut().map(|e| e as &mut dyn OcrEngine);
                let mut landing = ViewLanding { store, view };
                intake::intake(&mut *pages, &opts, engine, &mut landing, batching, p)
            }
        };
        let status = move |s: &ProgressSnapshot| {
            let n = count.load(Ordering::Relaxed);
            // The page being scanned: the share done says how many are.
            let at = (s.fraction.unwrap_or(0.0) as f64 * n as f64) as u64 + 1;
            (n > 0).then(|| format!("Scanning page {} of {n} — Esc stops", at.min(n)))
        };
        let end = move |v: &mut DocView, result, _: &mut Context<DocView>| {
            if let State::Ready(doc) = &mut v.state {
                doc.model.end_group(group);
            }
            v.scan = None;
            Some(scanned(result))
        };
        self.drive(tracker, work, status, end, cx);
    }

    /// The intake's `request`, answered: the version to plan a batch on,
    /// or a batch committed ([`Self::land_batch`]). With no document
    /// shown, it goes unanswered, which stops the intake.
    fn answer(&mut self, request: Request, cx: &mut Context<Self>) {
        match request {
            Request::Base(answer) => {
                if let State::Ready(doc) = &self.state {
                    let lower = doc.model.lower_cx(Actor::Job(JobKind::Scan));
                    answer.send((doc.model.snapshot(), lower)).ok();
                }
            }
            Request::Commit { tx, on, landed } => {
                landed.send(self.land_batch(*tx, on, cx)).ok();
            }
        }
    }

    /// A batch planned on version `on` committed in the scan's group, its
    /// ops lowered again on the current version if the user edited since;
    /// then everything that follows the document catches up, and the
    /// recovery sidecar's write starts at once, or as soon as the one in
    /// flight ends ([`DocView::write_recovery_at_once`]), not once the
    /// batches stop coming, so a crash loses at most the batches landed
    /// since the last write finished. It's made on the
    /// model rather than through [`DocView::commit`], whose notice would
    /// replace the scan's. Once the user undid the scan, it's
    /// [`EditError::GroupEnded`], which stops the scan.
    fn land_batch(&mut self, tx: Tx, on: VersionId, cx: &mut Context<Self>) -> anyhow::Result<()> {
        let (Some(run), State::Ready(doc)) = (&self.scan, &mut self.state) else {
            return Err(anyhow!("the scan is over"));
        };
        let opts = CommitOpts {
            base: Base::At(on),
            stale: Stale::Relower,
            group: Some(run.group),
            ..CommitOpts::labelled("Scan")
        };
        let commit = doc.model.commit(tx, opts)?;
        self.on_change(&commit.change, cx);
        self.write_recovery_at_once(cx);
        Ok(())
    }

    /// Stop the scan, keeping the pages it brought in. Returns whether
    /// there was one.
    pub(crate) fn cancel_scan(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(run) = &self.scan else {
            return false;
        };
        run.tracker.cancel();
        self.set_notice("Scan: stopping…", cx);
        true
    }

    /// Stop the scan, if one runs, keeping the pages it brought in:
    /// resolves once it has ended, its last batch landed (at once with
    /// none).
    pub(crate) fn stop_scan(&mut self, cx: &mut Context<Self>) -> impl Future<Output = ()> + use<> {
        let ended = (self.scan.as_mut()).map(|run| {
            let (tell, told) = oneshot::channel();
            run.ended.push(tell);
            told
        });
        self.cancel_scan(cx);
        async move {
            if let Some(told) = ended {
                told.await.ok();
            }
        }
    }

    /// Fraction of the scan done, while one runs.
    pub(crate) fn scan_progress(&self) -> Option<f32> {
        self.scan.as_ref()?.tracker.snapshot().fraction
    }
}

/// What a scan that ended with `result` says.
fn scanned(result: anyhow::Result<ScanReport>) -> String {
    match result {
        Ok(report) if report.pages == 0 => "Every page came out blank".into(),
        Ok(report) => {
            let s = if report.pages == 1 { "" } else { "s" };
            let blank = match report.dropped {
                0 => String::new(),
                k => format!(" ({k} left out as blank)"),
            };
            format!("Scanned {} page{s}{blank} — Ctrl+Z undoes", report.pages)
        }
        Err(e) if matches!(e.downcast_ref(), Some(EditError::GroupEnded)) => UNDONE.into(),
        Err(e) => crate::jobs::ended("Scan", &e),
    }
}

// Headless app tests, which run on Linux only.
#[cfg(all(test, target_os = "linux"))]
mod tests {
    use std::path::Path;
    use std::sync::atomic::{AtomicBool, AtomicUsize};
    use std::time::Duration;

    use gpui::{App, AppContext as _, AsyncApp, Entity};
    use image::{Rgb, RgbImage};
    use sheaf_doc::materialize::AskAnswers;
    use sheaf_doc::recovery;
    use sheaf_edit::{Op, PageSel, Turn};
    use sheaf_ops::images::Paper;
    use sheaf_ops::intake::{RawPage, SessionLanding};
    use sheaf_ops::session::{SaveTo, Session};

    use super::*;
    use crate::docview::test_support::{
        ALONE, alone, shown, sidecar_of, sidecar_unwanted, sidecar_writing,
    };

    /// A page at a time, never by the clock.
    const ONE_BY_ONE: Batching = Batching {
        pages: 1,
        wait: None,
    };

    /// Eight pages at a time, never by the clock.
    const EIGHT: Batching = Batching {
        pages: 8,
        wait: None,
    };

    /// `n` pages of "text" under `dir`'s `images/`, a PNG each, no two
    /// alike: dark bars on white, their lengths from the page's number.
    fn images(dir: &Path, n: u64) -> Vec<PathBuf> {
        let folder = dir.join("images");
        std::fs::create_dir_all(&folder).unwrap();
        (0..n)
            .map(|k| {
                let mut rng = fastrand::Rng::with_seed(k);
                let mut img = RgbImage::from_pixel(170, 220, Rgb([250, 250, 250]));
                for line in 0..8 {
                    let (top, end) = (20 + line * 24, rng.u32(60..150));
                    for y in top..top + 6 {
                        for x in 15..end {
                            img.put_pixel(x, y, Rgb([30, 30, 30]));
                        }
                    }
                }
                let path = folder.join(format!("page {k:02}.png"));
                img.save(&path).unwrap();
                path
            })
            .collect()
    }

    /// What's in `dir`, by name.
    fn listed(dir: &Path) -> Vec<PathBuf> {
        let mut files: Vec<PathBuf> = (std::fs::read_dir(dir).unwrap())
            .map(|e| e.unwrap().path())
            .collect();
        files.sort();
        files
    }

    /// What's in the recovery folder `dir` but the files its names' claims
    /// are held by.
    fn kept(dir: &Path) -> Vec<PathBuf> {
        let claims = |p: &PathBuf| {
            let name = p.file_name().unwrap().to_string_lossy();
            name == ".lock" || name.ends_with(".sheaf-lock")
        };
        listed(dir).into_iter().filter(|p| !claims(p)).collect()
    }

    /// Each image a page as it is, at 100 dpi, in color: none of cleanup's
    /// steps, and no OCR.
    fn opts() -> ScanOptions {
        let mut opts = ScanOptions {
            page: Paper::Original,
            dpi: 100,
            ..ScanOptions::default()
        };
        let clean = &mut opts.clean;
        (clean.crop, clean.flatten, clean.deskew, clean.orient) = (false, false, false, false);
        opts
    }

    /// `files`' pages, the first `free` of them at once and the rest once
    /// the gate opens, counting how many it gave.
    struct Gated {
        files: Files,
        free: usize,
        gate: Option<mpsc::Receiver<()>>,
        gave: Arc<AtomicUsize>,
    }

    impl Gated {
        /// `files`, gated after `free`, and the gate: dropping it opens.
        fn new(files: &[PathBuf], free: usize) -> (Gated, mpsc::Sender<()>) {
            let (gate, waiting) = mpsc::channel();
            let gated = Gated {
                files: Files::new(files).unwrap(),
                free,
                gate: Some(waiting),
                gave: Arc::default(),
            };
            (gated, gate)
        }

        /// What a scan reads its pages from.
        fn opening(self) -> impl FnOnce() -> anyhow::Result<Box<dyn Pages + Send>> + Send {
            move || Ok(Box::new(self) as Box<dyn Pages + Send>)
        }
    }

    impl Pages for Gated {
        fn next(&mut self, p: &dyn Progress) -> anyhow::Result<Option<RawPage>> {
            if self.gave.load(Ordering::Relaxed) == self.free
                && let Some(gate) = self.gate.take()
            {
                // Until the gate is dropped.
                gate.recv().ok();
            }
            let page = self.files.next(p)?;
            (self.gave).fetch_add(usize::from(page.is_some()), Ordering::Relaxed);
            Ok(page)
        }

        fn count(&self) -> Option<usize> {
            self.files.count()
        }
    }

    /// The untitled document `name`, its sidecar in `dir`'s `recovery/`,
    /// claimed: what a test in the test binary's own process makes, which
    /// writes nothing to the user's state folder, and doesn't depend on
    /// it.
    fn untitled_in(dir: &Path, name: &str) -> Untitled {
        let claim = recovery::claim_untitled(&dir.join("recovery"), name).unwrap();
        Untitled {
            name: name.into(),
            dir: dir.join("images"),
            claim: Some(Arc::new(claim.expect("unclaimed"))),
        }
    }

    /// The sidecar `u` claims.
    fn sidecar(u: &Untitled) -> PathBuf {
        u.claim.as_ref().unwrap().sidecar()
    }

    /// A view of `u`, a new untitled document, as the workspace makes one.
    fn untitled(u: Untitled, cx: &mut App) -> Entity<DocView> {
        let (who, at) = (Identity::default(), Instant::now());
        cx.new(|cx| DocView::untitled(u, who, Default::default(), false, at, cx))
    }

    /// How many pages `view`'s document has.
    fn pages(view: &DocView) -> usize {
        match &view.state {
            State::Ready(doc) => doc.model.page_count(),
            _ => 0,
        }
    }

    /// The current version of the document `view` shows.
    fn shown_version(view: &DocView) -> VersionId {
        match &view.state {
            State::Ready(doc) => doc.model.current_version(),
            _ => panic!("ready"),
        }
    }

    /// Whether `view`'s recovery sidecar holds its current version, none
    /// being written.
    fn sidecar_current(view: &DocView) -> bool {
        let State::Ready(doc) = &view.state else {
            return false;
        };
        !sidecar_writing(view) && sidecar_of(view) == Some(doc.model.current_version())
    }

    /// How many pages the untitled sidecar `sidecar` holds, while it's
    /// there: what follows its head's three lines.
    fn sidecar_pages(sidecar: &Path) -> Option<usize> {
        let bytes = std::fs::read(sidecar).ok()?;
        let mut lines = bytes.splitn(4, |&b| b == b'\n');
        let body = lines.nth(3).unwrap();
        let pdf = lopdf::Document::load_mem(body).unwrap();
        Some(pdf.get_pages().len())
    }

    /// The user turns `view`'s first page a quarter.
    fn turn_page_1(view: &mut DocView, cx: &mut Context<DocView>) {
        let pages = PageSel::ids([shown(view).page_id(0).unwrap()]);
        let tx = (view.lower(
            vec![Op::RotatePages {
                pages,
                by: Turn::Quarter,
            }],
            "Turn",
            cx,
        ))
        .unwrap();
        view.commit(tx, CommitOpts::labelled("Turn"), cx).unwrap();
    }

    /// Waits until `done` holds, looking every 10 ms, for a minute at most.
    async fn waiting(cx: &mut AsyncApp, done: impl Fn(&mut AsyncApp) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(60);
        while !done(cx) {
            assert!(Instant::now() < deadline, "not done in a minute");
            cx.background_executor()
                .timer(Duration::from_millis(10))
                .await;
        }
    }

    /// Waits until `done` holds of `view`, as [`waiting`] waits.
    async fn until(cx: &mut AsyncApp, view: &Entity<DocView>, done: impl Fn(&DocView) -> bool) {
        waiting(cx, |cx| cx.update(|cx| done(view.read(cx)))).await;
    }

    /// Create from Scans of three images, as the workspace makes it
    /// (`untitled::for_scan`, `DocView::scan_untitled`), streams them into
    /// a new untitled document, "Untitled scan 1", that has no file (nor
    /// does File > Properties say it has): its pages one version of the
    /// scan job's, which one undo takes back and one redo brings again,
    /// and nothing written but its sidecar, the one file in the state
    /// folder's `recovery/`. Save is Save As there, and the view then takes
    /// the file's name and path, its document the file's, clean, and the
    /// sidecar is gone. In a process of its own, whose config folder is
    /// the test's and whose save picker answers with a file in its `out/`,
    /// all in a `sheaf-` temporary directory, which the recent files leave
    /// out.
    #[test]
    fn create_from_scans_streams_into_an_untitled_document() {
        let Some(dir) = std::env::var_os(ALONE).map(PathBuf::from) else {
            let name = "scan_run::tests::create_from_scans_streams_into_an_untitled_document";
            let dir = tempfile::Builder::new().prefix("sheaf-").tempdir().unwrap();
            images(dir.path(), 3);
            let (config, out) = (dir.path().join("config"), dir.path().join("out"));
            std::fs::create_dir(&out).unwrap();
            let to = out.join("scans.pdf");
            let env = [("SHEAF_CONFIG_DIR", &*config), ("SHEAF_SCRIPT_SAVE", &*to)];
            alone(name, dir.path(), &env);
            return;
        };
        let files = listed(&dir.join("images"));
        let (out, to) = (dir.join("out"), dir.join("out").join("scans.pdf"));
        let recovery = dir.join("config").join("state").join("recovery");
        let ran = Arc::new(AtomicBool::new(false));
        gpui_platform::headless().run({
            let ran = ran.clone();
            move |cx| {
                let u = crate::untitled::for_scan(&files);
                let sidecar = sidecar(&u);
                assert_eq!(sidecar, recovery.join("Untitled scan 1.sheaf-recovery"));
                let view = cx.new(|cx| {
                    let (who, at) = (Identity::default(), Instant::now());
                    let (ocr, scale) = (OcrConfig::default(), Default::default());
                    let opts = opts();
                    let files = files.clone();
                    DocView::scan_untitled(
                        u, files, opts, ocr, ONE_BY_ONE, who, scale, false, at, cx,
                    )
                });
                cx.spawn(async move |cx| {
                    until(cx, &view, |v| v.scan.is_none()).await;
                    cx.update(|cx| {
                        view.update(cx, |v, cx| {
                            assert_eq!(&*v.title(), "Untitled scan 1 •");
                            assert_eq!(v.path(), None);
                            // What File > Properties shows of it.
                            let props = v.properties_context().unwrap();
                            assert_eq!((props.path, &*props.name), (None, "Untitled scan 1"));
                            let model = &shown(v).model;
                            assert_eq!(model.page_count(), 3);
                            let timeline = model.timeline();
                            assert_eq!(timeline.len(), 2, "the empty document and the scan");
                            assert_eq!(timeline[1].label, "Scan");
                            assert_eq!(timeline[1].actor, Actor::Job(JobKind::Scan));
                            v.undo(cx);
                            assert_eq!(pages(v), 0);
                            assert!(!v.is_dirty());
                            v.redo(cx);
                            assert_eq!(pages(v), 3);
                        })
                    });
                    assert_eq!(
                        listed(&dir.join("images")),
                        files,
                        "nothing written beside them"
                    );
                    assert!(listed(&out).is_empty(), "nor where it's saved");
                    until(cx, &view, sidecar_current).await;
                    assert_eq!(kept(&recovery), [sidecar], "but its sidecar");
                    let saving = cx.update(|cx| view.update(cx, |v, cx| v.save(cx)));
                    assert!(saving.await, "saved");
                    cx.update(|cx| {
                        view.update(cx, |v, _| {
                            assert_eq!(v.path(), Some(to.as_path()));
                            assert_eq!(&*v.title(), "scans.pdf");
                            let model = &shown(v).model;
                            let file = model.file().map(|(file, _)| file.to_path_buf());
                            assert_eq!(file, Some(to.clone()));
                            assert!(!model.is_dirty());
                        })
                    });
                    let saved = lopdf::Document::load(&to).unwrap();
                    assert_eq!(saved.get_pages().len(), 3);
                    assert!(
                        kept(&recovery).is_empty(),
                        "its sidecar goes once it's saved"
                    );
                    ran.store(true, Ordering::Relaxed);
                    cx.update(|cx| cx.quit());
                })
                .detach();
            }
        });
        assert!(ran.load(Ordering::Relaxed));
    }

    /// Closing asks about an untitled document only once it has pages, as
    /// it asks about a changed one: by [`DocView::is_dirty`], which
    /// closing a window or a document asks (the prompt itself, and its
    /// answers' wiring, are hand checks), naming it by
    /// [`DocView::file_name`]. With no pages it's clean, after a scan of
    /// one image it isn't, and once that's undone it's clean again.
    #[test]
    fn closing_an_untitled_document_asks_only_with_pages() {
        let dir = tempfile::Builder::new().prefix("sheaf-").tempdir().unwrap();
        let files = images(dir.path(), 1);
        let u = untitled_in(dir.path(), "Untitled scan 1");
        let ran = Arc::new(AtomicBool::new(false));
        gpui_platform::headless().run({
            let ran = ran.clone();
            move |cx| {
                let view = untitled(u, cx);
                view.update(cx, |v, cx| {
                    assert!(!v.is_dirty());
                    assert_eq!(v.file_name(), "Untitled scan 1");
                    let (ocr, read) = (OcrConfig::default(), reading(files));
                    v.start_scan(read, opts(), ocr, ONE_BY_ONE, cx);
                });
                cx.spawn(async move |cx| {
                    until(cx, &view, |v| v.scan.is_none() && !sidecar_writing(v)).await;
                    cx.update(|cx| {
                        view.update(cx, |v, cx| {
                            assert_eq!(pages(v), 1);
                            assert!(v.is_dirty());
                            v.undo(cx);
                            assert!(!v.is_dirty());
                        })
                    });
                    ran.store(true, Ordering::Relaxed);
                    cx.update(|cx| cx.quit());
                })
                .detach();
            }
        });
        assert!(ran.load(Ordering::Relaxed));
    }

    /// An untitled document made from no files (its sidecar's head said no
    /// folder, say) starts its Save As in the home folder, not the
    /// process's working one.
    #[test]
    fn an_untitled_document_of_no_folder_saves_from_home() {
        let u = Untitled {
            name: "Untitled scan 1".into(),
            dir: PathBuf::new(),
            claim: None,
        };
        let ran = Arc::new(AtomicBool::new(false));
        gpui_platform::headless().run({
            let ran = ran.clone();
            move |cx| {
                let view = untitled(u, cx);
                cx.spawn(async move |cx| {
                    let folder = cx.update(|cx| view.read(cx).folder());
                    assert_eq!(folder, sheaf_ops::dirs::home().unwrap());
                    ran.store(true, Ordering::Relaxed);
                    cx.update(|cx| cx.quit());
                })
                .detach();
            }
        });
        assert!(ran.load(Ordering::Relaxed));
    }

    /// The close prompt's Save mid-scan ([`DocView::save_to_close`]) stops
    /// the scan and saves once it has ended, every page landed in the file,
    /// so closing after it loses none: twenty images, three given at once
    /// and the rest once the gate opens, which it does once Save was
    /// chosen. The saved file holds every page the view does, the document
    /// is clean, and the scan stopped short of the twenty. In a process of
    /// its own, whose save picker answers with a file in its `out/`.
    #[test]
    fn saving_to_close_mid_scan_keeps_every_page() {
        let Some(dir) = std::env::var_os(ALONE).map(PathBuf::from) else {
            let name = "scan_run::tests::saving_to_close_mid_scan_keeps_every_page";
            let dir = tempfile::Builder::new().prefix("sheaf-").tempdir().unwrap();
            images(dir.path(), 20);
            let (config, to) = (dir.path().join("config"), dir.path().join("scans.pdf"));
            let env = [("SHEAF_CONFIG_DIR", &*config), ("SHEAF_SCRIPT_SAVE", &*to)];
            alone(name, dir.path(), &env);
            return;
        };
        let files = listed(&dir.join("images"));
        let to = dir.join("scans.pdf");
        let (gated, gate) = Gated::new(&files, 3);
        let gave = gated.gave.clone();
        let u = untitled_in(&dir, "Untitled scan 1");
        let ran = Arc::new(AtomicBool::new(false));
        gpui_platform::headless().run({
            let ran = ran.clone();
            move |cx| {
                let view = untitled(u, cx);
                view.update(cx, |v, cx| {
                    let ocr = OcrConfig::default();
                    v.start_scan(gated.opening(), opts(), ocr, ONE_BY_ONE, cx);
                });
                cx.spawn(async move |cx| {
                    until(cx, &view, |v| pages(v) == 3).await;
                    let closing = cx.update(|cx| view.update(cx, |v, cx| v.save_to_close(cx)));
                    drop(gate);
                    assert!(closing.await, "it may close");
                    let landed = cx.update(|cx| {
                        view.update(cx, |v, _| {
                            assert!(v.scan.is_none(), "stopped");
                            assert_eq!(v.path(), Some(to.as_path()));
                            assert!(!v.is_dirty());
                            pages(v)
                        })
                    });
                    let saved = lopdf::Document::load(&to).unwrap();
                    assert_eq!(saved.get_pages().len(), landed, "every page landed");
                    assert!(gave.load(Ordering::Relaxed) < 20, "stopped short");
                    ran.store(true, Ordering::Relaxed);
                    cx.update(|cx| cx.quit());
                })
                .detach();
            }
        });
        assert!(ran.load(Ordering::Relaxed));
    }

    /// The close prompt's Save on an untitled document whose Save As is
    /// cancelled resolves to not closing: the document stays untitled,
    /// unsaved, its sidecar kept. In a process of its own, whose save
    /// picker is cancelled.
    #[test]
    fn a_cancelled_save_to_close_keeps_it_open() {
        let Some(dir) = std::env::var_os(ALONE).map(PathBuf::from) else {
            let name = "scan_run::tests::a_cancelled_save_to_close_keeps_it_open";
            let dir = tempfile::Builder::new().prefix("sheaf-").tempdir().unwrap();
            images(dir.path(), 1);
            let config = dir.path().join("config");
            let env = [
                ("SHEAF_CONFIG_DIR", &*config),
                ("SHEAF_SCRIPT_SAVE", Path::new("")),
            ];
            alone(name, dir.path(), &env);
            return;
        };
        let files = listed(&dir.join("images"));
        let u = untitled_in(&dir, "Untitled scan 1");
        let file = sidecar(&u);
        let ran = Arc::new(AtomicBool::new(false));
        gpui_platform::headless().run({
            let ran = ran.clone();
            move |cx| {
                let view = untitled(u, cx);
                view.update(cx, |v, cx| {
                    let (ocr, read) = (OcrConfig::default(), reading(files));
                    v.start_scan(read, opts(), ocr, ONE_BY_ONE, cx);
                });
                cx.spawn(async move |cx| {
                    until(cx, &view, |v| v.scan.is_none() && sidecar_current(v)).await;
                    let closing = cx.update(|cx| view.update(cx, |v, cx| v.save_to_close(cx)));
                    assert!(!closing.await, "it stays open");
                    cx.update(|cx| {
                        view.update(cx, |v, _| {
                            assert_eq!((v.path(), &*v.title()), (None, "Untitled scan 1 •"));
                            assert!(v.is_dirty());
                        })
                    });
                    assert_eq!(sidecar_pages(&file), Some(1));
                    ran.store(true, Ordering::Relaxed);
                    cx.update(|cx| cx.quit());
                })
                .detach();
            }
        });
        assert!(ran.load(Ordering::Relaxed));
    }

    /// Undoing a scan in the view stops it: twenty images, three given at
    /// once and the rest once the gate opens, landed one by one; once the
    /// three are undone, the fourth's batch is GroupEnded, and the scan
    /// stops, saying so, no page put back and none read after it.
    #[test]
    fn undoing_a_scan_in_the_view_stops_it() {
        let dir = tempfile::Builder::new().prefix("sheaf-").tempdir().unwrap();
        let files = images(dir.path(), 20);
        let (gated, gate) = Gated::new(&files, 3);
        let gave = gated.gave.clone();
        let u = untitled_in(dir.path(), "Untitled scan 1");
        let ran = Arc::new(AtomicBool::new(false));
        gpui_platform::headless().run({
            let ran = ran.clone();
            move |cx| {
                let view = untitled(u, cx);
                view.update(cx, |v, cx| {
                    v.start_scan(
                        gated.opening(),
                        opts(),
                        OcrConfig::default(),
                        ONE_BY_ONE,
                        cx,
                    );
                });
                cx.spawn(async move |cx| {
                    until(cx, &view, |v| pages(v) == 3).await;
                    cx.update(|cx| view.update(cx, |v, cx| v.undo(cx)));
                    drop(gate);
                    until(cx, &view, |v| v.scan.is_none() && !sidecar_writing(v)).await;
                    cx.update(|cx| {
                        view.update(cx, |v, _| {
                            assert_eq!(v.notice(), Some(UNDONE));
                            assert_eq!(pages(v), 0);
                        })
                    });
                    assert_eq!(gave.load(Ordering::Relaxed), 4);
                    ran.store(true, Ordering::Relaxed);
                    cx.update(|cx| cx.quit());
                })
                .detach();
            }
        });
        assert!(ran.load(Ordering::Relaxed));
    }

    /// A batch's sidecar is written as the batch lands, not on autosave's
    /// clock, which batches coming every two seconds would keep putting
    /// off: eight images given at once and the ninth held back, landed
    /// eight at a time. In the same look that first finds the eight pages,
    /// their sidecar is being written, or holds them; then it holds the
    /// eight, and the nine once the gate opens. Pages landing while it's
    /// being written start the next write as that one ends: never, from
    /// the landing to the sidecar holding them, does a look find no write
    /// running with the sidecar behind, as autosave's few seconds would.
    #[test]
    fn a_batch_s_sidecar_is_written_as_it_lands() {
        let dir = tempfile::Builder::new().prefix("sheaf-").tempdir().unwrap();
        let files = images(dir.path(), 9);
        let (gated, gate) = Gated::new(&files, 8);
        let u = untitled_in(dir.path(), "Untitled scan 1");
        let file = sidecar(&u);
        let ran = Arc::new(AtomicBool::new(false));
        gpui_platform::headless().run({
            let ran = ran.clone();
            move |cx| {
                let view = untitled(u, cx);
                view.update(cx, |v, cx| {
                    v.start_scan(gated.opening(), opts(), OcrConfig::default(), EIGHT, cx);
                });
                cx.spawn(async move |cx| {
                    until(cx, &view, |v| {
                        let landed = pages(v) == 8;
                        if landed {
                            let now = shown_version(v);
                            let written = sidecar_writing(v) || sidecar_of(v) == Some(now);
                            assert!(written, "the sidecar is written as the batch lands");
                        }
                        landed
                    })
                    .await;
                    waiting(cx, |_| sidecar_pages(&file) == Some(8)).await;
                    drop(gate);
                    until(cx, &view, |v| v.scan.is_none() && sidecar_current(v)).await;
                    assert_eq!(sidecar_pages(&file), Some(9));
                    // A write in flight, and pages landing meanwhile, as
                    // land_batch lands them.
                    cx.update(|cx| {
                        view.update(cx, |v, cx| {
                            turn_page_1(v, cx);
                            v.write_recovery(cx);
                            assert!(sidecar_writing(v));
                            turn_page_1(v, cx);
                            v.write_recovery_at_once(cx);
                        })
                    });
                    until(cx, &view, |v| {
                        let behind = !sidecar_writing(v) && !sidecar_current(v);
                        assert!(!behind, "the next write starts as the last ends");
                        sidecar_current(v)
                    })
                    .await;
                    ran.store(true, Ordering::Relaxed);
                    cx.update(|cx| cx.quit());
                })
                .detach();
            }
        });
        assert!(ran.load(Ordering::Relaxed));
    }

    /// An untitled document's sidecar follows it: there once a scan of two
    /// images landed, gone once that's undone (autosave's few seconds
    /// later), back once it's redone, and gone once the view closes with it
    /// all undone, a write still in flight then removing what it wrote,
    /// its name claimed until the write is done. Another's goes with its
    /// changes dropped, as Don't Save drops them, and its write in flight
    /// with it, though the view closed meanwhile. A
    /// third's goes once it's saved to a file, and an edit after that
    /// writes the file's own, beside it.
    #[test]
    fn an_untitled_document_s_sidecar_follows_it() {
        let dir = tempfile::Builder::new().prefix("sheaf-").tempdir().unwrap();
        let files = images(dir.path(), 2);
        let recovery = dir.path().join("recovery");
        let [first, second, third] = ["Untitled scan 1", "Untitled scan 2", "Untitled scan 3"]
            .map(|name| untitled_in(dir.path(), name));
        let [one, two, three] = [&first, &second, &third].map(sidecar);
        let to = dir.path().join("saved.pdf");
        let ran = Arc::new(AtomicBool::new(false));
        gpui_platform::headless().run({
            let ran = ran.clone();
            move |cx| {
                let scan = move |u: Untitled, cx: &mut App| {
                    let view = untitled(u, cx);
                    let (ocr, read) = (OcrConfig::default(), reading(files.clone()));
                    view.update(cx, |v, cx| v.start_scan(read, opts(), ocr, ONE_BY_ONE, cx));
                    view
                };
                let view = scan(first, cx);
                cx.spawn(async move |cx| {
                    until(cx, &view, |v| v.scan.is_none() && sidecar_current(v)).await;
                    assert_eq!(kept(&recovery), std::slice::from_ref(&one));
                    assert_eq!(sidecar_pages(&one), Some(2));
                    cx.update(|cx| view.update(cx, |v, cx| v.undo(cx)));
                    waiting(cx, |_| !one.exists()).await;
                    cx.update(|cx| view.update(cx, |v, cx| v.redo(cx)));
                    until(cx, &view, sidecar_current).await;
                    assert_eq!(sidecar_pages(&one), Some(2));
                    // An edit's sidecar being written when the edit and the
                    // scan are undone, and the view closed with nothing to
                    // ask about.
                    let unwanted = cx.update(|cx| {
                        view.update(cx, |v, cx| {
                            turn_page_1(v, cx);
                            v.write_recovery(cx);
                            assert!(sidecar_writing(v));
                            v.undo(cx);
                            v.undo(cx);
                            assert!(!v.is_dirty());
                            sidecar_unwanted(v)
                        })
                    });
                    drop(view);
                    closed_once_written(cx, unwanted, &recovery, "Untitled scan 1").await;
                    assert!(kept(&recovery).is_empty(), "none once it's written");
                    // Another's changes dropped while their sidecar is
                    // being written, and the view closed, as Don't Save
                    // drops them and closes it.
                    let view = cx.update(|cx| scan(second, cx));
                    until(cx, &view, |v| v.scan.is_none() && sidecar_current(v)).await;
                    assert_eq!(kept(&recovery), [two]);
                    let unwanted = cx.update(|cx| {
                        view.update(cx, |v, cx| {
                            turn_page_1(v, cx);
                            v.write_recovery(cx);
                            assert!(sidecar_writing(v));
                            v.discard_recovery();
                            sidecar_unwanted(v)
                        })
                    });
                    drop(view);
                    closed_once_written(cx, unwanted, &recovery, "Untitled scan 2").await;
                    assert!(kept(&recovery).is_empty(), "none once it's written");
                    // A third, saved to a file.
                    let view = cx.update(|cx| scan(third, cx));
                    until(cx, &view, |v| v.scan.is_none() && sidecar_current(v)).await;
                    assert_eq!(kept(&recovery), [three]);
                    let saving = cx.update(|cx| view.update(cx, |v, cx| v.save_to(to.clone(), cx)));
                    assert!(saving.await, "saved");
                    assert!(kept(&recovery).is_empty(), "saved, it keeps none");
                    assert!(recovery::find(&to).is_none());
                    cx.update(|cx| view.update(cx, turn_page_1));
                    waiting(cx, |_| recovery::find(&to).is_some()).await;
                    until(cx, &view, |v| !sidecar_writing(v)).await;
                    assert!(kept(&recovery).is_empty(), "and it's beside the file");
                    ran.store(true, Ordering::Relaxed);
                    cx.update(|cx| cx.quit());
                })
                .detach();
            }
        });
        assert!(ran.load(Ordering::Relaxed));
    }

    /// Waits until a closed view's sidecar write is done: until its name
    /// `name` in `dir` can be claimed. That's once `unwanted`, the view's
    /// flag, which the view and each write of it in flight hold, is held
    /// here alone: the write holds the name till it's done, so it never
    /// writes over, or removes, a new document's sidecar of that name.
    async fn closed_once_written(
        cx: &mut AsyncApp,
        unwanted: Arc<AtomicBool>,
        dir: &Path,
        name: &str,
    ) {
        waiting(cx, |cx| {
            // Lets the app release the view.
            cx.update(|_| {});
            let free = recovery::claim_untitled(dir, name).unwrap().is_some();
            let alone = Arc::strong_count(&unwanted) == 1;
            assert!(!free || alone, "claimed while its write runs");
            free
        })
        .await;
    }

    /// A child process, this test binary running one test alone: killed
    /// and waited for when it's dropped, so a parent's failure doesn't
    /// leave it running.
    struct Child {
        process: std::process::Child,
        /// What it says, on standard output and error.
        said: PathBuf,
    }

    impl Child {
        /// Test `name` run in a process of its own, [`ALONE`] set to `dir`,
        /// with its config folder `config`; what it says goes to a file in
        /// `dir`.
        fn spawn(name: &str, dir: &Path, config: &Path) -> Child {
            let said = dir.join("child.txt");
            let out = std::fs::File::create(&said).unwrap();
            let process = std::process::Command::new(std::env::current_exe().unwrap())
                .args([name, "--exact", "--nocapture", "--test-threads=1"])
                .env(ALONE, dir)
                .env("SHEAF_CONFIG_DIR", config)
                .stdout(out.try_clone().unwrap())
                .stderr(out)
                .spawn()
                .unwrap();
            Child { process, said }
        }

        /// Panics, with what it said, once it has ended.
        fn running(&mut self) {
            if let Some(status) = self.process.try_wait().unwrap() {
                let said = std::fs::read_to_string(&self.said).unwrap_or_default();
                panic!("the child ended ({status}):\n{said}");
            }
        }

        /// Killed (SIGKILL), as a crash kills sheaf: how it ended.
        fn kill(&mut self) -> std::process::ExitStatus {
            self.process.kill().unwrap();
            self.process.wait().unwrap()
        }
    }

    impl Drop for Child {
        fn drop(&mut self) {
            let _ = self.process.kill();
            let _ = self.process.wait();
        }
    }

    /// Each page's images' bytes in the PDF at `path`, in page order.
    fn page_images(path: &Path) -> Vec<Vec<Vec<u8>>> {
        let pdf = lopdf::Document::load(path).unwrap();
        (pdf.get_pages().into_values())
            .map(|page| {
                let images = pdf.get_page_images(page).unwrap();
                images.iter().map(|image| image.content.to_vec()).collect()
            })
            .collect()
    }

    /// A process killed once the first batch of its scan of twenty images
    /// landed (eight at a time, the rest held back) leaves that batch's
    /// sidecar, the untitled document's, in its state folder. While it
    /// runs, its claim keeps the name and the sidecar its own: another
    /// process can't claim the name, and a launch offers nothing. Once it's
    /// killed, the next launch offers it, if it owns sheaf's state:
    /// exactly "Untitled scan 1", with its images' folder. Recovered, it's
    /// the eight pages as an intake of their images makes them, image for
    /// image, unsaved and never saved, Save As starting in that folder, so
    /// its first save writes them and takes the sidecar away. The child is
    /// this test in a process of its own, whose config folder is the
    /// test's, all in a `sheaf-` temporary directory.
    #[test]
    fn a_scan_killed_after_its_first_batch_is_offered_at_the_next_launch() {
        const NAME: &str =
            "scan_run::tests::a_scan_killed_after_its_first_batch_is_offered_at_the_next_launch";
        if let Some(dir) = std::env::var_os(ALONE).map(PathBuf::from) {
            return scan_until_killed(&dir);
        }
        let dir = tempfile::Builder::new().prefix("sheaf-").tempdir().unwrap();
        let files = images(dir.path(), 20);
        let config = dir.path().join("config");
        let recovery = config.join("state").join("recovery");
        let sidecar = recovery.join("Untitled scan 1.sheaf-recovery");
        let mut child = Child::spawn(NAME, dir.path(), &config);
        let deadline = Instant::now() + Duration::from_secs(120);
        while sidecar_pages(&sidecar) != Some(8) {
            child.running();
            assert!(
                Instant::now() < deadline,
                "no sidecar of 8 pages in two minutes"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        let unclaimed = recovery::claim_untitled(&recovery, "Untitled scan 1").unwrap();
        assert!(unclaimed.is_none(), "the live child's");
        assert!(crate::workspace::offered(&recovery, true).is_empty());
        let ended = child.kill();
        {
            use std::os::unix::process::ExitStatusExt;
            assert_eq!(ended.signal(), Some(9), "killed, as a crash ends it");
        }
        assert!(crate::workspace::offered(&recovery, false).is_empty());
        let found = crate::workspace::offered(&recovery, true);
        let names: Vec<&str> = found.iter().map(|rec| rec.claim.name()).collect();
        assert_eq!(names, ["Untitled scan 1"]);
        let rec = found.into_iter().next().unwrap();
        let images = dir.path().join("images");
        assert_eq!(rec.folder, images);
        // The same eight images, as an intake makes them.
        let intake_pdf = dir.path().join("intake.pdf");
        let mut session = Session::new_empty();
        let mut landing = SessionLanding::new(&mut session, "Scan");
        let mut eight = Files::new(&files[..8]).unwrap();
        intake::intake(&mut eight, &opts(), None, &mut landing, EIGHT, &NoProgress).unwrap();
        landing.finish();
        let to = SaveTo::File {
            path: intake_pdf.clone(),
            append: false,
        };
        session
            .save(to, &AskAnswers::default(), &NoProgress)
            .unwrap();
        let saved = dir.path().join("recovered.pdf");
        let ran = Arc::new(AtomicBool::new(false));
        gpui_platform::headless().run({
            let (ran, saved) = (ran.clone(), saved.clone());
            move |cx| {
                let view = cx.new(|cx| {
                    let (who, at) = (Identity::default(), Instant::now());
                    DocView::recovered_untitled(rec, who, Default::default(), false, at, cx)
                });
                cx.spawn(async move |cx| {
                    let read =
                        |v: &DocView| matches!(&v.state, State::Ready(d) if d.reading.is_none());
                    until(cx, &view, read).await;
                    cx.update(|cx| {
                        view.update(cx, |v, _| {
                            assert_eq!(&*v.title(), "Untitled scan 1 •");
                            assert_eq!(v.path(), None);
                            assert_eq!(v.folder(), images);
                            assert!(v.is_dirty());
                            assert_eq!(pages(v), 8);
                        })
                    });
                    let saving = cx.update(|cx| view.update(cx, |v, cx| v.save_to(saved, cx)));
                    assert!(saving.await, "saved");
                    assert!(!sidecar.exists(), "its sidecar goes once it's saved");
                    ran.store(true, Ordering::Relaxed);
                    cx.update(|cx| cx.quit());
                })
                .detach();
            }
        });
        assert!(ran.load(Ordering::Relaxed));
        let (recovered, scanned) = (page_images(&saved), page_images(&intake_pdf));
        assert_eq!(recovered.len(), 8);
        assert!(recovered.iter().all(|images| images.len() == 1));
        assert!(recovered == scanned, "the intake's images");
        assert!(kept(&recovery).is_empty());
    }

    /// The child's half of the test above: Create from Scans of the twenty
    /// images in `dir`, named and kept as the workspace names and keeps one
    /// (`untitled::for_scan`, in the config folder the test gave), eight
    /// given at once and the rest never, until the parent kills it.
    fn scan_until_killed(dir: &Path) {
        let files = listed(&dir.join("images"));
        // Ended by the parent; and on its own if the parent never does.
        std::thread::spawn(|| {
            std::thread::sleep(Duration::from_secs(600));
            std::process::exit(2);
        });
        gpui_platform::headless().run(move |cx| {
            let u = crate::untitled::for_scan(&files);
            let (gated, gate) = Gated::new(&files, 8);
            let view = untitled(u, cx);
            view.update(cx, |v, cx| {
                v.start_scan(gated.opening(), opts(), OcrConfig::default(), EIGHT, cx);
            });
            cx.spawn(async move |_| {
                // The view kept, and the gate never opened.
                let _kept = (view, gate);
                futures::future::pending::<()>().await;
            })
            .detach();
        });
    }
}
