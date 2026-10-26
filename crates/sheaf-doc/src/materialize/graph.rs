//! A source loaded whole, as lopdf sees it (M10.5 §11.2): what a Rewrite
//! edits the primary as ([`rewrite_graph`]).
//!
//! A Rewrite's graph is the primary loaded eagerly with the emitted
//! objects put in at their numbers, for `keep_only` to prune against the
//! pages kept: it's ported to a graph of keys only if the Rewrite gate
//! fails (proposal 37). Which pages use each of its objects
//! ([`page_uses`]) is what a packed write groups them by
//! ([`group`](crate::serialize::group)).

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use anyhow::{Context, Result, bail};
use lopdf::{Document as Pdf, Object, ObjectId, Stream};

use super::emit::{Emission, SymObject, Syms};
use super::key::ObjKey;
use super::number::Numbers;
use crate::serialize::group::Use;
use crate::source::Source;
use crate::store::BlobId;

/// A source as lopdf sees it, loaded eagerly and decrypted as
/// [`crate::file::load_pdf`] does: its blob's bytes, opened with its
/// password, so a file replaced since (by a save in place) doesn't
/// matter. A save that writes it encrypts with the source's
/// [`crypt`](crate::source::Source::crypt).
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

/// Which of `pages` use each object, by number, `refs` giving each
/// reference an object holds: what each page reaches, walked from the
/// pages in order, never into a page or tree node in `stop` but the
/// page walked from, so another page's link, an annotation's `/P` or a
/// page's `/Parent` brings in nothing of another page's. An object no
/// page reaches isn't listed: its use is [`Use::None`].
///
/// The walk from page `i` goes into an object it meets unused, which
/// becomes `Page(i)`, or one another page alone uses, which becomes
/// `Shared`; it stops at one that's `Page(i)` or `Shared` already. An
/// object is gone into only as its use changes, which is twice at most,
/// so however many pages share what an object reaches, that's walked
/// twice: by the first page that reaches it, and by the second, which
/// turns all of it shared. The walk keeps its own stack, so a deep chain
/// can't overflow the thread's.
pub fn page_uses(
    pages: &[ObjectId],
    stop: &HashSet<ObjectId>,
    refs: impl Fn(ObjectId, &mut dyn FnMut(ObjectId)),
) -> HashMap<u32, Use> {
    let mut uses = HashMap::new();
    let mut stack = Vec::new();
    for (i, &page) in (0..).zip(pages) {
        meet(&mut uses, &mut stack, page, i);
        while let Some(id) = stack.pop() {
            refs(id, &mut |r| {
                if r == page || !stop.contains(&r) {
                    meet(&mut uses, &mut stack, r, i);
                }
            });
        }
    }
    uses
}

