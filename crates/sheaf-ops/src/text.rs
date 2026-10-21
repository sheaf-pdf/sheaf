//! A document's text (M10.5 §10.5): each page's characters read from what
//! it draws under the caller's inputs, by the engine and page its views
//! render it from ([`Views::locate`]), so the screen, the file and the
//! text agree. An untouched page's engine is its source's base engine; an
//! edited, duplicated or cropped page's is its group's realization, which
//! draws an unsaved header or recognized text where the inputs do. A page
//! whose source PDFium refuses is read from its group, as it's drawn
//! (the query's fallback).
//!
//! A page's characters are read on the pool's worker, boxes in PageSpace
//! (§5), and kept by the page's render key, the last [`CHARS_KEPT`] pages
//! read: a page that draws the same under the same inputs, in this state
//! or another, its duplicate too, is read once. Its key changes with
//! anything it draws, so an edit to it reads it again, and §10.5's cache
//! of untouched pages by source and index is this one: an untouched page's
//! key is its base page's.
//!
//! Find and Patterns walk a document's pages in its order on the worker,
//! a few between renders ([`Text::search`]), each read where its views
//! read it, so they hit what every page draws: an inserted file's, a
//! duplicate, a crop, recognized text and a header not yet saved, and a
//! source PDFium refuses through its groups. A walk keeps none of what it
//! reads.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, PoisonError};

use anyhow::{Context as _, Result};
use futures::channel::{mpsc, oneshot};
use futures::executor::block_on;
use sheaf_doc::realize::ViewInputs;
use sheaf_doc::{
    DocState, PageChars, PageId, Query, RenderKey, Snapshot, TextHit, TextOpts, TextService,
};
use sheaf_render::pdfium::PdfiumEngine;
use sheaf_render::{Budget, PageIndex, Renderer, Walked};

use crate::views::{Views, answered};

/// The pages whose characters a document's text keeps, the oldest read
/// let go first: a few screens' worth of the pages being read.
pub const CHARS_KEPT: usize = 64;

/// One document's text, read through its views in one budget: the
/// screen's for selection and Find, the background's for a scan of every
/// page. Cheap to clone; clones share what's kept.
#[derive(Clone)]
pub struct Text {
    views: Views,
    budget: Budget,
    kept: Arc<Mutex<Kept>>,
}

/// What a search tells as it walks a document's pages ([`Text::search`]),
/// in their order.
#[derive(Debug)]
pub enum Found {
    /// The matches on the page at `position` in the state searched.
    Hits {
        position: usize,
        page: PageId,
        hits: Vec<TextHit>,
    },
    /// The pages at the first `n` positions are searched.
    Scanned(usize),
    /// Every page is searched.
    Done,
}

impl From<Walked> for Found {
    fn from(walked: Walked) -> Found {
        match walked {
            Walked::Scanned(n) => Found::Scanned(n),
            Walked::Done => Found::Done,
        }
    }
}

/// Pages' characters by render key, in the order they were read.
#[derive(Default)]
struct Kept {
    by: HashMap<RenderKey, Arc<PageChars>>,
    order: VecDeque<RenderKey>,
}

impl Kept {
    fn keep(&mut self, key: RenderKey, chars: Arc<PageChars>) {
        if self.by.insert(key, chars).is_some() {
            return;
        }
        self.order.push_back(key);
        while self.order.len() > CHARS_KEPT {
            if let Some(old) = self.order.pop_front() {
                self.by.remove(&old);
            }
        }
    }
}

impl Text {
    /// The text of `views`' document, its engines opened in `budget`.
    pub fn new(views: Views, budget: Budget) -> Text {
        Text {
            views,
            budget,
            kept: Arc::default(),
        }
    }

    /// Page `page` of `st`'s characters as it draws under `inputs`: at
    /// once when they're kept, or once the pool's worker has read them. An
    /// error for a page `st` hasn't got, or one no engine reads.
    pub fn chars(
        &self,
        st: &DocState,
        page: PageId,
        inputs: &ViewInputs,
    ) -> oneshot::Receiver<Result<Arc<PageChars>>> {
        if let Some(chars) = self.cached(st, page, inputs) {
            return answered(Ok(chars));
        }
        let asked = (self.views.view(st, page, inputs)).and_then(|view| {
            let (target, index, fallback) =
                (self.views).locate(st, page, &view, inputs, self.budget)?;
            let kept = self.kept.clone();
            let read = move |engine: &PdfiumEngine, at| {
                let chars = Arc::new(read(engine, at)?);
                lock(&kept).keep(view.key, chars.clone());
                Ok(chars)
            };
            Ok(self.views.pool().query(target, index, fallback, read))
        });
        asked.unwrap_or_else(|e| answered(Err(e)))
    }

    /// Page `page` of `st`'s characters as it draws under `inputs`, when
    /// they're kept: for a caller that can't wait, such as a paint.
    pub fn cached(
        &self,
        st: &DocState,
        page: PageId,
        inputs: &ViewInputs,
    ) -> Option<Arc<PageChars>> {
        let key = sheaf_doc::render_key(st, page, inputs)?;
        lock(&self.kept).by.get(&key).cloned()
    }

    /// Every match of `q` on `st`'s pages as they draw under `inputs`, page
    /// by page in `st`'s order: each page viewed as the walk reaches it on
    /// the pool's worker, its characters read from its view's engine (its
    /// group's, should PDFium refuse its source) and searched there, a few
    /// pages between renders. The pages that match are told with their
    /// positions, then how far the walk has got, until it's done. The walk
    /// holds `st` until it's done or the receiver is dropped, which ends
    /// it.
    pub fn search(
        &self,
        st: &DocState,
        q: Query,
        inputs: &ViewInputs,
    ) -> mpsc::UnboundedReceiver<Found> {
        let (tx, rx) = mpsc::unbounded();
        let (order, count) = (st.order().clone(), st.order().len());
        let (views, budget) = (self.views.clone(), self.budget);
        let (st, inputs) = (st.clone(), inputs.clone());
        let item = move |at: usize| {
            let page = st.page_at(at).context("a page of the document")?;
            let view = views.view(&st, page, &inputs)?;
            views.locate(&st, page, &view, &inputs, budget)
        };
        let told = tx.clone();
        let work = move |engine: &PdfiumEngine, index, at: usize| {
            let hits = read(engine, index)?.find(&q);
            if !hits.is_empty() {
                let page = order[at];
                let _ = told.unbounded_send(Found::Hits {
                    position: at,
                    page,
                    hits,
                });
            }
            Ok(())
        };
        self.views.pool().walk(count, item, work, tx);
        rx
    }
}

/// Page `index` of `engine`'s characters, boxes in PageSpace.
fn read(engine: &PdfiumEngine, index: PageIndex) -> Result<PageChars> {
    let chars = engine.page_chars(index)?;
    Ok(PageChars::from_engine(
        chars.into_iter().map(|c| (c.ch, c.rect, c.angle)),
    ))
}

fn lock(kept: &Mutex<Kept>) -> std::sync::MutexGuard<'_, Kept> {
    kept.lock().unwrap_or_else(PoisonError::into_inner)
}

impl TextService for Text {
    /// [`Text::chars`], waited for: never on the pool's worker, which
    /// would wait for itself.
    fn chars(&self, snap: &Snapshot, page: PageId, o: &TextOpts) -> Result<Arc<PageChars>> {
        let rx = Text::chars(self, &snap.state, page, &o.inputs);
        block_on(rx).context("the render thread is gone")?
    }
}
