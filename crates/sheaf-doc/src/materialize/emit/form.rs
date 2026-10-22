//! The form as a save writes it (M10.5 §11.3), every instance from the
//! records (the old writers' forms/merge.rs, generalized).
//!
//! - **Field dictionaries** are their source's, copied as their instance
//!   ([`FormInst::copy`]): an instance past the first of its file's bytes
//!   (a duplicate's, the same file inserted again) gets objects of its
//!   own. `/Parent` and `/Kids` are the tree's; a parent sheaf made for an
//!   instance whose names clashed is written from its record. Copy 0 of
//!   the primary's own keeps its numbers, and a field the save changed
//!   nothing of is the file's object as it is.
//! - **A field merged with its one widget** (ISO 32000-1 12.7.3.1) is one
//!   object under [`ObjKey::Field`]: the page's `/Annots` names it, and the
//!   widget's key is an alias of it ([`Plan::aliases`]), so no number is
//!   written twice. A copy stays merged, with no `/Parent` but the tree's,
//!   so it's never an orphan. One whose widget the state shows more than
//!   once (a page the tree lists twice) is written apart: the field's keys
//!   in the field, each widget with the widget's.
//! - **Widgets** not merged are their source's, `/Parent` their field and
//!   `/P` their page, `/StructParent` kept only where the page keeps its
//!   tags.
//! - **Widgets on page templates** (`/Names /Templates`) aren't records: a
//!   primary field's kids the records don't account for and no page shows
//!   stay in its `/Kids`, as the file has them.
//! - **Fields left with no widgets** go, as `keep_only` takes them out
//!   (bug 14): a node that showed as its file was read and shows nowhere
//!   now, with the parents it leaves with none; and the form, when no
//!   field is left of one that had some.
//! - **The form's dictionary** writes its owned keys from [`FormDoc`]:
//!   `/Fields` its roots, `/CO`, `/DA`, `/Q`, `/NeedAppearances` and
//!   `/SigFlags`. A field tree the state doesn't model (a damaged one, or
//!   one a document made without reading its form never took in) is kept
//!   as the file has it, its form's every key with it, and its widgets
//!   stay on their pages as they are. `/DR` is the primary's with each other file's fonts and
//!   resources merged in, a font of the same name that isn't the same font
//!   under a new name, which that file's fields' and widgets' `/DA` then
//!   name; that file's form's `/DA` and `/Q`, where they differ, are
//!   pushed down onto its roots. Every other key is the primary's.
//! - **Document scripts** of the other files come along in the catalog's
//!   `/Names /JavaScript`, renamed apart (but Adobe's version checks, which
//!   are the same everywhere).
//! - **Save policy** ([`SaveTransform`]) applies here: `DropXfa` leaves
//!   `/XFA` out, its tooltips in the fields' `/TU`; `FixSigFlags` drops
//!   `/SigFlags` when nothing signed needs it (`DropUr3` is the catalog's).
//! - **An overlay's form** is of the fields with a widget on its pages
//!   ([`Plan::scoped`]): `/Fields` its roots among them, each node's
//!   `/Kids` those of its nodes and its widgets that are, so PDFium loads
//!   only them (slice 2's scoped groups). A field whose widgets are on
//!   pages of other overlays too is in each, and `/CO` and scripts see
//!   only the fields of their overlay.
//!
//! [`FormDoc`]: crate::model::FormDoc

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::Arc;

use lopdf::{Dictionary, LazyPdf, Object, ObjectId, StringFormat};

use super::Cx;
use crate::annot::AnnotId;
use crate::forms::{FieldId, FieldKind, Value};
use crate::materialize::copy::Scope;
use crate::materialize::key::{NameTree, ObjKey};
use crate::materialize::plan::SaveTransform;
use crate::materialize::record::ACROFORM_OWNED;
use crate::model::{AnnotRec, DocState, FieldNode, FormInst, PageId, Widget};
use crate::source::{Limits, ObjRef};
use crate::store::{BlobId, Src};

/// The keys of a merged field and widget that are the widget's: what a
/// field written apart from its widgets leaves out (ISO 32000-1 Tables
/// 164 and 188).
const WIDGET_KEYS: &[&[u8]] = &[
    b"Type",
    b"Subtype",
    b"Rect",
    b"P",
    b"NM",
    b"M",
    b"F",
    b"AP",
    b"AS",
    b"Border",
    b"C",
    b"StructParent",
    b"OC",
    b"H",
    b"MK",
    b"A",
    b"BS",
];

/// The keys of a merged field and widget that are the field's: what a
/// widget written apart from its field leaves out (ISO 32000-1 Tables 220,
/// 222, 229, 231 and 232).
const FIELD_KEYS: &[&[u8]] = &[
    b"FT", b"T", b"TU", b"TM", b"Ff", b"V", b"DV", b"Opt", b"TI", b"I", b"MaxLen", b"Lock", b"SV",
    b"RV", b"DS", b"Kids",
];

/// The most document scripts read from one file, as `forms::read` counts
/// them.
const MAX_SCRIPTS: usize = Limits::DEFAULT.forms.scripts;

