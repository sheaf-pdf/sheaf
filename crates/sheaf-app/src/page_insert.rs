//! Edits planned off the UI thread (M10.5 §14.1), each a planner
//! (jobs.rs):
//! - **Inserts:** Document > Insert Pages (a PDF's, Blank Pages… and From
//!   Images…) and Replace Pages…. Off the UI thread the file is read into
//!   the document's store, what lowering reads of it is read, the engine
//!   is opened on it, and the op is lowered on a snapshot ([`plan_insert`],
//!   [`plan_replace`]); on the UI thread it lands ([`DocView::land`]),
//!   lowered again on the current version if the document has moved on
//!   (`Stale::Relower`). Pages with no file of their own become PDFs in the
//!   generated-file cache (`sheaf_doc::generated`) first.
//! - **Derived pages:** Crop Pages, Watermark / Background and Header &
//!   Footer show pages from copies made of them off the UI thread, over
//!   the version their dialog opened on, and commit on that version
//!   (`Stale::Validate`): an edit since to what they changed is a conflict,
//!   and nothing changes. Their dialogs show no preview of the document
//!   until M10.5 slice 13's rendered previews.
//! - **Page Labels**, from its dialog, commits on the version the dialog
//!   opened on in the same way.

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use gpui::Context;
use sheaf_doc::images::PageFit;
use sheaf_doc::labels::Style;
use sheaf_doc::tx::LowerCx;
use sheaf_doc::{
    Actor, Commit, CommitOpts, Derive, DocState, Document, JobKind, PageId, Snapshot, Src, Stale,
    Store, Tx, VersionId,
};
use sheaf_edit::{
    BlobSel, Bring, FormBring, Op, OutlineBring, PageLabelRange, PageRef, PageSel, PageSize,
    PageSource, RangeSet, ScopeSel,
};
use sheaf_ops::derive::DerivedSource;
use sheaf_ops::{Progress, Tracker};
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

/// Pages to come in from a file, planned off the UI thread on a snapshot
/// ([`plan_insert`], [`plan_replace`]), for [`DocView::land`] to commit.
pub(crate) struct PlannedInsert {
    /// The file, a source in the document's store.
    src: Src,
    /// The engine opened on it, to draw its pages.
    handle: RenderHandle,
    /// The op that brings them, lowered on the snapshot, holding what
    /// their marks hold until it's committed.
    tx: Tx,
    /// The snapshot's version.
    on: VersionId,
}

#[cfg(test)]
impl PlannedInsert {
    pub(crate) fn src(&self) -> &Src {
        &self.src
    }

    /// What its op made, as lowered on the snapshot.
    pub(crate) fn created(&self) -> sheaf_doc::tx::Created {
        self.tx.preview().created
    }
}

/// Pages `pages` (None: all) of the file at `path` ready to come into the
/// document `snap` is of: the file read into `store`, a transaction on
/// `snap` lowering with `cx` holding what their marks hold, what lowering
/// reads of them read, and the engine opened on the file.
fn bring_in(
    snap: &Snapshot,
    store: &Store,
    cx: Arc<LowerCx>,
    path: &Path,
    pages: Option<Range<u32>>,
) -> anyhow::Result<(Src, RenderHandle, Tx)> {
    let (src, _) = store.read_file(path, None)?;
    let pages: Vec<u32> = pages.unwrap_or(0..src.page_count()).collect();
    let mut tx = Document::tx_at(snap, cx);
    for blob in sheaf_doc::tx::ready_to_insert(store, &snap.primary, &src, &pages) {
        tx.pin(blob);
    }
    tx.pin(src.blob().clone());
    let handle = RenderHandle::open_bytes(src.bytes().clone(), None)?;
    Ok((src, handle, tx))
}

/// Every page of the file at `path` inserted before `before` (None: at the
/// end) of the document `snap` is of, planned off the UI thread
/// ([`bring_in`]), as `cx` lowers it: with its bookmarks merged by title,
/// and its marks and form brought, when `bookmarks`; pages sheaf made
/// (blank, images) bring nothing more.
pub(crate) fn plan_insert(
    snap: &Snapshot,
    store: &Store,
    cx: Arc<LowerCx>,
    path: &Path,
    before: Option<PageId>,
    bookmarks: bool,
    p: &dyn Progress,
) -> anyhow::Result<PlannedInsert> {
    let (src, handle, mut tx) = bring_in(snap, store, cx, path, None)?;
    let bring = Bring {
        marks: bookmarks,
        form: FormBring::New,
        outline: match bookmarks {
            true => OutlineBring::MergeByTitle,
            false => OutlineBring::None,
        },
    };
    let insert = Op::InsertPages {
        from: PageSource::File {
            file: BlobSel::Id(src.id()),
            name: sheaf_doc::store::file_name(path).to_string(),
        },
        pages: None,
        before: before.map(PageRef::Id),
        bring,
        locals: Vec::new(),
    };
    tx.apply(insert, p)?;
    Ok(PlannedInsert {
        src,
        handle,
        tx,
        on: snap.version,
    })
}

