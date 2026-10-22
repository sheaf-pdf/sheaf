//! Transactions made on a version the document has since moved on from
//! (M10.5 §8.5): each made on a snapshot ([`Document::tx_at`]), another
//! edit committed, then the transaction committed on the version it was
//! made on, validated by value or lowered again.

use std::sync::Arc;

use in_progress::NoProgress;
use lopdf::{Object, dictionary};
use sheaf_edit::{Anchor, Bring, Op, OutlineBring, PageRef, PageSel, Rotation, Turn};

use super::fixture as fx;
use super::tests::{doc_of, note, rect, style};
use super::*;
use crate::history::MAX_VERSIONS;
use crate::labels::LabelSpec;
use crate::model::Document;
use crate::text_layer::TextLayer;

/// The version `d` is at, and a transaction of the user's on it, as a
/// planner or a dialog takes one.
pub(super) fn planned(d: &Document) -> (VersionId, Tx) {
    let snap = d.snapshot();
    (
        snap.version,
        Document::tx_at(&snap, d.lower_cx(Actor::User)),
    )
}

/// A commit of a transaction made on version `v`, landing as `stale`
/// says.
pub(super) fn on(v: VersionId, stale: Stale) -> CommitOpts {
    CommitOpts::based(v, stale, "stale")
}

/// `d`'s pages, in order.
fn pages<const N: usize>(d: &Document) -> [PageId; N] {
    std::array::from_fn(|i| d.page(i).unwrap())
}

/// `by` points off each side of a page.
fn all(by: f32) -> sheaf_edit::Margins {
    sheaf_edit::Margins {
        top: by,
        bottom: by,
        left: by,
        right: by,
    }
}

/// Recognized text of one word, `text`.
fn layer(text: &str) -> Arc<TextLayer> {
    Arc::new(TextLayer {
        words: vec![crate::text_layer::OcrWord {
            text: text.into(),
            rect: rect(40.0),
            confidence: 0.9,
        }],
        lines: std::iter::once(0..1).collect(),
        upright: Rotation::R0,
        engine: "test".into(),
    })
}

/// A connector pair from page `a` to page `b`.
fn connector(a: PageId, b: PageId) -> Op {
    let at = |page| Anchor {
        page: PageRef::Id(page),
        rect: rect(40.0),
    };
    Op::AddConnector {
        a: at(a),
        b: at(b),
        label: "1".into(),
        style: style(),
        locals: Vec::new(),
    }
}

/// A conflict over `key`, by the version labelled `by`.
fn conflict(key: Key, by: &str) -> EditError {
    EditError::Conflict {
        key,
        by: Some(by.into()),
    }
}

/// Runs `commit` on `d`, which must fail and leave the current version
/// and its state as they were: why it failed.
fn refused(
    d: &mut Document,
    commit: impl FnOnce(&mut Document) -> Result<Commit, EditError>,
) -> EditError {
    let (at, st, n) = (d.current_version(), d.state().clone(), d.version_count());
    let e = commit(d).unwrap_err();
    assert_eq!((d.current_version(), d.version_count()), (at, n), "{e}");
    assert_eq!(d.state(), &st, "{e}");
    e
}

#[test]
fn a_relowered_insert_before_a_page_deleted_since_is_not_found() {
    let mut d = doc_of(4, &[]);
    let other = crate::store::blank(d.store(), 2);
    let [_, _, p2, _] = pages(&d);
    let (v, mut tx) = planned(&d);
    tx.apply(fx::insert(&other, [0, 1], "b.pdf", Some(p2)), &NoProgress)
        .unwrap();
    d.commit_ops(vec![fx::delete(&[p2])], "Delete page 3")
        .unwrap();
    let e = refused(&mut d, |d| d.commit(tx, on(v, Stale::Relower)));
    assert!(matches!(e, EditError::NotFound { .. }), "{e}");
}

