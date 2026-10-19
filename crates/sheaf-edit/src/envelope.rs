//! Op JSON as it's kept and sent: a list of ops with the version they're
//! written in.

use std::fmt;

use schemars::JsonSchema;
use serde::de::{self, MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};

use crate::op::Op;

/// The Op JSON version this build writes, and the newest it reads. A field
/// added since 1.0 has a default, so a reader of a later minor version
/// reads every earlier one.
pub const VERSION: &str = "1.0";

/// Ops as a file (`sheaf edit`) or a plugin hands them over:
/// `{"v": "1.0", "label": "Bookmark forms", "ops": [...]}`. Reading one
/// refuses a version this build can't read: another major, or a later
/// minor. Write `v` first, so a reader that can't read the rest says why.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Envelope {
    /// The version it's written in, as `"1.0"`.
    pub v: String,
    /// What the version it makes is called, in history.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub ops: Vec<Op>,
}

impl Envelope {
    /// `ops`, written in this build's version.
    pub fn new(label: Option<String>, ops: Vec<Op>) -> Envelope {
        Envelope {
            v: VERSION.into(),
            label,
            ops,
        }
    }
}

/// Whether this build reads Op JSON of version `v`: its major and an
/// earlier or the same minor.
fn readable(v: &str) -> Result<(), String> {
    let parse = |v: &str| {
        let (major, minor) = v.split_once('.')?;
        let number = |n: &str| {
            let digits = !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit());
            digits.then(|| n.parse::<u32>().ok()).flatten()
        };
        number(major).zip(number(minor))
    };
    let ours = parse(VERSION).expect("VERSION is major.minor");
    match parse(v) {
        None => Err(format!("{v:?} isn't an Op JSON version, as \"{VERSION}\"")),
        Some((major, minor)) if major == ours.0 && minor <= ours.1 => Ok(()),
        Some(_) => Err(format!(
            "these ops are written in version {v}, and this sheaf reads up to {VERSION}"
        )),
    }
}

impl<'de> Deserialize<'de> for Envelope {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Envelope, D::Error> {
        const FIELDS: &[&str] = &["v", "label", "ops"];

        struct EnvelopeVisitor;

        impl<'de> Visitor<'de> for EnvelopeVisitor {
            type Value = Envelope;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("an op envelope: {\"v\": \"1.0\", \"ops\": [...]}")
            }

            // `v` is checked as soon as it's read, so ops after it are read
            // only in a version this build reads.
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Envelope, A::Error> {
                let mut v: Option<String> = None;
                let mut label: Option<Option<String>> = None;
                let mut ops: Option<Vec<Op>> = None;
                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "v" if v.is_some() => return Err(de::Error::duplicate_field("v")),
                        "v" => {
                            let version: String = map.next_value()?;
                            readable(&version).map_err(de::Error::custom)?;
                            v = Some(version);
                        }
                        "label" if label.is_some() => {
                            return Err(de::Error::duplicate_field("label"));
                        }
                        "label" => label = Some(map.next_value()?),
                        "ops" if ops.is_some() => return Err(de::Error::duplicate_field("ops")),
                        "ops" => ops = Some(map.next_value()?),
                        other => return Err(de::Error::unknown_field(other, FIELDS)),
                    }
                }
                Ok(Envelope {
                    v: v.ok_or_else(|| de::Error::missing_field("v"))?,
                    label: label.flatten(),
                    ops: ops.ok_or_else(|| de::Error::missing_field("ops"))?,
                })
            }
        }

        d.deserialize_struct("Envelope", FIELDS, EnvelopeVisitor)
    }
}
