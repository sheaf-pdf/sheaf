//! History's proof (M10.5 slice 1): over random runs of every kind of
//! op, in transactions of one op and of several, with locals, positions
//! and refs by name and title path, and with groups, undo, redo, jumps,
//! trims, pruning and saves, each version rebuilt from its prims is the
//! state it was made as, and every file it refers to is still in the store
//! (slice 3). Every state on the way keeps the records' invariants (slice
//! 8), a version being saved resolves to the state it was sealed with
//! until its save ends (slice 9), and a save of the current version would
//! find changed exactly what differs from the saved state (slice 10). Each
//! kind's no-op makes no version (slice 11 runs it all over ops). Each
//! version's keys are its prims', and a save's path keys, taken from them,
//! are those of the prims on its path, after what's kept of the saved
//! version once pruned; a transaction made on an earlier version lands on
//! the current one, checked by value or lowered again, or changes nothing;
//! a group's commits amend its version or continue it in fragments, or
//! find it ended and change nothing, and its versions are taken back by a
//! revert or a rollback, or a Conflict changes nothing; and what the
//! document counts of what history holds, kept as it changes, is what a
//! count finds (slice 12).

use std::collections::{BTreeMap, BTreeSet};

use in_progress::NoProgress;
use lopdf::{Object, ObjectId, dictionary};
use proptest::prelude::*;
use sheaf_edit::{
    Anchor, AnnotRef, BlobSel, BookmarkPatch, BookmarkRef, Bring, Delta, Imported, LabelStyle,
    Local, MarkKind, MarkPatch, NewMark, Op, OutlineBring, PageRef, PageSel, Patch, RangeSet,
    ScopeSel, Sibling, Style, Turn, Vec2,
};

use super::tests::{flat, opened_doc, square, tl};
use super::*;
use crate::annot::{Box2, Pt, REVIEW};
use crate::forms::fixture::Form;
use crate::history::{AllOf, FixedCount, MemoryBudget};
use crate::materialize::StrategyKind;
use crate::open::FileAnnots;
use crate::store::Blob;
use crate::tx::bookmarks::{kids_of, subtree};
use crate::tx::fixture as fx;
use crate::tx::{Base, Derive, JobKind, Made, OpResult, Stale};

/// The plugin whose calls the run's plugin groups are.
const PLUGIN: &str = "test-plugin";

/// The ops a run makes versions of, by name: below [`KINDS`], an action
/// is an op of the kind at its place here, the page rebase of a derived
/// flow last (`Tx::set_page_base`).
const OPS: [&str; 22] = [
    "move-pages",
    "rotate-pages",
    "delete-pages",
    "duplicate-pages",
    "insert-pages",
    "replace-pages",
    "set-page-labels",
    "add-bookmark",
    "patch-bookmarks",
    "move-bookmarks",
    "delete-bookmarks",
    "add-mark",
    "add-connector",
    "patch-marks",
    "move-marks",
    "delete-annotations",
    "adopt-annotations",
    "set-ocr-text",
    "clear-ocr-text",
    "set-header-footer",
    "remove-header-footer",
    "set-page-base",
];

/// A run's actions: below this, an op of the kind [`OPS`] names; then
/// these.
const KINDS: u8 = OPS.len() as u8;
const BATCH: u8 = KINDS;
const GROUP: u8 = KINDS + 1;
const UNDO: u8 = KINDS + 2;
const REDO: u8 = KINDS + 3;
const JUMP: u8 = KINDS + 4;
pub(crate) const TRIM: u8 = KINDS + 5;
pub(crate) const PRUNE: u8 = KINDS + 6;
const SAVE: u8 = KINDS + 7;
const SEAL: u8 = KINDS + 8;
const UNSEAL: u8 = KINDS + 9;
const STALE: u8 = KINDS + 10;
const REVERT: u8 = KINDS + 11;
const ROLLBACK: u8 = KINDS + 12;
const ACTIONS: u8 = KINDS + 13;

/// The kind of op `name` is.
fn kind(name: &str) -> u8 {
    let at = OPS.iter().position(|&op| op == name);
    at.unwrap_or_else(|| panic!("no op {name}")) as u8
}

/// A document, and what a run of actions did to it.
pub(crate) struct Run {
    pub(crate) d: Document,
    /// The opened file, and another with a form and marks, whose pages
    /// come in, each held by the run, as an op's caller holds what it
    /// inserts; and a stamp's page and an attachment's file, which marks
    /// hold.
    opened: Src,
    other: Src,
    held: [Blob; 2],
    /// The group the run's group actions commit in, until one begins
    /// another, or it ends.
    group: Option<RunGroup>,
    /// Each version a transaction made that still resolves, with its state
    /// then.
    made: Vec<(VersionId, DocState)>,
    /// The kinds of op that made a version.
    kinds: BTreeSet<&'static str>,
    /// Versions being saved, with the state each was sealed with.
    sealed: Vec<(VersionId, DocState)>,
    /// Every version the document has held, as it was made: pruning
    /// takes neither these nor their prims.
    ever: HashMap<VersionId, Was>,
}

/// A version as it was made: its parent, its group and its steps' prims.
struct Was {
    parent: Option<VersionId>,
    group: Option<GroupId>,
    prims: Vec<Arc<[Prim]>>,
}

/// A group a run commits in: its kind, and the version it began on, with
/// that version's state, which an OCR run's flushes are made on, as its
/// pages were rendered from it.
pub(crate) struct RunGroup {
    id: GroupId,
    kind: GroupKind,
    began: (VersionId, DocState),
}

/// What a transaction's ops made that its locals name, for the ops after
/// them to name.
#[derive(Default)]
struct Locals {
    named: Vec<(Local, Made)>,
    next: std::cell::Cell<u16>,
}

impl Locals {
    /// A local no op has named yet.
    fn fresh(&self) -> Local {
        self.next.set(self.next.get() + 1);
        Local(self.next.get())
    }

    /// The local naming `made`, if one does.
    fn of(&self, made: Made) -> Option<Local> {
        self.named.iter().find(|(_, m)| *m == made).map(|&(l, _)| l)
    }
}

/// An op, or the internal page rebase of a derived flow.
enum Gen {
    Op(Box<Op>),
    Base(Vec<(PageId, Src, u32)>),
}

/// Other tools' annotations on page `page` of `f`: a comment named
/// `name`, Acrobat's replace-text edit (a Caret, its StrikeOut grouped
/// under it), with a reply, and a link: its object.
pub(crate) fn with_theirs(f: &mut Form, page: usize, name: &str) -> ObjectId {
    let rect = |x: i64| vec![x.into(), 10.into(), (x + 20).into(), 30.into()];
    let caret = f.pdf.add_object(dictionary! {
        "Type" => "Annot", "Subtype" => "Caret", "Rect" => rect(10),
        "NM" => Object::string_literal(name), "Contents" => Object::string_literal("the"),
    });
    let strike = f.pdf.add_object(dictionary! {
        "Type" => "Annot", "Subtype" => "StrikeOut", "Rect" => rect(30),
        "QuadPoints" => vec![30.into(), 30.into(), 50.into(), 30.into(), 30.into(), 10.into(), 50.into(), 10.into()],
        "IRT" => caret, "RT" => "Group", "NM" => Object::string_literal(format!("{name}-strike")),
    });
    let reply = f.pdf.add_object(dictionary! {
        "Type" => "Annot", "Subtype" => "Text", "Rect" => rect(10), "IRT" => caret,
        "NM" => Object::string_literal(format!("{name}-reply")),
        "Contents" => Object::string_literal("a reply"),
    });
    let link = f.pdf.add_object(dictionary! {
        "Type" => "Annot", "Subtype" => "Link", "Rect" => rect(100),
    });
    let mut annots = f.annots(page);
    annots.extend([caret, strike, reply, link]);
    f.set_annots(page, &annots);
    link
}

