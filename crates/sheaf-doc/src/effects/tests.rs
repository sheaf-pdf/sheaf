//! `classify`, row by row of §7.1's table, and over every pair of a
//! sample of each slot's values: what differs means something, and the
//! same means nothing. `judge` over a path, what it found removed, the
//! fields it touched and the slots that changed.

use super::*;
use crate::annot::{Box2, Pt};
use crate::file::ForeignAnnot;
use crate::forms::FieldKind;
use crate::forms::field::Mk;
use crate::labels::Style;
use crate::model::{
    FieldProps, FormDoc, FormInst, GenId, GenSpec, Mark, OutlineId, PageBase, SourceId, SourceInfo,
    Target, TextStyle, WidgetProps,
};
use crate::stamps::HeaderFooter;
use crate::store::{Src, Store};
use crate::tape::{Line, Op, Row, Tape};
use crate::text_layer::{OcrWord, TextLayer};

const OTHER: SigClass = SigClass::OTHER;

/// `n` blank pages, page `i` showing page `i` of `src`.
fn state_of(src: &Src, n: u32) -> DocState {
    let pages = (0..n).map(|index| {
        let def = PageDef {
            base: PageBase {
                source: SourceId(0),
                src: src.clone(),
                index,
            },
            copy: 0,
            content: crate::model::ContentStack::empty(),
        };
        (
            PageId(index.into()),
            Arc::new(def),
            Arc::new(src.geom(index)),
        )
    });
    DocState::builder().pages(pages).build()
}

fn blank(n: u32) -> DocState {
    state_of(&SourceInfo::blank(&Store::new(), n).src, n)
}

fn sq(x: f32) -> Box2 {
    Box2 {
        x,
        y: 1.0,
        w: 5.0,
        h: 5.0,
    }
}

fn mark(kind: AnnotKind, contents: &str) -> Annot {
    Annot {
        contents: contents.into(),
        kind,
        ..crate::forms::fixture::note()
    }
}

fn rec(page: u64, body: AnnotBody) -> Val {
    Val::Annot(AnnotRec {
        page: PageId(page),
        body,
    })
}

fn of_mark(a: Annot) -> Val {
    rec(0, AnnotBody::Mark(Arc::new(Mark::from(a))))
}

fn summary(contents: &str, mark: Option<Annot>) -> ForeignAnnot {
    ForeignAnnot {
        subtype: "Text".into(),
        author: "them".into(),
        contents: contents.into(),
        modified: String::new(),
        rect: sq(1.0),
        name: "theirs".into(),
        parent: None,
        grouped: false,
        state: None,
        created: String::new(),
        slot: 0,
        hidden: false,
        color: None,
        mark: mark.map(Box::new),
    }
}

fn foreign(summary: ForeignAnnot) -> Val {
    rec(
        0,
        AnnotBody::Foreign(Arc::new(Foreign {
            src: None,
            copy: 0,
            summary: Arc::new(summary),
        })),
    )
}

fn widget(field: u64, state: Option<&str>) -> Widget {
    Widget {
        field: FieldId(field),
        src: None,
        inst: FormInst {
            src: SourceId(0),
            copy: 0,
        },
        merged: true,
        rect: [0.0, 0.0, 10.0, 10.0],
        flags: 4,
        mk: Arc::default(),
        da: None,
        state: state.map(Into::into),
        states: Arc::from([]),
        props: Arc::new(WidgetProps {
            border: None,
            actions: Vec::new(),
            has_appearance: true,
        }),
    }
}

fn of_widget(w: Widget) -> Val {
    rec(0, AnnotBody::Widget(Arc::new(w)))
}

fn terminal(kind: FieldKind, value: Option<Value>) -> Terminal {
    Terminal {
        kind,
        flags: 0,
        value,
        default: None,
        props: Arc::new(FieldProps {
            da: None,
            align: None,
            max_len: None,
            options: Arc::from([]),
            tooltip: None,
            actions: Vec::new(),
        }),
        widgets: Vector::new(),
        outside: 0,
    }
}

fn node(partial: &str, parent: Option<u64>, term: Option<Terminal>) -> FieldNode {
    FieldNode {
        parent: parent.map(FieldId),
        partial: partial.into(),
        src: None,
        inst: FormInst {
            src: SourceId(0),
            copy: 0,
        },
        kids: Vector::new(),
        term: term.map(Arc::new),
    }
}

fn of_field(n: FieldNode) -> Val {
    Val::Field(Arc::new(n))
}

