//! A page's `/Annots` as a save writes it (M10.5 §11.3): the page's
//! records, in their order.
//!
//! - **sheaf's marks** are written through their appearances
//!   ([`annot::emission`]): the stamp a page stamp holds as a form
//!   ([`FormRef::Base`] the source's object, [`FormRef::Blob`] its one-page
//!   PDF made a form, [`FormRef::Pinned`] drawn from its inputs, once a
//!   file), an attachment's file from its blob, a replacement's StrikeOut
//!   beside its Caret, grouped under it (`/IRT`, `/RT /Group`) and marked
//!   no other way. A mark read from the primary keeps its object's number;
//!   one the state holds as it was read, on its own page, is the file's
//!   object as it is, with its appearance and companion.
//! - **Another tool's comment or link** is its source dictionary, its
//!   references copied with the page (`/P` the page as written), a link to
//!   a page that isn't written dropped by `keep_only`. Copy 0 of one in the
//!   primary keeps its number, and is the file's object as it is when
//!   nothing in it changed. A dictionary written straight into `/Annots`
//!   stays one.
//! - **A widget** is the form's ([`super::form`]): a field merged with its
//!   one widget is its field's object.
//! - **Replies.** `/IRT` names the key of the annotation replied to, found
//!   by its name on the page (or by its object, on another page), so a
//!   reply follows a mark given a fresh id. A mark's reply takes its
//!   parent's `/Rect`. A comment a reply here names that has no `/NM` is
//!   given the name the reply uses.
//! - **A connector's `/Dest`** is the next connector of its group, in
//!   document order and wrapping, at its place: the page's key, numbered
//!   with the rest.
//! - **Annotations the records don't hold** stay with their page where
//!   they were: a pop-up goes with the comment that opens it (`/Popup`,
//!   [`Part::Popup`]) and is dropped with anything else; a screen, a
//!   printer's mark, a trap network, a watermark, 3D, media, a widget
//!   outside the form and an entry with no subtype are kept as the page
//!   has them, copied with it. sheaf's old marks and companions are left
//!   out: their records were written.
//! - **What's left out**: an annotation the primary's page lists that its
//!   copy 0 doesn't write now, and nothing writes at its number (a
//!   flatten's, a delete's), is one of the emission's removed annotations
//!   (`Emission::removed_annots`), which a Rewrite's `keep_only` takes out
//!   of what still names it, as the structure tree does.
//! - **`/StructParent`** stays only where the page keeps its
//!   `/StructParents` (copy 0 of a primary page): the structure tree of a
//!   copy or of another file isn't written (proposal 24).
//!
//! **Without sheaf's private data** (`Private::Strip`, §15), a mark is
//! always written anew, without `/SHEF_Annot`, and the name sheaf gave it,
//! its StrikeOut's and a comment's a reply made here named, is a neutral
//! one ([`private::neutral_nm`]); `/IRT`, `/RT /Group` and a connector's
//! `/Dest` refer to objects, and follow. Another tool's comment loses any
//! key of sheaf's it holds.
//!
//! An overlay writes what its view draws ([`Cx::draws`]): sheaf's marks
//! only when PDFium draws them, others' comments and links (and what else
//! of a page the records don't hold, but widgets) only when the view shows
//! them, so a page drawn without them carries none. An annotation it hides
//! is written under its own key, flagged hidden ([`Cx::hidden`]); a reply
//! to one it doesn't write isn't threaded, and a connector goes nowhere.
//!
//! [`Cx::draws`]: super::Cx::draws
//! [`Cx::hidden`]: super::Cx::hidden

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use lopdf::{Dictionary, Object, ObjectId, Stream, dictionary};

use super::{Cx, Shown};
use crate::annot::{self, Annot, AnnotId, AnnotKind, Emitted};
use crate::content::{Res, ResId};
use crate::file::PageAnnots;
use crate::geom::{Affine, Rect};
use crate::materialize::copy::{self, Scope};
use crate::materialize::key::{ObjKey, Part};
use crate::model::{AnnotBody, DocState, Foreign, FormRef, Mark, PageId, PinnedAp};
use crate::open::FileBody;
use crate::private;
use crate::serialize::built::{Held, StampForm};
use crate::source::ObjRef;
use crate::store::Src;

