//! `SetFieldValues` (M10 slice 9) on forms built in code: each kind of
//! field set and cleared; what each kind refuses, nothing changed after;
//! fields by name, by id and by a name two fields have; presses by the
//! group's rules; what History says; undo and redo; a transaction made on
//! an earlier version lowered again or validated; a plugin refused; and a
//! display set, changed and dropped, with what each means for a save.

use std::collections::BTreeSet;
use std::sync::Arc;

use in_progress::NoProgress;
use sheaf_edit::{AnnotRef, FieldRef, FieldSet, FieldValue, Op};

use super::stale_tests::{on, planned};
use super::tests::opened;
use super::*;
use crate::effects::{Flags, RemovedKind, SigClass, judge};
use crate::forms::appearance::{self, ChoiceShows, Look};
use crate::forms::field::flags;
use crate::forms::fixture::{
    Button, Form, NO_TOGGLE_TO_OFF, RADIO, RADIOS_IN_UNISON, every_kind, row,
};
use crate::forms::{FieldId, Value, buttons};
use crate::model::Document;

/// `every_kind`'s fields, opened: on page 1 `form1[0].name[0]` (filled
/// "Ann Lee"), `empty[0]`, check boxes `on[0]` (on) and `off[0]`, the radio
/// group `pick[0]` (its second button on); on page 2 the combo box
/// `state[0]` (NY of CA and NY), the list box `kind[0]` (1099 of W-2 and
/// 1099), the push button `print[0]` and the signature field `sig[0]`.
fn every() -> Document {
    opened(every_kind().bytes(), "form.pdf")
}

/// `every_kind`'s field `name`'s full name.
fn full(name: &str) -> String {
    format!("form1[0].{name}")
}

/// The field named `name` (a full name) in `d`.
fn id(d: &Document, name: &str) -> FieldId {
    (d.state().field_named(name)).unwrap_or_else(|| panic!("no field {name}"))
}

/// Field `name` of `d` given `value`.
fn set(name: &str, value: FieldValue) -> FieldSet {
    FieldSet {
        field: FieldRef::Name(name.into()),
        value,
        display: None,
    }
}

/// `values` set, and nothing calculated.
fn fill(values: Vec<FieldSet>) -> Op {
    Op::SetFieldValues {
        values,
        calculated: Vec::new(),
    }
}

/// Field `name` of `d`'s value.
fn value(d: &Document, name: &str) -> Option<Value> {
    let node = d.state().field(id(d, name)).expect("the field");
    node.term.as_ref().expect("a terminal").value.clone()
}

/// Field `name` of `d`'s display.
fn display(d: &Document, name: &str) -> Option<Arc<str>> {
    let node = d.state().field(id(d, name)).expect("the field");
    node.term.as_ref().expect("a terminal").display.clone()
}

fn text(t: &str) -> Option<Value> {
    Some(Value::Text(t.into()))
}

fn state(s: &str) -> Option<Value> {
    Some(Value::State(s.into()))
}

/// The widgets of field `name` of `d`, in its terminal's order.
fn widgets(d: &Document, name: &str) -> Vec<AnnotId> {
    let node = d.state().field(id(d, name)).expect("the field");
    let term = node.term.as_ref().expect("a terminal");
    term.widgets.iter().copied().collect()
}

/// The state (`/AS`) each widget of field `name` of `d` shows.
fn shown(d: &Document, name: &str) -> Vec<Option<String>> {
    (widgets(d, name).into_iter())
        .map(|w| {
            let rec = d.state().annot(w).expect("its widget");
            rec.widget().expect("a widget").state.clone()
        })
        .collect()
}

fn states(s: &[&str]) -> Vec<Option<String>> {
    s.iter().map(|s| Some((*s).to_string())).collect()
}

/// `ops` committed in `d`, which must fail with nothing changed: no
/// version, the state as it was. Why.
fn refused(d: &mut Document, ops: Vec<Op>) -> EditError {
    let (st, n, at) = (d.state().clone(), d.version_count(), d.current_version());
    let e = d.commit_ops(ops, "refused").unwrap_err();
    assert_eq!((d.version_count(), d.current_version()), (n, at), "{e}");
    assert_eq!(d.state(), &st, "{e}");
    e
}

