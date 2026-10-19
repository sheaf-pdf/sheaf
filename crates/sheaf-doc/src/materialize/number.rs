//! Where a written file's objects go (M10.5 §11.6).
//!
//! - **Stable,** for saves ([`stable`]): every primary object keeps its
//!   number, and so does each key the emitter says is a primary object
//!   ([`Emission::owned`]: copy 0 of a primary page's dictionary, bug 3,
//!   and a bookmark read from the primary); a key the previous save
//!   numbered keeps that number, but at a primary object it no longer is;
//!   every other key is numbered from past anything the primary or the
//!   previous save used: the pages in order, then the rest by key
//!   (annotations by id, fields by id, resources by digest). A save never
//!   renumbers what it didn't make, so raw passthrough holds for what it
//!   didn't touch.
//! - **Dense,** for exports ([`dense`]): a finished file's objects from 1,
//!   a stream byte-identical to one before it written once.
//! - **Appended,** for an Append's section ([`appended`]): a key the saved
//!   file's map has keeps its number, so a changed object is written over
//!   itself; a key that's a primary object sits at its own number; every
//!   other is numbered past every number the file uses, its `/Size`, its
//!   cross-reference streams' and its map's (slice 2's 5801985: an object
//!   numbered at the base's cross-reference stream's number was read as
//!   it), the section's own cross-reference stream after them.
//!
//! No number is kept, or given, past what a file may use
//! ([`NUMBERS`](crate::serialize::NUMBERS), Annex C's 8,388,607): a primary
//! object numbered past it, which only an object stream the table doesn't
//! place can hold, is a key like a new one (its previous save's number,
//! else a new one), and a number past it the primary or the previous save
//! says it used isn't reserved; nor are the primary's `/Size` and its
//! cross-reference stream's number when reserving them would leave too few.
//! A save that would need more numbers than are left stops.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use anyhow::{Result, bail};
use lopdf::{Document as Pdf, Object, ObjectId};

use super::emit::Emission;
use super::key::ObjKey;
use super::record::{ObjMap, SaveRecord};
use crate::model::DocState;
use crate::serialize::NUMBERS;
use crate::source::Source;
use crate::store::BlobId;

/// The number each key of an emission is written at.
#[derive(Debug, Clone)]
pub struct Numbers {
    /// The blob whose copy 0 is at its own numbers: the primary's.
    identity: BlobId,
    by_key: HashMap<ObjKey, ObjectId>,
    /// The first number given to a new key.
    start: u32,
}

impl Numbers {
    /// Where `key`'s object is written: None for a key the emission
    /// neither emits nor refers to.
    pub fn get(&self, key: &ObjKey) -> Option<ObjectId> {
        match *key {
            ObjKey::Base { blob, obj, copy: 0 } if blob == self.identity && obj.0 < NUMBERS => {
                Some(obj)
            }
            _ => self.by_key.get(key).copied(),
        }
    }

    /// The first number a key new to this save was given: past every
    /// number the primary and the previous save used.
    pub fn start(&self) -> u32 {
        self.start
    }
}

