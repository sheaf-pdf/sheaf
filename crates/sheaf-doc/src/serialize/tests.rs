//! What the serializer writes, read back by lopdf's parser through
//! `LazyPdf`: objects as they were, cross-reference sections that read
//! as sound, objects encrypted as the reader decrypts them, whole files,
//! rewrites of encrypted ones encrypted as they were, and updates after a
//! file's last section, encrypted as the file is and placed past any
//! bytes before its header.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use in_progress::NoProgress;
use lopdf::encryption::crypt_filters::{Aes128CryptFilter, CryptFilter};
use lopdf::{
    Document as Pdf, EncryptionState, EncryptionVersion, LazyPdf, LoadOptions, Object, ObjectId,
    Permissions, Stream, StringFormat, XrefKind, dictionary,
};
use sheaf_core::Rotation;

use super::append::{self, Base};
use super::crypt::Crypt;
use super::xref::{Entry, Section};
use super::{Error, full, object};
use crate::annot::{Annot, AnnotId, AnnotKind, Box2};
use crate::history::FixedCount;
use crate::model::{Document, Edit, SourceInfo};
use crate::serialize::fixture::{
    ATTACHED, BOOKMARK, FIELD, FORM_SECRETS, OWNER, PREFIX, USER, VALUE, attachment_only,
    encrypted, form, holds, object_streams_encrypted, one_id, rc4_aes_128_and_aes_256,
};
use crate::source::Limits;
use crate::source::lazy_tests::{
    ATTACHMENT, CONTENT, FORM_STRING, PAGE_STRING, SECRETS, SIGNATURE_WRITTEN, TITLE, XMP,
    assert_decrypts_to, assert_same, plaintext, same,
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
    section.insert(2, Entry::deleted(4));
    section.insert(5, Entry::deleted(0));
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
    section.insert(3, Entry::deleted(0));
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
    assert_eq!(
        Entry::deleted(u16::MAX),
        Entry::Free {
            generation: u16::MAX
        }
    );
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
/// field, generation]`, read by its `/W`.
fn rows(pdf: &LazyPdf, id: ObjectId) -> (Vec<i64>, Vec<i64>, Vec<[u64; 3]>) {
    let s = pdf.get(id).unwrap().as_stream().unwrap();
    assert_eq!(
        s.dict.get(b"Type").unwrap(),
        &Object::Name(b"XRef".to_vec())
    );
    assert!(!s.dict.has(b"Filter"), "written uncompressed");
    let ints = |key: &[u8]| -> Vec<i64> {
        let array = s.dict.get(key).unwrap().as_array().unwrap();
        array.iter().map(|o| o.as_i64().unwrap()).collect()
    };
    let (index, w) = (ints(b"Index"), ints(b"W"));
    let widths: Vec<usize> = w.iter().map(|&n| n as usize).collect();
    let rows = s
        .content
        .chunks(widths.iter().sum())
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
    section.insert(3, Entry::deleted(0));
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

/// How `bytes`, opened with the user password, are encrypted.
fn crypt_of(bytes: &[u8]) -> Crypt {
    Crypt::of(&open_as_user(bytes).unwrap()).unwrap().unwrap()
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
        put(&mut out, id, &crypt.encrypt(id, obj).unwrap());
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

#[test]
fn ivs_are_keyed() {
    let plain = plaintext();
    let text = |s: &str| Object::String(s.as_bytes().to_vec(), StringFormat::Literal);
    for (name, state) in rc4_aes_128_and_aes_256(&plain) {
        let [source, _] = encrypted(&plain, &state, |_| {});
        let crypt = crypt_of(&source);
        let encrypt = |id: ObjectId, obj: &Object| crypt.encrypt(id, obj).unwrap();
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
        // RC4 takes no IV.
        if name == "RC4-128" {
            continue;
        }
        // An AES string's IV, the first 16 bytes, is another for another
        // value, another number or another generation.
        let iv = |id: ObjectId, value: &str| match encrypt(id, &text(value)) {
            Object::String(bytes, _) => bytes[..16].to_vec(),
            other => panic!("{name}: {other:?}"),
        };
        let first = iv((5, 0), "a value");
        assert_eq!(iv((5, 0), "a value"), first, "{name}");
        for other in [
            iv((5, 0), "another value"),
            iv((6, 0), "a value"),
            iv((5, 1), "a value"),
        ] {
            assert_ne!(other, first, "{name}");
        }
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
    let stays = |id: ObjectId, obj: &Object| assert_eq!(&crypt.encrypt(id, obj).unwrap(), obj);
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
    let encrypted = crypt.encrypt(sig, signature).unwrap();
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
    let primary = SourceInfo {
        src: src.clone(),
        name: "encrypted.pdf".into(),
        derived: None,
        stamp: None,
    };
    let mut doc = crate::open::start(store, primary, Box::new(FixedCount(10)));
    doc.ready(crate::open::finish(&src, &NoProgress)).unwrap();
    let first = doc.page(0).unwrap().id;
    let turn = Edit::RotatePages {
        ids: vec![first],
        by: Rotation::R90,
    };
    doc.apply(turn, "turn").unwrap();
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

#[test]
fn rewrites_stay_encrypted() {
    for (ids, plain) in plaintexts() {
        for (name, state) in rc4_aes_128_and_aes_256(&plain) {
            for (source, kind) in encrypted(&plain, &state, |_| {}).iter().zip(KINDS) {
                let name = format!("{name}, {kind:?}, {ids}");
                let bytes = crate::save::rewrite(&turned(source).snapshot()).unwrap();
                for text in SECRETS {
                    assert!(
                        !holds(&bytes, text),
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

#[test]
fn an_eff_file_rewrites_with_only_its_attachment_encrypted() {
    let plain = plaintext();
    for (source, kind) in encrypted(&plain, &attachment_only(&plain), |_| {})
        .iter()
        .zip(KINDS)
    {
        let bytes = crate::save::rewrite(&turned(source).snapshot()).unwrap();
        assert!(!holds(&bytes, ATTACHMENT), "{kind:?}");
        for text in [TITLE, PAGE_STRING, FORM_STRING, CONTENT, XMP] {
            assert!(
                holds(&bytes, text),
                "{kind:?}: {:?}",
                String::from_utf8_lossy(text)
            );
        }
        let pdf = open_as_user(&bytes).unwrap();
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
/// each member as it is, lazily and eagerly, and rewrites encrypted with
/// no object stream: each member a plain object encrypted on its own.
#[test]
fn an_encrypted_object_stream_base_rewrites() {
    let plain = form();
    for (name, state) in rc4_aes_128_and_aes_256(&plain) {
        let base = object_streams_encrypted(&plain, &state);
        assert!(holds(&base, b"/ObjStm"), "{name}");
        let pdf = open_as_user(&base).unwrap();
        assert!(pdf.base_sound(), "{name}");
        assert_eq!(pdf.xref().kind, XrefKind::Stream, "{name}");
        let eager = Pdf::load_mem_with_options(&base, LoadOptions::with_password(USER)).unwrap();
        for (id, obj) in &plain.objects {
            assert!(same(pdf.get(*id).unwrap(), obj), "{name}: {id:?}");
            assert!(same(&eager.objects[id], obj), "{name}, eagerly: {id:?}");
        }
        let read = form_read(&pdf);

        let bytes = crate::save::rewrite(&turned(&base).snapshot()).unwrap();
        for bytes in [&base, &bytes] {
            for text in FORM_SECRETS {
                assert!(
                    !holds(bytes, text),
                    "{name}: {:?}",
                    String::from_utf8_lossy(text)
                );
            }
        }
        assert!(!holds(&bytes, b"/ObjStm"), "{name}");
        let pdf = open_as_user(&bytes).unwrap();
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
            let write = || {
                full::write(&plain.objects, &plain.trailer, kind, "1.7", crypt.as_ref()).unwrap()
            };
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
        let bytes = full::write(&objects, &base, kind, "1.4", None).unwrap();
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
        let written = sheaf_core::checksum(&bytes[..pdf.xref().start]).to_be_bytes();
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
        let snap = turned(&source).snapshot();
        let ids: Vec<_> = snap.state.pages.iter().map(|p| p.id).collect();
        for markup in [true, false] {
            let bytes = crate::save::print_pdf(&snap, &ids, markup).unwrap();
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
    let page = doc.page(0).unwrap().id;
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
    let add = Edit::AnnotAdd {
        page,
        annot: square,
    };
    doc.apply(add, "mark").unwrap();
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
                let snap = marked(source).snapshot();
                let bytes = crate::incremental::incremental(&snap).unwrap();
                assert_eq!(crate::incremental::incremental(&snap).unwrap(), bytes);
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
        let file = full::write(&plain.objects, &plain.trailer, kind, "1.7", None).unwrap();
        let prefixed = [&PREFIX[..], &file].concat();
        let append = |bytes: &[u8]| crate::incremental::incremental(&turned(bytes).snapshot());
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

#[test]
fn an_appended_free_entry_takes_the_next_generation() {
    // Object 3 at generation 4, deleted; 2 rewritten, 4 added.
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
    let objects = BTreeMap::from([((2, 0), 22.into()), ((4, 0), 4.into())]);
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
        assert!(
            append::section(&base, &BTreeMap::new(), &[], 5, None)
                .unwrap()
                .is_empty()
        );
        let section = append::section(&base, &objects, &[(3, 4)], 5, None).unwrap();
        let bytes = [&file[..], &section].concat();
        let pdf = open(&bytes);
        assert!(pdf.base_sound(), "{kind:?}");
        assert_eq!(pdf.xref().kind, kind);
        assert_eq!(pdf.xref().sections, 2, "{kind:?}");
        assert_eq!(pdf.get((2, 0)).unwrap(), &Object::Integer(22));
        assert_eq!(pdf.get((4, 0)).unwrap(), &Object::Integer(4));
        // Neither an /ID nor /Encrypt, with none to keep.
        assert!(!pdf.trailer().has(b"ID") && !pdf.trailer().has(b"Encrypt"));
        let free = match kind {
            XrefKind::Table => {
                assert_eq!(pdf.xref().size, 5);
                let table = String::from_utf8_lossy(&section);
                assert!(table.contains("xref\n0 1\n0000000003 65535 f \n2 3\n"));
                table.contains("\n0000000000 00005 f \n")
            }
            XrefKind::Stream => {
                assert_eq!(pdf.xref().size, 6);
                let (index, _, rows) = rows(&pdf, (5, 0));
                assert_eq!(index, [0, 1, 2, 4], "{kind:?}");
                rows[0] == [0, 3, 65535] && rows[2] == [0, 0, 5]
            }
        };
        assert!(free, "{kind:?}: 3 is free, used next at generation 5");
    }
}

/// A file whose cross-reference stream is numbered 4,294,967,295, which
/// the reader takes: an update would have no number left past it, so it's
/// refused, and a rewrite, which renumbers, saves the file.
#[test]
fn a_base_numbered_to_the_last_number_isnt_appended_to() {
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

    let snap = turned(&file).snapshot();
    let refused = crate::incremental::incremental(&snap).unwrap_err();
    assert!(
        format!("{refused:#}").contains("no object number left"),
        "{refused:#}"
    );
    let pdf = open(&crate::save::rewrite(&snap).unwrap());
    assert!(pdf.base_sound());
    assert!(pdf.xref().max_stream.is_some_and(|n| n < 10));
}
