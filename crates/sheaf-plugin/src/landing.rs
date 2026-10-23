//! Where a plugin call's edits land (M10.5 §14.3, proposal 19): its
//! transaction committed as the plugin's, in a group of the call's, so a
//! call's commits are one undo step; or, when the call fails, traps, is
//! stopped or its edits don't land, the group's versions taken back. A
//! commit is made on the version the call read and lands on the current
//! one by lowering its ops again ([`Stale::Relower`]): an edit since to
//! what the call read refuses it, and one elsewhere doesn't; what the
//! call was told its ops made, they must make again.

use std::sync::Arc;

use sheaf_doc::{
    ChangeSet, Commit, CommitOpts, Document, EditError, GroupId, GroupKind, Published, Stale, Tx,
    VersionId,
};

use crate::runtime::{CallOutput, DocEvent, PluginError};

/// A document, and the door its changes go through: the app's view, which
/// tells the reader of each and follows it, or the document itself.
pub trait Door {
    fn model(&mut self) -> &mut Document;
    fn commit(&mut self, tx: Tx, opts: CommitOpts) -> Result<Commit, EditError>;
    /// The document went through `change`, which no commit made: a
    /// rollback's.
    fn follow(&mut self, change: &ChangeSet);
}

impl Door for Document {
    fn model(&mut self) -> &mut Document {
        self
    }

    fn commit(&mut self, tx: Tx, opts: CommitOpts) -> Result<Commit, EditError> {
        Document::commit(self, tx, opts)
    }

    fn follow(&mut self, _: &ChangeSet) {}
}

/// Lands a call's edits while it runs (`edits.commit`), on the thread that
/// owns the document, and waits for it: the app's UI thread.
pub trait Committer: Send + Sync {
    /// `tx`, the call's edits so far, landed in the call's group as
    /// `label`, `depth` down a chain of answers ([`Landing::commit`]).
    fn commit(&self, tx: Tx, label: &str, depth: u8) -> Result<Committed, EditError>;
}

/// What a call's commit made: the version the document is at, and the
/// document as published then, which the call's next edits are made on.
pub struct Committed {
    pub version: VersionId,
    pub instance: u64,
    pub base: Arc<Published>,
}

/// One call's landing: the group its commits make, begun at its first.
pub struct Landing {
    plugin: Arc<str>,
    group: Option<GroupId>,
}

/// How a call's edits landed ([`Landing::finish`]).
#[derive(Debug, Default)]
pub struct Landed {
    /// Its last commit failed, and why, with what its edits are called:
    /// they were taken back.
    pub failed: Option<(String, EditError)>,
    /// Taking them back failed too, the user having changed them since, so
    /// they stay.
    pub kept: bool,
    /// Why its edits were refused as it made them ([`CallOutput::refused`]).
    pub refused: Vec<String>,
}

impl Landing {
    /// Plugin `plugin`'s call, which has made no commit yet.
    pub fn new(plugin: impl Into<Arc<str>>) -> Landing {
        Landing {
            plugin: plugin.into(),
            group: None,
        }
    }

    /// `tx`, the call's, committed through `door` as `label`, `depth` down a
    /// chain of answers: made on the version it was made on, and lowered
    /// again on the current one ([`Stale::Relower`]), in the call's group,
    /// which joins its version unless the user edited since.
    pub fn commit(
        &mut self,
        door: &mut impl Door,
        tx: Tx,
        label: &str,
        depth: u8,
    ) -> Result<Commit, EditError> {
        let plugin = &self.plugin;
        let g = *(self.group).get_or_insert_with(|| {
            let plugin = plugin.clone();
            door.model().begin_group(GroupKind::PluginCall { plugin })
        });
        let opts = CommitOpts {
            group: Some(g),
            depth,
            ..CommitOpts::based(tx.base(), Stale::Relower, label)
        };
        door.commit(tx, opts)
    }

