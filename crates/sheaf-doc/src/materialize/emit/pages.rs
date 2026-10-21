//! A page's dictionary as a save writes it (M10.5 §11.3): its base page's,
//! read lazily, with every key sheaf doesn't model kept, and what the
//! state holds of the page written from the state.
//!
//! - **Boxes and `/Rotate`** are the page's `Geom`, written where they
//!   differ from what the page would inherit where it's placed. A Rewrite
//!   places every page in a new tree whose nodes carry nothing, so all
//!   four inheritable keys are written, `/Rotate` even when it's 0: the
//!   boxes as the file had them while the state keeps them, else the
//!   state's. An Append places it in the saved file's tree, whose nodes
//!   may hold any of them, so a page with no `/CropBox` of its own writes
//!   its media box as one, and one whose turn the state keeps keeps its
//!   own `/Rotate` as it says it. An overlay places it under a new root
//!   that carries nothing, as a Rewrite does, but writes its base page's
//!   own `/Rotate` ([`RotatePolicy::KeepBase`]): the render turns it by
//!   the rest.
//! - **`/StructParents`** stays only on copy 0 of a primary page, whose
//!   structure tree is written; a copy and a page from another file lose
//!   it, with a note when their file was tagged.
//! - **`/Thumb`** goes when sheaf draws over the page.
//! - **References** are copy 0 of what they name, shared by every copy of
//!   the page. `/Annots` is the page's records' ([`super::annots`]): a
//!   copy's annotations are its own.
//! - **Copy 0 of a primary page owns the page's object** (bug 3): numbering
//!   writes it at its own number, wherever the page moved (claimed before
//!   anything is emitted, `emit::claim`).
//! - **A derived copy's page** (a crop, a header stripped, a watermark: its
//!   [`Derived`](crate::Derived) record) is written as the page it was made
//!   from ([`Shown`]): that page's object, keys and annotations, with the
//!   copy's `/Contents` and `/Resources` copied in (M10 bug 1), until slice
//!   17's content stack.
//! - **What sheaf draws** over the page ([`super::content`]) goes into its
//!   `/Contents` and `/Resources` last.

use anyhow::{Context as _, Result};
use lopdf::{Dictionary, LazyPdf, Object};

use super::{Cx, RotatePolicy, Shown};
use crate::geom::{Geom, Rect};
use crate::materialize::copy::Scope;
use crate::materialize::key::ObjKey;
use crate::model::{PageDef, PageId};
use crate::source::PageFacts;

