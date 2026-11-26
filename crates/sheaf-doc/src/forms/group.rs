//! A check box or radio field's group (M10 slice 8): its widgets as the
//! model holds them, read into the family's plain data
//! ([`crate::pdf::forms::group`]), and what pressing one, or setting the
//! field's value, puts ([`Change`]): each widget's state (`/AS`) that
//! changes, the field's value, and its export value. The rules are the
//! family's: radio buttons, NoToggleToOff, RadiosInUnison, check boxes of
//! one on state going together and check boxes of distinct on states
//! acting as radio buttons. Which widgets there are, their states and
//! their names are the model's. `SetFieldValues`' lowering puts a
//! [`Change`] (`crate::tx::fill`), which a test's press and state go
//! through too (`crate::tx::fixture::press` and `set_buttons`).
//!
//! **By index.** A group's widgets go by their places among their field's
//! widgets in its file ([`WidgetProps::index`](crate::model::WidgetProps::index)),
//! which the field's `/Opt` entries and an on state made under `/Opt` go
//! by. An index the model holds no widget at (one on a page template,
//! which the model doesn't hold, or one a page delete took) keeps the on
//! state its file gives it, which the field's record holds for every
//! index ([`FieldProps::on_states`](crate::model::FieldProps::on_states)),
//! so the rules name it as its file does; it shows no state, and nothing
//! is put for it. An index two of
//! the model's widgets share (a page the page tree lists twice) puts both.
//! A duplicate's copy of a field is a group of its own, of the copy's
//! widgets.
//!
//! **Names** are held as the model holds them, a character a byte
//! (Latin-1), and handed to the rules as those bytes. `Debug` prints
//! counts and flags, never a name or an export value.

use std::borrow::Cow;
use std::fmt;
use std::sync::Arc;

use crate::annot::AnnotId;
use crate::model::{DocState, Terminal, Widget};
use crate::pdf::forms::group::{Group, GroupKind, GroupWidget, on_state_name};

use super::FieldId;
use super::field::{FieldKind, Value, flags};

/// A check box or radio field's widgets as the group's rules take them:
/// its kind and flags, whether it has `/Opt`, and by index each widget's
/// on state and export value, the model's widgets at it and the state each
/// shows.
#[derive(Clone)]
pub struct Buttons {
    kind: GroupKind,
    no_toggle_to_off: bool,
    in_unison: bool,
    has_opt: bool,
    /// Each index's on state and export value, as the rules take them.
    members: Vec<GroupWidget<'static>>,
    /// The model's widgets at each index, with the state (`/AS`) each
    /// shows.
    shown: Vec<Vec<(AnnotId, Option<String>)>>,
}

/// What pressing a widget of a group, or setting its field's value, puts:
/// the field's value, and each of the model's widgets whose state changes.
#[derive(Clone, PartialEq)]
pub struct Change {
    /// The field's value (`/V`): the name of the first on widget's on
    /// state, `Off` where none is on.
    pub value: Value,
    /// Each widget whose state (`/AS`) changes, and its state after: its
    /// on state's name, or `Off`. A widget that shows no state is one
    /// whose state changes.
    pub states: Vec<(AnnotId, Arc<str>)>,
    /// The field's export value after: the first on widget's, its `/Opt`
    /// entry or else its on state's name; none where none is on. What data
    /// export writes (M10 slice 16).
    pub export: Option<String>,
}

