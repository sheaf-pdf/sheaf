//! Optional content (layers): the groups in `/OCProperties`, listed in
//! the default configuration's `/Order`, and the catalog a view that
//! changes which are shown draws the file with ([`ocg_patch`]). Visibility
//! changes are never saved.

use std::collections::{BTreeSet, HashSet};

use lopdf::{Dictionary, Object, ObjectId};

use crate::attachments::{Objects, once, resolve};
use crate::file::pdf_text;
use crate::source::Source;

/// An optional content group of a source: sheaf's name for a layer. It is
/// the group's object in its file, which only this module reads, so the
/// view and its inputs never handle a file's object numbers (M10.5 §2,
/// rule 7). A source's layers are told apart by it; which source they're
/// of, the [`OcgOverride`](crate::OcgOverride) says.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LayerId(ObjectId);

impl LayerId {
    /// The layer of the group `id`, as a test names it.
    #[cfg(any(test, feature = "fixtures"))]
    pub fn new(id: ObjectId) -> LayerId {
        LayerId(id)
    }

    /// What a digest of the layers a view turns off takes of it.
    pub(crate) fn to_le_bytes(self) -> [u8; 6] {
        let (n, g) = self.0;
        let mut out = [0; 6];
        out[..4].copy_from_slice(&n.to_le_bytes());
        out[4..].copy_from_slice(&g.to_le_bytes());
        out
    }
}

/// One row of the layers list: a group, or a heading in `/Order`.
#[derive(Debug, Clone, PartialEq)]
pub struct Layer {
    /// The optional content group; None for a heading.
    pub id: Option<LayerId>,
    pub name: String,
    /// Shown in the default configuration.
    pub on: bool,
    /// Listed in `/Locked`: the user may not change it.
    pub locked: bool,
    pub depth: u16,
}

/// The file's layers in display order: `/D /Order` if it has one (nested
/// arrays give depth; a string first in an array is a heading), then any
/// group the order leaves out. Empty when the file has no layers.
pub fn layers_in<P: Objects + ?Sized>(pdf: &P) -> Vec<Layer> {
    let Some(props) = oc_properties(pdf) else {
        return Vec::new();
    };
    let groups: Vec<ObjectId> = props
        .get(b"OCGs")
        .ok()
        .and_then(|o| resolve(pdf, o))
        .and_then(|o| o.as_array().ok())
        .map(|a| a.iter().filter_map(|o| o.as_reference().ok()).collect())
        .unwrap_or_default();
    let d = props
        .get(b"D")
        .ok()
        .and_then(|o| resolve(pdf, o))
        .and_then(|o| o.as_dict().ok());
    let ids = |key: &[u8]| -> HashSet<ObjectId> {
        d.and_then(|d| d.get(key).ok())
            .and_then(|o| resolve(pdf, o))
            .and_then(|o| o.as_array().ok())
            .map(|a| a.iter().filter_map(|o| o.as_reference().ok()).collect())
            .unwrap_or_default()
    };
    let base_off = d
        .and_then(|d| d.get(b"BaseState").ok())
        .and_then(|o| o.as_name().ok())
        == Some(b"OFF");
    let (on, off, locked) = (ids(b"ON"), ids(b"OFF"), ids(b"Locked"));
    let visible = |id: &ObjectId| {
        if base_off {
            on.contains(id)
        } else {
            !off.contains(id)
        }
    };
    let name = |id: ObjectId| {
        pdf.get_dictionary(id)
            .ok()
            .and_then(|g| g.get(b"Name").ok())
            .and_then(|o| resolve(pdf, o))
            .map(pdf_text)
            .unwrap_or_default()
    };

    let mut out = Vec::new();
    let mut listed = HashSet::new();
    let mut seen = HashSet::new();
    if let Some(order) = d
        .and_then(|d| d.get(b"Order").ok())
        .and_then(|o| once(pdf, &mut seen, o))
        .and_then(|o| o.as_array().ok())
    {
        walk_order(pdf, order, 0, &mut seen, &mut |id, label, depth| match id {
            Some(id) if groups.contains(&id) || pdf.get_dictionary(id).is_ok() => {
                if listed.insert(id) {
                    out.push(Layer {
                        id: Some(LayerId(id)),
                        name: name(id),
                        on: visible(&id),
                        locked: locked.contains(&id),
                        depth,
                    });
                }
            }
            Some(_) => {}
            None => out.push(Layer {
                id: None,
                name: label,
                on: true,
                locked: true,
                depth,
            }),
        });
    }
    for id in groups {
        if listed.insert(id) {
            out.push(Layer {
                id: Some(LayerId(id)),
                name: name(id),
                on: visible(&id),
                locked: locked.contains(&id),
                depth: 0,
            });
        }
    }
    out
}

/// `/Order`'s items below `depth`, each array reached by reference once
/// (an order may hold itself).
fn walk_order<P: Objects + ?Sized>(
    pdf: &P,
    items: &[Object],
    depth: u16,
    seen: &mut HashSet<ObjectId>,
    f: &mut impl FnMut(Option<ObjectId>, String, u16),
) {
    if depth > 32 {
        return;
    }
    let mut it = items.iter().peekable();
    // A label first in a nested array heads it.
    if depth > 0
        && let Some(o) = it.peek()
        && let Some(s) = resolve(pdf, o).filter(|o| o.as_str().is_ok())
    {
        f(None, pdf_text(s), depth - 1);
        it.next();
    }
    for o in it {
        match o {
            Object::Reference(id) => match pdf.get_object(*id) {
                Ok(Object::Array(a)) => {
                    if seen.insert(*id) {
                        walk_order(pdf, a, depth + 1, seen, f);
                    }
                }
                _ => f(Some(*id), String::new(), depth),
            },
            Object::Array(a) => walk_order(pdf, a, depth + 1, seen, f),
            _ => {}
        }
    }
}

