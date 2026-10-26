//! Encrypted files as sheaf saves them, read back by PDFium (M10.5 slice
//! 7): RC4-40, RC4-128, AES-128 and AES-256 files saved by both strategies
//! stay encrypted with their own password and show what the same edit of
//! the unencrypted file shows, a Rewrite of one whose cross-references
//! are a stream packed into object streams, each encrypted whole (object
//! streams' slice 2); an AES-256 file whose strings are in an encrypted
//! object stream rewrites into object streams of its own; and a file with
//! bytes before its header appends as the same file without them does.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use lopdf::Object;
use sheaf_doc::materialize::How;
use sheaf_doc::serialize::fixture::{
    BOOKMARK, FIELD, FORM_SECRETS, PREFIX, USER, VALUE, edited, encrypted, form, in_clear,
    object_streams_encrypted, object_streams_of, rc4_40, rc4_aes_128_and_aes_256, written,
};
use sheaf_doc::store::BlobKind;
use sheaf_doc::{Src, Store};

use super::bookmarks::bookmarks;
use super::*;

/// What PDFium shows of a file: its pages, its first page drawn (its size
/// and a hash of its pixels), that page's text and annotations, the
/// form's values by field name, and the bookmarks' titles.
#[derive(Debug, PartialEq)]
struct Shown {
    pages: u32,
    size: (u32, u32),
    pixels: u128,
    text: String,
    annots: usize,
    fields: BTreeMap<String, Option<String>>,
    titles: Vec<String>,
}

fn shown(bytes: &[u8], password: Option<&str>) -> Shown {
    let engine = PdfiumEngine::open_bytes(bytes.into(), password).unwrap();
    let page = engine
        .render_page(PageIndex(0), 400, PixelFormat::Rgba)
        .unwrap();
    // Its characters: `page_text` asks for the text inside the page's
    // shown size, in its unturned space (pdfium-render's
    // `PdfPageText::all`), which a turned page's text can be past.
    let text = engine
        .page_chars(PageIndex(0))
        .unwrap()
        .into_iter()
        .map(|c| c.ch)
        .collect();
    let (annots, fields) = {
        let _pdfium = lock();
        let pages = engine.doc.pages();
        let annots = pages.get(0).unwrap().annotations().len();
        let fields = engine
            .doc
            .form()
            .map(|form| form.field_values(pages).into_iter().collect())
            .unwrap_or_default();
        (annots, fields)
    };
    Shown {
        pages: engine.page_count(),
        size: (page.width, page.height),
        pixels: sheaf_util::checksum(&page.data),
        text,
        annots,
        fields,
        titles: bookmarks(&engine).into_iter().map(|b| b.title).collect(),
    }
}

/// [`form`]'s field and its value, as PDFium gives them.
fn form_fields() -> BTreeMap<String, Option<String>> {
    BTreeMap::from([(FIELD.to_string(), Some(VALUE.to_string()))])
}

