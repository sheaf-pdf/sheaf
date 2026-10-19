//! What every written file is checked for before it's handed out (M10.5
//! §11.9), failing closed: a save whose output breaks one stops with
//! [`SaveStop::Failed`](super::SaveStop::Failed), and nothing is written.
//!
//! On the Rewrite's graph, before it's serialized:
//! - **Copy-on-write** (§11.5), under stable numbering: an emitted object
//!   is written at a number the primary uses only when its key is the
//!   primary object it is ([`Emission::owned`]: a page, an annotation, a
//!   field, a bookmark, the outline's root or the form), or one of the
//!   file's root objects, which the state owns (the catalog, the page
//!   tree's nodes, the page labels) ([`graph`]). And every other object
//!   at a number the primary uses is the primary's object there, as its
//!   source reads it ([`copied`]), but what `keep_only` reported changing,
//!   the annotations [`release_written`] let go of from a dropped page's
//!   `/Annots`, and an object naming one moved from a number past what a
//!   file may use ([`Graph::followed`]). An object copied as its bytes
//!   were is the source's, and isn't compared.
//! - **Every reference resolves**: to an object the file has, or the
//!   encryption dictionary. A reference to a number the primary never
//!   used and the save didn't give (none past what a file may use), or to
//!   an object it lists and can't read, is the input's damage, and passes
//!   ([`links`]).
//! - **Reachability**: the pages and page tree nodes the file reaches are
//!   the written tree's and the page templates `/Names /Templates` names
//!   (which `keep_only` keeps), and every page of the tree is there
//!   ([`links`]). An export's post-pass runs after these, and they're
//!   checked again on the graph it leaves.
//!
//! On the written bytes ([`output`]):
//! - **Forms**: when the file has a form the save wrote any of anew (its
//!   dictionary, a field or a widget, or what `keep_only` changed of
//!   them), `forms::check` finds nothing no input form had: no widget,
//!   field or font of a kind of finding (widgets no field reaches or on no
//!   page, shared widgets, wrong `/P` or `/Parent`, missing fonts,
//!   duplicate names) that no input's finding of the kind names, under
//!   its name or as a field sheaf put under a parent of its own; and no
//!   damaged tree unless an input's was. A damaged input's names aren't
//!   its fields' (its tree couldn't be read whole), so what it has a
//!   finding of is compared by kind. What save policy decides and
//!   asks about first isn't checked here: signatures a Rewrite breaks,
//!   `/SigFlags` and the XFA copy kept as read, earlier revisions; nor a
//!   value with no appearance, which a page delete leaves where the chosen
//!   button's widget went, and which fills (M10) give appearances.
//! - **Reading back**, in debug builds: every object reads back from the
//!   bytes as the graph has it, through the file's keys when it keeps its
//!   encryption, so no string or stream went out in the clear, and an
//!   object copied as its bytes were says what the checked graph says.
//!
//! On an Append's section ([`section`]):
//! - **Copy-on-write**: an object is written over a number the saved file
//!   uses only when it's the key the file's map numbers there, or the
//!   primary object it is (a model-owned identity, §11.5), and no key of
//!   the map stays at that number: none is but, for a field, the widget
//!   merged with it now, a key the section writes elsewhere, or a record
//!   the state saved no longer has.
//! - **Every reference resolves**: to an object of the section, one the
//!   saved file's map numbers, a number in use in the file, or the
//!   encryption dictionary; or to a number the primary never used, the
//!   input's damage, but none the section gave (at or past its first new
//!   number, which is past every primary number it names).
//! - **Forms**, as on a whole file, on the file with the section after it,
//!   when the section writes any of the form ([`form`]).
//! - **Reading back**, in debug builds: every object of the section reads
//!   back from the file with it as written ([`reads_back_section`]).
//!
//! [`release_written`]: super::graph::release_written
//! [`Graph::followed`]: super::graph::Graph::followed
//!
//! [`Emission::owned`]: super::emit::Emission::owned

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use anyhow::{Result, bail};
use lopdf::{Dictionary, Document as Pdf, Object, ObjectId};

