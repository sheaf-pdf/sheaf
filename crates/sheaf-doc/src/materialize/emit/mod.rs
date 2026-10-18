//! The emitter (M10.5 §11.3): the objects a state is written as, derived
//! from its records, each named by what it is ([`ObjKey`]) and never by
//! the number it's written at. A built object refers to others by
//! placeholder ([`Syms`]), which numbering ([`super::number`]) resolves;
//! the same state, sources and previous map give the same objects in the
//! same order, from any thread.
//!
//! - [`pages`]: each page's dictionary, from its base page's, read lazily;
//!   a derived copy's page as the page it was made from ([`Shown`]).
//! - [`content`]: what sheaf draws over a page, OCR text and the header
//!   and footer, through the legacy installers.
//! - [`tree`]: the balanced page tree a Rewrite writes.
//! - [`catalog`]: the catalog by its ownership table (§9.1), and
//!   `/PageLabels`.
//! - [`outline`]: the bookmarks, in place by provenance.
//! - [`annots`]: each page's `/Annots`, from its records.
//! - [`form`]: the field tree and the form's dictionary, from the records
//!   and [`FormDoc`](crate::model::FormDoc), with save policy's transforms.
//!
//! Some keys are written at the number of the primary's object they are
//! (§11.5's model-owned identities, [`Emission::owned`]): copy 0 of a
//! primary page; an annotation, field or bookmark read from the primary
//! (copy 0 of it); the outline's root and the form's dictionary. They're
//! claimed before anything is emitted, so any key can refer to them by
//! number; one of them nothing changed is the file's object as it is
//! ([`SymObject::Raw`]).

pub mod annots;
pub mod catalog;
pub mod content;
pub mod form;
pub mod outline;
pub mod pages;
pub mod tree;

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt;
use std::sync::Arc;

use anyhow::{Context as _, Result};
use in_progress::Progress;
use lopdf::{Dictionary, Object, ObjectId};

use super::copy::{Closure, Scope};
use super::key::ObjKey;
use super::plan::{Note, SaveTransform};
use super::record::SaveInput;
use crate::annot::{Annot, AnnotId, StampForm};
use crate::content::sink::{self, Sink};
use crate::content::{Res, ResBody, ResId, Reserved};
use crate::model::{AnnotBody, FormRef, PageId, SourceId, SourceInfo};
use crate::source::ObjRef;
use crate::store::{BlobId, Src};

/// The generation a placeholder reference carries. No object in use has
/// it (an entry at 65,535 is never used again), so a reference the
/// emitter didn't make can't pass for one.
pub const PLACEHOLDER: u16 = u16::MAX;

/// What a state is written as: its objects by key, and what the write
/// needs to know of them.
pub struct Emission {
    /// In the order they were emitted: the pages in order, their tree,
    /// the catalog and what it names, then what copies reached.
    pub objects: Vec<(ObjKey, SymObject)>,
    /// What each placeholder names ([`Syms`]).
    pub syms: Syms,
    /// The PDF version the emitted objects need: the primary's at least.
    pub version: PdfVersion,
    /// Annotations of the base the emission leaves out. None yet: the
    /// pages' `/Annots` are the base's.
    pub removed_annots: Vec<ObjRef>,
    /// Keys whose old content an Append may drop: live redraws (slice
    /// 10's removal check). None yet.
    pub allowed_drops: Vec<ObjKey>,
    /// What the reader should be told about the written file.
    pub notes: Vec<Note>,
    /// The keys written at the number of the primary's object they are,
    /// wherever they sit now: copy 0 of a primary page (bug 3), a bookmark
    /// read from the primary. No two share a number.
    pub owned: BTreeMap<ObjKey, ObjectId>,
    /// Keys that name another's object, by the key they name: a widget
    /// merged with its field is its field's object. The save record maps
    /// both to its number.
    pub aliases: Vec<(ObjKey, ObjKey)>,
    /// The form's keys: its dictionary, fields and widgets. And whether
    /// any was written anew, or the state doesn't model the primary's
    /// form: else the form is the file's as it was, and checking it again
    /// finds nothing new.
    pub form_keys: Vec<ObjKey>,
    pub form_touched: bool,
}

