//! Encrypted files built in code, for this crate's tests and, with the
//! `fixtures` feature, other crates' (M18 slices 11–13), synthesized via
//! `crate::pdf::test_crypt` (`pdf-crypt`'s `insecure-test-writer` +
//! `pdf-structure::write::Encryptor`).

use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use in_progress::NoProgress;
use sheaf_edit::Turn;

use super::crypt::Crypt;
use super::group::Use;
use super::{full, object, objstm};
use crate::forms::fixture::{Form, note, outline_tree, row};
use crate::materialize::ObjKey;
use crate::materialize::emit::PdfVersion;
use crate::model::Document;
use crate::pdf::test_crypt::{CipherKind, TestEncryption};
use crate::pdf::{
    Dictionary, Document as Pdf, LazyPdf, Object, ObjectId, ReadLimits, Stream, XrefKind,
    dictionary, inflate, literal, resolve_fn,
};
use crate::store::BlobId;
use crate::tx::fixture as fx;

pub const OWNER: &str = "owner";
pub const USER: &str = "user";

pub const FIELD: &str = "applicant";
pub const VALUE: &str = "Ann Lee";
pub const BOOKMARK: &str = "Chapter One";
pub const ATTACHED: &[u8] = b"an attached note";

pub const FORM_SECRETS: [&[u8]; 5] = [
    FIELD.as_bytes(),
    VALUE.as_bytes(),
    BOOKMARK.as_bytes(),
    ATTACHED,
    b"Page 1",
];

/// A one-page form to encrypt: "Page 1" in Helvetica, a text field
/// [`FIELD`] filled with [`VALUE`], a bookmark [`BOOKMARK`] to the page,
/// an attached file holding [`ATTACHED`], which `/EFF` encrypts alone,
/// and the `/ID` RC4's and AES-128's keys are made from.
pub fn form() -> Pdf {
    let mut f = Form::new(1);
    let field = f.text(FIELD, 0, row(0));
    f.fill(field, VALUE);
    let page = f.pages[0];
    outline_tree(
        &mut f,
        vec![(
            0,
            dictionary! {
                "Title" => literal(BOOKMARK),
                "Dest" => vec![Object::from(page), Object::name("Fit")],
            },
        )],
    );
    let data = f.pdf.add_object(Stream::new(
        dictionary! { "Type" => Object::name("EmbeddedFile") },
        ATTACHED.to_vec(),
    ));
    let spec = f.pdf.add_object(dictionary! {
        "Type" => Object::name("Filespec"),
        "F" => literal("note.txt"),
        "EF" => dictionary! { "F" => data },
    });
    let catalog = f.catalog;
    f.dict(catalog).set(
        "Names",
        dictionary! { "EmbeddedFiles" => dictionary! {
            "Names" => vec![literal("note.txt"), Object::from(spec)],
        } },
    );
    let id = literal("0123456789abcdef");
    f.pdf.trailer.set("ID", vec![id.clone(), id]);
    f.pdf
}

/// `pdf` with its `/ID` cut to its first string.
pub fn one_id(mut pdf: Pdf) -> Pdf {
    let first = pdf.trailer.get(b"ID").unwrap().as_array().unwrap()[0].clone();
    pdf.trailer.set("ID", vec![first]);
    pdf
}

/// RC4-40 (`/V 1`) encryption spec for `plain`.
pub fn rc4_40(_plain: &Pdf) -> (&'static str, TestEncryption) {
    (
        "RC4-40",
        TestEncryption::new(CipherKind::Rc4_40, USER, OWNER),
    )
}

