//! A push button's appearances made anew where the model holds its captions
//! changed (M10 slice 8), through the emitter: a Rewrite and an Append each
//! write them where a text field's is, `/N` alone under `Part::Ap` with no
//! `/H`, and with `/H /P` `/N`, `/R` and `/D` under `Ap`, `ApRollover` and
//! `ApDown`, the down one its `/AC`, and nothing else anew; a button nothing
//! changed keeps its appearance, object for object, and one with none stays
//! so though one can be made for it; captions in two fonts, three forms and
//! one subset; an icon unmade with no `/AP`; no `/DA` at all at 12 points;
//! and an overlay and a save holding the same forms.

use std::collections::BTreeSet;
use std::sync::Arc;

use crate::pdf::{
    Dictionary, LazyPdf, Object, ObjectId, Stream, StreamExt, XrefKind, dictionary, literal, text,
};

use super::append_tests::{appended_in_place, written};
use super::key::{ObjKey, Part};
use super::removal_tests::{opened, scratch, warned};
use super::tests::{lazy, open, rewrite, span};
use super::*;
use crate::annot::AnnotId;
use crate::content::Fallback;
use crate::content::fixture::{fallback_subsets, subsets_hold};
use crate::forms::appearance::tests::shows;
use crate::forms::appearance::{self, DaFont, Look, Slot};
use crate::forms::fixture::{Form, row};
use crate::model::Document;
use crate::pdf::fonts::Std14;
use crate::pdf::forms::FieldFont;
use crate::realize::{ViewInputs, overlay_appearances, untouched};
use crate::tx::fixture as fx;

/// `f`'s file, its cross-reference a table, so a Rewrite writes each object
/// on its own and an object nothing changed is its bytes as they were.
fn table(f: &mut Form) -> Vec<u8> {
    f.pdf.xref_kind = XrefKind::Table;
    f.bytes()
}

/// A push button `name` at row `i` on page 0, its own widget, that says
/// `caption` (`/MK /CA`), its appearance one that draws it.
fn button(f: &mut Form, name: &str, i: usize, caption: &str) -> ObjectId {
    f.push_button(name, 0, row(i), caption)
}

/// [`button`] with `/H /P` and `/MK << /CA caption /RC rollover /AC down
/// >>`, each a text string.
fn pushed(f: &mut Form, name: &str, i: usize, captions: [&str; 3]) -> ObjectId {
    let id = button(f, name, i, captions[0]);
    let [ca, rc, ac] = captions.map(text::string);
    f.dict(id)
        .set("MK", dictionary! { "CA" => ca, "RC" => rc, "AC" => ac });
    f.dict(id).set("H", Object::name("P"));
    id
}

/// The one widget of the field named `name` in `d`.
fn widget_of(d: &Document, name: &str) -> AnnotId {
    let st = d.state();
    let id = (st.field_named(name)).unwrap_or_else(|| panic!("no field {name}"));
    let term = (st.field(id).and_then(|f| f.term.clone())).expect("a terminal");
    let [aid] = term.widgets.iter().copied().collect::<Vec<_>>()[..] else {
        panic!("{name}: one widget")
    };
    aid
}

/// The widget of the field named `name` in `d` given the captions `normal`,
/// `rollover` and `down`.
fn caption(d: &mut Document, name: &str, [normal, rollover, down]: [Option<&str>; 3]) {
    let aid = widget_of(d, name);
    fx::caption(d, aid, normal, rollover, down);
}

/// A widget's `/AP` in `pdf`, references followed.
fn ap_dict(pdf: &LazyPdf, widget: ObjectId) -> Option<Dictionary> {
    let ap = pdf.get_dictionary(widget).ok()?.get(b"AP")?;
    Some(pdf.dereference(ap).ok()?.1.as_dict()?.clone())
}