/// `refused`, and the error is `Invalid` and says `words`.
fn invalid(d: &mut Document, op: Op, words: &str) {
    match refused(d, vec![op]) {
        EditError::Invalid { op, why } => {
            assert_eq!(op, "set-field-values");
            assert!(why.contains(words), "{why:?} doesn't say {words:?}");
        }
        e => panic!("{e:?} isn't Invalid"),
    }
}

/// The text the first widget of field `name` of `d` is drawn with: a text
/// field's or a combo box's.
fn drawn_text(d: &Document, name: &str) -> String {
    let w = widgets(d, name)[0];
    match appearance::look(d.state(), w).expect("a look") {
        Look::Text(look) => look.text.to_string(),
        Look::Choice(look) => match look.shows {
            ChoiceShows::Combo { text } => text.to_string(),
            other => panic!("{other:?}"),
        },
        other => panic!("{other:?}"),
    }
}

/// Whether every widget of field `name` of `d` has its appearance made
/// anew by a save.
fn stale(d: &Document, name: &str) -> bool {
    (widgets(d, name).into_iter()).all(|w| appearance::stale(d.state(), w))
}

#[test]
fn a_text_field_is_filled_and_cleared() {
    let mut d = every();
    let (empty, name) = (full("empty[0]"), full("name[0]"));
    assert!(!stale(&d, &empty));
    d.commit_ops(
        vec![fill(vec![set(&empty, FieldValue::Text("Bo".into()))])],
        "fill",
    )
    .unwrap();
    assert_eq!(value(&d, &empty), text("Bo"));
    assert!(stale(&d, &empty), "its appearance is made anew");
    assert_eq!(drawn_text(&d, &empty), "Bo");
    d.commit_ops(vec![fill(vec![set(&name, FieldValue::Clear)])], "clear")
        .unwrap();
    assert_eq!(value(&d, &name), None);
    assert_eq!(drawn_text(&d, &name), "");
    // Composed as it's put.
    d.commit_ops(
        vec![fill(vec![set(
            &name,
            FieldValue::Text("Rene\u{301}e".into()),
        )])],
        "fill",
    )
    .unwrap();
    assert_eq!(value(&d, &name), text("Renée"));
}

#[test]
fn a_choice_is_set_to_its_options_and_cleared() {
    let mut d = every();
    let (combo, list) = (full("state[0]"), full("kind[0]"));
    let ops = vec![fill(vec![
        set(&combo, FieldValue::Text("CA".into())),
        set(&list, FieldValue::Items(vec!["W-2".into()])),
    ])];
    d.commit_ops(ops, "choose").unwrap();
    assert_eq!(value(&d, &combo), text("CA"));
    assert_eq!(value(&d, &list), text("W-2"), "one option is a text");
    assert!(stale(&d, &combo) && stale(&d, &list));
    assert_eq!(drawn_text(&d, &combo), "CA");
    d.commit_ops(
        vec![fill(vec![set(&list, FieldValue::Text("1099".into()))])],
        "one",
    )
    .unwrap();
    assert_eq!(value(&d, &list), text("1099"));
    let ops = vec![fill(vec![
        set(&combo, FieldValue::Clear),
        set(&list, FieldValue::Items(Vec::new())),
    ])];
    d.commit_ops(ops, "clear").unwrap();
    assert_eq!((value(&d, &combo), value(&d, &list)), (None, None));
}

/// A check box's and a radio group's value, by state and by export value,
/// and cleared: the field's value and each widget's state, by the group's
/// rules. Setting a value isn't a press: NoToggleToOff doesn't keep a
/// radio button on that's cleared.
#[test]
fn a_check_box_and_a_radio_group_are_set_and_cleared() {
    let mut d = every();
    let (off, pick) = (full("off[0]"), full("pick[0]"));
    d.commit_ops(
        vec![fill(vec![set(&off, FieldValue::State("Yes".into()))])],
        "check",
    )
    .unwrap();
    assert_eq!(
        (value(&d, &off), shown(&d, &off)),
        (state("Yes"), states(&["Yes"]))
    );
    d.commit_ops(vec![fill(vec![set(&off, FieldValue::Clear)])], "uncheck")
        .unwrap();
    assert_eq!(
        (value(&d, &off), shown(&d, &off)),
        (state("Off"), states(&["Off"]))
    );
    assert_eq!(shown(&d, &pick), states(&["Off", "1"]));
    d.commit_ops(
        vec![fill(vec![set(&pick, FieldValue::State("0".into()))])],
        "pick",
    )
    .unwrap();
    assert_eq!(value(&d, &pick), state("0"));
    assert_eq!(shown(&d, &pick), states(&["0", "Off"]));
    d.commit_ops(
        vec![fill(vec![set(&pick, FieldValue::Export("1".into()))])],
        "pick",
    )
    .unwrap();
    assert_eq!(value(&d, &pick), state("1"));
    assert_eq!(shown(&d, &pick), states(&["Off", "1"]));
    d.commit_ops(vec![fill(vec![set(&pick, FieldValue::Clear)])], "clear")
        .unwrap();
    assert_eq!(value(&d, &pick), state("Off"));
    assert_eq!(shown(&d, &pick), states(&["Off", "Off"]));
}

