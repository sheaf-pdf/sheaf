//! M18 slices 8 and 11–12: `LazyPdf` backed directly by
//! `pdf_structure::Reading` and `pdf_objects::Object`, with zero conversion.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::sync::{Arc, OnceLock};

use pdf_crypt::Password;
use pdf_filters::Backend;
use pdf_objects::object::{Data, Dict, Object, ObjectId, Stream, Value};
use pdf_objects::parse::Parser;
use pdf_structure::read::{Fault, Options};
use pdf_structure::write::{Encryptor, ReadFrom};
use pdf_structure::{ByteSource, Encryption, Origin, Reading, Record, Unread};

use super::{DecryptionError, Document, Error, Limit, Result};

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

/// What [`LazyPdf::to_document_and_raw`] hands over beside the document, of
/// the objects a writer may copy as they were read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Copies {
    /// Nothing: every object is written anew.
    None,
    /// A stable save's: each candidate's bytes at an offset, or as a member
    /// of an object stream, copied at its own number ([`RawCopies::spans`],
    /// [`RawCopies::members`]).
    Stable,
    /// A renumbered write's: each candidate as the family read it, with its
    /// record ([`RawCopies::as_read`]), which its writer copies under a
    /// renumbering from [`LazyPdf::read_from`].
    Renumbered,
}

/// First-parse verbatim copy candidates from [`LazyPdf::to_document_and_raw`].
/// A candidate is read at an offset its sections give, ending with `endobj`,
/// a stream's `/Length` direct and holding (`Origin::File`), or is an object
/// stream's member of generation 0 whose container leaves nothing in the
/// clear (`Origin::Stream`); and isn't a stream whose data read only by
/// falling back on its bytes as written, in an encrypted file. For a stable
/// save, their outer-file byte spans and decoded member slices; for a
/// renumbered write, the family's document as read, holding only them.
#[derive(Debug, Default)]
pub struct RawCopies {
    pub spans: HashMap<ObjectId, Range<usize>>,
    pub members: HashMap<ObjectId, Vec<u8>>,
    /// Under [`Copies::Renumbered`], the candidates as read, each with its
    /// record: what else the reading's document held removed.
    read: Option<pdf_structure::Document>,
}

impl RawCopies {
    /// Candidate `id` as the family read it, and its record: under
    /// [`Copies::Renumbered`] alone.
    #[must_use]
    pub fn as_read(&self, id: ObjectId) -> Option<(&Object, &Record)> {
        let read = self.read.as_ref()?;
        let record = read.record(id)?;
        let object = pdf_structure::Source::get(read, id).ok()?;
        Some((object, record))
    }

    /// Each candidate held as read ([`RawCopies::as_read`]), by number.
    pub fn as_read_ids(&self) -> impl Iterator<Item = ObjectId> + '_ {
        (self.read.iter()).flat_map(|read| read.objects().map(|(id, _)| id))
    }

    /// Keeps of the candidates held as read only those `keep` says.
    pub fn retain_as_read(&mut self, mut keep: impl FnMut(ObjectId) -> bool) {
        let Some(read) = &mut self.read else {
            return;
        };
        let gone: Vec<ObjectId> = (read.objects())
            .map(|(id, _)| id)
            .filter(|&id| !keep(id))
            .collect();
        for id in gone {
            read.remove(id);
        }
    }
}

