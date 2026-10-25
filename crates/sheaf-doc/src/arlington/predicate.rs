//! The predicates in the model's cells, parsed, and evaluated where the
//! objects sheaf writes need them. The language is INTERNAL_GRAMMAR.md's
//! "Validation of predicates (declarative functions)": `fn:` functions,
//! fully bracketed `&&` and `||`, six comparisons, arithmetic, and keys,
//! `@` values and `::` paths from the object, its `parent::` or the
//! `trailer::`. What each function means is its "Predicates (declarative
//! functions)"; "Parameters to predicates" says any part of a predicate
//! may be indeterminable: a key the object lacks or has null, or a value
//! of a type the comparison can't use.
//!
//! [`SUPPORTED`] are the functions evaluated. Any other comes to
//! [`Truth::Unknown`], named, and so does arithmetic: never guessed. The
//! checker counts a check an unknown predicate stands in the way of as not
//! made.
//!
//! The version predicates, and `fn:Extension` with a statement, say when
//! the statement applies. Where it doesn't, it's vacant, and `&&` and `||`
//! go on without it, as TestGrammar's reduction does (its
//! `CPDFFile::ProcessPredicate`, Apache-2.0): `fn:IsPDFVersion(1.1,X) &&
//! fn:SinceVersion(1.7,Y)` is `Y` at 1.7 and asks nothing at 1.3. Each
//! column's reader says what a vacant predicate comes to there.

use std::cmp::Ordering;
use std::fmt;

use anyhow::{Context, Result, bail, ensure};
use lopdf::{Dictionary, Object, ObjectId};

use super::grammar::{At, Wrap, WrapFn, version};
use crate::content::Std14;
use crate::file::text_string;
use crate::materialize::emit::PdfVersion;

/// The functions [`required`], [`holds`] and [`reference()`] evaluate.
pub const SUPPORTED: [&str; 19] = [
    "fn:ArrayLength",
    "fn:BeforeVersion",
    "fn:BitClear",
    "fn:BitSet",
    "fn:BitsClear",
    "fn:BitsSet",
    "fn:Deprecated",
    "fn:Eval",
    "fn:Extension",
    "fn:IsPDFVersion",
    "fn:IsPresent",
    "fn:IsRequired",
    "fn:MustBeDirect",
    "fn:MustBeIndirect",
    "fn:Not",
    "fn:NotStandard14Font",
    "fn:RectHeight",
    "fn:RectWidth",
    "fn:SinceVersion",
];

/// How deep a predicate may nest, in brackets and calls. The model's
/// deepest is 5 brackets deep (the census test prints it).
const DEPTH: usize = 32;

/// How many references a value may go through to its object.
const HOPS: usize = 32;

/// How near two numbers are equal: TestGrammar's `ArlNumberTolerance`,
/// half the fifth digit old PDF specifications recommended.
pub(super) const TOLERANCE: f64 = 0.000005;

const AND: &str = " && ";
const OR: &str = " || ";

/// A predicate, or a part of one.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    /// `fn:Name(a,b)`: the function, `fn:` and all, and its arguments,
    /// none, one or two.
    Call(String, Vec<Expr>),
    /// `(e)`.
    Group(Box<Expr>),
    /// `a && b && c`.
    And(Vec<Expr>),
    /// `a || b || c`.
    Or(Vec<Expr>),
    /// `a==b`, `a<=b`.
    Cmp(Cmp, Box<Expr>, Box<Expr>),
    /// `a+b`, `a - b`, `a * b`, `a mod b`, left to right: parsed, never
    /// evaluated.
    Arith(Box<Expr>, Vec<(Arith, Expr)>),
    /// A key, a path, or the value at one: `Key`, `A::B`, `@Key`, `*`. A
    /// bare word compared with a value is a name (`@Type==Template`).
    Path(Path),
    Int(i64),
    /// A real as written: `1.5` is also a PDF version.
    Real(String),
    /// `'text'`.
    Str(String),
    Bool(bool),
    Null,
}

/// A comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cmp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

/// An arithmetic operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arith {
    Add,
    Sub,
    Mul,
    Mod,
}

/// A path through objects: keys and array indices between `::`, the
/// last maybe `@`, the value there (INTERNAL_GRAMMAR.md, "Validation of
/// predicates").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Path {
    pub base: Base,
    /// The keys and indices it goes through. `*` (or a repeating row's
    /// `1*`) first is the row's own value, as in `@*`; `*` last after
    /// others is any entry.
    pub keys: Vec<String>,
    /// Whether it's the value at the path (`@Key`) rather than the key.
    pub value: bool,
    /// As written.
    pub text: String,
}

/// Where a path starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Base {
    /// The object the row is in.
    Here,
    /// The object `n` steps back on the walk's way to it: `parent::` is 1.
    Parent(usize),
    /// The trailer. `trailer::Catalog` is `trailer::Root`, which `keys`
    /// has.
    Trailer,
}

/// What a predicate comes to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Truth {
    True,
    False,
    /// It can't be told, for the first thing in the way: a function the
    /// evaluator doesn't support (`fn:BitsClear`), arithmetic (`+`), or a
    /// value it can't read (`@Type`, which the object lacks).
    Unknown(String),
}