use super::graph::Graph;
use super::key::ObjKey;
use super::record::SaveRecord;
use crate::forms::{Finding, Report};
use crate::model::DocState;
use crate::source::Source;

/// The primary a Rewrite's graph was made over, as the checks see it.
pub(crate) struct Primary<'a> {
    /// The numbers its table lists in use.
    pub(crate) in_use: &'a HashSet<u32>,
    /// Whether it reads object `id`: one it lists that doesn't read is
    /// its damage, which a reference to it keeps.
    pub(crate) reads: &'a (dyn Fn(ObjectId) -> bool + Sync),
    /// The first number the save gave a key new to it: past every number
    /// the primary and the previous save used.
    pub(crate) start: u32,
}

/// The Rewrite's `graph` over `primary`: copy-on-write's ownership when
/// `stable` (`owned` the keys written at the primary object they are),
/// and its [`links`].
pub(crate) fn graph(
    graph: &Graph,
    primary: &Primary,
    owned: &BTreeMap<ObjKey, ObjectId>,
    tree: &HashSet<ObjectId>,
    stable: bool,
    encrypt: Option<ObjectId>,
) -> Result<()> {
    if stable {
        for (&at, &key) in &graph.keys {
            if primary.in_use.contains(&at.0) && !key.is_root() && owned.get(&key) != Some(&at) {
                bail!(
                    "{key:?} would be written over the file's object {} {}",
                    at.0,
                    at.1
                );
            }
        }
    }
    links(graph, primary, tree, encrypt)
}

/// `graph`'s objects at the numbers the primary uses that are the
/// primary's own (no emitted key's), each the object `primary` reads
/// there, but those `changed` says a step of the Rewrite may change
/// (`keep_only`'s, and the like) or that are written as their bytes were:
/// copy-on-write (§11.5), under stable numbering. An object the source
/// can't read is its damage, and isn't compared. On a thread per core (8
/// at most).
pub(crate) fn copied(
    graph: &Graph,
    primary: &Source,
    in_use: &HashSet<u32>,
    changed: &(dyn Fn(ObjectId) -> bool + Sync),
) -> Result<()> {
    let own: Vec<(&ObjectId, &Object)> = (graph.pdf.objects.iter())
        .filter(|&(id, obj)| {
            in_use.contains(&id.0)
                && !graph.keys.contains_key(id)
                && !graph.deferred.contains_key(id)
                && !super::super::serialize::full::left_out(obj)
                && !changed(*id)
        })
        .collect();
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get().min(8));
    let chunk = own.len().div_ceil(threads).max(1024);
    let differs = std::thread::scope(|s| {
        let parts: Vec<_> = (own.chunks(chunk))
            .map(|part| {
                s.spawn(move || {
                    part.iter().find_map(|&(&id, obj)| {
                        let read = primary.pdf().object_unkept(id).ok()?;
                        (!plain_eq(obj, &read, false)).then_some(id)
                    })
                })
            })
            .collect();
        (parts.into_iter())
            .filter_map(|p| p.join().expect("checking doesn't panic"))
            .min()
    });
    if let Some(id) = differs {
        bail!(
            "the file's object {} {} would be written with other content",
            id.0,
            id.1
        );
    }
    Ok(())
}