#[test]
fn a_relowered_insert_lands_beside_a_move_with_the_ids_it_drew() {
    let mut d = doc_of(5, &[]);
    let other = crate::store::blank(d.store(), 2);
    let [p0, p1, p2, p3, p4] = pages(&d);
    let (v, mut tx) = planned(&d);
    tx.apply(fx::insert(&other, [0, 1], "b.pdf", Some(p3)), &NoProgress)
        .unwrap();
    let created = tx.preview().created;
    let new: Vec<PageId> = created.pages().collect();
    assert_eq!(new.len(), 2);
    d.commit_ops(vec![fx::move_pages(&[p0], None)], "Move page 1")
        .unwrap();
    let c = d.commit(tx, on(v, Stale::Relower)).unwrap();
    assert_eq!(c.created, created, "the ids the first lowering drew");
    let order: Vec<PageId> = d.state().order().iter().copied().collect();
    assert_eq!(order, [p1, p2, new[0], new[1], p3, p4, p0]);
}

/// An insert of a file with a form, our marks, other tools' comments and
/// an outline, then a header, lowered again beside a turn of another page:
/// every id they draw, the fields' and widgets' too, is the one they drew
/// first, so what they make is what they made.
#[test]
fn a_relowered_insert_keeps_every_id_it_drew() {
    let mut d = doc_of(3, &[]);
    let mut f = crate::forms::fixture::every_kind();
    crate::model::history_tests::with_ours(&mut f);
    crate::model::history_tests::with_theirs(&mut f, 0, "theirs");
    let at = f.pages.clone();
    let item = |title: &str, page: usize| {
        dictionary! {
            "Title" => Object::string_literal(title),
            "Dest" => vec![at[page].into(), "Fit".into()],
        }
    };
    let outline = vec![(0, item("One", 0)), (1, item("Two", 1))];
    crate::forms::fixture::outline_tree(&mut f, outline);
    let kind = crate::store::BlobKind::file(std::path::Path::new("form.pdf"), None);
    let other = d.store().open(f.bytes().into(), kind).unwrap();
    let [p0, _, p2] = pages(&d);
    let (v, mut tx) = planned(&d);
    let insert = Op::InsertPages {
        from: fx::file(&other, "form.pdf"),
        pages: None,
        before: Some(PageRef::Id(p2)),
        bring: Bring {
            marks: true,
            outline: OutlineBring::MergeByTitle,
            ..Bring::default()
        },
        locals: Vec::new(),
    };
    fx::hold_incoming(&mut tx, std::slice::from_ref(&insert));
    tx.apply(insert, &NoProgress).unwrap();
    tx.apply(fx::set_header(fx::header("Draft"), None), &NoProgress)
        .unwrap();
    let mut made = tx.state().clone();
    assert!(made.field_count() > 0 && made.bookmark_count() == 2);
    assert_eq!(made.generators().len(), 1);
    d.commit_ops(vec![fx::rotate(&[p0], Turn::Quarter)], "Rotate page 1")
        .unwrap();
    made.set(&Key::Geom(p0), d.state().get(&Key::Geom(p0)))
        .unwrap();
    d.commit(tx, on(v, Stale::Relower)).unwrap();
    assert_eq!(d.state(), &made);
}

/// A crop is its pages' boxes alone: made on an older version, it lands
/// beside another page's delete, and conflicts with a crop of its own page
/// made since, naming it.
#[test]
fn a_crop_lands_beside_another_page_s_delete_and_conflicts_with_its_own_page_s_crop() {
    let mut d = doc_of(4, &[]);
    let [p0, p1, p2, p3] = pages(&d);
    let (v, mut tx) = planned(&d);
    tx.apply(fx::crop(&[p1], all(36.0)), &NoProgress).unwrap();
    d.commit_ops(vec![fx::delete(&[p3])], "Delete page 4")
        .unwrap();
    d.commit(tx, on(v, Stale::Validate)).unwrap();
    let order: Vec<PageId> = d.state().order().iter().copied().collect();
    assert_eq!(order, [p0, p1, p2]);
    let cropped = crate::geom::Rect::new(36.0, 36.0, 576.0, 756.0);
    assert_eq!(d.geom(p1).crop, cropped);

    let (v, mut tx) = planned(&d);
    tx.apply(fx::crop(&[p0], all(72.0)), &NoProgress).unwrap();
    d.commit_ops(vec![fx::crop(&[p0], all(18.0))], "Crop page 1")
        .unwrap();
    let e = refused(&mut d, |d| d.commit(tx, on(v, Stale::Validate)));
    assert_eq!(e, conflict(Key::Geom(p0), "Crop page 1"));
    assert_eq!(e.to_string(), "“Crop page 1” changed the same data");
}