impl Emission {
    /// The keys of the pages and of the nodes of their tree: what a
    /// Rewrite keeps every other page out of.
    pub fn tree_keys(&self) -> impl Iterator<Item = ObjKey> + '_ {
        (self.objects.iter())
            .map(|(key, _)| *key)
            .filter(|key| matches!(key, ObjKey::Page(_) | ObjKey::PagesNode(_)))
    }
}

/// An emitted object.
pub enum SymObject {
    /// An object of a source as the source holds it, at its own number:
    /// the primary's, which a Rewrite's graph holds already.
    Raw(ObjRef),
    /// An object made here, its references placeholders.
    Built(Object),
    /// A stream whose data is made only as it's written (§11.1), so peak
    /// memory stays near the largest such stream.
    Deferred(Deferred),
}

/// A stream made as it's written: its dictionary now, its data later.
#[derive(Clone)]
pub struct Deferred {
    /// Its references placeholders; `/Length` is the writer's.
    pub dict: Dictionary,
    pub data: Arc<dyn Fn() -> Result<Vec<u8>> + Send + Sync>,
}

/// What an emission covers. Slice 10's Appends and recovery sections
/// emit only some keys, and slice 13's overlays some pages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmitScope {
    /// The whole document.
    All,
}

/// What an emission is written onto.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmitBase {
    /// A whole new file over the primary's objects: every page placed in
    /// a new tree, the root objects' unowned keys from the primary.
    /// Slice 10 adds the saved file an Append writes after.
    Rewrite,
}

/// Placeholder references: `(i, PLACEHOLDER)` names the `i`th key.
#[derive(Debug, Default, Clone)]
pub struct Syms {
    keys: Vec<ObjKey>,
    index: HashMap<ObjKey, u32>,
}

impl Syms {
    /// A reference to `key`'s object.
    pub fn placeholder(&mut self, key: ObjKey) -> Object {
        Object::Reference(self.id(key))
    }

    /// The placeholder that names `key`.
    pub fn id(&mut self, key: ObjKey) -> ObjectId {
        let next = self.keys.len() as u32;
        let i = *self.index.entry(key).or_insert_with(|| {
            self.keys.push(key);
            next
        });
        (i, PLACEHOLDER)
    }

    /// The key placeholder `r` names: None for a reference that isn't
    /// one.
    pub fn key(&self, (i, generation): ObjectId) -> Option<ObjKey> {
        (generation == PLACEHOLDER)
            .then(|| self.keys.get(i as usize).copied())
            .flatten()
    }

    /// Every key a placeholder names.
    pub fn keys(&self) -> &[ObjKey] {
        &self.keys
    }
}

/// A PDF version, as a header or the catalog's `/Version` gives it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PdfVersion {
    pub major: u8,
    pub minor: u8,
}

impl PdfVersion {
    /// Cross-reference streams and object streams (ISO 32000-1 7.5.8).
    pub const XREF_STREAMS: PdfVersion = PdfVersion::new(1, 5);
    /// AES encryption (`/AESV2`).
    pub const AES: PdfVersion = PdfVersion::new(1, 6);
    /// CIDFonts (ISO 32000-1 9.7.4), which an OCR layer's text is in.
    pub const CID_FONTS: PdfVersion = PdfVersion::new(1, 2);
    /// Marked-content artifacts with property lists (14.8.2.2), which a
    /// header and footer are drawn as.
    pub const ARTIFACTS: PdfVersion = PdfVersion::new(1, 4);

    pub const fn new(major: u8, minor: u8) -> PdfVersion {
        PdfVersion { major, minor }
    }

    /// `text` read as `1.7` is: None for anything else.
    pub fn parse(text: &[u8]) -> Option<PdfVersion> {
        let text = std::str::from_utf8(text).ok()?;
        let (major, minor) = text.trim().split_once('.')?;
        Some(PdfVersion::new(major.parse().ok()?, minor.parse().ok()?))
    }
}

