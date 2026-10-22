//! One version of the document (M10.5 §3.2): its pages, annotations,
//! form, bookmarks, page labels and generators as records. A state is
//! made by [`DocState::builder`] or from another by applying prims, and
//! read through accessors: no struct literal outside this module can make
//! one whose derived indexes don't fit its tables.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt;
use std::hash::{BuildHasher, BuildHasherDefault, DefaultHasher};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use im::Vector;
use lopdf::ObjectId;
use sheaf_edit::{Rotation, Z};

use super::{
    AnnotBody, AnnotRec, Bookmark, FieldNode, Foreign, FormDoc, GenId, Generator, Mark, OutlineId,
    PageDef, PageId, Scope, Terminal, Widget,
};
use crate::annot::{Annot, AnnotId, AnnotKind};
use crate::forms::{FieldId, full_name_of};
use crate::geom::Geom;
use crate::labels::{LabelRange, LabelSpec};
use crate::prim::{Key, Mismatch, Val};
use crate::store::BlobId;
use crate::text_layer::TextLayer;

/// One immutable snapshot of the document. Only tests compare whole
/// states; history knows what an edit changed from its prims.
#[derive(Debug, Clone)]
pub struct DocState {
    /// The pages in document order: [`Key::Order`].
    order: Vector<PageId>,
    /// What each page shows: [`Key::Page`].
    pages: im::HashMap<PageId, Arc<PageDef>>,
    /// Each page's boxes and `/Rotate`: [`Key::Geom`].
    geoms: im::HashMap<PageId, Arc<Geom>>,
    /// Every bookmark: [`Key::Bookmark`].
    bookmarks: im::HashMap<OutlineId, Arc<Bookmark>>,
    /// The top-level bookmarks, in order: [`Key::OutlineRoots`].
    outline_roots: Vector<OutlineId>,
    /// Each page's annotations, in `/Annots` order: [`Key::PageAnnots`].
    /// A page with none has no entry.
    page_annots: im::HashMap<PageId, Vector<AnnotId>>,
    /// Every annotation, sheaf's marks, other tools' comments and links,
    /// and the form's widgets: [`Key::Annot`].
    annots: im::HashMap<AnnotId, AnnotRec>,
    /// Page labels, anchored to the pages ranges start at: [`Key::Labels`].
    /// None: the file has none.
    labels: Option<Arc<[LabelSpec]>>,
    /// What sheaf draws on pages from one definition each:
    /// [`Key::Generator`].
    generators: im::OrdMap<GenId, Arc<Generator>>,
    /// The generators the file the document opened had, by id, whatever
    /// became of them since: whose blocks in its pages are its own
    /// ([`DocState::backs`]). Never changed by an edit.
    opened_generators: Arc<Opened>,
    /// Every node of the form's field tree: [`Key::Field`].
    fields: im::OrdMap<FieldId, Arc<FieldNode>>,
    /// The form as a whole: [`Key::Form`].
    form: Arc<FormDoc>,
    idx: Indexes,
}

/// The generators the file a document opened had, by id, and that file
/// ([`DocState::backs`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Opened {
    file: Option<BlobId>,
    ids: BTreeSet<GenId>,
}

/// What a state derives from its tables to find things without a scan:
/// what [`DocState::check_invariants`] would build again from them. Never
/// compared, since one state may have found its positions and an equal one
/// not yet.
#[derive(Clone, Default)]
struct Indexes {
    /// Each page's position in `order`, built the first time one is
    /// asked: the clones of one `order` share it, and a new `order`
    /// starts another.
    pos: Arc<OnceLock<HashMap<PageId, u32>>>,
    /// Each page's label, when the document has labels, built the first
    /// time one is asked: a new order, or new labels, start another.
    labels: Arc<OnceLock<HashMap<PageId, Arc<str>>>>,
    /// The page showing each copy of each source page, by the source, the
    /// page's object and the copy, built the first time one is asked. Of
    /// two that do (a page the tree lists twice), the first in order: a new
    /// order, or a page put showing another copy, starts another. Ordered,
    /// so a page's copies are found together ([`DocState::page_named`]).
    shows: Arc<OnceLock<BTreeMap<Shown, PageId>>>,
    /// Every named annotation by its page and a hash of its `/NM`, kept
    /// current as annotations are set. `/NM` need only be unique on its
    /// page, and files don't always keep that, so a name holds every
    /// annotation it names.
    nm: im::OrdSet<(PageId, u64, AnnotId)>,
    /// Every field node by a hash of its full name, kept current as nodes
    /// are set: a node set anew finds the names of the nodes under it
    /// again. Full names needn't be unique in a file, so each name holds
    /// every node it names.
    names: im::OrdSet<(u64, FieldId)>,
    /// The hash each node is under in `names`.
    named: im::HashMap<FieldId, u64>,
    /// Headers' and footers' layouts' digests, one for every version
    /// derived from this one, which lay a page out alike from the same
    /// ([`Laid`]).
    laid: Arc<Laid>,
}

/// Layouts' digests, each by the generator, its recipe on the page and
/// what its layout reads of the page, which is all a header's layout
/// reads (`emit::content::header_digest`), so whatever version asks;
/// let go whole past [`LAID_CAP`] of them.
#[derive(Default)]
pub(crate) struct Laid(Mutex<HashMap<LaidKey, u128>>);

/// What a layout is laid out from: the generator, its recipe on the page
/// (`realize::recipe_digest`) and what it reads of the page
/// (`realize::layout_reads`).
pub(crate) type LaidKey = (GenId, u128, u128);

/// The layouts [`Laid`] keeps at most: a "Page n of N" header on 7,000
/// pages before and after a move fills 14,000.
const LAID_CAP: usize = 1 << 15;

impl Laid {
    /// The digest kept for `key`.
    pub(crate) fn get(&self, key: &LaidKey) -> Option<u128> {
        let kept = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        kept.get(key).copied()
    }

    /// `digest`, kept for `key`.
    pub(crate) fn put(&self, key: LaidKey, digest: u128) {
        let mut kept = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if kept.len() >= LAID_CAP {
            kept.clear();
        }
        kept.insert(key, digest);
    }
}

/// A copy of a source page, as [`DocState::page_showing`] finds the page
/// showing it: the source, the page's object and the copy.
type Shown = (BlobId, ObjectId, u32);

/// The hash the indexes key a name by: the same in every state.
fn name_hash(name: &str) -> u64 {
    BuildHasherDefault::<DefaultHasher>::default().hash_one(name)
}

/// Where the `/NM` index holds `rec`, annotation `id`: nowhere for a
/// widget, which has no name.
fn nm_key(id: AnnotId, rec: &AnnotRec) -> Option<(PageId, u64, AnnotId)> {
    let name = rec.name();
    (!name.is_empty()).then(|| (rec.page, name_hash(&name), id))
}

impl Indexes {
    /// Annotation `id`, which was `was` and is `now`, by its name now.
    fn name(&mut self, id: AnnotId, was: Option<&AnnotRec>, now: Option<&AnnotRec>) {
        let was = was.and_then(|rec| nm_key(id, rec));
        let now = now.and_then(|rec| nm_key(id, rec));
        if was != now {
            if let Some(key) = was {
                self.nm.remove(&key);
            }
            if let Some(key) = now {
                self.nm.insert(key);
            }
        }
    }

    /// Field `id` by full name `name`, in place of the one it had.
    fn name_field(&mut self, id: FieldId, name: &str) {
        let hash = name_hash(name);
        if let Some(was) = self.named.insert(id, hash) {
            if was == hash {
                return;
            }
            self.names.remove(&(was, id));
        }
        self.names.insert((hash, id));
    }

    /// Field `id` by no name.
    fn unname_field(&mut self, id: FieldId) {
        if let Some(was) = self.named.remove(&id) {
            self.names.remove(&(was, id));
        }
    }
}

impl fmt::Debug for Indexes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Indexes").finish_non_exhaustive()
    }
}

#[cfg(any(test, feature = "fixtures"))]
impl PartialEq for DocState {
    fn eq(&self, other: &DocState) -> bool {
        let DocState {
            order,
            pages,
            geoms,
            bookmarks,
            outline_roots,
            page_annots,
            annots,
            labels,
            generators,
            opened_generators,
            fields,
            form,
            idx: _,
        } = self;
        *order == other.order
            && *pages == other.pages
            && *geoms == other.geoms
            && *bookmarks == other.bookmarks
            && *outline_roots == other.outline_roots
            && *page_annots == other.page_annots
            && *annots == other.annots
            && *labels == other.labels
            && *generators == other.generators
            && *opened_generators == other.opened_generators
            && *fields == other.fields
            && *form == other.form
    }
}

