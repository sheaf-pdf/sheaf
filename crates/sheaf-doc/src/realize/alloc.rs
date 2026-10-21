//! A source's object numbers for its overlays (M10.5 §10.1): one allocator
//! a source, for the session, so a key is written at the same number in
//! every section over that source, and no two keys share one.
//!
//! What's written at its own number is the source's own: copy 0 of a page,
//! annotation or field read from it, and its catalog and form, which the
//! emission says ([`Emission::owned`](crate::materialize::emit::Emission::owned)).
//! Every other key (the group's root, resources and blocks, copies, what
//! sheaf made) takes a number past every number the base uses, its
//! `/Size` and every cross-reference stream's own number (slice 2's
//! 5801985, whose newest stream is past `/Size`), the first time a section
//! writes it, and keeps it. The first of those numbers is the sections'
//! own cross-reference stream's, for a base whose last section is one.
//!
//! **Bounds.** It grows by each key the source's sections ever wrote, a few
//! dozen bytes each, and never shrinks while the document is open: a
//! header's text changed a hundred times draws a hundred blocks. Past the
//! numbers a file may use ([`NUMBERS`]) no section can be made: opening a
//! group is an error, which a session would take 8 million keys to meet.

use std::collections::HashMap;

use anyhow::{Context as _, Result, bail};
use lopdf::ObjectId;

use crate::materialize::ObjKey;
use crate::serialize::NUMBERS;
use crate::serialize::append::Base;
use crate::source::Source;

/// One source's numbers, and the bytes its sections go after.
pub(crate) struct Numbers {
    /// The source's bytes, as their last section describes them.
    pub(crate) base: Base,
    /// The sections' cross-reference stream's number.
    pub(crate) xref: u32,
    /// The next number a key takes.
    next: u32,
    by_key: HashMap<ObjKey, u32>,
}

impl Numbers {
    /// `src`'s, none given yet.
    pub(crate) fn over(src: &Source) -> Result<Numbers> {
        let base = Base::over(src).context("its trailer names no catalog")?;
        let xref = base.size;
        Ok(Numbers {
            xref,
            next: xref.saturating_add(1),
            base,
            by_key: HashMap::new(),
        })
    }

    /// The number `key` is written at, when it isn't the source's own
    /// object: the one it took the first time, or the next.
    pub(crate) fn number(&mut self, key: ObjKey) -> Result<ObjectId> {
        if let Some(&n) = self.by_key.get(&key) {
            return Ok((n, 0));
        }
        if self.next >= NUMBERS {
            bail!("the source's overlays have used every number a file may");
        }
        let n = self.next;
        self.next += 1;
        self.by_key.insert(key, n);
        Ok((n, 0))
    }
}