impl fmt::Display for PdfVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}

/// What a page is written as: the page of a file whose dictionary, keys
/// and annotations it is, and as which copy of it; for a derived copy's
/// page (a crop, a header stripped, a watermark), the page it was made
/// from (M10 bug 1), with the copy's page what it draws.
#[derive(Clone)]
pub(crate) struct Shown {
    pub(crate) src: Src,
    pub(crate) index: u32,
    /// [`PageDef::copy`](crate::PageDef::copy); for a derived copy's page,
    /// the lowest copy of its original, from its own up, that no other
    /// page is.
    pub(crate) copy: u32,
    /// A derived copy's page: its `/Contents` and `/Resources` are what
    /// the page draws.
    pub(crate) content: Option<(Src, u32)>,
}

/// What the emitters share while they emit.
pub(crate) struct Cx<'a> {
    pub(crate) input: &'a SaveInput,
    /// What save policy does to the form as it's written.
    pub(crate) transforms: &'a [SaveTransform],
    pub(crate) primary: Src,
    /// Every registration's source, by its blob: what a placeholder to a
    /// source's object is read from.
    srcs: HashMap<BlobId, Src>,
    /// What each page is written as.
    pub(crate) shown: HashMap<PageId, Shown>,
    pub(crate) objects: Vec<(ObjKey, SymObject)>,
    pub(crate) syms: Syms,
    pub(crate) closure: Closure,
    pub(crate) version: PdfVersion,
    pub(crate) notes: Vec<Note>,
    /// Each page's Bates number, by its place, when the header and footer
    /// counts them.
    pub(crate) bates: Vec<Option<u64>>,
    /// [`Emission::owned`] so far, and the numbers it holds.
    owned: BTreeMap<ObjKey, ObjectId>,
    claimed: HashSet<ObjectId>,
    /// Resources and blocks pushed already: each is written once.
    made: HashSet<ObjKey>,
    /// The sources whose pages lost their tags, noted once each.
    untagged: BTreeSet<BlobId>,
    /// What the form's emission decided before the pages.
    pub(crate) form: form::Plan,
    /// Each page stamp's page as a form in the file, by what its mark
    /// holds: None when it can't be had.
    pub(crate) stamps: HashMap<FormRef, Option<StampForm>>,
    /// Each attachment's embedded file, by its blob.
    pub(crate) files: HashMap<BlobId, ObjectId>,
    /// Every annotation read from a file, by the object it was read as and
    /// which copy of it it is: what a reply's `/IRT` names.
    pub(crate) by_src: HashMap<(BlobId, ObjectId, u32), AnnotId>,
    /// The connectors of each group, by label, in document order.
    pub(crate) groups: HashMap<String, Vec<(PageId, Annot)>>,
}

impl<'a> Cx<'a> {
    /// `obj` of `scope`'s source, its references placeholders (see
    /// [`Closure::sym`]).
    pub(crate) fn sym(&mut self, obj: &Object, scope: &Scope) -> Object {
        self.closure.sym(&mut self.syms, obj, scope)
    }

    /// A placeholder reference to `key`'s object.
    pub(crate) fn placeholder(&mut self, key: ObjKey) -> Object {
        self.syms.placeholder(key)
    }

    pub(crate) fn push(&mut self, key: ObjKey, obj: Object) {
        self.objects.push((key, SymObject::Built(obj)));
    }

    /// `key` written at `at`, the number of the primary's object it is,
    /// unless another key has it: whether it does now.
    pub(crate) fn own(&mut self, key: ObjKey, at: ObjectId) -> bool {
        let owns = self.claimed.insert(at);
        if owns {
            self.owned.insert(key, at);
        }
        owns
    }

    /// The number `key` is written at as the primary's object it is, when
    /// it is one.
    pub(crate) fn owned_at(&self, key: ObjKey) -> Option<ObjectId> {
        self.owned.get(&key).copied()
    }

    /// Whether the page written as primary object `at` is `page`.
    pub(crate) fn owns(&self, page: PageId, at: ObjectId) -> bool {
        self.owned.get(&ObjKey::Page(page)) == Some(&at)
    }

