//! Render keys and render reads (M10.5 §10.2, §6.3): what changes a page's
//! key under which inputs, what doesn't, and what a decision made from a
//! page's pixels reads.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

use im::Vector;
use in_progress::NoProgress;
use lopdf::{Object, dictionary};
use sheaf_edit::{
    AnnotRef, BatesSpec, Delta, HeaderFooterSpec, LabelStyle, MarkPatch, Op, Turn, Vec2,
};

use super::fixture::{dragging, every_input, hiding, layer, their_comment};
use super::*;
use crate::DocState;
use crate::forms::Value;
use crate::geom::{Geom, Rect};
use crate::model::history_tests::with_theirs;
use crate::model::{
    AnnotBody, AnnotRec, Document, FieldNode, Foreign, FormDoc, PageId, Snapshot, Terminal, Widget,
};
use crate::prim::{Key, Prim, Val};
use crate::source::ObjRef;
use crate::tx::fixture as fx;
use crate::tx::tests::{doc_of, opened};
use crate::tx::{Actor, Base, Commit, CommitOpts, EditError, JobKind, Stale};

/// Three pages: [`every_kind`](crate::forms::fixture::every_kind)'s fields
/// on the first two, and other tools' comments and a link on the first
/// and the third.
fn theirs() -> Document {
    let mut f = crate::forms::fixture::every_kind_in(3);
    with_theirs(&mut f, 0, "theirs-1");
    with_theirs(&mut f, 2, "theirs-3");
    opened(f.bytes(), "form.pdf")
}

fn key(st: &DocState, page: PageId, inputs: &ViewInputs) -> RenderKey {
    render_key(st, page, inputs).expect("a page of the state")
}

/// Every page's key under `inputs`.
fn keys(st: &DocState, inputs: &ViewInputs) -> HashMap<PageId, RenderKey> {
    (st.order().iter())
        .map(|&p| (p, key(st, p, inputs)))
        .collect()
}

