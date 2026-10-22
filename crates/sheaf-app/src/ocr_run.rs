//! Running an OCR job on one document. The job (see `sheaf_ocr::job`)
//! streams per-page layers; they're committed in flushes as one undoable
//! step, the run's group (M10.5 §8.4), the status bar shows progress, and
//! Escape or Document > Cancel OCR stops it, keeping what's done. Each
//! flush is made on the version its round began on, which its pages were
//! rendered from, and lands by value: a page changed since keeps no text
//! from it, and is recognized again, as it is then, in a round that
//! begins when this one ends (twice at most). A page whose content can't
//! be read gets no text, and is counted as failed. Undoing the run stops
//! it.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::StreamExt;
use gpui::{Context, Task};
use sheaf_doc::{
    Actor, Base, ChangeSet, CommitOpts, Document, EditError, GroupId, GroupKind, JobKind, PageId,
    Snapshot, Stale, TextLayer, ViewInputs,
};
use sheaf_edit::{Op, PageRef};
use sheaf_ocr::{JobHandle, OcrConfig, OcrEvent, OcrPage, OcrRequest, Scope};
use sheaf_ops::views::{ViewPages, Views};

use crate::docview::{DocView, State};
use crate::ocr_dialog::OcrSpec;

/// Results are applied to the document every so many pages or seconds,
/// whichever comes first, so Find sees them while the run continues.
const FLUSH_PAGES: usize = 10;
const FLUSH_EVERY: Duration = Duration::from_secs(2);

/// How often a page changed during the run is recognized again, at most.
const REQUEUES: u8 = 2;

pub(crate) struct OcrRun {
    /// Dev hook: save here when the run finishes.
    save_to: Option<std::path::PathBuf>,
    /// The running round's job, and the task taking its events.
    handle: JobHandle,
    _task: Task<()>,
    /// What each round asks of the engine, but its pages.
    ask: OcrRequest,
    tally: Tally,
}

/// A run apart from its jobs and the view: what its rounds brought in,
/// what its flushes made of it, and whether it begins another round or
/// ends, so tests can drive it with a job's events ([`Tally::take`]).
struct Tally {
    /// The run's group, and the version the running round began on, which
    /// its pages were rendered from.
    group: GroupId,
    base: Snapshot,
    engine: String,
    /// Pages the rounds read: a page read again counts again.
    total: usize,
    /// Results that came in, `dropped` ones included.
    done: usize,
    /// Results dropped because their page changed since their round began.
    dropped: usize,
    skipped: usize,
    /// (1-based page number, error).
    failed: Vec<(usize, String)>,
    /// Pages changed since the run began, whose text wasn't added.
    changed: usize,
    /// Pages changed since their round began, to recognize again.
    requeue: Requeue,
    /// Pages read again after a change.
    again: usize,
    pending: Vec<(PageId, Arc<TextLayer>)>,
    /// At least one flush has been committed.
    flushed: bool,
    /// The user undid the run's results: none are added again.
    undone: bool,
    last_flush: Instant,
    started: Instant,
}

/// What a run does after an event ([`Tally::take`]).
#[derive(Debug, PartialEq)]
enum Next {
    /// Takes the running round's next event.
    Wait,
    /// Begins a round reading these pages as the tally's base shows them.
    Round(Vec<PageId>),
    /// Is over, its group ended: what it came to.
    Over(String),
    /// Is over, no engine loaded: why.
    Aborted(String),
}

/// What an event came to: the commits of the flush it made, if any, the
/// notice that flush left, and what the run does next.
#[derive(Debug)]
struct Taken {
    changes: Vec<ChangeSet>,
    notice: Option<String>,
    next: Next,
}

impl Tally {
    /// A run whose first round reads `pages` as `base` shows them, its
    /// results committed in `group`.
    fn new(group: GroupId, base: Snapshot, pages: usize, engine: String) -> Self {
        Tally {
            group,
            base,
            engine,
            total: pages,
            done: 0,
            dropped: 0,
            skipped: 0,
            failed: Vec::new(),
            changed: 0,
            requeue: Requeue::default(),
            again: 0,
            pending: Vec::new(),
            flushed: false,
            undone: false,
            last_flush: Instant::now(),
            started: Instant::now(),
        }
    }

    fn processed(&self) -> usize {
        self.done + self.skipped + self.failed.len()
    }

