//! A text field's appearance made anew where the model holds its value
//! changed (M10 slice 6), through the emitter: a Rewrite and an Append each
//! write it where a mark's is, naming the `/DR` font's own object, and
//! nothing else anew; every kind of widget (one with its field, one split
//! from it, a duplicate's, an inserted file's whose font `/DR`'s merge
//! renamed) in its own source's font; each kind of `/DR` font bound; an
//! overlay and a save holding the same form; no stream of the old
//! appearance left in a Rewrite; saves in a row; a value put back; and one
//! that can't be made.

use std::collections::BTreeSet;
use std::sync::Arc;

use crate::pdf::{
    Dictionary, LazyPdf, Object, ObjectId, Stream, StreamExt, Value as PdfValue, XrefKind,
    dictionary, literal,
};
use in_progress::NoProgress;

use super::append_tests::{appended_in_place, planned_in_place, written};
use super::emit::{self, EmitBase, EmitScope, Overlay, RotatePolicy, SymObject};
use super::key::{ObjKey, Part};
use super::removal_tests::{opened, scratch, warned};
use super::tests::{add, insert, lazy, open, rewrite, span};
use super::*;
use crate::annot::AnnotId;
use crate::forms::appearance::tests::shown;
use crate::forms::fixture::{Form, row};
use crate::forms::{FieldId, Value};
use crate::model::{Document, FieldNode, PageId, Terminal};
use crate::prim::{Key, Val};
use crate::realize::{ViewInputs, emission_is_base, untouched};
use crate::tx::fixture as fx;

/// `f`'s file, its cross-reference a table, so a Rewrite writes each object
/// on its own and an object nothing changed is its bytes as they were.
fn table(f: &mut Form) -> Vec<u8> {
    f.pdf.xref_kind = XrefKind::Table;
    f.bytes()
}

/// Two pages: on the first the text field `a`, its own widget, holding
/// "Trenton" with an appearance that draws it; on the second `b`, holding
/// "Other". Both draw in `/DR`'s Helvetica (`/Helv 10 Tf 0 g`), which `/DR`
/// holds by reference.
fn two_fields() -> (Form, ObjectId, ObjectId) {
    let mut f = Form::new(2);
    let a = f.text("a", 0, row(0));
    f.fill(a, "Trenton");
    let b = f.text("b", 1, row(0));
    f.fill(b, "Other");
    (f, a, b)
}

/// The widgets of the field named `name` in `d`.
fn widgets_of(d: &Document, name: &str) -> Vec<AnnotId> {
    let st = d.state();
    let id = (st.field_named(name)).unwrap_or_else(|| panic!("no field {name}"));
    let term = st.field(id).and_then(|f| f.term.clone());
    term.expect("a terminal").widgets.iter().copied().collect()
}

/// Field `id` of `d` filled with the text `value`, as
/// [`fx::fill`] fills one by name.
fn fill_id(d: &mut Document, id: FieldId, value: &str) {
    let mut node = FieldNode::clone(d.state().field(id).expect("a field"));
    let mut term = Terminal::clone(node.term.as_deref().expect("a terminal"));
    term.value = Some(Value::Text(value.into()));
    node.term = Some(Arc::new(term));
    d.put_for_test(Key::Field(id), Some(Val::Field(Arc::new(node))), "fill");
}

/// A widget's `/AP` in `pdf`, references followed.
fn ap_dict(pdf: &LazyPdf, widget: ObjectId) -> Option<Dictionary> {
    let ap = pdf.get_dictionary(widget).ok()?.get(b"AP")?;
    Some(pdf.dereference(ap).ok()?.1.as_dict()?.clone())
}

/// The number a widget's `/AP /N` names in `pdf`.
fn ap_n(pdf: &LazyPdf, widget: ObjectId) -> Option<ObjectId> {
    ap_dict(pdf, widget)?.get(b"N")?.as_reference()
}

/// The stream object `id` is in `pdf`.
fn stream(pdf: &LazyPdf, id: ObjectId) -> Stream {
    let obj = pdf.get(id).unwrap_or_else(|e| panic!("{id:?}: {e}"));
    obj.as_stream().expect("a stream").clone()
}