    /// The number `key` is written at when that's known before numbering:
    /// a primary object's own, or one a key claimed ([`Self::own`]).
    pub(crate) fn number_known(&self, key: ObjKey) -> Option<ObjectId> {
        match key {
            ObjKey::Base { blob, obj, copy: 0 } if blob == self.primary.id() => Some(obj),
            key => self.owned_at(key),
        }
    }

    /// Whether `built`, an object made with placeholders, is `read`, the
    /// primary's object it's written over, as written: the same values,
    /// each placeholder naming the number `read` refers to there. What
    /// lets an object nothing changed be copied as the file has it.
    pub(crate) fn unchanged(&self, built: &Dictionary, read: &Dictionary) -> bool {
        self.same_dict(built, read)
    }

    fn same_dict(&self, built: &Dictionary, read: &Dictionary) -> bool {
        built.len() == read.len()
            && (built.iter()).all(|(k, v)| read.get(k).is_ok_and(|r| self.same(v, r)))
    }

    fn same(&self, built: &Object, read: &Object) -> bool {
        match (built, read) {
            (Object::Reference(p), Object::Reference(r)) => {
                self.syms.key(*p).and_then(|k| self.number_known(k)) == Some(*r)
            }
            (Object::Array(a), Object::Array(b)) => {
                a.len() == b.len() && a.iter().zip(b).all(|(a, b)| self.same(a, b))
            }
            (Object::Dictionary(a), Object::Dictionary(b)) => self.same_dict(a, b),
            (Object::Stream(_), _) | (_, Object::Stream(_)) => false,
            (Object::Reference(_), _) | (_, Object::Reference(_)) => false,
            (a, b) => a == b,
        }
    }

    /// The emitted objects need `version` at least.
    pub(crate) fn needs(&mut self, version: PdfVersion) {
        self.version = self.version.max(version);
    }

    /// Notes once that pages copied from `src`, a tagged file, lost their
    /// tags; `name` is its file's.
    pub(crate) fn untagged(&mut self, src: &Src, name: &str) {
        if !self.untagged.insert(src.id()) {
            return;
        }
        let tagged = (src.pdf().catalog()).is_ok_and(|c| c.has(b"StructTreeRoot"));
        if tagged {
            self.notes.push(Note(format!(
                "Pages copied from {name} lost their tags (the structure screen readers read)."
            )));
        }
    }

    /// The name of the file page `id` came from.
    pub(crate) fn file_name(&self, id: PageId) -> String {
        (self.input.target.def(id))
            .and_then(|def| self.input.sources.get(&def.base.source))
            .and_then(|info| info.name.file_name())
            .map_or_else(
                || "another file".to_string(),
                |n| n.to_string_lossy().into_owned(),
            )
    }

    /// What placeholder `id` names, when it's an object of a source: the
    /// object, references followed, its source and the copy it's read as.
    fn read(&self, id: ObjectId) -> Option<(Arc<Object>, Src, u32)> {
        let ObjKey::Base { blob, obj, copy } = self.syms.key(id)? else {
            return None;
        };
        let src = self.srcs.get(&blob)?.clone();
        let mut read = src.pdf().object_unkept(obj).ok()?;
        for _ in 0..16 {
            match *read {
                Object::Reference(next) => read = src.pdf().object_unkept(next).ok()?,
                _ => return Some((read, src, copy)),
            }
        }
        None
    }

    /// `d`, read from `src` as `copy` of it, its references placeholders.
    fn sym_dict(&mut self, d: &Dictionary, src: &Src, copy: u32, stream: bool) -> Dictionary {
        let scope = Scope { src, copy };
        self.closure.sym_dict(&mut self.syms, d, &scope, stream)
    }

    /// `obj` pushed under `key` unless it was already: the placeholder.
    fn push_once(&mut self, key: ObjKey, obj: impl FnOnce(&mut Self) -> Object) -> ObjectId {
        if self.made.insert(key) {
            let obj = obj(self);
            self.push(key, obj);
        }
        self.syms.id(key)
    }
}

