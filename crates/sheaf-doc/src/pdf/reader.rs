//! M18 slice 8: `LazyPdf` backed by `pdf_structure::Reading`, converting
//! objects to `lopdf`'s model at the door until slice 11 moves sheaf onto
//! `pdf_objects`' types directly.

use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::sync::{Arc, OnceLock};

use lopdf::encryption::DecryptionError;
use lopdf::{
    Dictionary, Document, EncryptionState, Error, Limit, Object, ObjectId, Result, Stream,
    StringFormat,
};
use pdf_crypt::Password;
use pdf_filters::Backend;
use pdf_objects::object::{Data, Dict, Form, Object as PdfObj, ObjectId as PdfId, Str, Value};
use pdf_objects::parse::Parser;
use pdf_structure::read::{Fault, Options};
use pdf_structure::{ByteSource, Encryption, Origin, Reading, Record, Unread};

/// Caps on what [`LazyPdf`] reads. Breaking one is [`Error::Limit`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadLimits {
    /// Bytes an object stream or a cross-reference stream may decompress to.
    pub max_decompressed: usize,
    /// Cross-reference sections in the `/Prev` chain.
    pub max_sections: usize,
    /// Object numbers: the cross-reference table's size (ISO 32000-1 Annex C's 8,388,607
    /// indirect objects).
    pub max_objects: u32,
    /// Objects on one read's resolution path, and references in one chain.
    pub max_depth: usize,
}

impl ReadLimits {
    pub const DEFAULT: ReadLimits = ReadLimits {
        max_decompressed: 256 << 20,
        max_sections: 1024,
        max_objects: 8_388_607,
        max_depth: 64,
    };
}

impl Default for ReadLimits {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// How a cross-reference section is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XrefKind {
    Table,
    Stream,
}

impl From<pdf_structure::XrefKind> for XrefKind {
    fn from(kind: pdf_structure::XrefKind) -> XrefKind {
        match kind {
            pdf_structure::XrefKind::Stream => XrefKind::Stream,
            _ => XrefKind::Table,
        }
    }
}

/// What a file's cross-reference sections say, for writing after them. Offsets
/// count from `%PDF-`, at [`XrefFacts::header_offset`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XrefFacts {
    pub kind: XrefKind,
    pub start: usize,
    pub size: u32,
    pub max_stream: Option<u32>,
    pub id: Option<[Vec<u8>; 2]>,
    pub header_offset: usize,
    pub hybrid: bool,
    pub linearized: bool,
    pub sections: usize,
}

/// First-parse verbatim copy candidates from [`LazyPdf::to_document_and_raw`]:
/// outer-file byte spans (`Origin::File`) and decoded object-stream member
/// slices (`Origin::Stream`).
#[derive(Debug, Default)]
pub struct RawCopies {
    pub spans: HashMap<ObjectId, Range<usize>>,
    pub members: HashMap<ObjectId, Vec<u8>>,
}

type Kept = std::result::Result<Arc<Object>, KeptError>;

#[derive(Clone)]
enum KeptError {
    Missing,
    Mismatch,
    IndirectObject(usize),
    Limit(Limit),
    Padding,
    CryptFilter(Vec<u8>),
    Syntax(String),
}

impl KeptError {
    fn to_error(&self) -> Error {
        match self {
            KeptError::Missing => Error::MissingXrefEntry,
            KeptError::Mismatch => Error::ObjectIdMismatch,
            KeptError::IndirectObject(offset) => Error::IndirectObject { offset: *offset },
            KeptError::Limit(limit) => Error::Limit(*limit),
            KeptError::Padding => Error::Decryption(DecryptionError::Padding),
            KeptError::CryptFilter(name) => {
                Error::Decryption(DecryptionError::UndefinedCryptFilter(name.clone()))
            }
            KeptError::Syntax(msg) => Error::Syntax(msg.clone()),
        }
    }
}

/// A PDF read lazily through [`pdf_structure::Reading`].
pub struct LazyPdf {
    reading: Reading<Box<dyn ByteSource>>,
    limits: ReadLimits,
    version: String,
    trailer: Dictionary,
    facts: XrefFacts,
    sound: bool,
    crypt: Option<(EncryptionState, ObjectId)>,
    /// Converted `lopdf::Object` cache per listed object, sorted by `ObjectId`
    /// (removed in slice 11 when callers take `pdf_objects::Object` directly).
    slots: Box<[(ObjectId, OnceLock<Kept>)]>,
}

