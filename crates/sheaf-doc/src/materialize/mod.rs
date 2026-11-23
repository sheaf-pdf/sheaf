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
//! 4. runs [`keep_only`](kept::keep_only), unchanged, against the
//!    pages kept, and prunes what nothing reaches;
//! 5. checks the graph ([`verify::graph`], [`verify::copied`]):
//!    copy-on-write, every reference resolved, the pages reached the
//!    tree's and the templates; and for a file without sheaf's private
//!    data, that it holds none ([`verify::private`]);
//! 6. writes the file through [`serialize::full`](crate::serialize::full):
//!    an object neither emitted nor changed by `keep_only` is copied as its
//!    source has it, when the base is sound and the encryption the same
//!    (§11.6): under stable numbers as its bytes, at its own number; under
//!    dense ones by the family's writer, its header and each reference that
//!    moved written anew in place, but one read at an offset whose number
//!    moves under an encryption that keys by number (RC4, AES-128), which
//!    is written anew. A write that refuses a copy is made again with
//!    nothing copied ([`Output::copy_refused`]). Everything else is
//!    written. A source
//!    that used object streams (its cross-reference section is a stream,
//!    or it's a hybrid, which becomes a stream file) is packed: what's
//!    written anew that may go in one does, grouped by the pages that use
//!    it ([`graph::page_uses`]), its containers numbered first where the
//!    source's own were on a stable save, else after the objects. A
//!    table's objects, and any under an encryption that leaves streams in
//!    the clear but not strings, are written on their own, an object
//!    stream's members too. Under dense numbers ([`number::dense`]) the
//!    graph is held at its own numbers through the write, which writes
//!    each object at its dense number and every reference as it names it,
//!    and is renumbered after, for the checks and the record;
//! 7. checks what it wrote ([`verify::output`]): the form, and in debug
//!    builds the encryption. A check that fails stops the save.
//!
//! An Append ([`append`]) emits only the closure of what the save's path
//! changed ([`deps`]) onto the saved file, numbers it through the file's
//! record and writes it as a section after the file's last byte, once the
//! [`removal`] check finds it leaves no removed text readable.
//!
//! [`plan`] decides a save before any of it, with save policy's form
//! transforms ([`crate::forms::rules::plan`]), which the emitter applies.
//! [`write_file`] writes a planned save's file, once the reader answered
//! its asks, and makes its record; [`save_with`] runs a whole save on one
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
//! - [`kept`]: that graph pruned to the pages it keeps.
//! - [`verify`]: what every written file is checked for (§11.9).
//! - [`deps`]: what an Append writes.
//! - [`append`]: an Append's section.
//! - [`removal`]: what an Append would leave readable in the file.
//!
//! [`Document::save_input`]: crate::Document::save_input
//! [`Document::record_save`]: crate::Document::record_save

#[cfg(test)]
mod annot_form_tests;
pub mod append;
#[cfg(test)]
mod append_rewrite_tests;
#[cfg(test)]
mod append_tests;
pub mod copy;
pub mod deps;
pub mod emit;
#[cfg(test)]
mod field_ap_tests;
#[cfg(test)]
mod flatten_tests;
pub mod graph;
#[cfg(test)]
pub(crate) mod header_tests;
pub mod kept;
pub mod key;
pub mod number;
#[cfg(test)]
pub(crate) mod ocr_tests;
#[cfg(test)]
pub(crate) mod others_tests;
#[cfg(test)]
mod overlay_tests;
pub mod plan;
#[cfg(test)]
mod plan_tests;
#[cfg(test)]
mod private_tests;
#[cfg(test)]
mod recognized_tests;
pub mod record;
pub mod removal;
#[cfg(test)]
mod removal_tests;
#[cfg(test)]
pub(crate) mod rewrite_tests;
mod saving;
#[cfg(test)]
mod saving_tests;
#[cfg(test)]
pub(crate) mod tests;
pub mod verify;
#[cfg(test)]
pub(crate) mod watermark_tests;

use std::borrow::Cow;
use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use crate::pdf::write::{Renumber, read_at_offset};
use crate::pdf::{ByteSource as _, Handover, Object, ObjectId, Stream, XrefKind};
use anyhow::{Context as _, Result};
use in_progress::Progress;

