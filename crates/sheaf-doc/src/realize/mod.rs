//! Rendering the model (M10.5 §10): what each consumer asks a page to draw
//! ([`ViewInputs`]), what a page draws under them, as a key
//! ([`RenderKey`]) and as the slots it's drawn from ([`render_reads`]),
//! and what PDFium draws it from ([`Realizer`]).
//!
//! **The realizer** gives each page a [`PageView`]: the engine that renders
//! it, its index there, its key, and the turn its render's job adds. A page
//! that draws as its base page does ([`untouched`]) renders from its
//! source's base engine, or from the source's view patch when the view
//! turns some of its layers off. Any other renders from its group's: the
//! pages over [`GROUP`] consecutive base pages of one source that aren't
//! untouched, in base order then copy, realized as one overlay, an append
//! section over the source's bytes whose root lists only them (slice 2's
//! scoped groups), so the engine's page i is the group's i-th. A view is
//! cheap, for the UI thread: a key and the predicate, the group's pages
//! found once a state and inputs. Realizing an engine ([`Realizer::open`])
//! is the pool's worker's: each page's delta emitted alone and kept by
//! page and key, the group's frame (its root, catalog and form) emitted
//! over all of them, and the two numbered and written by `assemble`, from
//! the source's allocator (`alloc`). A damaged base's group is a file of
//! its own. So is the group of a base PDFium refuses
//! ([`Source::base_refused`], which the engine pool finds out), and every
//! page of such a source renders from its group, untouched or not: there's
//! no base engine to draw it (Greg, 2026-10-20). Nothing reaches disk.
//!
//! **Bounds.** The deltas kept are at most [`DELTAS_BYTES`] by estimate,
//! the oldest let go first; a delta let go is emitted again when its group
//! is next opened. Each source's allocator grows by every key its sections
//! wrote, for the session (`alloc`). The groups found are a state's: a new
//! state finds them again. The digests of the last [`INPUTS_KEPT`] inputs
//! that hide or drag something are kept, with their sets, and the last few
//! images and pages watermarks drew, so each is made once, not once a page
//! (`Arts`).

mod alloc;
mod assemble;
#[cfg(test)]
pub(crate) mod fixture;
mod inputs;
mod key;
#[cfg(test)]
mod key_tests;
mod untouched;
#[cfg(test)]
mod untouched_tests;

use std::collections::{BTreeSet, HashMap, VecDeque};
use std::sync::{Arc, Mutex, PoisonError, Weak};

use anyhow::{Context as _, Result, bail};
use in_progress::NoProgress;
use sheaf_edit::Rotation;
use sheaf_util::Checksum;

pub use inputs::{MarkDraw, OcgOverride, ViewInputs};
pub use key::{RenderKey, render_key, render_reads};
pub(crate) use key::{
    form_digest, form_ref_digest, layout_reads, page_digest, pinned_digest, recipe_digest,
};
#[cfg(any(test, feature = "fixtures"))]
pub use untouched::emission_is_base;
pub use untouched::untouched;

use alloc::Numbers;
use assemble::Part;

use crate::layers::LayerId;
use crate::materialize::emit::content::Arts;
use crate::materialize::emit::{self, EmitBase, EmitScope, Overlay, RotatePolicy};
use crate::model::{DocState, PageId};
use crate::source::Source;
use crate::store::{BlobId, Src};

/// The consecutive base pages of one source whose edited pages share an
/// overlay: slice 2's, the largest whose scoped engine opens within 0.5 ms
/// of the bare base's and fits the 16 MB per-engine cap with every page of
/// its group previewed.
pub const GROUP: u32 = 128;

/// The deltas a realizer keeps, by its estimate of their bytes: a group's
/// pages with a header and a crop each are a few kilobytes apiece.
pub const DELTAS_BYTES: usize = 32 << 20;

/// The inputs whose digests a realizer keeps: the screen's, print
/// preview's, OCR's and a plugin's, and room for those made again.
const INPUTS_KEPT: usize = 8;

/// A group: the `chunk`th [`GROUP`] base pages of `source`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct GroupKey {
    pub source: BlobId,
    pub chunk: u32,
}

/// An engine a page renders from (§10.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EngineKey {
    /// A source's own bytes.
    Base(BlobId),
    /// A group's overlay, by a digest of its pages, what each draws and
    /// what the inputs draw it with, so a group whose pages changed is
    /// another engine, and one whose pages didn't is the same whatever's
    /// hidden on other pages. Over a damaged base or one PDFium refuses, a
    /// file of its own.
    Group(GroupKey, u128),
    /// A source with its catalog patched for a view, by a digest of the
    /// layers the view turns off: what its untouched pages render from
    /// under that view.
    SourcePatch(BlobId, u128),
}