impl LazyPdf {
    /// Opens `bytes` lazily through [`pdf_structure::Reading`] under `limits`,
    /// authenticating the empty password and then `password` when encrypted.
    pub fn open(
        bytes: impl ByteSource + 'static,
        password: Option<&str>,
        limits: &ReadLimits,
    ) -> Result<LazyPdf> {
        let mut struct_limits = pdf_structure::Limits::DEFAULT;
        struct_limits.sections = limits.max_sections;
        struct_limits.stream = limits.max_decompressed;
        struct_limits.budget = usize::MAX;
        struct_limits.entries = Some(limits.max_objects as usize);

        let pw_pair;
        let passwords: &[Password<'_>] = match password {
            Some(pw) if !pw.is_empty() => {
                pw_pair = [Password(""), Password(pw)];
                &pw_pair
            }
            _ => &[],
        };

        let mut options = Options::DEFAULT;
        options.limits = struct_limits;
        options.flate = Backend::ZlibRs;
        options.passwords = passwords;

        let mut boxed: Box<dyn ByteSource> = Box::new(bytes);
        let blank_ranges = precheck_xref_stream_caps(&*boxed, limits)?;
        if !blank_ranges.is_empty() {
            boxed = Box::new(BlankedSource::new(boxed, blank_ranges));
        }
        let reading = Reading::open(boxed, &options).map_err(|e| match e {
            pdf_structure::Error::Io(io) => Error::IO(io),
            other => Error::Syntax(other.to_string()),
        })?;

        let Some(header_offset) = reading.facts().header_offset else {
            return Err(Error::Parse(lopdf::ParseError::InvalidFileHeader));
        };
        let Some(ver) = reading.header_version() else {
            return Err(Error::Parse(lopdf::ParseError::InvalidFileHeader));
        };
        let version = ver.to_string();

        let open_faults = reading.faults();
        let mut entry_shifted = false;
        for (_, fault) in &open_faults {
            match fault {
                Fault::Sections { .. } => return Err(Error::Limit(Limit::Sections)),
                Fault::Entries { .. } | Fault::StreamEntries { .. } => {
                    return Err(Error::Limit(Limit::Objects));
                }
                Fault::Decode {
                    fault: pdf_filters::Fault::Limit,
                    ..
                } => return Err(Error::Limit(Limit::Decompressed)),
                Fault::Shifted { .. } | Fault::Renumbered { .. } => entry_shifted = true,
                Fault::Moved {
                    moved,
                    scanned,
                    neither,
                    ..
                } if *moved > 0 || *scanned > 0 || *neither > 0 => entry_shifted = true,
                _ => {}
            }
        }

        let unwritten = reading.trailer().unwritten.is_some();
        if unwritten && !reading.trailer().dict().has(b"Root") {
            return Err(Error::Parse(lopdf::ParseError::InvalidXref));
        }

        let sound = !unwritten && !entry_shifted;
        let sf = reading.facts();
        let (start, kind) = match sf.newest {
            Some((pos, k)) if sound => (pos.saturating_sub(header_offset), XrefKind::from(k)),
            Some((_, k)) => (0, XrefKind::from(k)),
            None => (0, XrefKind::Table),
        };

        let trailer = convert_trailer(reading.trailer());
        let size = trailer
            .get(b"Size")
            .and_then(Object::as_i64)
            .ok()
            .and_then(|s| u32::try_from(s).ok())
            .unwrap_or(0);

        let facts = XrefFacts {
            kind,
            start,
            size,
            max_stream: sf.max_stream,
            id: file_id(&trailer),
            header_offset,
            hybrid: sf.hybrid,
            linearized: sf.linearized,
            sections: if sound { sf.sections } else { 0 },
        };

        let slots: Box<[(ObjectId, OnceLock<Kept>)]> = reading
            .ids()
            .map(|id| ((id.number, id.generation), OnceLock::new()))
            .collect();

        let mut pdf = LazyPdf {
            reading,
            limits: *limits,
            version,
            trailer,
            facts,
            sound,
            crypt: None,
            slots,
        };
        pdf.authenticate(password)?;
        Ok(pdf)
    }

    pub fn base_sound(&self) -> bool {
        self.sound
    }

    pub fn xref(&self) -> &XrefFacts {
        &self.facts
    }

    pub fn version(&self) -> &str {
        &self.version
    }

    pub fn trailer(&self) -> &Dictionary {
        &self.trailer
    }

    pub fn is_encrypted(&self) -> bool {
        self.crypt.is_some()
    }

    pub fn encryption(&self) -> Option<(&EncryptionState, ObjectId)> {
        self.crypt.as_ref().map(|(state, at)| (state, *at))
    }

    /// Underlying `pdf_structure::Reading` (for slices 9–12).
    #[allow(dead_code)]
    pub(crate) fn reading(&self) -> &Reading<Box<dyn ByteSource>> {
        &self.reading
    }

