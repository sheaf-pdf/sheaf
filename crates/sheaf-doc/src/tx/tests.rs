//! Transactions: references resolved against the state an op finds,
//! locals, capabilities per actor and target, what the host fills in, and
//! what a commit says.

use std::sync::Arc;

use in_progress::NoProgress;
use sheaf_edit::{
    Anchor, AnnotRef, BlobSel, BookmarkRef, Imported, Local, MarkKind, NewMark, Op, PageRef,
    PageSel, PageSource, Rotation, Sibling, Style, Turn,
};

use super::fixture as fx;
use super::*;
use crate::annot::{AnnotKind, Box2, Pt};
use crate::history::FixedCount;
use crate::model::Document;
use crate::open::FlatOutlineItem;
use crate::store::{BlobKind, Store};

/// A document of `n` blank pages, in a store of its own, with `outline`.
pub(crate) fn doc_of(n: u32, outline: &[FlatOutlineItem]) -> Document {
    let store = Store::new();
    let primary = crate::store::blank(&store, n);
    let policy = Box::new(FixedCount(10));
    Document::new(store, primary, "a.pdf".into(), None, outline, policy)
}

/// `bytes` opened as a file named `name` is.
pub(crate) fn opened(bytes: Vec<u8>, name: &str) -> Document {
    let store = Store::new();
    let kind = BlobKind::file(std::path::Path::new(name), None);
    let primary = store.open(bytes.into(), kind).unwrap();
    let policy = Box::new(FixedCount(10));
    let mut d = crate::open::start(store, primary.clone(), name.into(), None, policy);
    d.ready(crate::open::finish(&primary, &NoProgress)).unwrap();
    d
}

/// Bookmarks titled, going to the page at each 0-based index, at each
/// depth.
pub(super) fn flat(items: &[(&str, u32, u16)]) -> Vec<FlatOutlineItem> {
    (items.iter())
        .map(|&(title, page, depth)| FlatOutlineItem {
            title: title.into(),
            page: Some(page),
            depth,
            view: None,
            color: None,
            style: Default::default(),
            open: true,
            object: None,
            kept: false,
        })
        .collect()
}

/// `ops`, applied by `actor` in one transaction and committed.
pub(super) fn commit(d: &mut Document, actor: Actor, ops: Vec<Op>) -> Result<Commit, EditError> {
    let mut tx = d.tx(actor);
    tx.apply_batch(ops, &NoProgress)?;
    d.commit(tx, CommitOpts::default())
}

pub(super) fn style() -> Style {
    Style {
        color: [255, 0, 0],
        opacity: 1.0,
        stroke: 1.0,
        fill: None,
        dashed: false,
        cloudy: false,
    }
}

pub(super) fn rect(x: f32) -> Box2 {
    Box2 {
        x,
        y: 100.0,
        w: 20.0,
        h: 20.0,
    }
}

pub(super) fn new_mark(kind: MarkKind, contents: &str) -> NewMark {
    NewMark {
        kind,
        style: style(),
        contents: contents.into(),
        author: None,
        turn: Rotation::R0,
        asset: None,
    }
}

pub(super) fn note(contents: &str) -> NewMark {
    new_mark(
        MarkKind::Note {
            pos: Pt { x: 50.0, y: 50.0 },
        },
        contents,
    )
}

/// Recognized text of one word.
fn layer() -> Arc<crate::text_layer::TextLayer> {
    Arc::new(crate::text_layer::TextLayer {
        words: vec![crate::text_layer::OcrWord {
            text: "word".into(),
            rect: rect(40.0),
            confidence: 0.9,
        }],
        lines: std::iter::once(0..1).collect(),
        upright: Rotation::R0,
        engine: "test".into(),
    })
}

fn plugin() -> Actor {
    Actor::Plugin {
        id: "tax".into(),
        version: "1.0".into(),
    }
}

fn imported(nm: &str) -> Imported {
    Imported {
        nm: nm.into(),
        created: "D:20260101000000Z".into(),
        modified: "D:20260101000000Z".into(),
        author: "Them".into(),
    }
}

/// The titles in pre-order, with depths.
pub(super) fn titles(d: &Document) -> Vec<(u16, String)> {
    (d.state().bookmarks_preorder().iter())
        .map(|(depth, _, b)| (*depth, b.title.to_string()))
        .collect()
}

/// The pages' indexes in their sources, in order.
fn order(d: &Document) -> Vec<u32> {
    (d.state().pages())
        .map(|(_, def, _)| def.base.index)
        .collect()
}

/// A bookmark made `$1` with three children under it, two connectors made
/// together (`$2`, `$3`), and a reply to a note made in the same batch: the
/// later ops name what the earlier made, and the commit says what each
/// local is.
#[test]
fn a_batch_names_what_it_makes_with_locals() {
    let mut d = doc_of(4, &[]);
    let child = |i: u32| Op::AddBookmark {
        parent: Some(BookmarkRef::New(Local(1))),
        at: Sibling::Last,
        title: format!("Chapter {i}"),
        target: Some(PageRef::Pos(i + 1)),
        view: None,
        color: None,
        style: Default::default(),
        local: None,
    };
    let ops = vec![
        Op::AddBookmark {
            parent: None,
            at: Sibling::Last,
            title: "Part".into(),
            target: Some(PageRef::Pos(1)),
            view: None,
            color: None,
            style: Default::default(),
            local: Some(Local(1)),
        },
        child(1),
        child(2),
        child(3),
        Op::AddConnector {
            a: Anchor {
                page: PageRef::Pos(1),
                rect: rect(10.0),
            },
            b: Anchor {
                page: PageRef::Pos(3),
                rect: rect(40.0),
            },
            label: "1".into(),
            style: style(),
            locals: vec![Local(2), Local(3)],
        },
        Op::AddMark {
            page: PageRef::Pos(2),
            mark: note("Why?"),
            reply_to: None,
            local: Some(Local(4)),
            imported: None,
        },
        Op::AddMark {
            page: PageRef::Pos(2),
            mark: new_mark(MarkKind::Reply {}, "Because."),
            reply_to: Some(AnnotRef::New(Local(4))),
            local: Some(Local(5)),
            imported: None,
        },
    ];
    let c = commit(&mut d, Actor::User, ops).unwrap();
    assert!(c.version.is_some());
    assert_eq!(c.created.ops.len(), 7);
    assert_eq!(c.created.locals.len(), 5);
    let local = |n: u16| c.created.locals[&Local(n)];
    let Made::Bookmark(part) = local(1) else {
        panic!("$1 is a bookmark");
    };
    let st = d.state();
    let kids: Vec<String> = (st.bookmark(part).unwrap().kids.iter())
        .map(|&k| st.bookmark(k).unwrap().title.to_string())
        .collect();
    assert_eq!(kids, ["Chapter 1", "Chapter 2", "Chapter 3"]);
    let (Made::Annot(a), Made::Annot(b)) = (local(2), local(3)) else {
        panic!("connectors");
    };
    let partner = |id| st.annot(id).unwrap().mark().unwrap().partner();
    assert_eq!((partner(a), partner(b)), (Some(b), Some(a)));
    let (Made::Annot(question), Made::Annot(answer)) = (local(4), local(5)) else {
        panic!("marks");
    };
    let asked = st.annot(question).unwrap().name();
    let reply = st.annot(answer).unwrap().mark().unwrap();
    assert_eq!(reply.thread_parent(), Some(asked.as_str()));
}

