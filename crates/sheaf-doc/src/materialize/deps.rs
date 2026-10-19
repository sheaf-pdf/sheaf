//! What an Append writes (M10.5 §7.2): the keys of the objects whose
//! emission a save's path changed, closed over what each one's object
//! depends on (`emit_deps`), judged by value against the saved state. An
//! Append writes these and what they reach that the saved file hasn't
//! got; every other object of the state is the saved file's.
//!
//! | A path key | changes the objects of |
//! |---|---|
//! | `Order` | the page tree's nodes it changed, and the pages put under another node ([`tree::spliced`]); the page labels, and the catalog when there are none now or were none; every connector (its `/Dest` is the next in order); each page the header and footer draws on (its numbers count the pages), and the catalog when it's on some pages (their places); a link to a source page another page, or none, is written as now, and its page |
//! | `Page(p)` | `p`'s dictionary; when its base page or copy changed, its annotations (they're copied as its), each bookmark to it (it may be written at another number) and each link to a source page another page, or none, is written as now |
//! | `Geom(p)`, `PageAnnots(p)` | `p`'s dictionary |
//! | `Annot(a)` | `a`'s objects; for a widget, its field's (a merged pair is one object); its replies (a mark's reply takes its `/Rect`, and each its `/IRT`); every connector of its group, for one |
//! | `Field(f)` | `f`'s dictionary, its widgets, every node above it (a node left with no widgets isn't written, nor the parents it empties); the form's dictionary when `f` came, went, moved or shows other widgets (its `/Fields` and `/CO` list what's written); the pages its widgets are on when one was its object and isn't, or the other way round |
//! | `Form` | the form's dictionary; when the files the forms come from changed, every field and widget of the other files (their fonts' renames in `/DR`), their document scripts and the catalog; when `/DA` or `/Q` changed, the other files' roots (theirs are pushed down) |
//! | `Bookmark(b)`, `OutlineRoots` | each bookmark whose item or place in the tree (`/Parent`, `/Prev`, `/Next`, `/First`, `/Last`, `/Count`) changed, and the outline's root when its own did |
//! | `Labels` | the page labels; the catalog when they came or went; the header and footer's pages |
//! | `Generator(g)` | the header and footer's pages, then and now, and the catalog, which holds its settings |
//!
//! And beside the path: the form's dictionary for a save transform the file
//! hasn't had (every field, for `DropXfa`'s tooltips), and the catalog for
//! `DropUr3`'s `/Perms`; the same for an XFA copy the file dropped that
//! policy keeps now; a page put under another node, or new to the saved
//! tree, when any page is written. A bookmark that goes to a page
//! that came or went changed itself, its target retargeted with the page.
//! What the file has of a source one of whose pages is written as another
//! page, or none, now is copied again where the section reaches it (a
//! link's indirect action names the page as it was copied).
//! The emitter adds the catalog when the section needs a later version
//! than the file, or the form or the outline came or went.
//!
//! An instance of a form whose objects are another copy of its file's now
//! ([`FormCopies`] moved its index, as instances came and went) is
//! [`Closure::form_shift`]: the saved file's objects of it are another
//! instance's, so the Append is refused (until slice 11 makes an instance
//! its blob's).

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use anyhow::Result;
use lopdf::{Object, ObjectId};

use super::emit::{self, tree};
use super::key::{FormCopies, NameTree, ObjKey, Part};
use super::plan::SaveTransform;
use super::record::{SaveInput, SavedSide};
use crate::annot::{AnnotId, AnnotKind};
use crate::model::{AnnotBody, AnnotRec, DocState, OutlineId, PageId};
use crate::prim::{Key, same};
use crate::store::BlobId;

/// An Append's scope.
#[derive(Debug, Clone)]
pub struct Closure {
    /// The keys whose objects it writes.
    pub keys: BTreeSet<ObjKey>,
    /// The node each page it writes is a kid of.
    pub parents: BTreeMap<PageId, ObjKey>,
    /// The saved file's page tree with the target's pages placed in it,
    /// when the order changed or a page has no place in it.
    pub tree: Option<tree::Spliced>,
    /// Whether a form instance both states hold is another copy of its
    /// file's now.
    pub form_shift: bool,
    /// The sources a page of which is written as another page, or none,
    /// than in the saved state: what the saved file's copies of their
    /// objects name of their pages is stale, so they're copied again.
    pub relinked: BTreeSet<BlobId>,
}