/// Object `id`, met on page `i`'s walk, put on `stack` to go into when
/// it's unused or another page's alone, and its use changed.
fn meet(uses: &mut HashMap<u32, Use>, stack: &mut Vec<ObjectId>, id: ObjectId, i: u32) {
    let used = uses.entry(id.0).or_insert(Use::None);
    *used = match *used {
        Use::None => Use::Page(i),
        Use::Page(j) if j != i => Use::Shared,
        Use::Page(_) | Use::Shared => return,
    };
    stack.push(id);
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

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use lopdf::dictionary;

    use super::*;
    use crate::materialize::append::refs;
    use crate::serialize::group::group;

    /// `objects`' references, each object's walks counted in `walked`.
    fn counted<'a>(
        objects: &'a HashMap<ObjectId, Object>,
        walked: &'a RefCell<HashMap<ObjectId, usize>>,
    ) -> impl Fn(ObjectId, &mut dyn FnMut(ObjectId)) + 'a {
        move |id, to| {
            *walked.borrow_mut().entry(id).or_default() += 1;
            if let Some(obj) = objects.get(&id) {
                refs(obj, to);
            }
        }
    }

    /// 1,000 pages share a chain 1,000 objects deep: it's walked twice,
    /// by the first page and the second, and each page once, so the work
    /// doesn't grow with the pages that share it. The tree's root, which
    /// every page names as its `/Parent`, isn't walked.
    #[test]
    fn page_uses_descends_each_object_at_most_twice() {
        let root = (5000, 0);
        let pages: Vec<ObjectId> = (1..=1000).map(|n| (n, 0)).collect();
        let mut objects = HashMap::new();
        for (i, &page) in pages.iter().enumerate() {
            let next = pages.get(i + 1).copied().unwrap_or(pages[0]);
            let dest = Object::Array(vec![next.into(), "Fit".into()]);
            let page_dict =
                dictionary! { "Parent" => root, "Contents" => (1001, 0), "Dest" => dest };
            objects.insert(page, Object::Dictionary(page_dict));
        }
        for n in 1001..=2000 {
            let link = dictionary! { "Next" => (n + 1, 0), "Page" => (1, 0) };
            objects.insert((n, 0), Object::Dictionary(link));
        }
        let kids: Vec<Object> = pages.iter().map(|&p| p.into()).collect();
        objects.insert(root, Object::Dictionary(dictionary! { "Kids" => kids }));
        let stop: HashSet<ObjectId> = pages.iter().copied().chain([root]).collect();
        let walked = RefCell::default();
        let uses = page_uses(&pages, &stop, counted(&objects, &walked));
        let walked = walked.into_inner();
        for (i, page) in (0..).zip(&pages) {
            assert_eq!(uses[&page.0], Use::Page(i));
            assert_eq!(walked[page], 1, "page {i}");
        }
        for n in 1001..=2000 {
            assert_eq!(uses[&n], Use::Shared, "{n}");
            assert_eq!(walked[&(n, 0)], 2, "{n}");
        }
        assert_eq!(uses[&2001], Use::Shared, "named, if not there");
        assert_eq!(walked[&(2001, 0)], 2);
        assert!(!uses.contains_key(&root.0) && !walked.contains_key(&root));
        assert_eq!(uses.len(), 2001);
    }

    /// Each object is used by the one page that reaches it, by several, or
    /// by none, never through another page or the tree; the grouping then
    /// puts page 1's in a container of their own, and the rest in page
    /// order, the shared, then what no page reaches.
    #[test]
    fn page_uses_sorts_objects_by_their_pages() {
        let r = |n: u32| Object::Reference((n, 0));
        let page = |contents, resources, annots: Vec<Object>| {
            Object::Dictionary(dictionary! {
                "Type" => "Page",
                "Parent" => r(1),
                "Contents" => r(contents),
                "Resources" => r(resources),
                "Annots" => annots,
            })
        };
        let objects: HashMap<ObjectId, Object> = [
            (
                50,
                dictionary! { "Type" => "Catalog", "Pages" => r(1) }.into(),
            ),
            (1, dictionary! { "Kids" => vec![r(2), r(3), r(4)] }.into()),
            (2, page(10, 11, vec![r(12)])),
            (3, page(20, 11, vec![r(21)])),
            (4, page(30, 31, vec![])),
            (10, Object::Integer(10)),
            (11, dictionary! { "Font" => r(13) }.into()),
            (
                12,
                dictionary! { "P" => r(2), "Dest" => vec![r(3), "Fit".into()] }.into(),
            ),
            (13, dictionary! { "Back" => r(11) }.into()),
            (20, Object::Integer(20)),
            (
                21,
                dictionary! { "P" => r(3), "Parent" => r(1), "Popup" => r(22) }.into(),
            ),
            (22, dictionary! { "Parent" => r(21) }.into()),
            (30, Object::Integer(30)),
            (31, dictionary! { "Loop" => r(32) }.into()),
            (
                32,
                dictionary! { "Loop" => r(31), "Contents" => r(30) }.into(),
            ),
            (40, dictionary! { "Orphan" => r(10) }.into()),
        ]
        .into_iter()
        .map(|(n, obj)| ((n, 0), obj))
        .collect();
        let pages = [(2, 0), (3, 0), (4, 0)];
        let stop: HashSet<ObjectId> = pages.iter().copied().chain([(1, 0)]).collect();
        let uses = page_uses(&pages, &stop, |id, to| {
            if let Some(obj) = objects.get(&id) {
                refs(obj, to);
            }
        });
        let mut got: Vec<(u32, Use)> = uses.into_iter().collect();
        got.sort_unstable_by_key(|&(n, _)| n);
        let want = [
            (2, Use::Page(0)),
            (3, Use::Page(1)),
            (4, Use::Page(2)),
            (10, Use::Page(0)),
            (11, Use::Shared),
            (12, Use::Page(0)),
            (13, Use::Shared),
            (20, Use::Page(1)),
            (21, Use::Page(1)),
            (22, Use::Page(1)),
            (30, Use::Page(2)),
            (31, Use::Page(2)),
            (32, Use::Page(2)),
        ];
        assert_eq!(got, want);
        let all = objects.keys().map(|id| {
            let used = got.iter().find(|(n, _)| *n == id.0).map(|&(_, u)| u);
            (id.0, used.unwrap_or(Use::None))
        });
        assert_eq!(
            group(all),
            [
                vec![2, 10, 12],
                vec![3, 20, 21, 22, 4, 30, 31, 32, 11, 13, 1, 40, 50]
            ]
        );
    }
}
