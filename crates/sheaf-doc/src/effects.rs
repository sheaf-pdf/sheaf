//! What a change means for saving (M10.5 §7). [`classify`], the one place
//! that knows, judges one slot's change as [`Effects`]: flags the save
//! planner reads, and the classes of change a signature may allow
//! ([`SigClass`]). [`judge`] judges a save's path, the slots changed on
//! the way from the saved version to the one being saved, by their
//! values: an undo back to the saved state, by whatever route, changes
//! nothing, and replaced text counts wherever it came from.
//!
//! Each step keeps what it did ([`StepEffects`]): its effects, judged
//! against the state its op found, and the pages and slots it touched,
//! for the History panel's badge and, from slices 12–13, previews,
//! approvals and the realizer.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fmt;
use std::ops::{BitOr, BitOrAssign};
use std::sync::{Arc, OnceLock};

use bitflags::bitflags;
use im::Vector;
use sheaf_edit::Z;

use crate::annot::{Annot, AnnotId, AnnotKind};
use crate::content::{GenId, PagOrigin, PaginationKind};
use crate::forms::{FieldId, Value};
use crate::labels::LabelSpec;
use crate::model::{
    AnnotBody, AnnotRec, Bookmark, CtxNeeds, DocState, FieldNode, Filter, Foreign, GenSpec,
    Generator, Layer, PageDef, PageId, Scope, Static, Terminal, WatermarkContent, Widget,
};
use crate::prim::{Key, Prim, Val, same};
use crate::store::BlobId;

bitflags! {
    /// What a change does, for saving (§7.1).
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
    pub struct Flags: u32 {
        const PAGE_ADD = 1 << 0;
        const PAGE_REMOVE = 1 << 1;
        const PAGE_ORDER = 1 << 2;
        const GEOMETRY = 1 << 3;
        /// What's drawn on a page: a layer or a generator's output added
        /// or changed.
        const CONTENT = 1 << 4;
        /// Content that existed is gone: a page no page shows now, a
        /// comment, a field, a bookmark, a layer.
        const REMOVES = 1 << 5;
        /// Text replaced or cleared: a comment's contents, struck text,
        /// tape rows or author, a field's name or a value it held, a
        /// bookmark's title, a label prefix, OCR words, a header's text.
        const REPLACES_TEXT = 1 << 6;
        /// A generator's live tokens laid out again under the same recipe:
        /// a reorder or a label change under a header that numbers pages.
        /// It appends.
        const LIVE_REDRAW = 1 << 7;
        const ANNOTS = 1 << 8;
        const FIELD_VALUES = 1 << 9;
        const FIELD_TREE = 1 << 10;
        const FORM = 1 << 11;
        const OUTLINE = 1 << 12;
        const LABELS = 1 << 13;
    }
}

bitflags! {
    /// The classes of change a signature may allow after it (§9.3): a set,
    /// not a ladder, so DocMDP's and usage rights' permissions are masks
    /// over it. M12 adds signing and LTV.
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
    pub struct SigClass: u8 {
        /// Filling in fields.
        const FILL = 1 << 0;
        /// Comments: annotations added, changed or deleted.
        const ANNOT = 1 << 1;
        /// Anything else.
        const OTHER = 1 << 2;
    }
}

bitflags! {
    /// What a change does to annotations, as Reader's usage rights grant
    /// it (`/Annots`'s `/Create`, `/Modify` and `/Delete`), which
    /// [`SigClass::ANNOT`] doesn't say.
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
    pub struct AnnotOps: u8 {
        const CREATE = 1 << 0;
        const MODIFY = 1 << 1;
        const DELETE = 1 << 2;
    }
}

/// What one change means.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Effects {
    pub flags: Flags,
    pub sig: SigClass,
}

impl Effects {
    /// What a change to the same value means: nothing.
    pub const NONE: Effects = Effects::of(Flags::empty(), SigClass::empty());

    const fn of(flags: Flags, sig: SigClass) -> Effects {
        Effects { flags, sig }
    }
}

impl BitOr for Effects {
    type Output = Effects;

    fn bitor(self, other: Effects) -> Effects {
        Effects::of(self.flags | other.flags, self.sig | other.sig)
    }
}

impl BitOrAssign for Effects {
    fn bitor_assign(&mut self, other: Effects) {
        *self = *self | other;
    }
}

/// What one step did (§7.1): its effects, judged against the state its op
/// found ([`ClassifyCx::of_step`]), and the pages and slots it touched.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StepEffects {
    pub effects: Effects,
    /// The pages whose content, boxes or annotations it changed, and those
    /// it added or removed.
    pub pages: Arc<BTreeSet<PageId>>,
    pub keys: Arc<BTreeSet<Key>>,
}

impl StepEffects {
    /// What `prims`, which made `after` of `before`, did.
    pub fn of(before: &DocState, after: &DocState, prims: &[Prim]) -> StepEffects {
        let cx = ClassifyCx::of_step(before, after);
        let mut effects = Effects::NONE;
        let mut pages = Vec::new();
        for p in prims {
            effects |= classify(&p.key, p.before.as_ref(), p.after.as_ref(), &cx);
            match p.key {
                Key::Page(page) | Key::Geom(page) | Key::PageAnnots(page) => pages.push(page),
                Key::Annot(_) => {
                    let page = |v: &Option<Val>| match v {
                        Some(Val::Annot(rec)) => Some(rec.page),
                        _ => None,
                    };
                    pages.extend(page(&p.before));
                    pages.extend(page(&p.after));
                }
                Key::Order
                | Key::Field(_)
                | Key::Form
                | Key::Bookmark(_)
                | Key::OutlineRoots
                | Key::Labels
                | Key::Generator(_) => {}
            }
        }
        StepEffects {
            effects,
            pages: Arc::new(pages.into_iter().collect()),
            keys: Arc::new(prims.iter().map(|p| p.key).collect()),
        }
    }