/// A local nothing named is nothing; one named twice, or one naming
/// something else, is invalid; and the batch applies nothing.
#[test]
fn locals_must_be_named_once_and_of_their_kind() {
    let mut d = doc_of(2, &[]);
    let bookmark = |local, target| Op::AddBookmark {
        parent: None,
        at: Sibling::First,
        title: "B".into(),
        target,
        view: None,
        color: None,
        style: Default::default(),
        local,
    };
    let before = d.current_version();
    let unnamed = vec![bookmark(None, Some(PageRef::New(Local(7))))];
    let e = commit(&mut d, Actor::User, unnamed).unwrap_err();
    assert!(matches!(e, EditError::NotFound { .. }), "{e}");
    for ops in [
        vec![
            bookmark(Some(Local(1)), None),
            bookmark(Some(Local(1)), None),
        ],
        vec![
            bookmark(Some(Local(1)), None),
            bookmark(None, Some(PageRef::New(Local(1)))),
        ],
    ] {
        let e = commit(&mut d, Actor::User, ops).unwrap_err();
        assert!(matches!(e, EditError::Invalid { .. }), "{e}");
    }
    assert_eq!(d.current_version(), before);
}

/// Positions are 1-based and resolve against the state each op finds:
/// after the first page is deleted, position 2 is what was the third. The
/// step keeps the page by id.
#[test]
fn positions_resolve_against_the_state_each_op_finds() {
    let mut d = doc_of(4, &[]);
    let third = d.page(2).unwrap();
    let ops: Vec<Op> = serde_json::from_str(
        r#"[{"op": "delete-pages", "pages": {"positions": "1"}},
            {"op": "rotate-pages", "pages": {"ids": [{"pos": 2}]}, "by": 90}]"#,
    )
    .unwrap();
    let c = commit(&mut d, Actor::User, ops).unwrap();
    assert_eq!(d.page(1), Some(third));
    assert_eq!(d.geom(third).rotate, Rotation::R90);
    let steps = d.edge(c.version.unwrap()).unwrap();
    assert_eq!(
        *steps[1].op,
        ResolvedOp::RotatePages {
            pages: vec![third],
            by: Turn::Quarter
        }
    );
    assert_eq!(steps[1].line, "Rotate page 2 90°");
    // A position past the last page is nothing.
    let past = Op::DeletePages {
        pages: PageSel::Positions("9".parse().unwrap()),
    };
    let e = commit(&mut d, Actor::User, vec![past]).unwrap_err();
    assert!(matches!(e, EditError::NotFound { .. }), "{e}");
}

/// Moving a bookmark with its pages moves the pages it spans, from its
/// page up to the next bookmark's, after the span of the bookmark before
/// it now; undo puts them back.
#[test]
fn a_bookmark_moves_with_its_pages() {
    // A → 0 { A1 → 1, A2 → 2 }, B → 3, C → 5.
    let outline = flat(&[
        ("A", 0, 0),
        ("A1", 1, 1),
        ("A2", 2, 1),
        ("B", 3, 0),
        ("C", 5, 0),
    ]);
    let mut d = doc_of(6, &outline);
    let path = |title: &str| BookmarkRef::Path(vec![title.into()]);
    let move_with = |title: &str, at: Sibling| Op::MoveBookmarks {
        bookmarks: vec![path(title)],
        parent: None,
        at,
        with_pages: true,
    };
    // C (page 5) after A, whose span is pages 0–2: its page lands at 3.
    let after_a = move_with("C", Sibling::After(path("A")));
    commit(&mut d, Actor::User, vec![after_a]).unwrap();
    let top: Vec<String> = (titles(&d).into_iter())
        .filter(|(depth, _)| *depth == 0)
        .map(|(_, t)| t)
        .collect();
    assert_eq!(top, ["A", "C", "B"]);
    assert_eq!(order(&d), [0, 1, 2, 5, 3, 4]);
    // B (pages 3 and 4) first: its pages too.
    commit(&mut d, Actor::User, vec![move_with("B", Sibling::First)]).unwrap();
    assert_eq!(order(&d), [3, 4, 0, 1, 2, 5]);
    assert_eq!(titles(&d)[0].1, "B");
    d.undo().unwrap();
    assert_eq!(order(&d), [0, 1, 2, 5, 3, 4]);
}

/// One op of every kind, resolved, for the capability tables.
fn every_op(d: &Document) -> Vec<ResolvedOp> {
    use ResolvedOp as R;
    let page = d.page(0).unwrap();
    let pages = vec![page];
    let src = d.primary().clone();
    let id = crate::annot::AnnotId(1);
    let spec = fx::header("");
    vec![
        R::MovePages {
            pages: pages.clone(),
            before: None,
        },
        R::RotatePages {
            pages: pages.clone(),
            by: Turn::Half,
        },
        R::DeletePages {
            pages: pages.clone(),
        },
        R::DuplicatePages {
            pages: pages.clone(),
            locals: Vec::new(),
        },
        R::InsertPages {
            from: src.clone(),
            name: "a.pdf".into(),
            pages: vec![0],
            before: None,
            bring: Default::default(),
            locals: Vec::new(),
        },
        R::ReplacePages {
            pages: pages.clone(),
            from: src,
            name: "a.pdf".into(),
            with: vec![0],
            bring: Default::default(),
        },
        R::SetPageBoxes {
            pages: pages.clone(),
            crop: sheaf_edit::Patch::Clear,
            trim: sheaf_edit::Patch::Keep,
            bleed: sheaf_edit::Patch::Keep,
            art: sheaf_edit::Patch::Keep,
        },
        R::SetPageLabels { ranges: None },
        R::AddBookmark {
            parent: None,
            at: At::First,
            title: "B".into(),
            target: None,
            view: None,
            color: None,
            style: Default::default(),
            local: None,
        },
        R::PatchBookmarks {
            bookmarks: Vec::new(),
            patch: Default::default(),
        },
        R::MoveBookmarks {
            bookmarks: Vec::new(),
            parent: None,
            at: At::Last,
            with_pages: false,
        },
        R::DeleteBookmarks {
            bookmarks: Vec::new(),
        },
        R::AddMark {
            page,
            mark: note("n"),
            held: None,
            reply_to: None,
            local: None,
            imported: None,
        },
        R::AddConnector {
            a: (page, rect(0.0)),
            b: (page, rect(30.0)),
            label: "1".into(),
            style: style(),
            locals: Vec::new(),
        },
        R::PatchMarks {
            marks: vec![id],
            patch: Default::default(),
            imported: None,
        },
        R::MoveMarks {
            marks: vec![id],
            by: sheaf_edit::Delta::Page(sheaf_edit::Vec2 { x: 1.0, y: 0.0 }),
            to_page: None,
        },
        R::DeleteAnnotations { annots: vec![id] },
        R::AdoptAnnotations { annots: vec![id] },
        R::SetOcrText {
            page,
            layer: layer(),
        },
        R::ClearOcrText {
            pages: pages.clone(),
        },
        R::SetWatermark {
            content: crate::model::WatermarkContent::Text {
                text: "DRAFT".into(),
                font: Default::default(),
                size: 72.0,
                color: [0, 0, 0],
                opacity: 0.3,
                angle: 45.0,
            },
            position: Default::default(),
            z: sheaf_edit::Z::Over,
            scope: None,
        },
        R::RemoveWatermark {
            z: None,
            pages: pages.clone(),
            include_authored: true,
            others: Default::default(),
        },
        R::SetHeaderFooter { spec, scope: None },
        R::RemoveHeaderFooter {
            pages: pages.clone(),
            include_authored: true,
            others: Default::default(),
        },
        R::FlattenAnnotations {
            pages,
            which: sheaf_edit::FlattenSet::All,
            flattened: 0,
        },
        R::Revert {
            label: "Recognize text".into(),
        },
    ]
}

