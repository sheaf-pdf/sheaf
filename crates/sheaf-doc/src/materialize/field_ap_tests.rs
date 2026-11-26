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
use std::path::Path;

use crate::pdf::{
    Dictionary, LazyPdf, Object, ObjectId, Stream, StreamExt, Value as PdfValue, XrefKind,
    dictionary, literal,
};
use in_progress::NoProgress;
use sheaf_edit::{FieldRef, FieldSet, FieldValue, Op};

use super::append_tests::{appended_in_place, planned_in_place, written};
use super::emit::{
    self, Emission, EmitBase, EmitScope, Overlay, PdfVersion, RotatePolicy, SymObject,
};
use super::key::{ObjKey, Part};
use super::removal_tests::{opened, scratch, warned};
use super::tests::{add, insert, lazy, open, rewrite, span};
use super::*;
use super::{AskAnswers, SaveHow, StrategyKind, save_with};
use crate::annot::AnnotId;
use crate::content::fixture::{FALLBACK_TEXT, fallback_subsets, subsets_hold};
use crate::content::{Fallback, Glyphs, Objects, Unkept};
use crate::forms::appearance::tests::shown;
use crate::forms::field::flags;
use crate::forms::fixture::{Form, Rect, kids_beside, row};
use crate::forms::{FieldId, FieldKind, Value};
use crate::model::{Document, PageId};
use crate::pdf::forms::{self as family, FieldValue as FamilyValue};
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

