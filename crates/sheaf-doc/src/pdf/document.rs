//! In-memory PDF document owned by `sheaf_doc::pdf` (M18 slices 10–12).
//!
//! Loaded whole through [`super::LazyPdf`] (`pdf-structure`) for the Rewrite
//! graph, stamp libraries, form checks and test assertions, and built in
//! memory for PDFs `sheaf` synthesizes (`serialize/built.rs`, `annot.rs`,
//! fixtures, and CLI helpers), written through `pdf_objects::serialize` and
//! `pdf_structure::write`.

use std::collections::{BTreeMap, HashSet};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

use pdf_crypt::{Password, Permissions, Protection};
use pdf_objects::{Object, ObjectId, Value, resolve_fn};
use pdf_structure::write::{Encryptor, Entry, Section, container, packable};

use crate::serialize::full;

use super::write as obj_write;
use super::{
    Dictionary, Error, FileBytes, ReadLimits, Result, StreamExt, XrefKind, deflate_options,
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
    /// Indirect objects keyed by `ObjectId`.
    pub objects: BTreeMap<ObjectId, Object>,
    /// Highest object number allocated or loaded so far.
    pub max_id: u32,
    /// Cross-reference section format used by [`Document::save`] / [`Document::save_to`].
    pub xref_kind: XrefKind,
    /// Byte offset of the cross-reference section (`0` when built or recovered by scanning).
    pub xref_start: usize,
    /// Whether this document was decrypted from an encrypted source on load.
    pub(crate) decrypted: bool,
    /// Optional active encryptor when encrypting on `save_to` / `save`.
    encryptor: Option<Encryptor>,
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
            encryptor: None,
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
        ObjectId::new(self.max_id, 0)
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
        while let Some(ref_id) = object.as_reference() {
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
            match self.objects.get(&id).and_then(Object::as_reference) {
                Some(next) => {
                    id = next;
                    nb_deref += 1;
                    if nb_deref > DEREF_LIMIT {
                        return Err(Error::ReferenceLimit);
                    }
                }
                None => {
                    if !self.objects.contains_key(&id) {
                        return Err(Error::ObjectNotFound(id));
                    }
                    break;
                }
            }
        }
        self.objects.get_mut(&id).ok_or(Error::ObjectNotFound(id))
    }

    /// Looks up `id` as a plain dictionary (not a stream).
    pub fn get_dictionary(&self, id: ObjectId) -> Result<&Dictionary> {
        match self.get_object(id)?.value() {
            Value::Dict(d) => Ok(d),
            _ => Err(Error::ObjectType {
                expected: "Dictionary",
            }),
        }
    }

    /// Looks up `id` mutably as a plain dictionary (not a stream).
    pub fn get_dictionary_mut(&mut self, id: ObjectId) -> Result<&mut Dictionary> {
        let obj = self.get_object_mut(id)?;
        if !matches!(obj.value(), Value::Dict(_)) {
            return Err(Error::ObjectType {
                expected: "Dictionary",
            });
        }
        obj.as_dict_mut().ok_or(Error::ObjectType {
            expected: "Dictionary",
        })
    }

    /// Returns the document `/Root` catalog dictionary.
    pub fn catalog(&self) -> Result<&Dictionary> {
        let id = self
            .trailer
            .get(b"Root")
            .and_then(Object::as_reference)
            .ok_or(Error::ObjectType {
                expected: "Catalog reference",
            })?;
        self.get_dictionary(id)
    }

    /// Returns a mutable reference to the document `/Root` catalog dictionary.
    pub fn catalog_mut(&mut self) -> Result<&mut Dictionary> {
        let id = self
            .trailer
            .get(b"Root")
            .and_then(Object::as_reference)
            .ok_or(Error::ObjectType {
                expected: "Catalog reference",
            })?;
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
            if let Some(res) = dict.get(b"Resources") {
                let (_, resolved) = self.dereference(res).ok()?;
                return match resolved.value() {
                    Value::Dict(d) => Some(d),
                    _ => None,
                };
            }
            cur = dict.get(b"Parent").and_then(Object::as_reference);
        }
        None
    }

    /// Resolves annotation dictionaries from `page_id`'s `/Annots` array.
    pub fn get_page_annotations(&self, page_id: ObjectId) -> Result<Vec<&Dictionary>> {
        let Ok(page) = self.get_dictionary(page_id) else {
            return Ok(Vec::new());
        };
        let Some(annots) = page.get(b"Annots") else {
            return Ok(Vec::new());
        };
        let entries: &[Object] = match annots.value() {
            Value::Ref(id) => self
                .get_object(*id)?
                .as_array()
                .ok_or(Error::ObjectType { expected: "Array" })?,
            Value::Array(entries) => entries,
            _ => return Ok(Vec::new()),
        };
        let mut out = Vec::with_capacity(entries.len());
        for entry in entries {
            match entry.value() {
                Value::Ref(id) => {
                    if let Ok(dict) = self.get_dictionary(*id) {
                        out.push(dict);
                    }
                }
                Value::Dict(dict) => out.push(dict),
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
        let Some(contents) = page.get(b"Contents") else {
            return Vec::new();
        };
        let mut streams = Vec::new();
        let mut cur = contents;
        let mut deref = 0;
        loop {
            match cur.value() {
                Value::Ref(id) => match self.objects.get(id).map(Object::value) {
                    None | Some(Value::Stream(_)) => {
                        streams.push(*id);
                        break;
                    }
                    Some(_) => {
                        let Some(other) = self.objects.get(id) else {
                            break;
                        };
                        deref += 1;
                        if deref > DEREF_LIMIT {
                            break;
                        }
                        cur = other;
                    }
                },
                Value::Array(arr) => {
                    for item in arr {
                        if let Some(id) = item.as_reference() {
                            streams.push(id);
                        }
                    }
                    break;
                }
                _ => break,
            }
        }
        let resolve = resolve_fn(|id| self.objects.get(&id));
        let mut out = Vec::new();
        for (i, id) in streams.into_iter().enumerate() {
            let Ok(obj) = self.get_object(id) else {
                continue;
            };
            let Some(stream) = obj.as_stream() else {
                continue;
            };
            if i > 0 && !out.is_empty() {
                out.push(b'\n');
            }
            match stream
                .decompressed_content_resolved(ReadLimits::DEFAULT.max_decompressed, Some(&resolve))
            {
                Ok(decoded) => out.extend_from_slice(&decoded),
                Err(_) => out.extend_from_slice(stream.raw_bytes()),
            }
        }
        out
    }

    /// Removes all objects unreachable from `self.trailer`, returning their IDs.
    pub fn prune_objects(&mut self) -> Vec<ObjectId> {
        let mut reachable = HashSet::new();
        let mut stack: Vec<ObjectId> = self
            .trailer
            .iter()
            .flat_map(|(_, v)| v.references())
            .collect();
        while let Some(id) = stack.pop() {
            if !reachable.insert(id) {
                continue;
            }
            if let Some(obj) = self.objects.get(&id) {
                stack.extend(obj.references());
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

    /// Configures revision-6 AES-256 encryption (OS CSPRNG keys, salts, and IVs)
    /// to be applied when saving `self`.
    pub fn encrypt_aes256(
        &mut self,
        user_password: &str,
        owner_password: &str,
        permissions: Permissions,
    ) -> Result<()> {
        if self.is_encrypted() {
            return Err(Error::AlreadyEncrypted);
        }
        let user_enc = (!user_password.is_empty())
            .then(|| Password(user_password).prepared())
            .transpose()
            .map_err(|e| Error::Decryption(super::DecryptionError::Failed(e.to_string())))?;
        let owner_enc = (!owner_password.is_empty())
            .then(|| Password(owner_password).prepared())
            .transpose()
            .map_err(|e| Error::Decryption(super::DecryptionError::Failed(e.to_string())))?;
        let mut protection = Protection::DEFAULT;
        protection.user = user_enc.as_ref();
        protection.owner = owner_enc.as_ref();
        protection.permissions = permissions;
        let enc = Encryptor::create(&protection)
            .map_err(|e| Error::Decryption(super::DecryptionError::Failed(e.to_string())))?;
        if self.version.as_str() < "2.0" {
            self.version = "2.0".to_string();
        }
        let encrypt_id = self.add_object(enc.dictionary().clone());
        self.trailer.set("Encrypt", encrypt_id);
        self.encryptor = Some(enc);
        Ok(())
    }

    /// Configures deterministic test encryption from `spec` (`#[cfg(any(test, feature = "fixtures"))]`),
    /// recording `/Encrypt` in `self.objects` and `self.trailer` so callers may
    /// inspect or mutate the dictionary before `save` / `save_to`.
    #[cfg(any(test, feature = "fixtures"))]
    pub fn encrypt(&mut self, spec: &super::test_crypt::TestEncryption) -> Result<()> {
        if self.is_encrypted() {
            return Err(Error::AlreadyEncrypted);
        }
        let enc = spec.encryptor(self);
        let dict = spec.encrypt_dict(self);
        let encrypt_id = self.add_object(dict);
        self.trailer.set("Encrypt", encrypt_id);
        self.encryptor = Some(enc);
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
    /// already encrypted).
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

        let encrypt_id = self.trailer.get(b"Encrypt").and_then(Object::as_reference);
        let resolve = resolve_fn(|id| self.objects.get(&id));
        let encrypting = match &self.encryptor {
            Some(enc) => Some(enc.laid_out(None, encrypt_id, &self.trailer, &resolve)?),
            None => None,
        };

        let mut next = self.max_id.saturating_add(1).max(1);
        let mut section = Section::full(next);
        let mut packable_members: Vec<(u32, Vec<u8>)> = Vec::new();
        let deflate = deflate_options();

        for (&id, obj) in &self.objects {
            if full::left_out(obj) {
                continue;
            }
            next = next.max(id.number.saturating_add(1));
            if Some(id) == encrypt_id {
                let offset = obj_write::indirect_renumbered(
                    &mut out,
                    id,
                    obj,
                    &(),
                    &obj_write::non_zero_ref,
                )?;
                section.insert(
                    id.number,
                    Entry::InUse {
                        offset,
                        generation: id.generation,
                    },
                );
                continue;
            }
            if pack.is_some() && packable(id, obj) {
                let mut member_bytes = Vec::new();
                anyhow::Context::with_context(
                    obj_write::write_renumbered(&mut member_bytes, obj, &obj_write::non_zero_ref),
                    || format!("writing object {id}"),
                )?;
                packable_members.push((id.number, member_bytes));
            } else {
                let offset = match &encrypting {
                    Some(enc) => {
                        let stripped = strip_identity_metadata(enc, id, obj);
                        let body = stripped.as_ref().unwrap_or(obj);
                        obj_write::indirect_renumbered(
                            &mut out,
                            id,
                            body,
                            enc,
                            &obj_write::non_zero_ref,
                        )?
                    }
                    None => obj_write::indirect_renumbered(
                        &mut out,
                        id,
                        obj,
                        &(),
                        &obj_write::non_zero_ref,
                    )?,
                };
                section.insert(
                    id.number,
                    Entry::InUse {
                        offset,
                        generation: id.generation,
                    },
                );
            }
        }

        if let Some(per) = pack {
            for chunk in packable_members.chunks(per) {
                if let Some(stream) = container(chunk, deflate)? {
                    let container_id = next;
                    next += 1;
                    let offset = obj_write::indirect(
                        &mut out,
                        ObjectId::new(container_id, 0),
                        &Object::from(stream),
                    )?;
                    section.insert(
                        container_id,
                        Entry::InUse {
                            offset,
                            generation: 0,
                        },
                    );
                    for (index, &(member_num, _)) in chunk.iter().enumerate() {
                        section.insert(
                            member_num,
                            Entry::Compressed {
                                stream: container_id,
                                index: index as u16,
                            },
                        );
                    }
                } else {
                    for (member_num, member_bytes) in chunk {
                        let offset = out.len();
                        out.extend_from_slice(format!("{member_num} 0 obj\n").as_bytes());
                        out.extend_from_slice(member_bytes);
                        out.extend_from_slice(b"\nendobj\n");
                        section.insert(
                            *member_num,
                            Entry::InUse {
                                offset,
                                generation: 0,
                            },
                        );
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
                section.stream(&mut out, next, trailer, pos, deflate)?;
            }
        }
        Ok(out)
    }
}

/// If a newly created encryption (`encrypt_aes256`) encrypts metadata and the
/// catalog's `/Metadata` stream carries `/Filter /Crypt` (default `/Identity`),
/// strip that filter so `Encrypting::stream` encrypts it instead of refusing
/// `Unencryptable::Metadata`.
fn strip_identity_metadata(
    enc: &pdf_structure::write::Encrypting<'_>,
    _id: ObjectId,
    obj: &Object,
) -> Option<Object> {
    if enc.at().is_some() {
        return None;
    }
    let Value::Stream(stream) = obj.value() else {
        return None;
    };
    if !stream.dict.has_type(b"Metadata") {
        return None;
    }
    if stream.dict.get(b"Filter").and_then(Object::as_name) == Some(b"Crypt") {
        let mut copy = stream.as_ref().clone();
        copy.dict.remove(b"Filter");
        copy.dict.remove(b"DecodeParms");
        return Some(Object::from(copy));
    }
    None
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
            .ok()
            .and_then(|cat| cat.get(b"Pages"))
            .and_then(Object::as_reference)
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
        let kids = pages.get(b"Kids")?;
        doc.dereference(kids).ok()?.1.as_array()
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
                if let Some(kid_id) = kid.as_reference() {
                    match self.doc.get_dictionary(kid_id) {
                        Ok(dict) => match dict.type_name() {
                            Some(b"Page") | None => return Some(kid_id),
                            Some(b"Pages") if self.stack.len() < PAGE_TREE_DEPTH_LIMIT => {
                                if !rest.is_empty() {
                                    self.stack.push(rest);
                                }
                                self.kids = Self::kids(self.doc, kid_id);
                            }
                            Some(_) => {}
                        },
                        Err(_) => return Some(kid_id),
                    }
                }
            }
            self.kids = Some(self.stack.pop()?);
        }
    }
}

impl std::iter::FusedIterator for PageTreeIter<'_> {}
