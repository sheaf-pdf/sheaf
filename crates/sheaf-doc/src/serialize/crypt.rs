//! Encryption as a save writes it: each object encrypted on its own with
//! the source's [`Crypt`], so an encrypted file saves encrypted, its
//! `/Encrypt` dictionary as stored and its `/ID[0]` kept.
//!
//! lopdf's walk decides what an object's encryption covers: every string,
//! a stream dictionary's included, and every stream, an embedded file's
//! with `/EFF`'s filter and a stream that names its own `/Crypt` with
//! that; but not an encryption dictionary, a cross-reference stream, the
//! metadata when `/EncryptMetadata` is false, or a signature's
//! `/Contents`.

use hmac::{Hmac, KeyInit as _, Mac as _};
use lopdf::encryption::{self, EncryptionState};
use lopdf::{Dictionary, LazyPdf, Object, ObjectId};
use sha2::Sha256;

/// How a source is encrypted: what a save needs to write it encrypted
/// the same way, whose password opens it.
pub struct Crypt {
    state: EncryptionState,
    /// The `/Encrypt` dictionary, as stored.
    dict: Dictionary,
    /// Its number.
    at: ObjectId,
    /// The trailer's `/ID[0]`, which RC4's and AES-128's keys are made
    /// from.
    first_id: Option<Vec<u8>>,
    /// HMAC-SHA256 under the file key, for AES IVs.
    ivs: Hmac<Sha256>,
}

impl Crypt {
    /// How `pdf` is encrypted, or None when it isn't. The `/Encrypt`
    /// dictionary is read as `pdf` stores it.
    pub fn of(pdf: &LazyPdf) -> lopdf::Result<Option<Crypt>> {
        let Some((state, at)) = pdf.encryption() else {
            return Ok(None);
        };
        let ivs = Hmac::new_from_slice(state.file_encryption_key())
            .expect("HMAC takes a key of any length");
        Ok(Some(Crypt {
            state: state.clone(),
            dict: pdf.get_dictionary(at)?.clone(),
            at,
            first_id: super::first_id(pdf.trailer()),
            ivs,
        }))
    }

    pub fn state(&self) -> &EncryptionState {
        &self.state
    }

    /// The `/Encrypt` dictionary, as the source stores it: its `/P`, its
    /// filters and its keys.
    pub fn dict(&self) -> &Dictionary {
        &self.dict
    }

    /// Where the source keeps the dictionary: its number. lopdf opens an
    /// encrypted file only through a reference to it.
    pub fn at(&self) -> ObjectId {
        self.at
    }

    /// The source's `/ID[0]`, read as the key is made from it: the array's
    /// first string, however many it has. None only in an AES-256 file,
    /// whose key isn't made from it; no other opens without one.
    pub fn first_id(&self) -> Option<&[u8]> {
        self.first_id.as_deref()
    }

    /// `obj` encrypted as object `id`, its strings in their formats. The
    /// same object under the same number encrypts to the same bytes.
    pub fn encrypt(&self, id: ObjectId, obj: &Object) -> lopdf::Result<Object> {
        let mut obj = obj.clone();
        encryption::encrypt_object_with(&self.state, id, &mut obj, &mut |payload| {
            self.iv(id, payload)
        })
        .map_err(lopdf::Error::Decryption)?;
        Ok(obj)
    }

    /// The AES IV for `payload` in object `id`: the first 16 bytes of
    /// HMAC-SHA256, under the file key, of the number and generation it's
    /// written as (its ObjKey from slice 9) and the payload. So a save is
    /// deterministic, and no IV serves two payloads of an object.
    fn iv(&self, (num, generation): ObjectId, payload: &[u8]) -> [u8; 16] {
        let mut mac = self.ivs.clone();
        mac.update(&num.to_be_bytes());
        mac.update(&generation.to_be_bytes());
        mac.update(payload);
        let mut iv = [0; 16];
        iv.copy_from_slice(&mac.finalize().into_bytes()[..16]);
        iv
    }
}