/// The tables of a state that what a page draws is made of, by reference
/// ([`DocState::render_tables`]).
pub(crate) struct RenderTables<'a> {
    pub(crate) pages: &'a im::HashMap<PageId, Arc<PageDef>>,
    pub(crate) geoms: &'a im::HashMap<PageId, Arc<Geom>>,
    pub(crate) page_annots: &'a im::HashMap<PageId, Vector<AnnotId>>,
    pub(crate) annots: &'a im::HashMap<AnnotId, AnnotRec>,
    pub(crate) generators: &'a im::OrdMap<GenId, Arc<Generator>>,
    pub(crate) fields: &'a im::OrdMap<FieldId, Arc<FieldNode>>,
    pub(crate) form: &'a Arc<FormDoc>,
}

/// Makes a [`DocState`]: its pages in order, with what each shows and its
/// boxes, and its other tables, empty unless given.
pub struct Builder {
    st: DocState,
}

impl Builder {
    /// `pages` after those it has, in order.
    pub fn pages(
        mut self,
        pages: impl IntoIterator<Item = (PageId, Arc<PageDef>, Arc<Geom>)>,
    ) -> Builder {
        for (id, def, geom) in pages {
            self.st.order.push_back(id);
            self.st.pages.insert(id, def);
            self.st.geoms.insert(id, geom);
        }
        self
    }

    /// `roots` the top-level bookmarks, `bookmarks` every one.
    pub fn outline(
        mut self,
        roots: Vector<OutlineId>,
        bookmarks: im::HashMap<OutlineId, Arc<Bookmark>>,
    ) -> Builder {
        self.st.outline_roots = roots;
        self.st.bookmarks = bookmarks;
        self
    }

    /// `annots`, each page's in `/Annots` order, after those it has.
    pub fn annots(
        mut self,
        annots: impl IntoIterator<Item = (PageId, Vec<(AnnotId, AnnotBody)>)>,
    ) -> Builder {
        for (page, list) in annots {
            if list.is_empty() {
                continue;
            }
            let ids = self.st.page_annots.entry(page).or_default();
            for (id, body) in list {
                ids.push_back(id);
                let rec = AnnotRec { page, body };
                self.st.idx.name(id, None, Some(&rec));
                self.st.annots.insert(id, rec);
            }
        }
        self
    }

    pub fn labels(mut self, labels: Option<Arc<[LabelSpec]>>) -> Builder {
        self.st.labels = labels;
        self
    }

    pub fn generators(mut self, generators: impl IntoIterator<Item = Arc<Generator>>) -> Builder {
        self.st.generators = generators.into_iter().map(|g| (g.id, g)).collect();
        self
    }

    /// `ids`, the generators `file`, the file the document opened, has
    /// ([`DocState::backs`]).
    pub fn opened_generators(
        mut self,
        file: BlobId,
        ids: impl IntoIterator<Item = GenId>,
    ) -> Builder {
        self.st.opened_generators = Arc::new(Opened {
            file: Some(file),
            ids: ids.into_iter().collect(),
        });
        self
    }

    /// The file the document `st` is a state of opened, and the generators
    /// it had, as `st` has them: what a state made of `st`'s pages backs as
    /// `st` does.
    pub fn opened_as(mut self, st: &DocState) -> Builder {
        self.st.opened_generators = st.opened_generators.clone();
        self
    }

    /// `fields`, every node of the form's field tree, and `form`, the
    /// form they make, in place of those it has.
    pub fn form(
        mut self,
        fields: impl IntoIterator<Item = (FieldId, Arc<FieldNode>)>,
        form: Arc<FormDoc>,
    ) -> Builder {
        self.st.fields = fields.into_iter().collect();
        self.st.form = form;
        self.st.idx.names = im::OrdSet::new();
        self.st.idx.named = im::HashMap::new();
        let tops: Vec<FieldId> = (self.st.fields.iter())
            .filter(|(_, node)| node.parent.is_none())
            .map(|(&id, _)| id)
            .collect();
        for id in tops {
            self.st.name_fields(id);
        }
        self
    }

    pub fn build(self) -> DocState {
        self.st
    }
}

impl DocState {
    /// A state with no pages and nothing else, to build on.
    pub fn builder() -> Builder {
        Builder {
            st: DocState {
                order: Vector::new(),
                pages: im::HashMap::new(),
                geoms: im::HashMap::new(),
                bookmarks: im::HashMap::new(),
                outline_roots: Vector::new(),
                page_annots: im::HashMap::new(),
                annots: im::HashMap::new(),
                labels: None,
                generators: im::OrdMap::new(),
                opened_generators: Arc::default(),
                fields: im::OrdMap::new(),
                form: Arc::default(),
                idx: Indexes::default(),
            },
        }
    }

    /// What `key` holds.
    pub(crate) fn get(&self, key: &Key) -> Option<Val> {
        match *key {
            Key::Order => Some(Val::Order(self.order.clone())),
            Key::Page(page) => self.pages.get(&page).cloned().map(Val::Page),
            Key::Geom(page) => self.geoms.get(&page).cloned().map(Val::Geom),
            Key::Bookmark(id) => self.bookmarks.get(&id).cloned().map(Val::Bookmark),
            Key::OutlineRoots => Some(Val::OutlineRoots(self.outline_roots.clone())),
            Key::PageAnnots(page) => self.page_annots.get(&page).cloned().map(Val::PageAnnots),
            Key::Annot(id) => self.annots.get(&id).cloned().map(Val::Annot),
            Key::Labels => self.labels.clone().map(Val::Labels),
            Key::Generator(id) => self.generators.get(&id).cloned().map(Val::Generator),
            Key::Field(id) => self.fields.get(&id).cloned().map(Val::Field),
            Key::Form => Some(Val::Form(self.form.clone())),
        }
    }

    /// Puts `val` in `key`'s slot, keeping the indexes current. A value
    /// `key` can't hold leaves the state as it was.
    pub(crate) fn set(&mut self, key: &Key, val: Option<Val>) -> Result<(), Mismatch> {
        let misfit = Err(Mismatch { key: *key });
        match *key {
            Key::Order => match val {
                Some(Val::Order(v)) => {
                    self.order = v;
                    self.idx.pos = Arc::default();
                    self.idx.labels = Arc::default();
                    self.idx.shows = Arc::default();
                }
                _ => return misfit,
            },
            Key::Page(page) => {
                let old = match val {
                    Some(Val::Page(v)) => self.pages.insert(page, v),
                    None => self.pages.remove(&page),
                    _ => return misfit,
                };
                // What it shows is all the page map knows of it.
                let shows = |def: &PageDef| (def.base.src.id(), def.base.index, def.copy);
                if old.as_deref().map(shows) != self.pages.get(&page).map(|d| shows(d)) {
                    self.idx.shows = Arc::default();
                }
            }
            Key::Geom(page) => match val {
                Some(Val::Geom(v)) => _ = self.geoms.insert(page, v),
                None => _ = self.geoms.remove(&page),
                _ => return misfit,
            },
            Key::Bookmark(id) => match val {
                Some(Val::Bookmark(v)) => _ = self.bookmarks.insert(id, v),
                None => _ = self.bookmarks.remove(&id),
                _ => return misfit,
            },
            Key::OutlineRoots => match val {
                Some(Val::OutlineRoots(v)) => self.outline_roots = v,
                _ => return misfit,
            },
            Key::PageAnnots(page) => match val {
                Some(Val::PageAnnots(v)) => _ = self.page_annots.insert(page, v),
                None => _ = self.page_annots.remove(&page),
                _ => return misfit,
            },
            Key::Annot(id) => {
                let new = match val {
                    Some(Val::Annot(v)) => Some(v),
                    None => None,
                    _ => return misfit,
                };
                let old = match new {
                    Some(rec) => self.annots.insert(id, rec),
                    None => self.annots.remove(&id),
                };
                self.idx.name(id, old.as_ref(), self.annots.get(&id));
            }
            Key::Labels => {
                match val {
                    Some(Val::Labels(v)) => self.labels = Some(v),
                    None => self.labels = None,
                    _ => return misfit,
                }
                self.idx.labels = Arc::default();
            }
            Key::Generator(id) => match val {
                Some(Val::Generator(v)) => _ = self.generators.insert(id, v),
                None => _ = self.generators.remove(&id),
                _ => return misfit,
            },
            Key::Field(id) => {
                let new = match val {
                    Some(Val::Field(v)) => Some(v),
                    None => None,
                    _ => return misfit,
                };
                let old = match &new {
                    Some(node) => self.fields.insert(id, node.clone()),
                    None => self.fields.remove(&id),
                };
                // Its name, and those of the nodes under it, change only
                // with where it is and what it's called.
                let renamed = match (&old, &new) {
                    (Some(a), Some(b)) => {
                        !Arc::ptr_eq(a, b)
                            && (a.partial != b.partial || a.parent != b.parent || a.kids != b.kids)
                    }
                    _ => true,
                };
                if renamed {
                    match new {
                        Some(_) => self.name_fields(id),
                        None => {
                            self.idx.unname_field(id);
                            // Its kids, if they stay, are at the top now.
                            for kid in old.iter().flat_map(|node| node.kids.iter()) {
                                self.name_fields(*kid);
                            }
                        }
                    }
                }
            }
            Key::Form => match val {
                Some(Val::Form(v)) => self.form = v,
                _ => return misfit,
            },
        }
        Ok(())
    }

