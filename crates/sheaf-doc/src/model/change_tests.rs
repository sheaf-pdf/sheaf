//! What each transition changed (M10.5 §14.1): a commit's and an amend's
//! own prims' keys, by their transaction's actor; an undo's, a redo's and
//! a jump's, the keys of every version on the way, through where the
//! branches meet, by the user; a save's, nothing. And what those keys mean
//! for the pages, the outline and the rest.

use std::collections::BTreeSet;

use in_progress::NoProgress;
use sheaf_edit::{AnnotRef, Delta, Op, PageRef, Turn, Vec2};

use super::tests::{doc, id_at, insert_of, record_for, rotate, square, tl};
use super::*;
use crate::tx::JobKind;
use crate::tx::fixture as fx;

/// `ops` in a transaction of `actor`'s, committed as `label`.
fn commit_as(d: &mut Document, actor: Actor, ops: Vec<Op>, label: &str) -> Commit {
    let mut tx = d.tx(actor);
    tx.apply_batch(ops, &NoProgress).unwrap();
    d.commit(tx, CommitOpts::labelled(label)).unwrap()
}

/// The keys of version `v`'s prims.
fn prim_keys(d: &Document, v: VersionId) -> BTreeSet<Key> {
    (d.versions[&v].steps.iter())
        .flat_map(|s| s.prims.iter().map(|p| p.key))
        .collect()
}

/// A commit changes its prims' keys, the set its version keeps, by its
/// transaction's actor, and says what they are: a page turned, a mark
/// added, the order.
#[test]
fn a_commit_s_keys_are_its_prims() {
    let mut d = doc();
    let (p0, p1) = (id_at(&d, 0), id_at(&d, 1));
    let from = d.current_version();
    let ops = vec![
        fx::rotate(&[p0], Turn::Quarter),
        fx::add(p1, &square(1)),
        fx::move_pages(&[p1], Some(p0)),
    ];
    let c = commit_as(&mut d, Actor::Cli, ops, "three");
    let (v, ch) = (c.version.unwrap(), &c.change);
    assert_eq!((ch.from, ch.to), (from, v));
    let what = (ch.cause, &ch.actor, &*ch.label);
    assert_eq!(what, (Cause::Commit, &Actor::Cli, "three"));
    assert_eq!(*ch.keys, prim_keys(&d, v));
    assert!(
        Arc::ptr_eq(&ch.keys, &d.versions[&v].keys),
        "the version's own"
    );
    assert!(ch.order);
    assert_eq!(*ch.geometry, BTreeSet::from([p0]));
    assert_eq!(*ch.annots, BTreeSet::from([p1]));
    assert!(ch.content.is_empty() && ch.pages_added.is_empty() && ch.fields.is_empty());
    assert!(!(ch.outline || ch.labels || ch.form || ch.generators));
}

/// A mark moved to another page changes the annotations of both: its own
/// slot alone says it was on one and is on the other.
#[test]
fn a_mark_moved_to_another_page_changes_both() {
    let mut d = doc();
    let (p0, p1) = (id_at(&d, 0), id_at(&d, 1));
    d.commit_ops(vec![fx::add(p0, &square(1))], "add").unwrap();
    let a = d.state().marks_on(p0).next().unwrap().id;
    let before = d.state().clone();
    let to_p1 = Op::MoveMarks {
        marks: vec![AnnotRef::Id(a)],
        by: Delta::Page(Vec2 { x: 5.0, y: 0.0 }),
        to_page: Some(PageRef::Id(p1)),
    };
    let c = d.commit_ops(vec![to_p1], "move").unwrap();
    let both = BTreeSet::from([p0, p1]);
    assert_eq!(*c.change.annots, both);
    let alone = Arc::new(BTreeSet::from([Key::Annot(a)]));
    let (from, to) = (c.change.from, c.change.to);
    let label = c.change.label.clone();
    let ch = ChangeSet::of(
        from,
        to,
        Cause::Commit,
        Actor::User,
        label,
        alone,
        &before,
        d.state(),
    );
    assert_eq!(*ch.annots, both);
}

/// An insert adds its pages, and a delete removes its, by id; both change
/// the order, and a delete the bookmarks that went to its pages.
#[test]
fn inserts_and_deletes_add_and_remove_pages() {
    let mut d = doc();
    let src = crate::store::blank(d.store(), 2);
    let c = d
        .commit_ops(vec![insert_of(&src, "a.pdf")], "insert")
        .unwrap();
    let made: Vec<PageId> = c.created.pages().collect();
    assert_eq!(made.len(), 2);
    assert_eq!(c.change.pages_added, made);
    assert!(c.change.order && c.change.pages_removed.is_empty());
    assert!(c.change.content.is_empty());
    let gone = vec![id_at(&d, 0), id_at(&d, 3)];
    let c = d.commit_ops(vec![fx::delete(&gone)], "delete").unwrap();
    assert_eq!(c.change.pages_removed, gone);
    assert!(c.change.order && c.change.outline && c.change.pages_added.is_empty());
}

