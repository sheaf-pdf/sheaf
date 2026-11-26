//! The form reader on forms built in code, written and read back from
//! their sources: every kind of field and what it inherits, its widgets on
//! pages, the facts, and damaged trees, which read as not fillable; and
//! hybrid-reference files, built and from the corpus. Then any tree at
//! all, as a proptest.

use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use crate::pdf::{Object, ObjectId, Stream, StreamExt, deflate_best, dictionary, literal};
use proptest::prelude::*;

use super::*;
use crate::forms::corpus::Corpus;
use crate::forms::field::{annot_flags, flags};
use crate::forms::fixture::{Form, every_kind, hybrid_reference, hybrid_update, row, source};
use crate::source::Limits;
use crate::store::{BlobKind, Store};

/// The form as a file holds it, written and read back.
fn read_back(f: &mut Form) -> FileForm {
    read(&f.source())
}

/// [`read_back`] within `limits`.
fn read_within(f: &mut Form, limits: FormLimits) -> FileForm {
    let store = Store::with_limits(Limits {
        forms: limits,
        ..Limits::DEFAULT
    });
    let kind = BlobKind::file(Path::new("form.pdf"), None);
    read(&store.open(f.bytes().into(), kind).unwrap())
}

/// A placeholder for a real past f64's range, which the writer can't write:
/// [`with_huge`] puts a 9, 318 zeros and a point in its place, which reads
/// as infinity.
fn huge() -> Object {
    literal("~".repeat(320))
}

/// `bytes` with each [`huge`] placeholder swapped for its real, byte for
/// byte, so no offset moves. A string needs no space around it, so the
/// real brings its own.
fn with_huge(mut bytes: Vec<u8>) -> Vec<u8> {
    let placeholder = format!("({})", "~".repeat(320));
    let real = format!(" 9{}. ", "0".repeat(placeholder.len() - 4));
    while let Some(at) = bytes
        .windows(placeholder.len())
        .position(|w| w == placeholder.as_bytes())
    {
        bytes[at..at + real.len()].copy_from_slice(real.as_bytes());
    }
    bytes
}

fn field<'a>(form: &'a FileForm, name: &str) -> &'a Field {
    form.fields
        .iter()
        .find(|f| f.name == name)
        .unwrap_or_else(|| panic!("no field {name}"))
}

fn text(s: &str) -> Option<Value> {
    Some(Value::Text(s.into()))
}

fn state(s: &str) -> Option<Value> {
    Some(Value::State(s.into()))
}

fn js(script: &str) -> Object {
    Object::from(dictionary! {
        "S" => Object::name("JavaScript"), "JS" => literal(script),
    })
}

#[test]
fn every_kind_of_field() {
    let mut f = every_kind();
    let form = read_back(&mut f);
    let fields: Vec<(&str, FieldKind, Option<Value>)> = form
        .fields
        .iter()
        .map(|f| (f.name.as_str(), f.kind, f.value.clone()))
        .collect();
    use FieldKind::*;
    assert_eq!(
        fields,
        [
            ("form1[0].name[0]", Text, text("Ann Lee")),
            ("form1[0].empty[0]", Text, None),
            ("form1[0].on[0]", CheckBox, state("Yes")),
            ("form1[0].off[0]", CheckBox, state("Off")),
            ("form1[0].pick[0]", Radio, state("1")),
            ("form1[0].state[0]", Combo, text("NY")),
            ("form1[0].kind[0]", List, text("1099")),
            ("form1[0].print[0]", PushButton, None),
            ("form1[0].sig[0]", Signature, None),
        ]
    );
    // Every node in tree order, a terminal's id its node's: the parent
    // first, then its kids.
    let nodes: Vec<(&str, Option<FieldId>)> = (form.nodes.iter())
        .map(|n| (&*n.partial, n.parent))
        .collect();
    assert_eq!(
        nodes[..2],
        [("form1[0]", None), ("name[0]", Some(FieldId(0)))]
    );
    assert!(nodes[1..].iter().all(|&(_, up)| up == Some(FieldId(0))));
    assert_eq!(form.nodes[0].kids, (1..10).map(FieldId).collect::<Vec<_>>());
    assert_eq!(form.nodes[0].object, f.fields()[0]);
    let ids: Vec<u64> = form.fields.iter().map(|f| f.id.0).collect();
    assert_eq!(ids, (1..10).collect::<Vec<_>>());
    let name = field(&form, "form1[0].name[0]");
    assert_eq!(name.da.as_deref(), Some("/Helv 10 Tf 0 g"));
    assert_eq!(
        (name.align, name.max_len, name.tooltip.as_ref()),
        (None, None, None)
    );
    let combo = field(&form, "form1[0].state[0]");
    let items: Vec<(&str, &str)> = combo
        .options
        .iter()
        .map(|c| (&*c.export, &*c.shown))
        .collect();
    assert_eq!(items, [("CA", "CA"), ("NY", "NY")]);
    assert!(combo.has(flags::COMBO));

    // Widgets by page, in /Annots order, each with its field.
    let on_page = |i: u32| -> Vec<(u64, usize)> {
        form.widgets[&i]
            .iter()
            .map(|w| (w.field.0, w.slot))
            .collect()
    };
    assert_eq!(on_page(0), [(1, 0), (2, 1), (3, 2), (4, 3), (5, 4), (5, 5)]);
    assert_eq!(on_page(1), [(6, 0), (7, 1), (8, 2), (9, 3)]);
    // Each is its field's own dictionary, but for the radio group's kids.
    let merged: Vec<bool> = form.widgets[&0].iter().map(|w| w.merged).collect();
    assert_eq!(merged, [true, true, true, true, false, false]);
    assert!(form.widgets[&1].iter().all(|w| w.merged));
    assert!(form.fields.iter().all(|f| f.outside == 0));
    let first = &form.widgets[&0][0];
    assert_eq!(first.rect, row(0));
    assert_eq!(first.flags, annot_flags::PRINT);
    assert!(first.has_appearance && first.states.is_empty());
    let picks: Vec<(Option<&str>, &[String])> = form.widgets[&0][4..]
        .iter()
        .map(|w| (w.state.as_deref(), w.states.as_slice()))
        .collect();
    let (zero, one) = (
        ["0".to_string(), "Off".into()],
        ["1".to_string(), "Off".into()],
    );
    assert_eq!(picks, [(Some("Off"), &zero[..]), (Some("1"), &one[..])]);
    let button = &form.widgets[&1][2];
    assert_eq!(button.mk.caption.as_deref(), Some("Print"));

    let facts = &form.facts;
    assert_eq!(facts.class, FormClass::AcroForm);
    assert_eq!((facts.fields, facts.widgets), (9, 10));
    assert_eq!(facts.kinds[&Text], 2);
    assert_eq!(facts.kinds[&CheckBox], 2);
    assert_eq!((facts.signature_fields, facts.signed), (1, 0));
    assert_eq!(facts.fonts, ["Helv", "ZaDb"]);
    assert_eq!(facts.da.as_deref(), Some("/Helv 0 Tf 0 g"));
    assert!(facts.is_fillable() && !facts.has_scripts());
    assert_eq!(
        facts.to_string(),
        "AcroForm; 9 fields; no scripts; 1 signature field, 0 signed"
    );
}

#[test]
fn fields_inherit_from_their_parents() {
    let mut f = Form::new(1);
    let a = f.text("a", 0, row(0));
    let b = f.text("b", 0, row(1));
    for k in [a, b] {
        let d = f.dict(k);
        d.remove(b"FT");
        d.remove(b"DA");
    }
    // b's own keys win over its parent's.
    f.dict(b).set("FT", Object::name("Ch"));
    f.dict(b).set("Q", 1);
    f.dict(b).set("TU", literal("Choose one"));
    let parent = f.group("p", &[a, b]);
    let opt = vec![literal("x"), vec![literal("e"), literal("Shown")].into()];
    let p = f.dict(parent);
    p.set("FT", Object::name("Tx"));
    p.set("Ff", flags::MULTILINE as i64);
    p.set("V", literal("shared"));
    p.set("DV", literal("default"));
    p.set("DA", literal("/Cour 9 Tf 0 0 1 rg"));
    p.set("Q", 2);
    p.set("MaxLen", 5000);
    p.set("Opt", opt);
    p.set("TU", literal("not inherited"));
    let form = read_back(&mut f);
    let (a, b) = (field(&form, "p.a"), field(&form, "p.b"));
    assert_eq!(a.kind, FieldKind::Text);
    assert_eq!(b.kind, FieldKind::List);
    assert!(a.has(flags::MULTILINE) && b.has(flags::MULTILINE));
    assert_eq!(
        (a.value.clone(), a.default.clone()),
        (text("shared"), text("default"))
    );
    assert_eq!(a.da.as_deref(), Some("/Cour 9 Tf 0 0 1 rg"));
    assert_eq!((a.align, b.align), (Some(2), Some(1)));
    assert_eq!(a.max_len, Some(5000));
    assert_eq!(a.options.len(), 2);
    assert_eq!(
        (&*a.options[1].export, &*a.options[1].shown),
        ("e", "Shown")
    );
    assert_eq!(a.tooltip, None);
    assert_eq!(b.tooltip.as_deref(), Some("Choose one"));
    // Read once, and shared.
    let (Some(Value::Text(va)), Some(Value::Text(vb))) = (&a.value, &b.value) else {
        panic!()
    };
    assert!(Arc::ptr_eq(va, vb));
    assert!(Arc::ptr_eq(&a.options, &b.options));
}

