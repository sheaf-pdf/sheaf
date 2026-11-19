//! In-memory PDF document owned by `sheaf_doc::pdf` (M18 slice 10).
//!
//! Replaces `lopdf::Document` across `sheaf` with the exact surface `sheaf`
//! uses: loaded whole through [`super::LazyPdf`] (`pdf-structure`) for the
//! Rewrite graph, stamp libraries, form checks and test assertions, and built
//! in memory for PDFs `sheaf` synthesizes (`serialize/built.rs`, `annot.rs`,
//! fixtures, and CLI helpers), written through `crate::serialize`.

use std::collections::{BTreeMap, HashSet};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

use crate::serialize::xref::{Entry, Section};
use crate::serialize::{full, object, objstm};

use super::{
    Dictionary, EncryptionState, Error, FileBytes, Object, ObjectId, Permissions, ReadLimits,
    Result, XrefKind, encryption,
};

const DEREF_LIMIT: usize = 128;
const PAGE_TREE_DEPTH_LIMIT: usize = 64;

/// Trailer keys belonging to a cross-reference section rather than the logical
/// trailer, stripped when writing a fresh section from an in-memory `Document`.
const SECTION_TRAILER_KEYS: [&[u8]; 12] = [
    b"Prev",
    b"XRefStm",
    b"Type",
    b"Size",
    b"W",
    b"Index",
    b"Length",
    b"Filter",
    b"DecodeParms",
    b"F",
    b"FFilter",
    b"FDecodeParms",
];

/// An owned, in-memory PDF object graph.
#[derive(Debug, Clone)]
pub struct Document {
    /// Header PDF version (e.g. `"1.4"`, `"1.7"`).
    pub version: String,
    /// Trailer dictionary (`/Root`, `/Info`, `/ID`, and `/Encrypt` when held un-decrypted).
    pub trailer: Dictionary,
    /// Indirect objects keyed by `(number, generation)`.
    pub objects: BTreeMap<ObjectId, Object>,
    /// Highest object number allocated or loaded so far.
    pub max_id: u32,
    /// Cross-reference section format used by [`Document::save`] / [`Document::save_to`].
    pub xref_kind: XrefKind,
    /// Byte offset of the cross-reference section (`0` when built or recovered by scanning).
    pub xref_start: usize,
    /// Whether this document was decrypted from an encrypted source on load.
    pub(crate) decrypted: bool,
}

impl Default for Document {
    fn default() -> Self {
        Self::new()
    }
}

impl Document {
    /// Creates an empty PDF 1.4 document.
    #[must_use]
    pub fn new() -> Self {
        Self {
            version: "1.4".to_string(),
            trailer: Dictionary::new(),
            objects: BTreeMap::new(),
            max_id: 0,
            xref_kind: XrefKind::Stream,
            xref_start: 0,
            decrypted: false,
        }
    }

    /// Creates an empty document with header `version`.
    #[must_use]
    pub fn with_version(version: impl Into<String>) -> Self {
        let mut doc = Self::new();
        doc.version = version.into();
        doc
    }

