//! The records a document is made of. What a page is (M10.5 §3.3, §3.4):
//! the source page it shows, which copy of that page it is, and what sheaf
//! draws over it. Its boxes are a [`Geom`](crate::geom::Geom), a key of
//! their own, so a crop or a turn doesn't touch what OCR wrote. The
//! annotations (§3.6), one id space: sheaf's marks, other tools' comments
//! and links, and the form's widgets, each on its page, a mark holding its
//! stamp's page and its attachment's file itself. The form (§3.7): every
//! node of its field tree, each in an instance of a source's form, a
//! terminal with its value and its widgets by id. The bookmarks (§3.8),
//! each its own record, its kids by id. And the generators (§3.5): what
//! sheaf draws on pages from one definition.
//!
//! A stack holds what something makes today: layers over the page, each a
//! block of its own named by its id, of recognized text or of the
//! annotations a flatten drew there, forms placed by matrix (an appearance
//! a source holds, or a mark's drawn again from what it was drawn from);
//! filters that take others' watermarks and backgrounds out of its own
//! content (Remove Watermark's): blocks of sheaf's no generator of the
//! document's backs, and the pagination artifacts Acrobat, an older sheaf
//! or, asked to, the author drew; filters that take the recognized text
//! sheaf wrote in its file out (a re-OCR's and Clear's); and one that takes
//! the settings an older sheaf kept out of its watermarks (Clean Copy's).
//! Other filters and layers under it come with the features that make them.
//! A generator is a header and footer or a watermark or background, with
//! its z, its scope, what it fixed when it was set (the date, the file and
//! Bates numbers) and the kinds of others' artifacts it replaces. A form
//! instance is one per insert of a source's pages, or duplicate of them,
//! and the form's own keys (`/DR`, `/CO`, `/DA`…) are written from
//! [`FormDoc`].

use std::collections::BTreeSet;
use std::fmt;
use std::sync::{Arc, LazyLock, OnceLock};

use anyhow::Result;
use im::Vector;
use sheaf_edit::{Color, DestView, FontChoice, GridPos, HeaderFooterSpec, Z};

use sheaf_edit::Rotation;

use crate::annot::{Annot, AnnotId, AnnotKind};
use crate::content::{BlockId, PagOrigin, PaginationKind, ResId};
use crate::file::ForeignAnnot;
use crate::forms::field::{Action, Border, Choice, Mk, Trigger};
use crate::forms::{FieldId, FieldKind, Value};
use crate::geom::{Affine, Geom};
use crate::model::{OutlineId, PageId, SourceId, TextStyle};
use crate::source::ObjRef;
use crate::store::{BlobRef, Src};
use crate::text_layer::TextLayer;

// A generator's id is the one its blocks name it by (M10.5 §4).
pub use crate::content::GenId;

/// A record's digest of what it draws (M10.5 §10.2), worked out the first
/// time it's asked for and kept with it. No part of what the record is: a
/// clone, made to be changed, starts without one, and records are equal,
/// or print, whatever theirs.
#[derive(Default)]
pub(crate) struct DigestCell(OnceLock<u128>);

impl DigestCell {
    /// The digest, worked out by `digest` the first time.
    pub(crate) fn get_or_init(&self, digest: impl FnOnce() -> u128) -> u128 {
        *self.0.get_or_init(digest)
    }
}

impl Clone for DigestCell {
    fn clone(&self) -> DigestCell {
        DigestCell::default()
    }
}

impl PartialEq for DigestCell {
    fn eq(&self, _: &DigestCell) -> bool {
        true
    }
}

/// A page of the document. Made by [`PageDef::new`], and changed by making
/// another, since it keeps its digest.
#[derive(Clone, PartialEq)]
pub struct PageDef {
    pub base: PageBase,
    /// Which copy of its base page it is, fixed when the page is made: 0
    /// is the base page's own object, n ≥ 1 the nth copy (a duplicate, the
    /// same page inserted again, a page its file's tree lists twice).
    pub copy: u32,
    /// [`ContentStack::empty`] when nothing is drawn over it.
    pub content: Arc<ContentStack>,
    digest: DigestCell,
}

impl PageDef {
    pub fn new(base: PageBase, copy: u32, content: Arc<ContentStack>) -> PageDef {
        PageDef {
            base,
            copy,
            content,
            digest: DigestCell::default(),
        }
    }

    /// A digest of what it draws: its base page and what's drawn over it,
    /// not which copy it is, which draws the same (M10.5 §10.2). Worked
    /// out once.
    pub fn digest(&self) -> u128 {
        let PageDef {
            base,
            copy: _,
            content,
            digest,
        } = self;
        digest.get_or_init(|| crate::realize::page_digest(base, content))
    }
}

impl fmt::Debug for PageDef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let PageDef {
            base,
            copy,
            content,
            digest: _,
        } = self;
        f.debug_struct("PageDef")
            .field("base", base)
            .field("copy", copy)
            .field("content", content)
            .finish_non_exhaustive()
    }
}

/// The source page a page shows.
#[derive(Debug, Clone, PartialEq)]
pub struct PageBase {
    pub src: Src,
    /// 0-based, in `src`.
    pub index: u32,
}

/// What's taken out of a page's own content, and what's drawn over it,
/// bottom first.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ContentStack {
    /// Applied to the page's own content as a save writes it, in order.
    pub filters: Vector<Filter>,
    pub over: Vector<Layer>,
}