/// The scope of an Append of `input` onto the file of `saved`, with save
/// policy's `transforms`: the keys of `input`'s path whose values differ
/// between the saved state and the target, closed over what depends on
/// them. An error when the saved page tree can't be spliced.
pub fn closure(
    input: &SaveInput,
    saved: &SavedSide,
    transforms: &[SaveTransform],
) -> Result<Closure> {
    let (was, now) = (&saved.state, &input.target);
    let record = &saved.record;
    let mut cx = Deps {
        was,
        now,
        keys: BTreeSet::new(),
        fields: HashSet::new(),
        relinked: BTreeSet::new(),
    };
    let changed =
        (input.path_keys.iter()).filter(|key| !same(was.get(key).as_ref(), now.get(key).as_ref()));
    let (mut order, mut form, mut outline, mut labels, mut header) =
        (false, false, false, false, false);
    // Pages that show another page, or another copy, than they did.
    let mut rebased = BTreeSet::new();
    for key in changed {
        match *key {
            Key::Order => order = true,
            Key::Page(p) => {
                cx.keys.insert(ObjKey::Page(p));
                let base = |st: &DocState| st.def(p).map(|d| (d.base.clone(), d.copy));
                if base(was) != base(now) {
                    for (a, _) in now.annots_on(p) {
                        cx.annot(a);
                    }
                    rebased.insert(p);
                }
            }
            Key::Geom(p) | Key::PageAnnots(p) => {
                cx.keys.insert(ObjKey::Page(p));
            }
            Key::Annot(a) => cx.annot(a),
            Key::Field(f) => cx.field(f),
            Key::Form => form = true,
            Key::Bookmark(_) | Key::OutlineRoots => outline = true,
            Key::Labels => labels = true,
            Key::Generator(_) => header = true,
        }
    }
    if order {
        cx.connectors(None);
    }
    // The page labels say pages by their places; the catalog names them,
    // or no longer does (a range's first page gone).
    if order || labels {
        let has = |st: &DocState| st.label_ranges().is_some_and(|r| !r.is_empty());
        if has(now) {
            cx.keys.insert(ObjKey::Labels);
        }
        if has(was) != has(now) {
            cx.keys.insert(ObjKey::Catalog);
        }
    }
    if order || !rebased.is_empty() {
        cx.links(input);
    }
    cx.retargeted(&rebased);
    // The catalog holds the header and footer's settings, its pages by
    // their places.
    let placed = order && now.header_footer().is_some_and(|hf| hf.pages.is_some());
    if header || placed {
        cx.keys.insert(ObjKey::Catalog);
    }
    if header || ((order || labels) && now.header_footer().is_some()) {
        for st in [was, now] {
            for (id, _, _) in st.pages() {
                if crate::stamps::drawn_over(st, id).is_some() {
                    cx.keys.insert(ObjKey::Page(id));
                }
            }
        }
    }
    if form {
        cx.form(input);
    }
    if let Some(info) = input.sources.get(&input.primary)
        && record.map.identity == Some(info.src.id())
    {
        cx.fresh(&info.src, &record.map);
    }
    if outline {
        cx.outline();
    }
    for t in transforms.iter().filter(|t| !record.transforms.contains(t)) {
        match t {
            SaveTransform::DropXfa { tooltips_to_tu } => {
                cx.keys.insert(ObjKey::AcroForm);
                if *tooltips_to_tu {
                    cx.keys.extend(now.fields().map(|(f, _)| ObjKey::Field(f)));
                }
            }
            SaveTransform::FixSigFlags => {
                cx.keys.insert(ObjKey::AcroForm);
            }
            SaveTransform::DropUr3 => {
                cx.keys.insert(ObjKey::Catalog);
            }
        }
    }
    // An XFA copy the file dropped that policy keeps now comes back to the
    // form's dictionary, and the fields give back the tooltips it gave them.
    let dropped = |t: &SaveTransform| matches!(t, SaveTransform::DropXfa { .. });
    if record.transforms.iter().any(dropped) && !transforms.iter().any(dropped) {
        cx.keys.insert(ObjKey::AcroForm);
        cx.keys.extend(now.fields().map(|(f, _)| ObjKey::Field(f)));
    }
    // The pages written, and the nodes they go under: in the saved tree
    // as it is, while the order is, no page shows another page than it
    // did, and each has its place there; else in it spliced. A page that
    // shows another may be written at another page's number (two primary
    // pages trading their objects), which only the splice puts in order.
    let primary = input.sources.get(&input.primary).map(|info| info.src.id());
    let written = |keys: &BTreeSet<ObjKey>| -> BTreeSet<PageId> {
        (keys.iter())
            .filter_map(|k| match k {
                ObjKey::Page(p) => now.def(*p).map(|_| *p),
                _ => None,
            })
            .collect()
    };
    let mut parents = BTreeMap::new();
    let mut tree = None;
    let pages = written(&cx.keys);
    if let Some(primary) = primary.filter(|_| order || !pages.is_empty()) {
        let shown = emit::shown(now, &input.sources);
        let owned: HashMap<PageId, ObjectId> = emit::owned_pages(now, &shown, primary)
            .into_iter()
            .collect();
        let number = |p: PageId| {
            (record.map.keys.get(&ObjKey::Page(p)).copied()).or_else(|| owned.get(&p).copied())
        };
        let kept = (!order && rebased.is_empty())
            .then(|| tree::parents(&record.tree, &record.map, &number, &pages))
            .flatten();
        match kept {
            Some(kept) => parents = kept,
            None => {
                let order: Vec<PageId> = now.order().iter().copied().collect();
                let spliced = tree::spliced(&record.tree, &record.map, &number, &order)?;
                for node in spliced.nodes.iter().filter(|n| n.changed) {
                    cx.keys.insert(node.key);
                }
                cx.keys
                    .extend(spliced.moved.iter().map(|&p| ObjKey::Page(p)));
                parents = (written(&cx.keys).into_iter())
                    .filter_map(|p| Some((p, spliced.parent_of(p)?)))
                    .collect();
                tree = Some(spliced);
            }
        }
    }
    let form_shift = (was.field_count() > 0 || now.field_count() > 0) && {
        let copies = |st: &DocState| FormCopies::of(st, input.primary, &input.sources);
        let (then, at) = (copies(was), copies(now));
        (then.instances()).any(|inst| at.get(inst).is_some_and(|i| then.get(inst) != Some(i)))
    };
    Ok(Closure {
        keys: cx.keys,
        parents,
        tree,
        form_shift,
        relinked: cx.relinked,
    })
}

