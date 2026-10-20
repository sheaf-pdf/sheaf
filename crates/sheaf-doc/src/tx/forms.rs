//! Forms as pages bring them (M10.5 §3.7; M10 slices 4–5): every insert,
//! replace or duplicate of a form's pages is an instance of its own
//! ([`FormInst`]), its fields renamed apart under a new parent when their
//! names are taken, and scripts turned off once they come from more than
//! one instance. What the reader is told comes out as [`Notice`]s.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use im::Vector;

use super::{Lower, Notice};
use crate::annot::AnnotId;
use crate::forms::{FieldId, full_name_of};
use crate::model::{
    AnnotBody, AnnotRec, DocState, FieldNode, FormDoc, FormInst, PageId, ScriptState, Terminal,
    Widget,
};
use crate::prim::{Key, Rec, Val};
use crate::source::ObjRef;
use crate::store::{Src, Store};

/// Each page index's widgets that came in, with their places in its
/// `/Annots`.
pub(crate) type Placed = HashMap<u32, Vec<(usize, AnnotId)>>;

/// The form of `src` that comes in with its pages (`shown`: each page's
/// index in the source, and the page showing it), as an instance of its
/// own, named `name`: its fields with widgets on those pages, its fields
/// with no widget at all, and the nodes above them, each with a new id,
/// and their widgets on those pages. When any of their full names, or a
/// parent of one, is a node's in the document, the instance goes under a
/// new parent named for the file, and the reader is told. The instance,
/// when any field came; and the widgets, by page index, each with its
/// place in its page's `/Annots`, which the annotations that came are put
/// among.
pub(crate) fn take_in(
    lw: &mut Lower,
    src: &Src,
    name: &Arc<str>,
    shown: &[(u32, PageId)],
) -> (Option<FormInst>, Placed) {
    let form = src.form().clone();
    let on_pages: HashSet<FieldId> = (shown.iter())
        .flat_map(|(index, _)| form.widgets.get(index).into_iter().flatten())
        .map(|w| w.field)
        .collect();
    if on_pages.is_empty() {
        return (None, Placed::new());
    }
    let with_widgets: HashSet<FieldId> = form.widgets.values().flatten().map(|w| w.field).collect();
    // The nodes that come in, by the file's ids, in its tree's order:
    // those fields, and every node above one.
    let mut coming = BTreeSet::new();
    for f in &form.fields {
        if on_pages.contains(&f.id) || !with_widgets.contains(&f.id) {
            let mut at = Some(f.id);
            while let Some(id) = at.filter(|&id| coming.insert(id)) {
                at = form.nodes[id.0 as usize].parent;
            }
        }
    }
    let mut doc = FormDoc::clone(lw.rec.form());
    let ids: HashMap<FieldId, FieldId> = (coming.iter()).map(|&n| (n, lw.field_id())).collect();
    let inst = FormInst {
        src: src.id(),
        copy: instance_copy(&doc, src.id(), lw.cx.primary.id()),
    };
    let names = (form.fields.iter())
        .filter(|f| coming.contains(&f.id))
        .map(|f| f.name.as_str());
    let under = apart(&lw.rec, name, names);
    doc.names.insert(inst, name.clone());
    // What its form owns joins the document's: its fields that
    // calculate after the rest, and whether it needs appearances.
    let calculates = form.facts.calculation_order.iter();
    doc.calc_order
        .extend(calculates.filter_map(|f| ids.get(f)).copied());
    doc.need_appearances |= form.facts.need_appearances;
    // The node its roots go under, if any: one named apart now.
    let top = under.as_deref().map(|under| {
        let id = lw.field_id();
        put_field(&mut lw.rec, id, sheaf_parent(under, inst));
        doc.roots.push_back(id);
        id
    });
    // The widgets on the pages coming in, each with a new id.
    let mut shows: HashMap<FieldId, Vector<AnnotId>> = HashMap::new();
    let mut placed = Placed::new();
    for &(index, page) in shown {
        for w in form.widgets.get(&index).into_iter().flatten() {
            let Some(&field) = ids.get(&w.field) else {
                continue;
            };
            let id = lw.annot_id();
            let at = Some(ObjRef {
                src: src.clone(),
                id: w.object,
            });
            let body = AnnotBody::Widget(Arc::new(Widget::read(w, field, at, inst)));
            (lw.rec).put(Key::Annot(id), Some(Val::Annot(AnnotRec { page, body })));
            shows.entry(field).or_default().push_back(id);
            placed.entry(index).or_default().push((w.slot, id));
        }
    }
    let terms: HashMap<FieldId, &crate::forms::Field> =
        form.fields.iter().map(|f| (f.id, f)).collect();
    // The new parent's kids: its file's roots.
    let mut under_top = Vector::new();
    for &n in &coming {
        let node = &form.nodes[n.0 as usize];
        let id = ids[&n];
        let parent = node.parent.map(|p| ids[&p]).or(top);
        // A node with a parent in its file is among that one's kids.
        match parent {
            _ if node.parent.is_some() => {}
            Some(_) => under_top.push_back(id),
            None => doc.roots.push_back(id),
        }
        let term = (terms.get(&n))
            .map(|f| Arc::new(Terminal::read(f, shows.remove(&id).unwrap_or_default())));
        let node = FieldNode {
            parent,
            partial: node.partial.clone(),
            src: Some(ObjRef {
                src: src.clone(),
                id: node.object,
            }),
            inst,
            kids: (node.kids.iter())
                .filter_map(|k| ids.get(k))
                .copied()
                .collect(),
            term,
        };
        put_field(&mut lw.rec, id, node);
    }
    if let Some(top) = top.filter(|_| !under_top.is_empty()) {
        let mut node = FieldNode::clone(lw.rec.field(top).expect("the new parent"));
        node.kids.append(under_top);
        put_field(&mut lw.rec, top, node);
    }
    lw.rec.put(Key::Form, Some(Val::Form(Arc::new(doc))));
    if let Some(under) = under {
        let scripted = renamed_scripted(&lw.rec, &lw.cx.store, inst, &under);
        lw.notices.push(Notice::FormRenamed {
            under,
            file: name.to_string(),
            scripted,
        });
    }
    if signed_in(&lw.rec, inst) {
        lw.notices.push(Notice::SignatureNotValidHere {
            file: name.to_string(),
        });
    }
    (Some(inst), placed)
}

