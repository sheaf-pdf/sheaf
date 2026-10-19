//! History's proof (M10.5 slice 1): over random runs of every kind of
//! edit, with batches, jobs, undo, redo, jumps, trims and saves, each version
//! rebuilt from its prims is the state it was made as, and every file it
//! refers to is still registered (slice 3). Every state on the way keeps
//! the records' invariants (slice 8), a version being saved resolves to
//! the state it was sealed with until its save ends (slice 9), and a save
//! of the current version would find changed exactly what differs from
//! the saved state (slice 10). Each kind's no-op makes no version.

use std::collections::BTreeSet;

use lopdf::{Object, ObjectId, dictionary};
use proptest::prelude::*;

use super::tests::{connector, flat, header, opened_doc, square, tl};
use super::*;
use crate::forms::fixture::Form;
use crate::history::FixedCount;
use crate::labels::{LabelSpec, Style};
use crate::materialize::StrategyKind;
use crate::open::FileAnnots;
use crate::stamps::HeaderFooter;

/// A run's actions: below [`KINDS`], an edit of that kind, in [`Edit`]'s
/// order; then these.
const KINDS: u8 = 21;
const BATCH: u8 = 21;
const JOB: u8 = 22;
const UNDO: u8 = 23;
const REDO: u8 = 24;
const JUMP: u8 = 25;
pub(crate) const TRIM: u8 = 26;
const SAVE: u8 = 27;
const SEAL: u8 = 28;
const UNSEAL: u8 = 29;
const ACTIONS: u8 = 30;

/// `edit`'s kind, by name.
fn kind(edit: &Edit) -> &'static str {
    match edit {
        Edit::MovePages { .. } => "MovePages",
        Edit::RotatePages { .. } => "RotatePages",
        Edit::DeletePages { .. } => "DeletePages",
        Edit::InsertPages { .. } => "InsertPages",
        Edit::DuplicatePages { .. } => "DuplicatePages",
        Edit::ReplacePages { .. } => "ReplacePages",
        Edit::OutlineInsert { .. } => "OutlineInsert",
        Edit::OutlineInsertTree { .. } => "OutlineInsertTree",
        Edit::OutlineMove { .. } => "OutlineMove",
        Edit::OutlineEdit { .. } => "OutlineEdit",
        Edit::OutlineDelete { .. } => "OutlineDelete",
        Edit::AnnotAdd { .. } => "AnnotAdd",
        Edit::AnnotUpdate { .. } => "AnnotUpdate",
        Edit::AnnotDelete { .. } => "AnnotDelete",
        Edit::CommentsDelete { .. } => "CommentsDelete",
        Edit::ConnectorAdd { .. } => "ConnectorAdd",
        Edit::SetTextLayers { .. } => "SetTextLayers",
        Edit::ClearTextLayers { .. } => "ClearTextLayers",
        Edit::RebasePages { .. } => "RebasePages",
        Edit::SetPageLabels { .. } => "SetPageLabels",
        Edit::SetHeaderFooter { .. } => "SetHeaderFooter",
    }
}

