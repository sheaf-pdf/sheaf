//! A push button's appearances from the model's plain data (M10 slice 8):
//! which widgets need theirs made (their captions moved), each case; what a
//! look holds (its captions, none with `/MK /TP 1`; `/H`; whether it has a
//! `/DA` at all) and when it's refused (an icon shown); and what's made for
//! a button nothing changed, each slot, its captions and their size, on
//! forms built in code.

use std::sync::Arc;

use super::tests::shows;
use super::*;
use crate::forms::fixture::{Form, row};
use crate::materialize::tests::open;
use crate::model::Document;
use crate::pdf::{Dictionary, Object, Stream, dictionary, literal};
use crate::tx::fixture as fx;

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

/// The push look of the widget of the field named `name`.
fn push_of(d: &Document, name: &str) -> PushLook {
    match look(d.state(), widget_of(d, name)) {
        Ok(Look::Push(look)) => look,
        other => panic!("{name}: a push button's widget: {other:?}"),
    }
}

/// A push button `name` at row `i` on page 0, its own widget, its `/MK`
/// `mk`.
fn button(f: &mut Form, name: &str, i: usize, mk: Dictionary) -> ObjectId {
    let id = f.push_button(name, 0, row(i), "");
    f.dict(id).set("MK", mk);
    id
}

/// An icon: a form XObject that draws a square.
fn icon(f: &mut Form) -> Object {
    let id = f.pdf.add_object(Stream::new(
        dictionary! {
            "Type" => Object::name("XObject"), "Subtype" => Object::name("Form"),
            "BBox" => vec![0.into(), 0.into(), 10.into(), 10.into()],
        },
        b"0 g 0 0 10 10 re f".to_vec(),
    ));
    Object::from(id)
}

/// Helvetica, and its own resource.
fn helvetica() -> (Arc<FieldFont>, Res) {
    let font = FieldFont::standard(Std14::Helvetica);
    let res = font.res().expect("a standard font's dictionary");
    (Arc::new(font), res)
}

/// `look` made in Helvetica, the name its `/DA` gives held in `asked`.
fn made_asking(look: &PushLook, asked: &mut Option<Option<Vec<u8>>>) -> Result<Made, Unmade> {
    let (font, res) = helvetica();
    make(&Look::Push(look.clone()), |name| {
        *asked = Some(name.map(<[u8]>::to_vec));
        Ok(DaFont { font, res })
    })
}

/// `look` made in Helvetica.
fn made(look: &PushLook) -> Result<Made, Unmade> {
    made_asking(look, &mut None)
}

/// The slots a push button's forms are made for, in order.
fn slots(made: &Made) -> Vec<Slot> {
    made.forms.iter().map(|(slot, _)| *slot).collect()
}

/// What the form made for `slot` draws, as text.
fn content(made: &Made, slot: Slot) -> String {
    let form = made.form(slot).unwrap_or_else(|| panic!("no {slot:?}"));
    String::from_utf8(form.content.clone()).expect("content that's text")
}

/// Which push buttons need their appearances made ([`stale`]), each case,
/// after each put: none as read; the normal caption changed, the rollover
/// one, the down one, each alone; one given a caption where its file has
/// none; one whose caption is taken away; each put back, none. A text field
/// filled is stale and no push button with it; [`state_changed`] is none
/// for every push button.
#[test]
fn a_push_button_is_stale_where_its_captions_moved() {
    let mut f = Form::new(1);
    let city = f.text("city", 0, row(0));
    f.fill(city, "Trenton");
    let all =
        dictionary! { "CA" => literal("Go"), "RC" => literal("Over"), "AC" => literal("Down") };
    for (i, name) in ["n", "r", "a"].into_iter().enumerate() {
        button(&mut f, name, i + 1, all.clone());
    }
    button(&mut f, "bare", 4, Dictionary::new());
    button(&mut f, "gone", 5, dictionary! { "CA" => literal("Gone") });
    let mut d = open(&f.bytes(), None);
    let names = ["city", "n", "r", "a", "bare", "gone"];
    let aids: Vec<AnnotId> = names.iter().map(|n| widget_of(&d, n)).collect();
    let now = |d: &Document| -> Vec<bool> {
        let st = d.state();
        for &aid in &aids[1..] {
            assert_eq!(state_changed(st, aid), None, "never a state");
        }
        aids.iter().map(|&aid| stale(st, aid)).collect()
    };
    assert_eq!(now(&d), [false; 6], "as read");
    let all_but = |n, r, a| [Some(n), Some(r), Some(a)];
    let puts: [(usize, [Option<&str>; 3]); 5] = [
        (1, all_but("Stop", "Over", "Down")),
        (2, all_but("Go", "Above", "Down")),
        (3, all_but("Go", "Over", "Pressed")),
        (4, [Some("New"), None, None]),
        (5, [None, None, None]),
    ];
    let mut want = [false; 6];
    for (i, [normal, rollover, down]) in puts {
        fx::caption(&mut d, aids[i], normal, rollover, down);
        want[i] = true;
        assert_eq!(now(&d), want, "after {}", names[i]);
    }
    fx::fill(&mut d, "city", "Camden");
    want[0] = true;
    assert_eq!(now(&d), want, "a text field filled");
    let back: [(usize, [Option<&str>; 3]); 5] = [
        (1, all_but("Go", "Over", "Down")),
        (2, all_but("Go", "Over", "Down")),
        (3, all_but("Go", "Over", "Down")),
        (4, [None, None, None]),
        (5, [Some("Gone"), None, None]),
    ];
    for (i, [normal, rollover, down]) in back {
        fx::caption(&mut d, aids[i], normal, rollover, down);
        want[i] = false;
        assert_eq!(now(&d), want, "{} put back", names[i]);
    }
}