/// What the form's emission decides before the pages are written.
#[derive(Default)]
pub(crate) struct Plan {
    /// Widgets that are their field's one object, by id: their field.
    merged: HashMap<AnnotId, FieldId>,
    /// Fields read merged with a widget the state shows more than once:
    /// written apart from their widgets.
    split: HashSet<FieldId>,
    /// `/DR` merged, when files other than the primary's bring forms.
    dr: Option<Dictionary>,
    /// Each other file's fonts renamed in `/DR`, by old name.
    renamed: HashMap<BlobId, HashMap<Vec<u8>, Vec<u8>>>,
    /// The other files the forms come from, by their blobs.
    others: Vec<Src>,
    /// Whether the state models the primary's form: its records were read
    /// (a document made by `Document::new` reads none) and its tree isn't
    /// damaged. One it doesn't is kept as the file has it.
    pub(crate) modeled: bool,
    /// The keys written: the form's dictionary, its fields and widgets
    /// ([`Emission::form_keys`](super::Emission::form_keys)).
    pub(crate) keys: Vec<ObjKey>,
    /// Whether any was written anew, or the primary's form isn't modeled.
    pub(crate) touched: bool,
    /// An overlay's fields: the nodes with a widget on its pages, and the
    /// nodes above them. None for a save, which writes every one.
    pub(crate) scoped: Option<BTreeSet<FieldId>>,
}

impl Plan {
    /// Each widget key that names its field's object: what the save
    /// record maps to the same number.
    pub(crate) fn aliases(&self) -> impl Iterator<Item = (ObjKey, ObjKey)> + '_ {
        (self.merged.iter()).map(|(&w, &f)| (ObjKey::Annot(w), ObjKey::Field(f)))
    }

    /// The field widget `id` is one object with, if it is.
    pub(crate) fn merged_into(&self, id: AnnotId) -> Option<FieldId> {
        self.merged.get(&id).copied()
    }
}

/// The form's plan, and its fields' claims to the primary's numbers: copy 0
/// of the primary's own nodes keep theirs. An overlay's is of the fields
/// with a widget on its pages alone.
pub(crate) fn prepare(cx: &mut Cx) -> Plan {
    let st = cx.st;
    let scoped = (cx.overlay).map(|_| scoped(st, cx.shown.keys().copied()));
    let terminals = (nodes(st, scoped.as_ref()))
        .filter_map(|(fid, node)| Some((fid, &**node, &**node.term.as_ref()?)));
    let mut merged = HashMap::new();
    let mut split = HashSet::new();
    for (fid, node, term) in terminals {
        let Some(src) = &node.src else {
            continue;
        };
        let shows: Vec<(AnnotId, &Arc<Widget>)> = (term.widgets.iter())
            .filter_map(|&w| Some((w, st.annot(w)?.widget()?)))
            .collect();
        let one = |w: &Widget| w.merged && w.src.as_ref().is_some_and(|r| r.id == src.id);
        match shows.as_slice() {
            [(w, widget)] if one(widget) => {
                merged.insert(*w, fid);
            }
            many if many.iter().any(|(_, w)| one(w)) => {
                split.insert(fid);
            }
            _ => {}
        }
    }
    let primary = cx.primary.id();
    let own = FormInst {
        src: primary,
        copy: 0,
    };
    for (fid, node) in nodes(st, scoped.as_ref()) {
        if let Some(r) = &node.src
            && r.src.id() == primary
            && node.inst == own
        {
            cx.own(ObjKey::Field(fid), r.id);
        }
    }
    let others = (sources(nodes(st, scoped.as_ref())).into_values())
        .filter(|src| src.id() != primary)
        .collect();
    let file = cx.primary.form();
    let modeled = file.facts.damaged.is_none()
        && (file.fields.is_empty() || st.form().names.contains_key(&own));
    let mut plan = Plan {
        merged,
        split,
        dr: None,
        renamed: HashMap::new(),
        others,
        modeled,
        keys: Vec::new(),
        touched: !modeled,
        scoped,
    };
    if !plan.others.is_empty() {
        merge_dr(cx, &mut plan);
    }
    plan
}

/// The field nodes with a widget on `pages`, and the nodes above them: an
/// overlay's.
fn scoped(st: &DocState, pages: impl Iterator<Item = PageId>) -> BTreeSet<FieldId> {
    let mut nodes = BTreeSet::new();
    for page in pages {
        for (_, w) in st.widgets_on(page) {
            let mut at = Some(w.field);
            while let Some(fid) = at.filter(|&fid| nodes.insert(fid)) {
                at = st.field(fid).and_then(|node| node.parent);
            }
        }
    }
    nodes
}

