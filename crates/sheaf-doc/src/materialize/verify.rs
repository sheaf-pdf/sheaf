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
//! [`release_written`]: super::graph::release_written
//! [`Graph::followed`]: super::graph::Graph::followed
//!
//! [`Emission::owned`]: super::emit::Emission::owned

use std::collections::{BTreeMap, BTreeSet, HashSet};

use anyhow::{Result, bail};
use lopdf::{Dictionary, Document as Pdf, Object, ObjectId};

use super::graph::Graph;
use super::key::ObjKey;
use crate::forms::{Finding, Report};
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
            if primary.in_use.contains(&at.0) && !root(key) && owned.get(&key) != Some(&at) {
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

/// Whether `key` is one of the file's root objects, which the state owns
/// wherever they're numbered: the catalog, the page tree's nodes, the
/// outline's root, the form, the page labels and the document
/// information.
fn root(key: ObjKey) -> bool {
    matches!(
        key,
        ObjKey::Catalog
            | ObjKey::PagesNode(_)
            | ObjKey::OutlinesRoot
            | ObjKey::AcroForm
            | ObjKey::Labels
            | ObjKey::Info
    )
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
    let (bytes, password) = (out.bytes, out.password);
    let has_form = (graph.pdf.catalog().ok()).is_some_and(|c| c.has(b"AcroForm"));
    if has_form && out.form_changed {
        let report = crate::forms::check(bytes, password)?;
        let new = new_findings(&report, inputs);
        if !new.is_empty() {
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
            bail!("the written form has {}", said.join(", "));
        }
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