#[test]
fn a_merged_step_is_said_against_the_document_it_lands_on() {
    let mut d = doc_of(3, &[]);
    let [p0, _, p2] = pages(&d);
    let (v, mut tx) = planned(&d);
    tx.apply(fx::crop(&[p2], all(36.0)), &NoProgress).unwrap();
    assert_eq!(tx.preview().lines[0].text, "Crop page 3");
    d.commit_ops(vec![fx::move_pages(&[p2], Some(p0))], "Move page 3")
        .unwrap();
    let c = d.commit(tx, on(v, Stale::Validate)).unwrap();
    assert_eq!(c.lines[0].text, "Crop page 1");
    assert_eq!(d.timeline().pop().unwrap().edits, c.lines);
}

#[test]
fn a_stale_delete_is_relowered_with_the_cascades_of_what_came_since() {
    let mut d = doc_of(4, &[]);
    let [p0, p1, p2, p3] = pages(&d);
    let (v, mut tx) = planned(&d);
    // Blank review's: the page the reader confirmed, as its thumbnail
    // showed it.
    tx.read(Key::Page(p1));
    tx.apply(fx::delete(&[p1]), &NoProgress).unwrap();
    let header = fx::set_header(fx::header("Draft"), Some(&[p1, p2]));
    d.commit_ops(vec![header], "Header").unwrap();
    d.commit_ops(vec![fx::bookmark(None, None, "Two", Some(p1))], "Bookmark")
        .unwrap();
    d.commit_ops(vec![connector(p1, p2)], "Connect").unwrap();
    let c = d.commit(tx, on(v, Stale::Relower)).unwrap();
    let st = d.state();
    let order: Vec<PageId> = st.order().iter().copied().collect();
    assert_eq!(order, [p0, p2, p3]);
    let on = crate::model::Scope::Pages(im::OrdSet::unit(p2));
    assert_eq!(st.header_footer().unwrap().scope, on);
    assert_eq!(st.bookmark_count(), 0);
    let ends: Vec<&crate::annot::Annot> = st.marks_on(p2).collect();
    assert_eq!(ends.len(), 1);
    assert_eq!(ends[0].partner(), None);
    let rules: Vec<Rule> = c.cascades.iter().map(|c| c.rule).collect();
    let want = [
        Rule::PageAnnots,
        Rule::AnnotPartner,
        Rule::PageBookmarks,
        Rule::PageGenerators,
    ];
    assert_eq!(rules, want);
}

#[test]
fn a_validated_delete_of_a_page_given_a_connector_since_conflicts() {
    let mut d = doc_of(3, &[]);
    let [_, p1, p2] = pages(&d);
    let (v, mut tx) = planned(&d);
    tx.apply(fx::delete(&[p1]), &NoProgress).unwrap();
    d.commit_ops(vec![connector(p1, p2)], "Connect").unwrap();
    // Its own slots are as they were: the cascades on the page's new
    // annotations would add to it.
    let e = refused(&mut d, |d| d.commit(tx, on(v, Stale::Validate)));
    assert_eq!(e, conflict(Key::PageAnnots(p1), "Connect"));
}