    /// Loads `path` into memory through `pdf-structure`. Files unlocked by the
    /// empty user password are decrypted; files requiring a password retain
    /// `/Encrypt` in `trailer` (`is_encrypted() == true`).
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let file = File::open(path.as_ref()).map_err(Error::IO)?;
        let bytes = FileBytes::new(file).map_err(Error::IO)?;
        super::reader::load_document(bytes, None, &ReadLimits::DEFAULT)
    }

    /// Loads `path` into memory through `pdf-structure`, authenticating with
    /// the empty user password or `password`.
    pub fn load_with_password(path: impl AsRef<Path>, password: &str) -> Result<Self> {
        let file = File::open(path.as_ref()).map_err(Error::IO)?;
        let bytes = FileBytes::new(file).map_err(Error::IO)?;
        super::reader::load_document(bytes, Some(password), &ReadLimits::DEFAULT)
    }

    /// Loads `bytes` into memory through `pdf-structure`.
    pub fn load_mem(bytes: &[u8]) -> Result<Self> {
        let arc: std::sync::Arc<[u8]> = bytes.into();
        super::reader::load_document(arc, None, &ReadLimits::DEFAULT)
    }

    /// Loads `bytes` into memory through `pdf-structure`, authenticating with
    /// the empty user password or `password`.
    pub fn load_mem_with_password(bytes: &[u8], password: &str) -> Result<Self> {
        let arc: std::sync::Arc<[u8]> = bytes.into();
        super::reader::load_document(arc, Some(password), &ReadLimits::DEFAULT)
    }

    /// Whether this document was decrypted when loaded.
    #[must_use]
    pub fn was_decrypted(&self) -> bool {
        self.decrypted
    }

    /// Allocates the next unused `(id, 0)` object identifier.
    pub fn new_object_id(&mut self) -> ObjectId {
        self.max_id += 1;
        (self.max_id, 0)
    }

    /// Inserts `object` at a freshly allocated [`ObjectId`] and returns it.
    pub fn add_object(&mut self, object: impl Into<Object>) -> ObjectId {
        let id = self.new_object_id();
        self.objects.insert(id, object.into());
        id
    }

    /// Follows indirect references from `object`, returning `(last_ref_id, target)`.
    pub fn dereference<'a>(
        &'a self,
        mut object: &'a Object,
    ) -> Result<(Option<ObjectId>, &'a Object)> {
        let mut nb_deref = 0;
        let mut id = None;
        while let Ok(ref_id) = object.as_reference() {
            id = Some(ref_id);
            object = self
                .objects
                .get(&ref_id)
                .ok_or(Error::ObjectNotFound(ref_id))?;
            nb_deref += 1;
            if nb_deref > DEREF_LIMIT {
                return Err(Error::ReferenceLimit);
            }
        }
        Ok((id, object))
    }

    /// Looks up `id`, following indirect references.
    pub fn get_object(&self, id: ObjectId) -> Result<&Object> {
        let object = self.objects.get(&id).ok_or(Error::ObjectNotFound(id))?;
        self.dereference(object).map(|(_, obj)| obj)
    }

    /// Looks up `id` mutably, following indirect references.
    pub fn get_object_mut(&mut self, mut id: ObjectId) -> Result<&mut Object> {
        let mut nb_deref = 0;
        loop {
            match self.objects.get(&id) {
                Some(Object::Reference(next)) => {
                    id = *next;
                    nb_deref += 1;
                    if nb_deref > DEREF_LIMIT {
                        return Err(Error::ReferenceLimit);
                    }
                }
                Some(_) => break,
                None => return Err(Error::ObjectNotFound(id)),
            }
        }
        self.objects.get_mut(&id).ok_or(Error::ObjectNotFound(id))
    }

    /// Looks up `id` as a dictionary.
    pub fn get_dictionary(&self, id: ObjectId) -> Result<&Dictionary> {
        self.get_object(id).and_then(Object::as_dict)
    }

    /// Looks up `id` mutably as a dictionary.
    pub fn get_dictionary_mut(&mut self, id: ObjectId) -> Result<&mut Dictionary> {
        self.get_object_mut(id).and_then(Object::as_dict_mut)
    }

    /// Returns the document `/Root` catalog dictionary.
    pub fn catalog(&self) -> Result<&Dictionary> {
        self.trailer
            .get(b"Root")
            .and_then(Object::as_reference)
            .and_then(|id| self.get_dictionary(id))
    }

    /// Returns a mutable reference to the document `/Root` catalog dictionary.
    pub fn catalog_mut(&mut self) -> Result<&mut Dictionary> {
        let id = self.trailer.get(b"Root").and_then(Object::as_reference)?;
        self.get_dictionary_mut(id)
    }

    /// Iterates leaf page [`ObjectId`]s in page-tree order.
    pub fn page_iter(&self) -> impl Iterator<Item = ObjectId> + '_ {
        PageTreeIter::new(self)
    }

    /// Returns 1-based page numbers mapped to leaf page [`ObjectId`]s.
    #[must_use]
    pub fn get_pages(&self) -> BTreeMap<u32, ObjectId> {
        self.page_iter()
            .enumerate()
            .map(|(i, p)| ((i + 1) as u32, p))
            .collect()
    }

    /// Resolves `page_id`'s effective `/Resources` dictionary (inheriting up `/Parent`).
    #[must_use]
    pub fn resources_of_page(&self, page_id: ObjectId) -> Option<&Dictionary> {
        let mut cur = Some(page_id);
        let mut visited = HashSet::new();
        while let Some(id) = cur {
            if !visited.insert(id) || visited.len() > PAGE_TREE_DEPTH_LIMIT {
                break;
            }
            let dict = self.get_dictionary(id).ok()?;
            if let Ok(res) = dict.get(b"Resources") {
                return self.dereference(res).ok()?.1.as_dict().ok();
            }
            cur = dict.get(b"Parent").and_then(Object::as_reference).ok();
        }
        None
    }

    /// Resolves annotation dictionaries from `page_id`'s `/Annots` array.
    pub fn get_page_annotations(&self, page_id: ObjectId) -> Result<Vec<&Dictionary>> {
        let Ok(page) = self.get_dictionary(page_id) else {
            return Ok(Vec::new());
        };
        let entries: &[Object] = match page.get(b"Annots") {
            Ok(Object::Reference(id)) => self.get_object(*id).and_then(Object::as_array)?,
            Ok(Object::Array(entries)) => entries,
            _ => return Ok(Vec::new()),
        };
        let mut out = Vec::with_capacity(entries.len());
        for entry in entries {
            match entry {
                Object::Reference(id) => {
                    if let Ok(dict) = self.get_dictionary(*id) {
                        out.push(dict);
                    }
                }
                Object::Dictionary(dict) => out.push(dict),
                _ => {}
            }
        }
        Ok(out)
    }

    /// Concatenates decoded content streams of `page_id` separated by `\n`,
    /// falling back to raw stream bytes if a filter fails.
    #[must_use]
    pub fn get_page_content(&self, page_id: ObjectId) -> Vec<u8> {
        let Ok(page) = self.get_dictionary(page_id) else {
            return Vec::new();
        };
        let Ok(contents) = page.get(b"Contents") else {
            return Vec::new();
        };
        let mut streams = Vec::new();
        let mut cur = contents;
        let mut deref = 0;
        loop {
            match cur {
                Object::Reference(id) => match self.objects.get(id) {
                    None | Some(Object::Stream(_)) => {
                        streams.push(*id);
                        break;
                    }
                    Some(other) => {
                        deref += 1;
                        if deref > DEREF_LIMIT {
                            break;
                        }
                        cur = other;
                    }
                },
                Object::Array(arr) => {
                    for item in arr {
                        if let Ok((_, Object::Stream(_))) = self.dereference(item)
                            && let Ok(id) = item.as_reference()
                        {
                            streams.push(id);
                        } else if let Ok(id) = item.as_reference() {
                            streams.push(id);
                        }
                    }
                    break;
                }
                _ => break,
            }
        }
        let mut out = Vec::new();
        for (i, id) in streams.into_iter().enumerate() {
            let Ok(Object::Stream(stream)) = self.get_object(id) else {
                continue;
            };
            if i > 0 && !out.is_empty() {
                out.push(b'\n');
            }
            match stream.decompressed_content() {
                Ok(decoded) => out.extend_from_slice(&decoded),
                Err(_) => out.extend_from_slice(&stream.content),
            }
        }
        out
    }

    /// Removes all objects unreachable from `self.trailer`, returning their IDs.
    pub fn prune_objects(&mut self) -> Vec<ObjectId> {
        let mut reachable = HashSet::new();
        let mut stack = Vec::new();
        collect_refs_dict(&self.trailer, &mut stack);
        while let Some(id) = stack.pop() {
            if !reachable.insert(id) {
                continue;
            }
            if let Some(obj) = self.objects.get(&id) {
                collect_refs_obj(obj, &mut stack);
            }
        }
        let removed: Vec<ObjectId> = self
            .objects
            .keys()
            .copied()
            .filter(|id| !reachable.contains(id))
            .collect();
        for id in &removed {
            self.objects.remove(id);
        }
        removed
    }

    /// Whether `self.trailer` carries an `/Encrypt` entry.
    #[must_use]
    pub fn is_encrypted(&self) -> bool {
        self.trailer.has(b"Encrypt")
    }

    /// Encrypts all objects in `self` in place under `state` and records the
    /// `/Encrypt` dictionary in `self.objects` and `self.trailer`.
    pub fn encrypt(&mut self, state: &EncryptionState) -> Result<()> {
        if self.is_encrypted() {
            return Err(Error::AlreadyEncrypted);
        }
        let unencrypted_metadata = (!state.encrypt_metadata())
            .then(|| {
                self.catalog()
                    .ok()?
                    .get(b"Metadata")
                    .and_then(Object::as_reference)
                    .ok()
            })
            .flatten();
        let encrypted_dict = state.encode()?;
        for (&id, obj) in &mut self.objects {
            if Some(id) == unencrypted_metadata {
                continue;
            }
            let is_xref = matches!(
                obj,
                Object::Stream(s) if s.dict.get(b"Type").and_then(Object::as_name).ok() == Some(b"XRef")
            );
            if is_xref {
                continue;
            }
            let saved_type =
                if !state.encrypt_metadata() && obj.type_name().ok() == Some(b"Metadata") {
                    match obj {
                        Object::Stream(s) => s.dict.remove(b"Type"),
                        Object::Dictionary(d) => d.remove(b"Type"),
                        _ => None,
                    }
                } else {
                    None
                };
            encryption::encrypt_object(state, id, obj)?;
            if let Some(t) = saved_type {
                match obj {
                    Object::Stream(s) => s.dict.set("Type", t),
                    Object::Dictionary(d) => d.set("Type", t),
                    _ => {}
                }
            }
        }
        let encrypt_id = self.add_object(encrypted_dict);
        self.trailer.set("Encrypt", encrypt_id);
        Ok(())
    }

    /// Writes `self` to `path`.
    pub fn save(&mut self, path: impl AsRef<Path>) -> std::io::Result<File> {
        let mut writer = BufWriter::new(File::create(path)?);
        self.save_to(&mut writer)?;
        writer
            .into_inner()
            .map_err(std::io::IntoInnerError::into_error)
    }

    /// Writes `self` to `target` using `self.xref_kind` without object streams.
    pub fn save_to(&mut self, target: &mut impl Write) -> std::io::Result<()> {
        let bytes = self
            .write_bytes(self.xref_kind, None)
            .map_err(std::io::Error::other)?;
        target.write_all(&bytes)
    }

    /// Writes `self` to `target` with object streams (up to 100 objects per
    /// stream) and a cross-reference stream (falling back to plain objects if
    /// already encrypted in place).
    pub fn save_modern(&mut self, target: &mut impl Write) -> std::io::Result<()> {
        self.save_packed(target, 100)
    }

    /// Writes `self` to `target` with object streams of at most
    /// `max_per_stream` objects and a cross-reference stream.
    pub fn save_packed(
        &mut self,
        target: &mut impl Write,
        max_per_stream: usize,
    ) -> std::io::Result<()> {
        if self.version.as_str() < "1.5" {
            self.version = "1.5".to_string();
        }
        self.xref_kind = XrefKind::Stream;
        let pack = (!self.is_encrypted()).then_some(max_per_stream.max(1));
        let bytes = self
            .write_bytes(XrefKind::Stream, pack)
            .map_err(std::io::Error::other)?;
        target.write_all(&bytes)
    }

    fn write_bytes(&self, kind: XrefKind, pack: Option<usize>) -> anyhow::Result<Vec<u8>> {
        let mut out = Vec::new();
        out.extend_from_slice(format!("%PDF-{}\n", self.version).as_bytes());
        out.extend_from_slice(full::BINARY);

        let mut next = self.max_id.saturating_add(1).max(1);
        let mut section = Section::full(next);
        let mut packable_members: Vec<(u32, Vec<u8>)> = Vec::new();

        for (&id, obj) in &self.objects {
            if full::left_out(obj) {
                continue;
            }
            next = next.max(id.0.saturating_add(1));
            if pack.is_some() && objstm::packable(id, obj) {
                let mut member_bytes = Vec::new();
                anyhow::Context::with_context(object::write(&mut member_bytes, obj), || {
                    format!("writing object {} {}", id.0, id.1)
                })?;
                packable_members.push((id.0, member_bytes));
            } else {
                let pos = object::indirect(&mut out, id, obj)?;
                section.insert(
                    id.0,
                    Entry::InUse {
                        pos,
                        generation: id.1,
                    },
                );
            }
        }

        if let Some(per) = pack {
            for chunk in packable_members.chunks(per) {
                if let Some(stream) = objstm::container(chunk) {
                    let container_id = next;
                    next += 1;
                    let pos =
                        object::indirect(&mut out, (container_id, 0), &Object::Stream(stream))?;
                    section.insert(container_id, Entry::InUse { pos, generation: 0 });
                    for (index, &(member_num, _)) in chunk.iter().enumerate() {
                        section.insert(
                            member_num,
                            Entry::Packed {
                                container: container_id,
                                index: index as u16,
                            },
                        );
                    }
                } else {
                    for (member_num, member_bytes) in chunk {
                        let pos = out.len();
                        out.extend_from_slice(format!("{member_num} 0 obj\n").as_bytes());
                        out.extend_from_slice(member_bytes);
                        out.extend_from_slice(b"\nendobj\n");
                        section.insert(*member_num, Entry::InUse { pos, generation: 0 });
                    }
                }
            }
        }

        let mut trailer = self.trailer.clone();
        for key in SECTION_TRAILER_KEYS {
            trailer.remove(key);
        }

        let pos = out.len();
        match kind {
            XrefKind::Table => {
                section.table(&mut out, trailer, pos)?;
            }
            XrefKind::Stream => {
                section.stream(&mut out, next, trailer, pos)?;
            }
        }
        Ok(out)
    }
}