/// What a predicate is evaluated in: the row's place, the walk's way to
/// it, and the file.
pub struct Cx<'a> {
    /// The dictionary, array or stream the row's value is in.
    pub container: &'a Object,
    /// Where in it: the key or element the row matched, which `@*` and
    /// `@1*` read.
    pub at: At<'a>,
    /// The objects the walk came through to `container`, nearest first
    /// (`parent::`).
    pub parents: &'a [&'a Object],
    /// The trailer as written (`trailer::`).
    pub trailer: &'a Dictionary,
    /// An indirect object; none for one the file hasn't, which is null
    /// (ISO 32000-1 7.3.10).
    pub resolve: &'a dyn Fn(ObjectId) -> Option<&'a Object>,
    /// The file's version: its header's, or its catalog's `/Version` when
    /// later.
    pub version: PdfVersion,
}

/// What a type's IndirectReference cell asks of its value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reference {
    /// That it be direct.
    Direct,
    /// That it be an indirect reference.
    Indirect,
    /// Nothing.
    Either,
    /// It can't be told, as [`Truth::Unknown`].
    Unknown(String),
}

/// When a key came in: its SinceVersion cell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Since {
    /// In this version.
    In(PdfVersion),
    /// With this extension, and in no version: allowed when the extension
    /// is, which no file sheaf writes claims.
    Extension(String),
    /// A cell the reader doesn't read.
    Unknown(String),
}

/// What the version predicates around a type, a value or a link make of
/// it at a file's version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gate {
    Allowed,
    /// Allowed, and deprecated from this version, which the file's is at
    /// or past.
    AllowedDeprecated(PdfVersion),
    Excluded,
}

/// The Required cell (INTERNAL_GRAMMAR.md, "Column 5 - Required"): `TRUE`,
/// `FALSE`, or `fn:IsRequired(cond)`, required when `cond` is true and
/// optional when it's false, or when nothing in it applies at the file's
/// version.
pub fn required(cell: &str, cx: &Cx) -> Truth {
    match cell {
        "TRUE" => return Truth::True,
        "FALSE" => return Truth::False,
        _ => {}
    }
    let Ok(Expr::Call(f, args)) = parse(cell) else {
        return Truth::Unknown(cell.to_string());
    };
    match (f.as_str(), &args[..]) {
        ("fn:IsRequired", [cond]) => truth(cond, cx).truth(false),
        _ => Truth::Unknown(cell.to_string()),
    }
}

/// Whether a value meets a predicate: its SpecialCase (INTERNAL_GRAMMAR.md,
/// "Column 10 - SpecialCase": true valid, false invalid), or an `fn:Eval`
/// among its PossibleValues. Met when nothing in it applies at the file's
/// version.
pub fn holds(text: &str, cx: &Cx) -> Truth {
    match parse(text) {
        Ok(expr) => truth(&expr, cx).truth(true),
        Err(_) => Truth::Unknown(text.to_string()),
    }
}

/// What a type's IndirectReference cell asks (INTERNAL_GRAMMAR.md, "Column
/// 6 - IndirectReference"): `TRUE` an indirect reference, `FALSE` nothing,
/// `fn:MustBeDirect()` and `fn:MustBeIndirect()` that it be direct or
/// indirect, and with a condition, that when it holds; when it doesn't, or
/// nothing in it applies at the file's version, nothing.
pub fn reference(cell: &str, cx: &Cx) -> Reference {
    match cell {
        "TRUE" => return Reference::Indirect,
        "FALSE" => return Reference::Either,
        _ => {}
    }
    let Ok(Expr::Call(f, args)) = parse(cell) else {
        return Reference::Unknown(cell.to_string());
    };
    let must = match f.as_str() {
        "fn:MustBeDirect" => Reference::Direct,
        "fn:MustBeIndirect" => Reference::Indirect,
        _ => return Reference::Unknown(cell.to_string()),
    };
    let applies = match &args[..] {
        [] => Tri::Is(true),
        [cond] => truth(cond, cx),
        _ => return Reference::Unknown(cell.to_string()),
    };
    match applies {
        Tri::Is(true) => must,
        Tri::Is(false) | Tri::Vacant => Reference::Either,
        Tri::Unknown(why) => Reference::Unknown(why),
    }
}

/// The SinceVersion cell read (INTERNAL_GRAMMAR.md, "Column 3 -
/// SinceVersion"): a version; `fn:Extension(X)` or `fn:Extension(X,v)`, an
/// extension's; or `fn:Eval(fn:Extension(X,v) || w)`, an extension's from
/// `v` and standard from `w`, which with no extension allowed (TestGrammar
/// without `--extensions`) is `w`.
pub fn since(cell: &str) -> Since {
    if let Some(v) = version(cell) {
        return Since::In(v);
    }
    let unread = || Since::Unknown(cell.to_string());
    let Ok(Expr::Call(f, args)) = parse(cell) else {
        return unread();
    };
    match (f.as_str(), &args[..]) {
        ("fn:Extension", [Expr::Path(name)]) => Since::Extension(name.text.clone()),
        ("fn:Extension", [Expr::Path(name), Expr::Real(v)]) if version(v).is_some() => {
            Since::Extension(name.text.clone())
        }
        ("fn:Eval", [Expr::Or(items)]) => match &items[..] {
            [Expr::Call(g, _), Expr::Real(w)] if g == "fn:Extension" => {
                version(w).map_or_else(unread, Since::In)
            }
            _ => unread(),
        },
        _ => unread(),
    }
}