/// A turned page's geometry changes; recognized text changes what a page
/// shows, by the job's actor.
#[test]
fn a_rotate_changes_geometry_and_recognized_text_content() {
    let mut d = doc();
    let (p0, p1) = (id_at(&d, 0), id_at(&d, 1));
    let turn = vec![fx::rotate(&[p0], Turn::Quarter)];
    let c = d.commit_ops(turn, "rotate").unwrap();
    assert_eq!(*c.change.geometry, BTreeSet::from([p0]));
    assert!(c.change.content.is_empty() && !c.change.order);
    let ocr = Actor::Job(JobKind::Ocr);
    let c = commit_as(&mut d, ocr.clone(), fx::ocr([(p1, tl("a"))]), "ocr");
    assert_eq!(*c.change.content, BTreeSet::from([p1]));
    assert!(c.change.geometry.is_empty() && c.change.annots.is_empty());
    assert_eq!(c.change.actor, ocr);
}

/// Undo, redo and a jump are the user's, whoever made the versions, and
/// change the keys of every version on the way: a jump across branches,
/// both branches' keys, back to where they meet and no further. A jump to
/// where the document is changes nothing.
#[test]
fn undo_redo_and_jump_change_the_keys_on_the_way() {
    let mut d = doc();
    let (p1, p2) = (id_at(&d, 1), id_at(&d, 2));
    rotate(&mut d, 0, "before");
    let fork = d.current_version();
    rotate(&mut d, 1, "turn");
    let turned = d.current_version();
    let ch = d.undo().unwrap().unwrap();
    assert_eq!((ch.from, ch.to, ch.cause), (turned, fork, Cause::Undo));
    assert_eq!((&ch.actor, &*ch.label), (&Actor::User, "turn"));
    assert_eq!(*ch.keys, BTreeSet::from([Key::Geom(p1)]));
    assert_eq!(*ch.geometry, BTreeSet::from([p1]));
    let ch = d.redo().unwrap().unwrap();
    assert_eq!((ch.from, ch.to, ch.cause), (fork, turned, Cause::Redo));
    assert_eq!((&ch.actor, &*ch.label), (&Actor::User, "turn"));
    assert_eq!(*ch.keys, BTreeSet::from([Key::Geom(p1)]));
    // Another branch from the fork, recognized text.
    d.jump(fork).unwrap();
    let ocr = Actor::Job(JobKind::Ocr);
    commit_as(&mut d, ocr, fx::ocr([(p2, tl("a"))]), "ocr");
    let read = d.current_version();
    let ch = d.jump(turned).unwrap().unwrap();
    assert_eq!((ch.from, ch.to, ch.cause), (read, turned, Cause::Jump));
    assert_eq!((&ch.actor, &*ch.label), (&Actor::User, "turn"));
    let keys = BTreeSet::from([Key::Page(p2), Key::Geom(p1)]);
    assert_eq!(*ch.keys, keys, "none from before the fork");
    assert_eq!((&*ch.content, &*ch.geometry), (&[p2].into(), &[p1].into()));
    assert_eq!(d.jump(turned), Ok(None));
    // Undoing the job's version is the user's too.
    d.jump(read).unwrap();
    let ch = d.undo().unwrap().unwrap();
    assert_eq!(
        (ch.cause, &ch.actor, &*ch.label),
        (Cause::Undo, &Actor::User, "ocr")
    );
    assert_eq!(*ch.content, BTreeSet::from([p2]));
}

/// A page's slot is looked up only on a path that changed the order
/// ([`ChangeSet::of`]): a jump from a branch that recognized text to one
/// that inserted a page adds that page and changes the read one, and an
/// undo of the insert removes it; with the order as it was, every page
/// whose slot changed is in both.
#[test]
fn a_page_s_slot_is_judged_by_whether_the_order_changed() {
    let mut d = doc();
    let p2 = id_at(&d, 2);
    let fork = d.current_version();
    let src = crate::store::blank(d.store(), 1);
    let c = d
        .commit_ops(vec![insert_of(&src, "a.pdf")], "insert")
        .unwrap();
    let new: Vec<PageId> = c.created.pages().collect();
    let inserted = d.current_version();
    d.jump(fork).unwrap();
    let c = d.commit_ops(fx::ocr([(p2, tl("a"))]), "ocr").unwrap();
    assert!(!c.change.order);
    assert_eq!(*c.change.content, BTreeSet::from([p2]));
    let ch = d.jump(inserted).unwrap().unwrap();
    assert!(ch.order && ch.pages_removed.is_empty());
    assert_eq!(ch.pages_added, new);
    assert_eq!(*ch.content, BTreeSet::from([p2]));
    let ch = d.undo().unwrap().unwrap();
    assert_eq!(ch.pages_removed, new);
    assert!(ch.order && ch.content.is_empty() && ch.pages_added.is_empty());
}