/// Page `id`'s annotations, emitted: its `/Annots`, None when it has
/// none. `shown` is what the page is written as; `tags`, whether it keeps
/// its `/StructParents`.
pub(crate) fn page(cx: &mut Cx, id: PageId, shown: &Shown, tags: bool) -> Option<Object> {
    let st = cx.st;
    let ids: Vec<AnnotId> = (st.annot_ids_on(id).into_iter().flatten())
        .copied()
        .collect();
    let base = base_annots(shown);
    if ids.is_empty() && base.is_empty() {
        return None;
    }
    let mut p = Page {
        id,
        shown,
        tags,
        base: &base,
        rects: HashMap::new(),
        out: HashMap::new(),
    };
    // Replies wait for what they reply to: a mark's takes its parent's
    // `/Rect`.
    let mut threads = Vec::new();
    for &aid in &ids {
        let Some(rec) = st.annot(aid).filter(|rec| cx.draws(&rec.body)) else {
            continue;
        };
        match &rec.body {
            AnnotBody::Mark(m) if m.annot.thread_parent().is_some() => threads.push((aid, m)),
            AnnotBody::Mark(m) => {
                let (entries, rect) = p.mark(cx, aid, m, None);
                p.done(aid, &m.annot.name(), entries, rect);
            }
            AnnotBody::Foreign(f) => {
                let (entries, rect) = p.foreign(cx, aid, f);
                p.done(aid, &f.summary.name, entries, rect);
            }
            AnnotBody::Widget(w) => {
                let entries = super::form::widget(cx, aid, w, id, tags);
                p.out.insert(aid, entries.into_iter().collect());
            }
        }
    }
    while !threads.is_empty() {
        let parent_of = |m: &Mark| m.annot.thread_parent().unwrap_or_default().to_string();
        let (ready, waiting): (Vec<_>, Vec<_>) = (threads.iter())
            .partition(|(_, m)| p.rects.contains_key(&parent_of(m)) || !named(st, id, m));
        // A reply to a reply that never comes is written unthreaded.
        let stuck = ready.is_empty();
        for &&(aid, m) in if stuck { &waiting } else { &ready } {
            let parent = (st.annot_by_nm(id, &parent_of(m)))
                .zip(p.rects.get(&parent_of(m)).cloned())
                .map(|(pid, rect)| (ObjKey::Annot(pid), rect));
            let (entries, rect) = p.mark(cx, aid, m, parent);
            p.done(aid, &m.annot.name(), entries, rect);
        }
        threads = if stuck {
            Vec::new()
        } else {
            waiting.into_iter().copied().collect()
        };
    }
    let list = p.list(cx, &ids);
    // What the primary's page lists that its copy 0 now leaves out.
    if shown.copy == 0 && shown.src.id() == cx.primary.id() {
        let written: HashSet<ObjectId> = (list.iter())
            .filter_map(|entry| entry.as_reference().ok())
            .filter_map(|r| cx.number_known(cx.syms.key(r)?))
            .collect();
        for entry in &base {
            if let Object::Reference(r) = entry
                && !written.contains(r)
            {
                cx.left_out.push(*r);
            }
        }
    }
    (!list.is_empty()).then_some(Object::Array(list))
}

/// Whether mark `m`'s thread parent is an annotation on page `id`.
fn named(st: &crate::DocState, id: PageId, m: &Mark) -> bool {
    let parent = m.annot.thread_parent().unwrap_or_default();
    st.annot_by_nm(id, parent).is_some()
}

/// The entries of `shown`'s page's own `/Annots`, as its file has them.
pub(crate) fn base_annots(shown: &Shown) -> Vec<Object> {
    let pdf = shown.src.pdf();
    let Some(facts) = shown.src.page(shown.index) else {
        return Vec::new();
    };
    (pdf.get_dictionary(facts.id).ok())
        .and_then(|d| d.get(b"Annots").ok())
        .and_then(|a| pdf.dereference(a).ok())
        .and_then(|(_, a)| a.as_array().ok().cloned())
        .unwrap_or_default()
}

/// One page's annotations as they're emitted.
struct Page<'s> {
    id: PageId,
    shown: &'s Shown,
    tags: bool,
    /// The page's own `/Annots`, as its file has them.
    base: &'s [Object],
    /// Each annotation written so far by name, with its `/Rect`: what a
    /// mark's reply takes.
    rects: HashMap<String, Object>,
    /// What each record puts in `/Annots`: itself, then its companion or
    /// pop-up.
    out: HashMap<AnnotId, Vec<Object>>,
}