/// The capability table is total over every op and every kind of target
/// (§6.7): sheaf's own marks are a plugin's to delete, anything else's
/// needs approval, as a flatten does; content from files and imported
/// names need the user. The actor table is total over every actor and
/// capability.
#[test]
fn caps_cover_every_op_target_and_actor() {
    use ResolvedOp as R;
    let d = doc_of(1, &[]);
    let ops = every_op(&d);
    let names: std::collections::BTreeSet<&str> = ops.iter().map(ResolvedOp::name).collect();
    assert_eq!(names.len(), ops.len(), "one op of each kind");
    assert_eq!(names.len(), 26, "every kind there is");
    let actors = [
        Actor::User,
        Actor::Cli,
        Actor::Job(JobKind::Ocr),
        Actor::Job(JobKind::Trim),
        Actor::Job(JobKind::Import),
        Actor::Job(JobKind::Insert),
        Actor::Job(JobKind::Scan),
        plugin(),
    ];
    for op in &ops {
        for target in Target::ALL {
            let cap = cap_of(op, &[target]);
            let want = match (op, target) {
                (R::DeleteAnnotations { .. }, Target::Mark) => Cap::Plugin,
                (
                    R::DeleteAnnotations { .. }
                    | R::SetPageBoxes { .. }
                    | R::AdoptAnnotations { .. }
                    | R::ClearOcrText { .. }
                    | R::SetWatermark { .. }
                    | R::RemoveWatermark { .. }
                    | R::SetHeaderFooter { .. }
                    | R::RemoveHeaderFooter { .. }
                    | R::FlattenAnnotations { .. },
                    _,
                ) => Cap::Approval,
                (
                    R::InsertPages { .. }
                    | R::ReplacePages { .. }
                    | R::SetOcrText { .. }
                    | R::Revert { .. },
                    _,
                ) => Cap::User,
                _ => Cap::Plugin,
            };
            assert_eq!(cap, want, "{} on a {target:?}", op.name());
            for actor in &actors {
                let want = match actor {
                    Actor::Job(JobKind::Ocr) => {
                        matches!(op, R::SetOcrText { .. } | R::ClearOcrText { .. })
                    }
                    Actor::Job(JobKind::Trim) => matches!(op, R::SetPageBoxes { .. }),
                    Actor::Job(JobKind::Scan) => {
                        matches!(op, R::InsertPages { .. } | R::SetOcrText { .. })
                    }
                    Actor::Plugin { .. } => cap == Cap::Plugin,
                    _ => true,
                };
                let allowed = actor_allows(actor, cap, op);
                assert_eq!(allowed, want, "{actor:?} making {}", op.name());
            }
        }
    }
    // An imported comment's own name and dates are the user's to keep.
    let page = d.page(0).unwrap();
    for op in [
        R::AddMark {
            page,
            mark: note("n"),
            held: None,
            reply_to: None,
            local: None,
            imported: Some(imported("x")),
        },
        R::PatchMarks {
            marks: Vec::new(),
            patch: Default::default(),
            imported: Some(imported("x")),
        },
    ] {
        assert_eq!(cap_of(&op, &[]), Cap::User);
        assert!(!actor_allows(&plugin(), Cap::User, &op));
    }
}

/// Trim White's job sets page boxes, and is refused anything else before
/// it's lowered; a plugin needs the user's approval to set them.
#[test]
fn trim_white_sets_page_boxes_alone() {
    let mut d = doc_of(2, &[]);
    let p = d.page(0).unwrap();
    let trim = Actor::Job(JobKind::Trim);
    let margins = sheaf_edit::Margins {
        top: 36.0,
        bottom: 36.0,
        left: 36.0,
        right: 36.0,
    };
    let crop = fx::crop(&[p], margins);
    assert_eq!(
        d.tx(plugin()).check(&crop),
        Err(EditError::Refused(Cap::Approval))
    );
    let c = commit(&mut d, trim.clone(), vec![crop]).unwrap();
    assert_eq!(c.change.actor, trim);
    assert_eq!(c.lines[0].text, "Crop page 1");
    let ids = d.ids_next();
    let rotate = fx::rotate(&[p], Turn::Quarter);
    let refused = Err(EditError::Refused(Cap::Plugin));
    assert_eq!(d.tx(trim.clone()).check(&rotate), refused);
    assert_eq!(commit(&mut d, trim, vec![rotate]).map(|_| ()), refused);
    assert_eq!(d.ids_next(), ids, "nothing was lowered");
    assert_eq!(d.geom(p).rotate, Rotation::R0);
}

/// A plugin deletes the marks it made, never another tool's comment, and
/// never keeps an imported comment's name or names a raw blob: those are
/// refused before anything is lowered.
#[test]
fn a_plugin_is_refused_what_needs_more_than_its_cap() {
    let mut f = crate::forms::fixture::Form::new(1);
    crate::model::history_tests::with_theirs(&mut f, 0, "theirs");
    let mut d = opened(f.bytes(), "a.pdf");
    let page = PageRef::Pos(1);
    let theirs = AnnotRef::Nm {
        nm: "theirs".into(),
        page: Some(page),
    };
    let ids = d.ids_next();
    let refused = [
        (
            Op::DeleteAnnotations {
                annots: vec![theirs],
            },
            Cap::Approval,
        ),
        (
            Op::AddMark {
                page,
                mark: note("n"),
                reply_to: None,
                local: None,
                imported: Some(imported("theirs-2")),
            },
            Cap::User,
        ),
        (
            Op::InsertPages {
                from: PageSource::File {
                    file: BlobSel::Id(d.primary().id()),
                    name: "a.pdf".into(),
                },
                pages: None,
                before: None,
                bring: Default::default(),
                locals: Vec::new(),
            },
            Cap::User,
        ),
    ];
    for (op, cap) in refused {
        let tx = d.tx(plugin());
        assert_eq!(tx.check(&op), Err(EditError::Refused(cap)));
        let e = commit(&mut d, plugin(), vec![op]).unwrap_err();
        assert_eq!(e, EditError::Refused(cap));
    }
    assert_eq!(d.ids_next(), ids, "nothing was lowered");
    // Its own mark, it may delete.
    let mine = Op::AddMark {
        page,
        mark: note("mine"),
        reply_to: None,
        local: Some(Local(1)),
        imported: None,
    };
    let c = commit(&mut d, plugin(), vec![mine]).unwrap();
    let Made::Annot(mine) = c.created.locals[&Local(1)] else {
        panic!("a mark");
    };
    let delete = Op::DeleteAnnotations {
        annots: vec![AnnotRef::Id(mine)],
    };
    commit(&mut d, plugin(), vec![delete]).unwrap();
    assert!(d.state().annot(mine).is_none());
}