/// The field nodes of `st` an emission writes, by id: every one, or those
/// `scoped` holds.
fn nodes<'s>(
    st: &'s DocState,
    scoped: Option<&'s BTreeSet<FieldId>>,
) -> Box<dyn Iterator<Item = (FieldId, &'s Arc<FieldNode>)> + 's> {
    match scoped {
        None => Box::new(st.fields()),
        Some(scoped) => Box::new((scoped.iter()).filter_map(|&fid| Some((fid, st.field(fid)?)))),
    }
}

/// The sources the form instances of `nodes` are of, by their blobs: what
/// their fields were read from.
fn sources<'s>(
    nodes: impl Iterator<Item = (FieldId, &'s Arc<FieldNode>)>,
) -> BTreeMap<BlobId, Src> {
    (nodes.filter_map(|(_, node)| node.src.as_ref()))
        .map(|r| (r.src.id(), r.src.clone()))
        .collect()
}

/// The files the form `st` holds is made of, as a written form's check
/// compares it with them: `primary`, then the others its instances are of.
pub(crate) fn inputs(st: &DocState, primary: &Src) -> Vec<Src> {
    let others = sources(st.fields())
        .into_values()
        .filter(|src| src != primary);
    std::iter::once(primary.clone()).chain(others).collect()
}

/// The form dictionary `src`'s catalog names, its number when it's an
/// object of its own.
fn form_of(src: &Src) -> Option<(Option<ObjectId>, &Dictionary)> {
    let pdf = src.pdf();
    let at = pdf.catalog().ok()?.get(b"AcroForm").ok()?;
    let (id, form) = pdf.dereference(at).ok()?;
    Some((id, form.as_dict().ok()?))
}

/// `key` of `dict`, references followed in `pdf`, if it's a dictionary.
fn dict_at<'a>(pdf: &'a LazyPdf, dict: &'a Dictionary, key: &[u8]) -> Option<&'a Dictionary> {
    pdf.dereference(dict.get(key).ok()?).ok()?.1.as_dict().ok()
}

/// `/DR`: the primary's, each category read whole, with each other file's
/// entries merged in. A name it lacks is copied in; a font of a name it
/// has that isn't the same font comes in under a new name, which the
/// plan's renames give that file's `/DA`s.
fn merge_dr(cx: &mut Cx, plan: &mut Plan) {
    let primary = cx.primary.clone();
    let mut dr = Dictionary::new();
    // Where each entry came from, as read: what a font is compared by.
    let mut origin: HashMap<(Vec<u8>, Vec<u8>), (Src, Object)> = HashMap::new();
    let files = std::iter::once(primary.clone()).chain(plan.others.iter().cloned());
    for src in files {
        let pdf = src.pdf();
        let Some((_, form)) = form_of(&src) else {
            continue;
        };
        let Some(theirs) = dict_at(pdf, form, b"DR") else {
            continue;
        };
        let scope = Scope { src: &src, copy: 0 };
        let mut renamed = HashMap::new();
        for (kind, entries) in theirs.iter() {
            let Some(entries) = (pdf.dereference(entries).ok()).and_then(|(_, e)| e.as_dict().ok())
            else {
                continue;
            };
            let mut ours = match dr.get(kind) {
                Ok(Object::Dictionary(d)) => d.clone(),
                _ => Dictionary::new(),
            };
            for (name, value) in entries.iter() {
                let at = (kind.clone(), name.clone());
                let new = match origin.get(&at) {
                    None => name.clone(),
                    Some((mine, read))
                        if kind == b"Font" && !same_font((mine, read), (&src, value)) =>
                    {
                        let new = (2..)
                            .map(|n| [name.as_slice(), format!("_{n}").as_bytes()].concat())
                            .find(|n| !ours.has(n) && !entries.has(n))
                            .expect("a free name");
                        renamed.insert(name.clone(), new.clone());
                        new
                    }
                    Some(_) => continue,
                };
                let v = cx.sym(value, &scope);
                ours.set(new.clone(), v);
                origin.insert((kind.clone(), new), (src.clone(), value.clone()));
            }
            dr.set(kind.clone(), ours);
        }
        if !renamed.is_empty() {
            plan.renamed.insert(src.id(), renamed);
        }
    }
    plan.dr = (!dr.is_empty()).then_some(dr);
}

/// Whether two fonts of a `/DR` are the same: the same `/Subtype`,
/// `/BaseFont` and named `/Encoding` (a font with an encoding of its own
/// isn't taken for another).
fn same_font((a_src, a): (&Src, &Object), (b_src, b): (&Src, &Object)) -> bool {
    let read = |src: &Src, o: &Object| -> Option<Dictionary> {
        Some(src.pdf().dereference(o).ok()?.1.as_dict().ok()?.clone())
    };
    let (Some(a), Some(b)) = (read(a_src, a), read(b_src, b)) else {
        return false;
    };
    let name = |d: &Dictionary, k: &[u8]| {
        d.get(k)
            .ok()
            .and_then(|v| v.as_name().ok())
            .map(<[u8]>::to_vec)
    };
    let plain = |d: &Dictionary| d.get(b"Encoding").map_or(true, |e| e.as_name().is_ok());
    plain(&a)
        && plain(&b)
        && [b"Subtype".as_slice(), b"BaseFont", b"Encoding"]
            .iter()
            .all(|k| name(&a, k) == name(&b, k))
        && name(&a, b"BaseFont").is_some()
}

/// A `/DA` string with each name in `renamed` replaced.
pub(crate) fn rename_in_da(da: &[u8], renamed: &HashMap<Vec<u8>, Vec<u8>>) -> Vec<u8> {
    let delimiter = |b: u8| b.is_ascii_whitespace() || b"()<>[]{}/%".contains(&b);
    let mut out = Vec::with_capacity(da.len());
    let mut i = 0;
    while i < da.len() {
        if da[i] == b'/' {
            let start = i + 1;
            let mut end = start;
            while end < da.len() && !delimiter(da[end]) {
                end += 1;
            }
            out.push(b'/');
            match renamed.get(&da[start..end]) {
                Some(new) => out.extend_from_slice(new),
                None => out.extend_from_slice(&da[start..end]),
            }
            i = end;
        } else {
            out.push(da[i]);
            i += 1;
        }
    }
    out
}

impl Cx<'_> {
    /// `v`, a `/DA` of an object of `src`, its fonts renamed as `/DR`'s
    /// merge renamed them.
    fn da(&mut self, v: &Object, src: &Src, scope: &Scope) -> Object {
        match (v, self.form.renamed.get(&src.id())) {
            (Object::String(da, format), Some(renamed)) => {
                Object::String(rename_in_da(da, renamed), *format)
            }
            _ => self.sym(v, scope),
        }
    }

    /// Whether page `id` keeps its `/StructParents`: it's copy 0 of a
    /// primary page.
    pub(crate) fn keeps_tags(&self, id: PageId) -> bool {
        self.shown
            .get(&id)
            .is_some_and(|s| s.copy == 0 && s.src.id() == self.primary.id())
    }
}