/// A made form as a file or an emission holds it: its box, its matrix, its
/// content decoded, and the names its `/Resources /Font` binds.
#[derive(Debug, PartialEq)]
struct Made {
    bbox: Vec<f64>,
    matrix: Option<Vec<f64>>,
    content: Vec<u8>,
    names: Vec<Vec<u8>>,
}

/// What `form` is ([`Made`]), and what each of its font names is bound to.
fn made(form: &Stream) -> (Made, Vec<ObjectId>) {
    let numbers = |o: Option<&Object>| -> Option<Vec<f64>> {
        let items = o?.as_array()?;
        Some(
            items
                .iter()
                .map(|n| n.as_number().expect("a number"))
                .collect(),
        )
    };
    let fonts = (form.dict.get(b"Resources").and_then(Object::as_dict))
        .and_then(|r| r.get(b"Font"))
        .and_then(Object::as_dict)
        .cloned()
        .unwrap_or_default();
    let (names, bound) = (fonts.iter())
        .map(|(k, v)| (k.to_vec(), v.as_reference().expect("a font by reference")))
        .unzip();
    let made = Made {
        bbox: numbers(form.dict.get(b"BBox")).expect("a box"),
        matrix: numbers(form.dict.get(b"Matrix")),
        content: form.decompressed_content().expect("content that decodes"),
        names,
    };
    (made, bound)
}

/// The form a save holds for the widget `widget` of `pdf`, by its `/AP /N`.
fn saved_form(pdf: &LazyPdf, widget: ObjectId) -> (Made, Vec<ObjectId>) {
    made(&stream(pdf, ap_n(pdf, widget).expect("an /AP /N")))
}

/// The form an overlay of page `page` of `d` alone, for the screen,
/// writes for widget `aid`: none where it writes none.
fn overlay_form(d: &Document, page: PageId, aid: AnnotId) -> Option<Made> {
    let st = d.state();
    let inputs = ViewInputs::screen();
    let overlay = Overlay {
        src: st.def(page).unwrap().base.src.clone(),
        inputs: &inputs,
        rotate: RotatePolicy::KeepBase,
        arts: None,
    };
    let scope = EmitScope::Pages([page].into());
    let e = emit::emit(st, scope, EmitBase::Overlay(&overlay), &[], &NoProgress).unwrap();
    let key = ObjKey::AnnotPart(aid, Part::Ap);
    let (_, form) = e.objects.iter().find(|(k, _)| *k == key)?;
    let SymObject::Built(form) = form else {
        panic!("{key:?} isn't built");
    };
    Some(made(form.as_stream().expect("a stream")).0)
}

/// Whether any stream `pdf` holds, decoded, holds `text`.
fn any_stream_holds(pdf: &LazyPdf, text: &[u8]) -> Vec<ObjectId> {
    let ids: Vec<ObjectId> = pdf.ids().collect();
    (ids.into_iter())
        .filter(|&id| {
            let Ok(obj) = pdf.get(id) else {
                return false;
            };
            let Some(s) = obj.as_stream() else {
                return false;
            };
            let data = s
                .decompressed_content()
                .unwrap_or_else(|_| s.raw_bytes().to_vec());
            data.windows(text.len()).any(|w| w == text)
        })
        .collect()
}

/// `d`'s keys and values, wherever each was read.
fn entries(d: &Dictionary) -> Vec<(Vec<u8>, PdfValue)> {
    d.iter()
        .map(|(k, v)| (k.to_vec(), v.value().clone()))
        .collect()
}

/// A string `/V` in `pdf`'s object `field`, its bytes.
fn value_of(pdf: &LazyPdf, field: ObjectId) -> Option<Vec<u8>> {
    let v = pdf.get_dictionary(field).ok()?.get(b"V")?;
    v.as_bytes().map(<[u8]>::to_vec)
}