/// `graph`'s references and pages over `primary`: every reference
/// resolved, and the pages reached the tree's (`tree`, its pages and
/// nodes) and the templates. `encrypt` is where the encryption dictionary
/// goes, when there's one.
pub(crate) fn links(
    graph: &Graph,
    primary: &Primary,
    tree: &HashSet<ObjectId>,
    encrypt: Option<ObjectId>,
) -> Result<()> {
    let in_use = primary.in_use;
    let objects = &graph.pdf.objects;
    let resolves = |r: ObjectId| objects.contains_key(&r) || Some(r) == encrypt;
    // A number the save gave, or an object the primary reads, that the
    // file hasn't got: a reference to anything else was the input's. A
    // save gives no number past what a file may use.
    let given = |n: u32| n >= primary.start && n < crate::serialize::NUMBERS;
    let lost =
        |r: ObjectId| !resolves(r) && (given(r.0) || (in_use.contains(&r.0) && (primary.reads)(r)));
    // What nothing reaches was pruned: every object is reached, and so is
    // every page among them. On a thread per core (8 at most).
    let all: Vec<(&ObjectId, &Object)> = objects.iter().collect();
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get().min(8));
    let chunk = all.len().div_ceil(threads).max(1024);
    let found = std::thread::scope(|s| {
        let parts: Vec<_> = (all.chunks(chunk))
            .map(|part| {
                let lost = &lost;
                s.spawn(move || {
                    let mut pages = Vec::new();
                    let mut dangling = None;
                    for &(&id, obj) in part {
                        if is_page_node(obj) {
                            pages.push(id);
                        }
                        refs(obj, &mut |r| {
                            if dangling.is_none() && lost(r) {
                                dangling = Some((id, r));
                            }
                        });
                    }
                    (pages, dangling)
                })
            })
            .collect();
        (parts.into_iter())
            .map(|p| p.join().expect("checking doesn't panic"))
            .collect::<Vec<_>>()
    });
    let mut reached = BTreeSet::new();
    for (pages, dangling) in found {
        if let Some((id, r)) = dangling {
            bail!(
                "object {} {} refers to {} {}, which the file hasn't got",
                id.0,
                id.1,
                r.0,
                r.1
            );
        }
        reached.extend(pages);
    }
    let mut trailer_refs = Vec::new();
    refs(&Object::Dictionary(graph.pdf.trailer.clone()), &mut |r| {
        trailer_refs.push(r)
    });
    if let Some(r) = trailer_refs.into_iter().find(|&r| lost(r)) {
        bail!(
            "the trailer refers to {} {}, which the file hasn't got",
            r.0,
            r.1
        );
    }
    let templates = templates(&graph.pdf);
    if let Some(stray) = reached
        .iter()
        .find(|id| !tree.contains(id) && !templates.contains(id))
    {
        bail!(
            "the file reaches page {} {}, which it doesn't keep",
            stray.0,
            stray.1
        );
    }
    if let Some(missing) = tree.iter().find(|id| !reached.contains(id)) {
        bail!(
            "page {} {} of the tree isn't in the file",
            missing.0,
            missing.1
        );
    }
    Ok(())
}

/// Each reference in `obj`, to `to`.
fn refs(obj: &Object, to: &mut dyn FnMut(ObjectId)) {
    match obj {
        Object::Reference(r) => to(*r),
        Object::Array(items) => items.iter().for_each(|o| refs(o, to)),
        Object::Dictionary(d) => d.iter().for_each(|(_, o)| refs(o, to)),
        Object::Stream(s) => s.dict.iter().for_each(|(_, o)| refs(o, to)),
        _ => {}
    }
}

/// Whether `obj` is a page or a node of a page tree.
fn is_page_node(obj: &Object) -> bool {
    let Object::Dictionary(d) = obj else {
        return false;
    };
    (d.get(b"Type").ok())
        .and_then(|t| t.as_name().ok())
        .is_some_and(|t| t == b"Page" || t == b"Pages")
}