fn text(t: &str) -> Option<Value> {
    Some(Value::Text(t.into()))
}

fn state(s: &str) -> Option<Value> {
    Some(Value::State(s.into()))
}

fn bookmark(title: &str, page: u64) -> Val {
    Val::Bookmark(Arc::new(Bookmark {
        title: title.into(),
        target: Some(Target::Page {
            page: PageId(page),
            view: None,
        }),
        color: None,
        style: TextStyle::default(),
        open: true,
        kids: Vector::new(),
        src: None,
    }))
}

fn labels(prefixes: &[&str]) -> Val {
    let specs = prefixes.iter().zip(0..).map(|(p, i)| LabelSpec {
        start: PageId(i),
        style: Style::Decimal,
        prefix: p.to_string(),
        first: 1,
    });
    Val::Labels(specs.collect())
}

fn header(t: &str) -> Val {
    let mut hf = HeaderFooter::default();
    hf.texts[0] = t.into();
    Val::Generator(Arc::new(Generator {
        id: GenId(1),
        spec: GenSpec::HeaderFooter(Arc::new(hf)),
    }))
}

fn layer(words: &str) -> Arc<TextLayer> {
    let word = OcrWord {
        text: words.into(),
        rect: sq(10.0),
        confidence: 1.0,
    };
    Arc::new(TextLayer::from_engine(
        vec![word],
        &crate::geom::Geom::read(None, None, 0),
        "test",
    ))
}

/// `st`'s page `i` with `text` its recognized text, or none.
fn with_ocr(st: &DocState, i: u64, text: Option<Arc<TextLayer>>) -> Val {
    let def = st.def(PageId(i)).unwrap();
    Val::Page(Arc::new(PageDef {
        content: def.content.with_ocr_text(text),
        ..PageDef::clone(def)
    }))
}

fn order(ids: &[u64]) -> Val {
    Val::Order(ids.iter().map(|&i| PageId(i)).collect())
}

/// `st` with `key` holding `val`.
fn putting(st: &DocState, puts: &[(Key, Option<Val>)]) -> DocState {
    let mut target = st.clone();
    for (key, val) in puts {
        target.set(key, val.clone()).unwrap();
    }
    target
}

/// What putting each of `puts` in `st` means, judged as a path against
/// `st`. Each key's classify agrees with the path's.
fn path(st: &DocState, puts: &[(Key, Option<Val>)]) -> PathEffects {
    let target = putting(st, puts);
    let keys: BTreeSet<Key> = puts.iter().map(|(k, _)| *k).collect();
    let judged = judge(st, &target, &keys);
    let cx = ClassifyCx::new(st, &target);
    let classified = (keys.iter())
        .map(|k| classify(k, st.get(k).as_ref(), target.get(k).as_ref(), &cx))
        .fold(Effects::NONE, |a, b| a | b);
    assert_eq!(
        (judged.flags, judged.sig),
        (classified.flags, classified.sig)
    );
    judged
}

/// What putting `val` at `key` of `st` means.
fn put(st: &DocState, key: Key, val: Option<Val>) -> Effects {
    let judged = path(st, &[(key, val)]);
    Effects::of(judged.flags, judged.sig)
}

fn removed(path: &PathEffects) -> Vec<(RemovedKind, usize)> {
    path.removed.iter().map(|r| (r.kind, r.count)).collect()
}

/// `st` holding `val` at `key`: what a change is judged against.
fn holding(mut st: DocState, puts: &[(Key, Val)]) -> DocState {
    for (key, val) in puts {
        st.set(key, Some(val.clone())).unwrap();
    }
    st
}

const A: Key = Key::Annot(AnnotId(7));

#[test]
fn comments_added_and_removed() {
    let st = blank(2);
    let note = || {
        of_mark(mark(
            AnnotKind::Note {
                pos: Pt { x: 1.0, y: 1.0 },
            },
            "hi",
        ))
    };
    let added = path(&st, &[(A, Some(note()))]);
    assert_eq!((added.flags, added.sig), (Flags::ANNOTS, SigClass::ANNOT));
    assert_eq!(added.annots, AnnotOps::CREATE);
    let theirs = path(&st, &[(A, Some(foreign(summary("x", None))))]);
    assert_eq!(
        (theirs.flags, theirs.annots),
        (Flags::ANNOTS, AnnotOps::CREATE)
    );
    // A mark, another tool's comment, a link: each gone is removed.
    let mut link = summary("", None);
    link.subtype = "Link".into();
    for was in [note(), foreign(summary("x", None)), foreign(link)] {
        let st = holding(blank(2), &[(A, was)]);
        let gone = path(&st, &[(A, None)]);
        assert_eq!(gone.flags, Flags::ANNOTS | Flags::REMOVES);
        assert_eq!(gone.sig, SigClass::ANNOT);
        assert_eq!(removed(&gone), [(RemovedKind::Comments, 1)]);
        assert_eq!(gone.annots, AnnotOps::DELETE);
    }
}

