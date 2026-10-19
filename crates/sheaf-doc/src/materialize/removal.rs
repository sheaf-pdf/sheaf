//! The removal check (M10.5 §11.9): an Append keeps every byte before it,
//! so what its section writes over or unlinks stays readable in the file's
//! earlier revision. Before a byte of a section is written, the check
//! finds whether text it takes out of the file's newest revision is text
//! the state being saved no longer holds: a removal the planner missed
//! (§7.1), which a Rewrite writes cleanly instead, or a signed document
//! asks about.
//!
//! - **What it examines**: each object the section writes over, as the
//!   saved file has it (read lazily, a restored document's pending
//!   sections after its bytes on disk), and what that object referred to
//!   that nothing the section writes refers to now, followed through:
//!   unlinked, though an object the section leaves may still refer to it.
//!   A page or a page tree node is followed only as a node's kid, the
//!   catalog never, and nor are `/Parent`, `/P` and `/IRT`, which point
//!   back up a tree or across to another annotation.
//! - **Derived objects** are judged on their record inputs, the saved
//!   state's against the target's: a sheaf mark's
//!   objects, and what they reach, on what the mark says; the blocks drawn
//!   over a page (OCR text, the header and footer) on its OCR words and
//!   header text. A resource sheaf drew with is its user's, and says
//!   nothing of its own.
//! - **Other objects** are judged on their text, as a [`TextBag`]: their
//!   strings; what content draws through its fonts (a page's, a form's, a
//!   tiling pattern's, a Type 3 glyph's); scripts, rich text, XFA and XMP
//!   as words; images and embedded files by hash. Their bag must be within
//!   the bag of everything the section writes.
//! - **What can't be read** counts as removed.
//!
//! What it finds stops the Append (a veto): a derived object's is a
//! misclassification, logged as [`RemovalMisclassified`] and counted, and
//! another's is counted apart ([`vetoes`]). In "Append anyway" mode it's
//! reported instead, and a recovery sidecar's section is flagged by it;
//! but a derived object whose record inputs the save's path didn't say
//! remove text (`Section::said`) is still a misclassification, logged and
//! counted, which stops the Append unless the reader agreed that what the
//! check finds stays readable too.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::fmt;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};

use lopdf::{Dictionary, LazyPdf, Object, ObjectId, Stream};

use super::append::refs;
use super::key::ObjKey;
use super::record::SavedSide;
use crate::annot::{AnnotId, AnnotKind};
use crate::content::{Concat, FontDecoders, ReadObjects, ResCx, TextBag, text_bag};
use crate::file::text_string;
use crate::model::{AnnotBody, AnnotRec, DocState, FormRef, Mark, PageId};
use crate::prim::Key;
use crate::source::Limits;
use crate::store::BlobId;

/// What the check does with a removal it finds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// It stops the Append.
    Veto,
    /// It stops the Append, which the reader chose to make though the
    /// file's signatures don't allow it ("Append anyway" to an ask that
    /// said nothing stays readable): they agreed to that, not to removed
    /// text staying readable, which the save asks about again.
    Anyway,
    /// "Append anyway": the reader chose to keep what the save's path
    /// says it removes readable in the earlier revision, and it's
    /// reported. A derived object whose record inputs it didn't say
    /// remove text is a misclassification, which stops the Append, as
    /// [`Mode::Anyway`] does, to ask again.
    Report,
    /// "Append anyway" to an ask that said what the removal check found
    /// stays readable too: whatever it finds is reported.
    ReportAll,
    /// A recovery sidecar's section (§11.10), which is never the file:
    /// what it finds flags the sidecar, for the save that writes the
    /// section to judge.
    Recovery,
}

/// A section's object whose text, as the saved file has it, the target
/// hasn't got: its key, whether sheaf derived it, and for a derived one,
/// whether the save's path said its record inputs remove or replace text
/// ([`Section::said`]): a misclassification when it didn't.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Found {
    pub key: ObjKey,
    pub derived: bool,
    pub said: bool,
}

impl Found {
    /// Whether it's a misclassification: a derived object the save's path
    /// didn't say removes text.
    pub fn misclassified(&self) -> bool {
        self.derived && !self.said
    }
}

/// A derived object whose record inputs the check found text removed
/// from, which the planner judged an Append: what the corpus run counts,
/// never an edit's error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RemovalMisclassified {
    pub key: ObjKey,
}

