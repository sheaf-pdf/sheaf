//! A combo box's or list box's appearance from the model's plain data (M10
//! slice 8): which widgets need theirs made, each case; what a look holds
//! (a combo box's text, a list box's options, the selected ones and its
//! `/TI`); and what's made for a choice nothing changed, in the field's font
//! alone and with the fallback, and what's refused, by kind, on forms built
//! in code.

use std::collections::BTreeSet;
use std::sync::Arc;

use super::tests::shows;
use super::*;
use crate::content::Fallback;
use crate::forms::field::{Choice, Value, flags};
use crate::forms::fixture::{COMBO, Form, row};
use crate::materialize::tests::open;
use crate::model::{Document, FieldNode, Terminal};
use crate::pdf::gfx::UnencodableChar;
use crate::pdf::{Object, dictionary, literal, text};
use crate::prim::{Key, Val};
use crate::tx::fixture as fx;

/// A combo box (or, `combo` false, a list box) `name` at row `i` on page
/// 0, its own widget, of `options` (an export value and its label, a
/// string where the two are one), its `/V` `value` where there is one.
fn choice(
    f: &mut Form,
    name: &str,
    i: usize,
    options: &[(&str, &str)],
    combo: bool,
    value: Option<Object>,
) -> ObjectId {
    let id = f.choice(name, 0, row(i), &[], combo);
    let opt: Vec<Object> = (options.iter())
        .map(|&(export, label)| match export == label {
            true => text::string(export),
            false => Object::from(vec![text::string(export), text::string(label)]),
        })
        .collect();
    f.dict(id).set("Opt", opt);
    if let Some(value) = value {
        f.dict(id).set("V", value);
    }
    id
}

/// `texts` as a `/V` array of strings.
fn array(texts: &[&str]) -> Object {
    Object::from(texts.iter().map(|&t| literal(t)).collect::<Vec<_>>())
}

/// Four options, two of them with labels of their own.
const ABCD: &[(&str, &str)] = &[("a", "Alpha"), ("b", "Bravo"), ("c", "c"), ("d", "Delta")];

/// Widget `n` of the field named `name` in `d`.
fn widget_of(d: &Document, name: &str, n: usize) -> AnnotId {
    let st = d.state();
    let id = (st.field_named(name)).unwrap_or_else(|| panic!("no field {name}"));
    let term = (st.field(id).and_then(|f| f.term.clone())).expect("a terminal");
    term.widgets[n]
}

/// The choice look of the first widget of the field named `name`.
fn choice_of(d: &Document, name: &str) -> ChoiceLook {
    match look(d.state(), widget_of(d, name, 0)) {
        Ok(Look::Choice(look)) => look,
        other => panic!("{name}: a choice's widget: {other:?}"),
    }
}

/// The field named `name` of `d` given `value` as one version, whatever
/// it is: what [`fx::choose`] puts, and what it doesn't (an array of one
/// text, an empty array).
fn put_value(d: &mut Document, name: &str, value: Option<Value>) {
    let id = d.state().field_named(name).expect("the field");
    let mut node = FieldNode::clone(d.state().field(id).expect("a field"));
    let mut term = Terminal::clone(node.term.as_deref().expect("a terminal"));
    term.value = value;
    node.term = Some(Arc::new(term));
    let val = Some(Val::Field(Arc::new(node)));
    d.put_for_test(vec![(Key::Field(id), val)], "put");
}

/// `texts` as a value of several.
fn items(texts: &[&str]) -> Value {
    Value::Items(texts.iter().map(|&t| Arc::from(t)).collect())
}