    /// What `steps` did together.
    pub fn union(steps: &[&StepEffects]) -> StepEffects {
        if let [one] = steps {
            return StepEffects::clone(one);
        }
        let mut effects = Effects::NONE;
        let mut pages = BTreeSet::new();
        let mut keys = BTreeSet::new();
        for s in steps {
            effects |= s.effects;
            pages.extend(s.pages.iter().copied());
            keys.extend(s.keys.iter().copied());
        }
        StepEffects {
            effects,
            pages: Arc::new(pages),
            keys: Arc::new(keys),
        }
    }
}

/// A value a slot can't hold, which history never makes: judged as
/// everything, so nothing is missed.
const MISFIT: Effects = Effects::of(Flags::all(), SigClass::all());

/// How many of a kind of thing a save's path removed or replaced: what an
/// Append would leave readable in the earlier revision (§9.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Removed {
    pub kind: RemovedKind,
    pub count: usize,
}

/// What a path removed or replaced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RemovedKind {
    /// Pages whose source page no page shows now.
    Pages,
    /// Sheaf's marks, other tools' comments and links.
    Comments,
    /// Where fields showed.
    Widgets,
    /// Nodes of the field tree.
    Fields,
    Bookmarks,
    /// Recognized text, removed or replaced.
    OcrLayers,
    /// Comments whose text was replaced.
    CommentText,
    /// Fields whose value or default was replaced or cleared.
    FieldValues,
    /// Fields whose name, tooltip, items or scripts were replaced.
    FieldText,
    BookmarkTitles,
    /// Label prefixes no range has now.
    LabelPrefixes,
    /// A header and footer drawn with other text, or no longer drawn.
    HeaderFooter,
    /// A watermark drawing another text, image or page, on fewer pages, or
    /// no longer drawn; `text`, when it drew text.
    Watermark {
        text: bool,
    },
    /// Likewise a background.
    Background {
        text: bool,
    },
    /// Pages a filter, or a generator's `replaces`, takes others'
    /// watermarks or backgrounds off: another file's, another program's
    /// (§3.4, §11.4).
    Filtered,
    /// Pages a filter takes the settings an older sheaf kept out of, an
    /// image's path among them, what they drew kept (§15's `StripLegacy`).
    OlderSettings,
    /// The form's XFA part, which a save drops (§11.8): one.
    XfaDatasets,
    /// What a recovery sidecar's sections removed or replaced (§11.10),
    /// which its flags say, but not what: one.
    Recovered,
    /// Text the removal check found the save takes out of the file
    /// (§11.9), which the save's path didn't say: one.
    Found,
}

impl RemovedKind {
    /// Whether it's text replaced or cleared, rather than something gone
    /// whole: what a path's `REPLACES_TEXT` counts, and the rest its
    /// `REMOVES`.
    pub fn is_text(self) -> bool {
        use RemovedKind as K;
        match self {
            K::Pages
            | K::Comments
            | K::Widgets
            | K::Fields
            | K::Bookmarks
            | K::Filtered
            | K::OlderSettings
            | K::XfaDatasets
            | K::Recovered
            | K::Found => false,
            K::OcrLayers
            | K::CommentText
            | K::FieldValues
            | K::FieldText
            | K::BookmarkTitles
            | K::LabelPrefixes
            | K::HeaderFooter => true,
            K::Watermark { text } | K::Background { text } => text,
        }
    }
}

impl fmt::Display for Removed {
    /// How many, of what, for the reader: "2 comments".
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use RemovedKind as K;
        let (one, many) = match self.kind {
            K::Pages => ("page", "pages"),
            K::Comments => ("comment", "comments"),
            K::Widgets => ("form field widget", "form field widgets"),
            K::Fields => ("form field", "form fields"),
            K::Bookmarks => ("bookmark", "bookmarks"),
            K::OcrLayers => ("page's recognized text", "pages' recognized text"),
            K::CommentText => ("comment's text", "comments' text"),
            K::FieldValues => ("form field's value", "form fields' values"),
            K::FieldText => ("form field's name or text", "form fields' names or text"),
            K::BookmarkTitles => ("bookmark's title", "bookmarks' titles"),
            K::LabelPrefixes => ("page label prefix", "page label prefixes"),
            K::HeaderFooter => ("header and footer", "headers and footers"),
            K::Watermark { .. } => ("watermark", "watermarks"),
            K::Background { .. } => ("background", "backgrounds"),
            K::Filtered => (
                "page's watermark or background from another program or file",
                "pages' watermarks or backgrounds from other programs or files",
            ),
            K::OlderSettings => (
                "page's watermark settings from an older sheaf",
                "pages' watermark settings from an older sheaf",
            ),
            K::XfaDatasets => ("form's XFA part", "forms' XFA parts"),
            K::Recovered => return f.write_str("what the recovered changes removed"),
            K::Found => return f.write_str("removed text the removal check found"),
        };
        let noun = if self.count == 1 { one } else { many };
        write!(f, "{} {noun}", self.count)
    }
}

