//! A combo box's or list box's widget (M10 slice 8): what its appearance is
//! made from ([`ChoiceLook`]: what the field shows, a combo box's text or a
//! list box's options with the selected ones and the first shown, beside
//! the widget's [`Frame`]), which widgets need theirs made (`stale`), and
//! its making by pdf-forms ([`combo_box`], [`list_box`]) in the field's font
//! and the fallback font.
//!
//! **What a value shows** is the family's rule (`selected_options` with no
//! `/I`, which a test sets beside sheaf's `selected`), over
//! the value alone: `/I` isn't modeled, so every option whose export value
//! is one of the value's is selected, and a combo box shows the first one's
//! label, else the value as it is (an editable combo box's outside its
//! options), else, with no value, nothing.

use std::fmt;
use std::sync::{Arc, LazyLock, Mutex, PoisonError};

use super::{DaFont, Frame, Made, Slot, frame, limits};
use crate::content::{Fallback, Glyphs};
use crate::forms::field::{Choice, FieldKind, Value};
use crate::model::{DocState, FieldNode, Terminal, Widget};
use crate::pdf::forms::{ComboBox, ListBox, Unmade, combo_box, list_box};
use crate::pdf::gfx::WithFallback;

/// What one combo box's or list box's widget's appearance is made from:
/// owned plain data out of the model, as [`look`](super::look) gives it
/// ([`Look::Choice`]), and what [`make`](super::make) makes it of.
///
/// `Debug` prints lengths and counts, never a text or a label.
///
/// [`Look::Choice`]: super::Look::Choice
#[derive(Clone, PartialEq)]
pub struct ChoiceLook {
    /// What the field shows: a combo box's text, or a list box's options.
    pub shows: ChoiceShows,
    /// The widget's box, `/MK`, border and effective `/DA`.
    pub frame: Frame,
}

/// What a combo box or a list box shows, by which it is.
#[derive(Clone, PartialEq)]
pub enum ChoiceShows {
    /// A combo box's text, beside its drop button: its display, the text
    /// a format script made of its value, where it has one
    /// ([`Terminal::display`]); else the label of the first option its
    /// value (the first of its texts) selects, else that text as it is;
    /// empty with no value.
    Combo { text: Arc<str> },
    /// A list box's options, each drawn by its label (`shown`), from the
    /// one at `top` (`/TI`) on; the indexes in `options` of those its value
    /// selects, sorted and each once, none with no value.
    List {
        options: Arc<[Choice]>,
        selected: Vec<usize>,
        top: usize,
    },
}

impl fmt::Debug for ChoiceLook {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let ChoiceLook { shows, frame } = self;
        f.debug_struct("ChoiceLook")
            .field("shows", shows)
            .field("frame", frame)
            .finish()
    }
}

impl fmt::Debug for ChoiceShows {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ChoiceShows::Combo { text } => f
                .debug_struct("Combo")
                .field("text_len", &text.len())
                .finish(),
            ChoiceShows::List {
                options,
                selected,
                top,
            } => f
                .debug_struct("List")
                .field("options", &options.len())
                .field("selected", &selected.len())
                .field("top", top)
                .finish(),
        }
    }
}

/// Widget `w`'s look, `node` its field and `term` that field's terminal, a
/// combo box or list box: a combo box's display where it has one, else
/// what its value shows among its options (`selected`, by the value
/// alone), its `/TI` for a list box, and its frame.
pub(super) fn look(st: &DocState, w: &Widget, node: &FieldNode, term: &Terminal) -> ChoiceLook {
    let options = &term.props.options;
    let shows = match term.kind {
        FieldKind::Combo => ChoiceShows::Combo {
            text: (term.display.clone())
                .unwrap_or_else(|| combo_text(options, term.value.as_ref())),
        },
        _ => ChoiceShows::List {
            options: options.clone(),
            selected: selected(options, &values(term.value.as_ref())),
            top: usize::try_from(term.props.top).unwrap_or(usize::MAX),
        },
    };
    ChoiceLook {
        shows,
        frame: frame(st, w, node),
    }
}

/// The indexes in `options` of those `values` select, as the family finds
/// them with no `/I` (`selected_options`, which a test sets beside this):
/// every option whose export value is one of them, in order, each once.
/// Read from the options as the field's record shares them: no option is
/// copied, however many the field has and however many widgets show it.
pub(super) fn selected(options: &[Choice], values: &[&str]) -> Vec<usize> {
    let mut given = values.to_vec();
    given.sort_unstable();
    (options.iter().enumerate())
        .filter(|(_, o)| given.binary_search(&&*o.export).is_ok())
        .map(|(i, _)| i)
        .collect()
}

