//! What a transition between versions changed (M10.5 §14.1): one
//! [`ChangeSet`] from each commit, amend, undo, redo, jump, save,
//! rollback and revert, and from the file's facts coming in, for whatever
//! follows the document (the view, its panels, caches and plugins) to
//! refresh by. It's made from the slots the transition changed, a lookup
//! or two a slot, never by diffing the states' tables.

use std::collections::BTreeSet;
use std::sync::Arc;

use crate::forms::FieldId;
use crate::model::{DocState, PageId, VersionId};
use crate::prim::Key;
use crate::tx::Actor;

/// What made a transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Cause {
    /// The file's facts are in: the opened version
    /// ([`Document::ready`](crate::Document::ready)).
    Opened,
    /// A transaction made a version.
    Commit,
    /// A transaction joined a group's version, and a new version took its
    /// place.
    Amend,
    Undo,
    Redo,
    /// A jump to another version, as the History panel makes.
    Jump,
    /// A save wrote the document's file, or found it holds the document
    /// already: what the document shows didn't change.
    Saved,
    /// A group's versions taken back after its call failed.
    Rollback,
    /// A new version undoing a group's versions.
    Revert,
}

/// What one transition changed, for everything that follows the document.
#[derive(Debug, Clone, PartialEq)]
pub struct ChangeSet {
    /// The version the document was at, and the one it's at now: the same
    /// for a commit that changed nothing, and for a save.
    pub from: VersionId,
    pub to: VersionId,
    pub cause: Cause,
    /// Who made it: a commit's or an amend's transaction's actor, a
    /// rollback's group's; the user's otherwise.
    pub actor: Actor,
    /// What it was, in words: the version's label, the one undone for an
    /// undo.
    pub label: Arc<str>,
    /// The slots it changed: a commit's or an amend's own prims' keys; for
    /// an undo, a redo or a jump, those of every version's steps on the way,
    /// through the version both ends come from, a few perhaps holding what
    /// they held.
    pub keys: Arc<BTreeSet<Key>>,
    /// The pages' order.
    pub order: bool,
    /// Pages it added and removed, by id.
    pub pages_added: Vec<PageId>,
    pub pages_removed: Vec<PageId>,
    /// Pages whose boxes or `/Rotate` changed, those added or removed
    /// among them.
    pub geometry: Arc<BTreeSet<PageId>>,
    /// Pages in both versions that show something else: their render keys
    /// may differ.
    pub content: Arc<BTreeSet<PageId>>,
    /// Pages whose annotations changed: the pages an annotation was on and
    /// is on, and those whose `/Annots` changed.
    pub annots: Arc<BTreeSet<PageId>>,
    /// Form fields whose nodes changed.
    pub fields: Vec<FieldId>,
    /// The outline, the page labels, the form as a whole, the generators.
    pub outline: bool,
    pub labels: bool,
    pub form: bool,
    pub generators: bool,
}

impl ChangeSet {
    /// What changing `keys` took `before`, version `from`'s state, to
    /// `after`, version `to`'s, judged slot by slot: a page's own slot
    /// added, removed or changed, where an annotation was and is.
    ///
    /// A page's slot is looked up only when the order is among `keys`:
    /// every page is in the order ([`DocState::check_invariants`]), so
    /// with the order as it was, the pages are the same ones, and a page
    /// whose slot changed is in both.
    #[allow(clippy::too_many_arguments)]
    pub fn of(
        from: VersionId,
        to: VersionId,
        cause: Cause,
        actor: Actor,
        label: Arc<str>,
        keys: Arc<BTreeSet<Key>>,
        before: &DocState,
        after: &DocState,
    ) -> ChangeSet {
        let mut out = ChangeSet {
            to,
            ..ChangeSet::empty(from, cause, actor, label)
        };
        let reordered = keys.contains(&Key::Order);
        let (mut geometry, mut content, mut annots) = (Vec::new(), Vec::new(), Vec::new());
        for &key in keys.iter() {
            match key {
                Key::Order => out.order = true,
                Key::Page(p) if !reordered => {
                    debug_assert!(before.def(p).is_some() && after.def(p).is_some());
                    content.push(p);
                }
                Key::Page(p) => match (before.def(p).is_some(), after.def(p).is_some()) {
                    (false, true) => out.pages_added.push(p),
                    (true, false) => out.pages_removed.push(p),
                    (true, true) => content.push(p),
                    (false, false) => {}
                },
                Key::Geom(p) => geometry.push(p),
                Key::PageAnnots(p) => annots.push(p),
                Key::Annot(a) => {
                    let page = |st: &DocState| st.annot(a).map(|rec| rec.page);
                    annots.extend(page(before));
                    annots.extend(page(after));
                }
                Key::Field(f) => out.fields.push(f),
                Key::Form => out.form = true,
                Key::Labels => out.labels = true,
                Key::Generator(_) => out.generators = true,
                Key::Bookmark(_) | Key::OutlineRoots => out.outline = true,
            }
        }
        out.keys = keys;
        out.geometry = Arc::new(geometry.into_iter().collect());
        out.content = Arc::new(content.into_iter().collect());
        out.annots = Arc::new(annots.into_iter().collect());
        out
    }

    /// A transition at version `at` that changed nothing: a commit of
    /// nothing, or a save.
    pub fn empty(at: VersionId, cause: Cause, actor: Actor, label: Arc<str>) -> ChangeSet {
        ChangeSet {
            from: at,
            to: at,
            cause,
            actor,
            label,
            keys: Arc::default(),
            order: false,
            pages_added: Vec::new(),
            pages_removed: Vec::new(),
            geometry: Arc::default(),
            content: Arc::default(),
            annots: Arc::default(),
            fields: Vec::new(),
            outline: false,
            labels: false,
            form: false,
            generators: false,
        }
    }

    /// The file's facts made version `to`, with `state`, of version
    /// `from`, which had its pages alone: anything may have changed. It
    /// names no slot, but every flag is set, and every page and field is
    /// in its set.
    pub fn opened(from: VersionId, to: VersionId, label: Arc<str>, state: &DocState) -> ChangeSet {
        let pages: Arc<BTreeSet<PageId>> = Arc::new(state.order().iter().copied().collect());
        ChangeSet {
            to,
            order: true,
            geometry: pages.clone(),
            content: pages.clone(),
            annots: pages,
            fields: state.fields().map(|(id, _)| id).collect(),
            outline: true,
            labels: true,
            form: true,
            generators: true,
            ..ChangeSet::empty(from, Cause::Opened, Actor::User, label)
        }
    }

    /// Whether it changed nothing: no slot, and no flag or set.
    pub fn is_empty(&self) -> bool {
        let flags = self.order || self.outline || self.labels || self.form || self.generators;
        let sets = [&self.geometry, &self.content, &self.annots];
        self.keys.is_empty()
            && !flags
            && self.pages_added.is_empty()
            && self.pages_removed.is_empty()
            && sets.iter().all(|s| s.is_empty())
            && self.fields.is_empty()
    }
}
