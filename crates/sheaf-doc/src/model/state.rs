//! One version of the document (M10.5 §3.2): its pages, annotations,
//! form, bookmarks, page labels and generators as records. A state is
//! made by [`DocState::builder`] or from another by applying prims, and
//! read through accessors: no struct literal outside this module can make
//! one whose derived indexes don't fit its tables.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::hash::{BuildHasher, BuildHasherDefault, DefaultHasher};
use std::sync::{Arc, OnceLock};

use im::Vector;
use sheaf_core::Rotation;

use super::{
    AnnotBody, AnnotRec, Bookmark, FieldNode, Foreign, FormDoc, GenId, Generator, Mark, OutlineId,
    PageDef, PageId, Terminal, Widget, bookmark_span,
};
use crate::annot::{Annot, AnnotId, AnnotKind};
use crate::forms::{FieldId, full_name_of};
use crate::geom::Geom;
use crate::labels::{LabelRange, LabelSpec};
use crate::prim::{Key, Mismatch, Val};
use crate::stamps::HeaderFooter;
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
    /// Every node of the form's field tree: [`Key::Field`].
    fields: im::OrdMap<FieldId, Arc<FieldNode>>,
    /// The form as a whole: [`Key::Form`].
    form: Arc<FormDoc>,
    idx: Indexes,
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
}

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
            && *fields == other.fields
            && *form == other.form
    }
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
                }
                _ => return misfit,
            },
            Key::Page(page) => match val {
                Some(Val::Page(v)) => _ = self.pages.insert(page, v),
                None => _ = self.pages.remove(&page),
                _ => return misfit,
            },
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
            Key::Labels => match val {
                Some(Val::Labels(v)) => self.labels = Some(v),
                None => self.labels = None,
                _ => return misfit,
            },
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

    /// What page `id` shows.
    pub fn def(&self, id: PageId) -> Option<&Arc<PageDef>> {
        self.pages.get(&id)
    }

    /// Page `id`'s boxes and `/Rotate`.
    pub fn geom(&self, id: PageId) -> Option<&Arc<Geom>> {
        self.geoms.get(&id)
    }

    /// Every page in order, with what it shows and its boxes.
    pub fn pages(&self) -> impl ExactSizeIterator<Item = (PageId, &Arc<PageDef>, &Arc<Geom>)> {
        self.order
            .iter()
            .map(|&id| (id, &self.pages[&id], &self.geoms[&id]))
    }

    /// Page `id`'s recognized text, which no file holds yet.
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

    /// Whether `other`'s page labels are these.
    pub(crate) fn labels_eq(&self, other: &DocState) -> bool {
        match (&self.labels, &other.labels) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b) || a == b,
            (a, b) => a.is_none() && b.is_none(),
        }
    }

    /// Every generator, by id.
    pub fn generators(&self) -> &im::OrdMap<GenId, Arc<Generator>> {
        &self.generators
    }

    /// The header/footer: the spec of the generator that draws it as sheaf
    /// did before generators (M10.5 slice 8, until 17).
    pub fn header_footer(&self) -> Option<&Arc<HeaderFooter>> {
        self.generators
            .values()
            .find_map(|g| g.spec.header_footer())
    }

    /// Annotation `id`.
    pub fn annot(&self, id: AnnotId) -> Option<&AnnotRec> {
        self.annots.get(&id)
    }

    /// Whether some annotation has id `id`.
    pub fn has_annot(&self, id: AnnotId) -> bool {
        self.annots.contains_key(&id)
    }

    /// Page `page`'s annotations, in `/Annots` order.
    pub fn annots_on(&self, page: PageId) -> impl Iterator<Item = (AnnotId, &AnnotRec)> {
        let ids = self.page_annots.get(&page).into_iter().flatten();
        ids.filter_map(|&id| Some((id, self.annots.get(&id)?)))
    }

    /// The ids of page `page`'s annotations, in `/Annots` order; None when
    /// it has none.
    pub fn annot_ids_on(&self, page: PageId) -> Option<&Vector<AnnotId>> {
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
        self.annots.iter().map(|(&id, rec)| (id, rec))
    }

    /// Every one of sheaf's marks, with its page, in no order.
    pub fn marks(&self) -> impl Iterator<Item = (PageId, &Annot)> {
        self.annots
            .values()
            .filter_map(|rec| Some((rec.page, rec.mark()?)))
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
        self.fields.get(&id)
    }

    /// Every field node, by id.
    pub fn fields(&self) -> impl Iterator<Item = (FieldId, &Arc<FieldNode>)> {
        self.fields.iter().map(|(&id, node)| (id, node))
    }

    /// How many field nodes there are.
    pub fn field_count(&self) -> usize {
        self.fields.len()
    }

    /// Every terminal field, by id.
    pub fn terminals(&self) -> impl Iterator<Item = (FieldId, &FieldNode, &Terminal)> {
        self.fields
            .iter()
            .filter_map(|(&id, node)| Some((id, &**node, &**node.term.as_ref()?)))
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
        let hash = name_hash(name);
        let ids = self
            .idx
            .names
            .range((hash, FieldId(0))..=(hash, FieldId(u64::MAX)));
        ids.map(|&(_, id)| id)
            .find(|&id| self.full_name(id).as_deref() == Some(name))
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
        bookmark_span(self, id)
    }

    /// Whether `other`'s bookmarks are these.
    pub(crate) fn outline_eq(&self, other: &DocState) -> bool {
        (self.outline_roots.ptr_eq(&other.outline_roots)
            || self.outline_roots == other.outline_roots)
            && (self.bookmarks.ptr_eq(&other.bookmarks) || self.bookmarks == other.bookmarks)
    }

    /// The pages whose annotations aren't `base`'s: an annotation added,
    /// changed or gone, or the page's list in another order.
    pub(crate) fn annots_changed_from(&self, base: &DocState) -> HashSet<PageId> {
        let mut changed = HashSet::new();
        let same_annots = self.annots.ptr_eq(&base.annots);
        if same_annots && self.page_annots.ptr_eq(&base.page_annots) {
            return changed;
        }
        let same = |id: &AnnotId| match (self.annots.get(id), base.annots.get(id)) {
            (Some(a), Some(b)) => a.same(b),
            _ => false,
        };
        for (&page, ids) in &self.page_annots {
            let kept = base
                .page_annots
                .get(&page)
                .is_some_and(|was| was.ptr_eq(ids) || was == ids);
            if !kept || !(same_annots || ids.iter().all(same)) {
                changed.insert(page);
            }
        }
        changed
            .extend((base.page_annots.keys()).filter(|page| !self.page_annots.contains_key(page)));
        changed
    }

    /// Whether page `page`'s widgets are `base`'s: the same ones, in the
    /// same order.
    pub(crate) fn widgets_eq_on(&self, base: &DocState, page: PageId) -> bool {
        let now: Vec<_> = self.widgets_on(page).collect();
        let was: Vec<_> = base.widgets_on(page).collect();
        now.len() == was.len()
            && now
                .iter()
                .zip(&was)
                .all(|((a, x), (b, y))| a == b && (Arc::ptr_eq(x, y) || x == y))
    }

    /// The field nodes that aren't `base`'s: added, changed or gone.
    pub(crate) fn fields_changed_from(&self, base: &DocState) -> Vec<FieldId> {
        if self.fields.ptr_eq(&base.fields) {
            return Vec::new();
        }
        let mut changed: Vec<FieldId> = (self.fields.iter())
            .filter(|(id, node)| {
                (base.fields.get(id)).is_none_or(|was| !Arc::ptr_eq(was, node) && was != *node)
            })
            .map(|(&id, _)| id)
            .collect();
        changed.extend(
            base.fields
                .keys()
                .filter(|id| !self.fields.contains_key(id)),
        );
        changed
    }

    /// Whether it has every annotation `base` has, or a mark in its place:
    /// another tool's comment made ours (the mark is its object, or for
    /// one written into `/Annots`, has its name) and the parts grouped
    /// under it, which the mark draws.
    pub(crate) fn keeps_annots_of(&self, base: &DocState) -> bool {
        if self.annots.ptr_eq(&base.annots) {
            return true;
        }
        (base.annots.iter())
            .filter(|(id, _)| !self.annots.contains_key(id))
            .all(|(_, was)| {
                let Some(f) = was.foreign() else {
                    return false;
                };
                let mut marks = self.mark_records_on(was.page);
                match (&f.summary.parent, &f.src) {
                    (Some(parent), _) if f.summary.grouped => {
                        marks.any(|m| m.annot.name() == *parent)
                    }
                    (_, Some(src)) => marks.any(|m| m.src.as_ref() == Some(src)),
                    (_, None) => marks.any(|m| m.annot.name() == f.summary.name),
                }
            })
    }

    /// Whether it has every bookmark `base` has.
    pub(crate) fn keeps_bookmarks_of(&self, base: &DocState) -> bool {
        self.bookmarks.ptr_eq(&base.bookmarks)
            || base
                .bookmarks
                .keys()
                .all(|id| self.bookmarks.contains_key(id))
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
    /// is listed once, on its page. Each widget and its terminal field name
    /// each other, in one instance of a form with a name. The bookmarks and
    /// the field nodes are trees from their roots, each node in once, a
    /// field under the parent it names, and a bookmark going to a page
    /// there is. There's one header/footer at most, and each generator is
    /// under its id. Every index is what its table makes again.
    ///
    /// Not yet: the pages label ranges start at, the header/footer's
    /// pages, and the comments replies and connectors name, which edits
    /// don't keep or check until slice 11 (M10.5 bugs 11 and 12); nor that
    /// the `/NM`s on a page or the form's full names are unique, which
    /// files break (the indexes hold each one).
    pub fn check_invariants(&self) -> Result<(), Invariant> {
        let DocState {
            order,
            pages,
            geoms,
            bookmarks,
            outline_roots,
            page_annots,
            annots,
            // Anchored to pages no edit checks yet.
            labels: _,
            generators,
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

        // The indexes.
        if let Some(pos) = idx.pos.get() {
            let found = pos.len() == order.len()
                && (order.iter().zip(0..)).all(|(id, i)| pos.get(id) == Some(&i));
            hold(found, || {
                Invariant::Index("the positions aren't the order's".into())
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

    /// What its snapshot copied from its parent's to put `keys` (M10.5
    /// §3.9): for each slot of a per-page, per-annotation, per-field,
    /// per-bookmark or per-generator map, a path down from the root, and
    /// for an annotation or a field, others in the indexes that find it by
    /// name; for a new order, the positions it finds. A whole value (the
    /// top-level bookmarks, the labels, the form) is its prim's.
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
            fields,
            form: _,
            idx,
        } = self;
        let mut new_order = false;
        let [mut p, mut g, mut b, mut pa, mut a, mut f, mut gens] = [0; 7];
        for key in keys {
            match key {
                Key::Order => new_order = true,
                Key::Page(_) => p += 1,
                Key::Geom(_) => g += 1,
                Key::Bookmark(_) => b += 1,
                Key::PageAnnots(_) => pa += 1,
                Key::Annot(_) => a += 1,
                Key::Field(_) => f += 1,
                Key::Generator(_) => gens += 1,
                Key::OutlineRoots | Key::Labels | Key::Form => {}
            }
        }
        // A hash table of positions, about an eighth of it empty.
        let positions = match new_order {
            true => order.len() * (size_of::<(PageId, u32)>() + 1) * 8 / 7,
            false => 0,
        };
        size_of::<DocState>()
            + positions
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::FixedCount;
    use crate::model::Edit;
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
        let add = Edit::AnnotAdd {
            page: p0,
            annot: square(0),
        };
        d.apply(add, "add").unwrap();
        let st = d.state().clone();
        assert_eq!(st.pos_of(p0), Some(0));
        assert_eq!(st.check_invariants(), Ok(()));
        let (mark, rec) = st.annots_on(p0).find(|(_, a)| a.mark().is_some()).unwrap();
        let named = nm_key(mark, rec).unwrap();
        let field = st.terminals().next().unwrap().0;
        let hash = name_hash(&st.full_name(field).unwrap());
        type Breaks<'a> = &'a dyn Fn(&mut Indexes);
        let cases: [(Breaks, &str); 6] = [
            (
                &|idx| idx.pos = Arc::new(OnceLock::from(HashMap::from([(p0, 1)]))),
                "the positions aren't the order's",
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
}
