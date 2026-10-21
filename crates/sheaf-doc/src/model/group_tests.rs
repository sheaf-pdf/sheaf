//! Groups (M10.5 §8.4): an OCR run's flushes and a plugin call's batches
//! as one undo step, or as fragments around the user's edits; a commit in
//! a group whose edits were undone changes nothing; History's "Undo all
//! of …" and a failed call's rollback take a group's versions back, and
//! keep what was edited between them, or refuse and change nothing.

use std::collections::BTreeSet;
use std::sync::Arc;

use in_progress::NoProgress;
use sheaf_edit::{Op, Turn};

use super::tests::{doc, id_at, record_for, tl};
use super::*;
use crate::history::{FixedCount, MAX_VERSIONS};
use crate::realize::ViewInputs;
use crate::tx::fixture::{self as fx, ids};
use crate::tx::{Base, Derive, JobKind, Stale};

/// An OCR run: its group, and the version its pages were rendered from.
struct Ocr {
    group: GroupId,
    base: Snapshot,
}

/// An OCR run begun on the current version.
fn start(d: &mut Document) -> Ocr {
    Ocr {
        group: d.begin_group(GroupKind::Ocr),
        base: d.snapshot(),
    }
}

/// One of `run`'s flushes, as the app makes it: `layers`' text set on
/// their pages, made on the version the run began on, deciding by each
/// page's content as it was rendered, and landed by value in the run's
/// group.
fn flush(d: &mut Document, run: &Ocr, layers: &[(PageId, &str)]) -> Result<Commit, EditError> {
    let mut tx = Document::tx_at(&run.base, d.lower_cx(Actor::Job(JobKind::Ocr)));
    for &(page, _) in layers {
        tx.read_render(page, &ViewInputs::content_only());
    }
    let ops = fx::ocr(layers.iter().map(|&(page, text)| (page, tl(text))));
    tx.apply_batch(ops, &NoProgress)?;
    let opts = CommitOpts {
        base: Base::At(run.base.version),
        stale: Stale::Validate,
        group: Some(run.group),
        ..CommitOpts::labelled("Recognize text")
    };
    d.commit(tx, opts)
}

/// The pages that have recognized text, in order.
fn recognized(d: &Document) -> Vec<PageId> {
    (d.state().pages())
        .filter(|(_, def, _)| def.content.ocr_text().is_some())
        .map(|(id, _, _)| id)
        .collect()
}

/// A plugin call's group, plugin `plugin`'s.
fn call(d: &mut Document, plugin: &str) -> GroupId {
    d.begin_group(GroupKind::PluginCall {
        plugin: plugin.into(),
    })
}

/// The current version, the version count and the state.
fn where_at(d: &Document) -> (VersionId, usize, DocState) {
    (d.current_version(), d.version_count(), d.state().clone())
}

/// The labels of the versions from the opened one to the current one.
fn line(d: &Document) -> Vec<String> {
    (d.line().iter())
        .map(|id| d.versions[id].label.clone())
        .collect()
}

/// Undo during an OCR run takes its flushes back, and its next flush is
/// GroupEnded and changes nothing: undone results are never put back.
#[test]
fn undo_during_ocr_stops_its_flushes() {
    let mut d = doc();
    let (p0, p1) = (id_at(&d, 0), id_at(&d, 1));
    let run = start(&mut d);
    flush(&mut d, &run, &[(p0, "a")]).unwrap();
    d.undo().unwrap();
    let at = where_at(&d);
    let e = flush(&mut d, &run, &[(p1, "b")]).unwrap_err();
    assert_eq!(e, EditError::GroupEnded);
    assert_eq!(where_at(&d), at);
    assert!(recognized(&d).is_empty());
    // A redo before the next flush brings the run back.
    d.redo().unwrap();
    let c = flush(&mut d, &run, &[(p1, "b")]).unwrap();
    assert!(c.amended);
    assert_eq!(recognized(&d), [p0, p1]);
}