impl Page<'_> {
    fn done(&mut self, aid: AnnotId, name: &str, entries: Vec<Object>, rect: Option<Object>) {
        if let Some(rect) = rect.filter(|_| !name.is_empty()) {
            self.rects.entry(name.to_string()).or_insert(rect);
        }
        self.out.insert(aid, entries);
    }

    /// `/Annots`: each record's entries in the records' order, and after
    /// each the annotations the records don't hold that followed it on
    /// the page (those before every record first).
    fn list(&mut self, cx: &mut Cx, ids: &[AnnotId]) -> Vec<Object> {
        let st = cx.st;
        let shown = self.shown;
        // The page's records by the entries they were read as.
        let mut by_object = HashMap::new();
        let mut by_slot = HashMap::new();
        for &aid in ids {
            let (src, slot) = match st.annot(aid).map(|r| &r.body) {
                Some(AnnotBody::Mark(m)) => (m.src.as_ref(), None),
                Some(AnnotBody::Foreign(f)) => (f.src.as_ref(), Some(f.summary.slot)),
                Some(AnnotBody::Widget(w)) => (w.src.as_ref(), None),
                None => continue,
            };
            match src {
                Some(r) if r.src == shown.src => {
                    by_object.insert(r.id, aid);
                }
                Some(_) => {}
                None => {
                    by_slot.extend(slot.map(|s| (s, aid)));
                }
            }
        }
        // What the file's facts read as records, found the first time an
        // entry isn't one: the form's widgets are, unless the primary's form
        // isn't modeled; then they stay as the page has them.
        let widgets_are_records = shown.src.id() != cx.primary.id() || cx.form.modeled;
        let modeled = std::cell::OnceCell::new();
        let modeled = || {
            modeled.get_or_init(|| {
                let annots = shown.src.annots();
                let form = shown.src.form();
                let widgets = widgets_are_records
                    .then(|| form.widgets.get(&shown.index))
                    .flatten();
                (annots.on(shown.index).iter().map(|a| a.slot))
                    .chain(widgets.into_iter().flatten().map(|w| w.slot))
                    .collect::<HashSet<usize>>()
            })
        };
        let entries = PageAnnots::new(self.base);
        let mut after: HashMap<Option<AnnotId>, Vec<Object>> = HashMap::new();
        let mut last = None;
        for (slot, entry) in self.base.iter().enumerate() {
            let rec = match entry {
                Object::Reference(r) => by_object.get(r),
                _ => by_slot.get(&slot),
            };
            if let Some(&aid) = rec {
                if self.out.contains_key(&aid) {
                    last = Some(aid);
                }
                continue;
            }
            if modeled().contains(&slot) || crate::file::is_ours(shown.src.pdf(), entry, &entries) {
                continue;
            }
            if let Some(kept) = self.unmodeled(cx, entry) {
                after.entry(last).or_default().push(kept);
            }
        }
        let mut list = after.remove(&None).unwrap_or_default();
        for aid in ids {
            list.extend(self.out.remove(aid).into_iter().flatten());
            list.extend(after.remove(&Some(*aid)).into_iter().flatten());
        }
        list
    }

    /// An annotation of the page the records don't hold, as written: None
    /// for a pop-up, which goes with what opens it, for what the view
    /// doesn't draw, and for what a markup removal takes off
    /// ([`Unheld`](crate::materialize::Unheld)).
    fn unmodeled(&self, cx: &mut Cx, entry: &Object) -> Option<Object> {
        let shown = self.shown;
        let pdf = shown.src.pdf();
        let (_, dict) = pdf.dereference(entry).ok()?;
        let dict = dict.as_dict().ok()?;
        let widget = is(dict, b"Subtype", b"Widget");
        let link = is(dict, b"Subtype", b"Link");
        if is(dict, b"Subtype", b"Popup")
            || !cx.draws_unmodeled(widget)
            || !cx.keeps_unheld(link, widget)
        {
            return None;
        }
        let scope = Scope {
            src: &shown.src,
            copy: shown.copy,
        };
        if self.tags {
            return Some(cx.sym(entry, &scope));
        }
        let built = |cx: &mut Cx| -> Dictionary {
            let mut out = Dictionary::new();
            for (k, v) in dict.iter() {
                match k.as_slice() {
                    b"StructParent" => {}
                    b"P" => out.set("P", cx.placeholder(ObjKey::Page(self.id))),
                    _ => {
                        let v = cx.sym(v, &scope);
                        out.set(k.clone(), v);
                    }
                }
            }
            out
        };
        match entry {
            Object::Reference(r) => {
                let key = ObjKey::Base {
                    blob: shown.src.id(),
                    obj: *r,
                    copy: shown.copy,
                };
                // An Append's saved file may have it already, its `/P` the
                // number of the page it was written with: written again when
                // its source's pages are named otherwise now (the page
                // renumbered, or another page that copy of its page).
                if cx.closure.claim(key) && cx.writes(&key) {
                    let out = built(cx);
                    cx.push(key, Object::Dictionary(out));
                }
                Some(cx.placeholder(key))
            }
            _ => Some(Object::Dictionary(built(cx))),
        }
    }

    /// Another tool's comment or link `aid`, `f`: its entries in `/Annots`,
    /// and its `/Rect`.
    fn foreign(&mut self, cx: &mut Cx, aid: AnnotId, f: &Foreign) -> (Vec<Object>, Option<Object>) {
        let st = cx.st;
        let shown = self.shown;
        let (src, dict) = match &f.src {
            Some(r) => match r.src.pdf().get_dictionary(r.id) {
                Ok(d) => (&r.src, d),
                Err(_) => return (Vec::new(), None),
            },
            None => match self.base.get(f.summary.slot) {
                Some(Object::Dictionary(d)) => (&shown.src, d),
                _ => return (Vec::new(), None),
            },
        };
        let scope = Scope { src, copy: f.copy };
        // A link to a page that isn't written leads nowhere: a Rewrite's
        // `keep_only` drops it, and an Append or an overlay leaves it out as
        // that would.
        if (cx.saved().is_some() || cx.overlay.is_some())
            && f.summary.subtype == "Link"
            && leads_nowhere(src.pdf(), dict, |page| cx.closure.writes_page(page, &scope))
        {
            return (Vec::new(), None);
        }
        let parent = (f.summary.parent.as_deref())
            .and_then(|name| st.annot_by_nm(self.id, name))
            .filter(|&pid| cx.draws_id(pid))
            .map(ObjKey::Annot)
            .or_else(|| {
                let irt = dict.get(b"IRT").ok()?.as_reference().ok()?;
                let pid = cx.annot_at(src, irt, f.copy)?;
                cx.draws_id(pid).then_some(ObjKey::Annot(pid))
            });
        let rect = (dict.get(b"Rect").ok())
            .and_then(|r| src.pdf().dereference(r).ok())
            .map(|(_, r)| r.clone());
        let key = ObjKey::Annot(aid);
        let name = &f.summary.name;
        // A reply made here names it by what it says: it takes that name.
        let named = || {
            (st.annots_on(self.id))
                .any(|(_, a)| a.mark().and_then(Annot::thread_parent) == Some(name.as_str()))
        };
        // Nothing of it changes: the file's object as it is, unbuilt.
        if let Some(r) = &f.src
            && self.tags
            && cx.owned_at(key) == Some(r.id)
            && !dict.has(b"Popup")
            && self.refs_kept(cx, dict, parent)
            && (dict.has(b"NM") || !named())
            && !cx.hides(aid)
            && !(cx.strips() && private::in_annot(dict))
        {
            cx.push_raw(key, r.clone());
            return (vec![cx.placeholder(key)], rect);
        }
        let mut popup = None;
        let mut out = Dictionary::new();
        for (k, v) in dict.iter() {
            match k.as_slice() {
                b"P" => out.set("P", cx.placeholder(ObjKey::Page(self.id))),
                b"StructParent" if !self.tags => {}
                b"IRT" | b"RT" if parent.is_none() => {}
                b"IRT" => out.set("IRT", cx.placeholder(parent.expect("a parent"))),
                b"Popup" => {
                    popup = self.popup(cx, aid, v, &scope, f.src.is_some() && f.copy == 0);
                    if let Some(key) = popup {
                        out.set("Popup", cx.placeholder(key));
                    }
                }
                _ => {
                    let v = cx.sym(v, &scope);
                    out.set(k.clone(), v);
                }
            }
        }
        if !dict.has(b"NM") && named() {
            out.set("NM", Object::string_literal(name.as_str()));
        }
        if cx.strips() {
            private::strip_annot(&mut out);
        }
        cx.hidden(aid, &mut out);
        let Some(r) = &f.src else {
            return (vec![Object::Dictionary(out)], rect);
        };
        if cx.owned_at(key) == Some(r.id) && cx.unchanged(&out, dict) {
            cx.push_raw(key, r.clone());
        } else {
            cx.push(key, Object::Dictionary(out));
        }
        let mut entries = vec![cx.placeholder(key)];
        // Its pop-up after it, where the page listed it.
        if let (Some(key), Ok(Object::Reference(at))) = (popup, dict.get(b"Popup"))
            && self.base.contains(&Object::Reference(*at))
        {
            entries.push(cx.placeholder(key));
        }
        (entries, rect)
    }

    /// Whether `dict`, an annotation of the page as the file has it, names
    /// the numbers the save writes what it names at: its `/P` the page's
    /// and its `/IRT` its `parent`'s (when it has them), and has no `/RT`
    /// without a parent, which the save drops.
    fn refs_kept(&self, cx: &Cx, dict: &Dictionary, parent: Option<ObjKey>) -> bool {
        if parent.is_none() && dict.has(b"RT") {
            return false;
        }
        let page = cx.number_known(ObjKey::Page(self.id));
        let p = match dict.get(b"P") {
            Err(_) => true,
            Ok(&Object::Reference(p)) => page == Some(p),
            Ok(_) => false,
        };
        let irt = match (dict.get(b"IRT"), parent) {
            (Err(_), _) => true,
            (Ok(&Object::Reference(r)), Some(parent)) => cx.number_known(parent) == Some(r),
            _ => false,
        };
        p && irt
    }

    /// The pop-up `/Popup` value `v` names, of annotation `aid` read
    /// through `scope`, emitted under its key: None when it names no
    /// pop-up. `own`: the annotation keeps its number, and so does the
    /// pop-up.
    fn popup(
        &self,
        cx: &mut Cx,
        aid: AnnotId,
        v: &Object,
        scope: &Scope,
        own: bool,
    ) -> Option<ObjKey> {
        let at = v.as_reference().ok()?;
        let pdf = scope.src.pdf();
        let dict = pdf.get_dictionary(at).ok()?;
        if !is(dict, b"Subtype", b"Popup") {
            return None;
        }
        let key = ObjKey::AnnotPart(aid, Part::Popup);
        let primary = scope.src.id() == cx.primary.id();
        let owned = own && primary && cx.own(key, at);
        let mut out = Dictionary::new();
        for (k, v) in dict.iter() {
            match k.as_slice() {
                b"Parent" => out.set("Parent", cx.placeholder(ObjKey::Annot(aid))),
                b"P" => out.set("P", cx.placeholder(ObjKey::Page(self.id))),
                b"StructParent" if !self.tags => {}
                _ => {
                    let v = cx.sym(v, scope);
                    out.set(k.clone(), v);
                }
            }
        }
        if owned && cx.unchanged(&out, dict) {
            let raw = ObjRef {
                src: scope.src.clone(),
                id: at,
            };
            cx.push_raw(key, raw);
        } else {
            cx.push(key, Object::Dictionary(out));
        }
        Some(key)
    }

    /// Mark `aid`, `m`, a reply to `parent` (its key and `/Rect`) when it
    /// is one: its entries in `/Annots`, and its `/Rect`.
    fn mark(
        &mut self,
        cx: &mut Cx,
        aid: AnnotId,
        m: &Mark,
        parent: Option<(ObjKey, Object)>,
    ) -> (Vec<Object>, Option<Object>) {
        let key = ObjKey::Annot(aid);
        if !cx.hides(aid)
            && !cx.strips()
            && let Some(kept) = self.as_read(cx, aid, m, parent.as_ref().map(|(k, _)| *k))
        {
            return kept;
        }
        let held = Held {
            stamp: cx.stamp_form(m),
            file: cx.file_of(m),
        };
        let Emitted {
            mut main,
            appearance,
            mut companions,
        } = annot::emission(&m.annot, held);
        if cx.strips() {
            private::strip_annot(&mut main);
            for (dict, _) in &mut companions {
                private::strip_annot(dict);
            }
        }
        let page = cx.placeholder(ObjKey::Page(self.id));
        main.set("P", page.clone());
        // An appearance an Append doesn't write isn't drawn.
        let ap = ObjKey::AnnotPart(aid, Part::Ap);
        if cx.writes(&ap) {
            let form = appearance.form(cx);
            cx.push(ap, Object::Stream(form));
        }
        main.set("AP", dictionary! { "N" => cx.placeholder(ap) });
        if let Some((parent, rect)) = parent {
            main.set("IRT", cx.placeholder(parent));
            main.set("Rect", rect);
        }
        // An overlay's goes nowhere: where it leads isn't drawn, and its
        // group is found by a walk of the document.
        if let AnnotKind::Connector { label, .. } = &m.annot.kind
            && cx.overlay.is_none()
            && let Some(dest) = cx.connector_dest(aid, label)
        {
            main.set("Dest", dest);
        }
        cx.hidden(aid, &mut main);
        let rect = main.get(b"Rect").ok().cloned();
        cx.push(key, Object::Dictionary(main));
        let mut entries = vec![cx.placeholder(key)];
        // A replacement has one companion, its StrikeOut.
        if let Some((mut dict, appearance)) = companions.into_iter().next() {
            let ap = ObjKey::AnnotPart(aid, Part::CompanionAp);
            if cx.writes(&ap) {
                let form = appearance.form(cx);
                cx.push(ap, Object::Stream(form));
            }
            dict.set("AP", dictionary! { "N" => cx.placeholder(ap) });
            dict.set("IRT", cx.placeholder(key));
            dict.set("RT", Object::Name(b"Group".to_vec()));
            dict.set("P", page);
            cx.hidden(aid, &mut dict);
            let part = ObjKey::AnnotPart(aid, Part::Companion);
            cx.push(part, Object::Dictionary(dict));
            entries.push(cx.placeholder(part));
        }
        (entries, rect)
    }

    /// Mark `aid`, `m`, as the file has it, when the state holds it as it
    /// was read and it keeps its number ([`mark_kept`]): its object as it
    /// is, then its companions as they are; a reply only while it still
    /// names its parent's number (`parent` the parent's key).
    fn as_read(
        &self,
        cx: &mut Cx,
        aid: AnnotId,
        m: &Mark,
        parent: Option<ObjKey>,
    ) -> Option<(Vec<Object>, Option<Object>)> {
        let r = m.src.as_ref()?;
        if cx.owned_at(ObjKey::Annot(aid)) != Some(r.id) {
            return None;
        }
        let page = cx.number_known(ObjKey::Page(self.id));
        let parent = parent.map(|parent| cx.number_known(parent));
        let Kept { companions, rect } = mark_kept(self.shown, self.base, m, page, parent)?;
        cx.push_raw(ObjKey::Annot(aid), r.clone());
        let scope = Scope {
            src: &r.src,
            copy: 0,
        };
        let mut entries = vec![cx.placeholder(ObjKey::Annot(aid))];
        for entry in companions {
            entries.push(cx.sym(entry, &scope));
        }
        Some((entries, rect))
    }
}