/// A user's text is held to `/MaxLen`, in grapheme clusters, where it's
/// above 0, and to one line where the field isn't multiline; a calculated
/// one to neither.
#[test]
fn a_user_s_text_fits_its_field() {
    let mut f = Form::new(1);
    let short = f.text("short", 0, row(0));
    f.dict(short).set("MaxLen", 3i64);
    let none = f.text("none", 0, row(1));
    f.dict(none).set("MaxLen", 0i64);
    let lines = f.text("lines", 0, row(2));
    f.dict(lines).set("Ff", i64::from(flags::MULTILINE));
    let mut d = opened(f.bytes(), "form.pdf");
    let put = |name: &str, t: &str| fill(vec![set(name, FieldValue::Text(t.into()))]);
    invalid(&mut d, put("short", "abcd"), "takes at most 3 characters");
    // Three clusters of six characters fit three.
    d.commit_ops(vec![put("short", "e\u{301}e\u{301}e\u{301}")], "fits")
        .unwrap();
    assert_eq!(value(&d, "short"), text("ééé"));
    d.commit_ops(vec![put("none", "abcd")], "no maximum")
        .unwrap();
    invalid(&mut d, put("short", "a\nb"), "is one line");
    invalid(&mut d, put("none", "a\rb"), "is one line");
    d.commit_ops(vec![put("lines", "a\r\nb")], "two lines")
        .unwrap();
    assert_eq!(value(&d, "lines"), text("a\r\nb"));
    let calculated = Op::SetFieldValues {
        values: Vec::new(),
        calculated: vec![
            set("short", FieldValue::Text("abcdef".into())),
            set("none", FieldValue::Text("a\nb".into())),
        ],
    };
    d.commit_ops(vec![calculated], "calculated").unwrap();
    assert_eq!(
        (value(&d, "short"), value(&d, "none")),
        (text("abcdef"), text("a\nb"))
    );
}

/// A read-only field is refused to the user, and set by the form's
/// calculations.
#[test]
fn a_read_only_field_is_the_calculations_to_set() {
    let mut f = Form::new(1);
    let total = f.text("total", 0, row(0));
    f.dict(total).set("Ff", i64::from(flags::READ_ONLY));
    let mut d = opened(f.bytes(), "form.pdf");
    let total = || set("total", FieldValue::Text("12".into()));
    invalid(&mut d, fill(vec![total()]), "is read-only");
    let calculated = Op::SetFieldValues {
        values: Vec::new(),
        calculated: vec![total()],
    };
    d.commit_ops(vec![calculated], "calculated").unwrap();
    assert_eq!(value(&d, "total"), text("12"));
}