/// `wraps` at the file's version: `fn:SinceVersion`, `fn:BeforeVersion`
/// and `fn:IsPDFVersion` let the item in at their versions only;
/// `fn:Deprecated(v,X)` lets it in at every version, deprecated from `v`
/// on (INTERNAL_GRAMMAR.md, "Predicates (declarative functions)":
/// deprecation "is allowed/permitted but is strongly recommended
/// against"); and `fn:Extension` keeps it out, no extension being
/// allowed.
pub fn gate(wraps: &[Wrap], file: PdfVersion) -> Gate {
    let mut deprecated: Option<PdfVersion> = None;
    for wrap in wraps {
        // The loader takes no wrap whose version doesn't read.
        let allowed = match (wrap.func, version(&wrap.arg)) {
            (WrapFn::SinceVersion, Some(v)) => file >= v,
            (WrapFn::BeforeVersion, Some(v)) => file < v,
            (WrapFn::IsPDFVersion, Some(v)) => file == v,
            (WrapFn::Deprecated, Some(v)) => {
                if file >= v {
                    deprecated = Some(deprecated.map_or(v, |d| d.min(v)));
                }
                true
            }
            (WrapFn::Extension, _) | (_, None) => false,
        };
        if !allowed {
            return Gate::Excluded;
        }
    }
    deprecated.map_or(Gate::Allowed, Gate::AllowedDeprecated)
}

/// [`holds`], when every function in `text` is [`SUPPORTED`]: else
/// unknown, by the first that isn't, even where the rest would decide
/// it. TestGrammar takes a predicate it hasn't wholly implemented as no
/// verdict either way (its `WasFullyImplemented`, which ParseObjects.cpp's
/// `check_everything` reads); the checker's rules read SpecialCase and
/// PossibleValues so.
pub fn holds_fully(text: &str, cx: &Cx) -> Truth {
    match parse(text) {
        Ok(expr) => match unsupported(&expr) {
            Some(f) => Truth::Unknown(f.to_string()),
            None => truth(&expr, cx).truth(true),
        },
        Err(_) => Truth::Unknown(text.to_string()),
    }
}

/// The first function in `e` that isn't [`SUPPORTED`].
fn unsupported(e: &Expr) -> Option<&str> {
    match e {
        Expr::Call(f, _) if !SUPPORTED.contains(&f.as_str()) => Some(f),
        Expr::Call(_, args) | Expr::And(args) | Expr::Or(args) => args.iter().find_map(unsupported),
        Expr::Group(inner) => unsupported(inner),
        Expr::Cmp(_, a, b) => unsupported(a).or_else(|| unsupported(b)),
        Expr::Arith(first, rest) => {
            unsupported(first).or_else(|| rest.iter().find_map(|(_, e)| unsupported(e)))
        }
        Expr::Path(_)
        | Expr::Int(_)
        | Expr::Real(_)
        | Expr::Str(_)
        | Expr::Bool(_)
        | Expr::Null => None,
    }
}

/// `text`, the whole of it, parsed.
pub fn parse(text: &str) -> Result<Expr> {
    let mut p = Parser {
        text,
        at: 0,
        depth: 0,
    };
    let expr = p.expr().with_context(|| format!("{text:?}"))?;
    ensure!(
        p.at == text.len(),
        "{text:?}: {:?} after a whole predicate",
        p.rest()
    );
    Ok(expr)
}

/// [`Truth`] as evaluation carries it, with one more: vacant, a statement
/// that doesn't apply, which `&&` and `||` go on without.
enum Tri {
    Is(bool),
    Vacant,
    Unknown(String),
}

impl Tri {
    /// As a [`Truth`], `vacant` what a vacant one comes to.
    fn truth(self, vacant: bool) -> Truth {
        match self {
            Tri::Is(true) => Truth::True,
            Tri::Is(false) => Truth::False,
            Tri::Vacant if vacant => Truth::True,
            Tri::Vacant => Truth::False,
            Tri::Unknown(why) => Truth::Unknown(why),
        }
    }
}

/// What `e` comes to as a condition.
fn truth(e: &Expr, cx: &Cx) -> Tri {
    match e {
        Expr::Group(inner) => truth(inner, cx),
        Expr::And(items) => join(items, false, cx),
        Expr::Or(items) => join(items, true, cx),
        Expr::Cmp(op, a, b) => compare(*op, a, b, cx),
        Expr::Call(f, args) => call(f, args, cx),
        // `Key` means the key is present (INTERNAL_GRAMMAR.md, "Validation
        // of predicates").
        Expr::Path(p) if !p.value => present(p, cx),
        Expr::Bool(b) => Tri::Is(*b),
        Expr::Arith(_, ops) => Tri::Unknown(arithmetic(ops)),
        Expr::Path(_) | Expr::Int(_) | Expr::Real(_) | Expr::Str(_) | Expr::Null => {
            Tri::Unknown(e.to_string())
        }
    }
}

/// `items` joined by `||` (`or`) or `&&`: decided by the first item that
/// decides it, true for `||` and false for `&&`; else unknown when one
/// is, by the first that is. Vacant items are left out, and when all are,
/// it's vacant.
fn join(items: &[Expr], or: bool, cx: &Cx) -> Tri {
    let mut unknown = None;
    let mut vacant = true;
    for item in items {
        match truth(item, cx) {
            Tri::Is(b) if b == or => return Tri::Is(or),
            Tri::Is(_) => vacant = false,
            Tri::Vacant => {}
            Tri::Unknown(why) => {
                unknown.get_or_insert(why);
            }
        }
    }
    match unknown {
        Some(why) => Tri::Unknown(why),
        None if vacant => Tri::Vacant,
        None => Tri::Is(!or),
    }
}