#[test]
fn a_mark_s_text_replaced_and_its_style_changed() {
    let square = mark(AnnotKind::Square { rect: sq(1.0) }, "about this");
    let st = holding(blank(1), &[(A, of_mark(square.clone()))]);
    let replaced = |a: Annot| {
        let p = path(&st, &[(A, Some(of_mark(a)))]);
        assert_eq!(p.annots, AnnotOps::MODIFY);
        p
    };
    // Its contents, author, kind, and its kind's own text.
    let texts = [
        Annot {
            contents: "about that".into(),
            ..square.clone()
        },
        Annot {
            author: "another".into(),
            ..square.clone()
        },
        Annot {
            kind: AnnotKind::Ellipse { rect: sq(1.0) },
            ..square.clone()
        },
    ];
    for a in texts {
        let p = replaced(a);
        assert_eq!(p.flags, Flags::ANNOTS | Flags::REPLACES_TEXT);
        assert_eq!(removed(&p), [(RemovedKind::CommentText, 1)]);
    }
    // Moved, recolored, restyled: geometry and style only.
    let styles = [
        Annot {
            kind: AnnotKind::Square { rect: sq(9.0) },
            ..square.clone()
        },
        Annot {
            color: [0, 0, 255],
            opacity: 0.5,
            dashed: true,
            modified: "D:20261018".into(),
            ..square.clone()
        },
    ];
    for a in styles {
        let p = replaced(a);
        assert_eq!((p.flags, p.sig), (Flags::ANNOTS, SigClass::ANNOT));
        assert!(p.removed.is_empty());
    }
    // Struck text and tape rows are the kind's own text.
    let replace = |struck: &str| {
        let kind = AnnotKind::Replace {
            rects: vec![sq(1.0)],
            struck: struck.into(),
            at: sq(2.0),
        };
        of_mark(mark(kind, "new"))
    };
    let st = holding(blank(1), &[(A, replace("old"))]);
    assert!(
        put(&st, A, Some(replace("older")))
            .flags
            .contains(Flags::REPLACES_TEXT)
    );
    let tape = |value: f64| {
        let tape = Tape {
            lines: vec![Line::new(Row::Entry { value, op: Op::Add })],
        };
        let kind = AnnotKind::Tape {
            rect: sq(1.0),
            size: 10.0,
            tape,
            look: Default::default(),
        };
        of_mark(mark(kind, ""))
    };
    let st = holding(blank(1), &[(A, tape(1.0))]);
    assert!(
        put(&st, A, Some(tape(2.0)))
            .flags
            .contains(Flags::REPLACES_TEXT)
    );
}

/// Another tool's comment edited in sheaf becomes the mark its summary
/// reads it as: the same words are a style change, other words replaced
/// text.
#[test]
fn another_tool_s_comment_made_a_mark() {
    let as_mark = Annot {
        author: "them".into(),
        name: "theirs".into(),
        ..mark(
            AnnotKind::Note {
                pos: Pt { x: 1.0, y: 1.0 },
            },
            "theirs",
        )
    };
    let st = holding(
        blank(1),
        &[(A, foreign(summary("theirs", Some(as_mark.clone()))))],
    );
    let recolored = Annot {
        color: [9, 9, 9],
        ..as_mark.clone()
    };
    assert_eq!(put(&st, A, Some(of_mark(recolored))).flags, Flags::ANNOTS);
    let reworded = Annot {
        contents: "mine".into(),
        ..as_mark
    };
    let e = put(&st, A, Some(of_mark(reworded)));
    assert_eq!(e.flags, Flags::ANNOTS | Flags::REPLACES_TEXT);
    // One read as no mark says something sheaf can't compare.
    let st = holding(blank(1), &[(A, foreign(summary("theirs", None)))]);
    let e = put(
        &st,
        A,
        Some(of_mark(mark(AnnotKind::Square { rect: sq(1.0) }, "theirs"))),
    );
    assert!(e.flags.contains(Flags::REPLACES_TEXT));
}

