//! Virtual document, edits, and snapshot history.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};

use anyhow::{Context, Result, bail};
use im::Vector;
use lopdf::ObjectId;
use serde::{Deserialize, Serialize};
use sheaf_core::{DestView, PageSize, Rotation};

use crate::annot::{Annot, AnnotId, AnnotKind};
use crate::file::ForeignAnnot;
use crate::forms::{FieldId, FileForm, full_name_of};
use crate::geom::Geom;
use crate::history::{HistoryPolicy, HistoryStats};
use crate::open::{Facts, FileAnnot, FileAnnots, FileBody, Renamed, ViewFacts};
use crate::prim::{HeapSize, Key, Prim, Rec, Val, same};
use crate::serialize::durable::FileStamp;
use crate::source::ObjRef;
use crate::store::{BlobId, BlobKind, BlobRef, Origin, Src, Store};
use crate::text_layer::TextLayer;

mod legacy;
mod records;
mod state;

pub(crate) use legacy::{LegacyDocState, legacy_field};
pub use records::{
    AnnotBody, AnnotRec, Bookmark, ContentStack, FieldNode, FieldProps, Foreign, FormDoc, FormInst,
    FormRef, GenId, GenSpec, Generator, Layer, Mark, PageBase, PageDef, ScriptState, Static,
    Target, Terminal, Widget, WidgetProps,
};
pub use state::{Builder, DocState, Invariant};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SourceId(pub u32);

/// Stable identity of a page in the virtual document. Never reused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PageId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct OutlineId(pub u64);

/// A file registered for the document's pages: its source, and the name
/// it goes by. Every insert registers anew, so inserting the same file
/// twice gives two registrations of one [`Src`], and two forms (until
/// M10.5 slice 11 makes the source its blob). The bytes, page table and
/// password are the source's.
#[derive(Debug, Clone)]
pub struct SourceInfo {
    pub src: Src,
    /// The path the bytes came from, or a derived copy's original's name.
    /// Form instances' names ([`FormDoc::names`]), notices, the recovery
    /// sidecar and save's same-file check read it; it's never read for
    /// bytes: after a save in place the path holds the new file, and a
    /// version from before the save still means the old one.
    pub name: PathBuf,
    /// For a copy sheaf made of another source's pages (cropped,
    /// watermarked, a header/footer stripped): which pages. A save writes
    /// each as the page it was made from, with its annotations and form
    /// fields, and the copy's content (M10 bug 1).
    pub derived: Option<Derived>,
    /// The file at `name` as the bytes were read from it: a save in place
    /// writes only while it's still that file, and otherwise asks (M10.5
    /// proposal 33). None for a source not read from a file: blank pages,
    /// bytes in memory, a derived copy.
    pub stamp: Option<FileStamp>,
}

/// Where a derived source's pages come from: its page `k` is a copy of page
/// `pages[k]` of `from`.
#[derive(Debug, Clone, PartialEq)]
pub struct Derived {
    pub from: SourceId,
    pub pages: Vec<u32>,
}

impl SourceInfo {
    /// The file at `path`, read once, opened with `password` and parsed
    /// into `store`, under its own path. A wrong or missing password is
    /// [`OpenError::Password`](crate::source::OpenError::Password) in the
    /// chain.
    pub fn read(store: &Store, path: &Path, password: Option<&str>) -> Result<SourceInfo> {
        let reading = || format!("reading {}", path.display());
        let (bytes, metadata) = sheaf_core::read_file_and_metadata(path).with_context(reading)?;
        let stamp = FileStamp::of(&metadata, &bytes).with_context(reading)?;
        let src = store
            .open(bytes, BlobKind::file(path, password))
            .with_context(|| format!("opening {}", path.display()))?;
        Ok(SourceInfo {
            src,
            name: path.to_path_buf(),
            derived: None,
            stamp: Some(stamp),
        })
    }

    /// For tests: `pages` blank Letter pages, named `a.pdf`, parsed into
    /// `store`.
    #[cfg(any(test, feature = "fixtures"))]
    pub fn blank(store: &Store, pages: u32) -> SourceInfo {
        let bytes = crate::generated::blank_pages(612.0, 792.0, pages as usize)
            .expect("blank pages are written");
        let kind = BlobKind::file(Path::new("a.pdf"), None);
        SourceInfo {
            src: store.open(bytes.into(), kind).expect("blank pages open"),
            name: "a.pdf".into(),
            derived: None,
            stamp: None,
        }
    }
}

/// How a bookmark's title is drawn (its `/F`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextStyle {
    #[serde(default)]
    pub bold: bool,
    #[serde(default)]
    pub italic: bool,
}

impl TextStyle {
    pub fn is_plain(&self) -> bool {
        !self.bold && !self.italic
    }
}

/// A bookmark as read from a file: pre-order with depth.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FlatOutlineItem {
    pub title: String,
    pub page: Option<u32>,
    pub depth: u16,
    /// Where on `page` it lands, in PageSpace; None is the top of the
    /// page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub view: Option<DestView>,
    /// `/C` and `/F`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<[u8; 3]>,
    #[serde(default, skip_serializing_if = "TextStyle::is_plain")]
    pub style: TextStyle,
    /// Its `/Count` isn't negative (see [`Bookmark::open`]).
    #[serde(default = "yes", skip_serializing_if = "is_yes")]
    pub open: bool,
    /// The item's object in its file, which its bookmark keeps as its
    /// [`Bookmark::src`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object: Option<lopdf::ObjectId>,
    /// It goes somewhere sheaf keeps as read ([`Target::Kept`]): an action
    /// other than a GoTo to a page here, or a destination that isn't a
    /// page here. `page` is None.
    #[serde(default, skip_serializing_if = "is_no")]
    pub kept: bool,
}

fn yes() -> bool {
    true
}

fn is_yes(b: &bool) -> bool {
    *b
}

fn is_no(b: &bool) -> bool {
    !*b
}

/// One bookmark of a subtree inserted by [`Edit::OutlineInsertTree`], in
/// pre-order. `depth` is relative to the insert point: 0 is a sibling there,
/// 1 a child of the item above at 0, and so on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewOutlineItem {
    pub title: String,
    pub target: Option<PageId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub view: Option<DestView>,
    pub depth: u16,
}

/// One change to the document. Serializable so plugins can submit edits
/// as data (see `todos/plugins.md`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Edit {
    /// Move `ids` (kept in document order) to just before `before`, or to the end.
    MovePages {
        ids: Vec<PageId>,
        before: Option<PageId>,
    },
    RotatePages {
        ids: Vec<PageId>,
        by: Rotation,
    },
    /// Removes the pages and cascades to bookmarks targeting them (their
    /// children are promoted).
    DeletePages {
        ids: Vec<PageId>,
    },
    /// Insert `range` of `source` before `before` (or at the end). If
    /// `outline` is given, merge it: top-level bookmarks whose title matches
    /// an existing top-level bookmark contribute their children to it;
    /// others are appended.
    InsertPages {
        source: SourceId,
        range: Range<u32>,
        before: Option<PageId>,
        outline: Option<Vec<FlatOutlineItem>>,
        /// sheaf's own marks in the file, by source page index. They come
        /// in on the new pages with fresh ids, after the file's other
        /// annotations there, which come from its source (see
        /// `bring_annots`).
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        annots: Vec<(u32, Vec<Annot>)>,
    },
    /// Copies inserted directly after each original.
    DuplicatePages {
        ids: Vec<PageId>,
    },
    /// Replace pages in place. The first `min(ids.len(), range.len())`
    /// keep their `PageId` (and so their bookmarks); extra originals are
    /// deleted, extra replacements inserted after.
    ReplacePages {
        ids: Vec<PageId>,
        source: SourceId,
        range: Range<u32>,
        /// sheaf's own marks on the replacement pages, by source page
        /// index; added beside the marks the kept pages already have, and
        /// the replacements' other annotations, from their source, in
        /// place of the kept pages' (see `bring_annots`).
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        annots: Vec<(u32, Vec<Annot>)>,
    },
    OutlineInsert {
        parent: Option<OutlineId>,
        after: Option<OutlineId>,
        title: String,
        target: Option<PageId>,
        /// Where on `target` it lands; None is the top of the page.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        view: Option<DestView>,
    },
    /// Insert several bookmarks at once, nested by depth (a new parent and
    /// its children, say), after `after` under `parent`, in order.
    OutlineInsertTree {
        parent: Option<OutlineId>,
        after: Option<OutlineId>,
        items: Vec<NewOutlineItem>,
    },
    /// Move a bookmark subtree. With `with_pages`, the pages it spans (from
    /// its first target up to the next bookmark's target) move with it.
    OutlineMove {
        id: OutlineId,
        parent: Option<OutlineId>,
        after: Option<OutlineId>,
        with_pages: bool,
    },
    /// `None` fields are left unchanged. For `target` and `color`,
    /// `Some(None)` clears the value; in JSON that is `null`, and an absent
    /// key means unchanged.
    OutlineEdit {
        id: OutlineId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            with = "double_option"
        )]
        target: Option<Option<PageId>>,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            with = "double_option"
        )]
        color: Option<Option<[u8; 3]>>,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            with = "double_option"
        )]
        view: Option<Option<DestView>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        style: Option<TextStyle>,
    },
    /// Delete a bookmark and its subtree.
    OutlineDelete {
        id: OutlineId,
    },
    /// Add markup to a page. The annotation's id is assigned by the document.
    /// One named as another tool's comment on the page takes its place (M9
    /// decision 5): of two named so, the one whose id the annotation's is.
    AnnotAdd {
        page: PageId,
        annot: Annot,
    },
    /// Replace an annotation (matched by id) with a new version.
    AnnotUpdate {
        page: PageId,
        annot: Annot,
    },
    /// Delete an annotation, and its thread: replies and statuses, ours
    /// and other tools'. A connector's others stay (M9 decision 8). A
    /// form's widget isn't deleted on its own: it goes with its page.
    AnnotDelete {
        page: PageId,
        id: AnnotId,
    },
    /// Delete other tools' comments from a page, by name; a save leaves
    /// them out. Their threads go too: replies and statuses, ours and
    /// other tools'.
    CommentsDelete {
        page: PageId,
        names: Vec<String>,
    },
    /// Add both ends of a connector pair in one step. Ids are assigned by the
    /// document and each end's `partner` is pointed at the other.
    ConnectorAdd {
        a: (PageId, Annot),
        b: (PageId, Annot),
    },
    /// Set (or replace) the OCR layer of each page. Unknown pages are an
    /// error.
    SetTextLayers {
        layers: Vec<(PageId, Arc<TextLayer>)>,
    },
    /// Drop unsaved OCR layers. Pages without one are ignored; a layer
    /// already written to the file can't be removed from the model.
    ClearTextLayers {
        ids: Vec<PageId>,
    },
    /// Show pages from other source pages (a cropped copy, say) while
    /// keeping their ids, rotation, bookmarks, marks and OCR text, which
    /// are in PageSpace, which a copy keeps: nothing moves.
    RebasePages {
        pages: Vec<Rebase>,
    },
    /// Replace the page labels. Empty removes them.
    SetPageLabels {
        ranges: Vec<crate::labels::LabelSpec>,
    },
    /// Set or remove the header/footer.
    SetHeaderFooter {
        header_footer: Option<crate::stamps::HeaderFooter>,
    },
}

/// One page of [`Edit::RebasePages`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Rebase {
    pub id: PageId,
    pub source: SourceId,
    pub index: u32,
}

/// Serde for `Option<Option<T>>` fields that distinguishes "absent"
/// (outer `None`, via `#[serde(default)]`) from `null` (`Some(None)`).
mod double_option {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<T: Serialize, S: Serializer>(
        v: &Option<Option<T>>,
        s: S,
    ) -> Result<S::Ok, S::Error> {
        match v {
            Some(inner) => inner.serialize(s),
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, T: Deserialize<'de>, D: Deserializer<'de>>(
        d: D,
    ) -> Result<Option<Option<T>>, D::Error> {
        Option::<T>::deserialize(d).map(Some)
    }
}

/// Identifies a long-running job whose results arrive in batches but
/// should undo as one step. See [`Document::apply_in_job`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct JobId(u64);

/// What an edit did, for user-facing notices.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct EditOutcome {
    pub pages_removed: usize,
    pub pages_added: usize,
    pub bookmarks_removed: usize,
    pub bookmarks_added: usize,
    /// Forms that came in with pages and were put under a new parent
    /// field, since their names clashed (M10 proposal 5): its name, and the
    /// file they came from.
    pub forms_renamed: Vec<(String, SourceId)>,
    /// Files whose signed fields came in with their pages: the signatures
    /// don't cover this file (M10 proposal 7).
    pub signatures_in: Vec<SourceId>,
    /// Text it added or changed (a mark's, a header's or footer's) has
    /// characters the built-in fonts can't draw: they show as `?` in the
    /// file (see [`Document::unencodable_notice`]).
    pub unencodable: bool,
}

/// A version in the history tree. It names one state, which never changes:
/// a job's amend or a save puts a new version in its place. Ids aren't
/// reused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct VersionId(pub u64);

/// One edit as applied, and what it changed: prims with the values they
/// replaced. A replay applies them and checks each before-value, so it
/// rebuilds the version exactly, ids included, and never runs the edit
/// again.
#[derive(Debug, Clone)]
pub struct Step {
    pub edit: Edit,
    /// What it did, in words (see [`crate::describe`]), against the
    /// document just before it.
    pub line: String,
    /// What it changed. Coalesced over its version's steps (see
    /// [`coalesce`]), so a slot's change is on the last step that made
    /// one, and an edit whose change a later one undid has none.
    pub(crate) prims: Arc<[Prim]>,
}

/// A version's prims don't fit the state they were made from: an internal
/// bug, never the reader's. The version can't be rebuilt.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[error("History is damaged at version {}", .version.0)]
pub struct Damaged {
    pub version: VersionId,
}

/// An edit came while the file is still opening: its facts aren't read
/// yet, so there's nothing to edit (M10.5 proposal 28). Slice 11 makes it
/// `EditError::NotReady`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the file is still opening")]
pub struct NotReady;

/// A save that wrote a version.
#[derive(Debug, Clone, PartialEq)]
pub struct Saved {
    pub path: PathBuf,
    /// PDF date string.
    pub at: String,
}

/// A node of the history tree: a version of the document, and the edits
/// that made it from its parent (the edge into it). See `todos/m9.md`,
/// decision 16.
struct Version {
    parent: Option<VersionId>,
    /// Oldest first.
    children: Vec<VersionId>,
    /// The child redo goes to: the one last left by undo, or made last.
    redo_to: Option<VersionId>,
    label: String,
    author: String,
    /// When it was made: a PDF date string.
    at: String,
    /// The edits from the parent; empty for the root. A job's later
    /// batches add to its version's steps.
    steps: Vec<Step>,
    /// The registrations it holds: the files registered since the parent
    /// (inserted ones, and the file a save wrote), which its steps and
    /// state refer to; the first version holds those of the versions
    /// pruned before it too. They go when it does (see `remove_version`).
    sources: Vec<SourceId>,
    /// The state, unless trimming dropped it; then a jump rebuilds it by
    /// applying the steps' prims from the nearest ancestor that kept one.
    state: Option<DocState>,
    /// Rebuilding it failed (see [`Damaged`]), so it can't be reached.
    damaged: bool,
    /// What its snapshot holds that no other version's does, which
    /// trimming frees, and what its steps hold, which only pruning the
    /// version does (see [`accounting`]).
    snapshot_bytes: usize,
    step_bytes: usize,
    /// The job that produced this version, if it may still amend it.
    job: Option<JobId>,
    /// Saves that wrote it, oldest first.
    saves: Vec<Saved>,
}

/// A version as the History panel shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct VersionView {
    pub id: VersionId,
    pub label: String,
    pub author: String,
    /// PDF date string.
    pub at: String,
    /// What its edits did, a line each.
    pub edits: Vec<String>,
    pub saves: Vec<Saved>,
    pub current: bool,
    /// The version the file on disk holds.
    pub saved: bool,
    /// Past the current version: undone, and reachable with redo.
    pub undone: bool,
    /// First versions of other branches from here: edits that were undone
    /// and then replaced by others.
    pub branches: Vec<VersionId>,
    /// Its snapshot was dropped to save memory; jumping there replays it.
    pub replays: bool,
    /// Its replay failed, so it can't be reached (see [`Damaged`]).
    pub damaged: bool,
}

/// Everything a save needs, detached from the document so it can run on
/// another thread while editing continues.
#[derive(Clone)]
pub struct Snapshot {
    pub state: DocState,
    pub primary: SourceId,
    pub sources: HashMap<SourceId, Arc<SourceInfo>>,
    /// The outline differs from what's in the primary file.
    pub outline_dirty: bool,
    /// The page labels differ from what's in the primary file.
    pub labels_dirty: bool,
    /// The header/footer differs from what's drawn in the primary file:
    /// it's drawn over the pages on screen, and a save writes it.
    pub header_footer_dirty: bool,
    /// Pages whose markup differs from what's in the primary file.
    pub annots_dirty: HashSet<PageId>,
    /// Fields that differ from what's in the primary file (see
    /// `Document::fields_dirty`).
    pub fields_dirty: HashSet<FieldId>,
    /// Something in the primary file is gone from the document (see
    /// `Document::removes_content`): only a full rewrite takes it out of
    /// the file.
    pub removes_content: bool,
    /// The form differs from the primary file's (see
    /// `Document::form_changed`): a save takes out what described it as
    /// it was, its XFA copy and usage rights (M10 proposals 6 and 7).
    pub form_changed: bool,
    /// Each source's marks the open gave new ids (see
    /// `Document::set_source_renamed`).
    pub(crate) renamed: HashMap<SourceId, Arc<Renames>>,
    /// What the old writers read, built the first time one asks (see
    /// [`legacy`](Self::legacy)).
    pub(crate) legacy: OnceLock<Arc<LegacyDocState>>,
}

/// A source's marks the open gave new ids ([`Renamed`]): by page index,
/// the id each had, which the page's `/NM` says, and the one it has.
pub(crate) type Renames = HashMap<u32, Vec<(AnnotId, AnnotId)>>;

impl Snapshot {
    pub fn source(&self, id: SourceId) -> Option<&Arc<SourceInfo>> {
        self.sources.get(&id)
    }

    /// [`Document::geom`].
    pub fn geom(&self, page: PageId) -> Geom {
        geom_in(&self.state, page)
    }

    /// [`Document::render_delta`].
    pub fn render_delta(&self, page: PageId) -> Rotation {
        self.state.render_delta(page).unwrap_or(Rotation::R0)
    }

    /// [`Document::group_members`].
    pub fn group_members(&self, group: &crate::annot::Group) -> Vec<(usize, PageId, Annot)> {
        group_members_in(&self.state, group)
    }

    /// The marks on page `index` of `source` the open gave new ids: the id
    /// each had, which the page's `/NM` says, and the one it has.
    pub(crate) fn renamed_on(&self, source: SourceId, index: u32) -> &[(AnnotId, AnnotId)] {
        self.renamed
            .get(&source)
            .and_then(|r| r.get(&index))
            .map_or(&[], Vec::as_slice)
    }

    /// The state in the shape the old writers read it (M10.5 slice 8's
    /// legacy projection), built from the records on first use.
    pub(crate) fn legacy(&self) -> &LegacyDocState {
        self.legacy
            .get_or_init(|| Arc::new(LegacyDocState::of(self)))
    }
}

/// Every mark of `state` in `group`, in document order: by page, then
/// down the page as it shows, then across.
fn group_members_in(state: &DocState, group: &crate::annot::Group) -> Vec<(usize, PageId, Annot)> {
    let in_group = |a: &&Annot| crate::annot::group_of(&a.kind).as_ref() == Some(group);
    let mut out = Vec::new();
    for (i, (id, _, geom)) in state.pages().enumerate() {
        let on_page: Vec<&Annot> = state.marks_on(id).filter(in_group).collect();
        if on_page.is_empty() {
            continue;
        }
        let to_view = geom.to_view();
        let mut shown: Vec<(crate::annot::Box2, &Annot)> = on_page
            .into_iter()
            .map(|a| (to_view.apply_box(a.page_bounds()), a))
            .collect();
        shown.sort_by(|(a, _), (b, _)| a.y.total_cmp(&b.y).then(a.x.total_cmp(&b.x)));
        out.extend(shown.into_iter().map(|(_, a)| (i, id, a.clone())));
    }
    out
}

/// Page `page`'s boxes and `/Rotate`: a Letter page's when `state` hasn't
/// got it.
fn geom_in(state: &DocState, page: PageId) -> Geom {
    state
        .geom(page)
        .map_or_else(|| Geom::read(None, None, 0), |g| **g)
}

/// Where page, bookmark and field ids come from. The counters only go up,
/// so an id is never handed out twice, whatever undo does. Edits draw from
/// it; a replay applies their prims, which hold the ids, and draws nothing.
#[derive(Default)]
struct IdAlloc {
    page: AtomicU64,
    outline: AtomicU64,
    field: AtomicU64,
}

impl IdAlloc {
    fn page(&self) -> PageId {
        PageId(self.page.fetch_add(1, Ordering::Relaxed))
    }

    fn outline(&self) -> OutlineId {
        OutlineId(self.outline.fetch_add(1, Ordering::Relaxed))
    }

    fn field(&self) -> FieldId {
        FieldId(self.field.fetch_add(1, Ordering::Relaxed))
    }

    /// Field ids from `next` on: a file's fields keep the ids it read them
    /// with.
    fn fields_from(&self, next: u64) {
        self.field.fetch_max(next, Ordering::Relaxed);
    }
}

pub struct Document {
    /// The blobs its sources are in, outside undo.
    store: Arc<Store>,
    sources: HashMap<SourceId, Arc<SourceInfo>>,
    primary: SourceId,
    versions: HashMap<VersionId, Version>,
    root: VersionId,
    cursor: VersionId,
    /// The version the primary file on disk holds, if any.
    saved: Option<VersionId>,
    next_version: u64,
    /// Sources registered since the last version was made, for the next.
    unlogged_sources: Vec<SourceId>,
    /// Who makes the versions from now on (the app sets it).
    author: String,
    ids: IdAlloc,
    next_source: u32,
    policy: Box<dyn HistoryPolicy>,
    next_job: u64,
    /// What the primary file on disk holds: the opened version's state,
    /// then each save's. The dirty flags compare the cursor's
    /// annotations, form, bookmarks, page labels and header/footer (the
    /// one the file has drawn) with it, until slice 9, whose saves know
    /// what changed from the prims.
    baseline: DocState,
    /// Each source's marks the open gave new ids (`set_source_renamed`),
    /// which its pages name by the old ones until a save writes them.
    renamed: HashMap<SourceId, Arc<Renames>>,
    /// The reader has been told some text shows as `?` in the file
    /// ([`Document::unencodable_notice`]). Outside history: undo doesn't
    /// make it news again.
    told_unencodable: bool,
    /// The opened version holds the file's facts: until then edits are
    /// refused (see [`NotReady`]).
    ready: bool,
}

impl Document {
    /// A document showing `primary`, a source in `store`, in its
    /// entirety, with `outline`, what its pages hold beside their content
    /// (its source's [`annots`](crate::Source::annots)), and nothing else:
    /// a file sheaf has just made, which says nothing more about itself.
    /// A file that's opened opens through [`crate::open`], which reads
    /// what it says.
    pub fn new(
        store: Arc<Store>,
        primary: SourceInfo,
        outline: &[FlatOutlineItem],
        policy: Box<dyn HistoryPolicy>,
    ) -> Self {
        let annots = primary.src.annots().clone();
        let mut doc = Self::loading(store, primary, policy);
        let facts = Facts {
            annots,
            outline: outline.to_vec(),
            ..Default::default()
        };
        let (state, _) = doc.opened(facts);
        let root = doc.root;
        doc.versions
            .get_mut(&root)
            .expect("the opened version")
            .state = Some(state);
        doc.ready = true;
        doc
    }

    /// A document showing `primary` while it opens: its pages, from the
    /// page table its source read as it opened, and nothing else, until
    /// [`ready`](Self::ready). See [`crate::open::start`].
    pub(crate) fn loading(
        store: Arc<Store>,
        primary: SourceInfo,
        policy: Box<dyn HistoryPolicy>,
    ) -> Self {
        let source = SourceId(0);
        let mut doc = Self {
            store,
            sources: HashMap::new(),
            primary: source,
            versions: HashMap::new(),
            root: VersionId(0),
            cursor: VersionId(0),
            saved: Some(VersionId(0)),
            next_version: 1,
            unlogged_sources: Vec::new(),
            author: crate::annot::default_author(),
            ids: IdAlloc::default(),
            next_source: 1,
            policy,
            next_job: 1,
            baseline: DocState::builder().build(),
            renamed: HashMap::new(),
            told_unencodable: false,
            ready: false,
        };
        doc.debug_assert_ours(&primary);
        // A page the tree lists again is a copy of the first.
        let src = &primary.src;
        let pages = src.pages().iter().zip(0..).map(|(facts, index)| {
            let base = PageBase {
                source,
                src: src.clone(),
                index,
            };
            let def = PageDef {
                base,
                copy: facts.repeat,
                content: ContentStack::empty(),
            };
            (doc.ids.page(), Arc::new(def), Arc::new(facts.geom))
        });
        let state = DocState::builder().pages(pages).build();
        doc.baseline = state.clone();
        doc.sources.insert(source, Arc::new(primary));
        doc.versions.insert(
            VersionId(0),
            Version {
                parent: None,
                children: Vec::new(),
                redo_to: None,
                label: "Open".into(),
                author: String::new(),
                at: crate::annot::pdf_date_now(),
                steps: Vec::new(),
                sources: vec![source],
                state: Some(state),
                damaged: false,
                snapshot_bytes: 0,
                step_bytes: 0,
                job: None,
                saves: Vec::new(),
            },
        );
        doc
    }

    /// The file has opened: `facts`, as [`crate::open::finish`] read them,
    /// make the opened version complete. It's a new version in place of
    /// the one with the pages alone, which no longer resolves; the current
    /// and saved versions are this one, so the document isn't dirty and
    /// there's nothing to undo. Gives what the view shows beside the
    /// document. Only once, while the file opens: there are no late facts
    /// (M10.5 proposal 28).
    pub fn ready(&mut self, facts: Facts) -> Result<ViewFacts> {
        if self.ready {
            bail!("the file has opened already");
        }
        let (state, view) = self.opened(facts);
        let old = self.root;
        let mut v = self.versions.remove(&old).expect("the opened version");
        v.state = Some(state);
        self.replace(old, v);
        self.ready = true;
        Ok(view)
    }

    /// Whether the file's facts are in: until then edits are refused.
    pub fn is_ready(&self) -> bool {
        self.ready
    }

    /// The opened version's state: its pages, and what `facts` say about
    /// them, each page index mapped to its page once. That's what the
    /// primary file holds, so the baseline is set from it. Gives back what
    /// the state doesn't hold.
    fn opened(&mut self, facts: Facts) -> (DocState, ViewFacts) {
        let loading = self.state().clone();
        let by_index: Vec<PageId> = loading.order().iter().copied().collect();
        let at = |index: usize| by_index.get(index).copied();
        let src = self.sources[&self.primary].src.clone();
        let (roots_outline, bookmarks) =
            self.outline_from_flat(&facts.outline, |index| at(index as usize), &src);
        let bookmarks = bookmarks
            .into_iter()
            .map(|(id, b)| (id, Arc::new(b)))
            .collect();
        // Each page's annotations as records, each the object it was read
        // as, by their places in its `/Annots`; a page the tree lists again
        // holds copies of the first listing's.
        let store = &self.store;
        let mut annots: HashMap<PageId, Vec<(usize, AnnotId, AnnotBody)>> = (facts.annots.pages)
            .iter()
            .filter_map(|(&index, list)| {
                let page = at(index as usize)?;
                let copy = loading.def(page)?.copy;
                let list = list.iter().map(|a| {
                    let (id, body) = record_of(store, a, &src, copy);
                    (a.slot, id, body)
                });
                Some((page, list.collect()))
            })
            .collect();
        let labels: Option<Arc<[crate::labels::LabelSpec]>> = facts
            .labels
            .map(|ranges| crate::labels::to_specs(&ranges, &by_index).into())
            .filter(|specs: &Arc<[_]>| !specs.is_empty());
        // The header/footer sheaf drew, as the generator that draws it, its
        // id drawn from the file's hash, so the same bytes open the same.
        let hash = src.id().hash;
        let mut rng = fastrand::Rng::with_seed(hash as u64 ^ (hash >> 64) as u64);
        let generator = facts.header_footer.map(|(mut hf, scope)| {
            hf.pages = scope.map(|ix| ix.into_iter().filter_map(at).collect());
            Arc::new(Generator {
                id: GenId::draw(&mut rng, |_| false),
                spec: GenSpec::HeaderFooter(Arc::new(hf)),
            })
        });
        // The form: its nodes as the file numbers them, and its widgets,
        // their ids drawn on from the same rng, none another annotation's.
        let form = &facts.form;
        let inst = FormInst {
            src: self.primary,
            copy: 0,
        };
        let mut taken: HashSet<AnnotId> =
            (annots.values().flatten()).map(|&(_, id, _)| id).collect();
        let mut shows: HashMap<FieldId, Vector<AnnotId>> = HashMap::new();
        for (&index, list) in &form.widgets {
            let Some(page) = at(index as usize) else {
                continue;
            };
            for w in list {
                let id = crate::open::annots::draw(&mut rng, &mut taken);
                shows.entry(w.field).or_default().push_back(id);
                let src = Some(ObjRef {
                    src: src.clone(),
                    id: w.object,
                });
                let body = AnnotBody::Widget(Arc::new(Widget::read(w, w.field, src, inst)));
                annots.entry(page).or_default().push((w.slot, id, body));
            }
        }
        let fields = nodes_of(form, &src, inst, shows);
        self.ids.fields_from(form.nodes.len() as u64);
        let annots = annots.into_iter().map(|(page, mut list)| {
            list.sort_by_key(|&(slot, _, _)| slot);
            (
                page,
                list.into_iter().map(|(_, id, body)| (id, body)).collect(),
            )
        });
        let renamed: Vec<(PageId, AnnotId)> = (facts.annots.renamed.iter())
            .filter_map(|r| Some((at(r.page as usize)?, r.was)))
            .collect();
        self.set_source_renamed(self.primary, &facts.annots.renamed);
        let view = ViewFacts {
            renamed,
            attachments: facts.attachments,
            collection: facts.collection,
            layers: facts.layers,
            form: form.facts.clone(),
        };
        let roots = (fields.iter())
            .filter(|(_, node)| node.parent.is_none())
            .map(|&(id, _)| id)
            .collect();
        let names = match fields.is_empty() {
            true => im::OrdMap::new(),
            false => im::OrdMap::unit(inst, file_name(&self.sources[&self.primary])),
        };
        let form_doc = FormDoc {
            roots,
            names,
            scripts: ScriptState::On,
        };
        let pages = loading
            .pages()
            .map(|(id, def, geom)| (id, def.clone(), geom.clone()));
        let state = DocState::builder()
            .pages(pages)
            .outline(roots_outline, bookmarks)
            .annots(annots)
            .labels(labels)
            .generators(generator)
            .form(fields, Arc::new(form_doc))
            .build();
        // The file says other ids for the marks the open gave new ones:
        // their pages are written at the next save, of either kind.
        let mut baseline = state.clone();
        for &(page, _) in &view.renamed {
            for id in state.annot_ids_on(page).into_iter().flatten() {
                baseline
                    .set(&Key::Annot(*id), None)
                    .expect("an annotation's key");
            }
            baseline
                .set(&Key::PageAnnots(page), None)
                .expect("a page's annotations' key");
        }
        self.baseline = baseline;
        debug_assert_eq!(state.check_invariants(), Ok(()));
        (state, view)
    }

    /// Who the versions made from now on are by.
    pub fn set_author(&mut self, name: impl Into<String>) {
        self.author = name.into();
    }

    pub fn primary(&self) -> SourceId {
        self.primary
    }

    /// Where its sources' bytes are: what a registration's source is
    /// interned into.
    pub fn store(&self) -> &Arc<Store> {
        &self.store
    }

    pub fn source(&self, id: SourceId) -> Option<&Arc<SourceInfo>> {
        self.sources.get(&id)
    }

    pub fn sources(&self) -> impl Iterator<Item = (SourceId, &Arc<SourceInfo>)> {
        self.sources.iter().map(|(k, v)| (*k, v))
    }

    /// Register another file so its pages can be inserted. The next
    /// version records it, with the edit that uses it, and the
    /// registration goes when that version is pruned: an edit on another
    /// branch registers the file again.
    pub fn add_source(&mut self, info: SourceInfo) -> SourceId {
        let id = self.register_source(info);
        self.unlogged_sources.push(id);
        id
    }

    fn register_source(&mut self, info: SourceInfo) -> SourceId {
        self.debug_assert_ours(&info);
        let id = SourceId(self.next_source);
        self.next_source += 1;
        self.sources.insert(id, Arc::new(info));
        id
    }

    /// Forget registrations whose version is gone, and their renamed
    /// marks: a source goes with the last registration holding it, and an
    /// engine opened on it keeps only its bytes, until slice 13's engine
    /// pool. The primary stays while it is the primary.
    fn unregister(&mut self, ids: &[SourceId]) {
        for id in ids.iter().filter(|&&id| id != self.primary) {
            self.sources.remove(id);
            self.renamed.remove(id);
        }
    }

    /// A registration's source is a blob of this document's store: the
    /// store is what counts its bytes.
    fn debug_assert_ours(&self, info: &SourceInfo) {
        debug_assert!(
            self.store.holds(info.src.blob()),
            "{} is in another store",
            info.name.display()
        );
    }

    /// Source `id`'s marks the open gave new ids, as `open::finish` read
    /// them: its pages name them by their old ids until a save writes
    /// them, and a save follows other tools' replies to them through these
    /// (see `save::write_page_annots`). Call it before the edit that brings
    /// its pages.
    pub fn set_source_renamed(&mut self, id: SourceId, renamed: &[Renamed]) {
        let mut by_page = Renames::new();
        for r in renamed {
            by_page.entry(r.page).or_default().push((r.was, r.id));
        }
        if !by_page.is_empty() {
            self.renamed.insert(id, Arc::new(by_page));
        }
    }

