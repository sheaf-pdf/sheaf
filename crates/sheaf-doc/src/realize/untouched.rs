//! Which pages draw as their base pages do (M10.5 §10.1): those the
//! source's base engine renders, turned by their job, while any other is
//! realized in its group's overlay. [`untouched`] is a model predicate,
//! O(the annotations on the page), with the overlay's rules for what it
//! writes as the base has it; [`emission_is_base`] checks it against what
//! an overlay of the page writes, for tests.
//!
//! A page is untouched under some inputs when it's copy 0 of its base
//! page, at the page's own object, with its boxes (its turn is the job's),
//! no filter on its content, nothing drawn over it that the inputs draw
//! (recognized text with `ocr_over`; with `generators`, the header and
//! footer where its file doesn't draw it already), its own content as the
//! emitter keeps it as it is ([`content::intact`]: the blocks of the
//! document's generators its file holds, by their streams' markers, the
//! watermarks and backgrounds on it laid out alike there, which with
//! `generators` must hold one for each, and without hold none), and, as
//! far as the render's flags draw them ([`ViewInputs::draws_annots`] the
//! annotations, `widgets` the form's widgets), the annotations of its base
//! page as they are: each drawn record as read from its own entry there,
//! none hidden or dragged, no entry left out (a record deleted, or a kind
//! the inputs leave to the screen, as sheaf's marks under
//! [`MarkDraw::ByHost`]), each widget's `/Parent`s as the model's field
//! tree has them, and the form's own keys the overlay writes from the
//! document's as its file has them. A field's value isn't one of them: no
//! save writes it before M10 slice 9's fills.

use std::collections::HashSet;

use lopdf::{Object, ObjectId};

use super::{MarkDraw, ViewInputs};
use crate::annot::AnnotId;
use crate::file::{PageAnnots, is_ours};
use crate::forms::FieldId;
use crate::geom::Geom;
use crate::materialize::emit::Shown;
use crate::materialize::emit::annots::{dead_link, derived, mark_kept};
use crate::materialize::emit::content;
use crate::model::{AnnotBody, DocState, FormInst, PageDef, PageId, Static, Widget};
use crate::source::{ObjRef, PageFacts};
use crate::stamps;
use crate::store::Src;

/// Whether page `page` of `st` draws under `inputs` as its base page does
/// in its source's own file: what the base engine renders it from. False
/// for a page `st` hasn't got.
pub fn untouched(st: &DocState, page: PageId, inputs: &ViewInputs) -> bool {
    let (Some(def), Some(geom)) = (st.def(page), st.geom(page)) else {
        return false;
    };
    let src = &def.base.src;
    let Some(facts) = src.page(def.base.index) else {
        return false;
    };
    // At its base page's own object, which an overlay writes it over.
    if def.copy != 0 || facts.repeat != 0 {
        return false;
    }
    let drawn_over = !def.content.filters.is_empty()
        || (def.content.over.iter()).any(|layer| match &layer.body {
            Static::OcrText(_) => inputs.ocr_over,
        });
    let boxes_kept = geom.media == facts.geom.media
        && geom.crop == facts.geom.crop
        && geom.trim == facts.geom.trim
        && geom.bleed == facts.geom.bleed
        && geom.art == facts.geom.art
        && geom.user_unit == facts.geom.user_unit;
    !drawn_over
        && boxes_kept
        && !generated(st, page, def, geom, facts, inputs)
        && annots_kept(st, page, def, facts, inputs)
}

/// Whether a generator draws on page `page`, showing `def` with `geom`,
/// under `inputs`, or the blocks of one its base page `facts` holds are
/// set aside: the watermarks and backgrounds on it aren't the blocks its
/// file holds ([`content::intact`]), or the header and footer covers the
/// page, its file doesn't draw it already, and a text of it isn't blank
/// there.
fn generated(
    st: &DocState,
    page: PageId,
    def: &PageDef,
    geom: &Geom,
    facts: &PageFacts,
    inputs: &ViewInputs,
) -> bool {
    let on = match inputs.generators {
        true => content::generators_on(st, page),
        false => Vec::new(),
    };
    let marks = def.base.src.markers(def.base.index);
    if !(on.is_empty() && marks.is_empty())
        && !content::intact(st, page, &marks, &facts.contents, &on, |g| {
            content::model_layout_digest(g, geom)
        })
    {
        return true;
    }
    if !inputs.generators {
        return false;
    }
    let Some(hf) = st.header_footer() else {
        return false;
    };
    if !st.header_covers(page) || stamps::base_draws(&def.base.src, def.base.index, hf) {
        return false;
    }
    let facts = stamps::PageFacts {
        index: st.pos_of(page).unwrap_or(0),
        count: st.page_count(),
        bates: st.bates_of(page),
    };
    hf.draws(&facts)
}

