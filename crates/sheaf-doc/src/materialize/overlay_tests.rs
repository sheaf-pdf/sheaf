//! Overlays (M10.5 §10.1, slice 13): some pages of a state written as a
//! section over their source's own bytes, for a view's inputs. What the
//! emission holds: where its pages hang and what they inherit, the turn
//! they keep, what each view draws and hides, a duplicate's pages, the
//! form and the layers, and that its setup visits only its pages. The
//! realizer numbers, assembles and renders them.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::sync::Arc;

use in_progress::NoProgress;
use lopdf::{Dictionary, Object, ObjectId, dictionary};
use sheaf_edit::{Rotation, Turn};

use super::emit::{self, Emission, EmitBase, EmitScope, Overlay, RotatePolicy, SymObject};
use super::key::{ObjKey, Part};
use super::tests::{add, bytes, file, insert, open};
use crate::annot::AnnotId;
use crate::forms::fixture::{Form, every_kind_in, note};
use crate::model::history_tests::{with_ours, with_theirs};
use crate::model::visits;
use crate::model::{AnnotBody, AnnotRec, DocState, Document, Mark, PageId};
use crate::realize::fixture::{dragging, hiding};
use crate::realize::{MarkDraw, OcgOverride, ViewInputs};
use crate::store::{BlobId, Src};
use crate::tx::fixture as fx;
use crate::tx::tests::{doc_of, opened};

/// `pages` of `st` as an overlay over `src` writes them for `inputs`, with
/// `rotate` saying what `/Rotate` is: never naming what neither it nor the
/// base has.
fn emitted(
    st: &DocState,
    src: &Src,
    pages: &[PageId],
    inputs: &ViewInputs,
    rotate: RotatePolicy,
) -> Emission {
    let overlay = Overlay {
        src: src.clone(),
        inputs,
        rotate,
        arts: None,
    };
    let scope = EmitScope::Pages(pages.into());
    let e = emit::emit(st, scope, EmitBase::Overlay(&overlay), &[], &NoProgress).unwrap();
    names_only_what_it_has(&e, src.id());
    e
}

/// `pages` of `d` as an overlay over its primary writes them for
/// `inputs`.
fn overlay(d: &Document, pages: &[PageId], inputs: &ViewInputs) -> Emission {
    emitted(
        d.state(),
        d.primary(),
        pages,
        inputs,
        RotatePolicy::KeepBase,
    )
}

/// `d` as a Rewrite emits it.
fn rewritten(d: &Document) -> Emission {
    let snap = d.snapshot();
    let base = EmitBase::Rewrite {
        primary: &snap.primary,
        drawn: None,
        private: super::Private::Keep,
        unheld: super::Unheld::Keep,
    };
    emit::emit(&snap.state, EmitScope::All, base, &[], &NoProgress).unwrap()
}

/// Every key `e`, an overlay over `src`'s bytes, names is one it writes,
/// or copy 0 of an object of `src`, which its base has and it never
/// writes.
fn names_only_what_it_has(e: &Emission, src: BlobId) {
    let based = |key: &ObjKey| matches!(key, ObjKey::Base { blob, copy: 0, .. } if *blob == src);
    let written: HashSet<ObjKey> = e.objects.iter().map(|(key, _)| *key).collect();
    for key in e.syms.keys() {
        let had = based(key) || written.contains(key);
        assert!(had, "{key:?} is named, not written");
    }
    assert!(!written.iter().any(based), "it writes its base's objects");
}

/// What `e` writes under `key`.
fn object(e: &Emission, key: ObjKey) -> &SymObject {
    let found = e.objects.iter().find(|(k, _)| *k == key);
    &found.unwrap_or_else(|| panic!("{key:?} isn't written")).1
}

/// The dictionary `e` writes under `key`, made anew.
fn built(e: &Emission, key: ObjKey) -> &Dictionary {
    match object(e, key) {
        SymObject::Built(Object::Dictionary(d)) => d,
        SymObject::Raw(r) => panic!("{key:?} is the file's {:?}", r.id),
        _ => panic!("{key:?} isn't made anew"),
    }
}

fn writes(e: &Emission, key: ObjKey) -> bool {
    e.objects.iter().any(|(k, _)| *k == key)
}

/// The key placeholder `o` names.
fn named(e: &Emission, o: &Object) -> ObjKey {
    let at = o.as_reference().expect("a reference");
    e.syms.key(at).expect("a placeholder")
}

/// The keys page `page`'s `/Annots` names, in order.
fn listed(e: &Emission, page: PageId) -> Vec<ObjKey> {
    let annots = built(e, ObjKey::Page(page)).get(b"Annots").ok();
    let annots = annots.map(|a| a.as_array().unwrap().as_slice());
    annots
        .unwrap_or_default()
        .iter()
        .map(|a| named(e, a))
        .collect()
}

/// What each entry of page `page`'s `/Annots` is, in order: one of
/// sheaf's marks, another tool's comment or link, a widget, or what the
/// records don't hold.
fn kinds(e: &Emission, st: &DocState, page: PageId) -> Vec<&'static str> {
    let kind = |key: ObjKey| match key {
        ObjKey::Annot(a) => match st.annot(a).map(|rec| &rec.body) {
            Some(AnnotBody::Mark(_)) => "mark",
            Some(AnnotBody::Foreign(_)) => "theirs",
            Some(AnnotBody::Widget(_)) => "widget",
            None => "none",
        },
        ObjKey::Field(_) => "widget",
        ObjKey::AnnotPart(_, Part::Companion) => "mark",
        _ => "unmodeled",
    };
    listed(e, page).into_iter().map(kind).collect()
}