/// The pages `pdf`'s `/Names /Templates` names.
fn templates(pdf: &Pdf) -> HashSet<ObjectId> {
    let mut out = HashSet::new();
    let deref = |o: &Object| -> Option<Object> {
        match o {
            Object::Reference(r) => pdf.objects.get(r).cloned(),
            o => Some(o.clone()),
        }
    };
    let tree = (pdf.catalog().ok())
        .and_then(|c| c.get(b"Names").ok())
        .and_then(deref)
        .and_then(|n| {
            n.as_dict()
                .ok()
                .and_then(|n| n.get(b"Templates").ok().cloned())
        });
    let mut stack: Vec<(Object, u32)> = tree.into_iter().map(|t| (t, 0)).collect();
    let mut seen = HashSet::new();
    while let Some((node, depth)) = stack.pop() {
        if depth > 32 || node.as_reference().is_ok_and(|r| !seen.insert(r)) {
            continue;
        }
        let Some(Object::Dictionary(d)) = deref(&node) else {
            continue;
        };
        if let Some(Object::Array(pairs)) = d.get(b"Names").ok().and_then(deref) {
            out.extend(
                pairs
                    .chunks(2)
                    .filter_map(|p| p.get(1)?.as_reference().ok()),
            );
        }
        if let Some(Object::Array(kids)) = d.get(b"Kids").ok().and_then(deref) {
            stack.extend(kids.into_iter().map(|k| (k, depth + 1)));
        }
    }
    out
}

/// How the file was written, for checking what it reads back as.
pub(crate) struct Written<'a> {
    pub(crate) bytes: &'a [u8],
    /// Whether the save wrote any of the form anew: else it's the file's
    /// form as it was, and isn't checked again.
    pub(crate) form_changed: bool,
    /// What it's read with: the primary's password, when it keeps its
    /// encryption.
    pub(crate) password: Option<&'a str>,
    pub(crate) encrypted: bool,
}

/// `out`, the file written from `graph`: its form checked against the
/// forms of `inputs` (each source the form's instances came from), when it
/// has one; in debug builds, every object read back as it was written.
pub(crate) fn output(out: &Written, graph: &Graph, inputs: &[&Source]) -> Result<()> {
    let has_form = (graph.pdf.catalog().ok()).is_some_and(|c| c.has(b"AcroForm"));
    if has_form && out.form_changed {
        let report = crate::forms::check(out.bytes, out.password)?;
        form(&report, inputs)?;
    }
    #[cfg(debug_assertions)]
    reads_back(out, graph).map_err(|e| match out.encrypted {
        true => e.context("it wasn't encrypted as written"),
        false => e,
    })?;
    #[cfg(not(debug_assertions))]
    let _ = out.encrypted;
    Ok(())
}

/// A written file's form, as `forms::check` found it (`report`), with
/// nothing the forms of `inputs` (each source the form's instances came
/// from) didn't have.
pub(crate) fn form(report: &Report, inputs: &[&Source]) -> Result<()> {
    let new = new_findings(report, inputs);
    if new.is_empty() {
        return Ok(());
    }
    let mut kinds: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for (kind, item) in &new {
        kinds.entry(kind).or_default().extend(*item);
    }
    let said: Vec<String> = (kinds.into_iter())
        .map(|(kind, items)| match items.len() {
            0 => kind.to_string(),
            _ => format!("{kind} ({})", shown(&items)),
        })
        .collect();
    bail!("the written form has {}", said.join(", "))
}

/// An Append's section, as the checks see it.
pub(crate) struct Section<'a> {
    /// Its objects, by where they're written, each with its key.
    pub(crate) objects: &'a BTreeMap<ObjectId, (ObjKey, Object)>,
    /// The saved file's.
    pub(crate) record: &'a SaveRecord,
    /// The keys written at the number of the primary's object they are.
    pub(crate) owned: &'a BTreeMap<ObjKey, ObjectId>,
    /// The first number given to a key new to the file: past every number
    /// the file uses and every primary number the section names.
    pub(crate) start: u32,
    /// The numbers the saved file's map has a key at.
    pub(crate) mapped: &'a HashSet<u32>,
    /// Each widget merged with its field, and the field: the map lists the
    /// pair's object by either.
    pub(crate) aliases: &'a [(ObjKey, ObjKey)],
    /// The state saved.
    pub(crate) target: &'a DocState,
    /// Whether the primary used number `n`: a reference to one it never
    /// did is its damage, which the section keeps.
    pub(crate) used: &'a dyn Fn(u32) -> bool,
}