/// The emitter is what the legacy installers draw through: a page
/// dictionary it builds refers to objects by placeholder, so what one
/// names is read from its source as the copy it is, and what they add is
/// keyed by digest, written once a file.
impl Sink for Cx<'_> {
    fn dict(&mut self, id: ObjectId) -> Option<Dictionary> {
        let (obj, src, copy) = self.read(id)?;
        let Object::Dictionary(d) = &*obj else {
            return None;
        };
        Some(self.sym_dict(d, &src, copy, false))
    }

    fn stream_dict(&mut self, id: ObjectId) -> Option<Dictionary> {
        let (obj, src, copy) = self.read(id)?;
        let Object::Stream(s) = &*obj else {
            return None;
        };
        Some(self.sym_dict(&s.dict, &src, copy, true))
    }

    fn array(&mut self, id: ObjectId) -> Option<Vec<Object>> {
        let (obj, src, copy) = self.read(id)?;
        let Object::Array(items) = &*obj else {
            return None;
        };
        let scope = Scope { src: &src, copy };
        Some(items.iter().map(|o| self.sym(o, &scope)).collect())
    }

    fn add(&mut self, obj: Object) -> ObjectId {
        let key = ObjKey::Block(ResId::of(std::slice::from_ref(&obj)).0);
        self.push_once(key, |_| obj)
    }

    fn resource(&mut self, res: &Res) -> ObjectId {
        let objects = match &res.body {
            ResBody::Written(id) => return *id,
            ResBody::Objects(objects) => objects.clone(),
        };
        let key = |i: usize| match i {
            0 => ObjKey::Res(res.id),
            i => ObjKey::ResPart(res.id, i as u32),
        };
        for (i, obj) in objects.iter().enumerate().skip(1) {
            self.push_once(key(i), |cx| part(cx, obj, &key));
        }
        self.push_once(key(0), |cx| part(cx, &objects[0], &key))
    }

    fn reserved(&mut self, resources: Option<&Object>) -> Reserved {
        sink::reserved_shallow(self, resources)
    }
}

/// `obj`, one of a resource's objects, as written: each reference `(i, 0)`
/// among them the placeholder of `key(i)`, its streams compressed.
fn part(cx: &mut Cx, obj: &Object, key: &dyn Fn(usize) -> ObjKey) -> Object {
    fn follow(obj: &mut Object, to: &mut dyn FnMut(usize) -> ObjectId) {
        match obj {
            Object::Reference(r) => *r = to(r.0 as usize),
            Object::Array(items) => items.iter_mut().for_each(|o| follow(o, to)),
            Object::Dictionary(d) => d.iter_mut().for_each(|(_, o)| follow(o, to)),
            Object::Stream(s) => s.dict.iter_mut().for_each(|(_, o)| follow(o, to)),
            _ => {}
        }
    }
    let mut obj = obj.clone();
    follow(&mut obj, &mut |i| cx.syms.id(key(i)));
    if let Object::Stream(s) = &mut obj {
        let _ = s.compress();
    }
    obj
}

/// What each page of `input`'s target is written as ([`Shown`]): itself,
/// or for a derived copy's page the page it was made from, through copies
/// of copies, as the copy whose number no other page of it has.
fn shown(input: &SaveInput) -> HashMap<PageId, Shown> {
    let mut out = HashMap::new();
    let mut taken = HashSet::new();
    let mut derived = Vec::new();
    for (id, def, _) in input.target.pages() {
        let made_from =
            origin(&input.sources, def.base.source, def.base.index).and_then(|(source, index)| {
                let src = &input.sources.get(&source)?.src;
                src.page(index)?;
                Some((src.clone(), index))
            });
        match made_from {
            Some(from) => derived.push((id, def, from)),
            None => {
                taken.insert((def.base.src.id(), def.base.index, def.copy));
                let shown = Shown {
                    src: def.base.src.clone(),
                    index: def.base.index,
                    copy: def.copy,
                    content: None,
                };
                out.insert(id, shown);
            }
        }
    }
    for (id, def, (src, index)) in derived {
        let mut copy = def.copy;
        while !taken.insert((src.id(), index, copy)) {
            copy += 1;
        }
        let content = Some((def.base.src.clone(), def.base.index));
        out.insert(
            id,
            Shown {
                src,
                index,
                copy,
                content,
            },
        );
    }
    out
}