#[test]
fn the_forms_own_keys() {
    let mut f = every_kind();
    let form1 = f.fields()[0];
    let kids = f.widgets_of(form1);
    let co = vec![
        Object::from(kids[1]),
        // A parent isn't a field that calculates.
        Object::from(form1),
        Object::from(kids[0]),
    ];
    let d = f.dict(f.acroform);
    d.set("NeedAppearances", true);
    d.set("Q", 1);
    d.set("CO", co);
    let facts = read_back(&mut f).facts;
    assert!(facts.need_appearances);
    assert_eq!(facts.align, 1);
    assert_eq!(facts.calculation_order, [FieldId(2), FieldId(1)]);
}

#[test]
fn the_class_of_a_form() {
    let class = |f: &mut Form| read_back(f).facts.class;
    assert_eq!(class(&mut every_kind()), FormClass::AcroForm);
    let mut hybrid = every_kind();
    hybrid.xfa();
    assert_eq!(class(&mut hybrid), FormClass::Hybrid);
    // Dynamic: the XFA is the form, fields or not.
    hybrid.dict(hybrid.catalog).set("NeedsRendering", true);
    let form = read_back(&mut hybrid);
    assert_eq!(
        (form.facts.class, form.fields.len()),
        (FormClass::XfaOnly, 9)
    );
    assert!(!form.facts.is_fillable());
    let mut only = every_kind();
    only.xfa_only();
    let facts = read_back(&mut only).facts;
    assert_eq!(facts.class, FormClass::XfaOnly);
    assert!(facts.xfa && facts.dynamic && facts.fields == 0);
    assert_eq!(facts.to_string(), "XFA only, which sheaf can't fill");
    // A static XFA copy with no AcroForm fields.
    let mut stat = every_kind();
    stat.xfa();
    stat.set_fields(&[]);
    assert_eq!(class(&mut stat), FormClass::XfaOnly);
    // A damaged hybrid form is hybrid, though none of its fields were read
    // before the damage.
    let mut damaged = every_kind();
    damaged.xfa();
    let mut chain = damaged.text("leaf", 0, row(9));
    for i in 0..40 {
        chain = damaged.group(&format!("n{i}"), &[chain]);
    }
    let mut fields = damaged.fields();
    fields.rotate_right(1);
    damaged.set_fields(&fields);
    let facts = read_back(&mut damaged).facts;
    assert!(facts.damaged.is_some() && facts.xfa);
    assert_eq!((facts.class, facts.fields), (FormClass::Hybrid, 0));

    let mut empty = Form::new(1);
    assert_eq!(class(&mut empty), FormClass::None);
    empty.dict(empty.catalog).remove(b"AcroForm");
    let facts = read_back(&mut empty).facts;
    assert_eq!(facts.class, FormClass::None);
    assert_eq!(facts.to_string(), "None");
    let labels = [
        FormClass::None,
        FormClass::AcroForm,
        FormClass::Hybrid,
        FormClass::XfaOnly,
    ]
    .map(FormClass::label);
    assert_eq!(labels, ["no form", "AcroForm", "hybrid", "XFA-only"]);
}

#[test]
fn scripts_by_trigger_and_at_document_level() {
    let mut f = Form::new(1);
    let a = f.text("a", 0, row(0));
    f.dict(a).set(
        "AA",
        dictionary! {
            "K" => js("AFNumber_Keystroke(2,0,0,0,\"\",true);"),
            "F" => js("AFNumber_Format(2,0,0,0,\"\",true);"),
            "Fo" => js("app.alert('hi');"),
        },
    );
    let b = f.text("b", 0, row(1));
    f.dict(b).set(
        "AA",
        dictionary! {
            "C" => js("AFSimple_Calculate(\"SUM\", new Array (\"a\"));"),
            // Blank scripts don't count.
            "V" => js("  "),
        },
    );
    let c = f.push_button("c", 0, row(2), "Print");
    f.dict(c).set("A", js("this.print();"));
    let d = f.text("d", 0, row(3));
    f.dict(d).set(
        "AA",
        dictionary! { "K" => dictionary! { "S" => Object::name("ResetForm") } },
    );
    // A script in a stream, compressed.
    let script = "event.value = util.printx(\"999-99-9999\", event.value);";
    let mut stream = Stream::new(dictionary! {}, script.as_bytes().to_vec());
    stream.compress();
    let stream = f.pdf.add_object(stream);
    let e = f.text("e", 0, row(4));
    f.dict(e).set(
        "AA",
        dictionary! { "F" => dictionary! { "S" => Object::name("JavaScript"), "JS" => stream } },
    );
    // Document scripts: Adobe's version check, and one in a kid.
    let kid = f.pdf.add_object(dictionary! {
        "Names" => vec![literal("init"), js("var x = 1;")],
    });
    f.dict(f.catalog).set(
        "Names",
        dictionary! { "JavaScript" => dictionary! {
            "Names" => vec![literal("!ADBE::0100_VersChkStrings"), js("")],
            "Kids" => vec![Object::from(kid)],
        } },
    );
    let form = read_back(&mut f);
    let facts = &form.facts;
    assert_eq!(
        facts.scripts,
        ScriptCounts {
            keystroke: 1,
            format: 2,
            validate: 0,
            calculate: 1,
            fields: 4,
        }
    );
    assert_eq!((facts.document_scripts, facts.version_checks), (1, 1));
    assert!(facts.has_scripts());
    assert_eq!(
        facts.to_string(),
        "AcroForm; 5 fields; scripts on 4 fields and 1 at document level"
    );
    assert_eq!(field(&form, "e").script(Trigger::Format), Some(script));
    assert_eq!(field(&form, "b").script(Trigger::Validate), None);
    let reset = &field(&form, "d").actions[0];
    assert_eq!(reset.0, Trigger::Keystroke);
    assert_eq!(
        (reset.1.kind.as_str(), reset.1.script.is_none()),
        ("ResetForm", true)
    );
    // Widget triggers are the widget's, even when it's the field's
    // dictionary too.
    let widget = &form.widgets[&0][0];
    assert_eq!(widget.actions.len(), 1);
    assert_eq!(widget.actions[0].0, Trigger::Focus);
    assert_eq!(form.widgets[&0][2].actions[0].0, Trigger::Activate);
}

#[test]
fn shared_document_scripts_count_once() {
    // Two nodes share one /Names array, and a third, a direct object in a
    // /Kids array, lists that array as its own kids.
    let mut f = Form::new(1);
    let names = f
        .pdf
        .add_object(vec![literal("a"), js("var a;"), literal("b"), js("var b;")]);
    let kids = f.pdf.new_object_id();
    f.pdf.objects.insert(
        kids,
        Object::from(vec![
            Object::from(dictionary! { "Names" => names }),
            Object::from(dictionary! { "Names" => names }),
            Object::from(dictionary! { "Kids" => kids }),
        ]),
    );
    f.dict(f.catalog).set(
        "Names",
        dictionary! { "JavaScript" => dictionary! { "Kids" => kids } },
    );
    let facts = read_back(&mut f).facts;
    assert_eq!((facts.document_scripts, facts.version_checks), (2, 0));
}

#[test]
fn usage_rights_signatures_and_a_certification() {
    let mut f = every_kind();
    f.usage_rights();
    let facts = read_back(&mut f).facts;
    assert!(facts.usage_rights && !facts.certified);
    assert_eq!(
        (facts.sig_flags, facts.signature_fields, facts.signed),
        (2, 1, 0)
    );
    assert!(facts.to_string().contains("; Reader usage rights; "));

    let mut f = Form::new(1);
    let sig = f.signature("Signature1", 0, row(0));
    f.sign(sig);
    let form = read_back(&mut f);
    assert_eq!(form.fields[0].value, Some(Value::Signed));
    assert_eq!((form.facts.signature_fields, form.facts.signed), (1, 1));

    let mut f = Form::new(1);
    let sig = f.signature("Author", 0, row(0));
    f.certify(sig, 2);
    let facts = read_back(&mut f).facts;
    assert!(facts.certified && !facts.usage_rights);
    assert_eq!(
        facts.to_string(),
        "AcroForm; 1 field; no scripts; certified; 1 signature field, 1 signed"
    );
}