#[test]
fn a_page_s_annotations() {
    let w = Key::Annot(AnnotId(1));
    let m = Key::Annot(AnnotId(2));
    let n = Key::Annot(AnnotId(3));
    let note = of_mark(mark(
        AnnotKind::Note {
            pos: Pt { x: 1.0, y: 1.0 },
        },
        "",
    ));
    let st = holding(
        blank(1),
        &[
            (w, of_widget(widget(0, None))),
            (m, note.clone()),
            (n, note),
        ],
    );
    let list = |ids: &[u64]| Some(Val::PageAnnots(ids.iter().map(|&i| AnnotId(i)).collect()));
    let p = Key::PageAnnots(PageId(0));
    let st = holding(st, &[(p, list(&[1, 2, 3]).unwrap())]);
    // Comments in or out, or in another order.
    let e = put(&st, p, list(&[1, 2]));
    assert_eq!((e.flags, e.sig), (Flags::ANNOTS, SigClass::ANNOT));
    let reordered = path(&st, &[(p, list(&[1, 3, 2]))]);
    assert_eq!(reordered.flags, Flags::ANNOTS);
    assert_eq!(reordered.annots, AnnotOps::MODIFY);
    // Widgets in or out: the form's tree.
    let e = put(&st, p, list(&[2, 3]));
    assert_eq!((e.flags, e.sig), (Flags::FIELD_TREE, OTHER));
    // A comment moved among the widgets.
    let e = put(&st, p, list(&[2, 1, 3]));
    assert_eq!((e.flags, e.sig), (Flags::ANNOTS, SigClass::ANNOT));
    assert_eq!(put(&st, p, None).flags, Flags::ANNOTS | Flags::FIELD_TREE);
}

#[test]
fn widgets() {
    let st = holding(blank(1), &[(A, of_widget(widget(0, Some("Off"))))]);
    let e = put(&st, A, Some(of_widget(widget(0, Some("Yes")))));
    assert_eq!((e.flags, e.sig), (Flags::FIELD_VALUES, SigClass::FILL));
    let changes: [fn(&mut Widget); 4] = [
        |w| w.rect = [1.0, 1.0, 9.0, 9.0],
        |w| {
            w.mk = Arc::new(Mk {
                rotation: 90,
                ..Mk::default()
            })
        },
        |w| w.da = Some("/Helv 9 Tf".into()),
        |w| w.flags = 2,
    ];
    for change in changes {
        let mut w = widget(0, Some("Off"));
        change(&mut w);
        let e = put(&st, A, Some(of_widget(w)));
        assert_eq!((e.flags, e.sig), (Flags::FIELD_TREE, OTHER));
    }
    // Moved to another page.
    let moved = rec(1, AnnotBody::Widget(Arc::new(widget(0, Some("Off")))));
    assert_eq!(put(&st, A, Some(moved)).flags, Flags::FIELD_TREE);
    // Added, and removed.
    let e = put(&blank(1), A, Some(of_widget(widget(0, None))));
    assert_eq!((e.flags, e.sig), (Flags::FIELD_TREE, OTHER));
    let gone = path(&st, &[(A, None)]);
    assert_eq!(gone.flags, Flags::FIELD_TREE | Flags::REMOVES);
    assert_eq!(removed(&gone), [(RemovedKind::Widgets, 1)]);
}

#[test]
fn geometry_and_order() {
    let st = blank(3);
    let turned = crate::geom::Geom::read(None, None, 90);
    let e = put(&st, Key::Geom(PageId(1)), Some(Val::Geom(Arc::new(turned))));
    assert_eq!((e.flags, e.sig), (Flags::GEOMETRY, OTHER));
    let e = put(&st, Key::Order, Some(order(&[2, 1, 0])));
    assert_eq!((e.flags, e.sig), (Flags::PAGE_ORDER, OTHER));
    let e = put(&st, Key::Order, Some(order(&[0, 1, 2, 9])));
    assert_eq!(e.flags, Flags::PAGE_ORDER | Flags::PAGE_ADD);
    let e = put(&st, Key::Order, Some(order(&[2, 0])));
    assert_eq!(e.flags, Flags::PAGE_ORDER | Flags::PAGE_REMOVE);
}

