//! What an edit changed, as history keeps it (M10.5 §6.2).
//!
//! A [`Prim`] puts one slot of the document, a [`Key`], from the value it
//! held before the edit to the value it holds after. `None` is absent: no
//! labels, no such generator, nothing for that page, bookmark or field.
//! Before-values cost nothing, since they are the parent's values (im and
//! `Arc` clones), and they make replay checked: [`DocState::apply`]
//! refuses a prim whose before isn't what the slot holds, so a damaged
//! history fails instead of quietly building another document.
//!
//! An edit writes through a recording context, [`Rec`], which turns what it
//! wrote into prims. A slot put back to the value it had makes none, so an
//! edit that changes nothing has none.
//!
//! History counts what it holds with [`HeapSize`] (§3.9).
//!
//! The keys are the records' own, eleven kinds of slot (`Order`, `Page`,
//! `Geom`, `PageAnnots`, `Annot`, `Field`, `Form`, `Bookmark`,
//! `OutlineRoots`, `Labels`, `Generator`).

use std::collections::BTreeSet;
use std::ops::Deref;
use std::sync::Arc;

use im::Vector;
use sheaf_core::DestView;

use crate::annot::{Annot, AnnotId, AnnotKind, Box2, Pt};
use crate::file::ForeignAnnot;
use crate::forms::FieldId;
use crate::forms::field::{Action, Choice, Mk, Trigger, Value};
use crate::geom::Geom;
use crate::labels::LabelSpec;
use crate::measure::Scale;
use crate::model::{
    AnnotBody, AnnotRec, Bookmark, ContentStack, DocState, Edit, FieldNode, FieldProps,
    FlatOutlineItem, Foreign, FormDoc, FormInst, GenId, GenSpec, Generator, Layer, Mark,
    NewOutlineItem, OutlineId, PageDef, PageId, Rebase, ScriptState, Static, Target, Terminal,
    Widget, WidgetProps,
};
use crate::stamps::{Bates, HeaderFooter};
use crate::tape::{Row, Tape};
use crate::text_layer::TextLayer;

/// A slot of the document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Key {
    /// The pages in document order.
    Order,
    /// What a page shows.
    Page(PageId),
    /// A page's boxes and `/Rotate`.
    Geom(PageId),
    Bookmark(OutlineId),
    /// The top-level bookmarks, in order.
    OutlineRoots,
    /// A page's annotations, in `/Annots` order.
    PageAnnots(PageId),
    Annot(AnnotId),
    /// A node of the form's field tree.
    Field(FieldId),
    /// The form as a whole.
    Form,
    Labels,
    Generator(GenId),
}

/// What a slot holds. Order, OutlineRoots and Form always hold one.
#[derive(Debug, Clone)]
pub enum Val {
    Order(Vector<PageId>),
    Page(Arc<PageDef>),
    Geom(Arc<Geom>),
    Bookmark(Arc<Bookmark>),
    OutlineRoots(Vector<OutlineId>),
    PageAnnots(Vector<AnnotId>),
    Annot(AnnotRec),
    Field(Arc<FieldNode>),
    Form(Arc<FormDoc>),
    Labels(Arc<[LabelSpec]>),
    Generator(Arc<Generator>),
}

/// One slot, from `before` to `after`.
#[derive(Debug, Clone)]
pub struct Prim {
    pub key: Key,
    pub before: Option<Val>,
    pub after: Option<Val>,
}

/// A prim that doesn't fit the state: `key` doesn't hold its before, or
/// its after isn't a value `key` can hold.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[error("{key:?} doesn't hold what the history says it held")]
pub struct Mismatch {
    pub key: Key,
}