/// A plugin attaches a file by the grant the host gave it: naming a blob
/// in the store by its id is refused at resolve, though its cap allows
/// the op.
#[test]
fn a_plugin_names_a_blob_only_by_its_grant() {
    let mut d = doc_of(1, &[]);
    let file = d.primary().blob().clone();
    let attach = |asset| Op::AddMark {
        page: PageRef::Pos(1),
        mark: NewMark {
            asset: Some(asset),
            ..new_mark(
                MarkKind::FileAttachment {
                    pos: Pt { x: 50.0, y: 50.0 },
                    icon: "PushPin".into(),
                    name: "a.pdf".into(),
                },
                "",
            )
        },
        reply_to: None,
        local: None,
        imported: None,
    };
    let ids = d.ids_next();
    let by_id = attach(BlobSel::Id(file.id()));
    let refused = Err(EditError::Refused(Cap::User));
    assert_eq!(d.tx(plugin()).check(&by_id), refused);
    assert_eq!(commit(&mut d, plugin(), vec![by_id]).map(|_| ()), refused);
    assert_eq!(d.ids_next(), ids, "nothing was lowered");
    let mut tx = d.tx(plugin());
    let granted = tx.grant(Grant::Blob(file.into()));
    tx.apply(attach(granted), &NoProgress).unwrap();
    d.commit(tx, CommitOpts::default()).unwrap();
    assert_eq!(d.state().annot_count(), 1);
}

/// A check of deleting or duplicating all 7,000 pages resolves and allows
/// it, and lowers nothing: no id is drawn, no slot is put.
#[test]
fn a_check_lowers_nothing() {
    let d = doc_of(7_000, &[]);
    let tx = d.tx(Actor::User);
    let ids = d.ids_next();
    let all = Op::DeletePages {
        pages: PageSel::All,
    };
    tx.check(&all).unwrap();
    let copies = Op::DuplicatePages {
        pages: PageSel::All,
        locals: Vec::new(),
    };
    tx.check(&copies).unwrap();
    assert_eq!(d.ids_next(), ids);
    assert!(tx.state().order().ptr_eq(d.state().order()));
    assert_eq!(tx.state().page_count(), 7_000);
    assert_eq!(tx.state().annot_count(), d.state().annot_count());
}

/// A batch of 7,000 ops of one kind is one line, a crop of pages names
/// them, and a removal of the header and footer says what it did (M10.5
/// bug 8). 700 in a debug build, which checks every invariant after each
/// op.
#[test]
fn a_run_of_one_kind_is_one_line() {
    let n = if cfg!(debug_assertions) { 700 } else { 7_000 };
    let mut d = doc_of(n, &[]);
    let layer = layer();
    let ops = (1..=n)
        .map(|pos| Op::SetOcrText {
            page: PageRef::Pos(pos),
            layer: layer.clone(),
        })
        .collect();
    let c = commit(&mut d, Actor::User, ops).unwrap();
    let lines: Vec<&str> = c.lines.iter().map(|l| l.text.as_str()).collect();
    assert_eq!(lines, [format!("Recognize text on {n} pages")]);
    let view = d.timeline().pop().unwrap();
    assert_eq!(view.edits, c.lines);
    let first = [0, 1, 2].map(|i| d.page(i).unwrap());
    let m = sheaf_edit::Margins {
        top: 36.0,
        bottom: 36.0,
        left: 36.0,
        right: 36.0,
    };
    let c = commit(&mut d, Actor::User, vec![fx::crop(&first, m)]).unwrap();
    assert_eq!(c.lines[0].text, "Crop pages 1–3");
    let c = commit(&mut d, Actor::User, vec![fx::remove_header(&first)]).unwrap();
    assert_eq!(
        c.lines[0].text,
        "Remove the header and footer from pages 1–3"
    );
}

/// A run's line sums what its steps name, or is the last's when each
/// replaces the one before, and removes (the History panel's badge) when
/// any of its steps does.
#[test]
fn a_line_says_whether_it_removes() {
    let line = |text: &str, removes| crate::describe::Line {
        text: text.into(),
        removes,
    };
    let mut d = doc_of(3, &[]);
    let add = |local| Op::AddMark {
        page: PageRef::Pos(1),
        mark: note("n"),
        reply_to: None,
        local: Some(Local(local)),
        imported: None,
    };
    let c = commit(&mut d, Actor::User, vec![add(1), add(2)]).unwrap();
    assert_eq!(c.lines, [line("Add 2 marks", false)]);
    let gone = |l| Op::DeleteAnnotations {
        annots: vec![AnnotRef::Id(match c.created.locals[&Local(l)] {
            Made::Annot(id) => id,
            other => panic!("{other:?}"),
        })],
    };
    let c = commit(&mut d, Actor::User, vec![gone(1), gone(2)]).unwrap();
    assert_eq!(c.lines, [line("Delete 2 comments", true)]);
    let labels = || Op::SetPageLabels { ranges: None };
    let c = commit(&mut d, Actor::User, vec![labels(), labels()]).unwrap();
    assert_eq!(c.lines, [line("Change page labels", false)]);
    let rotate = Op::RotatePages {
        pages: PageSel::Positions("1".parse().unwrap()),
        by: Turn::Quarter,
    };
    let delete = Op::DeletePages {
        pages: PageSel::Positions("3".parse().unwrap()),
    };
    let c = commit(&mut d, Actor::User, vec![rotate, delete]).unwrap();
    let rotated = line("Rotate page 1 90°", false);
    assert_eq!(c.lines, [rotated, line("Delete page 3", true)]);
}

/// The host fills in who made a mark (§6.8). A plugin's marks are its own,
/// and the user's name and initials stay as written in them: it never
/// learns who the user is. The date and time are filled in for anyone.
#[test]
fn a_plugin_never_learns_who_the_user_is() {
    let mut d = doc_of(1, &[]);
    d.set_identity(Identity {
        person: "Pat Smith".into(),
        initials: "PS".into(),
        date_format: "%Y".into(),
        time_format: "%H".into(),
    });
    let text = "<<name>> <<initials>> <<date>>";
    let stamp = MarkKind::TextStamp {
        rect: rect(10.0),
        text: "OK <<initials>> <<date>>".into(),
        status: false,
    };
    let made = |d: &mut Document, actor: Actor, mark: NewMark| {
        let add = Op::AddMark {
            page: PageRef::Pos(1),
            mark: NewMark {
                author: Some("Someone".into()),
                ..mark
            },
            reply_to: None,
            local: Some(Local(1)),
            imported: None,
        };
        let c = commit(d, actor, vec![add]).unwrap();
        let Made::Annot(id) = c.created.locals[&Local(1)] else {
            panic!("a mark");
        };
        d.state().annot(id).unwrap().mark().unwrap().clone()
    };
    let year = d.lower_cx(Actor::User).host().date.to_string();
    let theirs = made(&mut d, plugin(), note(text));
    assert_eq!(theirs.author, "tax");
    assert_eq!(theirs.contents, format!("<<name>> <<initials>> {year}"));
    let theirs = made(&mut d, plugin(), new_mark(stamp, ""));
    let AnnotKind::TextStamp { text: drawn, .. } = &theirs.kind else {
        panic!("a text stamp");
    };
    assert_eq!(*drawn, format!("OK <<initials>> {year}"));
    let mine = made(&mut d, Actor::User, note(text));
    assert_eq!(mine.author, "Someone");
    assert_eq!(mine.contents, format!("Pat Smith PS {year}"));
    assert_eq!(mine.created, mine.modified, "made now");
}

