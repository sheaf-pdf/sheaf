//! What the serializer writes, read back by lopdf's parser through
//! `LazyPdf`: objects as they were, cross-reference sections that read
//! as sound, objects encrypted as the reader decrypts them, whole files,
//! packed or not, rewrites of encrypted ones encrypted as they were,
//! rewrites packed when their sources used object streams, their
//! containers numbered as their sources' were, and updates after a
//! file's last section, encrypted as the file is and placed past any
//! bytes before its header.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use in_progress::NoProgress;
use lopdf::encryption::crypt_filters::{Aes128CryptFilter, CryptFilter};
use lopdf::{
    Dictionary, Document as Pdf, EncryptionState, EncryptionVersion, LazyPdf, LoadOptions, Object,
    ObjectId, Permissions, Stream, StringFormat, XrefKind, dictionary,
};
use sheaf_edit::Rotation;

use super::append::{self, Base};
use super::crypt::Crypt;
use super::group::Use;
use super::xref::{Entry, Section};
use super::{Error, group, object, objstm};
use crate::annot::{Annot, AnnotId, AnnotKind, Box2};
use crate::forms::fixture::hybrid_reference_encrypted;
use crate::history::FixedCount;
use crate::materialize::append::refs;
use crate::materialize::graph::page_uses;
use crate::model::Document;
use crate::serialize::fixture::{
    ATTACHED, BOOKMARK, FIELD, FORM_SECRETS, OWNER, PREFIX, USER, VALUE, attachment_only,
    encrypt_dict, encrypted, form, holds, in_clear, object_streams_encrypted, object_streams_of,
    one_id, rc4_40, rc4_aes_128_and_aes_256, whole, whole_packed,
};
use crate::source::Limits;
use crate::source::lazy_tests::{
    ATTACHMENT, CONTENT, FIELD_NAME, FORM_STRING, PAGE_STRING, SECRETS, SIGNATURE_WRITTEN, TITLE,
    XMP, assert_decrypts_to, assert_same, plaintext, same,
};
use crate::store::{BlobKind, Store};

fn open(bytes: &[u8]) -> LazyPdf {
    LazyPdf::open(Arc::<[u8]>::from(bytes), None, &Limits::DEFAULT.reader).unwrap()
}

fn catalog() -> Object {
    Object::Dictionary(dictionary! { "Type" => "Catalog" })
}

/// A file the serializer wrote: a catalog as object 1, then `objects`,
/// and a full table.
fn file(objects: &[(u32, Object)]) -> Vec<u8> {
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut section = Section::full(0);
    for (num, obj) in std::iter::once(&(1, catalog())).chain(objects) {
        let pos = object::indirect(&mut out, (*num, 0), obj).unwrap();
        section.insert(*num, Entry::InUse { pos, generation: 0 });
    }
    let pos = out.len();
    section
        .table(&mut out, dictionary! { "Root" => (1, 0) }, pos)
        .unwrap();
    out
}

/// `obj` written as object 2 of a file, and read back.
fn round_trip(obj: &Object) -> Object {
    open(&file(&[(2, obj.clone())]))
        .get((2, 0))
        .unwrap()
        .clone()
}

/// `obj` as written.
fn written(obj: &Object) -> String {
    let mut out = Vec::new();
    object::write(&mut out, obj).unwrap();
    String::from_utf8(out).unwrap()
}

#[test]
fn reals_round_trip_at_f64() {
    let kept = [
        0.1,
        1.0 / 3.0,
        595.32,
        1e-7,
        1e20,
        3.4e38,
        -3.4e38,
        1.2e-38,
        612.0,
        -612.0,
        0.0,
    ]
    .map(Object::Real)
    .into_iter()
    .chain([0.1f32, 1.0 / 3.0, 595.32, 841.89, 1e20].map(Object::from));
    let clamped = [
        (1e39, 3.4e38),
        (-f64::MAX, -3.4e38),
        (f64::from(f32::MAX), 3.4e38),
        (1e-39, 0.0),
        (-1e-300, 0.0),
        (-0.0, 0.0),
    ];
    let wrote: Vec<Object> = kept
        .clone()
        .chain(clamped.iter().map(|&(x, _)| Object::Real(x)))
        .collect();
    let want: Vec<f64> = kept
        .map(|o| o.as_f64().unwrap())
        .chain(clamped.iter().map(|&(_, y)| y))
        .collect();
    let Object::Array(back) = round_trip(&Object::Array(wrote)) else {
        panic!("not an array");
    };
    assert_eq!(back.len(), want.len());
    for (back, want) in back.iter().zip(want) {
        let Object::Real(x) = back else {
            panic!("{want} read back as {back:?}");
        };
        assert_eq!(x.to_bits(), want.to_bits(), "{want} read back as {x}");
    }
    // Shortest digits, a point always, and never an exponent.
    assert_eq!(written(&Object::Real(612.0)), "612.0");
    assert_eq!(written(&Object::Real(-0.0)), "0.0");
    assert_eq!(written(&Object::Real(1e-7)), "0.0000001");
    assert_eq!(written(&Object::from(0.1f32)), "0.1");
    assert_eq!(written(&Object::Real(1e20)), "100000000000000000000.0");
}

#[test]
fn nan_and_infinities_are_refused() {
    for x in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut out = b"%PDF-1.7\n".to_vec();
        assert_eq!(
            object::write(&mut out, &Object::Real(x)),
            Err(Error::NonFinite { id: None })
        );
        let deep = Object::Array(vec![
            Object::Integer(1),
            Object::Dictionary(dictionary! { "Rect" => vec![Object::Real(x)] }),
        ]);
        assert_eq!(
            object::indirect(&mut out, (7, 0), &deep),
            Err(Error::NonFinite { id: Some((7, 0)) })
        );
        let stream = Stream::new(
            dictionary! { "Decode" => vec![Object::Real(x)] },
            b"data".to_vec(),
        );
        assert_eq!(
            object::indirect(&mut out, (8, 1), &Object::Stream(stream)),
            Err(Error::NonFinite { id: Some((8, 1)) })
        );
        let trailer = dictionary! { "Root" => (1, 0), "Odd" => x };
        assert_eq!(
            Section::full(2).table(&mut out, trailer, 9),
            Err(Error::NonFinite { id: None })
        );
        assert_eq!(out, b"%PDF-1.7\n", "nothing written");
    }
    assert_eq!(
        Error::NonFinite { id: Some((7, 0)) }.to_string(),
        "a real that isn't a finite number in object 7 0"
    );
}

#[test]
fn strings_names_and_streams_round_trip() {
    let literal = |s: &[u8]| Object::String(s.to_vec(), StringFormat::Literal);
    let hex = |s: &[u8]| Object::String(s.to_vec(), StringFormat::Hexadecimal);
    let name = |s: &[u8]| Object::Name(s.to_vec());
    let values = Object::Array(vec![
        literal(b"balanced (parens (nest)) as they are"),
        literal(b"unbalanced ) and ( and ((x)"),
        literal(b"a back\\slash, and \\) escaped"),
        literal(b"CR\r alone, CR LF\r\n and LF\n"),
        literal(b"\x00\x80\xff binary"),
        literal(b""),
        hex(b"\x00\x01()\\\r"),
        hex(b""),
        name(b"A B"),
        name(b"#23 and /slash"),
        name(b"\x00\t\x7f\x80\xff"),
        name(b"()<>[]{}%"),
        name(b"Plain-Name_1.0"),
        name(b""),
        name(b"After"),
        Object::Dictionary(dictionary! {
            "True" => true,
            "Null" => Object::Null,
            "Ref" => (3, 0),
            "Neg" => -1,
            "Mixed" => vec![
                name(b"A"),
                name(b"B"),
                Object::Integer(1),
                Object::Real(2.5),
                Object::Boolean(false),
                Object::Null,
                Object::Reference((3, 0)),
                literal(b"s"),
            ],
        }),
    ]);
    let content = b"\x00binary\nendstream\nendobj\n\xff".to_vec();
    let mut long = Stream::new(dictionary! { "Kind" => "Test" }, content.clone());
    long.dict.set("Length", 999);
    let mut unsaid = Stream::new(dictionary! {}, content.clone());
    unsaid.dict.remove(b"Length");
    let mut indirect = Stream::new(dictionary! {}, content.clone());
    indirect.dict.set("Length", Object::Reference((9, 0)));
    let pdf = open(&file(&[
        (2, values.clone()),
        (3, Object::Stream(long)),
        (4, Object::Stream(unsaid)),
        (5, Object::Stream(indirect)),
    ]));
    assert_eq!(pdf.get((2, 0)).unwrap(), &values);
    for id in [(3, 0), (4, 0), (5, 0)] {
        let s = pdf.get(id).unwrap().as_stream().unwrap();
        assert_eq!(s.content, content, "{id:?}");
        let length = s.dict.get(b"Length").unwrap();
        assert_eq!(length, &Object::Integer(content.len() as i64), "{id:?}");
    }
    assert_eq!(
        pdf.get((3, 0))
            .unwrap()
            .as_stream()
            .unwrap()
            .dict
            .get(b"Kind")
            .unwrap(),
        &name(b"Test")
    );
}

/// Numbers, names and strings are written byte by byte: as `write!`
/// formatted them.
#[test]
fn numbers_names_and_strings_write_as_formatting_would() {
    let bytes = |obj: &Object| {
        let mut out = Vec::new();
        object::write(&mut out, obj).unwrap();
        out
    };
    let mut integers = vec![i64::MAX, i64::MIN, i64::MIN + 1, u32::MAX.into()];
    for power in 0..19 {
        let ten = 10i64.pow(power);
        integers.extend([ten - 1, ten, ten + 1, -ten, 1 - ten]);
    }
    for i in integers {
        assert_eq!(written(&Object::Integer(i)), format!("{i}"));
    }
    for id in [(0, 0), (7, 0), (10, 1), (u32::MAX, u16::MAX)] {
        let (n, g) = id;
        assert_eq!(written(&Object::Reference(id)), format!("{n} {g} R"));
        let mut out = Vec::new();
        object::indirect(&mut out, id, &Object::Null).unwrap();
        assert_eq!(out, format!("{n} {g} obj\nnull\nendobj\n").into_bytes());
    }
    let every: Vec<u8> = (0..=255).collect();
    let mut name = b"/".to_vec();
    for &b in &every {
        match b.is_ascii_graphic() && !b"#()<>[]{}/%".contains(&b) {
            true => name.push(b),
            false => name.extend(format!("#{b:02X}").bytes()),
        }
    }
    assert_eq!(bytes(&Object::Name(every.clone())), name);
    assert_eq!(
        written(&Object::Name(b"Plain-Name_1.0".to_vec())),
        "/Plain-Name_1.0"
    );
    let hex: String = every.iter().map(|b| format!("{b:02X}")).collect();
    let string = |text: &[u8], format| Object::String(text.to_vec(), format);
    assert_eq!(
        written(&string(&every, StringFormat::Hexadecimal)),
        format!("<{hex}>")
    );
    // A literal with nothing to escape is written as it is.
    let plain: Vec<u8> = (every.iter().copied())
        .filter(|b| !b"\\\r()".contains(b))
        .collect();
    let want = [b"(".as_slice(), &plain, b")"].concat();
    assert_eq!(bytes(&string(&plain, StringFormat::Literal)), want);
    assert_eq!(
        written(&string(b"a\\b\rc(d) e)", StringFormat::Literal)),
        "(a\\\\b\\rc(d) e\\))"
    );
    let mut stream = Stream::new(dictionary! {}, vec![0; 1234]);
    let head = |s: &Stream| bytes(&Object::Stream(s.clone()))[..16].to_vec();
    assert_eq!(head(&stream), b"<</Length 1234>>");
    stream.dict.remove(b"Length");
    assert_eq!(head(&stream), b"<</Length 1234>>");
}

#[test]
fn a_full_table_links_its_free_entries() {
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut section = Section::full(4);
    let one = object::indirect(&mut out, (1, 0), &catalog()).unwrap();
    section.insert(
        1,
        Entry::InUse {
            pos: one,
            generation: 0,
        },
    );
    let three = object::indirect(&mut out, (3, 2), &Object::Integer(3)).unwrap();
    section.insert(
        3,
        Entry::InUse {
            pos: three,
            generation: 2,
        },
    );
    // 2 was deleted at generation 4, 4 was never used, and 5, deleted,
    // grows the section past the size it was given.
    section.insert(2, Entry::Free { generation: 5 });
    section.insert(5, Entry::Free { generation: 1 });
    let pos = out.len();
    section
        .table(&mut out, dictionary! { "Root" => (1, 0) }, pos)
        .unwrap();
    let want = format!(
        "xref\n0 6\n\
         0000000002 65535 f \n\
         {one:010} 00000 n \n\
         0000000004 00005 f \n\
         {three:010} 00002 n \n\
         0000000005 00000 f \n\
         0000000000 00001 f \n\
         trailer\n<</Root 1 0 R/Size 6>>\nstartxref\n{pos}\n%%EOF\n"
    );
    assert_eq!(String::from_utf8_lossy(&out[pos..]), want);
    let pdf = open(&out);
    assert!(pdf.base_sound());
    assert_eq!(pdf.xref().kind, XrefKind::Table);
    assert_eq!(pdf.xref().size, 6);
    assert_eq!(pdf.ids().collect::<Vec<_>>(), [(1, 0), (3, 2)]);
    assert_eq!(pdf.get((3, 2)).unwrap(), &Object::Integer(3));
}

