//! Transactions (M10.5 §6.3): ops lowered into prims. A [`Tx`] collects
//! ops against the version it was made at, each lowered at once into the
//! transaction's own state, so a later op reads what an earlier one did;
//! [`Document::commit`] makes them one version.
//!
//! [`Tx::apply`] runs in three stages, all or nothing for the op:
//! 1. **Resolve** ([`resolve`]): references become ids, locals what
//!    earlier ops made, blobs what the store or the transaction's grants
//!    hold.
//! 2. **Check capabilities** ([`caps`]): whether the actor
//!    [`Document::lower_cx`] bound may make the change, before anything is
//!    lowered.
//! 3. **Lower**: the op written into the state through a recording
//!    context, one module per family of ops, then the cascades
//!    ([`cascade`]) to a fixed point. Lowering reads only the
//!    transaction's state, sources' facts, the store, the host's
//!    attributes (one date per transaction) and the id allocator.
//!
//! What it makes is the host's to name (§6.8): ids and `/NM`s, dates, the
//! default author, the identity tokens in a mark's text, and a
//! connector's partner. Each op is a [`Step`] with its line ([`describe`]),
//! its effects ([`StepEffects`]) and its cascades; the commit tells the
//! reader its [`Notice`]s.
//!
//! [`Document::commit`]: crate::model::Document::commit
//! [`Document::lower_cx`]: crate::model::Document::lower_cx
//! [`describe`]: crate::describe
//! [`StepEffects`]: crate::effects::StepEffects

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt::Write;
use std::sync::Arc;

use im::Vector;
use in_progress::Progress;
use sheaf_edit::{BlobSel, Local, Op};

use crate::annot::AnnotId;
use crate::effects::StepEffects;
use crate::forms::FieldId;
use crate::labels::LabelSpec;
use crate::model::{
    Bookmark, DocState, FieldNode, FormDoc, FormRef, IdAlloc, OutlineId, PageDef, PageId, Step,
    VersionId,
};
use crate::prim::{Key, Rec, Val, same};
use crate::store::{Blob, BlobRef, Src, Store};

pub(crate) mod bookmarks;
pub mod caps;
pub mod cascade;
mod content;
pub mod error;
#[cfg(any(test, feature = "fixtures"))]
pub mod fixture;
pub(crate) mod forms;
pub(crate) mod marks;
pub(crate) mod pages;
pub mod resolve;

#[cfg(test)]
mod cascade_tests;
#[cfg(test)]
pub(crate) mod tests;

pub use caps::{Actor, Cap, JobKind, Target, actor_allows, cap_of};
pub use cascade::{Cascade, Rule};
pub use error::{EditError, Notice, notice_text};
pub use marks::{mark_changes, mark_kind, mark_patch, mark_style};
pub use resolve::{At, BookmarkChange, Derive, ResolvedOp, incoming, op_name, ready_to_insert};

/// Who the user is, as the host fills a mark in (§6.8): the name marks are
/// by, the initials dynamic stamps write, and how they write the date and
/// time (`chrono`'s `strftime` formats).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    pub person: Arc<str>,
    pub initials: Arc<str>,
    pub date_format: Arc<str>,
    pub time_format: Arc<str>,
}

/// How dynamic stamps write the date and time unless the user says.
const DATE: &str = "%m/%d/%Y";
const TIME: &str = "%-I:%M %p";

impl Default for Identity {
    /// The OS user, and the stamps' default formats.
    fn default() -> Identity {
        let person = crate::annot::default_author();
        Identity {
            initials: crate::annot::initials_of(&person).into(),
            person: person.into(),
            date_format: DATE.into(),
            time_format: TIME.into(),
        }
    }
}

/// What lowering sets that no caller may (§6.8), taken once a
/// transaction: who the user is, and now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostAttrs {
    pub person: Arc<str>,
    pub initials: Arc<str>,
    /// Now, as a PDF date (`D:20261019104000Z`): what a mark is made or
    /// changed at.
    pub now: Arc<str>,
    /// Now as a dynamic stamp writes it (`<<date>>`, `<<time>>`), and a
    /// header set now (`<<date>>`).
    pub date: Arc<str>,
    pub time: Arc<str>,
    /// The name of the document's file, as a header set now writes
    /// `<<file>>`.
    pub file: Arc<str>,
}