    /// The call over, as it ended (`result`, and whether the user stopped
    /// it, `cancelled`): what it left committed through `door`, and its
    /// group ended; or, when it failed, trapped or was stopped, or that
    /// commit failed, what it left dropped and what it committed taken back
    /// ([`Document::rollback_group`]: its only version gone, with nothing
    /// to redo to it, or else a version that takes them back), each change
    /// followed.
    pub fn finish(
        mut self,
        door: &mut impl Door,
        result: Result<CallOutput, PluginError>,
        cancelled: bool,
    ) -> Landed {
        let mut landed = Landed::default();
        let out = match result {
            Ok(out) => {
                landed.refused = out.refused;
                (!cancelled).then_some((out.tx, out.label, out.depth))
            }
            Err(_) => None,
        };
        let stays = match out {
            Some((Some(tx), label, depth)) => {
                let label = label.unwrap_or_else(|| self.plugin.to_string());
                match self.commit(door, tx, &label, depth) {
                    Ok(_) => true,
                    Err(e) => {
                        landed.failed = Some((label, e));
                        false
                    }
                }
            }
            Some((None, ..)) => true,
            None => false,
        };
        match stays {
            true => self.end_group(door),
            false => landed.kept = !self.roll_back(door),
        }
        landed
    }

    /// The call's group takes no more commits; its versions stay.
    pub fn end_group(self, door: &mut impl Door) {
        if let Some(g) = self.group {
            door.model().end_group(g);
        }
    }

    /// The call's committed versions taken back, each change followed:
    /// false when they couldn't be and stay, the group ended.
    fn roll_back(&mut self, door: &mut impl Door) -> bool {
        let Some(g) = self.group.take() else {
            return true;
        };
        match door.model().rollback_group(g) {
            Ok(change) => {
                change.iter().for_each(|ch| door.follow(ch));
                true
            }
            Err(why) => {
                log::warn!("plugin {}: taking back its edits: {why}", self.plugin);
                door.model().end_group(g);
                false
            }
        }
    }
}

/// What the plugin is told of how its call's edits landed: an
/// `edit-refused` for each refusal, and for a last commit that failed.
pub fn follow_ups(landed: &Landed) -> Vec<DocEvent> {
    let failed = landed.failed.iter().map(|(_, e)| e.to_string());
    (landed.refused.iter().cloned().chain(failed))
        .map(DocEvent::EditRefused)
        .collect()
}

#[cfg(test)]
mod tests {
    use in_progress::NoProgress;
    use sheaf_doc::prim::Key;
    use sheaf_doc::tx::fixture;
    use sheaf_doc::{FixedCount, Identity, PageId, Store};
    use sheaf_edit::{Box2, MarkKind, NewMark, Op, Style};

    use super::*;

    /// `n` blank pages.
    fn blank(n: u32) -> Document {
        let store = Store::new();
        let primary = sheaf_doc::store::blank(&store, n);
        Document::new(
            store,
            primary,
            "a.pdf".into(),
            None,
            &[],
            Box::new(FixedCount(10)),
        )
    }

    /// A transaction of plugin "p"'s on `d` as it's published now.
    fn plugin_tx(d: &Document) -> Tx {
        d.published().plugin_tx("p".into(), "1.0".into(), false)
    }

    /// A call that left `tx`, its edits called `label`.
    fn left(tx: Tx, label: &str) -> Result<CallOutput, PluginError> {
        Ok(CallOutput {
            tx: Some(tx),
            label: Some(label.into()),
            ..CallOutput::default()
        })
    }

    /// A square added to `page`.
    fn square(page: PageId) -> Op {
        let rect = Box2 {
            x: 10.0,
            y: 10.0,
            w: 20.0,
            h: 20.0,
        };
        Op::AddMark {
            page: page.into(),
            mark: NewMark {
                kind: MarkKind::Square { rect },
                style: Style {
                    color: [255, 0, 0],
                    opacity: 1.0,
                    stroke: 1.0,
                    fill: None,
                    dashed: false,
                    cloudy: false,
                },
                contents: String::new(),
                author: None,
                turn: Default::default(),
                asset: None,
            },
            reply_to: None,
            local: None,
            imported: None,
        }
    }

    /// `ops` submitted into `tx`, as a plugin's submit does: what they
    /// made handed out.
    fn submitted(tx: &mut Tx, ops: Vec<Op>) {
        tx.apply_said(ops, &NoProgress).unwrap();
        tx.mark_handed_out();
    }

