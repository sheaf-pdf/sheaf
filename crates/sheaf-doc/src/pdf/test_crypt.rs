//! Deterministic encryption fixtures for tests (`#[cfg(any(test, feature = "fixtures"))]`),
//! backed by `pdf_crypt::writer` and `pdf_structure::write::Encryptor` with
//! `insecure-test-writer` (M18 slices 12–13).
//!
//! Production saves never enable `insecure-test-writer` and always draw fresh
//! 16-byte OS CSPRNG IVs via `pdf_crypt::Ivs::new()`.

use pdf_crypt::Password;
use pdf_crypt::writer::{Kind, Settings, dictionary};
use pdf_filters::Backend;
use pdf_objects::Object;
use pdf_structure::Reading;
use pdf_structure::read::Options;
use pdf_structure::write::{Encryptor, Section, encryption};

use super::{Dictionary, Document, Permissions, literal};

/// Cipher revisions exercised across `sheaf`'s test suites.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CipherKind {
    /// RC4 40-bit (`/V 1 /R 2`).
    Rc4_40,
    /// RC4 128-bit (`/V 2 /R 3 /Length 128`).
    Rc4_128,
    /// AES-128 crypt filter (`/V 4 /R 4 /CF << /StdCF << /CFM /AESV2 /Length 16 >> >>`).
    Aes128,
    /// AES-256 crypt filter (`/V 5 /R 6 /CF << /StdCF << /CFM /AESV3 /Length 32 >> >>`).
    Aes256,
}

/// Specification of a test PDF's encryption dictionary and passwords.
#[derive(Clone, Debug)]
pub struct TestEncryption {
    pub kind: CipherKind,
    pub user_password: String,
    pub owner_password: String,
    pub permissions: Permissions,
    pub encrypt_metadata: bool,
    pub stm_f: Option<Vec<u8>>,
    pub str_f: Option<Vec<u8>>,
    pub eff: Option<Vec<u8>>,
}

