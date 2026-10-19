//! Document > Insert Pages > Blank Pages… and From Images…, and Replace
//! Pages…: pages with no file of their own become PDFs in the generated-
//! file cache (`sheaf_doc::generated`), then go in like any inserted file.

use std::path::PathBuf;

use gpui::Context;
use sheaf_doc::images::PageFit;
use sheaf_doc::{Actor, Derive, JobKind, PageId, Src};
use sheaf_edit::{
    BlobSel, Bring, Op, PageLabelRange, PageRef, PageSel, PageSize, PageSource, RangeSet, ScopeSel,
};
use sheaf_ops::derive::DerivedSource;
use sheaf_render::RenderHandle;

use crate::docview::{DocView, State, failed};

/// Where inserted pages go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Where {
    AfterCurrent,
    BeforeCurrent,
    Start,
    End,
}

/// A page size choice: the current page's, or a sheet.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Size {
    Current,
    Sheet(PageSize),
}

pub const LETTER: PageSize = PageSize {
    width: 612.0,
    height: 792.0,
};
pub const A4: PageSize = PageSize {
    width: 595.28,
    height: 841.89,
};
pub const LEGAL: PageSize = PageSize {
    width: 612.0,
    height: 1008.0,
};

impl DocView {
    /// The page inserted pages go before, for `at` (None: the end).
    pub(crate) fn before_for(&self, at: Where) -> Option<PageId> {
        let State::Ready(doc) = &self.state else {
            return None;
        };
        match at {
            Where::AfterCurrent => self.insert_point(),
            Where::BeforeCurrent => self.target_ids().first().copied(),
            Where::Start => doc.page_id(0),
            Where::End => None,
        }
    }

    /// The current page's size as shown.
    pub(crate) fn current_size(&self) -> PageSize {
        let State::Ready(doc) = &self.state else {
            return LETTER;
        };
        doc.page_id(doc.current_page())
            .map(|p| doc.model.page_size(p))
            .unwrap_or(LETTER)
    }

    fn size_of(&self, size: Size) -> PageSize {
        match size {
            Size::Current => self.current_size(),
            Size::Sheet(s) => s,
        }
    }

    pub(crate) fn insert_blank(
        &mut self,
        count: usize,
        size: Size,
        at: Where,
        cx: &mut Context<Self>,
    ) {
        let s = self.size_of(size);
        let made = sheaf_doc::generated::blank_pages(s.width, s.height, count)
            .and_then(|pdf| sheaf_doc::generated::store(&pdf));
        match made {
            Ok(path) => {
                let before = self.before_for(at);
                let name = if count == 1 {
                    "blank page".to_string()
                } else {
                    format!("{count} blank pages")
                };
                self.insert_file(path, before, name, false, cx);
            }
            Err(e) => self.set_notice(format!("Insert failed: {e:#}"), cx),
        }
    }

    /// Images as pages, fitted to `size` (None: each image's own size).
    pub(crate) fn insert_images(
        &mut self,
        files: Vec<PathBuf>,
        size: Option<Size>,
        at: Where,
        cx: &mut Context<Self>,
    ) {
        if self.still_opening(cx) {
            return;
        }
        let fit = match size.map(|s| self.size_of(s)) {
            Some(s) => PageFit::Paper {
                width: s.width.min(s.height),
                height: s.width.max(s.height),
                margin: 0.0,
            },
            None => PageFit::Original,
        };
        let before = self.before_for(at);
        let name = match files.as_slice() {
            [one] => one
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default(),
            many => format!("{} images", many.len()),
        };
        self.set_notice(format!("Reading {name}…"), cx);
        cx.spawn(async move |this, cx| {
            let made = cx
                .background_executor()
                .spawn(async move {
                    let pdf = sheaf_doc::images::files_to_pdf(&files, fit)?;
                    sheaf_doc::generated::store(&pdf)
                })
                .await;
            this.update(cx, |v, cx| match made {
                Ok(path) => v.insert_file(path, before, name, false, cx),
                Err(e) => v.set_notice(format!("Insert failed: {e:#}"), cx),
            })
            .ok();
        })
        .detach();
    }

