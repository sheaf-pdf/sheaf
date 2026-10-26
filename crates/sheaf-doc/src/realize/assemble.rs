//! A group's realization put together (M10.5 §10.1): its pages' deltas,
//! deduplicated by key, and its frame, numbered by the source's allocator
//! ([`Numbers`]), as an append section after the source's bytes, encrypted
//! with the source's key when it is (spike A); over a damaged base, whose
//! cross-references were rebuilt (spike B), as a file of its own, holding
//! what its pages reach of the source, unencrypted; and a source's view
//! patch, its catalog with the layers the view turns off.
//!
//! What a page's emission writes at the source's own number as the source
//! has it ([`SymObject::Raw`]) the base has: the section doesn't write it.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};

use anyhow::{Context as _, Result, bail};
use in_progress::NoProgress;
use lopdf::{Dictionary, Object, ObjectId, Stream, XrefKind};

use super::alloc::Numbers;
use crate::layers::{LayerId, ocg_patch};
use crate::materialize::ObjKey;
use crate::materialize::emit::{Emission, PdfVersion, SymObject, Syms};
use crate::model::PageId;
use crate::serialize::{append, full};
use crate::store::Src;

/// What an emission puts in a section: its objects by key, its
/// placeholders, and the keys it writes at the source's own numbers.
pub(crate) struct Part {
    objects: Vec<(ObjKey, SymObject)>,
    syms: Syms,
    owned: BTreeMap<ObjKey, ObjectId>,
    version: PdfVersion,
}

impl Part {
    /// Page `page`'s delta: its emission alone ([`EmitScope::Delta`]),
    /// which writes none of the frame's keys (the group's root, the
    /// catalog, the form and its fields) that [`Part::frame`] writes for
    /// the group, without the numbers it claims for them.
    ///
    /// [`EmitScope::Delta`]: crate::materialize::emit::EmitScope::Delta
    pub(crate) fn page(e: Emission, page: PageId) -> Result<Part> {
        let Emission {
            objects,
            syms,
            version,
            owned,
            ..
        } = e;
        if !objects.iter().any(|(key, _)| *key == ObjKey::Page(page)) {
            bail!("{page:?} wasn't emitted");
        }
        let objects = (objects.into_iter())
            .filter(|(key, _)| !framed(key))
            .collect();
        let owned = owned.into_iter().filter(|(key, _)| !framed(key)).collect();
        Ok(Part {
            objects,
            syms,
            owned,
            version,
        })
    }

    /// A group's frame, its pages' emissions aside.
    pub(crate) fn frame(e: Emission) -> Part {
        let Emission {
            objects,
            syms,
            version,
            owned,
            ..
        } = e;
        Part {
            objects,
            syms,
            owned,
            version,
        }
    }

    /// About how many bytes it holds, allocations and their overhead
    /// counted: what the delta cache is bounded by. A delta of a page with
    /// a header weighs 11.6 KB on the synthetic 7k and 14.7 KB on the heavy
    /// by this count; the synthetic's held 11.1 KB of RSS each, uncapped,
    /// and a cache at its cap about its bytes (the 7k gate harness).
    pub(crate) fn weight(&self) -> usize {
        let object = |obj: &SymObject| match obj {
            SymObject::Raw(_) => 0,
            SymObject::Built(obj) => held(obj),
            SymObject::Deferred(d) => dict_held(&d.dict) + CHUNK,
        };
        let entry = size_of::<(ObjKey, SymObject)>();
        let key = size_of::<ObjKey>();
        (self
            .objects
            .iter()
            .map(|(_, obj)| object(obj))
            .sum::<usize>())
            + CHUNK
            + self.objects.capacity() * entry
            + self.syms.keys().len() * (3 * key + 16)
            + self.owned.len() * (key + size_of::<ObjectId>() + 32)
    }
}

/// What a part writes that a page reaches, by key ([`Part::reached`]):
/// each object as text, and the number it's written at when that's the
/// source's own.
#[cfg(test)]
pub(crate) type Reached = BTreeMap<ObjKey, (String, Option<ObjectId>)>;

