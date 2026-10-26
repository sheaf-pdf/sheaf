//! An Append (M10.5 §9.1, §11.7): what a save's path changed, closed over
//! what depends on it ([`deps`](super::deps)), emitted onto the saved file
//! and numbered through its record, as a section after the file's last
//! byte.
//!
//! - **Numbering** ([`number::appended`]): an object the file's map has
//!   is written over itself, under its number; a primary object at its own;
//!   anything new past every number the file uses and every primary
//!   number the section names, so a reference to one that nothing is (the
//!   input's damage) stays one. A page the file has at the number of a
//!   primary page that another page is copy 0 of now leaves it to that
//!   page, and is written as a new one, with what names it by number
//!   ([`deps::Closure::renumbered`](super::deps::Closure::renumbered)).
//! - **References** to what the section doesn't write are the file's: a
//!   key its map numbers, a primary object still in it (`present`), a
//!   primary page, annotation, field or bookmark at its own number. One the
//!   file hasn't got ([`NotInFile`]) refuses the Append: the base objects a
//!   Rewrite pruned (delete pages, Rewrite, undo, save) are written only
//!   by a Rewrite.
//! - **Refused** ([`Refused`]) as well onto a file whose cross-reference
//!   sections were rebuilt, or whose page tree can't be spliced, when a
//!   primary object's number in it is another record's of that object,
//!   which stays there (unless it's a page the closure renumbered), or the
//!   record it is sits elsewhere in it, and when what it has names a page
//!   that a copy's reference names another page by now, which the section
//!   doesn't write again: the save is a Rewrite instead, or a signed
//!   document asks first. A section that writes the catalog or the form's
//!   dictionary of a file whose signatures hold, which none of them
//!   allows, is refused too unless the reader chose to append anyway: the
//!   plan judged the save within what they allow, and the save asks as it
//!   would have.
//! - **Checked** ([`verify::section`]): copy-on-write and every reference;
//!   then the removal check ([`removal`]), against the file as its record
//!   has it, which refuses an Append that would leave removed text
//!   readable, or reports it where the reader agreed it stays readable,
//!   but for a derived object whose removal the save's path didn't say
//!   (a misclassification), unless they agreed to what the check finds;
//!   the blocks a generator lays out otherwise now from the same recipe
//!   (live redraws) may go;
//!   the form, with the file, when the section writes any of it or writes
//!   a page at another number; in debug builds every object read back.
//! - **Save policy** (§11.8): what the file had of it applies to what the
//!   section writes, beside what the plan adds, but an XFA copy dropped
//!   that policy keeps now, which comes back.
//! - **The record**: the file's, its map grown by the section's keys, its
//!   tree spliced when the section wrote a node, its root objects' unowned
//!   keys as the section wrote them, the generators' blocks on the pages
//!   the section wrote as it drew them (`drawn`), the transforms the
//!   section applied, and the section's cross-reference facts.
//! - **A restored document's pending tail** (§11.10), the sections of the
//!   recovery sidecar it was restored from, which its file hasn't got, is
//!   the file's as far as the section goes: the section follows it, and
//!   both are written after the file's bytes.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::io;
use std::ops::Range;
use std::sync::{Arc, OnceLock};

use anyhow::{Context as _, Result, bail};
use in_progress::Progress;
use lopdf::{ByteSource, LazyPdf, Object, ObjectId, Stream, XrefFacts, XrefKind};

use super::emit::{self, Emission, EmitBase, EmitScope, SymObject};
use super::key::ObjKey;
use super::plan::{Note, SavePlan, SaveTransform, StrategyKind, Why};
use super::record::{
    ACROFORM_OWNED, CATALOG_OWNED, DrawnBlocks, OUTLINES_OWNED, OutputRecord, RootValues,
    SaveInput, SaveRecord, unowned,
};
use super::removal::{self, Mode};
use super::{Output, deps, number, verify};
use crate::effects::judge;
use crate::forms::rules;
use crate::model::PageId;
use crate::prim::Key;
use crate::serialize::append::{self, Base};
use crate::source::Source;

