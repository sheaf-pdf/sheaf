//! Render keys (M10.5 §10.2): what a page draws under some view inputs, as
//! a digest of what it's drawn from: its base page and what's drawn over
//! it, its boxes, the annotations the inputs draw, and the generators on
//! it with what their tokens read. Never of ids or copy numbers, so a
//! duplicate shares its original's tiles, a save changes no key, and an
//! undo back to a value finds what was drawn of it. A page's turn isn't
//! in it either, since a render's job turns it (§5), unless a generator
//! lays itself out in the turned page.
//!
//! [`render_reads`] are the slots a key is made from: what a decision made
//! from a page's pixels or text reads ([`Tx::read_render`]).
//!
//! [`Tx::read_render`]: crate::Tx::read_render

use std::collections::{BTreeSet, HashSet};
use std::fmt::Write as _;
use std::ops::Range;
use std::sync::Arc;

use sheaf_edit::{BatesSpec, HeaderFooterSpec, Margins, Rotation};
use sheaf_util::Checksum;

use super::{MarkDraw, ViewInputs};
use crate::annot::{Annot, AnnotId, Box2, Pt};
use crate::content::{Marker, PagOrigin, PaginationKind};
use crate::file::ForeignAnnot;
use crate::forms::FieldId;
use crate::forms::field::{Action, Border, Choice, Color, Mk, Trigger, Value};
use crate::geom::{Affine, Geom, Rect};
use crate::materialize::emit::Shown;
use crate::materialize::emit::annots::{LinkTo, base_annots};
use crate::materialize::emit::content;
use crate::model::{
    AnnotBody, ContentStack, CtxNeeds, DocState, FieldProps, Filter, Foreign, FormDoc, FormInst,
    FormRef, Frozen, GenSpec, Generator, ImageRef, Layer, LegacySel, Mark, MarkAp, MarkedSel,
    PageBase, PageCtx, PageDef, PageForm, PageId, PlaceTag, Placed, Placement, RenderTables,
    ScriptState, Static, StreamSel, Terminal, WatermarkContent, WatermarkGen, Widget, WidgetProps,
    XfaState,
};
use crate::prim::Key;
use crate::source::ObjRef;
use crate::store::{BlobId, BlobRef, Src};
use crate::text_layer::{OcrWord, TextLayer};

/// What a page draws under some view inputs: two pages with one key draw
/// the same pixels. What tiles, previews and page text are cached by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RenderKey(pub(super) u128);

/// Page `page` of `st`'s key under `inputs`; None when it isn't a page of
/// `st`. Made from the tables a page is drawn from, which the state gives
/// out of all of them (§2's registry rule: `DocState::render_tables`).
/// O(the annotations on the page) once its records' digests are worked
/// out, which each record keeps.
pub fn render_key(st: &DocState, page: PageId, inputs: &ViewInputs) -> Option<RenderKey> {
    let drawn = Drawn::of(st, page, inputs)?;
    let ViewInputs {
        marks,
        generators,
        ocr_over,
        widgets,
        foreign,
        hidden,
        dragging,
        ocg_off,
    } = inputs;
    let mut sum = Sum(Checksum::new());
    // What the inputs draw, whatever the page holds: they say too what's
    // drawn of what its base page has and no record does (pop-ups, the
    // widgets of a form that isn't modeled).
    let marks = match marks {
        MarkDraw::ByHost => 0u8,
        MarkDraw::ByEngine => 1,
        MarkDraw::Off => 2,
    };
    sum.feed(&marks)
        .feed(generators)
        .feed(ocr_over)
        .feed(widgets)
        .feed(foreign);
    sum.feed(&drawn.def.digest());
    feed_boxes(&mut sum, drawn.geom);
    sum.feed(&drawn.annots.len());
    for a in &drawn.annots {
        sum.feed(&hidden.contains(&a.id))
            .feed(&dragging.contains(&a.id));
        match a.body {
            AnnotBody::Mark(m) => sum.feed(&0u8).feed(&**m),
            AnnotBody::Foreign(f) => sum.feed(&1u8).feed(&**f).feed(&(a.to == LinkTo::Nowhere)),
            AnnotBody::Widget(w) => sum.feed(&2u8).feed(&**w).feed(&a.term.map(|(_, t)| t)),
        };
    }
    sum.feed(&drawn.form.map(FormDoc::digest));
    sum.feed(&drawn.generators.len());
    for on in &drawn.generators {
        let g = on.generator;
        sum.feed(&g.id.0)
            .feed(&recipe_digest(g, page))
            .feed(&on.base_draws);
        if !on.base_draws {
            // It's laid out in the page as it shows, turned, with what its
            // tokens read of where the page is, in the version that numbers
            // it (`generate::page_ctx`'s); its Bates number is its recipe's.
            sum.feed(drawn.geom);
            let CtxNeeds {
                index,
                count,
                label,
                bates: _,
            } = on.needs;
            let (st, pos) = match st.numbered_in(page) {
                Some((from, pos)) => (from, Some(pos)),
                None => (st, index.then(|| st.pos_of(page)).flatten()),
            };
            if index {
                sum.feed(&pos);
            }
            if count {
                sum.feed(&st.page_count());
            }
            if label {
                sum.feed(&st.label_of(page).as_deref());
            }
        }
    }
    sum.feed(&drawn.backed);
    // The layers its source shows, on every page of that source.
    let blob = drawn.def.base.src.id();
    sum.feed(&ocg_off.as_ref().and_then(|ocg| ocg.off.get(&blob)));
    Some(RenderKey(sum.0.finish()))
}