/// Pages `pages` replaced with pages `range` (0-based) of the file at
/// `path`, planned off the UI thread ([`bring_in`]), as `cx` lowers it.
fn plan_replace(
    snap: &Snapshot,
    store: &Store,
    cx: Arc<LowerCx>,
    path: &Path,
    pages: Vec<PageId>,
    range: Range<u32>,
    p: &dyn Progress,
) -> anyhow::Result<PlannedInsert> {
    let with = RangeSet::from_runs(std::iter::once(range.start + 1..=range.end))?;
    let (src, handle, mut tx) = bring_in(snap, store, cx, path, Some(range))?;
    let replace = Op::ReplacePages {
        pages: pages.into_iter().map(PageRef::Id).collect(),
        from: PageSource::File {
            file: BlobSel::Id(src.id()),
            name: sheaf_doc::store::file_name(path).to_string(),
        },
        with: Some(with),
        bring: Bring::default(),
    };
    tx.apply(replace, p)?;
    Ok(PlannedInsert {
        src,
        handle,
        tx,
        on: snap.version,
    })
}

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

    /// `plan` committed as one version called `label`: on the version
    /// it was planned on, its op lowered again on the current one if the
    /// document has moved on since, keeping the ids it made (§8.5). The
    /// commit, or None when it failed, which the notice says. Only then is
    /// the engine opened on the file kept, and PDFium stops drawing the
    /// marks sheaf now draws.
    pub(crate) fn land(
        &mut self,
        plan: PlannedInsert,
        label: &str,
        cx: &mut Context<Self>,
    ) -> Option<Commit> {
        let PlannedInsert {
            src,
            handle,
            tx,
            on,
        } = plan;
        let commit = self
            .commit(tx, CommitOpts::based(on, Stale::Relower, label), cx)
            .ok()?;
        let State::Ready(doc) = &mut self.state else {
            return None;
        };
        doc.handles.entry(src.id()).or_insert(handle);
        doc.take_in(&src);
        Some(commit)
    }

    /// Insert every page of `path` before `before` (None: at the end), as
    /// one undo step labelled "Insert `name`", with its bookmarks merged
    /// when `bookmarks`.
    pub(crate) fn insert_file(
        &mut self,
        path: PathBuf,
        before: Option<PageId>,
        name: String,
        bookmarks: bool,
        cx: &mut Context<Self>,
    ) {
        self.insert_made(name, before, bookmarks, move || Ok(path), cx);
    }

    /// [`insert_file`](Self::insert_file) of the file `make` makes, off
    /// the UI thread.
    fn insert_made(
        &mut self,
        name: String,
        before: Option<PageId>,
        bookmarks: bool,
        make: impl FnOnce() -> anyhow::Result<PathBuf> + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        if self.still_opening(cx) {
            return;
        }
        let State::Ready(doc) = &self.state else {
            return;
        };
        let snap = doc.model.snapshot();
        let store = doc.model.store().clone();
        let lower = doc.model.lower_cx(Actor::Job(JobKind::Insert));
        let label = format!("Insert {name}");
        let (timing, t) = (self.timing, Instant::now());
        let work = move |p: &Tracker| {
            let path = make()?;
            plan_insert(&snap, &store, lower, &path, before, bookmarks, p)
        };
        let land = {
            let label = label.clone();
            move |v: &mut DocView, plan: PlannedInsert, cx: &mut Context<DocView>| {
                let State::Ready(doc) = &v.state else {
                    return None;
                };
                let had = doc.model.state().bookmark_count();
                let page_count = plan.src.page_count();
                let commit = v.land(plan, &label, cx)?;
                let State::Ready(doc) = &v.state else {
                    return None;
                };
                let added = commit.created.pages().count();
                let merged = doc.model.state().bookmark_count().saturating_sub(had);
                let mut notice = if !bookmarks {
                    // Pages sheaf made (blank, images): name says it all.
                    format!("Inserted {name}")
                } else {
                    format!(
                        "Inserted {added} page{} from {name}{}",
                        if added == 1 { "" } else { "s" },
                        match merged {
                            0 => String::new(),
                            1 => "; 1 bookmark merged".into(),
                            n => format!("; {n} bookmarks merged"),
                        }
                    )
                };
                if let Some(told) = sheaf_doc::notice_text(&commit.notices) {
                    notice = format!("{notice}. {told}");
                }
                if timing {
                    eprintln!(
                        "[timing] inserted {name} ({page_count} pages) in {:?}",
                        t.elapsed()
                    );
                }
                Some(notice)
            }
        };
        self.run_planner(&label, work, |_| None, land, cx);
    }

    pub(crate) fn insert_blank(
        &mut self,
        count: usize,
        size: Size,
        at: Where,
        cx: &mut Context<Self>,
    ) {
        let s = self.size_of(size);
        let before = self.before_for(at);
        let name = if count == 1 {
            "blank page".to_string()
        } else {
            format!("{count} blank pages")
        };
        let make = move || {
            let pdf = sheaf_doc::generated::blank_pages(s.width, s.height, count)?;
            sheaf_doc::generated::store(&pdf)
        };
        self.insert_made(name, before, false, make, cx);
    }

    /// Images as pages, fitted to `size` (None: each image's own size).
    pub(crate) fn insert_images(
        &mut self,
        files: Vec<PathBuf>,
        size: Option<Size>,
        at: Where,
        cx: &mut Context<Self>,
    ) {
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
        let make = move || {
            let pdf = sheaf_doc::images::files_to_pdf(&files, fit)?;
            sheaf_doc::generated::store(&pdf)
        };
        self.insert_made(name, before, false, make, cx);
    }

    /// Replace `ids` with pages `range` (0-based) of the PDF at `path`, as
    /// one undo step. They keep their bookmarks; extra pages on either
    /// side are deleted or inserted after.
    pub(crate) fn replace_pages_with(
        &mut self,
        path: PathBuf,
        ids: Vec<PageId>,
        range: Range<u32>,
        cx: &mut Context<Self>,
    ) {
        if self.still_opening(cx) {
            return;
        }
        if range.is_empty() {
            return self.set_notice("Replace failed: no pages to replace with", cx);
        }
        let State::Ready(doc) = &self.state else {
            return;
        };
        let snap = doc.model.snapshot();
        let store = doc.model.store().clone();
        let lower = doc.model.lower_cx(Actor::Job(JobKind::Insert));
        let (n, with) = (ids.len(), range.len());
        let label = format!("Replace {n} page{}", if n == 1 { "" } else { "s" });
        let name = sheaf_doc::store::file_name(&path);
        let work = move |p: &Tracker| plan_replace(&snap, &store, lower, &path, ids, range, p);
        let land = {
            let label = label.clone();
            move |v: &mut DocView, plan: PlannedInsert, cx: &mut Context<DocView>| {
                let commit = v.land(plan, &label, cx)?;
                let mut notice = format!(
                    "Replaced {n} page{} with {with} from {name}",
                    if n == 1 { "" } else { "s" },
                );
                if let Some(told) = sheaf_doc::notice_text(&commit.notices) {
                    notice = format!("{notice}. {told}");
                }
                Some(notice)
            }
        };
        self.run_planner(&label, work, |_| None, land, cx);
    }
}

