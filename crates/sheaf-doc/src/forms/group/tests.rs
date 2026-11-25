//! The group's rules over sheaf's model (M10 slice 8), on forms built in
//! code: radio buttons with and without NoToggleToOff and RadiosInUnison,
//! check boxes of one on state and of distinct ones, names made for a
//! widget with no appearance, `/Opt`'s export values, a value set by name
//! and by export value, a duplicate's copy, a page listed twice, and a
//! press undone as one version. And over the corpus: every check box and
//! radio field's group as sheaf's model holds it, beside the family's own
//! reading of the file (`Field::group`), every difference a named class
//! with its count held; a radio-like check box group pressed; and CA
//! 540's `/Opt` radio buttons' export value.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

use in_progress::NoProgress;

use super::*;
use crate::content::{Objects, Unkept};
use crate::forms::Value;
use crate::forms::corpus::Corpus;
use crate::forms::fixture::{Button, Form, NO_TOGGLE_TO_OFF, RADIO, RADIOS_IN_UNISON};
use crate::history::FixedCount;
use crate::materialize::tests::open;
use crate::model::{DocState, Document};
use crate::pdf::forms::group::{FieldGroup, on_state_name};
use crate::pdf::forms::{self, Limits};
use crate::pdf::{Object, ObjectId, literal};
use crate::store::{BlobKind, Store};
use crate::tx::fixture as fx;

/// The family's caps, as it reads the corpus with them.
const LIMITS: Limits = Limits::DEFAULT;

/// The field named `name` in `d`.
fn field(d: &Document, name: &str) -> FieldId {
    (d.state().field_named(name)).unwrap_or_else(|| panic!("no field {name}"))
}

/// The widgets of field `id` of `d`, in its terminal's order: by page, and
/// each page's by `/Annots`.
fn widgets_of(d: &Document, id: FieldId) -> Vec<AnnotId> {
    let term = d.state().field(id).and_then(|f| f.term.clone());
    term.expect("a terminal").widgets.iter().copied().collect()
}

/// The widgets of the field named `name` in `d`.
fn widgets(d: &Document, name: &str) -> Vec<AnnotId> {
    widgets_of(d, field(d, name))
}

/// Field `id`'s group in `d`.
fn group_of(d: &Document, id: FieldId) -> Buttons {
    buttons(d.state(), id).expect("a check box or radio field")
}

/// The group of the field named `name` in `d`.
fn group(d: &Document, name: &str) -> Buttons {
    group_of(d, field(d, name))
}

/// The state (`/AS`) each widget of field `id` shows, in its terminal's
/// order.
fn shown_of(d: &Document, id: FieldId) -> Vec<Option<String>> {
    let st = d.state();
    (widgets_of(d, id).into_iter())
        .map(|aid| {
            st.annot(aid)
                .and_then(|r| r.widget())
                .expect("a widget")
                .state
                .clone()
        })
        .collect()
}

/// [`shown_of`] the field named `name`, as `&str`s to compare.
fn shown(d: &Document, name: &str) -> Vec<Option<String>> {
    shown_of(d, field(d, name))
}

/// `states` as [`shown`] gives them.
fn states(states: &[&str]) -> Vec<Option<String>> {
    states.iter().map(|s| Some((*s).to_string())).collect()
}

/// The value field `name` of `d` holds.
fn value(d: &Document, name: &str) -> Option<Value> {
    let node = d.state().field(field(d, name)).expect("the field");
    node.term.as_ref().expect("a terminal").value.clone()
}

fn state(name: &str) -> Value {
    Value::State(name.into())
}

/// What `change` puts, to compare: the value, each widget's state after by
/// its place in `widgets`, and the export value.
fn said(change: &Change, widgets: &[AnnotId]) -> (Value, Vec<(usize, String)>, Option<String>) {
    let at = |aid: &AnnotId| widgets.iter().position(|w| w == aid).expect("one of them");
    let states = (change.states.iter())
        .map(|(aid, s)| (at(aid), s.to_string()))
        .collect();
    (change.value.clone(), states, change.export.clone())
}

/// A press puts every widget's state and the field's value; a radio button
/// pressed while on stays on with NoToggleToOff, and turns off without.
#[test]
fn a_radio_button_pressed_turns_the_others_off() {
    for (ff, toggles) in [(RADIO | NO_TOGGLE_TO_OFF, false), (RADIO, true)] {
        let mut f = Form::new(1);
        f.buttons(
            "r",
            ff,
            &[
                Button::at(0, Some("a"), Some("a")),
                Button::at(1, Some("b"), Some("Off")),
                Button::at(2, Some("c"), Some("Off")),
            ],
        );
        let mut d = open(&f.bytes(), None);
        let w = widgets(&d, "r");
        let change = group(&d, "r").press(w[1]).unwrap();
        assert_eq!(
            said(&change, &w),
            (
                state("b"),
                vec![(0, "Off".into()), (1, "b".into())],
                Some("b".into())
            ),
            "{ff}"
        );
        fx::press(&mut d, w[1]);
        assert_eq!(shown(&d, "r"), states(&["Off", "b", "Off"]));
        assert_eq!(value(&d, "r"), Some(state("b")));
        // Pressed again, while on.
        let versions = d.version_count();
        fx::press(&mut d, w[1]);
        match toggles {
            false => {
                assert_eq!(d.version_count(), versions, "nothing changed, no version");
                assert_eq!(shown(&d, "r"), states(&["Off", "b", "Off"]));
                assert_eq!(value(&d, "r"), Some(state("b")));
            }
            true => {
                assert_eq!(shown(&d, "r"), states(&["Off", "Off", "Off"]));
                assert_eq!(value(&d, "r"), Some(state("Off")));
                assert_eq!(group(&d, "r").export(), None);
            }
        }
    }
}