/// Field `field`'s group as `st` holds it: none unless it's a check box or
/// radio field's terminal.
pub fn buttons(st: &DocState, field: FieldId) -> Option<Buttons> {
    let term = st.field(field)?.term.as_deref()?;
    let kind = match term.kind {
        FieldKind::CheckBox => GroupKind::CheckBox,
        FieldKind::Radio => GroupKind::Radio,
        _ => return None,
    };
    // Every index its file has, by the on state its file gives it; then
    // the model's widgets, each at its own.
    let member = |on_state: Option<&str>| {
        let mut member = GroupWidget::new(None);
        member.on_state = on_state.and_then(name_bytes).map(Cow::Owned);
        member
    };
    let mut members: Vec<GroupWidget<'static>> = (term.props.on_states.iter())
        .map(|on_state| member(on_state.as_deref()))
        .collect();
    let mut shown: Vec<Vec<(AnnotId, Option<String>)>> = vec![Vec::new(); members.len()];
    for &aid in &term.widgets {
        let Some(w) = st.annot(aid).and_then(|rec| rec.widget()) else {
            continue;
        };
        let index = w.props.index as usize;
        if index >= members.len() {
            members.resize_with(index + 1, || GroupWidget::new(None));
            shown.resize_with(index + 1, Vec::new);
            members[index] = member(file_on_state(term, w));
        }
        shown[index].push((aid, w.state.clone()));
    }
    let options = &term.props.options;
    for (member, option) in members.iter_mut().zip(options.iter()) {
        member.export = Some(Cow::Owned(option.export.to_string()));
    }
    Some(Buttons {
        kind,
        no_toggle_to_off: term.flags & flags::NO_TOGGLE_TO_OFF != 0,
        in_unison: term.flags & flags::RADIOS_IN_UNISON != 0,
        has_opt: term.props.has_opt,
        members,
        shown,
    })
}

/// The on state widget `w`'s file gives it, `term` its field's terminal:
/// its field's record's for its index, and its own where the record has
/// none for it (a widget its file's field didn't list).
fn file_on_state<'a>(term: &'a Terminal, w: &'a Widget) -> Option<&'a str> {
    match term.props.on_states.get(w.props.index as usize) {
        Some(on_state) => on_state.as_deref(),
        None => w.props.on_state.as_deref(),
    }
}

/// The name widget `w`'s on state goes under, `term` its field's terminal,
/// a check box or radio field's: its file's, else its index under `/Opt`,
/// else `Yes` ([`on_state_name`]), as a made appearance names it and as a
/// press puts it in `/AS`. What [`Buttons::on_state`] gives for it, from
/// the widget's and the field's records alone: no group is built, so one
/// widget's name costs nothing of its field's other widgets.
pub(crate) fn on_state(term: &Terminal, w: &Widget) -> Vec<u8> {
    let own = file_on_state(term, w).and_then(name_bytes);
    on_state_name(own.as_deref(), term.props.has_opt, w.props.index as usize).into_owned()
}

impl Buttons {
    /// What pressing `widget` puts: by the rules, an off widget turns on,
    /// with those in unison with it, and every other off; an on one turns
    /// off, but for a radio button where the field has NoToggleToOff. None
    /// where `widget` isn't one of the group's.
    pub fn press(&self, widget: AnnotId) -> Option<Change> {
        let index = self.index_of(widget)?;
        let group = self.group();
        let after = group.press(&self.states(&group), index);
        Some(self.change(&group, &after))
    }

    /// What setting the field's value to the state `name` puts: on, each
    /// widget whose on state is named so (in unison every such widget,
    /// out of it the first), and every other off. `Off`, or a name no
    /// widget's on state has, turns every widget off.
    pub fn set_state(&self, name: &str) -> Change {
        let group = self.group();
        let after = match name_bytes(name) {
            Some(name) => group.states_for(&name),
            None => vec![false; self.members.len()],
        };
        self.change(&group, &after)
    }

    /// What setting the field's value by the export value `export` puts:
    /// the first widget whose export value it is pressed from every widget
    /// off. None where no widget's export value is `export`.
    pub fn set_export(&self, export: &str) -> Option<Change> {
        let group = self.group();
        let after = group.states_for_export(export)?;
        Some(self.change(&group, &after))
    }

    /// Whether `name` is the name an on state goes under, of a widget at
    /// some index ([`on_state`](Self::on_state), an index the model holds
    /// no widget at among them): one [`set_state`](Self::set_state) turns
    /// on. `Off` is none's.
    pub fn has_on_state(&self, name: &str) -> bool {
        let Some(name) = name_bytes(name).filter(|n| n != b"Off") else {
            return false;
        };
        let group = self.group();
        (0..self.members.len()).any(|index| group.on_state(index).is_some_and(|on| *on == *name))
    }