/// OCR flushes around the user's edit leave the run in fragments, the
/// later one saying it continues the first; "Undo all" takes back every
/// fragment's text in one version of the user's and keeps the edit. The
/// run's next flush is GroupEnded.
#[test]
fn ocr_around_a_user_edit_is_fragments_undone_all_at_once() {
    let mut d = doc();
    let [p0, p1, p5] = [0, 1, 5].map(|i| id_at(&d, i));
    let run = start(&mut d);
    flush(&mut d, &run, &[(p0, "a")]).unwrap();
    d.commit_ops(vec![fx::rotate(&[p5], Turn::Quarter)], "Rotate")
        .unwrap();
    let c = flush(&mut d, &run, &[(p1, "b")]).unwrap();
    assert!(!c.amended);
    assert_eq!(c.change.label.as_ref(), "Recognize text (continued)");
    let tail = &line(&d)[1..];
    assert_eq!(
        tail,
        ["Recognize text", "Rotate", "Recognize text (continued)"]
    );
    let grouped: Vec<_> = (d.timeline().into_iter())
        .filter_map(|view| view.group)
        .collect();
    let named = (run.group, Arc::from("Recognize text"));
    assert_eq!(grouped, [named.clone(), named]);
    let (n, from) = (d.version_count(), d.current_version());
    let c = d.revert_group(run.group, "Recognize text").unwrap();
    assert_eq!(d.version_count(), n + 1);
    assert_eq!(c.version, Some(d.current_version()));
    assert_eq!((c.change.from, c.change.cause), (from, Cause::Revert));
    assert_eq!(c.change.actor, Actor::User);
    assert_eq!(*c.change.content, BTreeSet::from([p0, p1]));
    let lines: Vec<&str> = c.lines.iter().map(|l| l.text.as_str()).collect();
    assert_eq!(lines, ["Undo all of Recognize text"]);
    assert_eq!(d.undo_label(), Some("Undo all of Recognize text"));
    assert!(recognized(&d).is_empty());
    assert_eq!(d.geom(p5).rotate, Rotation::R90, "the user's edit stays");
    let p2 = id_at(&d, 2);
    let e = flush(&mut d, &run, &[(p2, "c")]).unwrap_err();
    assert_eq!(e, EditError::GroupEnded);
    // Undone, the fragments are back.
    d.undo().unwrap();
    assert_eq!(recognized(&d), [p0, p1]);
}

/// A crop of a page during an OCR run writes the page's boxes alone, and
/// the run's flush decides by the page's content: its text lands on the
/// cropped page, and the crop holds.
#[test]
fn an_ocr_flush_lands_after_a_crop_of_its_page() {
    let mut d = doc();
    let p0 = id_at(&d, 0);
    let run = start(&mut d);
    let margins = sheaf_edit::Margins {
        top: 36.0,
        bottom: 36.0,
        left: 36.0,
        right: 36.0,
    };
    d.commit_ops(vec![fx::crop(&[p0], margins)], "Crop page 1")
        .unwrap();
    let cropped = d.geom(p0);
    assert_ne!(cropped.crop, cropped.media);
    flush(&mut d, &run, &[(p0, "a")]).unwrap();
    assert_eq!(recognized(&d), [p0]);
    assert_eq!(d.geom(p0), cropped, "the crop holds");
}

/// A plugin call's versions around the user's edit, rolled back after the
/// call failed: one version, the plugin's, takes back its edits alone,
/// and its versions stay in the history.
#[test]
fn a_plugin_rollback_reverts_only_its_versions() {
    let mut d = doc();
    let [p0, p1, p2] = [0, 1, 2].map(|i| id_at(&d, i));
    let g = call(&mut d, "turner");
    d.commit_ops_in_group(g, vec![fx::rotate(&[p0], Turn::Quarter)], "Turn")
        .unwrap();
    let first = d.current_version();
    d.commit_ops(vec![fx::rotate(&[p1], Turn::Half)], "Rotate")
        .unwrap();
    d.commit_ops_in_group(g, vec![fx::rotate(&[p2], Turn::Quarter)], "Turn")
        .unwrap();
    let (n, from) = (d.version_count(), d.current_version());
    let change = d.rollback_group(g).unwrap().unwrap();
    assert_eq!((change.from, change.cause), (from, Cause::Rollback));
    assert_eq!(change.to, d.current_version());
    let plugin = Actor::Plugin {
        id: "turner".into(),
        version: "1.0".into(),
    };
    assert_eq!(change.actor, plugin);
    assert_eq!(*change.geometry, BTreeSet::from([p0, p2]));
    assert_eq!(d.version_count(), n + 1, "its versions stay");
    assert!(d.edge(first).is_some() && d.edge(from).is_some());
    let turns = [p0, p1, p2].map(|p| d.geom(p).rotate);
    assert_eq!(turns, [Rotation::R0, Rotation::R180, Rotation::R0]);
    assert_eq!(d.undo_label(), Some("Undo all of Turn"));
    let e = d.commit_ops_in_group(g, vec![fx::rotate(&[p0], Turn::Quarter)], "Turn");
    assert_eq!(e.unwrap_err(), EditError::GroupEnded, "the group ended");
}