/// With RadiosInUnison, radio buttons of one on state go together; without,
/// the one pressed goes on alone.
#[test]
fn radios_in_unison_turn_on_together() {
    for (ff, together) in [(RADIO | RADIOS_IN_UNISON, true), (RADIO, false)] {
        let mut f = Form::new(1);
        f.buttons(
            "r",
            ff,
            &[
                Button::at(0, Some("a"), Some("Off")),
                Button::at(1, Some("b"), Some("b")),
                Button::at(2, Some("a"), Some("Off")),
            ],
        );
        let mut d = open(&f.bytes(), None);
        let w = widgets(&d, "r");
        fx::press(&mut d, w[0]);
        let third = if together { "a" } else { "Off" };
        assert_eq!(shown(&d, "r"), states(&["a", "Off", third]), "{ff}");
        assert_eq!(value(&d, "r"), Some(state("a")));
        // A value by name turns on every widget of it in unison, the first
        // out of it.
        let change = group(&d, "r").set_state("a");
        assert!(change.states.is_empty(), "{ff}: as it is");
    }
}

/// Check boxes of one on state go on and off together.
#[test]
fn check_boxes_of_one_on_state_go_together() {
    let mut f = Form::new(1);
    f.buttons(
        "c",
        0,
        &[
            Button::at(0, Some("Yes"), Some("Off")),
            Button::at(1, Some("Yes"), Some("Off")),
        ],
    );
    let mut d = open(&f.bytes(), None);
    let w = widgets(&d, "c");
    fx::press(&mut d, w[0]);
    assert_eq!(shown(&d, "c"), states(&["Yes", "Yes"]));
    assert_eq!(value(&d, "c"), Some(state("Yes")));
    fx::press(&mut d, w[1]);
    assert_eq!(shown(&d, "c"), states(&["Off", "Off"]));
    assert_eq!(value(&d, "c"), Some(state("Off")));
}

/// Check boxes of distinct on states act as radio buttons: the one pressed
/// goes on and turns the others off; pressed again, it goes off, as a
/// check box does.
#[test]
fn check_boxes_of_distinct_on_states_act_as_radio_buttons() {
    let mut f = Form::new(1);
    f.buttons(
        "c",
        0,
        &[
            Button::at(0, Some("single"), Some("single")),
            Button::at(1, Some("married"), Some("Off")),
            Button::at(2, Some("widowed"), Some("Off")),
        ],
    );
    let mut d = open(&f.bytes(), None);
    let w = widgets(&d, "c");
    fx::press(&mut d, w[1]);
    assert_eq!(shown(&d, "c"), states(&["Off", "married", "Off"]));
    assert_eq!(value(&d, "c"), Some(state("married")));
    assert_eq!(group(&d, "c").export().as_deref(), Some("married"));
    fx::press(&mut d, w[1]);
    assert_eq!(shown(&d, "c"), states(&["Off", "Off", "Off"]));
    assert_eq!(value(&d, "c"), Some(state("Off")));
}

/// A widget with no appearance has its on state named `Yes`, or under
/// `/Opt` by its index, as the family's `on_state_name` names one: what a
/// press puts in its `/AS` and what its made appearance goes under.
#[test]
fn a_widget_with_no_appearance_is_named_yes_or_by_its_index() {
    let make = |opt: Option<Vec<Object>>| {
        let mut f = Form::new(1);
        let (c, _) = f.buttons(
            "c",
            0,
            &[
                Button::at(0, None, None),
                Button::at(1, None, Some("Off")),
                Button::at(2, Some("own"), Some("Off")),
            ],
        );
        if let Some(opt) = opt {
            f.dict(c).set("Opt", opt);
        }
        open(&f.bytes(), None)
    };
    let mut d = make(None);
    let w = widgets(&d, "c");
    let names: Vec<Option<Vec<u8>>> = w.iter().map(|&aid| group(&d, "c").on_state(aid)).collect();
    assert_eq!(
        names,
        [
            Some(b"Yes".to_vec()),
            Some(b"Yes".to_vec()),
            Some(b"own".to_vec())
        ]
    );
    // Both named `Yes`, they go together.
    fx::press(&mut d, w[0]);
    assert_eq!(shown(&d, "c"), states(&["Yes", "Yes", "Off"]));
    assert_eq!(value(&d, "c"), Some(state("Yes")));

    let opt = vec![literal("x"), literal("y"), literal("z")];
    let mut d = make(Some(opt));
    let w = widgets(&d, "c");
    for (i, &aid) in w.iter().enumerate() {
        let own = (i == 2).then_some(&b"own"[..]);
        let name = group(&d, "c").on_state(aid).unwrap();
        assert_eq!(name, &*on_state_name(own, true, i), "{i}");
    }
    assert_eq!(group(&d, "c").on_state(w[1]).unwrap(), b"1");
    fx::press(&mut d, w[1]);
    assert_eq!(shown(&d, "c"), states(&["Off", "1", "Off"]));
    assert_eq!(value(&d, "c"), Some(state("1")));
    assert_eq!(group(&d, "c").export().as_deref(), Some("y"));
}

