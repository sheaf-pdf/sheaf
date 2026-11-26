//! The field model's types (M10 proposal 8): a form's terminal fields and
//! their widgets as [`super::read`] reads them from a file. `DocState`
//! holds a form as records: `FieldNode`s, their `Terminal`s, and `Widget`
//! annotations on their pages.
//!
//! A field has a sheaf id; its full name is data. Names from the file
//! (`/AS` states, `/DR` fonts) are kept as Latin-1 text, one character per
//! byte, so nothing is lost and they compare as the bytes did.

use std::sync::Arc;

use crate::model::{FieldNode, Terminal};
use crate::pdf::ObjectId;

pub use sheaf_edit::FieldId;

/// What a field is, from `/FT` and `/Ff`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum FieldKind {
    Text,
    CheckBox,
    Radio,
    PushButton,
    /// A combo box (a drop-down list), editable or not.
    Combo,
    /// A list box.
    List,
    Signature,
    /// No `/FT`, or one this build doesn't know: not fillable.
    Unknown,
}

impl FieldKind {
    /// From `/FT` and `/Ff`, both inherited.
    pub fn of(ft: &[u8], flags: u32) -> FieldKind {
        match ft {
            b"Tx" => FieldKind::Text,
            b"Btn" if flags & flags::PUSH_BUTTON != 0 => FieldKind::PushButton,
            b"Btn" if flags & flags::RADIO != 0 => FieldKind::Radio,
            b"Btn" => FieldKind::CheckBox,
            b"Ch" if flags & flags::COMBO != 0 => FieldKind::Combo,
            b"Ch" => FieldKind::List,
            b"Sig" => FieldKind::Signature,
            _ => FieldKind::Unknown,
        }
    }

    /// The manifest's name for it (`text`, `check`, `radio`, `push`,
    /// `choice`, `sig`), and `other` for [`FieldKind::Unknown`].
    pub fn short(self) -> &'static str {
        match self {
            FieldKind::Text => "text",
            FieldKind::CheckBox => "check",
            FieldKind::Radio => "radio",
            FieldKind::PushButton => "push",
            FieldKind::Combo | FieldKind::List => "choice",
            FieldKind::Signature => "sig",
            FieldKind::Unknown => "other",
        }
    }
}

/// `/Ff` bits.
pub mod flags {
    pub const READ_ONLY: u32 = 1;
    pub const REQUIRED: u32 = 1 << 1;
    pub const NO_EXPORT: u32 = 1 << 2;
    pub const MULTILINE: u32 = 1 << 12;
    pub const PASSWORD: u32 = 1 << 13;
    pub const NO_TOGGLE_TO_OFF: u32 = 1 << 14;
    pub const RADIO: u32 = 1 << 15;
    pub const PUSH_BUTTON: u32 = 1 << 16;
    pub const COMBO: u32 = 1 << 17;
    pub const EDIT: u32 = 1 << 18;
    pub const FILE_SELECT: u32 = 1 << 20;
    pub const MULTI_SELECT: u32 = 1 << 21;
    pub const DO_NOT_SCROLL: u32 = 1 << 23;
    pub const COMB: u32 = 1 << 24;
    pub const RADIOS_IN_UNISON: u32 = 1 << 25;
    pub const COMMIT_ON_SEL_CHANGE: u32 = 1 << 26;
}

/// A field's value (`/V`) or default (`/DV`).
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// A text or choice field's text.
    Text(Arc<str>),
    /// A check box's or radio group's state: the name of the appearance
    /// its widgets show (`Off` for none).
    State(Arc<str>),
    /// A list box's selection, when `/V` is an array.
    Items(Arc<[Arc<str>]>),
    /// A signature field holding a signature.
    Signed,
}

/// Whether terminal `term`, field `node`'s, holds another value than its
/// source's field as read (M10 slice 9), as a field of its kind shows one
/// ([`values_alike`]); or the field was read from no source, or isn't
/// among its source's fields as read. Where it does, a save writes the
/// model's `/V` (the emitter's `read_from`), a text field's or choice
/// field's widgets have their appearances made anew
/// ([`appearance::stale`](super::appearance::stale)), and an overlay of a
/// page with one of its widgets isn't the base page
/// ([`untouched`](crate::realize::untouched)). A map lookup and a compare
/// of two values.
pub fn value_changed(node: &FieldNode, term: &Terminal) -> bool {
    let Some(src) = &node.src else {
        return true;
    };
    match src.src.form().field_by_object(src.id) {
        Some(read) => !values_alike(term.kind, read.value.as_ref(), term.value.as_ref()),
        None => true,
    }
}

