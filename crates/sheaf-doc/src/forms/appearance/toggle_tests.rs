//! A check box's or radio button's appearances from the model's plain data
//! (M10 slice 8): which widgets show a state their file's doesn't and which
//! of those need theirs made, each case; what a look holds (the kind, the
//! caption, the name its on state goes under); and what's made for a
//! widget nothing changed, each slot, its mark and its frame, on forms
//! built in code.

use std::sync::Arc;

use super::tests::shown;
use super::*;
use crate::forms::fixture::{Button, Form, RADIO, row};
use crate::materialize::tests::open;
use crate::model::{Document, FieldNode, Terminal};
use crate::pdf::{Object, Stream, dictionary, literal};
use crate::prim::{Key, Val};
use crate::tx::fixture as fx;

/// Widget `n` of the field named `name` in `d`.
fn widget_of(d: &Document, name: &str, n: usize) -> AnnotId {
    let st = d.state();
    let id = (st.field_named(name)).unwrap_or_else(|| panic!("no field {name}"));
    let term = (st.field(id).and_then(|f| f.term.clone())).expect("a terminal");
    term.widgets[n]
}

/// The toggle look of widget `n` of the field named `name`.
fn toggle_of(d: &Document, name: &str, n: usize) -> ToggleLook {
    match look(d.state(), widget_of(d, name, n)) {
        Ok(Look::Toggle(look)) => look,
        other => panic!("{name}: a button's widget: {other:?}"),
    }
}

/// `look` made, its `/DA`'s font never asked for.
fn made(look: &ToggleLook) -> Result<Made, Unmade> {
    make(&Look::Toggle(look.clone()), |_| {
        panic!("a check box's or radio button's /DA font is never read")
    })
}

/// The slots a toggle's forms are made for, in order.
fn slots(made: &Made) -> Vec<Slot> {
    made.forms.iter().map(|(slot, _)| *slot).collect()
}

/// An `/AP /N` of `Off` alone, an empty form: a normal appearance with no
/// on state.
fn off_alone(f: &mut Form) -> Object {
    let off = f.pdf.add_object(Stream::new(
        dictionary! {
            "Type" => Object::name("XObject"), "Subtype" => Object::name("Form"),
            "BBox" => vec![0.into(), 0.into(), 200.into(), 20.into()],
        },
        Vec::new(),
    ));
    Object::from(dictionary! { "N" => dictionary! { "Off" => off } })
}

/// Which widgets show a state their file's doesn't ([`state_changed`]) and
/// which of those need appearances made ([`stale`]), each case, after each
/// press: none as read; a widget with no `/AP` pressed on, both; one whose
/// file shows `/Yes` with no `/AP` pressed off, both (`Off`); one with an
/// on state, its state alone, and so one with no `/AS`; one whose `/N` has
/// `Off` alone, both; a radio group's pressed widget alone, the other
/// unchanged; a text field filled, stale but no state; a widget pressed
/// back to its file's state, neither.
#[test]
fn a_button_is_stale_where_its_state_moved_and_its_file_draws_no_on_state() {
    let mut f = Form::new(1);
    let city = f.text("city", 0, row(0));
    f.fill(city, "Trenton");
    f.buttons("bare", 0, &[Button::at(1, None, Some("Off"))]);
    f.buttons("on", 0, &[Button::at(2, None, Some("Yes"))]);
    f.buttons("drawn", 0, &[Button::at(3, Some("Yes"), Some("Off"))]);
    f.buttons("unshown", 0, &[Button::at(4, Some("Yes"), None)]);
    let (_, kids) = f.buttons("offonly", 0, &[Button::at(5, None, Some("Off"))]);
    let ap = off_alone(&mut f);
    f.dict(kids[0]).set("AP", ap);
    let radio = [
        Button::at(6, None, Some("Off")),
        Button::at(7, None, Some("Off")),
    ];
    f.buttons("radio", RADIO, &radio);
    let mut d = open(&f.bytes(), None);
    let cases = [
        ("city", 0),
        ("bare", 0),
        ("on", 0),
        ("drawn", 0),
        ("unshown", 0),
        ("offonly", 0),
        ("radio", 0),
        ("radio", 1),
    ];
    let aids: Vec<AnnotId> = (cases.iter())
        .map(|&(name, n)| widget_of(&d, name, n))
        .collect();
    let now = |d: &Document| -> Vec<(bool, Option<String>)> {
        let st = d.state();
        (aids.iter())
            .map(|&aid| (stale(st, aid), state_changed(st, aid).map(str::to_string)))
            .collect()
    };
    let none = || vec![(false, None); cases.len()];
    assert_eq!(now(&d), none(), "as read");

    let both = |s: &str| (true, Some(s.to_string()));
    let alone = |s: &str| (false, Some(s.to_string()));
    let mut want = none();
    let presses = [
        (1, both("Yes")),
        (2, both("Off")),
        (3, alone("Yes")),
        (4, alone("Yes")),
        (5, both("Yes")),
        (6, both("Yes")),
    ];
    for (i, after) in presses {
        fx::press(&mut d, aids[i]);
        want[i] = after;
        assert_eq!(now(&d), want, "{:?} pressed", cases[i]);
    }
    fx::fill(&mut d, "city", "Camden");
    want[0] = (true, None);
    assert_eq!(now(&d), want, "a text field: no state");
    fx::press(&mut d, aids[1]);
    want[1] = (false, None);
    assert_eq!(now(&d), want, "back to its file's");
    fx::press(&mut d, aids[2]);
    want[2] = (false, None);
    assert_eq!(now(&d), want, "back to its file's /Yes");
}