/// `/F`, as written in `dict`.
fn flags(dict: &Dictionary) -> i64 {
    dict.get(b"F").map_or(0, |f| f.as_i64().unwrap())
}

/// [`every_kind_in`]'s fields on the first two of `pages` pages, and
/// other tools' comments and a link on the first.
fn theirs(pages: usize) -> Form {
    let mut f = every_kind_in(pages);
    with_theirs(&mut f, 0, "theirs");
    f
}

/// An overlay's pages hang under one new root that lists only them, in
/// its order, each written at its own object's number with what it
/// inherited written out; the catalog is the source's over that root, at
/// the catalog's number. Nothing else is written: the rest is the base's.
#[test]
fn an_overlay_page_hangs_under_the_group_root_with_its_inherited_keys() {
    let d = open(&bytes(&file("A", 3, false)), None);
    let src = d.primary();
    let pdf = src.pdf();
    let [p0, _, p2] = [0, 1, 2].map(|i| d.page(i).unwrap());
    let e = overlay(&d, &[p2, p0], &ViewInputs::screen());
    let object_of = |i: u32| src.page(i).unwrap().id;
    assert_eq!(e.owned.get(&ObjKey::Page(p2)), Some(&object_of(2)));
    assert_eq!(e.owned.get(&ObjKey::Page(p0)), Some(&object_of(0)));
    let root = built(&e, ObjKey::PagesNode(0));
    let kids: Vec<ObjKey> = (root.get(b"Kids").unwrap().as_array().unwrap().iter())
        .map(|k| named(&e, k))
        .collect();
    assert_eq!(kids, [ObjKey::Page(p2), ObjKey::Page(p0)]);
    assert_eq!(root.get(b"Count").unwrap(), &Object::Integer(2));
    assert!(!root.has(b"Parent"));
    // The node above it gave it its resources and media box.
    let node = (pdf.catalog().unwrap().get(b"Pages").unwrap())
        .as_reference()
        .unwrap();
    let resources = (pdf.get_dictionary(node).unwrap().get(b"Resources").unwrap())
        .as_reference()
        .unwrap();
    for p in [p0, p2] {
        let page = built(&e, ObjKey::Page(p));
        assert_eq!(
            named(&e, page.get(b"Parent").unwrap()),
            ObjKey::PagesNode(0)
        );
        let base = ObjKey::Base {
            blob: src.id(),
            obj: resources,
            copy: 0,
        };
        assert_eq!(named(&e, page.get(b"Resources").unwrap()), base);
        let media: Vec<Object> = vec![0.into(), 0.into(), 612.into(), 792.into()];
        assert_eq!(page.get(b"MediaBox").unwrap(), &Object::Array(media));
        assert_eq!(page.get(b"Rotate").unwrap(), &Object::Integer(0));
        assert!(!page.has(b"CropBox"));
    }
    let catalog = built(&e, ObjKey::Catalog);
    let at = pdf.trailer().get(b"Root").unwrap().as_reference().unwrap();
    assert_eq!(e.owned.get(&ObjKey::Catalog), Some(&at));
    assert_eq!(
        named(&e, catalog.get(b"Pages").unwrap()),
        ObjKey::PagesNode(0)
    );
    let metadata = named(&e, catalog.get(b"Metadata").unwrap());
    assert!(matches!(metadata, ObjKey::Base { copy: 0, .. }));
    let keys: BTreeSet<ObjKey> = e.objects.iter().map(|(key, _)| *key).collect();
    let want = [
        ObjKey::Catalog,
        ObjKey::PagesNode(0),
        ObjKey::Page(p0),
        ObjKey::Page(p2),
    ];
    assert_eq!(keys, BTreeSet::from(want));
}

/// An overlay keeps its base page's `/Rotate`, inherited or its own, as
/// its file says it, and the render turns the rest, so a page turned is
/// turned once. A save writes the state's turn, and so does an overlay
/// under the model's policy.
#[test]
fn keep_base_keeps_the_base_rotate_after_a_turn() {
    let mut pdf = file("A", 2, false);
    let pages: Vec<ObjectId> = pdf.get_pages().into_values().collect();
    let node = (pdf
        .get_dictionary(pages[0])
        .unwrap()
        .get(b"Parent")
        .unwrap())
    .as_reference()
    .unwrap();
    pdf.get_dictionary_mut(node).unwrap().set("Rotate", 90);
    pdf.get_dictionary_mut(pages[1]).unwrap().set("Rotate", -90);
    let mut d = open(&bytes(&pdf), None);
    let ids = [d.page(0).unwrap(), d.page(1).unwrap()];
    d.commit_ops(vec![fx::rotate(&ids, Turn::Quarter)], "turn")
        .unwrap();
    let st = d.state();
    let rotate = |e: &Emission, p| built(e, ObjKey::Page(p)).get(b"Rotate").unwrap().clone();
    let screen = ViewInputs::screen();
    let kept = emitted(st, d.primary(), &ids, &screen, RotatePolicy::KeepBase);
    assert_eq!(rotate(&kept, ids[0]), Object::Integer(90));
    assert_eq!(rotate(&kept, ids[1]), Object::Integer(-90));
    for p in ids {
        let written = Rotation::from_degrees(rotate(&kept, p).as_i64().unwrap() as i32);
        let turned = written.plus(st.render_delta(p).unwrap());
        assert_eq!(turned, st.geom(p).unwrap().rotate, "{p:?}");
    }
    let model = emitted(st, d.primary(), &ids, &screen, RotatePolicy::Model);
    let rewrite = rewritten(&d);
    for (p, want) in [(ids[0], 180), (ids[1], 0)] {
        assert_eq!(rotate(&model, p), Object::Integer(want));
        assert_eq!(rotate(&rewrite, p), Object::Integer(want));
    }
}