#[test]
fn what_names_a_page_deleted_since_conflicts() {
    let note = |page| Op::AddMark {
        page: PageRef::Id(page),
        mark: note("a note"),
        reply_to: None,
        local: None,
        imported: None,
    };
    let labels = |page| {
        fx::labels(&[LabelSpec {
            start: page,
            style: crate::labels::Style::Decimal,
            prefix: "A-".into(),
            first: 1,
        }])
    };
    let bookmark = |page| fx::bookmark(None, None, "Two", Some(page));
    let header = |page| fx::set_header(fx::header("Draft"), Some(&[page]));
    let watermark =
        |page| fx::set_watermark(fx::watermark("Draft"), sheaf_edit::Z::Under, Some(&[page]));
    let ops: [&dyn Fn(PageId) -> Op; 5] = [&note, &labels, &bookmark, &header, &watermark];
    for op in ops {
        let mut d = doc_of(3, &[]);
        let [_, p1, _] = pages(&d);
        let (v, mut tx) = planned(&d);
        tx.apply(op(p1), &NoProgress).unwrap();
        // The page had no annotations, label range, bookmark, header or
        // watermark, so its delete wrote none of the slots the op does.
        d.commit_ops(vec![fx::delete(&[p1])], "Delete page 2")
            .unwrap();
        let e = refused(&mut d, |d| d.commit(tx, on(v, Stale::Validate)));
        assert_eq!(e, conflict(Key::Page(p1), "Delete page 2"));
    }
}

/// Two pages each replaced by the same page of another file, one by the
/// transaction and one by an edit since: each took that page's first copy,
/// so merged, they'd be one copy of it twice.
#[test]
fn two_pages_shown_as_one_copy_of_a_page_conflict() {
    let mut d = doc_of(3, &[]);
    let other = crate::store::blank(d.store(), 2);
    let [p0, p1, _] = pages(&d);
    let (v, mut tx) = planned(&d);
    tx.apply(fx::replace(&[p0], &other, [0], "b.pdf"), &NoProgress)
        .unwrap();
    let replace = fx::replace(&[p1], &other, [0], "b.pdf");
    d.commit_ops(vec![replace], "Replace page 2").unwrap();
    let e = refused(&mut d, |d| d.commit(tx, on(v, Stale::Validate)));
    assert_eq!(e, conflict(Key::Page(p1), "Replace page 2"));
}

/// No op an edit since could make gives a page's annotation a name another
/// has there, or a node a full name another has, without writing what a
/// transaction adding one writes too (the page's annotations, the form): so
/// the rules are judged on states made for them, as a merge would make
/// them.
#[test]
fn a_name_given_twice_by_a_merge_breaks_uniqueness_unless_it_was_so() {
    let mut d = doc_of(2, &[]);
    let [p0, _] = pages(&d);
    let imported = |text| Op::AddMark {
        page: PageRef::Id(p0),
        mark: note(text),
        reply_to: None,
        local: None,
        imported: Some(sheaf_edit::Imported {
            nm: "theirs".into(),
            created: "D:20260101000000Z".into(),
            modified: "D:20260101000000Z".into(),
            author: "Them".into(),
        }),
    };
    let base = d.state().clone();
    let a = d.commit_ops(vec![imported("a")], "a").unwrap();
    let a = a.created.annots().next().unwrap();
    let one = d.state().clone();
    let b = d.commit_ops(vec![imported("b")], "b").unwrap();
    let b = b.created.annots().next().unwrap();
    let two = d.state().clone();
    // `b`, made on `base`, put on `one`.
    let mut made = crate::prim::Rec::new(base);
    for key in [Key::Annot(b), Key::PageAnnots(p0)] {
        let val = match key {
            Key::PageAnnots(_) => Some(Val::PageAnnots(im::vector![b])),
            _ => two.get(&key),
        };
        made.put(key, val);
    }
    let written = [Key::Annot(b), Key::PageAnnots(p0)];
    assert_eq!(
        two.unique_after(&written, [&one, &made]),
        Err(Key::Annot(a))
    );
    assert_eq!(two.unique_after(&written, [&two, &made]), Ok(()));

    // A root node named as one there is, put beside it.
    let node = |partial: &str| crate::model::FieldNode {
        parent: None,
        partial: partial.into(),
        src: None,
        inst: crate::model::FormInst {
            src: crate::store::blob_id(b"form"),
            copy: 0,
        },
        kids: im::Vector::new(),
        term: None,
    };
    let (f, g) = (crate::forms::FieldId(1), crate::forms::FieldId(2));
    let field = |n: crate::model::FieldNode| Some(Val::Field(Arc::new(n)));
    let mut st = crate::prim::Rec::new(one.clone());
    st.put(Key::Field(f), field(node("total")));
    let one = DocState::clone(&st);
    st.put(Key::Field(g), field(node("total")));
    let both = DocState::clone(&st);
    let written = [Key::Field(g)];
    assert_eq!(
        both.unique_after(&written, [&one, &one]),
        Err(Key::Field(f))
    );
    assert_eq!(both.unique_after(&written, [&both, &one]), Ok(()));
}

