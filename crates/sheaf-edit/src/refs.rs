//! How ops name what they act on (M10.5 §6.6): by id, by a local an earlier
//! op in the same transaction gave something it made, or by what a person
//! can see: a page's position, a bookmark's titles, a comment's name.
//!
//! A reference is a number (an id), a string (a local, `"$1"`) or an
//! object (`{"pos": 3}`). Each kind is read by hand, so a misspelled local
//! is an error rather than another form. Positions are 1-based, as the UI
//! shows them, and resolve against the document as it is when the op
//! runs: `[delete-pages 1, rotate-pages 2]` turns the page that's second
//! after the delete.

use std::borrow::Cow;
use std::fmt;
use std::marker::PhantomData;
use std::ops::RangeInclusive;
use std::str::FromStr;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::value::MapAccessDeserializer;
use serde::de::{self, DeserializeOwned, MapAccess, Unexpected, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::ids::{AnnotId, BlobId, Local, OutlineId, PageId};

/// A page: by id, as a page an earlier op made, or by its position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PageRef {
    Id(PageId),
    New(Local),
    /// 1-based.
    Pos(u32),
}

/// A bookmark: by id, as one an earlier op made, or by the titles from the
/// top of the outline down to it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum BookmarkRef {
    Id(OutlineId),
    New(Local),
    Path(Vec<String>),
}

/// An annotation: by id, as a mark an earlier op made, or by its `/NM`,
/// on `page` when the name might be on more than one.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum AnnotRef {
    Id(AnnotId),
    New(Local),
    Nm { nm: String, page: Option<PageRef> },
}

/// A blob an op uses (an inserted file, a stamp's page, an attached file):
/// by its id; by a name the command line gave a file (`--with NAME=PATH`);
/// or as an asset a plugin uploaded, by its handle. Resolved before the op
/// is lowered.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub enum BlobSel {
    #[serde(rename = "blob")]
    Id(BlobId),
    With(String),
    Asset(u32),
}

/// Pages an op acts on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
#[non_exhaustive]
pub enum PageSel {
    Ids(Vec<PageRef>),
    All,
    /// From `first` to `last`, both included, in document order.
    Span {
        first: PageRef,
        last: PageRef,
    },
    Positions(RangeSet),
    /// The pages a bookmark spans: from its page up to the next
    /// bookmark's.
    Bookmark(BookmarkRef),
}

impl PageSel {
    /// Pages `ids`, by id.
    pub fn ids(ids: impl IntoIterator<Item = PageId>) -> PageSel {
        PageSel::Ids(ids.into_iter().map(PageRef::Id).collect())
    }
}

impl From<PageId> for PageRef {
    fn from(id: PageId) -> PageRef {
        PageRef::Id(id)
    }
}

impl From<OutlineId> for BookmarkRef {
    fn from(id: OutlineId) -> BookmarkRef {
        BookmarkRef::Id(id)
    }
}

impl From<AnnotId> for AnnotRef {
    fn from(id: AnnotId) -> AnnotRef {
        AnnotRef::Id(id)
    }
}

/// Pages something is on: every page, including those added later, or
/// these.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub enum ScopeSel {
    All,
    Pages(PageSel),
}

/// A reference's three forms: an id, a local and an object.
trait RefForms: Sized {
    const EXPECTING: &'static str;
    /// The object form, as read.
    type Object: DeserializeOwned;
    fn id(n: u64) -> Self;
    fn local(l: Local) -> Self;
    fn object(o: Self::Object) -> Result<Self, String>;
}

struct RefVisitor<R>(PhantomData<R>);

impl<'de, R: RefForms> Visitor<'de> for RefVisitor<R> {
    type Value = R;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(R::EXPECTING)
    }

    fn visit_u64<E: de::Error>(self, n: u64) -> Result<R, E> {
        Ok(R::id(n))
    }

    fn visit_i64<E: de::Error>(self, n: i64) -> Result<R, E> {
        let n = u64::try_from(n).map_err(|_| E::invalid_value(Unexpected::Signed(n), &self))?;
        Ok(R::id(n))
    }

    fn visit_str<E: de::Error>(self, s: &str) -> Result<R, E> {
        s.parse().map(R::local).map_err(E::custom)
    }

    fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<R, A::Error> {
        let o = R::Object::deserialize(MapAccessDeserializer::new(map))?;
        R::object(o).map_err(de::Error::custom)
    }
}

fn deserialize_ref<'de, R: RefForms, D: Deserializer<'de>>(d: D) -> Result<R, D::Error> {
    d.deserialize_any(RefVisitor(PhantomData))
}

/// A page's position, 1-based.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Pos {
    pos: u32,
}