    /// Names node `id`, and every one under it, by their full names now.
    fn name_fields(&mut self, id: FieldId) {
        let Some(top) = self.full_name(id) else {
            return;
        };
        let mut next = vec![(id, top)];
        // A node is named once, should kids ever make a loop.
        let mut left = self.fields.len();
        while let Some((id, name)) = next.pop() {
            let Some(node) = self.fields.get(&id) else {
                continue;
            };
            for &kid in &node.kids {
                if let Some(k) = self.fields.get(&kid) {
                    next.push((kid, full_name_of(&name, &k.partial)));
                }
            }
            self.idx.name_field(id, &name);
            left -= 1;
            if left == 0 {
                break;
            }
        }
    }

    pub fn page_count(&self) -> usize {
        self.order.len()
    }

    /// The pages in document order.
    pub fn order(&self) -> &Vector<PageId> {
        &self.order
    }

    /// The page at position `pos`.
    pub fn page_at(&self, pos: usize) -> Option<PageId> {
        self.order.get(pos).copied()
    }

    /// Page `id`'s position: O(1) once a state of this order has found
    /// one.
    pub fn pos_of(&self, id: PageId) -> Option<usize> {
        let pos = self.idx.pos.get_or_init(|| {
            (self.order.iter().enumerate())
                .map(|(i, &id)| (id, i as u32))
                .collect()
        });
        pos.get(&id).map(|&i| i as usize)
    }

    /// The layouts' digests this version and those derived from it share
    /// ([`Laid`]).
    pub(crate) fn laid(&self) -> &Laid {
        &self.idx.laid
    }

    /// Page `id`'s label: its number when the document has no labels.
    /// O(1) once a state of this order and these labels has found one.
    pub fn label_of(&self, id: PageId) -> Option<Arc<str>> {
        if self.labels.is_none() {
            return Some((self.pos_of(id)? + 1).to_string().into());
        }
        let labels = self.idx.labels.get_or_init(|| self.label_table());
        labels.get(&id).cloned()
    }

    /// Every page's label, by page.
    fn label_table(&self) -> HashMap<PageId, Arc<str>> {
        let labels = self.page_labels().unwrap_or_default();
        (self.order.iter().enumerate())
            .map(|(i, &id)| {
                let label = labels.get(i).cloned();
                (id, label.unwrap_or_else(|| (i + 1).to_string()).into())
            })
            .collect()
    }

    /// The page showing copy `copy` of the page whose object is `object` in
    /// source `blob`: of two that do, the first in order. O(1) once a state
    /// of this order and these pages has found one.
    pub fn page_showing(&self, blob: BlobId, object: ObjectId, copy: u32) -> Option<PageId> {
        self.shown_pages().get(&(blob, object, copy)).copied()
    }

    /// The page a reference to the page whose object is `object` in source
    /// `blob`, read as copy `copy` of it, names: the page showing that copy,
    /// else the lowest copy any page shows; None when none does. What a
    /// copied reference to a page is written as
    /// ([`page_named`](crate::materialize::copy::page_named)), found as
    /// [`page_showing`](Self::page_showing) finds a page.
    pub(crate) fn page_named(&self, blob: BlobId, object: ObjectId, copy: u32) -> Option<PageId> {
        crate::materialize::copy::page_named(self.shown_pages(), blob, object, copy)
    }

