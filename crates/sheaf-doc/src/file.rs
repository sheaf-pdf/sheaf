//! Direct access to a PDF file's object graph via lopdf. This is the
//! serialization backend (see docs/document-model.md), not the editing
//! model; the viewer never touches it.

use std::path::Path;

use anyhow::{Context, Result};

pub struct PdfFile {
    inner: lopdf::Document,
}

impl PdfFile {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let inner =
            lopdf::Document::load(path).with_context(|| format!("loading {}", path.display()))?;
        Ok(Self { inner })
    }

    pub fn page_count(&self) -> u32 {
        self.inner.get_pages().len() as u32
    }

    /// Full rewrite. Incremental save comes in M2.
    pub fn save_as(&mut self, path: impl AsRef<Path>) -> Result<()> {
        self.inner.save(path.as_ref())?;
        Ok(())
    }

    pub fn into_inner(self) -> lopdf::Document {
        self.inner
    }
}

use std::collections::HashMap;

use lopdf::{Dictionary, Object, ObjectId};
use sheaf_edit::Rotation;

use crate::annot::{Annot, AnnotKind, Box2, Pt, from_json};
use crate::attachments::Objects;

/// The mark an `/Annots` entry carries, when this build can read it and
/// no other tool has changed it since sheaf wrote it (M10.5 §3.6): its
/// payload is this build's version ([`VERSION`](crate::annot::VERSION)),
/// and fresh while its digest is the dictionary's
/// ([`staleness`](crate::annot::staleness)). A page stamp must hold its
/// page and an attachment its file ([`holds`]). None for anything else,
/// which is another tool's.
pub(crate) fn sheaf_mark<P: Objects + ?Sized>(pdf: &P, entry: &Object) -> Option<Annot> {
    let (_, dict) = pdf.dereference(entry).ok()?;
    let dict = dict.as_dict().ok()?;
    let (_, json) = pdf.dereference(dict.get(b"SHEF_Annot").ok()?).ok()?;
    let payload = from_json(text_string(json.as_str().ok()?).as_bytes())?;
    if payload.v != crate::annot::VERSION {
        return None;
    }
    let a = payload.mark;
    let digest = crate::annot::staleness(pdf, dict, a.thread_parent().is_some());
    (payload.std == digest && holds(pdf, dict, &a)).then_some(a)
}

/// Whether `dict`, mark `a`'s, holds what the mark names: a page stamp
/// its page and an attachment its file ([`held`]). Without it, the mark is
/// drawn by its appearance alone, as another tool's comment is, and opens
/// as one, as a stale mark does.
fn holds<P: Objects + ?Sized>(pdf: &P, dict: &Dictionary, a: &Annot) -> bool {
    match a.kind {
        AnnotKind::PageStamp { .. } | AnnotKind::FileAttachment { .. } => {
            held(pdf, dict, a).is_some()
        }
        _ => true,
    }
}

/// What `dict`, mark `a`'s, holds of its own: a page stamp's page form
/// ([`stamp_page`]) or an attachment's embedded file ([`attached_file`]).
pub(crate) fn held<P: Objects + ?Sized>(pdf: &P, dict: &Dictionary, a: &Annot) -> Option<ObjectId> {
    match a.kind {
        AnnotKind::PageStamp { .. } => stamp_page(pdf, dict),
        AnnotKind::FileAttachment { .. } => attached_file(pdf, dict),
        _ => None,
    }
}

/// The page a page stamp's appearance draws, `dict` its dictionary: the
/// one form its normal appearance's resources name. The form itself isn't
/// read.
pub(crate) fn stamp_page<P: Objects + ?Sized>(pdf: &P, dict: &Dictionary) -> Option<ObjectId> {
    let normal = dict_at(pdf, dict_at(pdf, dict, b"AP")?, b"N")?;
    let forms = dict_at(pdf, dict_at(pdf, normal, b"Resources")?, b"XObject")?;
    if forms.len() != 1 {
        return None;
    }
    forms.iter().next()?.1.as_reference().ok()
}

/// The embedded file an attachment's file specification holds, `dict` its
/// dictionary: its stream, which isn't read.
pub(crate) fn attached_file<P: Objects + ?Sized>(pdf: &P, dict: &Dictionary) -> Option<ObjectId> {
    let ef = dict_at(pdf, dict_at(pdf, dict, b"FS")?, b"EF")?;
    ef.get(b"F")
        .or_else(|_| ef.get(b"UF"))
        .ok()?
        .as_reference()
        .ok()
}

/// `d`'s `key`, its references followed: a dictionary, or a stream's.
fn dict_at<'a, P: Objects + ?Sized>(
    pdf: &'a P,
    d: &'a Dictionary,
    key: &[u8],
) -> Option<&'a Dictionary> {
    match pdf.dereference(d.get(key).ok()?).ok()?.1 {
        Object::Dictionary(d) => Some(d),
        Object::Stream(s) => Some(&s.dict),
        _ => None,
    }
}

