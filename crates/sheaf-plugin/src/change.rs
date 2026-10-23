//! What a plugin is told of a change to the document (M10.5 §14.3): what
//! it changed, as scopes a plugin can act on, mapped from the
//! [`ChangeSet`] the change made; the slots it changed stay sheaf's own.
//! A plugin busy with a call is told of every change made meanwhile once
//! it's free, those alike to answer merged ([`ChangeInfo::merge`]), so
//! nothing it should re-read is lost.

use std::collections::BTreeSet;

use sheaf_doc::{Actor, Cause, ChangeSet, PageId, VersionId};

/// One change, or several merged, as a plugin is told.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeInfo {
    /// The version the document is at, and the one it was at before: a
    /// plugin whose last version is `parent` missed nothing, and one whose
    /// isn't re-reads.
    pub version: VersionId,
    pub parent: VersionId,
    /// The document, which a version names a state only within.
    pub instance: u64,
    pub cause: ChangeCause,
    pub actor_kind: ActorKind,
    pub scopes: Scopes,
    /// How far down a chain of answers it is ([`ChangeSet::depth`]): an
    /// edit answering it is one deeper.
    pub depth: u8,
    /// The user moved through history (an undo, a redo, a jump) or a
    /// group's edits were taken back: an edit answering it would fight
    /// the user, and is refused.
    pub undone: bool,
}

/// What made a change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChangeCause {
    /// The file's facts came in.
    Opened,
    Commit,
    /// A commit joined its group's version.
    Amend,
    Undo,
    Redo,
    Jump,
    Saved,
    /// A failed plugin call's edits taken back.
    Rollback,
    /// A group's edits taken back by a version of their own.
    Revert,
    /// Changes of different causes, merged.
    Coalesced,
}

/// Who made a change, by kind: never a name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ActorKind {
    /// The user, at the app or the command line.
    User,
    /// A plugin: another one, since a plugin isn't told of its own.
    Plugin,
    /// A job sheaf runs for the user: text recognition, an import, an
    /// insert, Trim White.
    Job,
    /// Changes by different kinds, merged.
    Mixed,
}

/// What a change changed, as a plugin reads the document: its pages by
/// id, and the rest by part.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Scopes {
    /// The pages' order: pages moved, added or removed. A header's page
    /// numbers may read differently on any page.
    pub order: bool,
    /// Pages whose boxes or turn changed, those added or removed among
    /// them.
    pub geometry: BTreeSet<PageId>,
    /// Pages that may draw something else, or nothing: those whose own
    /// content changed, and those added or removed.
    pub content: BTreeSet<PageId>,
    /// Pages whose annotations changed.
    pub annots: BTreeSet<PageId>,
    pub outline: bool,
    pub labels: bool,
    /// The form, or a field of it.
    pub form: bool,
    /// A watermark, background or header/footer: what any page it's on
    /// draws may differ, which no page set says.
    pub generators: bool,
}

impl ChangeInfo {
    /// What `ch`, a change to document `instance`, is to a plugin.
    pub fn of(ch: &ChangeSet, instance: u64) -> ChangeInfo {
        let (added, removed) = (ch.pages_added.iter(), ch.pages_removed.iter());
        let content = ch.content.iter().chain(added).chain(removed).copied();
        ChangeInfo {
            version: ch.to,
            parent: ch.from,
            instance,
            cause: ChangeCause::of(ch.cause),
            actor_kind: ActorKind::of(&ch.actor),
            scopes: Scopes {
                order: ch.order,
                geometry: ch.geometry.iter().copied().collect(),
                content: content.collect(),
                annots: ch.annots.iter().copied().collect(),
                outline: ch.outline,
                labels: ch.labels,
                form: ch.form || !ch.fields.is_empty(),
                generators: ch.generators,
            },
            depth: ch.depth,
            undone: matches!(
                ch.cause,
                Cause::Undo | Cause::Redo | Cause::Jump | Cause::Rollback | Cause::Revert
            ),
        }
    }

    /// Whether a plugin's edits made in answer to it may land: it wasn't
    /// the user moving through history, nor itself an answer.
    pub fn answerable(&self) -> bool {
        !self.undone && self.depth == 0
    }

