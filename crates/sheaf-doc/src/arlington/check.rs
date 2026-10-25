// Parts of this file are adapted from TestGrammar, the Arlington PDF
// Model's checker (pdf-association/arlington-pdf-model, `TestGrammar/src`,
// Copyright 2020-2022 PDF Association, Inc.), under the Apache License,
// Version 2.0: `Walk::score` (from `CParsePDF::recommended_link_for_object`),
// `Walk::array_rows` (from `CParsePDF::parse_object`'s order), `is_date`
// (from `is_valid_pdf_date_string`) and `Class::of` (from
// `is_second_class_pdf_name` and `is_third_class_pdf_name`). Those parts
// stay under Apache-2.0 alone, whichever of sheaf's licenses the rest of
// the file is taken under. Each one's doc says how it differs from
// TestGrammar. The license and NOTICE.txt are in vendor/arlington-pdf-model.

//! The walk: from the trailer as the file writes it, every object it
//! reaches, each checked against the table it's reached as. Links are
//! followed as TestGrammar follows them (upstream's
//! `TestGrammar/src/ParseObjects.cpp`, Apache-2.0): an object reached as
//! an annotation is held to the annotation table its keys fit best, and an
//! object is checked once, as the first table it's reached as (reached
//! again as another, it's counted: [`Report::two_contexts`]). But a
//! `_Universal` table, which fits anything (the model's link where it says
//! only "a dictionary", as for a structure element's `/OBJR /Obj`), comes
//! last: an object reached as one is checked as it only when the walk
//! reaches it as no other table. TestGrammar checks an object as the first
//! table it reaches it as, whichever, so a widget its structure tree
//! reaches before its page is checked against no annotation's table.
//! Only the objects held ([`Hold`]: what sheaf wrote) give findings and
//! counts; the rest are walked so the held ones get their tables.
//!
//! The rules here: a value's type ([`Rule::Type`]); a required key or
//! element ([`Rule::Missing`]), found through `/Parent` for an inheritable
//! key (and a field's `/DA` in the form, its document-wide default); a key
//! from a later version than the file's ([`Rule::TooNew`]); a key, type or
//! value deprecated by the file's version ([`Rule::Deprecated`]); a value
//! its row doesn't list ([`Rule::Value`]); a value direct where it must be
//! an indirect reference, or the reverse ([`Rule::Indirect`],
//! [`Rule::Direct`]); a SpecialCase that comes to false
//! ([`Rule::Special`]); a reference to no object ([`Rule::Unresolved`]); a
//! malformed name or number tree ([`Rule::Tree`]); a held object the
//! walk never reached ([`Rule::Unreached`]); and in an object sheaf built
//! whole, a key the model doesn't list that's no private name
//! ([`Rule::Unlisted`]). A check whose predicate can't be told, or uses a
//! function the evaluator doesn't support, is counted as not made
//! ([`Report::unchecked`]), and every key the model doesn't list is
//! counted with what kind of name it is ([`Report::unlisted`]); in an
//! object merged over a source's, never failed, as TestGrammar reports it
//! as information: it may be the source's.
//!
//! [`Report::beyond`] says which findings are sheaf's to answer for, given
//! the reports of the files it merged objects from, and
//! [`Allowances`](super::Allowances) which of those are known and held for
//! a backlog entry.
//!
//! Every input is walked in bounded work: from a queue, never recursion on
//! the file (the way to an object, as long as the file makes it, is
//! dropped a step at a time too); each indirect object once; direct
//! objects nested at most [`NESTING`] deep in one; `/Parent` chains at most
//! the page tree's depth.

use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::fmt::{self, Write as _};
use std::rc::Rc;

use lopdf::{Dictionary, LazyPdf, Object, ObjectId, ObjectStream, Stream, XrefKind, dictionary};

use super::grammar::{ArlType, At, Row, Table, WrapFn, Wrapped, grammar, version};
use super::predicate::{self, Cx, Gate, Reference, Since, TOLERANCE, Truth};
use crate::file::text_string;
use crate::materialize::emit::PdfVersion;
use crate::private;
use crate::serialize::full::left_out;
use crate::source::Limits;

/// How deeply direct objects may nest in an indirect one before the walk
/// stops going in. lopdf's parser nests them up to 100 deep, keeping its
/// cap to itself; a real file nests a few.
pub const NESTING: usize = 64;

/// How many of the objects on the walk's way to one `parent::` may reach
/// back through: the model's deepest is `parent::parent::parent::`.
const PARENTS: usize = 8;

/// How many steps a finding's path shows: its last ones.
const PATH: usize = 24;

/// How many characters of a finding's value its display shows.
const SHOWN: usize = 80;

/// How many bytes of a name or string a finding's value shows.
const TOKEN: usize = 80;

/// How sheaf made an object it wrote.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Made {
    /// Built whole: every key in it is sheaf's.
    Built,
    /// Emitted over a source's dictionary, whose keys it may keep.
    Merged,
}

/// The objects held to the grammar: what sheaf wrote. The trailer as
/// written is always held, as [`Made::Merged`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Hold {
    /// Every object in the file.
    All(Made),
    /// These.
    Only(BTreeMap<ObjectId, Made>),
}

/// What a finding breaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Rule {
    /// A value of none of the types its row allows at the file's version.
    Type,
    /// A required key or element that's absent, and for an inheritable
    /// key, on no `/Parent` either.
    Missing,
    /// A key or element whose SinceVersion is later than the file's
    /// version.
    TooNew,
    /// A key or element whose DeprecatedIn is the file's version or
    /// earlier; or a type or value its row allows at the file's version
    /// only as deprecated (`fn:Deprecated`).
    Deprecated,
    /// A value none of its row's PossibleValues for its type is, of those
    /// the file's version lets in.
    Value,
    /// A direct value where its row's IndirectReference asks for an
    /// indirect reference.
    Indirect,
    /// An indirect reference where its row's IndirectReference asks for a
    /// direct value.
    Direct,
    /// A value its row's SpecialCase comes to false for.
    Special,
    /// A held object the walk never reached, and not under a key the
    /// model doesn't list.
    Unreached,
    /// A reference to an object the file hasn't, or a trailer the file's
    /// newest cross-reference section doesn't give.
    Unresolved,
    /// A name or number tree's node that breaks ISO 32000-1 7.9.6 or
    /// 7.9.7.
    Tree,
    /// A key or element of a [`Made::Built`] object that the model
    /// doesn't list ([`Unlisted`]), and isn't a second- or third-class
    /// name nor one of the private keys sheaf writes
    /// ([`private::KEYS`](crate::private::KEYS)), in a table that says
    /// what the object is (not a `_Universal` one, nor the plain `Stream`
    /// one such a table links a stream to): sheaf builds the object whole,
    /// so it's a key PDF doesn't define, misspelled or misplaced.
    Unlisted,
}

/// A rule a held object breaks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// The indirect object it's in: none for a table's trailer.
    pub object: Option<ObjectId>,
    pub made: Made,
    /// Where the walk reached it from the trailer:
    /// `Root/Pages/Kids[0]/Annots[2]`, `trailer` for the trailer, `-` for
    /// an object it never reached.
    pub path: String,
    /// The table and key of the row it breaks; for an unreached object,
    /// those of the object that references it, `-` for none.
    pub table: String,
    pub key: String,
    pub rule: Rule,
    /// The value as written (its first entries, names and strings cut
    /// short), and what's wrong with it; its display shows the first 80
    /// characters (`SHOWN`).
    pub value: String,
}

/// A key or element of held objects that the model doesn't list: no row
/// has it, and its table has no wildcard.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Unlisted {
    pub table: String,
    pub key: String,
    pub class: Class,
}

/// What kind of name a key the model doesn't list is (ISO 32000-2 Annex
/// E), as TestGrammar tells (`is_second_class_pdf_name` and
/// `is_third_class_pdf_name`, `TestGrammar/src/Utils.cpp`).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Class {
    /// A second-class name: a registered prefix of four or five letters,
    /// digits, `_` or `-`, then `_` or `:` (`ABCD_x`).
    Second,
    /// A third-class name, free for anyone's use: `XX` first.
    Third,
    /// A key the model has under this extension, which no file sheaf
    /// writes claims.
    Extension(String),
    /// Any other: a first-class name the model doesn't define, an array's
    /// element past its table's rows, or a writer's own key.
    Other,
}

/// A check a predicate that can't be told stood in the way of.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Unchecked {
    /// The column whose cell it is.
    pub column: &'static str,
    /// What's in the way ([`Truth::Unknown`]'s): a function
    /// (`fn:IsMeaningful`), arithmetic (`+`), or a value that can't be read
    /// (`@Type`).
    pub why: String,
    pub table: String,
    pub key: String,
}

/// What a check of a file found in its held objects.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Report {
    pub findings: Vec<Finding>,
    /// Held objects checked, by table; tree nodes as `name-tree` and
    /// `number-tree`.
    pub checked: BTreeMap<String, usize>,
    /// Keys and elements of held objects checked against their rows.
    pub keys: usize,
    /// Checks not made, for a predicate that can't be told.
    pub unchecked: BTreeMap<Unchecked, usize>,
    /// Keys and elements of held objects the model doesn't list.
    pub unlisted: BTreeMap<Unlisted, usize>,
    /// Held objects reached through a type with no table to check them as.
    pub no_table: usize,
    /// Held objects two tables fit best, the first taken.
    pub ties: usize,
    /// Direct objects in held ones nested past [`NESTING`], not walked.
    pub truncated: usize,
    /// Held objects reached again as another table, kept as the first.
    pub two_contexts: usize,
    /// The cross-reference stream the trailer is, when it's one: a finding
    /// in the trailer is in it, or in no object.
    pub trailer: Option<ObjectId>,
}

/// The objects in `pdf`, whose bytes are `bytes`, that `hold` names, held
/// to the model: each reached from the trailer as the file's newest
/// cross-reference section writes it, at the file's version (its
/// header's, or its catalog's `/Version` when later: [`file_version`]). A
/// trailer that doesn't read so is a finding, and the walk starts from
/// what's there instead: a cross-reference stream the sections don't list,
/// read from the bytes at `startxref`, or else the trailer the reader made
/// of the file ([`LazyPdf::trailer`]), as readers see it. A source's
/// objects have their faults whatever its trailer's.
pub fn check(pdf: &LazyPdf, bytes: &[u8], hold: &Hold) -> Report {
    check_at(pdf, bytes, hold, file_version(pdf))
}