#[test]
fn pages_added_removed_and_rebased() {
    let store = Store::new();
    let (one, other) = (
        SourceInfo::blank(&store, 3).src,
        SourceInfo::blank(&store, 4).src,
    );
    let st = state_of(&one, 3);
    let page = |i: u64| Key::Page(PageId(i));
    // Page 2 shows page 1 of the file too.
    let st = holding(
        st.clone(),
        &[(page(2), Val::Page(st.def(PageId(1)).unwrap().clone()))],
    );
    let moved = Flags::PAGE_ORDER | Flags::GEOMETRY;
    // Added: page 9, another copy of page 0.
    let added = path(
        &st,
        &[
            (Key::Order, Some(order(&[0, 1, 2, 9]))),
            (page(9), Some(Val::Page(st.def(PageId(0)).unwrap().clone()))),
            (
                Key::Geom(PageId(9)),
                Some(Val::Geom(st.geom(PageId(0)).unwrap().clone())),
            ),
        ],
    );
    assert_eq!((added.flags, added.sig), (moved | Flags::PAGE_ADD, OTHER));
    // Removed: page 1's source page is shown still, page 0's isn't.
    let without = |i: u64| {
        let left: Vec<u64> = (0..3).filter(|&p| p != i).collect();
        path(
            &st,
            &[
                (Key::Order, Some(order(&left))),
                (page(i), None),
                (Key::Geom(PageId(i)), None),
            ],
        )
    };
    assert_eq!(without(1).flags, moved | Flags::PAGE_REMOVE);
    let gone = without(0);
    assert_eq!(gone.flags, moved | Flags::PAGE_REMOVE | Flags::REMOVES);
    assert_eq!(removed(&gone), [(RemovedKind::Pages, 1)]);
    // Rebased onto another file: the old page shows nowhere, but for page
    // 1's, which page 2 shows.
    let rebased = |i: u64| {
        let def = st.def(PageId(i)).unwrap();
        let base = PageBase {
            src: other.clone(),
            ..def.base.clone()
        };
        Some(Val::Page(Arc::new(PageDef {
            base,
            ..PageDef::clone(def)
        })))
    };
    assert_eq!(
        put(&st, page(0), rebased(0)).flags,
        Flags::CONTENT | Flags::REMOVES
    );
    assert_eq!(put(&st, page(1), rebased(1)).flags, Flags::CONTENT);
}

#[test]
fn recognized_text() {
    let st = blank(2);
    let page = Key::Page(PageId(0));
    let e = put(&st, page, Some(with_ocr(&st, 0, Some(layer("first")))));
    assert_eq!((e.flags, e.sig), (Flags::CONTENT, OTHER));
    let ocr = layer("first");
    let st = holding(st.clone(), &[(page, with_ocr(&st, 0, Some(ocr.clone())))]);
    let removes = Flags::CONTENT | Flags::REMOVES | Flags::REPLACES_TEXT;
    // Replaced, and removed.
    for text in [Some(layer("second")), None] {
        let p = path(&st, &[(page, Some(with_ocr(&st, 0, text)))]);
        assert_eq!(p.flags, removes);
        assert_eq!(removed(&p), [(RemovedKind::OcrLayers, 1)]);
    }
    // Removed where another page shows it still: nothing's gone.
    let shared = with_ocr(&st, 1, Some(ocr));
    let p = path(
        &st,
        &[
            (page, Some(with_ocr(&st, 0, None))),
            (Key::Page(PageId(1)), Some(shared)),
        ],
    );
    assert_eq!(p.flags, Flags::CONTENT);
}

#[test]
fn field_values() {
    let f = Key::Field(FieldId(0));
    let valued = |v| of_field(node("name", None, Some(terminal(FieldKind::Text, v))));
    let st = holding(blank(1), &[(f, valued(None))]);
    let e = put(&st, f, Some(valued(text("Ann"))));
    assert_eq!((e.flags, e.sig), (Flags::FIELD_VALUES, SigClass::FILL));
    let st = holding(blank(1), &[(f, valued(text("")))]);
    assert_eq!(
        put(&st, f, Some(valued(text("Ann")))).flags,
        Flags::FIELD_VALUES
    );
    // A value replaced or cleared.
    let st = holding(blank(1), &[(f, valued(text("Ann")))]);
    for now in [text("Bob"), text(""), None] {
        let p = path(&st, &[(f, Some(valued(now)))]);
        assert_eq!(p.flags, Flags::FIELD_VALUES | Flags::REPLACES_TEXT);
        assert_eq!(p.sig, SigClass::FILL);
        assert_eq!(removed(&p), [(RemovedKind::FieldValues, 1)]);
    }
    // A box checked is filled in; unchecked, its value is replaced.
    let boxed = |v| of_field(node("agree", None, Some(terminal(FieldKind::CheckBox, v))));
    let st = holding(blank(1), &[(f, boxed(state("Off")))]);
    assert_eq!(
        put(&st, f, Some(boxed(state("Yes")))).flags,
        Flags::FIELD_VALUES
    );
    let st = holding(blank(1), &[(f, boxed(state("Yes")))]);
    let e = put(&st, f, Some(boxed(state("Off"))));
    assert_eq!(e.flags, Flags::FIELD_VALUES | Flags::REPLACES_TEXT);
}

