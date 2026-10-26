//! One writer for saves (M10.5 §11.7): saves, Save As, extracted pages,
//! Clean Copy, Export Flattened, print and recovery sidecars reach their
//! bytes through here, and so do the PDFs sheaf builds in memory (blank
//! pages, images as pages, the stamp library's files, the comment summary,
//! FDF files and a tape's picture); lopdf's writer is left to tests and
//! examples (`docs/document-model.md`, "One writer").
//!
//! - [`object`]: an object as PDF syntax, and an indirect object's
//!   `n g obj` frame. Reals are written at f64, a NaN or an infinity
//!   refused and magnitudes clamped as Annex C has them.
//! - [`xref`]: cross-reference sections, as a table or a stream: a full
//!   write's one subsection with its free entries linked, an append's
//!   runs, a deleted object's entry at the next generation, an object
//!   stream's member as its container and place (a stream's alone), and
//!   positions counted from the header. A full write's stream is deflated
//!   under the PNG Up predictor; an append's is uncompressed.
//! - [`objstm`]: object streams (7.5.7): which objects may go in one, and
//!   a container of them, deflated, no bigger than the reader takes.
//! - [`group`]: which objects share a container, by the pages that use
//!   them, page 1's apart, as Annex F lays out a linearized file; free of
//!   lopdf's types, for the linearization crate.
//! - [`append`]: an update after a file's last section, in the base's
//!   cross-reference kind, `/Prev` at its real start, `/ID[0]` kept, its
//!   AES IVs keyed by what each object is.
//! - [`full`]: a whole file, with no `/Prev`: an object either copied as
//!   the source has it, at its own number, or written, on its own, its AES
//!   IVs keyed by what it is, or, asked to pack, in an object stream with
//!   those the same pages use, which is encrypted once; and a PDF built in
//!   memory, each object written at its number.
//! - [`built`]: what a PDF built in memory is made of: the resources its
//!   content draws with, marks as the writer makes them, and pages.
//! - [`crypt`]: each object encrypted on its own with the source's
//!   [`Crypt`](crypt::Crypt), an object stream whole, its AES IVs keyed
//!   hashes, so an encrypted file saves encrypted, its `/Encrypt` and
//!   `/ID[0]` kept.
//! - [`durable`]: a file replaced whole through a unique temp name that
//!   fits beside it, its permissions kept, fsynced, and refused when
//!   another program changed it since it was read.

pub mod append;
pub mod built;
pub mod crypt;
pub mod durable;
#[cfg(any(test, feature = "fixtures"))]
pub mod fixture;
pub mod full;
pub mod group;
pub mod object;
pub mod objstm;
#[cfg(test)]
pub(crate) mod tests;
pub mod xref;

use std::io::Write as _;

use anyhow::Context as _;
use lopdf::{Dictionary, Object, ObjectId, StringFormat};

use crate::materialize::ObjKey;
use crypt::Crypt;

/// The object numbers a written file may use are below it: ISO 32000-1
/// Annex C's 8,388,607 indirect objects, as many as sheaf's reader takes
/// (`ReadLimits::max_objects`), so a file sheaf writes is one it reads,
/// and a full write's cross-reference section, which lists every number
/// from 0, has at most that many rows.
pub const NUMBERS: u32 = lopdf::ReadLimits::DEFAULT.max_objects;

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
    /// An object in an object stream, by number: only a cross-reference
    /// stream can list one.
    #[error("a cross-reference table can't list object {0}, which is in an object stream")]
    Packed(u32),
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

/// Object `id`, which is `key`, written at the end of `out`, encrypted
/// under its id, its IVs keyed by `key`, when there's a `crypt`: where it
/// starts.
fn indirect_as(
    out: &mut Vec<u8>,
    id: ObjectId,
    key: &ObjKey,
    obj: &Object,
    crypt: Option<&Crypt>,
) -> anyhow::Result<usize> {
    let Some(crypt) = crypt else {
        return Ok(object::indirect(out, id, obj)?);
    };
    let obj = crypt
        .encrypt_as(id, key, obj)
        .with_context(|| format!("encrypting object {} {}", id.0, id.1))?;
    Ok(object::indirect(out, id, &obj)?)
}

/// `data` deflated at zlib's default level, as a stream whose `/Filter`
/// is `/FlateDecode` holds it.
fn deflated(data: &[u8]) -> Vec<u8> {
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    z.write_all(data).expect("deflating in memory");
    z.finish().expect("deflating in memory")
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
    let written = sheaf_util::checksum(written).to_be_bytes().to_vec();
    let first = kept.unwrap_or_else(|| written.clone());
    Some(Object::Array(vec![
        Object::String(first, StringFormat::Hexadecimal),
        Object::String(written, StringFormat::Hexadecimal),
    ]))
}