/// The pages at these 0-based indexes of `snap`, a dialog's version.
pub(crate) fn ids_at(snap: &Snapshot, indexes: &[usize]) -> Vec<PageId> {
    (indexes.iter())
        .filter_map(|&i| snap.state.page_at(i))
        .collect()
}

/// The copies a derived flow made, each with the engine opened on it to
/// draw its pages.
type Copies = Vec<(DerivedSource, RenderHandle)>;

/// Each of `copies` with the engine opened on it: off the UI thread.
fn with_engines(copies: Vec<DerivedSource>) -> anyhow::Result<Copies> {
    (copies.into_iter())
        .map(|c| {
            let handle = RenderHandle::open_bytes(c.src.bytes().clone(), None)?;
            Ok((c, handle))
        })
        .collect()
}

impl DocView {
    /// `tx` with each page of `copies` shown from the copy a derived flow
    /// made of it, as `why` says, for a version called `label`: None when
    /// that failed, which the notice says.
    fn show_copies(
        &mut self,
        mut tx: Tx,
        copies: &[(DerivedSource, RenderHandle)],
        why: Derive,
        label: &str,
        cx: &mut Context<Self>,
    ) -> Option<Tx> {
        let pages: Vec<(PageId, Src, u32)> = copies.iter().flat_map(|(c, _)| c.bases()).collect();
        if !pages.is_empty()
            && let Err(e) = tx.set_page_base(pages, why)
        {
            self.set_notice(failed(label, &e), cx);
            return None;
        }
        Some(tx)
    }

