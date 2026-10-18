//! Materialization: a [`DocState`](crate::DocState) turned into bytes
//! (M10.5 §11), and what a save decides and leaves behind (§9).
//!
//! A save is an output, never an edit: [`Document::save_input`] seals the
//! version being saved and hands out a [`SaveInput`], the plan
//! ([`SavePlan`]) is decided before a byte is written, and
//! [`Document::record_save`] attaches the [`SaveRecord`] the write
//! produced to that version, which unseals it. The opened file's record is
//! synthesized at open ([`SaveRecord::opened`]), so the first save isn't a
//! special case.
//!
//! [`materialize`] writes a plan's file. A Rewrite (§11.1):
//! 1. [`emit`]s the state's objects by key;
//! 2. [`number`]s them, stably: what the primary and the previous save had
//!    keeps its number;
//! 3. builds the [`graph`]: the primary loaded eagerly, the emitted objects
//!    put in at their numbers;
//! 4. runs [`keep_only`](crate::kept::keep_only), unchanged, against the
//!    pages kept, and prunes what nothing reaches;
//! 5. checks the graph ([`verify::graph`], [`verify::copied`]):
//!    copy-on-write, every reference resolved, the pages reached the
//!    tree's and the templates; and again, but copy-on-write, after an
//!    export's post-pass;
//! 6. writes the file through [`serialize::full`](crate::serialize::full):
//!    an object neither emitted nor changed by `keep_only` is copied as its
//!    source has it, when the numbers are stable, the base sound and the
//!    encryption the same (§11.6); everything else is written, an object
//!    stream's members as objects of their own;
//! 7. checks what it wrote ([`verify::output`]): the form, and in debug
//!    builds the encryption. A check that fails stops the save.
//!
//! [`plan`] decides a save before any of it, with save policy's form
//! transforms ([`crate::forms::rules::plan`]), which the emitter applies.
//! [`write_file`] writes a planned save's file, once the reader answered
//! its asks, and makes its record; [`save`] runs a whole save on one
//! thread, from the seal to the record.
//!
//! - [`key`]: [`ObjKey`], what the emitter names an object by.
//! - [`plan`]: the plan, its asks and why a save stops.
//! - [`record`]: the save record, the History panel's note of a save, and
//!   a save's input.
//! - [`emit`]: the objects a state is written as.
//! - [`number`]: where they're written.
//! - [`copy`]: copying a closure of objects from one file into another.
//! - [`graph`]: a source loaded whole, for a Rewrite.
//! - [`verify`]: what every written file is checked for (§11.9).
//!
//! [`Document::save_input`]: crate::Document::save_input
//! [`Document::record_save`]: crate::Document::record_save

#[cfg(test)]
mod annot_form_tests;
pub mod copy;
pub mod emit;
pub mod graph;
pub mod key;
pub mod number;
pub mod plan;
pub mod record;
#[cfg(test)]
pub(crate) mod rewrite_tests;
mod saving;
#[cfg(test)]
mod saving_tests;
#[cfg(test)]
mod tests;
pub mod verify;

use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context as _, Result};
use in_progress::Progress;
use lopdf::{Object, ObjectId, Stream, XrefKind};

pub use key::ObjKey;
pub use plan::{
    AppendAnyway, Ask, AskAnswers, Encryption, Note, Numbering, Onto, Profile, SavePlan, SaveStop,
    SaveTarget, SaveTransform, Strategy, StrategyKind, Why, plan, unanswered,
};
pub use record::{OutputRecord, SaveInput, SaveNote, SaveRecord, SavedSide};
#[cfg(any(test, feature = "fixtures"))]
pub use saving::{How, appended, save_appended};
pub use saving::{SaveReport, Written, save, write_file};

use crate::serialize::crypt::Crypt;
use crate::serialize::full::{self, Body, Layout};
use emit::{EmitBase, EmitScope, PdfVersion};
use record::{BitSet, ObjMap, RootValues, TreeShape};

/// What a write made: the file's bytes, what the file is for the save that
/// keeps it, and what the reader should be told.
pub struct Output {
    pub bytes: Vec<u8>,
    pub record: OutputRecord,
    pub notes: Vec<Note>,
}