#[test]
fn values_of_every_shape() {
    let mut f = Form::new(1);
    // One value object, many fields.
    let shared = f.pdf.add_object(literal("same"));
    let many: Vec<ObjectId> = (0..1000)
        .map(|i| {
            let t = f.text(&format!("t{i}"), 0, [0.0, 0.0, 10.0, 10.0]);
            f.dict(t).set("V", shared);
            t
        })
        .collect();
    let list = f.choice("list", 0, row(1), &["a", "b", "c"], false);
    let items = vec![literal("a"), literal("c")];
    f.dict(list).set("V", items);
    f.dict(list).set("Ff", flags::MULTI_SELECT as i64);
    // A check box's state written as a string; a text field's as a name.
    let cb = f.check_box("cb", 0, row(2), false);
    f.dict(cb).set("V", literal("Yes"));
    let named = f.text("named", 0, row(3));
    f.dict(named).set("V", Object::name(b"Plain"));
    // UTF-16, and text in a stream.
    let utf16 = f.text("utf16", 0, row(4));
    let mut bytes = vec![0xFE, 0xFF];
    bytes.extend("Łukasz".encode_utf16().flat_map(u16::to_be_bytes));
    f.dict(utf16).set("V", Object::hex(bytes));
    let streamed = f.text("streamed", 0, row(5));
    let s = f
        .pdf
        .add_object(Stream::new(dictionary! {}, b"long text".to_vec()));
    f.dict(streamed).set("V", s);
    let number = f.text("number", 0, row(6));
    f.dict(number).set("V", 42);
    let form = read_back(&mut f);
    assert_eq!(form.fields.len(), many.len() + 6);
    let Some(Value::Text(first)) = &form.fields[0].value else {
        panic!()
    };
    assert!(form.fields[..1000].iter().all(|f| matches!(
        &f.value, Some(Value::Text(t)) if Arc::ptr_eq(t, first)
    )));
    let Some(Value::Items(picked)) = &field(&form, "list").value else {
        panic!()
    };
    assert_eq!(picked.iter().map(|i| &**i).collect::<Vec<_>>(), ["a", "c"]);
    assert_eq!(field(&form, "cb").value, state("Yes"));
    assert_eq!(field(&form, "named").value, text("Plain"));
    assert_eq!(field(&form, "utf16").value, text("Łukasz"));
    assert_eq!(field(&form, "streamed").value, text("long text"));
    assert_eq!(field(&form, "number").value, None);
}

#[test]
fn widgets_are_finite_and_clamped() {
    let mut f = Form::new(1);
    // A link first: slots count every annotation, as PDFium does.
    let link = f.pdf.add_object(dictionary! {
        "Type" => Object::name("Annot"),
        "Subtype" => Object::name("Link"),
    });
    f.set_annots(0, &[link]);
    let a = f.text("a", 0, row(0));
    let rect = [300.into(), huge(), (-2e9).into(), 10.into()];
    let d = f.dict(a);
    d.set("Rect", rect.to_vec());
    d.set(
        "MK",
        dictionary! {
            "BG" => vec![Object::from(2.0)],
            "BC" => vec![Object::from(0.5), huge(), Object::from(-1)],
            "R" => -90,
            "CA" => literal("4"),
        },
    );
    d.set(
        "BS",
        dictionary! { "W" => Object::from(1e9), "S" => Object::name("U") },
    );
    d.set("F", 4 | 32);
    let b = f.text("b", 0, row(1));
    let d = f.dict(b);
    d.set("Rect", vec![1.into(), 2.into(), 3.into()]);
    d.set(
        "MK",
        dictionary! {
            "BG" => Vec::<Object>::new(),
            "BC" => vec![1.into(), 0.into()],
            "R" => 45,
        },
    );
    d.set("Border", vec![0.into(), 0.into(), 2.into()]);
    // A radio button's own /DA, apart from its field's.
    let pick = f.radio("pick", 0, &[row(2)], Some(0));
    let kid = f.widgets_of(pick)[0];
    f.dict(kid).set("DA", literal("/ZaDb 12 Tf 1 0 0 rg"));
    let form = read(&source(with_huge(f.bytes())));
    let [wa, wb, wk] = &form.widgets[&0][..] else {
        panic!("{:?}", form.widgets)
    };
    assert_eq!((wa.slot, wb.slot, wk.slot), (1, 2, 3));
    assert_eq!(wa.rect, [-MAX_COORD, 0.0, 300.0, 10.0]);
    assert_eq!(wa.flags, annot_flags::PRINT | annot_flags::NO_VIEW);
    assert_eq!(wa.mk.background, Some(Color::Gray(1.0)));
    assert_eq!(wa.mk.border_color, Some(Color::Rgb([0.5, 0.0, 0.0])));
    assert_eq!(wa.mk.rotation, 270);
    assert_eq!(wa.mk.caption.as_deref(), Some("4"));
    assert_eq!(
        wa.border,
        Some(Border {
            width: 100.0,
            style: BorderStyle::Underline,
            dash: Vec::new(),
        })
    );
    assert_eq!(wa.da, None);
    assert_eq!(wb.rect, [0.0; 4]);
    assert_eq!(
        (wb.mk.background, wb.mk.border_color, wb.mk.rotation),
        (None, None, 0)
    );
    assert_eq!(wb.border, Some(Border::solid(2.0)));
    assert_eq!(wk.da.as_deref(), Some("/ZaDb 12 Tf 1 0 0 rg"));
    assert_eq!(wk.field, field(&form, "pick").id);
    // None is a kid with a /Q of its own.
    assert_eq!((wa.align, wb.align, wk.align), (None, None, None));
}

/// A border's dashes are `/BS /D`'s, `[3]` for a dashed one that gives
/// none, or a `/Border`'s fourth element, which makes it dashed: each
/// number followed where it's a reference, the finite ones not negative,
/// at most 32.
#[test]
fn a_border_s_dashes_are_read_as_pdf_annots_reads_them() {
    let mut f = Form::new(1);
    let three = f.pdf.add_object(3);
    let bs = |s: &str, d: Option<Vec<Object>>| {
        let mut bs = dictionary! { "W" => 1, "S" => Object::name(s) };
        if let Some(d) = d {
            bs.set("D", d);
        }
        bs
    };
    let many: Vec<Object> = (1..=40).map(Object::from).collect();
    let cases: [(&str, Dictionary); 9] = [
        ("dashed", bs("D", None)),
        (
            "own",
            bs("D", Some(vec![4.into(), (-1).into(), three.into()])),
        ),
        ("odd", bs("D", Some(vec![literal("x"), huge(), 2.5.into()]))),
        ("solid", bs("S", Some(vec![5.into()]))),
        ("empty", bs("D", Some(Vec::new()))),
        ("many", bs("D", Some(many))),
        ("legacy", dictionary! {}),
        ("legacy empty", dictionary! {}),
        ("legacy name", dictionary! {}),
    ];
    for (i, (name, bs)) in cases.iter().enumerate() {
        let id = f.text(name, 0, row(i));
        if !bs.is_empty() {
            f.dict(id).set("BS", bs.clone());
        }
    }
    let old_border = |f: &mut Form, name: &str, fourth: Object| {
        let id = (f.fields().into_iter())
            .find(|&id| f.pdf.get_dictionary(id).unwrap().get(b"T") == Some(&literal(name)))
            .unwrap();
        let w = f.pdf.add_object(2);
        f.dict(id)
            .set("Border", vec![0.into(), 0.into(), Object::from(w), fourth]);
    };
    old_border(&mut f, "legacy", vec![Object::from(2), 1.into()].into());
    old_border(&mut f, "legacy empty", Vec::<Object>::new().into());
    old_border(&mut f, "legacy name", Object::name("D"));
    let form = read(&source(with_huge(f.bytes())));
    let widgets = &form.widgets[&0];
    let read: Vec<(Option<BorderStyle>, Vec<f32>)> = (widgets.iter())
        .map(|w| {
            let b = w.border.as_ref();
            (
                b.map(|b| b.style),
                b.map_or_else(Vec::new, |b| b.dash.clone()),
            )
        })
        .collect();
    let (d, s) = (Some(BorderStyle::Dashed), Some(BorderStyle::Solid));
    assert_eq!(
        read,
        [
            (d, vec![3.0]),
            (d, vec![4.0, 3.0]),
            (d, vec![2.5]),
            (s, vec![5.0]),
            (d, vec![3.0]),
            (d, (1..=32).map(|n| n as f32).collect()),
            (d, vec![2.0, 1.0]),
            (d, vec![3.0]),
            (s, vec![]),
        ]
    );
    // The legacy borders' widths, references, are followed.
    assert!((widgets[6..].iter()).all(|w| w.border.as_ref().is_some_and(|b| b.width == 2.0)));
}

/// A `/Border` of three or more elements is a border whatever its third
/// is: a width that isn't a number is 0, and the fourth element is still
/// its dash array, as `pdf-annots` reads one; a shorter array is no border
/// record, and the appearance draws width 1, solid.
#[test]
fn a_border_whose_width_isn_t_a_number_is_width_0() {
    let mut f = Form::new(1);
    let dash: Object = vec![Object::from(3), 2.into()].into();
    let cases: [(&str, Vec<Object>); 5] = [
        (
            "name",
            vec![0.into(), 0.into(), Object::name("x"), dash.clone()],
        ),
        ("null", vec![0.into(), 0.into(), Object::null()]),
        (
            "missing",
            vec![0.into(), 0.into(), f.pdf.new_object_id().into(), dash],
        ),
        ("short", vec![0.into(), 0.into()]),
        ("empty", Vec::new()),
    ];
    for (i, (name, border)) in cases.iter().enumerate() {
        let id = f.text(name, 0, row(i));
        f.dict(id).set("Border", border.clone());
    }
    let form = read(&source(f.bytes()));
    let read: Vec<Option<Border>> = (form.widgets[&0].iter())
        .map(|w| w.border.clone())
        .collect();
    let dashed = Border {
        width: 0.0,
        style: BorderStyle::Dashed,
        dash: vec![3.0, 2.0],
    };
    assert_eq!(
        read,
        [
            Some(dashed.clone()),
            Some(Border::solid(0.0)),
            Some(dashed),
            None,
            None,
        ]
    );
}

