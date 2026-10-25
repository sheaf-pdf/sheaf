//! The emitter (M10.5 §11.3): the objects a state is written as, derived
//! from its records, each named by what it is ([`ObjKey`]) and never by
//! the number it's written at. A built object refers to others by
//! placeholder ([`Syms`]), which numbering ([`super::number`]) resolves;
//! the same state, sources and previous map give the same objects in the
//! same order, from any thread.
//!
//! - [`pages`]: each page's dictionary, from its base page's, read lazily
//!   ([`Shown`]).
//! - [`content`]: what sheaf draws on a page: the generators and its
//!   recognized text around its own content, isolated, with resources of
//!   its own.
//! - [`tree`]: the balanced page tree a Rewrite writes, and the saved
//!   file's tree an Append splices the pages into.
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
//!
//! **An Append** ([`EmitScope::Keys`] onto [`EmitBase::Saved`]) emits only
//! the keys in its scope, the closure of what its path changed
//! (`materialize::deps`), as a Rewrite would emit them, and what they reach
//! that the saved file hasn't got. Only the pages whose dictionaries or
//! annotations are in scope are visited, and the form, the outline and the
//! catalog only when any of theirs is. Of what a visit builds, a key the
//! state derives that's out of scope is left out, its object the saved
//! file's: a placeholder to it is its number there. A resource, a block or
//! a source's object the saved file has already (in its map) is referred
//! to, never built again. What only a left-out object reached is left out
//! too. The root objects' keys the state doesn't own are the saved file's
//! ([`RootValues`](super::record::RootValues)), so a key a save dropped
//! stays gone, and the catalog says `/Version` when the section needs a
//! later one than the file is (§11.7).
//!
//! **An overlay** ([`EmitScope::Pages`] over [`EmitBase::Overlay`], §10.1)
//! writes some pages as a section over their source's own bytes, what
//! PDFium draws them from under a view's inputs ([`ViewInputs`]). Its setup
//! visits only those pages, their annotations and the fields with a widget
//! on them; where a page is, the Bates numbers and which page shows a
//! source's page are the state's own tables. The source is the emission's
//! own, as a save's primary is: copy 0 of its objects is where it is, and
//! copy 0 of its pages and annotations is claimed at their own numbers,
//! while another copy, or another file's object, is built as a copy. The
//! pages hang under one new root that lists only them, each with its four
//! inheritable keys written out and its base page's `/Rotate`
//! ([`RotatePolicy::KeepBase`]), which the render turns by the rest. The
//! catalog is the source's over that root, with a form of the fields with
//! a widget on those pages, and the layers the view turns off. What the
//! view doesn't draw isn't written: sheaf's marks unless PDFium draws
//! them, others' comments and links unless the view shows them, the
//! watermarks, backgrounds, header and footer and recognized text unless it
//! asks for them; and what it
//! hides, or a mark being dragged, is written flagged hidden under its own
//! key. An overlay's frame ([`EmitScope::Frame`]) is the
//! root, the catalog and the form alone, without the pages, so the realizer
//! can emit each page on its own and keep it (its delta,
//! [`EmitScope::Delta`]: the page without them), and build the frame over
//! the pages it puts together.

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

use anyhow::{Context as _, Result, bail};
use in_progress::Progress;
use lopdf::{Dictionary, Object, ObjectId};

use super::copy::{Closure, Pages, Scope};
use super::key::{NameTree, ObjKey};
use super::plan::{Note, Private, SaveTransform, Unheld};
use super::record::{DrawnBlocks, SaveRecord};
use crate::annot::{Annot, AnnotId};
use crate::content::sink::{self, Sink};
use crate::content::{GenId, Res, ResBody, Reserved};
use crate::forms::field::annot_flags;
use crate::model::{AnnotBody, DocState, FormRef, PageId};
use crate::realize::{MarkDraw, ViewInputs};
use crate::serialize::built::StampForm;
use crate::source::{ObjRef, PageObj};
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
    /// The primary's annotations its pages list that the pages written as
    /// those pages leave out, and nothing writes at their numbers: a
    /// flatten's, a delete's. What `keep_only` takes out of what still
    /// names them (the structure tree's object references, §12.2).
    pub removed_annots: Vec<ObjRef>,
    /// The saved file's blocks a save's section may unlink though they're
    /// derived (§11.9): live redraws, each the block a generator drew on a
    /// page that it lays out otherwise now from the same recipe, for where
    /// the page is, the count or its label, or its boxes or turn.
    pub allowed_drops: AllowedDrops,
    /// A save's blocks of the generators on the pages it wrote (§9.1's
    /// `drawn`).
    pub drawn: DrawnBlocks,
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

/// The live redraws a save's section makes (§11.9): what the removal
/// check lets go. A page's block is content-addressed, so pages laid out
/// alike share one, and a redraw on one page says nothing of another's.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AllowedDrops {
    /// The saved file's blocks that may go though they're derived: each a
    /// block whose every page draws its generator again from the recipe it
    /// was drawn from, laid out otherwise, or is gone. Not judged, as the
    /// walk reaches them.
    pub keys: Vec<ObjKey>,
    /// The pages that draw a generator again from the recipe their saved
    /// block of it was drawn from: what it drew there before isn't judged
    /// on them.
    pub redrawn: HashSet<(PageId, GenId)>,
}