/// [`check`] at `version`, not the file's: a file sheaf emitted objects
/// over, checked at the version of the file it wrote them in, has the
/// faults its objects have there ([`Report::beyond`]), as a key that came
/// in at a version between the two, or one a key then requires.
pub fn check_at(pdf: &LazyPdf, bytes: &[u8], hold: &Hold, version: PdfVersion) -> Report {
    let mut unread = None;
    let (id, object) = match written_trailer(pdf, bytes) {
        Ok(written) => written,
        Err(why) => {
            unread = Some(Finding {
                object: None,
                made: Made::Merged,
                path: "trailer".to_string(),
                table: "FileTrailer".to_string(),
                key: "-".to_string(),
                rule: Rule::Unresolved,
                value: why,
            });
            match unlisted_stream(pdf, bytes) {
                Some((id, stream)) => (Some(id), Cow::Owned(stream)),
                None => (None, Cow::Owned(Object::Dictionary(pdf.trailer().clone()))),
            }
        }
    };
    let object: &Object = &object;
    let (trailer, name) = match object {
        Object::Stream(s) => (&s.dict, "XRefStream"),
        Object::Dictionary(d) => (d, "FileTrailer"),
        _ => unreachable!("written_trailer gives a dictionary or a stream"),
    };
    let table = grammar()
        .table(name)
        .expect("the model has the trailers' tables");
    let mut walk = Walk {
        pdf,
        bytes,
        trailer,
        trailer_id: id,
        version,
        hold,
        queue: VecDeque::new(),
        later: VecDeque::new(),
        waiting: HashSet::new(),
        reached: HashMap::new(),
        referrers: HashMap::new(),
        under_unlisted: HashSet::new(),
        report: Report {
            findings: unread.into_iter().collect(),
            trailer: id,
            ..Report::default()
        },
    };
    if let Some(id) = id {
        walk.reached.insert(id, Some(table.name.as_str()));
    }
    let trail = Rc::new(Trail {
        up: None,
        step: Step::Trailer,
        object,
    });
    walk.queue.push_back(Visit {
        object,
        holder: id,
        table,
        trail,
        depth: 0,
        loose: false,
    });
    loop {
        while let Some(visit) = walk.queue.pop_front() {
            walk.visit(visit);
        }
        let Some(visit) = walk.later.pop_front() else {
            break;
        };
        if visit.holder.is_some_and(|id| walk.waiting.remove(&id)) {
            walk.visit(visit);
        }
    }
    walk.unreached();
    walk.report
}

impl Report {
    /// The findings sheaf answers for, given `sources`, the reports of
    /// the files it emitted objects over: every finding in a
    /// [`Made::Built`] object, and those in [`Made::Merged`] objects and
    /// the trailer that no source has one like, of the same table, key,
    /// rule and value, the value's references' numbers aside, which a
    /// rewrite renumbers; for a key that came in later than the file's
    /// version, any value, as its value isn't what's wrong. A merged object
    /// keeps the keys of the source dictionary it's emitted over (the edit
    /// API's plan, §11.3: a page "keeps unknown keys"), faults and all, and
    /// a fault the source has is the file's, carried; a built object is
    /// sheaf's whole, and never excused. The sources are best checked at
    /// this file's version ([`check_at`]). A like finding in any object of
    /// a source carries it, not only in the one the merged object was
    /// emitted over, which no [`Hold`] records (backlog B46): a fault sheaf
    /// writes into a merged object is missed when some object of the same
    /// table in a source has it.
    ///
    /// The trailer is written anew by every save, in the file's form: its
    /// `/Size`, `/Prev`, `/Index` and `/W` are the save's own. A key too new
    /// for the file's version in it is carried when a source's trailer has
    /// one too: what's too new is the form, as a cross-reference stream
    /// below PDF 1.5 is, which an Append goes on writing.
    pub fn beyond<'a>(&'a self, sources: &[&Report]) -> Vec<&'a Finding> {
        let like = |f: &Finding| {
            let value = match f.rule {
                Rule::TooNew => String::new(),
                _ => numberless(&f.value),
            };
            (f.table.clone(), f.key.clone(), f.rule, value)
        };
        let carried: HashSet<_> = (sources.iter())
            .flat_map(|source| &source.findings)
            .map(like)
            .collect();
        let too_new = |report: &Report, f: &Finding| f.rule == Rule::TooNew && report.in_trailer(f);
        let form = (sources.iter()).any(|s| s.findings.iter().any(|f| too_new(s, f)));
        (self.findings.iter())
            .filter(|f| {
                f.made == Made::Built || !(carried.contains(&like(f)) || (form && too_new(self, f)))
            })
            .collect()
    }

    /// Whether `finding` is in the trailer as written: in its
    /// cross-reference stream, or in no object.
    fn in_trailer(&self, finding: &Finding) -> bool {
        finding.object.is_none() || finding.object == self.trailer
    }
}

/// The trailer as the file's newest cross-reference section writes it:
/// its cross-reference stream, with the stream's number, or the
/// dictionary after its table's `trailer`. [`LazyPdf::trailer`] isn't:
/// it has no `/Prev` or `/XRefStm`.
fn written_trailer<'a>(
    pdf: &'a LazyPdf,
    bytes: &[u8],
) -> Result<(Option<ObjectId>, Cow<'a, Object>), String> {
    if !pdf.base_sound() {
        return Err("the cross-reference sections don't read".to_string());
    }
    let facts = pdf.xref();
    let section = (bytes.get(facts.header_offset + facts.start..))
        .ok_or("startxref is past the end of the file")?;
    match facts.kind {
        XrefKind::Stream => {
            let id = object_header(section).ok_or("no object at startxref")?;
            match pdf.get(id) {
                Ok(stream @ Object::Stream(_)) => Ok((Some(id), Cow::Borrowed(stream))),
                Ok(_) => Err(format!("{} {} R, at startxref, isn't a stream", id.0, id.1)),
                Err(e) => Err(format!("{} {} R, at startxref: {e}", id.0, id.1)),
            }
        }
        XrefKind::Table => {
            let at = (section.windows(7).position(|w| w == b"trailer"))
                .ok_or("no trailer after the table")?;
            let dict = parse_dictionary(&section[at + 7..]).ok_or("the trailer doesn't parse")?;
            Ok((None, Cow::Owned(Object::Dictionary(dict))))
        }
    }
}

/// The cross-reference stream at `startxref` when the file's sections,
/// which read, don't list it, as some writers leave theirs out (an object
/// past their own `/Size`): its number, and its dictionary read from the
/// bytes. None when the newest section isn't a stream, or doesn't parse.
fn unlisted_stream(pdf: &LazyPdf, bytes: &[u8]) -> Option<(ObjectId, Object)> {
    let facts = pdf.xref();
    if !pdf.base_sound() || !matches!(facts.kind, XrefKind::Stream) {
        return None;
    }
    let at = facts.header_offset + facts.start;
    let section = bytes.get(at..)?;
    let id = object_header(section)?;
    let body = section.windows(3).position(|w| w == b"obj")? + 3;
    let dict = parse_dictionary(&section[body..])?;
    let xref = dict.get(b"Type").and_then(Object::as_name).ok() == Some(b"XRef");
    xref.then(|| (id, Object::Stream(Stream::with_position(dict, at))))
}

/// The object an indirect object's header at the start of `bytes`
/// (`12 0 obj`, after white-space) names.
pub(super) fn object_header(bytes: &[u8]) -> Option<ObjectId> {
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(64)]);
    let mut words = head.split_ascii_whitespace();
    let number = words.next()?.parse().ok()?;
    let generation = words.next()?.parse().ok()?;
    words
        .next()?
        .starts_with("obj")
        .then_some((number, generation))
}

/// The dictionary at the start of `bytes`, after white-space, as lopdf's
/// parser reads it. lopdf keeps its parser to itself, and an object
/// stream is how its public API reads an object from bytes: the bytes go
/// in one, as its only object.
fn parse_dictionary(bytes: &[u8]) -> Option<Dictionary> {
    let mut content = b"0 0 ".to_vec();
    content.extend_from_slice(bytes);
    let stream = Stream::new(dictionary! { "N" => 1, "First" => 4 }, content);
    match ObjectStream::new(&stream).ok()?.objects.remove(&(0, 0))? {
        Object::Dictionary(dict) => Some(dict),
        _ => None,
    }
}

/// The file's version: its header's, or its catalog's `/Version` when
/// that's later, as [`PdfVersion::of`] reads a source's.
pub fn file_version(pdf: &LazyPdf) -> PdfVersion {
    let header = PdfVersion::parse(pdf.version().as_bytes()).unwrap_or(PdfVersion::new(1, 4));
    let catalog = (pdf.catalog().ok())
        .and_then(|c| c.get(b"Version").ok())
        .and_then(|v| v.as_name().ok())
        .and_then(PdfVersion::parse);
    header.max(catalog.unwrap_or(header))
}

/// A walk of one file.
struct Walk<'a> {
    pdf: &'a LazyPdf,
    /// The file's bytes.
    bytes: &'a [u8],
    /// The trailer as written (`trailer::`).
    trailer: &'a Dictionary,
    /// The cross-reference stream the trailer is, when it is one.
    trailer_id: Option<ObjectId>,
    version: PdfVersion,
    hold: &'a Hold,
    queue: VecDeque<Visit<'a>>,
    /// Indirect objects reached as a `_Universal` table, which fits
    /// anything, to visit as that once the queue is empty, unless one is
    /// reached as another table first; and which of them haven't been.
    later: VecDeque<Visit<'a>>,
    waiting: HashSet<ObjectId>,
    /// Each indirect object reached, with the table it was given; none for
    /// one with no table, a value of a simple type.
    reached: HashMap<ObjectId, Option<&'static str>>,
    /// The table and key each object was first seen referenced under in an
    /// object walked.
    referrers: HashMap<ObjectId, (&'static str, String)>,
    /// The objects referenced under keys the model doesn't list.
    under_unlisted: HashSet<ObjectId>,
    report: Report,
}

