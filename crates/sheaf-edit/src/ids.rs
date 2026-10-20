//! What a document's things are called: ids a document hands out, which
//! last the session, and blobs, named by their bytes.

use std::borrow::Cow;
use std::fmt;
use std::str::FromStr;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::{self, Unexpected};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Stable identity of a page in the virtual document. Never reused.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
pub struct PageId(pub u64);

/// A bookmark's id in a document. Never reused.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
pub struct OutlineId(pub u64);

/// An annotation's id in a document: a mark's, another tool's comment's or
/// link's, a field's widget's. One id space for them all.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
pub struct AnnotId(pub u64);

impl AnnotId {
    /// What a mark with this id and no name of its own is called in the
    /// file (`/NM`), and the StrikeOut of a replacement: the names PDFium
    /// is told to hide.
    pub fn sheaf_names(self) -> [String; 2] {
        [
            format!("sheaf:{}", self.0),
            format!("sheaf:{}:strike", self.0),
        ]
    }
}

/// A field's id in a document. Never reused.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
pub struct FieldId(pub u64);

/// A name for something an op makes, for the ops after it in the same
/// transaction: an op names what it makes with `local` or `locals`, and a
/// later op refers to it as `New(local)`. Written `"$1"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Local(pub u16);

impl fmt::Display for Local {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "${}", self.0)
    }
}

/// A string that isn't a local: `$` and a number, as in `"$1"`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotALocal(pub String);

impl fmt::Display for NotALocal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:?} isn't a local: a local is `$` and a number up to {}, as in \"$1\"",
            self.0,
            u16::MAX
        )
    }
}

impl std::error::Error for NotALocal {}

impl FromStr for Local {
    type Err = NotALocal;

    fn from_str(s: &str) -> Result<Local, NotALocal> {
        let n = (s.strip_prefix('$'))
            .filter(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
            .and_then(|n| n.parse().ok());
        n.map(Local).ok_or_else(|| NotALocal(s.to_owned()))
    }
}

impl Serialize for Local {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Local {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Local, D::Error> {
        let s = String::deserialize(d)?;
        s.parse().map_err(de::Error::custom)
    }
}

impl JsonSchema for Local {
    fn schema_name() -> Cow<'static, str> {
        "Local".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "Something an earlier op in the same transaction made, by the name it gave it: `$` and a number.",
            "type": "string",
            "pattern": "^\\$[0-9]+$",
        })
    }
}

/// A blob's name: its checksum and its length. Two blobs with one id are
/// compared byte for byte before they're taken as one. Written
/// `"b<32 hex digits>-<length>"`: the checksum is too wide for a JSON
/// number.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct BlobId {
    /// sheaf's checksum of the bytes (xxh3-128).
    pub hash: u128,
    pub len: u64,
}

impl BlobId {
    /// The blob `stem` names: the generated cache's name for a file (its
    /// checksum in 32 hex digits, a dash and its length), which marks name
    /// a stamp's page and an attachment's file by. None for any other
    /// name, an older one of 16 hex digits among them.
    pub fn from_stem(stem: &str) -> Option<BlobId> {
        let (hash, len) = stem.split_once('-')?;
        let id = BlobId {
            hash: u128::from_str_radix(hash, 16).ok()?,
            len: len.parse().ok()?,
        };
        (id.stem() == stem).then_some(id)
    }

    /// Its name as the generated cache gives it, what marks name it by.
    pub fn stem(&self) -> String {
        format!("{:032x}-{}", self.hash, self.len)
    }
}

/// A source: a PDF blob, parsed (M10.5 §3.1). Inserting the same bytes
/// twice is one source; which copy of its pages and of its form each
/// insert brought is the page's and the form instance's.
pub type SourceId = BlobId;

impl Serialize for BlobId {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(&format_args!("b{}", self.stem()))
    }
}

impl<'de> Deserialize<'de> for BlobId {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<BlobId, D::Error> {
        let s = String::deserialize(d)?;
        (s.strip_prefix('b').and_then(BlobId::from_stem)).ok_or_else(|| {
            de::Error::invalid_value(
                Unexpected::Str(&s),
                &"a blob id: `b`, 32 lowercase hex digits, `-` and a length",
            )
        })
    }
}

impl JsonSchema for BlobId {
    fn schema_name() -> Cow<'static, str> {
        "BlobId".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "A blob by its bytes: `b`, their checksum in 32 lowercase hex digits, `-` and their length.",
            "type": "string",
            "pattern": "^b[0-9a-f]{32}-(0|[1-9][0-9]*)$",
        })
    }
}
