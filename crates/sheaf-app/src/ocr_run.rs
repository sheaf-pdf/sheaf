//! Running an OCR job on one document. The job (see `sheaf_ocr::job`)
//! streams per-page layers; they're committed in flushes as one undoable
//! step, the run's group (M10.5 §8.4), the status bar shows progress, and
//! Escape or Document > Cancel OCR stops it, keeping what's done. Each
//! flush is made on the version the run began on, which its pages were
//! rendered from, and lands by value: a page changed since keeps no text.
//! Undoing the run stops it.

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
use sheaf_ops::views::ViewPages;

use crate::docview::{DocView, State};
use crate::ocr_dialog::OcrSpec;

/// Results are applied to the document every so many pages or seconds,
/// whichever comes first, so Find sees them while the run continues.
const FLUSH_PAGES: usize = 10;
const FLUSH_EVERY: Duration = Duration::from_secs(2);

pub(crate) struct OcrRun {
    /// Dev hook: save here when the run finishes.
    save_to: Option<std::path::PathBuf>,
    handle: JobHandle,
    _task: Task<()>,
    /// The run's group, and the version its pages were rendered from.
    group: GroupId,
    base: Snapshot,
    engine: String,
    total: usize,
    done: usize,
    skipped: usize,
    /// (1-based page number, error).
    failed: Vec<(usize, String)>,
    /// Pages changed since the run began, whose text wasn't added.
    changed: usize,
    pending: Vec<(PageId, Arc<TextLayer>)>,
    /// At least one flush has been committed.
    flushed: bool,
    /// The user undid the run's results: none are added again.
    undone: bool,
    last_flush: Instant,
    started: Instant,
}