fn collect_refs_obj(obj: &Object, stack: &mut Vec<ObjectId>) {
    let mut work = vec![obj];
    while let Some(cur) = work.pop() {
        match cur {
            Object::Reference(id) => stack.push(*id),
            Object::Array(items) => work.extend(items.iter()),
            Object::Dictionary(dict) => work.extend(dict.iter().map(|(_, v)| v)),
            Object::Stream(stream) => work.extend(stream.dict.iter().map(|(_, v)| v)),
            _ => {}
        }
    }
}

fn collect_refs_dict(dict: &Dictionary, stack: &mut Vec<ObjectId>) {
    for (_, v) in dict.iter() {
        collect_refs_obj(v, stack);
    }
}

struct PageTreeIter<'a> {
    doc: &'a Document,
    stack: Vec<&'a [Object]>,
    kids: Option<&'a [Object]>,
    iter_limit: usize,
}

impl<'a> PageTreeIter<'a> {
    fn new(doc: &'a Document) -> Self {
        let root_kids = doc
            .catalog()
            .and_then(|cat| cat.get(b"Pages"))
            .and_then(Object::as_reference)
            .ok()
            .and_then(|pages_id| Self::kids(doc, pages_id));
        Self {
            doc,
            stack: Vec::with_capacity(16),
            kids: root_kids,
            iter_limit: doc.objects.len(),
        }
    }