/// A kid widget's own `/Q` is read beside its field's, which a merged
/// widget's dictionary holds; one outside 0 to 2 isn't.
#[test]
fn a_kid_widget_s_own_q_is_read() {
    let mut f = Form::new(1);
    let merged = f.text("merged", 0, row(0));
    f.dict(merged).set("Q", 2);
    let parent = f.text("parent", 0, row(1));
    f.dict(parent).set("Q", 1);
    let mut kids = Vec::new();
    for (i, q) in [Some(2), None, Some(7)].into_iter().enumerate() {
        let r: Vec<Object> = row(2 + i).iter().map(|&v| Object::from(v)).collect();
        let kid = f.pdf.add_object(dictionary! {
            "Type" => Object::name("Annot"), "Subtype" => Object::name("Widget"),
            "Rect" => r, "P" => f.pages[0], "Parent" => parent,
        });
        if let Some(q) = q {
            f.dict(kid).set("Q", q);
        }
        kids.push(kid);
    }
    let d = f.dict(parent);
    for key in ["Subtype", "Rect", "F", "P", "Type"] {
        d.remove(key.as_bytes());
    }
    d.set(
        "Kids",
        kids.iter().map(|&k| Object::from(k)).collect::<Vec<_>>(),
    );
    let annots: Vec<ObjectId> = (f.annots(0).into_iter().filter(|&a| a != parent))
        .chain(kids)
        .collect();
    f.set_annots(0, &annots);
    let form = read_back(&mut f);
    let aligns: Vec<Option<u8>> = form.widgets[&0].iter().map(|w| w.align).collect();
    assert_eq!(aligns, [None, Some(2), None, None]);
    assert_eq!(field(&form, "merged").align, Some(2));
    assert_eq!(field(&form, "parent").align, Some(1));
}

/// A `/Rect`'s numbers and an `/MK` color's parts that are references are
/// followed, as PDFium follows them.
#[test]
fn a_widget_s_numbers_that_are_references_are_followed() {
    let mut f = Form::new(1);
    let a = f.text("a", 0, row(0));
    let [x0, y0, x1, y1] = [10, 20, 110, 40].map(|v| f.pdf.add_object(v));
    let half = f.pdf.add_object(0.5);
    let d = f.dict(a);
    d.set(
        "Rect",
        vec![Object::from(x0), y0.into(), x1.into(), y1.into()],
    );
    d.set(
        "MK",
        dictionary! { "BG" => vec![Object::from(half)], "BC" => vec![Object::from(0), half.into(), 1.into()] },
    );
    let form = read_back(&mut f);
    let w = &form.widgets[&0][0];
    assert_eq!(w.rect, [10.0, 20.0, 110.0, 40.0]);
    assert_eq!(w.mk.background, Some(Color::Gray(0.5)));
    assert_eq!(w.mk.border_color, Some(Color::Rgb([0.0, 0.5, 1.0])));
}

/// A field is found by its object, as read.
#[test]
fn a_field_is_found_by_its_object() {
    let mut f = every_kind();
    let form = read_back(&mut f);
    for field in &form.fields {
        assert_eq!(form.field_by_object(field.object), Some(field));
    }
    assert_eq!(form.field_by_object(ObjectId::new(99_999, 0)), None);
}

/// A widget is found by its object, as read, a widget on two pages by its
/// first.
#[test]
fn a_widget_is_found_by_its_object() {
    let mut f = every_kind();
    let page = f.pages[0];
    let root = f.dict(page).get(b"Parent").unwrap().as_reference().unwrap();
    let mut kids = f
        .pdf
        .get_dictionary(root)
        .unwrap()
        .get(b"Kids")
        .unwrap()
        .clone();
    kids.as_array_mut().unwrap().push(page.into());
    f.dict(root).set("Kids", kids);
    f.dict(root).set("Count", 3);
    let form = read_back(&mut f);
    assert_eq!(form.widgets.len(), 3, "the first page listed again");
    for w in form.widgets.values().flatten() {
        let found = form.widget_by_object(w.object).expect("found");
        assert_eq!(found, w, "on its pages alike");
        let first = form
            .widgets
            .values()
            .flatten()
            .find(|o| o.object == w.object);
        assert!(std::ptr::eq(found, first.unwrap()), "its first page's");
    }
    assert_eq!(form.widget_by_object(ObjectId::new(99_999, 0)), None);
}

/// A widget's place among its field's widgets: its own dictionary's 0, or
/// in `/Kids` order among the kids that are widgets, a kid listed twice
/// where it's first; a field that is a widget itself and has kid widgets
/// too holds its own dictionary after them, where PDFium counts none of
/// it.
#[test]
fn a_widget_has_its_place_among_its_fields_widgets() {
    let mut f = Form::new(1);
    let own = f.check_box("own", 0, row(0), false);
    let r = f.radio("r", 0, &[row(1), row(2), row(3)], Some(1));
    let kids = f.widgets_of(r);
    // A kid listed twice is read once, where it's first.
    let mut listed: Vec<Object> = kids.iter().map(|&k| Object::from(k)).collect();
    listed.insert(1, kids[2].into());
    f.dict(r).set("Kids", listed);
    // A field that is also a widget, with kid widgets.
    let (both, both_kids) = f.buttons(
        "both",
        0,
        &[
            crate::forms::fixture::Button::at(4, Some("a"), Some("Off")),
            crate::forms::fixture::Button::at(5, Some("b"), Some("Off")),
        ],
    );
    let rect: Vec<Object> = row(6).iter().map(|&v| Object::from(v)).collect();
    f.dict(both).set("Type", Object::name("Annot"));
    f.dict(both).set("Subtype", Object::name("Widget"));
    f.dict(both).set("Rect", rect);
    let mut annots = f.annots(0);
    annots.push(both);
    f.set_annots(0, &annots);
    let form = read_back(&mut f);
    let index = |id: ObjectId| form.widget_by_object(id).expect("on a page").index;
    assert_eq!(index(own), 0);
    assert_eq!(
        kids.iter().map(|&k| index(k)).collect::<Vec<_>>(),
        [0, 2, 1]
    );
    assert_eq!(index(both_kids[0]), 0);
    assert_eq!(index(both_kids[1]), 1);
    assert_eq!(index(both), 2, "after its kids");
}

/// A state of `/AP /N`: its name, and whether its value is a stream.
type States<'a> = (&'a [u8], bool);

/// A widget's on state: of `/AP /N`'s names whose value is a stream, not
/// empty and not `Off`, the least by its bytes; none where `/N` is one
/// stream, or no such name's value is a stream. `states` keeps every
/// name, in order.
#[test]
fn a_widgets_on_state_is_the_least_name_of_a_stream() {
    let mut f = Form::new(1);
    let ap = |f: &mut Form, names: &[States]| {
        let mut n = crate::pdf::Dictionary::new();
        for &(name, stream) in names {
            let value = match stream {
                true => f.pdf.add_object(Stream::new(dictionary! {}, Vec::new())),
                false => f.pdf.add_object(literal("not a stream")),
            };
            n.set(name, value);
        }
        n
    };
    let cases: [(&[States], Option<&str>); 6] = [
        (
            &[(b"Zeta", true), (b"Alpha", true), (b"Off", true)],
            Some("Alpha"),
        ),
        (&[(b"Off", true), (b"On", false)], None),
        (&[(b"Off", true), (b"", true)], None),
        (&[(b"B", false), (b"C", true), (b"Off", true)], Some("C")),
        (&[(b"Off", true)], None),
        // By its bytes: 0x90 after `Y`.
        (&[(b"\x90x", true), (b"Yes", true)], Some("Yes")),
    ];
    let mut ids = Vec::new();
    for (i, (names, _)) in cases.iter().enumerate() {
        let id = f.check_box(&format!("c{i}"), 0, row(i), false);
        let n = ap(&mut f, names);
        f.dict(id).set("AP", dictionary! { "N" => n });
        ids.push(id);
    }
    let stream = f.check_box("stream", 0, row(7), false);
    let single = f.pdf.add_object(Stream::new(dictionary! {}, Vec::new()));
    f.dict(stream).set("AP", dictionary! { "N" => single });
    let form = read_back(&mut f);
    for (i, (names, on)) in cases.iter().enumerate() {
        let w = form.widget_by_object(ids[i]).unwrap();
        assert_eq!(w.on_state.as_deref(), *on, "case {i}");
        let all: Vec<String> = (names.iter())
            .map(|(n, _)| n.iter().map(|&b| char::from(b)).collect())
            .collect();
        assert_eq!(w.states, all, "case {i}");
    }
    let w = form.widget_by_object(stream).unwrap();
    assert_eq!((w.on_state.as_deref(), w.has_appearance), (None, true));
}