/// A plugin learns who the user is only when the user granted it
/// `identity`, which only a plugin's published transaction carries
/// (proposal 20): one bound by `lower_cx`, or published without the
/// grant, keeps `<<name>>` and `<<initials>>` as written. Its marks are by
/// its id either way.
#[test]
fn only_a_plugin_granted_identity_learns_who_the_user_is() {
    let mut d = doc_of(1, &[]);
    d.set_identity(Identity {
        person: "Pat Smith".into(),
        initials: "PS".into(),
        date_format: "%Y".into(),
        time_format: "%H".into(),
    });
    let note_by = |d: &mut Document, mut tx: Tx| {
        let add = Op::AddMark {
            page: PageRef::Pos(1),
            mark: note("<<name>> <<initials>>"),
            reply_to: None,
            local: None,
            imported: None,
        };
        tx.apply(add, &NoProgress).unwrap();
        let c = d.commit(tx, CommitOpts::default()).unwrap();
        let id = c.created.annots().next().unwrap();
        let mark = d.state().annot(id).unwrap().mark().unwrap().clone();
        (mark.author, mark.contents)
    };
    assert!(!d.lower_cx(plugin()).identity, "never from lower_cx");
    let bound = d.tx(plugin());
    let literal = ("tax".to_string(), "<<name>> <<initials>>".to_string());
    assert_eq!(note_by(&mut d, bound), literal);
    let ungranted = d.published().plugin_tx("tax".into(), "1.0".into(), false);
    assert_eq!(note_by(&mut d, ungranted), literal);
    let granted = d.published().plugin_tx("tax".into(), "1.0".into(), true);
    let named = ("tax".to_string(), "Pat Smith PS".to_string());
    assert_eq!(note_by(&mut d, granted), named);
}

/// A plugin's published transaction is on the version and state a
/// transaction of its own made now would be, and lowers as a plugin.
#[test]
fn a_published_plugin_tx_is_on_the_current_version() {
    let mut d = doc_of(2, &[]);
    d.commit_ops(
        vec![fx::rotate(&[d.page(0).unwrap()], Turn::Quarter)],
        "Rotate",
    )
    .unwrap();
    let published = d.published();
    let tx = published.plugin_tx("tax".into(), "1.0".into(), false);
    let own = d.tx(plugin());
    assert_eq!((tx.instance, tx.base), (own.instance, own.base));
    assert_eq!(tx.base, d.current_version());
    assert_eq!(tx.state(), own.state());
    assert_eq!(tx.actor(), &plugin());
    assert_eq!(published.snap.version, d.current_version());
}

/// A preview of ops says what applying them would, and leaves the
/// transaction as it was: its state, its steps, its locals and what it
/// made. Applied, they say the same, but for the ids the preview drew.
#[test]
fn a_batch_preview_leaves_the_transaction_as_it_was() {
    let d = doc_of(2, &[]);
    let [p0, p1] = [0, 1].map(|i| d.page(i).unwrap());
    let mut tx = d.tx(plugin());
    tx.apply(fx::rotate(&[p0], Turn::Quarter), &NoProgress)
        .unwrap();
    let (state, before) = (tx.state().clone(), tx.preview());
    let add = |local| Op::AddMark {
        page: PageRef::Id(p1),
        mark: note("Tie out"),
        reply_to: None,
        local: Some(Local(local)),
        imported: None,
    };
    let delete_first = Op::DeletePages {
        pages: PageSel::Positions("1".parse().unwrap()),
    };
    let previewed = tx
        .preview_batch(
            vec![add(1), fx::duplicate(&[p1]), delete_first.clone()],
            &NoProgress,
        )
        .unwrap();
    let lines: Vec<&str> = previewed.lines.iter().map(|l| l.text.as_str()).collect();
    assert_eq!(
        lines,
        [
            "Add a sticky note on page 2",
            "Duplicate page 2",
            "Delete page 1"
        ]
    );
    assert_eq!(previewed.caps, [Cap::Plugin; 3]);
    assert_eq!(previewed.created.ops.len(), 3);
    assert_eq!(previewed.created.locals.len(), 1);
    assert_eq!(tx.state(), &state);
    let after = tx.preview();
    assert_eq!((after.lines, after.created), (before.lines, before.created));
    // A local the preview gave names nothing, and is free to give again.
    let applied = tx.apply_said(vec![add(1)], &NoProgress).unwrap();
    assert_eq!(
        applied.created.locals.keys().collect::<Vec<_>>(),
        [&Local(1)]
    );
    assert_eq!(applied.caps, [Cap::Plugin]);
    let previewed =
        sheaf_edit::Previewed::from(tx.preview_batch(vec![delete_first], &NoProgress).unwrap());
    assert!(previewed.removes && previewed.created == [Vec::<Made>::new()]);
    // What a submit is answered, and a failure.
    let answer = sheaf_edit::Applied::from(applied);
    assert_eq!(answer.lines, ["Add a sticky note on page 2"]);
    assert_eq!(answer.created.len(), 1);
    let e = tx
        .preview_batch(vec![fx::delete(&[PageId(99)])], &NoProgress)
        .unwrap_err();
    let failure = sheaf_edit::Failure::from(&e);
    assert_eq!(failure.kind, sheaf_edit::FailureKind::NotFound);
    assert_eq!(failure.message, e.to_string());
}

/// A header's `<<file>>` is the name of the file that holds the document
/// when it's set: after a Save As, the new one, not the one it opened as.
#[test]
fn a_header_names_the_file_the_document_is_in() {
    let mut d = doc_of(1, &[]);
    assert_eq!(&*d.lower_cx(Actor::User).host().file, "a.pdf");
    let stamp = crate::serialize::durable::FileStamp {
        len: 0,
        mtime: std::time::SystemTime::UNIX_EPOCH,
        tail: 0,
        window: 0,
    };
    let path = std::path::Path::new("saved").join("b.pdf");
    let v = d.current_version();
    let saved = crate::materialize::SaveRecord::opened(v, d.primary(), path.into(), stamp);
    d.record_save(saved).unwrap();
    let set = Op::SetHeaderFooter {
        spec: fx::header("<<file>>"),
        scope: sheaf_edit::ScopeSel::All,
    };
    commit(&mut d, Actor::User, vec![set]).unwrap();
    assert_eq!(&*d.state().header_footer().unwrap().frozen.file, "b.pdf");
}