impl fmt::Display for RemovalMisclassified {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "RemovalMisclassified {{ key: {:?} }}", self.key)
    }
}

/// How many misclassifications the check found in this process, each
/// stopping an Append or reported by one the reader agreed to (`derived`),
/// and how many Appends it stopped on another's object (`source`, for
/// tuning).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Vetoes {
    pub derived: usize,
    pub source: usize,
}

static DERIVED: AtomicUsize = AtomicUsize::new(0);
static SOURCE: AtomicUsize = AtomicUsize::new(0);

/// The vetoes so far, in this process.
pub fn vetoes() -> Vetoes {
    Vetoes {
        derived: DERIVED.load(Ordering::Relaxed),
        source: SOURCE.load(Ordering::Relaxed),
    }
}

#[cfg(test)]
thread_local! {
    static HERE: std::cell::Cell<Vetoes> = const {
        std::cell::Cell::new(Vetoes { derived: 0, source: 0 })
    };
}

/// For tests, which run in parallel: the vetoes so far on this thread.
#[cfg(test)]
pub(crate) fn vetoes_here() -> Vetoes {
    HERE.get()
}

/// `found`, stopping an Append, or a misclassification an Append anyway
/// reports: logged when it's a misclassification, and counted.
pub(crate) fn count(found: Found) {
    let derived = found.misclassified();
    if derived {
        log::warn!("{}", RemovalMisclassified { key: found.key });
        DERIVED.fetch_add(1, Ordering::Relaxed);
    } else {
        SOURCE.fetch_add(1, Ordering::Relaxed);
    }
    #[cfg(test)]
    HERE.set(Vetoes {
        derived: HERE.get().derived + usize::from(derived),
        source: HERE.get().source + usize::from(!derived),
    });
}

/// An Append's section, for the check.
pub(crate) struct Section<'a> {
    /// The saved file as its record has it: on disk, then a restored
    /// document's pending sections.
    pub old: &'a LazyPdf,
    /// What the section writes, by number.
    pub objects: &'a BTreeMap<ObjectId, (ObjKey, Object)>,
    /// What the section's trailer names: the catalog, and `/Info`.
    pub trailer: &'a [ObjectId],
    pub saved: &'a SavedSide,
    pub target: &'a DocState,
    /// The primary's blob: its objects the file holds at their numbers.
    pub primary: BlobId,
    /// Keys whose old objects may go (live redraws, slice 17).
    pub allowed: &'a [ObjKey],
    /// The slots the save's path says remove or replace text, when the
    /// reader agreed that what it lists stays readable (an Append anyway):
    /// a derived object judged on their values in the two states is what
    /// they agreed to, and the rest a misclassification. Empty otherwise.
    pub said: &'a BTreeSet<Key>,
    pub lim: &'a Limits,
}

/// What `section` would leave readable that the target hasn't got: None
/// when the old text it examines is all the target's still.
pub(crate) fn check(section: &Section) -> Option<Found> {
    let mut walk = Walk::new(section);
    walk.run();
    walk.found()
}

/// The check as a test oracle on a Rewrite (§11.9): what `out`, the whole
/// file a Rewrite of `input` wrote, took out of the saved file, whose
/// bytes are `on_disk`, that the target hasn't got, as though every object
/// it writes were a section written over the file. None when the old text
/// it examines is all in the new file still: so a Rewrite the planner gave
/// for a removal is one this finds, unless what went is drawn elsewhere
/// too. Every object a Rewrite writes is one the file's map numbers, or the
/// primary's own at its number.
#[cfg(any(test, feature = "fixtures"))]
pub fn of_rewrite(
    input: &super::SaveInput,
    out: &super::Output,
    on_disk: std::sync::Arc<[u8]>,
) -> anyhow::Result<Option<Found>> {
    use anyhow::Context as _;
    let saved = (input.saved.as_ref()).context("nothing says what the saved file holds")?;
    let primary = &(input.sources.get(&input.primary))
        .context("the primary isn't registered")?
        .src;
    let password = saved.record.crypt.as_ref().and(primary.password());
    let lim = primary.limits();
    let old = LazyPdf::open(on_disk, password, &lim.reader).context("reading the saved file")?;
    let bytes: std::sync::Arc<[u8]> = std::sync::Arc::from(out.bytes.as_slice());
    let new = LazyPdf::open(bytes, password, &lim.reader).context("reading the Rewrite")?;
    let keys: HashMap<ObjectId, ObjKey> = (out.record.map.keys.iter())
        .map(|(&key, &at)| (at, key))
        .collect();
    let mut objects = BTreeMap::new();
    for id in new.ids() {
        let key = keys.get(&id).copied().unwrap_or(ObjKey::Base {
            blob: primary.id(),
            obj: id,
            copy: 0,
        });
        objects.insert(id, (key, new.get(id)?.clone()));
    }
    let root = (saved.record.map.keys.get(&ObjKey::Catalog)).context("the file has no catalog")?;
    let trailer: Vec<ObjectId> = [Some(*root), saved.record.roots.info]
        .into_iter()
        .flatten()
        .collect();
    Ok(check(&Section {
        old: &old,
        objects: &objects,
        trailer: &trailer,
        saved,
        target: &input.target,
        primary: primary.id(),
        allowed: &[],
        said: &BTreeSet::new(),
        lim,
    }))
}