/// sheaf's marks on the first two pages of `f`, as a save writes them: a
/// square with a reply to it and one end of a connector pair on the
/// first, the other end on the second.
pub(crate) fn with_ours(f: &mut Form) {
    let mut sq = square(1);
    sq.name = "ours-square".into();
    let reply = Annot {
        kind: AnnotKind::Reply {
            parent: sq.name.clone(),
        },
        ..square(2)
    };
    let [a, b] = [(3, 4), (4, 3)].map(|(id, partner)| super::tests::connector(id, partner));
    for (page, marks) in [(0, vec![sq, reply, a]), (1, vec![b])] {
        let id = f.pages[page];
        let mut dict = f.dict(id).clone();
        let marks = crate::forms::fixture::holding(&marks);
        crate::forms::fixture::write_marks(&mut f.pdf, &mut dict, &marks);
        *f.dict(id) = dict;
    }
}

/// The annotations of a file of six pages, each with other tools'
/// annotations ([`with_theirs`]), `theirs-1` on the first and every other
/// one after, `theirs-2` on the rest.
fn their_annots() -> Arc<FileAnnots> {
    let mut f = Form::new(6);
    for page in 0..6 {
        with_theirs(&mut f, page, ["theirs-1", "theirs-2"][page % 2]);
    }
    crate::forms::fixture::source(f.bytes()).annots().clone()
}

impl Run {
    /// Six pages with two bookmarks, a form and other tools' comments and
    /// links, beside a file with the same form, our marks and their
    /// comments and links on its two pages, keeping no snapshot it may
    /// drop.
    fn new() -> Self {
        let mut other = crate::forms::fixture::every_kind();
        with_theirs(&mut other, 0, "theirs-1");
        with_theirs(&mut other, 1, "theirs-2");
        with_ours(&mut other);
        let bytes = other.bytes();
        let facts = Facts {
            outline: flat(&[("A", Some(0), 0), ("B", Some(3), 0)]),
            form: crate::forms::fixture::source(bytes.clone()).form().clone(),
            annots: their_annots(),
            ..Default::default()
        };
        let d = opened_doc(6, facts, Box::new(FixedCount(1)));
        Self::over(d, bytes)
    }

    /// A run over `d`, as opened, beside the file `other`, a form, whose
    /// pages come in.
    pub(crate) fn over(d: Document, other: Vec<u8>) -> Self {
        let kind = BlobKind::file(Path::new("form.pdf"), None);
        let other = d.store().open(other.into(), kind).unwrap();
        let made = vec![(d.current_version(), d.state().clone())];
        Self {
            opened: d.primary().clone(),
            held: crate::forms::fixture::mark_blobs(d.store()),
            d,
            other,
            group: None,
            made,
            kinds: BTreeSet::new(),
            sealed: Vec::new(),
            ever: HashMap::new(),
        }
    }

    /// Runs `action`, its choices drawn from `seed`: the state it leaves
    /// keeps the invariants.
    pub(crate) fn act(&mut self, (action, seed): (u8, u64)) {
        let rng = &mut fastrand::Rng::with_seed(seed);
        self.note_versions();
        match action {
            0..KINDS => {
                let mut tx = self.d.tx(Actor::User);
                self.lower(&mut tx, action, rng, &mut Locals::default());
                self.record(|d| d.commit(tx, label("op")));
            }
            // Ops one after another in one transaction, each on what the
            // ones before it made, naming what they made by locals.
            BATCH => {
                let mut tx = self.d.tx(Actor::User);
                let mut locals = Locals::default();
                for _ in 0..rng.usize(2..=5) {
                    self.lower(&mut tx, rng.u8(..KINDS), rng, &mut locals);
                }
                self.record(|d| d.commit(tx, label("batch")));
            }
            GROUP => self.group_commits(rng),
            UNDO => {
                self.d.undo().unwrap();
            }
            REDO => {
                self.d.redo().unwrap();
            }
            JUMP => {
                let mut ids: Vec<VersionId> = self.d.versions.keys().copied().collect();
                ids.sort();
                self.d.jump(ids[rng.usize(..ids.len())]).unwrap();
            }
            TRIM => self.d.trim(),
            // History kept from now on to a budget drawn below what it
            // holds: pruned to it now, and again as edits come, the first
            // version kept moving on and the saved version detaching.
            PRUNE => {
                let budget = MemoryBudget(rng.usize(..=self.d.stats().total_bytes()));
                let policy = AllOf(vec![Box::new(FixedCount(1)), Box::new(budget)]);
                self.d.set_policy(Box::new(policy));
                self.d.trim();
            }
            // A save of the current version, start to end: an output, which
            // changes no version or state.
            SAVE => {
                let v = self.d.current_version();
                self.d.save_input(v).unwrap();
                self.saved(v);
            }
            SEAL => {
                let v = self.d.current_version();
                let input = self.d.save_input(v).unwrap();
                self.sealed.push((v, input.target));
            }
            UNSEAL if self.sealed.is_empty() => {}
            UNSEAL => {
                let (v, _) = self.sealed.remove(rng.usize(..self.sealed.len()));
                if rng.bool() {
                    self.saved(v);
                } else {
                    self.d.abandon_save(v);
                }
            }
            STALE => self.stale(rng),
            REVERT => self.revert(rng),
            ROLLBACK => self.rollback(rng),
            _ => unreachable!("action {action}"),
        }
        // A group the document ended, by pruning or a revert, takes no
        // more of the run's commits.
        if let Some(group) = &self.group
            && !self.d.groups.contains_key(&group.id)
        {
            self.group = None;
        }
        self.made.retain(|(id, _)| self.d.versions.contains_key(id));
        let st = self.d.state();
        assert_eq!(st.check_invariants(), Ok(()), "after action {action}");
        for (v, st) in &self.sealed {
            let kept = self.d.versions.get(v).and_then(|v| v.state.as_ref());
            assert_eq!(kept, Some(st), "{v:?}, sealed, after action {action}");
        }
        self.counted(action);
        // A save of the current version finds changed exactly the slots
        // that differ from the saved state: its path keys, the versions'
        // own, are their prims' along the path, after what the saved
        // version's anchor keeps once it was pruned, reach every one, and
        // are judged by value.
        let input = self.d.input(self.d.cursor);
        if let Some(saved) = &input.saved {
            let walked = match self.d.saved.as_ref().expect("the saved version") {
                SavedVersion::At(s) => walked_keys(&self.d, *s, self.d.cursor),
                SavedVersion::Detached(anchor) => {
                    let mut keys = anchor.keys_to_root.clone();
                    keys.extend(walked_keys(&self.d, self.d.root, self.d.cursor));
                    keys
                }
            };
            assert_eq!(*input.path_keys, walked, "after action {action}");
            let path = crate::effects::judge(&saved.state, &input.target, &input.path_keys);
            let differ = differing(&saved.state, &input.target);
            assert_eq!(path.changed, differ, "after action {action}");
            assert_eq!(path.flags.is_empty(), differ.is_empty(), "{:?}", path.flags);
        }
    }

