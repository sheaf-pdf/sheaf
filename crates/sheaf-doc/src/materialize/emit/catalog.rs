//! The catalog as a save writes it, by its ownership table (M10.5 §9.1,
//! [`CATALOG_OWNED`]): an owned key is always written from the target
//! state, never taken from a file, so set, save, undo and save gives each
//! back as it was; on a Rewrite every other key is the primary's, which
//! `keep_only` then prunes against the pages kept. And the page labels,
//! from the state's `LabelSpec`s.
//!
//! The outline comes from its emitter ([`super::outline`]), and the form
//! from its ([`super::form`]), which also gives `/Names /JavaScript` when
//! forms from other files bring document scripts, and `/Perms` without
//! Reader's usage rights when the save drops them (`DropUr3`).
//! `/SheafGenerators` names a string of its own
//! ([`ObjKey::Generators`]) saying what the generators are, when there
//! are any and the output keeps sheaf's private data (§15);
//! `/SheafHeaderFooter`, where an older sheaf kept its header's settings,
//! is owned and never written. The header gives the version
//! (§11.7), so a Rewrite writes no `/Version`.
//!
//! An Append's catalog ([`appended`]) holds the keys the state doesn't own
//! as the saved file holds them ([`RootValues`]), so a key a save dropped
//! stays gone, and says `/Version`: the later of the file's and what the
//! section needs.
//!
//! An overlay's ([`overlay`]) is its source's, written over it, with what
//! the overlay changes: its pages' tree, their form, and the layers its
//! view turns off.
//!
//! [`RootValues`]: crate::materialize::record::RootValues

use lopdf::{Dictionary, Object};

use super::PdfVersion;

use super::Cx;
use crate::materialize::copy::Scope;
use crate::materialize::key::ObjKey;
use crate::materialize::record::CATALOG_OWNED;

/// The catalog, over the tree's root, and the page labels it names.
pub(crate) fn catalog(cx: &mut Cx) {
    let primary = cx.primary.clone();
    let source = (primary.pdf().catalog().ok().cloned()).unwrap_or_default();
    let scope = Scope {
        src: &primary,
        copy: 0,
    };
    let scripts = super::form::scripts(cx);
    let mut out = Dictionary::new();
    for (key, value) in source.iter() {
        let value = match key.as_slice() {
            k if CATALOG_OWNED.contains(&k) => continue,
            b"Perms" => match super::form::perms(cx, value) {
                Some(perms) => perms,
                None => continue,
            },
            b"Names" if scripts.is_some() => {
                let names = (primary.pdf().dereference(value).ok())
                    .and_then(|(_, n)| n.as_dict().ok().cloned())
                    .unwrap_or_default();
                let mut out = Dictionary::new();
                for (k, v) in names.iter().filter(|(k, _)| k.as_slice() != b"JavaScript") {
                    let v = cx.sym(v, &scope);
                    out.set(k.clone(), v);
                }
                out.set("JavaScript", scripts.clone().expect("scripts"));
                Object::Dictionary(out)
            }
            _ => cx.sym(value, &scope),
        };
        out.set(key.clone(), value);
    }
    if let Some(scripts) = scripts.filter(|_| !source.has(b"Names")) {
        let mut names = Dictionary::new();
        names.set("JavaScript", scripts);
        out.set("Names", names);
    }
    out.set("Type", Object::Name(b"Catalog".to_vec()));
    out.set("Pages", cx.placeholder(ObjKey::PagesNode(0)));
    if let Some(root) = super::outline::outline(cx) {
        out.set("Outlines", root);
    }
    if let Some(form) = super::form::acroform(cx) {
        out.set("AcroForm", form);
    }
    let st = cx.st;
    if let Some(ranges) = st.label_ranges().filter(|r| !r.is_empty()) {
        cx.push(
            ObjKey::Labels,
            Object::Dictionary(crate::labels::number_tree(&ranges)),
        );
        out.set("PageLabels", cx.placeholder(ObjKey::Labels));
    }
    let json = (!cx.strips())
        .then(|| crate::generate::catalog_json(st, |c| super::content::drawn_as(cx, c)))
        .flatten();
    if let Some(json) = json {
        cx.push(ObjKey::Generators, generators(json));
        out.set(GENERATORS, cx.placeholder(ObjKey::Generators));
    }
    cx.push(ObjKey::Catalog, Object::Dictionary(out));
}

/// The catalog's key for what the document's generators are.
pub(crate) const GENERATORS: &str = "SheafGenerators";

/// `/SheafGenerators`' object: its JSON, a string of its own
/// ([`crate::generate::catalog_json`]).
pub(crate) fn generators(json: Vec<u8>) -> Object {
    Object::String(json, lopdf::StringFormat::Literal)
}