/// How an object is reached.
#[derive(Debug, Clone)]
enum Use {
    /// By reference, or written over.
    Object,
    /// As a kid of a page tree node.
    Kid,
    /// As content drawn with these resources: a page's, or a Type 3 font's
    /// glyphs'.
    Content(Option<Object>),
    /// As text in bytes: a script, rich text, XFA.
    Text,
}

/// An old object to examine: how it's reached, and the page whose
/// dictionary referred to it, by key.
struct Item {
    id: ObjectId,
    how: Use,
    page: Option<PageId>,
}

/// The check under way.
struct Walk<'a> {
    s: &'a Section<'a>,
    /// What the section writes, or refers to, by number: not unlinked.
    linked: HashSet<u32>,
    /// The saved file's keys by number, found when first asked.
    keys: OnceLock<HashMap<u32, ObjKey>>,
    seen: HashSet<u32>,
    /// The sheaf marks whose old objects were examined, each with the key
    /// of the first.
    marks: BTreeMap<AnnotId, ObjKey>,
    /// The pages whose old blocks were examined, each with the first's
    /// key.
    pages: BTreeMap<PageId, ObjKey>,
    /// The other old objects' text, in the order they were examined.
    bags: Vec<(ObjKey, TextBag)>,
    /// The first old object that couldn't be read.
    unread: Option<ObjKey>,
}

