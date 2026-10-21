//! The cascades, one test a rule (M10.5 §6.5): each deletes what triggers
//! its rule and finds the effect and the rule's report, and lowered again
//! with that rule switched off finds it missing. Then validation, which
//! fails and never repairs (§6.4), and what a duplicate copies (bug 12).

use in_progress::NoProgress;
use lopdf::{Object, dictionary};
use sheaf_edit::{
    AnnotRef, BookmarkRef, Imported, LabelStyle, Local, MarkKind, Op, PageLabelRange, PageRef,
    PageSel, ScopeSel, Sibling, Turn,
};

use super::fixture as fx;
use super::tests::{commit, doc_of, flat, new_mark, note, opened, rect, style, titles};
use super::*;
use crate::model::Document;
use crate::prim::Rec;

/// `op` lowered by the user into `d`'s state with only the cascades
/// `rules`: the state it makes, and what they did.
fn lowered_with(d: &Document, op: &Op, rules: &[Rule]) -> (DocState, Vec<Cascade>) {
    let tx = d.tx(Actor::User);
    let (mut resolved, _) = tx.resolve(op).unwrap();
    let state = Rec::new(tx.state().clone());
    let mut lw = Lower::new(state, &tx.cx, &NoProgress, resolved.name());
    lower_op(&mut lw, &mut resolved).unwrap();
    let cascades = cascade::settle_with(&mut lw, rules).unwrap();
    (lw.rec.finish().0, cascades)
}

/// `op` lowered by the user into `d`'s state with every rule but `off`.
fn without(d: &Document, op: &Op, off: Rule) -> DocState {
    let rules: Vec<Rule> = Rule::ALL.into_iter().filter(|&r| r != off).collect();
    let (st, cascades) = lowered_with(d, op, &rules);
    assert!(cascades.iter().all(|c| c.rule != off));
    st
}

fn cascade(rule: Rule, objects: usize) -> Cascade {
    Cascade { rule, objects }
}

fn delete(pages: &[u32]) -> Op {
    Op::DeletePages {
        pages: PageSel::Positions(
            (pages.iter().map(u32::to_string))
                .collect::<Vec<_>>()
                .join(",")
                .parse()
                .unwrap(),
        ),
    }
}

fn add_mark(page: u32, mark: sheaf_edit::NewMark, reply_to: Option<Local>, local: u16) -> Op {
    Op::AddMark {
        page: PageRef::Pos(page),
        mark,
        reply_to: reply_to.map(AnnotRef::New),
        local: Some(Local(local)),
        imported: None,
    }
}

/// The annotation local `l` names.
fn annot(c: &Commit, l: u16) -> crate::annot::AnnotId {
    match c.created.locals[&Local(l)] {
        Made::Annot(id) => id,
        other => panic!("${l} is {other:?}"),
    }
}

/// `page.annots`: a deleted page's annotations go with it, which without
/// the rule are left on no page.
#[test]
fn a_deleted_page_takes_its_annotations() {
    let mut d = doc_of(3, &[]);
    let adds = vec![
        add_mark(2, note("a"), None, 1),
        add_mark(2, note("b"), None, 2),
        add_mark(3, note("c"), None, 3),
    ];
    commit(&mut d, Actor::User, adds).unwrap();
    let op = delete(&[2]);
    let left = without(&d, &op, Rule::PageAnnots);
    assert_eq!(left.annot_count(), 3);
    assert!(left.check_invariants().is_err());
    let c = commit(&mut d, Actor::User, vec![op]).unwrap();
    assert_eq!(c.cascades, [cascade(Rule::PageAnnots, 2)]);
    assert_eq!(d.state().annot_count(), 1);
}