    /// Where page stamp `stem`'s page is, as an edit that brings a mark
    /// naming it finds it: the store's blob of that name, held by whoever
    /// placed or pasted it; else the form a registered file's marks draw it
    /// with, the primary's first; else, until slice 19, the generated
    /// cache's file of that name, read into the store.
    pub fn find_stamp(&self, stem: &str) -> Option<FormRef> {
        find_stamp(&self.store, &self.sources, self.primary, stem)
    }

    /// Where attachment `stem`'s file is, `size` bytes long, as an edit
    /// that brings a mark naming it finds it: the store's blob of that
    /// name, held by whoever attached or pasted it; else the file a
    /// registered file's marks embed under it, the primary's first,
    /// decoded into the store.
    pub fn find_file(&self, stem: &str, size: u64) -> Option<BlobRef> {
        find_file(&self.store, &self.sources, self.primary, stem, size)
    }

    /// `annot` as a mark, `src` the object it was read as, holding what it
    /// names by stem: its page stamp's page or its attachment's file, as
    /// [`find_stamp`](Self::find_stamp) and [`find_file`](Self::find_file)
    /// find them. One that can't be found is an error.
    fn mark(&self, annot: Annot, src: Option<ObjRef>) -> Result<Mark> {
        let mut mark = Mark {
            src,
            ..Mark::from(annot)
        };
        match &mark.annot.kind {
            AnnotKind::PageStamp { stamp, name, .. } => {
                let form = self.find_stamp(stamp);
                let missing =
                    || format!("the stamp {name} isn't in the document or on this machine");
                mark.stamp = Some(form.with_context(missing)?);
            }
            AnnotKind::FileAttachment {
                file, name, size, ..
            } => {
                let found = self.find_file(file, *size);
                let missing = || format!("the attached file {name} isn't in the document");
                mark.file = Some(found.with_context(missing)?);
            }
            _ => {}
        }
        Ok(mark)
    }

    /// The registrations whose form instances in the document have
    /// scripts: their fields' or widgets', or document scripts beside
    /// them. More than one, and scripts are off by default (M10 proposal
    /// 5): document scripts share one scope.
    pub fn script_sources(&self) -> Vec<SourceId> {
        let scripted = self.scripted(self.state());
        let files: BTreeSet<SourceId> = scripted.iter().map(|inst| inst.src).collect();
        files.into_iter().collect()
    }

    /// The form instances in `st` with scripts: a field's own, a
    /// widget's, or document scripts in the file it came from.
    fn scripted(&self, st: &DocState) -> BTreeSet<FormInst> {
        let mut out: BTreeSet<FormInst> = (st.form().names.keys())
            .filter(|inst| {
                (self.sources.get(&inst.src))
                    .is_some_and(|s| s.src.form().facts.document_scripts > 0)
            })
            .copied()
            .collect();
        for (_, node, term) in st.terminals() {
            let mut widgets = term.widgets.iter().filter_map(|&w| st.annot(w)?.widget());
            if !out.contains(&node.inst) && (term.has_scripts() || widgets.any(|w| w.has_scripts()))
            {
                out.insert(node.inst);
            }
        }
        out
    }

    /// Scripts off, when the form's came in from more than one file now
    /// (M10 proposal 5): they share one scope. Once off they stay off,
    /// across saves too, until undo takes back what brought them.
    fn scripts_apart(&self, rec: &mut Rec) {
        if rec.form().scripts != ScriptState::On {
            return;
        }
        let scripted = self.scripted(rec);
        let files: BTreeSet<SourceId> = scripted.iter().map(|inst| inst.src).collect();
        if files.len() > 1 {
            let form = FormDoc {
                scripts: ScriptState::Off {
                    instances: scripted.into_iter().collect(),
                },
                ..FormDoc::clone(rec.form())
            };
            rec.put(Key::Form, Some(Val::Form(Arc::new(form))));
        }
    }

    /// Whether the document's scripts came from more than one file: then
    /// they're off by default (M10 proposal 5).
    pub fn scripts_mixed(&self) -> bool {
        self.state().form().scripts != ScriptState::On
    }

    /// What to tell the reader about the forms an edit brought: fields put
    /// under a new parent (a copy's, after Duplicate Pages, when `copies`),
    /// whether their scripts still name the old fields, and scripts turned
    /// off when `mixed_before` (`scripts_mixed` before the edit) wasn't.
    pub fn form_notice(
        &self,
        out: &EditOutcome,
        copies: bool,
        mixed_before: bool,
    ) -> Option<String> {
        let st = self.state();
        let mut parts = Vec::new();
        for (under, sid) in &out.forms_renamed {
            let file = self
                .sources
                .get(sid)
                .map(|s| file_name(s))
                .unwrap_or_default();
            let documents =
                (self.sources.get(sid)).is_some_and(|s| s.src.form().facts.document_scripts > 0);
            // The fields under the new parent, and their own scripts.
            let scripted = (st.field_named(under).into_iter())
                .flat_map(|top| field_subtree(st, top))
                .filter_map(|id| st.field(id)?.term.clone())
                .any(|t| documents || t.has_scripts());
            let mut part = if copies {
                format!("the copies' fields are under “{under}”, apart from the originals")
            } else {
                format!("fields from {file} are under “{under}”, since their names were taken")
            };
            if scripted {
                part.push_str("; its scripts still use the old names");
            }
            parts.push(part);
        }
        for sid in &out.signatures_in {
            let file = self
                .sources
                .get(sid)
                .map(|s| file_name(s))
                .unwrap_or_default();
            parts.push(format!("the signature from {file} is no longer valid here"));
        }
        if !mixed_before && self.scripts_mixed() {
            parts.push("scripts are off: fields from more than one form have them".into());
        }
        if parts.is_empty() {
            return None;
        }
        let sentences: Vec<String> = parts
            .iter()
            .map(|p| {
                let mut chars = p.chars();
                chars
                    .next()
                    .map(|c| c.to_uppercase().chain(chars).collect())
                    .unwrap_or_default()
            })
            .collect();
        Some(sentences.join(". "))
    }

    /// What to tell the reader when an edit brought text the built-in
    /// fonts can't draw ([`EditOutcome::unencodable`]): once a document,
    /// the first time, and never again after.
    pub fn unencodable_notice(&mut self, out: &EditOutcome) -> Option<String> {
        if !out.unencodable || self.told_unencodable {
            return None;
        }
        self.told_unencodable = true;
        Some(
            "Some characters can't be drawn in the built-in fonts and show as “?” in the file; \
             the text itself is kept"
                .into(),
        )
    }

    /// The header/footer differs from what the primary file has drawn.
    pub fn header_footer_dirty(&self) -> bool {
        match (self.state().header_footer(), self.baseline.header_footer()) {
            (Some(now), Some(drawn)) => !Arc::ptr_eq(now, drawn) && now != drawn,
            (now, drawn) => now.is_some() || drawn.is_some(),
        }
    }

    /// Terminal fields that differ from what's in the primary file:
    /// changed, added or gone, as the old writers read them (their
    /// widgets aside: a page's widgets gone isn't a field changed).
    pub fn fields_dirty(&self) -> HashSet<FieldId> {
        let (st, base) = (self.state(), &self.baseline);
        (st.fields_changed_from(base).into_iter())
            .filter(|&id| legacy_field(st, id) != legacy_field(base, id))
            .collect()
    }

    /// Pages whose annotations differ from what's in the primary file
    /// (`changed`, from [`DocState::annots_changed_from`]): a mark or
    /// another tool's comment added, changed or gone, or the page's list
    /// in another order.
    fn annots_dirty_pages(&self, changed: &HashSet<PageId>) -> HashSet<PageId> {
        let cur = self.state();
        let mut dirty = changed.clone();
        // A changed connector changes its group's cycle of /Dest: every
        // page with one of its label is written again.
        let label = |a: &Annot| match &a.kind {
            AnnotKind::Connector { label, .. } => Some(label.clone()),
            _ => None,
        };
        let labels: HashSet<String> = (dirty.iter())
            .flat_map(|&page| cur.marks_on(page).chain(self.baseline.marks_on(page)))
            .filter_map(label)
            .collect();
        if !labels.is_empty() {
            dirty.extend(
                cur.marks()
                    .filter(|(_, a)| label(a).is_some_and(|l| labels.contains(&l)))
                    .map(|(page, _)| page),
            );
        }
        dirty
    }

    pub fn state(&self) -> &DocState {
        self.versions[&self.cursor]
            .state
            .as_ref()
            .expect("the current version keeps its state")
    }

    pub fn snapshot(&self) -> Snapshot {
        let st = self.state();
        let changed = st.annots_changed_from(&self.baseline);
        let fields_dirty = self.fields_dirty();
        Snapshot {
            state: st.clone(),
            primary: self.primary,
            sources: self.sources.clone(),
            outline_dirty: !st.outline_eq(&self.baseline),
            labels_dirty: !st.labels_eq(&self.baseline),
            header_footer_dirty: self.header_footer_dirty(),
            annots_dirty: self.annots_dirty_pages(&changed),
            form_changed: !fields_dirty.is_empty() || self.widgets_changed(&changed),
            fields_dirty,
            removes_content: self.removes_content(),
            renamed: self.renamed.clone(),
            legacy: OnceLock::new(),
        }
    }

    /// Whether the form differs from the primary file's: a field added,
    /// removed or changed, or widgets of a page not as they were (it's
    /// gone, or shows another file's page now). Then a save takes out what
    /// described the form as it was: its XFA copy and Reader's usage rights
    /// (M10 proposals 6 and 7).
    pub fn form_changed(&self) -> bool {
        !self.fields_dirty().is_empty()
            || self.widgets_changed(&self.state().annots_changed_from(&self.baseline))
    }

    /// Whether a page of the primary file's with widgets hasn't them as
    /// they were: of `changed`, the pages whose annotations aren't the
    /// baseline's.
    fn widgets_changed(&self, changed: &HashSet<PageId>) -> bool {
        let (st, base) = (self.state(), &self.baseline);
        changed
            .iter()
            .any(|&page| base.widgets_on(page).next().is_some() && !st.widgets_eq_on(base, page))
    }

    /// Whether something in the primary file is gone from the document: a
    /// page (deleted, or shown from another source now, as a crop makes
    /// it), a mark or reply, another tool's comment, a bookmark. An
    /// incremental save would keep it in the file's earlier revision (M9
    /// bug 14). Undoing back to what was saved clears it.
    pub fn removes_content(&self) -> bool {
        let st = self.state();
        if let Some(primary) = self.sources.get(&self.primary) {
            let shown: HashSet<u32> = st
                .pages()
                .filter(|(_, def, _)| def.base.source == self.primary)
                .map(|(_, def, _)| def.base.index)
                .collect();
            if (0..primary.src.page_count()).any(|i| !shown.contains(&i)) {
                return true;
            }
        }
        !st.keeps_annots_of(&self.baseline) || !st.keeps_bookmarks_of(&self.baseline)
    }

    /// After a successful save of the current version to a file described by
    /// `saved`: register it as a new source and make the current version an
    /// identity mapping over it, keeping `PageId`s (and bookmarks) intact so
    /// the view doesn't flicker. Older versions still reference the old
    /// sources, so undo across a save keeps working. Each page is the saved
    /// file's page at its position, its own object (copy 0), with its boxes
    /// and `/Rotate` as the file has them, which are the page's. OCR text is
    /// in the file now, so nothing is drawn over the pages. Each mark is the
    /// object on its page with its `/NM`, each of the other tools'
    /// annotations the one there with its name and subtype (copy 0), each
    /// field the saved file's own, the node with its full name, each
    /// widget the one at its place among its field's on its page, and each
    /// bookmark the item at its place in pre-order, as the save wrote them;
    /// another tool's annotation the save left out (a link to a page that
    /// isn't there) goes, and a bookmark deeper than the file is read is
    /// from no item; a page stamp draws the form the saved file holds for
    /// it; the header/footer is drawn there now, and stays, as do scripts
    /// off.
    ///
    /// The content is the same, so this isn't an edit and makes no step,
    /// but the state is expressed anew, so the version gets a new id under
    /// the same parent (see [`replace`](Self::replace)). Its steps still
    /// lead to its state as it was before the save; it keeps its snapshot,
    /// as do its children (see `keeps_snapshot`), so nothing is ever
    /// replayed across a save. The new version holds the saved file's
    /// registration. M10.5 slice 8 adapts it to the records; slice 9
    /// deletes it.
    pub fn rebase_to_saved(&mut self, saved: SourceInfo) -> SourceId {
        debug_assert!(self.is_ready(), "a save of a file still opening");
        let path = saved.name.clone();
        let name = file_name(&saved);
        let src = saved.src.clone();
        let id = self.register_source(saved);
        let replaced = std::mem::replace(&mut self.primary, id);
        // Versions made from it before (and undone) were made from it as
        // it was, so their prims don't fit it re-expressed: they keep their
        // states (see `keeps_snapshot`). A damaged one stays out of reach.
        for child in self.versions[&self.cursor].children.clone() {
            let _ = self.ensure_state(child);
        }
        let old = self.cursor;
        let mut v = self.versions.remove(&old).expect("current version");
        v.sources.push(id);
        v.saves.push(Saved {
            path,
            at: crate::annot::pdf_date_now(),
        });
        let st = v
            .state
            .as_mut()
            .expect("the current version keeps its state");
        for (index, page) in (0..).zip(st.order().clone()) {
            let base = PageBase {
                source: id,
                src: src.clone(),
                index,
            };
            let def = PageDef {
                base,
                copy: 0,
                content: ContentStack::empty(),
            };
            let geom = Arc::new(src.geom(index));
            st.set(&Key::Page(page), Some(Val::Page(Arc::new(def))))
                .expect("a page key holds a page");
            st.set(&Key::Geom(page), Some(Val::Geom(geom)))
                .expect("a geom key holds a geom");
        }
        // The saved items as the reader lists them, with none deeper than
        // it reads: a bookmark past that depth is from no item, and once
        // one doesn't agree with its item, neither are the rest.
        let items = crate::open::outline(&src);
        let mut items = items.iter();
        let mut agree = true;
        let rebound: Vec<(OutlineId, Arc<Bookmark>)> = (st.bookmarks_preorder().into_iter())
            .map(|(depth, id, b)| {
                let item = (agree && depth <= crate::open::MAX_DEPTH)
                    .then(|| items.next())
                    .flatten();
                if let Some(item) = item
                    && (item.depth != depth || item.title != crate::open::shown_title(&b.title))
                {
                    log::warn!(
                        "the saved file's bookmark {:?} isn't {:?}",
                        item.title,
                        b.title
                    );
                    agree = false;
                }
                let from = item.filter(|_| agree).and_then(|item| {
                    Some(ObjRef {
                        src: src.clone(),
                        id: item.object?,
                    })
                });
                let b = Bookmark {
                    src: from,
                    ..b.clone()
                };
                (id, Arc::new(b))
            })
            .collect();
        for (id, b) in &rebound {
            st.set(&Key::Bookmark(*id), Some(Val::Bookmark(b.clone())))
                .expect("a bookmark key holds a bookmark");
        }
        let written = src.annots().clone();
        let mut gone = Vec::new();
        let mut annots = rebind_annots(st, &src, &written, &mut gone);
        let fields = rebind_fields(st, id, &src, name, &mut annots);
        debug_assert_eq!(st.check_invariants(), Ok(()));
        // The pages, annotations, form, bookmarks and fields expressed
        // anew are its snapshot's alone: their values, and the im nodes
        // put to hold them.
        let own = v.steps.iter().flat_map(|s| s.prims.iter().map(|p| p.key));
        let anew = (st.order().iter())
            .flat_map(|&page| [Key::Page(page), Key::Geom(page)])
            .chain(annots.iter().map(|&(id, _)| Key::Annot(id)))
            .chain((gone.iter()).flat_map(|&(page, id)| [Key::Annot(id), Key::PageAnnots(page)]))
            .chain(rebound.iter().map(|&(id, _)| Key::Bookmark(id)))
            .chain(fields.iter().map(|&(id, _)| Key::Field(id)));
        v.snapshot_bytes = st.snapshot_bytes(own.chain(anew))
            + st.pages()
                .map(|(_, def, _)| size_of::<PageDef>() + def.heap_size() + size_of::<Geom>())
                .sum::<usize>()
            + annots.iter().map(|(_, a)| a.heap_size()).sum::<usize>()
            + rebound
                .iter()
                .map(|(_, b)| size_of::<Bookmark>() + b.heap_size())
                .sum::<usize>()
            + (fields.iter())
                .map(|(_, node)| size_of::<FieldNode>() + node.heap_size())
                .sum::<usize>()
            + size_of::<FormDoc>()
            + st.form().heap_size();
        let new = self.replace(old, v);
        // The primary it replaces stayed only as the primary when pruning
        // took the version that held it: nothing refers to it now.
        let held = self
            .versions
            .values()
            .any(|v| v.sources.contains(&replaced));
        if !held {
            self.unregister(&[replaced]);
        }
        self.baseline = self.state().clone();
        self.saved = Some(new);
        id
    }

    pub fn page_count(&self) -> usize {
        self.state().page_count()
    }

    /// The page at position `index`.
    pub fn page(&self, index: usize) -> Option<PageId> {
        self.state().page_at(index)
    }

    pub fn index_of(&self, id: PageId) -> Option<usize> {
        self.state().pos_of(id)
    }

    /// Page `id`'s boxes and `/Rotate`: a Letter page's for a page the
    /// document hasn't got.
    pub fn geom(&self, id: PageId) -> Geom {
        geom_in(self.state(), id)
    }

    /// The turn PDFium adds to page `id`'s source page's own `/Rotate`:
    /// what a render `Job` takes.
    pub fn render_delta(&self, id: PageId) -> Rotation {
        self.state().render_delta(id).unwrap_or(Rotation::R0)
    }

    /// Page `id`'s size as shown.
    pub fn page_size(&self, id: PageId) -> PageSize {
        self.geom(id).shown()
    }

    /// Every mark in `group` (see `annot::group_of`), in document order:
    /// by page, then down the page as it shows, then across. With each,
    /// its page's position.
    pub fn group_members(&self, group: &crate::annot::Group) -> Vec<(usize, PageId, Annot)> {
        group_members_in(self.state(), group)
    }

    pub fn is_dirty(&self) -> bool {
        self.saved != Some(self.cursor)
    }

    pub fn mark_saved(&mut self) {
        self.saved = Some(self.cursor);
    }

    /// Treat the current state as unsaved even though nothing was edited
    /// (a recovered document: its primary source is a temporary file).
    pub fn mark_unsaved(&mut self) {
        self.saved = None;
    }

    pub fn can_undo(&self) -> bool {
        self.versions[&self.cursor].parent.is_some()
    }

    pub fn can_redo(&self) -> bool {
        self.redo_target().is_some()
    }

    /// Label of the edit that `undo` would revert.
    pub fn undo_label(&self) -> Option<&str> {
        let v = &self.versions[&self.cursor];
        v.parent.map(|_| v.label.as_str())
    }

    pub fn redo_label(&self) -> Option<&str> {
        self.redo_target()
            .map(|id| self.versions[&id].label.as_str())
    }

    /// Where redo goes: the child last left by undo (or made last).
    fn redo_target(&self) -> Option<VersionId> {
        let v = &self.versions[&self.cursor];
        v.redo_to
            .filter(|c| v.children.contains(c))
            .or_else(|| v.children.last().copied())
    }

    /// Back to the parent version: the label of the edit undone, or None
    /// with nothing to undo.
    pub fn undo(&mut self) -> Result<Option<&str>, Damaged> {
        let from = self.cursor;
        let Some(parent) = self.versions[&from].parent else {
            return Ok(None);
        };
        self.ensure_state(parent)?;
        self.versions.get_mut(&parent).expect("parent").redo_to = Some(from);
        self.cursor = parent;
        Ok(Some(self.versions[&from].label.as_str()))
    }

    /// On to the version redo goes to: its label, or None with nothing to
    /// redo.
    pub fn redo(&mut self) -> Result<Option<&str>, Damaged> {
        let Some(to) = self.redo_target() else {
            return Ok(None);
        };
        self.ensure_state(to)?;
        self.cursor = to;
        Ok(Some(self.versions[&to].label.as_str()))
    }

    /// Make `id` the current version, as undo and redo do: not an edit,
    /// so jumping back and forth is free. Redo from any version on the way
    /// to it then leads toward it.
    pub fn jump(&mut self, id: VersionId) -> Result<()> {
        if !self.versions.contains_key(&id) {
            bail!("that version is no longer kept");
        }
        self.ensure_state(id)?;
        let mut child = id;
        while let Some(parent) = self.versions[&child].parent {
            self.versions.get_mut(&parent).expect("parent").redo_to = Some(child);
            child = parent;
        }
        self.cursor = id;
        Ok(())
    }

    pub fn current_version(&self) -> VersionId {
        self.cursor
    }

    /// The edits that made version `id` from its parent, and the
    /// registrations it holds (the files that came in with them): the edge
    /// into it, which is what a merge would replay
    /// (`todos/file-contention.md`).
    pub fn edge(&self, id: VersionId) -> Option<(&[Step], &[SourceId])> {
        let v = self.versions.get(&id)?;
        Some((&v.steps, &v.sources))
    }

    /// Run `edit` as a new version. Refused with [`NotReady`] while the
    /// file opens, as are [`apply_batch`](Self::apply_batch) and
    /// [`apply_in_job`](Self::apply_in_job).
    pub fn apply(&mut self, edit: Edit, label: impl Into<String>) -> Result<EditOutcome> {
        self.check_ready()?;
        let (state, outcome, step) = self.step(self.state().clone(), edit)?;
        if step.prims.is_empty() {
            return Ok(outcome);
        }
        self.push(state, label.into(), vec![step], None);
        Ok(outcome)
    }

    /// Apply several edits in order as one version, so one undo reverts them
    /// all. Each edit sees the result of the ones before it. If any edit
    /// fails, none are applied.
    pub fn apply_batch(
        &mut self,
        edits: Vec<Edit>,
        label: impl Into<String>,
    ) -> Result<EditOutcome> {
        self.check_ready()?;
        let mut state = self.state().clone();
        let mut total = EditOutcome::default();
        let mut steps = Vec::with_capacity(edits.len());
        for (i, edit) in edits.into_iter().enumerate() {
            let (next, out, step) = self
                .step(state, edit)
                .with_context(|| format!("edit {} of batch", i + 1))?;
            state = next;
            steps.push(step);
            total.pages_removed += out.pages_removed;
            total.pages_added += out.pages_added;
            total.bookmarks_removed += out.bookmarks_removed;
            total.bookmarks_added += out.bookmarks_added;
            total.forms_renamed.extend(out.forms_renamed);
            total.signatures_in.extend(out.signatures_in);
            total.unencodable |= out.unencodable;
        }
        coalesce(&mut steps);
        if steps.iter().any(|s| !s.prims.is_empty()) {
            self.push(state, label.into(), steps, None);
        }
        Ok(total)
    }

    /// Start a job whose batches of edits should undo as one step.
    pub fn begin_job(&mut self) -> JobId {
        let id = JobId(self.next_job);
        self.next_job += 1;
        id
    }

    /// Like [`apply`](Self::apply), but when the current version was made
    /// by `job`, has nothing after it, was never saved, and isn't the first
    /// (pruning can make it so), the edit joins it instead of making a new
    /// version. The first batch of a job makes a version; later batches
    /// amend it, so the whole job is one undo step. An amend replaces the
    /// version with a new id under the same parent, and when the job has
    /// put back all it did, it leaves no version at all. A save, an undo or
    /// an unrelated edit in between ends the chain, and the rest of the job
    /// becomes another step.
    pub fn apply_in_job(
        &mut self,
        job: JobId,
        edit: Edit,
        label: impl Into<String>,
    ) -> Result<EditOutcome> {
        self.check_ready()?;
        let (state, outcome, step) = self.step(self.state().clone(), edit)?;
        if step.prims.is_empty() {
            return Ok(outcome);
        }
        let current = &self.versions[&self.cursor];
        let amend = current.job == Some(job)
            && current.parent.is_some()
            && current.children.is_empty()
            && current.saves.is_empty()
            && self.saved != Some(self.cursor);
        if !amend {
            self.push(state, label.into(), vec![step], Some(job));
            return Ok(outcome);
        }
        let old = self.cursor;
        let mut v = self.versions.remove(&old).expect("current version");
        let parent = v.parent.expect("a job's version has a parent");
        v.steps.push(step);
        coalesce(&mut v.steps);
        if v.steps.iter().all(|s| s.prims.is_empty()) {
            // The job put back all it did: back to the parent, whose state
            // this is (trimming may have dropped its snapshot). What the
            // version held waits for the next one, as if the job hadn't
            // run.
            let p = self.versions.get_mut(&parent).expect("parent");
            p.children.retain(|c| *c != old);
            if p.redo_to == Some(old) {
                p.redo_to = None;
            }
            p.state.get_or_insert(state);
            self.cursor = parent;
            v.sources.append(&mut self.unlogged_sources);
            self.unlogged_sources = v.sources;
        } else {
            (v.snapshot_bytes, v.step_bytes) = accounting(&v.steps, &state);
            v.state = Some(state);
            v.label = label.into();
            v.sources.append(&mut self.unlogged_sources);
            self.replace(old, v);
        }
        self.trim();
        Ok(outcome)
    }

    /// Whether the current version is the one `job` is building, i.e. the
    /// user hasn't undone it or edited past it. Meaningful once the job has
    /// applied its first batch.
    pub fn job_is_current(&self, job: JobId) -> bool {
        self.versions[&self.cursor].job == Some(job)
    }

    /// Whether the user has undone `job`'s version: it's past the current
    /// version, reachable by redo. An unrelated edit after the job doesn't
    /// count.
    pub fn job_is_undone(&self, job: JobId) -> bool {
        let mut stack = self.versions[&self.cursor].children.clone();
        while let Some(id) = stack.pop() {
            let v = &self.versions[&id];
            if v.job == Some(job) {
                return true;
            }
            stack.extend(v.children.iter().copied());
        }
        false
    }

    /// Edits wait for the file's facts: there are none to edit before
    /// them, and none come after.
    fn check_ready(&self) -> Result<(), NotReady> {
        if self.ready { Ok(()) } else { Err(NotReady) }
    }

    /// Run `edit` on `state` as a new step, recording what it changed and
    /// describing it against `state`.
    fn step(&self, state: DocState, edit: Edit) -> Result<(DocState, EditOutcome, Step)> {
        let line = crate::describe::describe(&edit, &state);
        let mut rec = Rec::new(state);
        let out = self.compute(&mut rec, edit.clone())?;
        let (state, prims) = rec.finish();
        debug_assert_eq!(state.check_invariants(), Ok(()), "{line}");
        let prims = prims.into();
        Ok((state, out, Step { edit, line, prims }))
    }

    /// Version `id`'s state, rebuilt from the nearest ancestor that kept
    /// one by applying the prims of each version on the way. Damaged when
    /// one of them is already, or when a prim doesn't fit.
    fn replay(&self, id: VersionId) -> Result<DocState, Damaged> {
        let mut path = vec![id];
        let mut at = id;
        let base = loop {
            let parent = self.versions[&at]
                .parent
                .expect("the opened version keeps its state");
            if self.versions[&parent].state.is_some() {
                break parent;
            }
            path.push(parent);
            at = parent;
        };
        let mut state = self.versions[&base].state.clone().expect("base state");
        for &version in path.iter().rev() {
            let v = &self.versions[&version];
            if v.damaged {
                return Err(Damaged { version });
            }
            for p in v.steps.iter().flat_map(|s| s.prims.iter()) {
                if let Err(m) = state.apply(p) {
                    log::error!("version {}: {m}", version.0);
                    return Err(Damaged { version });
                }
            }
        }
        Ok(state)
    }

    /// Make sure version `id` has its state, replaying it when trimming
    /// dropped it. A version that can't be rebuilt is marked damaged; that
    /// is a bug, so a debug build stops there.
    fn ensure_state(&mut self, id: VersionId) -> Result<(), Damaged> {
        if self.versions[&id].state.is_some() {
            return Ok(());
        }
        match self.replay(id) {
            Ok(state) => {
                self.versions.get_mut(&id).expect("version").state = Some(state);
                Ok(())
            }
            Err(e) => {
                self.versions.get_mut(&e.version).expect("version").damaged = true;
                log::error!("{e}");
                if cfg!(debug_assertions) {
                    panic!("{e}");
                }
                Err(e)
            }
        }
    }

    /// A new version after the current one, with the edits that made it.
    /// The versions that were after the current one stay, as a branch.
    fn push(&mut self, state: DocState, label: String, steps: Vec<Step>, job: Option<JobId>) {
        let id = VersionId(self.next_version);
        self.next_version += 1;
        let parent = self.cursor;
        let (snapshot_bytes, step_bytes) = accounting(&steps, &state);
        let p = self.versions.get_mut(&parent).expect("parent");
        p.children.push(id);
        p.redo_to = Some(id);
        self.versions.insert(
            id,
            Version {
                parent: Some(parent),
                children: Vec::new(),
                redo_to: None,
                label,
                author: self.author.clone(),
                at: crate::annot::pdf_date_now(),
                steps,
                sources: std::mem::take(&mut self.unlogged_sources),
                state: Some(state),
                damaged: false,
                snapshot_bytes,
                step_bytes,
                job,
                saves: Vec::new(),
            },
        );
        self.cursor = id;
        self.trim();
    }

    /// Put `v` in version `old`'s place under a new id: a VersionId names
    /// one state, so a version that changes is another version. It takes
    /// `old`'s place among its parent's children and as its children's
    /// parent, and becomes the root, current or saved version where `old`
    /// was. The caller has taken `old` out of the tree, so it no longer
    /// resolves.
    fn replace(&mut self, old: VersionId, v: Version) -> VersionId {
        let id = VersionId(self.next_version);
        self.next_version += 1;
        if let Some(p) = v.parent {
            let parent = self.versions.get_mut(&p).expect("parent");
            for c in parent.children.iter_mut().filter(|c| **c == old) {
                *c = id;
            }
            if parent.redo_to == Some(old) {
                parent.redo_to = Some(id);
            }
        }
        for c in &v.children {
            self.versions.get_mut(c).expect("child").parent = Some(id);
        }
        if self.root == old {
            self.root = id;
        }
        if self.cursor == old {
            self.cursor = id;
        }
        if self.saved == Some(old) {
            self.saved = Some(id);
        }
        self.versions.insert(id, v);
        id
    }

    /// Versions that must keep their snapshots: the opened one (every
    /// replay starts somewhere), the current one, every saved one (a save
    /// re-expressed it, so its steps don't lead to it), and the children
    /// of saved ones (those made before the save can't be replayed from it
    /// either).
    fn keeps_snapshot(&self, id: VersionId) -> bool {
        let v = &self.versions[&id];
        id == self.root
            || id == self.cursor
            || Some(id) == self.saved
            || !v.saves.is_empty()
            || v.parent
                .is_some_and(|p| !self.versions[&p].saves.is_empty())
    }

    /// The versions from the opened one to the current one.
    fn line(&self) -> Vec<VersionId> {
        let mut line = vec![self.cursor];
        while let Some(p) = self.versions[line.last().expect("nonempty")].parent {
            line.push(p);
        }
        line.reverse();
        line
    }

    /// Snapshots the policy may drop, in the order to drop them: versions
    /// off the current `line` first (other branches, undone ones), then the
    /// line's, oldest first.
    fn droppable(&self, line: &HashSet<VersionId>) -> Vec<VersionId> {
        let mut ids: Vec<VersionId> = self
            .versions
            .iter()
            .filter(|(id, v)| v.state.is_some() && !self.keeps_snapshot(**id))
            .map(|(id, _)| *id)
            .collect();
        ids.sort_by_key(|id| (line.contains(id), *id));
        ids
    }

    fn trim(&mut self) {
        let line = self.line().into_iter().collect();
        let droppable = self.droppable(&line);
        let stats = self.stats_for(&line, &droppable);
        let n = self.policy.trim(&stats).min(droppable.len());
        for id in &droppable[..n] {
            self.versions.get_mut(id).expect("version").state = None;
        }
        self.prune();
    }

    /// Keep the tree to [`crate::history::MAX_VERSIONS`]: drop versions on
    /// other branches, oldest first, then the oldest versions before the
    /// saved one. Edits back to the last save always stay (a later merge
    /// replays them). A version dropped from a branch takes its
    /// registrations with it; the first version's go to the next, whose
    /// state was made over them.
    fn prune(&mut self) {
        let max = crate::history::MAX_VERSIONS;
        while self.versions.len() > max {
            let line: HashSet<VersionId> = self.line().into_iter().collect();
            let leaf = self
                .versions
                .iter()
                .filter(|(id, v)| {
                    v.children.is_empty() && !line.contains(id) && Some(**id) != self.saved
                })
                .map(|(id, _)| *id)
                .min();
            let Some(leaf) = leaf else { break };
            self.remove_version(leaf);
        }
        // Then move the root forward along the line, up to the saved
        // version (or the current one when the saved one is elsewhere).
        let line = self.line();
        let stop = self
            .saved
            .and_then(|s| line.iter().position(|v| *v == s))
            .unwrap_or(line.len() - 1);
        let mut i = 0;
        while self.versions.len() > max && i < stop {
            let next = line[i + 1];
            if self.ensure_state(next).is_err() {
                break;
            }
            let old = line[i];
            for sibling in self.versions[&old].children.clone() {
                if sibling != next {
                    self.remove_subtree(sibling);
                }
            }
            let mut sources = self.versions.remove(&old).expect("old root").sources;
            let new_root = self.versions.get_mut(&next).expect("next");
            new_root.parent = None;
            sources.append(&mut new_root.sources);
            new_root.sources = sources;
            self.root = next;
            i += 1;
        }
    }

    /// Take version `id` out of the tree, and the registrations it holds
    /// out of the document.
    fn remove_version(&mut self, id: VersionId) {
        let Some(v) = self.versions.remove(&id) else {
            return;
        };
        if let Some(p) = v.parent
            && let Some(parent) = self.versions.get_mut(&p)
        {
            parent.children.retain(|c| *c != id);
            if parent.redo_to == Some(id) {
                parent.redo_to = None;
            }
        }
        if self.saved == Some(id) {
            self.saved = None;
        }
        self.unregister(&v.sources);
    }

