//! PDF layer errors and resource limits owned by `sheaf_doc::pdf` (M18 slices 8–13).

use pdf_objects::ObjectId;

pub type Result<T> = std::result::Result<T, Error>;

/// Caps [`super::LazyPdf`] and bounded stream decoders enforce.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Limit {
    Decompressed,
    Sections,
    Objects,
    Depth,
}

/// Decryption failure reasons surfaced at the PDF door.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DecryptionError {
    #[error("undefined crypt filter: {}", String::from_utf8_lossy(.0))]
    UndefinedCryptFilter(Vec<u8>),
    #[error("AES ciphertext padding error")]
    Padding,
    #[error("decryption failed: {0}")]
    Failed(String),
}

/// Errors produced by `sheaf_doc::pdf` reading, stream decoding, and document
/// graph lookups.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    IO(#[from] std::io::Error),
    #[error("invalid password")]
    InvalidPassword,
    #[error("limit exceeded: {0:?}")]
    Limit(Limit),
    #[error("missing cross-reference entry")]
    MissingXrefEntry,
    #[error("object ID mismatch")]
    ObjectIdMismatch,
    #[error("object {0} not found")]
    ObjectNotFound(ObjectId),
    #[error("unexpected PDF object type (expected {expected})")]
    ObjectType { expected: &'static str },
    #[error("indirect reference chain limit exceeded")]
    ReferenceLimit,
    #[error("indirect reference cycle at {0}")]
    ReferenceCycle(ObjectId),
    #[error("invalid indirect object at offset {offset}")]
    IndirectObject { offset: usize },
    #[error("decryption error: {0}")]
    Decryption(#[from] DecryptionError),
    #[error("document is already encrypted")]
    AlreadyEncrypted,
    #[error("invalid stream: {0}")]
    InvalidStream(String),
    #[error("{0}")]
    Syntax(String),
}