/// A function's truth: each in [`SUPPORTED`] as INTERNAL_GRAMMAR.md's
/// "Predicates (declarative functions)" defines it, any other unknown.
fn call(f: &str, args: &[Expr], cx: &Cx) -> Tri {
    match (f, args) {
        // From `v` on; or the statement, which applies from `v` on.
        ("fn:SinceVersion", [v, statement @ ..]) => {
            at_version(f, v, statement, cx, |file, v| file >= v)
        }
        // Before `v`; or the statement, which applies before `v` only.
        ("fn:BeforeVersion", [v, statement @ ..]) => {
            at_version(f, v, statement, cx, |file, v| file < v)
        }
        // At `v`; or the statement, which applies at `v` only.
        ("fn:IsPDFVersion", [v, statement @ ..]) => {
            at_version(f, v, statement, cx, |file, v| file == v)
        }
        // Under the extension, which no file sheaf writes claims
        // (TestGrammar without `--extensions`): false; and the statement
        // under it applies nowhere.
        ("fn:Extension", [_]) => Tri::Is(false),
        ("fn:Extension", [_, _]) => Tri::Vacant,
        // The statement, deprecated from `v`, and so still allowed.
        ("fn:Deprecated", [v, statement]) if version_of(v).is_some() => truth(statement, cx),
        // The expression; the condition the key is required under; the
        // condition it must be direct or indirect under, always without
        // one.
        ("fn:Eval" | "fn:IsRequired", [e]) => truth(e, cx),
        ("fn:MustBeDirect" | "fn:MustBeIndirect", []) => Tri::Is(true),
        ("fn:MustBeDirect" | "fn:MustBeIndirect", [cond]) => truth(cond, cx),
        ("fn:Not", [e]) => match truth(e, cx) {
            Tri::Is(b) => Tri::Is(!b),
            other => other,
        },
        // With a condition: when the key is present, the condition; else
        // it asks nothing.
        ("fn:IsPresent", [k]) => is_present(k, cx),
        ("fn:IsPresent", [k, cond]) => match is_present(k, cx) {
            Tri::Is(true) => truth(cond, cx),
            Tri::Is(false) => Tri::Vacant,
            other => other,
        },
        ("fn:NotStandard14Font", []) => Tri::Is(not_standard_14(cx)),
        // A bit of the row's own value, or a run of them, clear or set.
        ("fn:BitClear" | "fn:BitSet", [bit]) => bits(f, bit, bit, cx),
        ("fn:BitsClear" | "fn:BitsSet", [low, high]) => bits(f, low, high, cx),
        _ => Tri::Unknown(f.to_string()),
    }
}

/// Whether the row's container is a Type 1 font (`/Type /Font /Subtype
/// /Type1`) whose `/BaseFont` isn't one of the standard 14 fonts' names
/// (ISO 32000-1 9.6.2.2), as INTERNAL_GRAMMAR.md's `fn:NotStandard14Font`
/// asks.
fn not_standard_14(cx: &Cx) -> bool {
    let name = |key: &[u8]| value_at(cx.container, At::Key(key), cx)?.as_name().ok();
    name(b"Type") == Some(b"Font")
        && name(b"Subtype") == Some(b"Type1")
        && !name(b"BaseFont")
            .is_some_and(|base| Std14::ALL.iter().any(|f| f.name().as_bytes() == base))
}

/// Whether bits `low` to `high` of the row's own value, a bitmask whose
/// lowest bit is 1, are all clear (`f` `fn:BitClear` or `fn:BitsClear`)
/// or all set: false for a value that doesn't fit in 32 bits, which each
/// asserts it does, a negative one fitting as its two's complement.
fn bits(f: &str, low: &Expr, high: &Expr, cx: &Cx) -> Tri {
    let unknown = || Tri::Unknown(f.to_string());
    let (&Expr::Int(low), &Expr::Int(high)) = (low, high) else {
        return unknown();
    };
    let (Ok(low), Ok(high)) = (u32::try_from(low), u32::try_from(high)) else {
        return unknown();
    };
    if !(1 <= low && low <= high && high <= 32) {
        return unknown();
    }
    let Some(&Object::Integer(value)) = value_at(cx.container, cx.at, cx) else {
        return unknown();
    };
    let value = u32::try_from(value)
        .ok()
        .or_else(|| i32::try_from(value).ok().map(i32::cast_unsigned));
    let Some(value) = value else {
        return Tri::Is(false);
    };
    let mask = (u32::MAX >> (32 - (high - low + 1))) << (low - 1);
    Tri::Is(match f {
        "fn:BitClear" | "fn:BitsClear" => value & mask == 0,
        _ => value & mask == mask,
    })
}

/// A version predicate `f` of `v`: whether the file's version `applies`,
/// or with a statement, the statement where it applies.
fn at_version(
    f: &str,
    v: &Expr,
    statement: &[Expr],
    cx: &Cx,
    applies: fn(PdfVersion, PdfVersion) -> bool,
) -> Tri {
    let Some(v) = version_of(v) else {
        return Tri::Unknown(f.to_string());
    };
    match statement {
        [] => Tri::Is(applies(cx.version, v)),
        [statement] if applies(cx.version, v) => truth(statement, cx),
        [_] => Tri::Vacant,
        _ => Tri::Unknown(f.to_string()),
    }
}