    /// A plugin's marks are its own: by the plugin, whatever it says, and
    /// with `<<name>>` and `<<initials>>` as it wrote them, since a plugin
    /// not granted `identity` never learns who the user is; the date and
    /// time are filled in, as dynamic stamps are (M10.5 §6.8).
    #[test]
    fn a_plugins_marks_are_its_own_and_never_name_the_user() {
        let mut d = blank(1);
        d.set_identity(Identity {
            person: "Pat Q. Smith".into(),
            initials: "PQS".into(),
            ..Identity::default()
        });
        let stamp = NewMark {
            kind: MarkKind::TextStamp {
                rect: Box2 {
                    x: 0.0,
                    y: 0.0,
                    w: 9.0,
                    h: 9.0,
                },
                text: "REVIEWED\n<<initials>> <<date>>".into(),
                status: false,
            },
            style: Style {
                color: [0, 0, 0],
                opacity: 1.0,
                stroke: 1.0,
                fill: None,
                dashed: false,
                cloudy: false,
            },
            contents: "Checked by <<name>> at <<time>>".into(),
            author: Some("Someone Else".into()),
            turn: Default::default(),
            asset: None,
        };
        let page = d.page(0).unwrap();
        let add = Op::AddMark {
            page: page.into(),
            mark: stamp,
            reply_to: None,
            local: None,
            imported: None,
        };
        let mut tx = d
            .published()
            .plugin_tx("checker".into(), "1.0.0".into(), false);
        submitted(&mut tx, vec![add]);
        let landed = Landing::new("checker").finish(&mut d, left(tx, "Check"), false);
        assert!(landed.failed.is_none(), "{landed:?}");
        let a = d.state().marks_on(page).next().unwrap();
        assert_eq!(a.author, "checker", "the plugin's, not who it says");
        let (said, time) = a.contents.split_at("Checked by <<name>> at ".len());
        assert_eq!(said, "Checked by <<name>> at ");
        assert!(!time.is_empty() && !time.contains("<<"), "{time}");
        let sheaf_doc::AnnotKind::TextStamp { text, .. } = &a.kind else {
            panic!("{:?}", a.kind)
        };
        assert!(text.starts_with("REVIEWED\n<<initials>> "), "{text}");
        assert!(!text.contains("<<date>>"), "{text}");
        let version = &d.timeline()[1];
        assert_eq!(version.author, "checker");
    }

    /// A call's edits land only while what it read holds (M10.5 §14.3):
    /// the user changed the page's annotations it read since, so its mark
    /// on another page is a Conflict naming that edit, told to the plugin,
    /// and the document is as the user left it.
    #[test]
    fn a_call_on_a_page_changed_since_conflicts_and_changes_nothing() {
        let mut d = blank(2);
        let (first, second) = (d.page(0).unwrap(), d.page(1).unwrap());
        let mut tx = plugin_tx(&d);
        tx.read(Key::Page(first));
        tx.read(Key::PageAnnots(first));
        submitted(&mut tx, vec![square(second)]);
        d.commit_ops(vec![square(first)], "Square").unwrap();
        let (at, versions) = (d.current_version(), d.timeline().len());
        let landed = Landing::new("p").finish(&mut d, left(tx, "Total"), false);
        let Some((label, EditError::Conflict { key, by })) = &landed.failed else {
            panic!("{landed:?}")
        };
        assert_eq!((label.as_str(), *key), ("Total", Key::PageAnnots(first)));
        assert_eq!(by.as_deref(), Some("Square"));
        assert!(!landed.kept);
        assert_eq!((d.current_version(), d.timeline().len()), (at, versions));
        assert_eq!(d.state().marks_on(second).count(), 0);
        let told = follow_ups(&landed);
        assert!(
            matches!(&told[..], [DocEvent::EditRefused(why)] if why.contains("Square")),
            "{told:?}"
        );
    }

    /// An edit elsewhere doesn't refuse it: its ops are lowered again on
    /// the current version, making what they made, and both edits stay.
    #[test]
    fn a_call_lands_after_an_edit_to_a_page_it_didn_t_read() {
        let mut d = blank(2);
        let (first, second) = (d.page(0).unwrap(), d.page(1).unwrap());
        let mut tx = plugin_tx(&d);
        tx.read(Key::PageAnnots(first));
        submitted(&mut tx, vec![square(first)]);
        let made: Vec<_> = tx.preview().created.annots().collect();
        d.commit_ops(vec![square(second)], "Square").unwrap();
        let landed = Landing::new("p").finish(&mut d, left(tx, "Total"), false);
        assert!(landed.failed.is_none(), "{landed:?}");
        let st = d.state();
        let on_first: Vec<_> = st.marks_on(first).map(|a| a.id).collect();
        assert_eq!(on_first, made, "the ids it was told");
        assert_eq!(st.marks_on(second).count(), 1);
        assert_eq!(d.undo_label(), Some("Total"));
    }