    pub fn ids(&self) -> impl Iterator<Item = ObjectId> + '_ {
        self.slots.iter().map(|&(id, _)| id)
    }

    pub fn catalog(&self) -> Result<&Dictionary> {
        self.trailer
            .get(b"Root")
            .and_then(Object::as_reference)
            .and_then(|id| self.get_dictionary(id))
    }

    pub fn get(&self, id: ObjectId) -> Result<&Object> {
        let kept = self.read_kept(id)?;
        Ok(&**kept)
    }

    pub fn object(&self, id: ObjectId) -> Result<Arc<Object>> {
        self.read_kept(id).cloned()
    }

    pub fn object_unkept(&self, id: ObjectId) -> Result<Arc<Object>> {
        let Some(slot) = self.slot(id) else {
            return Err(Error::MissingXrefEntry);
        };
        if let Some(kept) = slot.get() {
            return kept.as_ref().cloned().map_err(KeptError::to_error);
        }
        let oid = PdfId::from(id);
        let (obj, _) = self
            .reading
            .object_unkept(oid)
            .map_err(|e| self.map_reading_error(id, e))?;
        let converted = self.convert_object(id, &obj, false)?;
        Ok(Arc::new(converted))
    }

    pub fn get_object(&self, id: ObjectId) -> Result<&Object> {
        self.dereference(self.get(id)?).map(|(_, obj)| obj)
    }

    pub fn get_dictionary(&self, id: ObjectId) -> Result<&Dictionary> {
        self.get_object(id).and_then(Object::as_dict)
    }

    pub fn dereference<'a>(&'a self, object: &'a Object) -> Result<(Option<ObjectId>, &'a Object)> {
        let mut object = object;
        let mut id = None;
        let mut chain = 0;
        while let Ok(next) = object.as_reference() {
            chain += 1;
            if chain > self.limits.max_depth {
                return Err(Error::Limit(Limit::Depth));
            }
            id = Some(next);
            object = self.get(next)?;
        }
        Ok((id, object))
    }

    pub fn is_read(&self, id: ObjectId) -> bool {
        self.slot(id).is_some_and(|s| s.get().is_some())
            || self.reading.record(PdfId::from(id)).is_some()
    }

    /// Byte span of `id` in the outer file (`None` for object-stream members
    /// or unreadable objects), read without keeping `id` in memory.
    pub fn raw_span(&self, id: ObjectId) -> Option<Range<usize>> {
        let oid = PdfId::from(id);
        if let Some(rec) = self.reading.record(oid) {
            if rec.at.stream().is_some() {
                return None;
            }
            if self.is_encrypted() && self.object_unkept(id).is_err() {
                return None;
            }
            return Some(rec.at.range());
        }
        let (obj, rec) = self.reading.object_unkept(oid).ok()?;
        if rec.at.stream().is_some() {
            return None;
        }
        if self.is_encrypted() && self.convert_object(id, &obj, false).is_err() {
            return None;
        }
        Some(rec.at.range())
    }

    /// [`LazyPdf::raw_span`] of an object a writer may copy verbatim: at its
    /// cross-reference offset (`Origin::File`), ending with `endobj`, and for
    /// streams having a direct `/Length` matching `endstream`.
    pub fn raw_span_whole(&self, id: ObjectId) -> Option<Range<usize>> {
        let oid = PdfId::from(id);
        if let Some(rec) = self.reading.record(oid) {
            let span = whole_span(rec)?;
            if self.is_encrypted() && self.object_unkept(id).is_err() {
                return None;
            }
            return Some(span);
        }
        let (obj, rec) = self.reading.object_unkept(oid).ok()?;
        let span = whole_span(&rec)?;
        if self.is_encrypted() && self.convert_object(id, &obj, false).is_err() {
            return None;
        }
        Some(span)
    }

    /// Materializes every readable in-use object into an in-memory `Document`
    /// through `pdf_structure::Reading::to_document`, matching what `LazyPdf`
    /// reads (`Encrypt` stripped as Rewrite expects).
    pub fn to_document(&self) -> Result<Document> {
        self.to_document_and_raw(false).map(|(doc, _)| doc)
    }

    /// [`LazyPdf::to_document`], plus first-parse verbatim spans and decoded
    /// object-stream member bytes when `want_raw` is `true` (M18 slice 9).
    pub fn to_document_and_raw(&self, want_raw: bool) -> Result<(Document, RawCopies)> {
        let pdoc = self.reading.to_document().map_err(|e| match e {
            pdf_structure::Error::Io(io) => Error::IO(io),
            other => Error::Syntax(other.to_string()),
        })?;
        let encrypt_id = self.crypt.as_ref().map(|(_, at)| *at);
        let mut out = Document::with_version(self.version.clone());
        let mut trailer = self.trailer.clone();
        if self.crypt.is_some() {
            trailer.remove(b"Encrypt");
        }
        out.trailer = trailer;
        let mut raw = RawCopies::default();
        for (oid, obj) in pdoc.objects() {
            let id = (oid.number, oid.generation);
            if Some(id) == encrypt_id {
                continue;
            }
            let mut decrypt_fallback = false;
            let converted = match obj.value() {
                Value::Stream(s) => {
                    let data = match (&s.data, self.reading.encryption()) {
                        (Data::Bytes(b), _) => b.as_slice().to_vec(),
                        (Data::At(span), Encryption::None) => self
                            .reading
                            .source()
                            .read_at(span.range())
                            .map_err(Error::IO)?
                            .into_owned(),
                        _ => match self.reading.data(oid) {
                            Ok(bytes) => bytes.into_owned(),
                            Err(pdf_structure::Error::Decrypt(pdf_crypt::Error {
                                data: Some(plain),
                                fault: pdf_crypt::Fault::Short { .. },
                            })) => plain,
                            // Stream stored in the clear in an encrypted file
                            // (e.g. 0169467's XMP): keep raw bytes so Rewrite
                            // re-encrypts them cleanly, never copying raw.
                            Err(pdf_structure::Error::Decrypt(_)) => match s.data.at() {
                                Some(span) => {
                                    decrypt_fallback = true;
                                    self.reading
                                        .source()
                                        .read_at(span.range())
                                        .map_err(Error::IO)?
                                        .into_owned()
                                }
                                None => continue,
                            },
                            Err(_) => continue,
                        },
                    };
                    let mut dict = convert_dict(&s.dict);
                    dict.set("Length", data.len() as i64);
                    Object::Stream(Stream {
                        dict,
                        content: data,
                        allows_compression: true,
                        start_position: s.data.at().map(|sp| sp.start()),
                    })
                }
                _ => convert_value(obj),
            };
            if want_raw
                && !decrypt_fallback
                && let Some(rec) = pdoc.record(oid)
            {
                if let Some(span) = whole_span(rec) {
                    raw.spans.insert(id, span);
                } else if id.1 == 0
                    && let Origin::Stream { stream, .. } = rec.origin
                {
                    let stm_oid = PdfId::new(stream, 0);
                    if !pdoc.in_the_clear(stm_oid)
                        && let Some(held) = pdoc.held(stm_oid)
                        && let Some(slice) = rec.at.stream_slice_in(stm_oid, held)
                    {
                        raw.members.insert(id, slice.to_vec());
                    }
                }
            }
            out.max_id = out.max_id.max(id.0);
            out.objects.insert(id, converted);
        }
        Ok((out, raw))
    }

    /// Decoded bytes of an object-stream member `(container_id, bytes)`
    /// when `id` was read from an object stream (`Origin::Stream`).
    pub fn member_bytes(&self, id: ObjectId) -> Option<(u32, Vec<u8>)> {
        let oid = PdfId::from(id);
        let rec = match self.reading.record(oid) {
            Some(rec) => rec.clone(),
            None => self.reading.object_unkept(oid).ok()?.1,
        };
        let Origin::Stream { stream, .. } = rec.origin else {
            return None;
        };
        let bytes = self.reading.bytes(rec.at).ok()?.into_owned();
        Some((stream, bytes))
    }

    fn slot(&self, id: ObjectId) -> Option<&OnceLock<Kept>> {
        let idx = self.slots.binary_search_by_key(&id, |&(k, _)| k).ok()?;
        Some(&self.slots[idx].1)
    }

    fn read_kept(&self, id: ObjectId) -> Result<&Arc<Object>> {
        let Some(slot) = self.slot(id) else {
            return Err(Error::MissingXrefEntry);
        };
        if let Some(kept) = slot.get() {
            return kept.as_ref().map_err(KeptError::to_error);
        }
        let oid = PdfId::from(id);
        let outcome = match self.reading.get(oid) {
            Ok(obj) => match self.convert_object(id, obj, true) {
                Ok(converted) => Ok(Arc::new(converted)),
                Err(e) => match cacheable(&e) {
                    Some(ke) => Err(ke),
                    None => return Err(e),
                },
            },
            Err(e) => {
                let mapped = self.map_reading_error(id, e);
                match cacheable(&mapped) {
                    Some(ke) => Err(ke),
                    None => return Err(mapped),
                }
            }
        };
        slot.get_or_init(|| outcome)
            .as_ref()
            .map_err(KeptError::to_error)
    }

    fn convert_object(&self, id: ObjectId, obj: &PdfObj, keep_stream_obj: bool) -> Result<Object> {
        match obj.value() {
            Value::Stream(s) => {
                let oid = PdfId::from(id);
                let data: Vec<u8> = match (&s.data, keep_stream_obj, self.reading.encryption()) {
                    (Data::Bytes(b), _, _) => b.as_slice().to_vec(),
                    (Data::At(span), false, Encryption::None) => self
                        .reading
                        .source()
                        .read_at(span.range())
                        .map_err(Error::IO)?
                        .into_owned(),
                    _ => match self.reading.data(oid) {
                        Ok(bytes) => bytes.into_owned(),
                        // An empty AES stream written as just a 16-byte IV with
                        // no ciphertext block decrypts to empty in qpdf, PDFium
                        // and lopdf (`Fault::Short` with `Some(vec![])`).
                        Err(pdf_structure::Error::Decrypt(pdf_crypt::Error {
                            data: Some(plain),
                            fault: pdf_crypt::Fault::Short { .. },
                        })) => plain,
                        Err(e) => return Err(self.map_reading_error(id, e)),
                    },
                };
                let mut dict = convert_dict(&s.dict);
                let start_pos = s.data.at().map(|sp| sp.start());
                dict.set("Length", data.len() as i64);
                Ok(Object::Stream(Stream {
                    dict,
                    content: data,
                    allows_compression: true,
                    start_position: start_pos,
                }))
            }
            _ => Ok(convert_value(obj)),
        }
    }

    fn map_reading_error(&self, _id: ObjectId, err: pdf_structure::Error) -> Error {
        match err {
            pdf_structure::Error::Io(io) => Error::IO(io),
            pdf_structure::Error::Locked | pdf_structure::Error::Unsupported => {
                Error::InvalidPassword
            }
            pdf_structure::Error::Decrypt(_) | pdf_structure::Error::Encrypted => {
                Error::Decryption(DecryptionError::Padding)
            }
            pdf_structure::Error::Unread(unread) => self.map_unread(unread),
            other => Error::Syntax(other.to_string()),
        }
    }

    fn map_unread(&self, unread: Unread) -> Error {
        match unread {
            Unread::Missing { .. } => Error::MissingXrefEntry,
            Unread::Elsewhere { .. } => Error::ObjectIdMismatch,
            Unread::Undecodable { .. } => Error::InvalidPassword,
            Unread::Container { stream, .. }
            | Unread::ContainerHeader { stream, .. }
            | Unread::NoIndex { stream, .. }
            | Unread::NoData { stream }
            | Unread::Cut { stream } => {
                if self.container_hit_limit(stream) {
                    Error::Limit(Limit::Decompressed)
                } else {
                    Error::Syntax(unread.to_string())
                }
            }
            Unread::Parse(pe) => {
                use pdf_objects::parse::ErrorKind;
                match pe.kind() {
                    ErrorKind::Nested { .. } => Error::Limit(Limit::Depth),
                    _ => Error::IndirectObject { offset: 0 },
                }
            }
            other => Error::Syntax(other.to_string()),
        }
    }

    fn container_hit_limit(&self, stream: u32) -> bool {
        let sid = PdfId::new(stream, 0);
        self.reading.faults().into_iter().any(|(place, fault)| {
            place.object == Some(sid)
                && matches!(
                    fault,
                    Fault::Decode {
                        fault: pdf_filters::Fault::Limit,
                        ..
                    }
                )
        })
    }

    fn authenticate(&mut self, password: Option<&str>) -> Result<()> {
        let encrypt_entry = match self.trailer.get(b"Encrypt") {
            Ok(Object::Null) | Err(_) => return Ok(()),
            Ok(other) => other.clone(),
        };
        let Ok(encrypt_id) = encrypt_entry.as_reference() else {
            return Err(Error::InvalidPassword);
        };
        let mut doc = Document::new();
        doc.trailer = self.trailer.clone();
        if let Ok(enc_obj) = self.get(encrypt_id) {
            doc.objects.insert(encrypt_id, enc_obj.clone());
        } else {
            return Err(Error::InvalidPassword);
        }

        match self.reading.encryption() {
            Encryption::None => Ok(()),
            Encryption::Locked | Encryption::Plain => Err(Error::InvalidPassword),
            Encryption::Unsupported(_) => match decode_lopdf_encryption(&doc, password) {
                Err(e) => Err(e),
                Ok(_) => Err(Error::InvalidPassword),
            },
            Encryption::Opened { by, .. } => {
                let state = decode_lopdf_encryption_opened(&doc, password, *by)?;
                self.crypt = Some((state, encrypt_id));
                Ok(())
            }
            _ => Err(Error::InvalidPassword),
        }
    }
}