/// Duplicate Pages: each copy's widgets get fields of their own (M10 bug
/// 4), so filling one never fills the other. For each instance of a form
/// the pages show, a new one, named as it is in [`FormDoc::names`], of its
/// fields with widgets on them and the nodes above those that were read
/// from it (not a parent sheaf made), renamed apart by [`take_in`]'s rule;
/// its widgets on the copies have new ids. `copies` gives each page and
/// its copy. Each widget's copy, by the id of the one it's a copy of.
pub(crate) fn copy_forms(
    lw: &mut Lower,
    copies: &[(PageId, PageId, u32)],
) -> HashMap<AnnotId, AnnotId> {
    let mut shown: BTreeMap<FormInst, BTreeSet<FieldId>> = BTreeMap::new();
    for &(page, ..) in copies {
        for (_, w) in lw.rec.widgets_on(page) {
            shown.entry(w.inst).or_default().insert(w.field);
        }
    }
    let mut doc = FormDoc::clone(lw.rec.form());
    let mut renamed = Vec::new();
    let mut copied = HashMap::new();
    for (orig, fields) in shown {
        let inst = FormInst {
            src: orig.src,
            copy: instance_copy(&doc, orig.src, lw.cx.primary.id()),
        };
        let name = doc.names.get(&orig).cloned().unwrap_or_default();
        doc.names.insert(inst, name.clone());
        let mut coming = BTreeSet::new();
        for &f in &fields {
            coming.insert(f);
            let mut at = lw.rec.field(f).and_then(|n| n.parent);
            while let Some(id) = at {
                let Some(node) = lw.rec.field(id).filter(|n| n.src.is_some()) else {
                    break;
                };
                if !coming.insert(id) {
                    break;
                }
                at = node.parent;
            }
        }
        let names: Vec<String> = fields.iter().map(|&f| inst_name(&lw.rec, f)).collect();
        let under = apart(&lw.rec, &name, names.iter().map(String::as_str));
        let top = under.as_deref().map(|under| {
            let id = lw.field_id();
            put_field(&mut lw.rec, id, sheaf_parent(under, inst));
            doc.roots.push_back(id);
            id
        });
        renamed.extend(under.map(|under| (under, inst)));
        let ids: HashMap<FieldId, FieldId> =
            (coming.iter()).map(|&id| (id, lw.field_id())).collect();
        // The copies of its fields that calculate, after the rest.
        let calculates: Vec<FieldId> = (doc.calc_order.iter())
            .filter_map(|f| ids.get(f))
            .copied()
            .collect();
        doc.calc_order.extend(calculates);
        // Its widgets on the copies.
        let mut shows: HashMap<FieldId, Vector<AnnotId>> = HashMap::new();
        for &(page, copy, _) in copies {
            let theirs: Vec<(AnnotId, Arc<Widget>)> = (lw.rec.widgets_on(page))
                .filter(|(_, w)| w.inst == orig)
                .map(|(id, w)| (id, w.clone()))
                .collect();
            for (was, w) in theirs {
                let id = lw.annot_id();
                let field = ids[&w.field];
                let w = Widget {
                    field,
                    inst,
                    ..Widget::clone(&w)
                };
                let widget = AnnotRec {
                    page: copy,
                    body: AnnotBody::Widget(Arc::new(w)),
                };
                lw.rec.put(Key::Annot(id), Some(Val::Annot(widget)));
                shows.entry(field).or_default().push_back(id);
                copied.insert(was, id);
            }
        }
        let mut roots = Vector::new();
        for &old in &coming {
            let was = FieldNode::clone(lw.rec.field(old).expect("a node shown"));
            let id = ids[&old];
            let parent = was.parent.and_then(|p| ids.get(&p).copied());
            if parent.is_none() {
                roots.push_back(id);
            }
            let term = was.term.as_ref().map(|t| {
                let widgets = shows.remove(&id).unwrap_or_default();
                Arc::new(Terminal {
                    widgets,
                    ..Terminal::clone(t)
                })
            });
            let node = FieldNode {
                parent: parent.or(top),
                kids: (was.kids.iter())
                    .filter_map(|k| ids.get(k))
                    .copied()
                    .collect(),
                inst,
                term,
                ..was
            };
            put_field(&mut lw.rec, id, node);
        }
        match top {
            Some(top) => {
                let mut node = FieldNode::clone(lw.rec.field(top).expect("the new parent"));
                node.kids = roots;
                put_field(&mut lw.rec, top, node);
            }
            None => doc.roots.append(roots),
        }
    }
    lw.rec.put(Key::Form, Some(Val::Form(Arc::new(doc))));
    for (under, inst) in renamed {
        let scripted = renamed_scripted(&lw.rec, &lw.cx.store, inst, &under);
        lw.notices.push(Notice::CopiesRenamed { under, scripted });
    }
    copied
}

