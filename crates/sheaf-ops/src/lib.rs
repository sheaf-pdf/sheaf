//! Document operations with no GUI, shared by the app and `sheaf-cli`.
//!
//! An op is a plain function of its inputs and a `&dyn Progress`
//! (the `in-progress` crate): it reports as it goes and stops at the next
//! `check()` once cancelled. Ops move here one at a time, when the
//! command line needs them.

// Tests may use what clippy.toml bans (the lint ledger, M10.5 slice 19).
#![cfg_attr(test, allow(clippy::disallowed_methods))]

pub mod blank;
pub mod combine;
pub mod comments;
pub mod crop;
pub mod dirs;
pub mod export;
pub mod images;
pub mod intake;
pub mod pages;
pub mod prefs;
pub mod scan;
pub mod session;
pub mod split;
pub mod stamps;
pub mod text;
pub mod views;

pub use in_progress::{Cancelled, NoProgress, Progress, Snapshot as ProgressSnapshot, Tracker};

/// Whether `e` is an op stopping because it was cancelled: [`Cancelled`],
/// with or without context, or inside an `io::Error`, which `p.check()?`
/// makes in a function that returns `io::Result`; or an edit's lowering
/// stopping at a page (`EditError::Cancelled`).
pub fn was_cancelled(e: &anyhow::Error) -> bool {
    e.chain().any(|c| {
        c.is::<Cancelled>()
            || c.downcast_ref::<std::io::Error>()
                .and_then(std::io::Error::get_ref)
                .is_some_and(|inner| inner.is::<Cancelled>())
            || c.downcast_ref::<sheaf_doc::EditError>() == Some(&sheaf_doc::EditError::Cancelled)
    })
}