/// A push button's `/MK` and `/H`: its three captions, each a text and
/// followed where it's a reference; `/TP` an integer, a real taken toward
/// 0, one past `i32` its low 32 bits (as the family reads it), and none
/// where it's no number; `/I` an
/// icon only where it's a stream, followed; `/H /P`, `/T` and the string
/// `(P)` pressed, `/I`, `/N`, `/O` and none not. A widget with no `/MK` has
/// none of them.
#[test]
fn a_push_button_s_captions_icon_and_highlight_are_read() {
    let mut f = Form::new(1);
    let rc = f.pdf.add_object(literal("Over"));
    let icon = f
        .pdf
        .add_object(Stream::new(dictionary! {}, b"0 g".to_vec()));
    let tps: [(Object, Option<i32>); 7] = [
        (Object::from(1), Some(1)),
        (Object::from(2.9), Some(2)),
        (Object::from(-0.5), Some(0)),
        (Object::from(1i64 << 40), Some(0)),
        (literal("1"), None),
        (Object::from((1i64 << 32) + 1), Some(1)),
        (Object::from(4_294_967_297.0), Some(1)),
    ];
    let mut ids = Vec::new();
    for (i, (tp, _)) in tps.iter().enumerate() {
        let id = f.push_button(&format!("b{i}"), 0, row(i), "Go");
        let i_entry = match i {
            0 => Object::from(icon),
            _ => literal("x"),
        };
        let mk = dictionary! {
            "CA" => literal("Go"), "RC" => rc, "AC" => crate::pdf::text::string("\u{141}"),
            "TP" => tp.clone(), "I" => i_entry,
        };
        f.dict(id).set("MK", mk);
        ids.push(id);
    }
    let highlights: [(Object, bool); 6] = [
        (Object::name("P"), true),
        (Object::name("T"), true),
        (literal("P"), true),
        (Object::name("I"), false),
        (Object::name("N"), false),
        (Object::name("O"), false),
    ];
    let mut his = Vec::new();
    for (i, (h, _)) in highlights.iter().enumerate() {
        let id = f.push_button(&format!("h{i}"), 0, row(7 + i), "Go");
        f.dict(id).set("H", h.clone());
        his.push(id);
    }
    let bare = f.push_button("bare", 0, row(14), "Go");
    f.dict(bare).remove(b"MK");
    let form = read_back(&mut f);
    for (i, (_, tp)) in tps.iter().enumerate() {
        let w = form.widget_by_object(ids[i]).unwrap();
        assert_eq!(w.mk.caption.as_deref(), Some("Go"), "{i}");
        assert_eq!(w.mk.rollover.as_deref(), Some("Over"), "{i}: followed");
        assert_eq!(w.mk.down.as_deref(), Some("\u{141}"), "{i}");
        assert_eq!(w.mk.text_position, *tp, "{i}");
        assert_eq!(w.mk.icon, i == 0, "{i}");
        assert!(!w.pressed, "{i}: no /H");
    }
    for (i, (_, pressed)) in highlights.iter().enumerate() {
        let w = form.widget_by_object(his[i]).unwrap();
        assert_eq!(w.pressed, *pressed, "{i}");
    }
    let w = form.widget_by_object(bare).unwrap();
    assert_eq!(w.mk, Mk::default());
}

/// A field has `/Opt` where its own or a parent's is an array, an empty
/// one too, and not where it's none or no array; a button field's and a
/// choice field's each keep every item's place, one that is no text (as
/// each reads an item) an empty option, from one array they share.
#[test]
fn a_fields_opt_is_read_for_its_kind() {
    let mut f = Form::new(1);
    let none = f.check_box("none", 0, row(0), false);
    let empty = f.check_box("empty", 0, row(1), false);
    f.dict(empty).set("Opt", Vec::<Object>::new());
    let name = f.check_box("name", 0, row(2), false);
    f.dict(name).set("Opt", Object::name("x"));
    let kid = f.check_box("kid", 0, row(3), false);
    let parent = f.group("parent", &[kid]);
    f.dict(parent).set("Opt", vec![literal("p")]);
    let stream_text = (f.pdf).add_object(Stream::new(Dictionary::new(), b"s".to_vec()));
    let items = vec![
        literal("x"),
        5.into(),
        Object::from(vec![literal("y"), literal("Y")]),
        Object::from(vec![literal("short")]),
        literal("z"),
        // An array of three, and a stream's text: a choice's options, no
        // export values of a button's, as the family reads an option.
        Object::from(vec![literal("t"), literal("T"), literal("more")]),
        Object::from(stream_text),
        Object::from(vec![Object::from(stream_text), literal("S")]),
        literal("end"),
    ];
    let shared = f.pdf.add_object(Object::from(items));
    let button = f.check_box("button", 0, row(4), false);
    f.dict(button).set("Opt", shared);
    let choice = f.choice("choice", 0, row(5), &[], true);
    f.dict(choice).set("Opt", shared);
    let direct = f.check_box("direct", 0, row(6), false);
    f.dict(direct)
        .set("Opt", vec![literal("a"), Object::null(), literal("b")]);
    let form = read_back(&mut f);
    let read = |id: ObjectId| {
        let field = form.field_by_object(id).unwrap();
        let exports: Vec<&str> = field.options.iter().map(|o| &*o.export).collect();
        (field.has_opt, exports.join("|"))
    };
    assert_eq!(read(none), (false, String::new()));
    assert_eq!(read(empty), (true, String::new()));
    assert_eq!(read(name), (false, String::new()));
    assert_eq!(read(kid), (true, "p".into()));
    assert_eq!(read(button), (true, "x||y||z||||end".into()));
    assert_eq!(read(choice), (true, "x||y||z|t|s|s|end".into()));
    assert_eq!(read(direct), (true, "a||b".into()));
    let shown: Vec<&str> = (form.field_by_object(choice).unwrap().options.iter())
        .map(|o| &*o.shown)
        .collect();
    assert_eq!(shown, ["x", "", "Y", "", "z", "T", "s", "S", "end"]);
}

/// A list box whose `/Opt` has an item that is no text before its others,
/// `[1 (a) (b)]`, reads three options, the first of empty text, so its
/// `/TI 1` names the first option it shows (`a`) as the family and PDFium
/// take it, and the index a save writes into `/I` is the file's.
#[test]
fn a_choice_s_opt_item_that_is_no_text_keeps_its_place() {
    let mut f = Form::new(1);
    let list = f.choice("list", 0, row(0), &[], false);
    f.dict(list)
        .set("Opt", vec![1.into(), literal("a"), literal("b")]);
    f.dict(list).set("TI", 1);
    let form = read_back(&mut f);
    let field = form.field_by_object(list).unwrap();
    let options: Vec<(&str, &str)> = (field.options.iter())
        .map(|o| (&*o.export, &*o.shown))
        .collect();
    assert_eq!(options, [("", ""), ("a", "a"), ("b", "b")]);
    assert_eq!(field.top, 1);
    assert_eq!(&*field.options[field.top as usize].export, "a");
}

/// What damage reads as: no fields, and a reason.
fn assert_damaged(f: &mut Form, why: &str) {
    assert_damaged_within(f, Limits::DEFAULT.forms, why);
}

/// [`assert_damaged`] within `limits`.
fn assert_damaged_within(f: &mut Form, limits: FormLimits, why: &str) {
    let form = read_within(f, limits);
    let facts = &form.facts;
    let reason = facts
        .damaged
        .as_deref()
        .unwrap_or_else(|| panic!("not damaged: {why}"));
    assert!(reason.contains(why), "{reason:?}, not {why:?}");
    assert!(form.fields.is_empty() && form.widgets.is_empty());
    assert!(!facts.is_fillable());
    assert!(facts.to_string().ends_with("so not fillable"), "{facts}");
}