/// The screen draws sheaf's marks itself, so its overlay leaves them out,
/// those the file holds as well as one made since; a plugin's has PDFium
/// draw them. A view that draws no annotations writes only the widgets,
/// which its render's flags then leave undrawn or draw: not others'
/// comments, nor what else of theirs the records don't hold (a screen).
#[test]
fn by_host_leaves_sheaf_marks_out_and_by_engine_draws_them() {
    let mut f = theirs(2);
    with_ours(&mut f);
    let screen = f.pdf.add_object(dictionary! {
        "Type" => "Annot", "Subtype" => "Screen",
        "Rect" => vec![300.into(), 300.into(), 400.into(), 400.into()],
    });
    // Their reply to one of sheaf's: not threaded where that isn't drawn.
    let square = (f.annots(0).into_iter())
        .find(|&a| f.dict(a).get(b"NM").ok() == Some(&Object::string_literal("ours-square")))
        .unwrap();
    let reply = f.pdf.add_object(dictionary! {
        "Type" => "Annot", "Subtype" => "Text", "IRT" => square,
        "Rect" => vec![10.into(), 10.into(), 30.into(), 30.into()],
        "NM" => Object::string_literal("their-reply"),
    });
    let mut annots = f.annots(0);
    annots.extend([screen, reply]);
    f.set_annots(0, &annots);
    let mut d = opened(f.bytes(), "form.pdf");
    let p0 = d.page(0).unwrap();
    d.commit_ops(vec![fx::add(p0, &note())], "Add").unwrap();
    let st = d.state();
    let count = |kinds: &[&str], kind: &str| kinds.iter().filter(|k| **k == kind).count();
    let on = |test: fn(&AnnotRec) -> bool| st.annots_on(p0).filter(|(_, r)| test(r)).count();
    let marks = on(|r| r.mark().is_some());
    let widgets = on(|r| r.widget().is_some());
    assert!(marks >= 4, "the file's three, and one made since");

    let by_host = overlay(&d, &[p0], &ViewInputs::screen());
    let shown = kinds(&by_host, st, p0);
    assert_eq!(count(&shown, "mark"), 0, "{shown:?}");
    assert_eq!(count(&shown, "widget"), widgets);
    assert_eq!(count(&shown, "theirs"), 5, "{shown:?}");
    assert_eq!(count(&shown, "unmodeled"), 1, "{shown:?}");
    for (a, _) in st.annots_on(p0).filter(|(_, r)| r.mark().is_some()) {
        assert!(!writes(&by_host, ObjKey::Annot(a)), "{a:?}");
    }
    let (reply, _) = (st.foreign_on(p0))
        .find(|(_, f)| f.summary.name == "their-reply")
        .unwrap();
    assert!(!built(&by_host, ObjKey::Annot(reply)).has(b"IRT"));

    let by_engine = overlay(&d, &[p0], &ViewInputs::plugin());
    let drawn = kinds(&by_engine, st, p0);
    assert!(count(&drawn, "mark") >= marks, "{drawn:?}");
    assert_eq!(count(&drawn, "theirs"), 5);
    assert_eq!(count(&drawn, "widget"), widgets);
    assert_eq!(count(&drawn, "unmodeled"), 1);

    for inputs in [ViewInputs::content_only(), ViewInputs::print(false)] {
        let e = overlay(&d, &[p0], &inputs);
        assert_eq!(kinds(&e, st, p0), vec!["widget"; widgets], "{inputs:?}");
    }
    // Print preview with markup draws theirs, and leaves sheaf's to GPUI.
    let print = kinds(&overlay(&d, &[p0], &ViewInputs::print(true)), st, p0);
    assert_eq!((count(&print, "mark"), count(&print, "theirs")), (0, 5));
}