    /// The pages showing copies of the page whose object is `object` in
    /// source `blob`, by copy: what a group of the realizer finds its
    /// pages by (M10.5 §10.1). O(log n) and those, once the page map is
    /// built.
    pub(crate) fn pages_showing(
        &self,
        blob: BlobId,
        object: ObjectId,
    ) -> impl Iterator<Item = PageId> + '_ {
        let copies = (blob, object, 0)..=(blob, object, u32::MAX);
        self.shown_pages().range(copies).map(|(_, &page)| page)
    }

    fn shown_pages(&self) -> &BTreeMap<Shown, PageId> {
        self.idx.shows.get_or_init(|| self.map_shown())
    }

    /// The page showing each copy of each source page.
    fn map_shown(&self) -> BTreeMap<Shown, PageId> {
        let mut shows = BTreeMap::new();
        for (id, def, _) in self.pages() {
            if let Some(facts) = def.base.src.page(def.base.index) {
                let shown = (def.base.src.id(), facts.id, def.copy);
                shows.entry(shown).or_insert(id);
            }
        }
        shows
    }

    /// The tables what a page draws is made of (M10.5 §10.2), out of every
    /// one, so a table added fails to compile here until rendering says
    /// whether it draws (§2's registry rule): the pages, their boxes and
    /// annotations, the form, and the generators. Where a page is and how
    /// many there are come from the positions found
    /// ([`pos_of`](Self::pos_of)); bookmarks and labels draw nothing.
    pub(crate) fn render_tables(&self) -> RenderTables<'_> {
        let DocState {
            order: _,
            pages,
            geoms,
            bookmarks: _,
            outline_roots: _,
            page_annots,
            annots,
            labels: _,
            generators,
            // Read through `backs`.
            opened_generators: _,
            fields,
            form,
            idx: _,
        } = self;
        RenderTables {
            pages,
            geoms,
            page_annots,
            annots,
            generators,
            fields,
            form,
        }
    }

    /// Whether `other` draws as this state does by holding its very tables,
    /// clones of them: the same order and the same
    /// [`render_tables`](Self::render_tables), each one table in memory.
    /// O(1), and false for equal tables made apart: what the realizer
    /// knows a state by.
    pub(crate) fn draws_alike(&self, other: &DocState) -> bool {
        let RenderTables {
            pages,
            geoms,
            page_annots,
            annots,
            generators,
            fields,
            form,
        } = self.render_tables();
        let theirs = other.render_tables();
        Arc::ptr_eq(&self.idx.pos, &other.idx.pos)
            && pages.ptr_eq(theirs.pages)
            && geoms.ptr_eq(theirs.geoms)
            && page_annots.ptr_eq(theirs.page_annots)
            && annots.ptr_eq(theirs.annots)
            && generators.ptr_eq(theirs.generators)
            && fields.ptr_eq(theirs.fields)
            && Arc::ptr_eq(form, theirs.form)
    }

    /// What page `id` shows.
    pub fn def(&self, id: PageId) -> Option<&Arc<PageDef>> {
        #[cfg(test)]
        visits::page(id);
        self.pages.get(&id)
    }

    /// Page `id`'s boxes and `/Rotate`.
    pub fn geom(&self, id: PageId) -> Option<&Arc<Geom>> {
        self.geoms.get(&id)
    }

    /// Every page in order, with what it shows and its boxes.
    pub fn pages(&self) -> impl ExactSizeIterator<Item = (PageId, &Arc<PageDef>, &Arc<Geom>)> {
        self.order.iter().map(|&id| {
            #[cfg(test)]
            visits::page(id);
            (id, &self.pages[&id], &self.geoms[&id])
        })
    }

    /// Page `id`'s recognized text: the layer its stack draws, saved or
    /// not. Text a file held when it opened is the page's own content.
    pub fn ocr_text(&self, id: PageId) -> Option<&Arc<TextLayer>> {
        self.def(id)?.content.ocr_text()
    }

    /// The turn PDFium adds to page `id`'s source page's own `/Rotate` to
    /// show it as it is: what a render `Job` takes.
    pub fn render_delta(&self, id: PageId) -> Option<Rotation> {
        let base = &self.def(id)?.base;
        Some(
            self.geom(id)?
                .rotate
                .minus(base.src.geom(base.index).rotate),
        )
    }

    /// The page labels, each range by the page it starts at.
    pub fn labels(&self) -> Option<&Arc<[LabelSpec]>> {
        self.labels.as_ref()
    }

    /// Every page's label, when the document has page labels.
    pub fn page_labels(&self) -> Option<Vec<String>> {
        let ranges = self.label_ranges()?;
        if ranges.is_empty() {
            return None;
        }
        Some(crate::labels::labels_for(&ranges, self.order.len() as u32))
    }

    /// The label ranges by page position, when there are labels.
    pub fn label_ranges(&self) -> Option<Vec<LabelRange>> {
        let specs = self.labels.as_ref()?;
        let ids: Vec<PageId> = self.order.iter().copied().collect();
        Some(crate::labels::resolve_specs(specs, &ids))
    }

    /// Every generator, by id.
    pub fn generators(&self) -> &im::OrdMap<GenId, Arc<Generator>> {
        &self.generators
    }

    /// The generator of the header and footer.
    pub fn header_footer(&self) -> Option<&Arc<Generator>> {
        (self.generators.values()).find(|g| g.spec.header_footer().is_some())
    }

    /// The generator of the watermark (`z` over) or the background
    /// (under).
    pub fn watermark(&self, z: Z) -> Option<&Arc<Generator>> {
        (self.generators.values()).find(|g| g.spec.watermark().is_some() && g.z == z)
    }

    /// Whether a block of generator `id` in page `page`'s own content is
    /// the document's (M10.5 §3.5): `id` is a generator of its that covers
    /// the page, and draws it anew; or the page is one of the file it
    /// opened, and `id` one of its generators, or one that file had, which
    /// an edit since removed. Such a block is what a save lays out again or
    /// leaves out, never the page's content; any other, another file's, is,
    /// though it names a generator of the document's, as every revision of
    /// a file does: a page brought in from another revision keeps it.
    pub fn backs(&self, id: GenId, page: PageId) -> bool {
        if self.generators.get(&id).is_some_and(|g| g.covers(page)) {
            return true;
        }
        let opened = &self.opened_generators;
        let file = |def: &Arc<PageDef>| Some(def.base.src.id()) == opened.file;
        (self.generators.contains_key(&id) || opened.ids.contains(&id))
            && self.pages.get(&page).is_some_and(file)
    }

    /// Annotation `id`.
    pub fn annot(&self, id: AnnotId) -> Option<&AnnotRec> {
        #[cfg(test)]
        visits::annot(id);
        self.annots.get(&id)
    }

    /// Whether some annotation has id `id`.
    pub fn has_annot(&self, id: AnnotId) -> bool {
        self.annots.contains_key(&id)
    }

    /// Page `page`'s annotations, in `/Annots` order.
    pub fn annots_on(&self, page: PageId) -> impl Iterator<Item = (AnnotId, &AnnotRec)> {
        let ids = self.annot_ids_on(page).into_iter().flatten();
        ids.filter_map(|&id| Some((id, self.annot(id)?)))
    }

    /// The ids of page `page`'s annotations, in `/Annots` order; None when
    /// it has none.
    pub fn annot_ids_on(&self, page: PageId) -> Option<&Vector<AnnotId>> {
        #[cfg(test)]
        visits::page(page);
        self.page_annots.get(&page)
    }

    /// sheaf's marks on page `page`, in `/Annots` order.
    pub fn marks_on(&self, page: PageId) -> impl Iterator<Item = &Annot> {
        self.annots_on(page).filter_map(|(_, rec)| rec.mark())
    }

    /// sheaf's marks on page `page`, with what they hold, in `/Annots`
    /// order.
    pub fn mark_records_on(&self, page: PageId) -> impl Iterator<Item = &Mark> {
        self.annots_on(page)
            .filter_map(|(_, rec)| Some(&**rec.mark_record()?))
    }

    /// Other tools' comments and links on page `page`, in `/Annots`
    /// order.
    pub fn foreign_on(&self, page: PageId) -> impl Iterator<Item = (AnnotId, &Arc<Foreign>)> {
        self.annots_on(page)
            .filter_map(|(id, rec)| Some((id, rec.foreign()?)))
    }

    /// Every annotation, in no order.
    pub fn annots(&self) -> impl Iterator<Item = (AnnotId, &AnnotRec)> {
        self.annots.iter().map(|(&id, rec)| {
            #[cfg(test)]
            visits::annot(id);
            (id, rec)
        })
    }

    /// Every one of sheaf's marks, with its page, in no order.
    pub fn marks(&self) -> impl Iterator<Item = (PageId, &Annot)> {
        self.annots()
            .filter_map(|(_, rec)| Some((rec.page, rec.mark()?)))
    }

    /// How many annotations there are.
    pub fn annot_count(&self) -> usize {
        self.annots.len()
    }

    /// The annotation on page `page` whose `/NM` is `nm` (see
    /// [`AnnotRec::name`]), found without a scan: of two named alike, as a
    /// file may have them, the first by id.
    pub fn annot_by_nm(&self, page: PageId, nm: &str) -> Option<AnnotId> {
        self.annots_named(page, nm).next()
    }

    /// Every annotation on page `page` whose `/NM` is `nm`, by id, found
    /// without a scan.
    pub fn annots_named<'a>(
        &'a self,
        page: PageId,
        nm: &'a str,
    ) -> impl Iterator<Item = AnnotId> + 'a {
        let hash = name_hash(nm);
        let ids = (self.idx.nm).range((page, hash, AnnotId(0))..=(page, hash, AnnotId(u64::MAX)));
        ids.map(|&(_, _, id)| id)
            .filter(move |id| self.annots.get(id).is_some_and(|rec| rec.name() == nm))
    }

    /// The form's widgets on page `page`, in `/Annots` order.
    pub fn widgets_on(&self, page: PageId) -> impl Iterator<Item = (AnnotId, &Arc<Widget>)> {
        self.annots_on(page)
            .filter_map(|(id, rec)| Some((id, rec.widget()?)))
    }

    /// Field node `id`.
    pub fn field(&self, id: FieldId) -> Option<&Arc<FieldNode>> {
        #[cfg(test)]
        visits::field(id);
        self.fields.get(&id)
    }

    /// Every field node, by id.
    pub fn fields(&self) -> impl Iterator<Item = (FieldId, &Arc<FieldNode>)> {
        self.fields.iter().map(|(&id, node)| {
            #[cfg(test)]
            visits::field(id);
            (id, node)
        })
    }

    /// How many field nodes there are.
    pub fn field_count(&self) -> usize {
        self.fields.len()
    }

    /// Every terminal field, by id.
    pub fn terminals(&self) -> impl Iterator<Item = (FieldId, &FieldNode, &Terminal)> {
        self.fields()
            .filter_map(|(id, node)| Some((id, &**node, &**node.term.as_ref()?)))
    }

    /// The form as a whole.
    pub fn form(&self) -> &Arc<FormDoc> {
        &self.form
    }

    /// Node `id`'s full name: the partials from the root down, joined by
    /// dots.
    pub fn full_name(&self, id: FieldId) -> Option<String> {
        let mut partials = Vec::new();
        let mut at = Some(id);
        // Up at most as many nodes as there are, should parents loop.
        for _ in 0..=self.fields.len() {
            let Some(node) = at.and_then(|id| self.fields.get(&id)) else {
                break;
            };
            partials.push(&*node.partial);
            at = node.parent;
        }
        if partials.is_empty() {
            return None;
        }
        Some((partials.iter().rev()).fold(String::new(), |up, p| full_name_of(&up, p)))
    }

    /// The node whose full name is `name`, found without a scan: of two
    /// named alike, as a file may have them, the first by id.
    pub fn field_named(&self, name: &str) -> Option<FieldId> {
        self.fields_named(name).next()
    }

    /// Every node whose full name is `name`, by id, found without a scan.
    pub(crate) fn fields_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = FieldId> + 'a {
        let hash = name_hash(name);
        let ids = (self.idx.names).range((hash, FieldId(0))..=(hash, FieldId(u64::MAX)));
        ids.map(|&(_, id)| id)
            .filter(move |&id| self.full_name(id).as_deref() == Some(name))
    }

    /// Mark `id`, and its page.
    pub fn find_annot(&self, id: AnnotId) -> Option<(PageId, &Annot)> {
        let rec = self.annots.get(&id)?;
        Some((rec.page, rec.mark()?))
    }

    /// The first connector label (1–50, then A–ZZ) no connector uses yet;
    /// past those, one more than the highest number.
    pub fn next_connector_label(&self) -> String {
        let used: HashSet<&str> = self
            .marks()
            .filter_map(|(_, a)| match &a.kind {
                AnnotKind::Connector { label, .. } => Some(label.as_str()),
                _ => None,
            })
            .collect();
        if let Some(free) = crate::annot::connector_labels().find(|l| !used.contains(l.as_str())) {
            return free;
        }
        let max = used
            .iter()
            .filter_map(|l| l.trim().parse::<u32>().ok())
            .max()
            .unwrap_or(0);
        (max + 1).to_string()
    }

    /// Bookmark `id`.
    pub fn bookmark(&self, id: OutlineId) -> Option<&Arc<Bookmark>> {
        self.bookmarks.get(&id)
    }

    /// The top-level bookmarks, in order.
    pub fn outline_roots(&self) -> &Vector<OutlineId> {
        &self.outline_roots
    }

    /// How many bookmarks there are.
    pub fn bookmark_count(&self) -> usize {
        self.bookmarks.len()
    }

    /// Every bookmark in pre-order (each before its kids, as the panel
    /// lists them), with its depth.
    pub fn bookmarks_preorder(&self) -> Vec<(u16, OutlineId, &Bookmark)> {
        let mut out = Vec::with_capacity(self.bookmarks.len());
        let mut next: Vec<(u16, OutlineId)> =
            self.outline_roots.iter().rev().map(|&id| (0, id)).collect();
        while let Some((depth, id)) = next.pop() {
            let Some(b) = self.bookmarks.get(&id) else {
                continue;
            };
            out.push((depth, id, &**b));
            next.extend(b.kids.iter().rev().map(|&kid| (depth + 1, kid)));
        }
        out
    }

    /// Where bookmark `id` is: its parent (None at the top level) and the
    /// bookmark before it there (None when it's first).
    pub fn bookmark_place(&self, id: OutlineId) -> Option<(Option<OutlineId>, Option<OutlineId>)> {
        let before = |kids: &Vector<OutlineId>| {
            let i = kids.index_of(&id)?;
            Some(i.checked_sub(1).map(|i| kids[i]))
        };
        if let Some(prev) = before(&self.outline_roots) {
            return Some((None, prev));
        }
        self.bookmarks
            .iter()
            .find_map(|(&parent, b)| Some((Some(parent), before(&b.kids)?)))
    }

    /// The pages bookmark `id` owns (as moving it with its pages takes
    /// them), in document order.
    pub fn bookmark_pages(&self, id: OutlineId) -> Option<Vec<PageId>> {
        crate::tx::bookmarks::bookmark_span(self, id)
    }
}