    /// One event of the running round's job taken into `model`: counted,
    /// its results flushed when due, and, once the round is finished, the
    /// next round's pages, or the run ended. `stopped` says the user
    /// stopped the run, which then begins no round, as undoing it doesn't.
    fn take(&mut self, ev: OcrEvent, model: &mut Document, stopped: bool) -> Taken {
        let mut due = false;
        let mut finished: Option<bool> = None;
        match ev {
            OcrEvent::Started { engine, .. } => self.engine = engine,
            OcrEvent::Skipped { id } => {
                self.skipped += 1;
                self.again += usize::from(self.requeue.read(id));
            }
            OcrEvent::Page { id, layer } => {
                self.done += 1;
                self.again += usize::from(self.requeue.read(id));
                self.pending.push((id, Arc::new(layer)));
                due = self.pending.len() >= FLUSH_PAGES || self.last_flush.elapsed() >= FLUSH_EVERY;
            }
            OcrEvent::Failed { id, index, error } => {
                let number = (model.index_of(id)).map_or(index as usize + 1, |i| i + 1);
                log::warn!("ocr: page {number}: {error}");
                self.failed.push((number, error));
                self.again += usize::from(self.requeue.read(id));
            }
            OcrEvent::Finished { cancelled, .. } => {
                due = true;
                finished = Some(cancelled);
            }
            OcrEvent::Aborted(msg) => {
                log::error!("ocr: {msg}");
                model.end_group(self.group);
                return Taken {
                    changes: Vec::new(),
                    notice: None,
                    next: Next::Aborted(format!("OCR failed: {msg}")),
                };
            }
        }
        let (changes, notice) = if due {
            self.flush(model)
        } else {
            (Vec::new(), None)
        };
        let next = match finished {
            None => Next::Wait,
            Some(cancelled) => match self.next_round(model, stopped || cancelled) {
                Some(ids) => Next::Round(ids),
                None => {
                    model.end_group(self.group);
                    Next::Over(self.message(cancelled))
                }
            },
        };
        Taken {
            changes,
            notice,
            next,
        }
    }

    /// After a round, the next one's pages: those changed since their
    /// round began and still in `model`, read as its current version shows
    /// them, which is then the base their flushes are made on. None once
    /// the run was stopped or undone.
    fn next_round(&mut self, model: &Document, stopped: bool) -> Option<Vec<PageId>> {
        if stopped || self.undone {
            return None;
        }
        let (ids, gone) = self.requeue.take(model);
        self.changed += gone;
        if ids.is_empty() {
            return None;
        }
        self.base = model.snapshot();
        self.total += ids.len();
        Some(ids)
    }

    /// Commit the pending layers in the run's group: part of its one undo
    /// step, or another fragment of it after the user's edits. When some
    /// page changed since its round began, or a page's content can't be
    /// read, the layers go one by one: a changed page's is dropped, and the
    /// page read again in the next round (twice at most; then it keeps
    /// none), and a page that can't be read has failed. Returns the
    /// commits' changes, for the view to follow, and a notice when the
    /// flush stopped: the user undid the run, which then stops, or another
    /// error.
    fn flush(&mut self, model: &mut Document) -> (Vec<ChangeSet>, Option<String>) {
        let pending = std::mem::take(&mut self.pending);
        if pending.is_empty() || self.undone {
            return (Vec::new(), None);
        }
        let landed = land(model, &self.base, self.group, &pending);
        if landed.changes.iter().any(|ch| ch.from != ch.to) {
            self.flushed = true;
            self.last_flush = Instant::now();
        }
        for page in landed.requeue {
            // Counted as recognized when it came in: dropped instead.
            self.dropped += 1;
            if !self.requeue.push(page) {
                self.changed += 1;
            }
        }
        for (page, e) in landed.failed {
            let at = model.index_of(page).or(self.base.state.pos_of(page));
            let number = at.map_or(0, |i| i + 1);
            log::warn!("ocr: page {number}: {e}");
            // Counted as recognized when it came in: a failure instead.
            self.done -= 1;
            self.failed.push((number, e.to_string()));
        }
        let notice = match landed.stopped {
            None => None,
            Some(EditError::GroupEnded) => {
                // The user undid the run: don't put the results back.
                self.undone = true;
                Some("OCR stopped: its results were undone".into())
            }
            Some(e) => {
                log::error!("ocr: applying results: {e:#}");
                Some(crate::docview::failed("OCR: applying results", &e))
            }
        };
        (landed.changes, notice)
    }