impl<'a> Walk<'a> {
    fn new(s: &'a Section<'a>) -> Walk<'a> {
        let mut linked: HashSet<u32> = s.trailer.iter().map(|id| id.0).collect();
        for (id, (_, obj)) in s.objects {
            linked.insert(id.0);
            refs(obj, &mut |r| {
                linked.insert(r.0);
            });
        }
        Walk {
            s,
            linked,
            keys: OnceLock::new(),
            seen: HashSet::new(),
            marks: BTreeMap::new(),
            pages: BTreeMap::new(),
            bags: Vec::new(),
            unread: None,
        }
    }

    /// Each object the section writes over, then what it unlinks.
    fn run(&mut self) {
        let mut next: VecDeque<Item> = (self.s.objects.keys())
            .map(|&id| Item {
                id,
                how: Use::Object,
                page: None,
            })
            .collect();
        while let Some(item) = next.pop_front() {
            self.examine(item, &mut next);
        }
    }

    /// Examines `item`'s old object, putting what it unlinks on `next`. A
    /// sheaf mark's, or a page's block, is judged on the record's inputs,
    /// and what it reaches is its too; a resource sheaf drew with is its
    /// user's.
    fn examine(&mut self, item: Item, next: &mut VecDeque<Item>) {
        let s = self.s;
        if self.seen.contains(&item.id.0) {
            return;
        }
        let obj = match s.old.object_unkept(item.id) {
            Ok(obj) => obj,
            // Not in the file: nothing of it is readable there.
            Err(lopdf::Error::MissingXrefEntry | lopdf::Error::ObjectNotFound(_)) => return,
            Err(_) => {
                self.seen.insert(item.id.0);
                self.unread.get_or_insert(self.key(item.id));
                return;
            }
        };
        // A page is what the tree holds: reached otherwise, it's a link's
        // or an annotation's, and still in the file.
        let kind = name_of(dict_of(&obj), b"Type");
        let page = matches!(kind, Some(b"Page" | b"Pages")) && !matches!(item.how, Use::Kid);
        if !s.objects.contains_key(&item.id) && (page || kind == Some(b"Catalog")) {
            return;
        }
        self.seen.insert(item.id.0);
        let key = self.key(item.id);
        if s.allowed.contains(&key) {
            return;
        }
        if let Some(a) = self.mark_of(&key) {
            self.marks.entry(a).or_insert(key);
            return;
        }
        match (key, item.page) {
            (ObjKey::Res(_) | ObjKey::ResPart(..) | ObjKey::Shared(_), _) => return,
            (ObjKey::Block(_), Some(page)) => {
                self.pages.entry(page).or_insert(key);
                return;
            }
            _ => {}
        }
        let resources = |d: &Dictionary| inherited(s.old, d);
        match bag(s.old, &obj, &item.how, s.lim, &resources) {
            Some(bag) if bag.is_empty() => {}
            Some(bag) => self.bags.push((key, bag)),
            None => {
                self.unread.get_or_insert(key);
            }
        }
        let page = match key {
            ObjKey::Page(id) => Some(id),
            _ => None,
        };
        for (id, how) in children(&obj, &item.how, &resources) {
            if !self.linked.contains(&id.0) && !self.seen.contains(&id.0) {
                next.push_back(Item { id, how, page });
            }
        }
    }

    /// The key of the saved file's object `id`: its map's, a sheaf mark's
    /// the saved state reads at its own number, or the primary's object.
    fn key(&self, id: ObjectId) -> ObjKey {
        let keys = self.keys.get_or_init(|| {
            let saved = &self.s.saved;
            let mut keys: HashMap<u32, ObjKey> = (saved.record.map.keys.iter())
                .map(|(&k, &at)| (at.0, k))
                .collect();
            for (a, rec) in saved.state.annots() {
                if let AnnotBody::Mark(m) = &rec.body
                    && let Some(r) = m.src.as_ref().filter(|r| r.src.id() == self.s.primary)
                {
                    keys.entry(r.id.0).or_insert(ObjKey::Annot(a));
                }
            }
            keys
        });
        keys.get(&id.0).copied().unwrap_or(ObjKey::Base {
            blob: self.s.primary,
            obj: id,
            copy: 0,
        })
    }

    /// The sheaf mark of the saved state whose object `key` is.
    fn mark_of(&self, key: &ObjKey) -> Option<AnnotId> {
        let (ObjKey::Annot(a) | ObjKey::AnnotPart(a, _)) = *key else {
            return None;
        };
        let rec = self.s.saved.state.annot(a)?;
        matches!(rec.body, AnnotBody::Mark(_)).then_some(a)
    }

    /// What the walk found, derived objects first: a mark that says less
    /// now, then a page with less drawn over it, the first the save's path
    /// didn't say (a misclassification) before the first it did; then what
    /// couldn't be read, then the first object whose text the section
    /// hasn't got. What a mark says is its slot's to say; what's drawn
    /// over a page is its recognized text, its page's slot's, and the
    /// header and footer's, a generator's (a page's place and the count
    /// change that too, which no slot says).
    fn found(&self) -> Option<Found> {
        let s = self.s;
        let (was, now) = (&s.saved.state, s.target);
        let derived = |key: ObjKey, said: bool| Found {
            key,
            derived: true,
            said,
        };
        let mut said = None;
        for (&a, &key) in &self.marks {
            if says(was.annot(a)).missing_from(&says(now.annot(a))) > 0 {
                if !s.said.contains(&Key::Annot(a)) {
                    return Some(derived(key, false));
                }
                said.get_or_insert(key);
            }
        }
        let (then, after) = (Drawn::new(was), Drawn::new(now));
        let header = (s.said.iter()).any(|k| matches!(k, Key::Generator(_)));
        for (&page, &key) in &self.pages {
            let ([ocr, hf], [a, b]) = (then.on(page), after.on(page));
            let mut drawn = a;
            drawn.extend(&b);
            let mut all = ocr.clone();
            all.extend(&hf);
            if all.missing_from(&drawn) == 0 {
                continue;
            }
            let mut unsaid = TextBag::default();
            if !s.said.contains(&Key::Page(page)) {
                unsaid.extend(&ocr);
            }
            if !header {
                unsaid.extend(&hf);
            }
            if unsaid.missing_from(&drawn) > 0 {
                return Some(derived(key, false));
            }
            said.get_or_insert(key);
        }
        if let Some(key) = said {
            return Some(derived(key, true));
        }
        let source = |key: ObjKey| Found {
            key,
            derived: false,
            said: false,
        };
        if let Some(key) = self.unread {
            return Some(source(key));
        }
        if self.bags.is_empty() {
            return None;
        }
        let new = match written(self.s) {
            Ok(bag) => bag,
            Err(key) => return Some(source(key)),
        };
        let mut old = TextBag::default();
        for (_, bag) in &self.bags {
            old.extend(bag);
        }
        if old.missing_from(&new) == 0 {
            return None;
        }
        let first = self.bags.iter().find(|(_, bag)| bag.missing_from(&new) > 0);
        Some(source(first.unwrap_or(&self.bags[0]).0))
    }
}