/// The page page `index` of registration `source` was made from, when
/// it's a derived copy's (its `derived` record), through copies of
/// copies: None for a page that isn't a copy's.
fn origin(
    sources: &HashMap<SourceId, Arc<SourceInfo>>,
    source: SourceId,
    index: u32,
) -> Option<(SourceId, u32)> {
    let mut at = None;
    let (mut source, mut index) = (source, index);
    // A chain longer than there are registrations would be a loop.
    for _ in 0..sources.len() {
        let Some(derived) = sources.get(&source).and_then(|s| s.derived.as_ref()) else {
            break;
        };
        let Some(&i) = derived.pages.get(index as usize) else {
            break;
        };
        (source, index) = (derived.from, i);
        at = Some((source, index));
    }
    at
}

/// `input`'s target state as `base` writes it, `scope` of it, with save
/// policy's `transforms` applied, and `p` counting the pages and saying
/// when to stop.
pub fn emit(
    input: &SaveInput,
    scope: EmitScope,
    base: EmitBase,
    transforms: &[SaveTransform],
    p: &dyn Progress,
) -> Result<Emission> {
    let (EmitScope::All, EmitBase::Rewrite) = (scope, base);
    let st = &input.target;
    let primary = (input.sources.get(&input.primary))
        .context("the primary isn't registered")?
        .src
        .clone();
    let version = base_version(&primary);
    let shown = shown(input);
    let mut pages = BTreeMap::new();
    for (id, _, _) in st.pages() {
        let page = &shown[&id];
        if let Some(facts) = page.src.page(page.index) {
            let at = (page.src.id(), facts.id, page.copy);
            pages.entry(at).or_insert(id);
        }
    }
    let order: Vec<PageId> = st.order().iter().copied().collect();
    let bates = match st.header_footer() {
        Some(hf) => hf.bates_numbers(&order),
        None => Vec::new(),
    };
    let srcs = (input.sources.values())
        .map(|info| (info.src.id(), info.src.clone()))
        .collect();
    let mut cx = Cx {
        input,
        transforms,
        closure: Closure::new(primary.id(), pages),
        primary,
        srcs,
        shown,
        objects: Vec::new(),
        syms: Syms::default(),
        version,
        notes: Vec::new(),
        bates,
        owned: BTreeMap::new(),
        claimed: HashSet::new(),
        made: HashSet::new(),
        untagged: BTreeSet::new(),
        form: form::Plan::default(),
        stamps: HashMap::new(),
        files: HashMap::new(),
        by_src: HashMap::new(),
        groups: HashMap::new(),
    };
    claim(&mut cx);
    p.set_total(Some(st.page_count() as u64), crate::PAGES);
    let shape = tree::balanced(&order);
    for (pos, (id, def, geom)) in st.pages().enumerate() {
        p.check()?;
        let parent = ObjKey::PagesNode(shape.leaf[pos]);
        pages::page(&mut cx, id, def, geom, pos, parent)?;
        p.inc(1);
    }
    tree::nodes(&mut cx, &shape);
    catalog::catalog(&mut cx, &order);
    while let Some(next) = cx.closure.next(&mut cx.syms) {
        p.check()?;
        let (key, obj) = next?;
        cx.push(key, obj);
    }
    if cx.closure.unreadable > 0 {
        cx.notes.push(Note(format!(
            "{} objects the copied pages refer to couldn't be read, and were left out",
            cx.closure.unreadable
        )));
    }
    let aliases = cx.form.aliases().collect();
    let form_keys = std::mem::take(&mut cx.form.keys);
    let form_touched = cx.form.touched;
    Ok(Emission {
        objects: cx.objects,
        syms: cx.syms,
        version: cx.version,
        removed_annots: Vec::new(),
        allowed_drops: Vec::new(),
        notes: cx.notes,
        owned: cx.owned,
        aliases,
        form_keys,
        form_touched,
    })
}