/// `/Opt` gives each widget its export value by its index: one past a
/// short `/Opt`, or under an empty one, exports `Yes`; an item that is no
/// text keeps its place, as an empty one, and exports `Yes`; and an
/// array a button field and a choice field share is read for each in its
/// own way.
#[test]
fn opt_gives_each_widget_its_export_value_by_its_index() {
    let exports = |opt: Object| {
        let mut f = Form::new(1);
        let (c, _) = f.buttons(
            "c",
            RADIO,
            &[
                Button::at(0, Some("a"), Some("Off")),
                Button::at(1, Some("b"), Some("Off")),
                Button::at(2, Some("c"), Some("Off")),
            ],
        );
        f.dict(c).set("Opt", opt);
        let mut d = open(&f.bytes(), None);
        let w = widgets(&d, "c");
        let mut out = Vec::new();
        for &aid in &w {
            fx::press(&mut d, aid);
            out.push(group(&d, "c").export());
        }
        out
    };
    let some =
        |s: &[&str]| -> Vec<Option<String>> { s.iter().map(|s| Some(s.to_string())).collect() };
    let opt = |items: Vec<Object>| Object::from(items);
    assert_eq!(
        exports(opt(vec![literal("x"), literal("y"), literal("z")])),
        some(&["x", "y", "z"])
    );
    assert_eq!(exports(opt(vec![literal("x")])), some(&["x", "Yes", "Yes"]));
    assert_eq!(exports(opt(vec![])), some(&["Yes", "Yes", "Yes"]));
    assert_eq!(
        exports(opt(vec![literal("x"), 5.into(), literal("z")])),
        some(&["x", "Yes", "z"])
    );
    assert_eq!(
        exports(opt(vec![
            Object::from(vec![literal("x"), literal("X shown")]),
            literal("y"),
            literal("z")
        ])),
        some(&["x", "y", "z"])
    );
    // No /Opt: the on state's name, and `/Opt` that is no array is none.
    let mut f = Form::new(1);
    f.buttons("c", RADIO, &[Button::at(0, Some("a"), Some("Off"))]);
    let mut d = open(&f.bytes(), None);
    let w = widgets(&d, "c");
    fx::press(&mut d, w[0]);
    assert_eq!(group(&d, "c").export().as_deref(), Some("a"));
    assert_eq!(exports(literal("x")), some(&["a", "b", "c"]));

    // One array, a button field's and a choice field's.
    let mut f = Form::new(1);
    let (c, _) = f.buttons(
        "c",
        0,
        &[
            Button::at(0, Some("a"), Some("Off")),
            Button::at(1, Some("b"), Some("Off")),
            Button::at(2, Some("c"), Some("Off")),
        ],
    );
    let list = f.choice("l", 0, crate::forms::fixture::row(3), &["unused"], false);
    let shared = f
        .pdf
        .add_object(Object::from(vec![literal("x"), 5.into(), literal("z")]));
    f.dict(c).set("Opt", shared);
    f.dict(list).set("Opt", shared);
    let d = open(&f.bytes(), None);
    let options = |name: &str| -> Vec<String> {
        let node = d.state().field(field(&d, name)).unwrap();
        let props = &node.term.as_ref().unwrap().props;
        assert!(props.has_opt, "{name}");
        props.options.iter().map(|o| o.export.to_string()).collect()
    };
    assert_eq!(options("c"), ["x", "", "z"]);
    assert_eq!(options("l"), ["x", "z"]);
}

/// A value set by a state's name, or by an export value, and the export
/// value the widgets show.
#[test]
fn a_value_is_set_by_name_or_by_export_value() {
    let mut f = Form::new(1);
    let (r, _) = f.buttons(
        "r",
        RADIO,
        &[
            Button::at(0, Some("a"), Some("a")),
            Button::at(1, Some("b"), Some("Off")),
            Button::at(2, Some("c"), Some("Off")),
        ],
    );
    f.dict(r).set(
        "Opt",
        vec![literal("ex a"), literal("ex b"), literal("ex c")],
    );
    f.check_box("x", 0, crate::forms::fixture::row(5), false);
    let mut d = open(&f.bytes(), None);
    let w = widgets(&d, "r");
    let g = group(&d, "r");
    assert_eq!(g.export().as_deref(), Some("ex a"));
    assert_eq!(
        said(&g.set_state("c"), &w),
        (
            state("c"),
            vec![(0, "Off".into()), (2, "c".into())],
            Some("ex c".into())
        )
    );
    assert_eq!(
        said(&g.set_state("Off"), &w),
        (state("Off"), vec![(0, "Off".into())], None)
    );
    // A name no widget has, or no name at all, is every widget off.
    assert_eq!(said(&g.set_state("zz"), &w), said(&g.set_state("Off"), &w));
    assert_eq!(said(&g.set_state("€"), &w), said(&g.set_state("Off"), &w));
    let by_export = g.set_export("ex b").expect("a widget exports it");
    assert_eq!(
        said(&by_export, &w),
        (
            state("b"),
            vec![(0, "Off".into()), (1, "b".into())],
            Some("ex b".into())
        )
    );
    assert!(
        g.set_export("b").is_none(),
        "under /Opt, b is no export value"
    );
    fx::set_buttons(&mut d, "r", "b");
    assert_eq!(shown(&d, "r"), states(&["Off", "b", "Off"]));
    assert_eq!(value(&d, "r"), Some(state("b")));
    assert_eq!(group(&d, "r").export().as_deref(), Some("ex b"));
    // A widget not the group's.
    assert_eq!(group(&d, "x").press(w[0]), None);
    assert_eq!(group(&d, "x").on_state(w[0]), None);
}