/// A key a section refers to that the saved file hasn't got: a base
/// object a Rewrite pruned, or one the closure left out.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[error("the saved file hasn't got {key:?}, which the section refers to")]
pub struct NotInFile {
    pub key: ObjKey,
}

/// Why an Append can't follow the saved file, found as its section is
/// made (M10.5 §9.2's write-time checks): the save is a Rewrite for it
/// instead ([`Refused::why`]), or a signed document asks first.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum Refused {
    /// The file's cross-reference sections were rebuilt to read it, its
    /// page tree can't be spliced, or too few object numbers are left past
    /// the primary numbers the section names.
    #[error("the saved file can't take an update: {0}")]
    Unsound(String),
    #[error(transparent)]
    Missing(#[from] NotInFile),
    /// A key that is a primary object would be written, or is in the file,
    /// somewhere other than its own number, or its number in the file still
    /// holds another record of that object (a page brought in again before
    /// the first): only a Rewrite renumbers them.
    #[error("{0:?}'s object in the saved file is another's there")]
    Held(ObjKey),
    /// What the file has of the key's object, which the section doesn't
    /// write again, names a page of its source that a copy's reference to
    /// it names another page by now, or none (a copy of the page came or
    /// went, [`deps::Closure::stale`]); or the catalog's holds a page of
    /// the primary that's no page's copy 0 now: only a Rewrite names it
    /// anew, or prunes it.
    #[error("{0:?}'s object in the saved file names a page another page is written as now")]
    Stale(ObjKey),
    /// The removal check found text the section would leave readable in
    /// `found`'s object.
    #[error("the update would leave removed text readable in {:?}", .0.key)]
    Removes(removal::Found),
    /// The section writes the catalog or the form's dictionary, which no
    /// signature allows after it, though the plan found the save within
    /// what the file's signatures allow (§9.3): the save asks, as it
    /// would have, had the plan known (it fails closed).
    #[error("the update writes the catalog or the form, which the file's signatures don't allow")]
    Unplanned,
}

impl Refused {
    /// Why the save is a Rewrite instead: None when it asks first, whatever
    /// the answer, as an Append the signatures don't allow does
    /// ([`Refused::Unplanned`]).
    pub fn why(&self) -> Option<Why> {
        match self {
            Refused::Unsound(_) => Some(Why::BaseUnsound),
            Refused::Missing(_) | Refused::Held(_) | Refused::Stale(_) => {
                Some(Why::BaseObjectsMissing)
            }
            Refused::Removes(found) => Some(Why::RemovalCheck { key: found.key }),
            Refused::Unplanned => None,
        }
    }
}

/// What the reader is told when an "Append anyway" removed text.
pub const STAYS_READABLE: &str =
    "What this save removed stays readable in the file's earlier revision";

/// What an Append's write-time checks found of its section that its plan
/// didn't judge, where they go on (§9.2): text it leaves readable that
/// the target hasn't got (the removal check's find, in a mode that doesn't
/// stop on it), and whether it writes the catalog or the form's
/// dictionary, which no signature allows after it. What a recovery
/// sidecar's flags carry to the save that writes its sections (§11.10).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Unjudged {
    pub stays_readable: bool,
    pub writes_roots: bool,
}