    /// What the run came to, once it's over.
    fn message(&mut self, cancelled: bool) -> String {
        if self.undone {
            return "OCR stopped: its results were undone".into();
        }
        // Pages whose results were dropped for a change, which the run
        // stopped before reading again.
        self.changed += self.requeue.left();
        let s = |n: usize| if n == 1 { "" } else { "s" };
        let recognized = self.done - self.dropped;
        let mut msg = format!(
            "OCR {}: {} page{} recognized",
            if cancelled { "stopped" } else { "done" },
            recognized,
            s(recognized)
        );
        if self.again > 0 {
            msg.push_str(&format!(", {} read again after a change", self.again));
        }
        if self.skipped > 0 {
            msg.push_str(&format!(", {} skipped (had text)", self.skipped));
        }
        if self.changed > 0 {
            msg.push_str(&format!(
                ", {} changed during the run (their text wasn't added)",
                self.changed
            ));
        }
        if !self.failed.is_empty() {
            let pages: Vec<String> = (self.failed.iter())
                .take(5)
                .map(|(n, _)| n.to_string())
                .collect();
            msg.push_str(&format!(
                ", {} failed (p. {}{})",
                self.failed.len(),
                pages.join(", "),
                if self.failed.len() > 5 { ", …" } else { "" }
            ));
        }
        msg.push_str(&format!(
            " with {} in {:.0?}",
            self.engine,
            self.started.elapsed()
        ));
        if self.flushed {
            msg.push_str(" — Ctrl+Z undoes");
        }
        msg
    }
}

/// The pages a run recognizes again: those changed since the round that
/// read them began.
#[derive(Default)]
struct Requeue {
    /// For the round after the running one.
    next: Vec<PageId>,
    /// The running round's, not read again yet.
    reading: HashSet<PageId>,
    /// How often each page's result was dropped.
    tries: HashMap<PageId, u8>,
}

impl Requeue {
    /// `page`'s result dropped for a change: false when it was recognized
    /// again [`REQUEUES`] times already, and isn't again.
    fn push(&mut self, page: PageId) -> bool {
        let tries = self.tries.entry(page).or_default();
        *tries += 1;
        let again = *tries <= REQUEUES;
        if again {
            self.next.push(page);
        }
        again
    }

    /// The next round's pages, those still in `model`, in its order; and
    /// how many it drops: those gone from it, and any the round before
    /// didn't read again.
    fn take(&mut self, model: &Document) -> (Vec<PageId>, usize) {
        let next = std::mem::take(&mut self.next);
        let mut kept: Vec<(usize, PageId)> = (next.iter())
            .filter_map(|&p| Some((model.index_of(p)?, p)))
            .collect();
        kept.sort_unstable();
        let pages: Vec<PageId> = kept.into_iter().map(|(_, p)| p).collect();
        let unread = std::mem::replace(&mut self.reading, pages.iter().copied().collect());
        let dropped = next.len() - pages.len() + unread.len();
        (pages, dropped)
    }

    /// Whether `page`, whose result came in, is one the round reads again.
    fn read(&mut self, page: PageId) -> bool {
        self.reading.remove(&page)
    }

    /// The pages whose results were dropped and weren't read again.
    fn left(&self) -> usize {
        self.next.len() + self.reading.len()
    }
}

/// A round reading `ids` as `base` shows them, as `ask` asks: its job, and
/// the task taking the job's events into the view.
fn round(
    views: Views,
    base: &Snapshot,
    ids: &[PageId],
    ask: &OcrRequest,
    cx: &mut Context<DocView>,
) -> (JobHandle, Task<()>) {
    let source = ViewPages::new(views, base.clone());
    let pages: Vec<OcrPage> = ids.iter().filter_map(|&id| source.page(id)).collect();
    let req = OcrRequest {
        pages,
        ..ask.clone()
    };
    let (handle, mut rx) = sheaf_ocr::job::start(req, Arc::new(source));
    let task = cx.spawn(async move |this, cx| {
        while let Some(ev) = rx.next().await {
            let stop = this
                .update(cx, |view, cx| view.on_ocr_event(ev, cx))
                .unwrap_or(true);
            if stop {
                break;
            }
        }
    });
    (handle, task)
}

impl DocView {
    /// What the dialog needs: (page count, current page, selected count).
    pub(crate) fn ocr_context(&self) -> Option<(usize, usize, usize)> {
        let State::Ready(doc) = &self.state else {
            return None;
        };
        Some((
            doc.model.page_count(),
            doc.current_page(),
            doc.selection.len(),
        ))
    }

    pub(crate) fn ocr_running(&self) -> bool {
        self.ocr.is_some()
    }

    /// Fraction of the run done, while one is running.
    pub(crate) fn ocr_progress(&self) -> Option<f32> {
        let tally = &self.ocr.as_ref()?.tally;
        Some(if tally.total == 0 {
            0.0
        } else {
            (tally.processed() as f32 / tally.total as f32).clamp(0.0, 1.0)
        })
    }