/// A dictionary, stream or array to check, as a table.
struct Visit<'a> {
    object: &'a Object,
    /// The indirect object it is, or is in: none for a table's trailer.
    holder: Option<ObjectId>,
    table: &'static Table,
    trail: Rc<Trail<'a>>,
    /// How deeply it's nested in its holder, direct.
    depth: usize,
    /// Whether its table says only what kind of object it is: a
    /// `_Universal` one, or the plain `Stream` one a `_Universal` one links
    /// a stream to. No key is one such a table doesn't list.
    loose: bool,
}

/// The walk's way to an object: each step's object, and how it was
/// reached from the one before.
struct Trail<'a> {
    up: Option<Rc<Trail<'a>>>,
    step: Step<'a>,
    object: &'a Object,
}

impl Drop for Trail<'_> {
    /// Unlinks the way back a step at a time, as far as no other trail
    /// shares it: dropped whole, a way as long as the file makes it (an
    /// outline's `/Next` siblings, a thread's beads) would drop
    /// recursively, a step deeper each, and overflow the stack.
    fn drop(&mut self) {
        let mut up = self.up.take();
        while let Some(trail) = up {
            up = match Rc::try_unwrap(trail) {
                Ok(mut trail) => trail.up.take(),
                Err(_) => None,
            };
        }
    }
}

/// How an object was reached from the one before it.
#[derive(Clone)]
enum Step<'a> {
    Trailer,
    Key(&'a [u8]),
    Index(usize),
    /// A name or number tree's entry.
    Entry(String),
}

/// A value's type among its row's.
enum Typed {
    /// The type its row's `i`th is; and when the row allows it at the
    /// file's version only as deprecated, from which version.
    Is(usize, Option<PdfVersion>),
    /// Of a type its row allows only at other versions, or under an
    /// extension.
    Excluded(ArlType),
    Not,
}

/// What a value's row's PossibleValues for its type make of it.
enum Values {
    /// One of them; or any, none being listed.
    Allowed,
    /// One of them only as deprecated (`fn:Deprecated`), from this
    /// version.
    Deprecated(PdfVersion),
    /// None of them: those the file's version lets in, as the model
    /// writes them.
    Not(Vec<&'static str>),
    /// It can't be told, for one of them that's a predicate that can't be
    /// ([`Truth::Unknown`]'s).
    Unknown(String),
}

impl<'a> Walk<'a> {
    /// How the object `holder` was made, when it's held: the trailer as
    /// [`Made::Merged`].
    fn made(&self, holder: Option<ObjectId>) -> Option<Made> {
        match holder {
            None => Some(Made::Merged),
            Some(id) if Some(id) == self.trailer_id => Some(Made::Merged),
            Some(id) => match self.hold {
                Hold::All(made) => Some(*made),
                Hold::Only(objects) => objects.get(&id).copied(),
            },
        }
    }

    /// Checks `v`'s keys or elements, and when it's held, that it has those
    /// its table requires.
    fn visit(&mut self, v: Visit<'a>) {
        let made = self.made(v.holder);
        if made.is_some() {
            *self.report.checked.entry(v.table.name.clone()).or_default() += 1;
        }
        let parents = v.trail.parents();
        match v.object {
            Object::Dictionary(dict) | Object::Stream(Stream { dict, .. }) => {
                for (key, value) in dict.iter() {
                    let row = v.table.row(At::Key(key));
                    self.entry(&v, At::Key(key), row, value, made, &parents);
                }
                if let Some(made) = made {
                    self.required_keys(&v, dict, made, &parents);
                }
            }
            Object::Array(array) => {
                let rows = self.array_rows(v.table, array);
                for (i, (value, row)) in array.iter().zip(rows).enumerate() {
                    self.entry(&v, At::Index(i), row, value, made, &parents);
                }
                if let Some(made) = made {
                    self.required_elements(&v, array.len(), made, &parents);
                }
            }
            _ => {}
        }
    }

    /// The rows `array`'s elements fall on in `table`, as TestGrammar's
    /// walk assigns them (`CParsePDF::parse_object`, ParseObjects.cpp): in
    /// order, round and round from the first row with a `*` (a repeating
    /// set's, or the wildcard); and where some rows aren't required, an
    /// element at or past the first of those that isn't of its type starts
    /// again from the first row with a `*`. None for an element past every
    /// row, which TestGrammar notes and checks no further.
    fn array_rows(&self, table: &'static Table, array: &[Object]) -> Vec<Option<&'static Row>> {
        let rows = &table.rows;
        let wildcard = rows
            .last()
            .filter(|row| row.key == "*")
            .map(|_| rows.len() - 1);
        let repeat = rows.iter().position(|row| row.key.contains('*'));
        let optional = rows.iter().position(|row| row.required != "TRUE");
        let mut next = 0;
        let mut assigned = Vec::with_capacity(array.len());
        for (i, element) in array.iter().enumerate() {
            let at = match (wildcard, repeat, optional) {
                (Some(wildcard), _, _) if i >= wildcard => wildcard,
                (_, Some(repeat), None) if next >= repeat && next < rows.len() => next,
                (_, Some(repeat), None) if next >= repeat => repeat,
                (_, Some(repeat), Some(optional)) if next >= optional => {
                    let fits = match &rows[optional].r#type[..] {
                        [ty] if ty.wraps.is_empty() => (self.resolve(element))
                            .is_ok_and(|(_, value)| self.matches(value, ty.item)),
                        _ => false,
                    };
                    if fits { next } else { repeat }
                }
                _ => next,
            };
            next = at + 1;
            assigned.push(rows.get(at));
        }
        assigned
    }