/// A save's path judged (§7.2): what changed between the saved state and
/// the one being saved, by value.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PathEffects {
    pub flags: Flags,
    pub sig: SigClass,
    /// What an Append would leave readable, by kind.
    pub removed: Vec<Removed>,
    /// What it does to annotations, for Reader's usage rights.
    pub annots: AnnotOps,
    /// The full names of the fields it touches, in the saved state and
    /// now, for FieldMDP: a field's own, every node's under one renamed,
    /// moved, added or removed, and a changed widget's field's.
    pub fields: BTreeSet<String>,
    /// The slots whose values differ: none when the state being saved is
    /// the saved one.
    pub changed: BTreeSet<Key>,
    /// The slots whose change removes or replaces text, each with which
    /// (`REMOVES`, `REPLACES_TEXT`): what `removed` counts, slot by slot.
    /// What the removal check tells an Append anyway's listed removals by
    /// from what the planner missed (§11.9).
    pub removing: BTreeMap<Key, Flags>,
}

/// What [`classify`] judges a change against: the saved file's state, and
/// the state being saved, and through their pages' sources the blocks
/// those pages hold, which a save's path reads for what a watermark
/// replaces and a step never does.
pub struct ClassifyCx<'a> {
    pub saved: &'a DocState,
    pub target: &'a DocState,
    /// What the target's pages show, found the first time a page's change
    /// asks.
    shown: OnceLock<Shown>,
    /// It judges one step ([`of_step`](Self::of_step)).
    step: bool,
}

/// What a state's pages show: their source pages, and their recognized
/// text by its `Arc`.
struct Shown {
    pages: HashSet<(BlobId, u32)>,
    ocr: HashSet<usize>,
}

impl<'a> ClassifyCx<'a> {
    pub fn new(saved: &'a DocState, target: &'a DocState) -> ClassifyCx<'a> {
        ClassifyCx {
            saved,
            target,
            shown: OnceLock::new(),
            step: false,
        }
    }

    /// What a step judges its change against: the state its op found, and
    /// the one it made, looking at nothing the step didn't touch. A page
    /// or a layer it took away is removed, though another page may show
    /// the same, and the pages it added or removed are its `Page` slots'
    /// (an op that adds or removes a page puts its `Page`): what a step
    /// did, judged in O(what it touched), not what a save of it would
    /// leave out.
    pub fn of_step(before: &'a DocState, after: &'a DocState) -> ClassifyCx<'a> {
        let shown = Shown {
            pages: HashSet::new(),
            ocr: HashSet::new(),
        };
        ClassifyCx {
            saved: before,
            target: after,
            shown: OnceLock::from(shown),
            step: true,
        }
    }

    fn shown(&self) -> &Shown {
        self.shown.get_or_init(|| {
            let defs = || self.target.pages().map(|(_, def, _)| def);
            Shown {
                pages: defs()
                    .map(|def| (def.base.src.id(), def.base.index))
                    .collect(),
                ocr: (defs().flat_map(|def| def.content.over.iter()))
                    .filter_map(|l| l.body.ocr_text().map(addr))
                    .collect(),
            }
        })
    }

    /// The annotation `id`, now or in the saved state.
    fn annot(&self, id: AnnotId) -> Option<&AnnotRec> {
        self.target.annot(id).or_else(|| self.saved.annot(id))
    }
}

/// Where an `Arc`'s value is: the same layer wherever a page shows it.
fn addr<T>(arc: &Arc<T>) -> usize {
    Arc::as_ptr(arc).addr()
}

/// What changing `key` from `before` to `after` means (§7.1). Pure, and
/// total over keys: a change to the same value (`prim::same`) means
/// nothing, and any other means something.
pub fn classify(key: &Key, before: Option<&Val>, after: Option<&Val>, cx: &ClassifyCx) -> Effects {
    if same(before, after) {
        return Effects::NONE;
    }
    effects(key, before, after, cx, &mut Found::default())
}

/// `keys`, a save's path, judged against `saved`, the saved file's state,
/// with `target` the state being saved: each key classified once, by its
/// values in the two (§7.2).
pub fn judge(saved: &DocState, target: &DocState, keys: &BTreeSet<Key>) -> PathEffects {
    let cx = ClassifyCx::new(saved, target);
    let mut found = Found::default();
    let mut path = PathEffects::default();
    for key in keys {
        let (before, after) = (saved.get(key), target.get(key));
        let (before, after) = (before.as_ref(), after.as_ref());
        if same(before, after) {
            continue;
        }
        let e = effects(key, before, after, &cx, &mut found);
        path.flags |= e.flags;
        path.sig |= e.sig;
        path.changed.insert(*key);
        let text = e.flags & (Flags::REMOVES | Flags::REPLACES_TEXT);
        if !text.is_empty() {
            path.removing.insert(*key, text);
        }
        touched_fields(key, before, after, &cx, &mut path.fields);
    }
    path.removed = (found.removed.into_iter())
        .map(|(kind, count)| Removed { kind, count })
        .collect();
    path.annots = found.ops;
    #[cfg(test)]
    if let Some(wrong) = MISJUDGE.get() {
        wrong(&mut path);
    }
    path
}

#[cfg(test)]
thread_local! {
    static MISJUDGE: std::cell::Cell<Option<fn(&mut PathEffects)>> =
        const { std::cell::Cell::new(None) };
}