    fn remove_subtree(&mut self, id: VersionId) {
        let mut stack = vec![id];
        while let Some(v) = stack.pop() {
            if let Some(version) = self.versions.get(&v) {
                stack.extend(version.children.iter().copied());
            }
            self.remove_version(v);
        }
    }

    fn stats_for(&self, line: &HashSet<VersionId>, droppable: &[VersionId]) -> HistoryStats {
        let mut kept: Vec<VersionId> = self
            .versions
            .iter()
            .filter(|(id, v)| v.state.is_some() && !droppable.contains(id))
            .map(|(id, _)| *id)
            .collect();
        // The current version's last.
        kept.sort_by_key(|id| (*id == self.cursor, *id));
        let snapshot_bytes: Vec<usize> = droppable
            .iter()
            .chain(&kept)
            .map(|id| self.versions[id].snapshot_bytes)
            .collect();
        HistoryStats {
            versions: snapshot_bytes.len(),
            cursor: snapshot_bytes.len() - 1,
            snapshot_bytes,
            step_bytes: self.versions.values().map(|v| v.step_bytes).sum(),
            blob_bytes: self.store.bytes(),
            undone_source_bytes: self.undone_source_bytes(line),
        }
    }

    /// The bytes of the blobs only versions off the current `line` hold
    /// registrations of (inserts that were undone), each blob once. A blob
    /// a registration on the line, one waiting for the next version, or the
    /// primary shares isn't counted: pruning wouldn't free it.
    fn undone_source_bytes(&self, line: &HashSet<VersionId>) -> usize {
        let held = self.versions.iter().flat_map(|(id, v)| {
            let on_line = line.contains(id);
            v.sources.iter().map(move |s| (s, on_line))
        });
        let waiting = self.unlogged_sources.iter().chain([&self.primary]);
        let (mut kept, mut undone) = (HashSet::new(), HashMap::new());
        for (sid, on_line) in held.chain(waiting.map(|s| (s, true))) {
            let Some(blob) = self.sources.get(sid).map(|s| s.src.blob()) else {
                continue;
            };
            if on_line {
                kept.insert(blob.id());
            } else {
                undone.insert(blob.id(), blob.len());
            }
        }
        undone
            .into_iter()
            .filter(|(id, _)| !kept.contains(id))
            .map(|(_, len)| len)
            .sum()
    }

    /// Snapshots kept and their estimated bytes (droppable ones first), the
    /// bytes every version's steps hold, and the sources' bytes.
    pub fn stats(&self) -> HistoryStats {
        let line = self.line().into_iter().collect();
        self.stats_for(&line, &self.droppable(&line))
    }

    /// How many versions the history holds, with or without snapshots.
    pub fn version_count(&self) -> usize {
        self.versions.len()
    }

    fn view(&self, id: VersionId, shown_child: Option<VersionId>, undone: bool) -> VersionView {
        let v = &self.versions[&id];
        VersionView {
            id,
            label: v.label.clone(),
            author: v.author.clone(),
            at: v.at.clone(),
            edits: v.steps.iter().map(|s| s.line.clone()).collect(),
            saves: v.saves.clone(),
            current: id == self.cursor,
            saved: Some(id) == self.saved,
            undone,
            branches: v
                .children
                .iter()
                .copied()
                .filter(|c| Some(*c) != shown_child)
                .collect(),
            replays: v.state.is_none(),
            damaged: v.damaged,
        }
    }

    /// The history as the panel lists it: from the oldest version kept to
    /// the current one, then on along redo.
    pub fn timeline(&self) -> Vec<VersionView> {
        let line = self.line();
        let mut out: Vec<VersionView> = line
            .iter()
            .enumerate()
            .map(|(i, id)| {
                let next = line.get(i + 1).copied().or_else(|| self.redo_target());
                self.view(*id, next, false)
            })
            .collect();
        let mut at = self.redo_target();
        while let Some(id) = at {
            let v = &self.versions[&id];
            let next = v
                .redo_to
                .filter(|c| v.children.contains(c))
                .or_else(|| v.children.last().copied());
            out.push(self.view(id, next, true));
            at = next;
        }
        out
    }

    /// The versions of a branch, from its first along its newest edits.
    pub fn branch(&self, start: VersionId) -> Vec<VersionView> {
        let mut out = Vec::new();
        let mut at = Some(start);
        while let Some(id) = at.filter(|id| self.versions.contains_key(id)) {
            let v = &self.versions[&id];
            let next = v
                .redo_to
                .filter(|c| v.children.contains(c))
                .or_else(|| v.children.last().copied());
            out.push(self.view(id, next, id != self.cursor));
            at = next;
        }
        out
    }

    /// The form of registration `source` that comes in with its pages
    /// (`shown`: each page's index in the source, and the page showing
    /// it), into `rec` as an instance of its own (M10 proposal 5): its
    /// fields with widgets on those pages, its fields with no widget at
    /// all, and the nodes above them, each with a new id, and their
    /// widgets on those pages. When any of their full names, or a parent
    /// of one, is a node's in the document, the instance goes under a new
    /// parent named for the file: its name, given back. Pages of a file
    /// whose form is here already (a run of its pages after another, as
    /// Combine inserts them) join that instance. The widgets, by page
    /// index, each with its place in its page's `/Annots`, which
    /// [`bring_annots`](Self::bring_annots) puts them at.
    fn take_in_fields(
        &self,
        rec: &mut Rec,
        source: SourceId,
        shown: &[(u32, PageId)],
    ) -> (Option<String>, Placed) {
        let Some(info) = self.sources.get(&source) else {
            return (None, Placed::new());
        };
        let form = info.src.form().clone();
        let on_pages: HashSet<FieldId> = (shown.iter())
            .flat_map(|(index, _)| form.widgets.get(index).into_iter().flatten())
            .map(|w| w.field)
            .collect();
        if on_pages.is_empty() {
            return (None, Placed::new());
        }
        let with_widgets: HashSet<FieldId> =
            form.widgets.values().flatten().map(|w| w.field).collect();
        // The nodes that come in, by the file's ids, in its tree's order:
        // those fields, and every node above one.
        let mut coming = BTreeSet::new();
        for f in &form.fields {
            if on_pages.contains(&f.id) || !with_widgets.contains(&f.id) {
                let mut at = Some(f.id);
                while let Some(id) = at.filter(|&id| coming.insert(id)) {
                    at = form.nodes[id.0 as usize].parent;
                }
            }
        }
        // The instance a run of its pages before brought, if one did, and
        // its nodes by object.
        let joined = FormInst {
            src: source,
            copy: 0,
        };
        let mut doc = FormDoc::clone(rec.form());
        let join = doc.names.contains_key(&joined);
        let here: HashMap<ObjectId, FieldId> = match join {
            true => (rec.fields())
                .filter(|(_, node)| node.inst == joined)
                .filter_map(|(id, node)| Some((node.src.as_ref()?.id, id)))
                .collect(),
            false => HashMap::new(),
        };
        let ids: HashMap<FieldId, FieldId> = (coming.iter())
            .map(|&n| {
                let here = here.get(&form.nodes[n.0 as usize].object);
                (n, here.copied().unwrap_or_else(|| self.ids.field()))
            })
            .collect();
        // The node its roots go under, if any: the one its first run went
        // under, or one named apart now.
        let (inst, under, top) = if join {
            let top = (doc.roots.iter().copied()).find(|&r| {
                rec.field(r)
                    .is_some_and(|n| n.inst == joined && n.src.is_none())
            });
            (joined, None, top)
        } else {
            let inst = FormInst {
                src: source,
                copy: next_copy(&doc, source),
            };
            let name = file_name(info);
            let names = (form.fields.iter())
                .filter(|f| coming.contains(&f.id))
                .map(|f| f.name.as_str());
            let under = name_apart(rec, &name, names);
            doc.names.insert(inst, name);
            let top = under.as_deref().map(|under| {
                let id = self.ids.field();
                put_field(rec, id, sheaf_parent(under, inst));
                doc.roots.push_back(id);
                id
            });
            (inst, under, top)
        };
        // The widgets on the pages coming in, each with a new id.
        let mut shows: HashMap<FieldId, Vector<AnnotId>> = HashMap::new();
        let mut placed = Placed::new();
        let mut taken = HashSet::new();
        for &(index, page) in shown {
            for w in form.widgets.get(&index).into_iter().flatten() {
                let Some(&field) = ids.get(&w.field) else {
                    continue;
                };
                let id = fresh_annot_id(rec, &mut taken);
                let src = Some(ObjRef {
                    src: info.src.clone(),
                    id: w.object,
                });
                let body = AnnotBody::Widget(Arc::new(Widget::read(w, field, src, inst)));
                rec.put(Key::Annot(id), Some(Val::Annot(AnnotRec { page, body })));
                shows.entry(field).or_default().push_back(id);
                placed.entry(index).or_default().push((w.slot, id));
            }
        }
        let terms: HashMap<FieldId, &crate::forms::Field> =
            form.fields.iter().map(|f| (f.id, f)).collect();
        // New kids of nodes that were here, and of the new parent.
        let mut more: BTreeMap<FieldId, Vector<FieldId>> = BTreeMap::new();
        for &n in &coming {
            let node = &form.nodes[n.0 as usize];
            let id = ids[&n];
            if here.contains_key(&node.object) {
                // It was here: its widgets on these pages join its own.
                if let Some(widgets) = shows.remove(&id) {
                    let was = rec.field(id).expect("a node here").clone();
                    let mut term =
                        Terminal::clone(was.term.as_ref().expect("a field with widgets"));
                    term.widgets.append(widgets);
                    let node = FieldNode {
                        term: Some(Arc::new(term)),
                        ..FieldNode::clone(&was)
                    };
                    put_field(rec, id, node);
                }
                continue;
            }
            let parent = node.parent.map(|p| ids[&p]).or(top);
            let listed = node
                .parent
                .is_some_and(|p| !here.contains_key(&form.nodes[p.0 as usize].object));
            match parent {
                _ if listed => {}
                Some(p) => more.entry(p).or_default().push_back(id),
                None => doc.roots.push_back(id),
            }
            let term = (terms.get(&n))
                .map(|f| Arc::new(Terminal::read(f, shows.remove(&id).unwrap_or_default())));
            let node = FieldNode {
                parent,
                partial: node.partial.clone(),
                src: Some(ObjRef {
                    src: info.src.clone(),
                    id: node.object,
                }),
                inst,
                kids: node
                    .kids
                    .iter()
                    .filter_map(|k| ids.get(k))
                    .copied()
                    .collect(),
                term,
            };
            put_field(rec, id, node);
        }
        for (p, kids) in more {
            let mut node = FieldNode::clone(rec.field(p).expect("a node they came under"));
            node.kids.append(kids);
            put_field(rec, p, node);
        }
        rec.put(Key::Form, Some(Val::Form(Arc::new(doc))));
        (under, placed)
    }

    /// Duplicate Pages: each copy's widgets get fields of their own (M10
    /// bug 4), so filling one never fills the other. For each instance of
    /// a form the pages show, a new one, named as it is in
    /// [`FormDoc::names`], of its fields with widgets on them and the
    /// nodes above those that were read from it (not a parent sheaf made),
    /// renamed apart by [`take_in_fields`](Self::take_in_fields)' rule;
    /// its widgets on the copies have new ids. `copies` gives each page
    /// and its copy. The parents the copies' fields went under, with their
    /// files, and each widget's copy, by the id of the one it's a copy of.
    fn copy_fields(
        &self,
        rec: &mut Rec,
        copies: &[(PageId, PageId, u32)],
    ) -> (Vec<(String, SourceId)>, HashMap<AnnotId, AnnotId>) {
        let mut shown: BTreeMap<FormInst, BTreeSet<FieldId>> = BTreeMap::new();
        for &(page, ..) in copies {
            for (_, w) in rec.widgets_on(page) {
                shown.entry(w.inst).or_default().insert(w.field);
            }
        }
        let mut doc = FormDoc::clone(rec.form());
        let mut renamed = Vec::new();
        let mut copied = HashMap::new();
        let mut taken = HashSet::new();
        for (orig, fields) in shown {
            let inst = FormInst {
                src: orig.src,
                copy: next_copy(&doc, orig.src),
            };
            let name = doc.names.get(&orig).cloned().unwrap_or_default();
            doc.names.insert(inst, name.clone());
            let mut coming = BTreeSet::new();
            for &f in &fields {
                coming.insert(f);
                let mut at = rec.field(f).and_then(|n| n.parent);
                while let Some(id) = at {
                    let Some(node) = rec.field(id).filter(|n| n.src.is_some()) else {
                        break;
                    };
                    if !coming.insert(id) {
                        break;
                    }
                    at = node.parent;
                }
            }
            let names: Vec<String> = fields.iter().map(|&f| inst_name(rec, f)).collect();
            let under = name_apart(rec, &name, names.iter().map(String::as_str));
            let top = under.as_deref().map(|under| {
                let id = self.ids.field();
                put_field(rec, id, sheaf_parent(under, inst));
                doc.roots.push_back(id);
                id
            });
            renamed.extend(under.map(|under| (under, orig.src)));
            let ids: HashMap<FieldId, FieldId> =
                coming.iter().map(|&id| (id, self.ids.field())).collect();
            // Its widgets on the copies.
            let mut shows: HashMap<FieldId, Vector<AnnotId>> = HashMap::new();
            for &(page, copy, _) in copies {
                let theirs: Vec<(AnnotId, Arc<Widget>)> = (rec.widgets_on(page))
                    .filter(|(_, w)| w.inst == orig)
                    .map(|(id, w)| (id, w.clone()))
                    .collect();
                for (was, w) in theirs {
                    let id = fresh_annot_id(rec, &mut taken);
                    let field = ids[&w.field];
                    let w = Widget {
                        field,
                        inst,
                        ..Widget::clone(&w)
                    };
                    let widget = AnnotRec {
                        page: copy,
                        body: AnnotBody::Widget(Arc::new(w)),
                    };
                    rec.put(Key::Annot(id), Some(Val::Annot(widget)));
                    shows.entry(field).or_default().push_back(id);
                    copied.insert(was, id);
                }
            }
            let mut roots = Vector::new();
            for &old in &coming {
                let was = FieldNode::clone(rec.field(old).expect("a node shown"));
                let id = ids[&old];
                let parent = was.parent.and_then(|p| ids.get(&p).copied());
                if parent.is_none() {
                    roots.push_back(id);
                }
                let term = was.term.as_ref().map(|t| {
                    let widgets = shows.remove(&id).unwrap_or_default();
                    Arc::new(Terminal {
                        widgets,
                        ..Terminal::clone(t)
                    })
                });
                let node = FieldNode {
                    parent: parent.or(top),
                    kids: was
                        .kids
                        .iter()
                        .filter_map(|k| ids.get(k))
                        .copied()
                        .collect(),
                    inst,
                    term,
                    ..was
                };
                put_field(rec, id, node);
            }
            match top {
                Some(top) => {
                    let mut node = FieldNode::clone(rec.field(top).expect("the new parent"));
                    node.kids = roots;
                    put_field(rec, top, node);
                }
                None => doc.roots.append(roots),
            }
        }
        rec.put(Key::Form, Some(Val::Form(Arc::new(doc))));
        (renamed, copied)
    }

    /// Bookmarks for [`Edit::OutlineInsertTree`]: pre-order items with
    /// depths that start at 0 and go down at most one level at a time.
    fn tree_from_items(&self, items: &[NewOutlineItem]) -> Result<Nested> {
        let mut prev: Option<u16> = None;
        for item in items {
            match prev {
                None if item.depth != 0 => bail!("the first bookmark must be at depth 0"),
                Some(p) if item.depth > p + 1 => {
                    bail!("bookmark depth jumps from {p} to {}", item.depth)
                }
                _ => {}
            }
            prev = Some(item.depth);
        }
        Ok(self.nest(items.iter().map(|item| {
            let b = Bookmark {
                title: item.title.as_str().into(),
                target: item.target.map(|page| Target::Page {
                    page,
                    view: item.view.clone(),
                }),
                color: None,
                style: TextStyle::default(),
                open: true,
                kids: Vector::new(),
                src: None,
            };
            (item.depth, b)
        })))
    }

    /// Bookmarks of a file's items, as [`crate::open::outline`] read them
    /// from `src`: `page` gives the page that shows each page of the file,
    /// and an item going to one none shows goes nowhere.
    fn outline_from_flat(
        &self,
        items: &[FlatOutlineItem],
        page: impl Fn(u32) -> Option<PageId>,
        src: &Src,
    ) -> Nested {
        self.nest(items.iter().map(|item| {
            let target = if item.kept {
                Some(Target::Kept)
            } else {
                item.page.and_then(&page).map(|page| Target::Page {
                    page,
                    view: item.view.clone(),
                })
            };
            let b = Bookmark {
                title: item.title.as_str().into(),
                target,
                color: item.color,
                style: item.style,
                open: item.open,
                kids: Vector::new(),
                src: item.object.map(|id| ObjRef {
                    src: src.clone(),
                    id,
                }),
            };
            (item.depth, b)
        }))
    }

    /// `items`, bookmarks in pre-order with their depths, under one
    /// another, each with a new id: one deeper than the one before is its
    /// kid, and one as deep or less a sibling of the last one less deep.
    fn nest(&self, items: impl IntoIterator<Item = (u16, Bookmark)>) -> Nested {
        let mut roots = Vector::new();
        let mut out: Vec<(OutlineId, Bookmark)> = Vec::new();
        // The path down to the last one: each one's depth and place in
        // `out`.
        let mut path: Vec<(u16, usize)> = Vec::new();
        for (depth, b) in items {
            while path.last().is_some_and(|&(d, _)| d >= depth) {
                path.pop();
            }
            let id = self.ids.outline();
            match path.last() {
                Some(&(_, parent)) => out[parent].1.kids.push_back(id),
                None => roots.push_back(id),
            }
            path.push((depth, out.len()));
            out.push((id, b));
        }
        (roots, out)
    }

    // ----- edits ------------------------------------------------------------

    /// What pages that came in from `src` hold beside their content
    /// (`shown`: each page's index in `src`, and the page showing it): the
    /// other tools' comments and links its fact has on each, with the
    /// form's widgets [`take_in_fields`](Self::take_in_fields) `placed`
    /// among them as the file has them, and then sheaf's marks, `marks`, by
    /// index, as the edit carries them. Each gets a fresh id (the file's
    /// could clash with ours), and goes after what its page has. A mark is
    /// the object the fact has under its id, holding
    /// what that holds while it names the same, else what
    /// [`mark`](Self::mark) finds; a connector keeps its partner only if
    /// the partner came too; and a reply to a mark, ours or another
    /// tool's, follows it to its new name. Whether any mark draws text the
    /// built-in fonts can't ([`Annot::unencodable`]).
    fn bring_annots(
        &self,
        rec: &mut Rec,
        src: &Src,
        shown: &[(u32, PageId)],
        marks: Vec<(u32, Vec<Annot>)>,
        placed: &Placed,
    ) -> Result<bool> {
        let fact = src.annots().clone();
        let mut marks: HashMap<u32, Vec<Annot>> = marks.into_iter().collect();
        let incoming: Vec<(u32, PageId, Vec<Annot>)> = (shown.iter())
            .map(|&(index, page)| (index, page, marks.remove(&index).unwrap_or_default()))
            .collect();
        let unencodable = (incoming.iter())
            .flat_map(|(_, _, list)| list)
            .any(Annot::unencodable);
        let mut taken = HashSet::new();
        let mut ids = HashMap::new();
        for a in incoming.iter().flat_map(|(_, _, list)| list) {
            ids.insert(a.id, fresh_annot_id(rec, &mut taken));
        }
        let at = |id| ObjRef {
            src: src.clone(),
            id,
        };
        for (index, page, list) in incoming {
            let copy = rec.def(page).map_or(0, |def| def.copy);
            // The names of the marks sheaf made, which change with their ids.
            let names: HashMap<String, String> = (list.iter())
                .filter(|a| a.name.is_empty())
                .map(|a| {
                    let ([was, _], [now, _]) = (a.id.sheaf_names(), ids[&a.id].sheaf_names());
                    (was, now)
                })
                .collect();
            let mut on_page = rec.annot_ids_on(page).cloned().unwrap_or_default();
            let mut theirs = placed.get(&index).cloned().unwrap_or_default();
            for (_, object, summary) in fact.foreign_on(index) {
                let id = fresh_annot_id(rec, &mut taken);
                let parent = (summary.parent.as_ref()).and_then(|p| names.get(p));
                let summary = match parent {
                    Some(now) => Arc::new(ForeignAnnot {
                        parent: Some(now.clone()),
                        ..ForeignAnnot::clone(summary)
                    }),
                    None => summary.clone(),
                };
                theirs.push((summary.slot, id));
                let src = object.map(at);
                let body = AnnotBody::Foreign(Arc::new(Foreign { src, copy, summary }));
                rec.put(Key::Annot(id), Some(Val::Annot(AnnotRec { page, body })));
            }
            theirs.sort_by_key(|&(slot, _)| slot);
            on_page.extend(theirs.into_iter().map(|(_, id)| id));
            for mut a in list {
                let read = (fact.on(index).iter()).find_map(|f| match &f.body {
                    FileBody::Mark { annot, held } if annot.id == a.id => {
                        Some((f.object, holds_same(&annot.kind, &a.kind).then_some(*held)))
                    }
                    _ => None,
                });
                let (object, held) = read.unwrap_or_default();
                let object = object.filter(|_| copy == 0).map(at);
                a.id = ids[&a.id];
                match &mut a.kind {
                    AnnotKind::Connector { partner, .. } => {
                        *partner = partner.and_then(|p| ids.get(&p).copied());
                    }
                    AnnotKind::Reply { parent } | AnnotKind::Status { parent, .. } => {
                        if let Some(now) = names.get(parent) {
                            *parent = now.clone();
                        }
                    }
                    _ => {}
                }
                let id = a.id;
                let mark = match held {
                    Some(held) => {
                        let mark = Mark {
                            src: object,
                            ..Mark::from(a)
                        };
                        holding(mark, &self.store, src, held)
                    }
                    None => self.mark(a, object)?,
                };
                let body = AnnotBody::Mark(Arc::new(mark));
                rec.put(Key::Annot(id), Some(Val::Annot(AnnotRec { page, body })));
                on_page.push_back(id);
            }
            put_page_annots(rec, page, on_page);
        }
        Ok(unencodable)
    }

    fn compute(&self, rec: &mut Rec, edit: Edit) -> Result<EditOutcome> {
        let mut out = EditOutcome::default();
        match edit {
            Edit::MovePages { ids, before } => {
                let set: HashSet<PageId> = ids.iter().copied().collect();
                if let Some(b) = before
                    && set.contains(&b)
                {
                    bail!("cannot move pages before one of themselves");
                }
                let (moving, rest): (Vector<PageId>, Vector<PageId>) =
                    rec.order().iter().copied().partition(|id| set.contains(id));
                let at = match before {
                    Some(b) => rest
                        .iter()
                        .position(|&id| id == b)
                        .context("destination page not found")?,
                    None => rest.len(),
                };
                rec.put(Key::Order, Some(Val::Order(splice(&rest, at, moving))));
            }
            Edit::RotatePages { ids, by } => {
                let set: HashSet<PageId> = ids.into_iter().collect();
                for id in set {
                    if let Some(geom) = rec.geom(id) {
                        let geom = Geom {
                            rotate: geom.rotate.plus(by),
                            ..**geom
                        };
                        rec.put(Key::Geom(id), Some(Val::Geom(Arc::new(geom))));
                    }
                }
            }
            Edit::DeletePages { ids } => {
                let set: HashSet<PageId> = ids.into_iter().collect();
                let order: Vector<PageId> = rec
                    .order()
                    .iter()
                    .filter(|id| !set.contains(id))
                    .copied()
                    .collect();
                out.pages_removed = rec.page_count() - order.len();
                rec.put(Key::Order, Some(Val::Order(order)));
                for &id in &set {
                    rec.put(Key::Page(id), None);
                    rec.put(Key::Geom(id), None);
                    drop_annots(rec, id, |_, _| true);
                }
                out.bookmarks_removed = cascade_delete(rec, &set);
            }
            Edit::InsertPages {
                source,
                range,
                before,
                outline,
                annots,
            } => {
                let info = self.sources.get(&source).context("unknown source")?.clone();
                if range.end > info.src.page_count() {
                    bail!("range exceeds source page count");
                }
                let at = match before {
                    Some(b) => rec.pos_of(b).context("destination page not found")?,
                    None => rec.page_count(),
                };
                let mut copies = NextCopy::of(rec);
                // Source index -> new PageId, for bookmark targets.
                let mut by_index = vec![PageId(u64::MAX); info.src.page_count() as usize];
                for index in range.clone() {
                    let id = self.ids.page();
                    let (def, geom) = copies.page(source, &info.src, index);
                    rec.put(Key::Page(id), Some(Val::Page(def)));
                    rec.put(Key::Geom(id), Some(Val::Geom(geom)));
                    by_index[index as usize] = id;
                }
                out.pages_added = range.len();
                let new = range.clone().map(|index| by_index[index as usize]);
                let order = splice(rec.order(), at, new);
                rec.put(Key::Order, Some(Val::Order(order)));
                if let Some(items) = outline {
                    // Pages outside the inserted range are the sentinel's:
                    // bookmarks to them go nowhere.
                    let page = |index: u32| {
                        let id = by_index.get(index as usize).copied();
                        id.filter(|&id| id != PageId(u64::MAX))
                    };
                    let incoming = self.outline_from_flat(&items, page, &info.src);
                    out.bookmarks_added = merge_outline(rec, incoming);
                }
                let shown: Vec<(u32, PageId)> = range.map(|i| (i, by_index[i as usize])).collect();
                let (under, placed) = self.take_in_fields(rec, source, &shown);
                out.forms_renamed.extend(under.map(|under| (under, source)));
                out.unencodable = self.bring_annots(rec, &info.src, &shown, annots, &placed)?;
                if signed_from(rec, source) {
                    out.signatures_in.push(source);
                }
                self.scripts_apart(rec);
            }
            Edit::DuplicatePages { ids } => {
                let set: HashSet<PageId> = ids.into_iter().collect();
                let mut copies = NextCopy::of(rec);
                let pages: Vec<(PageId, Arc<PageDef>, Arc<Geom>)> = rec
                    .pages()
                    .map(|(id, def, geom)| (id, def.clone(), geom.clone()))
                    .collect();
                let mut order = Vector::new();
                // Each page, its copy and the copy's number.
                let mut pairs = Vec::new();
                for (id, def, geom) in pages {
                    order.push_back(id);
                    if !set.contains(&id) {
                        continue;
                    }
                    let copy = self.ids.page();
                    // The next copy of the same page, with the same text
                    // over it, shared (M10.5 §6.5), and the same boxes.
                    let def = PageDef {
                        copy: copies.take(&def.base),
                        ..PageDef::clone(&def)
                    };
                    let n = def.copy;
                    rec.put(Key::Page(copy), Some(Val::Page(Arc::new(def))));
                    rec.put(Key::Geom(copy), Some(Val::Geom(geom)));
                    pairs.push((id, copy, n));
                    order.push_back(copy);
                    out.pages_added += 1;
                }
                rec.put(Key::Order, Some(Val::Order(order)));
                let (renamed, widgets) = self.copy_fields(rec, &pairs);
                out.forms_renamed = renamed;
                for &(page, copy, n) in &pairs {
                    copy_annots(rec, page, copy, n, &widgets);
                }
                self.scripts_apart(rec);
            }
            Edit::ReplacePages {
                ids,
                source,
                range,
                annots,
            } => {
                let info = self.sources.get(&source).context("unknown source")?.clone();
                if range.end > info.src.page_count() {
                    bail!("range exceeds source page count");
                }
                let mut copies = NextCopy::of(rec);
                let mut replacements = range.clone();
                let mut last_pos = None;
                let mut extra_old = HashSet::new();
                // Which page now shows each replacement, for its marks.
                let mut shown_by: HashMap<u32, PageId> = HashMap::new();
                // A page listed again is replaced once.
                let mut listed = HashSet::new();
                for &id in ids.iter().filter(|&&id| listed.insert(id)) {
                    let i = rec.pos_of(id).context("page to replace not found")?;
                    // The page shows different content now; its widgets
                    // and other tools' annotations aren't on it. Its marks
                    // stay, as do the widgets' fields.
                    drop_annots(rec, id, |_, a| a.mark().is_none());
                    match replacements.next() {
                        Some(index) => {
                            // Its OCR was of the page it showed.
                            let (def, geom) = copies.page(source, &info.src, index);
                            rec.put(Key::Page(id), Some(Val::Page(def)));
                            rec.put(Key::Geom(id), Some(Val::Geom(geom)));
                            shown_by.insert(index, id);
                            last_pos = Some(i);
                        }
                        None => {
                            extra_old.insert(id);
                        }
                    }
                }
                // Extra replacements go after the last replaced page.
                let at = last_pos.map_or(rec.page_count(), |i| i + 1);
                let mut extra = Vec::new();
                for index in replacements {
                    let id = self.ids.page();
                    let (def, geom) = copies.page(source, &info.src, index);
                    rec.put(Key::Page(id), Some(Val::Page(def)));
                    rec.put(Key::Geom(id), Some(Val::Geom(geom)));
                    shown_by.insert(index, id);
                    extra.push(id);
                }
                out.pages_added += extra.len();
                if !extra.is_empty() || !extra_old.is_empty() {
                    let order: Vector<PageId> = splice(rec.order(), at, extra)
                        .into_iter()
                        .filter(|id| !extra_old.contains(id))
                        .collect();
                    rec.put(Key::Order, Some(Val::Order(order)));
                }
                if !extra_old.is_empty() {
                    out.pages_removed = extra_old.len();
                    for &id in &extra_old {
                        rec.put(Key::Page(id), None);
                        rec.put(Key::Geom(id), None);
                        drop_annots(rec, id, |_, _| true);
                    }
                    out.bookmarks_removed = cascade_delete(rec, &extra_old);
                }
                let mut shown: Vec<(u32, PageId)> = shown_by.into_iter().collect();
                shown.sort();
                let (under, placed) = self.take_in_fields(rec, source, &shown);
                out.forms_renamed.extend(under.map(|under| (under, source)));
                out.unencodable = self.bring_annots(rec, &info.src, &shown, annots, &placed)?;
                if signed_from(rec, source) {
                    out.signatures_in.push(source);
                }
                self.scripts_apart(rec);
            }
            Edit::OutlineInsert {
                parent,
                after,
                title,
                target,
                view,
            } => {
                // A bookmark goes to a page there is.
                if target.is_some_and(|page| rec.def(page).is_none()) {
                    bail!("page not found");
                }
                let id = self.ids.outline();
                let b = Bookmark {
                    title: title.into(),
                    target: target.map(|page| Target::Page { page, view }),
                    color: None,
                    style: TextStyle::default(),
                    open: true,
                    kids: Vector::new(),
                    src: None,
                };
                rec.put(Key::Bookmark(id), Some(Val::Bookmark(Arc::new(b))));
                outline_insert(rec, parent, after, id)?;
                out.bookmarks_added = 1;
            }
            Edit::OutlineInsertTree {
                parent,
                after,
                items,
            } => {
                let mut targets = items.iter().filter_map(|item| item.target);
                if targets.any(|page| rec.def(page).is_none()) {
                    bail!("page not found");
                }
                let (roots, made) = self.tree_from_items(&items)?;
                out.bookmarks_added = items.len();
                for (id, b) in made {
                    rec.put(Key::Bookmark(id), Some(Val::Bookmark(Arc::new(b))));
                }
                let mut after = after;
                for id in roots {
                    outline_insert(rec, parent, after, id)?;
                    after = Some(id);
                }
            }
            Edit::OutlineMove {
                id,
                parent,
                after,
                with_pages,
            } => {
                if parent == Some(id) || rec.bookmark(id).is_none() {
                    bail!("bookmark not found or moved into itself");
                }
                if let Some(p) = parent
                    && subtree(rec, id).contains(&p)
                {
                    bail!("cannot move a bookmark under its own descendant");
                }
                let span = with_pages.then(|| bookmark_span(rec, id)).flatten();
                outline_unlink(rec, id).context("bookmark not found")?;
                outline_insert(rec, parent, after, id)?;
                if let Some(span) = span {
                    // Destination: right after the span of whatever precedes the
                    // moved node in the new pre-order (or the start).
                    let flat = rec.bookmarks_preorder();
                    let my_pos = flat.iter().position(|&(_, at, _)| at == id).unwrap();
                    let moving: HashSet<PageId> = span.iter().copied().collect();
                    let rest: Vector<PageId> = rec
                        .order()
                        .iter()
                        .filter(|id| !moving.contains(id))
                        .copied()
                        .collect();
                    let prev = flat[..my_pos]
                        .iter()
                        .rev()
                        .find(|(_, _, b)| b.page().is_some());
                    let dest = match prev {
                        Some(&(_, prev, b)) => {
                            // End of the previous bookmark's span, in `rest`.
                            let at: HashMap<PageId, usize> =
                                rest.iter().enumerate().map(|(i, &id)| (id, i)).collect();
                            let pos = |id| at.get(&id).copied();
                            match span_at(&flat, prev, rest.len(), pos) {
                                Some(owned) => owned.end,
                                None => b.page().and_then(pos).map_or(0, |i| i + 1),
                            }
                        }
                        None => 0,
                    };
                    rec.put(Key::Order, Some(Val::Order(splice(&rest, dest, span))));
                }
            }
            Edit::OutlineEdit {
                id,
                title,
                target,
                color,
                view,
                style,
            } => {
                let mut b = Bookmark::clone(rec.bookmark(id).context("bookmark not found")?);
                if let Some(Some(page)) = target
                    && rec.def(page).is_none()
                {
                    bail!("page not found");
                }
                if let Some(t) = title {
                    b.title = t.into();
                }
                if let Some(t) = target {
                    // A new page: the old position meant the old page.
                    b.target = t.map(|page| Target::Page { page, view: None });
                }
                if let Some(v) = view
                    && let Some(Target::Page { view, .. }) = &mut b.target
                {
                    *view = v;
                }
                if let Some(c) = color {
                    b.color = c;
                }
                if let Some(s) = style {
                    b.style = s;
                }
                rec.put(Key::Bookmark(id), Some(Val::Bookmark(Arc::new(b))));
            }
            Edit::OutlineDelete { id } => {
                outline_unlink(rec, id).context("bookmark not found")?;
                let gone = subtree(rec, id);
                for &id in &gone {
                    rec.put(Key::Bookmark(id), None);
                }
                out.bookmarks_removed = gone.len();
            }
            Edit::AnnotAdd { page, mut annot } => {
                if rec.def(page).is_none() {
                    bail!("page not found");
                }
                let from = annot.id;
                annot.id = fresh_annot_id(rec, &mut HashSet::new());
                out.unencodable = annot.unencodable();
                let src = adopt(rec, page, &annot.name, from);
                add_annot(rec, page, self.mark(annot, src)?);
            }
            Edit::AnnotUpdate { page, annot } => {
                let id = annot.id;
                let was = (rec.annot(id))
                    .filter(|a| a.page == page)
                    .and_then(AnnotRec::mark_record)
                    .context("annotation not found")?
                    .clone();
                out.unencodable = annot.unencodable();
                // What it holds stays while it names the same.
                let mark = if holds_same(&was.annot.kind, &annot.kind) {
                    Mark {
                        annot,
                        ..Mark::clone(&was)
                    }
                } else {
                    self.mark(annot, was.src.clone())?
                };
                let body = AnnotBody::Mark(Arc::new(mark));
                rec.put(Key::Annot(id), Some(Val::Annot(AnnotRec { page, body })));
            }
            Edit::AnnotDelete { page, id } => {
                let a = (rec.annot(id))
                    .filter(|a| a.page == page)
                    .context("annotation not found")?;
                if a.widget().is_some() {
                    bail!("a field's widget goes only with its page");
                }
                let name = a.name();
                // Its thread goes with it: replies and statuses, and
                // theirs, ours and other tools'.
                let mut gone = thread_of(rec, page, HashSet::from([name]));
                gone.insert(id);
                drop_annots(rec, page, |id, _| gone.contains(&id));
                // A connector is one of a group by label: the others stay
                // (M9 decision 8).
            }
            Edit::CommentsDelete { page, names } => {
                if rec.def(page).is_none() {
                    bail!("page not found");
                }
                // Every comment of another tool's on the page by those
                // names (a file may name two alike), and their threads:
                // replies and statuses, and theirs, ours and other tools'.
                let names: HashSet<String> = names.into_iter().collect();
                let mut gone: HashSet<AnnotId> = (rec.foreign_on(page))
                    .filter(|(_, f)| names.contains(&f.summary.name))
                    .map(|(id, _)| id)
                    .collect();
                gone.extend(thread_of(rec, page, names));
                drop_annots(rec, page, |id, _| gone.contains(&id));
            }
            Edit::ConnectorAdd {
                a: (page_a, mut a),
                b: (page_b, mut b),
            } => {
                if rec.def(page_a).is_none() || rec.def(page_b).is_none() {
                    bail!("page not found");
                }
                let mut taken = HashSet::new();
                a.id = fresh_annot_id(rec, &mut taken);
                b.id = fresh_annot_id(rec, &mut taken);
                if let AnnotKind::Connector { partner, .. } = &mut a.kind {
                    *partner = Some(b.id);
                }
                if let AnnotKind::Connector { partner, .. } = &mut b.kind {
                    *partner = Some(a.id);
                }
                out.unencodable = a.unencodable() || b.unencodable();
                for (page, annot) in [(page_a, a), (page_b, b)] {
                    add_annot(rec, page, Mark::from(annot));
                }
            }
            Edit::SetTextLayers { layers } => {
                for (id, layer) in layers {
                    let def = rec.def(id).context("page not found")?;
                    let def = PageDef {
                        content: def.content.with_ocr_text(Some(layer)),
                        ..PageDef::clone(def)
                    };
                    rec.put(Key::Page(id), Some(Val::Page(Arc::new(def))));
                }
            }
            Edit::ClearTextLayers { ids } => {
                for id in ids {
                    let Some(def) = rec.def(id).filter(|d| d.content.ocr_text().is_some()) else {
                        continue;
                    };
                    let def = PageDef {
                        content: def.content.with_ocr_text(None),
                        ..PageDef::clone(def)
                    };
                    rec.put(Key::Page(id), Some(Val::Page(Arc::new(def))));
                }
            }
            Edit::SetHeaderFooter { header_footer } => {
                let hf = header_footer.filter(|h| !h.is_empty());
                out.unencodable = hf.as_ref().is_some_and(|h| h.unencodable());
                // The generator that draws it: set again, it keeps its id.
                let drawn = (rec.generators().values())
                    .find(|g| g.spec.header_footer().is_some())
                    .map(|g| g.id);
                match (hf, drawn) {
                    (Some(hf), id) => {
                        let id = id.unwrap_or_else(|| {
                            GenId::draw(&mut fastrand::Rng::new(), |id| {
                                rec.generators().contains_key(&id)
                            })
                        });
                        let spec = GenSpec::HeaderFooter(Arc::new(hf));
                        let g = Arc::new(Generator { id, spec });
                        rec.put(Key::Generator(id), Some(Val::Generator(g)));
                    }
                    (None, Some(id)) => rec.put(Key::Generator(id), None),
                    (None, None) => {}
                }
            }
            Edit::SetPageLabels { ranges } => {
                let labels = (!ranges.is_empty()).then(|| Val::Labels(ranges.into()));
                rec.put(Key::Labels, labels);
            }
            Edit::RebasePages { pages: rebases } => {
                let mut copies = NextCopy::of(rec);
                for r in rebases {
                    let def = rec.def(r.id).context("page to rebase not found")?;
                    let info = self.sources.get(&r.source).context("unknown source")?;
                    if r.index >= info.src.page_count() {
                        bail!("page {} isn't in the source", r.index);
                    }
                    if def.base.source == r.source && def.base.index == r.index {
                        continue;
                    }
                    // Its text over it and its turn stay: they're in
                    // PageSpace, which a copy keeps.
                    let content = def.content.clone();
                    let rotate = rec.geom(r.id).context("page to rebase not found")?.rotate;
                    let (new, geom) = copies.page(r.source, &info.src, r.index);
                    let def = PageDef {
                        content,
                        ..PageDef::clone(&new)
                    };
                    let geom = Geom { rotate, ..*geom };
                    rec.put(Key::Page(r.id), Some(Val::Page(Arc::new(def))));
                    rec.put(Key::Geom(r.id), Some(Val::Geom(Arc::new(geom))));
                }
            }
        }
        Ok(out)
    }
}