/// A save changes nothing the document shows: its change is empty, at the
/// current version, though an edit came after the version it wrote, and
/// the user's. So is an Unchanged one's.
#[test]
fn a_save_changes_nothing() {
    let mut d = doc();
    rotate(&mut d, 0, "turn");
    let v = d.current_version();
    d.save_input(v).unwrap();
    rotate(&mut d, 1, "while it saves");
    let at = d.current_version();
    let ch = d.record_save(record_for(&d, v)).unwrap();
    assert_eq!((ch.from, ch.to, ch.cause), (at, at, Cause::Saved));
    assert_eq!(ch.actor, Actor::User);
    assert!(ch.is_empty());
    let ch = d.record_unchanged(record_for(&d, at)).unwrap();
    assert_eq!((ch.from, ch.to, ch.cause), (at, at, Cause::Saved));
    assert!(ch.is_empty());
}

/// A commit that changes nothing changes nothing, at the current version.
#[test]
fn a_commit_of_nothing_is_empty() {
    let mut d = doc();
    let at = d.current_version();
    let c = d
        .commit_ops(vec![fx::rotate(&[], Turn::Quarter)], "none")
        .unwrap();
    assert_eq!(c.version, None);
    assert_eq!((c.change.from, c.change.to), (at, at));
    assert_eq!((c.change.cause, &*c.change.label), (Cause::Commit, "none"));
    assert!(c.change.is_empty());
}

/// An amend's change is its own commit's, from the version it replaced to
/// the one in its place, by its transaction's actor, while that version
/// holds every commit's keys. One that puts back all the group did goes
/// back to the parent.
#[test]
fn an_amend_changes_its_own_keys() {
    let mut d = doc();
    let (p0, p1) = (id_at(&d, 0), id_at(&d, 1));
    let parent = d.current_version();
    let g = d.begin_group(GroupKind::Ocr);
    let ocr = Actor::Job(JobKind::Ocr);
    let batch = |d: &mut Document, ops: Vec<Op>| {
        let mut tx = d.tx(ocr.clone());
        tx.apply_batch(ops, &NoProgress).unwrap();
        let opts = CommitOpts {
            group: Some(g),
            ..CommitOpts::labelled("Recognize text")
        };
        d.commit(tx, opts).unwrap()
    };
    let c = batch(&mut d, fx::ocr([(p0, tl("a"))]));
    let v1 = c.version.unwrap();
    assert_eq!((c.change.from, c.change.cause), (parent, Cause::Commit));
    let c = batch(&mut d, fx::ocr([(p1, tl("b"))]));
    let v2 = c.version.unwrap();
    assert!(c.amended);
    assert_eq!(
        (c.change.from, c.change.to, c.change.cause),
        (v1, v2, Cause::Amend)
    );
    assert_eq!(c.change.actor, ocr);
    assert_eq!(*c.change.keys, BTreeSet::from([Key::Page(p1)]));
    assert_eq!(*c.change.content, BTreeSet::from([p1]));
    let both = BTreeSet::from([Key::Page(p0), Key::Page(p1)]);
    assert_eq!(*d.versions[&v2].keys, both);
    assert_eq!(*d.versions[&v2].keys, prim_keys(&d, v2));
    let c = batch(&mut d, vec![fx::clear_ocr(&[p0, p1])]);
    assert_eq!((c.change.from, c.change.to), (v2, parent));
    assert_eq!((c.change.cause, &*c.change.keys), (Cause::Amend, &both));
}

/// Labels, a header and a form's page each say what they changed, and so
/// does each one's undo: the page labels, the generators, and the form
/// with its field.
#[test]
fn labels_generators_and_fields_are_named() {
    let mut d = doc();
    let p0 = id_at(&d, 0);
    let said = |ch: &ChangeSet| (ch.labels, ch.generators, ch.form, ch.fields.clone());
    let roman = crate::labels::LabelSpec {
        start: p0,
        style: crate::labels::Style::LowerRoman,
        prefix: String::new(),
        first: 1,
    };
    let c = d.commit_ops(vec![fx::labels(&[roman])], "labels").unwrap();
    assert_eq!(said(&c.change), (true, false, false, vec![]));
    assert_eq!(
        said(&d.undo().unwrap().unwrap()),
        (true, false, false, vec![])
    );
    let header = fx::set_header(fx::header("Page <<1>>"), None);
    let c = d.commit_ops(vec![header], "header").unwrap();
    assert_eq!(said(&c.change), (false, true, false, vec![]));
    assert_eq!(
        said(&d.undo().unwrap().unwrap()),
        (false, true, false, vec![])
    );
    let mut form = crate::forms::fixture::Form::new(1);
    form.text("field", 0, crate::forms::fixture::row(0));
    let kind = BlobKind::file(Path::new("form.pdf"), None);
    let src = d.store().open(form.bytes().into(), kind).unwrap();
    let c = d
        .commit_ops(vec![insert_of(&src, "form.pdf")], "form")
        .unwrap();
    let fields: Vec<FieldId> = d.state().fields().map(|(id, _)| id).collect();
    assert_eq!(fields.len(), 1);
    assert_eq!(said(&c.change), (false, false, true, fields.clone()));
    assert_eq!(
        said(&d.undo().unwrap().unwrap()),
        (false, false, true, fields)
    );
}
