//! An update after a file's last section: the objects it writes, each
//! encrypted on its own when the file is, and a section in the file's
//! cross-reference kind whose runs list only those, its `/Prev` at the
//! last section's real start. Positions count from the file's header, so
//! bytes a writer put before `%PDF-` don't shift them. `/ID[0]` is kept,
//! `/ID[1]` is new, and `/Encrypt` is repeated.

use std::collections::BTreeMap;

use anyhow::{Context as _, Result};
use lopdf::{Dictionary, Object, ObjectId, XrefKind};

use super::crypt::Crypt;
use super::xref::{Entry, Section};
use crate::source::Source;

/// The file an update goes after, as its last section describes it.
#[derive(Debug, Clone)]
pub struct Base {
    /// Its length: where the update starts.
    pub len: usize,
    /// Whether its last line is ended; if not, the update ends it.
    pub ends_line: bool,
    /// Where its header, `%PDF-`, starts: positions count from there.
    pub header: usize,
    /// Where its last section starts, from the header.
    pub prev: usize,
    /// Past every object number it uses, its cross-reference streams'
    /// included: where the update's new numbers start.
    pub size: u32,
    /// Its last section's kind, which the update's is.
    pub kind: XrefKind,
    /// Its `/ID[0]`, read as an encrypted file's key is made from it.
    pub first_id: Option<Vec<u8>>,
    /// Its catalog.
    pub root: ObjectId,
    /// Its document information, as the trailer gives it.
    pub info: Option<Object>,
}

impl Base {
    /// The file `src` was read from. A file that says it numbers objects
    /// past the reader's cap (Annex C's 8,388,607), as only a hostile one
    /// does, has no number left for an update to take.
    pub fn of(src: &Source) -> Result<Base> {
        let bytes = src.bytes();
        let xref = src.xref();
        let trailer = src.pdf().trailer();
        let root = trailer
            .get(b"Root")
            .and_then(Object::as_reference)
            .context("the trailer names no catalog")?;
        let size = match xref.max_stream {
            Some(number) => number.checked_add(1).map(|past| past.max(xref.size)),
            None => Some(xref.size),
        }
        .filter(|&size| size <= src.limits().reader.max_objects)
        .context("no object number left for an update")?;
        Ok(Base {
            len: bytes.len(),
            ends_line: matches!(bytes.last(), Some(b'\n' | b'\r')),
            header: xref.header_offset,
            prev: xref.start,
            size,
            kind: xref.kind,
            first_id: super::first_id(trailer),
            root,
            info: trailer.get(b"Info").ok().cloned(),
        })
    }
}

/// The update after `base` that writes `objects`, each as the id it's
/// keyed by, and frees `deleted`, which aren't among them: nothing when
/// it would do neither. A stream section is object `xref_number`; a
/// table has no number. With `crypt`, each object is encrypted under its
/// id, and the trailer repeats `crypt`'s `/Encrypt`.
///
/// `/Size` is `base`'s, or past the highest number written. The `/ID`
/// keeps the first string, `crypt`'s or else `base`'s, and the second is
/// a hash of the objects written, so the same update writes the same
/// bytes. A file without one gets one only when it's encrypted.
pub fn section(
    base: &Base,
    objects: &BTreeMap<ObjectId, Object>,
    deleted: &[ObjectId],
    xref_number: u32,
    crypt: Option<&Crypt>,
) -> Result<Vec<u8>> {
    if objects.is_empty() && deleted.is_empty() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    if !base.ends_line {
        out.push(b'\n');
    }
    let mut section = Section::append(base.size, base.header);
    for (&id, obj) in objects {
        let pos = base.len + super::indirect(&mut out, id, obj, crypt)?;
        section.insert(
            id.0,
            Entry::InUse {
                pos,
                generation: id.1,
            },
        );
    }
    for &(num, generation) in deleted {
        section.insert(num, Entry::deleted(generation));
    }
    let mut trailer = Dictionary::new();
    trailer.set("Root", base.root);
    trailer.set("Prev", base.prev as i64);
    if let Some(info) = &base.info {
        trailer.set("Info", info.clone());
    }
    if let Some(crypt) = crypt {
        trailer.set("Encrypt", crypt.at());
    }
    let kept = crypt
        .and_then(Crypt::first_id)
        .map(<[u8]>::to_vec)
        .or_else(|| base.first_id.clone());
    if let Some(id) = super::file_id(kept, crypt.is_some(), &out) {
        trailer.set("ID", id);
    }
    let pos = base.len + out.len();
    match base.kind {
        XrefKind::Table => section.table(&mut out, trailer, pos)?,
        XrefKind::Stream => section.stream(&mut out, xref_number, trailer, pos)?,
    }
    Ok(out)
}