/// A mark kept as its file has it ([`mark_kept`]).
pub(crate) struct Kept<'b> {
    /// Its companions' entries in its page's `/Annots`, in order.
    pub(crate) companions: Vec<&'b Object>,
    /// Its `/Rect`.
    pub(crate) rect: Option<Object>,
}

/// Mark `m` as the file `shown`'s page is of has it, on a page whose
/// `/Annots` the file has as `base`: when the state holds it as it was
/// read there (not a copy another program made, given a new id at open,
/// whose object says the id it was copied with, and holding the stamp's
/// page it was read with), with its companions: what's grouped under it
/// and isn't a mark itself. Not when its `/P` isn't `page` (the number its
/// page is written at), its `/IRT` doesn't name `parent` (the number its
/// thread's parent is written at, None while that isn't written; a mark
/// that isn't a reply names none), it has a pop-up, or a companion is
/// marked `/SheafPart`, none of which the mark is written with. What a save
/// and an overlay keep, and the realizer's model of them
/// ([`untouched`](crate::realize::untouched)).
pub(crate) fn mark_kept<'b>(
    shown: &Shown,
    base: &'b [Object],
    m: &Mark,
    page: Option<ObjectId>,
    parent: Option<Option<ObjectId>>,
) -> Option<Kept<'b>> {
    let r = m.src.as_ref()?;
    if r.src != shown.src {
        return None;
    }
    let facts = shown.src.annots();
    let read = (facts.on(shown.index).iter())
        .filter(|a| !facts.fresh.contains(&(shown.index, a.slot)))
        .find_map(|a| match (&a.body, a.object) {
            (FileBody::Mark { annot, .. }, Some(at)) if at == r.id => Some(annot),
            _ => None,
        })?;
    let stamp_kept =
        (m.stamp.as_ref()).is_none_or(|f| matches!(f, FormRef::Base(at) if at.src == r.src));
    if **read != m.annot || !stamp_kept {
        return None;
    }
    let pdf = r.src.pdf();
    let dict = pdf.get_dictionary(r.id).ok()?;
    let irt = dict.get(b"IRT").ok().and_then(|o| o.as_reference().ok());
    let threads = match (m.annot.thread_parent(), parent) {
        (None, _) => irt.is_none(),
        (Some(_), Some(parent)) => irt.is_some() && irt == parent,
        (Some(_), None) => false,
    };
    let p_kept = match dict.get(b"P") {
        Err(_) => true,
        Ok(&Object::Reference(p)) => page == Some(p),
        Ok(_) => false,
    };
    if !threads || !p_kept || dict.has(b"Popup") {
        return None;
    }
    let entries = PageAnnots::new(base);
    let mut companions = Vec::new();
    for entry in base {
        let read = (entry.as_reference().ok()).and_then(|at| pdf.get_dictionary(at).ok());
        let under = read.is_some_and(|d| {
            d.get(b"IRT").ok() == Some(&Object::Reference(r.id)) && !d.has(b"SHEF_Annot")
        });
        if under && crate::file::is_ours(pdf, entry, &entries) {
            if read.is_some_and(|d| d.has(crate::annot::PART.as_bytes())) {
                return None;
            }
            companions.push(entry);
        }
    }
    let rect = (dict.get(b"Rect").ok())
        .and_then(|o| pdf.dereference(o).ok())
        .map(|(_, o)| o.clone());
    Some(Kept { companions, rect })
}