/// An edit, named, made to a page of a document.
type Edit = (&'static str, fn(&Document, PageId) -> Op);

/// Every page's key under each of [`every_input`].
fn all_keys(d: &Document) -> [HashMap<PageId, RenderKey>; 5] {
    every_input().map(|inputs| keys(d.state(), &inputs))
}

/// Hiding another tool's comment changes the screen's key and no other
/// consumer's: print preview and OCR pass their own inputs, and those that
/// draw no comments are the same with it hidden.
#[test]
fn hiding_a_comment_changes_only_the_screen_s_key() {
    let d = theirs();
    let st = d.state();
    let p0 = st.order()[0];
    let comment = their_comment(st, p0);
    let screen = key(st, p0, &ViewInputs::screen());
    let hidden = hiding(&[comment], ViewInputs::screen());
    assert_ne!(key(st, p0, &hidden), screen);
    let print = key(st, p0, &ViewInputs::print(true));
    let clean = key(st, p0, &ViewInputs::print(false));
    assert_ne!(print, clean, "print preview draws their comments");
    let content = key(st, p0, &ViewInputs::content_only());
    for (inputs, was) in [
        (ViewInputs::print(false), clean),
        (ViewInputs::content_only(), content),
    ] {
        assert_eq!(key(st, p0, &hiding(&[comment], inputs)), was);
    }
    assert_eq!(key(st, p0, &ViewInputs::print(true)), print);
}

/// A duplicate shares its original's key, widgets and comments and all;
/// hiding the original's comment leaves the duplicate's copy of it shown.
#[test]
fn hiding_a_comment_leaves_its_duplicate_s_key_alone() {
    let mut d = theirs();
    let p0 = d.page(0).unwrap();
    d.commit_ops(vec![fx::duplicate(&[p0])], "Duplicate page 1")
        .unwrap();
    let copy = d.page(1).unwrap();
    let st = d.state();
    assert!(st.widgets_on(copy).next().is_some() && st.foreign_on(copy).next().is_some());
    for inputs in every_input() {
        assert_eq!(key(st, copy, &inputs), key(st, p0, &inputs), "{inputs:?}");
    }
    let screen = ViewInputs::screen();
    let (was, copy_was) = (key(st, p0, &screen), key(st, copy, &screen));
    let hidden = hiding(&[their_comment(st, p0)], screen);
    assert_ne!(key(st, p0, &hidden), was);
    assert_eq!(key(st, copy, &hidden), copy_was);
}

/// `st` with its pages in reverse order.
fn reversed(st: &DocState) -> DocState {
    let mut st = st.clone();
    let order: Vector<PageId> = st.order().iter().rev().copied().collect();
    let prim = Prim {
        key: Key::Order,
        before: Some(Val::Order(st.order().clone())),
        after: Some(Val::Order(order)),
    };
    st.apply(&prim).unwrap();
    st
}

/// No page's key reads where it is unless a generator's tokens do: 7,000
/// pages reversed under a header of fixed text keep every key.
#[test]
fn a_reorder_of_7000_pages_changes_no_key_without_live_tokens() {
    let mut d = doc_of(7000, &[]);
    let header = fx::header("Confidential <<date>> <<file>>");
    d.commit_ops(vec![fx::set_header(header, None)], "Header")
        .unwrap();
    let screen = ViewInputs::screen();
    let st = d.state();
    let without = ViewInputs {
        generators: false,
        ..screen.clone()
    };
    let p0 = st.order()[0];
    assert_ne!(key(st, p0, &screen), key(st, p0, &without));
    assert_eq!(keys(&reversed(st), &screen), keys(st, &screen));
}

/// "Page n of N" on the first 100 pages: moving the last page to the
/// front changes the keys of the covered pages, whose numbers it changes,
/// and no other; a delete changes every covered page's count and no
/// other page.
#[test]
fn with_page_n_of_n_a_move_changes_only_the_covered_pages_keys() {
    let mut d = doc_of(7000, &[]);
    let ids: Vec<PageId> = d.state().order().iter().copied().collect();
    let covered = &ids[..100];
    let header = fx::set_header(fx::header("Page <<1>> of <<N>>"), Some(covered));
    d.commit_ops(vec![header], "Header").unwrap();
    let screen = ViewInputs::screen();
    let before = keys(d.state(), &screen);
    let was = d.state().clone();
    d.commit_ops(vec![fx::move_pages(&ids[6999..], Some(ids[0]))], "Move")
        .unwrap();
    let st = d.state();
    let after = keys(st, &screen);
    for &id in &ids {
        let renumbered = covered.contains(&id) && was.pos_of(id) != st.pos_of(id);
        assert_eq!(after[&id] != before[&id], renumbered, "{id:?}");
    }
    let changed = ids.iter().filter(|id| after[id] != before[id]).count();
    assert_eq!(changed, 100);
    d.commit_ops(vec![fx::delete(&[ids[3000]])], "Delete")
        .unwrap();
    for (id, now) in keys(d.state(), &screen) {
        assert_eq!(now != after[&id], covered.contains(&id), "{id:?}");
    }
}

/// GPUI draws sheaf's marks on screen: adding, moving, restyling or
/// dragging one changes no screen key, and does change a plugin's, whose
/// render draws them.
#[test]
fn dragging_a_mark_under_by_host_changes_no_key() {
    let mut d = doc_of(2, &[]);
    let p = d.page(0).unwrap();
    let both = |d: &Document| {
        let st = d.state();
        (
            key(st, p, &ViewInputs::screen()),
            key(st, p, &ViewInputs::plugin()),
        )
    };
    let (host, mut plugin) = both(&d);
    let c = d
        .commit_ops(vec![fx::add(p, &crate::forms::fixture::note())], "Add")
        .unwrap();
    let mark = c.created.annots().next().unwrap();
    let moved = Op::MoveMarks {
        marks: vec![AnnotRef::Id(mark)],
        by: Delta::Page(Vec2 { x: 5.0, y: 5.0 }),
        to_page: None,
    };
    let restyled = Op::PatchMarks {
        marks: vec![AnnotRef::Id(mark)],
        patch: MarkPatch {
            color: Some([9, 9, 9]),
            ..MarkPatch::default()
        },
        imported: None,
    };
    let (now, added) = both(&d);
    assert_eq!(now, host, "Add");
    assert_ne!(added, plugin, "Add");
    plugin = added;
    for (op, label) in [(moved, "Move"), (restyled, "Restyle")] {
        d.commit_ops(vec![op], label).unwrap();
        let (now, then) = both(&d);
        assert_eq!(now, host, "{label}");
        assert_ne!(then, plugin, "{label}");
        plugin = then;
    }
    let st = d.state();
    assert_eq!(key(st, p, &dragging(&[mark], ViewInputs::screen())), host);
    assert_ne!(key(st, p, &dragging(&[mark], ViewInputs::plugin())), plugin);
}

/// A layer turned off is a view of its source's catalog: it changes the
/// keys of that source's pages, and an inserted file's keep theirs.
#[test]
fn toggling_a_layer_changes_the_keys_of_its_source_s_pages_only() {
    let mut d = doc_of(3, &[]);
    let other = crate::store::blank(d.store(), 2);
    d.commit_ops(vec![fx::insert(&other, [0, 1], "b.pdf", None)], "Insert")
        .unwrap();
    let st = d.state();
    let screen = ViewInputs::screen();
    let before = keys(st, &screen);
    for (source, layer) in [(d.primary().id(), (9, 0)), (other.id(), (4, 0))] {
        let off = OcgOverride {
            off: BTreeMap::from([(source, BTreeSet::from([LayerId::new(layer)]))]),
        };
        let toggled = ViewInputs {
            ocg_off: Some(Arc::new(off)),
            ..screen.clone()
        };
        for (id, def, _) in st.pages() {
            let changed = key(st, id, &toggled) != before[&id];
            assert_eq!(changed, def.base.src.id() == source, "{id:?}");
        }
    }
}

/// Each consumer's key is its own, on a page with nothing on it: what the
/// inputs draw says too what's drawn of what the base page has that no
/// record holds. Print preview with markup draws what the screen does.
#[test]
fn each_consumer_s_key_is_its_own() {
    let d = doc_of(1, &[]);
    let st = d.state();
    let p = st.order()[0];
    let [screen, plugin, print, clean, content] = every_input().map(|i| key(st, p, &i));
    assert_eq!(print, screen);
    let distinct: BTreeSet<RenderKey> = [screen, plugin, clean, content].into();
    assert_eq!(distinct.len(), 4);
}

/// A key is its base page's: blank pages alike but for which page of their
/// file each shows have keys of their own, and showing another file's
/// page changes one.
#[test]
fn pages_showing_other_pages_have_other_keys() {
    let mut d = doc_of(3, &[]);
    let other = crate::store::blank(d.store(), 1);
    let screen = ViewInputs::screen();
    let before = keys(d.state(), &screen);
    let distinct: BTreeSet<RenderKey> = before.values().copied().collect();
    assert_eq!(distinct.len(), 3);
    let p0 = d.page(0).unwrap();
    let replace = fx::replace(&[p0], &other, [0], "b.pdf");
    d.commit_ops(vec![replace], "Replace").unwrap();
    let st = d.state();
    assert_eq!(
        st.geom(p0).unwrap().media,
        Rect::new(0.0, 0.0, 612.0, 792.0)
    );
    assert_ne!(key(st, p0, &screen), before[&p0]);
}

/// A page whose filters differ draws differently, a filter that takes
/// ranges out from one that takes sheaf's private data out of the same
/// ranges' lists among them, so its key differs; the same filters again
/// give the same key.
#[test]
fn each_filter_is_its_page_s_key() {
    use crate::content::{PagOrigin, PaginationKind};
    use crate::model::{Filter, MarkedSel, PageDef, PrivateSel, StreamSel};
    let d = doc_of(1, &[]);
    let p0 = d.page(0).unwrap();
    let screen = ViewInputs::screen();
    let sel = MarkedSel::Pagination {
        kinds: BTreeSet::from([PaginationKind::Watermark]),
        origins: BTreeSet::from([PagOrigin::Acrobat]),
    };
    let filtered = |filter: Option<Filter>| {
        let def = d.state().def(p0).unwrap();
        let content = match filter {
            Some(f) => def.content.with_filter(f),
            None => def.content.clone(),
        };
        let def = PageDef::new(def.base.clone(), def.copy, content);
        key(
            &putting(d.state(), Key::Page(p0), Val::Page(Arc::new(def))),
            p0,
            &screen,
        )
    };
    let filters = [
        None,
        Some(Filter::DropMarked(sel.clone())),
        Some(Filter::StripPrivate(PrivateSel::Marked(sel.clone()))),
        Some(Filter::DropStreams(StreamSel::SheafGen(
            crate::content::GenId(1),
        ))),
        Some(Filter::DropMarked(MarkedSel::SheafGen(
            crate::content::GenId(1),
        ))),
        Some(Filter::StripPrivate(PrivateSel::Streams)),
        Some(Filter::StripPrivate(PrivateSel::NamedLists)),
    ];
    let keys: BTreeSet<RenderKey> = filters.iter().cloned().map(filtered).collect();
    assert_eq!(keys.len(), filters.len());
    let again = Some(Filter::StripPrivate(PrivateSel::Marked(sel)));
    assert_eq!(filtered(again.clone()), filtered(again));
}

/// `st` with `key` holding `val`.
fn putting(st: &DocState, key: Key, val: Val) -> DocState {
    let mut st = st.clone();
    let prim = Prim {
        key,
        before: st.get(&key),
        after: Some(val),
    };
    st.apply(&prim).unwrap();
    st
}

/// A page's turn is its render job's, so turning it changes no key, but
/// for a page an unsaved header is on, which is laid out in the page as
/// it shows; its boxes are its key's.
#[test]
fn a_turn_changes_only_a_header_s_pages_keys_and_a_box_its_page_s() {
    let mut d = doc_of(3, &[]);
    let [p0, p1, p2] = [0, 1, 2].map(|i| d.page(i).unwrap());
    let header = fx::set_header(fx::header("Draft"), Some(&[p1]));
    d.commit_ops(vec![header], "Header").unwrap();
    let screen = ViewInputs::screen();
    let before = keys(d.state(), &screen);
    d.commit_ops(vec![fx::rotate(&[p0, p1], Turn::Quarter)], "Turn")
        .unwrap();
    let st = d.state();
    let after = keys(st, &screen);
    assert_eq!(after[&p0], before[&p0]);
    assert_ne!(after[&p1], before[&p1]);
    assert_eq!(after[&p2], before[&p2]);
    let geom = Geom {
        crop: Rect::new(10.0, 10.0, 500.0, 700.0),
        ..**st.geom(p0).unwrap()
    };
    let cropped = putting(st, Key::Geom(p0), Val::Geom(Arc::new(geom)));
    assert_ne!(key(&cropped, p0, &screen), after[&p0]);
    assert_eq!(key(&cropped, p2, &screen), after[&p2]);
}

/// Bates numbers are fixed when the header's set: a move changes neither
/// a number nor the key of a page of a header of Bates numbers alone. A
/// header of page numbers or labels changes the keys of the pages whose
/// places a move changes, and one of the count none; new labels change the
/// keys of a header of labels' pages, and of no other's.
#[test]
fn a_move_changes_the_keys_of_pages_whose_live_tokens_it_changes() {
    let mut d = doc_of(6, &[]);
    let ids: Vec<PageId> = d.state().order().iter().copied().collect();
    let spec = HeaderFooterSpec {
        bates: Some(BatesSpec {
            start: 100,
            ..BatesSpec::default()
        }),
        ..fx::header("<<bates>>")
    };
    let covered = &ids[1..5];
    d.commit_ops(vec![fx::set_header(spec.clone(), Some(covered))], "Bates")
        .unwrap();
    let numbers = |st: &DocState| -> Vec<Option<u64>> {
        let g = st.header_footer().unwrap();
        ids.iter().map(|&id| g.frozen.bates_number(id)).collect()
    };
    let first = numbers(d.state());
    assert_eq!(
        first,
        [None, Some(100), Some(101), Some(102), Some(103), None]
    );
    let screen = ViewInputs::screen();
    let reads = render_reads(d.state(), ids[1], &screen);
    assert!(!reads.contains(&Key::Order) && !reads.contains(&Key::Labels));
    for (moved, before) in [(ids[0], None), (ids[4], Some(ids[2]))] {
        let was = keys(d.state(), &screen);
        d.commit_ops(vec![fx::move_pages(&[moved], before)], "Move")
            .unwrap();
        assert_eq!(keys(d.state(), &screen), was);
        assert_eq!(numbers(d.state()), first);
    }
    // Set again, it numbers the pages in their order now: the keys of the
    // pages whose numbers that changes change.
    let was = keys(d.state(), &screen);
    let covered = covered.to_vec();
    d.commit_ops(vec![fx::set_header(spec, Some(&covered))], "Bates")
        .unwrap();
    let now = keys(d.state(), &screen);
    let renumbered = numbers(d.state());
    assert_ne!(renumbered, first);
    for (i, id) in ids.iter().enumerate() {
        assert_eq!(now[id] != was[id], renumbered[i] != first[i], "{id:?}");
    }
    // Roman numbers from the first page, which relabel every page.
    let labels = |d: &Document| Op::SetPageLabels {
        ranges: Some(vec![fx::label(
            d.page(0).unwrap(),
            LabelStyle::LowerRoman,
            "",
            1,
        )]),
    };
    for (text, by_place) in [("<<1>>", true), ("<<label>>", true), ("of <<N>>", false)] {
        d.commit_ops(vec![fx::set_header(fx::header(text), None)], "Header")
            .unwrap();
        let (was, then) = (keys(d.state(), &screen), d.state().clone());
        d.commit_ops(vec![fx::move_pages(&[ids[3]], Some(ids[1]))], "Move")
            .unwrap();
        let st = d.state();
        let now = keys(st, &screen);
        for &id in &ids {
            let moved = by_place && then.pos_of(id) != st.pos_of(id);
            assert_eq!(now[&id] != was[&id], moved, "{text}: {id:?}");
        }
        let relabelled = text == "<<label>>";
        let reads = render_reads(d.state(), ids[0], &screen);
        assert_eq!(reads.contains(&Key::Labels), relabelled, "{text}");
        assert!(reads.contains(&Key::Order), "{text}");
        let was = keys(d.state(), &screen);
        d.commit_ops(vec![labels(&d)], "Labels").unwrap();
        let now = keys(d.state(), &screen);
        assert!(
            ids.iter().all(|id| (now[id] != was[id]) == relabelled),
            "{text}"
        );
        d.undo().unwrap();
    }
}

/// A record's change is its page's: another tool's comment drawn from
/// another object, a widget showing another state, and the form's own
/// keys, on the pages its widgets are on.
#[test]
fn records_change_the_keys_of_the_pages_drawing_them() {
    let d = theirs();
    let st = d.state();
    let [p0, p1, p2] = [0, 1, 2].map(|i| st.order()[i]);
    let screen = ViewInputs::screen();
    let before = keys(st, &screen);
    let comment = their_comment(st, p0);
    let f = st.annot(comment).unwrap().foreign().unwrap();
    let elsewhere = Foreign {
        src: f.src.as_ref().map(|r| ObjRef {
            src: r.src.clone(),
            id: (r.id.0 + 1, 0),
        }),
        ..Foreign::clone(f)
    };
    let (w, widget) = st.widgets_on(p0).next().unwrap();
    let switched = Widget {
        state: Some("Elsewhere".into()),
        ..Widget::clone(widget)
    };
    let needing = FormDoc {
        need_appearances: !st.form().need_appearances,
        ..FormDoc::clone(st.form())
    };
    let body = |body| Val::Annot(AnnotRec { page: p0, body });
    let changed = [
        (
            Key::Annot(comment),
            body(AnnotBody::Foreign(Arc::new(elsewhere))),
            vec![p0],
        ),
        (
            Key::Annot(w),
            body(AnnotBody::Widget(Arc::new(switched))),
            vec![p0],
        ),
        (Key::Form, Val::Form(Arc::new(needing)), vec![p0, p1]),
    ];
    for (slot, val, pages) in changed {
        let now = putting(st, slot, val);
        for p in [p0, p1, p2] {
            let differs = key(&now, p, &screen) != before[&p];
            assert_eq!(differs, pages.contains(&p), "{slot:?} on {p:?}");
            let content = ViewInputs::content_only();
            assert_eq!(key(&now, p, &content), key(st, p, &content));
        }
    }
}

/// What's drawn changes the screen's key, and only its page's: recognized
/// text, a header, another tool's comment deleted, a field's value.
#[test]
fn keys_change_with_ocr_text_a_header_a_deleted_comment_and_a_field_value() {
    let edits: [Edit; 3] = [
        ("Recognize text", |_, p| {
            fx::ocr([(p, layer("a"))]).remove(0)
        }),
        ("Header", |_, p| {
            fx::set_header(fx::header("Draft"), Some(&[p]))
        }),
        ("Delete comment", |d, p| {
            fx::delete_annots(&[their_comment(d.state(), p)])
        }),
    ];
    let screen = ViewInputs::screen();
    for (label, edit) in edits {
        let mut d = theirs();
        let [p0, p1] = [0, 1].map(|i| d.page(i).unwrap());
        let (was, other) = (key(d.state(), p0, &screen), key(d.state(), p1, &screen));
        let op = edit(&d, p0);
        d.commit_ops(vec![op], label).unwrap();
        assert_ne!(key(d.state(), p0, &screen), was, "{label}");
        assert_eq!(key(d.state(), p1, &screen), other, "{label}");
    }
    // The header's text changed, its generator kept.
    let mut d = theirs();
    let [p0, p1] = [0, 1].map(|i| d.page(i).unwrap());
    let header = |text| fx::set_header(fx::header(text), Some(&[p0]));
    d.commit_ops(vec![header("Draft")], "Header").unwrap();
    let g: Vec<_> = d.state().generators().keys().copied().collect();
    let (was, other) = (key(d.state(), p0, &screen), key(d.state(), p1, &screen));
    d.commit_ops(vec![header("Final")], "Header").unwrap();
    let st = d.state();
    assert!(st.generators().keys().copied().eq(g));
    assert_ne!(key(st, p0, &screen), was);
    assert_eq!(key(st, p1, &screen), other);
    // No op fills a field yet: its slot is put as a fill would put it.
    let d = theirs();
    let mut st = d.state().clone();
    let p0 = st.order()[0];
    let field = st.widgets_on(p0).next().unwrap().1.field;
    let node = st.field(field).unwrap().clone();
    let term = Terminal {
        value: Some(Value::Text("Bo".into())),
        ..Terminal::clone(node.term.as_ref().unwrap())
    };
    let filled = FieldNode {
        term: Some(Arc::new(term)),
        ..FieldNode::clone(&node)
    };
    let was = key(&st, p0, &screen);
    let content = key(&st, p0, &ViewInputs::content_only());
    let prim = Prim {
        key: Key::Field(field),
        before: Some(Val::Field(node)),
        after: Some(Val::Field(Arc::new(filled))),
    };
    st.apply(&prim).unwrap();
    assert_ne!(key(&st, p0, &screen), was);
    assert_eq!(key(&st, p0, &ViewInputs::content_only()), content);
}

/// Flattened, a page draws its annotations as its own content, whichever
/// the inputs draw: its key changes under every one, and another page's
/// doesn't.
#[test]
fn flattening_changes_a_page_s_key_under_every_input() {
    let mut d = theirs();
    let [p0, p1] = [0, 1].map(|i| d.page(i).unwrap());
    let was = all_keys(&d);
    let op = fx::flatten(&[p0], sheaf_edit::FlattenSet::All);
    d.commit_ops(vec![op], "Flatten").unwrap();
    for (inputs, was) in every_input().iter().zip(&was) {
        let st = d.state();
        assert_ne!(key(st, p0, inputs), was[&p0], "{inputs:?}");
        assert_eq!(key(st, p1, inputs), was[&p1], "{inputs:?}");
    }
}

/// A header its page's file draws already, as its blocks laid out alike
/// there say, isn't drawn again: that page's key reads no position. Moved
/// where its tokens read otherwise, it's drawn anew, and its key is a
/// drawn page's.
#[test]
fn a_header_its_base_draws_reads_no_position() {
    for (text, moved) in [("Draft", false), ("Page <<1>>", true)] {
        let mut d = doc_of(3, &[]);
        let p2 = d.page(2).unwrap();
        d.commit_ops(
            vec![fx::set_header(fx::header(text), Some(&[p2]))],
            "Header",
        )
        .unwrap();
        let bytes = crate::materialize::How::Rewritten.write(&d);
        let mut d = opened(bytes, "drawn.pdf");
        let [p0, p1, p2] = [0, 1, 2].map(|i| d.page(i).unwrap());
        let st = d.state();
        let g = st.header_footer().unwrap();
        assert_eq!([p0, p1, p2].map(|p| g.covers(p)), [false, false, true]);
        let screen = ViewInputs::screen();
        assert!(untouched(st, p2, &screen), "{text}");
        let before = keys(st, &screen);
        d.commit_ops(vec![fx::move_pages(&[p2], Some(p0))], "Move")
            .unwrap();
        let after = keys(d.state(), &screen);
        assert_eq!(after[&p2] != before[&p2], moved, "{text}");
        assert_eq!(!untouched(d.state(), p2, &screen), moved, "{text}");
        assert_eq!((after[&p0], after[&p1]), (before[&p0], before[&p1]));
    }
}

/// A save changes no key, an undo back to a value gives its keys back,
/// and a duplicate's is its original's.
#[test]
fn a_save_and_an_undo_keep_keys_and_a_duplicate_shares_its_original_s() {
    let mut d = theirs();
    let p0 = d.page(0).unwrap();
    let before = all_keys(&d);
    d.commit_ops(fx::ocr([(p0, layer("a"))]), "Recognize text")
        .unwrap();
    let recognized = all_keys(&d);
    assert_ne!(recognized, before);
    let v = d.current_version();
    d.save_input(v).unwrap();
    let stamp = crate::serialize::durable::FileStamp {
        len: d.primary().bytes().len() as u64,
        mtime: std::time::SystemTime::UNIX_EPOCH,
        tail: 0,
        window: 0,
    };
    let path: Arc<std::path::Path> = std::path::Path::new("form.pdf").into();
    let rec = crate::materialize::SaveRecord::opened(v, d.primary(), path, stamp);
    d.record_save(rec).unwrap();
    assert_eq!(all_keys(&d), recognized);
    d.undo().unwrap();
    assert_eq!(all_keys(&d), before);
    d.commit_ops(vec![fx::duplicate(&[p0])], "Duplicate")
        .unwrap();
    let copy = d.page(1).unwrap();
    let st = d.state();
    for inputs in every_input() {
        assert_eq!(key(st, copy, &inputs), key(st, p0, &inputs), "{inputs:?}");
    }
}

/// The decision reads a transaction on `d` records for what `inputs` draw
/// of `page`.
fn read(d: &Document, page: PageId, inputs: &ViewInputs) -> BTreeSet<Key> {
    let mut tx = d.tx(Actor::User);
    tx.read_render(page, inputs);
    tx.into_parts().decided.into_keys().collect()
}

/// Under `content_only` a render reads what the page shows alone; on
/// screen, the annotations it draws (not sheaf's marks, which GPUI draws),
/// their fields and the form, and the header, with the order its page
/// number reads; a plugin's reads the marks too. Never the boxes.
#[test]
fn read_render_records_what_the_inputs_draw() {
    let mut d = theirs();
    let p0 = d.page(0).unwrap();
    let ops = vec![
        fx::add(p0, &crate::forms::fixture::note()),
        fx::set_header(fx::header("Page <<1>>"), None),
    ];
    d.commit_ops(ops, "Mark and header").unwrap();
    assert_eq!(
        read(&d, p0, &ViewInputs::content_only()),
        BTreeSet::from([Key::Page(p0)])
    );
    let st = d.state();
    let g = *st.generators().keys().next().unwrap();
    let mut screen = BTreeSet::from([
        Key::Page(p0),
        Key::PageAnnots(p0),
        Key::Form,
        Key::Generator(g),
        Key::Order,
    ]);
    let mut marks = BTreeSet::new();
    for (id, rec) in st.annots_on(p0) {
        match &rec.body {
            AnnotBody::Mark(_) => _ = marks.insert(Key::Annot(id)),
            AnnotBody::Foreign(_) => _ = screen.insert(Key::Annot(id)),
            AnnotBody::Widget(w) => screen.extend([Key::Annot(id), Key::Field(w.field)]),
        }
    }
    assert_eq!(marks.len(), 1);
    assert_eq!(read(&d, p0, &ViewInputs::screen()), screen);
    let plugin: BTreeSet<Key> = screen.union(&marks).copied().collect();
    assert_eq!(read(&d, p0, &ViewInputs::plugin()), plugin);
    // What's hidden isn't drawn, so isn't read.
    let comment = their_comment(st, p0);
    let mut shown = screen.clone();
    shown.remove(&Key::Annot(comment));
    let hidden = hiding(&[comment], ViewInputs::screen());
    assert_eq!(read(&d, p0, &hidden), shown);
}

/// An OCR flush made on `snap`, reading what `read` showed as OCR read
/// them and putting recognized text on `write`, landed by value.
fn flush(
    d: &mut Document,
    snap: &Snapshot,
    read: &[PageId],
    write: PageId,
) -> Result<Commit, EditError> {
    let mut tx = Document::tx_at(snap, d.lower_cx(Actor::Job(JobKind::Ocr)));
    for &page in read {
        tx.read_render(page, &ViewInputs::content_only());
    }
    tx.apply_batch(fx::ocr([(write, layer("ocr"))]), &NoProgress)?;
    let opts = CommitOpts {
        base: Base::At(snap.version),
        stale: Stale::Validate,
        ..CommitOpts::labelled("Recognize text")
    };
    d.commit(tx, opts)
}

/// OCR's read of what its pages showed lands beside a mark, another
/// tool's comment deleted (no op adds one) or a header on them, none of
/// which it read; recognized text or a replace there conflicts, whether
/// the flush writes the page or only read it.
#[test]
fn an_ocr_flush_lands_beside_annotations_and_headers_and_not_a_changed_page() {
    let lands: [Edit; 3] = [
        ("Add", |_, p| fx::add(p, &crate::forms::fixture::note())),
        ("Delete comment", |d, p| {
            fx::delete_annots(&[their_comment(d.state(), p)])
        }),
        ("Header", |_, p| {
            fx::set_header(fx::header("Page <<1>>"), Some(&[p]))
        }),
    ];
    for (label, edit) in lands {
        for write_it in [true, false] {
            let mut d = theirs();
            let [p0, p1] = [0, 1].map(|i| d.page(i).unwrap());
            let snap = d.snapshot();
            let op = edit(&d, p0);
            d.commit_ops(vec![op], label).unwrap();
            let write = if write_it { p0 } else { p1 };
            if let Err(e) = flush(&mut d, &snap, &[p0, p1], write) {
                panic!("{label}, writing it: {write_it}: {e}");
            }
        }
    }
    for label in ["Recognize text", "Replace"] {
        for write_it in [true, false] {
            let mut d = theirs();
            let other = crate::store::blank(d.store(), 1);
            let [p0, p1] = [0, 1].map(|i| d.page(i).unwrap());
            let snap = d.snapshot();
            let op = match label {
                "Replace" => fx::replace(&[p0], &other, [0], "b.pdf"),
                _ => fx::ocr([(p0, layer("b"))]).remove(0),
            };
            d.commit_ops(vec![op], label).unwrap();
            let write = if write_it { p0 } else { p1 };
            let e = flush(&mut d, &snap, &[p0, p1], write).unwrap_err();
            let conflict = EditError::Conflict {
                key: Key::Page(p0),
                by: Some(label.into()),
            };
            assert_eq!(e, conflict, "{label}, writing it: {write_it}");
        }
    }
}

/// A decision made on `snap` from what `inputs` drew of `page`, adding a
/// note to page `on`, landed by value.
fn decide(
    d: &mut Document,
    snap: &Snapshot,
    page: PageId,
    inputs: &ViewInputs,
    on: PageId,
) -> Result<Commit, EditError> {
    let mut tx = Document::tx_at(snap, d.lower_cx(Actor::User));
    tx.read_render(page, inputs);
    tx.apply(fx::add(on, &crate::forms::fixture::note()), &NoProgress)?;
    let opts = CommitOpts {
        base: Base::At(snap.version),
        stale: Stale::Validate,
        ..CommitOpts::labelled("Decide")
    };
    d.commit(tx, opts)
}

/// Another tool's link drawn on a page reads the page it leads to: that
/// page replaced by another file's, which changes what it shows and not
/// the order, leaves the link leading nowhere, so undrawn, and conflicts
/// with a decision made from the link's page.
#[test]
fn a_link_s_page_reads_the_page_it_leads_to() {
    let mut f = crate::forms::fixture::every_kind_in(3);
    let last = f.pages[2];
    let link = f.pdf.add_object(dictionary! {
        "Type" => "Annot", "Subtype" => "Link",
        "Rect" => vec![200.into(), 200.into(), 300.into(), 220.into()],
        "Dest" => vec![last.into(), Object::Name(b"Fit".to_vec())],
        "Border" => vec![0.into(), 0.into(), 2.into()],
    });
    let mut annots = f.annots(0);
    annots.push(link);
    f.set_annots(0, &annots);
    let mut d = opened(f.bytes(), "form.pdf");
    let [p0, p1, p2] = [0, 1, 2].map(|i| d.page(i).unwrap());
    let screen = ViewInputs::screen();
    let reads = read(&d, p0, &screen);
    assert!(reads.contains(&Key::Page(p2)) && reads.contains(&Key::Order));
    assert!(!read(&d, p0, &ViewInputs::content_only()).contains(&Key::Page(p2)));
    let snap = d.snapshot();
    let other = crate::store::blank(d.store(), 1);
    let replace = fx::replace(&[p2], &other, [0], "b.pdf");
    d.commit_ops(vec![replace], "Replace").unwrap();
    assert_ne!(key(d.state(), p0, &screen), key(&snap.state, p0, &screen));
    let e = decide(&mut d, &snap, p0, &screen, p1).unwrap_err();
    let conflict = EditError::Conflict {
        key: Key::Page(p2),
        by: Some("Replace".into()),
    };
    assert_eq!(e, conflict);
}

/// A read with generators drawn reads every generator, so the header's
/// pages widened to take the page in conflicts with a decision made from
/// it without the header; under `content_only` it doesn't.
#[test]
fn a_header_coming_onto_a_page_conflicts_with_its_render_read() {
    let mut d = theirs();
    let [p0, p1, p2] = [0, 1, 2].map(|i| d.page(i).unwrap());
    let header = |pages: &[PageId]| fx::set_header(fx::header("Draft"), Some(pages));
    d.commit_ops(vec![header(&[p0])], "Header").unwrap();
    let g = *d.state().generators().keys().next().unwrap();
    assert!(read(&d, p1, &ViewInputs::plugin()).contains(&Key::Generator(g)));
    let snap = d.snapshot();
    d.commit_ops(vec![header(&[p0, p1])], "Widen").unwrap();
    let e = decide(&mut d, &snap, p1, &ViewInputs::plugin(), p2).unwrap_err();
    let conflict = EditError::Conflict {
        key: Key::Generator(g),
        by: Some("Widen".into()),
    };
    assert_eq!(e, conflict);
    decide(&mut d, &snap, p1, &ViewInputs::content_only(), p2).unwrap();
}

/// A watermark keys the pages it's on by what it draws and where, turned
/// as the page is; not the pages it's off, nor which its other pages are,
/// nor where they are. Under `content_only` it draws nothing.
#[test]
fn a_watermark_keys_the_pages_it_is_on() {
    use sheaf_edit::{GridPos, Z};
    let mut d = doc_of(3, &[]);
    let [p0, p1, p2] = [0, 1, 2].map(|i| d.page(i).unwrap());
    let screen = ViewInputs::screen();
    let content = ViewInputs::content_only();
    let was = |d: &Document, p| (key(d.state(), p, &screen), key(d.state(), p, &content));
    let [k0, k1, k2] = [p0, p1, p2].map(|p| was(&d, p));
    let set = |spec, pages: &[PageId]| fx::set_watermark(spec, Z::Over, Some(pages));
    d.commit_ops(vec![set(fx::watermark("A"), &[p0, p1])], "Watermark")
        .unwrap();
    let [m0, m1, m2] = [p0, p1, p2].map(|p| was(&d, p));
    assert!(m0.0 != k0.0 && m1.0 != k1.0);
    assert_eq!((m0.1, m1.1), (k0.1, k1.1));
    assert_eq!(m2, k2);
    // It reads nothing of where a page is: a reorder changes no key, and
    // its pages' reads don't hold the order.
    d.commit_ops(vec![fx::move_pages(&[p2], Some(p0))], "Move")
        .unwrap();
    assert_eq!([p0, p1, p2].map(|p| was(&d, p)), [m0, m1, m2]);
    let reads = render_reads(d.state(), p0, &screen);
    assert!(!reads.contains(&Key::Order), "{reads:?}");
    d.undo().unwrap();
    // On a page more: the pages it was on keep their keys.
    d.commit_ops(vec![set(fx::watermark("A"), &[p0, p1, p2])], "Widen")
        .unwrap();
    assert_eq!(was(&d, p0), m0);
    assert_ne!(was(&d, p2).0, k2.0);
    let moved = sheaf_edit::WatermarkSpec {
        position: GridPos::TopLeft,
        ..fx::watermark("A")
    };
    d.commit_ops(vec![set(moved, &[p0, p1, p2])], "Move")
        .unwrap();
    assert_ne!(was(&d, p0).0, m0.0);
    // Laid out on the page as it shows: a turn changes it.
    let before = was(&d, p1);
    d.commit_ops(vec![fx::rotate(&[p1], Turn::Quarter)], "Turn")
        .unwrap();
    assert_ne!(was(&d, p1).0, before.0);
    assert_eq!(was(&d, p1).1, before.1);
}

/// A header's layout on a page is laid out once for every version of the
/// document that lays the page out alike ([`content::header_digest`]),
/// and what's kept is what laying it out again gives, in every version:
/// its tokens read where a page is, the count, its label and its Bates
/// number, its spec changed under the same generator, its pages moved,
/// one deleted, relabelled, turned, and the edits undone.
///
/// [`content::header_digest`]: crate::materialize::emit::content::header_digest
#[test]
fn a_header_s_layouts_are_kept_as_laid_out_in_every_version() {
    use crate::generate;
    use crate::materialize::emit::content;

    let mut d = doc_of(40, &[]);
    let ids: Vec<PageId> = d.state().order().iter().copied().collect();
    let mut spec = fx::header("Page <<1>> of <<N>>");
    spec.texts[2] = "<<label>>".into();
    spec.texts[5] = "<<bates>>".into();
    spec.bates = Some(BatesSpec {
        start: 7,
        ..BatesSpec::default()
    });
    // Every page's layout in the current version, as kept and as laid
    // out afresh, which must agree; and how many were kept already.
    let check = |d: &Document, case: &str| {
        let st = d.state();
        let mut kept = 0;
        for (pos, &p) in st.order().iter().enumerate() {
            let geom = st.geom(p).unwrap();
            for g in st.generators().values().filter(|g| g.covers(p)) {
                let ctx = generate::page_ctx(st, g, p, pos, geom);
                let key = content::laid_key(g, p, &ctx);
                kept += usize::from(st.laid().get(&key).is_some());
                let fresh = generate::layout_digest(g, &ctx, None);
                let laid = content::model_layout_digest(st, g, p, geom);
                assert_eq!(laid, Some(fresh), "{case}: {p:?}");
                assert_eq!(st.laid().get(&key), Some(fresh), "{case}: {p:?}");
            }
        }
        kept
    };
    d.commit_ops(
        vec![fx::set_header(spec.clone(), Some(&ids[2..30]))],
        "Header",
    )
    .unwrap();
    let first = d.state().clone();
    assert_eq!(check(&d, "set"), 0);
    // Laid out again alike: every layout kept.
    assert_eq!(check(&d, "again"), 28);
    let kept: Vec<PageId> = (ids[2..30].iter())
        .copied()
        .filter(|&p| p != ids[10])
        .collect();
    let edits: Vec<(&str, Op)> = vec![
        ("moved", fx::move_pages(&ids[35..], Some(ids[0]))),
        ("deleted", fx::delete(&[ids[10]])),
        (
            "relabelled",
            Op::SetPageLabels {
                ranges: Some(vec![fx::label(ids[0], LabelStyle::UpperRoman, "A-", 3)]),
            },
        ),
        ("turned", fx::rotate(&[ids[4]], Turn::Quarter)),
        (
            "respecified",
            fx::set_header(
                HeaderFooterSpec {
                    first_number: 5,
                    ..spec.clone()
                },
                Some(&kept),
            ),
        ),
    ];
    for (case, op) in edits {
        d.commit_ops(vec![op], case).unwrap();
        check(&d, case);
    }
    // Every version shares the one memo, and the first's layouts are
    // kept still.
    assert!(std::ptr::eq(first.laid(), d.state().laid()));
    while d.undo().unwrap().is_some() {
        check(&d, "undone");
    }
}

/// A page an export keeps as the document numbers it (print's
/// `PageNumbering::AsDocument`) keys as it does in the document under
/// every input, its header's tokens reading where the document has it
/// ([`DocState::numbered_in`]); kept as the export numbers it (an
/// extract's), it keys as the second of two.
#[test]
fn a_page_kept_as_the_document_numbers_it_keys_as_in_the_document() {
    use crate::tx::ExportTx;
    use sheaf_edit::{OutlinePolicy, PageNumbering, PageSel, Transform};

    let mut d = doc_of(10, &[]);
    let ids: Vec<PageId> = d.state().order().iter().copied().collect();
    let mut spec = fx::header("Page <<1>> of <<N>>");
    spec.texts[2] = "<<label>>".into();
    let labels = Op::SetPageLabels {
        ranges: Some(vec![fx::label(ids[0], LabelStyle::UpperRoman, "", 1)]),
    };
    d.commit_ops(vec![fx::set_header(spec, None), labels], "Header")
        .unwrap();
    let kept = |numbering| {
        let mut xtx = ExportTx::new(&d.snapshot(), d.lower_cx(Actor::User));
        let keep = Transform::KeepPages {
            pages: PageSel::ids([ids[4], ids[5]]),
            outline: OutlinePolicy::Drop,
            labels: false,
            numbering,
        };
        xtx.apply(keep, &NoProgress).unwrap();
        xtx.state().clone()
    };
    let (printed, extracted) = (kept(PageNumbering::AsDocument), kept(PageNumbering::AsKept));
    for inputs in every_input() {
        for &p in &ids[4..6] {
            assert_eq!(
                key(&printed, p, &inputs),
                key(d.state(), p, &inputs),
                "{inputs:?}"
            );
        }
    }
    let print = ViewInputs::print(true);
    assert_ne!(
        key(&extracted, ids[5], &print),
        key(d.state(), ids[5], &print)
    );
}