    /// What the document counts as history holds is what it holds, counted
    /// again: the versions keeping snapshots and those snapshots' bytes,
    /// steps' bytes, and the undone sources', once counted, until something
    /// changed them; the versions it says are on the line are the line's;
    /// and the snapshots the policy weighs are in the order it drops them.
    fn counted(&self, action: u8) {
        let d = &self.d;
        let kept = || (d.versions.iter()).filter(|(_, v)| v.state.is_some());
        let held = &d.held;
        let snapshots = (
            kept().map(|(&id, _)| id).collect(),
            kept().map(|(_, v)| v.snapshot_bytes).sum(),
        );
        assert_eq!(
            (held.snapshots.clone(), held.snapshot_bytes),
            snapshots,
            "after action {action}"
        );
        let steps: usize = d.versions.values().map(|v| v.step_bytes).sum();
        assert_eq!(held.step_bytes, steps, "after action {action}");
        let line: BTreeSet<VersionId> = d.line().into_iter().collect();
        let on: BTreeSet<VersionId> = (d.versions.iter())
            .filter(|(_, v)| v.on_line)
            .map(|(&id, _)| id)
            .collect();
        assert_eq!(on, line, "after action {action}");
        if let Some(undone) = held.undone {
            assert_eq!(undone, d.undone_source_bytes(), "after action {action}");
        }
        // The policy's order: the snapshots it may drop, off the line and
        // then on it, each oldest first, then those that must stay, the
        // current version's last.
        let must = |id: &VersionId| {
            [d.root, d.cursor].contains(id)
                || d.saved_at() == Some(*id)
                || d.versions[id].sealed > 0
        };
        let (stay, may): (Vec<VersionId>, Vec<VersionId>) =
            held.snapshots.iter().copied().partition(must);
        let (on, off): (Vec<VersionId>, Vec<VersionId>) =
            may.into_iter().partition(|id| line.contains(id));
        let (cursor, stay): (Vec<VersionId>, Vec<VersionId>) =
            stay.into_iter().partition(|id| *id == d.cursor);
        let order = [off, on].concat();
        let bytes: Vec<usize> = (order.iter().chain(&stay).chain(&cursor))
            .map(|id| d.versions[id].snapshot_bytes)
            .collect();
        let (droppable, stats) = d.weigh(0);
        assert_eq!(droppable, order, "after action {action}");
        assert_eq!(stats.snapshot_bytes, bytes, "after action {action}");
        assert_eq!(
            (stats.droppable, stats.cursor),
            (order.len(), bytes.len() - 1),
            "after action {action}"
        );
    }

    /// A transaction made on a version made earlier that's still kept, as
    /// a planner or a dialog makes one, sometimes deciding by a page as it
    /// showed there, committed now: landed by value or lowered again. One
    /// that can't land changes nothing.
    fn stale(&mut self, rng: &mut fastrand::Rng) {
        let kept: Vec<&(VersionId, DocState)> = (self.made.iter())
            .filter(|(v, _)| self.d.versions.contains_key(v))
            .collect();
        let (v, st) = kept[rng.usize(..kept.len())].clone();
        let mut tx = self.tx_on(v, st, Actor::User, rng);
        let mut locals = Locals::default();
        for _ in 0..rng.usize(1..=2) {
            self.lower(&mut tx, rng.u8(..KINDS), rng, &mut locals);
        }
        let stale = match rng.bool() {
            true => Stale::Validate,
            false => Stale::Relower,
        };
        let opts = CommitOpts {
            base: Base::At(v),
            stale,
            ..label("stale")
        };
        self.record(|d| d.commit(tx, opts));
    }

    /// A transaction of `actor`'s on version `v`, whose state was `st`,
    /// sometimes deciding by a page as it showed there.
    fn tx_on(&self, v: VersionId, st: DocState, actor: Actor, rng: &mut fastrand::Rng) -> Tx {
        let order = st.order().clone();
        let snap = Snapshot {
            version: v,
            state: st,
            ..self.d.snapshot()
        };
        let mut tx = Document::tx_at(&snap, self.d.lower_cx(actor));
        if rng.bool() {
            tx.read(Key::Page(order[rng.usize(..order.len())]));
        }
        tx
    }

    /// A few commits in the run's group, or a new one's, in a row: an OCR
    /// run's flushes of recognized text, set and cleared, made on the
    /// version it began on and landed by value, as a run's are; a plugin
    /// call's batches, made on the current version. Later ones amend the
    /// group's version, or continue the group after another edit, and
    /// text set and cleared again can leave none. Once its edits were
    /// undone, the group has ended for the run.
    fn group_commits(&mut self, rng: &mut fastrand::Rng) {
        if self.group.is_none() || rng.u8(..4) == 0 {
            if let Some(old) = self.group.take() {
                self.d.end_group(old.id);
            }
            let of = match rng.bool() {
                true => GroupKind::Ocr,
                false => GroupKind::PluginCall {
                    plugin: PLUGIN.into(),
                },
            };
            let id = self.d.begin_group(of.clone());
            let began = (self.d.current_version(), self.d.state().clone());
            self.group = Some(RunGroup {
                id,
                kind: of,
                began,
            });
        }
        for _ in 0..rng.usize(1..=3) {
            let group = self.group.as_ref().expect("a group");
            let id = group.id;
            let (tx, opts) = match &group.kind {
                GroupKind::Ocr => {
                    let (v, st) = group.began.clone();
                    let mut tx = self.tx_on(v, st, Actor::Job(JobKind::Ocr), rng);
                    let k = match rng.u8(..4) {
                        0 => kind("clear-ocr-text"),
                        _ => kind("set-ocr-text"),
                    };
                    self.lower(&mut tx, k, rng, &mut Locals::default());
                    let opts = CommitOpts {
                        base: Base::At(v),
                        stale: Stale::Validate,
                        ..label("group")
                    };
                    (tx, opts)
                }
                GroupKind::PluginCall { plugin } => {
                    let actor = Actor::Plugin {
                        id: plugin.clone(),
                        version: "1.0".into(),
                    };
                    let mut tx = self.d.tx(actor);
                    self.lower(&mut tx, rng.u8(..KINDS), rng, &mut Locals::default());
                    (tx, label("group"))
                }
            };
            let opts = CommitOpts {
                group: Some(id),
                ..opts
            };
            if let Some(EditError::GroupEnded) = self.record(|d| d.commit(tx, opts)) {
                self.d.end_group(id);
                self.group = None;
                return;
            }
        }
    }

    /// A group with versions on the current line, as History offers to
    /// undo all of one, sometimes the run's own: none when there's none.
    fn some_group(&self, rng: &mut fastrand::Rng) -> Option<GroupId> {
        if let Some(group) = self.group.as_ref().filter(|_| rng.bool()) {
            return Some(group.id);
        }
        let mut tags: Vec<GroupId> = (self.d.line().iter())
            .filter_map(|id| Some(self.d.versions[id].group.as_ref()?.0))
            .collect();
        tags.dedup();
        (!tags.is_empty()).then(|| tags[rng.usize(..tags.len())])
    }

    /// History's "Undo all of …" of a group: a version after the current
    /// one, which its prims rebuild and which takes all of the group back,
    /// or a refusal that changes nothing.
    fn revert(&mut self, rng: &mut fastrand::Rng) {
        if let Some(g) = self.some_group(rng) {
            let at = self.d.current_version();
            let mut change = None;
            self.record(|d| {
                let commit = d.revert_group(g, "group")?;
                change = Some(commit.change.clone());
                Ok(commit)
            });
            if let Some(change) = change {
                self.taken_back(g, at, &change);
            }
            self.replays();
        }
    }

    /// A group rolled back, as after a plugin call fails: its only version
    /// taken out, or its versions taken back by one after the current one,
    /// which its prims rebuild; or a refusal that changes nothing.
    fn rollback(&mut self, rng: &mut fastrand::Rng) {
        let Some(g) = self.some_group(rng) else {
            return;
        };
        let at = self.d.current_version();
        if let Ok(change) = self.landed(|d| d.rollback_group(g)) {
            if self.d.current_version() != at {
                self.moved();
            }
            if let Some(change) = change {
                self.taken_back(g, at, &change);
            }
        }
        self.replays();
    }