    pub(crate) fn start_ocr(&mut self, spec: OcrSpec, config: OcrConfig, cx: &mut Context<Self>) {
        if self.ocr.is_some() {
            self.set_notice("OCR is already running — Esc cancels it", cx);
            return;
        }
        let State::Ready(doc) = &mut self.state else {
            return;
        };
        if doc.saving {
            doc.notice = Some("Wait for the save to finish before running OCR".into());
            cx.notify();
            return;
        }
        let st = doc.model.state();
        let ids: Vec<PageId> = match spec.scope {
            Scope::All => st.order().iter().copied().collect(),
            Scope::Current => doc.page_id(doc.current_page()).into_iter().collect(),
            Scope::Selected => (st.order().iter())
                .filter(|p| doc.selection.contains(p))
                .copied()
                .collect(),
            Scope::Range => spec.pages.iter().filter_map(|&i| st.page_at(i)).collect(),
        };
        if ids.is_empty() {
            doc.notice = Some("No pages to recognize".into());
            cx.notify();
            return;
        }
        let ask = OcrRequest {
            pages: Vec::new(),
            skip_pages_with_text: spec.skip_pages_with_text,
            engine: spec.engine,
            config,
        };
        // The version the run begins on, whose pages it renders.
        let base = doc.model.snapshot();
        let (handle, task) = round(doc.views.clone(), &base, &ids, &ask, cx);
        let group = doc.model.begin_group(GroupKind::Ocr);
        doc.notice = Some(format!("OCR: loading {}…", spec.engine.label()));
        let engine = spec.engine.label().to_string();
        self.ocr = Some(OcrRun {
            save_to: None,
            handle,
            _task: task,
            ask,
            tally: Tally::new(group, base, ids.len(), engine),
        });
        cx.notify();
    }

    /// `--selftest-ocr <out.pdf>`: recognize every page with the default
    /// engine and save the result, so the whole path can run without a
    /// keyboard.
    pub(crate) fn selftest_ocr(&mut self, path: std::path::PathBuf, cx: &mut Context<Self>) {
        let config = OcrConfig::default();
        let spec = OcrSpec {
            scope: Scope::All,
            pages: Vec::new(),
            skip_pages_with_text: true,
            engine: config.engine,
            language: sheaf_ocr::configured_language(config.engine, &config).to_string(),
            dpi: config.dpi,
        };
        self.start_ocr(spec, config, cx);
        if let Some(run) = &mut self.ocr {
            run.save_to = Some(path);
        }
    }

    /// Stop the run, keeping what's recognized so far. Returns whether
    /// there was one.
    pub(crate) fn cancel_ocr(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(run) = &self.ocr else {
            return false;
        };
        run.handle.cancel();
        self.set_notice("OCR: stopping after the current page…", cx);
        true
    }

    /// Clear the notice if it still says `text` (a "working…" message
    /// whose work ended quietly).
    #[cfg_attr(not(feature = "plugins"), allow(dead_code))]
    pub(crate) fn clear_notice(&mut self, cx: &mut Context<Self>) {
        if let State::Ready(doc) = &mut self.state {
            doc.notice = None;
        }
        cx.notify();
    }

    #[cfg_attr(not(feature = "plugins"), allow(dead_code))]
    pub(crate) fn clear_notice_if(&mut self, text: &str, cx: &mut Context<Self>) {
        if let State::Ready(doc) = &mut self.state
            && doc.notice.as_deref() == Some(text)
        {
            doc.notice = None;
            cx.notify();
        }
    }

    #[cfg_attr(not(feature = "plugins"), allow(dead_code))]
    pub(crate) fn notice(&self) -> Option<&str> {
        match &self.state {
            State::Ready(doc) => doc.notice.as_deref(),
            _ => None,
        }
    }

    pub(crate) fn set_notice(&mut self, text: impl Into<String>, cx: &mut Context<Self>) {
        if let State::Ready(doc) = &mut self.state {
            doc.notice = Some(text.into());
        }
        cx.notify();
    }