    /// The entry `raw` at `at` in `v`'s object, on `row` of its table:
    /// checked against it, and followed to the tables it links to.
    fn entry(
        &mut self,
        v: &Visit<'a>,
        at: At<'a>,
        row: Option<&'static Row>,
        raw: &'a Object,
        made: Option<Made>,
        parents: &[&'a Object],
    ) {
        let key = at_text(at);
        let step = match at {
            At::Key(key) => Step::Key(key),
            At::Index(i) => Step::Index(i),
        };
        let (id, value) = match self.resolve(raw) {
            Ok(found) => found,
            Err(gone) => {
                if let Some(made) = made {
                    let value = format!("{} {} R", gone.0, gone.1);
                    self.find(v, made, Rule::Unresolved, &v.table.name, &key, value);
                }
                return;
            }
        };
        // An extension's row is no row (TestGrammar without `--extensions`):
        // the key is checked as the table's wildcard, if it has one.
        let mut extension = None;
        let row = match row.map(|row| (row, predicate::since(&row.since_version))) {
            Some((_, Since::Extension(name))) => {
                extension = Some(name);
                (v.table.row(At::Key(b"*"))).filter(|row| {
                    row.key == "*"
                        && !matches!(predicate::since(&row.since_version), Since::Extension(_))
                })
            }
            row => row.map(|(row, _)| row),
        };
        let Some(row) = row else {
            // Metadata streams may be anywhere from PDF 1.4 (ISO 32000-1
            // 14.3.2), and associated files from 2.0 (ISO 32000-2 14.13),
            // which TestGrammar checks as such where the model has no row.
            let table = match (at, value) {
                (At::Key(b"Metadata"), Object::Stream(_)) => "Metadata",
                (At::Key(b"AF"), Object::Array(_)) => "ArrayOfFileSpecifications",
                _ => {
                    if let Some(made) = made {
                        let class = match (extension, at) {
                            (Some(name), _) => Class::Extension(name),
                            (None, At::Key(key)) => Class::of(key),
                            (None, At::Index(_)) => Class::Other,
                        };
                        let own = matches!(at, At::Key(key) if sheaf_s_own(key));
                        if made == Made::Built
                            && matches!(class, Class::Other | Class::Extension(_))
                            && !own
                            && !v.loose
                        {
                            let value = render(value);
                            self.find(v, made, Rule::Unlisted, &v.table.name, &key, value);
                        }
                        let unlisted = Unlisted {
                            table: v.table.name.clone(),
                            key,
                            class,
                        };
                        *self.report.unlisted.entry(unlisted).or_default() += 1;
                    }
                    each_reference(raw, |to| {
                        self.under_unlisted.insert(to);
                    });
                    return;
                }
            };
            if made.is_some() {
                self.report.keys += 1;
            }
            self.refer(raw, v.table, key);
            let table = grammar().table(table).expect("the model has the table");
            self.reach(v, step, (id, value), &[table], parents);
            return;
        };
        self.refer(raw, v.table, key.clone());
        // A dictionary's null entry is no entry (ISO 32000-1 7.3.7), unless
        // its row allows null.
        if matches!((at, value), (At::Key(_), Object::Null))
            && !row.r#type.iter().any(|t| t.item == ArlType::Null)
        {
            return;
        }
        if let Some(made) = made {
            self.report.keys += 1;
            self.versions(v, made, row, &key, value);
        }
        match (self.typed(row, value), made) {
            (Typed::Is(i, deprecated), Some(made)) => {
                if let Some(from) = deprecated {
                    let ty = row.r#type[i].item.name();
                    let value = format!("{} ({ty} deprecated in {from})", render(value));
                    self.find(v, made, Rule::Deprecated, &v.table.name, &key, value);
                }
                self.rules(v, at, made, row, i, (raw, value), parents);
                self.follow(v, step, (id, value), row, i, parents);
            }
            (Typed::Is(i, _), None) => self.follow(v, step, (id, value), row, i, parents),
            (Typed::Excluded(ty), Some(made)) => {
                let value = format!(
                    "{} ({} isn't allowed at {})",
                    render(value),
                    ty.name(),
                    self.version
                );
                self.find(v, made, Rule::Type, &v.table.name, &key, value);
            }
            (Typed::Not, Some(made)) => {
                self.find(v, made, Rule::Type, &v.table.name, &key, render(value));
            }
            (_, None) => {}
        }
    }

    /// Whether `row`'s key or element, with `value` in a held object `v`,
    /// came in by the file's version (its SinceVersion; an extension's row
    /// never gets here) and isn't deprecated by it (its DeprecatedIn), as
    /// TestGrammar's `ArlVersion` reads the two (ArlVersion.cpp).
    fn versions(&mut self, v: &Visit, made: Made, row: &Row, key: &str, value: &Object) {
        match predicate::since(&row.since_version) {
            Since::In(since) if since > self.version => {
                let value = format!("{} (since {since})", render(value));
                self.find(v, made, Rule::TooNew, &v.table.name, key, value);
            }
            Since::In(_) | Since::Extension(_) => {}
            Since::Unknown(why) => self.unchecked("SinceVersion", why, &v.table.name, key),
        }
        if let Some(from) = version(&row.deprecated_in)
            && from <= self.version
        {
            let value = format!("{} (deprecated in {from})", render(value));
            self.find(v, made, Rule::Deprecated, &v.table.name, key, value);
        }
    }

    /// The rules on `raw` (`value`, its references followed) at `at` in
    /// the held object `v`, of its row's `i`th type: what its
    /// IndirectReference, PossibleValues and SpecialCase ask (TestGrammar's
    /// `check_everything`, ParseObjects.cpp, which asks only for indirect
    /// references; here direct values too).
    #[allow(clippy::too_many_arguments)]
    fn rules(
        &mut self,
        v: &Visit<'a>,
        at: At<'a>,
        made: Made,
        row: &'static Row,
        i: usize,
        (raw, value): (&'a Object, &'a Object),
        parents: &[&'a Object],
    ) {
        let key = at_text(at);
        let table = &v.table.name;
        // A tree's IndirectReference is its values' (INTERNAL_GRAMMAR.md,
        // "Column 6 - IndirectReference"), which the model asks of none
        // but under an extension (a test says so): unread.
        if !matches!(row.r#type[i].item, ArlType::NameTree | ArlType::NumberTree) {
            let asked = self.with_cx(v.object, at, parents, |cx| {
                predicate::reference(&row.indirect_reference[i], cx)
            });
            let referenced = match (v.object, at, &asked) {
                // lopdf reads a referenced length into the stream's
                // dictionary: whether it was one is in the bytes.
                (
                    Object::Stream(_),
                    At::Key(b"Length"),
                    Reference::Direct | Reference::Indirect,
                ) => {
                    let written = (v.holder.filter(|_| v.depth == 0))
                        .and_then(|id| self.written_dictionary(id));
                    match written {
                        Some(dict) => Some(matches!(dict.get(b"Length"), Ok(Object::Reference(_)))),
                        None => {
                            let why = "the stream's dictionary as written".to_string();
                            self.unchecked("IndirectReference", why, table, &key);
                            None
                        }
                    }
                }
                _ => Some(matches!(raw, Object::Reference(_))),
            };
            match (asked, referenced) {
                (Reference::Indirect, Some(false)) if !matches!(value, Object::Null) => {
                    self.find(v, made, Rule::Indirect, table, &key, render(value));
                }
                (Reference::Direct, Some(true)) => {
                    self.find(v, made, Rule::Direct, table, &key, render(raw));
                }
                (Reference::Unknown(why), _) => {
                    self.unchecked("IndirectReference", why, table, &key)
                }
                _ => {}
            }
        }
        match self.with_cx(v.object, at, parents, |cx| self.values(row, i, value, cx)) {
            Values::Allowed => {}
            Values::Deprecated(from) => {
                let value = format!("{} (deprecated in {from})", render(value));
                self.find(v, made, Rule::Deprecated, table, &key, value);
            }
            Values::Not(listed) => {
                let value = format!("{} (not one of {})", render(value), listed.join(", "));
                self.find(v, made, Rule::Value, table, &key, value);
            }
            Values::Unknown(why) => self.unchecked("PossibleValues", why, table, &key),
        }
        let special = &row.special_case[i];
        if !special.is_empty() {
            match self.with_cx(v.object, at, parents, |cx| {
                predicate::holds_fully(special, cx)
            }) {
                Truth::True => {}
                Truth::False => {
                    let value = format!("{} ({special})", render(value));
                    self.find(v, made, Rule::Special, table, &key, value);
                }
                Truth::Unknown(why) => self.unchecked("SpecialCase", why, table, &key),
            }
        }
    }

    /// Notes that `raw`, when it's a reference, was seen under `key` of
    /// an object walked as `table`.
    fn refer(&mut self, raw: &Object, table: &'static Table, key: String) {
        if let Object::Reference(to) = raw {
            (self.referrers.entry(*to)).or_insert_with(|| (table.name.as_str(), key));
        }
    }

    /// Goes on from `found`, the entry at `step` of `v`'s object (and the
    /// number of the object it's in), of its row's `i`th type: to the
    /// table of its links it fits best, or through its tree.
    fn follow(
        &mut self,
        v: &Visit<'a>,
        step: Step<'a>,
        found: (Option<ObjectId>, &'a Object),
        row: &'static Row,
        i: usize,
        parents: &[&'a Object],
    ) {
        match row.r#type[i].item {
            ArlType::NameTree | ArlType::NumberTree => self.tree(v, step, found, row, i, parents),
            ArlType::Array | ArlType::Dictionary | ArlType::Stream => {
                let links = self.links(&row.link[i]);
                self.reach(v, step, found, &links, parents)
            }
            _ => {
                if let (Some(id), _) = found {
                    self.reached.entry(id).or_insert(None);
                }
            }
        }
    }

    /// The tables of a type's links that their version predicates let in
    /// at the file's version; all of them when those let in none, as
    /// TestGrammar's walk always takes all (its `get_full_linkset`).
    fn links(&self, links: &'static [Wrapped<String>]) -> Vec<&'static Table> {
        let table = |l: &Wrapped<String>| {
            grammar()
                .table(&l.item)
                .expect("the loader checks that every link names a table")
        };
        let allowed: Vec<&Table> = (links.iter())
            .filter(|l| predicate::gate(&l.wraps, self.version) != Gate::Excluded)
            .map(table)
            .collect();
        if allowed.is_empty() {
            links.iter().map(table).collect()
        } else {
            allowed
        }
    }

    /// Goes on to `found`, a dictionary, stream or array at `step` from
    /// `from`'s object (with its number, when it's an indirect object), as
    /// the table of `links` it fits best: queued, unless it was reached
    /// already or is nested past [`NESTING`]. An indirect object reached as
    /// a `_Universal` table waits for the queue to empty, and reached as
    /// another table meanwhile, is queued as that.
    fn reach(
        &mut self,
        from: &Visit<'a>,
        step: Step<'a>,
        (id, value): (Option<ObjectId>, &'a Object),
        links: &[&'static Table],
        parents: &[&'a Object],
    ) {
        let held = self.made(id.or(from.holder)).is_some();
        let first = id.and_then(|id| self.reached.get(&id).copied().flatten());
        let waiting = id.is_some_and(|id| self.waiting.contains(&id));
        if first.is_some() && !held && !waiting {
            return;
        }
        let mut up = Vec::with_capacity(PARENTS);
        up.push(from.object);
        up.extend(parents.iter().take(PARENTS - 1));
        let Some((table, tie)) = self.choose(value, links, &up) else {
            if held && id.is_none_or(|id| !self.reached.contains_key(&id)) {
                self.report.no_table += 1;
            }
            if let Some(id) = id {
                self.reached.entry(id).or_insert(None);
            }
            return;
        };
        if let Some(first) = first {
            // Reached as a `_Universal` table and not visited yet, it's this
            // one's instead. TestGrammar visits it as the first table, and
            // doesn't count `_Universal` ones, which fit anything.
            let instead = waiting && !universal(&table.name);
            if !instead {
                if first != table.name && !universal(first) && !universal(&table.name) {
                    self.report.two_contexts += 1;
                }
                return;
            }
            if let Some(id) = id {
                self.waiting.remove(&id);
            }
        }
        if tie && held {
            self.report.ties += 1;
        }
        let trail = Rc::new(Trail {
            up: Some(from.trail.clone()),
            step,
            object: value,
        });
        let loose =
            universal(&table.name) || (universal(&from.table.name) && table.name == "Stream");
        let visit = match id {
            Some(id) => {
                self.reached.insert(id, Some(table.name.as_str()));
                Visit {
                    object: value,
                    holder: Some(id),
                    table,
                    trail,
                    depth: 0,
                    loose,
                }
            }
            None if from.depth < NESTING => Visit {
                object: value,
                holder: from.holder,
                table,
                trail,
                depth: from.depth + 1,
                loose,
            },
            None => {
                if held {
                    self.report.truncated += 1;
                }
                return;
            }
        };
        match id {
            Some(id) if universal(&table.name) => {
                self.waiting.insert(id);
                self.later.push_back(visit);
            }
            _ => self.queue.push_back(visit),
        }
    }

    /// The table of `links` that `object` fits best, and whether another
    /// fit as well, the first of them taken; none when there are none.
    /// TestGrammar's `recommended_link_for_object` (ParseObjects.cpp),
    /// ported: see [`Walk::score`].
    fn choose(
        &self,
        object: &'a Object,
        links: &[&'static Table],
        parents: &[&'a Object],
    ) -> Option<(&'static Table, bool)> {
        // Where TestGrammar scores an array against a dictionary's table
        // too, and one as long as the table has rows took it (a structure
        // parent tree's array of 19, a structure element's 19 rows), only
        // the tables of the object's kind are scored, when there are any.
        let fitting: Vec<&'static Table> = (links.iter().copied())
            .filter(|table| fits(object, table))
            .collect();
        let links = if fitting.is_empty() { links } else { &fitting };
        if let [only] = links {
            return Some((only, false));
        }
        let mut best: Option<(&'static Table, i32)> = None;
        let mut tie = false;
        for &table in links {
            let score = self.score(object, table, parents);
            match best {
                Some((_, least)) if score > least => {}
                Some((_, least)) if score == least => tie = true,
                _ => {
                    best = Some((table, score));
                    tie = false;
                }
            }
        }
        best.map(|(table, _)| (table, tie))
    }

    /// How well `object` fits `table`, lower better (like golf): ported
    /// from `CParsePDF::recommended_link_for_object` in TestGrammar's
    /// `src/ParseObjects.cpp`, Copyright 2020-2022 PDF Association, Inc.,
    /// Apache-2.0 (the vendored model's NOTICE.txt), its weights kept. Each
    /// row's entry scores: a disambiguating key's (`Type`, `Subtype`, `S`,
    /// `Parent`, `TransformMethod`) value of an allowed type and value -80,
    /// an array's first element's -60 required or -20, another's -10
    /// required or -4; a value it doesn't allow +10, +7 or +5, a type it
    /// doesn't +20, +3 or +1; a deprecated key's +8 more. All required
    /// entries good, -8 for each entry found; and -10 times the share of
    /// rows found. An array as long as the table has rows -20.
    ///
    /// Where its code departs from what its comments say, this follows
    /// the comments: the array bonus goes to tables without repeating or
    /// wildcard rows ("but not for wildcards or repeating sets", where its
    /// code tests that the last row has a `*`); and a key is deprecated
    /// when the file's version is at or past its DeprecatedIn (its
    /// `IsDeprecated` tests the reverse). A required entry that's absent
    /// scores nothing, as in its code, which works out whether an entry is
    /// required only for entries present and so never reaches the +12 its
    /// comment gives one: a key required and inherited (a field's `/DA`,
    /// from the form's) is absent from the right table's object. And a
    /// value it allows meets its row's SpecialCase too, where TestGrammar
    /// reads PossibleValues alone: button fields differ only in their
    /// `/Ff` bits, which the model checks in SpecialCase, and would tie,
    /// the push button taken. Both are read as the rules read them: what
    /// can't be told counts as allowed.
    fn score(&self, object: &'a Object, table: &'static Table, parents: &[&'a Object]) -> i32 {
        let rows = &table.rows;
        let (dict, array) = match object {
            Object::Dictionary(d) | Object::Stream(Stream { dict: d, .. }) => (Some(d), None),
            Object::Array(a) => (None, Some(a)),
            _ => return 0,
        };
        let mut score = 0;
        if let Some(array) = array
            && array.len() == rows.len()
            && rows.last().is_some_and(|row| !row.key.contains('*'))
        {
            score -= 20;
        }
        let mut found = 0;
        let mut bad = false;
        for row in rows {
            let (at, entry) = match (dict, array) {
                (Some(dict), _) => {
                    let key = row.key.as_bytes();
                    (At::Key(key), dict.get(key).ok())
                }
                (_, Some(array)) => match index(&row.key) {
                    Some(i) => (At::Index(i), array.get(i)),
                    None => continue,
                },
                (None, None) => return 0,
            };
            let entry = (entry.and_then(|e| self.resolve(e).ok()))
                .map(|(_, e)| e)
                .filter(|e| array.is_some() || !matches!(e, Object::Null));
            let Some(entry) = entry else {
                continue;
            };
            found += 1;
            let required =
                self.with_cx(object, at, parents, |cx| self.required(row, cx)) == Truth::True;
            let decisive = matches!(row.key.as_str(), "Type" | "Subtype" | "S");
            let first = array.is_some() && row.key == "0";
            let typed = row.r#type.iter().position(|t| {
                !t.wraps.iter().any(|w| w.func == WrapFn::Extension) && self.matches(entry, t.item)
            });
            let Some(i) = typed else {
                score += if decisive {
                    20
                } else if first {
                    3
                } else {
                    1
                };
                bad |= required;
                continue;
            };
            let special = &row.special_case[i];
            let allowed = self.with_cx(object, at, parents, |cx| {
                !matches!(self.values(row, i, entry, cx), Values::Not(_))
                    && (special.is_empty() || predicate::holds_fully(special, cx) != Truth::False)
            });
            if allowed {
                score += if decisive || matches!(row.key.as_str(), "Parent" | "TransformMethod") {
                    -80
                } else if first && required {
                    -60
                } else if first {
                    -20
                } else if required {
                    -10
                } else {
                    -4
                };
            } else {
                score += if decisive {
                    10
                } else if first {
                    7
                } else {
                    5
                };
                bad |= required;
            }
            if version(&row.deprecated_in).is_some_and(|v| self.version >= v) {
                score += 8;
            }
        }
        if !bad {
            score -= 8 * found;
        }
        score + (-10.0 * f64::from(found) / rows.len() as f64) as i32
    }

    /// Walks the name or number tree `root` (with its number, when it's an
    /// indirect object), the value of `row`'s key at `step` from `from`'s
    /// object, of the row's `i`th type: each node through its `/Kids`, its
    /// `/Names` or `/Nums` pairs' values as the table of the type's links
    /// they fit best, as TestGrammar's `parse_name_tree` and
    /// `parse_number_tree` do. A node that breaks ISO 32000-1 7.9.6 or
    /// 7.9.7 is a [`Rule::Tree`] finding.
    fn tree(
        &mut self,
        from: &Visit<'a>,
        step: Step<'a>,
        (id, root): (Option<ObjectId>, &'a Object),
        row: &'static Row,
        i: usize,
        parents: &[&'a Object],
    ) {
        let ty = row.r#type[i].item;
        let links = self.links(&row.link[i]);
        let (entries, names): (&[u8], _) = match ty {
            ArlType::NameTree => (b"Names", true),
            _ => (b"Nums", false),
        };
        let mut up = Vec::with_capacity(PARENTS);
        up.push(from.object);
        up.extend(parents.iter().take(PARENTS - 1));
        let depth = if id.is_some() { 0 } else { from.depth + 1 };
        let mut nodes = vec![(root, id, depth, true)];
        while let Some((node, node_id, depth, is_root)) = nodes.pop() {
            if let Some(n) = node_id {
                if self.reached.get(&n).copied().flatten().is_some() {
                    continue;
                }
                self.reached.insert(n, Some(ty.name()));
            }
            let Object::Dictionary(dict) = node else {
                continue;
            };
            let here = Visit {
                object: node,
                holder: node_id.or(from.holder),
                table: from.table,
                trail: Rc::new(Trail {
                    up: Some(from.trail.clone()),
                    step: step.clone(),
                    object: node,
                }),
                depth,
                loose: from.loose,
            };
            let made = self.made(here.holder);
            // What's wrong with the node, as a tree finding unless said.
            let mut wrong = Vec::new();
            let gone = |what: &str, (n, g): ObjectId| {
                (
                    Rule::Unresolved,
                    format!("its {what} is {n} {g} R, no object"),
                )
            };
            if made.is_some() {
                *self
                    .report
                    .checked
                    .entry(ty.name().to_string())
                    .or_default() += 1;
                let has = |k: &[u8]| dict.get(k).is_ok_and(|o| !matches!(o, Object::Null));
                let entries_name = String::from_utf8_lossy(entries);
                let (kids, leaves, limits) = (has(b"Kids"), has(entries), has(b"Limits"));
                if is_root && !kids && !leaves {
                    let what = format!("a root with neither /Kids nor /{entries_name}");
                    wrong.push((Rule::Tree, what));
                }
                if kids && leaves {
                    let what = format!("a node with both /Kids and /{entries_name}");
                    wrong.push((Rule::Tree, what));
                }
                if is_root && limits {
                    wrong.push((Rule::Tree, "a root with /Limits".to_string()));
                }
                if !is_root && !limits {
                    let what = "a node under the root without /Limits".to_string();
                    wrong.push((Rule::Tree, what));
                }
            }
            if let Ok(raw) = dict.get(entries) {
                self.refer(raw, from.table, row.key.clone());
                match self.resolve(raw) {
                    Ok((_, Object::Array(pairs))) => {
                        for (k, pair) in pairs.chunks(2).enumerate() {
                            let (label, value) = match pair {
                                [label, value] => (label, value),
                                _ => {
                                    let what = format!("its last key, [{}], has no value", 2 * k);
                                    wrong.push((Rule::Tree, what));
                                    continue;
                                }
                            };
                            let label = match self.resolve(label) {
                                Ok((_, Object::String(s, _))) if names => text_string(s),
                                Ok((_, Object::Integer(n))) if !names => n.to_string(),
                                _ => {
                                    let kind = if names { "a string" } else { "an integer" };
                                    let what = format!("its key [{}] isn't {kind}", 2 * k);
                                    wrong.push((Rule::Tree, what));
                                    continue;
                                }
                            };
                            self.refer(value, from.table, row.key.clone());
                            match self.resolve(value) {
                                Ok((
                                    vid,
                                    v @ (Object::Dictionary(_)
                                    | Object::Stream(_)
                                    | Object::Array(_)),
                                )) => self.reach(&here, Step::Entry(label), (vid, v), &links, &up),
                                Ok((vid, v)) => {
                                    if !links.is_empty() {
                                        let what =
                                            format!("its {label} is {}, of no table", render(v));
                                        wrong.push((Rule::Tree, what));
                                    }
                                    if let Some(vid) = vid {
                                        self.reached.entry(vid).or_insert(None);
                                    }
                                }
                                Err(to) => wrong.push(gone(&label, to)),
                            }
                        }
                    }
                    Ok((_, Object::Null)) => {}
                    Ok(_) => {
                        let what =
                            format!("its /{} isn't an array", String::from_utf8_lossy(entries));
                        wrong.push((Rule::Tree, what));
                    }
                    Err(to) => {
                        wrong.push(gone(&format!("/{}", String::from_utf8_lossy(entries)), to))
                    }
                }
            }
            if let Ok(raw) = dict.get(b"Kids") {
                self.refer(raw, from.table, row.key.clone());
                match self.resolve(raw) {
                    Ok((_, Object::Array(kids))) => {
                        for (i, kid) in kids.iter().enumerate() {
                            self.refer(kid, from.table, row.key.clone());
                            match self.resolve(kid) {
                                Ok((kid_id, kid @ Object::Dictionary(_))) => {
                                    let depth = if kid_id.is_some() { 0 } else { depth + 1 };
                                    if depth <= NESTING {
                                        nodes.push((kid, kid_id, depth, false));
                                    } else if made.is_some() {
                                        self.report.truncated += 1;
                                    }
                                }
                                Ok((_, other)) => {
                                    let what = format!(
                                        "its /Kids[{i}] is {}, not a dictionary",
                                        render(other)
                                    );
                                    wrong.push((Rule::Tree, what));
                                }
                                Err(to) => wrong.push(gone(&format!("/Kids[{i}]"), to)),
                            }
                        }
                    }
                    Ok((_, Object::Null)) => {}
                    Ok(_) => wrong.push((Rule::Tree, "its /Kids isn't an array".to_string())),
                    Err(to) => wrong.push(gone("/Kids", to)),
                }
            }
            if let Some(made) = made {
                for (rule, what) in wrong {
                    self.find(&here, made, rule, &from.table.name, &row.key, what);
                }
            }
        }
    }

    /// Whether `v`'s dictionary has each key its table requires, for an
    /// inheritable one on a `/Parent` if not on it.
    fn required_keys(
        &mut self,
        v: &Visit<'a>,
        dict: &Dictionary,
        made: Made,
        parents: &[&'a Object],
    ) {
        for row in &v.table.rows {
            // A wildcard is never required (INTERNAL_GRAMMAR.md, "Column 5
            // - Required").
            let key = row.key.as_bytes();
            if row.key.ends_with('*') || self.present(dict.get(key).ok()) {
                continue;
            }
            match self.with_cx(v.object, At::Key(key), parents, |cx| self.required(row, cx)) {
                Truth::True if row.inheritable && self.inherited(dict, key) => {}
                Truth::True => self.find(
                    v,
                    made,
                    Rule::Missing,
                    &v.table.name,
                    &row.key,
                    "absent".to_string(),
                ),
                Truth::False => {}
                Truth::Unknown(why) => self.unchecked("Required", why, &v.table.name, &row.key),
            }
        }
    }

    /// Whether `v`'s array, `n` long, has each element its table requires:
    /// those before its repeating rows, and of its repeating rows, those
    /// of the last set it starts (or the first).
    fn required_elements(&mut self, v: &Visit<'a>, n: usize, made: Made, parents: &[&'a Object]) {
        let set = v.table.repeating().len();
        let first = v.table.rows.len() - set;
        for (p, row) in v.table.rows.iter().enumerate() {
            let i = if p < first {
                match row.key.parse::<usize>() {
                    Ok(i) => i,
                    // A wildcard is never required.
                    Err(_) => continue,
                }
            } else {
                let sets = n.saturating_sub(first).div_ceil(set).max(1);
                first + (sets - 1) * set + (p - first)
            };
            if i < n {
                continue;
            }
            match self.with_cx(v.object, At::Index(i), parents, |cx| self.required(row, cx)) {
                Truth::True => self.find(
                    v,
                    made,
                    Rule::Missing,
                    &v.table.name,
                    &row.key,
                    "absent".to_string(),
                ),
                Truth::False => {}
                Truth::Unknown(why) => self.unchecked("Required", why, &v.table.name, &row.key),
            }
        }
    }

    /// Whether `row`'s key is required where `cx` is: never before its
    /// SinceVersion, nor an extension's key (TestGrammar's
    /// `IsValidForPDFVersion`); else as its Required cell says.
    fn required(&self, row: &Row, cx: &Cx) -> Truth {
        match predicate::since(&row.since_version) {
            Since::In(v) if self.version >= v => predicate::required(&row.required, cx),
            _ => Truth::False,
        }
    }

    /// Whether an entry is there: not absent, and not null, which a
    /// dictionary's entry may be (ISO 32000-1 7.3.7) through a reference
    /// too. A reference to no object is there, found unresolved.
    fn present(&self, entry: Option<&Object>) -> bool {
        match entry.map(|e| self.resolve(e)) {
            None | Some(Ok((_, Object::Null))) => false,
            Some(_) => true,
        }
    }

    /// Whether a `/Parent` up from `dict` has `key`: each once, and no more
    /// of them than the page tree's depth (PDFium's `kMaxPageLevel`). Or,
    /// for `/DA`, which only fields inherit, whether the interactive form
    /// dictionary has it: its `/DA` is the document-wide default (ISO
    /// 32000-1 12.7.2, Table 218), where TestGrammar looks up the
    /// `/Parent`s alone.
    fn inherited(&self, dict: &Dictionary, key: &[u8]) -> bool {
        let mut seen = HashSet::new();
        let mut at = dict;
        for _ in 0..Limits::DEFAULT.page_tree.depth {
            let parent = match at.get(b"Parent").ok().map(|p| self.resolve(p)) {
                Some(Ok((id, Object::Dictionary(parent)))) => (id, parent),
                _ => break,
            };
            if let (Some(id), _) = parent
                && !seen.insert(id)
            {
                break;
            }
            if self.present(parent.1.get(key).ok()) {
                return true;
            }
            at = parent.1;
        }
        key == b"DA" && (self.form()).is_some_and(|form| self.present(form.get(key).ok()))
    }

    /// The interactive form dictionary: the catalog's `/AcroForm`.
    fn form(&self) -> Option<&'a Dictionary> {
        let dict = |o: &'a Object| self.resolve(o).ok()?.1.as_dict().ok();
        let catalog = dict(self.trailer.get(b"Root").ok()?)?;
        dict(catalog.get(b"AcroForm").ok()?)
    }

    /// `value`'s type among `row`'s: the first it is that the file's
    /// version lets in, and not as deprecated if one is.
    fn typed(&self, row: &Row, value: &Object) -> Typed {
        let (mut deprecated, mut excluded) = (None, None);
        for (i, ty) in row.r#type.iter().enumerate() {
            if !self.matches(value, ty.item) {
                continue;
            }
            match predicate::gate(&ty.wraps, self.version) {
                Gate::Allowed => return Typed::Is(i, None),
                Gate::AllowedDeprecated(from) => {
                    deprecated.get_or_insert(Typed::Is(i, Some(from)));
                }
                Gate::Excluded => {
                    excluded.get_or_insert(Typed::Excluded(ty.item));
                }
            }
        }
        deprecated.or(excluded).unwrap_or(Typed::Not)
    }

    /// Whether `o` is of the type `ty`, as TestGrammar's `ArlVersion` tells
    /// (ArlVersion.cpp): an integer is an integer, a bitmask or a number,
    /// a string any string or a date, an array an array, a rectangle or a
    /// matrix, and a dictionary a dictionary or a tree. Refined as the
    /// model's types are (INTERNAL_GRAMMAR.md, "Column 2 - Type"): a
    /// rectangle is four numbers and a matrix six (ISO 32000-1 7.9.5 and
    /// 8.3.4), a string-ascii printable ASCII, and a date reads as
    /// [`is_date`] reads one.
    fn matches(&self, o: &Object, ty: ArlType) -> bool {
        use ArlType as T;
        match (o, ty) {
            (Object::Integer(_), T::Integer | T::Bitmask | T::Number)
            | (Object::Real(_), T::Number)
            | (Object::Boolean(_), T::Boolean)
            | (Object::Name(_), T::Name)
            | (Object::Null, T::Null)
            | (Object::Stream(_), T::Stream)
            | (Object::String(..), T::String | T::StringByte | T::StringText)
            | (Object::Array(_), T::Array)
            | (Object::Dictionary(_), T::Dictionary | T::NameTree | T::NumberTree) => true,
            (Object::String(s, _), T::StringAscii) => s.iter().all(|b| (0x20..=0x7E).contains(b)),
            (Object::String(s, _), T::Date) => is_date(&text_string(s)),
            (Object::Array(a), T::Rectangle) => self.numbers(a, 4),
            (Object::Array(a), T::Matrix) => self.numbers(a, 6),
            _ => false,
        }
    }

    /// Whether `a` is `n` numbers.
    fn numbers(&self, a: &[Object], n: usize) -> bool {
        a.len() == n
            && (a.iter()).all(|e| {
                matches!(
                    self.resolve(e),
                    Ok((_, Object::Integer(_) | Object::Real(_)))
                )
            })
    }

    /// What `row`'s PossibleValues for its `i`th type make of `value`,
    /// of those the file's version lets in: each a constant
    /// ([`is_value`]), the wildcard `*` for any name, or a predicate
    /// ([`predicate::holds_fully`]). A predicate that can't be told makes
    /// them all unknown, met or not: `fn:RequiredValue` says what the
    /// value must be, whatever else is listed.
    fn values(&self, row: &'static Row, i: usize, value: &Object, cx: &Cx) -> Values {
        let items = &row.possible_values[i];
        if items.is_empty() {
            return Values::Allowed;
        }
        let (mut listed, mut met, mut unknown) = (Vec::new(), None, None);
        for item in items {
            let gate = predicate::gate(&item.wraps, self.version);
            if gate == Gate::Excluded {
                continue;
            }
            listed.push(item.item.as_str());
            let truth = if item.item.starts_with("fn:") {
                predicate::holds_fully(&item.item, cx)
            } else if is_value(&item.item, value) {
                Truth::True
            } else {
                Truth::False
            };
            match (truth, gate) {
                (Truth::True, Gate::AllowedDeprecated(from)) => {
                    met.get_or_insert(Values::Deprecated(from));
                }
                (Truth::True, _) => met = Some(Values::Allowed),
                (Truth::False, _) => {}
                (Truth::Unknown(why), _) => {
                    unknown.get_or_insert(why);
                }
            }
        }
        match (unknown, met) {
            (Some(why), _) => Values::Unknown(why),
            (None, Some(met)) => met,
            (None, None) => Values::Not(listed),
        }
    }

    /// `f` of the predicates' view of the row at `at` in `container`.
    fn with_cx<R>(
        &self,
        container: &Object,
        at: At,
        parents: &[&Object],
        f: impl FnOnce(&Cx) -> R,
    ) -> R {
        let resolve = |id: ObjectId| self.pdf.get(id).ok();
        f(&Cx {
            container,
            at,
            parents,
            trailer: self.trailer,
            resolve: &resolve,
            version: self.version,
        })
    }

    /// `raw` with its references followed, at most as many as a reader
    /// follows in one chain: the number of the object it's in, if it's an
    /// indirect object, and it. The number of one the file hasn't, or of
    /// the last of a chain too long, is the error.
    fn resolve<'o>(&self, raw: &'o Object) -> Result<(Option<ObjectId>, &'o Object), ObjectId>
    where
        'a: 'o,
    {
        let (mut id, mut at) = (None, raw);
        for _ in 0..Limits::DEFAULT.reader.max_depth {
            let Object::Reference(to) = at else {
                return Ok((id, at));
            };
            at = self.pdf.get(*to).map_err(|_| *to)?;
            id = Some(*to);
        }
        Err(id.unwrap_or((0, 0)))
    }

    fn find(&mut self, at: &Visit, made: Made, rule: Rule, table: &str, key: &str, value: String) {
        self.report.findings.push(Finding {
            object: at.holder,
            made,
            path: at.trail.path(),
            table: table.to_string(),
            key: key.to_string(),
            rule,
            value,
        });
    }

    fn unchecked(&mut self, column: &'static str, why: String, table: &str, key: &str) {
        let unchecked = Unchecked {
            column,
            why,
            table: table.to_string(),
            key: key.to_string(),
        };
        *self.report.unchecked.entry(unchecked).or_default() += 1;
    }

    /// The held objects the walk never reached, each a [`Rule::Unreached`]
    /// finding but those referenced under a key the model doesn't list, an
    /// integer a reached stream's `/Length` references, and the file's own
    /// structures (object and cross-reference streams). Its table and key are those it was
    /// referenced under in an object walked, else the key of a held object
    /// unreached too, else `-`. One the file hasn't, or that doesn't read,
    /// is [`Rule::Unresolved`].
    fn unreached(&mut self) {
        let held: Vec<(ObjectId, Made)> = match self.hold {
            Hold::All(made) => self.pdf.ids().map(|id| (id, *made)).collect(),
            Hold::Only(objects) => objects.iter().map(|(&id, &made)| (id, made)).collect(),
        };
        let mut left = Vec::new();
        for (id, made) in held {
            if self.reached.contains_key(&id) || self.under_unlisted.contains(&id) {
                continue;
            }
            match self.pdf.get(id) {
                Ok(object) if left_out(object) => {}
                Ok(object) => left.push((id, made, object)),
                Err(e) => self.report.findings.push(Finding {
                    object: Some(id),
                    made,
                    path: "-".to_string(),
                    table: "-".to_string(),
                    key: "-".to_string(),
                    rule: Rule::Unresolved,
                    value: e.to_string(),
                }),
            }
        }
        let integer = |object: &Object| matches!(object, Object::Integer(_));
        if left.iter().any(|(_, _, object)| integer(object)) {
            let lengths = self.lengths();
            left.retain(|(id, _, object)| !(integer(object) && lengths.contains(id)));
        }
        let mut among = HashMap::new();
        for &(_, _, object) in &left {
            let entries: Vec<(String, &Object)> = match object {
                Object::Dictionary(d) | Object::Stream(Stream { dict: d, .. }) => (d.iter())
                    .map(|(k, v)| (String::from_utf8_lossy(k).into_owned(), v))
                    .collect(),
                Object::Array(a) => a
                    .iter()
                    .enumerate()
                    .map(|(i, v)| (i.to_string(), v))
                    .collect(),
                _ => Vec::new(),
            };
            for (key, value) in entries {
                each_reference(value, |to| {
                    among.entry(to).or_insert_with(|| key.clone());
                });
            }
        }
        for (id, made, object) in left {
            let (table, key) = match self.referrers.get(&id) {
                Some((table, key)) => (table.to_string(), key.clone()),
                None => (
                    "-".to_string(),
                    among.get(&id).cloned().unwrap_or("-".to_string()),
                ),
            };
            self.report.findings.push(Finding {
                object: Some(id),
                made,
                path: "-".to_string(),
                table,
                key,
                rule: Rule::Unreached,
                value: render(object),
            });
        }
    }

    /// The objects reached streams' `/Length`s reference, as the streams'
    /// bytes write them: lopdf reads a referenced length into the
    /// dictionary it gives, so the walk never sees the reference.
    fn lengths(&self) -> HashSet<ObjectId> {
        let mut lengths = HashSet::new();
        for &id in self.reached.keys() {
            if !matches!(self.pdf.get(id), Ok(Object::Stream(_))) {
                continue;
            }
            let written = self.written_dictionary(id);
            if let Some(Ok(Object::Reference(length))) = written.as_ref().map(|d| d.get(b"Length"))
            {
                lengths.insert(*length);
            }
        }
        lengths
    }

    /// The stream `id`'s dictionary as its bytes write it, its `/Length`
    /// as written; none when they don't read.
    fn written_dictionary(&self, id: ObjectId) -> Option<Dictionary> {
        (self.pdf.raw_span(id))
            .and_then(|span| self.bytes.get(span))
            .and_then(stream_dictionary)
            .and_then(parse_dictionary)
    }
}

/// A stream object's bytes, `n g obj` on, to the end of its dictionary,
/// before its `stream` keyword.
fn stream_dictionary(object: &[u8]) -> Option<&[u8]> {
    let start = object.windows(3).position(|w| w == b"obj")? + 3;
    let mut from = start;
    loop {
        let at = from + object[from..].windows(6).position(|w| w == b"stream")?;
        if object[..at].trim_ascii_end().ends_with(b">>") {
            return Some(&object[start..at]);
        }
        from = at + 6;
    }
}

impl<'a> Trail<'a> {
    /// The objects the walk came through to this one, nearest first, as
    /// many as `parent::` reaches.
    fn parents(&self) -> Vec<&'a Object> {
        let mut parents = Vec::with_capacity(PARENTS);
        let mut at = self.up.as_deref();
        while let Some(trail) = at
            && parents.len() < PARENTS
        {
            parents.push(trail.object);
            at = trail.up.as_deref();
        }
        parents
    }

    /// The way from the trailer, its last [`PATH`] steps:
    /// `Root/Pages/Kids[0]/Annots[2]`, a tree's entries in braces.
    fn path(&self) -> String {
        let mut steps = Vec::new();
        let mut at = Some(self);
        while let Some(trail) = at
            && steps.len() <= PATH
        {
            steps.push(&trail.step);
            at = trail.up.as_deref();
        }
        let mut path = String::new();
        if steps.len() > PATH {
            steps.truncate(PATH);
            path.push('…');
        }
        for step in steps.into_iter().rev() {
            match step {
                Step::Trailer => {}
                Step::Key(key) => {
                    if !path.is_empty() {
                        path.push('/');
                    }
                    path.push_str(&String::from_utf8_lossy(key));
                }
                Step::Index(i) => {
                    let _ = write!(path, "[{i}]");
                }
                Step::Entry(entry) => {
                    let _ = write!(path, "{{{entry}}}");
                }
            }
        }
        if path.is_empty() {
            "trailer".to_string()
        } else {
            path
        }
    }
}

/// Whether `key` is one of the private keys sheaf writes (M10.5 §15,
/// [`private::KEYS`]), in a dictionary or a property list: neither a
/// second- nor a third-class name (backlog F24), but sheaf's on purpose.
/// An older sheaf's, which sheaf no longer writes, isn't.
fn sheaf_s_own(key: &[u8]) -> bool {
    use private::Place;
    (private::KEYS.iter()).any(|own| {
        let keyed = (own.places.iter()).any(|p| {
            matches!(
                p,
                Place::Annot | Place::Catalog | Place::Stream | Place::List
            )
        });
        let named = match own.prefix {
            true => key.starts_with(own.name),
            false => key == own.name,
        };
        keyed && named && !own.older
    })
}

/// Whether `table` is one of the model's `_Universal` tables, which fit
/// anything.
fn universal(table: &str) -> bool {
    table.starts_with("_Universal")
}

/// Whether `object` can be of `table`'s kind: an array of a table of
/// elements (its keys indices, repeating rows' `1*` or the wildcard), a
/// dictionary or stream of a table of keys; a table of the wildcard alone,
/// either.
fn fits(object: &Object, table: &Table) -> bool {
    let element = |row: &Row| {
        let index = row.key.strip_suffix('*').unwrap_or(&row.key);
        !index.is_empty() && index.bytes().all(|b| b.is_ascii_digit())
    };
    let keyed = |row: &Row| row.key != "*" && !element(row);
    match object {
        Object::Array(_) => !table.rows.iter().any(keyed),
        Object::Dictionary(_) | Object::Stream(_) => !table.rows.iter().any(element),
        _ => true,
    }
}

/// The array index a row's key names: `0`, or a repeating row's `1*`;
/// none for the wildcard (TestGrammar's `key_to_array_index`).
fn index(key: &str) -> Option<usize> {
    let digits = key.bytes().take_while(u8::is_ascii_digit).count();
    key[..digits].parse().ok()
}

/// A key or element as findings name it.
fn at_text(at: At) -> String {
    match at {
        At::Key(key) => String::from_utf8_lossy(key).into_owned(),
        At::Index(i) => i.to_string(),
    }
}

/// Whether `value` is the PossibleValues constant `text`
/// (INTERNAL_GRAMMAR.md, "PDF Object conventions" and "Column 9 -
/// PossibleValues"): a name, or any name for `*`; a string in single
/// quotes; a number, near enough; `true` or `false`; an array in square
/// brackets, its elements apart by spaces (an image mask's `[0 1]`). A
/// null, a dictionary or a stream is any.
fn is_value(text: &str, value: &Object) -> bool {
    match value {
        Object::Array(elements) => {
            let inner = text.strip_prefix('[').and_then(|t| t.strip_suffix(']'));
            inner.is_some_and(|inner| {
                let items: Vec<&str> = inner.split_ascii_whitespace().collect();
                items.len() == elements.len()
                    && (items.iter().zip(elements)).all(|(item, e)| is_value(item, e))
            })
        }
        Object::Name(name) => text == "*" || name.as_slice() == text.as_bytes(),
        Object::String(s, _) => {
            text.strip_prefix('\'').and_then(|t| t.strip_suffix('\'')) == Some(&text_string(s))
        }
        Object::Integer(_) | Object::Real(_) => {
            let n = value.as_float().unwrap_or(f64::NAN);
            text.parse::<f64>()
                .is_ok_and(|t| (t - n).abs() <= TOLERANCE)
        }
        Object::Boolean(b) => text == if *b { "true" } else { "false" },
        Object::Null | Object::Dictionary(_) | Object::Stream(_) | Object::Reference(_) => true,
    }
}

/// Each reference in `o`, and in the direct objects it holds, no deeper
/// than [`NESTING`].
pub(super) fn each_reference(o: &Object, mut f: impl FnMut(ObjectId)) {
    let mut stack = vec![(o, 0)];
    while let Some((o, depth)) = stack.pop() {
        match o {
            Object::Reference(id) => f(*id),
            _ if depth >= NESTING => {}
            Object::Array(a) => stack.extend(a.iter().map(|e| (e, depth + 1))),
            Object::Dictionary(d) | Object::Stream(Stream { dict: d, .. }) => {
                stack.extend(d.iter().map(|(_, v)| (v, depth + 1)))
            }
            _ => {}
        }
    }
}

/// Whether `text` is a date (ISO 32000-1 7.9.4) as TestGrammar's
/// `is_valid_pdf_date_string` reads one (`TestGrammar/src/Utils.cpp`,
/// Copyright 2020-2022 PDF Association, Inc., Apache-2.0), ported: `D:`
/// and a year, then as many of month, day, hour, minute and second as
/// follow, two digits each and in range; then an offset's `Z`, `+` or
/// `-`, its hours, an apostrophe and its minutes, each if there. Its
/// expression is anchored only at the start, so anything after is let be.
pub(super) fn is_date(text: &str) -> bool {
    let Some(rest) = text.strip_prefix("D:") else {
        return false;
    };
    let b = rest.as_bytes();
    let two = |at: usize| {
        (b.get(at..at + 2))
            .filter(|d| d.iter().all(u8::is_ascii_digit))
            .map(|d| u32::from(d[0] - b'0') * 10 + u32::from(d[1] - b'0'))
    };
    if !b
        .get(..4)
        .is_some_and(|year| year.iter().all(u8::is_ascii_digit))
    {
        return false;
    }
    let mut at = 4;
    for range in [1..=12, 1..=31, 0..=23, 0..=59, 0..=59] {
        let Some(n) = two(at) else {
            break;
        };
        if !range.contains(&n) {
            return false;
        }
        at += 2;
    }
    if b.get(at).is_some_and(|c| b"Z+-".contains(c)) {
        at += 1;
    }
    if let Some(hours) = two(at) {
        if hours > 23 {
            return false;
        }
        at += 2;
    }
    if b.get(at) == Some(&b'\'') {
        at += 1;
    }
    two(at).is_none_or(|minutes| minutes <= 59)
}

/// `value` with each reference in it, `12 0 R`, as `R`.
pub(super) fn numberless(value: &str) -> String {
    let b = value.as_bytes();
    let digits = |from: usize| b[from..].iter().take_while(|c| c.is_ascii_digit()).count();
    let mut out = String::with_capacity(value.len());
    let (mut at, mut kept) = (0, 0);
    while at < b.len() {
        let starts = b[at].is_ascii_digit() && (at == 0 || !b[at - 1].is_ascii_alphanumeric());
        let number = if starts { digits(at) } else { 0 };
        let generation = match b.get(at + number) {
            Some(b' ') if number > 0 => digits(at + number + 1),
            _ => 0,
        };
        let r = at + number + 1 + generation;
        let reference = generation > 0
            && b.get(r..r + 2) == Some(b" R")
            && b.get(r + 2).is_none_or(|c| !c.is_ascii_alphanumeric());
        if reference {
            out.push_str(&value[kept..at]);
            out.push('R');
            at = r + 2;
            kept = at;
        } else {
            at += 1;
        }
    }
    out.push_str(&value[kept..]);
    out
}

/// `o` as a finding has it: its first entries, two deep, names and
/// strings (as text) their first [`TOKEN`] bytes.
fn render(o: &Object) -> String {
    let mut out = String::new();
    write_value(&mut out, o, 2);
    out
}

fn write_value(out: &mut String, o: &Object, depth: usize) {
    match o {
        Object::Null => out.push_str("null"),
        Object::Boolean(b) => {
            let _ = write!(out, "{b}");
        }
        Object::Integer(i) => {
            let _ = write!(out, "{i}");
        }
        Object::Real(r) => {
            let _ = write!(out, "{r}");
        }
        Object::Name(name) => {
            out.push('/');
            out.push_str(&String::from_utf8_lossy(&name[..name.len().min(TOKEN)]));
        }
        Object::String(s, _) => {
            let _ = write!(out, "({})", text_string(&s[..s.len().min(TOKEN)]));
        }
        Object::Reference((n, g)) => {
            let _ = write!(out, "{n} {g} R");
        }
        Object::Array(_) | Object::Dictionary(_) | Object::Stream(_) if depth == 0 => out.push('…'),
        Object::Array(a) => {
            out.push('[');
            for (i, e) in a.iter().take(6).enumerate() {
                if i > 0 {
                    out.push(' ');
                }
                write_value(out, e, depth - 1);
            }
            if a.len() > 6 {
                out.push_str(" …");
            }
            out.push(']');
        }
        Object::Dictionary(d) => write_dictionary(out, d, depth),
        Object::Stream(s) => {
            out.push_str("stream ");
            write_dictionary(out, &s.dict, depth);
        }
    }
}

fn write_dictionary(out: &mut String, d: &Dictionary, depth: usize) {
    out.push_str("<<");
    for (key, value) in d.iter().take(4) {
        let key = &key[..key.len().min(TOKEN)];
        let _ = write!(out, " /{} ", String::from_utf8_lossy(key));
        write_value(out, value, depth - 1);
    }
    if d.len() > 4 {
        out.push_str(" …");
    }
    out.push_str(" >>");
}

impl Made {
    pub fn name(self) -> &'static str {
        match self {
            Made::Built => "built",
            Made::Merged => "merged",
        }
    }
}

impl Rule {
    pub const ALL: [Rule; 12] = [
        Rule::Type,
        Rule::Missing,
        Rule::TooNew,
        Rule::Deprecated,
        Rule::Value,
        Rule::Indirect,
        Rule::Direct,
        Rule::Special,
        Rule::Unreached,
        Rule::Unresolved,
        Rule::Tree,
        Rule::Unlisted,
    ];

    /// Its name, as findings and allowances write it.
    pub fn name(self) -> &'static str {
        match self {
            Rule::Type => "type",
            Rule::Missing => "missing",
            Rule::TooNew => "too-new",
            Rule::Deprecated => "deprecated",
            Rule::Value => "value",
            Rule::Indirect => "indirect",
            Rule::Direct => "direct",
            Rule::Special => "special",
            Rule::Unreached => "unreached",
            Rule::Unresolved => "unresolved",
            Rule::Tree => "tree",
            Rule::Unlisted => "unlisted",
        }
    }

    /// The rule named `name`.
    pub fn named(name: &str) -> Option<Rule> {
        Rule::ALL.into_iter().find(|r| r.name() == name)
    }
}

impl Class {
    /// The kind of name `key` is: [`Class::Second`] or [`Class::Third`] by
    /// TestGrammar's expressions for them (`^([a-zA-Z0-9_\-]{4,5}(_|:))`
    /// and `^XX`), else [`Class::Other`].
    pub fn of(key: &[u8]) -> Class {
        let prefix = |n: usize| {
            key.len() > n
                && (key[..n].iter()).all(|&b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
                && matches!(key[n], b'_' | b':')
        };
        if prefix(4) || prefix(5) {
            Class::Second
        } else if key.starts_with(b"XX") {
            Class::Third
        } else {
            Class::Other
        }
    }
}

impl fmt::Display for Class {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Class::Second => f.write_str("second-class"),
            Class::Third => f.write_str("third-class"),
            Class::Extension(name) => write!(f, "{name}'s"),
            Class::Other => f.write_str("other"),
        }
    }
}

impl fmt::Display for Rule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let object = match self.object {
            Some((n, g)) => format!("{n} {g} R"),
            None => "the trailer".to_string(),
        };
        let value: String = match self.value.char_indices().nth(SHOWN - 1) {
            Some((cut, _)) if self.value.chars().count() > SHOWN => {
                format!("{}…", &self.value[..cut])
            }
            _ => self.value.clone(),
        };
        write!(
            f,
            "{}: {}/{} at {}, in {object} ({}): {value}",
            self.rule,
            self.table,
            self.key,
            self.path,
            self.made.name(),
        )
    }
}

impl fmt::Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let objects: usize = self.checked.values().sum();
        let unchecked: usize = self.unchecked.values().sum();
        let unlisted: usize = self.unlisted.values().sum();
        writeln!(
            f,
            "Arlington: {objects} held objects checked as {} tables, {} keys; {} findings; \
             {unchecked} checks not made for predicates that can't be told; {unlisted} keys \
             the model doesn't list; {} with no table, {} ties, {} truncated, {} in two contexts",
            self.checked.len(),
            self.keys,
            self.findings.len(),
            self.no_table,
            self.ties,
            self.truncated,
            self.two_contexts
        )?;
        for (table, n) in &self.checked {
            writeln!(f, "  checked as {table}: {n}")?;
        }
        let mut by_why: BTreeMap<(&str, &str), (usize, Vec<String>)> = BTreeMap::new();
        for (u, n) in &self.unchecked {
            let (count, keys) = by_why.entry((u.column, &u.why)).or_default();
            *count += n;
            keys.push(format!("{}/{}", u.table, u.key));
        }
        for ((column, why), (n, keys)) in by_why {
            let more = if keys.len() > 3 { ", …" } else { "" };
            let some = keys[..keys.len().min(3)].join(", ");
            writeln!(f, "  not checked, {column} {why}: {n} ({some}{more})")?;
        }
        for (u, n) in &self.unlisted {
            writeln!(f, "  unlisted: {}/{} ({}): {n}", u.table, u.key, u.class)?;
        }
        for finding in &self.findings {
            writeln!(f, "- {finding}")?;
        }
        Ok(())
    }
}
