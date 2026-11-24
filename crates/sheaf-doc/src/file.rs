//! Direct access to a PDF file's object graph. This is the
//! serialization backend (see docs/document-model.md), not the editing
//! model; the viewer never touches it.
//!
//! **A file's annotations as the open lists them** ([`annots_in`]). The
//! walk of each page's `/Annots` is sheaf's: every entry followed, its
//! slot its place in the array, and sheaf's own marks found on the entry
//! as written (`sheaf_mark`: its `/SHEF_Annot`, fresh by its staleness
//! digest) with their companions (`is_ours`), so none of them reaches
//! another reader. Every other annotation (a stale mark of sheaf's too)
//! is read by pdf-annots, one dictionary at a time, under
//! PDFium's rules and sheaf's caps (`source::annots::read`; M18 slice
//! 24), and listed ([`ForeignAnnot`]) and converted to the mark editing it
//! makes ([`import::convert_page`](crate::import::convert_page)) from what
//! it read. What's never listed (a widget, a pop-up, media), by the subtype
//! it's written with, isn't read; one pdf-annots refuses is passed over
//! alone, and said in the log. What the family's reading has no place for
//! is read from its dictionary, as PDFium reads it: a review state on a
//! subtype that isn't a note, the author of an annotation that isn't
//! markup, `/IRT` written in place, and the dates, kept as written. A name
//! for one without `/NM` ([`anon_name`]) and a thread's names
//! ([`thread_name`]) are sheaf's.

use std::path::Path;

use anyhow::{Context, Result};

pub struct PdfFile {
    inner: crate::pdf::Document,
}

impl PdfFile {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let inner = crate::pdf::Document::load(path)
            .with_context(|| format!("loading {}", path.display()))?;
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

    pub fn into_inner(self) -> crate::pdf::Document {
        self.inner
    }
}

use std::collections::HashMap;

use crate::pdf::annots::{self, AnnotFlags, TypedAnnot};
use crate::pdf::{Dictionary, Object, ObjectId, Value};
use sheaf_edit::Rotation;

use crate::annot::{Annot, AnnotKind, Box2, Pt, from_json};
use crate::attachments::Objects;
use crate::content::{ReadObjects, Unkept};

/// The mark an `/Annots` entry carries, when this build can read it and
/// no other tool has changed it since sheaf wrote it (M10.5 §3.6): its
/// payload is this build's version ([`VERSION`](crate::annot::VERSION)),
/// and fresh while its digest is the dictionary's
/// ([`staleness`](crate::annot::staleness)). A page stamp must hold its
/// page and an attachment its file ([`holds`]). None for anything else,
/// which is another tool's.
pub(crate) fn sheaf_mark<P: Objects + ?Sized>(pdf: &P, entry: &Object) -> Option<Annot> {
    let (_, dict) = pdf.dereference(entry).ok()?;
    let Value::Dict(dict) = dict.value() else {
        return None;
    };
    let (_, json) = pdf.dereference(dict.get(b"SHEF_Annot")?).ok()?;
    let payload = from_json(text_string(json.as_bytes()?).as_bytes())?;
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
    forms.iter().next()?.1.as_reference()
}

/// The embedded file an attachment's file specification holds, `dict` its
/// dictionary: its stream, which isn't read.
pub(crate) fn attached_file<P: Objects + ?Sized>(pdf: &P, dict: &Dictionary) -> Option<ObjectId> {
    let ef = dict_at(pdf, dict_at(pdf, dict, b"FS")?, b"EF")?;
    ef.get(b"F").or_else(|| ef.get(b"UF"))?.as_reference()
}