/// What a look holds: whether it's a radio button's, its `/MK /CA`, the
/// name its on state goes under (its file's own, else its index under
/// `/Opt`, else `Yes`, as a press names it), and its frame, the field's
/// `/DA` among it; `Debug` says neither the caption nor the name. A widget
/// its field's group doesn't hold is `not-button`.
#[test]
fn a_toggle_s_look_is_its_kind_caption_and_on_state() {
    let mut f = Form::new(1);
    let (_, kids) = f.buttons("own", 0, &[Button::at(0, Some("Choice1"), Some("Off"))]);
    f.dict(kids[0])
        .set("MK", dictionary! { "CA" => literal("8") });
    f.buttons("bare", 0, &[Button::at(1, None, Some("Off"))]);
    let opt = [
        Button::at(2, None, Some("Off")),
        Button::at(3, None, Some("Off")),
    ];
    let (field, _) = f.buttons("opt", 0, &opt);
    f.dict(field)
        .set("Opt", vec![literal("first"), literal("second")]);
    f.buttons("radio", RADIO, &[Button::at(4, None, Some("Off"))]);
    let mut d = open(&f.bytes(), None);

    let own = toggle_of(&d, "own", 0);
    assert!(!own.radio);
    assert_eq!(own.caption.as_deref(), Some("8"));
    assert_eq!(own.on_state, b"Choice1");
    assert_eq!(own.frame.da, b"/ZaDb 0 Tf 0 g");
    assert_eq!((own.frame.width, own.frame.height), (200.0, 20.0));
    let debug = format!("{own:?}");
    assert!(
        debug.contains("caption_len: Some(1)") && !debug.contains("Choice1"),
        "{debug}"
    );
    assert_eq!(toggle_of(&d, "bare", 0).on_state, b"Yes");
    assert_eq!(toggle_of(&d, "bare", 0).caption, None);
    assert_eq!(toggle_of(&d, "opt", 0).on_state, b"0");
    assert_eq!(toggle_of(&d, "opt", 1).on_state, b"1");
    let radio = toggle_of(&d, "radio", 0);
    assert!(radio.radio);
    assert_eq!(radio.on_state, b"Yes");
    let aid = widget_of(&d, "opt", 1);
    let st = d.state();
    let group = crate::forms::buttons(st, st.field_named("opt").unwrap()).unwrap();
    assert_eq!(group.on_state(aid).as_deref(), Some(b"1".as_slice()));
    assert_eq!(group.press(aid).unwrap().states, [(aid, "1".into())]);

    // The name is read from the widget's and its field's records alone:
    // its field's terminal holding none of its widgets, it's the same.
    let id = d.state().field_named("opt").unwrap();
    let mut node = FieldNode::clone(d.state().field(id).unwrap());
    let mut term = Terminal::clone(node.term.as_deref().unwrap());
    term.widgets.clear();
    node.term = Some(Arc::new(term));
    d.put_for_test(
        vec![(Key::Field(id), Some(Val::Field(Arc::new(node))))],
        "orphan",
    );
    let st = d.state();
    assert_eq!(crate::forms::buttons(st, id).unwrap().on_state(aid), None);
    match look(st, aid) {
        Ok(Look::Toggle(look)) => assert_eq!(look.on_state, b"1"),
        other => panic!("a button's widget: {other:?}"),
    }
}