impl HostAttrs {
    /// `who`, now, in a document named `file`.
    pub fn now(who: &Identity, file: Arc<str>) -> HostAttrs {
        let local = chrono::Local::now();
        // A format the user wrote that isn't one is the default's.
        let written = |format: &str, otherwise: &str| -> Arc<str> {
            let mut out = String::new();
            match write!(out, "{}", local.format(format)) {
                Ok(()) => out.into(),
                Err(_) => local.format(otherwise).to_string().into(),
            }
        };
        HostAttrs {
            person: who.person.clone(),
            initials: who.initials.clone(),
            now: crate::annot::pdf_date_now().into(),
            date: written(&who.date_format, DATE),
            time: written(&who.time_format, TIME),
            file,
        }
    }
}

/// What a transaction lowers with: the document's store and primary, its
/// id allocator, the host's attributes, and the actor. Only
/// [`Document::lower_cx`](crate::model::Document::lower_cx) makes one.
pub struct LowerCx {
    pub(crate) store: Arc<Store>,
    pub(crate) primary: Src,
    pub(crate) alloc: Arc<IdAlloc>,
    pub(crate) host: HostAttrs,
    pub(crate) actor: Actor,
    /// The document's file has opened: until then every op is refused
    /// before it's lowered ([`EditError::NotReady`]).
    pub(crate) ready: bool,
}

impl LowerCx {
    /// Who the user is, and now, as this transaction fills marks in.
    pub fn host(&self) -> &HostAttrs {
        &self.host
    }
}

/// A blob a transaction may name by handle ([`BlobSel::Asset`] or
/// [`BlobSel::With`]): what a plugin uploaded, what the command line was
/// given, or what the host found a stem to name.
#[derive(Debug, Clone, PartialEq)]
pub enum Grant {
    /// A file: pages to insert, or an attachment's.
    Blob(BlobRef),
    /// A page stamp's page, as its marks name it by `stem`: a blob's
    /// one-page PDF, or the form a source's mark draws it with.
    Stamp { stem: String, form: FormRef },
}

/// Something an op made, which a local can name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Made {
    Page(PageId),
    Bookmark(OutlineId),
    Annot(AnnotId),
}

/// What a transaction's ops made: each op's, in order, and what each
/// local names.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Created {
    pub ops: Vec<Vec<Made>>,
    pub locals: BTreeMap<Local, Made>,
}

impl Created {
    /// Everything the ops made, in order.
    pub fn made(&self) -> impl Iterator<Item = Made> + '_ {
        self.ops.iter().flatten().copied()
    }

    /// The pages they made, in order.
    pub fn pages(&self) -> impl Iterator<Item = PageId> + '_ {
        self.made().filter_map(|m| match m {
            Made::Page(id) => Some(id),
            _ => None,
        })
    }

    /// The bookmarks they made, in order.
    pub fn bookmarks(&self) -> impl Iterator<Item = OutlineId> + '_ {
        self.made().filter_map(|m| match m {
            Made::Bookmark(id) => Some(id),
            _ => None,
        })
    }

    /// The marks they made, in order.
    pub fn annots(&self) -> impl Iterator<Item = AnnotId> + '_ {
        self.made().filter_map(|m| match m {
            Made::Annot(id) => Some(id),
            _ => None,
        })
    }
}

/// What one op made.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct OpResult {
    pub made: Vec<Made>,
}

/// How a transaction is committed.
#[derive(Debug, Clone, Default)]
pub struct CommitOpts {
    /// What undo calls the version; its first line when none.
    pub label: Option<Arc<str>>,
}