/// A Rewrite of a filled field writes its appearance where a mark's is:
/// under `AnnotPart(aid, Ap)`, the widget's `/AP` naming it alone (no
/// `/D`, no `/R`), its `/Resources` naming `/DR`'s Helvetica, the font's
/// own object, no copy of it; its content `/Tx BMC`, drawing the value now.
/// The field's `/V` is the file's (no save writes a value before M10 slice
/// 9). Nothing else is written anew: the objects are the unfilled
/// Rewrite's less the old appearance's stream, which nothing names now and
/// no stream of the file holds again, and with the made form, each other
/// one its bytes as they were; the unchanged field `b` is the file's
/// object as it is. Where a widget nothing made names the same stream, it
/// stays, named by that widget alone.
#[test]
fn a_rewrite_writes_a_filled_field_s_appearance_where_a_mark_s_is() {
    let (mut f, a, b) = two_fields();
    let helv = f.helv;
    let bytes = table(&mut f);
    let was = lazy(&bytes, None);
    let old = ap_n(&was, a).expect("the file's appearance");
    assert_eq!(any_stream_holds(&was, b"Trenton"), [old]);
    let plain = rewrite(&open(&bytes, None), Profile::default());
    let plain_pdf = lazy(&plain.bytes, None);
    assert_eq!(ap_n(&plain_pdf, a), Some(old));

    let mut d = open(&bytes, None);
    let [aid] = widgets_of(&d, "a")[..] else {
        panic!("one widget")
    };
    fx::fill(&mut d, "a", "Camden");
    let out = rewrite(&d, Profile::default());
    let pdf = lazy(&out.bytes, None);
    let ap = out.record.map.keys[&ObjKey::AnnotPart(aid, Part::Ap)];
    let dict = ap_dict(&pdf, a).expect("an /AP");
    assert_eq!(dict.len(), 1, "{dict:?}");
    assert_eq!(ap_n(&pdf, a), Some(ap));
    let (form, fonts) = saved_form(&pdf, a);
    assert!(form.content.starts_with(b"/Tx BMC"), "{form:?}");
    assert_eq!(shown(&form.content), b"Camden");
    assert_eq!(fonts, [helv], "the /DR font's own object");
    assert_eq!(value_of(&pdf, a).as_deref(), Some(b"Trenton".as_slice()));

    let ids = |p: &LazyPdf| -> BTreeSet<ObjectId> { p.ids().collect() };
    let mut want = ids(&plain_pdf);
    assert!(want.remove(&old));
    assert!(want.insert(ap));
    assert_eq!(ids(&pdf), want);
    assert_eq!(pdf.ids().count(), plain_pdf.ids().count());
    for &id in want.iter().filter(|&&id| id != a && id != ap) {
        let (now, then) = (
            span(&pdf, &out.bytes, id),
            span(&plain_pdf, &plain.bytes, id),
        );
        assert_eq!(now, then, "{id:?}");
    }
    assert_eq!(span(&pdf, &out.bytes, b), span(&was, &bytes, b));
    assert_eq!(any_stream_holds(&pdf, b"Trenton"), []);

    // `b` naming `a`'s stream too: it stays, for `b`.
    let (mut f, a, b) = two_fields();
    let shared = ap_n(&lazy(&table(&mut f), None), a).unwrap();
    f.dict(b).set("AP", dictionary! { "N" => shared });
    let bytes = table(&mut f);
    let mut d = open(&bytes, None);
    fx::fill(&mut d, "a", "Camden");
    let out = rewrite(&d, Profile::default());
    let pdf = lazy(&out.bytes, None);
    assert_ne!(ap_n(&pdf, a), Some(shared));
    assert_eq!(ap_n(&pdf, b), Some(shared));
    assert_eq!(any_stream_holds(&pdf, b"Trenton"), [shared]);
}