/// Whether page `page`'s annotations, its base page `facts` of `def`'s
/// source, draw as that page's do under `inputs` (the module's rules).
fn annots_kept(
    st: &DocState,
    page: PageId,
    def: &PageDef,
    facts: &PageFacts,
    inputs: &ViewInputs,
) -> bool {
    let (annots, widgets) = (inputs.draws_annots(), inputs.widgets);
    if !annots && !widgets {
        return true;
    }
    let src = &def.base.src;
    let base = &facts.annots;
    let shown = Shown {
        src: src.clone(),
        index: def.base.index,
        copy: 0,
        content: None,
    };
    let objects: HashSet<ObjectId> = base.iter().filter_map(|e| e.as_reference().ok()).collect();
    let listed = |id: ObjectId| objects.contains(&id);
    // The base's entries each record is read from, by object, and by place
    // for one written into `/Annots`; and of those, what the overlay draws
    // as the base has it.
    let (mut recorded, mut recorded_slots) = (HashSet::new(), HashSet::new());
    let (mut kept, mut kept_slots) = (HashSet::new(), HashSet::new());
    let mut form = false;
    for &aid in st.annot_ids_on(page).into_iter().flatten() {
        let Some(rec) = st.annot(aid) else {
            continue;
        };
        let (read, drawn) = match &rec.body {
            AnnotBody::Mark(m) => (
                own(&m.src, src),
                annots && inputs.marks == MarkDraw::ByEngine,
            ),
            AnnotBody::Foreign(f) => (own(&f.src, src), annots && inputs.foreign),
            AnnotBody::Widget(w) => (own(&w.src, src), widgets),
        };
        match (read, &rec.body) {
            (Some(r), _) => _ = recorded.insert(r.id),
            (None, AnnotBody::Foreign(f)) if f.src.is_none() => {
                _ = recorded_slots.insert(f.summary.slot)
            }
            (None, _) => {}
        }
        if !drawn {
            continue;
        }
        if inputs.hidden.contains(&aid) || inputs.dragging.contains(&aid) {
            return false;
        }
        match &rec.body {
            AnnotBody::Mark(m) => {
                let Some(r) = read.filter(|r| listed(r.id)) else {
                    return false;
                };
                let parent = (m.annot.thread_parent())
                    .and_then(|name| st.annot_by_nm(page, name))
                    .and_then(|pid| drawn_at(st, src, pid, inputs));
                let Some(as_read) = mark_kept(&shown, base, m, Some(facts.id), parent) else {
                    return false;
                };
                kept.insert(r.id);
                kept.extend(
                    as_read
                        .companions
                        .iter()
                        .filter_map(|c| c.as_reference().ok()),
                );
            }
            AnnotBody::Foreign(f) => {
                if f.copy != 0 || dead_link(st, src, || base.to_vec(), f) {
                    return false;
                }
                match (&f.src, read) {
                    (Some(_), Some(r)) if listed(r.id) => _ = kept.insert(r.id),
                    (None, _)
                        if matches!(base.get(f.summary.slot), Some(Object::Dictionary(_))) =>
                    {
                        _ = kept_slots.insert(f.summary.slot)
                    }
                    _ => return false,
                }
            }
            AnnotBody::Widget(w) => {
                let Some(r) = read.filter(|r| listed(r.id) && w.inst.copy == 0) else {
                    return false;
                };
                // One object with its field, which shows elsewhere too: the
                // two are written apart.
                let term = st.field(w.field).and_then(|node| node.term.as_ref());
                if w.merged && term.is_some_and(|t| t.widgets.len() > 1) {
                    return false;
                }
                if !tree_kept(st, src, w, r.id) {
                    return false;
                }
                kept.insert(r.id);
                form = true;
            }
        }
    }
    // The form's own keys, which the overlay writes from the document's
    // over a form the state models.
    let file = src.form();
    let modeled = file.facts.damaged.is_none()
        && (file.fields.is_empty() || {
            let inst = FormInst {
                src: src.id(),
                copy: 0,
            };
            st.form().names.contains_key(&inst)
        });
    let doc = st.form();
    if form
        && modeled
        && (doc.da != file.facts.da
            || doc.align != file.facts.align
            || doc.need_appearances != file.facts.need_appearances)
    {
        return false;
    }
    // Every entry of the base page the render draws is drawn as it is.
    let pdf = src.pdf();
    let entries = PageAnnots::new(base);
    let modeled_slots: HashSet<usize> = (src.annots().on(def.base.index).iter())
        .map(|a| a.slot)
        .chain(
            (file.widgets.get(&def.base.index).into_iter().flatten())
                .filter(|_| modeled)
                .map(|w| w.slot),
        )
        .collect();
    for (slot, entry) in base.iter().enumerate() {
        let held = match entry {
            Object::Reference(r) => kept.contains(r),
            _ => kept_slots.contains(&slot),
        };
        let Some(dict) = (!held)
            .then(|| pdf.dereference(entry).ok())
            .flatten()
            .and_then(|(_, o)| o.as_dict().ok())
        else {
            continue;
        };
        let subtype = dict.get(b"Subtype").and_then(Object::as_name).ok();
        let widget = subtype == Some(b"Widget".as_slice());
        // PDFium draws no pop-up from `/Annots`, and the render's flags
        // leave out the kinds they don't draw.
        let drawn = match subtype {
            Some(b"Popup") => false,
            Some(b"Widget") => widgets,
            _ => annots,
        };
        if !drawn {
            continue;
        }
        let of_record = match entry {
            Object::Reference(r) => recorded.contains(r),
            _ => recorded_slots.contains(&slot),
        };
        // A record's that the overlay doesn't draw as it is, one deleted,
        // or sheaf's own, written with its mark alone.
        if of_record || modeled_slots.contains(&slot) || is_ours(pdf, entry, &entries) {
            return false;
        }
        // One the records don't hold, kept as the page has it but a derived
        // copy's widget, and another tool's when the view shows them.
        let overlaid = match widget {
            true => !derived(src),
            false => inputs.foreign,
        };
        if !overlaid {
            return false;
        }
    }
    true
}