/// `fn:IsPresent`'s first argument: a key or path, or an array's index
/// (INTERNAL_GRAMMAR.md, "Column 1 - Key": an annotation's color's
/// `fn:IsPresent(2)`), whether the object has it; else an expression, the
/// form that asks the row be present when the expression is true. For a
/// present row, the only kind a SpecialCase is read for, that comes to the
/// expression (TestGrammar reduces it so).
fn is_present(k: &Expr, cx: &Cx) -> Tri {
    match k {
        Expr::Path(p) if !p.value => present(p, cx),
        Expr::Int(i) if *i >= 0 => {
            let index = Path {
                base: Base::Here,
                keys: vec![i.to_string()],
                value: false,
                text: i.to_string(),
            };
            present(&index, cx)
        }
        _ => truth(k, cx),
    }
}

/// Whether the object has what `p` names, null being absent (ISO 32000-1
/// 7.3.7). A path ending in `*` asks for any entry of the dictionary or
/// array before it, and not of a stream's dictionary: the rows that ask
/// (an annotation's AS, `AP::N::*`) mean a dictionary of appearance
/// states, which INTERNAL_GRAMMAR.md's "Proposals for future predicates"
/// would write `fn:IsDictionary(AP::N)`.
fn present(p: &Path, cx: &Cx) -> Tri {
    let found = match p.keys.split_last() {
        Some((last, before)) if last == "*" && !before.is_empty() => {
            walk(p, before, cx).map(|o| o.is_some_and(|o| has_entry(o, cx)))
        }
        _ => walk(p, &p.keys, cx).map(|o| o.is_some()),
    };
    match found {
        Ok(found) => Tri::Is(found),
        Err(why) => Tri::Unknown(why),
    }
}

/// The object at the end of `keys` from `p`'s base: none when one of them
/// isn't there; an error, `p` as written, for a `*` past the start, which
/// only presence reads, and for a step that can't be told ([`step`]).
fn walk<'a>(p: &Path, keys: &[String], cx: &Cx<'a>) -> Result<Option<&'a Object>, String> {
    let (mut at, keys) = match (p.base, keys) {
        (Base::Here, [first, rest @ ..]) if wild(first) => {
            (value_at(cx.container, cx.at, cx), rest)
        }
        (Base::Here, _) => (Some(cx.container), keys),
        (Base::Parent(n), _) => (
            n.checked_sub(1).and_then(|n| cx.parents.get(n)).copied(),
            keys,
        ),
        (Base::Trailer, [first, rest @ ..]) => {
            let entry = cx.trailer.get(first.as_bytes()).ok();
            (entry.and_then(|o| deref(o, cx)), rest)
        }
        (Base::Trailer, []) => return Err(p.text.clone()),
    };
    for key in keys {
        if wild(key) {
            return Err(p.text.clone());
        }
        at = match at {
            Some(o) => step(o, key, cx).ok_or_else(|| p.text.clone())?,
            None => None,
        };
    }
    Ok(at)
}

/// Whether `key` is `*` or a repeating row's `1*`.
fn wild(key: &str) -> bool {
    key.ends_with('*')
}

/// `o`'s entry under `key`, an array's element when it's an index: none
/// when `o` is null or hasn't it. A name into an array, or any key into a
/// value that isn't an array, a dictionary or a stream, can't be told
/// (INTERNAL_GRAMMAR.md, "Parameters to predicates"): itself none. The
/// model's `parent::Pg` from a structure element's `/K` array's object
/// reference is one: its parent is the array.
fn step<'a>(o: &'a Object, key: &str, cx: &Cx<'a>) -> Option<Option<&'a Object>> {
    let at = match deref(o, cx) {
        None => return Some(None),
        Some(Object::Array(_)) => At::Index(key.parse().ok()?),
        Some(Object::Dictionary(_) | Object::Stream(_)) => At::Key(key.as_bytes()),
        Some(_) => return None,
    };
    Some(value_at(o, at, cx))
}

/// `o`'s entry at `at`, a stream's from its dictionary; none for null.
fn value_at<'a>(o: &'a Object, at: At, cx: &Cx<'a>) -> Option<&'a Object> {
    let entry = match (deref(o, cx)?, at) {
        (Object::Dictionary(d), At::Key(key)) => d.get(key).ok(),
        (Object::Stream(s), At::Key(key)) => s.dict.get(key).ok(),
        (Object::Array(a), At::Index(i)) => a.get(i),
        _ => None,
    }?;
    deref(entry, cx)
}

/// `o` with its references followed, at most [`HOPS`]: none for null, or
/// for a reference to an object the file hasn't, which is null (ISO
/// 32000-1 7.3.10).
fn deref<'a>(mut o: &'a Object, cx: &Cx<'a>) -> Option<&'a Object> {
    for _ in 0..HOPS {
        match o {
            Object::Reference(id) => o = (cx.resolve)(*id)?,
            Object::Null => return None,
            _ => return Some(o),
        }
    }
    None
}

/// Whether `o` is a dictionary or array with an entry that isn't null.
fn has_entry<'a>(o: &'a Object, cx: &Cx<'a>) -> bool {
    match o {
        Object::Dictionary(d) => d.iter().any(|(_, v)| deref(v, cx).is_some()),
        Object::Array(a) => a.iter().any(|v| deref(v, cx).is_some()),
        _ => false,
    }
}