/// The text of everything `s` writes: Err with the key of an object of it
/// that can't be read.
fn written(s: &Section) -> Result<TextBag, ObjKey> {
    let joined = Joined {
        section: s.objects,
        old: s.old,
    };
    let resources = |d: &Dictionary| inherited(&joined, d);
    // How each object written is reached from the others: content and text
    // by the key that holds it.
    let mut uses: HashMap<ObjectId, Use> = HashMap::new();
    let mut next: Vec<(ObjectId, Use)> = (s.objects.keys()).map(|&id| (id, Use::Object)).collect();
    while let Some((id, how)) = next.pop() {
        let Some((_, obj)) = s.objects.get(&id) else {
            continue;
        };
        for (kid, how) in children(obj, &how, &resources) {
            if s.objects.contains_key(&kid)
                && !matches!(how, Use::Object | Use::Kid)
                && !uses.contains_key(&kid)
            {
                uses.insert(kid, how.clone());
                next.push((kid, how));
            }
        }
    }
    let mut out = TextBag::default();
    for (id, (key, obj)) in s.objects {
        let how = uses.get(id).unwrap_or(&Use::Object);
        out.extend(&bag(&joined, obj, how, s.lim, &resources).ok_or(*key)?);
    }
    Ok(out)
}

/// The text of `obj`, reached as `how`, whose objects `objects` reads, a
/// page's resources by `resources`: its strings, and a stream's text by
/// what it is. None when it can't be read.
fn bag(
    objects: &impl ReadObjects,
    obj: &Object,
    how: &Use,
    lim: &Limits,
    resources: &dyn Fn(&Dictionary) -> Option<Object>,
) -> Option<TextBag> {
    let mut bag = TextBag::default();
    strings(obj, &mut bag);
    if let Object::Stream(stream) = obj {
        bag.extend(&stream_text(objects, stream, how, lim, resources)?);
    }
    Some(bag)
}

/// A stream's text, as what it is: an image or an embedded file by hash;
/// a form's, a tiling pattern's, or content's (`how`) drawn through its
/// fonts; XMP, or text `how` says it holds, as words. Nothing for the rest
/// (fonts, CMaps, color profiles, functions). None when it can't be read.
fn stream_text(
    objects: &impl ReadObjects,
    stream: &Stream,
    how: &Use,
    lim: &Limits,
    resources: &dyn Fn(&Dictionary) -> Option<Object>,
) -> Option<TextBag> {
    let dict = &stream.dict;
    let (kind, subtype) = (
        name_of(Some(dict), b"Type"),
        name_of(Some(dict), b"Subtype"),
    );
    let mut bag = TextBag::default();
    if subtype == Some(b"Image") || kind == Some(b"EmbeddedFile") {
        bag.add_hash(sheaf_core::checksum(&stream.content));
        return Some(bag);
    }
    let pattern = dict.get(b"PatternType").and_then(Object::as_i64).ok() == Some(1);
    let content = match how {
        _ if subtype == Some(b"Form") || pattern => Some(resources(dict)),
        Use::Content(res) => Some(res.clone()),
        _ => None,
    };
    let text = matches!(how, Use::Text) || kind == Some(b"Metadata") || subtype == Some(b"XML");
    if content.is_none() && !text {
        return Some(bag);
    }
    let data = stream
        .decompressed_content_with_limit(lim.content.decompressed)
        .ok()?;
    match content {
        Some(res) => {
            let res = res.as_ref();
            let fonts = FontDecoders::read(objects, res, lim);
            let cx = ResCx::read(objects, res);
            bag = text_bag(&Concat::new(&[&data]), &cx, lim, &fonts).ok()?;
        }
        None => bag.add(&text_string(&data)),
    }
    Some(bag)
}

