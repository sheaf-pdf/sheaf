//! A source loaded whole, as lopdf sees it (M10.5 §11.2): what a Rewrite
//! edits the primary as ([`rewrite_graph`]), and what the old writers copy
//! other files' pages out of.
//!
//! A Rewrite's graph is the primary loaded eagerly with the emitted
//! objects put in at their numbers, for `keep_only` to prune against the
//! pages kept: it's ported to a graph of keys only if the Rewrite gate
//! fails (proposal 37).

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use anyhow::{Context, Result, bail};
use lopdf::{Document as Pdf, Object, ObjectId, Stream};

use super::copy::inherited_attrs;
use super::emit::{Emission, SymObject, Syms};
use super::key::ObjKey;
use super::number::Numbers;
use crate::source::Source;
use crate::store::BlobId;

/// A source as lopdf sees it, decrypted as [`crate::file::load_pdf`]
/// does: its blob's bytes, opened with its password, so a file replaced
/// since (by a save in place) doesn't matter. A save that writes it
/// encrypts with the source's [`crypt`](crate::source::Source::crypt).
pub fn load(info: &crate::SourceInfo) -> Result<Pdf> {
    eager(&info.src).with_context(|| format!("loading {}", info.name.display()))
}

/// [`load`]: `src`, loaded eagerly.
pub fn eager(src: &Source) -> Result<Pdf> {
    crate::file::load_pdf_mem(src.bytes(), src.password())
}

/// A Rewrite's objects: the primary's, the emitted ones over them.
pub struct Graph {
    pub pdf: Pdf,
    /// Each emitted object's key, by where it is, and each primary object
    /// moved from a number past what a file may use; any other object is
    /// the primary's own, copy 0 at its number.
    pub keys: BTreeMap<ObjectId, ObjKey>,
    /// The streams whose data is made as they're written, by where they
    /// are: the graph holds their dictionaries.
    pub deferred: BTreeMap<ObjectId, super::emit::Deferred>,
    /// The primary's objects at their own numbers that name a moved one,
    /// which now name it where it went: written anew.
    pub followed: BTreeSet<ObjectId>,
    /// How many primary objects were moved.
    pub moved: usize,
}

impl Graph {
    /// The key of the object at `id`.
    pub fn key(&self, id: ObjectId, primary: BlobId) -> ObjKey {
        self.keys.get(&id).copied().unwrap_or(ObjKey::Base {
            blob: primary,
            obj: id,
            copy: 0,
        })
    }

    /// The graph with each object at its number in `numbers`, which has
    /// every object's ([`super::number::dense`]): references follow, one
    /// to an object the graph hasn't got becomes null, and an object given
    /// another's number is gone. Every object then has a key, the
    /// primary's own named as such.
    pub fn renumber(&mut self, numbers: &BTreeMap<ObjectId, ObjectId>, primary: BlobId) {
        let objects = std::mem::take(&mut self.pdf.objects);
        let mut keys = BTreeMap::new();
        let mut deferred = BTreeMap::new();
        self.followed = (self.followed.iter())
            .filter_map(|id| numbers.get(id).copied())
            .collect();
        for (id, mut obj) in objects {
            let at = numbers[&id];
            if self.pdf.objects.contains_key(&at) {
                continue;
            }
            follow(&mut obj, &mut |r| numbers.get(&r).copied());
            keys.insert(at, self.key(id, primary));
            if let Some(d) = self.deferred.remove(&id) {
                deferred.insert(at, d);
            }
            self.pdf.objects.insert(at, obj);
        }
        let mut trailer = Object::Dictionary(std::mem::take(&mut self.pdf.trailer));
        follow(&mut trailer, &mut |r| numbers.get(&r).copied());
        if let Object::Dictionary(trailer) = trailer {
            self.pdf.trailer = trailer;
        }
        self.keys = keys;
        self.deferred = deferred;
        self.pdf.max_id = self.pdf.objects.keys().map(|id| id.0).max().unwrap_or(0);
    }