/// An index the model holds no widget at keeps the on state its file gives
/// it: a widget on no page, and one a page delete took. So a value set by
/// the name of a widget held after it turns that widget on, not the one
/// that isn't there (which the rules would else name `Yes`, or its index
/// under `/Opt`); set by the missing one's own name, the value is that
/// name and every held widget is off, as the family's rules give for its
/// reading of the file; and a press of the held one is as with every
/// widget held.
#[test]
fn a_widget_the_model_doesnt_hold_keeps_its_files_on_state() {
    let on = |page, on_state, shows| Button {
        page,
        rect: crate::forms::fixture::row(0),
        on_state: Some(on_state),
        shows: Some(shows),
    };
    let check = |d: &mut Document, opt: bool| {
        let w = widgets(d, "r");
        assert_eq!(w.len(), 1, "the widget held");
        let g = group(d, "r");
        assert_eq!(g.members.len(), 2, "both indexes");
        assert!(g.shown[0].is_empty(), "the first is held nowhere");
        assert!(format!("{g:?}").contains("holes: 1"), "{g:?}");
        assert_eq!(g.on_state(w[0]), Some(b"Yes".to_vec()));
        // The value it has: nothing changes.
        let export = Some(if opt { "ex yes" } else { "Yes" }.to_string());
        assert_eq!(
            said(&g.set_state("Yes"), &w),
            (state("Yes"), vec![], export.clone())
        );
        // The missing widget's name: the value is it, and the held one off.
        let export_no = Some(if opt { "ex no" } else { "No" }.to_string());
        assert_eq!(
            said(&g.set_state("No"), &w),
            (state("No"), vec![(0, "Off".into())], export_no)
        );
        assert_eq!(
            said(
                &g.set_export(if opt { "ex yes" } else { "Yes" }).unwrap(),
                &w
            ),
            (state("Yes"), vec![], export)
        );
        // Pressed while on, with no NoToggleToOff, it turns off.
        assert_eq!(
            said(&g.press(w[0]).unwrap(), &w),
            (state("Off"), vec![(0, "Off".into())], None)
        );
        fx::set_buttons(d, "r", "Yes");
        assert_eq!(shown(d, "r"), states(&["Yes"]));
        assert_eq!(value(d, "r"), Some(state("Yes")));
    };
    for opt in [false, true] {
        // On no page: its field lists it, no page's /Annots does.
        let mut f = Form::new(1);
        let (r, kids) = f.buttons("r", RADIO, &[on(0, "No", "Off"), on(0, "Yes", "Yes")]);
        if opt {
            f.dict(r)
                .set("Opt", vec![literal("ex no"), literal("ex yes")]);
        }
        f.set_annots(0, &kids[1..]);
        let mut d = open(&f.bytes(), None);
        let node = d.state().field(field(&d, "r")).unwrap();
        let term = node.term.as_ref().unwrap();
        assert_eq!(term.outside, 1);
        assert_eq!(
            term.props.on_states,
            [Some("No".to_string()), Some("Yes".to_string())]
        );
        check(&mut d, opt);

        // On a page since deleted.
        let mut f = Form::new(2);
        let (r, _) = f.buttons("r", RADIO, &[on(0, "No", "Off"), on(1, "Yes", "Yes")]);
        if opt {
            f.dict(r)
                .set("Opt", vec![literal("ex no"), literal("ex yes")]);
        }
        let mut d = open(&f.bytes(), None);
        assert_eq!(widgets(&d, "r").len(), 2);
        let first = d.state().order()[0];
        d.commit_ops(vec![fx::delete(&[first])], "delete").unwrap();
        check(&mut d, opt);
    }
}

/// The name a widget's on state goes under is the same read from its
/// record and its field's alone ([`on_state`], what a toggle's look takes)
/// as its group gives ([`Buttons::on_state`]): a name of its own, none,
/// under `/Opt` and not, and a duplicate's copy.
#[test]
fn a_widgets_on_state_s_name_needs_no_group() {
    let mut f = Form::new(2);
    let buttons = [
        Button::at(0, None, None),
        Button::at(1, Some("own"), Some("Off")),
        Button::at(2, Some("Off"), Some("Off")),
    ];
    f.buttons("plain", 0, &buttons);
    let (c, _) = f.buttons("opt", RADIO, &buttons);
    f.dict(c).set("Opt", vec![literal("x")]);
    let mut d = open(&f.bytes(), None);
    let first = d.state().order()[0];
    d.commit_ops(vec![fx::duplicate(&[first])], "dup").unwrap();
    let st = d.state();
    let mut names = Vec::new();
    for (id, _, term) in st.terminals() {
        let group = buttons_of(st, id);
        for &aid in &term.widgets {
            let w = st.annot(aid).and_then(|r| r.widget()).unwrap();
            let name = on_state(term, w);
            assert_eq!(Some(&name), group.on_state(aid).as_ref());
            names.push(String::from_utf8(name).unwrap());
        }
    }
    names.sort();
    let expected = [
        "0", "0", "2", "2", "Yes", "Yes", "Yes", "Yes", "own", "own", "own", "own",
    ];
    assert_eq!(names, expected);
}

