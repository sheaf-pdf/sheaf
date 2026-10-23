//! Virtual document, edits, and snapshot history.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, Result, bail};
use im::Vector;
use sheaf_edit::{PageSize, Rotation};

use crate::annot::{Annot, AnnotId, AnnotKind};
use crate::change::{Cause, ChangeSet};
use crate::describe::Line;
use crate::effects::StepEffects;
use crate::forms::{FieldId, FileForm};
use crate::geom::Geom;
use crate::history::{HistoryPolicy, HistoryStats, MAX_VERSIONS};
use crate::materialize::record::drawn_of;
use crate::materialize::{Came, Profile, SaveInput, SaveNote, SaveRecord, SaveTarget, SavedSide};
use crate::open::{Facts, FileAnnot, FileBody, FlatOutlineItem, ViewFacts};
use crate::prim::{HeapSize, Key, Prim, Val, same};
use crate::serialize::durable::{self, FileStamp};
use crate::source::ObjRef;
use crate::store::{BlobId, BlobKind, BlobRef, Src, Store};
use crate::tx::{
    Actor, Cascade, Commit, CommitOpts, EditError, HostAttrs, Identity, LowerCx, Notice,
    ResolvedOp, Tx,
};

mod records;
mod stale;
mod state;

pub use records::{
    AnnotBody, AnnotRec, BlockKind, Bookmark, ContentStack, CtxNeeds, FieldNode, FieldProps,
    Filter, Foreign, FormDoc, FormInst, FormRef, Frozen, GenId, GenSpec, Generator, ImageRef,
    Layer, LegacySel, Mark, MarkAp, MarkedSel, PageBase, PageCtx, PageDef, PageForm, PinnedAp,
    PlaceTag, Placed, Placement, Scope, ScriptState, Static, StreamSel, Target, Terminal,
    WatermarkContent, WatermarkGen, Widget, WidgetProps, XfaState,
};
pub use sheaf_edit::{OutlineId, PageId, SourceId, TextStyle};
#[cfg(test)]
pub(crate) use state::visits;
pub use state::{Builder, DocState, Invariant};
pub(crate) use state::{LaidKey, RenderTables};

/// A group of versions that undo as one step (M10.5 §8.4): an OCR run's
/// flushes, a plugin call's batches. A commit in the group joins its
/// version when that's the current one and nothing came after it, and
/// makes another of the group's otherwise, so edits made in between leave
/// the group in fragments, undone one at a time or all at once
/// ([`Document::revert_group`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GroupId(u64);

/// What a group's versions are, which says whose commits it takes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GroupKind {
    /// An OCR run's flushes, the OCR job's.
    Ocr,
    /// A call of plugin `plugin`'s, its batches.
    PluginCall { plugin: Arc<str> },
}

impl GroupKind {
    /// Whether `actor` makes this kind of group's versions: one job's or
    /// one plugin's, never another's, whose edit would undo with them.
    fn takes(&self, actor: &Actor) -> bool {
        match (self, actor) {
            (GroupKind::Ocr, Actor::Job(crate::tx::JobKind::Ocr)) => true,
            (GroupKind::PluginCall { plugin }, Actor::Plugin { id, .. }) => id == plugin,
            _ => false,
        }
    }
}

/// A group that takes commits ([`Document::begin_group`]).
struct Group {
    kind: GroupKind,
    /// What its versions are called: its first one's label, which those
    /// made after another edit say they continue.
    label: Option<Arc<str>>,
    /// Its newest version, while one is kept.
    latest: Option<VersionId>,
}

/// A version in the history tree. It names one state, which never changes:
/// a group's amend puts a new version in its place. Ids aren't reused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct VersionId(pub u64);

/// One op as applied, and what it changed: prims with the values they
/// replaced (M10.5 §8.1). A replay applies them and checks each
/// before-value, so it rebuilds the version exactly, ids included, and
/// never lowers the op again.
#[derive(Debug, Clone)]
pub struct Step {
    /// The op, its references resolved: what describe reads.
    pub op: Arc<ResolvedOp>,
    /// What it changed. Coalesced over its version's steps (see
    /// [`coalesce`]), so a slot's change is on the last step that made
    /// one, and an op whose change a later one undid has none. None once
    /// its version is the first the history keeps, which nothing is
    /// rebuilt into (M10.5 §8.7).
    pub(crate) prims: Arc<[Prim]>,
    /// What it did, as it was lowered: against the state its op found.
    pub effects: StepEffects,
    /// What it did, in words (see [`crate::describe`]), against the
    /// document just before it.
    pub line: String,
    /// What the cascades did with it.
    pub cascades: Arc<[Cascade]>,
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
    /// Who made it, and the name the History panel shows them by.
    actor: Actor,
    author: String,
    /// When it was made: a PDF date string.
    at: String,
    /// The ops from the parent; none for the opened version. A group's
    /// later commits add to its version's steps. The first version kept
    /// keeps them, but not their prims.
    steps: Vec<Step>,
    /// The slots its steps change: their prims' keys, after coalescing
    /// ([`keys_of`]). What a save's path and the change of an undo, a redo
    /// or a jump take from it, without walking its prims.
    keys: Arc<BTreeSet<Key>>,
    /// The sources its steps bring pages of (inserted files), by blob,
    /// noted once as it's made: what pruning it may free
    /// ([`Document::undone_source_bytes`]). The first version holds those
    /// of the versions pruned before it too.
    blobs: HashSet<BlobId>,
    /// The state, unless trimming dropped it; then a jump rebuilds it by
    /// applying the steps' prims from the nearest ancestor that kept one.
    state: Option<DocState>,
    /// Rebuilding it failed ([`EditError::Mismatch`]), so it can't be
    /// reached.
    damaged: bool,
    /// It's on the line from the first version to the current one: not
    /// undone, nor on another branch. Kept as the cursor moves.
    on_line: bool,
    /// What its snapshot holds that no other version's does, which
    /// trimming frees, and what its steps hold, which only pruning the
    /// version does (see [`accounting`]).
    snapshot_bytes: usize,
    step_bytes: usize,
    /// The group it's of ([`Document::begin_group`]), and what the group
    /// is called, as History's "Undo all of …" says.
    group: Option<(GroupId, Arc<str>)>,
    /// Saves of it in flight ([`Document::save_input`]): while there's
    /// one, nothing amends it, and pruning never removes it.
    sealed: u16,
    /// The record of the file it's in (M10.5 §9.1), kept whole only while
    /// it's the saved version: a save's, or the opened file's, synthesized
    /// at open when the primary came from a file. An older one shrinks to
    /// its note in `saves`.
    record: Option<Arc<SaveRecord>>,
    /// Saves that wrote it, oldest first.
    saves: Vec<SaveNote>,
}

/// A version as the History panel shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct VersionView {
    pub id: VersionId,
    pub label: String,
    /// Who made it, and their name as the panel shows it.
    pub actor: Actor,
    pub author: String,
    /// PDF date string.
    pub at: String,
    /// What its ops did: a line each run of ops of one kind, with the
    /// badge of one that removed content.
    pub edits: Vec<Line>,
    pub saves: Vec<SaveNote>,
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
    /// Its replay failed, so it can't be reached
    /// ([`EditError::Mismatch`]).
    pub damaged: bool,
    /// The group it's of, and what the group is called: History offers to
    /// undo all of it ([`Document::revert_group`]). None once pruning took
    /// part of the group.
    pub group: Option<(GroupId, Arc<str>)>,
}

/// Everything a save needs, detached from the document so it can run on
/// another thread while editing continues.
#[derive(Clone)]
pub struct Snapshot {
    /// The document it's of, a number no other in the process has, and the
    /// version whose state it is: a version id names one only within its
    /// document.
    pub instance: u64,
    pub version: VersionId,
    pub state: DocState,
    /// The source the document opened on: what a Rewrite writes onto.
    pub primary: Src,
    /// The primary's name ([`Document::name`]).
    pub name: Arc<str>,
    /// The document's file ([`Document::file`]), which an export never
    /// replaces.
    pub file: Option<Arc<Path>>,
}

impl Snapshot {
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
}

/// What a plugin's call reads and lowers with (M10.5 §14.1, §14.3): the
/// current version, detached, and what a transaction on it lowers with.
/// The app publishes one at every change, and a call takes its base from
/// the latest at its first read or submit. It binds no actor: it makes a
/// plugin's transaction alone ([`plugin_tx`](Self::plugin_tx)), and
/// [`Document::lower_cx`] stays the only way to make any other's.
#[derive(Clone)]
pub struct Published {
    pub snap: Snapshot,
    lowering: Lowering,
}

impl Published {
    /// A transaction of plugin `id`'s, at its `version`, on the snapshot's
    /// version, as [`Document::tx_at`] makes one: lowering as of now, its
    /// marks' `<<name>>` and `<<initials>>` filled in only when the user
    /// granted it `identity` (§6.8).
    pub fn plugin_tx(&self, id: Arc<str>, version: Arc<str>, identity: bool) -> Tx {
        let actor = Actor::Plugin { id, version };
        Document::tx_at(
            &self.snap,
            self.lowering.cx(&self.snap.primary, actor, identity),
        )
    }

    /// The document's store, which what a plugin uploads is interned
    /// into, as a paste's assets are.
    pub fn store(&self) -> &Arc<Store> {
        &self.lowering.store
    }

    /// Whether `author` is the user, as the document knows them: what a
    /// plugin is told of an author it may not learn the name of (§6.8).
    pub fn is_user(&self, author: &str) -> bool {
        *self.lowering.identity.person == *author
    }
}

/// What a [`LowerCx`] is made of beside its actor and primary: the
/// document's store and id allocator, who the user is, the name of the
/// file that holds the document, and whether that file has opened.
#[derive(Clone)]
struct Lowering {
    store: Arc<Store>,
    alloc: Arc<IdAlloc>,
    identity: Identity,
    file: Arc<str>,
    ready: bool,
}

impl Lowering {
    /// What a transaction of `actor`'s on `primary` lowers with, as of
    /// now; a plugin's marks name the user only with `identity`.
    fn cx(&self, primary: &Src, actor: Actor, identity: bool) -> Arc<LowerCx> {
        Arc::new(LowerCx {
            store: self.store.clone(),
            primary: primary.clone(),
            alloc: self.alloc.clone(),
            host: HostAttrs::now(&self.identity, self.file.clone()),
            actor,
            identity,
            ready: self.ready,
        })
    }
}

