//! AcroForms (M10). The field model ([`field`]) and reading it from a
//! source with the facts sheaf shows about the form ([`read`]), what its
//! signatures allow after them ([`sigs`]), the form check ([`check`]):
//! what in a file's form other viewers would trip on, or what a save would
//! carry where it shouldn't; a widget's appearances made from the model's
//! plain data through pdf-forms, a text field's, a check box's or radio
//! button's, a combo box's or list box's and a push button's
//! ([`appearance`]); and a check box or radio field's group, what a press
//! or a value puts by the family's rules ([`group`]), which M10 slice 9's
//! lowering of a value calls.
//!
//! Tests build forms in code ([`fixture`]) and read real ones from Greg's
//! corpus ([`corpus`]). Both are compiled for this crate's tests and, with
//! the `fixtures` feature, for other crates' tests. A dump of the corpus's
//! text, button and choice widgets (`dump`) sets sheaf's reading of each
//! beside pdf-forms', and the appearances each makes.

pub mod appearance;
pub mod check;
#[cfg(any(test, feature = "fixtures"))]
pub mod corpus;
#[cfg(test)]
mod dump;
pub mod field;
#[cfg(any(test, feature = "fixtures"))]
pub mod fixture;
pub mod group;
pub mod read;
pub mod rules;
pub mod sigs;
pub mod xfa;

#[cfg(test)]
mod tests;

pub use check::{Finding, Report, check, check_file, check_latest};
pub use field::{Field, FieldId, FieldKind, Value, Widget, value_changed};
pub use group::{Buttons, Change, buttons};
pub use read::{FileForm, FormClass, FormFacts, read};

/// The full name of a node named `partial` under one named `up`: the two
/// joined by a dot, passing over an empty one, as PDFium's are. The
/// reader names a file's nodes by it, and the document's full-name index
/// its own.
pub(crate) fn full_name_of(up: &str, partial: &str) -> String {
    match (up, partial) {
        (up, "") => up.to_string(),
        ("", partial) => partial.to_string(),
        (up, partial) => format!("{up}.{partial}"),
    }
}