/// Widget `aid`, `w`, on page `page` (`tags`: the page keeps its tags),
/// emitted: its entry in `/Annots`. A widget that's its field's one
/// object is the field's, written with the form.
pub(crate) fn widget(
    cx: &mut Cx,
    aid: AnnotId,
    w: &Widget,
    page: PageId,
    tags: bool,
) -> Option<Object> {
    if let Some(field) = cx.form.merged_into(aid) {
        return Some(cx.placeholder(ObjKey::Field(field)));
    }
    let st = cx.st;
    let r = w.src.as_ref()?;
    let dict = r.src.pdf().get_dictionary(r.id).ok()?;
    let scope = Scope {
        src: &r.src,
        copy: w.inst.copy,
    };
    let split = cx.form.split.contains(&w.field);
    let key = ObjKey::Annot(aid);
    // Nothing of it changes: the file's object as it is, unbuilt.
    let named = |k: &[u8], to: Option<ObjectId>| match dict.get(k) {
        Ok(&Object::Reference(r)) => Some(r) == to,
        _ => false,
    };
    let field = cx.number_known(ObjKey::Field(w.field));
    let page_at = cx.number_known(ObjKey::Page(page));
    if tags
        && !split
        && cx.owned_at(key) == Some(r.id)
        && named(b"Parent", field)
        && (!dict.has(b"P") || named(b"P", page_at))
        && !cx.form.renamed.contains_key(&r.src.id())
        && !cx.hides(aid)
    {
        cx.push_raw(key, r.clone());
        cx.form.keys.push(key);
        return Some(cx.placeholder(key));
    }
    let mut out = Dictionary::new();
    for (k, v) in dict.iter() {
        match k.as_slice() {
            b"Parent" => {}
            b"P" => out.set("P", cx.placeholder(ObjKey::Page(page))),
            b"StructParent" if !tags => {}
            k if split && FIELD_KEYS.contains(&k) => {}
            b"DA" => {
                let v = cx.da(v, &r.src, &scope);
                out.set("DA", v);
            }
            _ => {
                let v = cx.sym(v, &scope);
                out.set(k.clone(), v);
            }
        }
    }
    if st.field(w.field).is_some() {
        out.set("Parent", cx.placeholder(ObjKey::Field(w.field)));
    }
    cx.hidden(aid, &mut out);
    put(cx, key, out, r, dict);
    Some(cx.placeholder(key))
}

/// `out`, built from `read`, object `r`, pushed under `key`: as the file
/// has it when `key` keeps `r`'s number and nothing in it changed.
fn put(cx: &mut Cx, key: ObjKey, out: Dictionary, r: &ObjRef, read: &Dictionary) {
    if cx.owned_at(key) == Some(r.id) && cx.unchanged(&out, read) {
        cx.push_raw(key, r.clone());
        cx.form.keys.push(key);
    } else {
        built(cx, key, out);
    }
}

/// `out`, made anew, pushed under `key`, one of the form's.
fn built(cx: &mut Cx, key: ObjKey, out: Dictionary) {
    // An Append writes it only when it's in scope.
    cx.form.touched |= cx.writes(&key);
    cx.push(key, Object::Dictionary(out));
    cx.form.keys.push(key);
}