    /// Each primary object numbered past what a file may use moved to the
    /// number `numbers` gives it, under its key, and every reference to
    /// such a number following it there, or null where nothing is: what
    /// a full write leaves out of them goes. Nothing to do, and nothing
    /// walked, when there's none. How many were moved.
    fn relocate(&mut self, primary: BlobId, numbers: &Numbers) -> Result<usize> {
        use crate::serialize::NUMBERS;
        let objects = &mut self.pdf.objects;
        let high: Vec<ObjectId> = objects.range((NUMBERS, 0)..).map(|(&id, _)| id).collect();
        if high.is_empty() {
            return Ok(0);
        }
        let mut moved = HashMap::new();
        let mut taken = Vec::new();
        for id in high {
            let obj = objects.remove(&id).expect("listed above");
            if crate::serialize::full::left_out(&obj) {
                continue;
            }
            let key = ObjKey::Base {
                blob: primary,
                obj: id,
                copy: 0,
            };
            let at = numbers
                .get(&key)
                .with_context(|| format!("object {} {} has no number", id.0, id.1))?;
            moved.insert(id, at);
            taken.push((at, key, obj));
        }
        let mut to = |r: ObjectId| match r.0 < NUMBERS {
            true => Some(r),
            false => moved.get(&r).copied(),
        };
        for (&id, obj) in objects.iter_mut() {
            let mut named = false;
            follow(obj, &mut |r| {
                named |= r.0 >= NUMBERS;
                to(r)
            });
            if named {
                self.followed.insert(id);
            }
        }
        let count = taken.len();
        for (at, key, mut obj) in taken {
            follow(&mut obj, &mut to);
            objects.insert(at, obj);
            self.keys.insert(at, key);
        }
        let mut trailer = Object::Dictionary(std::mem::take(&mut self.pdf.trailer));
        follow(&mut trailer, &mut to);
        if let Object::Dictionary(trailer) = trailer {
            self.pdf.trailer = trailer;
        }
        Ok(count)
    }
}

/// The Rewrite graph of `e` over `primary`, which is `pdf` loaded eagerly
/// ([`eager`]): without its `/Encrypt` (a save writes the encryption it
/// keeps), each object numbered past what a file may use moved to its
/// number in `numbers` ([`super::number::beyond`]), and each emitted
/// object at its number in `numbers`, its placeholders resolved. The
/// trailer's `/Root` is the emitted catalog.
pub fn rewrite_graph(
    e: Emission,
    numbers: &Numbers,
    primary: &Source,
    mut pdf: Pdf,
) -> Result<Graph> {
    if let Ok(at) = pdf.trailer.get(b"Encrypt").and_then(Object::as_reference) {
        pdf.objects.remove(&at);
    }
    pdf.trailer.remove(b"Encrypt");
    if let Some(crypt) = primary.crypt() {
        pdf.objects.remove(&crypt.at());
    }
    let mut graph = Graph {
        pdf,
        keys: BTreeMap::new(),
        deferred: BTreeMap::new(),
        followed: BTreeSet::new(),
        moved: 0,
    };
    graph.moved = graph.relocate(primary.id(), numbers)?;
    let syms = &e.syms;
    for (key, obj) in e.objects {
        let at = numbers
            .get(&key)
            .context("an emitted object has no number")?;
        let obj = match obj {
            SymObject::Built(mut obj) => {
                resolve(&mut obj, syms, numbers);
                obj
            }
            SymObject::Deferred(d) => {
                let mut dict = Object::Dictionary(d.dict.clone());
                resolve(&mut dict, syms, numbers);
                let Object::Dictionary(dict) = dict else {
                    unreachable!("a dictionary resolves to one")
                };
                graph.deferred.insert(at, d);
                Object::Stream(Stream::new(dict, Vec::new()))
            }
            SymObject::Raw(r) if r.src.id() == primary.id() && r.id == at => continue,
            SymObject::Raw(r) => bail!(
                "object {} {} of another file can't be written as it is",
                r.id.0,
                r.id.1
            ),
        };
        graph.pdf.objects.insert(at, obj);
        graph.keys.insert(at, key);
    }
    let catalog = numbers.get(&ObjKey::Catalog).context("no catalog")?;
    graph.pdf.trailer.set("Root", catalog);
    // What keep_only adds is numbered past everything here, and past every
    // number the primary used (its `/Encrypt`'s, which a save keeps, too),
    // but for what a full write leaves out, which nothing names.
    let last = (graph.pdf.objects.iter())
        .filter(|(_, obj)| !crate::serialize::full::left_out(obj))
        .map(|(id, _)| id.0)
        .max()
        .unwrap_or(0);
    graph.pdf.max_id = last.max(numbers.start() - 1);
    Ok(graph)
}