/// What a save takes out of a page's own content (M10.5 §3.4): sheaf's
/// blocks that no generator of the document's backs, which another file
/// drew, or which another tool merged into a stream of its own; others'
/// pagination artifacts, by kind and by who drew them; the recognized text
/// sheaf wrote in the file; and sheaf's private data in it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Filter {
    /// Marked ranges, spliced out of the streams they're in (§13): each
    /// stream that loses one is a copy of its own, so a page that shares
    /// it keeps it whole.
    DropMarked(MarkedSel),
    /// Whole streams of its `/Contents`.
    DropStreams(StreamSel),
    /// sheaf's private data in the page's own content taken out, what it
    /// drew kept (§15): what an older sheaf kept in what it drew, the names
    /// it gave what it drew with, and what sheaf marks its blocks with, as
    /// [`LegacySel`] selects it. Each stream it changes is a copy of its
    /// own, as [`Filter::DropMarked`]'s are. A list that can't be written
    /// again without it keeps it, said in a note. `RemovePrivateData`'s, a
    /// Strip output's ([`Private::Strip`]), whose check fails on what's
    /// left (§11.9), or says it in a note under `Private::Scrub`.
    ///
    /// [`Private::Strip`]: crate::materialize::Private::Strip
    StripLegacy(LegacySel),
}

impl Filter {
    /// Whether it takes out the recognized text sheaf wrote in the file
    /// ([`StreamSel::SheafOcr`], [`MarkedSel::SheafOcr`]): replaced text,
    /// where the others take out what's drawn (§7.1).
    pub fn drops_ocr(&self) -> bool {
        matches!(
            self,
            Filter::DropStreams(StreamSel::SheafOcr) | Filter::DropMarked(MarkedSel::SheafOcr)
        )
    }
}

/// What a [`Filter::StripLegacy`] takes sheaf's private data out of.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum LegacySel {
    /// The property lists of the ranges selected written in the content:
    /// written again without [`private::LIST_KEYS`] (§13), an older
    /// sheaf's settings, an image's path among them, and its mark among
    /// them. A list named in the resources is [`LegacySel::Resources`]'.
    ///
    /// [`private::LIST_KEYS`]: crate::private::LIST_KEYS
    Marked(MarkedSel),
    /// Every stream of the page's content sheaf marked: its dictionary
    /// without [`private::STREAM_KEYS`] (a block's `/SheafBlock`, an older
    /// sheaf's `/SheafOcrLayer`), its data as it was.
    ///
    /// [`private::STREAM_KEYS`]: crate::private::STREAM_KEYS
    Streams,
    /// The page's resources: each property list named in its
    /// `/Properties` without [`private::LIST_KEYS`], and each name an
    /// older sheaf gave a resource ([`private::is_resource_name`]: its
    /// header's font, its watermark's, its recognized text's, what its
    /// flatten drew) given a new one, `/Sh…`, where the resources bind it
    /// and wherever the page's content draws with it, lexically, every
    /// other byte as it was.
    ///
    /// [`private::LIST_KEYS`]: crate::private::LIST_KEYS
    /// [`private::is_resource_name`]: crate::private::is_resource_name
    Resources,
}

/// The marked ranges a [`Filter::DropMarked`] takes out.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MarkedSel {
    /// A generator's blocks merged into another stream: the ranges whose
    /// property list has `/SheafGen` naming it.
    SheafGen(GenId),
    /// The pagination artifacts of `kinds` that `origins` drew
    /// ([`PageBlocks::selected`]), each cut as [`Pagination::cuts`] says;
    /// one a splice refuses stays. A block of sheaf's in a stream of its
    /// own isn't cut: one the document's generators back is set aside
    /// anyway, and another file's is left out whole.
    ///
    /// [`PageBlocks::selected`]: crate::content::PageBlocks::selected
    /// [`Pagination::cuts`]: crate::content::Pagination::cuts
    Pagination {
        kinds: BTreeSet<PaginationKind>,
        origins: BTreeSet<PagOrigin>,
    },
    /// sheaf's OCR blocks merged into another stream: the ranges whose
    /// property list has `/SheafOcrBlock`, whatever block it names. An op
    /// puts it only once a splice is shown to take them out, and fails
    /// where one refuses ([`EditError::Content`]).
    ///
    /// [`EditError::Content`]: crate::tx::EditError::Content
    SheafOcr,
}

/// The streams a [`Filter::DropStreams`] leaves out.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum StreamSel {
    /// A generator's blocks: the streams whose `/SheafBlock` names it.
    SheafGen(GenId),
    /// sheaf's recognized text in the file: its OCR blocks, each a stream
    /// marked `/SheafBlock << /K /Ocr >>`, whatever block it names; and
    /// the layer an older sheaf wrote, the `/SheafOcrLayer` stream after
    /// the page's content with the `q` stream before it that its leading
    /// `Q` ends, both or neither, as [`PageBlocks::legacy_ocr`] pairs
    /// them, so what's left balances as it did.
    ///
    /// [`PageBlocks::legacy_ocr`]: crate::content::PageBlocks::legacy_ocr
    SheafOcr,
}

/// One thing drawn over a page: a block of its own in the file (§11.4).
#[derive(Debug, Clone, PartialEq)]
pub struct Layer {
    /// Its block's (`/SheafBlock << /B (b…) >>`): drawn when it's made,
    /// kept by one that replaces it on the stack, saved or not, and shared
    /// by a duplicate.
    pub id: BlockId,
    pub body: Static,
}

/// What a layer is, as its block says (`/K`): its body's kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BlockKind {
    /// `/Ocr`: recognized text.
    Ocr,
    /// `/Flat`: annotations a flatten drew.
    Flattened,
}

impl Layer {
    /// Its kind, which its body is.
    pub fn kind(&self) -> BlockKind {
        match self.body {
            Static::OcrText(_) => BlockKind::Ocr,
            Static::Placements(_) => BlockKind::Flattened,
        }
    }
}