/// What a commit did.
#[derive(Debug, Clone)]
pub struct Commit {
    /// The version it made, or amended; None when it changed nothing.
    pub version: Option<VersionId>,
    /// It joined the job's version rather than making one
    /// ([`Document::apply_in_job`](crate::model::Document::apply_in_job)).
    pub amended: bool,
    pub created: Created,
    /// What its steps did, together.
    pub effects: StepEffects,
    /// Its lines, as the History panel shows them.
    pub lines: Vec<crate::describe::Line>,
    /// What the cascades did, by rule.
    pub cascades: Vec<Cascade>,
    pub notices: Vec<Notice>,
}

/// Ops against a version, lowered as they come (§6.3). Send and
/// `'static`: it can be built off the UI thread.
pub struct Tx {
    base: VersionId,
    /// The state of the version it's based on, and the one its ops made.
    base_state: DocState,
    state: DocState,
    steps: Vec<Step>,
    /// What each local names: im collections, so a checkpoint is cheap.
    locals: im::HashMap<Local, Made>,
    created: Created,
    notices: Vec<Notice>,
    /// Lowering reads: the value each slot the ops read without writing
    /// it held when first read.
    seen: im::OrdMap<Key, Option<Val>>,
    /// Decision reads: what the caller based its ops on ([`read`],
    /// [`reader`]).
    ///
    /// [`read`]: Tx::read
    /// [`reader`]: Tx::reader
    decided: BTreeMap<Key, Option<Val>>,
    /// Blobs the caller interned for its ops, held until they're
    /// committed, when the records hold what they use.
    pins: Vec<Blob>,
    grants: Vec<Grant>,
    named: HashMap<String, usize>,
    cx: Arc<LowerCx>,
}

/// Where a transaction was, for putting it back: what's appended to, and
/// what's replaced.
struct Checkpoint {
    state: DocState,
    steps: usize,
    ops: usize,
    notices: usize,
    locals: im::HashMap<Local, Made>,
    seen: im::OrdMap<Key, Option<Val>>,
}

impl Tx {
    /// A transaction on `state`, version `base`'s, lowering with `cx`.
    pub(crate) fn new(base: VersionId, state: DocState, cx: Arc<LowerCx>) -> Tx {
        Tx {
            base,
            base_state: state.clone(),
            state,
            steps: Vec::new(),
            locals: im::HashMap::new(),
            created: Created::default(),
            notices: Vec::new(),
            seen: im::OrdMap::new(),
            decided: BTreeMap::new(),
            pins: Vec::new(),
            grants: Vec::new(),
            named: HashMap::new(),
            cx,
        }
    }

    /// `op`, lowered: all of it, or nothing and why not. Canceled by `p`,
    /// it leaves the transaction as it was.
    pub fn apply(&mut self, op: Op, p: &dyn Progress) -> Result<OpResult, EditError> {
        self.ready()?;
        let (resolved, reads) = self.resolve(&op)?;
        self.allowed(&resolved)?;
        self.lower(resolved, reads, p)
    }

    /// `ops`, each lowered against what the ones before it did: all of
    /// them, or none and why.
    pub fn apply_batch(
        &mut self,
        ops: Vec<Op>,
        p: &dyn Progress,
    ) -> Result<Vec<OpResult>, EditError> {
        let back = self.checkpoint();
        let mut out = Vec::with_capacity(ops.len());
        for op in ops {
            match self.apply(op, p) {
                Ok(r) => out.push(r),
                Err(e) => {
                    self.rewind(back);
                    return Err(e);
                }
            }
        }
        Ok(out)
    }

    /// Whether `op` could be applied now: its references resolve, and the
    /// actor may make it. Nothing is lowered, and no id is drawn.
    pub fn check(&self, op: &Op) -> Result<(), EditError> {
        self.ready()?;
        let (resolved, _) = self.resolve(op)?;
        self.allowed(&resolved)
    }

    /// A decision read: the caller based its ops on `key`'s value.
    pub fn read(&mut self, key: Key) {
        self.reader().note(key);
    }