/// Is this `/Annots` entry one of ours to rewrite: a mark this build can
/// read, fresh, or a companion of one? A companion is a replacement's
/// StrikeOut, grouped (`/RT /Group`) under a sheaf Caret by its `/IRT`. A
/// stale mark isn't ours, nor is a `/SHEF_Annot` this build can't read,
/// from another sheaf: each is kept like any other tool's annotation, so a
/// save here never loses it or reverts its edit. It answers as
/// [`sheaf_mark`] does, so what the open lists as another tool's comment
/// is what a save keeps.
pub(crate) fn is_ours<P: Objects + ?Sized>(pdf: &P, entry: &Object) -> bool {
    let Some(dict) = pdf
        .dereference(entry)
        .ok()
        .and_then(|(_, o)| o.as_dict().ok())
    else {
        return false;
    };
    let mark = |entry: &Object| sheaf_mark(pdf, entry);
    if dict.has(b"SHEF_Annot") {
        return mark(entry).is_some();
    }
    // One step only: a companion's /IRT is its mark, never another companion.
    let Ok(irt) = dict.get(b"IRT") else {
        return false;
    };
    let name = |key: &[u8], is: &[u8]| {
        dict.get(key)
            .and_then(Object::as_name)
            .is_ok_and(|n| n == is)
    };
    name(b"RT", b"Group")
        && name(b"Subtype", b"StrikeOut")
        && mark(irt).is_some_and(|a| matches!(a.kind, AnnotKind::Replace { .. }))
}

/// An annotation some other tool left on a page of the file, a comment or
/// a link, as read: what a [`Foreign`](crate::model::Foreign) record
/// summarizes it by. A comment is listed read-only in the Comments panel;
/// PDFium draws both.
#[derive(Debug, Clone, PartialEq)]
pub struct ForeignAnnot {
    /// `/Subtype`, e.g. `Text`, `Highlight`, `Link`.
    pub subtype: String,
    pub author: String,
    pub contents: String,
    /// `/M`, a PDF date string, or empty.
    pub modified: String,
    /// Its `/Rect`, normalized: PageSpace.
    pub rect: Box2,
    /// `/NM`, or for one without, a stand-in made from what it says
    /// ([`anon_name`]), so replies can still name it.
    pub name: String,
    /// The comment whose thread it's in (`/IRT`), by name.
    pub parent: Option<String>,
    /// `/RT /Group`: part of its parent (the StrikeOut of Acrobat's
    /// replace-text edit, say) rather than a reply to it.
    pub grouped: bool,
    /// A review state it sets on its parent: (`/StateModel`, `/State`).
    pub state: Option<(String, String)>,
    /// `/CreationDate`, or empty.
    pub created: String,
    /// Its position in the page's `/Annots`.
    pub slot: usize,
    /// The file hides it (`/F` bit 2): sheaf never shows it.
    pub hidden: bool,
    /// `/C`, when it's RGB.
    pub color: Option<[u8; 3]>,
    /// What it is as one of our marks, when sheaf can hold it: what
    /// editing it makes it (M9 decision 5). Read when the file is.
    pub mark: Option<Box<Annot>>,
    /// For a stamp whose `mark` is a page stamp, the form its appearance
    /// is (`/AP /N`): the page that mark names by stem, which it holds
    /// once it's made.
    pub stamp: Option<ObjectId>,
}

impl ForeignAnnot {
    /// Whether it's a link, which isn't a comment.
    pub fn is_link(&self) -> bool {
        self.subtype == "Link"
    }
}

/// A name for an annotation without `/NM`, from what it says: the same
/// every time it's read, so a reply made in sheaf can name it, and a save
/// writes it in as its `/NM`.
pub fn anon_name(dict: &lopdf::Dictionary) -> String {
    // The fields one after another, each ended by a byte no text has, and
    // their checksum: stable across builds, unlike std's hasher.
    let mut fields: Vec<u8> = Vec::new();
    let mut eat = |bytes: &[u8]| {
        fields.extend_from_slice(bytes);
        fields.push(0xff);
    };
    for key in [
        b"Subtype".as_slice(),
        b"Rect",
        b"T",
        b"M",
        b"CreationDate",
        b"Contents",
    ] {
        let v = dict.get(key).ok();
        match v {
            Some(lopdf::Object::Array(a)) => {
                for o in a {
                    // As an f32, as reals were read before, so names stay.
                    eat(format!("{:.2}", o.as_float().unwrap_or(0.0) as f32).as_bytes());
                }
            }
            Some(lopdf::Object::Name(n)) => eat(n),
            Some(o) => eat(pdf_text(o).as_bytes()),
            None => eat(b""),
        }
    }
    format!("sheaf-anon:{}", sheaf_util::checksum_hex(&fields))
}

/// What an annotation is called in threads: its `/NM`, else [`anon_name`].
pub fn thread_name(dict: &lopdf::Dictionary) -> String {
    dict.get(b"NM")
        .ok()
        .map(pdf_text)
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| anon_name(dict))
}

/// Everything annotation-shaped in a file: on each page, our marks
/// (editable), with what they hold, and everyone else's comments and
/// links.
#[derive(Debug, Default)]
pub struct AnnotsInFile {
    /// Each page's that has any, by index, in `/Annots` order.
    pub pages: HashMap<u32, Vec<InFile>>,
}

/// An annotation as a page of the file has it, and the object it is:
/// None for a dictionary written into the page's `/Annots`.
#[derive(Debug, Clone, PartialEq)]
pub struct InFile {
    pub object: Option<ObjectId>,
    /// Its place in the page's `/Annots`.
    pub slot: usize,
    pub found: Found,
}

/// What an annotation in a file is.
#[derive(Debug, Clone, PartialEq)]
pub enum Found {
    /// One of sheaf's marks, fresh, and what it holds of its own: its page
    /// stamp's page form or its attachment's embedded file.
    Mark {
        annot: Annot,
        held: Option<ObjectId>,
    },
    /// Another tool's comment or link, a stale mark among them.
    Foreign(ForeignAnnot),
}

