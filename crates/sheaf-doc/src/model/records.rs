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
//! A stack holds what something makes today: recognized text. Filters on
//! the page's own content, layers under it, a layer's id and kind, and
//! placed forms come with the features that make them (slices 16–19). A
//! generator is today's header/footer, with its own scope, date and file,
//! and its Bates numbers counted at each Rewrite; its scope, frozen values
//! and the watermark come with slices 16–17. A form instance is one per
//! insert of a source's pages, or duplicate of them, and the form's own
//! keys (`/DR`, `/CO`, `/DA`…) are written from [`FormDoc`].

use std::fmt;
use std::sync::{Arc, LazyLock, OnceLock};

use anyhow::Result;
use im::Vector;
use sheaf_edit::DestView;

use crate::annot::{Annot, AnnotId};
use crate::file::ForeignAnnot;
use crate::forms::field::{Action, Border, Choice, Mk, Trigger};
use crate::forms::{FieldId, FieldKind, Value};
use crate::model::{OutlineId, PageId, SourceId, TextStyle};
use crate::source::ObjRef;
use crate::stamps::HeaderFooter;
use crate::store::{BlobRef, Src};
use crate::text_layer::TextLayer;

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

/// What's drawn over a page's own content, bottom first.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ContentStack {
    pub over: Vector<Layer>,
}

/// One thing drawn over a page.
#[derive(Debug, Clone, PartialEq)]
pub struct Layer {
    pub body: Static,
}

/// A layer's content, fixed when it's made.
#[derive(Debug, Clone, PartialEq)]
pub enum Static {
    /// Recognized text, invisible, its words in PageSpace.
    OcrText(Arc<TextLayer>),
}

impl Static {
    /// The recognized text it is, if it is some.
    pub fn ocr_text(&self) -> Option<&Arc<TextLayer>> {
        match self {
            Static::OcrText(text) => Some(text),
        }
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

    /// The stack with `text` as its recognized text, in place of what it
    /// had, or with none: the empty one when that leaves nothing.
    pub fn with_ocr_text(&self, text: Option<Arc<TextLayer>>) -> Arc<ContentStack> {
        let mut over = self.over.clone();
        over.retain(|l| l.body.ocr_text().is_none());
        over.extend(text.map(|text| Layer {
            body: Static::OcrText(text),
        }));
        if over.is_empty() {
            return ContentStack::empty();
        }
        Arc::new(ContentStack { over })
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

/// The page a page stamp draws, as a form.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum FormRef {
    /// The form a source's appearance of the stamp draws, as a file sheaf
    /// saved holds it.
    Base(ObjRef),
    /// A one-page PDF, its page the form: a library stamp placed, or a
    /// stamp pasted from another document.
    Blob(BlobRef),
}

impl FormRef {
    /// Its page as a one-page PDF, to show or copy: the blob's own, or one
    /// made of the form.
    pub fn to_pdf(&self) -> Result<Arc<[u8]>> {
        match self {
            FormRef::Blob(pdf) => Ok(pdf.bytes().clone()),
            FormRef::Base(at) => Ok(crate::stamp_lib::form_to_pdf(at.src.pdf(), at.id)?.into()),
        }
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

/// A generator's id: random in [1, 2^53), as an id a file will hold is
/// (M10.5 §4), so two files' never meet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct GenId(pub u64);

impl GenId {
    /// One drawn from `rng` that `taken` says no generator has.
    pub(crate) fn draw(rng: &mut fastrand::Rng, taken: impl Fn(GenId) -> bool) -> GenId {
        loop {
            let id = GenId(rng.u64(1..1 << 53));
            if !taken(id) {
                return id;
            }
        }
    }
}

/// Content sheaf draws on pages from one definition, laid out on each
/// page as a save writes it.
#[derive(Debug, Clone, PartialEq)]
pub struct Generator {
    pub id: GenId,
    pub spec: GenSpec,
}

/// What a generator draws.
#[derive(Debug, Clone, PartialEq)]
pub enum GenSpec {
    /// The header/footer as sheaf drew it before generators (until slice
    /// 17): the pages it's on, its date and file fixed when it was set,
    /// Bates numbers counted at each Rewrite, and drawn only by one.
    HeaderFooter(Arc<HeaderFooter>),
}

impl GenSpec {
    /// The header/footer it draws, if it's one.
    pub fn header_footer(&self) -> Option<&Arc<HeaderFooter>> {
        match self {
            GenSpec::HeaderFooter(hf) => Some(hf),
        }
    }

    /// Whether it and `other` are of one kind and z, of which a document
    /// has one generator at most (M10.5 §6.4). A header/footer has no z:
    /// a document has one.
    pub(crate) fn same_place(&self, other: &GenSpec) -> bool {
        match (self, other) {
            (GenSpec::HeaderFooter(_), GenSpec::HeaderFooter(_)) => true,
        }
    }
}

/// Which of a page's places in the document a generator's tokens read
/// (M10.5 §3.5), besides the page's boxes and turn, which its layout always
/// does: what a reorder changes on the pages it's on.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CtxNeeds {
    /// The page's position (`<<1>>`).
    pub index: bool,
    /// How many pages there are (`<<N>>`).
    pub count: bool,
    /// The page's Bates number (`<<bates>>`).
    pub bates: bool,
}

impl CtxNeeds {
    /// Whether it reads any of them.
    pub fn any(self) -> bool {
        self.index || self.count || self.bates
    }
}

impl Generator {
    /// What its tokens read of the pages it's on.
    pub fn needs(&self) -> CtxNeeds {
        match &self.spec {
            GenSpec::HeaderFooter(hf) => hf.needs(),
        }
    }

    /// It once only the pages `stays` says stay are left: on those of the
    /// pages it names, or none when that's none of them; every page when it
    /// names none. How many of the pages it names went.
    pub(crate) fn on_pages_left(
        self: &Arc<Self>,
        stays: impl Fn(PageId) -> bool,
    ) -> (Option<Arc<Generator>>, usize) {
        match &self.spec {
            GenSpec::HeaderFooter(hf) => {
                let Some(scope) = &hf.pages else {
                    return (Some(self.clone()), 0);
                };
                let left: Vec<PageId> = scope.iter().copied().filter(|&p| stays(p)).collect();
                let went = scope.len() - left.len();
                if went == 0 {
                    return (Some(self.clone()), 0);
                }
                let g = (!left.is_empty()).then(|| {
                    let hf = HeaderFooter {
                        pages: Some(left),
                        ..HeaderFooter::clone(hf)
                    };
                    Arc::new(Generator {
                        id: self.id,
                        spec: GenSpec::HeaderFooter(Arc::new(hf)),
                    })
                });
                (g, went)
            }
        }
    }
}