fn whole_span(rec: &Record) -> Option<Range<usize>> {
    if rec.origin != Origin::File || !rec.endobj || rec.at.stream().is_some() {
        return None;
    }
    if let Some(d) = rec.data
        && (!d.declared || d.indirect_length)
    {
        return None;
    }
    Some(rec.at.range())
}

/// `ByteSource` wrapper used only when multiple `/Prev` sections point
/// `/XRefStm` at the exact same cross-reference stream object, blanking
/// the redundant `/XRefStm` entries so `Reading::open` decodes the shared
/// stream once.
struct BlankedSource {
    inner: Box<dyn ByteSource>,
    whole: Option<Box<[u8]>>,
    ranges: Vec<Range<usize>>,
}

impl BlankedSource {
    fn new(inner: Box<dyn ByteSource>, ranges: Vec<Range<usize>>) -> Self {
        let whole = inner.whole().map(|bytes| {
            let mut copy = bytes.to_vec();
            for r in &ranges {
                let end = r.end.min(copy.len());
                if r.start < end {
                    copy[r.start..end].fill(b' ');
                }
            }
            copy.into_boxed_slice()
        });
        Self {
            inner,
            whole,
            ranges,
        }
    }
}

impl ByteSource for BlankedSource {
    fn len(&self) -> usize {
        self.inner.len()
    }