pub use key::ObjKey;
#[cfg(any(test, feature = "fixtures"))]
pub use plan::forced_append;
pub use plan::{
    AppendAnyway, Ask, AskAnswers, Encryption, Note, Numbering, Onto, Private, Profile, SavePlan,
    SaveStop, SaveTarget, SaveTransform, Strategy, StrategyKind, Unheld, Why, appending,
    forced_rewrite, plan, unanswered,
};
pub use record::{
    Came, OutputRecord, PendingTail, SaveInput, SaveNote, SaveRecord, SavedSide, TailFlags,
};
#[cfg(any(test, feature = "fixtures"))]
pub use saving::{How, appended, save_appended, save_rewritten};
pub use saving::{SaveHow, SaveReport, Written, save_with, write_file};

use crate::serialize::crypt::Crypt;
use crate::serialize::full::{self, Body, Layout};
use crate::serialize::objstm;
use emit::{EmitBase, EmitScope, PdfVersion};
use record::{BitSet, ObjMap, RootValues, TreeShape};
use removal::Mode;

/// What a write made: the file's bytes, what the file is for the save that
/// keeps it, and what the reader should be told.
pub struct Output {
    pub bytes: Vec<u8>,
    /// For an Append, the length of the saved file on disk its bytes go
    /// after (a restored document's pending tail, then the section): None
    /// for a whole file.
    pub after: Option<usize>,
    pub record: OutputRecord,
    pub notes: Vec<Note>,
    /// For an Append's section, what its checks found that its plan didn't
    /// judge: nothing for a whole file.
    pub unjudged: append::Unjudged,
    /// The saved file's blocks it unlinks as live redraws, which the
    /// removal check lets go ([`Emission::allowed_drops`]): what it judges
    /// a Rewrite with too, as a test oracle.
    ///
    /// [`Emission::allowed_drops`]: emit::Emission::allowed_drops
    pub allowed_drops: emit::AllowedDrops,
    /// For a whole file under dense numbers, why the writer refused a copy
    /// (its bytes not those it was read from, say), which wrote the file
    /// again with nothing copied: none where nothing was refused, and for
    /// an Append.
    pub copy_refused: Option<String>,
}

/// For tests: what a dense Rewrite copies from.
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TestCopies {
    /// The primary's objects as its reading read them, as a Rewrite does.
    AsRead,
    /// Those of a second reading of the primary's bytes, whose records the
    /// writer refuses as another reading's.
    ReadAgain,
    /// Nothing: every object written anew, as before M18 slice 14.
    Nothing,
}

#[cfg(test)]
thread_local! {
    /// What a dense Rewrite on this thread copies from ([`TestCopies`]).
    pub(crate) static TEST_COPIES: std::cell::Cell<TestCopies> =
        const { std::cell::Cell::new(TestCopies::AsRead) };
}

/// `plan`'s file of `input`, written as `target` says, `p` counting the
/// pages emitted and saying when to stop: a whole file, or for an Append,
/// the section after the saved file, which fails when it can't follow the
/// file ([`append::Refused`]: [`write_file`] rewrites it instead). An
/// Unchanged plan writes nothing, and has no file to make.
pub fn materialize(
    input: &SaveInput,
    plan: &SavePlan,
    target: SaveTarget,
    p: &dyn Progress,
) -> Result<Output, SaveStop> {
    match plan.strategy {
        Strategy::Rewrite { .. } => rewrite(input, plan, &target, p).map_err(stopped),
        // What it would write after holds what it holds.
        Strategy::Append { .. } if target.profile.private.strips() => Err(SaveStop::Failed(
            "a file without sheaf's private data is written whole".into(),
        )),
        Strategy::Append { .. } => append_onto_saved(input, plan, &target, Mode::Veto, p)?
            .map_err(|refused| SaveStop::Failed(refused.to_string())),
        Strategy::Unchanged => Err(SaveStop::Failed(
            "an Unchanged plan has no file to make".into(),
        )),
    }
}