#[cfg(test)]
impl Part {
    /// What it writes that page `page` reaches, by key: each object as
    /// text, a placeholder in it as the key it names, a source's object
    /// it keeps as it is as that object, and the number it's written at
    /// when that's the source's own; and the keys it writes that the page
    /// doesn't reach. Two deltas alike in these write the same section.
    pub(crate) fn reached(&self, page: PageId) -> (Reached, BTreeSet<ObjKey>) {
        let at: HashMap<ObjKey, &SymObject> =
            self.objects.iter().map(|(key, obj)| (*key, obj)).collect();
        let by_number: HashMap<ObjectId, ObjKey> =
            self.owned.iter().map(|(key, id)| (*id, *key)).collect();
        let mut reached = BTreeMap::new();
        let mut next = vec![ObjKey::Page(page)];
        while let Some(key) = next.pop() {
            if reached.contains_key(&key) {
                continue;
            }
            let Some(obj) = at.get(&key) else {
                continue;
            };
            let mut named = Vec::new();
            let text = match obj {
                SymObject::Raw(r) => {
                    if let Ok(obj) = r.src.pdf().get_object(r.id) {
                        refs(obj, &mut |id| named.extend(by_number.get(&id)));
                    }
                    format!("raw {:?} {:?}", r.src.id(), r.id)
                }
                SymObject::Built(obj) => self.text(obj, &mut named),
                SymObject::Deferred(d) => {
                    let dict = self.text(&Object::Dictionary(d.dict.clone()), &mut named);
                    let data = (d.data)().expect("a deferred stream's data");
                    format!("deferred {dict} {}", sheaf_util::checksum(&data))
                }
            };
            next.extend(named);
            reached.insert(key, (text, self.owned.get(&key).copied()));
        }
        let unreached = (self.objects.iter())
            .map(|(key, _)| *key)
            .filter(|key| !reached.contains_key(key))
            .collect();
        (reached, unreached)
    }

    /// `obj` as text, each placeholder in it as the key it names, which
    /// `named` gets.
    fn text(&self, obj: &Object, named: &mut Vec<ObjKey>) -> String {
        fn named_in(obj: &Object, syms: &Syms, named: &mut Vec<ObjKey>) -> Object {
            match obj {
                Object::Reference(r) => match syms.key(*r) {
                    Some(key) => {
                        named.push(key);
                        Object::Name(format!("@{key:?}").into_bytes())
                    }
                    None => obj.clone(),
                },
                Object::Array(items) => {
                    Object::Array(items.iter().map(|o| named_in(o, syms, named)).collect())
                }
                Object::Dictionary(d) => Object::Dictionary(dict_named(d, syms, named)),
                Object::Stream(s) => {
                    let dict = dict_named(&s.dict, syms, named);
                    Object::Stream(Stream::new(dict, s.content.clone()))
                }
                _ => obj.clone(),
            }
        }
        fn dict_named(d: &Dictionary, syms: &Syms, named: &mut Vec<ObjKey>) -> Dictionary {
            let mut out = Dictionary::new();
            for (k, v) in d.iter() {
                out.set(k.clone(), named_in(v, syms, named));
            }
            out
        }
        format!("{:?}", named_in(obj, &self.syms, named))
    }
}

/// What an allocation costs past what it holds: glibc's chunk header,
/// rounded up.
const CHUNK: usize = 32;

/// About what `obj` holds on the heap beyond its own slot.
fn held(obj: &Object) -> usize {
    match obj {
        Object::Name(b) | Object::String(b, _) => CHUNK + b.capacity(),
        Object::Array(items) => {
            CHUNK + items.capacity() * size_of::<Object>() + items.iter().map(held).sum::<usize>()
        }
        Object::Dictionary(d) => dict_held(d),
        Object::Stream(s) => dict_held(&s.dict) + CHUNK + s.content.capacity(),
        _ => 0,
    }
}

/// About what `d` holds on the heap: its entries, each key's bytes and
/// what each value holds, and its index, both grown by doubling.
fn dict_held(d: &Dictionary) -> usize {
    let slots = d.len().next_power_of_two().max(4);
    let entry = size_of::<(u64, Vec<u8>, Object)>();
    let index = 2 * slots * (size_of::<usize>() + 1);
    let values = (d.iter())
        .map(|(k, v)| CHUNK + k.capacity() + held(v))
        .sum::<usize>();
    2 * CHUNK + slots * entry + index + values
}

/// Whether `key` is the frame's: the group's, not a page's.
fn framed(key: &ObjKey) -> bool {
    matches!(
        key,
        ObjKey::Catalog | ObjKey::PagesNode(_) | ObjKey::AcroForm | ObjKey::Field(_)
    )
}