/// Whether widget `w`, read as object `id` of `src`, hangs in its file's
/// field tree as the model hangs it: its `/Parent` (merged, its field's)
/// and each one up from there the object of the model's parent, read from
/// `src` at copy 0, and none at the root. The overlay writes them from the
/// model's tree, which a file's `/Kids` made where its `/Parent`s say
/// otherwise, and PDFium draws a widget with what it inherits up the
/// `/Parent`s.
fn tree_kept(st: &DocState, src: &Src, w: &Widget, id: ObjectId) -> bool {
    let pdf = src.pdf();
    // What `object`'s `/Parent` names; None when it isn't a reference.
    let parent = |object: ObjectId| match pdf.get_dictionary(object).map(|d| d.get(b"Parent")) {
        Ok(Err(_)) => Some(None),
        Ok(Ok(Object::Reference(r))) => Some(Some(*r)),
        _ => None,
    };
    let node = |field: FieldId| st.field(field).filter(|node| node.inst.copy == 0);
    let at = |field: FieldId| {
        node(field)
            .and_then(|node| own(&node.src, src))
            .map(|r| r.id)
    };
    // The object, and the field the model hangs it from.
    let (mut object, mut up) = match w.merged {
        true if at(w.field) != Some(id) => return false,
        true => (id, node(w.field).and_then(|node| node.parent)),
        false => (id, Some(w.field)),
    };
    loop {
        // The model's parent's object, none at the root; a parent sheaf
        // made, or another file's, is written anew.
        let wanted = match up.map(at) {
            None => None,
            Some(None) => return false,
            Some(found) => found,
        };
        if parent(object) != Some(wanted) {
            return false;
        }
        let (Some(field), Some(next)) = (up, wanted) else {
            return true;
        };
        object = next;
        up = node(field).and_then(|node| node.parent);
    }
}

/// `r`, the object a record was read as, when it's `src`'s.
fn own<'r>(r: &'r Option<ObjRef>, src: &Src) -> Option<&'r ObjRef> {
    r.as_ref().filter(|r| r.src == *src)
}

