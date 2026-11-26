//! A form filled through `SetFieldValues` and saved (M10 slice 9), each of
//! the slice's checks by its words:
//!
//! - the corpus's 1040 and W-9 filled (two text fields and a check box:
//!   neither has a choice field), saved by a Rewrite, by an Append and as
//!   planned, and reopened: the values read back, the form check is clean,
//!   and PDFium draws each page appearances only as it draws its own
//!   appearances of the same values; after the fill their `/XFA` and
//!   `/Perms` are gone; and a value the next fill replaced is nowhere in
//!   the file that save writes;
//! - fill, Rewrite, fill another field, Append, reopen: both values, one
//!   object a field; the same after Save As, and after undoing past a
//!   save then saving;
//! - a file whose revisions filled a field and cleared it, then a fill of
//!   another field: no trace of the first value after the next save;
//! - a signed hybrid form: a fill's plan asks, offers Append anyway and
//!   writes nothing unasked; taken, the signature still fits; a field the
//!   signature locks is `Locked`;
//! - a form with only a permissions password, and one with an open
//!   password: encrypted after a fill saved both ways and after a recovery
//!   restore, the restored document holding the value and its sidecar
//!   holding it only as ciphertext; and an unencrypted fill restored from
//!   its sidecar holds its value and its appearance;
//! - undoing back to the saved version makes the next save an Append
//!   again: a replaced value plans a Rewrite, undone it plans nothing,
//!   and a fill of an empty field after it appends;
//! - a form whose usage rights allow filling in, with no XFA: a fill of
//!   an empty field appends, and `/Perms` stays;
//! - what a fill's Append writes, by object.
//!
//! **Every "nowhere in the file" is one search** ([`search`]), run first
//! on a file that holds the value, where it must find it: a miss with no
//! such control would prove nothing. It looks for the value in
//! PDFDocEncoding and UTF-16BE, in the bytes as they are (a string written
//! in hex too), in every stream's data inflated, in every string and
//! decoded stream of every revision's objects (object streams' members
//! among them, decrypted with the file's password), and as the text an
//! appearance's strings show through their fonts, which a font's codes
//! hold where its encoding isn't the value's own bytes.
//! [`the_search_finds_a_value_wherever_a_file_holds_it`] holds each of
//! those ways to a file that holds the value only so.
//!
//! The corpus checks skip without the corpus. Needs libpdfium
//! (`scripts/fetch-pdfium.sh`), like the app itself.

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::ops::ControlFlow;
use std::path::Path;
use std::sync::Arc;

use sheaf_doc::content::interp::{Forms, Kind, Opts, Wants, interpret};
use sheaf_doc::content::{Concat, Fonts};
use sheaf_doc::forms::fixture::{Form, append_revision, detached, row};
use sheaf_doc::forms::{Finding, Value, check};
use sheaf_doc::materialize::{
    Ask, AskAnswers, How, SaveHow, SaveReport, SaveStop, Strategy, StrategyKind, Why, plan,
    save_with,
};
use sheaf_doc::pdf::test_crypt::{CipherKind, TestEncryption};
use sheaf_doc::pdf::{
    Document as Pdf, LazyPdf, Object, ObjectId, Permissions, ReadLimits, Stream, StreamExt,
    Value as PdfValue, dictionary, inflate, literal,
};
use sheaf_doc::tx::fixture as fx;
use sheaf_doc::{AnnotId, Document, EditError, Store};
use sheaf_ops::NoProgress;
use sheaf_render::pdfium::PdfiumEngine;
use sheaf_render::{PageIndex, Renderer};

use common::fields::{TOLERANCE, differing_in, flat_at, live_at, one_other};
use common::{
    appended_objects, clean, corpus_form, form_in, open, opened, planned, saved, scratch, values,
};

// ----- The search --------------------------------------------------------

/// How [`search`] reads a file.
#[derive(Clone, Copy, Debug)]
enum Read<'a> {
    /// As its bytes are, nothing decrypted or parsed: the bytes, a string
    /// written in hex among them, and each stream's data inflated. What
    /// "only as ciphertext" asks of an encrypted file, and all there is
    /// to read of bytes that aren't a file (a recovery sidecar's
    /// sections).
    Clear,
    /// As [`Read::Clear`] does, and as the file reads: every revision's
    /// objects, decrypted with this password (none: the empty one), each
    /// string and decoded stream, and what each appearance shows.
    Decoded(Option<&'a str>),
}

/// Where [`search`] found a value.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct Found {
    /// In the bytes as they are: its own, or a string written in hex.
    bytes: bool,
    /// In a stream's data, as it inflates.
    inflated: bool,
    /// In an object of a revision as the file reads: a string, or a
    /// stream decoded.
    objects: bool,
    /// In the text an appearance's strings show through its fonts.
    shown: bool,
}

impl Found {
    fn any(self) -> bool {
        self.bytes || self.inflated || self.objects || self.shown
    }
}

/// Whether `value` is anywhere in `bytes`, read as `read` says
/// ([`search`]).
fn found(bytes: &[u8], value: &str, read: Read) -> bool {
    search(bytes, value, read).any()
}

/// Where `value` is in `bytes`, read as `read` says: in PDFDocEncoding and
/// in UTF-16BE ([`forms_of`]), in the bytes as they are and in hex, in
/// each stream's data inflated, and, decoded, in every string and stream
/// of every revision's objects and as what every appearance's strings
/// show.
fn search(bytes: &[u8], value: &str, read: Read) -> Found {
    let forms = forms_of(value);
    let hexes: Vec<Vec<u8>> = (forms.iter())
        .flat_map(|f| [hex(f, b"0123456789ABCDEF"), hex(f, b"0123456789abcdef")])
        .collect();
    let has = |hay: &[u8]| forms.iter().any(|f| holds(hay, f));
    let mut found = Found {
        bytes: has(bytes) || hexes.iter().any(|h| holds(bytes, h)),
        inflated: (stream_data(bytes).iter())
            .any(|data| inflate(data, ReadLimits::DEFAULT.max_decompressed).is_ok_and(|p| has(&p))),
        ..Found::default()
    };
    let Read::Decoded(password) = read else {
        return found;
    };
    // An earlier revision that doesn't read alone is passed by; the file
    // itself must read, or a miss would prove nothing.
    for end in revision_ends(bytes) {
        let prefix = &bytes[..end];
        let pdf = match password {
            Some(password) => Pdf::load_mem_with_password(prefix, password),
            None => Pdf::load_mem(prefix),
        };
        let pdf = match pdf {
            Ok(pdf) => pdf,
            Err(e) if end == bytes.len() => panic!("the file doesn't read: {e}"),
            Err(_) => continue,
        };
        found.objects |= pdf.objects.values().any(|o| object_holds(o, &has));
        found.shown |= shows(&pdf, value);
    }
    found
}

/// `value` as a file may hold it: in PDFDocEncoding, where it has a byte
/// for every character, and in UTF-16BE (a string of it holds these after
/// its byte order mark).
fn forms_of(value: &str) -> Vec<Vec<u8>> {
    let utf16: Vec<u8> = value.encode_utf16().flat_map(u16::to_be_bytes).collect();
    pdf_doc(value).into_iter().chain([utf16]).collect()
}