/// Whether two slot values are the same: the same im nodes or `Arc`, or
/// else equal.
pub(crate) fn same(a: Option<&Val>, b: Option<&Val>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(Val::Order(a)), Some(Val::Order(b))) => a.ptr_eq(b) || a == b,
        (Some(Val::Page(a)), Some(Val::Page(b))) => Arc::ptr_eq(a, b) || a == b,
        (Some(Val::Geom(a)), Some(Val::Geom(b))) => Arc::ptr_eq(a, b) || a == b,
        (Some(Val::Bookmark(a)), Some(Val::Bookmark(b))) => Arc::ptr_eq(a, b) || a == b,
        (Some(Val::OutlineRoots(a)), Some(Val::OutlineRoots(b))) => a.ptr_eq(b) || a == b,
        (Some(Val::PageAnnots(a)), Some(Val::PageAnnots(b))) => a.ptr_eq(b) || a == b,
        (Some(Val::Annot(a)), Some(Val::Annot(b))) => a.same(b),
        (Some(Val::Labels(a)), Some(Val::Labels(b))) => Arc::ptr_eq(a, b) || a == b,
        (Some(Val::Generator(a)), Some(Val::Generator(b))) => Arc::ptr_eq(a, b) || a == b,
        (Some(Val::Field(a)), Some(Val::Field(b))) => Arc::ptr_eq(a, b) || a == b,
        (Some(Val::Form(a)), Some(Val::Form(b))) => Arc::ptr_eq(a, b) || a == b,
        _ => false,
    }
}

impl DocState {
    /// Applies `p`: when its key holds its before, puts its after there.
    /// Otherwise the state is left as it was.
    pub fn apply(&mut self, p: &Prim) -> Result<(), Mismatch> {
        if !same(self.get(&p.key).as_ref(), p.before.as_ref()) {
            return Err(Mismatch { key: p.key });
        }
        self.set(&p.key, p.after.clone())
    }
}

/// The state an edit is computing, and what it started from. The edit
/// reads its own writes through `Deref`, and writes only with
/// [`put`](Self::put), so [`finish`](Self::finish) knows each slot it
/// changed.
pub(crate) struct Rec {
    base: DocState,
    st: DocState,
    touched: BTreeSet<Key>,
}

impl Rec {
    pub(crate) fn new(base: DocState) -> Self {
        Self {
            st: base.clone(),
            base,
            touched: BTreeSet::new(),
        }
    }

    pub(crate) fn put(&mut self, key: Key, val: Option<Val>) {
        self.st
            .set(&key, val)
            .expect("an edit puts values its keys can hold");
        self.touched.insert(key);
    }

    /// The state the edit made, and its prims from the state it started
    /// from, in key order: none for a slot that ended as it was.
    pub(crate) fn finish(self) -> (DocState, Vec<Prim>) {
        let prims = self
            .touched
            .into_iter()
            .filter_map(|key| {
                let before = self.base.get(&key);
                let after = self.st.get(&key);
                (!same(before.as_ref(), after.as_ref())).then_some(Prim { key, before, after })
            })
            .collect();
        (self.st, prims)
    }
}

impl Deref for Rec {
    type Target = DocState;

    fn deref(&self) -> &DocState {
        &self.st
    }
}

/// The bytes a value holds beyond its own `size_of`, as history counts
/// them. An estimate: an im collection counts as its elements, and an
/// `Arc` as if nothing else held it.
pub trait HeapSize {
    fn heap_size(&self) -> usize;
}

/// What a `Vec` holds for its elements, not counting what they hold.
fn buf<T>(v: &Vec<T>) -> usize {
    v.capacity() * size_of::<T>()
}

/// Plain data: nothing beyond its own size.
macro_rules! no_heap {
    ($($t:ty),*) => {
        $(impl HeapSize for $t {
            fn heap_size(&self) -> usize {
                0
            }
        })*
    };
}

no_heap!(
    u32, PageId, OutlineId, AnnotId, FieldId, FormInst, Pt, Box2, Trigger, Rebase
);

impl HeapSize for String {
    fn heap_size(&self) -> usize {
        self.capacity()
    }
}

impl HeapSize for Arc<str> {
    fn heap_size(&self) -> usize {
        2 * size_of::<usize>() + self.len()
    }
}