/// A rule of M10.5 §6.4 that a state's records break: whatever made the
/// state has a bug.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Invariant {
    #[error("pages: {0}")]
    Pages(String),
    #[error("annotations: {0}")]
    Annots(String),
    #[error("the form: {0}")]
    Form(String),
    #[error("bookmarks: {0}")]
    Outline(String),
    #[error("page labels: {0}")]
    Labels(String),
    #[error("generators: {0}")]
    Generators(String),
    #[error("indexes: {0}")]
    Index(String),
}

/// Fine when `ok`, else what `broken` says.
fn hold(ok: bool, broken: impl FnOnce() -> Invariant) -> Result<(), Invariant> {
    if ok { Ok(()) } else { Err(broken()) }
}

impl DocState {
    /// Whether its records keep the rules every edit leaves them in
    /// (M10.5 §6.4), as tests and debug builds check after each. The
    /// order, the pages and their boxes name the same pages, each once,
    /// and no two pages are one copy of one source page. Each annotation
    /// is listed once, on its page. The two ends of a connector name each
    /// other. A reply or status that's one of sheaf's marks is to a comment
    /// on its page, or names no mark at all. Each widget and its terminal
    /// field name each other, in one instance of a form with a name. The
    /// bookmarks and the field nodes are trees from their roots, each node
    /// in once, a field under the parent it names, and a bookmark going to
    /// a page there is. Each label range starts at a page there is. There's
    /// one generator at most of each kind and z, each under its id, on
    /// pages there are, and numbering only pages it's on. Every index is
    /// what its table makes again.
    ///
    /// Not: that another tool's reply is to a comment on its page, as files
    /// have them to ones that are gone or elsewhere, nor a mark's that names
    /// another tool's comment, which a file may have deleted and have a
    /// copy of on another page; nor that the `/NM`s on a page or the form's
    /// full names are unique, which files break (the indexes hold each
    /// one).
    pub fn check_invariants(&self) -> Result<(), Invariant> {
        let DocState {
            order,
            pages,
            geoms,
            bookmarks,
            outline_roots,
            page_annots,
            annots,
            labels,
            generators,
            opened_generators: _,
            fields,
            form,
            idx,
        } = self;

        // The pages.
        let mut listed = HashSet::with_capacity(order.len());
        for &id in order {
            hold(listed.insert(id), || {
                Invariant::Pages(format!("{id:?} is in the order twice"))
            })?;
        }
        let ordered = pages.len() == listed.len() && listed.iter().all(|id| pages.contains_key(id));
        hold(ordered, || {
            Invariant::Pages("the pages aren't the order's".into())
        })?;
        let ordered = geoms.len() == listed.len() && listed.iter().all(|id| geoms.contains_key(id));
        hold(ordered, || {
            Invariant::Pages("the pages with boxes aren't the order's".into())
        })?;
        // In the order, not the map's, so a clash names the same page each
        // time: the later one.
        let mut copies = HashSet::with_capacity(order.len());
        for id in order {
            let def = &pages[id];
            let base = &def.base;
            let count = base.src.page_count();
            hold(base.index < count, || {
                Invariant::Pages(format!("{id:?} shows page {} of {count}", base.index + 1))
            })?;
            hold(copies.insert((base.src.id(), base.index, def.copy)), || {
                Invariant::Pages(format!(
                    "{id:?} is copy {} of a page another is too",
                    def.copy
                ))
            })?;
        }

        // The annotations.
        let mut listed = HashSet::with_capacity(annots.len());
        for (&page, ids) in page_annots {
            hold(pages.contains_key(&page), || {
                Invariant::Annots(format!("{page:?} isn't a page, but has some"))
            })?;
            hold(!ids.is_empty(), || {
                Invariant::Annots(format!("{page:?} lists none"))
            })?;
            for &id in ids {
                hold(listed.insert(id), || {
                    Invariant::Annots(format!("{id:?} is listed twice"))
                })?;
                hold(annots.get(&id).is_some_and(|a| a.page == page), || {
                    Invariant::Annots(format!("{page:?} lists {id:?}, which isn't on it"))
                })?;
            }
        }
        hold(listed.len() == annots.len(), || {
            let unlisted = annots.len() - listed.len();
            Invariant::Annots(format!("{unlisted} aren't listed on their pages"))
        })?;
        // Connectors and threads.
        let mut marked: Option<HashSet<String>> = None;
        for (&id, a) in annots {
            let Some(m) = a.mark() else {
                continue;
            };
            if let Some(other) = m.partner() {
                let back = (annots.get(&other).and_then(AnnotRec::mark)).and_then(Annot::partner);
                hold(back == Some(id), || {
                    Invariant::Annots(format!("{id:?}'s partner {other:?} isn't paired with it"))
                })?;
            }
            if let Some(parent) = m.thread_parent()
                && self.annot_by_nm(a.page, parent).is_none()
            {
                let marked = marked.get_or_insert_with(|| {
                    (annots.values().filter_map(AnnotRec::mark))
                        .map(Annot::name)
                        .collect()
                });
                hold(!marked.contains(parent), || {
                    Invariant::Annots(format!(
                        "{id:?} is a reply to “{parent}”, a mark on another page"
                    ))
                })?;
            }
        }

        // The widgets and their fields.
        let mut widgets = 0;
        for (&id, a) in annots {
            let Some(w) = a.widget() else {
                continue;
            };
            widgets += 1;
            let node = fields.get(&w.field);
            let term = node.and_then(|node| node.term.as_ref());
            hold(term.is_some_and(|t| t.widgets.contains(&id)), || {
                Invariant::Form(format!("widget {id:?} isn't one of {:?}'s", w.field))
            })?;
            hold(node.is_some_and(|node| node.inst == w.inst), || {
                Invariant::Form(format!("widget {id:?} isn't in {:?}'s instance", w.field))
            })?;
        }
        let mut shown = 0;
        for (&field, node) in fields {
            for &id in node.term.iter().flat_map(|t| &t.widgets) {
                shown += 1;
                let named =
                    (annots.get(&id).and_then(AnnotRec::widget)).is_some_and(|w| w.field == field);
                hold(named, || {
                    Invariant::Form(format!("{field:?} lists {id:?}, which isn't its widget"))
                })?;
            }
        }
        hold(shown == widgets, || {
            Invariant::Form(format!("the fields list {shown} widgets, of {widgets}"))
        })?;

        // The field tree.
        let mut reached = HashSet::with_capacity(fields.len());
        let mut next: Vec<(Option<FieldId>, FieldId)> =
            form.roots.iter().map(|&id| (None, id)).collect();
        while let Some((up, id)) = next.pop() {
            let node = fields
                .get(&id)
                .ok_or_else(|| Invariant::Form(format!("{id:?}, under {up:?}, isn't a node")))?;
            hold(node.parent == up, || {
                Invariant::Form(format!("{id:?} is under {up:?}, not {:?}", node.parent))
            })?;
            hold(reached.insert(id), || {
                Invariant::Form(format!("{id:?} is in the tree twice"))
            })?;
            hold(form.names.contains_key(&node.inst), || {
                Invariant::Form(format!("{id:?}'s instance {:?} has no name", node.inst))
            })?;
            next.extend(node.kids.iter().map(|&kid| (Some(id), kid)));
        }
        hold(reached.len() == fields.len(), || {
            let out = fields.len() - reached.len();
            Invariant::Form(format!("{out} nodes aren't in the tree"))
        })?;

        // The outline.
        let mut reached = HashSet::with_capacity(bookmarks.len());
        let mut next: Vec<OutlineId> = outline_roots.iter().copied().collect();
        while let Some(id) = next.pop() {
            let b = (bookmarks.get(&id))
                .ok_or_else(|| Invariant::Outline(format!("{id:?} isn't a bookmark")))?;
            hold(reached.insert(id), || {
                Invariant::Outline(format!("{id:?} is in the outline twice"))
            })?;
            if let Some(page) = b.page() {
                hold(pages.contains_key(&page), || {
                    Invariant::Outline(format!("{id:?} goes to {page:?}, which isn't a page"))
                })?;
            }
            next.extend(b.kids.iter().copied());
        }
        hold(reached.len() == bookmarks.len(), || {
            let out = bookmarks.len() - reached.len();
            Invariant::Outline(format!("{out} bookmarks aren't in the outline"))
        })?;

        // The labels.
        for spec in labels.iter().flat_map(|l| l.iter()) {
            hold(pages.contains_key(&spec.start), || {
                Invariant::Labels(format!(
                    "a range starts at {:?}, which isn't a page",
                    spec.start
                ))
            })?;
        }

        // The generators.
        for (&id, g) in generators {
            hold(g.id == id, || {
                Invariant::Generators(format!("{:?} is under {id:?}", g.id))
            })?;
        }
        let headers = (generators.values())
            .filter(|g| g.spec.header_footer().is_some())
            .count();
        hold(headers <= 1, || {
            Invariant::Generators(format!("{headers} draw a header/footer"))
        })?;
        for z in [Z::Under, Z::Over] {
            let drawn = (generators.values())
                .filter(|g| g.spec.watermark().is_some() && g.z == z)
                .count();
            hold(drawn <= 1, || {
                Invariant::Generators(format!("{drawn} draw the {z:?} watermark"))
            })?;
        }
        for g in generators.values() {
            if let Scope::Pages(scope) = &g.scope {
                hold(!scope.is_empty(), || {
                    Invariant::Generators(format!("{:?} is on no page", g.id))
                })?;
                for &page in scope {
                    hold(pages.contains_key(&page), || {
                        Invariant::Generators(format!(
                            "{:?} is on {page:?}, which isn't a page",
                            g.id
                        ))
                    })?;
                }
            }
            for &page in g.frozen.bates.iter().flat_map(|b| b.keys()) {
                hold(pages.contains_key(&page) && g.covers(page), || {
                    Invariant::Generators(format!("{:?} numbers {page:?}, which it isn't on", g.id))
                })?;
            }
        }

        // The indexes.
        if let Some(pos) = idx.pos.get() {
            let found = pos.len() == order.len()
                && (order.iter().zip(0..)).all(|(id, i)| pos.get(id) == Some(&i));
            hold(found, || {
                Invariant::Index("the positions aren't the order's".into())
            })?;
        }
        if let Some(labels) = idx.labels.get() {
            hold(*labels == self.label_table(), || {
                Invariant::Index("the labels aren't the pages'".into())
            })?;
        }
        if let Some(shows) = idx.shows.get() {
            hold(*shows == self.map_shown(), || {
                Invariant::Index("the page map isn't what the pages show".into())
            })?;
        }
        // Each finds every record it should, and no more.
        let mut named = 0;
        for (&id, a) in annots {
            if let Some(key) = nm_key(id, a) {
                named += 1;
                hold(idx.nm.contains(&key), || {
                    Invariant::Index(format!("{id:?} isn't found by its /NM"))
                })?;
            }
        }
        hold(idx.nm.len() == named, || {
            Invariant::Index(format!("the /NM index has {}, of {named}", idx.nm.len()))
        })?;
        for &id in fields.keys() {
            let hash = name_hash(&self.full_name(id).unwrap_or_default());
            let found = idx.named.get(&id) == Some(&hash) && idx.names.contains(&(hash, id));
            hold(found, || {
                Invariant::Index(format!("{id:?} isn't found by its full name"))
            })?;
        }
        let (n, m) = (idx.names.len(), idx.named.len());
        hold(n == fields.len() && m == fields.len(), || {
            Invariant::Index(format!(
                "the full-name index has {n} and {m}, of {}",
                fields.len()
            ))
        })
    }