/// `plan`'s file of `input`, written as `target` says, `p` counting the
/// pages emitted and saying when to stop. Rewrites only: Appends and
/// Unchanged plans come in slice 10.
pub fn materialize(
    input: &SaveInput,
    plan: &SavePlan,
    target: SaveTarget,
    p: &dyn Progress,
) -> Result<Output, SaveStop> {
    materialize_then(input, plan, target, None, p)
}

/// What an export does to a Rewrite's graph once it's checked, before
/// it's numbered and written: `save::flatten_annots`, and the clean copy's
/// strip of every annotation the records don't hold but links and
/// widgets. Transitional: slice 19's export transforms replace it
/// (`FlattenAnnotations`, `RemoveMarkup`).
pub type PostPass<'a> = &'a mut dyn FnMut(&mut lopdf::Document) -> Result<()>;

/// [`materialize`], with `post`, when given, run on a Rewrite's graph
/// after the graph's checks: what it leaves unreachable is pruned, the
/// graph's references and pages are checked again ([`verify::links`]),
/// and no object of a graph it ran on is copied as its source has it,
/// since it may have changed any of them. The written file is checked as
/// any other.
pub fn materialize_then(
    input: &SaveInput,
    plan: &SavePlan,
    target: SaveTarget,
    post: Option<PostPass>,
    p: &dyn Progress,
) -> Result<Output, SaveStop> {
    match plan.strategy {
        Strategy::Rewrite { .. } => rewrite(input, plan, &target, post, p).map_err(stopped),
        Strategy::Append { .. } => Err(SaveStop::Failed("Append comes in slice 10".into())),
        Strategy::Unchanged => Err(SaveStop::Failed(
            "an Unchanged save writes nothing, from slice 10".into(),
        )),
    }
}

/// Why a write that failed with `e` stopped.
fn stopped(e: anyhow::Error) -> SaveStop {
    if e.chain().any(|c| c.is::<in_progress::Cancelled>()) {
        return SaveStop::Cancelled(in_progress::Cancelled);
    }
    match e.downcast_ref::<SaveStop>() {
        Some(stop) => stop.clone(),
        None => SaveStop::Failed(format!("{e:#}")),
    }
}