    /// Replace `ids` with pages `range` (0-based) of the PDF at `path`, as
    /// one undo step. They keep their bookmarks; extra pages on either
    /// side are deleted or inserted after.
    pub(crate) fn replace_pages_with(
        &mut self,
        path: PathBuf,
        ids: Vec<PageId>,
        range: std::ops::Range<u32>,
        cx: &mut Context<Self>,
    ) {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        if self.still_opening(cx) {
            return;
        }
        let Ok(with) = RangeSet::from_runs(std::iter::once(range.start + 1..=range.end)) else {
            return self.set_notice("Replace failed: no pages to replace with", cx);
        };
        let State::Ready(doc) = &self.state else {
            return;
        };
        let store = doc.model.store().clone();
        let primary = doc.model.primary().clone();
        let pages: Vec<u32> = range.clone().collect();
        self.set_notice(format!("Replacing pages from {name}…"), cx);
        cx.spawn(async move |this, cx| {
            let opened = cx
                .background_executor()
                .spawn(async move {
                    let (src, _) = store.read_file(&path, None)?;
                    // What lowering reads of it, read here; what its marks
                    // hold, held until the replace is committed.
                    let held = sheaf_doc::tx::ready_to_insert(&store, &primary, &src, &pages);
                    let handle = RenderHandle::open_bytes(src.bytes().clone(), None)?;
                    anyhow::Ok((src, handle, held))
                })
                .await;
            this.update(cx, |v, cx| {
                let (src, handle, held) = match opened {
                    Ok(h) => h,
                    Err(e) => {
                        v.set_notice(format!("Replace failed: {e:#}"), cx);
                        return;
                    }
                };
                // A recovery may have reopened the view meanwhile: a document
                // still opening takes nothing in.
                if v.still_opening(cx) {
                    return;
                }
                let State::Ready(doc) = &mut v.state else {
                    return;
                };
                doc.handles.entry(src.id()).or_insert(handle);
                let mut tx = doc.model.tx(Actor::Job(JobKind::Insert));
                for blob in held {
                    tx.pin(blob);
                }
                tx.pin(src.blob().clone());
                let n = ids.len();
                let label = format!("Replace {n} page{}", if n == 1 { "" } else { "s" });
                let replace = Op::ReplacePages {
                    pages: ids.into_iter().map(PageRef::Id).collect(),
                    from: PageSource::File {
                        file: BlobSel::Id(src.id()),
                        name: name.clone(),
                    },
                    with: Some(with),
                    bring: Bring::default(),
                };
                let Some(commit) = v.submit(tx, vec![replace], &label, cx) else {
                    return;
                };
                if let State::Ready(doc) = &mut v.state {
                    doc.take_in(&src);
                }
                let mut notice = format!(
                    "Replaced {n} page{} with {} from {name}",
                    if n == 1 { "" } else { "s" },
                    range.len()
                );
                if let Some(told) = sheaf_doc::notice_text(&commit.notices) {
                    notice = format!("{notice}. {told}");
                }
                v.set_notice(notice, cx);
            })
            .ok();
        })
        .detach();
    }

    /// Each page of `copies` shown from the copy a derived flow made of it,
    /// as `why` says, as one version called `label`, the copies noted with
    /// the document and opened to draw: what the commit did, or None when
    /// it failed, which the notice says. `then` follows in the same
    /// version.
    fn rebase(
        &mut self,
        copies: Vec<(DerivedSource, RenderHandle)>,
        why: Derive,
        then: Vec<Op>,
        label: &str,
        cx: &mut Context<Self>,
    ) -> Option<sheaf_doc::Commit> {
        let State::Ready(doc) = &mut self.state else {
            return None;
        };
        let mut pages: Vec<(PageId, Src, u32)> = Vec::new();
        for (c, handle) in copies {
            c.note(&mut doc.model);
            doc.handles.insert(c.src.id(), handle);
            pages.extend(c.bases());
        }
        let mut tx = doc.model.tx(Actor::User);
        if !pages.is_empty()
            && let Err(e) = tx.set_page_base(pages, why)
        {
            self.set_notice(failed(label, &e), cx);
            return None;
        }
        self.submit(tx, then, label, cx)
    }
}

impl DocView {
    /// The page count, and the 0-based indexes of the selected pages (or
    /// the current one).
    pub(crate) fn target_indexes(&self) -> (usize, Vec<usize>) {
        let State::Ready(doc) = &self.state else {
            return (0, Vec::new());
        };
        let idx = self
            .target_ids()
            .iter()
            .filter_map(|id| doc.model.index_of(*id))
            .collect();
        (doc.model.page_count(), idx)
    }

    /// The ids of the pages at these 0-based indexes.
    pub(crate) fn page_ids_at(&self, indexes: &[usize]) -> Vec<PageId> {
        let State::Ready(doc) = &self.state else {
            return Vec::new();
        };
        indexes.iter().filter_map(|&i| doc.page_id(i)).collect()
    }
}

