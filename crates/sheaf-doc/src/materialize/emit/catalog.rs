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
//! Reader's usage rights when the save drops them (`DropUr3`). The header
//! gives the version (§11.7), so a Rewrite writes no `/Version`.

use lopdf::{Dictionary, Object};

use super::Cx;
use crate::materialize::copy::Scope;
use crate::materialize::key::ObjKey;
use crate::materialize::record::CATALOG_OWNED;
use crate::model::PageId;

/// The catalog, over the tree's root, and the page labels it names;
/// `order` is the pages'.
pub(crate) fn catalog(cx: &mut Cx, order: &[PageId]) {
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
    let st = &cx.input.target;
    if let Some(ranges) = st.label_ranges().filter(|r| !r.is_empty()) {
        cx.push(
            ObjKey::Labels,
            Object::Dictionary(crate::labels::number_tree(&ranges)),
        );
        out.set("PageLabels", cx.placeholder(ObjKey::Labels));
    }
    if let Some(hf) = st.header_footer() {
        let settings = crate::stamps::to_catalog(hf, order);
        out.set(crate::stamps::CATALOG_KEY, settings);
    }
    cx.push(ObjKey::Catalog, Object::Dictionary(out));
}