/// Scripts off, when the form's came in as more than one instance now
/// (M10 proposal 5, M10.5 §3.7): they share one scope, and a copy's
/// scripts name the original's fields. Once off they stay off, across
/// saves too, until undo takes back what brought them. The reader is told
/// when this turns them off.
pub(crate) fn turn_scripts_off(lw: &mut Lower) {
    if lw.rec.form().scripts != ScriptState::On {
        return;
    }
    let scripted = scripted_instances(&lw.rec, &lw.cx.store);
    if scripted.len() > 1 {
        let form = FormDoc {
            scripts: ScriptState::Off {
                instances: scripted.into_iter().collect(),
            },
            ..FormDoc::clone(lw.rec.form())
        };
        lw.rec.put(Key::Form, Some(Val::Form(Arc::new(form))));
        lw.notices.push(Notice::ScriptsOff);
    }
}

/// The form instances in `st` with scripts: a field's own, a widget's, or
/// document scripts in the file it came from.
fn scripted_instances(st: &DocState, store: &Store) -> BTreeSet<FormInst> {
    let mut out: BTreeSet<FormInst> = (st.form().names.keys())
        .filter(|&&inst| document_scripts(store, inst))
        .copied()
        .collect();
    for (_, node, term) in st.terminals() {
        let mut widgets = term.widgets.iter().filter_map(|&w| st.annot(w)?.widget());
        if !out.contains(&node.inst) && (term.has_scripts() || widgets.any(|w| w.has_scripts())) {
            out.insert(node.inst);
        }
    }
    out
}

/// Whether instance `inst`'s file has document scripts.
fn document_scripts(store: &Store, inst: FormInst) -> bool {
    let src = store
        .get(inst.src)
        .and_then(|blob| store.source(&blob).ok());
    src.is_some_and(|src| src.form().facts.document_scripts > 0)
}