/// `plan`'s Append of `input`: its section after the saved file as it is on
/// disk (§11.7), never a source's bytes, the removal check run in `mode`,
/// its record the saved file's, or (`Onto::CopyOf`) the copy's at
/// `target`'s path. A saved file gone, or not the length its record says,
/// is another program's: the save asks ([`Ask::FileChanged`]). An Append
/// that can't follow the file is the inner error.
pub(crate) fn append_onto_saved(
    input: &SaveInput,
    plan: &SavePlan,
    target: &SaveTarget,
    mode: Mode,
    p: &dyn Progress,
) -> Result<Result<Output, append::Refused>, SaveStop> {
    let saved = (input.saved.as_ref())
        .ok_or_else(|| SaveStop::Failed("nothing says what the saved file holds".into()))?;
    let copy = match plan.strategy {
        Strategy::Append { onto: Onto::CopyOf } => true,
        _ if target.same_file => false,
        _ => {
            return Err(SaveStop::Failed(
                "an Append in place writes the saved file".into(),
            ));
        }
    };
    let record = &saved.record;
    let changed = || {
        SaveStop::Ask(Ask::FileChanged {
            path: record.path.clone(),
        })
    };
    let file = match std::fs::File::open(&record.path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(changed()),
        Err(e) => return Err(stopped(anyhow::Error::new(e))),
    };
    let base = crate::pdf::FileBytes::new(file).map_err(|e| stopped(e.into()))?;
    if base.len() as u64 != record.stamp.len {
        return Err(changed());
    }
    let mut out = match append::append(input, plan, Arc::new(base), mode, p) {
        Ok(out) => out,
        Err(e) => {
            let refused = e.chain().find_map(|c| c.downcast_ref::<append::Refused>());
            return match refused {
                Some(refused) => Ok(Err(refused.clone())),
                None => Err(stopped(e)),
            };
        }
    };
    if copy {
        out.record.path = target.path.clone();
    }
    Ok(Ok(out))
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

/// `input` as a whole new file over its primary's objects.
fn rewrite(
    input: &SaveInput,
    plan: &SavePlan,
    target: &SaveTarget,
    p: &dyn Progress,
) -> Result<Output> {
    let src = &input.primary;
    let blob = src.id();
    let base = EmitBase::Rewrite {
        primary: src,
        drawn: input.saved.as_ref().map(|s| &*s.record.drawn),
        private: target.profile.private,
        unheld: target.profile.unheld,
    };
    let mut e = emit::emit(&input.target, EmitScope::All, base, &plan.transforms, p)?;
    let drawn = std::mem::take(&mut e.drawn);
    let allowed_drops = std::mem::take(&mut e.allowed_drops);
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
    let raw = src.base_sound() && same_crypt(crypt.as_ref(), src.crypt());
    #[cfg(test)]
    let raw = raw && (stable || TEST_COPIES.get() != TestCopies::Nothing);
    let want = match (raw, stable) {
        (false, _) => Handover::None,
        (true, true) => Handover::Stable,
        (true, false) => Handover::Renumbered,
    };
    let (loaded, mut raw_copies) = graph::eager_with_raw(src, want)?;
    #[cfg(test)]
    if want == Handover::Renumbered && TEST_COPIES.get() == TestCopies::ReadAgain {
        let again = crate::pdf::LazyPdf::open(
            src.bytes().clone(),
            src.password(),
            &crate::source::Limits::DEFAULT.reader,
        )?;
        raw_copies = again.to_document_and_raw(want)?.1;
    }
    let in_use: HashSet<u32> = src.pdf().ids().map(|id| id.number).collect();
    let numbers = number::stable(&e, &input.target, src, &loaded, &in_use, prev)?;
    let owned = std::mem::take(&mut e.owned);
    // A source that used object streams writes them again (a hybrid's
    // table goes), unless its encryption would leave their strings in the
    // clear.
    let used_streams = src.xref().kind == XrefKind::Stream || src.xref().hybrid;
    let pack = used_streams && crypt.as_deref().is_none_or(Crypt::packs);
    let kind = if pack {
        XrefKind::Stream
    } else {
        src.xref().kind
    };
    // The numbers the source's own containers had, which a stable save's
    // containers take first, once nothing else has them.
    let mut reuse: Vec<u32> = match pack && stable {
        true => (loaded.objects.iter())
            .filter(|(id, obj)| id.number < crate::serialize::NUMBERS && objstm::is_container(obj))
            .map(|(id, _)| id.number)
            .collect(),
        false => Vec::new(),
    };
    let mut version = e.version;
    if kind == XrefKind::Stream {
        version = version.max(PdfVersion::XREF_STREAMS);
    }
    if crypt.as_deref().is_some_and(Crypt::is_aes) {
        version = version.max(PdfVersion::AES);
    }
    let mut notes = std::mem::take(&mut e.notes);
    let tree: HashSet<ObjectId> = e.tree_keys().filter_map(|k| numbers.get(&k)).collect();
    let aliases: Vec<(ObjKey, ObjectId)> = (e.aliases.iter())
        .filter_map(|(alias, of)| Some((*alias, numbers.get(of)?)))
        .collect();
    let form_at: HashSet<ObjectId> = e.form_keys.iter().filter_map(|k| numbers.get(k)).collect();
    let form_touched = e.form_touched;
    // The annotations the pages left out, where the graph has them: one
    // numbered past what a file may use moves.
    let removed: Vec<ObjectId> = (e.removed_annots.iter())
        .filter_map(|r| match r.id.number < crate::serialize::NUMBERS {
            true => Some(r.id),
            false => numbers.get(&ObjKey::Base {
                blob,
                obj: r.id,
                copy: 0,
            }),
        })
        .collect();
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
    let kept = kept::keep_only(&mut graph.pdf, &tree, &removed);
    notes.extend(kept.notes.into_iter().map(Note));
    graph.pdf.prune_objects();
    // A container something still names keeps its number, so the
    // reference doesn't come to name a new one.
    reuse.retain(|&n| {
        (graph
            .pdf
            .objects
            .range(ObjectId::new(n, 0)..=ObjectId::new(n, u16::MAX)))
        .next()
        .is_none()
    });
    let encrypt = src.crypt().map(|c| c.at());
    let reads = |id| src.pdf().object_unkept(id).is_ok();
    let checked = verify::Primary {
        in_use: &in_use,
        reads: &reads,
        start: numbers.start(),
    };
    verify::graph(&graph, &checked, &owned, &tree, stable, encrypt)?;
    // What the emitter or `keep_only` changed, what `release_written`
    // let go of, or what names a moved object, is written anew.
    let changed = |id: ObjectId| {
        kept.modified.contains(&id) || released.contains(&id) || graph.followed.contains(&id)
    };
    let keep_raw = |id: ObjectId| {
        graph.pdf.objects.contains_key(&id)
            && !graph.keys.contains_key(&id)
            && !graph.deferred.contains_key(&id)
            && !changed(id)
    };
    raw_copies.spans.retain(|&id, _| keep_raw(id));
    raw_copies
        .members
        .retain(|&id, _| keep_raw(id) && pack && objstm::packable(id, &graph.pdf.objects[&id]));
    raw_copies.retain_as_read(keep_raw);
    if stable {
        let changed = |id| {
            changed(id)
                || raw_copies.spans.contains_key(&id)
                || raw_copies.members.contains_key(&id)
        };
        verify::copied(&graph, src, &in_use, &changed)?;
    }
    match target.profile.private {
        Private::Keep => {}
        Private::Strip => verify::private(&graph)?,
        Private::Scrub => notes.extend(verify::private_left(&graph)),
    }
    // Under dense numbers the graph is held at its own through the write,
    // which writes each object at its dense number, and renumbered after.
    let mut dense = (!stable).then(|| Dense::of(&graph));
    p.check()?;
    // The page tree, in the numbers held, which the pages are walked from.
    let held = written_tree(&graph.pdf);
    p.check()?;
    // Where each object held is written, and what a reference names.
    let name = |dense: Option<&Dense>, id: ObjectId| match dense {
        Some(d) => d.to.get(&id).copied(),
        None => Some(id),
    };
    let past = (written(&graph, dense.as_ref()).filter_map(|(&id, _)| name(dense.as_ref(), id)))
        .map(|at| at.number + 1)
        .max()
        .unwrap_or(1);
    let encrypt_at = match &crypt {
        Some(c) if stable && c.at().number < crate::serialize::NUMBERS => c.at(),
        _ => ObjectId::new(past, 0),
    };
    if let (Some(dense), Some(crypt)) = (&mut dense, &crypt) {
        dense.encrypt(crypt.at(), encrypt_at);
    }

    let bytes = src.bytes();
    let ((out, xref), copy_refused) = {
        let graph = &graph;
        let raw_copies = &raw_copies;
        let dense = dense.as_ref();
        let name = |id: ObjectId| name(dense, id);
        // Which pages use each object, by number written, which its
        // container is grouped by: the walk over the graph as written.
        let uses = || {
            let pages: Vec<ObjectId> = held.pages().into_iter().filter_map(name).collect();
            let nodes = held.nodes.iter().filter_map(|node| name(node.id));
            let stop: HashSet<ObjectId> = pages.iter().copied().chain(nodes).collect();
            graph::page_uses(&pages, &stop, |id, to| {
                let obj = match dense {
                    Some(d) => d.held(id).and_then(|id| graph.pdf.objects.get(&id)),
                    None => graph.pdf.objects.get(&id),
                };
                match (obj, dense) {
                    (None, _) => {}
                    (Some(obj), None) => append::refs(obj, to),
                    (Some(obj), Some(d)) => append::refs(obj, &mut |r| {
                        if let Some(&at) = d.to.get(&r) {
                            to(at);
                        }
                    }),
                }
            })
        };
        // Under an encryption that keys by number, an object read at an
        // offset whose number moves is given anew: its bytes wouldn't
        // decrypt there, so the family would write it anew from the object
        // as read, on its own, where given anew it's packed.
        let by_number = crypt.as_deref().is_some_and(Crypt::keys_by_number);
        // The bodies, made once a write, as read where `copy` says and a
        // candidate is, a deferred stream's data made each time.
        let bodies = |copy: bool| {
            written(graph, dense).map(move |(&id, obj)| -> Result<(ObjectId, Body<'_>)> {
                let key = graph.key(id, blob);
                if let Some(d) = graph.deferred.get(&id) {
                    let dict = (obj.as_stream().map(|s| s.dict.clone())).unwrap_or_default();
                    let data = (d.data)().with_context(|| format!("making object {id}"))?;
                    let made = Object::from(Stream::new(dict, data));
                    return Ok((id, Body::Object(Cow::Owned(made), key)));
                }
                if let Some(span) = raw_copies.spans.get(&id) {
                    return Ok((id, Body::Raw(&bytes[span.clone()])));
                }
                if let Some(member) = raw_copies.members.get(&id) {
                    return Ok((id, Body::Member(member.as_slice(), Cow::Borrowed(obj), key)));
                }
                if copy
                    && let Some((object, record)) = raw_copies.as_read(id)
                    && !(by_number && read_at_offset(record) && name(id) != Some(id))
                {
                    let (object, record) = (Cow::Borrowed(object), Cow::Borrowed(record));
                    return Ok((
                        id,
                        Body::Read {
                            object,
                            record,
                            key,
                        },
                    ));
                }
                Ok((id, Body::Object(Cow::Borrowed(obj), key)))
            })
        };
        let layout = |from| Layout {
            trailer: &graph.pdf.trailer,
            kind,
            version,
            crypt: crypt.as_deref(),
            encrypt_at,
            xref_from: if stable { numbers.start() } else { 1 },
            pack: pack.then_some(full::Pack {
                uses: &uses,
                reuse: &reuse,
            }),
            to: dense.map(|_| &name as &(dyn Renumber + Sync)),
            from,
        };
        // The reading the objects as read are copied from, when there are
        // any.
        let from = (raw_copies.as_read_ids().next()).map(|_| src.pdf().read_from());
        match full::write_bodies(bodies(true), &layout(from), p) {
            Ok(written) => (written, None),
            // A refused copy doesn't stop the export: it's written as it was
            // before copies under a renumbering, with none.
            Err(e) if from.is_some() && refused_copy(&e) => {
                let why = format!("{e:#}");
                log::warn!("a copy was refused, so the file is written with nothing copied: {why}");
                (
                    full::write_bodies(bodies(false), &layout(None), p)?,
                    Some(why),
                )
            }
            Err(e) => return Err(e),
        }
    };
    let shape = match dense {
        Some(dense) => {
            graph.renumber(&dense.to, blob);
            written_tree(&graph.pdf)
        }
        None => held,
    };
    let graph = &graph;
    let inputs = emit::form::inputs(&input.target, src);
    let inputs: Vec<&crate::Source> = inputs.iter().map(|s| &**s).collect();
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
        (written(graph, None).map(|(id, _)| id.number))
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
        sound: true,
        signatures_hold: false,
        pdf_version: version,
        xref,
        crypt: match crypt {
            Some(c) if c.at() != encrypt_at => Some(Arc::new(c.moved(encrypt_at))),
            c => c,
        },
        map: Arc::new(map),
        present: Arc::new(present),
        roots: Arc::new(RootValues {
            info: graph
                .pdf
                .trailer
                .get(b"Info")
                .and_then(Object::as_reference),
            ..RootValues::of(&graph.pdf)
        }),
        tree: Arc::new(shape),
        drawn: Arc::new(drawn),
        transforms: plan.transforms.as_slice().into(),
    };
    Ok(Output {
        bytes: out,
        after: None,
        record,
        notes,
        unjudged: append::Unjudged::default(),
        allowed_drops,
        copy_refused,
    })
}