/// What an Append's catalog names that its emission decided: the page
/// tree's root, and the form, the outline and the document scripts when
/// they were emitted (a reference, or None when there's none). One not
/// emitted is the saved file's.
pub(crate) struct Parts {
    pub(crate) root: ObjKey,
    pub(crate) form: Option<Option<Object>>,
    pub(crate) outline: Option<Option<Object>>,
    pub(crate) scripts: Option<Option<Object>>,
}

/// An Append's catalog, over `parts`: the saved file's keys the state
/// doesn't own, `/Perms` without Reader's usage rights when the save drops
/// them and `/Names` with the document scripts when they were emitted; the
/// owned keys from the target state; and `/Version`, the later of the
/// file's and what the section needs.
pub(crate) fn appended(cx: &mut Cx, parts: Parts) {
    let record = cx.saved().expect("an Append");
    let primary = cx.primary.clone();
    let scope = Scope {
        src: &primary,
        copy: 0,
    };
    // Other files' document scripts that went leave the primary's names.
    let restore = matches!(parts.scripts, Some(None));
    let scripts = parts.scripts.flatten();
    let mut out = Dictionary::new();
    for (key, value) in record.roots.catalog.iter() {
        let value = match key.as_slice() {
            b"Perms" => match super::form::perms(cx, value) {
                Some(perms) => perms,
                None => continue,
            },
            b"Names" if restore => continue,
            b"Names" if scripts.is_some() => {
                let names = record.roots.names.clone().unwrap_or_default();
                let mut out = Dictionary::new();
                for (k, v) in names.iter().filter(|(k, _)| k.as_slice() != b"JavaScript") {
                    let v = cx.sym(v, &scope);
                    out.set(k.clone(), v);
                }
                out.set("JavaScript", scripts.clone().expect("scripts"));
                Object::Dictionary(out)
            }
            _ => cx.sym(value, &scope),
        };
        out.set(key.clone(), value);
    }
    if let Some(scripts) = scripts.filter(|_| !record.roots.catalog.has(b"Names")) {
        let mut names = Dictionary::new();
        names.set("JavaScript", scripts);
        out.set("Names", names);
    }
    let names = (primary.pdf().catalog().ok()).and_then(|c| c.get(b"Names").ok());
    if let Some(names) = names.filter(|_| restore) {
        let names = cx.sym(names, &scope);
        out.set("Names", names);
    }
    out.set("Type", Object::Name(b"Catalog".to_vec()));
    out.set("Pages", cx.placeholder(parts.root));
    let st = cx.st;
    let outline = match parts.outline {
        Some(outline) => outline,
        None => (!st.outline_roots().is_empty()).then(|| cx.placeholder(ObjKey::OutlinesRoot)),
    };
    if let Some(root) = outline {
        out.set("Outlines", root);
    }
    let form = match parts.form {
        Some(form) => form,
        None => (record.roots.acroform.is_some()).then(|| cx.placeholder(ObjKey::AcroForm)),
    };
    if let Some(form) = form {
        out.set("AcroForm", form);
    }
    if st.label_ranges().is_some_and(|r| !r.is_empty()) {
        out.set("PageLabels", cx.placeholder(ObjKey::Labels));
    }
    if !st.generators().is_empty() {
        out.set(GENERATORS, cx.placeholder(ObjKey::Generators));
    }
    let version: PdfVersion = cx.version.max(record.pdf_version);
    out.set("Version", Object::Name(version.to_string().into_bytes()));
    cx.push(ObjKey::Catalog, Object::Dictionary(out));
}

/// An overlay's catalog, its source's (whose number it's claimed at), but
/// over the overlay's page tree, its form the fields with a widget on the
/// overlay's pages (none when there are none), and `/OCProperties` showing
/// every layer of the source but those the view turns off, when it turns
/// any off ([`ocg_patch`](crate::layers::ocg_patch)).
pub(crate) fn overlay(cx: &mut Cx) {
    let o = cx.overlay.expect("an overlay");
    let src = cx.primary;
    let pdf = src.pdf();
    let source = (pdf.catalog().ok().cloned()).unwrap_or_default();
    let scope = Scope { src, copy: 0 };
    let layers = (o.inputs.ocg_off.as_ref())
        .and_then(|ocg| ocg.off.get(&src.id()))
        .and_then(|off| crate::layers::ocg_patch(src, off));
    let mut out = Dictionary::new();
    for (key, value) in source.iter() {
        match key.as_slice() {
            b"Pages" | b"AcroForm" => {}
            b"OCProperties" if layers.is_some() => {}
            _ => {
                let value = cx.sym(value, &scope);
                out.set(key.clone(), value);
            }
        }
    }
    out.set("Pages", cx.placeholder(ObjKey::PagesNode(0)));
    if let Some(form) = super::form::acroform(cx) {
        out.set("AcroForm", form);
    }
    if let Some(layers) = layers {
        let layers = cx.sym(&layers, &scope);
        out.set("OCProperties", layers);
    }
    cx.push(ObjKey::Catalog, Object::Dictionary(out));
}