/// The form's dictionary, its fields emitted: a reference to it, or None
/// when there's no form (no fields, and none in the primary).
pub(crate) fn acroform(cx: &mut Cx) -> Option<Object> {
    let st = cx.st;
    let primary = cx.primary.clone();
    let read = form_of(&primary);
    if st.field_count() == 0 && read.is_none() {
        return None;
    }
    let doc = st.form();
    let transforms = cx.transforms;
    let drop_xfa = (transforms.iter()).find_map(|t| match t {
        SaveTransform::DropXfa { tooltips_to_tu } => Some(*tooltips_to_tu),
        _ => None,
    });
    let fix_flags = transforms.contains(&SaveTransform::FixSigFlags);
    let mut fields = Fields {
        drop_xfa,
        scoped: cx.form.scoped.clone(),
        written: HashSet::new(),
        gone: HashSet::new(),
        shown: HashMap::new(),
        accounted: HashSet::new(),
        file: HashMap::new(),
    };
    // What the records account for of the primary's own instance: a kid
    // of one of its fields that's none of these, and on no page, is on a
    // page template. An overlay lists only what's on its pages.
    if fields.scoped.is_none() {
        for (_, node) in st.fields() {
            if node.inst.copy == 0
                && let Some(r) = node.src.as_ref().filter(|r| r.src.id() == primary.id())
            {
                fields.accounted.insert(r.id);
            }
        }
        for (_, rec) in st.annots() {
            if let Some(w) = rec.widget()
                && w.inst.copy == 0
                && let Some(r) = w.src.as_ref().filter(|r| r.src.id() == primary.id())
            {
                fields.accounted.insert(r.id);
            }
        }
    }
    // An overlay's roots are those of its fields, by id.
    let tops: Vec<FieldId> = match &fields.scoped {
        None => doc.roots.iter().copied().collect(),
        Some(scoped) => (scoped.iter().copied())
            .filter(|&fid| st.field(fid).is_some_and(|node| node.parent.is_none()))
            .collect(),
    };
    let mut roots = Vec::new();
    for root in tops {
        if fields.field(cx, root) {
            roots.push(cx.placeholder(ObjKey::Field(root)));
        }
    }
    let scope = Scope {
        src: &primary,
        copy: 0,
    };
    // A tree the state doesn't model (a damaged one, whose fields aren't
    // records) is kept as the file has it, with every key the form holds,
    // and the roots of the forms that came in after.
    let damaged = !cx.form.modeled;
    // A form left with no fields goes, as `keep_only` takes one out:
    // Acrobat would offer to fill nothing, and an XFA copy beside it would
    // describe fields that are gone. One that never had any, as sheaf
    // reads its file, stays.
    let had = st.field_count() > 0 || !primary.form().nodes.is_empty();
    if !damaged && roots.is_empty() && had {
        return None;
    }
    // An Append writes the keys the state doesn't own as the saved file
    // holds them: one a save dropped stays gone, but for an XFA copy save
    // policy keeps now, the fields as the file had them again (§11.8).
    let saved = cx.saved().and_then(|record| record.roots.acroform.as_ref());
    let mut out = Dictionary::new();
    for (k, v) in read.iter().flat_map(|(_, d)| d.iter()) {
        let unowned = !ACROFORM_OWNED.contains(&k.as_slice());
        if !(unowned || damaged) || (k == b"XFA" && drop_xfa.is_some()) {
            continue;
        }
        let v = match saved.filter(|_| unowned && k != b"XFA") {
            Some(saved) => match saved.get(k) {
                Ok(v) => v,
                Err(_) => continue,
            },
            None => v,
        };
        let v = cx.sym(v, &scope);
        out.set(k.clone(), v);
    }
    if damaged {
        let pdf = primary.pdf();
        let listed = (read.and_then(|(_, d)| d.get(b"Fields").ok()))
            .and_then(|f| pdf.dereference(f).ok())
            .and_then(|(_, f)| f.as_array().ok().cloned())
            .unwrap_or_default();
        let mut fields: Vec<Object> = listed.iter().map(|f| cx.sym(f, &scope)).collect();
        fields.extend(roots);
        out.set("Fields", fields);
        if let Some(dr) = cx.form.dr.clone() {
            out.set("DR", dr);
        }
        return Some(push_form(cx, out, read));
    }
    out.set("Fields", roots);
    let order: Vec<Object> = (doc.calc_order.iter())
        .filter(|f| fields.written.contains(f))
        .map(|&f| cx.placeholder(ObjKey::Field(f)))
        .collect();
    if !order.is_empty() {
        out.set("CO", order);
    }
    if let Some(da) = &doc.da {
        out.set(
            "DA",
            Object::String(da.as_bytes().to_vec(), StringFormat::Literal),
        );
    }
    if doc.align != 0 {
        out.set("Q", i64::from(doc.align));
    }
    match (&cx.form.dr, read.and_then(|(_, d)| d.get(b"DR").ok())) {
        (Some(dr), _) => out.set("DR", dr.clone()),
        (None, Some(dr)) => {
            let dr = cx.sym(dr, &scope);
            out.set("DR", dr);
        }
        (None, None) => {}
    }
    if doc.need_appearances {
        out.set("NeedAppearances", true);
    }
    let needed = || {
        let signed = (st.terminals())
            .any(|(_, _, t)| t.kind == FieldKind::Signature && t.value == Some(Value::Signed));
        signed || primary.form().facts.certified
    };
    if doc.sig_flags != 0 && !(fix_flags && !needed()) {
        out.set("SigFlags", doc.sig_flags);
    }
    Some(push_form(cx, out, read))
}

/// `out`, the form's dictionary, pushed (at the primary's form's number,
/// `read`, when it's an object): a reference to it.
fn push_form(
    cx: &mut Cx,
    out: Dictionary,
    read: Option<(Option<ObjectId>, &Dictionary)>,
) -> Object {
    let key = ObjKey::AcroForm;
    match read {
        Some((Some(at), dict)) if cx.own(key, at) => {
            let r = ObjRef {
                src: cx.primary.clone(),
                id: at,
            };
            put(cx, key, out, &r, dict);
        }
        _ => built(cx, key, out),
    }
    cx.placeholder(key)
}

/// The field tree as it's written.
struct Fields {
    /// `DropXfa`'s `tooltips_to_tu`, when the save drops XFA.
    drop_xfa: Option<bool>,
    /// An overlay's nodes ([`Plan::scoped`]): None for a save.
    scoped: Option<BTreeSet<FieldId>>,
    /// The nodes written so far.
    written: HashSet<FieldId>,
    /// The nodes left with no widgets, which aren't written.
    gone: HashSet<FieldId>,
    /// What each file's form showed, as sheaf read it: the objects of its
    /// nodes with a widget under them, on a page or a page template.
    shown: HashMap<BlobId, HashSet<ObjectId>>,
    /// The primary's objects its own instance's records were read as.
    accounted: HashSet<ObjectId>,
    /// What each file's form holds: its nodes' objects and its widgets on
    /// pages.
    file: HashMap<BlobId, HashSet<ObjectId>>,
}

impl Fields {
    /// Node `fid` and the nodes under it, emitted: whether it was (a node
    /// the state hasn't got, or one met again, isn't). A node left with no
    /// widgets isn't: one that showed as its file was read ([`showed`]) and
    /// shows nowhere now, with no node under it written, as `keep_only`
    /// takes such a field out of a file (bug 14). An edit that takes a
    /// node's last widget takes the node too (the `widget.field` cascade,
    /// M10.5 §6.5); the rule here keeps what's written to what the file
    /// showed, whatever the state holds. Nor is a node that isn't a field
    /// with a value (no terminal) and has no node under it written, as one
    /// whose `/Kids` another node holds: written, it would be a typeless
    /// terminal field of its own, and a duplicate where its name is
    /// another's (Greg, 2026-10-22).
    ///
    /// [`showed`]: Self::showed
    fn field(&mut self, cx: &mut Cx, fid: FieldId) -> bool {
        let st = cx.st;
        let Some(node) = st.field(fid) else {
            return false;
        };
        if self.gone.contains(&fid) || !self.written.insert(fid) {
            return false;
        }
        let mut kids = Vec::new();
        for &kid in &node.kids {
            let within = self.scoped.as_ref().is_none_or(|s| s.contains(&kid));
            if within && self.field(cx, kid) {
                kids.push(cx.placeholder(ObjKey::Field(kid)));
            }
        }
        let shows = (node.term.as_ref()).is_some_and(|t| !t.widgets.is_empty() || t.outside > 0);
        if kids.is_empty() && !shows && (node.term.is_none() || self.showed(node)) {
            self.written.remove(&fid);
            self.gone.insert(fid);
            return false;
        }
        match &node.src {
            Some(r) => self.read_from(cx, fid, node, r, kids),
            // A parent sheaf made for an instance whose names clashed.
            None => {
                let mut out = Dictionary::new();
                out.set("T", crate::save::text_string(&node.partial));
                if !kids.is_empty() {
                    out.set("Kids", kids);
                }
                if let Some(parent) = node.parent {
                    out.set("Parent", cx.placeholder(ObjKey::Field(parent)));
                }
                self.push_down(cx, node, &mut out);
                built(cx, ObjKey::Field(fid), out);
            }
        }
        true
    }

