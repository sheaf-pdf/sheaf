//! One writer for saves (M10.5 §11.7, M18 slice 12): saves, Save As,
//! extracted pages, Clean Copy, Export Flattened, print and recovery sidecars
//! reach their bytes through here, and so do the PDFs sheaf builds in memory,
//! backed by `pdf-objects::serialize`, `pdf-structure::write`, `pdf-filters`
//! (`Backend::ZlibRs`), and `pdf-crypt`.

pub mod append;
pub mod built;
pub mod crypt;
pub mod durable;
#[cfg(any(test, feature = "fixtures"))]
pub mod fixture;
pub mod full;
pub mod group;
#[cfg(test)]
pub(crate) mod tests;

/// Direct and indirect object serialization backed by `pdf_objects::serialize`.
pub mod object {
    pub use crate::pdf::write::{
        ObjectError as Error, decimal, indirect, indirect_stream, indirect_stream_with,
        indirect_with, real, write, write_name as name,
    };
}

/// Object-stream eligibility and container assembly backed by `pdf_structure::write`.
pub mod objstm {
    use crate::pdf::Stream;
    pub use crate::pdf::write::{MAX_DATA, fits, is_container, packable};

    /// Packs `(object_number, direct_syntax_bytes)` pairs into a `/FlateDecode`
    /// `/ObjStm` stream using `Backend::ZlibRs`, returning `None` if the
    /// uncompressed payload exceeds [`MAX_DATA`].
    #[must_use]
    pub fn container(members: &[(u32, Vec<u8>)]) -> Option<Stream> {
        crate::pdf::write::container(members, crate::pdf::deflate_options())
            .ok()
            .flatten()
    }
}

/// Cross-reference table and PNG-Up deflated cross-reference stream writer
/// backed by `pdf_structure::write`.
pub mod xref {
    pub use crate::pdf::write::{Entry, Section, StructureError as Error};
}

use anyhow::Context as _;

use crate::pdf::write::Encrypting;
use crate::pdf::{Dictionary, Object, ObjectId};

/// The object numbers a written file may use are below it: ISO 32000-1
/// Annex C's 8,388,607 indirect objects (`pdf_structure::write::NUMBERS`).
pub const NUMBERS: u32 = crate::pdf::write::NUMBERS;

/// Writes `obj` at the end of `out` as indirect object `id`, encrypting
/// strings and stream bytes on the fly when `encrypting` is `Some`.
fn indirect_as(
    out: &mut Vec<u8>,
    id: ObjectId,
    obj: &Object,
    encrypting: Option<&Encrypting<'_>>,
) -> anyhow::Result<usize> {
    use crate::pdf::write::{Refused, indirect_renumbered, non_zero_ref};
    match encrypting {
        None => match indirect_renumbered(out, id, obj, &(), &non_zero_ref) {
            Ok(offset) => Ok(offset),
            Err(Refused::Write(e)) => Err(e.into()),
            Err(Refused::Encrypt { why, .. }) => match why {},
        },
        Some(enc) => indirect_renumbered(out, id, obj, enc, &non_zero_ref)
            .with_context(|| format!("writing encrypted object {id}")),
    }
}

/// `trailer`'s `/ID[0]` when `/ID` is an array whose first element is a string.
fn first_id(trailer: &Dictionary) -> Option<Vec<u8>> {
    trailer
        .get(b"ID")
        .and_then(Object::as_array)?
        .first()?
        .as_bytes()
        .map(<[u8]>::to_vec)
}

/// A trailer's `/ID`: `kept` (the first string a file already has) and a
/// checksum of `written`. A file with no first string gets the checksum for
/// both when `encrypted`, and `None` otherwise.
fn file_id(kept: Option<Vec<u8>>, encrypted: bool, written: &[u8]) -> Option<Object> {
    if kept.is_none() && !encrypted {
        return None;
    }
    let written = sheaf_util::checksum(written).to_be_bytes().to_vec();
    let first = kept.unwrap_or_else(|| written.clone());
    Some(Object::from(vec![Object::hex(first), Object::hex(written)]))
}