#[test]
fn appended_runs_free_deleted_objects_with_the_next_generation() {
    let mut out = file(&[(2, Object::Integer(2)), (3, Object::Integer(3))]);
    let base = open(&out);
    let prev = base.xref().start;
    let mut section = Section::append(base.xref().size, 0);
    let marked = Object::Dictionary(dictionary! { "Type" => "Catalog", "Marked" => true });
    let one = object::indirect(&mut out, (1, 0), &marked).unwrap();
    section.insert(
        1,
        Entry::InUse {
            pos: one,
            generation: 0,
        },
    );
    section.insert(3, Entry::Free { generation: 1 });
    let five = object::indirect(&mut out, (5, 0), &Object::Integer(5)).unwrap();
    section.insert(
        5,
        Entry::InUse {
            pos: five,
            generation: 0,
        },
    );
    let pos = out.len();
    let trailer = dictionary! { "Root" => (1, 0), "Prev" => prev as i64 };
    section.table(&mut out, trailer, pos).unwrap();
    let want = format!(
        "xref\n0 2\n\
         0000000003 65535 f \n\
         {one:010} 00000 n \n\
         3 1\n\
         0000000000 00001 f \n\
         5 1\n\
         {five:010} 00000 n \n\
         trailer\n<</Root 1 0 R/Prev {prev}/Size 6>>\nstartxref\n{pos}\n%%EOF\n"
    );
    assert_eq!(String::from_utf8_lossy(&out[pos..]), want);
    // lopdf's readers pass over free entries, so object 3 still reads from
    // the earlier revision there: PDFium and qpdf take it out.
    let pdf = open(&out);
    assert!(pdf.base_sound());
    assert_eq!(pdf.xref().sections, 2);
    assert_eq!(pdf.xref().size, 6);
    assert_eq!(pdf.get((1, 0)).unwrap(), &marked);
    assert_eq!(pdf.get((2, 0)).unwrap(), &Object::Integer(2));
    assert_eq!(pdf.get((5, 0)).unwrap(), &Object::Integer(5));
}

#[test]
fn positions_count_from_the_header() {
    let mut out = vec![b'~'; 100];
    out.extend(file(&[(2, Object::Integer(2))]));
    let base = open(&out);
    assert_eq!(base.xref().header_offset, 100);
    let mut section = Section::append(base.xref().size, 100);
    let pos = object::indirect(&mut out, (2, 0), &Object::Integer(22)).unwrap();
    section.insert(2, Entry::InUse { pos, generation: 0 });
    let pos = out.len();
    let trailer = dictionary! { "Root" => (1, 0), "Prev" => base.xref().start as i64 };
    section.table(&mut out, trailer, pos).unwrap();
    assert!(out.ends_with(format!("startxref\n{}\n%%EOF\n", pos - 100).as_bytes()));
    let pdf = open(&out);
    assert!(pdf.base_sound());
    assert_eq!(pdf.get((2, 0)).unwrap(), &Object::Integer(22));
}

#[test]
fn a_table_refuses_offsets_past_ten_digits() {
    let far = 10_000_000_000;
    let mut section = Section::full(0);
    section.insert(
        1,
        Entry::InUse {
            pos: far,
            generation: 0,
        },
    );
    let mut out = b"%PDF-1.7\n".to_vec();
    assert_eq!(
        section.table(&mut out, dictionary! {}, far + 10),
        Err(Error::TooFar(far as u64))
    );
    assert_eq!(out, b"%PDF-1.7\n", "nothing written");
}

/// Cross-reference stream `id`'s `/Index` and its rows, each `[type,
/// field, generation]`, read by its `/W`. A full write's, which has no
/// `/Prev`, is deflated under the PNG Up predictor, a column for each
/// byte of a row; an append's is uncompressed.
fn rows(pdf: &LazyPdf, id: ObjectId) -> (Vec<i64>, Vec<i64>, Vec<[u64; 3]>) {
    let s = pdf.get(id).unwrap().as_stream().unwrap();
    assert_eq!(
        s.dict.get(b"Type").unwrap(),
        &Object::Name(b"XRef".to_vec())
    );
    let ints = |key: &[u8]| -> Vec<i64> {
        let array = s.dict.get(key).unwrap().as_array().unwrap();
        array.iter().map(|o| o.as_i64().unwrap()).collect()
    };
    let (index, w) = (ints(b"Index"), ints(b"W"));
    let widths: Vec<usize> = w.iter().map(|&n| n as usize).collect();
    let columns: usize = widths.iter().sum();
    if s.dict.has(b"Prev") {
        assert!(!s.dict.has(b"Filter"), "an update's is uncompressed");
        assert!(!s.dict.has(b"DecodeParms"), "an update's has no predictor");
    } else {
        let flate = Object::Name(b"FlateDecode".to_vec());
        assert_eq!(s.dict.get(b"Filter").unwrap(), &flate);
        let parms = s.dict.get(b"DecodeParms").unwrap().as_dict().unwrap();
        assert_eq!(parms.get(b"Predictor").unwrap(), &Object::Integer(12));
        assert_eq!(
            parms.get(b"Columns").unwrap(),
            &Object::Integer(columns as i64)
        );
    }
    let rows = s
        .decompressed_content()
        .unwrap()
        .chunks(columns)
        .map(|row| {
            let mut fields = [0; 3];
            let mut at = 0;
            for (field, width) in fields.iter_mut().zip(&widths) {
                *field = row[at..at + width]
                    .iter()
                    .fold(0, |n, &b| n << 8 | u64::from(b));
                at += width;
            }
            fields
        })
        .collect();
    (index, w, rows)
}

#[test]
fn an_xref_stream_lists_its_index() {
    // A full write: three objects, and the stream as 4.
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut section = Section::full(0);
    let mut at = Vec::new();
    for (num, obj) in [(1, catalog()), (2, 2.into()), (3, 3.into())] {
        let pos = object::indirect(&mut out, (num, 0), &obj).unwrap();
        section.insert(num, Entry::InUse { pos, generation: 0 });
        at.push(pos as u64);
    }
    let pos = out.len();
    section
        .stream(&mut out, 4, dictionary! { "Root" => (1, 0) }, pos)
        .unwrap();
    assert!(out.ends_with(format!("endobj\nstartxref\n{pos}\n%%EOF\n").as_bytes()));
    let base = open(&out);
    assert!(base.base_sound());
    assert_eq!(base.xref().kind, XrefKind::Stream);
    assert_eq!(base.xref().size, 5);
    let (index, w, got) = rows(&base, (4, 0));
    assert_eq!(index, [0, 5]);
    // As wide as the largest offset, the stream's own, needs.
    assert_eq!(w, [1, if pos < 256 { 1 } else { 2 }, 2]);
    let want = [
        [0, 0, 65535],
        [1, at[0], 0],
        [1, at[1], 0],
        [1, at[2], 0],
        [1, pos as u64, 0],
    ];
    assert_eq!(got, want);

    // An append: 2 rewritten, 3 deleted, and the stream as 5.
    let mut section = Section::append(base.xref().size, 0);
    let two = object::indirect(&mut out, (2, 0), &22.into()).unwrap();
    section.insert(
        2,
        Entry::InUse {
            pos: two,
            generation: 0,
        },
    );
    section.insert(3, Entry::Free { generation: 1 });
    let pos = out.len();
    let trailer = dictionary! { "Root" => (1, 0), "Prev" => base.xref().start as i64 };
    section.stream(&mut out, 5, trailer, pos).unwrap();
    let pdf = open(&out);
    assert!(pdf.base_sound());
    assert_eq!(pdf.xref().kind, XrefKind::Stream);
    assert_eq!(pdf.xref().sections, 2);
    assert_eq!(pdf.xref().size, 6);
    let (index, _, got) = rows(&pdf, (5, 0));
    assert_eq!(index, [0, 1, 2, 2, 5, 1]);
    let want = [
        [0, 3, 65535],
        [1, two as u64, 0],
        [0, 0, 1],
        [1, pos as u64, 0],
    ];
    assert_eq!(got, want);
    assert_eq!(pdf.get((2, 0)).unwrap(), &Object::Integer(22));
}

/// A full write's cross-reference stream is deflated under the PNG Up
/// predictor, into fewer bytes than its rows, and sheaf's lazy reader and
/// lopdf's eager one read it back.
#[test]
fn a_full_xref_stream_is_compressed_with_the_up_predictor() {
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut section = Section::full(0);
    let mut at = Vec::new();
    for num in 1..=200 {
        let obj = if num == 1 { catalog() } else { num.into() };
        let pos = object::indirect(&mut out, (num, 0), &obj).unwrap();
        section.insert(num, Entry::InUse { pos, generation: 0 });
        at.push(pos as u64);
    }
    let pos = out.len();
    section
        .stream(&mut out, 201, dictionary! { "Root" => (1, 0) }, pos)
        .unwrap();
    let pdf = open(&out);
    assert!(pdf.base_sound());
    assert_eq!(pdf.xref().size, 202);
    let (index, w, got) = rows(&pdf, (201, 0));
    assert_eq!(index, [0, 202]);
    assert_eq!(w, [1, 2, 2]);
    let want: Vec<[u64; 3]> = std::iter::once([0, 0, 65535])
        .chain(at.iter().map(|&pos| [1, pos, 0]))
        .chain([[1, pos as u64, 0]])
        .collect();
    assert_eq!(got, want);
    let stored = pdf.get((201, 0)).unwrap().as_stream().unwrap();
    assert!(
        stored.content.len() < got.len() * 5,
        "{} bytes for {} rows of 5",
        stored.content.len(),
        got.len()
    );
    let eager = Pdf::load_mem(&out).unwrap();
    for num in 2..=200 {
        assert_eq!(pdf.get((num, 0)).unwrap(), &Object::Integer(num.into()));
        let obj = eager.get_object((num, 0)).unwrap();
        assert_eq!(obj, &Object::Integer(num.into()));
    }
}

/// `members`, each its number and its object, as direct syntax.
fn as_members(members: &[(u32, Object)]) -> Vec<(u32, Vec<u8>)> {
    (members.iter())
        .map(|(num, obj)| {
            let mut bytes = Vec::new();
            object::write(&mut bytes, obj).unwrap();
            (*num, bytes)
        })
        .collect()
}

/// An object stream holding `members`, in order.
fn container(members: &[(u32, Object)]) -> Object {
    Object::Stream(objstm::container(&as_members(members)).unwrap())
}

/// Every exclusion of 7.5.7 and decision 3: streams (a cross-reference
/// stream's among them), a generation past 0, an encryption dictionary,
/// the linearization dictionary and a signature dictionary, told as the
/// encryption walk tells one. Everything else packs, a signature field's
/// widget with it: its `/Type /Annot` comes before its `/FT /Sig`.
#[test]
fn packable_leaves_out_what_iso_7_5_7_does() {
    let dict = |d: Dictionary| Object::Dictionary(d);
    let stream = |d: Dictionary| Object::Stream(Stream::new(d, b"data".to_vec()));
    let bytes = |b: &[u8]| Object::String(b.to_vec(), StringFormat::Hexadecimal);
    let never = [
        ("a stream", stream(dictionary! {})),
        (
            "a cross-reference stream",
            stream(dictionary! { "Type" => "XRef" }),
        ),
        (
            "an object stream",
            stream(dictionary! { "Type" => "ObjStm" }),
        ),
        (
            "an encryption dictionary",
            dict(dictionary! {
                "Filter" => "Standard",
                "V" => 2,
                "O" => bytes(b"o"),
                "U" => bytes(b"u"),
            }),
        ),
        (
            "the linearization dictionary",
            dict(dictionary! { "Linearized" => 1, "L" => 1000 }),
        ),
        (
            "a signature",
            dict(dictionary! { "Type" => "Sig", "Filter" => "Adobe.PPKLite" }),
        ),
        (
            "a time stamp",
            dict(dictionary! { "Type" => "DocTimeStamp" }),
        ),
        ("a signature by /FT", dict(dictionary! { "FT" => "Sig" })),
        (
            "a signature without /Type",
            dict(dictionary! {
                "ByteRange" => vec![0.into(), 10.into()],
                "Contents" => bytes(b"sig"),
            }),
        ),
    ];
    for (what, obj) in &never {
        assert!(!objstm::packable((5, 0), obj), "{what}");
    }
    let catalog = catalog();
    assert!(objstm::packable((5, 0), &catalog));
    assert!(!objstm::packable((5, 1), &catalog), "generation 1");
    let packs = [
        ("a catalog", catalog),
        ("an integer", Object::Integer(7)),
        ("null", Object::Null),
        ("a string", bytes(b"text")),
        (
            "an array",
            Object::Array(vec![(2, 0).into(), Object::Real(1.5)]),
        ),
        ("a reference", Object::Reference((3, 0))),
        (
            "a signature field's widget",
            dict(dictionary! {
                "Type" => "Annot",
                "Subtype" => "Widget",
                "FT" => "Sig",
                "V" => (9, 0),
            }),
        ),
        (
            "a button field",
            dict(dictionary! { "FT" => "Btn", "V" => (9, 0) }),
        ),
        (
            "a /Filter name alone",
            dict(dictionary! { "Filter" => "Standard", "O" => bytes(b"o") }),
        ),
        (
            "/Contents without /ByteRange",
            dict(dictionary! { "Contents" => bytes(b"text") }),
        ),
    ];
    for (what, obj) in &packs {
        assert!(objstm::packable((5, 0), obj), "{what}");
    }
}