/// A push button's look, by what its widget and its field read: its three
/// captions; none of them with `/MK /TP 1`; `/H /P`, `/T` and the string
/// `(P)` call for the rollover and down appearances, `/I` and none don't;
/// it has a `/DA` where it, its field or its form has one, an empty one
/// too, and none where none does; an icon shown beside the caption (`/TP`
/// 2) is refused as `icon`, one with `/TP 0` or none isn't (its caption is
/// shown alone), nor is `/I` that isn't a stream. `Debug` prints no
/// caption.
#[test]
fn the_look_is_the_button_s_captions_highlight_and_da() {
    let mut f = Form::new(1);
    let all =
        dictionary! { "CA" => literal("Go"), "RC" => literal("Over"), "AC" => literal("Down") };
    let p = button(&mut f, "p", 0, all.clone());
    f.dict(p).set("H", Object::name("P"));
    let t = button(&mut f, "t", 1, all.clone());
    f.dict(t).set("H", Object::name("T"));
    let s = button(&mut f, "s", 2, all.clone());
    f.dict(s).set("H", literal("P"));
    let i = button(&mut f, "i", 3, all.clone());
    f.dict(i).set("H", Object::name("I"));
    let mut icon_only = all.clone();
    icon_only.set("TP", 1);
    button(&mut f, "tp1", 4, icon_only);
    let own = button(&mut f, "own", 5, all.clone());
    f.dict(own).remove(b"DA");
    let empty = button(&mut f, "empty", 6, all.clone());
    f.dict(empty).set("DA", literal(""));
    for (k, (name, tp)) in [("tp0", Some(0)), ("none", None), ("tp2", Some(2))]
        .into_iter()
        .enumerate()
    {
        let mut mk = dictionary! { "CA" => literal("Go"), "I" => icon(&mut f) };
        if let Some(tp) = tp {
            mk.set("TP", tp);
        }
        button(&mut f, name, 7 + k, mk);
    }
    let mut not_stream = all.clone();
    not_stream.set("I", 5);
    not_stream.set("TP", 2);
    button(&mut f, "nostream", 10, not_stream);
    let bytes = f.bytes();
    let d = open(&bytes, None);

    let p = push_of(&d, "p");
    assert_eq!(
        (&*p.caption, p.rollover.as_deref(), p.down.as_deref()),
        ("Go", Some("Over"), Some("Down"))
    );
    assert!(p.pressed && p.has_da);
    for (name, pressed) in [("t", true), ("s", true), ("i", false), ("own", false)] {
        assert_eq!(push_of(&d, name).pressed, pressed, "{name}");
    }
    let tp1 = push_of(&d, "tp1");
    assert_eq!(
        (&*tp1.caption, &tp1.rollover, &tp1.down),
        ("", &None, &None)
    );
    assert!(push_of(&d, "own").has_da, "its form's /DA");
    assert!(push_of(&d, "empty").has_da, "an empty one");
    assert_eq!(push_of(&d, "empty").frame.da, b"");
    for name in ["tp0", "none"] {
        assert_eq!(&*push_of(&d, name).caption, "Go", "{name}");
    }
    let refused = look(d.state(), widget_of(&d, "tp2"));
    assert!(matches!(refused, Err(Unmade::Icon)), "{refused:?}");
    assert_eq!(unmade_kind(&Unmade::Icon), "icon");
    assert_eq!(&*push_of(&d, "nostream").caption, "Go", "no icon to show");
    let debug = format!("{p:?}");
    assert!(
        !debug.contains("Go") && !debug.contains("Over") && !debug.contains("Down"),
        "{debug}"
    );
    assert!(debug.contains("caption_len: 2"), "{debug}");

    let mut f = Form::new(1);
    let bare = button(&mut f, "bare", 0, dictionary! { "CA" => literal("Go") });
    f.dict(bare).remove(b"DA");
    let acroform = f.acroform;
    f.dict(acroform).remove(b"DA");
    let d = open(&f.bytes(), None);
    let bare = push_of(&d, "bare");
    assert!(!bare.has_da && bare.frame.da.is_empty(), "no /DA at all");
}