/// The characters PDFDocEncoding (ISO 32000-2, Annex D, Table D.2) gives
/// a byte that isn't the character's own code: the accents at 0x18 to
/// 0x1F and the marks at 0x80 to 0xA0 (0x9F has none).
const PDF_DOC_HIGH: [(u8, char); 40] = [
    (0x18, '\u{2d8}'),
    (0x19, '\u{2c7}'),
    (0x1a, '\u{2c6}'),
    (0x1b, '\u{2d9}'),
    (0x1c, '\u{2dd}'),
    (0x1d, '\u{2db}'),
    (0x1e, '\u{2da}'),
    (0x1f, '\u{2dc}'),
    (0x80, '\u{2022}'),
    (0x81, '\u{2020}'),
    (0x82, '\u{2021}'),
    (0x83, '\u{2026}'),
    (0x84, '\u{2014}'),
    (0x85, '\u{2013}'),
    (0x86, '\u{192}'),
    (0x87, '\u{2044}'),
    (0x88, '\u{2039}'),
    (0x89, '\u{203a}'),
    (0x8a, '\u{2212}'),
    (0x8b, '\u{2030}'),
    (0x8c, '\u{201e}'),
    (0x8d, '\u{201c}'),
    (0x8e, '\u{201d}'),
    (0x8f, '\u{2018}'),
    (0x90, '\u{2019}'),
    (0x91, '\u{201a}'),
    (0x92, '\u{2122}'),
    (0x93, '\u{fb01}'),
    (0x94, '\u{fb02}'),
    (0x95, '\u{141}'),
    (0x96, '\u{152}'),
    (0x97, '\u{160}'),
    (0x98, '\u{178}'),
    (0x99, '\u{17d}'),
    (0x9a, '\u{131}'),
    (0x9b, '\u{142}'),
    (0x9c, '\u{153}'),
    (0x9d, '\u{161}'),
    (0x9e, '\u{17e}'),
    (0xa0, '\u{20ac}'),
];

/// `value` in PDFDocEncoding, or none where it has a character that
/// encoding has no byte for: a tab, a line feed, a carriage return and
/// printable ASCII are their own bytes, Latin-1's from 0xA1 are too (but
/// 0xAD, which it leaves undefined), and the rest are [`PDF_DOC_HIGH`]'s.
fn pdf_doc(value: &str) -> Option<Vec<u8>> {
    (value.chars())
        .map(|c| match c {
            '\t' | '\n' | '\r' | ' '..='~' => Some(c as u8),
            '\u{a1}'..='\u{ff}' if c != '\u{ad}' => Some(c as u32 as u8),
            _ => (PDF_DOC_HIGH.iter())
                .find(|&&(_, high)| high == c)
                .map(|&(b, _)| b),
        })
        .collect()
}

/// `bytes` written in hex with `digits`.
fn hex(bytes: &[u8], digits: &[u8; 16]) -> Vec<u8> {
    (bytes.iter())
        .flat_map(|&b| [digits[usize::from(b >> 4)], digits[usize::from(b & 15)]])
        .collect()
}

fn holds(hay: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && hay.windows(needle.len()).any(|w| w == needle)
}

/// Each stream's data in `bytes`, by its keywords: what follows `stream`
/// and its end of line, to the `endstream` after it.
fn stream_data(bytes: &[u8]) -> Vec<&[u8]> {
    (bytes.windows(6).enumerate())
        .filter(|&(at, w)| w == b"stream" && !bytes[..at].ends_with(b"end"))
        .filter_map(|(at, _)| {
            let data = &bytes[at + 6..];
            let data = data.strip_prefix(b"\r").unwrap_or(data);
            let data = data.strip_prefix(b"\n")?;
            let end = data.windows(9).position(|w| w == b"endstream")?;
            Some(&data[..end])
        })
        .collect()
}

/// Where each revision of `bytes` ends: after each `%%EOF` and the end of
/// line after it, and the end of the bytes, each once, in order.
fn revision_ends(bytes: &[u8]) -> Vec<usize> {
    let mut ends: Vec<usize> = (bytes.windows(5).enumerate())
        .filter(|&(_, w)| w == b"%%EOF")
        .map(|(at, _)| {
            let after = &bytes[at + 5..];
            let eol = after.iter().take_while(|&&b| b == b'\r' || b == b'\n');
            at + 5 + eol.count()
        })
        .collect();
    ends.push(bytes.len());
    ends.dedup();
    ends
}

/// Whether `o` holds what `has` looks for: in a string, or a stream's
/// dictionary or its data decoded (as it's stored, where it won't decode).
fn object_holds(o: &Object, has: &dyn Fn(&[u8]) -> bool) -> bool {
    match o.value() {
        PdfValue::String(s, _) => s.as_bytes().is_some_and(has),
        PdfValue::Array(a) => a.iter().any(|o| object_holds(o, has)),
        PdfValue::Dict(d) => d.iter().any(|(_, o)| object_holds(o, has)),
        PdfValue::Stream(s) => {
            s.dict.iter().any(|(_, o)| object_holds(o, has))
                || has(&(s.decompressed_content()).unwrap_or_else(|_| s.raw_bytes().to_vec()))
        }
        _ => false,
    }
}

/// Whether a form XObject of `pdf`, an appearance or any other, shows
/// `value`: the strings its content shows, decoded through the fonts its
/// `/Resources` name by sheaf's content interpreter, joined in the order
/// shown (a comb's cells, a value in two fonts, are one text).
fn shows(pdf: &Pdf, value: &str) -> bool {
    let lim = sheaf_doc::source::Limits::DEFAULT;
    let fonts = Fonts::for_text();
    pdf.objects.values().any(|o| {
        let Some(stream) = o.as_stream() else {
            return false;
        };
        let form = stream.dict.get(b"Subtype").and_then(Object::as_name) == Some(b"Form");
        let Some(content) = form.then(|| stream.decompressed_content().ok()).flatten() else {
            return false;
        };
        let opts = Opts {
            forms: Forms::Skip,
            wants: Wants {
                text: true,
                ..Wants::NONE
            },
        };
        let mut text = String::new();
        let resources = stream.dict.get(b"Resources");
        let concat = Concat::new(&[&content]);
        // What it showed before any error counts.
        let _ = interpret(pdf, &concat, resources, &lim, &fonts, opts, |event| {
            if let Kind::Text(show) = event.kind
                && let Some(shown) = show.font.and_then(|f| f.text(show.string))
            {
                text.push_str(&shown);
            }
            ControlFlow::Continue(())
        });
        text.contains(value)
    })
}