    fn whole(&self) -> Option<&[u8]> {
        self.whole.as_deref()
    }

    fn read_at(&self, range: Range<usize>) -> std::io::Result<std::borrow::Cow<'_, [u8]>> {
        if let Some(whole) = &self.whole {
            return Ok(std::borrow::Cow::Borrowed(&whole[range]));
        }
        let mut cow = self.inner.read_at(range.clone())?;
        for r in &self.ranges {
            let start = r.start.max(range.start);
            let end = r.end.min(range.end);
            if start < end {
                cow.to_mut()[start - range.start..end - range.start].fill(b' ');
            }
        }
        Ok(cow)
    }

    fn first_window(&self) -> usize {
        self.inner.first_window()
    }
}

fn check_xref_dict_caps(
    dict: &Dict,
    limits: &ReadLimits,
    total_rows: &mut usize,
    max_entries: usize,
) -> Result<()> {
    match dict.get(b"Index").map(PdfObj::value) {
        Some(Value::Array(arr)) => {
            for pair in arr.as_chunks::<2>().0 {
                let Some(start) = pair[0].as_integer().and_then(|n| u64::try_from(n).ok()) else {
                    break;
                };
                let Some(count) = pair[1].as_integer().and_then(|n| u32::try_from(n).ok()) else {
                    break;
                };
                if count > limits.max_objects || start + u64::from(count) > u64::from(u32::MAX) + 1
                {
                    return Err(Error::Limit(Limit::Objects));
                }
                *total_rows = total_rows.saturating_add(count as usize);
            }
        }
        _ => {
            if let Some(size) = dict.get(b"Size").and_then(PdfObj::as_integer) {
                let Ok(count) = u32::try_from(size) else {
                    return Err(Error::Limit(Limit::Objects));
                };
                if count > limits.max_objects {
                    return Err(Error::Limit(Limit::Objects));
                }
                *total_rows = total_rows.saturating_add(count as usize);
            }
        }
    }
    if *total_rows > max_entries {
        return Err(Error::Limit(Limit::Objects));
    }
    Ok(())
}