/// A value a comparison reads.
enum Scalar {
    Int(i64),
    Real(f64),
    Name(Vec<u8>),
    Str(String),
    Bool(bool),
}

impl Scalar {
    fn number(&self) -> Option<f64> {
        match *self {
            Scalar::Int(i) => Some(i as f64),
            Scalar::Real(r) => Some(r),
            _ => None,
        }
    }
}

/// `a op b`: names, strings and booleans compared for equality, numbers
/// also in order, an integer standing for a number (INTERNAL_GRAMMAR.md,
/// "Validation of predicates"); unknown for anything else, or a side that
/// can't be read.
fn compare(op: Cmp, a: &Expr, b: &Expr, cx: &Cx) -> Tri {
    let (x, y) = match (operand(a, cx), operand(b, cx)) {
        (Ok(x), Ok(y)) => (x, y),
        (Err(why), _) | (_, Err(why)) => return Tri::Unknown(why),
    };
    let numbers = x.number().zip(y.number());
    let equal = match (&x, &y) {
        (Scalar::Int(x), Scalar::Int(y)) => Some(x == y),
        (Scalar::Name(x), Scalar::Name(y)) => Some(x == y),
        (Scalar::Str(x), Scalar::Str(y)) => Some(x == y),
        (Scalar::Bool(x), Scalar::Bool(y)) => Some(x == y),
        _ => numbers.map(|(x, y)| (x - y).abs() <= TOLERANCE),
    };
    let order = match (&x, &y) {
        (Scalar::Int(x), Scalar::Int(y)) => Some(x.cmp(y)),
        _ => numbers.and_then(|(x, y)| x.partial_cmp(&y)),
    };
    let holds = match op {
        Cmp::Eq => equal,
        Cmp::Ne => equal.map(|equal| !equal),
        _ => order.map(|o| op.holds(o)),
    };
    holds.map_or_else(|| Tri::Unknown(format!("{a}{}{b}", op.symbol())), Tri::Is)
}

/// What `e` comes to as a comparison's side, or what's in the way.
fn operand(e: &Expr, cx: &Cx) -> Result<Scalar, String> {
    match e {
        Expr::Group(inner) => operand(inner, cx),
        Expr::Int(i) => Ok(Scalar::Int(*i)),
        Expr::Real(r) => r.parse().map(Scalar::Real).map_err(|_| r.clone()),
        Expr::Str(s) => Ok(Scalar::Str(s.clone())),
        Expr::Bool(b) => Ok(Scalar::Bool(*b)),
        // A bare word compared is a name: `@Type==Template`.
        Expr::Path(p) if !p.value => match (p.base, &p.keys[..]) {
            (Base::Here, [name]) => Ok(Scalar::Name(name.as_bytes().to_vec())),
            _ => Err(p.text.clone()),
        },
        Expr::Path(p) => walk(p, &p.keys, cx)?
            .and_then(scalar)
            .ok_or_else(|| p.text.clone()),
        Expr::Call(f, args) => match (f.as_str(), &args[..]) {
            ("fn:Eval", [e]) => operand(e, cx),
            // The rectangle's width and height, its corners any two
            // opposite ones (ISO 32000-1 7.9.5).
            ("fn:RectWidth", [k]) => {
                rect(k, cx).map(|[x0, _, x1, _]| Scalar::Real((x1 - x0).abs()))
            }
            ("fn:RectHeight", [k]) => {
                rect(k, cx).map(|[_, y0, _, y1]| Scalar::Real((y1 - y0).abs()))
            }
            ("fn:ArrayLength", [k]) => array(k, cx).map(|a| Scalar::Int(a.len() as i64)),
            _ => Err(f.clone()),
        },
        Expr::Arith(_, ops) => Err(arithmetic(ops)),
        Expr::And(_) | Expr::Or(_) | Expr::Cmp(..) | Expr::Null => Err(e.to_string()),
    }
}

/// `o` as a comparison reads it: none for an array, a dictionary or a
/// stream.
fn scalar(o: &Object) -> Option<Scalar> {
    Some(match o {
        Object::Integer(i) => Scalar::Int(*i),
        Object::Real(r) => Scalar::Real(*r),
        Object::Name(name) => Scalar::Name(name.clone()),
        Object::String(s, _) => Scalar::Str(text_string(s)),
        Object::Boolean(b) => Scalar::Bool(*b),
        _ => return None,
    })
}

/// The rectangle at `k`, a key or path: four numbers.
fn rect(k: &Expr, cx: &Cx) -> Result<[f64; 4], String> {
    let numbers: Option<Vec<f64>> = (array(k, cx)?.iter())
        .map(|o| deref(o, cx).and_then(|o| scalar(o)?.number()))
        .collect();
    numbers
        .and_then(|n| <[f64; 4]>::try_from(n).ok())
        .ok_or_else(|| k.to_string())
}

/// The array at `k`, a key or path.
fn array<'a>(k: &Expr, cx: &Cx<'a>) -> Result<&'a [Object], String> {
    match k {
        Expr::Path(p) if !p.value => match walk(p, &p.keys, cx)? {
            Some(Object::Array(a)) => Ok(a),
            _ => Err(p.text.clone()),
        },
        Expr::Call(f, _) => Err(f.clone()),
        _ => Err(k.to_string()),
    }
}

