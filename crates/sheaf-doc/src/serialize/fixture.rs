//! Encrypted files built in code, for this crate's tests and, with the
//! `fixtures` feature, other crates': the encryptions a save keeps
//! (RC4-128, AES-128 and AES-256, and AES-128 for attachments only, by
//! `/EFF`) as lopdf writes them, a form to encrypt, a file whose `/ID`
//! is one string, and a file whose objects are in an encrypted object
//! stream, which lopdf can't write; and what the serializer's tests
//! share: the edit they save, the bytes some writers put before the
//! header, and a search of the bytes written.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use lopdf::encryption;
use lopdf::encryption::crypt_filters::{Aes128CryptFilter, Aes256CryptFilter, CryptFilter};
use lopdf::xref::XrefType;
use lopdf::{
    Dictionary, Document as Pdf, EncryptionState, EncryptionVersion, Object, Permissions, Stream,
    dictionary,
};

use sheaf_core::Rotation;

use super::{full, object};
use crate::forms::fixture::{Form, note, outline_tree, row};
use crate::model::{Edit, Snapshot};

pub const OWNER: &str = "owner";
pub const USER: &str = "user";

/// What [`form`] holds, each a string nothing else in the file says: its
/// text field's name and value, its bookmark's title, and its attached
/// file's data.
pub const FIELD: &str = "applicant";
pub const VALUE: &str = "Ann Lee";
pub const BOOKMARK: &str = "Chapter One";
pub const ATTACHED: &[u8] = b"an attached note";

/// Everything [`form`] says, its page's text included, which an
/// encrypted file of it never shows in the clear.
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
                "Title" => Object::string_literal(BOOKMARK),
                "Dest" => vec![page.into(), "Fit".into()],
            },
        )],
    );
    let data = f.pdf.add_object(Stream::new(
        dictionary! { "Type" => "EmbeddedFile" },
        ATTACHED.to_vec(),
    ));
    let spec = f.pdf.add_object(dictionary! {
        "Type" => "Filespec",
        "F" => Object::string_literal("note.txt"),
        "EF" => dictionary! { "F" => data },
    });
    let catalog = f.catalog;
    f.dict(catalog).set(
        "Names",
        dictionary! { "EmbeddedFiles" => dictionary! {
            "Names" => vec![Object::string_literal("note.txt"), spec.into()],
        } },
    );
    let id = Object::string_literal("0123456789abcdef");
    f.pdf.trailer.set("ID", vec![id.clone(), id]);
    f.pdf
}

/// `pdf` with its `/ID` cut to its first string, as some writers write
/// it: RC4's and AES-128's keys are made from that string all the same.
pub fn one_id(mut pdf: Pdf) -> Pdf {
    let first = pdf.trailer.get(b"ID").unwrap().as_array().unwrap()[0].clone();
    pdf.trailer.set("ID", vec![first]);
    pdf
}

