//! [`super::annots_in`] with each annotation read through `pdf-annots`
//! (M18 slice 24): sheaf's walk of each page's `/Annots`, its marks and
//! their companions found first on the raw entry as now, and every other
//! dictionary read once by the family ([`crate::import::family::read`]),
//! listed and converted from what it read. Set beside the walk by the
//! annotations dump (`annot::corpus`, `SHEAF_ANNOTS_DUMP_FAMILY`) until
//! the open reads through it.

use std::collections::HashMap;

use sheaf_edit::Rotation;

use super::{
    AnnotsInFile, ForeignAnnot, Found, InFile, held, is_listed_subtype, is_ours, pdf_text,
    sheaf_mark, thread_name,
};
use crate::annot::{Box2, Pt};
use crate::attachments::Objects;
use crate::import::family::{self as read, rgb};
use crate::pdf::annots::{AnnotFlags, Error, TypedAnnot};
use crate::pdf::{Dictionary, ObjectId, Value};

/// What the walk found, and each entry the family refused to read, by its
/// page's index and its slot, with why.
#[derive(Debug, Default)]
pub(crate) struct Walked {
    pub(crate) found: AnnotsInFile,
    pub(crate) refused: HashMap<(u32, usize), Error>,
}

/// [`super::annots_in`], each other tool's annotation read by the family.
pub(crate) fn annots_in<P: Objects + ?Sized>(
    pdf: &P,
    pages: impl IntoIterator<Item = (ObjectId, Rotation)>,
) -> Walked {
    let mut out = Walked::default();
    for (i, (page_id, turn)) in pages.into_iter().enumerate() {
        let Ok(page) = pdf.get_dictionary(page_id) else {
            continue;
        };
        let Some(annots) = page
            .get(b"Annots")
            .and_then(|a| pdf.dereference(a).ok().map(|(_, o)| o))
        else {
            continue;
        };
        let Some(list) = annots.as_array() else {
            continue;
        };
        if list.is_empty() {
            continue;
        }
        let mut found = Vec::new();
        // Other tools', each by its place in `found`, with what was read
        // of them, to convert together.
        let mut dicts = Vec::new();
        for (slot, entry) in list.iter().enumerate() {
            let Ok((object, obj)) = pdf.dereference(entry) else {
                continue;
            };
            let Value::Dict(dict) = obj.value() else {
                continue;
            };
            if let Some(annot) = sheaf_mark(pdf, entry) {
                let held = held(pdf, dict, &annot);
                found.push(InFile {
                    object,
                    slot,
                    found: Found::Mark { annot, held },
                });
                continue;
            }
            if is_ours(pdf, entry) {
                continue;
            }
            let a = match read::read(pdf, dict, object) {
                Ok(a) => a,
                Err(why) => {
                    out.refused.insert((i as u32, slot), why);
                    continue;
                }
            };
            let Some(foreign) = foreign(pdf, dict, &a, slot) else {
                continue;
            };
            // A link is no mark sheaf can hold.
            if !foreign.is_link() {
                dicts.push((found.len(), dict, a));
            }
            found.push(InFile {
                object,
                slot,
                found: Found::Foreign(foreign),
            });
        }
        if !dicts.is_empty() {
            let pairs: Vec<(&Dictionary, &read::ReadAnnot<'_>)> =
                dicts.iter().map(|(_, d, a)| (*d, a)).collect();
            let marks = read::convert_page(pdf, &pairs, turn, Default::default());
            for ((at, _, a), m) in dicts.iter().zip(marks) {
                if let Found::Foreign(f) = &mut found[*at].found {
                    f.mark = m.ok().map(Box::new);
                    f.stamp = (f.mark.as_deref())
                        .filter(|m| matches!(m.kind, crate::annot::AnnotKind::PageStamp { .. }))
                        .and_then(|_| read::normal_form(a));
                }
            }
        }
        if !found.is_empty() {
            out.found.pages.insert(i as u32, found);
        }
    }
    out
}

/// What the open lists of another tool's annotation `dict`, `a` as the
/// family read it, in slot `slot`: None for what isn't listed (a form
/// field, a popup, what has no `/Rect` of four numbers).
pub(crate) fn foreign<P: Objects + ?Sized>(
    pdf: &P,
    dict: &Dictionary,
    a: &read::ReadAnnot<'_>,
    slot: usize,
) -> Option<ForeignAnnot> {
    let subtype = read::subtype_of(a);
    if subtype.is_empty() || !is_listed_subtype(&subtype) || !a.has_valid_rect() {
        return None;
    }
    let [x0, y0, x1, y1] = a.raw_rect();
    let text = |key: &[u8]| dict.get(key).map(pdf_text).unwrap_or_default();
    let name_of = |key: &[u8]| -> Option<String> {
        let (_, o) = pdf.dereference(dict.get(key)?).ok()?;
        let Value::Dict(d) = o.value() else {
            return None;
        };
        Some(thread_name(d))
    };
    // A note's state is the family's; another subtype's, as written.
    let (model, state) = match a.typed() {
        TypedAnnot::Text(t) => (
            t.state_model.clone().unwrap_or_default(),
            t.state.clone().unwrap_or_default(),
        ),
        _ => (text(b"StateModel"), text(b"State")),
    };
    let state = match (model, state) {
        (_, s) if s.is_empty() => None,
        // Acrobat's default model.
        (m, s) if m.is_empty() => Some((crate::annot::REVIEW.to_string(), s)),
        (m, s) => Some((m, s)),
    };
    let markup = a.markup();
    Some(ForeignAnnot {
        subtype,
        // What isn't a markup annotation has no author in the family's
        // view: its `/T` as PDFium reads any annotation's.
        author: (markup.and_then(|m| m.author.clone()))
            .or_else(|| {
                let (_, t) = pdf.dereference(dict.get(b"T")?).ok()?;
                Some(super::text_string(t.as_bytes()?))
            })
            .unwrap_or_default(),
        contents: a.contents().unwrap_or_default().to_string(),
        modified: text(b"M"),
        rect: Box2::from_corners(Pt { x: x0, y: y0 }, Pt { x: x1, y: y1 }),
        name: thread_name(dict),
        parent: name_of(b"IRT"),
        grouped: read::is_grouped(a),
        state,
        created: text(b"CreationDate"),
        slot,
        hidden: a.flags().contains(AnnotFlags::HIDDEN),
        color: a.color().and_then(rgb),
        mark: None,
        stamp: None,
    })
}
