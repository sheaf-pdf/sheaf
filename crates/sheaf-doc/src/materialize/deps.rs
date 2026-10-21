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
//! | `Geom(p)` | `p`'s dictionary; every connector of each group with one on `p` (a group's order is how its connectors show on their pages) |
//! | `PageAnnots(p)` | `p`'s dictionary |
//! | `Annot(a)` | `a`'s objects; for a widget, its field's (a merged pair is one object); its replies (a mark's reply takes its `/Rect`, and each its `/IRT`); every connector of its group, for one; its page, when it's a replacement now and wasn't or the other way round (a replacement's StrikeOut is beside it in `/Annots`) |
//! | `Field(f)` | `f`'s dictionary, its widgets, every node above it (a node left with no widgets isn't written, nor the parents it empties); the form's dictionary when `f` came, went, moved or shows other widgets (its `/Fields` and `/CO` list what's written); the pages its widgets are on when one was its object and isn't, or the other way round |
//! | `Form` | the form's dictionary; when the files the forms come from changed, every field and widget of the other files (their fonts' renames in `/DR`), their document scripts and the catalog; when `/DA` or `/Q` changed, the other files' roots (theirs are pushed down) |
//! | `Bookmark(b)`, `OutlineRoots` | each bookmark whose item or place in the tree (`/Parent`, `/Prev`, `/Next`, `/First`, `/Last`, `/Count`) changed, and the outline's root when its own did |
//! | `Labels` | the page labels; the catalog when they came or went; the header and footer's pages |
//! | `Generator(g)` | a watermark's: what the generators are (`/SheafGenerators`), the catalog when the first came or the last went, and the pages it's on then or now (only those it came onto or went from, when nothing else of it changed); the header and footer's: its pages, then and now, and the catalog, which holds its settings |
//!
//! And beside the path: the form's dictionary for a save transform the file
//! hasn't had (every field, for `DropXfa`'s tooltips), and the catalog for
//! `DropUr3`'s `/Perms`; the same for an XFA copy the file dropped that
//! policy keeps now; a page put under another node, or new to the saved
//! tree, when any page is written. A page the file has at the number of a
//! primary page that another page is copy 0 of now leaves it to that page
//! (a page replaced, appended over the object it was, and that page brought
//! back): it's written at a number of its own, with what depends on it as
//! for a page that shows another ([`Closure::renumbered`]). A bookmark that
//! goes to a page that came or went changed itself, its target retargeted
//! with the page. What the file has of a source one of whose pages is
//! written as another page, or none, or at another number, now is copied
//! again where the section reaches it (a link's indirect action names the
//! page as it was copied), an annotation the records don't hold in a page's
//! `/Annots` among it (its `/P` names the page it was written with). What else of the file names such a page, as a
//! copy's reference resolves it now, and isn't written again
//! ([`Closure::stale`]: a widget's action, a field's, a link written
//! straight into `/Annots`, an annotation the records don't hold, a page's
//! own keys, or anything of the primary's copy 0, whose references are the
//! numbers the file has) refuses the Append, as does what the primary's
//! catalog holds that names a page of it that's no page's copy 0 now (an
//! `/OpenAction`, a named destination, the structure tree), which a Rewrite
//! prunes.
//! The emitter adds the catalog when the section needs a later version
//! than the file, or the form or the outline came or went.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use anyhow::Result;
use lopdf::{Object, ObjectId};