/// Field `id`'s group in `st`.
fn buttons_of(st: &DocState, id: FieldId) -> Buttons {
    buttons(st, id).expect("a check box or radio field")
}

/// A duplicated page's copy of a group is a group of its own: pressing its
/// widget changes the copy alone, and the original's widget on the page
/// not duplicated is no widget of the copy's.
#[test]
fn a_duplicated_pages_copy_of_a_group_changes_alone() {
    let mut f = Form::new(2);
    let on = |page, on_state, shows| Button {
        page,
        rect: crate::forms::fixture::row(0),
        on_state: Some(on_state),
        shows: Some(shows),
    };
    f.buttons("r", RADIO, &[on(0, "a", "a"), on(1, "b", "Off")]);
    let mut d = open(&f.bytes(), None);
    let first = d.state().order()[0];
    d.commit_ops(vec![fx::duplicate(&[first])], "dup").unwrap();
    let st = d.state();
    let radios = |copy: bool| -> Vec<FieldId> {
        (st.terminals())
            .filter(|(_, node, term)| term.kind == FieldKind::Radio && (node.inst.copy > 0) == copy)
            .map(|(id, _, _)| id)
            .collect()
    };
    let (original, copy) = match (&radios(false)[..], &radios(true)[..]) {
        (&[original], &[copy]) => (original, copy),
        other => panic!("the field and its copy: {other:?}"),
    };
    assert_eq!(
        widgets_of(&d, copy).len(),
        1,
        "the duplicated page's widget"
    );
    let copied = widgets_of(&d, copy)[0];
    // Its on state is the original's, at the original's index.
    assert_eq!(group_of(&d, copy).on_state(copied), Some(b"a".to_vec()));
    let change = group_of(&d, copy).press(copied).unwrap();
    assert_eq!(change.value, state("Off"), "pressed while on, it turns off");
    fx::press(&mut d, copied);
    assert_eq!(shown_of(&d, copy), states(&["Off"]));
    assert_eq!(shown_of(&d, original), states(&["a", "Off"]));
    let held = |id: FieldId| {
        let node = d.state().field(id).unwrap();
        node.term.as_ref().unwrap().value.clone()
    };
    assert_eq!(held(original), Some(state("a")));
    assert_eq!(held(copy), Some(state("Off")));
}

/// A page the page tree lists twice shows each widget twice, at one
/// index: a press puts both.
#[test]
fn a_page_listed_twice_puts_both_of_a_widgets_records() {
    let mut f = Form::new(1);
    f.buttons(
        "r",
        RADIO,
        &[
            Button::at(0, Some("a"), Some("Off")),
            Button::at(1, Some("b"), Some("b")),
        ],
    );
    let page = f.pages[0];
    let root = f.dict(page).get(b"Parent").unwrap().as_reference().unwrap();
    f.dict(root).set("Kids", vec![page.into(), page.into()]);
    f.dict(root).set("Count", 2);
    let mut d = open(&f.bytes(), None);
    let w = widgets(&d, "r");
    assert_eq!(w.len(), 4, "each widget on both pages");
    let index = |aid: AnnotId| {
        let st = d.state();
        st.annot(aid).and_then(|r| r.widget()).unwrap().props.index
    };
    assert_eq!(
        w.iter().map(|&a| index(a)).collect::<Vec<_>>(),
        [0, 1, 0, 1]
    );
    fx::press(&mut d, w[2]);
    assert_eq!(shown(&d, "r"), states(&["a", "Off", "a", "Off"]));
    assert_eq!(value(&d, "r"), Some(state("a")));
}

/// A press is one version: undone, every widget and the value are as they
/// were, and redone, as pressed.
#[test]
fn a_press_is_one_version() {
    let mut f = Form::new(1);
    f.buttons(
        "r",
        RADIO,
        &[
            Button::at(0, Some("a"), Some("a")),
            Button::at(1, Some("b"), Some("Off")),
            Button::at(2, Some("c"), None),
        ],
    );
    let mut d = open(&f.bytes(), None);
    let w = widgets(&d, "r");
    let versions = d.version_count();
    fx::press(&mut d, w[1]);
    assert_eq!(d.version_count(), versions + 1);
    let pressed = (shown(&d, "r"), value(&d, "r"));
    assert_eq!(
        pressed,
        (states(&["Off", "b", "Off"]), Some(state("b"))),
        "a widget that showed no state shows Off"
    );
    d.undo().unwrap().expect("a version to undo");
    assert_eq!(
        (shown(&d, "r"), value(&d, "r")),
        (
            vec![Some("a".into()), Some("Off".into()), None],
            Some(state("a"))
        )
    );
    assert!(!d.can_undo(), "the press was one version");
    d.redo().unwrap().expect("a version to redo");
    assert_eq!((shown(&d, "r"), value(&d, "r")), pressed);
}

/// `Debug` says the kind, the flags and the counts, never a name or an
/// export value.
#[test]
fn debug_says_counts_and_flags_alone() {
    let mut f = Form::new(1);
    let (r, _) = f.buttons(
        "r",
        RADIO | NO_TOGGLE_TO_OFF,
        &[
            Button::at(0, Some("secret-a"), Some("secret-a")),
            Button::at(1, Some("secret-b"), Some("Off")),
        ],
    );
    f.dict(r)
        .set("Opt", vec![literal("hidden-x"), literal("hidden-y")]);
    let d = open(&f.bytes(), None);
    let g = group(&d, "r");
    let said = format!("{g:?}");
    assert!(
        !said.contains("secret") && !said.contains("hidden"),
        "{said}"
    );
    assert!(
        said.contains("no_toggle_to_off: true") && said.contains("widgets: 2"),
        "{said}"
    );
    let change = format!("{:?}", g.set_state("secret-b"));
    assert!(
        !change.contains("secret") && !change.contains("hidden"),
        "{change}"
    );
}