impl AllowedDrops {
    /// The blocks of `candidates`, each a saved block of `prior`'s that a
    /// live redraw replaced on some page, that every page of `st` it was on
    /// drew again (`redrawn`), or that `st` has none of.
    fn of(
        st: &DocState,
        prior: Option<&DrawnBlocks>,
        mut candidates: BTreeSet<ObjKey>,
        redrawn: HashSet<(PageId, GenId)>,
    ) -> AllowedDrops {
        if let Some(prior) = prior.filter(|_| !candidates.is_empty()) {
            for (&(page, g), block) in prior {
                if candidates.contains(&block.key)
                    && !redrawn.contains(&(page, g))
                    && st.pos_of(page).is_some()
                {
                    candidates.remove(&block.key);
                }
            }
        }
        AllowedDrops {
            keys: candidates.into_iter().collect(),
            redrawn,
        }
    }
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

/// What an emission covers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmitScope {
    /// The whole document.
    All,
    /// The keys an Append writes: the closure of what its path changed
    /// under what each depends on (`materialize::deps`).
    Keys(Arc<BTreeSet<ObjKey>>),
    /// An overlay's pages, in the order its page tree lists them.
    Pages(Arc<[PageId]>),
    /// An overlay's frame over its pages: the root that lists them, the
    /// catalog over it and the form of the fields with a widget on them,
    /// without the pages themselves, each of which the realizer emits on
    /// its own ([`EmitScope::Delta`]).
    Frame(Arc<[PageId]>),
    /// One page of an overlay without the frame: what [`EmitScope::Pages`]
    /// of it alone writes but the root, the catalog and the form, which
    /// the realizer's delta leaves out for the frame to write.
    Delta(PageId),
}

/// What an emission is written onto.
#[derive(Clone, Copy)]
pub enum EmitBase<'a> {
    /// A whole new file over `primary`'s objects: every page placed in a
    /// new tree, the root objects' unowned keys from the primary. `drawn`
    /// is what the saved file's blocks are, when it's known: a block of it
    /// a generator draws otherwise now from the same recipe is a live
    /// redraw ([`Emission::allowed_drops`]). `private` says whether it
    /// holds sheaf's private data (§15), and `unheld` which annotations
    /// the records don't hold it keeps.
    Rewrite {
        primary: &'a Src,
        drawn: Option<&'a DrawnBlocks>,
        private: Private,
        unheld: Unheld,
    },
    /// The saved file an Append writes after, `primary` the file it was,
    /// as `record` says it is: the node each page written goes under, its
    /// page tree with the target's pages placed in it when that changed it,
    /// and the sources whose objects it has are copied again, their pages
    /// named otherwise now
    /// ([`Closure::relinked`](super::deps::Closure::relinked)).
    Saved {
        primary: &'a Src,
        record: &'a SaveRecord,
        parents: &'a BTreeMap<PageId, ObjKey>,
        tree: Option<&'a tree::Spliced>,
        relinked: &'a BTreeSet<BlobId>,
    },
    /// A source's own bytes, which an overlay's section goes after.
    Overlay(&'a Overlay<'a>),
}

/// What an overlay is written over and for (M10.5 §10.1).
pub struct Overlay<'a> {
    /// The source whose bytes its section goes after: the emission's own,
    /// as a save's primary is.
    pub src: Src,
    /// The view it's drawn for.
    pub inputs: &'a ViewInputs,
    pub rotate: RotatePolicy,
    /// Watermarks' images and pages, kept across the emissions of the
    /// realizer that makes it, when one does.
    pub(crate) arts: Option<&'a content::Arts>,
}

/// What a page's `/Rotate` is written from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RotatePolicy {
    /// Its `Geom`'s, as a save writes it.
    Model,
    /// Its base page's own, the value its file gives it, as an overlay
    /// writes it: the render turns the page by the rest of its turn (its
    /// job's, §5), so nothing is turned twice.
    KeepBase,
}