impl RefForms for PageRef {
    const EXPECTING: &'static str = "a page: its id, a local (\"$1\") or {\"pos\": n}";
    type Object = Pos;

    fn id(n: u64) -> PageRef {
        PageRef::Id(PageId(n))
    }

    fn local(l: Local) -> PageRef {
        PageRef::New(l)
    }

    fn object(o: Pos) -> Result<PageRef, String> {
        match o.pos {
            0 => Err("page positions are 1-based: the first page is {\"pos\": 1}".into()),
            n => Ok(PageRef::Pos(n)),
        }
    }
}

impl Serialize for PageRef {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match *self {
            PageRef::Id(id) => s.serialize_u64(id.0),
            PageRef::New(l) => l.serialize(s),
            PageRef::Pos(pos) => Pos { pos }.serialize(s),
        }
    }
}

impl<'de> Deserialize<'de> for PageRef {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<PageRef, D::Error> {
        deserialize_ref(d)
    }
}

impl JsonSchema for PageRef {
    fn schema_name() -> Cow<'static, str> {
        "PageRef".into()
    }

    fn json_schema(g: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "A page: its id, a page an earlier op made, or its position, 1-based.",
            "oneOf": [
                g.subschema_for::<PageId>(),
                g.subschema_for::<Local>(),
                {
                    "type": "object",
                    "properties": {
                        "pos": { "description": "1-based.", "type": "integer", "minimum": 1 },
                    },
                    "required": ["pos"],
                    "additionalProperties": false,
                },
            ],
        })
    }
}

/// A bookmark's titles, from the top of the outline down.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TitlePath {
    path: Vec<String>,
}

impl RefForms for BookmarkRef {
    const EXPECTING: &'static str =
        "a bookmark: its id, a local (\"$1\") or {\"path\": [titles from the top]}";
    type Object = TitlePath;

    fn id(n: u64) -> BookmarkRef {
        BookmarkRef::Id(OutlineId(n))
    }

    fn local(l: Local) -> BookmarkRef {
        BookmarkRef::New(l)
    }

    fn object(o: TitlePath) -> Result<BookmarkRef, String> {
        match o.path.is_empty() {
            true => Err("a bookmark's path names at least its own title".into()),
            false => Ok(BookmarkRef::Path(o.path)),
        }
    }
}

impl Serialize for BookmarkRef {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            BookmarkRef::Id(id) => s.serialize_u64(id.0),
            BookmarkRef::New(l) => l.serialize(s),
            BookmarkRef::Path(path) => TitlePath { path: path.clone() }.serialize(s),
        }
    }
}

impl<'de> Deserialize<'de> for BookmarkRef {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<BookmarkRef, D::Error> {
        deserialize_ref(d)
    }
}

impl JsonSchema for BookmarkRef {
    fn schema_name() -> Cow<'static, str> {
        "BookmarkRef".into()
    }

    fn json_schema(g: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "A bookmark: its id, a bookmark an earlier op made, or its titles from the top of the outline down.",
            "oneOf": [
                g.subschema_for::<OutlineId>(),
                g.subschema_for::<Local>(),
                {
                    "type": "object",
                    "properties": {
                        "path": { "type": "array", "items": { "type": "string" }, "minItems": 1 },
                    },
                    "required": ["path"],
                    "additionalProperties": false,
                },
            ],
        })
    }
}

/// A comment's `/NM`, and the page it's on.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Named {
    nm: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    page: Option<PageRef>,
}

impl RefForms for AnnotRef {
    const EXPECTING: &'static str =
        "an annotation: its id, a local (\"$1\") or {\"nm\": its /NM, \"page\": its page}";
    type Object = Named;

    fn id(n: u64) -> AnnotRef {
        AnnotRef::Id(AnnotId(n))
    }

    fn local(l: Local) -> AnnotRef {
        AnnotRef::New(l)
    }

    fn object(o: Named) -> Result<AnnotRef, String> {
        Ok(AnnotRef::Nm {
            nm: o.nm,
            page: o.page,
        })
    }
}

impl Serialize for AnnotRef {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            AnnotRef::Id(id) => s.serialize_u64(id.0),
            AnnotRef::New(l) => l.serialize(s),
            AnnotRef::Nm { nm, page } => Named {
                nm: nm.clone(),
                page: *page,
            }
            .serialize(s),
        }
    }
}

impl<'de> Deserialize<'de> for AnnotRef {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<AnnotRef, D::Error> {
        deserialize_ref(d)
    }
}