impl DocView {
    /// Document > Crop Pages: plan in the background (cropped copies are
    /// made in memory, into the document's store), then show the pages
    /// from them as one step.
    pub(crate) fn crop_pages(
        &mut self,
        pages: Vec<usize>,
        spec: sheaf_ops::crop::CropSpec,
        cx: &mut Context<Self>,
    ) {
        let ids = self.page_ids_at(&pages);
        let State::Ready(doc) = &self.state else {
            return;
        };
        if ids.is_empty() {
            return;
        }
        let snapshot = doc.model.snapshot();
        let store = doc.model.store().clone();
        let n = ids.len();
        self.set_notice(
            format!("Cropping {n} page{}…", if n == 1 { "" } else { "s" }),
            cx,
        );
        cx.spawn(async move |this, cx| {
            let planned = cx
                .background_executor()
                .spawn(async move {
                    let plan = sheaf_ops::crop::plan(
                        &store,
                        &snapshot,
                        &ids,
                        spec,
                        &sheaf_ops::NoProgress,
                    )?;
                    let mut opened = Vec::new();
                    for c in plan {
                        let handle = RenderHandle::open_bytes(c.src.bytes().clone(), None)?;
                        opened.push((c, handle));
                    }
                    anyhow::Ok(opened)
                })
                .await;
            this.update(cx, |v, cx| {
                let opened = match planned {
                    Ok(o) => o,
                    Err(e) => {
                        v.set_notice(format!("Crop failed: {e:#}"), cx);
                        return;
                    }
                };
                let label = format!("Crop {n} page{}", if n == 1 { "" } else { "s" });
                if v.rebase(opened, Derive::Crop, Vec::new(), &label, cx)
                    .is_some()
                {
                    let cropped = format!("Cropped {n} page{}", if n == 1 { "" } else { "s" });
                    v.set_notice(cropped, cx);
                }
            })
            .ok();
        })
        .detach();
    }
}

impl DocView {
    /// The current page's 0-based index.
    pub(crate) fn current_index(&self) -> usize {
        match &self.state {
            State::Ready(doc) => doc.current_page(),
            _ => 0,
        }
    }
}

impl DocView {
    /// Document > Page Labels: number pages `from..=to` with `style`,
    /// keeping the numbering after them; or remove every label.
    pub(crate) fn set_page_labels(
        &mut self,
        change: Option<(usize, usize, sheaf_doc::labels::Style, String, u32)>,
        cx: &mut Context<Self>,
    ) {
        use sheaf_doc::labels;
        let State::Ready(doc) = &mut self.state else {
            return;
        };
        let st = doc.model.state();
        let ids: Vec<PageId> = st.order().iter().copied().collect();
        let (specs, label) = match change {
            None => (Vec::new(), "Remove page labels"),
            Some((from, to, style, prefix, first)) => {
                let current = st.label_ranges().unwrap_or_default();
                let ranges = labels::apply(
                    &current,
                    ids.len() as u32,
                    from as u32,
                    to as u32,
                    style,
                    &prefix,
                    first,
                );
                (labels::to_specs(&ranges, &ids), "Page labels")
            }
        };
        let range = |r: labels::LabelSpec| PageLabelRange {
            start: r.start.into(),
            style: r.style,
            prefix: r.prefix,
            first: r.first,
        };
        let ranges = (!specs.is_empty()).then(|| specs.into_iter().map(range).collect());
        self.apply(Op::SetPageLabels { ranges }, label, cx);
    }
}

impl DocView {
    pub(crate) fn has_page_labels(&self) -> bool {
        matches!(&self.state, State::Ready(doc) if doc.model.state().labels().is_some())
    }
}