/// The slots page `page` of `st` is drawn from under `inputs`: what it
/// shows, and its annotations, the fields and form its widgets show, every
/// generator (one may come to be on it), the order when one on it reads
/// where the page is or a link on it leads to a page, with what that page
/// shows (whether it's still the link's), and the labels when one on it
/// draws the page's, as far as the inputs draw them. Never its boxes or
/// turn (§6.3). Under [`ViewInputs::content_only`], what it shows alone.
pub fn render_reads(st: &DocState, page: PageId, inputs: &ViewInputs) -> Vec<Key> {
    let mut reads = vec![Key::Page(page)];
    let Some(drawn) = Drawn::of(st, page, inputs) else {
        return reads;
    };
    if inputs.draws_annots() || inputs.widgets {
        reads.push(Key::PageAnnots(page));
    }
    let mut terms = HashSet::new();
    // A link is drawn while the page it leads to is there: the page
    // showing it, and the order, which moves, adds and takes away pages.
    let mut links = false;
    for a in &drawn.annots {
        if inputs.hidden.contains(&a.id) || inputs.dragging.contains(&a.id) {
            continue;
        }
        reads.push(Key::Annot(a.id));
        links |= matches!(a.body, AnnotBody::Foreign(f) if f.summary.is_link());
        if let LinkTo::Page(to) = a.to
            && to != page
        {
            reads.push(Key::Page(to));
        }
        if let Some((field, _)) = a.term
            && terms.insert(field)
        {
            reads.push(Key::Field(field));
        }
    }
    if drawn.form.is_some() {
        reads.push(Key::Form);
    }
    // Every generator, not only those on the page now: one whose pages
    // come to take this one in draws on it. One added after the read has an
    // id no read could name, so it doesn't conflict (slice 13's notes).
    if inputs.generators {
        reads.extend(st.generators().keys().map(|&g| Key::Generator(g)));
    }
    // Whether its base draws one is read too: a block laid out alike there
    // is one whose tokens read the page alike.
    if links || drawn.generators.iter().any(|on| on.needs.order()) {
        reads.push(Key::Order);
    }
    if drawn.generators.iter().any(|on| on.needs.label) {
        reads.push(Key::Labels);
    }
    reads
}

/// A page's digest of what it draws: its base page and what's drawn over
/// it ([`PageDef::digest`]).
pub(crate) fn page_digest(base: &PageBase, content: &ContentStack) -> u128 {
    let mut sum = Sum(Checksum::new());
    sum.feed(base).feed(content);
    sum.0.finish()
}

/// The form's digest ([`FormDoc::digest`]).
pub(crate) fn form_digest(form: &FormDoc) -> u128 {
    let mut sum = Sum(Checksum::new());
    sum.feed(form);
    sum.0.finish()
}