/// The closure being made: the saved state, the target, the keys so far,
/// the fields whose objects' dependents are in them, and the sources whose
/// pages are written otherwise.
struct Deps<'a> {
    was: &'a DocState,
    now: &'a DocState,
    keys: BTreeSet<ObjKey>,
    fields: HashSet<crate::forms::FieldId>,
    relinked: BTreeSet<BlobId>,
}

impl<'a> Deps<'a> {
    /// Each state, the saved one first.
    fn states(&self) -> [&'a DocState; 2] {
        [self.was, self.now]
    }

    /// Annotation `a`'s objects, and what depends on them.
    fn annot(&mut self, a: AnnotId) {
        if !self.keys.insert(ObjKey::Annot(a)) {
            return;
        }
        self.keys.extend(
            [
                Part::Ap,
                Part::Companion,
                Part::CompanionAp,
                Part::FileSpec,
                Part::Popup,
            ]
            .map(|part| ObjKey::AnnotPart(a, part)),
        );
        let mut names = HashSet::new();
        let mut pages = HashSet::new();
        let mut groups = Vec::new();
        for rec in self.states().into_iter().filter_map(|st| st.annot(a)) {
            if let Some(w) = rec.widget() {
                self.keys.insert(ObjKey::Field(w.field));
            }
            if let Some(AnnotKind::Connector { label, .. }) = rec.mark().map(|m| &m.kind) {
                groups.push(label.clone());
            }
            names.extend(name_of(rec));
            pages.insert(rec.page);
        }
        let mut replies = Vec::new();
        for st in self.states() {
            for &page in &pages {
                for (r, rec) in st.annots_on(page) {
                    if parent_of(rec).is_some_and(|n| names.contains(&n)) {
                        replies.push(r);
                    }
                }
            }
        }
        // Another tool's comment it replies to that has no `/NM` is given
        // the name it's replied to by.
        if let Some(rec) = self.now.annot(a)
            && let Some(parent) = parent_of(rec)
        {
            let unnamed = (self.now.annots_on(rec.page)).find(|(_, p)| match &p.body {
                AnnotBody::Foreign(f) => {
                    f.summary.name == parent
                        && (f.src.as_ref())
                            .and_then(|r| r.src.pdf().get_dictionary(r.id).ok())
                            .is_none_or(|d| !d.has(b"NM"))
                }
                _ => false,
            });
            replies.extend(unnamed.map(|(p, _)| p));
        }
        for r in replies {
            self.annot(r);
        }
        for label in groups {
            self.connectors(Some(&label));
        }
    }