impl JsonSchema for AnnotRef {
    fn schema_name() -> Cow<'static, str> {
        "AnnotRef".into()
    }

    fn json_schema(g: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "An annotation: its id, a mark an earlier op made, or its /NM, on its page when the name could be on more than one.",
            "oneOf": [
                g.subschema_for::<AnnotId>(),
                g.subschema_for::<Local>(),
                {
                    "type": "object",
                    "properties": {
                        "nm": { "type": "string" },
                        "page": g.subschema_for::<PageRef>(),
                    },
                    "required": ["nm"],
                    "additionalProperties": false,
                },
            ],
        })
    }
}

/// Positions, 1-based as the UI shows them: runs, sorted and merged, never
/// empty. Written as the UI takes them: `"1-3,5,8-10"`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RangeSet {
    runs: Vec<RangeInclusive<u32>>,
}

/// Positions that aren't a [`RangeSet`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RangeSetError {
    /// Nothing at all.
    Empty,
    /// A run that isn't `n` or `n-m`, with `n` from 1 and `m` from `n`.
    Run(String),
}

impl fmt::Display for RangeSetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RangeSetError::Empty => f.write_str("no pages: positions name at least one"),
            RangeSetError::Run(run) => write!(
                f,
                "{run:?} isn't a page or pages: positions are 1-based, as \"3\" or \"3-5\""
            ),
        }
    }
}

impl std::error::Error for RangeSetError {}

impl RangeSet {
    /// The positions in `runs`, in any order and overlapping or not; a run
    /// is from its start to its end, both included.
    pub fn from_runs(
        runs: impl IntoIterator<Item = RangeInclusive<u32>>,
    ) -> Result<RangeSet, RangeSetError> {
        let mut runs: Vec<RangeInclusive<u32>> = runs.into_iter().collect();
        if let Some(bad) = runs.iter().find(|r| *r.start() == 0 || r.is_empty()) {
            return Err(RangeSetError::Run(format!("{}-{}", bad.start(), bad.end())));
        }
        runs.sort_by_key(|r| *r.start());
        let mut merged: Vec<RangeInclusive<u32>> = Vec::with_capacity(runs.len());
        for r in runs {
            match merged.last_mut() {
                Some(last) if *r.start() <= last.end().saturating_add(1) => {
                    *last = *last.start()..=*last.end().max(r.end());
                }
                _ => merged.push(r),
            }
        }
        match merged.is_empty() {
            true => Err(RangeSetError::Empty),
            false => Ok(RangeSet { runs: merged }),
        }
    }

    /// Its runs, sorted, none touching another.
    pub fn runs(&self) -> &[RangeInclusive<u32>] {
        &self.runs
    }

    /// Every position in it, in order.
    pub fn iter(&self) -> impl Iterator<Item = u32> + '_ {
        self.runs.iter().flat_map(|r| r.clone())
    }

    /// Its last position.
    pub fn last(&self) -> u32 {
        *self.runs[self.runs.len() - 1].end()
    }
}

impl FromStr for RangeSet {
    type Err = RangeSetError;

    /// `"1-3,5"`, spaces allowed around the numbers.
    fn from_str(s: &str) -> Result<RangeSet, RangeSetError> {
        let number = |n: &str| {
            let n = n.trim();
            (!n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
                .then(|| n.parse::<u32>().ok())
                .flatten()
        };
        let run = |run: &str| {
            let (a, b) = run.split_once('-').unwrap_or((run, run));
            let (a, b) = number(a)
                .zip(number(b))
                .filter(|(a, b)| *a >= 1 && a <= b)?;
            Some(a..=b)
        };
        if s.trim().is_empty() {
            return Err(RangeSetError::Empty);
        }
        let runs = s
            .split(',')
            .map(|r| run(r).ok_or_else(|| RangeSetError::Run(r.trim().into())));
        RangeSet::from_runs(runs.collect::<Result<Vec<_>, _>>()?)
    }
}

impl fmt::Display for RangeSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, r) in self.runs.iter().enumerate() {
            if i > 0 {
                f.write_str(",")?;
            }
            match r.start() == r.end() {
                true => write!(f, "{}", r.start())?,
                false => write!(f, "{}-{}", r.start(), r.end())?,
            }
        }
        Ok(())
    }
}

impl Serialize for RangeSet {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for RangeSet {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<RangeSet, D::Error> {
        let s = String::deserialize(d)?;
        s.parse().map_err(de::Error::custom)
    }
}

impl JsonSchema for RangeSet {
    fn schema_name() -> Cow<'static, str> {
        "RangeSet".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "Page positions, 1-based: numbers and ranges, comma-separated, as \"1-3,5\".",
            "type": "string",
            "pattern": "^ *[0-9]+ *(- *[0-9]+ *)?(, *[0-9]+ *(- *[0-9]+ *)?)*$",
        })
    }
}