/// What's made for a push button nothing changed, as slices 9 and 23 will
/// ask for one: with no `/H`, `/N` alone, its caption in the `/DA`'s font
/// (asked for by its name); with `/H /P`, `/N`, `/R` and `/D` in that order,
/// the rollover caption in `/R` and the down one (`/AC`) in `/D`, which is
/// drawn on the pressed frame; a rollover or down caption that's empty
/// shows the normal one; with no `/DA` at all, at 12 points in Helvetica,
/// no font name asked; with `/MK /TP 1`, the frame alone. The same look
/// makes the same bytes.
#[test]
fn a_push_button_nothing_changed_is_made_in_its_font() {
    let mut f = Form::new(1);
    let plain = button(&mut f, "plain", 0, dictionary! { "CA" => literal("Go") });
    f.dict(plain).set(
        "MK",
        dictionary! { "CA" => literal("Go"), "BG" => vec![Object::from(0.75)] },
    );
    let all =
        dictionary! { "CA" => literal("Go"), "RC" => literal("Over"), "AC" => literal("Down") };
    let p = button(&mut f, "p", 1, all.clone());
    f.dict(p).set("H", Object::name("P"));
    let e = button(
        &mut f,
        "e",
        2,
        dictionary! { "CA" => literal("Go"), "RC" => literal(""), "AC" => literal("") },
    );
    f.dict(e).set("H", Object::name("P"));
    // No `/DA` at all: none of its own, and the form's taken away (every
    // other button here has its own).
    let bare = button(&mut f, "bare", 3, dictionary! { "CA" => literal("Go") });
    f.dict(bare).remove(b"DA");
    let acroform = f.acroform;
    f.dict(acroform).remove(b"DA");
    let mut icon_only = all.clone();
    icon_only.set("TP", 1);
    button(&mut f, "tp1", 4, icon_only);
    let d = open(&f.bytes(), None);
    assert!(
        ["plain", "p", "e", "bare", "tp1"]
            .iter()
            .all(|n| !stale(d.state(), widget_of(&d, n))),
        "nothing changed"
    );

    let mut asked = None;
    let one = made_asking(&push_of(&d, "plain"), &mut asked).expect("made");
    assert_eq!(asked, Some(Some(b"Helv".to_vec())), "the /DA's font");
    assert_eq!(slots(&one), [Slot::Normal], "/N alone");
    assert_eq!(
        shows(content(&one, Slot::Normal).as_bytes()),
        [b"Go".to_vec()]
    );
    assert!(
        content(&one, Slot::Normal).contains(" 10 Tf\n"),
        "the /DA's size"
    );
    assert!(one.glyphs.is_empty(), "Helvetica draws it all");

    let three = made(&push_of(&d, "p")).expect("made");
    assert_eq!(slots(&three), [Slot::Normal, Slot::Rollover, Slot::Down]);
    for (slot, shown) in [
        (Slot::Normal, "Go"),
        (Slot::Rollover, "Over"),
        (Slot::Down, "Down"),
    ] {
        let text = content(&three, slot);
        assert_eq!(
            shows(text.as_bytes()),
            [shown.as_bytes().to_vec()],
            "{slot:?}"
        );
    }
    // The pressed frame: the background less 0.25, a gray where it has none.
    let (normal, down) = (content(&three, Slot::Normal), content(&three, Slot::Down));
    assert!(down.starts_with("q\n0.75 0.75 0.75 rg\n"), "{down}");
    assert!(!normal.contains("0.75 0.75 0.75 rg"), "{normal}");
    let plain_down = made(&PushLook {
        pressed: true,
        ..push_of(&d, "plain")
    })
    .expect("made");
    let down = content(&plain_down, Slot::Down);
    assert!(down.starts_with("q\n0.5 g\n"), "/BG 0.75 less 0.25: {down}");

    let empty = made(&push_of(&d, "e")).expect("made");
    for slot in [Slot::Normal, Slot::Rollover, Slot::Down] {
        let shown = shows(content(&empty, slot).as_bytes());
        assert_eq!(shown, [b"Go".to_vec()], "{slot:?}: the normal caption");
    }

    let mut asked = None;
    let twelve = made_asking(&push_of(&d, "bare"), &mut asked).expect("made");
    assert_eq!(asked, Some(None), "no font named");
    assert!(
        content(&twelve, Slot::Normal).contains(" 12 Tf\n"),
        "12 points"
    );

    let frame = made(&push_of(&d, "tp1")).expect("made");
    assert_eq!(slots(&frame), [Slot::Normal], "no /H");
    assert!(
        shows(content(&frame, Slot::Normal).as_bytes()).is_empty(),
        "the frame alone"
    );
    assert!(!content(&frame, Slot::Normal).contains("BT"));

    assert_eq!(made(&push_of(&d, "p")), Ok(three), "pure");
}