    /// Whether node `node` showed as its file was read: a widget of it, or
    /// of a node under it, was on a page or a page template. One sheaf made,
    /// whether it had nodes under it. A node whose `/Kids` no read took
    /// (another node's, by their `/Parent`) didn't.
    fn showed(&mut self, node: &FieldNode) -> bool {
        let Some(r) = &node.src else {
            return !node.kids.is_empty();
        };
        let shown = self.shown.entry(r.src.id()).or_insert_with(|| {
            let form = r.src.form();
            let mut marked = vec![false; form.nodes.len()];
            let terminals = (form.widgets.values().flatten().map(|w| w.field))
                .chain(form.fields.iter().filter(|f| f.outside > 0).map(|f| f.id));
            for FieldId(mut at) in terminals {
                // Up the tree, until a node already marked.
                while let Some(seen) = marked.get_mut(at as usize).filter(|m| !**m) {
                    *seen = true;
                    match form.nodes[at as usize].parent {
                        Some(FieldId(up)) => at = up,
                        None => break,
                    }
                }
            }
            (form.nodes.iter().zip(marked))
                .filter_map(|(n, m)| m.then_some(n.object))
                .collect()
        });
        shown.contains(&r.id)
    }

    /// Node `fid`, `node`, read as `r`, emitted, with `kids` (its nodes')
    /// and its widgets in its `/Kids`. Nothing when it can't be read.
    fn read_from(
        &mut self,
        cx: &mut Cx,
        fid: FieldId,
        node: &FieldNode,
        r: &ObjRef,
        kids: Vec<Object>,
    ) {
        let st = cx.st;
        let Ok(read) = r.src.pdf().get_dictionary(r.id) else {
            return;
        };
        let copy = node.inst.copy;
        let scope = Scope { src: &r.src, copy };
        let term = node.term.as_ref();
        let split = cx.form.split.contains(&fid);
        // The widget it's one object with, and that widget's page.
        let merged = (term.into_iter().flat_map(|t| t.widgets.iter()))
            .find(|&&w| cx.form.merged_into(w) == Some(fid))
            .copied();
        let one = merged.and_then(|w| Some(st.annot(w)?.page));
        let tags = one.is_some_and(|page| cx.keeps_tags(page));
        let mut kids = kids;
        for &w in term.into_iter().flat_map(|t| t.widgets.iter()) {
            // An overlay's are those on its pages.
            let listed = |page| self.scoped.is_none() || cx.shown.contains_key(&page);
            let widget = |a: &AnnotRec| a.widget().is_some() && listed(a.page);
            if cx.form.merged_into(w).is_none() && st.annot(w).is_some_and(widget) {
                kids.push(cx.placeholder(ObjKey::Annot(w)));
            }
        }
        // Widgets on page templates: a kid of the primary's own instance
        // nothing accounts for and that no page shows, when it's a widget.
        // Any other such kid isn't the field's to write: a page, or the
        // catalog, as 0092321's `Page2[0]` lists it, copied as a kid would
        // carry what the save wrote anew, a removed generator's text in
        // the catalog's `/SheafGenerators` among it (Greg, 2026-10-22).
        let primary = cx.primary.id();
        if copy == 0 && r.src.id() == primary && !split && self.scoped.is_none() {
            let pdf = r.src.pdf();
            let listed = (read.get(b"Kids").ok())
                .and_then(|k| pdf.dereference(k).ok())
                .and_then(|(_, k)| k.as_array().ok());
            let known = self.file.entry(primary).or_insert_with(|| {
                let form = r.src.form();
                (form.nodes.iter().map(|n| n.object))
                    .chain(form.widgets.values().flatten().map(|w| w.object))
                    .collect()
            });
            let widget = |k: &ObjectId| {
                (pdf.get_dictionary(*k).ok())
                    .and_then(|d| d.get(b"Subtype").ok())
                    .is_some_and(|s| s.as_name().is_ok_and(|s| s == b"Widget"))
            };
            let outside: Vec<ObjectId> = (listed.into_iter().flatten())
                .filter_map(|k| k.as_reference().ok())
                .filter(|k| !self.accounted.contains(k) && !known.contains(k))
                .filter(widget)
                .collect();
            for k in outside {
                let k = cx.sym(&Object::Reference(k), &scope);
                kids.push(k);
            }
        }
        let key = ObjKey::Field(fid);
        let tip = (self.drop_xfa == Some(true) && !read.has(b"TU"))
            .then(|| term.and_then(|t| t.props.tooltip.as_deref()))
            .flatten();
        // Nothing of it changes: the file's object as it is, unbuilt.
        let (da, q) = self.defaults(cx, node);
        let defaults_kept = da.is_none_or(|_| read.has(b"DA")) && q.is_none_or(|_| read.has(b"Q"));
        if cx.owned_at(key) == Some(r.id)
            && !split
            && tip.is_none()
            && defaults_kept
            && (one.is_none() || tags)
            && merged.is_none_or(|w| !cx.hides(w))
            && self.as_read(cx, node, read, &kids, one)
        {
            cx.push_raw(key, r.clone());
            cx.form.keys.push(key);
            return;
        }
        let mut out = Dictionary::new();
        for (k, v) in read.iter() {
            match k.as_slice() {
                b"Parent" | b"Kids" => {}
                k if split && WIDGET_KEYS.contains(&k) => {}
                b"P" if one.is_some() => {
                    let page = one.expect("its widget's page");
                    out.set("P", cx.placeholder(ObjKey::Page(page)));
                }
                b"StructParent" if one.is_some() && !tags => {}
                b"DA" => {
                    let v = cx.da(v, &r.src, &scope);
                    out.set("DA", v);
                }
                _ => {
                    let v = cx.sym(v, &scope);
                    out.set(k.clone(), v);
                }
            }
        }
        if !kids.is_empty() {
            out.set("Kids", kids);
        }
        if let Some(tip) = tip {
            out.set("TU", crate::save::text_string(tip));
        }
        if let Some(parent) = node.parent {
            out.set("Parent", cx.placeholder(ObjKey::Field(parent)));
        }
        self.push_down(cx, node, &mut out);
        if let Some(w) = merged {
            cx.hidden(w, &mut out);
        }
        put(cx, ObjKey::Field(fid), out, r, read);
    }