/// For tests: `run`, with every path [`judge`] judges on this thread then
/// misjudged as `wrong` makes it, a misclassification for what comes after
/// the planner (the removal check, the section's own checks) to catch.
#[cfg(test)]
pub(crate) fn misjudged<R>(wrong: fn(&mut PathEffects), run: impl FnOnce() -> R) -> R {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            MISJUDGE.set(None);
        }
    }
    MISJUDGE.set(Some(wrong));
    let _reset = Reset;
    run()
}

/// What judging changes found beside their effects.
#[derive(Default)]
struct Found {
    removed: BTreeMap<RemovedKind, usize>,
    ops: AnnotOps,
}

impl Found {
    fn removed(&mut self, kind: RemovedKind, n: usize) {
        *self.removed.entry(kind).or_default() += n;
    }
}

/// A value a slot can't hold.
struct Misfit;

/// Each slot's values as what it holds: Err for a value it can't hold.
macro_rules! held {
    ($name:ident, $variant:ident, $t:ty) => {
        fn $name(v: Option<&Val>) -> Result<Option<&$t>, Misfit> {
            match v {
                None => Ok(None),
                Some(Val::$variant(x)) => Ok(Some(x)),
                Some(_) => Err(Misfit),
            }
        }
    };
}

held!(orders, Order, Vector<PageId>);
held!(pages, Page, Arc<PageDef>);
held!(bookmarks, Bookmark, Arc<Bookmark>);
held!(annot_lists, PageAnnots, Vector<AnnotId>);
held!(annots, Annot, AnnotRec);
held!(fields, Field, Arc<FieldNode>);
held!(labels, Labels, Arc<[LabelSpec]>);
held!(generators, Generator, Arc<Generator>);

/// [`classify`] of values that differ, what it found noted in `found`.
fn effects(
    key: &Key,
    before: Option<&Val>,
    after: Option<&Val>,
    cx: &ClassifyCx,
    found: &mut Found,
) -> Effects {
    judged(key, before, after, cx, found).unwrap_or(MISFIT)
}

fn judged(
    key: &Key,
    before: Option<&Val>,
    after: Option<&Val>,
    cx: &ClassifyCx,
    found: &mut Found,
) -> Result<Effects, Misfit> {
    Ok(match key {
        Key::Order => match (orders(before)?, orders(after)?) {
            (Some(_), Some(_)) if cx.step => {
                Effects::of(Flags::PAGE_ORDER, SigClass::OTHER) | redrawn(cx, CtxNeeds::order)
            }
            (Some(a), Some(b)) => order(a, b) | redrawn(cx, CtxNeeds::order),
            _ => return Err(Misfit),
        },
        Key::Page(_) => page(pages(before)?, pages(after)?, cx, found),
        Key::Geom(_) => Effects::of(Flags::GEOMETRY, SigClass::OTHER),
        Key::Bookmark(_) => bookmark(bookmarks(before)?, bookmarks(after)?, found),
        Key::OutlineRoots => Effects::of(Flags::OUTLINE, SigClass::OTHER),
        Key::PageAnnots(_) => page_annots(annot_lists(before)?, annot_lists(after)?, cx, found),
        Key::Annot(_) => annot(annots(before)?, annots(after)?, found),
        Key::Field(_) => field(fields(before)?, fields(after)?, found),
        Key::Form => Effects::of(Flags::FORM, SigClass::OTHER),
        Key::Labels => {
            page_labels(labels(before)?, labels(after)?, found) | redrawn(cx, |n| n.label)
        }
        Key::Generator(_) => generator(generators(before)?, generators(after)?, cx, found),
    })
}

/// A generator's live tokens laid out again (§3.5): `LIVE_REDRAW` when a
/// generator of the target's reads, of where its pages are, what `reads`
/// says changed. It appends: only where a page is changes what it draws.
fn redrawn(cx: &ClassifyCx, reads: fn(CtxNeeds) -> bool) -> Effects {
    match (cx.target.generators().values()).any(|g| reads(g.needs())) {
        true => Effects::of(Flags::LIVE_REDRAW, SigClass::OTHER),
        false => Effects::NONE,
    }
}

/// A new order: pages added and removed, by its difference from the old.
fn order(a: &Vector<PageId>, b: &Vector<PageId>) -> Effects {
    let was: HashSet<PageId> = a.iter().copied().collect();
    let now: HashSet<PageId> = b.iter().copied().collect();
    let mut flags = Flags::PAGE_ORDER;
    flags.set(Flags::PAGE_ADD, b.iter().any(|p| !was.contains(p)));
    flags.set(Flags::PAGE_REMOVE, a.iter().any(|p| !now.contains(p)));
    Effects::of(flags, SigClass::OTHER)
}