    /// After `change` took group `g`'s versions on version `at`'s line
    /// back: every slot one of them wrote, those pruning took included,
    /// holds what it held before the first of them wrote it, and it
    /// changed no other slot.
    fn taken_back(&self, g: GroupId, at: VersionId, change: &ChangeSet) {
        let mut first = BTreeMap::new();
        let mut on = Some(at);
        while let Some(id) = on {
            let was = &self.ever[&id];
            if was.group == Some(g) {
                for p in was.prims.iter().rev().flat_map(|ps| ps.iter().rev()) {
                    first.insert(p.key, p.before.clone());
                }
            }
            on = was.parent;
        }
        let st = self.d.state();
        for (key, before) in &first {
            assert!(same(st.get(key).as_ref(), before.as_ref()), "{key:?}");
        }
        assert!(change.keys.iter().all(|k| first.contains_key(k)));
    }

    /// Each version the document holds that the run hasn't seen yet, kept
    /// as it was made.
    fn note_versions(&mut self) {
        for (&id, v) in &self.d.versions {
            self.ever.entry(id).or_insert_with(|| Was {
                parent: v.parent,
                group: v.group.as_ref().map(|(g, _)| *g),
                prims: v.steps.iter().map(|s| s.prims.clone()).collect(),
            });
        }
    }

    /// The current version, rebuilt from the nearest version before it
    /// that keeps its state by its prims, unless it's the first: the state
    /// it has.
    fn replays(&self) {
        let at = self.d.current_version();
        if self.d.versions[&at].parent.is_some() {
            assert_eq!(self.d.replay(at).as_ref(), Ok(self.d.state()), "{at:?}");
        }
    }

    /// A save of version `v`, sealed, wrote a file: `v` is the saved one,
    /// with a record of it.
    fn saved(&mut self, v: VersionId) {
        let stamp = FileStamp {
            len: 0,
            mtime: std::time::SystemTime::UNIX_EPOCH,
            tail: 0,
            window: 0,
        };
        let path = Path::new("a.pdf").into();
        let rec = SaveRecord {
            strategy: StrategyKind::Rewrite,
            ..SaveRecord::opened(v, self.d.primary(), path, stamp)
        };
        self.d.record_save(rec).unwrap();
        assert_eq!(self.d.saved_at(), Some(v));
    }

    /// Runs `commit`, a transaction's, a revert's. When it made a version,
    /// or put a new one in the cursor's place, records it, and the kinds of
    /// its steps' ops. Why it failed, if it did.
    fn record(
        &mut self,
        commit: impl FnOnce(&mut Document) -> Result<Commit, EditError>,
    ) -> Option<EditError> {
        let at = self.d.current_version();
        let failed = self.landed(commit).err();
        if failed.is_none() && self.d.current_version() != at {
            self.moved();
        }
        failed
    }

    /// Runs `change`, a transition: one that fails leaves the document as
    /// it was, and isn't a bug.
    fn landed<T>(
        &mut self,
        change: impl FnOnce(&mut Document) -> Result<T, EditError>,
    ) -> Result<T, EditError> {
        let was = |d: &Document| (d.cursor, d.version_count(), d.state().clone());
        let before = was(&self.d);
        let r = change(&mut self.d);
        if let Err(e) = &r {
            let bug = matches!(e, EditError::Internal { .. } | EditError::Mismatch { .. });
            assert!(!bug, "{e}");
            assert_eq!(was(&self.d), before, "{e}");
        }
        self.note_versions();
        r
    }

    /// The cursor moved to a version a transition made, or to one before
    /// it: records the version, unless it was, and the kinds of its steps'
    /// ops. An amend replaces the version it joins, and a group that put
    /// back all it did, or a rollback, removes it: those are no longer
    /// kept.
    fn moved(&mut self) {
        self.made.retain(|(id, _)| self.d.versions.contains_key(id));
        let id = self.d.current_version();
        if self.made.iter().all(|(v, _)| *v != id) {
            self.made.push((id, self.d.state().clone()));
            let steps = self.d.edge(id).unwrap_or_default();
            self.kinds.extend(steps.iter().map(|s| s.op.name()));
        }
    }

    /// An op of kind `kind` on what `tx`'s ops have made so far, drawn from
    /// `rng`, lowered into it; what it makes that it names, noted in
    /// `locals`. One that fails leaves `tx` as it was.
    fn lower(&self, tx: &mut Tx, kind: u8, rng: &mut fastrand::Rng, locals: &mut Locals) {
        match self.op(tx.state(), kind, rng, locals) {
            Gen::Base(pages) => {
                let _ = tx.set_page_base(pages, Derive::Crop);
            }
            Gen::Op(op) => {
                let names = declared(&op);
                if let Ok(OpResult { made }) = tx.apply(*op, &NoProgress) {
                    locals.named.extend(names.into_iter().zip(made));
                }
            }
        }
    }