    /// Handle one job event ([`Tally::take`]); true means the round's job
    /// is done with: the run is over, or the next round took its place.
    /// The view follows each commit ([`DocView::on_change`]), made on the
    /// model rather than through [`DocView::commit`], whose notice would
    /// replace the run's own and tell each page left out as an error. A
    /// run whose document was opened again ends with it.
    fn on_ocr_event(&mut self, ev: OcrEvent, cx: &mut Context<Self>) -> bool {
        let (Some(run), State::Ready(doc)) = (&mut self.ocr, &mut self.state) else {
            self.ocr = None;
            return true;
        };
        let taken = run
            .tally
            .take(ev, &mut doc.model, run.handle.is_cancelled());
        if run.tally.undone {
            run.handle.cancel();
        }
        if let Some(notice) = taken.notice {
            doc.notice = Some(notice);
        }
        for ch in &taken.changes {
            self.on_change(ch, cx);
        }
        let ids = match taken.next {
            Next::Wait => None,
            Next::Round(ids) => Some(ids),
            Next::Over(msg) => {
                let run = self.ocr.take().expect("run exists");
                if self.timing {
                    eprintln!("[timing] {msg}");
                }
                if let Some(path) = run.save_to {
                    eprintln!("[selftest] {msg}; saving to {}", path.display());
                    self.save_to(path, cx).detach();
                } else {
                    self.set_notice(msg, cx);
                }
                return true;
            }
            Next::Aborted(msg) => {
                self.ocr = None;
                self.set_notice(msg, cx);
                return true;
            }
        };
        let (Some(run), State::Ready(doc)) = (&mut self.ocr, &mut self.state) else {
            return true;
        };
        if let Some(ids) = &ids {
            // The finished round's job and task go: the task is the one
            // taking this event, and stops once it's taken.
            let (handle, task) = round(doc.views.clone(), &run.tally.base, ids, &run.ask, cx);
            run.handle = handle;
            run._task = task;
        }
        // Stopping: the notice says why.
        if !run.tally.undone {
            let tally = &run.tally;
            doc.notice = Some(format!(
                "OCR {}/{} — {} skipped — {} — Esc stops",
                tally.processed(),
                tally.total,
                tally.skipped,
                tally.engine
            ));
        }
        cx.notify();
        ids.is_some()
    }
}

/// What a flush made of its layers.
#[derive(Debug, Default)]
struct Landed {
    /// Its commits' changes.
    changes: Vec<ChangeSet>,
    /// The pages changed since the flush's base, whose layers were dropped.
    requeue: Vec<PageId>,
    /// The pages whose layers can't be added: their content can't be read.
    failed: Vec<(PageId, EditError)>,
    /// What stopped it: the run's group ended (the user undid it), or
    /// another error.
    stopped: Option<EditError>,
}

/// `layers` committed in `group`, made on `base`: in one commit, or page
/// by page ([`page_by_page`]) when a page changed since or can't be read.
fn land(
    model: &mut Document,
    base: &Snapshot,
    group: GroupId,
    layers: &[(PageId, Arc<TextLayer>)],
) -> Landed {
    match flush(model, base, group, layers) {
        Ok(change) => Landed {
            changes: vec![change],
            ..Landed::default()
        },
        Err(EditError::Conflict { .. } | EditError::Content { .. }) => {
            page_by_page(model, base, group, layers)
        }
        Err(e) => Landed {
            stopped: Some(e),
            ..Landed::default()
        },
    }
}

/// `layers` committed in `group`, made on `base`, the version their pages
/// were rendered from, and deciding by their pages as OCR read them (their
/// own content, what each shows), so a page changed since is a Conflict:
/// the change.
fn flush(
    model: &mut Document,
    base: &Snapshot,
    group: GroupId,
    layers: &[(PageId, Arc<TextLayer>)],
) -> Result<ChangeSet, EditError> {
    let mut tx = Document::tx_at(base, model.lower_cx(Actor::Job(JobKind::Ocr)));
    for &(page, _) in layers {
        tx.read_render(page, &ViewInputs::content_only());
    }
    let ops = (layers.iter())
        .map(|(page, layer)| Op::SetOcrText {
            page: PageRef::Id(*page),
            layer: layer.clone(),
        })
        .collect();
    tx.apply_batch(ops, &sheaf_ops::NoProgress)?;
    let opts = CommitOpts {
        base: Base::At(base.version),
        stale: Stale::Validate,
        group: Some(group),
        ..CommitOpts::labelled("Recognize text")
    };
    Ok(model.commit(tx, opts)?.change)
}

/// `layers` flushed one by one ([`flush`]): a page changed since `base` is
/// re-queued, one whose content can't be read has failed, and the others
/// land, until the group ends or another error stops it.
fn page_by_page(
    model: &mut Document,
    base: &Snapshot,
    group: GroupId,
    layers: &[(PageId, Arc<TextLayer>)],
) -> Landed {
    let mut landed = Landed::default();
    for layer in layers {
        match flush(model, base, group, std::slice::from_ref(layer)) {
            Ok(change) => landed.changes.push(change),
            Err(EditError::Conflict { .. }) => landed.requeue.push(layer.0),
            Err(e @ EditError::Content { .. }) => landed.failed.push((layer.0, e)),
            Err(e) => {
                landed.stopped = Some(e);
                break;
            }
        }
    }
    landed
}

#[cfg(test)]
mod tests {
    use super::*;
    use sheaf_doc::tx::fixture as fx;
    use sheaf_doc::{Box2, FixedCount, OcrWord, Store};
    use sheaf_edit::{Margins, Rotation, Turn};

    /// `n` blank Letter pages.
    fn blank(n: u32) -> Document {
        let store = Store::new();
        let primary = sheaf_doc::store::blank(&store, n);
        let policy = Box::new(FixedCount(10));
        Document::new(store, primary, "a.pdf".into(), None, &[], policy)
    }