fn skip_pdf_space_and_comments(
    source: &dyn ByteSource,
    mut pos: usize,
    len: usize,
) -> Result<usize> {
    let is_ws = |b: u8| matches!(b, 0 | b'\t' | b'\n' | 0x0c | b'\r' | b' ');
    while pos < len {
        let win = match source.whole() {
            Some(bytes) => std::borrow::Cow::Borrowed(&bytes[pos..(pos + 4096).min(len)]),
            None => source
                .read_at(pos..(pos + 4096).min(len))
                .map_err(Error::IO)?,
        };
        let mut i = 0;
        while i < win.len() {
            if is_ws(win[i]) {
                i += 1;
            } else if win[i] == b'%' {
                i += 1;
                while i < win.len() && win[i] != b'\n' && win[i] != b'\r' {
                    i += 1;
                }
                if i == win.len() && pos + i < len {
                    // Comment spans past this window: advance into it; if
                    // we land mid-comment without `%`, find newline first.
                    pos += i;
                    while pos < len {
                        let cwin = match source.whole() {
                            Some(b) => std::borrow::Cow::Borrowed(&b[pos..(pos + 4096).min(len)]),
                            None => source
                                .read_at(pos..(pos + 4096).min(len))
                                .map_err(Error::IO)?,
                        };
                        if let Some(nl) = cwin.iter().position(|&b| b == b'\n' || b == b'\r') {
                            pos += nl + 1;
                            break;
                        }
                        pos += cwin.len();
                    }
                    i = 0;
                    break;
                }
            } else {
                return Ok(pos + i);
            }
        }
        if i == 0 {
            continue;
        }
        pos += i;
    }
    Ok(pos)
}

fn find_classic_trailer_dict_offset(
    source: &dyn ByteSource,
    pos: usize,
    len: usize,
) -> Result<Option<usize>> {
    let is_ws = |b: u8| matches!(b, 0 | b'\t' | b'\n' | 0x0c | b'\r' | b' ');
    let mut cur = skip_pdf_space_and_comments(source, pos, len)?;
    if cur + 4 >= len {
        return Ok(None);
    }
    let head = match source.whole() {
        Some(b) => std::borrow::Cow::Borrowed(&b[cur..(cur + 16).min(len)]),
        None => source
            .read_at(cur..(cur + 16).min(len))
            .map_err(Error::IO)?,
    };
    if !head.starts_with(b"xref") || !head.get(4).is_some_and(|&b| is_ws(b)) {
        return Ok(None);
    }
    cur += 4;
    loop {
        cur = skip_pdf_space_and_comments(source, cur, len)?;
        if cur >= len {
            return Ok(None);
        }
        let win = match source.whole() {
            Some(b) => std::borrow::Cow::Borrowed(&b[cur..(cur + 128).min(len)]),
            None => source
                .read_at(cur..(cur + 128).min(len))
                .map_err(Error::IO)?,
        };
        if win.starts_with(b"trailer")
            && win
                .get(7)
                .is_some_and(|&b| is_ws(b) || b == b'<' || b == b'%')
        {
            return Ok(Some(skip_pdf_space_and_comments(source, cur + 7, len)?));
        }
        let text = std::str::from_utf8(&win).unwrap_or("");
        let mut parts = text.split_ascii_whitespace();
        let Some(_start) = parts.next().and_then(|t| t.parse::<u64>().ok()) else {
            return Ok(None);
        };
        let Some(count) = parts.next().and_then(|t| t.parse::<usize>().ok()) else {
            return Ok(None);
        };
        let Some(line_end) = win.iter().position(|&b| b == b'\n' || b == b'\r') else {
            return Ok(None);
        };
        let mut after_header = cur + line_end + 1;
        if win[line_end] == b'\r' && win.get(line_end + 1) == Some(&b'\n') {
            after_header += 1;
        }
        let Some(rows_bytes) = count.checked_mul(20) else {
            return Ok(None);
        };
        let Some(next_cur) = after_header.checked_add(rows_bytes).filter(|&p| p <= len) else {
            return Ok(None);
        };
        cur = next_cur;
    }
}

