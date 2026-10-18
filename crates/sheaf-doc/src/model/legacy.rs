//! The legacy projection (M10.5 slice 8; slice 9 deletes it with the old
//! writers): what `save.rs` and `incremental.rs` read, in the shape they
//! read it before the records, built from a snapshot's records the first
//! time an old writer asks ([`Snapshot::legacy`](super::Snapshot::legacy)),
//! never when a snapshot is taken.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use super::{DocState, PageId, Snapshot};
use crate::forms::{Field, FieldId};
use crate::stamps::HeaderFooter;
use crate::text_layer::TextLayer;

pub(crate) struct LegacyDocState {
    /// Recognized text no file holds yet, by page.
    pub(crate) text_layers: HashMap<PageId, Arc<TextLayer>>,
    /// The header/footer: the legacy generator's.
    pub(crate) header_footer: Option<Arc<HeaderFooter>>,
    /// Other tools' comments and links a page's file holds that the page
    /// hasn't any more, by name, which a save leaves out with their
    /// threads. The page's file is its base's, or for a derived copy's
    /// page the page it was made from ([`origin`](crate::save::origin)):
    /// what a save writes. One a mark has the name of isn't among them:
    /// the mark takes its place.
    pub(crate) removed_comments: HashMap<PageId, im::OrdSet<String>>,
    /// The terminal fields, each written as its instance's copy of the
    /// field it was read as (see [`legacy_field`]). Which instance a page's
    /// widgets are in, the old writers read from the widgets themselves.
    pub(crate) fields: BTreeMap<FieldId, Field>,
}

impl LegacyDocState {
    pub(crate) fn of(snap: &Snapshot) -> LegacyDocState {
        let st = &snap.state;
        let text_layers = st
            .pages()
            .filter_map(|(id, def, _)| Some((id, def.content.ocr_text()?.clone())))
            .collect();
        let removed_comments = st
            .pages()
            .filter_map(|(id, def, _)| {
                let (source, index) = crate::save::origin(snap, def.base.source, def.base.index)
                    .unwrap_or((def.base.source, def.base.index));
                let annots = snap.source(source)?.src.annots();
                let mut names: im::OrdSet<String> = (annots.foreign_on(index))
                    .map(|(_, _, f)| f.name.clone())
                    .collect();
                for (_, a) in st.annots_on(id) {
                    names.remove(&a.name());
                }
                (!names.is_empty()).then_some((id, names))
            })
            .collect();
        LegacyDocState {
            text_layers,
            header_footer: st.header_footer().cloned(),
            removed_comments,
            fields: (st.terminals())
                .filter_map(|(id, _, _)| Some((id, legacy_field(st, id)?)))
                .collect(),
        }
    }
}

/// Terminal field `id` of `st` as the old writers read it: its instance's
/// registration and copy as its `source` and `copy`, the field it was read
/// as as its `object`, the parent sheaf made for its instance, if any, as
/// its `under`, and its full name. None for a node that isn't a terminal,
/// or one read from no object.
pub(crate) fn legacy_field(st: &DocState, id: FieldId) -> Option<Field> {
    let node = st.field(id)?;
    let term = node.term.as_ref()?;
    let object = node.src.as_ref()?.id;
    // The top of its tree, short of a loop.
    let mut top = &**node;
    for _ in 0..st.field_count() {
        match top.parent.and_then(|p| st.field(p)) {
            Some(up) => top = up,
            None => break,
        }
    }
    let under = (top.src.is_none()).then(|| top.partial.clone());
    let props = &term.props;
    Some(Field {
        id,
        source: node.inst.src,
        object,
        name: st.full_name(id)?,
        kind: term.kind,
        flags: term.flags,
        value: term.value.clone(),
        default: term.default.clone(),
        da: props.da.clone(),
        align: props.align,
        max_len: props.max_len,
        options: props.options.clone(),
        tooltip: props.tooltip.clone(),
        actions: props.actions.clone(),
        copy: node.inst.copy,
        under,
        outside: term.outside,
    })
}