/// A combo box takes its options, or any text where it's editable; a
/// list box its options alone, several where it's multiple-select, in
/// their order, each once. An option is found as its file writes it, else
/// as both compose, and the value is its own text.
#[test]
fn a_choice_takes_its_options() {
    let mut f = Form::new(1);
    f.choice("fixed", 0, row(0), &["CA", "NY"], true);
    let typed = f.choice("typed", 0, row(1), &["CA", "NY"], true);
    let edit = crate::forms::fixture::COMBO | i64::from(flags::EDIT);
    f.dict(typed).set("Ff", edit);
    let one = f.choice("one", 0, row(2), &[], false);
    let options = ["W-2", "1099", "Rene\u{301}e"].map(crate::pdf::text::string);
    f.dict(one).set("Opt", options.to_vec());
    let many = f.choice("many", 0, row(3), &["W-2", "1099", "1040"], false);
    f.dict(many).set("Ff", i64::from(flags::MULTI_SELECT));
    let mut d = opened(f.bytes(), "form.pdf");
    let put = |name: &str, v: FieldValue| fill(vec![set(name, v)]);
    let t = |t: &str| FieldValue::Text(t.into());
    let items = |i: &[&str]| FieldValue::Items(i.iter().map(|s| s.to_string()).collect());
    invalid(&mut d, put("fixed", t("TX")), "can't be typed in");
    invalid(&mut d, put("fixed", items(&["CA"])), "is a combo box");
    d.commit_ops(vec![put("typed", t("Te\u{301}xas"))], "typed")
        .unwrap();
    assert_eq!(value(&d, "typed"), text("Téxas"), "composed");
    invalid(
        &mut d,
        put("one", items(&["W-2", "1099"])),
        "isn't multiple-select",
    );
    invalid(
        &mut d,
        put("one", items(&["W-2", "W-3"])),
        "value 2 of 2 isn't one",
    );
    // Named composed, the option as its file writes it.
    d.commit_ops(vec![put("one", t("Renée"))], "composed")
        .unwrap();
    assert_eq!(value(&d, "one"), text("Rene\u{301}e"));
    d.commit_ops(vec![put("one", items(&["1099", "1099"]))], "once")
        .unwrap();
    assert_eq!(value(&d, "one"), text("1099"));
    d.commit_ops(vec![put("many", items(&["1040", "W-2", "1040"]))], "many")
        .unwrap();
    let want: Arc<[Arc<str>]> = Arc::new(["W-2".into(), "1040".into()]);
    assert_eq!(
        value(&d, "many"),
        Some(Value::Items(want)),
        "in options order"
    );
}

/// What holds no value, a value of another kind, a state or export value
/// no widget has, a display on a kind that shows none, a parent of fields,
/// a field that isn't there and a widget of another field's: each refused,
/// and nothing of the op set, its sets before the refused one among them.
#[test]
fn what_a_field_can_t_hold_is_refused() {
    let mut d = every();
    let t = |s: &str| FieldValue::Text(s.into());
    for (name, v, words) in [
        ("print[0]", FieldValue::Clear, "is a push button"),
        ("sig[0]", FieldValue::Clear, "is a signature field"),
        (
            "name[0]",
            FieldValue::State("Yes".into()),
            "is a text field",
        ),
        ("on[0]", t("x"), "is a check box"),
        ("pick[0]", FieldValue::Items(Vec::new()), "is a radio group"),
        ("kind[0]", FieldValue::State("x".into()), "is a list box"),
        (
            "on[0]",
            FieldValue::State("Maybe".into()),
            "no on state of that name",
        ),
        (
            "on[0]",
            FieldValue::Export("Maybe".into()),
            "no widget of that export value",
        ),
    ] {
        invalid(&mut d, fill(vec![set(&full(name), v)]), words);
    }
    for name in ["on[0]", "kind[0]"] {
        let shown = FieldSet {
            display: Some("x".into()),
            ..set(&full(name), FieldValue::Clear)
        };
        invalid(
            &mut d,
            fill(vec![shown]),
            "only a text field or a combo box",
        );
    }
    invalid(
        &mut d,
        fill(vec![set("form1[0]", FieldValue::Clear)]),
        "a parent of fields",
    );
    let parent = id(&d, "form1[0]");
    let by_id = |id: FieldId, v| FieldSet {
        field: FieldRef::Id(id),
        value: v,
        display: None,
    };
    invalid(
        &mut d,
        fill(vec![by_id(parent, FieldValue::Clear)]),
        "a parent of fields",
    );
    let e = refused(&mut d, vec![fill(vec![set("nope", FieldValue::Clear)])]);
    assert!(matches!(e, EditError::NotFound { .. }), "{e}");
    let e = refused(
        &mut d,
        vec![fill(vec![by_id(FieldId(9999), FieldValue::Clear)])],
    );
    assert!(matches!(e, EditError::NotFound { .. }), "{e}");
    let other = widgets(&d, &full("on[0]"))[0];
    let press = FieldValue::Press {
        widget: AnnotRef::Id(other),
    };
    invalid(
        &mut d,
        fill(vec![set(&full("off[0]"), press)]),
        "isn't a widget of",
    );
    // The first set is fine, the second isn't: neither is put.
    let ops = vec![fill(vec![
        set(&full("empty[0]"), t("Bo")),
        set(&full("print[0]"), FieldValue::Clear),
    ])];
    refused(&mut d, ops);
    assert_eq!(value(&d, &full("empty[0]")), None);
}

