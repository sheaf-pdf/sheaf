//! What a save takes out of a form because it described the file as it was
//! (M10 proposals 6 and 7): the XFA copy, once the AcroForm changed, and
//! Reader's usage rights, whose signature covers the original bytes.
//!
//! [`plan`] decides it before anything is written (M10.5 §11.8), from the
//! primary's form as read and the state being saved: policy, never an
//! edit, which the emitter applies to the form's dictionary and the
//! catalog as it writes them.
//!
//! A Rewrite always takes the usage rights out, and `/SigFlags` with them
//! when nothing signed needs it; a certification stays. When the form
//! changed, the XFA copy goes too, its tooltips copied into `/TU` first:
//! Acrobat would show the XFA's fields and values, not the ones sheaf
//! wrote. A recovery sidecar applies the same transforms.

use std::collections::HashMap;

use lopdf::ObjectId;

use super::field::{FieldKind, Value};
use super::read::FileForm;
use crate::materialize::{SaveRecord, SaveTransform, Strategy};
use crate::model::{DocState, FormInst, SourceId};

/// What a save took out, for the reader.
pub const XFA_REMOVED: &str =
    "Removed the form's XFA copy, so Acrobat shows the form as it is here";
pub const RIGHTS_REMOVED: &str =
    "Removed Reader's usage rights: their signature covers the file as it was";

/// What save policy does to the form of a save of `target` by `strategy`
/// over `base`, the form of registration `primary` as read, after the save
/// `rec` recorded, when there's one:
/// - **`DropXfa`** when the XFA copy describes fields the target no longer
///   has as they were: the field tree, a name, a value or a widget differs
///   from `base`'s, or fields came in from elsewhere. The fields' XFA
///   tooltips go into their `/TU`.
/// - **`DropUr3`** when there are usage rights: on a Rewrite always, since
///   their signature covers the bytes as they were. Slice 10 keeps them on
///   an Append their `/TransformParams` allow.
/// - **`FixSigFlags`** beside it, when `/SigFlags` says there are
///   signatures and neither a signed field of the target nor a
///   certification needs it.
///
/// An Append (slice 10) never repeats what `rec` says the file lost
/// already.
pub fn plan(
    base: &FileForm,
    target: &DocState,
    primary: SourceId,
    rec: Option<&SaveRecord>,
    strategy: &Strategy,
) -> Vec<SaveTransform> {
    let facts = &base.facts;
    let mut out = Vec::new();
    if facts.xfa && form_differs(base, target, primary) {
        out.push(SaveTransform::DropXfa {
            tooltips_to_tu: true,
        });
    }
    if facts.usage_rights {
        out.push(SaveTransform::DropUr3);
        let signed = (target.terminals())
            .any(|(_, _, t)| t.kind == FieldKind::Signature && t.value == Some(Value::Signed));
        if facts.sig_flags != 0 && !facts.certified && !signed {
            out.push(SaveTransform::FixSigFlags);
        }
    }
    if let (Strategy::Append { .. }, Some(rec)) = (strategy, rec) {
        out.retain(|t| !rec.transforms.contains(t));
    }
    out
}

/// Whether `target`'s fields differ from `base`'s, registration
/// `primary`'s form as read: a node of another instance, a node gone, or a
/// node with another name, value or number of widgets.
fn form_differs(base: &FileForm, target: &DocState, primary: SourceId) -> bool {
    let own = FormInst {
        src: primary,
        copy: 0,
    };
    if target.fields().any(|(_, node)| node.inst != own) {
        return true;
    }
    if target.field_count() != base.nodes.len() {
        return true;
    }
    let nodes: HashMap<ObjectId, &str> = (base.nodes.iter())
        .map(|n| (n.object, &*n.partial))
        .collect();
    let fields: HashMap<ObjectId, &super::Field> =
        base.fields.iter().map(|f| (f.object, f)).collect();
    let mut shown: HashMap<super::FieldId, usize> = HashMap::new();
    for w in base.widgets.values().flatten() {
        *shown.entry(w.field).or_default() += 1;
    }
    target.fields().any(|(_, node)| {
        let Some(r) = &node.src else {
            return true;
        };
        if nodes.get(&r.id) != Some(&&*node.partial) {
            return true;
        }
        match (&node.term, fields.get(&r.id)) {
            (None, None) => false,
            (Some(term), Some(field)) => {
                term.value != field.value
                    || term.widgets.len() != shown.get(&field.id).copied().unwrap_or(0)
            }
            _ => true,
        }
    })
}