/// Stable numbers for `e`, the emission of `st`, written over `primary`,
/// which is `loaded` whole and whose table lists the numbers `in_use`,
/// after the save whose map is `prev`. Every object `loaded` has past what
/// a file may use ([`beyond`]) is numbered as a key of its own. An error
/// when the numbers run out.
pub fn stable(
    e: &Emission,
    st: &DocState,
    primary: &Source,
    loaded: &Pdf,
    in_use: &HashSet<u32>,
    prev: &ObjMap,
) -> Result<Numbers> {
    let identity = primary.id();
    let base = |obj| ObjKey::Base {
        blob: identity,
        obj,
        copy: 0,
    };
    let keys: BTreeSet<ObjKey> = (e.objects.iter().map(|(key, _)| *key))
        .chain(e.syms.keys().iter().copied())
        .chain(beyond(loaded).map(base))
        .collect();
    let own = |key: &ObjKey| match *key {
        ObjKey::Base { blob, obj, copy: 0 } if blob == identity && obj.0 < NUMBERS => Some(obj),
        _ => None,
    };
    let mut taken: HashSet<u32> = keys.iter().filter_map(own).map(|(n, _)| n).collect();
    let mut by_key = HashMap::new();
    for (key, &at) in &e.owned {
        if keys.contains(key) && at.0 < NUMBERS {
            taken.insert(at.0);
            by_key.insert(*key, at);
        }
    }
    // A key the previous save wrote at a primary object it isn't now (a
    // page that showed that page, and shows another) gives the number up:
    // only the object it is, or a root, sits at one.
    for key in &keys {
        if own(key).is_none()
            && !by_key.contains_key(key)
            && let Some(&at) = prev.keys.get(key)
            && at.0 < NUMBERS
            && (key.is_root() || !in_use.contains(&at.0))
            && taken.insert(at.0)
        {
            by_key.insert(*key, at);
        }
    }
    // The keys given new numbers: the pages in order, then the rest by key.
    let mut fresh: Vec<ObjKey> = Vec::new();
    let mut listed = HashSet::new();
    let pages = st.order().iter().map(|&id| ObjKey::Page(id));
    for key in pages.chain(keys.iter().copied()) {
        if keys.contains(&key)
            && own(&key).is_none()
            && !by_key.contains_key(&key)
            && listed.insert(key)
        {
            fresh.push(key);
        }
    }
    // And one for a cross-reference stream, past them.
    let needed = fresh.len() as u64 + 1;
    // Past every number the primary, its whole load and the previous save
    // use that a file may (one past it isn't kept), and every primary
    // number the emission names, so a reference to one that nothing is
    // stays one; and, while that leaves the numbers this save needs, past
    // the primary's `/Size` and its cross-reference stream, which nothing
    // names but an update after it.
    let usable = |id: &ObjectId| id.0 < NUMBERS;
    let below = loaded.objects.range(..(NUMBERS, 0)).next_back();
    let named = taken.iter().map(|&n| n.saturating_add(1)).max();
    let used = [
        1,
        past(prev.keys.values().copied().filter(usable)),
        past(primary.pdf().ids().filter(usable)),
        past(below.map(|(&id, _)| id).into_iter()),
        named.unwrap_or(0),
    ]
    .into_iter()
    .max()
    .unwrap_or(1);
    let xref = primary.xref();
    let reserved = [
        xref.size,
        xref.max_stream.map_or(0, |n| n.saturating_add(1)),
    ]
    .into_iter()
    .filter(|&n| u64::from(n) + needed <= u64::from(NUMBERS))
    .fold(used, u32::max);
    if u64::from(used) + needed > u64::from(NUMBERS) {
        bail!("the file would need more than the {NUMBERS} objects a PDF may have");
    }
    let start = reserved;
    for (key, next) in fresh.into_iter().zip(start..) {
        debug_assert!(!taken.contains(&next), "{next} is taken");
        by_key.insert(key, (next, 0));
    }
    Ok(Numbers {
        identity,
        by_key,
        start,
    })
}

/// Where an Append's objects are written ([`appended`]).
#[derive(Debug, Clone)]
pub struct Appended {
    /// Each emitted key's number.
    pub by_key: HashMap<ObjKey, ObjectId>,
    /// The first number given to a key new to the file.
    pub start: u32,
    /// The section's cross-reference stream's number, past them all.
    pub xref: u32,
}

/// The numbers an Append of `e` onto the file `record` says writes its
/// objects at: each key the file's map has at its number there, each the
/// emission says is a primary object ([`Emission::owned`]) at its own, and
/// the rest from past every number the file uses, the pages in order, then
/// the rest by key, and the section's cross-reference stream after them.
/// An error when no numbers are left for them.
pub fn appended(e: &Emission, record: &SaveRecord) -> Result<Appended> {
    let mut by_key = HashMap::new();
    let mut taken = HashSet::new();
    let mut pages = Vec::new();
    let mut fresh = BTreeSet::new();
    // A widget merged with its field in the saved file is its field's
    // object, which the field keeps: written apart from it now, the widget
    // takes a number of its own.
    let (widgets, others): (Vec<&ObjKey>, Vec<&ObjKey>) = (e.objects.iter())
        .map(|(key, _)| key)
        .partition(|key| matches!(key, ObjKey::Annot(_)));
    for key in others.into_iter().chain(widgets) {
        match record.map.get(key).or_else(|| e.owned.get(key).copied()) {
            Some(at) if taken.insert(at.0) => {
                by_key.insert(*key, at);
            }
            // Emitted in order.
            _ if matches!(key, ObjKey::Page(_)) => pages.push(*key),
            _ => {
                fresh.insert(*key);
            }
        }
    }
    let xref = &record.xref;
    let start = [
        1,
        xref.size,
        xref.max_stream.map_or(0, |n| n.saturating_add(1)),
        past(record.map.keys.values().copied()),
        record.present.last().map_or(0, |n| n.saturating_add(1)),
    ]
    .into_iter()
    .max()
    .unwrap_or(1);
    let needed = (pages.len() + fresh.len()) as u64 + 1;
    if u64::from(start) + needed > u64::from(NUMBERS) {
        bail!("no object number left for an update");
    }
    let mut next = start;
    for key in pages.into_iter().chain(fresh) {
        by_key.insert(key, (next, 0));
        next += 1;
    }
    Ok(Appended {
        by_key,
        start,
        xref: next,
    })
}