/// `annot.thread`: a deleted comment's replies go, and the replies to
/// them; but a reply names its comment by `/NM` on its page, so while
/// another comment there has the name, its replies stay.
#[test]
fn a_deleted_comment_takes_its_thread() {
    let mut d = doc_of(1, &[]);
    let status = MarkKind::Status {
        model: crate::annot::REVIEW.into(),
        state: "Accepted".into(),
    };
    let ops = vec![
        add_mark(1, note("Why?"), None, 1),
        add_mark(
            1,
            new_mark(MarkKind::Reply {}, "Because."),
            Some(Local(1)),
            2,
        ),
        add_mark(1, new_mark(status, ""), Some(Local(2)), 3),
        add_mark(1, note("Aside"), None, 4),
    ];
    let c = commit(&mut d, Actor::User, ops).unwrap();
    let op = Op::DeleteAnnotations {
        annots: vec![AnnotRef::Id(annot(&c, 1))],
    };
    assert_eq!(without(&d, &op, Rule::AnnotThread).annot_count(), 3);
    let c2 = commit(&mut d, Actor::User, vec![op]).unwrap();
    assert_eq!(c2.cascades, [cascade(Rule::AnnotThread, 2)]);
    let left: Vec<_> = d.state().annots().map(|(id, _)| id).collect();
    assert_eq!(left, [annot(&c, 4)]);
    // Two comments named alike, as files have them, and a reply.
    let imported = |l| Op::AddMark {
        page: PageRef::Pos(1),
        mark: note("Twice"),
        reply_to: None,
        local: Some(Local(l)),
        imported: Some(Imported {
            nm: "twice".into(),
            created: "D:20260101000000Z".into(),
            modified: "D:20260101000000Z".into(),
            author: "Them".into(),
        }),
    };
    let ops = vec![
        imported(1),
        imported(2),
        add_mark(1, new_mark(MarkKind::Reply {}, "Once"), Some(Local(1)), 3),
    ];
    let c = commit(&mut d, Actor::User, ops).unwrap();
    let op = Op::DeleteAnnotations {
        annots: vec![AnnotRef::Id(annot(&c, 1))],
    };
    let c2 = commit(&mut d, Actor::User, vec![op]).unwrap();
    assert_eq!(c2.cascades, []);
    let reply = d.state().annot(annot(&c, 3)).expect("the reply stays");
    assert_eq!(reply.parent(), Some("twice"));
}

/// `annot.partner`: a connector deleted leaves the other end without a
/// partner, which without the rule names one that's gone.
#[test]
fn a_deleted_connector_leaves_its_partner_unpaired() {
    let mut d = doc_of(2, &[]);
    let connect = Op::AddConnector {
        a: sheaf_edit::Anchor {
            page: PageRef::Pos(1),
            rect: rect(10.0),
        },
        b: sheaf_edit::Anchor {
            page: PageRef::Pos(2),
            rect: rect(40.0),
        },
        label: "1".into(),
        style: style(),
        locals: vec![Local(1), Local(2)],
    };
    let c = commit(&mut d, Actor::User, vec![connect]).unwrap();
    let (a, b) = (annot(&c, 1), annot(&c, 2));
    let op = Op::DeleteAnnotations {
        annots: vec![AnnotRef::Id(a)],
    };
    let left = without(&d, &op, Rule::AnnotPartner);
    assert_eq!(left.annot(b).unwrap().mark().unwrap().partner(), Some(a));
    assert!(left.check_invariants().is_err());
    let c = commit(&mut d, Actor::User, vec![op]).unwrap();
    assert_eq!(c.cascades, [cascade(Rule::AnnotPartner, 1)]);
    assert_eq!(d.state().annot(b).unwrap().mark().unwrap().partner(), None);
    d.undo().unwrap();
    assert_eq!(
        d.state().annot(b).unwrap().mark().unwrap().partner(),
        Some(a)
    );
}

/// `widget.field`: deleting a page takes the fields whose widgets were
/// all on it, which a save takes out of the file (proposal 25), and then
/// the parent left with none, from the roots too; without the rule they
/// stay with no widgets.
#[test]
fn a_field_goes_with_its_last_widget() {
    let bytes = crate::forms::fixture::every_kind_in(3).bytes();
    let mut d = opened(bytes, "form.pdf");
    assert_eq!(
        (d.state().field_count(), d.state().terminals().count()),
        (10, 9)
    );
    let op = delete(&[2]);
    assert_eq!(without(&d, &op, Rule::WidgetField).field_count(), 10);
    let c = commit(&mut d, Actor::User, vec![op]).unwrap();
    assert!(
        c.cascades.contains(&cascade(Rule::WidgetField, 4)),
        "{:?}",
        c.cascades
    );
    let st = d.state();
    assert_eq!((st.field_count(), st.terminals().count()), (6, 5));
    assert_eq!(st.field_named("form1[0].sig[0]"), None);
    assert!(st.field_named("form1[0].name[0]").is_some());
    // The first page's fields go too, and the parent they leave.
    let c = commit(&mut d, Actor::User, vec![delete(&[1])]).unwrap();
    assert!(
        c.cascades.contains(&cascade(Rule::WidgetField, 6)),
        "{:?}",
        c.cascades
    );
    let st = d.state();
    assert_eq!(st.field_count(), 0);
    assert!(st.form().roots.is_empty() && st.form().calc_order.is_empty());
    d.undo().unwrap();
    assert_eq!(d.state().field_count(), 6);
}

