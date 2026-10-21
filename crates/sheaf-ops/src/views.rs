//! A document's pages drawn (M10.5 §10): each page's view from the
//! document's realizer, rendered by the process's engine pool.
//!
//! [`Views::render`] finds where a page renders from under the caller's
//! inputs, turns the job by the page's turn from its engine's page, draws
//! what the inputs draw, and hands the pool a way to open the engine: a
//! source's bytes, or the realizer's overlay of the state the page was
//! viewed in; [`Views::draw`] draws a view the caller holds, by a job as
//! its engine draws the page. Text is read from a source's base engine by
//! page index ([`Views::base`]) until slice 14's text service.
//!
//! A source whose bytes PDFium refuses has no base engine: the pool tells
//! the source when an engine over its bytes fails to open so (its base
//! engine, its view patch, or a group's section over them;
//! [`Source::refuse_base`](sheaf_doc::source::Source::refuse_base)), and
//! from then on the realizer draws every page of it from its groups, files
//! of their own. A render already asked of such an engine renders from the
//! page's view found again, its group's (the request's fallback), so no
//! consumer's render fails for it (Greg, 2026-10-20). Its text, read by
//! index from the base engine, waits for slice 14.
//!
//! [`ViewPages`] is a snapshot's pages as OCR reads them: each drawn with
//! its content alone, in the background's budget.

use std::sync::Arc;

use anyhow::{Context as _, Result};
use futures::channel::oneshot;
use futures::executor::block_on;
use sheaf_doc::realize::{EngineKey, PageView, Realizer, ViewInputs};
use sheaf_doc::{DocState, PageId, Snapshot, Src};
use sheaf_edit::Rotation;
use sheaf_image::raster::{PixelFormat, Raster};
use sheaf_ocr::{OcrPage, PageSource};
use sheaf_render::{
    Budget, Draw, EnginePool, Fallback, Job, OpenError, Opener, PageIndex, Rendered, Req, Target,
};

/// One document's views, drawn by the process's pool.
#[derive(Clone)]
pub struct Views {
    realizer: Arc<Realizer>,
    pool: EnginePool<EngineKey>,
}

impl Views {
    /// A document's views, drawn by `pool`.
    pub fn new(pool: EnginePool<EngineKey>) -> Views {
        Views {
            realizer: Arc::new(Realizer::new()),
            pool,
        }
    }

    pub fn pool(&self) -> &EnginePool<EngineKey> {
        &self.pool
    }

    pub fn realizer(&self) -> &Arc<Realizer> {
        &self.realizer
    }

    /// Where page `page` of `st` renders from under `inputs`.
    pub fn view(&self, st: &DocState, page: PageId, inputs: &ViewInputs) -> Result<PageView> {
        self.realizer.view(st, page, inputs)
    }

