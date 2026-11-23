//! PDF serialization, cross-reference section, object-stream, and encryption
//! primitives exposed through `sheaf_doc::pdf` (M18 slices 11–12), and the
//! family's per-body writer, [`Parts`], with what it writes ([`PartBody`],
//! [`Packed`]) and reads from ([`ReadFrom`], [`Record`]), for a whole file
//! written under a renumbering (M18 slice 14).

use pdf_objects::Object;

pub use self::object::{
    Encrypt, Error as ObjectError, Refused, Renumber, decimal, indirect, indirect_renumbered,
    indirect_stream, indirect_stream_with, indirect_with, real, write, write_renumbered,
};
pub use pdf_objects::serialize as object;
pub use pdf_structure::Record;
pub use pdf_structure::write::{
    Body as PartBody, Deflate, Encrypting, Encryptor, Entry, Error as StructureError, KeptOut,
    MAX_DATA, MEMBERS, NUMBERS, Packed, Parts, ReadFrom, Section, Unencryptable, container,
    encryption as encryption_dict, fits, kept_out, packable,
};

/// Writes `/name` in canonical PDF name syntax (`#xx` escaping delimiters,
/// whitespace, `#`, and bytes outside `!`..=`~`).
pub fn write_name(out: &mut Vec<u8>, name: &[u8]) {
    write(out, &Object::name(name)).expect("names without NUL serialize infallibly");
}

/// Maps broken `0 g R` indirect references to `None` (`null` in PDF syntax,
/// ISO 32000-2 §7.3.10) while keeping every non-zero reference unchanged.
#[inline]
#[must_use]
pub fn non_zero_ref(id: pdf_objects::ObjectId) -> Option<pdf_objects::ObjectId> {
    (id.number != 0).then_some(id)
}

/// Whether `obj` is an `/ObjStm` stream container (ISO 32000-2 §7.5.7).
#[must_use]
pub fn is_container(obj: &Object) -> bool {
    obj.as_stream().is_some_and(|s| s.dict.has_type(b"ObjStm"))
}

/// Parses a single serialized indirect object `buf` (`<num> <gen> obj ... endobj`)
/// back into an [`Object`], keeping any stream payload inline as `Data::Bytes`.
#[cfg(test)]
pub(crate) fn parse_indirect_object(buf: &[u8]) -> super::Result<Object> {
    let mut parser = pdf_objects::parse::Parser::at(buf, 0);
    let id = parser
        .header(&mut ())
        .map_err(|e| super::Error::Syntax(e.to_string()))?;
    match parser
        .body(id, &mut ())
        .map_err(|e| super::Error::Syntax(e.to_string()))?
    {
        pdf_objects::parse::Body::Object(obj) => Ok(obj),
        pdf_objects::parse::Body::Stream(head) => {
            let len = head
                .dict()
                .get(b"Length")
                .and_then(Object::as_integer)
                .and_then(|n| usize::try_from(n).ok());
            let span = len
                .and_then(|n| head.declared(n))
                .unwrap_or_else(|| head.scanned());
            let mut obj = parser
                .stream(head, span, &mut ())
                .map_err(|e| super::Error::Syntax(e.to_string()))?;
            if let pdf_objects::Value::Stream(s) = obj.value_mut() {
                s.data = pdf_objects::Data::Bytes(buf[span.range()].to_vec());
            }
            Ok(obj)
        }
    }
}
