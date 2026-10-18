//! A whole file: the header, every object, and one cross-reference section
//! in the base's kind, with no `/Prev`, no `/XRefStm` and no object
//! streams. Encrypted, each object is encrypted under the number it's
//! written as, and the source's `/Encrypt` and `/ID[0]` are kept.

use std::collections::BTreeMap;

use anyhow::Result;
use lopdf::{Dictionary, Object, ObjectId, XrefKind};

use super::crypt::Crypt;
use super::object;
use super::xref::{Entry, Section};

/// After the header, a comment of bytes past ASCII, so a tool that guesses
/// from the first lines takes the file for binary.
pub(crate) const BINARY: &[u8] = b"%\xE2\xE3\xCF\xD3\n";

/// Trailer keys the base's cross-reference sections had, which this
/// section writes anew or doesn't have: its links to other sections, a
/// cross-reference stream's dictionary (lopdf reads it as the trailer),
/// and the encryption and `/ID`.
const SECTION_KEYS: [&[u8]; 15] = [
    b"Prev",
    b"XRefStm",
    b"Type",
    b"Size",
    b"W",
    b"Index",
    b"Length",
    b"Filter",
    b"DecodeParms",
    b"F",
    b"FFilter",
    b"FDecodeParms",
    b"DL",
    b"Encrypt",
    b"ID",
];

/// `objects` as a PDF `version` file whose cross-references are `kind`
/// and whose trailer is `base`'s, but for its sections' own keys. With
/// `crypt`, each object is encrypted under its number, and `crypt`'s
/// `/Encrypt` dictionary, as stored, is the object after the last.
///
/// The `/ID` keeps the first string, `crypt`'s or else `base`'s, and the
/// second is a hash of what's written, so the same objects write the same
/// bytes. A file with neither gets one only when it's encrypted.
///
/// An object stream, a cross-reference stream or a linearization
/// dictionary among `objects` is left out: they'd describe the base's
/// layout, not this file's.
pub fn write(
    objects: &BTreeMap<ObjectId, Object>,
    base: &Dictionary,
    kind: XrefKind,
    version: &str,
    crypt: Option<&Crypt>,
) -> Result<Vec<u8>> {
    let mut out = format!("%PDF-{version}\n").into_bytes();
    out.extend_from_slice(BINARY);
    let mut section = Section::full(1);
    let mut next = 1;
    for (&id, obj) in objects {
        if left_out(obj) {
            continue;
        }
        let pos = super::indirect(&mut out, id, obj, crypt)?;
        section.insert(
            id.0,
            Entry::InUse {
                pos,
                generation: id.1,
            },
        );
        next = next.max(id.0 + 1);
    }
    let mut trailer = base.clone();
    for key in SECTION_KEYS {
        trailer.remove(key);
    }
    if let Some(crypt) = crypt {
        let dict = Object::Dictionary(crypt.dict().clone());
        let pos = object::indirect(&mut out, (next, 0), &dict)?;
        section.insert(next, Entry::InUse { pos, generation: 0 });
        trailer.set("Encrypt", Object::Reference((next, 0)));
        next += 1;
    }
    let kept = crypt
        .and_then(Crypt::first_id)
        .map(<[u8]>::to_vec)
        .or_else(|| super::first_id(base));
    if let Some(id) = super::file_id(kept, crypt.is_some(), &out) {
        trailer.set("ID", id);
    }
    let pos = out.len();
    match kind {
        XrefKind::Table => section.table(&mut out, trailer, pos)?,
        XrefKind::Stream => section.stream(&mut out, next, trailer, pos)?,
    }
    Ok(out)
}

/// Whether `obj` is one a full write leaves out: an object stream (its
/// members are written as objects of their own), a cross-reference stream
/// or a linearization dictionary.
fn left_out(obj: &Object) -> bool {
    match obj {
        Object::Stream(s) => matches!(s.dict.get_type(), Ok(t) if t == b"ObjStm" || t == b"XRef"),
        Object::Dictionary(d) => d.has(b"Linearized"),
        _ => false,
    }
}