/// Whether link `dict`, of `pdf`, goes to a page that isn't written, or
/// nowhere: its destination ([`link_dest`]) is a page `written` says the
/// save doesn't write (as the copy reading it resolves it,
/// [`Closure::writes_page`]), or null.
///
/// [`Closure::writes_page`]: crate::materialize::copy::Closure::writes_page
pub(crate) fn leads_nowhere(
    pdf: &lopdf::LazyPdf,
    dict: &Dictionary,
    written: impl Fn(ObjectId) -> bool,
) -> bool {
    match link_dest(pdf, dict) {
        Some(None) => true,
        Some(Some(page)) => !written(page),
        None => false,
    }
}

/// Where link `dict`, of `pdf`, goes by its explicit destination (its
/// `/Dest`, or its GoTo action's `/D`): `Some(Some(page))` when its first
/// entry is a page of `pdf`, `Some(None)` when it's null or there's none,
/// and None for anything else. A named destination is the name tree's to
/// resolve.
fn link_dest(pdf: &lopdf::LazyPdf, dict: &Dictionary) -> Option<Option<ObjectId>> {
    let deref = |o: &Object| pdf.dereference(o).ok().map(|(_, o)| o.clone());
    let dest = match dict.get(b"Dest") {
        Ok(dest) => deref(dest),
        Err(_) => (dict.get(b"A").ok().and_then(deref))
            .filter(|a| a.as_dict().is_ok_and(|a| is(a, b"S", b"GoTo")))
            .and_then(|a| a.as_dict().ok()?.get(b"D").ok().and_then(deref)),
    };
    let dest = match dest {
        Some(Object::Dictionary(d)) => d.get(b"D").ok().and_then(deref),
        dest => dest,
    };
    let Some(Object::Array(dest)) = dest else {
        return None;
    };
    match dest.first() {
        None | Some(Object::Null) => Some(None),
        Some(Object::Reference(page)) if crate::materialize::copy::is_page_node(pdf, *page) => {
            Some(Some(*page))
        }
        Some(_) => None,
    }
}