/// A call's only version, the current one, rolled back: it goes, the
/// cursor back where the call began, with nothing to redo to it. A call
/// that made none rolls back nothing.
#[test]
fn a_rollback_of_a_call_s_only_version_takes_it_out() {
    let mut d = doc();
    let p0 = id_at(&d, 0);
    let g = call(&mut d, "nothing");
    assert_eq!(d.rollback_group(g).unwrap(), None);
    let (at, n, st) = where_at(&d);
    let g = call(&mut d, "turner");
    d.commit_ops_in_group(g, vec![fx::rotate(&[p0], Turn::Quarter)], "Turn")
        .unwrap();
    let v = d.current_version();
    let change = d.rollback_group(g).unwrap().unwrap();
    assert_eq!((change.from, change.to), (v, at));
    assert_eq!(
        (change.cause, change.label.as_ref()),
        (Cause::Rollback, "Turn")
    );
    assert_eq!(*change.geometry, BTreeSet::from([p0]));
    assert_eq!(where_at(&d), (at, n, st));
    assert!(d.edge(v).is_none() && !d.can_redo());
}

/// A rollback after the call's version was saved keeps that version as it
/// was written: a version after it takes the call's edits back, and the
/// caller has its change for the notice it gives.
#[test]
fn a_rollback_after_a_save_keeps_the_saved_version() {
    let mut d = doc();
    let p0 = id_at(&d, 0);
    let g = call(&mut d, "turner");
    d.commit_ops_in_group(g, vec![fx::rotate(&[p0], Turn::Quarter)], "Turn")
        .unwrap();
    let v = d.current_version();
    d.save_input(v).unwrap();
    d.record_save(record_for(&d, v)).unwrap();
    let change = d.rollback_group(g).unwrap().unwrap();
    assert_eq!((change.from, change.cause), (v, Cause::Rollback));
    assert_eq!(d.versions[&d.current_version()].parent, Some(v));
    assert_eq!(d.geom(p0).rotate, Rotation::R0);
    assert!(d.is_dirty());
    d.undo().unwrap();
    assert_eq!(d.current_version(), v);
    assert!(!d.is_dirty(), "the saved version is as it was written");
}

/// "Undo all" over a later edit that changed what the group wrote is a
/// Conflict naming that edit, and changes nothing.
#[test]
fn a_revert_over_a_later_edit_names_it() {
    let mut d = doc();
    let p0 = id_at(&d, 0);
    let copy = crate::store::blank(d.store(), 1);
    let run = start(&mut d);
    flush(&mut d, &run, &[(p0, "a")]).unwrap();
    let strip = Derive::StripHeaderFooter;
    d.commit_page_base(vec![(p0, copy, 0)], strip, "Strip page 1")
        .unwrap();
    let at = where_at(&d);
    let e = d.revert_group(run.group, "Recognize text").unwrap_err();
    let by = Some("Strip page 1".into());
    assert_eq!(
        e,
        EditError::Conflict {
            key: Key::Page(p0),
            by
        }
    );
    assert_eq!(where_at(&d), at);
}

