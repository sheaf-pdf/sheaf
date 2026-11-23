//! Encryption as a save writes it (M18 slice 12): wraps
//! [`crate::pdf::write::Encryptor`] (`pdf-structure` + `pdf-crypt`) so saves
//! keep the source's `/Encrypt` dictionary and `/ID[0]`, drawing fresh
//! 16-byte OS CSPRNG AES IVs in production (`pdf_crypt::Ivs::new()`) per
//! ISO 32000-2 §7.6.3.2 & §7.6.3.3.

use crate::pdf::write::{Encrypting, Encryptor, StructureError};
use crate::pdf::{Dictionary, LazyPdf, Object, ObjectId, Resolve};

/// How a source is encrypted: what a save needs to write it encrypted
/// the same way, whose password opens it.
#[derive(Clone)]
pub struct Crypt {
    encryptor: Encryptor,
    /// Where the source (or saved file) keeps the `/Encrypt` dictionary.
    at: ObjectId,
    /// Trailer `/ID[0]` when present in the source trailer (`None` if absent).
    first_id: Option<Vec<u8>>,
    /// Source catalog dictionary kept as a fallback resolver for
    /// `/EncryptMetadata false` when an incremental update or raw-copied
    /// rewrite doesn't hold a parsed catalog object in memory.
    fallback_root: Option<(ObjectId, Object)>,
    /// Source document `/Metadata` stream kept alongside `fallback_root` so
    /// `Encryptor::laid_out` resolves `Root -> /Metadata` to a stream even
    /// when the metadata object itself is not rewritten in an update.
    fallback_metadata: Option<(ObjectId, Object)>,
}

impl Crypt {
    /// How `pdf` is encrypted, or `None` when it isn't.
    pub fn of(pdf: &LazyPdf) -> crate::pdf::Result<Option<Crypt>> {
        let Some((encryptor, at)) = pdf.encryption() else {
            return Ok(None);
        };
        let fallback_root = pdf
            .trailer()
            .get(b"Root")
            .and_then(Object::as_reference)
            .and_then(|root_id| {
                pdf.catalog()
                    .ok()
                    .cloned()
                    .map(|cat| (root_id, Object::from(cat)))
            });
        let fallback_metadata = fallback_root
            .as_ref()
            .and_then(|(_, cat)| cat.as_dict()?.get(b"Metadata")?.as_reference())
            .and_then(|meta_id| pdf.get(meta_id).ok().map(|o| (meta_id, o.clone())));
        Ok(Some(Crypt {
            encryptor: encryptor.clone().reusing_rc4_keystreams(),
            at,
            first_id: super::first_id(pdf.trailer()),
            fallback_root,
            fallback_metadata,
        }))
    }

    /// Underlying `pdf_structure::write::Encryptor`.
    #[must_use]
    pub fn encryptor(&self) -> &Encryptor {
        &self.encryptor
    }

    /// The `/Encrypt` dictionary, as the source stores it: its `/P`, its
    /// filters and its keys.
    #[must_use]
    pub fn dict(&self) -> &Dictionary {
        self.encryptor.dictionary()
    }

    /// Where the source keeps the `/Encrypt` dictionary.
    #[must_use]
    pub fn at(&self) -> ObjectId {
        self.at
    }

    /// The source's `/ID[0]`, when its trailer had an `/ID` array whose first
    /// item is a string.
    #[must_use]
    pub fn first_id(&self) -> Option<&[u8]> {
        self.first_id.as_deref()
    }

    /// Whether this encryption uses AES (`AESV2` or `AESV3` in `/CF`).
    #[must_use]
    pub fn is_aes(&self) -> bool {
        let Some(cf) = self.dict().get(b"CF").and_then(Object::as_dict) else {
            return false;
        };
        cf.iter().any(|(_, filter)| {
            filter
                .as_dict()
                .and_then(|f| f.get(b"CFM"))
                .and_then(Object::as_name)
                .is_some_and(|m| m == b"AESV2" || m == b"AESV3")
        })
    }

    /// Whether objects may go in object streams under it (`!Identity /StmF`
    /// or `Identity /StrF`).
    #[must_use]
    pub fn packs(&self) -> bool {
        self.encryptor.packs()
    }

    /// Resolves the source's `/Root` catalog or document `/Metadata` stream
    /// when a save doesn't hold a parsed copy of them in memory.
    pub(super) fn fallback_resolve(&self, id: ObjectId) -> Option<&Object> {
        if let Some((rid, root)) = &self.fallback_root
            && *rid == id
        {
            return Some(root);
        }
        if let Some((mid, meta)) = &self.fallback_metadata
            && *mid == id
        {
            return Some(meta);
        }
        None
    }

    /// Prepares a per-file [`Encrypting`] context for writing objects with
    /// `/Encrypt` placed at `at`.
    pub fn laid_out<'a>(
        &'a self,
        at: ObjectId,
        trailer: &Dictionary,
        resolve: &'a (dyn Resolve<'a> + Sync),
    ) -> Result<Encrypting<'a>, StructureError> {
        self.encryptor.laid_out(None, Some(at), trailer, resolve)
    }

    /// The same encryption with its `/Encrypt` dictionary moved to `at`.
    #[must_use]
    pub fn moved(&self, at: ObjectId) -> Crypt {
        Crypt { at, ..self.clone() }
    }

    /// Switches AES IV generation to deterministic `pdf_crypt::Ivs::derived`
    /// (`insecure-test-writer`), exclusively for tests that pin exact bytes
    /// across threads or runs, here and in other crates' (the `fixtures`
    /// feature: `forms::fixture::try_append_revision`).
    #[cfg(any(test, feature = "fixtures"))]
    #[must_use]
    pub fn with_test_ivs(mut self) -> Crypt {
        self.encryptor = self.encryptor.with_test_ivs(crate::pdf::Ivs::derived);
        self
    }

    /// Serializes `obj` as encrypted indirect object `id` and parses the
    /// written object back, for unit tests inspecting individual encrypted
    /// objects.
    #[cfg(test)]
    pub fn encrypt_object_for_test(&self, id: ObjectId, obj: &Object) -> anyhow::Result<Object> {
        let mut trailer = Dictionary::new();
        if let Some((root_id, _)) = &self.fallback_root {
            trailer.set("Root", *root_id);
        }
        let resolve = crate::pdf::resolve_fn(|oid| {
            if oid == id {
                Some(obj)
            } else {
                self.fallback_resolve(oid)
            }
        });
        let enc = self.laid_out(self.at, &trailer, &resolve)?;
        let mut buf = Vec::new();
        crate::pdf::write::indirect_with(&mut buf, id, obj, &enc)?;
        Ok(crate::pdf::write::parse_indirect_object(&buf)?)
    }
}