/// Whether `a` and `b`, values of a field of `kind`, show the same: a text
/// field's text (`text_shown`: none and an empty one alike); a combo box's
/// or list box's texts, in order (`choice_texts`: none and an empty array
/// alike, a text and an array of it alike); a check box's or
/// radio group's state, none and `Off` alike; any other kind's value as it
/// is.
///
/// The one compare of the value a field holds with another: a save writes
/// the model's, and makes its appearances, where it isn't alike its
/// file's ([`value_changed`]), and lowering puts one where it isn't alike
/// the one the field holds ([`set_alike`], which is this but for a
/// choice's order).
pub fn values_alike(kind: FieldKind, a: Option<&Value>, b: Option<&Value>) -> bool {
    fn state(v: Option<&Value>) -> Option<&Value> {
        match v {
            Some(Value::State(s)) if &**s == "Off" => None,
            v => v,
        }
    }
    match kind {
        FieldKind::Text => text_shown(a) == text_shown(b),
        FieldKind::Combo | FieldKind::List => choice_texts(a) == choice_texts(b),
        FieldKind::CheckBox | FieldKind::Radio => state(a) == state(b),
        FieldKind::PushButton | FieldKind::Signature | FieldKind::Unknown => a == b,
    }
}

/// Whether setting a field of `kind` that holds `held` to `set` would
/// leave what it holds, so that lowering puts nothing (`tx::fill`):
/// [`values_alike`], and for a choice its texts as a set, whatever their
/// order and each once. A choice selects the options its texts name,
/// which no order changes, and an op's are in the options' order where
/// the file's may be in any: an op that chose what the field holds would
/// else put the same selection in another order, a value a save writes
/// and calls replaced. So the field keeps its file's order, and
/// [`values_alike`], which a save asks of the value kept, stays the
/// stricter of the two: what this calls alike and that doesn't is never
/// put.
pub(crate) fn set_alike(kind: FieldKind, held: Option<&Value>, set: Option<&Value>) -> bool {
    let texts = |v| {
        let mut texts = choice_texts(v);
        texts.sort_unstable();
        texts.dedup();
        texts
    };
    match kind {
        FieldKind::Combo | FieldKind::List => texts(held) == texts(set),
        _ => values_alike(kind, held, set),
    }
}

/// The text a text field's value shows: none, and any value that's no
/// text, the empty one.
pub(crate) fn text_shown(value: Option<&Value>) -> &str {
    match value {
        Some(Value::Text(text)) => text,
        _ => "",
    }
}

/// The texts a choice field's value holds, as the family takes them
/// (`field_choice`): a text is one, even an empty one, which selects an
/// option whose export value is empty; an array its texts; none, no value
/// and anything else.
pub(crate) fn choice_texts(value: Option<&Value>) -> Vec<&str> {
    match value {
        Some(Value::Text(text)) => vec![&**text],
        Some(Value::Items(items)) => items.iter().map(|t| &**t).collect(),
        _ => Vec::new(),
    }
}

/// One of a choice field's items, or a check box's or radio button's
/// export value: `/Opt`.
#[derive(Debug, Clone, PartialEq)]
pub struct Choice {
    /// What `/V` holds when it's picked.
    pub export: Arc<str>,
    /// What the list shows (the same, unless `/Opt` gives both).
    pub shown: Arc<str>,
}

/// When an action runs: the key it's under in `/AA`, or `/A`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Trigger {
    /// `/A`: the widget is clicked (a button's action).
    Activate,
    /// `/AA /K`, on each change to the value.
    Keystroke,
    /// `/AA /F`, to show the value.
    Format,
    /// `/AA /V`, when the value is committed.
    Validate,
    /// `/AA /C`, when another field changes.
    Calculate,
    /// `/AA /E` and `/X`: the pointer comes in and leaves.
    Enter,
    Exit,
    /// `/AA /D` and `/U`: the mouse button goes down and up.
    Down,
    Up,
    /// `/AA /Fo` and `/Bl`: the widget gains and loses focus.
    Focus,
    Blur,
}

