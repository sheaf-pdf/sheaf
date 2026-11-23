//! Encryption as a save writes it (M18 slice 12): wraps
//! [`crate::pdf::write::Encryptor`] (`pdf-structure` + `pdf-crypt`) so saves
//! keep the source's `/Encrypt` dictionary and `/ID[0]`, drawing fresh
//! 16-byte OS CSPRNG AES IVs in production (`pdf_crypt::Ivs::new()`) per
//! ISO 32000-2 §7.6.3.2 & §7.6.3.3.

use crate::pdf::write::{Encrypting, Encryptor, Renumber, StructureError};
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

    /// Whether a method it encrypts by may make each object's key of its
    /// number (RC4, AES-128: Algorithm 1), so that an object's bytes read
    /// at one number are right at that number alone
    /// ([`keys_by_number`], of its dictionary).
    #[must_use]
    pub fn keys_by_number(&self) -> bool {
        keys_by_number(self.dict())
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
    /// `/Encrypt` placed at `at`, a number written: each object at the
    /// number `to` gives it (none for each at its own), what's left in the
    /// clear and the dictionary's references named as `to` names them.
    /// `trailer` and `resolve` are in the numbers the objects are held at.
    pub fn laid_out<'a>(
        &'a self,
        to: Option<&'a (dyn Renumber + Sync)>,
        at: ObjectId,
        trailer: &Dictionary,
        resolve: &'a (dyn Resolve<'a> + Sync),
    ) -> Result<Encrypting<'a>, StructureError> {
        self.encryptor.laid_out(to, Some(at), trailer, resolve)
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
        let enc = self.laid_out(None, self.at, &trailer, &resolve)?;
        let mut buf = Vec::new();
        crate::pdf::write::indirect_with(&mut buf, id, obj, &enc)?;
        Ok(crate::pdf::write::parse_indirect_object(&buf)?)
    }
}

/// Whether a method the encryption dictionary `dict` names may make each
/// object's key of its number: a `/V` under 5 or none, or under `/V 5` a
/// crypt filter in `/CF` but AES-256's (`/CFM /AESV3`), or a `/CF` that
/// isn't a dictionary where it is (a reference). A `/V 5` dictionary with
/// no `/CF` names no method, so no key is made of a number. Read from the
/// dictionary, it errs toward yes, which loses a copy; the family's writer
/// asks its own reading of the methods before it copies at another number,
/// so a wrong no is an object written anew on its own, never bytes under
/// the wrong key.
fn keys_by_number(dict: &Dictionary) -> bool {
    let v = dict.get(b"V").and_then(Object::as_integer);
    if v.is_none_or(|v| v < 5) {
        return true;
    }
    let Some(filters) = dict.get(b"CF") else {
        return false;
    };
    let Some(filters) = filters.as_dict() else {
        return true;
    };
    filters.iter().any(|(_, filter)| {
        let method = filter.as_dict().and_then(|f| f.get(b"CFM"));
        method.and_then(Object::as_name) != Some(&b"AESV3"[..])
    })
}

#[cfg(test)]
mod tests {
    use super::keys_by_number;
    use crate::pdf::{Dictionary, Object, ObjectId, dictionary};

    /// A crypt filter dictionary of method `cfm`, or one that names none.
    fn filter(cfm: Option<&str>) -> Object {
        let mut d = Dictionary::new();
        if let Some(cfm) = cfm {
            d.set("CFM", Object::name(cfm));
        }
        Object::from(d)
    }

    /// An encryption dictionary of version `v`, its `/CF` as given.
    fn dict(v: Option<i64>, cf: Option<Object>) -> Dictionary {
        let mut d = dictionary! { "Filter" => Object::name("Standard") };
        if let Some(v) = v {
            d.set("V", Object::from(v));
        }
        if let Some(cf) = cf {
            d.set("CF", cf);
        }
        d
    }

    /// Keys are made of an object's number under any `/V` below 5 (RC4,
    /// AES-128), whatever `/CF` says, and where there's no `/V`; under
    /// `/V 5`, only where a crypt filter may be other than AES-256's: one
    /// of another method or none, or a `/CF` that's a reference, which
    /// isn't read. A `/V 5` dictionary with no `/CF`, or whose filters are
    /// all `AESV3`, makes no key of a number: the one answer that lets an
    /// object read at an offset be copied at another number.
    #[test]
    fn keys_are_made_of_the_number_but_under_aes_256_alone() {
        let aes_256 = || filter(Some("AESV3"));
        let cf = |filters: Vec<(&str, Object)>| {
            let mut d = Dictionary::new();
            for (name, f) in filters {
                d.set(name, f);
            }
            Some(Object::from(d))
        };
        let cases = [
            ("no /V", dict(None, None), true),
            ("/V 1", dict(Some(1), None), true),
            ("/V 2", dict(Some(2), None), true),
            (
                "/V 4, AESV2",
                dict(Some(4), cf(vec![("StdCF", filter(Some("AESV2")))])),
                true,
            ),
            (
                "/V 4, a filter named AESV3",
                dict(Some(4), cf(vec![("StdCF", aes_256())])),
                true,
            ),
            ("/V 5, no /CF", dict(Some(5), None), false),
            ("/V 5, an empty /CF", dict(Some(5), cf(vec![])), false),
            (
                "/V 5, AESV3",
                dict(Some(5), cf(vec![("StdCF", aes_256())])),
                false,
            ),
            (
                "/V 5, two AESV3",
                dict(Some(5), cf(vec![("A", aes_256()), ("B", aes_256())])),
                false,
            ),
            (
                "/V 5, /CF a reference",
                dict(Some(5), Some(Object::from(ObjectId::new(11, 0)))),
                true,
            ),
            (
                "/V 5, AESV2",
                dict(Some(5), cf(vec![("StdCF", filter(Some("AESV2")))])),
                true,
            ),
            (
                "/V 5, V2",
                dict(Some(5), cf(vec![("StdCF", filter(Some("V2")))])),
                true,
            ),
            (
                "/V 5, AESV3 and AESV2",
                dict(
                    Some(5),
                    cf(vec![("A", aes_256()), ("B", filter(Some("AESV2")))]),
                ),
                true,
            ),
            (
                "/V 5, no /CFM",
                dict(Some(5), cf(vec![("StdCF", filter(None))])),
                true,
            ),
            (
                "/V 5, a filter that's a reference",
                dict(
                    Some(5),
                    cf(vec![("StdCF", Object::from(ObjectId::new(11, 0)))]),
                ),
                true,
            ),
            (
                "/V 6, AESV3",
                dict(Some(6), cf(vec![("StdCF", aes_256())])),
                false,
            ),
        ];
        for (what, dict, by_number) in cases {
            assert_eq!(keys_by_number(&dict), by_number, "{what}");
        }
    }
}