impl OcrRun {
    fn processed(&self) -> usize {
        self.done + self.skipped + self.failed.len()
    }
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
        let run = self.ocr.as_ref()?;
        Some(if run.total == 0 {
            0.0
        } else {
            (run.processed() as f32 / run.total as f32).clamp(0.0, 1.0)
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
        // The version the run begins on, whose pages it renders.
        let base = doc.model.snapshot();
        let source = ViewPages::new(doc.views.clone(), base.clone());
        let pages: Vec<OcrPage> = ids.iter().filter_map(|&id| source.page(id)).collect();
        let total = pages.len();
        let req = OcrRequest {
            pages,
            skip_pages_with_text: spec.skip_pages_with_text,
            engine: spec.engine,
            config,
        };
        let (handle, mut rx) = sheaf_ocr::job::start(req, Arc::new(source));
        let group = doc.model.begin_group(GroupKind::Ocr);
        doc.notice = Some(format!("OCR: loading {}…", spec.engine.label()));
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
        self.ocr = Some(OcrRun {
            save_to: None,
            handle,
            _task: task,
            group,
            base,
            engine: spec.engine.label().to_string(),
            total,
            done: 0,
            skipped: 0,
            failed: Vec::new(),
            changed: 0,
            pending: Vec::new(),
            flushed: false,
            undone: false,
            last_flush: Instant::now(),
            started: Instant::now(),
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

    /// Handle one job event; true means the run is over.
    fn on_ocr_event(&mut self, ev: OcrEvent, cx: &mut Context<Self>) -> bool {
        let Some(run) = &mut self.ocr else {
            return true;
        };
        let mut flush = false;
        let mut finished: Option<bool> = None;
        match ev {
            OcrEvent::Started { engine, total, .. } => {
                run.engine = engine;
                run.total = total;
            }
            OcrEvent::Skipped { .. } => run.skipped += 1,
            OcrEvent::Page { id, layer } => {
                run.done += 1;
                run.pending.push((id, Arc::new(layer)));
                flush = run.pending.len() >= FLUSH_PAGES || run.last_flush.elapsed() >= FLUSH_EVERY;
            }
            OcrEvent::Failed { id, index, error } => {
                let number = match &self.state {
                    State::Ready(doc) => doc.model.index_of(id).map(|i| i + 1),
                    _ => None,
                }
                .unwrap_or(index as usize + 1);
                log::warn!("ocr: page {number}: {error}");
                run.failed.push((number, error));
            }
            OcrEvent::Finished { cancelled, .. } => {
                flush = true;
                finished = Some(cancelled);
            }
            OcrEvent::Aborted(msg) => {
                log::error!("ocr: {msg}");
                if let (Some(run), State::Ready(doc)) = (self.ocr.take(), &mut self.state) {
                    doc.model.end_group(run.group);
                }
                self.set_notice(format!("OCR failed: {msg}"), cx);
                return true;
            }
        }
        if flush {
            self.flush_ocr(cx);
        }
        match finished {
            Some(cancelled) => {
                let run = self.ocr.take().expect("run exists");
                if let State::Ready(doc) = &mut self.state {
                    doc.model.end_group(run.group);
                }
                let s = |n: usize| if n == 1 { "" } else { "s" };
                let mut msg = format!(
                    "OCR {}: {} page{} recognized",
                    if cancelled { "stopped" } else { "done" },
                    run.done,
                    s(run.done)
                );
                if run.skipped > 0 {
                    msg.push_str(&format!(", {} skipped (had text)", run.skipped));
                }
                if run.changed > 0 {
                    msg.push_str(&format!(
                        ", {} changed during the run (their text wasn't added)",
                        run.changed
                    ));
                }
                if !run.failed.is_empty() {
                    let pages: Vec<String> = run
                        .failed
                        .iter()
                        .take(5)
                        .map(|(n, _)| n.to_string())
                        .collect();
                    msg.push_str(&format!(
                        ", {} failed (p. {}{})",
                        run.failed.len(),
                        pages.join(", "),
                        if run.failed.len() > 5 { ", …" } else { "" }
                    ));
                }
                msg.push_str(&format!(
                    " with {} in {:.0?}",
                    run.engine,
                    run.started.elapsed()
                ));
                if run.undone {
                    msg = "OCR stopped: its results were undone".into();
                } else if run.flushed {
                    msg.push_str(" — Ctrl+Z undoes");
                }
                if self.timing {
                    eprintln!("[timing] {msg}");
                }
                if let Some(path) = run.save_to {
                    eprintln!("[selftest] {msg}; saving to {}", path.display());
                    self.save_to(path, cx).detach();
                } else {
                    self.set_notice(msg, cx);
                }
                true
            }
            // Stopping: the notice says why.
            None if self.ocr.as_ref().is_some_and(|run| run.undone) => false,
            None => {
                let run = self.ocr.as_ref().expect("run exists");
                let text = format!(
                    "OCR {}/{} — {} skipped — {} — Esc stops",
                    run.processed(),
                    run.total,
                    run.skipped,
                    run.engine
                );
                self.set_notice(text, cx);
                false
            }
        }
    }

    /// Commit the pending layers in the run's group: part of its one undo
    /// step, or another fragment of it after the user's edits. When some
    /// page changed since the run began, the layers go one by one, and the
    /// changed pages' are left out. The view follows each commit
    /// ([`DocView::on_change`]), made on the model rather than through
    /// [`DocView::commit`], whose notice would replace the run's own and
    /// tell each page left out as an error. When the user undid the run,
    /// it stops.
    fn flush_ocr(&mut self, cx: &mut Context<Self>) {
        let Some(run) = &mut self.ocr else {
            return;
        };
        let pending = std::mem::take(&mut run.pending);
        if pending.is_empty() || run.undone {
            return;
        }
        let State::Ready(doc) = &mut self.state else {
            return;
        };
        let mut changes = Vec::new();
        let mut landed = flush(&mut doc.model, run, &pending).map(|ch| changes.push(ch));
        if let Err(EditError::Conflict { .. }) = landed {
            landed = page_by_page(&mut doc.model, run, &pending, &mut changes);
        }
        if changes.iter().any(|ch| ch.from != ch.to) {
            run.flushed = true;
            run.last_flush = Instant::now();
        }
        match landed {
            Ok(()) => {}
            Err(EditError::GroupEnded) => {
                // The user undid the run: don't put the results back.
                run.undone = true;
                run.handle.cancel();
                doc.notice = Some("OCR stopped: its results were undone".into());
            }
            Err(e) => {
                log::error!("ocr: applying results: {e:#}");
                doc.notice = Some(crate::docview::failed("OCR: applying results", &e));
            }
        }
        for ch in &changes {
            self.on_change(ch, cx);
        }
        cx.notify();
    }
}

/// `layers` committed in `run`'s group, made on the version the run began
/// on and deciding by their pages as OCR read them (their own content, what
/// each shows), so a page changed since is a Conflict: the change.
fn flush(
    model: &mut Document,
    run: &OcrRun,
    layers: &[(PageId, Arc<TextLayer>)],
) -> Result<ChangeSet, EditError> {
    let mut tx = Document::tx_at(&run.base, model.lower_cx(Actor::Job(JobKind::Ocr)));
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
        base: Base::At(run.base.version),
        stale: Stale::Validate,
        group: Some(run.group),
        ..CommitOpts::labelled("Recognize text")
    };
    Ok(model.commit(tx, opts)?.change)
}

/// `layers` flushed one by one ([`flush`]), each change onto `changes`: a
/// page changed since the run began keeps no text, and is counted.
fn page_by_page(
    model: &mut Document,
    run: &mut OcrRun,
    layers: &[(PageId, Arc<TextLayer>)],
    changes: &mut Vec<ChangeSet>,
) -> Result<(), EditError> {
    for layer in layers {
        match flush(model, run, std::slice::from_ref(layer)) {
            Ok(change) => changes.push(change),
            Err(EditError::Conflict { .. }) => run.changed += 1,
            Err(e) => return Err(e),
        }
    }
    Ok(())
}