/// The number the overlay over `src` writes annotation `pid` at, when it
/// draws it under `inputs`: a mark's or another tool's comment's own
/// object there, copy 0 of it; None while it's drawn at a new one. None
/// when it isn't drawn: what a reply to it names ([`mark_kept`]'s
/// `parent`).
fn drawn_at(
    st: &DocState,
    src: &Src,
    pid: AnnotId,
    inputs: &ViewInputs,
) -> Option<Option<ObjectId>> {
    let rec = st.annot(pid)?;
    let at = |r: &Option<ObjRef>| own(r, src).map(|r| r.id);
    match &rec.body {
        AnnotBody::Mark(m) if inputs.marks == MarkDraw::ByEngine => Some(at(&m.src)),
        AnnotBody::Foreign(f) if inputs.foreign => Some(at(&f.src).filter(|_| f.copy == 0)),
        AnnotBody::Mark(_) | AnnotBody::Foreign(_) | AnnotBody::Widget(_) => None,
    }
}

/// Whether an overlay of page `page` of `st` alone, for `inputs`, writes
/// it as its base page is, as far as the render draws it: what
/// [`untouched`] models, emitted, for tests. The page at its own object,
/// with the base page's keys, and its inheritable ones as the base page
/// has them where it is; in `/Annots`, each annotation the render's flags
/// draw in the base's order, each the base's object as it is or made anew
/// differing from it only where it's placed (`/P`, its thread's `/IRT`
/// and `/RT`, `/NM` and its pop-up); with widgets, their fields so too but
/// for their `/Kids`, and the form's dictionary but for the fields it
/// lists, their calculation order and `/SigFlags`; and the catalog the
/// source's but for its page tree, its form and the layers the view turns
/// off.
#[cfg(any(test, feature = "fixtures"))]
pub fn emission_is_base(st: &DocState, page: PageId, inputs: &ViewInputs) -> anyhow::Result<bool> {
    use anyhow::Context as _;
    use in_progress::NoProgress;

    use crate::materialize::emit::{self, EmitBase, EmitScope, Overlay, RotatePolicy};

    let def = st.def(page).context("not a page of the state")?;
    let src = &def.base.src;
    let facts = (src.page(def.base.index)).context("its file hasn't got its base page")?;
    let overlay = Overlay {
        src: src.clone(),
        inputs,
        rotate: RotatePolicy::KeepBase,
        arts: None,
    };
    let scope = EmitScope::Pages([page].into());
    let e = emit::emit(st, scope, EmitBase::Overlay(&overlay), &[], &NoProgress)?;
    let emitted = emitted::Emitted::new(&e, src);
    Ok(emitted.page(page, facts, inputs) && emitted.frame(inputs))
}

/// [`emission_is_base`]'s comparison.
#[cfg(any(test, feature = "fixtures"))]
mod emitted {
    use std::borrow::Cow;
    use std::collections::HashMap;

    use lopdf::{Dictionary, Object, ObjectId};

    use super::ViewInputs;
    use crate::geom::{Geom, Rect};
    use crate::materialize::ObjKey;
    use crate::materialize::emit::{Emission, SymObject};
    use crate::model::PageId;
    use crate::source::PageFacts;
    use crate::store::Src;

    /// A page's keys an overlay writes for where it places the page.
    const PAGE_PLACED: &[&[u8]] = &[
        b"Type",
        b"Parent",
        b"Thumb",
        b"Annots",
        b"Resources",
        b"MediaBox",
        b"CropBox",
        b"Rotate",
    ];
    /// An annotation's keys that say where it's placed, which draw
    /// nothing.
    const ANNOT_PLACED: &[&[u8]] = &[b"P", b"IRT", b"RT", b"NM", b"Popup"];
    /// A field's, beside them: its kids, which an overlay lists only on its
    /// pages.
    const FIELD_PLACED: &[&[u8]] = &[b"P", b"IRT", b"RT", b"NM", b"Popup", b"Kids"];
    /// The form's, which an overlay lists or writes for its pages alone, or
    /// says otherwise as the same (`/Q`, `/NeedAppearances`).
    const FORM_PLACED: &[&[u8]] = &[b"Fields", b"CO", b"SigFlags", b"Q", b"NeedAppearances"];