impl Trigger {
    /// A field's own triggers.
    pub const FIELD: [(&'static [u8], Trigger); 4] = [
        (b"K", Trigger::Keystroke),
        (b"F", Trigger::Format),
        (b"V", Trigger::Validate),
        (b"C", Trigger::Calculate),
    ];
    /// A widget's.
    pub const WIDGET: [(&'static [u8], Trigger); 6] = [
        (b"E", Trigger::Enter),
        (b"X", Trigger::Exit),
        (b"D", Trigger::Down),
        (b"U", Trigger::Up),
        (b"Fo", Trigger::Focus),
        (b"Bl", Trigger::Blur),
    ];
}

/// An action, as read: what kind it is, and a JavaScript action's script.
/// Nothing runs it yet (slices 13 and 18–22).
#[derive(Debug, Clone, PartialEq)]
pub struct Action {
    /// `/S`: `JavaScript`, `ResetForm`, `SubmitForm`, `Named`, `URI`…
    pub kind: String,
    /// `/JS`, for a JavaScript action.
    pub script: Option<Arc<str>>,
}

impl Action {
    /// A JavaScript action with something in it.
    pub fn has_script(&self) -> bool {
        self.script.as_deref().is_some_and(|s| !s.trim().is_empty())
    }
}

/// A terminal field: the one that holds a value, with inherited keys
/// resolved.
#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    pub id: FieldId,
    /// Its dictionary in its file (the widget's, when the two are merged).
    pub object: ObjectId,
    /// The `/T`s from the root down, joined by dots.
    pub name: String,
    pub kind: FieldKind,
    /// `/Ff` (see [`flags`]).
    pub flags: u32,
    pub value: Option<Value>,
    /// `/DV`: what Reset Form puts back.
    pub default: Option<Value>,
    /// `/DA`, its own or a parent field's. The form's is
    /// [`super::FormFacts::da`].
    pub da: Option<Arc<str>>,
    /// `/Q`: 0 left, 1 centered, 2 right; its own or a parent field's.
    pub align: Option<u8>,
    /// `/MaxLen`, as the file has it. Layout uses at most
    /// [`super::read::MAX_LAYOUT_LEN`] of it.
    pub max_len: Option<u32>,
    /// `/TI`: the index in [`options`](Self::options) of the first option
    /// a list box shows at its top, its own or a parent field's; 0 where
    /// it has none, or gives no integer within `u32` (less than 0 too), as
    /// the family reads it.
    pub top: u32,
    /// `/Opt`: a choice field's items, or a check box's or radio group's
    /// export values (one per widget, by its [`index`](Widget::index)).
    /// Shared with the fields that inherit it. Each keeps every item's
    /// place, as the family and PDFium read one, so an index into the
    /// file's `/Opt` (`/TI`, `/I`) is one into it: an item that is no
    /// text an option of empty text. A choice field's text is a string's
    /// or a stream's, an item an array that starts with two of them; a
    /// check box's or radio group's is read as the family reads an option,
    /// a string or an array of exactly two strings, anything else an
    /// option of empty text.
    pub options: Arc<[Choice]>,
    /// `/Opt` is an array, its own or a parent field's, an empty one too:
    /// a check box's or radio group's widgets with no on state of their
    /// own are named by their indexes (`pdf::forms::group::on_state_name`).
    pub has_opt: bool,
    /// A check box's or radio group's widgets' on states, by their
    /// [`index`](Widget::index): each one's [`on_state`](Widget::on_state),
    /// a widget's on no page too, which no [`Widget`] is read for, so the
    /// group names every one of its widgets as its file does. Empty for a
    /// field of any other kind.
    pub on_states: Vec<Option<String>>,
    /// `/TU`: what the field is for, in words.
    pub tooltip: Option<String>,
    /// Its own `/AA`: keystroke, format, validate and calculate.
    pub actions: Vec<(Trigger, Action)>,
    /// Its widgets no page of the page tree lists: on page templates,
    /// which aren't pages until a script spawns one (M10.5 §3.7).
    pub outside: u16,
}

impl Field {
    pub fn has(&self, flag: u32) -> bool {
        self.flags & flag != 0
    }

    /// `/MaxLen` as layout uses it: comb cells and the character limit,
    /// at most [`super::read::MAX_LAYOUT_LEN`].
    pub fn layout_len(&self) -> Option<u32> {
        self.max_len.map(|n| n.min(super::read::MAX_LAYOUT_LEN))
    }

