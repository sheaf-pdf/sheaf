//! A combo box's or list box's appearance made anew where the model holds
//! its value changed (M10 slice 8), through the emitter: a Rewrite and an
//! Append each write it where a text field's is, one form under `Part::Ap`,
//! naming the `/DR` font's own object, and nothing else anew; a combo box
//! its option's label with its drop button, an editable one's value as it
//! is, none with no value; a list box's highlight over one and several
//! selected, from its `/TI`; a value in the field's font alone, and one
//! in two fonts with its subset, which holds no glyph of a make that was
//! refused; what neither font draws, and more options than sheaf reads,
//! unmade with no `/AP`; a choice of two widgets; and an overlay and a
//! save holding the same form.

use std::collections::BTreeSet;
use std::sync::Arc;

use crate::pdf::{
    Dictionary, LazyPdf, Object, ObjectId, Stream, StreamExt, XrefKind, dictionary, literal, text,
};

use super::append_tests::{appended_in_place, written};
use super::field_ap_tests::{Theirs, read_back, saved_each_way, text_read};
use super::key::{ObjKey, Part};
use super::removal_tests::{opened, scratch, warned};
use super::tests::{lazy, open, rewrite, span};
use super::*;
use crate::annot::AnnotId;
use crate::content::fixture::{fallback_subsets, subsets_hold};
use crate::content::{Fallback, Objects, Unkept};
use crate::forms::appearance::tests::shows;
use crate::forms::field::{Choice, Value, flags};
use crate::forms::fixture::{COMBO, Form, Rect, row};
use crate::model::{Document, FieldNode, FieldProps, Terminal};
use crate::pdf::forms as family;
use crate::prim::{Key, Val};
use crate::realize::{ViewInputs, overlay_appearances, untouched};
use crate::tx::fixture as fx;

/// `f`'s file, its cross-reference a table, so a Rewrite writes each object
/// on its own and an object nothing changed is its bytes as they were.
fn table(f: &mut Form) -> Vec<u8> {
    f.pdf.xref_kind = XrefKind::Table;
    f.bytes()
}

/// `options` as an `/Opt`: an export value and its label each, a string
/// where the two are one.
fn opt(options: &[(&str, &str)]) -> Vec<Object> {
    (options.iter())
        .map(|&(export, label)| match export == label {
            true => text::string(export),
            false => Object::from(vec![text::string(export), text::string(label)]),
        })
        .collect()
}

/// A combo box (or, `combo` false, a list box) `name`, its own widget at
/// `rect` on page 0, of `options` ([`opt`]), with no value.
fn choice(f: &mut Form, name: &str, rect: Rect, options: &[(&str, &str)], combo: bool) -> ObjectId {
    let id = f.choice(name, 0, rect, &[], combo);
    f.dict(id).set("Opt", opt(options));
    id
}

/// A list box's rect at row `i`, 60 points high: five lines of 10 points.
fn tall(i: usize) -> Rect {
    let [x0, _, x1, y1] = row(i);
    [x0, y1 - 60.0, x1, y1]
}

/// Four options, two with labels of their own.
const ABCD: &[(&str, &str)] = &[("a", "Alpha"), ("b", "Bravo"), ("c", "c"), ("d", "Delta")];

/// The widgets of the field named `name` in `d`.
fn widgets_of(d: &Document, name: &str) -> Vec<AnnotId> {
    let st = d.state();
    let id = (st.field_named(name)).unwrap_or_else(|| panic!("no field {name}"));
    let term = st.field(id).and_then(|f| f.term.clone());
    term.expect("a terminal").widgets.iter().copied().collect()
}