/// What a pinned appearance is drawn from, as a digest
/// ([`PinnedAp::digest`](crate::model::PinnedAp)).
pub(crate) fn pinned_digest(ap: &MarkAp) -> u128 {
    let mut sum = Sum(Checksum::new());
    sum.feed(ap);
    sum.0.finish()
}

/// Which form `form` is, as a digest ([`FormRef::digest`]).
pub(crate) fn form_ref_digest(form: &FormRef) -> u128 {
    let mut sum = Sum(Checksum::new());
    sum.feed(form);
    sum.0.finish()
}

/// What generator `g` draws on page `page`, whichever pages it's on: its
/// spec, z and the kinds it replaces, and what it fixed for the page (a
/// header's date, file and the page's Bates number), which its blocks say
/// they were drawn from (`/R`). A watermark's is the same on every page.
pub(crate) fn recipe_digest(g: &Generator, page: PageId) -> u128 {
    let mut sum = Sum(Checksum::new());
    match &g.spec {
        GenSpec::HeaderFooter(hf) => sum.feed(&**hf),
        GenSpec::Watermark(wm) => sum.feed(&**wm),
    };
    sum.feed(&(g.z as u8)).feed(&g.replaces);
    if g.spec.header_footer().is_some() {
        let Frozen { date, file, bates } = &g.frozen;
        let n = bates.as_ref().and_then(|b| b.get(&page));
        sum.feed(&**date).feed(&**file).feed(&n.copied());
    }
    sum.0.finish()
}

/// What a generator whose tokens read `needs` lays out on the page `ctx`
/// says reads of it, as a digest: the page's boxes and turn, and of where
/// the page is and its number only what the tokens read. A generator lays
/// out pages alike in it alike.
pub(crate) fn layout_reads(needs: CtxNeeds, ctx: &PageCtx) -> u128 {
    let CtxNeeds {
        index,
        count,
        label,
        bates,
    } = needs;
    let mut sum = Sum(Checksum::new());
    sum.feed(&ctx.geom);
    if index {
        sum.feed(&ctx.position);
    }
    if count {
        sum.feed(&ctx.count);
    }
    if label {
        sum.feed(&ctx.label);
    }
    if bates {
        sum.feed(&ctx.bates);
    }
    sum.0.finish()
}

/// What a page draws under some inputs, by the records it's drawn from.
struct Drawn<'a> {
    def: &'a PageDef,
    geom: &'a Geom,
    /// The annotations of the kinds the inputs draw, in `/Annots` order.
    annots: Vec<DrawnAnnot<'a>>,
    /// The form, when a widget is among them.
    form: Option<&'a FormDoc>,
    /// The generators on the page, when the inputs draw them.
    generators: Vec<OnPage<'a>>,
    /// The blocks of the document's generators its base holds, by their
    /// generators, in order: set aside unless they're what's drawn.
    backed: Vec<u64>,
}

/// An annotation the inputs draw, hidden or not.
struct DrawnAnnot<'a> {
    id: AnnotId,
    body: &'a AnnotBody,
    /// A widget's terminal field.
    term: Option<(FieldId, Option<&'a Terminal>)>,
    /// Where another tool's link leads: one to a page the document hasn't
    /// got isn't drawn ([`LinkTo::Nowhere`]).
    to: LinkTo,
}

/// A generator on the page.
struct OnPage<'a> {
    generator: &'a Generator,
    /// The page's base draws it already, so the page doesn't again.
    base_draws: bool,
    needs: CtxNeeds,
}