/// Where another tool's link `f`, on a page showing a page of `own`, leads
/// in `st`, as an overlay over `own` finds its pages ([`copy::page_of`]).
/// Read from its object, or for one written into its page's `/Annots`,
/// from the entry at its place there, of those `base` gives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LinkTo {
    /// A page of `st`.
    Page(PageId),
    /// A page `st` hasn't got, or nowhere ([`leads_nowhere`]): an overlay
    /// leaves it out, as a save does.
    Nowhere,
    /// Not to a page by an explicit destination, or not a link.
    Elsewhere,
}

impl LinkTo {
    pub(crate) fn of(
        st: &DocState,
        own: &Src,
        base: impl FnOnce() -> Vec<Object>,
        f: &Foreign,
    ) -> LinkTo {
        if f.summary.subtype != "Link" {
            return LinkTo::Elsewhere;
        }
        let src = f.src.as_ref().map_or(own, |r| &r.src);
        let whole = src.id() == own.id() && f.copy == 0;
        let leads = |dict: &Dictionary| match link_dest(src.pdf(), dict) {
            Some(Some(page)) => match copy::page_of(st, src.id(), page, !whole) {
                Some(shown) => LinkTo::Page(shown),
                None => LinkTo::Nowhere,
            },
            Some(None) => LinkTo::Nowhere,
            None => LinkTo::Elsewhere,
        };
        let dict = match &f.src {
            Some(r) => r.src.pdf().get_dictionary(r.id).ok().map(leads),
            None => match base().get(f.summary.slot) {
                Some(Object::Dictionary(d)) => Some(leads(d)),
                _ => None,
            },
        };
        dict.unwrap_or(LinkTo::Elsewhere)
    }
}

