//! Marks and generators in two fonts (M10 slice 7): text their standard
//! 14 face lacks drawn in the fallback font sheaf bundles, through the
//! emitter. A note, a text stamp, a header, a watermark and a flattened
//! mark each draw it, one subset a save, nothing said; what no font has
//! still draws `?`, said once. Pages alike share one subset and one
//! dictionary of resources; an Append that draws a new glyph writes a
//! subset and resources of its own for the page it draws on and leaves
//! another page's as they were, and one that draws no new glyph writes no
//! font; an overlay's pages, emitted apart, each bind their own subset.
//! Every file is read back through `subsets_hold`: each code a glyph of
//! the subset its font is, with its text.

use std::collections::BTreeSet;
use std::sync::Arc;

use crate::pdf::{Document as Pdf, Object, ObjectId};
use in_progress::NoProgress;
use sheaf_edit::{AnnotRef, Delta, FlattenSet, LabelStyle, Op, Rotation, Vec2, Z};

use super::append_tests::{appended_in_place, keys_at, written};
use super::emit::{self, Emission, EmitBase, EmitScope, Overlay, RotatePolicy, SymObject};
use super::key::{ObjKey, Part};
use super::recognized_tests::{drawn, pages, stream};
use super::removal_tests::{opened, scratch, text_file};
use super::{How, save_rewritten};
use crate::annot::{Annot, AnnotId, AnnotKind, Box2};
use crate::content::fixture::{fallback_subsets, listed_on_a_page, subsets_hold};
use crate::content::{Fallback, Glyphs};
use crate::model::{Document, PageId};
use crate::realize::{Realizer, ViewInputs};
use crate::tx::Notice;
use crate::tx::fixture as fx;

/// A mark of `kind` saying `contents`, as an op makes one.
fn mark(kind: AnnotKind, contents: &str) -> Annot {
    Annot {
        id: AnnotId(0),
        kind,
        color: [200, 30, 30],
        opacity: 1.0,
        stroke: 1.0,
        author: "Greg".into(),
        contents: contents.into(),
        modified: "D:20261124000000".into(),
        created: String::new(),
        fill: None,
        dashed: false,
        cloudy: false,
        turn: Rotation::R0,
        name: String::new(),
    }
}

/// A text box saying `text` at height `y`.
fn note(text: &str, y: f32) -> Annot {
    let rect = Box2 {
        x: 72.0,
        y,
        w: 200.0,
        h: 30.0,
    };
    mark(AnnotKind::FreeText { rect, size: 12.0 }, text)
}

/// A text stamp saying `text` at height `y`.
fn text_stamp(text: &str, y: f32) -> Annot {
    let rect = Box2 {
        x: 300.0,
        y,
        w: 160.0,
        h: 40.0,
    };
    let kind = AnnotKind::TextStamp {
        rect,
        text: text.into(),
        status: false,
    };
    mark(kind, "")
}

/// Pages `0..n` of `doc`.
fn pages_of<const N: usize>(doc: &Document) -> [PageId; N] {
    std::array::from_fn(|i| doc.page(i).unwrap())
}

/// The glyphs of `chars` in the fallback, by number.
fn glyphs_of(chars: &str) -> BTreeSet<u16> {
    let program = Fallback::SANS.program();
    (chars.chars())
        .map(|c| program.glyph(c).expect("a character the fallback draws"))
        .collect()
}

/// The glyph numbers `glyphs` keeps.
fn numbers(glyphs: &Glyphs) -> BTreeSet<u16> {
    glyphs.iter().map(|(glyph, _)| glyph).collect()
}

/// The object page `i` of `bytes` takes its resources from.
fn resources_at(bytes: &[u8], i: usize) -> ObjectId {
    let pdf = Pdf::load_mem(bytes).unwrap();
    let page = pages(&pdf)[i];
    let resources = pdf.get_dictionary(page).unwrap().get(b"Resources");
    resources
        .and_then(Object::as_reference)
        .expect("resources of its own")
}

/// Every stream of the annotations' normal appearances on `pdf`'s pages,
/// decoded, and each page's content: what's drawn, to read its codes.
fn everything_drawn(pdf: &Pdf) -> Vec<String> {
    let mut out = Vec::new();
    for page in pages(pdf) {
        out.push(drawn(pdf, page));
        let annots = pdf.get_dictionary(page).unwrap().get(b"Annots");
        let annots = annots.and_then(Object::as_array).map(<[Object]>::to_vec);
        for annot in annots.unwrap_or_default() {
            let annot = pdf.get_dictionary(annot.as_reference().unwrap()).unwrap();
            let ap = annot.get(b"AP").and_then(Object::as_dict);
            if let Some(n) = ap.and_then(|ap| ap.get(b"N")?.as_reference()) {
                out.push(String::from_utf8_lossy(&stream(pdf, n).1).into_owned());
            }
        }
    }
    out
}