impl AnnotsInFile {
    /// sheaf's marks, by page index.
    pub fn sheaf(&self) -> HashMap<u32, Vec<Annot>> {
        (self.pages.iter())
            .map(|(&page, list)| {
                let marks = list.iter().filter_map(|a| match &a.found {
                    Found::Mark { annot, .. } => Some(annot.clone()),
                    Found::Foreign(_) => None,
                });
                (page, marks.collect::<Vec<_>>())
            })
            .filter(|(_, marks)| !marks.is_empty())
            .collect()
    }

    /// Other tools' comments and links, by page index and then in
    /// `/Annots` order, with their pages.
    pub fn foreign(&self) -> Vec<(u32, &ForeignAnnot)> {
        let mut pages: Vec<u32> = self.pages.keys().copied().collect();
        pages.sort_unstable();
        pages
            .into_iter()
            .flat_map(|page| {
                self.pages[&page]
                    .iter()
                    .filter_map(move |a| match &a.found {
                        Found::Foreign(f) => Some((page, f)),
                        Found::Mark { .. } => None,
                    })
            })
            .collect()
    }
}

/// [`load_pdf`] for a file in memory.
pub fn load_pdf_mem(bytes: &[u8], password: Option<&str>) -> Result<lopdf::Document> {
    let mut pdf = lopdf::Document::load_mem(bytes)?;
    if pdf.is_encrypted() {
        let options = lopdf::LoadOptions::with_password(password.unwrap_or(""));
        pdf = lopdf::Document::load_mem_with_options(bytes, options)
            .map_err(|e| anyhow::anyhow!("{e}"))
            .context("decrypting")?;
    }
    Ok(pdf)
}

/// Load a file with lopdf and, if it's encrypted, decrypt it in memory with
/// `password` (or the empty user password many files use): strings and
/// streams readable, lopdf's `EncryptionState` kept. lopdf's reader takes
/// the `/Encrypt` dictionary out, so lopdf's own writer would write the
/// file unencrypted; sheaf's writers take the encryption from the source
/// instead ([`Source::crypt`](crate::source::Source::crypt)), and keep it.
pub fn load_pdf(path: &Path, password: Option<&str>) -> Result<lopdf::Document> {
    let mut pdf =
        lopdf::Document::load(path).with_context(|| format!("loading {}", path.display()))?;
    if pdf.is_encrypted() {
        // lopdf only parses an encrypted file's objects when it has the
        // password at load time, so load again with it.
        pdf = lopdf::Document::load_with_password(path, password.unwrap_or(""))
            .map_err(|e| anyhow::anyhow!("{e}"))
            .with_context(|| format!("decrypting {}", path.display()))?;
    }
    Ok(pdf)
}

/// A text string's text, as PDFium decodes one: UTF-16 (a last odd byte
/// left out) or UTF-8 after their byte order marks, PDFDocEncoding
/// without one. ([`pdf_text`] reads PDFDocEncoding as Latin-1, which most
/// of it is, but not its quotes and dashes.)
pub fn text_string(bytes: &[u8]) -> String {
    let utf16 = |rest: &[u8], unit: fn([u8; 2]) -> u16| {
        let units: Vec<u16> = rest.as_chunks::<2>().0.iter().map(|&c| unit(c)).collect();
        String::from_utf16_lossy(&units)
    };
    match bytes {
        [0xFE, 0xFF, rest @ ..] => utf16(rest, u16::from_be_bytes),
        [0xFF, 0xFE, rest @ ..] => utf16(rest, u16::from_le_bytes),
        [0xEF, 0xBB, 0xBF, rest @ ..] => String::from_utf8_lossy(rest).into_owned(),
        _ => bytes.iter().map(|&b| pdf_doc_char(b)).collect(),
    }
}

/// Whether `s` reads back as itself written byte for byte, without a BOM:
/// ASCII but for the bytes PDFDocEncoding gives other characters, the
/// accents at 0x18–0x1F and the undefined 0x7F (see [`pdf_doc_char`]).
/// Anything else is written as UTF-16BE.
pub(crate) fn is_pdf_doc_ascii(s: &str) -> bool {
    s.bytes()
        .all(|b| b.is_ascii() && !matches!(b, 0x18..=0x1F | 0x7F))
}

/// A PDFDocEncoding byte's character: Latin-1's, but for the accents at
/// 0x18–0x1F and the punctuation and letters at 0x80–0xA0, and NUL for
/// the three bytes it leaves undefined (0x7F, 0x9F and 0xAD), as PDFium's
/// table has them.
fn pdf_doc_char(b: u8) -> char {
    const ACCENTS: [char; 8] = ['˘', 'ˇ', 'ˆ', '˙', '˝', '˛', '˚', '˜'];
    const HIGH: [char; 33] = [
        '•', '†', '‡', '…', '—', '–', 'ƒ', '⁄', '‹', '›', '−', '‰', '„', '“', '”', '‘', '’', '‚',
        '™', 'ﬁ', 'ﬂ', 'Ł', 'Œ', 'Š', 'Ÿ', 'Ž', 'ı', 'ł', 'œ', 'š', 'ž', '\0', '€',
    ];
    match b {
        0x18..=0x1F => ACCENTS[usize::from(b - 0x18)],
        0x80..=0xA0 => HIGH[usize::from(b - 0x80)],
        0x7F | 0xAD => '\0',
        b => char::from(b),
    }
}