/// The section `plan`'s Append of `input` writes after the saved file,
/// whose bytes on disk are `on_disk`, with the record of the file it makes,
/// the removal check run in `mode`; `p` counts the pages emitted and says
/// when to stop. The output's bytes are what's written after the file's:
/// the record's pending tail, if any, then the section. An Append that
/// can't follow the file is an error that is a [`Refused`].
pub fn append(
    input: &SaveInput,
    plan: &SavePlan,
    on_disk: Arc<dyn ByteSource>,
    mode: Mode,
    p: &dyn Progress,
) -> Result<Output> {
    let saved = (input.saved.as_ref()).context("nothing says what the saved file holds")?;
    let record = &*saved.record;
    if !record.takes_updates() {
        let why = match record.knows_file() {
            true => "its cross-reference table was rebuilt to read it",
            false => "a recovery sidecar holds the whole document in its place",
        };
        return Err(Refused::Unsound(why.into()).into());
    }
    // The file as the record has it: a restored document's pending
    // sections after its bytes on disk.
    let tail = (record.pending_tail.as_ref()).and_then(|tail| tail.bytes.clone());
    let base: Arc<dyn ByteSource> = match &tail {
        Some(tail) => Arc::new(Joined {
            base: on_disk.clone(),
            section: tail.clone(),
        }),
        None => on_disk.clone(),
    };
    let primary = &input.primary;
    // What save policy did to the file applies to what the section writes
    // too (an XFA copy dropped gave its tooltips to the fields' `/TU`, and
    // Reader's rights stay gone), beside what the plan adds (§11.8); but an
    // XFA copy policy keeps now, the fields as the file had them again,
    // comes back.
    let keeps_xfa = !rules::drops_xfa(primary, &input.target);
    let mut transforms: Vec<SaveTransform> = (record.transforms.iter())
        .filter(|t| !(keeps_xfa && matches!(t, SaveTransform::DropXfa { .. })))
        .copied()
        .collect();
    for t in &plan.transforms {
        if !transforms.contains(t) {
            transforms.push(*t);
        }
    }
    let deps =
        deps::closure(input, saved, &transforms).map_err(|e| Refused::Unsound(format!("{e:#}")))?;
    if let Some(key) = deps.stale {
        return Err(Refused::Stale(key).into());
    }
    let scope = EmitScope::Keys(Arc::new(deps.keys));
    let onto = EmitBase::Saved {
        primary: &input.primary,
        record,
        parents: &deps.parents,
        tree: deps.tree.as_ref(),
        relinked: &deps.relinked,
    };
    let e = emit::emit(&input.target, scope, onto, &transforms, p)?;
    // The primary's objects the section writes as the file has them: the
    // numbers they refer to are primary numbers it names, which its new
    // keys are numbered past.
    let mut raw: HashMap<ObjKey, Object> = HashMap::new();
    let mut named = Vec::new();
    for (key, obj) in &e.objects {
        if let SymObject::Raw(r) = obj
            && r.src.id() == primary.id()
        {
            let obj = (*r.src.pdf().object_unkept(r.id)?).clone();
            refs(&obj, &mut |n| named.push(n));
            raw.insert(*key, obj);
        }
    }
    let numbers = number::appended(&e, record, named)?;
    let file = Saved::new(record, primary, &e.owned);
    held(&e, &numbers, &file, &deps.renumbered)?;
    let number = |key: &ObjKey| match numbers.by_key.get(key) {
        Some(&at) => Ok(at),
        None => file.number(key),
    };
    let mut objects: BTreeMap<ObjectId, (ObjKey, Object)> = BTreeMap::new();
    for (key, obj) in &e.objects {
        let at = numbers.by_key[key];
        let placeholder = |r: ObjectId| -> Result<ObjectId> {
            let key = (e.syms.key(r)).with_context(|| format!("{r:?} isn't a placeholder"))?;
            Ok(number(&key).map_err(Refused::from)?)
        };
        let obj = match obj {
            SymObject::Built(obj) => {
                let mut obj = obj.clone();
                follow(&mut obj, &placeholder)?;
                obj
            }
            SymObject::Deferred(d) => {
                let mut dict = Object::Dictionary(d.dict.clone());
                follow(&mut dict, &placeholder)?;
                let Object::Dictionary(dict) = dict else {
                    unreachable!("a dictionary resolves to one")
                };
                let data =
                    (d.data)().with_context(|| format!("making object {} {}", at.0, at.1))?;
                Object::Stream(Stream::new(dict, data))
            }
            // The primary's object as it is: what it names is the file's.
            SymObject::Raw(r) if r.src.id() == primary.id() => {
                let obj = (raw.remove(key)).with_context(|| format!("{key:?} is emitted twice"))?;
                let mut missing = None;
                refs(&obj, &mut |n| {
                    if missing.is_none() {
                        missing = file.own(n).err();
                    }
                });
                if let Some(missing) = missing {
                    return Err(Refused::from(missing).into());
                }
                obj
            }
            SymObject::Raw(r) => bail!(
                "object {} {} of another file can't be written as it is",
                r.id.0,
                r.id.1
            ),
        };
        if objects.insert(at, (*key, obj)).is_some() {
            bail!("two objects would be written at {} {}", at.0, at.1);
        }
    }
    p.check()?;
    let used = |n: u32| file.used(n);
    let checked = verify::Section {
        objects: &objects,
        record,
        owned: &e.owned,
        start: numbers.start,
        mapped: file.mapped(),
        aliases: &e.aliases,
        target: &input.target,
        used: &used,
    };
    verify::section(&checked)?;
    // What the plan found the file's signatures allow, unless the reader
    // chose to append anyway, never writes their roots (§9.3), while one
    // isn't broken already (B5: there'd be nothing to ask about).
    let signed =
        (objects.values()).any(|(key, _)| matches!(key, ObjKey::Catalog | ObjKey::AcroForm));
    let holding = || (primary.signatures().signed.iter()).any(|s| !s.integrity().is_broken());
    if mode == Mode::Veto && signed && record.signatures_hold && holding() {
        return Err(Refused::Unplanned.into());
    }

    let mut notes = e.notes;
    let password = record.crypt.as_ref().and(primary.password());
    let lim = primary.limits();
    // The file as the record has it: its bytes on disk, then a restored
    // document's pending sections, which are written before the section
    // too.
    let file = Joined {
        base: base.clone(),
        section: Arc::from([]),
    };
    let old = LazyPdf::open(file, password, &lim.reader).context("reading the saved file")?;
    let root = (record.map.keys.get(&ObjKey::Catalog)).context("the saved file has no catalog")?;
    let trailer: Vec<ObjectId> = [Some(*root), record.roots.info]
        .into_iter()
        .flatten()
        .collect();
    // What the reader agreed stays readable, when they chose to append
    // anyway, is what the save's path says it removes, slot by slot: what
    // the plan's offer listed, judged again.
    let said: BTreeSet<Key> = match mode {
        Mode::Report | Mode::ReportAll => {
            let path = judge(&saved.state, &input.target, &input.path_keys);
            path.removing.into_keys().collect()
        }
        Mode::Veto | Mode::Anyway | Mode::Recovery => BTreeSet::new(),
    };
    let removed = removal::check(&removal::Section {
        old: &old,
        objects: &objects,
        trailer: &trailer,
        saved,
        target: &input.target,
        primary: primary.id(),
        allowed: &e.allowed_drops,
        said: &said,
        lim,
    });
    match (removed, mode) {
        (Some(found), Mode::Veto | Mode::Anyway) => {
            removal::count(found);
            return Err(Refused::Removes(found).into());
        }
        // A misclassification the offer didn't list: the save asks again,
        // saying the check found removed text.
        (Some(found), Mode::Report) if found.misclassified() => {
            removal::count(found);
            return Err(Refused::Removes(found).into());
        }
        (Some(found), Mode::Report | Mode::ReportAll) => {
            if found.misclassified() {
                removal::count(found);
            }
            notes.push(Note(STAYS_READABLE.into()));
        }
        (Some(_), Mode::Recovery) | (None, _) => {}
    }
    let unjudged = Unjudged {
        stays_readable: removed.is_some(),
        writes_roots: signed,
    };

    let len = base.len();
    let last = match len {
        0 => None,
        len => base.read_at(len - 1..len)?.first().copied(),
    };
    let at = Base {
        len,
        ends_line: matches!(last, Some(b'\n' | b'\r')),
        header: record.xref.header_offset,
        prev: record.xref.start,
        size: numbers.start,
        kind: record.xref.kind,
        first_id: record.xref.id.as_ref().map(|[first, _]| first.clone()),
        root: *root,
        info: record.roots.info,
    };
    let update = append::section(&at, &objects, numbers.xref, record.crypt.as_deref())?;
    let bytes: &[u8] = update.as_ref().map_or(&[], |u| &u.bytes);

    // The form as the file has it has nothing new to find, unless a page
    // is written at another number than the file has it at, which its
    // widgets' `/P` name.
    let form: HashSet<&ObjKey> = e.form_keys.iter().collect();
    let moved = |at: &ObjectId, key: &ObjKey| {
        matches!(key, ObjKey::Page(_)) && record.map.keys.get(key).is_some_and(|was| was != at)
    };
    let touched = (objects.iter()).any(|(at, (key, _))| form.contains(key) || moved(at, key));
    if touched && !bytes.is_empty() {
        let mut whole = base.read_at(0..len)?.into_owned();
        whole.extend_from_slice(bytes);
        let inputs = super::emit::form::inputs(&input.target, primary);
        let inputs: Vec<&Source> = inputs.iter().map(|s| &**s).collect();
        // What the section unlinks stays in the file, for the revisions
        // before it: only what the new one reaches is its form.
        let report = crate::forms::check_latest(&whole, password)?;
        verify::form(&report, &inputs)?;
    }
    #[cfg(debug_assertions)]
    if !bytes.is_empty() {
        let joined = Joined {
            base: base.clone(),
            section: Arc::from(bytes),
        };
        verify::reads_back_section(joined, password, &objects).map_err(|e| {
            match record.crypt.is_some() {
                true => e.context("it wasn't encrypted as written"),
                false => e,
            }
        })?;
    }

    let mut map = (*record.map).clone();
    for (&at, (key, _)) in &objects {
        map.keys.insert(*key, at);
    }
    // A widget merged with its field is the field's object.
    for (alias, of) in &e.aliases {
        if let Some(&at) = numbers.by_key.get(of) {
            map.keys.insert(*alias, at);
        }
    }
    let tree = match deps
        .tree
        .as_ref()
        .filter(|t| t.nodes.iter().any(|n| n.changed))
    {
        Some(spliced) => {
            let shape = spliced.shape(&|key| number(&key).ok());
            Arc::new(shape.context("a node or page of the spliced tree has no number")?)
        }
        None => record.tree.clone(),
    };
    // What the generators drew: the file's, but on the pages the section
    // writes, as it draws them, and none on the pages gone.
    let wrote: HashSet<PageId> = (e.objects.iter())
        .filter_map(|(key, _)| match key {
            ObjKey::Page(page) => Some(*page),
            _ => None,
        })
        .collect();
    let stale = |page: &PageId| wrote.contains(page) || input.target.def(*page).is_none();
    let drawn = match e.drawn.is_empty() && !record.drawn.keys().any(|(p, _)| stale(p)) {
        true => record.drawn.clone(),
        false => {
            let mut drawn = DrawnBlocks::clone(&record.drawn);
            drawn.retain(|(page, _), _| !stale(page));
            drawn.extend(e.drawn.iter().map(|(&at, &block)| (at, block)));
            Arc::new(drawn)
        }
    };
    let xref = match &update {
        Some(update) => XrefFacts {
            kind: record.xref.kind,
            start: update.start,
            size: update.size,
            max_stream: match record.xref.kind {
                XrefKind::Stream => Some(numbers.xref),
                XrefKind::Table => record.xref.max_stream,
            },
            id: update.id.clone(),
            header_offset: record.xref.header_offset,
            hybrid: false,
            // An update voids a linearization: its `/L` is no longer the
            // file's length (Annex F; B3).
            linearized: false,
            sections: record.xref.sections + 1,
        },
        None => record.xref.clone(),
    };
    let record = OutputRecord {
        version: input.version,
        path: record.path.clone(),
        at: crate::annot::pdf_date_now(),
        strategy: StrategyKind::Append,
        sound: record.sound,
        signatures_hold: record.signatures_hold,
        pdf_version: record.pdf_version.max(e.version),
        xref,
        crypt: record.crypt.clone(),
        map: Arc::new(map),
        present: record.present.clone(),
        roots: Arc::new(roots(&record.roots, &objects)),
        tree,
        drawn,
        transforms: transforms.into(),
    };
    let mut out = tail.map_or_else(Vec::new, |tail| tail.to_vec());
    out.extend_from_slice(bytes);
    Ok(Output {
        bytes: out,
        after: Some(on_disk.len()),
        record,
        notes,
        unjudged,
        allowed_drops: e.allowed_drops,
    })
}

