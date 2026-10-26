//! Object streams (ISO 32000-1 7.5.7): which objects may go in one, and a
//! container of them as the writer packs it, its members as direct
//! objects after a header of where each starts, deflated.

use lopdf::{Dictionary, Object, ObjectId, Stream, dictionary};

use super::object;

/// The most a container's data may hold, as sheaf's reader decompresses
/// it: [`ReadLimits::max_decompressed`](lopdf::ReadLimits), so a file
/// sheaf writes is one it reads.
pub const MAX_DATA: usize = lopdf::ReadLimits::DEFAULT.max_decompressed;

/// Whether a container whose data is `len` bytes is one the reader takes.
pub fn fits(len: usize) -> bool {
    len <= MAX_DATA
}

/// Whether `obj` is an object stream (7.5.7): a container of others.
pub fn is_container(obj: &Object) -> bool {
    obj.as_stream().is_ok_and(|s| s.dict.has_type(b"ObjStm"))
}

/// Whether object `id`, `obj`, may go in an object stream (7.5.7): not a
/// stream, an object whose generation isn't 0, an encryption dictionary
/// (the trailer's `/Encrypt`, or an earlier revision's) or the
/// linearization dictionary; nor a signature dictionary, so a later
/// signing tool finds it where it expects, its `/Contents` unencrypted as
/// the walk leaves it ([`crypt`](super::crypt)). A cross-reference stream
/// is a stream, and a container's own `/Length` is direct
/// ([`container`]).
pub fn packable(id: ObjectId, obj: &Object) -> bool {
    if id.1 != 0 {
        return false;
    }
    match obj {
        Object::Stream(_) => false,
        Object::Dictionary(d) => !(encryption(d) || signature(d) || d.has(b"Linearized")),
        _ => true,
    }
}

/// Whether `d` has an encryption dictionary's keys, as lopdf's walk tells
/// one: a `/Filter` name, `/O` and `/U`.
fn encryption(d: &Dictionary) -> bool {
    d.get(b"Filter").and_then(Object::as_name).is_ok() && d.has(b"O") && d.has(b"U")
}

/// Whether `d` is a signature dictionary, as lopdf's walk (after PDFium)
/// tells one: a `/Type` of `/Sig` or `/DocTimeStamp`, or with no `/Type`,
/// that `/FT`; with neither, a signature's `/ByteRange` and `/Contents`,
/// since its `/Type` is optional (ISO 32000-2 Table 255).
fn signature(d: &Dictionary) -> bool {
    match d.get(b"Type").or_else(|_| d.get(b"FT")) {
        Ok(kind) => (kind.as_name()).is_ok_and(|kind| kind == b"Sig" || kind == b"DocTimeStamp"),
        Err(_) => d.has(b"ByteRange") && d.has(b"Contents"),
    }
}

/// An object stream of `members`, each its number and its object as
/// direct syntax ([`object::write`](super::object::write)), in order: a
/// header of each member's number and where it starts, counted from
/// `/First`, then each member followed by a newline, deflated. Its
/// `/Length` is direct, as the writer writes every stream's. None when
/// its data wouldn't [`fit`](fits).
pub fn container(members: &[(u32, Vec<u8>)]) -> Option<Stream> {
    let mut header = Vec::new();
    let mut offset = 0;
    for (i, (n, bytes)) in members.iter().enumerate() {
        if i > 0 {
            header.push(b' ');
        }
        object::decimal(&mut header, (*n).into());
        header.push(b' ');
        object::decimal(&mut header, offset as u64);
        offset += bytes.len() + 1;
    }
    header.push(b'\n');
    let first = header.len();
    if !fits(first + offset) {
        return None;
    }
    let mut data = header;
    data.reserve(offset);
    for (_, bytes) in members {
        data.extend_from_slice(bytes);
        data.push(b'\n');
    }
    let dict = dictionary! {
        "Type" => "ObjStm",
        "N" => members.len() as i64,
        "First" => first as i64,
        "Filter" => "FlateDecode",
    };
    Some(Stream::new(dict, super::deflated(&data)))
}