/// A corpus file opened as a document, beside the family's reading of it.
struct Opened {
    path: String,
    doc: Document,
}

/// The file at `path` of the corpus opened as a document; none where it
/// doesn't open.
fn opened(corpus: &Corpus, path: &str) -> Option<Opened> {
    let bytes = std::fs::read(corpus.root.join(path)).unwrap();
    let store = Store::new();
    let src = store
        .open(bytes.into(), BlobKind::file(Path::new(path), None))
        .ok()?;
    let policy = Box::new(FixedCount(1));
    let mut doc = crate::open::start(store, src.clone(), path.into(), None, policy);
    doc.ready(crate::open::finish(&src, &NoProgress)).ok()?;
    Some(Opened {
        path: path.into(),
        doc,
    })
}

/// The family's group of the field `field` of `o`'s file, at its object.
fn family_group(o: &Opened, object: ObjectId) -> Option<FieldGroup> {
    let src = o.doc.primary();
    let pdf = src.pdf();
    let unkept = Unkept::default();
    let objects = Objects::new(pdf, &unkept);
    let form = forms::read(objects, pdf.catalog().ok()?, &LIMITS).ok()??;
    let (_, theirs) = form.terminals().find(|(_, f)| f.id() == Some(object))?;
    forms::field_group(theirs, objects, &LIMITS).ok()?
}

/// Every check box and radio field of one corpus file set beside the
/// family's reading of it, by class.
#[derive(Debug, Default)]
struct Tally {
    /// Fields both read as groups, set beside each other, and the indexes
    /// compared among them.
    compared: usize,
    indexes: usize,
    /// Fields whose own dictionary is a widget beside kid widgets: sheaf
    /// holds it at an index after the kids', where the family (as PDFium)
    /// counts the kids alone.
    own_after_kids: usize,
    /// Fields with widgets on no page, which the model doesn't hold, and
    /// how many: indexes none of whose widgets is compared.
    on_no_page: usize,
    unheld: usize,
    /// Of those, the indexes set beside the family's all the same, by the
    /// on state the field's record keeps for each.
    unheld_compared: usize,
    /// Fields whose own dictionary is a widget beside kid widgets that no
    /// page lists: past every kid, so in neither group, and one of the
    /// field's widgets on no page (its `outside`).
    own_on_no_page: usize,
    /// sheaf's fields the family reads as no terminal field: a nameless
    /// widget beside named kids, which the family reads as a field of its
    /// own.
    not_the_familys: usize,
    /// The family's check box and radio fields sheaf reads none of: in a
    /// form sheaf reads as damaged, and others (a field reached only
    /// through `/Parent`, a nameless widget beside named kids).
    family_only_damaged: usize,
    family_only: usize,
    /// Any other difference, which must be none.
    other: Vec<String>,
}

impl Tally {
    fn add(&mut self, t: Tally) {
        self.compared += t.compared;
        self.indexes += t.indexes;
        self.own_after_kids += t.own_after_kids;
        self.on_no_page += t.on_no_page;
        self.unheld += t.unheld;
        self.unheld_compared += t.unheld_compared;
        self.own_on_no_page += t.own_on_no_page;
        self.not_the_familys += t.not_the_familys;
        self.family_only_damaged += t.family_only_damaged;
        self.family_only += t.family_only;
        self.other.extend(t.other);
    }
}