/// `plain` encrypted with RC4-128, AES-128 and AES-256, each with its
/// name.
pub fn rc4_aes_128_and_aes_256(plain: &Pdf) -> [(&'static str, EncryptionState); 3] {
    let std_cf = |filter: Arc<dyn CryptFilter>| BTreeMap::from([(b"StdCF".to_vec(), filter)]);
    let versions = [
        (
            "RC4-128",
            EncryptionVersion::V2 {
                document: plain,
                owner_password: OWNER,
                user_password: USER,
                key_length: 128,
                permissions: Permissions::all(),
            },
        ),
        (
            "AES-128",
            EncryptionVersion::V4 {
                document: plain,
                encrypt_metadata: true,
                crypt_filters: std_cf(Arc::new(Aes128CryptFilter)),
                stream_filter: b"StdCF".to_vec(),
                string_filter: b"StdCF".to_vec(),
                owner_password: OWNER,
                user_password: USER,
                permissions: Permissions::all(),
            },
        ),
        (
            "AES-256",
            EncryptionVersion::V5 {
                encrypt_metadata: true,
                crypt_filters: std_cf(Arc::new(Aes256CryptFilter)),
                file_encryption_key: &[7; 32],
                stream_filter: b"StdCF".to_vec(),
                string_filter: b"StdCF".to_vec(),
                owner_password: OWNER,
                user_password: USER,
                permissions: Permissions::all(),
            },
        ),
    ];
    versions.map(|(name, version)| (name, EncryptionState::try_from(version).unwrap()))
}

/// V4 with AES-128 for embedded files only: `/StmF` and `/StrF` are
/// `/Identity`, and `/EFF` names `/CF`'s `/StdCF`.
pub fn attachment_only(plain: &Pdf) -> EncryptionState {
    let mut state = EncryptionState::try_from(EncryptionVersion::V4 {
        document: plain,
        encrypt_metadata: true,
        crypt_filters: BTreeMap::from([(
            b"StdCF".to_vec(),
            Arc::new(Aes128CryptFilter) as Arc<dyn CryptFilter>,
        )]),
        stream_filter: b"Identity".to_vec(),
        string_filter: b"Identity".to_vec(),
        owner_password: OWNER,
        user_password: USER,
        permissions: Permissions::all(),
    })
    .unwrap();
    state.set_embedded_file_filter(b"StdCF".to_vec());
    state
}

/// `state`'s `/Encrypt` dictionary as Acrobat writes it: lopdf's, with
/// AES-256's `/Length 256`, which lopdf leaves out (M11) and qpdf warns
/// of, and poppler garbles every stream without.
pub fn encrypt_dict(state: &EncryptionState) -> Dictionary {
    let mut dict = state.encode().unwrap();
    if dict.get(b"V").and_then(Object::as_i64).ok() == Some(5) {
        dict.set("Length", 256);
    }
    dict
}

/// `plain` encrypted with `state` and [`written`]. The `/Encrypt`
/// dictionary, [`encrypt_dict`]'s, gets `edit`'s changes first.
pub fn encrypted(
    plain: &Pdf,
    state: &EncryptionState,
    edit: impl Fn(&mut Dictionary),
) -> [Vec<u8>; 2] {
    let mut pdf = plain.clone();
    pdf.encrypt(state).unwrap();
    let id = pdf.trailer.get(b"Encrypt").unwrap().as_reference().unwrap();
    let dict = pdf.get_dictionary_mut(id).unwrap();
    *dict = encrypt_dict(state);
    edit(dict);
    written(&pdf)
}

/// `pdf` as lopdf writes it, with a cross-reference table and with a
/// stream.
pub fn written(pdf: &Pdf) -> [Vec<u8>; 2] {
    [
        XrefType::CrossReferenceTable,
        XrefType::CrossReferenceStream,
    ]
    .map(|kind| {
        let mut pdf = pdf.clone();
        pdf.reference_table.cross_reference_type = kind;
        let mut bytes = Vec::new();
        pdf.save_to(&mut bytes).unwrap();
        bytes
    })
}

/// `pdf` encrypted with `state` the way a writer that packs objects into
/// an object stream does, which lopdf's `save_modern` gets wrong (it
/// encrypts each member under its own number, in a container it leaves
/// plain): every object but the streams in one object stream, plain
/// inside it, the container encrypted whole; each stream encrypted under
/// its own number; the `/Encrypt` dictionary, [`encrypt_dict`]'s; and a
/// cross-reference stream, which isn't encrypted, listing each member by
/// its place in the container. `pdf`'s trailer gives `/Root`, `/Info` and
/// `/ID`.
pub fn object_streams_encrypted(pdf: &Pdf, state: &EncryptionState) -> Vec<u8> {
    let container = pdf.max_id + 1;
    let (encrypt, xref) = (container + 1, container + 2);
    // Each number's row: its type, then its offset or container, then its
    // generation or place.
    let mut rows: BTreeMap<u32, (u8, u64, u16)> = BTreeMap::from([(0, (0, 0, u16::MAX))]);
    let (mut pairs, mut members) = (String::new(), Vec::new());
    let mut packed = 0;
    for (&(num, generation), obj) in &pdf.objects {
        if matches!(obj, Object::Stream(_)) {
            continue;
        }
        assert_eq!(generation, 0, "an object stream holds generation 0 only");
        pairs.push_str(&format!("{num} {} ", members.len()));
        object::write(&mut members, obj).unwrap();
        members.push(b'\n');
        rows.insert(num, (2, u64::from(container), packed));
        packed += 1;
    }
    let objstm = Stream::new(
        dictionary! {
            "Type" => "ObjStm",
            "N" => i64::from(packed),
            "First" => pairs.len() as i64,
        },
        [pairs.into_bytes(), members].concat(),
    );

    let mut out = format!("%PDF-{}\n", pdf.version).into_bytes();
    out.extend_from_slice(full::BINARY);
    let streams = pdf
        .objects
        .iter()
        .filter(|(_, obj)| matches!(obj, Object::Stream(_)))
        .map(|(&id, obj)| (id, obj.clone()))
        .chain([((container, 0), Object::Stream(objstm))]);
    for (id, mut obj) in streams {
        encryption::encrypt_object(state, id, &mut obj).unwrap();
        let pos = object::indirect(&mut out, id, &obj).unwrap();
        rows.insert(id.0, (1, pos as u64, id.1));
    }
    let dict = Object::Dictionary(encrypt_dict(state));
    let pos = object::indirect(&mut out, (encrypt, 0), &dict).unwrap();
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
        "Type" => "XRef",
        "Size" => i64::from(xref + 1),
        "W" => vec![1.into(), 4.into(), 2.into()],
        "Encrypt" => (encrypt, 0),
    };
    for key in [&b"Root"[..], b"Info", b"ID"] {
        if let Ok(value) = pdf.trailer.get(key) {
            trailer.set(key, value.clone());
        }
    }
    object::indirect(
        &mut out,
        (xref, 0),
        &Object::Stream(Stream::new(trailer, data)),
    )
    .unwrap();
    out.extend_from_slice(format!("startxref\n{start}\n%%EOF\n").as_bytes());
    out
}

/// Bytes some writers put before `%PDF-`.
pub const PREFIX: [u8; 100] = [b'~'; 100];

/// Whether `bytes` hold `text` anywhere.
pub fn holds(bytes: &[u8], text: &[u8]) -> bool {
    bytes.windows(text.len()).any(|w| w == text)
}

/// `path` opened as the app opens it, with `password`, its first page
/// turned a quarter and a note put on it.
pub fn edited(path: &Path, password: Option<&str>) -> Snapshot {
    let mut doc = crate::open::file(path, password).unwrap().doc;
    let page = doc.page(0).unwrap();
    let turn = Edit::RotatePages {
        ids: vec![page],
        by: Rotation::R90,
    };
    doc.apply(turn, "turn").unwrap();
    doc.apply(
        Edit::AnnotAdd {
            page,
            annot: note(),
        },
        "note",
    )
    .unwrap();
    doc.snapshot()
}