/// `page.bookmarks`: a deleted page's bookmark goes, its kids taking its
/// place, which without the rule goes nowhere.
#[test]
fn a_deleted_page_takes_its_bookmarks() {
    let mut d = doc_of(3, &flat(&[("A", 0, 0), ("A1", 1, 1), ("B", 2, 0)]));
    let op = delete(&[1]);
    assert!(
        without(&d, &op, Rule::PageBookmarks)
            .check_invariants()
            .is_err()
    );
    let c = commit(&mut d, Actor::User, vec![op]).unwrap();
    assert_eq!(c.cascades, [cascade(Rule::PageBookmarks, 1)]);
    assert_eq!(titles(&d), [(0, "A1".into()), (0, "B".into())]);
}

/// `page.labels`: a range whose first page is deleted starts at the next
/// page of its range, numbered as it was, and one with no page left goes
/// (proposal 25); without the rule they'd start at pages that are gone.
#[test]
fn a_label_range_moves_to_the_next_page_of_its_range() {
    let mut d = doc_of(6, &[]);
    let range = |start, prefix: &str| PageLabelRange {
        start: PageRef::Pos(start),
        style: LabelStyle::Decimal,
        prefix: prefix.into(),
        first: 1,
    };
    let labels = Op::SetPageLabels {
        ranges: Some(vec![range(2, "A-"), range(4, "B-"), range(6, "C-")]),
    };
    commit(&mut d, Actor::User, vec![labels]).unwrap();
    let shown = |d: &Document| d.state().page_labels().unwrap();
    assert_eq!(shown(&d), ["1", "A-1", "A-2", "B-1", "B-2", "C-1"]);
    let op = delete(&[2, 4, 5]);
    assert!(
        without(&d, &op, Rule::PageLabels)
            .check_invariants()
            .is_err()
    );
    let c = commit(&mut d, Actor::User, vec![op]).unwrap();
    assert_eq!(c.cascades, [cascade(Rule::PageLabels, 2)]);
    assert_eq!(shown(&d), ["1", "A-1", "C-1"]);
    d.undo().unwrap();
    assert_eq!(shown(&d).len(), 6);
}

/// `page.generators`: a deleted page leaves the header/footer's pages,
/// and the header/footer goes once it's on none; without the rule it'd be
/// on a page that's gone.
#[test]
fn a_deleted_page_leaves_the_header_s_pages() {
    let mut d = doc_of(4, &[]);
    let set = Op::SetHeaderFooter {
        spec: fx::header("Draft"),
        scope: ScopeSel::Pages(PageSel::Positions("2-3".parse().unwrap())),
    };
    commit(&mut d, Actor::User, vec![set]).unwrap();
    let third = d.page(2).unwrap();
    let op = delete(&[2]);
    assert!(
        without(&d, &op, Rule::PageGenerators)
            .check_invariants()
            .is_err()
    );
    let c = commit(&mut d, Actor::User, vec![op]).unwrap();
    assert_eq!(c.cascades, [cascade(Rule::PageGenerators, 1)]);
    let hf = d.state().header_footer().unwrap();
    assert_eq!(hf.pages.as_deref(), Some(&[third][..]));
    let c = commit(&mut d, Actor::User, vec![delete(&[2])]).unwrap();
    assert_eq!(c.cascades, [cascade(Rule::PageGenerators, 1)]);
    assert!(d.state().generators().is_empty());
}