/// Whether `e`, why a write failed, is its refusal of a copy, which a write
/// with nothing copied doesn't meet: an object as read whose record is
/// another reading's or another object's, whose bytes aren't those it was
/// read from or aren't there, or a stream's data that doesn't read where
/// it was written.
fn refused_copy(e: &anyhow::Error) -> bool {
    use crate::pdf::write::StructureError as E;
    (e.chain().filter_map(|c| c.downcast_ref::<E>())).any(|e| {
        matches!(
            e,
            E::Elsewhere(_)
                | E::Changed(_)
                | E::Gone { .. }
                | E::Uncontained { .. }
                | E::Placed(_)
                | E::Data { .. }
        )
    })
}

/// The page tree of `pdf`, a Rewrite's graph, whose nodes hold nothing
/// but their place in it.
fn written_tree(pdf: &crate::pdf::Document) -> TreeShape {
    let mut tree = TreeShape::of(pdf);
    for node in &mut tree.nodes {
        node.from = None;
    }
    tree
}

/// A dense write's numbers ([`number::dense`]): where each object of a
/// Rewrite's graph, held at its own number, is written, and the source's
/// encryption dictionary, which the graph hasn't, where the write puts it;
/// and of each number written, which object held is written there, the
/// first of the streams given it, as
/// [`Graph::renumber`](graph::Graph::renumber) keeps.
struct Dense {
    to: BTreeMap<ObjectId, ObjectId>,
    /// By number written: the object held that's written there.
    first: Vec<Option<ObjectId>>,
}