    /// Whether `read`, node `node`'s dictionary as its primary file has
    /// it, names what the save writes: its `/Parent` its parent's number,
    /// its `/Kids` (a direct array) the numbers of `kids`, and, merged with
    /// its widget on page `one`, its `/P` that page's.
    fn as_read(
        &self,
        cx: &Cx,
        node: &FieldNode,
        read: &Dictionary,
        kids: &[Object],
        one: Option<PageId>,
    ) -> bool {
        let at = |o: &Object| -> Option<ObjectId> {
            cx.number_known(cx.syms.key(o.as_reference().ok()?)?)
        };
        let names = |k: &[u8], to: Option<ObjectId>| match read.get(k) {
            Err(_) => to.is_none(),
            Ok(&Object::Reference(r)) => to == Some(r),
            Ok(_) => false,
        };
        let parent = node.parent.map(|p| cx.number_known(ObjKey::Field(p)));
        let parent_kept = match parent {
            None => !read.has(b"Parent"),
            Some(p) => p.is_some() && names(b"Parent", p),
        };
        let kids_kept = match read.get(b"Kids") {
            Err(_) => kids.is_empty(),
            // An empty `/Kids` isn't written.
            Ok(Object::Array(listed)) => {
                !listed.is_empty()
                    && listed.len() == kids.len()
                    && (listed.iter().zip(kids))
                        .all(|(l, k)| l.as_reference().ok().is_some_and(|l| at(k) == Some(l)))
            }
            Ok(_) => false,
        };
        let page_kept = match one {
            None => true,
            Some(page) => names(b"P", cx.number_known(ObjKey::Page(page))),
        };
        parent_kept && kids_kept && page_kept
    }

    /// Instance `node.inst`'s form's `/DA` and `/Q`, where they differ
    /// from the document's, onto `out`, `node`'s dictionary, when it's the
    /// top of its instance's tree (a root, or a parent sheaf made) and has
    /// none of its own ([`defaults`](Self::defaults)).
    fn push_down(&self, cx: &mut Cx, node: &FieldNode, out: &mut Dictionary) {
        let (da, q) = self.defaults(cx, node);
        if let Some(da) = da.filter(|_| !out.has(b"DA")) {
            out.set("DA", Object::String(da, StringFormat::Literal));
        }
        if let Some(q) = q.filter(|_| !out.has(b"Q")) {
            out.set("Q", i64::from(q));
        }
    }

    /// The `/DA` and `/Q` instance `node.inst`'s form gives its fields
    /// where they differ from the document's, when `node` is the top of its
    /// instance's tree: what [`push_down`](Self::push_down) puts on it. An
    /// overlay's own source may be a file inserted, its form's defaults
    /// the document's only when it's the file the document opened: so its
    /// roots get them too, as a save's other files' do.
    fn defaults(&self, cx: &Cx, node: &FieldNode) -> (Option<Vec<u8>>, Option<u8>) {
        let st = cx.st;
        let own = node.inst.src == cx.primary.id();
        if node.parent.is_some() || (own && cx.overlay.is_none()) {
            return (None, None);
        }
        let src = match own {
            true => cx.primary,
            false => match (cx.form.others.iter()).find(|src| src.id() == node.inst.src) {
                Some(src) => src,
                None => return (None, None),
            },
        };
        let facts = &src.form().facts;
        let doc = st.form();
        let renamed = cx.form.renamed.get(&src.id());
        let da = (facts.da.as_deref()).map(|da| match renamed {
            Some(renamed) => rename_in_da(da.as_bytes(), renamed),
            None => da.as_bytes().to_vec(),
        });
        let da = da.filter(|da| Some(da.as_slice()) != doc.da.as_deref().map(str::as_bytes));
        (da, (facts.align != doc.align).then_some(facts.align))
    }
}