    /// A file of Letter pages, each drawing its content, opened.
    fn drawing(contents: &[&[u8]]) -> (tempfile::TempDir, Document) {
        let mut pdf = sheaf_doc::text_layer::fixture::pages_drawing(contents);
        let dir = tempfile::Builder::new().prefix("sheaf-").tempdir().unwrap();
        let path = dir.path().join("pages.pdf");
        pdf.save(&path).unwrap();
        let doc = sheaf_doc::open::file(&path, None).unwrap().doc;
        (dir, doc)
    }

    /// An OCR run begun on the current version: its group, and that
    /// version, which its first round renders.
    fn start(d: &mut Document) -> (GroupId, Snapshot) {
        (d.begin_group(GroupKind::Ocr), d.snapshot())
    }

    /// Recognized text saying `text` near `page`'s top, as it shows now.
    fn layer(d: &Document, page: PageId, text: &str) -> (PageId, Arc<TextLayer>) {
        let word = OcrWord {
            text: text.into(),
            rect: Box2 {
                x: 72.0,
                y: 80.0,
                w: 60.0,
                h: 12.0,
            },
            confidence: 0.9,
        };
        let layer = sheaf_doc::text_layer::from_engine(vec![word], &d.geom(page), "test");
        (page, Arc::new(layer))
    }

    /// The pages that have recognized text, in order.
    fn recognized(d: &Document) -> Vec<PageId> {
        (d.state().pages())
            .filter(|(_, def, _)| def.content.ocr_text().is_some())
            .map(|(id, _, _)| id)
            .collect()
    }

    /// A page replaced from another file after the run's round rendered it
    /// keeps no text from that round: it's re-queued, and the other pages'
    /// text lands. Read again as the version since shows it, in the next
    /// round, its text lands too, in the run's group and its one version:
    /// one undo takes the run's text back and keeps the replace.
    #[test]
    fn a_replaced_page_is_recognized_again() {
        let mut d = blank(3);
        let [p0, p1, p2] = [0, 1, 2].map(|i| d.page(i).unwrap());
        let (group, base) = start(&mut d);
        let layers = [p0, p1, p2].map(|p| layer(&d, p, "first"));
        let other = sheaf_doc::store::blank(d.store(), 1);
        let replace = fx::replace(&[p1], &other, [0], "b.pdf");
        d.commit_ops(vec![replace], "Replace").unwrap();
        let replaced = d.state().def(p1).unwrap().clone();
        let n = d.version_count();
        let landed = land(&mut d, &base, group, &layers);
        assert_eq!(landed.requeue, [p1]);
        assert!(landed.failed.is_empty(), "{landed:?}");
        assert!(landed.stopped.is_none(), "{landed:?}");
        assert_eq!(landed.changes.len(), 2);
        assert_eq!(recognized(&d), [p0, p2]);
        let mut requeue = Requeue::default();
        for page in landed.requeue {
            assert!(requeue.push(page));
        }
        let (next, dropped) = requeue.take(&d);
        assert_eq!((&next[..], dropped), (&[p1][..], 0));
        let base = d.snapshot();
        let again = layer(&d, p1, "again");
        assert!(requeue.read(p1));
        assert_eq!(requeue.left(), 0);
        let landed = land(&mut d, &base, group, std::slice::from_ref(&again));
        assert!(
            landed.requeue.is_empty() && landed.stopped.is_none(),
            "{landed:?}"
        );
        assert_eq!(landed.changes.len(), 1);
        assert_eq!(recognized(&d), [p0, p1, p2]);
        assert!(Arc::ptr_eq(d.state().ocr_text(p1).unwrap(), &again.1));
        assert_eq!(d.version_count(), n + 1, "the run's one version");
        assert_eq!(d.undo_label(), Some("Recognize text"));
        d.undo().unwrap();
        assert!(recognized(&d).is_empty());
        assert_eq!(d.state().def(p1), Some(&replaced), "the replace stays");
    }

    /// A page is recognized again twice at most; the next round reads the
    /// pages still in the document, in its order, and drops those gone and
    /// those the round before didn't read again, which are left until then.
    #[test]
    fn a_page_is_recognized_again_twice_at_most() {
        let mut d = blank(3);
        let [p0, p1, p2] = [0, 1, 2].map(|i| d.page(i).unwrap());
        let mut requeue = Requeue::default();
        assert!(requeue.push(p2) && requeue.push(p1) && requeue.push(p0));
        d.commit_ops(vec![fx::delete(&[p0])], "Delete").unwrap();
        let (next, dropped) = requeue.take(&d);
        assert_eq!((&next[..], dropped), (&[p1, p2][..], 1), "p0, gone");
        assert!(requeue.read(p2) && !requeue.read(p2));
        assert!(!requeue.read(p0), "not in the round");
        assert_eq!(requeue.left(), 1);
        assert!(requeue.push(p2), "again");
        assert!(!requeue.push(p2), "a third time");
        assert_eq!(requeue.left(), 2);
        let (next, dropped) = requeue.take(&d);
        assert_eq!((&next[..], dropped), (&[p2][..], 1), "p1, not read again");
        assert_eq!(requeue.left(), 1);
    }

