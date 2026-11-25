//! A text field's widget (M10 slice 6): what its appearance is made from
//! ([`TextLook`]: the field's value, flags, `/MaxLen` and justification
//! beside the widget's [`Frame`]), which widgets need theirs made
//! (`stale`), and its making by pdf-forms ([`text_with_fallback`]) in the
//! field's font and the fallback font.

use std::fmt;
use std::sync::{Arc, LazyLock};

use super::{DaFont, Frame, Made, Slot, form_defaults, frame, limits, quadding};
use crate::content::Fallback;
use crate::forms::field::Value;
use crate::model::{DocState, FieldNode, Terminal, Widget};
use crate::pdf::forms::{FieldFlags, MadeWithFallback, TextField, Unmade, text_with_fallback};

/// What one text widget's appearance is made from: owned plain data out of
/// the model, as [`look`](super::look) gives it ([`Look::Text`]), and what
/// [`make`](super::make) makes it of.
///
/// `Debug` prints lengths and numbers, never the text.
///
/// [`Look::Text`]: super::Look::Text
#[derive(Clone, PartialEq)]
pub struct TextLook {
    /// The text shown: the field's value now; once an op carries one, the
    /// display string a format script gives for it (M10 slice 9), which
    /// pdf-forms draws as it draws a value.
    pub text: Arc<str>,
    /// The field's `/Ff` (see [`field::flags`](crate::forms::field::flags)).
    pub flags: u32,
    /// Its `/MaxLen`, as the file has it.
    pub max_len: Option<u32>,
    /// `/Q`: 0 left, 1 centered, 2 right. The widget's own, else its
    /// field's, else its form's.
    pub align: u8,
    /// The widget's box, `/MK`, border and effective `/DA`.
    pub frame: Frame,
}

impl fmt::Debug for TextLook {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let TextLook {
            text,
            flags,
            max_len,
            align,
            frame,
        } = self;
        f.debug_struct("TextLook")
            .field("text_len", &text.len())
            .field("flags", &format_args!("{flags:#x}"))
            .field("max_len", max_len)
            .field("align", align)
            .field("frame", frame)
            .finish()
    }
}

/// Widget `w`'s look, `node` its field and `term` that field's terminal, a
/// text field: its value and flags, `/MaxLen` and `/Q`, and its frame. The
/// `/Q` is the widget's own, else the terminal's (its own or a parent's,
/// as read), else its instance's form's, as the frame's `/DA` is.
pub(super) fn look(st: &DocState, w: &Widget, node: &FieldNode, term: &Terminal) -> TextLook {
    let (_, form_align) = form_defaults(st, w, node);
    TextLook {
        text: text_of(term.value.as_ref()),
        flags: term.flags,
        max_len: term.props.max_len,
        align: (w.props.align.or(term.props.align)).unwrap_or(form_align),
        frame: frame(st, w, node),
    }
}

/// Whether a widget of `node`, a text field whose terminal is `term`, needs
/// its appearance made ([`stale`](super::stale)): its text isn't what its
/// source's field held as read (no value and an empty one are the same),
/// or the field was read from no source, or isn't among its source's
/// fields as read.
pub(super) fn stale(node: &FieldNode, term: &Terminal) -> bool {
    let Some(src) = &node.src else {
        return true;
    };
    match src.src.form().field_by_object(src.id) {
        Some(read) => shows(read.value.as_ref()) != shows(term.value.as_ref()),
        None => true,
    }
}

/// The text a text field's value shows, borrowed: none and an empty one
/// alike.
fn shows(value: Option<&Value>) -> &str {
    match value {
        Some(Value::Text(text)) => text,
        _ => "",
    }
}

/// The text a text field's value shows: none and an empty one alike.
fn text_of(value: Option<&Value>) -> Arc<str> {
    static EMPTY: LazyLock<Arc<str>> = LazyLock::new(|| Arc::from(""));
    match value {
        Some(Value::Text(text)) => text.clone(),
        _ => EMPTY.clone(),
    }
}

/// `look`'s appearance in `font`, and the fallback font
/// ([`Fallback::SANS`]) for what that can't draw: its `/DA` parsed (none,
/// or one that doesn't parse, is the empty one: Helvetica at an automatic
/// size), then pdf-forms' maker under [`limits`] ([`text_with_fallback`]),
/// each grapheme cluster in the first of the two that draws the whole of
/// it. What's made is one form, `/N`'s ([`Slot::Normal`]), and the glyphs
/// it draws in the fallback: none where `font` draws all of the text, and
/// then the form names `font` alone, byte for byte as one font draws it. A
/// cluster neither draws is [`Unmade::Unencodable`].
pub(super) fn make(look: &TextLook, font: DaFont) -> Result<Made, Unmade> {
    let limits = limits();
    let da = look.frame.da(&limits);
    let mut field = TextField::new(&look.text);
    field.flags = FieldFlags::from_bits(look.flags);
    field.max_len = look.max_len;
    field.quadding = quadding(look.align);
    let widget = look.frame.widget();
    let fallback = Fallback::SANS.field();
    let layout = font.font.layout();
    let MadeWithFallback {
        appearance, glyphs, ..
    } = text_with_fallback(&field, &widget, &da, font.res, &layout, &fallback, &limits)?;
    Ok(Made {
        forms: vec![(Slot::Normal, appearance)],
        glyphs,
    })
}