/// What [`section`] looks up of a section `s` that writes a primary object
/// over a number its map lists another key at, found once.
struct Moved<'a> {
    /// The keys the section writes.
    written: HashSet<&'a ObjKey>,
    /// The map's keys at each number.
    at: HashMap<ObjectId, Vec<&'a ObjKey>>,
    /// The section's merged widgets, each with its field.
    merged: HashSet<(ObjKey, ObjKey)>,
}

impl<'a> Moved<'a> {
    fn new(s: &Section<'a>) -> Moved<'a> {
        let mut at: HashMap<ObjectId, Vec<&ObjKey>> = HashMap::new();
        for (k, &n) in &s.record.map.keys {
            at.entry(n).or_default().push(k);
        }
        Moved {
            written: s.objects.values().map(|(k, _)| k).collect(),
            at,
            merged: s.aliases.iter().copied().collect(),
        }
    }

    /// Whether every key the saved file's map has at `at`, where the
    /// section writes `key`, the primary object it is, has gone from there:
    /// the widget merged with `key`, a field, now (the field's object, which
    /// the map lists by its widget alone), a key the section writes
    /// elsewhere, or a record `target` no longer holds.
    fn from(&self, key: &ObjKey, at: ObjectId, target: &DocState) -> bool {
        (self.at.get(&at).into_iter().flatten()).all(|k| {
            self.merged.contains(&(**k, *key)) || self.written.contains(k) || !holds(target, k)
        })
    }
}

/// Whether `st` holds `key`'s record: false only for an annotation, a
/// field or a page it no longer has.
fn holds(st: &DocState, key: &ObjKey) -> bool {
    match *key {
        ObjKey::Annot(a) | ObjKey::AnnotPart(a, _) => st.annot(a).is_some(),
        ObjKey::Field(f) => st.field(f).is_some(),
        ObjKey::Page(p) => st.def(p).is_some(),
        _ => true,
    }
}

/// An Append's section: copy-on-write against the saved file, and every
/// reference resolved.
pub(crate) fn section(s: &Section) -> Result<()> {
    let map = &s.record.map;
    let mut moved = None;
    for (&at, (key, _)) in s.objects {
        if at.0 >= s.start || map.keys.get(key) == Some(&at) {
            continue;
        }
        let owned = key.of_state()
            && s.owned.get(key) == Some(&at)
            && (!s.mapped.contains(&at.0)
                || moved
                    .get_or_insert_with(|| Moved::new(s))
                    .from(key, at, s.target));
        if !owned {
            bail!(
                "{key:?} would be written over the file's object {} {}",
                at.0,
                at.1
            );
        }
    }
    let encrypt = s.record.crypt.as_ref().map(|c| c.at());
    // A number the section gave, which it writes: its new numbers start
    // past every number the file uses and every primary number it names,
    // so a reference at or past them to what it doesn't write is lost, as
    // the Rewrite's `given` is ([`links`]). None past what a file may use
    // is given.
    let given = |n: u32| n >= s.start && n < crate::serialize::NUMBERS;
    let resolves = |r: ObjectId| {
        s.objects.contains_key(&r)
            || Some(r) == encrypt
            || (!given(r.0)
                && (s.mapped.contains(&r.0) || s.record.present.contains(r.0) || !(s.used)(r.0)))
    };
    for (&id, (_, obj)) in s.objects {
        let mut dangling = None;
        refs(obj, &mut |r| {
            if dangling.is_none() && !resolves(r) {
                dangling = Some(r);
            }
        });
        if let Some(r) = dangling {
            bail!(
                "object {} {} refers to {} {}, which neither the section nor the file has",
                id.0,
                id.1,
                r.0,
                r.1
            );
        }
    }
    Ok(())
}

/// Every object of `objects`, an Append's section, read back as written
/// from `file`, the saved file with the section after it, read with
/// `password`: an object that reads otherwise went out unencrypted or
/// encrypted wrong.
#[cfg(debug_assertions)]
pub(crate) fn reads_back_section(
    file: impl lopdf::ByteSource + 'static,
    password: Option<&str>,
    objects: &BTreeMap<ObjectId, (ObjKey, Object)>,
) -> Result<()> {
    let read = lopdf::LazyPdf::open(file, password, &crate::source::Limits::DEFAULT.reader)?;
    for (&id, (_, obj)) in objects {
        let Ok(back) = read.object_unkept(id) else {
            bail!("object {} {} doesn't read back", id.0, id.1);
        };
        if !plain_eq(obj, &back, false) {
            bail!("object {} {} reads back otherwise", id.0, id.1);
        }
    }
    Ok(())
}

/// Whether forms::check's `finding` is one an emitter could make: what
/// save policy decides isn't.
fn checked(finding: &Finding) -> bool {
    !matches!(
        finding,
        Finding::EarlierValues { .. }
            | Finding::XfaMismatch { .. }
            | Finding::StaleSignature { .. }
            | Finding::StaleSigFlags { .. }
            | Finding::NoAppearance(_)
    )
}

/// What `finding` names: its widgets', fields' or fonts' names, each
/// on its own; None for a damaged tree, which names no field.
fn items(finding: &Finding) -> Vec<Option<&str>> {
    match finding {
        Finding::Orphans(names)
        | Finding::OffPage(names)
        | Finding::SharedWidgets(names)
        | Finding::WrongPage(names)
        | Finding::NullPage(names)
        | Finding::WrongParent(names)
        | Finding::DuplicateNames(names)
        | Finding::NoAppearance(names) => names.iter().map(|n| Some(n.as_str())).collect(),
        Finding::MissingFonts(fonts) => fonts.iter().map(|(f, _)| Some(f.as_str())).collect(),
        _ => vec![None],
    }
}

/// `report`'s findings that no input's report has, by kind and what each
/// names (None for what names nothing). A name an input's finding of the
/// kind has, or has under a parent of its own (`P.name`, sheaf's parent
/// of a form whose names clashed), was the input's. So was any of a kind
/// a damaged input's finding has: its tree couldn't be read whole, so the
/// names it gives aren't those its fields have once the tree is written
/// sound (0982696's shared kids, each written under one parent, name
/// otherwise the duplicates it had).
fn new_findings<'a>(
    report: &'a Report,
    inputs: &[&Source],
) -> Vec<(&'static str, Option<&'a str>)> {
    let found: Vec<(&'static str, Option<&str>)> = (report.findings.iter())
        .filter(|f| checked(f))
        .flat_map(|f| items(f).into_iter().map(move |item| (f.kind(), item)))
        .collect();
    if found.is_empty() {
        return found;
    }
    let mut had: HashSet<(&'static str, Option<String>)> = HashSet::new();
    let mut had_any: HashSet<&'static str> = HashSet::new();
    for src in inputs {
        if let Ok(input) = crate::forms::check(src.bytes(), src.password()) {
            let damaged = (input.findings.iter()).any(|f| matches!(f, Finding::Damaged(_)));
            for f in &input.findings {
                had.extend(
                    items(f)
                        .into_iter()
                        .map(|i| (f.kind(), i.map(str::to_string))),
                );
                if damaged {
                    had_any.insert(f.kind());
                }
            }
        }
    }
    let was_had = |kind: &'static str, item: Option<&str>| {
        if had_any.contains(kind) {
            return true;
        }
        let Some(name) = item else {
            return had.contains(&(kind, None));
        };
        let under = name.match_indices('.').map(|(at, _)| &name[at + 1..]);
        (std::iter::once(name).chain(under)).any(|n| had.contains(&(kind, Some(n.to_string()))))
    };
    (found.into_iter())
        .filter(|&(kind, item)| !was_had(kind, item))
        .collect()
}

/// `items`, the first few, for a message.
fn shown(items: &[&str]) -> String {
    const SHOWN: usize = 5;
    let mut out: Vec<String> = (items.iter().take(SHOWN))
        .map(|i| format!("{i:?}"))
        .collect();
    if items.len() > SHOWN {
        out.push(format!("and {} more", items.len() - SHOWN));
    }
    out.join(", ")
}

/// Every object of `graph` read back from `bytes`, the file written from
/// it, as written: an object that reads otherwise went out unencrypted or
/// encrypted wrong, or was copied naming an object the file hasn't kept.
/// The data of a stream made as it's written isn't in the graph, and isn't
/// compared.
#[cfg(debug_assertions)]
fn reads_back(out: &Written, graph: &Graph) -> Result<()> {
    use std::sync::Arc;
    let read = lopdf::LazyPdf::open(
        Arc::from(out.bytes),
        out.password,
        &crate::source::Limits::DEFAULT.reader,
    )?;
    for (&id, obj) in &graph.pdf.objects {
        if super::super::serialize::full::left_out(obj) {
            continue;
        }
        let Ok(back) = read.object_unkept(id) else {
            bail!("object {} {} doesn't read back", id.0, id.1);
        };
        let deferred = graph.deferred.contains_key(&id);
        if !plain_eq(obj, &back, deferred) {
            bail!("object {} {} reads back otherwise", id.0, id.1);
        }
    }
    Ok(())
}

/// Whether `a` and `b` are the same object as values: strings by their
/// bytes, whatever their form; streams by their dictionaries but
/// `/Length`, and by their data unless `deferred`.
fn plain_eq(a: &Object, b: &Object, deferred: bool) -> bool {
    fn dict_eq(a: &Dictionary, b: &Dictionary, skip: &[&[u8]]) -> bool {
        let keys = |d: &Dictionary| -> BTreeSet<Vec<u8>> {
            (d.iter().map(|(k, _)| k.clone()))
                .filter(|k| !skip.contains(&k.as_slice()))
                .collect()
        };
        keys(a) == keys(b)
            && (a.iter())
                .filter(|(k, _)| !skip.contains(&k.as_slice()))
                .all(|(k, v)| b.get(k).is_ok_and(|w| plain_eq(v, w, false)))
    }
    match (a, b) {
        (Object::String(x, _), Object::String(y, _)) => x == y,
        (Object::Array(x), Object::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(x, y)| plain_eq(x, y, false))
        }
        (Object::Dictionary(x), Object::Dictionary(y)) => dict_eq(x, y, &[]),
        (Object::Stream(x), Object::Stream(y)) => {
            let skip: &[&[u8]] = if deferred {
                &[b"Length", b"Filter", b"DecodeParms"]
            } else {
                &[b"Length"]
            };
            dict_eq(&x.dict, &y.dict, skip) && (deferred || x.content == y.content)
        }
        (Object::Real(x), Object::Real(y)) => (x - y).abs() <= 1e-9 * x.abs().max(1.0),
        (Object::Integer(x), Object::Real(y)) | (Object::Real(y), Object::Integer(x)) => {
            *x as f64 == *y
        }
        (x, y) => x == y,
    }
}

