//! `classify`, row by row of §7.1's table, and over every pair of a
//! sample of each slot's values: what differs means something, and the
//! same means nothing. `judge` over a path, what it found removed, the
//! fields it touched and the slots that changed.

use sheaf_edit::{FontChoice, GridPos};

use super::*;
use crate::annot::{Box2, Pt};
use crate::content::{BlockId, ResId};
use crate::file::ForeignAnnot;
use crate::forms::FieldKind;
use crate::forms::field::Mk;
use crate::labels::Style;
use crate::model::{
    FieldProps, Filter, FormDoc, FormInst, Frozen, GenId, GenSpec, ImageRef, LegacySel, Mark,
    MarkedSel, OutlineId, PageBase, Placed, Scope, SourceId, StreamSel, Target, TextStyle,
    WatermarkGen, WidgetProps,
};
use crate::source::ObjRef;
use crate::store::{Src, Store};
use crate::tape::{Line, Op, Row, Tape};
use crate::text_layer::{OcrWord, TextLayer};

const OTHER: SigClass = SigClass::OTHER;

/// `n` blank pages, page `i` showing page `i` of `src`.
fn state_of(src: &Src, n: u32) -> DocState {
    let pages = (0..n).map(|index| {
        let base = PageBase {
            src: src.clone(),
            index,
        };
        let def = PageDef::new(base, 0, crate::model::ContentStack::empty());
        (
            PageId(index.into()),
            Arc::new(def),
            Arc::new(src.geom(index)),
        )
    });
    DocState::builder().pages(pages).build()
}

fn blank(n: u32) -> DocState {
    state_of(&crate::store::blank(&Store::new(), n), n)
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
            src: SourceId { hash: 0, len: 0 },
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
            src: SourceId { hash: 0, len: 0 },
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
    let spec = crate::tx::fixture::header(t);
    Val::Generator(Arc::new(Generator {
        id: GenId(1),
        spec: GenSpec::HeaderFooter(Arc::new(spec)),
        z: Z::Over,
        scope: Scope::All,
        frozen: Frozen::default(),
        replaces: BTreeSet::from([PaginationKind::Header, PaginationKind::Footer]),
    }))
}

/// A watermark at `z` drawing `content` at `position` on `scope`.
fn watermark(content: WatermarkContent, position: GridPos, z: Z, scope: Scope) -> Val {
    let wm = WatermarkGen { content, position };
    Val::Generator(Arc::new(Generator {
        id: GenId(1),
        spec: GenSpec::Watermark(Arc::new(wm)),
        z,
        scope,
        frozen: Frozen::default(),
        replaces: BTreeSet::from([WatermarkGen::kind_at(z)]),
    }))
}

/// `text`, 72 points, red, a third opaque, at 45°.
fn wm_text(text: &str, size: f32) -> WatermarkContent {
    WatermarkContent::Text {
        text: text.into(),
        font: Default::default(),
        size,
        color: [200, 30, 30],
        opacity: 0.3,
        angle: 45.0,
    }
}

/// The image object `n` of `src` draws, half the page wide.
fn wm_image(src: &Src, n: u32, scale: f32) -> WatermarkContent {
    WatermarkContent::Image {
        image: ImageRef::Placed(Placed {
            at: ObjRef {
                src: src.clone(),
                id: (n, 0),
            },
            res: ResId(u128::from(n)),
            turns: 0,
        }),
        scale,
        opacity: 1.0,
    }
}

/// Pages `ids`.
fn on(ids: &[u64]) -> Scope {
    Scope::Pages(ids.iter().map(|&i| PageId(i)).collect())
}

fn layer(words: &str) -> Arc<TextLayer> {
    let word = OcrWord {
        text: words.into(),
        rect: sq(10.0),
        confidence: 1.0,
    };
    Arc::new(crate::text_layer::from_engine(
        vec![word],
        &crate::geom::Geom::read(None, None, 0),
        "test",
    ))
}

/// `st`'s page `i` with `text` its recognized text, block `b1`'s, or
/// none.
fn with_ocr(st: &DocState, i: u64, text: Option<Arc<TextLayer>>) -> Val {
    with_layer(st, i, text.map(|text| (BlockId(1), text)))
}