/// Arithmetic, as the unknown it comes to: its first operator.
fn arithmetic(ops: &[(Arith, Expr)]) -> String {
    let first = ops.first().map(|(op, _)| op.symbol().trim());
    first.unwrap_or("arithmetic").to_string()
}

/// `e`'s version, when it's a version as the model writes one.
fn version_of(e: &Expr) -> Option<PdfVersion> {
    match e {
        Expr::Real(text) => version(text),
        _ => None,
    }
}

impl Cmp {
    /// Every comparison, the longer of those that start alike first.
    const ALL: [Cmp; 6] = [Cmp::Eq, Cmp::Ne, Cmp::Le, Cmp::Ge, Cmp::Lt, Cmp::Gt];

    pub fn symbol(self) -> &'static str {
        match self {
            Cmp::Eq => "==",
            Cmp::Ne => "!=",
            Cmp::Lt => "<",
            Cmp::Le => "<=",
            Cmp::Gt => ">",
            Cmp::Ge => ">=",
        }
    }

    /// Whether it holds of two values in `order`.
    fn holds(self, order: Ordering) -> bool {
        match self {
            Cmp::Eq => order.is_eq(),
            Cmp::Ne => order.is_ne(),
            Cmp::Lt => order.is_lt(),
            Cmp::Le => order.is_le(),
            Cmp::Gt => order.is_gt(),
            Cmp::Ge => order.is_ge(),
        }
    }
}

impl Arith {
    const ALL: [Arith; 4] = [Arith::Add, Arith::Sub, Arith::Mul, Arith::Mod];

    /// As the model writes it: `-` and `*` spaced, to tell them from a
    /// negative number and a wildcard, and `mod` (INTERNAL_GRAMMAR.md,
    /// "Predicates (declarative functions)").
    pub fn symbol(self) -> &'static str {
        match self {
            Arith::Add => "+",
            Arith::Sub => " - ",
            Arith::Mul => " * ",
            Arith::Mod => " mod ",
        }
    }
}

/// A predicate as the model writes it, which every one in the model
/// prints back as.
impl fmt::Display for Expr {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let join = |f: &mut fmt::Formatter, items: &[Expr], sep: &str| -> fmt::Result {
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    f.write_str(sep)?;
                }
                write!(f, "{item}")?;
            }
            Ok(())
        };
        match self {
            Expr::Call(name, args) => {
                write!(f, "{name}(")?;
                join(f, args, ",")?;
                f.write_str(")")
            }
            Expr::Group(inner) => write!(f, "({inner})"),
            Expr::And(items) => join(f, items, AND),
            Expr::Or(items) => join(f, items, OR),
            Expr::Cmp(op, a, b) => write!(f, "{a}{}{b}", op.symbol()),
            Expr::Arith(first, rest) => {
                write!(f, "{first}")?;
                for (op, e) in rest {
                    write!(f, "{}{e}", op.symbol())?;
                }
                Ok(())
            }
            Expr::Path(p) => f.write_str(&p.text),
            Expr::Int(i) => write!(f, "{i}"),
            Expr::Real(text) => f.write_str(text),
            Expr::Str(s) => write!(f, "'{s}'"),
            Expr::Bool(b) => write!(f, "{b}"),
            Expr::Null => f.write_str("null"),
        }
    }
}

/// A recursive descent over a predicate's text, as INTERNAL_GRAMMAR.md's
/// "Predicates (declarative functions)" spaces it: a space each side of
/// `&&`, `||`, `-`, `*` and `mod`, and nowhere else.
struct Parser<'s> {
    text: &'s str,
    at: usize,
    /// How many terms the parser is inside.
    depth: usize,
}