/// Whether another tool's link `f` leads nowhere in `st` ([`LinkTo::of`]).
pub(crate) fn dead_link(
    st: &DocState,
    own: &Src,
    base: impl FnOnce() -> Vec<Object>,
    f: &Foreign,
) -> bool {
    LinkTo::of(st, own, base, f) == LinkTo::Nowhere
}

/// Whether `dict`'s `key` is the name `name`.
pub(crate) fn is(dict: &Dictionary, key: &[u8], name: &[u8]) -> bool {
    dict.get(key)
        .and_then(Object::as_name)
        .is_ok_and(|n| n == name)
}

impl Cx<'_> {
    /// Page stamp `m`'s page as a form in the file ([`form_of`]). None,
    /// said once, when it can't be had.
    ///
    /// [`form_of`]: Self::form_of
    pub(crate) fn stamp_form(&mut self, m: &Mark) -> Option<StampForm> {
        let AnnotKind::PageStamp { stamp, name, .. } = &m.annot.kind else {
            return None;
        };
        let Some(form) = &m.stamp else {
            log::warn!("stamp {name} ({stamp}) is drawn without its page: it holds none");
            return None;
        };
        self.form_of(form, |e| {
            log::warn!("stamp {name} ({stamp}) is drawn without its page: {e:#}");
        })
    }

    /// `form` as a form in the file, once a file: the source's form a
    /// [`FormRef::Base`] names, copied when it isn't the primary's, or a
    /// [`FormRef::Blob`]'s page made a form, or a [`FormRef::Pinned`]
    /// appearance drawn from its inputs ([`Cx::pinned`]). None when it
    /// can't be had, which `said` is told the first time.
    pub(crate) fn form_of(
        &mut self,
        form: &FormRef,
        said: impl FnOnce(&anyhow::Error),
    ) -> Option<StampForm> {
        if let Some(&made) = self.stamps.get(form) {
            return made;
        }
        let made = match form {
            FormRef::Base(at) => {
                let scope = Scope {
                    src: &at.src,
                    copy: 0,
                };
                let id = self.sym(&Object::Reference(at.id), &scope);
                crate::stamp_lib::form_frame(at.src.pdf(), at.id)
                    .map(|(bbox, matrix)| (id, bbox, matrix))
                    .ok_or_else(|| anyhow::anyhow!("a form with no box"))
            }
            FormRef::Blob(page) => blob_form(page.blob(), 0).map(|(objects, bbox, matrix)| {
                let id = self.resource_of(objects);
                (Object::Reference(id), bbox, matrix)
            }),
            FormRef::Pinned(ap) => Ok(self.pinned(ap)),
        };
        let made = made
            .and_then(|(id, bbox, matrix)| {
                let id = id
                    .as_reference()
                    .map_err(|_| anyhow::anyhow!("not an object"))?;
                Ok(StampForm::written(id, bbox, matrix))
            })
            .inspect_err(said)
            .ok();
        self.stamps.insert(form.clone(), made);
        made
    }

    /// Pinned appearance `ap` as a form in the file, drawn from its inputs
    /// as the mark it was drawn from is ([`annot::appearance`]), with the
    /// stamp's page it holds: its dictionary now, its content as it's
    /// written ([`ObjKey::Pinned`], once a file), the same each time. Its
    /// placeholder, `/BBox` and `/Matrix`.
    fn pinned(&mut self, ap: &Arc<PinnedAp>) -> (Object, Rect, Affine) {
        let inputs = &ap.inputs;
        // The stamp's page named by its form, not its number, which an
        // Append and a Rewrite give it apart: the same appearance either way.
        let page = match (&inputs.kind, &inputs.stamp) {
            (AnnotKind::PageStamp { stamp, name, .. }, Some(page)) => self
                .form_of(page, |e| {
                    log::warn!("stamp {name} ({stamp}) is drawn without its page: {e:#}");
                })
                .map(|form| StampForm {
                    res: ResId(page.digest()),
                    ..form
                }),
            _ => None,
        };
        let drawn = annot::appearance(&inputs.annot(), page);
        let bbox = drawn.bbox();
        let id = self.push_deferred(ObjKey::Pinned(ap.digest), |cx| {
            let mut dict = drawn.form_dict(cx);
            dict.set("Filter", "FlateDecode");
            let ap = ap.clone();
            let data = move || {
                let drawn = annot::appearance(&ap.inputs.annot(), page);
                super::deflated(&drawn.into_content())
            };
            super::Deferred {
                dict,
                data: Arc::new(data),
            }
        });
        (Object::Reference(id), bbox, Affine::IDENTITY)
    }

    /// Attachment `m`'s file as an embedded file stream in the file, from
    /// its blob, once a file. None, said once, when it holds none.
    pub(crate) fn file_of(&mut self, m: &Mark) -> Option<ObjectId> {
        let AnnotKind::FileAttachment {
            file: stem, name, ..
        } = &m.annot.kind
        else {
            return None;
        };
        let Some(file) = &m.file else {
            log::warn!("attachment {name} ({stem}) is written without its file: it holds none");
            return None;
        };
        if let Some(&made) = self.files.get(&file.id()) {
            return Some(made);
        }
        if let Some(at) = self.held_file(stem, file.id()) {
            self.files.insert(file.id(), at);
            return Some(at);
        }
        let stream = Stream::new(
            dictionary! {
                "Type" => "EmbeddedFile",
                "Params" => dictionary! { "Size" => file.bytes().len() as i64 },
            },
            file.bytes().to_vec(),
        );
        let made = self.resource_of(vec![Object::Stream(stream)]);
        self.files.insert(file.id(), made);
        Some(made)
    }

    /// In an Append, the embedded file stream the saved file holds for an
    /// attachment naming `stem`, a primary's object still in the file whose
    /// data is blob `file`'s: its placeholder, so the section refers to it
    /// rather than embed the file again.
    fn held_file(&mut self, stem: &str, file: crate::store::BlobId) -> Option<ObjectId> {
        let record = self.saved()?;
        let at = self.primary.annots().file(stem)?;
        if !record.present.contains(at.0) {
            return None;
        }
        let stream = self.primary.pdf().get_object(at).ok()?;
        let data = crate::attachments::file_bytes(stream.as_stream().ok()?).ok()?;
        if crate::store::blob_id(&data) != file {
            return None;
        }
        let scope = Scope {
            src: &self.primary.clone(),
            copy: 0,
        };
        self.sym(&Object::Reference(at), &scope).as_reference().ok()
    }

    /// `objects`, a resource's (the resource first; a reference `(i, 0)`
    /// among them to the `i`th), written once a file: its placeholder.
    fn resource_of(&mut self, objects: Vec<Object>) -> ObjectId {
        use crate::content::sink::Sink as _;
        self.resource(&Res::of(objects))
    }

    /// The annotation whose object `obj` of `src` is, as `copy` of it,
    /// else as copy 0: what a reply on another page names.
    pub(crate) fn annot_at(
        &self,
        src: &crate::store::Src,
        obj: ObjectId,
        copy: u32,
    ) -> Option<AnnotId> {
        let at = |copy| self.by_src.get(&(src.id(), obj, copy)).copied();
        at(copy).or_else(|| at(0))
    }

    /// Connector `aid`'s `/Dest`, labelled `label`: the next connector of
    /// its group in document order, wrapping, a little in from its
    /// top-left corner. None alone in its group.
    fn connector_dest(&mut self, aid: AnnotId, label: &str) -> Option<Object> {
        let st = self.st;
        let members = self.groups.entry(label.to_string()).or_insert_with(|| {
            let group = crate::annot::Group::Connector(label.to_string());
            crate::model::group_members_in(st, &group)
                .into_iter()
                .map(|(_, page, a)| (page, a))
                .collect()
        });
        if members.len() < 2 {
            return None;
        }
        let i = members.iter().position(|(_, a)| a.id == aid)?;
        let (page, next) = members[(i + 1) % members.len()].clone();
        let AnnotKind::Connector { rect, .. } = &next.kind else {
            return None;
        };
        Some(Object::Array(vec![
            self.placeholder(ObjKey::Page(page)),
            Object::Name(b"XYZ".to_vec()),
            Object::from(rect.x - 36.0),
            Object::from(rect.bottom() + 36.0),
            Object::Null,
        ]))
    }
}