/// RC4-128, AES-128 and AES-256 encryption specs for `plain`.
pub fn rc4_aes_128_and_aes_256(_plain: &Pdf) -> [(&'static str, TestEncryption); 3] {
    [
        (
            "RC4-128",
            TestEncryption::new(CipherKind::Rc4_128, USER, OWNER),
        ),
        (
            "AES-128",
            TestEncryption::new(CipherKind::Aes128, USER, OWNER),
        ),
        (
            "AES-256",
            TestEncryption::new(CipherKind::Aes256, USER, OWNER),
        ),
    ]
}

/// V4 with AES-128 for embedded files only (`/StmF /Identity`, `/StrF /Identity`, `/EFF /StdCF`).
pub fn attachment_only(_plain: &Pdf) -> TestEncryption {
    TestEncryption::new(CipherKind::Aes128, USER, OWNER).with_filters(
        b"Identity",
        b"Identity",
        Some(b"StdCF".to_vec()),
    )
}

/// `state`'s `/Encrypt` dictionary as stored in `plain`.
pub fn encrypt_dict_for(plain: &Pdf, state: &TestEncryption) -> Dictionary {
    state.encrypt_dict(plain)
}

/// `plain` encrypted with `state` and [`written`], applying `edit` to the
/// emitted `/Encrypt` dictionary before writing.
pub fn encrypted(
    plain: &Pdf,
    state: &TestEncryption,
    edit: impl Fn(&mut Dictionary),
) -> [Vec<u8>; 2] {
    let mut pdf = plain.clone();
    pdf.encrypt(state).unwrap();
    let id = pdf.trailer.get(b"Encrypt").unwrap().as_reference().unwrap();
    let dict = pdf.get_dictionary_mut(id).unwrap();
    edit(dict);
    written(&pdf)
}

/// `pdf` written with a cross-reference table and with a stream.
pub fn written(pdf: &Pdf) -> [Vec<u8>; 2] {
    [XrefKind::Table, XrefKind::Stream].map(|kind| {
        let mut pdf = pdf.clone();
        pdf.xref_kind = kind;
        let mut bytes = Vec::new();
        pdf.save_to(&mut bytes).unwrap();
        bytes
    })
}

pub fn whole(
    objects: &BTreeMap<ObjectId, Object>,
    base: &Dictionary,
    kind: XrefKind,
    version: &str,
    crypt: Option<&Crypt>,
) -> Vec<u8> {
    write_whole(objects, base, kind, version, crypt, None)
}

pub fn whole_packed(
    objects: &BTreeMap<ObjectId, Object>,
    base: &Dictionary,
    crypt: Option<&Crypt>,
    uses: &HashMap<u32, Use>,
) -> Vec<u8> {
    let uses = || uses.clone();
    let pack = full::Pack {
        uses: &uses,
        reuse: &[],
    };
    write_whole(objects, base, XrefKind::Stream, "1.7", crypt, Some(pack))
}

fn write_whole(
    objects: &BTreeMap<ObjectId, Object>,
    base: &Dictionary,
    kind: XrefKind,
    version: &str,
    crypt: Option<&Crypt>,
    pack: Option<full::Pack<'_>>,
) -> Vec<u8> {
    let blob = BlobId { hash: 0, len: 0 };
    let written: Vec<(&ObjectId, &Object)> = (objects.iter())
        .filter(|(_, obj)| !full::left_out(obj))
        .collect();
    let past = written
        .iter()
        .map(|(id, _)| id.number + 1)
        .max()
        .unwrap_or(1);
    let bodies = written.into_iter().map(|(&id, obj)| {
        let key = ObjKey::Base {
            blob,
            obj: id,
            copy: 0,
        };
        Ok((id, full::Body::Object(Cow::Borrowed(obj), key)))
    });
    let layout = full::Layout {
        trailer: base,
        kind,
        version: PdfVersion::parse(version.as_bytes()).expect("a version"),
        crypt,
        encrypt_at: ObjectId::new(past, 0),
        xref_from: 1,
        pack,
    };
    full::write_bodies(bodies, &layout, &NoProgress).unwrap().0
}

/// `pdf` encrypted with `state` with every non-stream object in one encrypted
/// `/ObjStm` container and a cross-reference stream.
pub fn object_streams_encrypted(pdf: &Pdf, state: &TestEncryption) -> Vec<u8> {
    let container = pdf.max_id + 1;
    let (encrypt, xref) = (container + 1, container + 2);
    let mut rows: BTreeMap<u32, (u8, u64, u16)> = BTreeMap::from([(0, (0, 0, u16::MAX))]);
    let (mut pairs, mut members) = (String::new(), Vec::new());
    let mut packed = 0;
    for (&id, obj) in &pdf.objects {
        if obj.as_stream().is_some() {
            continue;
        }
        assert_eq!(id.generation, 0, "an object stream holds generation 0 only");
        pairs.push_str(&format!("{} {} ", id.number, members.len()));
        object::write(&mut members, obj).unwrap();
        members.push(b'\n');
        rows.insert(id.number, (2, u64::from(container), packed));
        packed += 1;
    }
    let objstm = Stream::new(
        dictionary! {
            "Type" => Object::name("ObjStm"),
            "N" => i64::from(packed),
            "First" => pairs.len() as i64,
        },
        [pairs.into_bytes(), members].concat(),
    );

    let enc = state.encryptor(pdf);
    let encrypt_id = ObjectId::new(encrypt, 0);
    let resolve = resolve_fn(|id| pdf.objects.get(&id));
    let encrypting = enc
        .laid_out(None, Some(encrypt_id), &pdf.trailer, &resolve)
        .unwrap();

    let mut out = format!("%PDF-{}\n", pdf.version).into_bytes();
    out.extend_from_slice(full::BINARY);
    let streams = pdf
        .objects
        .iter()
        .filter(|(_, obj)| obj.as_stream().is_some())
        .map(|(&id, obj)| (id, obj.clone()))
        .chain([(ObjectId::new(container, 0), Object::from(objstm))]);
    for (id, obj) in streams {
        let pos = object::indirect_with(&mut out, id, &obj, &encrypting).unwrap();
        rows.insert(id.number, (1, pos as u64, id.generation));
    }
    let dict = Object::from(state.encrypt_dict(pdf));
    let pos = object::indirect(&mut out, encrypt_id, &dict).unwrap();
    rows.insert(encrypt, (1, pos as u64, 0));

    let start = out.len();
    rows.insert(xref, (1, start as u64, 0));
    let mut data = Vec::new();
    for num in 0..=xref {
        let (kind, field, last) = rows.get(&num).copied().unwrap_or((0, 0, 0));
        data.push(kind);
        data.extend_from_slice(&(field as u32).to_be_bytes());
        data.extend_from_slice(&last.to_be_bytes());
    }
    let mut trailer = dictionary! {
        "Type" => Object::name("XRef"),
        "Size" => i64::from(xref + 1),
        "W" => vec![Object::from(1), Object::from(4), Object::from(2)],
        "Encrypt" => encrypt_id,
    };
    for key in [&b"Root"[..], b"Info", b"ID"] {
        if let Some(value) = pdf.trailer.get(key) {
            trailer.set(key, value.clone());
        }
    }
    object::indirect(
        &mut out,
        ObjectId::new(xref, 0),
        &Object::from(Stream::new(trailer, data)),
    )
    .unwrap();
    out.extend_from_slice(format!("startxref\n{start}\n%%EOF\n").as_bytes());
    out
}

pub const PREFIX: [u8; 100] = [b'~'; 100];

pub fn blank_table(count: usize) -> Vec<u8> {
    let mut pdf = Pdf::with_version("1.4");
    let pages = pdf.new_object_id();
    let content = pdf.add_object(Stream::new(dictionary! {}, Vec::new()));
    let kids: Vec<Object> = (0..count)
        .map(|_| {
            let page = dictionary! {
                "Type" => Object::name("Page"), "Parent" => pages, "Contents" => content,
                "MediaBox" => vec![Object::from(0), Object::from(0), Object::from(612), Object::from(792)],
            };
            pdf.add_object(page).into()
        })
        .collect();
    let tree = dictionary! {
        "Type" => Object::name("Pages"),
        "Kids" => kids,
        "Count" => count as i64,
    };
    pdf.objects.insert(pages, tree.into());
    let catalog = pdf.add_object(dictionary! {
        "Type" => Object::name("Catalog"),
        "Pages" => pages,
    });
    pdf.trailer.set("Root", catalog);
    pdf.xref_kind = XrefKind::Table;
    let mut bytes = Vec::new();
    pdf.save_to(&mut bytes).unwrap();
    bytes
}

pub fn revision_cut_out(mut bytes: Vec<u8>) -> Vec<u8> {
    let max = Pdf::load_mem(&bytes).unwrap().max_id;
    let at = bytes.windows(9).rposition(|w| w == b"startxref").unwrap() + 9;
    let digits: String = (bytes[at..].iter())
        .skip_while(|b| b.is_ascii_whitespace())
        .take_while(|b| b.is_ascii_digit())
        .map(|&b| b as char)
        .collect();
    let table: usize = digits.parse().unwrap();
    let cut = format!(
        "\ntrailer\n<< /Size {} /Root {} 0 R >>\nstartxref\n{}\n%%EOF\n",
        max + 3,
        max + 1,
        table + 20
    );
    bytes.extend_from_slice(cut.as_bytes());
    bytes
}

pub fn linearized_file() -> Vec<u8> {
    linearized_file_after(b"")
}

pub fn linearized_file_after(lead: &[u8]) -> Vec<u8> {
    const L: &str = "/L 0000000000";
    let mut out = b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n".to_vec();
    out.extend_from_slice(lead);
    let content = b"q 0 0 1 RG 72 72 m 540 720 l S Q";
    let objects = [
        (
            5,
            format!("<< /Linearized 1 {L} /O 3 /E 0 /N 1 /T 0 /H [0 0] >>").into_bytes(),
        ),
        (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
        (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
        (
            3,
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << >> \
              /Contents 4 0 R >>"
                .to_vec(),
        ),
        (
            4,
            [
                format!("<< /Length {} >>\nstream\n", content.len()).as_bytes(),
                content,
                b"\nendstream",
            ]
            .concat(),
        ),
    ];
    let mut offsets = BTreeMap::new();
    for (number, body) in objects {
        offsets.insert(number, out.len());
        out.extend_from_slice(format!("{number} 0 obj\n").as_bytes());
        out.extend_from_slice(&body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend_from_slice(b"xref\n0 6\n0000000000 65535 f \n");
    for offset in offsets.values() {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    let tail = format!("trailer\n<< /Size 6 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n");
    out.extend_from_slice(tail.as_bytes());
    let at = out
        .windows(L.len())
        .position(|w| w == L.as_bytes())
        .unwrap();
    let length = format!("/L {:<10}", out.len());
    out[at..at + L.len()].copy_from_slice(length.as_bytes());
    out
}

pub fn holds(bytes: &[u8], text: &[u8]) -> bool {
    bytes.windows(text.len()).any(|w| w == text)
}

pub fn in_clear(bytes: &[u8], text: &[u8]) -> bool {
    let hex = |digits: &[u8; 16]| -> Vec<u8> {
        (text.iter())
            .flat_map(|&b| [digits[usize::from(b >> 4)], digits[usize::from(b & 15)]])
            .collect()
    };
    let forms = [
        text.to_vec(),
        hex(b"0123456789ABCDEF"),
        hex(b"0123456789abcdef"),
    ];
    let found = |hay: &[u8]| forms.iter().any(|form| holds(hay, form));
    let data = |at: usize| {
        let data = &bytes[at..];
        let data = data.strip_prefix(b"\r").unwrap_or(data);
        let data = data.strip_prefix(b"\n")?;
        let end = data.windows(9).position(|w| w == b"endstream")?;
        Some(&data[..end])
    };
    let inflates_to_it = |data: &[u8]| {
        inflate(data, ReadLimits::DEFAULT.max_decompressed).is_ok_and(|plain| found(&plain))
    };
    found(bytes)
        || (bytes.windows(6).enumerate())
            .filter(|&(at, w)| w == b"stream" && !bytes[..at].ends_with(b"end"))
            .filter_map(|(at, _)| data(at + 6))
            .any(inflates_to_it)
}

pub fn object_streams_of(pdf: &LazyPdf) -> Vec<ObjectId> {
    (pdf.ids())
        .filter(|&id| pdf.get(id).is_ok_and(objstm::is_container))
        .collect()
}

pub fn edited(path: &Path, password: Option<&str>) -> Document {
    let mut doc = crate::open::file(path, password).unwrap().doc;
    let page = doc.page(0).unwrap();
    doc.commit_ops(vec![fx::rotate(&[page], Turn::Quarter)], "turn")
        .unwrap();
    doc.commit_ops(vec![fx::add(page, &note())], "note")
        .unwrap();
    doc
}