    /// An op of kind `kind` against `st`, what it acts on drawn from `rng`;
    /// sometimes naming what it makes with locals from `locals`, and what
    /// earlier ops made by theirs. A kind with nothing to act on is one
    /// that has: a patch of marks with no mark adds one, and so on.
    fn op(&self, st: &DocState, kind: u8, rng: &mut fastrand::Rng, locals: &Locals) -> Gen {
        let pages: Vec<PageId> = st.order().iter().copied().collect();
        let marks: Vec<(PageId, &Annot)> = pages
            .iter()
            .flat_map(|&p| st.marks_on(p).map(move |a| (p, a)))
            .collect();
        let foreign: Vec<(PageId, AnnotId, &Foreign)> = pages
            .iter()
            .flat_map(|&p| st.foreign_on(p).map(move |(id, f)| (p, id, &**f)))
            .collect();
        let adoptable: Vec<(PageId, AnnotId)> = (foreign.iter())
            .filter(|(_, _, f)| f.summary.mark.is_some() && !f.summary.is_link())
            .map(|&(page, id, _)| (page, id))
            .collect();
        let layered: Vec<PageId> = pages
            .iter()
            .copied()
            .filter(|&p| st.ocr_text(p).is_some())
            .collect();
        let preorder = st.bookmarks_preorder();
        let bookmarks: Vec<OutlineId> = preorder.iter().map(|&(_, id, _)| id).collect();
        let kind = match OPS[kind as usize] {
            "delete-pages" if pages.len() <= 2 => "insert-pages",
            "patch-bookmarks" | "delete-bookmarks" if bookmarks.is_empty() => "add-bookmark",
            // One alone has nowhere else to go.
            "move-bookmarks" if bookmarks.len() < 2 => "add-bookmark",
            "patch-marks" | "move-marks" if marks.is_empty() => "add-mark",
            "delete-annotations" if marks.is_empty() && foreign.is_empty() => "add-mark",
            "adopt-annotations" if adoptable.is_empty() => "add-mark",
            "clear-ocr-text" if layered.is_empty() => "set-ocr-text",
            "remove-header-footer" if st.generators().is_empty() => "set-header-footer",
            k => k,
        };
        let page = |rng: &mut fastrand::Rng| pages[rng.usize(..pages.len())];
        let some = |rng: &mut fastrand::Rng| {
            let mut ids = vec![page(rng)];
            if rng.bool() {
                ids.push(page(rng));
            }
            ids
        };
        // A page by its id, its position, or the local naming it.
        let page_by = |rng: &mut fastrand::Rng, id: PageId| match rng.u8(..6) {
            0 => PageRef::Pos(st.pos_of(id).expect("a page") as u32 + 1),
            1 => locals
                .of(Made::Page(id))
                .map_or(PageRef::Id(id), PageRef::New),
            _ => PageRef::Id(id),
        };
        // Pages, as an op selects them: by ref or by position; `widen`ing
        // them, as the span from one to the other, or as a bookmark's.
        let select = |rng: &mut fastrand::Rng, ids: &[PageId], widen: bool| -> PageSel {
            let pos = |id: &PageId| st.pos_of(*id).expect("a page") as u32 + 1;
            match rng.u8(..8) {
                0 => {
                    let runs = ids.iter().map(|id| pos(id)..=pos(id));
                    PageSel::Positions(RangeSet::from_runs(runs).expect("a page"))
                }
                1 if widen && ids.len() == 2 => {
                    let (first, last) = match pos(&ids[0]) <= pos(&ids[1]) {
                        true => (ids[0], ids[1]),
                        false => (ids[1], ids[0]),
                    };
                    PageSel::Span {
                        first: page_by(rng, first),
                        last: page_by(rng, last),
                    }
                }
                2 if widen && !bookmarks.is_empty() => {
                    let b = bookmarks[rng.usize(..bookmarks.len())];
                    PageSel::Bookmark(bookmark_ref(st, rng, b, locals))
                }
                _ => PageSel::Ids(ids.iter().map(|&id| page_by(rng, id)).collect()),
            }
        };
        let any_page = |rng: &mut fastrand::Rng| {
            let id = page(rng);
            page_by(rng, id)
        };
        let some_pages = |rng: &mut fastrand::Rng| {
            let ids = some(rng);
            select(rng, &ids, true)
        };
        let mark = |rng: &mut fastrand::Rng| marks[rng.usize(..marks.len())];
        let bookmark = |rng: &mut fastrand::Rng| bookmarks[rng.usize(..bookmarks.len())];
        let any_bookmark = |rng: &mut fastrand::Rng| {
            let id = bookmark(rng);
            bookmark_ref(st, rng, id, locals)
        };
        // Under a bookmark or at the top, first, last or after one there;
        // for bookmarks `moving`, outside them.
        let place = |rng: &mut fastrand::Rng, moving: &[OutlineId]| {
            let outside = |id: OutlineId| moving.iter().all(|&m| !subtree(st, m).contains(&id));
            let parents: Vec<OutlineId> = bookmarks
                .iter()
                .copied()
                .filter(|&id| outside(id))
                .collect();
            let parent =
                (!parents.is_empty() && rng.bool()).then(|| parents[rng.usize(..parents.len())]);
            let siblings: Vec<OutlineId> = kids_of(st, parent)
                .expect("a bookmark")
                .into_iter()
                .filter(|&id| outside(id))
                .collect();
            let at = match rng.u8(..3) {
                0 => Sibling::First,
                1 if !siblings.is_empty() => {
                    let after = siblings[rng.usize(..siblings.len())];
                    Sibling::After(bookmark_ref(st, rng, after, locals))
                }
                _ => Sibling::Last,
            };
            let parent = parent.map(|p| bookmark_ref(st, rng, p, locals));
            (parent, at)
        };
        // A file pages come from, and the op's name for it.
        let source = |rng: &mut fastrand::Rng| {
            let (src, name) = match rng.bool() {
                true => (&self.other, "form.pdf"),
                false => (&self.opened, "a.pdf"),
            };
            (src, fx::file(src, name))
        };
        // A few of its pages, from 1: a saved file has all the document's,
        // and bringing in all of them doubles it.
        let within = |rng: &mut fastrand::Rng, count: u32| -> RangeSet {
            let start = rng.u32(1..=count);
            let end = rng.u32(start..=count.min(start + 3));
            RangeSet::from_runs(std::iter::once(start..=end)).expect("a page")
        };
        let bring = |rng: &mut fastrand::Rng| Bring {
            marks: rng.u8(..4) > 0,
            outline: match rng.bool() {
                true => OutlineBring::MergeByTitle,
                false => OutlineBring::None,
            },
            ..Bring::default()
        };
        let view = |rng: &mut fastrand::Rng| DestView::Xyz {
            left: Some(rng.f32() * 100.0),
            top: Some(rng.f32() * 100.0),
            zoom: None,
        };
        // An annotation by its id, its name on its page, or its local.
        let annot_ref =
            |rng: &mut fastrand::Rng, page: PageId, id: AnnotId, nm: String| match rng.u8(..4) {
                0 => AnnotRef::Nm {
                    nm,
                    page: Some(page_by(rng, page)),
                },
                1 => locals
                    .of(Made::Annot(id))
                    .map_or(AnnotRef::Id(id), AnnotRef::New),
                _ => AnnotRef::Id(id),
            };
        let some_marks = |rng: &mut fastrand::Rng| -> Vec<AnnotRef> {
            (0..rng.usize(1..=2))
                .map(|_| {
                    let (page, a) = mark(rng);
                    annot_ref(rng, page, a.id, a.name())
                })
                .collect()
        };
        let made = |rng: &mut fastrand::Rng, n: usize| -> Vec<Local> {
            match rng.bool() {
                true => (0..n).map(|_| locals.fresh()).collect(),
                false => Vec::new(),
            }
        };
        let style = |rng: &mut fastrand::Rng| Style {
            color: [rng.u8(..), 0, 0],
            opacity: 1.0,
            stroke: 1.0,
            fill: None,
            dashed: rng.u8(..4) == 0,
            cloudy: false,
        };
        Gen::Op(Box::new(match kind {
            "move-pages" => {
                let ids = some(rng);
                let rest: Vec<PageId> =
                    pages.iter().copied().filter(|p| !ids.contains(p)).collect();
                let before = (!rest.is_empty() && rng.bool()).then(|| {
                    let id = rest[rng.usize(..rest.len())];
                    page_by(rng, id)
                });
                Op::MovePages {
                    pages: select(rng, &ids, true),
                    before,
                }
            }
            "rotate-pages" => Op::RotatePages {
                pages: match rng.u8(..8) {
                    0 => PageSel::All,
                    _ => some_pages(rng),
                },
                by: [Turn::Quarter, Turn::Half, Turn::Back][rng.usize(..3)],
            },
            "delete-pages" => {
                let id = page(rng);
                Op::DeletePages {
                    pages: select(rng, &[id], false),
                }
            }
            "duplicate-pages" => {
                let ids = some(rng);
                Op::DuplicatePages {
                    pages: PageSel::Ids(ids.iter().map(|&id| page_by(rng, id)).collect()),
                    locals: made(rng, 1),
                }
            }
            "insert-pages" => {
                let (src, from) = source(rng);
                let pages = within(rng, src.page_count());
                let n = pages.iter().count().min(2);
                Op::InsertPages {
                    from,
                    pages: Some(pages),
                    before: rng.bool().then(|| any_page(rng)),
                    bring: bring(rng),
                    locals: made(rng, n),
                }
            }
            "replace-pages" => {
                let (src, from) = source(rng);
                // With two pages left, no more pages replaced than there
                // are replacements, so none goes.
                let ids = match pages.len() > 2 {
                    true => some(rng),
                    false => vec![page(rng)],
                };
                Op::ReplacePages {
                    pages: ids.iter().map(|&id| page_by(rng, id)).collect(),
                    from,
                    with: Some(within(rng, src.page_count())),
                    bring: bring(rng),
                }
            }
            "set-page-labels" => Op::SetPageLabels {
                ranges: match rng.usize(..3) {
                    0 => None,
                    n => Some(
                        (0..n)
                            .map(|_| sheaf_edit::PageLabelRange {
                                start: any_page(rng),
                                style: [
                                    LabelStyle::Decimal,
                                    LabelStyle::LowerRoman,
                                    LabelStyle::UpperLetters,
                                ][rng.usize(..3)],
                                prefix: if rng.bool() {
                                    "A-".into()
                                } else {
                                    String::new()
                                },
                                first: rng.u32(1..5),
                            })
                            .collect(),
                    ),
                },
            },
            "add-bookmark" => {
                // Under one an earlier op made, sometimes, as a tree is
                // made in one transaction.
                let made_one = (locals.named.iter())
                    .filter(|(_, m)| matches!(m, Made::Bookmark(_)))
                    .map(|&(l, _)| l)
                    .next_back();
                let (parent, at) = match made_one {
                    Some(l) if rng.bool() => (Some(BookmarkRef::New(l)), Sibling::Last),
                    _ => place(rng, &[]),
                };
                Op::AddBookmark {
                    parent,
                    at,
                    title: format!("b{}", rng.u8(..)),
                    target: rng.bool().then(|| any_page(rng)),
                    view: rng.bool().then(|| view(rng)),
                    color: rng.bool().then_some([0, 0, 255]),
                    style: TextStyle {
                        bold: rng.bool(),
                        italic: rng.u8(..4) == 0,
                    },
                    local: rng.bool().then(|| locals.fresh()),
                }
            }
            "patch-bookmarks" => {
                let mut patch = BookmarkPatch::default();
                if rng.bool() {
                    patch.title = Some(format!("e{}", rng.u8(..)));
                }
                patch.target = match rng.u8(..3) {
                    0 => Patch::Clear,
                    1 => Patch::Set(any_page(rng)),
                    _ => Patch::Keep,
                };
                patch.view = match rng.u8(..3) {
                    0 => Patch::Clear,
                    1 => Patch::Set(view(rng)),
                    _ => Patch::Keep,
                };
                patch.color = match rng.u8(..3) {
                    0 => Patch::Clear,
                    1 => Patch::Set([255, 0, 0]),
                    _ => Patch::Keep,
                };
                if rng.bool() {
                    patch.style = Some(TextStyle {
                        bold: rng.bool(),
                        italic: rng.bool(),
                    });
                }
                Op::PatchBookmarks {
                    bookmarks: (0..rng.usize(1..=2)).map(|_| any_bookmark(rng)).collect(),
                    patch,
                }
            }
            "move-bookmarks" => {
                let id = bookmark(rng);
                let (parent, at) = place(rng, &[id]);
                Op::MoveBookmarks {
                    bookmarks: vec![bookmark_ref(st, rng, id, locals)],
                    parent,
                    at,
                    with_pages: rng.bool(),
                }
            }
            "delete-bookmarks" => Op::DeleteBookmarks {
                bookmarks: (0..rng.usize(1..=2)).map(|_| any_bookmark(rng)).collect(),
            },
            "add-mark" => {
                let page_id = page(rng);
                // What's on the page that a reply or status may go to.
                let threads: Vec<(AnnotId, String)> = (st.annots_on(page_id))
                    .filter(|(_, a)| a.widget().is_none())
                    .filter(|(_, a)| a.foreign().is_none_or(|f| !f.summary.is_link()))
                    .map(|(id, a)| (id, a.name()))
                    .collect();
                let rect = Box2 {
                    x: 10.0 + rng.f32() * 300.0,
                    y: 10.0 + rng.f32() * 500.0,
                    w: 30.0,
                    h: 20.0,
                };
                let (kind, reply_to, asset) = match rng.u8(..8) {
                    0 | 1 if !threads.is_empty() => {
                        let (id, nm) = threads[rng.usize(..threads.len())].clone();
                        let kind = match rng.bool() {
                            true => MarkKind::Reply {},
                            false => MarkKind::Status {
                                model: REVIEW.into(),
                                state: "Accepted".into(),
                            },
                        };
                        (kind, Some(annot_ref(rng, page_id, id, nm)), None)
                    }
                    2 => (
                        MarkKind::Note {
                            pos: Pt {
                                x: rect.x,
                                y: rect.y,
                            },
                        },
                        None,
                        None,
                    ),
                    3 => (MarkKind::Highlight { rects: vec![rect] }, None, None),
                    4 if rng.bool() => (
                        MarkKind::PageStamp {
                            rect,
                            name: "Approved".into(),
                            text: None,
                        },
                        None,
                        Some(BlobSel::Id(self.held[0].id())),
                    ),
                    4 => (
                        MarkKind::FileAttachment {
                            pos: Pt {
                                x: rect.x,
                                y: rect.y,
                            },
                            icon: "Paperclip".into(),
                            name: "notes.txt".into(),
                        },
                        None,
                        Some(BlobSel::Id(self.held[1].id())),
                    ),
                    _ => (MarkKind::Square { rect }, None, None),
                };
                let imported = (rng.u8(..6) == 0).then(|| Imported {
                    nm: format!("imported-{}", rng.u32(..)),
                    created: "D:20260101000000Z".into(),
                    modified: "D:20260102000000Z".into(),
                    author: "Them".into(),
                });
                Op::AddMark {
                    page: page_by(rng, page_id),
                    mark: NewMark {
                        kind,
                        style: style(rng),
                        contents: format!("c{}", rng.u8(..)),
                        author: None,
                        turn: [Rotation::R0, Rotation::R90][rng.usize(..2)],
                        asset,
                    },
                    reply_to,
                    local: rng.bool().then(|| locals.fresh()),
                    imported,
                }
            }
            "add-connector" => {
                let anchor = |rng: &mut fastrand::Rng, x: f32| Anchor {
                    page: any_page(rng),
                    rect: Box2 {
                        x,
                        y: 40.0,
                        w: 22.0,
                        h: 22.0,
                    },
                };
                Op::AddConnector {
                    a: anchor(rng, 40.0),
                    b: anchor(rng, 80.0),
                    label: st.next_connector_label(),
                    style: style(rng),
                    locals: made(rng, 2),
                }
            }
            "patch-marks" => {
                let (page, a) = mark(rng);
                let mut patch = MarkPatch {
                    color: rng.bool().then(|| [rng.u8(..), 0, 0]),
                    contents: rng.bool().then(|| format!("p{}", rng.u8(..))),
                    ..MarkPatch::default()
                };
                if rng.bool() {
                    let dx = rng.f32() * 10.0;
                    patch.kind = Some(crate::tx::mark_kind(&a.kind.translated(dx, 0.0)));
                }
                // An import's, keeping the mark's name.
                let imported = (rng.u8(..5) == 0).then(|| Imported {
                    nm: a.name(),
                    created: "D:20260101000000Z".into(),
                    modified: "D:20260103000000Z".into(),
                    author: "Them".into(),
                });
                Op::PatchMarks {
                    marks: vec![annot_ref(rng, page, a.id, a.name())],
                    patch,
                    imported,
                }
            }
            "move-marks" => {
                let v = Vec2 {
                    x: rng.f32() * 20.0 - 10.0,
                    y: rng.f32() * 20.0 - 10.0,
                };
                Op::MoveMarks {
                    marks: some_marks(rng),
                    by: match rng.bool() {
                        true => Delta::Page(v),
                        false => Delta::View(v),
                    },
                    to_page: (rng.u8(..3) == 0).then(|| any_page(rng)),
                }
            }
            "delete-annotations" => {
                // Ours, or another tool's comment or link.
                let one = |rng: &mut fastrand::Rng| match rng.bool() {
                    true if !foreign.is_empty() => {
                        let (page, id, f) = foreign[rng.usize(..foreign.len())];
                        annot_ref(rng, page, id, f.summary.name.clone())
                    }
                    _ if !marks.is_empty() => {
                        let (page, a) = mark(rng);
                        annot_ref(rng, page, a.id, a.name())
                    }
                    _ => {
                        let (page, id, f) = foreign[rng.usize(..foreign.len())];
                        annot_ref(rng, page, id, f.summary.name.clone())
                    }
                };
                Op::DeleteAnnotations {
                    annots: (0..rng.usize(1..=2)).map(|_| one(rng)).collect(),
                }
            }
            "adopt-annotations" => {
                let (page, id) = adoptable[rng.usize(..adoptable.len())];
                let nm = st.annot(id).expect("a comment").name();
                Op::AdoptAnnotations {
                    annots: vec![annot_ref(rng, page, id, nm)],
                }
            }
            "set-ocr-text" => Op::SetOcrText {
                page: any_page(rng),
                layer: tl(&format!("w{}", rng.u8(..))),
            },
            "clear-ocr-text" => {
                let mut ids = vec![layered[rng.usize(..layered.len())]];
                if rng.bool() {
                    ids.push(page(rng));
                }
                Op::ClearOcrText {
                    pages: select(rng, &ids, true),
                }
            }
            "set-header-footer" => Op::SetHeaderFooter {
                spec: fx::header(&format!("h{}", rng.u8(..))),
                scope: match rng.bool() {
                    true => ScopeSel::All,
                    false => ScopeSel::Pages(some_pages(rng)),
                },
            },
            "remove-header-footer" => Op::RemoveHeaderFooter {
                pages: match rng.bool() {
                    true => PageSel::All,
                    false => some_pages(rng),
                },
            },
            "set-page-base" => {
                let pages = (some(rng).into_iter())
                    .map(|id| {
                        let (src, _) = source(rng);
                        (id, src.clone(), rng.u32(..src.page_count()))
                    })
                    .collect();
                return Gen::Base(pages);
            }
            k => unreachable!("kind {k}"),
        }))
    }