/// A page added, removed or showing something else. Its source page is
/// removed when no page shows it now, and a layer it had when no page
/// shows that either: recognized text so removed or replaced is replaced
/// text.
fn page(
    a: Option<&Arc<PageDef>>,
    b: Option<&Arc<PageDef>>,
    cx: &ClassifyCx,
    found: &mut Found,
) -> Effects {
    let mut flags = match (a, b) {
        (None, None) => Flags::empty(),
        (None, Some(_)) => Flags::PAGE_ADD,
        (Some(_), None) => Flags::PAGE_REMOVE,
        (Some(a), Some(b)) if a.base != b.base || a.copy != b.copy => Flags::CONTENT,
        (Some(_), Some(_)) => Flags::empty(),
    };
    if let Some(a) = a
        && b.is_none_or(|b| a.base != b.base)
        && !(cx.shown().pages).contains(&(a.base.src.id(), a.base.index))
    {
        flags |= Flags::REMOVES;
        found.removed(RemovedKind::Pages, 1);
    }
    let none = Vector::new();
    let was = a.map_or(&none, |def| &def.content.over);
    let now = b.map_or(&none, |def| &def.content.over);
    if was != now {
        flags |= Flags::CONTENT;
    }
    // A filter added takes out what the page drew, or what an older sheaf
    // kept in it; one gone puts it back.
    let unfiltered = Vector::new();
    let had = (a.filter(|a| b.is_some_and(|b| a.base == b.base)))
        .map_or(&unfiltered, |def| &def.content.filters);
    let has = b.map_or(&unfiltered, |def| &def.content.filters);
    if had != has {
        flags |= Flags::CONTENT;
    }
    let added = || has.iter().filter(|f| !had.contains(f));
    if added().any(|f| !matches!(f, Filter::StripLegacy(_))) {
        flags |= Flags::REMOVES;
        found.removed(RemovedKind::Filtered, 1);
    }
    if added().any(|f| matches!(f, Filter::StripLegacy(_))) {
        flags |= Flags::REMOVES;
        found.removed(RemovedKind::OlderSettings, 1);
    }
    let shown = |l: &Layer| match &l.body {
        Static::OcrText(text) => cx.shown().ocr.contains(&addr(text)),
    };
    for l in was.iter().filter(|l| !now.contains(l) && !shown(l)) {
        flags |= Flags::REMOVES;
        if l.body.ocr_text().is_some() {
            flags |= Flags::REPLACES_TEXT;
            found.removed(RemovedKind::OcrLayers, 1);
        }
    }
    Effects::of(flags, SigClass::OTHER)
}

/// A bookmark added, removed, or changed: a title replaced is replaced
/// text.
fn bookmark(a: Option<&Arc<Bookmark>>, b: Option<&Arc<Bookmark>>, found: &mut Found) -> Effects {
    let flags = match (a, b) {
        (Some(_), None) => {
            found.removed(RemovedKind::Bookmarks, 1);
            Flags::OUTLINE | Flags::REMOVES
        }
        (Some(a), Some(b)) if !a.title.is_empty() && a.title != b.title => {
            found.removed(RemovedKind::BookmarkTitles, 1);
            Flags::OUTLINE | Flags::REPLACES_TEXT
        }
        _ => Flags::OUTLINE,
    };
    Effects::of(flags, SigClass::OTHER)
}

/// A page's annotations in another order or with others in it: widgets'
/// are the form's tree, the rest comments. The same comments in another
/// order are modified.
fn page_annots(
    a: Option<&Vector<AnnotId>>,
    b: Option<&Vector<AnnotId>>,
    cx: &ClassifyCx,
    found: &mut Found,
) -> Effects {
    let none = Vector::new();
    let (a, b) = (a.unwrap_or(&none), b.unwrap_or(&none));
    let widget = |id: &AnnotId| cx.annot(*id).is_some_and(|rec| rec.widget().is_some());
    let split = |list: &Vector<AnnotId>| -> (Vec<AnnotId>, Vec<AnnotId>) {
        list.iter().copied().partition(|id| widget(id))
    };
    let ((wa, oa), (wb, ob)) = (split(a), split(b));
    let mut e = Effects::NONE;
    if wa != wb {
        e |= Effects::of(Flags::FIELD_TREE, SigClass::OTHER);
    }
    // Comments that moved among widgets are in another order too.
    if oa != ob || e == Effects::NONE {
        e |= Effects::of(Flags::ANNOTS, SigClass::ANNOT);
        let sorted = |mut l: Vec<AnnotId>| {
            l.sort();
            l
        };
        if sorted(oa) == sorted(ob) {
            found.ops |= AnnotOps::MODIFY;
        }
    }
    e
}

/// An annotation added, removed or changed. A comment that says something
/// else, its text or anything it holds, is replaced text; geometry and
/// style alone aren't. A widget's state is filling in its field; the rest
/// of it is the form's tree.
fn annot(a: Option<&AnnotRec>, b: Option<&AnnotRec>, found: &mut Found) -> Effects {
    let comment = Effects::of(Flags::ANNOTS, SigClass::ANNOT);
    let tree = Effects::of(Flags::FIELD_TREE, SigClass::OTHER);
    let removes = Effects::of(Flags::REMOVES, SigClass::empty());
    match (a, b) {
        (None, None) => Effects::NONE,
        (None, Some(rec)) => match rec.body {
            AnnotBody::Widget(_) => tree,
            AnnotBody::Mark(_) | AnnotBody::Foreign(_) => {
                found.ops |= AnnotOps::CREATE;
                comment
            }
        },
        (Some(rec), None) => match rec.body {
            AnnotBody::Widget(_) => {
                found.removed(RemovedKind::Widgets, 1);
                tree | removes
            }
            AnnotBody::Mark(_) | AnnotBody::Foreign(_) => {
                found.removed(RemovedKind::Comments, 1);
                found.ops |= AnnotOps::DELETE;
                comment | removes
            }
        },
        (Some(x), Some(y)) => match (&x.body, &y.body) {
            (AnnotBody::Widget(a), AnnotBody::Widget(b)) => widget(x.page != y.page, a, b),
            (AnnotBody::Widget(_), _) | (_, AnnotBody::Widget(_)) => {
                let kind = match x.body {
                    AnnotBody::Widget(_) => RemovedKind::Widgets,
                    AnnotBody::Mark(_) | AnnotBody::Foreign(_) => RemovedKind::Comments,
                };
                found.removed(kind, 1);
                found.ops |= AnnotOps::all();
                tree | comment
                    | Effects::of(Flags::REMOVES | Flags::REPLACES_TEXT, SigClass::empty())
            }
            (a, b) => {
                found.ops |= AnnotOps::MODIFY;
                if says_the_same(a, b) {
                    comment
                } else {
                    found.removed(RemovedKind::CommentText, 1);
                    comment | Effects::of(Flags::REPLACES_TEXT, SigClass::empty())
                }
            }
        },
    }
}

