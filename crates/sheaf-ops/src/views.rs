//! A document's pages drawn (M10.5 §10): each page's view from the
//! document's realizer, rendered by the process's engine pool, or on the
//! command line by one of their own ([`Views::start`]).
//!
//! [`Views::render`] finds where a page renders from under the caller's
//! inputs, turns the job by the page's turn from its engine's page, draws
//! what the inputs draw, and hands the pool a way to open the engine: a
//! source's bytes, or the realizer's overlay of the state the page was
//! viewed in; [`Views::draw`] draws a view the caller holds, by a job as
//! its engine draws the page. A page's text is read from the same engine
//! and page ([`Views::locate`]; [`crate::text`]), so the screen, the file
//! and the text agree.
//!
//! A source whose bytes PDFium refuses has no base engine: the pool tells
//! the source when an engine over its bytes fails to open so (its base
//! engine, its view patch, or a group's section over them;
//! [`Source::refuse_base`](sheaf_doc::source::Source::refuse_base)), and
//! from then on the realizer draws every page of it from its groups, files
//! of their own. A render or a text read already asked of such an engine
//! reads the page from its view found again, its group's (the request's
//! fallback), so no consumer's render or text fails for it (Greg,
//! 2026-10-20).
//!
//! [`ViewPages`] is a snapshot's pages as OCR reads them: each drawn, and
//! its text read, with its content alone, in the background's budget.

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

use crate::text::Text;

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

    /// A document's views, drawn by a pool of their own: the command
    /// line's, which has no process's pool to share, or a test's.
    pub fn start() -> Result<Views> {
        Ok(Views::new(EnginePool::new()?))
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
        drawn.unwrap_or_else(|e| answered(Err(e)))
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
        let (target, index, fallback) = self.locate(st, page, view, inputs, budget)?;
        Ok(self.pool.render(Req {
            target,
            page: index,
            job,
            draw: Draw {
                annots: view.annots,
                forms: view.forms,
            },
            format,
            priority,
            fallback,
        }))
    }

    /// Where page `page` of `st`, viewed under `inputs` as `view`, is read
    /// from in `budget`, by renders and text alike: its engine (its
    /// source's base engine, or the realizer's overlay of `st`, which the
    /// pool opens as `st` has it whenever it needs to); its index there;
    /// and, for a page viewed on an engine over its source's bytes, where
    /// else it is should PDFium refuse them, its view found again. An error
    /// when `st` hasn't the page.
    pub fn locate(
        &self,
        st: &DocState,
        page: PageId,
        view: &PageView,
        inputs: &ViewInputs,
        budget: Budget,
    ) -> Result<(Target<EngineKey>, PageIndex, Option<Fallback<EngineKey>>)> {
        let target = self.target(st, page, view, inputs, budget)?;
        let fallback = self.fallback(st, page, view, inputs, budget);
        Ok((target, PageIndex(view.index), fallback))
    }

    /// For a page viewed on an engine over its source's bytes
    /// ([`over_source`]), where it's read from should PDFium refuse them:
    /// its view found again, once the pool has told the source, which is
    /// its group's. A render's job is the same, a view's turn being the
    /// page's from its base page either way.
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
    /// whenever it needs to. An overlay over the source's bytes tells the
    /// source when PDFium refuses it, as its base engine does; one viewed
    /// before the source was refused, which the page no longer views,
    /// fails as refused without opening, so its request's fallback finds
    /// the page's view again.
    fn target(
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

    /// Source `src`'s base engine, opened in `budget`: what its untouched
    /// pages render from and their text is read from, and what a document
    /// asks of its file (its form). It doesn't keep the source, and tells
    /// it when PDFium refuses its bytes.
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

/// A receiver answered already, with `done`.
pub(crate) fn answered<T>(done: Result<T>) -> oneshot::Receiver<Result<T>> {
    let (tx, rx) = oneshot::channel();
    let _ = tx.send(done);
    rx
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
/// [`ViewInputs::content_only`], so a header or footer sheaf draws, saved
/// or not, a widget or a comment is never read as the page's content,
/// drawn and its text read there ([`Text`]) in the background's budget,
/// which never closes an engine the screen shows. Its text is read with
/// the recognized text on its stack too, as blank detection reads it, so
/// the page's own recognized text counts as text whether it's saved or
/// not.
pub struct ViewPages {
    views: Views,
    text: Text,
    snap: Snapshot,
    inputs: ViewInputs,
    read: ViewInputs,
}

impl ViewPages {
    /// The pages of `snap`, drawn by `views`.
    pub fn new(views: Views, snap: Snapshot) -> ViewPages {
        let inputs = ViewInputs::content_only();
        ViewPages {
            text: Text::new(views.clone(), Budget::Background),
            views,
            snap,
            read: ViewInputs {
                ocr_over: true,
                ..inputs.clone()
            },
            inputs,
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
        let rx = self.text.chars(&self.snap.state, page.id, &self.read);
        let chars = block_on(rx).context("the render thread is gone")??;
        Ok(chars.text_len())
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