/// A planned Append of a fill (a field the file holds empty, so the save
/// replaces no text and plans no Rewrite) writes its field's object, the
/// widget, at its own number, and the made form past the file's last, and
/// nothing else: the font is the file's `/DR`'s, which it has. Then a
/// change of what a filled field shows, forced to an Append as a signed
/// file's reader would take one anyway (a planned save of it is a Rewrite:
/// the old value's text stays in the earlier revision), writes the same
/// two, the file's appearance unlinked and not copied. The unchanged
/// field is the file's.
#[test]
fn an_append_writes_a_filled_field_s_appearance_and_the_widget_alone() {
    let dir = scratch();
    let (mut f, a, b) = two_fields();
    let helv = f.helv;
    let empty = f.text("empty", 0, row(1));
    let bytes = table(&mut f);
    let was = lazy(&bytes, None);
    let (path, mut d) = opened(&dir, "a.pdf", &bytes);
    let [ew] = widgets_of(&d, "empty")[..] else {
        panic!("one widget")
    };
    fx::fill(&mut d, "empty", "Camden");
    let (base, section) = planned_in_place(&mut d);
    let record = d.saved_record().unwrap();
    let ap = record.map.keys[&ObjKey::AnnotPart(ew, Part::Ap)];
    assert!(ap.number >= was.xref().size, "{ap:?}");
    assert_eq!(
        written(base.len(), &section, &path),
        BTreeSet::from([empty, ap])
    );
    let now = std::fs::read(&path).unwrap();
    let pdf = lazy(&now, None);
    assert_eq!(ap_dict(&pdf, empty).map(|d| d.len()), Some(1));
    assert_eq!(ap_n(&pdf, empty), Some(ap));
    let (form, fonts) = saved_form(&pdf, empty);
    assert_eq!(shown(&form.content), b"Camden");
    assert_eq!(fonts, [helv]);
    assert_eq!(value_of(&pdf, empty), None, "no /V written");

    let [aid] = widgets_of(&d, "a")[..] else {
        panic!("one widget")
    };
    fx::fill(&mut d, "a", "Newark");
    let (base, section) = appended_in_place(&mut d).unwrap();
    let ap = d.saved_record().unwrap().map.keys[&ObjKey::AnnotPart(aid, Part::Ap)];
    assert_eq!(
        written(base.len(), &section, &path),
        BTreeSet::from([a, ap])
    );
    let now = std::fs::read(&path).unwrap();
    let pdf = lazy(&now, None);
    let (form, fonts) = saved_form(&pdf, a);
    assert_eq!(shown(&form.content), b"Newark");
    assert_eq!(fonts, [helv]);
    assert_eq!(value_of(&pdf, a).as_deref(), Some(b"Trenton".as_slice()));
    assert_eq!(ap_n(&pdf, b), ap_n(&was, b));
}

