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
//! when nothing signed needs it; a certification stays. An Append keeps
//! them when what it changes is within their `/TransformParams` (M10.5
//! proposal 34): Reader checks later revisions against those. When the
//! form changed, the XFA copy goes too, its tooltips copied into `/TU`
//! first: Acrobat would show the XFA's fields and values, not the ones
//! sheaf wrote. A recovery sidecar applies the same transforms.

use std::collections::HashMap;

use lopdf::ObjectId;

use super::field::{FieldKind, Value};
use super::read::FileForm;
use crate::effects::{AnnotOps, SigClass};
use crate::materialize::{SaveRecord, SaveTransform, Strategy};
use crate::model::{DocState, FormInst, SourceId};
use crate::source::Source;

/// What a save took out, for the reader.
pub const XFA_REMOVED: &str =
    "Removed the form's XFA copy, so Acrobat shows the form as it is here";
pub const RIGHTS_REMOVED: &str =
    "Removed Reader's usage rights: their signature covers the file as it was";

/// What save policy does to the form of a save of `target` by `strategy`
/// over `src`, the file of registration `primary` as read, after the save
/// `rec` recorded, when there's one. `class` and `ops` are what an Append
/// changes: its classes of change (§9.3), but for its transforms', and
/// what it does to annotations.
/// - **`DropXfa`** when the XFA copy describes fields the target no longer
///   has as they were: the field tree, a name, a value or a widget differs
///   from the form as read, or fields came in from elsewhere. The fields'
///   XFA tooltips go into their `/TU`.
/// - **`DropUr3`** when there are usage rights: on a Rewrite always, since
///   their signature covers the bytes as they were; on an Append unless
///   their `/TransformParams` allow what it changes, the form's dictionary
///   that dropping the XFA copy writes counting as anything else.
/// - **`FixSigFlags`** beside it, when `/SigFlags` says there are
///   signatures and neither a signed field of the target nor a
///   certification needs it.
///
/// An Append never repeats what `rec` says the file lost already.
pub fn plan(
    src: &Source,
    target: &DocState,
    primary: SourceId,
    rec: Option<&SaveRecord>,
    strategy: &Strategy,
    class: SigClass,
    ops: AnnotOps,
) -> Vec<SaveTransform> {
    let facts = &src.form().facts;
    let append = matches!(strategy, Strategy::Append { .. });
    let done = |t: &SaveTransform| append && rec.is_some_and(|rec| rec.transforms.contains(t));
    let mut out = Vec::new();
    if drops_xfa(src, target, primary) {
        out.push(SaveTransform::DropXfa {
            tooltips_to_tu: true,
        });
    }
    if facts.usage_rights {
        let class = match out.iter().any(|t| !done(t)) {
            true => class | SigClass::OTHER,
            false => class,
        };
        let rights = src.signatures().usage_rights;
        if !(append && rights.is_some_and(|r| r.allow(class, ops))) {
            out.push(SaveTransform::DropUr3);
            let signed = (target.terminals())
                .any(|(_, _, t)| t.kind == FieldKind::Signature && t.value == Some(Value::Signed));
            if facts.sig_flags != 0 && !facts.certified && !signed {
                out.push(SaveTransform::FixSigFlags);
            }
        }
    }
    out.retain(|t| !done(t));
    out
}

/// Whether save policy drops the XFA copy of `src`, the file of
/// registration `primary` as read, from a save of `target` ([`plan`]'s
/// `DropXfa`): it has one, and the fields differ from its.
pub(crate) fn drops_xfa(src: &Source, target: &DocState, primary: SourceId) -> bool {
    let base = src.form();
    base.facts.xfa && form_differs(base, target, primary)
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