/// A layer's content, fixed when it's made.
#[derive(Debug, Clone, PartialEq)]
pub enum Static {
    /// Recognized text, invisible, its words in PageSpace.
    OcrText(Arc<TextLayer>),
    /// Forms placed on the page, in order: the annotations a flatten drew
    /// there.
    Placements(Arc<[Placement]>),
}

impl Static {
    /// The recognized text it is, if it is some.
    pub fn ocr_text(&self) -> Option<&Arc<TextLayer>> {
        match self {
            Static::OcrText(text) => Some(text),
            Static::Placements(_) => None,
        }
    }

    /// Where its content is: the same layer wherever a page shows it.
    pub(crate) fn addr(&self) -> usize {
        match self {
            Static::OcrText(text) => Arc::as_ptr(text).addr(),
            Static::Placements(placed) => Arc::as_ptr(placed).addr(),
        }
    }
}

/// A form drawn on a page: `form`, through `matrix`, which takes the box
/// its own matrix makes of its bounding box onto where it goes (an
/// annotation's `/Rect`, ISO 32000-1 12.5.5), tagged as `tag` says.
#[derive(Debug, Clone, PartialEq)]
pub struct Placement {
    pub form: FormRef,
    pub matrix: Affine,
    pub tag: PlaceTag,
}

/// How a placement is marked in a page's content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PlaceTag {
    /// `/Artifact`: no part of the page's structure, on a page of a
    /// tagged file.
    Artifact,
    /// Not marked: on a page of a file with no structure.
    Untagged,
}

/// A mark's appearance pinned by what it's drawn from (§3.4), so a layer
/// draws it again, the same each time, without the mark or a cache. Made
/// by [`PinnedAp::new`], which works out its digest.
#[derive(Debug, Clone)]
pub struct PinnedAp {
    pub inputs: MarkAp,
    /// Of the inputs: what it's known by.
    pub(crate) digest: u128,
}

impl PinnedAp {
    pub fn new(inputs: MarkAp) -> PinnedAp {
        PinnedAp {
            digest: crate::realize::pinned_digest(&inputs),
            inputs,
        }
    }
}

/// Pinned appearances are the same when their inputs' digests are.
impl PartialEq for PinnedAp {
    fn eq(&self, other: &PinnedAp) -> bool {
        self.digest == other.digest
    }
}

impl Eq for PinnedAp {}

impl std::hash::Hash for PinnedAp {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.digest.hash(state);
    }
}

/// What a sheaf mark's appearance is drawn from, held by value, naming no
/// annotation: its kind (a connector's without its partner), its look and
/// text, the turn it reads upright in and the stamp's page it draws. Its
/// author, dates and names aren't drawn. A widget's (its terminal's value
/// and display, its state and the form's `/DA`, `/Q` and `/DR`) come with
/// M10 slice 15's FlattenFields.
#[derive(Debug, Clone, PartialEq)]
pub struct MarkAp {
    pub kind: AnnotKind,
    pub color: [u8; 3],
    pub opacity: f32,
    pub stroke: f32,
    pub contents: String,
    pub fill: Option<[u8; 3]>,
    pub dashed: bool,
    pub cloudy: bool,
    pub turn: Rotation,
    /// A page stamp's page.
    pub stamp: Option<FormRef>,
}

impl MarkAp {
    /// The appearances mark `m` writes, by what each is drawn from: its
    /// own, then a replacement's struck text, its companion's. None for a
    /// reply or a status, which draw nothing of their own.
    pub fn of(m: &Mark) -> Vec<MarkAp> {
        let a = &m.annot;
        if matches!(a.kind, AnnotKind::Reply { .. } | AnnotKind::Status { .. }) {
            return Vec::new();
        }
        let stamp = (m.stamp.clone()).filter(|_| matches!(a.kind, AnnotKind::PageStamp { .. }));
        let companion = crate::annot::companion(a).map(|c| MarkAp::drawn(&c, None));
        std::iter::once(MarkAp::drawn(a, stamp))
            .chain(companion)
            .collect()
    }

    /// What `a` is drawn from, drawing `stamp`.
    fn drawn(a: &Annot, stamp: Option<FormRef>) -> MarkAp {
        let mut kind = a.kind.clone();
        if let AnnotKind::Connector { partner, .. } = &mut kind {
            *partner = None;
        }
        MarkAp {
            kind,
            color: a.color,
            opacity: a.opacity,
            stroke: a.stroke,
            contents: a.contents.clone(),
            fill: a.fill,
            dashed: a.dashed,
            cloudy: a.cloudy,
            turn: a.turn,
            stamp,
        }
    }

    /// The mark it's drawn as, with no id, author, dates or name.
    pub fn annot(&self) -> Annot {
        Annot {
            id: AnnotId(0),
            kind: self.kind.clone(),
            color: self.color,
            opacity: self.opacity,
            stroke: self.stroke,
            author: String::new(),
            contents: self.contents.clone(),
            modified: String::new(),
            created: String::new(),
            fill: self.fill,
            dashed: self.dashed,
            cloudy: self.cloudy,
            turn: self.turn,
            name: String::new(),
        }
    }

    /// Whether it names an annotation, which a content stack never does
    /// (`DocState::check_invariants`): a connector's partner, a thread's
    /// parent.
    pub(crate) fn names_annotation(&self) -> bool {
        matches!(
            self.kind,
            AnnotKind::Connector {
                partner: Some(_),
                ..
            } | AnnotKind::Reply { .. }
                | AnnotKind::Status { .. }
        )
    }
}

/// The stack of every page with nothing drawn over it.
static EMPTY: LazyLock<Arc<ContentStack>> = LazyLock::new(Arc::default);