/// Each kind of widget draws in its own source's font, named by where it
/// is: a field merged with its widget; one split from its widgets, as a
/// page the tree lists twice shows it twice, each widget its own form; a
/// duplicate's, another instance of the form, filled alone, whose form
/// names the font `/DR` names, as the original's would; and an inserted
/// file's, whose `/Helv` isn't the document's and comes into `/DR` as
/// `/Helv_2`: its form names that font, the other file's, and the
/// document's field still the document's.
#[test]
fn each_kind_of_widget_draws_in_its_own_source_s_font() {
    // Merged, and a duplicate's.
    let (mut f, a, _) = two_fields();
    let helv = f.helv;
    let bytes = table(&mut f);
    let mut d = open(&bytes, None);
    let p0 = d.page(0).unwrap();
    d.commit_ops(vec![fx::duplicate(&[p0])], "Duplicate")
        .unwrap();
    let st = d.state();
    let copies: Vec<FieldId> = (st.terminals())
        .filter(|(_, node, _)| node.inst.copy == 1)
        .map(|(id, _, _)| id)
        .collect();
    let [copy] = copies[..] else {
        panic!("one field of the duplicate's: {copies:?}")
    };
    let copy_widget = st.field(copy).unwrap().term.as_ref().unwrap().widgets[0];
    fill_id(&mut d, copy, "Camden");
    let out = rewrite(&d, Profile::default());
    let pdf = lazy(&out.bytes, None);
    let ap = out.record.map.keys[&ObjKey::AnnotPart(copy_widget, Part::Ap)];
    let copied = out.record.map.keys[&ObjKey::Field(copy)];
    assert_ne!(copied, a);
    assert_eq!(ap_n(&pdf, copied), Some(ap));
    let (form, fonts) = saved_form(&pdf, copied);
    assert_eq!(shown(&form.content), b"Camden");
    assert_eq!(fonts, [helv], "the font /DR names");
    // The original's is the file's.
    assert_eq!(ap_n(&pdf, a), ap_n(&lazy(&bytes, None), a));

    // Split: the page listed twice.
    let mut f = Form::new(1);
    let s = f.text("s", 0, row(0));
    f.fill(s, "Trenton");
    let pages = f
        .dict(f.catalog)
        .get(b"Pages")
        .unwrap()
        .as_reference()
        .unwrap();
    let p = f.pages[0];
    f.dict(pages)
        .set("Kids", vec![Object::from(p), Object::from(p)]);
    f.dict(pages).set("Count", 2);
    let bytes = table(&mut f);
    let mut d = open(&bytes, None);
    let shows = widgets_of(&d, "s");
    assert_eq!(shows.len(), 2, "the widget on both pages");
    fx::fill(&mut d, "s", "Camden");
    let out = rewrite(&d, Profile::default());
    let pdf = lazy(&out.bytes, None);
    let field = out.record.map.keys[&ObjKey::Field(d.state().field_named("s").unwrap())];
    assert!(
        ap_dict(&pdf, field).is_none(),
        "the field holds no widget's keys"
    );
    let mut forms = BTreeSet::new();
    for &w in &shows {
        let at = out.record.map.keys[&ObjKey::Annot(w)];
        assert_ne!(at, field);
        let ap = out.record.map.keys[&ObjKey::AnnotPart(w, Part::Ap)];
        assert_eq!(ap_n(&pdf, at), Some(ap));
        let (form, fonts) = saved_form(&pdf, at);
        assert_eq!(shown(&form.content), b"Camden");
        assert_eq!(fonts, [f.helv]);
        forms.insert(ap);
    }
    assert_eq!(forms.len(), 2, "a form each");

    // An inserted file's, its font renamed in `/DR`.
    let (mut f, _, _) = two_fields();
    let bytes = table(&mut f);
    let mut other = Form::new(1);
    let cour = other.pdf.add_object(dictionary! {
        "Type" => Object::name("Font"), "Subtype" => Object::name("Type1"),
        "BaseFont" => Object::name("Courier"), "Encoding" => Object::name("WinAnsiEncoding"),
    });
    let acroform = other.acroform;
    let dr = dictionary! { "Font" => dictionary! { "Helv" => cour } };
    other.dict(acroform).set("DR", dr);
    let o = other.text("o", 0, row(2));
    other.fill(o, "Old");
    let other = table(&mut other);
    let mut d = open(&bytes, None);
    let s = add(&d, &other, "other.pdf");
    insert(&mut d, &s, 0);
    let [ow] = widgets_of(&d, "o")[..] else {
        panic!("one widget")
    };
    fx::fill(&mut d, "o", "New");
    fx::fill(&mut d, "a", "Camden");
    let out = rewrite(&d, Profile::default());
    let pdf = lazy(&out.bytes, None);
    let form = pdf.catalog().unwrap().get(b"AcroForm").unwrap();
    let form = pdf.dereference(form).unwrap().1.as_dict().unwrap().clone();
    let dr = pdf
        .dereference(form.get(b"DR").unwrap())
        .unwrap()
        .1
        .as_dict()
        .unwrap();
    let dr_fonts = pdf.dereference(dr.get(b"Font").unwrap()).unwrap().1;
    let dr_fonts = dr_fonts.as_dict().unwrap();
    let named = |n: &[u8]| dr_fonts.get(n).and_then(Object::as_reference).unwrap();
    let base = |id: ObjectId| {
        let d = pdf.get_dictionary(id).unwrap();
        d.get(b"BaseFont")
            .and_then(Object::as_name)
            .unwrap()
            .to_vec()
    };
    assert_eq!(base(named(b"Helv")), b"Helvetica");
    assert_eq!(base(named(b"Helv_2")), b"Courier");
    let at = out.record.map.keys[&ObjKey::Field(d.state().field_named("o").unwrap())];
    let da = pdf.get_dictionary(at).unwrap().get(b"DA").unwrap();
    assert_eq!(da.as_bytes(), Some(b"/Helv_2 10 Tf 0 g".as_slice()));
    let ap = out.record.map.keys[&ObjKey::AnnotPart(ow, Part::Ap)];
    assert_eq!(ap_n(&pdf, at), Some(ap));
    let (made_o, fonts) = saved_form(&pdf, at);
    assert_eq!(shown(&made_o.content), b"New");
    assert_eq!(fonts, [named(b"Helv_2")], "the other file's own font");
    let (made_a, fonts) = saved_form(&pdf, a);
    assert_eq!(shown(&made_a.content), b"Camden");
    assert_eq!(fonts, [named(b"Helv")], "the document's");
    assert_ne!(made_o.names, made_a.names, "named by where each is");
    // Neither old appearance copied: the other file's is one only a copy
    // of its `/AP` would bring.
    assert_eq!(any_stream_holds(&pdf, b"(Old)"), []);
    assert_eq!(any_stream_holds(&pdf, b"Trenton"), []);
}