    fn kids(doc: &'a Document, pages_id: ObjectId) -> Option<&'a [Object]> {
        let pages = doc.get_dictionary(pages_id).ok()?;
        let kids = pages.get(b"Kids").ok()?;
        doc.dereference(kids)
            .ok()?
            .1
            .as_array()
            .ok()
            .map(Vec::as_slice)
    }
}

impl Iterator for PageTreeIter<'_> {
    type Item = ObjectId;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            while let Some((kid, rest)) = self.kids.and_then(<[Object]>::split_first) {
                if self.iter_limit == 0 {
                    return None;
                }
                self.iter_limit -= 1;
                self.kids = Some(rest);
                if let Ok(kid_id) = kid.as_reference() {
                    match self
                        .doc
                        .get_dictionary(kid_id)
                        .and_then(Dictionary::get_type)
                    {
                        Ok(b"Page") => return Some(kid_id),
                        Ok(b"Pages") if self.stack.len() < PAGE_TREE_DEPTH_LIMIT => {
                            if !rest.is_empty() {
                                self.stack.push(rest);
                            }
                            self.kids = Self::kids(self.doc, kid_id);
                        }
                        Ok(_) => {}
                        Err(_) => return Some(kid_id),
                    }
                }
            }
            self.kids = Some(self.stack.pop()?);
        }
    }
}