    /// Every connector, or those labelled `label`, now: each one's `/Dest`
    /// is the next of its group in order.
    fn connectors(&mut self, label: Option<&str>) {
        let of_label = |rec: &AnnotRec| match rec.mark().map(|m| &m.kind) {
            Some(AnnotKind::Connector { label: l, .. }) => label.is_none_or(|label| l == label),
            _ => false,
        };
        let found: Vec<AnnotId> = (self.now.annots())
            .filter(|(_, rec)| of_label(rec))
            .map(|(a, _)| a)
            .collect();
        for a in found {
            if !self.keys.contains(&ObjKey::Annot(a)) {
                self.annot(a);
            }
        }
    }

    /// Field `f`'s object, and what depends on it.
    fn field(&mut self, f: crate::forms::FieldId) {
        // A widget's change put its field's key in already, which this
        // doesn't stop: the field's dependents are its own.
        if !self.fields.insert(f) {
            return;
        }
        self.keys.insert(ObjKey::Field(f));
        // The form's `/Fields` and `/CO` list the fields written, which a
        // node that came, went, moved or shows other widgets may change; a
        // value or a property changes neither, so a fill leaves the form's
        // dictionary as the file has it.
        let place = |st: &'a DocState| {
            (st.field(f)).map(|n| {
                let widgets = n.term.as_ref().map(|t| &t.widgets);
                (n.parent, &n.kids, n.inst, &n.src, widgets)
            })
        };
        if place(self.was) != place(self.now) {
            self.keys.insert(ObjKey::AcroForm);
        }
        let mut widgets = Vec::new();
        for st in self.states() {
            let Some(node) = st.field(f) else {
                continue;
            };
            widgets.extend(node.term.iter().flat_map(|t| t.widgets.iter().copied()));
            let mut up = node.parent;
            // A tree's depth is capped when it's read; this bounds a loop.
            for _ in 0..st.field_count() {
                let Some(p) = up else {
                    break;
                };
                self.keys.insert(ObjKey::Field(p));
                up = st.field(p).and_then(|n| n.parent);
            }
        }
        // A widget that was its field's object and isn't now, or the other
        // way round, is written at another number: the pages listing the
        // field's widgets list them anew.
        if merged_widget(self.was, f) != merged_widget(self.now, f) {
            let pages: Vec<PageId> = (widgets.iter())
                .filter_map(|&w| Some(self.now.annot(w)?.page))
                .collect();
            self.keys.extend(pages.into_iter().map(ObjKey::Page));
        }
        for w in widgets {
            self.annot(w);
        }
    }

    /// The form's dictionary, and what depends on what it holds.
    fn form(&mut self, input: &SaveInput) {
        self.keys.insert(ObjKey::AcroForm);
        let primary = input.primary;
        let others = |st: &DocState| -> Vec<BlobId> {
            let copies = FormCopies::of(st, primary, &input.sources);
            let mut seen = HashSet::new();
            (copies.instances())
                .filter(|inst| inst.src != primary)
                .filter_map(|inst| Some(input.sources.get(&inst.src)?.src.id()))
                .filter(|blob| seen.insert(*blob))
                .collect()
        };
        let (was, now) = (self.was, self.now);
        if others(was) != others(now) {
            self.keys.insert(ObjKey::Names(NameTree::JavaScript));
            self.keys.insert(ObjKey::Catalog);
            let fields: Vec<_> = (now.fields())
                .filter(|(_, node)| node.inst.src != primary)
                .map(|(f, _)| f)
                .collect();
            for f in fields {
                self.field(f);
            }
        }
        let (a, b) = (was.form(), now.form());
        if a.da != b.da || a.align != b.align {
            let roots: Vec<_> = (b.roots.iter().copied())
                .filter(|&r| now.field(r).is_some_and(|n| n.inst.src != primary))
                .collect();
            self.keys.extend(roots.into_iter().map(ObjKey::Field));
        }
    }

    /// The marks of `primary`, the file's, the open gave new ids (copies
    /// another program made of a page's marks) that the file, whose `map`
    /// lists no object for them, still has under the ids they were copied
    /// with: the first save writes them anew, as a Rewrite would.
    fn fresh(&mut self, primary: &crate::Source, map: &super::record::ObjMap) {
        let facts = primary.annots();
        let mut found = Vec::new();
        for &(page, slot) in &facts.fresh {
            let id = (facts.on(page).iter()).find_map(|a| match &a.body {
                crate::open::FileBody::Mark { annot, .. } if a.slot == slot => Some(annot.id),
                _ => None,
            });
            if let Some(id) = id
                && self.now.annot(id).is_some()
                && !map.keys.contains_key(&ObjKey::Annot(id))
            {
                found.push(id);
            }
        }
        for a in found {
            self.annot(a);
        }
    }

    /// Each bookmark that goes to one of `rebased`, pages that show another
    /// page than they did: such a page may be written at another number.
    fn retargeted(&mut self, rebased: &BTreeSet<PageId>) {
        if rebased.is_empty() {
            return;
        }
        let to = (self.now.bookmarks_preorder().into_iter())
            .filter(|(_, _, b)| b.page().is_some_and(|p| rebased.contains(&p)))
            .map(|(_, id, _)| ObjKey::Bookmark(id))
            .collect::<Vec<_>>();
        self.keys.extend(to);
    }

    /// Each bookmark whose item or place changed, and the outline's root
    /// when its own place did.
    fn outline(&mut self) {
        let (was, now) = (places(self.was), places(self.now));
        for (&b, place) in &now.items {
            let changed = !same(
                self.was.get(&Key::Bookmark(b)).as_ref(),
                self.now.get(&Key::Bookmark(b)).as_ref(),
            );
            if changed || was.items.get(&b) != Some(place) {
                self.keys.insert(ObjKey::Bookmark(b));
            }
        }
        if was.root != now.root {
            self.keys.insert(ObjKey::OutlinesRoot);
        }
    }

    /// Each link to a source page that another page, or none, is written
    /// as now ([`emit::written_pages`]), and the page it's on: its
    /// destination is that page as written, or null. And the sources of
    /// such pages, whose objects the file has (a link's action, a
    /// destination of its own) may name one too: they're copied again.
    fn links(&mut self, input: &SaveInput) {
        let written = |st: &DocState| emit::written_pages(st, &emit::shown(st, &input.sources));
        let (then, now) = (written(self.was), written(self.now));
        let gone: HashSet<(BlobId, ObjectId)> = (then.iter().chain(&now))
            .filter(|&(at, page)| then.get(at) != Some(page) || now.get(at) != Some(page))
            .map(|(&(blob, obj, _), _)| (blob, obj))
            .collect();
        if gone.is_empty() {
            return;
        }
        self.relinked.extend(gone.iter().map(|&(blob, _)| blob));
        let mut found = Vec::new();
        for (a, rec) in self.now.annots() {
            let AnnotBody::Foreign(f) = &rec.body else {
                continue;
            };
            let Some(r) = f.src.as_ref().filter(|_| f.summary.subtype == "Link") else {
                continue;
            };
            let to = dest_page(r.src.pdf(), r.id);
            if to.is_some_and(|to| gone.contains(&(r.src.id(), to))) {
                found.push((a, rec.page));
            }
        }
        for (a, page) in found {
            self.annot(a);
            self.keys.insert(ObjKey::Page(page));
        }
    }
}