/// The primary's pages the Rewrite doesn't keep (not in `tree`), let go of
/// the annotations it writes as the primary objects they are (`owned`):
/// what `keep_only` cuts as a dropped page's annotation is one no kept page
/// writes, so a widget a deleted page shared with a kept one stays its
/// field's. The objects it changed: each such page's, or its `/Annots`
/// array's.
pub fn release_written(
    pdf: &mut Pdf,
    primary: &Source,
    tree: &HashSet<ObjectId>,
    owned: &BTreeMap<ObjKey, ObjectId>,
) -> HashSet<ObjectId> {
    let written: HashSet<ObjectId> = owned.values().copied().collect();
    let mut changed = HashSet::new();
    for page in primary.page_ids().filter(|id| !tree.contains(id)) {
        let annots = match pdf
            .get_dictionary(page)
            .ok()
            .and_then(|d| d.get(b"Annots").ok())
        {
            Some(Object::Reference(r)) => *r,
            Some(Object::Array(_)) => page,
            _ => continue,
        };
        let list = match pdf.objects.get_mut(&annots) {
            Some(Object::Array(list)) => list,
            Some(Object::Dictionary(d)) => match d.get_mut(b"Annots") {
                Ok(Object::Array(list)) => list,
                _ => continue,
            },
            _ => continue,
        };
        let was = list.len();
        list.retain(|a| a.as_reference().map_or(true, |r| !written.contains(&r)));
        if list.len() != was {
            changed.insert(annots);
        }
    }
    changed
}

/// `obj` with each placeholder the number `numbers` gives its key, or
/// null for a key it hasn't numbered.
fn resolve(obj: &mut Object, syms: &Syms, numbers: &Numbers) {
    follow(obj, &mut |r| {
        let key = syms.key(r);
        debug_assert!(key.is_some(), "{r:?} isn't a placeholder");
        numbers.get(&key?)
    });
}

/// `obj` with each reference `to` gives a number for at that number, and
/// any other null.
fn follow(obj: &mut Object, to: &mut dyn FnMut(ObjectId) -> Option<ObjectId>) {
    match obj {
        Object::Reference(r) => {
            *obj = match to(*r) {
                Some(id) => Object::Reference(id),
                None => Object::Null,
            }
        }
        Object::Array(items) => items.iter_mut().for_each(|o| follow(o, to)),
        Object::Dictionary(d) => d.iter_mut().for_each(|(_, o)| follow(o, to)),
        Object::Stream(s) => s.dict.iter_mut().for_each(|(_, o)| follow(o, to)),
        _ => {}
    }
}

/// A loaded source's pages, and what each inherits, read before anything
/// is re-parented.
pub(crate) struct Loaded {
    /// Page object ids in page order.
    pub(crate) pages: Vec<ObjectId>,
    /// Inherited attributes per page, as found in the original tree.
    pub(crate) inherited: HashMap<ObjectId, Vec<(Vec<u8>, Object)>>,
}

impl Loaded {
    pub(crate) fn from_pdf(pdf: &Pdf) -> Result<Self> {
        let pages: Vec<ObjectId> = pdf.get_pages().into_values().collect();
        let mut inherited = HashMap::new();
        for &page in &pages {
            inherited.insert(page, inherited_attrs(pdf, page));
        }
        Ok(Self { pages, inherited })
    }
}