/// `d`'s `key`, its references followed: a dictionary, or a stream's.
fn dict_at<'a, P: Objects + ?Sized>(
    pdf: &'a P,
    d: &'a Dictionary,
    key: &[u8],
) -> Option<&'a Dictionary> {
    pdf.dereference(d.get(key)?).ok()?.1.as_dict()
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
        .and_then(|(_, o)| match o.value() {
            Value::Dict(d) => Some(d),
            _ => None,
        })
    else {
        return false;
    };
    let mark = |entry: &Object| sheaf_mark(pdf, entry);
    if dict.has(b"SHEF_Annot") {
        return mark(entry).is_some();
    }
    // One step only: a companion's /IRT is its mark, never another companion.
    let Some(irt) = dict.get(b"IRT") else {
        return false;
    };
    let name = |key: &[u8], is: &[u8]| {
        dict.get(key)
            .and_then(Object::as_name)
            .is_some_and(|n| n == is)
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
    /// `/C` as RGB, gray and CMYK made RGB, when it has one.
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
pub fn anon_name(dict: &crate::pdf::Dictionary) -> String {
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
        let v = dict.get(key);
        match v.map(Object::value) {
            Some(Value::Array(a)) => {
                for o in a {
                    // As an f32, as reals were read before, so names stay.
                    eat(format!("{:.2}", o.as_number().unwrap_or(0.0) as f32).as_bytes());
                }
            }
            Some(Value::Name(n)) => eat(n.as_bytes()),
            Some(_) => eat(pdf_text(v.unwrap()).as_bytes()),
            None => eat(b""),
        }
    }
    format!("sheaf-anon:{}", sheaf_util::checksum_hex(&fields))
}