    /// The first slot at which this state, a transaction's changes put on
    /// a version other than its own (M10.5 §8.5's `Stale::Validate`), at
    /// `keys`, breaks a rule of uniqueness that neither state it came from
    /// (`was`: that version's and the transaction's) broke: the generator
    /// of a written one's kind and z
    /// ([`GenSpec::same_place`](super::GenSpec::same_place)), the
    /// annotation a written one's `/NM` names on its page too, the node a
    /// written one's full name, or one under it, names too, the page that
    /// shows the copy of its source page a written one now does. Files
    /// break the second and third, so a name either state held twice
    /// already isn't one.
    pub(crate) fn unique_after<'a>(
        &self,
        keys: impl IntoIterator<Item = &'a Key>,
        was: [&DocState; 2],
    ) -> Result<(), Key> {
        let copy_of = |def: &PageDef| (def.base.src.id(), def.base.index, def.copy);
        // The copies the written pages show that the version's didn't.
        let mut copies = HashMap::new();
        let mut nodes = Vec::new();
        for &key in keys {
            match key {
                Key::Page(p) => {
                    let Some(def) = self.pages.get(&p) else {
                        continue;
                    };
                    if was[0].def(p).is_none_or(|d| copy_of(d) != copy_of(def)) {
                        copies.insert(copy_of(def), p);
                    }
                }
                Key::Annot(a) => {
                    let Some(rec) = self.annots.get(&a) else {
                        continue;
                    };
                    let name = rec.name();
                    if name.is_empty() {
                        continue;
                    }
                    let twice = |st: &DocState| st.annots_named(rec.page, &name).nth(1).is_some();
                    let other = self.annots_named(rec.page, &name).find(|&id| id != a);
                    if let Some(other) = other
                        && !was.into_iter().any(twice)
                    {
                        return Err(Key::Annot(other));
                    }
                }
                Key::Field(f) => nodes.push(f),
                Key::Generator(g) => {
                    let Some(made) = self.generators.get(&g) else {
                        continue;
                    };
                    let other = (self.generators.values())
                        .find(|other| other.id != g && other.same_place(made));
                    if let Some(other) = other {
                        return Err(Key::Generator(other.id));
                    }
                }
                Key::Order
                | Key::Geom(_)
                | Key::PageAnnots(_)
                | Key::Form
                | Key::Bookmark(_)
                | Key::OutlineRoots
                | Key::Labels => {}
            }
        }
        let mut named = HashSet::new();
        while let Some(id) = nodes.pop() {
            let Some(node) = self.fields.get(&id).filter(|_| named.insert(id)) else {
                continue;
            };
            nodes.extend(node.kids.iter().copied());
            let Some(name) = self.full_name(id) else {
                continue;
            };
            let twice = |st: &DocState| st.fields_named(&name).nth(1).is_some();
            let other = self.fields_named(&name).find(|&other| other != id);
            if let Some(other) = other
                && !was.into_iter().any(twice)
            {
                return Err(Key::Field(other));
            }
        }
        if copies.is_empty() {
            return Ok(());
        }
        for (id, def, _) in self.pages() {
            if let Some(&page) = copies.get(&copy_of(def))
                && page != id
            {
                return Err(Key::Page(id));
            }
        }
        Ok(())
    }

    /// The first page that `keys`, slots a transaction's changes put on a
    /// version other than its own wrote (M10.5 §8.5's `Stale::Validate`),
    /// name in this state, which isn't one of its pages: a page the version
    /// took away, which the transaction didn't see go, as the page of
    /// annotations, an annotation's, a bookmark's target, a label range's
    /// start, or among a generator's pages or those it numbers.
    pub(crate) fn resolves_after<'a>(
        &self,
        keys: impl IntoIterator<Item = &'a Key>,
    ) -> Result<(), Key> {
        for &key in keys {
            let mut named = Vec::new();
            match key {
                Key::PageAnnots(p) if self.page_annots.contains_key(&p) => named.push(p),
                Key::Annot(a) => named.extend(self.annots.get(&a).map(|rec| rec.page)),
                Key::Bookmark(b) => named.extend(self.bookmarks.get(&b).and_then(|b| b.page())),
                Key::Labels => {
                    named.extend(self.labels.iter().flat_map(|l| l.iter().map(|s| s.start)))
                }
                Key::Generator(g) => {
                    if let Some(g) = self.generators.get(&g) {
                        if let Scope::Pages(scope) = &g.scope {
                            named.extend(scope.iter().copied());
                        }
                        named.extend(g.frozen.bates.iter().flat_map(|b| b.keys().copied()));
                    }
                }
                Key::Order
                | Key::Page(_)
                | Key::Geom(_)
                | Key::PageAnnots(_)
                | Key::Field(_)
                | Key::Form
                | Key::OutlineRoots => {}
            }
            if let Some(&gone) = named.iter().find(|p| !self.pages.contains_key(p)) {
                return Err(Key::Page(gone));
            }
        }
        Ok(())
    }

    /// What its snapshot copied from its parent's to put `keys` (M10.5
    /// §3.9): for each slot of a per-page, per-annotation, per-field,
    /// per-bookmark or per-generator map, a path down from the root, and
    /// for an annotation or a field, others in the indexes that find it by
    /// name; for a new order, the positions and the page map it finds, and
    /// for a new order or new labels, the labels it finds when a generator
    /// draws them. A whole value (the top-level bookmarks, the labels, the
    /// form) is its prim's.
    pub(crate) fn snapshot_bytes(&self, keys: impl IntoIterator<Item = Key>) -> usize {
        let DocState {
            order,
            pages,
            geoms,
            bookmarks,
            outline_roots: _,
            page_annots,
            annots,
            labels: _,
            generators,
            // One for every version, which none changes.
            opened_generators: _,
            fields,
            form: _,
            idx,
        } = self;
        let (mut new_order, mut new_labels) = (false, false);
        let [mut p, mut g, mut b, mut pa, mut a, mut f, mut gens] = [0; 7];
        for key in keys {
            match key {
                Key::Order => new_order = true,
                Key::Labels => new_labels = true,
                Key::Page(_) => p += 1,
                Key::Geom(_) => g += 1,
                Key::Bookmark(_) => b += 1,
                Key::PageAnnots(_) => pa += 1,
                Key::Annot(_) => a += 1,
                Key::Field(_) => f += 1,
                Key::Generator(_) => gens += 1,
                Key::OutlineRoots | Key::Form => {}
            }
        }
        // A hash table of positions, about an eighth of it empty, and the
        // page map, a B-tree whose nodes are about two-thirds full.
        let table = |len: usize, entry: usize| len * (entry + 1) * 8 / 7;
        let tree = |len: usize, entry: usize| len * entry * 3 / 2;
        let positions = match new_order {
            true => {
                table(order.len(), size_of::<(PageId, u32)>())
                    + tree(order.len(), size_of::<(Shown, PageId)>())
            }
            false => 0,
        };
        // Labels are found for a generator that draws them, a few bytes of
        // text each.
        let drawn = || (generators.values()).any(|g| g.needs().label);
        let labelled = match self.labels.is_some() && (new_order || new_labels) && drawn() {
            true => table(order.len(), size_of::<(PageId, Arc<str>)>() + 8),
            false => 0,
        };
        size_of::<DocState>()
            + positions
            + labelled
            + path_copies::<(PageId, Arc<PageDef>)>(p, pages.len(), HAMT)
            + path_copies::<(PageId, Arc<Geom>)>(g, geoms.len(), HAMT)
            + path_copies::<(OutlineId, Arc<Bookmark>)>(b, bookmarks.len(), HAMT)
            + path_copies::<(PageId, Vector<AnnotId>)>(pa, page_annots.len(), HAMT)
            + path_copies::<(AnnotId, AnnotRec)>(a, annots.len(), HAMT)
            + path_copies::<(PageId, u64, AnnotId)>(a, idx.nm.len(), BTREE)
            + path_copies::<(FieldId, Arc<FieldNode>)>(f, fields.len(), BTREE)
            + path_copies::<(u64, FieldId)>(f, idx.names.len(), BTREE)
            + path_copies::<(FieldId, u64)>(f, idx.named.len(), HAMT)
            + path_copies::<(GenId, Arc<Generator>)>(gens, generators.len(), BTREE)
    }
}