/// What a render's job adds to its engine's page: the turn from the page
/// its engine has, its base page's `/Rotate`, to the page's own (§5). No
/// box: spike C failed, so a crop is realized.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct JobXform {
    pub rotation: Rotation,
}

/// Where a page renders from under some inputs, and how.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PageView {
    pub engine: EngineKey,
    /// The page's index in its engine: its base page's, or its place in
    /// its group.
    pub index: u32,
    pub key: RenderKey,
    pub job: JobXform,
    /// The render's flags: annotations but widgets (PDFium's
    /// `FPDF_ANNOT`), and the form's widgets (`FPDF_FFLDraw`), as the inputs
    /// draw them.
    pub annots: bool,
    pub forms: bool,
}

/// What an engine opens (§10.3): its bytes, segments read one after
/// another (a source's, and a section after them; or a file of its own),
/// and the password they're read with.
pub struct Realization {
    pub segments: Vec<Arc<[u8]>>,
    pub password: Option<String>,
}

impl Realization {
    /// `src`'s bytes, and `section` after them when there's one.
    fn over(src: &Src, section: Option<Vec<u8>>) -> Realization {
        let segments = std::iter::once(src.bytes().clone())
            .chain(section.map(Arc::from))
            .collect();
        Realization {
            segments,
            password: src.password().map(String::from),
        }
    }
}

/// One document's realizer (§10.1). Send and Sync: views are asked for on
/// the UI thread, and engines realized on the pool's worker, each cache
/// behind a lock of its own.
#[derive(Default)]
pub struct Realizer {
    /// Each source a view was asked of, unpinned.
    sources: Mutex<HashMap<BlobId, Weak<Source>>>,
    /// Each source's allocator.
    numbers: Mutex<HashMap<BlobId, Numbers>>,
    groups: Mutex<Groups>,
    deltas: Mutex<Deltas>,
    /// The digests of inputs lately asked of, with the inputs, newest
    /// first ([`Realizer::inputs_id`]).
    inputs: Mutex<VecDeque<(ViewInputs, u128)>>,
    /// Watermarks' images and pages, made once for every page's delta.
    arts: Arts,
}

/// The groups found in one state, by group, inputs and whether PDFium
/// had refused the group's source when they were found.
#[derive(Default)]
struct Groups {
    state: Option<DocState>,
    found: HashMap<(GroupKey, u128, bool), Arc<Members>>,
}

/// A group's pages, in its engine's order, and what each draws.
struct Members {
    pages: Arc<[PageId]>,
    keys: Vec<RenderKey>,
    /// Of the pages and their keys, and what the inputs draw them with:
    /// [`EngineKey::Group`]'s.
    digest: u128,
}

/// The pages' deltas, by page and key, the oldest let go first past
/// `cap` bytes ([`DELTAS_BYTES`]).
struct Deltas {
    by: HashMap<(PageId, RenderKey), (Arc<Part>, usize)>,
    order: VecDeque<(PageId, RenderKey)>,
    bytes: usize,
    cap: usize,
}

impl Default for Deltas {
    fn default() -> Deltas {
        Deltas {
            by: HashMap::new(),
            order: VecDeque::new(),
            bytes: 0,
            cap: DELTAS_BYTES,
        }
    }
}

impl Realizer {
    pub fn new() -> Realizer {
        Realizer::default()
    }

    /// One keeping deltas of `cap` bytes at most.
    #[cfg(test)]
    fn capped(cap: usize) -> Realizer {
        let deltas = Deltas {
            cap,
            ..Deltas::default()
        };
        Realizer {
            deltas: Mutex::new(deltas),
            ..Realizer::default()
        }
    }

    /// Where page `page` of `st` renders from under `inputs`. An error for
    /// a page `st` hasn't got.
    pub fn view(&self, st: &DocState, page: PageId, inputs: &ViewInputs) -> Result<PageView> {
        let def =
            (st.def(page)).with_context(|| format!("{page:?} isn't a page of the document"))?;
        let key = render_key(st, page, inputs).context("a page has a key")?;
        let src = &def.base.src;
        self.know(src);
        let view = |engine, index| PageView {
            engine,
            index,
            key,
            job: JobXform {
                rotation: st.render_delta(page).unwrap_or(Rotation::R0),
            },
            annots: inputs.draws_annots(),
            forms: inputs.widgets,
        };
        if on_base(st, page, inputs) {
            let engine = match layers_off(src, inputs) {
                Some(off) => EngineKey::SourcePatch(src.id(), layers_digest(off)),
                None => EngineKey::Base(src.id()),
            };
            return Ok(view(engine, def.base.index));
        }
        let group = GroupKey {
            source: src.id(),
            chunk: def.base.index / GROUP,
        };
        let members = self.members(st, src, group, inputs);
        let index = (members.pages.iter().position(|&p| p == page))
            .with_context(|| format!("{page:?} isn't one of its group's pages"))?;
        Ok(view(EngineKey::Group(group, members.digest), index as u32))
    }