/// A full name names every field that has it, and an id one; ops from a
/// file name fields by name, never by id.
#[test]
fn fields_are_named_by_full_name_or_by_id() {
    let mut f = Form::new(1);
    f.text("dup", 0, row(0));
    f.text("dup", 0, row(1));
    let mut d = opened(f.bytes(), "form.pdf");
    let both: Vec<FieldId> = d.state().fields_named("dup").collect();
    assert_eq!(both.len(), 2);
    let values = |d: &Document| -> Vec<Option<Value>> {
        (both.iter())
            .map(|&id| {
                d.state()
                    .field(id)
                    .unwrap()
                    .term
                    .as_ref()
                    .unwrap()
                    .value
                    .clone()
            })
            .collect()
    };
    let c = d
        .commit_ops(
            vec![fill(vec![set("dup", FieldValue::Text("Bo".into()))])],
            "both",
        )
        .unwrap();
    assert_eq!(values(&d), [text("Bo"), text("Bo")]);
    assert_eq!(
        c.lines[0].text, "Fill “dup” on page 1",
        "one field to a reader"
    );
    let one = FieldSet {
        field: FieldRef::Id(both[1]),
        value: FieldValue::Text("Al".into()),
        display: None,
    };
    d.commit_ops(vec![fill(vec![one.clone()])], "one").unwrap();
    assert_eq!(values(&d), [text("Bo"), text("Al")]);
    // From a file, as `sheaf edit` reads them.
    let json = serde_json::json!({
        "op": "set-field-values",
        "values": [{"field": {"name": "dup"}, "value": "clear"}],
    });
    let mut tx = d.tx(Actor::Cli);
    tx.ops_from_file();
    let e = tx.apply(fill(vec![one]), &NoProgress).unwrap_err();
    assert!(
        e.to_string().contains("session ids aren't meaningful"),
        "{e}"
    );
    tx.apply(serde_json::from_value(json).unwrap(), &NoProgress)
        .unwrap();
    d.commit(tx, CommitOpts::default()).unwrap();
    assert_eq!(values(&d), [None, None]);
}

/// The widgets of `name` in `d` pressed, each in turn and each twice, as
/// ops: each press puts what the group's rules say it does
/// ([`Buttons::press`](crate::forms::Buttons::press)), a press that
/// changes nothing makes no version, and each is undone as one.
fn presses_as_the_group_says(d: &mut Document, name: &str) {
    let field = id(d, name);
    let ws = widgets(d, name);
    let off = |v: Option<&Value>| v.is_none() || v == Some(&Value::State("Off".into()));
    for &w in ws.iter().chain(&ws) {
        let change = buttons(d.state(), field).unwrap().press(w).unwrap();
        let (before, was) = (shown(d, name), value(d, name));
        let mut want = before.clone();
        for (aid, s) in &change.states {
            want[ws.iter().position(|x| x == aid).unwrap()] = Some(s.to_string());
        }
        let press = FieldSet {
            field: FieldRef::Id(field),
            value: FieldValue::Press {
                widget: AnnotRef::Id(w),
            },
            display: None,
        };
        let c = d.commit_ops(vec![fill(vec![press])], "press").unwrap();
        assert_eq!(shown(d, name), want, "{name}: {w:?}");
        let now = value(d, name);
        let held = now.as_ref() == Some(&change.value) || (off(now.as_ref()) && off(was.as_ref()));
        assert!(held, "{name}: {now:?}, the group's {:?}", change.value);
        let moved = !change.states.is_empty() || now != was;
        assert_eq!(c.version.is_some(), moved, "{name}: {w:?}");
        if moved {
            d.undo().unwrap();
            assert_eq!(
                (shown(d, name), value(d, name)),
                (before.clone(), was.clone())
            );
            d.redo().unwrap();
            assert_eq!(shown(d, name), want);
        }
    }
}