/// A comment the view hides is written under its own key, at its own
/// number, flagged hidden, where its file's object would otherwise do; so
/// is a widget, and a mark PDFium draws, hidden or being dragged. A mark
/// the screen draws itself isn't written, dragged or not.
#[test]
fn a_hidden_comment_carries_the_hidden_flag_under_its_own_key() {
    let mut f = theirs(2);
    with_ours(&mut f);
    let mut d = opened(f.bytes(), "form.pdf");
    let p0 = d.page(0).unwrap();
    d.commit_ops(vec![fx::add(p0, &note())], "Add").unwrap();
    let st = d.state();
    let (comment, caret) = (st.foreign_on(p0))
        .find(|(_, f)| f.summary.subtype == "Caret")
        .unwrap();
    let at = caret.src.as_ref().unwrap().id;
    let key = ObjKey::Annot(comment);
    let shown = overlay(&d, &[p0], &ViewInputs::screen());
    assert!(matches!(object(&shown, key), SymObject::Raw(r) if r.id == at));
    let hidden = overlay(&d, &[p0], &hiding(&[comment], ViewInputs::screen()));
    assert_eq!(hidden.owned.get(&key), Some(&at));
    assert_eq!(flags(built(&hidden, key)), 2);
    assert!(listed(&hidden, p0).contains(&key));
    // Only it.
    let others = (st.foreign_on(p0)).filter(|(a, _)| *a != comment);
    for (a, _) in others {
        assert!(matches!(
            object(&hidden, ObjKey::Annot(a)),
            SymObject::Raw(_)
        ));
    }

    // As the file has it, when it's shown.
    let flags_of = |e: &Emission, key| match object(e, key) {
        SymObject::Raw(r) => flags(r.src.pdf().get_dictionary(r.id).unwrap()),
        _ => flags(built(e, key)),
    };
    // A widget merged with its field, its field's object, and one of a
    // radio group's: flagged.
    let merged = (st.widgets_on(p0)).find(|(_, w)| w.merged).unwrap();
    let apart = (st.widgets_on(p0)).find(|(_, w)| !w.merged).unwrap();
    for (widget, key) in [
        (merged.0, ObjKey::Field(merged.1.field)),
        (apart.0, ObjKey::Annot(apart.0)),
    ] {
        let was = flags_of(&shown, key);
        let hidden = overlay(&d, &[p0], &hiding(&[widget], ViewInputs::screen()));
        assert_eq!(flags(built(&hidden, key)), was | 2, "{key:?}");
    }

    // sheaf's own, drawn by PDFium, the file's and one made since: hidden
    // or dragged alike.
    let marks = st.annots_on(p0).filter(|(_, r)| r.mark().is_some());
    let marks: Vec<AnnotId> = marks.map(|(a, _)| a).collect();
    let read = |a: &AnnotId| st.annot(*a).unwrap().mark_record().unwrap().src.is_some();
    assert!(marks.iter().any(read) && !marks.iter().all(read));
    let drawn = overlay(&d, &[p0], &ViewInputs::plugin());
    for mark in marks {
        let key = ObjKey::Annot(mark);
        let was = flags_of(&drawn, key);
        assert_eq!(was & 2, 0);
        for inputs in [
            hiding(&[mark], ViewInputs::plugin()),
            dragging(&[mark], ViewInputs::plugin()),
        ] {
            let e = overlay(&d, &[p0], &inputs);
            assert_eq!(flags(built(&e, key)), was | 2, "{key:?}");
        }
        let by_host = overlay(&d, &[p0], &dragging(&[mark], ViewInputs::screen()));
        assert!(!writes(&by_host, key));
    }
}

/// What `e` writes page `page`'s `/Contents` as: each stream's key.
fn contents(e: &Emission, page: PageId) -> Vec<ObjKey> {
    match built(e, ObjKey::Page(page)).get(b"Contents").unwrap() {
        Object::Array(items) => items.iter().map(|o| named(e, o)).collect(),
        one => vec![named(e, one)],
    }
}

/// The header and footer and recognized text are drawn over a page as a
/// save draws them, the same streams (its position, the page count and
/// its Bates number found from the state's tables, not a walk of the
/// pages), and only when the view asks: what OCR reads is the page's own
/// content alone.
#[test]
fn the_header_and_recognized_text_are_drawn_as_saved_when_the_view_asks() {
    let mut d = open(&bytes(&file("A", 3, false)), None);
    let [p0, p1, p2] = [0, 1, 2].map(|i| d.page(i).unwrap());
    let bates = sheaf_edit::BatesSpec {
        prefix: "X".into(),
        suffix: String::new(),
        digits: 4,
        start: 7,
    };
    let header = sheaf_edit::HeaderFooterSpec {
        bates: Some(bates),
        ..fx::header("Page <<1>> of <<N>> <<bates>>")
    };
    let word = crate::text_layer::OcrWord {
        text: "word".into(),
        rect: crate::annot::Box2 {
            x: 10.0,
            y: 10.0,
            w: 30.0,
            h: 10.0,
        },
        confidence: 0.9,
    };
    let layer = Arc::new(crate::text_layer::TextLayer {
        words: vec![word],
        lines: std::iter::once(0..1).collect(),
        upright: Rotation::R0,
        engine: "test".into(),
    });
    let mut ops = vec![fx::set_header(header, Some(&[p0, p2]))];
    ops.extend(fx::ocr([(p2, layer)]));
    d.commit_ops(ops, "header and text").unwrap();
    let saved = rewritten(&d);
    let screen = overlay(&d, &[p2], &ViewInputs::screen());
    assert_eq!(contents(&screen, p2), contents(&saved, p2));
    assert!(
        contents(&screen, p2).len() > 3,
        "{:?}",
        contents(&screen, p2)
    );
    let read = d
        .primary()
        .pdf()
        .get_dictionary(d.primary().page(2).unwrap().id);
    let own = read
        .unwrap()
        .get(b"Contents")
        .unwrap()
        .as_reference()
        .unwrap();
    let own = ObjKey::Base {
        blob: d.primary().id(),
        obj: own,
        copy: 0,
    };
    let content = overlay(&d, &[p2], &ViewInputs::content_only());
    assert_eq!(contents(&content, p2), [own]);
    // Text alone, or the header alone.
    let only = |generators, ocr_over| ViewInputs {
        generators,
        ocr_over,
        ..ViewInputs::screen()
    };
    let text = contents(&overlay(&d, &[p2], &only(false, true)), p2);
    let head = contents(&overlay(&d, &[p2], &only(true, false)), p2);
    assert!(text.len() > 1 && head.len() > 1 && text != head);
    // A page it doesn't cover has none.
    let p1_saved = contents(&saved, p1);
    assert_eq!(
        contents(&overlay(&d, &[p1], &ViewInputs::screen()), p1),
        p1_saved
    );
}