/// `o`'s check box and radio fields, sheaf's beside the family's.
fn tally(o: &Opened) -> Tally {
    let mut t = Tally::default();
    let st = o.doc.state();
    let src = o.doc.primary();
    let pdf = src.pdf();
    let unkept = Unkept::default();
    let objects = Objects::new(pdf, &unkept);
    let family = match pdf.catalog().map(|c| forms::read(objects, c, &LIMITS)) {
        Ok(Ok(Some(form))) => form,
        _ => {
            t.other
                .push(format!("{}: the family reads no form", o.path));
            return t;
        }
    };
    let theirs: HashMap<ObjectId, _> = (family.terminals())
        .filter_map(|(_, f)| Some((f.id()?, f)))
        .collect();
    let mut ours_objects = HashSet::new();
    for (fid, node, term) in st.terminals() {
        let object = node.src.as_ref().expect("read from the file").id;
        ours_objects.insert(object);
        if !matches!(term.kind, FieldKind::CheckBox | FieldKind::Radio) {
            continue;
        }
        let at = format!("{} {} {}", o.path, object.number, object.generation);
        let ours = buttons(st, fid).expect("a group");
        let Some(field) = theirs.get(&object) else {
            t.not_the_familys += 1;
            continue;
        };
        let theirs = match forms::field_group(field, objects, &LIMITS) {
            Ok(Some(group)) => group,
            other => {
                t.other.push(format!(
                    "{at}: the family's group: {:?}",
                    other.map(|g| g.is_some())
                ));
                continue;
            }
        };
        t.compared += 1;
        let (a, b) = (ours.group(), theirs.group());
        let flags = |g: &Group<'_>| (g.kind, g.no_toggle_to_off, g.in_unison, g.has_opt);
        if flags(&a) != flags(&b) {
            t.other
                .push(format!("{at}: {:?} and {:?}", flags(&a), flags(&b)));
            continue;
        }
        let (n, m) = (a.widgets.len(), b.widgets.len());
        // The field's own dictionary is a widget beside its kids.
        let own_widget = {
            let d = pdf.get_dictionary(object).ok();
            d.is_some_and(|d| {
                d.get(b"Subtype").and_then(Object::as_name) == Some(&b"Widget"[..])
                    && d.has(b"Kids")
            })
        };
        if n > m {
            // The extra index is the field's own dictionary, the last,
            // held where a page lists it.
            let own = |aid: &(AnnotId, Option<String>)| {
                st.annot(aid.0)
                    .and_then(|r| r.widget())
                    .is_some_and(|w| w.merged)
            };
            match n == m + 1 && own_widget && ours.shown[m].iter().all(own) {
                true if !ours.shown[m].is_empty() => t.own_after_kids += 1,
                true => {}
                false => t.other.push(format!("{at}: {n} indexes, the family's {m}")),
            }
        }
        let unheld = (ours.shown[..n.min(m)]
            .iter()
            .filter(|w| w.is_empty())
            .count())
            + m.saturating_sub(n);
        let own_unlisted = usize::from(own_widget && src.form().widget_by_object(object).is_none());
        // Every widget of the field on no page is accounted for.
        if unheld + own_unlisted != usize::from(term.outside) {
            t.other.push(format!(
                "{at}: {unheld} unheld and {own_unlisted} of its own, {} outside",
                term.outside
            ));
        }
        if unheld > 0 {
            t.on_no_page += 1;
            t.unheld += unheld;
        }
        t.own_on_no_page += own_unlisted;
        let on = ours.states(&a);
        for i in 0..n.min(m) {
            // An index the model holds no widget at is named and exports
            // as the family's; what its file's widget shows isn't read.
            let held = !ours.shown[i].is_empty();
            match held {
                true => t.indexes += 1,
                false => t.unheld_compared += 1,
            }
            let ours_on = if held { on[i] } else { theirs.states()[i] };
            let named = (a.on_state(i), a.export_value(i), ours_on);
            let family = (b.on_state(i), b.export_value(i), theirs.states()[i]);
            if named != family {
                t.other.push(format!("{at} #{i}: {named:?} and {family:?}"));
            }
            // A held widget's own record names its on state as its field's.
            for (aid, _) in &ours.shown[i] {
                let w = st.annot(*aid).and_then(|r| r.widget()).expect("a widget");
                if w.props.on_state != term.props.on_states[i] {
                    t.other
                        .push(format!("{at} #{i}: the widget's on state and the field's"));
                }
                if Some(on_state(term, w)) != a.on_state(i).map(Cow::into_owned) {
                    t.other
                        .push(format!("{at} #{i}: the name without the group"));
                }
            }
        }
    }
    let damaged = o.doc.primary().form().facts.damaged.is_some();
    for (_, f) in family.terminals() {
        let toggle = matches!(
            f.kind(),
            Some(forms::FieldKind::CheckBox | forms::FieldKind::Radio)
        );
        if toggle && !f.id().is_some_and(|id| ours_objects.contains(&id)) {
            match damaged {
                true => t.family_only_damaged += 1,
                false => t.family_only += 1,
            }
        }
    }
    t
}

/// Every check box and radio field of every corpus form: its group as
/// sheaf's model holds it, beside the family's reading of the file
/// (`Field::group`), equal in kind, flags and `/Opt`, at every index in
/// its on state's name and its export value (one the model holds no
/// widget at too, by the name the field's record keeps), and at each the
/// model holds in whether it's on. What differs is a class held to its
/// count. Skips without the corpus.
#[test]
fn corpus_groups_are_the_familys() {
    let Some(corpus) = Corpus::or_skip() else {
        return;
    };
    // On this thread alone: the lib's other tests run beside it, one of
    // them holding a time.
    let mut t = Tally::default();
    for entry in corpus.forms().unwrap() {
        if let Some(o) = opened(&corpus, &entry.path) {
            t.add(tally(&o));
        }
    }
    t.other.sort();
    eprintln!(
        "{} check box and radio fields compared, {} indexes; {} own after kids; {} fields \
         with {} widgets on no page; {} own on no page; {} not the family's; the family's \
         alone: {} in damaged forms, {} others",
        t.compared,
        t.indexes,
        t.own_after_kids,
        t.on_no_page,
        t.unheld,
        t.own_on_no_page,
        t.not_the_familys,
        t.family_only_damaged,
        t.family_only,
    );
    assert!(t.other.is_empty(), "{:#?}", t.other);
    let held: BTreeMap<&str, usize> = BTreeMap::from([
        ("compared", t.compared),
        ("indexes", t.indexes),
        ("own_after_kids", t.own_after_kids),
        ("on_no_page", t.on_no_page),
        ("unheld", t.unheld),
        ("unheld_compared", t.unheld_compared),
        ("own_on_no_page", t.own_on_no_page),
        ("not_the_familys", t.not_the_familys),
        ("family_only_damaged", t.family_only_damaged),
        ("family_only", t.family_only),
    ]);
    let expected = BTreeMap::from([
        ("compared", 63_116),
        ("indexes", 76_925),
        ("own_after_kids", 36),
        ("on_no_page", 105),
        ("unheld", 109),
        ("unheld_compared", 109),
        ("own_on_no_page", 10),
        ("not_the_familys", 1),
        ("family_only_damaged", 28),
        ("family_only", 2),
    ]);
    assert_eq!(held, expected);
}