/// Which widgets need their appearance made ([`stale`]), after each change
/// of a value: none as read; a combo box's value moved, and put back; a
/// text and an array of that one text alike; no value and an empty array
/// alike, an empty text not (it selects an option whose export value is
/// empty); a list box's texts in another order; and a text field's fill
/// moves no choice's.
#[test]
fn a_choice_is_stale_where_its_value_s_texts_moved() {
    let mut f = Form::new(1);
    choice(&mut f, "combo", 0, ABCD, true, Some(literal("a")));
    choice(&mut f, "list", 1, ABCD, false, None);
    let multi = choice(&mut f, "multi", 2, ABCD, false, Some(array(&["a", "b"])));
    f.dict(multi).set("Ff", flags::MULTI_SELECT as i64);
    f.text("city", 0, row(3));
    let mut d = open(&f.bytes(), None);
    let names = ["combo", "list", "multi"];
    let aids = names.map(|name| widget_of(&d, name, 0));
    let now = |d: &Document| aids.map(|aid| stale(d.state(), aid));
    assert_eq!(now(&d), [false; 3], "as read");

    type Step = (&'static str, Option<Value>, [bool; 3]);
    let text = |t: &str| Some(Value::Text(t.into()));
    let steps: [Step; 10] = [
        ("combo", text("b"), [true, false, false]),
        ("combo", text("a"), [false, false, false]),
        ("combo", Some(items(&["a"])), [false, false, false]),
        ("combo", Some(items(&["a", "b"])), [true, false, false]),
        ("combo", Some(items(&["b", "a"])), [true, false, false]),
        ("combo", text("a"), [false, false, false]),
        ("list", Some(items(&[])), [false, false, false]),
        ("list", text(""), [false, true, false]),
        ("list", None, [false, false, false]),
        ("multi", Some(items(&["b", "a"])), [false, false, true]),
    ];
    for (name, value, want) in steps {
        put_value(&mut d, name, value.clone());
        assert_eq!(now(&d), want, "{name} = {value:?}");
    }
    fx::choose(&mut d, "multi", &["a", "b"]);
    fx::fill(&mut d, "city", "Camden");
    assert_eq!(now(&d), [false; 3], "put back, and a text field filled");
    fx::choose(&mut d, "list", &["c"]);
    assert_eq!(now(&d), [false, true, false], "chosen");
}

/// What a look holds: a combo box's text, its value's option's label,
/// else the value as it is (an editable one's outside its options), none
/// with no value, an array's first text's, the first of two options of
/// one export value; a list box's options, the indexes its value selects
/// (one, several, every option of an export value two have, none), and its
/// `/TI`, its own or a parent's, 0 where less than 0 or past `u32`.
/// `Debug` says no text.
#[test]
fn the_look_is_what_the_value_shows_among_the_options() {
    let mut f = Form::new(1);
    choice(&mut f, "c1", 0, ABCD, true, Some(literal("b")));
    let c2 = choice(&mut f, "c2", 1, ABCD, true, Some(literal("Zulu")));
    f.dict(c2).set("Ff", COMBO | flags::EDIT as i64);
    choice(&mut f, "c3", 2, ABCD, true, None);
    choice(&mut f, "c4", 3, ABCD, true, Some(array(&["d", "a"])));
    let twice = &[("a", "First"), ("b", "Bravo"), ("a", "Second")];
    choice(&mut f, "c5", 4, twice, true, Some(literal("a")));
    choice(&mut f, "l1", 5, ABCD, false, Some(literal("b")));
    choice(&mut f, "l2", 6, ABCD, false, Some(array(&["c", "a"])));
    choice(&mut f, "l3", 7, twice, false, Some(literal("a")));
    choice(&mut f, "l4", 8, ABCD, false, None);
    let l5 = choice(&mut f, "l5", 9, ABCD, false, Some(literal("a")));
    f.dict(l5).set("TI", 2);
    let l6 = choice(&mut f, "l6", 10, ABCD, false, None);
    f.dict(l6).set("TI", -3);
    let l7 = choice(&mut f, "l7", 11, ABCD, false, None);
    f.dict(l7).set("TI", i64::from(u32::MAX) + 1);
    // A list box whose `/TI` is its parent's.
    let parent = f.pdf.add_object(dictionary! {
        "FT" => Object::name("Ch"), "T" => literal("p"), "TI" => 1,
        "DA" => literal("/Helv 10 Tf 0 g"),
        "Opt" => vec![literal("x"), literal("y")],
    });
    let k = choice(&mut f, "k", 12, &[], false, None);
    for key in [b"FT".as_slice(), b"Opt", b"DA"] {
        f.dict(k).remove(key);
    }
    f.dict(k).set("Parent", parent);
    f.dict(parent).set("Kids", vec![Object::from(k)]);
    let mut fields: Vec<ObjectId> = (f.fields().into_iter()).filter(|&id| id != k).collect();
    fields.push(parent);
    f.set_fields(&fields);
    let d = open(&f.bytes(), None);

    let combo = |name: &str| match choice_of(&d, name).shows {
        ChoiceShows::Combo { text } => text.to_string(),
        other => panic!("{name}: a combo box's: {other:?}"),
    };
    assert_eq!(combo("c1"), "Bravo");
    assert_eq!(combo("c2"), "Zulu", "outside its options, as it is");
    assert_eq!(combo("c3"), "", "no value");
    assert_eq!(combo("c4"), "Delta", "an array's first");
    assert_eq!(combo("c5"), "First", "the first of an export value's");

    let list = |name: &str| match choice_of(&d, name).shows {
        ChoiceShows::List {
            options,
            selected,
            top,
        } => {
            let labels: Vec<String> = options.iter().map(|o| o.shown.to_string()).collect();
            (labels, selected, top)
        }
        other => panic!("{name}: a list box's: {other:?}"),
    };
    let strings = |s: &[&str]| -> Vec<String> { s.iter().map(|&s| s.to_owned()).collect() };
    let abcd = strings(&["Alpha", "Bravo", "c", "Delta"]);
    assert_eq!(list("l1"), (abcd.clone(), vec![1], 0));
    assert_eq!(list("l2"), (abcd.clone(), vec![0, 2], 0), "several, sorted");
    let firsts = strings(&["First", "Bravo", "Second"]);
    assert_eq!(list("l3"), (firsts, vec![0, 2], 0), "both of export a");
    assert_eq!(list("l4"), (abcd.clone(), vec![], 0), "no value");
    assert_eq!(list("l5"), (abcd.clone(), vec![0], 2), "/TI 2");
    assert_eq!(list("l6").2, 0, "/TI less than 0");
    assert_eq!(list("l7").2, 0, "/TI past u32");
    assert_eq!(
        list("p.k"),
        (strings(&["x", "y"]), vec![], 1),
        "its parent's /TI and /Opt"
    );

    // Never a text in its Debug.
    for name in ["c1", "c2", "l2"] {
        let debug = format!("{:?}", Look::Choice(choice_of(&d, name)));
        for text in ["Bravo", "Zulu", "Alpha", "Delta"] {
            assert!(!debug.contains(text), "{name}: {debug}");
        }
    }
    let debug = format!("{:?}", choice_of(&d, "l2"));
    assert!(
        debug.contains("options: 4") && debug.contains("selected: 2"),
        "{debug}"
    );
}

/// Helvetica, and its own resource.
fn helvetica() -> (Arc<FieldFont>, Res) {
    let font = FieldFont::standard(Std14::Helvetica);
    let res = font.res().expect("a standard font's dictionary");
    (Arc::new(font), res)
}

/// `look` made in Helvetica, the name its `/DA` gives held in `asked`.
fn made_asking(look: &ChoiceLook, asked: &mut Option<Vec<u8>>) -> Result<Made, Unmade> {
    let (font, res) = helvetica();
    make(&Look::Choice(look.clone()), |name| {
        *asked = name.map(<[u8]>::to_vec);
        Ok(DaFont { font, res })
    })
}

/// `look` made in Helvetica.
fn made(look: &ChoiceLook) -> Result<Made, Unmade> {
    made_asking(look, &mut None)
}

/// The one form a choice's look makes, `/N`'s.
fn normal(made: &Made) -> &Appearance {
    let slots: Vec<Slot> = made.forms.iter().map(|(slot, _)| *slot).collect();
    assert_eq!(slots, [Slot::Normal], "a choice's /N alone");
    &made.forms[0].1
}

/// What `made`'s one form draws, as text.
fn content(made: &Made) -> String {
    String::from_utf8(normal(made).content.clone()).expect("content that's text")
}

/// The ids of the resources a form names.
fn names(a: &Appearance) -> BTreeSet<ResId> {
    a.needs.0.iter().map(|n| n.res.id()).collect()
}

/// `texts`, each as the bytes a `Tj` shows.
fn bytes_of(texts: &[&str]) -> Vec<Vec<u8>> {
    texts.iter().map(|t| t.as_bytes().to_vec()).collect()
}

/// A choice nothing changed is made as any other ([`make`] doesn't ask
/// [`stale`]), in the font its `/DA` names: a combo box its option's label
/// in `/Tx BMC`, clipped to the box less its border and the drop button,
/// with the drop button after it, light gray with its triangle; one with no
/// value `/Tx BMC EMC` and the button; a list box each line shown from its
/// `/TI`, a selected one over the highlight in white, two selected each
/// so. One form, `/N`'s, and no glyph of the fallback.
#[test]
fn a_choice_nothing_changed_is_made_in_its_font() {
    let mut f = Form::new(1);
    choice(&mut f, "combo", 0, ABCD, true, Some(literal("b")));
    choice(&mut f, "empty", 1, ABCD, true, None);
    let list = choice(&mut f, "list", 2, ABCD, false, Some(array(&["a", "c"])));
    let tall = |y: i64| vec![Object::from(72), y.into(), 272.into(), (y + 60).into()];
    f.dict(list).set("Rect", tall(300));
    let top = choice(&mut f, "top", 3, ABCD, false, Some(literal("d")));
    f.dict(top).set("Rect", tall(200));
    f.dict(top).set("TI", 1);
    let d = open(&f.bytes(), None);
    for name in ["combo", "empty", "list", "top"] {
        assert!(!stale(d.state(), widget_of(&d, name, 0)), "{name}");
    }

    let mut asked = None;
    let combo = made_asking(&choice_of(&d, "combo"), &mut asked).unwrap();
    assert_eq!(asked.as_deref(), Some(b"Helv".as_slice()), "the /DA's font");
    assert!(combo.glyphs.is_empty());
    let text = content(&combo);
    assert!(text.contains("/Tx BMC\nq\n"), "{text}");
    assert_eq!(shows(text.as_bytes()), bytes_of(&["Bravo"]));
    // The text before the drop button, which is light gray with a triangle.
    let (tx, button) = text.split_once("EMC\n").expect("the text's end");
    assert!(tx.contains(" re\nW\nn\n"), "clipped: {tx}");
    assert!(button.contains("0.8627 0.8627 0.8627 rg\n"), "{button}");
    assert!(button.contains(" l\n"), "the triangle: {button}");
    let (_, res) = helvetica();
    assert_eq!(names(normal(&combo)), BTreeSet::from([res.id()]));

    let empty = content(&made(&choice_of(&d, "empty")).unwrap());
    assert!(empty.contains("/Tx BMC\nEMC\n"), "{empty}");
    assert!(shows(empty.as_bytes()).is_empty());
    assert!(
        empty.contains("0.8627 0.8627 0.8627 rg\n"),
        "the button still"
    );

    let list = content(&made(&choice_of(&d, "list")).unwrap());
    assert_eq!(
        shows(list.as_bytes()),
        bytes_of(&["Alpha", "Bravo", "c", "Delta"])
    );
    assert_eq!(list.matches("0 0.2 0.4431 rg\n").count(), 2, "{list}");
    assert_eq!(list.matches("1 g\nBT\n").count(), 2, "{list}");
    let top = content(&made(&choice_of(&d, "top")).unwrap());
    assert_eq!(
        shows(top.as_bytes()),
        bytes_of(&["Bravo", "c", "Delta"]),
        "from /TI 1"
    );
    assert_eq!(top.matches("0 0.2 0.4431 rg\n").count(), 1, "Delta's");
}

/// A plain frame: a box of 200 by 60, a black border of width 1,
/// Helvetica at 10 points.
fn plain_frame() -> Frame {
    Frame {
        width: 200.0,
        height: 60.0,
        background: None,
        border_color: Some(field::Color::Gray(0.0)),
        rotation: 0,
        border: Border::solid(1.0),
        da: b"/Helv 10 Tf 0 g".to_vec(),
    }
}

/// A combo box's look that shows `text`.
fn combo(text: &str) -> ChoiceLook {
    ChoiceLook {
        shows: ChoiceShows::Combo { text: text.into() },
        frame: plain_frame(),
    }
}

/// A list box's look of `labels`, `selected` selected, from `top`.
fn list(labels: &[&str], selected: &[usize], top: usize) -> ChoiceLook {
    let options: Arc<[Choice]> = (labels.iter())
        .map(|&l| Choice {
            export: l.into(),
            shown: l.into(),
        })
        .collect();
    ChoiceLook {
        shows: ChoiceShows::List {
            options,
            selected: selected.to_vec(),
            top,
        },
        frame: plain_frame(),
    }
}

/// What the field's font lacks is drawn in the fallback, named by its
/// marker, each glyph it draws recorded with its text: a WinAnsi text
/// names Helvetica alone and records none; `Łukasz` in a combo box, and
/// `Łukasz` and `Ńowak` in a list box, record those two letters and
/// nothing else. A cluster neither font draws is refused by kind: in a
/// combo box's text `unencodable`, at its offset; in a list box's line
/// that's drawn `unencodable-option`, with its index among the options; a
/// line below the box isn't drawn, so isn't refused.
#[test]
fn a_choice_draws_in_its_font_then_the_fallback() {
    let marker = Fallback::SANS.res().id();
    let (_, helv) = helvetica();
    let program = Fallback::SANS.program();
    let glyphs = |chars: &str| -> Vec<(u16, String)> {
        let mut want: Vec<(u16, String)> = (chars.chars())
            .map(|c| (program.glyph(c).unwrap(), c.to_string()))
            .collect();
        want.sort();
        want
    };
    let got = |made: &Made| -> Vec<(u16, String)> {
        (made.glyphs.iter())
            .map(|(g, t)| (g, t.to_string()))
            .collect()
    };

    let winansi = made(&combo("Camden \u{d6}l\u{e7}er \u{20ac}")).unwrap();
    assert!(winansi.glyphs.is_empty());
    assert_eq!(names(normal(&winansi)), BTreeSet::from([helv.id()]));
    let lukasz = made(&combo("\u{141}ukasz")).unwrap();
    assert_eq!(got(&lukasz), glyphs("\u{141}"));
    assert_eq!(names(normal(&lukasz)), BTreeSet::from([helv.id(), marker]));
    let two = made(&list(&["\u{141}ukasz", "\u{143}owak", "Camden"], &[1], 0)).unwrap();
    assert_eq!(got(&two), glyphs("\u{141}\u{143}"));
    assert_eq!(names(normal(&two)), BTreeSet::from([helv.id(), marker]));
    let alone = made(&list(&["Camden", "Trenton"], &[0], 0)).unwrap();
    assert!(alone.glyphs.is_empty());
    assert_eq!(names(normal(&alone)), BTreeSet::from([helv.id()]));
    // What's made says how many glyphs, never which.
    let debug = format!("{two:?}");
    assert!(
        debug.contains("glyphs: 2") && !debug.contains('\u{141}'),
        "{debug}"
    );

    let character = |at, character| UnencodableChar { at, character };
    let refused = made(&combo("ok \u{738b}")).unwrap_err();
    assert_eq!(refused, Unmade::Unencodable(character(3, '\u{738b}')));
    assert_eq!(unmade_kind(&refused), "unencodable");
    let refused = made(&list(&["\u{141}ukasz", "ok", "\u{1f600}"], &[], 0)).unwrap_err();
    let want = Unmade::UnencodableOption {
        option: 2,
        character: character(0, '\u{1f600}'),
    };
    assert_eq!(refused, want);
    assert_eq!(unmade_kind(&refused), "unencodable-option");
    // Counted from the first option, not from the top.
    let refused = made(&list(&["a", "\u{1f600}"], &[], 1)).unwrap_err();
    assert!(
        matches!(refused, Unmade::UnencodableOption { option: 1, .. }),
        "{refused:?}"
    );
    // Below the box (60 points high), never drawn.
    let mut below = vec!["x"; 8];
    below.push("\u{1f600}");
    assert!(made(&list(&below, &[], 0)).unwrap().glyphs.is_empty());
}

/// What [`make`] guards against: the family's maker, refused partway in a
/// list box, has recorded in the fallback the glyphs of the lines before
/// the refusal (`Ł`), which nothing draws. `make` gives none of them, only
/// the refusal; the emitter's tests hold that no subset a file holds gets
/// them.
#[test]
fn a_refused_make_s_recorded_glyphs_go_with_it() {
    use std::sync::Mutex;

    use crate::content::Glyphs;
    use crate::pdf::forms::{ListBox, list_box};
    use crate::pdf::gfx::WithFallback;

    let look = list(&["\u{141}ukasz", "\u{1f600}"], &[], 0);
    let labels = ["\u{141}ukasz", "\u{1f600}"];
    let (font, res) = helvetica();
    let layout = font.layout();
    let recorded = Mutex::new(Glyphs::new());
    let fallback = Fallback::SANS.recording(&recorded);
    let fonts = WithFallback::new(&layout, &fallback, Fallback::SANS.res());
    let (widget, caps) = (look.frame.widget(), limits());
    let da = look.frame.da(&caps);
    let refused = list_box(&ListBox::new(&labels), &widget, &da, res, &fonts, &caps);
    let is_option_1 =
        |r: Option<&Unmade>| matches!(r, Some(Unmade::UnencodableOption { option: 1, .. }));
    assert!(is_option_1(refused.as_ref().err()), "{refused:?}");
    let program = Fallback::SANS.program();
    let kept: Vec<u16> = recorded.lock().unwrap().iter().map(|(g, _)| g).collect();
    assert_eq!(kept, [program.glyph('\u{141}').unwrap()], "Ł, before it");
    let made = made(&look).map(|m| m.glyphs.len());
    assert!(is_option_1(made.as_ref().err()), "{made:?}");
}

/// A list box is made of as many options from its top as sheaf reads of
/// an `/Opt` (`FormLimits::items`, [`limits`]' `items`), and one more is
/// refused as `limit:items`, before anything is laid out; a top past one
/// shows one fewer, and past every option none. A box of no size, and a
/// combo box's text past the text cap, are refused by kind.
#[test]
fn a_choice_is_made_within_sheaf_s_caps() {
    use crate::pdf::forms::Cap;
    let items = crate::source::Limits::DEFAULT.forms.items;
    assert_eq!(limits().items, items);
    let labels: Vec<String> = (0..=items).map(|i| format!("Option {i}")).collect();
    let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
    let over = made(&list(&labels, &[0], 0)).unwrap_err();
    assert_eq!(over, Unmade::Limit(Cap::Items));
    assert_eq!(unmade_kind(&over), "limit:items");
    assert!(made(&list(&labels[..items], &[0], 0)).is_ok(), "as many");
    assert!(
        made(&list(&labels, &[0], 1)).is_ok(),
        "one fewer from the top"
    );
    let past = made(&list(&["a", "b"], &[0], 9)).unwrap();
    assert!(
        shows(&normal(&past).content).is_empty(),
        "none from past them"
    );

    let none = ChoiceLook {
        frame: Frame {
            width: 0.0,
            ..plain_frame()
        },
        ..combo("Camden")
    };
    assert_eq!(made(&none).unwrap_err(), Unmade::NoBox);
    let long = "x".repeat(crate::source::Limits::DEFAULT.forms.text + 1);
    assert_eq!(made(&combo(&long)).unwrap_err(), Unmade::Limit(Cap::Text));
}

/// Which options a value selects is the family's rule with no `/I`
/// (`selected_options`), read from sheaf's options without a copy: export
/// values given twice, in any order, an empty one, one no option has, and
/// no value at all.
#[test]
fn what_a_value_selects_is_the_familys_rule() {
    use super::choice::selected;
    use crate::pdf::forms::{FieldOption, selected_options};
    let exports = ["b", "a", "", "b", "c", "a", "é"];
    let options: Vec<Choice> = (exports.iter())
        .map(|&e| Choice {
            export: e.into(),
            shown: format!("shown {e}").into(),
        })
        .collect();
    let family: Vec<FieldOption> = exports.iter().map(|&e| FieldOption::new(e)).collect();
    let cases: [&[&str]; 9] = [
        &[],
        &["a"],
        &["b", "a"],
        &["a", "b", "a"],
        &[""],
        &["zz"],
        &["c", "zz", ""],
        &["é", "c"],
        &["B"],
    ];
    for values in cases {
        assert_eq!(
            selected(&options, values),
            selected_options(&family, &[], values),
            "{values:?}"
        );
    }
    assert_eq!(selected(&options, &["b", "a"]), [0, 1, 3, 5]);
    assert_eq!(selected(&options, &[]), [] as [usize; 0]);
}