/// Page `id`, which shows `def` with `geom`, emitted under the tree node
/// `parent`; `pos` is its place in the document.
pub(crate) fn page(
    cx: &mut Cx,
    id: PageId,
    def: &PageDef,
    geom: &Geom,
    pos: usize,
    parent: ObjKey,
) -> Result<()> {
    let Shown {
        src,
        index,
        copy,
        content,
    } = cx.shown[&id].clone();
    let facts = (src.page(index)).with_context(|| format!("its file has no page {index}"))?;
    let pdf = src.pdf();
    let none = Dictionary::new();
    let base = match pdf.get(facts.id) {
        Ok(Object::Dictionary(d)) => d,
        Ok(Object::Stream(s)) => &s.dict,
        _ => &none,
    };
    let primary = src.id() == cx.primary.id();
    let shared = Scope { src: &src, copy: 0 };
    // What the page draws: a derived copy's page's, or its own.
    let drawn = match &content {
        Some((copy_src, at)) => {
            let facts =
                (copy_src.page(*at)).with_context(|| format!("a derived copy has no page {at}"))?;
            Some((copy_src, facts))
        }
        None => None,
    };
    let was = &facts.geom;
    let drawn_over = !def.content.over.is_empty();
    let mut out = Dictionary::new();
    out.set("Type", Object::Name(b"Page".to_vec()));
    out.set("Parent", cx.placeholder(parent));
    let mut untagged = false;
    for (key, value) in base.iter() {
        let keep = match key.as_slice() {
            b"Type" | b"Parent" => false,
            // The records' ([`super::annots`]).
            b"Annots" => false,
            // Written below, from the page's effective values.
            b"Resources" | b"MediaBox" | b"CropBox" | b"Rotate" => false,
            // A derived copy's page draws the copy's.
            b"Contents" => drawn.is_none(),
            b"StructParents" => {
                let keep = primary && copy == 0;
                untagged |= !keep;
                keep
            }
            b"Thumb" => !drawn_over && drawn.is_none(),
            b"TrimBox" => geom.trim == was.trim,
            b"BleedBox" => geom.bleed == was.bleed,
            b"ArtBox" => geom.art == was.art,
            b"UserUnit" => geom.user_unit == was.user_unit,
            _ => true,
        };
        if keep {
            let value = cx.sym(value, &shared);
            out.set(key.clone(), value);
        }
    }
    let shown = cx.shown[&id].clone();
    if let Some(annots) = super::annots::page(cx, id, &shown, primary && copy == 0) {
        out.set("Annots", annots);
    }
    let resources = match drawn {
        Some((copy_src, facts)) => {
            let from = Scope {
                src: copy_src,
                copy: 0,
            };
            let dict = (copy_src.pdf().get_dictionary(facts.id)).ok();
            if let Some(contents) = dict.and_then(|d| d.get(b"Contents").ok()) {
                let contents = cx.sym(contents, &from);
                out.set("Contents", contents);
            }
            facts.resources.as_deref().map(|r| cx.sym(r, &from))
        }
        None => facts.resources.as_deref().map(|r| cx.sym(r, &shared)),
    };
    // Required, and inherited from nowhere when it's None.
    let resources = resources.unwrap_or_else(|| Object::Dictionary(Dictionary::new()));
    out.set("Resources", resources);
    let boxes_kept = geom.media == was.media && geom.crop == was.crop;
    let as_read = |key: &[u8]| boxes_kept.then(|| effective(pdf, base, facts, key))?;
    match as_read(b"MediaBox") {
        Some(media) => out.set("MediaBox", cx.sym(&media, &shared)),
        None => out.set("MediaBox", rect(geom.media)),
    }
    if boxes_kept {
        if let Some(crop) = as_read(b"CropBox") {
            out.set("CropBox", cx.sym(&crop, &shared));
        }
    } else if geom.crop != geom.media {
        out.set("CropBox", rect(geom.crop));
    }
    // An Append places the page in the saved file's tree, whose nodes may
    // crop what's under them: a page with no crop of its own says so.
    if cx.saved().is_some()
        && !out.has(b"CropBox")
        && let Ok(media) = out.get(b"MediaBox").cloned()
    {
        out.set("CropBox", media);
    }
    // An Append keeps a turn the state left as the page's own `/Rotate`
    // says it, however it says it (a 135 reads as 90); an overlay keeps the
    // base page's, whatever the state says.
    let own = match cx.rotate() {
        RotatePolicy::Model => (base.get(b"Rotate").ok().cloned())
            .filter(|_| cx.saved().is_some() && geom.rotate == was.rotate),
        RotatePolicy::KeepBase => {
            Some(effective(pdf, base, facts, b"Rotate").unwrap_or(Object::Integer(0)))
        }
    };
    match own {
        Some(rotate) => out.set("Rotate", cx.sym(&rotate, &shared)),
        None => out.set("Rotate", i64::from(geom.rotate.degrees())),
    }
    let others = [
        (b"TrimBox".as_slice(), geom.trim, was.trim),
        (b"BleedBox", geom.bleed, was.bleed),
        (b"ArtBox", geom.art, was.art),
    ];
    for (key, now, then) in others {
        if let Some(r) = now.filter(|_| now != then) {
            out.set(key, rect(r));
        }
    }
    if geom.user_unit != was.user_unit && geom.user_unit != 1.0 {
        out.set("UserUnit", geom.user_unit);
    }

    super::content::page(cx, id, def, geom, pos, &mut out);
    if untagged {
        let name = cx.file_name(id);
        cx.untagged(&src, &name);
    }
    cx.push(ObjKey::Page(id), Object::Dictionary(out));
    Ok(())
}

/// Page `facts`' value of the inheritable `key`, its `base` dictionary's
/// or else the nearest on its `/Parent` chain's, references followed.
fn effective(pdf: &LazyPdf, base: &Dictionary, facts: &PageFacts, key: &[u8]) -> Option<Object> {
    let value = base.get(key).ok().or_else(|| {
        (facts.parents()).find_map(|node| pdf.get_dictionary(node).ok()?.get(key).ok())
    })?;
    Some(pdf.dereference(value).ok()?.1.clone())
}

/// A box as written.
fn rect(r: Rect) -> Object {
    Object::Array([r.x0, r.y0, r.x1, r.y1].map(Object::from).to_vec())
}