#[test]
fn a_second_header_footer_from_one_base_conflicts() {
    let mut d = doc_of(3, &[]);
    let (v, mut a) = planned(&d);
    a.apply(fx::set_header(fx::header("A"), None), &NoProgress)
        .unwrap();
    let (_, mut b) = planned(&d);
    b.apply(fx::set_header(fx::header("B"), None), &NoProgress)
        .unwrap();
    let opts = CommitOpts {
        label: Some("Header A".into()),
        ..on(v, Stale::Validate)
    };
    d.commit(a, opts).unwrap();
    let g = *d.state().generators().keys().next().unwrap();
    let e = refused(&mut d, |d| d.commit(b, on(v, Stale::Validate)));
    assert_eq!(e, conflict(Key::Generator(g), "Header A"));
}

/// Two watermarks of one z, set by dialogs opened on one version, can't
/// both be: the second conflicts. A background beside the watermark can.
#[test]
fn a_second_watermark_of_one_z_from_one_base_conflicts() {
    let mut d = doc_of(3, &[]);
    let set = |text: &str, z| fx::set_watermark(fx::watermark(text), z, None);
    let (v, mut a) = planned(&d);
    a.apply(set("A", sheaf_edit::Z::Over), &NoProgress).unwrap();
    let (_, mut b) = planned(&d);
    b.apply(set("B", sheaf_edit::Z::Over), &NoProgress).unwrap();
    let (_, mut under) = planned(&d);
    under
        .apply(set("C", sheaf_edit::Z::Under), &NoProgress)
        .unwrap();
    let opts = CommitOpts {
        label: Some("Watermark A".into()),
        ..on(v, Stale::Validate)
    };
    d.commit(a, opts).unwrap();
    let g = d.state().watermark(sheaf_edit::Z::Over).unwrap().id;
    let e = refused(&mut d, |d| d.commit(b, on(v, Stale::Validate)));
    assert_eq!(e, conflict(Key::Generator(g), "Watermark A"));
    d.commit(under, on(v, Stale::Validate)).unwrap();
    assert_eq!(d.state().generators().len(), 2);
}

#[test]
fn a_changed_decision_read_conflicts_either_way() {
    let mut d = doc_of(3, &[]);
    let [p0, p1, _] = pages(&d);
    for (stale, text) in [(Stale::Validate, "a"), (Stale::Relower, "b")] {
        let (v, mut tx) = planned(&d);
        tx.read(Key::Page(p0));
        tx.apply(fx::rotate(&[p1], Turn::Quarter), &NoProgress)
            .unwrap();
        d.commit_ops(fx::ocr([(p0, layer(text))]), "Recognize text")
            .unwrap();
        let e = refused(&mut d, |d| d.commit(tx, on(v, stale)));
        assert_eq!(e, conflict(Key::Page(p0), "Recognize text"), "{stale:?}");
    }
}