/// `input` as a whole new file over its primary's objects, `post` run on
/// its checked graph.
fn rewrite(
    input: &SaveInput,
    plan: &SavePlan,
    target: &SaveTarget,
    post: Option<PostPass>,
    p: &dyn Progress,
) -> Result<Output> {
    let info = (input.sources.get(&input.primary)).context("the primary isn't registered")?;
    let src = &info.src;
    let blob = src.id();
    let mut e = emit::emit(
        input,
        EmitScope::All,
        EmitBase::Rewrite,
        &plan.transforms,
        p,
    )?;
    let crypt = match target.profile.encryption {
        Encryption::Keep => src.crypt().cloned(),
        Encryption::None => None,
    };
    let saved = (input.saved.as_ref())
        .map(|s| &*s.record.map)
        .filter(|map| map.identity == Some(blob));
    let opened;
    let prev = match saved {
        Some(map) => map,
        None => {
            opened = ObjMap::opened(src.pdf(), blob, &TreeShape::of(src.pdf()));
            &opened
        }
    };
    let stable = target.profile.numbering == Numbering::Stable;
    let raw =
        stable && post.is_none() && src.base_sound() && same_crypt(crypt.as_ref(), src.crypt());
    // Where the primary's objects are in its bytes, found while the graph
    // is built and pruned, for those written as they were.
    let spans = raw.then(|| Spans::find(src));
    let loaded = graph::eager(src)?;
    let numbers = number::stable(&e, &input.target, src, &loaded, prev)?;
    let owned = std::mem::take(&mut e.owned);
    let mut version = e.version;
    if src.xref().kind == XrefKind::Stream {
        version = version.max(PdfVersion::XREF_STREAMS);
    }
    if crypt.as_deref().is_some_and(is_aes) {
        version = version.max(PdfVersion::AES);
    }
    let mut notes = std::mem::take(&mut e.notes);
    let tree: HashSet<ObjectId> = e.tree_keys().filter_map(|k| numbers.get(&k)).collect();
    let aliases: Vec<(ObjKey, ObjectId)> = (e.aliases.iter())
        .filter_map(|(alias, of)| Some((*alias, numbers.get(of)?)))
        .collect();
    let form_at: HashSet<ObjectId> = e.form_keys.iter().filter_map(|k| numbers.get(k)).collect();
    let form_touched = e.form_touched;
    let mut graph = graph::rewrite_graph(e, &numbers, src, loaded)?;
    if graph.moved > 0 {
        notes.push(Note(format!(
            "{} objects numbered past the {} a PDF may have were given new numbers",
            graph.moved,
            crate::serialize::NUMBERS
        )));
    }
    p.check()?;
    let released = graph::release_written(&mut graph.pdf, src, &tree, &owned);
    let kept = crate::kept::keep_only(&mut graph.pdf, &tree);
    notes.extend(kept.notes.into_iter().map(Note));
    graph.pdf.prune_objects();
    let in_use: HashSet<u32> = src.pdf().ids().map(|id| id.0).collect();
    let encrypt = src.crypt().map(|c| c.at());
    let reads = |id| src.pdf().object_unkept(id).is_ok();
    let checked = verify::Primary {
        in_use: &in_use,
        reads: &reads,
        start: numbers.start(),
    };
    verify::graph(&graph, &checked, &owned, &tree, stable, encrypt)?;
    let mut spans = spans.map(Spans::join).unwrap_or_default();
    // What the emitter or `keep_only` changed, what `release_written`
    // let go of, or what names a moved object, is written anew.
    let changed = |id: ObjectId| {
        kept.modified.contains(&id) || released.contains(&id) || graph.followed.contains(&id)
    };
    spans.retain(|&id, _| {
        graph.pdf.objects.contains_key(&id)
            && !graph.keys.contains_key(&id)
            && !graph.deferred.contains_key(&id)
            && !changed(id)
    });
    if stable {
        let changed = |id| changed(id) || spans.contains_key(&id);
        verify::copied(&graph, src, &in_use, &changed)?;
    }
    if let Some(post) = post {
        post(&mut graph.pdf)?;
        graph.pdf.prune_objects();
        verify::links(&graph, &checked, &tree, encrypt)?;
    }
    if !stable {
        let dense = number::dense(&graph.pdf.objects, |id| graph.deferred.contains_key(&id));
        graph.renumber(&dense, blob);
    }
    p.check()?;

    let bytes = src.bytes();
    let graph = &graph;
    let written = || (graph.pdf.objects.iter()).filter(|(_, obj)| !full::left_out(obj));
    let bodies = written().map(|(&id, obj)| -> Result<(ObjectId, Body<'_>)> {
        let key = graph.key(id, blob);
        if let Some(d) = graph.deferred.get(&id) {
            let dict = (obj.as_stream().map(|s| s.dict.clone())).unwrap_or_default();
            let data = (d.data)().with_context(|| format!("making object {} {}", id.0, id.1))?;
            let made = Object::Stream(Stream::new(dict, data));
            return Ok((id, Body::Object(Cow::Owned(made), key)));
        }
        if let Some(span) = spans.get(&id) {
            return Ok((id, Body::Raw(&bytes[span.clone()])));
        }
        Ok((id, Body::Object(Cow::Borrowed(obj), key)))
    });
    let past = written().map(|(id, _)| id.0 + 1).max().unwrap_or(1);
    let encrypt_at = match &crypt {
        Some(c) if stable && c.at().0 < crate::serialize::NUMBERS => c.at(),
        _ => (past, 0),
    };
    let layout = Layout {
        trailer: &graph.pdf.trailer,
        kind: src.xref().kind,
        version,
        crypt: crypt.as_deref(),
        encrypt_at,
        xref_from: if stable { numbers.start() } else { 1 },
    };
    let (out, xref) = full::write_bodies(bodies, &layout, p)?;
    let mut inputs = vec![&**src];
    for inst in input.target.form().names.keys() {
        if let Some(info) = input.sources.get(&inst.src)
            && inputs.iter().all(|s| s.id() != info.src.id())
        {
            inputs.push(&info.src);
        }
    }
    let checked = verify::Written {
        bytes: &out,
        // The form as the file had it, untouched by `keep_only` too, has
        // nothing new to find.
        form_changed: form_touched
            || (kept.modified.iter())
                .chain(&released)
                .any(|id| form_at.contains(id)),
        password: crypt.as_ref().and(src.password()),
        encrypted: crypt.is_some(),
    };
    verify::output(&checked, graph, &inputs)?;

    let present: BitSet = if stable {
        (written().map(|(id, _)| id.0))
            .filter(|n| in_use.contains(n))
            .collect()
    } else {
        BitSet::default()
    };
    let mut keys: BTreeMap<ObjKey, ObjectId> = (graph.keys.iter())
        .filter(|(id, _)| graph.pdf.objects.contains_key(id))
        .map(|(&id, &key)| (key, id))
        .collect();
    if stable {
        // A widget merged with its field is the field's object.
        keys.extend(
            aliases
                .into_iter()
                .filter(|(_, id)| graph.pdf.objects.contains_key(id)),
        );
    }
    let map = ObjMap {
        identity: stable.then_some(blob),
        keys,
    };
    let record = OutputRecord {
        version: input.version,
        path: target.path.clone(),
        at: crate::annot::pdf_date_now(),
        strategy: StrategyKind::Rewrite,
        xref,
        crypt: match crypt {
            Some(c) if c.at() != encrypt_at => Some(Arc::new(c.moved(encrypt_at))),
            c => c,
        },
        map: Arc::new(map),
        present: Arc::new(present),
        roots: Arc::new(RootValues::of(&graph.pdf)),
        tree: Arc::new(TreeShape::of(&graph.pdf)),
        transforms: plan.transforms.as_slice().into(),
    };
    Ok(Output {
        bytes: out,
        record,
        notes,
    })
}