/// A note, a text stamp, a header, a watermark and a flattened note each
/// saying a name WinAnsi hasn't all of draw it in the fallback, nothing
/// said: one subset, every code one of its glyphs with its text, no `?`
/// drawn. Then `漢`, which no font sheaf bundles has, draws `?`, said once,
/// beside an `Ł` in the fallback. Both ways.
fn marks_and_generators_in_two_fonts(how: How) {
    let dir = scratch();
    let (path, mut doc) = opened(&dir, "a.pdf", &text_file(2));
    let [p0, p1] = pages_of(&doc);
    let ops = vec![
        fx::add(p0, &note("\u{141}ukasz", 600.0)),
        fx::add(p0, &text_stamp("\u{141}ukasz", 500.0)),
        fx::add(p1, &note("\u{141}\u{f3}d\u{17a}", 600.0)),
        fx::set_header(fx::header("\u{141}ukasz"), None),
        fx::set_watermark(fx::watermark("\u{141}ukasz"), Z::Over, None),
    ];
    let c = doc.commit_ops(ops, "two fonts").unwrap();
    assert_eq!(c.notices, [], "{how:?}");
    let c = doc
        .commit_ops(vec![fx::flatten(&[p1], FlattenSet::All)], "flatten")
        .unwrap();
    assert_eq!(c.notices, [], "{how:?}");
    how.save(&mut doc, &path);
    let bytes = std::fs::read(&path).unwrap();
    // The header's and the watermark's Ł on each page, the note's and the
    // stamp's on the first, the flattened note's Ł and ź on the second.
    assert_eq!(subsets_hold(&bytes), 2 + 2 + 1 + 1 + 2, "{how:?}");
    let subsets = fallback_subsets(&bytes);
    let [(_, held)] = &subsets[..] else {
        panic!("{how:?}: one subset, not {subsets:?}")
    };
    assert_eq!(*held, glyphs_of("\u{141}\u{17a}"), "{how:?}");
    let pdf = Pdf::load_mem(&bytes).unwrap();
    for drawn in everything_drawn(&pdf) {
        assert!(!drawn.contains('?'), "{how:?}: {drawn}");
    }

    let c = doc
        .commit_ops(vec![fx::add(p1, &note("\u{141} \u{6f22}", 400.0))], "cjk")
        .unwrap();
    assert_eq!(c.notices, [Notice::Unencodable], "{how:?}");
    let c = doc
        .commit_ops(vec![fx::add(p1, &note("\u{6f22}", 300.0))], "cjk")
        .unwrap();
    assert_eq!(c.notices, [], "{how:?}: told once");
    how.save(&mut doc, &path);
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(subsets_hold(&bytes), 8 + 1, "{how:?}: one more Ł");
    let pdf = Pdf::load_mem(&bytes).unwrap();
    let drawn = everything_drawn(&pdf);
    let questions = drawn.iter().filter(|d| d.contains("(?) Tj")).count();
    assert_eq!(questions, 1, "{how:?}: {drawn:?}");
    let both = drawn
        .iter()
        .find(|d| d.contains("( ?) Tj"))
        .expect("Ł then ?");
    assert!(both.contains("> Tj"), "{how:?}: Ł in the fallback: {both}");
}

crate::both_ways!(
    marks_and_generators_draw_in_two_fonts,
    marks_and_generators_draw_in_two_fonts_appended: marks_and_generators_in_two_fonts
);

/// A header on three pages alike is one subset and one dictionary of
/// resources, which all three pages name: their resources' key says the
/// subset they bind, the same. Both ways.
#[test]
fn a_header_on_pages_alike_is_one_subset_and_one_dictionary() {
    let dir = scratch();
    let (_path, mut doc) = opened(&dir, "a.pdf", &text_file(3));
    let header = fx::set_header(fx::header("\u{141}ukasz <<1>>"), None);
    doc.commit_ops(vec![header], "header").unwrap();
    for how in [How::Rewritten, How::Appended] {
        let bytes = how.write(&doc);
        assert_eq!(subsets_hold(&bytes), 3, "{how:?}");
        assert_eq!(fallback_subsets(&bytes).len(), 1, "{how:?}");
        let resources: BTreeSet<ObjectId> = (0..3).map(|i| resources_at(&bytes, i)).collect();
        assert_eq!(resources.len(), 1, "{how:?}: one dictionary");
    }
}