#[test]
fn a_conflict_by_an_undo_names_no_edit() {
    let mut d = doc_of(2, &[]);
    let [p0, _] = pages(&d);
    d.commit_ops(fx::ocr([(p0, layer("a"))]), "Recognize text")
        .unwrap();
    d.commit_ops(fx::ocr([(p0, layer("b"))]), "Recognize text again")
        .unwrap();
    let (v, mut tx) = planned(&d);
    tx.read(Key::Page(p0));
    d.undo().unwrap();
    // The first recognition changed the page too, but before the version
    // the transaction was made on.
    let e = refused(&mut d, |d| d.commit(tx, on(v, Stale::Relower)));
    let by = None;
    assert_eq!(
        e,
        EditError::Conflict {
            key: Key::Page(p0),
            by
        }
    );
    assert_eq!(e.to_string(), "another edit changed the same data");
}

#[test]
fn a_change_put_back_since_is_no_conflict() {
    for stale in [Stale::Validate, Stale::Relower] {
        let mut d = doc_of(2, &[]);
        let [p0, _] = pages(&d);
        let (v, mut tx) = planned(&d);
        tx.read(Key::Geom(p0));
        tx.apply(fx::rotate(&[p0], Turn::Quarter), &NoProgress)
            .unwrap();
        // Turned all the way round: the same boxes, by value.
        for _ in 0..2 {
            d.commit_ops(vec![fx::rotate(&[p0], Turn::Half)], "Rotate")
                .unwrap();
        }
        d.commit(tx, on(v, stale)).unwrap();
        assert_eq!(d.geom(p0).rotate, Rotation::R90, "{stale:?}");
    }
}

/// A dialog's transaction made on version V, an OCR flush that amends V
/// (a new version in its place), then the dialog's commit: it lands by
/// value, keeping the amended layers.
#[test]
fn a_dialog_on_v_keeps_an_ocr_amend_of_v() {
    let mut d = doc_of(3, &[]);
    let copy = crate::store::blank(d.store(), 2);
    let [p0, p1, p2] = pages(&d);
    let g = d.begin_group(crate::model::GroupKind::Ocr);
    d.commit_ops_in_group(g, fx::ocr([(p0, layer("a"))]), "Recognize text")
        .unwrap();
    let (v, mut tx) = planned(&d);
    tx.apply(fx::replace(&[p1], &copy, [0], "b.pdf"), &NoProgress)
        .unwrap();
    d.commit_ops_in_group(g, fx::ocr([(p2, layer("b"))]), "Recognize text")
        .unwrap();
    assert!(
        d.edge(v).is_none(),
        "the amend put a new version in its place"
    );
    d.commit(tx, on(v, Stale::Validate)).unwrap();
    let st = d.state();
    assert!(st.ocr_text(p0).is_some() && st.ocr_text(p2).is_some());
    assert_eq!(st.def(p1).unwrap().base.src, copy);
}

#[test]
fn a_base_pruned_since_still_lands() {
    for stale in [Stale::Validate, Stale::Relower] {
        let mut d = doc_of(3, &[]);
        let [p0, p1, p2] = pages(&d);
        d.commit_ops(vec![fx::rotate(&[p0], Turn::Quarter)], "Rotate")
            .unwrap();
        let (v, mut tx) = planned(&d);
        tx.apply(fx::rotate(&[p1], Turn::Quarter), &NoProgress)
            .unwrap();
        // Undone, then left behind on its branch, which pruning takes
        // first.
        d.undo().unwrap();
        for _ in 0..MAX_VERSIONS {
            d.commit_ops(vec![fx::rotate(&[p2], Turn::Quarter)], "Rotate")
                .unwrap();
        }
        assert!(d.edge(v).is_none(), "pruned");
        d.commit(tx, on(v, stale)).unwrap();
        // Its own change, not its version's.
        assert_eq!(d.geom(p1).rotate, Rotation::R90, "{stale:?}");
        assert_eq!(d.geom(p0).rotate, Rotation::R0, "{stale:?}");
    }
}