/// What makes an object a copy candidate ([`RawCopies`]).
enum Candidate<'d> {
    /// Its span in the file, `n g obj` to past `endobj`.
    At(Range<usize>),
    /// Its bytes in its object stream's decoded data.
    Member(&'d [u8]),
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
    /// Leading preamble offset stripped when `%PDF-` sits past the 1,024-byte
    /// window `pdf-structure` checks on open (`0` otherwise).
    extra_header: usize,
    limits: ReadLimits,
    version: String,
    trailer: Dict,
    facts: XrefFacts,
    sound: bool,
    crypt: Option<(Encryptor, ObjectId)>,
    /// Materialized `Arc<Object>` cache per listed object, sorted by `ObjectId`
    /// (streams hold decrypted `Data::Bytes` so callers read `&Stream` directly).
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
        let mut pdf = Self::open_unauthenticated(bytes, password, limits)?;
        pdf.authenticate()?;
        Ok(pdf)
    }

    fn open_unauthenticated(
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
        let extra_header = find_extra_header(&*boxed)?;
        if extra_header > 0 {
            boxed = Box::new(SubSource {
                inner: boxed,
                start: extra_header,
            });
        }
        let blank_ranges = precheck_xref_stream_caps(&*boxed, limits)?;
        if !blank_ranges.is_empty() {
            boxed = Box::new(BlankedSource::new(boxed, blank_ranges));
        }
        let reading = Reading::open(boxed, &options).map_err(|e| match e {
            pdf_structure::Error::Io(io) => Error::IO(io),
            other => Error::Syntax(other.to_string()),
        })?;

        let Some(inner_header_offset) = reading.facts().header_offset else {
            return Err(Error::Syntax("invalid PDF file header".to_string()));
        };
        let header_offset = inner_header_offset + extra_header;
        let Some(ver) = reading.header_version() else {
            return Err(Error::Syntax("invalid PDF file header".to_string()));
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
        let mut trailer = clean_trailer(reading.trailer());
        if unwritten {
            if !trailer.has(b"Root") {
                return Err(Error::Syntax("invalid cross-reference table".to_string()));
            }
            let root_valid = trailer
                .get(b"Root")
                .and_then(Object::as_reference)
                .is_some_and(|id| {
                    reading
                        .get(id)
                        .is_ok_and(|o| matches!(o.value(), Value::Dict(_)))
                });
            if !root_valid {
                let found_root = reading
                    .ids()
                    .filter(|id| {
                        reading
                            .get(*id)
                            .is_ok_and(|o| o.as_dict().is_some_and(|d| d.has_type(b"Catalog")))
                    })
                    .last();
                match found_root {
                    Some(cat_id) => trailer.set("Root", cat_id),
                    None => return Err(Error::Syntax("invalid cross-reference table".to_string())),
                }
            }
        }

        let sound = !unwritten && !entry_shifted;
        let sf = reading.facts();
        let (start, kind) = match sf.newest {
            Some((pos, k)) if sound => (pos.saturating_sub(inner_header_offset), XrefKind::from(k)),
            Some((_, k)) => (0, XrefKind::from(k)),
            None => (0, XrefKind::Table),
        };
        let size = trailer
            .get(b"Size")
            .and_then(Object::as_integer)
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

        let slots: Box<[(ObjectId, OnceLock<Kept>)]> =
            reading.ids().map(|id| (id, OnceLock::new())).collect();

        Ok(LazyPdf {
            reading,
            extra_header,
            limits: *limits,
            version,
            trailer,
            facts,
            sound,
            crypt: None,
            slots,
        })
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

    pub fn trailer(&self) -> &Dict {
        &self.trailer
    }

    pub fn is_encrypted(&self) -> bool {
        self.crypt.is_some()
    }

    pub fn encryption(&self) -> Option<(&Encryptor, ObjectId)> {
        self.crypt.as_ref().map(|(enc, at)| (enc, *at))
    }

    pub fn ids(&self) -> impl Iterator<Item = ObjectId> + '_ {
        self.slots.iter().map(|&(id, _)| id)
    }

    pub fn catalog(&self) -> Result<&Dict> {
        let id = self
            .trailer
            .get(b"Root")
            .and_then(Object::as_reference)
            .ok_or(Error::ObjectType {
                expected: "Catalog reference",
            })?;
        self.get_dictionary(id)
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
        let (obj, _) = self
            .reading
            .object_unkept(id)
            .map_err(|e| self.map_reading_error(id, e))?;
        let loaded = self.materialize_stream(id, obj, false)?;
        Ok(Arc::new(loaded))
    }

    /// Object `id`, its references followed: None where it isn't there
    /// (no entry in the cross-reference table, or none where its entry
    /// points), and why where it's there and doesn't read: a chain of
    /// references past the reader's depth is the reader's cap
    /// ([`Error::ReferenceLimit`]), and one that comes back to an object
    /// of its own within that depth is the file's damage, the same under
    /// any cap ([`Error::ReferenceCycle`]). What's kept is borrowed. A stream that
    /// isn't kept is read as the file has it, its dictionary alone, and
    /// handed over, kept nowhere: its data isn't read, and nothing of it
    /// stays, so asking for a stream's dictionary holds neither its bytes,
    /// as [`LazyPdf::get`] does, nor the dictionary, for the life of the
    /// reader. Its `/Length` is then the file's (a kept stream's is its
    /// data's), and its data is read by [`LazyPdf::object_unkept`]. A
    /// stream whose data doesn't read is handed over the same way, kept as
    /// that error or not. Anything else is read once and kept in the
    /// reader's slot, where `get` finds it; the reading keeps neither it
    /// nor its record, which what copies an object's bytes reads again
    /// when it asks ([`LazyPdf::raw_span`]).
    pub fn peek(&self, id: ObjectId) -> Result<Option<Cow<'_, Object>>> {
        let Some(mut object) = self.peek_one(id)? else {
            return Ok(None);
        };
        let mut chain = 0;
        while let Some(next) = object.as_reference() {
            chain += 1;
            if chain > self.limits.max_depth {
                return Err(self.chain_past_depth(id));
            }
            match self.peek_one(next)? {
                Some(next) => object = next,
                None => return Ok(None),
            }
        }
        Ok(Some(object))
    }

    /// Why the chain of references from object `id` passes the reader's
    /// depth: a loop where it comes back to an object of its own within
    /// that depth, and the cap where it's only long. Asked once the chain
    /// has passed it, so a chain within the depth pays nothing.
    fn chain_past_depth(&self, id: ObjectId) -> Error {
        let mut chain = vec![id];
        while let Ok(Some(object)) = self.peek_one(chain[chain.len() - 1])
            && let Some(next) = object.as_reference()
            && chain.len() <= self.limits.max_depth
        {
            if chain.contains(&next) {
                return Error::ReferenceCycle(next);
            }
            chain.push(next);
        }
        Error::ReferenceLimit
    }

    /// [`LazyPdf::peek`], no reference followed. A stream the reading
    /// keeps as written, read for something else, is lent so; one kept as
    /// the error its data gave ([`LazyPdf::get`] keeps one) is as one not
    /// kept at all, by its dictionary: what a stream is lent as doesn't
    /// turn on whether something asked for its data first. Anything else
    /// read here is kept in its slot, the one place `get` and this look
    /// first, so it's read once: by a read with no record
    /// (`object_unrecorded`), since whether it's a stream is known only
    /// once it's read, and a kept read (`read_kept`) would hash a
    /// stream's data for a record before handing it over unkept. So the
    /// reading holds no record of an object kept here, and no second copy
    /// of it, where it holds both of one `get` read first; what needs the
    /// record reads it then. One that fails to read is read again by
    /// `get`, which keeps the error as it keeps any.
    fn peek_one(&self, id: ObjectId) -> Result<Option<Cow<'_, Object>>> {
        let Some(slot) = self.slot(id) else {
            return Ok(None);
        };
        if let Some(Ok(kept)) = slot.get() {
            return Ok(Some(Cow::Borrowed(&**kept)));
        }
        if self.reading.record(id).is_some()
            && let Ok(kept) = self.reading.get(id)
            && kept.as_stream().is_some()
        {
            return Ok(Some(Cow::Borrowed(kept)));
        }
        match self.reading.object_unrecorded(id) {
            Ok(object) if object.as_stream().is_some() => Ok(Some(Cow::Owned(object))),
            Ok(object) if slot.get().is_none() => {
                let kept = slot.get_or_init(|| Ok(Arc::new(object)));
                let kept = kept.as_ref().map_err(KeptError::to_error)?;
                Ok(Some(Cow::Borrowed(&**kept)))
            }
            _ => match self.get(id) {
                Ok(object) => Ok(Some(Cow::Borrowed(object))),
                Err(Error::MissingXrefEntry | Error::ObjectNotFound(_)) => Ok(None),
                Err(error) => Err(error),
            },
        }
    }

    /// Whether object `id` is kept: read by [`LazyPdf::get`], a stream
    /// with its data.
    #[cfg(test)]
    pub(crate) fn is_kept(&self, id: ObjectId) -> bool {
        self.slot(id).is_some_and(|slot| slot.get().is_some())
    }

    pub fn get_object(&self, id: ObjectId) -> Result<&Object> {
        self.dereference(self.get(id)?).map(|(_, obj)| obj)
    }

    /// Resolves `id` as a plain PDF dictionary (not a stream).
    pub fn get_dictionary(&self, id: ObjectId) -> Result<&Dict> {
        match self.get_object(id)?.value() {
            Value::Dict(d) => Ok(d),
            _ => Err(Error::ObjectType {
                expected: "Dictionary",
            }),
        }
    }

    pub fn dereference<'a>(&'a self, object: &'a Object) -> Result<(Option<ObjectId>, &'a Object)> {
        let mut object = object;
        let mut id = None;
        let mut chain = 0;
        while let Some(next) = object.as_reference() {
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
        self.slot(id).is_some_and(|s| s.get().is_some()) || self.reading.record(id).is_some()
    }

    /// Byte span of `id` in the outer file (`None` for object-stream members
    /// or unreadable objects), read without keeping `id` in memory.
    pub fn raw_span(&self, id: ObjectId) -> Option<Range<usize>> {
        let shift = |r: Range<usize>| (r.start + self.extra_header)..(r.end + self.extra_header);
        if let Some(rec) = self.reading.record(id) {
            if rec.at.stream().is_some() {
                return None;
            }
            if self.is_encrypted() && self.object_unkept(id).is_err() {
                return None;
            }
            return Some(shift(rec.at.range()));
        }
        let (obj, rec) = self.reading.object_unkept(id).ok()?;
        if rec.at.stream().is_some() {
            return None;
        }
        if self.is_encrypted() && self.materialize_stream(id, obj, false).is_err() {
            return None;
        }
        Some(shift(rec.at.range()))
    }

    /// [`LazyPdf::raw_span`] of an object a writer may copy verbatim: at its
    /// cross-reference offset (`Origin::File`), ending with `endobj`, and for
    /// streams having a direct `/Length` matching `endstream`.
    pub fn raw_span_whole(&self, id: ObjectId) -> Option<Range<usize>> {
        let shift = |r: Range<usize>| (r.start + self.extra_header)..(r.end + self.extra_header);
        if let Some(rec) = self.reading.record(id) {
            let span = whole_span(rec)?;
            if self.is_encrypted() && self.object_unkept(id).is_err() {
                return None;
            }
            return Some(shift(span));
        }
        let (obj, rec) = self.reading.object_unkept(id).ok()?;
        let span = whole_span(&rec)?;
        if self.is_encrypted() && self.materialize_stream(id, obj, false).is_err() {
            return None;
        }
        Some(shift(span))
    }

    /// Materializes every readable in-use object into an in-memory `Document`,
    /// matching what `LazyPdf` reads (`Encrypt` stripped as Rewrite expects).
    pub fn to_document(&self) -> Result<Document> {
        let encrypt_id = self.crypt.as_ref().map(|(_, at)| *at);
        let mut out = Document::with_version(self.version.clone());
        out.xref_kind = self.facts.kind;
        out.xref_start = self.facts.start;
        out.decrypted = self.crypt.is_some();
        let mut trailer = self.trailer.clone();
        if self.crypt.is_some() {
            trailer.remove(b"Encrypt");
        }
        out.trailer = trailer;
        for &(id, _) in &self.slots {
            if Some(id) == encrypt_id {
                continue;
            }
            let Ok((obj, _)) = self.reading.object_unkept(id) else {
                continue;
            };
            let Some((loaded, _)) = self.load_doc_object(id, &obj)? else {
                continue;
            };
            out.max_id = out.max_id.max(id.number);
            out.objects.insert(id, loaded);
        }
        Ok(out)
    }

    /// [`LazyPdf::to_document`], plus what `want` asks of the objects a
    /// writer may copy as they were read ([`RawCopies`]): first-parse spans
    /// and decoded member bytes for a stable save (M18 slice 9), or the
    /// family's objects as read, with their records, for a renumbered write
    /// (M18 slice 14). The candidates are the same either way.
    pub fn to_document_and_raw(&self, want: Copies) -> Result<(Document, RawCopies)> {
        if want == Copies::None {
            return Ok((self.to_document()?, RawCopies::default()));
        }
        let shift = |r: Range<usize>| (r.start + self.extra_header)..(r.end + self.extra_header);
        let mut pdoc = self.reading.to_document().map_err(|e| match e {
            pdf_structure::Error::Io(io) => Error::IO(io),
            other => Error::Syntax(other.to_string()),
        })?;
        let encrypt_id = self.crypt.as_ref().map(|(_, at)| *at);
        let mut out = Document::with_version(self.version.clone());
        out.xref_kind = self.facts.kind;
        out.xref_start = self.facts.start;
        out.decrypted = self.crypt.is_some();
        let mut trailer = self.trailer.clone();
        if self.crypt.is_some() {
            trailer.remove(b"Encrypt");
        }
        out.trailer = trailer;
        let mut raw = RawCopies::default();
        // What the renumbered write's document doesn't keep.
        let mut unwanted = Vec::new();
        for (id, obj) in pdoc.objects() {
            if Some(id) == encrypt_id {
                unwanted.push(id);
                continue;
            }
            let Some((loaded, decrypt_fallback)) = self.load_doc_object(id, obj)? else {
                unwanted.push(id);
                continue;
            };
            let candidate = (pdoc.record(id))
                .filter(|_| !decrypt_fallback)
                .and_then(|rec| candidate(&pdoc, id, rec));
            match (want, candidate) {
                (Copies::Stable, Some(Candidate::At(span))) => {
                    raw.spans.insert(id, shift(span));
                }
                (Copies::Stable, Some(Candidate::Member(slice))) => {
                    raw.members.insert(id, slice.to_vec());
                }
                (_, Some(_)) => {}
                (_, None) => unwanted.push(id),
            }
            out.max_id = out.max_id.max(id.number);
            out.objects.insert(id, loaded);
        }
        if want == Copies::Renumbered {
            unwanted.extend(pdoc.unread().map(|(id, _)| id));
            for id in unwanted {
                pdoc.remove(id);
            }
            raw.read = Some(pdoc);
        }
        Ok((out, raw))
    }

    /// The reading the objects [`Copies::Renumbered`] hands over were read
    /// in, which their writer copies them from ([`RawCopies::as_read`]).
    #[must_use]
    pub fn read_from(&self) -> &dyn ReadFrom {
        &self.reading
    }

    fn load_doc_object(&self, id: ObjectId, obj: &Object) -> Result<Option<(Object, bool)>> {
        let mut decrypt_fallback = false;
        let loaded = match obj.value() {
            Value::Stream(s) => {
                let data = match (&s.data, self.reading.encryption()) {
                    (Data::Bytes(b), _) => b.as_slice().to_vec(),
                    (Data::At(span), Encryption::None) => self
                        .reading
                        .source()
                        .read_at(span.range())
                        .map_err(Error::IO)?
                        .into_owned(),
                    _ => match self.reading.data(id) {
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
                            None => return Ok(None),
                        },
                        Err(_) => return Ok(None),
                    },
                };
                let mut dict = s.dict.clone();
                dict.set("Length", data.len() as i64);
                Object::from(Stream::new(dict, data))
            }
            _ => obj.clone(),
        };
        Ok(Some((loaded, decrypt_fallback)))
    }

    /// Decoded bytes of an object-stream member `(container_id, bytes)`
    /// when `id` was read from an object stream (`Origin::Stream`).
    pub fn member_bytes(&self, id: ObjectId) -> Option<(u32, Vec<u8>)> {
        let rec = match self.reading.record(id) {
            Some(rec) => rec.clone(),
            None => self.reading.object_unkept(id).ok()?.1,
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
        let outcome = match self.reading.get(id) {
            Ok(obj) => match self.materialize_stream(id, obj.clone(), true) {
                Ok(loaded) => Ok(Arc::new(loaded)),
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

    fn materialize_stream(
        &self,
        id: ObjectId,
        obj: Object,
        keep_stream_obj: bool,
    ) -> Result<Object> {
        let Value::Stream(s) = obj.value() else {
            return Ok(obj);
        };
        let data: Vec<u8> = match (&s.data, keep_stream_obj, self.reading.encryption()) {
            (Data::Bytes(b), _, _) => b.as_slice().to_vec(),
            (Data::At(span), false, Encryption::None) => self
                .reading
                .source()
                .read_at(span.range())
                .map_err(Error::IO)?
                .into_owned(),
            _ => match self.reading.data(id) {
                Ok(bytes) => bytes.into_owned(),
                // An empty AES stream written as just a 16-byte IV with
                // no ciphertext block decrypts to empty in qpdf and PDFium
                // (`Fault::Short` with `Some(vec![])`).
                Err(pdf_structure::Error::Decrypt(pdf_crypt::Error {
                    data: Some(plain),
                    fault: pdf_crypt::Fault::Short { .. },
                })) => plain,
                Err(e) => return Err(self.map_reading_error(id, e)),
            },
        };
        let mut dict = s.dict.clone();
        dict.set("Length", data.len() as i64);
        Ok(Object::from(Stream::new(dict, data)))
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
        let sid = ObjectId::new(stream, 0);
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

    fn authenticate(&mut self) -> Result<()> {
        let encrypt_entry = match self.trailer.get(b"Encrypt") {
            None => return Ok(()),
            Some(o) if o.is_null() => return Ok(()),
            Some(other) => other,
        };
        let Some(encrypt_id) = encrypt_entry.as_reference() else {
            return Err(Error::InvalidPassword);
        };

        match self.reading.encryption() {
            Encryption::None => Ok(()),
            Encryption::Locked | Encryption::Plain | Encryption::Unsupported(_) => {
                Err(Error::InvalidPassword)
            }
            Encryption::Opened {
                strings,
                streams,
                files,
                ..
            } => {
                for method in [strings.as_ref(), streams.as_ref(), files.as_ref()]
                    .into_iter()
                    .flatten()
                {
                    if let pdf_crypt::Method::Unknown(name) = method {
                        return Err(Error::Decryption(DecryptionError::UndefinedCryptFilter(
                            name.clone(),
                        )));
                    }
                }
                let Some(encryptor) =
                    Encryptor::of(&self.reading).map_err(|_| Error::InvalidPassword)?
                else {
                    return Ok(());
                };
                self.crypt = Some((encryptor.reusing_rc4_keystreams(), encrypt_id));
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

/// Whether object `id` of `pdoc`, read as `rec` says, is a copy candidate
/// ([`RawCopies`]), and its bytes if it is: its span, by [`whole_span`]; or,
/// of generation 0, its bytes in an object stream that leaves nothing in
/// the clear.
fn candidate<'d>(
    pdoc: &'d pdf_structure::Document,
    id: ObjectId,
    rec: &Record,
) -> Option<Candidate<'d>> {
    if let Some(span) = whole_span(rec) {
        return Some(Candidate::At(span));
    }
    let Origin::Stream { stream, .. } = rec.origin else {
        return None;
    };
    let stm_id = ObjectId::new(stream, 0);
    if id.generation != 0 || pdoc.in_the_clear(stm_id) {
        return None;
    }
    let held = pdoc.held(stm_id)?;
    rec.at.stream_slice_in(stm_id, held).map(Candidate::Member)
}

/// `ByteSource` wrapper stripping leading preamble bytes when `%PDF-` sits past
/// byte 1,024, so `pdf_structure::Reading` sees `%PDF-` at offset 0.
struct SubSource {
    inner: Box<dyn ByteSource>,
    start: usize,
}

impl ByteSource for SubSource {
    fn len(&self) -> usize {
        self.inner.len() - self.start
    }

    fn whole(&self) -> Option<&[u8]> {
        self.inner.whole().map(|b| &b[self.start..])
    }

    fn read_at(&self, range: Range<usize>) -> std::io::Result<std::borrow::Cow<'_, [u8]>> {
        self.inner
            .read_at((range.start + self.start)..(range.end + self.start))
    }

    fn first_window(&self) -> usize {
        self.inner.first_window()
    }
}

/// Returns `offset >= 1024` if `%PDF-` is absent from the first 1,024 bytes of
/// `source` and appears later in the stream (`0` otherwise).
fn find_extra_header(source: &dyn ByteSource) -> Result<usize> {
    let len = source.len();
    if len < 5 {
        return Ok(0);
    }
    let first = match source.whole() {
        Some(b) => std::borrow::Cow::Borrowed(&b[..len.min(1024)]),
        None => source.read_at(0..len.min(1024)).map_err(Error::IO)?,
    };
    if first.windows(5).any(|w| w == b"%PDF-") {
        return Ok(0);
    }
    if len <= 1024 {
        return Ok(0);
    }
    if let Some(whole) = source.whole() {
        return Ok(whole.windows(5).position(|w| w == b"%PDF-").unwrap_or(0));
    }
    let mut window = source.first_window().max(4096);
    loop {
        let end = window.min(len);
        let chunk = source.read_at(0..end).map_err(Error::IO)?;
        if let Some(pos) = chunk.windows(5).position(|w| w == b"%PDF-") {
            return Ok(pos);
        }
        if end == len {
            return Ok(0);
        }
        window = window.saturating_mul(2);
    }
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
    match dict.get(b"Index").map(Object::value) {
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
            if let Some(size) = dict.get(b"Size").and_then(Object::as_integer) {
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
                .and_then(Object::as_integer)
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
            let mut obj_pos = skip_pdf_space_and_comments(source, stm_pos, len)?;
            while obj_pos + 1 < len {
                let zwin = slice(obj_pos..(obj_pos + 256).min(len))?;
                let mut skip = 0;
                while skip + 1 < zwin.len() && zwin[skip] == b'0' && zwin[skip + 1].is_ascii_digit()
                {
                    skip += 1;
                }
                obj_pos += skip;
                if skip == 0 || skip + 1 < zwin.len() {
                    break;
                }
            }
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
            .and_then(Object::as_integer)
            .and_then(|n| usize::try_from(n).ok());
    }
    Ok(blank_ranges)
}

pub(super) fn load_document(
    bytes: impl ByteSource + 'static,
    password: Option<&str>,
    limits: &ReadLimits,
) -> Result<Document> {
    let mut pdf = LazyPdf::open_unauthenticated(bytes, password, limits)?;
    if password.is_none()
        && matches!(
            pdf.reading.encryption(),
            Encryption::Locked | Encryption::Plain
        )
    {
        return pdf.to_locked_document();
    }
    pdf.authenticate()?;
    pdf.to_document()
}

impl LazyPdf {
    fn to_locked_document(&self) -> Result<Document> {
        let pdoc = self.reading.to_document().map_err(|e| match e {
            pdf_structure::Error::Io(io) => Error::IO(io),
            other => Error::Syntax(other.to_string()),
        })?;
        let mut out = Document::with_version(self.version.clone());
        out.xref_kind = self.facts.kind;
        out.xref_start = self.facts.start;
        out.trailer = self.trailer.clone();
        for (id, obj) in pdoc.objects() {
            let loaded = match obj.value() {
                Value::Stream(s) => {
                    let data = match &s.data {
                        Data::Bytes(b) => b.as_slice().to_vec(),
                        Data::At(span) => self
                            .reading
                            .source()
                            .read_at(span.range())
                            .map_err(Error::IO)?
                            .into_owned(),
                        _ => Vec::new(),
                    };
                    let mut dict = s.dict.clone();
                    dict.set("Length", data.len() as i64);
                    Object::from(Stream::new(dict, data))
                }
                _ => obj.clone(),
            };
            out.max_id = out.max_id.max(id.number);
            out.objects.insert(id, loaded);
        }
        Ok(out)
    }
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

fn clean_trailer(trailer: &pdf_structure::Trailer) -> Dict {
    let src = trailer.dict();
    let mut dict = src.clone();
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

fn file_id(trailer: &Dict) -> Option<[Vec<u8>; 2]> {
    match trailer.get(b"ID").and_then(Object::as_array)? {
        [first, second] => Some([first.as_bytes()?.to_vec(), second.as_bytes()?.to_vec()]),
        _ => None,
    }
}
