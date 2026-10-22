//! Edits planned off the UI thread (M10.5 §14.1), each a planner
//! (jobs.rs), and the edits of dialogs, committed on the version they
//! opened on:
//! - **Inserts:** Document > Insert Pages (a PDF's, Blank Pages… and From
//!   Images…) and Replace Pages…. Off the UI thread the file is read into
//!   the document's store, what lowering reads of it is read, and the op
//!   is lowered on a snapshot ([`plan_insert`], [`plan_replace`]); on the
//!   UI thread it lands ([`DocView::land`]), lowered again on the current
//!   version if the document has moved on (`Stale::Relower`). Pages with no
//!   file of their own become PDFs in the generated-file cache
//!   (`sheaf_doc::generated`) first.
//! - **Crop Pages** changes pages' boxes alone (M10.5 slice 15). Margins
//!   are one op, committed on the version the dialog opened on
//!   (`Stale::Validate`): an edit since to a page's boxes is a conflict,
//!   and nothing changes. Trim White is a planner: it measures each page's
//!   content as it draws through the document's views, off the UI thread
//!   (`sheaf_ops::crop::trim_white`), and commits on the version it
//!   measured in the same way, measuring again on the current one when
//!   that conflicts. The dialog previews its page as either would draw it
//!   (`crop_dialog`).
//! - **Watermark / Background** is the document's watermark at that z
//!   (M10.5 slice 16), laid out on each page as a save writes it. Setting
//!   it is one op, committed on the version the dialog opened on as a
//!   crop is. Remove is a planner: its lowering reads each page's content
//!   off the UI thread, for the watermarks other programs and files drew,
//!   which it takes off too and counts by who drew them (§6.5, D61), and
//!   it commits on the version the dialog opened on; the notice is its
//!   line, with those counts. The dialog previews its page as applying
//!   would draw it (`watermark_dialog`).
//! - **Header & Footer** is the document's header and footer (M10.5 slice
//!   17), set and taken off as a watermark is: setting it is one op,
//!   committed on the version the dialog opened on, and Remove is a
//!   planner, which takes off the headers and footers others drew too and
//!   counts them. The dialog previews its page as applying would draw it
//!   (`header_footer_dialog`).
//! - **Page Labels**, from its dialog, commits on the version the dialog
//!   opened on in the same way.
//!
//! None opens an engine: the render worker opens a file's when one of its
//! pages is first drawn (M10.5 §10.3).

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use gpui::Context;
use sheaf_doc::images::PageFit;
use sheaf_doc::labels::Style;
use sheaf_doc::tx::LowerCx;
use sheaf_doc::{
    Actor, Blob, Commit, CommitOpts, DocState, Document, EditError, JobKind, PageId, Snapshot, Src,
    Stale, Store, Tx, VersionId,
};
use sheaf_edit::{
    BlobSel, BoxSpec, Bring, FormBring, Margins, Op, OutlineBring, PageLabelRange, PageRef,
    PageSel, PageSize, PageSource, Patch, RangeSet, Z,
};
use sheaf_ops::crop::{self, Trimmed};
use sheaf_ops::views::Views;
use sheaf_ops::{Progress, Tracker};

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
    /// The op that brings them, lowered on the snapshot, holding what
    /// their marks hold until it's committed.
    tx: Tx,
    /// The snapshot's version.
    on: VersionId,
}