/// The widget field `f` of `st` is written as one object with, as the
/// emitter merges them: its one widget, read merged with it.
fn merged_widget(st: &DocState, f: crate::forms::FieldId) -> Option<AnnotId> {
    let node = st.field(f)?;
    let src = node.src.as_ref()?;
    let widgets = &node.term.as_ref()?.widgets;
    let w = *widgets.front().filter(|_| widgets.len() == 1)?;
    let widget = st.annot(w)?.widget()?;
    let own = widget.src.as_ref().is_some_and(|r| r.id == src.id);
    (widget.merged && own).then_some(w)
}

/// The name a reply names annotation `rec` by.
fn name_of(rec: &AnnotRec) -> Option<String> {
    match &rec.body {
        AnnotBody::Mark(m) => Some(m.annot.name().to_string()),
        AnnotBody::Foreign(f) => Some(f.summary.name.clone()),
        AnnotBody::Widget(_) => None,
    }
}

/// The name of the annotation `rec` replies to, or belongs to.
fn parent_of(rec: &AnnotRec) -> Option<String> {
    match &rec.body {
        AnnotBody::Mark(m) => m.annot.thread_parent().map(str::to_string),
        AnnotBody::Foreign(f) => f.summary.parent.clone(),
        AnnotBody::Widget(_) => None,
    }
}