    /// The transaction's state, as a caller deciding its ops reads it:
    /// each read noted as a decision read.
    pub fn reader(&mut self) -> Reader<'_> {
        Reader { tx: self }
    }

    /// Holds `blob` until the transaction is committed or dropped: a blob
    /// the caller interned for its ops, which nothing else holds yet.
    pub fn pin(&mut self, blob: Blob) {
        self.pins.push(blob);
    }

    /// `grant`, for the ops to name as [`BlobSel::Asset`].
    pub fn grant(&mut self, grant: Grant) -> BlobSel {
        self.grants.push(grant);
        BlobSel::Asset(self.grants.len() as u32 - 1)
    }

    /// `grant`, for the ops to name as [`BlobSel::With`] `name`.
    pub fn grant_named(&mut self, name: impl Into<String>, grant: Grant) -> BlobSel {
        let name = name.into();
        self.grants.push(grant);
        self.named.insert(name.clone(), self.grants.len() - 1);
        BlobSel::With(name)
    }

    /// The state its ops have made so far.
    pub fn state(&self) -> &DocState {
        &self.state
    }

    /// The internal page rebase of a derived flow (transitional, until
    /// slice 17): each page shown from page `index` of `src`, a copy the
    /// flow made and noted ([`Document::add_derived`]), as `why` says.
    ///
    /// [`Document::add_derived`]: crate::model::Document::add_derived
    pub fn set_page_base(
        &mut self,
        pages: Vec<(PageId, Src, u32)>,
        why: Derive,
    ) -> Result<OpResult, EditError> {
        self.ready()?;
        let op = "set-page-base";
        for &(id, ref src, index) in &pages {
            if self.state.def(id).is_none() {
                return Err(EditError::not_found(op, format!("page {}", id.0)));
            }
            if index >= src.page_count() {
                return Err(EditError::invalid(
                    op,
                    format!("page {} isn't in the source", index + 1),
                ));
            }
        }
        let resolved = ResolvedOp::SetPageBase { pages, why };
        self.allowed(&resolved)?;
        self.lower(resolved, BTreeSet::new(), &in_progress::NoProgress)
    }

    /// Ops wait for the document's file to open.
    fn ready(&self) -> Result<(), EditError> {
        match self.cx.ready {
            true => Ok(()),
            false => Err(EditError::NotReady),
        }
    }

    /// `op` with its references resolved against the state as it is, and
    /// the slots resolving read.
    fn resolve(&self, op: &Op) -> Result<(ResolvedOp, BTreeSet<Key>), EditError> {
        let mut r = resolve::Resolver {
            st: &self.state,
            locals: &self.locals,
            cx: &self.cx,
            grants: &self.grants,
            named: &self.named,
            op: op_name(op),
            reads: BTreeSet::new(),
        };
        let resolved = r.op(op)?;
        Ok((resolved, r.reads))
    }

    /// Whether the actor may make `op`.
    fn allowed(&self, op: &ResolvedOp) -> Result<(), EditError> {
        let cap = cap_of(op, &caps::targets(op, &self.state));
        match actor_allows(&self.cx.actor, cap, op) {
            true => Ok(()),
            false => Err(EditError::Refused(cap)),
        }
    }

    /// `op`, resolved and allowed, lowered into a step.
    fn lower(
        &mut self,
        op: ResolvedOp,
        reads: BTreeSet<Key>,
        p: &dyn Progress,
    ) -> Result<OpResult, EditError> {
        let before = self.state.clone();
        let line = crate::describe::describe(&op, &before);
        let mut lw = Lower::new(Rec::new(before.clone()), &self.cx, p, op.name());
        lw.reads = reads;
        lower_op(&mut lw, &op)?;
        let cascades = cascade::settle(&mut lw)?;
        let Lower {
            rec,
            made,
            notices,
            reads,
            ..
        } = lw;
        let names = op.locals();
        if names.len() > made.len() {
            return Err(EditError::invalid(
                op.name(),
                format!(
                    "{} names nothing: it made {}",
                    names[made.len()],
                    made.len()
                ),
            ));
        }
        let (state, prims) = rec.finish();
        debug_assert_eq!(state.check_invariants(), Ok(()), "{line}");
        let effects = StepEffects::of(&before, &state, &prims);
        for key in reads {
            if !self.seen.contains_key(&key) && self.unwritten(&key) {
                self.seen.insert(key, before.get(&key));
            }
        }
        for (&l, &m) in names.iter().zip(&made) {
            self.locals.insert(l, m);
        }
        self.created.ops.push(made.clone());
        self.notices.extend(notices);
        self.state = state;
        self.steps.push(Step {
            op: Arc::new(op),
            prims: prims.into(),
            effects,
            line,
            cascades: cascades.into(),
        });
        Ok(OpResult { made })
    }

    /// Whether `key` holds what it held when the transaction began: a slot
    /// it wrote holds its own value, which a read of it doesn't depend on.
    fn unwritten(&self, key: &Key) -> bool {
        same(
            self.base_state.get(key).as_ref(),
            self.state.get(key).as_ref(),
        )
    }

    fn checkpoint(&self) -> Checkpoint {
        Checkpoint {
            state: self.state.clone(),
            steps: self.steps.len(),
            ops: self.created.ops.len(),
            notices: self.notices.len(),
            locals: self.locals.clone(),
            seen: self.seen.clone(),
        }
    }

    fn rewind(&mut self, m: Checkpoint) {
        self.state = m.state;
        self.steps.truncate(m.steps);
        self.created.ops.truncate(m.ops);
        self.notices.truncate(m.notices);
        self.locals = m.locals;
        self.seen = m.seen;
    }

    /// What a commit takes of it.
    pub(crate) fn into_parts(self) -> TxParts {
        let locals = self.locals.into_iter().collect();
        TxParts {
            base: self.base,
            state: self.state,
            steps: self.steps,
            created: Created {
                locals,
                ..self.created
            },
            notices: self.notices,
            actor: self.cx.actor.clone(),
        }
    }
}