/// The search finds a value in each way a file holds one, each alone: a
/// literal string; a text string in UTF-16BE, written in hex; a member of
/// a compressed object stream; an encrypted file's string, only once it's
/// decrypted; and a value drawn in two fonts in a field's appearance, the
/// field's `/V` taken out, which is in no string or stream as its text,
/// only as the fonts' codes. A value the file doesn't hold is found
/// nowhere.
#[test]
fn the_search_finds_a_value_wherever_a_file_holds_it() {
    // A literal string: in the bytes, and as the file reads.
    let mut f = Form::new(1);
    let a = f.text("a", 0, row(0));
    f.dict(a).set("V", literal("Literal7"));
    let plain = f.bytes();
    assert_eq!(
        search(&plain, "Literal7", Read::Decoded(None)),
        Found {
            bytes: true,
            objects: true,
            ..Found::default()
        }
    );
    assert_eq!(
        search(&plain, "Literal7", Read::Clear),
        Found {
            bytes: true,
            ..Found::default()
        }
    );
    assert!(!found(&plain, "Elsewhere8", Read::Decoded(None)));
    // UTF-16BE, written in hex.
    let mut f = Form::new(1);
    let a = f.text("a", 0, row(0));
    f.dict(a)
        .set("V", sheaf_doc::pdf::text::string("\u{141}\u{f3}d\u{17a} 9"));
    let utf16 = f.bytes();
    assert!(!holds(&utf16, "\u{141}\u{f3}d\u{17a} 9".as_bytes()));
    let got = search(&utf16, "\u{141}\u{f3}d\u{17a} 9", Read::Decoded(None));
    assert!(got.bytes && got.objects, "{got:?}");
    // In a compressed object stream: inflated, and as the file reads.
    let mut f = Form::new(1);
    let a = f.text("a", 0, row(0));
    f.dict(a).set("V", literal("Packed10"));
    let mut packed = Vec::new();
    f.pdf.save_packed(&mut packed, 100).unwrap();
    let limits = &sheaf_doc::source::Limits::DEFAULT.reader;
    let lazy = LazyPdf::open(Arc::<[u8]>::from(&packed[..]), None, limits).unwrap();
    assert!(!sheaf_doc::serialize::fixture::object_streams_of(&lazy).is_empty());
    assert!(!holds(&packed, b"Packed10"), "packed: not as it is");
    let got = search(&packed, "Packed10", Read::Decoded(None));
    assert_eq!(
        got,
        Found {
            inflated: true,
            objects: true,
            ..Found::default()
        }
    );
    // Encrypted: only decrypted.
    let mut f = Form::new(1);
    let a = f.text("a", 0, row(0));
    f.dict(a).set("V", literal("Cipher11"));
    let encrypted = encrypted_bytes(f, "secret", Permissions::ALL);
    assert!(!found(&encrypted, "Cipher11", Read::Clear));
    assert!(!found(&encrypted, "Cipher11", Read::Decoded(None)));
    let got = search(&encrypted, "Cipher11", Read::Decoded(Some("secret")));
    assert!(got.objects, "{got:?}");
    // In two fonts' codes alone: "Łukasz", its `Ł` in the fallback font
    // and the rest in Helvetica.
    let value = "\u{141}ukasz";
    let mut f = Form::new(1);
    f.text("name", 0, row(0));
    let mut doc = opened(f.bytes(), "fonts.pdf", None);
    fx::fill(&mut doc, "name", value);
    let filled = sheaf_doc::save::rewrite(&doc).unwrap();
    assert!(found(&filled, value, Read::Decoded(None)));
    let mut pdf = Pdf::load_mem(&filled).unwrap();
    let field = form_in(&filled).fields[0].object;
    pdf.get_dictionary_mut(field).unwrap().remove(b"V");
    let mut drawn_only = Vec::new();
    pdf.save_to(&mut drawn_only).unwrap();
    assert_eq!(
        search(&drawn_only, value, Read::Decoded(None)),
        Found {
            shown: true,
            ..Found::default()
        },
        "only as the fonts' codes"
    );
}

/// `f`'s file encrypted with AES-128, `user` its user password (empty for
/// a file that opens with none) and `owner` its owner's, allowing
/// `permissions`.
fn encrypted_bytes(mut f: Form, user: &str, permissions: Permissions) -> Vec<u8> {
    let id = literal("0123456789abcdef");
    f.pdf.trailer.set("ID", vec![id.clone(), id]);
    let state =
        TestEncryption::new(CipherKind::Aes128, user, "owner").with_permissions(permissions);
    f.pdf.encrypt(&state).unwrap();
    f.bytes()
}

// ----- What the checks share ---------------------------------------------

/// The answers a save that asks is given where the test means to save:
/// signatures broken, an XFA part's values given up.
fn told() -> AskAnswers {
    AskAnswers {
        break_signatures: true,
        drop_xfa_values: true,
        ..Default::default()
    }
}

/// `doc` saved to `path` as its plan says, with `answers`.
fn save_planned(
    doc: &mut Document,
    path: &Path,
    answers: AskAnswers,
) -> Result<SaveReport, SaveStop> {
    save_with(doc, path, SaveHow::Planned, &answers, &NoProgress)
}

/// What a save of `doc`'s current version to `path` would do.
fn plan_of(doc: &Document, path: &Path) -> sheaf_doc::materialize::SavePlan {
    plan(&doc.recovery_input(), &doc.save_target(path))
}

/// The value `doc` holds for the field whose full name is `name`.
fn value_of(doc: &Document, name: &str) -> Option<Value> {
    let st = doc.state();
    let id = (st.field_named(name)).unwrap_or_else(|| panic!("no field {name}"));
    st.field(id)
        .and_then(|f| f.term.as_deref())
        .and_then(|t| t.value.clone())
}

/// The widgets of the field of `doc` whose full name is `name`.
fn widgets_of(doc: &Document, name: &str) -> Vec<AnnotId> {
    let st = doc.state();
    let id = (st.field_named(name)).unwrap_or_else(|| panic!("no field {name}"));
    let term = st.field(id).and_then(|f| f.term.clone()).unwrap();
    term.widgets.iter().copied().collect()
}

fn text(value: &str) -> Option<Value> {
    Some(Value::Text(value.into()))
}

/// The form's `/XFA` and the catalog's `/Perms`, as there or not.
fn xfa_and_perms(bytes: &[u8]) -> (bool, bool) {
    let pdf = Pdf::load_mem(bytes).unwrap();
    let catalog = pdf.catalog().unwrap();
    let form = (catalog.get(b"AcroForm"))
        .and_then(|f| pdf.dereference(f).ok())
        .and_then(|(_, f)| f.as_dict());
    (form.is_some_and(|f| f.has(b"XFA")), catalog.has(b"Perms"))
}

/// Whether the file's signatures fit it: each `/ByteRange` ends where a
/// revision ends.
fn signature_fits(bytes: &[u8]) -> bool {
    !(check(bytes, None).unwrap().findings.iter())
        .any(|f| matches!(f, Finding::StaleSignature { .. }))
}

/// Each field of `bytes`, a form whose fields are at its root (so a full
/// name is a `/T`), is one object: every full name the reader reads is one
/// field's, every field's object is its own, and of the file's latest
/// objects exactly one is a field dictionary of each name (none left
/// behind at another number). The fields' objects, by full name.
fn one_object_a_field(bytes: &[u8], what: &str) -> BTreeMap<String, ObjectId> {
    let form = form_in(bytes);
    let by_name: BTreeMap<String, ObjectId> = (form.fields.iter())
        .map(|f| (f.name.clone(), f.object))
        .collect();
    assert_eq!(by_name.len(), form.fields.len(), "{what}: a name twice");
    let objects: BTreeSet<ObjectId> = by_name.values().copied().collect();
    assert_eq!(objects.len(), by_name.len(), "{what}: an object twice");
    let pdf = Pdf::load_mem(bytes).unwrap();
    for (name, id) in &by_name {
        let named: Vec<ObjectId> = (pdf.objects.iter())
            .filter(|(_, o)| {
                (o.as_dict()).is_some_and(|d| {
                    d.has(b"FT")
                        && d.get(b"T").map(sheaf_doc::pdf::text::of).as_deref() == Some(name)
                })
            })
            .map(|(&id, _)| id)
            .collect();
        assert_eq!(named, [*id], "{what}: {name}'s dictionaries");
    }
    by_name
}

// ----- The 1040 and the W-9 ----------------------------------------------