/// The one widget of the field named `name` in `d`.
fn widget_of(d: &Document, name: &str) -> AnnotId {
    let [aid] = widgets_of(d, name)[..] else {
        panic!("{name}: one widget")
    };
    aid
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

/// The content of the form a save holds for `widget`, as text.
fn saved_text(pdf: &LazyPdf, widget: ObjectId) -> String {
    String::from_utf8(saved_form(pdf, widget).0.content).expect("content that's text")
}

/// A string `/V` in `pdf`'s object `field`, its bytes.
fn value_of(pdf: &LazyPdf, field: ObjectId) -> Option<Vec<u8>> {
    let v = pdf.get_dictionary(field).ok()?.get(b"V")?;
    v.as_bytes().map(<[u8]>::to_vec)
}

/// Every object `pdf` holds.
fn ids(pdf: &LazyPdf) -> BTreeSet<ObjectId> {
    pdf.ids().collect()
}

/// `texts`, each as the bytes a `Tj` shows.
fn bytes_of(texts: &[&str]) -> Vec<Vec<u8>> {
    texts.iter().map(|t| t.as_bytes().to_vec()).collect()
}

/// The numbers in the fallback font's whole program of the glyphs that
/// draw `chars`, a character each.
fn glyphs_of(chars: &str) -> BTreeSet<u16> {
    let program = Fallback::SANS.program();
    (chars.chars())
        .map(|c| program.glyph(c).expect("a character the fallback draws"))
        .collect()
}

/// The light gray a combo box's drop button is filled in.
const BUTTON: &str = "0.8627 0.8627 0.8627 rg\n";
/// The blue a list box's selected line is drawn over.
const HIGHLIGHT: &str = "0 0.2 0.4431 rg\n";

/// A Rewrite of a combo box whose value moved to an option's export value
/// writes its appearance where a text field's is: under `AnnotPart(aid,
/// Ap)`, the widget's `/AP` naming it alone, its `/Resources` naming
/// `/DR`'s Helvetica, the font's own object; its content `/Tx BMC`, the
/// option's label read back, and the drop button after it. The field's `/V`
/// is the value now (M10 slice 9), and it has no `/I`. Nothing else is
/// written anew: the objects are the unchanged Rewrite's less the old
/// appearance's stream and with the made form, each other one its bytes as
/// they were; the unchanged combo box is the file's object. An Append writes
/// the widget and its form alone, and another after it neither.
#[test]
fn a_combo_box_shows_its_value_s_label_with_its_drop_button() {
    let mut f = Form::new(1);
    let helv = f.helv;
    let c = choice(&mut f, "c", row(0), ABCD, true);
    f.fill(c, "a");
    let u = choice(&mut f, "u", row(1), ABCD, true);
    f.fill(u, "d");
    let bytes = table(&mut f);
    let was = lazy(&bytes, None);
    let old = ap_n(&was, c).expect("the file's appearance");
    let plain = rewrite(&open(&bytes, None), Profile::default());
    let plain_pdf = lazy(&plain.bytes, None);
    assert_eq!(ap_n(&plain_pdf, c), Some(old));

    let mut d = open(&bytes, None);
    let aid = widget_of(&d, "c");
    fx::choose(&mut d, "c", &["b"]);
    let out = rewrite(&d, Profile::default());
    let pdf = lazy(&out.bytes, None);
    let ap = out.record.map.keys[&ObjKey::AnnotPart(aid, Part::Ap)];
    assert_eq!(ap_dict(&pdf, c).map(|d| d.len()), Some(1), "/N alone");
    assert_eq!(ap_n(&pdf, c), Some(ap));
    let (form, fonts) = saved_form(&pdf, c);
    assert_eq!(fonts, [helv], "the /DR font's own object");
    let content = saved_text(&pdf, c);
    assert!(content.starts_with("/Tx BMC\nq\n"), "{content}");
    assert_eq!(shows(&form.content), bytes_of(&["Bravo"]), "its label");
    let (_, button) = content.split_once("EMC\n").expect("the text's end");
    assert!(button.contains(BUTTON), "the drop button after: {button}");
    assert_eq!(value_of(&pdf, c).as_deref(), Some(b"b".as_slice()));
    assert!(!pdf.get_dictionary(c).unwrap().has(b"I"), "a combo box's");

    let mut want = ids(&plain_pdf);
    assert!(want.remove(&old));
    assert!(want.insert(ap));
    assert_eq!(ids(&pdf), want);
    for &id in want.iter().filter(|&&id| id != c && id != ap) {
        let (now, then) = (
            span(&pdf, &out.bytes, id),
            span(&plain_pdf, &plain.bytes, id),
        );
        assert_eq!(now, then, "{id:?}");
    }
    assert_eq!(span(&pdf, &out.bytes, u), span(&was, &bytes, u));

    let dir = scratch();
    let (path, mut d) = opened(&dir, "c.pdf", &bytes);
    let aid = widget_of(&d, "c");
    fx::choose(&mut d, "c", &["b"]);
    let (base, section) = appended_in_place(&mut d).unwrap();
    let ap = d.saved_record().unwrap().map.keys[&ObjKey::AnnotPart(aid, Part::Ap)];
    assert_eq!(
        written(base.len(), &section, &path),
        BTreeSet::from([c, ap])
    );
    let now = std::fs::read(&path).unwrap();
    let pdf = lazy(&now, None);
    let (form, fonts) = saved_form(&pdf, c);
    assert_eq!(shows(&form.content), bytes_of(&["Bravo"]));
    assert_eq!(fonts, [helv]);
    assert_eq!(value_of(&pdf, c).as_deref(), Some(b"b".as_slice()));
    let (base, section) = appended_in_place(&mut d).unwrap();
    if !section.is_empty() {
        let wrote = written(base.len(), &section, &path);
        assert!(!wrote.contains(&c) && !wrote.contains(&ap), "{wrote:?}");
    }
}

/// An editable combo box's value outside its options shows as it is; a
/// value taken away shows none, `/Tx BMC EMC` with nothing between, and
/// the drop button still.
#[test]
fn an_editable_combo_s_own_text_shows_as_it_is_and_no_value_none() {
    let mut f = Form::new(1);
    let e = choice(&mut f, "e", row(0), ABCD, true);
    f.dict(e).set("Ff", COMBO | flags::EDIT as i64);
    let n = choice(&mut f, "n", row(1), ABCD, true);
    f.fill(n, "a");
    let bytes = table(&mut f);
    let mut d = open(&bytes, None);
    fx::choose(&mut d, "e", &["Zulu 9"]);
    fx::choose(&mut d, "n", &[]);
    let out = rewrite(&d, Profile::default());
    let pdf = lazy(&out.bytes, None);
    let (form, _) = saved_form(&pdf, e);
    assert_eq!(shows(&form.content), bytes_of(&["Zulu 9"]));
    let none = saved_text(&pdf, n);
    assert!(none.contains("/Tx BMC\nEMC\n"), "{none}");
    assert!(shows(none.as_bytes()).is_empty(), "{none}");
    assert!(none.contains(BUTTON), "{none}");
}

/// A list box draws each option from its `/TI`, those its value selects
/// over the highlight: one; several, as an array; every option of an
/// export value two have, both highlighted; from its `/TI` past the first
/// line, the lines before it not drawn.
#[test]
fn a_list_box_highlights_what_its_value_selects_from_its_top() {
    let mut f = Form::new(1);
    let one = choice(&mut f, "one", tall(0), ABCD, false);
    let many = choice(&mut f, "many", tall(3), ABCD, false);
    f.dict(many).set("Ff", flags::MULTI_SELECT as i64);
    let twice = &[("x", "One"), ("y", "Two"), ("x", "Three")];
    let both = choice(&mut f, "both", tall(6), twice, false);
    let top = choice(&mut f, "top", tall(9), ABCD, false);
    f.dict(top).set("TI", 2);
    let bytes = table(&mut f);
    let mut d = open(&bytes, None);
    fx::choose(&mut d, "one", &["b"]);
    fx::choose(&mut d, "many", &["d", "a"]);
    fx::choose(&mut d, "both", &["x"]);
    fx::choose(&mut d, "top", &["c"]);
    let out = rewrite(&d, Profile::default());
    let pdf = lazy(&out.bytes, None);
    let abcd = bytes_of(&["Alpha", "Bravo", "c", "Delta"]);
    for (widget, lines, lit) in [
        (one, abcd.clone(), 1),
        (many, abcd.clone(), 2),
        (both, bytes_of(&["One", "Two", "Three"]), 2),
        (top, bytes_of(&["c", "Delta"]), 1),
    ] {
        let content = saved_text(&pdf, widget);
        assert!(content.starts_with("/Tx BMC\nq\n"), "{content}");
        assert_eq!(shows(content.as_bytes()), lines, "{widget:?}");
        assert_eq!(content.matches(HIGHLIGHT).count(), lit, "{content}");
        assert_eq!(content.matches("1 g\nBT\n").count(), lit, "in white");
    }
}

/// A value the field's font draws all of names that font alone and joins
/// no glyph: no subset is written. `Łukasz` as an option's label draws `Ł`
/// in the fallback, by a Rewrite and by an Append: one subset, of `Ł`
/// alone, which the form binds beside `/DR`'s Helvetica, each code it shows
/// read back as a glyph of the cut program. Beside it a list box whose
/// make was refused (an option it would draw has an emoji) after it drew
/// `Ń` in the fallback: its widget has no `/AP`, and no subset holds `Ń`.
#[test]
fn a_choice_in_two_fonts_writes_its_subset_and_a_refused_one_s_none() {
    let mut f = Form::new(1);
    let helv = f.helv;
    let options = &[("a", "Alpha"), ("l", "\u{141}ukasz")];
    let c = choice(&mut f, "c", row(0), options, true);
    let refused = &[("n", "\u{143}owak"), ("e", "\u{1f600}")];
    let r = choice(&mut f, "r", tall(2), refused, false);
    let bytes = table(&mut f);

    let mut d = open(&bytes, None);
    fx::choose(&mut d, "c", &["a"]);
    let out = rewrite(&d, Profile::default());
    assert_eq!(fallback_subsets(&out.bytes), [], "one font alone");
    assert!(out.record.fallbacks.is_empty());
    assert_eq!(saved_form(&lazy(&out.bytes, None), c).1, [helv]);

    fx::choose(&mut d, "c", &["l"]);
    fx::choose(&mut d, "r", &["n"]);
    let (out, said) = warned(|| rewrite(&d, Profile::default()));
    let pdf = lazy(&out.bytes, None);
    let subsets = fallback_subsets(&out.bytes);
    let [(font, codes)] = &subsets[..] else {
        panic!("one subset: {subsets:?}")
    };
    assert_eq!(*codes, glyphs_of("\u{141}"), "Ł alone, never Ń");
    assert!(subsets_hold(&out.bytes) > 0);
    let fonts: BTreeSet<ObjectId> = saved_form(&pdf, c).1.into_iter().collect();
    assert_eq!(fonts, BTreeSet::from([helv, *font]));
    let [(id, glyphs)] = &out.record.fallbacks[..] else {
        panic!("the record's subset")
    };
    assert_eq!(out.record.map.keys[&ObjKey::Res(*id)], *font);
    assert_eq!(
        glyphs.iter().map(|(g, _)| g).collect::<BTreeSet<_>>(),
        *codes
    );
    assert!(ap_dict(&pdf, r).is_none(), "the refused one's: none");
    let unmade: Vec<&String> = said.iter().filter(|w| w.contains("appearance")).collect();
    assert_eq!(unmade.len(), 1, "{said:?}");
    assert!(unmade[0].contains("unencodable-option"), "{said:?}");

    let dir = scratch();
    let (path, mut d) = opened(&dir, "c.pdf", &bytes);
    let aid = widget_of(&d, "c");
    fx::choose(&mut d, "c", &["l"]);
    fx::choose(&mut d, "r", &["n"]);
    let (base, section) = appended_in_place(&mut d).unwrap();
    let record = d.saved_record().unwrap();
    let [(s, g)] = &record.fallbacks[..] else {
        panic!("one subset")
    };
    let s_at = record.map.keys[&ObjKey::Res(*s)];
    let ap = record.map.keys[&ObjKey::AnnotPart(aid, Part::Ap)];
    let wrote = written(base.len(), &section, &path);
    assert!(
        wrote.contains(&c) && wrote.contains(&ap) && wrote.contains(&s_at) && wrote.contains(&r),
        "{wrote:?}"
    );
    let now = std::fs::read(&path).unwrap();
    assert_eq!(
        fallback_subsets(&now),
        [(s_at, g.iter().map(|(g, _)| g).collect())]
    );
    assert_eq!(fallback_subsets(&now)[0].1, glyphs_of("\u{141}"));
    assert!(subsets_hold(&now) > 0);
    let pdf = lazy(&now, None);
    assert!(ap_dict(&pdf, r).is_none(), "no /AP on the refused one");
}

/// A cluster neither font draws leaves its widget unmade, with no `/AP`
/// and no subset: an editable combo box's own text, `unencodable`; a list
/// box's option it would draw, `unencodable-option`. The warning says the
/// kind, never the text. The op refuses the combo box's text, so it's put
/// as only a file gives one (`fx::put_value`); a list box's option no font
/// draws doesn't refuse a choice of another.
#[test]
fn what_no_font_draws_leaves_a_choice_with_no_appearance() {
    let mut f = Form::new(1);
    let e = choice(&mut f, "e", row(0), ABCD, true);
    f.dict(e).set("Ff", COMBO | flags::EDIT as i64);
    let l = choice(
        &mut f,
        "l",
        tall(2),
        &[("a", "a"), ("w", "\u{738b}")],
        false,
    );
    let bytes = table(&mut f);
    for (name, widget, value, kind) in [
        ("e", e, "\u{141}ukasz \u{738b}", "unencodable"),
        ("l", l, "a", "unencodable-option"),
    ] {
        let mut d = open(&bytes, None);
        match kind {
            "unencodable" => fx::put_value(&mut d, name, Some(Value::Text(value.into()))),
            _ => fx::choose(&mut d, name, &[value]),
        }
        let (out, said) = warned(|| rewrite(&d, Profile::default()));
        let pdf = lazy(&out.bytes, None);
        assert!(ap_dict(&pdf, widget).is_none(), "{name}: no /AP");
        assert_eq!(fallback_subsets(&out.bytes), [], "{name}");
        assert!(out.record.fallbacks.is_empty(), "{name}");
        let unmade: Vec<&String> = said.iter().filter(|w| w.contains("appearance")).collect();
        assert_eq!(unmade.len(), 1, "{said:?}");
        assert!(unmade[0].contains(&format!("({kind})")), "{said:?}");
        assert!(
            !said
                .iter()
                .any(|w| w.contains("ukasz") || w.contains('\u{738b}'))
        );
    }
}

/// A list box whose model holds more options than sheaf reads of an
/// `/Opt` (a model edited by hand: the reader takes none so long) is
/// unmade as `limit:items`, its widget with no `/AP`.
#[test]
fn a_list_box_past_sheaf_s_items_is_unmade() {
    let mut f = Form::new(1);
    let l = choice(&mut f, "l", tall(0), ABCD, false);
    let bytes = table(&mut f);
    let mut d = open(&bytes, None);
    let id = d.state().field_named("l").expect("the field");
    let mut node = FieldNode::clone(d.state().field(id).expect("a field"));
    let mut term = Terminal::clone(node.term.as_deref().expect("a terminal"));
    let items = crate::source::Limits::DEFAULT.forms.items;
    let options: Arc<[Choice]> = (0..=items)
        .map(|i| {
            let text: Arc<str> = Arc::from(format!("Option {i}"));
            Choice {
                export: text.clone(),
                shown: text,
            }
        })
        .collect();
    term.props = Arc::new(FieldProps {
        options,
        ..FieldProps::clone(&term.props)
    });
    term.value = Some(crate::forms::Value::Text("Option 1".into()));
    node.term = Some(Arc::new(term));
    d.put_for_test(
        vec![(Key::Field(id), Some(Val::Field(Arc::new(node))))],
        "options",
    );
    let (out, said) = warned(|| rewrite(&d, Profile::default()));
    assert!(ap_dict(&lazy(&out.bytes, None), l).is_none());
    let unmade: Vec<&String> = said.iter().filter(|w| w.contains("appearance")).collect();
    assert_eq!(unmade.len(), 1, "{said:?}");
    assert!(unmade[0].contains("(limit:items)"), "{said:?}");
}

/// A choice field of two kid widgets, on two pages: each is made, its own
/// form under its own key, each showing the value.
#[test]
fn each_widget_of_a_choice_gets_its_form() {
    let mut f = Form::new(2);
    let field = f.pdf.add_object(dictionary! {
        "FT" => Object::name("Ch"), "T" => literal("two"), "Ff" => COMBO,
        "DA" => literal("/Helv 10 Tf 0 g"), "Opt" => opt(ABCD), "V" => literal("a"),
    });
    let mut kids = Vec::new();
    for page in 0..2 {
        let r: Vec<Object> = row(page).iter().map(|&v| Object::from(v)).collect();
        let kid = f.pdf.add_object(dictionary! {
            "Type" => Object::name("Annot"), "Subtype" => Object::name("Widget"),
            "Rect" => r, "F" => 4, "P" => f.pages[page], "Parent" => field,
        });
        let mut annots = f.annots(page);
        annots.push(kid);
        f.set_annots(page, &annots);
        kids.push(kid);
    }
    let list: Vec<Object> = kids.iter().map(|&k| Object::from(k)).collect();
    f.dict(field).set("Kids", list);
    let mut fields = f.fields();
    fields.push(field);
    f.set_fields(&fields);
    let bytes = table(&mut f);
    let mut d = open(&bytes, None);
    let aids = widgets_of(&d, "two");
    assert_eq!(aids.len(), 2);
    fx::choose(&mut d, "two", &["d"]);
    let out = rewrite(&d, Profile::default());
    let pdf = lazy(&out.bytes, None);
    let mut forms = BTreeSet::new();
    for (aid, kid) in aids.into_iter().zip(kids) {
        let ap = out.record.map.keys[&ObjKey::AnnotPart(aid, Part::Ap)];
        assert_eq!(ap_n(&pdf, kid), Some(ap), "{kid:?}");
        assert_eq!(
            shows(&saved_form(&pdf, kid).0.content),
            bytes_of(&["Delta"])
        );
        forms.insert(ap);
    }
    assert_eq!(forms.len(), 2, "a form each");
}

/// An overlay of the page holds the save's form for a changed combo box,
/// list box (multiple-select, two chosen) and one in two fonts, form for
/// form (its box, matrix, content and font names), and no other; the page
/// draws as its base does again once each value is put back.
#[test]
fn an_overlay_s_forms_are_the_save_s() {
    let mut f = Form::new(1);
    let c = choice(&mut f, "c", row(0), ABCD, true);
    f.fill(c, "a");
    let l = choice(&mut f, "l", tall(2), ABCD, false);
    f.dict(l).set("Ff", flags::MULTI_SELECT as i64);
    let options = &[("a", "Alpha"), ("l", "\u{141}ukasz")];
    let two = choice(&mut f, "two", row(5), options, true);
    let bytes = table(&mut f);
    let mut d = open(&bytes, None);
    let screen = ViewInputs::screen();
    let p0 = d.page(0).unwrap();
    assert!(untouched(d.state(), p0, &screen));
    fx::choose(&mut d, "c", &["b"]);
    fx::choose(&mut d, "l", &["a", "c"]);
    fx::choose(&mut d, "two", &["l"]);
    assert!(!untouched(d.state(), p0, &screen));
    let out = rewrite(&d, Profile::default());
    let pdf = lazy(&out.bytes, None);
    let overlay = overlay_appearances(d.state(), p0, &screen).unwrap();
    let widgets = [
        (widget_of(&d, "c"), c),
        (widget_of(&d, "l"), l),
        (widget_of(&d, "two"), two),
    ];
    for (aid, w) in widgets {
        let over = (overlay.iter()).find(|(a, p, _)| *a == aid && *p == Part::Ap);
        let (_, _, over) = over.unwrap_or_else(|| panic!("{aid:?}: the overlay's"));
        assert_eq!(made(over).0, saved_form(&pdf, w).0, "{aid:?}");
    }
    assert_eq!(overlay.len(), 3, "no other form");
    fx::choose(&mut d, "c", &["a"]);
    fx::choose(&mut d, "l", &[]);
    fx::choose(&mut d, "two", &[]);
    assert!(untouched(d.state(), p0, &screen), "put back");
}

// ----- the value (M10 slice 9) ----------------------------------------------

/// The indexes `field`'s `/I` holds in `pdf`; none where it has none.
fn list_indexes(pdf: &LazyPdf, field: ObjectId) -> Option<Vec<i64>> {
    let i = pdf.get_dictionary(field).ok()?.get(b"I")?;
    let i = i.as_array().expect("an array");
    Some(
        i.iter()
            .map(|n| n.as_integer().expect("an index"))
            .collect(),
    )
}

/// What the family's reading of the list box named `name` in the file
/// `bytes` selects (by `/I` where it agrees with `/V`) and shows at its
/// top (`/TI`).
fn family_selects(bytes: &[u8], name: &str) -> (Vec<usize>, usize) {
    let src = super::tests::source(bytes, None);
    let pdf = src.pdf();
    let unkept = Unkept::default();
    let objects = Objects::new(pdf, &unkept);
    let limits = family::Limits::DEFAULT;
    let form = (family::read(objects, pdf.catalog().unwrap(), &limits).unwrap()).expect("a form");
    let (_, field) = (form.terminals())
        .find(|(i, _)| form.full_name(*i).as_deref() == Some(name))
        .unwrap_or_else(|| panic!("the family reads no {name}"));
    let choice = family::field_choice(field, objects, &limits).expect("a choice");
    let list = choice.list().expect("a list box");
    let mut selected = list.selected.to_vec();
    selected.sort_unstable();
    (selected, list.top)
}

/// A list box's value is a save's `/V` and what it selects its `/I` (M10
/// slice 9), by a Rewrite and by an Append, read back alike by sheaf's
/// reader and the family's, the family selecting by that `/I`: one option
/// (the file's `/I` replaced), several of a multiple-select one, a value
/// two options share as their export value (both selected), and an `/Opt`
/// whose first item is no text (`[1 (a) (b)]`, `/TI 1`), its `/I` the
/// file's place. A combo box's `/V` is the value and it has no `/I`, the
/// file's left out.
#[test]
fn a_choice_s_value_and_its_list_box_s_indexes_are_written_both_ways() {
    let mut f = Form::new(1);
    let one = choice(&mut f, "one", tall(0), ABCD, false);
    f.dict(one).set("V", literal("a"));
    f.dict(one).set("I", vec![Object::from(0)]);
    let several = choice(&mut f, "several", tall(2), ABCD, false);
    f.dict(several).set("Ff", i64::from(flags::MULTI_SELECT));
    let shared = &[("a", "First"), ("a", "Second"), ("b", "b")];
    choice(&mut f, "twice", tall(4), shared, false);
    let placed = choice(&mut f, "placed", tall(6), &[], false);
    f.dict(placed)
        .set("Opt", vec![1.into(), literal("a"), literal("b")]);
    f.dict(placed).set("TI", 1);
    let combo = choice(&mut f, "combo", row(8), ABCD, true);
    f.dict(combo).set("V", literal("a"));
    f.dict(combo).set("I", vec![Object::from(0)]);
    let bytes = table(&mut f);
    for how in ["a Rewrite", "an Append"] {
        let dir = scratch();
        let (path, mut d) = opened(&dir, "c.pdf", &bytes);
        fx::choose(&mut d, "one", &["c"]);
        fx::choose(&mut d, "several", &["d", "b"]);
        fx::choose(&mut d, "twice", &["a"]);
        fx::choose(&mut d, "placed", &["b"]);
        fx::choose(&mut d, "combo", &["b"]);
        saved_each_way(&mut d, &path, how);
        let now = std::fs::read(&path).unwrap();
        let pdf = lazy(&now, None);
        let items = |texts: &[&str]| {
            let ours = texts.iter().map(|&t| Arc::from(t)).collect();
            let theirs = texts.iter().map(|&t| t.to_string()).collect();
            (Some(Value::Items(ours)), Some(Theirs::Items(theirs)))
        };
        let values = [
            ("one", text_read("c"), Some(vec![2]), (vec![2], 0)),
            (
                "several",
                items(&["b", "d"]),
                Some(vec![1, 3]),
                (vec![1, 3], 0),
            ),
            ("twice", text_read("a"), Some(vec![0, 1]), (vec![0, 1], 0)),
            ("placed", text_read("b"), Some(vec![2]), (vec![2], 1)),
        ];
        for (name, value, indexes, selects) in values {
            let id = (d.state().field_named(name))
                .and_then(|f| d.state().field(f)?.src.as_ref().map(|r| r.id))
                .unwrap();
            assert_eq!(read_back(&now, name), value, "{how}: {name}");
            assert_eq!(list_indexes(&pdf, id), indexes, "{how}: {name}");
            assert_eq!(family_selects(&now, name), selects, "{how}: {name}");
        }
        assert_eq!(read_back(&now, "combo"), text_read("b"), "{how}");
        assert_eq!(list_indexes(&pdf, combo), None, "{how}: a combo box's");
        let shown = shows(&saved_form(&pdf, placed).0.content);
        assert_eq!(shown, bytes_of(&["a", "b"]), "{how}: from /TI on");
    }
}

/// A combo box or list box cleared under a parent that holds a `/V` is
/// written with no value of its own that selects nothing: a combo box's
/// the empty text, a list box's an empty array (the empty text would select
/// an option of empty text), and no `/I`. Both ways, read back alike by
/// sheaf and the family.
#[test]
fn a_cleared_choice_under_a_parent_holding_a_value_selects_nothing() {
    let mut f = Form::new(1);
    let list = choice(&mut f, "l", tall(0), ABCD, false);
    f.dict(list).set("V", literal("b"));
    f.dict(list).set("I", vec![Object::from(1)]);
    let combo = choice(&mut f, "c", row(4), ABCD, true);
    f.dict(combo).set("V", literal("b"));
    let parent = f.group("p", &[list, combo]);
    f.dict(parent).set("V", literal("a"));
    let bytes = table(&mut f);
    for how in ["a Rewrite", "an Append"] {
        let dir = scratch();
        let (path, mut d) = opened(&dir, "c.pdf", &bytes);
        fx::choose(&mut d, "p.l", &[]);
        fx::choose(&mut d, "p.c", &[]);
        saved_each_way(&mut d, &path, how);
        let now = std::fs::read(&path).unwrap();
        let pdf = lazy(&now, None);
        let none = (
            Some(Value::Items(Arc::from([]))),
            Some(Theirs::Items(Vec::new())),
        );
        assert_eq!(read_back(&now, "p.l"), none, "{how}");
        assert_eq!(list_indexes(&pdf, list), None, "{how}");
        assert_eq!(family_selects(&now, "p.l"), (Vec::new(), 0), "{how}");
        assert_eq!(read_back(&now, "p.c"), text_read(""), "{how}");
        assert_eq!(value_of(&pdf, parent).as_deref(), Some(b"a".as_slice()));
    }
}