/// The same bytes inserted under another name are one blob, and their
/// fields come in under the name the op gives them, never the one the
/// bytes were first read by.
#[test]
fn an_insert_names_its_form_by_the_op() {
    let bytes = crate::forms::fixture::every_kind().bytes();
    let mut d = opened(bytes.clone(), "form.pdf");
    let kind = BlobKind::file(std::path::Path::new("copy.pdf"), None);
    let copy = d.store().open(bytes.into(), kind).unwrap();
    assert_eq!(copy.id(), d.primary().id(), "one blob");
    let insert = Op::InsertPages {
        from: PageSource::File {
            file: BlobSel::Id(copy.id()),
            name: "copy.pdf".into(),
        },
        pages: Some("1-2".parse().unwrap()),
        before: None,
        bring: Default::default(),
        locals: Vec::new(),
    };
    let c = commit(&mut d, Actor::User, vec![insert]).unwrap();
    let st = d.state();
    let parent = st.field_named("copy").expect("a parent named by the op");
    assert!(st.field(parent).unwrap().src.is_none(), "sheaf made it");
    let renamed = Notice::FormRenamed {
        under: "copy".into(),
        file: "copy.pdf".into(),
        scripted: false,
    };
    assert_eq!(c.notices, [renamed]);
}

/// What resolving and lowering read is noted by the value it had, and so
/// is what the caller read deciding its ops; a slot the transaction
/// changed first holds its own value, and isn't.
#[test]
fn reads_are_noted_by_value() {
    let d = doc_of(3, &[]);
    let first = d.page(0).unwrap();
    let mut tx = d.tx(Actor::User);
    let rotate = Op::RotatePages {
        pages: PageSel::Positions("2".parse().unwrap()),
        by: Turn::Half,
    };
    tx.apply(rotate, &NoProgress).unwrap();
    let order = tx.seen.get(&crate::prim::Key::Order).cloned().flatten();
    assert!(matches!(order, Some(crate::prim::Val::Order(o)) if o == *d.state().order()));
    let mut reader = tx.reader();
    assert_eq!(reader.page_at(0), Some(first));
    assert!(reader.geom(first).is_some());
    let second = d.page(1).unwrap();
    reader.geom(second);
    let decided: Vec<crate::prim::Key> = tx.decided.keys().copied().collect();
    use crate::prim::Key;
    assert_eq!(
        decided,
        [Key::Order, Key::Geom(first)],
        "not the turned page's"
    );
}

/// A blob given a name for the ops resolves by it, and one no grant has
/// is nothing.
#[test]
fn a_named_blob_resolves_by_its_name() {
    let mut d = doc_of(1, &[]);
    let other = crate::store::blank(d.store(), 2);
    let mut tx = d.tx(Actor::Cli);
    tx.pin(other.blob().clone());
    let file = tx.grant_named("more", Grant::Blob(other.blob().clone().into()));
    assert_eq!(file, BlobSel::With("more".into()));
    let insert = |file| Op::InsertPages {
        from: PageSource::File {
            file,
            name: "more.pdf".into(),
        },
        pages: None,
        before: None,
        bring: Default::default(),
        locals: Vec::new(),
    };
    let missing = tx.check(&insert(BlobSel::With("less".into())));
    assert!(
        matches!(missing, Err(EditError::NotFound { .. })),
        "{missing:?}"
    );
    let r = tx.apply(insert(file), &NoProgress).unwrap();
    assert_eq!(r.made.len(), 2);
    d.commit(tx, CommitOpts::default()).unwrap();
    assert_eq!(d.page_count(), 3);
}

/// Blank pages and an image's are made in memory as they're inserted: a
/// blob of their own kind, written by sheaf's serializer, the same bytes
/// for the same op, so lowered again it's the same blob. A check makes
/// none, reads no image, and names what's wrong as applying would.
#[test]
fn blank_and_image_pages_are_memory_sources() {
    use crate::store::{MemoryKind, Origin};
    use sheaf_edit::{Fit, PageSize};
    let mut d = doc_of(1, &[]);
    let png = crate::materialize::watermark_tests::png();
    let image = (d
        .store()
        .intern(png.as_slice().into(), BlobKind::image(&png).unwrap()))
    .unwrap();
    let letter = PageSize {
        width: 612.0,
        height: 792.0,
    };
    let blank_of = |count, pages: Option<&str>| Op::InsertPages {
        from: PageSource::Blank {
            size: letter,
            count,
        },
        pages: pages.map(|p| p.parse().unwrap()),
        before: None,
        bring: Default::default(),
        locals: Vec::new(),
    };
    let blank = |count| blank_of(count, None);
    let images = |images: Vec<BlobSel>, pages: Option<&str>| Op::InsertPages {
        from: PageSource::Images {
            images,
            fit: Fit::LETTER,
        },
        pages: pages.map(|p| p.parse().unwrap()),
        before: None,
        bring: Default::default(),
        locals: Vec::new(),
    };
    let by_id = || vec![BlobSel::Id(image.id())];
    let store = d.store().clone();
    // A check makes nothing, and refuses what applying would.
    let mut tx = d.tx(Actor::User);
    tx.check(&blank(2)).unwrap();
    tx.check(&images(by_id(), None)).unwrap();
    let invalid = |r: Result<(), EditError>| matches!(r, Err(EditError::Invalid { .. }));
    assert!(invalid(tx.check(&blank(0))));
    // More blank pages than a file opens with: refused by a check and by
    // applying, before any is made.
    let most = crate::source::Limits::DEFAULT.page_tree.pages as u32;
    let held = store.bytes();
    assert!(invalid(tx.check(&blank(most + 1))));
    let applied = tx.apply(blank(most + 1), &NoProgress);
    assert!(
        matches!(applied, Err(EditError::Invalid { .. })),
        "{applied:?}"
    );
    assert_eq!(store.bytes(), held, "nothing interned");
    assert!(invalid(tx.check(&images(Vec::new(), None))));
    let not_an_image = BlobSel::Id(d.primary().id());
    assert!(invalid(tx.check(&images(vec![not_an_image], None))));
    let past = tx.check(&blank_of(2, Some("3")));
    assert!(matches!(past, Err(EditError::NotFound { .. })), "{past:?}");
    // An image file is read only to make its pages: a check takes one
    // that can't be on trust, and applying it refuses it.
    let mut broken = png[..16].to_vec();
    broken.extend([0; 64]);
    let kind = BlobKind::image(&broken).unwrap();
    let broken = (d.store().intern(broken.into(), kind)).unwrap();
    let unread = images(vec![BlobSel::Id(broken.id())], None);
    tx.check(&unread).unwrap();
    assert!(matches!(
        tx.apply(unread, &NoProgress),
        Err(EditError::Invalid { .. })
    ));
    drop(tx);

    let c = commit(&mut d, Actor::User, vec![blank(2), images(by_id(), None)]).unwrap();
    assert_eq!(c.lines[0].text, "Insert 3 pages");
    let made: Vec<PageId> = c.created.pages().collect();
    let src = |id| d.state().def(id).unwrap().base.src.clone();
    let (blanks, imaged) = (src(made[0]), src(made[2]));
    assert_eq!(src(made[1]), blanks);
    let origin = |src: &Src| match src.blob().kind() {
        BlobKind::Pdf { origin, .. } => origin.clone(),
        _ => panic!("a PDF"),
    };
    assert_eq!(origin(&blanks), Origin::Memory(MemoryKind::Blank));
    assert_eq!(origin(&imaged), Origin::Memory(MemoryKind::Images));
    let pages = sheaf_image::io::read_bytes(png.clone()).unwrap();
    let want = [
        crate::memory::blank_pages(612.0, 792.0, 2).unwrap(),
        crate::images::to_pdf(&pages, Fit::LETTER).unwrap(),
    ];
    for (src, want) in [&blanks, &imaged].into_iter().zip(want) {
        assert_eq!(**src.blob().bytes(), *want);
        // Written by sheaf's serializer.
        let after = src
            .blob()
            .bytes()
            .splitn(2, |&b| b == b'\n')
            .nth(1)
            .unwrap();
        assert!(after.starts_with(crate::serialize::full::BINARY));
    }
    // The same op makes the same blob.
    let mut tx = d.tx(Actor::User);
    let again = tx.apply(images(by_id(), Some("1")), &NoProgress).unwrap();
    let Made::Page(page) = again.made[0] else {
        panic!("a page")
    };
    assert_eq!(tx.state().def(page).unwrap().base.src, imaged);
    // A single image named by the command line goes by its name.
    let named = tx.grant_named("scan.png", Grant::Blob(image.clone().into()));
    tx.apply(images(vec![named], None), &NoProgress).unwrap();
    let Some(ResolvedOp::InsertPages { name, .. }) = tx.steps.last().map(|s| &*s.op) else {
        panic!("an insert")
    };
    assert_eq!(&**name, "scan.png");
}