/// A corpus IRS form and what the checks fill in it: two text fields and
/// a check box, by full name, all on its first page.
struct Irs {
    path: &'static str,
    texts: [&'static str; 2],
    check_box: &'static str,
}

/// The 1040's first and last name, and its first filing status box.
const F1040: Irs = Irs {
    path: "forms/irs/f1040.pdf",
    texts: [
        "topmostSubform[0].Page1[0].f1_14[0]",
        "topmostSubform[0].Page1[0].f1_15[0]",
    ],
    check_box: "topmostSubform[0].Page1[0].c1_1[0]",
};

/// The W-9's name and business name, and its first tax classification
/// box.
const FW9: Irs = Irs {
    path: "forms/irs/fw9.pdf",
    texts: [
        "topmostSubform[0].Page1[0].f1_01[0]",
        "topmostSubform[0].Page1[0].f1_02[0]",
    ],
    check_box: "topmostSubform[0].Page1[0].Boxes3a-b_ReadOrder[0].c1_1[0]",
};

/// What the first text field holds first, which the next fill replaces;
/// what replaces it; and what the second text field holds.
const REPLACED: &str = "Zebulon Quinn";
const REPLACEMENT: &str = "Ann Lee";
const KEPT: &str = "Harbor Works";

sheaf_doc::both_ways!(
    corpus_the_1040_filled_saves_and_reads_back,
    corpus_the_1040_filled_saves_and_reads_back_appended: the_1040_filled
);

fn the_1040_filled(how: How) {
    irs_filled(&F1040, Some(how));
}

#[test]
fn corpus_the_1040_filled_saves_as_planned_and_reads_back() {
    irs_filled(&F1040, None);
}

sheaf_doc::both_ways!(
    corpus_the_w9_filled_saves_and_reads_back,
    corpus_the_w9_filled_saves_and_reads_back_appended: the_w9_filled
);

fn the_w9_filled(how: How) {
    irs_filled(&FW9, Some(how));
}

#[test]
fn corpus_the_w9_filled_saves_as_planned_and_reads_back() {
    irs_filled(&FW9, None);
}

/// `irs` filled through the op ([`REPLACED`] and [`KEPT`], its check box
/// pressed), saved `how` (as planned for none), then its first text field
/// filled again ([`REPLACEMENT`]) and saved as planned, a Rewrite for the
/// replaced text: each save as [`filled_as`] says, and the replaced value
/// in the first (the control) and nowhere in the second.
fn irs_filled(irs: &Irs, how: Option<How>) {
    let Some(original) = corpus_form(irs.path) else {
        return;
    };
    let name = Path::new(irs.path).file_stem().unwrap().to_string_lossy();
    let dir = scratch(&format!("fill-{name}-{how:?}"));
    let path = dir.join(format!("{name}.pdf"));
    std::fs::write(&path, &original).unwrap();
    assert!(
        !found(&original, REPLACED, Read::Decoded(None)),
        "{name}: the form holds the value already"
    );
    let mut doc = open(&path);
    fx::fill(&mut doc, irs.texts[0], REPLACED);
    fx::fill(&mut doc, irs.texts[1], KEPT);
    let widget = widgets_of(&doc, irs.check_box)[0];
    fx::press(&mut doc, widget);
    let Some(Value::State(on)) = value_of(&doc, irs.check_box) else {
        panic!("{name}: the check box isn't on");
    };
    assert_ne!(&*on, "Off", "{name}");
    let (strategy, first) = match how {
        Some(how) => saved(&mut doc, &path, how),
        None => {
            let report = save_planned(&mut doc, &path, told()).unwrap();
            println!(
                "{name}, as planned: {:?} for {:?}",
                report.strategy, report.why
            );
            (report.strategy.unwrap(), std::fs::read(&path).unwrap())
        }
    };
    if let Some(how) = how {
        assert_eq!(strategy, how.strategy(), "{name}");
    }
    let what = format!("{name}, {how:?} ({strategy:?})");
    println!("{what}: {} bytes", first.len());
    let fills = [(irs.texts[0], REPLACED), (irs.texts[1], KEPT)];
    filled_as(&first, &original, irs, &fills, &on, &what);
    assert!(
        found(&first, REPLACED, Read::Decoded(None)),
        "{what}: the control, the search finds the value the next fill replaces"
    );

    fx::fill(&mut doc, irs.texts[0], REPLACEMENT);
    let report = save_planned(&mut doc, &path, told()).unwrap();
    assert_eq!(report.strategy, Some(StrategyKind::Rewrite), "{name}");
    assert!(
        (report.why.iter()).any(|w| matches!(w, Why::ReplacesText(_))),
        "{name}: {:?}",
        report.why
    );
    let second = std::fs::read(&path).unwrap();
    let what = format!("{name}, {how:?} then replaced");
    let fills = [(irs.texts[0], REPLACEMENT), (irs.texts[1], KEPT)];
    filled_as(&second, &original, irs, &fills, &on, &what);
    assert!(
        !found(&second, REPLACED, Read::Decoded(None)),
        "{what}: the replaced value is in the file"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// `bytes`, `original` saved with `fills` (full name, text) and `irs`'s
/// check box in its state `on`: reopened, sheaf reads each value back, and
/// the check box's widget shows `on`; the form check is clean; the form
/// has no `/XFA` and the file no `/Perms`; and PDFium draws it as it draws
/// its own appearances of those values ([`drawn_as_pdfium`]).
fn filled_as(
    bytes: &[u8],
    original: &[u8],
    irs: &Irs,
    fills: &[(&str, &str)],
    on: &str,
    what: &str,
) {
    let read = values(bytes);
    for (name, value) in fills {
        assert_eq!(read.get(*name).map(String::as_str), Some(*value), "{what}");
    }
    let reopened = opened(bytes.to_vec(), "reopened.pdf", None);
    for (name, value) in fills {
        assert_eq!(value_of(&reopened, name), text(value), "{what}: {name}");
    }
    let state = Some(Value::State(on.into()));
    assert_eq!(value_of(&reopened, irs.check_box), state, "{what}");
    let widget = widgets_of(&reopened, irs.check_box)[0];
    let shows = (reopened.state().annot(widget))
        .and_then(|a| a.widget())
        .and_then(|w| w.state.clone());
    assert_eq!(shows.as_deref(), Some(on), "{what}: its /AS");
    clean(bytes, what);
    assert_eq!(xfa_and_perms(bytes), (false, false), "{what}: /XFA, /Perms");
    // PDFium draws its own text appearances alike on Linux alone (the
    // pixel checks' `fields.rs` says why).
    if cfg!(target_os = "linux") {
        drawn_as_pdfium(bytes, original, irs, fills, on, what);
    }
}

/// `bytes`, sheaf's save of `original` with `fills` and the check box `on`,
/// drawn by PDFium appearances only ([`flat_at`], the pixel checks'
/// drawing, two pixels a point), page by page of the filled widgets, as it
/// draws its own appearances of the same values ([`pdfium_s_own`]): alike
/// within the pixel checks' tolerance. Its twin, PDFium's own with the
/// first value one character other ([`one_other`]), is past it, so the
/// check can fail; and each filled widget's box is drawn otherwise than
/// the form as it came.
fn drawn_as_pdfium(
    bytes: &[u8],
    original: &[u8],
    irs: &Irs,
    fills: &[(&str, &str)],
    on: &str,
    what: &str,
) {
    let form = Filled::of(original, irs, fills);
    let twin_value = one_other(fills[0].1);
    let twin_fills = [&[(fills[0].0, twin_value.as_str())], &fills[1..]].concat();
    let engine = |bytes: &[u8]| PdfiumEngine::open_bytes(bytes.to_vec().into(), None).unwrap();
    let [ours, pdfium, twin, was] = [
        engine(bytes),
        engine(&pdfium_s_own(original, &form, fills, on)),
        engine(&pdfium_s_own(original, &form, &twin_fills, on)),
        engine(original),
    ];
    for &page in &form.pages() {
        let index = PageIndex(page);
        let size = ours.page_size(index).unwrap();
        let width = (2.0 * size.width).round() as u32;
        let [drawn, pdfium, twin, was] =
            [&ours, &pdfium, &twin, &was].map(|e| flat_at(e, index, width).unwrap());
        let whole = [0, 0, drawn.width as usize, drawn.height as usize];
        let [apart, twin_apart] = [&pdfium, &twin].map(|r| differing_in(&drawn, r, whole));
        println!(
            "{what}: page {page} {apart} px apart from PDFium's own, {twin_apart} from its twin's"
        );
        assert!(!past(apart), "{what}: page {page}, {apart} px");
        assert!(past(twin_apart), "{what}: page {page}, the twin");
        let scale = drawn.width as f32 / size.width;
        for (name, w) in form.widgets_on(page) {
            let [x0, y0, x1, y1] = w.rect;
            let px = |v: f32| (v * scale) as usize;
            let at = [
                px(x0),
                px(size.height - y1),
                px(x1).min(whole[2]),
                px(size.height - y0).min(whole[3]),
            ];
            let drew = differing_in(&drawn, &was, at);
            assert!(drew > 0, "{what}: {name} draws as it was");
        }
    }
}

/// Whether a count of differing pixels is past the pixel checks'
/// [`TOLERANCE`].
fn past(count: usize) -> bool {
    count > TOLERANCE
}

/// The fields of a corpus form that [`drawn_as_pdfium`] fills, as the file
/// as it came has them: the text fields and the check box, by full name,
/// and each one's widgets with their pages.
struct Filled<'a> {
    fields: Vec<(&'a str, sheaf_doc::forms::Field)>,
    widgets: Vec<(&'a str, u32, sheaf_doc::forms::Widget)>,
    check_box: &'a str,
}

impl<'a> Filled<'a> {
    fn of(original: &[u8], irs: &'a Irs, fills: &[(&'a str, &str)]) -> Filled<'a> {
        let form = form_in(original);
        let names = (fills.iter().map(|&(n, _)| n)).chain([irs.check_box]);
        let fields: Vec<(&str, sheaf_doc::forms::Field)> = names
            .map(|n| {
                (
                    n,
                    (form.fields.iter()).find(|f| f.name == n).unwrap().clone(),
                )
            })
            .collect();
        let widgets = (fields.iter())
            .flat_map(|(n, f)| {
                (form.widgets.iter())
                    .flat_map(|(&page, ws)| ws.iter().map(move |w| (page, w)))
                    .filter(|(_, w)| w.field == f.id)
                    .map(|(page, w)| (*n, page, w.clone()))
                    .collect::<Vec<_>>()
            })
            .collect();
        Filled {
            fields,
            widgets,
            check_box: irs.check_box,
        }
    }

    fn field(&self, name: &str) -> &sheaf_doc::forms::Field {
        &(self.fields.iter()).find(|(n, _)| *n == name).unwrap().1
    }

    fn widgets_of(&self, name: &str) -> impl Iterator<Item = &sheaf_doc::forms::Widget> {
        (self.widgets.iter())
            .filter(move |(n, _, _)| *n == name)
            .map(|(_, _, w)| w)
    }

    fn widgets_on(&self, page: u32) -> impl Iterator<Item = (&str, &sheaf_doc::forms::Widget)> {
        (self.widgets.iter())
            .filter(move |(_, p, _)| *p == page)
            .map(|(n, _, w)| (*n, w))
    }

    /// The pages the widgets are on.
    fn pages(&self) -> BTreeSet<u32> {
        self.widgets.iter().map(|(_, page, _)| *page).collect()
    }
}

/// PDFium's own appearances of `fills` and the check box `on` in
/// `original`: the values written as another tool fills, in a revision of
/// their own (each text field's `/V` and none of its widgets'
/// appearances, the check box's `/V` and its widgets' `/AS`), the pages
/// drawn once under PDFium's form-fill environment, which makes the
/// appearances they lack, and saved by PDFium
/// ([`common::fields::theirs`]'s way).
fn pdfium_s_own(original: &[u8], form: &Filled, fills: &[(&str, &str)], on: &str) -> Vec<u8> {
    let ids: BTreeSet<ObjectId> = (form.fields.iter().map(|(_, f)| f.object))
        .chain(form.widgets.iter().map(|(_, _, w)| w.object))
        .collect();
    let theirs = append_revision(original, &Vec::from_iter(ids), |pdf| {
        for (name, value) in fills {
            let v = sheaf_doc::pdf::text::string(value);
            dict(pdf, form.field(name).object).set("V", v);
            for w in form.widgets_of(name) {
                dict(pdf, w.object).remove(b"AP");
            }
        }
        let state = Object::name(on.as_bytes());
        dict(pdf, form.field(form.check_box).object).set("V", state.clone());
        for w in form.widgets_of(form.check_box) {
            dict(pdf, w.object).set("AS", state.clone());
        }
    });
    let engine = PdfiumEngine::open_bytes(theirs.into(), None).unwrap();
    for page in form.pages() {
        let index = PageIndex(page);
        let width = (2.0 * engine.page_size(index).unwrap().width).round() as u32;
        live_at(&engine, index, width).unwrap();
    }
    engine.save_copy().unwrap()
}

/// Object `id`'s dictionary in `pdf`, to change.
fn dict(pdf: &mut Pdf, id: ObjectId) -> &mut sheaf_doc::pdf::Dictionary {
    pdf.get_dictionary_mut(id).unwrap()
}

// ----- Saves after saves -------------------------------------------------

/// Four empty text fields, `a` to `d`, on one page.
fn four() -> Form {
    let mut f = Form::new(1);
    for (i, name) in ["a", "b", "c", "d"].into_iter().enumerate() {
        f.text(name, 0, row(i));
    }
    f
}

/// The text values `bytes` holds, as `want` lists them by name.
fn holds_values(bytes: &[u8], want: &[(&str, &str)], what: &str) {
    let want: BTreeMap<String, String> = (want.iter())
        .map(|(n, v)| (n.to_string(), v.to_string()))
        .collect();
    assert_eq!(values(bytes), want, "{what}");
}

/// Fill, Rewrite, fill another field, Append, reopen: both values, one
/// object a field, the Append's at the number the Rewrite gave it. The
/// same after Save As (a Rewrite to another file, which is the document's
/// then, the first file untouched) and a fill appended onto it. Undoing
/// past both of those saves and saving again rewrites the file, the two
/// values undone gone from it, by the search that finds them in the file
/// before.
#[test]
fn fills_saved_by_each_strategy_reopen_one_object_a_field() {
    let dir = scratch("fill-saves");
    let path = common::write(&dir, "form.pdf", &mut four());
    let mut doc = open(&path);
    fx::fill(&mut doc, "a", "Alpha1");
    let report = sheaf_doc::materialize::save_rewritten(&mut doc, &path).unwrap();
    assert_eq!(report.strategy, Some(StrategyKind::Rewrite));
    let rewritten = std::fs::read(&path).unwrap();
    let numbers = one_object_a_field(&rewritten, "rewritten");
    fx::fill(&mut doc, "b", "Beta2");
    let (strategy, appended) = planned(&mut doc, &path);
    assert_eq!(strategy, StrategyKind::Append);
    assert!(appended.starts_with(&rewritten));
    holds_values(&appended, &[("a", "Alpha1"), ("b", "Beta2")], "appended");
    assert_eq!(one_object_a_field(&appended, "appended"), numbers);
    let reopened = open(&path);
    assert_eq!(value_of(&reopened, "a"), text("Alpha1"));
    assert_eq!(value_of(&reopened, "b"), text("Beta2"));

    // Save As: a Rewrite to the other file, which is the document's now.
    let other = dir.join("other.pdf");
    fx::fill(&mut doc, "c", "Gamma3");
    let report = save_planned(&mut doc, &other, AskAnswers::default()).unwrap();
    assert_eq!(report.strategy, Some(StrategyKind::Rewrite));
    assert_eq!(report.why, [Why::OtherTarget]);
    assert_eq!(&**doc.file().unwrap().0, other.as_path());
    let saved_as = std::fs::read(&other).unwrap();
    let three = [("a", "Alpha1"), ("b", "Beta2"), ("c", "Gamma3")];
    holds_values(&saved_as, &three, "saved as");
    one_object_a_field(&saved_as, "saved as");
    fx::fill(&mut doc, "d", "Delta4");
    let (strategy, onto) = planned(&mut doc, &other);
    assert_eq!(strategy, StrategyKind::Append);
    assert!(onto.starts_with(&saved_as));
    let all = [three.as_slice(), &[("d", "Delta4")]].concat();
    holds_values(&onto, &all, "appended to the other");
    one_object_a_field(&onto, "appended to the other");
    assert_eq!(std::fs::read(&path).unwrap(), appended, "the first file");

    // Undone past both saves, to the first file's version, and saved.
    for value in ["Gamma3", "Delta4"] {
        assert!(found(&onto, value, Read::Decoded(None)), "the control");
    }
    doc.undo().unwrap();
    doc.undo().unwrap();
    assert_eq!(value_of(&doc, "c"), None);
    assert!(doc.is_dirty());
    let report = save_planned(&mut doc, &other, AskAnswers::default()).unwrap();
    assert_eq!(report.strategy, Some(StrategyKind::Rewrite));
    assert!(
        (report.why.iter()).any(|w| matches!(w, Why::ReplacesText(_))),
        "{:?}",
        report.why
    );
    let undone = std::fs::read(&other).unwrap();
    holds_values(&undone, &[("a", "Alpha1"), ("b", "Beta2")], "undone");
    one_object_a_field(&undone, "undone");
    for value in ["Gamma3", "Delta4"] {
        assert!(!found(&undone, value, Read::Decoded(None)), "{value}");
    }
    let reopened = open(&other);
    assert_eq!(value_of(&reopened, "b"), text("Beta2"));
    assert_eq!(value_of(&reopened, "c"), None);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A file whose revisions filled `a` and cleared it, as another tool saves
/// (`append_revision`; sheaf never appends a clear), then a fill of `b`
/// through the op: the next save, as planned, is a Rewrite for the
/// earlier revisions' values, and holds no trace of `a`'s first value, by
/// the search that finds it in the file as it came.
#[test]
fn a_value_earlier_revisions_held_is_gone_after_the_next_save() {
    let dir = scratch("fill-earlier");
    let mut f = Form::new(1);
    let a = f.text("a", 0, row(0));
    f.text("b", 0, row(1));
    let filled = append_revision(&f.bytes(), &[a], |pdf| {
        pdf.get_dictionary_mut(a)
            .unwrap()
            .set("V", literal("FIRSTVALUE42"));
    });
    let cleared = append_revision(&filled, &[a], |pdf| {
        pdf.get_dictionary_mut(a).unwrap().remove(b"V");
    });
    assert!(
        found(&cleared, "FIRSTVALUE42", Read::Decoded(None)),
        "the control"
    );
    let path = dir.join("form.pdf");
    std::fs::write(&path, &cleared).unwrap();
    let mut doc = open(&path);
    assert_eq!(value_of(&doc, "a"), None);
    fx::fill(&mut doc, "b", "SecondValue");
    let report = save_planned(&mut doc, &path, AskAnswers::default()).unwrap();
    assert_eq!(report.strategy, Some(StrategyKind::Rewrite));
    assert_eq!(report.why, [Why::EarlierValues]);
    let saved = std::fs::read(&path).unwrap();
    assert!(found(&saved, "SecondValue", Read::Decoded(None)));
    assert!(!found(&saved, "FIRSTVALUE42", Read::Decoded(None)));
    holds_values(&saved, &[("b", "SecondValue")], "saved");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Undoing back to the saved version makes the next save an Append again:
/// a fill appended; that value replaced, which plans a Rewrite; undone, to
/// the saved version, which plans nothing; then a fill of an empty field,
/// which plans an Append and appends, the replacing value in no file.
#[test]
fn undoing_back_to_the_saved_version_makes_the_next_save_an_append_again() {
    let dir = scratch("fill-undo");
    let path = common::write(&dir, "form.pdf", &mut four());
    let mut doc = open(&path);
    fx::fill(&mut doc, "a", "One1");
    let (strategy, first) = planned(&mut doc, &path);
    assert_eq!(strategy, StrategyKind::Append);
    fx::fill(&mut doc, "a", "Two2");
    let replaced = plan_of(&doc, &path).strategy;
    assert!(
        matches!(&replaced, Strategy::Rewrite { why }
            if why.iter().any(|w| matches!(w, Why::ReplacesText(_)))),
        "{replaced:?}"
    );
    doc.undo().unwrap();
    assert_eq!(plan_of(&doc, &path).strategy, Strategy::Unchanged);
    assert!(!doc.is_dirty());
    fx::fill(&mut doc, "b", "Three3");
    let filled = plan_of(&doc, &path).strategy;
    assert!(matches!(filled, Strategy::Append { .. }), "{filled:?}");
    let (strategy, second) = planned(&mut doc, &path);
    assert_eq!(strategy, StrategyKind::Append);
    assert!(second.starts_with(&first));
    holds_values(&second, &[("a", "One1"), ("b", "Three3")], "appended");
    assert!(found(&second, "Three3", Read::Decoded(None)), "the control");
    assert!(!found(&second, "Two2", Read::Decoded(None)));
    let _ = std::fs::remove_dir_all(&dir);
}

// ----- Signatures and usage rights ---------------------------------------

/// A hybrid form, its XFA copy as Designer writes one: empty text fields
/// `form1[0].x[0]` and `form1[0].y[0]`, and a signature field `sig[0]`,
/// signed (an intact CMS), whose FieldMDP lock includes `y`.
fn signed_hybrid() -> Vec<u8> {
    let mut f = Form::new(1);
    let x = f.text("x[0]", 0, row(0));
    let y = f.text("y[0]", 0, row(1));
    let sig = f.signature("sig[0]", 0, row(2));
    f.group("form1[0]", &[x, y, sig]);
    f.xfa();
    f.sign_locking(sig, "Include", &["form1[0].y[0]"], None);
    f.signed_bytes(detached)
}

/// A fill of a signed hybrid form: its plan is an Append that changes more
/// than the signature allows (dropping the XFA copy writes the form, so
/// the signature is flagged), which it asks about, offering the Append
/// anyway with the copy's datasets said to stay readable in the signed
/// revision; a save given no answers writes nothing, neither Append nor
/// Rewrite, and leaves the document unsaved. Taken, the Append keeps the
/// signed bytes and the signature fits, the XFA copy gone from the form
/// and the value there. A fill of the field the signature locks is
/// refused, `Locked`, and changes nothing.
#[test]
fn a_fill_of_a_signed_hybrid_form_offers_append_anyway() {
    let dir = scratch("fill-signed");
    let path = dir.join("signed.pdf");
    let original = signed_hybrid();
    std::fs::write(&path, &original).unwrap();
    assert!(signature_fits(&original));
    let mut doc = open(&path);
    fx::fill(&mut doc, "form1[0].x[0]", "Filled5");
    let planned = plan_of(&doc, &path);
    println!("the plan: {planned:?}");
    let offer = (planned.asks.iter())
        .find_map(|ask| match ask {
            Ask::Signatures {
                append_anyway: Some(offer),
                ..
            } => Some(offer.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no Append anyway: {:?}", planned.asks));
    let onto = sheaf_doc::materialize::Onto::Saved;
    assert_eq!(planned.strategy, Strategy::Append { onto });
    let datasets = sheaf_doc::effects::RemovedKind::XfaDatasets;
    assert!(
        (offer.stays_readable.iter()).any(|r| r.kind == datasets),
        "{offer:?}"
    );
    let stop = save_planned(&mut doc, &path, AskAnswers::default()).unwrap_err();
    assert!(
        matches!(
            &stop,
            SaveStop::Ask(Ask::Signatures {
                append_anyway: Some(_),
                ..
            })
        ),
        "{stop:?}"
    );
    assert_eq!(std::fs::read(&path).unwrap(), original, "nothing written");
    assert!(doc.is_dirty());
    let mut anyway = AskAnswers::default();
    anyway.take(&offer);
    let report = save_planned(&mut doc, &path, anyway).unwrap();
    assert_eq!(report.strategy, Some(StrategyKind::Append));
    let appended = std::fs::read(&path).unwrap();
    assert!(appended.starts_with(&original));
    assert!(signature_fits(&appended));
    assert!(!xfa_and_perms(&appended).0, "the XFA copy");
    assert_eq!(
        values(&appended).get("form1[0].x[0]").map(String::as_str),
        Some("Filled5")
    );

    let version = doc.current_version();
    let err = fx::try_fill(&mut doc, "form1[0].y[0]", "Locked6").unwrap_err();
    assert!(
        matches!(&err, EditError::Locked { field, .. } if field == "form1[0].y[0]"),
        "{err:?}"
    );
    assert_eq!(doc.current_version(), version, "nothing changed");
    assert_eq!(value_of(&doc, "form1[0].y[0]"), None);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A form whose Reader usage rights allow filling in (`/Form [/FillIn]`),
/// with no XFA: a fill of an empty field plans an Append that keeps them
/// and appends, `/Perms` still in the file, the check clean.
#[test]
fn a_fill_within_usage_rights_appends_and_keeps_them() {
    let dir = scratch("fill-rights");
    let mut f = Form::new(1);
    f.text("a", 0, row(0));
    f.text("b", 0, row(1));
    f.usage_rights();
    let path = common::write(&dir, "rights.pdf", &mut f);
    let original = std::fs::read(&path).unwrap();
    assert_eq!(xfa_and_perms(&original), (false, true));
    let mut doc = open(&path);
    fx::fill(&mut doc, "a", "Within7");
    let planned_now = plan_of(&doc, &path);
    assert!(
        matches!(planned_now.strategy, Strategy::Append { .. }),
        "{planned_now:?}"
    );
    let (strategy, bytes) = planned(&mut doc, &path);
    assert_eq!(strategy, StrategyKind::Append);
    assert!(bytes.starts_with(&original));
    assert_eq!(xfa_and_perms(&bytes), (false, true), "/Perms stays");
    holds_values(&bytes, &[("a", "Within7")], "appended");
    clean(&bytes, "appended");
    let _ = std::fs::remove_dir_all(&dir);
}

// ----- Encryption and recovery -------------------------------------------

/// What the encryption and recovery checks fill `name` with.
const SECRET: &str = "CIPHERVALUE77";

/// Two empty text fields, `name` and `other`, on one page, encrypted with
/// AES-128 as [`encrypted_bytes`] does.
fn encrypted_form(user: &str, permissions: Permissions) -> Vec<u8> {
    let mut f = Form::new(1);
    f.text("name", 0, row(0));
    f.text("other", 0, row(1));
    encrypted_bytes(f, user, permissions)
}

/// `doc`'s recovery sidecar, written beside its file as the app writes
/// it: its bytes.
fn sidecar(doc: &Document) -> Vec<u8> {
    let (file, base) = doc.file().map(|(f, s)| (f.to_path_buf(), *s)).unwrap();
    let size = sheaf_doc::recovery::write(&doc.recovery_input(), &file, &base, &NoProgress);
    assert!(size.unwrap() > 0, "a sidecar");
    std::fs::read(sheaf_doc::recovery::sidecar_path(&file)).unwrap()
}

/// The document the sidecar beside `path` restores, with `password`,
/// opened as the app opens it; and the bytes it reads, the file with the
/// sidecar's sections after it.
fn restored(path: &Path, password: Option<&str>) -> (Document, Vec<u8>) {
    let rec = sheaf_doc::recovery::find(path).expect("a sidecar");
    let store = Store::new();
    let (src, name, came) = sheaf_doc::recovery::restore(&store, path, &rec, password).unwrap();
    let bytes = src.bytes().to_vec();
    let policy = sheaf_doc::history::default_policy();
    let mut doc = sheaf_doc::open::start(store, src.clone(), name, Some(came), policy);
    doc.ready(sheaf_doc::open::finish(&src, &NoProgress))
        .unwrap();
    (doc, bytes)
}

/// Whether `bytes` read as an encrypted file, with `password`.
fn encrypted(bytes: &[u8], password: Option<&str>) -> bool {
    let limits = &sheaf_doc::source::Limits::DEFAULT.reader;
    let pdf = LazyPdf::open(Arc::<[u8]>::from(bytes), password, limits).unwrap();
    pdf.is_encrypted()
}

/// A form with only a permissions password (an owner's that allows
/// printing and filling in; it opens with the empty user password) keeps
/// its encryption through a fill saved both ways and through recovery
/// ([`encrypted_fill`]).
#[test]
fn a_permissions_password_s_form_stays_encrypted_through_a_fill() {
    let perms = Permissions::PRINT | Permissions::FILL_FORMS;
    encrypted_fill("permissions", &encrypted_form("", perms), None);
}

/// A form with an open password keeps its encryption through a fill saved
/// both ways and through recovery ([`encrypted_fill`]), and opens only
/// with its password after each.
#[test]
fn an_open_password_s_form_stays_encrypted_through_a_fill() {
    let bytes = encrypted_form("secret", Permissions::ALL);
    encrypted_fill("open", &bytes, Some("secret"));
}

/// `bytes`, an encrypted form that opens with `password`, its `name`
/// filled with [`SECRET`]: saved by a Rewrite and by an Append, each file
/// still encrypted, the value in it decrypted (the control) and nowhere in
/// the clear, and read back reopened; and from a recovery sidecar of the
/// fill, which holds the value nowhere in the clear, restored, the file
/// and the sidecar's sections read as one still encrypted and holding the
/// value decrypted, the restored document holding it, and its save as
/// planned encrypted, holding it, read back.
fn encrypted_fill(name: &str, bytes: &[u8], password: Option<&str>) {
    let dir = scratch(&format!("fill-encrypted-{name}"));
    let open_with = |path: &Path| sheaf_doc::open::file(path, password).unwrap().doc;
    let held = |bytes: &[u8], what: &str| {
        assert!(encrypted(bytes, password), "{name}, {what}: encrypted");
        assert!(
            found(bytes, SECRET, Read::Decoded(password)),
            "{name}, {what}: the control, the value decrypted"
        );
        assert!(
            !found(bytes, SECRET, Read::Clear),
            "{name}, {what}: the value in the clear"
        );
    };
    for how in [How::Rewritten, How::Appended] {
        let path = dir.join(format!("{how:?}.pdf"));
        std::fs::write(&path, bytes).unwrap();
        let mut doc = open_with(&path);
        fx::fill(&mut doc, "name", SECRET);
        let (strategy, saved) = saved(&mut doc, &path, how);
        assert_eq!(strategy, how.strategy(), "{name}");
        held(&saved, &format!("{how:?}"));
        assert_eq!(value_of(&open_with(&path), "name"), text(SECRET), "{name}");
        if password.is_some() {
            assert!(sheaf_doc::open::file(&path, None).is_err(), "{name}");
        }
    }

    let path = dir.join("recovered.pdf");
    std::fs::write(&path, bytes).unwrap();
    let mut doc = open_with(&path);
    fx::fill(&mut doc, "name", SECRET);
    let side = sidecar(&doc);
    assert!(
        !found(&side, SECRET, Read::Clear),
        "{name}: the sidecar holds the value in the clear"
    );
    let (mut back, read) = restored(&path, password);
    held(&read, "restored");
    assert_eq!(value_of(&back, "name"), text(SECRET), "{name}: restored");
    let (strategy, saved) = planned(&mut back, &path);
    println!("{name}: the restored document saved as planned, {strategy:?}");
    held(&saved, "the restored document saved");
    assert_eq!(value_of(&open_with(&path), "name"), text(SECRET), "{name}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// An unencrypted fill restored from its sidecar holds its value and its
/// appearance: the sidecar holds the value as it is (the control for the
/// encrypted checks' "only as ciphertext"); the restored document holds
/// it, its widget's appearance is the file's now (not one a save would
/// make again) and draws it, and PDFium draws the restored file's page,
/// appearances only, as it draws a Rewrite of the fill, the widget's box
/// otherwise than the form as it came.
#[test]
fn a_fill_restored_from_its_sidecar_holds_its_value_and_its_appearance() {
    let dir = scratch("fill-restored");
    let mut f = Form::new(1);
    f.text("name", 0, row(0));
    f.text("other", 0, row(1));
    let path = common::write(&dir, "form.pdf", &mut f);
    let original = std::fs::read(&path).unwrap();
    let mut doc = open(&path);
    fx::fill(&mut doc, "name", SECRET);
    let side = sidecar(&doc);
    assert!(found(&side, SECRET, Read::Clear), "the sidecar holds it");
    let (back, read) = restored(&path, None);
    assert_eq!(value_of(&back, "name"), text(SECRET));
    let widget = widgets_of(&back, "name")[0];
    assert!(!sheaf_doc::forms::appearance::stale(back.state(), widget));
    let got = search(&read, SECRET, Read::Decoded(None));
    assert!(got.shown, "its appearance draws it: {got:?}");
    let rewritten = sheaf_doc::save::rewrite(&doc).unwrap();
    let engine = |bytes: &[u8]| PdfiumEngine::open_bytes(bytes.to_vec().into(), None).unwrap();
    let [drawn, rewrite, was] =
        [&read, &rewritten, &original].map(|b| flat_at(&engine(b), PageIndex(0), 1224).unwrap());
    let whole = [0, 0, drawn.width as usize, drawn.height as usize];
    assert_eq!(
        differing_in(&drawn, &rewrite, whole),
        0,
        "drawn as the Rewrite"
    );
    assert!(differing_in(&drawn, &was, whole) > 0, "drawn as it came");
    let _ = std::fs::remove_dir_all(&dir);
}

// ----- What an Append writes ---------------------------------------------

/// Two pages: a text field `name` whose two widgets are kids, one on each
/// page, under a parent field `top`; and `other`, a text field that's its
/// own widget, on the first. The fields' objects: `top`, `name`, its two
/// widgets, and `other`.
fn kids_under_a_parent() -> (Form, [ObjectId; 5]) {
    let mut f = Form::new(2);
    let w1 = f.text("w1", 0, row(0));
    let w2 = f.text("w2", 1, row(0));
    let other = f.text("other", 0, row(2));
    let name = f.pdf.add_object(dictionary! {
        "FT" => Object::name("Tx"),
        "T" => literal("name"),
        "DA" => literal("/Helv 10 Tf 0 g"),
        "Kids" => vec![w1.into(), w2.into()],
    });
    for w in [w1, w2] {
        let d = f.dict(w);
        for key in [b"FT".as_slice(), b"T", b"DA"] {
            d.remove(key);
        }
        d.set("Parent", name);
    }
    f.set_fields(&[name, other]);
    let top = f.group("top", &[name]);
    (f, [top, name, w1, w2, other])
}

/// Each object of `whole`'s section after its first `base` bytes, the
/// form XObjects among them, by number.
fn forms_in_section(base: usize, whole: &[u8]) -> BTreeSet<ObjectId> {
    let pdf = Pdf::load_mem(whole).unwrap();
    (appended_objects(base, whole).into_keys())
        .filter(|id| {
            (pdf.get_object(*id).ok())
                .and_then(Object::as_stream)
                .is_some_and(|s: &Stream| {
                    s.dict.get(b"Subtype").and_then(Object::as_name) == Some(b"Form")
                })
        })
        .collect()
}

/// Object `id`'s bytes as `bytes`'s latest revision holds it.
fn raw(bytes: &[u8], id: ObjectId) -> Vec<u8> {
    let limits = &sheaf_doc::source::Limits::DEFAULT.reader;
    let pdf = LazyPdf::open(Arc::<[u8]>::from(bytes), None, limits).unwrap();
    bytes[pdf.raw_span(id).expect("an object of its own")].to_vec()
}

/// What a fill's Append writes, by object, as it is today: the field, its
/// widgets and their new forms, and its ancestor written again byte for
/// byte at its own number (an Append writes every ancestor of a changed
/// field as the file has it; one that left out what it would write as the
/// file has it would move this); no page, no `/AcroForm`, nothing of the
/// field beside it. A fill whose value needs the fallback font writes the
/// field and its form, and the new font subset: each other object it
/// writes is new, and reached from the form's `/Resources /Font`.
#[test]
fn what_a_fill_s_append_writes_by_object() {
    let dir = scratch("fill-objects");
    let (mut f, [top, name, w1, w2, other]) = kids_under_a_parent();
    let path = common::write(&dir, "form.pdf", &mut f);
    let original = std::fs::read(&path).unwrap();
    let mut doc = open(&path);
    fx::fill(&mut doc, "top.name", "Camden");
    let (strategy, first) = planned(&mut doc, &path);
    assert_eq!(strategy, StrategyKind::Append);
    let written: BTreeSet<ObjectId> = appended_objects(original.len(), &first)
        .into_keys()
        .collect();
    let forms = forms_in_section(original.len(), &first);
    assert_eq!(forms.len(), 2, "a form for each widget: {forms:?}");
    let want: BTreeSet<ObjectId> = [top, name, w1, w2].into_iter().chain(forms).collect();
    assert_eq!(written, want);
    assert_eq!(raw(&first, top), raw(&original, top), "the ancestor");
    assert_ne!(raw(&first, name), raw(&original, name), "the field");
    holds_values(&first, &[("top.name", "Camden")], "appended");

    fx::fill(&mut doc, "other", "\u{141}ukasz");
    let (strategy, second) = planned(&mut doc, &path);
    assert_eq!(strategy, StrategyKind::Append);
    let written: BTreeSet<ObjectId> = appended_objects(first.len(), &second).into_keys().collect();
    let forms = forms_in_section(first.len(), &second);
    let [form] = Vec::from_iter(forms).try_into().expect("one form");
    assert!(written.contains(&other) && written.contains(&form));
    let pdf = Pdf::load_mem(&second).unwrap();
    let reached = reached_from(&pdf, form);
    let size = LazyPdf::open(
        Arc::<[u8]>::from(&first[..]),
        None,
        &sheaf_doc::source::Limits::DEFAULT.reader,
    )
    .unwrap()
    .trailer()
    .get(b"Size")
    .and_then(Object::as_integer)
    .unwrap();
    let fonts: Vec<ObjectId> = (written.iter().copied())
        .filter(|&id| id != other && id != form)
        .collect();
    assert!(!fonts.is_empty(), "a font subset");
    for id in &fonts {
        assert!(i64::from(id.number) >= size, "{id:?} is new");
        assert!(reached.contains(id), "{id:?} is the form's font's");
    }
    let type0 = (fonts.iter()).any(|&id| {
        (pdf.get_dictionary(id).ok())
            .is_some_and(|d| d.get(b"Subtype").and_then(Object::as_name) == Some(b"Type0"))
    });
    assert!(type0, "the subset's font");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Every object `pdf` reaches from object `from`'s dictionary by
/// references.
fn reached_from(pdf: &Pdf, from: ObjectId) -> BTreeSet<ObjectId> {
    fn refs(o: &Object, out: &mut Vec<ObjectId>) {
        match o.value() {
            PdfValue::Ref(id) => out.push(*id),
            PdfValue::Array(a) => a.iter().for_each(|o| refs(o, out)),
            PdfValue::Dict(d) => d.iter().for_each(|(_, o)| refs(o, out)),
            PdfValue::Stream(s) => s.dict.iter().for_each(|(_, o)| refs(o, out)),
            _ => {}
        }
    }
    let mut seen = BTreeSet::new();
    let mut next = vec![from];
    while let Some(id) = next.pop() {
        if !seen.insert(id) {
            continue;
        }
        if let Ok(o) = pdf.get_object(id) {
            refs(o, &mut next);
        }
    }
    seen
}