    /// A run begun on the current version, as the view begins one, that
    /// reads `pages` in its first round.
    fn tally(d: &mut Document, pages: usize) -> Tally {
        let (group, base) = start(d);
        Tally::new(group, base, pages, "test".into())
    }

    /// `page`'s result, saying `text`, taken into the run.
    fn result(t: &mut Tally, d: &mut Document, page: PageId, text: &str) -> Next {
        let layer = Arc::unwrap_or_clone(layer(d, page, text).1);
        t.take(OcrEvent::Page { id: page, layer }, d, false).next
    }

    /// The running round's end taken into the run.
    fn finish(t: &mut Tally, d: &mut Document, cancelled: bool) -> Next {
        let ev = OcrEvent::Finished {
            done: 0,
            skipped: 0,
            failed: 0,
            cancelled,
        };
        t.take(ev, d, cancelled).next
    }

    /// What the run came to, but its engine and time.
    fn over(next: Next) -> String {
        let Next::Over(msg) = next else {
            panic!("not over: {next:?}");
        };
        msg.split(" with ").next().unwrap().to_string()
    }

    /// The run as the view drives it: a page replaced while the first
    /// round read it keeps none of that round's text, and the next round,
    /// begun when it ends, reads it as the version since shows it, on
    /// which its flush is made; its text lands in the run's one version,
    /// and the page counts once as recognized.
    #[test]
    fn a_run_reads_a_replaced_page_again_in_its_next_round() {
        let mut d = blank(3);
        let [p0, p1, p2] = [0, 1, 2].map(|i| d.page(i).unwrap());
        let mut t = tally(&mut d, 3);
        for p in [p0, p1, p2] {
            assert_eq!(result(&mut t, &mut d, p, "first"), Next::Wait);
        }
        let other = sheaf_doc::store::blank(d.store(), 1);
        let replace = fx::replace(&[p1], &other, [0], "b.pdf");
        d.commit_ops(vec![replace], "Replace").unwrap();
        let n = d.version_count();
        assert_eq!(finish(&mut t, &mut d, false), Next::Round(vec![p1]));
        assert_eq!(recognized(&d), [p0, p2]);
        assert_eq!((t.base.version, t.total), (d.current_version(), 4));
        assert_eq!(result(&mut t, &mut d, p1, "again"), Next::Wait);
        let msg = over(finish(&mut t, &mut d, false));
        assert_eq!(
            msg,
            "OCR done: 3 pages recognized, 1 read again after a change"
        );
        assert_eq!(recognized(&d), [p0, p1, p2]);
        assert_eq!(d.state().ocr_text(p1).unwrap().words[0].text, "again");
        assert_eq!(d.version_count(), n + 1, "the run's one version");
        assert_eq!(t.processed(), t.total);
    }

    /// A page whose result was dropped and that isn't read again counts as
    /// changed, not recognized: when the run is stopped before its next
    /// round, when it's gone by then, and when it changed a third time.
    #[test]
    fn a_page_not_read_again_counts_as_changed() {
        let replace = |d: &mut Document, page: PageId| {
            let other = sheaf_doc::store::blank(d.store(), 1);
            let replace = fx::replace(&[page], &other, [0], "b.pdf");
            d.commit_ops(vec![replace], "Replace").unwrap();
        };
        let changed = "1 changed during the run (their text wasn't added)";
        // Stopped.
        let mut d = blank(3);
        let [p0, p1, p2] = [0, 1, 2].map(|i| d.page(i).unwrap());
        let mut t = tally(&mut d, 3);
        for p in [p0, p1, p2] {
            result(&mut t, &mut d, p, "a");
        }
        replace(&mut d, p1);
        let msg = over(finish(&mut t, &mut d, true));
        assert_eq!(msg, format!("OCR stopped: 2 pages recognized, {changed}"));
        assert_eq!(recognized(&d), [p0, p2]);
        // Gone: dropped by a flush during the round, deleted before its end.
        let mut d = blank(3);
        let [p0, p1, p2] = [0, 1, 2].map(|i| d.page(i).unwrap());
        let mut t = tally(&mut d, 3);
        for p in [p0, p1, p2] {
            result(&mut t, &mut d, p, "a");
        }
        replace(&mut d, p1);
        assert_eq!(t.flush(&mut d).0.len(), 2);
        d.commit_ops(vec![fx::delete(&[p1])], "Delete").unwrap();
        let msg = over(finish(&mut t, &mut d, false));
        assert_eq!(msg, format!("OCR done: 2 pages recognized, {changed}"));
        // Changed in each of its three rounds.
        let mut d = blank(1);
        let p0 = d.page(0).unwrap();
        let mut t = tally(&mut d, 1);
        for round in 0..3 {
            result(&mut t, &mut d, p0, "a");
            replace(&mut d, p0);
            let next = finish(&mut t, &mut d, false);
            if round < 2 {
                assert_eq!(next, Next::Round(vec![p0]));
            } else {
                let again = "2 read again after a change";
                let msg = format!("OCR done: 0 pages recognized, {again}, {changed}");
                assert_eq!(over(next), msg);
            }
        }
        assert!(recognized(&d).is_empty());
    }