/// A duplicate in its original's overlay is a page object of its own
/// beside the original's, which keeps its number; they share their content
/// and resources, and its annotations and fields are its own.
#[test]
fn a_duplicate_is_a_new_page_object_in_its_original_s_scope() {
    let mut f = every_kind_in(2);
    let link = with_theirs(&mut f, 0, "theirs");
    let to = f.pages[1];
    f.dict(link).set("Dest", vec![to.into(), "Fit".into()]);
    let mut d = opened(f.bytes(), "form.pdf");
    let p0 = d.page(0).unwrap();
    d.commit_ops(vec![fx::duplicate(&[p0])], "Duplicate page 1")
        .unwrap();
    let copy = d.page(1).unwrap();
    let st = d.state();
    let src = d.primary();
    let e = overlay(&d, &[p0, copy], &ViewInputs::plugin());
    assert_eq!(
        e.owned.get(&ObjKey::Page(p0)),
        Some(&src.page(0).unwrap().id)
    );
    assert!(!e.owned.contains_key(&ObjKey::Page(copy)));
    let root = built(&e, ObjKey::PagesNode(0));
    let kids: Vec<ObjKey> = (root.get(b"Kids").unwrap().as_array().unwrap().iter())
        .map(|k| named(&e, k))
        .collect();
    assert_eq!(kids, [ObjKey::Page(p0), ObjKey::Page(copy)]);
    let (page, dup) = (built(&e, ObjKey::Page(p0)), built(&e, ObjKey::Page(copy)));
    for key in [b"Contents".as_slice(), b"Resources"] {
        let (a, b) = (page.get(key).unwrap(), dup.get(key).unwrap());
        assert_eq!(a, b, "{}", String::from_utf8_lossy(key));
    }
    assert_eq!(named(&e, dup.get(b"Parent").unwrap()), ObjKey::PagesNode(0));
    // Its link leads nowhere in the overlay, whose pages are these two:
    // page 2 is in another's.
    let (link, _) = st
        .foreign_on(copy)
        .find(|(_, f)| f.summary.is_link())
        .unwrap();
    let dest = built(&e, ObjKey::Annot(link)).get(b"Dest").unwrap();
    assert_eq!(dest.as_array().unwrap()[0], Object::Null);
    // With page 2 in the overlay, the copy's link leads there: to the one
    // copy of it there is.
    let p1 = d.page(2).unwrap();
    let all = overlay(&d, &[p0, copy, p1], &ViewInputs::plugin());
    let dest = built(&all, ObjKey::Annot(link)).get(b"Dest").unwrap();
    assert_eq!(named(&all, &dest.as_array().unwrap()[0]), ObjKey::Page(p1));
    // Its comments and widgets: new objects, on it where they say a page.
    let keys = listed(&e, copy);
    for &key in &keys {
        assert!(!e.owned.contains_key(&key), "{key:?}");
        if let Ok(p) = built(&e, key).get(b"P") {
            assert_eq!(named(&e, p), ObjKey::Page(copy));
        }
    }
    let originals = listed(&e, p0);
    assert_eq!(keys.len(), originals.len());
    assert!(keys.iter().all(|k| !originals.contains(k)));
    // The form lists both instances.
    let fields = built(&e, ObjKey::AcroForm).get(b"Fields").unwrap();
    let roots: Vec<ObjKey> = fields
        .as_array()
        .unwrap()
        .iter()
        .map(|f| named(&e, f))
        .collect();
    let tops = |p| {
        (st.widgets_on(p))
            .map(|(_, w)| top(st, w.field))
            .collect::<BTreeSet<_>>()
    };
    let want: BTreeSet<ObjKey> = (tops(p0).into_iter().chain(tops(copy)))
        .map(ObjKey::Field)
        .collect();
    assert_eq!(roots.into_iter().collect::<BTreeSet<_>>(), want);
}