    /// Page `page` of `st` drawn under `inputs`, by `job` as its pane shows
    /// it (turned by the pane's rotation), in `budget`. The rendered
    /// request is the engine's: the page's index there, and the job turned
    /// by the page's view too.
    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &self,
        st: &DocState,
        page: PageId,
        inputs: &ViewInputs,
        job: Job,
        format: PixelFormat,
        priority: u8,
        budget: Budget,
    ) -> oneshot::Receiver<Result<Rendered>> {
        let drawn = (self.view(st, page, inputs)).and_then(|view| {
            let job = job.turned(view.job.rotation);
            self.draw(st, page, &view, inputs, job, format, priority, budget)
        });
        drawn.unwrap_or_else(|e| {
            let (tx, rx) = oneshot::channel();
            let _ = tx.send(Err(e));
            rx
        })
    }

    /// Page `page` of `st`, viewed under `inputs` as `view`, drawn by `job`
    /// as its engine draws it: from the engine's page, so the view's turn
    /// is the caller's to add (the screen's jobs have it; a plugin's render
    /// in its file's own orientation hasn't). In `budget`; an error when
    /// `st` hasn't the page.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &self,
        st: &DocState,
        page: PageId,
        view: &PageView,
        inputs: &ViewInputs,
        job: Job,
        format: PixelFormat,
        priority: u8,
        budget: Budget,
    ) -> Result<oneshot::Receiver<Result<Rendered>>> {
        let target = self.target(st, page, view, inputs, budget)?;
        Ok(self.pool.render(Req {
            target,
            page: PageIndex(view.index),
            job,
            draw: Draw {
                annots: view.annots,
                forms: view.forms,
            },
            format,
            priority,
            fallback: self.fallback(st, page, view, inputs, budget),
        }))
    }

    /// For a page viewed on an engine over its source's bytes
    /// ([`over_source`]), where it renders from should PDFium refuse them:
    /// its view found again, once the pool has told the source, which is
    /// its group's. The job is the same, a view's turn being the page's
    /// from its base page either way.
    fn fallback(
        &self,
        st: &DocState,
        page: PageId,
        view: &PageView,
        inputs: &ViewInputs,
        budget: Budget,
    ) -> Option<Fallback<EngineKey>> {
        if !over_source(&st.def(page)?.base.src, view.engine) {
            return None;
        }
        let (views, st, inputs) = (self.clone(), st.clone(), inputs.clone());
        Some(Box::new(move || {
            let view = views.view(&st, page, &inputs)?;
            let target = views.target(&st, page, &view, &inputs, budget)?;
            Ok((target, PageIndex(view.index)))
        }))
    }

    /// The engine page `page` of `st`, viewed under `inputs` as `view`,
    /// renders from, opened in `budget`: its source's base engine, or the
    /// realizer's overlay of `st`, which the pool opens as `st` has it
    /// whenever it needs to. A caller that keeps a page's view, as the
    /// screen does for the key its images are found by, asks for its
    /// engine with it. An overlay over the source's bytes tells the source
    /// when PDFium refuses it, as its base engine does; one viewed before
    /// the source was refused, which the page no longer views, fails as
    /// refused without opening, so its render's fallback finds the page's
    /// view again.
    pub fn target(
        &self,
        st: &DocState,
        page: PageId,
        view: &PageView,
        inputs: &ViewInputs,
        budget: Budget,
    ) -> Result<Target<EngineKey>> {
        let def = st.def(page).context("a page of the document")?;
        let src = &def.base.src;
        Ok(match view.engine {
            EngineKey::Base(_) => self.base(src, budget),
            engine => {
                let over = over_source(src, engine);
                let (realizer, st, inputs) = (self.realizer.clone(), st.clone(), inputs.clone());
                let told = src.downgrade();
                let open = move || {
                    let refused = Src::upgrade(&told).is_some_and(|src| src.base_refused());
                    if over && refused && realizer.view(&st, page, &inputs)?.engine != engine {
                        let why = "refused since the page's view".into();
                        return Err(OpenError::Refused(why).into());
                    }
                    let r = realizer.open(&st, engine, &inputs)?;
                    Ok((r.segments, r.password))
                };
                let weak = src.downgrade();
                let open = Opener::overlay(open, move || weak.strong_count() > 0);
                Target {
                    engine,
                    open: if over {
                        open.on_refused(refuse(src))
                    } else {
                        open
                    },
                    budget,
                }
            }
        })
    }

    /// Source `src`'s base engine, opened in `budget`: what its pages
    /// render from untouched, and what their text is read from by index.
    /// It doesn't keep the source, and tells it when PDFium refuses its
    /// bytes.
    pub fn base(&self, src: &Src, budget: Budget) -> Target<EngineKey> {
        let (weak, probe) = (src.downgrade(), src.downgrade());
        let open = move || {
            let src = Src::upgrade(&weak).context("the source is gone")?;
            Ok((vec![src.bytes().clone()], src.password().map(String::from)))
        };
        Target {
            engine: EngineKey::Base(src.id()),
            open: Opener::base(open, move || probe.strong_count() > 0).on_refused(refuse(src)),
            budget,
        }
    }
}

/// Whether `engine`, a view's of a page of `src`, may read `src`'s bytes:
/// its base engine does, and so does an overlay while `src` is sound (its
/// view patch, or a group's section over them, until PDFium has refused
/// them). A damaged base's groups are files of their own.
fn over_source(src: &Src, engine: EngineKey) -> bool {
    matches!(engine, EngineKey::Base(_)) || src.base_sound()
}

/// Tells `src`, while it lives, that PDFium refused its bytes.
fn refuse(src: &Src) -> impl Fn() + Send + Sync + 'static {
    let weak = src.downgrade();
    move || {
        if let Some(src) = Src::upgrade(&weak) {
            src.refuse_base();
        }
    }
}

/// A snapshot's pages as OCR reads them (M10.5 §10.4): each realized under
/// [`ViewInputs::content_only`], so an unsaved header, a widget or a
/// comment is never read as the page's content, and drawn in the
/// background's budget, which never closes an engine the screen shows. A
/// page's text is its source page's, read from its base engine, until
/// slice 14's text service.
pub struct ViewPages {
    views: Views,
    snap: Snapshot,
    inputs: ViewInputs,
}

impl ViewPages {
    /// The pages of `snap`, drawn by `views`.
    pub fn new(views: Views, snap: Snapshot) -> ViewPages {
        ViewPages {
            views,
            snap,
            inputs: ViewInputs::content_only(),
        }
    }

    /// Page `id`, to recognize as it shows: None for a page the snapshot
    /// hasn't got.
    pub fn page(&self, id: PageId) -> Option<OcrPage> {
        let base = &self.snap.state.def(id)?.base;
        Some(OcrPage {
            id,
            index: base.index,
            geom: self.snap.geom(id),
        })
    }
}

impl PageSource for ViewPages {
    fn text_len(&self, page: &OcrPage) -> Result<usize> {
        let def = self.snap.state.def(page.id);
        let base = &def.context("a page of the document")?.base;
        let target = self.views.base(&base.src, Budget::Background);
        let rx = (self.views.pool()).page_text_len(target, PageIndex(base.index));
        block_on(rx).context("the render thread is gone")?
    }

    /// The page's view turns the job from its engine's page to the page's
    /// own rotation, as it shows.
    fn render(&self, page: &OcrPage, width_px: u32) -> Result<Raster> {
        let job = Job::Page {
            width: width_px,
            rotation: Rotation::R0,
        };
        let (st, format, budget) = (&self.snap.state, PixelFormat::Rgba, Budget::Background);
        let rx = (self.views).render(st, page.id, &self.inputs, job, format, u8::MAX, budget);
        Ok(block_on(rx).context("the render thread is gone")??.raster)
    }
}