#[cfg(test)]
mod section_tests {
    use super::*;
    use crate::annot::AnnotId;
    use crate::forms::fixture::{Form, row};
    use crate::materialize::removal_tests::{note, opened, scratch};

    /// The keys of a one-page form's text field, its widget, merged with
    /// it, and a comment beside them; and the form's state.
    fn form() -> ([ObjKey; 3], DocState) {
        let dir = scratch();
        let mut f = Form::new(1);
        f.text("t", 0, row(0));
        let (_, mut doc) = opened(&dir, "a.pdf", &f.bytes());
        let _ = std::fs::remove_dir_all(&dir);
        let page = doc.page(0).unwrap();
        let add = crate::tx::fixture::add(page, &note("a comment"));
        doc.commit_ops(vec![add], "add").unwrap();
        let st = doc.state().clone();
        let (field, _, term) = st.terminals().next().unwrap();
        let widget = term.widgets[0];
        let comment = st.marks_on(page).next().unwrap().id;
        let keys = [
            ObjKey::Field(field),
            ObjKey::Annot(widget),
            ObjKey::Annot(comment),
        ];
        (keys, st)
    }

    /// Whether a section writing `key` alone at `at`, the primary object
    /// it is, passes copy-on-write over a file whose map lists `listed` at
    /// `at` too, `aliases` the section's merged widgets.
    fn passes(
        key: ObjKey,
        listed: &[ObjKey],
        aliases: &[(ObjKey, ObjKey)],
        target: &DocState,
    ) -> bool {
        let dir = scratch();
        let (_, doc) = opened(&dir, "a.pdf", &Form::new(1).bytes());
        let _ = std::fs::remove_dir_all(&dir);
        let mut record = SaveRecord::clone(doc.saved_record().unwrap());
        let at = ObjectId::from((40, 0));
        let map = std::sync::Arc::make_mut(&mut record.map);
        for &k in listed {
            map.keys.insert(k, at);
        }
        let objects = BTreeMap::from([(at, (key, Object::Null))]);
        let owned = BTreeMap::from([(key, at)]);
        let mapped = HashSet::from([at.0]);
        section(&Section {
            objects: &objects,
            record: &record,
            owned: &owned,
            start: 100,
            mapped: &mapped,
            aliases,
            target,
            used: &|_| true,
        })
        .is_ok()
    }