/// Each slot's change over `steps` as one prim, on the last step that
/// changed it: from the first step's before to the last one's after, and
/// none when they're the same (M10.5 §6.2). So a batch that puts a slot
/// back changes nothing. Every step keeps its edit, with or without prims.
fn coalesce(steps: &mut [Step]) {
    // Each slot's first before, and the first and last steps that put it.
    let mut spans: BTreeMap<Key, (Option<Val>, usize, usize)> = BTreeMap::new();
    for (i, step) in steps.iter().enumerate() {
        for p in step.prims.iter() {
            spans
                .entry(p.key)
                .and_modify(|(_, _, last)| *last = i)
                .or_insert_with(|| (p.before.clone(), i, i));
        }
    }
    let spans_steps = |p: &Prim| {
        let (_, first, last) = &spans[&p.key];
        first != last
    };
    for (i, step) in steps.iter_mut().enumerate() {
        if !step.prims.iter().any(spans_steps) {
            continue;
        }
        step.prims = step
            .prims
            .iter()
            .filter_map(|p| {
                let (before, _, last) = &spans[&p.key];
                (*last == i && !same(before.as_ref(), p.after.as_ref())).then(|| Prim {
                    key: p.key,
                    before: before.clone(),
                    after: p.after.clone(),
                })
            })
            .collect();
    }
}

/// What a version holds that no other does (M10.5 §3.9), from the steps
/// that made it and its state: its snapshot's bytes, which trimming frees,
/// and its steps', which only pruning the version frees.
///
/// A step's before-values are its parent's after-values, so each value is
/// counted once, by the version that made it, and an `Arc` once however
/// many of the version's prims hold it: a page's definition, its boxes,
/// its content stack (a duplicate shares its original's), an
/// annotation's body, another tool's annotation's summary and a widget's
/// props (their copies share them), a field node and its terminal's
/// props (unless the node it replaces has them), a bookmark, the labels,
/// the form, a generator. A source's bytes and a blob's aren't a
/// value's: the store counts them. The values are the snapshot's too, so
/// the snapshot adds only the im nodes it copied to put them
/// ([`DocState::snapshot_bytes`]). The order, a page's annotations, the
/// top-level bookmarks and the form count in full, though im shares all
/// but the changed paths of the order, the top-level bookmarks and the
/// form's roots and names with the parent's: an over-count.
fn accounting(steps: &[Step], state: &DocState) -> (usize, usize) {
    let mut step_bytes = 0;
    let mut arcs = HashSet::new();
    let mut once = |arc: *const ()| arcs.insert(arc);
    for s in steps {
        step_bytes += size_of::<Step>() + s.edit.heap_size() + s.line.heap_size();
        for p in s.prims.iter() {
            let after = match p.key {
                Key::Order | Key::OutlineRoots | Key::PageAnnots(_) => p.after.heap_size(),
                Key::Page(_) => match &p.after {
                    Some(Val::Page(def)) if once(Arc::as_ptr(def).cast()) => {
                        // Its stack counts once too: a duplicate shares it.
                        let stack = once(Arc::as_ptr(&def.content).cast());
                        size_of::<PageDef>() + if stack { def.heap_size() } else { 0 }
                    }
                    _ => 0,
                },
                Key::Geom(_) => match &p.after {
                    Some(Val::Geom(g)) if once(Arc::as_ptr(g).cast()) => size_of::<Geom>(),
                    _ => 0,
                },
                Key::Bookmark(_) => match &p.after {
                    Some(Val::Bookmark(b)) if once(Arc::as_ptr(b).cast()) => {
                        size_of::<Bookmark>() + b.heap_size()
                    }
                    _ => 0,
                },
                Key::Annot(_) => match &p.after {
                    Some(Val::Annot(a)) => match &a.body {
                        AnnotBody::Mark(m) if once(Arc::as_ptr(m).cast()) => {
                            size_of::<Mark>() + m.heap_size()
                        }
                        AnnotBody::Foreign(f) if once(Arc::as_ptr(f).cast()) => {
                            // Its summary counts once too: a copy shares it.
                            let summary = once(Arc::as_ptr(&f.summary).cast());
                            size_of::<Foreign>() + if summary { f.heap_size() } else { 0 }
                        }
                        AnnotBody::Widget(w) if once(Arc::as_ptr(w).cast()) => {
                            // Its props count once too: a copy shares them.
                            let props = once(Arc::as_ptr(&w.props).cast());
                            size_of::<Widget>() + w.heap_size() - if props { 0 } else { w.shared() }
                        }
                        AnnotBody::Mark(_) | AnnotBody::Foreign(_) | AnnotBody::Widget(_) => 0,
                    },
                    _ => 0,
                },
                Key::Field(_) => match &p.after {
                    Some(Val::Field(node)) if once(Arc::as_ptr(node).cast()) => {
                        // Its terminal's props count once too, and not when
                        // the node it replaces has them: a copy shares them,
                        // and a field whose widgets went keeps them.
                        let had = |props: &Arc<FieldProps>| match &p.before {
                            Some(Val::Field(was)) => {
                                (was.term.as_ref()).is_some_and(|t| Arc::ptr_eq(&t.props, props))
                            }
                            _ => false,
                        };
                        let props = (node.term.as_ref())
                            .is_none_or(|t| !had(&t.props) && once(Arc::as_ptr(&t.props).cast()));
                        size_of::<FieldNode>() + node.heap_size()
                            - if props { 0 } else { node.shared() }
                    }
                    _ => 0,
                },
                Key::Form => match &p.after {
                    Some(Val::Form(form)) if once(Arc::as_ptr(form).cast()) => {
                        size_of::<FormDoc>() + form.heap_size()
                    }
                    _ => 0,
                },
                Key::Labels => match &p.after {
                    Some(Val::Labels(labels)) if once(Arc::as_ptr(labels).cast()) => {
                        labels.heap_size()
                    }
                    _ => 0,
                },
                Key::Generator(_) => match &p.after {
                    Some(Val::Generator(g)) if once(Arc::as_ptr(g).cast()) => {
                        size_of::<Generator>() + g.heap_size()
                    }
                    _ => 0,
                },
            };
            step_bytes += size_of::<Prim>() + after;
        }
    }
    let keys = steps.iter().flat_map(|s| s.prims.iter().map(|p| p.key));
    (state.snapshot_bytes(keys), step_bytes)
}

/// `order` with `new` put in at position `at`, by one split and two
/// joins: inserting them one by one edits the tree once a page (189 ms for
/// half of 7,000 pages, against half a millisecond).
fn splice(
    order: &Vector<PageId>,
    at: usize,
    new: impl IntoIterator<Item = PageId>,
) -> Vector<PageId> {
    let mut out = order.clone();
    let tail = out.split_off(at);
    out.extend(new);
    out.append(tail);
    out
}

/// The copy of each source page a page made next takes (M10.5 §3.3): one
/// past the highest any page holds, by the source's bytes and the page,
/// whichever registration brought it. Counted once an edit.
struct NextCopy(HashMap<(crate::store::BlobId, u32), u32>);

impl NextCopy {
    fn of(st: &DocState) -> NextCopy {
        let mut next = HashMap::new();
        for (_, def, _) in st.pages() {
            let n = next.entry((def.base.src.id(), def.base.index)).or_insert(0);
            *n = (*n).max(def.copy + 1);
        }
        NextCopy(next)
    }

    /// The copy a page showing `base` takes.
    fn take(&mut self, base: &PageBase) -> u32 {
        let n = self.0.entry((base.src.id(), base.index)).or_insert(0);
        *n += 1;
        *n - 1
    }

    /// A new page showing page `index` of registration `source`, as its
    /// next copy: nothing drawn over it, and the page's own boxes.
    fn page(&mut self, source: SourceId, src: &Src, index: u32) -> (Arc<PageDef>, Arc<Geom>) {
        let base = PageBase {
            source,
            src: src.clone(),
            index,
        };
        let def = PageDef {
            copy: self.take(&base),
            base,
            content: ContentStack::empty(),
        };
        (Arc::new(def), Arc::new(src.geom(index)))
    }
}

// ----- outline helpers: bookmarks are records, their kids by id ----------

/// Bookmarks nested by depth, with new ids: the top-level ones' ids in
/// order, and every one in pre-order.
type Nested = (Vector<OutlineId>, Vec<(OutlineId, Bookmark)>);

/// The bookmarks under `parent`, or at the top level: None when there's
/// no such bookmark.
fn kids_of(st: &DocState, parent: Option<OutlineId>) -> Option<Vector<OutlineId>> {
    match parent {
        None => Some(st.outline_roots().clone()),
        Some(p) => Some(st.bookmark(p)?.kids.clone()),
    }
}

/// Puts `kids` under `parent`, a bookmark there is, or at the top level.
fn put_kids(rec: &mut Rec, parent: Option<OutlineId>, kids: Vector<OutlineId>) {
    match parent {
        None => rec.put(Key::OutlineRoots, Some(Val::OutlineRoots(kids))),
        Some(p) => {
            let parent = rec.bookmark(p).expect("a parent in the outline");
            let b = Bookmark {
                kids,
                ..Bookmark::clone(parent)
            };
            rec.put(Key::Bookmark(p), Some(Val::Bookmark(Arc::new(b))));
        }
    }
}

/// Puts bookmark `id` under `parent` (or at the top level), after `after`
/// there, or first.
fn outline_insert(
    rec: &mut Rec,
    parent: Option<OutlineId>,
    after: Option<OutlineId>,
    id: OutlineId,
) -> Result<()> {
    let mut kids = kids_of(rec, parent).context("parent bookmark not found")?;
    let at = match after {
        Some(a) => {
            kids.index_of(&a)
                .context("'after' bookmark not among siblings")?
                + 1
        }
        None => 0,
    };
    kids.insert(at, id);
    put_kids(rec, parent, kids);
    Ok(())
}

/// Takes bookmark `id` from under its parent, or from the top level; its
/// record and those under it stay. None when it isn't in the outline.
fn outline_unlink(rec: &mut Rec, id: OutlineId) -> Option<()> {
    let (parent, _) = rec.bookmark_place(id)?;
    let mut kids = kids_of(rec, parent)?;
    kids.retain(|&k| k != id);
    put_kids(rec, parent, kids);
    Some(())
}

/// Bookmark `id` and every one under it, in pre-order.
fn subtree(st: &DocState, id: OutlineId) -> Vec<OutlineId> {
    let mut out = Vec::new();
    let mut next = vec![id];
    while let Some(id) = next.pop() {
        out.push(id);
        if let Some(b) = st.bookmark(id) {
            next.extend(b.kids.iter().rev());
        }
    }
    out
}

/// Takes the bookmarks going to pages in `gone` out of the outline, each
/// one's kids in its place. Puts the bookmarks whose kids change, and the
/// top level when it does. How many went.
fn cascade_delete(rec: &mut Rec, gone: &HashSet<PageId>) -> usize {
    /// `kids` with each one going to a page in `gone` replaced by its own
    /// kids, so treated; the ones replaced into `removed`, and those whose
    /// kids change, with their new kids, into `changed`.
    fn kept(
        st: &DocState,
        kids: &Vector<OutlineId>,
        gone: &HashSet<PageId>,
        removed: &mut Vec<OutlineId>,
        changed: &mut Vec<(OutlineId, Vector<OutlineId>)>,
    ) -> Vector<OutlineId> {
        let mut out = Vector::new();
        for &id in kids {
            let Some(b) = st.bookmark(id) else {
                out.push_back(id);
                continue;
            };
            let below = kept(st, &b.kids, gone, removed, changed);
            if b.page().is_some_and(|p| gone.contains(&p)) {
                removed.push(id);
                out.append(below);
            } else {
                if below != b.kids {
                    changed.push((id, below));
                }
                out.push_back(id);
            }
        }
        out
    }
    let (mut removed, mut changed) = (Vec::new(), Vec::new());
    let st: &DocState = rec;
    let roots = kept(st, st.outline_roots(), gone, &mut removed, &mut changed);
    if removed.is_empty() {
        return 0;
    }
    for (id, kids) in changed {
        put_kids(rec, Some(id), kids);
    }
    for &id in &removed {
        rec.put(Key::Bookmark(id), None);
    }
    rec.put(Key::OutlineRoots, Some(Val::OutlineRoots(roots)));
    removed.len()
}

/// Bookmarks that came with pages, into the outline PDFlyer's way: one at
/// the top level titled as one there already gives that one its kids, and
/// the others go at the end. How many came in.
fn merge_outline(rec: &mut Rec, (roots, made): Nested) -> usize {
    let mut made: HashMap<OutlineId, Bookmark> = made.into_iter().collect();
    let mut top = rec.outline_roots().clone();
    for id in roots {
        let title = made[&id].title.clone();
        let same = top.iter().copied().find(|t| match made.get(t) {
            Some(b) => b.title == title,
            None => rec.bookmark(*t).is_some_and(|b| b.title == title),
        });
        let Some(same) = same else {
            top.push_back(id);
            continue;
        };
        let kids = made.remove(&id).expect("one that came in").kids;
        match made.get_mut(&same) {
            Some(b) => b.kids.append(kids),
            None => {
                let mut b = Bookmark::clone(rec.bookmark(same).expect("a top-level bookmark"));
                b.kids.append(kids);
                rec.put(Key::Bookmark(same), Some(Val::Bookmark(Arc::new(b))));
            }
        }
    }
    let added = made.len();
    for (id, b) in made {
        rec.put(Key::Bookmark(id), Some(Val::Bookmark(Arc::new(b))));
    }
    rec.put(Key::OutlineRoots, Some(Val::OutlineRoots(top)));
    added
}

/// The pages a bookmark "owns": from its first target (in document order)
/// up to, but not including, the first page targeted by a bookmark outside
/// its subtree that comes after it in reading order.
fn bookmark_span(st: &DocState, id: OutlineId) -> Option<Vec<PageId>> {
    let flat = st.bookmarks_preorder();
    let owned = span_at(&flat, id, st.page_count(), |p| st.pos_of(p))?;
    Some(
        st.order()
            .iter()
            .skip(owned.start)
            .take(owned.len())
            .copied()
            .collect(),
    )
}

/// The positions [`bookmark_span`] gives bookmark `id`, of `count` pages
/// whose positions `pos` gives, under the outline `flat` (in pre-order,
/// where a bookmark's subtree follows it).
fn span_at(
    flat: &[(u16, OutlineId, &Bookmark)],
    id: OutlineId,
    count: usize,
    pos: impl Fn(PageId) -> Option<usize>,
) -> Option<Range<usize>> {
    let at = flat.iter().position(|&(_, other, _)| other == id)?;
    let depth = flat[at].0;
    let len = 1 + flat[at + 1..]
        .iter()
        .take_while(|(d, _, _)| *d > depth)
        .count();
    let page = |&(_, _, b): &(u16, OutlineId, &Bookmark)| b.page().and_then(&pos);
    let start = flat[at..at + len].iter().filter_map(page).min()?;
    let end = (flat[..at].iter().chain(&flat[at + len..]))
        .filter_map(page)
        .filter(|&i| i > start)
        .min()
        .unwrap_or(count);
    Some(start..end)
}

// ----- annotation helpers: records, each page's by id ---------------------

/// Annotation `a` of a source page as a record of a page showing copy
/// `copy` of it, under the id the source's fact gives it: the object it
/// is in `src`, as a mark is only on copy 0, and a mark holding what it
/// holds there, its file decoded into `store`.
fn record_of(store: &Store, a: &FileAnnot, src: &Src, copy: u32) -> (AnnotId, AnnotBody) {
    let object = a.object.map(|id| ObjRef {
        src: src.clone(),
        id,
    });
    let body = match &a.body {
        FileBody::Mark { annot, held } => {
            let mark = Mark {
                src: object.filter(|_| copy == 0),
                ..Mark::from(Annot::clone(annot))
            };
            AnnotBody::Mark(Arc::new(holding(mark, store, src, *held)))
        }
        FileBody::Foreign { summary, .. } => AnnotBody::Foreign(Arc::new(Foreign {
            src: object,
            copy,
            summary: summary.clone(),
        })),
    };
    (a.id(), body)
}

/// `mark`, read from `src`, where it holds `held` (see [`FileBody::Mark`]),
/// with what that is: its page stamp's form, or its attachment's file
/// decoded into `store`. A file that can't be read is said, and the mark
/// holds none.
fn holding(mut mark: Mark, store: &Store, src: &Src, held: Option<ObjectId>) -> Mark {
    let Some(object) = held else {
        return mark;
    };
    match &mark.annot.kind {
        AnnotKind::PageStamp { .. } => {
            mark.stamp = Some(FormRef::Base(ObjRef {
                src: src.clone(),
                id: object,
            }));
        }
        AnnotKind::FileAttachment { name, size, .. } => {
            mark.file = file_in(store, src, object, *size)
                .inspect_err(|e| log::warn!("attachment {name}: {e:#}"))
                .ok();
        }
        _ => {}
    }
    mark
}

/// Whether marks of kinds `a` and `b` hold the same: they name the same
/// stamp's page or the same attachment's file, or neither holds anything.
fn holds_same(a: &AnnotKind, b: &AnnotKind) -> bool {
    match (a, b) {
        (AnnotKind::PageStamp { stamp: x, .. }, AnnotKind::PageStamp { stamp: y, .. })
        | (AnnotKind::FileAttachment { file: x, .. }, AnnotKind::FileAttachment { file: y, .. }) => {
            x == y
        }
        (AnnotKind::PageStamp { .. } | AnnotKind::FileAttachment { .. }, _)
        | (_, AnnotKind::PageStamp { .. } | AnnotKind::FileAttachment { .. }) => false,
        _ => true,
    }
}

/// [`Document::find_stamp`] in a document whose blobs are in `store` and
/// whose files are `sources`, `primary` among them: a snapshot's, too.
pub(crate) fn find_stamp(
    store: &Store,
    sources: &HashMap<SourceId, Arc<SourceInfo>>,
    primary: SourceId,
    stem: &str,
) -> Option<FormRef> {
    let named = || store.get(BlobId::from_stem(stem)?);
    let held = || {
        registered(sources, primary).find_map(|src| {
            let id = src.annots().stamp(stem)?;
            Some(FormRef::Base(ObjRef {
                src: src.clone(),
                id,
            }))
        })
    };
    let cached = || {
        let path = crate::generated::path_of(stem)?;
        let bytes = sheaf_core::read_file(&path).ok()?;
        let kind = BlobKind::Pdf {
            password: None,
            origin: Origin::File(path.into()),
        };
        let blob = store.intern(bytes, kind);
        blob.inspect_err(|e| log::warn!("stamp {stem}: {e}")).ok()
    };
    (named().map(|pdf| FormRef::Blob(pdf.into())))
        .or_else(held)
        .or_else(|| cached().map(|pdf| FormRef::Blob(pdf.into())))
}

/// [`Document::find_file`] in a document whose blobs are in `store` and
/// whose files are `sources`, `primary` among them: a snapshot's, too.
pub(crate) fn find_file(
    store: &Store,
    sources: &HashMap<SourceId, Arc<SourceInfo>>,
    primary: SourceId,
    stem: &str,
    size: u64,
) -> Option<BlobRef> {
    if let Some(file) = BlobId::from_stem(stem).and_then(|id| store.get(id)) {
        return Some(file.into());
    }
    registered(sources, primary).find_map(|src| {
        let stream = src.annots().file(stem)?;
        let file = file_in(store, src, stream, size);
        file.inspect_err(|e| log::warn!("attachment {stem}: {e:#}"))
            .ok()
    })
}

/// The registered files, the primary first and then as they came: where
/// a stem is looked for.
fn registered(
    sources: &HashMap<SourceId, Arc<SourceInfo>>,
    primary: SourceId,
) -> impl Iterator<Item = &Src> {
    let mut ids: Vec<SourceId> = sources
        .keys()
        .copied()
        .filter(|&id| id != primary)
        .collect();
    ids.sort_unstable();
    std::iter::once(primary)
        .chain(ids)
        .filter_map(|id| Some(&sources.get(&id)?.src))
}

/// Embedded file `stream` of `src`, which an attachment mark saying its
/// file is `size` bytes holds, decoded into `store`.
fn file_in(store: &Store, src: &Src, stream: ObjectId, size: u64) -> Result<BlobRef> {
    use crate::content::ReadObjects;
    let bytes = src
        .pdf()
        .read(stream, |o| match o {
            lopdf::Object::Stream(s) => crate::attachments::mark_file_bytes(s, size),
            _ => bail!("the embedded file isn't a stream"),
        })
        .context("reading the embedded file")??;
    Ok(store.intern(bytes.into(), BlobKind::Attachment)?.into())
}

/// Puts `ids` as page `page`'s annotations: none when it's empty.
fn put_page_annots(rec: &mut Rec, page: PageId, ids: Vector<AnnotId>) {
    let val = (!ids.is_empty()).then_some(Val::PageAnnots(ids));
    rec.put(Key::PageAnnots(page), val);
}

/// Adds `mark` after page `page`'s others.
fn add_annot(rec: &mut Rec, page: PageId, mark: Mark) {
    let id = mark.annot.id;
    let body = AnnotBody::Mark(Arc::new(mark));
    rec.put(Key::Annot(id), Some(Val::Annot(AnnotRec { page, body })));
    let mut ids = rec.annot_ids_on(page).cloned().unwrap_or_default();
    ids.push_back(id);
    put_page_annots(rec, page, ids);
}

/// Takes page `page`'s annotations that `gone` says go off it. A widget
/// that goes leaves its field's list; the field stays, as it does in a
/// save, which keeps it while another widget does (the `widget.field`
/// cascade, which takes a field with no widget left, is slice 11's).
fn drop_annots(rec: &mut Rec, page: PageId, gone: impl Fn(AnnotId, &AnnotRec) -> bool) {
    let dropped: HashMap<AnnotId, Option<FieldId>> = (rec.annots_on(page))
        .filter(|&(id, a)| gone(id, a))
        .map(|(id, a)| (id, a.widget().map(|w| w.field)))
        .collect();
    if dropped.is_empty() {
        return;
    }
    for &id in dropped.keys() {
        rec.put(Key::Annot(id), None);
    }
    let mut ids = rec.annot_ids_on(page).cloned().unwrap_or_default();
    ids.retain(|id| !dropped.contains_key(id));
    put_page_annots(rec, page, ids);
    let fields: BTreeSet<FieldId> = dropped.values().flatten().copied().collect();
    for field in fields {
        let Some(was) = rec.field(field) else {
            continue;
        };
        let mut node = FieldNode::clone(was);
        if let Some(term) = &mut node.term {
            Arc::make_mut(term)
                .widgets
                .retain(|w| !dropped.contains_key(w));
        }
        put_field(rec, field, node);
    }
}

/// Puts `node` as field node `id`.
fn put_field(rec: &mut Rec, id: FieldId, node: FieldNode) {
    rec.put(Key::Field(id), Some(Val::Field(Arc::new(node))));
}

/// A parent named `under` that sheaf makes for instance `inst`'s fields,
/// whose names clashed with the document's.
fn sheaf_parent(under: &str, inst: FormInst) -> FieldNode {
    FieldNode {
        parent: None,
        partial: under.into(),
        src: None,
        inst,
        kids: Vector::new(),
        term: None,
    }
}

/// The annotations on page `page` in the threads of the comments named
/// `names`: replies and statuses to them, and to those, ours and other
/// tools'.
fn thread_of(st: &DocState, page: PageId, mut names: HashSet<String>) -> HashSet<AnnotId> {
    let mut thread = HashSet::new();
    loop {
        let more: Vec<(AnnotId, String)> = (st.annots_on(page))
            .filter(|(id, a)| !thread.contains(id) && a.parent().is_some_and(|p| names.contains(p)))
            .map(|(id, a)| (id, a.name()))
            .collect();
        if more.is_empty() {
            return thread;
        }
        for (id, name) in more {
            thread.insert(id);
            names.insert(name);
        }
    }
}

/// Another tool's comment on page `page` a mark named `name` takes the
/// place of, made editable or imported over it (M9 decision 5): of those
/// named so, the one whose id is `from` (the one the mark was made from),
/// else the first. It goes, with the parts grouped under it (`/RT
/// /Group`), which the mark draws itself, and the mark is the object it
/// was. Its replies stay, in the mark's thread.
fn adopt(rec: &mut Rec, page: PageId, name: &str, from: AnnotId) -> Option<ObjRef> {
    if name.is_empty() {
        return None;
    }
    let theirs: Vec<AnnotId> = (rec.annots_named(page, name))
        .filter(|&id| rec.annot(id).is_some_and(|a| a.foreign().is_some()))
        .collect();
    let id = (theirs.iter().copied())
        .find(|&id| id == from)
        .or(theirs.first().copied())?;
    let src = rec.annot(id)?.foreign()?.src.clone();
    drop_annots(rec, page, |other, a| {
        other == id
            || a.foreign()
                .is_some_and(|f| f.summary.grouped && f.summary.parent.as_deref() == Some(name))
    });
    src
}

/// Page `page`'s annotations onto page `copy`, copy `n` of the same page:
/// each with a fresh id. A mark is a mark of its own, with no name taken
/// from another tool (the copy isn't the comment replies elsewhere are
/// to) and no object, holding what the original holds; another tool's
/// annotation is copy `n` of its object, so one deleted from the page
/// isn't on the copy either (M10.5 bug 12). A widget's copy is in
/// `widgets`, by the original's id: [`copy_fields`](Document::copy_fields)
/// made it.
fn copy_annots(
    rec: &mut Rec,
    page: PageId,
    copy: PageId,
    n: u32,
    widgets: &HashMap<AnnotId, AnnotId>,
) {
    let annots: Vec<(AnnotId, AnnotBody)> = rec
        .annots_on(page)
        .map(|(id, a)| (id, a.body.clone()))
        .collect();
    let mut taken = HashSet::new();
    let mut ids = Vector::new();
    for (was, body) in annots {
        let (id, body) = match body {
            AnnotBody::Mark(m) => {
                let id = fresh_annot_id(rec, &mut taken);
                let mut annot = m.annot.clone();
                annot.id = id;
                annot.name.clear();
                let mark = Mark {
                    annot,
                    src: None,
                    ..Mark::clone(&m)
                };
                (id, AnnotBody::Mark(Arc::new(mark)))
            }
            AnnotBody::Foreign(f) => {
                let f = Foreign {
                    copy: n,
                    ..Foreign::clone(&f)
                };
                (
                    fresh_annot_id(rec, &mut taken),
                    AnnotBody::Foreign(Arc::new(f)),
                )
            }
            AnnotBody::Widget(_) => {
                ids.extend(widgets.get(&was).copied());
                continue;
            }
        };
        rec.put(
            Key::Annot(id),
            Some(Val::Annot(AnnotRec { page: copy, body })),
        );
        ids.push_back(id);
    }
    put_page_annots(rec, copy, ids);
}