    /// Every version recorded, rebuilt from the opened version's state by
    /// its prims alone (unless a save is on the way), and reached by a jump
    /// with no snapshot kept that may be dropped: the state it was made as,
    /// ids and all, which keeps the invariants. Neither draws an id. What
    /// the versions refer to is in the store, and each source a version
    /// brings is one its steps show pages of. The kinds of edit that made a
    /// version.
    fn check(mut self) -> BTreeSet<&'static str> {
        let d = &mut self.d;
        // Snapshots dropped, so jumps replay, and nothing more pruned.
        d.set_policy(Box::new(FixedCount(1)));
        d.trim();
        for (id, v) in &d.versions {
            let brought = super::brought(&v.steps);
            match *id == d.root {
                true => assert!(v.blobs.is_superset(&brought)),
                false => assert_eq!(v.blobs, brought),
            }
            let prims: Vec<&Prim> = v.steps.iter().flat_map(|s| s.prims.iter()).collect();
            let keys: BTreeSet<Key> = prims.iter().map(|p| p.key).collect();
            // The first version kept its edits' keys, but no prims: nothing
            // is rebuilt into it.
            match *id == d.root {
                true => assert!(prims.is_empty(), "{id:?}, the first, keeps prims"),
                false => assert_eq!(*v.keys, keys, "{id:?}'s keys"),
            }
            // Coalesced: each slot's change once, on one step, and none that
            // puts back what was there.
            assert_eq!(
                keys.len(),
                prims.len(),
                "{id:?}'s steps change a slot twice"
            );
            let back = prims
                .iter()
                .find(|p| same(p.before.as_ref(), p.after.as_ref()));
            assert!(back.is_none(), "{id:?} puts back {back:?}");
            // What it holds is as counted again, a group's commits counted
            // as they joined it.
            if *id != d.root {
                let held = super::steps_bytes(&v.steps, &v.keys);
                assert_eq!(v.step_bytes, held, "{id:?}'s steps' bytes");
                if let Some(st) = &v.state {
                    let copied = st.snapshot_bytes(v.keys.iter().copied());
                    assert_eq!(v.snapshot_bytes, copied, "{id:?}'s snapshot's bytes");
                }
            }
        }
        for (id, st) in &self.made {
            assert_eq!(st.check_invariants(), Ok(()), "{id:?}");
            let pages = st.pages().map(|(_, def, _)| def.base.src.id());
            let fields = st.fields().map(|(_, node)| node.inst.src);
            let widgets = (st.annots()).filter_map(|(_, a)| Some(a.widget()?.inst.src));
            let instances = st.form().names.keys().map(|inst| inst.src);
            for s in pages.chain(fields).chain(widgets).chain(instances) {
                assert!(d.store().get(s).is_some(), "{id:?} refers to {s:?}");
            }
        }
        // The opened, current and saved versions keep their snapshots, and
        // so do versions being saved (`keeps_snapshot`): with any other
        // version, snapshots were dropped.
        let sealed = (d.versions.iter()).filter(|(_, v)| v.sealed > 0);
        let kept: BTreeSet<VersionId> = [Some(d.root), Some(d.cursor), d.saved_at()]
            .into_iter()
            .flatten()
            .chain(sealed.map(|(&id, _)| id))
            .collect();
        if d.version_count() > kept.len() {
            assert!(
                d.stats().versions < d.version_count(),
                "snapshots were dropped"
            );
        }
        let drawn = |d: &Document| d.ids.next();
        let ids = drawn(d);
        for (id, want) in &self.made {
            if let Some(st) = from_root(d, *id) {
                assert_eq!(&st, want, "{id:?} from the opened version");
                assert_eq!(st.check_invariants(), Ok(()), "{id:?} rebuilt");
            }
            d.trim();
            d.jump(*id).unwrap();
            assert_eq!(d.state(), want, "{id:?} by a jump");
        }
        assert_eq!(drawn(d), ids, "a replay draws no ids");
        self.kinds
    }
}