/// `st`'s page `i` with `layer` its recognized text, a block's id and its
/// text, or none.
fn with_layer(st: &DocState, i: u64, layer: Option<(BlockId, Arc<TextLayer>)>) -> Val {
    let def = st.def(PageId(i)).unwrap();
    let layer = layer.map(|(id, text)| Layer {
        id,
        body: Static::OcrText(text),
    });
    let content = def.content.with_ocr(layer);
    Val::Page(Arc::new(PageDef::new(def.base.clone(), def.copy, content)))
}

/// `st`'s page `i` with `filter` added to its filters.
fn with_filter(st: &DocState, i: u64, filter: Filter) -> Val {
    let def = st.def(PageId(i)).unwrap();
    let content = def.content.with_filter(filter);
    Val::Page(Arc::new(PageDef::new(def.base.clone(), def.copy, content)))
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
        crate::store::blank(&store, 3),
        crate::store::blank(&store, 4),
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
        let def = PageDef::new(base, def.copy, def.content.clone());
        Some(Val::Page(Arc::new(def)))
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
    // The same text in another block is another layer.
    let other = with_layer(&st, 0, Some((BlockId(2), layer("first"))));
    let p = path(&st, &[(page, Some(other))]);
    assert_eq!(p.flags, removes);
}

/// The recognized text sheaf wrote in the file, taken out by a filter, in
/// streams of its own or merged into another's: replaced text, once a
/// page, whether a layer replaces it or not; the filter taken off again,
/// it's put back, and nothing's gone.
#[test]
fn saved_recognized_text_filtered() {
    let st = blank(1);
    let page = Key::Page(PageId(0));
    let removes = Flags::CONTENT | Flags::REMOVES | Flags::REPLACES_TEXT;
    let streams = Filter::DropStreams(StreamSel::SheafOcr);
    let merged = Filter::DropMarked(MarkedSel::SheafOcr);
    for filter in [&streams, &merged] {
        assert!(filter.drops_ocr());
        let p = path(&st, &[(page, Some(with_filter(&st, 0, filter.clone())))]);
        assert_eq!(p.flags, removes, "{filter:?}");
        assert_eq!(removed(&p), [(RemovedKind::OcrLayers, 1)]);
        assert_eq!(p.sig, OTHER);
    }
    let both = holding(st.clone(), &[(page, with_filter(&st, 0, streams))]);
    let both = holding(both.clone(), &[(page, with_filter(&both, 0, merged))]);
    let both = holding(
        both.clone(),
        &[(page, with_ocr(&both, 0, Some(layer("new"))))],
    );
    let p = path(&st, &[(page, both.get(&page))]);
    assert_eq!(p.flags, removes);
    assert_eq!(removed(&p), [(RemovedKind::OcrLayers, 1)]);
    let p = path(&both, &[(page, st.get(&page))]);
    assert_eq!(p.flags, removes, "the layer goes");
    assert_eq!(removed(&p), [(RemovedKind::OcrLayers, 1)]);
    let filtered = holding(
        st.clone(),
        &[(
            page,
            with_filter(&st, 0, Filter::DropStreams(StreamSel::SheafOcr)),
        )],
    );
    let p = path(&filtered, &[(page, st.get(&page))]);
    assert_eq!(p.flags, Flags::CONTENT);
    assert!(removed(&p).is_empty());
    assert!(!Filter::DropStreams(StreamSel::SheafGen(GenId(5))).drops_ocr());
}