    /// What `engine`, a view's of `st` under `inputs`, opens: a source's
    /// bytes; with its view patch after them; or a group's overlay, its
    /// pages' deltas kept, its frame emitted, put together. An error when
    /// the group's pages have changed since the view (a view of `st` asks
    /// again), or the source is gone.
    pub fn open(
        &self,
        st: &DocState,
        engine: EngineKey,
        inputs: &ViewInputs,
    ) -> Result<Realization> {
        match engine {
            EngineKey::Base(blob) => Ok(Realization::over(&self.source(st, blob)?, None)),
            EngineKey::SourcePatch(blob, _) => {
                let src = self.source(st, blob)?;
                let off = layers_off(&src, inputs).context("the view shows all its layers")?;
                if standalone(&src) {
                    bail!("a damaged or refused base takes no patch");
                }
                let section = self.with_numbers(&src, |numbers| {
                    assemble::patch(&src, off, numbers)?.context("it has no layers")
                })?;
                Ok(Realization::over(&src, Some(section)))
            }
            EngineKey::Group(group, digest) => {
                let src = self.source(st, group.source)?;
                let members = self.members(st, &src, group, inputs);
                if members.digest != digest {
                    bail!("the group's pages changed since its view");
                }
                let mut parts = Vec::with_capacity(members.pages.len() + 1);
                for (&page, &key) in members.pages.iter().zip(&members.keys) {
                    parts.push(self.delta(st, &src, page, key, inputs)?);
                }
                let overlay = Overlay {
                    src: src.clone(),
                    inputs,
                    rotate: RotatePolicy::KeepBase,
                    arts: Some(&self.arts),
                };
                let scope = EmitScope::Frame(members.pages.clone());
                let e = emit::emit(st, scope, EmitBase::Overlay(&overlay), &[], &NoProgress)?;
                let frame = Part::frame(e);
                let all: Vec<&Part> = (parts.iter().map(|p| &**p)).chain([&frame]).collect();
                match standalone(&src) {
                    false => {
                        let section =
                            self.with_numbers(&src, |n| assemble::section(&src, &all, n))?;
                        Ok(Realization::over(&src, Some(section)))
                    }
                    true => {
                        let pdf =
                            self.with_numbers(&src, |n| assemble::standalone(&src, &all, n))?;
                        Ok(Realization {
                            segments: vec![pdf.into()],
                            password: None,
                        })
                    }
                }
            }
        }
    }

    /// Notes `src`, which a view was asked of.
    fn know(&self, src: &Src) {
        let mut sources = self.sources.lock().unwrap_or_else(PoisonError::into_inner);
        let known = sources.entry(src.id()).or_default();
        if known.strong_count() == 0 {
            *known = src.downgrade();
        }
    }

    /// Source `blob`: one a view was asked of, else one a page of `st`
    /// shows.
    fn source(&self, st: &DocState, blob: BlobId) -> Result<Src> {
        let known = (self.sources.lock().unwrap_or_else(PoisonError::into_inner))
            .get(&blob)
            .and_then(Src::upgrade);
        let src = known.or_else(|| {
            (st.pages().map(|(_, def, _)| &def.base.src))
                .find(|src| src.id() == blob)
                .cloned()
        });
        let src = src.context("the document has no pages of that source")?;
        self.know(&src);
        Ok(src)
    }

    /// `f` of `src`'s allocator, made the first time.
    fn with_numbers<T>(&self, src: &Src, f: impl FnOnce(&mut Numbers) -> Result<T>) -> Result<T> {
        let mut all = self.numbers.lock().unwrap_or_else(PoisonError::into_inner);
        let numbers = match all.entry(src.id()) {
            std::collections::hash_map::Entry::Occupied(e) => e.into_mut(),
            std::collections::hash_map::Entry::Vacant(e) => e.insert(Numbers::over(src)?),
        };
        f(numbers)
    }

