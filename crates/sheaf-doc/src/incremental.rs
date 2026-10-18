//! Incremental save: append only what changed to the original file, with a
//! new cross-reference section chained to the previous one via `/Prev`.
//! The original bytes are preserved verbatim (what Acrobat does by default,
//! and what auditors like). Only possible when saving a document back to
//! its own primary file.
//!
//! The section is the serializer's ([`serialize::append`]): in the file's
//! cross-reference kind, its positions counted from the file's header, its
//! `/ID[0]` kept, and, when the file is encrypted, each object encrypted
//! with the file's own key, so an encrypted file stays encrypted.

use std::collections::BTreeMap;

use anyhow::{Context, Result, bail};
use lopdf::xref::XrefEntry;
use lopdf::{Dictionary, Object, ObjectId, dictionary};
use sheaf_core::Rotation;

use crate::model::{Snapshot, SourceInfo};
use crate::save::{
    LoadedSource, copy_page_dict, is_identity, link_connectors, strip_sheaf_annots, write_outline,
    write_page_annots,
};
use crate::serialize;
use crate::text_layer;

/// Produce the full new file (original bytes + update section). Refuses
/// to replace a page's OCR text already in the file, which would stay in
/// the earlier revision (M9 bug 14). Whether anything else was removed is
/// `save::save_to_path`'s to check (`Snapshot::removes_content`).
pub fn incremental(snap: &Snapshot) -> Result<Vec<u8>> {
    appended(snap, Mode::Save)
}

/// The update a signed file gets when the reader would rather append than
/// break its signatures (M10 proposal 7): what it takes out (a removed
/// page, a changed form's XFA copy and usage rights, a page's old OCR
/// text) stays in the earlier revision.
pub fn append(snap: &Snapshot) -> Result<Vec<u8>> {
    appended(snap, Mode::Append)
}

/// Just the update section, with the length of the original it appends
/// to. Empty when there's nothing to write. Used for crash recovery.
pub fn incremental_section(snap: &Snapshot) -> Result<(usize, Vec<u8>)> {
    let section = build(snap, Mode::Sidecar)?;
    Ok((primary_info(snap)?.src.bytes().len(), section))
}

/// The primary's bytes with `mode`'s update after them.
fn appended(snap: &Snapshot, mode: Mode) -> Result<Vec<u8>> {
    let section = build(snap, mode)?;
    Ok([&primary_info(snap)?.src.bytes()[..], &section].concat())
}

/// The primary source's registration.
fn primary_info(snap: &Snapshot) -> Result<&SourceInfo> {
    let info = snap
        .source(snap.primary)
        .context("primary source missing")?;
    Ok(info)
}

/// What an update is for, and so what it refuses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// A save, which a full one replaces when it refuses: what an update
    /// can't take out of the file (a page's OCR text replaced, a changed
    /// form's XFA copy and usage rights) or point back at (cross-references
    /// lopdf rebuilt).
    Save,
    /// An append chosen over breaking a file's signatures: what it takes
    /// out stays in the earlier revision. Damaged cross-references still
    /// refuse.
    Append,
    /// A recovery sidecar, which is never the file itself.
    Sidecar,
}