/// An Append that changes the header to a name with a glyph no saved
/// subset has writes a subset of its own and new resources for the page
/// it redraws, which bind it; a page it doesn't touch keeps its resources
/// and the subset they bind; each code of both pages is a glyph of its
/// font's subset. The record says both subsets.
#[test]
fn an_append_drawing_a_new_glyph_writes_a_subset_and_resources_of_its_own() {
    let dir = scratch();
    let (path, mut doc) = opened(&dir, "a.pdf", &text_file(2));
    let [p0, p1] = pages_of(&doc);
    let ops = vec![
        fx::set_header(fx::header("\u{141}ukasz"), Some(&[p0])),
        fx::set_watermark(fx::watermark("\u{141}\u{f3}d\u{17a}"), Z::Over, Some(&[p1])),
    ];
    doc.commit_ops(ops, "set").unwrap();
    save_rewritten(&mut doc, &path).unwrap();
    let was = std::fs::read(&path).unwrap();
    let (r0, r1) = (resources_at(&was, 0), resources_at(&was, 1));
    assert_ne!(r0, r1);
    let [(s1, g1)] = &doc.saved_record().unwrap().fallbacks[..] else {
        panic!("the Rewrite's subset")
    };
    let s1 = *s1;
    assert_eq!(numbers(g1), glyphs_of("\u{141}\u{17a}"));
    assert_eq!(subsets_hold(&was), 3);

    let header = fx::set_header(fx::header("\u{141}ukasz Nguy\u{1ec5}n"), Some(&[p0]));
    doc.commit_ops(vec![header], "header").unwrap();
    let (base, section) = appended_in_place(&mut doc).unwrap();
    let now = std::fs::read(&path).unwrap();
    let record = doc.saved_record().unwrap();
    let [(first, _), (s2, g2)] = &record.fallbacks[..] else {
        panic!("two subsets")
    };
    assert_eq!(*first, s1);
    assert_eq!(numbers(g2), glyphs_of("\u{141}\u{1ec5}"));
    let s2_at = record.map.keys[&ObjKey::Res(*s2)];
    let wrote = written(base.len(), &section, &path);
    assert!(wrote.contains(&s2_at), "{wrote:?}");
    let keys = keys_at(&doc, &wrote);
    let page_res: Vec<&ObjKey> = keys
        .iter()
        .filter(|k| matches!(k, ObjKey::PageRes { .. }))
        .collect();
    assert_eq!(page_res.len(), 1, "{keys:?}");
    assert_ne!(resources_at(&now, 0), r0, "the redrawn page's own");
    assert!(wrote.contains(&resources_at(&now, 0)));
    assert_eq!(resources_at(&now, 1), r1, "the other page's as they were");
    assert!(!wrote.contains(&r1));
    assert_eq!(fallback_subsets(&now).len(), 2);
    // Ł and ễ on the first page, Ł and ź on the second.
    assert_eq!(subsets_hold(&now), 4);
}

/// An Append that moves a note on a page whose header is in the fallback
/// draws its glyphs in the saved subset, which has them: it writes the
/// note and its appearance, no font and no page resources.
#[test]
fn an_append_drawing_no_new_glyph_writes_no_font() {
    let dir = scratch();
    let (path, mut doc) = opened(&dir, "a.pdf", &text_file(1));
    let [p0] = pages_of(&doc);
    let ops = vec![
        fx::set_header(fx::header("\u{141}ukasz <<1>>"), None),
        fx::add(p0, &note("\u{141}u", 600.0)),
    ];
    doc.commit_ops(ops, "set").unwrap();
    save_rewritten(&mut doc, &path).unwrap();
    let subsets = fallback_subsets(&std::fs::read(&path).unwrap());
    assert_eq!(subsets.len(), 1);
    let a = doc.state().marks_on(p0).next().unwrap().id;
    let by = Op::MoveMarks {
        marks: vec![AnnotRef::Id(a)],
        by: Delta::Page(Vec2 { x: 10.0, y: 0.0 }),
        to_page: None,
    };
    doc.commit_ops(vec![by], "move").unwrap();
    let (base, section) = appended_in_place(&mut doc).unwrap();
    let wrote = written(base.len(), &section, &path);
    let keys = keys_at(&doc, &wrote);
    assert!(keys.contains(&ObjKey::AnnotPart(a, Part::Ap)), "{keys:?}");
    let fonts_or_resources = (keys.iter()).filter(|k| {
        matches!(
            k,
            ObjKey::Res(_) | ObjKey::ResPart(..) | ObjKey::PageRes { .. }
        )
    });
    assert_eq!(fonts_or_resources.count(), 0, "{keys:?}");
    let now = std::fs::read(&path).unwrap();
    assert_eq!(fallback_subsets(&now), subsets);
    assert_eq!(doc.saved_record().unwrap().fallbacks.len(), 1);
    assert_eq!(subsets_hold(&now), 2);
}