/// The catalog's `/Names /JavaScript` when forms from other files bring
/// document scripts: the primary's with theirs beside them, a name taken
/// already renamed ("Init 2"), but Adobe's version checks (`!ADBE::`),
/// which are the same everywhere and never run in sheaf. A reference to
/// it, or None when no other file brings any.
pub(crate) fn scripts(cx: &mut Cx) -> Option<Object> {
    let others = cx.form.others.clone();
    let mut theirs = Vec::new();
    for src in &others {
        let scope = Scope { src, copy: 0 };
        for (name, script) in document_scripts(src) {
            let script = cx.sym(&script, &scope);
            theirs.push((name, script));
        }
    }
    if theirs.is_empty() {
        return None;
    }
    let primary = cx.primary.clone();
    let scope = Scope {
        src: &primary,
        copy: 0,
    };
    let mut all = Vec::new();
    for (name, script) in document_scripts(&primary) {
        let script = cx.sym(&script, &scope);
        all.push((name, script));
    }
    let mut taken: HashSet<Vec<u8>> = all.iter().map(|(k, _)| k.clone()).collect();
    for (key, script) in theirs {
        let key = if !taken.contains(&key) {
            key
        } else if key.starts_with(b"!ADBE::") {
            continue;
        } else {
            (2..)
                .map(|n| [key.as_slice(), format!(" {n}").as_bytes()].concat())
                .find(|k| !taken.contains(k))
                .expect("a free name")
        };
        taken.insert(key.clone());
        all.push((key, script));
    }
    all.sort_by(|a, b| a.0.cmp(&b.0));
    let pairs: Vec<Object> = (all.into_iter())
        .flat_map(|(k, v)| [Object::String(k, StringFormat::Literal), v])
        .collect();
    let mut tree = Dictionary::new();
    tree.set("Names", pairs);
    let key = ObjKey::Names(NameTree::JavaScript);
    cx.push(key, Object::Dictionary(tree));
    Some(cx.placeholder(key))
}

/// `src`'s document scripts (`/Names /JavaScript`), by name, at most
/// [`MAX_SCRIPTS`], each node of the tree read once.
fn document_scripts(src: &Src) -> Vec<(Vec<u8>, Object)> {
    let pdf = src.pdf();
    let mut found = Vec::new();
    let tree = (pdf.catalog().ok())
        .and_then(|c| dict_at(pdf, c, b"Names"))
        .and_then(|n| n.get(b"JavaScript").ok());
    if let Some(tree) = tree {
        name_tree(pdf, tree, &mut found, &mut HashSet::new(), 0);
    }
    found
}

/// A name tree's entries, at most [`MAX_SCRIPTS`], each node read once.
fn name_tree(
    pdf: &LazyPdf,
    node: &Object,
    out: &mut Vec<(Vec<u8>, Object)>,
    seen: &mut HashSet<ObjectId>,
    depth: u32,
) {
    if depth > 32 || out.len() >= MAX_SCRIPTS {
        return;
    }
    if let Ok(id) = node.as_reference()
        && !seen.insert(id)
    {
        return;
    }
    let Some(d) = (pdf.dereference(node).ok()).and_then(|(_, n)| n.as_dict().ok()) else {
        return;
    };
    let pairs = (d.get(b"Names").ok())
        .and_then(|n| pdf.dereference(n).ok())
        .and_then(|(_, n)| n.as_array().ok());
    for pair in pairs.into_iter().flat_map(|p| p.chunks(2)) {
        if let [Object::String(k, _), v] = pair
            && out.len() < MAX_SCRIPTS
        {
            out.push((k.clone(), v.clone()));
        }
    }
    let kids = (d.get(b"Kids").ok())
        .and_then(|k| pdf.dereference(k).ok())
        .and_then(|(_, k)| k.as_array().ok());
    for kid in kids.into_iter().flatten() {
        name_tree(pdf, kid, out, seen, depth + 1);
    }
}

/// `/Perms` as a save writes it, `perms` the primary's: without Reader's
/// usage rights (`/UR3`, and the older `/UR`) when the save drops them, or
/// gone when that leaves it empty.
pub(crate) fn perms(cx: &mut Cx, perms: &Object) -> Option<Object> {
    let primary = cx.primary.clone();
    let scope = Scope {
        src: &primary,
        copy: 0,
    };
    if !cx.transforms.contains(&SaveTransform::DropUr3) {
        return Some(cx.sym(perms, &scope));
    }
    let pdf = primary.pdf();
    let read = pdf.dereference(perms).ok()?.1.as_dict().ok()?;
    let mut out = Dictionary::new();
    for (k, v) in read
        .iter()
        .filter(|(k, _)| !matches!(k.as_slice(), b"UR3" | b"UR"))
    {
        let v = cx.sym(v, &scope);
        out.set(k.clone(), v);
    }
    (!out.is_empty()).then_some(Object::Dictionary(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn da_fonts_are_renamed_by_name() {
        let renamed = HashMap::from([(b"Helv".to_vec(), b"Helv_2".to_vec())]);
        assert_eq!(
            rename_in_da(b"/Helv 0 Tf 0 g", &renamed),
            b"/Helv_2 0 Tf 0 g"
        );
        // Only whole names.
        assert_eq!(rename_in_da(b"/HelvB 9 Tf", &renamed), b"/HelvB 9 Tf");
        assert_eq!(
            rename_in_da(b"0 g /Helv 12 Tf", &renamed),
            b"0 g /Helv_2 12 Tf"
        );
    }

    /// The keys a merged dictionary is split by are each the field's or the
    /// widget's, never both.
    #[test]
    fn field_and_widget_keys_are_apart() {
        let widget: std::collections::BTreeSet<&[u8]> = WIDGET_KEYS.iter().copied().collect();
        assert!(FIELD_KEYS.iter().all(|k| !widget.contains(k)));
    }
}