/// `parts`, numbered: each key at the source's own number the first part
/// that writes it there says (a number a later one says too, it takes
/// another's, given by `numbers`), and every other key at `numbers`'.
/// Each key's object is the first part's that writes it; one the source
/// has as it is at that number isn't written.
fn numbered(
    src: &Src,
    parts: &[&Part],
    numbers: &mut Numbers,
) -> Result<BTreeMap<ObjectId, (ObjKey, Object)>> {
    let mut at: HashMap<ObjKey, ObjectId> = HashMap::new();
    let mut taken = HashSet::new();
    for part in parts {
        for (key, &n) in &part.owned {
            if !at.contains_key(key) && taken.insert(n) {
                at.insert(*key, n);
            }
        }
    }
    let blob = src.id();
    let mut number = |key: ObjKey| -> Result<ObjectId> {
        match key {
            ObjKey::Base {
                blob: b,
                obj,
                copy: 0,
            } if b == blob => Ok(obj),
            key => match at.get(&key) {
                Some(&n) => Ok(n),
                None => numbers.number(key),
            },
        }
    };
    let mut out = BTreeMap::new();
    let mut done = HashSet::new();
    for part in parts {
        for (key, obj) in &part.objects {
            if !done.insert(*key) {
                continue;
            }
            let n = number(*key)?;
            let mut resolved = |obj: &mut Object| follow(obj, &part.syms, &mut number);
            let obj = match obj {
                SymObject::Raw(r) if r.src == *src && r.id == n => continue,
                // Its number another key's: the source's object, elsewhere.
                SymObject::Raw(r) => (*r.src.pdf().object_unkept(r.id)?).clone(),
                SymObject::Built(obj) => {
                    let mut obj = obj.clone();
                    resolved(&mut obj)?;
                    obj
                }
                SymObject::Deferred(d) => {
                    let mut dict = Object::Dictionary(d.dict.clone());
                    resolved(&mut dict)?;
                    let Object::Dictionary(dict) = dict else {
                        unreachable!("a dictionary resolves to one")
                    };
                    let data = (d.data)().with_context(|| format!("making {key:?}"))?;
                    Object::Stream(Stream::new(dict, data))
                }
            };
            if out.insert(n, (*key, obj)).is_some() {
                bail!("two objects would be written at {} {}", n.0, n.1);
            }
        }
    }
    Ok(out)
}

