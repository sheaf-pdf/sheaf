//! A check box's or radio button's appearances and state written where the
//! model holds its state changed (M10 slice 8), through the emitter: a
//! Rewrite and an Append each write `/AS` from the model, and where the
//! widget's file draws no on state, its forms in state dictionaries, each
//! under its own key, `/Off` where it paints anything, and nothing else
//! anew but one ZapfDingbats a file; a widget whose file draws an on state
//! keeps its appearances, object for object; a state put back is the
//! file's widget again; every kind of widget (merged with its field, split
//! from it, a duplicate's, an inserted file's); an overlay and a save
//! holding the same forms; saves in a row; and a box of no size.

use std::collections::BTreeSet;

use crate::pdf::{
    Dictionary, LazyPdf, Object, ObjectId, Stream, StreamExt, XrefKind, dictionary, literal,
};
use in_progress::NoProgress;

use super::append_tests::{appended_in_place, written};
use super::key::{ObjKey, Part};
use super::removal_tests::{opened, scratch, warned};
use super::tests::{add, insert, lazy, open, rewrite, span};
use super::*;
use crate::annot::AnnotId;
use crate::forms::appearance::tests::shown;
use crate::forms::fixture::{Button, Form, RADIO, row};
use crate::model::Document;
use crate::realize::{ViewInputs, overlay_appearances, untouched};
use crate::tx::fixture as fx;

/// A check box's or radio button's parts, in the order the emitter makes
/// them.
const PARTS: [Part; 4] = [Part::Ap, Part::ApOff, Part::ApDown, Part::ApDownOff];

/// `f`'s file, its cross-reference a table, so a Rewrite writes each object
/// on its own and an object nothing changed is its bytes as they were.
fn table(f: &mut Form) -> Vec<u8> {
    f.pdf.xref_kind = XrefKind::Table;
    f.bytes()
}

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

/// The state dictionary `/AP`'s `entry` (`N` or `D`) is for `widget` in
/// `pdf`: each state's name and the form it names, in order.
fn states(pdf: &LazyPdf, widget: ObjectId, entry: &[u8]) -> States {
    let ap = ap_dict(pdf, widget).expect("an /AP");
    let states = ap.get(entry).unwrap_or_else(|| panic!("no /{entry:?}"));
    let states = pdf
        .dereference(states)
        .unwrap()
        .1
        .as_dict()
        .expect("states");
    (states.iter())
        .map(|(k, v)| (k.to_vec(), v.as_reference().expect("a form by reference")))
        .collect()
}

/// `name`, as [`states`] gives a state's name.
fn n(name: &str) -> Vec<u8> {
    name.as_bytes().to_vec()
}

/// The state `/AS` names on `widget` in `pdf`.
fn shows(pdf: &LazyPdf, widget: ObjectId) -> Option<Vec<u8>> {
    let d = pdf.get_dictionary(widget).ok()?;
    d.get(b"AS")?.as_name().map(<[u8]>::to_vec)
}