/// Whether two comments say the same. Another tool's comment edited in
/// sheaf becomes the mark its summary reads it as (`ForeignAnnot::mark`),
/// so it says the same as that does.
fn says_the_same(a: &AnnotBody, b: &AnnotBody) -> bool {
    match (a, b) {
        (AnnotBody::Mark(a), AnnotBody::Mark(b)) => {
            same_words(&a.annot, &b.annot) && a.file == b.file && a.stamp == b.stamp
        }
        (AnnotBody::Foreign(a), AnnotBody::Foreign(b)) => foreign_says_the_same(a, b),
        (AnnotBody::Foreign(f), AnnotBody::Mark(m))
        | (AnnotBody::Mark(m), AnnotBody::Foreign(f)) => {
            (f.summary.mark.as_deref()).is_some_and(|was| same_words(was, &m.annot))
                && m.file.is_none()
                && m.stamp.is_none()
        }
        _ => false,
    }
}

/// Whether two of other tools' comments are the same object, saying the
/// same: what sheaf reads of what they say.
fn foreign_says_the_same(a: &Foreign, b: &Foreign) -> bool {
    let (x, y) = (&a.summary, &b.summary);
    a.src == b.src
        && x.subtype == y.subtype
        && x.contents == y.contents
        && x.author == y.author
        && x.name == y.name
        && x.parent == y.parent
        && x.state == y.state
}

/// Whether two marks say the same: the same kind, with the same text
/// wherever a mark holds some (contents, author, name, and its kind's own:
/// struck text, tape rows, labels, stamps, statuses, attachments). What's
/// left to differ is geometry and style.
fn same_words(a: &Annot, b: &Annot) -> bool {
    use AnnotKind as K;
    let kinds = match (&a.kind, &b.kind) {
        (K::Replace { struck: x, .. }, K::Replace { struck: y, .. }) => x == y,
        (K::Tape { tape: x, .. }, K::Tape { tape: y, .. }) => x == y,
        (K::Stamp { glyph: x, .. }, K::Stamp { glyph: y, .. }) => x == y,
        (K::Connector { label: x, .. }, K::Connector { label: y, .. }) => x == y,
        (K::Reply { parent: x }, K::Reply { parent: y }) => x == y,
        (
            K::Status {
                parent,
                model,
                state,
            },
            K::Status {
                parent: p,
                model: m,
                state: s,
            },
        ) => (parent, model, state) == (p, m, s),
        (
            K::PageStamp {
                stamp, name, text, ..
            },
            K::PageStamp {
                stamp: s,
                name: n,
                text: t,
                ..
            },
        ) => (stamp, name, text) == (s, n, t),
        (K::TextStamp { text: x, .. }, K::TextStamp { text: y, .. }) => x == y,
        (K::PageBorder { status: x, .. }, K::PageBorder { status: y, .. }) => x == y,
        (
            K::FileAttachment { file, name, .. },
            K::FileAttachment {
                file: f, name: n, ..
            },
        ) => (file, name) == (f, n),
        (x, y) => std::mem::discriminant(x) == std::mem::discriminant(y),
    };
    kinds && a.contents == b.contents && a.author == b.author && a.name == b.name
}

/// A widget changed: its state is filling in its field, and anything else
/// about it (moved, its rect, `/MK`, `/DA`, flags) is the form's tree.
fn widget(moved: bool, a: &Widget, b: &Widget) -> Effects {
    let mut e = Effects::NONE;
    if a.state != b.state {
        e |= Effects::of(Flags::FIELD_VALUES, SigClass::FILL);
    }
    let rest = Widget {
        state: b.state.clone(),
        ..Widget::clone(a)
    };
    if moved || rest != *b {
        e |= Effects::of(Flags::FIELD_TREE, SigClass::OTHER);
    }
    e
}

/// A node of the field tree added, removed, renamed, moved, or its
/// terminal changed.
fn field(a: Option<&Arc<FieldNode>>, b: Option<&Arc<FieldNode>>, found: &mut Found) -> Effects {
    let tree = Effects::of(Flags::FIELD_TREE, SigClass::OTHER);
    let (a, b) = match (a, b) {
        (None, None) => return Effects::NONE,
        (None, Some(_)) => return tree,
        (Some(_), None) => {
            found.removed(RemovedKind::Fields, 1);
            return tree | Effects::of(Flags::REMOVES, SigClass::empty());
        }
        (Some(a), Some(b)) => (a, b),
    };
    let mut e = Effects::NONE;
    let mut text = false;
    if a.partial != b.partial {
        e |= tree;
        text |= !a.partial.is_empty();
    }
    if a.parent != b.parent || a.kids != b.kids || a.src != b.src || a.inst != b.inst {
        e |= tree;
    }
    match (a.term.as_deref(), b.term.as_deref()) {
        (Some(ta), Some(tb)) => e |= terminal(ta, tb, &mut text, found),
        (None, None) => {}
        (ta, _) => {
            e |= tree;
            if ta.is_some_and(|t| !blank(&t.value)) {
                e.flags |= Flags::REPLACES_TEXT;
                found.removed(RemovedKind::FieldValues, 1);
            }
        }
    }
    if text {
        e.flags |= Flags::REPLACES_TEXT;
        found.removed(RemovedKind::FieldText, 1);
    }
    e
}