/// Page `page` of `d` as an overlay emits it under `scope`, for a view
/// that draws marks and generators as PDFium would.
fn overlaid(d: &Document, page: PageId, scope: EmitScope) -> Emission {
    let st = d.state();
    let inputs = ViewInputs::plugin();
    let overlay = Overlay {
        src: st.def(page).unwrap().base.src.clone(),
        inputs: &inputs,
        rotate: RotatePolicy::KeepBase,
        arts: None,
    };
    emit::emit(st, scope, EmitBase::Overlay(&overlay), &[], &NoProgress).unwrap()
}

/// The page resources `e` writes, by key.
fn page_res(e: &Emission) -> BTreeSet<ObjKey> {
    (e.objects.iter())
        .map(|(key, _)| *key)
        .filter(|key| matches!(key, ObjKey::PageRes { .. }))
        .collect()
}

/// The keys of the fonts what `e` writes under `key` binds.
fn fonts_bound(e: &Emission, key: ObjKey) -> BTreeSet<ObjKey> {
    let (_, object) = (e.objects.iter().find(|(k, _)| *k == key))
        .unwrap_or_else(|| panic!("{key:?} isn't written"));
    let dict = match object {
        SymObject::Built(o) => match o.as_stream() {
            Some(s) => s.dict.get(b"Resources").and_then(Object::as_dict).cloned(),
            None => o.as_dict().cloned(),
        },
        _ => panic!("{key:?} isn't built"),
    };
    let fonts = dict.and_then(|d| d.get(b"Font").and_then(Object::as_dict).cloned());
    (fonts.unwrap_or_default().iter())
        .filter_map(|(_, v)| e.syms.key(v.as_reference()?))
        .collect()
}

/// Two pages of an overlay whose header and marks draw other glyphs in
/// the fallback, each emitted apart: each page binds the subset of what it
/// draws, its resources keyed by it, so neither takes the other's however
/// the realizer puts them together; and the realized file draws each code
/// a glyph of its font's subset.
#[test]
fn an_overlay_s_pages_each_bind_their_own_subset() {
    let dir = scratch();
    let (_path, mut doc) = opened(&dir, "a.pdf", &text_file(2));
    let [p0, p1] = pages_of(&doc);
    let labels = Op::SetPageLabels {
        ranges: Some(vec![
            fx::label(p0, LabelStyle::Decimal, "\u{3a9}-", 1),
            fx::label(p1, LabelStyle::Decimal, "\u{141}-", 1),
        ]),
    };
    let ops = vec![
        fx::set_header(fx::header("<<label>>"), None),
        labels,
        fx::add(p0, &note("\u{17a}", 600.0)),
        fx::add(p1, &note("\u{15b}", 600.0)),
    ];
    doc.commit_ops(ops, "set").unwrap();
    let (e0, e1) = (
        overlaid(&doc, p0, EmitScope::Delta(p0)),
        overlaid(&doc, p1, EmitScope::Delta(p1)),
    );
    let (id0, g0) = e0.fallback.clone().expect("page 1's subset");
    let (id1, g1) = e1.fallback.clone().expect("page 2's subset");
    assert_eq!(numbers(&g0), glyphs_of("\u{3a9}\u{17a}"));
    assert_eq!(numbers(&g1), glyphs_of("\u{141}\u{15b}"));
    let ([r0], [r1]) = (
        <[ObjKey; 1]>::try_from(Vec::from_iter(page_res(&e0))).unwrap(),
        <[ObjKey; 1]>::try_from(Vec::from_iter(page_res(&e1))).unwrap(),
    );
    assert_ne!(r0, r1, "the pages' resources apart");
    assert!(fonts_bound(&e0, r0).contains(&ObjKey::Res(id0)));
    assert!(fonts_bound(&e1, r1).contains(&ObjKey::Res(id1)));
    let st = doc.state();
    let mark = |page| st.marks_on(page).next().unwrap().id;
    let ap = |page| ObjKey::AnnotPart(mark(page), Part::Ap);
    // Each note's appearance binds its page's subset (beside Helvetica,
    // which it sets again after its run in the fallback).
    let (f0, f1) = (fonts_bound(&e0, ap(p0)), fonts_bound(&e1, ap(p1)));
    assert!(f0.contains(&ObjKey::Res(id0)) && !f0.contains(&ObjKey::Res(id1)));
    assert!(f1.contains(&ObjKey::Res(id1)) && !f1.contains(&ObjKey::Res(id0)));

    let inputs = ViewInputs::plugin();
    let realizer = Realizer::new();
    let [v0, v1] = [p0, p1].map(|p| realizer.view(st, p, &inputs).unwrap());
    assert_eq!(v0.engine, v1.engine, "one group");
    let realized = realizer.open(st, v0.engine, &inputs).unwrap();
    let bytes = realized.segments.concat();
    // Ω and ź on the first page, Ł and ś on the second.
    assert_eq!(subsets_hold(&bytes), 4);
    assert_ne!(resources_at(&bytes, 0), resources_at(&bytes, 1));
}

