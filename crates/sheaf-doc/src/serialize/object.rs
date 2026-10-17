//! Objects as PDF syntax, written by hand: lopdf's writer is private, and
//! writes a real past i64's range in a form its own parser can't read
//! back.

use std::io::Write as _;

use lopdf::{Dictionary, Object, ObjectId, StringFormat};

use super::Error;

/// The largest magnitude a real is written at: Annex C's, for readers
/// that hold reals in single precision.
const MAX_REAL: f64 = 3.4e38;

/// The smallest nonzero magnitude: a real nearer 0 is written as 0.
const MIN_REAL: f64 = 1.175e-38;

/// `obj` as a direct object. A real that isn't finite is refused, and
/// then nothing is written.
pub fn write(out: &mut Vec<u8>, obj: &Object) -> Result<(), Error> {
    let start = out.len();
    value(out, obj).inspect_err(|_| out.truncate(start))
}

/// `obj` as indirect object `id`, `n g obj` to `endobj`, and where in
/// `out` it starts. Refused as [`write()`] refuses, the error naming `id`.
pub fn indirect(out: &mut Vec<u8>, id: ObjectId, obj: &Object) -> Result<usize, Error> {
    let start = out.len();
    let _ = writeln!(out, "{} {} obj", id.0, id.1);
    match value(out, obj) {
        Ok(()) => {
            out.extend_from_slice(b"\nendobj\n");
            Ok(start)
        }
        Err(e) => {
            out.truncate(start);
            Err(e.within(id))
        }
    }
}

/// `x` in its shortest form that reads back as the same f64, never with
/// an exponent and always with a point, so an integral real reads back
/// as a real. Magnitudes past [`MAX_REAL`] are clamped to it, and those
/// under [`MIN_REAL`] written as 0, as is -0.
pub fn real(out: &mut Vec<u8>, x: f64) -> Result<(), Error> {
    if !x.is_finite() {
        return Err(Error::NonFinite { id: None });
    }
    let x = if x.abs() > MAX_REAL {
        MAX_REAL.copysign(x)
    } else if x.abs() < MIN_REAL {
        0.0
    } else {
        x
    };
    let start = out.len();
    let _ = write!(out, "{x}");
    if !out[start..].contains(&b'.') {
        out.extend_from_slice(b".0");
    }
    Ok(())
}

/// `/` and `name`, `#`, white-space, a delimiter or a byte outside
/// `!`–`~` written `#xx`.
pub fn name(out: &mut Vec<u8>, name: &[u8]) {
    out.push(b'/');
    for &b in name {
        if b.is_ascii_graphic() && !b"#()<>[]{}/%".contains(&b) {
            out.push(b);
        } else {
            let _ = write!(out, "#{b:02X}");
        }
    }
}

/// `text` as a string in `format`. A literal escapes `\`, CR (which would
/// read back as LF) and any parenthesis left unbalanced.
pub fn string(out: &mut Vec<u8>, text: &[u8], format: StringFormat) {
    match format {
        StringFormat::Literal => {
            let mut open = Vec::new();
            let mut lone = Vec::new();
            for (i, &b) in text.iter().enumerate() {
                match b {
                    b'(' => open.push(i),
                    b')' if open.pop().is_none() => lone.push(i),
                    _ => {}
                }
            }
            lone.extend(open);
            lone.sort_unstable();
            let mut lone = lone.into_iter().peekable();
            out.push(b'(');
            for (i, &b) in text.iter().enumerate() {
                match b {
                    b'\\' => out.extend_from_slice(b"\\\\"),
                    b'\r' => out.extend_from_slice(b"\\r"),
                    b'(' | b')' if lone.next_if_eq(&i).is_some() => {
                        out.extend_from_slice(&[b'\\', b]);
                    }
                    b => out.push(b),
                }
            }
            out.push(b')');
        }
        StringFormat::Hexadecimal => {
            out.push(b'<');
            for b in text {
                let _ = write!(out, "{b:02X}");
            }
            out.push(b'>');
        }
    }
}

fn value(out: &mut Vec<u8>, obj: &Object) -> Result<(), Error> {
    match obj {
        Object::Null => out.extend_from_slice(b"null"),
        Object::Boolean(b) => out.extend_from_slice(if *b { b"true" } else { b"false" }),
        Object::Integer(i) => {
            let _ = write!(out, "{i}");
        }
        Object::Real(x) => real(out, *x)?,
        Object::Name(n) => name(out, n),
        Object::String(text, format) => string(out, text, *format),
        Object::Array(items) => {
            out.push(b'[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 && spaced(item) {
                    out.push(b' ');
                }
                value(out, item)?;
            }
            out.push(b']');
        }
        Object::Dictionary(d) => dictionary(out, d, None)?,
        Object::Stream(s) => {
            dictionary(out, &s.dict, Some(s.content.len()))?;
            out.extend_from_slice(b"\nstream\n");
            out.extend_from_slice(&s.content);
            out.extend_from_slice(b"\nendstream");
        }
        Object::Reference((n, g)) => {
            let _ = write!(out, "{n} {g} R");
        }
    }
    Ok(())
}

/// `d`, its `/Length` the stream's `length` when it's a stream's.
fn dictionary(out: &mut Vec<u8>, d: &Dictionary, length: Option<usize>) -> Result<(), Error> {
    out.extend_from_slice(b"<<");
    for (k, v) in d {
        name(out, k);
        match length {
            Some(n) if k == b"Length" => {
                let _ = write!(out, " {n}");
            }
            _ => {
                if spaced(v) {
                    out.push(b' ');
                }
                value(out, v)?;
            }
        }
    }
    if let Some(n) = length
        && !d.has(b"Length")
    {
        let _ = write!(out, "/Length {n}");
    }
    out.extend_from_slice(b">>");
    Ok(())
}

/// Whether `obj` starts with a regular character, so needs white-space
/// after what comes before it.
fn spaced(obj: &Object) -> bool {
    matches!(
        obj,
        Object::Null
            | Object::Boolean(_)
            | Object::Integer(_)
            | Object::Real(_)
            | Object::Reference(_)
    )
}