/// An id that isn't the document's is an error in the ops that once
/// passed over one (bug 11), and the transaction is as it was.
#[test]
fn an_unknown_page_is_not_found() {
    let mut d = doc_of(3, &[]);
    let pages = || PageSel::Ids(vec![PageRef::Pos(1), PageRef::Id(PageId(99))]);
    let ops = [
        Op::MovePages {
            pages: pages(),
            before: None,
        },
        Op::RotatePages {
            pages: pages(),
            by: Turn::Quarter,
        },
        Op::DeletePages { pages: pages() },
        Op::DuplicatePages {
            pages: pages(),
            locals: Vec::new(),
        },
        Op::ClearOcrText { pages: pages() },
    ];
    let mut tx = d.tx(Actor::User);
    tx.apply(delete(&[3]), &NoProgress).unwrap();
    let order = tx.state().order().clone();
    for op in ops {
        let name = op_name(&op);
        let e = tx.apply(op, &NoProgress).unwrap_err();
        assert_eq!(
            e,
            EditError::NotFound {
                op: name,
                what: "page 99".into()
            }
        );
        assert_eq!(tx.state().order(), &order, "{name}");
        assert_eq!((tx.steps.len(), tx.created.ops.len()), (1, 1), "{name}");
    }
    let c = d.commit(tx, CommitOpts::default()).unwrap();
    assert_eq!(c.lines.len(), 1);
    assert_eq!(d.page_count(), 2);
}

/// Every reference must resolve (§6.4): a bookmark's page, a label
/// range's, the header's, the comment a reply is to and a local. A name
/// that fits two is ambiguous.
#[test]
fn every_reference_resolves_or_is_an_error() {
    let mut d = doc_of(2, &flat(&[("A", 0, 0), ("A", 1, 0)]));
    let gone = || PageRef::Id(PageId(99));
    let tx = d.tx(Actor::User);
    let not_found = [
        Op::AddBookmark {
            parent: None,
            at: Sibling::Last,
            title: "B".into(),
            target: Some(gone()),
            view: None,
            color: None,
            style: Default::default(),
            local: None,
        },
        Op::SetPageLabels {
            ranges: Some(vec![PageLabelRange {
                start: gone(),
                style: LabelStyle::Decimal,
                prefix: String::new(),
                first: 1,
            }]),
        },
        Op::SetHeaderFooter {
            spec: fx::header("Draft"),
            scope: ScopeSel::Pages(PageSel::Ids(vec![gone()])),
        },
        Op::AddMark {
            page: PageRef::Pos(1),
            mark: new_mark(MarkKind::Reply {}, "Yes"),
            reply_to: Some(AnnotRef::Id(crate::annot::AnnotId(7))),
            local: None,
            imported: None,
        },
        Op::RotatePages {
            pages: PageSel::Ids(vec![PageRef::New(Local(3))]),
            by: Turn::Half,
        },
    ];
    for op in not_found {
        let e = tx.check(&op).unwrap_err();
        assert!(matches!(e, EditError::NotFound { .. }), "{e}");
    }
    let path = Op::DeleteBookmarks {
        bookmarks: vec![BookmarkRef::Path(vec!["A".into()])],
    };
    let e = tx.check(&path).unwrap_err();
    assert!(matches!(e, EditError::Ambiguous { .. }), "{e}");
    let imported = Imported {
        nm: "same".into(),
        created: "D:20260101000000Z".into(),
        modified: "D:20260101000000Z".into(),
        author: "Them".into(),
    };
    let named = |page| Op::AddMark {
        page: PageRef::Pos(page),
        mark: note("n"),
        reply_to: None,
        local: None,
        imported: Some(imported.clone()),
    };
    commit(&mut d, Actor::User, vec![named(1), named(2)]).unwrap();
    let nm = Op::DeleteAnnotations {
        annots: vec![AnnotRef::Nm {
            nm: "same".into(),
            page: None,
        }],
    };
    let e = d.tx(Actor::User).check(&nm).unwrap_err();
    assert!(matches!(e, EditError::Ambiguous { .. }), "{e}");
}