impl std::iter::FusedIterator for PageTreeIter<'_> {}

/// Encryption parameters over [`Document`], bridged to `EncryptionState` until
/// slice 12 replaces `lopdf::encryption` with `pdf_crypt`.
#[derive(Clone, Debug)]
pub enum EncryptionVersion<'a> {
    V1 {
        document: &'a Document,
        owner_password: &'a str,
        user_password: &'a str,
        permissions: Permissions,
    },
    V2 {
        document: &'a Document,
        owner_password: &'a str,
        user_password: &'a str,
        key_length: usize,
        permissions: Permissions,
    },
    V4 {
        document: &'a Document,
        encrypt_metadata: bool,
        crypt_filters:
            BTreeMap<Vec<u8>, std::sync::Arc<dyn encryption::crypt_filters::CryptFilter>>,
        stream_filter: Vec<u8>,
        string_filter: Vec<u8>,
        owner_password: &'a str,
        user_password: &'a str,
        permissions: Permissions,
    },
    V5 {
        encrypt_metadata: bool,
        crypt_filters:
            BTreeMap<Vec<u8>, std::sync::Arc<dyn encryption::crypt_filters::CryptFilter>>,
        file_encryption_key: &'a [u8],
        stream_filter: Vec<u8>,
        string_filter: Vec<u8>,
        owner_password: &'a str,
        user_password: &'a str,
        permissions: Permissions,
    },
}