/// im's HashMap is a tree of 32 entries a node; its OrdMap and OrdSet, of
/// 64.
const HAMT: usize = 32;
const BTREE: usize = 64;

/// The bytes of the im nodes copied to change `touched` of a map's `len`
/// entries, at `fanout` entries `E` a node: a path down from the root to
/// each, sharing the nodes near the root.
fn path_copies<E>(touched: usize, len: usize, fanout: usize) -> usize {
    if touched == 0 {
        return 0;
    }
    let (mut nodes, mut level, mut reach) = (0, 1, fanout);
    loop {
        nodes += touched.min(level);
        if reach >= len {
            return nodes * fanout * size_of::<E>();
        }
        level *= fanout;
        reach *= fanout;
    }
}

/// For tests: what reads of states on a thread visit while [`watching`]
/// watches them, by id: each page a walk of the pages comes to, or whose
/// definition or annotations are looked up; each annotation and each
/// field node. What a state's own tables read as they're built counts too,
/// so a test builds them first.
///
/// [`watching`]: visits::watching
#[cfg(test)]
pub(crate) mod visits {
    use std::cell::RefCell;
    use std::collections::BTreeSet;

    use crate::annot::AnnotId;
    use crate::forms::FieldId;
    use crate::model::PageId;

    #[derive(Debug, Default)]
    pub(crate) struct Visits {
        pub(crate) pages: BTreeSet<PageId>,
        pub(crate) annots: BTreeSet<AnnotId>,
        pub(crate) fields: BTreeSet<FieldId>,
    }

    thread_local! {
        static WATCHED: RefCell<Option<Visits>> = const { RefCell::new(None) };
    }