    /// Group `group`'s pages in `st` under `inputs`, of `src`: found once
    /// a state and inputs, and again once PDFium has refused `src`.
    fn members(
        &self,
        st: &DocState,
        src: &Src,
        group: GroupKey,
        inputs: &ViewInputs,
    ) -> Arc<Members> {
        let at = (group, self.inputs_id(inputs), src.base_refused());
        {
            let groups = self.groups.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some(found) = groups.found.get(&at)
                && groups.state.as_ref().is_some_and(|was| was.draws_alike(st))
            {
                return found.clone();
            }
        }
        let members = Arc::new(find(st, src, group, inputs));
        let mut groups = self.groups.lock().unwrap_or_else(PoisonError::into_inner);
        if !groups.state.as_ref().is_some_and(|was| was.draws_alike(st)) {
            groups.state = Some(st.clone());
            groups.found.clear();
        }
        groups.found.insert(at, members.clone());
        members
    }

    /// `inputs`' digest ([`inputs_digest`]), kept for the last inputs asked
    /// of that hide or drag something, matched by their sets' pointers
    /// (held here, so a pointer names one set while it's kept): a view is
    /// asked for each page near the screen every frame, and Hide Comments
    /// hides every other tool's comment of the document.
    fn inputs_id(&self, inputs: &ViewInputs) -> u128 {
        if inputs.hidden.is_empty() && inputs.dragging.is_empty() {
            return inputs_digest(inputs);
        }
        let mut kept = self.inputs.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some((_, id)) = kept.iter().find(|(was, _)| same_sets(was, inputs)) {
            return *id;
        }
        let id = inputs_digest(inputs);
        kept.push_front((inputs.clone(), id));
        kept.truncate(INPUTS_KEPT);
        id
    }

    /// Page `page`'s delta, which draws as `key` says: kept, or emitted
    /// alone over `src` without the frame ([`EmitScope::Delta`]).
    fn delta(
        &self,
        st: &DocState,
        src: &Src,
        page: PageId,
        key: RenderKey,
        inputs: &ViewInputs,
    ) -> Result<Arc<Part>> {
        let kept = (self.deltas.lock().unwrap_or_else(PoisonError::into_inner))
            .by
            .get(&(page, key))
            .map(|(part, _)| part.clone());
        if let Some(part) = kept {
            return Ok(part);
        }
        let overlay = Overlay {
            src: src.clone(),
            inputs,
            rotate: RotatePolicy::KeepBase,
            arts: Some(&self.arts),
        };
        let scope = EmitScope::Delta(page);
        let e = emit::emit(st, scope, EmitBase::Overlay(&overlay), &[], &NoProgress)?;
        let part = Arc::new(Part::page(e, page)?);
        let weight = part.weight();
        let mut deltas = self.deltas.lock().unwrap_or_else(PoisonError::into_inner);
        if deltas
            .by
            .insert((page, key), (part.clone(), weight))
            .is_none()
        {
            deltas.order.push_back((page, key));
            deltas.bytes += weight;
        }
        while deltas.bytes > deltas.cap
            && let Some(oldest) = deltas.order.pop_front()
        {
            if let Some((_, weight)) = deltas.by.remove(&oldest) {
                deltas.bytes -= weight;
            }
        }
        Ok(part)
    }
}

/// Whether page `page` of `st` renders under `inputs` from its source's
/// base engine, or the source's view patch: it's [`untouched`], PDFium
/// reads its source, and when the view turns layers of its source off,
/// the source takes a patch (a damaged base doesn't: its groups hold every
/// page then).
fn on_base(st: &DocState, page: PageId, inputs: &ViewInputs) -> bool {
    let drawn = |src: &Src| {
        !src.base_refused() && !(layers_off(src, inputs).is_some() && !src.base_sound())
    };
    untouched(st, page, inputs) && st.def(page).is_some_and(|def| drawn(&def.base.src))
}

/// Whether `src`'s groups are files of their own, rather than sections
/// over its bytes: its cross-references were rebuilt (spike B), or PDFium
/// refuses them.
fn standalone(src: &Source) -> bool {
    !src.base_sound() || src.base_refused()
}

/// The layers of `src` `inputs` turn off, when it has layers.
fn layers_off<'a>(src: &Source, inputs: &'a ViewInputs) -> Option<&'a BTreeSet<LayerId>> {
    let off = inputs.ocg_off.as_ref()?.off.get(&src.id())?;
    let layered = src.pdf().catalog().ok()?.has(b"OCProperties");
    layered.then_some(off)
}

