//! A push button's widget (M10 slice 8): what its appearances are made from
//! ([`PushLook`]: its captions, whether it has rollover and down
//! appearances, and whether it has a `/DA` at all, beside the widget's
//! [`Frame`]), which widgets need theirs made (`stale`), and their making by
//! pdf-forms ([`push_button`]) in the field's font and the fallback font.
//!
//! **What's read is the family's rule** (its read layer's `widget_push`):
//! the captions are `/MK /CA`, `/RC` and `/AC`, none shown with `/MK /TP 1`
//! (the icon alone); a widget whose `/MK /I` is a stream and whose `/TP`
//! isn't 0 shows an icon, which isn't drawn ([`Unmade::Icon`]); `/H /P` or
//! `/T` calls for the rollover and down appearances; with no `/DA` at all
//! (the widget's, its field's or its form's) the captions are drawn at 12
//! points, as PDFium draws them, and with one that names no font in
//! Helvetica at an automatic size.
//!
//! **Made only where its captions changed**: they're all of a push button
//! the model can change. One nothing changed keeps its appearances, and one
//! with none stays so (a file with `/NeedAppearances` is M10 slice 9's
//! transform), though [`make`](super::make) makes one for it as for any
//! other.

use std::fmt;
use std::sync::{Mutex, PoisonError};

use super::{DaFont, Frame, Made, Slot, effective_da, frame, limits, read_widget};
use crate::content::{Fallback, Glyphs};
use crate::forms::field::Mk;
use crate::model::{DocState, FieldNode, Widget};
use crate::pdf::forms::{PushAppearances, PushButton, Unmade, push_button};
use crate::pdf::gfx::WithFallback;

/// The size a push button's captions are drawn at where it has no `/DA` at
/// all, as PDFium as of chromium/8152 draws them.
const NO_DA_SIZE: f32 = 12.0;

/// What one push button's widget's appearances are made from: owned plain
/// data out of the model, as [`look`](super::look) gives it
/// ([`Look::Push`]), and what [`make`](super::make) makes them of.
///
/// `Debug` prints lengths and flags, never a caption.
///
/// [`Look::Push`]: super::Look::Push
#[derive(Clone, PartialEq)]
pub struct PushLook {
    /// `/MK /CA`, the normal caption: empty for none, and with `/MK /TP 1`,
    /// which draws the frame alone.
    pub caption: String,
    /// `/MK /RC` and `/AC`, the rollover and down captions: none for none,
    /// and with `/MK /TP 1`; the normal one is drawn where one is none or
    /// empty.
    pub rollover: Option<String>,
    pub down: Option<String>,
    /// `/H` is `/P` or `/T`: the rollover and down appearances are made.
    pub pressed: bool,
    /// It has a `/DA` at all: its own, its field's (its own or a
    /// parent's), or its form's, an empty one too. With none, its captions
    /// are drawn at 12 points.
    pub has_da: bool,
    /// The widget's box, `/MK`, border and effective `/DA`.
    pub frame: Frame,
}

impl fmt::Debug for PushLook {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let PushLook {
            caption,
            rollover,
            down,
            pressed,
            has_da,
            frame,
        } = self;
        f.debug_struct("PushLook")
            .field("caption_len", &caption.len())
            .field("rollover_len", &rollover.as_deref().map(str::len))
            .field("down_len", &down.as_deref().map(str::len))
            .field("pressed", pressed)
            .field("has_da", has_da)
            .field("frame", frame)
            .finish()
    }
}

/// Widget `w`'s look, `node` its field, a push button field: its captions
/// (none with `/MK /TP 1`), whether `/H` calls for its rollover and down
/// appearances, whether it has a `/DA` at all, and its frame.
/// [`Unmade::Icon`] where its `/MK /I` is a stream and its `/MK /TP` isn't
/// 0 (none is 0), as no icon is drawn.
pub(super) fn look(st: &DocState, w: &Widget, node: &FieldNode) -> Result<PushLook, Unmade> {
    let mk = &w.mk;
    if mk.icon && mk.text_position.unwrap_or(0) != 0 {
        return Err(Unmade::Icon);
    }
    let shown = |caption: &Option<String>| caption.clone().filter(|_| mk.text_position != Some(1));
    Ok(PushLook {
        caption: shown(&mk.caption).unwrap_or_default(),
        rollover: shown(&mk.rollover),
        down: shown(&mk.down),
        pressed: w.props.pressed,
        has_da: effective_da(st, w, node).is_some(),
        frame: frame(st, w, node),
    })
}

/// Whether widget `w`, a push button's, needs its appearances made
/// ([`stale`](super::stale)): its captions (`/MK /CA`, `/RC`, `/AC`) aren't
/// its source widget's as read. A widget read from no source, or not among
/// its source's widgets as read, had none.
pub(super) fn stale(w: &Widget) -> bool {
    let was = read_widget(w).map(|read| captions(&read.mk));
    was.unwrap_or_default() != captions(&w.mk)
}

/// A push button's three captions, as its `/MK` has them.
fn captions(mk: &Mk) -> [Option<&str>; 3] {
    [&mk.caption, &mk.rollover, &mk.down].map(|c| c.as_deref())
}

/// `look`'s appearances in `font`, and the fallback font
/// ([`Fallback::SANS`]) for what that can't draw: its `/DA` parsed (none,
/// or one that doesn't parse, is the empty one), at 12 points where it has
/// no `/DA` at all, then pdf-forms' maker under [`limits`]
/// ([`push_button`]), each grapheme cluster in the first of the two that
/// draws the whole of it. The fallback records each glyph it's drawn in
/// ([`Fallback::recording`]): what's made is the normal form
/// ([`Slot::Normal`]), and where `/H` calls for them the rollover and down
/// ones ([`Slot::Rollover`], [`Slot::Down`]), with those glyphs; a form the
/// field's font draws all of names that font alone. A refused make's
/// glyphs, some recorded before the refusal, go with it. A cluster neither
/// draws is [`Unmade::Unencodable`].
pub(super) fn make(look: &PushLook, font: DaFont) -> Result<Made, Unmade> {
    let limits = limits();
    let mut da = look.frame.da(&limits);
    if !look.has_da {
        da.font_size = Some(NO_DA_SIZE);
    }
    let widget = look.frame.widget();
    let mut button = PushButton::new(&look.caption);
    button.rollover_caption = look.rollover.as_deref();
    button.down_caption = look.down.as_deref();
    button.pressed_states = look.pressed;
    let recorded = Mutex::new(Glyphs::new());
    let layout = font.font.layout();
    let fallback = Fallback::SANS.recording(&recorded);
    let fonts = WithFallback::new(&layout, &fallback, Fallback::SANS.res());
    let PushAppearances {
        normal,
        rollover,
        down,
        ..
    } = push_button(&button, &widget, &da, font.res, &fonts, &limits)?;
    let forms = [
        (Slot::Normal, Some(normal)),
        (Slot::Rollover, rollover),
        (Slot::Down, down),
    ];
    let glyphs = recorded
        .into_inner()
        .unwrap_or_else(PoisonError::into_inner);
    Ok(Made {
        forms: (forms.into_iter())
            .filter_map(|(slot, form)| Some((slot, form?)))
            .collect(),
        glyphs,
    })
}