    /// `run`, and what it visited.
    pub(crate) fn watching<R>(run: impl FnOnce() -> R) -> (R, Visits) {
        struct Stop;
        impl Drop for Stop {
            fn drop(&mut self) {
                WATCHED.take();
            }
        }
        WATCHED.set(Some(Visits::default()));
        let stop = Stop;
        let out = run();
        let seen = WATCHED.take().unwrap_or_default();
        drop(stop);
        (out, seen)
    }

    fn seen(note: impl FnOnce(&mut Visits)) {
        WATCHED.with_borrow_mut(|watched| watched.as_mut().map(note));
    }

    pub(super) fn page(id: PageId) {
        seen(|v| _ = v.pages.insert(id));
    }

    pub(super) fn annot(id: AnnotId) {
        seen(|v| _ = v.annots.insert(id));
    }

    pub(super) fn field(id: FieldId) {
        seen(|v| _ = v.fields.insert(id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::FixedCount;
    use crate::model::tests::{opened_doc, square};
    use crate::open::Facts;

    /// Each index that doesn't find what its table holds, or finds more,
    /// is found as an index's rule (M10.5 §6.4): a page's position, an
    /// annotation's `/NM` and a node's full name.
    #[test]
    fn the_invariants_find_each_index_broken() {
        let facts = Facts {
            form: crate::forms::fixture::every_kind().source().form().clone(),
            ..Default::default()
        };
        let mut d = opened_doc(2, facts, Box::new(FixedCount(10)));
        let p0 = d.state().order()[0];
        let add = crate::tx::fixture::add(p0, &square(0));
        let header = crate::tx::fixture::set_header(numbered(), Some(&[p0]));
        d.commit_ops(vec![add, header], "add").unwrap();
        let st = d.state().clone();
        assert_eq!(st.pos_of(p0), Some(0));
        let header = st.header_footer().unwrap();
        assert_eq!(header.frozen.bates_number(p0), Some(100));
        let def = st.def(p0).unwrap();
        let object = def.base.src.page(def.base.index).unwrap().id;
        assert_eq!(st.page_showing(def.base.src.id(), object, 0), Some(p0));
        assert_eq!(st.check_invariants(), Ok(()));
        let (mark, rec) = st.annots_on(p0).find(|(_, a)| a.mark().is_some()).unwrap();
        let named = nm_key(mark, rec).unwrap();
        let field = st.terminals().next().unwrap().0;
        let hash = name_hash(&st.full_name(field).unwrap());
        type Breaks<'a> = &'a dyn Fn(&mut Indexes);
        let cases: [(Breaks, &str); 8] = [
            (
                &|idx| idx.pos = Arc::new(OnceLock::from(HashMap::from([(p0, 1)]))),
                "the positions aren't the order's",
            ),
            (
                &|idx| idx.labels = Arc::new(OnceLock::from(HashMap::from([(p0, "7".into())]))),
                "the labels aren't the pages'",
            ),
            (
                &|idx| idx.shows = Arc::new(OnceLock::from(BTreeMap::new())),
                "the page map isn't what the pages show",
            ),
            (
                &|idx| {
                    idx.nm.remove(&named);
                },
                "isn't found by its /NM",
            ),
            (
                &|idx| {
                    idx.nm.insert((p0, 0, AnnotId(7)));
                },
                "the /NM index has",
            ),
            (
                &|idx| {
                    idx.names.remove(&(hash, field));
                },
                "isn't found by its full name",
            ),
            (
                &|idx| {
                    idx.named.insert(field, hash ^ 1);
                },
                "isn't found by its full name",
            ),
            (
                &|idx| {
                    idx.names.insert((0, FieldId(99)));
                },
                "the full-name index has",
            ),
        ];
        for (breaks, says) in cases {
            let mut st = st.clone();
            breaks(&mut st.idx);
            match st.check_invariants() {
                Err(Invariant::Index(why)) => assert!(why.contains(says), "{why}"),
                other => panic!("{says}: {other:?}"),
            }
        }
    }

    /// A header of Bates numbers from 100.
    fn numbered() -> sheaf_edit::HeaderFooterSpec {
        sheaf_edit::HeaderFooterSpec {
            bates: Some(sheaf_edit::BatesSpec {
                start: 100,
                ..Default::default()
            }),
            ..crate::tx::fixture::header("<<bates>>")
        }
    }

    /// Each page's label is the one its place has, by the labels, or its
    /// number without them, after a move and with labels set again; the
    /// page map finds a duplicate as the copy it is, and the first in
    /// order of two that show one.
    #[test]
    fn the_state_s_tables_find_what_a_walk_does() {
        use crate::tx::fixture::{duplicate, label, move_pages};
        let mut d = crate::tx::tests::doc_of(6, &[]);
        let ids: Vec<PageId> = d.state().order().iter().copied().collect();
        let labelled_as_walked = |st: &DocState| {
            let count = st.page_count();
            let walked =
                (st.page_labels()).unwrap_or_else(|| (1..=count).map(|n| n.to_string()).collect());
            let found: Vec<String> = (st.order().iter())
                .map(|&id| st.label_of(id).unwrap().to_string())
                .collect();
            assert_eq!(found, walked);
            assert_eq!(st.check_invariants(), Ok(()));
        };
        labelled_as_walked(d.state());
        let style = sheaf_edit::LabelStyle::LowerRoman;
        let ranges = vec![label(ids[0], style, "", 1), label(ids[3], style, "A-", 1)];
        let set = |ranges| sheaf_edit::Op::SetPageLabels {
            ranges: Some(ranges),
        };
        d.commit_ops(vec![set(ranges.clone())], "Labels").unwrap();
        labelled_as_walked(d.state());
        d.commit_ops(vec![move_pages(&[ids[4]], Some(ids[0]))], "Move")
            .unwrap();
        labelled_as_walked(d.state());
        d.commit_ops(vec![set(ranges[..1].to_vec())], "Labels")
            .unwrap();
        labelled_as_walked(d.state());

        d.commit_ops(vec![duplicate(&[ids[2]])], "Duplicate")
            .unwrap();
        let st = d.state();
        let copy = st.page_at(st.pos_of(ids[2]).unwrap() + 1).unwrap();
        let def = st.def(ids[2]).unwrap();
        let (blob, object) = (def.base.src.id(), def.base.src.page(2).unwrap().id);
        assert_eq!(st.page_showing(blob, object, 0), Some(ids[2]));
        assert_eq!(st.page_showing(blob, object, 1), Some(copy));
        assert_eq!(st.page_showing(blob, object, 2), None);
        // A tree that lists its first page again, as copy 1 of it, and a
        // duplicate of the first, copy 1 too: of the two, the first in
        // order is found, before a move and after; a page shown from
        // another page leaves the map.
        let mut f = crate::forms::fixture::Form::new(2);
        let tree = f
            .dict(f.catalog)
            .get(b"Pages")
            .unwrap()
            .as_reference()
            .unwrap();
        let first = f.pages[0];
        let kids: Vec<lopdf::Object> = [first, f.pages[1], first].map(Into::into).into();
        f.dict(tree).set("Kids", kids);
        f.dict(tree).set("Count", 3);
        let mut d = crate::tx::tests::opened(f.bytes(), "twice.pdf");
        let [p0, p1, again] = [0, 1, 2].map(|i| d.page(i).unwrap());
        d.commit_ops(vec![duplicate(&[p0])], "Duplicate").unwrap();
        let copy = d.page(1).unwrap();
        let st = d.state();
        let blob = d.primary().id();
        assert_eq!(st.def(again).unwrap().copy, 1);
        assert_eq!(st.page_showing(blob, first, 0), Some(p0));
        assert_eq!(st.page_showing(blob, first, 1), Some(copy));
        d.commit_ops(vec![move_pages(&[again], Some(p0))], "Move")
            .unwrap();
        assert_eq!(d.state().page_showing(blob, first, 1), Some(again));
        let second = f.pages[1];
        assert_eq!(d.state().page_showing(blob, second, 0), Some(p1));
        let other = crate::store::blank(d.store(), 1);
        let replace = crate::tx::fixture::replace(&[p1], &other, [0], "b.pdf");
        d.commit_ops(vec![replace], "Replace").unwrap();
        let st = d.state();
        assert_eq!(st.page_showing(blob, second, 0), None);
        let shown = other.page(0).unwrap().id;
        assert_eq!(st.page_showing(other.id(), shown, 0), Some(p1));
        assert_eq!(st.check_invariants(), Ok(()));
    }
}