/// Refuses cross-reference stream `/Size` and `/Index` bombs from their
/// dictionaries before `Reading::open` inflates any stream data, and
/// returns byte ranges of duplicate `/XRefStm` entries pointing to a
/// stream object already referenced by a newer section.
fn precheck_xref_stream_caps(
    source: &dyn ByteSource,
    limits: &ReadLimits,
) -> Result<Vec<Range<usize>>> {
    let len = source.len();
    if len < 16 {
        return Ok(Vec::new());
    }
    let slice = |range: Range<usize>| -> Result<std::borrow::Cow<'_, [u8]>> {
        match source.whole() {
            Some(bytes) => Ok(std::borrow::Cow::Borrowed(&bytes[range])),
            None => source.read_at(range).map_err(Error::IO),
        }
    };
    let head = slice(0..len.min(1024))?;
    let Some(header_offset) = head.windows(5).position(|w| w == b"%PDF-") else {
        return Ok(Vec::new());
    };
    let tail_start = len.saturating_sub(1024);
    let tail = slice(tail_start..len)?;
    let Some(sx) = tail.windows(9).rposition(|w| w == b"startxref") else {
        return Ok(Vec::new());
    };
    let Some(first_off) = std::str::from_utf8(&tail[sx + 9..])
        .ok()
        .and_then(|s| s.split_ascii_whitespace().next())
        .and_then(|tok| tok.parse::<usize>().ok())
    else {
        return Ok(Vec::new());
    };

    let max_entries = 2usize.saturating_mul(limits.max_objects as usize);
    let mut total_rows = 0usize;
    let mut visited = HashSet::new();
    let mut seen_xref_stms = HashSet::new();
    let mut blank_ranges = Vec::new();
    let mut next = Some(first_off);
    while let Some(off) = next {
        if !visited.insert(off) {
            break;
        }
        if visited.len() > limits.max_sections {
            return Err(Error::Limit(Limit::Sections));
        }
        let Some(pos) = header_offset.checked_add(off).filter(|&p| p < len) else {
            break;
        };
        let win = slice(pos..(pos + 4096).min(len))?;
        let mut parser = Parser::at(&win, 0).window(pos, pos + win.len() == len);
        if parser.header(&mut ()).is_ok() {
            let header_end = parser.offset();
            let Ok(val) = parser.value(&mut ()) else {
                break;
            };
            let Some(dict) = val.as_dict() else {
                break;
            };
            if !dict.has_type(b"XRef") {
                break;
            }
            seen_xref_stms.insert(header_end);
            check_xref_dict_caps(dict, limits, &mut total_rows, max_entries)?;
            next = dict
                .get(b"Prev")
                .and_then(PdfObj::as_integer)
                .and_then(|n| usize::try_from(n).ok());
            continue;
        }

        let Some(t_pos) = find_classic_trailer_dict_offset(source, pos, len)? else {
            break;
        };
        let t_win = slice(t_pos..(t_pos + 4096).min(len))?;
        let mut t_parser = Parser::at(&t_win, 0).window(t_pos, t_pos + t_win.len() == len);
        let Ok(t_val) = t_parser.value(&mut ()) else {
            break;
        };
        let Some(t_dict) = t_val.as_dict() else {
            break;
        };
        if let Some(entry) = t_dict.entry(b"XRefStm")
            && let Some(stm_off) = entry
                .value()
                .as_integer()
                .and_then(|n| usize::try_from(n).ok())
            && let Some(stm_pos) = header_offset.checked_add(stm_off).filter(|&p| p < len)
        {
            let obj_pos = skip_pdf_space_and_comments(source, stm_pos, len)?;
            if obj_pos < len {
                let s_win = slice(obj_pos..(obj_pos + 4096).min(len))?;
                let mut s_parser =
                    Parser::at(&s_win, 0).window(obj_pos, obj_pos + s_win.len() == len);
                if s_parser.header(&mut ()).is_ok() {
                    let stm_header_end = s_parser.offset();
                    if seen_xref_stms.insert(stm_header_end) {
                        if let Ok(s_val) = s_parser.value(&mut ())
                            && let Some(s_dict) = s_val.as_dict()
                            && s_dict.has_type(b"XRef")
                        {
                            check_xref_dict_caps(s_dict, limits, &mut total_rows, max_entries)?;
                        }
                    } else if let (Some(k_span), Some(v_span)) =
                        (entry.key_at(), entry.value().at())
                    {
                        blank_ranges.push(k_span.start()..v_span.end());
                    }
                }
            }
        }
        next = t_dict
            .get(b"Prev")
            .and_then(PdfObj::as_integer)
            .and_then(|n| usize::try_from(n).ok());
    }
    Ok(blank_ranges)
}

