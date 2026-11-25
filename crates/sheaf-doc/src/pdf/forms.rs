//! Text fields', check boxes', radio buttons', combo boxes' and list boxes'
//! appearances through `pdf-forms` (M10 slices 6 and 8), and a check box or
//! radio group's rules (M10 slice 8).
//!
//! **Making one.** A text field's appearance is made from plain data
//! ([`text_with_fallback`]): what the field shows ([`TextField`]: its text, flags
//! ([`FieldFlags`]), maximum length and justification ([`Quadding`])),
//! the widget's box and what's drawn around it ([`WidgetLook`], its border
//! pdf-gfx's [`BoxBorder`](super::gfx::BoxBorder)), its default
//! appearance string parsed
//! ([`DefaultAppearance`](super::gfx::DefaultAppearance)), and the font
//! that names ([`FieldFont`], read once from the `/DR /Font` of the form
//! a catalog names by [`FieldFont::find_in_catalog`], the walk there the
//! family's, or a standard 14 font where that has none; a direct font
//! that holds references lends its dictionary, [`FieldFont::direct`], for
//! the writer to write as an object), and the fallback font for what
//! that font can't draw ([`FieldFallback`]: its program and the resource
//! content names it by; M10 slice 7), under the family's caps
//! ([`Limits`], [`Cap`]). What comes back is the form XObject's parts
//! ([`Appearance`]: its box, its matrix, its content and what that draws
//! with, by what each is) with the fallback's glyphs it draws
//! ([`MadeWithFallback`]), or why none could be made ([`Unmade`]); a font
//! that can't be read is an [`Error`].
//!
//! A check box's or radio button's ([`check_box`], [`radio_button`]) are
//! made from what it shows ([`ToggleButton`]: its caption, which names its
//! mark), the same [`WidgetLook`] and default appearance string, and the
//! resource that names ZapfDingbats, the mark's font: the normal and down
//! appearances of its on state and of `Off`, an `Off` one none where it
//! would paint nothing ([`ToggleAppearances`]).
//!
//! A combo box's ([`combo_box`]) is made from the text it shows
//! ([`ComboBox`]), a list box's ([`list_box`]) from its options' labels,
//! the selected ones and the first shown ([`ListBox`]); each with the same
//! [`WidgetLook`] and default appearance string, and a set of fonts: the
//! field's, then the fallback, a program that records each glyph it draws
//! (pdf-gfx's [`ProgramFont`](super::gfx::ProgramFont) in a
//! [`WithFallback`](super::gfx::WithFallback)). Which options a value
//! selects ([`selected_options`], over each option's export value,
//! [`FieldOption`]) is the family's rule too.
//!
//! **A group's rules.** What pressing a check box or radio button, or
//! setting its field's value, does to each widget's state and to the
//! value ([`group`]: [`Group`](group::Group) of
//! [`GroupWidget`](group::GroupWidget)s, by [`GroupKind`](group::GroupKind)),
//! and the name a made on state goes under
//! ([`on_state_name`](group::on_state_name)), are the family's, over plain
//! data sheaf fills from its model (`crate::forms::group`).
//!
//! **Reading a form.** The family's own reading of a form's field tree
//! and of a widget for its appearance (`read`, `Form`, `Field`,
//! `FieldKind`, `Widget`, `widget_text`, `WidgetText`, under PDFium's
//! rules) is set beside sheaf's in tests alone: sheaf reads its form
//! itself (`crate::forms::read`), as its model holds widgets the tree
//! doesn't reach, copies of a form and ids of its own, and hands the maker
//! plain data from that model.
//!
//! A font is read from a source's objects through [`Resolve`] and
//! [`Streams`]; `crate::content::Objects` is any of sheaf's sources as
//! both, as for fonts ([`super::fonts`]).
//!
//! [`Resolve`]: super::Resolve
//! [`Streams`]: super::Streams

pub use pdf_forms::appearance::{
    Appearance, ComboBox, Fallback as FieldFallback, FieldFont, ListBox, MadeWithFallback,
    TextField, ToggleAppearances, ToggleButton, Unmade, WidgetLook, check_box, combo_box, list_box,
    radio_button, text_with_fallback,
};
pub use pdf_forms::{Cap, Error, FieldFlags, FieldOption, Limits, Quadding, selected_options};

/// A check box or radio group's rules, over plain data: which widgets a
/// press or a value turns on and off, the field's value and export value,
/// and the name each widget's on state goes under. For tests, the
/// family's own reading of a field's group ([`FieldGroup`](group::FieldGroup),
/// by [`field_group`]).
pub mod group {
    #[cfg(test)]
    pub use pdf_forms::group::FieldGroup;
    pub use pdf_forms::group::{Group, GroupKind, GroupWidget, on_state_name};
}

/// The family's reading of a form, which tests set beside sheaf's: its
/// field tree, its fields and their widgets, and a widget of a text field
/// read for its appearance.
#[cfg(test)]
pub use pdf_forms::appearance::WidgetText;
#[cfg(test)]
pub use pdf_forms::{Field, FieldKind, Form, Widget};

#[cfg(test)]
use crate::pdf::{Dictionary, Resolve, Streams};

/// The rules the family reads a form under in sheaf's tests: PDFium's, the
/// page view's engine, as for an annotation ([`super::annots`]).
#[cfg(test)]
const RULES: pdf_objects::pdf_tokenizer::Rules = pdf_objects::pdf_tokenizer::Rules::Pdfium;

/// The form `catalog` names, as the family reads it from `objects` under
/// PDFium's rules and `limits`: none where it names none, an error where
/// what it reads is past one of the caps or can't be read.
#[cfg(test)]
pub fn read<'o, R: Resolve<'o>>(
    objects: R,
    catalog: &'o Dictionary,
    limits: &Limits,
) -> Result<Option<Form<'o>>, Error> {
    Form::read(objects, catalog, RULES, limits)
}

/// `field`'s widgets as a group, as the family reads them from `objects`
/// under PDFium's rules and `limits`: the kind, the flags, whether it has
/// `/Opt`, each widget's on state and export value by its place among the
/// field's widgets, and each one's state as its `/AS` has it. None for a
/// field that is neither a check box nor a radio field.
#[cfg(test)]
pub fn field_group<'o, R: Resolve<'o>>(
    field: &Field<'o>,
    objects: R,
    limits: &Limits,
) -> Result<Option<group::FieldGroup>, Error> {
    field.group(objects, RULES, limits)
}

/// `widget` of `field`, a text field, as the family reads it for its
/// appearance from `objects` under PDFium's rules and `limits`: the
/// field's value, flags, maximum length and justification, the widget's
/// box, `/MK` and border, and its effective `/DA` parsed.
#[cfg(test)]
pub fn widget_text<'a, 'o: 'a, R: Resolve<'o> + Streams + Copy>(
    field: &'a Field<'o>,
    widget: &Widget<'o>,
    objects: R,
    limits: &Limits,
) -> Result<WidgetText<'a>, Unmade> {
    pdf_forms::appearance::widget_text(field, widget, objects, RULES, None, limits, &mut ())
}