impl ContentStack {
    /// The one empty stack, which untouched pages share.
    pub fn empty() -> Arc<ContentStack> {
        EMPTY.clone()
    }

    /// Whether `stack` is the shared empty one.
    pub fn is_shared_empty(stack: &Arc<ContentStack>) -> bool {
        Arc::ptr_eq(stack, &EMPTY)
    }

    /// Its recognized text, if it has some.
    pub fn ocr_text(&self) -> Option<&Arc<TextLayer>> {
        self.over.iter().find_map(|l| l.body.ocr_text())
    }

    /// The layer of its recognized text, if it has some.
    pub fn ocr_layer(&self) -> Option<&Layer> {
        self.over.iter().find(|l| l.kind() == BlockKind::Ocr)
    }

    /// The stack with `layer` as its recognized text, in place of what it
    /// had, or with none: the empty one when that leaves nothing.
    pub fn with_ocr(&self, layer: Option<Layer>) -> Arc<ContentStack> {
        let mut over = self.over.clone();
        over.retain(|l| l.kind() != BlockKind::Ocr);
        over.extend(layer);
        ContentStack {
            filters: self.filters.clone(),
            over,
        }
        .shared()
    }

    /// The stack with `layer` drawn over the rest.
    pub fn with_layer(&self, layer: Layer) -> Arc<ContentStack> {
        let mut over = self.over.clone();
        over.push_back(layer);
        ContentStack {
            filters: self.filters.clone(),
            over,
        }
        .shared()
    }

    /// The stack with `filter` after its filters, unless it has it.
    pub fn with_filter(&self, filter: Filter) -> Arc<ContentStack> {
        self.with_filters([filter])
    }

    /// The stack with each of `filters` after its filters, in order, but
    /// those it has.
    pub fn with_filters(&self, filters: impl IntoIterator<Item = Filter>) -> Arc<ContentStack> {
        let mut with = self.filters.clone();
        for filter in filters {
            if !with.contains(&filter) {
                with.push_back(filter);
            }
        }
        ContentStack {
            filters: with,
            over: self.over.clone(),
        }
        .shared()
    }

    /// It, or the empty one when it's empty.
    fn shared(self) -> Arc<ContentStack> {
        match self.filters.is_empty() && self.over.is_empty() {
            true => ContentStack::empty(),
            false => Arc::new(self),
        }
    }
}

/// An annotation, on its page.
#[derive(Debug, Clone, PartialEq)]
pub struct AnnotRec {
    pub page: PageId,
    pub body: AnnotBody,
}

/// Whose an annotation is.
#[derive(Debug, Clone, PartialEq)]
pub enum AnnotBody {
    Mark(Arc<Mark>),
    Foreign(Arc<Foreign>),
    /// Where a field shows: it goes with its page, and is never deleted
    /// on its own.
    Widget(Arc<Widget>),
}

/// One of sheaf's marks.
#[derive(Debug, Clone, PartialEq)]
pub struct Mark {
    /// What it is and says; its id is the record's.
    pub annot: Annot,
    /// The object it was read as, in the file it came from: what it is
    /// across saves. None for one made since, or a copy.
    pub src: Option<ObjRef>,
    /// An attachment's file, which its kind names by its stem.
    pub file: Option<BlobRef>,
    /// A page stamp's page, which its kind names by its stem.
    pub stamp: Option<FormRef>,
}

impl From<Annot> for Mark {
    /// `annot`, read from no file and holding no stamp's page or file.
    fn from(annot: Annot) -> Mark {
        Mark {
            annot,
            src: None,
            file: None,
            stamp: None,
        }
    }
}

/// A form: the page a page stamp draws, or one a layer places.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum FormRef {
    /// A form a source holds: the stamp's page a file sheaf saved holds,
    /// or an annotation's appearance.
    Base(ObjRef),
    /// A one-page PDF, its page the form: a library stamp placed, or a
    /// stamp pasted from another document.
    Blob(BlobRef),
    /// A mark's appearance, drawn from what it was drawn from as it was
    /// flattened.
    Pinned(Arc<PinnedAp>),
}

impl FormRef {
    /// Its page as a one-page PDF, to show or copy: the blob's own, or one
    /// made of the form.
    pub fn to_pdf(&self) -> Result<Arc<[u8]>> {
        match self {
            FormRef::Blob(pdf) => Ok(pdf.bytes().clone()),
            FormRef::Base(at) => Ok(crate::stamp_lib::form_to_pdf(at.src.pdf(), at.id)?.into()),
            FormRef::Pinned(ap) => Ok(crate::annot::pinned_pdf(ap)?.into()),
        }
    }

    /// A digest of which form it is: a source's object, a blob, or a
    /// pinned appearance's inputs.
    pub fn digest(&self) -> u128 {
        crate::realize::form_ref_digest(self)
    }
}

/// Another tool's comment or link, as a file holds it: sheaf lists it,
/// threads replies to it, deletes it and leaves it out of a save, and
/// otherwise keeps it as it is.
#[derive(Debug, Clone, PartialEq)]
pub struct Foreign {
    /// The object it is; None for a dictionary written into its page's
    /// `/Annots`, at its `slot` there.
    pub src: Option<ObjRef>,
    /// Which copy of that object it is: its page's copy, as a page listed
    /// again or duplicated makes one (0 is the object itself).
    pub copy: u32,
    /// What it says, as read.
    pub summary: Arc<ForeignAnnot>,
}

impl AnnotRec {
    /// The mark it is, if it's one of sheaf's.
    pub fn mark(&self) -> Option<&Annot> {
        self.mark_record().map(|m| &m.annot)
    }