/// A link to a page the document no longer has is left out, as a save
/// leaves it out, though the base still has the page.
#[test]
fn a_link_to_a_page_gone_is_left_out_as_a_save_leaves_it() {
    let mut f = every_kind_in(2);
    let link = with_theirs(&mut f, 0, "theirs");
    let to = f.pages[1];
    f.dict(link).set("Dest", vec![to.into(), "Fit".into()]);
    let mut d = opened(f.bytes(), "form.pdf");
    let [p0, p1] = [0, 1].map(|i| d.page(i).unwrap());
    let st = d.state();
    let (link, _) = st
        .foreign_on(p0)
        .find(|(_, f)| f.summary.is_link())
        .unwrap();
    let key = ObjKey::Annot(link);
    assert!(listed(&overlay(&d, &[p0], &ViewInputs::screen()), p0).contains(&key));
    d.commit_ops(vec![fx::delete(&[p1])], "delete").unwrap();
    let e = overlay(&d, &[p0], &ViewInputs::screen());
    assert!(!listed(&e, p0).contains(&key) && !writes(&e, key));
}

/// The root of the field tree field `fid` is in.
fn top(st: &DocState, fid: crate::forms::FieldId) -> crate::forms::FieldId {
    let mut at = fid;
    while let Some(up) = st.field(at).and_then(|n| n.parent) {
        at = up;
    }
    at
}

/// A text field `name` with a widget on each of `pages` of `f`.
fn spanning(f: &mut Form, name: &str, pages: [usize; 2]) {
    let field = f.pdf.add_object(dictionary! {
        "FT" => "Tx", "T" => Object::string_literal(name),
        "DA" => Object::string_literal("/Helv 10 Tf 0 g"),
    });
    let mut kids: Vec<Object> = Vec::new();
    for page in pages {
        let rect: Vec<Object> = (crate::forms::fixture::row(4).iter())
            .map(|&v| Object::from(v))
            .collect();
        let widget = f.pdf.add_object(dictionary! {
            "Type" => "Annot", "Subtype" => "Widget", "Rect" => rect, "F" => 4,
            "P" => f.pages[page], "Parent" => field,
        });
        let mut annots = f.annots(page);
        annots.push(widget);
        f.set_annots(page, &annots);
        kids.push(widget.into());
    }
    f.dict(field).set("Kids", kids);
    let mut fields = f.fields();
    fields.push(field);
    f.set_fields(&fields);
}

/// An overlay's form is the source's form dictionary at its number, of
/// only the fields with a widget on the overlay's pages: `/Fields` their
/// roots, each node's `/Kids` those of its nodes and widgets that are, so
/// a tree under one root (every kind's `form1[0]`) is cut down to them,
/// and a field with widgets on two pages lists only the page's. An overlay
/// of a page with none has no form.
#[test]
fn the_group_form_lists_only_fields_with_a_widget_on_its_pages() {
    let mut top_level = Form::new(3);
    for (name, page, row) in [("a", 0, 0), ("b", 1, 0), ("c", 1, 1)] {
        top_level.text(name, page, crate::forms::fixture::row(row));
    }
    spanning(&mut top_level, "both", [0, 1]);
    for mut f in [every_kind_in(3), top_level] {
        let d = opened(f.bytes(), "form.pdf");
        let st = d.state();
        let src = d.primary();
        let form_at = (src.pdf().catalog().unwrap().get(b"AcroForm").unwrap())
            .as_reference()
            .unwrap();
        let [p0, p1, p2] = [0, 1, 2].map(|i| d.page(i).unwrap());
        for p in [p0, p1] {
            let e = overlay(&d, &[p], &ViewInputs::screen());
            assert_eq!(e.owned.get(&ObjKey::AcroForm), Some(&form_at));
            let catalog = built(&e, ObjKey::Catalog);
            let form = named(&e, catalog.get(b"AcroForm").unwrap());
            assert_eq!(form, ObjKey::AcroForm);
            // The nodes written: those with a widget on the page, and the
            // nodes above them.
            let on: BTreeSet<_> = st.widgets_on(p).map(|(_, w)| w.field).collect();
            let mut nodes = BTreeSet::new();
            for &f in &on {
                let mut at = Some(f);
                while let Some(fid) = at {
                    nodes.insert(ObjKey::Field(fid));
                    at = st.field(fid).and_then(|n| n.parent);
                }
            }
            let written: BTreeSet<ObjKey> = (e.objects.iter())
                .map(|(key, _)| *key)
                .filter(|key| matches!(key, ObjKey::Field(_)))
                .collect();
            assert_eq!(written, nodes, "{p:?}");
            // Each lists only those, and only the page's widgets.
            for (key, obj) in &e.objects {
                let (ObjKey::Field(_), SymObject::Built(Object::Dictionary(d))) = (key, obj) else {
                    continue;
                };
                let kids = (d.get(b"Kids").ok()).map(|k| k.as_array().unwrap().as_slice());
                for kid in kids.unwrap_or_default().iter().map(|k| named(&e, k)) {
                    match kid {
                        ObjKey::Field(_) => assert!(nodes.contains(&kid), "{key:?}: {kid:?}"),
                        ObjKey::Annot(w) => assert_eq!(st.annot(w).unwrap().page, p),
                        _ => panic!("{key:?} lists {kid:?}"),
                    }
                }
            }
            let tops: BTreeSet<ObjKey> = on.iter().map(|&f| ObjKey::Field(top(st, f))).collect();
            match object(&e, ObjKey::AcroForm) {
                // Every root is the page's: the file's dictionary as it is,
                // its one root cut down.
                SymObject::Raw(_) => {
                    let all = st.form().roots.iter().copied().map(ObjKey::Field);
                    assert_eq!(all.collect::<BTreeSet<_>>(), tops);
                    assert!(
                        tops.iter()
                            .all(|&t| matches!(object(&e, t), SymObject::Built(_)))
                    );
                }
                _ => {
                    let form = built(&e, ObjKey::AcroForm);
                    let fields = form.get(b"Fields").unwrap().as_array().unwrap();
                    let fields: BTreeSet<ObjKey> = fields.iter().map(|f| named(&e, f)).collect();
                    assert_eq!(fields, tops, "{p:?}");
                }
            }
        }
        let e = overlay(&d, &[p2], &ViewInputs::screen());
        assert!(!writes(&e, ObjKey::AcroForm));
        assert!(!built(&e, ObjKey::Catalog).has(b"AcroForm"));
        assert!(
            e.objects
                .iter()
                .all(|(key, _)| !matches!(key, ObjKey::Field(_)))
        );
    }
}