/// Field `id` of `d` filled with the text `value` by `SetFieldValues`, as
/// [`fx::fill`] fills one by name: the one field, where another has its
/// name.
fn fill_id(d: &mut Document, id: FieldId, value: &str) {
    let op = Op::SetFieldValues {
        values: vec![FieldSet {
            field: FieldRef::Id(id),
            value: FieldValue::Text(value.into()),
            display: None,
        }],
        calculated: Vec::new(),
    };
    d.commit_ops(vec![op], "fill").unwrap();
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
/// The field's `/V` is the value now (M10 slice 9). Nothing else is written
/// anew: the objects are the unfilled Rewrite's less the old appearance's
/// stream, which nothing names now and no stream of the file holds again,
/// and with the made form, each other one its bytes as they were; the
/// unchanged field `b` is the file's object as it is. Where a widget
/// nothing made names the same stream, it stays, named by that widget
/// alone.
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
    assert_eq!(value_of(&pdf, a).as_deref(), Some(b"Camden".as_slice()));

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

    // The whole saved file, by this Rewrite and by an Append of the same
    // fill, as the code before M10 slice 8 made `appearance` a module of
    // kinds wrote it: the widget's dictionary, its form's and the form's
    // compressed content, which nothing above holds byte for byte, are as
    // they were. Taken from that code, and moved once since, by M10 slice
    // 9's `/V`: each file's `qpdf --qdf` dump then differed from the one
    // before in the field's `/V (Camden)` alone and the offsets after it,
    // nothing of a stream. Never edited otherwise.
    assert_eq!(
        sheaf_util::checksum_hex(&out.bytes),
        "5b8cbabc969c540d1eae9379fa65684d"
    );
    let dir = scratch();
    let (path, mut appended) = opened(&dir, "a.pdf", &bytes);
    fx::fill(&mut appended, "a", "Camden");
    appended_in_place(&mut appended).unwrap();
    assert_eq!(
        sheaf_util::checksum_hex(&std::fs::read(&path).unwrap()),
        "4eca288e19c35b1284ad0deec05b4197"
    );

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
    assert_eq!(value_of(&pdf, empty).as_deref(), Some(b"Camden".as_slice()));

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
    assert_eq!(value_of(&pdf, a).as_deref(), Some(b"Newark".as_slice()));
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

/// A terminal field as [`form_shape`] gives it: its full name, its kind,
/// and each of its widgets' page, place among the field's widgets and
/// whether it's the field's own dictionary.
pub(super) type FieldShape = (String, FieldKind, Vec<(usize, u32, bool)>);

/// What `d`'s form is, as a saved file reopened is held to it: each
/// terminal's [`FieldShape`].
pub(super) fn form_shape(d: &Document) -> Vec<FieldShape> {
    let st = d.state();
    (st.terminals())
        .map(|(id, _, term)| {
            let widgets = (term.widgets.iter())
                .map(|&w| {
                    let rec = st.annot(w).expect("its widget");
                    let widget = rec.widget().expect("a widget");
                    let page = st.pos_of(rec.page).expect("its page");
                    (page, widget.props.index, widget.merged)
                })
                .collect();
            (st.full_name(id).unwrap_or_default(), term.kind, widgets)
        })
        .collect()
}

/// How the tests of a field beside its kid widgets save: each way, then as
/// planned.
pub(super) const EACH_WAY: [&str; 3] = ["a Rewrite", "an Append", "as planned"];

/// `doc`, opened from `path`, saved `how` ([`EACH_WAY`]): checked as every
/// save is, or why it stopped.
pub(super) fn saved_each_way(doc: &mut Document, path: &Path, how: &str) -> StrategyKind {
    let report = match how {
        "a Rewrite" => save_rewritten(doc, path),
        "an Append" => save_appended(doc, path),
        _ => save_with(
            doc,
            path,
            SaveHow::Planned,
            &AskAnswers::default(),
            &NoProgress,
        ),
    };
    let report = report.unwrap_or_else(|stop| panic!("{how}: {stop}"));
    report.strategy.expect("a file written")
}

/// What field `field`'s `/Kids` names in `pdf`, in order.
pub(super) fn kids_of(pdf: &LazyPdf, field: ObjectId) -> Vec<ObjectId> {
    let d = pdf.get_dictionary(field).expect("the field");
    let kids = d
        .get(b"Kids")
        .and_then(Object::as_array)
        .expect("its /Kids");
    kids.iter().filter_map(Object::as_reference).collect()
}

/// A field whose own dictionary is a widget beside kid widgets is written
/// as its file has it: one object that's the field and that widget, at the
/// field's number, with the widget's keys and its made form, its kids in
/// its `/Kids` (written apart from the field, that widget took a number of
/// its own that its page's `/Annots` didn't name, so an Append left the
/// field's object there with no widget's keys, and stopped at the save's
/// check). The empty text field `t` filled, saved by a Rewrite, an Append
/// and as planned (an Append: it held nothing), is checked as every save
/// is; each widget's form draws the value; the Append writes the field and
/// its kids at their numbers, what's new past the file's last, and no
/// page; and each saved file reopens to the same fields, each widget at
/// its place.
#[test]
fn a_field_beside_its_kid_widgets_is_saved_as_its_file_has_it() {
    let mut f = kids_beside();
    let t = f.fields()[0];
    let kids = f.widgets_of(t);
    let pages = f.pages.clone();
    let bytes = table(&mut f);
    let size = lazy(&bytes, None).xref().size;
    let shape = form_shape(&open(&bytes, None));
    // Its own first, as the tree is read, at the place after its kids'.
    assert_eq!(shape[0].2, [(0, 2, true), (0, 0, false), (1, 1, false)]);
    for how in EACH_WAY {
        let dir = scratch();
        let (path, mut d) = opened(&dir, "a.pdf", &bytes);
        let [own, k0, k1] = widgets_of(&d, "t")[..] else {
            panic!("{how}: the field's own and two kids")
        };
        fx::fill(&mut d, "t", "Camden");
        let strategy = saved_each_way(&mut d, &path, how);
        let appended = how != "a Rewrite";
        let want = match appended {
            true => StrategyKind::Append,
            false => StrategyKind::Rewrite,
        };
        assert_eq!(strategy, want, "{how}");
        let record = d.saved_record().unwrap();
        let at = |key| record.map.keys[&key];
        let field = d.state().field_named("t").unwrap();
        assert_eq!(at(ObjKey::Field(field)), t, "{how}");
        assert_eq!(at(ObjKey::Annot(own)), t, "{how}: one object");
        assert_eq!([at(ObjKey::Annot(k0)), at(ObjKey::Annot(k1))], kids[..]);
        let now = std::fs::read(&path).unwrap();
        let pdf = lazy(&now, None);
        let dict = pdf.get_dictionary(t).unwrap();
        let subtype = dict.get(b"Subtype").and_then(Object::as_name);
        assert_eq!(subtype, Some(b"Widget".as_slice()), "{how}");
        assert_eq!(
            dict.get(b"P").and_then(Object::as_reference),
            Some(pages[0])
        );
        assert_eq!(kids_of(&pdf, t), kids, "{how}");
        for (w, id) in [(own, t), (k0, kids[0]), (k1, kids[1])] {
            assert_eq!(ap_n(&pdf, id), Some(at(ObjKey::AnnotPart(w, Part::Ap))));
            assert_eq!(shown(&saved_form(&pdf, id).0.content), b"Camden", "{how}");
        }
        if appended {
            let wrote = written(bytes.len(), &now[bytes.len()..], &path);
            let old: BTreeSet<ObjectId> = (wrote.iter().copied())
                .filter(|id| id.number < size)
                .collect();
            assert_eq!(old, BTreeSet::from([t, kids[0], kids[1]]), "{how}");
            assert!(pages.iter().all(|p| !wrote.contains(p)), "{how}: no page");
        }
        assert_eq!(form_shape(&open(&now, None)), shape, "{how}: reopened");
    }
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
/// no longer holds; its `/V` is the value it holds (M10 slice 9); the
/// save's warning says which kind, never the text. An overlay writes the
/// widget so too, and warns of nothing (it says so at debug level, as it's
/// made at every realize). The op refuses such a value, so it's put as
/// only a file gives one (`fx::put_value`).
#[test]
fn an_appearance_that_can_t_be_made_leaves_no_ap() {
    let (mut f, a, _) = two_fields();
    let bytes = table(&mut f);
    let mut d = open(&bytes, None);
    let [aid] = widgets_of(&d, "a")[..] else {
        panic!("one widget")
    };
    fx::put_value(&mut d, "a", Some(Value::Text("Camden \u{4e2d}".into())));
    let (out, said) = warned(|| rewrite(&d, Profile::default()));
    let pdf = lazy(&out.bytes, None);
    assert!(ap_dict(&pdf, a).is_none(), "no /AP");
    assert!(
        !out.record
            .map
            .keys
            .contains_key(&ObjKey::AnnotPart(aid, Part::Ap))
    );
    let value = crate::pdf::text::decode(&value_of(&pdf, a).expect("a /V"));
    assert_eq!(value, "Camden \u{4e2d}");
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
    let ((), said) = warned(|| assert!(overlay_form(&d, p0, aid).is_none()));
    assert_eq!(said, Vec::<String>::new(), "an overlay's at debug");
    let page = d.state().order()[0];
    assert!(!untouched(d.state(), page, &ViewInputs::screen()));
}

/// A WinAnsi value's form, as a Rewrite and an Append write it, the whole
/// Rewrite and the Append's section: the bytes the code before the
/// fallback font (M10 slice 7) wrote, held as literals taken from it, its
/// length and checksum each, the form's never edited, the file's and the
/// section's moved once since, by M10 slice 9's `/V`: each then differed
/// from the one before in the field's `/V`, the value in UTF-16BE, and the
/// offsets after it alone (each file's `qpdf --qdf` dump, and the
/// section's bytes), nothing of a stream. The form names `/DR`'s Helvetica
/// alone, and nothing is written in the fallback.
#[test]
fn a_winansi_value_s_form_is_the_bytes_it_was() {
    let held = |bytes: &[u8]| (bytes.len(), sheaf_util::checksum_hex(bytes));
    let at = |len: usize, sum: &str| (len, sum.to_string());
    let (mut f, a, _) = two_fields();
    let helv = f.helv;
    let bytes = table(&mut f);
    let mut d = open(&bytes, None);
    let [aid] = widgets_of(&d, "a")[..] else {
        panic!("one widget")
    };
    fx::fill(&mut d, "a", "Camden \u{d6}l\u{e7}er");
    let out = rewrite(&d, Profile::default());
    let pdf = lazy(&out.bytes, None);
    let ap = out.record.map.keys[&ObjKey::AnnotPart(aid, Part::Ap)];
    let form = span(&pdf, &out.bytes, ap).unwrap();
    assert_eq!(held(form), at(260, "a05b80a7d9917226804cc8d8d5fd512a"));
    assert_eq!(
        held(&out.bytes),
        at(1969, "bde58c8e3d7fe713912d264b35a07070")
    );
    assert_eq!(saved_form(&pdf, a).1, [helv]);
    assert_eq!(subsets_hold(&out.bytes), 0);
    assert!(out.record.fallbacks.is_empty());

    let dir = scratch();
    let (path, mut d) = opened(&dir, "a.pdf", &bytes);
    fx::fill(&mut d, "a", "Camden \u{d6}l\u{e7}er");
    let (_, section) = appended_in_place(&mut d).unwrap();
    let ap = d.saved_record().unwrap().map.keys[&ObjKey::AnnotPart(aid, Part::Ap)];
    let now = std::fs::read(&path).unwrap();
    let pdf = lazy(&now, None);
    let form = span(&pdf, &now, ap).unwrap();
    assert_eq!(held(form), at(260, "a05b80a7d9917226804cc8d8d5fd512a"));
    assert_eq!(held(&section), at(570, "1a1255d8938f29aaab7c7bf0f999a9ba"));
    assert_eq!(subsets_hold(&now), 0);
    assert!(d.saved_record().unwrap().fallbacks.is_empty());
}

// ----- the value (M10 slice 9) ----------------------------------------------

/// A field's value as the family reads it (`Field::value`, under PDFium's
/// rules), owned.
#[derive(Debug, PartialEq)]
pub(super) enum Theirs {
    Text(String),
    State(Vec<u8>),
    Items(Vec<String>),
    Other,
}

/// The value of the field named `name` in the file `bytes`, as sheaf's
/// reader reads it and as the family's does: what a save's `/V` reads back
/// as, none for none. Panics where either reads no such field.
pub(super) fn read_back(bytes: &[u8], name: &str) -> (Option<Value>, Option<Theirs>) {
    let src = super::tests::source(bytes, None);
    let form = src.form();
    let ours = (form.fields.iter().find(|f| f.name == name))
        .unwrap_or_else(|| panic!("sheaf reads no {name}"));
    let pdf = src.pdf();
    let unkept = Unkept::default();
    let objects = Objects::new(pdf, &unkept);
    let catalog = pdf.catalog().expect("a catalog");
    let theirs = family::read(objects, catalog, &family::Limits::DEFAULT)
        .expect("a form the family reads")
        .expect("a form");
    let (_, field) = (theirs.terminals())
        .find(|(i, _)| theirs.full_name(*i).as_deref() == Some(name))
        .unwrap_or_else(|| panic!("the family reads no {name}"));
    let value = field.value().map(|v| match v {
        FamilyValue::Text(t) => Theirs::Text(t.clone()),
        FamilyValue::State(s) => Theirs::State(s.to_vec()),
        FamilyValue::Items(items) => Theirs::Items(items.clone()),
        _ => Theirs::Other,
    });
    (ours.value.clone(), value)
}

/// What [`read_back`] gives for the text `t`, read alike by both.
pub(super) fn text_read(t: &str) -> (Option<Value>, Option<Theirs>) {
    (Some(Value::Text(t.into())), Some(Theirs::Text(t.into())))
}

/// `text` in UTF-16BE after its mark, as `pdf::text::string` writes what
/// isn't printable ASCII.
pub(super) fn utf16be(text: &str) -> Vec<u8> {
    let units = text.encode_utf16().flat_map(u16::to_be_bytes);
    [0xFE, 0xFF].into_iter().chain(units).collect()
}

/// Whether `bytes` hold `text` as written: its own bytes, or in UTF-16BE.
/// A stream's text is read decoded ([`any_stream_holds`]).
fn bytes_hold(bytes: &[u8], text: &str) -> bool {
    let utf16 = utf16be(text);
    let has = |needle: &[u8]| bytes.windows(needle.len()).any(|w| w == needle);
    has(text.as_bytes()) || has(&utf16[2..])
}

/// A text field's value is a save's `/V` (M10 slice 9), by a Rewrite and
/// by an Append, written in the field's own dictionary at its own number,
/// and reads back as the model holds it by sheaf's reader and the
/// family's: ASCII as a literal string, text past PDFDocEncoding (`ễ`) in
/// UTF-16BE after its mark, text typed decomposed (NFD) composed (NFC), as
/// the op holds it, and the empty text as `()`. A field nothing set is its
/// file's object.
#[test]
fn a_text_field_s_value_is_written_and_reads_back_both_ways() {
    let mut f = Form::new(1);
    let ascii = f.text("ascii", 0, row(0));
    f.fill(ascii, "Trenton");
    let past = f.text("past", 0, row(1));
    let nfd = f.text("nfd", 0, row(2));
    let emptied = f.text("emptied", 0, row(3));
    f.fill(emptied, "Gone");
    let kept = f.text("kept", 0, row(4));
    f.fill(kept, "Kept");
    let bytes = table(&mut f);
    let was = lazy(&bytes, None);
    let fill = |d: &mut Document| {
        fx::fill(d, "ascii", "Camden");
        fx::fill(d, "past", "Nguy\u{1ec5}n");
        fx::fill(d, "nfd", "Cafe\u{301}");
        fx::fill(d, "emptied", "");
    };
    let check = |now: &[u8], how: &str| {
        let pdf = lazy(now, None);
        let values = [
            ("ascii", "Camden"),
            ("past", "Nguy\u{1ec5}n"),
            ("nfd", "Caf\u{e9}"),
            ("emptied", ""),
            ("kept", "Kept"),
        ];
        for (name, want) in values {
            assert_eq!(read_back(now, name), text_read(want), "{how}: {name}");
        }
        let holds = |id: ObjectId, s: &[u8]| {
            let raw = span(&pdf, now, id).expect("the field");
            raw.windows(s.len()).any(|w| w == s)
        };
        assert!(holds(ascii, b"/V(Camden)"), "{how}: a literal");
        assert!(holds(emptied, b"/V()"), "{how}: the empty text");
        assert!(holds(past, b"/V<FEFF"), "{how}: UTF-16BE, its mark first");
        let utf16 = |t: &str| Some(utf16be(t));
        assert_eq!(value_of(&pdf, past), utf16("Nguy\u{1ec5}n"), "{how}");
        assert_eq!(value_of(&pdf, nfd), utf16("Caf\u{e9}"), "{how}: composed");
    };
    let mut d = open(&bytes, None);
    fill(&mut d);
    let out = rewrite(&d, Profile::default());
    check(&out.bytes, "Rewrite");
    let pdf = lazy(&out.bytes, None);
    assert_eq!(span(&pdf, &out.bytes, kept), span(&was, &bytes, kept));

    let dir = scratch();
    let (path, mut d) = opened(&dir, "a.pdf", &bytes);
    fill(&mut d);
    let (base, section) = appended_in_place(&mut d).unwrap();
    let wrote = written(base.len(), &section, &path);
    for id in [ascii, past, nfd, emptied] {
        assert!(wrote.contains(&id), "{id:?} at its own number");
    }
    assert!(!wrote.contains(&kept), "{wrote:?}");
    check(&std::fs::read(&path).unwrap(), "Append");
}

/// A field cleared (no value) under a parent that holds a `/V` is written
/// with the empty text, so it doesn't take the parent's again; one under
/// no such parent has no `/V`; a field that took its parent's, filled,
/// holds its own. Both ways, read back by sheaf and the family; the parent
/// keeps its file's `/V`.
#[test]
fn a_cleared_field_under_a_parent_holding_a_value_holds_the_empty_text() {
    let mut f = Form::new(1);
    let own = f.text("own", 0, row(0));
    f.fill(own, "Mine");
    let took = f.text("took", 0, row(1));
    let parent = f.group("p", &[own, took]);
    f.dict(parent).set("V", literal("Theirs"));
    let alone = f.text("alone", 0, row(2));
    f.fill(alone, "Solo");
    let bytes = table(&mut f);
    assert_eq!(
        read_back(&bytes, "p.took"),
        text_read("Theirs"),
        "as opened"
    );
    let set = |d: &mut Document| {
        fx::clear(d, "p.own");
        fx::fill(d, "p.took", "Taken");
        fx::clear(d, "alone");
    };
    let check = |now: &[u8], how: &str| {
        let pdf = lazy(now, None);
        assert_eq!(read_back(now, "p.own"), text_read(""), "{how}");
        assert_eq!(
            value_of(&pdf, own).as_deref(),
            Some(b"".as_slice()),
            "{how}"
        );
        assert_eq!(read_back(now, "p.took"), text_read("Taken"), "{how}");
        assert_eq!(read_back(now, "alone"), (None, None), "{how}");
        assert!(!pdf.get_dictionary(alone).unwrap().has(b"V"), "{how}");
        let theirs = value_of(&pdf, parent);
        assert_eq!(theirs.as_deref(), Some(b"Theirs".as_slice()), "{how}");
    };
    let mut d = open(&bytes, None);
    set(&mut d);
    check(&rewrite(&d, Profile::default()).bytes, "Rewrite");
    let dir = scratch();
    let (path, mut d) = opened(&dir, "a.pdf", &bytes);
    set(&mut d);
    appended_in_place(&mut d).unwrap();
    check(&std::fs::read(&path).unwrap(), "Append");
}

/// A password field's value is never written (M10 slice 9): filled, its
/// file's `/V` goes and none is written, by a Rewrite and by an Append, and
/// the value is nowhere in what either writes, a string or a stream, its
/// appearance a star for each character; reopened, it holds no value. A
/// plain field filled beside it is found by the same search, which finds
/// the opened value in the file as opened too. The revision an Append
/// leaves before it holds the file's own value, as the file had it.
#[test]
fn a_password_field_s_value_is_never_written() {
    let mut f = Form::new(1);
    let pw = f.text("pw", 0, row(0));
    f.dict(pw).set("Ff", i64::from(flags::PASSWORD));
    f.fill(pw, "Opened1");
    f.text("plain", 0, row(1));
    let bytes = table(&mut f);
    let found = |bytes: &[u8], text: &str| {
        bytes_hold(bytes, text) || !any_stream_holds(&lazy(bytes, None), text.as_bytes()).is_empty()
    };
    assert!(
        found(&bytes, "Opened1"),
        "the control: the search finds a value"
    );
    let fill = |d: &mut Document| {
        fx::fill(d, "pw", "Hunter2");
        fx::fill(d, "plain", "Shown3");
    };
    let mut d = open(&bytes, None);
    fill(&mut d);
    let out = rewrite(&d, Profile::default());
    let pdf = lazy(&out.bytes, None);
    assert!(!pdf.get_dictionary(pw).unwrap().has(b"V"), "no /V");
    assert_eq!(read_back(&out.bytes, "pw"), (None, None));
    assert!(
        found(&out.bytes, "Shown3"),
        "the control: the plain field's"
    );
    assert!(!found(&out.bytes, "Hunter2"), "the password nowhere");
    assert!(!found(&out.bytes, "Opened1"), "nor the file's");
    assert_eq!(shown(&saved_form(&pdf, pw).0.content), b"*******");

    let dir = scratch();
    let (path, mut d) = opened(&dir, "a.pdf", &bytes);
    fill(&mut d);
    let (base, section) = appended_in_place(&mut d).unwrap();
    assert!(written(base.len(), &section, &path).contains(&pw));
    let now = std::fs::read(&path).unwrap();
    let pdf = lazy(&now, None);
    assert!(!pdf.get_dictionary(pw).unwrap().has(b"V"), "no /V");
    assert_eq!(read_back(&now, "pw"), (None, None));
    assert!(found(&now, "Shown3"), "the control: the section's");
    assert!(!found(&now, "Hunter2"), "the password nowhere");
    assert_eq!(shown(&saved_form(&pdf, pw).0.content), b"*******");
}

/// A filled field's rich value (`/RV`) goes, as it would show the old value
/// to a viewer that draws it, and its default style (`/DS`) stays as its
/// file has it: it styles, it holds no value. A field nothing set keeps
/// both. Both ways.
#[test]
fn a_filled_field_s_rich_value_goes_and_its_style_stays() {
    let (mut f, a, b) = two_fields();
    for (id, was) in [(a, "Trenton"), (b, "Other")] {
        let rich = format!("<body><p>{was}</p></body>");
        f.dict(id).set("RV", literal(rich));
        f.dict(id).set("DS", literal("font: 12pt Helvetica"));
    }
    let bytes = table(&mut f);
    let was = lazy(&bytes, None);
    let check = |now: &[u8], how: &str| {
        let pdf = lazy(now, None);
        let field = |id| pdf.get_dictionary(id).unwrap().clone();
        let (a_now, b_now) = (field(a), field(b));
        assert!(!a_now.has(b"RV"), "{how}: the old value's rich text gone");
        assert!(!bytes_hold(now, "<p>Trenton"), "{how}");
        let ds = |d: &Dictionary| d.get(b"DS").and_then(Object::as_bytes).map(<[u8]>::to_vec);
        let style = Some(b"font: 12pt Helvetica".to_vec());
        assert_eq!(ds(&a_now), style, "{how}: its style kept");
        assert_eq!(value_of(&pdf, a).as_deref(), Some(b"Camden".as_slice()));
        assert!(
            b_now.has(b"RV") && ds(&b_now) == style,
            "{how}: b untouched"
        );
    };
    let mut d = open(&bytes, None);
    fx::fill(&mut d, "a", "Camden");
    let out = rewrite(&d, Profile::default());
    check(&out.bytes, "Rewrite");
    let pdf = lazy(&out.bytes, None);
    assert_eq!(span(&pdf, &out.bytes, b), span(&was, &bytes, b));
    let dir = scratch();
    let (path, mut d) = opened(&dir, "a.pdf", &bytes);
    fx::fill(&mut d, "a", "Camden");
    let (_, section) = appended_in_place(&mut d).unwrap();
    assert!(!bytes_hold(&section, "<p>Trenton"));
    let now = std::fs::read(&path).unwrap();
    let pdf = lazy(&now, None);
    let a_now = pdf.get_dictionary(a).unwrap();
    assert!(!a_now.has(b"RV") && a_now.has(b"DS"), "Append");
    assert_eq!(value_of(&pdf, a).as_deref(), Some(b"Camden".as_slice()));
}

/// An overlay writes a filled field's `/V` from the model, as a save does,
/// so PDFium, which may draw a field from its value, is given the value the
/// model holds; the page isn't its base page. A field nothing set on
/// another page is its file's object in that page's overlay.
#[test]
fn an_overlay_s_field_holds_the_model_s_value() {
    let (mut f, _, _) = two_fields();
    let bytes = table(&mut f);
    let mut d = open(&bytes, None);
    fx::fill(&mut d, "a", "Camden");
    let [p0, p1] = [0, 1].map(|i| d.page(i).unwrap());
    let field = |e: &Emission, name: &str| {
        let fid = d.state().field_named(name).unwrap();
        let (_, obj) = (e.objects.iter().find(|(k, _)| *k == ObjKey::Field(fid)))
            .unwrap_or_else(|| panic!("{name} written"));
        match obj {
            SymObject::Built(obj) => Some(obj.as_dict().expect("a dictionary").clone()),
            _ => None,
        }
    };
    let e = overlaid(&d, p0, EmitScope::Pages([p0].into()));
    let a = field(&e, "a").expect("built: its value changed");
    let v = a.get(b"V").and_then(Object::as_bytes);
    assert_eq!(v, Some(b"Camden".as_slice()));
    assert!(!untouched(d.state(), p0, &ViewInputs::screen()));
    let e = overlaid(&d, p1, EmitScope::Pages([p1].into()));
    assert!(field(&e, "b").is_none(), "the file's object, raw");
    assert!(untouched(d.state(), p1, &ViewInputs::screen()));
}

/// A field filled and saved by an Append, then undone to the value it was
/// opened with and saved again, is the opened file's object again: the
/// value the Append wrote is gone from the file (strict removal plans a
/// Rewrite), and the Rewrite, writing from the file as opened and the
/// model, finds nothing of the field changed.
#[test]
fn a_fill_undone_after_its_append_saves_the_file_s_object_again() {
    let dir = scratch();
    let (mut f, _, _) = two_fields();
    let empty = f.text("empty", 0, row(1));
    let bytes = table(&mut f);
    let was = lazy(&bytes, None);
    let (path, mut d) = opened(&dir, "a.pdf", &bytes);
    fx::fill(&mut d, "empty", "Camden");
    planned_in_place(&mut d);
    let appended = std::fs::read(&path).unwrap();
    assert_eq!(read_back(&appended, "empty"), text_read("Camden"));
    assert!(bytes_hold(&appended, "Camden"), "the control");
    d.undo().unwrap();
    let report = save_with(
        &mut d,
        &path,
        SaveHow::Planned,
        &AskAnswers::default(),
        &NoProgress,
    )
    .unwrap();
    assert_eq!(report.strategy, Some(StrategyKind::Rewrite));
    let now = std::fs::read(&path).unwrap();
    let pdf = lazy(&now, None);
    assert_eq!(span(&pdf, &now, empty), span(&was, &bytes, empty));
    assert_eq!(read_back(&now, "empty"), (None, None));
    assert!(!bytes_hold(&now, "Camden"));
    assert_eq!(any_stream_holds(&pdf, b"Camden"), []);
}

// ----- in two fonts (M10 slice 7) -------------------------------------------

/// The numbers in the fallback font's whole program of the glyphs that
/// draw `chars`, a character each.
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

/// The fonts the form a widget's `/AP /N` names in `bytes` binds.
fn bound(bytes: &[u8], widget: ObjectId) -> BTreeSet<ObjectId> {
    saved_form(&lazy(bytes, None), widget)
        .1
        .into_iter()
        .collect()
}

/// How many objects a subset of `glyphs` is written as.
fn parts(glyphs: &Glyphs) -> usize {
    match Fallback::SANS.subset(glyphs).body() {
        crate::content::ResBody::Objects(objects) => objects.len(),
        body => panic!("a subset's objects, not {body:?}"),
    }
}

/// A Rewrite of a value the field's font can't draw all of draws the rest
/// in the fallback: one subset, holding only the glyphs the value draws in
/// it, which the form binds beside `/DR`'s Helvetica, each code it shows
/// a glyph of the cut program with a text; the record says the subset and
/// the map has it. Two widgets' glyphs are one subset, which both forms
/// bind. A Rewrite starts the record's subsets again: its own.
#[test]
fn a_rewrite_writes_one_subset_of_what_its_widgets_draw_in_the_fallback() {
    let (mut f, a, b) = two_fields();
    let helv = f.helv;
    let bytes = table(&mut f);
    let mut d = open(&bytes, None);
    fx::fill(&mut d, "a", FALLBACK_TEXT);
    let out = rewrite(&d, Profile::default());
    let subsets = fallback_subsets(&out.bytes);
    let [(font, codes)] = &subsets[..] else {
        panic!("one subset: {subsets:?}")
    };
    assert_eq!(*codes, glyphs_of("\u{141}\u{1ec5}"), "Ł and ễ alone");
    assert_eq!(subsets_hold(&out.bytes), 2);
    assert_eq!(bound(&out.bytes, a), BTreeSet::from([helv, *font]));
    let [(id, glyphs)] = &out.record.fallbacks[..] else {
        panic!("the record's subset")
    };
    assert_eq!(out.record.map.keys[&ObjKey::Res(*id)], *font);
    assert_eq!(numbers(glyphs), *codes);
    assert!(out.record.pdf_version >= PdfVersion::CID_FONTS);

    fx::fill(&mut d, "b", "\u{d6}l\u{e7}er \u{143}owak");
    let out = rewrite(&d, Profile::default());
    let subsets = fallback_subsets(&out.bytes);
    let [(font, codes)] = &subsets[..] else {
        panic!("one subset of both: {subsets:?}")
    };
    assert_eq!(*codes, glyphs_of("\u{141}\u{1ec5}\u{143}"));
    assert_eq!(subsets_hold(&out.bytes), 3);
    assert_eq!(bound(&out.bytes, a), BTreeSet::from([helv, *font]));
    assert_eq!(bound(&out.bytes, b), BTreeSet::from([helv, *font]));
    assert_eq!(out.record.fallbacks.len(), 1);
}

/// Appends in a row onto one file, each through [`subsets_hold`], the
/// saved file each one's prefix: the first draws `Ł` and `Ń`, which no
/// subset the record knows has, and writes one of them with the widget and
/// its form; one with `ễ` too writes a second, of `ễ` alone, the first as
/// it was. Then each binds the latest saved subset that draws its glyphs
/// and writes no font: one widget of two changed to `Ł` alone (fewer than
/// the first holds) binds the first's object; `ễ`, the second's; `Ń`, the
/// first's again, as the record carries both; a WinAnsi value names
/// Helvetica alone. Reopened, the record knows no subset (nothing reads
/// one back), so an Append that draws `Ł` writes one more: said, and
/// held. A Rewrite in place then starts the record's subsets again (its
/// own; a widget it copies as the file has it keeps its form's), and an
/// Append after it binds the Rewrite's.
#[test]
fn appends_bind_a_saved_subset_and_write_one_only_for_new_glyphs() {
    let dir = scratch();
    let (mut f, a, b) = two_fields();
    let helv = f.helv;
    let empty = f.text("empty", 0, row(1));
    let bytes = table(&mut f);
    let (path, mut d) = opened(&dir, "a.pdf", &bytes);
    let [aw] = widgets_of(&d, "a")[..] else {
        panic!("one widget")
    };
    let [bw] = widgets_of(&d, "b")[..] else {
        panic!("one widget")
    };
    let [ew] = widgets_of(&d, "empty")[..] else {
        panic!("one widget")
    };
    let form_at =
        |d: &Document, aid| d.saved_record().unwrap().map.keys[&ObjKey::AnnotPart(aid, Part::Ap)];
    let now = || std::fs::read(&path).unwrap();

    // A planned Append of a fill: a subset of Ł and Ń.
    fx::fill(&mut d, "empty", "\u{141}ukasz \u{143}owak");
    let (base, section) = planned_in_place(&mut d);
    let record = d.saved_record().unwrap();
    let [(s1, g1)] = &record.fallbacks[..] else {
        panic!("one subset")
    };
    let (s1, g1) = (*s1, g1.clone());
    assert_eq!(numbers(&g1), glyphs_of("\u{141}\u{143}"));
    let s1_at = record.map.keys[&ObjKey::Res(s1)];
    let wrote = written(base.len(), &section, &path);
    assert!(wrote.contains(&empty) && wrote.contains(&form_at(&d, ew)) && wrote.contains(&s1_at));
    assert_eq!(wrote.len(), 2 + parts(&g1), "{wrote:?}");
    assert_eq!(fallback_subsets(&now()), [(s1_at, numbers(&g1))]);
    assert_eq!(bound(&now(), empty), BTreeSet::from([helv, s1_at]));
    assert_eq!(subsets_hold(&now()), 2);

    // `ễ`, which no saved subset has: a second subset, of it alone.
    fx::fill(&mut d, "a", "Nguy\u{1ec5}n");
    let (base, section) = appended_in_place(&mut d).unwrap();
    let record = d.saved_record().unwrap();
    let [(first, _), (s2, g2)] = &record.fallbacks[..] else {
        panic!("two subsets")
    };
    assert_eq!(*first, s1);
    let (s2, g2) = (*s2, g2.clone());
    assert_eq!(numbers(&g2), glyphs_of("\u{1ec5}"));
    let s2_at = record.map.keys[&ObjKey::Res(s2)];
    let wrote = written(base.len(), &section, &path);
    assert!(wrote.contains(&a) && wrote.contains(&form_at(&d, aw)) && wrote.contains(&s2_at));
    assert_eq!(wrote.len(), 2 + parts(&g2), "{wrote:?}");
    let both = [(s1_at, numbers(&g1)), (s2_at, numbers(&g2))];
    assert_eq!(fallback_subsets(&now()), both);
    assert_eq!(bound(&now(), a), BTreeSet::from([helv, s2_at]));
    assert_eq!(subsets_hold(&now()), 3);

    // Each a widget of two, its glyphs a saved subset's: no font written.
    // `ễ` alone draws nothing in Helvetica, which its form doesn't name.
    for (field, widget, aid, value, fonts) in [
        ("b", b, bw, "\u{141}uk", vec![helv, s1_at]),
        ("empty", empty, ew, "\u{1ec5}", vec![s2_at]),
        ("b", b, bw, "\u{143}owak", vec![helv, s1_at]),
    ] {
        fx::fill(&mut d, field, value);
        let (base, section) = appended_in_place(&mut d).unwrap();
        let wrote = written(base.len(), &section, &path);
        assert_eq!(wrote, BTreeSet::from([widget, form_at(&d, aid)]), "{value}");
        assert_eq!(bound(&now(), widget), BTreeSet::from_iter(fonts), "{value}");
        assert_eq!(fallback_subsets(&now()), both, "{value}");
        assert_eq!(d.saved_record().unwrap().fallbacks.len(), 2);
    }
    // What the file shows now: `a`'s `ễ`, `b`'s `Ń`, `empty`'s `ễ`.
    assert_eq!(subsets_hold(&now()), 3);

    // A WinAnsi value: no font.
    fx::fill(&mut d, "a", "Camden \u{d6}l\u{e7}er");
    let (base, section) = appended_in_place(&mut d).unwrap();
    let wrote = written(base.len(), &section, &path);
    assert_eq!(wrote, BTreeSet::from([a, form_at(&d, aw)]));
    assert_eq!(bound(&now(), a), BTreeSet::from([helv]));
    assert_eq!(fallback_subsets(&now()), both);

    // Reopened: no subset known, so one more is written, though the file
    // holds one that draws `Ł`.
    let mut d = crate::open::file(&path, None).unwrap().doc;
    assert!(d.saved_record().unwrap().fallbacks.is_empty());
    let [aw] = widgets_of(&d, "a")[..] else {
        panic!("one widget")
    };
    fx::fill(&mut d, "a", "\u{141}");
    let (base, section) = appended_in_place(&mut d).unwrap();
    let record = d.saved_record().unwrap();
    let [(s3, g3)] = &record.fallbacks[..] else {
        panic!("one subset")
    };
    assert_eq!(numbers(g3), glyphs_of("\u{141}"));
    let s3_at = record.map.keys[&ObjKey::Res(*s3)];
    let wrote = written(base.len(), &section, &path);
    assert_eq!(wrote.len(), 2 + parts(g3), "{wrote:?}");
    let subsets = fallback_subsets(&now());
    assert_eq!(subsets.len(), 3);
    assert_eq!(subsets[2], (s3_at, numbers(g3)));
    assert_eq!(bound(&now(), a), BTreeSet::from([s3_at]));
    subsets_hold(&now());

    // A Rewrite in place: its own subset alone; an Append after it binds it.
    fx::fill(&mut d, "b", "\u{141}\u{143}");
    save_with(
        &mut d,
        &path,
        SaveHow::Rewrite,
        &AskAnswers::default(),
        &NoProgress,
    )
    .unwrap();
    let record = d.saved_record().unwrap();
    let [(s4, g4)] = &record.fallbacks[..] else {
        panic!("the Rewrite's subset")
    };
    assert_eq!(numbers(g4), glyphs_of("\u{141}\u{143}"));
    let s4_at = record.map.keys[&ObjKey::Res(*s4)];
    // Beside it, the file's `empty` widget as it was, which reads no value
    // since the reopen (no save writes `/V` before M10 slice 9), so isn't
    // made anew: its form and that form's subset copied, which the record
    // doesn't list.
    let subsets = fallback_subsets(&now());
    assert_eq!(subsets.len(), 2, "{subsets:?}");
    assert!(subsets.contains(&(s4_at, numbers(g4))), "{subsets:?}");
    assert!(subsets.iter().any(|(_, codes)| *codes == numbers(&g2)));
    fx::fill(&mut d, "a", "\u{143}");
    let (base, section) = appended_in_place(&mut d).unwrap();
    let wrote = written(base.len(), &section, &path);
    assert_eq!(wrote, BTreeSet::from([a, form_at(&d, aw)]));
    assert_eq!(bound(&now(), a), BTreeSet::from([s4_at]));
    // `a`'s `Ń`, `b`'s `ŁŃ`, and the copied `ễ`.
    assert_eq!(subsets_hold(&now()), 4);
}

/// An Append onto a file older than PDF 1.2, which has no Type 0 fonts,
/// says 1.2 in the catalog where it binds the fallback, and the record
/// says it; a WinAnsi value onto the same file writes no catalog.
#[test]
fn an_append_onto_an_older_file_says_the_version_its_subset_needs() {
    let dir = scratch();
    let (mut f, _, _) = two_fields();
    f.pdf.version = "1.1".into();
    let empty = f.text("empty", 0, row(1));
    let bytes = table(&mut f);
    let (path, mut d) = opened(&dir, "a.pdf", &bytes);
    let catalog = d.saved_record().unwrap().map.keys[&ObjKey::Catalog];
    assert_eq!(d.saved_record().unwrap().pdf_version, PdfVersion::new(1, 1));
    fx::fill(&mut d, "a", "Camden");
    let (base, section) = appended_in_place(&mut d).unwrap();
    assert!(!written(base.len(), &section, &path).contains(&catalog));
    assert_eq!(d.saved_record().unwrap().pdf_version, PdfVersion::new(1, 1));

    fx::fill(&mut d, "empty", "\u{141}ukasz");
    let (base, section) = appended_in_place(&mut d).unwrap();
    let wrote = written(base.len(), &section, &path);
    assert!(
        wrote.contains(&catalog) && wrote.contains(&empty),
        "{wrote:?}"
    );
    let now = std::fs::read(&path).unwrap();
    let pdf = lazy(&now, None);
    let version = pdf.catalog().unwrap().get(b"Version").cloned();
    assert_eq!(
        version.as_ref().and_then(Object::as_name),
        Some(b"1.2".as_slice())
    );
    assert_eq!(d.saved_record().unwrap().pdf_version, PdfVersion::CID_FONTS);
    assert_eq!(subsets_hold(&now), 1);
}

/// A text field on page `page` whose one widget is a kid of its own at
/// `rect`, not merged with it, and the widget.
fn kid_field(f: &mut Form, name: &str, page: usize, rect: Rect) -> (ObjectId, ObjectId) {
    let field = f.pdf.add_object(dictionary! {
        "FT" => Object::name("Tx"), "T" => literal(name), "DA" => literal("/Helv 10 Tf 0 g"),
    });
    let r: Vec<Object> = rect.iter().map(|&v| Object::from(v)).collect();
    let kid = f.pdf.add_object(dictionary! {
        "Type" => Object::name("Annot"), "Subtype" => Object::name("Widget"),
        "Rect" => r, "F" => 4, "P" => f.pages[page], "Parent" => field,
    });
    f.dict(field).set("Kids", vec![Object::from(kid)]);
    let mut annots = f.annots(page);
    annots.push(kid);
    f.set_annots(page, &annots);
    let mut fields = f.fields();
    fields.push(field);
    f.set_fields(&fields);
    (field, kid)
}

/// What an overlay of `d` for the screen emits for `scope`.
fn overlaid(d: &Document, page: PageId, scope: EmitScope) -> Emission {
    let st = d.state();
    let inputs = ViewInputs::screen();
    let overlay = Overlay {
        src: st.def(page).unwrap().base.src.clone(),
        inputs: &inputs,
        rotate: RotatePolicy::KeepBase,
        arts: None,
    };
    emit::emit(st, scope, EmitBase::Overlay(&overlay), &[], &NoProgress).unwrap()
}

/// The keys of resources the form `e` writes under `AnnotPart(aid, Ap)`
/// binds a font name to.
fn binds(e: &Emission, aid: AnnotId) -> BTreeSet<ObjKey> {
    let key = ObjKey::AnnotPart(aid, Part::Ap);
    let (_, form) = (e.objects.iter().find(|(k, _)| *k == key))
        .unwrap_or_else(|| panic!("{key:?} isn't written"));
    let SymObject::Built(form) = form else {
        panic!("a built form")
    };
    let fonts = (form.as_stream().unwrap().dict.get(b"Resources"))
        .and_then(Object::as_dict)
        .and_then(|r| r.get(b"Font"))
        .and_then(Object::as_dict)
        .unwrap();
    (fonts.iter())
        .filter_map(|(_, v)| e.syms.key(v.as_reference()?))
        .filter(|k| matches!(k, ObjKey::Res(_)))
        .collect()
}

/// Whether `e` writes `key`.
fn emits(e: &Emission, key: ObjKey) -> bool {
    e.objects.iter().any(|(k, _)| *k == key)
}

/// An overlay's form for a widget in two fonts is the save's, its box,
/// matrix, content and names. Each emission writes the subset of what it
/// draws: a widget merged with its field is the field's object, which an
/// overlay's frame writes for its group's pages, so the frame's one subset
/// holds what every such widget on them draws and a page's delta writes
/// none; a kid widget is its page's, so each page's delta, emitted apart,
/// writes a subset of what it draws, neither the other's, and two with
/// the same glyphs share one by key.
#[test]
fn an_overlay_s_form_is_the_save_s_and_each_emission_binds_its_own_subset() {
    let (mut f, a, b) = two_fields();
    kid_field(&mut f, "ka", 0, row(2));
    kid_field(&mut f, "kb", 1, row(2));
    let bytes = table(&mut f);
    let mut d = open(&bytes, None);
    let widget = |d: &Document, name| {
        let [w] = widgets_of(d, name)[..] else {
            panic!("one widget")
        };
        w
    };
    let (aw, bw, kaw, kbw) = (
        widget(&d, "a"),
        widget(&d, "b"),
        widget(&d, "ka"),
        widget(&d, "kb"),
    );
    fx::fill(&mut d, "a", FALLBACK_TEXT);
    fx::fill(&mut d, "b", "\u{143}owak");
    fx::fill(&mut d, "ka", "\u{17a}le");
    fx::fill(&mut d, "kb", "\u{15b}wiat");
    let out = rewrite(&d, Profile::default());
    let pdf = lazy(&out.bytes, None);
    let (p0, p1) = (d.page(0).unwrap(), d.page(1).unwrap());
    for (page, aid, widget) in [(p0, aw, a), (p1, bw, b)] {
        let over = overlay_form(&d, page, aid).expect("an overlay's form");
        assert_eq!(over, saved_form(&pdf, widget).0, "{aid:?}");
    }
    assert_eq!(fallback_subsets(&out.bytes).len(), 1, "a save's one");
    assert_eq!(subsets_hold(&out.bytes), 5, "Ł, ễ, Ń, ź, ś");

    // The frame: the merged widgets' subset.
    let frame = overlaid(&d, p0, EmitScope::Frame([p0, p1].into()));
    let (id, glyphs) = frame.fallback.clone().expect("the frame's subset");
    assert_eq!(numbers(&glyphs), glyphs_of("\u{141}\u{1ec5}\u{143}"));
    assert!(binds(&frame, aw).contains(&ObjKey::Res(id)));
    assert!(binds(&frame, bw).contains(&ObjKey::Res(id)));
    assert!(emits(&frame, ObjKey::Res(id)));

    // The deltas: each kid's, apart.
    let (e0, e1) = (
        overlaid(&d, p0, EmitScope::Delta(p0)),
        overlaid(&d, p1, EmitScope::Delta(p1)),
    );
    let (id0, g0) = e0.fallback.clone().expect("page 1's subset");
    let (id1, g1) = e1.fallback.clone().expect("page 2's subset");
    assert_eq!(numbers(&g0), glyphs_of("\u{17a}"));
    assert_eq!(numbers(&g1), glyphs_of("\u{15b}"));
    assert_ne!(id0, id1);
    assert!(binds(&e0, kaw).contains(&ObjKey::Res(id0)));
    assert!(binds(&e1, kbw).contains(&ObjKey::Res(id1)));
    assert!(emits(&e0, ObjKey::Res(id0)) && !emits(&e0, ObjKey::Res(id1)));
    assert!(emits(&e1, ObjKey::Res(id1)) && !emits(&e1, ObjKey::Res(id0)));
    assert!(!emits(&e0, ObjKey::AnnotPart(aw, Part::Ap)), "the frame's");

    fx::fill(&mut d, "kb", "\u{17a}");
    let (e0, e1) = (
        overlaid(&d, p0, EmitScope::Delta(p0)),
        overlaid(&d, p1, EmitScope::Delta(p1)),
    );
    assert_eq!(
        e0.fallback.unwrap().0,
        e1.fallback.unwrap().0,
        "shared by key"
    );
}

/// A value with a cluster neither the field's font nor the fallback
/// draws, a CJK ideograph (`王`) or an emoji (`😀`), leaves its widget
/// unmade as `unencodable`: no `/AP`, no subset, nothing in the record;
/// the warning says the kind, never the text. The op refuses such a value,
/// so it's put as only a file gives one (`fx::put_value`).
#[test]
fn a_value_neither_font_draws_is_unencodable() {
    let (mut f, a, _) = two_fields();
    let bytes = table(&mut f);
    for value in ["\u{141}ukasz \u{738b}", "\u{1f600}"] {
        let mut d = open(&bytes, None);
        fx::put_value(&mut d, "a", Some(Value::Text(value.into())));
        let (out, said) = warned(|| rewrite(&d, Profile::default()));
        let pdf = lazy(&out.bytes, None);
        assert!(ap_dict(&pdf, a).is_none(), "{value}: no /AP");
        assert_eq!(fallback_subsets(&out.bytes), [], "{value}");
        assert_eq!(subsets_hold(&out.bytes), 0, "{value}");
        assert!(out.record.fallbacks.is_empty(), "{value}");
        let unmade: Vec<&String> = said.iter().filter(|w| w.contains("appearance")).collect();
        assert_eq!(unmade.len(), 1, "{said:?}");
        assert!(unmade[0].contains("unencodable"), "{said:?}");
        assert!(
            !said
                .iter()
                .any(|w| w.contains(value) || w.contains("ukasz"))
        );
    }
}

/// The same state in two fonts is written as the same bytes every time,
/// and from other threads: the subset, its key and its number with them;
/// and an overlay of a page of it is the same objects.
#[test]
fn a_state_in_two_fonts_is_written_alike_every_time_and_from_any_thread() {
    let (mut f, _, _) = two_fields();
    let bytes = table(&mut f);
    let mut d = open(&bytes, None);
    fx::fill(&mut d, "a", FALLBACK_TEXT);
    fx::fill(&mut d, "b", "\u{143}owak");
    let input = SaveInput::detached(&d.snapshot());
    let once = super::tests::write(&input, Profile::default()).bytes;
    assert_eq!(fallback_subsets(&once).len(), 1);
    assert_eq!(subsets_hold(&once), 3);
    let again = std::thread::scope(|scope| {
        let threads: Vec<_> = (0..2)
            .map(|_| scope.spawn(|| super::tests::write(&input, Profile::default()).bytes))
            .collect();
        (threads.into_iter())
            .map(|t| t.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert_eq!(super::tests::write(&input, Profile::default()).bytes, once);
    assert_eq!(again, [once.clone(), once]);

    let page = d.page(0).unwrap();
    let delta = || {
        let st = &input.target;
        let inputs = ViewInputs::screen();
        let overlay = Overlay {
            src: st.def(page).unwrap().base.src.clone(),
            inputs: &inputs,
            rotate: RotatePolicy::KeepBase,
            arts: None,
        };
        let scope = EmitScope::Pages([page].into());
        let e = emit::emit(st, scope, EmitBase::Overlay(&overlay), &[], &NoProgress);
        let e = e.unwrap();
        assert!(e.fallback.is_some(), "the page's form draws in it");
        let built: Vec<(ObjKey, Option<Object>)> = (e.objects.into_iter())
            .map(|(key, obj)| match obj {
                SymObject::Built(obj) => (key, Some(obj)),
                _ => (key, None),
            })
            .collect();
        (built, e.syms.keys().to_vec(), e.fallback.map(|(id, _)| id))
    };
    let first = delta();
    let other = std::thread::scope(|scope| scope.spawn(delta).join().unwrap());
    assert_eq!(delta(), first);
    assert_eq!(other, first);
}