/// A transaction as a commit takes it.
pub(crate) struct TxParts {
    pub(crate) base: VersionId,
    pub(crate) state: DocState,
    pub(crate) steps: Vec<Step>,
    pub(crate) created: Created,
    pub(crate) notices: Vec<Notice>,
    pub(crate) actor: Actor,
}

/// The transaction's state as a caller deciding its ops reads it: each
/// slot read is a decision read, by the value read, unless the
/// transaction changed it.
pub struct Reader<'a> {
    tx: &'a mut Tx,
}

impl Reader<'_> {
    fn note(&mut self, key: Key) {
        if !self.tx.decided.contains_key(&key) && self.tx.unwritten(&key) {
            let val = self.tx.state.get(&key);
            self.tx.decided.insert(key, val);
        }
    }

    /// The pages in document order.
    pub fn order(&mut self) -> &Vector<PageId> {
        self.note(Key::Order);
        self.tx.state.order()
    }

    /// The page at position `pos`, 0-based.
    pub fn page_at(&mut self, pos: usize) -> Option<PageId> {
        self.note(Key::Order);
        self.tx.state.page_at(pos)
    }

    /// Page `id`'s position, 0-based.
    pub fn pos_of(&mut self, id: PageId) -> Option<usize> {
        self.note(Key::Order);
        self.tx.state.pos_of(id)
    }

    pub fn def(&mut self, id: PageId) -> Option<&Arc<PageDef>> {
        self.note(Key::Page(id));
        self.tx.state.def(id)
    }

    pub fn geom(&mut self, id: PageId) -> Option<&Arc<crate::geom::Geom>> {
        self.note(Key::Geom(id));
        self.tx.state.geom(id)
    }

    /// Page `id`'s annotations, in `/Annots` order.
    pub fn annot_ids_on(&mut self, id: PageId) -> Option<&Vector<AnnotId>> {
        self.note(Key::PageAnnots(id));
        self.tx.state.annot_ids_on(id)
    }

    /// Page `id`'s marks, sheaf's own, in `/Annots` order.
    pub fn marks_on(&mut self, id: PageId) -> impl Iterator<Item = &crate::annot::Annot> {
        self.note(Key::PageAnnots(id));
        let ids: Vec<AnnotId> = (self.tx.state.marks_on(id)).map(|a| a.id).collect();
        for a in ids {
            self.note(Key::Annot(a));
        }
        self.tx.state.marks_on(id)
    }

    /// The pages bookmark `id` spans: from its page up to the next
    /// bookmark's ([`DocState::bookmark_pages`]).
    pub fn bookmark_pages(&mut self, id: OutlineId) -> Option<Vec<PageId>> {
        self.note(Key::Order);
        self.note(Key::Bookmark(id));
        self.tx.state.bookmark_pages(id)
    }

    pub fn annot(&mut self, id: AnnotId) -> Option<&crate::model::AnnotRec> {
        self.note(Key::Annot(id));
        self.tx.state.annot(id)
    }

    pub fn bookmark(&mut self, id: OutlineId) -> Option<&Arc<Bookmark>> {
        self.note(Key::Bookmark(id));
        self.tx.state.bookmark(id)
    }

    pub fn outline_roots(&mut self) -> &Vector<OutlineId> {
        self.note(Key::OutlineRoots);
        self.tx.state.outline_roots()
    }

    pub fn labels(&mut self) -> Option<&Arc<[LabelSpec]>> {
        self.note(Key::Labels);
        self.tx.state.labels()
    }

    pub fn form(&mut self) -> &Arc<FormDoc> {
        self.note(Key::Form);
        self.tx.state.form()
    }

    pub fn field(&mut self, id: FieldId) -> Option<&Arc<FieldNode>> {
        self.note(Key::Field(id));
        self.tx.state.field(id)
    }
}