/// What can't be done as said is invalid, found before anything is
/// lowered: pages moved before one of themselves, a bookmark under
/// itself, a widget deleted on its own, an imported name that isn't the
/// mark's, a comment in a thread moved off its page, and a connector
/// made another kind of mark.
#[test]
fn what_cant_be_done_is_invalid() {
    let bytes = crate::forms::fixture::every_kind().bytes();
    let mut d = opened(bytes, "form.pdf");
    let ops = vec![
        Op::AddBookmark {
            parent: None,
            at: Sibling::Last,
            title: "A".into(),
            target: None,
            view: None,
            color: None,
            style: Default::default(),
            local: Some(Local(1)),
        },
        Op::AddBookmark {
            parent: Some(BookmarkRef::New(Local(1))),
            at: Sibling::Last,
            title: "A1".into(),
            target: None,
            view: None,
            color: None,
            style: Default::default(),
            local: Some(Local(2)),
        },
        add_mark(1, note("Why?"), None, 3),
        add_mark(
            1,
            new_mark(MarkKind::Reply {}, "Because."),
            Some(Local(3)),
            4,
        ),
        Op::AddConnector {
            a: sheaf_edit::Anchor {
                page: PageRef::Pos(1),
                rect: rect(10.0),
            },
            b: sheaf_edit::Anchor {
                page: PageRef::Pos(2),
                rect: rect(10.0),
            },
            label: "1".into(),
            style: style(),
            locals: vec![Local(5), Local(6)],
        },
    ];
    let c = commit(&mut d, Actor::User, ops).unwrap();
    let bookmark = |l| match c.created.locals[&Local(l)] {
        Made::Bookmark(id) => BookmarkRef::Id(id),
        other => panic!("{other:?}"),
    };
    let id = |l| AnnotRef::Id(annot(&c, l));
    let widget = d.state().widgets_on(d.page(0).unwrap()).next().unwrap().0;
    let imported = Imported {
        nm: "theirs".into(),
        created: String::new(),
        modified: String::new(),
        author: String::new(),
    };
    let moved = |l| Op::MoveMarks {
        marks: vec![id(l)],
        by: sheaf_edit::Delta::Page(sheaf_edit::Vec2 { x: 0.0, y: 0.0 }),
        to_page: Some(PageRef::Pos(2)),
    };
    let tx = d.tx(Actor::User);
    for op in [
        Op::MovePages {
            pages: PageSel::Positions("1-2".parse().unwrap()),
            before: Some(PageRef::Pos(2)),
        },
        Op::MoveBookmarks {
            bookmarks: vec![bookmark(1)],
            parent: Some(bookmark(2)),
            at: Sibling::Last,
            with_pages: false,
        },
        Op::DeleteAnnotations {
            annots: vec![AnnotRef::Id(widget)],
        },
        Op::PatchMarks {
            marks: vec![id(3)],
            patch: Default::default(),
            imported: Some(imported),
        },
        moved(3),
        moved(4),
    ] {
        let e = tx.check(&op).unwrap_err();
        assert!(matches!(e, EditError::Invalid { .. }), "{e}");
    }
    let unthreaded = add_mark(1, note("Alone"), None, 7);
    let c2 = commit(&mut d, Actor::User, vec![unthreaded]).unwrap();
    let off = Op::MoveMarks {
        marks: vec![AnnotRef::Id(annot(&c2, 7))],
        by: sheaf_edit::Delta::Page(sheaf_edit::Vec2 { x: 1.0, y: 0.0 }),
        to_page: Some(PageRef::Pos(2)),
    };
    commit(&mut d, Actor::User, vec![off]).unwrap();
    let square = Op::PatchMarks {
        marks: vec![id(5)],
        patch: sheaf_edit::MarkPatch {
            kind: Some(MarkKind::Square { rect: rect(10.0) }),
            ..Default::default()
        },
        imported: None,
    };
    let e = commit(&mut d, Actor::User, vec![square]).unwrap_err();
    assert!(matches!(e, EditError::Invalid { .. }), "{e}");
}

/// A file of three pages: on the first, a note of sheaf's ([`note`]) and
/// another tool's reply to it; the others empty.
///
/// [`note`]: crate::forms::fixture::note
fn replied_to() -> Vec<u8> {
    let mut f = crate::forms::fixture::Form::new(3);
    let written = crate::annot::to_pdf(&crate::forms::fixture::note(), &mut f.pdf).main;
    let note = f.pdf.add_object(written);
    let reply = f.pdf.add_object(dictionary! {
        "Type" => "Annot", "Subtype" => "Text", "IRT" => note, "NM" => Object::string_literal("theirs"),
        "Rect" => vec![0.into(), 0.into(), 20.into(), 20.into()],
        "Contents" => Object::string_literal("Agreed"),
    });
    f.set_annots(0, &[note, reply]);
    f.bytes()
}