/// Decode a PDF text string: UTF-16BE with BOM, else PDFDoc/Latin-1.
pub fn pdf_text(o: &lopdf::Object) -> String {
    match o.as_str() {
        Ok(bytes) => pdf_text_bytes(bytes),
        Err(_) => String::new(),
    }
}

/// [`pdf_text`] for a string's bytes (or a stream's, which hold text the
/// same way).
pub fn pdf_text_bytes(bytes: &[u8]) -> String {
    if bytes.starts_with(&[0xFE, 0xFF]) {
        let units: Vec<u16> = bytes[2..]
            .chunks(2)
            .map(|c| u16::from_be_bytes([c[0], *c.get(1).unwrap_or(&0)]))
            .collect();
        String::from_utf16_lossy(&units)
    } else {
        bytes.iter().map(|&b| b as char).collect()
    }
}

/// Subtypes listed as other tools' annotations: comments, and links.
/// Form fields, the popup windows that belong to other annotations, and
/// what else is no comment are left to the page.
fn is_listed_subtype(s: &str) -> bool {
    !matches!(
        s,
        "Widget"
            | "Popup"
            | "Screen"
            | "PrinterMark"
            | "TrapNet"
            | "Watermark"
            | "3D"
            | "Movie"
            | "Sound"
            | "RichMedia"
    )
}