/// What's made for a widget nothing changed (as M10 slice 9's
/// `NeedAppearances` transform and slice 23's new widgets will ask), its
/// `/DA` font never read: with no `/MK`, the on state's normal and down
/// forms and `Off`'s down one (the pressed gray), no normal `Off`, which
/// would paint nothing; with a border color, all four, `Off`'s the on
/// state's frame without the mark. The mark is ZapfDingbats' glyph of the
/// caption's code, the default a check box's `4` and a radio button's `l`;
/// each on form names ZapfDingbats alone, an `Off` one nothing; no glyph
/// of the fallback. A radio button's default frame is round; one beveled
/// with no width and no background makes its two on forms alone. A box of
/// no size is `no-box`.
#[test]
fn a_toggle_nothing_changed_is_made_whole() {
    let mut f = Form::new(1);
    f.buttons("plain", 0, &[Button::at(0, None, Some("Off"))]);
    let (_, kids) = f.buttons("framed", 0, &[Button::at(1, None, Some("Off"))]);
    f.dict(kids[0])
        .set("MK", dictionary! { "BC" => vec![Object::from(0)] });
    let mut captions = Vec::new();
    for (i, caption) in ["4", "8", "6", "l"].into_iter().enumerate() {
        let name = format!("ca{i}");
        let (_, kids) = f.buttons(&name, 0, &[Button::at(2 + i, None, Some("Off"))]);
        f.dict(kids[0])
            .set("MK", dictionary! { "CA" => literal(caption) });
        captions.push((name, caption));
    }
    f.buttons("radio", RADIO, &[Button::at(6, None, Some("Off"))]);
    let (_, kids) = f.buttons("flat", RADIO, &[Button::at(7, None, Some("Off"))]);
    f.dict(kids[0])
        .set("BS", dictionary! { "W" => 0, "S" => Object::name("B") });
    let (_, kids) = f.buttons("flat_box", 0, &[Button::at(8, None, Some("Off"))]);
    f.dict(kids[0])
        .set("Rect", vec![72.into(), 400.into(), 72.into(), 400.into()]);
    let d = open(&f.bytes(), None);
    let zapf = toggle::zapf_dingbats();

    let plain = made(&toggle_of(&d, "plain", 0)).unwrap();
    assert_eq!(slots(&plain), [Slot::Normal, Slot::Down, Slot::DownOff]);
    assert!(plain.glyphs.is_empty());
    let on = plain.form(Slot::Normal).unwrap();
    assert_eq!(shown(&on.content), b"4", "a check box's default");
    let fonts: Vec<ResId> = on.needs.0.iter().map(|n| n.res.id()).collect();
    assert_eq!(fonts, [zapf.id()], "ZapfDingbats alone");
    let down_off = plain.form(Slot::DownOff).unwrap();
    assert!(down_off.needs.0.is_empty());
    assert!(
        down_off.content.starts_with(b"q\n0.75 0.75 0.75 rg\n"),
        "{:?}",
        String::from_utf8_lossy(&down_off.content)
    );
    let down = plain.form(Slot::Down).unwrap();
    assert!(down.content.starts_with(&down_off.content));
    assert_eq!(shown(&down.content), b"4");

    let framed = made(&toggle_of(&d, "framed", 0)).unwrap();
    let all = [Slot::Normal, Slot::NormalOff, Slot::Down, Slot::DownOff];
    assert_eq!(slots(&framed), all);
    let (on, off) = (
        framed.form(Slot::Normal).unwrap(),
        framed.form(Slot::NormalOff).unwrap(),
    );
    assert!(!off.content.is_empty() && on.content.starts_with(&off.content));
    assert!(off.needs.0.is_empty());

    for (name, caption) in &captions {
        let made = made(&toggle_of(&d, name, 0)).unwrap();
        let on = made.form(Slot::Normal).unwrap();
        assert_eq!(shown(&on.content), caption.as_bytes(), "{name}");
    }

    let curves = |content: &[u8]| content.windows(3).any(|w| w == b" c\n");
    let radio = made(&toggle_of(&d, "radio", 0)).unwrap();
    assert_eq!(slots(&radio), [Slot::Normal, Slot::Down, Slot::DownOff]);
    assert_eq!(shown(&radio.form(Slot::Normal).unwrap().content), b"l");
    assert!(curves(&radio.form(Slot::DownOff).unwrap().content), "round");
    assert!(!curves(&down_off.content), "a check box's is square");
    let flat = made(&toggle_of(&d, "flat", 0)).unwrap();
    assert_eq!(slots(&flat), [Slot::Normal, Slot::Down]);

    let flat_box = toggle_of(&d, "flat_box", 0);
    assert_eq!(made(&flat_box).unwrap_err(), Unmade::NoBox);
    // Pure: the same look, the same forms.
    assert_eq!(made(&toggle_of(&d, "framed", 0)).unwrap(), framed);
}