/// Duplicated pages' marks are marks of their own (bug 12): a reply copied
/// with the comment it's to, sheaf's or another tool's, is to the
/// comment's copy, by its name; a connector copied with its partner is
/// paired with the partner's copy, and one whose partner wasn't copied
/// has none; and each copy is in the header/footer's pages when its page
/// is.
#[test]
fn a_duplicate_s_threads_and_pairs_are_its_own() {
    let mut d = opened(replied_to(), "a.pdf");
    let [first, second, third] = [0, 1, 2].map(|i| d.page(i).unwrap());
    let theirs = d.state().foreign_on(first).next().unwrap().0;
    let original = d
        .state()
        .annot_by_nm(first, "sheaf:4")
        .expect("the file's note");
    let connect = |a, b, locals: [u16; 2]| Op::AddConnector {
        a: sheaf_edit::Anchor {
            page: PageRef::Pos(a),
            rect: rect(10.0),
        },
        b: sheaf_edit::Anchor {
            page: PageRef::Pos(b),
            rect: rect(40.0),
        },
        label: "1".into(),
        style: style(),
        locals: locals.map(Local).to_vec(),
    };
    let ops = vec![
        Op::AddMark {
            page: PageRef::Pos(1),
            mark: new_mark(MarkKind::Reply {}, "Mine"),
            reply_to: Some(AnnotRef::Id(original)),
            local: Some(Local(1)),
            imported: None,
        },
        connect(1, 2, [2, 3]),
        connect(1, 3, [4, 5]),
        Op::SetHeaderFooter {
            spec: fx::header("Draft"),
            scope: ScopeSel::Pages(PageSel::Positions("1,3".parse().unwrap())),
        },
    ];
    let c = commit(&mut d, Actor::User, ops).unwrap();
    let copy = Op::DuplicatePages {
        pages: PageSel::Positions("1-2".parse().unwrap()),
        locals: vec![Local(6), Local(7)],
    };
    let c2 = commit(&mut d, Actor::User, vec![copy]).unwrap();
    let copied = |l| match c2.created.locals[&Local(l)] {
        Made::Page(id) => id,
        other => panic!("{other:?}"),
    };
    let (first2, second2) = (copied(6), copied(7));
    let st = d.state();
    assert_eq!(
        st.order().iter().copied().collect::<Vec<_>>(),
        [first, first2, second, second2, third]
    );
    // The copies on the first page's copy, by what they're copies of.
    let on = |page| -> Vec<(crate::annot::AnnotId, crate::model::AnnotRec)> {
        st.annots_on(page).map(|(id, a)| (id, a.clone())).collect()
    };
    let [note2, theirs2, mine2, end2, lone2] = on(first2).try_into().unwrap();
    let note_name = note2.1.name();
    assert_ne!(note_name, "sheaf:4");
    assert_eq!(theirs2.1.foreign().unwrap().copy, 1);
    assert_eq!(theirs2.1.parent(), Some(note_name.as_str()));
    assert_eq!(mine2.1.parent(), Some(note_name.as_str()));
    assert_ne!(mine2.0, annot(&c, 1));
    assert_eq!(st.annot(theirs).unwrap().parent(), Some("sheaf:4"));
    // The pair copied with both ends is a pair of its own; the end whose
    // partner stayed behind has none.
    let [(other2, _)] = on(second2).try_into().unwrap();
    let partner = |id| st.annot(id).unwrap().mark().unwrap().partner();
    assert_eq!(
        (partner(end2.0), partner(other2)),
        (Some(other2), Some(end2.0))
    );
    assert_eq!(partner(lone2.0), None);
    assert_eq!(partner(annot(&c, 4)), Some(annot(&c, 5)));
    // The first page is the header's, and so is its copy; the second isn't.
    let hf = st.header_footer().unwrap();
    assert_eq!(hf.pages.as_deref(), Some(&[first, first2, third][..]));
    assert_eq!(st.check_invariants(), Ok(()));
    // The other tool's reply on the copy, made a mark, is in the copy's
    // thread: its own, not the one the file says.
    let adopt = Op::AdoptAnnotations {
        annots: vec![AnnotRef::Id(theirs2.0)],
    };
    commit(&mut d, Actor::User, vec![adopt]).unwrap();
    let adopted = d.state().annot(theirs2.0).unwrap().mark().unwrap();
    assert_eq!(adopted.thread_parent(), Some(note_name.as_str()));
}