/// What a commit of a run's is labelled.
fn label(text: &str) -> CommitOpts {
    CommitOpts::labelled(text)
}

/// The locals `op` names what it makes by, in order.
fn declared(op: &Op) -> Vec<Local> {
    match op {
        Op::DuplicatePages { locals, .. }
        | Op::InsertPages { locals, .. }
        | Op::AddConnector { locals, .. } => locals.clone(),
        Op::AddBookmark { local, .. } | Op::AddMark { local, .. } => {
            local.iter().copied().collect()
        }
        _ => Vec::new(),
    }
}

/// Bookmark `id` of `st` by its id, its titles from the top level down,
/// or the local naming it.
fn bookmark_ref(
    st: &DocState,
    rng: &mut fastrand::Rng,
    id: OutlineId,
    locals: &Locals,
) -> BookmarkRef {
    match rng.u8(..4) {
        0 => {
            let mut path: Vec<String> = Vec::new();
            for (depth, at, b) in st.bookmarks_preorder() {
                path.truncate(usize::from(depth));
                path.push(b.title.to_string());
                if at == id {
                    break;
                }
            }
            BookmarkRef::Path(path)
        }
        1 => (locals.of(Made::Bookmark(id))).map_or(BookmarkRef::Id(id), BookmarkRef::New),
        _ => BookmarkRef::Id(id),
    }
}

/// Every slot `st` holds a value in.
fn keys_of(st: &DocState) -> BTreeSet<Key> {
    let mut keys = BTreeSet::from([Key::Order, Key::OutlineRoots, Key::Form, Key::Labels]);
    for (id, _, _) in st.pages() {
        keys.extend([Key::Page(id), Key::Geom(id), Key::PageAnnots(id)]);
    }
    keys.extend(st.annots().map(|(id, _)| Key::Annot(id)));
    keys.extend((st.bookmarks_preorder().into_iter()).map(|(_, id, _)| Key::Bookmark(id)));
    keys.extend(st.fields().map(|(id, _)| Key::Field(id)));
    keys.extend(st.generators().keys().map(|&id| Key::Generator(id)));
    keys
}

/// The slots whose values differ between `a` and `b`.
fn differing(a: &DocState, b: &DocState) -> BTreeSet<Key> {
    (keys_of(a).union(&keys_of(b)))
        .filter(|k| !same(a.get(k).as_ref(), b.get(k).as_ref()))
        .copied()
        .collect()
}

/// The keys of the prims of every version on the tree path from version
/// `a` to version `b`, the lines from the root to each walked whole: what
/// a save's path keys were before versions kept theirs.
fn walked_keys(d: &Document, a: VersionId, b: VersionId) -> BTreeSet<Key> {
    let line = |mut at: VersionId| {
        let mut line = vec![at];
        while let Some(p) = d.versions[&at].parent {
            line.push(p);
            at = p;
        }
        line
    };
    let (ours, theirs) = (line(a), line(b));
    let shared = (ours.iter().rev())
        .zip(theirs.iter().rev())
        .take_while(|(x, y)| x == y)
        .count();
    (ours[..ours.len() - shared].iter())
        .chain(&theirs[..theirs.len() - shared])
        .flat_map(|id| d.versions[id].steps.iter())
        .flat_map(|step| step.prims.iter().map(|p| p.key))
        .collect()
}

