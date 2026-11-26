//! Fields' values set (M10 slice 9): [`ResolvedOp::SetFieldValues`]
//! lowered a set at a time, `values` then `calculated`, each against the
//! state the sets before it left, and each to every terminal its reference
//! names. A set holds its field to the rules of its kind:
//!
//! - **a text field** takes a text or none, composed (NFC). A user's (one
//!   of `values`) is held to the field's `/MaxLen`, counted in grapheme
//!   clusters (a character and the marks on it, as a comb field's cell
//!   holds one), where it's above 0, as PDFium holds it; and to one line,
//!   where the field isn't multiline: no `\r` or `\n`, which the maker
//!   drops from a single line. Either is refused, never cut;
//! - **a combo box** takes one of its options, by its export value, or,
//!   where it's editable (`/Ff` Edit), any text, composed; **a list box**
//!   takes its options alone: one, or several where it's multiple-select.
//!   An option is matched as its file writes it, else as both compose, and
//!   the value holds the option's own text; a list box's several are in
//!   the order of its options, each once;
//! - **a check box or radio group** takes a state by name (one an on state
//!   goes under, or `Off`), an export value, or a press of one of its
//!   widgets, by the group's rules ([`crate::forms::group`]): the field's
//!   value, and the state of each widget whose state moves. None is `Off`;
//! - a push button, a signature field, and a field of no kind sheaf knows
//!   hold no value: refused.
//!
//! Before its kind's rules, a set of a field that's locked is refused,
//! [`EditError::Locked`], from either list: a signature field holding a
//! signature, or a field of the primary file's own instance whose full
//! name a signature of that file locks (FieldMDP, or a field's `/Lock`),
//! as its signatures say, unchecked. After the put, each of the field's
//! widgets whose appearance a save makes anew is made as the emitter will
//! make it: a character no font sheaf has draws refuses the set, named
//! (right-to-left scripts, CJK and emoji among them); any other reason a
//! widget's appearance isn't made doesn't.
//!
//! A user's set of a read-only field is refused. A calculated one isn't (a
//! calculated field usually is read-only), nor held to `/MaxLen` or one
//! line: the form's own script set it. `display`, what a text field's or
//! a combo box's widgets show in its value's place, is put in
//! [`Terminal::display`], composed, and dropped by a set that gives none;
//! another kind's is refused. A set whose value means what the field's
//! does ([`set_alike`]: the compare a save asks, `values_alike`, but a
//! choice's texts as a set; a text field's none and empty text alike, a
//! group's none and `Off` alike, its widgets' states as they are)
//! and whose display is the field's puts nothing. A set refused refuses
//! its op, which changes nothing (the transaction's rule). No refusal's
//! words hold a value.
//!
//! [`ResolvedOp::SetFieldValues`]: super::ResolvedOp::SetFieldValues

use std::sync::Arc;

use unicode_normalization::UnicodeNormalization;
use unicode_segmentation::UnicodeSegmentation;

use super::resolve::{Set, SetValue};
use super::{EditError, Lower};
use crate::forms::appearance;
use crate::forms::field::{Choice, FieldKind, Value, flags, set_alike};
use crate::forms::{FieldId, group};
use crate::model::{AnnotBody, AnnotRec, FieldNode, FormInst, Terminal, Widget};
use crate::pdf::forms::Unmade;
use crate::prim::{Key, Val};

/// Who a set's value is by: the user (`values`), or the form's
/// calculations (`calculated`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum By {
    User,
    Calculation,
}

/// Each of `values`, then each of `calculated`, set on every terminal it
/// names, in order: all of them, or the first refusal.
pub(crate) fn set_values(
    lw: &mut Lower,
    values: &[Set],
    calculated: &[Set],
) -> Result<(), EditError> {
    let sets = (values.iter().map(|s| (s, By::User)))
        .chain(calculated.iter().map(|s| (s, By::Calculation)));
    for (set, by) in sets {
        for &field in &set.fields {
            set_field(lw, field, set, by)?;
        }
    }
    Ok(())
}