/// A container's header gives each member's number and where it starts,
/// counted from `/First`, each member followed by a newline so numbers
/// side by side don't run together; it's deflated, lopdf's reader reads
/// every member back, and its `/Length` is written direct.
#[test]
fn a_container_lists_each_member_where_it_is() {
    let members = [
        (4, Object::Integer(12)),
        (9, Object::Integer(13)),
        (2, Object::Name(b"Name".to_vec())),
        (
            3,
            Object::String(b"two\nlines (".to_vec(), StringFormat::Literal),
        ),
        (
            7,
            Object::Dictionary(dictionary! { "Kids" => vec![(4, 0).into(), Object::Null] }),
        ),
        (8, Object::Real(-0.5)),
        (11, Object::Boolean(true)),
    ];
    let bytes = as_members(&members);
    let stream = objstm::container(&bytes).unwrap();
    let ints = |key: &[u8]| stream.dict.get(key).unwrap().as_i64().unwrap();
    assert_eq!(stream.dict.get_type().unwrap(), b"ObjStm");
    assert_eq!(
        stream.dict.get(b"Filter").unwrap(),
        &Object::Name(b"FlateDecode".to_vec())
    );
    assert_eq!(ints(b"N"), 7);
    let data = stream.decompressed_content().unwrap();
    let first = ints(b"First") as usize;
    let header = std::str::from_utf8(&data[..first]).unwrap();
    assert!(header.ends_with('\n'), "{header:?}");
    let pairs: Vec<usize> = (header.split_whitespace())
        .map(|n| n.parse().unwrap())
        .collect();
    assert_eq!(pairs.len(), 14);
    for (pair, (num, member)) in pairs.chunks(2).zip(&bytes) {
        assert_eq!(pair[0], *num as usize);
        let at = first + pair[1];
        assert_eq!(&data[at..at + member.len()], member.as_slice());
        assert_eq!(data[at + member.len()], b'\n');
    }
    assert_eq!(
        data.len(),
        first + bytes.iter().map(|(_, b)| b.len() + 1).sum::<usize>()
    );
    let read = lopdf::ObjectStream::new(&stream).unwrap();
    assert_eq!(read.objects.len(), 7);
    for (num, obj) in &members {
        assert_eq!(&read.objects[&(*num, 0)], obj, "object {num}");
    }

    let mut out = Vec::new();
    object::indirect(&mut out, (20, 0), &Object::Stream(stream.clone())).unwrap();
    let length = format!("/Length {}>>", stream.content.len());
    assert!(
        out.windows(length.len()).any(|w| w == length.as_bytes()),
        "{}",
        String::from_utf8_lossy(&out[..out.len().min(80)])
    );
    let reread = open(&file(&[(20, Object::Stream(stream))]));
    let reread = reread.get((20, 0)).unwrap().as_stream().unwrap();
    assert_eq!(
        reread.dict.get(b"Length").unwrap(),
        &Object::Integer(reread.content.len() as i64)
    );
}

/// A container's data may be as large as the reader decompresses, and no
/// larger: one a byte past it isn't made, told before anything is copied
/// or deflated (the member here is zeroed memory nothing touches).
#[test]
fn a_container_past_what_the_reader_takes_isn_t_made() {
    assert!(objstm::fits(objstm::MAX_DATA));
    assert!(!objstm::fits(objstm::MAX_DATA + 1));
    assert_eq!(objstm::MAX_DATA, Limits::DEFAULT.reader.max_decompressed);
    // "1 0\n", then the member and its newline: one byte too many.
    let member = vec![0; objstm::MAX_DATA - 4];
    assert!(objstm::container(&[(1, member)]).is_none());
}

/// Objects go into containers in Annex F's parts: page 1's, apart, then
/// a single later page's by page, the shared, and the rest, numbers
/// ascending within each, at most 100 to a container.
#[test]
fn the_grouping_keeps_page_one_apart_and_caps_containers() {
    use group::{MEMBERS, Use, group};
    let mut objects: Vec<(u32, Use)> = Vec::new();
    objects.extend((1..=150).map(|n| (n, Use::Page(0))));
    objects.extend((300..310).map(|n| (n, Use::Page(2))));
    objects.extend((400..420).map(|n| (n, Use::Page(1))));
    objects.extend((500..600).map(|n| (n, Use::Shared)));
    objects.extend((600..651).map(|n| (n, Use::None)));
    objects.reverse();
    let got = group(objects);
    let range = |r: std::ops::Range<u32>| r.collect::<Vec<_>>();
    let rest: Vec<u32> = [
        range(400..420),
        range(300..310),
        range(500..600),
        range(600..651),
    ]
    .concat();
    let want = vec![
        range(1..101),
        range(101..151),
        rest[..100].to_vec(),
        rest[100..].to_vec(),
    ];
    assert_eq!(got, want);
    assert!(got.iter().all(|c| c.len() <= MEMBERS));

    // Page 1's part filling its container exactly still ends it, and a
    // file with nothing only page 1 uses starts with the next part.
    let exactly: Vec<(u32, Use)> = (1..=100)
        .map(|n| (n, Use::Page(0)))
        .chain([(101, Use::Shared)])
        .collect();
    assert_eq!(group(exactly), [range(1..101), vec![101]]);
    assert_eq!(group([(7, Use::None), (5, Use::Page(3))]), [vec![5, 7]]);
    assert_eq!(group([]), Vec::<Vec<u32>>::new());
}

/// An object in an object stream is listed as its container's number and
/// its place there (7.5.8.3's type 2), and the readers find it: in a full
/// write, and in an update after bytes before the header, which a
/// container's number isn't counted from.
#[test]
fn a_type_2_entry_is_written_as_its_container_and_place() {
    let five = Object::String(b"five".to_vec(), StringFormat::Literal);
    let six = Object::Dictionary(dictionary! { "Six" => 6 });
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut section = Section::full(0);
    let objects = [
        (1, catalog()),
        (7, container(&[(5, five.clone()), (6, six.clone())])),
    ];
    let mut at = Vec::new();
    for (num, obj) in objects {
        let pos = object::indirect(&mut out, (num, 0), &obj).unwrap();
        section.insert(num, Entry::InUse { pos, generation: 0 });
        at.push(pos as u64);
    }
    for (num, index) in [(5, 0), (6, 1)] {
        section.insert(
            num,
            Entry::Packed {
                container: 7,
                index,
            },
        );
    }
    let pos = out.len();
    section
        .stream(&mut out, 8, dictionary! { "Root" => (1, 0) }, pos)
        .unwrap();
    let base = open(&out);
    assert!(base.base_sound());
    let (index, _, got) = rows(&base, (8, 0));
    assert_eq!(index, [0, 9]);
    let want = [
        [0, 2, 65535],
        [1, at[0], 0],
        [0, 3, 0],
        [0, 4, 0],
        [0, 0, 0],
        [2, 7, 0],
        [2, 7, 1],
        [1, at[1], 0],
        [1, pos as u64, 0],
    ];
    assert_eq!(got, want);
    let eager = Pdf::load_mem(&out).unwrap();
    for (id, obj) in [((5, 0), &five), ((6, 0), &six)] {
        assert_eq!(base.get(id).unwrap(), obj);
        assert_eq!(eager.get_object(id).unwrap(), obj);
    }

    // An update to the file with 100 bytes before its header: 6 again,
    // in container 10.
    let mut out = [vec![b'~'; 100], out].concat();
    let base = open(&out);
    assert_eq!(base.xref().header_offset, 100);
    let mut section = Section::append(base.xref().size, 100);
    let sixty = Object::Dictionary(dictionary! { "Six" => 60 });
    let pos = object::indirect(&mut out, (10, 0), &container(&[(6, sixty.clone())])).unwrap();
    section.insert(10, Entry::InUse { pos, generation: 0 });
    section.insert(
        6,
        Entry::Packed {
            container: 10,
            index: 0,
        },
    );
    let pos = out.len();
    let trailer = dictionary! { "Root" => (1, 0), "Prev" => base.xref().start as i64 };
    section.stream(&mut out, 11, trailer, pos).unwrap();
    let pdf = open(&out);
    assert!(pdf.base_sound());
    assert_eq!(pdf.xref().sections, 2);
    let (index, _, got) = rows(&pdf, (11, 0));
    assert_eq!(index, [6, 1, 10, 2]);
    assert_eq!(got[0], [2, 10, 0]);
    assert_eq!(pdf.get((5, 0)).unwrap(), &five);
    assert_eq!(pdf.get((6, 0)).unwrap(), &sixty);
    let eager = Pdf::load_mem(&out).unwrap();
    assert_eq!(eager.get_object((6, 0)).unwrap(), &sixty);
}

/// A table can't list an object stream's member: it's refused, and
/// nothing is written.
#[test]
fn a_table_refuses_a_packed_entry() {
    let mut section = Section::full(0);
    section.insert(
        1,
        Entry::InUse {
            pos: 9,
            generation: 0,
        },
    );
    section.insert(
        5,
        Entry::Packed {
            container: 7,
            index: 0,
        },
    );
    let mut out = b"%PDF-1.7\n".to_vec();
    assert_eq!(
        section.table(&mut out, dictionary! { "Root" => (1, 0) }, 20),
        Err(Error::Packed(5))
    );
    assert_eq!(out, b"%PDF-1.7\n", "nothing written");
    assert_eq!(
        Error::Packed(5).to_string(),
        "a cross-reference table can't list object 5, which is in an object stream"
    );
}

/// How `bytes`, opened with the user password, are encrypted.
fn crypt_of(bytes: &[u8]) -> Crypt {
    Crypt::of(&open_as_user(bytes).unwrap()).unwrap().unwrap()
}

/// What object `id` of a test's file is keyed as: the object of a
/// source it is.
fn key_of(id: ObjectId) -> crate::materialize::ObjKey {
    crate::materialize::ObjKey::Base {
        blob: crate::store::BlobId { hash: 0, len: 0 },
        obj: id,
        copy: 0,
    }
}

/// `plain`'s objects encrypted through `crypt` and written by the
/// serializer, with `crypt`'s `/Encrypt`, where its source keeps it, and
/// `plain`'s `/ID`, its source's: with a cross-reference table, or with a
/// stream.
fn encrypted_by(crypt: &Crypt, plain: &Pdf, kind: XrefKind) -> Vec<u8> {
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut section = Section::full(0);
    let mut put = |out: &mut Vec<u8>, id: ObjectId, obj: &Object| {
        let pos = object::indirect(out, id, obj).unwrap();
        let generation = id.1;
        section.insert(id.0, Entry::InUse { pos, generation });
    };
    for (&id, obj) in &plain.objects {
        put(
            &mut out,
            id,
            &crypt.encrypt_as(id, &key_of(id), obj).unwrap(),
        );
    }
    let at = crypt.at();
    put(&mut out, at, &Object::Dictionary(crypt.dict().clone()));
    let mut trailer = plain.trailer.clone();
    trailer.set("Encrypt", at);
    let pos = out.len();
    match kind {
        XrefKind::Table => section.table(&mut out, trailer, pos).unwrap(),
        XrefKind::Stream => section.stream(&mut out, at.0 + 1, trailer, pos).unwrap(),
    }
    out
}

#[test]
fn objects_encrypt_as_the_lazy_reader_decrypts() {
    let plain = plaintext();
    let mut states = rc4_aes_128_and_aes_256(&plain).to_vec();
    states.push(("attachment only", attachment_only(&plain)));
    for (name, state) in states {
        let strings = state.get_string_filter().unwrap().method() != b"Identity";
        let [source, _] = encrypted(&plain, &state, |_| {});
        let crypt = crypt_of(&source);
        for kind in [XrefKind::Table, XrefKind::Stream] {
            let bytes = encrypted_by(&crypt, &plain, kind);
            let name = format!("{name}, {kind:?}");
            if strings {
                // Every string and stream is encrypted, those in a stream's
                // dictionary too, but a signature's /Contents.
                for text in SECRETS {
                    assert!(
                        !holds(&bytes, text),
                        "{name}: {:?}",
                        String::from_utf8_lossy(text)
                    );
                }
            } else {
                // Only the attachment's data is.
                assert!(!holds(&bytes, ATTACHMENT), "{name}");
                for text in SECRETS.into_iter().filter(|&text| text != ATTACHMENT) {
                    assert!(
                        holds(&bytes, text),
                        "{name}: {:?}",
                        String::from_utf8_lossy(text)
                    );
                }
            }
            assert!(holds(&bytes, SIGNATURE_WRITTEN), "{name}");
            // Each string in its own format.
            let lazy = assert_decrypts_to(&plain, &bytes, &name);
            // Its /Encrypt as stored, /P and all, and its /ID[0].
            let back = Crypt::of(&lazy).unwrap().unwrap();
            assert_eq!(back.dict(), crypt.dict(), "{name}");
            assert_eq!(back.first_id(), crypt.first_id(), "{name}");
        }
    }
}

/// The same objects encrypt to the same bytes, twice and on two threads.
#[test]
fn objects_encrypt_alike_every_time() {
    let plain = plaintext();
    for (name, state) in rc4_aes_128_and_aes_256(&plain) {
        let [source, _] = encrypted(&plain, &state, |_| {});
        let crypt = crypt_of(&source);
        let encrypt = |id: ObjectId, obj: &Object| crypt.encrypt_as(id, &key_of(id), obj).unwrap();
        // Every object, twice, and on two threads.
        let once: Vec<Object> = plain
            .objects
            .iter()
            .map(|(&id, obj)| encrypt(id, obj))
            .collect();
        let again = std::thread::scope(|scope| {
            let threads: Vec<_> = (0..2)
                .map(|_| {
                    scope.spawn(|| {
                        let all = plain.objects.iter().map(|(&id, obj)| encrypt(id, obj));
                        all.collect::<Vec<Object>>()
                    })
                })
                .collect();
            threads
                .into_iter()
                .map(|t| t.join().unwrap())
                .collect::<Vec<_>>()
        });
        assert_eq!(again, [once.clone(), once], "{name}");
    }
}