use super::append::refs;
use super::copy;
use super::emit::{self, annots, tree};
use super::key::{NameTree, ObjKey, Part};
use super::plan::SaveTransform;
use super::record::{CATALOG_OWNED, SaveInput, SavedSide};
use crate::annot::{AnnotId, AnnotKind};
use crate::model::{AnnotBody, AnnotRec, DocState, Generator, OutlineId, PageId, WatermarkGen};
use crate::prim::{Key, same};
use crate::store::{BlobId, Src};

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
    /// The sources a page of which is written as another page, or none,
    /// than in the saved state: what the saved file's copies of their
    /// objects name of their pages is stale, so they're copied again.
    pub relinked: BTreeSet<BlobId>,
    /// What the saved file has that names a page of its source that a
    /// copy's reference to it names another page by now, or none, and
    /// that the section doesn't write again: the key of what holds it
    /// (an annotation, a field, a page, or another tool's annotation the
    /// records don't hold), or the catalog, for what it holds that names a
    /// page of the primary that's no page's copy 0 now. Only a Rewrite
    /// names it anew, or prunes it.
    pub stale: Option<ObjKey>,
    /// The pages the file has at the number of a primary page another
    /// page is copy 0 of now: written at a number of their own, with what
    /// names them by number. The only keys `append::held` lets leave their
    /// number to the primary object that's another key now.
    pub renumbered: BTreeSet<PageId>,
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
        relinking: None,
        connected: HashSet::new(),
        every_connector: false,
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
            Key::Geom(p) => {
                cx.keys.insert(ObjKey::Page(p));
                cx.connectors_on(p);
            }
            Key::PageAnnots(p) => {
                cx.keys.insert(ObjKey::Page(p));
            }
            Key::Annot(a) => cx.annot(a),
            Key::Field(f) => cx.field(f),
            Key::Form => form = true,
            Key::Bookmark(_) | Key::OutlineRoots => outline = true,
            Key::Labels => labels = true,
            Key::Generator(g) => {
                let generator = |st: &'_ DocState| st.generators().get(&g).cloned();
                match (generator(was), generator(now)) {
                    (then, after) if is_watermark(&then) || is_watermark(&after) => {
                        cx.watermark(then.as_deref(), after.as_deref());
                    }
                    _ => header = true,
                }
            }
        }
    }
    // A page the file has at the number of a primary page that another
    // page is copy 0 of now (a page replaced, appended over the object it
    // was, and that page brought back) leaves the number to that page and
    // is written at one of its own: what names it by number is written
    // again, as for a page that shows another page.
    let primary = input.primary.id();
    let shown = emit::shown(now, &input.derived);
    let owned: HashMap<PageId, ObjectId> =
        emit::owned_pages(now.order().iter().copied(), &shown, primary)
            .into_iter()
            .collect();
    let owners: HashMap<ObjectId, PageId> = owned.iter().map(|(&p, &at)| (at, p)).collect();
    let renumbered: BTreeSet<PageId> = (now.pages())
        .filter(|&(p, _, _)| {
            (record.map.keys.get(&ObjKey::Page(p)))
                .and_then(|at| owners.get(at))
                .is_some_and(|&q| q != p)
        })
        .map(|(p, _, _)| p)
        .collect();
    for &p in &renumbered {
        cx.keys.insert(ObjKey::Page(p));
        for (a, _) in now.annots_on(p) {
            cx.annot(a);
        }
        rebased.insert(p);
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
        cx.links(input, &renumbered);
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
    if record.map.identity == Some(input.primary.id()) {
        cx.fresh(&input.primary, &record.map);
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
    if order || !pages.is_empty() {
        // A page renumbered has no place in the saved tree: its number
        // there is the page's that's copy 0 of its object now.
        let number = |p: PageId| match renumbered.contains(&p) {
            true => None,
            false => {
                (record.map.keys.get(&ObjKey::Page(p)).copied()).or_else(|| owned.get(&p).copied())
            }
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
    let stale = cx.stale(input);
    Ok(Closure {
        keys: cx.keys,
        parents,
        tree,
        relinked: cx.relinked,
        stale,
        renumbered,
    })
}

/// The closure being made: the saved state, the target, the keys so far,
/// the fields whose objects' dependents are in them, and the sources whose
/// pages are written otherwise, and how.
struct Deps<'a> {
    was: &'a DocState,
    now: &'a DocState,
    keys: BTreeSet<ObjKey>,
    fields: HashSet<crate::forms::FieldId>,
    relinked: BTreeSet<BlobId>,
    relinking: Option<Relinking>,
    /// The connector groups taken whole, by label, or whether every
    /// connector was ([`connectors`](Self::connectors)).
    connected: HashSet<String>,
    every_connector: bool,
}

/// The pages written as each copy of each source page ([`emit::written_pages`]),
/// by the source's blob, the page's object and the copy.
type Written = BTreeMap<(BlobId, ObjectId, u32), PageId>;

/// How the pages of sources are written in the saved state and the
/// target, when any is written as another page, or none, now: what a
/// copy's reference to one names ([`Relinking::names`]).
struct Relinking {
    primary: BlobId,
    then: Written,
    now: Written,
    /// The source pages written otherwise, as any copy, or as a page
    /// renumbered.
    gone: HashSet<(BlobId, ObjectId)>,
    /// The pages written at another number than the file has them at
    /// ([`Closure::renumbered`]).
    renumbered: BTreeSet<PageId>,
    /// What each page of the target is written as.
    shown: HashMap<PageId, emit::Shown>,
}

impl Relinking {
    /// The page a reference to page `page` of `blob`, read as `copy` of
    /// it, names when `pages` are written, as the copy closure writes it:
    /// from the primary's copy 0 the page's own object, which the page
    /// that's its copy 0 is written at (and which `append::held` keeps
    /// there); from anything else the page written as that copy, else as
    /// its lowest copy written ([`copy::page_named`]).
    fn names(&self, pages: &Written, blob: BlobId, page: ObjectId, copy: u32) -> Option<PageId> {
        match blob == self.primary && copy == 0 {
            true => pages.get(&(blob, page, 0)).copied(),
            false => copy::page_named(pages, blob, page, copy),
        }
    }

    /// Whether a reference to page `page` of `blob`, read as `copy` of it,
    /// that the file has names another page now than in the saved state,
    /// or none. From the primary's copy 0 it's the page's number, which
    /// holds the page that's its copy 0 now (`append::held` refuses
    /// otherwise): one that named no page names the page that came; any
    /// other was written as the page it named then, or null, which may be
    /// at another number now.
    fn moved(&self, blob: BlobId, page: ObjectId, copy: u32) -> bool {
        if !self.gone.contains(&(blob, page)) {
            return false;
        }
        let then = self.names(&self.then, blob, page, copy);
        let now = self.names(&self.now, blob, page, copy);
        match blob == self.primary && copy == 0 {
            true => then.is_some() && then != now,
            false => then != now || then.is_some_and(|p| self.renumbered.contains(&p)),
        }
    }

    /// Whether page `page` of the primary, read as copy 0, was a page's
    /// copy 0 in the saved state and is no page's now: what names it by
    /// its number names a page outside the tree. (One that's another page's copy 0 now is
    /// written there, or `append::held` refuses, as a Rewrite writes it.)
    fn went(&self, blob: BlobId, page: ObjectId, copy: u32) -> bool {
        blob == self.primary
            && copy == 0
            && self.gone.contains(&(blob, page))
            && self.then.contains_key(&(blob, page, 0))
            && !self.now.contains_key(&(blob, page, 0))
    }

    /// Whether a link read from `blob` as `copy` of it can name page
    /// `page` now, as the copy closure judges it
    /// ([`Closure::writes_page`](copy::Closure::writes_page)).
    fn writes(&self, blob: BlobId, page: ObjectId, copy: u32) -> bool {
        self.names(&self.now, blob, page, copy).is_some()
    }
}

/// The keys of an annotation an emitter writes itself, never copying them
/// from its file: its page, its parent field, the annotation it replies
/// to and its pop-up (written as its own, `/Parent` and `/P` the emitter's).
const ANNOT_OWN: &[&[u8]] = &[b"P", b"Parent", b"IRT", b"Popup"];
/// A field's: its parent and kids (each walked as its own), and its page
/// when it's its widget too.
const FIELD_OWN: &[&[u8]] = &[b"P", b"Parent", b"Kids"];
/// A page's: its node, its annotations (each walked as its own), and what
/// it draws, whose objects never name a page.
const PAGE_OWN: &[&[u8]] = &[b"Parent", b"Annots", b"Contents", b"Resources", b"Thumb"];

/// What a copy of a source reaches from an object of it that's written,
/// walked for a reference to a page that names another page now than in
/// the saved state ([`Relinking::moved`]).
struct Walk<'a> {
    rl: &'a Relinking,
    /// Whether any reference to a page of a source, read as a copy of it,
    /// names another page now, by the source and copy.
    moves: HashMap<(BlobId, u32), bool>,
    /// Each source's document-level objects, which a copy never follows.
    levels: HashMap<BlobId, HashMap<ObjectId, Option<ObjKey>>>,
    /// The objects walked already, by their source and copy.
    seen: HashMap<(BlobId, u32), HashSet<ObjectId>>,
}

impl Walk<'_> {
    /// Whether any reference to a page of `blob`, read as `copy` of it,
    /// names another page now, or none: else nothing read so needs
    /// walking.
    fn moves(&mut self, blob: BlobId, copy: u32) -> bool {
        let rl = self.rl;
        *(self.moves.entry((blob, copy))).or_insert_with(|| {
            (rl.gone.iter()).any(|&(b, page)| b == blob && rl.moved(b, page, copy))
        })
    }

    /// Whether `root`, an object of `src` read as `copy` of it (or a
    /// reference to one), but for its keys `own` (what its emitter writes
    /// itself), reaches a page a reference to which names another page now,
    /// or none: as a copy follows references, never into a page or a
    /// document-level object, each object once a copy.
    fn reaches(&mut self, src: &Src, copy: u32, root: &Object, own: &[&[u8]]) -> bool {
        self.reaches_by(src, copy, root, own, Relinking::moved)
    }

    /// [`Walk::reaches`], a page found judged by `hit` (a page's
    /// [`Relinking::moved`], or its [`Relinking::went`], which implies it,
    /// so what any walk saw is never walked again).
    fn reaches_by(
        &mut self,
        src: &Src,
        copy: u32,
        root: &Object,
        own: &[&[u8]],
        hit: fn(&Relinking, BlobId, ObjectId, u32) -> bool,
    ) -> bool {
        let blob = src.id();
        if !self.moves(blob, copy) {
            return false;
        }
        let pdf = src.pdf();
        let levels = (self.levels.entry(blob)).or_insert_with(|| copy::document_level(pdf));
        let seen = self.seen.entry((blob, copy)).or_default();
        let mut next: Vec<ObjectId> = Vec::new();
        let read = match root {
            Object::Reference(r) => match pdf.object_unkept(*r) {
                Ok(read) => Some(read),
                // Copied as null.
                Err(_) => return false,
            },
            _ => None,
        };
        let dict = match read.as_deref().unwrap_or(root) {
            Object::Dictionary(d) => Some(d),
            Object::Stream(s) => Some(&s.dict),
            other => {
                refs(other, &mut |r| next.push(r));
                None
            }
        };
        for (_, v) in
            (dict.into_iter().flat_map(|d| d.iter())).filter(|(k, _)| !own.contains(&k.as_slice()))
        {
            refs(v, &mut |r| next.push(r));
        }
        while let Some(r) = next.pop() {
            if levels.contains_key(&r) || !seen.insert(r) {
                continue;
            }
            let Ok(obj) = pdf.object_unkept(r) else {
                continue;
            };
            let kind = match &*obj {
                Object::Dictionary(d) => d.get(b"Type").and_then(Object::as_name).ok(),
                _ => None,
            };
            match kind {
                Some(b"Page") => {
                    if hit(self.rl, blob, r, copy) {
                        return true;
                    }
                    continue;
                }
                Some(b"Pages" | b"Catalog") => continue,
                _ => refs(&obj, &mut |r| next.push(r)),
            }
        }
        false
    }
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
        // A replacement is listed with its StrikeOut: one that came or
        // went changes its page's `/Annots`.
        let replaces = |st: &DocState| {
            (st.annot(a).and_then(AnnotRec::mark))
                .is_some_and(|m| matches!(m.kind, AnnotKind::Replace { .. }))
        };
        let listed = replaces(self.was) != replaces(self.now);
        let mut names = HashSet::new();
        let mut pages = HashSet::new();
        let mut groups = Vec::new();
        for rec in self.states().into_iter().filter_map(|st| st.annot(a)) {
            if listed {
                self.keys.insert(ObjKey::Page(rec.page));
            }
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
    /// is the next of its group in order. Once for each group: a page
    /// turned, or a connector, of a group already taken adds nothing.
    fn connectors(&mut self, label: Option<&str>) {
        let done = match label {
            None => std::mem::replace(&mut self.every_connector, true),
            Some(l) => self.every_connector || !self.connected.insert(l.to_owned()),
        };
        if done {
            return;
        }
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

    /// The connectors of each group with one on page `p` now, whose turn
    /// or boxes changed: a group goes in the order its connectors show in
    /// on their pages, so each one's next may be another
    /// ([`connectors`](Self::connectors)).
    fn connectors_on(&mut self, p: PageId) {
        let labels: BTreeSet<String> = (self.now.annots_on(p))
            .filter_map(|(_, rec)| match rec.mark().map(|m| &m.kind) {
                Some(AnnotKind::Connector { label, .. }) => Some(label.clone()),
                _ => None,
            })
            .collect();
        for label in labels {
            self.connectors(Some(&label));
        }
    }

    /// What a watermark that was `then` and is `after` changes: what the
    /// generators are, the catalog when the first came or the last went,
    /// and the pages it's drawn on then or now. Only those it came onto or
    /// went from when nothing else of it changed: on the others it draws
    /// what it drew, which only their boxes change (`Geom`).
    fn watermark(&mut self, then: Option<&Generator>, after: Option<&Generator>) {
        self.keys.insert(ObjKey::Generators);
        let any = |st: &DocState| (st.generators().values()).any(|g| g.spec.watermark().is_some());
        if any(self.was) != any(self.now) {
            self.keys.insert(ObjKey::Catalog);
        }
        let (was, now) = (self.was, self.now);
        let covers = |g: Option<&Generator>, st: &DocState, p: PageId| {
            g.is_some_and(|g| st.def(p).is_some() && g.covers(p))
        };
        let drawn =
            (then.and_then(|g| g.spec.watermark())).zip(after.and_then(|g| g.spec.watermark()));
        let rescoped = match drawn {
            Some((a, b)) => {
                let a = WatermarkGen {
                    scope: b.scope.clone(),
                    ..WatermarkGen::clone(a)
                };
                a == **b
            }
            None => false,
        };
        let pages: Vec<PageId> = (now.pages())
            .map(|(p, _, _)| p)
            .filter(|&p| match rescoped {
                true => covers(then, was, p) != covers(after, now, p),
                false => covers(then, was, p) || covers(after, now, p),
            })
            .collect();
        self.keys.extend(pages.into_iter().map(ObjKey::Page));
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
        let primary = input.primary.id();
        let others = |st: &DocState| -> BTreeSet<BlobId> {
            let held = (st.form().names.keys()).chain(st.fields().map(|(_, node)| &node.inst));
            held.map(|inst| inst.src)
                .filter(|&src| src != primary)
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
    /// as now ([`emit::written_pages`]), or that's written as a page of
    /// `renumbered`, and the page it's on: its destination is that page as
    /// written, or null. And the sources of such pages, whose objects the
    /// file has (a link's action, a destination of its own) may name one
    /// too: they're copied again.
    fn links(&mut self, input: &SaveInput, renumbered: &BTreeSet<PageId>) {
        let shown = emit::shown(self.now, &input.derived);
        let then = emit::written_pages(self.was, &emit::shown(self.was, &input.derived));
        let now = emit::written_pages(self.now, &shown);
        let gone: HashSet<(BlobId, ObjectId)> = (then.iter().chain(&now))
            .filter(|&(at, page)| {
                then.get(at) != Some(page) || now.get(at) != Some(page) || renumbered.contains(page)
            })
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
        self.relinking = Some(Relinking {
            primary: input.primary.id(),
            then,
            now,
            gone,
            renumbered: renumbered.clone(),
            shown,
        });
    }

    /// What the saved file has that names a page a copy's reference to it
    /// names another page by now, or none, and that the section doesn't
    /// write again, with every key of it in ([`Closure::stale`]): its
    /// holder's key. Each holder is read as the emitter reads it: a page's
    /// own keys as copy 0 of its page; what its `/Annots` has that no
    /// record holds as the page's copy; another tool's annotation, a
    /// widget and a field as theirs, and a mark of sheaf's read from a
    /// file as copy 0 of its object. What's written again names the page
    /// as it's written now, and what it reaches of its source is copied
    /// again, but from the primary's copy 0, whose references are numbers.
    /// A link a page written again leaves out, leading nowhere now, names
    /// nothing. Last, the primary's catalog, as copy 0, but the keys a save
    /// writes from the state: the catalog's key when it names a page of
    /// the primary that's no page's copy 0 now ([`Relinking::went`]). Only
    /// what's read as a copy whose references move is read.
    fn stale(&self, input: &SaveInput) -> Option<ObjKey> {
        let rl = self.relinking.as_ref()?;
        let now = self.now;
        let mut walk = Walk {
            rl,
            moves: HashMap::new(),
            levels: HashMap::new(),
            seen: HashMap::new(),
        };
        let written = |key: ObjKey| self.keys.contains(&key);
        // Named as it's written now: written again, and not the primary's
        // copy 0.
        let anew =
            |again: bool, blob: BlobId, copy: u32| again && !(blob == rl.primary && copy == 0);
        for (id, _, _) in now.pages() {
            let Some(shown) = rl.shown.get(&id) else {
                continue;
            };
            let src = &shown.src;
            let Some(facts) = src.page(shown.index) else {
                continue;
            };
            let page = ObjKey::Page(id);
            let again = written(page);
            if !anew(again, src.id(), 0)
                && walk.reaches(src, 0, &Object::Reference(facts.id), PAGE_OWN)
            {
                return Some(page);
            }
            // Its `/Annots`' entries no record holds, as annotations'
            // `list` keeps them: not one read as a record that's gone, nor
            // one of sheaf's old marks, nor a pop-up.
            if !walk.moves(src.id(), shown.copy) {
                continue;
            }
            let base = annots::base_annots(shown);
            let mut held = HashSet::new();
            let mut slots = HashSet::new();
            for (_, rec) in now.annots_on(id) {
                let (r, slot) = match &rec.body {
                    AnnotBody::Mark(m) => (m.src.as_ref(), None),
                    AnnotBody::Foreign(f) => (f.src.as_ref(), Some(f.summary.slot)),
                    AnnotBody::Widget(w) => (w.src.as_ref(), None),
                };
                match r {
                    Some(r) if r.src == *src => {
                        held.insert(r.id);
                    }
                    Some(_) => {}
                    None => slots.extend(slot),
                }
            }
            let facts = src.annots();
            let read: HashSet<usize> = facts.on(shown.index).iter().map(|a| a.slot).collect();
            let entries = crate::file::PageAnnots::new(&base);
            for (slot, entry) in base.iter().enumerate() {
                let by_record = match entry {
                    Object::Reference(r) => held.contains(r),
                    _ => slots.contains(&slot),
                };
                if by_record
                    || read.contains(&slot)
                    || crate::file::is_ours(src.pdf(), entry, &entries)
                {
                    continue;
                }
                let popup = (src.pdf().dereference(entry).ok())
                    .and_then(|(_, d)| d.as_dict().ok())
                    .is_some_and(|d| annots::is(d, b"Subtype", b"Popup"));
                // Written again with its page: one written straight into
                // `/Annots`, and one of its own the file has, as what the
                // file has of its source is copied again.
                let key = match entry {
                    Object::Reference(r) => ObjKey::Base {
                        blob: src.id(),
                        obj: *r,
                        copy: shown.copy,
                    },
                    _ => page,
                };
                if !popup
                    && !anew(again, src.id(), shown.copy)
                    && walk.reaches(src, shown.copy, entry, ANNOT_OWN)
                {
                    return Some(key);
                }
            }
        }
        for (a, rec) in now.annots() {
            let key = ObjKey::Annot(a);
            match &rec.body {
                AnnotBody::Foreign(f) => {
                    let Some(shown) = rl.shown.get(&rec.page) else {
                        continue;
                    };
                    let src = f.src.as_ref().map_or(&shown.src, |r| &r.src);
                    if !walk.moves(src.id(), f.copy) {
                        continue;
                    }
                    let page_again = written(ObjKey::Page(rec.page));
                    let (root, again) = match &f.src {
                        Some(r) => (Object::Reference(r.id), written(key)),
                        None => match annots::base_annots(shown).get(f.summary.slot) {
                            Some(d @ Object::Dictionary(_)) => (d.clone(), page_again),
                            _ => continue,
                        },
                    };
                    // A link that leads nowhere now is left out of its page:
                    // gone with it when that's written again, else the
                    // file's as it was.
                    let pdf = src.pdf();
                    let dict = match &root {
                        Object::Reference(r) => pdf.get_dictionary(*r).ok(),
                        d => d.as_dict().ok(),
                    };
                    let nowhere = f.summary.subtype == "Link"
                        && dict.is_some_and(|d| {
                            annots::leads_nowhere(pdf, d, |page| rl.writes(src.id(), page, f.copy))
                        });
                    let again = match nowhere {
                        true if page_again => continue,
                        true => false,
                        false => again,
                    };
                    if !anew(again, src.id(), f.copy) && walk.reaches(src, f.copy, &root, ANNOT_OWN)
                    {
                        return Some(key);
                    }
                }
                AnnotBody::Widget(w) => {
                    let Some(r) = &w.src else {
                        continue;
                    };
                    let copy = w.inst.copy;
                    // One its field's object is, written with it.
                    let merged = merged_widget(now, w.field) == Some(a);
                    let again = written(key) || (merged && written(ObjKey::Field(w.field)));
                    if !anew(again, r.src.id(), copy)
                        && walk.reaches(&r.src, copy, &Object::Reference(r.id), ANNOT_OWN)
                    {
                        return Some(key);
                    }
                }
                // sheaf's own, built from its record, but one the file has
                // as it was read, copy 0 of its object.
                AnnotBody::Mark(m) => {
                    let Some(r) = &m.src else {
                        continue;
                    };
                    if !anew(written(key), r.src.id(), 0)
                        && walk.reaches(&r.src, 0, &Object::Reference(r.id), ANNOT_OWN)
                    {
                        return Some(key);
                    }
                }
            }
        }
        for (f, node) in now.fields() {
            let key = ObjKey::Field(f);
            let Some(r) = &node.src else {
                continue;
            };
            let copy = node.inst.copy;
            if !anew(written(key), r.src.id(), copy)
                && walk.reaches(&r.src, copy, &Object::Reference(r.id), FIELD_OWN)
            {
                return Some(key);
            }
        }
        // The primary's document-level objects, read as its copy 0: what
        // its catalog holds but the keys a save writes from the state (an
        // `/OpenAction`, `/AA`, the name dictionary's and the catalog's
        // `/Dests`, the structure tree's `/Pg`, an article's beads). An
        // Append keeps them as the file has them, by number, where a
        // Rewrite's `keep_only` prunes them against the pages it keeps: one
        // that names a page that's no page's copy 0 now names a page
        // outside the tree (a page deleted whose duplicate stays, which
        // removes nothing, so the plan appends).
        let src = &input.primary;
        if walk.moves(src.id(), 0)
            && let Ok(catalog) = src.pdf().catalog()
        {
            let catalog = catalog.clone();
            for (_, v) in (catalog.iter()).filter(|(k, _)| !CATALOG_OWNED.contains(&k.as_slice())) {
                if walk.reaches_by(src, 0, v, &[], Relinking::went) {
                    return Some(ObjKey::Catalog);
                }
            }
        }
        None
    }
}

/// Whether `g` is a watermark or background.
fn is_watermark(g: &Option<std::sync::Arc<Generator>>) -> bool {
    g.as_ref().is_some_and(|g| g.spec.watermark().is_some())
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