/// Each kind of `/DR` font is bound: a direct dictionary that holds no
/// reference as it is, an object of its own; a direct one that holds a
/// reference, its dictionary written once as an object of its own, the
/// reference the file's object; and one `/DR` lacks, by value, the face
/// its name falls back to, `/DR` gaining nothing.
#[test]
fn each_kind_of_dr_font_is_bound() {
    let direct = dictionary! {
        "Type" => Object::name("Font"), "Subtype" => Object::name("Type1"),
        "BaseFont" => Object::name("Helvetica"), "Encoding" => Object::name("WinAnsiEncoding"),
    };
    for case in ["direct", "direct with a reference", "missing"] {
        let mut f = Form::new(1);
        let a = f.text("a", 0, row(0));
        f.fill(a, "Trenton");
        let acroform = f.acroform;
        let mut encoding = None;
        match case {
            "direct" => {
                let dr = dictionary! { "Font" => dictionary! { "Helv" => direct.clone() } };
                f.dict(acroform).set("DR", dr);
            }
            "direct with a reference" => {
                let enc = f.pdf.add_object(dictionary! {
                    "Type" => Object::name("Encoding"),
                    "BaseEncoding" => Object::name("WinAnsiEncoding"),
                    "Differences" => vec![Object::from(200), Object::name("Euro")],
                });
                let mut font = direct.clone();
                font.set("Encoding", enc);
                encoding = Some(enc);
                let dr = dictionary! { "Font" => dictionary! { "Helv" => font } };
                f.dict(acroform).set("DR", dr);
            }
            _ => f.dict(a).set("DA", literal("/Gone 10 Tf 0 g")),
        }
        let bytes = table(&mut f);
        let mut d = open(&bytes, None);
        fx::fill(&mut d, "a", "Camden");
        let out = rewrite(&d, Profile::default());
        let pdf = lazy(&out.bytes, None);
        let (form, fonts) = saved_form(&pdf, a);
        assert_eq!(shown(&form.content), b"Camden", "{case}");
        let [font] = fonts[..] else {
            panic!("{case}: one font, {fonts:?}")
        };
        let font = pdf.get_dictionary(font).unwrap();
        match case {
            "direct" => assert_eq!(entries(font), entries(&direct), "{case}"),
            "direct with a reference" => {
                let enc = font.get(b"Encoding").and_then(Object::as_reference);
                assert_eq!(enc, encoding, "{case}: the file's object");
                let mut want = entries(&direct);
                want.retain(|(k, _)| k != b"Encoding");
                let mut got = entries(font);
                got.retain(|(k, _)| k != b"Encoding");
                assert_eq!(got, want, "{case}");
            }
            _ => {
                let face = font.get(b"BaseFont").and_then(Object::as_name);
                assert_eq!(face, Some(b"Helvetica".as_slice()), "{case}");
                let form = pdf.catalog().unwrap().get(b"AcroForm").unwrap();
                let form = pdf.dereference(form).unwrap().1.as_dict().unwrap();
                let dr = pdf.dereference(form.get(b"DR").unwrap()).unwrap().1;
                let dr = dr.as_dict().unwrap().get(b"Font").unwrap();
                let dr = pdf.dereference(dr).unwrap().1.as_dict().unwrap();
                assert!(!dr.has(b"Gone"), "{case}: /DR gains nothing");
            }
        }
    }
}

