//! The Arlington PDF Model (backlog F22): a test-only checker that holds
//! the objects sheaf writes to the grammar of PDF's objects the PDF
//! Association keeps, ISO 32000-2 as its errata correct it. Only what
//! sheaf writes is held to it, never what it copies from a file: most real
//! files break a rule somewhere.
//!
//! The model is vendored in `vendor/arlington-pdf-model` (its `SOURCE.md`
//! says from where): a TSV table per object, a row per key or array
//! element, saying its types, the PDF version it came in and any it was
//! deprecated in, whether it's required, the values it may take and the
//! tables its value links to. The tables' layout is upstream's README.md,
//! "TSV Data Overview"; the rules for each column are INTERNAL_GRAMMAR.md's
//! "TSV file rules" and "TSV Data Fields", from "Column 1 - Key" to
//! "Column 11 - Link". [`grammar()`] reads them, [`predicate`] the
//! predicates in their cells, and [`check()`] walks a file from its
//! trailer, holding the objects sheaf wrote ([`Hold::written`]) to the
//! tables they're reached as; [`Report::beyond`] leaves out the faults
//! sheaf carried from a source, checked at the written file's version
//! ([`check_at`]), and [`Allowances`] reads the findings held for a
//! backlog entry.
//!
//! Compiled for this crate's tests and, with the `fixtures` feature, for
//! other crates' tests; never in a shipped build.

pub mod allowance;
pub mod check;
pub mod grammar;
pub mod hold;
pub mod predicate;

#[cfg(test)]
mod check_tests;
#[cfg(test)]
mod hold_tests;
#[cfg(test)]
mod predicate_tests;
#[cfg(test)]
mod tests;

pub use allowance::{Allowance, Allowances};
pub use check::{
    Class, Finding, Hold, Made, Report, Rule, Unchecked, Unlisted, check, check_at, file_version,
};
pub use grammar::{ArlType, At, Grammar, Row, Table, Wrap, WrapFn, Wrapped, grammar};
pub use predicate::{Cx, Gate, Reference, Since, Truth};

/// The upstream commit the vendored model is (`SOURCE.md`).
pub const COMMIT: &str = "d148fee5a0cb6dcacadf58c09ed842e32847024d";

/// The vendored model's tables: upstream's `tsv/latest`, one grammar for
/// every PDF version.
pub const DIR: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../vendor/arlington-pdf-model/tsv/latest"
);