/// The update section: nothing when there's nothing to write.
fn build(snap: &Snapshot, mode: Mode) -> Result<Vec<u8>> {
    let primary_id = snap.primary;
    let info = primary_info(snap)?;
    if snap.header_footer_dirty {
        // Drawing it touches every page's content: a rewrite does that.
        bail!("a new header/footer is written by a full save");
    }
    if mode != Mode::Sidecar && !info.src.base_sound() {
        // lopdf rebuilt a damaged file's cross-references by scanning it:
        // there's no section for /Prev to point at. A rewrite writes new ones.
        bail!("the file's cross-references are damaged; a full save writes new ones");
    }
    let base = serialize::append::Base::of(&info.src)?;
    // Decrypted with the file's password, when it has one.
    let mut pdf = crate::file::load_source(info)?;
    // New objects are numbered past every number the file uses, those of
    // its cross-reference streams too.
    pdf.max_id = pdf.max_id.max(base.size.saturating_sub(1));
    let orig_max_id = pdf.max_id;

    let primary = LoadedSource::from_pdf(&pdf)?;
    let origins = crate::save::origins(snap);
    let mut others: std::collections::HashMap<_, LoadedSource> = std::collections::HashMap::new();
    for (&sid, sinfo) in &snap.sources {
        if sid != primary_id && crate::save::needed(snap, &origins, sid) {
            let other = crate::file::load_source(sinfo)?;
            others.insert(sid, LoadedSource::from_pdf_owned(other)?);
        }
    }

    // Objects to (re)write, by id. New objects are allocated through `pdf`
    // so ids never collide; everything above the original max is new.
    let mut changed: BTreeMap<ObjectId, Object> = BTreeMap::new();
    let catalog_id = base.root;
    let mut catalog = pdf.get_dictionary(catalog_id)?.clone();
    let mut catalog_changed = false;

    let tree_changed = !is_identity(snap, &primary);
    let mut page_obj: std::collections::HashMap<crate::model::PageId, ObjectId> =
        std::collections::HashMap::new();
    let mut copy_maps: crate::forms::merge::CopyMaps = std::collections::HashMap::new();
    // A copy of a primary page's form is copied from the file as it was.
    let mut pristine = None;

    let mut connectors = Vec::new();
    // OCR's font and its isolating `q`, and what the marks draw with and
    // embed, once a save; the original's where it has them.
    let mut cx = crate::annot::WriteCx::new().owning(info.src.clone());
    if tree_changed {
        let pages_id = pdf.new_object_id();
        let mut kids = Vec::with_capacity(snap.state.page_count());
        let mut used = std::collections::HashSet::new();
        // Pages from other files get their ids first, so links among them
        // follow and links to their other pages don't copy those (bug 14),
        // and so does each page showing a copy of a form (M10 bug 4).
        let own_objects = crate::save::reserve_page_objects(
            snap,
            &origins,
            &|s, i| {
                let from = if s == primary_id {
                    &primary
                } else {
                    others.get(&s)?
                };
                from.pages.get(i as usize).copied()
            },
            &mut pdf,
            &mut copy_maps,
        );
        if own_objects
            .keys()
            .any(|p| crate::save::copy_of(snap, &origins, *p).0 == primary_id)
        {
            pristine = Some(pdf.clone());
        }
        for (page_id, def, geom) in snap.state.pages() {
            // A derived copy's page is written as the page it was made
            // from: its object, annotations and form fields, with the
            // copy's content (M10 bug 1).
            let origin = origins.get(&page_id).copied();
            let source = origin.map_or(def.base.source, |(s, _)| s);
            let (src_page, inherited, mut dict) = if let Some((_, index)) = origin {
                let shown = others.get(&def.base.source).context("unknown source")?;
                let id = *shown
                    .pages
                    .get(def.base.index as usize)
                    .context("page index out of range")?;
                let map = copy_maps.entry((def.base.source, 0)).or_default();
                let mut dict =
                    crate::save::copy_derived_page(shown.pdf.as_ref().unwrap(), id, &mut pdf, map)?;
                let own = own_objects.contains_key(&page_id);
                let (src_page, annots) = if source == primary_id {
                    let page = *primary
                        .pages
                        .get(index as usize)
                        .context("page index out of range")?;
                    (page, pdf.get_dictionary(page)?.get(b"Annots").ok().cloned())
                } else {
                    let from = others.get(&source).context("unknown source")?;
                    let page = *from
                        .pages
                        .get(index as usize)
                        .context("page index out of range")?;
                    let from = from.pdf.as_ref().unwrap();
                    let map = copy_maps.entry((source, 0)).or_default();
                    let annots = match from.get_dictionary(page)?.get(b"Annots") {
                        // A copy of a form gets its own, below.
                        Ok(a) if !own => Some(crate::save::remap(from, a, &mut pdf, map)?),
                        _ => None,
                    };
                    (page, annots)
                };
                if let Some(a) = annots {
                    dict.set("Annots", a);
                }
                if source != primary_id {
                    crate::kept::drop_dead_links(&pdf, &mut dict);
                }
                (
                    src_page,
                    shown.inherited.get(&id).cloned().unwrap_or_default(),
                    dict,
                )
            } else if def.base.source == primary_id {
                let id = *primary
                    .pages
                    .get(def.base.index as usize)
                    .context("page index out of range")?;
                (
                    id,
                    primary.inherited.get(&id).cloned().unwrap_or_default(),
                    pdf.get_dictionary(id)?.clone(),
                )
            } else {
                let src = others.get(&def.base.source).context("unknown source")?;
                let id = *src
                    .pages
                    .get(def.base.index as usize)
                    .context("page index out of range")?;
                let map = copy_maps.entry((def.base.source, 0)).or_default();
                let src_pdf = src.pdf.as_ref().unwrap();
                let mut dict = if own_objects.contains_key(&page_id) {
                    crate::save::copy_derived_page(src_pdf, id, &mut pdf, map)?
                } else {
                    copy_page_dict(src_pdf, id, &mut pdf, map)?
                };
                crate::kept::drop_dead_links(&pdf, &mut dict);
                (
                    id,
                    src.inherited.get(&id).cloned().unwrap_or_default(),
                    dict,
                )
            };
            if own_objects.contains_key(&page_id) {
                // A copy of a form: its annotations are its own (M10 bug 4).
                let form = crate::save::copy_of(snap, &origins, page_id);
                let from = if form.0 == primary_id {
                    pristine.as_ref()
                } else {
                    others.get(&form.0).and_then(|o| o.pdf.as_ref())
                }
                .context("a copy's source isn't loaded")?;
                let map = copy_maps.entry(form).or_default();
                match crate::forms::merge::copy_annots(from, src_page, &mut pdf, map)? {
                    Some(a) => dict.set("Annots", a),
                    None => {
                        dict.remove(b"Annots");
                    }
                }
            }
            for (k, v) in &inherited {
                if !dict.has(k) {
                    dict.set(k.clone(), v.clone());
                }
            }
            let mut marks = snap.state.mark_records_on(page_id).peekable();
            match (marks.peek(), snap.legacy().removed_comments.get(&page_id)) {
                (None, None) => strip_sheaf_annots(&pdf, &mut dict),
                (_, removed) => {
                    let index = origin.map_or(def.base.index, |(_, i)| i);
                    let renamed = snap.renamed_on(source, index);
                    let w =
                        write_page_annots(&mut pdf, &mut dict, marks, removed, renamed, &mut cx);
                    connectors.extend(w.connectors);
                    for id in w.touched {
                        changed.insert(id, pdf.get_object(id)?.clone());
                    }
                }
            }
            if let Some(layer) = snap.legacy().text_layers.get(&page_id) {
                if mode == Mode::Save && text_layer::has_layer(&pdf, &dict) {
                    bail!(
                        "a page's OCR text is replaced: a full save takes the old out of the file"
                    );
                }
                text_layer::install(&mut pdf, &mut dict, layer, &mut cx.res);
            }
            // The new tree's root has no /Rotate for it to inherit.
            if geom.rotate == Rotation::R0 {
                dict.remove(b"Rotate");
            } else {
                dict.set("Rotate", geom.rotate.degrees());
            }
            dict.set("Parent", pages_id);
            dict.set("Type", Object::Name(b"Page".to_vec()));
            let first = !own_objects.contains_key(&page_id) && used.insert((source, src_page));
            let id = if let Some(&id) = own_objects.get(&page_id) {
                id
            } else if source == primary_id && first {
                src_page
            } else if source != primary_id && first {
                // Its id was reserved above; a second use is a copy.
                copy_maps[&(source, 0)][&src_page]
            } else {
                pdf.new_object_id()
            };
            changed.insert(id, Object::Dictionary(dict));
            kids.push(Object::Reference(id));
            page_obj.insert(page_id, id);
        }
        changed.insert(
            pages_id,
            Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => kids, "Count" => snap.state.page_count() as i64 }),
        );
        catalog.set("Pages", pages_id);
        catalog.remove(b"PageLabels");
        catalog_changed = true;
    } else {
        for (page_id, def, geom) in snap.state.pages() {
            let id = primary.pages[def.base.index as usize];
            page_obj.insert(page_id, id);
            let layer = snap.legacy().text_layers.get(&page_id);
            let turned = snap.render_delta(page_id) != Rotation::R0;
            if turned || snap.annots_dirty.contains(&page_id) || layer.is_some() {
                let mut dict = pdf.get_dictionary(id)?.clone();
                // The rewritten dict stands alone: pull down what it
                // inherited (a layer inlines Resources, which would
                // otherwise hide the inherited ones).
                if let Some(attrs) = primary.inherited.get(&id) {
                    for (k, v) in attrs {
                        if !dict.has(k) {
                            dict.set(k.clone(), v.clone());
                        }
                    }
                }
                if let Some(layer) = layer {
                    if mode == Mode::Save && text_layer::has_layer(&pdf, &dict) {
                        bail!(
                            "a page's OCR text is replaced: a full save takes the old out of the file"
                        );
                    }
                    text_layer::install(&mut pdf, &mut dict, layer, &mut cx.res);
                }
                if snap.annots_dirty.contains(&page_id) {
                    let mut marks = snap.state.mark_records_on(page_id).peekable();
                    match (marks.peek(), snap.legacy().removed_comments.get(&page_id)) {
                        (None, None) => strip_sheaf_annots(&pdf, &mut dict),
                        (_, removed) => {
                            let w = write_page_annots(
                                &mut pdf,
                                &mut dict,
                                marks,
                                removed,
                                snap.renamed_on(primary_id, def.base.index),
                                &mut cx,
                            );
                            connectors.extend(w.connectors);
                            for id in w.touched {
                                changed.insert(id, pdf.get_object(id)?.clone());
                            }
                        }
                    }
                }
                // A turned page says its rotation, 0 too: it stays under
                // its parents, any of which may have a /Rotate. Any other
                // keeps the one it has, its own or the one pulled down.
                if turned {
                    dict.set("Rotate", geom.rotate.degrees());
                }
                changed.insert(id, Object::Dictionary(dict));
            }
        }
    }

    // Only a page-tree rewrite has changed the catalog so far.
    let reordered = catalog_changed;
    let labels_now = snap.state.label_ranges().filter(|r| !r.is_empty());
    if snap.labels_dirty || (reordered && labels_now.is_some()) {
        match labels_now {
            Some(r) => {
                let id = pdf.add_object(crate::labels::number_tree(&r));
                catalog.set("PageLabels", id);
            }
            None => {
                catalog.remove(b"PageLabels");
            }
        }
        catalog_changed = true;
    }
    if snap.outline_dirty {
        catalog.remove(b"Outlines");
        if !snap.state.outline_roots().is_empty() {
            let outlines_id = write_outline(&mut pdf, &snap.state, &page_obj, info.src.id());
            catalog.set("Outlines", outlines_id);
        }
        catalog_changed = true;
    }
    // Forms from other files, and copies of forms, join the file's.
    let written: Vec<Dictionary> = page_obj
        .values()
        .filter_map(|id| match changed.get(id) {
            Some(Object::Dictionary(d)) => Some(d.clone()),
            _ => pdf.get_dictionary(*id).ok().cloned(),
        })
        .collect();
    let on_pages = crate::forms::merge::annots_on(&pdf, written);
    pdf.objects
        .insert(catalog_id, Object::Dictionary(catalog.clone()));
    let touched = crate::forms::merge::merge(
        &mut pdf,
        snap,
        &|sid| {
            if sid == primary_id {
                pristine.as_ref()
            } else {
                others.get(&sid).and_then(|o| o.pdf.as_ref())
            }
        },
        &mut copy_maps,
        &on_pages,
    )?;
    let mut touched = touched;
    // A changed form's XFA copy and usage rights describe the file as it
    // was: a save takes them out by a full rewrite, so the earlier revision
    // doesn't keep them; a recovery sidecar takes them out here.
    if snap.form_changed {
        let (xfa, rights) = crate::forms::rules::has(&pdf);
        if xfa || rights {
            if mode == Mode::Save {
                bail!("the form changed: a full save takes out its XFA copy and usage rights");
            }
            touched.extend(crate::forms::rules::take_out(&mut pdf, true).0);
        }
    }
    for id in touched {
        if id == catalog_id {
            catalog = pdf.get_dictionary(catalog_id)?.clone();
            catalog_changed = true;
        } else {
            changed.insert(id, pdf.get_object(id)?.clone());
        }
    }
    if catalog_changed {
        changed.insert(catalog_id, Object::Dictionary(catalog));
    }
    link_connectors(&mut pdf, &connectors, snap, &page_obj);
    // What's new and reachable from what's written is written too: objects
    // a copy brought that nothing keeps (a widget of a page left behind)
    // stay out.
    let mut queue: Vec<ObjectId> = changed.keys().copied().collect();
    let mut reached = std::collections::HashSet::new();
    while let Some(id) = queue.pop() {
        let Some(obj) = changed.get(&id).or_else(|| pdf.objects.get(&id)) else {
            continue;
        };
        refs(obj, &mut |r| {
            if r.0 > orig_max_id && reached.insert(r) {
                queue.push(r);
            }
        });
    }
    for id in reached {
        if let Some(obj) = pdf.objects.get(&id) {
            changed.entry(id).or_insert_with(|| obj.clone());
        }
    }

    // Each object as the generation the file's table gives it.
    let objects: BTreeMap<ObjectId, Object> = changed
        .into_iter()
        .map(|(id, obj)| match pdf.reference_table.entries.get(&id.0) {
            Some(XrefEntry::Normal { generation, .. }) => ((id.0, *generation), obj),
            _ => (id, obj),
        })
        .collect();
    let xref_number = pdf.new_object_id().0;
    let crypt = info.src.crypt().map(|crypt| &**crypt);
    serialize::append::section(&base, &objects, &[], xref_number, crypt)
}

/// Each reference in `o`.
fn refs(o: &Object, f: &mut dyn FnMut(ObjectId)) {
    match o {
        Object::Reference(id) => f(*id),
        Object::Array(a) => a.iter().for_each(|o| refs(o, f)),
        Object::Dictionary(d) => d.iter().for_each(|(_, o)| refs(o, f)),
        Object::Stream(s) => s.dict.iter().for_each(|(_, o)| refs(o, f)),
        _ => {}
    }
}