/// An overlay of the page and a save hold the same form for a filled
/// field's widget: its box, its matrix, its content decoded and the names
/// it binds its font by, in each kind of source: the document's own field,
/// and an inserted file's whose font `/DR`'s merge renamed. An overlay
/// writes no form for a field nobody filled.
#[test]
fn an_overlay_and_a_save_hold_the_same_form() {
    let (mut f, a, _) = two_fields();
    let bytes = table(&mut f);
    let mut other = Form::new(1);
    let cour = other.pdf.add_object(dictionary! {
        "Type" => Object::name("Font"), "Subtype" => Object::name("Type1"),
        "BaseFont" => Object::name("Courier"), "Encoding" => Object::name("WinAnsiEncoding"),
    });
    let acroform = other.acroform;
    let dr = dictionary! { "Font" => dictionary! { "Helv" => cour } };
    other.dict(acroform).set("DR", dr);
    let o = other.text("o", 0, row(2));
    other.fill(o, "Old");
    let other = table(&mut other);
    let mut d = open(&bytes, None);
    let s = add(&d, &other, "other.pdf");
    insert(&mut d, &s, 0);
    let [aw] = widgets_of(&d, "a")[..] else {
        panic!("one widget")
    };
    let [bw] = widgets_of(&d, "b")[..] else {
        panic!("one widget")
    };
    let [ow] = widgets_of(&d, "o")[..] else {
        panic!("one widget")
    };
    let p1 = d.page(1).unwrap();
    assert!(overlay_form(&d, p1, bw).is_none(), "nothing made for b");
    fx::fill(&mut d, "a", "Camden");
    fx::fill(&mut d, "o", "New");
    let out = rewrite(&d, Profile::default());
    let pdf = lazy(&out.bytes, None);
    let at = |key| out.record.map.keys[&key];
    let o_at = at(ObjKey::Field(d.state().field_named("o").unwrap()));
    for (aid, widget, page) in [(aw, a, 0), (ow, o_at, 2)] {
        let page = d.page(page).unwrap();
        let shown_over = overlay_form(&d, page, aid).expect("an overlay's form");
        let (saved, _) = saved_form(&pdf, widget);
        assert_eq!(shown_over, saved, "{aid:?}");
    }
}