/// What the built owners write of a note in two fonts: a mark written
/// into a page through a writer of its own, a flattened mark's page, a
/// stamp's page back from a saved appearance, and the comments gathered
/// for an exchange file, each a subset of what it draws, which every code
/// is a glyph of.
#[test]
fn a_note_in_two_fonts_through_each_built_owner() {
    use crate::model::{MarkAp, PinnedAp};

    let a = note("\u{141}ukasz Nguy\u{1ec5}n", 600.0);
    // Written into a page through a writer of its own, finished.
    let mut f = crate::forms::fixture::Form::new(1);
    let mut page = f.dict(f.pages[0]).clone();
    let held = crate::forms::fixture::holding(std::slice::from_ref(&a));
    crate::forms::fixture::write_marks(&mut f.pdf, &mut page, &held);
    *f.dict(f.pages[0]) = page;
    let bytes = f.bytes();
    assert_eq!(subsets_hold(&bytes), 2, "written");
    assert_eq!(fallback_subsets(&bytes).len(), 1);

    // A flattened mark's page: its writer's subset beside the page's.
    let inputs = MarkAp {
        kind: a.kind.clone(),
        color: a.color,
        opacity: a.opacity,
        stroke: a.stroke,
        contents: a.contents.clone(),
        fill: None,
        dashed: false,
        cloudy: false,
        turn: Rotation::R0,
        stamp: None,
    };
    let pinned = crate::annot::pinned_pdf(&Arc::new(PinnedAp::new(inputs))).unwrap();
    assert_eq!(subsets_hold(&pinned), 2, "flattened");

    // A stamp's page back from the appearance a save wrote.
    let dir = scratch();
    let (path, mut doc) = opened(&dir, "a.pdf", &text_file(1));
    let [p0] = pages_of(&doc);
    doc.commit_ops(vec![fx::add(p0, &a)], "add").unwrap();
    save_rewritten(&mut doc, &path).unwrap();
    let saved = Pdf::load(&path).unwrap();
    let annots = saved
        .get_dictionary(pages(&saved)[0])
        .unwrap()
        .get(b"Annots");
    let annot = annots.and_then(Object::as_array).unwrap()[0].as_reference();
    let ap = saved.get_dictionary(annot.unwrap()).unwrap().get(b"AP");
    let form = ap.and_then(Object::as_dict).unwrap().get(b"N");
    let form = form.and_then(Object::as_reference).unwrap();
    let stamp = crate::stamp_lib::form_to_pdf(&saved, form).unwrap();
    assert_eq!(subsets_hold(&stamp), 2, "a stamp's page");

    // The comments gathered for an exchange file: a file of no pages,
    // read here through a page that lists them.
    let id = doc.state().marks_on(p0).next().unwrap().id;
    let comments = crate::exchange::gather(&doc.snapshot(), &[id], &[]).unwrap();
    let ids: Vec<ObjectId> = comments.annots.iter().map(|&(_, id)| id).collect();
    let listed = listed_on_a_page(&comments.src.bytes()[..], &ids);
    assert_eq!(subsets_hold(&listed), 2, "gathered");
}