#[test]
fn a_relowered_position_conflicts_once_the_order_moved() {
    let mut d = doc_of(3, &[]);
    let [p0, _, p2] = pages(&d);
    let second = || Op::RotatePages {
        pages: PageSel::Positions(fx::positions([1])),
        by: Turn::Quarter,
    };
    let (v, mut tx) = planned(&d);
    tx.apply(second(), &NoProgress).unwrap();
    d.commit_ops(vec![fx::rotate(&[p2], Turn::Half)], "Rotate page 3")
        .unwrap();
    d.commit(tx, on(v, Stale::Relower)).unwrap();
    let (v, mut tx) = planned(&d);
    tx.apply(second(), &NoProgress).unwrap();
    d.commit_ops(vec![fx::move_pages(&[p0], None)], "Move page 1")
        .unwrap();
    let e = refused(&mut d, |d| d.commit(tx, on(v, Stale::Relower)));
    assert_eq!(e, conflict(Key::Order, "Move page 1"));
}

#[test]
fn a_preview_says_what_its_commit_does() {
    let mut d = doc_of(3, &[]);
    let [p0, p1, _] = pages(&d);
    let mut tx = d.tx(Actor::User);
    tx.apply(fx::rotate(&[p0], Turn::Quarter), &NoProgress)
        .unwrap();
    tx.apply(fx::duplicate(&[p1]), &NoProgress).unwrap();
    let preview = tx.preview();
    let c = d.commit(tx, CommitOpts::default()).unwrap();
    assert_eq!(preview.lines, c.lines);
    assert_eq!(preview.effects, c.effects);
    assert_eq!(preview.created, c.created);
    assert_eq!(c.created.pages().count(), 1);
}

#[test]
fn a_transaction_on_another_version_said_to_be_current_is_a_bug() {
    let mut d = doc_of(2, &[]);
    let [p0, _] = pages(&d);
    let (_, mut tx) = planned(&d);
    tx.apply(fx::rotate(&[p0], Turn::Quarter), &NoProgress)
        .unwrap();
    d.commit_ops(vec![fx::rotate(&[p0], Turn::Half)], "Rotate")
        .unwrap();
    // A debug build stops there.
    let at = d.current_version();
    let commit = std::panic::AssertUnwindSafe(|| d.commit(tx, CommitOpts::default()));
    let refused = std::panic::catch_unwind(commit);
    assert_eq!(
        refused.is_err(),
        cfg!(debug_assertions),
        "panics in debug only"
    );
    if let Ok(r) = refused {
        assert!(matches!(r, Err(EditError::Internal { .. })), "{r:?}");
    }
    assert_eq!(d.current_version(), at);
}

/// A transaction made on another document, one a reopen replaced, whose
/// version ids are counted afresh: the version it was made on is this
/// one's current one by number, but not this document's.
#[test]
fn a_transaction_made_on_another_document_is_a_bug() {
    let gone = doc_of(2, &[]);
    let mut d = doc_of(2, &[]);
    let [p0, _] = pages(&d);
    let (v, mut tx) = planned(&gone);
    assert_eq!(v, d.current_version(), "the same number in both");
    tx.apply(fx::rotate(&[p0], Turn::Quarter), &NoProgress)
        .unwrap();
    let n = d.version_count();
    // A debug build stops there.
    let commit = std::panic::AssertUnwindSafe(|| d.commit(tx, on(v, Stale::Validate)));
    let refused = std::panic::catch_unwind(commit);
    assert_eq!(
        refused.is_err(),
        cfg!(debug_assertions),
        "panics in debug only"
    );
    if let Ok(r) = refused {
        assert!(matches!(r, Err(EditError::Internal { .. })), "{r:?}");
    }
    assert_eq!((d.current_version(), d.version_count()), (v, n));
}