/// One op being lowered: the state it writes, what it may read, and what
/// it makes.
pub(crate) struct Lower<'a> {
    pub(crate) rec: Rec,
    pub(crate) cx: &'a LowerCx,
    progress: &'a dyn Progress,
    /// The op's name, for its errors.
    pub(crate) op: &'static str,
    /// What it makes that its locals name, in order.
    pub(crate) made: Vec<Made>,
    pub(crate) notices: Vec<Notice>,
    /// Slots it reads and may not write.
    pub(crate) reads: BTreeSet<Key>,
    /// Annotation ids drawn so far: not yet in the state.
    taken: HashSet<AnnotId>,
}

impl<'a> Lower<'a> {
    pub(crate) fn new(
        rec: Rec,
        cx: &'a LowerCx,
        progress: &'a dyn Progress,
        op: &'static str,
    ) -> Lower<'a> {
        Lower {
            rec,
            cx,
            progress,
            op,
            made: Vec::new(),
            notices: Vec::new(),
            reads: BTreeSet::new(),
            taken: HashSet::new(),
        }
    }

    /// A mark id no annotation has, nor one drawn before in this op.
    /// Random rather than counted, so two copies of a file never give
    /// different marks the same `/NM`; 53 bits, so the JSON is exact in any
    /// reader.
    pub(crate) fn annot_id(&mut self) -> AnnotId {
        loop {
            let id = AnnotId(fastrand::u64(1..1 << 53));
            if !self.taken.contains(&id) && !self.rec.has_annot(id) {
                self.taken.insert(id);
                return id;
            }
        }
    }

    /// A lowering read of `key`.
    pub(crate) fn read(&mut self, key: Key) {
        self.reads.insert(key);
    }

    /// Progress through `total` pages: `done` of them.
    pub(crate) fn pages_done(&self, done: usize, total: usize) -> Result<(), EditError> {
        if done == 0 {
            self.progress.set_total(Some(total as u64), crate::PAGES);
        }
        self.progress.set(done as u64);
        self.progress.check().map_err(|_| EditError::Cancelled)
    }

    pub(crate) fn not_found(&self, what: impl std::fmt::Display) -> EditError {
        EditError::not_found(self.op, what)
    }

    pub(crate) fn invalid(&self, why: impl std::fmt::Display) -> EditError {
        EditError::invalid(self.op, why)
    }
}

