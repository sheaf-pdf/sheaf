//! A check box's or radio button's widget (M10 slice 8): what its
//! appearances are made from ([`ToggleLook`]: whether it's a radio
//! button's, its caption and the name its on state goes under, beside the
//! widget's [`Frame`]); which widgets show a state their file's doesn't
//! ([`state_changed`]) and which of those need appearances made (`stale`);
//! and their making by pdf-forms ([`check_box`], [`radio_button`]) with the
//! mark in ZapfDingbats: the normal and down appearances of the on state
//! and of `Off`, each `Off` one none where it would paint nothing.
//!
//! **Made only where its file has none to show.** A widget whose file's
//! normal appearance has an on state keeps its appearances whatever state
//! it shows: only its `/AS` moves (M10 proposal 10: on states come from
//! `/AP /N`). One whose file has none, or `Off` alone, gets the whole set
//! where its state changed, as PDFium makes every state at once.
//!
//! **The mark's font** is ZapfDingbats as the standard 14 font is written
//! by name, never the `/DR` font the `/DA` names: one small dictionary a
//! file, which the emitter writes once however many widgets name it.

use std::fmt;
use std::sync::LazyLock;

use super::{Frame, Made, Slot, frame, limits, read_widget};
use crate::annot::AnnotId;
use crate::content::{Glyphs, Res};
use crate::forms::field::FieldKind;
use crate::forms::group::{buttons, name_bytes};
use crate::model::{DocState, FieldNode, Terminal, Widget};
use crate::pdf::fonts::Std14;
use crate::pdf::forms::{
    FieldFont, ToggleAppearances, ToggleButton, Unmade, check_box, radio_button,
};

/// What one check box's or radio button's widget's appearances are made
/// from: owned plain data out of the model, as [`look`](super::look) gives
/// it ([`Look::Toggle`]), and what [`make`](super::make) makes them of.
///
/// `Debug` prints lengths and flags, never the caption or the name.
///
/// [`Look::Toggle`]: super::Look::Toggle
#[derive(Clone, PartialEq)]
pub struct ToggleLook {
    /// A radio button's: its default mark a circle, and with that mark its
    /// frame round. Else a check box's: its default mark a check.
    pub radio: bool,
    /// `/MK /CA`, whose first character is the mark's code in ZapfDingbats;
    /// none for the default mark.
    pub caption: Option<String>,
    /// The name its on state's forms go under, its bytes: its file's, else
    /// its index under `/Opt`, else `Yes`, as its field's group names it
    /// ([`Buttons::on_state`](crate::forms::Buttons::on_state)) and as a
    /// press puts it in `/AS`.
    pub on_state: Vec<u8>,
    /// The widget's box, `/MK`, border and effective `/DA`: the mark's
    /// color, and its size where the `/DA` gives one.
    pub frame: Frame,
}

impl fmt::Debug for ToggleLook {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let ToggleLook {
            radio,
            caption,
            on_state,
            frame,
        } = self;
        f.debug_struct("ToggleLook")
            .field("radio", radio)
            .field("caption_len", &caption.as_deref().map(str::len))
            .field("on_state_len", &on_state.len())
            .field("frame", frame)
            .finish()
    }
}

/// Widget `aid`, `w`, its look, `node` its field and `term` that field's
/// terminal, a check box or radio field: whether it's a radio button's,
/// its `/MK /CA`, the name its on state goes under, and its frame.
/// [`Unmade::NotButton`] where its field's group doesn't hold it.
pub(super) fn look(
    st: &DocState,
    aid: AnnotId,
    w: &Widget,
    node: &FieldNode,
    term: &Terminal,
) -> Result<ToggleLook, Unmade> {
    let on_state = (buttons(st, w.field))
        .and_then(|group| group.on_state(aid))
        .ok_or(Unmade::NotButton)?;
    Ok(ToggleLook {
        radio: term.kind == FieldKind::Radio,
        caption: w.mk.caption.clone(),
        on_state,
        frame: frame(st, w, node),
    })
}

/// The state widget `aid` of `st` shows where it isn't the one its source
/// widget showed as read: what its `/AS` is written as. None for a widget
/// that shows no state (nothing to write), one that shows its file's,
/// anything but a check box's or radio button's, and a state that names
/// nothing (a character past U+00FF, which no file and no press gives). A
/// widget read from no source, or not among its source's widgets as read,
/// showed none.
pub fn state_changed(st: &DocState, aid: AnnotId) -> Option<&str> {
    let w = st.annot(aid)?.widget()?;
    let kind = st.field(w.field)?.term.as_deref()?.kind;
    if !matches!(kind, FieldKind::CheckBox | FieldKind::Radio) {
        return None;
    }
    let state = w.state.as_deref()?;
    let was = read_widget(w).and_then(|read| read.state.as_deref());
    (Some(state) != was && name_bytes(state).is_some()).then_some(state)
}

/// Whether widget `aid`, `w`, a check box's or radio button's, needs its
/// appearances made ([`stale`](super::stale)): it shows a state its file's
/// doesn't ([`state_changed`]), and its source widget as read has no on
/// state in its normal appearance (or wasn't read).
pub(super) fn stale(st: &DocState, aid: AnnotId, w: &Widget) -> bool {
    state_changed(st, aid).is_some() && read_widget(w).is_none_or(|read| read.on_state.is_none())
}

/// `look`'s appearances, made by pdf-forms ([`check_box`] or
/// [`radio_button`]) under [`limits`]: its `/DA` parsed (none, or one that
/// doesn't parse, is the empty one: black, at the mark's own size), the
/// mark the caption names in ZapfDingbats ([`zapf_dingbats`]). The forms:
/// the on state's normal ([`Slot::Normal`]) and down ([`Slot::Down`]), and
/// `Off`'s ([`Slot::NormalOff`], [`Slot::DownOff`]) where it paints
/// anything, each with no glyph of the fallback. Never
/// [`Unmade::Unencodable`]: a caption ZapfDingbats has no glyph for shows
/// the default mark.
pub(super) fn make(look: &ToggleLook) -> Result<Made, Unmade> {
    let limits = limits();
    let da = look.frame.da(&limits);
    let widget = look.frame.widget();
    let mut button = ToggleButton::new();
    button.caption = look.caption.as_deref();
    let maker = match look.radio {
        true => radio_button,
        false => check_box,
    };
    let ToggleAppearances {
        on,
        off,
        down_on,
        down_off,
        ..
    } = maker(&button, &widget, &da, zapf_dingbats(), &limits)?;
    let forms = [
        (Slot::Normal, Some(on)),
        (Slot::NormalOff, off),
        (Slot::Down, Some(down_on)),
        (Slot::DownOff, down_off),
    ];
    Ok(Made {
        forms: (forms.into_iter())
            .filter_map(|(slot, form)| Some((slot, form?)))
            .collect(),
        glyphs: Glyphs::new(),
    })
}

/// The resource a mark's form names its font by: ZapfDingbats as the
/// standard 14 font is written by name (`/Type1`, `/BaseFont
/// /ZapfDingbats`, its own encoding, not embedded), a dictionary that
/// reaches nothing else, so its id is its bytes' and every form that names
/// it names one object.
pub(crate) fn zapf_dingbats() -> Res {
    static ZAPF: LazyLock<Res> = LazyLock::new(|| {
        (FieldFont::standard(Std14::ZapfDingbats).res()).expect("a standard font's dictionary")
    });
    ZAPF.clone()
}