/// A combo box's text for `value`: the label of the first option its
/// first text selects (the first of [`selected`]'s), else that text as it is, else, with
/// no value, none. A value of several texts shows its first, as the family
/// and PDFium show one.
fn combo_text(options: &[Choice], value: Option<&Value>) -> Arc<str> {
    static EMPTY: LazyLock<Arc<str>> = LazyLock::new(|| Arc::from(""));
    let first = match value {
        Some(Value::Text(text)) => Some(text),
        Some(Value::Items(items)) => items.first(),
        _ => None,
    };
    let Some(first) = first else {
        return EMPTY.clone();
    };
    match options.iter().find(|o| o.export == *first) {
        Some(option) => option.shown.clone(),
        None => first.clone(),
    }
}

/// The texts a choice field's value holds, as the family takes them
/// (`field_choice`): a text is one, even an empty one, which selects an
/// option whose export value is empty; an array its texts; none, no value
/// and anything else.
fn values(value: Option<&Value>) -> Vec<&str> {
    match value {
        Some(Value::Text(text)) => vec![&**text],
        Some(Value::Items(items)) => items.iter().map(|t| &**t).collect(),
        _ => Vec::new(),
    }
}

/// Whether a widget of `node`, a combo box or list box whose terminal is
/// `term`, needs its appearance made ([`stale`](super::stale)): it has a
/// display, which no file holds; or its value's texts ([`values`]) aren't
/// its source's field's as read, or the field was read from no source, or
/// isn't among its source's fields as read. No
/// value and an empty array are alike, and a text and an array of that
/// one text, as each shows the same; an empty text isn't no value, as it
/// selects an option whose export value is empty.
pub(super) fn stale(node: &FieldNode, term: &Terminal) -> bool {
    let Some(src) = node.src.as_ref().filter(|_| term.display.is_none()) else {
        return true;
    };
    match src.src.form().field_by_object(src.id) {
        Some(read) => values(read.value.as_ref()) != values(term.value.as_ref()),
        None => true,
    }
}

/// `look`'s appearance in `font`, and the fallback font
/// ([`Fallback::SANS`]) for what that can't draw: its `/DA` parsed (none,
/// or one that doesn't parse, is the empty one), then pdf-forms' maker
/// under [`limits`] ([`combo_box`], with its drop button, or [`list_box`]),
/// each grapheme cluster in the first of the two that draws the whole of
/// it. The fallback records each glyph it's drawn in
/// ([`Fallback::recording`]): what's made is one form, `/N`'s
/// ([`Slot::Normal`]), and those glyphs, none where `font` draws all of
/// it, and then the form names `font` alone. A refused make's glyphs, some
/// recorded before the refusal, go with it. A cluster neither draws is
/// [`Unmade::Unencodable`] in a combo box's text,
/// [`Unmade::UnencodableOption`] in a list box's line; more options listed
/// from the top than [`limits`]' `items` is [`Unmade::Limit`].
pub(super) fn make(look: &ChoiceLook, font: DaFont) -> Result<Made, Unmade> {
    let limits = limits();
    let da = look.frame.da(&limits);
    let widget = look.frame.widget();
    let recorded = Mutex::new(Glyphs::new());
    let layout = font.font.layout();
    let fallback = Fallback::SANS.recording(&recorded);
    let fonts = WithFallback::new(&layout, &fallback, Fallback::SANS.res());
    let appearance = match &look.shows {
        ChoiceShows::Combo { text } => combo_box(
            &ComboBox::new(text),
            &widget,
            &da,
            font.res,
            &fonts,
            &limits,
        )?,
        ChoiceShows::List {
            options,
            selected,
            top,
        } => {
            let labels: Vec<&str> = options.iter().map(|o| &*o.shown).collect();
            let mut list = ListBox::new(&labels);
            list.selected = selected;
            list.top = *top;
            list_box(&list, &widget, &da, font.res, &fonts, &limits)?
        }
    };
    let glyphs = recorded
        .into_inner()
        .unwrap_or_else(PoisonError::into_inner);
    Ok(Made {
        forms: vec![(Slot::Normal, appearance)],
        glyphs,
    })
}