/// Presses by the group's rules, over the cases its own tests build: radio
/// buttons with NoToggleToOff and without, check boxes of one on state
/// and of distinct ones, radio buttons in unison, and a widget with no
/// appearance, whose on state's name is made.
#[test]
fn presses_follow_the_group_s_rules() {
    let cases: [(i64, Vec<Button>); 6] = [
        (
            RADIO | NO_TOGGLE_TO_OFF,
            vec![
                Button::at(0, Some("a"), Some("a")),
                Button::at(1, Some("b"), Some("Off")),
                Button::at(2, Some("c"), Some("Off")),
            ],
        ),
        (
            RADIO,
            vec![
                Button::at(0, Some("a"), Some("a")),
                Button::at(1, Some("b"), Some("Off")),
            ],
        ),
        (
            0,
            vec![
                Button::at(0, Some("Yes"), Some("Off")),
                Button::at(1, Some("Yes"), Some("Off")),
            ],
        ),
        (
            0,
            vec![
                Button::at(0, Some("a"), Some("a")),
                Button::at(1, Some("b"), Some("Off")),
            ],
        ),
        (
            RADIO | RADIOS_IN_UNISON,
            vec![
                Button::at(0, Some("x"), Some("Off")),
                Button::at(1, Some("x"), Some("Off")),
                Button::at(2, Some("y"), Some("y")),
            ],
        ),
        (
            RADIO,
            vec![
                Button::at(0, None, None),
                Button::at(1, Some("b"), Some("b")),
            ],
        ),
    ];
    for (ff, buttons) in cases {
        let mut f = Form::new(1);
        f.buttons("g", ff, &buttons);
        let mut d = opened(f.bytes(), "form.pdf");
        presses_as_the_group_says(&mut d, "g");
    }
    // What the first case's press of its second button puts, as the
    // group's own test has it.
    let mut f = Form::new(1);
    let (_, kids) = f.buttons(
        "r",
        RADIO | NO_TOGGLE_TO_OFF,
        &[
            Button::at(0, Some("a"), Some("a")),
            Button::at(1, Some("b"), Some("Off")),
            Button::at(2, Some("c"), Some("Off")),
        ],
    );
    let mut d = opened(f.bytes(), "form.pdf");
    let b = widgets(&d, "r")[1];
    assert_eq!(kids.len(), 3);
    let press = FieldValue::Press {
        widget: AnnotRef::Id(b),
    };
    d.commit_ops(vec![fill(vec![set("r", press)])], "press")
        .unwrap();
    assert_eq!(shown(&d, "r"), states(&["Off", "b", "Off"]));
    assert_eq!(value(&d, "r"), state("b"));
    // A radio button on, in a field with NoToggleToOff, pressed: nothing.
    let mut d = every();
    let pick = full("pick[0]");
    let on = widgets(&d, &pick)[1];
    let press = FieldValue::Press {
        widget: AnnotRef::Id(on),
    };
    let c = d
        .commit_ops(vec![fill(vec![set(&pick, press)])], "press")
        .unwrap();
    assert!(c.version.is_none());
}