/// A form inserted from a file whose form's defaults (`/DA`, `/Q`) aren't
/// the document's has them pushed down onto its roots in an overlay over
/// that file, as a save pushes them down, so its fields draw alike; the
/// file the document opened gets none pushed, its defaults the form's.
#[test]
fn an_inserted_form_s_defaults_are_pushed_down_as_a_save_pushes_them() {
    let mut d = opened(every_kind_in(2).bytes(), "form.pdf");
    let mut other = Form::new(1);
    let plain = other.text("plain", 0, crate::forms::fixture::row(0));
    other.dict(plain).remove(b"DA");
    let acroform = other.acroform;
    other
        .dict(acroform)
        .set("DA", Object::string_literal("/Helv 9 Tf 1 0 0 rg"));
    other.dict(acroform).set("Q", 1);
    let other = add(&d, &other.bytes(), "other.pdf");
    insert(&mut d, &other, 0);
    let st = d.state();
    let page = *st.order().last().unwrap();
    let rewrite = rewritten(&d);
    let screen = ViewInputs::screen();
    let e = emitted(st, &other.src, &[page], &screen, RotatePolicy::KeepBase);
    let roots = |src: BlobId| -> Vec<crate::forms::FieldId> {
        (st.form().roots.iter().copied())
            .filter(|&f| st.field(f).unwrap().inst.src == src)
            .collect()
    };
    let theirs = roots(other.src.id());
    assert_eq!(theirs.len(), 1);
    let defaults = |e: &Emission, f| {
        let field = built(e, ObjKey::Field(f));
        (
            field.get(b"DA").ok().cloned(),
            field.get(b"Q").ok().cloned(),
        )
    };
    let pushed = defaults(&rewrite, theirs[0]);
    assert_eq!(
        pushed,
        (
            Some(Object::string_literal("/Helv 9 Tf 1 0 0 rg")),
            Some(Object::Integer(1))
        )
    );
    assert_eq!(defaults(&e, theirs[0]), pushed);
    let p0 = d.page(0).unwrap();
    let own = overlay(&d, &[p0], &screen);
    for f in roots(d.primary().id()) {
        if let SymObject::Built(Object::Dictionary(field)) = object(&own, ObjKey::Field(f)) {
            assert!(!field.has(b"Q"), "{f:?}");
        }
    }
}

/// A file of one page with three layers, the second off, its default
/// configuration an object of its own: the bytes and the layers.
fn layered() -> (Vec<u8>, [ObjectId; 3]) {
    let mut pdf = file("A", 1, false);
    let group = |pdf: &mut lopdf::Document, name: &str| {
        pdf.add_object(dictionary! { "Type" => "OCG", "Name" => Object::string_literal(name) })
    };
    let ids = [
        group(&mut pdf, "Grid"),
        group(&mut pdf, "Notes"),
        group(&mut pdf, "Orphan"),
    ];
    let config = pdf.add_object(dictionary! {
        "OFF" => vec![ids[1].into()],
        "Order" => ids.iter().map(|&id| id.into()).collect::<Vec<Object>>(),
    });
    let props = dictionary! {
        "OCGs" => ids.iter().map(|&id| id.into()).collect::<Vec<Object>>(),
        "D" => config,
    };
    let root = pdf.trailer.get(b"Root").unwrap().as_reference().unwrap();
    pdf.get_dictionary_mut(root)
        .unwrap()
        .set("OCProperties", props);
    (bytes(&pdf), ids)
}