fn oc_properties<P: Objects + ?Sized>(pdf: &P) -> Option<&Dictionary> {
    pdf.catalog()
        .ok()?
        .get(b"OCProperties")
        .ok()
        .and_then(|o| resolve(pdf, o))?
        .as_dict()
        .ok()
}

/// `src`'s `/OCProperties` with its default configuration showing every
/// group but `off`, whatever the file's hides: what a view of the file
/// that hides those layers says in place of the file's, which never
/// changes (an overlay's catalog, or a source's catalog-only patch, M10.5
/// §10.1). A direct dictionary, its references the source's own numbers.
/// None when the file has no layers.
pub fn ocg_patch(src: &Source, off: &BTreeSet<LayerId>) -> Option<Object> {
    let pdf = src.pdf();
    let props = oc_properties(pdf)?;
    let groups: Vec<ObjectId> = (props.get(b"OCGs").ok())
        .and_then(|o| resolve(pdf, o))
        .and_then(|o| o.as_array().ok())
        .map(|a| a.iter().filter_map(|o| o.as_reference().ok()).collect())
        .unwrap_or_default();
    let mut d = (props.get(b"D").ok())
        .and_then(|o| resolve(pdf, o))
        .and_then(|o| o.as_dict().ok())
        .cloned()
        .unwrap_or_default();
    let refs = |hidden: bool| -> Object {
        let listed = groups
            .iter()
            .filter(|&&g| off.contains(&LayerId(g)) == hidden);
        Object::Array(listed.map(|&g| Object::Reference(g)).collect())
    };
    d.set("BaseState", Object::Name(b"ON".to_vec()));
    d.set("ON", refs(false));
    d.set("OFF", refs(true));
    // Usage-based auto states (/AS) would override the lists.
    d.remove(b"AS");
    let mut props = props.clone();
    props.set("D", d);
    Some(Object::Dictionary(props))
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::{Document as Pdf, dictionary};

    fn sample() -> (Pdf, ObjectId, ObjectId, ObjectId) {
        let mut pdf = Pdf::with_version("1.7");
        let a = pdf
            .add_object(dictionary! { "Type" => "OCG", "Name" => Object::string_literal("Grid") });
        let b = pdf
            .add_object(dictionary! { "Type" => "OCG", "Name" => Object::string_literal("Notes") });
        let c = pdf.add_object(
            dictionary! { "Type" => "OCG", "Name" => Object::string_literal("Orphan") },
        );
        let d = pdf.add_object(dictionary! {
            "OFF" => vec![b.into()],
            "Locked" => vec![a.into()],
            "Order" => vec![
                a.into(),
                vec![Object::string_literal("Review"), b.into()].into(),
            ],
        });
        let catalog = pdf.add_object(dictionary! {
            "Type" => "Catalog",
            "OCProperties" => dictionary! { "OCGs" => vec![a.into(), b.into(), c.into()], "D" => d },
        });
        pdf.trailer.set("Root", catalog);
        (pdf, a, b, c)
    }

    #[test]
    fn lists_in_order_with_headings_and_states() {
        let (mut pdf, a, b, c) = sample();
        let l = layers_in(&pdf);
        // Read from a source, as a file opens: the same rows.
        assert_eq!(layers_in(crate::open::tests::lazily(&mut pdf).pdf()), l);
        let rows: Vec<(Option<LayerId>, &str, bool, bool, u16)> = l
            .iter()
            .map(|l| (l.id, l.name.as_str(), l.on, l.locked, l.depth))
            .collect();
        let [a, b, c] = [a, b, c].map(LayerId);
        assert_eq!(
            rows,
            [
                (Some(a), "Grid", true, true, 0),
                (None, "Review", true, true, 0),
                (Some(b), "Notes", false, false, 1),
                (Some(c), "Orphan", true, false, 0),
            ]
        );
    }

    /// A view's patch shows every layer but those it turns off, the one
    /// the file's default configuration hides included.
    #[test]
    fn a_patch_shows_every_layer_but_those_off() {
        let (mut pdf, a, _, c) = sample();
        let src = crate::open::tests::lazily(&mut pdf);
        let props = ocg_patch(&src, &[LayerId(a), LayerId(c)].into()).unwrap();
        pdf.catalog_mut().unwrap().set("OCProperties", props);
        let on: Vec<(String, bool)> = layers_in(&pdf)
            .into_iter()
            .filter(|l| l.id.is_some())
            .map(|l| (l.name, l.on))
            .collect();
        assert_eq!(
            on,
            [
                ("Grid".to_string(), false),
                ("Notes".to_string(), true),
                ("Orphan".to_string(), false)
            ]
        );
    }

    #[test]
    fn files_without_layers_list_none() {
        let mut pdf = crate::attachments::tests::sample();
        assert!(layers_in(&pdf).is_empty());
        let src = crate::open::tests::lazily(&mut pdf);
        assert!(ocg_patch(&src, &BTreeSet::new()).is_none());
    }
}