/// The primary's numbers the keys that are its objects keep (§11.5),
/// claimed before anything is emitted, in a fixed order, so whichever
/// emitter refers to one first, it's numbered alike: copy 0 of each primary
/// page, the form's nodes ([`form::prepare`]), then each page's
/// annotations read from the primary, copy 0 of them (a widget merged with
/// its field is the field's). And every annotation read from a file, by
/// its object and copy, for replies.
fn claim(cx: &mut Cx) {
    let input = cx.input;
    let st = &input.target;
    let primary = cx.primary.id();
    for (id, _, _) in st.pages() {
        let page = &cx.shown[&id];
        if page.src.id() == primary
            && page.copy == 0
            && let Some(facts) = page.src.page(page.index)
            && facts.repeat == 0
        {
            cx.own(ObjKey::Page(id), facts.id);
        }
    }
    cx.form = form::prepare(cx);
    for (id, _, _) in st.pages() {
        for &aid in st.annot_ids_on(id).into_iter().flatten() {
            let Some(rec) = st.annot(aid) else {
                continue;
            };
            let read = match &rec.body {
                AnnotBody::Mark(m) => m.src.as_ref().map(|r| (r, 0)),
                AnnotBody::Foreign(f) => f.src.as_ref().map(|r| (r, f.copy)),
                AnnotBody::Widget(w) if cx.form.merged_into(aid).is_none() => {
                    let copy = cx.form.copies.get(w.inst);
                    w.src.as_ref().zip(copy)
                }
                AnnotBody::Widget(_) => None,
            };
            let Some((r, copy)) = read else {
                continue;
            };
            cx.by_src.entry((r.src.id(), r.id, copy)).or_insert(aid);
            if copy == 0 && r.src.id() == primary {
                cx.own(ObjKey::Annot(aid), r.id);
            }
        }
    }
}

/// The version `src` says it is: its header's, or its catalog's
/// `/Version` when that's later (ISO 32000-1 7.7.2).
fn base_version(src: &Src) -> PdfVersion {
    let pdf = src.pdf();
    let header = PdfVersion::parse(pdf.version().as_bytes()).unwrap_or(PdfVersion::new(1, 4));
    let catalog = (pdf.catalog().ok())
        .and_then(|c| c.get(b"Version").ok())
        .and_then(|v| v.as_name().ok())
        .and_then(PdfVersion::parse);
    header.max(catalog.unwrap_or(header))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_read_and_order() {
        assert_eq!(PdfVersion::parse(b"1.7"), Some(PdfVersion::new(1, 7)));
        assert_eq!(PdfVersion::parse(b"2.0"), Some(PdfVersion::new(2, 0)));
        assert_eq!(PdfVersion::parse(b"17"), None);
        assert_eq!(PdfVersion::parse(b"1.x"), None);
        assert!(PdfVersion::new(1, 10) > PdfVersion::new(1, 7));
        assert!(PdfVersion::new(2, 0) > PdfVersion::new(1, 7));
        assert_eq!(PdfVersion::new(1, 6).to_string(), "1.6");
    }

    /// A key has one placeholder, which names it, and nothing else is
    /// one.
    #[test]
    fn placeholders_name_their_keys() {
        let mut syms = Syms::default();
        let a = syms.placeholder(ObjKey::Catalog);
        let b = syms.placeholder(ObjKey::Labels);
        assert_eq!(syms.placeholder(ObjKey::Catalog), a);
        assert_ne!(a, b);
        let r = |o: &Object| o.as_reference().unwrap();
        assert_eq!(syms.key(r(&a)), Some(ObjKey::Catalog));
        assert_eq!(syms.key(r(&b)), Some(ObjKey::Labels));
        assert_eq!(syms.key((0, 0)), None);
        assert_eq!(syms.key((9, PLACEHOLDER)), None);
        assert_eq!(syms.keys(), [ObjKey::Catalog, ObjKey::Labels]);
    }
}