    /// The field's export value as its widgets show it now: the first on
    /// widget's; none where none is on.
    pub fn export(&self) -> Option<String> {
        let group = self.group();
        let states = self.states(&group);
        group.export(&states).map(Cow::into_owned)
    }

    /// The name `widget`'s on state goes under: its normal appearance's,
    /// else its index under `/Opt`, else `Yes` ([`on_state_name`]), as a
    /// made appearance names it and as a press puts it in `/AS`. None
    /// where `widget` isn't one of the group's.
    pub fn on_state(&self, widget: AnnotId) -> Option<Vec<u8>> {
        let index = self.index_of(widget)?;
        self.group().on_state(index).map(Cow::into_owned)
    }

    /// The group the rules take.
    fn group(&self) -> Group<'_> {
        let mut group = Group::new(self.kind, &self.members);
        group.no_toggle_to_off = self.no_toggle_to_off;
        group.in_unison = self.in_unison;
        group.has_opt = self.has_opt;
        group
    }

    /// The index `widget` is at.
    fn index_of(&self, widget: AnnotId) -> Option<usize> {
        (self.shown.iter()).position(|at| at.iter().any(|&(aid, _)| aid == widget))
    }

    /// Each index on or off as its widgets show it: on where one of them
    /// shows its on state's name as the group names it, so a widget with
    /// no appearance yet is on where its state names the one its
    /// appearance would be made under (the family's `FieldGroup::states`).
    /// An index the model holds no widget at is off: what its file's
    /// widget shows isn't read.
    fn states(&self, group: &Group<'_>) -> Vec<bool> {
        (self.shown.iter().enumerate())
            .map(|(index, at)| {
                group.on_state(index).is_some_and(|on| {
                    let shows = |state: &Option<String>| {
                        state.as_deref().and_then(name_bytes).as_deref() == Some(&*on)
                    };
                    at.iter().any(|(_, state)| shows(state))
                })
            })
            .collect()
    }

    /// What `after`, each index on or off, puts.
    fn change(&self, group: &Group<'_>, after: &[bool]) -> Change {
        let mut states = Vec::new();
        for (index, at) in self.shown.iter().enumerate() {
            let on = after.get(index).copied().unwrap_or(false);
            let name = group.state_name(index, on);
            for (aid, was) in at {
                if was.as_deref().and_then(name_bytes).as_deref() != Some(&*name) {
                    states.push((*aid, name_text(&name).into()));
                }
            }
        }
        Change {
            value: Value::State(name_text(&group.value(after)).into()),
            states,
            export: group.export(after).map(Cow::into_owned),
        }
    }
}

impl fmt::Debug for Buttons {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let widgets: usize = self.shown.iter().map(Vec::len).sum();
        let holes = self.shown.iter().filter(|at| at.is_empty()).count();
        f.debug_struct("Buttons")
            .field("kind", &self.kind)
            .field("no_toggle_to_off", &self.no_toggle_to_off)
            .field("in_unison", &self.in_unison)
            .field("has_opt", &self.has_opt)
            .field("indexes", &self.members.len())
            .field("widgets", &widgets)
            .field("holes", &holes)
            .finish()
    }
}

impl fmt::Debug for Change {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Change")
            .field("states", &self.states.len())
            .field("has_export", &self.export.is_some())
            .finish_non_exhaustive()
    }
}

/// A name the model holds (Latin-1, a character a byte) as its bytes;
/// none for text that names nothing, with a character past U+00FF.
pub(crate) fn name_bytes(name: &str) -> Option<Vec<u8>> {
    name.chars()
        .map(|c| u8::try_from(u32::from(c)).ok())
        .collect()
}

/// A name's bytes as the model holds them, a character a byte.
fn name_text(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| char::from(b)).collect()
}

#[cfg(test)]
mod tests;