/// A terminal field changed: its value is filling it in, replaced text
/// when it held one; the rest is the form's tree. `text` is set when its
/// tooltip, items or scripts were replaced.
fn terminal(a: &Terminal, b: &Terminal, text: &mut bool, found: &mut Found) -> Effects {
    let mut e = Effects::NONE;
    if a.value != b.value {
        e |= Effects::of(Flags::FIELD_VALUES, SigClass::FILL);
    }
    let replaced =
        (a.value != b.value && !blank(&a.value)) || (a.default != b.default && !blank(&a.default));
    if replaced {
        e.flags |= Flags::REPLACES_TEXT;
        found.removed(RemovedKind::FieldValues, 1);
    }
    if a.kind != b.kind
        || a.flags != b.flags
        || a.default != b.default
        || a.props != b.props
        || a.widgets != b.widgets
        || a.outside != b.outside
    {
        e |= Effects::of(Flags::FIELD_TREE, SigClass::OTHER);
    }
    let (p, q) = (&a.props, &b.props);
    *text |= (p.tooltip != q.tooltip && p.tooltip.as_deref().is_some_and(|t| !t.is_empty()))
        || (p.options != q.options && !p.options.is_empty())
        || (p.actions != q.actions && !p.actions.is_empty());
    e
}

/// Whether a field holds nothing: no value, empty text or items, a box
/// that's off.
fn blank(v: &Option<Value>) -> bool {
    match v {
        None => true,
        Some(Value::Text(t)) => t.is_empty(),
        Some(Value::Items(items)) => items.is_empty(),
        Some(Value::State(s)) => &**s == "Off",
        Some(Value::Signed) => false,
    }
}

/// Page labels changed: a prefix no range has now is replaced text.
fn page_labels(
    a: Option<&Arc<[LabelSpec]>>,
    b: Option<&Arc<[LabelSpec]>>,
    found: &mut Found,
) -> Effects {
    let now: HashSet<&str> = b
        .into_iter()
        .flat_map(|l| l.iter())
        .map(|s| &*s.prefix)
        .collect();
    let gone: HashSet<&str> = (a.into_iter().flat_map(|l| l.iter()))
        .map(|s| &*s.prefix)
        .filter(|p| !p.is_empty() && !now.contains(p))
        .collect();
    let mut flags = Flags::LABELS;
    if !gone.is_empty() {
        flags |= Flags::REPLACES_TEXT;
        found.removed(RemovedKind::LabelPrefixes, gone.len());
    }
    Effects::of(flags, SigClass::OTHER)
}

/// A generator added, changed or removed. One that was drawn in the saved
/// file, gone, takes what it drew with it; so does one that draws
/// otherwise ([`unsaid`]): a watermark another text, image or page, a
/// header and footer other text, and either on fewer pages. One only laid
/// out otherwise is drawn again (§7.1). One that replaces others'
/// artifacts on pages of the saved file that drew them takes those off
/// ([`replaced`]).
fn generator(
    a: Option<&Arc<Generator>>,
    b: Option<&Arc<Generator>>,
    cx: &ClassifyCx,
    found: &mut Found,
) -> Effects {
    let drawn = |b: Option<&Arc<Generator>>| match b {
        Some(_) => Flags::CONTENT,
        None => Flags::empty(),
    };
    let flags = match (a, b) {
        (None, None) => Flags::empty(),
        (None, Some(_)) => Flags::CONTENT,
        (Some(a), Some(b)) if !unsaid(a, b, cx) => Flags::CONTENT,
        (Some(a), b) => match &a.spec {
            GenSpec::Watermark(was) => {
                let text = matches!(was.content, WatermarkContent::Text { .. });
                let kind = match a.z {
                    Z::Over => RemovedKind::Watermark { text },
                    Z::Under => RemovedKind::Background { text },
                };
                found.removed(kind, 1);
                let replaced = match text {
                    true => Flags::REPLACES_TEXT,
                    false => Flags::empty(),
                };
                drawn(b) | Flags::REMOVES | replaced
            }
            GenSpec::HeaderFooter(_) => {
                found.removed(RemovedKind::HeaderFooter, 1);
                drawn(b) | Flags::REMOVES | Flags::REPLACES_TEXT
            }
        },
    };
    let others = match replaced(b, cx) {
        0 => Flags::empty(),
        n => {
            found.removed(RemovedKind::Filtered, n);
            Flags::CONTENT | Flags::REMOVES
        }
    };
    Effects::of(flags | others, SigClass::OTHER)
}