/// Each page's annotations in `st`, after a save wrote them to `src`, the
/// page at its position there, whose annotations `saved` has: each mark
/// is the object there with its `/NM`, a stamp's page the form it draws
/// there, and each other tool's annotation the one there with its name
/// and subtype (the k-th of them on the page the k-th there), copy 0. A
/// mark the saved page hasn't got keeps its own as it was, with a log
/// line: the save didn't write what the document has. Another tool's
/// annotation it hasn't got goes, with a log line: the save left it out,
/// as it does a link to a page that isn't there. The records put anew,
/// by id; the ones gone, with their pages, into `gone`.
fn rebind_annots(
    st: &mut DocState,
    src: &Src,
    saved: &FileAnnots,
    gone: &mut Vec<(PageId, AnnotId)>,
) -> Vec<(AnnotId, AnnotRec)> {
    type Named<'a> = HashMap<&'a str, VecDeque<(Option<ObjectId>, &'a Arc<ForeignAnnot>)>>;
    let at = |id| ObjRef {
        src: src.clone(),
        id,
    };
    let mut out = Vec::new();
    for (index, page) in (0..).zip(st.order().clone()) {
        let objects: HashMap<String, (ObjectId, Option<ObjectId>)> = (saved.on(index).iter())
            .filter_map(|a| match &a.body {
                FileBody::Mark { annot, held } => Some((annot.name(), (a.object?, *held))),
                FileBody::Foreign { .. } => None,
            })
            .collect();
        // The saved page's other tools' annotations by name, each name's
        // in `/Annots` order.
        let mut theirs = Named::new();
        for (_, object, read) in saved.foreign_on(index) {
            (theirs.entry(&read.name).or_default()).push_back((object, read));
        }
        let annots: Vec<(AnnotId, AnnotRec)> =
            st.annots_on(page).map(|(id, a)| (id, a.clone())).collect();
        let mut left = Vector::new();
        for (id, a) in annots {
            let body = match &a.body {
                AnnotBody::Mark(m) => {
                    let name = m.annot.name();
                    let Some(&(object, held)) = objects.get(&name) else {
                        log::warn!("page {}: the saved file hasn't {name}", index + 1);
                        left.push_back(id);
                        continue;
                    };
                    let stamp = match held {
                        Some(form) if m.stamp.is_some() => Some(FormRef::Base(at(form))),
                        _ => m.stamp.clone(),
                    };
                    AnnotBody::Mark(Arc::new(Mark {
                        src: Some(at(object)),
                        stamp,
                        ..Mark::clone(m)
                    }))
                }
                AnnotBody::Foreign(f) => {
                    let named = theirs.get_mut(f.summary.name.as_str());
                    let first = named.and_then(|named| {
                        let i = named
                            .iter()
                            .position(|(_, r)| r.subtype == f.summary.subtype)?;
                        named.remove(i)
                    });
                    let Some((object, read)) = first else {
                        log::warn!(
                            "page {}: the saved file left out {} {}",
                            index + 1,
                            f.summary.subtype,
                            f.summary.name
                        );
                        st.set(&Key::Annot(id), None)
                            .expect("an annotation key holds an annotation");
                        gone.push((page, id));
                        continue;
                    };
                    let summary = if read.slot == f.summary.slot {
                        f.summary.clone()
                    } else {
                        Arc::new(ForeignAnnot {
                            slot: read.slot,
                            ..ForeignAnnot::clone(&f.summary)
                        })
                    };
                    AnnotBody::Foreign(Arc::new(Foreign {
                        src: object.map(at),
                        copy: 0,
                        summary,
                    }))
                }
                // The form's, with their fields (`rebind_fields`).
                AnnotBody::Widget(_) => {
                    left.push_back(id);
                    continue;
                }
            };
            left.push_back(id);
            let a = AnnotRec { page, body };
            st.set(&Key::Annot(id), Some(Val::Annot(a.clone())))
                .expect("an annotation key holds an annotation");
            out.push((id, a));
        }
        // Some went from this page: it lists the rest.
        if gone.last().is_some_and(|&(p, _)| p == page) {
            let list = (!left.is_empty()).then_some(Val::PageAnnots(left));
            st.set(&Key::PageAnnots(page), list)
                .expect("a page's annotations key holds a list");
        }
    }
    out
}

/// The form in `st`, after a save wrote it to `src`, registration `id`,
/// whose file is `name`: every node is the saved file's own instance's
/// now (copy 0), the node there with its full name (the k-th of a name
/// in tree order the k-th there, as forms repeat full names), and each
/// widget the one there at its place among its field's on the page at
/// its position, as the save wrote them. The saved instance is named for
/// the file, and whether scripts run stays as it was. One the saved file
/// hasn't got is from no object, with a log line: the save didn't write
/// what the document has. The nodes put anew, by id; the widgets into
/// `widgets`.
fn rebind_fields(
    st: &mut DocState,
    id: SourceId,
    src: &Src,
    name: Arc<str>,
    widgets: &mut Vec<(AnnotId, AnnotRec)>,
) -> Vec<(FieldId, Arc<FieldNode>)> {
    let saved = src.form().clone();
    let inst = FormInst { src: id, copy: 0 };
    let at = |id| ObjRef {
        src: src.clone(),
        id,
    };
    // The saved nodes by full name, each name's in tree order: a node
    // comes after its parent.
    let mut full: Vec<String> = Vec::with_capacity(saved.nodes.len());
    for n in &saved.nodes {
        let up = n.parent.map_or("", |p| full[p.0 as usize].as_str());
        full.push(full_name_of(up, &n.partial));
    }
    let mut by_name: HashMap<&str, VecDeque<FieldId>> = HashMap::new();
    for (i, name) in (0..).zip(&full) {
        by_name.entry(name).or_default().push_back(FieldId(i));
    }
    // The document's nodes in tree order, each the saved node it is.
    let mut next: Vec<(FieldId, String)> = (st.form().roots.iter().rev())
        .filter_map(|&root| Some((root, full_name_of("", &st.field(root)?.partial))))
        .collect();
    let mut paired = HashMap::new();
    let mut nodes = Vec::new();
    while let Some((fid, name)) = next.pop().filter(|_| nodes.len() < st.field_count()) {
        let Some(was) = st.field(fid).cloned() else {
            continue;
        };
        next.extend(
            (was.kids.iter().rev())
                .filter_map(|&kid| Some((kid, full_name_of(&name, &st.field(kid)?.partial)))),
        );
        let to = by_name.get_mut(name.as_str()).and_then(VecDeque::pop_front);
        match to {
            Some(to) => {
                paired.insert(fid, to);
            }
            None => log::warn!("the saved file hasn't the field {name}"),
        }
        let node = Arc::new(FieldNode {
            src: to.map(|to| at(saved.nodes[to.0 as usize].object)),
            inst,
            ..FieldNode::clone(&was)
        });
        st.set(&Key::Field(fid), Some(Val::Field(node.clone())))
            .expect("a field key holds a field node");
        nodes.push((fid, node));
    }
    for (index, page) in (0..).zip(st.order().clone()) {
        // The saved page's widgets, each field's in order.
        let mut theirs: HashMap<FieldId, VecDeque<ObjectId>> = HashMap::new();
        for w in saved.widgets.get(&index).into_iter().flatten() {
            theirs.entry(w.field).or_default().push_back(w.object);
        }
        let ours: Vec<(AnnotId, Arc<Widget>)> =
            st.widgets_on(page).map(|(id, w)| (id, w.clone())).collect();
        for (wid, w) in ours {
            let object = (paired.get(&w.field))
                .and_then(|to| theirs.get_mut(to))
                .and_then(VecDeque::pop_front);
            if object.is_none() {
                let field = st.full_name(w.field).unwrap_or_default();
                log::warn!(
                    "page {}: the saved file hasn't a widget of {field}",
                    index + 1
                );
            }
            let w = Widget {
                src: object.map(at),
                inst,
                ..Widget::clone(&w)
            };
            let rec = AnnotRec {
                page,
                body: AnnotBody::Widget(Arc::new(w)),
            };
            st.set(&Key::Annot(wid), Some(Val::Annot(rec.clone())))
                .expect("an annotation key holds an annotation");
            widgets.push((wid, rec));
        }
    }
    let names = match st.field_count() {
        0 => im::OrdMap::new(),
        _ => im::OrdMap::unit(inst, name),
    };
    let form = FormDoc {
        names,
        ..FormDoc::clone(st.form())
    };
    st.set(&Key::Form, Some(Val::Form(Arc::new(form))))
        .expect("the form key holds the form");
    nodes
}

/// A mark id no annotation in `st` has, and not in `taken` (ids handed
/// out earlier in the same edit, which records this one). Random rather
/// than counted, so two copies of a file never give different marks the
/// same `/NM`; 53 bits, so the JSON is exact in any reader.
fn fresh_annot_id(st: &DocState, taken: &mut HashSet<AnnotId>) -> AnnotId {
    loop {
        let id = AnnotId(fastrand::u64(1..1 << 53));
        if !taken.contains(&id) && !st.has_annot(id) {
            taken.insert(id);
            return id;
        }
    }
}

/// Whether `st` has signed fields from `source` (another file's: they're
/// kept, and their signatures don't cover this one).
fn signed_from(st: &DocState, source: SourceId) -> bool {
    st.terminals().any(|(_, node, term)| {
        node.inst.src == source && term.value == Some(crate::forms::Value::Signed)
    })
}

/// The copy of `source`'s form an instance brought in next takes: one
/// past the last `form` has, or 0.
fn next_copy(form: &FormDoc, source: SourceId) -> u32 {
    (form.names.keys())
        .filter(|inst| inst.src == source)
        .map(|inst| inst.copy + 1)
        .max()
        .unwrap_or(0)
}

/// Each page index's widgets that came in, with their places in its
/// `/Annots` (see `Document::take_in_fields`).
type Placed = HashMap<u32, Vec<(usize, AnnotId)>>;

/// The nodes of `form`, read from `src`, as instance `inst`'s: each under
/// the id the file's tree numbers it with, a terminal showing in the
/// widgets `shows` gives it.
fn nodes_of(
    form: &FileForm,
    src: &Src,
    inst: FormInst,
    mut shows: HashMap<FieldId, Vector<AnnotId>>,
) -> Vec<(FieldId, Arc<FieldNode>)> {
    let terms: HashMap<FieldId, &crate::forms::Field> =
        form.fields.iter().map(|f| (f.id, f)).collect();
    (form.nodes.iter().zip(0..))
        .map(|(n, i)| {
            let id = FieldId(i);
            let term = (terms.get(&id))
                .map(|f| Arc::new(Terminal::read(f, shows.remove(&id).unwrap_or_default())));
            let node = FieldNode {
                parent: n.parent,
                partial: n.partial.clone(),
                src: Some(ObjRef {
                    src: src.clone(),
                    id: n.object,
                }),
                inst,
                kids: n.kids.iter().copied().collect(),
                term,
            };
            (id, Arc::new(node))
        })
        .collect()
}

/// The name of the file a registration came from, as the reader knows it.
fn file_name(info: &SourceInfo) -> Arc<str> {
    info.name
        .file_name()
        .map_or_else(|| "".into(), |n| n.to_string_lossy().into())
}

/// The parent fields named `names` (full names, as their file has them)
/// go under, when any of them, or a parent of one, is a node's full name
/// in `st` already: the stem of `file`, or with `_2`, `_3`… when that's
/// taken. None when nothing clashes.
fn name_apart<'a>(
    st: &DocState,
    file: &str,
    names: impl IntoIterator<Item = &'a str>,
) -> Option<String> {
    if !field_nodes(names)
        .iter()
        .any(|n| st.field_named(n).is_some())
    {
        return None;
    }
    // A dot would make it two names.
    let stem = Path::new(file)
        .file_stem()
        .map(|s| s.to_string_lossy().replace('.', "_"))
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "form".to_string());
    (1..)
        .map(|n| match n {
            1 => stem.clone(),
            n => format!("{stem}_{n}"),
        })
        .find(|name| st.field_named(name).is_none())
}

/// Field `id`'s full name in its instance's form, as its file has it: up
/// to a parent sheaf made for it.
fn inst_name(st: &DocState, id: FieldId) -> String {
    let Some(node) = st.field(id) else {
        return String::new();
    };
    let mut partials = vec![&*node.partial];
    let mut at = node.parent.and_then(|p| st.field(p));
    while let Some(up) = at.filter(|n| n.src.is_some() && partials.len() <= st.field_count()) {
        partials.push(&*up.partial);
        at = up.parent.and_then(|p| st.field(p));
    }
    (partials.iter().rev()).fold(String::new(), |up, p| full_name_of(&up, p))
}

/// Field node `top` and every one under it.
fn field_subtree(st: &DocState, top: FieldId) -> Vec<FieldId> {
    let mut out = Vec::new();
    let mut next = vec![top];
    while let Some(id) = next.pop().filter(|_| out.len() <= st.field_count()) {
        out.push(id);
        next.extend(
            st.field(id)
                .into_iter()
                .flat_map(|n| n.kids.iter().copied()),
        );
    }
    out
}

/// Every node of a field tree with these full names: each name, and the
/// parents in it (what comes before each dot).
fn field_nodes<'a>(names: impl IntoIterator<Item = &'a str>) -> HashSet<&'a str> {
    let mut out = HashSet::new();
    for name in names {
        out.insert(name);
        for (i, _) in name.match_indices('.') {
            out.insert(&name[..i]);
        }
    }
    out
}