    /// A call's commits are one undo step (proposal 19): what it commits
    /// before it returns makes a version, and what it leaves joins it.
    #[test]
    fn a_call_s_commits_are_one_undo_step() {
        let mut d = blank(1);
        let first = d.page(0).unwrap();
        let (at, versions) = (d.current_version(), d.timeline().len());
        let mut landing = Landing::new("p");
        let mut tx = plugin_tx(&d);
        submitted(&mut tx, vec![square(first)]);
        let committed = landing.commit(&mut d, tx, "Check", 0).unwrap();
        assert!(!committed.amended);
        let mut tx = plugin_tx(&d);
        submitted(&mut tx, vec![square(first)]);
        let landed = landing.finish(&mut d, left(tx, "Check"), false);
        assert!(landed.failed.is_none(), "{landed:?}");
        assert_eq!(d.state().marks_on(first).count(), 2);
        assert_eq!(d.timeline().len(), versions + 1, "one version");
        assert_eq!(d.undo_label(), Some("Check"));
        d.undo().unwrap();
        assert_eq!(d.current_version(), at);
        assert_eq!(d.state().marks_on(first).count(), 0);
    }

    /// A call that fails, or is stopped, after a commit takes it back: its
    /// only version goes, with nothing to redo to it, and what it left is
    /// dropped.
    #[test]
    fn a_failed_or_stopped_call_takes_back_what_it_committed() {
        for (result, cancelled) in [
            (Err(PluginError::Failed("nope".into())), false),
            (Ok(()), true),
        ] {
            let mut d = blank(1);
            let first = d.page(0).unwrap();
            let (at, versions) = (d.current_version(), d.timeline().len());
            let mut landing = Landing::new("p");
            let mut tx = plugin_tx(&d);
            submitted(&mut tx, vec![square(first)]);
            landing.commit(&mut d, tx, "Check", 0).unwrap();
            let mut tx = plugin_tx(&d);
            submitted(&mut tx, vec![square(first)]);
            let result = result.clone().and_then(|()| left(tx, "Check"));
            let landed = landing.finish(&mut d, result, cancelled);
            assert!(landed.failed.is_none() && !landed.kept, "{landed:?}");
            assert_eq!((d.current_version(), d.timeline().len()), (at, versions));
            assert!(!d.can_redo(), "nothing to redo to it");
            assert_eq!(d.state().marks_on(first).count(), 0);
        }
    }

    /// A document the user edits right after a call's commit, writing over
    /// the call's mark.
    struct EditedBetween(Document);

    impl Door for EditedBetween {
        fn model(&mut self) -> &mut Document {
            &mut self.0
        }

        fn commit(&mut self, tx: Tx, opts: CommitOpts) -> Result<Commit, EditError> {
            let made = self.0.commit(tx, opts)?;
            let page = self.0.page(0).unwrap();
            let mut mark = self.0.state().marks_on(page).next().unwrap().clone();
            mark.contents = "Mine now".into();
            let retell = fixture::update(&mark);
            self.0.commit_ops(vec![retell], "Retell").unwrap();
            Ok(made)
        }

        fn follow(&mut self, _: &ChangeSet) {}
    }

    /// A failed call whose edits can't be taken back, since the user wrote
    /// over them meanwhile, keeps them: the rollback's Conflict changes
    /// nothing, and the call's group ends.
    #[test]
    fn a_call_whose_edits_were_written_over_keeps_them_when_it_fails() {
        let mut door = EditedBetween(blank(1));
        let first = door.0.page(0).unwrap();
        let mut landing = Landing::new("p");
        let mut tx = plugin_tx(&door.0);
        submitted(&mut tx, vec![square(first)]);
        landing.commit(&mut door, tx, "Check", 0).unwrap();
        let failed = Err(PluginError::Trapped("boom".into()));
        let landed = landing.finish(&mut door, failed, false);
        assert!(landed.kept && landed.failed.is_none(), "{landed:?}");
        let d = &door.0;
        assert_eq!(d.undo_label(), Some("Retell"));
        let mark = d.state().marks_on(first).next().unwrap();
        assert_eq!(mark.contents, "Mine now");
    }

    /// A call that left nothing changes nothing, and is told of each
    /// refusal it was answered with.
    #[test]
    fn a_call_that_left_nothing_is_told_its_refusals() {
        let mut d = blank(1);
        let versions = d.timeline().len();
        let out = CallOutput {
            refused: vec!["the user undid this".into()],
            ..CallOutput::default()
        };
        let landed = Landing::new("p").finish(&mut d, Ok(out), false);
        assert_eq!(d.timeline().len(), versions);
        let told = follow_ups(&landed);
        assert_eq!(told, [DocEvent::EditRefused("the user undid this".into())]);
    }
}