impl<T: HeapSize> HeapSize for Arc<[T]> {
    fn heap_size(&self) -> usize {
        2 * size_of::<usize>()
            + self.len() * size_of::<T>()
            + self.iter().map(T::heap_size).sum::<usize>()
    }
}

impl<T: HeapSize> HeapSize for Box<T> {
    fn heap_size(&self) -> usize {
        size_of::<T>() + T::heap_size(self)
    }
}

impl<T: HeapSize> HeapSize for Option<T> {
    fn heap_size(&self) -> usize {
        self.as_ref().map_or(0, T::heap_size)
    }
}

impl<A: HeapSize, B: HeapSize> HeapSize for (A, B) {
    fn heap_size(&self) -> usize {
        self.0.heap_size() + self.1.heap_size()
    }
}

impl<T: HeapSize> HeapSize for Vec<T> {
    fn heap_size(&self) -> usize {
        buf(self) + self.iter().map(T::heap_size).sum::<usize>()
    }
}

impl<T: HeapSize + Clone> HeapSize for Vector<T> {
    fn heap_size(&self) -> usize {
        self.len() * size_of::<T>() + self.iter().map(T::heap_size).sum::<usize>()
    }
}

impl HeapSize for DestView {
    fn heap_size(&self) -> usize {
        match self {
            DestView::Xyz { .. } => 0,
            DestView::Other { kind, params } => kind.heap_size() + buf(params),
        }
    }
}

impl HeapSize for Target {
    fn heap_size(&self) -> usize {
        match self {
            Target::Page { view, .. } => view.heap_size(),
            Target::Kept => 0,
        }
    }
}

/// Its title, where it goes and its kids; not its item's source, whose
/// bytes the store counts.
impl HeapSize for Bookmark {
    fn heap_size(&self) -> usize {
        self.title.heap_size() + self.target.heap_size() + self.kids.heap_size()
    }
}

impl HeapSize for FlatOutlineItem {
    fn heap_size(&self) -> usize {
        self.title.heap_size() + self.view.heap_size()
    }
}

impl HeapSize for NewOutlineItem {
    fn heap_size(&self) -> usize {
        self.title.heap_size() + self.view.heap_size()
    }
}

impl HeapSize for LabelSpec {
    fn heap_size(&self) -> usize {
        self.prefix.heap_size()
    }
}

impl HeapSize for Bates {
    fn heap_size(&self) -> usize {
        self.prefix.heap_size() + self.suffix.heap_size()
    }
}

impl HeapSize for HeaderFooter {
    fn heap_size(&self) -> usize {
        self.texts.iter().map(String::heap_size).sum::<usize>()
            + self.pages.heap_size()
            + self.date.heap_size()
            + self.file.heap_size()
            + self.bates.heap_size()
    }
}

impl HeapSize for GenSpec {
    fn heap_size(&self) -> usize {
        match self {
            GenSpec::HeaderFooter(hf) => size_of::<HeaderFooter>() + hf.heap_size(),
        }
    }
}

impl HeapSize for Generator {
    fn heap_size(&self) -> usize {
        self.spec.heap_size()
    }
}

impl HeapSize for Tape {
    fn heap_size(&self) -> usize {
        buf(&self.lines)
            + self
                .lines
                .iter()
                .map(|l| {
                    l.note.heap_size()
                        + match &l.row {
                            Row::Title(t) => t.heap_size(),
                            _ => 0,
                        }
                })
                .sum::<usize>()
    }
}

impl HeapSize for Scale {
    fn heap_size(&self) -> usize {
        self.page_unit.heap_size() + self.unit.heap_size()
    }
}