    /// `later`, a change made after this one while the plugin was busy,
    /// merged into it: from this one's parent to `later`'s version, what
    /// either changed, coalesced when their causes differ, mixed when
    /// their kinds of actor do. Only changes alike to answer merge, so
    /// a merged change is refused an answer only when each of its parts
    /// would be; `later` comes back when it isn't, or is a change to
    /// another document (one a reopen replaced), to be told on its own.
    pub fn merge(&mut self, later: ChangeInfo) -> Option<ChangeInfo> {
        if later.instance != self.instance || later.answerable() != self.answerable() {
            return Some(later);
        }
        self.version = later.version;
        if later.cause != self.cause {
            self.cause = ChangeCause::Coalesced;
        }
        if later.actor_kind != self.actor_kind {
            self.actor_kind = ActorKind::Mixed;
        }
        self.scopes.merge(later.scopes);
        // Parts no answer may land on: what each was is refused alike.
        self.depth = self.depth.max(later.depth);
        self.undone |= later.undone;
        None
    }
}

impl ChangeCause {
    fn of(cause: Cause) -> ChangeCause {
        match cause {
            Cause::Opened => ChangeCause::Opened,
            Cause::Commit => ChangeCause::Commit,
            Cause::Amend => ChangeCause::Amend,
            Cause::Undo => ChangeCause::Undo,
            Cause::Redo => ChangeCause::Redo,
            Cause::Jump => ChangeCause::Jump,
            Cause::Saved => ChangeCause::Saved,
            Cause::Rollback => ChangeCause::Rollback,
            Cause::Revert => ChangeCause::Revert,
        }
    }
}

impl ActorKind {
    fn of(actor: &Actor) -> ActorKind {
        match actor {
            Actor::Plugin { .. } => ActorKind::Plugin,
            Actor::Job(_) => ActorKind::Job,
            // The command line is the user's; an actor sheaf adds later is
            // too, until this API names its kind.
            _ => ActorKind::User,
        }
    }
}

impl Scopes {
    /// What `other` changed, added to what this does.
    fn merge(&mut self, other: Scopes) {
        self.order |= other.order;
        self.geometry.extend(other.geometry);
        self.content.extend(other.content);
        self.annots.extend(other.annots);
        self.outline |= other.outline;
        self.labels |= other.labels;
        self.form |= other.form;
        self.generators |= other.generators;
    }
}

#[cfg(test)]
mod tests {
    use in_progress::NoProgress;
    use sheaf_doc::tx::fixture as fx;
    use sheaf_doc::{CommitOpts, Document, FixedCount, Store};
    use sheaf_edit::{MarkKind, NewMark, Op, PageRef, Pt, Style, Turn};

    use super::*;

    fn doc(pages: u32) -> Document {
        let store = Store::new();
        let primary = sheaf_doc::store::blank(&store, pages);
        let policy = Box::new(FixedCount(10));
        Document::new(store, primary, "a.pdf".into(), None, &[], policy)
    }

    fn page(d: &Document, i: usize) -> PageId {
        d.page(i).unwrap()
    }

    /// A note on page `page`.
    fn note(page: PageId) -> Op {
        Op::AddMark {
            page: PageRef::Id(page),
            mark: NewMark {
                kind: MarkKind::Note {
                    pos: Pt { x: 50.0, y: 50.0 },
                },
                style: Style {
                    color: [255, 0, 0],
                    opacity: 1.0,
                    stroke: 1.0,
                    fill: None,
                    dashed: false,
                    cloudy: false,
                },
                contents: "Tie out".into(),
                author: None,
                turn: Default::default(),
                asset: None,
            },
            reply_to: None,
            local: None,
            imported: None,
        }
    }

    /// `ops` committed by a plugin as an edit of depth `depth`: what
    /// another plugin is told of it.
    fn by_plugin(d: &mut Document, ops: Vec<Op>, depth: u8) -> ChangeInfo {
        let actor = Actor::Plugin {
            id: "tax".into(),
            version: "1.0".into(),
        };
        let mut tx = d.tx(actor);
        tx.apply_batch(ops, &NoProgress).unwrap();
        let opts = CommitOpts {
            depth,
            ..CommitOpts::labelled("Tax")
        };
        let ch = d.commit(tx, opts).unwrap().change;
        ChangeInfo::of(&ch, 7)
    }