/// Saves in a row, each an Append (forced, as a signed file's reader
/// would take one anyway: each changes what a filled field shows, which a
/// plan writes as a Rewrite): a fill writes the widget and its made form;
/// a save with nothing changed writes neither; the file's value put back
/// writes the widget as the file has it, its `/AP` the file's again, over
/// the number the made one's widget took, and reopened its page draws as
/// its file does.
#[test]
fn appends_in_a_row_fill_then_nothing_then_the_value_put_back() {
    let dir = scratch();
    let (mut f, a, _) = two_fields();
    let bytes = table(&mut f);
    let was = lazy(&bytes, None);
    let old = ap_n(&was, a).unwrap();
    let (path, mut d) = opened(&dir, "a.pdf", &bytes);
    let [aid] = widgets_of(&d, "a")[..] else {
        panic!("one widget")
    };
    fx::fill(&mut d, "a", "Camden");
    let (base, section) = appended_in_place(&mut d).unwrap();
    let ap = d.saved_record().unwrap().map.keys[&ObjKey::AnnotPart(aid, Part::Ap)];
    assert_eq!(
        written(base.len(), &section, &path),
        BTreeSet::from([a, ap])
    );

    // Nothing changed: nothing that writes the widget or a form.
    let (base, section) = appended_in_place(&mut d).unwrap();
    if !section.is_empty() {
        let wrote = written(base.len(), &section, &path);
        assert!(!wrote.contains(&a) && !wrote.contains(&ap), "{wrote:?}");
    }

    // The file's value back: the widget as the file has it.
    fx::fill(&mut d, "a", "Trenton");
    let (base, section) = appended_in_place(&mut d).unwrap();
    assert_eq!(written(base.len(), &section, &path), BTreeSet::from([a]));
    let now = std::fs::read(&path).unwrap();
    let pdf = lazy(&now, None);
    let raw = |pdf: &LazyPdf, bytes: &[u8]| {
        let span = span(pdf, bytes, a).unwrap();
        let at = (span.windows(3).position(|w| w == b"obj")).unwrap();
        span[at..].to_vec()
    };
    assert_eq!(raw(&pdf, &now), raw(&was, &bytes), "the file's object");
    assert_eq!(ap_n(&pdf, a), Some(old));
    let back = crate::open::file(&path, None).unwrap().doc;
    let st = back.state();
    let p0 = st.order()[0];
    let screen = ViewInputs::screen();
    assert!(untouched(st, p0, &screen));
    assert!(emission_is_base(st, p0, &screen).unwrap());
}

/// A value put back to the file's makes nothing: the Rewrite is the
/// unfilled one's bytes, and the page draws as its base does again.
#[test]
fn a_value_put_back_makes_nothing() {
    let (mut f, _, _) = two_fields();
    let bytes = table(&mut f);
    let plain = rewrite(&open(&bytes, None), Profile::default());
    let mut d = open(&bytes, None);
    let p0 = d.page(0).unwrap();
    let screen = ViewInputs::screen();
    fx::fill(&mut d, "a", "Camden");
    assert!(!untouched(d.state(), p0, &screen));
    fx::fill(&mut d, "a", "Trenton");
    assert!(untouched(d.state(), p0, &screen));
    assert!(emission_is_base(d.state(), p0, &screen).unwrap());
    let out = rewrite(&d, Profile::default());
    assert!(out.bytes == plain.bytes, "the unfilled Rewrite's bytes");
}

/// A value the field's font has no code for makes no appearance: the
/// widget is written with no `/AP`, so the file shows no value the field
/// no longer holds; its `/V` is the file's; the warning says which kind,
/// never the text. An overlay writes the widget so too.
#[test]
fn an_appearance_that_can_t_be_made_leaves_no_ap() {
    let (mut f, a, _) = two_fields();
    let bytes = table(&mut f);
    let mut d = open(&bytes, None);
    let [aid] = widgets_of(&d, "a")[..] else {
        panic!("one widget")
    };
    fx::fill(&mut d, "a", "Camden \u{4e2d}");
    let (out, said) = warned(|| rewrite(&d, Profile::default()));
    let pdf = lazy(&out.bytes, None);
    assert!(ap_dict(&pdf, a).is_none(), "no /AP");
    assert!(
        !out.record
            .map
            .keys
            .contains_key(&ObjKey::AnnotPart(aid, Part::Ap))
    );
    assert_eq!(value_of(&pdf, a).as_deref(), Some(b"Trenton".as_slice()));
    assert_eq!(any_stream_holds(&pdf, b"Trenton"), []);
    let unmade: Vec<&String> = said.iter().filter(|w| w.contains("appearance")).collect();
    assert_eq!(unmade.len(), 1, "{said:?}");
    assert!(unmade[0].contains("unencodable"), "{said:?}");
    assert!(
        !said
            .iter()
            .any(|w| w.contains("Camden") || w.contains('\u{4e2d}'))
    );
    let p0 = d.page(0).unwrap();
    let ((), _) = warned(|| assert!(overlay_form(&d, p0, aid).is_none()));
    let page = d.state().order()[0];
    assert!(!untouched(d.state(), page, &ViewInputs::screen()));
}