    /// A key the map numbers where the section writes it passes as it is,
    /// whatever else the map has there.
    #[test]
    fn a_key_written_over_its_own_number_passes() {
        let ([field, _, comment], st) = form();
        assert!(passes(field, &[field], &[], &st));
        assert!(passes(field, &[field, comment], &[], &st));
    }

    /// A field written at its own number, which the map lists by its
    /// widget, passes while the widget is merged with it, or once the
    /// widget is gone; a comment the map has there, or a widget no longer
    /// merged with it, would be overwritten, and is refused.
    #[test]
    fn only_the_field_s_merged_widget_may_stay_at_its_number() {
        let ([field, widget, comment], st) = form();
        let merged = [(widget, field)];
        assert!(passes(field, &[widget], &merged, &st));
        assert!(passes(field, &[ObjKey::Annot(AnnotId(1))], &[], &st));
        assert!(!passes(field, &[comment], &merged, &st));
        assert!(!passes(field, &[widget], &[], &st));
    }

    /// Whether a section writing one new object at 100 0, its first new
    /// number, that refers to `to`, passes over a one-page file.
    fn refers(to: ObjectId) -> bool {
        let dir = scratch();
        let (_, doc) = opened(&dir, "a.pdf", &Form::new(1).bytes());
        let _ = std::fs::remove_dir_all(&dir);
        let record = doc.saved_record().unwrap();
        assert!(record.xref.size < 100);
        let at = ObjectId::from((100, 0));
        let obj = Object::Dictionary(lopdf::dictionary! { "A" => to });
        let objects = BTreeMap::from([(at, (ObjKey::Annot(AnnotId(7)), obj))]);
        let mapped = (record.map.keys.values()).map(|id| id.0).collect();
        let used = |n: u32| record.present.contains(n);
        section(&Section {
            objects: &objects,
            record,
            owned: &BTreeMap::new(),
            start: 100,
            mapped: &mapped,
            aliases: &[],
            target: doc.state(),
            used: &used,
        })
        .is_ok()
    }

    /// A reference to a number the section gave that it doesn't write is
    /// lost, though the primary never used it, as the Rewrite's is (review
    /// finding 5, after the commit): before the first new number, that's
    /// the input's damage, and passes, as one to what the section writes,
    /// or past what a file may use, does.
    #[test]
    fn a_reference_to_a_number_the_section_gave_and_doesn_t_write_fails() {
        assert!(!refers((101, 0)));
        assert!(!refers((100, 3)));
        assert!(refers((100, 0)));
        assert!(refers((99, 0)));
        assert!(refers((crate::serialize::NUMBERS, 0)));
    }
}