#[test]
fn damaged_trees_are_not_fillable() {
    // A cycle through /Kids.
    let mut f = Form::new(1);
    let a = f.pdf.add_object(dictionary! { "T" => literal("a") });
    let b = f
        .pdf
        .add_object(dictionary! { "T" => literal("b"), "Kids" => vec![Object::from(a)] });
    f.dict(a).set("Kids", vec![Object::from(b)]);
    f.dict(a).set("Parent", b);
    f.dict(b).set("Parent", a);
    f.set_fields(&[a]);
    assert_damaged(&mut f, "its own ancestor");

    // A field that's its own kid and parent; and a widget that's its own
    // kid.
    let mut f = Form::new(1);
    let ok = f.text("ok", 0, row(0));
    let a = f.pdf.add_object(dictionary! { "T" => literal("a") });
    f.dict(a).set("Kids", vec![Object::from(a)]);
    f.dict(a).set("Parent", a);
    f.set_fields(&[ok, a]);
    assert_damaged(&mut f, "its own ancestor");
    let mut f = Form::new(1);
    let w = f.text("w", 0, row(0));
    f.dict(w).set("Kids", vec![Object::from(w)]);
    assert_damaged(&mut f, "its own ancestor");

    // Smaller caps, for trees past them that would take long to build.
    let small = FormLimits {
        nodes: 1000,
        entries: 2000,
        widgets: 1000,
        texts: 4 << 20,
        ..Limits::DEFAULT.forms
    };

    // A hundred fields that share one /Kids array of a hundred: each kid
    // is read once, but the lists come to ten thousand entries.
    let mut f = Form::new(1);
    let kids: Vec<Object> = (0..100)
        .map(|i| {
            let t = literal(format!("k{i}"));
            Object::from(
                f.pdf
                    .add_object(dictionary! { "FT" => Object::name("Tx"), "T" => t }),
            )
        })
        .collect();
    let kids = f.pdf.add_object(kids);
    let parents: Vec<ObjectId> = (0..100)
        .map(|i| {
            let t = literal(format!("p{i}"));
            f.pdf.add_object(dictionary! { "T" => t, "Kids" => kids })
        })
        .collect();
    f.set_fields(&parents);
    assert_damaged_within(&mut f, small, "more than 2000 entries");

    // A chain 10,000 deep.
    let mut f = Form::new(1);
    let mut below = f.text("leaf", 0, row(0));
    for i in 0..10_000 {
        let up = f.pdf.add_object(dictionary! {
            "T" => literal(format!("n{i}")),
            "Kids" => vec![Object::from(below)],
        });
        f.dict(below).set("Parent", up);
        below = up;
    }
    f.set_fields(&[below]);
    assert_damaged(&mut f, "more than 32 deep");

    // More fields than the cap.
    let mut f = Form::new(1);
    let fields: Vec<ObjectId> = (0..=small.nodes)
        .map(|i| {
            let t = literal(format!("f{i}"));
            f.pdf.add_object(dictionary! { "T" => t })
        })
        .collect();
    f.set_fields(&fields);
    assert_damaged_within(&mut f, small, "more than 1000 nodes");

    // More widgets on pages than the cap, under one field.
    let mut f = Form::new(1);
    let widgets: Vec<ObjectId> = (0..=small.widgets)
        .map(|_| {
            f.pdf
                .add_object(dictionary! { "Subtype" => Object::name("Widget") })
        })
        .collect();
    let kids: Vec<Object> = widgets.iter().map(|&w| Object::from(w)).collect();
    let many = f.pdf.add_object(dictionary! {
        "FT" => Object::name("Btn"), "T" => literal("many"), "Kids" => kids,
    });
    f.set_fields(&[many]);
    f.set_annots(0, &widgets);
    assert_damaged_within(&mut f, small, "more than 1000 widgets");

    // A full name over 64 KiB.
    let mut f = Form::new(1);
    let kid = f.text("k", 0, row(0));
    f.group(&"n".repeat(70_000), &[kid]);
    assert_damaged(&mut f, "longer than 64 KiB");

    // Full names over 64 MiB in all: a 65,000-byte name in each of 1,100
    // kids' names.
    let mut f = Form::new(1);
    let kids: Vec<ObjectId> = (0..1100)
        .map(|i| f.text(&format!("k{i}"), 0, row(0)))
        .collect();
    f.group(&"n".repeat(65_000), &kids);
    assert_damaged(&mut f, "full names come to more than 64 MiB");

    let caps = Limits::DEFAULT.forms;
    // An /Opt with more than 10,000 items.
    let mut f = Form::new(1);
    let items: Vec<String> = (0..=caps.items).map(|i| i.to_string()).collect();
    let items: Vec<&str> = items.iter().map(String::as_str).collect();
    f.choice("big", 0, row(0), &items, true);
    assert_damaged(&mut f, "an /Opt has more than 10000 items");
    // And a list box's value.
    let mut f = Form::new(1);
    let list = f.choice("list", 0, row(0), &["a"], false);
    let items = vec![literal("a"); caps.items + 1];
    f.dict(list).set("V", items);
    assert_damaged(&mut f, "a value has more than 10000 items");

    // A value over 1 MiB, as a string.
    let mut f = Form::new(1);
    let t = f.text("t", 0, row(0));
    f.dict(t).set("V", literal(vec![b'x'; caps.text + 1]));
    assert_damaged(&mut f, "longer than 1 MiB");

    // More text in all than the cap: 5 values of 1 MiB.
    let mut f = Form::new(1);
    for i in 0..5 {
        let t = f.text(&format!("t{i}"), 0, row(0));
        f.dict(t).set("V", literal(vec![b'x'; caps.text]));
    }
    assert_damaged_within(&mut f, small, "its text comes to more than 4 MiB");

    // A value that inflates past 1 MiB.
    let mut f = Form::new(1);
    let bomb = f.pdf.add_object(Stream::new(
        dictionary! { "Filter" => Object::name("FlateDecode") },
        deflate_best(&vec![b'0'; caps.text + 1]),
    ));
    let t = f.text("t", 0, row(0));
    f.dict(t).set("V", bomb);
    assert_damaged(&mut f, "longer than 1 MiB");
}

#[test]
fn fields_listed_twice_are_read_once() {
    // A field in /Fields twice; a kid in /Fields before its parent; a
    // widget two fields list; and one listed twice by its field.
    let mut f = Form::new(1);
    let twice = f.text("twice", 0, row(0));
    let kid = f.text("kid", 0, row(1));
    let parent = f.group("parent", &[kid]);
    let x = f.radio("x", 0, &[row(2)], Some(0));
    let y = f.radio("y", 0, &[row(3)], Some(0));
    let shared = f.widgets_of(x)[0];
    f.dict(y)
        .set("Kids", vec![Object::from(shared), Object::from(shared)]);
    f.set_fields(&[twice, kid, parent, x, y, twice]);
    let form = read_back(&mut f);
    assert_eq!(form.facts.damaged, None);
    let names: Vec<&str> = form.fields.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, ["twice", "parent.kid", "x", "y"]);
    // Nodes: twice, parent, kid, x and y; kid read once, under parent.
    let nodes: Vec<(&str, Option<u64>)> = (form.nodes.iter())
        .map(|n| (&*n.partial, n.parent.map(|p| p.0)))
        .collect();
    assert_eq!(
        nodes,
        [
            ("twice", None),
            ("parent", None),
            ("kid", Some(1)),
            ("x", None),
            ("y", None)
        ]
    );
    let owners: Vec<(u64, usize)> = form.widgets[&0]
        .iter()
        .map(|w| (w.field.0, w.slot))
        .collect();
    // y's own widget isn't its kid any more: no field has it.
    assert_eq!(owners, [(0, 0), (2, 1), (3, 2)]);
}

#[test]
fn a_page_listed_twice_has_its_widgets_at_both_indexes() {
    // Widgets are found through the page table, which numbers pages as
    // PDFium does: the page object twice is two pages.
    let mut f = Form::new(1);
    let a = f.text("a", 0, row(0));
    let page = Object::from(f.pages[0]);
    let root = f
        .dict(f.catalog)
        .get(b"Pages")
        .unwrap()
        .as_reference()
        .unwrap();
    f.dict(root).set("Kids", vec![page.clone(), page]);
    f.dict(root).set("Count", 2);
    let form = read_back(&mut f);
    let on: Vec<(u32, ObjectId)> = form
        .widgets
        .iter()
        .flat_map(|(&i, ws)| ws.iter().map(move |w| (i, w.object)))
        .collect();
    assert_eq!(on, [(0, a), (1, a)]);
    assert_eq!(form.facts.widgets, 2);
}

/// A widget no page of the page tree lists, as a page template's, counts
/// as its field's outside; a widget on a page doesn't.
#[test]
fn a_template_s_widgets_are_outside() {
    let mut f = every_kind();
    let spawned = f.text("spawned", 0, row(6));
    let annots: Vec<_> = f.annots(0).into_iter().filter(|&a| a != spawned).collect();
    f.set_annots(0, &annots);
    let template = f.pdf.add_object(dictionary! {
        "Type" => Object::name("Page"),
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        "Annots" => vec![Object::from(spawned)],
    });
    let templates = dictionary! { "Names" => vec![literal("extra"), template.into()] };
    f.dict(f.catalog)
        .set("Names", dictionary! { "Templates" => templates });
    let form = read_back(&mut f);
    let outside: Vec<(&str, u16)> = (form.fields.iter())
        .filter(|f| f.outside > 0)
        .map(|f| (f.name.as_str(), f.outside))
        .collect();
    assert_eq!(outside, [("spawned", 1)]);
    assert_eq!(form.facts.widgets, 10);
}

/// A full name passes over a node with no `/T` or an empty one, as
/// PDFium's does.
#[test]
fn an_empty_partial_name_is_passed_over() {
    let mut f = Form::new(1);
    let a = f.text("a", 0, row(0));
    let b = f.text("b", 0, row(1));
    let unnamed = f.group("", &[a]);
    f.dict(unnamed).remove(b"T");
    let empty = f.group("", &[b]);
    f.group("top", &[unnamed, empty]);
    let form = read_back(&mut f);
    let names: Vec<&str> = form.fields.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, ["top.a", "top.b"]);
    let partials: Vec<&str> = form.nodes.iter().map(|n| &*n.partial).collect();
    assert_eq!(partials, ["top", "", "a", "", "b"]);
}