    /// The mark it is, with what it holds, if it's one of sheaf's.
    pub fn mark_record(&self) -> Option<&Arc<Mark>> {
        match &self.body {
            AnnotBody::Mark(m) => Some(m),
            AnnotBody::Foreign(_) | AnnotBody::Widget(_) => None,
        }
    }

    /// Another tool's annotation it is, if it is one.
    pub fn foreign(&self) -> Option<&Arc<Foreign>> {
        match &self.body {
            AnnotBody::Foreign(f) => Some(f),
            AnnotBody::Mark(_) | AnnotBody::Widget(_) => None,
        }
    }

    /// The widget it is, if it's one of the form's.
    pub fn widget(&self) -> Option<&Arc<Widget>> {
        match &self.body {
            AnnotBody::Widget(w) => Some(w),
            AnnotBody::Mark(_) | AnnotBody::Foreign(_) => None,
        }
    }

    /// What it's called in threads and in the file (`/NM`): nothing for a
    /// widget, which sheaf doesn't name.
    pub fn name(&self) -> String {
        match &self.body {
            AnnotBody::Mark(m) => m.annot.name(),
            AnnotBody::Foreign(f) => f.summary.name.clone(),
            AnnotBody::Widget(_) => String::new(),
        }
    }

    /// The comment whose thread it's in, by name.
    pub fn parent(&self) -> Option<&str> {
        match &self.body {
            AnnotBody::Mark(m) => m.annot.thread_parent(),
            AnnotBody::Foreign(f) => f.summary.parent.as_deref(),
            AnnotBody::Widget(_) => None,
        }
    }

    /// Whether `other` is the same record: the same page and the same body,
    /// by pointer first.
    pub(crate) fn same(&self, other: &AnnotRec) -> bool {
        self.page == other.page
            && match (&self.body, &other.body) {
                (AnnotBody::Mark(a), AnnotBody::Mark(b)) => Arc::ptr_eq(a, b) || a == b,
                (AnnotBody::Foreign(a), AnnotBody::Foreign(b)) => Arc::ptr_eq(a, b) || a == b,
                (AnnotBody::Widget(a), AnnotBody::Widget(b)) => Arc::ptr_eq(a, b) || a == b,
                _ => false,
            }
    }
}

/// One instance of a source's form in the document: the source, and
/// which copy of its form it is, fixed when the instance is made (M10.5
/// §3.7). Copy 0 of the primary's is its own objects. Every insert of a
/// file's pages makes another, and so does a duplicate of its pages, each
/// written with objects of its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FormInst {
    pub src: SourceId,
    pub copy: u32,
}

/// A node of the form's field tree: a terminal field, which holds a value
/// and shows in widgets, or a parent of others.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldNode {
    pub parent: Option<FieldId>,
    /// `/T` as it's written; empty for none. Full names are made from the
    /// partials from the root down.
    pub partial: Arc<str>,
    /// The field it was read as, in the file it came from; None for a
    /// parent sheaf made, which a form whose names clashed went under.
    pub src: Option<ObjRef>,
    pub inst: FormInst,
    /// The fields under it, in order; not its widgets.
    pub kids: Vector<FieldId>,
    /// What it holds when it's a terminal field.
    pub term: Option<Arc<Terminal>>,
}

/// A terminal field: the node that holds a value, with what it inherits
/// resolved.
#[derive(Debug, Clone, PartialEq)]
pub struct Terminal {
    pub kind: FieldKind,
    /// `/Ff` (see [`crate::forms::field::flags`]).
    pub flags: u32,
    pub value: Option<Value>,
    /// `/DV`: what Reset Form puts back.
    pub default: Option<Value>,
    /// What doesn't change when it's filled, shared by its copies.
    pub props: Arc<FieldProps>,
    /// Its widgets on pages, by id: page order as it came in.
    pub widgets: Vector<AnnotId>,
    /// Its widgets on page templates, which aren't pages: not modeled,
    /// kept by a save as they are.
    pub outside: u16,
}

/// What a terminal field is beside its value.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldProps {
    /// `/DA`, its own or a parent field's.
    pub da: Option<Arc<str>>,
    /// `/Q`: 0 left, 1 centered, 2 right; its own or a parent field's.
    pub align: Option<u8>,
    /// `/MaxLen`, as the file has it.
    pub max_len: Option<u32>,
    /// `/Opt`: a choice field's items, or a check box's or radio group's
    /// export values.
    pub options: Arc<[Choice]>,
    /// `/TU`: what the field is for, in words.
    pub tooltip: Option<String>,
    /// Its own `/AA`: keystroke, format, validate and calculate.
    pub actions: Vec<(Trigger, Action)>,
}

impl Terminal {
    /// `field` as read from its file, showing in `widgets`.
    pub(crate) fn read(field: &crate::forms::Field, widgets: Vector<AnnotId>) -> Terminal {
        Terminal {
            kind: field.kind,
            flags: field.flags,
            value: field.value.clone(),
            default: field.default.clone(),
            props: Arc::new(FieldProps {
                da: field.da.clone(),
                align: field.align,
                max_len: field.max_len,
                options: field.options.clone(),
                tooltip: field.tooltip.clone(),
                actions: field.actions.clone(),
            }),
            widgets,
            outside: field.outside,
        }
    }

    /// Whether any of its own actions is a script.
    pub fn has_scripts(&self) -> bool {
        self.props.actions.iter().any(|(_, a)| a.has_script())
    }
}