/// An Append's emission, as the emitters see it.
pub(crate) struct Append<'a> {
    pub(crate) keys: &'a BTreeSet<ObjKey>,
    pub(crate) record: &'a SaveRecord,
    pub(crate) parents: &'a BTreeMap<PageId, ObjKey>,
    pub(crate) tree: Option<&'a tree::Spliced>,
    /// Keys the emission found it must write too: the catalog, for a later
    /// `/Version` or a form or an outline that came or went.
    extra: BTreeSet<ObjKey>,
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

    /// The version `src` says it is: its header's, or its catalog's
    /// `/Version` when that's later (ISO 32000-1 7.7.2).
    pub fn of(src: &crate::source::Source) -> PdfVersion {
        let pdf = src.pdf();
        let header = PdfVersion::parse(pdf.version().as_bytes()).unwrap_or(PdfVersion::new(1, 4));
        let catalog = (pdf.catalog().ok())
            .and_then(|c| c.get(b"Version").ok())
            .and_then(|v| v.as_name().ok())
            .and_then(PdfVersion::parse);
        header.max(catalog.unwrap_or(header))
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
/// and annotations it is, and as which copy of it
/// ([`PageDef::copy`](crate::PageDef::copy)).
#[derive(Clone)]
pub(crate) struct Shown {
    pub(crate) src: Src,
    pub(crate) index: u32,
    pub(crate) copy: u32,
}

/// What the emitters share while they emit.
pub(crate) struct Cx<'a> {
    /// The state written.
    pub(crate) st: &'a DocState,
    /// An Append's scope and saved file: None for a whole file.
    pub(crate) append: Option<Append<'a>>,
    /// An overlay's source and view: None for a save.
    pub(crate) overlay: Option<&'a Overlay<'a>>,
    /// What save policy does to the form as it's written.
    pub(crate) transforms: &'a [SaveTransform],
    /// Whether what it writes holds sheaf's private data: under
    /// [`Private::Strip`], what it draws is unmarked, its marks have
    /// neutral names and no JSON, and the catalog doesn't say the
    /// generators (§15).
    pub(crate) private: Private,
    /// Which annotations of the pages the records don't hold it writes.
    pub(crate) unheld: Unheld,
    pub(crate) primary: &'a Src,
    /// The sources the pages are written from, by their blobs: what a
    /// placeholder to a source's object is read from.
    srcs: HashMap<BlobId, Src>,
    /// What each page is written as.
    pub(crate) shown: HashMap<PageId, Shown>,
    pub(crate) objects: Vec<(ObjKey, SymObject)>,
    pub(crate) syms: Syms,
    pub(crate) closure: Closure<'a>,
    pub(crate) version: PdfVersion,
    pub(crate) notes: Vec<Note>,
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
    /// Each watermark's image, by its file's blob, and page, by its PDF's
    /// blob and the page's index: None when it can't be had.
    pub(crate) arts: HashMap<content::ArtKey, Option<crate::generate::Art>>,
    /// The resources of pages sheaf draws on with its own, by key, and the
    /// names each base page's resources bind, by what they are and the
    /// names dropped from them.
    pub(crate) page_res: HashMap<ObjKey, Dictionary>,
    pub(crate) reserved_of: HashMap<(u128, u64), Reserved>,
    /// The pages the generators covering them and their recognized text
    /// aren't drawn on, whose content can't be read.
    pub(crate) unread: Vec<PageId>,
    /// The pages where others' pagination artifacts that a filter or a
    /// generator's `replaces` takes out stay, as a splice refuses them, and
    /// how many on each.
    pub(crate) stayed: Vec<(PageId, usize)>,
    /// The pages where recognized text sheaf wrote that a filter takes out
    /// stays, as a splice refuses it.
    pub(crate) text_stayed: Vec<PageId>,
    /// The pages where property lists keep the private data sheaf marked
    /// them with that a filter takes out, as they can't be written again
    /// without it, and how many on each.
    pub(crate) lists_kept: Vec<(PageId, usize)>,
    /// Every annotation read from a file, by the object it was read as and
    /// which copy of it it is: what a reply's `/IRT` names.
    pub(crate) by_src: HashMap<(BlobId, ObjectId, u32), AnnotId>,
    /// The connectors of each group, by label, in document order.
    pub(crate) groups: HashMap<String, Vec<(PageId, Annot)>>,
    /// What the saved file's blocks are, for a save that knows: what a
    /// block drawn now replaces.
    pub(crate) prior: Option<&'a DrawnBlocks>,
    /// [`Emission::drawn`] so far.
    pub(crate) drawn: DrawnBlocks,
    /// The saved blocks a live redraw replaced so far, each of them
    /// allowed to go once every page it was on draws it again or is gone
    /// ([`AllowedDrops::keys`]).
    pub(crate) allowed: BTreeSet<ObjKey>,
    /// [`AllowedDrops::redrawn`] so far.
    pub(crate) redrawn: HashSet<(PageId, GenId)>,
    /// The primary's annotations its pages list that the pages written as
    /// copy 0 of them leave out ([`Emission::removed_annots`] once nothing
    /// is written at their numbers).
    pub(crate) left_out: Vec<ObjectId>,
}

