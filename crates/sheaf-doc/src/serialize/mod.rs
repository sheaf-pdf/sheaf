//! One writer for saves (M10.5 §11.7): saves, Save As, extracted pages,
//! Clean Copy, Export Flattened, print and recovery sidecars reach their
//! bytes through here. lopdf's writer still writes the rest, each until a
//! later slice moves it (`docs/document-model.md`, "One writer"): the
//! derived copies, the comment summary and FDF files, PDFs made from
//! images and blank pages, and the stamp library's files.
//!
//! - [`object`]: an object as PDF syntax, and an indirect object's
//!   `n g obj` frame. Reals are written at f64, a NaN or an infinity
//!   refused and magnitudes clamped as Annex C has them.
//! - [`xref`]: cross-reference sections, as a table or a stream: a full
//!   write's one subsection with its free entries linked, an append's
//!   runs, a deleted object's entry at the next generation, and positions
//!   counted from the header.
//! - [`append`]: an update after a file's last section, in the base's
//!   cross-reference kind, `/Prev` at its real start, `/ID[0]` kept.
//! - [`full`]: a whole file, with no `/Prev` and no object streams.
//! - [`crypt`]: each object encrypted on its own with the source's
//!   [`Crypt`](crypt::Crypt), its AES IVs keyed hashes, so an encrypted
//!   file saves encrypted, its `/Encrypt` and `/ID[0]` kept.
//! - [`durable`]: a file replaced whole through a unique temp name that
//!   fits beside it, its permissions kept, fsynced, and refused when
//!   another program changed it since it was read.

pub mod append;
pub mod crypt;
pub mod durable;
#[cfg(any(test, feature = "fixtures"))]
pub mod fixture;
pub mod full;
pub mod object;
#[cfg(test)]
pub(crate) mod tests;
pub mod xref;

use anyhow::Context as _;
use lopdf::{Dictionary, Object, ObjectId, StringFormat};

use crypt::Crypt;

/// Why the serializer wrote nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    /// A real that's NaN or an infinity, which no reader can read back,
    /// in object `id` when that's known.
    #[error("a real that isn't a finite number{}", in_object(.id))]
    NonFinite { id: Option<ObjectId> },
    /// An offset past a cross-reference table's ten digits.
    #[error("a cross-reference table can't list offset {0}")]
    TooFar(u64),
}

impl Error {
    /// The error, said of object `id`.
    fn within(self, id: ObjectId) -> Error {
        match self {
            Error::NonFinite { id: None } => Error::NonFinite { id: Some(id) },
            e => e,
        }
    }
}

fn in_object(id: &Option<ObjectId>) -> String {
    id.map(|(n, g)| format!(" in object {n} {g}"))
        .unwrap_or_default()
}

/// Object `id` written at the end of `out`, encrypted under its id when
/// there's a `crypt`: where it starts.
fn indirect(
    out: &mut Vec<u8>,
    id: ObjectId,
    obj: &Object,
    crypt: Option<&Crypt>,
) -> anyhow::Result<usize> {
    let Some(crypt) = crypt else {
        return Ok(object::indirect(out, id, obj)?);
    };
    let obj = crypt
        .encrypt(id, obj)
        .with_context(|| format!("encrypting object {} {}", id.0, id.1))?;
    Ok(object::indirect(out, id, &obj)?)
}

/// `trailer`'s `/ID[0]`, read as lopdf makes an RC4 or AES-128 key from
/// it: the array's first element when that's a string, however many
/// elements the array has.
fn first_id(trailer: &Dictionary) -> Option<Vec<u8>> {
    match trailer
        .get(b"ID")
        .and_then(Object::as_array)
        .ok()?
        .first()?
    {
        Object::String(first, _) => Some(first.clone()),
        _ => None,
    }
}

/// A trailer's `/ID`: `kept`, the first string a file already has, and a
/// hash of `written`, so the same bytes get the same `/ID`. A file with no
/// first string gets the hash for both when it's `encrypted`, which needs
/// an `/ID`, and none otherwise.
fn file_id(kept: Option<Vec<u8>>, encrypted: bool, written: &[u8]) -> Option<Object> {
    if kept.is_none() && !encrypted {
        return None;
    }
    let written = sheaf_core::checksum(written).to_be_bytes().to_vec();
    let first = kept.unwrap_or_else(|| written.clone());
    Some(Object::Array(vec![
        Object::String(first, StringFormat::Hexadecimal),
        Object::String(written, StringFormat::Hexadecimal),
    ]))
}