#[test]
fn field_nodes() {
    let f = Key::Field(FieldId(1));
    let st = holding(blank(1), &[(f, of_field(node("b", Some(0), None)))]);
    let tree = Effects::of(Flags::FIELD_TREE, OTHER);
    assert_eq!(
        put(&blank(1), f, Some(of_field(node("b", None, None)))),
        tree
    );
    let gone = path(&st, &[(f, None)]);
    assert_eq!(gone.flags, Flags::FIELD_TREE | Flags::REMOVES);
    assert_eq!(removed(&gone), [(RemovedKind::Fields, 1)]);
    // Reparented; renamed, its old name replaced text.
    assert_eq!(put(&st, f, Some(of_field(node("b", Some(5), None)))), tree);
    let renamed = path(&st, &[(f, Some(of_field(node("c", Some(0), None))))]);
    assert_eq!(renamed.flags, Flags::FIELD_TREE | Flags::REPLACES_TEXT);
    assert_eq!(removed(&renamed), [(RemovedKind::FieldText, 1)]);
    // A terminal's tooltip replaced, or its flags changed.
    let with = |tooltip: Option<&str>, flags: u32| {
        let mut t = terminal(FieldKind::Text, None);
        t.flags = flags;
        t.props = Arc::new(FieldProps {
            tooltip: tooltip.map(Into::into),
            ..FieldProps::clone(&t.props)
        });
        of_field(node("b", Some(0), Some(t)))
    };
    let st = holding(blank(1), &[(f, with(Some("Your name"), 0))]);
    let e = put(&st, f, Some(with(Some("Name"), 0)));
    assert_eq!(e.flags, Flags::FIELD_TREE | Flags::REPLACES_TEXT);
    assert_eq!(put(&st, f, Some(with(Some("Your name"), 1))), tree);
}

#[test]
fn the_form_bookmarks_labels_and_generators() {
    let st = blank(2);
    let form = FormDoc {
        sig_flags: 3,
        ..FormDoc::default()
    };
    let e = put(&st, Key::Form, Some(Val::Form(Arc::new(form))));
    assert_eq!((e.flags, e.sig), (Flags::FORM, OTHER));
    let roots = Val::OutlineRoots([OutlineId(1)].into_iter().collect());
    assert_eq!(
        put(&st, Key::OutlineRoots, Some(roots)).flags,
        Flags::OUTLINE
    );
    // Bookmarks: added, retargeted, retitled, removed.
    let b = Key::Bookmark(OutlineId(1));
    let e = put(&st, b, Some(bookmark("Intro", 0)));
    assert_eq!((e.flags, e.sig), (Flags::OUTLINE, OTHER));
    let st = holding(st, &[(b, bookmark("Intro", 0))]);
    assert_eq!(
        put(&st, b, Some(bookmark("Intro", 1))).flags,
        Flags::OUTLINE
    );
    let retitled = path(&st, &[(b, Some(bookmark("Preface", 0)))]);
    assert_eq!(retitled.flags, Flags::OUTLINE | Flags::REPLACES_TEXT);
    assert_eq!(removed(&retitled), [(RemovedKind::BookmarkTitles, 1)]);
    let gone = path(&st, &[(b, None)]);
    assert_eq!(gone.flags, Flags::OUTLINE | Flags::REMOVES);
    assert_eq!(removed(&gone), [(RemovedKind::Bookmarks, 1)]);
    // Labels: a prefix that disappears is replaced text.
    let l = Key::Labels;
    assert_eq!(put(&st, l, Some(labels(&["A-"]))).flags, Flags::LABELS);
    let st = holding(st, &[(l, labels(&["A-", "B-"]))]);
    assert_eq!(
        put(&st, l, Some(labels(&["B-", "A-", "C-"]))).flags,
        Flags::LABELS
    );
    let p = path(&st, &[(l, Some(labels(&["A-", ""])))]);
    assert_eq!(p.flags, Flags::LABELS | Flags::REPLACES_TEXT);
    assert_eq!(removed(&p), [(RemovedKind::LabelPrefixes, 1)]);
    assert_eq!(
        put(&st, l, None).flags,
        Flags::LABELS | Flags::REPLACES_TEXT
    );
    // The legacy header: added is drawn; changed or gone, its text with
    // it.
    let g = Key::Generator(GenId(1));
    let e = put(&st, g, Some(header("Draft")));
    assert_eq!((e.flags, e.sig), (Flags::CONTENT, OTHER));
    let st = holding(st, &[(g, header("Draft"))]);
    let changed = path(&st, &[(g, Some(header("Final")))]);
    assert_eq!(
        changed.flags,
        Flags::CONTENT | Flags::REMOVES | Flags::REPLACES_TEXT
    );
    assert_eq!(removed(&changed), [(RemovedKind::HeaderFooter, 1)]);
    assert_eq!(
        put(&st, g, None).flags,
        Flags::REMOVES | Flags::REPLACES_TEXT
    );
}