/// The page link `id` of `pdf` goes to: its `/Dest`'s, or its GoTo
/// action's, when that's an explicit destination.
fn dest_page(pdf: &lopdf::LazyPdf, id: ObjectId) -> Option<ObjectId> {
    let link = pdf.get_dictionary(id).ok()?;
    let dest = match link.get(b"Dest") {
        Ok(dest) => dest,
        Err(_) => {
            let (_, action) = pdf.dereference(link.get(b"A").ok()?).ok()?;
            action.as_dict().ok()?.get(b"D").ok()?
        }
    };
    let (_, dest) = pdf.dereference(dest).ok()?;
    match dest.as_array().ok()?.first()? {
        Object::Reference(page) => Some(*page),
        _ => None,
    }
}

/// An outline's places: each bookmark's, and its root's (first, last and
/// count).
struct Places {
    items: HashMap<OutlineId, Place>,
    root: (Option<OutlineId>, Option<OutlineId>, i64),
}

/// A bookmark's place, as the emitter writes it: its parent (None for the
/// root), its neighbors, its first and last kids, and `/Count`.
#[derive(Debug, PartialEq)]
struct Place {
    parent: Option<OutlineId>,
    prev: Option<OutlineId>,
    next: Option<OutlineId>,
    first: Option<OutlineId>,
    last: Option<OutlineId>,
    count: Option<i64>,
}

/// `st`'s outline's places.
fn places(st: &DocState) -> Places {
    let mut items = HashMap::new();
    let roots: Vec<OutlineId> = st.outline_roots().iter().copied().collect();
    let shown = level(st, None, &roots, &mut items, 0);
    Places {
        items,
        root: (roots.first().copied(), roots.last().copied(), shown),
    }
}

/// The places of `kids`, under `parent`, into `items`: how many items show
/// below `parent` while it's open. `depth` bounds the walk as the state's
/// outline is bounded.
fn level(
    st: &DocState,
    parent: Option<OutlineId>,
    kids: &[OutlineId],
    items: &mut HashMap<OutlineId, Place>,
    depth: usize,
) -> i64 {
    let kids: Vec<OutlineId> = (kids.iter().copied())
        .filter(|&id| st.bookmark(id).is_some())
        .collect();
    let mut shown = 0;
    for (i, &id) in kids.iter().enumerate() {
        let b = st.bookmark(id).expect("filtered");
        let under: Vec<OutlineId> = b.kids.iter().copied().collect();
        let below = match depth < st.bookmark_count() {
            true => level(st, Some(id), &under, items, depth + 1),
            false => 0,
        };
        let first = under.iter().copied().find(|&k| st.bookmark(k).is_some());
        let last = under
            .iter()
            .rev()
            .copied()
            .find(|&k| st.bookmark(k).is_some());
        items.insert(
            id,
            Place {
                parent,
                prev: i.checked_sub(1).map(|i| kids[i]),
                next: kids.get(i + 1).copied(),
                first,
                last,
                count: first.map(|_| if b.open { below } else { -below }),
            },
        );
        shown += 1 + if b.open { below } else { 0 };
    }
    shown
}