/// How many pages of the saved file generator `b` takes others' artifacts
/// off as it's drawn there now (§7.1's `replaces` row): those it covers
/// that showed the same base page then, which holds pagination artifacts
/// of a kind it replaces and the saved state's generators there didn't,
/// drawn by any tool but the author ([`PagOrigin::REPLACED`]), a block of
/// a generator the document backs aside. Read from the pages' blocks, which
/// their sources keep once read: judging a save's path reads them, and a
/// step's never does, judging what one step did in O(what it touched).
fn replaced(b: Option<&Arc<Generator>>, cx: &ClassifyCx) -> usize {
    let Some(g) = b else {
        return 0;
    };
    if cx.step || g.replaces.is_empty() {
        return 0;
    }
    let origins = BTreeSet::from(PagOrigin::REPLACED);
    let pages: Box<dyn Iterator<Item = &PageId>> = match &g.scope {
        Scope::All => Box::new(cx.target.order().iter()),
        Scope::Pages(on) => Box::new(on.iter()),
    };
    let mut n = 0;
    for &p in pages {
        let (Some(then), Some(now)) = (cx.saved.def(p), cx.target.def(p)) else {
            continue;
        };
        if then.base != now.base {
            continue;
        }
        let saved_replaced = |k: &PaginationKind| {
            (cx.saved.generators().values()).any(|g| g.covers(p) && g.replaces.contains(k))
        };
        let kinds: BTreeSet<PaginationKind> = (g.replaces.iter())
            .filter(|k| !saved_replaced(k))
            .copied()
            .collect();
        if kinds.is_empty() {
            continue;
        }
        let Ok(blocks) = then.base.src.blocks(then.base.index) else {
            continue;
        };
        let ours = |id: Option<GenId>| id.is_some_and(|id| cx.target.backs(id, p));
        if (blocks.selected(&kinds, &origins)).any(|r| !ours(r.generator)) {
            n += 1;
        }
    }
    n
}

/// Whether generator `was`, become `now`, no longer draws something it
/// drew: a watermark another text, image or page; a header and footer
/// other text (its slots, its first page number, its Bates numbering, or
/// what its date, its file or a page's Bates number say); or either not
/// on a page it was on that's still a page. Where it is, how big, turned,
/// colored or see-through it is, its face, and its z, are how it's laid
/// out.
fn unsaid(was: &Generator, now: &Generator, cx: &ClassifyCx) -> bool {
    use WatermarkContent as C;
    let another = match (&was.spec, &now.spec) {
        (GenSpec::Watermark(a), GenSpec::Watermark(b)) => match (&a.content, &b.content) {
            (C::Text { text: a, .. }, C::Text { text: b, .. }) => a != b,
            (C::Image { image: a, .. }, C::Image { image: b, .. }) => a != b,
            (C::Page { page: a, .. }, C::Page { page: b, .. }) => a != b,
            _ => true,
        },
        (GenSpec::HeaderFooter(a), GenSpec::HeaderFooter(b)) => {
            a.texts != b.texts
                || a.first_number != b.first_number
                || a.bates != b.bates
                || was.frozen.date != now.frozen.date
                || was.frozen.file != now.frozen.file
                || renumbered(was, now, cx)
        }
        _ => true,
    };
    let off = |on: &im::OrdSet<PageId>, p: &PageId| !on.contains(p) && cx.target.def(*p).is_some();
    another
        || match (&was.scope, &now.scope) {
            (_, Scope::All) => false,
            (Scope::All, Scope::Pages(on)) => cx.saved.order().iter().any(|p| off(on, p)),
            (Scope::Pages(was), Scope::Pages(on)) => was.iter().any(|p| off(on, p)),
        }
}

/// Whether header `was`, become `now`, says another Bates number, or none,
/// on a page still in the document that it numbered.
fn renumbered(was: &Generator, now: &Generator, cx: &ClassifyCx) -> bool {
    let Some(numbers) = &was.frozen.bates else {
        return false;
    };
    if now
        .frozen
        .bates
        .as_ref()
        .is_some_and(|b| Arc::ptr_eq(b, numbers))
    {
        return false;
    }
    (numbers.iter())
        .any(|(&p, &n)| cx.target.def(p).is_some() && now.frozen.bates_number(p) != Some(n))
}

/// Adds the full names of the fields a change to `key` touches to `names`,
/// in the saved state and now: a field's, and every node's under it when
/// it was renamed, moved, added or removed; a widget's field's.
fn touched_fields(
    key: &Key,
    before: Option<&Val>,
    after: Option<&Val>,
    cx: &ClassifyCx,
    names: &mut BTreeSet<String>,
) {
    let states = [cx.saved, cx.target];
    match *key {
        Key::Field(id) => {
            let moved = match (before, after) {
                (Some(Val::Field(a)), Some(Val::Field(b))) => {
                    a.partial != b.partial || a.parent != b.parent
                }
                _ => true,
            };
            for st in states {
                if moved {
                    names_under(st, id, names);
                } else {
                    names.extend(st.full_name(id));
                }
            }
        }
        Key::Annot(_) => {
            let widgets = [before, after]
                .into_iter()
                .flatten()
                .filter_map(|v| match v {
                    Val::Annot(rec) => Some(rec.widget()?.field),
                    _ => None,
                });
            for (field, st) in widgets.flat_map(|f| states.map(|st| (f, st))) {
                names.extend(st.full_name(field));
            }
        }
        _ => {}
    }
}

/// Adds the full names of node `id` of `st` and every node under it to
/// `names`.
fn names_under(st: &DocState, id: FieldId, names: &mut BTreeSet<String>) {
    let mut next = vec![id];
    let mut seen = HashSet::new();
    while let Some(id) = next.pop() {
        let Some(node) = st.field(id).filter(|_| seen.insert(id)) else {
            continue;
        };
        names.extend(st.full_name(id));
        next.extend(node.kids.iter().copied());
    }
}

#[cfg(test)]
mod tests;