impl TestEncryption {
    #[must_use]
    pub fn new(
        kind: CipherKind,
        user_password: impl Into<String>,
        owner_password: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            user_password: user_password.into(),
            owner_password: owner_password.into(),
            permissions: Permissions::ALL,
            encrypt_metadata: true,
            stm_f: None,
            str_f: None,
            eff: None,
        }
    }

    #[must_use]
    pub fn with_permissions(mut self, permissions: Permissions) -> Self {
        self.permissions = permissions;
        self
    }

    #[must_use]
    pub fn with_encrypt_metadata(mut self, encrypt_metadata: bool) -> Self {
        self.encrypt_metadata = encrypt_metadata;
        self
    }

    #[must_use]
    pub fn with_filters(
        mut self,
        stm_f: impl Into<Vec<u8>>,
        str_f: impl Into<Vec<u8>>,
        eff: Option<Vec<u8>>,
    ) -> Self {
        self.stm_f = Some(stm_f.into());
        self.str_f = Some(str_f.into());
        self.eff = eff;
        self
    }

    /// Whether `/StrF` encrypts strings (i.e. is not `/Identity`).
    #[must_use]
    pub fn encrypts_strings(&self) -> bool {
        !matches!(self.str_f.as_deref(), Some(b"Identity"))
    }

    /// Builds the PDF `/Encrypt` dictionary as stored in a test file, with
    /// literal strings matching parsed `Dictionary` forms.
    #[must_use]
    pub fn encrypt_dict(&self, plain: &Document) -> Dictionary {
        let first_id = first_id_bytes(&plain.trailer);
        let writer_kind = match self.kind {
            CipherKind::Rc4_40 => Kind::Rc4By40,
            CipherKind::Rc4_128 => Kind::Rc4By128,
            CipherKind::Aes128 => Kind::Aes128,
            CipherKind::Aes256 => Kind::Aes256,
        };
        let mut p = self.permissions;
        if self.kind == CipherKind::Aes256 {
            // Revision 6 mandates bit 10 (ACCESSIBILITY) in /P (ISO 32000-2 Table 22).
            p = p | Permissions::ACCESSIBILITY;
        }
        let settings = Settings {
            kind: writer_kind,
            user: self.user_password.as_bytes(),
            owner: self.owner_password.as_bytes(),
            p: p.p(),
            id: &first_id,
            encrypt_metadata: self.encrypt_metadata,
        };
        let mut values = dictionary(&settings);
        if self.kind == CipherKind::Rc4_40 {
            // /Length was introduced in PDF 1.4 (/V >= 2); omit on /V 1.
            values.length = None;
        }
        if let Some(stm_f) = &self.stm_f {
            values.stm_f = Some(stm_f.clone());
        }
        if let Some(str_f) = &self.str_f {
            values.str_f = Some(str_f.clone());
        }
        let cf_bytes = match self.kind {
            CipherKind::Aes128 => Some(16),
            CipherKind::Aes256 => Some(32),
            CipherKind::Rc4_40 | CipherKind::Rc4_128 => None,
        };
        let mut raw = encryption(&values, cf_bytes);
        if self.kind == CipherKind::Aes256 {
            // Keep top-level /Length 256 so poppler/qpdf accept V5 streams cleanly.
            raw.set("Length", 256_i64);
        }
        if let Some(eff) = &self.eff {
            raw.set("EFF", Object::name(eff.as_slice()));
        }
        // Normalize string entries to explicit Form::Literal so in-memory and
        // parsed dictionaries compare equal under `Object::eq`.
        let mut out = Dictionary::new();
        for (k, v) in raw.iter() {
            let normalized = match v.as_bytes() {
                Some(b) => literal(b),
                None => v.clone(),
            };
            out.set(k, normalized);
        }
        out
    }

    /// Opens a genuine [`Encryptor`] (using deterministic `Ivs::derived` for
    /// reproducible test fixtures) from `self.encrypt_dict(plain)`.
    #[must_use]
    pub fn encryptor(&self, plain: &Document) -> Encryptor {
        let dict = self.encrypt_dict(plain);
        let first_id = first_id_bytes(&plain.trailer);
        let mut carrier = b"%PDF-1.7\n".to_vec();
        let mut section = Section::full(3);
        let cat_pos = super::write::indirect(
            &mut carrier,
            pdf_objects::ObjectId::new(1, 0),
            &Object::from(super::dictionary! { "Type" => Object::name("Catalog") }),
        )
        .expect("carrier catalog");
        section.insert(
            1,
            pdf_structure::write::Entry::InUse {
                offset: cat_pos,
                generation: 0,
            },
        );
        let enc_pos = super::write::indirect(
            &mut carrier,
            pdf_objects::ObjectId::new(2, 0),
            &Object::from(dict),
        )
        .expect("carrier /Encrypt");
        section.insert(
            2,
            pdf_structure::write::Entry::InUse {
                offset: enc_pos,
                generation: 0,
            },
        );
        let mut trailer = super::dictionary! {
            "Root" => pdf_objects::ObjectId::new(1, 0),
            "Encrypt" => pdf_objects::ObjectId::new(2, 0),
        };
        if !first_id.is_empty() || plain.trailer.has(b"ID") {
            let id_obj = Object::hex(&first_id);
            trailer.set("ID", vec![id_obj.clone(), id_obj]);
        }
        let xref_pos = carrier.len();
        section
            .table(&mut carrier, trailer, xref_pos)
            .expect("carrier xref");

        let pw_pair = [
            Password(""),
            Password(&self.user_password),
            Password(&self.owner_password),
        ];
        let mut opts = Options::DEFAULT;
        opts.flate = Backend::ZlibRs;
        opts.passwords = &pw_pair;
        let reading = Reading::open(carrier, &opts).expect("carrier opens");
        Encryptor::of(&reading)
            .expect("carrier encryptor")
            .expect("carrier is encrypted")
            .reusing_rc4_keystreams()
            .with_test_ivs(pdf_crypt::Ivs::derived)
    }
}

fn first_id_bytes(trailer: &Dictionary) -> Vec<u8> {
    trailer
        .get(b"ID")
        .and_then(Object::as_array)
        .and_then(|arr| arr.first())
        .and_then(Object::as_bytes)
        .map(<[u8]>::to_vec)
        .unwrap_or_default()
}