/// History names a field by its partial name and the first page it shows
/// on, never by its value; several by how many, calculated ones apart; a
/// run of them in one batch as one line.
#[test]
fn history_says_which_fields_were_filled() {
    let mut d = every();
    let t = |s: &str| FieldValue::Text(s.into());
    let line =
        |d: &mut Document, ops: Vec<Op>| d.commit_ops(ops, "l").unwrap().lines[0].text.clone();
    let said = line(&mut d, vec![fill(vec![set(&full("empty[0]"), t("Bo"))])]);
    assert_eq!(said, "Fill “empty[0]” on page 1");
    let said = line(&mut d, vec![fill(vec![set(&full("state[0]"), t("CA"))])]);
    assert_eq!(said, "Fill “state[0]” on page 2");
    let said = line(
        &mut d,
        vec![fill(vec![set(&full("name[0]"), FieldValue::Clear)])],
    );
    assert_eq!(said, "Clear “name[0]” on page 1");
    let two = fill(vec![
        set(&full("name[0]"), t("Al")),
        set(&full("on[0]"), FieldValue::Clear),
    ]);
    assert_eq!(line(&mut d, vec![two]), "Fill 2 fields");
    let two = fill(vec![
        set(&full("name[0]"), FieldValue::Clear),
        set(&full("empty[0]"), FieldValue::Clear),
    ]);
    assert_eq!(line(&mut d, vec![two]), "Clear 2 fields");
    let calculated = Op::SetFieldValues {
        values: vec![set(&full("name[0]"), t("Cy"))],
        calculated: vec![set(&full("empty[0]"), t("12"))],
    };
    let said = line(&mut d, vec![calculated]);
    assert_eq!(said, "Fill “name[0]” on page 1 (and 1 calculated field)");
    let calculated = Op::SetFieldValues {
        values: Vec::new(),
        calculated: vec![
            set(&full("name[0]"), t("Di")),
            set(&full("empty[0]"), t("13")),
        ],
    };
    assert_eq!(line(&mut d, vec![calculated]), "Fill 2 calculated fields");
    let batch = vec![
        fill(vec![set(&full("name[0]"), t("Ed"))]),
        fill(vec![set(&full("empty[0]"), t("14"))]),
        Op::SetFieldValues {
            values: vec![set(&full("off[0]"), FieldValue::State("Yes".into()))],
            calculated: vec![set(&full("empty[0]"), t("15"))],
        },
    ];
    let c = d.commit_ops(batch, "batch").unwrap();
    let lines: Vec<&str> = c.lines.iter().map(|l| &*l.text).collect();
    assert_eq!(lines, ["Fill 3 fields (and 1 calculated field)"]);
    for value in ["Bo", "CA", "Al", "Cy", "12", "Ed", "15"] {
        assert!(!lines[0].contains(value));
    }
}

/// A fill is undone and redone as one version.
#[test]
fn a_fill_is_undone_and_redone() {
    let mut d = every();
    let (name, off) = (full("name[0]"), full("off[0]"));
    let ops = vec![fill(vec![
        set(&name, FieldValue::Text("Bo".into())),
        set(&off, FieldValue::State("Yes".into())),
    ])];
    d.commit_ops(ops, "Fill").unwrap();
    assert_eq!(d.undo_label(), Some("Fill"));
    d.undo().unwrap();
    assert_eq!(value(&d, &name), text("Ann Lee"));
    assert_eq!(
        (value(&d, &off), shown(&d, &off)),
        (state("Off"), states(&["Off"]))
    );
    d.redo().unwrap();
    assert_eq!(value(&d, &name), text("Bo"));
    assert_eq!(
        (value(&d, &off), shown(&d, &off)),
        (state("Yes"), states(&["Yes"]))
    );
}

/// A set that leaves a field meaning what it did puts nothing: no version.
#[test]
fn a_set_that_changes_nothing_puts_nothing() {
    let mut d = every();
    let n = d.version_count();
    let ops = vec![fill(vec![
        set(&full("name[0]"), FieldValue::Text("Ann Lee".into())),
        set(&full("empty[0]"), FieldValue::Clear),
        set(&full("empty[0]"), FieldValue::Text(String::new())),
        set(&full("on[0]"), FieldValue::State("Yes".into())),
        set(&full("pick[0]"), FieldValue::Export("1".into())),
        set(&full("state[0]"), FieldValue::Text("NY".into())),
        set(&full("kind[0]"), FieldValue::Items(vec!["1099".into()])),
    ])];
    let c = d.commit_ops(ops, "nothing").unwrap();
    assert_eq!((c.version, d.version_count()), (None, n));
    let c = d.commit_ops(vec![fill(Vec::new())], "none").unwrap();
    assert_eq!(c.version, None);
}

/// A fill made on an earlier version is lowered again on the current one,
/// beside a fill of another field since; validated by value, it's a
/// Conflict where its field was filled since.
#[test]
fn a_fill_on_an_earlier_version_lands() {
    let mut d = every();
    let (name, empty) = (full("name[0]"), full("empty[0]"));
    let (v, mut tx) = planned(&d);
    tx.apply(
        fill(vec![set(&name, FieldValue::Text("Bo".into()))]),
        &NoProgress,
    )
    .unwrap();
    d.commit_ops(
        vec![fill(vec![set(&empty, FieldValue::Text("Al".into()))])],
        "Al",
    )
    .unwrap();
    d.commit(tx, on(v, Stale::Relower)).unwrap();
    assert_eq!(
        (value(&d, &name), value(&d, &empty)),
        (text("Bo"), text("Al"))
    );
    let (v, mut tx) = planned(&d);
    tx.apply(
        fill(vec![set(&name, FieldValue::Text("Cy".into()))]),
        &NoProgress,
    )
    .unwrap();
    d.commit_ops(
        vec![fill(vec![set(&name, FieldValue::Text("Di".into()))])],
        "Di",
    )
    .unwrap();
    let e = d.commit(tx, on(v, Stale::Validate)).unwrap_err();
    let field = id(&d, &name);
    assert!(
        matches!(e, EditError::Conflict { key, .. } if key == Key::Field(field)),
        "{e:?}"
    );
    assert_eq!(value(&d, &name), text("Di"));
}