/// The objects of `loaded`, a primary loaded whole, numbered past what a
/// file may use, but for those a full write leaves out: what the eager
/// reader takes from an object stream that the cross-reference table
/// doesn't place, which the lazy reader never reads.
pub fn beyond(loaded: &Pdf) -> impl Iterator<Item = ObjectId> + '_ {
    (loaded.objects.range((NUMBERS, 0)..))
        .filter(|(_, obj)| !crate::serialize::full::left_out(obj))
        .map(|(&id, _)| id)
}

/// The number past the largest of `ids`', or 0.
fn past(ids: impl Iterator<Item = ObjectId>) -> u32 {
    ids.map(|id| id.0.saturating_add(1)).max().unwrap_or(0)
}

/// Dense numbers for a finished file's `objects`: from 1, in the order of
/// their numbers, at generation 0. A stream byte-identical to one numbered
/// before it, as written, takes that one's number, unless `apart` keeps it
/// apart (a stream whose data is made only as it's written).
pub fn dense(
    objects: &BTreeMap<ObjectId, Object>,
    apart: impl Fn(ObjectId) -> bool,
) -> BTreeMap<ObjectId, ObjectId> {
    let written = |obj: &Object| {
        let mut out = Vec::new();
        crate::serialize::object::write(&mut out, obj).ok()?;
        Some(out)
    };
    let mut out = BTreeMap::new();
    let mut streams: HashMap<u128, Vec<ObjectId>> = HashMap::new();
    let mut next = 1;
    for (&id, obj) in objects {
        if matches!(obj, Object::Stream(_))
            && !apart(id)
            && let Some(bytes) = written(obj)
        {
            let like = streams.entry(sheaf_core::checksum(&bytes)).or_default();
            let same = (like.iter())
                .find(|&&first| written(&objects[&first]).as_deref() == Some(bytes.as_slice()));
            if let Some(first) = same {
                out.insert(id, out[first]);
                continue;
            }
            like.push(id);
        }
        out.insert(id, (next, 0));
        next += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use lopdf::{Stream, dictionary};

    use super::*;

    /// Numbers from 1 in order, and a stream like one before it, dict and
    /// data alike, sharing its number, but for one kept apart.
    #[test]
    fn dense_numbers_write_alike_streams_once() {
        let stream = |data: &[u8]| Object::Stream(Stream::new(dictionary! {}, data.to_vec()));
        let objects = BTreeMap::from([
            ((4, 0), Object::Integer(1)),
            ((7, 2), stream(b"abc")),
            ((9, 0), stream(b"xyz")),
            ((12, 0), stream(b"abc")),
            (
                (13, 0),
                Object::Stream(Stream::new(dictionary! { "A" => 1 }, b"abc".to_vec())),
            ),
            ((20, 0), stream(b"abc")),
            ((21, 0), Object::Integer(1)),
        ]);
        let numbers = dense(&objects, |id| id == (20, 0));
        let want = [
            ((4, 0), (1, 0)),
            ((7, 2), (2, 0)),
            ((9, 0), (3, 0)),
            ((12, 0), (2, 0)),
            ((13, 0), (4, 0)),
            ((20, 0), (5, 0)),
            ((21, 0), (6, 0)),
        ];
        assert_eq!(numbers, BTreeMap::from(want));
    }
}