// For docview's headless tests, which run on Linux only.
#[cfg(all(test, target_os = "linux"))]
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
/// `snap` lowering with `cx` holding what their marks hold, and what
/// lowering reads of them read.
fn bring_in(
    snap: &Snapshot,
    store: &Store,
    cx: Arc<LowerCx>,
    path: &Path,
    pages: Option<Range<u32>>,
) -> anyhow::Result<(Src, Tx)> {
    let (src, _) = store.read_file(path, None)?;
    let pages: Vec<u32> = pages.unwrap_or(0..src.page_count()).collect();
    let mut tx = Document::tx_at(snap, cx);
    for blob in sheaf_doc::tx::ready_to_insert(store, &snap.primary, &src, &pages) {
        tx.pin(blob);
    }
    tx.pin(src.blob().clone());
    Ok((src, tx))
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
    let (src, mut tx) = bring_in(snap, store, cx, path, None)?;
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
    let (src, mut tx) = bring_in(snap, store, cx, path, Some(range))?;
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
    /// commit, or None when it failed, which the notice says; then nothing
    /// keeps the file it read.
    pub(crate) fn land(
        &mut self,
        plan: PlannedInsert,
        label: &str,
        cx: &mut Context<Self>,
    ) -> Option<Commit> {
        let opts = CommitOpts::based(plan.on, Stale::Relower, label);
        self.commit(plan.tx, opts, cx).ok()
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

impl DocView {
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

    /// The 0-based indexes of the selected pages, in document order, when
    /// more than one is: what the Crop Pages dialog offers as the selection,
    /// and what the Watermark and Header & Footer dialogs count others' on;
    /// none otherwise.
    pub(crate) fn selected_indexes(&self) -> Vec<usize> {
        let (_, targets) = self.target_indexes();
        match targets.len() {
            0 | 1 => Vec::new(),
            _ => targets,
        }
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
    /// Document > Crop Pages, by margins: pages `ids` of `opened`, the
    /// version the dialog opened on, each cropped by `margins` off its
    /// sides as it shows, as one op committed on `opened`. Margins that
    /// leave too little of a page crop nothing, and the notice says so.
    pub(crate) fn crop_pages(
        &mut self,
        opened: Snapshot,
        ids: Vec<PageId>,
        margins: Margins,
        cx: &mut Context<Self>,
    ) {
        if ids.is_empty() {
            return;
        }
        let n = ids.len();
        let label = crop_label(n);
        let Some(tx) = self.user_tx_at(&opened) else {
            return;
        };
        let Some(tx) = self.lower_in(tx, vec![crop_op(ids, margins)], &label, cx) else {
            return;
        };
        let opts = CommitOpts::based(opened.version, Stale::Validate, label.as_str());
        if let Ok(commit) = self.commit(tx, opts, cx) {
            let mut notice = format!("Cropped {n} page{}", if n == 1 { "" } else { "s" });
            if let Some(told) = sheaf_doc::notice_text(&commit.notices) {
                notice = format!("{notice}. {told}");
            }
            self.set_notice(notice, cx);
        }
    }

    /// Document > Crop Pages, Trim White: pages `ids` of `opened`, the
    /// version the dialog opened on, each cropped down to its content with
    /// [`crop::PAD`] to spare, as one step. They're measured off the UI
    /// thread, as their content draws through the document's views in the
    /// background's budget ([`crop::trim_white`]), and the crops commit on
    /// the version they were measured in (M10.5 §8.5).
    pub(crate) fn trim_white(
        &mut self,
        opened: Snapshot,
        ids: Vec<PageId>,
        cx: &mut Context<Self>,
    ) {
        self.trim_from(opened, ids, 1, cx);
    }

    /// [`trim_white`](Self::trim_white) of pages `ids`, measured in `snap`,
    /// on its `attempt`th try: when another edit since changed a page's
    /// boxes or what it draws, the commit conflicts, and they're measured
    /// again in the document as it is then, the pages still in it, up to
    /// [`TRIM_ATTEMPTS`] tries; then the conflict's notice stays.
    fn trim_from(
        &mut self,
        snap: Snapshot,
        ids: Vec<PageId>,
        attempt: u32,
        cx: &mut Context<Self>,
    ) {
        let State::Ready(doc) = &self.state else {
            return;
        };
        if ids.is_empty() {
            return;
        }
        // A realizer of its own: the state it measures in is never the
        // screen's.
        let views = Views::new(doc.views.pool().clone());
        let lower = doc.model.lower_cx(Actor::Job(JobKind::Trim));
        let work = {
            let (snap, ids) = (snap.clone(), ids.clone());
            move |p: &Tracker| {
                let (pad, urgency) = (crop::PAD, crop::Urgency::Background);
                crop::trim_white(&views, &snap, lower, &ids, pad, urgency, p)
            }
        };
        let land = move |v: &mut DocView, found: Trimmed, cx: &mut Context<DocView>| {
            let State::Ready(doc) = &v.state else {
                return None;
            };
            let label = crop_label(found.crops.len());
            let mut tx = Document::tx_at(&snap, doc.model.lower_cx(Actor::Job(JobKind::Trim)));
            if let Err(e) = found.lower(&mut tx) {
                return Some(failed(&label, &e));
            }
            let opts = CommitOpts::based(snap.version, Stale::Validate, label.as_str());
            match v.commit(tx, opts, cx) {
                Ok(commit) => Some(trimmed_notice(&found, &commit)),
                Err(EditError::Conflict { .. }) if attempt < TRIM_ATTEMPTS => {
                    let now = v.model_snapshot()?;
                    let ids = (ids.into_iter())
                        .filter(|&id| now.state.def(id).is_some())
                        .collect();
                    v.trim_from(now, ids, attempt + 1, cx);
                    None
                }
                // The commit's notice says why.
                Err(_) => None,
            }
        };
        self.run_planner("Trim White", work, |_| None, land, cx);
    }
}

/// The op cropping pages `ids` by `margins`, off each side as it shows:
/// what the Crop Pages dialog applies, and what its preview lowers.
pub(crate) fn crop_op(ids: Vec<PageId>, margins: Margins) -> Op {
    Op::SetPageBoxes {
        pages: PageSel::ids(ids),
        crop: Patch::Set(BoxSpec::Margins(margins)),
        trim: Patch::Keep,
        bleed: Patch::Keep,
        art: Patch::Keep,
    }
}

/// How many times Trim White measures pages before a conflict stands.
const TRIM_ATTEMPTS: u32 = 3;

/// What undo calls a crop of `n` pages.
fn crop_label(n: usize) -> String {
    format!("Crop {n} page{}", if n == 1 { "" } else { "s" })
}

/// What the reader is told of Trim White's `found`, committed as `commit`:
/// how many pages it cropped, and how many it left as they were.
fn trimmed_notice(found: &Trimmed, commit: &Commit) -> String {
    let n = found.crops.len();
    let mut notice = format!("Trimmed {n} page{}", if n == 1 { "" } else { "s" });
    match found.left.len() {
        0 => {}
        1 => notice.push_str(", 1 left as it was"),
        left => notice.push_str(&format!(", {left} left as they were")),
    }
    if let Some(told) = sheaf_doc::notice_text(&commit.notices) {
        notice = format!("{notice}. {told}");
    }
    notice
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
    /// Document > Header & Footer: `set`, the dialog's op, which sets the
    /// header and footer on the pages it names of `opened`, the version
    /// the dialog opened on, committed on that version. Text its font
    /// can't draw all of is said in the notice, and so is a change of
    /// nothing.
    pub(crate) fn set_header_footer(&mut self, opened: Snapshot, set: Op, cx: &mut Context<Self>) {
        let label = "Header & footer";
        let Some(tx) = self.user_tx_at(&opened) else {
            return;
        };
        let Some(tx) = self.lower_in(tx, vec![set], label, cx) else {
            return;
        };
        let opts = CommitOpts::based(opened.version, Stale::Validate, label);
        if let Ok(commit) = self.commit(tx, opts, cx) {
            let none = "No change to the header and footer";
            self.set_notice(line_notice(&commit, none), cx);
        }
    }

    /// Document > Header & Footer, Remove: the header and footer taken off
    /// every page of `opened`, the version the dialog opened on, and those
    /// other programs and files drew, the author's own too with
    /// `include_authored`, planned as [`remove_watermark`]'s removal is.
    ///
    /// [`remove_watermark`]: Self::remove_watermark
    pub(crate) fn remove_header_footer(
        &mut self,
        opened: Snapshot,
        include_authored: bool,
        cx: &mut Context<Self>,
    ) {
        let remove = Op::RemoveHeaderFooter {
            pages: PageSel::ids(opened.state.order().iter().copied()),
            include_authored,
        };
        let (label, none) = ("Remove header & footer", "No header or footer to remove");
        self.plan_removal(opened, remove, label, none, cx);
    }
}

impl DocView {
    /// Document > Watermark / Background: `set`, the dialog's op, which
    /// sets the watermark at its z on the pages it names of `opened`, the
    /// version the dialog opened on, committed on that version, holding
    /// `holds`, the file it draws, until it is. Text its font can't draw
    /// all of is said in the notice, and so is a change of nothing.
    pub(crate) fn set_watermark(
        &mut self,
        opened: Snapshot,
        set: Op,
        holds: Option<Blob>,
        cx: &mut Context<Self>,
    ) {
        let (label, none) = match &set {
            Op::SetWatermark { z: Z::Under, .. } => ("Background", "No change to the background"),
            _ => ("Watermark", "No change to the watermark"),
        };
        let Some(mut tx) = self.user_tx_at(&opened) else {
            return;
        };
        if let Some(blob) = holds {
            tx.pin(blob);
        }
        let Some(tx) = self.lower_in(tx, vec![set], label, cx) else {
            return;
        };
        let opts = CommitOpts::based(opened.version, Stale::Validate, label);
        if let Ok(commit) = self.commit(tx, opts, cx) {
            self.set_notice(line_notice(&commit, none), cx);
        }
    }

    /// Document > Watermark / Background, Remove: the watermark at `z`
    /// taken off every page of `opened`, the version the dialog opened on,
    /// and those other programs and files drew, the author's own too with
    /// `include_authored` ([`plan_removal`](Self::plan_removal)).
    pub(crate) fn remove_watermark(
        &mut self,
        opened: Snapshot,
        z: Z,
        include_authored: bool,
        cx: &mut Context<Self>,
    ) {
        let remove = Op::RemoveWatermark {
            z: Some(z),
            pages: PageSel::ids(opened.state.order().iter().copied()),
            include_authored,
        };
        let (label, none) = match z {
            Z::Over => ("Remove watermark", "No watermark to remove"),
            Z::Under => ("Remove background", "No background to remove"),
        };
        self.plan_removal(opened, remove, label, none, cx);
    }

    /// `remove`, a dialog's removal from the pages it names of `opened`,
    /// the version the dialog opened on, as a planner called `label`: its
    /// lowering reads each page's content off the UI thread (§6.5), and it
    /// commits on `opened`. The notice is its line, which counts what it
    /// found of others' by who drew them, and what it left in place (D61),
    /// or `none` when it changed nothing.
    fn plan_removal(
        &mut self,
        opened: Snapshot,
        remove: Op,
        label: &'static str,
        none: &'static str,
        cx: &mut Context<Self>,
    ) {
        let State::Ready(doc) = &self.state else {
            return;
        };
        let lower = doc.model.lower_cx(Actor::User);
        let work = {
            let opened = opened.clone();
            move |p: &Tracker| {
                let mut tx = Document::tx_at(&opened, lower);
                tx.apply(remove, p)?;
                Ok(tx)
            }
        };
        let land = move |v: &mut DocView, tx: Tx, cx: &mut Context<DocView>| {
            let opts = CommitOpts::based(opened.version, Stale::Validate, label);
            let commit = v.commit(tx, opts, cx).ok()?;
            Some(line_notice(&commit, none))
        };
        self.run_planner(label, work, |_| None, land, cx);
    }
}

/// What the reader is told of a watermark's or a header and footer's
/// `commit`: its line, with its notices, or `none` when it changed nothing.
fn line_notice(commit: &Commit, none: &str) -> String {
    let line = commit.lines.first().filter(|_| commit.version.is_some());
    let Some(line) = line else {
        return none.to_string();
    };
    match sheaf_doc::notice_text(&commit.notices) {
        Some(told) => format!("{}. {told}", line.text),
        None => line.text.clone(),
    }
}