/// `set`, by `by`, on terminal `field`, by the rules of its kind.
fn set_field(lw: &mut Lower, field: FieldId, set: &Set, by: By) -> Result<(), EditError> {
    lw.read(Key::Field(field));
    let name = named(lw, field);
    let Some(node) = lw.rec.field(field).cloned() else {
        return Err(lw.not_found(format!("field {}", field.0)));
    };
    let Some(term) = node.term.clone() else {
        return Err(lw.invalid(format!("{name} holds no value: it's a parent of fields")));
    };
    if locked(lw, &node, &term, field) {
        return Err(EditError::Locked {
            op: lw.op,
            field: lw.rec.full_name(field).unwrap_or_default(),
        });
    }
    if by == By::User && term.flags & flags::READ_ONLY != 0 {
        return Err(lw.invalid(format!("{name} is read-only")));
    }
    let display = match (&set.display, term.kind) {
        (None, _) => None,
        (Some(d), FieldKind::Text | FieldKind::Combo) => Some(Arc::from(composed(d))),
        (Some(_), _) => {
            return Err(lw.invalid(format!(
                "{name} shows its value as it is: only a text field or a combo box shows a display"
            )));
        }
    };
    let value = match term.kind {
        FieldKind::Text => text(lw, &name, &term, &set.value, by)?,
        FieldKind::Combo => combo(lw, &name, &term, &set.value)?,
        FieldKind::List => list(lw, &name, &term, &set.value)?,
        FieldKind::CheckBox | FieldKind::Radio => {
            return buttons(lw, field, &name, &term, &set.value);
        }
        FieldKind::PushButton => {
            return Err(lw.invalid(format!("{name} is a push button: it holds no value")));
        }
        FieldKind::Signature => {
            return Err(lw.invalid(format!(
                "{name} is a signature field: it's signed, not filled"
            )));
        }
        FieldKind::Unknown => {
            return Err(lw.invalid(format!("{name} is of no kind of field sheaf can fill")));
        }
    };
    if put(lw, field, &node, &term, value, display) {
        drawn(lw, field, &name)?;
    }
    Ok(())
}

/// Whether field `field`, `node` holding `term`, is locked: a signature
/// field holding a signature, or a field of the primary file's own instance
/// whose full name a signature of that file locks (its FieldMDP, or its
/// field's `/Lock`), as the file's signatures say: not checked against
/// their digests, since lowering hashes no file and a broken signature's
/// lock still says what its signer locked. Another instance's fields (an
/// inserted file's, a duplicate's copies) aren't the signed file's.
fn locked(lw: &Lower, node: &FieldNode, term: &Terminal, field: FieldId) -> bool {
    if term.kind == FieldKind::Signature && term.value == Some(Value::Signed) {
        return true;
    }
    let primary = &lw.cx.primary;
    let own = FormInst {
        src: primary.id(),
        copy: 0,
    };
    let signed = &primary.signatures().signed;
    if node.inst != own || signed.iter().all(|s| s.locks.is_empty()) {
        return false;
    }
    let full = lw.rec.full_name(field).unwrap_or_default();
    (signed.iter().flat_map(|s| &s.locks)).any(|lock| lock.locks(&full))
}

/// Whether the widgets of field `field`, `name` as a refusal names it, can
/// show what it now holds: each whose appearance a save makes anew
/// ([`appearance::stale`]) made here as the emitter makes it, its look in
/// the font its `/DA` names in its source and the fallback for what that
/// can't draw. A character neither draws ([`Unmade::Unencodable`]) refuses
/// the set, naming that character. Any other reason one isn't made (no
/// box, a font that can't be read, a list box's own option no font draws)
/// doesn't: the widget is saved with no appearance. A check box's or radio
/// button's is never made here: its mark is ZapfDingbats', from its file's
/// caption, and its maker refuses no character.
fn drawn(lw: &mut Lower, field: FieldId, name: &str) -> Result<(), EditError> {
    let Some(term) = lw.rec.field(field).and_then(|n| n.term.clone()) else {
        return Ok(());
    };
    for &aid in &term.widgets {
        lw.read(Key::Annot(aid));
        if !appearance::stale(&lw.rec, aid) {
            continue;
        }
        let Some(src) = (lw.rec.annot(aid).and_then(AnnotRec::widget))
            .and_then(|w| w.src.as_ref())
            .map(|r| r.src.clone())
        else {
            continue;
        };
        let Ok(look) = appearance::look(&lw.rec, aid) else {
            continue;
        };
        if let Err(Unmade::Unencodable(c)) = appearance::make_in(&look, &src) {
            let c = c.character;
            return Err(lw.invalid(format!(
                "“{c}” (U+{:04X}) can't be drawn in any font sheaf has, so {name} can't show it",
                c as u32
            )));
        }
    }
    Ok(())
}