impl Parser<'_> {
    fn rest(&self) -> &str {
        &self.text[self.at..]
    }

    fn eat(&mut self, token: &str) -> bool {
        let ate = self.rest().starts_with(token);
        if ate {
            self.at += token.len();
        }
        ate
    }

    /// The one of `ops` the text goes on with.
    fn peek<T: Copy, const N: usize>(
        &self,
        ops: [T; N],
        symbol: fn(T) -> &'static str,
    ) -> Option<T> {
        ops.into_iter()
            .find(|&op| self.rest().starts_with(symbol(op)))
    }

    fn expect(&mut self, token: &str) -> Result<()> {
        ensure!(
            self.eat(token),
            "{token:?} expected at {}, not {:?}",
            self.at,
            self.rest()
        );
        Ok(())
    }

    /// Comparisons joined by `&&` or by `||`: mixing them takes brackets
    /// (INTERNAL_GRAMMAR.md, "Validation of predicates").
    fn expr(&mut self) -> Result<Expr> {
        let first = self.comparison()?;
        let (op, other, joined): (_, _, fn(Vec<Expr>) -> Expr) = match self.rest() {
            rest if rest.starts_with(AND) => (AND, OR, Expr::And),
            rest if rest.starts_with(OR) => (OR, AND, Expr::Or),
            _ => return Ok(first),
        };
        let mut items = vec![first];
        while self.eat(op) {
            items.push(self.comparison()?);
        }
        ensure!(
            !self.rest().starts_with(other),
            "&& and || mixed without brackets at {}",
            self.at
        );
        Ok(joined(items))
    }

    /// Arithmetic, maybe compared.
    fn comparison(&mut self) -> Result<Expr> {
        let left = self.arith()?;
        let Some(op) = self.peek(Cmp::ALL, Cmp::symbol) else {
            return Ok(left);
        };
        self.at += op.symbol().len();
        let right = self.arith()?;
        Ok(Expr::Cmp(op, Box::new(left), Box::new(right)))
    }

    /// Terms joined by arithmetic.
    fn arith(&mut self) -> Result<Expr> {
        let first = self.term()?;
        let mut rest = Vec::new();
        while let Some(op) = self.peek(Arith::ALL, Arith::symbol) {
            self.at += op.symbol().len();
            rest.push((op, self.term()?));
        }
        Ok(if rest.is_empty() {
            first
        } else {
            Expr::Arith(Box::new(first), rest)
        })
    }

    /// A bracketed expression, a call, a string or a word, inside no more
    /// than [`DEPTH`] others.
    fn term(&mut self) -> Result<Expr> {
        ensure!(
            self.depth < DEPTH,
            "nested deeper than {DEPTH} at {}",
            self.at
        );
        self.depth += 1;
        let term = self.term_within();
        self.depth -= 1;
        term
    }

    fn term_within(&mut self) -> Result<Expr> {
        if self.eat("(") {
            let inner = self.expr()?;
            self.expect(")")?;
            return Ok(Expr::Group(Box::new(inner)));
        }
        if self.eat("fn:") {
            return self.call();
        }
        let text = self.text;
        if self.eat("'") {
            // Never empty, and without escapes (TestGrammar's `ArlString`).
            let rest = &text[self.at..];
            let end = rest.find('\'').context("a string left open")?;
            ensure!(end > 0, "an empty string at {}", self.at);
            self.at += end + 1;
            return Ok(Expr::Str(rest[..end].to_string()));
        }
        let rest = &text[self.at..];
        let len = rest
            .find(|c: char| !(c.is_ascii_alphanumeric() || "_.-*@:".contains(c)))
            .unwrap_or(rest.len());
        ensure!(len > 0, "a term expected at {}, not {rest:?}", self.at);
        self.at += len;
        word(&rest[..len])
    }

    /// `Name(...)`, after a call's `fn:`: none, one or two arguments
    /// (INTERNAL_GRAMMAR.md, "Validation of predicates").
    fn call(&mut self) -> Result<Expr> {
        let text = self.text;
        let rest = &text[self.at..];
        let len = (rest.find(|c: char| !c.is_ascii_alphanumeric())).unwrap_or(rest.len());
        let name = &rest[..len];
        ensure!(
            name.starts_with(|c: char| c.is_ascii_uppercase()),
            "fn: and a CamelCase name expected at {}",
            self.at
        );
        self.at += len;
        self.expect("(")?;
        let mut args = Vec::new();
        if !self.eat(")") {
            args.push(self.expr()?);
            if self.eat(",") {
                args.push(self.expr()?);
            }
            self.expect(")")?;
        }
        Ok(Expr::Call(format!("fn:{name}"), args))
    }
}

/// A word: `true`, `false` or `null`, an integer, a real, or else a path.
fn word(word: &str) -> Result<Expr> {
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    let unsigned = word.strip_prefix('-').unwrap_or(word);
    let real = unsigned
        .split_once('.')
        .is_some_and(|(whole, fraction)| digits(whole) && digits(fraction));
    Ok(match word {
        "true" => Expr::Bool(true),
        "false" => Expr::Bool(false),
        "null" => Expr::Null,
        _ if digits(unsigned) => Expr::Int(
            word.parse()
                .with_context(|| format!("{word} is out of range"))?,
        ),
        _ if real => Expr::Real(word.to_string()),
        _ => Expr::Path(path(word)?),
    })
}

/// `A::B::@C` as a path: `parent::`s or a `trailer::` first, then keys,
/// which only `parent` (the object itself) may go without.
fn path(text: &str) -> Result<Path> {
    let mut keys: Vec<&str> = text.split("::").collect();
    let mut value = false;
    if let Some(last) = keys.last_mut()
        && let Some(key) = last.strip_prefix('@')
    {
        *last = key;
        value = true;
    }
    let parents = keys.iter().take_while(|&&k| k == "parent").count();
    let (base, skip) = match keys[0] {
        _ if parents > 0 => (Base::Parent(parents), parents),
        "trailer" => (Base::Trailer, 1),
        _ => (Base::Here, 0),
    };
    let mut keys: Vec<String> = keys[skip..].iter().map(|k| k.to_string()).collect();
    if base == Base::Trailer && keys.first().is_some_and(|k| k == "Catalog") {
        keys[0] = "Root".to_string();
    }
    let parent = matches!(base, Base::Parent(_));
    ensure!(!keys.is_empty() || parent, "{text:?}: a path to nothing");
    if let Some(key) = keys.iter().find(|k| !key_like(k)) {
        bail!("{text:?}: {key:?} isn't a key");
    }
    Ok(Path {
        base,
        keys,
        value,
        text: text.to_string(),
    })
}

/// Whether a path may go through `key`: a key or an index, `*`, or a
/// repeating row's `1*` (INTERNAL_GRAMMAR.md, "Column 1 - Key"); never
/// `parent` or `trailer` past the path's start.
fn key_like(key: &str) -> bool {
    let plain =
        !key.is_empty() && (key.bytes()).all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b));
    let repeat = (key.strip_suffix('*')).is_some_and(|n| n.bytes().all(|b| b.is_ascii_digit()));
    !matches!(key, "parent" | "trailer") && (plain || repeat)
}