    /// The field's script for `trigger`, if it has one.
    pub fn script(&self, trigger: Trigger) -> Option<&str> {
        self.actions
            .iter()
            .find(|(t, a)| *t == trigger && a.has_script())
            .and_then(|(_, a)| a.script.as_deref())
    }
}

/// A widget: where a field shows on a page.
#[derive(Debug, Clone, PartialEq)]
pub struct Widget {
    pub field: FieldId,
    pub object: ObjectId,
    /// Its dictionary is its field's: one object, field and widget.
    pub merged: bool,
    /// Its place in its page's `/Annots`, which is how PDFium counts
    /// annotations.
    pub slot: usize,
    /// Its place among its field's widgets, from 0: its kids that are
    /// widgets, in `/Kids` order, then the field's own dictionary when
    /// it's a widget too, as the field tree is read. PDFium, and the
    /// family, count a field's own dictionary only where it has no such
    /// kids, so every kid's place is theirs. What its field's `/Opt`
    /// entry, and a check box's or radio button's on state made under
    /// `/Opt`, go by.
    pub index: u32,
    /// `/Rect` in the page's own space (before `/Rotate`): left, bottom,
    /// right, top, in order, finite and within [`super::read::MAX_COORD`],
    /// each number followed where it's a reference.
    pub rect: [f32; 4],
    /// `/F` (annotation flags: hidden, print, no view…).
    pub flags: u32,
    /// Its own `/DA`, when it isn't its field's dictionary too.
    pub da: Option<Arc<str>>,
    pub mk: Mk,
    /// `/BS` (or the older `/Border`), when it has one: its width, style
    /// and dash array, one value.
    pub border: Option<Border>,
    /// Its own `/Q`, when it isn't its field's dictionary too: 0 left, 1
    /// centered, 2 right. A text field's widget is justified by it first.
    pub align: Option<u8>,
    /// `/AS`: the appearance state it shows.
    pub state: Option<String>,
    /// The states `/AP /N` has, when it has several: a check box's on state
    /// and `Off`.
    pub states: Vec<String>,
    /// The name of its normal appearance's on state: of the names of
    /// `/AP /N`'s states that are streams, not empty and not `Off`, the
    /// least by its bytes, as the family reads one under PDFium's rules.
    /// None where `/N` is a stream, or none.
    pub on_state: Option<String>,
    /// It has a normal appearance: a stream, or states.
    pub has_appearance: bool,
    /// `/H` is `/P` (push) or `/T` (toggle), a name or, as PDFium reads
    /// one, a string: a push button's rollover and down appearances are
    /// made.
    pub pressed: bool,
    /// `/A` and its own `/AA` (enter, exit, down, up, focus, blur).
    pub actions: Vec<(Trigger, Action)>,
}

/// Annotation flags (`/F`).
pub mod annot_flags {
    pub const HIDDEN: u32 = 1 << 1;
    pub const PRINT: u32 = 1 << 2;
    pub const NO_ZOOM: u32 = 1 << 3;
    pub const NO_ROTATE: u32 = 1 << 4;
    pub const NO_VIEW: u32 = 1 << 5;
    pub const READ_ONLY: u32 = 1 << 6;
}

/// `/MK`: how a widget looks.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Mk {
    /// `/BG` and `/BC`; none for transparent.
    pub background: Option<Color>,
    pub border_color: Option<Color>,
    /// `/R`: 0, 90, 180 or 270.
    pub rotation: u16,
    /// `/CA`: a check box's glyph (in ZapfDingbats), or a button's caption.
    pub caption: Option<String>,
    /// `/RC`: a push button's rollover caption.
    pub rollover: Option<String>,
    /// `/AC`: a push button's down caption.
    pub down: Option<String>,
    /// `/TP`: where a push button's caption goes beside its icon, 0 the
    /// caption alone, 1 the icon alone: an integer, or a real taken toward
    /// 0, its low 32 bits, as the family reads it; none where it's
    /// neither.
    pub text_position: Option<i32>,
    /// `/I` is a stream: a push button's normal icon, which sheaf doesn't
    /// draw.
    pub icon: bool,
}

/// A color in `/MK`, each part within 0–1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Color {
    Gray(f32),
    Rgb([f32; 3]),
    Cmyk([f32; 4]),
}

/// A widget's border: `/BS`, or the older `/Border` where it has no `/BS`.
#[derive(Debug, Clone, PartialEq)]
pub struct Border {
    /// In points, within 0–100.
    pub width: f32,
    pub style: BorderStyle,
    /// Its dash array: `/BS /D`, or `/Border`'s fourth element (which makes
    /// that border dashed); the default
    /// ([`DEFAULT_DASH`](super::read::DEFAULT_DASH)) for a dashed border
    /// that gives none. At most [`MAX_DASHES`](super::read::MAX_DASHES)
    /// numbers, each finite and not negative; empty for none. A border
    /// that isn't dashed keeps the one its `/BS /D` gives, which nothing
    /// draws.
    pub dash: Vec<f32>,
}

impl Border {
    /// A solid border `width` wide, with no dash array: what a widget
    /// with neither `/BS` nor `/Border` draws, at width 1.
    pub const fn solid(width: f32) -> Border {
        Border {
            width,
            style: BorderStyle::Solid,
            dash: Vec::new(),
        }
    }
}

/// `/BS /S`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum BorderStyle {
    #[default]
    Solid,
    Dashed,
    Beveled,
    Inset,
    Underline,
}