impl<'a> Drawn<'a> {
    /// What page `page` of `st` draws under `inputs`: None when it isn't
    /// one of its pages.
    fn of(st: &'a DocState, page: PageId, inputs: &ViewInputs) -> Option<Drawn<'a>> {
        let RenderTables {
            pages,
            geoms,
            page_annots,
            annots,
            generators,
            fields,
            form,
        } = st.render_tables();
        let def = pages.get(&page)?;
        let geom = geoms.get(&page)?;
        let base = || {
            let shown = Shown {
                src: def.base.src.clone(),
                index: def.base.index,
                copy: def.copy,
            };
            base_annots(&shown)
        };
        let mut drawn = Vec::new();
        for &id in page_annots.get(&page).into_iter().flatten() {
            let Some(rec) = annots.get(&id) else {
                continue;
            };
            let (term, to) = match &rec.body {
                AnnotBody::Mark(_) if inputs.marks == MarkDraw::ByEngine => {
                    (None, LinkTo::Elsewhere)
                }
                AnnotBody::Foreign(f) if inputs.foreign => {
                    (None, LinkTo::of(st, &def.base.src, base, f))
                }
                AnnotBody::Widget(w) if inputs.widgets => {
                    let node = fields.get(&w.field);
                    let term = node.and_then(|node| node.term.as_deref());
                    (Some((w.field, term)), LinkTo::Elsewhere)
                }
                AnnotBody::Mark(_) | AnnotBody::Foreign(_) | AnnotBody::Widget(_) => continue,
            };
            let body = &rec.body;
            drawn.push(DrawnAnnot { id, body, term, to });
        }
        let widgets = drawn.iter().any(|a| a.term.is_some());
        let mut on = Vec::new();
        let base = &def.base;
        // Whether the blocks of the document's generators the base holds are
        // the generators on the page, in place around its isolated content
        // and the recognized text it holds (`content::intact`), and which
        // those blocks are, which are set aside when they aren't.
        let marks = base.src.markers(base.index);
        let on_page = match inputs.generators {
            true => content::generators_on(st, page),
            false => Vec::new(),
        };
        let contents = base.src.page(base.index).map_or(&[][..], |f| &f.contents);
        let intact = (on_page.is_empty() && marks.is_empty())
            || content::intact(st, page, &marks, contents, &on_page, |g| {
                content::model_layout_digest(st, g, page, geom)
            });
        let backed = (marks.iter())
            .filter_map(|(_, m)| match m {
                Marker::Gen { id, .. } if st.backs(*id, page) => Some(id.0),
                _ => None,
            })
            .collect();
        for g in generators.values().filter(|_| inputs.generators) {
            if !g.covers(page) {
                continue;
            }
            on.push(OnPage {
                generator: g,
                base_draws: intact,
                needs: g.needs(),
            });
        }
        Some(Drawn {
            def,
            geom,
            annots: drawn,
            form: widgets.then_some(&**form),
            generators: on,
            backed,
        })
    }
}

/// A digest being made, each value fed in an encoding no two values
/// share: a length before what it counts, a tag before each variant.
struct Sum(Checksum);

impl Sum {
    fn feed<T: Feed + ?Sized>(&mut self, v: &T) -> &mut Sum {
        v.feed(self);
        self
    }
}

impl std::fmt::Write for Sum {
    fn write_str(&mut self, s: &str) -> std::fmt::Result {
        self.0.write(s.as_bytes());
        Ok(())
    }
}

/// What a value puts in a digest: what it's drawn with, never what it's
/// called. Each record is destructured whole, so a field added to one
/// fails to compile here until it's fed or said not to draw.
trait Feed {
    fn feed(&self, sum: &mut Sum);
}

macro_rules! le_bytes {
    ($($t:ty),*) => {
        $(impl Feed for $t {
            fn feed(&self, sum: &mut Sum) {
                sum.0.write(&self.to_le_bytes());
            }
        })*
    };
}

le_bytes!(u8, u16, u32, u64, u128, i64);

impl Feed for usize {
    fn feed(&self, sum: &mut Sum) {
        sum.feed(&(*self as u64));
    }
}

impl Feed for bool {
    fn feed(&self, sum: &mut Sum) {
        sum.feed(&u8::from(*self));
    }
}

impl Feed for f32 {
    fn feed(&self, sum: &mut Sum) {
        sum.feed(&self.to_bits());
    }
}

impl Feed for str {
    fn feed(&self, sum: &mut Sum) {
        sum.feed(&self.len());
        sum.0.write(self.as_bytes());
    }
}

impl Feed for String {
    fn feed(&self, sum: &mut Sum) {
        sum.feed(self.as_str());
    }
}

impl<T: Feed + ?Sized> Feed for &T {
    fn feed(&self, sum: &mut Sum) {
        (**self).feed(sum);
    }
}

impl<T: Feed + ?Sized> Feed for Arc<T> {
    fn feed(&self, sum: &mut Sum) {
        (**self).feed(sum);
    }
}

impl<T: Feed> Feed for Option<T> {
    fn feed(&self, sum: &mut Sum) {
        match self {
            None => sum.feed(&0u8),
            Some(v) => sum.feed(&1u8).feed(v),
        };
    }
}

impl<T: Feed> Feed for [T] {
    fn feed(&self, sum: &mut Sum) {
        sum.feed(&self.len());
        for v in self {
            sum.feed(v);
        }
    }
}

impl<T: Feed> Feed for Vec<T> {
    fn feed(&self, sum: &mut Sum) {
        sum.feed(self.as_slice());
    }
}

impl<T: Feed, const N: usize> Feed for [T; N] {
    fn feed(&self, sum: &mut Sum) {
        for v in self {
            sum.feed(v);
        }
    }
}

impl<T: Feed + Clone> Feed for im::Vector<T> {
    fn feed(&self, sum: &mut Sum) {
        sum.feed(&self.len());
        for v in self {
            sum.feed(v);
        }
    }
}

impl<T: Feed> Feed for BTreeSet<T> {
    fn feed(&self, sum: &mut Sum) {
        sum.feed(&self.len());
        for v in self {
            sum.feed(v);
        }
    }
}

impl<A: Feed, B: Feed> Feed for (A, B) {
    fn feed(&self, sum: &mut Sum) {
        sum.feed(&self.0).feed(&self.1);
    }
}

impl Feed for Range<usize> {
    fn feed(&self, sum: &mut Sum) {
        sum.feed(&self.start).feed(&self.end);
    }
}

impl Feed for Rotation {
    fn feed(&self, sum: &mut Sum) {
        sum.feed(&(self.degrees() as u16));
    }
}

impl Feed for FieldId {
    fn feed(&self, sum: &mut Sum) {
        sum.feed(&self.0);
    }
}

impl Feed for BlobId {
    fn feed(&self, sum: &mut Sum) {
        let BlobId { hash, len } = self;
        sum.feed(hash).feed(len);
    }
}

impl Feed for Src {
    fn feed(&self, sum: &mut Sum) {
        sum.feed(&self.id());
    }
}

impl Feed for BlobRef {
    fn feed(&self, sum: &mut Sum) {
        sum.feed(&self.id());
    }
}

impl Feed for ObjRef {
    fn feed(&self, sum: &mut Sum) {
        let ObjRef { src, id } = self;
        sum.feed(src).feed(id);
    }
}

impl Feed for Pt {
    fn feed(&self, sum: &mut Sum) {
        let Pt { x, y } = self;
        sum.feed(x).feed(y);
    }
}

impl Feed for Box2 {
    fn feed(&self, sum: &mut Sum) {
        let Box2 { x, y, w, h } = self;
        sum.feed(x).feed(y).feed(w).feed(h);
    }
}

impl Feed for Rect {
    fn feed(&self, sum: &mut Sum) {
        let Rect { x0, y0, x1, y1 } = self;
        sum.feed(x0).feed(y0).feed(x1).feed(y1);
    }
}

/// All of it, its turn too: what a generator is laid out in.
impl Feed for Geom {
    fn feed(&self, sum: &mut Sum) {
        feed_boxes(sum, self);
        sum.feed(&self.rotate);
    }
}

/// Its boxes: all of it but its turn, which a render's job turns.
fn feed_boxes(sum: &mut Sum, geom: &Geom) {
    let Geom {
        media,
        crop,
        trim,
        bleed,
        art,
        rotate: _,
        user_unit,
    } = geom;
    sum.feed(media)
        .feed(crop)
        .feed(trim)
        .feed(bleed)
        .feed(art)
        .feed(user_unit);
}

impl Feed for PageBase {
    fn feed(&self, sum: &mut Sum) {
        let PageBase { src, index } = self;
        sum.feed(src).feed(index);
    }
}

impl Feed for ContentStack {
    fn feed(&self, sum: &mut Sum) {
        let ContentStack { filters, over } = self;
        sum.feed(filters).feed(over);
    }
}

/// Its id too, which its block names.
impl Feed for Layer {
    fn feed(&self, sum: &mut Sum) {
        let Layer { id, body } = self;
        sum.feed(&id.0);
        match body {
            Static::OcrText(text) => sum.feed(&0u8).feed(text),
            Static::Placements(placed) => sum.feed(&1u8).feed(placed),
        };
    }
}

impl Feed for Placement {
    fn feed(&self, sum: &mut Sum) {
        let Placement { form, matrix, tag } = self;
        let tag = match tag {
            PlaceTag::Artifact => 0u8,
            PlaceTag::Untagged => 1,
        };
        sum.feed(form).feed(matrix).feed(&tag);
    }
}

impl Feed for Affine {
    fn feed(&self, sum: &mut Sum) {
        let Affine { a, b, c, d, e, f } = self;
        sum.feed(a).feed(b).feed(c).feed(d).feed(e).feed(f);
    }
}

/// All of it, as [`Annot`]'s feed has a mark's look.
impl Feed for MarkAp {
    fn feed(&self, sum: &mut Sum) {
        let MarkAp {
            kind,
            color,
            opacity,
            stroke,
            contents,
            fill,
            dashed,
            cloudy,
            turn,
            stamp,
        } = self;
        _ = write!(sum, "{kind:?}");
        sum.feed(&0xffu8);
        sum.feed(color)
            .feed(opacity)
            .feed(stroke)
            .feed(contents)
            .feed(fill)
            .feed(dashed)
            .feed(cloudy)
            .feed(turn)
            .feed(stamp);
    }
}

impl Feed for TextLayer {
    fn feed(&self, sum: &mut Sum) {
        let TextLayer {
            words,
            lines,
            upright,
            engine,
        } = self;
        sum.feed(words).feed(lines).feed(upright).feed(engine);
    }
}

impl Feed for OcrWord {
    fn feed(&self, sum: &mut Sum) {
        let OcrWord {
            text,
            rect,
            confidence,
        } = self;
        sum.feed(text).feed(rect).feed(confidence);
    }
}

/// Its mark and what it holds: the object it was read as, whose
/// appearance it keeps while it's as read, its file, and its stamp's page.
impl Feed for Mark {
    fn feed(&self, sum: &mut Sum) {
        let Mark {
            annot,
            src,
            file,
            stamp,
        } = self;
        sum.feed(annot).feed(src).feed(file).feed(stamp);
    }
}

impl Feed for Annot {
    fn feed(&self, sum: &mut Sum) {
        let Annot {
            id: _,
            kind,
            color,
            opacity,
            stroke,
            author,
            contents,
            modified,
            created,
            fill,
            dashed,
            cloudy,
            turn,
            name,
        } = self;
        // Its kind as its derived Debug prints it, every field of every
        // kind, then a byte no text holds. Marks are fed for plugins'
        // renders alone, which pay for it.
        _ = write!(sum, "{kind:?}");
        sum.feed(&0xffu8);
        sum.feed(color)
            .feed(opacity)
            .feed(stroke)
            .feed(author)
            .feed(contents)
            .feed(modified)
            .feed(created)
            .feed(fill)
            .feed(dashed)
            .feed(cloudy)
            .feed(turn)
            .feed(name);
    }
}

impl Feed for FormRef {
    fn feed(&self, sum: &mut Sum) {
        match self {
            FormRef::Base(at) => sum.feed(&0u8).feed(at),
            FormRef::Blob(pdf) => sum.feed(&1u8).feed(pdf),
            FormRef::Pinned(ap) => sum.feed(&2u8).feed(&ap.digest),
        };
    }
}

/// What it's drawn from: its object, or for a dictionary written into its
/// page's `/Annots`, its place there. Which copy it is draws alike, and
/// what it says is read from there (a copy's thread renamed apart draws
/// alike too).
impl Feed for Foreign {
    fn feed(&self, sum: &mut Sum) {
        let Foreign {
            src,
            copy: _,
            summary,
        } = self;
        let ForeignAnnot {
            subtype: _,
            author: _,
            contents: _,
            modified: _,
            rect: _,
            name: _,
            parent: _,
            grouped: _,
            state: _,
            created: _,
            slot,
            hidden: _,
            color: _,
            mark: _,
            stamp: _,
        } = &**summary;
        sum.feed(src).feed(slot);
    }
}

/// Where it is and how it looks; its field's terminal is fed beside it,
/// and which copy of its form it's in draws alike.
impl Feed for Widget {
    fn feed(&self, sum: &mut Sum) {
        let Widget {
            field: _,
            src,
            inst,
            merged,
            rect,
            flags,
            mk,
            da,
            state,
            states,
            props,
        } = self;
        let FormInst { src: from, copy: _ } = inst;
        sum.feed(src)
            .feed(from)
            .feed(merged)
            .feed(rect)
            .feed(flags)
            .feed(mk)
            .feed(da)
            .feed(state)
            .feed(states)
            .feed(props);
    }
}

impl Feed for Mk {
    fn feed(&self, sum: &mut Sum) {
        let Mk {
            background,
            border_color,
            rotation,
            caption,
        } = self;
        sum.feed(background)
            .feed(border_color)
            .feed(rotation)
            .feed(caption);
    }
}

impl Feed for Color {
    fn feed(&self, sum: &mut Sum) {
        match self {
            Color::Gray(g) => sum.feed(&0u8).feed(g),
            Color::Rgb(rgb) => sum.feed(&1u8).feed(rgb),
            Color::Cmyk(cmyk) => sum.feed(&2u8).feed(cmyk),
        };
    }
}

impl Feed for WidgetProps {
    fn feed(&self, sum: &mut Sum) {
        let WidgetProps {
            border,
            actions,
            has_appearance,
        } = self;
        sum.feed(border).feed(actions).feed(has_appearance);
    }
}

impl Feed for Border {
    fn feed(&self, sum: &mut Sum) {
        let Border { width, style } = self;
        sum.feed(width).feed(&(*style as u8));
    }
}

impl Feed for Trigger {
    fn feed(&self, sum: &mut Sum) {
        sum.feed(&(*self as u8));
    }
}

impl Feed for Action {
    fn feed(&self, sum: &mut Sum) {
        let Action { kind, script } = self;
        sum.feed(kind).feed(script);
    }
}

/// What it holds and how it's shown; which widgets show it are ids, and a
/// duplicate's are others.
impl Feed for Terminal {
    fn feed(&self, sum: &mut Sum) {
        let Terminal {
            kind,
            flags,
            value,
            default,
            props,
            widgets: _,
            outside,
        } = self;
        sum.feed(&(*kind as u8))
            .feed(flags)
            .feed(value)
            .feed(default)
            .feed(props)
            .feed(outside);
    }
}

impl Feed for Value {
    fn feed(&self, sum: &mut Sum) {
        match self {
            Value::Text(text) => sum.feed(&0u8).feed(text),
            Value::State(state) => sum.feed(&1u8).feed(state),
            Value::Items(items) => sum.feed(&2u8).feed(items),
            Value::Signed => sum.feed(&3u8),
        };
    }
}

impl Feed for FieldProps {
    fn feed(&self, sum: &mut Sum) {
        let FieldProps {
            da,
            align,
            max_len,
            options,
            tooltip,
            actions,
        } = self;
        sum.feed(da)
            .feed(align)
            .feed(max_len)
            .feed(options)
            .feed(tooltip)
            .feed(actions);
    }
}

impl Feed for Choice {
    fn feed(&self, sum: &mut Sum) {
        let Choice { export, shown } = self;
        sum.feed(export).feed(shown);
    }
}

impl Feed for FormDoc {
    fn feed(&self, sum: &mut Sum) {
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
        sum.feed(roots).feed(&names.len());
        for (inst, name) in names {
            sum.feed(inst).feed(name);
        }
        sum.feed(scripts)
            .feed(calc_order)
            .feed(da)
            .feed(align)
            .feed(need_appearances)
            .feed(sig_flags)
            .feed(xfa);
    }
}

impl Feed for FormInst {
    fn feed(&self, sum: &mut Sum) {
        let FormInst { src, copy } = self;
        sum.feed(src).feed(copy);
    }
}

impl Feed for ScriptState {
    fn feed(&self, sum: &mut Sum) {
        match self {
            ScriptState::On => sum.feed(&0u8),
            ScriptState::Off { instances } => sum.feed(&1u8).feed(instances),
        };
    }
}

impl Feed for XfaState {
    fn feed(&self, sum: &mut Sum) {
        let XfaState {
            present,
            dynamic,
            values,
        } = self;
        sum.feed(present).feed(dynamic).feed(values);
    }
}

/// What it draws, and where on each page.
impl Feed for WatermarkGen {
    fn feed(&self, sum: &mut Sum) {
        let WatermarkGen { content, position } = self;
        sum.feed(content).feed(&(*position as u8));
    }
}

impl Feed for WatermarkContent {
    fn feed(&self, sum: &mut Sum) {
        match self {
            WatermarkContent::Text {
                text,
                font,
                size,
                color,
                opacity,
                angle,
            } => sum
                .feed(&0u8)
                .feed(text)
                .feed(&(*font as u8))
                .feed(size)
                .feed(color)
                .feed(opacity)
                .feed(angle),
            WatermarkContent::Image {
                image,
                scale,
                opacity,
            } => sum.feed(&1u8).feed(image).feed(scale).feed(opacity),
            WatermarkContent::Page {
                page,
                scale,
                opacity,
            } => sum.feed(&2u8).feed(page).feed(scale).feed(opacity),
        };
    }
}

impl Feed for ImageRef {
    fn feed(&self, sum: &mut Sum) {
        match self {
            ImageRef::Blob(blob) => sum.feed(&0u8).feed(blob),
            ImageRef::Placed(placed) => sum.feed(&1u8).feed(placed),
        };
    }
}

impl Feed for Placed {
    fn feed(&self, sum: &mut Sum) {
        let Placed { at, res, turns } = self;
        sum.feed(at).feed(&res.0).feed(turns);
    }
}

impl Feed for Filter {
    fn feed(&self, sum: &mut Sum) {
        match self {
            Filter::DropMarked(sel) => sum.feed(sel),
            Filter::DropStreams(StreamSel::SheafGen(g)) => sum.feed(&1u8).feed(&g.0),
            Filter::DropStreams(StreamSel::SheafOcr) => sum.feed(&4u8),
            Filter::StripLegacy(LegacySel::Marked(sel)) => sum.feed(&3u8).feed(sel),
            Filter::StripLegacy(LegacySel::Streams) => sum.feed(&6u8),
            Filter::StripLegacy(LegacySel::Resources) => sum.feed(&7u8),
        };
    }
}

/// Fed as its [`Filter::DropMarked`]'s, 0, 2 and 5 the tags they share
/// with [`Filter::DropStreams`]' 1 and 4 and [`Filter::StripLegacy`]'s 3,
/// 6 and 7.
impl Feed for MarkedSel {
    fn feed(&self, sum: &mut Sum) {
        match self {
            MarkedSel::SheafGen(g) => sum.feed(&0u8).feed(&g.0),
            MarkedSel::Pagination { kinds, origins } => sum.feed(&2u8).feed(kinds).feed(origins),
            MarkedSel::SheafOcr => sum.feed(&5u8),
        };
    }
}

impl Feed for PageForm {
    fn feed(&self, sum: &mut Sum) {
        match self {
            PageForm::Blob { pdf, index } => sum.feed(&0u8).feed(pdf).feed(index),
            PageForm::Placed(placed) => sum.feed(&1u8).feed(placed),
        };
    }
}

impl Feed for PaginationKind {
    fn feed(&self, sum: &mut Sum) {
        sum.feed(&(*self as u8));
    }
}

impl Feed for PagOrigin {
    fn feed(&self, sum: &mut Sum) {
        sum.feed(&(*self as u8));
    }
}

/// What it draws on each page it's on, but not which pages those are,
/// which only [`Drawn`] asks.
impl Feed for HeaderFooterSpec {
    fn feed(&self, sum: &mut Sum) {
        let HeaderFooterSpec {
            texts,
            font,
            size,
            color,
            margins,
            first_number,
            bates,
        } = self;
        let Margins {
            top,
            bottom,
            left,
            right,
        } = margins;
        sum.feed(texts)
            .feed(&(*font as u8))
            .feed(size)
            .feed(color)
            .feed(top)
            .feed(bottom)
            .feed(left)
            .feed(right)
            .feed(first_number)
            .feed(bates);
    }
}

impl Feed for BatesSpec {
    fn feed(&self, sum: &mut Sum) {
        let BatesSpec {
            prefix,
            suffix,
            digits,
            start,
        } = self;
        sum.feed(prefix).feed(suffix).feed(digits).feed(start);
    }
}