/// Field `field` as a refusal names it: “its full name”, or its id where it
/// has none.
fn named(lw: &Lower, field: FieldId) -> String {
    match lw.rec.full_name(field).filter(|n| !n.is_empty()) {
        Some(name) => format!("“{name}”"),
        None => format!("field {}", field.0),
    }
}

/// `text` composed: Normalization Form C.
fn composed(text: &str) -> String {
    text.nfc().collect()
}

/// A text field's value for `value`, by `by`: its text, composed, or none.
fn text(
    lw: &Lower,
    name: &str,
    term: &Terminal,
    value: &SetValue,
    by: By,
) -> Result<Option<Value>, EditError> {
    let text = match value {
        SetValue::Clear => return Ok(None),
        SetValue::Text(text) => composed(text),
        _ => return Err(lw.invalid(format!("{name} is a text field: it takes text, or none"))),
    };
    if by == By::User {
        if term.flags & flags::MULTILINE == 0 && text.contains(['\r', '\n']) {
            return Err(lw.invalid(format!("{name} is one line: its value can't break lines")));
        }
        if let Some(max) = term.props.max_len.filter(|&max| max > 0) {
            let n = text.graphemes(true).count();
            if n > max as usize {
                return Err(lw.invalid(format!(
                    "{name} takes at most {max} characters, and the value has {n}"
                )));
            }
        }
    }
    Ok(Some(Value::Text(text.into())))
}

/// A combo box's value for `value`: one of its options' export value, or,
/// where it's editable, any text, composed; or none.
fn combo(
    lw: &Lower,
    name: &str,
    term: &Terminal,
    value: &SetValue,
) -> Result<Option<Value>, EditError> {
    let text = match value {
        SetValue::Clear => return Ok(None),
        SetValue::Text(text) => text,
        _ => {
            return Err(lw.invalid(format!("{name} is a combo box: it takes one text, or none")));
        }
    };
    let options = &term.props.options;
    match option(options, text) {
        Some(i) => Ok(Some(Value::Text(options[i].export.clone()))),
        None if term.flags & flags::EDIT != 0 => Ok(Some(Value::Text(composed(text).into()))),
        None => Err(lw.invalid(format!(
            "{name} takes one of its options, and the value isn't one: it can't be typed in"
        ))),
    }
}

/// A list box's value for `value`: the export values of the options it
/// names, in their order, each once: none for none, a text for one, and
/// several where the list box is multiple-select.
fn list(
    lw: &Lower,
    name: &str,
    term: &Terminal,
    value: &SetValue,
) -> Result<Option<Value>, EditError> {
    let given: Vec<&str> = match value {
        SetValue::Clear => Vec::new(),
        SetValue::Text(text) => vec![text],
        SetValue::Items(items) => items.iter().map(String::as_str).collect(),
        _ => {
            return Err(lw.invalid(format!(
                "{name} is a list box: it takes its options, or none"
            )));
        }
    };
    let options = &term.props.options;
    let mut picked = Vec::with_capacity(given.len());
    for (n, text) in given.iter().enumerate() {
        match option(options, text) {
            Some(i) => picked.push(i),
            None => {
                return Err(lw.invalid(format!(
                    "{name} takes its options alone, and value {} of {} isn't one",
                    n + 1,
                    given.len()
                )));
            }
        }
    }
    picked.sort_unstable();
    let mut exports: Vec<Arc<str>> = Vec::with_capacity(picked.len());
    for i in picked {
        let export = &options[i].export;
        if !exports.contains(export) {
            exports.push(export.clone());
        }
    }
    match exports.len() {
        0 => Ok(None),
        1 => Ok(exports.pop().map(Value::Text)),
        _ if term.flags & flags::MULTI_SELECT != 0 => Ok(Some(Value::Items(exports.into()))),
        n => Err(lw.invalid(format!(
            "{name} takes one option, and the value names {n}: it isn't multiple-select"
        ))),
    }
}