/// Where an object is in its source's bytes.
type Span = std::ops::Range<usize>;

/// The search for where the primary's objects are in its bytes
/// ([`raw_spans`]), on a thread of its own while the graph is built: told
/// to stop and waited for when it's dropped unjoined, so a save that
/// stops first (cancelled, or failing) leaves nothing reading the file.
struct Spans {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<HashMap<ObjectId, Span>>>,
}

impl Spans {
    fn find(src: &crate::store::Src) -> Spans {
        let stop = Arc::new(AtomicBool::new(false));
        let (src, halt) = (src.clone(), stop.clone());
        let thread = std::thread::spawn(move || {
            let ids: Vec<ObjectId> = src.pdf().ids().collect();
            raw_spans(&src, &ids, &halt)
        });
        Spans {
            stop,
            thread: Some(thread),
        }
    }

    fn join(mut self) -> HashMap<ObjectId, Span> {
        let thread = self.thread.take().expect("joined once");
        thread.join().expect("finding spans doesn't panic")
    }
}

impl Drop for Spans {
    fn drop(&mut self) {
        if let Some(thread) = self.thread.take() {
            self.stop.store(true, Ordering::Relaxed);
            let _ = thread.join();
        }
    }
}

/// Where each of `ids`, objects of `src`, is in its bytes, those a write
/// may copy as they are ([`Source::raw_span_whole`]): found on a thread
/// per core (8 at most), each a parse of its object, until `stop`.
///
/// [`Source::raw_span_whole`]: crate::source::Source::raw_span_whole
fn raw_spans(
    src: &crate::source::Source,
    ids: &[ObjectId],
    stop: &AtomicBool,
) -> HashMap<ObjectId, Span> {
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get().min(8));
    let chunk = ids.len().div_ceil(threads).max(1024);
    std::thread::scope(|s| {
        let found: Vec<_> = (ids.chunks(chunk))
            .map(|ids| {
                s.spawn(move || {
                    (ids.iter())
                        .take_while(|_| !stop.load(Ordering::Relaxed))
                        .filter_map(|&id| Some((id, src.raw_span_whole(id)?)))
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        (found.into_iter())
            .flat_map(|f| f.join().expect("finding spans doesn't panic"))
            .collect()
    })
}

/// Whether a file encrypted with `a` is encrypted as one with `b` is,
/// keys and all: whether their objects' bytes can be copied as they are.
/// Saves keep the primary's, the one encryption they write.
fn same_crypt(a: Option<&Arc<Crypt>>, b: Option<&Arc<Crypt>>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => Arc::ptr_eq(a, b),
        _ => false,
    }
}

/// Whether `crypt` encrypts with AES, which PDF 1.6 brought.
fn is_aes(crypt: &Crypt) -> bool {
    let filters = crypt.dict().get(b"CF").and_then(Object::as_dict);
    (filters.iter().flat_map(|f| f.iter())).any(|(_, filter)| {
        (filter.as_dict().ok())
            .and_then(|f| f.get(b"CFM").ok())
            .and_then(|m| m.as_name().ok())
            .is_some_and(|m| m == b"AESV2" || m == b"AESV3")
    })
}