/// A partial name is a text string, read as PDFium reads one: 0x90 and
/// 0x85 are PDFDocEncoding's ’ and –, not Latin-1's control characters,
/// and UTF-16LE after its mark (PDFium's, which ISO's table hasn't) is
/// read too, a parent's as a field's own.
#[test]
fn a_partial_name_reads_as_pdfium_reads_it() {
    let utf16le = |text: &str| {
        let units = text.encode_utf16().flat_map(u16::to_le_bytes);
        Object::hex([0xFF, 0xFE].into_iter().chain(units).collect::<Vec<u8>>())
    };
    let mut f = Form::new(1);
    let quoted = f.text("quoted", 0, row(0));
    f.dict(quoted).set("T", literal(b"Ann\x90s \x85 name"));
    let le = f.text("le", 0, row(1));
    f.dict(le).set("T", utf16le("Łódź"));
    let kid = f.text("kid", 0, row(2));
    let parent = f.group("parent", &[kid]);
    f.dict(parent).set("T", utf16le("Straße"));
    let form = read_back(&mut f);
    let names: Vec<&str> = form.fields.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, ["Ann’s – name", "Łódź", "Straße.kid"]);
    let partials: Vec<&str> = form.nodes.iter().map(|n| &*n.partial).collect();
    assert_eq!(partials, ["Ann’s – name", "Łódź", "Straße", "kid"]);
}

#[test]
fn a_parent_loop_alone_isnt_damage() {
    // /Parent isn't followed, so a field that's its own parent, but not its
    // own kid, reads like any other.
    let mut f = Form::new(1);
    let a = f.text("a", 0, row(0));
    f.dict(a).set("Parent", a);
    let form = read_back(&mut f);
    assert_eq!(form.facts.damaged, None);
    let names: Vec<&str> = form.fields.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, ["a"]);
    assert_eq!(form.widgets[&0].len(), 1);
}

#[test]
fn layout_uses_at_most_a_thousand_of_max_len() {
    // Kept as the file has it; capped where it's used.
    let mut f = Form::new(1);
    let t = f.text("t", 0, row(0));
    f.dict(t).set("MaxLen", 1_000_000);
    let u = f.text("u", 0, row(1));
    f.dict(u).set("MaxLen", -3);
    let form = read_back(&mut f);
    assert_eq!(form.fields[0].max_len, Some(1_000_000));
    assert_eq!(form.fields[0].layout_len(), Some(MAX_LAYOUT_LEN));
    assert_eq!(form.fields[1].max_len, Some(0));
}

#[test]
fn hybrid_reference_files_read_their_newest_revision() {
    let fields = |form: &FileForm| -> Vec<(String, FieldKind, Option<Value>)> {
        form.fields
            .iter()
            .map(|f| (f.name.clone(), f.kind, f.value.clone()))
            .collect()
    };
    // The form is in an object stream only /XRefStm lists, and the
    // trailer has no /Prev.
    let first = hybrid_reference();
    let src = source(first.clone());
    let form = read(&src);
    assert_eq!(
        fields(&form),
        [
            ("name".into(), FieldKind::Text, text("Ann Lee")),
            ("city".into(), FieldKind::Text, text("Hull")),
        ]
    );
    assert!(src.xref().hybrid && src.xref().sections == 1);

    // The update's catalog is only in its /XRefStm, and the first table
    // lists the old one. The field it changes is in both sections' object
    // streams, and `city` only in the first section's.
    let src = source(hybrid_update(&first));
    let acroform = src.pdf().catalog().unwrap().get(b"AcroForm").unwrap();
    assert_eq!(acroform.as_reference().unwrap(), ObjectId::new(11, 0));
    let form = read(&src);
    assert_eq!(
        fields(&form),
        [
            ("name".into(), FieldKind::Text, text("Bo Ray")),
            ("city".into(), FieldKind::Text, text("Hull")),
            ("sig".into(), FieldKind::Signature, None),
        ]
    );
    assert_eq!(form.widgets[&0].len(), 3);
    assert_eq!(form.facts.signature_fields, 1);
}

/// A crafted file can point thousands of sections' /XRefStm at one large
/// cross-reference stream, by offsets that differ only in the white-space,
/// comments and zeros before its header. The reader reads it once: read
/// for each section, this file takes seconds to load, not milliseconds.
/// Its 1,003 sections are within the lazy reader's cap, and its source
/// opens and its form reads as quickly.
#[test]
fn a_cross_reference_stream_many_sections_share_is_read_once() {
    let mut bytes = hybrid_reference();
    let mut prev = source(bytes.clone()).xref().start;
    // A million free entries, 7 MB unpacked.
    let entries = deflate_best(&vec![0; 7_000_000]);
    let padding = [" ".repeat(300), "%\n".repeat(300), "0".repeat(100)].concat();
    let at = bytes.len();
    bytes.extend_from_slice(padding.as_bytes());
    let header = bytes.len();
    let dict = "/Type /XRef /Size 1000100 /Index [100 1000000] /W [1 4 2] /Filter /FlateDecode";
    let head = format!("99 0 obj\n<< {dict} /Length {} >>\nstream\n", entries.len());
    bytes.extend_from_slice(head.as_bytes());
    bytes.extend_from_slice(&entries);
    bytes.extend_from_slice(b"\nendstream\nendobj\n");
    // Every offset in the padding reaches the stream, and so do both
    // digits of its number.
    for stream in at..header + 2 {
        let section = bytes.len();
        let trailer = format!("<< /Size 10 /Root 1 0 R /XRefStm {stream} /Prev {prev} >>");
        bytes.extend_from_slice(format!("xref\n0 0\ntrailer\n{trailer}\n").as_bytes());
        prev = section;
    }
    bytes.extend_from_slice(format!("startxref\n{prev}\n%%EOF\n").as_bytes());

    // Eagerly, as a Rewrite loads its primary (`materialize::graph`).
    let start = Instant::now();
    let pdf = crate::file::load_pdf_mem(&bytes, None).unwrap();
    let took = start.elapsed();
    let acroform = pdf.catalog().unwrap().get(b"AcroForm").unwrap();
    assert!(resolve(&pdf, acroform).unwrap().as_dict().is_some());
    assert!(took.as_secs_f32() < 1.0, "{took:?}");

    let start = Instant::now();
    let src = source(bytes);
    let form = read(&src);
    let took = start.elapsed();
    assert_eq!(form.fields.len(), 2);
    assert_eq!(src.xref().sections, 1003);
    assert!(src.xref().sections <= src.limits().reader.max_sections);
    assert!(took.as_secs_f32() < 1.0, "{took:?}");
}

/// Every corpus file with a hybrid-reference section (a trailer with
/// `/XRefStm`) has the manifest's fields by type, as pypdf read them.
/// lopdf 0.45 read an earlier revision of 9 SafeDocs files, without the
/// signature field their update added. As in `forms_corpus.rs`, since
/// pypdf counts a field listed twice twice, every curated form must match
/// and 99% of the SafeDocs sample; damaged trees, which sheaf doesn't
/// read, are left out. Skips without the corpus.
#[test]
fn corpus_hybrid_reference_files_read_their_newest_revision() {
    let Some(corpus) = Corpus::or_skip() else {
        return;
    };
    let sample = |path: &str| path.starts_with("forms/safedocs/");
    let (mut files, mut sampled) = (0, 0);
    let mut differ = Vec::new();
    // pypdf couldn't read the `unparsed` ones: their rows say nothing.
    for entry in corpus.forms().unwrap() {
        if entry.kind == "unparsed" {
            continue;
        }
        let bytes = std::fs::read(corpus.path(&entry)).unwrap();
        if !bytes.windows(8).any(|w| w == b"/XRefStm") {
            continue;
        }
        // Some damaged files don't open at all (slice 1's note).
        let Ok((src, _)) = Store::new().read_file(&corpus.path(&entry), None) else {
            continue;
        };
        let facts = read(&src).facts;
        if facts.damaged.is_some() {
            continue;
        }
        files += 1;
        if sample(&entry.path) {
            sampled += 1;
        }
        let kinds: BTreeMap<String, u32> = facts
            .kinds
            .iter()
            .filter(|(k, _)| **k != FieldKind::Unknown)
            .map(|(k, n)| (k.short().into(), *n as u32))
            .collect();
        if (facts.fields as u32, &kinds) != (entry.fields, &entry.field_types) {
            differ.push((
                entry.path.clone(),
                format!("{kinds:?}, not {:?}", entry.field_types),
            ));
        }
    }
    eprintln!("{files} files with /XRefStm read, {} differ", differ.len());
    let curated: Vec<_> = differ.iter().filter(|(p, _)| !sample(p)).collect();
    assert!(curated.is_empty(), "{curated:#?}");
    assert!(differ.len() * 100 <= sampled, "{differ:#?}");
}