impl DocView {
    /// Document > Header & Footer: set (or with None, remove) the
    /// header/footer. Pages whose file already has one drawn are moved onto
    /// copies without it first, in the same undo step, so the new one
    /// isn't drawn over the old.
    pub(crate) fn set_header_footer(
        &mut self,
        hf: Option<sheaf_doc::stamps::HeaderFooter>,
        cx: &mut Context<Self>,
    ) {
        let State::Ready(doc) = &self.state else {
            return;
        };
        let snapshot = doc.model.snapshot();
        let store = doc.model.store().clone();
        let label = if hf.is_some() {
            "Header & footer"
        } else {
            "Remove header & footer"
        };
        self.set_notice(format!("{label}…"), cx);
        cx.spawn(async move |this, cx| {
            let planned = cx
                .background_executor()
                .spawn(async move {
                    let plan =
                        sheaf_ops::derive::strip_stamps(&store, &snapshot, &sheaf_ops::NoProgress)?;
                    let mut opened = Vec::new();
                    for c in plan {
                        let handle = RenderHandle::open_bytes(c.src.bytes().clone(), None)?;
                        opened.push((c, handle));
                    }
                    anyhow::Ok(opened)
                })
                .await;
            this.update(cx, |v, cx| {
                let opened = match planned {
                    Ok(o) => o,
                    Err(e) => {
                        v.set_notice(format!("{label} failed: {e:#}"), cx);
                        return;
                    }
                };
                let set = match hf.filter(|hf| !hf.is_empty()) {
                    Some(hf) => Op::SetHeaderFooter {
                        spec: hf.spec(),
                        scope: match hf.pages {
                            None => ScopeSel::All,
                            Some(pages) => ScopeSel::Pages(PageSel::ids(pages)),
                        },
                    },
                    None => Op::RemoveHeaderFooter {
                        pages: PageSel::All,
                    },
                };
                let why = Derive::StripHeaderFooter;
                if let Some(commit) = v.rebase(opened, why, vec![set], label, cx) {
                    let mut notice = format!("{label}: shown now, written when you save");
                    if let Some(told) = sheaf_doc::notice_text(&commit.notices) {
                        notice = format!("{notice}. {told}");
                    }
                    v.set_notice(notice, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    /// What the Header & Footer dialog starts from.
    pub(crate) fn header_footer(&self) -> Option<sheaf_doc::stamps::HeaderFooter> {
        let State::Ready(doc) = &self.state else {
            return None;
        };
        doc.model.state().header_footer().map(|hf| (**hf).clone())
    }
}

impl DocView {
    /// 0-based indexes of these pages, in document order.
    pub(crate) fn indexes_of(&self, ids: &[PageId]) -> Vec<usize> {
        let State::Ready(doc) = &self.state else {
            return Vec::new();
        };
        doc.model
            .state()
            .order()
            .iter()
            .enumerate()
            .filter(|(_, p)| ids.contains(p))
            .map(|(i, _)| i)
            .collect()
    }
}

impl DocView {
    /// Document > Watermark / Background: draw `wm` on `pages` (0-based;
    /// None: all), stripping the old one of its kind everywhere; with
    /// `wm` None, remove it. Pages show generated copies, as cropped pages
    /// do, so what's on screen is what's saved.
    pub(crate) fn apply_watermark(
        &mut self,
        behind: bool,
        wm: Option<sheaf_doc::watermark::Watermark>,
        pages: Option<Vec<usize>>,
        cx: &mut Context<Self>,
    ) {
        let State::Ready(doc) = &self.state else {
            return;
        };
        let snapshot = doc.model.snapshot();
        let store = doc.model.store().clone();
        let scope: Vec<PageId> = match pages {
            Some(ix) => self.page_ids_at(&ix),
            None => snapshot.state.order().iter().copied().collect(),
        };
        let what = if behind { "Background" } else { "Watermark" };
        let label = if wm.is_some() {
            what.to_string()
        } else {
            format!("Remove {}", what.to_lowercase())
        };
        // Its text is drawn into the copies, out of the model's sight.
        let (drawn, unencodable) = (wm.is_some(), wm.as_ref().is_some_and(|w| w.unencodable()));
        self.set_notice(format!("{label}…"), cx);
        cx.spawn(async move |this, cx| {
            let planned = cx
                .background_executor()
                .spawn(async move {
                    let plan = sheaf_ops::derive::watermark(
                        &store,
                        &snapshot,
                        behind,
                        wm.as_ref(),
                        &scope,
                        &sheaf_ops::NoProgress,
                    )?;
                    let mut opened = Vec::new();
                    for c in plan {
                        let handle = RenderHandle::open_bytes(c.src.bytes().clone(), None)?;
                        opened.push((c, handle));
                    }
                    anyhow::Ok(opened)
                })
                .await;
            this.update(cx, |v, cx| {
                let opened = match planned {
                    Ok(o) => o,
                    Err(e) => {
                        v.set_notice(format!("{label} failed: {e:#}"), cx);
                        return;
                    }
                };
                if opened.is_empty() {
                    v.set_notice(format!("No {} to remove", what.to_lowercase()), cx);
                    return;
                }
                let n: usize = opened.iter().map(|(c, _)| c.pages.len()).sum();
                let why = match drawn {
                    true => Derive::Watermark {
                        behind,
                        unencodable,
                    },
                    false => Derive::RemoveWatermark { behind },
                };
                if let Some(commit) = v.rebase(opened, why, Vec::new(), &label, cx) {
                    let mut notice = format!("{label}: {n} page{}", if n == 1 { "" } else { "s" });
                    if let Some(told) = sheaf_doc::notice_text(&commit.notices) {
                        notice = format!("{notice}. {told}");
                    }
                    v.set_notice(notice, cx);
                }
            })
            .ok();
        })
        .detach();
    }
}
