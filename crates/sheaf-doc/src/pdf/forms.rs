//! Text fields' appearances through `pdf-forms` (M10 slice 6).
//!
//! **Making one.** A text field's appearance is made from plain data
//! ([`text`]): what the field shows ([`TextField`]: its text, flags
//! ([`FieldFlags`]), maximum length and justification ([`Quadding`])),
//! the widget's box and what's drawn around it ([`WidgetLook`], its border
//! pdf-gfx's [`BoxBorder`](super::gfx::BoxBorder)), its default
//! appearance string parsed
//! ([`DefaultAppearance`](super::gfx::DefaultAppearance)), and the font
//! that names ([`FieldFont`], read once from a form's `/DR /Font` by
//! [`FieldFont::find`], or a standard 14 font where that has none), under
//! the family's caps ([`Limits`], [`Cap`]). What comes back is the form
//! XObject's parts ([`Appearance`]: its box, its matrix, its content and
//! what that draws with, by what each is), or why none could be made
//! ([`Unmade`]); a font that can't be read is an [`Error`].
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

pub use pdf_forms::appearance::{Appearance, FieldFont, TextField, Unmade, WidgetLook, text};
pub use pdf_forms::{Cap, Error, FieldFlags, Limits, Quadding};

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