/// The `/V` name of `field` in `pdf`.
fn value_of(pdf: &LazyPdf, field: ObjectId) -> Option<Vec<u8>> {
    let d = pdf.get_dictionary(field).ok()?;
    d.get(b"V")?.as_name().map(<[u8]>::to_vec)
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

/// The objects `pdf`'s trailer reaches: what its latest revision holds.
fn live(pdf: &LazyPdf) -> BTreeSet<ObjectId> {
    let mut seen = BTreeSet::new();
    let trailer = Object::from(pdf.trailer().clone());
    let mut stack: Vec<ObjectId> = trailer.references().collect();
    while let Some(id) = stack.pop() {
        if seen.insert(id)
            && let Ok(o) = pdf.get(id)
        {
            stack.extend(o.references());
        }
    }
    seen
}

/// Every object `pdf` holds.
fn ids(pdf: &LazyPdf) -> BTreeSet<ObjectId> {
    pdf.ids().collect()
}

/// The numbers `map` gives each of widget `aid`'s parts it holds, in
/// [`PARTS`]' order.
fn parts_in(map: &record::ObjMap, aid: AnnotId) -> Vec<Option<ObjectId>> {
    (PARTS.iter())
        .map(|&part| map.keys.get(&ObjKey::AnnotPart(aid, part)).copied())
        .collect()
}

/// A state dictionary's states: each one's name and the form it names.
type States = Vec<(Vec<u8>, ObjectId)>;

/// Widget `w`'s `/AP` in `pdf`, wherever each part was read: each of its
/// entries' name, and the form it names or each state's name and form.
fn ap_of(pdf: &LazyPdf, w: ObjectId) -> Option<Vec<(Vec<u8>, States)>> {
    let ap = ap_dict(pdf, w)?;
    let entries = (ap.iter())
        .map(|(k, v)| {
            let at = match v.as_reference() {
                Some(id) if pdf.get(id).is_ok_and(|o| o.as_stream().is_some()) => {
                    vec![(Vec::new(), id)]
                }
                _ => states(pdf, w, k),
            };
            (k.to_vec(), at)
        })
        .collect();
    Some(entries)
}

/// A check box `name`, a kid widget of its field at row `i` on page 0,
/// with no `/AP`, showing `shows`, its border black (`/MK /BC`), so its
/// `Off` forms draw the frame. Its field and its widget.
fn framed_box(f: &mut Form, name: &str, i: usize, shows: &str) -> (ObjectId, ObjectId) {
    let (field, kids) = f.buttons(name, 0, &[Button::at(i, None, Some(shows))]);
    f.dict(kids[0])
        .set("MK", dictionary! { "BC" => vec![Object::from(0)] });
    (field, kids[0])
}

/// Object `id` of `pdf`, read from `bytes`, from its `obj` on.
fn raw(pdf: &LazyPdf, bytes: &[u8], id: ObjectId) -> Vec<u8> {
    let span = span(pdf, bytes, id).unwrap();
    let at = (span.windows(3).position(|x| x == b"obj")).unwrap();
    span[at..].to_vec()
}

/// A Rewrite of a check box with no `/AP`, off in its file, pressed on:
/// `/N` and `/D` each `<< /Yes … /Off … >>`, the on state's name a press
/// puts, each form under its key; `/AS /Yes`, its field's `/V` the file's
/// (no save writes a value before M10 slice 9); the on forms the mark in
/// ZapfDingbats, the standard font's dictionary, which nothing else names.
/// Nothing else anew: the objects are the unpressed Rewrite's and the four
/// forms and the font, every other one its bytes as they were. An Append
/// of the same press writes the widget, the four and the font, and the
/// field, whose value the press changed, as its file has it; a second
/// Append in a row writes none of them.
#[test]
fn a_pressed_check_box_with_no_appearance_gets_four_forms_and_its_as() {
    let mut f = Form::new(1);
    let (field, w) = framed_box(&mut f, "cb", 0, "Off");
    let bytes = table(&mut f);
    let plain = rewrite(&open(&bytes, None), Profile::default());
    let plain_pdf = lazy(&plain.bytes, None);
    assert!(ap_dict(&plain_pdf, w).is_none());

    let mut d = open(&bytes, None);
    let aid = widget_of(&d, "cb");
    fx::press(&mut d, aid);
    let out = rewrite(&d, Profile::default());
    let pdf = lazy(&out.bytes, None);
    let parts = parts_in(&out.record.map, aid);
    let [on, off, down, down_off] = [0, 1, 2, 3].map(|i| parts[i].expect("each part"));
    assert_eq!(states(&pdf, w, b"N"), [(n("Yes"), on), (n("Off"), off)]);
    assert_eq!(
        states(&pdf, w, b"D"),
        [(n("Yes"), down), (n("Off"), down_off)]
    );
    assert_eq!(ap_dict(&pdf, w).unwrap().len(), 2, "/N and /D alone");
    assert_eq!(shows(&pdf, w), Some(n("Yes")));
    assert_eq!(value_of(&pdf, field), Some(n("Off")), "the file's /V");

    let (on_form, fonts) = made(&stream(&pdf, on));
    assert_eq!(shown(&on_form.content), b"4");
    let [zapf] = fonts[..] else {
        panic!("one font: {fonts:?}")
    };
    let font = pdf.get_dictionary(zapf).unwrap();
    let name = |k: &[u8]| font.get(k).and_then(Object::as_name).map(<[u8]>::to_vec);
    assert_eq!(name(b"BaseFont"), Some(n("ZapfDingbats")));
    assert!(!font.has(b"Encoding") && !font.has(b"FontDescriptor"));
    assert_eq!(made(&stream(&pdf, down)).1, [zapf]);
    for id in [off, down_off] {
        assert_eq!(made(&stream(&pdf, id)).1, [], "{id:?}: Off draws no mark");
    }

    let mut want = ids(&plain_pdf);
    for id in [on, off, down, down_off, zapf] {
        assert!(want.insert(id), "{id:?} anew");
    }
    assert_eq!(ids(&pdf), want);
    for &id in want
        .iter()
        .filter(|&&id| ![w, on, off, down, down_off, zapf].contains(&id))
    {
        let (now, then) = (
            span(&pdf, &out.bytes, id),
            span(&plain_pdf, &plain.bytes, id),
        );
        assert_eq!(now, then, "{id:?}");
    }

    // An Append, then another with nothing changed.
    let dir = scratch();
    let (path, mut d) = opened(&dir, "a.pdf", &bytes);
    let aid = widget_of(&d, "cb");
    fx::press(&mut d, aid);
    let (base, section) = appended_in_place(&mut d).unwrap();
    let record = d.saved_record().unwrap();
    let parts: Vec<ObjectId> = (parts_in(&record.map, aid).into_iter())
        .map(|p| p.expect("each part"))
        .collect();
    let now = std::fs::read(&path).unwrap();
    let pdf = lazy(&now, None);
    let zapf = made(&stream(&pdf, parts[0])).1[0];
    let mut want: BTreeSet<ObjectId> = parts.iter().copied().collect();
    want.extend([w, zapf, field]);
    assert_eq!(written(base.len(), &section, &path), want);
    let was = lazy(&bytes, None);
    assert_eq!(
        raw(&pdf, &now, field),
        raw(&was, &bytes, field),
        "its field as its file has it"
    );
    assert_eq!(shows(&pdf, w), Some(n("Yes")));
    assert_eq!(
        states(&pdf, w, b"N"),
        [(n("Yes"), parts[0]), (n("Off"), parts[1])]
    );
    assert_eq!(
        states(&pdf, w, b"D"),
        [(n("Yes"), parts[2]), (n("Off"), parts[3])]
    );
    let (base, section) = appended_in_place(&mut d).unwrap();
    if !section.is_empty() {
        let wrote = written(base.len(), &section, &path);
        assert!(wrote.is_disjoint(&want), "{wrote:?}");
    }
}

/// An `Off` that would paint nothing isn't made: a check box with no `/MK`
/// gets no `/Off` in `/N`, and its pressed gray in `/D`, three forms; a
/// radio button beveled with no width and no background, none in either,
/// two. The record holds no key for a part not made.
#[test]
fn an_off_that_paints_nothing_is_not_made() {
    let mut f = Form::new(1);
    let (_, plain) = f.buttons("plain", 0, &[Button::at(0, None, Some("Off"))]);
    let (_, flat) = f.buttons("flat", RADIO, &[Button::at(1, None, Some("Off"))]);
    f.dict(flat[0])
        .set("BS", dictionary! { "W" => 0, "S" => Object::name("B") });
    let bytes = table(&mut f);
    let mut d = open(&bytes, None);
    let (pa, fa) = (widget_of(&d, "plain"), widget_of(&d, "flat"));
    fx::press(&mut d, pa);
    fx::press(&mut d, fa);
    let out = rewrite(&d, Profile::default());
    let pdf = lazy(&out.bytes, None);

    let [on, off, down, down_off] = parts_in(&out.record.map, pa)[..] else {
        unreachable!()
    };
    assert!(off.is_none());
    let (on, down, down_off) = (on.unwrap(), down.unwrap(), down_off.unwrap());
    assert_eq!(states(&pdf, plain[0], b"N"), [(n("Yes"), on)]);
    assert_eq!(
        states(&pdf, plain[0], b"D"),
        [(n("Yes"), down), (n("Off"), down_off)]
    );
    assert_eq!(shows(&pdf, plain[0]), Some(n("Yes")));

    let [on, off, down, down_off] = parts_in(&out.record.map, fa)[..] else {
        unreachable!()
    };
    assert!(off.is_none() && down_off.is_none());
    assert_eq!(states(&pdf, flat[0], b"N"), [(n("Yes"), on.unwrap())]);
    assert_eq!(states(&pdf, flat[0], b"D"), [(n("Yes"), down.unwrap())]);
    assert_eq!(shows(&pdf, flat[0]), Some(n("Yes")));
}

/// A check box pressed and saved, then pressed back to its file's state
/// and saved by an Append and then by a Rewrite: each time the widget is
/// its file's again, no `/AP` and `/AS /Off`, the Append writing it and
/// its field (whose value moved back) as the file has them over the
/// numbers they had, and none of the four parts the first save wrote is
/// among the file's live objects; the Rewrite holds no part at all. The
/// page draws as its file does again.
#[test]
fn a_check_box_put_back_is_its_file_s_again() {
    let dir = scratch();
    let mut f = Form::new(1);
    let (field, w) = framed_box(&mut f, "cb", 0, "Off");
    let bytes = table(&mut f);
    let was = lazy(&bytes, None);
    let (path, mut d) = opened(&dir, "a.pdf", &bytes);
    let aid = widget_of(&d, "cb");

    fx::press(&mut d, aid);
    appended_in_place(&mut d).unwrap();
    let made: Vec<ObjectId> = (parts_in(&d.saved_record().unwrap().map, aid).into_iter())
        .map(|p| p.expect("each part"))
        .collect();
    let pdf = lazy(&std::fs::read(&path).unwrap(), None);
    assert!(made.iter().all(|id| live(&pdf).contains(id)));

    fx::press(&mut d, aid);
    let screen = ViewInputs::screen();
    let p0 = d.state().order()[0];
    assert!(untouched(d.state(), p0, &screen), "drawn as its file is");
    let (base, section) = appended_in_place(&mut d).unwrap();
    assert_eq!(
        written(base.len(), &section, &path),
        BTreeSet::from([w, field])
    );
    let now = std::fs::read(&path).unwrap();
    let pdf = lazy(&now, None);
    for id in [w, field] {
        assert_eq!(
            raw(&pdf, &now, id),
            raw(&was, &bytes, id),
            "{id:?}: the file's"
        );
    }
    assert!(ap_dict(&pdf, w).is_none());
    assert_eq!(shows(&pdf, w), Some(n("Off")));
    let reached = live(&pdf);
    assert!(made.iter().all(|id| !reached.contains(id)), "{made:?}");

    save_with(
        &mut d,
        &path,
        SaveHow::Rewrite,
        &AskAnswers::default(),
        &NoProgress,
    )
    .unwrap();
    let now = std::fs::read(&path).unwrap();
    let pdf = lazy(&now, None);
    assert!(ap_dict(&pdf, w).is_none());
    assert_eq!(shows(&pdf, w), Some(n("Off")));
    assert_eq!(parts_in(&d.saved_record().unwrap().map, aid), [None; 4]);
    let reached = live(&pdf);
    assert_eq!(ids(&pdf), reached, "a Rewrite holds its live objects alone");
}

/// A check box whose file shows `/Yes` with no `/AP`, pressed off: its whole
/// set made, `/AS /Off`, the on state named `/Yes` as its file names it.
#[test]
fn a_file_s_yes_with_no_appearance_pressed_off_gets_the_whole_set() {
    let mut f = Form::new(1);
    let (_, w) = framed_box(&mut f, "cb", 0, "Yes");
    let bytes = table(&mut f);
    let mut d = open(&bytes, None);
    let aid = widget_of(&d, "cb");
    fx::press(&mut d, aid);
    let out = rewrite(&d, Profile::default());
    let pdf = lazy(&out.bytes, None);
    let parts: Vec<ObjectId> = (parts_in(&out.record.map, aid).into_iter())
        .map(|p| p.expect("each part"))
        .collect();
    assert_eq!(shows(&pdf, w), Some(n("Off")));
    assert_eq!(
        states(&pdf, w, b"N"),
        [(n("Yes"), parts[0]), (n("Off"), parts[1])]
    );
    assert_eq!(
        states(&pdf, w, b"D"),
        [(n("Yes"), parts[2]), (n("Off"), parts[3])]
    );
}

/// The on state's name and the mark: under `/Opt` each widget's index,
/// the pressed one's `/AS` that name and the other's left `/Off`, as its
/// file has it; a radio button's mark the circle `l` and its frame round
/// (its down `Off` drawn in curves, a check box's a rectangle); and each
/// caption's glyph, `4`, `8`, `6` and `l`, with none a check box's `4`.
#[test]
fn the_on_state_is_named_as_a_press_names_it_and_the_mark_as_its_caption() {
    let mut f = Form::new(1);
    let opt = [
        Button::at(0, None, Some("Off")),
        Button::at(1, None, Some("Off")),
    ];
    let (field, opt_kids) = f.buttons("opt", 0, &opt);
    f.dict(field)
        .set("Opt", vec![literal("first"), literal("second")]);
    let (_, radio) = f.buttons("radio", RADIO, &[Button::at(2, None, Some("Off"))]);
    let mut captions = Vec::new();
    for (i, caption) in [Some("4"), Some("8"), Some("6"), Some("l"), None]
        .into_iter()
        .enumerate()
    {
        let name = format!("ca{i}");
        let (_, kids) = f.buttons(&name, 0, &[Button::at(3 + i, None, Some("Off"))]);
        if let Some(caption) = caption {
            f.dict(kids[0])
                .set("MK", dictionary! { "CA" => literal(caption) });
        }
        captions.push((name, kids[0], caption.unwrap_or("4")));
    }
    let bytes = table(&mut f);
    let mut d = open(&bytes, None);
    let second = widgets_of(&d, "opt")[1];
    fx::press(&mut d, second);
    let ra = widget_of(&d, "radio");
    fx::press(&mut d, ra);
    for (name, _, _) in &captions {
        let aid = widget_of(&d, name);
        fx::press(&mut d, aid);
    }
    let out = rewrite(&d, Profile::default());
    let pdf = lazy(&out.bytes, None);

    let on = states(&pdf, opt_kids[1], b"N");
    assert_eq!(
        on.iter().map(|(k, _)| k.clone()).collect::<Vec<_>>(),
        [n("1")]
    );
    assert_eq!(shows(&pdf, opt_kids[1]), Some(n("1")));
    assert_eq!(shows(&pdf, opt_kids[0]), Some(n("Off")));
    assert!(
        ap_dict(&pdf, opt_kids[0]).is_none(),
        "nothing changed of it"
    );

    let curves = |id: ObjectId| {
        let content = made(&stream(&pdf, id)).0.content;
        content.windows(3).any(|w| w == b" c\n")
    };
    let [(_, on), ..] = states(&pdf, radio[0], b"N")[..] else {
        panic!("an on state")
    };
    assert_eq!(shown(&made(&stream(&pdf, on)).0.content), b"l");
    let down = states(&pdf, radio[0], b"D");
    assert!(curves(down[1].1), "round");
    for (name, widget, caption) in &captions {
        let [(_, on)] = states(&pdf, *widget, b"N")[..] else {
            panic!("{name}: an on state alone")
        };
        let content = made(&stream(&pdf, on)).0.content;
        assert_eq!(shown(&content), caption.as_bytes(), "{name}");
        let down = states(&pdf, *widget, b"D");
        assert!(!curves(down[1].1), "{name}: square");
    }
}

/// A widget whose file draws an on state keeps its `/AP`, object for
/// object, and only `/AS` moves: one merged with its field and a kid, by a
/// Rewrite (every object the unpressed Rewrite's, but the two widgets) and
/// by an Append (the two widgets written, nothing made); one whose file
/// shows no `/AS` gains it.
#[test]
fn a_widget_whose_file_draws_an_on_state_keeps_its_appearances() {
    let mut f = Form::new(1);
    let merged = f.check_box("merged", 0, row(0), false);
    let (_, kid) = f.buttons("kid", 0, &[Button::at(1, Some("Yes"), Some("Off"))]);
    let (_, bare) = f.buttons("bare", 0, &[Button::at(2, Some("On"), None)]);
    let (kid, bare) = (kid[0], bare[0]);
    let bytes = table(&mut f);
    let was = lazy(&bytes, None);
    let plain = rewrite(&open(&bytes, None), Profile::default());
    let plain_pdf = lazy(&plain.bytes, None);
    let press_all = |d: &mut Document| {
        for name in ["merged", "kid", "bare"] {
            let aid = widget_of(d, name);
            fx::press(d, aid);
        }
    };

    let mut d = open(&bytes, None);
    press_all(&mut d);
    let out = rewrite(&d, Profile::default());
    let pdf = lazy(&out.bytes, None);
    for (w, on) in [(merged, "Yes"), (kid, "Yes"), (bare, "On")] {
        assert_eq!(shows(&pdf, w), Some(n(on)), "{w:?}");
        assert_eq!(ap_of(&pdf, w), ap_of(&plain_pdf, w), "{w:?}");
        let aid = (d.state().annots())
            .find(|(_, rec)| {
                rec.widget()
                    .and_then(|x| x.src.as_ref())
                    .is_some_and(|r| r.id == w)
            })
            .map(|(aid, _)| aid)
            .unwrap();
        assert_eq!(parts_in(&out.record.map, aid), [None; 4], "nothing made");
    }
    assert_eq!(ids(&pdf), ids(&plain_pdf));
    for id in ids(&pdf)
        .into_iter()
        .filter(|id| ![merged, kid, bare].contains(id))
    {
        let (now, then) = (
            span(&pdf, &out.bytes, id),
            span(&plain_pdf, &plain.bytes, id),
        );
        assert_eq!(now, then, "{id:?}");
    }

    let dir = scratch();
    let (path, mut d) = opened(&dir, "a.pdf", &bytes);
    press_all(&mut d);
    let (base, section) = appended_in_place(&mut d).unwrap();
    let wrote = written(base.len(), &section, &path);
    assert!(wrote.contains(&merged) && wrote.contains(&kid) && wrote.contains(&bare));
    let now = std::fs::read(&path).unwrap();
    let pdf = lazy(&now, None);
    for (w, on) in [(merged, "Yes"), (kid, "Yes"), (bare, "On")] {
        assert_eq!(shows(&pdf, w), Some(n(on)), "{w:?}");
        assert_eq!(ap_of(&pdf, w), ap_of(&was, w), "{w:?}: the file's");
    }
    let made: BTreeSet<ObjectId> = (wrote.iter().copied())
        .filter(|&id| pdf.get(id).is_ok_and(|o| o.as_stream().is_some()))
        .collect();
    assert_eq!(made, BTreeSet::new(), "no form written");
}

/// A check box group whose widgets have distinct on states acts as radio
/// buttons: one pressed, its `/AS` its on state's name and every other's
/// `/Off` in the saved file, by a Rewrite and by an Append; each keeps its
/// file's appearances; reopened, the model reads the same states.
#[test]
fn a_radio_like_check_box_group_s_states_are_saved() {
    let mut f = Form::new(1);
    let buttons = [
        Button::at(0, Some("a"), Some("a")),
        Button::at(1, Some("b"), Some("Off")),
        Button::at(2, Some("c"), Some("Off")),
    ];
    let (_, kids) = f.buttons("group", 0, &buttons);
    let bytes = table(&mut f);
    let want = [n("Off"), n("b"), n("Off")];
    let saved = |pdf: &LazyPdf| {
        kids.iter()
            .map(|&w| shows(pdf, w).unwrap())
            .collect::<Vec<_>>()
    };

    let mut d = open(&bytes, None);
    let second = widgets_of(&d, "group")[1];
    fx::press(&mut d, second);
    let out = rewrite(&d, Profile::default());
    assert_eq!(saved(&lazy(&out.bytes, None)), want);

    let dir = scratch();
    let (path, mut d) = opened(&dir, "a.pdf", &bytes);
    let second = widgets_of(&d, "group")[1];
    fx::press(&mut d, second);
    appended_in_place(&mut d).unwrap();
    let now = std::fs::read(&path).unwrap();
    let pdf = lazy(&now, None);
    assert_eq!(saved(&pdf), want);
    let was = lazy(&bytes, None);
    for &w in &kids {
        assert_eq!(ap_of(&pdf, w), ap_of(&was, w), "{w:?}: the file's");
    }
    let back = crate::open::file(&path, None).unwrap().doc;
    let states: Vec<Option<String>> = (widgets_of(&back, "group").into_iter())
        .map(|aid| {
            back.state()
                .annot(aid)
                .unwrap()
                .widget()
                .unwrap()
                .state
                .clone()
        })
        .collect();
    let off = Some("Off".to_string());
    assert_eq!(states, [off.clone(), Some("b".into()), off]);
}

/// Each kind of widget gets its forms: a check box merged with its field,
/// its forms on the field's object; one split from its field, as a page
/// the tree lists twice shows it twice, both its widgets pressed together
/// (one index), each written apart with forms of its own; a duplicate's
/// copy pressed alone, the original's left as its file has it; an inserted
/// file's.
#[test]
fn each_kind_of_widget_gets_its_forms() {
    // Merged, and a duplicate's copy of a kid.
    let mut f = Form::new(1);
    let merged = f.check_box("merged", 0, row(0), false);
    f.dict(merged).remove(b"AP");
    let (_, kid) = framed_box(&mut f, "kid", 1, "Off");
    let bytes = table(&mut f);
    let mut d = open(&bytes, None);
    let ma = widget_of(&d, "merged");
    fx::press(&mut d, ma);
    let p0 = d.page(0).unwrap();
    d.commit_ops(vec![fx::duplicate(&[p0])], "Duplicate")
        .unwrap();
    let copies: Vec<AnnotId> = (d.state().annots())
        .filter_map(|(aid, rec)| {
            let w = rec.widget()?;
            let r = w.src.as_ref()?;
            (r.id == kid && w.inst.copy == 1).then_some(aid)
        })
        .collect();
    let [copy] = copies[..] else {
        panic!("the duplicate's copy: {copies:?}")
    };
    fx::press(&mut d, copy);
    let out = rewrite(&d, Profile::default());
    let pdf = lazy(&out.bytes, None);
    let at = |key| out.record.map.keys[&key];
    assert_eq!(at(ObjKey::Annot(ma)), merged, "one object with its field");
    let parts = parts_in(&out.record.map, ma);
    assert!(parts[1].is_none(), "no /MK, no normal Off");
    assert_eq!(states(&pdf, merged, b"N"), [(n("Yes"), parts[0].unwrap())]);
    assert_eq!(shows(&pdf, merged), Some(n("Yes")));
    let copied = at(ObjKey::Annot(copy));
    assert_ne!(copied, kid);
    let parts = parts_in(&out.record.map, copy);
    assert_eq!(states(&pdf, copied, b"N")[0], (n("Yes"), parts[0].unwrap()));
    assert_eq!(shows(&pdf, copied), Some(n("Yes")));
    assert!(
        ap_dict(&pdf, kid).is_none(),
        "the original's as its file has it"
    );
    assert_eq!(shows(&pdf, kid), Some(n("Off")));

    // Split: the page listed twice.
    let mut f = Form::new(1);
    let s = f.check_box("s", 0, row(0), false);
    f.dict(s).remove(b"AP");
    let pages = (f.dict(f.catalog).get(b"Pages"))
        .unwrap()
        .as_reference()
        .unwrap();
    let p = f.pages[0];
    f.dict(pages)
        .set("Kids", vec![Object::from(p), Object::from(p)]);
    f.dict(pages).set("Count", 2);
    let bytes = table(&mut f);
    let mut d = open(&bytes, None);
    let shows_on = widgets_of(&d, "s");
    assert_eq!(shows_on.len(), 2, "the widget on both pages");
    fx::press(&mut d, shows_on[0]);
    let out = rewrite(&d, Profile::default());
    let pdf = lazy(&out.bytes, None);
    let field = out.record.map.keys[&ObjKey::Field(d.state().field_named("s").unwrap())];
    assert!(
        ap_dict(&pdf, field).is_none(),
        "the field holds no widget's keys"
    );
    let mut forms = BTreeSet::new();
    for &w in &shows_on {
        let at = out.record.map.keys[&ObjKey::Annot(w)];
        assert_ne!(at, field);
        assert_eq!(shows(&pdf, at), Some(n("Yes")), "both pressed");
        let on = parts_in(&out.record.map, w)[0].unwrap();
        assert_eq!(states(&pdf, at, b"N"), [(n("Yes"), on)]);
        forms.insert(on);
    }
    assert_eq!(forms.len(), 2, "a form each");

    // An inserted file's.
    let mut f = Form::new(1);
    let bytes = table(&mut f);
    let mut other = Form::new(1);
    let (_, theirs) = framed_box(&mut other, "o", 0, "Off");
    let other_bytes = table(&mut other);
    let mut d = open(&bytes, None);
    let s = add(&d, &other_bytes, "other.pdf");
    insert(&mut d, &s, 0);
    let oa = widget_of(&d, "o");
    fx::press(&mut d, oa);
    let out = rewrite(&d, Profile::default());
    let pdf = lazy(&out.bytes, None);
    let at = out.record.map.keys[&ObjKey::Annot(oa)];
    let parts = parts_in(&out.record.map, oa);
    assert!(parts.iter().all(Option::is_some), "{parts:?}");
    assert_eq!(
        states(&pdf, at, b"N"),
        [(n("Yes"), parts[0].unwrap()), (n("Off"), parts[1].unwrap())]
    );
    assert_eq!(shows(&pdf, at), Some(n("Yes")));
    let _ = theirs;
}

/// An overlay of the page holds the save's forms for a pressed check box
/// and radio button, part for part (its box, matrix, content and font
/// names), and no others; the page doesn't draw as its base does until the
/// state is put back.
#[test]
fn an_overlay_s_forms_are_the_save_s_part_for_part() {
    let mut f = Form::new(1);
    let (_, cb) = framed_box(&mut f, "cb", 0, "Off");
    let (_, radio) = f.buttons("radio", RADIO, &[Button::at(1, None, Some("Off"))]);
    let (_, drawn) = f.buttons("drawn", 0, &[Button::at(2, Some("Yes"), Some("Off"))]);
    let bytes = table(&mut f);
    let mut d = open(&bytes, None);
    let screen = ViewInputs::screen();
    let p0 = d.page(0).unwrap();
    assert!(untouched(d.state(), p0, &screen));
    let widgets = [
        (widget_of(&d, "cb"), cb),
        (widget_of(&d, "radio"), radio[0]),
    ];
    let da = widget_of(&d, "drawn");
    for (aid, _) in widgets {
        fx::press(&mut d, aid);
    }
    fx::press(&mut d, da);
    assert!(!untouched(d.state(), p0, &screen));
    let out = rewrite(&d, Profile::default());
    let pdf = lazy(&out.bytes, None);
    let overlay = overlay_appearances(d.state(), p0, &screen).unwrap();
    let mut held = 0;
    for (aid, w) in widgets {
        let parts = parts_in(&out.record.map, aid);
        assert_eq!(states(&pdf, w, b"N")[0].1, parts[0].unwrap(), "{w:?}");
        for (part, saved) in PARTS.iter().zip(parts) {
            let over = (overlay.iter()).find(|(a, p, _)| *a == aid && p == part);
            match (saved, over) {
                (Some(saved), Some((_, _, over))) => {
                    assert_eq!(
                        made(over).0,
                        made(&stream(&pdf, saved)).0,
                        "{aid:?} {part:?}"
                    );
                    held += 1;
                }
                (None, None) => {}
                other => panic!("{aid:?} {part:?}: {other:?}"),
            }
        }
    }
    assert_eq!(held, overlay.len(), "no other form");
    assert_eq!(held, 7, "four for the framed box, three for the radio");
    assert!(
        overlay.iter().all(|(a, _, _)| *a != da),
        "the drawn one's are its file's"
    );
    assert_eq!(shows(&pdf, drawn[0]), Some(n("Yes")));
    for (aid, _) in widgets {
        fx::press(&mut d, aid);
    }
    fx::press(&mut d, da);
    assert!(untouched(d.state(), p0, &screen), "put back");
}

/// A box of no size makes no appearance: the widget is written with its
/// `/AS` from the model and no `/AP`, no part in the record, and a warning
/// that says the kind.
#[test]
fn a_box_of_no_size_is_unmade_and_its_as_still_written() {
    let mut f = Form::new(1);
    let (_, kids) = f.buttons("cb", 0, &[Button::at(0, None, Some("Off"))]);
    f.dict(kids[0])
        .set("Rect", vec![72.into(), 400.into(), 72.into(), 400.into()]);
    let bytes = table(&mut f);
    let mut d = open(&bytes, None);
    let aid = widget_of(&d, "cb");
    fx::press(&mut d, aid);
    let (out, said) = warned(|| rewrite(&d, Profile::default()));
    let pdf = lazy(&out.bytes, None);
    assert!(ap_dict(&pdf, kids[0]).is_none());
    assert_eq!(shows(&pdf, kids[0]), Some(n("Yes")));
    assert_eq!(parts_in(&out.record.map, aid), [None; 4]);
    let unmade: Vec<&String> = said.iter().filter(|w| w.contains("appearance")).collect();
    assert_eq!(unmade.len(), 1, "{said:?}");
    assert!(unmade[0].contains("no-box"), "{said:?}");
}

/// However many widgets are made, their forms name one ZapfDingbats object
/// a file, which is all the save adds of fonts: a Rewrite and an Append.
#[test]
fn every_made_form_names_one_zapf_dingbats_a_file() {
    let mut f = Form::new(2);
    let mut kids = Vec::new();
    for i in 0..3 {
        kids.push(framed_box(&mut f, &format!("cb{i}"), i, "Off").1);
    }
    let (_, radio) = f.buttons(
        "radio",
        RADIO,
        &[Button {
            page: 1,
            ..Button::at(0, None, Some("Off"))
        }],
    );
    kids.push(radio[0]);
    let bytes = table(&mut f);
    let press_all = |d: &mut Document| {
        for name in ["cb0", "cb1", "cb2", "radio"] {
            let aid = widget_of(d, name);
            fx::press(d, aid);
        }
    };
    let fonts_in = |pdf: &LazyPdf| -> BTreeSet<ObjectId> {
        let mut fonts = BTreeSet::new();
        for &w in &kids {
            for entry in [b"N", b"D"] {
                for (_, form) in states(pdf, w, entry) {
                    fonts.extend(made(&stream(pdf, form)).1);
                }
            }
        }
        fonts
    };
    let zapfs = |pdf: &LazyPdf| {
        (pdf.ids())
            .filter(|&id| {
                (pdf.get_dictionary(id).ok())
                    .and_then(|d| d.get(b"BaseFont").and_then(Object::as_name))
                    == Some(b"ZapfDingbats".as_slice())
            })
            .count()
    };
    let plain = rewrite(&open(&bytes, None), Profile::default());
    let before = zapfs(&lazy(&plain.bytes, None));

    let mut d = open(&bytes, None);
    press_all(&mut d);
    let out = rewrite(&d, Profile::default());
    let pdf = lazy(&out.bytes, None);
    assert_eq!(fonts_in(&pdf).len(), 1, "one font");
    assert_eq!(zapfs(&pdf), before + 1);

    let dir = scratch();
    let (path, mut d) = opened(&dir, "a.pdf", &bytes);
    press_all(&mut d);
    let (base, section) = appended_in_place(&mut d).unwrap();
    let pdf = lazy(&std::fs::read(&path).unwrap(), None);
    let fonts = fonts_in(&pdf);
    assert_eq!(fonts.len(), 1, "one font");
    let wrote = written(base.len(), &section, &path);
    assert!(fonts.is_subset(&wrote));
}