/// `op` lowered into `lw`.
fn lower_op(lw: &mut Lower, op: &ResolvedOp) -> Result<(), EditError> {
    use ResolvedOp as R;
    match op {
        R::MovePages { pages, before } => {
            pages::move_pages(lw, pages, *before);
            Ok(())
        }
        R::RotatePages { pages, by } => {
            pages::rotate(lw, pages, by.rotation());
            Ok(())
        }
        R::DeletePages { pages } => pages::delete(lw, pages),
        R::DuplicatePages { pages, .. } => pages::duplicate(lw, pages),
        R::InsertPages {
            from,
            name,
            pages,
            before,
            bring,
            ..
        } => pages::insert(lw, from, name, pages, *before, bring),
        R::ReplacePages {
            pages,
            from,
            name,
            with,
            bring,
        } => pages::replace(lw, pages, from, name, with, bring),
        R::SetPageLabels { ranges } => {
            pages::set_labels(lw, ranges.clone());
            Ok(())
        }
        R::SetPageBase { pages, why } => {
            pages::set_base(lw, pages);
            // A watermark's text is drawn into the copies, out of the
            // model's sight: the flow says what it drew.
            if let Derive::Watermark {
                unencodable: true, ..
            } = why
            {
                lw.notices.push(Notice::Unencodable);
            }
            Ok(())
        }
        R::AddBookmark {
            parent,
            at,
            title,
            target,
            view,
            color,
            style,
            ..
        } => {
            let b = Bookmark {
                title: title.as_str().into(),
                target: target.map(|page| crate::model::Target::Page {
                    page,
                    view: view.clone(),
                }),
                color: *color,
                style: *style,
                open: true,
                kids: Vector::new(),
                src: None,
            };
            bookmarks::add(lw, *parent, *at, b)
        }
        R::PatchBookmarks { bookmarks, patch } => {
            bookmarks::patch(lw, bookmarks, patch);
            Ok(())
        }
        R::MoveBookmarks {
            bookmarks,
            parent,
            at,
            with_pages,
        } => bookmarks::move_bookmarks(lw, bookmarks, *parent, *at, *with_pages),
        R::DeleteBookmarks { bookmarks } => {
            bookmarks::delete(lw, bookmarks);
            Ok(())
        }
        R::AddMark {
            page,
            mark,
            held,
            reply_to,
            imported,
            ..
        } => marks::add(lw, *page, mark, held.as_ref(), *reply_to, imported.as_ref()),
        R::AddConnector {
            a, b, label, style, ..
        } => {
            marks::connect(lw, *a, *b, label, style);
            Ok(())
        }
        R::PatchMarks {
            marks: ids,
            patch,
            imported,
        } => marks::patch(lw, ids, patch, imported.as_ref()),
        R::MoveMarks {
            marks: ids,
            by,
            to_page,
        } => marks::move_marks(lw, ids, *by, *to_page),
        R::DeleteAnnotations { annots } => {
            marks::delete(lw, annots);
            Ok(())
        }
        R::AdoptAnnotations { annots } => marks::adopt(lw, annots),
        R::SetOcrText { page, layer } => {
            content::set_ocr(lw, *page, Some(layer.clone()));
            Ok(())
        }
        R::ClearOcrText { pages } => {
            for &page in pages {
                content::set_ocr(lw, page, None);
            }
            Ok(())
        }
        R::SetHeaderFooter { spec, scope } => {
            content::set_header_footer(lw, spec, scope.clone());
            Ok(())
        }
        R::RemoveHeaderFooter { pages } => {
            content::remove_header_footer(lw, pages);
            Ok(())
        }
    }
}

/// Puts `ids` as page `page`'s annotations: none when it's empty.
pub(crate) fn put_page_annots(rec: &mut Rec, page: PageId, ids: Vector<AnnotId>) {
    let val = (!ids.is_empty()).then_some(Val::PageAnnots(ids));
    rec.put(Key::PageAnnots(page), val);
}

/// `order` with `new` put in at position `at`, by one split and two
/// joins: inserting them one by one edits the tree once a page (189 ms for
/// half of 7,000 pages, against half a millisecond).
pub(crate) fn splice(
    order: &Vector<PageId>,
    at: usize,
    new: impl IntoIterator<Item = PageId>,
) -> Vector<PageId> {
    let mut out = order.clone();
    let tail = out.split_off(at);
    out.extend(new);
    out.append(tail);
    out
}