impl HeapSize for AnnotKind {
    fn heap_size(&self) -> usize {
        match self {
            AnnotKind::Highlight { rects }
            | AnnotKind::Underline { rects }
            | AnnotKind::StrikeOut { rects }
            | AnnotKind::Squiggly { rects } => rects.heap_size(),
            AnnotKind::Replace { rects, struck, .. } => rects.heap_size() + struck.heap_size(),
            AnnotKind::Ink { paths } => paths.heap_size(),
            AnnotKind::Polygon { points } | AnnotKind::Polyline { points } => points.heap_size(),
            AnnotKind::Measure { points, scale, .. } => points.heap_size() + scale.heap_size(),
            AnnotKind::Tape { tape, .. } => tape.heap_size(),
            AnnotKind::Stamp { glyph: s, .. }
            | AnnotKind::Connector { label: s, .. }
            | AnnotKind::Reply { parent: s }
            | AnnotKind::TextStamp { text: s, .. }
            | AnnotKind::PageBorder { status: s, .. } => s.heap_size(),
            AnnotKind::Status {
                parent,
                model,
                state,
            } => parent.heap_size() + model.heap_size() + state.heap_size(),
            AnnotKind::PageStamp {
                stamp, name, text, ..
            } => stamp.heap_size() + name.heap_size() + text.heap_size(),
            AnnotKind::FileAttachment {
                icon, file, name, ..
            } => icon.heap_size() + file.heap_size() + name.heap_size(),
            AnnotKind::Note { .. }
            | AnnotKind::FreeText { .. }
            | AnnotKind::Square { .. }
            | AnnotKind::Ellipse { .. }
            | AnnotKind::Line { .. }
            | AnnotKind::Insert { .. }
            | AnnotKind::Callout { .. } => 0,
        }
    }
}

impl HeapSize for Annot {
    fn heap_size(&self) -> usize {
        self.kind.heap_size()
            + self.author.heap_size()
            + self.contents.heap_size()
            + self.modified.heap_size()
            + self.created.heap_size()
            + self.name.heap_size()
    }
}

/// Its mark, as read: not the object it was read as, nor its stamp's page
/// or its file, whose bytes the store counts.
impl HeapSize for Mark {
    fn heap_size(&self) -> usize {
        self.annot.heap_size()
    }
}

impl HeapSize for ForeignAnnot {
    fn heap_size(&self) -> usize {
        self.subtype.heap_size()
            + self.author.heap_size()
            + self.contents.heap_size()
            + self.modified.heap_size()
            + self.name.heap_size()
            + self.parent.heap_size()
            + self.state.heap_size()
            + self.created.heap_size()
            + self.mark.heap_size()
    }
}

/// Its summary, which its copies share: not its object, whose bytes the
/// store counts.
impl HeapSize for Foreign {
    fn heap_size(&self) -> usize {
        size_of::<ForeignAnnot>() + self.summary.heap_size()
    }
}

impl HeapSize for Mk {
    fn heap_size(&self) -> usize {
        self.caption.heap_size()
    }
}

impl HeapSize for WidgetProps {
    fn heap_size(&self) -> usize {
        self.actions.heap_size()
    }
}

/// Where it is and what it shows, and its props, which its copies share;
/// not the annotation it was read as, whose bytes the store counts.
impl HeapSize for Widget {
    fn heap_size(&self) -> usize {
        self.da.heap_size()
            + self.state.heap_size()
            + self.states.heap_size()
            + size_of::<Mk>()
            + self.mk.heap_size()
            + self.shared()
    }
}

impl Widget {
    /// Its props, which its copies share.
    pub(crate) fn shared(&self) -> usize {
        size_of::<WidgetProps>() + self.props.heap_size()
    }
}

impl HeapSize for AnnotBody {
    fn heap_size(&self) -> usize {
        match self {
            AnnotBody::Mark(m) => size_of::<Mark>() + m.heap_size(),
            AnnotBody::Foreign(f) => size_of::<Foreign>() + f.heap_size(),
            AnnotBody::Widget(w) => size_of::<Widget>() + w.heap_size(),
        }
    }
}

impl HeapSize for AnnotRec {
    fn heap_size(&self) -> usize {
        self.body.heap_size()
    }
}