/// The index of the first of `options` whose export value is `text` as
/// written, else as both compose.
fn option(options: &[Choice], text: &str) -> Option<usize> {
    (options.iter().position(|o| *o.export == *text)).or_else(|| {
        let text = composed(text);
        (options.iter()).position(|o| o.export.nfc().eq(text.chars()))
    })
}

/// A check box's or radio group's `value` set on field `field`, by the
/// group's rules: the field's value where it moves (none and `Off` alike),
/// and each widget whose state moves.
fn buttons(
    lw: &mut Lower,
    field: FieldId,
    name: &str,
    term: &Terminal,
    value: &SetValue,
) -> Result<(), EditError> {
    let kind = match term.kind {
        FieldKind::Radio => "a radio group",
        _ => "a check box",
    };
    for &aid in &term.widgets {
        lw.read(Key::Annot(aid));
    }
    let Some(group) = group::buttons(&lw.rec, field) else {
        return Err(EditError::internal(format!(
            "field {} is no group",
            field.0
        )));
    };
    let change = match value {
        SetValue::Clear => group.set_state("Off"),
        SetValue::State(state) if state == "Off" || group.has_on_state(state) => {
            group.set_state(state)
        }
        SetValue::State(_) => {
            return Err(lw.invalid(format!(
                "{name} has no on state of that name: its widgets' states name the ones it has"
            )));
        }
        SetValue::Export(export) => group
            .set_export(export)
            .ok_or_else(|| lw.invalid(format!("{name} has no widget of that export value")))?,
        SetValue::Press(widget) => group.press(*widget).ok_or_else(|| {
            lw.invalid(format!("annotation {} isn't a widget of {name}", widget.0))
        })?,
        SetValue::Text(_) | SetValue::Items(_) => {
            return Err(lw.invalid(format!(
                "{name} is {kind}: it takes a state, an export value or a press"
            )));
        }
    };
    if !set_alike(term.kind, term.value.as_ref(), Some(&change.value)) {
        let node = FieldNode::clone(lw.rec.field(field).expect("the field"));
        let term = Terminal {
            value: Some(change.value.clone()),
            ..Terminal::clone(term)
        };
        put_term(lw, field, node, term);
    }
    for (aid, state) in &change.states {
        let rec = lw.rec.annot(*aid).expect("a widget of the group's");
        let w = Widget {
            state: Some(state.to_string()),
            ..Widget::clone(rec.widget().expect("a widget"))
        };
        let rec = AnnotRec {
            page: rec.page,
            body: AnnotBody::Widget(Arc::new(w)),
        };
        lw.rec.put(Key::Annot(*aid), Some(Val::Annot(rec)));
    }
    Ok(())
}

/// Field `field`'s terminal `term` given `value` and `display`, unless the
/// value is alike the one it holds ([`set_alike`]) and the display the
/// same: then
/// nothing is put. A value alike stays as the field holds it. Whether it
/// put one.
fn put(
    lw: &mut Lower,
    field: FieldId,
    node: &FieldNode,
    term: &Terminal,
    value: Option<Value>,
    display: Option<Arc<str>>,
) -> bool {
    let kept = set_alike(term.kind, term.value.as_ref(), value.as_ref());
    if kept && term.display == display {
        return false;
    }
    let term = Terminal {
        value: if kept { term.value.clone() } else { value },
        display,
        ..Terminal::clone(term)
    };
    put_term(lw, field, FieldNode::clone(node), term);
    true
}

/// `node`, field `field`, holding `term`.
fn put_term(lw: &mut Lower, field: FieldId, node: FieldNode, term: Terminal) {
    let node = FieldNode {
        term: Some(Arc::new(term)),
        ..node
    };
    lw.rec
        .put(Key::Field(field), Some(Val::Field(Arc::new(node))));
}