/// What the saved file has of what a section refers to and doesn't write.
struct Saved<'a> {
    record: &'a SaveRecord,
    primary: &'a Source,
    owned: &'a BTreeMap<ObjKey, ObjectId>,
    /// The numbers its map has a key at, found when first asked.
    mapped: OnceLock<HashSet<u32>>,
    /// The numbers the primary uses, found when first asked.
    used: OnceLock<HashSet<u32>>,
}

impl<'a> Saved<'a> {
    fn new(
        record: &'a SaveRecord,
        primary: &'a Source,
        owned: &'a BTreeMap<ObjKey, ObjectId>,
    ) -> Saved<'a> {
        Saved {
            record,
            primary,
            owned,
            mapped: OnceLock::new(),
            used: OnceLock::new(),
        }
    }

    /// Where `key`'s object is in the file: the number its map gives it;
    /// for copy 0 of a primary object, its own while the file has it; for a
    /// primary page, annotation, field or bookmark, the number of the
    /// primary object it is, while the file has that.
    fn number(&self, key: &ObjKey) -> Result<ObjectId, NotInFile> {
        if let Some(&at) = self.record.map.keys.get(key) {
            return Ok(at);
        }
        let missing = NotInFile { key: *key };
        match *key {
            ObjKey::Base { blob, obj, copy: 0 } if self.record.map.identity == Some(blob) => {
                self.own(obj)
            }
            key if key.of_state() => match self.owned.get(&key) {
                Some(&at) if self.record.present.contains(at.0) => Ok(at),
                _ => Err(missing),
            },
            _ => Err(missing),
        }
    }

    /// Primary object `n` where the file has it: its own number, while
    /// the file has it, or a key of the map is there (the root objects'
    /// values name what saves wrote by number), or when the primary never
    /// had it (its damage, which the file keeps).
    fn own(&self, n: ObjectId) -> Result<ObjectId, NotInFile> {
        if self.record.present.contains(n.0) || self.mapped().contains(&n.0) || !self.used(n.0) {
            return Ok(n);
        }
        Err(NotInFile {
            key: ObjKey::Base {
                blob: self.primary.id(),
                obj: n,
                copy: 0,
            },
        })
    }

    fn mapped(&self) -> &HashSet<u32> {
        (self.mapped).get_or_init(|| self.record.map.keys.values().map(|id| id.0).collect())
    }

    fn used(&self, n: u32) -> bool {
        let used = self
            .used
            .get_or_init(|| self.primary.pdf().ids().map(|id| id.0).collect());
        used.contains(&n)
    }
}