/// Adds the text of every string in `obj` to `bag`, a stream's
/// dictionary's but not its data.
fn strings(obj: &Object, bag: &mut TextBag) {
    match obj {
        Object::String(s, _) => bag.add(&text_string(s)),
        Object::Array(items) => items.iter().for_each(|o| strings(o, bag)),
        Object::Dictionary(d) => d.iter().for_each(|(_, o)| strings(o, bag)),
        Object::Stream(s) => s.dict.iter().for_each(|(_, o)| strings(o, bag)),
        _ => {}
    }
}

/// What `obj`, reached as `how`, refers to, each with how it's reached: a
/// page's `/Contents` as content with its resources (`resources`), a Type
/// 3 font's glyphs with the font's, scripts, rich text and XFA as text, a
/// node's kids as kids, and whatever an array or a dictionary reached as
/// content or text holds as that too. `/Parent`, `/P` and `/IRT` aren't
/// followed.
fn children(
    obj: &Object,
    how: &Use,
    resources: &dyn Fn(&Dictionary) -> Option<Object>,
) -> Vec<(ObjectId, Use)> {
    let mut out = Vec::new();
    let held = matches!(how, Use::Content(_) | Use::Text) && !matches!(obj, Object::Stream(_));
    let Some(dict) = dict_of(obj).filter(|_| !held) else {
        let how = if held { how.clone() } else { Use::Object };
        refs(obj, &mut |r| out.push((r, how.clone())));
        return out;
    };
    let page = name_of(Some(dict), b"Type") == Some(b"Page");
    let type3 = name_of(Some(dict), b"Subtype") == Some(b"Type3");
    for (key, value) in dict.iter() {
        let how = match key.as_slice() {
            b"Parent" | b"P" | b"IRT" => continue,
            b"Contents" if page => Use::Content(resources(dict)),
            b"CharProcs" if type3 => Use::Content(dict.get(b"Resources").ok().cloned()),
            b"JS" | b"RC" | b"XFA" => Use::Text,
            b"Kids" => Use::Kid,
            _ => Use::Object,
        };
        refs(value, &mut |r| out.push((r, how.clone())));
    }
    out
}

/// The dictionary `obj` is, or a stream's.
fn dict_of(obj: &Object) -> Option<&Dictionary> {
    match obj {
        Object::Dictionary(d) => Some(d),
        Object::Stream(s) => Some(&s.dict),
        _ => None,
    }
}

/// The name `dict` has at `key`.
fn name_of<'d>(dict: Option<&'d Dictionary>, key: &[u8]) -> Option<&'d [u8]> {
    dict?.get(key).and_then(Object::as_name).ok()
}

/// A page's `/Resources`, or the nearest of its nodes', as `objects` reads
/// them.
fn inherited(objects: &impl ReadObjects, page: &Dictionary) -> Option<Object> {
    let mut at = page;
    // A loop of nodes is no deeper than this.
    for _ in 0..64 {
        if let Ok(res) = at.get(b"Resources") {
            return Some(res.clone());
        }
        let parent = at.get(b"Parent").and_then(Object::as_reference).ok()?;
        at = objects.object(parent).ok()?.as_dict().ok()?;
    }
    None
}

/// The section's objects, then the saved file's.
struct Joined<'a> {
    section: &'a BTreeMap<ObjectId, (ObjKey, Object)>,
    old: &'a LazyPdf,
}

impl ReadObjects for Joined<'_> {
    fn object(&self, id: ObjectId) -> lopdf::Result<&Object> {
        match self.section.get(&id) {
            Some((_, obj)) => Ok(obj),
            None => ReadObjects::object(self.old, id),
        }
    }

    fn read<R>(&self, id: ObjectId, read: impl FnOnce(&Object) -> R) -> lopdf::Result<R> {
        match self.section.get(&id) {
            Some((_, obj)) => Ok(read(obj)),
            None => ReadObjects::read(self.old, id, read),
        }
    }
}