/// Version `id` rebuilt from the opened version's state by the prims of
/// each version on the way, whatever other snapshots are kept. None when
/// one on the way was saved: a save expresses a version anew, so its
/// prims lead to it as it was, and its children's lead from it as it was
/// when they were made.
fn from_root(d: &Document, id: VersionId) -> Option<DocState> {
    let mut path = vec![id];
    while let Some(parent) = d.versions[path.last().expect("nonempty")].parent {
        path.push(parent);
    }
    if path.iter().any(|v| !d.versions[v].saves.is_empty()) {
        return None;
    }
    let root = path.pop().expect("nonempty");
    let mut st = d.versions[&root]
        .state
        .clone()
        .expect("the opened version keeps its state");
    for v in path.iter().rev() {
        for p in d.versions[v].steps.iter().flat_map(|s| s.prims.iter()) {
            st.apply(p)
                .unwrap_or_else(|m| panic!("version {}: {m}", v.0));
        }
    }
    Some(st)
}

/// Runs `actions` on a new document and checks every version they made:
/// the kinds of op that made one.
fn run(actions: &[(u8, u64)]) -> BTreeSet<&'static str> {
    play(actions).check()
}

/// `actions` run on a new document.
fn play(actions: &[(u8, u64)]) -> Run {
    // Mark ids come from the thread's generator: the same ones each run.
    fastrand::seed(1);
    let mut r = Run::new();
    for &action in actions {
        r.act(action);
    }
    r
}

fn runs() -> impl Strategy<Value = Vec<(u8, u64)>> {
    prop::collection::vec((0..ACTIONS, any::<u64>()), 1..48)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn prims_rebuild_every_version(actions in runs()) {
        run(&actions);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(4096))]

    #[test]
    #[ignore = "4,096 cases: cargo test -p sheaf-doc --release -- --ignored prims_rebuild"]
    fn prims_rebuild_every_version_4096(actions in runs()) {
        run(&actions);
    }
}

/// A long run, mostly edits, with saves on the way, old and new: pruning
/// past `MAX_VERSIONS` moves the opened version forward, never past a
/// version being saved, and every version left is still rebuilt.
#[test]
fn prims_rebuild_every_version_past_max_versions() {
    let rng = &mut fastrand::Rng::with_seed(0);
    let actions: Vec<(u8, u64)> = (0..2600)
        .map(|_| {
            // No jumps: one far back leaves a branch for pruning to take
            // instead.
            let action = match rng.u8(..40) {
                0 => UNDO,
                1 => REDO,
                2 => SAVE,
                3 => GROUP,
                4 => BATCH,
                5 => SEAL,
                6 => UNSEAL,
                7 => REVERT,
                _ => rng.u8(..KINDS),
            };
            (action, rng.u64(..))
        })
        .collect();
    let r = play(&actions);
    let opened = &r.d.versions[&r.d.root];
    assert!(!opened.steps.is_empty(), "pruned to a later one");
    r.check();
}

/// Each kind of op in turn, until it makes a version (one drawn against
/// the document as it is may change nothing, or make way for another,
/// as a move of one bookmark makes a second), then a batch, groups, a
/// revert and the rest: every kind makes one, so the property covers them
/// all.
#[test]
fn a_run_applies_every_kind() {
    fastrand::seed(1);
    let mut r = Run::new();
    for k in 0..KINDS {
        for pass in 0..8 {
            if r.kinds.contains(OPS[usize::from(k)]) {
                break;
            }
            r.act((k, pass * 100 + u64::from(k)));
        }
    }
    let rest = [
        (BATCH, 1),
        (GROUP, 2),
        (GROUP, 3),
        (UNDO, 0),
        (BATCH, 4),
        (GROUP, 5),
        (GROUP, 6),
        (BATCH, 7),
        (GROUP, 8),
        (REVERT, 9),
        (GROUP, 10),
        (ROLLBACK, 11),
        (PRUNE, 13),
        (JUMP, 12),
        (REDO, 0),
        (TRIM, 0),
    ];
    for action in rest {
        r.act(action);
    }
    let mut every = BTreeSet::from(OPS);
    every.insert("revert");
    assert_eq!(r.check(), every);
}

/// Each kind's no-op makes no version and leaves the cursor where it was.
/// Inserts and replacements always bring a page, and new bookmarks, marks
/// and connectors are made: those kinds have none.
#[test]
fn each_kind_s_no_op_pushes_nothing() {
    let facts = Facts {
        outline: flat(&[("A", Some(0), 0), ("A1", Some(1), 1), ("B", Some(3), 0)]),
        annots: their_annots(),
        ..Default::default()
    };
    let mut d = opened_doc(6, facts, Box::new(FixedCount(100)));
    let [p0, p1, p2] = [0, 1, 2].map(|i| d.page(i).unwrap());
    let roman = || Op::SetPageLabels {
        ranges: Some(vec![fx::label(p0, LabelStyle::LowerRoman, "", 1)]),
    };
    let header = || fx::set_header(fx::header("Draft"), None);
    let layer = tl("text");
    // What the no-ops leave as it is.
    let before = vec![
        fx::add(p0, &square(0)),
        fx::delete_named(p1, &["theirs-2"]),
        roman(),
        header(),
        fx::ocr([(p2, layer.clone())]).remove(0),
    ];
    d.commit_ops(before, "before").unwrap();
    let mark = d.state().marks_on(p0).next().unwrap().clone();
    let [a, b] = ["A", "B"].map(|title| {
        d.state()
            .bookmarks_preorder()
            .iter()
            .find(|(_, _, b)| &*b.title == title)
            .unwrap()
            .1
    });
    let none = || PageSel::Ids(Vec::new());
    let no_ops = vec![
        fx::move_pages(&[p1], Some(p2)),
        Op::RotatePages {
            pages: none(),
            by: Turn::Quarter,
        },
        Op::DeletePages { pages: none() },
        Op::DuplicatePages {
            pages: none(),
            locals: Vec::new(),
        },
        roman(),
        fx::patch_bookmark(a, BookmarkPatch::default()),
        // B and its pages, after A and its.
        Op::MoveBookmarks {
            bookmarks: vec![BookmarkRef::Id(b)],
            parent: None,
            at: fx::sibling(Some(a)),
            with_pages: true,
        },
        Op::DeleteBookmarks {
            bookmarks: Vec::new(),
        },
        // The mark said again as it is.
        fx::update(&mark),
        Op::MoveMarks {
            marks: vec![AnnotRef::Id(mark.id)],
            by: Delta::Page(Vec2 { x: 0.0, y: 0.0 }),
            to_page: None,
        },
        Op::DeleteAnnotations { annots: Vec::new() },
        Op::AdoptAnnotations { annots: Vec::new() },
        fx::ocr([(p2, layer)]).remove(0),
        fx::clear_ocr(&[p0]),
        header(),
        Op::RemoveHeaderFooter { pages: none() },
    ];
    let names: BTreeSet<&str> = no_ops.iter().map(crate::tx::op_name).collect();
    assert_eq!(names.len(), no_ops.len());
    // And the page rebase of no pages.
    assert_eq!(
        names.len() + 1,
        usize::from(KINDS) - 5,
        "every kind that has one"
    );
    let (n, at) = (d.version_count(), d.current_version());
    for op in no_ops {
        let name = crate::tx::op_name(&op);
        d.commit_ops(vec![op], "no-op").unwrap();
        assert_eq!((d.version_count(), d.current_version()), (n, at), "{name}");
    }
    let mut tx = d.tx(Actor::User);
    tx.set_page_base(Vec::new(), Derive::Crop).unwrap();
    d.commit(tx, label("no-op")).unwrap();
    assert_eq!((d.version_count(), d.current_version()), (n, at));
}