/// Page `index` (from 0) of the PDF `blob`, read as a source, made a
/// form, as a resource's objects (the form first, a reference `(i, 0)` to
/// the `i`th), with its `/BBox` and `/Matrix`: a page stamp's one page, or
/// a watermark's page.
pub(crate) fn blob_form(
    blob: &crate::store::Blob,
    index: u32,
) -> anyhow::Result<(Vec<Object>, crate::geom::Rect, crate::geom::Affine)> {
    let src = blob.source(&crate::source::Limits::DEFAULT)?;
    let mut dst = lopdf::Document::new();
    let form = (crate::stamp_lib::page_forms(&src, &[index], &mut dst)?).remove(0);
    let order: Vec<ObjectId> = std::iter::once(form.id)
        .chain(dst.objects.keys().copied().filter(|&id| id != form.id))
        .collect();
    let index: HashMap<ObjectId, u32> = (0..).zip(&order).map(|(i, &id)| (id, i)).collect();
    fn follow(obj: &mut Object, index: &HashMap<ObjectId, u32>) {
        match obj {
            Object::Reference(r) => {
                *obj = match index.get(r) {
                    Some(&i) => Object::Reference((i, 0)),
                    None => Object::Null,
                }
            }
            Object::Array(items) => items.iter_mut().for_each(|o| follow(o, index)),
            Object::Dictionary(d) => d.iter_mut().for_each(|(_, o)| follow(o, index)),
            Object::Stream(s) => s.dict.iter_mut().for_each(|(_, o)| follow(o, index)),
            _ => {}
        }
    }
    let objects = (order.iter())
        .map(|id| {
            let mut obj = dst.objects[id].clone();
            follow(&mut obj, &index);
            obj
        })
        .collect();
    Ok((objects, form.bbox, form.matrix))
}