/// A call's turns of a page around the user's turn of the same page: "Undo
/// all" can't take the earlier one back over the user's, and the Conflict
/// names the user's edit, not the call's later fragment, which changed the
/// page last.
#[test]
fn a_revert_names_the_edit_between_its_fragments() {
    let mut d = doc();
    let p0 = id_at(&d, 0);
    let g = call(&mut d, "turner");
    let turn = || vec![fx::rotate(&[p0], Turn::Quarter)];
    d.commit_ops_in_group(g, turn(), "Turn").unwrap();
    d.commit_ops(vec![fx::rotate(&[p0], Turn::Half)], "Rotate")
        .unwrap();
    d.commit_ops_in_group(g, turn(), "Turn").unwrap();
    let at = where_at(&d);
    let e = d.revert_group(g, "Turn").unwrap_err();
    let by = Some("Rotate".into());
    let key = Key::Geom(p0);
    assert_eq!(e, EditError::Conflict { key, by });
    assert_eq!(where_at(&d), at);
}

/// A plugin's copy of a page, then the user's bookmark to the copy: taking
/// the copy back would leave the bookmark going nowhere, so "Undo all" is
/// a Conflict naming the bookmark's edit, and changes nothing.
#[test]
fn a_revert_that_would_leave_a_dangling_bookmark_conflicts() {
    let mut d = doc();
    let p0 = id_at(&d, 0);
    let g = call(&mut d, "copier");
    let copy = Op::DuplicatePages {
        pages: ids(&[p0]),
        locals: Vec::new(),
    };
    let c = d.commit_ops_in_group(g, vec![copy], "Copy").unwrap();
    let made = c.created.pages().next().unwrap();
    let bookmark = fx::bookmark(None, None, "The copy", Some(made));
    d.commit_ops(vec![bookmark], "Bookmark").unwrap();
    let at = where_at(&d);
    let e = d.revert_group(g, "Copy").unwrap_err();
    let EditError::Conflict { key, by } = e else {
        panic!("{e}");
    };
    assert!(matches!(key, Key::Bookmark(_)), "{key:?}");
    assert_eq!(by.as_deref(), Some("Bookmark"));
    assert_eq!(where_at(&d), at);
}

/// A version after the current one turning page `i`.
fn turn(d: &mut Document, i: usize) {
    let page = id_at(d, i);
    d.commit_ops(vec![fx::rotate(&[page], Turn::Quarter)], "Rotate")
        .unwrap();
}

/// The current version saved: the root may move up to it.
fn save(d: &mut Document) {
    let v = d.current_version();
    d.save_input(v).unwrap();
    d.record_save(record_for(d, v)).unwrap();
}

/// "Undo all" of a group none of whose versions are on the current line,
/// or one whose first version is now the first the history keeps (its
/// edge from before is gone), is Invalid, and changes nothing.
#[test]
fn a_revert_of_what_isn_t_here_is_invalid() {
    let mut d = doc();
    let p0 = id_at(&d, 0);
    let run = start(&mut d);
    flush(&mut d, &run, &[(p0, "a")]).unwrap();
    d.undo().unwrap();
    let at = where_at(&d);
    let e = d.revert_group(run.group, "Recognize text").unwrap_err();
    assert!(matches!(e, EditError::Invalid { .. }), "{e}");
    assert_eq!(where_at(&d), at);
    d.redo().unwrap();
    // The opened version, the run's, then as many more as make one too
    // many: the opened one goes.
    turn(&mut d, 1);
    save(&mut d);
    for _ in 2..MAX_VERSIONS {
        turn(&mut d, 1);
    }
    let first = d.line()[0];
    assert_eq!(
        d.versions[&first].group.as_ref().map(|t| t.0),
        Some(run.group)
    );
    let at = where_at(&d);
    let e = d.revert_group(run.group, "Recognize text").unwrap_err();
    assert!(e.to_string().contains("older than the history kept"), "{e}");
    assert_eq!(where_at(&d), at);
}