/// A digest of the layers `off`: [`EngineKey::SourcePatch`]'s.
fn layers_digest(off: &BTreeSet<LayerId>) -> u128 {
    let mut sum = Checksum::new();
    for (n, g) in off {
        sum.write(&n.to_le_bytes());
        sum.write(&g.to_le_bytes());
    }
    sum.finish()
}

/// A digest of `inputs`: one for equal inputs, whatever order their sets
/// hold their ids in. O(n log n) in the ids hidden and dragged.
fn inputs_digest(inputs: &ViewInputs) -> u128 {
    #[cfg(test)]
    DIGESTS.set(DIGESTS.get() + 1);
    let ViewInputs {
        marks: _,
        generators: _,
        ocr_over: _,
        widgets: _,
        foreign: _,
        hidden,
        dragging,
        ocg_off,
    } = inputs;
    let mut sum = Checksum::new();
    write_flags(&mut sum, inputs);
    for ids in [hidden, dragging] {
        let mut ids: Vec<u64> = ids.iter().map(|id| id.0).collect();
        ids.sort_unstable();
        sum.write(&(ids.len() as u64).to_le_bytes());
        for id in ids {
            sum.write(&id.to_le_bytes());
        }
    }
    for (blob, off) in ocg_off.iter().flat_map(|ocg| &ocg.off) {
        sum.write(&blob.hash.to_le_bytes());
        sum.write(&blob.len.to_le_bytes());
        sum.write(&layers_digest(off).to_le_bytes());
    }
    sum.finish()
}

#[cfg(test)]
thread_local! {
    /// How many inputs digests this thread has worked out.
    static DIGESTS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// `inputs`' flags, put in `sum`.
fn write_flags(sum: &mut Checksum, inputs: &ViewInputs) {
    let marks = match inputs.marks {
        MarkDraw::ByHost => 0u8,
        MarkDraw::ByEngine => 1,
        MarkDraw::Off => 2,
    };
    let flags = [
        inputs.generators,
        inputs.ocr_over,
        inputs.widgets,
        inputs.foreign,
    ];
    sum.write(&[marks]);
    sum.write(&flags.map(u8::from));
}

/// Whether `a` and `b` are the same inputs: the same flags, and the very
/// same sets.
fn same_sets(a: &ViewInputs, b: &ViewInputs) -> bool {
    let ViewInputs {
        marks,
        generators,
        ocr_over,
        widgets,
        foreign,
        hidden,
        dragging,
        ocg_off,
    } = a;
    let flags = (*marks, *generators, *ocr_over, *widgets, *foreign);
    let ocg = match (ocg_off, &b.ocg_off) {
        (None, None) => true,
        (Some(a), Some(b)) => Arc::ptr_eq(a, b),
        _ => false,
    };
    flags == (b.marks, b.generators, b.ocr_over, b.widgets, b.foreign)
        && Arc::ptr_eq(hidden, &b.hidden)
        && Arc::ptr_eq(dragging, &b.dragging)
        && ocg
}

/// Group `group`'s pages in `st` under `inputs`, of `src`: those showing
/// its base pages that don't render from the base ([`on_base`]), in base
/// order then copy. Its digest is of them, their keys, the inputs' flags
/// and the layers they turn off of `src`, which with the keys are all the
/// inputs draw the group with (what's hidden or dragged on its pages is in
/// their keys, and what's on other pages doesn't change it), and whether
/// it's a file of its own, which a refusal learned later changes.
fn find(st: &DocState, src: &Src, group: GroupKey, inputs: &ViewInputs) -> Members {
    let start = group.chunk.saturating_mul(GROUP);
    let end = start.saturating_add(GROUP).min(src.page_count());
    let mut pages = Vec::new();
    let mut keys = Vec::new();
    let mut sum = Checksum::new();
    write_flags(&mut sum, inputs);
    let off = layers_off(src, inputs).map(layers_digest);
    sum.write(&[u8::from(standalone(src))]);
    sum.write(&[u8::from(off.is_some())]);
    sum.write(&off.unwrap_or_default().to_le_bytes());
    for index in start..end {
        let Some(object) = src.page_obj(index) else {
            continue;
        };
        for page in st.pages_showing(object) {
            let at_index = st.def(page).is_some_and(|def| def.base.index == index);
            if !at_index || on_base(st, page, inputs) {
                continue;
            }
            let Some(key) = render_key(st, page, inputs) else {
                continue;
            };
            sum.write(&page.0.to_le_bytes());
            sum.write(&key.0.to_le_bytes());
            pages.push(page);
            keys.push(key);
        }
    }
    Members {
        pages: pages.into(),
        keys,
        digest: sum.finish(),
    }
}