/// The entries of a widget's `/AP` in `pdf`, by name, and the number each
/// names.
fn ap_entries(pdf: &LazyPdf, widget: ObjectId) -> Vec<(Vec<u8>, ObjectId)> {
    let ap = ap_dict(pdf, widget).expect("an /AP");
    (ap.iter())
        .map(|(k, v)| (k.to_vec(), v.as_reference().expect("a form by reference")))
        .collect()
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

/// The form a save holds for the widget `widget` of `pdf` under its `/AP`'s
/// `entry`.
fn saved_form(pdf: &LazyPdf, widget: ObjectId, entry: &[u8]) -> (Made, Vec<ObjectId>) {
    let at = ap_dict(pdf, widget)
        .and_then(|ap| ap.get(entry)?.as_reference())
        .unwrap_or_else(|| panic!("{widget:?}: no /AP /{}", String::from_utf8_lossy(entry)));
    made(&stream(pdf, at))
}

/// What the form under `entry` of `widget`'s `/AP` in `pdf` shows: each
/// `Tj`'s bytes.
fn saved_shows(pdf: &LazyPdf, widget: ObjectId, entry: &[u8]) -> Vec<Vec<u8>> {
    shows(&saved_form(pdf, widget, entry).0.content)
}

/// `/MK /CA` of `widget` in `pdf`, its bytes.
fn mk_caption(pdf: &LazyPdf, widget: ObjectId) -> Option<Vec<u8>> {
    let mk = pdf
        .get_dictionary(widget)
        .ok()?
        .get(b"MK")?
        .as_dict()?
        .clone();
    mk.get(b"CA")?.as_bytes().map(<[u8]>::to_vec)
}

/// Every object `pdf` holds.
fn ids(pdf: &LazyPdf) -> BTreeSet<ObjectId> {
    pdf.ids().collect()
}

/// `texts`, each as the bytes a `Tj` shows.
fn bytes_of(texts: &[&str]) -> Vec<Vec<u8>> {
    texts.iter().map(|t| t.as_bytes().to_vec()).collect()
}

/// The numbers `map` gives widget `aid`'s three push button parts, `/N`'s,
/// `/R`'s and `/D`'s, where it holds them.
fn parts_in(map: &record::ObjMap, aid: AnnotId) -> [Option<ObjectId>; 3] {
    [Part::Ap, Part::ApRollover, Part::ApDown]
        .map(|part| map.keys.get(&ObjKey::AnnotPart(aid, part)).copied())
}

/// The numbers in the fallback font's whole program of the glyphs that
/// draw `chars`, a character each.
fn glyphs_of(chars: &str) -> BTreeSet<u16> {
    let program = Fallback::SANS.program();
    (chars.chars())
        .map(|c| program.glyph(c).expect("a character the fallback draws"))
        .collect()
}

/// A Rewrite of a push button whose caption changed writes its appearance
/// where a text field's is: with no `/H`, `/N` alone, under `AnnotPart(aid,
/// Ap)`, naming `/DR`'s Helvetica, the new caption read back; with `/H /P`,
/// `/N`, `/R` and `/D` in that order under `Ap`, `ApRollover` and `ApDown`,
/// the normal, rollover and down (`/AC`) captions. Its `/MK` is the file's
/// (a caption is written by no op yet). Nothing else is written anew: the
/// objects are the unchanged Rewrite's less the old appearances' streams
/// and with the made forms, every other one its bytes as they were. An
/// Append writes each widget and its forms alone, and another after it
/// none of them.
#[test]
fn a_changed_caption_gets_n_alone_or_with_h_p_n_r_and_d() {
    let mut f = Form::new(1);
    let helv = f.helv;
    let plain = button(&mut f, "plain", 0, "Go");
    let push = pushed(&mut f, "push", 1, ["Go", "Over", "Down"]);
    let bytes = table(&mut f);
    let was = lazy(&bytes, None);
    let old: Vec<ObjectId> = [plain, push]
        .iter()
        .map(|&w| ap_entries(&was, w)[0].1)
        .collect();
    let plain_out = rewrite(&open(&bytes, None), Profile::default());
    let plain_pdf = lazy(&plain_out.bytes, None);
    let changes = |d: &mut Document| {
        caption(d, "plain", [Some("Stop"), None, None]);
        caption(d, "push", [Some("Start"), Some("Hover"), Some("Pressed")]);
    };

    let mut d = open(&bytes, None);
    changes(&mut d);
    let out = rewrite(&d, Profile::default());
    let pdf = lazy(&out.bytes, None);
    let (aplain, apush) = (widget_of(&d, "plain"), widget_of(&d, "push"));
    let n = out.record.map.keys[&ObjKey::AnnotPart(aplain, Part::Ap)];
    assert_eq!(ap_entries(&pdf, plain), [(b"N".to_vec(), n)], "/N alone");
    assert_eq!(parts_in(&out.record.map, aplain), [Some(n), None, None]);
    assert_eq!(saved_shows(&pdf, plain, b"N"), bytes_of(&["Stop"]));
    assert_eq!(saved_form(&pdf, plain, b"N").1, [helv], "/DR's Helvetica");
    let [Some(pn), Some(pr), Some(pd)] = parts_in(&out.record.map, apush) else {
        panic!("three parts")
    };
    let entries = [
        (b"N".to_vec(), pn),
        (b"R".to_vec(), pr),
        (b"D".to_vec(), pd),
    ];
    assert_eq!(ap_entries(&pdf, push), entries);
    for (entry, shown) in [(b"N", "Start"), (b"R", "Hover"), (b"D", "Pressed")] {
        assert_eq!(saved_shows(&pdf, push, entry), bytes_of(&[shown]));
        assert_eq!(saved_form(&pdf, push, entry).1, [helv]);
    }
    let down = saved_form(&pdf, push, b"D").0.content;
    assert!(
        down.starts_with(b"q\n0.75 0.75 0.75 rg\n"),
        "the pressed frame"
    );
    assert_eq!(mk_caption(&pdf, plain).as_deref(), Some(b"Go".as_slice()));
    assert_eq!(mk_caption(&pdf, push).as_deref(), Some(b"Go".as_slice()));

    let mut want = ids(&plain_pdf);
    for id in &old {
        assert!(want.remove(id), "{id:?}");
    }
    let new = [n, pn, pr, pd];
    want.extend(new);
    assert_eq!(ids(&pdf), want);
    for &id in want
        .iter()
        .filter(|&&id| ![plain, push].contains(&id) && !new.contains(&id))
    {
        let (now, then) = (
            span(&pdf, &out.bytes, id),
            span(&plain_pdf, &plain_out.bytes, id),
        );
        assert_eq!(now, then, "{id:?}");
    }

    let dir = scratch();
    let (path, mut d) = opened(&dir, "p.pdf", &bytes);
    changes(&mut d);
    let (base, section) = appended_in_place(&mut d).unwrap();
    let record = d.saved_record().unwrap();
    let mut want = BTreeSet::from([plain, push]);
    for aid in [aplain, apush] {
        want.extend(parts_in(&record.map, aid).into_iter().flatten());
    }
    assert_eq!(want.len(), 6, "two widgets and four forms");
    assert_eq!(written(base.len(), &section, &path), want);
    let now = std::fs::read(&path).unwrap();
    let pdf = lazy(&now, None);
    assert_eq!(saved_shows(&pdf, plain, b"N"), bytes_of(&["Stop"]));
    assert_eq!(ap_entries(&pdf, plain).len(), 1);
    for (entry, shown) in [(b"N", "Start"), (b"R", "Hover"), (b"D", "Pressed")] {
        assert_eq!(saved_shows(&pdf, push, entry), bytes_of(&[shown]));
    }
    let (base, section) = appended_in_place(&mut d).unwrap();
    if !section.is_empty() {
        let wrote = written(base.len(), &section, &path);
        assert!(wrote.is_disjoint(&want), "{wrote:?}");
    }
}

/// A push button nothing changed keeps its appearance, object for object,
/// beside one whose caption changed: its widget and its `/AP /N` the
/// unchanged Rewrite's, number and bytes, and no part of its own. One whose
/// file has no appearance stays without, by a Rewrite and an Append (which
/// writes nothing of it), and a caption put back to its file's makes
/// nothing; yet [`appearance::make`] makes its `/N` from its look, as M10
/// slices 9 and 23 will ask for one.
#[test]
fn a_push_button_nothing_changed_keeps_its_appearance() {
    let mut f = Form::new(1);
    button(&mut f, "changed", 0, "Go");
    let kept = button(&mut f, "kept", 1, "Stay");
    let none = button(&mut f, "none", 2, "Bare");
    f.dict(none).remove(b"AP");
    let bytes = table(&mut f);
    let plain = rewrite(&open(&bytes, None), Profile::default());
    let plain_pdf = lazy(&plain.bytes, None);

    let mut d = open(&bytes, None);
    caption(&mut d, "changed", [Some("Went"), None, None]);
    let out = rewrite(&d, Profile::default());
    let pdf = lazy(&out.bytes, None);
    assert_eq!(ap_entries(&pdf, kept), ap_entries(&plain_pdf, kept));
    let n = ap_entries(&pdf, kept)[0].1;
    assert_eq!(span(&pdf, &out.bytes, n), span(&plain_pdf, &plain.bytes, n));
    assert_eq!(
        span(&pdf, &out.bytes, kept),
        span(&plain_pdf, &plain.bytes, kept)
    );
    for aid in [widget_of(&d, "kept"), widget_of(&d, "none")] {
        assert_eq!(parts_in(&out.record.map, aid), [None; 3], "{aid:?}");
        assert!(!appearance::stale(d.state(), aid));
    }
    assert!(ap_dict(&pdf, none).is_none(), "none still");
    assert_eq!(
        span(&pdf, &out.bytes, none),
        span(&plain_pdf, &plain.bytes, none)
    );

    let dir = scratch();
    let (path, mut d) = opened(&dir, "k.pdf", &bytes);
    caption(&mut d, "changed", [Some("Went"), None, None]);
    let (base, section) = appended_in_place(&mut d).unwrap();
    let wrote = written(base.len(), &section, &path);
    assert!(
        !wrote.contains(&kept) && !wrote.contains(&none),
        "{wrote:?}"
    );
    let pdf = lazy(&std::fs::read(&path).unwrap(), None);
    assert!(ap_dict(&pdf, none).is_none());
    caption(&mut d, "changed", [Some("Go"), None, None]);
    let st = d.state();
    assert!(!appearance::stale(st, widget_of(&d, "changed")), "put back");

    let Ok(Look::Push(look)) = appearance::look(st, widget_of(&d, "none")) else {
        panic!("a push button's look")
    };
    let font = FieldFont::standard(Std14::Helvetica);
    let res = font.res().expect("a standard font's dictionary");
    let font = Arc::new(font);
    let made = appearance::make(&Look::Push(look), |_| Ok(DaFont { font, res })).expect("made");
    assert_eq!(made.forms.len(), 1);
    let form = made.form(Slot::Normal).expect("/N");
    assert_eq!(shows(&form.content), bytes_of(&["Bare"]));
}

/// Captions in two fonts, by a Rewrite and an Append: a normal caption
/// `Łukasz` and a down one `Ńowak` draw their first letters in the
/// fallback, a rollover one `Über` in Helvetica alone; three forms, the
/// normal and down ones naming `/DR`'s Helvetica and the one subset, of
/// `Ł` and `Ń`, the rollover one Helvetica alone; each code shown read back
/// as a glyph of the cut program; the record's one subset its key.
#[test]
fn captions_in_two_fonts_make_three_forms_and_one_subset() {
    let mut f = Form::new(1);
    let helv = f.helv;
    let push = pushed(&mut f, "push", 0, ["Go", "Over", "Down"]);
    let bytes = table(&mut f);
    let captions = [Some("\u{141}ukasz"), Some("\u{dc}ber"), Some("\u{143}owak")];

    let mut d = open(&bytes, None);
    caption(&mut d, "push", captions);
    let out = rewrite(&d, Profile::default());
    let pdf = lazy(&out.bytes, None);
    let subsets = fallback_subsets(&out.bytes);
    let [(font, codes)] = &subsets[..] else {
        panic!("one subset: {subsets:?}")
    };
    assert_eq!(*codes, glyphs_of("\u{141}\u{143}"));
    assert!(subsets_hold(&out.bytes) > 0);
    let bound = |entry: &[u8]| -> BTreeSet<ObjectId> {
        saved_form(&pdf, push, entry).1.into_iter().collect()
    };
    assert_eq!(bound(b"N"), BTreeSet::from([helv, *font]));
    assert_eq!(bound(b"R"), BTreeSet::from([helv]), "Helvetica alone");
    assert_eq!(bound(b"D"), BTreeSet::from([helv, *font]));
    let [(id, glyphs)] = &out.record.fallbacks[..] else {
        panic!("the record's subset")
    };
    assert_eq!(out.record.map.keys[&ObjKey::Res(*id)], *font);
    let held: BTreeSet<u16> = glyphs.iter().map(|(g, _)| g).collect();
    assert_eq!(held, *codes);

    let dir = scratch();
    let (path, mut d) = opened(&dir, "t.pdf", &bytes);
    caption(&mut d, "push", captions);
    let aid = widget_of(&d, "push");
    let (base, section) = appended_in_place(&mut d).unwrap();
    let record = d.saved_record().unwrap();
    let [(s, _)] = &record.fallbacks[..] else {
        panic!("one subset")
    };
    let s_at = record.map.keys[&ObjKey::Res(*s)];
    let wrote = written(base.len(), &section, &path);
    let parts: Vec<ObjectId> = parts_in(&record.map, aid).into_iter().flatten().collect();
    assert_eq!(parts.len(), 3);
    assert!(wrote.contains(&push) && wrote.contains(&s_at), "{wrote:?}");
    assert!(parts.iter().all(|p| wrote.contains(p)), "{wrote:?}");
    let now = std::fs::read(&path).unwrap();
    let subsets = fallback_subsets(&now);
    assert_eq!(subsets, [(s_at, glyphs_of("\u{141}\u{143}"))]);
    assert!(subsets_hold(&now) > 0);
}

/// A push button that shows an icon beside its caption (`/MK /I` a stream,
/// `/TP 2`) whose caption changed is unmade: its file's appearance isn't
/// copied, it has no `/AP`, and one warning says `icon`, never the
/// caption. One with the same icon and `/TP 0` shows its caption alone and
/// is made.
#[test]
fn an_icon_is_unmade_and_its_widget_has_no_ap() {
    let mut f = Form::new(1);
    let icon = f.pdf.add_object(Stream::new(
        dictionary! {
            "Type" => Object::name("XObject"), "Subtype" => Object::name("Form"),
            "BBox" => vec![0.into(), 0.into(), 10.into(), 10.into()],
        },
        b"0 g 0 0 10 10 re f".to_vec(),
    ));
    let with = button(&mut f, "with", 0, "Go");
    f.dict(with).set(
        "MK",
        dictionary! { "CA" => literal("Go"), "I" => icon, "TP" => 2 },
    );
    let alone = button(&mut f, "alone", 1, "Go");
    f.dict(alone).set(
        "MK",
        dictionary! { "CA" => literal("Go"), "I" => icon, "TP" => 0 },
    );
    let bytes = table(&mut f);
    let mut d = open(&bytes, None);
    caption(&mut d, "with", [Some("Secret"), None, None]);
    caption(&mut d, "alone", [Some("Shown"), None, None]);
    let (out, said) = warned(|| rewrite(&d, Profile::default()));
    let pdf = lazy(&out.bytes, None);
    assert!(ap_dict(&pdf, with).is_none(), "no /AP");
    assert_eq!(parts_in(&out.record.map, widget_of(&d, "with")), [None; 3]);
    assert_eq!(saved_shows(&pdf, alone, b"N"), bytes_of(&["Shown"]));
    let unmade: Vec<&String> = said.iter().filter(|w| w.contains("appearance")).collect();
    assert_eq!(unmade.len(), 1, "{said:?}");
    assert!(unmade[0].contains("(icon)"), "{said:?}");
    assert!(!said.iter().any(|w| w.contains("Secret")), "{said:?}");
}

/// A push button with no `/DA` at all (none of its own, and its form none)
/// whose caption changed draws it at 12 points, in Helvetica as the
/// standard font is written by name, one object.
#[test]
fn no_da_at_all_draws_at_12_points() {
    let mut f = Form::new(1);
    let bare = button(&mut f, "bare", 0, "Go");
    f.dict(bare).remove(b"DA");
    let acroform = f.acroform;
    f.dict(acroform).remove(b"DA");
    let bytes = table(&mut f);
    let mut d = open(&bytes, None);
    caption(&mut d, "bare", [Some("Twelve"), None, None]);
    let out = rewrite(&d, Profile::default());
    let pdf = lazy(&out.bytes, None);
    let (form, fonts) = saved_form(&pdf, bare, b"N");
    let content = String::from_utf8(form.content.clone()).expect("text");
    assert!(content.contains(" 12 Tf\n"), "{content}");
    assert_eq!(shows(&form.content), bytes_of(&["Twelve"]));
    let [font] = fonts[..] else {
        panic!("one font: {fonts:?}")
    };
    let font = pdf.get_dictionary(font).expect("a font");
    assert_eq!(
        font.get(b"BaseFont").and_then(Object::as_name),
        Some(b"Helvetica".as_slice())
    );
}

/// An overlay of the page holds the save's forms for two changed push
/// buttons, part for part (`/N` alone, and `/N`, `/R`, `/D`), each its
/// box, matrix, content and font names, and no other; the page draws as
/// its base does again once each caption is put back.
#[test]
fn an_overlay_s_forms_are_the_save_s_part_for_part() {
    let mut f = Form::new(1);
    let plain = button(&mut f, "plain", 0, "Go");
    let push = pushed(&mut f, "push", 1, ["Go", "Over", "Down"]);
    let bytes = table(&mut f);
    let mut d = open(&bytes, None);
    let screen = ViewInputs::screen();
    let p0 = d.page(0).unwrap();
    assert!(untouched(d.state(), p0, &screen));
    caption(&mut d, "plain", [Some("Stop"), None, None]);
    caption(&mut d, "push", [Some("\u{141}ukasz"), Some("Hover"), None]);
    assert!(!untouched(d.state(), p0, &screen));
    let out = rewrite(&d, Profile::default());
    let pdf = lazy(&out.bytes, None);
    let overlay = overlay_appearances(d.state(), p0, &screen).unwrap();
    let forms = [
        (widget_of(&d, "plain"), plain, Part::Ap, &b"N"[..]),
        (widget_of(&d, "push"), push, Part::Ap, b"N"),
        (widget_of(&d, "push"), push, Part::ApRollover, b"R"),
        (widget_of(&d, "push"), push, Part::ApDown, b"D"),
    ];
    for (aid, w, part, entry) in forms {
        let over = (overlay.iter()).find(|(a, p, _)| *a == aid && *p == part);
        let (_, _, over) = over.unwrap_or_else(|| panic!("{aid:?} {part:?}: the overlay's"));
        assert_eq!(
            made(over).0,
            saved_form(&pdf, w, entry).0,
            "{aid:?} {part:?}"
        );
    }
    assert_eq!(overlay.len(), forms.len(), "no other form");
    caption(&mut d, "plain", [Some("Go"), None, None]);
    caption(&mut d, "push", [Some("Go"), Some("Over"), Some("Down")]);
    assert!(untouched(d.state(), p0, &screen), "put back");
}