/// The layers a view turns off are a patch of the source's catalog: its
/// `/OCProperties`, whose default configuration shows every layer but
/// those, the file's own order kept; an overlay's catalog says it.
#[test]
fn the_ocg_patch_turns_the_layer_off_in_the_default_config() {
    let (bytes, [grid, notes, orphan]) = layered();
    let src = crate::forms::fixture::source(bytes.clone());
    let off = BTreeSet::from([grid, orphan]);
    let patch = crate::layers::ocg_patch(&src, &off).unwrap();
    let patched = patch
        .as_dict()
        .unwrap()
        .get(b"D")
        .unwrap()
        .as_dict()
        .unwrap();
    let listed = |key: &[u8]| -> Vec<ObjectId> {
        (patched.get(key).unwrap().as_array().unwrap().iter())
            .map(|o| o.as_reference().unwrap())
            .collect()
    };
    let on = Object::Name(b"ON".to_vec());
    assert_eq!(patched.get(b"BaseState").unwrap(), &on);
    assert_eq!(
        (listed(b"ON"), listed(b"OFF")),
        (vec![notes], vec![grid, orphan])
    );
    assert_eq!(listed(b"Order"), [grid, notes, orphan]);
    // Put in the catalog, it reads as those layers off.
    let mut pdf = lopdf::Document::load_mem(&bytes).unwrap();
    let root = pdf.trailer.get(b"Root").unwrap().as_reference().unwrap();
    pdf.get_dictionary_mut(root)
        .unwrap()
        .set("OCProperties", patch.clone());
    let on: BTreeMap<ObjectId, bool> = (crate::layers::layers_in(&pdf).into_iter())
        .filter_map(|l| Some((l.id?, l.on)))
        .collect();
    assert_eq!(
        on,
        BTreeMap::from([(grid, false), (notes, true), (orphan, false)])
    );
    assert_eq!(crate::layers::ocg_patch(&file_source(), &off), None);

    // An overlay's catalog says it for its source, and only its source.
    let d = open(&bytes, None);
    let p0 = d.page(0).unwrap();
    let ocg = |blob| OcgOverride {
        off: BTreeMap::from([(blob, off.clone())]),
    };
    let view = |ocg: OcgOverride| ViewInputs {
        ocg_off: Some(Arc::new(ocg)),
        ..ViewInputs::screen()
    };
    let e = overlay(&d, &[p0], &view(ocg(d.primary().id())));
    let catalog = built(&e, ObjKey::Catalog);
    let props = catalog.get(b"OCProperties").unwrap().as_dict().unwrap();
    let config = props.get(b"D").unwrap().as_dict().unwrap();
    let base = |obj| ObjKey::Base {
        blob: d.primary().id(),
        obj,
        copy: 0,
    };
    let refs = |key: &[u8]| -> Vec<ObjKey> {
        (config.get(key).unwrap().as_array().unwrap().iter())
            .map(|o| named(&e, o))
            .collect()
    };
    assert_eq!(refs(b"OFF"), [base(grid), base(orphan)]);
    assert_eq!(refs(b"ON"), [base(notes)]);
    let other = BlobId { hash: 1, len: 1 };
    let e = overlay(&d, &[p0], &view(ocg(other)));
    let catalog = built(&e, ObjKey::Catalog);
    let read = d
        .primary()
        .pdf()
        .catalog()
        .unwrap()
        .get(b"OCProperties")
        .unwrap();
    let kept = catalog.get(b"OCProperties").unwrap().as_dict().unwrap();
    assert_eq!(named(&e, kept.get(b"D").unwrap()), base(config_of(read)));
}

/// A file with no layers.
fn file_source() -> Src {
    crate::forms::fixture::source(bytes(&file("B", 1, false)))
}

/// The default configuration `props`, a file's `/OCProperties`, names.
fn config_of(props: &Object) -> ObjectId {
    let props = props.as_dict().unwrap();
    props.get(b"D").unwrap().as_reference().unwrap()
}

/// An overlay's setup visits only its pages, their annotations and the
/// fields with a widget on them: of 7,000 pages with 20,000 marks, one
/// page and its own, whether its marks are written or not. The state's
/// tables (where pages are, which page shows a source's page) are built
/// once a state, as every reader's are, before it's watched.
#[test]
fn an_overlay_visits_only_its_scope() {
    let d = doc_of(7_000, &[]);
    let st = d.state();
    let total = 20_000;
    let marks = |pos: usize| (pos * total / 7_000)..((pos + 1) * total / 7_000);
    let annots = (st.order().iter().enumerate()).map(|(pos, &p)| {
        let on = marks(pos).map(|i| {
            let annot = crate::annot::Annot {
                id: AnnotId(i as u64 + 1),
                ..note()
            };
            (annot.id, AnnotBody::Mark(Arc::new(Mark::from(annot))))
        });
        (p, on.collect())
    });
    let pages = st
        .pages()
        .map(|(p, def, geom)| (p, def.clone(), geom.clone()));
    let st = DocState::builder().pages(pages).annots(annots).build();
    assert_eq!(st.annot_count(), total);
    let pos = 3_500;
    let p = st.order()[pos];
    let def = st.def(p).unwrap();
    let object = def.base.src.page(def.base.index).unwrap().id;
    assert_eq!(st.page_showing(def.base.src.id(), object, 0), Some(p));
    assert_eq!(st.pos_of(p), Some(pos));
    let own: BTreeSet<AnnotId> = marks(pos).map(|i| AnnotId(i as u64 + 1)).collect();
    assert!(own.len() >= 2);
    for inputs in [ViewInputs::screen(), ViewInputs::plugin()] {
        let (e, seen) =
            visits::watching(|| emitted(&st, d.primary(), &[p], &inputs, RotatePolicy::KeepBase));
        assert_eq!(seen.pages, BTreeSet::from([p]), "{inputs:?}");
        assert_eq!(seen.annots, own, "{inputs:?}");
        assert!(seen.fields.is_empty());
        let drawn = inputs.marks == MarkDraw::ByEngine;
        assert_eq!(listed(&e, p).len(), if drawn { own.len() } else { 0 });
    }
}