    /// A crop and a quarter turn while the run's round reads the pages
    /// don't conflict with its flush: the text lands on both, its words
    /// where they were recognized in PageSpace, and the crop and the turn
    /// hold.
    #[test]
    fn a_crop_and_a_turn_during_the_run_land() {
        let mut d = blank(2);
        let [p0, p1] = [0, 1].map(|i| d.page(i).unwrap());
        let (group, base) = start(&mut d);
        let layers = [layer(&d, p0, "cropped"), layer(&d, p1, "turned")];
        let margins = Margins {
            top: 36.0,
            bottom: 36.0,
            left: 36.0,
            right: 36.0,
        };
        d.commit_ops(vec![fx::crop(&[p0], margins)], "Crop")
            .unwrap();
        d.commit_ops(vec![fx::rotate(&[p1], Turn::Quarter)], "Rotate")
            .unwrap();
        let (cropped, turned) = (d.geom(p0), d.geom(p1));
        let landed = land(&mut d, &base, group, &layers);
        assert_eq!(landed.changes.len(), 1, "one commit: {landed:?}");
        assert!(landed.requeue.is_empty() && landed.failed.is_empty());
        assert!(landed.stopped.is_none());
        for (page, layer) in &layers {
            let text = d.state().ocr_text(*page).unwrap();
            let rects = |t: &TextLayer| t.words.iter().map(|w| w.rect).collect::<Vec<_>>();
            assert_eq!(rects(text), rects(layer));
        }
        assert_eq!((d.geom(p0), d.geom(p1)), (cropped, turned));
        assert_ne!(cropped.crop, cropped.media);
        assert_eq!(turned.rotate, Rotation::R90);
    }

    /// Once the user undid the run's text, its next flush is GroupEnded,
    /// which stops the run, and changes nothing.
    #[test]
    fn a_flush_after_undo_ends_the_run() {
        let mut d = blank(2);
        let [p0, p1] = [0, 1].map(|i| d.page(i).unwrap());
        let (group, base) = start(&mut d);
        let first = layer(&d, p0, "a");
        let landed = land(&mut d, &base, group, &[first]);
        assert!(landed.stopped.is_none() && landed.changes.len() == 1);
        d.undo().unwrap();
        let (at, n) = (d.current_version(), d.version_count());
        let next = layer(&d, p1, "b");
        let landed = land(&mut d, &base, group, &[next]);
        assert_eq!(landed.stopped, Some(EditError::GroupEnded));
        assert!(landed.changes.is_empty() && landed.requeue.is_empty());
        assert_eq!((d.current_version(), d.version_count()), (at, n));
        assert!(recognized(&d).is_empty());
    }

    /// A page whose content can't be read takes no text and fails, named
    /// by its place in the document; the others' text lands.
    #[test]
    fn a_page_that_can_t_be_read_fails_and_the_rest_land() {
        let fill: &[u8] = b"0 0 9 9 re f";
        let (_dir, mut d) = drawing(&[fill, b"BT (never ends Tj ET", fill]);
        let [p0, p1, p2] = [0, 1, 2].map(|i| d.page(i).unwrap());
        let (group, base) = start(&mut d);
        let layers = [p0, p1, p2].map(|p| layer(&d, p, "a"));
        let landed = land(&mut d, &base, group, &layers);
        let [(page, e)] = &landed.failed[..] else {
            panic!("{landed:?}");
        };
        assert_eq!(*page, p1);
        assert!(
            matches!(
                e,
                EditError::Content {
                    op: "set-ocr-text",
                    page: 1,
                    ..
                }
            ),
            "{e}"
        );
        assert!(landed.requeue.is_empty() && landed.stopped.is_none());
        assert_eq!(landed.changes.len(), 2);
        assert_eq!(recognized(&d), [p0, p2]);
    }
}