/// Refuses a section `e` numbered as `numbers` onto the saved `file` that
/// leaves a key that's a primary object ([`Emission::owned`]) anywhere but
/// at its number, or there over another key the file holds there still,
/// neither its own merged widget nor, when the section writes the key, a
/// page of `renumbered` it writes elsewhere (the closure wrote what names
/// that page by number again): the object would be the other's to what
/// refers to it by number. A key the section doesn't write is where the
/// file's map has it (else at its own number, as [`Saved::number`] finds
/// it): a page that's another page's copy 0 now, which the file has at a
/// number of its own, isn't at the page's object, where a Rewrite writes
/// it and what refers to the page by number goes.
fn held(
    e: &Emission,
    numbers: &number::Appended,
    file: &Saved,
    renumbered: &BTreeSet<PageId>,
) -> Result<(), Refused> {
    let mut there: Option<HashMap<u32, Vec<ObjKey>>> = None;
    for (key, &own) in &e.owned {
        let at = (numbers.by_key.get(key).copied())
            .or_else(|| file.record.map.keys.get(key).copied())
            .unwrap_or(own);
        if at != own {
            return Err(Refused::Held(*key));
        }
        if !file.mapped().contains(&own.0) {
            continue;
        }
        let there = there.get_or_insert_with(|| {
            let mut at: HashMap<u32, Vec<ObjKey>> = HashMap::new();
            for (k, n) in &file.record.map.keys {
                at.entry(n.0).or_default().push(*k);
            }
            at
        });
        let merged = |k: &ObjKey| matches!((key, k), (ObjKey::Field(_), ObjKey::Annot(_)));
        let written = numbers.by_key.contains_key(key);
        let moved = |k: &ObjKey| {
            written
                && matches!(k, ObjKey::Page(p) if renumbered.contains(p))
                && numbers.by_key.get(k).is_some_and(|&at| at != own)
        };
        let stays =
            (there.get(&own.0).into_iter().flatten()).any(|k| k != key && !merged(k) && !moved(k));
        if stays {
            return Err(Refused::Held(*key));
        }
    }
    Ok(())
}