/// Every mark, comment and link on `pages`, the file's page objects by
/// index, each with its `/Rotate` as the page table reads it
/// ([`Source::geom`](crate::Source::geom)), which another tool's comment
/// is converted with, each mark with what it holds: its page stamp's page
/// or its attachment's file.
pub fn annots_in<P: Objects + ?Sized>(
    pdf: &P,
    pages: impl IntoIterator<Item = (lopdf::ObjectId, Rotation)>,
) -> AnnotsInFile {
    let mut out = AnnotsInFile::default();
    for (i, (page_id, turn)) in pages.into_iter().enumerate() {
        let Ok(page) = pdf.get_dictionary(page_id) else {
            continue;
        };
        let Ok(annots) = page
            .get(b"Annots")
            .and_then(|a| pdf.dereference(a).map(|(_, o)| o))
        else {
            continue;
        };
        let Ok(list) = annots.as_array() else {
            continue;
        };
        if list.is_empty() {
            continue;
        }
        let mut found = Vec::new();
        // Other tools', each by its place in `found`, and their
        // dictionaries, to convert together.
        let mut dicts: Vec<(usize, &lopdf::Dictionary)> = Vec::new();
        for (slot, entry) in list.iter().enumerate() {
            let Ok((object, obj)) = pdf.dereference(entry) else {
                continue;
            };
            let Ok(dict) = obj.as_dict() else { continue };
            if let Some(annot) = sheaf_mark(pdf, entry) {
                let held = held(pdf, dict, &annot);
                found.push(InFile {
                    object,
                    slot,
                    found: Found::Mark { annot, held },
                });
                continue;
            }
            // A companion of one of ours is part of it. Anything else,
            // including a mark another tool has edited since sheaf wrote it
            // and one this build can't read, is listed as another tool's,
            // with what it is as one of our marks.
            if is_ours(pdf, entry) {
                continue;
            }
            let subtype = dict
                .get(b"Subtype")
                .ok()
                .and_then(|o| o.as_name().ok())
                .map(|n| String::from_utf8_lossy(n).into_owned())
                .unwrap_or_default();
            if subtype.is_empty() || !is_listed_subtype(&subtype) {
                continue;
            }
            let rect: Vec<f32> = dict
                .get(b"Rect")
                .ok()
                .and_then(|o| pdf.dereference(o).ok())
                .and_then(|(_, o)| o.as_array().ok())
                .map(|a| {
                    a.iter()
                        .map(|v| v.as_float().unwrap_or(0.0) as f32)
                        .collect()
                })
                .unwrap_or_default();
            if rect.len() != 4 {
                continue;
            }
            let text = |key: &[u8]| dict.get(key).map(pdf_text).unwrap_or_default();
            let name_of = |key: &[u8]| -> Option<String> {
                let (_, o) = pdf.dereference(dict.get(key).ok()?).ok()?;
                Some(thread_name(o.as_dict().ok()?))
            };
            let state = match (text(b"StateModel"), text(b"State")) {
                (_, s) if s.is_empty() => None,
                // Acrobat's default model.
                (m, s) if m.is_empty() => Some((crate::annot::REVIEW.to_string(), s)),
                (m, s) => Some((m, s)),
            };
            // A link is no mark sheaf can hold.
            if subtype != "Link" {
                dicts.push((found.len(), dict));
            }
            let foreign = ForeignAnnot {
                subtype,
                author: text(b"T"),
                contents: text(b"Contents"),
                modified: text(b"M"),
                rect: Box2::from_corners(
                    Pt {
                        x: rect[0],
                        y: rect[1],
                    },
                    Pt {
                        x: rect[2],
                        y: rect[3],
                    },
                ),
                name: thread_name(dict),
                parent: name_of(b"IRT"),
                grouped: dict
                    .get(b"RT")
                    .ok()
                    .and_then(|o| o.as_name().ok())
                    .is_some_and(|n| n == b"Group"),
                state,
                created: text(b"CreationDate"),
                slot,
                hidden: dict
                    .get(b"F")
                    .ok()
                    .and_then(|f| f.as_i64().ok())
                    .is_some_and(|f| f & 2 != 0),
                color: dict
                    .get(b"C")
                    .ok()
                    .and_then(|c| pdf.dereference(c).ok())
                    .and_then(|(_, c)| c.as_array().ok())
                    .filter(|c| c.len() == 3)
                    .map(|c| {
                        let v = |i: usize| {
                            (c[i].as_float().unwrap_or(0.0).clamp(0.0, 1.0) * 255.0).round() as u8
                        };
                        [v(0), v(1), v(2)]
                    }),
                mark: None,
                stamp: None,
            };
            found.push(InFile {
                object,
                slot,
                found: Found::Foreign(foreign),
            });
        }
        if !dicts.is_empty() {
            let (at, dicts): (Vec<usize>, Vec<&lopdf::Dictionary>) = dicts.into_iter().unzip();
            let marks = crate::import::convert_page(pdf, &dicts, turn, Default::default());
            for ((i, dict), m) in at.into_iter().zip(dicts).zip(marks) {
                if let Found::Foreign(f) = &mut found[i].found {
                    f.mark = m.ok().map(Box::new);
                    f.stamp = (f.mark.as_deref())
                        .filter(|m| matches!(m.kind, crate::annot::AnnotKind::PageStamp { .. }))
                        .and_then(|_| crate::import::appearance(pdf, dict));
                }
            }
        }
        if !found.is_empty() {
            out.pages.insert(i as u32, found);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::{Object, Stream, dictionary};

    /// `pdf`'s pages, none of them turned, as [`annots_in`] takes them.
    fn upright(pdf: &lopdf::Document) -> impl Iterator<Item = (lopdf::ObjectId, Rotation)> + '_ {
        pdf.page_iter().map(|page| (page, Rotation::R0))
    }

    /// Reals are f64s (vendor/lopdf/SHEAF.md). Read and written back by
    /// lopdf, a fraction, a large number and a 16-digit coordinate keep
    /// their digits, in a dictionary and in content; and an f32 that sheaf
    /// computes is written as the digits it always was.
    #[test]
    fn reals_keep_their_digits() {
        use lopdf::content::{Content, Operation};
        const TEXTS: [&str; 3] = ["0.1", "123456.789", "306.1234567890123"];
        let reals = || TEXTS.map(|t| Object::Real(t.parse().unwrap())).to_vec();
        let mut pdf = lopdf::Document::with_version("1.7");
        let pages_id = pdf.new_object_id();
        let ops = format!("{} {} m\n{} w", TEXTS[1], TEXTS[2], TEXTS[0]);
        let content = pdf.add_object(Stream::new(dictionary! {}, ops.clone().into_bytes()));
        let page = pdf.add_object(dictionary! {
            "Type" => "Page", "Parent" => pages_id, "Contents" => content,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "SheafReals" => reals(),
        });
        pdf.objects.insert(
            pages_id,
            dictionary! { "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1 }.into(),
        );
        let catalog = pdf.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        pdf.trailer.set("Root", catalog);
        let mut first = Vec::new();
        pdf.save_to(&mut first).unwrap();

        let mut read = lopdf::Document::load_mem(&first).unwrap();
        let mut written = Vec::new();
        read.save_to(&mut written).unwrap();
        let array = format!("[{}]", TEXTS.join(" "));
        assert!(
            written.windows(array.len()).any(|w| w == array.as_bytes()),
            "{}",
            String::from_utf8_lossy(&written)
        );
        let again = lopdf::Document::load_mem(&written).unwrap();
        let dict = again.get_dictionary(page).unwrap();
        assert_eq!(
            dict.get(b"SheafReals").unwrap().as_array().unwrap(),
            &reals()
        );
        let content = Content::decode(&again.get_page_content(page)).unwrap();
        let operands: Vec<Object> = content
            .operations
            .iter()
            .flat_map(|op| op.operands.clone())
            .collect();
        let [a, b, c] = reals().try_into().unwrap();
        assert_eq!(operands, vec![b, c, a]);
        assert_eq!(String::from_utf8(content.encode().unwrap()).unwrap(), ops);

        // An f32 writes its shortest digits, a whole one too.
        let reals = [0.1f32, 612.0, -3.0, 16_777_216.0, 1e-7].map(Object::from);
        let width = Content {
            operations: vec![Operation::new("w", reals.to_vec())],
        };
        assert_eq!(width.encode().unwrap(), b"0.1 612 -3 16777216 0.0000001 w");
    }

    #[test]
    fn their_threads_read_with_states_and_groups() {
        use lopdf::{Object, Stream, dictionary};
        let mut pdf = lopdf::Document::with_version("1.7");
        let pages_id = pdf.new_object_id();
        let content = pdf.add_object(Stream::new(dictionary! {}, b"".to_vec()));
        let rect = || Object::Array(vec![10.into(), 10.into(), 20.into(), 20.into()]);
        let text = |s: &str| Object::string_literal(s);
        let parent = pdf.add_object(dictionary! {
            "Type" => "Annot", "Subtype" => "Highlight", "Rect" => rect(), "NM" => text("p"),
        });
        let accepted = pdf.add_object(dictionary! {
            "Type" => "Annot", "Subtype" => "Text", "Rect" => rect(), "IRT" => parent,
            "State" => text("Accepted"), "StateModel" => text("Review"), "F" => 30,
        });
        // No /StateModel: Review, as Acrobat reads it.
        let completed = pdf.add_object(dictionary! {
            "Type" => "Annot", "Subtype" => "Text", "Rect" => rect(), "IRT" => parent,
            "State" => text("Completed"),
        });
        let part = pdf.add_object(dictionary! {
            "Type" => "Annot", "Subtype" => "StrikeOut", "Rect" => rect(), "IRT" => parent,
            "RT" => Object::Name(b"Group".to_vec()),
        });
        let list: Vec<Object> = [parent, accepted, completed, part]
            .into_iter()
            .map(Object::Reference)
            .collect();
        let page = pdf.add_object(dictionary! {
            "Type" => "Page", "Parent" => pages_id, "Contents" => content,
            "MediaBox" => Object::Array(vec![0.into(), 0.into(), 612.into(), 792.into()]),
            "Annots" => list,
        });
        pdf.objects.insert(
            pages_id,
            Object::Dictionary(
                dictionary! { "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1 },
            ),
        );
        let catalog = pdf.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        pdf.trailer.set("Root", catalog);
        let read = annots_in(&pdf, upright(&pdf));
        let found: Vec<&ForeignAnnot> = read.foreign().into_iter().map(|(_, f)| f).collect();
        assert_eq!(found.len(), 4);
        assert_eq!(found[0].name, "p");
        assert_eq!(found[0].parent, None);
        assert_eq!(
            found[1].state,
            Some(("Review".to_string(), "Accepted".to_string()))
        );
        assert_eq!(found[1].parent.as_deref(), Some("p"));
        assert_eq!(found[2].state.as_ref().unwrap().0, "Review");
        assert!(found[3].grouped && !found[1].grouped);
    }

    /// A mark this build can't read, a kind from a newer sheaf or a
    /// payload of another version, is another tool's, listed as one and
    /// kept.
    #[test]
    fn a_mark_this_build_cant_read_is_another_tool_s() {
        use lopdf::{Object, Stream, dictionary};
        let mut pdf = lopdf::Document::with_version("1.7");
        let pages_id = pdf.new_object_id();
        let content = pdf.add_object(Stream::new(dictionary! {}, b"".to_vec()));
        let rect = || Object::Array(vec![10.into(), 10.into(), 20.into(), 20.into()]);
        let ours = Annot {
            id: crate::annot::AnnotId(5),
            kind: crate::annot::AnnotKind::Note {
                pos: Pt { x: 1.0, y: 2.0 },
            },
            color: [1, 2, 3],
            opacity: 1.0,
            stroke: 1.0,
            author: "me".into(),
            contents: String::new(),
            modified: String::new(),
            created: String::new(),
            fill: None,
            dashed: false,
            cloudy: false,
            turn: Rotation::R0,
            name: String::new(),
        };
        let main = crate::serialize::built::to_pdf(&ours, &mut pdf).main;
        let mut other_version = main.clone();
        let json = text_string(main.get(b"SHEF_Annot").unwrap().as_str().unwrap());
        let version = format!("\"v\":{}", crate::annot::VERSION);
        assert!(json.contains(&version), "{json}");
        let json = json.replace(&version, "\"v\":999");
        other_version.set("SHEF_Annot", Object::string_literal(json));
        let main = pdf.add_object(main);
        let other_version = pdf.add_object(other_version);
        let unknown = pdf.add_object(dictionary! {
            "Type" => "Annot", "Subtype" => "StrikeOut", "Rect" => rect(),
            "SHEF_Annot" => Object::string_literal(
                format!(r#"{{"v":{},"id":6,"kind":{{"Future":{{}}}}}}"#, crate::annot::VERSION),
            ),
        });
        let list: Vec<Object> = [main, other_version, unknown]
            .into_iter()
            .map(Object::Reference)
            .collect();
        let page = pdf.add_object(dictionary! {
            "Type" => "Page", "Parent" => pages_id, "Contents" => content,
            "MediaBox" => Object::Array(vec![0.into(), 0.into(), 612.into(), 792.into()]),
            "Annots" => list.clone(),
        });
        pdf.objects.insert(
            pages_id,
            Object::Dictionary(
                dictionary! { "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1 },
            ),
        );
        let catalog = pdf.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        pdf.trailer.set("Root", catalog);

        let mine: Vec<bool> = list.iter().map(|e| is_ours(&pdf, e)).collect();
        assert_eq!(mine, vec![true, false, false]);
        let found = annots_in(&pdf, upright(&pdf));
        assert_eq!(found.sheaf()[&0], vec![ours]);
        let subtypes: Vec<&str> = (found.foreign().into_iter())
            .map(|(_, f)| f.subtype.as_str())
            .collect();
        assert_eq!(subtypes, vec!["Text", "StrikeOut"]);
    }

    /// A replacement's StrikeOut is its Caret's companion by its group
    /// alone, as sheaf writes it: grouped under a sheaf Caret. Grouped under another of sheaf's marks, or under another
    /// tool's Caret, a StrikeOut is another tool's.
    #[test]
    fn a_strike_grouped_under_a_sheaf_caret_is_its_companion() {
        use lopdf::{Object, Stream, dictionary};
        let mut pdf = lopdf::Document::with_version("1.7");
        let pages_id = pdf.new_object_id();
        let content = pdf.add_object(Stream::new(dictionary! {}, b"".to_vec()));
        let rect = || Object::Array(vec![10.into(), 10.into(), 20.into(), 20.into()]);
        let mark = |id, kind| Annot {
            id: crate::annot::AnnotId(id),
            kind,
            color: [1, 2, 3],
            opacity: 1.0,
            stroke: 1.0,
            author: "me".into(),
            contents: "new words".into(),
            modified: String::new(),
            created: String::new(),
            fill: None,
            dashed: false,
            cloudy: false,
            turn: Rotation::R0,
            name: String::new(),
        };
        let at = Box2 {
            x: 10.0,
            y: 10.0,
            w: 30.0,
            h: 10.0,
        };
        let replace = mark(
            5,
            crate::annot::AnnotKind::Replace {
                rects: vec![at],
                struck: "old".into(),
                at,
            },
        );
        let written = crate::serialize::built::to_pdf(&replace, &mut pdf);
        let caret = pdf.add_object(written.main);
        let [mut strike] = <[_; 1]>::try_from(written.companions).ok().unwrap();
        strike.set("IRT", caret);
        strike.set("RT", Object::Name(b"Group".to_vec()));
        let strike = pdf.add_object(strike);
        let note = mark(
            6,
            crate::annot::AnnotKind::Note {
                pos: Pt { x: 1.0, y: 2.0 },
            },
        );
        let note = crate::serialize::built::to_pdf(&note, &mut pdf).main;
        let note = pdf.add_object(note);
        let grouped = |pdf: &mut lopdf::Document, under| {
            pdf.add_object(dictionary! {
                "Type" => "Annot", "Subtype" => "StrikeOut", "Rect" => rect(),
                "IRT" => under, "RT" => "Group",
            })
        };
        let under_note = grouped(&mut pdf, note);
        let their_caret = pdf.add_object(dictionary! {
            "Type" => "Annot", "Subtype" => "Caret", "Rect" => rect(),
        });
        let under_theirs = grouped(&mut pdf, their_caret);
        let list: Vec<Object> = [caret, strike, note, under_note, their_caret, under_theirs]
            .into_iter()
            .map(Object::Reference)
            .collect();
        let page = pdf.add_object(dictionary! {
            "Type" => "Page", "Parent" => pages_id, "Contents" => content,
            "MediaBox" => Object::Array(vec![0.into(), 0.into(), 612.into(), 792.into()]),
            "Annots" => list.clone(),
        });
        pdf.objects.insert(
            pages_id,
            Object::Dictionary(
                dictionary! { "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1 },
            ),
        );
        let catalog = pdf.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        pdf.trailer.set("Root", catalog);

        let mine: Vec<bool> = list.iter().map(|e| is_ours(&pdf, e)).collect();
        assert_eq!(mine, vec![true, true, true, false, false, false]);
        let found = annots_in(&pdf, upright(&pdf));
        assert_eq!(found.sheaf()[&0].len(), 2);
        let subtypes: Vec<&str> = (found.foreign().into_iter())
            .map(|(_, f)| f.subtype.as_str())
            .collect();
        assert_eq!(subtypes, vec!["StrikeOut", "Caret", "StrikeOut"]);
    }

    /// One page carrying a third-party sticky note, a link (another tool's,
    /// not a comment), a popup (left to the page), and one of ours, each
    /// with its object.
    #[test]
    fn reads_foreign_and_sheaf_annotations() {
        let mut pdf = lopdf::Document::with_version("1.7");
        let pages_id = pdf.new_object_id();
        let content = pdf.add_object(Stream::new(dictionary! {}, b"".to_vec()));
        let note = pdf.add_object(dictionary! {
            "Type" => "Annot",
            "Subtype" => "Text",
            "Rect" => Object::Array(vec![100.0.into(), 700.0.into(), 120.0.into(), 720.0.into()]),
            "T" => Object::String(b"Reviewer".to_vec(), lopdf::StringFormat::Literal),
            "Contents" => Object::String(vec![0xFE, 0xFF, 0x00, b'H', 0x00, b'i', 0x20, 0x14], lopdf::StringFormat::Hexadecimal),
            "M" => Object::String(b"D:20260101120000Z".to_vec(), lopdf::StringFormat::Literal),
        });
        let link = pdf.add_object(dictionary! {
            "Type" => "Annot",
            "Subtype" => "Link",
            "Rect" => Object::Array(vec![0.0.into(), 0.0.into(), 10.0.into(), 10.0.into()]),
        });
        let popup = pdf.add_object(dictionary! {
            "Type" => "Annot",
            "Subtype" => "Popup",
            "Rect" => Object::Array(vec![0.0.into(), 0.0.into(), 10.0.into(), 10.0.into()]),
        });
        let ours = Annot {
            id: crate::annot::AnnotId(3),
            kind: crate::annot::AnnotKind::Note {
                pos: Pt { x: 1.0, y: 2.0 },
            },
            color: [1, 2, 3],
            opacity: 1.0,
            stroke: 1.0,
            author: "me".into(),
            contents: "mine".into(),
            modified: String::new(),
            created: String::new(),
            fill: None,
            dashed: false,
            cloudy: false,
            turn: Rotation::R0,
            name: String::new(),
        };
        let dict = crate::serialize::built::to_pdf(&ours, &mut pdf).main;
        let mine = pdf.add_object(dict);
        let page = pdf.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "MediaBox" => Object::Array(vec![0.into(), 0.into(), 612.into(), 792.into()]),
            "Contents" => content,
            "Annots" => Object::Array(vec![note.into(), link.into(), popup.into(), mine.into()]),
        });
        pdf.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1,
            }),
        );
        let catalog = pdf.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        pdf.trailer.set("Root", catalog);

        let found = annots_in(&pdf, upright(&pdf));
        assert_eq!(found.sheaf()[&0], vec![ours]);
        let objects: Vec<Option<ObjectId>> = found.pages[&0].iter().map(|a| a.object).collect();
        assert_eq!(objects, [Some(note), Some(link), Some(mine)]);
        let foreign = found.foreign();
        let subtypes: Vec<&str> = foreign.iter().map(|(_, f)| f.subtype.as_str()).collect();
        assert_eq!(subtypes, ["Text", "Link"]);
        assert!(foreign[1].1.is_link() && !foreign[0].1.is_link());
        let f = foreign[0].1;
        assert_eq!(f.subtype, "Text");
        assert_eq!(f.author, "Reviewer");
        assert_eq!(f.contents, "Hi\u{2014}");
        assert_eq!(f.modified, "D:20260101120000Z");
        // Its /Rect, which is PageSpace.
        assert_eq!(
            (f.rect.x, f.rect.y, f.rect.w, f.rect.h),
            (100.0, 700.0, 20.0, 20.0)
        );
    }

    /// One page with a mark and its companion, another tool's comment and
    /// a reply to it, a stamp with an appearance and a file attachment:
    /// read from a source, as a file opens, they read as they do loaded,
    /// the stamp converted through its appearance.
    #[test]
    fn annotations_read_from_a_source_as_loaded() {
        let mut pdf = lopdf::Document::with_version("1.7");
        let pages_id = pdf.new_object_id();
        let rect = || Object::Array(vec![10.into(), 10.into(), 20.into(), 20.into()]);
        let text = |s: &str| Object::string_literal(s);
        let at = Box2 {
            x: 10.0,
            y: 10.0,
            w: 30.0,
            h: 10.0,
        };
        let ours = Annot {
            id: crate::annot::AnnotId(9),
            kind: crate::annot::AnnotKind::Replace {
                rects: vec![at],
                struck: "old".into(),
                at,
            },
            color: [1, 2, 3],
            opacity: 1.0,
            stroke: 1.0,
            author: "me".into(),
            contents: "mine".into(),
            modified: String::new(),
            created: String::new(),
            fill: None,
            dashed: false,
            cloudy: false,
            turn: Rotation::R0,
            name: String::new(),
        };
        let written = crate::serialize::built::to_pdf(&ours, &mut pdf);
        let main = pdf.add_object(written.main);
        let [mut strike] = <[_; 1]>::try_from(written.companions).ok().unwrap();
        strike.set("IRT", main);
        strike.set("RT", Object::Name(b"Group".to_vec()));
        let part = pdf.add_object(strike);
        let note = pdf.add_object(dictionary! {
            "Type" => "Annot", "Subtype" => "Text", "Rect" => rect(), "NM" => text("n"),
            "T" => text("Reviewer"), "Contents" => text("Why?"),
        });
        let reply = pdf.add_object(dictionary! {
            "Type" => "Annot", "Subtype" => "Text", "Rect" => rect(), "IRT" => note,
            "Contents" => text("Because."),
        });
        // The same content every run, so its stamp page is one cache file.
        let form = pdf.add_object(Stream::new(
            dictionary! {
                "Type" => "XObject", "Subtype" => "Form",
                "BBox" => vec![0.into(), 0.into(), 100.into(), 40.into()],
            },
            b"0 0 1 rg 0 0 100 40 re f".to_vec(),
        ));
        let stamp = pdf.add_object(dictionary! {
            "Type" => "Annot", "Subtype" => "Stamp", "Name" => "SBApproved",
            "Rect" => vec![100.into(), 600.into(), 200.into(), 640.into()],
            "AP" => dictionary! { "N" => form },
        });
        let file = pdf.add_object(Stream::new(dictionary! {}, b"attached".to_vec()));
        let attached = pdf.add_object(dictionary! {
            "Type" => "Annot", "Subtype" => "FileAttachment", "Rect" => rect(),
            "FS" => dictionary! {
                "Type" => "Filespec", "F" => text("a.txt"), "EF" => dictionary! { "F" => file },
            },
        });
        let list: Vec<Object> = [main, part, note, reply, stamp, attached]
            .into_iter()
            .map(Object::Reference)
            .collect();
        let page = pdf.add_object(dictionary! {
            "Type" => "Page", "Parent" => pages_id,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "Annots" => list,
        });
        pdf.objects.insert(
            pages_id,
            dictionary! { "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1 }.into(),
        );
        let catalog = pdf.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        pdf.trailer.set("Root", catalog);

        let src = crate::open::tests::lazily(&mut pdf);
        let loaded = annots_in(&pdf, upright(&pdf));
        let read = annots_in(src.pdf(), src.pages().iter().map(|p| (p.id, p.geom.rotate)));
        assert_eq!(read.pages, loaded.pages);
        assert_eq!(loaded.sheaf()[&0], vec![ours]);
        let foreign: Vec<&ForeignAnnot> = loaded.foreign().into_iter().map(|(_, f)| f).collect();
        let subtypes: Vec<&str> = foreign.iter().map(|f| f.subtype.as_str()).collect();
        assert_eq!(subtypes, ["Text", "Text", "Stamp", "FileAttachment"]);
        assert_eq!(foreign[1].parent.as_deref(), Some("n"));
        assert!(matches!(
            foreign[2].mark.as_deref(),
            Some(Annot {
                kind: crate::annot::AnnotKind::PageStamp { .. },
                ..
            })
        ));
    }
}