    /// The copies a committed derived flow shows pages from: noted with
    /// the document, so a save writes their pages as the ones they were
    /// made of, and opened to draw.
    fn keep_copies(&mut self, copies: Copies) {
        let State::Ready(doc) = &mut self.state else {
            return;
        };
        for (c, handle) in copies {
            c.note(&mut doc.model);
            doc.handles.insert(c.src.id(), handle);
        }
    }

    /// A transaction of the user's on `snap`'s version: None while no
    /// document is open.
    fn user_tx_at(&self, snap: &Snapshot) -> Option<Tx> {
        match &self.state {
            State::Ready(doc) => Some(Document::tx_at(snap, doc.model.lower_cx(Actor::User))),
            _ => None,
        }
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
    /// Document > Crop Pages: pages `ids` of `opened`, the version the
    /// dialog opened on, cropped as `spec` says. Cropped copies are made
    /// off the UI thread, in memory, into the document's store; the pages
    /// are then shown from them, as one step on `opened`.
    pub(crate) fn crop_pages(
        &mut self,
        opened: Snapshot,
        ids: Vec<PageId>,
        spec: sheaf_ops::crop::CropSpec,
        cx: &mut Context<Self>,
    ) {
        let State::Ready(doc) = &self.state else {
            return;
        };
        if ids.is_empty() {
            return;
        }
        let store = doc.model.store().clone();
        let n = ids.len();
        let label = format!("Crop {n} page{}", if n == 1 { "" } else { "s" });
        let work = {
            let opened = opened.clone();
            move |p: &Tracker| with_engines(sheaf_ops::crop::plan(&store, &opened, &ids, spec, p)?)
        };
        let land = {
            let label = label.clone();
            move |v: &mut DocView, copies: Copies, cx: &mut Context<DocView>| {
                let tx = v.user_tx_at(&opened)?;
                let tx = v.show_copies(tx, &copies, Derive::Crop, &label, cx)?;
                let opts = CommitOpts::based(opened.version, Stale::Validate, label.as_str());
                v.commit(tx, opts, cx).ok()?;
                v.keep_copies(copies);
                Some(format!("Cropped {n} page{}", if n == 1 { "" } else { "s" }))
            }
        };
        self.run_planner(&label, work, |_| None, land, cx);
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

/// The op that numbers pages `from..=to` (0-based) of `st` with `style`,
/// `prefix` and its first number, keeping the numbering after them, or,
/// with `change` None, removes every label; and what undo calls it. Its
/// ranges name the pages they start at by id, as `st` has them.
pub(crate) fn labels_op(
    st: &DocState,
    change: Option<(usize, usize, Style, String, u32)>,
) -> (Op, &'static str) {
    use sheaf_doc::labels;
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
    (Op::SetPageLabels { ranges }, label)
}

impl DocView {
    /// Document > Page Labels: number pages `from..=to` of `opened`, the
    /// version the dialog opened on, with `style`, keeping the numbering
    /// after them; or remove every label. One step on `opened`.
    pub(crate) fn set_page_labels(
        &mut self,
        opened: Snapshot,
        change: Option<(usize, usize, Style, String, u32)>,
        cx: &mut Context<Self>,
    ) {
        let (op, label) = labels_op(&opened.state, change);
        let Some(tx) = self.user_tx_at(&opened) else {
            return;
        };
        if let Some(tx) = self.lower_in(tx, vec![op], label, cx) {
            let opts = CommitOpts::based(opened.version, Stale::Validate, label);
            self.commit(tx, opts, cx).ok();
        }
    }
}

impl DocView {
    /// Document > Header & Footer: set (or with None, remove) the
    /// header/footer, from the dialog opened on `opened`. Pages whose file
    /// already has one drawn are moved onto copies without it first, in
    /// the same undo step, so the new one isn't drawn over the old.
    pub(crate) fn set_header_footer(
        &mut self,
        opened: Snapshot,
        hf: Option<sheaf_doc::stamps::HeaderFooter>,
        cx: &mut Context<Self>,
    ) {
        let State::Ready(doc) = &self.state else {
            return;
        };
        let store = doc.model.store().clone();
        let label = if hf.is_some() {
            "Header & footer"
        } else {
            "Remove header & footer"
        };
        let work = {
            let opened = opened.clone();
            move |p: &Tracker| with_engines(sheaf_ops::derive::strip_stamps(&store, &opened, p)?)
        };
        let land = move |v: &mut DocView, copies: Copies, cx: &mut Context<DocView>| {
            let set = match hf.filter(|hf| !hf.is_empty()) {
                Some(hf) => Op::SetHeaderFooter {
                    spec: hf.spec(),
                    scope: match hf.pages {
                        None => ScopeSel::All,
                        Some(pages) => ScopeSel::Pages(PageSel::ids(pages)),
                    },
                },
                // Every page it opened on, by id: never the order.
                None => Op::RemoveHeaderFooter {
                    pages: PageSel::ids(opened.state.order().iter().copied()),
                },
            };
            let tx = v.user_tx_at(&opened)?;
            let tx = v.show_copies(tx, &copies, Derive::StripHeaderFooter, label, cx)?;
            let tx = v.lower_in(tx, vec![set], label, cx)?;
            let opts = CommitOpts::based(opened.version, Stale::Validate, label);
            let commit = v.commit(tx, opts, cx).ok()?;
            v.keep_copies(copies);
            let mut notice = format!("{label}: shown now, written when you save");
            if let Some(told) = sheaf_doc::notice_text(&commit.notices) {
                notice = format!("{notice}. {told}");
            }
            Some(notice)
        };
        self.run_planner(label, work, |_| None, land, cx);
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
    /// Document > Watermark / Background: draw `wm` on pages `pages` (None:
    /// all) of `opened`, the version the dialog opened on, stripping the
    /// old one of its kind everywhere; with `wm` None, remove it. Pages
    /// show generated copies, as cropped pages do, so what's on screen is
    /// what's saved.
    pub(crate) fn apply_watermark(
        &mut self,
        opened: Snapshot,
        behind: bool,
        wm: Option<sheaf_doc::watermark::Watermark>,
        pages: Option<Vec<PageId>>,
        cx: &mut Context<Self>,
    ) {
        let State::Ready(doc) = &self.state else {
            return;
        };
        let store = doc.model.store().clone();
        let scope: Vec<PageId> =
            pages.unwrap_or_else(|| opened.state.order().iter().copied().collect());
        let what = if behind { "Background" } else { "Watermark" };
        let label = if wm.is_some() {
            what.to_string()
        } else {
            format!("Remove {}", what.to_lowercase())
        };
        // Its text is drawn into the copies, out of the model's sight.
        let why = match &wm {
            Some(wm) => Derive::Watermark {
                behind,
                unencodable: wm.unencodable(),
            },
            None => Derive::RemoveWatermark { behind },
        };
        let work = {
            let opened = opened.clone();
            move |p: &Tracker| {
                let wm = wm.as_ref();
                with_engines(sheaf_ops::derive::watermark(
                    &store, &opened, behind, wm, &scope, p,
                )?)
            }
        };
        let land = {
            let label = label.clone();
            move |v: &mut DocView, copies: Copies, cx: &mut Context<DocView>| {
                if copies.is_empty() {
                    return Some(format!("No {} to remove", what.to_lowercase()));
                }
                let n: usize = copies.iter().map(|(c, _)| c.pages.len()).sum();
                let tx = v.user_tx_at(&opened)?;
                let tx = v.show_copies(tx, &copies, why, &label, cx)?;
                let opts = CommitOpts::based(opened.version, Stale::Validate, label.as_str());
                let commit = v.commit(tx, opts, cx).ok()?;
                v.keep_copies(copies);
                let mut notice = format!("{label}: {n} page{}", if n == 1 { "" } else { "s" });
                if let Some(told) = sheaf_doc::notice_text(&commit.notices) {
                    notice = format!("{notice}. {told}");
                }
                Some(notice)
            }
        };
        self.run_planner(&label, work, |_| None, land, cx);
    }
}