    /// What an overlay's emission writes, compared with its source's
    /// objects.
    pub(super) struct Emitted<'a> {
        e: &'a Emission,
        src: &'a Src,
        written: HashMap<ObjKey, &'a SymObject>,
    }

    impl<'a> Emitted<'a> {
        pub(super) fn new(e: &'a Emission, src: &'a Src) -> Emitted<'a> {
            let written = e.objects.iter().map(|(key, obj)| (*key, obj)).collect();
            Emitted { e, src, written }
        }

        /// The number reference `r` names: a placeholder's key's, copy 0 of
        /// the source's object where it is; any other as it is.
        fn number(&self, r: ObjectId) -> Option<ObjectId> {
            match self.e.syms.key(r) {
                None => Some(r),
                Some(ObjKey::Base { blob, obj, copy: 0 }) if blob == self.src.id() => Some(obj),
                Some(key) => self.e.owned.get(&key).copied(),
            }
        }

        /// Whether `built`, a value the emission writes, is `read`, the
        /// source's: equal, each reference naming the same number.
        fn same(&self, built: &Object, read: &Object) -> bool {
            match (built, read) {
                (Object::Reference(b), Object::Reference(r)) => self.number(*b) == Some(*r),
                (Object::Array(a), Object::Array(b)) => {
                    a.len() == b.len() && a.iter().zip(b).all(|(a, b)| self.same(a, b))
                }
                (Object::Dictionary(a), Object::Dictionary(b)) => self.same_dict(a, b, &[]),
                (Object::Stream(a), Object::Stream(b)) => {
                    a.content == b.content && self.same_dict(&a.dict, &b.dict, &[b"Length"])
                }
                (Object::String(a, _), Object::String(b, _)) => a == b,
                (a, b) => a == b,
            }
        }

        /// [`same`](Self::same) for dictionaries, but for the keys `but`.
        fn same_dict(&self, built: &Dictionary, read: &Dictionary, but: &[&[u8]]) -> bool {
            let keys = |d: &Dictionary| -> Vec<Vec<u8>> {
                let mut keys: Vec<Vec<u8>> = (d.iter().map(|(k, _)| k.clone()))
                    .filter(|k| !but.contains(&k.as_slice()))
                    .collect();
                keys.sort();
                keys
            };
            keys(built) == keys(read)
                && (built.iter())
                    .filter(|(k, _)| !but.contains(&k.as_slice()))
                    .all(|(k, v)| read.get(k).is_ok_and(|r| self.same(v, r)))
        }

        /// The source's object `id`, as a dictionary.
        fn read(&self, id: ObjectId) -> Option<&'a Dictionary> {
            match self.src.pdf().get(id).ok()? {
                Object::Dictionary(d) => Some(d),
                Object::Stream(s) => Some(&s.dict),
                _ => None,
            }
        }

        /// The dictionary entry `o` of an emitted `/Annots` is: one the
        /// emission writes, or the source's object it names.
        fn entry(&self, o: &'a Object) -> Option<Cow<'a, Dictionary>> {
            let r = match o {
                Object::Dictionary(d) => return Some(Cow::Borrowed(d)),
                Object::Reference(r) => *r,
                _ => return None,
            };
            match self.e.syms.key(r).and_then(|key| self.written.get(&key)) {
                Some(SymObject::Built(Object::Dictionary(d))) => Some(Cow::Borrowed(d)),
                Some(SymObject::Raw(raw)) => Some(Cow::Owned(
                    raw.src.pdf().get_dictionary(raw.id).ok()?.clone(),
                )),
                Some(_) => None,
                None => self.read(self.number(r)?).map(Cow::Borrowed),
            }
        }

        /// A box as written, its numbers read through references.
        fn rect(&self, o: Option<&Object>) -> Option<Rect> {
            let pdf = self.src.pdf();
            let Some(Object::Array(items)) = o else {
                return None;
            };
            let [a, b, c, d] = items.as_slice() else {
                return None;
            };
            let number = |o: &Object| {
                let o = match o {
                    Object::Reference(r) => pdf.get(self.number(*r)?).ok()?,
                    o => o,
                };
                match o {
                    Object::Integer(i) => Some(*i as f32),
                    Object::Real(r) => Some(*r as f32),
                    _ => Some(0.0),
                }
            };
            Some(Rect::new(number(a)?, number(b)?, number(c)?, number(d)?))
        }

        /// Whether page `page` is written as its base page, `facts`, is.
        pub(super) fn page(&self, page: PageId, facts: &PageFacts, inputs: &ViewInputs) -> bool {
            let key = ObjKey::Page(page);
            let (Some(&at), Some(SymObject::Built(Object::Dictionary(built)))) =
                (self.e.owned.get(&key), self.written.get(&key))
            else {
                return false;
            };
            let Some(read) = self.read(facts.id).filter(|_| at == facts.id) else {
                return false;
            };
            // What it inherited, as it has it where it is.
            let none = Object::Dictionary(Dictionary::new());
            let resources = facts.resources.as_deref().unwrap_or(&none);
            let rotate = built.get(b"Rotate").and_then(Object::as_i64).unwrap_or(0);
            let geom = Geom::read(
                self.rect(built.get(b"MediaBox").ok()),
                self.rect(built.get(b"CropBox").ok()),
                rotate,
            );
            let was = &facts.geom;
            self.same_dict(built, read, PAGE_PLACED)
                && built
                    .get(b"Resources")
                    .is_ok_and(|r| self.same(r, resources))
                && (geom.media, geom.crop, geom.rotate) == (was.media, was.crop, was.rotate)
                && self.annots(built.get(b"Annots").ok(), &facts.annots, inputs)
        }

        /// Whether `built`, an emitted page's `/Annots`, draws as `base`, its
        /// base page's, does under `inputs`: the entries the render draws,
        /// the same objects in the same order, each as the base has it.
        fn annots(
            &self,
            built: Option<&'a Object>,
            base: &'a [Object],
            inputs: &ViewInputs,
        ) -> bool {
            let pdf = self.src.pdf();
            let drawn = |d: &Dictionary| match d.get(b"Subtype").and_then(Object::as_name).ok() {
                Some(b"Popup") => false,
                Some(b"Widget") => inputs.widgets,
                _ => inputs.draws_annots(),
            };
            let items = built.and_then(|a| a.as_array().ok());
            let mut ours = Vec::new();
            for o in items.into_iter().flatten() {
                match self.entry(o) {
                    Some(d) if !drawn(&d) => {}
                    Some(d) => ours.push((o, d)),
                    None => return false,
                }
            }
            let theirs: Vec<(&Object, &Dictionary)> = (base.iter())
                .filter_map(|o| Some((o, pdf.dereference(o).ok()?.1.as_dict().ok()?)))
                .filter(|(_, d)| drawn(d))
                .collect();
            ours.len() == theirs.len()
                && ours
                    .iter()
                    .zip(&theirs)
                    .all(|((o, d), (r, read))| match (o, r) {
                        (Object::Reference(o), Object::Reference(r)) => {
                            let placed = match self.e.syms.key(*o) {
                                Some(ObjKey::Field(_)) => FIELD_PLACED,
                                _ => ANNOT_PLACED,
                            };
                            self.number(*o) == Some(*r) && self.same_dict(d, read, placed)
                        }
                        (Object::Dictionary(_), Object::Dictionary(_)) => {
                            self.same_dict(d, read, ANNOT_PLACED)
                        }
                        _ => false,
                    })
        }

        /// Whether the catalog, and with widgets the form and its fields,
        /// are the source's as far as they draw.
        pub(super) fn frame(&self, inputs: &ViewInputs) -> bool {
            let pdf = self.src.pdf();
            let Ok(catalog) = pdf.catalog() else {
                return false;
            };
            let layers =
                (inputs.ocg_off.as_ref()).is_some_and(|ocg| ocg.off.contains_key(&self.src.id()));
            let placed: &[&[u8]] = match layers {
                true => &[b"Pages", b"AcroForm", b"OCProperties"],
                false => &[b"Pages", b"AcroForm"],
            };
            let form = (catalog.get(b"AcroForm").ok())
                .and_then(|f| pdf.dereference(f).ok())
                .and_then(|(_, f)| f.as_dict().ok());
            self.written.iter().all(|(key, obj)| {
                let built = match obj {
                    SymObject::Built(Object::Dictionary(d)) => d,
                    _ => return true,
                };
                match key {
                    ObjKey::Catalog => self.same_dict(built, catalog, placed),
                    ObjKey::AcroForm if inputs.widgets => form.is_some_and(|read| {
                        let q = |d: &Dictionary| d.get(b"Q").and_then(Object::as_i64).unwrap_or(0);
                        let needs = |d: &Dictionary| {
                            d.get(b"NeedAppearances")
                                .and_then(Object::as_bool)
                                .unwrap_or(false)
                        };
                        self.same_dict(built, read, FORM_PLACED)
                            && q(built) == q(read)
                            && needs(built) == needs(read)
                    }),
                    ObjKey::Field(_) if inputs.widgets => {
                        let at = self.e.owned.get(key).copied();
                        (at.and_then(|at| self.read(at)))
                            .is_some_and(|read| self.same_dict(built, read, FIELD_PLACED))
                    }
                    _ => true,
                }
            })
        }
    }
}