/// What the materializer writes is keyed by what it is: an AES string's
/// IV is the same for the same key and value wherever it's written, and
/// another for another key or another value (M10.5 §11.7).
#[test]
fn ivs_are_keyed_by_what_the_object_is() {
    use crate::materialize::ObjKey;
    let plain = plaintext();
    let text = |s: &str| Object::String(s.as_bytes().to_vec(), StringFormat::Literal);
    for (name, state) in rc4_aes_128_and_aes_256(&plain).into_iter().skip(1) {
        let [source, _] = encrypted(&plain, &state, |_| {});
        let crypt = crypt_of(&source);
        let iv =
            |id: ObjectId, key: ObjKey, value: &str| match crypt.encrypt_as(id, &key, &text(value))
            {
                Ok(Object::String(bytes, _)) => bytes[..16].to_vec(),
                other => panic!("{name}: {other:?}"),
            };
        let first = iv((5, 0), ObjKey::Catalog, "a value");
        assert_eq!(iv((5, 0), ObjKey::Catalog, "a value"), first, "{name}");
        assert_eq!(iv((6, 0), ObjKey::Catalog, "a value"), first, "{name}");
        for other in [
            iv((5, 0), ObjKey::Labels, "a value"),
            iv((5, 0), ObjKey::Catalog, "another value"),
            iv((5, 0), ObjKey::PagesNode(0), "a value"),
        ] {
            assert_ne!(other, first, "{name}");
        }
        // Decrypted as the reader decrypts: by the number it's at.
        let id = (5, 0);
        let mut back = crypt
            .encrypt_as(id, &ObjKey::Catalog, &text("a value"))
            .unwrap();
        lopdf::encryption::decrypt_object(crypt.state(), id, &mut back).unwrap();
        assert_eq!(back, text("a value"), "{name}");
    }
}

#[test]
fn xref_streams_metadata_and_the_encrypt_dict_stay_plain() {
    let plain = plaintext();
    let state = EncryptionState::try_from(EncryptionVersion::V4 {
        document: &plain,
        encrypt_metadata: false,
        crypt_filters: BTreeMap::from([(
            b"StdCF".to_vec(),
            Arc::new(Aes128CryptFilter) as Arc<dyn CryptFilter>,
        )]),
        stream_filter: b"StdCF".to_vec(),
        string_filter: b"StdCF".to_vec(),
        owner_password: OWNER,
        user_password: USER,
        permissions: Permissions::all(),
    })
    .unwrap();
    let [source, _] = encrypted(&plain, &state, |_| {});
    let crypt = crypt_of(&source);
    let stays = |id: ObjectId, obj: &Object| {
        assert_eq!(&crypt.encrypt_as(id, &key_of(id), obj).unwrap(), obj)
    };
    let xref = Stream::new(
        dictionary! {
            "Type" => "XRef",
            "ID" => vec![Object::string_literal("an /ID"), Object::string_literal("an /ID")],
        },
        vec![1, 0, 0, 0],
    );
    stays((20, 0), &Object::Stream(xref));
    let at = crypt.at();
    let encrypt = Object::Dictionary(crypt.dict().clone());
    stays(at, &encrypt);
    // An earlier revision's /Encrypt too.
    stays((21, 0), &encrypt);
    let (&metadata, xmp) = plain
        .objects
        .iter()
        .find(|(_, obj)| obj.type_name().ok() == Some(b"Metadata"))
        .unwrap();
    stays(metadata, xmp);
    // A signature's /Contents, but not its other strings.
    let (&sig, signature) = plain
        .objects
        .iter()
        .find(|(_, obj)| obj.type_name().ok() == Some(b"Sig"))
        .unwrap();
    let encrypted = crypt.encrypt_as(sig, &key_of(sig), signature).unwrap();
    let (signature, encrypted) = (signature.as_dict().unwrap(), encrypted.as_dict().unwrap());
    assert_eq!(
        encrypted.get(b"Contents").unwrap(),
        signature.get(b"Contents").unwrap()
    );
    assert_ne!(encrypted.get(b"M").unwrap(), signature.get(b"M").unwrap());

    // Written, the metadata is in the clear and reads as it was, and the
    // cross-reference stream's /ID is as written.
    for kind in [XrefKind::Table, XrefKind::Stream] {
        let bytes = encrypted_by(&crypt, &plain, kind);
        assert!(holds(&bytes, XMP), "{kind:?}");
        assert_decrypts_to(&plain, &bytes, &format!("metadata in the clear, {kind:?}"));
    }
}

/// The cross-reference kinds [`encrypted`] writes its two files in.
const KINDS: [XrefKind; 2] = [XrefKind::Table, XrefKind::Stream];

/// `bytes`, opened with the user password as the app opens a file, its
/// outline and form read, as a document whose first page is turned a
/// quarter.
fn turned(bytes: &[u8]) -> Document {
    let store = Store::new();
    let kind = BlobKind::file(Path::new("encrypted.pdf"), Some(USER));
    let src = store.open(Arc::from(bytes), kind).unwrap();
    let policy = Box::new(FixedCount(10));
    let mut doc = crate::open::start(store, src.clone(), "encrypted.pdf".into(), None, policy);
    doc.ready(crate::open::finish(&src, &NoProgress)).unwrap();
    let first = doc.page(0).unwrap();
    let turn = crate::tx::fixture::rotate(&[first], sheaf_edit::Turn::Quarter);
    doc.commit_ops(vec![turn], "turn").unwrap();
    doc
}

/// What `path`, key by key, reaches from `obj` in `pdf`, each reference
/// followed.
fn at(pdf: &LazyPdf, obj: &Object, path: &[&[u8]]) -> Object {
    let mut obj = pdf.dereference(obj).unwrap().1.clone();
    for key in path {
        let next = obj.as_dict().unwrap().get(key).unwrap();
        obj = pdf.dereference(next).unwrap().1.clone();
    }
    obj
}

/// A rewrite of [`plaintext`], turned: its first page's `/Rotate`, and
/// what it holds where the fixture put it: the Info title, the page's
/// private data, the XMP and the attachment's data.
fn rewritten(pdf: &LazyPdf) -> (i64, [Vec<u8>; 4]) {
    let trailer = Object::Dictionary(pdf.trailer().clone());
    let catalog = at(pdf, &trailer, &[b"Root"]);
    let page = at(pdf, &catalog, &[b"Pages", b"Kids"]).as_array().unwrap()[0].clone();
    let files = at(pdf, &catalog, &[b"Names", b"EmbeddedFiles", b"Names"]);
    let text = |obj: Object| obj.as_str().unwrap().to_vec();
    let data = |obj: Object| obj.as_stream().unwrap().content.clone();
    let held = [
        text(at(pdf, &trailer, &[b"Info", b"Title"])),
        text(at(pdf, &page, &[b"PieceInfo", b"Sheaf", b"Private"])),
        data(at(pdf, &catalog, &[b"Metadata"])),
        data(at(pdf, &files.as_array().unwrap()[1], &[b"EF", b"F"])),
    ];
    (at(pdf, &page, &[b"Rotate"]).as_i64().unwrap(), held)
}

/// What [`rewritten`] gives of every rewrite of [`plaintext`].
fn as_written() -> (i64, [Vec<u8>; 4]) {
    (
        90,
        [TITLE, PAGE_STRING, XMP, ATTACHMENT].map(<[u8]>::to_vec),
    )
}

/// `bytes` opened with the user password.
fn open_as_user(bytes: &[u8]) -> lopdf::Result<LazyPdf> {
    LazyPdf::open(
        Arc::<[u8]>::from(bytes),
        Some(USER),
        &Limits::DEFAULT.reader,
    )
}