impl Dense {
    /// `graph`'s, a stream whose data is made as it's written kept apart.
    fn of(graph: &graph::Graph) -> Dense {
        let to = number::dense(&graph.pdf.objects, |id| graph.deferred.contains_key(&id));
        let mut first: Vec<Option<ObjectId>> = Vec::new();
        for (&id, at) in &to {
            let n = at.number as usize;
            if first.len() <= n {
                first.resize(n + 1, None);
            }
            first[n].get_or_insert(id);
        }
        Dense { to, first }
    }

    /// The object held that's written at `at`.
    fn held(&self, at: ObjectId) -> Option<ObjectId> {
        let first = self.first.get(at.number as usize).copied().flatten()?;
        (self.to.get(&first) == Some(&at)).then_some(first)
    }

    /// Whether object `id`, held, is written: the first given its number.
    fn written(&self, id: ObjectId) -> bool {
        self.to
            .get(&id)
            .is_some_and(|&at| self.held(at) == Some(id))
    }

    /// The source's encryption dictionary, which the graph hasn't, at `at`,
    /// where the write puts it ([`Layout::to`]): what refers to it names it
    /// there.
    fn encrypt(&mut self, source: ObjectId, at: ObjectId) {
        debug_assert!(!self.to.contains_key(&source), "{source} is the graph's");
        self.to.entry(source).or_insert(at);
    }
}

/// What a Rewrite writes of `graph`: every object but what a full write
/// leaves out, and under dense numbers, of streams given one number, the
/// first.
fn written<'g>(
    graph: &'g graph::Graph,
    dense: Option<&'g Dense>,
) -> impl Iterator<Item = (&'g ObjectId, &'g Object)> + 'g {
    (graph.pdf.objects.iter())
        .filter(move |(id, obj)| !full::left_out(obj) && dense.is_none_or(|d| d.written(**id)))
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