/// `obj` with each placeholder of `syms` the number `number` gives its
/// key; a reference that isn't one stays as it is, the source's.
fn follow(
    obj: &mut Object,
    syms: &Syms,
    number: &mut dyn FnMut(ObjKey) -> Result<ObjectId>,
) -> Result<()> {
    match obj {
        Object::Reference(r) => {
            if let Some(key) = syms.key(*r) {
                *r = number(key)?;
            }
        }
        Object::Array(items) => {
            for o in items {
                follow(o, syms, number)?;
            }
        }
        Object::Dictionary(d) => {
            for (_, o) in d.iter_mut() {
                follow(o, syms, number)?;
            }
        }
        Object::Stream(s) => {
            for (_, o) in s.dict.iter_mut() {
                follow(o, syms, number)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// A group's section over `src`'s bytes: `parts`, its pages' deltas then
/// its frame, numbered by `numbers` and written as an append section in
/// the base's cross-reference kind, encrypted with the source's key when
/// the source is.
pub(crate) fn section(src: &Src, parts: &[&Part], numbers: &mut Numbers) -> Result<Vec<u8>> {
    let objects = numbered(src, parts, numbers)?;
    write_section(src, &objects, numbers)
}

/// `objects` as an append section after `src`'s bytes (`numbers`' base),
/// encrypted with the source's key when it is.
fn write_section(
    src: &Src,
    objects: &BTreeMap<ObjectId, (ObjKey, Object)>,
    numbers: &Numbers,
) -> Result<Vec<u8>> {
    let crypt = src.crypt().map(|c| &**c);
    let update = append::section(&numbers.base, objects, numbers.xref, crypt)?;
    Ok(update.map(|u| u.bytes).unwrap_or_default())
}

/// A view patch of `src`: an append section with only its catalog, its
/// `/OCProperties` showing every layer but `off`. None when it has no
/// layers.
pub(crate) fn patch(
    src: &Src,
    off: &BTreeSet<LayerId>,
    numbers: &Numbers,
) -> Result<Option<Vec<u8>>> {
    let Some(layers) = ocg_patch(src, off) else {
        return Ok(None);
    };
    let mut catalog = src.pdf().catalog()?.clone();
    catalog.set("OCProperties", layers);
    let objects = BTreeMap::from([(
        numbers.base.root,
        (ObjKey::Catalog, Object::Dictionary(catalog)),
    )]);
    write_section(src, &objects, numbers).map(Some)
}

/// The catalog's keys a group of its own keeps: what its pages are drawn
/// with (§10.1's standalone groups).
const STANDALONE_CATALOG: [&[u8]; 4] = [b"Type", b"Pages", b"AcroForm", b"OCProperties"];

/// A group over `src`, a damaged base, as a file of its own: `parts`
/// numbered as [`section`] numbers them, the catalog keeping only what
/// draws, and every object of the source they reach, decrypted, but other
/// pages and page tree nodes, which nothing in it lists. Unencrypted, and
/// unpacked with a cross-reference table, as it never leaves memory: only
/// PDFium reads it, to draw.
pub(crate) fn standalone(src: &Src, parts: &[&Part], numbers: &mut Numbers) -> Result<Vec<u8>> {
    let mut objects = numbered(src, parts, numbers)?;
    let root = (objects.iter())
        .find(|(_, (key, _))| *key == ObjKey::Catalog)
        .map(|(&at, _)| at)
        .context("the group has no catalog")?;
    if let Some((_, Object::Dictionary(catalog))) = objects.get_mut(&root) {
        let drop: Vec<Vec<u8>> = (catalog.iter().map(|(k, _)| k.clone()))
            .filter(|k| !STANDALONE_CATALOG.contains(&k.as_slice()))
            .collect();
        for k in drop {
            catalog.remove(&k);
        }
    }
    // The source's objects at their own numbers, as the overlay wrote them
    // as they are, and what the written objects reach.
    let pdf = src.pdf();
    let blob = src.id();
    let mut queue: VecDeque<ObjectId> = VecDeque::new();
    for (_, obj) in objects.values() {
        refs(obj, &mut |r| queue.push_back(r));
    }
    let mut read: BTreeMap<ObjectId, Object> = BTreeMap::new();
    for part in parts {
        for (_, obj) in &part.objects {
            if let SymObject::Raw(r) = obj
                && r.src == *src
                && !objects.contains_key(&r.id)
            {
                queue.push_back(r.id);
            }
        }
    }
    while let Some(id) = queue.pop_front() {
        if objects.contains_key(&id) || read.contains_key(&id) {
            continue;
        }
        let Ok(obj) = pdf.object_unkept(id) else {
            continue;
        };
        let node = match &*obj {
            Object::Dictionary(d) => d.get(b"Type").and_then(Object::as_name).ok(),
            _ => None,
        };
        if matches!(node, Some(b"Page" | b"Pages")) || full::left_out(&obj) {
            continue;
        }
        refs(&obj, &mut |r| queue.push_back(r));
        read.insert(id, (*obj).clone());
    }
    let version = (parts.iter().map(|p| p.version)).fold(PdfVersion::of(src), PdfVersion::max);
    let theirs = read.into_iter().map(|(at, obj)| {
        let key = ObjKey::Base {
            blob,
            obj: at,
            copy: 0,
        };
        (at, (key, obj))
    });
    let bodies: BTreeMap<ObjectId, (ObjKey, Object)> = objects.into_iter().chain(theirs).collect();
    let mut trailer = Dictionary::new();
    trailer.set("Root", root);
    let layout = full::Layout {
        trailer: &trailer,
        kind: XrefKind::Table,
        version,
        crypt: None,
        encrypt_at: (0, 0),
        xref_from: 0,
        pack: None,
    };
    let bodies = (bodies.into_iter())
        .map(|(at, (key, obj))| Ok((at, full::Body::Object(Cow::Owned(obj), key))));
    Ok(full::write_bodies(bodies, &layout, &NoProgress)?.0)
}

/// Each reference in `obj`, to `to`.
fn refs(obj: &Object, to: &mut dyn FnMut(ObjectId)) {
    match obj {
        Object::Reference(r) => to(*r),
        Object::Array(items) => items.iter().for_each(|o| refs(o, to)),
        Object::Dictionary(d) => d.iter().for_each(|(_, o)| refs(o, to)),
        Object::Stream(s) => s.dict.iter().for_each(|(_, o)| refs(o, to)),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use lopdf::dictionary;

    use super::*;

    /// A dictionary weighs at least a slot for each entry and what each
    /// holds: a delta's page dictionary, its content stream's bytes and a
    /// nested array all count, so the delta cache's bound follows what it
    /// holds (it once counted 128 bytes an object, a tenth of a header
    /// page's delta).
    #[test]
    fn an_object_weighs_its_entries_and_what_they_hold() {
        let small = Object::Dictionary(dictionary! { "Type" => "Page" });
        let names: Vec<Object> = (0..100)
            .map(|i| Object::Name(format!("n{i}").into()))
            .collect();
        let page = Object::Dictionary(dictionary! {
            "Type" => "Page",
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "Names" => names,
        });
        assert!(held(&page) >= 100 * (size_of::<Object>() + CHUNK) + held(&small));
        let stream = Object::Stream(Stream::new(dictionary! {}, vec![0; 10_000]));
        assert!(held(&stream) > 10_000);
        let mut wide = Dictionary::new();
        for i in 0..64 {
            wide.set(format!("K{i}"), i);
        }
        let entry = size_of::<(u64, Vec<u8>, Object)>();
        assert!(held(&Object::Dictionary(wide)) >= 64 * (entry + CHUNK));
    }
}