/// The cross-reference kinds [`written`] writes its two files in.
const KINDS: [&str; 2] = ["table", "stream"];

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("sheaf-encrypted-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// `bytes` as file `name` in `dir`.
fn put(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

/// An encrypted file, opened with its password.
fn source(bytes: &[u8]) -> Src {
    let kind = BlobKind::file(Path::new("encrypted.pdf"), Some(USER));
    Store::new().open(bytes.into(), kind).unwrap()
}

/// An encrypted file's `/P`, as its `/Encrypt` stores it, and `/ID[0]`.
fn kept(bytes: &[u8]) -> (Object, Vec<u8>) {
    let src = source(bytes);
    let crypt = src.crypt().expect("encrypted");
    let p = crypt.dict().get(b"P").unwrap().clone();
    (p, crypt.first_id().expect("an /ID").to_vec())
}

/// Whether an encrypted file holds object streams, read as objects.
fn packed(bytes: &[u8]) -> bool {
    !object_streams_of(source(bytes).pdf()).is_empty()
}

sheaf_doc::both_ways!(
    encrypted_files_reopen_in_pdfium_by_both_strategies,
    encrypted_files_reopen_in_pdfium_by_both_strategies_appended: reopen_in_pdfium
);

/// RC4-40, RC4-128, AES-128 and AES-256 files, with a table and with a
/// stream, turned and noted and saved by both strategies: no output opens
/// without the password or says anything of the form in the clear, a
/// stream's Rewrite is packed, each shows with the password what the same
/// edit of the unencrypted file shows, and each keeps its `/P` and
/// `/ID[0]`.
fn reopen_in_pdfium(how: How) {
    let dir = scratch(&format!("both-{how:?}"));
    let plain = form();
    let states = std::iter::once(rc4_40(&plain)).chain(rc4_aes_128_and_aes_256(&plain));
    for (version, state) in states {
        let files = encrypted(&plain, &state, |_| {})
            .into_iter()
            .zip(written(&plain));
        for ((source, original), kind) in files.zip(KINDS) {
            let path = put(&dir, &format!("{version}-{kind}.pdf"), &source);
            let original = put(&dir, &format!("plain-{kind}.pdf"), &original);
            let (was, edit, plain_edit) = (
                kept(&source),
                edited(&path, Some(USER)),
                edited(&original, None),
            );
            let name = format!("{version}, {kind}, {how:?}");
            let bytes = how.write(&edit);
            for text in FORM_SECRETS {
                assert!(
                    !in_clear(&bytes, text),
                    "{name}: {:?}",
                    String::from_utf8_lossy(text)
                );
            }
            let packs = how == How::Rewritten && kind == "stream";
            assert_eq!(packed(&bytes), packs, "{name}");
            let refused = PdfiumEngine::open_bytes(bytes.as_slice().into(), None)
                .err()
                .expect("opened without the password");
            assert!(OpenError::is_password(&refused), "{name}: {refused:#}");
            // What the same edit of the unencrypted file shows: the
            // turn, the note, the field and the bookmark.
            let want = shown(&how.write(&plain_edit), None);
            assert!(want.size.0 > want.size.1, "{name}: {want:?}");
            assert!(want.text.contains("Page 1"), "{name}: {want:?}");
            assert_eq!(want.annots, 2, "{name}");
            assert_eq!(want.fields, form_fields(), "{name}");
            assert_eq!(want.titles, [BOOKMARK], "{name}");
            assert_eq!(shown(&bytes, Some(USER)), want, "{name}");
            assert_eq!(kept(&bytes), was, "{name}");
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// An AES-256 file whose field and bookmark are in an object stream,
/// encrypted whole, rewrites into object streams of its own, with nothing
/// in the clear, and shows what it showed.
#[test]
fn an_aes256_object_stream_base_rewrites() {
    let dir = scratch("object-streams");
    let plain = form();
    let [.., (_, aes_256)] = rc4_aes_128_and_aes_256(&plain);
    let base = object_streams_encrypted(&plain, &aes_256);
    assert!(packed(&base));
    let path = put(&dir, "object-streams.pdf", &base);
    let doc = sheaf_doc::open::file(&path, Some(USER)).unwrap().doc;
    let bytes = sheaf_doc::save::rewrite(&doc).unwrap();
    assert!(packed(&bytes));
    for text in FORM_SECRETS {
        assert!(
            !in_clear(&bytes, text),
            "{:?}",
            String::from_utf8_lossy(text)
        );
    }
    let rewritten = shown(&bytes, Some(USER));
    assert_eq!(rewritten.fields, form_fields());
    assert_eq!(rewritten.titles, [BOOKMARK]);
    assert_eq!(rewritten, shown(&base, Some(USER)));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A file with 100 bytes before its header appends as it does without
/// them, as PDFium reads it. PDFium rebuilds cross-references it can't
/// follow, so the positions themselves are checked by readers that
/// don't: `LazyPdf` (sheaf-doc's `serialize::tests`) and qpdf (sheaf-ops'
/// interop tests).
#[test]
fn a_prefixed_file_appends_as_pdfium_reads_it() {
    let dir = scratch("prefixed");
    let plain = form();
    for (file, kind) in written(&plain).into_iter().zip(KINDS) {
        let prefixed = [&PREFIX[..], &file].concat();
        let append = |name: &str, bytes: &[u8]| {
            let path = put(&dir, &format!("{name}-{kind}.pdf"), bytes);
            How::Appended.write(&edited(&path, None))
        };
        let (appended, after_prefix) = (append("file", &file), append("prefixed", &prefixed));
        assert!(after_prefix.starts_with(&prefixed), "{kind}");
        // The update is read: the page turned, with a note.
        let want = shown(&appended, None);
        assert!(want.size.0 > want.size.1, "{kind}: {want:?}");
        assert_eq!(want.annots, 2, "{kind}");
        assert_eq!(shown(&after_prefix, None), want, "{kind}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