/// A value a slot can't hold is judged as everything.
#[test]
fn a_misfit_is_everything() {
    let st = blank(1);
    let cx = ClassifyCx::new(&st, &st);
    let e = classify(&Key::Order, None, Some(&labels(&["A-"])), &cx);
    assert_eq!(e, MISFIT);
}

/// Values of every slot, a few each: the same twice over, built apart.
fn samples(st: &DocState) -> Vec<(Key, Vec<Option<Val>>)> {
    let annots = || {
        let mut vals = vec![None, Some(of_widget(widget(0, Some("Off"))))];
        vals.push(Some(of_widget(widget(0, Some("Yes")))));
        vals.push(Some(foreign(summary(
            "x",
            Some(crate::forms::fixture::note()),
        ))));
        vals.push(Some(foreign(summary("y", None))));
        vals.extend(
            (crate::forms::fixture::every_mark_kind().into_iter()).map(|a| Some(of_mark(a))),
        );
        vals
    };
    let fields = || {
        let mut vals = vec![None, Some(of_field(node("a", None, None)))];
        for v in [
            None,
            text("x"),
            text("y"),
            state("Off"),
            Some(Value::Signed),
        ] {
            vals.push(Some(of_field(node(
                "a",
                None,
                Some(terminal(FieldKind::Text, v)),
            ))));
        }
        vals.push(Some(of_field(node("a", Some(3), None))));
        vals
    };
    let lists = || {
        [&[][..], &[1], &[1, 2], &[2, 1]]
            .map(|ids| Some(Val::PageAnnots(ids.iter().map(|&i| AnnotId(i)).collect())))
    };
    let geoms = || {
        [0, 90].map(|turn| {
            Some(Val::Geom(Arc::new(crate::geom::Geom::read(
                None, None, turn,
            ))))
        })
    };
    let mut out = vec![
        (
            Key::Order,
            vec![Some(order(&[0])), Some(order(&[0, 1])), Some(order(&[1]))],
        ),
        (
            Key::Page(PageId(0)),
            vec![
                None,
                Some(with_ocr(st, 0, None)),
                Some(with_ocr(st, 0, Some(layer("a")))),
                Some(with_ocr(st, 0, Some(layer("b")))),
            ],
        ),
        (Key::Geom(PageId(0)), geoms().into()),
        (
            Key::Bookmark(OutlineId(0)),
            vec![
                None,
                Some(bookmark("a", 0)),
                Some(bookmark("b", 0)),
                Some(bookmark("a", 1)),
            ],
        ),
        (
            Key::OutlineRoots,
            [&[][..], &[1], &[2]]
                .map(|ids| {
                    Some(Val::OutlineRoots(
                        ids.iter().map(|&i| OutlineId(i)).collect(),
                    ))
                })
                .into(),
        ),
        (Key::PageAnnots(PageId(0)), lists().into()),
        (Key::Annot(AnnotId(0)), annots()),
        (Key::Field(FieldId(0)), fields()),
        (
            Key::Form,
            [0, 3]
                .map(|sig_flags| {
                    let form = FormDoc {
                        sig_flags,
                        ..FormDoc::default()
                    };
                    Some(Val::Form(Arc::new(form)))
                })
                .into(),
        ),
        (
            Key::Labels,
            vec![
                None,
                Some(labels(&[""])),
                Some(labels(&["A-"])),
                Some(labels(&["B-"])),
            ],
        ),
        (
            Key::Generator(GenId(1)),
            vec![None, Some(header("a")), Some(header("b"))],
        ),
    ];
    // Each value again, built apart from the first.
    for (_, vals) in &mut out {
        let again = built_apart(vals);
        vals.extend(again);
    }
    out
}