/// A document, and what a run of actions did to it.
pub(crate) struct Run {
    pub(crate) d: Document,
    /// The opened file, and another with a form, whose pages come in.
    opened: SourceId,
    other: SourceId,
    /// The job the run's job actions join, until one starts another.
    job: Option<JobId>,
    /// Each version an edit made that still resolves, with its state then.
    made: Vec<(VersionId, DocState)>,
    /// The kinds of edit that made a version.
    kinds: BTreeSet<&'static str>,
    /// Versions being saved, with the state each was sealed with.
    sealed: Vec<(VersionId, DocState)>,
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
    /// links, beside a file with the same form, and their comments and
    /// links on its two pages, keeping no snapshot it may drop.
    fn new() -> Self {
        let mut other = crate::forms::fixture::every_kind();
        with_theirs(&mut other, 0, "theirs-1");
        with_theirs(&mut other, 1, "theirs-2");
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
    pub(crate) fn over(mut d: Document, other: Vec<u8>) -> Self {
        let kind = BlobKind::file(Path::new("form.pdf"), None);
        let other = SourceInfo {
            src: d.store().open(other.into(), kind).unwrap(),
            name: "form.pdf".into(),
            derived: None,
        };
        let other = d.add_source(other);
        // The opened version holds it, so any version may bring its pages
        // in, as it may the opened file's. (The app registers a file for
        // each insert, and the version the insert made holds it.)
        d.unlogged_sources.clear();
        let root = d.root;
        d.versions.get_mut(&root).unwrap().sources.push(other);
        let made = vec![(d.current_version(), d.state().clone())];
        Self {
            opened: d.primary(),
            d,
            other,
            job: None,
            made,
            kinds: BTreeSet::new(),
            sealed: Vec::new(),
        }
    }

    /// Runs `action`, its choices drawn from `seed`: the state it leaves
    /// keeps the invariants.
    pub(crate) fn act(&mut self, (action, seed): (u8, u64)) {
        let rng = &mut fastrand::Rng::with_seed(seed);
        match action {
            0..KINDS => {
                let edit = self.edit(action, rng);
                let kinds = [kind(&edit)];
                self.record(kinds, |d| d.apply(edit, "edit"));
            }
            BATCH => {
                let edits: Vec<Edit> = (0..rng.usize(2..=4))
                    .map(|_| self.edit(rng.u8(..KINDS), rng))
                    .collect();
                let kinds: Vec<&str> = edits.iter().map(kind).collect();
                self.record(kinds, |d| d.apply_batch(edits, "batch"));
            }
            JOB => {
                let job = match self.job {
                    Some(job) if rng.u8(..4) > 0 => job,
                    _ => self.d.begin_job(),
                };
                self.job = Some(job);
                // A few batches in a row, mostly text layers, as OCR sends
                // them: later ones amend the job's version, and a layer set
                // and cleared again can leave none.
                for _ in 0..rng.usize(1..=3) {
                    let k = match rng.u8(..4) {
                        0 => 16, // SetTextLayers
                        1 => 17, // ClearTextLayers
                        _ => rng.u8(..KINDS),
                    };
                    let edit = self.edit(k, rng);
                    let kinds = [kind(&edit)];
                    self.record(kinds, |d| d.apply_in_job(job, edit, "job"));
                }
            }
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
            _ => unreachable!("action {action}"),
        }
        let st = self.d.state();
        assert_eq!(st.check_invariants(), Ok(()), "after action {action}");
        for (v, st) in &self.sealed {
            let kept = self.d.versions.get(v).and_then(|v| v.state.as_ref());
            assert_eq!(kept, Some(st), "{v:?}, sealed, after action {action}");
        }
        // A save of the current version finds changed exactly the slots
        // that differ from the saved state: its path keys reach every one,
        // and are judged by value.
        let input = self.d.input(self.d.cursor);
        if let Some(saved) = &input.saved {
            let path = crate::effects::judge(&saved.state, &input.target, &input.path_keys);
            let differ = differing(&saved.state, &input.target);
            assert_eq!(path.changed, differ, "after action {action}");
            assert_eq!(path.flags.is_empty(), differ.is_empty(), "{:?}", path.flags);
        }
    }

    /// A save of version `v`, sealed, wrote a file: `v` is the saved one,
    /// with a record of it.
    fn saved(&mut self, v: VersionId) {
        let primary = self.d.source(self.d.primary()).unwrap();
        let stamp = FileStamp {
            len: 0,
            mtime: std::time::SystemTime::UNIX_EPOCH,
            tail: 0,
            window: 0,
        };
        let rec = SaveRecord {
            strategy: StrategyKind::Rewrite,
            ..SaveRecord::opened(v, primary, stamp)
        };
        self.d.record_save(rec).unwrap();
        assert_eq!(self.d.saved, Some(v));
    }

    /// Runs `apply`, an edit of `kinds`. When it made a version, or put a
    /// new one in the cursor's place, records it.
    fn record(
        &mut self,
        kinds: impl IntoIterator<Item = &'static str>,
        apply: impl FnOnce(&mut Document) -> Result<EditOutcome>,
    ) {
        let at = self.d.current_version();
        if apply(&mut self.d).is_err() || self.d.current_version() == at {
            return;
        }
        // An amend replaces the version it joins, and a job that put back
        // all it did removes it: the cursor is back at its parent.
        self.made.retain(|(id, _)| self.d.versions.contains_key(id));
        let id = self.d.current_version();
        if self.made.iter().all(|(v, _)| *v != id) {
            self.made.push((id, self.d.state().clone()));
            self.kinds.extend(kinds);
        }
    }

    /// An edit of kind `kind` against the document as it is, what it acts
    /// on drawn from `rng`. A kind with nothing to act on is one that has:
    /// an AnnotUpdate with no mark adds one, and so on.
    fn edit(&self, kind: u8, rng: &mut fastrand::Rng) -> Edit {
        let st = self.d.state();
        let pages: Vec<PageId> = st.order().iter().copied().collect();
        let marks: Vec<(PageId, &Annot)> = pages
            .iter()
            .flat_map(|&p| st.marks_on(p).map(move |a| (p, a)))
            .collect();
        let foreign: Vec<(PageId, AnnotId, &ForeignAnnot)> = pages
            .iter()
            .flat_map(|&p| st.foreign_on(p).map(move |(id, f)| (p, id, &*f.summary)))
            .collect();
        let layered: Vec<PageId> = pages
            .iter()
            .copied()
            .filter(|&p| st.ocr_text(p).is_some())
            .collect();
        let bookmarks: Vec<OutlineId> = (st.bookmarks_preorder().into_iter())
            .map(|(_, id, _)| id)
            .collect();
        let kind = match kind {
            2 if pages.len() <= 2 => 3,
            8..=10 if bookmarks.is_empty() => 6,
            12 if marks.is_empty() => 11,
            13 if marks.is_empty() && foreign.is_empty() => 11,
            17 if layered.is_empty() => 16,
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
        let mark = |rng: &mut fastrand::Rng| marks[rng.usize(..marks.len())];
        let bookmark = |rng: &mut fastrand::Rng| bookmarks[rng.usize(..bookmarks.len())];
        // Under a bookmark or at the top, first or after one there; for a
        // bookmark `moving`, outside it.
        let place = |rng: &mut fastrand::Rng, moving: Option<OutlineId>| {
            let outside = |id: OutlineId| moving.is_none_or(|m| !subtree(st, m).contains(&id));
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
            let after =
                (!siblings.is_empty() && rng.bool()).then(|| siblings[rng.usize(..siblings.len())]);
            (parent, after)
        };
        // A file pages come from, and its page count.
        let source = |rng: &mut fastrand::Rng| {
            let id = if rng.bool() { self.other } else { self.opened };
            (id, self.d.sources[&id].src.page_count())
        };
        // A few of its pages: a saved file has all the document's, and
        // bringing in all of them doubles it.
        let within = |rng: &mut fastrand::Rng, count: u32| {
            let start = rng.u32(..count);
            start..rng.u32(start + 1..=count.min(start + 4))
        };
        let view = |rng: &mut fastrand::Rng| DestView::Xyz {
            left: Some(rng.f32() * 100.0),
            top: Some(rng.f32() * 100.0),
            zoom: None,
        };
        // Other tools' comments, by name.
        let theirs = |rng: &mut fastrand::Rng| ["theirs-1", "theirs-2"][rng.usize(..2)].to_string();
        match kind {
            0 => {
                let ids = some(rng);
                let rest: Vec<PageId> =
                    pages.iter().copied().filter(|p| !ids.contains(p)).collect();
                let before =
                    (!rest.is_empty() && rng.bool()).then(|| rest[rng.usize(..rest.len())]);
                Edit::MovePages { ids, before }
            }
            1 => Edit::RotatePages {
                ids: some(rng),
                by: [Rotation::R90, Rotation::R180, Rotation::R270][rng.usize(..3)],
            },
            2 => Edit::DeletePages {
                ids: vec![page(rng)],
            },
            3 => {
                let (source, count) = source(rng);
                let range = within(rng, count);
                Edit::InsertPages {
                    source,
                    before: rng.bool().then(|| page(rng)),
                    outline: rng
                        .bool()
                        .then(|| flat(&[("A", Some(0), 0), ("X", Some(1), 1)])),
                    annots: vec![
                        (range.start, vec![square(1), connector(2, 3)]),
                        (range.end - 1, vec![connector(3, 2)]),
                    ],
                    range,
                }
            }
            4 => Edit::DuplicatePages { ids: some(rng) },
            5 => {
                let (source, count) = source(rng);
                // With two pages left, no more pages replaced than there
                // are replacements, so none goes.
                let ids = if pages.len() > 2 {
                    some(rng)
                } else {
                    vec![page(rng)]
                };
                let range = within(rng, count);
                Edit::ReplacePages {
                    ids,
                    source,
                    annots: vec![(range.start, vec![square(1)])],
                    range,
                }
            }
            6 => {
                let (parent, after) = place(rng, None);
                Edit::OutlineInsert {
                    parent,
                    after,
                    title: format!("b{}", rng.u8(..)),
                    target: rng.bool().then(|| page(rng)),
                    view: rng.bool().then(|| view(rng)),
                }
            }
            7 => {
                let (parent, after) = place(rng, None);
                let mut depth = 0;
                let items = (0..rng.usize(1..=3))
                    .map(|i| {
                        let item = NewOutlineItem {
                            title: format!("t{i}"),
                            target: rng.bool().then(|| page(rng)),
                            view: None,
                            depth,
                        };
                        depth = rng.u16(..=depth + 1);
                        item
                    })
                    .collect();
                Edit::OutlineInsertTree {
                    parent,
                    after,
                    items,
                }
            }
            8 => {
                let id = bookmark(rng);
                let (parent, after) = place(rng, Some(id));
                Edit::OutlineMove {
                    id,
                    parent,
                    after,
                    with_pages: rng.bool(),
                }
            }
            9 => Edit::OutlineEdit {
                id: bookmark(rng),
                title: rng.bool().then(|| format!("e{}", rng.u8(..))),
                target: rng.bool().then(|| rng.bool().then(|| page(rng))),
                color: rng.bool().then(|| rng.bool().then_some([0, 0, 255])),
                view: rng.bool().then(|| rng.bool().then(|| view(rng))),
                style: rng.bool().then(|| TextStyle {
                    bold: rng.bool(),
                    italic: rng.bool(),
                }),
            },
            10 => Edit::OutlineDelete { id: bookmark(rng) },
            11 => {
                // Sometimes a reply, to one of ours on its page or to
                // another tool's comment, which AnnotDelete and
                // CommentsDelete take with them; sometimes another tool's
                // comment made ours, which takes its place.
                let reply = |parent| Annot {
                    kind: AnnotKind::Reply { parent },
                    ..square(0)
                };
                let adoptable: Vec<(PageId, AnnotId, &Annot)> = (foreign.iter())
                    .filter_map(|&(page, id, f)| Some((page, id, &**f.mark.as_ref()?)))
                    .collect();
                let (page, annot) = match rng.u8(..4) {
                    0 if !marks.is_empty() => {
                        let (page, a) = mark(rng);
                        (page, reply(a.name()))
                    }
                    1 => (page(rng), reply(theirs(rng))),
                    2 if !adoptable.is_empty() => {
                        let (page, id, mark) = adoptable[rng.usize(..adoptable.len())];
                        (page, Annot { id, ..mark.clone() })
                    }
                    _ => (page(rng), square(0)),
                };
                Edit::AnnotAdd { page, annot }
            }
            12 => {
                let (page, a) = mark(rng);
                Edit::AnnotUpdate {
                    page,
                    annot: Annot {
                        color: [rng.u8(..), 0, 0],
                        contents: format!("c{}", rng.u8(..)),
                        ..a.clone()
                    },
                }
            }
            13 => {
                // Ours, or another tool's comment or link.
                let (page, id) = match rng.bool() {
                    true if !foreign.is_empty() => {
                        let (page, id, _) = foreign[rng.usize(..foreign.len())];
                        (page, id)
                    }
                    _ if !marks.is_empty() => {
                        let (page, a) = mark(rng);
                        (page, a.id)
                    }
                    _ => {
                        let (page, id, _) = foreign[rng.usize(..foreign.len())];
                        (page, id)
                    }
                };
                Edit::AnnotDelete { page, id }
            }
            14 => Edit::CommentsDelete {
                page: page(rng),
                names: (0..rng.usize(..3)).map(|_| theirs(rng)).collect(),
            },
            15 => Edit::ConnectorAdd {
                a: (page(rng), connector(0, 0)),
                b: (page(rng), connector(0, 0)),
            },
            16 => Edit::SetTextLayers {
                layers: some(rng)
                    .into_iter()
                    .map(|p| (p, tl(&format!("w{}", rng.u8(..)))))
                    .collect(),
            },
            17 => {
                let mut ids = vec![layered[rng.usize(..layered.len())]];
                if rng.bool() {
                    ids.push(page(rng));
                }
                Edit::ClearTextLayers { ids }
            }
            18 => Edit::RebasePages {
                pages: some(rng)
                    .into_iter()
                    .map(|id| {
                        let (source, count) = source(rng);
                        Rebase {
                            id,
                            source,
                            index: rng.u32(..count),
                        }
                    })
                    .collect(),
            },
            19 => Edit::SetPageLabels {
                ranges: (0..rng.usize(..3))
                    .map(|_| LabelSpec {
                        start: page(rng),
                        style: [Style::Decimal, Style::LowerRoman, Style::UpperLetters]
                            [rng.usize(..3)],
                        prefix: if rng.bool() {
                            "A-".into()
                        } else {
                            String::new()
                        },
                        first: rng.u32(1..5),
                    })
                    .collect(),
            },
            20 => Edit::SetHeaderFooter {
                header_footer: (rng.u8(..3) > 0).then(|| HeaderFooter {
                    pages: rng.bool().then(|| some(rng)),
                    ..header(format!("h{}", rng.u8(..)))
                }),
            },
            _ => unreachable!("kind {kind}"),
        }
    }

    /// Every version recorded, rebuilt from the opened version's state by
    /// its prims alone (unless a save is on the way), and reached by a jump
    /// with no snapshot kept that may be dropped: the state it was made as,
    /// ids and all, which keeps the invariants. Neither draws an id. Each
    /// registration is held once, by a version or the next one to be made
    /// (the primary may be by none), and what the versions refer to is
    /// registered. The kinds of edit that made a version.
    fn check(mut self) -> BTreeSet<&'static str> {
        let d = &mut self.d;
        d.trim();
        let mut held = BTreeSet::new();
        for s in d.versions.values().flat_map(|v| &v.sources) {
            assert!(held.insert(*s), "{s:?} is held twice");
        }
        for s in &d.unlogged_sources {
            assert!(held.insert(*s), "{s:?} is held twice");
        }
        held.insert(d.primary);
        let registered: BTreeSet<SourceId> = d.sources.keys().copied().collect();
        assert_eq!(held, registered);
        for (id, st) in &self.made {
            assert_eq!(st.check_invariants(), Ok(()), "{id:?}");
            let pages = st.pages().map(|(_, def, _)| def.base.source);
            let fields = st.fields().map(|(_, node)| node.inst.src);
            let widgets = (st.annots()).filter_map(|(_, a)| Some(a.widget()?.inst.src));
            let instances = st.form().names.keys().map(|inst| inst.src);
            for s in pages.chain(fields).chain(widgets).chain(instances) {
                assert!(registered.contains(&s), "{id:?} refers to {s:?}");
            }
        }
        // The opened, current and saved versions keep their snapshots, and
        // so do versions being saved (`keeps_snapshot`): with any other
        // version, snapshots were dropped.
        let sealed = (d.versions.iter()).filter(|(_, v)| v.sealed > 0);
        let kept: BTreeSet<VersionId> = [Some(d.root), Some(d.cursor), d.saved]
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
        let drawn = |d: &Document| {
            [&d.ids.page, &d.ids.outline, &d.ids.field].map(|n| n.load(Ordering::Relaxed))
        };
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
/// the kinds of edit that made one.
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
                3 => JOB,
                4 => BATCH,
                5 => SEAL,
                6 => UNSEAL,
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

/// Each kind in turn, twice, then a batch, a job and the rest: every kind
/// makes a version, so the property covers all 21.
#[test]
fn a_run_applies_every_kind() {
    let actions: Vec<(u8, u64)> = (0..2)
        .flat_map(|pass| (0..KINDS).map(move |k| (k, pass * 100 + u64::from(k))))
        .chain([
            (BATCH, 1),
            (JOB, 2),
            (JOB, 3),
            (UNDO, 0),
            (BATCH, 4),
            (JUMP, 5),
            (REDO, 0),
            (TRIM, 0),
        ])
        .collect();
    let kinds = run(&actions);
    assert_eq!(kinds.len(), usize::from(KINDS), "{kinds:?}");
}

/// Each kind's no-op makes no version and leaves the cursor where it was.
/// OutlineInsert, OutlineDelete, AnnotAdd, AnnotDelete and ConnectorAdd
/// have none: each always makes or removes something, or fails.
#[test]
fn each_kind_s_no_op_pushes_nothing() {
    let facts = Facts {
        outline: flat(&[("A", Some(0), 0), ("A1", Some(1), 1), ("B", Some(3), 0)]),
        annots: their_annots(),
        ..Default::default()
    };
    let mut d = opened_doc(6, facts, Box::new(FixedCount(100)));
    let other = d.add_source(SourceInfo::blank(d.store(), 4));
    let [p0, p1, p2] = [0, 1, 2].map(|i| d.page(i).unwrap());
    let roman = || {
        vec![LabelSpec {
            start: p0,
            style: Style::LowerRoman,
            prefix: String::new(),
            first: 1,
        }]
    };
    // What the no-ops leave as it is.
    for edit in [
        Edit::AnnotAdd {
            page: p0,
            annot: square(0),
        },
        Edit::CommentsDelete {
            page: p1,
            names: vec!["theirs-2".into()],
        },
        Edit::SetPageLabels { ranges: roman() },
        Edit::SetHeaderFooter {
            header_footer: Some(header("Draft".into())),
        },
    ] {
        d.apply(edit, "before").unwrap();
    }
    let mark = d.state().marks_on(p0).next().unwrap().clone();
    let [a, b] = ["A", "B"].map(|title| {
        d.state()
            .bookmarks_preorder()
            .iter()
            .find(|(_, _, b)| &*b.title == title)
            .unwrap()
            .1
    });
    let no_ops = vec![
        Edit::MovePages {
            ids: vec![p1],
            before: Some(p2),
        },
        Edit::RotatePages {
            ids: vec![],
            by: Rotation::R90,
        },
        Edit::DeletePages { ids: vec![] },
        Edit::InsertPages {
            source: other,
            range: 0..0,
            before: Some(p1),
            outline: None,
            annots: vec![],
        },
        Edit::DuplicatePages { ids: vec![] },
        Edit::ReplacePages {
            ids: vec![],
            source: other,
            range: 0..0,
            annots: vec![],
        },
        Edit::OutlineInsertTree {
            parent: None,
            after: None,
            items: vec![],
        },
        // B and its pages, after A and its.
        Edit::OutlineMove {
            id: b,
            parent: None,
            after: Some(a),
            with_pages: true,
        },
        Edit::OutlineEdit {
            id: a,
            title: None,
            target: None,
            color: None,
            view: None,
            style: None,
        },
        Edit::AnnotUpdate {
            page: p0,
            annot: mark,
        },
        // Names deleted already, of no comment on the page, and none.
        Edit::CommentsDelete {
            page: p1,
            names: vec!["theirs-2".into()],
        },
        Edit::CommentsDelete {
            page: p1,
            names: vec!["theirs".into()],
        },
        Edit::CommentsDelete {
            page: p2,
            names: vec![],
        },
        Edit::SetTextLayers { layers: vec![] },
        Edit::ClearTextLayers { ids: vec![p0] },
        Edit::RebasePages { pages: vec![] },
        Edit::SetPageLabels { ranges: roman() },
        Edit::SetHeaderFooter {
            header_footer: Some(header("Draft".into())),
        },
    ];
    let kinds: BTreeSet<&str> = no_ops.iter().map(kind).collect();
    assert_eq!(
        kinds.len(),
        usize::from(KINDS) - 5,
        "every kind that has one"
    );
    let (n, at) = (d.version_count(), d.current_version());
    for edit in no_ops {
        let k = kind(&edit);
        d.apply(edit, "no-op").unwrap();
        assert_eq!((d.version_count(), d.current_version()), (n, at), "{k}");
    }
}