/// The form as a whole. Its keys of the form's dictionary a save writes
/// from the state, never from a file (M10.5 §9.1's ownership table):
/// `/Fields` (the roots), `/CO`, `/DA`, `/Q`, `/NeedAppearances` and
/// `/SigFlags`, read from the opened file's. `/DR` isn't held: a save
/// merges the instances' files' as it writes them (deciding its renames at
/// lowering, §6.5, is left for later; see M10.5 slice 11's notes). Changed
/// by making another, since it keeps its digest.
#[derive(Clone, Default, PartialEq)]
pub struct FormDoc {
    /// The top of the field tree, in order.
    pub roots: Vector<FieldId>,
    /// The file each instance came from, by name: the open's for the
    /// primary's, the inserted file's, or a duplicate's original's. What
    /// renamed fields go under, and notices.
    pub names: im::OrdMap<FormInst, Arc<str>>,
    pub scripts: ScriptState,
    /// `/CO`: the fields that calculate, in order; an inserted form's and
    /// a copy's after the rest. A node no longer in the tree is passed
    /// over.
    pub calc_order: Vector<FieldId>,
    /// `/DA` and `/Q`, for fields that set neither: the opened file's. An
    /// instance from another file whose form says otherwise has its own
    /// pushed down onto its roots as it's written.
    pub da: Option<Arc<str>>,
    pub align: u8,
    /// `/NeedAppearances`: the opened file's, or any form's that came in.
    pub need_appearances: bool,
    /// `/SigFlags`, the opened file's.
    pub sig_flags: i64,
    pub xfa: XfaState,
    /// Its digest once worked out ([`FormDoc::digest`]): one made from
    /// another's clone, or the default, starts without.
    pub(crate) digest: DigestCell,
}

impl fmt::Debug for FormDoc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let FormDoc {
            roots,
            names,
            scripts,
            calc_order,
            da,
            align,
            need_appearances,
            sig_flags,
            xfa,
            digest: _,
        } = self;
        f.debug_struct("FormDoc")
            .field("roots", roots)
            .field("names", names)
            .field("scripts", scripts)
            .field("calc_order", calc_order)
            .field("da", da)
            .field("align", align)
            .field("need_appearances", need_appearances)
            .field("sig_flags", sig_flags)
            .field("xfa", xfa)
            .finish_non_exhaustive()
    }
}

/// The XFA part the opened file's form has beside its fields (M10
/// proposal 6), as read at open: what save policy decides about
/// (`forms::rules::plan`). The part itself stays in the file's form,
/// which a save drops it from or keeps it in.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct XfaState {
    /// It has one.
    pub present: bool,
    /// The XFA is the form (`/NeedsRendering`): the pages are a
    /// placeholder.
    pub dynamic: bool,
    /// Fields whose values its datasets hold and the fields don't.
    pub values: usize,
}

impl FormDoc {
    /// What the form's own dictionary in `facts`, an opened file's, says
    /// of the keys a save owns, and of its XFA part; `roots` and `names`
    /// are the tree's.
    pub(crate) fn opened(
        roots: Vector<FieldId>,
        names: im::OrdMap<FormInst, Arc<str>>,
        facts: &crate::forms::FormFacts,
    ) -> FormDoc {
        FormDoc {
            roots,
            names,
            scripts: ScriptState::On,
            calc_order: facts.calculation_order.iter().copied().collect(),
            da: facts.da.clone(),
            align: facts.align,
            need_appearances: facts.need_appearances,
            sig_flags: facts.sig_flags,
            xfa: XfaState {
                present: facts.xfa,
                dynamic: facts.dynamic,
                values: facts.xfa_values,
            },
            digest: DigestCell::default(),
        }
    }

    /// A digest of all of it: what a page whose widgets are drawn draws
    /// them under (M10.5 §10.2). Worked out once.
    pub fn digest(&self) -> u128 {
        self.digest
            .get_or_init(|| crate::realize::form_digest(self))
    }
}

/// Whether the form's scripts run by default (M10 proposal 5).
#[derive(Debug, Clone, Default, PartialEq)]
pub enum ScriptState {
    #[default]
    On,
    /// Off: scripts came in from more than one file, and they share one
    /// scope. The instances with scripts when they went off.
    Off { instances: Arc<[FormInst]> },
}

/// A widget: where a terminal field shows on a page.
#[derive(Debug, Clone, PartialEq)]
pub struct Widget {
    pub field: FieldId,
    /// The annotation it was read as, in the file it came from.
    pub src: Option<ObjRef>,
    /// Its field's instance.
    pub inst: FormInst,
    /// Its dictionary is its field's: one object.
    pub merged: bool,
    /// `/Rect` in the page's own space (before `/Rotate`): left, bottom,
    /// right, top.
    pub rect: [f32; 4],
    /// `/F`.
    pub flags: u32,
    pub mk: Arc<Mk>,
    /// Its own `/DA`, when it isn't its field's dictionary too.
    pub da: Option<Arc<str>>,
    /// `/AS`: the appearance state it shows.
    pub state: Option<String>,
    /// The states `/AP /N` has, when it has several.
    pub states: Arc<[String]>,
    pub props: Arc<WidgetProps>,
}

/// What a widget is beside where it is and what it shows.
#[derive(Debug, Clone, PartialEq)]
pub struct WidgetProps {
    /// `/BS` (or the older `/Border`).
    pub border: Option<Border>,
    /// `/A` and its own `/AA`.
    pub actions: Vec<(Trigger, Action)>,
    /// It has a normal appearance: a stream, or states.
    pub has_appearance: bool,
}