/// A plugin may not fill fields until it can read them: its transaction's
/// op is refused, as needing the user, and nothing is lowered.
#[test]
fn a_plugin_s_fill_is_refused() {
    let d = every();
    let actor = Actor::Plugin {
        id: "p".into(),
        version: "1.0".into(),
    };
    let mut tx = d.tx(actor);
    let op = fill(vec![set(&full("name[0]"), FieldValue::Clear)]);
    assert_eq!(tx.check(&op), Err(EditError::Refused(Cap::User)));
    assert_eq!(
        tx.apply(op, &NoProgress),
        Err(EditError::Refused(Cap::User))
    );
    assert!(tx.is_empty());
}

/// A display is what a text field's or combo box's widgets draw in its
/// value's place, and makes them anew; set, it fills the field in; changed
/// or dropped, the one it replaced is replaced text, which a save keeps out
/// of the file (a Rewrite).
#[test]
fn a_display_is_drawn_and_judged_as_a_value_is() {
    let mut d = every();
    let (empty, combo) = (full("empty[0]"), full("state[0]"));
    let field = id(&d, &empty);
    let shown = |name: &str, v: &str, display: Option<&str>| FieldSet {
        display: display.map(Into::into),
        ..set(name, FieldValue::Text(v.into()))
    };
    let judged = |d: &mut Document, s: FieldSet| {
        let before = d.state().clone();
        let c = d.commit_ops(vec![fill(vec![s])], "display").unwrap();
        let keys = BTreeSet::from([Key::Field(field)]);
        let path = judge(&before, d.state(), &keys);
        let removed: Vec<_> = path.removed.iter().map(|r| (r.kind, r.count)).collect();
        (c.effects.effects.flags, c.effects.effects.sig, removed)
    };
    let (flags, sig, removed) = judged(&mut d, shown(&empty, "12", Some("$12.00")));
    assert_eq!((flags, sig), (Flags::FIELD_VALUES, SigClass::FILL));
    assert!(removed.is_empty());
    assert_eq!(display(&d, &empty).as_deref(), Some("$12.00"));
    assert_eq!(drawn_text(&d, &empty), "$12.00");
    let (flags, _, removed) = judged(&mut d, shown(&empty, "12", Some("12,00 $")));
    assert_eq!(flags, Flags::FIELD_VALUES | Flags::REPLACES_TEXT);
    assert_eq!(removed, [(RemovedKind::FieldValues, 1)]);
    assert_eq!(value(&d, &empty), text("12"));
    let (flags, _, removed) = judged(&mut d, shown(&empty, "12", None));
    assert_eq!(flags, Flags::FIELD_VALUES | Flags::REPLACES_TEXT);
    assert_eq!(removed, [(RemovedKind::FieldValues, 1)]);
    assert_eq!(
        (display(&d, &empty), drawn_text(&d, &empty)),
        (None, "12".into())
    );
    // A field whose value is its file's is stale while it shows a display.
    let mut d = every();
    let name = full("name[0]");
    d.commit_ops(
        vec![fill(vec![shown(&name, "Ann Lee", Some("ANN LEE"))])],
        "d",
    )
    .unwrap();
    assert_eq!(value(&d, &name), text("Ann Lee"));
    assert!(stale(&d, &name));
    d.commit_ops(vec![fill(vec![shown(&name, "Ann Lee", None)])], "d")
        .unwrap();
    assert!(!stale(&d, &name), "as its file draws it");
    // A combo box's, and composed.
    d.commit_ops(
        vec![fill(vec![shown(&combo, "CA", Some("Califo\u{301}rnia"))])],
        "d",
    )
    .unwrap();
    assert_eq!(drawn_text(&d, &combo), "Califórnia");
}