/// A filter added takes what it filters out of the page: removed; taken
/// off again, it's put back, and nothing's gone.
#[test]
fn filters_added_and_taken_off() {
    let st = blank(2);
    let page = Key::Page(PageId(0));
    let filter = Filter::DropStreams(StreamSel::SheafGen(GenId(5)));
    let p = path(&st, &[(page, Some(with_filter(&st, 0, filter.clone())))]);
    assert_eq!(p.flags, Flags::CONTENT | Flags::REMOVES);
    assert_eq!(removed(&p), [(RemovedKind::Filtered, 1)]);
    assert_eq!(p.sig, OTHER);
    let filtered = holding(st.clone(), &[(page, with_filter(&st, 0, filter))]);
    let p = path(&filtered, &[(page, Some(with_ocr(&st, 0, None)))]);
    assert_eq!(p.flags, Flags::CONTENT);
    assert!(removed(&p).is_empty());
    let marked = Filter::DropMarked(MarkedSel::SheafGen(GenId(5)));
    let p = path(
        &filtered,
        &[(page, Some(with_filter(&filtered, 0, marked)))],
    );
    assert_eq!(p.flags, Flags::CONTENT | Flags::REMOVES);
    let acrobat_s = Filter::DropMarked(MarkedSel::Pagination {
        kinds: BTreeSet::from([crate::content::PaginationKind::Watermark]),
        origins: BTreeSet::from([crate::content::PagOrigin::Acrobat]),
    });
    let p = path(&st, &[(page, Some(with_filter(&st, 0, acrobat_s.clone())))]);
    assert_eq!(p.flags, Flags::CONTENT | Flags::REMOVES);
    assert_eq!(removed(&p), [(RemovedKind::Filtered, 1)]);
    // What an older sheaf kept, taken out, is counted apart: what's drawn
    // stays.
    let older = Filter::StripLegacy(LegacySel::Marked(MarkedSel::Pagination {
        kinds: BTreeSet::from([crate::content::PaginationKind::Watermark]),
        origins: BTreeSet::from([crate::content::PagOrigin::SheafLegacy]),
    }));
    let p = path(&st, &[(page, Some(with_filter(&st, 0, older.clone())))]);
    assert_eq!(p.flags, Flags::CONTENT | Flags::REMOVES);
    assert_eq!(removed(&p), [(RemovedKind::OlderSettings, 1)]);
    let both = holding(st.clone(), &[(page, with_filter(&st, 0, acrobat_s))]);
    let p = path(&both, &[(page, Some(with_filter(&both, 0, older)))]);
    assert_eq!(removed(&p), [(RemovedKind::OlderSettings, 1)]);
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
    // The header: added is drawn; its text changed, or gone, its text
    // with it.
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

/// A header and footer (§7.1): added, it's drawn; laid out otherwise (its
/// face, size, color, margins) or on more pages, drawn again, which an
/// Append writes. Drawing other text (its slots, its first page number,
/// its Bates numbering, what its date or file or a page's Bates number
/// says) replaces its text; so does leaving a page that's still a page,
/// and going. A page that goes with its number isn't what it removes.
#[test]
fn headers_and_footers() {
    let src = crate::store::blank(&Store::new(), 3);
    let st = state_of(&src, 3);
    let g = Key::Generator(GenId(1));
    let hf = |spec: &sheaf_edit::HeaderFooterSpec, scope: Scope, numbers: &[(u64, u64)]| {
        let numbers = numbers.iter().map(|&(p, n)| (PageId(p), n)).collect();
        Val::Generator(Arc::new(Generator {
            id: GenId(1),
            spec: GenSpec::HeaderFooter(Arc::new(spec.clone())),
            z: Z::Over,
            scope,
            frozen: Frozen {
                date: "2026-10-21".into(),
                file: "a.pdf".into(),
                bates: Some(Arc::new(numbers)),
            },
            replaces: BTreeSet::from([PaginationKind::Header, PaginationKind::Footer]),
        }))
    };
    let spec = sheaf_edit::HeaderFooterSpec {
        bates: Some(sheaf_edit::BatesSpec::default()),
        ..crate::tx::fixture::header("Page <<1>> <<bates>>")
    };
    let numbered = [(0, 1), (1, 2)];
    let e = put(&st, g, Some(hf(&spec, on(&[0, 1]), &numbered)));
    assert_eq!((e.flags, e.sig), (Flags::CONTENT, OTHER));
    let st = holding(st, &[(g, hf(&spec, on(&[0, 1]), &numbered))]);
    let with = |change: fn(&mut sheaf_edit::HeaderFooterSpec)| {
        let mut spec = spec.clone();
        change(&mut spec);
        spec
    };
    for laid_out in [
        hf(
            &with(|s| s.font = FontChoice::TimesRoman),
            on(&[0, 1]),
            &numbered,
        ),
        hf(&with(|s| s.size = 14.0), on(&[0, 1]), &numbered),
        hf(&with(|s| s.color = [200, 0, 0]), on(&[0, 1]), &numbered),
        hf(&with(|s| s.margins.top = 20.0), on(&[0, 1]), &numbered),
        hf(&spec, Scope::All, &numbered),
    ] {
        let p = path(&st, &[(g, Some(laid_out))]);
        assert_eq!((p.flags, p.sig), (Flags::CONTENT, OTHER));
        assert!(removed(&p).is_empty());
    }
    let text = Flags::REMOVES | Flags::REPLACES_TEXT;
    let generator = |frozen: Frozen| {
        let Some(Val::Generator(g)) = st.get(&g) else {
            panic!("a header")
        };
        Val::Generator(Arc::new(Generator {
            frozen,
            ..Generator::clone(&g)
        }))
    };
    let dated = generator(Frozen {
        date: "2027-01-01".into(),
        ..header_of(&st).frozen.clone()
    });
    let filed = generator(Frozen {
        file: "b.pdf".into(),
        ..header_of(&st).frozen.clone()
    });
    for unsaid in [
        hf(
            &with(|s| s.texts[0] = "Page <<1>>".into()),
            on(&[0, 1]),
            &numbered,
        ),
        hf(&with(|s| s.first_number = 5), on(&[0, 1]), &numbered),
        hf(
            &with(|s| s.bates.as_mut().unwrap().digits = 8),
            on(&[0, 1]),
            &numbered,
        ),
        hf(&spec, on(&[0, 1]), &[(0, 1), (1, 3)]),
        hf(&spec, on(&[0]), &[(0, 1)]),
        dated,
        filed,
    ] {
        let p = path(&st, &[(g, Some(unsaid))]);
        assert_eq!(p.flags, Flags::CONTENT | text);
        assert_eq!(removed(&p), [(RemovedKind::HeaderFooter, 1)]);
    }
    let p = path(&st, &[(g, None)]);
    let gone = (p.flags, removed(&p));
    assert_eq!(gone, (text, vec![(RemovedKind::HeaderFooter, 1)]));
    // A page gone with its number isn't what it removes.
    let p = path(
        &st,
        &[
            (Key::Order, Some(order(&[0, 2]))),
            (Key::Page(PageId(1)), None),
            (g, Some(hf(&spec, on(&[0]), &[(0, 1)]))),
        ],
    );
    assert!(
        !removed(&p)
            .iter()
            .any(|(k, _)| *k == RemovedKind::HeaderFooter)
    );
}

/// A reorder or a label change under a header whose tokens read where a
/// page is (its place, the count, its label) or its label draws them again
/// (`LIVE_REDRAW`, §7.1), which removes nothing; under one that reads
/// neither it draws nothing again. A step judges a reorder alike.
#[test]
fn live_tokens_are_drawn_again() {
    let src = crate::store::blank(&Store::new(), 3);
    let st = state_of(&src, 3);
    let g = Key::Generator(GenId(1));
    let live = |yes: bool| match yes {
        true => Flags::LIVE_REDRAW,
        false => Flags::empty(),
    };
    for (text, reordered, relabelled) in [
        ("Page <<1>>", true, false),
        ("of <<N>>", true, false),
        ("<<label>>", true, true),
        ("Draft <<date>>", false, false),
    ] {
        let st = holding(st.clone(), &[(g, header(text))]);
        let reorder = [(Key::Order, Some(order(&[1, 0, 2])))];
        let p = path(&st, &reorder);
        assert_eq!(p.flags, Flags::PAGE_ORDER | live(reordered), "{text}");
        let after = putting(&st, &reorder);
        let (before, now) = (st.get(&Key::Order), after.get(&Key::Order));
        let cx = ClassifyCx::of_step(&st, &after);
        let step = classify(&Key::Order, before.as_ref(), now.as_ref(), &cx);
        assert_eq!(step.flags, Flags::PAGE_ORDER | live(reordered), "{text}");
        let p = path(&st, &[(Key::Labels, Some(labels(&["A-"])))]);
        assert_eq!(p.flags, Flags::LABELS | live(relabelled), "{text}");
    }
}

/// The header and footer `st` holds.
fn header_of(st: &DocState) -> &Generator {
    st.header_footer().expect("a header and footer")
}

/// A watermark (§7.1): added, it's drawn; laid out otherwise (where, how
/// big, turned, colored, see-through), drawn again, which an Append
/// writes. Drawing another text replaces its text; another image removes
/// what it drew; so does leaving a page that's still a page, and going.
#[test]
fn watermarks_and_backgrounds() {
    use sheaf_edit::GridPos::{Center, TopLeft};
    let src = crate::store::blank(&Store::new(), 3);
    let st = state_of(&src, 3);
    let g = Key::Generator(GenId(1));
    let over = |content, position, scope| watermark(content, position, Z::Over, scope);
    let e = put(&st, g, Some(over(wm_text("A", 72.0), Center, Scope::All)));
    assert_eq!((e.flags, e.sig), (Flags::CONTENT, OTHER));
    let st = holding(st, &[(g, over(wm_text("A", 72.0), Center, on(&[0, 1])))]);
    for laid_out in [
        over(wm_text("A", 72.0), TopLeft, on(&[0, 1])),
        over(wm_text("A", 40.0), Center, on(&[0, 1])),
        over(wm_text("A", 72.0), Center, on(&[0, 1, 2])),
        over(wm_text("A", 72.0), Center, Scope::All),
    ] {
        let p = path(&st, &[(g, Some(laid_out))]);
        assert_eq!((p.flags, p.sig), (Flags::CONTENT, OTHER));
        assert!(removed(&p).is_empty());
    }
    let text = Flags::REMOVES | Flags::REPLACES_TEXT;
    let replaced = RemovedKind::Watermark { text: true };
    for unsaid in [
        over(wm_text("B", 72.0), Center, on(&[0, 1])),
        over(wm_text("A", 72.0), Center, on(&[1])),
        over(wm_image(&src, 9, 0.5), Center, on(&[0, 1])),
    ] {
        let p = path(&st, &[(g, Some(unsaid))]);
        assert_eq!(p.flags, Flags::CONTENT | text);
        assert_eq!(removed(&p), [(replaced, 1)]);
    }
    let p = path(&st, &[(g, None)]);
    assert_eq!((p.flags, removed(&p)), (text, vec![(replaced, 1)]));
    // A page gone from its pages with the page isn't what it removes.
    let p = path(
        &st,
        &[
            (Key::Order, Some(order(&[1, 2]))),
            (Key::Page(PageId(0)), None),
            (g, Some(over(wm_text("A", 72.0), Center, on(&[1])))),
        ],
    );
    assert!(!removed(&p).iter().any(|(k, _)| *k == replaced));
    // An image behind: another is removed, not text.
    let under = |content| watermark(content, Center, Z::Under, Scope::All);
    let st = holding(st, &[(g, under(wm_image(&src, 9, 0.5)))]);
    let p = path(&st, &[(g, Some(under(wm_image(&src, 9, 1.0))))]);
    assert_eq!(p.flags, Flags::CONTENT);
    let p = path(&st, &[(g, Some(under(wm_image(&src, 8, 0.5))))]);
    assert_eq!(p.flags, Flags::CONTENT | Flags::REMOVES);
    assert_eq!(removed(&p), [(RemovedKind::Background { text: false }, 1)]);
    // From every page to one.
    let narrowed = watermark(wm_image(&src, 9, 0.5), Center, Z::Under, on(&[0]));
    let p = path(&st, &[(g, Some(narrowed))]);
    assert_eq!(p.flags, Flags::CONTENT | Flags::REMOVES);
}

/// A watermark replacing others' of its kind (§7.1's `replaces` row): on
/// a page of the saved file whose base page holds Acrobat's, it takes that
/// off; on one with none, or as a background, which replaces another kind,
/// it's drawn; nor where the saved state's watermark replaced them
/// already. A step reads no page's blocks.
#[test]
fn a_watermark_replacing_others_takes_them_off() {
    use crate::materialize::others_tests::{ACROBAT, BODY, acrobat_file};
    use sheaf_edit::GridPos::{Center, TopLeft};
    let file = acrobat_file(&[&[BODY, ACROBAT], &[b"0 g".as_slice()]]);
    let kind = crate::store::BlobKind::file(std::path::Path::new("acrobat.pdf"), None);
    let src = Store::new().open(file.into(), kind).unwrap();
    let st = state_of(&src, 2);
    let g = Key::Generator(GenId(1));
    let over = |position, scope| watermark(wm_text("A", 72.0), position, Z::Over, scope);
    let p = path(&st, &[(g, Some(over(Center, Scope::All)))]);
    assert_eq!(p.flags, Flags::CONTENT | Flags::REMOVES);
    assert_eq!(removed(&p), [(RemovedKind::Filtered, 1)]);
    let p = path(&st, &[(g, Some(over(Center, on(&[1]))))]);
    assert_eq!((p.flags, removed(&p)), (Flags::CONTENT, vec![]));
    let behind = watermark(wm_text("A", 72.0), Center, Z::Under, Scope::All);
    assert_eq!(put(&st, g, Some(behind)).flags, Flags::CONTENT);
    let target = putting(&st, &[(g, Some(over(Center, Scope::All)))]);
    let prims = [Prim {
        key: g,
        before: None,
        after: target.get(&g),
    }];
    assert_eq!(
        StepEffects::of(&st, &target, &prims).effects.flags,
        Flags::CONTENT
    );
    let saved = holding(st, &[(g, over(Center, on(&[0])))]);
    let p = path(&saved, &[(g, Some(over(TopLeft, Scope::All)))]);
    assert_eq!((p.flags, removed(&p)), (Flags::CONTENT, vec![]));
}

/// A header and footer replacing others' (§7.1's `replaces` row): on a
/// page of the saved file whose base page holds Acrobat's header, it takes
/// that off; on one with none it's drawn, and a step reads no page's
/// blocks. Taking it off the page by a filter, as RemoveHeaderFooter does,
/// removes too.
#[test]
fn a_header_replacing_others_takes_them_off() {
    use crate::materialize::header_tests::ACROBAT_HEADER;
    use crate::materialize::others_tests::{BODY, acrobat_file};
    let file = acrobat_file(&[&[BODY, ACROBAT_HEADER], &[BODY]]);
    let kind = crate::store::BlobKind::file(std::path::Path::new("acrobat.pdf"), None);
    let src = Store::new().open(file.into(), kind).unwrap();
    let st = state_of(&src, 2);
    let g = Key::Generator(GenId(1));
    let p = path(&st, &[(g, Some(header("Mine")))]);
    assert_eq!(p.flags, Flags::CONTENT | Flags::REMOVES);
    assert_eq!(removed(&p), [(RemovedKind::Filtered, 1)]);
    let target = putting(&st, &[(g, Some(header("Mine")))]);
    let prims = [Prim {
        key: g,
        before: None,
        after: target.get(&g),
    }];
    assert_eq!(
        StepEffects::of(&st, &target, &prims).effects.flags,
        Flags::CONTENT
    );
    let page = Key::Page(PageId(0));
    let acrobat_s = Filter::DropMarked(MarkedSel::Pagination {
        kinds: crate::generate::header::kinds(),
        origins: BTreeSet::from([crate::content::PagOrigin::Acrobat]),
    });
    let p = path(&st, &[(page, Some(with_filter(&st, 0, acrobat_s)))]);
    assert_eq!(p.flags, Flags::CONTENT | Flags::REMOVES);
    assert_eq!(removed(&p), [(RemovedKind::Filtered, 1)]);
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
    let wm = |text, position, z, scope| Some(watermark(wm_text(text, 72.0), position, z, scope));
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
                Some(with_layer(st, 0, Some((BlockId(2), layer("b"))))),
                Some(with_filter(
                    st,
                    0,
                    Filter::DropStreams(StreamSel::SheafGen(GenId(5))),
                )),
                Some(with_filter(st, 0, Filter::DropStreams(StreamSel::SheafOcr))),
                Some(with_filter(st, 0, Filter::DropMarked(MarkedSel::SheafOcr))),
                Some(with_filter(
                    st,
                    0,
                    Filter::DropMarked(MarkedSel::Pagination {
                        kinds: BTreeSet::from([crate::content::PaginationKind::Watermark]),
                        origins: BTreeSet::from([crate::content::PagOrigin::Acrobat]),
                    }),
                )),
                Some(with_filter(
                    st,
                    0,
                    Filter::StripLegacy(LegacySel::Marked(MarkedSel::Pagination {
                        kinds: BTreeSet::from([crate::content::PaginationKind::Watermark]),
                        origins: BTreeSet::from([crate::content::PagOrigin::SheafLegacy]),
                    })),
                )),
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
            vec![
                None,
                Some(header("a")),
                Some(header("b")),
                wm("a", GridPos::Center, Z::Over, Scope::All),
                wm("a", GridPos::Top, Z::Over, Scope::All),
                wm("a", GridPos::Center, Z::Over, on(&[1])),
                wm("b", GridPos::Center, Z::Under, Scope::All),
            ],
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