/// `vals` copied deep: equal values, no `Arc` shared.
fn built_apart(vals: &[Option<Val>]) -> Vec<Option<Val>> {
    vals.iter()
        .map(|v| {
            v.as_ref().map(|v| match v {
                Val::Order(o) => Val::Order(o.iter().copied().collect()),
                Val::Page(p) => Val::Page(Arc::new(PageDef::clone(p))),
                Val::Geom(g) => Val::Geom(Arc::new(**g)),
                Val::Bookmark(b) => Val::Bookmark(Arc::new(Bookmark::clone(b))),
                Val::OutlineRoots(r) => Val::OutlineRoots(r.iter().copied().collect()),
                Val::PageAnnots(l) => Val::PageAnnots(l.iter().copied().collect()),
                Val::Annot(rec) => Val::Annot(AnnotRec {
                    page: rec.page,
                    body: match &rec.body {
                        AnnotBody::Mark(m) => AnnotBody::Mark(Arc::new(Mark::clone(m))),
                        AnnotBody::Foreign(f) => AnnotBody::Foreign(Arc::new(Foreign::clone(f))),
                        AnnotBody::Widget(w) => AnnotBody::Widget(Arc::new(Widget::clone(w))),
                    },
                }),
                Val::Field(f) => Val::Field(Arc::new(FieldNode::clone(f))),
                Val::Form(f) => Val::Form(Arc::new(FormDoc::clone(f))),
                Val::Labels(l) => Val::Labels(l.iter().cloned().collect()),
                Val::Generator(g) => Val::Generator(Arc::new(Generator::clone(g))),
            })
        })
        .collect()
}

/// Over every pair of each slot's samples: values that differ mean
/// something, flags and a class both, and the same values nothing.
#[test]
fn what_differs_means_something() {
    let st = blank(2);
    let cx = ClassifyCx::new(&st, &st);
    for (key, vals) in samples(&st) {
        for a in &vals {
            for b in &vals {
                let e = classify(&key, a.as_ref(), b.as_ref(), &cx);
                if same(a.as_ref(), b.as_ref()) {
                    assert_eq!(e, Effects::NONE, "{key:?}: {a:?} to {b:?}");
                } else {
                    assert!(
                        !e.flags.is_empty() && !e.sig.is_empty(),
                        "{key:?}: {a:?} to {b:?}"
                    );
                }
            }
        }
    }
}

/// The fields a path touches, for FieldMDP: a filled field's name, every
/// name under a renamed node, and a widget's field's.
#[test]
fn the_fields_a_path_touches() {
    let (top, kid, w) = (Key::Field(FieldId(0)), Key::Field(FieldId(1)), A);
    let parent = |name: &str| {
        let mut n = node(name, None, None);
        n.kids.push_back(FieldId(1));
        of_field(n)
    };
    let leaf = |v| of_field(node("b", Some(0), Some(terminal(FieldKind::Text, v))));
    let st = holding(
        blank(1),
        &[
            (top, parent("a")),
            (kid, leaf(None)),
            (w, of_widget(widget(1, None))),
        ],
    );
    let names = |p: PathEffects| p.fields.into_iter().collect::<Vec<_>>();
    assert_eq!(names(path(&st, &[(kid, Some(leaf(text("x"))))])), ["a.b"]);
    assert_eq!(
        names(path(&st, &[(top, Some(parent("z")))])),
        ["a", "a.b", "z", "z.b"]
    );
    let checked = of_widget(widget(1, Some("Yes")));
    assert_eq!(names(path(&st, &[(w, Some(checked))])), ["a.b"]);
    assert!(
        path(&st, &[(Key::Labels, Some(labels(&["A-"])))])
            .fields
            .is_empty()
    );
}

/// `changed` holds the slots whose values differ, and nothing else: a key
/// put back to its value isn't one.
#[test]
fn changed_is_by_value() {
    let st = blank(2);
    let p = path(
        &st,
        &[
            (Key::Order, Some(order(&[0, 1]))),
            (Key::Labels, Some(labels(&["A-"]))),
            (Key::Page(PageId(1)), Some(with_ocr(&st, 1, None))),
        ],
    );
    assert_eq!(p.changed, BTreeSet::from([Key::Labels]));
    assert_eq!(p.flags, Flags::LABELS);
}