impl HeapSize for Value {
    fn heap_size(&self) -> usize {
        match self {
            Value::Text(s) | Value::State(s) => s.heap_size(),
            Value::Items(items) => items.heap_size(),
            Value::Signed => 0,
        }
    }
}

impl HeapSize for Choice {
    fn heap_size(&self) -> usize {
        self.export.heap_size() + self.shown.heap_size()
    }
}

impl HeapSize for Action {
    fn heap_size(&self) -> usize {
        self.kind.heap_size() + self.script.heap_size()
    }
}

impl HeapSize for FieldProps {
    fn heap_size(&self) -> usize {
        self.da.heap_size()
            + self.options.heap_size()
            + self.tooltip.heap_size()
            + self.actions.heap_size()
    }
}

impl HeapSize for Terminal {
    fn heap_size(&self) -> usize {
        self.value.heap_size()
            + self.default.heap_size()
            + self.widgets.heap_size()
            + size_of::<FieldProps>()
            + self.props.heap_size()
    }
}

/// Its name, kids and terminal; not the field it was read as, whose bytes
/// the store counts.
impl HeapSize for FieldNode {
    fn heap_size(&self) -> usize {
        self.partial.heap_size()
            + self.kids.heap_size()
            + self
                .term
                .as_ref()
                .map_or(0, |t| size_of::<Terminal>() + t.heap_size())
    }
}

impl FieldNode {
    /// Its terminal's props, which its copies share.
    pub(crate) fn shared(&self) -> usize {
        (self.term.as_ref()).map_or(0, |t| size_of::<FieldProps>() + t.props.heap_size())
    }
}

impl HeapSize for ScriptState {
    fn heap_size(&self) -> usize {
        match self {
            ScriptState::On => 0,
            ScriptState::Off { instances } => instances.heap_size(),
        }
    }
}

impl HeapSize for FormDoc {
    fn heap_size(&self) -> usize {
        self.roots.heap_size()
            + self.names.len() * size_of::<(FormInst, Arc<str>)>()
            + self.names.values().map(Arc::heap_size).sum::<usize>()
            + self.scripts.heap_size()
            + self.calc_order.heap_size()
            + self.da.as_ref().map_or(0, Arc::heap_size)
    }
}

impl HeapSize for TextLayer {
    fn heap_size(&self) -> usize {
        self.byte_estimate()
    }
}

impl HeapSize for Static {
    fn heap_size(&self) -> usize {
        match self {
            Static::OcrText(text) => size_of::<TextLayer>() + text.heap_size(),
        }
    }
}

impl HeapSize for Layer {
    fn heap_size(&self) -> usize {
        self.body.heap_size()
    }
}

impl HeapSize for ContentStack {
    fn heap_size(&self) -> usize {
        self.over.heap_size()
    }
}

/// Its content stack counts, but not the empty one every untouched page
/// shares, nor its source, whose bytes the store counts.
impl HeapSize for PageDef {
    fn heap_size(&self) -> usize {
        if ContentStack::is_shared_empty(&self.content) {
            0
        } else {
            size_of::<ContentStack>() + self.content.heap_size()
        }
    }
}

impl HeapSize for Val {
    fn heap_size(&self) -> usize {
        match self {
            Val::Order(v) => v.heap_size(),
            Val::Page(def) => size_of::<PageDef>() + def.heap_size(),
            Val::Geom(_) => size_of::<Geom>(),
            Val::Bookmark(b) => size_of::<Bookmark>() + b.heap_size(),
            Val::OutlineRoots(v) => v.heap_size(),
            Val::PageAnnots(v) => v.heap_size(),
            Val::Annot(rec) => rec.heap_size(),
            Val::Field(f) => size_of::<FieldNode>() + f.heap_size(),
            Val::Form(f) => size_of::<FormDoc>() + f.heap_size(),
            Val::Labels(v) => v.heap_size(),
            Val::Generator(g) => size_of::<Generator>() + g.heap_size(),
        }
    }
}