/// What an annotation is called in threads: its `/NM`, else [`anon_name`].
pub fn thread_name(dict: &crate::pdf::Dictionary) -> String {
    dict.get(b"NM")
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
pub fn load_pdf_mem(bytes: &[u8], password: Option<&str>) -> Result<crate::pdf::Document> {
    match password {
        Some(pw) => crate::pdf::Document::load_mem_with_password(bytes, pw)
            .map_err(|e| anyhow::anyhow!("{e}"))
            .context("decrypting"),
        None => {
            let pdf = crate::pdf::Document::load_mem(bytes)?;
            if pdf.is_encrypted() {
                anyhow::bail!("encrypted PDF requires a password");
            }
            Ok(pdf)
        }
    }
}

/// Load a file into memory through `pdf-structure` and, if it's encrypted,
/// decrypt it with `password` (or the empty user password many files use).
/// Decrypted documents omit `/Encrypt` from their trailer; sheaf's writers
/// take the encryption from the source instead
/// ([`Source::crypt`](crate::source::Source::crypt)), and keep it.
pub fn load_pdf(path: &Path, password: Option<&str>) -> Result<crate::pdf::Document> {
    match password {
        Some(pw) => crate::pdf::Document::load_with_password(path, pw)
            .map_err(|e| anyhow::anyhow!("{e}"))
            .with_context(|| format!("decrypting {}", path.display())),
        None => {
            let pdf = crate::pdf::Document::load(path)
                .with_context(|| format!("loading {}", path.display()))?;
            if pdf.is_encrypted() {
                anyhow::bail!("decrypting {}: password required", path.display());
            }
            Ok(pdf)
        }
    }
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
pub fn pdf_text(o: &crate::pdf::Object) -> String {
    match o.as_bytes() {
        Some(bytes) => pdf_text_bytes(bytes),
        None => String::new(),
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
fn is_listed_subtype(s: &[u8]) -> bool {
    !matches!(
        s,
        b"Widget"
            | b"Popup"
            | b"Screen"
            | b"PrinterMark"
            | b"TrapNet"
            | b"Watermark"
            | b"3D"
            | b"Movie"
            | b"Sound"
            | b"RichMedia"
    )
}

/// Every mark, comment and link on `pages`, the file's page objects by
/// index, each with its `/Rotate` as the page table reads it
/// ([`Source::geom`](crate::Source::geom)), which another tool's comment
/// is converted with, each mark with what it holds: its page stamp's page
/// or its attachment's file. sheaf's marks and their companions are found
/// on the entry as written (by its `/SHEF_Annot` and digest); every other
/// annotation is read by `pdf-annots` (`source::annots::read`),
/// listed and converted from what it read. One it can't read is passed
/// over, and said in the log.
pub fn annots_in<P: Objects + ReadObjects + ?Sized>(
    pdf: &P,
    pages: impl IntoIterator<Item = (ObjectId, Rotation)>,
) -> AnnotsInFile {
    walk(pdf, pages, |page, slot, why| {
        log::warn!("page {}, annotation {slot}: not read: {why}", page + 1);
    })
}

/// [`annots_in`], each entry `pdf-annots` couldn't read given to
/// `refused` as it's passed over: its page's index, its slot, and why.
pub(crate) fn walk<P: Objects + ReadObjects + ?Sized>(
    pdf: &P,
    pages: impl IntoIterator<Item = (ObjectId, Rotation)>,
    mut refused: impl FnMut(u32, usize, annots::Error),
) -> AnnotsInFile {
    let mut out = AnnotsInFile::default();
    for (i, (page_id, turn)) in pages.into_iter().enumerate() {
        let Ok(page) = pdf.get_dictionary(page_id) else {
            continue;
        };
        let Some(annots) = page
            .get(b"Annots")
            .and_then(|a| pdf.dereference(a).ok().map(|(_, o)| o))
        else {
            continue;
        };
        let Some(list) = annots.as_array() else {
            continue;
        };
        if list.is_empty() {
            continue;
        }
        // What the page's annotations read that the source doesn't keep:
        // their appearances' dictionaries, held until they're converted.
        let unkept = Unkept::default();
        let mut found = Vec::new();
        // Other tools', each by its place in `found`, as read, to convert
        // together.
        let mut comments = Vec::new();
        let mut read = Vec::new();
        for (slot, entry) in list.iter().enumerate() {
            let Ok((object, obj)) = pdf.dereference(entry) else {
                continue;
            };
            let Value::Dict(dict) = obj.value() else {
                continue;
            };
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
            // What's never listed, by the name it's written with, isn't
            // read: pdf-annots would read it as that subtype too, and a
            // form's thousands of widgets would be read for nothing.
            if (dict.get(b"Subtype").and_then(Object::as_name))
                .is_some_and(|s| !is_listed_subtype(s))
            {
                continue;
            }
            let a = match crate::source::annots::read(pdf, &unkept, dict, object) {
                Ok(a) => a,
                Err(why) => {
                    refused(i as u32, slot, why);
                    continue;
                }
            };
            let Some(foreign) = foreign(pdf, &a, slot) else {
                continue;
            };
            // A link is no mark sheaf can hold.
            if !foreign.is_link() {
                comments.push(found.len());
                read.push(a);
            }
            found.push(InFile {
                object,
                slot,
                found: Found::Foreign(foreign),
            });
        }
        if !read.is_empty() {
            let marks = crate::import::convert_page(pdf, &read, turn, Default::default());
            for ((at, a), m) in comments.into_iter().zip(&read).zip(marks) {
                if let Found::Foreign(f) = &mut found[at].found {
                    f.mark = m.ok().map(Box::new);
                    f.stamp = (f.mark.as_deref())
                        .filter(|m| matches!(m.kind, AnnotKind::PageStamp { .. }))
                        .and_then(|_| crate::import::normal_form(a));
                }
            }
        }
        if !found.is_empty() {
            out.pages.insert(i as u32, found);
        }
    }
    out
}

/// What the open lists of another tool's annotation `a`, as `pdf-annots`
/// read it, in slot `slot`: None for what isn't listed (a form field, a
/// popup, what has no `/Rect` of four numbers).
fn foreign<P: Objects + ?Sized>(
    pdf: &P,
    a: &annots::Annot<'_>,
    slot: usize,
) -> Option<ForeignAnnot> {
    let dict = a.dict();
    let subtype = crate::import::subtype_of(a);
    if subtype.is_empty() || !is_listed_subtype(subtype.as_bytes()) || !a.has_valid_rect() {
        return None;
    }
    let [x0, y0, x1, y1] = a.raw_rect();
    let text = |key: &[u8]| dict.get(key).map(pdf_text).unwrap_or_default();
    let name_of = |key: &[u8]| -> Option<String> {
        let (_, o) = pdf.dereference(dict.get(key)?).ok()?;
        let Value::Dict(d) = o.value() else {
            return None;
        };
        Some(thread_name(d))
    };
    // A note's state is the family's; another subtype's, as written.
    let (model, state) = match a.typed() {
        TypedAnnot::Text(t) => (
            t.state_model.clone().unwrap_or_default(),
            t.state.clone().unwrap_or_default(),
        ),
        _ => (text(b"StateModel"), text(b"State")),
    };
    let state = match (model, state) {
        (_, s) if s.is_empty() => None,
        // Acrobat's default model.
        (m, s) if m.is_empty() => Some((crate::annot::REVIEW.to_string(), s)),
        (m, s) => Some((m, s)),
    };
    let markup = a.markup();
    Some(ForeignAnnot {
        subtype,
        // What isn't a markup annotation has no author in the family's
        // view: its `/T` as PDFium reads any annotation's.
        author: (markup.and_then(|m| m.author.clone()))
            .or_else(|| {
                let (_, t) = pdf.dereference(dict.get(b"T")?).ok()?;
                Some(text_string(t.as_bytes()?))
            })
            .unwrap_or_default(),
        contents: a.contents().unwrap_or_default().to_string(),
        modified: text(b"M"),
        rect: Box2::from_corners(Pt { x: x0, y: y0 }, Pt { x: x1, y: y1 }),
        name: thread_name(dict),
        parent: name_of(b"IRT"),
        grouped: crate::import::is_grouped(a),
        state,
        created: text(b"CreationDate"),
        slot,
        hidden: a.flags().contains(AnnotFlags::HIDDEN),
        color: a.color().and_then(crate::import::rgb),
        mark: None,
        stamp: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pdf::{Object, Stream, dictionary, literal};

    /// `pdf`'s pages, none of them turned, as [`annots_in`] takes them.
    fn upright(
        pdf: &crate::pdf::Document,
    ) -> impl Iterator<Item = (crate::pdf::ObjectId, Rotation)> + '_ {
        pdf.page_iter().map(|page| (page, Rotation::R0))
    }

    /// Reals are f64s. Read and written back, a fraction, a large number
    /// and a 16-digit coordinate keep their digits, in a dictionary and in
    /// content; and an f32 that sheaf computes is written as the digits it
    /// always was.
    #[test]
    fn reals_keep_their_digits() {
        use crate::pdf::content::{Content, Operation};
        const TEXTS: [&str; 3] = ["0.1", "123456.789", "306.1234567890123"];
        let reals = || {
            TEXTS
                .map(|t| Object::from(t.parse::<f64>().unwrap()))
                .to_vec()
        };
        let mut pdf = crate::pdf::Document::with_version("1.7");
        let pages_id = pdf.new_object_id();
        let ops = format!("{} {} m\n{} w", TEXTS[1], TEXTS[2], TEXTS[0]);
        let content = pdf.add_object(Stream::new(dictionary! {}, ops.clone().into_bytes()));
        let page = pdf.add_object(dictionary! {
            "Type" => Object::name("Page"), "Parent" => pages_id, "Contents" => content,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "SheafReals" => reals(),
        });
        pdf.objects.insert(
            pages_id,
            dictionary! { "Type" => Object::name("Pages"), "Kids" => vec![page.into()], "Count" => 1 }.into(),
        );
        let catalog =
            pdf.add_object(dictionary! { "Type" => Object::name("Catalog"), "Pages" => pages_id });
        pdf.trailer.set("Root", catalog);
        let mut first = Vec::new();
        pdf.save_to(&mut first).unwrap();

        let mut read = crate::pdf::Document::load_mem(&first).unwrap();
        let mut written = Vec::new();
        read.save_to(&mut written).unwrap();
        let array = format!("[{}]", TEXTS.join(" "));
        assert!(
            written.windows(array.len()).any(|w| w == array.as_bytes()),
            "{}",
            String::from_utf8_lossy(&written)
        );
        let again = crate::pdf::Document::load_mem(&written).unwrap();
        let dict = again.get_dictionary(page).unwrap();
        assert!(
            dict.get(b"SheafReals")
                .unwrap()
                .eq_value(&Object::from(reals()))
        );
        let lim = crate::source::Limits::DEFAULT.content.family();
        let content = again.get_page_content(page);
        let operands: Vec<Object> =
            crate::content::operations(&content, &crate::content::ResCx::none(), &lim)
                .flat_map(|op| {
                    let op = op.unwrap();
                    let read: Vec<Object> =
                        op.operands().map(|o| o.object(&lim).unwrap()).collect();
                    read
                })
                .collect();
        let [a, b, c] = reals().try_into().unwrap();
        assert_eq!(operands, vec![b.clone(), c.clone(), a.clone()]);
        let again = Content {
            operations: vec![
                Operation::new("m", vec![b, c]),
                Operation::new("w", vec![a]),
            ],
        };
        assert_eq!(String::from_utf8(again.encode().unwrap()).unwrap(), ops);

        // An f32 writes its shortest digits, a point always so it reads back as a Real.
        let reals = [0.1f32, 612.0, -3.0, 16_777_216.0, 1e-7].map(Object::from);
        let width = Content {
            operations: vec![Operation::new("w", reals.to_vec())],
        };
        assert_eq!(
            width.encode().unwrap(),
            b"0.1 612.0 -3.0 16777216.0 0.0000001 w"
        );
    }

    #[test]
    fn their_threads_read_with_states_and_groups() {
        let mut pdf = crate::pdf::Document::with_version("1.7");
        let pages_id = pdf.new_object_id();
        let content = pdf.add_object(Stream::new(dictionary! {}, b"".to_vec()));
        let rect = || Object::from(vec![10.into(), 10.into(), 20.into(), 20.into()]);
        let text = |s: &str| literal(s);
        let parent = pdf.add_object(dictionary! {
            "Type" => Object::name("Annot"), "Subtype" => Object::name("Highlight"), "Rect" => rect(), "NM" => text("p"),
        });
        let accepted = pdf.add_object(dictionary! {
            "Type" => Object::name("Annot"), "Subtype" => Object::name("Text"), "Rect" => rect(), "IRT" => parent,
            "State" => text("Accepted"), "StateModel" => text("Review"), "F" => 30,
        });
        // No /StateModel: Review, as Acrobat reads it.
        let completed = pdf.add_object(dictionary! {
            "Type" => Object::name("Annot"), "Subtype" => Object::name("Text"), "Rect" => rect(), "IRT" => parent,
            "State" => text("Completed"),
        });
        let part = pdf.add_object(dictionary! {
            "Type" => Object::name("Annot"), "Subtype" => Object::name("StrikeOut"), "Rect" => rect(), "IRT" => parent,
            "RT" => Object::name("Group"),
        });
        let list: Vec<Object> = [parent, accepted, completed, part]
            .into_iter()
            .map(Object::from)
            .collect();
        let page = pdf.add_object(dictionary! {
            "Type" => Object::name("Page"), "Parent" => pages_id, "Contents" => content,
            "MediaBox" => Object::from(vec![0.into(), 0.into(), 612.into(), 792.into()]),
            "Annots" => list,
        });
        pdf.objects.insert(
            pages_id,
            Object::from(
                dictionary! { "Type" => Object::name("Pages"), "Kids" => vec![page.into()], "Count" => 1 },
            ),
        );
        let catalog =
            pdf.add_object(dictionary! { "Type" => Object::name("Catalog"), "Pages" => pages_id });
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

    /// An annotation pdf-annots refuses (a link whose chain of actions is
    /// past the cap) is passed over and said, by its page and slot, with
    /// why; the page's other entries are listed as ever, each in its own
    /// slot.
    #[test]
    fn an_annotation_the_family_refuses_is_passed_over_alone() {
        let mut pdf = crate::pdf::Document::with_version("1.7");
        let pages_id = pdf.new_object_id();
        let content = pdf.add_object(Stream::new(dictionary! {}, b"".to_vec()));
        let rect = || Object::from(vec![10.into(), 10.into(), 20.into(), 20.into()]);
        let note = pdf.add_object(dictionary! {
            "Type" => Object::name("Annot"), "Subtype" => Object::name("Text"), "Rect" => rect(),
        });
        let chain = crate::source::showing::tests::long_chain(&mut pdf);
        let link = pdf.add_object(dictionary! {
            "Type" => Object::name("Annot"), "Subtype" => Object::name("Link"), "Rect" => rect(), "A" => chain,
        });
        let highlight = pdf.add_object(dictionary! {
            "Type" => Object::name("Annot"), "Subtype" => Object::name("Highlight"), "Rect" => rect(),
        });
        let list: Vec<Object> = [note, link, highlight].map(Object::from).to_vec();
        let page = pdf.add_object(dictionary! {
            "Type" => Object::name("Page"), "Parent" => pages_id, "Contents" => content,
            "MediaBox" => Object::from(vec![0.into(), 0.into(), 612.into(), 792.into()]),
            "Annots" => list,
        });
        pdf.objects.insert(
            pages_id,
            Object::from(
                dictionary! { "Type" => Object::name("Pages"), "Kids" => vec![page.into()], "Count" => 1 },
            ),
        );
        let catalog =
            pdf.add_object(dictionary! { "Type" => Object::name("Catalog"), "Pages" => pages_id });
        pdf.trailer.set("Root", catalog);

        let mut refused = Vec::new();
        let read = walk(&pdf, upright(&pdf), |page, slot, why| {
            refused.push((page, slot, why.to_string()));
        });
        assert_eq!(
            refused,
            [(0, 1, "exceeded cap on chained actions".to_string())]
        );
        let listed: Vec<(usize, &str)> = (read.pages[&0].iter())
            .map(|a| match &a.found {
                Found::Foreign(f) => (a.slot, f.subtype.as_str()),
                Found::Mark { .. } => (a.slot, "a mark"),
            })
            .collect();
        assert_eq!(listed, [(0, "Text"), (2, "Highlight")]);
        // The open's own walk lists the same.
        assert_eq!(annots_in(&pdf, upright(&pdf)).foreign().len(), 2);
    }

    /// A mark this build can't read, a kind from a newer sheaf or a
    /// payload of another version, is another tool's, listed as one and
    /// kept.
    #[test]
    fn a_mark_this_build_cant_read_is_another_tool_s() {
        let mut pdf = crate::pdf::Document::with_version("1.7");
        let pages_id = pdf.new_object_id();
        let content = pdf.add_object(Stream::new(dictionary! {}, b"".to_vec()));
        let rect = || Object::from(vec![10.into(), 10.into(), 20.into(), 20.into()]);
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
        let json = text_string(main.get(b"SHEF_Annot").unwrap().as_bytes().unwrap());
        let version = format!("\"v\":{}", crate::annot::VERSION);
        assert!(json.contains(&version), "{json}");
        let json = json.replace(&version, "\"v\":999");
        other_version.set("SHEF_Annot", literal(json));
        let main = pdf.add_object(main);
        let other_version = pdf.add_object(other_version);
        let unknown = pdf.add_object(dictionary! {
            "Type" => Object::name("Annot"), "Subtype" => Object::name("StrikeOut"), "Rect" => rect(),
            "SHEF_Annot" => literal(
                format!(r#"{{"v":{},"id":6,"kind":{{"Future":{{}}}}}}"#, crate::annot::VERSION),
            ),
        });
        let list: Vec<Object> = [main, other_version, unknown]
            .into_iter()
            .map(Object::from)
            .collect();
        let page = pdf.add_object(dictionary! {
            "Type" => Object::name("Page"), "Parent" => pages_id, "Contents" => content,
            "MediaBox" => Object::from(vec![0.into(), 0.into(), 612.into(), 792.into()]),
            "Annots" => list.clone(),
        });
        pdf.objects.insert(
            pages_id,
            Object::from(
                dictionary! { "Type" => Object::name("Pages"), "Kids" => vec![page.into()], "Count" => 1 },
            ),
        );
        let catalog =
            pdf.add_object(dictionary! { "Type" => Object::name("Catalog"), "Pages" => pages_id });
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
        let mut pdf = crate::pdf::Document::with_version("1.7");
        let pages_id = pdf.new_object_id();
        let content = pdf.add_object(Stream::new(dictionary! {}, b"".to_vec()));
        let rect = || Object::from(vec![10.into(), 10.into(), 20.into(), 20.into()]);
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
        strike.set("RT", Object::name("Group"));
        let strike = pdf.add_object(strike);
        let note = mark(
            6,
            crate::annot::AnnotKind::Note {
                pos: Pt { x: 1.0, y: 2.0 },
            },
        );
        let note = crate::serialize::built::to_pdf(&note, &mut pdf).main;
        let note = pdf.add_object(note);
        let grouped = |pdf: &mut crate::pdf::Document, under| {
            pdf.add_object(dictionary! {
                "Type" => Object::name("Annot"), "Subtype" => Object::name("StrikeOut"), "Rect" => rect(),
                "IRT" => under, "RT" => Object::name("Group"),
            })
        };
        let under_note = grouped(&mut pdf, note);
        let their_caret = pdf.add_object(dictionary! {
            "Type" => Object::name("Annot"), "Subtype" => Object::name("Caret"), "Rect" => rect(),
        });
        let under_theirs = grouped(&mut pdf, their_caret);
        let list: Vec<Object> = [caret, strike, note, under_note, their_caret, under_theirs]
            .into_iter()
            .map(Object::from)
            .collect();
        let page = pdf.add_object(dictionary! {
            "Type" => Object::name("Page"), "Parent" => pages_id, "Contents" => content,
            "MediaBox" => Object::from(vec![0.into(), 0.into(), 612.into(), 792.into()]),
            "Annots" => list.clone(),
        });
        pdf.objects.insert(
            pages_id,
            Object::from(
                dictionary! { "Type" => Object::name("Pages"), "Kids" => vec![page.into()], "Count" => 1 },
            ),
        );
        let catalog =
            pdf.add_object(dictionary! { "Type" => Object::name("Catalog"), "Pages" => pages_id });
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
        let mut pdf = crate::pdf::Document::with_version("1.7");
        let pages_id = pdf.new_object_id();
        let content = pdf.add_object(Stream::new(dictionary! {}, b"".to_vec()));
        let note = pdf.add_object(dictionary! {
            "Type" => Object::name("Annot"),
            "Subtype" => Object::name("Text"),
            "Rect" => Object::from(vec![100.0.into(), 700.0.into(), 120.0.into(), 720.0.into()]),
            "T" => literal(b"Reviewer"),
            "Contents" => Object::hex(vec![0xFE, 0xFF, 0x00, b'H', 0x00, b'i', 0x20, 0x14]),
            "M" => literal(b"D:20260101120000Z"),
        });
        let link = pdf.add_object(dictionary! {
            "Type" => Object::name("Annot"),
            "Subtype" => Object::name("Link"),
            "Rect" => Object::from(vec![0.0.into(), 0.0.into(), 10.0.into(), 10.0.into()]),
        });
        let popup = pdf.add_object(dictionary! {
            "Type" => Object::name("Annot"),
            "Subtype" => Object::name("Popup"),
            "Rect" => Object::from(vec![0.0.into(), 0.0.into(), 10.0.into(), 10.0.into()]),
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
            "Type" => Object::name("Page"),
            "Parent" => pages_id,
            "MediaBox" => Object::from(vec![0.into(), 0.into(), 612.into(), 792.into()]),
            "Contents" => content,
            "Annots" => Object::from(vec![note.into(), link.into(), popup.into(), mine.into()]),
        });
        pdf.objects.insert(
            pages_id,
            Object::from(dictionary! {
                "Type" => Object::name("Pages"), "Kids" => vec![page.into()], "Count" => 1,
            }),
        );
        let catalog =
            pdf.add_object(dictionary! { "Type" => Object::name("Catalog"), "Pages" => pages_id });
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
        let mut pdf = crate::pdf::Document::with_version("1.7");
        let pages_id = pdf.new_object_id();
        let rect = || Object::from(vec![10.into(), 10.into(), 20.into(), 20.into()]);
        let text = |s: &str| literal(s);
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
        strike.set("RT", Object::name("Group"));
        let part = pdf.add_object(strike);
        let note = pdf.add_object(dictionary! {
            "Type" => Object::name("Annot"), "Subtype" => Object::name("Text"), "Rect" => rect(), "NM" => text("n"),
            "T" => text("Reviewer"), "Contents" => text("Why?"),
        });
        let reply = pdf.add_object(dictionary! {
            "Type" => Object::name("Annot"), "Subtype" => Object::name("Text"), "Rect" => rect(), "IRT" => note,
            "Contents" => text("Because."),
        });
        // The same content every run, so its stamp page is one cache file.
        let form = pdf.add_object(Stream::new(
            dictionary! {
                "Type" => Object::name("XObject"), "Subtype" => Object::name("Form"),
                "BBox" => vec![0.into(), 0.into(), 100.into(), 40.into()],
            },
            b"0 0 1 rg 0 0 100 40 re f".to_vec(),
        ));
        let stamp = pdf.add_object(dictionary! {
            "Type" => Object::name("Annot"), "Subtype" => Object::name("Stamp"), "Name" => Object::name("SBApproved"),
            "Rect" => vec![100.into(), 600.into(), 200.into(), 640.into()],
            "AP" => dictionary! { "N" => form },
        });
        let file = pdf.add_object(Stream::new(dictionary! {}, b"attached".to_vec()));
        let attached = pdf.add_object(dictionary! {
            "Type" => Object::name("Annot"), "Subtype" => Object::name("FileAttachment"), "Rect" => rect(),
            "FS" => dictionary! {
                "Type" => Object::name("Filespec"), "F" => text("a.txt"), "EF" => dictionary! { "F" => file },
            },
        });
        let list: Vec<Object> = [main, part, note, reply, stamp, attached]
            .into_iter()
            .map(Object::from)
            .collect();
        let page = pdf.add_object(dictionary! {
            "Type" => Object::name("Page"), "Parent" => pages_id,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "Annots" => list,
        });
        pdf.objects.insert(
            pages_id,
            dictionary! { "Type" => Object::name("Pages"), "Kids" => vec![page.into()], "Count" => 1 }.into(),
        );
        let catalog =
            pdf.add_object(dictionary! { "Type" => Object::name("Catalog"), "Pages" => pages_id });
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