/// Blank pages in place of others keep their ids, bookmarks and marks, as
/// a file's pages do, and show at their size.
#[test]
fn blank_pages_replace_pages() {
    let mut d = doc_of(2, &flat(&[("Two", 1, 0)]));
    let p1 = d.page(1).unwrap();
    let add = Op::AddMark {
        page: PageRef::Id(p1),
        mark: note("n"),
        reply_to: None,
        local: None,
        imported: None,
    };
    commit(&mut d, Actor::User, vec![add]).unwrap();
    let size = sheaf_edit::PageSize {
        width: 300.0,
        height: 200.0,
    };
    let replace = Op::ReplacePages {
        pages: vec![PageRef::Id(p1)],
        from: PageSource::Blank { size, count: 1 },
        with: None,
        bring: Default::default(),
    };
    let c = commit(&mut d, Actor::User, vec![replace]).unwrap();
    assert_eq!(c.lines[0].text, "Replace page 2 with 1 page");
    let st = d.state();
    assert_eq!(st.page_at(1), Some(p1), "the same id");
    assert_eq!(st.marks_on(p1).count(), 1);
    assert_eq!(st.bookmark_count(), 1);
    let media = st.geom(p1).unwrap().media;
    assert_eq!((media.x1 - media.x0, media.y1 - media.y0), (300.0, 200.0));
}

/// A mark changed to what it is, or moved by nothing, isn't changed: no
/// version, and it keeps when it was last changed.
#[test]
fn a_patch_that_changes_nothing_changes_nothing() {
    let mut d = doc_of(1, &[]);
    let add = Op::AddMark {
        page: PageRef::Pos(1),
        mark: note("n"),
        reply_to: None,
        local: Some(Local(1)),
        imported: None,
    };
    let c = commit(&mut d, Actor::User, vec![add]).unwrap();
    let Made::Annot(id) = c.created.locals[&Local(1)] else {
        panic!("a mark");
    };
    let same = sheaf_edit::MarkPatch {
        contents: Some("n".into()),
        color: Some(style().color),
        ..Default::default()
    };
    let unmoved = Op::MoveMarks {
        marks: vec![AnnotRef::Id(id)],
        by: sheaf_edit::Delta::View(sheaf_edit::Vec2 { x: 0.0, y: 0.0 }),
        to_page: None,
    };
    let patch = Op::PatchMarks {
        marks: vec![AnnotRef::Id(id)],
        patch: same,
        imported: None,
    };
    let c = commit(&mut d, Actor::User, vec![patch, unmoved]).unwrap();
    assert_eq!(c.version, None);
}

/// A date or time format from the settings that isn't one writes the
/// default's rather than failing the transaction.
#[test]
fn a_format_that_isnt_one_writes_the_default() {
    let who = Identity {
        date_format: "%Q".into(),
        time_format: "%H".into(),
        ..Identity::default()
    };
    let host = HostAttrs::now(&who, "a.pdf".into());
    let year = chrono::Local::now().format("%Y").to_string();
    assert!(host.date.ends_with(&year), "{}", host.date);
    assert_eq!(host.date.len(), "10/19/2026".len());
}

/// An op canceled through its progress leaves the transaction as it was
/// before it: the ops before it stay, and it can go on.
#[test]
fn a_canceled_op_leaves_the_transaction_as_it_was() {
    let d = doc_of(5, &[]);
    let mut tx = d.tx(Actor::User);
    let rotate = Op::RotatePages {
        pages: PageSel::Positions("1".parse().unwrap()),
        by: Turn::Quarter,
    };
    tx.apply(rotate, &NoProgress).unwrap();
    let state = tx.state().clone();
    let canceled = in_progress::Tracker::new();
    canceled.cancel();
    let delete = Op::DeletePages {
        pages: PageSel::All,
    };
    assert_eq!(
        tx.apply(delete.clone(), &canceled),
        Err(EditError::Cancelled)
    );
    assert_eq!(*tx.state(), state);
    assert_eq!(tx.steps.len(), 1);
    tx.apply(delete, &NoProgress).unwrap();
    assert_eq!(tx.state().page_count(), 0);
}

/// A transaction can be built on another thread than the document's.
#[test]
fn a_transaction_goes_to_another_thread() {
    fn sent<T: Send + 'static>(_: T) {}
    sent(doc_of(1, &[]).tx(Actor::User));
}

/// An op's errors name it as its Op JSON tag does, for every op there is.
#[test]
fn an_op_is_named_as_op_json_names_it() {
    for op in sheaf_edit::fixture::every_op() {
        let json = serde_json::to_value(&op).unwrap();
        assert_eq!(op_name(&op), json["op"], "{json}");
    }
}

/// Another tool's comment and a part grouped under it (`/RT /Group`),
/// adopted in one op, are refused in either order: adopting the comment
/// takes the part, which it draws. Adopted alone, it does.
#[test]
fn a_grouped_part_is_not_adopted_beside_its_comment() {
    use lopdf::{Object, dictionary};
    let mut f = crate::forms::fixture::Form::new(1);
    let rect = |x: i64| vec![x.into(), 10.into(), (x + 20).into(), 30.into()];
    let text = f.pdf.add_object(dictionary! {
        "Type" => "Annot", "Subtype" => "FreeText", "Rect" => rect(10),
        "NM" => Object::string_literal("g1"), "Contents" => Object::string_literal("text"),
        "DA" => Object::string_literal("/Helv 12 Tf 0 g"),
    });
    let part = f.pdf.add_object(dictionary! {
        "Type" => "Annot", "Subtype" => "Square", "Rect" => rect(40),
        "IRT" => text, "RT" => "Group", "NM" => Object::string_literal("g2"),
    });
    f.set_annots(0, &[text, part]);
    let mut d = opened(f.bytes(), "a.pdf");
    let nm = |nm: &str| AnnotRef::Nm {
        nm: nm.into(),
        page: Some(PageRef::Pos(1)),
    };
    let adoptable = |d: &Document, nm: &str| {
        let id = d.state().annot_by_nm(d.page(0).unwrap(), nm).unwrap();
        d.state()
            .annot(id)
            .unwrap()
            .foreign()
            .unwrap()
            .summary
            .mark
            .is_some()
    };
    assert!(adoptable(&d, "g1") && adoptable(&d, "g2"));
    for annots in [vec![nm("g1"), nm("g2")], vec![nm("g2"), nm("g1")]] {
        let adopt = Op::AdoptAnnotations { annots };
        let e = commit(&mut d, Actor::User, vec![adopt]).unwrap_err();
        assert!(matches!(e, EditError::Invalid { .. }), "{e:?}");
    }
    let adopt = Op::AdoptAnnotations {
        annots: vec![nm("g1")],
    };
    commit(&mut d, Actor::User, vec![adopt]).unwrap();
    assert_eq!(d.state().annot_count(), 1, "the part went with it");
    assert_eq!(d.state().marks().count(), 1);
}