/// Every mark of `state` in `group`, in document order: by page, then
/// down the page as it shows, then across.
pub(crate) fn group_members_in(
    state: &DocState,
    group: &crate::annot::Group,
) -> Vec<(usize, PageId, Annot)> {
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
/// so an id is never handed out twice, whatever undo does. Lowering draws
/// from it, in any transaction of the document's ([`LowerCx`]); a replay
/// applies prims, which hold the ids, and draws nothing.
#[derive(Default)]
pub(crate) struct IdAlloc {
    page: AtomicU64,
    outline: AtomicU64,
    field: AtomicU64,
}

impl IdAlloc {
    pub(crate) fn page(&self) -> PageId {
        PageId(self.page.fetch_add(1, Ordering::Relaxed))
    }

    pub(crate) fn outline(&self) -> OutlineId {
        OutlineId(self.outline.fetch_add(1, Ordering::Relaxed))
    }

    pub(crate) fn field(&self) -> FieldId {
        FieldId(self.field.fetch_add(1, Ordering::Relaxed))
    }

    /// The ids it would hand out next: page, bookmark and field.
    #[cfg(any(test, feature = "fixtures"))]
    pub(crate) fn next(&self) -> [u64; 3] {
        [&self.page, &self.outline, &self.field].map(|c| c.load(Ordering::Relaxed))
    }

    /// Field ids from `next` on: a file's fields keep the ids it read them
    /// with.
    fn fields_from(&self, next: u64) {
        self.field.fetch_max(next, Ordering::Relaxed);
    }
}

/// The saved version: the one whose record says what the document's file
/// holds, or, restored from the file's recovery sidecar, holds once its
/// first save writes what the record has pending.
enum SavedVersion {
    /// A version the history holds.
    At(VersionId),
    /// One pruning took (M10.5 §8.7): the first version kept moved past
    /// it, or past where the branch holding it turned off. What a save
    /// needs of it stays.
    Detached(Box<Anchor>),
}

/// What a save needs of a saved version pruning took: its state and its
/// record, which the next save writes after, and the slots any step
/// changed on the tree path from it to the first version kept, that
/// one's own edge included, which grow as that one moves on. A save's
/// path keys are those and the path's from there (§7.2).
struct Anchor {
    state: DocState,
    record: Arc<SaveRecord>,
    keys_to_root: BTreeSet<Key>,
}

/// What the history holds, counted as versions come and go and snapshots
/// are dropped and rebuilt, so an edit weighs it without walking the
/// history.
#[derive(Default)]
struct Held {
    /// Versions keeping their snapshots, and those snapshots' bytes.
    snapshots: BTreeSet<VersionId>,
    snapshot_bytes: usize,
    /// Every version's steps' bytes.
    step_bytes: usize,
    /// [`Document::undone_source_bytes`] as last counted, until a version
    /// that brings a source comes or goes, or the cursor moves.
    undone: Option<usize>,
}

pub struct Document {
    /// Which document of the process's it is: its versions' ids are
    /// counted afresh, so a transaction made on another's lands on none of
    /// its versions.
    instance: u64,
    /// The blobs its sources are in, outside undo.
    store: Arc<Store>,
    /// The source it opened on, and the name it goes by (see
    /// [`name`](Self::name)).
    primary: Src,
    name: Arc<str>,
    versions: HashMap<VersionId, Version>,
    /// What they hold, as the policy weighs it.
    held: Held,
    root: VersionId,
    cursor: VersionId,
    /// The saved version, or what a save needs of it once it was pruned.
    /// None when no file holds the document: one made in memory, before
    /// its first save.
    saved: Option<SavedVersion>,
    next_version: u64,
    /// Who the user is, as the host fills marks in (the app sets it).
    identity: Identity,
    ids: Arc<IdAlloc>,
    policy: Box<dyn HistoryPolicy>,
    /// The groups that take commits ([`begin_group`](Self::begin_group)),
    /// and the next one's id.
    groups: HashMap<GroupId, Group>,
    next_group: u64,
    /// Groups pruning took a version of, or the prims of the first version
    /// kept: undoing all of one would take back only part of it.
    cut_groups: HashSet<GroupId>,
    /// The reader has been told some text shows as `?` in the file
    /// ([`Notice::Unencodable`]). Outside history: undo doesn't make it
    /// news again.
    told_unencodable: bool,
    /// The opened version holds the file's facts: until then edits are
    /// refused ([`EditError::NotReady`]).
    ready: bool,
}

impl Document {
    /// A document showing `primary`, a source in `store`, in its
    /// entirety, under `name` (see [`name`](Self::name)), with `outline`,
    /// what its pages hold beside their content (its source's
    /// [`annots`](crate::Source::annots)), and nothing else: a file sheaf
    /// has just made, which says nothing more about itself. How it `came`
    /// is what's known of the file it's of, when it's of one (see
    /// [`loading`](Self::loading)). A file that's opened opens through
    /// [`crate::open`], which reads what it says.
    pub fn new(
        store: Arc<Store>,
        primary: Src,
        name: Arc<str>,
        came: Option<Came>,
        outline: &[FlatOutlineItem],
        policy: Box<dyn HistoryPolicy>,
    ) -> Self {
        let annots = primary.annots().clone();
        let mut doc = Self::loading(store, primary, name, came, policy);
        let facts = Facts {
            annots,
            outline: outline.to_vec(),
            ..Default::default()
        };
        let (state, _) = doc.opened(facts);
        doc.set_state(doc.root, Some(state));
        doc.ready = true;
        doc
    }

    /// A document showing `primary` while it opens: its pages, from the
    /// page table its source read as it opened, and nothing else, until
    /// [`ready`](Self::ready). See [`crate::open::start`]. How it `came`
    /// gives the opened version its file's record: the file's as it was
    /// read ([`SaveRecord::opened`]), or, restored from its recovery
    /// sidecar, the file's with the sidecar's sections pending
    /// ([`SaveRecord::restored`]), so the document isn't what the file
    /// holds until a save writes them. A primary made in memory has none,
    /// so its first save is a Rewrite.
    pub(crate) fn loading(
        store: Arc<Store>,
        primary: Src,
        name: Arc<str>,
        came: Option<Came>,
        policy: Box<dyn HistoryPolicy>,
    ) -> Self {
        debug_assert!(store.holds(primary.blob()), "{name} is in another store");
        // A page the tree lists again is a copy of the first.
        let ids = IdAlloc::default();
        let pages = primary.pages().iter().zip(0..).map(|(facts, index)| {
            let base = PageBase {
                src: primary.clone(),
                index,
            };
            let def = PageDef::new(base, facts.repeat, ContentStack::empty());
            (ids.page(), Arc::new(def), Arc::new(facts.geom))
        });
        let state = DocState::builder().pages(pages).build();
        let record = came.map(|came| {
            Arc::new(match came {
                Came::Read { path, stamp } => {
                    SaveRecord::opened(VersionId(0), &primary, path, stamp)
                }
                Came::Restored {
                    path,
                    base,
                    pending,
                } => SaveRecord::restored(VersionId(0), &primary, path, base, pending),
            })
        });
        static INSTANCES: AtomicU64 = AtomicU64::new(0);
        let mut doc = Self {
            instance: INSTANCES.fetch_add(1, Ordering::Relaxed),
            store,
            primary,
            name,
            versions: HashMap::new(),
            held: Held::default(),
            root: VersionId(0),
            cursor: VersionId(0),
            saved: Some(SavedVersion::At(VersionId(0))),
            next_version: 1,
            identity: Identity::default(),
            ids: Arc::new(ids),
            policy,
            groups: HashMap::new(),
            next_group: 1,
            cut_groups: HashSet::new(),
            told_unencodable: false,
            ready: false,
        };
        doc.insert_version(
            VersionId(0),
            Version {
                parent: None,
                children: Vec::new(),
                redo_to: None,
                label: "Open".into(),
                actor: Actor::User,
                author: String::new(),
                at: crate::annot::pdf_date_now(),
                steps: Vec::new(),
                keys: Arc::default(),
                blobs: HashSet::new(),
                state: Some(state),
                damaged: false,
                on_line: true,
                snapshot_bytes: 0,
                step_bytes: 0,
                group: None,
                sealed: 0,
                record,
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
    /// document, and the change, from the version with the pages alone,
    /// in which anything may have changed ([`ChangeSet::opened`]). Only
    /// once, while the file opens: there are no late facts (M10.5 proposal
    /// 28).
    pub fn ready(&mut self, facts: Facts) -> Result<(ViewFacts, ChangeSet)> {
        if self.ready {
            bail!("the file has opened already");
        }
        let (state, view) = self.opened(facts);
        let old = self.root;
        let mut v = self.take_version(old).expect("the opened version");
        v.state = Some(state);
        let label = v.label.as_str().into();
        let id = self.replace(old, v);
        self.ready = true;
        Ok((view, ChangeSet::opened(old, id, label, self.state())))
    }

    /// Whether the file's facts are in: until then edits are refused.
    pub fn is_ready(&self) -> bool {
        self.ready
    }

    /// The opened version's state: its pages, and what `facts` say about
    /// them, each page index mapped to its page once: what the primary
    /// file holds. Its record, when it has one, says what the generators
    /// found drew on its pages ([`drawn_of`]). Gives back what the state
    /// doesn't hold.
    ///
    /// [`drawn_of`]: crate::materialize::record::drawn_of
    fn opened(&mut self, facts: Facts) -> (DocState, ViewFacts) {
        let loading = self.state().clone();
        let by_index: Vec<PageId> = loading.order().iter().copied().collect();
        let at = |index: usize| by_index.get(index).copied();
        let src = self.primary.clone();
        let (roots_outline, bookmarks) = outline_from_flat(
            || self.ids.outline(),
            &facts.outline,
            |i| at(i as usize),
            &src,
        );
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
        // The generators sheaf drew, each the generator its blocks name, on
        // the pages that hold them, a header's Bates numbers its blocks'.
        let generators: Vec<Arc<Generator>> = (facts.generators.iter())
            .filter_map(|saved| saved.generator(&src, |i| at(i as usize)))
            .map(Arc::new)
            .collect();
        let opened: Vec<GenId> = generators.iter().map(|g| g.id).collect();
        let root = self.versions.get_mut(&self.root);
        if let Some(record) = root.and_then(|v| v.record.as_mut()) {
            let drawn = drawn_of(&facts.generators, src.id(), |i| at(i as usize));
            Arc::make_mut(record).drawn = Arc::new(drawn);
        }
        // The form: its nodes as the file numbers them, and its widgets,
        // their ids drawn from the file's hash, so the same bytes open the
        // same, none another annotation's.
        let hash = src.id().hash;
        let mut rng = fastrand::Rng::with_seed(hash as u64 ^ (hash >> 64) as u64);
        let form = &facts.form;
        let inst = FormInst {
            src: src.id(),
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
            false => im::OrdMap::unit(inst, self.name.clone()),
        };
        let form_doc = FormDoc::opened(roots, names, &form.facts);
        let pages = loading
            .pages()
            .map(|(id, def, geom)| (id, def.clone(), geom.clone()));
        let state = DocState::builder()
            .pages(pages)
            .outline(roots_outline, bookmarks)
            .annots(annots)
            .labels(labels)
            .generators(generators)
            .opened_generators(src.id(), opened)
            .form(fields, Arc::new(form_doc))
            .build();
        debug_assert_eq!(state.check_invariants(), Ok(()));
        (state, view)
    }

    /// Who the user is, as the marks and versions made from now on say
    /// (M10.5 §6.8).
    pub fn set_identity(&mut self, who: Identity) {
        self.identity = who;
    }

    /// The number no other document in the process has, which a version
    /// names a state only within ([`Snapshot::instance`]).
    pub fn instance(&self) -> u64 {
        self.instance
    }

    /// The source the document opened on: what a Rewrite writes onto.
    pub fn primary(&self) -> &Src {
        &self.primary
    }

    /// The name the primary goes by where the reader sees it: its form
    /// instance's ([`FormDoc::names`]). Never the document's file, which
    /// is [`file`](Self::file).
    pub fn name(&self) -> &Arc<str> {
        &self.name
    }

    /// Where its sources' bytes are: what an edit's source is interned
    /// into.
    pub fn store(&self) -> &Arc<Store> {
        &self.store
    }

    /// Where page stamp `stem`'s page is, as an edit that brings a mark
    /// naming it finds it: the store's blob of that name, held by whoever
    /// placed or pasted it; else the form a source's marks, or another
    /// tool's stamp a mark can be made of, draw it with, the primary's
    /// first. None when nothing holds it.
    pub fn find_stamp(&self, stem: &str) -> Option<FormRef> {
        find_stamp(&self.store, &self.primary, stem)
    }

    /// Where attachment `stem`'s file is, `size` bytes long, as an edit
    /// that brings a mark naming it finds it: the store's blob of that
    /// name, held by whoever attached or pasted it; else the file a
    /// source's marks embed under it, the primary's first, decoded into
    /// the store.
    pub fn find_file(&self, stem: &str, size: u64) -> Option<BlobRef> {
        find_file(&self.store, &self.primary, stem, size)
    }

    pub fn state(&self) -> &DocState {
        self.versions[&self.cursor]
            .state
            .as_ref()
            .expect("the current version keeps its state")
    }

    /// The current version, detached for another thread: its state and
    /// sources shared, nothing computed.
    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            instance: self.instance,
            version: self.cursor,
            state: self.state().clone(),
            primary: self.primary.clone(),
            name: self.name.clone(),
            file: self.file().map(|(path, _)| path.clone()),
        }
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

    /// Whether the document isn't what its file holds: the current
    /// version isn't the saved one, or a recovery sidecar restored it with
    /// sections its file hasn't got yet.
    pub fn is_dirty(&self) -> bool {
        self.on_disk() != Some(self.cursor)
    }

    /// The version the document's file holds: the saved one, unless its
    /// record has sections pending (§11.10), or pruning took it.
    fn on_disk(&self) -> Option<VersionId> {
        let pending = self
            .saved_record()
            .is_some_and(|r| r.pending_tail.is_some());
        self.saved_at().filter(|_| !pending)
    }

    /// The saved version, while the history holds it.
    fn saved_at(&self) -> Option<VersionId> {
        match self.saved {
            Some(SavedVersion::At(s)) => Some(s),
            _ => None,
        }
    }

    /// What a save of version `v` writes from (M10.5 §9.1), and `v` sealed
    /// until [`record_save`](Self::record_save) or
    /// [`abandon_save`](Self::abandon_save) for it: an edit made during
    /// the save, an OCR batch among them, makes a version after it rather
    /// than amending it, and pruning keeps it. A version no longer kept,
    /// or one that can't be rebuilt, is an error.
    pub fn save_input(&mut self, v: VersionId) -> Result<SaveInput> {
        if !self.versions.contains_key(&v) {
            bail!("that version is no longer kept");
        }
        self.ensure_state(v)?;
        let input = self.input(v);
        self.versions.get_mut(&v).expect("version").sealed += 1;
        Ok(input)
    }

    /// What a recovery sidecar of the current version writes from: as
    /// [`save_input`](Self::save_input), but nothing is sealed, since the
    /// sidecar is never the saved file.
    pub fn recovery_input(&self) -> SaveInput {
        self.input(self.cursor)
    }

    /// For tests: what an Append of version `v` writes from, nothing
    /// sealed. A document made in memory, which has no record, appends
    /// onto its saved version as though the file it was opened from held
    /// its primary's bytes: their record as opened, at the primary's name,
    /// which is never read.
    #[cfg(any(test, feature = "fixtures"))]
    pub fn append_input(&self, v: VersionId) -> SaveInput {
        let mut input = self.input(v);
        if input.saved.is_none()
            && let Some(s) = self.saved_at()
            && let Some(state) = self.versions[&s].state.clone()
        {
            let stamp = FileStamp {
                len: self.primary.bytes().len() as u64,
                mtime: std::time::SystemTime::UNIX_EPOCH,
                tail: 0,
                window: 0,
            };
            let path: Arc<Path> = Path::new(&*self.name).into();
            let record = SaveRecord {
                drawn: Arc::new(self.drawn_at_open(&state)),
                ..SaveRecord::opened(s, &self.primary, path, stamp)
            };
            input.saved = Some(SavedSide {
                state,
                record: Arc::new(record),
            });
            input.path_keys = self.path_keys(s, v);
        }
        input
    }

    /// For tests: a document made in memory, which has no record, as
    /// though opened from `path`: its primary's bytes written there, and
    /// its saved version given their record as opened.
    #[cfg(any(test, feature = "fixtures"))]
    pub fn opened_from(&mut self, path: &Path) -> Result<()> {
        let s = self.saved_at().context("nothing saved is kept")?;
        std::fs::write(path, &**self.primary.bytes())?;
        let stamp = FileStamp::read(path)?;
        let version = (self.versions.get(&s)).context("the saved version is no longer kept")?;
        let drawn =
            (version.state.as_ref()).map_or_else(Default::default, |st| self.drawn_at_open(st));
        let record = SaveRecord {
            drawn: Arc::new(drawn),
            ..SaveRecord::opened(s, &self.primary, path.into(), stamp)
        };
        let version = self.versions.get_mut(&s).expect("found above");
        version.record = Some(Arc::new(record));
        Ok(())
    }

    /// What the generators the primary's open finds drew on the pages of
    /// `st`, the state it opened as: each page showing its page of that
    /// index ([`drawn_of`]).
    ///
    /// [`drawn_of`]: crate::materialize::record::drawn_of
    #[cfg(any(test, feature = "fixtures"))]
    fn drawn_at_open(&self, st: &DocState) -> crate::materialize::record::DrawnBlocks {
        let src = &self.primary;
        drawn_of(src.generators(), src.id(), |i| st.page_at(i as usize))
    }

    /// Version `v`'s save input, from its state, which it keeps, and the
    /// saved side, the history's saved version's or what was kept of it
    /// once pruned.
    fn input(&self, v: VersionId) -> SaveInput {
        let (saved, path_keys) = match &self.saved {
            Some(SavedVersion::At(s)) => {
                let at = &self.versions[s];
                match (&at.state, &at.record) {
                    (Some(state), Some(record)) => {
                        let side = SavedSide {
                            state: state.clone(),
                            record: record.clone(),
                        };
                        (Some(side), self.path_keys(*s, v))
                    }
                    _ => (None, Arc::default()),
                }
            }
            Some(SavedVersion::Detached(anchor)) => {
                let side = SavedSide {
                    state: anchor.state.clone(),
                    record: anchor.record.clone(),
                };
                let mut keys = anchor.keys_to_root.clone();
                let after = self.keys_along(&self.edges_between(self.root, v));
                keys.extend(after.iter().copied());
                (Some(side), Arc::new(keys))
            }
            None => (None, Arc::default()),
        };
        SaveInput {
            version: v,
            target: (self.versions[&v].state.clone())
                .expect("a version being saved keeps its state"),
            primary: self.primary.clone(),
            saved,
            path_keys,
        }
    }

    /// The keys any step changed on the tree path from version `saved` to
    /// version `v`, through the last version both come from (M10.5 §7.2):
    /// the slots a save of `v` may find changed since `saved`'s file. The
    /// path never takes in the first version's edge, whose prims are gone.
    /// Once pruning took the saved version, its anchor has the keys from
    /// it to the first version kept, and the path runs on from there
    /// ([`input`](Self::input)).
    fn path_keys(&self, saved: VersionId, v: VersionId) -> Arc<BTreeSet<Key>> {
        self.keys_along(&self.edges_between(saved, v))
    }

    /// The versions whose edges make the tree path from version `a` to
    /// version `b`: those from each up to the last version both come from,
    /// which isn't one; none from a version to itself.
    fn edges_between(&self, a: VersionId, b: VersionId) -> Vec<VersionId> {
        let (from_a, from_b) = self.sides(a, b);
        [from_a, from_b].concat()
    }

    /// The versions whose edges make the tree path from version `a` to
    /// version `b`, `a`'s side and `b`'s: those from each up to the last
    /// version both come from, which isn't one. It walks the path, not the
    /// lines down from the root.
    fn sides(&self, a: VersionId, b: VersionId) -> (Vec<VersionId>, Vec<VersionId>) {
        let parent = |v: VersionId| self.versions[&v].parent;
        let (mut up_a, mut up_b) = (vec![a], vec![b]);
        let (mut seen_a, mut seen_b) = (HashSet::from([a]), HashSet::from([b]));
        // Up from both ends a version at a time, until one reaches a
        // version the other has passed: the first is where the path turns.
        let turn = loop {
            let (at_a, at_b) = (up_a[up_a.len() - 1], up_b[up_b.len() - 1]);
            if seen_b.contains(&at_a) {
                break at_a;
            }
            if seen_a.contains(&at_b) {
                break at_b;
            }
            let (pa, pb) = (parent(at_a), parent(at_b));
            assert!(pa.is_some() || pb.is_some(), "{a:?} and {b:?} in two trees");
            if let Some(p) = pa {
                up_a.push(p);
                seen_a.insert(p);
            }
            if let Some(p) = pb {
                up_b.push(p);
                seen_b.insert(p);
            }
        };
        let below = |up: Vec<VersionId>| up.into_iter().take_while(|&v| v != turn).collect();
        (below(up_a), below(up_b))
    }

    /// The slots the steps of `edges` change: one version's own set, or
    /// their union.
    fn keys_along(&self, edges: &[VersionId]) -> Arc<BTreeSet<Key>> {
        match edges {
            [one] => self.versions[one].keys.clone(),
            _ => Arc::new(
                (edges.iter())
                    .flat_map(|id| self.versions[id].keys.iter().copied())
                    .collect(),
            ),
        }
    }

    /// A save of `rec.version` wrote `rec`'s file: that version is the
    /// saved one now, and holds the record, the previous saved version's
    /// shrinking to the note it has, or, once pruned, what was kept of it
    /// going. It's noted among the version's saves and unsealed. Nothing
    /// else changes: no version, state or source, so the change is empty
    /// ([`Cause::Saved`]).
    pub fn record_save(&mut self, rec: SaveRecord) -> Result<ChangeSet> {
        self.attach(rec, true)
    }

    /// A save of `rec.version` found nothing to write, the version being
    /// what the saved file holds (an Unchanged plan, M10.5 §9.1): `rec`,
    /// the saved file's record, is that version's now, so it's the saved
    /// one and the document is clean. As [`record_save`](Self::record_save),
    /// but no save is noted.
    pub fn record_unchanged(&mut self, rec: SaveRecord) -> Result<ChangeSet> {
        self.attach(rec, false)
    }

    /// `rec` the record of its version, the saved one now, noted among its
    /// saves when `noted`: a change of nothing at the current version, the
    /// user's.
    fn attach(&mut self, rec: SaveRecord, noted: bool) -> Result<ChangeSet> {
        let v = rec.version;
        if !self.versions.contains_key(&v) {
            bail!("the saved version is no longer kept");
        }
        if let Some(was) = self.saved_at().filter(|&s| s != v)
            && let Some(was) = self.versions.get_mut(&was)
        {
            was.record = None;
        }
        let version = self.versions.get_mut(&v).expect("version");
        if noted {
            version.saves.push(rec.note());
        }
        version.record = Some(Arc::new(rec));
        version.sealed = version.sealed.saturating_sub(1);
        self.saved = Some(SavedVersion::At(v));
        let label = "Save".into();
        Ok(ChangeSet::empty(
            self.cursor,
            Cause::Saved,
            Actor::User,
            label,
        ))
    }

    /// A save of version `v` failed or was canceled: it's unsealed, and
    /// nothing else changes.
    pub fn abandon_save(&mut self, v: VersionId) {
        if let Some(version) = self.versions.get_mut(&v) {
            version.sealed = version.sealed.saturating_sub(1);
        }
    }

    /// How many saves of version `v` hold it sealed now.
    #[cfg(test)]
    pub(crate) fn sealed(&self, v: VersionId) -> u16 {
        self.versions.get(&v).map_or(0, |version| version.sealed)
    }

    /// The saved version's record: what sheaf knows of the file on disk.
    /// None when nothing is saved, or the saved version was made in
    /// memory.
    pub fn saved_record(&self) -> Option<&Arc<SaveRecord>> {
        match self.saved.as_ref()? {
            SavedVersion::At(s) => self.versions.get(s)?.record.as_ref(),
            SavedVersion::Detached(anchor) => Some(&anchor.record),
        }
    }

    /// The document's file, and that file as sheaf last read or wrote it:
    /// the saved record's, a restored document's the file its sidecar was
    /// written against, which its first save checks is still there (M10.5
    /// §11.10) and its own sidecars are written against too. Never the
    /// primary's name: a Save As makes another file the document's, and
    /// the primary stays the file it opened. None for a document no file
    /// holds.
    pub fn file(&self) -> Option<(&Arc<Path>, &FileStamp)> {
        self.saved_record().map(|rec| (&rec.path, &rec.stamp))
    }

    /// Where a save to `path` writes: the document's file
    /// ([`file`](Self::file)) or another, and when it's the document's, the
    /// stamp it must still have, for the plan's and the write's checks.
    pub fn save_target(&self, path: &Path) -> SaveTarget {
        let file = self.file().filter(|(at, _)| durable::same_file(at, path));
        SaveTarget {
            path: path.into(),
            same_file: file.is_some(),
            profile: Profile::default(),
            stamp: file.map(|(_, stamp)| *stamp),
        }
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

    /// Back to the parent version: what that changed, labelled as the
    /// version undone, or None with nothing to undo.
    pub fn undo(&mut self) -> Result<Option<ChangeSet>, EditError> {
        let from = self.cursor;
        let Some(parent) = self.versions[&from].parent else {
            return Ok(None);
        };
        self.ensure_state(parent)?;
        self.versions.get_mut(&parent).expect("parent").redo_to = Some(from);
        Ok(Some(self.move_to(parent, Cause::Undo)))
    }

    /// On to the version redo goes to: what that changed, labelled as that
    /// version, or None with nothing to redo.
    pub fn redo(&mut self) -> Result<Option<ChangeSet>, EditError> {
        let Some(to) = self.redo_target() else {
            return Ok(None);
        };
        self.ensure_state(to)?;
        Ok(Some(self.move_to(to, Cause::Redo)))
    }

    /// History before the current version forgotten, as pruning forgets it
    /// ([`advance_root`](Self::advance_root)): the current version is the
    /// first one kept, with nothing to undo, and what it brings it keeps.
    /// The saved version, when it goes, is kept as an anchor, so the
    /// document still knows its file and is dirty. What a document made by
    /// one transaction over another's file hands on (Combine, M10.5
    /// proposal 35). Not past a version being saved, nor while a branch
    /// off the line holds one: whether it got to the current version.
    pub fn forget_before_cursor(&mut self) -> bool {
        while self.root != self.cursor {
            if !self.advance_root() {
                return false;
            }
        }
        true
    }

    /// Make `id` the current version, as undo and redo do: not an edit,
    /// so jumping back and forth is free. Redo from any version on the way
    /// to it then leads toward it. What that changed, labelled as `id`, or
    /// None when it's the current version already.
    pub fn jump(&mut self, id: VersionId) -> Result<Option<ChangeSet>, EditError> {
        if !self.versions.contains_key(&id) {
            return Err(EditError::not_found("jump", format!("version {}", id.0)));
        }
        if id == self.cursor {
            return Ok(None);
        }
        self.ensure_state(id)?;
        let mut child = id;
        while let Some(parent) = self.versions[&child].parent {
            self.versions.get_mut(&parent).expect("parent").redo_to = Some(child);
            child = parent;
        }
        Ok(Some(self.move_to(id, Cause::Jump)))
    }

    /// The cursor moved to version `to`, which has its state, by an undo,
    /// a redo or a jump, which the user makes: what changed, by the keys
    /// of every version's steps on the way, labelled as the version undone
    /// or the one gone to.
    fn move_to(&mut self, to: VersionId, cause: Cause) -> ChangeSet {
        let from = std::mem::replace(&mut self.cursor, to);
        // Another line: the versions left are off it, and other versions'
        // sources may be undone now.
        let (left, joined) = self.sides(from, to);
        for (versions, on_line) in [(&left, false), (&joined, true)] {
            for v in versions {
                self.versions.get_mut(v).expect("version").on_line = on_line;
            }
        }
        self.held.undone = None;
        let keys = self.keys_along(&[left, joined].concat());
        let named = match cause {
            Cause::Undo => from,
            _ => to,
        };
        let label = self.versions[&named].label.as_str().into();
        let state = |id: VersionId| {
            (self.versions[&id].state.as_ref()).expect("both ends keep their states")
        };
        let (before, after) = (state(from), state(to));
        ChangeSet::of(from, to, cause, Actor::User, label, keys, before, after)
    }

    pub fn current_version(&self) -> VersionId {
        self.cursor
    }

    /// The steps that made version `id` from its parent: the edge into it,
    /// which is what a merge would replay (`todos/file-contention.md`).
    pub fn edge(&self, id: VersionId) -> Option<&[Step]> {
        Some(&self.versions.get(&id)?.steps)
    }

    /// What a transaction of `actor`'s lowers with: the store, the id
    /// allocator, who the user is and now, and the name of the file that
    /// holds the document (the one it opened as until it's saved). The
    /// only way to make one for any actor, so a transaction can't claim
    /// another (M10.5 §6.7); a plugin's never names the user here, which
    /// only [`Published::plugin_tx`] grants (§6.8).
    pub fn lower_cx(&self, actor: Actor) -> Arc<LowerCx> {
        self.lowering().cx(&self.primary, actor, false)
    }

    /// What its transactions lower with, beside the actor.
    fn lowering(&self) -> Lowering {
        let file = self.file().and_then(|(path, _)| path.file_name());
        let file = file.map_or_else(|| self.name.clone(), |f| f.to_string_lossy().into());
        Lowering {
            store: self.store.clone(),
            alloc: self.ids.clone(),
            identity: self.identity.clone(),
            file,
            ready: self.ready,
        }
    }

    /// The current version as plugin calls take it (M10.5 §14.3): its
    /// snapshot, and what a plugin's transaction on it lowers with.
    pub fn published(&self) -> Published {
        Published {
            snap: self.snapshot(),
            lowering: self.lowering(),
        }
    }

    /// A transaction of `actor`'s on the current version.
    pub fn tx(&self, actor: Actor) -> Tx {
        let cx = self.lower_cx(actor);
        Tx::new(self.instance, self.cursor, self.state().clone(), cx)
    }

    /// A transaction lowering with `cx` on `snap`'s version: one a planner
    /// builds off the UI thread, or a dialog keeps while it's open, and
    /// commits with [`Base::At`](crate::tx::Base::At) that version once
    /// the document may have moved on (M10.5 §8.5).
    pub fn tx_at(snap: &Snapshot, cx: Arc<LowerCx>) -> Tx {
        Tx::new(snap.instance, snap.version, snap.state.clone(), cx)
    }

    /// `tx`'s ops as a version after the current one, which `opts` names;
    /// the versions that were after the current one stay, as a branch.
    /// None when they change nothing. Refused while the file opens
    /// ([`EditError::NotReady`]). A transaction made on another version
    /// lands on the current one as `opts.stale` says
    /// ([`Stale`](crate::tx::Stale)), or is an [`EditError::Conflict`] and
    /// changes nothing. One in a group (`opts.group`) joins the group's
    /// version when that's the current one, nothing came after it, and it
    /// was never saved and isn't being saved, and makes another of the
    /// group's otherwise; once the group's edits were undone, or it ended,
    /// it's [`EditError::GroupEnded`] and changes nothing (M10.5 §8.4).
    pub fn commit(&mut self, tx: Tx, opts: CommitOpts) -> Result<Commit, EditError> {
        let (group, depth) = (opts.group, opts.depth);
        if let Some(g) = group {
            self.joins(g, tx.actor())?;
        }
        let (parts, mut commit) = self.settle(tx, opts)?;
        if parts.steps.iter().all(|s| s.prims.is_empty()) {
            return Ok(commit);
        }
        if let Some(g) = group
            && self.amends(g)
        {
            let mut commit = self.amend(g, parts, commit);
            commit.change.depth = depth;
            return Ok(commit);
        }
        let (from, before) = (self.cursor, self.state().clone());
        let (label, tag) = match group {
            Some(g) => self.of_group(g, commit.change.label.clone()),
            None => (commit.change.label.clone(), None),
        };
        let actor = parts.actor().clone();
        self.push(parts.state, label.to_string(), parts.steps, actor, tag);
        commit.version = Some(self.cursor);
        commit.change = self.made(from, &before, Cause::Commit, label);
        commit.change.depth = depth;
        Ok(commit)
    }

    /// What the version just made at the cursor changed from version
    /// `from`, whose state was `before`: all its keys, by its actor.
    fn made(&self, from: VersionId, before: &DocState, cause: Cause, label: Arc<str>) -> ChangeSet {
        let v = &self.versions[&self.cursor];
        let (actor, keys) = (v.actor.clone(), v.keys.clone());
        ChangeSet::of(
            from,
            self.cursor,
            cause,
            actor,
            label,
            keys,
            before,
            self.state(),
        )
    }

    /// `val` put at `key` as a version of its own, labelled `label`: what
    /// no op sets yet (a fill, the form's calculation order), for tests of
    /// what a save writes of it. Its step names an op it doesn't run.
    #[cfg(test)]
    pub(crate) fn put_for_test(&mut self, key: Key, val: Option<Val>, label: &str) {
        let before = self.state().clone();
        let mut rec = crate::prim::Rec::new(before.clone());
        rec.put(key, val);
        let (state, prims) = rec.finish();
        let step = Step {
            op: Arc::new(ResolvedOp::SetPageLabels { ranges: None }),
            effects: StepEffects::of(&before, &state, &prims),
            prims: prims.into(),
            line: label.to_string(),
            cascades: Arc::new([]),
        };
        self.push(state, label.into(), vec![step], Actor::User, None);
    }

    /// For tests: the ids the allocator hands out next (page, bookmark,
    /// field), which lowering draws and a check never does.
    #[cfg(any(test, feature = "fixtures"))]
    pub fn ids_next(&self) -> [u64; 3] {
        self.ids.next()
    }

    /// Start a group of `kind`, whose versions undo as one step: an OCR
    /// run, a plugin call. It takes commits until
    /// [`end_group`](Self::end_group).
    pub fn begin_group(&mut self, kind: GroupKind) -> GroupId {
        let id = GroupId(self.next_group);
        self.next_group += 1;
        let group = Group {
            kind,
            label: None,
            latest: None,
        };
        self.groups.insert(id, group);
        id
    }

    /// Group `g` takes no more commits: its run or call is over. Its
    /// versions stay, and History can still undo all of them.
    pub fn end_group(&mut self, g: GroupId) {
        self.groups.remove(&g);
    }

    /// For tests: what group `g` is, while it takes commits.
    #[cfg(any(test, feature = "fixtures"))]
    pub fn group_kind(&self, g: GroupId) -> Option<&GroupKind> {
        Some(&self.groups.get(&g)?.kind)
    }

    /// Whether a commit by `actor` may be in group `g`: one its kind takes
    /// (another's is a bug), while the group takes commits and its latest
    /// version is the current one or one before it (§8.4). Otherwise the
    /// group's edits were undone, or jumped away from, or the group has
    /// ended, and what the commit adds is never put back:
    /// [`EditError::GroupEnded`].
    fn joins(&self, g: GroupId, actor: &Actor) -> Result<(), EditError> {
        let Some(group) = self.groups.get(&g) else {
            return Err(EditError::GroupEnded);
        };
        if !group.kind.takes(actor) {
            let kind = &group.kind;
            return Err(EditError::internal(format!(
                "a commit of {actor:?}'s in a group of {kind:?}"
            )));
        }
        match group.latest {
            Some(latest) if !self.reaches(latest) => Err(EditError::GroupEnded),
            _ => Ok(()),
        }
    }

    /// Whether version `v` is the current one or one before it.
    fn reaches(&self, v: VersionId) -> bool {
        let mut at = Some(self.cursor);
        while let Some(id) = at {
            if id == v {
                return true;
            }
            at = self.versions[&id].parent;
        }
        false
    }

    /// Whether a commit in group `g` joins the current version (§8.4's
    /// amend rule): it's the group's, nothing came after it, it was never
    /// saved, it isn't being saved (sealed: what the save writes must stay
    /// as it is), and it isn't the first (pruning can make it so). It's the
    /// group's latest, then: [`joins`](Self::joins) let the commit in.
    fn amends(&self, g: GroupId) -> bool {
        let v = &self.versions[&self.cursor];
        v.group.as_ref().is_some_and(|(id, _)| *id == g)
            && v.parent.is_some()
            && v.children.is_empty()
            && v.saves.is_empty()
            && v.sealed == 0
            && self.saved_at() != Some(self.cursor)
    }

    /// A version of group `g`'s, which a commit labelled `label` makes:
    /// what it's called, and its tag. The group's first is called as its
    /// commit is, and names the group; one made after another edit, or a
    /// save, says it continues the group.
    fn of_group(&mut self, g: GroupId, label: Arc<str>) -> (Arc<str>, Option<(GroupId, Arc<str>)>) {
        let group = self.groups.get_mut(&g).expect("the group takes commits");
        match (&group.label, group.latest) {
            (Some(first), Some(_)) => {
                let continued = format!("{first} (continued)").into();
                (continued, Some((g, first.clone())))
            }
            _ => {
                group.label = Some(label.clone());
                (label.clone(), Some((g, label)))
            }
        }
    }

    /// `parts`, a commit in group `g`, joined to the current version, the
    /// group's latest ([`amends`](Self::amends)): its steps added to the
    /// version's and coalesced, a new version in its place, under its
    /// label; or, when they put back all it did, none, and the cursor at
    /// its parent, as if the commits hadn't been made. The change is the
    /// commit's own, from the version it replaces.
    fn amend(&mut self, g: GroupId, parts: crate::tx::TxParts, mut commit: Commit) -> Commit {
        let actor = parts.actor().clone();
        let keys = keys_of(&parts.steps);
        let state = parts.state;
        let old = self.cursor;
        let mut v = self.take_version(old).expect("current version");
        let before = v.state.take().expect("the current version keeps its state");
        let parent = v.parent.expect("an amended version has a parent");
        let label: Arc<str> = v.label.as_str().into();
        join(&mut v, parts.steps, &keys, &state);
        commit.amended = true;
        if v.steps.iter().all(|s| s.prims.is_empty()) {
            // Back to the parent, whose state this is (trimming may have
            // dropped its snapshot). What the version held waits for the
            // next one; the group's latest is the one before it, if any.
            let p = self.versions.get_mut(&parent).expect("parent");
            p.children.retain(|c| *c != old);
            if p.redo_to == Some(old) {
                p.redo_to = None;
            }
            if p.state.is_none() {
                self.set_state(parent, Some(state));
            }
            self.cursor = parent;
            let latest = self.group_line(g).first().copied();
            self.groups
                .get_mut(&g)
                .expect("the group takes commits")
                .latest = latest;
        } else {
            v.state = Some(state);
            commit.version = Some(self.replace(old, v));
        }
        let (to, after) = (self.cursor, self.state());
        commit.change = ChangeSet::of(old, to, Cause::Amend, actor, label, keys, &before, after);
        self.trim();
        commit
    }

    /// Group `g`'s versions on the line from the opened version to the
    /// current one, the newest first.
    fn group_line(&self, g: GroupId) -> Vec<VersionId> {
        let mut ours = Vec::new();
        let mut at = Some(self.cursor);
        while let Some(id) = at {
            let v = &self.versions[&id];
            if v.group.as_ref().is_some_and(|(of, _)| *of == g) {
                ours.push(id);
            }
            at = v.parent;
        }
        ours
    }

    /// History's "Undo all of `label`" (§8.4): group `g`'s versions on the
    /// current one's line taken back as one version after it, the user's.
    /// What was edited between them stays. An edit since that changed what
    /// they wrote, or a rule what they'd put back would break, is a Conflict
    /// naming that edit; none of them on the line, or a group pruning took
    /// part of, is [`EditError::Invalid`]; and either changes nothing. Otherwise the group ends, so what a run still adds
    /// is never put back ([`EditError::GroupEnded`]).
    pub fn revert_group(&mut self, g: GroupId, label: &str) -> Result<Commit, EditError> {
        let commit = self.revert(g, label.into(), Cause::Revert, Actor::User)?;
        self.groups.remove(&g);
        Ok(commit)
    }

    /// Group `g`'s versions taken back after its call failed (§8.4): the
    /// change, by the group's actor, or None when it has none here. Its only
    /// version, when that's the current one and the group could still
    /// amend it, goes, with nothing to redo to it; otherwise the versions
    /// stay, a saved one among them, and a version after the current one
    /// takes them back, as [`revert_group`](Self::revert_group) does. A
    /// Conflict, or a group pruning took part of, keeps them all and
    /// changes nothing; otherwise the group ends.
    pub fn rollback_group(&mut self, g: GroupId) -> Result<Option<ChangeSet>, EditError> {
        if self.cut_groups.contains(&g) {
            let label = self.groups.get(&g).and_then(|group| group.label.clone());
            return Err(cut(label.as_deref().unwrap_or("the edits")));
        }
        let change = match self.group_line(g)[..] {
            [] => None,
            [only] if only == self.cursor && self.amends(g) => Some(self.truncate()?),
            [latest, ..] => {
                let v = &self.versions[&latest];
                let actor = v.actor.clone();
                let label = v.group.as_ref().expect("the group's").1.clone();
                let commit = self.revert(g, label, Cause::Rollback, actor)?;
                commit.version.map(|_| commit.change)
            }
        };
        self.groups.remove(&g);
        Ok(change)
    }

    /// The current version taken out of the tree, the cursor back at its
    /// parent with nothing to redo to it: a rolled-back group's only
    /// version, by its actor.
    fn truncate(&mut self) -> Result<ChangeSet, EditError> {
        let old = self.cursor;
        let parent = self.versions[&old]
            .parent
            .expect("a group's version has a parent");
        self.ensure_state(parent)?;
        let v = self.take_version(old).expect("current version");
        let p = self.versions.get_mut(&parent).expect("parent");
        p.children.retain(|c| *c != old);
        p.redo_to = None;
        self.cursor = parent;
        let before = v
            .state
            .as_ref()
            .expect("the current version keeps its state");
        let label = v.label.as_str().into();
        Ok(ChangeSet::of(
            old,
            parent,
            Cause::Rollback,
            v.actor,
            label,
            v.keys,
            before,
            self.state(),
        ))
    }

    /// Group `g`'s versions on the current one's line taken back as one
    /// version after it, labelled "Undo all of `label`" and made by
    /// `actor`, its change's `cause` saying why: each one's prims with
    /// before and after swapped, the newest version's first, each landed
    /// on the current version by value, as a stale transaction's are, so
    /// any edit made since that changed what they wrote is a Conflict
    /// naming it. Then what they put back must leave the cascades nothing
    /// to do and keep the rules of uniqueness and reference
    /// ([`merge`](Self::merge)), as on the state before the group. None of
    /// it on the line is [`EditError::Invalid`], and so is a group pruning
    /// took a version of, or the prims of the first version kept: the
    /// versions left are only part of it. Nothing changes when it fails.
    fn revert(
        &mut self,
        g: GroupId,
        label: Arc<str>,
        cause: Cause,
        actor: Actor,
    ) -> Result<Commit, EditError> {
        if self.cut_groups.contains(&g) {
            return Err(cut(&label));
        }
        let ours = self.group_line(g);
        let Some(&first) = ours.last() else {
            return Err(EditError::invalid(
                "revert",
                format!("“{label}” isn't in this version"),
            ));
        };
        let made = self.versions[&first].parent.expect("not the first version");
        let mut rec = crate::prim::Rec::new(self.state().clone());
        for id in &ours {
            let steps = self.versions[id].steps.iter().rev();
            for p in steps.flat_map(|s| s.prims.iter().rev()) {
                if !same(rec.get(&p.key).as_ref(), p.after.as_ref()) {
                    return Err(self.conflict(p.key, made, Some(g)));
                }
                rec.put(p.key, p.before.clone());
            }
        }
        let (_, prims) = rec.finish();
        let undo = format!("Undo all of {label}");
        if prims.is_empty() {
            let change = ChangeSet::empty(self.cursor, cause, actor, undo.into());
            return Ok(reverted(None, change, &[]));
        }
        // What they put back held there, so a name it held twice already
        // isn't a clash.
        self.ensure_state(made)?;
        let theirs = self.versions[&made].state.clone().expect("ensured");
        let step = Step {
            op: Arc::new(ResolvedOp::Revert { label }),
            prims: prims.into(),
            effects: StepEffects::default(),
            line: String::new(),
            cascades: Arc::new([]),
        };
        let (state, steps) = self.merge(vec![step], made, Some(g), &theirs)?;
        debug_assert_eq!(state.check_invariants(), Ok(()), "{undo}");
        let (from, before) = (self.cursor, self.state().clone());
        self.push(state, undo.clone(), steps, actor, None);
        let change = self.made(from, &before, cause, undo.into());
        let steps = &self.versions[&self.cursor].steps;
        Ok(reverted(Some(self.cursor), change, steps))
    }

    /// `tx`, ready to commit at the cursor, where it lands as `opts`
    /// says when it was made on another version ([`onto_cursor`]): its
    /// steps coalesced, and what the commit says of them, unencodable text
    /// told once a document, labelled as `opts` says, or as its first line.
    /// Its change is none yet, as a commit that changes nothing has.
    ///
    /// [`onto_cursor`]: Self::onto_cursor
    fn settle(
        &mut self,
        tx: Tx,
        opts: CommitOpts,
    ) -> Result<(crate::tx::TxParts, Commit), EditError> {
        if !self.ready {
            return Err(EditError::NotReady);
        }
        let mut parts = self.onto_cursor(tx.into_parts(), opts.base, opts.stale)?;
        // One step's prims are its own already.
        if parts.steps.len() > 1 {
            coalesce(&mut parts.steps);
        }
        // Text the built-in fonts can't draw is told once a document.
        let mut notices = std::mem::take(&mut parts.notices);
        let mut tell = !self.told_unencodable;
        notices.retain(|n| *n != Notice::Unencodable || std::mem::take(&mut tell));
        self.told_unencodable |= notices.contains(&Notice::Unencodable);
        let lines = crate::describe::lines(&parts.steps);
        let label = opts.label.unwrap_or_else(|| {
            (lines.first()).map_or_else(Arc::default, |l| l.text.as_str().into())
        });
        let commit = Commit {
            version: None,
            amended: false,
            created: std::mem::take(&mut parts.created),
            change: ChangeSet {
                depth: opts.depth,
                ..ChangeSet::empty(self.cursor, Cause::Commit, parts.actor().clone(), label)
            },
            effects: StepEffects::union(
                &parts.steps.iter().map(|s| &s.effects).collect::<Vec<_>>(),
            ),
            lines,
            cascades: crate::tx::cascade::merged(
                parts.steps.iter().flat_map(|s| s.cascades.iter()),
            ),
            notices,
        };
        debug_assert_eq!(parts.state.check_invariants(), Ok(()), "{:?}", commit.lines);
        Ok((parts, commit))
    }

    /// Version `id`'s state, rebuilt from the nearest ancestor that kept
    /// one by applying the prims of each version on the way. A mismatch
    /// when one of them is damaged already, or when a prim doesn't fit.
    fn replay(&self, id: VersionId) -> Result<DocState, EditError> {
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
                return Err(EditError::Mismatch { version });
            }
            for p in v.steps.iter().flat_map(|s| s.prims.iter()) {
                if let Err(m) = state.apply(p) {
                    log::error!("version {}: {m}", version.0);
                    return Err(EditError::Mismatch { version });
                }
            }
        }
        Ok(state)
    }

    /// Make sure version `id` has its state, replaying it when trimming
    /// dropped it. A version that can't be rebuilt is marked damaged; that
    /// is a bug, so a debug build stops there.
    fn ensure_state(&mut self, id: VersionId) -> Result<(), EditError> {
        if self.versions[&id].state.is_some() {
            return Ok(());
        }
        match self.replay(id) {
            Ok(state) => {
                self.set_state(id, Some(state));
                Ok(())
            }
            Err(e) => {
                if let EditError::Mismatch { version } = e {
                    self.versions.get_mut(&version).expect("version").damaged = true;
                }
                log::error!("{e}");
                if cfg!(debug_assertions) {
                    panic!("{e}");
                }
                Err(e)
            }
        }
    }

    /// A new version after the current one, made by `actor` with the ops
    /// that made it, its `group`'s latest if it's of one. The versions that
    /// were after the current one stay, as a branch.
    fn push(
        &mut self,
        state: DocState,
        label: String,
        steps: Vec<Step>,
        actor: Actor,
        group: Option<(GroupId, Arc<str>)>,
    ) {
        let id = VersionId(self.next_version);
        self.next_version += 1;
        let parent = self.cursor;
        let keys = keys_of(&steps);
        let (snapshot_bytes, step_bytes) = accounting(&steps, &keys, &state);
        let blobs = brought(&steps);
        let author = match &actor {
            Actor::Plugin { id, .. } => id.to_string(),
            _ => self.identity.person.to_string(),
        };
        let p = self.versions.get_mut(&parent).expect("parent");
        p.children.push(id);
        p.redo_to = Some(id);
        if let Some((g, _)) = &group
            && let Some(group) = self.groups.get_mut(g)
        {
            group.latest = Some(id);
        }
        self.insert_version(
            id,
            Version {
                parent: Some(parent),
                children: Vec::new(),
                redo_to: None,
                label,
                actor,
                author,
                at: crate::annot::pdf_date_now(),
                steps,
                keys,
                blobs,
                state: Some(state),
                damaged: false,
                on_line: true,
                snapshot_bytes,
                step_bytes,
                group,
                sealed: 0,
                record: None,
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
    /// resolves. Its record, if it has one, says it's the version `id`.
    fn replace(&mut self, old: VersionId, mut v: Version) -> VersionId {
        let id = VersionId(self.next_version);
        self.next_version += 1;
        if let Some(rec) = &mut v.record {
            Arc::make_mut(rec).version = id;
        }
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
        if self.saved_at() == Some(old) {
            self.saved = Some(SavedVersion::At(id));
        }
        if let Some((g, _)) = &v.group
            && let Some(group) = self.groups.get_mut(g)
            && group.latest == Some(old)
        {
            group.latest = Some(id);
        }
        self.insert_version(id, v);
        id
    }

    /// `v` in the tree as version `id`, and what it holds counted.
    fn insert_version(&mut self, id: VersionId, v: Version) {
        let held = &mut self.held;
        if v.state.is_some() {
            held.snapshots.insert(id);
            held.snapshot_bytes += v.snapshot_bytes;
        }
        held.step_bytes += v.step_bytes;
        if !v.blobs.is_empty() {
            held.undone = None;
        }
        self.versions.insert(id, v);
    }

    /// Version `id` out of the tree, if it's there, and what it holds no
    /// longer counted.
    fn take_version(&mut self, id: VersionId) -> Option<Version> {
        let v = self.versions.remove(&id)?;
        let held = &mut self.held;
        if v.state.is_some() {
            held.snapshots.remove(&id);
            held.snapshot_bytes -= v.snapshot_bytes;
        }
        held.step_bytes -= v.step_bytes;
        if !v.blobs.is_empty() {
            held.undone = None;
        }
        Some(v)
    }

    /// Version `id`'s snapshot kept, or dropped with None, and counted so.
    fn set_state(&mut self, id: VersionId, state: Option<DocState>) {
        let v = self.versions.get_mut(&id).expect("version");
        let held = &mut self.held;
        match (v.state.is_some(), state.is_some()) {
            (false, true) => {
                held.snapshots.insert(id);
                held.snapshot_bytes += v.snapshot_bytes;
            }
            (true, false) => {
                held.snapshots.remove(&id);
                held.snapshot_bytes -= v.snapshot_bytes;
            }
            _ => {}
        }
        v.state = state;
    }

    /// For tests: the policy history is kept to from now on.
    #[cfg(test)]
    pub(crate) fn set_policy(&mut self, policy: Box<dyn HistoryPolicy>) {
        self.policy = policy;
    }

    /// Versions that must keep their snapshots: the first one kept (every
    /// replay starts somewhere), the current one, one being saved (its
    /// save reads it) and the saved one while the history holds it (the
    /// next save's saved side). A save changes no state, so every other
    /// version replays as any does. `v` is version `id`.
    fn keeps_snapshot(&self, id: VersionId, v: &Version) -> bool {
        id == self.root || id == self.cursor || self.saved_at() == Some(id) || v.sealed > 0
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

    /// Keep history to the policy and to [`MAX_VERSIONS`] (M10.5 §8.7):
    /// versions pruned one at a time while there are too many or the
    /// policy says ([`prune_one`](Self::prune_one)), then snapshots dropped
    /// as it says. History is weighed only when the policy could act on
    /// what it holds ([`Held`]), so an edit doesn't walk it.
    fn trim(&mut self) {
        loop {
            let weighed = self.weighed();
            let over = self.versions.len() > MAX_VERSIONS
                || (weighed.as_ref()).is_some_and(|(_, stats)| self.policy.prunes(stats));
            if over && self.prune_one() {
                continue;
            }
            if let Some((droppable, stats)) = weighed {
                let n = self.policy.trim(&stats).min(droppable.len());
                for &id in &droppable[..n] {
                    self.set_state(id, None);
                }
            }
            return;
        }
    }

    /// The snapshots the policy may drop, in the order to drop them, and
    /// the stats it weighs; none when it couldn't act on what history
    /// holds.
    fn weighed(&mut self) -> Option<(Vec<VersionId>, HistoryStats)> {
        let undone = self.undone_bytes();
        let Held {
            snapshots,
            snapshot_bytes,
            step_bytes,
            ..
        } = &self.held;
        let bytes = snapshot_bytes + step_bytes + undone;
        if !self.policy.could_act(snapshots.len(), bytes) {
            return None;
        }
        Some(self.weigh(undone))
    }

    /// One version out of the tree, as pruning takes them: the oldest leaf
    /// off the current line, unless it's the saved version or one being
    /// saved; else the first version, the next on the line in its place
    /// ([`advance_root`](Self::advance_root)). Whether one went.
    fn prune_one(&mut self) -> bool {
        let leaf = self
            .versions
            .iter()
            .filter(|(id, v)| v.children.is_empty() && !v.on_line && !self.protected(**id))
            .map(|(id, _)| *id)
            .min();
        match leaf {
            Some(leaf) => self.remove_version(leaf),
            None => self.advance_root(),
        }
    }

    /// The first version out of the tree, the next on the current line the
    /// first from now on: never past the current version or one being
    /// saved, nor while a branch off it holds one being saved. The other
    /// branches off it go with it. The saved version may go, the first
    /// version or on one of them: then it detaches
    /// ([`detach`](Self::detach)), and once it has, its anchor takes the
    /// keys of the edge into each new first version. The new first version
    /// takes the sources of the one before, whose state was made over
    /// them, and keeps its steps' edits and lines, but not their prims:
    /// nothing is rebuilt into it, so they would only hold what it
    /// replaced. A group whose latest it passes has none kept, and one it
    /// takes a version or prims of can't be undone all at once. Whether it
    /// moved.
    fn advance_root(&mut self) -> bool {
        let old = self.root;
        let first = &self.versions[&old];
        let next = first.children.iter().find(|c| self.versions[c].on_line);
        let Some(&next) = next.filter(|_| first.sealed == 0) else {
            return false;
        };
        let branches: Vec<VersionId> = (first.children.iter())
            .filter(|&&c| c != next)
            .flat_map(|&c| self.subtree(c))
            .collect();
        if branches.iter().any(|b| self.versions[b].sealed > 0) {
            return false;
        }
        if self.ensure_state(next).is_err() {
            return false;
        }
        match self.saved {
            Some(SavedVersion::At(s)) if s == old || branches.contains(&s) => self.detach(s, next),
            Some(SavedVersion::Detached(ref mut anchor)) => {
                let keys = self.versions[&next].keys.iter().copied();
                anchor.keys_to_root.extend(keys);
            }
            _ => {}
        }
        for b in branches {
            self.remove_version(b);
        }
        let gone = self.take_version(old).expect("the first version");
        // A group whose latest it was has none kept now.
        if let Some((g, _)) = gone.group
            && let Some(group) = self.groups.get_mut(&g)
            && group.latest == Some(old)
        {
            group.latest = None;
        }
        let mut root = self.take_version(next).expect("the next version");
        let cut = [&gone, &root].map(|v| v.group.as_ref().map(|(g, _)| *g));
        self.cut_groups.extend(cut.into_iter().flatten());
        root.parent = None;
        root.blobs.extend(gone.blobs);
        for step in &mut root.steps {
            step.prims = Arc::new([]);
        }
        root.step_bytes = steps_bytes(&root.steps, &root.keys);
        self.insert_version(next, root);
        self.root = next;
        true
    }

    /// The saved version `s`, which pruning takes, kept as an anchor: its
    /// state and record, and the keys on the tree path from it to version
    /// `next`, the first from now on. One made in memory has no record:
    /// no file holds the document, and nothing is kept.
    fn detach(&mut self, s: VersionId, next: VersionId) {
        let keys_to_root = BTreeSet::clone(&self.keys_along(&self.edges_between(s, next)));
        let v = &self.versions[&s];
        self.saved = v.record.clone().map(|record| {
            let state = v.state.clone().expect("the saved version keeps its state");
            SavedVersion::Detached(Box::new(Anchor {
                state,
                record,
                keys_to_root,
            }))
        });
    }

    /// Version `id` and every version after it.
    fn subtree(&self, id: VersionId) -> Vec<VersionId> {
        let mut out = vec![id];
        let mut i = 0;
        while let Some(&v) = out.get(i) {
            out.extend(self.versions[&v].children.iter().copied());
            i += 1;
        }
        out
    }

    /// Whether version `id` must stay in the tree: it's the saved version,
    /// or a save of it is in flight.
    fn protected(&self, id: VersionId) -> bool {
        self.saved_at() == Some(id) || self.versions.get(&id).is_some_and(|v| v.sealed > 0)
    }

    /// Take version `id`, off the current line, out of the tree: what only
    /// it held goes with it. Not the saved version, or one being saved. A
    /// group whose latest it was ends: its edits were undone. Whether it
    /// went.
    fn remove_version(&mut self, id: VersionId) -> bool {
        if self.protected(id) {
            return false;
        }
        let Some(v) = self.take_version(id) else {
            return false;
        };
        if let Some(p) = v.parent
            && let Some(parent) = self.versions.get_mut(&p)
        {
            parent.children.retain(|c| *c != id);
            if parent.redo_to == Some(id) {
                parent.redo_to = None;
            }
        }
        if let Some((g, _)) = v.group
            && self
                .groups
                .get(&g)
                .is_some_and(|group| group.latest == Some(id))
        {
            self.groups.remove(&g);
        }
        true
    }

    /// Snapshots the policy may drop, in the order to drop them, and
    /// history's stats, `undone` the bytes of the sources only versions off
    /// the current line bring. Versions off the current line go first
    /// (other branches, undone ones), then the line's, oldest first; then
    /// the snapshots that must stay, the current version's last. Each
    /// version keeping a snapshot is looked up once: the default policy
    /// weighs them at every edit once it holds its count.
    fn weigh(&self, undone: usize) -> (Vec<VersionId>, HistoryStats) {
        let (mut off, mut on, mut kept) = (Vec::new(), Vec::new(), Vec::new());
        let mut cursor = None;
        for &id in &self.held.snapshots {
            let v = &self.versions[&id];
            let held = (id, v.snapshot_bytes);
            if id == self.cursor {
                cursor = Some(v.snapshot_bytes);
            } else if self.keeps_snapshot(id, v) {
                kept.push(v.snapshot_bytes);
            } else if v.on_line {
                on.push(held);
            } else {
                off.push(held);
            }
        }
        let droppable: Vec<VersionId> = off.iter().chain(&on).map(|&(id, _)| id).collect();
        let snapshot_bytes: Vec<usize> = (off.iter().chain(&on))
            .map(|&(_, bytes)| bytes)
            .chain(kept)
            .chain(cursor)
            .collect();
        let stats = HistoryStats {
            versions: snapshot_bytes.len(),
            cursor: snapshot_bytes.len() - 1,
            droppable: droppable.len(),
            snapshot_bytes,
            step_bytes: self.held.step_bytes,
            blob_bytes: self.store.bytes(),
            undone_source_bytes: undone,
        };
        (droppable, stats)
    }

    /// [`undone_source_bytes`](Self::undone_source_bytes), counted again
    /// only when what it counts may have changed ([`Held::undone`]).
    fn undone_bytes(&mut self) -> usize {
        if let Some(bytes) = self.held.undone {
            return bytes;
        }
        let bytes = self.undone_source_bytes();
        self.held.undone = Some(bytes);
        bytes
    }

    /// The bytes of the sources only versions off the current line bring
    /// (inserts that were undone), each blob once. A source a version on
    /// the line brings, or the primary, isn't counted: pruning wouldn't
    /// free it. Each version noted its sources as it was made, so this
    /// never walks a step.
    fn undone_source_bytes(&self) -> usize {
        let mut kept = HashSet::from([self.primary.id()]);
        let mut undone = HashSet::new();
        for v in self.versions.values() {
            match v.on_line {
                true => kept.extend(v.blobs.iter().copied()),
                false => undone.extend(v.blobs.iter().copied()),
            }
        }
        (undone.difference(&kept))
            .map(|blob| blob.len as usize)
            .sum()
    }

    /// Snapshots kept and their estimated bytes (droppable ones first), the
    /// bytes every version's steps hold, and the sources' bytes.
    pub fn stats(&self) -> HistoryStats {
        let undone = (self.held.undone).unwrap_or_else(|| self.undone_source_bytes());
        self.weigh(undone).1
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
            actor: v.actor.clone(),
            author: v.author.clone(),
            at: v.at.clone(),
            edits: crate::describe::lines(&v.steps),
            saves: v.saves.clone(),
            current: id == self.cursor,
            saved: Some(id) == self.on_disk(),
            undone,
            branches: v
                .children
                .iter()
                .copied()
                .filter(|c| Some(*c) != shown_child)
                .collect(),
            replays: v.state.is_none(),
            damaged: v.damaged,
            group: (v.group.clone()).filter(|(g, _)| !self.cut_groups.contains(g)),
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
}

/// `steps`, a commit's in version `v`'s group, which changed `keys` and
/// made `state`, added to `v`'s: coalesced with them, and what they all
/// hold, change and bring counted again. When they change none of the
/// slots `v`'s steps do, they're added as they are and only what they hold
/// is counted, so a version of thousands of steps (an OCR run's flushes)
/// takes one more in the time its own take.
fn join(v: &mut Version, steps: Vec<Step>, keys: &BTreeSet<Key>, state: &DocState) {
    if keys.is_disjoint(&v.keys) {
        let was = keys_bytes(&v.steps, &v.keys);
        let mut arcs = HashSet::new();
        let held: usize = steps.iter().map(|s| step_bytes(s, &mut arcs)).sum();
        v.blobs.extend(brought(&steps));
        v.steps.extend(steps);
        Arc::make_mut(&mut v.keys).extend(keys.iter().copied());
        v.step_bytes = v.step_bytes - was + held + keys_bytes(&v.steps, &v.keys);
        v.snapshot_bytes = state.snapshot_bytes(v.keys.iter().copied());
    } else {
        v.steps.extend(steps);
        coalesce(&mut v.steps);
        v.keys = keys_of(&v.steps);
        (v.snapshot_bytes, v.step_bytes) = accounting(&v.steps, &v.keys, state);
        v.blobs = brought(&v.steps);
    }
}

/// A revert's commit: of `steps`, which made `version`, or none.
/// Why the edits labelled `label`, a group pruning took part of, can't be
/// taken back all at once.
fn cut(label: &str) -> EditError {
    EditError::invalid(
        "revert",
        format!("“{label}” is older than the history kept"),
    )
}

fn reverted(version: Option<VersionId>, change: ChangeSet, steps: &[Step]) -> Commit {
    Commit {
        version,
        amended: false,
        created: crate::tx::Created::default(),
        change,
        effects: StepEffects::union(&steps.iter().map(|s| &s.effects).collect::<Vec<_>>()),
        lines: crate::describe::lines(steps),
        cascades: Vec::new(),
        notices: Vec::new(),
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

/// The slots `steps`, coalesced, change: their prims' keys, each on one
/// step. One step's are the keys its effects hold, which are its prims'
/// as it was lowered: shared, not copied.
fn keys_of(steps: &[Step]) -> Arc<BTreeSet<Key>> {
    match steps {
        [one] => {
            let prims = one.prims.iter().map(|p| p.key);
            debug_assert!(prims.eq(one.effects.keys.iter().copied()), "{}", one.line);
            one.effects.keys.clone()
        }
        _ => Arc::new(
            (steps.iter())
                .flat_map(|s| s.prims.iter().map(|p| p.key))
                .collect(),
        ),
    }
}

/// The sources `steps` bring pages of, by blob: what a page they put
/// shows when it showed another source before, or nothing (an insert's
/// file).
fn brought(steps: &[Step]) -> HashSet<BlobId> {
    let src = |val: &Option<Val>| match val {
        Some(Val::Page(def)) => Some(def.base.src.id()),
        _ => None,
    };
    (steps.iter())
        .flat_map(|s| s.prims.iter())
        .filter_map(|p| src(&p.after).filter(|&now| src(&p.before) != Some(now)))
        .collect()
}

/// What a `BTreeSet` of `set`'s size holds: its elements, in nodes of up
/// to eleven with a parent pointer and two counts. Built from an iterator,
/// as a step's are, its nodes are full.
fn set_bytes<T>(set: &BTreeSet<T>) -> usize {
    set.len().div_ceil(11) * (11 * size_of::<T>() + 16)
}

/// What a version holds that no other does (M10.5 §3.9), from the steps
/// that made it and its state: its snapshot's bytes, which trimming frees,
/// and its steps' ([`steps_bytes`]), which only pruning the version frees.
/// The values are the snapshot's too, so the snapshot adds only the im
/// nodes it copied to put them ([`DocState::snapshot_bytes`]).
fn accounting(steps: &[Step], keys: &Arc<BTreeSet<Key>>, state: &DocState) -> (usize, usize) {
    (
        state.snapshot_bytes(keys.iter().copied()),
        steps_bytes(steps, keys),
    )
}

/// What a version's `steps` hold ([`step_bytes`]), and its `keys`
/// ([`keys_bytes`]).
fn steps_bytes(steps: &[Step], keys: &Arc<BTreeSet<Key>>) -> usize {
    let mut arcs = HashSet::new();
    let held = steps
        .iter()
        .map(|s| step_bytes(s, &mut arcs))
        .sum::<usize>();
    held + keys_bytes(steps, keys)
}

/// What a version's `keys` hold: nothing beyond its `steps` when they're
/// its one step's.
fn keys_bytes(steps: &[Step], keys: &Arc<BTreeSet<Key>>) -> usize {
    match steps {
        [one] if Arc::ptr_eq(&one.effects.keys, keys) => 0,
        _ => 2 * size_of::<usize>() + set_bytes(keys),
    }
}

/// What step `s` holds that no other version's steps do: its op, the sets
/// of what it did and its cascades, an `Arc` once each, and what its prims
/// put, the `Arc`s it counted noted in `arcs`, which it clears first.
/// Counted a step at a time, so a commit that joins a group's version adds
/// its own without counting the version's again.
///
/// A version's before-values are its parent's after-values, so each value
/// is counted once, by the step that made it, and an `Arc` once however
/// many of the step's prims hold it: a page's definition, its boxes, its
/// content stack (a duplicate shares its original's), an annotation's
/// body, another tool's annotation's summary and a widget's props (their
/// copies share them), a field node and its terminal's props (unless the
/// node it replaces has them), a bookmark, the labels, the form, a
/// generator. A source's bytes and a blob's aren't a value's: the store
/// counts them. An `Arc` two steps of a version hold counts in each, and
/// the order, a page's annotations, the top-level bookmarks and the form
/// count in full, though im shares all but the changed paths of the order,
/// the top-level bookmarks and the form's roots and names with the
/// parent's: over-counts.
fn step_bytes(s: &Step, arcs: &mut HashSet<*const ()>) -> usize {
    arcs.clear();
    let mut once = |arc: *const ()| arcs.insert(arc);
    // A step's own `Arc`s, which lowering made for it alone: each one's
    // counts and what it holds.
    let arc = 2 * size_of::<usize>();
    let StepEffects { pages, keys, .. } = &s.effects;
    let mut bytes = size_of::<Step>()
        + s.line.heap_size()
        + (arc + size_of::<ResolvedOp>() + s.op.heap_size())
        + (arc + set_bytes(pages))
        + (arc + set_bytes(keys))
        + (arc + size_of_val(&*s.cascades));
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
                Some(Val::Labels(labels)) if once(Arc::as_ptr(labels).cast()) => labels.heap_size(),
                _ => 0,
            },
            Key::Generator(_) => match &p.after {
                Some(Val::Generator(g)) if once(Arc::as_ptr(g).cast()) => {
                    size_of::<Generator>() + g.heap_size()
                }
                _ => 0,
            },
        };
        bytes += size_of::<Prim>() + after;
    }
    bytes
}

// ----- outline helpers: bookmarks are records, their kids by id ----------

/// Bookmarks nested by depth, with new ids: the top-level ones' ids in
/// order, and every one in pre-order.
pub(crate) type Nested = (Vector<OutlineId>, Vec<(OutlineId, Bookmark)>);

/// Bookmarks of a file's items, as [`crate::open::outline`] read them
/// from `src`, each with a new id from `new_id`: `page` gives the page
/// that shows each page of the file, and an item going to one none shows
/// goes nowhere.
pub(crate) fn outline_from_flat(
    new_id: impl FnMut() -> OutlineId,
    items: &[FlatOutlineItem],
    page: impl Fn(u32) -> Option<PageId>,
    src: &Src,
) -> Nested {
    nest(
        new_id,
        items.iter().map(|item| {
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
        }),
    )
}

/// `items`, bookmarks in pre-order with their depths, under one another,
/// each with a new id from `new_id`: one deeper than the one before is its
/// kid, and one as deep or less a sibling of the last one less deep.
fn nest(
    mut new_id: impl FnMut() -> OutlineId,
    items: impl IntoIterator<Item = (u16, Bookmark)>,
) -> Nested {
    let mut roots = Vector::new();
    let mut out: Vec<(OutlineId, Bookmark)> = Vec::new();
    // The path down to the last one: each one's depth and place in `out`.
    let mut path: Vec<(u16, usize)> = Vec::new();
    for (depth, b) in items {
        while path.last().is_some_and(|&(d, _)| d >= depth) {
            path.pop();
        }
        let id = new_id();
        match path.last() {
            Some(&(_, parent)) => out[parent].1.kids.push_back(id),
            None => roots.push_back(id),
        }
        path.push((depth, out.len()));
        out.push((id, b));
    }
    (roots, out)
}

// ----- annotation helpers: records, each page's by id ---------------------

/// Annotation `a` of a source page as a record of a page showing copy
/// `copy` of it, under the id the source's fact gives it: the object it
/// is in `src`, as a mark is only on copy 0, and a mark holding what it
/// holds there, its file decoded into `store`.
fn record_of(store: &Store, a: &FileAnnot, src: &Src, copy: u32) -> (AnnotId, AnnotBody) {
    let at = |id| ObjRef {
        src: src.clone(),
        id,
    };
    let object = a.object.map(at);
    let body = match &a.body {
        FileBody::Mark { annot, held } => {
            let mark = Mark {
                src: object.filter(|_| copy == 0),
                ..Mark::from(Annot::clone(annot))
            };
            AnnotBody::Mark(Arc::new(holding(mark, store, held.map(at))))
        }
        FileBody::Foreign { summary, .. } => AnnotBody::Foreign(Arc::new(Foreign {
            src: object,
            copy,
            summary: summary.clone(),
        })),
    };
    (a.id(), body)
}

/// `mark`, read from a source where it holds `held` (see
/// [`FileBody::Mark`]), with what that is: its page stamp's form, or its
/// attachment's file decoded into `store`. A file that can't be read is
/// said, and the mark holds none.
fn holding(mut mark: Mark, store: &Store, held: Option<ObjRef>) -> Mark {
    let Some(object) = held else {
        return mark;
    };
    match &mark.annot.kind {
        AnnotKind::PageStamp { .. } => {
            mark.stamp = Some(FormRef::Base(object));
        }
        AnnotKind::FileAttachment { name, size, .. } => {
            mark.file = file_in(store, &object, *size)
                .inspect_err(|e| log::warn!("attachment {name}: {e:#}"))
                .ok();
        }
        _ => {}
    }
    mark
}

/// [`Document::find_stamp`] in a document whose blobs are in `store` and
/// which opened on `primary`: a snapshot's, too. What lowering finds a
/// stamp by.
pub(crate) fn find_stamp(store: &Store, primary: &Src, stem: &str) -> Option<FormRef> {
    let named = || store.get(BlobId::from_stem(stem)?);
    let held = || {
        parsed(store, primary).find_map(|src| {
            let id = src.annots().stamp(stem)?;
            Some(FormRef::Base(ObjRef {
                src: src.clone(),
                id,
            }))
        })
    };
    (named().map(|pdf| FormRef::Blob(pdf.into()))).or_else(held)
}

/// [`Document::find_file`] in a document whose blobs are in `store` and
/// which opened on `primary`: a snapshot's, too.
pub(crate) fn find_file(store: &Store, primary: &Src, stem: &str, size: u64) -> Option<BlobRef> {
    if let Some(file) = BlobId::from_stem(stem).and_then(|id| store.get(id)) {
        return Some(file.into());
    }
    parsed(store, primary).find_map(|src| {
        let id = src.annots().file(stem)?;
        let file = file_in(store, &ObjRef { src, id }, size);
        file.inspect_err(|e| log::warn!("attachment {stem}: {e:#}"))
            .ok()
    })
}

/// The sources `store` has parsed, `primary` first and then by id: where
/// a stem is looked for, with no walk of the document's pages.
fn parsed(store: &Store, primary: &Src) -> impl Iterator<Item = Src> {
    let id = primary.id();
    let others = (store.sources().into_iter()).filter(move |src| src.id() != id);
    std::iter::once(primary.clone()).chain(others)
}

/// Embedded file `stream`, which an attachment mark saying its file is
/// `size` bytes holds, decoded into `store`.
pub(crate) fn file_in(store: &Store, stream: &ObjRef, size: u64) -> Result<BlobRef> {
    use crate::content::ReadObjects;
    let bytes = stream
        .src
        .pdf()
        .read(stream.id, |o| match o {
            lopdf::Object::Stream(s) => crate::attachments::mark_file_bytes(s, size),
            _ => bail!("the embedded file isn't a stream"),
        })
        .context("reading the embedded file")??;
    Ok(store.intern(bytes.into(), BlobKind::Attachment)?.into())
}

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

#[cfg(test)]
mod change_tests;
#[cfg(test)]
mod group_tests;
#[cfg(test)]
pub(crate) mod history_tests;

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use lopdf::ObjectId;
    use sheaf_edit::{
        AnnotRef, BookmarkPatch, BookmarkRef, HeaderFooterSpec, LabelStyle, Local, Op, PageRef,
        PageSel, Patch, Sibling, Turn,
    };

    use super::*;
    use crate::history::{AllOf, FixedCount, MemoryBudget};
    use crate::materialize::{How, StrategyKind};
    use crate::open::FileAnnots;
    use crate::text_layer::TextLayer;
    use crate::tx::fixture::{self as fx, ids};

    /// A document of `n` blank pages, in a store of its own.
    pub(super) fn new_doc(
        n: u32,
        outline: &[FlatOutlineItem],
        policy: Box<dyn HistoryPolicy>,
    ) -> Document {
        let store = Store::new();
        let primary = crate::store::blank(&store, n);
        Document::new(store, primary, "a.pdf".into(), None, outline, policy)
    }

    /// `n` blank pages opened as a file opens, saying `facts`: started on
    /// its pages, then ready.
    pub(super) fn opened_doc(n: u32, facts: Facts, policy: Box<dyn HistoryPolicy>) -> Document {
        let store = Store::new();
        let primary = crate::store::blank(&store, n);
        let mut d = crate::open::start(store, primary, "a.pdf".into(), None, policy);
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

    pub(super) fn doc() -> Document {
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

    pub(super) fn id_at(d: &Document, i: usize) -> PageId {
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

    /// The parents a commit's forms went under, renamed apart.
    fn renamed_under(c: &Commit) -> Vec<String> {
        (c.notices.iter())
            .filter_map(|n| match n {
                Notice::FormRenamed { under, .. } | Notice::CopiesRenamed { under, .. } => {
                    Some(under.clone())
                }
                _ => None,
            })
            .collect()
    }

    /// Whether `d`'s scripts are off: they came from more than one form.
    fn scripts_off(d: &Document) -> bool {
        d.state().form().scripts != ScriptState::On
    }

    /// While the file opens, every edit is refused before it draws an id
    /// or makes a version; once it's ready, the same edits are taken.
    #[test]
    fn an_edit_before_ready_is_not_ready() {
        let store = Store::new();
        let primary = crate::store::blank(&store, 3);
        let mut d = crate::open::start(
            store,
            primary,
            "a.pdf".into(),
            None,
            Box::new(FixedCount(10)),
        );
        assert!(!d.is_ready());
        let page = id_at(&d, 0);
        let duplicate = || Op::DuplicatePages {
            pages: ids(&[page]),
            locals: Vec::new(),
        };
        let bookmark = || fx::bookmark(None, None, "One", Some(page));
        let drawn = |d: &Document| d.ids.next();
        let before = (d.current_version(), d.next_version, drawn(&d));
        let g = d.begin_group(GroupKind::PluginCall {
            plugin: "bookmarks".into(),
        });
        let refused = [
            d.commit_ops(vec![duplicate()], "Duplicate").unwrap_err(),
            d.commit_ops(vec![duplicate(), bookmark()], "Both")
                .unwrap_err(),
            d.commit_ops_in_group(g, vec![bookmark()], "Bookmark")
                .unwrap_err(),
        ];
        for e in &refused {
            assert_eq!(*e, EditError::NotReady);
        }
        assert_eq!((d.current_version(), d.next_version, drawn(&d)), before);
        assert_eq!(d.versions.len(), 1);
        d.ready(Facts::default()).unwrap();
        d.commit_ops(vec![duplicate()], "Duplicate").unwrap();
        d.commit_ops(vec![duplicate(), bookmark()], "Both").unwrap();
        d.commit_ops_in_group(g, vec![bookmark()], "Bookmark")
            .unwrap();
        assert_eq!((d.page_count(), d.state().outline_roots().len()), (5, 2));
        assert_eq!(d.versions.len(), 4);
    }

    /// `ready` puts the complete opened version in place of the one with
    /// the pages alone: the facts are on the pages the document showed
    /// while it opened, and there's nothing to save or undo. Its change
    /// says anything may have changed. It's taken once.
    #[test]
    fn ready_replaces_the_opened_version() {
        let store = Store::new();
        let bytes = crate::forms::fixture::every_fact().bytes();
        let primary = file(&store, bytes, "facts.pdf");
        let src = primary.clone();
        let mut d = crate::open::start(
            store,
            primary,
            "facts.pdf".into(),
            None,
            Box::new(FixedCount(10)),
        );
        let loading = d.current_version();
        let pages: Vec<PageId> = d.state().order().iter().copied().collect();
        let st = d.state();
        assert_eq!(pages.len(), 3);
        assert!(st.bookmark_count() == 0 && st.annot_count() == 0 && st.field_count() == 0);
        assert!(st.labels().is_none() && st.header_footer().is_none());
        let (view, change) = d
            .ready(crate::open::finish(&src, &in_progress::NoProgress))
            .unwrap();
        assert!(d.is_ready());
        let opened = d.current_version();
        assert_ne!(opened, loading);
        assert_eq!((change.from, change.to), (loading, opened));
        assert_eq!((change.cause, &change.actor), (Cause::Opened, &Actor::User));
        let (order, outline, labels) = (change.order, change.outline, change.labels);
        assert!(order && outline && labels && change.form && change.generators);
        let all = BTreeSet::from_iter(pages.iter().copied());
        for set in [&change.geometry, &change.content, &change.annots] {
            assert_eq!(**set, all);
        }
        assert_eq!(change.fields.len(), d.state().field_count());
        assert!(change.pages_added.is_empty() && !change.is_empty());
        assert!(!d.versions.contains_key(&loading));
        assert!(d.jump(loading).is_err());
        assert_eq!((d.root, d.saved_at()), (opened, Some(opened)));
        let now: Vec<PageId> = d.state().order().iter().copied().collect();
        assert_eq!(now, pages);
        crate::open::tests::assert_every_fact(d.state());
        assert_eq!(view.form.fields, 1);
        assert!(!d.is_dirty() && !d.can_undo() && !d.can_redo());
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
            src: d.primary().id(),
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
        // A widget goes only with its page.
        let (widget, _) = d.state().widgets_on(p1).next().unwrap();
        let delete = Op::DeleteAnnotations {
            annots: vec![AnnotRef::Id(widget)],
        };
        assert!(d.commit_ops(vec![delete], "delete").is_err());
        // Moving a page carries its widgets; deleting it takes them, and
        // undo brings them back.
        let move_up = Op::MovePages {
            pages: ids(&[p2]),
            before: Some(PageRef::Id(p1)),
        };
        d.commit_ops(vec![move_up], "move").unwrap();
        assert_eq!(on(&d, p2), 4);
        // The file's facts came once: none come after.
        let late = Facts {
            form: every_kind_form(),
            ..Default::default()
        };
        assert!(d.ready(late).is_err());
        let delete = Op::DeletePages { pages: ids(&[p2]) };
        let deleted = d.commit_ops(vec![delete], "delete").unwrap();
        // The fields whose widgets were all there go with them, as a save
        // takes them out of the file (the `widget.field` rule, M10.5
        // proposal 25); their parent keeps the others.
        assert_eq!((on(&d, p2), d.state().terminals().count()), (0, 5));
        assert_eq!((d.state().field_count(), listed(&d)), (6, 6));
        let fields = Cascade {
            rule: crate::tx::Rule::WidgetField,
            objects: 4,
        };
        assert!(deleted.cascades.contains(&fields), "{:?}", deleted.cascades);
        assert_eq!(d.state().field_named("form1[0].state[0]"), None);
        assert!(
            d.state()
                .form()
                .roots
                .iter()
                .all(|&r| d.state().field(r).is_some())
        );
        d.undo().unwrap();
        assert_eq!((on(&d, p2), listed(&d)), (4, 10));
        assert_eq!(d.state().terminals().count(), 9);
        // A replaced page shows another page, without them.
        let other = crate::store::blank(d.store(), 1);
        let replace = fx::replace(&[p1], &other, [0], "a.pdf");
        d.commit_ops(vec![replace], "replace").unwrap();
        assert_eq!((on(&d, p1), listed(&d)), (0, 4));
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
        let header = fx::set_header(fx::header("Draft"), Some(&[p1]));
        d.commit_ops(vec![header], "header").unwrap();
        for page in [p0, p1] {
            d.commit_ops(vec![fx::add(page, &square(0))], "add")
                .unwrap();
        }
        let labels = Op::SetPageLabels {
            ranges: Some(vec![fx::label(p1, LabelStyle::UpperRoman, "", 1)]),
        };
        d.commit_ops(vec![labels], "labels").unwrap();
        let to_no_page = [
            fx::bookmark(None, None, "B", Some(PageId(99))),
            Op::PatchBookmarks {
                bookmarks: vec![BookmarkRef::Id(a)],
                patch: BookmarkPatch {
                    target: Patch::Set(PageRef::Id(PageId(99))),
                    ..Default::default()
                },
            },
        ];
        for op in to_no_page {
            let refused = d.commit_ops(vec![op], "to page 99");
            assert!(refused.is_err(), "there's no page 99");
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
        let elsewhere_name = st.marks_on(p1).next().unwrap().name();
        let mark_as = |kind: AnnotKind| {
            let mut m = Mark::clone(rec.mark_record().unwrap());
            m.annot.kind = kind;
            Some(Val::Annot(AnnotRec {
                body: AnnotBody::Mark(Arc::new(m)),
                ..rec.clone()
            }))
        };
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
        let generator = st.generators().values().next().unwrap().clone();
        let gen_with = |id: u64| {
            let g = Generator {
                id: GenId(id),
                ..Generator::clone(&generator)
            };
            Some(Val::Generator(Arc::new(g)))
        };
        let on_no_page = {
            let g = Generator {
                scope: Scope::Pages([p1, PageId(99)].into_iter().collect()),
                ..Generator::clone(&generator)
            };
            Some(Val::Generator(Arc::new(g)))
        };
        let numbering_another = {
            let mut g = Generator::clone(&generator);
            g.frozen.bates = Some(Arc::new(im::OrdMap::unit(p0, 7)));
            Some(Val::Generator(Arc::new(g)))
        };
        let starting_nowhere = Some(Val::Labels(
            vec![crate::labels::LabelSpec {
                start: PageId(99),
                ..st.labels().unwrap()[0].clone()
            }]
            .into(),
        ));
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
                Some(Val::Annot(rec.clone())),
                "annotations: 1 aren't listed",
            ),
            (
                Key::Annot(mark),
                mark_as(AnnotKind::Connector {
                    rect: crate::annot::Box2 {
                        x: 1.0,
                        y: 1.0,
                        w: 22.0,
                        h: 22.0,
                    },
                    label: "1".into(),
                    partner: Some(widget),
                }),
                "isn't paired with it",
            ),
            (
                Key::Annot(mark),
                mark_as(AnnotKind::Reply {
                    parent: elsewhere_name,
                }),
                "a mark on another page",
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
            (
                Key::Generator(generator.id),
                on_no_page,
                "is on PageId(99), which isn't a page",
            ),
            (
                Key::Generator(generator.id),
                numbering_another,
                &format!("numbers {p0:?}, which it isn't on"),
            ),
            (
                Key::Labels,
                starting_nowhere,
                "page labels: a range starts at PageId(99), which isn't a page",
            ),
        ];
        for (key, val, says) in cases {
            let why = broken(key, val).to_string();
            assert!(why.contains(says), "{key:?}: {why}");
        }
    }

    /// One file's bytes, interned twice, are one source, and each insert
    /// of its pages brings an instance of its form of its own (M10.5
    /// §3.7): opened as the primary, its own is copy 0, and two inserts of
    /// it copies 1 and 2, named apart.
    #[test]
    fn inserting_the_same_file_twice_gives_one_source_and_two_forms() {
        let bytes = crate::forms::fixture::every_kind().bytes();
        let store = Store::new();
        let primary = file(&store, bytes.clone(), "form.pdf");
        let mut d = crate::open::start(
            store,
            primary.clone(),
            "form.pdf".into(),
            None,
            Box::new(FixedCount(10)),
        );
        d.ready(crate::open::finish(&primary, &in_progress::NoProgress))
            .unwrap();
        let mut renamed = Vec::new();
        for name in ["form.pdf", "again.pdf"] {
            let src = file(d.store(), bytes.clone(), name);
            assert!(std::ptr::eq(&*src, &*primary), "one source");
            let out = d.commit_ops(vec![insert_of(&src, name)], "insert").unwrap();
            renamed.push(renamed_under(&out));
        }
        // Its bytes are in the store once.
        assert_eq!(d.store().bytes(), primary.bytes().len());
        let inst = |copy| FormInst {
            src: primary.id(),
            copy,
        };
        assert_eq!(renamed, [["form"], ["again"]]);
        // Three forms, an instance each: the inserts' fields are under
        // names of their own.
        let st = d.state();
        let names = |copy| -> BTreeSet<String> {
            (st.terminals())
                .filter(|(_, node, _)| node.inst == inst(copy))
                .map(|(id, _, _)| st.full_name(id).unwrap())
                .collect()
        };
        let [own, first, second] = [names(0), names(1), names(2)];
        assert_eq!((own.len(), first.len(), second.len()), (9, 9, 9));
        assert!(first.iter().all(|n| n.starts_with("form.")), "{first:?}");
        assert!(second.iter().all(|n| n.starts_with("again.")), "{second:?}");
        let instances: Vec<(FormInst, &str)> =
            (st.form().names.iter()).map(|(i, n)| (*i, &**n)).collect();
        assert_eq!(
            instances,
            [
                (inst(0), "form.pdf"),
                (inst(1), "form.pdf"),
                (inst(2), "again.pdf")
            ]
        );
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
        let src = file(d.store(), bytes, "form.pdf");
        d.commit_ops(vec![insert_of(&src, "form.pdf")], "insert")
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
        let steps = d.edge(d.current_version()).unwrap();
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

    /// Scripts brought in by a second instance of a form go off (the same
    /// bytes again under another name, here), and undo before the insert
    /// turns them on again. A save leaves them off: it writes the forms as
    /// one, but the document still holds both instances. A duplicate of
    /// the second instance's page is another instance of its form, named
    /// as it is, renamed apart again.
    #[test]
    fn scripts_from_two_files_stay_off_after_a_save() {
        let store = Store::new();
        let primary = file(&store, scripted(), "a.pdf");
        let src = primary.clone();
        let mut d = crate::open::start(
            store,
            primary,
            "a.pdf".into(),
            None,
            Box::new(FixedCount(10)),
        );
        d.ready(crate::open::finish(&src, &in_progress::NoProgress))
            .unwrap();
        assert!(!scripts_off(&d));
        let b = file(d.store(), scripted(), "b.pdf");
        assert_eq!(b, src, "the same bytes");
        let out = d
            .commit_ops(vec![insert_of(&b, "b.pdf")], "insert")
            .unwrap();
        let inst = |copy| FormInst {
            src: src.id(),
            copy,
        };
        assert_eq!(renamed_under(&out), ["b"]);
        assert!(out.notices.contains(&Notice::ScriptsOff));
        let ScriptState::Off { instances } = &d.state().form().scripts else {
            panic!("scripts on");
        };
        assert_eq!(**instances, [inst(0), inst(1)]);
        d.undo().unwrap();
        assert!(!scripts_off(&d));
        d.redo().unwrap();

        let page = id_at(&d, 1);
        let out = d
            .commit_ops(vec![fx::duplicate(&[page])], "duplicate")
            .unwrap();
        assert_eq!(renamed_under(&out), ["b_2"]);
        let names: Vec<(FormInst, &str)> = (d.state().form().names.iter())
            .map(|(i, n)| (*i, &**n))
            .collect();
        assert_eq!(
            names,
            [(inst(0), "a.pdf"), (inst(1), "b.pdf"), (inst(2), "b.pdf")]
        );
        let st = d.state();
        assert!(st.field_named("b_2.a").is_some());

        let saved = saved(&mut d);
        assert!(scripts_off(&d), "scripts still off");
        assert_eq!(d.state().form().names.len(), 3);
        // The saved file's form is one, with both files' fields.
        let form = saved.form();
        assert!(form.fields.iter().any(|f| f.name == "b_2.a"));
    }

    /// A duplicate of a page with a scripted form is an instance of that
    /// form of its own (M10.5 §3.7), whose scripts name the original's
    /// fields: scripts from two instances share one scope, so they go
    /// off, as two files' do, and undo turns them on again. (Until slice
    /// 11 a duplicate counted as its file's, which kept them on.)
    #[test]
    fn a_scripted_page_s_duplicate_turns_scripts_off() {
        let store = Store::new();
        let primary = file(&store, scripted(), "a.pdf");
        let policy = Box::new(FixedCount(10));
        let mut d = crate::open::start(store, primary.clone(), "a.pdf".into(), None, policy);
        d.ready(crate::open::finish(&primary, &in_progress::NoProgress))
            .unwrap();
        assert!(!scripts_off(&d));
        let page = id_at(&d, 0);
        d.commit_ops(vec![fx::duplicate(&[page])], "duplicate")
            .unwrap();
        assert!(scripts_off(&d));
        let inst = |copy| FormInst {
            src: primary.id(),
            copy,
        };
        let ScriptState::Off { instances } = &d.state().form().scripts else {
            panic!("scripts on");
        };
        assert_eq!(**instances, [inst(0), inst(1)]);
        d.undo().unwrap();
        assert!(!scripts_off(&d));
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
        let first = id_at(&d, 0);
        d.commit_ops(vec![fx::move_pages(&[p5], Some(first))], "move")
            .unwrap();
        assert_eq!(order(&d), vec![5, 0, 1, 2, 3, 4]);
        d.commit_ops(vec![fx::rotate(&[p5], Turn::Quarter)], "rotate")
            .unwrap();
        assert_eq!(rotation(&d, 0), Rotation::R90);
        assert_eq!(d.page_size(d.page(0).unwrap()).width, 792.0);
        assert!(d.is_dirty());
        assert_eq!(
            d.undo().map(|c| c.map(|c| c.label)),
            Ok(Some("rotate".into()))
        );
        assert_eq!(rotation(&d, 0), Rotation::R0);
        assert_eq!(
            d.undo().map(|c| c.map(|c| c.label)),
            Ok(Some("move".into()))
        );
        assert_eq!(order(&d), vec![0, 1, 2, 3, 4, 5]);
        assert!(!d.is_dirty());
        assert_eq!(
            d.redo().map(|c| c.map(|c| c.label)),
            Ok(Some("move".into()))
        );
        assert_eq!(order(&d), vec![5, 0, 1, 2, 3, 4]);
        // A new edit discards the redo branch.
        d.commit_ops(vec![fx::delete(&[p5])], "delete").unwrap();
        assert!(!d.can_redo());
    }

    #[test]
    fn delete_cascades_and_promotes_children() {
        let mut d = doc();
        let first = id_at(&d, 0);
        let out = d.commit_ops(vec![fx::delete(&[first])], "delete").unwrap();
        assert_eq!(d.page_count(), 5);
        let promoted = Cascade {
            rule: crate::tx::Rule::PageBookmarks,
            objects: 1,
        };
        assert_eq!(out.cascades, [promoted]);
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
        // The inserted file's bookmarks come from it.
        let bytes = crate::forms::fixture::outlined(3, &[("A", 0, 0), ("A3", 1, 1), ("D", 2, 0)]);
        let s = file(d.store(), bytes, "b.pdf");
        let bookmarks = d.state().bookmark_count();
        let insert = fx::insert(&s, 0..3, "b.pdf", Some(id_at(&d, 3)));
        let out = d.commit_ops(vec![insert], "insert").unwrap();
        assert_eq!(out.created.ops[0].len(), 3);
        assert_eq!(d.state().bookmark_count() - bookmarks, 2);
        assert_eq!(order(&d), vec![0, 1, 2, 0, 1, 2, 3, 4, 5]);
        assert_eq!(def_at(&d, 3).base.src.id(), s.id());
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
        let s = crate::store::blank(d.store(), 2);
        let p3 = id_at(&d, 3);
        d.commit_ops(vec![fx::replace(&[p3], &s, 0..2, "b.pdf")], "replace")
            .unwrap();
        assert_eq!(d.page(3).unwrap(), p3);
        assert_eq!(def_at(&d, 3).base.src.id(), s.id());
        assert_eq!(def_at(&d, 4).base.src.id(), s.id());
        assert_eq!(order(&d), vec![0, 1, 2, 0, 1, 4, 5]);
        assert_eq!(bookmark(&d, "B").page(), Some(p3));
    }

    /// A page listed twice is replaced once, and stays with the marks that
    /// come to it.
    #[test]
    fn a_page_listed_twice_is_replaced_once() {
        let mut d = doc();
        let bytes = crate::forms::fixture::with_marks(1, &[(0, vec![square(1)])]);
        let s = file(d.store(), bytes, "b.pdf");
        let p3 = id_at(&d, 3);
        let replace = fx::replace(&[p3, p3], &s, [0], "b.pdf");
        d.commit_ops(vec![replace], "replace").unwrap();
        assert_eq!(d.page_count(), 6);
        assert_eq!(def_at(&d, 3).base.src.id(), s.id());
        assert_eq!(d.state().marks_on(p3).count(), 1);
    }

    #[test]
    fn a_duplicate_goes_after_its_page() {
        let mut d = doc();
        let second = id_at(&d, 1);
        d.commit_ops(vec![fx::duplicate(&[second])], "dup").unwrap();
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
        let src = primary.clone();
        let d = Document::new(
            store,
            primary,
            "twice.pdf".into(),
            None,
            &[],
            Box::new(FixedCount(10)),
        );
        assert_eq!(copies(&d), [(0, 0), (1, 1)]);
        let object = |i| src.page(i).unwrap().id;
        assert_eq!(object(0), object(1), "one object, listed twice");
        assert!(!d.is_dirty());
    }

    /// A page made from a base page takes the copy after the highest any
    /// page holds of it: a duplicate, a duplicate of a duplicate, and the
    /// same bytes inserted again.
    #[test]
    fn copies_take_the_next_copy_of_their_base() {
        let mut d = new_doc(2, &[], Box::new(FixedCount(10)));
        let dup = |d: &mut Document, i| {
            let page = id_at(d, i);
            d.commit_ops(vec![fx::duplicate(&[page])], "dup").unwrap();
        };
        dup(&mut d, 0);
        dup(&mut d, 0);
        assert_eq!(copies(&d), [(0, 0), (0, 2), (0, 1), (1, 0)]);
        // A copy's copy is the next of the same base, and shows it.
        dup(&mut d, 2);
        assert_eq!(copies(&d)[3], (0, 3));
        let other = crate::store::blank(d.store(), 1);
        let first = insert_all(&mut d, other.clone());
        let again = insert_all(&mut d, other.clone());
        assert_eq!(first, again, "one source");
        let inserted: Vec<(SourceId, u32)> = (d.state().pages().skip(5))
            .map(|(_, def, _)| (def.base.src.id(), def.copy))
            .collect();
        assert_eq!(inserted, [(first, 0), (first, 1)]);
        // A page replaced by one the document shows is another copy of it.
        let last = id_at(&d, 6);
        let replace = fx::replace(&[last], &other, [0], "a.pdf");
        d.commit_ops(vec![replace], "replace").unwrap();
        assert_eq!(d.state().def(last).unwrap().copy, 2);
    }

    /// Moving pages changes the order and nothing else: one prim, and the
    /// new positions are there at once.
    #[test]
    fn a_reorder_puts_only_the_order() {
        let mut d = doc();
        let p5 = id_at(&d, 5);
        assert_eq!(d.index_of(p5), Some(5));
        let first = id_at(&d, 0);
        d.commit_ops(vec![fx::move_pages(&[p5], Some(first))], "move")
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
        d.commit_ops(vec![fx::duplicate(&[page])], "dup").unwrap();
        let copy = d.state().def(id_at(&d, 2)).unwrap();
        assert!(Arc::ptr_eq(&copy.content, &def.content));
        assert_eq!((copy.base.index, copy.copy), (1, 1));
        // Cleared, the page holds the stack untouched pages share.
        let clear = Op::ClearOcrText {
            pages: ids(&[page]),
        };
        d.commit_ops(vec![clear], "clear").unwrap();
        let cleared = &d.state().def(page).unwrap().content;
        assert!(ContentStack::is_shared_empty(cleared));
    }

    #[test]
    fn outline_edit_insert_delete() {
        let mut d = doc();
        let a = bm(&d, "A");
        let patch = BookmarkPatch {
            title: Some("Alpha".into()),
            color: Patch::Set([255, 0, 0]),
            ..Default::default()
        };
        d.commit_ops(vec![fx::patch_bookmark(a, patch)], "e")
            .unwrap();
        assert_eq!(titles(&d)[0].1, "Alpha");
        let a1 = bm(&d, "A1");
        let add = fx::bookmark(Some(a), Some(a1), "A1b", None);
        d.commit_ops(vec![add], "ins").unwrap();
        assert_eq!(titles(&d)[2], (1, "A1b".into()));
        let bookmarks = d.state().bookmark_count();
        let delete = Op::DeleteBookmarks {
            bookmarks: vec![BookmarkRef::Id(a)],
        };
        d.commit_ops(vec![delete], "del").unwrap();
        assert_eq!(bookmarks - d.state().bookmark_count(), 4);
        assert_eq!(titles(&d), vec![(0, "B".into()), (0, "C".into())]);
    }

    #[test]
    fn outline_move_rejects_cycles() {
        let mut d = doc();
        let a = bm(&d, "A");
        let a1 = bm(&d, "A1");
        let into_its_own = Op::MoveBookmarks {
            bookmarks: vec![BookmarkRef::Id(a)],
            parent: Some(BookmarkRef::Id(a1)),
            at: Sibling::First,
            with_pages: false,
        };
        assert!(d.commit_ops(vec![into_its_own], "x").is_err());
    }

    pub(super) fn rotate(d: &mut Document, page: usize, label: &str) {
        let page = id_at(d, page);
        d.commit_ops(vec![fx::rotate(&[page], Turn::Quarter)], label)
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
        assert_eq!(d.redo().map(|c| c.map(|c| c.label)), Ok(Some("c".into())));
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
            let first = id_at(&d, 0);
            let turn = fx::rotate(&[first], Turn::Quarter);
            d.commit_ops(vec![turn], &format!("r{i}")).unwrap();
        }
        // Three snapshots (the opened version's, the current one's and one
        // more); all six versions, with their edits.
        assert_eq!(d.stats().versions, 3);
        assert_eq!(d.version_count(), 6);
        // Undo goes all the way back, replaying what was dropped.
        for i in (0..5).rev() {
            assert_eq!(
                d.undo().map(|c| c.map(|c| c.label)),
                Ok(Some(format!("r{i}").into()))
            );
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
        let lines: Vec<Vec<&str>> = (views[1..].iter())
            .map(|v| v.edits.iter().map(|l| l.text.as_str()).collect())
            .collect();
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
        assert_eq!(d.replay(v), Err(EditError::Mismatch { version: v }));
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
        if let Some(undone) = unless_debug(|| d.undo().map(|c| c.map(|c| c.label))) {
            assert_eq!(undone, Err(EditError::Mismatch { version: v }));
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
        assert_eq!(d.replay(v), Err(EditError::Mismatch { version: v }));
        if let Some(jumped) = unless_debug(|| d.jump(v).is_ok()) {
            assert!(!jumped, "it can't be reached");
        }
        assert_eq!(d.current_version(), current);
    }

    /// A save is an output: the document keeps its pages, their sources
    /// and ids, and its history, and the saved file has what it shows.
    #[test]
    fn a_save_keeps_ids_and_history() {
        let mut d = doc();
        let p5 = id_at(&d, 5);
        let first = id_at(&d, 0);
        d.commit_ops(vec![fx::move_pages(&[p5], Some(first))], "move")
            .unwrap();
        d.commit_ops(vec![fx::rotate(&[p5], Turn::Quarter)], "rotate")
            .unwrap();
        let (primary, version) = (d.primary().clone(), d.current_version());
        let saved = saved(&mut d);
        assert_eq!((d.primary(), d.current_version()), (&primary, version));
        assert!(!d.is_dirty());
        assert_eq!(d.page(0).unwrap(), p5);
        assert_eq!(def_at(&d, 0).base.src, primary);
        assert_eq!(order(&d), vec![5, 0, 1, 2, 3, 4]);
        // The rotation is the page's /Rotate, which a save writes as is.
        assert_eq!(rotation(&d, 0), Rotation::R90);
        assert_eq!(saved.geom(0).rotate, Rotation::R90);
        assert_eq!(saved.page_count(), 6);
        // Undo reaches the version before, as it was.
        d.undo().unwrap();
        assert_eq!(def_at(&d, 0).base.index, 5);
        assert_eq!(rotation(&d, 0), Rotation::R0);
        assert!(d.is_dirty());
        d.redo().unwrap();
        assert!(!d.is_dirty());
    }

    /// One Letter page turned `degrees` by its `/Rotate`, or by its
    /// parent's when `inherited`, as a source in `store`.
    fn turned(store: &Store, own: Option<i64>, parent: Option<i64>) -> Src {
        let mut f = crate::forms::fixture::Form::new(1);
        let page = f.pages[0];
        let up = f.dict(page).get(b"Parent").unwrap().as_reference().unwrap();
        for (at, degrees) in [(page, own), (up, parent)] {
            if let Some(degrees) = degrees {
                f.dict(at).set("Rotate", degrees);
            }
        }
        file(store, f.bytes(), "turned.pdf")
    }

    /// A directory of the test's own.
    fn scratch() -> PathBuf {
        static N: AtomicU64 = AtomicU64::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("sheaf-model-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// `d` saved to a file of its own (`materialize::save_with`, a Rewrite),
    /// signatures and XFA values dropped as the reader agreed to, and the
    /// file read back as a source. The document doesn't change: which
    /// version is saved, its record, and nothing else.
    fn saved(d: &mut Document) -> Src {
        let path = scratch().join("saved.pdf");
        let answers = crate::materialize::AskAnswers {
            break_signatures: true,
            drop_xfa_values: true,
            ..Default::default()
        };
        let how = crate::materialize::SaveHow::Planned;
        crate::materialize::save_with(d, &path, how, &answers, &in_progress::NoProgress).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        d.store()
            .open(bytes.into(), BlobKind::file(&path, None))
            .unwrap()
    }

    /// As [`saved`], by an Append onto the primary's bytes, written there
    /// as the file it was opened from (`materialize::save_appended`).
    fn appended(d: &mut Document) -> Src {
        let path = scratch().join("saved.pdf");
        crate::materialize::save_appended(d, &path).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        d.store()
            .open(bytes.into(), BlobKind::file(&path, None))
            .unwrap()
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
        let s = turned(d.store(), Some(90), None);
        d.commit_ops(vec![insert_of(&s, "turned.pdf")], "insert")
            .unwrap();
        let p = id_at(&d, 1);
        assert_eq!(d.render_delta(p), Rotation::R0);
        assert_eq!(d.page_size(p), LANDSCAPE);
        assert_eq!(d.geom(p).rotate, Rotation::R90);
        // Turned again, it's turned from there, and PDFium adds the turn.
        d.commit_ops(vec![fx::rotate(&[p], Turn::Quarter)], "rotate")
            .unwrap();
        assert_eq!(rotation(&d, 1), Rotation::R180);
        assert_eq!(d.render_delta(p), Rotation::R90);
        assert_eq!(d.snapshot().render_delta(p), Rotation::R90);
        assert_eq!(d.page_size(p).width, 612.0);
    }

    /// A page's rotation is its `/Rotate`: a save writes it as it is, and
    /// the document, which a save doesn't change, still shows the page as
    /// the file it opened has it, with the turn sheaf adds for PDFium.
    #[test]
    fn a_rotation_survives_a_save() {
        rotation_survives(false);
    }

    #[test]
    fn a_rotation_survives_a_save_appended() {
        rotation_survives(true);
    }

    fn rotation_survives(incremental: bool) {
        let mut d = new_doc(2, &[], Box::new(FixedCount(10)));
        rotate(&mut d, 0, "rotate");
        let before = d.state().clone();
        let saved = match incremental {
            true => appended(&mut d),
            false => saved(&mut d),
        };
        assert_eq!(saved.geom(0).rotate, Rotation::R90);
        assert_eq!(saved.geom(1).rotate, Rotation::R0);
        assert!(*d.state() == before && !d.is_dirty());
        let p = id_at(&d, 0);
        assert_eq!(rotation(&d, 0), Rotation::R90);
        assert_eq!(d.render_delta(p), Rotation::R90);
        assert_eq!(d.page_size(p), LANDSCAPE);
    }

    /// A page turned upright is upright in the saved file whatever its
    /// parent's `/Rotate`: an Append leaves it under its parent,
    /// so it says 0. A page that's only marked keeps the `/Rotate` it has,
    /// its own or its parent's, a 135 too, which reads as 90.
    #[test]
    fn a_page_turned_upright_stays_upright_through_a_save() {
        upright_through(false);
    }

    #[test]
    fn a_page_turned_upright_stays_upright_through_a_save_appended() {
        upright_through(true);
    }

    fn upright_through(incremental: bool) {
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
            (None, Some(90), Some(Turn::Back)),
            (Some(90), Some(180), Some(Turn::Back)),
            (Some(0), Some(90), None),
            (Some(135), Some(180), None),
        ];
        for (own, parent, by) in cases {
            let case = format!("{own:?} under {parent:?}, turned {by:?}");
            let store = Store::new();
            let primary = turned(&store, own, parent);
            let mut d = Document::new(
                store,
                primary,
                "twice.pdf".into(),
                None,
                &[],
                Box::new(FixedCount(10)),
            );
            let page = id_at(&d, 0);
            let was = rotation(&d, 0);
            let op = match by {
                Some(by) => fx::rotate(&[page], by),
                None => fx::add(page, &mark(0, square.clone())),
            };
            d.commit_ops(vec![op], "edit").unwrap();
            let want = rotation(&d, 0);
            let turned = was.plus(by.map_or(Rotation::R0, Turn::rotation));
            assert_eq!(want, turned, "{case}");
            let saved = match incremental {
                true => appended(&mut d),
                false => saved(&mut d),
            };
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

    /// A mark on a page sheaf turned shows where it did after a save, and
    /// the saved file has it where the model does: marks are in PageSpace,
    /// which a save doesn't move.
    #[test]
    fn a_mark_on_a_rotated_page_stays_put_through_a_save() {
        mark_stays_put(false);
    }

    #[test]
    fn a_mark_on_a_rotated_page_stays_put_through_a_save_appended() {
        mark_stays_put(true);
    }

    fn mark_stays_put(incremental: bool) {
        let rect = crate::annot::Box2 {
            x: 100.0,
            y: 650.0,
            w: 50.0,
            h: 40.0,
        };
        let mut d = new_doc(1, &[], Box::new(FixedCount(10)));
        rotate(&mut d, 0, "rotate");
        let page = id_at(&d, 0);
        let annot = mark(0, AnnotKind::Square { rect });
        d.commit_ops(vec![fx::add(page, &annot)], "add").unwrap();
        let shown = |d: &Document| {
            let AnnotKind::Square { rect } = d.state().marks_on(page).next().unwrap().kind else {
                unreachable!("the square");
            };
            d.geom(d.page(0).unwrap()).to_view().apply_box(rect)
        };
        let before = shown(&d);
        let saved = match incremental {
            true => appended(&mut d),
            false => saved(&mut d),
        };
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

    /// A save changes no version, so the versions made from the one it
    /// wrote before (undone, then saved) replay from it as before: their
    /// prims are from it as it is.
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
        saved(&mut d);
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
        let pair = fx::connect((p0, &mk("1")), (p2, &mk("1")));
        d.commit_ops(vec![pair], "c").unwrap();
        let a = d.state().marks_on(p0).next().unwrap().clone();
        let b = d.state().marks_on(p2).next().unwrap().clone();
        assert_eq!(a.partner(), Some(b.id));
        assert_eq!(b.partner(), Some(a.id));
        assert_eq!(d.state().find_annot(b.id).map(|(p, _)| p), Some(p2));
        assert_eq!(d.state().next_connector_label(), "2");
        // Deleting one end leaves the other: a group of one (decision 8).
        d.commit_ops(vec![fx::delete_annots(&[b.id])], "rm")
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
            d.commit_ops(vec![fx::add(page, &tick(glyph, y))], "t")
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
        d.commit_ops(vec![fx::rotate(&[p1], Turn::Quarter)], "rotate")
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
            d.commit_ops(vec![fx::add(page, &annot)], "add").unwrap()
        };
        let first = add(&mut d, note.clone());
        assert_eq!(first.notices, [Notice::Unencodable]);
        let second = add(&mut d, note);
        assert_eq!(second.notices, [], "told once");
        let plain = add(&mut d, square(0));
        assert_eq!(plain.notices, []);

        let written = &d.state().marks_on(page).next().unwrap();
        let mut pdf = lopdf::Document::with_version("1.7");
        let main = crate::serialize::built::to_pdf(written, &mut pdf).main;
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

        // A header's is found once its tokens are filled in; told once
        // already, it isn't told again.
        let mut hf = fx::header("");
        hf.texts[1] = "報告 <<file>>".into();
        let set = |d: &mut Document, hf: &sheaf_edit::HeaderFooterSpec| {
            d.commit_ops(vec![fx::set_header(hf.clone(), None)], "hf")
                .unwrap()
        };
        let mut fresh = doc();
        assert_eq!(set(&mut fresh, &hf).notices, [Notice::Unencodable]);
        assert_eq!(set(&mut d, &hf).notices, [], "told once");
        hf.texts[1] = "report".into();
        hf.texts[2] = "Page <<1>> of <<N>> – <<date>>".into();
        assert_eq!(set(&mut fresh, &hf).notices, []);
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
        let policy = Box::new(FixedCount(5));
        let d = Document::new(store, src, "a.pdf".into(), None, &outline, policy);
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
        let steps = d.edge(d.current_version()).unwrap();
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
        let patch = BookmarkPatch {
            title: Some("First".into()),
            target: Patch::Set(PageRef::Id(p4)),
            color: Patch::Set([0, 0, 255]),
            ..Default::default()
        };
        d.commit_ops(vec![fx::patch_bookmark(a1, patch)], "Edit bookmark")
            .unwrap();
        assert_eq!(keys_put(&d), [Key::Bookmark(a1)]);
        let b = bookmark(&d, "First");
        assert_eq!((b.page(), b.color), (Some(p4), Some([0, 0, 255])));
        assert_eq!(titles(&d)[1], (1, "First".into()));
    }

    crate::both_ways!(
        a_uri_bookmark_is_kept,
        a_uri_bookmark_is_kept_appended: uri_bookmark_kept
    );

    /// An item going somewhere sheaf doesn't model, a link, opens kept
    /// from the item it was read as. A Rewrite after a reorder writes its
    /// `/A` again, and the `/SE`s of the file's items; so does one after a
    /// save, which leaves it the file's item (M10.5 bug 15).
    fn uri_bookmark_kept(how: How) {
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
        let read = crate::open::outline(&primary);
        let facts: Vec<_> = read.iter().map(|i| (i.object, i.kept, i.page)).collect();
        assert_eq!(
            facts,
            [
                (Some(items[0]), true, None),
                (Some(items[1]), false, Some(2))
            ]
        );
        let mut d = Document::new(
            store,
            primary,
            "links.pdf".into(),
            None,
            &read,
            Box::new(FixedCount(10)),
        );
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
        d.commit_ops(vec![fx::move_pages(&[last], Some(first))], "Move")
            .unwrap();
        assert_eq!(written(crate::save::rewrite(&d).unwrap()), want(0));
        // A save writes the outline anew once it changed.
        let rename = BookmarkPatch {
            title: Some("3".into()),
            ..Default::default()
        };
        let third = bm(&d, "Third");
        d.commit_ops(vec![fx::patch_bookmark(third, rename)], "Rename")
            .unwrap();
        assert_eq!(written(how.write(&d)), want(0));
        saved(&mut d);
        let site = bookmark(&d, "Site");
        assert_eq!(site.target, Some(Target::Kept));
        assert_eq!(site.src.as_ref().map(|r| r.id), Some(items[0]));
        d.commit_ops(vec![fx::move_pages(&[last], None)], "Move")
            .unwrap();
        assert_eq!(written(crate::save::rewrite(&d).unwrap()), want(2));
    }

    /// Bookmarks nested deeper than a file is read (32) are written, and a
    /// link bookmark after them in pre-order keeps its own item and its
    /// `/A`, through one save and a second.
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
        let read = crate::open::outline(&primary);
        let policy = Box::new(FixedCount(10));
        let mut d = Document::new(store, primary, "deep.pdf".into(), None, &read, policy);
        let deepest = 34;
        let first = id_at(&d, 0);
        let deep = (0..=deepest)
            .map(|depth: u16| Op::AddBookmark {
                parent: (depth > 0).then_some(BookmarkRef::New(Local(depth))),
                at: Sibling::First,
                title: format!("d{depth}"),
                target: Some(PageRef::Id(first)),
                view: None,
                color: None,
                style: Default::default(),
                local: Some(Local(depth + 1)),
            })
            .collect();
        d.commit_ops(deep, "Deep").unwrap();
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
        let first = saved(&mut d);
        assert_eq!(
            uri(first.bytes().to_vec()).as_deref(),
            Some("https://example.com/")
        );
        let site = bookmark(&d, "Site");
        assert_eq!(site.target, Some(Target::Kept));
        let again = saved(&mut d);
        assert_eq!(
            uri(again.bytes().to_vec()).as_deref(),
            Some("https://example.com/")
        );
        let titles = |src: &Src| crate::open::outline(src).len();
        assert_eq!(titles(&again), titles(&first));
    }

    /// The header/footer sheaf drew opens as the generator that draws it,
    /// on the pages it's on (`/SheafGenerators`, and its blocks). Set
    /// again, it's the same generator, its id kept; a Rewrite draws it as
    /// its blocks, which an open of the file finds again, as they are.
    #[test]
    fn the_header_footer_is_a_generator() {
        use crate::content::{PagOrigin, PaginationKind};
        let store = Store::new();
        let bytes = crate::forms::fixture::every_fact().bytes();
        let primary = file(&store, bytes, "facts.pdf");
        let src = primary.clone();
        let mut d = crate::open::start(
            store,
            primary,
            "facts.pdf".into(),
            None,
            Box::new(FixedCount(10)),
        );
        d.ready(crate::open::finish(&src, &in_progress::NoProgress))
            .unwrap();
        let pages: Vec<PageId> = d.state().order().iter().copied().collect();
        let only = |d: &Document| {
            let gens: Vec<Arc<Generator>> = d.state().generators().values().cloned().collect();
            assert_eq!(gens.len(), 1);
            gens[0].clone()
        };
        let g = only(&d);
        let (id, hf) = (g.id, g.spec.header_footer().unwrap().clone());
        let on = Scope::Pages(im::OrdSet::unit(pages[2]));
        assert_eq!((hf.texts[0].as_str(), &g.scope), ("Draft", &on));
        assert_eq!(id, GenId(crate::generate::fixture::SHEAF_HEADER_ID));
        for text in ["Final", "Final, again"] {
            let mut spec = HeaderFooterSpec::clone(&hf);
            spec.texts[0] = text.into();
            let set = fx::set_header(spec, Some(&[pages[2]]));
            d.commit_ops(vec![set], "Header & footer").unwrap();
            assert_eq!(keys_put(&d), [Key::Generator(id)]);
            assert_eq!(only(&d).id, id, "{text}");
        }
        let saved = saved(&mut d);
        let catalog = saved.pdf().catalog().unwrap();
        assert!(!catalog.has(b"SheafHeaderFooter"));
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
        let header = (PaginationKind::Header, PagOrigin::Sheaf);
        assert_eq!(drawn, [vec![], vec![], vec![header]]);
        // Opened, it's found again on its page, drawn there as the file
        // draws it.
        let mut again = crate::open::start(
            d.store().clone(),
            saved.clone(),
            "saved.pdf".into(),
            None,
            Box::new(FixedCount(10)),
        );
        again
            .ready(crate::open::finish(&saved, &in_progress::NoProgress))
            .unwrap();
        let reopened: Vec<PageId> = again.state().order().iter().copied().collect();
        let g = only(&again);
        let texts = &g.spec.header_footer().unwrap().texts;
        let on = Scope::Pages(im::OrdSet::unit(reopened[2]));
        assert_eq!(
            (g.id, texts[0].as_str(), &g.scope),
            (id, "Final, again", &on)
        );
        let screen = crate::realize::ViewInputs::screen();
        assert!(crate::realize::untouched(
            again.state(),
            reopened[2],
            &screen
        ));
        let remove = Op::RemoveHeaderFooter {
            pages: PageSel::All,
            include_authored: false,
        };
        again
            .commit_ops(vec![remove], "Remove header & footer")
            .unwrap();
        assert!(again.state().generators().is_empty());
    }

    #[test]
    fn inserted_pages_bring_their_marks_with_fresh_ids() {
        let mut d = doc();
        let first = id_at(&d, 0);
        let mine = d.commit_ops(vec![fx::add(first, &square(0))], "add");
        assert!(mine.is_ok());
        // Page 0: a square and one end of a pair whose other end is on
        // page 1; page 2 isn't inserted.
        let marks = [
            (0, vec![square(1), connector(2, 3)]),
            (1, vec![connector(3, 2), connector(4, 9)]),
            (2, vec![square(5)]),
        ];
        let bytes = crate::forms::fixture::with_marks(3, &marks);
        let sid = file(d.store(), bytes, "b.pdf");
        d.commit_ops(vec![fx::insert(&sid, 0..2, "a.pdf", None)], "insert")
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
            d.commit_ops(vec![fx::add(p, &square(0))], "add").unwrap();
        }
        let bytes = crate::forms::fixture::with_marks(1, &[(0, vec![square(7)])]);
        let sid = file(d.store(), bytes, "b.pdf");
        let replace = fx::replace(&[p1, p2], &sid, [0], "a.pdf");
        d.commit_ops(vec![replace], "replace").unwrap();
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
            d.commit_ops(vec![fx::add(p, &square(0))], "add").unwrap();
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
        let src = file(&store, bytes, "saved.pdf");
        let policy = Box::new(FixedCount(10));
        let mut d = crate::open::start(store, src.clone(), "saved.pdf".into(), None, policy);
        d.ready(crate::open::finish(&src, &in_progress::NoProgress))
            .unwrap();
        d
    }

    /// The page stamp mark `id` of `d` holds.
    fn stamp_held(d: &Document, id: AnnotId) -> Option<FormRef> {
        d.state().annot(id)?.mark_record()?.stamp.clone()
    }

    /// A page stamp's mark finds its page as the host finds it for the op
    /// that places it: in the store, placed and held; else in a source
    /// whose marks draw it, the primary's first; else nowhere, and the op,
    /// holding none, fails. An update keeps what it holds. Another tool's
    /// stamp, made a mark, holds the form its appearance is in its file.
    #[test]
    fn a_stamp_resolves_from_the_store_or_a_source() {
        use crate::forms::fixture::{self, MARK_STAMP};
        let stamp = fixture::every_mark_kind()[17].clone();
        let add = |d: &mut Document, annot: Annot| {
            let page = d.page(0).unwrap();
            let AnnotKind::PageStamp { stamp: stem, .. } = &annot.kind else {
                unreachable!("a page stamp")
            };
            let mut tx = d.tx(Actor::User);
            let asset = (d.find_stamp(stem)).map(|form| {
                let stem = stem.clone();
                tx.grant(crate::tx::Grant::Stamp { stem, form })
            });
            let op = Op::AddMark {
                page: PageRef::Id(page),
                mark: sheaf_edit::NewMark {
                    asset,
                    ..fx::new_mark(&annot)
                },
                reply_to: None,
                local: None,
                imported: None,
            };
            tx.apply(op, &in_progress::NoProgress)?;
            d.commit(tx, CommitOpts::default())?;
            Ok::<_, EditError>(d.state().marks_on(page).last().unwrap().id)
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
        d.commit_ops(vec![fx::update(&a)], "edit").unwrap();
        assert_eq!(stamp_held(&d, id), placed);
        let saved = crate::save::rewrite(&d).unwrap();

        // Nowhere: the op holds none, and fails.
        let mut other = new_doc(1, &[], Box::new(FixedCount(10)));
        let err = add(&mut other, stamp.clone()).unwrap_err();
        assert!(format!("{err:#}").contains("needs its page"), "{err:#}");
        // A file whose pages came in holds it.
        let came = file(other.store(), saved.clone(), "stamped.pdf");
        insert_all(&mut other, came.clone());
        let id = add(&mut other, stamp.clone()).unwrap();
        assert!(matches!(stamp_held(&other, id), Some(FormRef::Base(at)) if at.src == came));
        // The primary holds it.
        let mut opened = reopened(saved);
        let primary = opened.primary().clone();
        let id = add(&mut opened, stamp.clone()).unwrap();
        assert!(matches!(stamp_held(&opened, id), Some(FormRef::Base(at)) if at.src == primary));

        // Another tool's stamp, adopted: no blob, the file's form.
        use lopdf::dictionary;
        let mut f = fixture::Form::new(1);
        let form = f.pdf.add_object(lopdf::Stream::new(
            dictionary! {
                "Type" => "XObject", "Subtype" => "Form",
                "BBox" => vec![0.into(), 0.into(), 100.into(), 40.into()],
            },
            format!("0 0 1 rg 0 0 100 40 re f % {}", fastrand::u64(..)).into_bytes(),
        ));
        let theirs = f.pdf.add_object(dictionary! {
            "Type" => "Annot", "Subtype" => "Stamp", "Name" => "SBApproved",
            "Rect" => vec![100.into(), 600.into(), 200.into(), 640.into()],
            "AP" => dictionary! { "N" => form },
        });
        f.set_annots(0, &[theirs]);
        let mut d = reopened(f.bytes());
        let page = d.page(0).unwrap();
        let (id, stem) = (d.state().foreign_on(page))
            .find_map(|(id, f)| match &f.summary.mark.as_deref()?.kind {
                AnnotKind::PageStamp { stamp, .. } => Some((id, stamp.clone())),
                _ => None,
            })
            .expect("a stamp a mark can be made of");
        assert!(BlobId::from_stem(&stem).is_some_and(|b| d.store().get(b).is_none()));
        d.commit_ops(vec![fx::adopt(&[id])], "adopt").unwrap();
        let primary = d.primary().clone();
        let held = stamp_held(&d, id);
        assert!(
            matches!(&held, Some(FormRef::Base(at)) if at.src == primary && at.id == form),
            "{held:?}"
        );
    }

    /// An attachment's mark finds its file as the host finds it for the op
    /// that attaches it: in the store, attached and held; else in a source
    /// whose marks embed it, decoded into the store. One nothing holds
    /// isn't found, and the op, holding none, fails.
    #[test]
    fn an_attachment_is_a_blob_never_a_cache_file() {
        use crate::forms::fixture::{self, MARK_FILE, MARK_FILE_BYTES};
        let attach = fixture::every_mark_kind()[23].clone();
        let add = |d: &mut Document| {
            let page = d.page(0).unwrap();
            let mut tx = d.tx(Actor::User);
            let found = d.find_file(&MARK_FILE, MARK_FILE_BYTES.len() as u64);
            let asset = found.map(|file| tx.grant(crate::tx::Grant::Blob(file)));
            let op = Op::AddMark {
                page: PageRef::Id(page),
                mark: sheaf_edit::NewMark {
                    asset,
                    ..fx::new_mark(&attach)
                },
                reply_to: None,
                local: None,
                imported: None,
            };
            tx.apply(op, &in_progress::NoProgress)?;
            d.commit(tx, CommitOpts::default())?;
            Ok::<_, EditError>(d.state().marks_on(page).last().unwrap().id)
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
        let saved = crate::save::rewrite(&d).unwrap();

        // Nowhere: the op holds none, and fails.
        let mut other = new_doc(1, &[], Box::new(FixedCount(10)));
        let err = add(&mut other).unwrap_err();
        assert!(format!("{err:#}").contains("needs its file"), "{err:#}");
        // A file whose pages came in embeds it.
        let info = file(other.store(), saved, "attached.pdf");
        insert_all(&mut other, info);
        let id = add(&mut other).unwrap();
        assert_eq!(&**file_held(&other, id).unwrap().bytes(), MARK_FILE_BYTES);
    }

    crate::both_ways!(blob_liveness, blob_liveness_appended: blobs_live);

    /// What marks hold lives as long as they do (M10.5 slice 8): a file
    /// attached and a library stamp placed, every blob the caller held
    /// dropped, the bytes stay through undo and redo, and both saves write
    /// them, the file reopened reading the same. The store counts them
    /// while a version holds the marks.
    fn blobs_live(how: How) {
        use crate::forms::fixture::{self, MARK_FILE_BYTES};
        let every = fixture::every_mark_kind();
        let mut d = new_doc(1, &[], Box::new(FixedCount(10)));
        let p0 = d.page(0).unwrap();
        let before = d.store().bytes();
        let [stamp, file] = fixture::mark_blobs(d.store());
        let held = stamp.len() + file.len();
        for annot in [&every[17], &every[23]] {
            d.commit_ops(vec![fx::add(p0, annot)], "add").unwrap();
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
        let back = reopened(how.write(&d));
        check(&back, back.page(0).unwrap());
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
        d.commit_ops(vec![fx::add(p1, &a)], "add").unwrap();
        let stored = d.state().marks_on(p1).next().unwrap().clone();
        assert_ne!(stored.id, AnnotId(0), "id assigned by the document");
        let blue = Op::PatchMarks {
            marks: vec![AnnotRef::Id(stored.id)],
            patch: sheaf_edit::MarkPatch {
                color: Some([0, 0, 255]),
                ..Default::default()
            },
            imported: None,
        };
        d.commit_ops(vec![blue], "upd").unwrap();
        assert_eq!(d.state().marks_on(p1).next().unwrap().color, [0, 0, 255]);
        d.commit_ops(vec![fx::duplicate(&[p1])], "dup").unwrap();
        let p1_copy = id_at(&d, 2);
        assert_ne!(d.state().marks_on(p1_copy).next().unwrap().id, stored.id);
        d.commit_ops(vec![fx::delete(&[p1_copy])], "del").unwrap();
        assert!(d.state().marks_on(p1_copy).next().is_none());
        d.commit_ops(vec![fx::delete_annots(&[stored.id])], "rm")
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
        let rotate = |by| fx::rotate(&[p0], by);
        d.commit_ops(vec![rotate(Turn::Quarter), rotate(Turn::Back)], "spin")
            .unwrap();
        let to = |before| fx::move_pages(&[p0], before);
        d.commit_ops(vec![to(None), to(Some(p1))], "there and back")
            .unwrap();
        let header = vec![
            fx::set_header(fx::header("Draft"), None),
            Op::RemoveHeaderFooter {
                pages: PageSel::All,
                include_authored: false,
            },
        ];
        d.commit_ops(header, "header").unwrap();
        let roman = fx::label(p0, LabelStyle::LowerRoman, "", 1);
        let labels = vec![
            Op::SetPageLabels {
                ranges: Some(vec![roman]),
            },
            Op::SetPageLabels { ranges: None },
        ];
        d.commit_ops(labels, "labels").unwrap();
        assert!(!d.can_undo());
        assert_eq!(d.version_count(), n);
    }

    #[test]
    fn a_batch_keeps_each_slot_s_change_on_its_last_step() {
        let mut d = doc();
        let page = id_at(&d, 0);
        let before = d.state().get(&Key::Geom(page));
        let rotate = fx::rotate(&[page], Turn::Quarter);
        d.commit_ops(vec![rotate.clone(), rotate], "rotate twice")
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
        let ops = vec![
            fx::move_pages(&[id_at(&d, 2), id_at(&d, 3)], Some(first)),
            fx::delete(&[second]),
        ];
        d.commit_ops(ops, "move and delete").unwrap();
        let last = d.timeline().pop().unwrap();
        let lines: Vec<&str> = last.edits.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(lines, ["Move pages 3, 4", "Delete page 4"]);
    }

    // ----- OCR text layers ---------------------------------------------------

    pub(super) fn tl(text: &str) -> Arc<TextLayer> {
        Arc::new(crate::text_layer::from_engine(
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
        let layers: Vec<_> = pages.iter().map(|&i| (id_at(d, i), tl(text))).collect();
        d.commit_ops(fx::ocr(layers), "ocr").unwrap();
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
        let (first, last) = (id_at(&d, 0), id_at(&d, 5));
        d.commit_ops(vec![fx::clear_ocr(&[first, last])], "clear")
            .unwrap();
        assert_eq!(layer_texts(&d), vec![(1, "a".into())]);
        // Clearing nothing is a no-op version-wise.
        let n = d.stats().versions;
        d.commit_ops(vec![fx::clear_ocr(&[last])], "clear").unwrap();
        assert_eq!(d.stats().versions, n);
    }

    #[test]
    fn unknown_page_is_an_error() {
        let mut d = doc();
        let ocr = fx::ocr([(PageId(999), tl("x"))]);
        assert!(d.commit_ops(ocr, "ocr").is_err());
    }

    #[test]
    fn layers_follow_delete_duplicate_replace() {
        let mut d = doc();
        set(&mut d, &[1, 2, 3], "a");
        let second = id_at(&d, 1);
        d.commit_ops(vec![fx::delete(&[second])], "del").unwrap();
        // Former pages 2 and 3 are now 1 and 2.
        assert_eq!(layer_texts(&d), vec![(1, "a".into()), (2, "a".into())]);
        let dup = id_at(&d, 1);
        d.commit_ops(vec![fx::duplicate(&[dup])], "dup").unwrap();
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
        let other = crate::store::blank(d.store(), 2);
        let replace = fx::replace(&[copy], &other, [0], "a.pdf");
        d.commit_ops(vec![replace], "replace").unwrap();
        assert_eq!(layer_texts(&d), vec![(1, "a".into()), (3, "a".into())]);
    }

    /// An OCR group's commits join one version: one undo step, called as
    /// its first commit is. Undone, its next commit is GroupEnded and
    /// changes nothing; redone, the group goes on.
    #[test]
    fn a_group_s_commits_coalesce() {
        let mut d = doc();
        let n = d.version_count();
        let g = d.begin_group(GroupKind::Ocr);
        for (i, page) in [0usize, 1, 2].iter().enumerate() {
            let layers = fx::ocr([(id_at(&d, *page), tl("a"))]);
            d.commit_ops_in_group(g, layers, &format!("OCR batch {i}"))
                .unwrap();
        }
        assert_eq!(d.version_count(), n + 1);
        assert_eq!(layer_texts(&d).len(), 3);
        assert_eq!(d.undo_label(), Some("OCR batch 0"));
        d.undo().unwrap();
        assert!(layer_texts(&d).is_empty());
        let next = |d: &Document| fx::ocr([(id_at(d, 3), tl("a"))]);
        let (at, layers) = (d.current_version(), next(&d));
        let e = d.commit_ops_in_group(g, layers, "OCR batch 3");
        assert_eq!(e.unwrap_err(), EditError::GroupEnded);
        assert_eq!(d.current_version(), at);
        assert!(layer_texts(&d).is_empty());
        d.redo().unwrap();
        let layers = next(&d);
        d.commit_ops_in_group(g, layers, "OCR batch 3").unwrap();
        assert_eq!(layer_texts(&d).len(), 4);
        assert_eq!(d.version_count(), n + 1, "amended");
    }

    /// A group's commit after its version was saved makes another version,
    /// which says it continues the group: the saved one stays as written.
    #[test]
    fn a_group_never_amends_a_saved_version() {
        let mut d = doc();
        let g = d.begin_group(GroupKind::Ocr);
        d.commit_ops_in_group(g, fx::ocr([(id_at(&d, 0), tl("a"))]), "ocr")
            .unwrap();
        let n = d.version_count();
        let v = d.current_version();
        d.save_input(v).unwrap();
        d.record_save(record_for(&d, v)).unwrap();
        d.commit_ops_in_group(g, fx::ocr([(id_at(&d, 1), tl("a"))]), "ocr")
            .unwrap();
        assert_eq!(d.version_count(), n + 1, "pushed, not amended");
        assert_eq!(d.undo_label(), Some("ocr (continued)"));
        assert!(d.is_dirty());
        d.undo().unwrap();
        assert!(!d.is_dirty(), "the saved version is intact");
        assert_eq!(layer_texts(&d).len(), 1);
    }

    /// An edit between a group's commits leaves it in fragments: the next
    /// commit makes another of its versions, which says it continues it,
    /// and undo takes them back one at a time. Undoing the latest ends the
    /// group, though the one before it is still there.
    #[test]
    fn a_group_s_commit_after_another_edit_continues_it() {
        let mut d = doc();
        let g = d.begin_group(GroupKind::Ocr);
        d.commit_ops_in_group(g, fx::ocr([(id_at(&d, 0), tl("a"))]), "ocr")
            .unwrap();
        let last = id_at(&d, 5);
        d.commit_ops(vec![fx::rotate(&[last], Turn::Quarter)], "rotate")
            .unwrap();
        let n = d.version_count();
        d.commit_ops_in_group(g, fx::ocr([(id_at(&d, 1), tl("a"))]), "ocr")
            .unwrap();
        assert_eq!(d.version_count(), n + 1);
        assert_eq!(d.undo_label(), Some("ocr (continued)"));
        let view = d.timeline().pop().unwrap();
        assert_eq!(view.group, Some((g, "ocr".into())));
        // Undo peels the second fragment, then the rotation, then the
        // first.
        d.undo().unwrap();
        assert_eq!(layer_texts(&d).len(), 1);
        assert_eq!(rotation(&d, 5), Rotation::R90);
        let layers = fx::ocr([(id_at(&d, 2), tl("a"))]);
        let e = d.commit_ops_in_group(g, layers, "ocr").unwrap_err();
        assert_eq!(e, EditError::GroupEnded);
    }

    /// A group's commit that changes nothing makes no version, and the
    /// group's first version is called as the first commit that makes one
    /// is, not as continuing it.
    #[test]
    fn a_group_s_first_no_op_pushes_nothing() {
        let mut d = doc();
        let g = d.begin_group(GroupKind::Ocr);
        let (n, at) = (d.version_count(), d.current_version());
        d.commit_ops_in_group(g, vec![fx::clear_ocr(&[id_at(&d, 0)])], "nothing")
            .unwrap();
        assert_eq!((d.version_count(), d.current_version()), (n, at));
        d.commit_ops_in_group(g, fx::ocr([(id_at(&d, 1), tl("a"))]), "ocr")
            .unwrap();
        assert_eq!(d.undo_label(), Some("ocr"));
    }

    /// A version names one state, so a group's later commit makes another
    /// version in place of the one it amends.
    #[test]
    fn an_ocr_amend_gives_the_cursor_a_new_version() {
        let mut d = doc();
        let parent = d.current_version();
        let g = d.begin_group(GroupKind::Ocr);
        let ocr = |d: &mut Document, page: usize| {
            d.commit_ops_in_group(g, fx::ocr([(id_at(d, page), tl("a"))]), "ocr")
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
        assert_eq!(d.undo().unwrap().map(|c| c.label).as_deref(), Some("ocr"));
        assert_eq!(d.current_version(), parent);
        assert_eq!(d.redo().unwrap().map(|c| c.label).as_deref(), Some("ocr"));
        assert_eq!(d.current_version(), v2);
        assert_eq!(layer_texts(&d).len(), 2);
        // The group goes on from the version in its version's place.
        ocr(&mut d, 2);
        assert_eq!(d.edge(d.current_version()).unwrap().len(), 3);
    }

    /// A save is an output: the version it wrote keeps its id, and is the
    /// saved one; the opened one too. Each save is noted on its version,
    /// and undo and redo across them are exact.
    #[test]
    fn a_save_keeps_the_version_it_wrote() {
        let mut d = doc();
        let opened = d.current_version();
        let before = d.state().clone();
        saved(&mut d);
        assert_eq!(d.current_version(), opened);
        rotate(&mut d, 0, "rotate");
        let v = d.current_version();
        let after = d.state().clone();
        saved(&mut d);
        assert_eq!(d.current_version(), v);
        assert!(!d.is_dirty());
        let timeline = d.timeline();
        let saved_ids: Vec<VersionId> = (timeline.iter())
            .filter(|view| view.saved)
            .map(|view| view.id)
            .collect();
        assert_eq!(saved_ids, [v]);
        let saves = |id| {
            timeline
                .iter()
                .find(|view| view.id == id)
                .unwrap()
                .saves
                .len()
        };
        assert_eq!((saves(opened), saves(v)), (1, 1));
        assert_eq!(
            d.undo().unwrap().map(|c| c.label).as_deref(),
            Some("rotate")
        );
        assert_eq!(d.current_version(), opened);
        assert!(*d.state() == before && d.is_dirty());
        assert_eq!(
            d.redo().unwrap().map(|c| c.label).as_deref(),
            Some("rotate")
        );
        assert_eq!(d.current_version(), v);
        assert!(*d.state() == after && !d.is_dirty());
    }

    /// A group whose commits put back all they did leaves no version: the
    /// parent is current again, as if they were never made, even when its
    /// snapshot was dropped. The group has no version, so its next commit's
    /// is its first.
    #[test]
    fn a_group_that_nets_to_nothing_leaves_no_version() {
        let mut d = new_doc(3, &[], Box::new(FixedCount(1)));
        rotate(&mut d, 2, "rotate");
        let (parent, want) = (d.current_version(), d.state().clone());
        let n = d.version_count();
        let g = d.begin_group(GroupKind::Ocr);
        let p0 = id_at(&d, 0);
        d.commit_ops_in_group(g, fx::ocr([(p0, tl("a"))]), "ocr")
            .unwrap();
        let v = d.current_version();
        assert!(d.versions[&parent].state.is_none(), "trimmed");
        let c = d.commit_ops_in_group(g, vec![fx::clear_ocr(&[p0])], "ocr");
        assert_eq!(c.unwrap().version, None);
        assert_eq!(d.current_version(), parent);
        assert_eq!(d.version_count(), n);
        assert!(d.jump(v).is_err());
        assert!(!d.can_redo());
        assert_eq!(*d.state(), want);
        d.commit_ops_in_group(g, fx::ocr([(p0, tl("b"))]), "again")
            .unwrap();
        assert_eq!(d.undo_label(), Some("again"));
    }

    /// An amend notes the sources its commit brings pages of with the
    /// version it joins (a plugin's copy of an inserted page), and a
    /// commit that changes nothing leaves that version as it is.
    #[test]
    fn a_group_s_amend_takes_its_sources_and_its_no_op_changes_nothing() {
        let mut d = doc();
        let s = crate::store::blank(d.store(), 1);
        d.commit_ops(vec![insert_of(&s, "a.pdf")], "insert")
            .unwrap();
        let inserted = id_at(&d, d.page_count() - 1);
        let g = d.begin_group(GroupKind::PluginCall {
            plugin: "copies".into(),
        });
        let (p0, p1, p2) = (id_at(&d, 0), id_at(&d, 1), id_at(&d, 2));
        d.commit_ops_in_group(g, vec![fx::rotate(&[p0], Turn::Quarter)], "copies")
            .unwrap();
        let copy = Op::DuplicatePages {
            pages: ids(&[inserted]),
            locals: Vec::new(),
        };
        d.commit_ops_in_group(g, vec![copy], "other").unwrap();
        let v = d.current_version();
        assert_eq!(d.edge(v).unwrap().len(), 2);
        assert_eq!(d.versions[&v].blobs, HashSet::from([s.id()]));
        d.commit_ops_in_group(g, vec![fx::move_pages(&[p1], Some(p2))], "other")
            .unwrap();
        assert_eq!(d.current_version(), v);
        assert_eq!(d.undo_label(), Some("copies"));
        assert_eq!(d.edge(v).unwrap().len(), 2, "no step added");
    }

    /// `d`'s history kept to just under what pruning can bring it to
    /// ([`HistoryStats::untrimmable_bytes`]): a version or a few go. Then
    /// it's kept to one snapshot, and nothing more is pruned.
    fn squeeze(d: &mut Document) {
        let budget = d.stats().untrimmable_bytes() - 1;
        let policy = AllOf(vec![
            Box::new(FixedCount(1)),
            Box::new(MemoryBudget(budget)),
        ]);
        d.set_policy(Box::new(policy));
        d.trim();
        d.set_policy(Box::new(FixedCount(1)));
    }

    /// The saved version goes last (M10.5 §8.7): versions on other
    /// branches go first, the saved one staying, until the first version
    /// kept moves past where its branch turns off. Then it detaches: the
    /// document is dirty and still knows its file, and a save's saved side
    /// is the saved version's state and record, its path keys the slots
    /// changed from there, through where the branches meet, to the version
    /// saved, which are judged by value.
    #[test]
    fn pruning_takes_the_saved_branch_last() {
        let mut d = doc();
        rotate(&mut d, 0, "saved");
        let saved = d.current_version();
        let saved_state = d.state().clone();
        d.record_save(record_for(&d, saved)).unwrap();
        let record = d.saved_record().unwrap().clone();
        d.undo().unwrap();
        rotate(&mut d, 1, "kept");
        let kept = d.current_version();
        rotate(&mut d, 2, "undone");
        let undone = d.current_version();
        d.undo().unwrap();
        rotate(&mut d, 0, "after");
        // The undone version goes first; the saved one stays.
        squeeze(&mut d);
        assert!(!d.versions.contains_key(&undone));
        assert_eq!(d.saved_at(), Some(saved));
        // Nothing else off the line: the first version moves on, past
        // where the saved version's branch turns off, and it detaches.
        assert!(d.prune_one());
        assert_eq!(d.root, kept);
        assert!(!d.versions.contains_key(&saved));
        assert!(d.saved_at().is_none() && d.is_dirty());
        assert!(Arc::ptr_eq(d.saved_record().unwrap(), &record));
        assert_eq!(d.file().unwrap().0, &record.path);
        assert!(d.timeline().iter().all(|v| !v.saved));
        let (p0, p1) = (id_at(&d, 0), id_at(&d, 1));
        let (keys, path) = path_of(&mut d);
        assert_eq!(keys, BTreeSet::from([Key::Geom(p0), Key::Geom(p1)]));
        assert_eq!(path.changed, BTreeSet::from([Key::Geom(p1)]));
        let v = d.current_version();
        assert_eq!(d.save_input(v).unwrap().saved.unwrap().state, saved_state);
        // The save makes the version it wrote the saved one: the anchor
        // goes.
        d.record_save(record_for(&d, v)).unwrap();
        assert!(!d.is_dirty() && d.saved_at() == Some(v));
    }

    /// Forgetting what's before the current version makes it the first:
    /// nothing to undo, redo still leading on, the saved version kept as
    /// an anchor, so the document is dirty and still knows its file, and
    /// its next save is planned against it. Not past a version being
    /// saved.
    #[test]
    fn history_before_the_cursor_is_forgotten() {
        let mut d = doc();
        rotate(&mut d, 0, "first");
        let saved = d.current_version();
        d.record_save(record_for(&d, saved)).unwrap();
        let record = d.saved_record().unwrap().clone();
        rotate(&mut d, 1, "second");
        let second = d.current_version();
        rotate(&mut d, 2, "third");
        let third = d.current_version();
        let state = d.state().clone();
        rotate(&mut d, 0, "fourth");
        let fourth = d.state().clone();
        d.undo().unwrap();
        // A version being saved holds it back: the first kept stops there.
        d.save_input(second).unwrap();
        assert!(!d.forget_before_cursor());
        assert_eq!(d.root, second);
        d.abandon_save(second);
        assert!(d.forget_before_cursor());
        assert_eq!(d.root, third);
        assert_eq!(d.state(), &state);
        assert!(!d.can_undo() && d.can_redo());
        assert!(!d.versions.contains_key(&saved));
        assert!(d.saved_at().is_none() && d.is_dirty());
        assert!(Arc::ptr_eq(d.saved_record().unwrap(), &record));
        let saved_side = d.save_input(third).unwrap().saved.unwrap();
        assert!(Arc::ptr_eq(&saved_side.record, &record));
        d.abandon_save(third);
        d.redo().unwrap();
        assert_eq!(d.state(), &fourth);
        d.undo().unwrap();
        assert!(d.forget_before_cursor(), "nothing to forget");
        assert_eq!(d.version_count(), 2);
    }

    /// The first version kept keeps its steps' edits, lines and keys, but
    /// not their prims: nothing is rebuilt into it. Its step bytes are
    /// counted again without them, the History panel still says what it
    /// did, and the versions after it are rebuilt from it.
    #[test]
    fn the_root_keeps_lines_not_prims() {
        let mut d = doc();
        rotate(&mut d, 0, "first");
        let first = d.current_version();
        let step = d.versions[&first].steps[0].clone();
        rotate(&mut d, 1, "second");
        rotate(&mut d, 2, "third");
        let third = d.current_version();
        let held = d.stats().step_bytes;
        let opened = d.root;
        assert!(d.prune_one());
        assert_eq!(d.root, first);
        assert!(!d.versions.contains_key(&opened));
        let v = &d.versions[&first];
        assert!(v.steps.iter().all(|s| s.prims.is_empty()));
        assert!(Arc::ptr_eq(&v.steps[0].op, &step.op));
        assert_eq!(v.steps[0].line, step.line);
        assert_eq!(v.keys, step.effects.keys);
        assert_eq!(v.step_bytes, steps_bytes(&v.steps, &v.keys));
        assert!(d.stats().step_bytes < held);
        let timeline = d.timeline();
        assert_eq!(timeline[0].id, first);
        assert_eq!(timeline[0].edits[0].text, step.line);
        // The others replay from it.
        d.set_policy(Box::new(FixedCount(1)));
        d.jump(first).unwrap();
        d.trim();
        d.jump(third).unwrap();
        assert_eq!(rotation(&d, 2), Rotation::R90);
        d.undo().unwrap();
        d.undo().unwrap();
        assert_eq!(d.current_version(), first);
        assert_eq!(rotation(&d, 0), Rotation::R90);
        assert!(!d.can_undo());
    }

    /// A layer of 300 words, as a page of a scan reads, which `seed` makes
    /// differ from others'.
    fn words(seed: usize) -> Arc<TextLayer> {
        let words = (0..300).map(|i| crate::text_layer::OcrWord {
            text: format!("w{}", seed * 300 + i),
            rect: crate::annot::Box2 {
                x: 72.0 + 46.0 * (i % 10) as f32,
                y: 708.0 - 20.0 * (i / 10) as f32,
                w: 42.0,
                h: 12.0,
            },
            confidence: 0.9,
        });
        let geom = crate::geom::Geom::read(None, None, 0);
        let layer = crate::text_layer::from_engine(words.collect(), &geom, "test");
        Arc::new(layer)
    }

    /// 200 OCR runs over every page, each a version, kept to a budget in
    /// proportion to the history gate's (256 MB for 1,000 pages): after
    /// each, what history holds fits, the first version kept moving on,
    /// and the versions left undo, each rebuilt from the one before.
    #[test]
    fn two_hundred_ocr_versions_keep_to_a_budget() {
        let pages = 10;
        let budget = (256 << 20) / 1000 * pages;
        let policy = AllOf(vec![
            Box::new(FixedCount(200)),
            Box::new(MemoryBudget(budget)),
        ]);
        let mut d = new_doc(pages as u32, &[], Box::new(policy));
        let ids: Vec<PageId> = d.state().order().iter().copied().collect();
        let opened = d.root;
        for run in 0..200 {
            let layers = (ids.iter().enumerate()).map(|(i, &p)| (p, words(run * pages + i)));
            d.commit_ops(fx::ocr(layers), "OCR").unwrap();
            let total = d.stats().total_bytes();
            assert!(total <= budget, "run {run}: {total} bytes of history");
        }
        assert!(!d.versions.contains_key(&opened));
        let kept = d.version_count();
        assert!((2..200).contains(&kept), "{kept} versions");
        // The first page's first word, as the run that set it named it.
        let text = |d: &Document| {
            let def = d.state().def(ids[0]).unwrap().clone();
            let words = def.content.ocr_text().map(|t| t.words.clone());
            words?.into_iter().map(|w| w.text).min()
        };
        assert_eq!(text(&d).as_deref(), Some("w597000"));
        d.set_policy(Box::new(FixedCount(1)));
        d.trim();
        for back in 1..kept {
            d.undo().unwrap();
            let want = format!("w{}", (199 - back) * pages * 300);
            assert_eq!(text(&d), Some(want), "{back} back");
        }
        assert!(!d.can_undo());
    }

    /// A save keeps the text layers the document draws: the file has
    /// them, and the pages still show the file it opened.
    #[test]
    fn a_save_keeps_text_layers() {
        let mut d = doc();
        set(&mut d, &[0], "a");
        let file = saved(&mut d);
        assert_eq!(layer_texts(&d).len(), 1);
        assert!(!d.is_dirty());
        let pdf = lopdf::Document::load_mem(file.bytes()).unwrap();
        let block = |o: &lopdf::Object| {
            let stream = o.as_stream().ok();
            stream.and_then(|s| crate::materialize::ocr_tests::ocr_block(&s.dict))
        };
        assert_eq!(pdf.objects.values().filter_map(block).count(), 1);
        d.undo().unwrap();
        assert!(layer_texts(&d).is_empty(), "pre-OCR version");
        d.redo().unwrap();
        assert_eq!(layer_texts(&d).len(), 1);
        assert!(!d.is_dirty());
    }

    #[test]
    fn history_counts_the_layer_a_step_holds() {
        let mut d = doc();
        let layer = tl(&"x".repeat(100_000));
        let p0 = id_at(&d, 0);
        d.commit_ops(fx::ocr([(p0, layer.clone())]), "ocr").unwrap();
        let stats = d.stats();
        assert!(stats.step_bytes >= layer.byte_estimate());
        assert!(
            stats.step_bytes < 2 * layer.byte_estimate(),
            "counted once, though the edit holds it too"
        );
        // The snapshot shares the layer: it adds only the map's nodes.
        assert!(*stats.snapshot_bytes.last().unwrap() < 4 << 10);
    }

    /// A step counts what its op holds as well as what it made: an ink
    /// mark's points, in the op and in the mark.
    #[test]
    fn a_step_counts_its_op_and_what_it_made() {
        use crate::annot::{AnnotKind, Pt};
        let mut d = doc();
        let points: Vec<Pt> = (0..10_000)
            .map(|i| Pt {
                x: i as f32,
                y: 0.0,
            })
            .collect();
        let bytes = points.len() * size_of::<Pt>();
        let ink = Annot {
            kind: AnnotKind::Ink {
                paths: vec![points],
            },
            ..square(1)
        };
        d.commit_ops(vec![fx::add(id_at(&d, 0), &ink)], "Draw")
            .unwrap();
        let step_bytes = d.stats().step_bytes;
        assert!(step_bytes > 2 * bytes, "{step_bytes}");
        assert!(step_bytes < 2 * bytes + (4 << 10), "{step_bytes}");
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
        d.commit_ops(vec![fx::add(page, &square(0))], "add")
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
        let rename = BookmarkPatch {
            title: Some("Renamed".into()),
            ..Default::default()
        };
        d.commit_ops(vec![fx::patch_bookmark(id, rename)], "Rename")
            .unwrap();
        let stats = d.stats();
        assert!(stats.step_bytes < 2 << 10, "{}", stats.step_bytes);
        let snapshot = *stats.snapshot_bytes.last().unwrap();
        assert!(snapshot < 4 << 10, "{snapshot}");
    }

    /// `bytes` as though read from the file `name`, parsed into `store`.
    pub(super) fn file(store: &Store, bytes: Vec<u8>, name: &str) -> Src {
        let kind = BlobKind::file(Path::new(name), None);
        store.open(bytes.into(), kind).unwrap()
    }

    /// An insert of every page of `src`, under `name`, at the end.
    pub(super) fn insert_of(src: &Src, name: &str) -> Op {
        Op::InsertPages {
            from: fx::file(src, name),
            pages: None,
            before: None,
            bring: Default::default(),
            locals: Vec::new(),
        }
    }

    /// All of `src`'s pages inserted at the end, as a new version; the
    /// caller lets go of it after.
    fn insert_all(d: &mut Document, src: Src) -> SourceId {
        d.commit_ops(vec![insert_of(&src, "a.pdf")], "insert")
            .unwrap();
        src.id()
    }

    #[test]
    fn an_undone_insert_counts_its_source_until_redo() {
        let mut d = doc();
        let primary = d.store().bytes();
        let blank = |d: &Document| crate::store::blank(d.store(), 2);
        let src = blank(&d);
        let sid = insert_all(&mut d, src);
        let len = sid.len as usize;
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
        // Undone again: the same bytes, inserted on the line by another
        // edit, share its blob.
        d.undo().unwrap();
        let again = blank(&d);
        assert_eq!(again.id(), sid);
        assert_eq!(d.stats().undone_source_bytes, len);
        d.commit_ops(vec![insert_of(&again, "a.pdf")], "insert")
            .unwrap();
        let stats = d.stats();
        assert_eq!(stats.undone_source_bytes, 0);
        assert_eq!(stats.blob_bytes, primary + len, "one blob");
    }

    /// An undone insert of a 400-page scan holds the scan's bytes until
    /// its branch is pruned: by bytes, at the next edit, once they're over
    /// the budget. Nothing else goes.
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
        let scan = crate::images::to_pdf(&rasters, sheaf_edit::Fit::LETTER).unwrap();
        let (len, before) = (scan.len(), d.store().bytes());
        let src = file(d.store(), scan, "scan.pdf");
        let id = insert_all(&mut d, src);
        assert_eq!((d.page_count(), d.store().bytes()), (403, before + len));
        d.undo().unwrap();
        assert_eq!(d.stats().undone_source_bytes, len);
        let policy = AllOf(vec![
            Box::new(FixedCount(1)),
            Box::new(MemoryBudget(len / 2)),
        ]);
        d.set_policy(Box::new(policy));
        // One edit branches off, and the branch goes.
        rotate(&mut d, 0, "rotate");
        assert_eq!(d.version_count(), 2);
        assert_eq!(d.store().bytes(), before, "its bytes went with it");
        assert!(d.store().get(id).is_none());
        let stats = d.stats();
        assert_eq!((stats.undone_source_bytes, stats.blob_bytes), (0, before));
    }

    #[test]
    fn the_root_advancing_keeps_its_sources() {
        let mut d = new_doc(3, &[], Box::new(FixedCount(1)));
        let one = crate::store::blank(d.store(), 1);
        let gone = insert_all(&mut d, one);
        d.undo().unwrap();
        let two = crate::store::blank(d.store(), 2);
        let kept = insert_all(&mut d, two);
        for i in 0..crate::history::MAX_VERSIONS {
            rotate(&mut d, i % 3, "rotate");
        }
        let first = &d.versions[&d.root];
        assert!(!first.steps.is_empty(), "pruned to a later one");
        assert_eq!(first.blobs, HashSet::from([kept]));
        assert!(d.store().get(gone).is_none(), "its branch was pruned");
        assert_eq!(d.page_count(), 5);
        let shown = d.state().pages().map(|(_, def, _)| def.base.src.id());
        assert!(
            shown
                .collect::<Vec<_>>()
                .iter()
                .all(|&s| d.store().get(s).is_some())
        );
        let bytes = d.primary().bytes().len() + kept.len as usize;
        assert_eq!(d.store().bytes(), bytes);
    }

    /// A record of `d`'s primary for a save of version `v`, as a Rewrite
    /// that wrote it would leave.
    pub(super) fn record_for(d: &Document, v: VersionId) -> SaveRecord {
        let stamp = FileStamp {
            len: d.primary().bytes().len() as u64,
            mtime: std::time::SystemTime::UNIX_EPOCH,
            tail: 0,
            window: 0,
        };
        SaveRecord {
            strategy: StrategyKind::Rewrite,
            ..SaveRecord::opened(v, d.primary(), Path::new("a.pdf").into(), stamp)
        }
    }

    /// An OCR flush for page `page` of `d`, in group `g`.
    fn ocr_in(d: &mut Document, g: GroupId, page: usize) {
        let layers = fx::ocr([(id_at(d, page), tl("a"))]);
        d.commit_ops_in_group(g, layers, "ocr").unwrap();
    }

    /// A save in flight seals its version (M10.5 §9.1): the group's next
    /// commit, as an OCR flush sends it, makes a version after it rather
    /// than amending what's being written, and that one stays dirty once
    /// the save is recorded.
    #[test]
    fn an_amend_on_a_sealed_version_makes_a_child() {
        let mut d = new_doc(3, &[], Box::new(FixedCount(10)));
        let g = d.begin_group(GroupKind::Ocr);
        ocr_in(&mut d, g, 0);
        let v = d.current_version();
        let input = d.save_input(v).unwrap();
        assert_eq!(input.version, v);
        assert_eq!(Some(&input.target), d.versions[&v].state.as_ref());
        ocr_in(&mut d, g, 1);
        let child = d.current_version();
        assert_ne!(child, v);
        assert_eq!(d.versions[&child].parent, Some(v));
        assert_eq!(d.edge(v).unwrap().len(), 1, "v is as it was sealed");
        d.record_save(record_for(&d, v)).unwrap();
        assert!(d.is_dirty());
        let undone = d.undo().unwrap().map(|c| c.label);
        assert_eq!(undone.as_deref(), Some("ocr (continued)"));
        assert_eq!(d.current_version(), v);
        assert!(!d.is_dirty());
    }

    /// `record_save` and `abandon_save` each end a save: the version is
    /// unsealed, so a group's next commit amends it again (an abandoned save
    /// leaves it unsaved). A recorded save makes it the saved version and
    /// holds its record, noted among its saves, and the version saved
    /// before keeps only its note.
    #[test]
    fn record_save_and_abandon_save_unseal() {
        let mut d = new_doc(3, &[], Box::new(FixedCount(10)));
        let g = d.begin_group(GroupKind::Ocr);
        ocr_in(&mut d, g, 0);
        let v = d.current_version();
        d.save_input(v).unwrap();
        d.save_input(v).unwrap();
        assert_eq!(d.versions[&v].sealed, 2);
        d.abandon_save(v);
        d.abandon_save(v);
        assert_eq!(d.versions[&v].sealed, 0);
        assert!(d.saved_record().is_none());
        ocr_in(&mut d, g, 1);
        let v = d.current_version();
        assert_eq!(d.edge(v).unwrap().len(), 2, "amended");
        d.save_input(v).unwrap();
        d.record_save(record_for(&d, v)).unwrap();
        assert_eq!(d.versions[&v].sealed, 0);
        assert!(!d.is_dirty());
        assert_eq!(d.saved_record().unwrap().version, v);
        let notes = &d.versions[&v].saves;
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].strategy, StrategyKind::Rewrite);
        assert_eq!(d.timeline().last().unwrap().saves, *notes);
        // Saved again after another edit: the record moves on.
        rotate(&mut d, 0, "rotate");
        let w = d.current_version();
        d.save_input(w).unwrap();
        let input = d.save_input(w).unwrap();
        assert_eq!(input.saved.unwrap().record.version, v);
        d.abandon_save(w);
        d.record_save(record_for(&d, w)).unwrap();
        assert_eq!(d.saved_record().unwrap().version, w);
        assert!(d.versions[&v].record.is_none());
        assert_eq!(d.versions[&v].saves.len(), 1);
    }

    /// Pruning never takes a version being saved, nor moves the first
    /// version kept past where a branch holding one turns off: under a
    /// budget nothing fits, everything stays until the save ends. Then all
    /// but the current version go, the saved one, made in memory, with
    /// nothing to keep of it.
    #[test]
    fn pruning_keeps_a_version_being_saved() {
        let mut d = new_doc(3, &[], Box::new(FixedCount(1)));
        rotate(&mut d, 0, "rotate");
        let sealed = d.current_version();
        d.save_input(sealed).unwrap();
        d.undo().unwrap();
        for i in 1..10 {
            rotate(&mut d, i % 3, "rotate");
        }
        let policy = AllOf(vec![Box::new(FixedCount(1)), Box::new(MemoryBudget(1))]);
        d.set_policy(Box::new(policy));
        d.trim();
        assert_eq!(d.version_count(), 11);
        assert!(d.versions[&sealed].state.is_some(), "its snapshot is kept");
        d.abandon_save(sealed);
        rotate(&mut d, 1, "rotate");
        assert_eq!(d.version_count(), 1);
        assert_eq!(d.root, d.current_version());
        assert!(d.saved.is_none() && d.is_dirty());
    }

    /// A file of four pages under two nodes of two, with every root
    /// object a save reads: an info dictionary, a form with XFA, an outline
    /// root, page labels, names and an open action.
    fn rooted() -> (Vec<u8>, BTreeMap<&'static str, ObjectId>) {
        use lopdf::{Object, dictionary};
        let mut pdf = lopdf::Document::with_version("1.7");
        let root = pdf.new_object_id();
        let mut ids = BTreeMap::new();
        let mut nodes = Vec::new();
        for n in 0..2 {
            let node = pdf.new_object_id();
            let pages: Vec<Object> = (0..2)
                .map(|_| {
                    pdf.add_object(dictionary! {
                        "Type" => "Page", "Parent" => node,
                        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
                    })
                    .into()
                })
                .collect();
            if n == 0 {
                ids.insert("page", pages[0].as_reference().unwrap());
            }
            pdf.objects.insert(
                node,
                dictionary! { "Type" => "Pages", "Parent" => root, "Kids" => pages, "Count" => 2 }
                    .into(),
            );
            nodes.push(Object::Reference(node));
        }
        ids.insert("node0", nodes[0].as_reference().unwrap());
        ids.insert("node1", nodes[1].as_reference().unwrap());
        pdf.objects.insert(
            root,
            dictionary! { "Type" => "Pages", "Kids" => nodes, "Count" => 4 }.into(),
        );
        let xfa = pdf.add_object(lopdf::Stream::new(dictionary! {}, b"<xdp/>".to_vec()));
        let form = pdf.add_object(dictionary! {
            "Fields" => Vec::<Object>::new(), "XFA" => xfa,
            "DA" => Object::string_literal("/Helv 0 Tf 0 g"),
        });
        let outlines = pdf.add_object(dictionary! { "Type" => "Outlines", "Count" => 0 });
        let labels = pdf.add_object(
            dictionary! { "Nums" => vec![0.into(), dictionary! { "S" => "r" }.into()] },
        );
        let names = pdf
            .add_object(dictionary! { "Dests" => dictionary! { "Names" => Vec::<Object>::new() } });
        let catalog = pdf.add_object(dictionary! {
            "Type" => "Catalog", "Pages" => root, "AcroForm" => form, "Outlines" => outlines,
            "PageLabels" => labels, "Names" => names, "Lang" => Object::string_literal("en"),
            "OpenAction" => vec![ids["page"].into(), "Fit".into()],
        });
        let info = pdf.add_object(dictionary! { "Title" => Object::string_literal("Rooted") });
        pdf.trailer.set("Root", catalog);
        pdf.trailer.set("Info", info);
        ids.extend([
            ("root", root),
            ("form", form),
            ("outlines", outlines),
            ("labels", labels),
            ("catalog", catalog),
            ("info", info),
        ]);
        let mut bytes = Vec::new();
        pdf.save_to(&mut bytes).unwrap();
        (bytes, ids)
    }

    /// The opened file's record (M10.5 §9.1): its objects at their own
    /// numbers, every number in use present, the root objects' keys the
    /// state doesn't own, and the page tree as the file has it. It's the
    /// opened version's, which `ready` replaced, and the saved one's.
    #[test]
    fn the_opened_record_is_the_file_s() {
        use crate::materialize::ObjKey;
        let (bytes, ids) = rooted();
        let store = Store::new();
        let primary = file(&store, bytes, "rooted.pdf");
        let src = primary.clone();
        let stamp = FileStamp {
            len: src.bytes().len() as u64,
            mtime: std::time::SystemTime::UNIX_EPOCH,
            tail: 7,
            window: 8,
        };
        let came = Came::Read {
            path: Path::new("rooted.pdf").into(),
            stamp,
        };
        let mut d = crate::open::start(
            store,
            primary,
            "rooted.pdf".into(),
            Some(came),
            Box::new(FixedCount(10)),
        );
        d.ready(crate::open::finish(&src, &in_progress::NoProgress))
            .unwrap();
        let rec = d.saved_record().unwrap().clone();
        assert_eq!(rec.version, d.current_version());
        assert_eq!(&*rec.path, Path::new("rooted.pdf"));
        assert_eq!((rec.strategy, rec.stamp), (StrategyKind::Opened, stamp));
        assert_eq!(rec.xref, *src.xref());
        assert!(rec.crypt.is_none() && rec.drawn.is_empty() && rec.transforms.is_empty());
        // The map: the root objects by key, and the rest by identity.
        let map = &rec.map;
        for (key, name) in [
            (ObjKey::Catalog, "catalog"),
            (ObjKey::Info, "info"),
            (ObjKey::AcroForm, "form"),
            (ObjKey::OutlinesRoot, "outlines"),
            (ObjKey::Labels, "labels"),
            (ObjKey::PagesNode(0), "root"),
            (ObjKey::PagesNode(1), "node0"),
            (ObjKey::PagesNode(2), "node1"),
        ] {
            assert_eq!(map.get(&key), Some(ids[name]), "{key:?}");
        }
        assert_eq!(map.get(&ObjKey::PagesNode(3)), None);
        let base = |blob, copy| ObjKey::Base {
            blob,
            obj: ids["page"],
            copy,
        };
        assert_eq!(map.get(&base(src.id(), 0)), Some(ids["page"]));
        assert_eq!(map.get(&base(src.id(), 1)), None);
        assert_eq!(map.get(&base(BlobId { hash: 1, len: 1 }, 0)), None);
        // Present: the numbers in use, and no other.
        let mut used: Vec<u32> = src.pdf().ids().map(|(n, _)| n).collect();
        used.sort_unstable();
        assert_eq!(rec.present.iter().collect::<Vec<_>>(), used);
        assert_eq!(rec.present.len(), used.len());
        let past = used.last().unwrap() + 1;
        assert!(!rec.present.contains(0) && !rec.present.contains(past));
        // Roots: the keys a save doesn't emit from the state.
        fn keys(d: &lopdf::Dictionary) -> Vec<&[u8]> {
            let mut keys: Vec<&[u8]> = d.iter().map(|(k, _)| k.as_slice()).collect();
            keys.sort();
            keys
        }
        let roots = &rec.roots;
        let want: [&[u8]; 4] = [b"Lang", b"Names", b"OpenAction", b"Type"];
        assert_eq!(keys(&roots.catalog), want);
        assert_eq!(keys(roots.acroform.as_ref().unwrap()), [b"XFA".as_slice()]);
        assert_eq!(keys(roots.outlines.as_ref().unwrap()), [b"Type".as_slice()]);
        assert_eq!(keys(roots.names.as_ref().unwrap()), [b"Dests".as_slice()]);
        // The tree, the root first, each node's kids in order.
        let tree: Vec<(ObjectId, usize)> =
            (rec.tree.nodes.iter()).map(|n| (n.id, n.size)).collect();
        assert_eq!(
            tree,
            [(ids["root"], 2), (ids["node0"], 2), (ids["node1"], 2)]
        );
        assert_eq!(rec.tree.nodes[0].kids, [ids["node0"], ids["node1"]]);
        let pages: Vec<ObjectId> = (rec.tree.nodes[1..].iter())
            .flat_map(|n| n.kids.iter().copied())
            .collect();
        assert_eq!(pages, src.page_ids().collect::<Vec<_>>());
        // A save's input after it holds it.
        let input = d.save_input(d.current_version()).unwrap();
        let saved = input.saved.unwrap();
        assert!(Arc::ptr_eq(&saved.record, d.saved_record().unwrap()));
        assert_eq!(saved.state, *d.state());
    }

    /// A primary made in memory, blank pages or a document `new` makes,
    /// has no record: nothing says what a file of it would hold, so its
    /// first save is a Rewrite.
    #[test]
    fn a_memory_primary_has_no_record() {
        let mut d = new_doc(3, &[], Box::new(FixedCount(10)));
        assert!(d.saved_record().is_none());
        rotate(&mut d, 0, "rotate");
        // Nor any path from a saved file.
        let v = d.current_version();
        let input = d.save_input(v).unwrap();
        assert!(input.saved.is_none() && input.path_keys.is_empty());
        assert!(d.recovery_input().saved.is_none());
        let detached = SaveInput::detached(&d.snapshot());
        assert!(detached.saved.is_none() && detached.path_keys.is_empty());
        let opened = opened_doc(2, Facts::default(), Box::new(FixedCount(10)));
        assert!(opened.saved_record().is_none());
    }

    /// `d`'s bookmark `title` titled `to`.
    fn retitle(d: &mut Document, title: &str, to: &str) {
        let patch = BookmarkPatch {
            title: Some(to.into()),
            ..Default::default()
        };
        let op = fx::patch_bookmark(bm(d, title), patch);
        d.commit_ops(vec![op], "retitle").unwrap();
    }

    /// What a save of the current version would find changed since the
    /// saved file: its path keys, and those judged by value.
    fn path_of(d: &mut Document) -> (BTreeSet<Key>, crate::effects::PathEffects) {
        let v = d.current_version();
        let input = d.save_input(v).unwrap();
        d.abandon_save(v);
        let saved = input.saved.expect("a saved side");
        let path = crate::effects::judge(&saved.state, &input.target, &input.path_keys);
        (BTreeSet::clone(&input.path_keys), path)
    }

    /// A save's path keys (M10.5 §7.2) run from the saved version back to
    /// the version both it and the one being saved come from, and on to
    /// the one being saved: every key a step on the way changed, and none
    /// from before.
    #[test]
    fn path_keys_run_through_where_the_branches_meet() {
        use crate::effects::Flags;
        let mut d = doc();
        rotate(&mut d, 0, "before");
        let fork = d.current_version();
        rotate(&mut d, 1, "saved");
        let saved = d.current_version();
        d.record_save(record_for(&d, saved)).unwrap();
        let (keys, path) = path_of(&mut d);
        assert!(keys.is_empty() && path.changed.is_empty());
        d.jump(fork).unwrap();
        retitle(&mut d, "B", "Bee");
        rotate(&mut d, 2, "after");
        let (p1, p2, b) = (id_at(&d, 1), id_at(&d, 2), bm(&d, "Bee"));
        let (keys, path) = path_of(&mut d);
        let want = BTreeSet::from([Key::Geom(p1), Key::Geom(p2), Key::Bookmark(b)]);
        assert_eq!(keys, want);
        assert_eq!(path.changed, want);
        let flags = Flags::GEOMETRY | Flags::OUTLINE | Flags::REPLACES_TEXT;
        assert_eq!(path.flags, flags);
        // From the saved version's side of the branch, the same keys.
        d.jump(saved).unwrap();
        rotate(&mut d, 1, "past it");
        let (keys, _) = path_of(&mut d);
        assert_eq!(keys, BTreeSet::from([Key::Geom(p1)]));
    }

    /// Back at the saved state by another route, a save finds nothing
    /// changed, since a path's keys are judged by value: what M10.5 §9.1
    /// plans as Unchanged. So does a value changed and put back.
    #[test]
    fn the_saved_state_by_another_route_changes_nothing() {
        let mut d = doc();
        rotate(&mut d, 0, "turn");
        let saved = d.current_version();
        d.record_save(record_for(&d, saved)).unwrap();
        // Undone, and turned again: another version, the same state.
        d.undo().unwrap();
        rotate(&mut d, 0, "turn again");
        assert_ne!(d.current_version(), saved);
        let (keys, path) = path_of(&mut d);
        assert_eq!(keys, BTreeSet::from([Key::Geom(id_at(&d, 0))]));
        assert!(path.changed.is_empty() && path.flags.is_empty());
        // x → y → x.
        retitle(&mut d, "B", "Bee");
        retitle(&mut d, "Bee", "B");
        let (keys, path) = path_of(&mut d);
        assert_eq!(keys.len(), 2);
        assert!(path.changed.is_empty() && path.removed.is_empty());
    }

    /// A recovery sidecar's input seals nothing: an OCR batch after it
    /// still amends the group's version.
    #[test]
    fn a_recovery_input_seals_nothing() {
        let mut d = new_doc(3, &[], Box::new(FixedCount(10)));
        let g = d.begin_group(GroupKind::Ocr);
        ocr_in(&mut d, g, 0);
        let v = d.current_version();
        assert_eq!(d.recovery_input().version, v);
        assert_eq!(d.versions[&v].sealed, 0);
        ocr_in(&mut d, g, 1);
        assert_eq!(d.edge(d.current_version()).unwrap().len(), 2, "amended");
    }

    #[test]
    fn apply_batch_is_one_undo_step() {
        let mut d = doc();
        let before = d.state().clone();
        let p0 = id_at(&d, 0);
        let p3 = id_at(&d, 3);
        let ops = vec![
            fx::bookmark(None, None, "W-2", Some(p0)),
            fx::bookmark(None, None, "1099-INT", Some(p3)),
            fx::rotate(&[p3], Turn::Quarter),
        ];
        d.commit_ops(ops, "Plugin edits").unwrap();
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
        let ops = vec![
            fx::delete(&[p0]),
            fx::move_pages(&[id_at(&d, 1)], Some(PageId(9999))),
        ];
        let err = d.commit_ops(ops, "bad");
        assert!(err.is_err());
        assert_eq!(*d.state(), before);
        assert!(!d.can_undo());
    }

    #[test]
    fn apply_batch_sums_outcomes() {
        let mut d = doc();
        let first = id_at(&d, 0);
        let ops = vec![fx::duplicate(&[first]), fx::duplicate(&[first])];
        let out = d.commit_ops(ops, "dup twice").unwrap();
        assert_eq!(out.created.ops.iter().map(Vec::len).sum::<usize>(), 2);
        assert_eq!(d.page_count(), 8);
    }

    #[test]
    fn insert_tree_nests_in_one_step() {
        let mut d = doc();
        let (p0, p3) = (id_at(&d, 0), id_at(&d, 3));
        let first_top = d.state().outline_roots()[0];
        // Under the bookmark `$parent` names, or among the top ones, at
        // `at`.
        let item =
            |title: &str, target: Option<PageId>, parent: Option<u16>, at, local| Op::AddBookmark {
                parent: parent.map(|l| BookmarkRef::New(Local(l))),
                at,
                title: title.into(),
                target: target.map(PageRef::Id),
                view: None,
                color: None,
                style: Default::default(),
                local: Some(Local(local)),
            };
        let after = |l| Sibling::After(BookmarkRef::New(Local(l)));
        let ops = vec![
            item("Accounts", None, None, fx::sibling(Some(first_top)), 1),
            item("…4821", Some(p0), Some(1), Sibling::Last, 2),
            item("…9930", Some(p3), Some(1), Sibling::Last, 3),
            item("Loose", Some(p3), None, after(1), 4),
        ];
        d.commit_ops(ops, "Bookmark matches").unwrap();
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
        // A child goes under a bookmark an op before it made.
        let ops = vec![
            item("x", None, None, Sibling::First, 1),
            item("y", None, Some(2), Sibling::Last, 3),
        ];
        let bad = d.commit_ops(ops, "bad");
        assert!(matches!(bad, Err(EditError::NotFound { .. })), "{bad:?}");
        assert_eq!(d.state().outline_roots().len(), 3);
    }
}