impl<'a> Cx<'a> {
    /// An emission of `st` over `primary`'s objects, `closure` copying
    /// what it reaches, `transforms` applied: its pages, their sources and
    /// what it's written onto still to say.
    fn new(
        st: &'a DocState,
        primary: &'a Src,
        closure: Closure<'a>,
        transforms: &'a [SaveTransform],
    ) -> Cx<'a> {
        let version = PdfVersion::of(primary);
        Cx {
            st,
            append: None,
            overlay: None,
            transforms,
            private: Private::Keep,
            unheld: Unheld::Keep,
            primary,
            srcs: HashMap::new(),
            shown: HashMap::new(),
            objects: Vec::new(),
            syms: Syms::default(),
            closure,
            version,
            notes: Vec::new(),
            owned: BTreeMap::new(),
            claimed: HashSet::new(),
            made: HashSet::new(),
            untagged: BTreeSet::new(),
            form: form::Plan::default(),
            stamps: HashMap::new(),
            files: HashMap::new(),
            arts: HashMap::new(),
            page_res: HashMap::new(),
            reserved_of: HashMap::new(),
            unread: Vec::new(),
            stayed: Vec::new(),
            text_stayed: Vec::new(),
            lists_kept: Vec::new(),
            by_src: HashMap::new(),
            groups: HashMap::new(),
            prior: None,
            drawn: HashMap::new(),
            allowed: BTreeSet::new(),
            redrawn: HashSet::new(),
            left_out: Vec::new(),
        }
    }

    /// `obj` of `scope`'s source, its references placeholders (see
    /// [`Closure::sym`]).
    pub(crate) fn sym(&mut self, obj: &Object, scope: &Scope) -> Object {
        self.closure.sym(&mut self.syms, obj, scope)
    }

    /// Whether it leaves sheaf's private data out ([`Private::strips`]).
    pub(crate) fn strips(&self) -> bool {
        self.private.strips()
    }

    /// A placeholder reference to `key`'s object.
    pub(crate) fn placeholder(&mut self, key: ObjKey) -> Object {
        self.syms.placeholder(key)
    }

    pub(crate) fn push(&mut self, key: ObjKey, obj: Object) {
        if self.writes(&key) {
            self.objects.push((key, SymObject::Built(obj)));
        }
    }

    /// `key` as `r`, the primary's object it is, as the file has it.
    pub(crate) fn push_raw(&mut self, key: ObjKey, r: ObjRef) {
        if self.writes(&key) {
            self.objects.push((key, SymObject::Raw(r)));
        }
    }

    /// Whether `key`'s object is emitted: every one in a whole file; in an
    /// Append, a key the state derives when it's in scope, and any other
    /// when the saved file hasn't got it, or it's an object of a source
    /// whose pages it names otherwise now ([`Closure::relinks`]).
    pub(crate) fn writes(&self, key: &ObjKey) -> bool {
        match &self.append {
            None => true,
            Some(a) if key.of_state() => a.keys.contains(key) || a.extra.contains(key),
            Some(a) => a.record.map.get(key).is_none() || self.closure.relinks(key),
        }
    }

    /// An Append writes `key` too, whatever its scope.
    fn write_too(&mut self, key: ObjKey) {
        let append = self.append.as_mut().expect("an Append");
        append.extra.insert(key);
    }

    /// The saved file an Append writes after: None for a whole file.
    pub(crate) fn saved(&self) -> Option<&'a SaveRecord> {
        self.append.as_ref().map(|a| a.record)
    }

    /// What a page's `/Rotate` is written from.
    pub(crate) fn rotate(&self) -> RotatePolicy {
        self.overlay.map_or(RotatePolicy::Model, |o| o.rotate)
    }

    /// Whether an annotation, `body`, is written: by a save, every one; by
    /// an overlay, a widget (the render's flags say whether it's drawn),
    /// one of sheaf's marks when PDFium draws them, and another tool's
    /// comment or link when the view shows them.
    pub(crate) fn draws(&self, body: &AnnotBody) -> bool {
        let Some(o) = self.overlay else {
            return true;
        };
        match body {
            AnnotBody::Mark(_) => o.inputs.marks == MarkDraw::ByEngine,
            AnnotBody::Foreign(_) => o.inputs.foreign,
            AnnotBody::Widget(_) => true,
        }
    }

    /// Whether annotation `aid` is written ([`draws`](Self::draws)).
    pub(crate) fn draws_id(&self, aid: AnnotId) -> bool {
        self.overlay.is_none() || (self.st.annot(aid)).is_some_and(|rec| self.draws(&rec.body))
    }

    /// Whether an annotation of a page the records don't hold is written,
    /// a `widget` or not: a widget as the form's are, anything else as
    /// another tool's comment is ([`draws`](Self::draws)).
    pub(crate) fn draws_unmodeled(&self, widget: bool) -> bool {
        widget || self.overlay.is_none_or(|o| o.inputs.foreign)
    }

    /// Whether an annotation of a page the records don't hold, a `link`, a
    /// `widget` or neither, is kept ([`Unheld`]).
    pub(crate) fn keeps_unheld(&self, link: bool, widget: bool) -> bool {
        self.unheld.keeps(link, widget)
    }

    /// Whether an overlay writes annotation `aid` hidden: its view hides
    /// it, or it's a mark being dragged, whose ghost the screen draws. A
    /// save never does.
    pub(crate) fn hides(&self, aid: AnnotId) -> bool {
        self.overlay
            .is_some_and(|o| o.inputs.hidden.contains(&aid) || o.inputs.dragging.contains(&aid))
    }

    /// `dict`, annotation `aid`'s as written, flagged Hidden (`/F`, ISO
    /// 32000-1 12.5.3) when the overlay [`hides`](Self::hides) it: PDFium
    /// draws it no more, and it keeps its key.
    pub(crate) fn hidden(&self, aid: AnnotId, dict: &mut Dictionary) {
        if !self.hides(aid) {
            return;
        }
        let flags = match dict.get(b"F") {
            Ok(&Object::Integer(f)) => f,
            Ok(&Object::Reference(r)) => (self.read(r))
                .and_then(|(f, _, _)| f.as_i64().ok())
                .unwrap_or(0),
            _ => 0,
        };
        dict.set("F", flags | i64::from(annot_flags::HIDDEN));
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

    /// The name of the file page `id` is written from: the one its source
    /// was first read from.
    pub(crate) fn file_name(&self, id: PageId) -> String {
        (self.shown.get(&id))
            .and_then(|page| page.src.blob().origin_name())
            .map_or_else(|| "another file".to_string(), |n| n.to_string())
    }

    /// What placeholder `id` names, when it's an object of a source: the
    /// object, references followed, its source and the copy it's read as.
    pub(crate) fn read(&self, id: ObjectId) -> Option<(Arc<Object>, Src, u32)> {
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

    /// The stream `make` makes pushed under `key`, its data made as it's
    /// written ([`Deferred`]), unless it was already, or an Append's saved
    /// file has it: the placeholder.
    pub(crate) fn push_deferred(
        &mut self,
        key: ObjKey,
        make: impl FnOnce(&mut Self) -> Deferred,
    ) -> ObjectId {
        if self.writes(&key) && self.made.insert(key) {
            let stream = make(self);
            self.objects.push((key, SymObject::Deferred(stream)));
        }
        self.syms.id(key)
    }

    /// `obj` pushed under `key` unless it was already, or an Append's saved
    /// file has it: the placeholder.
    pub(crate) fn push_once(
        &mut self,
        key: ObjKey,
        obj: impl FnOnce(&mut Self) -> Object,
    ) -> ObjectId {
        if self.writes(&key) && self.made.insert(key) {
            let obj = obj(self);
            self.push(key, obj);
        }
        self.syms.id(key)
    }
}

/// The emitter is where what it draws writes its resources: a dictionary
/// it builds refers to objects by placeholder, so what one names is read
/// from its source as the copy it is, and a resource is keyed by digest,
/// written once a file.
impl Sink for Cx<'_> {
    /// A page's own resources ([`ObjKey::PageRes`]) as they're written.
    fn dict(&mut self, id: ObjectId) -> Option<Dictionary> {
        if let Some(key @ ObjKey::PageRes { .. }) = self.syms.key(id) {
            return self.page_res.get(&key).cloned();
        }
        let (obj, src, copy) = self.read(id)?;
        let Object::Dictionary(d) = &*obj else {
            return None;
        };
        let scope = Scope { src: &src, copy };
        Some(self.closure.sym_dict(&mut self.syms, d, &scope, false))
    }

    fn array(&mut self, id: ObjectId) -> Option<Vec<Object>> {
        let (obj, src, copy) = self.read(id)?;
        let Object::Array(items) = &*obj else {
            return None;
        };
        let scope = Scope { src: &src, copy };
        Some(items.iter().map(|o| self.sym(o, &scope)).collect())
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

/// `data` deflated, as a stream whose `/Filter` is `/FlateDecode` holds
/// it: what a [`Deferred`] stream's data is made as, whatever its size, as
/// its dictionary says so before it's made.
pub(crate) fn deflated(data: &[u8]) -> Result<Vec<u8>> {
    use std::io::Write as _;
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
    z.write_all(data)?;
    Ok(z.finish()?)
}

/// What each page of `st` is written as ([`Shown`]): its base page, as
/// its copy.
pub(crate) fn shown(st: &DocState) -> HashMap<PageId, Shown> {
    (st.pages())
        .map(|(id, def, _)| {
            let shown = Shown {
                src: def.base.src.clone(),
                index: def.base.index,
                copy: def.copy,
            };
            (id, shown)
        })
        .collect()
}

/// The page of `st` each source page's copy is written as, shown as
/// `shown` says, by the source page and the copy: the first in order that
/// shows it. What a reference to a source's page from that source's
/// objects is written as ([`Closure`]).
pub(crate) fn written_pages(
    st: &DocState,
    shown: &HashMap<PageId, Shown>,
) -> BTreeMap<(PageObj, u32), PageId> {
    let mut pages = BTreeMap::new();
    for (id, _, _) in st.pages() {
        let page = &shown[&id];
        if let Some(at) = page.src.page_obj(page.index) {
            pages.entry((at, page.copy)).or_insert(id);
        }
    }
    pages
}

/// `st` as `base` writes it, `scope` of it, with save policy's
/// `transforms` applied, and `p` counting the pages and saying when to
/// stop. A whole file is written as a Rewrite, some keys onto a saved
/// file, and some pages as an overlay over their source.
pub fn emit(
    st: &DocState,
    scope: EmitScope,
    base: EmitBase,
    transforms: &[SaveTransform],
    p: &dyn Progress,
) -> Result<Emission> {
    let (primary, keys, saved, prior, private, unheld) = match (&scope, base) {
        (
            EmitScope::All,
            EmitBase::Rewrite {
                primary,
                drawn,
                private,
                unheld,
            },
        ) => (primary, None, None, drawn, private, unheld),
        (
            EmitScope::Keys(keys),
            EmitBase::Saved {
                primary,
                record,
                parents,
                tree,
                relinked,
            },
        ) => (
            primary,
            Some(&**keys),
            Some((record, parents, tree, relinked)),
            Some(&*record.drawn),
            Private::Keep,
            Unheld::Keep,
        ),
        (EmitScope::Pages(pages), EmitBase::Overlay(overlay)) => {
            return emit_overlay(st, pages, overlay, (true, true), p);
        }
        (EmitScope::Frame(pages), EmitBase::Overlay(overlay)) => {
            return emit_overlay(st, pages, overlay, (false, true), p);
        }
        (EmitScope::Delta(page), EmitBase::Overlay(overlay)) => {
            return emit_overlay(st, &[*page], overlay, (true, false), p);
        }
        _ => bail!(
            "a whole file is a Rewrite, some keys go onto the saved file, and some pages over \
             their source"
        ),
    };
    let shown = shown(st);
    let pages = written_pages(st, &shown);
    let order: Vec<PageId> = st.order().iter().copied().collect();
    let srcs = (shown.values().map(|page| &page.src))
        .chain([primary])
        .map(|src| (src.id(), src.clone()))
        .collect();
    let mut closure = Closure::new(primary.id(), Pages::Written(pages));
    if let Some((record, _, _, relinked)) = saved {
        closure.saved(record.map.clone(), relinked);
    }
    let mut cx = Cx::new(st, primary, closure, transforms);
    cx.private = private;
    cx.unheld = unheld;
    cx.prior = prior;
    cx.append = keys
        .zip(saved)
        .map(|(keys, (record, parents, tree, _))| Append {
            keys,
            record,
            parents,
            tree,
            extra: BTreeSet::new(),
        });
    cx.srcs = srcs;
    cx.shown = shown;
    claim(&mut cx, &order);
    if cx.append.is_some() {
        appended(&mut cx, p)?;
    } else {
        p.set_total(Some(st.page_count() as u64), crate::PAGES);
        let shape = tree::balanced(&order);
        for (pos, (id, def, geom)) in st.pages().enumerate() {
            p.check()?;
            let parent = ObjKey::PagesNode(shape.leaf[pos]);
            pages::page(&mut cx, id, def, geom, pos, parent)?;
            p.inc(1);
        }
        tree::nodes(&mut cx, &shape);
        catalog::catalog(&mut cx);
    }
    finish(cx, p)
}

/// `pages` of `st`, each a page of `o`'s source, as overlay `o`'s section
/// writes them over the source, `p` counting them: each page as its source
/// page has it, under one root that lists them in order, and the catalog
/// over that root. Without `with_pages`, the root and the catalog alone:
/// the overlay's frame; without `with_frame`, the pages alone: a delta,
/// each page's `/Parent` the root still, which the frame writes.
fn emit_overlay(
    st: &DocState,
    pages: &[PageId],
    o: &Overlay,
    (with_pages, with_frame): (bool, bool),
    p: &dyn Progress,
) -> Result<Emission> {
    let mut shown = HashMap::with_capacity(pages.len());
    for &id in pages {
        let def = (st.def(id)).with_context(|| format!("{id:?} isn't a page of the document"))?;
        if def.base.src != o.src {
            bail!("{id:?} isn't a page of the overlay's source");
        }
        let page = Shown {
            src: def.base.src.clone(),
            index: def.base.index,
            copy: def.copy,
        };
        shown.insert(id, page);
    }
    let srcs = (shown.values().map(|page| &page.src))
        .chain([&o.src])
        .map(|src| (src.id(), src.clone()))
        .collect();
    let within = pages.iter().copied().collect();
    let closure = Closure::new(o.src.id(), Pages::Overlay { st, within });
    let mut cx = Cx::new(st, &o.src, closure, &[]);
    cx.overlay = Some(o);
    cx.srcs = srcs;
    cx.shown = shown;
    // The catalog is written over the source's, which the base's trailer
    // names.
    let catalog = o
        .src
        .pdf()
        .trailer()
        .get(b"Root")
        .and_then(Object::as_reference);
    if let Ok(at) = catalog {
        cx.own(ObjKey::Catalog, at);
    }
    claim(&mut cx, pages);
    p.set_total(Some(pages.len() as u64), crate::PAGES);
    let root = ObjKey::PagesNode(0);
    for &id in pages.iter().filter(|_| with_pages) {
        p.check()?;
        let (Some(def), Some(geom), Some(pos)) = (st.def(id), st.geom(id), st.pos_of(id)) else {
            bail!("{id:?} isn't a page of the document");
        };
        pages::page(&mut cx, id, def, geom, pos, root)?;
        p.inc(1);
    }
    if with_frame {
        tree::nodes(&mut cx, &tree::Shape::flat(pages));
        catalog::overlay(&mut cx);
    }
    finish(cx, p)
}

/// What `cx` emitted, once what its copies reach is copied too: for an
/// Append, only what something it writes of the state reaches.
fn finish(mut cx: Cx, p: &dyn Progress) -> Result<Emission> {
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
    if !cx.unread.is_empty() {
        let pages = crate::describe::pages_text(&cx.unread, cx.st);
        cx.notes.push(Note(format!(
            "No recognized text, watermark, background, header or footer is drawn on {pages}, \
             whose content can't be read."
        )));
    }
    if !cx.stayed.is_empty() {
        let on: Vec<PageId> = cx.stayed.iter().map(|&(page, _)| page).collect();
        let pages = crate::describe::pages_text(&on, cx.st);
        let (n, stay, them) = match cx.stayed.iter().map(|&(_, n)| n).sum::<usize>() {
            1 => (
                "1 watermark, background, header or footer".to_string(),
                "stays",
                "it",
            ),
            n => (
                format!("{n} watermarks, backgrounds, headers or footers"),
                "stay",
                "them",
            ),
        };
        cx.notes.push(Note(format!(
            "{n} other programs or files drew {stay} on {pages}: taking {them} out would change \
             what else the page draws."
        )));
    }
    if !cx.text_stayed.is_empty() {
        let pages = crate::describe::pages_text(&cx.text_stayed, cx.st);
        cx.notes.push(Note(format!(
            "Recognized text sheaf wrote earlier stays on {pages}: taking it out would change \
             what else the page draws."
        )));
    }
    if !cx.lists_kept.is_empty() {
        let on: Vec<PageId> = cx.lists_kept.iter().map(|&(page, _)| page).collect();
        let pages = crate::describe::pages_text(&on, cx.st);
        let (n, lists) = match cx.lists_kept.iter().map(|&(_, n)| n).sum::<usize>() {
            1 => (1, "property list"),
            n => (n, "property lists"),
        };
        cx.notes.push(Note(format!(
            "sheaf's private data stays in {n} {lists} on {pages}, which can't be written \
             again without it."
        )));
    }
    if cx.append.is_some() {
        reached(&mut cx.objects, &cx.syms);
    }
    let aliases = cx.form.aliases().collect();
    let form_keys = std::mem::take(&mut cx.form.keys);
    let form_touched = cx.form.touched;
    let allowed_drops = AllowedDrops::of(cx.st, cx.prior, cx.allowed, cx.redrawn);
    let written: HashSet<ObjectId> = cx.owned.values().copied().collect();
    let removed_annots = (cx.left_out.iter())
        .filter(|id| !written.contains(id))
        .map(|&id| ObjRef {
            src: cx.primary.clone(),
            id,
        })
        .collect();
    Ok(Emission {
        objects: cx.objects,
        syms: cx.syms,
        version: cx.version,
        removed_annots,
        allowed_drops,
        drawn: cx.drawn,
        notes: cx.notes,
        owned: cx.owned,
        aliases,
        form_keys,
        form_touched,
    })
}

/// An Append's objects, into `cx`: the pages whose dictionaries are in
/// scope, under their nodes in the spliced tree; the annotations in scope
/// of the other pages, their pages' `/Annots` the saved file's; the nodes
/// that changed; the form, the outline, the document scripts and the page
/// labels when any of theirs is in scope; and the catalog when it is, or
/// when the section needs a later version than the file is, or the form
/// or the outline came or went.
fn appended(cx: &mut Cx, p: &dyn Progress) -> Result<()> {
    let st = cx.st;
    let append = cx.append.as_ref().expect("an Append");
    let (keys, record, parents, spliced) =
        (append.keys, append.record, append.parents, append.tree);
    let annotated: HashSet<PageId> = (keys.iter())
        .filter_map(|key| match key {
            ObjKey::Annot(a) | ObjKey::AnnotPart(a, _) => Some(st.annot(*a)?.page),
            _ => None,
        })
        .collect();
    let visited = |id: PageId| keys.contains(&ObjKey::Page(id)) || annotated.contains(&id);
    let total = st.pages().filter(|&(id, _, _)| visited(id)).count();
    p.set_total(Some(total as u64), crate::PAGES);
    for (pos, (id, def, geom)) in st.pages().enumerate() {
        if keys.contains(&ObjKey::Page(id)) {
            p.check()?;
            let parent = (parents.get(&id).copied())
                .with_context(|| format!("{id:?} has no place in the saved file's page tree"))?;
            pages::page(cx, id, def, geom, pos, parent)?;
            p.inc(1);
        } else if annotated.contains(&id) {
            p.check()?;
            let shown = cx.shown[&id].clone();
            let tags = cx.keeps_tags(id);
            annots::page(cx, id, &shown, tags);
            p.inc(1);
        }
    }
    if let Some(spliced) = spliced {
        tree::spliced_nodes(cx, spliced);
    }
    let any = |of: fn(&ObjKey) -> bool| keys.iter().any(of);
    let mut form = (keys.contains(&ObjKey::AcroForm) || any(|k| matches!(k, ObjKey::Field(_))))
        .then(|| form::acroform(cx));
    let mut outline = (keys.contains(&ObjKey::OutlinesRoot)
        || any(|k| matches!(k, ObjKey::Bookmark(_))))
    .then(|| outline::outline(cx));
    let scripts = (keys.contains(&ObjKey::Names(NameTree::JavaScript))).then(|| form::scripts(cx));
    let came_or_went = |now: &Option<Option<Object>>, was: bool| {
        now.as_ref().is_some_and(|now| now.is_some() != was)
    };
    if cx.version > record.pdf_version
        || came_or_went(&form, record.roots.acroform.is_some())
        || came_or_went(&outline, record.roots.outlines.is_some())
    {
        cx.write_too(ObjKey::Catalog);
    }
    let labels = st.label_ranges().filter(|r| !r.is_empty());
    // What the saved catalog holds in itself rather than by number is
    // written anew with it: a form, an outline, page labels or the
    // generators the map has no number for.
    let unnumbered = |cx: &Cx, key: ObjKey| !cx.writes(&key) && record.map.get(&key).is_none();
    let watermarked = (st.generators().values()).any(|g| g.spec.watermark().is_some());
    if watermarked && cx.writes(&ObjKey::Catalog) && unnumbered(cx, ObjKey::Generators) {
        cx.write_too(ObjKey::Generators);
    }
    // Made only when it's written: an image's or a page's digest decodes it.
    let generators = (cx.writes(&ObjKey::Generators))
        .then(|| crate::generate::catalog_json(st, |c| content::drawn_as(cx, c)))
        .flatten();
    if cx.writes(&ObjKey::Catalog) {
        if form.is_none() && record.roots.acroform.is_some() && unnumbered(cx, ObjKey::AcroForm) {
            cx.write_too(ObjKey::AcroForm);
            form = Some(form::acroform(cx));
        }
        if outline.is_none()
            && !st.outline_roots().is_empty()
            && unnumbered(cx, ObjKey::OutlinesRoot)
        {
            cx.write_too(ObjKey::OutlinesRoot);
            outline = Some(outline::outline(cx));
        }
        if labels.is_some() && unnumbered(cx, ObjKey::Labels) {
            cx.write_too(ObjKey::Labels);
        }
    }
    if let Some(json) = generators {
        cx.push(ObjKey::Generators, catalog::generators(json));
    }
    if let Some(ranges) = labels.filter(|_| cx.writes(&ObjKey::Labels)) {
        cx.push(
            ObjKey::Labels,
            Object::Dictionary(crate::labels::number_tree(&ranges)),
        );
    }
    if cx.writes(&ObjKey::Catalog) {
        let root = match spliced {
            Some(t) => t.root(),
            None => tree::saved_root(record)?,
        };
        let parts = catalog::Parts {
            root,
            form,
            outline,
            scripts,
        };
        catalog::appended(cx, parts);
    }
    Ok(())
}

/// `objects`, an Append's, without what nothing it writes of the state
/// reaches: an object a key out of scope reached as it was built, a
/// resource or a copy, which only the saved file's object of it names.
fn reached(objects: &mut Vec<(ObjKey, SymObject)>, syms: &Syms) {
    let at: HashMap<ObjKey, usize> = (objects.iter().enumerate())
        .map(|(i, (key, _))| (*key, i))
        .collect();
    let mut seen = vec![false; objects.len()];
    let mut stack: Vec<usize> = (objects.iter().enumerate())
        .filter(|(_, (key, _))| key.of_state())
        .map(|(i, _)| i)
        .collect();
    while let Some(i) = stack.pop() {
        if std::mem::replace(&mut seen[i], true) {
            continue;
        }
        let mut next = Vec::new();
        let mut named = |r: ObjectId| {
            if let Some(&j) = syms.key(r).and_then(|key| at.get(&key)) {
                next.push(j);
            }
        };
        match &objects[i].1 {
            SymObject::Built(obj) => refs(obj, &mut named),
            SymObject::Deferred(d) => d.dict.iter().for_each(|(_, v)| refs(v, &mut named)),
            SymObject::Raw(_) => {}
        }
        stack.extend(next.into_iter().filter(|&j| !seen[j]));
    }
    let mut keep = seen.into_iter();
    objects.retain(|_| keep.next().expect("one each"));
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

/// The primary's page objects `pages`, in order, are written at (§11.5,
/// bug 3), shown as `shown` says: each page that's copy 0 of a primary
/// page, at the first of the page table's listings of it, the first such
/// page in order taking it.
pub(crate) fn owned_pages(
    pages: impl IntoIterator<Item = PageId>,
    shown: &HashMap<PageId, Shown>,
    primary: BlobId,
) -> Vec<(PageId, ObjectId)> {
    let mut taken = HashSet::new();
    let mut out = Vec::new();
    for id in pages {
        let page = &shown[&id];
        if page.src.id() == primary
            && page.copy == 0
            && let Some(facts) = page.src.page(page.index)
            && facts.repeat == 0
            && taken.insert(facts.id)
        {
            out.push((id, facts.id));
        }
    }
    out
}

/// The primary's numbers the keys that are its objects keep (§11.5),
/// claimed before anything is emitted, in a fixed order, so whichever
/// emitter refers to one first, it's numbered alike: copy 0 of each primary
/// page of `pages` (the emission's, in order), the form's nodes
/// ([`form::prepare`]), then each page's annotations read from the
/// primary, copy 0 of them (a widget merged with its field is the
/// field's). And every annotation read from a file, by its object and
/// copy, for replies.
fn claim(cx: &mut Cx, pages: &[PageId]) {
    let st = cx.st;
    let primary = cx.primary.id();
    for (id, at) in owned_pages(pages.iter().copied(), &cx.shown, primary) {
        cx.own(ObjKey::Page(id), at);
    }
    cx.form = form::prepare(cx);
    for &id in pages {
        for &aid in st.annot_ids_on(id).into_iter().flatten() {
            let Some(rec) = st.annot(aid) else {
                continue;
            };
            let read = match &rec.body {
                AnnotBody::Mark(m) => m.src.as_ref().map(|r| (r, 0)),
                AnnotBody::Foreign(f) => f.src.as_ref().map(|r| (r, f.copy)),
                AnnotBody::Widget(w) if cx.form.merged_into(aid).is_none() => {
                    w.src.as_ref().map(|r| (r, w.inst.copy))
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