impl Widget {
    /// `w` as read from its file, field `field`'s in instance `inst`, the
    /// annotation `src`.
    pub(crate) fn read(
        w: &crate::forms::Widget,
        field: FieldId,
        src: Option<ObjRef>,
        inst: FormInst,
    ) -> Widget {
        Widget {
            field,
            src,
            inst,
            merged: w.merged,
            rect: w.rect,
            flags: w.flags,
            mk: Arc::new(w.mk.clone()),
            da: w.da.clone(),
            state: w.state.clone(),
            states: w.states.clone().into(),
            props: Arc::new(WidgetProps {
                border: w.border,
                actions: w.actions.clone(),
                has_appearance: w.has_appearance,
            }),
        }
    }

    /// Whether any of its actions is a script.
    pub fn has_scripts(&self) -> bool {
        self.props.actions.iter().any(|(_, a)| a.has_script())
    }
}

/// A bookmark.
#[derive(Debug, Clone, PartialEq)]
pub struct Bookmark {
    pub title: Arc<str>,
    /// None: a heading that goes nowhere.
    pub target: Option<Target>,
    /// `/C`; None is black.
    pub color: Option<[u8; 3]>,
    pub style: TextStyle,
    /// Whether its kids show, as the file has it: a save writes a closed
    /// one's `/Count` negative.
    pub open: bool,
    /// The bookmarks under it, in order.
    pub kids: Vector<OutlineId>,
    /// The item it was read as, whose `/A`, `/SE` and the keys sheaf
    /// doesn't read survive it. None for one sheaf made.
    pub src: Option<ObjRef>,
}

/// Where a bookmark goes.
#[derive(Debug, Clone, PartialEq)]
pub enum Target {
    /// A page of the document; `view`, where on it it lands, in PageSpace,
    /// and None for the top of the page.
    Page {
        page: PageId,
        view: Option<DestView>,
    },
    /// Where its item went as read, kept from its `src`: an action other
    /// than a GoTo to a page here (a link, a script), or a destination
    /// that isn't a page here.
    Kept,
}

impl Bookmark {
    /// The page it goes to, if it goes to one.
    pub fn page(&self) -> Option<PageId> {
        match self.target {
            Some(Target::Page { page, .. }) => Some(page),
            _ => None,
        }
    }

    /// Where on its page it lands, if it goes to a place on a page.
    pub fn view(&self) -> Option<&DestView> {
        match &self.target {
            Some(Target::Page { view, .. }) => view.as_ref(),
            _ => None,
        }
    }
}

/// Content sheaf draws on pages from one definition, laid out on each
/// page as a save writes it (M10.5 §3.5): what it draws, over the page's
/// content or under it, on which pages, what it fixed when it was set, and
/// the kinds of pagination artifact others drew that it takes off its pages
/// (its own kinds, §11.4).
#[derive(Debug, Clone, PartialEq)]
pub struct Generator {
    pub id: GenId,
    pub spec: GenSpec,
    pub z: Z,
    pub scope: Scope,
    pub frozen: Frozen,
    pub replaces: BTreeSet<PaginationKind>,
}

/// What a generator fixed when it was set (M10.5 §3.5, proposal 8): what
/// `<<date>>` and `<<file>>` say, and each page's Bates number, by page,
/// counted then in the order of the pages it covered from the spec's
/// start, so a reorder never renumbers them and a page added later has
/// none. Setting it again is how to renumber. Empty for a watermark.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Frozen {
    pub date: Arc<str>,
    pub file: Arc<str>,
    pub bates: Option<Arc<im::OrdMap<PageId, u64>>>,
}

impl Frozen {
    /// Page `page`'s Bates number, if it has one.
    pub fn bates_number(&self, page: PageId) -> Option<u64> {
        self.bates.as_ref()?.get(&page).copied()
    }
}

/// What a generator draws.
#[derive(Debug, Clone, PartialEq)]
pub enum GenSpec {
    /// The header and footer, and Bates numbers.
    HeaderFooter(Arc<HeaderFooterSpec>),
    /// A watermark or background.
    Watermark(Arc<WatermarkGen>),
}

impl GenSpec {
    /// The header and footer it draws, if it's one.
    pub fn header_footer(&self) -> Option<&Arc<HeaderFooterSpec>> {
        match self {
            GenSpec::HeaderFooter(hf) => Some(hf),
            GenSpec::Watermark(_) => None,
        }
    }

    /// The watermark or background it draws, if it's one.
    pub fn watermark(&self) -> Option<&Arc<WatermarkGen>> {
        match self {
            GenSpec::Watermark(wm) => Some(wm),
            GenSpec::HeaderFooter(_) => None,
        }
    }
}

/// The pages a generator draws on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    /// Every page, those added later too.
    All,
    /// These, never none: a generator on none is gone.
    Pages(im::OrdSet<PageId>),
}

impl Scope {
    /// Whether page `page` is among them: O(log n).
    pub fn covers(&self, page: PageId) -> bool {
        match self {
            Scope::All => true,
            Scope::Pages(pages) => pages.contains(&page),
        }
    }
}

/// A watermark or background (M10.5 §3.5's `WatermarkSpec`): what it
/// draws, and where on each page. Laid out on each page as it shows when a
/// save writes it, so it stays upright through a turn, and on the page's
/// own boxes; its generator says whether it's over the page's content or
/// under it.
#[derive(Debug, Clone, PartialEq)]
pub struct WatermarkGen {
    pub content: WatermarkContent,
    pub position: GridPos,
}

impl WatermarkGen {
    /// The kind of pagination artifact one at `z` is: a watermark over the
    /// content, a background under it.
    pub fn kind_at(z: Z) -> PaginationKind {
        match z {
            Z::Over => PaginationKind::Watermark,
            Z::Under => PaginationKind::Background,
        }
    }
}