/// A reply sheaf holds is to a comment on its page: another tool's reply
/// to a comment on another page isn't adopted, nor a comment one of
/// sheaf's replies on another page is to. One whose replies are on its
/// page is.
#[test]
fn an_adopted_comment_keeps_its_thread_on_its_page() {
    use lopdf::{Object, dictionary};
    let mut f = crate::forms::fixture::Form::new(2);
    let rect = |x: i64| vec![x.into(), 10.into(), (x + 20).into(), 30.into()];
    let mut note = |nm: &str, irt: Option<lopdf::ObjectId>| {
        let mut dict = dictionary! {
            "Type" => "Annot", "Subtype" => "Text", "Rect" => rect(10),
            "NM" => Object::string_literal(nm), "Contents" => Object::string_literal(nm),
        };
        if let Some(irt) = irt {
            dict.set("IRT", irt);
        }
        f.pdf.add_object(dict)
    };
    let (c, c2) = (note("c", None), note("c2", None));
    let r = note("r", Some(c));
    f.set_annots(0, &[c, c2]);
    f.set_annots(1, &[r]);
    let ours = crate::annot::Annot {
        id: crate::annot::AnnotId(1),
        kind: AnnotKind::Reply {
            parent: "c2".into(),
        },
        color: [255, 0, 0],
        opacity: 1.0,
        stroke: 1.0,
        author: "t".into(),
        contents: "ours".into(),
        modified: String::new(),
        created: String::new(),
        fill: None,
        dashed: false,
        cloudy: false,
        turn: Rotation::R0,
        name: "ours".into(),
    };
    let page = f.pages[1];
    let mut dict = f.dict(page).clone();
    let marks = crate::forms::fixture::holding(&[ours]);
    crate::forms::fixture::write_marks(&mut f.pdf, &mut dict, &marks);
    *f.dict(page) = dict;
    let mut d = opened(f.bytes(), "a.pdf");
    assert!(
        d.state()
            .marks()
            .any(|(_, m)| m.thread_parent() == Some("c2"))
    );
    let adopt = |nm: &str, pos| Op::AdoptAnnotations {
        annots: vec![AnnotRef::Nm {
            nm: nm.into(),
            page: Some(PageRef::Pos(pos)),
        }],
    };
    for refused in [adopt("r", 2), adopt("c2", 1)] {
        let e = commit(&mut d, Actor::User, vec![refused]).unwrap_err();
        assert!(matches!(e, EditError::Invalid { .. }), "{e:?}");
    }
    commit(&mut d, Actor::User, vec![adopt("c", 1)]).unwrap();
    assert_eq!(d.state().check_invariants(), Ok(()));
    assert_eq!(d.state().marks().count(), 2);
}

/// Ops read from a file name things as a person sees them (§14.2): a page
/// by its position, a bookmark by its titles, an annotation by its `/NM`
/// and a file by the name the command line gave it. A session's ids mean
/// nothing there, so an op naming any of them by id is Invalid, checked or
/// applied, and changes nothing; the same op not from a file applies.
#[test]
fn ids_mean_nothing_in_ops_read_from_a_file() {
    let mut d = doc_of(3, &flat(&[("Part I", 0, 0), ("Ch. 1", 1, 1)]));
    let note: Op = serde_json::from_str(
        r#"{"op": "add-mark", "page": {"pos": 1},
            "mark": {"kind": {"type": "note", "pos": {"x": 10, "y": 10}}, "contents": "here",
                     "style": {"color": [255, 0, 0], "opacity": 1, "stroke": 1}}}"#,
    )
    .unwrap();
    let made = commit(&mut d, Actor::User, vec![note]).unwrap();
    let Made::Annot(mark) = made.created.ops[0][0] else {
        panic!("a mark: {:?}", made.created);
    };
    let name = d.state().annot(mark).unwrap().name();
    let chapter = d.state().bookmarks_preorder()[1].1;
    let page = d.page(1).unwrap();
    let primary = d.primary().clone();
    let from_file = |d: &Document| {
        let mut tx = d.tx(Actor::Cli);
        tx.ops_from_file();
        tx.grant_named("again", Grant::Blob(primary.blob().clone().into()));
        tx
    };
    let by_id = [
        Op::RotatePages {
            pages: PageSel::Ids(vec![PageRef::Id(page)]),
            by: Turn::Quarter,
        },
        Op::DeleteBookmarks {
            bookmarks: vec![BookmarkRef::Id(chapter)],
        },
        Op::DeleteAnnotations {
            annots: vec![AnnotRef::Id(mark)],
        },
        Op::InsertPages {
            from: PageSource::File {
                file: BlobSel::Id(primary.id()),
                name: "a.pdf".into(),
            },
            pages: None,
            before: None,
            bring: Default::default(),
            locals: Vec::new(),
        },
    ];
    let before = d.current_version();
    for op in &by_id {
        let mut tx = from_file(&d);
        let checked = tx.check(op).unwrap_err();
        let applied = tx.apply(op.clone(), &NoProgress).unwrap_err();
        assert_eq!(checked, applied);
        let EditError::Invalid { why, .. } = &applied else {
            panic!("{applied:?}");
        };
        assert!(
            why.contains("session ids aren't meaningful in a file"),
            "{why}"
        );
        assert!(tx.preview().lines.is_empty());
    }
    assert_eq!(d.current_version(), before);
    // Named as a person sees them, the same ops apply.
    let ops: Vec<Op> = serde_json::from_value(serde_json::json!([
        {"op": "rotate-pages", "pages": {"ids": [{"pos": 2}]}, "by": 90},
        {"op": "delete-bookmarks", "bookmarks": [{"path": ["Part I", "Ch. 1"]}]},
        {"op": "delete-annotations", "annots": [{"nm": name}]},
        {"op": "insert-pages", "from": {"type": "file", "file": {"with": "again"}, "name": "a.pdf"}},
    ]))
    .unwrap();
    let mut tx = from_file(&d);
    tx.apply_batch(ops, &NoProgress).unwrap();
    d.commit(tx, CommitOpts::default()).unwrap();
    assert_eq!(d.geom(page).rotate, Rotation::R90);
    assert_eq!(d.state().bookmark_count(), 1);
    assert!(d.state().annot(mark).is_none());
    assert_eq!(d.page_count(), 6);
    // Not from a file, ids name what they did.
    d.undo().unwrap();
    commit(&mut d, Actor::Cli, by_id.to_vec()).unwrap();
    assert_eq!(d.page_count(), 6);
}