/// Once pruning took a group's first fragment, "Undo all" of the
/// fragments left would take back only part of it, and says the group is
/// older than the history kept, as it does while the first fragment is
/// the first version kept; History no longer offers it.
#[test]
fn a_revert_of_a_group_pruning_cut_is_invalid() {
    let mut d = doc();
    let (p0, p1) = (id_at(&d, 0), id_at(&d, 1));
    let run = start(&mut d);
    flush(&mut d, &run, &[(p0, "a")]).unwrap();
    turn(&mut d, 2);
    flush(&mut d, &run, &[(p1, "b")]).unwrap();
    for left in [
        ["Recognize text", "Rotate", "Recognize text (continued)"],
        ["Rotate", "Recognize text (continued)", ""],
    ] {
        assert!(d.prune_one());
        let left: Vec<&str> = left.into_iter().filter(|l| !l.is_empty()).collect();
        assert_eq!(line(&d), left);
        let at = where_at(&d);
        let e = d.revert_group(run.group, "Recognize text").unwrap_err();
        assert!(e.to_string().contains("older than the history kept"), "{e}");
        assert_eq!(where_at(&d), at);
        assert!(d.timeline().iter().all(|v| v.group.is_none()));
    }
    assert_eq!(recognized(&d), [p0, p1]);
}

/// A rollback of a group pruning took part of keeps it all, as its
/// revert does, even when what's left is one version it could take out:
/// the history property's shrunk case.
#[test]
fn a_rollback_of_a_group_pruning_cut_changes_nothing() {
    let mut d = doc();
    let (p0, p1) = (id_at(&d, 0), id_at(&d, 1));
    let run = start(&mut d);
    flush(&mut d, &run, &[(p0, "a")]).unwrap();
    turn(&mut d, 2);
    assert!(d.prune_one() && d.prune_one());
    flush(&mut d, &run, &[(p1, "b")]).unwrap();
    let at = where_at(&d);
    let e = d.rollback_group(run.group).unwrap_err();
    assert!(e.to_string().contains("older than the history kept"), "{e}");
    assert_eq!(where_at(&d), at);
    assert_eq!(recognized(&d), [p0, p1]);
}

/// Pruning an undone group's latest version ends the group; the root
/// moving past a group's latest leaves it none, and the group's next
/// commit makes its first version again.
#[test]
fn pruning_a_group_s_latest_ends_it_or_leaves_it_none() {
    // Undone, then pruned as a branch.
    let mut d = super::tests::new_doc(3, &[], Box::new(FixedCount(1)));
    let (p0, p1) = (id_at(&d, 0), id_at(&d, 1));
    let run = start(&mut d);
    flush(&mut d, &run, &[(p0, "a")]).unwrap();
    d.undo().unwrap();
    for i in 0..MAX_VERSIONS {
        turn(&mut d, i % 3);
    }
    assert!(!d.groups.contains_key(&run.group), "ended");
    // Passed by the root as the history grows past `MAX_VERSIONS`.
    let mut d = super::tests::new_doc(3, &[], Box::new(FixedCount(1)));
    let run = start(&mut d);
    flush(&mut d, &run, &[(p0, "a")]).unwrap();
    turn(&mut d, 2);
    save(&mut d);
    for i in 1..MAX_VERSIONS {
        turn(&mut d, i % 3);
    }
    assert_eq!(d.groups[&run.group].latest, None);
    let c = flush(&mut d, &run, &[(p1, "b")]).unwrap();
    assert!(!c.amended);
    assert_eq!(d.undo_label(), Some("Recognize text"));
    assert_eq!(d.groups[&run.group].latest, Some(d.current_version()));
}

/// A group takes the commits of the job or plugin its kind says, and
/// another's is a bug: a debug build stops there, and a release build
/// changes nothing.
#[test]
fn a_commit_in_a_group_by_another_actor_is_a_bug() {
    let mut d = doc();
    let p0 = id_at(&d, 0);
    let g = d.begin_group(GroupKind::Ocr);
    let mut tx = d.tx(Actor::User);
    tx.apply(fx::rotate(&[p0], Turn::Quarter), &NoProgress)
        .unwrap();
    let at = where_at(&d);
    let opts = CommitOpts {
        group: Some(g),
        ..CommitOpts::default()
    };
    let commit = std::panic::AssertUnwindSafe(|| d.commit(tx, opts));
    let refused = std::panic::catch_unwind(commit);
    assert_eq!(
        refused.is_err(),
        cfg!(debug_assertions),
        "panics in debug only"
    );
    if let Ok(r) = refused {
        assert!(matches!(r, Err(EditError::Internal { .. })), "{r:?}");
    }
    assert_eq!(where_at(&d), at);
}