    #[test]
    fn a_change_is_told_by_its_scopes() {
        let mut d = doc(4);
        let [p0, p1, p2] = [0, 1, 2].map(|i| page(&d, i));
        let before = d.current_version();
        let turned = by_plugin(&mut d, vec![fx::rotate(&[p0], Turn::Quarter)], 1);
        let scopes = Scopes {
            geometry: BTreeSet::from([p0]),
            ..Scopes::default()
        };
        assert_eq!(turned.scopes, scopes);
        assert_eq!(
            (turned.cause, turned.actor_kind),
            (ChangeCause::Commit, ActorKind::Plugin)
        );
        assert_eq!(
            (turned.depth, turned.undone, turned.instance),
            (1, false, 7)
        );
        assert_eq!(
            (turned.parent, turned.version),
            (before, d.current_version())
        );
        let noted = ChangeInfo::of(&d.commit_ops(vec![note(p1)], "Note").unwrap().change, 7);
        assert_eq!(noted.scopes.annots, BTreeSet::from([p1]));
        assert_eq!((noted.actor_kind, noted.depth), (ActorKind::User, 0));
        let gone = d
            .commit_ops(vec![fx::delete(&[p2])], "Delete")
            .unwrap()
            .change;
        let gone = ChangeInfo::of(&gone, 7);
        assert!(gone.scopes.order);
        assert_eq!(gone.scopes.content, BTreeSet::from([p2]), "a page removed");
        assert_eq!(gone.scopes.geometry, BTreeSet::from([p2]));
        let marked = fx::bookmark(None, None, "Form 1040", Some(p0));
        let ch = d.commit_ops(vec![marked], "Bookmark").unwrap().change;
        assert!(ChangeInfo::of(&ch, 7).scopes.outline);
        let ch = d.commit_ops(vec![fx::set_header(fx::header("Draft"), None)], "Header");
        let header = ChangeInfo::of(&ch.unwrap().change, 7);
        assert!(header.scopes.generators && header.scopes.content.is_empty());
        let undone = ChangeInfo::of(&d.undo().unwrap().unwrap(), 7);
        assert_eq!((undone.cause, undone.undone), (ChangeCause::Undo, true));
        assert_eq!(undone.depth, 0);
        let redone = ChangeInfo::of(&d.redo().unwrap().unwrap(), 7);
        assert_eq!((redone.cause, redone.undone), (ChangeCause::Redo, true));
    }

    /// Changes made while a plugin was busy, merged: from the first one's
    /// parent to the last one's version, with the pages of every one.
    #[test]
    fn coalesced_changes_carry_the_union_of_their_pages() {
        let mut d = doc(3);
        let [p0, p1, p2] = [0, 1, 2].map(|i| page(&d, i));
        let first = d.current_version();
        let noted = d.commit_ops(vec![note(p2)], "Note").unwrap().change;
        let mut info = ChangeInfo::of(&noted, 7);
        let turned = d
            .commit_ops(vec![fx::rotate(&[p1], Turn::Half)], "Turn")
            .unwrap();
        assert_eq!(info.merge(ChangeInfo::of(&turned.change, 7)), None);
        // A plugin's command: an edit of its own, not an answer.
        let command = by_plugin(&mut d, vec![fx::rotate(&[p0], Turn::Quarter)], 0);
        assert_eq!(info.merge(command), None);
        assert_eq!((info.parent, info.version), (first, d.current_version()));
        assert_eq!(info.scopes.geometry, BTreeSet::from([p0, p1]));
        assert_eq!(info.scopes.annots, BTreeSet::from([p2]));
        assert_eq!(info.cause, ChangeCause::Commit, "all commits");
        assert_eq!((info.actor_kind, info.depth), (ActorKind::Mixed, 0));
        assert!(info.answerable());
        // A change to the document a reopen made is told on its own.
        let other = ChangeInfo::of(&turned.change, 8);
        assert_eq!(info.merge(other.clone()), Some(other));
    }

    /// Only changes alike to answer merge: a user's edit made after a
    /// plugin's answer, or after an undo, is told on its own, so a
    /// plugin may still answer it. Those it may not answer merge.
    #[test]
    fn changes_merge_only_with_those_alike_to_answer() {
        let mut d = doc(3);
        let [p0, p1] = [0, 1].map(|i| page(&d, i));
        let before = d.current_version();
        let mut answer = by_plugin(&mut d, vec![fx::rotate(&[p0], Turn::Quarter)], 1);
        let undone = ChangeInfo::of(&d.undo().unwrap().unwrap(), 7);
        assert!(!answer.answerable() && !undone.answerable());
        assert_eq!(answer.merge(undone.clone()), None, "neither answerable");
        assert_eq!((answer.parent, answer.version), (before, undone.version));
        assert_eq!(
            (answer.cause, answer.undone),
            (ChangeCause::Coalesced, true)
        );
        assert_eq!(answer.depth, 1);
        let user = d.commit_ops(vec![note(p1)], "Note").unwrap().change;
        let user = ChangeInfo::of(&user, 7);
        assert!(user.answerable());
        assert_eq!(answer.merge(user.clone()), Some(user), "apart");
        assert_eq!(answer.version, undone.version, "the rest as it was");
    }
}