#[cfg(test)]
mod history_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::FixedCount;

    /// A document of `n` blank pages, in a store of its own.
    pub(super) fn new_doc(
        n: u32,
        outline: &[FlatOutlineItem],
        policy: Box<dyn HistoryPolicy>,
    ) -> Document {
        let store = Store::new();
        let primary = SourceInfo::blank(&store, n);
        Document::new(store, primary, outline, policy)
    }

    /// `n` blank pages opened as a file opens, saying `facts`: started on
    /// its pages, then ready.
    pub(super) fn opened_doc(n: u32, facts: Facts, policy: Box<dyn HistoryPolicy>) -> Document {
        let store = Store::new();
        let primary = SourceInfo::blank(&store, n);
        let mut d = crate::open::start(store, primary, policy);
        d.ready(facts).unwrap();
        d
    }

    pub(super) fn flat(items: &[(&str, Option<u32>, u16)]) -> Vec<FlatOutlineItem> {
        items
            .iter()
            .map(|(t, p, d)| FlatOutlineItem {
                title: t.to_string(),
                page: *p,
                depth: *d,
                view: None,
                color: None,
                style: Default::default(),
                open: true,
                object: None,
                kept: false,
            })
            .collect()
    }

    fn doc() -> Document {
        // Pages 0..6; bookmarks: A->0 { A1->1, A2->2 }, B->3, C->5
        new_doc(
            6,
            &flat(&[
                ("A", Some(0), 0),
                ("A1", Some(1), 1),
                ("A2", Some(2), 1),
                ("B", Some(3), 0),
                ("C", Some(5), 0),
            ]),
            Box::new(FixedCount(100)),
        )
    }

    /// A header of `text`, top left.
    pub(super) fn header(text: String) -> crate::stamps::HeaderFooter {
        let mut texts: [String; 6] = Default::default();
        texts[0] = text;
        crate::stamps::HeaderFooter {
            texts,
            ..Default::default()
        }
    }

    fn order(d: &Document) -> Vec<u32> {
        d.state()
            .pages()
            .map(|(_, def, _)| def.base.index)
            .collect()
    }

    /// What the page at `i` shows.
    fn def_at(d: &Document, i: usize) -> Arc<PageDef> {
        d.state().def(id_at(d, i)).unwrap().clone()
    }

    /// The `/Rotate` of the page at `i`.
    fn rotation(d: &Document, i: usize) -> Rotation {
        d.geom(id_at(d, i)).rotate
    }

    fn titles(d: &Document) -> Vec<(u16, String)> {
        d.state()
            .bookmarks_preorder()
            .iter()
            .map(|(depth, _, b)| (*depth, b.title.to_string()))
            .collect()
    }

    fn id_at(d: &Document, i: usize) -> PageId {
        d.page(i).unwrap()
    }

    fn bm(d: &Document, title: &str) -> OutlineId {
        d.state()
            .bookmarks_preorder()
            .iter()
            .find(|(_, _, b)| &*b.title == title)
            .unwrap()
            .1
    }

    /// The bookmark titled `title`.
    fn bookmark(d: &Document, title: &str) -> Arc<Bookmark> {
        d.state().bookmark(bm(d, title)).unwrap().clone()
    }

    /// While the file opens, every edit is refused before it draws an id
    /// or makes a version; once it's ready, the same edits are taken.
    #[test]
    fn an_edit_before_ready_is_not_ready() {
        let store = Store::new();
        let primary = SourceInfo::blank(&store, 3);
        let mut d = crate::open::start(store, primary, Box::new(FixedCount(10)));
        assert!(!d.is_ready());
        let page = id_at(&d, 0);
        let duplicate = || Edit::DuplicatePages { ids: vec![page] };
        let bookmark = || Edit::OutlineInsert {
            parent: None,
            after: None,
            title: "One".into(),
            target: Some(page),
            view: None,
        };
        let drawn = |d: &Document| {
            [&d.ids.page, &d.ids.outline, &d.ids.field].map(|c| c.load(Ordering::Relaxed))
        };
        let before = (d.current_version(), d.next_version, drawn(&d));
        let job = d.begin_job();
        let refused = [
            d.apply(duplicate(), "Duplicate").unwrap_err(),
            d.apply_batch(vec![duplicate(), bookmark()], "Both")
                .unwrap_err(),
            d.apply_in_job(job, bookmark(), "Bookmark").unwrap_err(),
        ];
        for e in &refused {
            assert!(e.is::<NotReady>(), "{e:#}");
        }
        assert_eq!((d.current_version(), d.next_version, drawn(&d)), before);
        assert_eq!(d.versions.len(), 1);
        d.ready(Facts::default()).unwrap();
        d.apply(duplicate(), "Duplicate").unwrap();
        d.apply_batch(vec![duplicate(), bookmark()], "Both")
            .unwrap();
        d.apply_in_job(job, bookmark(), "Bookmark").unwrap();
        assert_eq!((d.page_count(), d.state().outline_roots().len()), (5, 2));
        assert_eq!(d.versions.len(), 4);
    }

    /// `ready` puts the complete opened version in place of the one with
    /// the pages alone: the facts are on the pages the document showed
    /// while it opened, and there's nothing to save or undo. It's taken
    /// once.
    #[test]
    fn ready_replaces_the_opened_version() {
        let store = Store::new();
        let bytes = crate::forms::fixture::every_fact().bytes();
        let primary = file(&store, bytes, "facts.pdf");
        let src = primary.src.clone();
        let mut d = crate::open::start(store, primary, Box::new(FixedCount(10)));
        let loading = d.current_version();
        let pages: Vec<PageId> = d.state().order().iter().copied().collect();
        let st = d.state();
        assert_eq!(pages.len(), 3);
        assert!(st.bookmark_count() == 0 && st.annot_count() == 0 && st.field_count() == 0);
        assert!(st.labels().is_none() && st.header_footer().is_none());
        let view = d
            .ready(crate::open::finish(&src, &in_progress::NoProgress))
            .unwrap();
        assert!(d.is_ready());
        let opened = d.current_version();
        assert_ne!(opened, loading);
        assert!(!d.versions.contains_key(&loading));
        assert!(d.jump(loading).is_err());
        assert_eq!((d.root, d.saved), (opened, Some(opened)));
        let now: Vec<PageId> = d.state().order().iter().copied().collect();
        assert_eq!(now, pages);
        crate::open::tests::assert_every_fact(d.state());
        assert_eq!(view.form.fields, 1);
        assert!(!d.is_dirty() && !d.can_undo() && !d.can_redo());
        let snap = d.snapshot();
        assert!(!snap.outline_dirty && !snap.labels_dirty && !snap.header_footer_dirty);
        assert!(snap.annots_dirty.is_empty() && snap.fields_dirty.is_empty());
        assert!(!snap.removes_content && !snap.form_changed);
        // Facts that came again would be late: they're refused.
        assert!(
            d.ready(crate::open::finish(&src, &in_progress::NoProgress))
                .is_err()
        );
        assert_eq!(d.current_version(), opened);
        crate::open::tests::assert_every_fact(d.state());
    }

    /// The form `every_kind` has, as a file's form reads.
    fn every_kind_form() -> Arc<FileForm> {
        crate::forms::fixture::every_kind().source().form().clone()
    }

    #[test]
    fn form_fields_follow_their_pages() {
        let facts = Facts {
            form: every_kind_form(),
            ..Default::default()
        };
        let mut d = opened_doc(2, facts, Box::new(FixedCount(10)));
        let (p1, p2) = (id_at(&d, 0), id_at(&d, 1));
        let st = d.state();
        assert_eq!((st.field_count(), st.terminals().count()), (10, 9));
        let inst = FormInst {
            src: d.primary(),
            copy: 0,
        };
        assert!(st.fields().all(|(_, node)| node.inst == inst));
        assert_eq!(
            st.full_name(FieldId(1)).as_deref(),
            Some("form1[0].name[0]")
        );
        assert_eq!(st.field_named("form1[0].pick[0]"), Some(FieldId(5)));
        let on = |d: &Document, p: PageId| d.state().widgets_on(p).count();
        assert_eq!((on(&d, p1), on(&d, p2)), (6, 4));
        let listed = |d: &Document| -> usize {
            (d.state().terminals())
                .map(|(_, _, t)| t.widgets.len())
                .sum()
        };
        assert_eq!(listed(&d), 10);
        assert!(d.fields_dirty().is_empty() && d.snapshot().fields_dirty.is_empty());
        // A widget goes only with its page.
        let (widget, _) = d.state().widgets_on(p1).next().unwrap();
        let delete = Edit::AnnotDelete {
            page: p1,
            id: widget,
        };
        assert!(d.apply(delete, "delete").is_err());
        // Moving a page carries its widgets; deleting it takes them, and
        // undo brings them back.
        d.apply(
            Edit::MovePages {
                ids: vec![p2],
                before: Some(p1),
            },
            "move",
        )
        .unwrap();
        assert_eq!(on(&d, p2), 4);
        // The file's facts came once: none come after.
        let late = Facts {
            form: every_kind_form(),
            ..Default::default()
        };
        assert!(d.ready(late).is_err());
        d.apply(Edit::DeletePages { ids: vec![p2] }, "delete")
            .unwrap();
        // The fields stay, their widgets there gone from their lists (a
        // field with none left goes with slice 11's `widget.field` rule).
        assert_eq!((on(&d, p2), d.state().terminals().count()), (0, 9));
        assert_eq!(listed(&d), 6);
        d.undo().unwrap();
        assert_eq!((on(&d, p2), listed(&d)), (4, 10));
        // A replaced page shows another page, without them.
        let other = d.add_source(SourceInfo::blank(d.store(), 1));
        d.apply(
            Edit::ReplacePages {
                ids: vec![p1],
                source: other,
                range: 0..1,
                annots: Vec::new(),
            },
            "replace",
        )
        .unwrap();
        assert_eq!((on(&d, p1), listed(&d)), (0, 4));
        // Nothing changed a field, but the form isn't the file's.
        assert!(d.fields_dirty().is_empty());
        assert!(d.form_changed());
    }

    #[test]
    fn fields_dirty_compares_with_the_saved_form() {
        let facts = Facts {
            form: every_kind_form(),
            ..Default::default()
        };
        let mut d = opened_doc(2, facts, Box::new(FixedCount(10)));
        let terms: Vec<FieldId> = d.state().terminals().map(|(id, ..)| id).collect();
        // A field changed, one gone and one added in the state `ready` set,
        // as edits will do; and one with its widgets gone, which changes no
        // field.
        let (changed, gone, fewer, added) = (terms[0], terms[1], terms[2], FieldId(99));
        let root = d.root;
        let st = d.versions.get_mut(&root).unwrap().state.as_mut().unwrap();
        let node = |st: &DocState, id| FieldNode::clone(st.field(id).unwrap());
        // Field `id` without its widgets, which go off their pages.
        let unshown = |st: &mut DocState, id| {
            let mut f = node(st, id);
            let term = Arc::make_mut(f.term.as_mut().unwrap());
            for w in std::mem::take(&mut term.widgets) {
                let page = st.annot(w).unwrap().page;
                let mut ids = st.annot_ids_on(page).unwrap().clone();
                ids.retain(|&other| other != w);
                st.set(&Key::Annot(w), None).unwrap();
                let ids = (!ids.is_empty()).then_some(Val::PageAnnots(ids));
                st.set(&Key::PageAnnots(page), ids).unwrap();
            }
            f
        };
        let mut f = node(st, changed);
        Arc::make_mut(f.term.as_mut().unwrap()).value =
            Some(crate::forms::Value::Text("Bo Ng".into()));
        let mut new = f.clone();
        Arc::make_mut(new.term.as_mut().unwrap()).widgets.clear();
        let less = unshown(st, fewer);
        unshown(st, gone);
        let mut parent = node(st, f.parent.unwrap());
        parent.kids.retain(|&kid| kid != gone);
        parent.kids.push_back(added);
        for (id, node) in [
            (changed, Some(f.clone())),
            (gone, None),
            (added, Some(new)),
            (fewer, Some(less)),
            (f.parent.unwrap(), Some(parent)),
        ] {
            st.set(&Key::Field(id), node.map(|n| Val::Field(Arc::new(n))))
                .unwrap();
        }
        assert_eq!(st.check_invariants(), Ok(()));
        let want = HashSet::from([changed, gone, added]);
        assert_eq!(d.fields_dirty(), want);
        assert_eq!(d.snapshot().fields_dirty, want);

        // A field of the file itself changed, then saved for real: each
        // node is the saved file's own object, the projection reads each
        // from there, and nothing is dirty.
        let store = Store::new();
        let primary = file(
            &store,
            crate::forms::fixture::every_kind().bytes(),
            "form.pdf",
        );
        let facts = crate::open::finish(&primary.src, &in_progress::NoProgress);
        let mut d = crate::open::start(store, primary, Box::new(FixedCount(10)));
        d.ready(facts).unwrap();
        let root = d.root;
        let st = d.versions.get_mut(&root).unwrap().state.as_mut().unwrap();
        let changed = st.terminals().next().unwrap().0;
        let mut f = node(st, changed);
        Arc::make_mut(f.term.as_mut().unwrap()).value =
            Some(crate::forms::Value::Text("Bo Ng".into()));
        st.set(&Key::Field(changed), Some(Val::Field(Arc::new(f))))
            .unwrap();
        assert_eq!(d.fields_dirty(), HashSet::from([changed]));
        let saved = save_and_rebase(&mut d, false);
        let st = d.state();
        let ours = |node: &FieldNode| node.src.as_ref().is_some_and(|r| r.src == saved);
        assert!(st.fields().all(|(_, node)| ours(node)));
        let terms = st.terminals().count();
        assert_eq!(d.snapshot().legacy().fields.len(), terms);
        assert!(d.fields_dirty().is_empty() && d.snapshot().fields_dirty.is_empty());
    }

    /// Each rule a state's records keep (M10.5 §6.4), broken by puts no
    /// edit makes, is found, as the rule it is; and an edit naming a page
    /// there isn't for a bookmark is refused, so none makes one. The
    /// indexes' rules are `state`'s tests'.
    #[test]
    fn the_invariants_find_each_rule_broken() {
        let facts = Facts {
            form: every_kind_form(),
            outline: flat(&[("A", Some(0), 0), ("A1", Some(1), 1)]),
            ..Default::default()
        };
        let mut d = opened_doc(2, facts, Box::new(FixedCount(10)));
        let [p0, p1] = [0, 1].map(|i| id_at(&d, i));
        let a = bm(&d, "A");
        let header = Some(header("Draft".into()));
        d.apply(
            Edit::SetHeaderFooter {
                header_footer: header,
            },
            "header",
        )
        .unwrap();
        d.apply(
            Edit::AnnotAdd {
                page: p0,
                annot: square(0),
            },
            "add",
        )
        .unwrap();
        let to_no_page = [
            Edit::OutlineInsert {
                parent: None,
                after: None,
                title: "B".into(),
                target: Some(PageId(99)),
                view: None,
            },
            Edit::OutlineInsertTree {
                parent: None,
                after: None,
                items: vec![NewOutlineItem {
                    title: "B".into(),
                    target: Some(PageId(99)),
                    view: None,
                    depth: 0,
                }],
            },
            Edit::OutlineEdit {
                id: a,
                title: None,
                target: Some(Some(PageId(99))),
                color: None,
                view: None,
                style: None,
            },
        ];
        for edit in to_no_page {
            assert!(d.apply(edit, "to page 99").is_err(), "there's no page 99");
        }
        let st = d.state().clone();
        assert_eq!(st.check_invariants(), Ok(()));
        let broken = |key: Key, val: Option<Val>| {
            let mut st = st.clone();
            st.set(&key, val).unwrap();
            st.check_invariants().unwrap_err()
        };
        let (mark, rec) = (st.annots_on(p0))
            .find(|(_, a)| a.mark().is_some())
            .map(|(id, a)| (id, a.clone()))
            .unwrap();
        let (widget, shown) = (st.annots_on(p0))
            .find(|(_, a)| a.widget().is_some())
            .map(|(id, a)| (id, a.clone()))
            .unwrap();
        let (field, node, _) = (st.terminals())
            .find(|(.., t)| !t.widgets.is_empty())
            .unwrap();
        let top = st.form().roots[0];
        let field_with = |node: &FieldNode, widgets: &dyn Fn(&mut Vector<AnnotId>)| {
            let mut node = node.clone();
            widgets(&mut Arc::make_mut(node.term.as_mut().unwrap()).widgets);
            Some(Val::Field(Arc::new(node)))
        };
        let unshown = field_with(node, &|w| w.clear());
        let top_with = |change: &dyn Fn(&mut FieldNode)| {
            let mut node = FieldNode::clone(st.field(top).unwrap());
            change(&mut node);
            Some(Val::Field(Arc::new(node)))
        };
        let other_inst = FormInst {
            copy: 5,
            ..node.inst
        };
        let def = |change: &dyn Fn(&mut PageDef)| {
            let mut def = PageDef::clone(st.def(p1).unwrap());
            change(&mut def);
            Some(Val::Page(Arc::new(def)))
        };
        let a_with = |change: &dyn Fn(&mut Bookmark)| {
            let mut b = Bookmark::clone(st.bookmark(a).unwrap());
            change(&mut b);
            Some(Val::Bookmark(Arc::new(b)))
        };
        let gen_with = |id: u64| {
            let g = Generator {
                id: GenId(id),
                ..Generator::clone(st.generators().values().next().unwrap())
            };
            Some(Val::Generator(Arc::new(g)))
        };
        let mut twice = st.annot_ids_on(p0).unwrap().clone();
        twice.push_back(mark);
        let mut roots = st.form().roots.clone();
        roots.push_back(FieldId(99));
        let form = FormDoc {
            roots,
            ..FormDoc::clone(st.form())
        };
        let elsewhere = AnnotBody::Widget(Arc::new(Widget {
            inst: other_inst,
            ..Widget::clone(shown.widget().unwrap())
        }));
        let cases = [
            (
                Key::Order,
                Some(Val::Order(vec![p0, p0, p1].into())),
                "pages: PageId(0) is in the order twice",
            ),
            (
                Key::Page(PageId(99)),
                def(&|_| ()),
                "pages: the pages aren't the order's",
            ),
            (Key::Geom(p1), None, "pages: the pages with boxes aren't"),
            (
                Key::Page(p1),
                def(&|d| d.base.index = 99),
                "pages: PageId(1) shows page 100 of 2",
            ),
            (
                Key::Page(p1),
                st.get(&Key::Page(p0)),
                "pages: PageId(1) is copy 0 of a page",
            ),
            (
                Key::PageAnnots(PageId(99)),
                Some(Val::PageAnnots(Vector::unit(mark))),
                "annotations: PageId(99) isn't a page",
            ),
            (
                Key::PageAnnots(p1),
                Some(Val::PageAnnots(Vector::new())),
                "annotations: PageId(1) lists none",
            ),
            (
                Key::PageAnnots(p0),
                Some(Val::PageAnnots(twice)),
                "is listed twice",
            ),
            (
                Key::Annot(mark),
                Some(Val::Annot(AnnotRec {
                    page: p1,
                    ..rec.clone()
                })),
                "which isn't on it",
            ),
            (
                Key::Annot(AnnotId(7)),
                Some(Val::Annot(rec)),
                "annotations: 1 aren't listed",
            ),
            (Key::Field(field), unshown.clone(), "isn't one of"),
            (
                Key::Annot(widget),
                Some(Val::Annot(AnnotRec {
                    body: elsewhere,
                    ..shown
                })),
                "isn't in",
            ),
            (
                Key::Field(field),
                field_with(node, &|w| w.push_back(mark)),
                "which isn't its widget",
            ),
            (
                Key::Field(field),
                field_with(node, &|w| w.push_back(widget)),
                "the form: the fields list",
            ),
            (
                Key::Form,
                Some(Val::Form(Arc::new(form))),
                "the form: FieldId(99), under None, isn't a node",
            ),
            (
                Key::Field(field),
                Some(Val::Field(Arc::new(FieldNode {
                    parent: None,
                    ..node.clone()
                }))),
                "is under Some",
            ),
            (
                Key::Field(top),
                top_with(&|n| n.kids.push_back(n.kids[0])),
                "is in the tree twice",
            ),
            (
                Key::Field(top),
                top_with(&|n| n.inst = other_inst),
                "has no name",
            ),
            (
                Key::Field(FieldId(99)),
                unshown,
                "the form: 1 nodes aren't in the tree",
            ),
            (
                Key::Bookmark(a),
                a_with(&|b| b.kids.push_back(OutlineId(99))),
                "bookmarks: OutlineId(99) isn't a bookmark",
            ),
            (
                Key::Bookmark(a),
                a_with(&|b| b.kids = Vector::unit(a)),
                "is in the outline twice",
            ),
            (
                Key::Bookmark(a),
                a_with(&|b| {
                    b.target = Some(Target::Page {
                        page: PageId(99),
                        view: None,
                    })
                }),
                "goes to PageId(99), which isn't a page",
            ),
            (
                Key::Bookmark(OutlineId(99)),
                a_with(&|_| ()),
                "bookmarks: 1 bookmarks aren't in the outline",
            ),
            (
                Key::Generator(GenId(8)),
                gen_with(7),
                "generators: GenId(7) is under GenId(8)",
            ),
            (
                Key::Generator(GenId(7)),
                gen_with(7),
                "generators: 2 draw a header/footer",
            ),
        ];
        for (key, val, says) in cases {
            let why = broken(key, val).to_string();
            assert!(why.contains(says), "{key:?}: {why}");
        }
    }

    /// One file's bytes, interned twice, are one source; each insert still
    /// registers it anew, so each brings a form of its own (M10.5 slice 3:
    /// form identity stays per registration until slice 11).
    #[test]
    fn inserting_the_same_file_twice_gives_two_registrations_and_two_forms() {
        let mut fixture = crate::forms::fixture::every_kind();
        let bytes: Arc<[u8]> = fixture.bytes().into();
        let mut d = new_doc(1, &[], Box::new(FixedCount(10)));
        let mut sids = Vec::new();
        let mut renamed = Vec::new();
        for _ in 0..2 {
            let kind = BlobKind::file(Path::new("form.pdf"), None);
            let src = d.store().open(bytes.clone(), kind).unwrap();
            let sid = d.add_source(SourceInfo {
                src,
                name: "form.pdf".into(),
                derived: None,
                stamp: None,
            });
            let out = d
                .apply(
                    Edit::InsertPages {
                        source: sid,
                        range: 0..2,
                        before: None,
                        outline: None,
                        annots: Vec::new(),
                    },
                    "insert",
                )
                .unwrap();
            renamed.push(out.forms_renamed);
            sids.push(sid);
        }
        let [a, b] = [sids[0], sids[1]];
        assert_ne!(a, b);
        let (first, second) = (&d.source(a).unwrap().src, &d.source(b).unwrap().src);
        assert!(std::ptr::eq(&**first, &**second), "one source");
        // Its bytes are in the store once, beside the blank primary's.
        assert_eq!(
            d.store().bytes(),
            first.bytes().len() + d.source(d.primary()).unwrap().src.bytes().len()
        );
        // Two forms, an instance each: the second's fields are under a
        // name of their own.
        let st = d.state();
        let names = |sid| -> BTreeSet<String> {
            (st.terminals())
                .filter(|(_, node, _)| node.inst.src == sid)
                .map(|(id, _, _)| st.full_name(id).unwrap())
                .collect()
        };
        let (of_a, of_b) = (names(a), names(b));
        assert_eq!((of_a.len(), of_b.len()), (9, 9));
        assert!(of_a.is_disjoint(&of_b), "{of_a:?} {of_b:?}");
        assert!(of_b.iter().all(|n| n.starts_with("form.")), "{of_b:?}");
        assert_eq!(renamed, [vec![], vec![("form".to_string(), b)]]);
        let instances: Vec<(FormInst, &str)> =
            (st.form().names.iter()).map(|(i, n)| (*i, &**n)).collect();
        let inst = |src| FormInst { src, copy: 0 };
        assert_eq!(instances, [(inst(a), "form.pdf"), (inst(b), "form.pdf")]);
        // Every widget is its field's, and in its field's instance.
        for (id, node, term) in st.terminals() {
            for w in &term.widgets {
                let w = st.annot(*w).unwrap().widget().unwrap();
                assert_eq!((w.field, w.inst), (id, node.inst));
            }
        }
    }

    /// Every node is found by its full name, in the state an edit made and
    /// in that state rebuilt from the edit's prims, which put a new parent
    /// after the nodes under it; undo takes the names away.
    #[test]
    fn fields_are_found_by_their_full_names() {
        let facts = Facts {
            form: every_kind_form(),
            ..Default::default()
        };
        let mut d = opened_doc(2, facts, Box::new(FixedCount(10)));
        let opened = d.state().clone();
        let bytes = crate::forms::fixture::every_kind().bytes();
        let sid = d.add_source(file(d.store(), bytes, "form.pdf"));
        d.apply(
            Edit::InsertPages {
                source: sid,
                range: 0..2,
                before: None,
                outline: None,
                annots: Vec::new(),
            },
            "insert",
        )
        .unwrap();
        let found = |st: &DocState| {
            for (id, _) in st.fields() {
                let name = st.full_name(id).unwrap();
                assert_eq!(st.field_named(&name), Some(id), "{name}");
            }
        };
        let st = d.state();
        assert_eq!(st.field_named("form1[0].name[0]"), Some(FieldId(1)));
        let top = st.field_named("form").unwrap();
        assert!(st.field(top).unwrap().src.is_none());
        let renamed = st.field_named("form.form1[0].name[0]").unwrap();
        // Its id is drawn after theirs: a replay puts it after them.
        assert!(top > renamed);
        found(st);
        let (steps, _) = d.edge(d.current_version()).unwrap();
        let mut replayed = opened;
        for p in steps.iter().flat_map(|s| s.prims.iter()) {
            replayed.apply(p).unwrap();
        }
        found(&replayed);
        assert_eq!(replayed.field_named("form.form1[0].name[0]"), Some(renamed));
        d.undo().unwrap();
        assert_eq!(d.state().field_named("form.form1[0].name[0]"), None);
        assert_eq!(d.state().field_named("form"), None);
    }

    /// A form of one page, its field `a` with a keystroke script.
    fn scripted() -> Vec<u8> {
        use lopdf::{Object, dictionary};
        let mut f = crate::forms::fixture::Form::new(1);
        let a = f.text("a", 0, crate::forms::fixture::row(0));
        let js = dictionary! {
            "S" => "JavaScript", "JS" => Object::string_literal("event.rc = true;"),
        };
        f.dict(a).set("AA", dictionary! { "K" => js });
        f.bytes()
    }

    /// Scripts brought in from a second file go off, and stay off when a
    /// save makes the forms one file's; undo before the save turns them
    /// on again. A duplicate of the second file's page is another
    /// instance of its form, named as it is, renamed apart again.
    #[test]
    fn scripts_from_two_files_stay_off_after_a_save() {
        let store = Store::new();
        let primary = file(&store, scripted(), "a.pdf");
        let src = primary.src.clone();
        let mut d = crate::open::start(store, primary, Box::new(FixedCount(10)));
        d.ready(crate::open::finish(&src, &in_progress::NoProgress))
            .unwrap();
        assert!(!d.scripts_mixed());
        let sid = d.add_source(file(d.store(), scripted(), "b.pdf"));
        let insert = Edit::InsertPages {
            source: sid,
            range: 0..1,
            before: None,
            outline: None,
            annots: Vec::new(),
        };
        let out = d.apply(insert, "insert").unwrap();
        assert_eq!(out.forms_renamed, [("b".to_string(), sid)]);
        assert!(d.scripts_mixed());
        assert_eq!(d.script_sources(), [d.primary(), sid]);
        let ScriptState::Off { instances } = &d.state().form().scripts else {
            panic!("scripts on");
        };
        let inst = |src, copy| FormInst { src, copy };
        assert_eq!(**instances, [inst(d.primary(), 0), inst(sid, 0)]);
        d.undo().unwrap();
        assert!(!d.scripts_mixed());
        d.redo().unwrap();

        let page = id_at(&d, 1);
        let out = d
            .apply(Edit::DuplicatePages { ids: vec![page] }, "duplicate")
            .unwrap();
        assert_eq!(out.forms_renamed, [("b_2".to_string(), sid)]);
        let names: Vec<(FormInst, &str)> = (d.state().form().names.iter())
            .map(|(i, n)| (*i, &**n))
            .collect();
        assert_eq!(
            names,
            [
                (inst(d.primary(), 0), "a.pdf"),
                (inst(sid, 0), "b.pdf"),
                (inst(sid, 1), "b.pdf")
            ]
        );
        let st = d.state();
        assert!(st.field_named("b_2.a").is_some());

        save_and_rebase(&mut d, false);
        assert!(d.scripts_mixed(), "one file's forms, scripts still off");
        assert_eq!(d.script_sources(), [d.primary()]);
        let names: Vec<&str> = d.state().form().names.values().map(|n| &**n).collect();
        assert_eq!(names, ["saved.pdf"]);
    }

    #[test]
    fn opens_with_identity_pages_and_tree() {
        let d = doc();
        assert_eq!(order(&d), vec![0, 1, 2, 3, 4, 5]);
        assert_eq!(
            titles(&d),
            vec![
                (0, "A".into()),
                (1, "A1".into()),
                (1, "A2".into()),
                (0, "B".into()),
                (0, "C".into())
            ]
        );
        assert!(!d.is_dirty());
    }

    #[test]
    fn move_rotate_undo_redo() {
        let mut d = doc();
        let p5 = id_at(&d, 5);
        d.apply(
            Edit::MovePages {
                ids: vec![p5],
                before: Some(id_at(&d, 0)),
            },
            "move",
        )
        .unwrap();
        assert_eq!(order(&d), vec![5, 0, 1, 2, 3, 4]);
        d.apply(
            Edit::RotatePages {
                ids: vec![p5],
                by: Rotation::R90,
            },
            "rotate",
        )
        .unwrap();
        assert_eq!(rotation(&d, 0), Rotation::R90);
        assert_eq!(d.page_size(d.page(0).unwrap()).width, 792.0);
        assert!(d.is_dirty());
        assert_eq!(d.undo(), Ok(Some("rotate")));
        assert_eq!(rotation(&d, 0), Rotation::R0);
        assert_eq!(d.undo(), Ok(Some("move")));
        assert_eq!(order(&d), vec![0, 1, 2, 3, 4, 5]);
        assert!(!d.is_dirty());
        assert_eq!(d.redo(), Ok(Some("move")));
        assert_eq!(order(&d), vec![5, 0, 1, 2, 3, 4]);
        // A new edit discards the redo branch.
        d.apply(Edit::DeletePages { ids: vec![p5] }, "delete")
            .unwrap();
        assert!(!d.can_redo());
    }

    #[test]
    fn delete_cascades_and_promotes_children() {
        let mut d = doc();
        let out = d
            .apply(
                Edit::DeletePages {
                    ids: vec![id_at(&d, 0)],
                },
                "delete",
            )
            .unwrap();
        assert_eq!(out.pages_removed, 1);
        assert_eq!(out.bookmarks_removed, 1);
        // A is gone; A1 and A2 promoted to top level.
        assert_eq!(
            titles(&d),
            vec![
                (0, "A1".into()),
                (0, "A2".into()),
                (0, "B".into()),
                (0, "C".into())
            ]
        );
        d.undo().unwrap();
        assert_eq!(titles(&d).len(), 5);
    }

    #[test]
    fn insert_with_outline_merge() {
        let mut d = doc();
        let s = d.add_source(SourceInfo {
            name: "b.pdf".into(),
            ..SourceInfo::blank(d.store(), 3)
        });
        let out = d
            .apply(
                Edit::InsertPages {
                    source: s,
                    range: 0..3,
                    before: Some(id_at(&d, 3)),
                    outline: Some(flat(&[
                        ("A", Some(0), 0),
                        ("A3", Some(1), 1),
                        ("D", Some(2), 0),
                    ])),
                    annots: Vec::new(),
                },
                "insert",
            )
            .unwrap();
        assert_eq!(out.pages_added, 3);
        assert_eq!(out.bookmarks_added, 2);
        assert_eq!(order(&d), vec![0, 1, 2, 0, 1, 2, 3, 4, 5]);
        assert_eq!(def_at(&d, 3).base.source, s);
        // "A" merged: A3 joined A's children; "D" appended.
        assert_eq!(
            titles(&d),
            vec![
                (0, "A".into()),
                (1, "A1".into()),
                (1, "A2".into()),
                (1, "A3".into()),
                (0, "B".into()),
                (0, "C".into()),
                (0, "D".into())
            ]
        );
        // A3 targets the inserted page with source index 1.
        let a3 = bookmark(&d, "A3").page().unwrap();
        assert_eq!(d.index_of(a3), Some(4));
    }

    #[test]
    fn replace_keeps_ids_and_bookmarks() {
        let mut d = doc();
        let s = d.add_source(SourceInfo {
            name: "b.pdf".into(),
            ..SourceInfo::blank(d.store(), 2)
        });
        let p3 = id_at(&d, 3);
        d.apply(
            Edit::ReplacePages {
                ids: vec![p3],
                source: s,
                range: 0..2,
                annots: Vec::new(),
            },
            "replace",
        )
        .unwrap();
        assert_eq!(d.page(3).unwrap(), p3);
        assert_eq!(def_at(&d, 3).base.source, s);
        assert_eq!(def_at(&d, 4).base.source, s);
        assert_eq!(order(&d), vec![0, 1, 2, 0, 1, 4, 5]);
        assert_eq!(bookmark(&d, "B").page(), Some(p3));
    }

    /// A page listed twice is replaced once, and stays with the marks that
    /// come to it.
    #[test]
    fn a_page_listed_twice_is_replaced_once() {
        let mut d = doc();
        let s = d.add_source(SourceInfo::blank(d.store(), 1));
        let p3 = id_at(&d, 3);
        d.apply(
            Edit::ReplacePages {
                ids: vec![p3, p3],
                source: s,
                range: 0..1,
                annots: vec![(0, vec![square(0)])],
            },
            "replace",
        )
        .unwrap();
        assert_eq!(d.page_count(), 6);
        assert_eq!(def_at(&d, 3).base.source, s);
        assert_eq!(d.state().marks_on(p3).count(), 1);
    }

    #[test]
    fn duplicate() {
        let mut d = doc();
        d.apply(
            Edit::DuplicatePages {
                ids: vec![id_at(&d, 1)],
            },
            "dup",
        )
        .unwrap();
        assert_eq!(order(&d), vec![0, 1, 1, 2, 3, 4, 5]);
        assert_ne!(id_at(&d, 1), id_at(&d, 2));
    }

    /// Each page's base page and copy, by position.
    fn copies(d: &Document) -> Vec<(u32, u32)> {
        let pages = d.state().pages();
        pages
            .map(|(_, def, _)| (def.base.index, def.copy))
            .collect()
    }

    /// A page the file's tree lists twice (invalid, but real) opens as two
    /// pages over its one object: the object itself, copy 0, then copy 1.
    #[test]
    fn a_page_listed_twice_opens_as_copy_0_and_copy_1() {
        let mut f = crate::forms::fixture::Form::new(1);
        let page = f.pages[0];
        let root = f.dict(page).get(b"Parent").unwrap().as_reference().unwrap();
        f.dict(root).set("Kids", vec![page.into(), page.into()]);
        f.dict(root).set("Count", 2);
        let store = Store::new();
        let primary = file(&store, f.bytes(), "twice.pdf");
        let src = primary.src.clone();
        let d = Document::new(store, primary, &[], Box::new(FixedCount(10)));
        assert_eq!(copies(&d), [(0, 0), (1, 1)]);
        let object = |i| src.page(i).unwrap().id;
        assert_eq!(object(0), object(1), "one object, listed twice");
        assert!(!d.is_dirty());
    }

    /// A page made from a base page takes the copy after the highest any
    /// page holds of it: a duplicate, a duplicate of a duplicate, and the
    /// same bytes inserted again under another registration.
    #[test]
    fn copies_take_the_next_copy_of_their_base() {
        let mut d = new_doc(2, &[], Box::new(FixedCount(10)));
        let dup = |d: &mut Document, i| {
            let ids = vec![id_at(d, i)];
            d.apply(Edit::DuplicatePages { ids }, "dup").unwrap();
        };
        dup(&mut d, 0);
        dup(&mut d, 0);
        assert_eq!(copies(&d), [(0, 0), (0, 2), (0, 1), (1, 0)]);
        // A copy's copy is the next of the same base, and shows it.
        dup(&mut d, 2);
        assert_eq!(copies(&d)[3], (0, 3));
        let other = SourceInfo::blank(d.store(), 1);
        let first = insert_all(&mut d, other.clone());
        let again = insert_all(&mut d, other);
        assert_ne!(first, again, "two registrations");
        let inserted: Vec<(SourceId, u32)> = (d.state().pages().skip(5))
            .map(|(_, def, _)| (def.base.source, def.copy))
            .collect();
        assert_eq!(inserted, [(first, 0), (again, 1)]);
        // A page replaced by one the document shows is another copy of it.
        let last = id_at(&d, 6);
        let edit = Edit::ReplacePages {
            ids: vec![last],
            source: first,
            range: 0..1,
            annots: Vec::new(),
        };
        d.apply(edit, "replace").unwrap();
        assert_eq!(d.state().def(last).unwrap().copy, 2);
    }

    /// Moving pages changes the order and nothing else: one prim, and the
    /// new positions are there at once.
    #[test]
    fn a_reorder_puts_only_the_order() {
        let mut d = doc();
        let p5 = id_at(&d, 5);
        assert_eq!(d.index_of(p5), Some(5));
        d.apply(
            Edit::MovePages {
                ids: vec![p5],
                before: Some(id_at(&d, 0)),
            },
            "move",
        )
        .unwrap();
        let keys: Vec<Key> = d.versions[&d.cursor].steps[0]
            .prims
            .iter()
            .map(|p| p.key)
            .collect();
        assert_eq!(keys, [Key::Order]);
        assert_eq!(d.index_of(p5), Some(0));
        assert_eq!(d.index_of(id_at(&d, 1)), Some(1));
    }

    /// Recognized text is what's drawn over the page: setting it puts the
    /// page's record and nothing else, and a duplicate shows the same
    /// text, the one `Arc`.
    #[test]
    fn ocr_text_is_the_page_s_content() {
        let mut d = doc();
        let page = id_at(&d, 1);
        set(&mut d, &[1], "a");
        let keys: Vec<Key> = d.versions[&d.cursor].steps[0]
            .prims
            .iter()
            .map(|p| p.key)
            .collect();
        assert_eq!(keys, [Key::Page(page)]);
        let def = d.state().def(page).unwrap().clone();
        assert_eq!(def.content.over.len(), 1);
        d.apply(Edit::DuplicatePages { ids: vec![page] }, "dup")
            .unwrap();
        let copy = d.state().def(id_at(&d, 2)).unwrap();
        assert!(Arc::ptr_eq(&copy.content, &def.content));
        assert_eq!((copy.base.index, copy.copy), (1, 1));
        // Cleared, the page holds the stack untouched pages share.
        let ids = vec![page];
        d.apply(Edit::ClearTextLayers { ids }, "clear").unwrap();
        let cleared = &d.state().def(page).unwrap().content;
        assert!(ContentStack::is_shared_empty(cleared));
    }

    #[test]
    fn outline_edit_insert_delete() {
        let mut d = doc();
        let a = bm(&d, "A");
        d.apply(
            Edit::OutlineEdit {
                id: a,
                title: Some("Alpha".into()),
                target: None,
                color: Some(Some([255, 0, 0])),
                view: None,
                style: None,
            },
            "e",
        )
        .unwrap();
        assert_eq!(titles(&d)[0].1, "Alpha");
        d.apply(
            Edit::OutlineInsert {
                parent: Some(a),
                after: Some(bm(&d, "A1")),
                title: "A1b".into(),
                target: None,
                view: None,
            },
            "ins",
        )
        .unwrap();
        assert_eq!(titles(&d)[2], (1, "A1b".into()));
        let out = d.apply(Edit::OutlineDelete { id: a }, "del").unwrap();
        assert_eq!(out.bookmarks_removed, 4);
        assert_eq!(titles(&d), vec![(0, "B".into()), (0, "C".into())]);
    }

    #[test]
    fn outline_move_with_pages() {
        let mut d = doc();
        // Move C (span: page 5) to before B (as first top-level after A? use after=A).
        let c = bm(&d, "C");
        d.apply(
            Edit::OutlineMove {
                id: c,
                parent: None,
                after: Some(bm(&d, "A")),
                with_pages: true,
            },
            "mv",
        )
        .unwrap();
        assert_eq!(
            titles(&d).iter().map(|t| t.1.as_str()).collect::<Vec<_>>(),
            vec!["A", "A1", "A2", "C", "B"]
        );
        // A's span is pages 0..3 (A, A1, A2 target 0,1,2; next outside is B->3), so C's page lands at index 3.
        assert_eq!(order(&d), vec![0, 1, 2, 5, 3, 4]);
        // Move B (span: pages 3,4) to the very start.
        let b = bm(&d, "B");
        d.apply(
            Edit::OutlineMove {
                id: b,
                parent: None,
                after: None,
                with_pages: true,
            },
            "mv",
        )
        .unwrap();
        assert_eq!(order(&d), vec![3, 4, 0, 1, 2, 5]);
        assert_eq!(titles(&d)[0].1, "B");
        d.undo().unwrap();
        assert_eq!(order(&d), vec![0, 1, 2, 5, 3, 4]);
    }

    #[test]
    fn outline_move_rejects_cycles() {
        let mut d = doc();
        let a = bm(&d, "A");
        let a1 = bm(&d, "A1");
        assert!(
            d.apply(
                Edit::OutlineMove {
                    id: a,
                    parent: Some(a1),
                    after: None,
                    with_pages: false
                },
                "x"
            )
            .is_err()
        );
    }

    fn rotate(d: &mut Document, page: usize, label: &str) {
        d.apply(
            Edit::RotatePages {
                ids: vec![id_at(d, page)],
                by: Rotation::R90,
            },
            label,
        )
        .unwrap();
    }

    #[test]
    fn an_edit_after_undo_keeps_the_old_branch() {
        let mut d = doc();
        rotate(&mut d, 0, "a");
        rotate(&mut d, 1, "b");
        rotate(&mut d, 2, "c");
        d.undo().unwrap();
        d.undo().unwrap();
        rotate(&mut d, 3, "d");
        let line: Vec<String> = d.timeline().iter().map(|v| v.label.clone()).collect();
        assert_eq!(line, vec!["Open", "a", "d"]);
        let a = &d.timeline()[1];
        assert_eq!(a.branches.len(), 1, "b and c are still there");
        let b = a.branches[0];
        assert_eq!(
            d.branch(b)
                .iter()
                .map(|v| v.label.as_str())
                .collect::<Vec<_>>(),
            vec!["b", "c"]
        );
        d.jump(b).unwrap();
        assert_eq!(rotation(&d, 1), Rotation::R90);
        assert_eq!(rotation(&d, 3), Rotation::R0);
        assert_eq!(d.redo(), Ok(Some("c")));
        // Undo to a, and redo now follows the branch last visited.
        d.undo().unwrap();
        d.undo().unwrap();
        assert_eq!(d.redo_label(), Some("b"));
    }

    #[test]
    fn jumps_go_back_and_forth_freely() {
        let mut d = doc();
        for i in 0..10 {
            rotate(&mut d, i % 6, &format!("r{i}"));
        }
        let states: Vec<(VersionId, DocState)> = d
            .timeline()
            .iter()
            .map(|v| v.id)
            .collect::<Vec<_>>()
            .into_iter()
            .map(|id| {
                d.jump(id).unwrap();
                (id, d.state().clone())
            })
            .collect();
        let last = states.last().unwrap().0;
        d.jump(states[2].0).unwrap();
        assert_eq!(*d.state(), states[2].1);
        assert!(d.timeline()[3].undone);
        d.jump(last).unwrap();
        assert_eq!(*d.state(), states.last().unwrap().1);
        assert_eq!(d.timeline().len(), 11);
    }

    #[test]
    fn history_trims_snapshots_and_replays_them() {
        let mut d = new_doc(3, &[], Box::new(FixedCount(3)));
        for i in 0..5 {
            d.apply(
                Edit::RotatePages {
                    ids: vec![id_at(&d, 0)],
                    by: Rotation::R90,
                },
                format!("r{i}"),
            )
            .unwrap();
        }
        // Three snapshots (the opened version's, the current one's and one
        // more); all six versions, with their edits.
        assert_eq!(d.stats().versions, 3);
        assert_eq!(d.version_count(), 6);
        // Undo goes all the way back, replaying what was dropped.
        for i in (0..5).rev() {
            assert_eq!(d.undo(), Ok(Some(format!("r{i}").as_str())));
            assert_eq!(rotation(&d, 0).degrees(), (i * 90) % 360);
        }
        assert!(!d.can_undo());
        assert!(!d.is_dirty(), "the opened version is the saved one");
        for _ in 0..5 {
            d.redo().unwrap();
        }
        assert_eq!(rotation(&d, 0), Rotation::R90);
    }

    /// Three rotations, keeping only the snapshots that must stay (the
    /// opened version's and the current one's): the versions, oldest
    /// first.
    fn trimmed_history() -> (Document, Vec<VersionId>) {
        let mut d = new_doc(6, &[], Box::new(FixedCount(1)));
        for i in 0..3 {
            rotate(&mut d, i, &format!("r{i}"));
        }
        let ids = d.timeline().iter().map(|v| v.id).collect();
        (d, ids)
    }

    /// Version `v`'s rotation, made to say the page was turned another way
    /// before: the prim, as it is now.
    fn damage_turn_before(d: &mut Document, v: VersionId) -> Prim {
        let step = &mut d.versions.get_mut(&v).unwrap().steps[0];
        let mut prims = step.prims.to_vec();
        let p = prims
            .iter_mut()
            .find(|p| matches!(p.key, Key::Geom(_)))
            .unwrap();
        let Some(Val::Geom(geom)) = &p.before else {
            panic!("a page has its boxes");
        };
        let rotate = geom.rotate.plus(Rotation::R180);
        p.before = Some(Val::Geom(Arc::new(Geom { rotate, ..**geom })));
        let damaged = p.clone();
        step.prims = prims.into();
        damaged
    }

    /// A version's lines are made when its edits are applied, so they still
    /// name pages once its parent's snapshot is dropped.
    #[test]
    fn a_trimmed_version_keeps_its_lines() {
        let (d, _) = trimmed_history();
        let views = d.timeline();
        assert!(
            views[1].replays && views[2].replays,
            "their snapshots were dropped"
        );
        let lines: Vec<&[String]> = views[1..].iter().map(|v| &v.edits[..]).collect();
        assert_eq!(
            lines,
            [
                ["Rotate page 1 90°"],
                ["Rotate page 2 90°"],
                ["Rotate page 3 90°"]
            ]
        );
    }

    #[test]
    fn a_corrupted_before_value_is_a_mismatch() {
        let (mut d, ids) = trimmed_history();
        let (parent, v) = (ids[1], ids[2]);
        assert!(d.versions[&v].state.is_none(), "its snapshot was dropped");
        let p = damage_turn_before(&mut d, v);
        assert_eq!(d.replay(v), Err(Damaged { version: v }));
        let mut st = d.replay(parent).unwrap();
        assert_eq!(st.apply(&p), Err(crate::prim::Mismatch { key: p.key }));
    }

    /// `f`'s result in a release build. A debug build stops at damage in
    /// history, once it's marked, so there `f` must panic.
    fn unless_debug<T>(f: impl FnOnce() -> T) -> Option<T> {
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
        assert_eq!(r.is_err(), cfg!(debug_assertions), "panics in debug only");
        r.ok()
    }

    #[test]
    fn undo_over_damaged_history_says_so() {
        let (mut d, ids) = trimmed_history();
        let (v, current) = (ids[2], ids[3]);
        let prims = d.versions[&v].steps[0].prims.clone();
        damage_turn_before(&mut d, v);
        if let Some(undone) = unless_debug(|| d.undo().map(|l| l.map(str::to_owned))) {
            assert_eq!(undone, Err(Damaged { version: v }));
        }
        assert_eq!(d.current_version(), current, "nothing was undone");
        let damaged: Vec<VersionId> = d
            .timeline()
            .iter()
            .filter(|view| view.damaged)
            .map(|view| view.id)
            .collect();
        assert_eq!(damaged, [v]);
        // Marked, it stays out of reach, even with its prims put right.
        d.versions.get_mut(&v).unwrap().steps[0].prims = prims;
        assert_eq!(d.replay(v), Err(Damaged { version: v }));
        if let Some(jumped) = unless_debug(|| d.jump(v).is_ok()) {
            assert!(!jumped, "it can't be reached");
        }
        assert_eq!(d.current_version(), current);
    }

    #[test]
    fn rebase_after_save_keeps_ids_and_history() {
        let mut d = doc();
        let p5 = id_at(&d, 5);
        d.apply(
            Edit::MovePages {
                ids: vec![p5],
                before: Some(id_at(&d, 0)),
            },
            "move",
        )
        .unwrap();
        d.apply(
            Edit::RotatePages {
                ids: vec![p5],
                by: Rotation::R90,
            },
            "rotate",
        )
        .unwrap();
        let old_primary = d.primary();
        let saved = save_and_rebase(&mut d, false);
        let new = d.primary();
        assert_ne!(new, old_primary);
        assert!(!d.is_dirty());
        assert_eq!(d.page(0).unwrap(), p5);
        assert_eq!(def_at(&d, 0).base.source, new);
        assert_eq!(def_at(&d, 0).base.src, saved);
        assert_eq!(order(&d), vec![0, 1, 2, 3, 4, 5]);
        // The rotation is the page's /Rotate, which a save writes as is.
        assert_eq!(rotation(&d, 0), Rotation::R90);
        assert_eq!(d.render_delta(p5), Rotation::R0);
        // Undo still reaches the pre-save version over the old source.
        d.undo().unwrap();
        assert_eq!(def_at(&d, 0).base.source, old_primary);
        assert_eq!(def_at(&d, 0).base.index, 5);
        assert!(d.is_dirty());
    }

    /// One Letter page turned `degrees` by its `/Rotate`, or by its
    /// parent's when `inherited`, as a source in `store`.
    fn turned(store: &Store, own: Option<i64>, parent: Option<i64>) -> SourceInfo {
        let mut f = crate::forms::fixture::Form::new(1);
        let page = f.pages[0];
        let up = f.dict(page).get(b"Parent").unwrap().as_reference().unwrap();
        for (at, degrees) in [(page, own), (up, parent)] {
            if let Some(degrees) = degrees {
                f.dict(at).set("Rotate", degrees);
            }
        }
        let kind = BlobKind::file(Path::new("turned.pdf"), None);
        SourceInfo {
            src: store.open(f.bytes().into(), kind).unwrap(),
            name: "turned.pdf".into(),
            derived: None,
            stamp: None,
        }
    }

    /// `d` saved, incrementally or by a rewrite, and rebased onto the
    /// saved file, which it returns.
    fn save_and_rebase(d: &mut Document, incremental: bool) -> Src {
        let snap = d.snapshot();
        let bytes = if incremental {
            crate::incremental::incremental(&snap)
        } else {
            crate::save::rewrite(&snap)
        }
        .unwrap();
        let kind = BlobKind::file(Path::new("saved.pdf"), None);
        let src = d.store().open(bytes.into(), kind).unwrap();
        d.rebase_to_saved(SourceInfo {
            src: src.clone(),
            name: "saved.pdf".into(),
            derived: None,
            stamp: None,
        });
        src
    }

    const LANDSCAPE: PageSize = PageSize {
        width: 792.0,
        height: 612.0,
    };

    /// A page from a file that turns it has that turn as its rotation,
    /// with nothing for PDFium to add, and shows turned.
    #[test]
    fn an_inserted_page_keeps_its_own_rotate() {
        let mut d = new_doc(1, &[], Box::new(FixedCount(10)));
        let s = d.add_source(turned(d.store(), Some(90), None));
        d.apply(
            Edit::InsertPages {
                source: s,
                range: 0..1,
                before: None,
                outline: None,
                annots: Vec::new(),
            },
            "insert",
        )
        .unwrap();
        let p = id_at(&d, 1);
        assert_eq!(d.render_delta(p), Rotation::R0);
        assert_eq!(d.page_size(p), LANDSCAPE);
        assert_eq!(d.geom(p).rotate, Rotation::R90);
        // Turned again, it's turned from there, and PDFium adds the turn.
        d.apply(
            Edit::RotatePages {
                ids: vec![p],
                by: Rotation::R90,
            },
            "rotate",
        )
        .unwrap();
        assert_eq!(rotation(&d, 1), Rotation::R180);
        assert_eq!(d.render_delta(p), Rotation::R90);
        assert_eq!(d.snapshot().render_delta(p), Rotation::R90);
        assert_eq!(d.page_size(p).width, 612.0);
    }

    /// A page's rotation is its `/Rotate`: a save writes it as it is, and
    /// the document rebased onto the saved file keeps it, with no turn
    /// left for PDFium to add.
    #[test]
    fn a_rotation_survives_a_save() {
        for incremental in [false, true] {
            let mut d = new_doc(2, &[], Box::new(FixedCount(10)));
            rotate(&mut d, 0, "rotate");
            let saved = save_and_rebase(&mut d, incremental);
            assert_eq!(saved.geom(0).rotate, Rotation::R90, "{incremental}");
            assert_eq!(saved.geom(1).rotate, Rotation::R0, "{incremental}");
            let p = id_at(&d, 0);
            assert_eq!(rotation(&d, 0), Rotation::R90, "{incremental}");
            assert_eq!(d.render_delta(p), Rotation::R0, "{incremental}");
            assert_eq!(d.page_size(p), LANDSCAPE, "{incremental}");
        }
    }

    /// A page turned upright is upright in the saved file whatever its
    /// parent's `/Rotate`: an incremental save leaves it under its parent,
    /// so it says 0. A page that's only marked keeps the `/Rotate` it has,
    /// its own or its parent's, a 135 too, which reads as 90.
    #[test]
    fn a_page_turned_upright_stays_upright_through_a_save() {
        let square = crate::annot::AnnotKind::Square {
            rect: crate::annot::Box2 {
                x: 100.0,
                y: 100.0,
                w: 50.0,
                h: 40.0,
            },
        };
        // The page's own /Rotate, its parent's, and the turn sheaf adds.
        let cases = [
            (None, Some(90), Some(Rotation::R270)),
            (Some(90), Some(180), Some(Rotation::R270)),
            (Some(0), Some(90), None),
            (Some(135), Some(180), None),
        ];
        for (own, parent, by) in cases {
            for incremental in [false, true] {
                let case = format!("{own:?} under {parent:?}, turned {by:?}, {incremental}");
                let store = Store::new();
                let primary = turned(&store, own, parent);
                let mut d = Document::new(store, primary, &[], Box::new(FixedCount(10)));
                let page = id_at(&d, 0);
                let was = rotation(&d, 0);
                let edit = match by {
                    Some(by) => Edit::RotatePages {
                        ids: vec![page],
                        by,
                    },
                    None => Edit::AnnotAdd {
                        page,
                        annot: mark(0, square.clone()),
                    },
                };
                d.apply(edit, "edit").unwrap();
                let want = rotation(&d, 0);
                assert_eq!(want, was.plus(by.unwrap_or(Rotation::R0)), "{case}");
                let saved = save_and_rebase(&mut d, incremental);
                assert_eq!(saved.geom(0).rotate, want, "{case}");
                assert_eq!(rotation(&d, 0), want, "{case}");
                if incremental && by.is_none() {
                    let pdf = lopdf::Document::load_mem(saved.bytes()).unwrap();
                    let dict = pdf.get_dictionary(pdf.get_pages()[&1]).unwrap();
                    let rotate = dict.get(b"Rotate").and_then(lopdf::Object::as_i64).ok();
                    assert_eq!(rotate, own, "{case}: the page's own /Rotate");
                }
            }
        }
    }

    /// A mark on a page sheaf turned shows where it did after a save, and
    /// the saved file has it where the model does: marks are in PageSpace,
    /// which neither the save nor the rebase moves.
    #[test]
    fn a_mark_on_a_rotated_page_stays_put_through_a_save() {
        let rect = crate::annot::Box2 {
            x: 100.0,
            y: 650.0,
            w: 50.0,
            h: 40.0,
        };
        for incremental in [false, true] {
            let mut d = new_doc(1, &[], Box::new(FixedCount(10)));
            rotate(&mut d, 0, "rotate");
            let page = id_at(&d, 0);
            let annot = mark(0, AnnotKind::Square { rect });
            d.apply(Edit::AnnotAdd { page, annot }, "add").unwrap();
            let shown = |d: &Document| {
                let AnnotKind::Square { rect } = d.state().marks_on(page).next().unwrap().kind
                else {
                    unreachable!("the square");
                };
                d.geom(d.page(0).unwrap()).to_view().apply_box(rect)
            };
            let before = shown(&d);
            let saved = save_and_rebase(&mut d, incremental);
            assert_eq!(shown(&d), before, "{incremental}");
            let pdf = lopdf::Document::load_mem(saved.bytes()).unwrap();
            let dict = pdf.get_dictionary(pdf.get_pages()[&1]).unwrap();
            let annots = dict.get(b"Annots").unwrap().as_array().unwrap();
            let (_, annot) = pdf.dereference(&annots[0]).unwrap();
            let r: Vec<f32> = annot
                .as_dict()
                .unwrap()
                .get(b"Rect")
                .unwrap()
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_float().unwrap() as f32)
                .collect();
            let centre = ((r[0] + r[2]) / 2.0, (r[1] + r[3]) / 2.0);
            let want = (rect.x + rect.w / 2.0, rect.y + rect.h / 2.0);
            assert!(
                (centre.0 - want.0).abs() < 1.0 / 64.0 && (centre.1 - want.1).abs() < 1.0 / 64.0,
                "{incremental}: /Rect {r:?}"
            );
        }
    }

    /// A save re-expresses the version it wrote, so the versions made from
    /// it before (undone, then saved) keep their own states: their prims
    /// are from it as it was.
    #[test]
    fn a_save_keeps_what_was_undone_before_it() {
        let mut d = new_doc(3, &[], Box::new(FixedCount(1)));
        rotate(&mut d, 0, "a");
        rotate(&mut d, 1, "b");
        let (b, want_b) = (d.current_version(), d.state().clone());
        d.undo().unwrap();
        rotate(&mut d, 2, "c");
        let (c, want_c) = (d.current_version(), d.state().clone());
        assert!(d.versions[&b].state.is_none(), "b's snapshot was dropped");
        d.undo().unwrap();
        d.rebase_to_saved(SourceInfo::blank(d.store(), 3));
        rotate(&mut d, 0, "d");
        d.jump(b).unwrap();
        assert_eq!(*d.state(), want_b);
        d.jump(c).unwrap();
        assert_eq!(*d.state(), want_c);
    }

    #[test]
    fn a_connector_pair_is_a_group_of_two() {
        use crate::annot::{AnnotKind, Box2};
        let mut d = doc();
        let (p0, p2) = (id_at(&d, 0), id_at(&d, 2));
        let mk = |label: &str| Annot {
            id: AnnotId(0),
            kind: AnnotKind::Connector {
                rect: Box2 {
                    x: 10.0,
                    y: 10.0,
                    w: 22.0,
                    h: 22.0,
                },
                label: label.into(),
                partner: None,
            },
            color: [255, 0, 0],
            opacity: 1.0,
            stroke: 1.0,
            author: "t".into(),
            contents: String::new(),
            modified: String::new(),
            created: String::new(),
            fill: None,
            dashed: false,
            cloudy: false,
            turn: Rotation::R0,
            name: String::new(),
        };
        assert_eq!(d.state().next_connector_label(), "1");
        d.apply(
            Edit::ConnectorAdd {
                a: (p0, mk("1")),
                b: (p2, mk("1")),
            },
            "c",
        )
        .unwrap();
        let a = d.state().marks_on(p0).next().unwrap().clone();
        let b = d.state().marks_on(p2).next().unwrap().clone();
        assert_eq!(a.partner(), Some(b.id));
        assert_eq!(b.partner(), Some(a.id));
        assert_eq!(d.state().find_annot(b.id).map(|(p, _)| p), Some(p2));
        assert_eq!(d.state().next_connector_label(), "2");
        // Deleting one end leaves the other: a group of one (decision 8).
        d.apply(Edit::AnnotDelete { page: p2, id: b.id }, "rm")
            .unwrap();
        assert_eq!(d.state().marks_on(p0).count(), 1);
        assert!(d.state().marks_on(p2).next().is_none());
        d.undo().unwrap();
        assert_eq!(d.state().marks_on(p0).count(), 1);
        assert_eq!(d.state().marks_on(p2).count(), 1);
    }

    #[test]
    fn a_group_runs_by_page_then_down_the_page_as_it_shows() {
        use crate::annot::{AnnotKind, Box2, Group};
        let mut d = doc();
        let (p0, p1) = (d.page(0).unwrap(), d.page(1).unwrap());
        let tick = |glyph: &str, y: f32| {
            mark(
                0,
                AnnotKind::Stamp {
                    rect: Box2 {
                        x: 10.0,
                        y,
                        w: 20.0,
                        h: 20.0,
                    },
                    glyph: glyph.into(),
                },
            )
        };
        for (page, glyph, y) in [
            (p1, "✓", 300.0),
            (p1, "✓", 100.0),
            (p0, "✓", 500.0),
            (p0, "✗", 50.0),
        ] {
            d.apply(
                Edit::AnnotAdd {
                    page,
                    annot: tick(glyph, y),
                },
                "t",
            )
            .unwrap();
        }
        let order = |d: &Document| -> Vec<(usize, f32)> {
            d.group_members(&Group::Tickmark("✓".into()))
                .iter()
                .map(|(i, _, a)| (*i, a.page_bounds().y))
                .collect()
        };
        // PageSpace is y up: the higher one is first.
        assert_eq!(order(&d), [(0, 500.0), (1, 300.0), (1, 100.0)]);
        assert_eq!(
            d.snapshot()
                .group_members(&Group::Tickmark("✓".into()))
                .len(),
            3
        );
        // Turned, the page shows them side by side, the lower one left.
        d.apply(
            Edit::RotatePages {
                ids: vec![p1],
                by: Rotation::R90,
            },
            "rotate",
        )
        .unwrap();
        assert_eq!(order(&d), [(0, 500.0), (1, 100.0), (1, 300.0)]);
    }

    fn mark(id: u64, kind: crate::annot::AnnotKind) -> Annot {
        Annot {
            id: AnnotId(id),
            kind,
            color: [255, 0, 0],
            opacity: 1.0,
            stroke: 1.0,
            author: "t".into(),
            contents: String::new(),
            modified: String::new(),
            created: String::new(),
            fill: None,
            dashed: false,
            cloudy: false,
            turn: Rotation::R0,
            name: String::new(),
        }
    }

    /// Text the built-in fonts can't draw (M10.5 proposal 22): a FreeText
    /// in Japanese draws a `?` for each character, its `/Contents` keeps
    /// the text exactly, and the reader is told once however many such
    /// edits come. A header/footer is read the same way.
    #[test]
    fn cjk_text_draws_question_marks_and_is_told_once() {
        use crate::annot::{AnnotKind, Box2};
        let mut d = doc();
        let page = id_at(&d, 0);
        let text = "日本語のメモ";
        let mut note = mark(
            0,
            AnnotKind::FreeText {
                rect: Box2 {
                    x: 10.0,
                    y: 10.0,
                    w: 200.0,
                    h: 40.0,
                },
                size: 12.0,
            },
        );
        note.contents = text.into();
        let add = |d: &mut Document, annot: Annot| {
            d.apply(Edit::AnnotAdd { page, annot }, "add").unwrap()
        };
        let first = add(&mut d, note.clone());
        assert!(first.unencodable);
        assert!(d.unencodable_notice(&first).is_some());
        let second = add(&mut d, note);
        assert!(second.unencodable);
        assert_eq!(d.unencodable_notice(&second), None, "told once");
        let plain = add(&mut d, square(0));
        assert!(!plain.unencodable);

        let written = &d.state().marks_on(page).next().unwrap();
        let mut pdf = lopdf::Document::with_version("1.7");
        let main = crate::annot::to_pdf(written, &mut pdf).main;
        let utf16: Vec<u8> = [0xFE, 0xFF]
            .into_iter()
            .chain(text.encode_utf16().flat_map(u16::to_be_bytes))
            .collect();
        assert_eq!(main.get(b"Contents").unwrap().as_str().unwrap(), utf16);
        let ap = main.get(b"AP").unwrap().as_dict().unwrap();
        let ap = ap.get(b"N").unwrap().as_reference().unwrap();
        let content = &pdf.get_object(ap).unwrap().as_stream().unwrap().content;
        let drawn = b"(??????) Tj";
        assert!(
            content.windows(drawn.len()).any(|w| w == drawn),
            "{}",
            String::from_utf8_lossy(content)
        );

        let mut hf = crate::stamps::HeaderFooter::default();
        hf.texts[1] = "<<file>>".into();
        hf.file = "報告.pdf".into();
        let set = |d: &mut Document, hf: &crate::stamps::HeaderFooter| {
            let header_footer = Some(hf.clone());
            d.apply(Edit::SetHeaderFooter { header_footer }, "hf")
                .unwrap()
        };
        assert!(set(&mut d, &hf).unencodable);
        hf.file = "report.pdf".into();
        hf.texts[2] = "Page <<1>> of <<N>> – <<date>>".into();
        assert!(!set(&mut d, &hf).unencodable);
    }

    pub(super) fn square(id: u64) -> Annot {
        use crate::annot::{AnnotKind, Box2};
        mark(
            id,
            AnnotKind::Square {
                rect: Box2 {
                    x: 1.0,
                    y: 1.0,
                    w: 5.0,
                    h: 5.0,
                },
            },
        )
    }

    pub(super) fn connector(id: u64, partner: u64) -> Annot {
        use crate::annot::{AnnotKind, Box2};
        mark(
            id,
            AnnotKind::Connector {
                rect: Box2 {
                    x: 1.0,
                    y: 1.0,
                    w: 22.0,
                    h: 22.0,
                },
                label: "1".into(),
                partner: Some(AnnotId(partner)),
            },
        )
    }

    #[test]
    fn bookmark_styles_come_from_the_file() {
        use lopdf::{Object, dictionary};
        let mut f = crate::forms::fixture::Form::new(3);
        let item = |title: &str, color: Option<f32>, flags: i64| {
            let mut d = dictionary! { "Title" => Object::string_literal(title), "F" => flags };
            if let Some(blue) = color {
                d.set("C", vec![0.into(), 0.into(), blue.into()]);
            }
            d
        };
        crate::forms::fixture::outline_tree(
            &mut f,
            vec![
                (0, item("A", Some(1.0), 2)),
                (1, item("A1", Some(1.0), 0)),
                (1, item("A2", None, 0)),
                (0, item("B", Some(1.0), 1)),
                (0, item("C", None, 0)),
            ],
        );
        let store = Store::new();
        let src = store
            .open(f.bytes().into(), BlobKind::file(Path::new("a.pdf"), None))
            .unwrap();
        let outline = crate::open::outline(&src);
        let primary = SourceInfo {
            src,
            name: "a.pdf".into(),
            derived: None,
            stamp: None,
        };
        let d = Document::new(store, primary, &outline, Box::new(FixedCount(5)));
        let by = |t: &str| bookmark(&d, t);
        assert!(by("A").style.bold);
        assert_eq!(by("A1").color, Some([0, 0, 255]));
        assert_eq!(by("A2").color, None);
        assert!(by("B").style.italic && by("B").color == Some([0, 0, 255]));
        assert!(by("C").color.is_none() && by("C").style.is_plain());
        // Read, not edited: nothing to save.
        assert!(!d.is_dirty());
    }

    /// The keys `d`'s current version put, in order.
    fn keys_put(d: &Document) -> Vec<Key> {
        let (steps, _) = d.edge(d.current_version()).unwrap();
        (steps.iter())
            .flat_map(|s| s.prims.iter().map(|p| p.key))
            .collect()
    }

    /// Editing a bookmark puts that bookmark alone: not its parent, the
    /// top level or the bookmarks beside it (M10.5 §3.2).
    #[test]
    fn a_bookmark_edit_puts_one_bookmark() {
        let mut d = doc();
        let (a1, p4) = (bm(&d, "A1"), id_at(&d, 4));
        let edit = Edit::OutlineEdit {
            id: a1,
            title: Some("First".into()),
            target: Some(Some(p4)),
            color: Some(Some([0, 0, 255])),
            view: None,
            style: None,
        };
        d.apply(edit, "Edit bookmark").unwrap();
        assert_eq!(keys_put(&d), [Key::Bookmark(a1)]);
        let b = bookmark(&d, "First");
        assert_eq!((b.page(), b.color), (Some(p4), Some([0, 0, 255])));
        assert_eq!(titles(&d)[1], (1, "First".into()));
    }

    /// An item going somewhere sheaf doesn't model, a link, opens kept
    /// from the item it was read as. A Rewrite after a reorder writes its
    /// `/A` again, and the `/SE`s of the file's items; so does one after a
    /// save, from the saved file's items (M10.5 bug 15).
    #[test]
    fn a_uri_bookmark_is_kept() {
        use crate::forms::fixture::{Form, outline_tree};
        use lopdf::{Object, dictionary};
        let mut f = Form::new(3);
        let pages = f.pages.clone();
        let link = f.pdf.add_object(dictionary! {
            "S" => "URI",
            "URI" => Object::string_literal("https://example.com/"),
        });
        let section = f
            .pdf
            .add_object(dictionary! { "Type" => "StructElem", "S" => "Sect" });
        let title = Object::string_literal;
        let site = dictionary! { "Title" => title("Site"), "A" => link, "SE" => section };
        let mut third = dictionary! { "Title" => title("Third"), "SE" => section };
        third.set("Dest", vec![pages[2].into(), "Fit".into()]);
        let items = outline_tree(&mut f, vec![(0, site), (0, third)]);
        let store = Store::new();
        let primary = file(&store, f.bytes(), "links.pdf");
        let read = crate::open::outline(&primary.src);
        let facts: Vec<_> = read.iter().map(|i| (i.object, i.kept, i.page)).collect();
        assert_eq!(
            facts,
            [
                (Some(items[0]), true, None),
                (Some(items[1]), false, Some(2))
            ]
        );
        let mut d = Document::new(store, primary, &read, Box::new(FixedCount(10)));
        let site = bookmark(&d, "Site");
        assert_eq!(site.target, Some(Target::Kept));
        assert_eq!(site.src.as_ref().map(|r| r.id), Some(items[0]));
        // What a file's items say: each one's page, the URI its `/A`
        // opens, and whether it has an `/SE`.
        let written = |bytes: Vec<u8>| -> Vec<(Option<u32>, Option<String>, bool)> {
            let src = crate::forms::fixture::source(bytes.clone());
            let pdf = lopdf::Document::load_mem(&bytes).unwrap();
            let uri = |id| {
                let item = pdf.get_dictionary(id).ok()?;
                let action = pdf.get_dictionary(item.get(b"A").ok()?.as_reference().ok()?);
                Some(crate::file::pdf_text(action.ok()?.get(b"URI").ok()?))
            };
            (crate::open::outline(&src).into_iter())
                .map(|i| {
                    let item = pdf.get_dictionary(i.object.unwrap()).unwrap();
                    (i.page, uri(i.object.unwrap()), item.has(b"SE"))
                })
                .collect()
        };
        let want = |third| {
            [
                (None, Some("https://example.com/".to_string()), true),
                (Some(third), None, true),
            ]
        };
        let (first, last) = (id_at(&d, 0), id_at(&d, 2));
        let to_front = Edit::MovePages {
            ids: vec![last],
            before: Some(first),
        };
        d.apply(to_front, "Move").unwrap();
        assert_eq!(
            written(crate::save::rewrite(&d.snapshot()).unwrap()),
            want(0)
        );
        // An incremental save writes the outline anew once it changed.
        let rename = Edit::OutlineEdit {
            id: bm(&d, "Third"),
            title: Some("3".into()),
            target: None,
            color: None,
            view: None,
            style: None,
        };
        d.apply(rename, "Rename").unwrap();
        let appended = crate::incremental::incremental(&d.snapshot()).unwrap();
        assert_eq!(written(appended), want(0));
        let saved = save_and_rebase(&mut d, false);
        let site = bookmark(&d, "Site");
        assert_eq!(site.target, Some(Target::Kept));
        assert_eq!(site.src.as_ref().map(|r| r.src.id()), Some(saved.id()));
        let back = Edit::MovePages {
            ids: vec![last],
            before: None,
        };
        d.apply(back, "Move").unwrap();
        assert_eq!(
            written(crate::save::rewrite(&d.snapshot()).unwrap()),
            want(2)
        );
    }

    /// Bookmarks nested deeper than a file is read are from no item after a
    /// save, and the ones after them in pre-order are still each from its
    /// own: a link bookmark after a deep branch keeps its `/A` through a
    /// second save.
    #[test]
    fn a_bookmark_after_a_deep_branch_keeps_its_own_item() {
        use crate::forms::fixture::{Form, outline_tree};
        use lopdf::{Object, dictionary};
        let mut f = Form::new(2);
        let link = f.pdf.add_object(dictionary! {
            "S" => "URI",
            "URI" => Object::string_literal("https://example.com/"),
        });
        let site = dictionary! { "Title" => Object::string_literal("Site"), "A" => link };
        outline_tree(&mut f, vec![(0, site)]);
        let store = Store::new();
        let primary = file(&store, f.bytes(), "deep.pdf");
        let read = crate::open::outline(&primary.src);
        let mut d = Document::new(store, primary, &read, Box::new(FixedCount(10)));
        let deepest = crate::open::MAX_DEPTH + 2;
        let items = (0..=deepest)
            .map(|depth| NewOutlineItem {
                title: format!("d{depth}"),
                target: Some(id_at(&d, 0)),
                view: None,
                depth,
            })
            .collect();
        let deep = Edit::OutlineInsertTree {
            parent: None,
            after: None,
            items,
        };
        d.apply(deep, "Deep").unwrap();
        // Where the saved file's "Site" sends a reader.
        let uri = |bytes: Vec<u8>| {
            let src = crate::forms::fixture::source(bytes.clone());
            let pdf = lopdf::Document::load_mem(&bytes).unwrap();
            let item = (crate::open::outline(&src).into_iter())
                .find(|i| i.title == "Site")
                .unwrap();
            let item = pdf.get_dictionary(item.object.unwrap()).unwrap();
            let action = item.get(b"A").ok()?.as_reference().ok()?;
            let action = pdf.get_dictionary(action).ok()?;
            Some(crate::file::pdf_text(action.get(b"URI").ok()?))
        };
        let saved = save_and_rebase(&mut d, false);
        for (depth, _, b) in d.state().bookmarks_preorder() {
            let src = b.src.as_ref().map(|r| r.src.id());
            match depth > crate::open::MAX_DEPTH {
                true => assert_eq!(src, None, "{}", b.title),
                false => assert_eq!(src, Some(saved.id()), "{}", b.title),
            }
        }
        let site = bookmark(&d, "Site");
        assert_eq!(site.target, Some(Target::Kept));
        let again = crate::save::rewrite(&d.snapshot()).unwrap();
        assert_eq!(uri(again).as_deref(), Some("https://example.com/"));
    }

    /// The header/footer sheaf drew opens as the generator that draws it,
    /// on the pages it's on. Set again, it's the same generator, its id
    /// kept, and a Rewrite draws it and writes its settings (M10.5 slice
    /// 8's legacy generator).
    #[test]
    fn the_header_footer_is_a_generator() {
        use crate::content::{PagOrigin, PaginationKind};
        let store = Store::new();
        let bytes = crate::forms::fixture::every_fact().bytes();
        let primary = file(&store, bytes, "facts.pdf");
        let src = primary.src.clone();
        let mut d = crate::open::start(store, primary, Box::new(FixedCount(10)));
        d.ready(crate::open::finish(&src, &in_progress::NoProgress))
            .unwrap();
        let pages: Vec<PageId> = d.state().order().iter().copied().collect();
        let only = |d: &Document| {
            let gens: Vec<Arc<Generator>> = d.state().generators().values().cloned().collect();
            assert_eq!(gens.len(), 1);
            (gens[0].id, gens[0].spec.header_footer().unwrap().clone())
        };
        let (id, hf) = only(&d);
        assert_eq!(
            (hf.texts[0].as_str(), &hf.pages),
            ("Draft", &Some(vec![pages[2]]))
        );
        assert!(!d.header_footer_dirty());
        for text in ["Final", "Final, again"] {
            let mut hf = crate::stamps::HeaderFooter::clone(&hf);
            hf.texts[0] = text.into();
            let set = Edit::SetHeaderFooter {
                header_footer: Some(hf),
            };
            d.apply(set, "Header & footer").unwrap();
            assert_eq!(keys_put(&d), [Key::Generator(id)]);
            assert_eq!(only(&d).0, id, "{text}");
        }
        assert!(d.header_footer_dirty());
        let saved = crate::forms::fixture::source(crate::save::rewrite(&d.snapshot()).unwrap());
        let (written, scope) = crate::stamps::from_catalog(saved.pdf()).unwrap();
        assert_eq!(
            (written.texts[0].as_str(), scope),
            ("Final, again", Some(vec![2]))
        );
        let drawn: Vec<Vec<(PaginationKind, PagOrigin)>> = (0..3)
            .map(|i| {
                let blocks = saved.blocks(i).unwrap();
                blocks
                    .pagination
                    .iter()
                    .map(|p| (p.kind, p.origin))
                    .collect()
            })
            .collect();
        let header = (PaginationKind::Header, PagOrigin::SheafLegacy);
        assert_eq!(drawn, [vec![], vec![], vec![header]]);
        let remove = Edit::SetHeaderFooter {
            header_footer: None,
        };
        d.apply(remove, "Remove header & footer").unwrap();
        assert!(d.state().generators().is_empty());
    }

    #[test]
    fn inserted_pages_bring_their_marks_with_fresh_ids() {
        let mut d = doc();
        let mine = d.apply(
            Edit::AnnotAdd {
                page: id_at(&d, 0),
                annot: square(0),
            },
            "add",
        );
        assert!(mine.is_ok());
        let sid = d.add_source(SourceInfo::blank(d.store(), 3));
        d.apply(
            Edit::InsertPages {
                source: sid,
                range: 0..2,
                before: None,
                outline: None,
                annots: vec![
                    // Page 0: a square and one end of a pair whose other end
                    // is on page 1; page 2 isn't inserted.
                    (0, vec![square(1), connector(2, 3)]),
                    (1, vec![connector(3, 2), connector(4, 9)]),
                    (2, vec![square(5)]),
                ],
            },
            "insert",
        )
        .unwrap();
        let (new0, new1) = (id_at(&d, 6), id_at(&d, 7));
        let on0 = d.state().marks_on(new0).cloned().collect::<Vec<_>>();
        let on1 = d.state().marks_on(new1).cloned().collect::<Vec<_>>();
        assert_eq!((on0.len(), on1.len()), (2, 2));
        let all: Vec<AnnotId> = on0.iter().chain(on1.iter()).map(|a| a.id).collect();
        // Fresh ids, none of the file's, all distinct.
        assert!(all.iter().all(|id| id.0 > 9), "{all:?}");
        let unique: HashSet<_> = all.iter().collect();
        assert_eq!(unique.len(), 4);
        // The pair still points at each other; a partner left behind is dropped.
        assert_eq!(on0[1].partner(), Some(on1[0].id));
        assert_eq!(on1[0].partner(), Some(on0[1].id));
        assert_eq!(on1[1].partner(), None);
        // Undo takes them away with the pages.
        d.undo().unwrap();
        assert!(d.state().marks_on(new0).next().is_none());
    }

    #[test]
    fn replacing_pages_drops_the_marks_of_pages_removed() {
        let mut d = doc();
        let (p1, p2) = (id_at(&d, 1), id_at(&d, 2));
        for p in [p1, p2] {
            d.apply(
                Edit::AnnotAdd {
                    page: p,
                    annot: square(0),
                },
                "add",
            )
            .unwrap();
        }
        let sid = d.add_source(SourceInfo::blank(d.store(), 1));
        d.apply(
            Edit::ReplacePages {
                ids: vec![p1, p2],
                source: sid,
                range: 0..1,
                annots: vec![(0, vec![square(7)])],
            },
            "replace",
        )
        .unwrap();
        // p1 keeps its mark and gains the replacement's; p2 is gone, and so
        // are its marks.
        assert_eq!(d.state().marks_on(p1).count(), 2);
        assert!(d.state().marks_on(p2).next().is_none());
    }

    #[test]
    fn mark_ids_are_random_and_distinct() {
        let mut d = doc();
        let p = id_at(&d, 0);
        for _ in 0..50 {
            d.apply(
                Edit::AnnotAdd {
                    page: p,
                    annot: square(0),
                },
                "add",
            )
            .unwrap();
        }
        let ids: Vec<u64> = d.state().marks_on(p).map(|a| a.id.0).collect();
        let unique: HashSet<_> = ids.iter().collect();
        assert_eq!(unique.len(), 50);
        assert!(ids.iter().all(|&id| id > 0 && id < 1 << 53));
        // Not handed out in order, as two copies of a file would.
        assert_ne!(ids, (ids[0]..ids[0] + 50).collect::<Vec<_>>());
    }

    /// `bytes` opened as a file opens, in a store of its own.
    fn reopened(bytes: Vec<u8>) -> Document {
        let store = Store::new();
        let info = file(&store, bytes, "saved.pdf");
        let src = info.src.clone();
        let mut d = crate::open::start(store, info, Box::new(FixedCount(10)));
        d.ready(crate::open::finish(&src, &in_progress::NoProgress))
            .unwrap();
        d
    }

    /// The page stamp mark `id` of `d` holds.
    fn stamp_held(d: &Document, id: AnnotId) -> Option<FormRef> {
        d.state().annot(id)?.mark_record()?.stamp.clone()
    }

    /// A page stamp's mark finds its page when it's added: in the store,
    /// placed and held; else in a registered file whose marks draw it, the
    /// primary's first; else in the generated cache, read into the store;
    /// else nowhere, and the edit fails, naming it. An update that names
    /// the same stamp keeps what it holds.
    #[test]
    fn a_stamp_resolves_from_the_store_a_source_or_the_cache() {
        use crate::forms::fixture::{self, MARK_STAMP};
        let stamp = fixture::every_mark_kind()[17].clone();
        let add = |d: &mut Document, annot: Annot| {
            let page = d.page(0).unwrap();
            let out = d.apply(Edit::AnnotAdd { page, annot }, "stamp");
            out.map(|_| d.state().marks_on(page).last().unwrap().id)
        };
        // Placed: the store's, held until it's added.
        let mut d = new_doc(1, &[], Box::new(FixedCount(10)));
        let held = fixture::mark_blobs(d.store());
        let id = add(&mut d, stamp.clone()).unwrap();
        drop(held);
        let placed = stamp_held(&d, id);
        assert!(matches!(&placed, Some(FormRef::Blob(b)) if b.id().stem() == *MARK_STAMP));
        let mut a = d.state().find_annot(id).unwrap().1.clone();
        a.contents = "edited".into();
        let page = d.page(0).unwrap();
        d.apply(Edit::AnnotUpdate { page, annot: a }, "edit")
            .unwrap();
        assert_eq!(stamp_held(&d, id), placed);
        let saved = crate::save::rewrite(&d.snapshot()).unwrap();

        // Nowhere: the edit fails, and says which.
        let mut other = new_doc(1, &[], Box::new(FixedCount(10)));
        let err = add(&mut other, stamp.clone()).unwrap_err();
        assert!(format!("{err:#}").contains("Approved"), "{err:#}");
        // A file whose pages came in holds it.
        let info = file(other.store(), saved.clone(), "stamped.pdf");
        let came = info.src.clone();
        insert_all(&mut other, info);
        let id = add(&mut other, stamp.clone()).unwrap();
        assert!(matches!(stamp_held(&other, id), Some(FormRef::Base(at)) if at.src == came));
        // The primary holds it.
        let mut opened = reopened(saved);
        let primary = opened.source(opened.primary()).unwrap().src.clone();
        let id = add(&mut opened, stamp.clone()).unwrap();
        assert!(matches!(stamp_held(&opened, id), Some(FormRef::Base(at)) if at.src == primary));

        // The generated cache has it: read into the store.
        let pdf = crate::generated::blank_pdf(fastrand::u16(300..10_000).into(), 50.0).unwrap();
        let path = crate::generated::store(&pdf).unwrap();
        let cached = Annot {
            kind: AnnotKind::PageStamp {
                rect: crate::annot::Box2 {
                    x: 10.0,
                    y: 10.0,
                    w: 100.0,
                    h: 50.0,
                },
                stamp: crate::generated::stem_of(&path),
                name: "Cached".into(),
                text: None,
            },
            ..stamp
        };
        let mut d = new_doc(1, &[], Box::new(FixedCount(10)));
        let id = add(&mut d, cached).unwrap();
        let Some(FormRef::Blob(b)) = stamp_held(&d, id) else {
            panic!("the cache's")
        };
        assert_eq!(**b.bytes(), *pdf);
        let origin = Origin::File(path.clone().into());
        assert!(matches!(b.blob().kind(), BlobKind::Pdf { origin: o, .. } if *o == origin));
        let _ = std::fs::remove_file(&path);
    }

    /// An attachment's mark finds its file when it's added: in the store,
    /// attached and held; else in a registered file whose marks embed it,
    /// decoded into the store. Never in the generated cache: one nothing
    /// holds isn't found, and the edit fails, naming it.
    #[test]
    fn an_attachment_is_a_blob_never_a_cache_file() {
        use crate::forms::fixture::{self, MARK_FILE, MARK_FILE_BYTES};
        let attach = fixture::every_mark_kind()[23].clone();
        let add = |d: &mut Document| {
            let page = d.page(0).unwrap();
            let out = d.apply(
                Edit::AnnotAdd {
                    page,
                    annot: attach.clone(),
                },
                "attach",
            );
            out.map(|_| d.state().marks_on(page).last().unwrap().id)
        };
        let file_held = |d: &Document, id: AnnotId| {
            let file = d.state().annot(id)?.mark_record()?.file.clone()?;
            assert!(matches!(file.blob().kind(), BlobKind::Attachment));
            Some(file)
        };
        // Attached: the store's, held until it's added, then the mark's.
        let mut d = new_doc(1, &[], Box::new(FixedCount(10)));
        let held = fixture::mark_blobs(d.store());
        let id = add(&mut d).unwrap();
        drop(held);
        let attached = file_held(&d, id).unwrap();
        assert_eq!(
            (&**attached.bytes(), attached.id().stem()),
            (MARK_FILE_BYTES, MARK_FILE.clone())
        );
        let saved = crate::save::rewrite(&d.snapshot()).unwrap();

        // Nowhere: the edit fails, and says which.
        let mut other = new_doc(1, &[], Box::new(FixedCount(10)));
        let err = add(&mut other).unwrap_err();
        assert!(format!("{err:#}").contains("notes.txt"), "{err:#}");
        // A file whose pages came in embeds it.
        let info = file(other.store(), saved, "attached.pdf");
        insert_all(&mut other, info);
        let id = add(&mut other).unwrap();
        assert_eq!(&**file_held(&other, id).unwrap().bytes(), MARK_FILE_BYTES);
    }

    /// What marks hold lives as long as they do (M10.5 slice 8): a file
    /// attached and a library stamp placed, every blob the caller held
    /// dropped, the bytes stay through undo and redo, and both saves write
    /// them, the file reopened reading the same. The store counts them
    /// while a version holds the marks.
    #[test]
    fn blob_liveness() {
        use crate::forms::fixture::{self, MARK_FILE_BYTES};
        let every = fixture::every_mark_kind();
        let mut d = new_doc(1, &[], Box::new(FixedCount(10)));
        let p0 = d.page(0).unwrap();
        let before = d.store().bytes();
        let [stamp, file] = fixture::mark_blobs(d.store());
        let held = stamp.len() + file.len();
        for annot in [every[17].clone(), every[23].clone()] {
            d.apply(Edit::AnnotAdd { page: p0, annot }, "add").unwrap();
        }
        drop((stamp, file));
        assert_eq!(d.store().bytes(), before + held);
        // The stamp's page draws its square, whichever PDF holds it.
        let square = |form: &FormRef| {
            let pdf = lopdf::Document::load_mem(&form.to_pdf().unwrap()).unwrap();
            let drawn = pdf.objects.values().filter_map(|o| o.as_stream().ok());
            let drawn: Vec<Vec<u8>> = drawn
                .map(|s| {
                    s.decompressed_content()
                        .unwrap_or_else(|_| s.content.clone())
                })
                .collect();
            let square = b"0 0 20 20 re";
            assert!(
                drawn
                    .iter()
                    .any(|c| c.windows(square.len()).any(|w| w == square))
            );
        };
        let check = |d: &Document, page: PageId| {
            let marks: Vec<&Mark> = d.state().mark_records_on(page).collect();
            assert_eq!(marks.len(), 2);
            square(marks[0].stamp.as_ref().unwrap());
            assert_eq!(&**marks[1].file.as_ref().unwrap().bytes(), MARK_FILE_BYTES);
        };
        check(&d, p0);
        d.undo().unwrap();
        d.undo().unwrap();
        assert!(d.state().mark_records_on(p0).next().is_none());
        assert_eq!(
            d.store().bytes(),
            before + held,
            "the undone versions hold them"
        );
        d.redo().unwrap();
        d.redo().unwrap();
        check(&d, p0);
        let snap = d.snapshot();
        for saved in [
            crate::save::rewrite(&snap).unwrap(),
            crate::incremental::incremental(&snap).unwrap(),
        ] {
            let back = reopened(saved);
            check(&back, back.page(0).unwrap());
        }
    }

    #[test]
    fn annot_add_update_delete_and_cascade() {
        use crate::annot::{AnnotKind, Box2};
        let mut d = doc();
        let p1 = id_at(&d, 1);
        let a = Annot {
            id: AnnotId(0),
            kind: AnnotKind::Square {
                rect: Box2 {
                    x: 10.0,
                    y: 10.0,
                    w: 50.0,
                    h: 20.0,
                },
            },
            color: [255, 0, 0],
            opacity: 1.0,
            stroke: 2.0,
            author: "t".into(),
            contents: String::new(),
            modified: String::new(),
            created: String::new(),
            fill: None,
            dashed: false,
            cloudy: false,
            turn: Rotation::R0,
            name: String::new(),
        };
        d.apply(
            Edit::AnnotAdd {
                page: p1,
                annot: a.clone(),
            },
            "add",
        )
        .unwrap();
        let stored = d.state().marks_on(p1).next().unwrap().clone();
        assert_ne!(stored.id, AnnotId(0), "id assigned by the document");
        let mut upd = stored.clone();
        upd.color = [0, 0, 255];
        d.apply(
            Edit::AnnotUpdate {
                page: p1,
                annot: upd,
            },
            "upd",
        )
        .unwrap();
        assert_eq!(d.state().marks_on(p1).next().unwrap().color, [0, 0, 255]);
        assert_eq!(d.snapshot().annots_dirty.len(), 1);
        d.apply(Edit::DuplicatePages { ids: vec![p1] }, "dup")
            .unwrap();
        let p1_copy = id_at(&d, 2);
        assert_ne!(d.state().marks_on(p1_copy).next().unwrap().id, stored.id);
        d.apply(Edit::DeletePages { ids: vec![p1_copy] }, "del")
            .unwrap();
        assert!(d.state().marks_on(p1_copy).next().is_none());
        d.apply(
            Edit::AnnotDelete {
                page: p1,
                id: stored.id,
            },
            "rm",
        )
        .unwrap();
        assert!(d.state().marks_on(p1).next().is_none());
        d.undo().unwrap();
        assert_eq!(d.state().marks_on(p1).count(), 1);
    }

    #[test]
    fn a_batch_that_puts_things_back_pushes_nothing() {
        let mut d = doc();
        let n = d.version_count();
        let (p0, p1) = (id_at(&d, 0), id_at(&d, 1));
        let rotate = |by| Edit::RotatePages { ids: vec![p0], by };
        d.apply_batch(vec![rotate(Rotation::R90), rotate(Rotation::R270)], "spin")
            .unwrap();
        let to = |before| Edit::MovePages {
            ids: vec![p0],
            before,
        };
        d.apply_batch(vec![to(None), to(Some(p1))], "there and back")
            .unwrap();
        d.apply_batch(
            vec![
                Edit::SetHeaderFooter {
                    header_footer: Some(header("Draft".into())),
                },
                Edit::SetHeaderFooter {
                    header_footer: None,
                },
            ],
            "header",
        )
        .unwrap();
        let roman = crate::labels::LabelSpec {
            start: p0,
            style: crate::labels::Style::LowerRoman,
            prefix: String::new(),
            first: 1,
        };
        d.apply_batch(
            vec![
                Edit::SetPageLabels {
                    ranges: vec![roman],
                },
                Edit::SetPageLabels { ranges: Vec::new() },
            ],
            "labels",
        )
        .unwrap();
        assert!(!d.can_undo());
        assert_eq!(d.version_count(), n);
    }

    #[test]
    fn a_batch_keeps_each_slot_s_change_on_its_last_step() {
        let mut d = doc();
        let page = id_at(&d, 0);
        let before = d.state().get(&Key::Geom(page));
        let rotate = Edit::RotatePages {
            ids: vec![page],
            by: Rotation::R90,
        };
        d.apply_batch(vec![rotate.clone(), rotate], "rotate twice")
            .unwrap();
        let steps = &d.versions[&d.cursor].steps;
        assert_eq!(steps.len(), 2, "each edit stays");
        assert!(steps[0].prims.is_empty());
        let [p] = &steps[1].prims[..] else {
            panic!("one prim: {:?}", steps[1].prims)
        };
        assert_eq!(p.key, Key::Geom(page));
        assert!(same(p.before.as_ref(), before.as_ref()));
        assert!(same(p.after.as_ref(), d.state().get(&p.key).as_ref()));
        assert_eq!(rotation(&d, 0), Rotation::R180);
    }

    #[test]
    fn a_batch_describes_each_edit_against_the_document_before_it() {
        let mut d = doc();
        // Pages 3 and 4 go first, which makes page 2 the 4th.
        let (first, second) = (id_at(&d, 0), id_at(&d, 1));
        d.apply_batch(
            vec![
                Edit::MovePages {
                    ids: vec![id_at(&d, 2), id_at(&d, 3)],
                    before: Some(first),
                },
                Edit::DeletePages { ids: vec![second] },
            ],
            "move and delete",
        )
        .unwrap();
        let last = d.timeline().pop().unwrap();
        assert_eq!(last.edits, ["Move pages 3, 4", "Delete page 4"]);
    }

    // ----- OCR text layers ---------------------------------------------------

    pub(super) fn tl(text: &str) -> Arc<TextLayer> {
        Arc::new(TextLayer::from_engine(
            vec![crate::text_layer::OcrWord {
                text: text.into(),
                rect: crate::annot::Box2 {
                    x: 10.0,
                    y: 10.0,
                    w: 50.0,
                    h: 10.0,
                },
                confidence: 1.0,
            }],
            &crate::geom::Geom::read(None, None, 0),
            "test",
        ))
    }

    fn set(d: &mut Document, pages: &[usize], text: &str) {
        let layers = pages.iter().map(|&i| (id_at(d, i), tl(text))).collect();
        d.apply(Edit::SetTextLayers { layers }, "ocr").unwrap();
    }

    /// Each page's recognized text, by position.
    fn layer_texts(d: &Document) -> Vec<(usize, String)> {
        (d.state().pages().enumerate())
            .filter_map(|(i, (_, def, _))| Some((i, def.content.ocr_text()?.words[0].text.clone())))
            .collect()
    }

    #[test]
    fn set_text_layers_undo_redo() {
        let mut d = doc();
        set(&mut d, &[1, 3], "a");
        assert_eq!(layer_texts(&d), vec![(1, "a".into()), (3, "a".into())]);
        assert!(d.state().ocr_text(id_at(&d, 1)).is_some());
        assert!(d.is_dirty());
        d.undo().unwrap();
        assert!(layer_texts(&d).is_empty());
        d.redo().unwrap();
        assert_eq!(layer_texts(&d).len(), 2);
        // Replacing a page's layer is a new version.
        set(&mut d, &[1], "b");
        assert_eq!(layer_texts(&d), vec![(1, "b".into()), (3, "a".into())]);
        assert_eq!(d.undo_label(), Some("ocr"));
    }

    #[test]
    fn clear_text_layers() {
        let mut d = doc();
        set(&mut d, &[0, 1], "a");
        d.apply(
            Edit::ClearTextLayers {
                ids: vec![id_at(&d, 0), id_at(&d, 5)],
            },
            "clear",
        )
        .unwrap();
        assert_eq!(layer_texts(&d), vec![(1, "a".into())]);
        // Clearing nothing is a no-op version-wise.
        let n = d.stats().versions;
        d.apply(
            Edit::ClearTextLayers {
                ids: vec![id_at(&d, 5)],
            },
            "clear",
        )
        .unwrap();
        assert_eq!(d.stats().versions, n);
    }

    #[test]
    fn unknown_page_is_an_error() {
        let mut d = doc();
        assert!(
            d.apply(
                Edit::SetTextLayers {
                    layers: vec![(PageId(999), tl("x"))]
                },
                "ocr"
            )
            .is_err()
        );
    }

    #[test]
    fn layers_follow_delete_duplicate_replace() {
        let mut d = doc();
        set(&mut d, &[1, 2, 3], "a");
        d.apply(
            Edit::DeletePages {
                ids: vec![id_at(&d, 1)],
            },
            "del",
        )
        .unwrap();
        // Former pages 2 and 3 are now 1 and 2.
        assert_eq!(layer_texts(&d), vec![(1, "a".into()), (2, "a".into())]);
        let dup = id_at(&d, 1);
        d.apply(Edit::DuplicatePages { ids: vec![dup] }, "dup")
            .unwrap();
        assert_eq!(
            layer_texts(&d),
            vec![(1, "a".into()), (2, "a".into()), (3, "a".into())]
        );
        let copy = id_at(&d, 2);
        assert!(Arc::ptr_eq(
            d.state().ocr_text(dup).unwrap(),
            d.state().ocr_text(copy).unwrap()
        ));
        // Replacing a page's content makes its OCR stale.
        let other = d.add_source(SourceInfo::blank(d.store(), 2));
        d.apply(
            Edit::ReplacePages {
                ids: vec![copy],
                source: other,
                range: 0..1,
                annots: Vec::new(),
            },
            "replace",
        )
        .unwrap();
        assert_eq!(layer_texts(&d), vec![(1, "a".into()), (3, "a".into())]);
    }

    #[test]
    fn job_batches_coalesce() {
        let mut d = doc();
        let before = d.stats().versions;
        let job = d.begin_job();
        for (i, page) in [0usize, 1, 2].iter().enumerate() {
            d.apply_in_job(
                job,
                Edit::SetTextLayers {
                    layers: vec![(id_at(&d, *page), tl("a"))],
                },
                format!("OCR batch {i}"),
            )
            .unwrap();
            assert!(d.job_is_current(job));
        }
        assert_eq!(d.stats().versions, before + 1);
        assert_eq!(layer_texts(&d).len(), 3);
        assert_eq!(d.undo_label(), Some("OCR batch 2"));
        d.undo().unwrap();
        assert!(layer_texts(&d).is_empty());
        assert!(!d.job_is_current(job));
        assert!(d.job_is_undone(job));
        d.redo().unwrap();
        assert_eq!(layer_texts(&d).len(), 3);
        assert!(!d.job_is_undone(job));
    }

    #[test]
    fn job_never_amends_saved_version() {
        let mut d = doc();
        let job = d.begin_job();
        d.apply_in_job(
            job,
            Edit::SetTextLayers {
                layers: vec![(id_at(&d, 0), tl("a"))],
            },
            "ocr",
        )
        .unwrap();
        let n = d.stats().versions;
        d.mark_saved();
        d.apply_in_job(
            job,
            Edit::SetTextLayers {
                layers: vec![(id_at(&d, 1), tl("a"))],
            },
            "ocr",
        )
        .unwrap();
        assert_eq!(d.stats().versions, n + 1, "pushed, not amended");
        assert!(d.is_dirty());
        d.undo().unwrap();
        assert!(!d.is_dirty(), "the saved version is intact");
        assert_eq!(layer_texts(&d).len(), 1);
    }

    #[test]
    fn job_breaks_after_foreign_edit() {
        let mut d = doc();
        let job = d.begin_job();
        d.apply_in_job(
            job,
            Edit::SetTextLayers {
                layers: vec![(id_at(&d, 0), tl("a"))],
            },
            "ocr",
        )
        .unwrap();
        d.apply(
            Edit::RotatePages {
                ids: vec![id_at(&d, 5)],
                by: Rotation::R90,
            },
            "rotate",
        )
        .unwrap();
        assert!(!d.job_is_current(job));
        assert!(!d.job_is_undone(job), "a later edit is not an undo");
        let n = d.stats().versions;
        d.apply_in_job(
            job,
            Edit::SetTextLayers {
                layers: vec![(id_at(&d, 1), tl("a"))],
            },
            "ocr",
        )
        .unwrap();
        assert_eq!(d.stats().versions, n + 1);
        assert!(d.job_is_current(job));
        // Undo peels the second batch, then the rotation, then the first.
        d.undo().unwrap();
        assert_eq!(layer_texts(&d).len(), 1);
        assert_eq!(rotation(&d, 5), Rotation::R90);
    }

    #[test]
    fn job_first_batch_noop_pushes_nothing() {
        let mut d = doc();
        let job = d.begin_job();
        let n = d.stats().versions;
        d.apply_in_job(
            job,
            Edit::ClearTextLayers {
                ids: vec![id_at(&d, 0)],
            },
            "ocr",
        )
        .unwrap();
        assert_eq!(d.stats().versions, n);
        assert!(!d.job_is_current(job));
    }

    /// A version names one state, so a job's later batch makes another
    /// version in place of the one it amends.
    #[test]
    fn an_ocr_amend_gives_the_cursor_a_new_version() {
        let mut d = doc();
        let parent = d.current_version();
        let job = d.begin_job();
        let ocr = |d: &mut Document, page: usize| {
            d.apply_in_job(
                job,
                Edit::SetTextLayers {
                    layers: vec![(id_at(d, page), tl("a"))],
                },
                "ocr",
            )
            .unwrap();
        };
        ocr(&mut d, 0);
        let v1 = d.current_version();
        ocr(&mut d, 1);
        let v2 = d.current_version();
        assert_ne!(v2, v1);
        assert!(d.jump(v1).is_err(), "the amended version is gone");
        assert!(d.edge(v1).is_none());
        let ids: Vec<VersionId> = d.timeline().iter().map(|view| view.id).collect();
        assert_eq!(ids, [parent, v2]);
        assert!(d.job_is_current(job));
        assert_eq!(d.undo().unwrap(), Some("ocr"));
        assert_eq!(d.current_version(), parent);
        assert_eq!(d.redo().unwrap(), Some("ocr"));
        assert_eq!(d.current_version(), v2);
        assert_eq!(layer_texts(&d).len(), 2);
    }

    /// A save expresses the version anew over the saved file, so it's
    /// another version, in the same place. The opened one too.
    #[test]
    fn a_save_gives_the_cursor_a_new_version() {
        let mut d = doc();
        let opened = d.current_version();
        d.rebase_to_saved(SourceInfo::blank(d.store(), 6));
        let root = d.current_version();
        assert_ne!(root, opened);
        assert!(d.jump(opened).is_err());
        rotate(&mut d, 0, "rotate");
        let old = d.current_version();
        d.rebase_to_saved(SourceInfo::blank(d.store(), 6));
        let new = d.current_version();
        assert_ne!(new, old);
        assert!(d.jump(old).is_err(), "the old id doesn't resolve");
        assert!(!d.is_dirty());
        let saved: Vec<VersionId> = d
            .timeline()
            .iter()
            .filter(|view| view.saved)
            .map(|view| view.id)
            .collect();
        assert_eq!(saved, [new]);
        assert_eq!(d.undo().unwrap(), Some("rotate"));
        assert_eq!(d.current_version(), root);
        assert!(d.is_dirty());
        assert_eq!(d.redo().unwrap(), Some("rotate"));
        assert_eq!(d.current_version(), new);
        assert!(!d.is_dirty());
    }

    /// A job whose batches put back all they did leaves no version: the
    /// parent is current again, as if the job never ran, even when its
    /// snapshot was dropped. A file registered before it waits for the
    /// next version again.
    #[test]
    fn a_job_that_nets_to_nothing_leaves_no_version() {
        let mut d = new_doc(3, &[], Box::new(FixedCount(1)));
        rotate(&mut d, 2, "rotate");
        let (parent, want) = (d.current_version(), d.state().clone());
        let n = d.version_count();
        let job = d.begin_job();
        let p0 = id_at(&d, 0);
        let s = d.add_source(SourceInfo::blank(d.store(), 1));
        d.apply_in_job(
            job,
            Edit::SetTextLayers {
                layers: vec![(p0, tl("a"))],
            },
            "ocr",
        )
        .unwrap();
        let v = d.current_version();
        assert_eq!(d.edge(v).unwrap().1, [s]);
        assert!(d.versions[&parent].state.is_none(), "trimmed");
        d.apply_in_job(job, Edit::ClearTextLayers { ids: vec![p0] }, "ocr")
            .unwrap();
        assert_eq!(d.current_version(), parent);
        assert_eq!(d.version_count(), n);
        assert!(d.jump(v).is_err());
        assert!(!d.can_redo());
        assert!(!d.job_is_current(job));
        assert_eq!(*d.state(), want);
        rotate(&mut d, 0, "rotate");
        assert_eq!(d.edge(d.current_version()).unwrap().1, [s]);
    }

    /// An amend takes the sources added since the version it joins, and a
    /// batch that changes nothing leaves that version as it is.
    #[test]
    fn a_job_s_amend_takes_its_sources_and_its_no_op_changes_nothing() {
        let mut d = doc();
        let job = d.begin_job();
        let ocr = |d: &mut Document, page: usize, label: &str| {
            d.apply_in_job(
                job,
                Edit::SetTextLayers {
                    layers: vec![(id_at(d, page), tl("a"))],
                },
                label,
            )
            .unwrap();
        };
        ocr(&mut d, 0, "ocr");
        let s = d.add_source(SourceInfo::blank(d.store(), 1));
        ocr(&mut d, 1, "ocr");
        let v = d.current_version();
        let (steps, sources) = d.edge(v).unwrap();
        assert_eq!((steps.len(), sources), (2, &[s][..]));
        d.apply_in_job(job, Edit::SetTextLayers { layers: vec![] }, "other")
            .unwrap();
        assert_eq!(d.current_version(), v);
        assert_eq!(d.undo_label(), Some("ocr"));
        assert_eq!(d.edge(v).unwrap().0.len(), 2, "no step added");
    }

    /// Pruning past `MAX_VERSIONS` can make a job's version the first one,
    /// with no parent to amend it under: the job's next batch makes a
    /// version after it.
    #[test]
    fn a_job_s_version_pruned_to_the_first_is_not_amended() {
        let mut d = new_doc(3, &[], Box::new(FixedCount(1)));
        for i in 1..crate::history::MAX_VERSIONS {
            rotate(&mut d, i % 3, "rotate");
        }
        d.rebase_to_saved(SourceInfo::blank(d.store(), 3));
        let opened = d.timeline()[0].id;
        d.jump(opened).unwrap();
        let job = d.begin_job();
        let ocr = |d: &mut Document, page: usize| {
            d.apply_in_job(
                job,
                Edit::SetTextLayers {
                    layers: vec![(id_at(d, page), tl("a"))],
                },
                "ocr",
            )
            .unwrap();
        };
        // One version too many: the saved branch goes, and the job's
        // version becomes the first.
        ocr(&mut d, 0);
        let v = d.current_version();
        assert_eq!(d.version_count(), 1);
        assert!(!d.can_undo());
        ocr(&mut d, 1);
        assert_ne!(d.current_version(), v);
        assert_eq!(d.undo().unwrap(), Some("ocr"));
        assert_eq!(d.current_version(), v);
        assert_eq!(layer_texts(&d).len(), 1);
    }

    #[test]
    fn rebase_clears_text_layers() {
        let mut d = doc();
        set(&mut d, &[0], "a");
        d.rebase_to_saved(SourceInfo::blank(d.store(), 6));
        assert!(layer_texts(&d).is_empty());
        assert!(!d.is_dirty());
        // The version before the save still carries its layer, so undoing
        // across the save re-emits it against the old source.
        d.undo().unwrap();
        assert!(layer_texts(&d).is_empty(), "pre-OCR version");
        d.redo().unwrap();
        assert!(layer_texts(&d).is_empty(), "rebased version");
    }

    #[test]
    fn history_counts_the_layer_a_step_holds() {
        let mut d = doc();
        let layer = tl(&"x".repeat(100_000));
        let p0 = id_at(&d, 0);
        d.apply(
            Edit::SetTextLayers {
                layers: vec![(p0, layer.clone())],
            },
            "ocr",
        )
        .unwrap();
        let stats = d.stats();
        assert!(stats.step_bytes >= layer.byte_estimate());
        assert!(
            stats.step_bytes < 2 * layer.byte_estimate(),
            "counted once, though the edit holds it too"
        );
        // The snapshot shares the layer: it adds only the map's nodes.
        assert!(*stats.snapshot_bytes.last().unwrap() < 4 << 10);
    }

    #[test]
    fn an_added_mark_costs_its_page_not_the_document() {
        let mark = |id| FileAnnot {
            object: None,
            slot: 0,
            body: FileBody::Mark {
                annot: Box::new(square(id)),
                held: None,
            },
        };
        let mut annots = FileAnnots::default();
        annots.pages = (0..2000u32)
            .map(|i| (i, (1..=10).map(|j| mark(u64::from(i) * 10 + j)).collect()))
            .collect();
        let facts = Facts {
            annots: Arc::new(annots),
            ..Default::default()
        };
        let mut d = opened_doc(2000, facts, Box::new(FixedCount(10)));
        let page = id_at(&d, 1000);
        d.apply(
            Edit::AnnotAdd {
                page,
                annot: square(0),
            },
            "add",
        )
        .unwrap();
        // 256 bytes for each of the document's 20,000 marks was 5 MB.
        let stats = d.stats();
        assert!(stats.step_bytes < 4 << 10, "{}", stats.step_bytes);
        let snapshot = *stats.snapshot_bytes.last().unwrap();
        assert!(snapshot < 16 << 10, "{snapshot}");
    }

    /// Renaming one bookmark of 2,000 costs it, not the outline: 375,627
    /// bytes of steps while the outline was one value.
    #[test]
    fn renaming_a_bookmark_costs_it_not_the_outline() {
        let titles: Vec<String> = (0..2000).map(|i| format!("b{i}")).collect();
        let items: Vec<(&str, Option<u32>, u16)> = (titles.iter().enumerate())
            .map(|(i, t)| (t.as_str(), Some(i as u32 % 10), u16::from(i % 10 != 0)))
            .collect();
        let mut d = new_doc(10, &flat(&items), Box::new(FixedCount(10)));
        let id = d.state().bookmarks_preorder()[1001].1;
        let rename = Edit::OutlineEdit {
            id,
            title: Some("Renamed".into()),
            target: None,
            color: None,
            view: None,
            style: None,
        };
        d.apply(rename, "Rename").unwrap();
        let stats = d.stats();
        assert!(stats.step_bytes < 2 << 10, "{}", stats.step_bytes);
        let snapshot = *stats.snapshot_bytes.last().unwrap();
        assert!(snapshot < 4 << 10, "{snapshot}");
    }

    /// `bytes` as the file `name`, parsed into `store`.
    fn file(store: &Store, bytes: Vec<u8>, name: &str) -> SourceInfo {
        let kind = BlobKind::file(Path::new(name), None);
        SourceInfo {
            src: store.open(bytes.into(), kind).unwrap(),
            name: name.into(),
            derived: None,
            stamp: None,
        }
    }

    /// All of `info`'s pages inserted at the end, as a new version.
    fn insert_all(d: &mut Document, info: SourceInfo) -> SourceId {
        let count = info.src.page_count();
        let source = d.add_source(info);
        d.apply(
            Edit::InsertPages {
                source,
                range: 0..count,
                before: None,
                outline: None,
                annots: Vec::new(),
            },
            "insert",
        )
        .unwrap();
        source
    }

    #[test]
    fn an_undone_insert_counts_its_source_until_redo() {
        let mut d = doc();
        let primary = d.store().bytes();
        let blank = |d: &Document| SourceInfo::blank(d.store(), 2);
        let info = blank(&d);
        let sid = insert_all(&mut d, info);
        let len = d.source(sid).unwrap().src.bytes().len();
        let stats = d.stats();
        assert_eq!(stats.undone_source_bytes, 0);
        assert_eq!(stats.blob_bytes, d.store().bytes());
        assert_eq!(stats.blob_bytes, primary + len);
        d.undo().unwrap();
        let stats = d.stats();
        assert_eq!(stats.undone_source_bytes, len);
        assert_eq!(stats.blob_bytes, primary + len, "undo frees nothing");
        assert_eq!(
            stats.total_bytes(),
            stats.snapshot_bytes.iter().sum::<usize>() + stats.step_bytes + len
        );
        d.redo().unwrap();
        assert_eq!(d.stats().undone_source_bytes, 0);
        // Undone again: the same bytes registered for the next version,
        // then inserted on the line, share its blob.
        d.undo().unwrap();
        let waiting = d.add_source(blank(&d));
        assert_eq!(d.stats().undone_source_bytes, 0);
        d.apply(
            Edit::InsertPages {
                source: waiting,
                range: 0..2,
                before: None,
                outline: None,
                annots: Vec::new(),
            },
            "insert",
        )
        .unwrap();
        assert_ne!(waiting, sid);
        let stats = d.stats();
        assert_eq!(stats.undone_source_bytes, 0);
        assert_eq!(stats.blob_bytes, primary + len, "one blob");
    }

    #[test]
    fn pruning_an_undone_400_page_scan_frees_its_bytes() {
        use sheaf_image::io::{PageImage, Pixels};
        let mut d = new_doc(3, &[], Box::new(FixedCount(1)));
        let rasters: Vec<PageImage> = (0..400u32)
            .map(|i| PageImage {
                width: 8,
                height: 8,
                dpi: (72.0, 72.0),
                rotate: 0,
                pixels: Pixels::Gray((0..64).map(|j| (i * 7 + j) as u8).collect()),
            })
            .collect();
        let scan = crate::images::to_pdf(&rasters, crate::images::PageFit::LETTER).unwrap();
        let (len, before) = (scan.len(), d.store().bytes());
        let info = file(d.store(), scan, "scan.pdf");
        let sid = insert_all(&mut d, info);
        let id = d.source(sid).unwrap().src.id();
        assert_eq!((d.page_count(), d.store().bytes()), (403, before + len));
        d.undo().unwrap();
        assert_eq!(d.stats().undone_source_bytes, len);
        // One edit branches off; past `MAX_VERSIONS`, the branch goes.
        for i in 0..crate::history::MAX_VERSIONS {
            rotate(&mut d, i % 3, "rotate");
        }
        assert!(d.source(sid).is_none(), "its registration went with it");
        assert_eq!(d.store().bytes(), before);
        assert!(d.store().get(id).is_none());
        let stats = d.stats();
        assert_eq!((stats.undone_source_bytes, stats.blob_bytes), (0, before));
    }

    #[test]
    fn the_root_advancing_keeps_its_sources() {
        let mut d = new_doc(3, &[], Box::new(FixedCount(1)));
        // Unsaved, as a recovered document is: the first version can move.
        d.mark_unsaved();
        let info = SourceInfo::blank(d.store(), 1);
        let gone = insert_all(&mut d, info);
        d.undo().unwrap();
        let info = SourceInfo::blank(d.store(), 2);
        let kept = insert_all(&mut d, info);
        for i in 0..crate::history::MAX_VERSIONS {
            rotate(&mut d, i % 3, "rotate");
        }
        let first = &d.versions[&d.root];
        assert!(!first.steps.is_empty(), "pruned to a later one");
        assert_eq!(first.sources, [d.primary(), kept]);
        assert!(d.source(gone).is_none(), "its branch was pruned");
        assert_eq!(d.page_count(), 5);
        let shown = d.state().pages().map(|(_, def, _)| def.base.source);
        assert!(
            shown
                .collect::<Vec<_>>()
                .iter()
                .all(|&s| d.source(s).is_some())
        );
        let bytes = |sid| d.source(sid).unwrap().src.bytes().len();
        assert_eq!(d.store().bytes(), bytes(d.primary()) + bytes(kept));
    }

    /// A save's file stays while it's the primary, though pruning took the
    /// version that held it, and goes when the next save replaces it.
    #[test]
    fn a_pruned_save_s_file_goes_when_the_next_save_replaces_it() {
        let mut d = new_doc(3, &[], Box::new(FixedCount(1)));
        for i in 1..crate::history::MAX_VERSIONS {
            rotate(&mut d, i % 3, "rotate");
        }
        let a4 = crate::generated::blank_pages(595.0, 842.0, 3).unwrap();
        let saved = d.rebase_to_saved(file(d.store(), a4, "saved.pdf"));
        let id = d.source(saved).unwrap().src.id();
        // Back to the first version, and one edit too many: the saved
        // branch goes.
        let opened = d.timeline()[0].id;
        d.jump(opened).unwrap();
        rotate(&mut d, 0, "rotate");
        assert_eq!(d.version_count(), 1);
        assert_eq!(d.primary(), saved);
        assert!(d.store().get(id).is_some());
        d.rebase_to_saved(SourceInfo::blank(d.store(), 3));
        assert!(d.source(saved).is_none());
        assert!(d.store().get(id).is_none());
    }

    #[test]
    fn apply_batch_is_one_undo_step() {
        let mut d = doc();
        let before = d.state().clone();
        let p0 = id_at(&d, 0);
        let p3 = id_at(&d, 3);
        d.apply_batch(
            vec![
                Edit::OutlineInsert {
                    parent: None,
                    after: None,
                    title: "W-2".into(),
                    target: Some(p0),
                    view: None,
                },
                Edit::OutlineInsert {
                    parent: None,
                    after: None,
                    title: "1099-INT".into(),
                    target: Some(p3),
                    view: None,
                },
                Edit::RotatePages {
                    ids: vec![p3],
                    by: Rotation::R90,
                },
            ],
            "Plugin edits",
        )
        .unwrap();
        assert_eq!(
            d.state().outline_roots().len(),
            before.outline_roots().len() + 2
        );
        assert_eq!(d.undo_label(), Some("Plugin edits"));
        d.undo().unwrap();
        assert_eq!(*d.state(), before);
        assert!(!d.can_undo());
    }

    #[test]
    fn apply_batch_failure_applies_nothing() {
        let mut d = doc();
        let before = d.state().clone();
        let p0 = id_at(&d, 0);
        let err = d.apply_batch(
            vec![
                Edit::DeletePages { ids: vec![p0] },
                Edit::MovePages {
                    ids: vec![id_at(&d, 1)],
                    before: Some(PageId(9999)),
                },
            ],
            "bad",
        );
        assert!(err.is_err());
        assert_eq!(*d.state(), before);
        assert!(!d.can_undo());
    }

    #[test]
    fn apply_batch_sums_outcomes() {
        let mut d = doc();
        let ids = vec![id_at(&d, 0)];
        let out = d
            .apply_batch(
                vec![
                    Edit::DuplicatePages { ids: ids.clone() },
                    Edit::DuplicatePages { ids },
                ],
                "dup twice",
            )
            .unwrap();
        assert_eq!(out.pages_added, 2);
        assert_eq!(d.page_count(), 8);
    }

    #[test]
    fn insert_tree_nests_in_one_step() {
        let mut d = doc();
        let (p0, p3) = (id_at(&d, 0), id_at(&d, 3));
        let first_top = d.state().outline_roots()[0];
        let item = |title: &str, target, depth| NewOutlineItem {
            title: title.into(),
            target,
            view: None,
            depth,
        };
        d.apply(
            Edit::OutlineInsertTree {
                parent: None,
                after: Some(first_top),
                items: vec![
                    item("Accounts", None, 0),
                    item("…4821", Some(p0), 1),
                    item("…9930", Some(p3), 1),
                    item("Loose", Some(p3), 0),
                ],
            },
            "Bookmark matches",
        )
        .unwrap();
        let titles: Vec<(u16, String)> = titles(&d)
            .into_iter()
            .filter(|(_, t)| {
                ["Accounts", "…4821", "…9930", "Loose", "A", "B"].contains(&t.as_str())
            })
            .collect();
        assert_eq!(
            titles,
            [
                (0, "A".to_string()),
                (0, "Accounts".to_string()),
                (1, "…4821".to_string()),
                (1, "…9930".to_string()),
                (0, "Loose".to_string()),
                (0, "B".to_string()),
            ]
        );
        assert_eq!(d.undo_label(), Some("Bookmark matches"));
        d.undo().unwrap();
        assert_eq!(d.state().outline_roots().len(), 3);
        // Depths must nest properly.
        let bad = d.apply(
            Edit::OutlineInsertTree {
                parent: None,
                after: None,
                items: vec![item("x", None, 0), item("y", None, 2)],
            },
            "bad",
        );
        assert!(bad.is_err());
    }

    #[test]
    fn edit_serde_round_trip() {
        let annot = Annot {
            id: AnnotId(0),
            kind: AnnotKind::Connector {
                rect: crate::annot::Box2 {
                    x: 1.0,
                    y: 2.0,
                    w: 3.0,
                    h: 4.0,
                },
                label: "1".into(),
                partner: None,
            },
            color: [255, 0, 0],
            opacity: 1.0,
            stroke: 1.0,
            author: "me".into(),
            contents: String::new(),
            modified: "D:20261013000000".into(),
            created: String::new(),
            fill: None,
            dashed: false,
            cloudy: false,
            turn: Rotation::R0,
            name: String::new(),
        };
        let edits = vec![
            Edit::MovePages {
                ids: vec![PageId(1), PageId(2)],
                before: None,
            },
            Edit::RotatePages {
                ids: vec![PageId(0)],
                by: Rotation::R270,
            },
            Edit::InsertPages {
                source: SourceId(1),
                range: 0..3,
                before: Some(PageId(4)),
                outline: Some(flat(&[("X", Some(0), 0)])),
                annots: Vec::new(),
            },
            Edit::OutlineEdit {
                id: OutlineId(3),
                title: Some("T".into()),
                target: Some(None),
                color: Some(Some([1, 2, 3])),
                view: None,
                style: None,
            },
            Edit::ConnectorAdd {
                a: (PageId(0), annot.clone()),
                b: (PageId(1), annot),
            },
            Edit::SetTextLayers {
                layers: vec![(PageId(0), tl("hi"))],
            },
        ];
        let json = serde_json::to_string(&edits).unwrap();
        let back: Vec<Edit> = serde_json::from_str(&json).unwrap();
        assert_eq!(back, edits);
        // Absent keys leave a bookmark field unchanged; null clears it.
        let e: Edit = serde_json::from_str(r#"{"OutlineEdit":{"id":3,"target":null}}"#).unwrap();
        assert_eq!(
            e,
            Edit::OutlineEdit {
                id: OutlineId(3),
                title: None,
                target: Some(None),
                color: None,
                view: None,
                style: None,
            }
        );
    }
}