/// What an edit holds beyond its own size. Ids are plain data.
impl HeapSize for Edit {
    fn heap_size(&self) -> usize {
        match self {
            Edit::MovePages { ids, .. }
            | Edit::RotatePages { ids, .. }
            | Edit::DeletePages { ids }
            | Edit::DuplicatePages { ids }
            | Edit::ClearTextLayers { ids } => ids.heap_size(),
            Edit::InsertPages {
                outline, annots, ..
            } => outline.heap_size() + annots.heap_size(),
            Edit::ReplacePages { ids, annots, .. } => ids.heap_size() + annots.heap_size(),
            Edit::OutlineInsert { title, view, .. } => title.heap_size() + view.heap_size(),
            Edit::OutlineInsertTree { items, .. } => items.heap_size(),
            Edit::OutlineMove { .. } | Edit::OutlineDelete { .. } | Edit::AnnotDelete { .. } => 0,
            Edit::OutlineEdit { title, view, .. } => title.heap_size() + view.heap_size(),
            Edit::AnnotAdd { annot, .. } | Edit::AnnotUpdate { annot, .. } => annot.heap_size(),
            Edit::CommentsDelete { names, .. } => names.heap_size(),
            Edit::ConnectorAdd { a, b } => a.heap_size() + b.heap_size(),
            // The layers are the ones its prims put, and counted there.
            Edit::SetTextLayers { layers } => buf(layers),
            Edit::RebasePages { pages } => pages.heap_size(),
            Edit::SetPageLabels { ranges } => ranges.heap_size(),
            Edit::SetHeaderFooter { header_footer } => header_footer.heap_size(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::labels::Style;
    use crate::model::{PageBase, SourceId, SourceInfo};
    use crate::store::Store;

    /// Three blank pages, in their file's order.
    fn state() -> DocState {
        let src = SourceInfo::blank(&Store::new(), 3).src;
        let pages = (0..3).map(|index| {
            let def = PageDef {
                base: PageBase {
                    source: SourceId(0),
                    src: src.clone(),
                    index,
                },
                copy: 0,
                content: ContentStack::empty(),
            };
            (
                PageId(index.into()),
                Arc::new(def),
                Arc::new(src.geom(index)),
            )
        });
        DocState::builder().pages(pages).build()
    }

    fn labels(prefix: &str) -> Val {
        Val::Labels(Arc::new([LabelSpec {
            start: PageId(0),
            style: Style::Decimal,
            prefix: prefix.into(),
            first: 1,
        }]))
    }

    fn reversed(st: &DocState) -> Vector<PageId> {
        st.order().iter().rev().copied().collect()
    }

    #[test]
    fn apply_sets_and_removes() {
        let mut st = state();
        let order = reversed(&st);
        st.apply(&Prim {
            key: Key::Order,
            before: st.get(&Key::Order),
            after: Some(Val::Order(order.clone())),
        })
        .unwrap();
        assert_eq!(*st.order(), order);
        st.apply(&Prim {
            key: Key::Labels,
            before: None,
            after: Some(labels("A-")),
        })
        .unwrap();
        assert_eq!(st.labels().unwrap()[0].prefix, "A-");
        st.apply(&Prim {
            key: Key::Labels,
            before: Some(labels("A-")),
            after: None,
        })
        .unwrap();
        assert_eq!(st.labels(), None);
    }

    /// A new order finds positions anew, while the states of the old one
    /// keep theirs.
    #[test]
    fn positions_follow_the_order() {
        let mut st = state();
        let was = st.clone();
        assert_eq!(st.pos_of(PageId(0)), Some(0));
        st.apply(&Prim {
            key: Key::Order,
            before: st.get(&Key::Order),
            after: Some(Val::Order(reversed(&st))),
        })
        .unwrap();
        assert_eq!(st.pos_of(PageId(0)), Some(2));
        assert_eq!(st.pos_of(PageId(2)), Some(0));
        assert_eq!(st.pos_of(PageId(9)), None);
        assert_eq!(was.pos_of(PageId(0)), Some(0));
        assert_eq!(st.page_at(0), Some(PageId(2)));
    }

    #[test]
    fn a_wrong_before_is_a_mismatch_and_changes_nothing() {
        let mut st = state();
        let was = st.clone();
        let wrong = Prim {
            key: Key::Order,
            before: Some(Val::Order(reversed(&st))),
            after: Some(Val::Order(Vector::new())),
        };
        assert_eq!(st.apply(&wrong), Err(Mismatch { key: Key::Order }));
        let absent = Prim {
            key: Key::Labels,
            before: Some(labels("A-")),
            after: None,
        };
        assert_eq!(st.apply(&absent), Err(Mismatch { key: Key::Labels }));
        // The order always holds a value, and only an order.
        let misfit = Prim {
            key: Key::Order,
            before: st.get(&Key::Order),
            after: Some(labels("A-")),
        };
        assert_eq!(st.apply(&misfit), Err(Mismatch { key: Key::Order }));
        assert_eq!(st, was);
    }

    #[test]
    fn same_sees_equal_values_as_the_same() {
        let st = state();
        let (a, b) = (Val::Order(reversed(&st)), Val::Order(reversed(&st)));
        let (Val::Order(va), Val::Order(vb)) = (&a, &b) else {
            unreachable!()
        };
        assert!(!va.ptr_eq(vb), "built apart");
        assert!(same(Some(&a), Some(&b)));
        assert!(!same(Some(&a), st.get(&Key::Order).as_ref()));
        // A page's records built apart, and equal.
        let def = st.def(PageId(1)).unwrap();
        let again = Val::Page(Arc::new(PageDef::clone(def)));
        assert!(same(Some(&again), st.get(&Key::Page(PageId(1))).as_ref()));
        assert!(!same(Some(&again), st.get(&Key::Page(PageId(2))).as_ref()));
        assert!(same(None, None));
        assert!(!same(Some(&labels("A-")), None));
        assert!(!same(Some(&labels("A-")), Some(&labels("B-"))));
        assert!(
            !same(Some(&a), Some(&labels("A-"))),
            "another kind of value"
        );
    }

    #[test]
    fn heap_size_counts_buffers_and_what_they_hold() {
        let names = vec!["abc".to_string(), String::with_capacity(10)];
        let strings = names.iter().map(String::capacity).sum::<usize>();
        assert_eq!(
            names.heap_size(),
            names.capacity() * size_of::<String>() + strings
        );
        // An im vector counts as its elements.
        let order = Val::Order(state().order().clone());
        assert_eq!(order.heap_size(), 3 * size_of::<PageId>());
        assert_eq!(
            labels("A-").heap_size(),
            2 * size_of::<usize>() + size_of::<LabelSpec>() + 2
        );
        // An untouched page holds the stack every one shares.
        let page = state().get(&Key::Page(PageId(0))).unwrap();
        assert_eq!(page.heap_size(), size_of::<PageDef>());
    }

    #[test]
    fn finish_drops_a_key_put_back() {
        let st = state();
        let mut rec = Rec::new(st.clone());
        rec.put(Key::Labels, Some(labels("A-")));
        rec.put(Key::Geom(PageId(9)), None);
        rec.put(Key::Order, Some(Val::Order(reversed(&st))));
        assert_eq!(rec.page_at(0), Some(PageId(2)), "it reads its own writes");
        rec.put(Key::Labels, None);
        let (after, prims) = rec.finish();
        let keys: Vec<Key> = prims.iter().map(|p| p.key).collect();
        assert_eq!(keys, [Key::Order]);
        assert_eq!(*after.order(), reversed(&st));
        // The prims take the state it started from to the one it made.
        let mut replayed = st;
        for p in &prims {
            replayed.apply(p).unwrap();
        }
        assert_eq!(replayed, after);
    }
}
