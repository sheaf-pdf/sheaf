//! Document operations with no GUI, shared by the app and `sheaf-cli`.
//!
//! An op is a plain function of its inputs and a `&dyn Progress`
//! (the `in-progress` crate): it reports as it goes and stops at the next
//! `check()` once cancelled. Ops move here one at a time, when the
//! command line needs them.

pub mod blank;
pub mod combine;
pub mod comments;
pub mod crop;
pub mod derive;
pub mod dirs;
pub mod export;
pub mod images;
pub mod pages;
pub mod prefs;
pub mod scan;
pub mod split;
pub mod stamps;
pub mod views;

use std::collections::BTreeMap;

use anyhow::Result;
use sheaf_doc::{SourceId, Src};
use sheaf_render::pdfium::PdfiumEngine;

pub use in_progress::{Cancelled, NoProgress, Progress, Snapshot as ProgressSnapshot, Tracker};

/// Whether `e` is an op stopping because it was cancelled: [`Cancelled`],
/// with or without context, or inside an `io::Error`, which `p.check()?`
/// makes in a function that returns `io::Result`.
pub fn was_cancelled(e: &anyhow::Error) -> bool {
    e.chain().any(|c| {
        c.is::<Cancelled>()
            || c.downcast_ref::<std::io::Error>()
                .and_then(std::io::Error::get_ref)
                .is_some_and(|inner| inner.is::<Cancelled>())
    })
}

/// `items` grouped by the source each is of: the sources by id, each
/// one's items in the order they came.
pub(crate) fn by_source<T>(items: impl IntoIterator<Item = (Src, T)>) -> Vec<(Src, Vec<T>)> {
    let mut groups: BTreeMap<SourceId, (Src, Vec<T>)> = BTreeMap::new();
    for (src, item) in items {
        let group = groups.entry(src.id()).or_insert_with(|| (src, Vec::new()));
        group.1.push(item);
    }
    groups.into_values().collect()
}

/// A renderer for a source, on its blob's bytes.
pub fn engine_for(src: &Src) -> Result<PdfiumEngine> {
    PdfiumEngine::open_bytes(src.bytes().clone(), src.password())
}