/// Whether the fields of `inst` renamed under `under` run scripts: their
/// own, or their file's document scripts. Then their scripts still use the
/// old names.
fn renamed_scripted(st: &DocState, store: &Store, inst: FormInst, under: &str) -> bool {
    let documents = document_scripts(store, inst);
    (st.field_named(under).into_iter())
        .flat_map(|top| field_subtree(st, top))
        .filter_map(|id| st.field(id)?.term.clone())
        .any(|t| documents || t.has_scripts())
}

/// Whether instance `inst`, another file's, has signed fields in `st`:
/// they're kept, and their signatures don't cover this file.
fn signed_in(st: &DocState, inst: FormInst) -> bool {
    st.terminals()
        .any(|(_, node, term)| node.inst == inst && term.value == Some(crate::forms::Value::Signed))
}

/// The copy of `source`'s form an instance brought in next takes, fixed
/// as it's made: one past the last `form` has, or the first. Copy 0 of
/// `primary`'s is its own, what it opened with, whether or not the state
/// holds it: another instance of its form is written as objects of its
/// own.
fn instance_copy(
    form: &FormDoc,
    source: crate::model::SourceId,
    primary: crate::model::SourceId,
) -> u32 {
    (form.names.keys())
        .filter(|inst| inst.src == source)
        .map(|inst| inst.copy + 1)
        .max()
        .unwrap_or(u32::from(source == primary))
}

/// The parent fields named `names` (full names, as their file has them)
/// go under, when any of them, or a parent of one, is a node's full name
/// in `st` already: the stem of `file`, or with `_2`, `_3`… when that's
/// taken. None when nothing clashes.
fn apart<'a>(
    st: &DocState,
    file: &str,
    names: impl IntoIterator<Item = &'a str>,
) -> Option<String> {
    if !field_nodes(names)
        .iter()
        .any(|n| st.field_named(n).is_some())
    {
        return None;
    }
    // A dot would make it two names.
    let stem = Path::new(file)
        .file_stem()
        .map(|s| s.to_string_lossy().replace('.', "_"))
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "form".to_string());
    (1..)
        .map(|n| match n {
            1 => stem.clone(),
            n => format!("{stem}_{n}"),
        })
        .find(|name| st.field_named(name).is_none())
}

/// Puts `node` as field node `id`.
fn put_field(rec: &mut Rec, id: FieldId, node: FieldNode) {
    rec.put(Key::Field(id), Some(Val::Field(Arc::new(node))));
}

/// A parent named `under` that sheaf makes for instance `inst`'s fields,
/// whose names clashed with the document's.
fn sheaf_parent(under: &str, inst: FormInst) -> FieldNode {
    FieldNode {
        parent: None,
        partial: under.into(),
        src: None,
        inst,
        kids: Vector::new(),
        term: None,
    }
}

/// Field `id`'s full name in its instance's form, as its file has it: up
/// to a parent sheaf made for it.
fn inst_name(st: &DocState, id: FieldId) -> String {
    let Some(node) = st.field(id) else {
        return String::new();
    };
    let mut partials = vec![&*node.partial];
    let mut at = node.parent.and_then(|p| st.field(p));
    while let Some(up) = at.filter(|n| n.src.is_some() && partials.len() <= st.field_count()) {
        partials.push(&*up.partial);
        at = up.parent.and_then(|p| st.field(p));
    }
    (partials.iter().rev()).fold(String::new(), |up, p| full_name_of(&up, p))
}

/// Field node `top` and every one under it.
fn field_subtree(st: &DocState, top: FieldId) -> Vec<FieldId> {
    let mut out = Vec::new();
    let mut next = vec![top];
    while let Some(id) = next.pop().filter(|_| out.len() <= st.field_count()) {
        out.push(id);
        next.extend(
            st.field(id)
                .into_iter()
                .flat_map(|n| n.kids.iter().copied()),
        );
    }
    out
}

/// Every node of a field tree with these full names: each name, and the
/// parents in it (what comes before each dot).
fn field_nodes<'a>(names: impl IntoIterator<Item = &'a str>) -> HashSet<&'a str> {
    let mut out = HashSet::new();
    for name in names {
        out.insert(name);
        for (i, _) in name.match_indices('.') {
            out.insert(&name[..i]);
        }
    }
    out
}
