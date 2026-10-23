//! A whole file: the header, every object, and one cross-reference section
//! in the base's kind, with no `/Prev`, no `/XRefStm` and no object
//! streams. Encrypted, each object is encrypted under the number it's
//! written as, and the source's `/Encrypt` and `/ID[0]` are kept.
//!
//! [`write_bodies`] is the materializer's (M10.5 §11.7): an object is
//! either its source's bytes, copied at its own number (raw passthrough),
//! or written, its AES IVs keyed by its [`ObjKey`]; the header gives the
//! version the emission needs. [`write_built`] is a PDF sheaf builds in
//! memory, written through it.

use std::borrow::Cow;

use anyhow::{Result, bail};
use in_progress::Progress;
use lopdf::{Dictionary, Object, ObjectId, XrefFacts, XrefKind};

use super::crypt::Crypt;
use super::object;
use super::xref::{Entry, Section};
use crate::materialize::ObjKey;
use crate::materialize::emit::PdfVersion;

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

/// What [`write_bodies`] writes of an object.
pub enum Body<'a> {
    /// The object as its source holds it, `n g obj` to `endobj`: copied as
    /// it is, at the number it gives (raw passthrough, §11.6).
    Raw(&'a [u8]),
    /// An object, which is the key: written, and encrypted when the file
    /// is, its IVs keyed by the key.
    Object(Cow<'a, Object>, ObjKey),
}

/// How [`write_bodies`] writes what isn't one of its objects.
pub struct Layout<'a> {
    /// The trailer: its `/Root` and `/Info`, and any key but its sections'
    /// own ([`SECTION_KEYS`]), which are left out.
    pub trailer: &'a Dictionary,
    pub kind: XrefKind,
    /// The header's.
    pub version: PdfVersion,
    pub crypt: Option<&'a Crypt>,
    /// Where the `/Encrypt` dictionary goes, with a `crypt`: at its
    /// source's number on a stable save, else after the last object.
    pub encrypt_at: ObjectId,
    /// The least number a cross-reference stream takes: on a stable save,
    /// past every number the primary and the previous save used, so it
    /// never takes a number an object pruned from the primary had.
    pub xref_from: u32,
}

/// `bodies`, by number, as a whole file laid out as `layout` says, `p`
/// saying when to stop: the bytes, and what its cross-reference section
/// says. The `/ID` keeps the first string, the crypt's or else the
/// trailer's, and the second is a hash of what's written, so the same
/// bodies write the same bytes. An encrypted file with neither gets the
/// hash for both. A number the `/Encrypt` dictionary should go at that a
/// body has is refused, and so is any number a file may not use
/// ([`NUMBERS`](super::NUMBERS)): the section lists every number below the
/// last, so one past it would list billions.
pub fn write_bodies<'a>(
    bodies: impl IntoIterator<Item = Result<(ObjectId, Body<'a>)>>,
    layout: &Layout<'_>,
    p: &dyn Progress,
) -> Result<(Vec<u8>, XrefFacts)> {
    let mut out = format!("%PDF-{}\n", layout.version).into_bytes();
    out.extend_from_slice(BINARY);
    let mut section = Section::full(1);
    let mut next = 1;
    for body in bodies {
        p.check()?;
        let (id, body) = body?;
        usable(id.0)?;
        let pos = match body {
            Body::Raw(bytes) => {
                let pos = out.len();
                out.extend_from_slice(bytes);
                out.push(b'\n');
                pos
            }
            Body::Object(obj, key) => super::indirect_as(&mut out, id, &key, &obj, layout.crypt)?,
        };
        section.insert(
            id.0,
            Entry::InUse {
                pos,
                generation: id.1,
            },
        );
        next = next.max(id.0 + 1);
    }
    let mut trailer = layout.trailer.clone();
    for key in SECTION_KEYS {
        trailer.remove(key);
    }
    if let Some(crypt) = layout.crypt {
        let at = layout.encrypt_at;
        usable(at.0)?;
        if section.lists(at.0) {
            bail!("object {} is the encryption dictionary's", at.0);
        }
        let dict = Object::Dictionary(crypt.dict().clone());
        let pos = object::indirect(&mut out, at, &dict)?;
        section.insert(
            at.0,
            Entry::InUse {
                pos,
                generation: at.1,
            },
        );
        trailer.set("Encrypt", Object::Reference(at));
        next = next.max(at.0 + 1);
    }
    let kept = (layout.crypt)
        .and_then(Crypt::first_id)
        .map(<[u8]>::to_vec)
        .or_else(|| super::first_id(layout.trailer));
    let id = super::file_id(kept, layout.crypt.is_some(), &out);
    if let Some(id) = &id {
        trailer.set("ID", id.clone());
    }
    let pos = out.len();
    let (size, max_stream) = match layout.kind {
        XrefKind::Table => {
            let size = section.size();
            section.table(&mut out, trailer, pos)?;
            (size, None)
        }
        XrefKind::Stream => {
            let at = next.max(layout.xref_from);
            usable(at)?;
            section.stream(&mut out, at, trailer, pos)?;
            (at + 1, Some(at))
        }
    };
    let id = match id {
        Some(Object::Array(ids)) => match ids.as_slice() {
            [Object::String(a, _), Object::String(b, _)] => Some([a.clone(), b.clone()]),
            _ => None,
        },
        _ => None,
    };
    let facts = XrefFacts {
        kind: layout.kind,
        start: pos,
        size,
        max_stream,
        id,
        header_offset: 0,
        hybrid: false,
        linearized: false,
        sections: 1,
    };
    Ok((out, facts))
}

/// `pdf`, a document sheaf built in memory (blank pages, images as pages,
/// the stamp library's files), as a whole file through [`write_bodies`]:
/// each object written anew at its number, unencrypted, with a
/// cross-reference table and the header `pdf`'s version gives. The same
/// document writes the same bytes, so it interns as the same blob.
pub fn write_built(pdf: &lopdf::Document) -> Result<Vec<u8>> {
    let Some(version) = PdfVersion::parse(pdf.version.as_bytes()) else {
        bail!("{:?} isn't a PDF version", pdf.version);
    };
    let bodies = (pdf.objects.iter())
        .map(|(&id, obj)| Ok((id, Body::Object(Cow::Borrowed(obj), ObjKey::Built(id)))));
    let layout = Layout {
        trailer: &pdf.trailer,
        kind: XrefKind::Table,
        version,
        crypt: None,
        encrypt_at: (0, 0),
        xref_from: 0,
    };
    Ok(write_bodies(bodies, &layout, &in_progress::NoProgress)?.0)
}

/// Whether object number `n` is one a file may use.
fn usable(n: u32) -> Result<()> {
    if n >= super::NUMBERS {
        bail!(
            "object {n} is past the {} numbers a PDF may use",
            super::NUMBERS
        );
    }
    Ok(())
}

/// Whether `obj` is one a full write leaves out: an object stream (its
/// members are written as objects of their own), a cross-reference stream
/// or a linearization dictionary.
pub(crate) fn left_out(obj: &Object) -> bool {
    match obj {
        Object::Stream(s) => matches!(s.dict.get_type(), Ok(t) if t == b"ObjStm" || t == b"XRef"),
        Object::Dictionary(d) => d.has(b"Linearized"),
        _ => false,
    }
}