/// A node of a random field tree: its keys, with references by index
/// into the tree (some of them to objects that aren't there).
#[derive(Debug, Clone)]
struct Node {
    name: Option<String>,
    ft: Option<&'static str>,
    ff: Option<i64>,
    value: Option<Val>,
    kids: Option<Vec<usize>>,
    parent: Option<usize>,
    widget: bool,
    rect: Option<Vec<Num>>,
    mk: Option<Vec<Num>>,
    opt: Option<usize>,
    on_page: bool,
}

/// A number in a rect or a color: one lopdf writes as it is, or one past
/// f64's range, which reads as infinity (see [`huge`]).
#[derive(Debug, Clone, Copy)]
enum Num {
    Real(f32),
    Huge,
}

impl From<Num> for Object {
    fn from(n: Num) -> Object {
        match n {
            Num::Real(v) => v.into(),
            Num::Huge => huge(),
        }
    }
}

#[derive(Debug, Clone)]
enum Val {
    Text(String),
    Name(String),
    Items(Vec<String>),
    Number(i64),
    Ref(usize),
}

fn node() -> impl Strategy<Value = Node> {
    let value = prop_oneof![
        "[a-z]{0,8}".prop_map(Val::Text),
        "[A-Za-z]{1,4}".prop_map(Val::Name),
        prop::collection::vec("[a-z]{0,3}", 0..4).prop_map(Val::Items),
        any::<i64>().prop_map(Val::Number),
        (0usize..24).prop_map(Val::Ref),
    ];
    // lopdf writes a real past i64's range as digits it can't read back.
    let float = prop_oneof![
        (-1e18f32..1e18).prop_map(Num::Real),
        Just(Num::Huge),
        (-1e7f32..1e7).prop_map(Num::Real),
    ];
    (
        (
            prop::option::of("[a-c]{1,3}"),
            prop::option::of(prop::sample::select(vec!["Tx", "Btn", "Ch", "Sig", "Zz"])),
            prop::option::of(any::<i64>()),
            prop::option::of(value),
            prop::option::of(prop::collection::vec(0usize..24, 0..5)),
            prop::option::of(0usize..24),
        ),
        (
            any::<bool>(),
            prop::option::of(prop::collection::vec(float.clone(), 0..6)),
            prop::option::of(prop::collection::vec(float, 0..6)),
            prop::option::of(0usize..5),
            any::<bool>(),
        ),
    )
        .prop_map(
            |((name, ft, ff, value, kids, parent), (widget, rect, mk, opt, on_page))| Node {
                name,
                ft,
                ff,
                value,
                kids,
                parent,
                widget,
                rect,
                mk,
                opt,
                on_page,
            },
        )
}

/// The tree as a file: node `i` is object `i + 1`; index 20 and up point
/// at objects that aren't there.
fn tree_file(nodes: &[Node], roots: &[usize]) -> Vec<u8> {
    let mut f = Form::new(1);
    let base = f.pdf.max_id + 1;
    let id = |i: usize| -> ObjectId { ObjectId::new(base + i as u32, 0) };
    let mut on_page = Vec::new();
    for (i, n) in nodes.iter().enumerate() {
        let mut d = crate::pdf::Dictionary::new();
        if let Some(name) = &n.name {
            d.set("T", literal(name.as_str()));
        }
        if let Some(ft) = n.ft {
            d.set("FT", Object::name(ft));
        }
        if let Some(ff) = n.ff {
            d.set("Ff", ff);
        }
        let v = match &n.value {
            Some(Val::Text(t)) => Some(literal(t.as_str())),
            Some(Val::Name(t)) => Some(Object::name(t.as_bytes())),
            Some(Val::Items(items)) => Some(
                items
                    .iter()
                    .map(|t| literal(t.as_str()))
                    .collect::<Vec<_>>()
                    .into(),
            ),
            Some(Val::Number(v)) => Some((*v).into()),
            Some(Val::Ref(r)) => Some(Object::from(id(*r))),
            None => None,
        };
        if let Some(v) = v {
            d.set("V", v);
        }
        if let Some(kids) = &n.kids {
            let kids: Vec<Object> = kids.iter().map(|&k| Object::from(id(k))).collect();
            d.set("Kids", kids);
        }
        if let Some(p) = n.parent {
            d.set("Parent", id(p));
        }
        if n.widget {
            d.set("Subtype", Object::name("Widget"));
        }
        if let Some(rect) = &n.rect {
            d.set(
                "Rect",
                rect.iter().map(|&v| Object::from(v)).collect::<Vec<_>>(),
            );
        }
        if let Some(mk) = &n.mk {
            let c: Vec<Object> = mk.iter().map(|&v| Object::from(v)).collect();
            d.set("MK", dictionary! { "BG" => c.clone(), "BC" => c });
        }
        if let Some(n) = n.opt {
            let opt: Vec<Object> = (0..n).map(|i| Object::from(i as i64)).collect();
            d.set("Opt", opt);
        }
        f.pdf.objects.insert(id(i), Object::from(d));
        if n.on_page {
            on_page.push(id(i));
        }
    }
    f.pdf.max_id = base + 24;
    f.set_annots(0, &on_page);
    f.set_fields(&roots.iter().map(|&r| id(r)).collect::<Vec<_>>());
    with_huge(f.bytes())
}

proptest! {
    /// Any tree reads without a panic, and what's read hangs together:
    /// fields are distinct objects, each its node, nodes come after their
    /// parents and name them as their kids, each widget belongs to one of
    /// the fields, and every number is finite.
    #[test]
    fn any_field_tree_reads(
        nodes in prop::collection::vec(node(), 1..20),
        roots in prop::collection::vec(0usize..24, 0..6),
    ) {
        let form = read(&source(tree_file(&nodes, &roots)));
        let facts = &form.facts;
        if facts.damaged.is_some() {
            prop_assert!(form.fields.is_empty() && form.widgets.is_empty());
        } else {
            prop_assert_eq!(facts.fields, form.fields.len());
        }
        let objects: HashSet<ObjectId> = form.fields.iter().map(|f| f.object).collect();
        prop_assert_eq!(objects.len(), form.fields.len());
        for f in &form.fields {
            prop_assert_eq!(form.nodes[f.id.0 as usize].object, f.object);
        }
        for (i, n) in (0..).zip(&form.nodes) {
            prop_assert!(n.parent.is_none_or(|p| p.0 < i));
            for kid in &n.kids {
                prop_assert_eq!(form.nodes[kid.0 as usize].parent, Some(FieldId(i)));
            }
        }
        let ids: HashSet<FieldId> = form.fields.iter().map(|f| f.id).collect();
        let mut owner = HashMap::new();
        for w in form.widgets.values().flatten() {
            prop_assert!(ids.contains(&w.field));
            prop_assert_eq!(*owner.entry(w.object).or_insert(w.field), w.field);
            prop_assert!(w.rect.iter().all(|v| v.is_finite() && v.abs() <= MAX_COORD));
            prop_assert!(w.rect[0] <= w.rect[2] && w.rect[1] <= w.rect[3]);
            for c in [w.mk.background, w.mk.border_color].into_iter().flatten() {
                let parts = match c {
                    Color::Gray(g) => vec![g],
                    Color::Rgb(p) => p.to_vec(),
                    Color::Cmyk(p) => p.to_vec(),
                };
                prop_assert!(parts.iter().all(|v| (0.0..=1.0).contains(v)));
            }
        }
    }
}

/// Every curated hybrid form's fields find their XFA fields by name, most
/// have its tooltips, and a blank form's XFA copy holds nothing the
/// AcroForm lacks (its check boxes' `0`s aren't values).
#[test]
fn corpus_hybrid_forms_bind_their_xfa() {
    let Some(corpus) = Corpus::or_skip() else {
        return;
    };
    let mut forms = 0;
    for entry in corpus.forms().unwrap() {
        if entry.kind != "hybrid" || entry.path.starts_with("forms/safedocs/") {
            continue;
        }
        let (src, _) = Store::new().read_file(&corpus.path(&entry), None).unwrap();
        let form = read(&src);
        let pdf = src.pdf();
        let acroform = pdf
            .catalog()
            .unwrap()
            .get(b"AcroForm")
            .and_then(|f| resolve(pdf, f))
            .and_then(Object::as_dict)
            .unwrap();
        let xfa: HashSet<String> = crate::forms::xfa::read(pdf, acroform, &src.limits().forms)
            .into_iter()
            .map(|f| f.name)
            .collect();
        let unbound: Vec<&str> = form
            .fields
            .iter()
            .map(|f| f.name.as_str())
            .filter(|n| !xfa.contains(*n))
            .collect();
        assert!(unbound.is_empty(), "{}: {unbound:?}", entry.path);
        assert_eq!(form.facts.xfa_values, 0, "{}", entry.path);
        let tooltips = form.fields.iter().filter(|f| f.tooltip.is_some()).count();
        assert!(
            tooltips * 2 > form.fields.len(),
            "{}: {tooltips} tooltips",
            entry.path
        );
        forms += 1;
    }
    println!("{forms} hybrid forms");
}