/// `obj` with each reference the number `to` gives it.
fn follow(obj: &mut Object, to: &dyn Fn(ObjectId) -> Result<ObjectId>) -> Result<()> {
    match obj {
        Object::Reference(r) => *r = to(*r)?,
        Object::Array(items) => {
            for o in items {
                follow(o, to)?;
            }
        }
        Object::Dictionary(d) => {
            for (_, o) in d.iter_mut() {
                follow(o, to)?;
            }
        }
        Object::Stream(s) => {
            for (_, o) in s.dict.iter_mut() {
                follow(o, to)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// Each reference in `obj`, to `to`.
pub(crate) fn refs(obj: &Object, to: &mut dyn FnMut(ObjectId)) {
    match obj {
        Object::Reference(r) => to(*r),
        Object::Array(items) => items.iter().for_each(|o| refs(o, to)),
        Object::Dictionary(d) => d.iter().for_each(|(_, o)| refs(o, to)),
        Object::Stream(s) => s.dict.iter().for_each(|(_, o)| refs(o, to)),
        _ => {}
    }
}

/// The file's root objects' unowned keys, `was`, as a section that wrote
/// `objects` leaves them: a catalog, form or outline root written holds
/// them as written, and one the catalog no longer names is gone.
fn roots(was: &RootValues, objects: &BTreeMap<ObjectId, (ObjKey, Object)>) -> RootValues {
    let written = |of: ObjKey| {
        (objects.values())
            .find(|(key, _)| *key == of)
            .and_then(|(_, obj)| obj.as_dict().ok())
    };
    let mut roots = was.clone();
    if let Some(catalog) = written(ObjKey::Catalog) {
        roots.catalog = unowned(catalog, CATALOG_OWNED);
        roots.names = match catalog.get(b"Names") {
            Ok(Object::Dictionary(names)) => Some(names.clone()),
            Ok(Object::Reference(at)) => match objects.get(at) {
                Some((_, obj)) => obj.as_dict().ok().cloned(),
                None => was.names.clone(),
            },
            _ => None,
        };
        if !catalog.has(b"AcroForm") {
            roots.acroform = None;
        }
        if !catalog.has(b"Outlines") {
            roots.outlines = None;
        }
    }
    if let Some(form) = written(ObjKey::AcroForm) {
        roots.acroform = Some(unowned(form, ACROFORM_OWNED));
    }
    if let Some(outlines) = written(ObjKey::OutlinesRoot) {
        roots.outlines = Some(unowned(outlines, OUTLINES_OWNED));
    }
    roots
}

/// A file's bytes with a section after them, read as one.
struct Joined {
    base: Arc<dyn ByteSource>,
    section: Arc<[u8]>,
}

impl ByteSource for Joined {
    fn len(&self) -> usize {
        self.base.len() + self.section.len()
    }

    fn read_at(&self, range: Range<usize>) -> io::Result<Cow<'_, [u8]>> {
        let split = self.base.len();
        if range.end <= split {
            return self.base.read_at(range);
        }
        if range.start >= split {
            return Ok(Cow::Borrowed(
                &self.section[range.start - split..range.end - split],
            ));
        }
        let mut out = self.base.read_at(range.start..split)?.into_owned();
        out.extend_from_slice(&self.section[..range.end - split]);
        Ok(Cow::Owned(out))
    }

    fn first_window(&self) -> usize {
        self.base.first_window()
    }
}