/// [`plaintext`] as it is, and with its `/ID` one string, each with
/// its name.
fn plaintexts() -> [(&'static str, Pdf); 2] {
    [
        ("/ID of two", plaintext()),
        ("/ID of one", one_id(plaintext())),
    ]
}

/// An encrypted file rewrites encrypted every way, packed when its base
/// has a cross-reference stream: nothing is in the clear, even inflated.
#[test]
fn rewrites_stay_encrypted() {
    for (ids, plain) in plaintexts() {
        let states = std::iter::once(rc4_40(&plain)).chain(rc4_aes_128_and_aes_256(&plain));
        for (name, state) in states {
            for (source, kind) in encrypted(&plain, &state, |_| {}).iter().zip(KINDS) {
                let name = format!("{name}, {kind:?}, {ids}");
                let bytes = crate::save::rewrite(&turned(source)).unwrap();
                for text in SECRETS {
                    assert!(
                        !in_clear(&bytes, text),
                        "{name}: {:?}",
                        String::from_utf8_lossy(text)
                    );
                }
                assert!(
                    matches!(
                        LazyPdf::open(Arc::<[u8]>::from(&bytes[..]), None, &Limits::DEFAULT.reader),
                        Err(lopdf::Error::InvalidPassword)
                    ),
                    "{name}"
                );
                let pdf = open_as_user(&bytes).unwrap();
                assert!(pdf.base_sound(), "{name}");
                assert_eq!(pdf.xref().kind, kind, "{name}");
                let packs = kind == XrefKind::Stream;
                assert_eq!(!object_streams_of(&pdf).is_empty(), packs, "{name}");
                assert_eq!(rewritten(&pdf), as_written(), "{name}");
                // The source's /Encrypt as stored, its /V, /R and /P with it,
                // and its /ID[0].
                let (was, now) = (crypt_of(source), Crypt::of(&pdf).unwrap().unwrap());
                assert_eq!(now.dict(), was.dict(), "{name}");
                assert!(was.first_id().is_some(), "{name}");
                assert_eq!(now.first_id(), was.first_id(), "{name}");
                // Two strings, whatever the source had.
                assert!(pdf.xref().id.is_some(), "{name}");
            }
        }
    }
}

/// [`in_clear`] finds a string written in hex, in either case of digit,
/// on its own or in an object stream's member, deflated; not a string
/// that differs.
#[test]
fn in_clear_finds_a_string_written_in_hex() {
    let file = |format, case: fn(&mut Vec<u8>)| {
        let mut member = Vec::new();
        let value = Object::String(b"Ann Lee".to_vec(), format);
        object::write(
            &mut member,
            &Object::Dictionary(dictionary! { "V" => value }),
        )
        .unwrap();
        case(&mut member);
        let container = objstm::container(&[(1, member.clone())]).unwrap();
        let mut packed = Vec::new();
        object::indirect(&mut packed, (2, 0), &Object::Stream(container)).unwrap();
        [member, packed]
    };
    let [plain, packed] = file(StringFormat::Hexadecimal, |_| {});
    assert!(holds(&plain, b"<416E6E204C6565>"));
    assert!(!holds(&packed, b"416E6E204C6565"));
    let [lower, lower_packed] = file(StringFormat::Hexadecimal, |m| m.make_ascii_lowercase());
    for bytes in [plain, packed, lower, lower_packed] {
        assert!(in_clear(&bytes, b"Ann Lee"));
        assert!(!in_clear(&bytes, b"Ann Lea"));
    }
    let [literal, packed] = file(StringFormat::Literal, |_| {});
    assert!(in_clear(&literal, b"Ann Lee") && in_clear(&packed, b"Ann Lee"));
}

/// A file whose `/EFF` encrypts its attachments alone rewrites with only
/// its attachment encrypted, packed when its base has a cross-reference
/// stream: every other string and stream is in the clear, if deflated in
/// a container.
#[test]
fn an_eff_file_rewrites_with_only_its_attachment_encrypted() {
    let plain = plaintext();
    for (source, kind) in encrypted(&plain, &attachment_only(&plain), |_| {})
        .iter()
        .zip(KINDS)
    {
        let bytes = crate::save::rewrite(&turned(source)).unwrap();
        assert!(!in_clear(&bytes, ATTACHMENT), "{kind:?}");
        for text in [TITLE, PAGE_STRING, FORM_STRING, CONTENT, XMP] {
            assert!(
                in_clear(&bytes, text),
                "{kind:?}: {:?}",
                String::from_utf8_lossy(text)
            );
        }
        let pdf = open_as_user(&bytes).unwrap();
        let packed = !object_streams_of(&pdf).is_empty();
        assert_eq!(packed, kind == XrefKind::Stream, "{kind:?}");
        assert_eq!(rewritten(&pdf), as_written(), "{kind:?}");
        // Its /Encrypt as stored, /EFF and all.
        let now = Crypt::of(&pdf).unwrap().unwrap();
        assert_eq!(now.dict(), crypt_of(source).dict(), "{kind:?}");
        assert!(now.dict().has(b"EFF"), "{kind:?}");
    }
}

/// A file of [`form`]'s, as `pdf` reads it: its field's name and value,
/// its bookmark's title, its attachment's data and its page's content.
fn form_read(pdf: &LazyPdf) -> [Vec<u8>; 5] {
    let trailer = Object::Dictionary(pdf.trailer().clone());
    let catalog = at(pdf, &trailer, &[b"Root"]);
    let field = at(pdf, &catalog, &[b"AcroForm", b"Fields"])
        .as_array()
        .unwrap()[0]
        .clone();
    let page = at(pdf, &catalog, &[b"Pages", b"Kids"]).as_array().unwrap()[0].clone();
    let files = at(pdf, &catalog, &[b"Names", b"EmbeddedFiles", b"Names"]);
    let text = |obj: Object| obj.as_str().unwrap().to_vec();
    let data = |obj: Object| obj.as_stream().unwrap().content.clone();
    [
        text(at(pdf, &field, &[b"T"])),
        text(at(pdf, &field, &[b"V"])),
        text(at(pdf, &catalog, &[b"Outlines", b"First", b"Title"])),
        data(at(pdf, &files.as_array().unwrap()[1], &[b"EF", b"F"])),
        data(at(pdf, &page, &[b"Contents"])),
    ]
}

/// A file whose objects are in an object stream encrypted whole reads
/// each member as it is, lazily and eagerly, and rewrites encrypted and
/// packed again: each container encrypted whole, nothing in the clear,
/// even inflated.
#[test]
fn an_encrypted_object_stream_base_rewrites() {
    let plain = form();
    let states = std::iter::once(rc4_40(&plain)).chain(rc4_aes_128_and_aes_256(&plain));
    for (name, state) in states {
        let base = object_streams_encrypted(&plain, &state);
        let pdf = open_as_user(&base).unwrap();
        assert!(!object_streams_of(&pdf).is_empty(), "{name}");
        assert!(pdf.base_sound(), "{name}");
        assert_eq!(pdf.xref().kind, XrefKind::Stream, "{name}");
        let eager = Pdf::load_mem_with_options(&base, LoadOptions::with_password(USER)).unwrap();
        for (id, obj) in &plain.objects {
            assert!(same(pdf.get(*id).unwrap(), obj), "{name}: {id:?}");
            assert!(same(&eager.objects[id], obj), "{name}, eagerly: {id:?}");
        }
        let read = form_read(&pdf);

        let bytes = crate::save::rewrite(&turned(&base)).unwrap();
        for bytes in [&base, &bytes] {
            for text in FORM_SECRETS {
                assert!(
                    !in_clear(bytes, text),
                    "{name}: {:?}",
                    String::from_utf8_lossy(text)
                );
            }
        }
        let pdf = open_as_user(&bytes).unwrap();
        assert!(!object_streams_of(&pdf).is_empty(), "{name}");
        assert!(pdf.base_sound(), "{name}");
        assert_eq!(pdf.xref().kind, XrefKind::Stream, "{name}");
        assert_eq!(form_read(&pdf), read, "{name}");
        let [field, value, title, attached, _] = read;
        assert_eq!(
            [field, value, title, attached],
            [
                FIELD.as_bytes(),
                VALUE.as_bytes(),
                BOOKMARK.as_bytes(),
                ATTACHED
            ]
            .map(<[u8]>::to_vec),
            "{name}"
        );
        assert_eq!(
            Crypt::of(&pdf).unwrap().unwrap().dict(),
            crypt_of(&base).dict()
        );
    }
}

#[test]
fn full_writes_are_deterministic() {
    let plain = plaintext();
    let crypts = rc4_aes_128_and_aes_256(&plain).map(|(_, state)| {
        let [source, _] = encrypted(&plain, &state, |_| {});
        Some(crypt_of(&source))
    });
    for crypt in std::iter::once(None).chain(crypts) {
        for kind in KINDS {
            let name = format!("{kind:?}, encrypted: {}", crypt.is_some());
            let write = || whole(&plain.objects, &plain.trailer, kind, "1.7", crypt.as_ref());
            let once = write();
            let again = std::thread::scope(|scope| {
                let threads: Vec<_> = (0..2).map(|_| scope.spawn(write)).collect();
                threads
                    .into_iter()
                    .map(|t| t.join().unwrap())
                    .collect::<Vec<_>>()
            });
            assert_eq!(again, [once.clone(), once.clone()], "{name}");
            let pdf = match crypt {
                Some(_) => open_as_user(&once).unwrap(),
                None => open(&once),
            };
            assert!(pdf.base_sound(), "{name}");
            assert_eq!(pdf.xref().kind, kind, "{name}");
        }
    }
}

/// Which pages use each of `pdf`'s objects, as a Rewrite finds them
/// ([`page_uses`]): walked from its pages in order, never into another
/// page or a tree node.
fn uses_of(pdf: &Pdf) -> HashMap<u32, Use> {
    let pages: Vec<ObjectId> = pdf.get_pages().into_values().collect();
    let stop: HashSet<ObjectId> = (pdf.objects.iter())
        .filter(|(_, obj)| matches!(obj.type_name(), Ok(b"Page" | b"Pages")))
        .map(|(&id, _)| id)
        .collect();
    page_uses(&pages, &stop, |id, to| {
        if let Some(obj) = pdf.objects.get(&id) {
            refs(obj, to);
        }
    })
}

/// [`plaintext`] with `count` more objects, each a dictionary with a
/// string: its page names the first quarter, and nothing the rest.
fn with_many(count: u32) -> Pdf {
    let mut pdf = plaintext();
    let many: Vec<Object> = (0..count)
        .map(|n| {
            let text = Object::string_literal(format!("object {n}"));
            pdf.add_object(dictionary! { "N" => n, "Text" => text })
                .into()
        })
        .collect();
    let page = *pdf.get_pages().get(&1).unwrap();
    let page = pdf.get_dictionary_mut(page).unwrap();
    page.set("Many", many[..count as usize / 4].to_vec());
    pdf
}

/// `plain` encrypted with RC4-40, RC4-128, AES-128 and AES-256, each
/// with its name: how each of the source's encryptions writes it.
fn every_crypt(plain: &Pdf) -> Vec<(&'static str, Crypt)> {
    let states = std::iter::once(rc4_40(plain)).chain(rc4_aes_128_and_aes_256(plain));
    (states.map(|(name, state)| {
        let [source, _] = encrypted(plain, &state, |_| {});
        (name, crypt_of(&source))
    }))
    .collect()
}

/// A packed write lists what it packs as its container and its place
/// there (7.5.8.3's type 2), and both readers read every object back as
/// it was. A stream, an object of generation 1 and a signature
/// dictionary as the encryption walk tells one (typed, untyped, and a
/// field merged with its `/FT /Sig`) stay where they are, listed by their
/// offsets (decision 3), and page 1's objects have a container of their
/// own.
#[test]
fn type_2_entries_read_back_lazily_and_eagerly() {
    let mut plain = plaintext();
    let older = (plain.max_id + 1, 1);
    let generation = dictionary! { "Generation" => 1 };
    plain.objects.insert(older, Object::Dictionary(generation));
    let uses = uses_of(&plain);
    let bytes = whole_packed(&plain.objects, &plain.trailer, None, &uses);
    let pdf = open(&bytes);
    assert!(pdf.base_sound());
    assert_eq!(pdf.xref().kind, XrefKind::Stream);
    let xref = pdf.xref().max_stream.unwrap();
    let (index, _, got) = rows(&pdf, (xref, 0));
    assert_eq!(index, [0, i64::from(xref) + 1]);
    let eager = Pdf::load_mem(&bytes).unwrap();
    let mut stayed = BTreeSet::new();
    let mut containers: BTreeMap<u64, Vec<(u64, ObjectId)>> = BTreeMap::new();
    for (&id, obj) in &plain.objects {
        assert!(same(pdf.get(id).unwrap(), obj), "{id:?}");
        assert!(same(&eager.objects[&id], obj), "eagerly: {id:?}");
        match got[id.0 as usize] {
            [1, _, generation] => {
                assert_eq!(generation, u64::from(id.1), "{id:?}");
                stayed.insert(id);
            }
            [2, container, index] => containers.entry(container).or_default().push((index, id)),
            row => panic!("{id:?}: {row:?}"),
        }
    }
    let streams = (plain.objects.iter()).filter(|(_, obj)| matches!(obj, Object::Stream(_)));
    let signatures = (plain.objects.iter()).filter(|(_, obj)| {
        obj.as_dict().is_ok_and(|d| {
            let field = d.get(b"FT").ok() == Some(&Object::Name(b"Sig".to_vec()));
            d.has_type(b"Sig") || d.has(b"ByteRange") || field
        })
    });
    let want: BTreeSet<ObjectId> = (streams.chain(signatures).map(|(&id, _)| id))
        .chain([older])
        .collect();
    assert_eq!(want.len(), 9);
    assert_eq!(stayed, want);
    // Two containers: the page's, and the rest's.
    assert_eq!(containers.len(), 2);
    for (&container, members) in &containers {
        assert_eq!(got[container as usize][0], 1, "{container}");
        let stream = pdf.get((container as u32, 0)).unwrap().as_stream().unwrap();
        assert!(stream.dict.has_type(b"ObjStm"), "{container}");
        let n = stream.dict.get(b"N").unwrap().as_i64().unwrap();
        assert_eq!(n, members.len() as i64, "{container}");
        let places: Vec<u64> = members.iter().map(|&(index, _)| index).collect();
        assert_eq!(places, (0..n as u64).collect::<Vec<_>>(), "{container}");
        let page_one = (members.iter())
            .filter(|(_, id)| uses.get(&id.0) == Some(&Use::Page(0)))
            .count();
        assert!(page_one == 0 || page_one == members.len(), "{container}");
    }
    let page = *plain.get_pages().get(&1).unwrap();
    let first = *containers.keys().next().unwrap();
    assert_eq!(got[page.0 as usize], [2, first, 0], "the page's is first");
}

/// A packed write writes the same bytes every time: twice and on two
/// threads, plain and encrypted every way, with eleven containers to
/// build on a thread per core.
#[test]
fn packed_writes_are_deterministic() {
    let plain = with_many(1000);
    let uses = uses_of(&plain);
    let crypts = every_crypt(&plain)
        .into_iter()
        .map(|(name, c)| (name, Some(c)));
    for (name, crypt) in std::iter::once(("plain", None)).chain(crypts) {
        let write = || whole_packed(&plain.objects, &plain.trailer, crypt.as_ref(), &uses);
        let once = write();
        let again = std::thread::scope(|scope| {
            let threads: Vec<_> = (0..2).map(|_| scope.spawn(write)).collect();
            threads
                .into_iter()
                .map(|t| t.join().unwrap())
                .collect::<Vec<_>>()
        });
        assert_eq!(again, [once.clone(), once.clone()], "{name}");
        let containers = once.windows(12).filter(|w| w == b"/Type/ObjStm").count();
        assert_eq!(containers, 11, "{name}");
        let pdf = match crypt {
            Some(_) => open_as_user(&once).unwrap(),
            None => open(&once),
        };
        assert!(pdf.base_sound(), "{name}");
    }
}

/// Containers build the same on one thread as on eight, plain and
/// encrypted every way: forty of them, on eight threads in eight runs of
/// five. Cancelled, neither builds.
#[test]
fn containers_build_alike_on_one_thread_and_on_eight() {
    use super::full::{Member, containers};

    let plain = plaintext();
    let members: Vec<Member> = (1..=4000)
        .map(|n| {
            let text = Object::string_literal(format!("object {n}"));
            let obj = Object::Dictionary(dictionary! { "N" => n, "Text" => text });
            ((n, 0), key_of((n, 0)), Cow::Owned(obj))
        })
        .collect();
    let groups: Vec<(u32, Vec<&Member>)> = (5000..)
        .zip(members.chunks(100))
        .map(|(n, part)| (n, part.iter().collect()))
        .collect();
    let crypts = every_crypt(&plain)
        .into_iter()
        .map(|(name, c)| (name, Some(c)));
    for (name, crypt) in std::iter::once(("plain", None)).chain(crypts) {
        let built = |threads| containers(&groups, crypt.as_ref(), threads, &NoProgress).unwrap();
        let one = built(1);
        assert_eq!(built(8), one, "{name}");
        assert_eq!(one.len(), 40, "{name}");
        for ((n, _), container) in groups.iter().zip(&one) {
            let Some(Object::Stream(stream)) = container else {
                panic!("{name}: {n} {container:?}");
            };
            assert!(stream.dict.has_type(b"ObjStm"), "{name}: {n}");
            assert_eq!(stream.dict.get(b"N").unwrap(), &Object::Integer(100));
        }
        let cancelled = in_progress::Tracker::default();
        cancelled.cancel();
        for threads in [1, 8] {
            let e = containers(&groups, crypt.as_ref(), threads, &cancelled).unwrap_err();
            assert!(e.is::<in_progress::Cancelled>(), "{name}: {e:#}");
        }
    }
}

/// An encrypted packed write encrypts each container once, as a stream
/// under its own number, and its members' strings only with it: the
/// readers read every object back with the user password, and refuse the
/// file without one; nothing is in the clear, even inflated, but a
/// signature's `/Contents`; the `/Encrypt` dictionary and `/ID[0]` are
/// the source's; and it writes the same bytes twice.
#[test]
fn containers_encrypt_once_as_the_reader_decrypts() {
    let plain = plaintext();
    let uses = uses_of(&plain);
    for (name, crypt) in every_crypt(&plain) {
        let bytes = whole_packed(&plain.objects, &plain.trailer, Some(&crypt), &uses);
        assert!(holds(&bytes, b"/Type/ObjStm"), "{name}");
        for text in SECRETS {
            assert!(
                !in_clear(&bytes, text),
                "{name}: {:?}",
                String::from_utf8_lossy(text)
            );
        }
        assert!(holds(&bytes, SIGNATURE_WRITTEN), "{name}");
        assert!(
            matches!(
                LazyPdf::open(Arc::<[u8]>::from(&bytes[..]), None, &Limits::DEFAULT.reader),
                Err(lopdf::Error::InvalidPassword)
            ),
            "{name}"
        );
        let pdf = open_as_user(&bytes).unwrap();
        assert!(pdf.base_sound(), "{name}");
        let xref = pdf.xref().max_stream.unwrap();
        let (_, _, got) = rows(&pdf, (xref, 0));
        let eager = Pdf::load_mem_with_options(&bytes, LoadOptions::with_password(USER)).unwrap();
        let mut packed = 0;
        for (&id, obj) in &plain.objects {
            assert!(same(pdf.get(id).unwrap(), obj), "{name}: {id:?}");
            assert!(same(&eager.objects[&id], obj), "{name}, eagerly: {id:?}");
            packed += usize::from(got[id.0 as usize][0] == 2);
        }
        assert_eq!(packed, 5, "{name}");
        let now = Crypt::of(&pdf).unwrap().unwrap();
        assert_eq!(now.dict(), crypt.dict(), "{name}");
        assert!(crypt.first_id().is_some(), "{name}");
        assert_eq!(now.first_id(), crypt.first_id(), "{name}");
        let again = whole_packed(&plain.objects, &plain.trailer, Some(&crypt), &uses);
        assert_eq!(again, bytes, "{name}");
    }
}

/// `plain` encrypted with AES-128 for its strings, its streams left in
/// the clear (`/StmF /Identity`).
fn strings_only(plain: &Pdf) -> EncryptionState {
    EncryptionState::try_from(EncryptionVersion::V4 {
        document: plain,
        encrypt_metadata: true,
        crypt_filters: BTreeMap::from([(
            b"StdCF".to_vec(),
            Arc::new(Aes128CryptFilter) as Arc<dyn CryptFilter>,
        )]),
        stream_filter: b"Identity".to_vec(),
        string_filter: b"StdCF".to_vec(),
        owner_password: OWNER,
        user_password: USER,
        permissions: Permissions::all(),
    })
    .unwrap()
}

/// Under an encryption that leaves streams in the clear (`/StmF
/// /Identity`) but not strings, nothing is packed, so no string is in the
/// clear; with strings in the clear too, it packs.
#[test]
fn streams_in_the_clear_under_encrypted_strings_pack_nothing() {
    let plain = plaintext();
    let uses = uses_of(&plain);
    let crypt = crypt_of(&encrypted(&plain, &strings_only(&plain), |_| {})[0]);
    assert!(!crypt.packs());
    let bytes = whole_packed(&plain.objects, &plain.trailer, Some(&crypt), &uses);
    assert!(!holds(&bytes, b"/ObjStm"));
    for text in [PAGE_STRING, TITLE, FIELD_NAME] {
        assert!(
            !in_clear(&bytes, text),
            "{:?}",
            String::from_utf8_lossy(text)
        );
    }
    assert!(holds(&bytes, CONTENT));
    let pdf = open_as_user(&bytes).unwrap();
    for (&id, obj) in &plain.objects {
        assert!(same(pdf.get(id).unwrap(), obj), "strings only: {id:?}");
    }

    let crypt = crypt_of(&encrypted(&plain, &attachment_only(&plain), |_| {})[0]);
    assert!(crypt.packs());
    let bytes = whole_packed(&plain.objects, &plain.trailer, Some(&crypt), &uses);
    assert!(holds(&bytes, b"/Type/ObjStm"));
    assert!(!in_clear(&bytes, ATTACHMENT));
    // Deflated in a container, a string a search of the bytes misses.
    assert!(!holds(&bytes, PAGE_STRING) && in_clear(&bytes, PAGE_STRING));
    let pdf = open_as_user(&bytes).unwrap();
    for (&id, obj) in &plain.objects {
        assert!(same(pdf.get(id).unwrap(), obj), "attachment only: {id:?}");
    }
}

/// A file's objects: a catalog (1), its page tree (2), a page (3) and
/// its content (4), and 250 dictionaries (10 to 259), for [`packed`].
fn numbered() -> BTreeMap<ObjectId, Object> {
    let mut objects = BTreeMap::from([
        (
            (1, 0),
            Object::Dictionary(dictionary! { "Type" => "Catalog", "Pages" => (2, 0) }),
        ),
        (
            (2, 0),
            Object::Dictionary(
                dictionary! { "Type" => "Pages", "Kids" => vec![(3, 0).into()], "Count" => 1 },
            ),
        ),
        (
            (3, 0),
            Object::Dictionary(dictionary! {
                "Type" => "Page", "Parent" => (2, 0), "Contents" => (4, 0),
                "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            }),
        ),
        (
            (4, 0),
            Object::Stream(Stream::new(dictionary! {}, b"0 0 m 9 9 l S".to_vec())),
        ),
    ]);
    for n in 10..260 {
        let text = Object::string_literal(format!("object {n}"));
        objects.insert((n, 0), Object::Dictionary(dictionary! { "Text" => text }));
    }
    objects
}

/// `objects` written whole with a cross-reference stream from
/// `xref_from`, packed with page 1 using object 3 and containers taking
/// `reuse`'s numbers first, encrypted with `crypt` at object 6 when given.
fn packed(
    objects: &BTreeMap<ObjectId, Object>,
    reuse: &[u32],
    xref_from: u32,
    crypt: Option<&Crypt>,
) -> anyhow::Result<Vec<u8>> {
    use super::full::{Body, Layout, Pack, write_bodies};
    use crate::materialize::emit::PdfVersion;

    let id = Object::String(b"0123456789abcdef".to_vec(), StringFormat::Hexadecimal);
    let trailer = dictionary! { "Root" => (1, 0), "ID" => vec![id.clone(), id] };
    let uses = || HashMap::from([(3, Use::Page(0))]);
    let layout = Layout {
        trailer: &trailer,
        kind: XrefKind::Stream,
        version: PdfVersion::new(1, 7),
        crypt,
        encrypt_at: (6, 0),
        xref_from,
        pack: Some(Pack { uses: &uses, reuse }),
    };
    let bodies =
        (objects.iter()).map(|(&id, obj)| Ok((id, Body::Object(Cow::Borrowed(obj), key_of(id)))));
    Ok(write_bodies(bodies, &layout, &NoProgress)?.0)
}

/// Containers take the numbers the source's own containers had first, in
/// order, passing over 0, what the file lists (a stream it writes on its
/// own, an object it packs, the `/Encrypt` dictionary), one given already
/// and one past what a file may use; then from past the last object and
/// the least the cross-reference stream takes, which goes after them.
#[test]
fn containers_take_the_source_s_numbers_first() {
    let objects = numbered();
    let plain = plaintext();
    let [source, _] = encrypted(&plain, &rc4_aes_128_and_aes_256(&plain)[1].1, |_| {});
    let crypt = crypt_of(&source);
    let reuse = [0, 4, 3, super::NUMBERS, 6, 8, 8, 5];
    let cases = [
        ("plain", &reuse[..], 300, None, [6, 8, 5, 300], 301),
        (
            "encrypted",
            &reuse[..],
            300,
            Some(&crypt),
            [8, 5, 300, 301],
            302,
        ),
        ("none to reuse", &[][..], 1, None, [260, 261, 262, 263], 264),
    ];
    for (name, reuse, xref_from, crypt, want, xref) in cases {
        let bytes = packed(&objects, reuse, xref_from, crypt).unwrap();
        let pdf = match crypt {
            Some(_) => open_as_user(&bytes).unwrap(),
            None => open(&bytes),
        };
        assert!(pdf.base_sound(), "{name}");
        assert_eq!(pdf.xref().max_stream, Some(xref), "{name}");
        assert_eq!(pdf.xref().size, xref + 1, "{name}");
        let (_, _, got) = rows(&pdf, (xref, 0));
        // Page 1's container, then the rest's: 1, 2 and 10 to 107; 108 to
        // 207; and 208 to 259.
        let containers = [3, 1, 108, 208].map(|n| got[n][1] as u32);
        assert_eq!(containers, want, "{name}");
        for (n, first) in want.into_iter().zip([3, 1, 108, 208]) {
            assert_eq!(got[first], [2, u64::from(n), 0], "{name}: {first}");
            assert_eq!(got[n as usize][0], 1, "{name}: {n}");
        }
        assert_eq!(got[4][0], 1, "{name}: the stream");
        for (&id, obj) in &objects {
            assert!(same(pdf.get(id).unwrap(), obj), "{name}: {id:?}");
        }
    }
}

/// Packing that would leave the cross-reference stream no number a file
/// may use writes every object on its own instead: four containers from
/// the fifth number before the last leave it the last, and from the
/// fourth, none.
#[test]
fn packing_runs_out_of_numbers_writes_plain() {
    let last = super::NUMBERS - 1;
    let objects = numbered();
    let bytes = packed(&objects, &[], last - 4, None).unwrap();
    let pdf = open(&bytes);
    assert!(pdf.base_sound());
    assert_eq!(pdf.xref().max_stream, Some(last));
    let (_, _, got) = rows(&pdf, (last, 0));
    assert_eq!(got[3], [2, u64::from(last - 4), 0]);
    assert_eq!(got[259], [2, u64::from(last - 1), 51]);

    // With no container, no object can be listed as in one.
    let bytes = packed(&objects, &[], last - 3, None).unwrap();
    assert!(!holds(&bytes, b"/ObjStm"));
    let pdf = open(&bytes);
    assert!(pdf.base_sound());
    assert_eq!(pdf.xref().max_stream, Some(last - 3));
    for (&id, obj) in &objects {
        assert!(same(pdf.get(id).unwrap(), obj), "{id:?}");
    }
}

/// A container whose data would be past what the reader takes isn't
/// made: its members are written on their own, its number left free, and
/// the other containers are made as ever. Object 11, a string as long as
/// a container's data may be, is in the second container, with 1, 2 and
/// 10 to 107.
#[test]
fn a_container_past_what_the_reader_takes_is_written_plain() {
    let mut objects = numbered();
    let long = Object::String(vec![b'a'; objstm::MAX_DATA], StringFormat::Literal);
    objects.insert((11, 0), long);
    let bytes = packed(&objects, &[], 0, None).unwrap();
    let pdf = open(&bytes);
    assert!(pdf.base_sound());
    let xref = pdf.xref().max_stream.unwrap();
    assert_eq!(xref, 264);
    let (_, _, got) = rows(&pdf, (xref, 0));
    for n in [1, 2].into_iter().chain(10..108) {
        assert_eq!(got[n][0], 1, "{n}");
    }
    assert_eq!(got[261][0], 0, "the second container's number");
    for (n, first) in [(260, 3), (262, 108), (263, 208)] {
        assert_eq!(got[n][0], 1, "{n}");
        assert_eq!(got[first], [2, n as u64, 0], "{first}");
    }
    assert_eq!(object_streams_of(&pdf), [(260, 0), (262, 0), (263, 0)]);
    for (&id, obj) in &objects {
        assert!(same(pdf.get(id).unwrap(), obj), "{id:?}");
    }
}

/// A cross-reference table can't list a container's members, so a layout
/// of one that packs is refused.
#[test]
fn a_table_layout_refuses_to_pack() {
    use super::full::{Layout, Pack, write_bodies};
    use crate::materialize::emit::PdfVersion;

    let trailer = dictionary! { "Root" => (1, 0) };
    let layout = Layout {
        trailer: &trailer,
        kind: XrefKind::Table,
        version: PdfVersion::new(1, 7),
        crypt: None,
        encrypt_at: (0, 0),
        xref_from: 0,
        pack: Some(Pack {
            uses: &HashMap::new,
            reuse: &[],
        }),
    };
    let e = write_bodies([], &layout, &NoProgress).unwrap_err();
    assert_eq!(
        e.to_string(),
        "a cross-reference table can't list objects in object streams"
    );
}

/// A file of `pages` pages ([`file`](crate::materialize::tests::file)),
/// each with a comment of its own, as lopdf writes it with its objects in
/// object streams of `per` each, numbered after them; the catalog's
/// `/Junk` names the object stream at `junk`, from 0, when given.
fn in_object_streams(pages: usize, per: usize, junk: Option<u32>) -> Vec<u8> {
    let mut pdf = crate::materialize::tests::file("A", pages, false);
    for (i, page) in (0..).zip(pdf.get_pages().into_values()) {
        let note = pdf.add_object(dictionary! {
            "Type" => "Annot", "Subtype" => "Square", "P" => page,
            "Rect" => vec![10.into(), 10.into(), 30.into(), 30.into()],
            "Contents" => Object::string_literal(format!("on page {i}")),
        });
        let page = pdf.get_dictionary_mut(page).unwrap();
        page.set("Annots", vec![note.into()]);
    }
    if let Some(junk) = junk {
        let at = (pdf.max_id + 1 + junk, 0);
        pdf.catalog_mut().unwrap().set("Junk", at);
    }
    let options = lopdf::SaveOptions::builder()
        .use_object_streams(true)
        .use_xref_streams(true)
        .max_objects_per_stream(per)
        .build();
    let mut out = Vec::new();
    pdf.save_with_options(&mut out, options).unwrap();
    out
}

/// The numbers of the object streams `pdf`'s table lists, in the order
/// they're written.
fn containers_in_order(pdf: &LazyPdf) -> Vec<u32> {
    let mut found: Vec<(usize, u32)> = (object_streams_of(pdf).into_iter())
        .map(|id| (pdf.raw_span(id).unwrap().start, id.0))
        .collect();
    found.sort_unstable();
    found.into_iter().map(|(_, n)| n).collect()
}

/// The object stream `pdf` lists object `id` in, if any.
fn container_of(pdf: &LazyPdf, id: ObjectId) -> Option<u32> {
    let (_, _, got) = rows(pdf, (pdf.xref().max_stream.unwrap(), 0));
    match got.get(id.0 as usize) {
        Some(&[2, container, _]) => Some(container as u32),
        _ => None,
    }
}

/// An empty directory of the test's own, named `name`.
fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("sheaf-serialize-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A file saved twice in a session, by a Rewrite each time, keeps its
/// `/Size` and its containers' numbers (decision 6): the second save
/// numbers them as the first did.
#[test]
fn saving_again_doesn_t_grow_size() {
    let dir = scratch("saving-again");
    let path = dir.join("a.pdf");
    std::fs::write(&path, in_object_streams(3, 100, None)).unwrap();
    let mut doc = crate::open::file(&path, None).unwrap().doc;
    let mut saved = Vec::new();
    for _ in 0..2 {
        let first = doc.page(0).unwrap();
        let turn = crate::tx::fixture::rotate(&[first], sheaf_edit::Turn::Quarter);
        doc.commit_ops(vec![turn], "turn").unwrap();
        crate::materialize::save_rewritten(&mut doc, &path).unwrap();
        let pdf = open(&std::fs::read(&path).unwrap());
        assert!(pdf.base_sound());
        saved.push((pdf.xref().size, containers_in_order(&pdf)));
    }
    assert_eq!(saved[0].1.len(), 2, "page 1's and the rest's");
    assert_eq!(saved[1], saved[0]);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A Rewrite's containers take the numbers its source's had first, in
/// order, then numbers past its `/Size`. Reopened and rewritten, the file
/// gives its containers the same numbers, and its `/Size` grows by one:
/// the cross-reference stream it was opened with keeps its number, which
/// an update after it would have, so the new one goes past it.
#[test]
fn a_rewrite_reuses_its_source_s_container_numbers() {
    // More containers than a Rewrite makes: it takes the first.
    let base = in_object_streams(3, 3, None);
    let theirs = containers_in_order(&open(&base));
    let once = crate::save::rewrite(&turned(&base)).unwrap();
    let ours = containers_in_order(&open(&once));
    assert_eq!(ours.len(), 2, "page 1's and the rest's");
    assert!(theirs.len() > ours.len());
    assert_eq!(ours, theirs[..2]);

    // Fewer: the rest are past what the source used.
    let base = in_object_streams(3, 100, None);
    let was = open(&base);
    let theirs = containers_in_order(&was);
    assert_eq!(theirs.len(), 1);
    let once = crate::save::rewrite(&turned(&base)).unwrap();
    let now = open(&once);
    let ours = containers_in_order(&now);
    assert_eq!(ours.len(), 2);
    assert_eq!(ours[0], theirs[0]);
    assert!(ours[1] >= was.xref().size, "{ours:?}");
    assert_eq!(now.xref().max_stream, Some(now.xref().size - 1));

    let twice = crate::save::rewrite(&turned(&once)).unwrap();
    let again = open(&twice);
    assert!(again.base_sound());
    assert_eq!(containers_in_order(&again), ours);
    assert_eq!(again.xref().size, now.xref().size + 1);
}

/// A container something in the file still names keeps its number, though
/// it isn't written: the reference doesn't come to name a new container,
/// which takes the source's other numbers.
#[test]
fn a_referenced_container_s_number_isn_t_reused() {
    let base = in_object_streams(3, 3, Some(1));
    let was = open(&base);
    let theirs = containers_in_order(&was);
    let junk = theirs[1];
    let named = |pdf: &LazyPdf| pdf.catalog().unwrap().get(b"Junk").unwrap().clone();
    assert_eq!(named(&was), Object::Reference((junk, 0)));
    let out = crate::save::rewrite(&turned(&base)).unwrap();
    let now = open(&out);
    assert!(now.base_sound());
    assert_eq!(named(&now), Object::Reference((junk, 0)));
    assert!(now.get((junk, 0)).is_err());
    let ours = containers_in_order(&now);
    assert_eq!(ours.len(), 2, "page 1's and the rest's");
    let others: Vec<u32> = theirs.into_iter().filter(|&n| n != junk).collect();
    assert_eq!(ours, others[..2]);
}

/// A Rewrite packs page 1's objects in a container of their own, written
/// before the rest: the page and its comment, but nothing another page,
/// the page tree or the catalog names.
#[test]
fn page_one_s_objects_have_a_container_of_their_own() {
    let base = in_object_streams(3, 100, None);
    let out = crate::save::rewrite(&turned(&base)).unwrap();
    let now = open(&out);
    let eager = Pdf::load_mem(&out).unwrap();
    let pages: Vec<ObjectId> = eager.get_pages().into_values().collect();
    let note = |page: ObjectId| {
        let annots = eager.get_dictionary(page).unwrap().get(b"Annots").unwrap();
        annots.as_array().unwrap()[0].as_reference().unwrap()
    };
    let first = containers_in_order(&now)[0];
    for id in [pages[0], note(pages[0])] {
        assert_eq!(container_of(&now, id), Some(first), "{id:?}");
    }
    let catalog = now.trailer().get(b"Root").unwrap().as_reference().unwrap();
    let tree = (eager.catalog().unwrap().get(b"Pages").unwrap())
        .as_reference()
        .unwrap();
    let others = pages[1..].iter().flat_map(|&page| [page, note(page)]);
    for id in others.chain([catalog, tree]) {
        let container = container_of(&now, id);
        assert!(container.is_some_and(|c| c != first), "{id:?}");
    }
}

/// A hybrid-reference file rewrites as a file with a cross-reference
/// stream alone, packed (decision 1): its table was only there for
/// readers before PDF 1.5. Its form reads as it did.
#[test]
fn a_hybrid_rewrites_as_a_packed_stream_file() {
    let base = crate::forms::fixture::hybrid_reference();
    let was = open(&base);
    assert!(was.xref().hybrid);
    assert_eq!(was.xref().kind, XrefKind::Table);
    let out = crate::save::rewrite(&turned(&base)).unwrap();
    let now = open(&out);
    assert!(now.base_sound());
    assert_eq!(now.xref().kind, XrefKind::Stream);
    assert!(!now.xref().hybrid && !now.trailer().has(b"XRefStm"));
    assert_eq!(now.version(), "1.5");
    // The source's container is page 1's now, its widgets with it.
    assert_eq!(containers_in_order(&now)[0], 8);
    for n in [3, 6, 7] {
        assert_eq!(container_of(&now, (n, 0)), Some(8), "{n}");
    }
    let values = |bytes: &[u8]| {
        let src = crate::forms::fixture::source(bytes.to_vec());
        let form = src.form();
        let named = (form.fields.iter()).map(|f| (f.name.clone(), f.value.clone()));
        named.collect::<BTreeMap<_, _>>()
    };
    let text = |s: &str| Some(crate::forms::field::Value::Text(s.into()));
    let filled = BTreeMap::from([
        ("city".into(), text("Hull")),
        ("name".into(), text("Ann Lee")),
    ]);
    assert_eq!(values(&base), filled);
    assert_eq!(values(&out), filled);
}

/// A file with a cross-reference table rewrites with one, never packed:
/// every object on its own, its header's version as it was.
#[test]
fn a_table_base_rewrites_unpacked() {
    let mut pdf = crate::materialize::tests::file("A", 3, false);
    pdf.version = "1.4".into();
    let base = crate::materialize::tests::bytes(&pdf);
    let out = crate::save::rewrite(&turned(&base)).unwrap();
    let now = open(&out);
    assert!(now.base_sound());
    assert_eq!(now.xref().kind, XrefKind::Table);
    assert_eq!(now.version(), "1.4");
    assert_eq!(object_streams_of(&now), []);
    for id in now.ids() {
        assert!(now.raw_span(id).is_some(), "{id:?}");
    }
}

/// A file encrypted with its strings encrypted and its streams left in
/// the clear (`/StmF /Identity`) rewrites unpacked, its object streams'
/// members too: packed, their strings would be in the clear with their
/// container. Each is encrypted on its own instead, so none is. A hybrid
/// so encrypted rewrites as a table alone.
#[test]
fn identity_stmf_under_an_encrypting_strf_isn_t_packed() {
    let plain = plaintext();
    let state = strings_only(&plain);
    let [_, stream] = encrypted(&plain, &state, |_| {});
    let packed = object_streams_encrypted(&plain, &state);
    assert!(in_clear(&packed, PAGE_STRING), "the source's container is");
    for (kind, base) in [("stream", stream), ("packed", packed)] {
        let out = crate::save::rewrite(&turned(&base)).unwrap();
        let pdf = open_as_user(&out).unwrap();
        assert!(pdf.base_sound(), "{kind}");
        assert_eq!(pdf.xref().kind, XrefKind::Stream, "{kind}");
        assert_eq!(object_streams_of(&pdf), [], "{kind}");
        for text in [PAGE_STRING, TITLE, FIELD_NAME] {
            assert!(
                !in_clear(&out, text),
                "{kind}: {:?}",
                String::from_utf8_lossy(text)
            );
        }
        assert!(holds(&out, CONTENT), "{kind}");
        assert_eq!(rewritten(&pdf), as_written(), "{kind}");
    }

    let id = plain.trailer.get(b"ID").unwrap().as_array().unwrap()[0].clone();
    let encrypt = encrypt_dict(&state);
    let hybrid = hybrid_reference_encrypted(&encrypt, id.as_str().unwrap());
    let was = open_as_user(&hybrid).unwrap();
    assert!(was.xref().hybrid && was.encryption().is_some());
    let values = |bytes: &[u8]| {
        let kind = BlobKind::file(Path::new("hybrid.pdf"), Some(USER));
        let src = Store::new().open(Arc::from(bytes), kind).unwrap();
        let form = src.form();
        let named = (form.fields.iter()).map(|f| (f.name.clone(), f.value.clone()));
        named.collect::<BTreeMap<_, _>>()
    };
    let text = |s: &str| Some(crate::forms::field::Value::Text(s.into()));
    let filled = BTreeMap::from([
        ("city".into(), text("Hull")),
        ("name".into(), text("Ann Lee")),
    ]);
    assert_eq!(values(&hybrid), filled);
    // The source's container holds its members' strings as they are.
    assert!(holds(&hybrid, b"(Ann Lee)"));
    let out = crate::save::rewrite(&turned(&hybrid)).unwrap();
    let pdf = open_as_user(&out).unwrap();
    assert!(pdf.base_sound());
    assert_eq!(pdf.xref().kind, XrefKind::Table);
    assert!(!pdf.xref().hybrid && !pdf.trailer().has(b"XRefStm"));
    assert_eq!(object_streams_of(&pdf), []);
    assert!(pdf.encryption().is_some());
    for text in [&b"Ann Lee"[..], b"Hull", b"name", b"city"] {
        assert!(!in_clear(&out, text), "{:?}", String::from_utf8_lossy(text));
    }
    assert_eq!(values(&out), filled);
}

#[test]
fn a_full_write_keeps_the_trailer_but_the_bases_sections() {
    let linearized = dictionary! { "Linearized" => 1, "L" => 1000 };
    let objstm = Stream::new(
        dictionary! { "Type" => "ObjStm", "N" => 0, "First" => 0 },
        vec![],
    );
    let xref = Stream::new(
        dictionary! { "Type" => "XRef", "W" => vec![1.into()] },
        vec![],
    );
    let objects = BTreeMap::from([
        ((1, 0), catalog()),
        (
            (2, 0),
            Object::Dictionary(dictionary! { "Title" => "Info" }),
        ),
        ((3, 0), Object::Dictionary(linearized)),
        ((4, 0), Object::Stream(objstm)),
        ((5, 0), Object::Stream(xref)),
        ((6, 2), Object::Integer(6)),
    ]);
    let id = Object::String(b"0123456789abcdef".to_vec(), StringFormat::Hexadecimal);
    let mut base = dictionary! {
        "Root" => (1, 0),
        "Info" => (2, 0),
        "Custom" => "Kept",
        "ID" => vec![id.clone(), id],
        "Encrypt" => (9, 0),
    };
    for key in ["Prev", "XRefStm", "Size", "Length", "DL"] {
        base.set(key, 1000);
    }
    for key in [
        "Type",
        "W",
        "Index",
        "Filter",
        "DecodeParms",
        "F",
        "FFilter",
        "FDecodeParms",
    ] {
        base.set(key, "Stale");
    }
    for kind in KINDS {
        let bytes = whole(&objects, &base, kind, "1.4", None);
        assert!(
            bytes.starts_with(b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n"),
            "{kind:?}"
        );
        let pdf = open(&bytes);
        assert!(pdf.base_sound(), "{kind:?}");
        assert_eq!(pdf.xref().sections, 1, "{kind:?}");
        // The linearization dictionary and the base's streams left out, and
        // the stream this section is, when it's one, listed.
        let (mut ids, own): (_, &[&[u8]]) = match kind {
            XrefKind::Table => (vec![(1, 0), (2, 0), (6, 2)], &[b"Size"]),
            XrefKind::Stream => (
                vec![(1, 0), (2, 0), (6, 2), (7, 0)],
                &[b"Size", b"Type", b"W", b"Index", b"Length"],
            ),
        };
        assert_eq!(pdf.ids().collect::<Vec<_>>(), ids, "{kind:?}");
        // The base's keys but its sections', and this section's own.
        let trailer = pdf.trailer();
        let mut kept: Vec<&[u8]> = trailer.iter().map(|(k, _)| k.as_slice()).collect();
        kept.retain(|k| !own.contains(k));
        kept.sort_unstable();
        assert_eq!(
            kept,
            [b"Custom".as_slice(), b"ID", b"Info", b"Root"],
            "{kind:?}"
        );
        let size = ids.pop().unwrap().0 + 1;
        assert_eq!(pdf.xref().size, size, "{kind:?}");
        assert_eq!(
            trailer.get(b"Custom").unwrap(),
            &Object::Name(b"Kept".to_vec())
        );
        // /ID[0] kept, and /ID[1] the hash of what comes before the section.
        let written = sheaf_util::checksum(&bytes[..pdf.xref().start]).to_be_bytes();
        let id = [b"0123456789abcdef".as_slice(), &written]
            .map(|id| Object::String(id.to_vec(), StringFormat::Hexadecimal));
        assert_eq!(trailer.get(b"ID").unwrap(), &Object::Array(id.to_vec()));
    }
}

#[test]
fn print_is_unencrypted() {
    let plain = plaintext();
    for (name, state) in rc4_aes_128_and_aes_256(&plain) {
        let [source, _] = encrypted(&plain, &state, |_| {});
        let doc = turned(&source);
        let ids: Vec<_> = doc.state().order().iter().copied().collect();
        for markup in [true, false] {
            let print = |f, _| crate::export::Export::print(f, &ids, markup);
            let bytes = doc.export_bytes(print);
            let pdf = open(&bytes);
            assert!(!pdf.is_encrypted(), "{name}");
            assert!(!pdf.trailer().has(b"Encrypt"), "{name}");
            assert!(holds(&bytes, CONTENT), "{name}");
        }
    }
}

/// What the mark [`mark`] puts on a page says.
pub(crate) const NOTE: &[u8] = b"a mark's note";

/// `doc`'s first page with a square marked on it, saying [`NOTE`].
pub(crate) fn mark(doc: &mut Document) {
    let page = doc.page(0).unwrap();
    let square = Annot {
        id: AnnotId(0),
        kind: AnnotKind::Square {
            rect: Box2 {
                x: 10.0,
                y: 10.0,
                w: 20.0,
                h: 20.0,
            },
        },
        color: [255, 0, 0],
        opacity: 1.0,
        stroke: 1.0,
        author: "t".into(),
        contents: String::from_utf8(NOTE.to_vec()).unwrap(),
        modified: String::new(),
        created: String::new(),
        fill: None,
        dashed: false,
        cloudy: false,
        turn: Rotation::R0,
        name: String::new(),
    };
    let add = crate::tx::fixture::add(page, &square);
    doc.commit_ops(vec![add], "mark").unwrap();
}

/// [`turned`], and [`mark`]ed.
fn marked(bytes: &[u8]) -> Document {
    let mut doc = turned(bytes);
    mark(&mut doc);
    doc
}

/// What the marks on the first page of `pdf` say.
pub(crate) fn notes(pdf: &LazyPdf) -> Vec<Vec<u8>> {
    let trailer = Object::Dictionary(pdf.trailer().clone());
    let catalog = at(pdf, &trailer, &[b"Root"]);
    let page = at(pdf, &catalog, &[b"Pages", b"Kids"]).as_array().unwrap()[0].clone();
    let annots = at(pdf, &page, &[b"Annots"]);
    annots
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|annot| {
            let annot = pdf.dereference(annot).unwrap().1.as_dict().unwrap();
            Some(annot.get(b"Contents").ok()?.as_str().unwrap().to_vec())
        })
        .collect()
}

#[test]
fn encrypted_files_append_encrypted() {
    for (ids, plain) in plaintexts() {
        let mut states = rc4_aes_128_and_aes_256(&plain).to_vec();
        states.push(("attachment only", attachment_only(&plain)));
        for (name, state) in states {
            let strings = state.get_string_filter().unwrap().method() != b"Identity";
            for (source, kind) in encrypted(&plain, &state, |_| {}).iter().zip(KINDS) {
                let name = format!("{name}, {kind:?}, {ids}");
                let doc = marked(source);
                let bytes = crate::materialize::appended(&doc).unwrap();
                assert_eq!(crate::materialize::appended(&doc).unwrap(), bytes);
                assert!(bytes.starts_with(source), "{name}");
                // The page it writes holds its string, and the mark its note:
                // encrypted, but where the file encrypts only its attachment.
                let section = &bytes[source.len()..];
                for text in [PAGE_STRING, NOTE] {
                    assert_eq!(
                        holds(section, text),
                        !strings,
                        "{name}: {:?}",
                        String::from_utf8_lossy(text)
                    );
                }
                // The new revision, lazily and eagerly.
                let pdf = open_as_user(&bytes).unwrap();
                assert!(pdf.base_sound(), "{name}");
                assert_eq!(pdf.xref().kind, kind, "{name}");
                assert_eq!(pdf.xref().sections, 2, "{name}");
                assert_eq!(rewritten(&pdf), as_written(), "{name}");
                assert_eq!(notes(&pdf), [NOTE], "{name}");
                let eager =
                    Pdf::load_mem_with_options(&bytes, LoadOptions::with_password(USER)).unwrap();
                assert_same(&eager, &pdf, &name);
                // Its /Encrypt where it was, as stored, /P and all; /ID[0]
                // kept and /ID[1] new.
                let was = open_as_user(source).unwrap();
                let encrypt = |pdf: &LazyPdf| pdf.trailer().get(b"Encrypt").unwrap().clone();
                assert_eq!(encrypt(&pdf), encrypt(&was), "{name}");
                assert!(matches!(encrypt(&pdf), Object::Reference(_)), "{name}");
                let now = Crypt::of(&pdf).unwrap().unwrap();
                assert_eq!(now.dict(), crypt_of(source).dict(), "{name}");
                let [first, second] = pdf.xref().id.clone().unwrap();
                assert_eq!(Some(first), super::first_id(was.trailer()), "{name}");
                let was_second = was.xref().id.clone().map(|[_, second]| second);
                assert_ne!(Some(second), was_second, "{name}");
            }
        }
    }
}

#[test]
fn a_file_with_a_100_byte_prefix_appends() {
    let plain = plaintext();
    for kind in KINDS {
        let file = whole(&plain.objects, &plain.trailer, kind, "1.7", None);
        let prefixed = [&PREFIX[..], &file].concat();
        let append = |bytes: &[u8]| crate::materialize::appended(&turned(bytes));
        let (appended, after_prefix) = (append(&file).unwrap(), append(&prefixed).unwrap());
        // Positions count from the header, so past the prefix it's the
        // same update.
        assert!(after_prefix[PREFIX.len()..] == appended[..], "{kind:?}");
        let (pdf, unprefixed) = (open(&after_prefix), open(&appended));
        assert!(pdf.base_sound(), "{kind:?}");
        assert_eq!(pdf.xref().header_offset, PREFIX.len(), "{kind:?}");
        assert_eq!(pdf.xref().sections, 2, "{kind:?}");
        let start = pdf.xref().start;
        assert_eq!(start, unprefixed.xref().start, "{kind:?}");
        assert!(start > file.len(), "{kind:?}: the update's section");
        let ids: Vec<ObjectId> = pdf.ids().collect();
        assert_eq!(ids, unprefixed.ids().collect::<Vec<_>>(), "{kind:?}");
        for id in ids {
            let (got, want) = (pdf.get(id).unwrap(), unprefixed.get(id).unwrap());
            assert!(same(got, want), "{kind:?}: {id:?}");
        }
        assert_eq!(rewritten(&pdf), as_written(), "{kind:?}");
    }
}

/// An update lists only what it writes, in runs, its `/Size` past them,
/// and nothing when it writes nothing.
#[test]
fn an_update_lists_only_what_it_writes() {
    // Object 3 at generation 4; 2 rewritten, 4 added.
    let mut file = b"%PDF-1.7\n".to_vec();
    let mut full = Section::full(0);
    for (id, obj) in [((1, 0), catalog()), ((2, 0), 2.into()), ((3, 4), 3.into())] {
        let pos = object::indirect(&mut file, id, &obj).unwrap();
        full.insert(
            id.0,
            Entry::InUse {
                pos,
                generation: id.1,
            },
        );
    }
    let pos = file.len();
    full.table(&mut file, dictionary! { "Root" => (1, 0) }, pos)
        .unwrap();
    let objects: BTreeMap<ObjectId, (crate::materialize::ObjKey, Object)> =
        [((2, 0), 22.into()), ((4, 0), 4.into())]
            .into_iter()
            .map(|(id, obj)| (id, (key_of(id), obj)))
            .collect();
    for kind in KINDS {
        let base = Base {
            len: file.len(),
            ends_line: true,
            header: 0,
            prev: open(&file).xref().start,
            size: 4,
            kind,
            first_id: None,
            root: (1, 0),
            info: None,
        };
        assert_eq!(
            append::section(&base, &BTreeMap::new(), 5, None).unwrap(),
            None
        );
        let update = append::section(&base, &objects, 5, None).unwrap().unwrap();
        let bytes = [&file[..], &update.bytes].concat();
        let pdf = open(&bytes);
        assert!(pdf.base_sound(), "{kind:?}");
        assert_eq!(pdf.xref().kind, kind);
        assert_eq!(pdf.xref().sections, 2, "{kind:?}");
        assert_eq!(pdf.xref().start, update.start, "{kind:?}");
        assert_eq!(pdf.xref().size, update.size, "{kind:?}");
        assert_eq!(pdf.get((2, 0)).unwrap(), &Object::Integer(22));
        assert_eq!(pdf.get((3, 4)).unwrap(), &Object::Integer(3));
        assert_eq!(pdf.get((4, 0)).unwrap(), &Object::Integer(4));
        // Neither an /ID nor /Encrypt, with none to keep.
        assert!(!pdf.trailer().has(b"ID") && !pdf.trailer().has(b"Encrypt"));
        assert_eq!(update.id, None);
        match kind {
            XrefKind::Table => {
                assert_eq!(update.size, 5);
                let table = String::from_utf8_lossy(&update.bytes);
                assert!(table.contains("xref\n2 1\n"), "{table}");
                assert!(table.contains("\n4 1\n"), "{table}");
            }
            XrefKind::Stream => {
                assert_eq!(update.size, 6);
                let (index, _, _) = rows(&pdf, (5, 0));
                assert_eq!(index, [2, 1, 4, 2], "{kind:?}");
            }
        }
    }
}

crate::both_ways!(
    a_base_numbered_to_the_last_number_isnt_appended_to,
    a_base_numbered_to_the_last_number_isnt_appended_to_appended: numbered_to_the_last
);

/// A file whose cross-reference stream is numbered 4,294,967,295, which
/// the reader takes: an update would have no number left past it, so it's
/// refused, and a Rewrite saves the file, small: it reserves no number
/// past what a file may use, so its own cross-reference stream is numbered
/// past the file's objects.
fn numbered_to_the_last(how: crate::materialize::How) {
    let mut file = b"%PDF-1.7\n".to_vec();
    let mut rows = vec![0, 0, 0, 0, 0, 0xff, 0xff];
    let media = vec![0.into(), 0.into(), 200.into(), 200.into()];
    let objects = [
        dictionary! { "Type" => "Catalog", "Pages" => (2, 0) },
        dictionary! { "Type" => "Pages", "Kids" => vec![(3, 0).into()], "Count" => 1 },
        dictionary! { "Type" => "Page", "Parent" => (2, 0), "MediaBox" => media },
    ];
    for (num, dict) in (1..).zip(objects) {
        let pos = object::indirect(&mut file, (num, 0), &dict.into()).unwrap();
        rows.push(1);
        rows.extend_from_slice(&(pos as u32).to_be_bytes());
        rows.extend_from_slice(&[0, 0]);
    }
    let start = file.len();
    let xref = dictionary! {
        "Type" => "XRef",
        "Size" => 4,
        "W" => vec![1.into(), 4.into(), 2.into()],
        "Index" => vec![0.into(), 4.into()],
        "Root" => (1, 0),
    };
    let stream = Object::Stream(Stream::new(xref, rows));
    object::indirect(&mut file, (u32::MAX, 0), &stream).unwrap();
    file.extend_from_slice(format!("startxref\n{start}\n%%EOF\n").as_bytes());
    assert_eq!(open(&file).xref().max_stream, Some(u32::MAX));

    let doc = turned(&file);
    if how == crate::materialize::How::Appended {
        let refused = crate::materialize::appended(&doc).unwrap_err();
        assert!(
            refused.to_string().contains("no object number left"),
            "{refused}"
        );
        return;
    }
    let rewritten = crate::save::rewrite(&doc).unwrap();
    assert!(rewritten.len() < 4096, "{} bytes", rewritten.len());
    let pdf = open(&rewritten);
    assert!(pdf.base_sound());
    assert!(pdf.xref().max_stream.is_some_and(|n| n < 10));
    assert!(pdf.xref().size < 10);
}

/// A whole file's body, or its cross-reference stream, at a number past
/// what a file may use is refused before the section lists a row: it
/// lists every number from 0, so it would grow without bound.
#[test]
fn a_number_past_what_a_file_may_use_isnt_written() {
    use std::borrow::Cow;

    use super::NUMBERS;
    use super::full::{Body, Layout, write_bodies};
    use crate::materialize::ObjKey;
    use crate::materialize::emit::PdfVersion;
    use crate::store::BlobId;

    let trailer = dictionary! { "Root" => (1, 0) };
    let layout = |kind, xref_from| Layout {
        trailer: &trailer,
        kind,
        version: PdfVersion::new(1, 7),
        crypt: None,
        encrypt_at: (0, 0),
        xref_from,
        pack: None,
    };
    let blob = BlobId { hash: 0, len: 0 };
    let catalog = catalog();
    let bodies = |at: u32| {
        [(1, 0), (at, 0)].map(|id| {
            let key = ObjKey::Base {
                blob,
                obj: id,
                copy: 0,
            };
            Ok((id, Body::Object(Cow::Borrowed(&catalog), key)))
        })
    };
    for kind in KINDS {
        let past = write_bodies(bodies(NUMBERS), &layout(kind, 1), &NoProgress);
        let e = past.expect_err("refused");
        assert!(
            format!("{e:#}").contains("past the 8388607"),
            "{kind:?}: {e:#}"
        );
        let fits = write_bodies(bodies(2), &layout(kind, 1), &NoProgress).unwrap();
        assert!(open(&fits.0).base_sound(), "{kind:?}");
    }
    let stream = write_bodies(bodies(2), &layout(XrefKind::Stream, NUMBERS), &NoProgress);
    assert!(stream.is_err());
}