impl TryFrom<EncryptionVersion<'_>> for EncryptionState {
    type Error = Error;

    fn try_from(version: EncryptionVersion<'_>) -> std::result::Result<Self, Self::Error> {
        let mut carrier = lopdf::Document::new();
        match version {
            EncryptionVersion::V1 {
                document,
                owner_password,
                user_password,
                permissions,
            } => {
                carrier.trailer = document.trailer.clone();
                EncryptionState::try_from(lopdf::EncryptionVersion::V1 {
                    document: &carrier,
                    owner_password,
                    user_password,
                    permissions,
                })
            }
            EncryptionVersion::V2 {
                document,
                owner_password,
                user_password,
                key_length,
                permissions,
            } => {
                carrier.trailer = document.trailer.clone();
                EncryptionState::try_from(lopdf::EncryptionVersion::V2 {
                    document: &carrier,
                    owner_password,
                    user_password,
                    key_length,
                    permissions,
                })
            }
            EncryptionVersion::V4 {
                document,
                encrypt_metadata,
                crypt_filters,
                stream_filter,
                string_filter,
                owner_password,
                user_password,
                permissions,
            } => {
                carrier.trailer = document.trailer.clone();
                EncryptionState::try_from(lopdf::EncryptionVersion::V4 {
                    document: &carrier,
                    encrypt_metadata,
                    crypt_filters,
                    stream_filter,
                    string_filter,
                    owner_password,
                    user_password,
                    permissions,
                })
            }
            EncryptionVersion::V5 {
                encrypt_metadata,
                crypt_filters,
                file_encryption_key,
                stream_filter,
                string_filter,
                owner_password,
                user_password,
                permissions,
            } => EncryptionState::try_from(lopdf::EncryptionVersion::V5 {
                encrypt_metadata,
                crypt_filters,
                file_encryption_key,
                stream_filter,
                string_filter,
                owner_password,
                user_password,
                permissions,
            }),
        }
    }
}