fn decode_lopdf_encryption(doc: &Document, password: Option<&str>) -> Result<EncryptionState> {
    if doc.authenticate_password("").is_ok() {
        return EncryptionState::decode(doc, b"");
    }
    if let Some(pw) = password {
        for enc in Password(pw).encodings() {
            if let Ok(state) = EncryptionState::decode(doc, enc.as_bytes()) {
                return Ok(state);
            }
        }
        if doc.authenticate_password(pw).is_ok() {
            return EncryptionState::decode(doc, pw.as_bytes());
        }
    }
    match EncryptionState::decode(doc, password.unwrap_or("").as_bytes()) {
        Err(e @ Error::Decryption(DecryptionError::UndefinedCryptFilter(_))) => Err(e),
        _ => Err(Error::InvalidPassword),
    }
}

fn decode_lopdf_encryption_opened(
    doc: &Document,
    password: Option<&str>,
    by: Option<(usize, pdf_crypt::Encoding)>,
) -> Result<EncryptionState> {
    if let Ok(state) = EncryptionState::decode(doc, b"")
        && (by.is_none() || by.is_some_and(|(idx, _)| idx == 0))
    {
        return Ok(state);
    }
    if let Some(pw) = password {
        for enc in Password(pw).encodings() {
            if let Ok(state) = EncryptionState::decode(doc, enc.as_bytes()) {
                return Ok(state);
            }
        }
        return EncryptionState::decode(doc, pw.as_bytes()).map_err(|e| match e {
            Error::Decryption(DecryptionError::IncorrectPassword) => Error::InvalidPassword,
            other => other,
        });
    }
    EncryptionState::decode(doc, b"").map_err(|e| match e {
        Error::Decryption(DecryptionError::IncorrectPassword) => Error::InvalidPassword,
        other => other,
    })
}

fn cacheable(err: &Error) -> Option<KeptError> {
    Some(match err {
        Error::MissingXrefEntry => KeptError::Missing,
        Error::ObjectIdMismatch => KeptError::Mismatch,
        Error::IndirectObject { offset } => KeptError::IndirectObject(*offset),
        Error::Limit(l) => KeptError::Limit(*l),
        Error::Decryption(DecryptionError::UndefinedCryptFilter(name)) => {
            KeptError::CryptFilter(name.clone())
        }
        Error::Decryption(_) => KeptError::Padding,
        Error::IO(_) => return None,
        other => KeptError::Syntax(other.to_string()),
    })
}

fn convert_trailer(trailer: &pdf_structure::Trailer) -> Dictionary {
    let src = trailer.dict();
    let mut dict = convert_dict(src);
    dict.remove(b"Prev");
    dict.remove(b"XRefStm");
    if trailer.stream.is_some() || src.has_type(b"XRef") {
        dict.remove(b"Length");
        dict.remove(b"W");
        dict.remove(b"Index");
        dict.remove(b"Filter");
        dict.remove(b"DecodeParms");
    }
    dict
}

fn convert_dict(dict: &Dict) -> Dictionary {
    let mut out = Dictionary::new();
    for (key, value) in dict.iter() {
        out.set(key.to_vec(), convert_value(value));
    }
    out
}

fn convert_value(obj: &PdfObj) -> Object {
    match obj.value() {
        Value::Null => Object::Null,
        Value::Bool(b) => Object::Boolean(*b),
        Value::Integer(i) => Object::Integer(*i),
        Value::Real(r) => Object::Real(*r),
        Value::Name(n) => Object::Name(n.as_bytes().to_vec()),
        Value::String(s, form) => {
            let bytes = match s {
                Str::Bytes(b) | Str::Encrypted(b) => b.as_slice().to_vec(),
            };
            let fmt = match form {
                Some(Form::Hex) => StringFormat::Hexadecimal,
                _ => StringFormat::Literal,
            };
            Object::String(bytes, fmt)
        }
        Value::Array(items) => Object::Array(items.iter().map(convert_value).collect()),
        Value::Dict(d) => Object::Dictionary(convert_dict(d)),
        Value::Ref(id) => Object::Reference((id.number, id.generation)),
        Value::Stream(s) => {
            let data = s.data.bytes().map_or_else(Vec::new, <[u8]>::to_vec);
            Object::Stream(Stream::new(convert_dict(&s.dict), data))
        }
    }
}

fn file_id(trailer: &Dictionary) -> Option<[Vec<u8>; 2]> {
    match trailer
        .get(b"ID")
        .and_then(Object::as_array)
        .ok()?
        .as_slice()
    {
        [Object::String(first, _), Object::String(second, _)] => {
            Some([first.clone(), second.clone()])
        }
        _ => None,
    }
}
