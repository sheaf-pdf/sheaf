//! Background work on a document, its progress in the status bar and
//! Escape to stop it (M10.5 §14.1), in two kinds:
//! - **Jobs** write files (Export Pages, Split Document), one at a time
//!   ([`DocView::run_job`]).
//! - **Planners** plan an edit off the UI thread, over a snapshot (an
//!   insert, Trim White, a watermark, a header and footer, a comment
//!   import, the blank-page search), and land it on the UI thread,
//!   committed with the snapshot's version as its base
//!   ([`DocView::run_planner`]). Any number run at once, beside a job and
//!   OCR's own run; Escape stops the newest.

use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context as _;
use gpui::Context;
use sheaf_doc::{PageId, Store};
use sheaf_ops::views::Views;
use sheaf_ops::{ProgressSnapshot as Snapshot, Tracker, was_cancelled};

use crate::docview::{DocView, State};

/// A running job or planner: its name ("Export") and tracker.
pub(crate) struct Job {
    pub name: String,
    pub tracker: Tracker,
}

impl DocView {
    /// Run `work` on a background thread. `status` turns progress into the
    /// status line; `done` the result into the final one. False when a job
    /// is already running.
    pub(crate) fn run_job<T: Send + 'static>(
        &mut self,
        name: &str,
        work: impl FnOnce(&Tracker) -> anyhow::Result<T> + Send + 'static,
        status: impl Fn(&Snapshot) -> Option<String> + 'static,
        done: impl FnOnce(T) -> String + 'static,
        cx: &mut Context<Self>,
    ) -> bool {
        if let Some(job) = &self.job {
            let msg = format!("{} is still running — Esc stops it", job.name);
            self.set_notice(msg, cx);
            return false;
        }
        let tracker = Tracker::new();
        self.job = Some(Job {
            name: name.to_string(),
            tracker: tracker.clone(),
        });
        self.set_notice(format!("{name}…"), cx);
        let name = name.to_string();
        let end = move |v: &mut DocView, result: anyhow::Result<T>, _: &mut Context<DocView>| {
            v.job = None;
            Some(match result {
                Ok(out) => done(out),
                Err(e) => ended(&name, &e),
            })
        };
        self.drive(tracker, work, status, end, cx);
        true
    }

    /// Plan an edit: `work` on a background thread, over what it took of
    /// the document, then `land` given the view to commit what it planned,
    /// returning the final status line, if any. `status` turns progress
    /// into the status line meanwhile. A planner stopped before it lands
    /// lands nothing, and so does one whose document the view no longer
    /// shows ([`Self::forget_planners`]).
    pub(crate) fn run_planner<T: Send + 'static>(
        &mut self,
        name: &str,
        work: impl FnOnce(&Tracker) -> anyhow::Result<T> + Send + 'static,
        status: impl Fn(&Snapshot) -> Option<String> + 'static,
        land: impl FnOnce(&mut DocView, T, &mut Context<DocView>) -> Option<String> + 'static,
        cx: &mut Context<Self>,
    ) {
        let tracker = Tracker::new();
        let planner = Rc::new(Job {
            name: name.to_string(),
            tracker: tracker.clone(),
        });
        self.planners.push(planner.clone());
        self.set_notice(format!("{name}…"), cx);
        let end = move |v: &mut DocView, result: anyhow::Result<T>, cx: &mut Context<DocView>| {
            let at = v.planners.iter().position(|p| Rc::ptr_eq(p, &planner))?;
            v.planners.remove(at);
            match result {
                // Stopped once its work was done, before it landed.
                Ok(_) if planner.tracker.snapshot().cancelled => {
                    Some(format!("{} stopped", planner.name))
                }
                Ok(out) => land(v, out, cx),
                Err(e) => Some(ended(&planner.name, &e)),
            }
        };
        self.drive(tracker, work, status, end, cx);
    }

    /// `work` run on a background thread with `tracker`, `status` turning
    /// its progress into the status line meanwhile; then `end` given the
    /// view and the result, returning the final status line, if any.
    fn drive<T: Send + 'static>(
        &mut self,
        tracker: Tracker,
        work: impl FnOnce(&Tracker) -> anyhow::Result<T> + Send + 'static,
        status: impl Fn(&Snapshot) -> Option<String> + 'static,
        end: impl FnOnce(&mut DocView, anyhow::Result<T>, &mut Context<DocView>) -> Option<String>
        + 'static,
        cx: &mut Context<Self>,
    ) {
        use futures::FutureExt;
        let progress = tracker.clone();
        let work = cx
            .background_executor()
            .spawn(async move { work(&progress) });
        cx.spawn(async move |this, cx| {
            let mut work = Box::pin(work.fuse());
            let result = loop {
                let mut tick = cx
                    .background_executor()
                    .timer(Duration::from_millis(100))
                    .fuse();
                futures::select_biased! {
                    r = work => break r,
                    _ = tick => {}
                }
                let snap = tracker.snapshot();
                let alive = this.update(cx, |v, cx| {
                    if !snap.cancelled
                        && let Some(line) = status(&snap)
                        && let State::Ready(doc) = &mut v.state
                    {
                        doc.notice = Some(line);
                    }
                    cx.notify();
                });
                if alive.is_err() {
                    // The view closed: stop the work too.
                    tracker.cancel();
                }
            };
            this.update(cx, |v, cx| {
                let line = end(v, result, cx);
                if let (Some(line), State::Ready(doc)) = (line, &mut v.state) {
                    doc.notice = Some(line);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Fraction done of the running job, else of the newest planner, while
    /// one runs.
    pub(crate) fn job_progress(&self) -> Option<f32> {
        let tracker = match (&self.job, self.planners.last()) {
            (Some(job), _) => &job.tracker,
            (None, Some(planner)) => &planner.tracker,
            (None, None) => return None,
        };
        Some(tracker.snapshot().fraction.unwrap_or(0.0))
    }

    /// Stop the newest planner not stopped yet. Returns whether there was
    /// one.
    pub(crate) fn cancel_planner(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(planner) = (self.planners.iter().rev()).find(|p| !p.tracker.snapshot().cancelled)
        else {
            return false;
        };
        planner.tracker.cancel();
        let msg = format!("{}: stopping…", planner.name);
        self.set_notice(msg, cx);
        true
    }

    /// The planners stopped and forgotten, and the blank pages a search
    /// found for review, as the view goes to show another document (a
    /// recovery's): what they planned was of this one.
    pub(crate) fn forget_planners(&mut self) {
        for planner in self.planners.drain(..) {
            planner.tracker.cancel();
        }
        self.blank_review = None;
    }

    /// Stop the running job. Returns whether there was one.
    pub(crate) fn cancel_job(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(job) = &self.job else {
            return false;
        };
        job.tracker.cancel();
        let msg = format!("{}: stopping…", job.name);
        self.set_notice(msg, cx);
        true
    }

    /// Ask the running plugin command to stop (it does at its next check).
    /// Returns whether one was running and had reported progress.
    pub(crate) fn cancel_plugin_run(&mut self, cx: &mut Context<Self>) -> bool {
        let Some((label, t)) = &self.plugin_run else {
            return false;
        };
        let s = t.snapshot();
        if s.total.is_none() && s.fraction.is_none() {
            return false;
        }
        t.cancel();
        let msg = format!("{label}: stopping…");
        self.set_notice(msg, cx);
        true
    }
}

impl DocView {
    /// Document > Split Document: ask for a folder, confirm replacing
    /// files already there, then split as a job.
    pub(crate) fn split_document(
        &mut self,
        by: sheaf_ops::split::SplitBy,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        if self.still_opening(cx) {
            return;
        }
        let State::Ready(doc) = &self.state else {
            return;
        };
        let snapshot = doc.model.snapshot();
        let stem = sheaf_ops::export::stem_of(&self.path);
        let rx = crate::file_dialog::open(
            cx,
            gpui::PathPromptOptions {
                files: false,
                directories: true,
                multiple: false,
                prompt: Some("Split Here".into()),
            },
        );
        let window_handle = window.window_handle();
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(mut dirs))) = rx.await else {
                return;
            };
            let Some(dir) = dirs.pop() else { return };
            // Names are known ahead except for a size split.
            let clash: Vec<std::path::PathBuf> = sheaf_ops::split::plan(&snapshot, &by, &stem)
                .map(|parts| {
                    parts
                        .iter()
                        .map(|p| dir.join(format!("{}.pdf", p.name)))
                        .filter(|p| p.exists())
                        .collect()
                })
                .unwrap_or_default();
            let mut force = false;
            if !clash.is_empty() {
                let answer = window_handle
                    .update(cx, |_, window, cx| {
                        sheaf_ui::prompts::ask(
                            window,
                            cx,
                            gpui::PromptLevel::Warning,
                            &format!(
                                "Replace {} existing file{}?",
                                clash.len(),
                                if clash.len() == 1 { "" } else { "s" }
                            ),
                            Some(&format!("{} already has them.", dir.display())),
                            &[
                                gpui::PromptButton::ok("Replace"),
                                gpui::PromptButton::cancel("Cancel"),
                            ],
                        )
                    })
                    .ok();
                let Some(rx) = answer else { return };
                if rx.await != Ok(0) {
                    return;
                }
                force = true;
            }
            this.update(cx, |v, cx| {
                let shown = dir.clone();
                v.run_job(
                    "Split",
                    move |p| sheaf_ops::split::split(&snapshot, &by, &dir, &stem, force, p),
                    |s| {
                        Some(match s.step.as_str() {
                            "measuring" => format!("Measuring: page {} of {}…", s.done, s.total?),
                            _ => format!("Writing file {} of {}…", s.done + 1, s.total?),
                        })
                    },
                    move |files| {
                        format!(
                            "Split into {} file{} in {}",
                            files.len(),
                            if files.len() == 1 { "" } else { "s" },
                            shown.display()
                        )
                    },
                    cx,
                );
            })
            .ok();
        })
        .detach();
    }
}

/// The final status line of a job or planner `name` that ended with `e`:
/// stopped, or failed and why.
fn ended(name: &str, e: &anyhow::Error) -> String {
    match was_cancelled(e) {
        true => format!("{name} stopped"),
        false => format!("{name} failed: {e:#}"),
    }
}

impl DocView {
    /// The document as it is now, for dialogs that plan over it.
    pub(crate) fn model_snapshot(&self) -> Option<sheaf_doc::Snapshot> {
        match &self.state {
            State::Ready(doc) => Some(doc.model.snapshot()),
            _ => None,
        }
    }

    /// [`Self::model_snapshot`], with a lowering context of the user's on
    /// it: what a dialog's preview lowers on.
    pub(crate) fn lowering_now(
        &self,
    ) -> Option<(sheaf_doc::Snapshot, std::sync::Arc<sheaf_doc::tx::LowerCx>)> {
        match &self.state {
            State::Ready(doc) => {
                let lower = doc.model.lower_cx(sheaf_doc::Actor::User);
                Some((doc.model.snapshot(), lower))
            }
            _ => None,
        }
    }

    /// What a dialog draws its preview of the document with.
    pub(crate) fn previewer(&self) -> Option<Previewer> {
        match &self.state {
            State::Ready(doc) => Some(Previewer {
                store: doc.model.store().clone(),
                views: Views::new(doc.views.pool().clone()),
                page: doc.page_id(doc.current_page())?,
            }),
            _ => None,
        }
    }
}

/// What a dialog draws its preview of the document with (M10.5 §8.6):
/// the document's store, which a copy the preview makes goes into; the
/// document's render worker, with a realizer of the dialog's own, since a
/// preview's state is never the screen's; and the page it shows, the
/// active pane's as it opened.
#[derive(Clone)]
pub(crate) struct Previewer {
    pub(crate) store: Arc<Store>,
    pub(crate) views: Views,
    pub(crate) page: PageId,
}

impl Previewer {
    /// The page it shows of `now`: its own, or the first, once `now`
    /// hasn't it.
    pub(crate) fn page_in(&self, now: &sheaf_doc::Snapshot) -> anyhow::Result<PageId> {
        match now.state.def(self.page) {
            Some(_) => Ok(self.page),
            None => (now.state.page_at(0)).context("the document has no pages"),
        }
    }
}