/// What annotation `rec` says, as words: a sheaf mark's contents, author
/// and name, its kind's own text, and the file or stamp it holds, by hash;
/// another tool's comment's contents, author and name. Nothing for a
/// widget, or none.
fn says(rec: Option<&AnnotRec>) -> TextBag {
    let mut bag = TextBag::default();
    match rec.map(|r| &r.body) {
        Some(AnnotBody::Mark(m)) => mark_says(m, &mut bag),
        Some(AnnotBody::Foreign(f)) => {
            let s = &f.summary;
            for text in [&s.contents, &s.author, &s.name] {
                bag.add(text);
            }
        }
        Some(AnnotBody::Widget(_)) | None => {}
    }
    bag
}

/// Adds what mark `m` says to `bag` ([`says`]).
fn mark_says(m: &Mark, bag: &mut TextBag) {
    let a = &m.annot;
    for text in [&a.contents, &a.author, &a.name()] {
        bag.add(text);
    }
    match &a.kind {
        AnnotKind::Stamp { glyph, .. } => bag.add(glyph),
        AnnotKind::Connector { label, .. } => bag.add(label),
        AnnotKind::Tape { tape, .. } => {
            for line in &tape.lines {
                if let crate::tape::Row::Title(title) = &line.row {
                    bag.add(title);
                }
                if let Some(value) = line.row.value() {
                    bag.add(&format!("{value} {}", line.row.symbol()));
                }
                bag.add(&line.note);
            }
        }
        AnnotKind::Replace { struck, .. } => bag.add(struck),
        AnnotKind::Reply { parent } => bag.add(parent),
        AnnotKind::Status {
            parent,
            model,
            state,
        } => {
            for text in [parent, model, state] {
                bag.add(text);
            }
        }
        AnnotKind::PageStamp {
            stamp, name, text, ..
        } => {
            bag.add(stamp);
            bag.add(name);
            bag.add(text.as_deref().unwrap_or_default());
        }
        AnnotKind::TextStamp { text, .. } => bag.add(text),
        AnnotKind::PageBorder { status, .. } => bag.add(status),
        AnnotKind::FileAttachment { file, name, .. } => {
            bag.add(file);
            bag.add(name);
        }
        _ => {}
    }
    if let Some(file) = &m.file {
        bag.add_hash(blob_hash(file.id()));
    }
    match &m.stamp {
        Some(FormRef::Blob(blob)) => bag.add_hash(blob_hash(blob.id())),
        Some(FormRef::Base(r)) => {
            let mut bytes = blob_hash(r.src.id()).to_be_bytes().to_vec();
            bytes.extend(r.id.0.to_be_bytes());
            bytes.extend(r.id.1.to_be_bytes());
            bag.add_hash(sheaf_core::checksum(&bytes));
        }
        None => {}
    }
}

/// A blob's identity as one hash.
fn blob_hash(id: BlobId) -> u128 {
    let mut bytes = id.hash.to_be_bytes().to_vec();
    bytes.extend(id.len.to_be_bytes());
    sheaf_core::checksum(&bytes)
}

/// What sheaf draws over the pages of a state, as words ([`Drawn::on`]):
/// the Bates numbers the header and footer counts found once.
struct Drawn<'a> {
    st: &'a DocState,
    bates: OnceLock<Vec<Option<u64>>>,
}

impl<'a> Drawn<'a> {
    fn new(st: &'a DocState) -> Drawn<'a> {
        Drawn {
            st,
            bates: OnceLock::new(),
        }
    }

    /// The words drawn over page `id`: its OCR text, then the header and
    /// footer's text on it. Nothing for a page the state hasn't got.
    fn on(&self, id: PageId) -> [TextBag; 2] {
        let st = self.st;
        let (mut ocr, mut bag) = (TextBag::default(), TextBag::default());
        let Some(pos) = st.pos_of(id) else {
            return [ocr, bag];
        };
        if let Some(layer) = st.ocr_text(id) {
            for word in &layer.words {
                ocr.add(&word.text);
            }
        }
        if let Some(hf) = crate::stamps::drawn_over(st, id)
            && let Some(geom) = st.geom(id)
        {
            let bates = self.bates.get_or_init(|| {
                let order: Vec<PageId> = st.order().iter().copied().collect();
                hf.bates_numbers(&order)
            });
            let facts = crate::stamps::PageFacts {
                index: pos,
                count: st.page_count(),
                bates: bates.get(pos).copied().flatten(),
            };
            for placed in hf.layout(geom.shown(), &facts) {
                bag.add(&placed.text);
            }
        }
        [ocr, bag]
    }
}