/// What a watermark draws: an op's [`sheaf_edit::WatermarkContent`], its
/// file held.
#[derive(Debug, Clone, PartialEq)]
pub enum WatermarkContent {
    /// `text` in `font`, kept as it was said, though the font may draw
    /// some of it as `?`.
    Text {
        text: Arc<str>,
        font: FontChoice,
        size: f32,
        color: Color,
        opacity: f32,
        angle: f32,
    },
    Image {
        image: ImageRef,
        scale: f32,
        opacity: f32,
    },
    Page {
        page: PageForm,
        scale: f32,
        opacity: f32,
    },
}

/// The image a watermark draws.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageRef {
    /// An image file in the store.
    Blob(BlobRef),
    /// The image XObject a saved block draws, which an open binds it to.
    Placed(Placed),
}

/// The PDF page a watermark draws (a stamp's is a
/// [`stamp_lib::PageForm`](crate::stamp_lib::PageForm)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PageForm {
    /// Page `index` (0-based) of a PDF in the store.
    Blob { pdf: BlobRef, index: u32 },
    /// The form XObject a saved block draws, which an open binds it to.
    Placed(Placed),
}

/// An image or a page a saved block draws, as an open finds it: the
/// XObject, the resource it was drawn as (its digest, which named it and
/// which the blocks' layouts were digested with, so a block laid out alike
/// again is the one the file has), and the quarter turns clockwise an
/// image shows upright at (0 for a page). The file's `/SheafGenerators`
/// says the last two.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placed {
    pub at: ObjRef,
    pub res: ResId,
    pub turns: u8,
}

/// Which of a page's places in the document a generator's tokens read
/// (M10.5 §3.5), besides the page's boxes and turn, which its layout always
/// does: what a reorder or new labels change on the pages it's on.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CtxNeeds {
    /// The page's position (`<<1>>`).
    pub index: bool,
    /// How many pages there are (`<<N>>`).
    pub count: bool,
    /// The page's label (`<<label>>`).
    pub label: bool,
    /// The page's Bates number (`<<bates>>`), fixed when it was set.
    pub bates: bool,
}

impl CtxNeeds {
    /// Whether it reads where the page is in the order, which a reorder
    /// changes: its position, the count or its label.
    pub fn order(self) -> bool {
        self.index || self.count || self.label
    }
}

/// What a generator's layout reads of one page (M10.5 §3.5): its boxes and
/// turn, its position (0-based) and the page count, its label and its
/// Bates number. What its tokens don't read ([`Generator::needs`]) may be
/// left at its default.
#[derive(Debug, Clone, PartialEq)]
pub struct PageCtx {
    pub geom: Geom,
    pub position: u32,
    pub count: u32,
    pub label: Arc<str>,
    pub bates: Option<u64>,
}

impl Generator {
    /// What its tokens read of the pages it's on: nothing, for a
    /// watermark.
    pub fn needs(&self) -> CtxNeeds {
        match &self.spec {
            GenSpec::HeaderFooter(hf) => crate::generate::header::needs(hf),
            GenSpec::Watermark(_) => CtxNeeds::default(),
        }
    }

    /// Whether it draws on page `page`.
    pub fn covers(&self, page: PageId) -> bool {
        self.scope.covers(page)
    }

    /// The kinds of pagination artifact it draws (§11.4): a watermark's or
    /// a background's own, a header and footer's both.
    pub fn kinds(&self) -> BTreeSet<PaginationKind> {
        match &self.spec {
            GenSpec::HeaderFooter(_) => crate::generate::header::kinds(),
            GenSpec::Watermark(_) => BTreeSet::from([WatermarkGen::kind_at(self.z)]),
        }
    }

    /// Whether it and `other` are of one kind and z, of which a document
    /// has one generator at most (M10.5 §6.4): a header and footer, a
    /// watermark, a background.
    pub(crate) fn same_place(&self, other: &Generator) -> bool {
        let kind = |g: &Generator| match g.spec {
            GenSpec::HeaderFooter(_) => 0,
            GenSpec::Watermark(_) => 1,
        };
        kind(self) == kind(other) && self.z == other.z
    }

    /// It once only the pages `stays` says stay are left: on those of the
    /// pages its scope names, or none when that's none of them; every page
    /// when it names none; the Bates numbers of the pages that went
    /// dropped, never given to another. How many of the pages it names
    /// went: its scope's, or with every page its Bates numbers'.
    pub(crate) fn on_pages_left(
        self: &Arc<Self>,
        stays: impl Fn(PageId) -> bool,
    ) -> (Option<Arc<Generator>>, usize) {
        let (scope, went) = match &self.scope {
            Scope::All => (Scope::All, 0),
            Scope::Pages(on) => {
                let left: im::OrdSet<PageId> = on.iter().copied().filter(|&p| stays(p)).collect();
                if left.is_empty() {
                    return (None, on.len());
                }
                let went = on.len() - left.len();
                (Scope::Pages(left), went)
            }
        };
        // The numbers left share the rest of the map's nodes.
        let bates = self.frozen.bates.as_ref().map(|numbers| {
            let mut left = im::OrdMap::clone(numbers);
            for p in numbers.keys().filter(|&&p| !stays(p)) {
                left.remove(p);
            }
            (numbers.len() - left.len(), left)
        });
        // A page with a number is one it covers.
        let went = went.max(bates.as_ref().map_or(0, |(went, _)| *went));
        if went == 0 {
            return (Some(self.clone()), 0);
        }
        let frozen = Frozen {
            bates: bates.map(|(_, left)| Arc::new(left)),
            ..self.frozen.clone()
        };
        let g = Generator {
            scope,
            frozen,
            ..Generator::clone(self)
        };
        (Some(Arc::new(g)), went)
    }
}