/// The family's rules over its reading of a field, `theirs`, pressing
/// index `index` where the widgets show `shown` (by index): what each
/// widget's `/AS` becomes and the value, names a character a byte as the
/// model holds them, and the export value.
fn family_pressed(
    theirs: &FieldGroup,
    shown: &[String],
    index: usize,
) -> (Vec<String>, Option<String>, Option<String>) {
    let g = theirs.group();
    let on: Vec<bool> = (shown.iter().enumerate())
        .map(|(i, s)| g.on_state(i).map(|n| n.into_owned()) == name_bytes(s))
        .collect();
    let after = g.press(&on, index);
    let name = |b: &[u8]| name_text(b);
    let states = (0..after.len())
        .map(|i| name(&g.state_name(i, after[i])))
        .collect();
    let value = Some(name(&g.value(&after)));
    (states, value, g.export(&after).map(|e| e.into_owned()))
}

/// The states the model's widgets of field `name` of `o` show, by index,
/// its value and its export value.
fn model_shows(o: &Opened, name: &str) -> (Vec<String>, Option<String>, Option<String>) {
    let d = &o.doc;
    let st = d.state();
    let mut by_index: BTreeMap<u32, String> = BTreeMap::new();
    for aid in widgets(d, name) {
        let w = st.annot(aid).and_then(|r| r.widget()).unwrap();
        by_index.insert(w.props.index, w.state.clone().unwrap_or_default());
    }
    let value = match value(d, name) {
        Some(Value::State(s)) => Some(s.to_string()),
        _ => None,
    };
    (
        by_index.into_values().collect(),
        value,
        group(d, name).export(),
    )
}

/// A radio-like check box group of the corpus's, NY IT-201's filing
/// status (five check boxes of distinct on states): each pressed puts it on
/// and every other off, as the family's rules do to the family's reading.
#[test]
fn corpus_a_radio_like_check_box_group_is_pressed() {
    let Some(corpus) = Corpus::or_skip() else {
        return;
    };
    let mut o = opened(&corpus, "forms/ny-tax/it201_2025_fill_in.pdf").expect("its check boxes");
    let name = "Filing_status";
    let id = field(&o.doc, name);
    let object = o.doc.state().field(id).unwrap().src.as_ref().unwrap().id;
    assert_eq!(
        o.doc.state().field(id).unwrap().term.as_ref().unwrap().kind,
        FieldKind::CheckBox
    );
    let w = widgets(&o.doc, name);
    assert_eq!(w.len(), 5);
    let names: HashSet<Vec<u8>> = w
        .iter()
        .filter_map(|&aid| group(&o.doc, name).on_state(aid))
        .collect();
    assert_eq!(names.len(), 5, "distinct on states");
    let theirs = family_group(&o, object).expect("the family's group");
    for aid in [w[2], w[4], w[0]] {
        let index = {
            let st = o.doc.state();
            st.annot(aid).and_then(|r| r.widget()).unwrap().props.index as usize
        };
        let want = family_pressed(&theirs, &model_shows(&o, name).0, index);
        fx::press(&mut o.doc, aid);
        let got = model_shows(&o, name);
        assert_eq!(got, want, "pressed #{index}");
        let on: Vec<usize> = (got.0.iter().enumerate())
            .filter(|(_, s)| *s != "Off")
            .map(|(i, _)| i)
            .collect();
        assert_eq!(on, [index], "it alone is on");
    }
}

/// CA 540's line 91 radio buttons have `/Opt`: pressed, the value is the
/// widget's on state's name and the export value its `/Opt` entry, as the
/// family's rules give them.
#[test]
fn corpus_an_opt_radio_buttons_export_value() {
    let Some(corpus) = Corpus::or_skip() else {
        return;
    };
    let mut o = opened(&corpus, "forms/ca-ftb/2025-540.pdf").expect("its buttons");
    let name = "540_form_3020 RB";
    let id = field(&o.doc, name);
    let object = o.doc.state().field(id).unwrap().src.as_ref().unwrap().id;
    let props = o
        .doc
        .state()
        .field(id)
        .unwrap()
        .term
        .as_ref()
        .unwrap()
        .props
        .clone();
    assert!(props.has_opt);
    let w = widgets(&o.doc, name);
    let index_of = |o: &Opened, aid: AnnotId| {
        let st = o.doc.state();
        st.annot(aid).and_then(|r| r.widget()).unwrap().props.index as usize
    };
    let second = *w
        .iter()
        .find(|&&aid| index_of(&o, aid) == 1)
        .expect("its second widget");
    let change = group(&o.doc, name).press(second).unwrap();
    assert_eq!(change.value, state("1"));
    assert_eq!(change.export.as_deref(), Some(&*props.options[1].export));
    assert!(
        change
            .export
            .as_deref()
            .unwrap()
            .starts_with("You paid your use tax"),
        "{:?}",
        change.export.as_ref().map(String::len)
    );
    let theirs = family_group(&o, object).expect("the family's group");
    let want = family_pressed(&theirs, &model_shows(&o, name).0, 1);
    fx::press(&mut o.doc, second);
    assert_eq!(model_shows(&o, name), want);
}
