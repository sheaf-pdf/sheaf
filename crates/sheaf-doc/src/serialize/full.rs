//! A whole file: the header, every object, and one cross-reference section
//! in the base's kind, with no `/Prev` or `/XRefStm` (M10.5 §11.7, M18 slice 12).
//! Encrypted, each object is encrypted under the number it's written at via
//! `pdf_structure::write::Encrypting`, an object stream's members with it,
//! and the source's `/Encrypt` and `/ID[0]` are kept.
//!
//! Under a renumbering ([`Layout::to`]) the objects are held at the numbers
//! they were given at and written at those it gives them, each through the
//! family's [`Parts`], which writes every reference as the renumbering
//! names it and encrypts under the number written; the `/Encrypt`
//! dictionary is the one laid out for the numbers written. An object given
//! as read ([`Body::Read`]) is copied there by the family's rules, its
//! header and each reference that moved written anew in place, or, where
//! they decline (at another number under a key made of it, say), written
//! anew from the object as read; a copy whose bytes or record aren't what
//! the reading read fails the write, which a dense Rewrite then makes
//! again with nothing given as read. Without a renumbering, each object is
//! written at its own number by sheaf's writer, copies as [`Body::Raw`]
//! and [`Body::Member`] say.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::thread::ScopedJoinHandle;

use anyhow::{Context as _, Result, bail};
use in_progress::Progress;

use super::crypt::Crypt;
use super::group::{self, Use};
use super::xref::{Entry, Section};
use super::{object, objstm};
use crate::materialize::ObjKey;
use crate::materialize::emit::PdfVersion;
use crate::pdf::write::{Encrypting, Packed, PartBody, Parts, ReadFrom, Record, Renumber};
use crate::pdf::{
    Dictionary, Object, ObjectId, Value, XrefFacts, XrefKind, deflate_options, resolve_fn,
};

/// After the header, a comment of bytes past ASCII, so a tool that guesses
/// from the first lines takes the file for binary.
pub(crate) const BINARY: &[u8] = b"%\xE2\xE3\xCF\xD3\n";

/// Trailer keys the base's cross-reference sections had, which this
/// section writes anew or doesn't have.
const SECTION_KEYS: [&[u8]; 15] = [
    b"Prev",
    b"XRefStm",
    b"Type",
    b"Size",
    b"W",
    b"Index",
    b"Length",
    b"Filter",
    b"DecodeParms",
    b"F",
    b"FFilter",
    b"FDecodeParms",
    b"DL",
    b"Encrypt",
    b"ID",
];

/// What [`write_bodies`] writes of an object.
pub enum Body<'a> {
    /// The object as its source holds it, `n g obj` to `endobj`: copied as
    /// it is, at the number it gives (raw passthrough, §11.6).
    Raw(&'a [u8]),
    /// An unchanged object-stream member's decoded bytes (M18 slice 9):
    /// copied as they are into its new container when packed, or written
    /// from its object value if unpacked.
    Member(&'a [u8], Cow<'a, Object>, ObjKey),
    /// An object written anew (and encrypted when the file is) on its own
    /// or, when the layout packs it, with its object stream.
    Object(Cow<'a, Object>, ObjKey),
    /// An object as the family read it, with where it was read
    /// ([`RawCopies::as_read`](crate::pdf::RawCopies::as_read)), for a write
    /// under a renumbering alone ([`Layout::to`]): the family's
    /// [`PartBody::Read`], copied from [`Layout::from`] where its rules for a
    /// copy hold, its header and each reference that moved written anew in
    /// place, else written anew from `object`. One read from an object
    /// stream goes in one where the layout packs it; one read at an offset is
    /// written on its own, never packed, as a stable save's [`Body::Raw`] is.
    Read {
        object: Cow<'a, Object>,
        record: Cow<'a, Record>,
        key: ObjKey,
    },
}

/// An object [`write_bodies`] packs: its id, its key, itself, and its
/// decoded bytes when copied from a source container.
pub(super) type Member<'a> = (ObjectId, ObjKey, Cow<'a, Object>, Option<&'a [u8]>);

/// How [`write_bodies`] writes what isn't one of its objects.
pub struct Layout<'a> {
    /// The trailer: its `/Root` and `/Info`, and any key but its sections'
    /// own ([`SECTION_KEYS`]), which are left out.
    pub trailer: &'a Dictionary,
    pub kind: XrefKind,
    /// The header's.
    pub version: PdfVersion,
    pub crypt: Option<&'a Crypt>,
    /// Where the `/Encrypt` dictionary goes, with a `crypt`: at its
    /// source's number on a stable save, else after the last object.
    pub encrypt_at: ObjectId,
    /// The least number a cross-reference stream takes, and a container
    /// [`Pack::reuse`] doesn't number.
    pub xref_from: u32,
    /// Whether objects go in object streams, and how. Only a
    /// cross-reference stream can list their members, so a table's layout
    /// that packs is refused.
    pub pack: Option<Pack<'a>>,
    /// Where each object is written, by the number it's given at, and what
    /// each reference names: none for each at its own. Under one, each
    /// body is given once, at the number it's held at, as a
    /// [`Body::Object`] or [`Body::Read`], and written through [`Parts`];
    /// `trailer` is in the numbers held too (what encryption reads its
    /// `/Root` by), its section's copy written in those `to` gives;
    /// `encrypt_at`, `xref_from`, [`Pack::uses`] and `reuse` are in the
    /// numbers written.
    /// With a `crypt`, `to` gives its dictionary's number ([`Crypt::at`])
    /// `encrypt_at`, where it's written, so that the encryption is laid out
    /// with it there, and what refers to it names it there: else the family
    /// takes it for a new file's, which writes the document's metadata
    /// stream without its own `/Identity` crypt filter.
    pub to: Option<&'a (dyn Renumber + Sync)>,
    /// The reading the bodies were read in, which [`Parts`] copies a
    /// [`Body::Read`] from and reads a stream's data where it was written
    /// from ([`LazyPdf::read_from`](crate::pdf::LazyPdf::read_from)): under
    /// `to` alone.
    pub from: Option<&'a dyn ReadFrom>,
}

/// How [`write_bodies`] packs what it writes anew that may go in an
/// object stream ([`objstm::packable`]).
pub struct Pack<'a> {
    /// Which pages use each object, by number written, which its container
    /// is grouped by ([`group::group`]).
    pub uses: &'a (dyn Fn() -> HashMap<u32, Use> + Sync),
    /// The numbers containers take first, in order: the source's own
    /// containers', now free.
    pub reuse: &'a [u32],
}

/// Writes `bodies`, by number, as a whole file laid out as `layout` says.
pub fn write_bodies<'a>(
    bodies: impl IntoIterator<Item = Result<(ObjectId, Body<'a>)>>,
    layout: &Layout<'_>,
    p: &dyn Progress,
) -> Result<(Vec<u8>, XrefFacts)> {
    if layout.pack.is_some() && layout.kind == XrefKind::Table {
        bail!("a cross-reference table can't list objects in object streams");
    }
    if layout.from.is_some() && layout.to.is_none() {
        bail!("a reading to copy from is for a write under a renumbering");
    }
    let pack = (layout.pack.as_ref()).filter(|_| layout.crypt.is_none_or(Crypt::packs));
    std::thread::scope(|s| {
        let pack = pack.map(|pack| (pack.reuse, s.spawn(pack.uses)));
        match layout.to {
            Some(to) => renumbered(bodies, to, layout, pack, p),
            None => file(bodies, layout, pack, p),
        }
    })
}

/// The thread finding which pages use each object ([`Pack::uses`]).
type Walk<'scope> = ScopedJoinHandle<'scope, HashMap<u32, Use>>;

fn file<'a>(
    bodies: impl IntoIterator<Item = Result<(ObjectId, Body<'a>)>>,
    layout: &Layout<'_>,
    pack: Option<(&[u32], Walk<'_>)>,
    p: &dyn Progress,
) -> Result<(Vec<u8>, XrefFacts)> {
    let mut out = format!("%PDF-{}\n", layout.version).into_bytes();
    out.extend_from_slice(BINARY);
    let mut section = Section::full(1);
    let mut next = 1;

    if let Some(crypt) = layout.crypt {
        let mut members: Vec<Member<'_>> = Vec::new();
        // Collect bodies so `Encryptor::laid_out` can resolve `/Root` -> `/Metadata`
        // (under `/EncryptMetadata false`) and any indirect stream `/Filter`.
        let mut collected = Vec::new();
        for item in bodies {
            p.check()?;
            collected.push(item?);
        }
        let by_id: HashMap<ObjectId, &Object> = collected
            .iter()
            .filter_map(|(id, body)| match body {
                Body::Object(obj, _) | Body::Member(_, obj, _) => Some((*id, obj.as_ref())),
                Body::Raw(_) | Body::Read { .. } => None,
            })
            .collect();
        let resolve = resolve_fn(|id| {
            by_id
                .get(&id)
                .copied()
                .or_else(|| crypt.fallback_resolve(id))
        });
        let encrypting = crypt.laid_out(None, layout.encrypt_at, layout.trailer, &resolve)?;
        for (id, body) in &collected {
            p.check()?;
            usable(id.number)?;
            next = next.max(id.number + 1);
            let offset = match body {
                Body::Raw(bytes) => {
                    let pos = out.len();
                    out.extend_from_slice(bytes);
                    out.push(b'\n');
                    pos
                }
                Body::Member(raw, obj, key)
                    if pack.is_some()
                        && !encrypting.dictionary_object(*id)
                        && objstm::packable(*id, obj) =>
                {
                    members.push((*id, *key, Cow::Borrowed(obj.as_ref()), Some(*raw)));
                    continue;
                }
                Body::Member(_, obj, _) => {
                    super::indirect_as(&mut out, *id, obj, Some(&encrypting))?
                }
                Body::Object(obj, key)
                    if pack.is_some()
                        && !encrypting.dictionary_object(*id)
                        && objstm::packable(*id, obj) =>
                {
                    members.push((*id, *key, Cow::Borrowed(obj.as_ref()), None));
                    continue;
                }
                Body::Object(obj, _) => super::indirect_as(&mut out, *id, obj, Some(&encrypting))?,
                Body::Read { .. } => return Err(as_read_unrenumbered(*id)),
            };
            section.insert(
                id.number,
                Entry::InUse {
                    offset,
                    generation: id.generation,
                },
            );
        }
        finish_file(
            out,
            section,
            members,
            layout,
            pack,
            Some(&encrypting),
            next,
            p,
        )
    } else {
        let mut members: Vec<Member<'a>> = Vec::new();
        for body in bodies {
            p.check()?;
            let (id, body) = body?;
            usable(id.number)?;
            next = next.max(id.number + 1);
            let offset = match body {
                Body::Raw(bytes) => {
                    let pos = out.len();
                    out.extend_from_slice(bytes);
                    out.push(b'\n');
                    pos
                }
                Body::Member(raw, obj, key) if pack.is_some() && objstm::packable(id, &obj) => {
                    members.push((id, key, obj, Some(raw)));
                    continue;
                }
                Body::Member(_, obj, _) => super::indirect_as(&mut out, id, &obj, None)?,
                Body::Object(obj, key) if pack.is_some() && objstm::packable(id, &obj) => {
                    members.push((id, key, obj, None));
                    continue;
                }
                Body::Object(obj, _) => super::indirect_as(&mut out, id, &obj, None)?,
                Body::Read { .. } => return Err(as_read_unrenumbered(id)),
            };
            section.insert(
                id.number,
                Entry::InUse {
                    offset,
                    generation: id.generation,
                },
            );
        }
        finish_file(out, section, members, layout, pack, None, next, p)
    }
}

#[allow(clippy::too_many_arguments)]
fn finish_file<'a>(
    mut out: Vec<u8>,
    mut section: Section,
    members: Vec<Member<'a>>,
    layout: &Layout<'_>,
    pack: Option<(&[u32], Walk<'_>)>,
    encrypting: Option<&Encrypting<'_>>,
    mut next: u32,
    p: &dyn Progress,
) -> Result<(Vec<u8>, XrefFacts)> {
    let mut trailer = layout.trailer.clone();
    for key in SECTION_KEYS {
        trailer.remove(key);
    }
    if let Some(crypt) = layout.crypt {
        let at = layout.encrypt_at;
        usable(at.number)?;
        if section.lists(at.number) || members.iter().any(|(id, ..)| id.number == at.number) {
            bail!("object {} is the encryption dictionary's", at.number);
        }
        let dict = Object::from(crypt.dict().clone());
        let offset = object::indirect(&mut out, at, &dict)?;
        section.insert(
            at.number,
            Entry::InUse {
                offset,
                generation: at.generation,
            },
        );
        trailer.set("Encrypt", Object::from(at));
        next = next.max(at.number + 1);
    }
    if let Some((reuse, uses)) = pack {
        let uses = uses.join().expect("the page walk doesn't panic");
        next = packed(
            &mut out,
            &mut section,
            &members,
            &uses,
            reuse,
            layout,
            encrypting,
            next,
            p,
        )?;
    }
    close(out, section, trailer, layout, next)
}

/// `out`, the file's objects written and listed in `section`, closed: the
/// `/ID`, then the cross-reference section in `layout`'s kind, from
/// `next` on for a stream's own number, with `trailer`'s keys.
fn close(
    mut out: Vec<u8>,
    mut section: Section,
    mut trailer: Dictionary,
    layout: &Layout<'_>,
    next: u32,
) -> Result<(Vec<u8>, XrefFacts)> {
    let kept = (layout.crypt)
        .and_then(Crypt::first_id)
        .map(<[u8]>::to_vec)
        .or_else(|| super::first_id(layout.trailer));
    let id = super::file_id(kept, layout.crypt.is_some(), &out);
    if let Some(id) = &id {
        trailer.set("ID", id.clone());
    }
    let pos = out.len();
    let deflate = deflate_options();
    let (size, max_stream) = match layout.kind {
        XrefKind::Table => {
            let size = section.size();
            section.table(&mut out, trailer, pos)?;
            (size, None)
        }
        XrefKind::Stream => {
            let at = next.max(layout.xref_from);
            usable(at)?;
            section.stream(&mut out, at, trailer, pos, deflate)?;
            (at + 1, Some(at))
        }
    };
    let id = id
        .as_ref()
        .and_then(Object::as_array)
        .and_then(|ids| match ids {
            [a, b] => Some([a.as_bytes()?.to_vec(), b.as_bytes()?.to_vec()]),
            _ => None,
        });
    let facts = XrefFacts {
        kind: layout.kind,
        start: pos,
        size,
        max_stream,
        id,
        header_offset: 0,
        hybrid: false,
        linearized: false,
        sections: 1,
    };
    Ok((out, facts))
}

/// How many object streams [`Parts::containers`] is given at once, the
/// write's progress checked between.
const BATCH: usize = 32;

/// [`file`] under the renumbering `to` ([`Layout::to`]): each body, held at
/// the number it's given at, written at the one `to` gives it through the
/// family's [`Parts`], on its own or, where the layout packs it, in an
/// object stream, but one as read at an offset on its own; encrypted as
/// [`Crypt::laid_out`] lays the file out under `to`, which `resolve`s by
/// the numbers held, an object as read by its object as read.
fn renumbered<'a>(
    bodies: impl IntoIterator<Item = Result<(ObjectId, Body<'a>)>>,
    to: &(dyn Renumber + Sync),
    layout: &Layout<'_>,
    pack: Option<(&[u32], Walk<'_>)>,
    p: &dyn Progress,
) -> Result<(Vec<u8>, XrefFacts)> {
    let mut file = Under::new(layout, to);
    let packs = pack.is_some();
    if let Some(crypt) = layout.crypt {
        let at = to.renumber(crypt.at());
        if at != Some(layout.encrypt_at) {
            bail!(
                "the renumbering puts the encryption dictionary, object {}, at {at:?}, not {}",
                crypt.at(),
                layout.encrypt_at
            );
        }
        // Collected, so that `Encryptor::laid_out` resolves `/Root` ->
        // `/Metadata` and a stream's indirect `/Filter` by the numbers held.
        let mut given = Vec::new();
        for item in bodies {
            p.check()?;
            given.push(part(item?)?);
        }
        let by_id: HashMap<ObjectId, &Object> = (given.iter())
            .map(|(id, body)| (*id, body.object()))
            .collect();
        let resolve = resolve_fn(|id| {
            by_id
                .get(&id)
                .copied()
                .or_else(|| crypt.fallback_resolve(id))
        });
        let encrypting = crypt.laid_out(Some(to), layout.encrypt_at, layout.trailer, &resolve)?;
        let parts = Parts::new(&encrypting, layout.from)?;
        let mut members = Vec::new();
        for (id, body) in &given {
            p.check()?;
            let at = file.at(*id)?;
            let packed = packs
                && !read_at_offset(body)
                && !encrypting.dictionary_object(at)
                && objstm::packable(at, body.object());
            match packed {
                true => members.push((*id, at, body)),
                false => file.on_its_own((*id, at, body), &parts)?,
            }
        }
        file.finish(&members, pack, &parts, Some(&encrypting), p)
    } else {
        let parts = Parts::plain(layout.from, Some(to))?;
        let mut held = Vec::new();
        for item in bodies {
            p.check()?;
            let (id, body) = part(item?)?;
            let at = file.at(id)?;
            match packs && !read_at_offset(&body) && objstm::packable(at, body.object()) {
                true => held.push((id, at, body)),
                false => file.on_its_own((id, at, &body), &parts)?,
            }
        }
        let members: Vec<Packed<'_>> = (held.iter())
            .map(|(id, at, body)| (*id, *at, body))
            .collect();
        file.finish(&members, pack, &parts, None, p)
    }
}

/// A body as [`Parts`] writes it: under a renumbering, an object written
/// anew or one as read is given ([`Layout::to`]); a stable save's copies
/// are bytes at their own numbers.
fn part((id, body): (ObjectId, Body<'_>)) -> Result<(ObjectId, PartBody<'_>)> {
    match body {
        Body::Object(obj, _) => Ok((id, PartBody::New(obj))),
        Body::Read { object, record, .. } => Ok((id, PartBody::Read { object, record })),
        Body::Raw(_) | Body::Member(..) => {
            bail!("object {id} is copied as it is, which a renumbered write doesn't take")
        }
    }
}

/// Whether `body` is an object as read at an offset in its file, which a
/// write under a renumbering writes on its own, as a stable save writes
/// its [`Body::Raw`]: where its rules hold, the family copies it there,
/// and never in an object stream.
fn read_at_offset(body: &PartBody<'_>) -> bool {
    matches!(body, PartBody::Read { record, .. } if record.at.stream().is_none())
}

/// The refusal of an object as read ([`Body::Read`]) given to a write at
/// the numbers the bodies are given at: its copy is the family's, under a
/// renumbering.
fn as_read_unrenumbered(id: ObjectId) -> anyhow::Error {
    anyhow::anyhow!("object {id} is given as read, which only a write under a renumbering takes")
}

/// A whole file being written under a renumbering ([`renumbered`]): what's
/// written so far, what it lists, and past the last number written.
struct Under<'l> {
    layout: &'l Layout<'l>,
    to: &'l (dyn Renumber + Sync),
    out: Vec<u8>,
    section: Section,
    next: u32,
}

impl<'l> Under<'l> {
    /// The header, for `layout` under `to`.
    fn new(layout: &'l Layout<'l>, to: &'l (dyn Renumber + Sync)) -> Under<'l> {
        let mut out = format!("%PDF-{}\n", layout.version).into_bytes();
        out.extend_from_slice(BINARY);
        Under {
            layout,
            to,
            out,
            section: Section::full(1),
            next: 1,
        }
    }

    /// Where object `id` is written: refused where the renumbering gives
    /// it none, or one a file may not use.
    fn at(&mut self, id: ObjectId) -> Result<ObjectId> {
        let Some(at) = self.to.renumber(id) else {
            bail!("object {id} has no number to be written at");
        };
        usable(at.number)?;
        self.next = self.next.max(at.number + 1);
        Ok(at)
    }

    /// Object `id`, written on its own at `at` through `parts`, and listed.
    fn on_its_own(&mut self, (id, at, body): Packed<'_>, parts: &Parts<'_>) -> Result<()> {
        if self.section.lists(at.number) {
            bail!("object {} is written twice", at.number);
        }
        let offset = (parts.body(&mut self.out, id, at, body))
            .with_context(|| format!("writing object {id} at {at}"))?;
        let generation = at.generation;
        (self.section).insert(at.number, Entry::InUse { offset, generation });
        Ok(())
    }

    /// The file finished: the `/Encrypt` dictionary as `encrypting` lays it
    /// out, `members` in object streams, and the section, its trailer in the
    /// numbers written.
    fn finish(
        mut self,
        members: &[Packed<'_>],
        pack: Option<(&[u32], Walk<'_>)>,
        parts: &Parts<'_>,
        encrypting: Option<&Encrypting<'_>>,
        p: &dyn Progress,
    ) -> Result<(Vec<u8>, XrefFacts)> {
        let layout = self.layout;
        let mut trailer = Object::from(layout.trailer.clone());
        trailer.renumber(|r| self.to.renumber(r));
        let Value::Dict(mut trailer) = trailer.into_value() else {
            unreachable!("a dictionary renumbered is one")
        };
        for key in SECTION_KEYS {
            trailer.remove(key);
        }
        if let Some(encrypting) = encrypting {
            let at = layout.encrypt_at;
            usable(at.number)?;
            let packed = members
                .iter()
                .any(|(_, written, _)| written.number == at.number);
            if self.section.lists(at.number) || packed {
                bail!("object {} is the encryption dictionary's", at.number);
            }
            let dict = Object::from(encrypting.dictionary().clone());
            let offset = object::indirect(&mut self.out, at, &dict)?;
            let generation = at.generation;
            (self.section).insert(at.number, Entry::InUse { offset, generation });
            trailer.set("Encrypt", Object::from(at));
            self.next = self.next.max(at.number + 1);
        }
        if let Some((reuse, uses)) = pack {
            let uses = uses.join().expect("the page walk doesn't panic");
            self.packed(members, &uses, reuse, parts, p)?;
        }
        close(self.out, self.section, trailer, layout, self.next)
    }

    /// `members` in object streams through `parts`, grouped by `uses` and
    /// numbered first from `reuse`, as [`packed`] packs: each written on its
    /// own where there aren't numbers enough, or its object stream would be
    /// more than a reader takes.
    fn packed(
        &mut self,
        members: &[Packed<'_>],
        uses: &HashMap<u32, Use>,
        reuse: &[u32],
        parts: &Parts<'_>,
        p: &dyn Progress,
    ) -> Result<()> {
        let by_number: HashMap<u32, Packed<'_>> =
            (members.iter()).map(|&m| (m.1.number, m)).collect();
        if by_number.len() != members.len()
            || members.iter().any(|m| self.section.lists(m.1.number))
        {
            bail!("an object is written twice");
        }
        let used = |n: &u32| uses.get(n).copied().unwrap_or(Use::None);
        let grouped = group::group(
            members
                .iter()
                .map(|(_, at, _)| (at.number, used(&at.number))),
        );
        let taken = |n| self.section.lists(n) || by_number.contains_key(&n);
        let from = self.next.max(self.layout.xref_from);
        let Some(numbers) = numbers(grouped.len(), reuse, from, taken) else {
            for &member in members {
                self.on_its_own(member, parts)?;
            }
            return Ok(());
        };
        let groups: Vec<(u32, Vec<Packed<'_>>)> = (numbers.into_iter().zip(grouped))
            .map(|(n, group)| (n, group.iter().map(|m| by_number[m]).collect()))
            .collect();
        let threads = std::thread::available_parallelism().map_or(4, |n| n.get().min(8));
        let deflate = deflate_options();
        let mut built = Vec::with_capacity(groups.len());
        for batch in groups.chunks(BATCH) {
            p.check()?;
            built.extend(parts.containers(batch, deflate, threads, None)?);
        }
        for ((n, group), container) in groups.iter().zip(built) {
            let Some(container) = container else {
                for &member in group {
                    self.on_its_own(member, parts)?;
                }
                continue;
            };
            let offset = self.out.len();
            self.out.extend_from_slice(&container);
            let generation = 0;
            (self.section).insert(*n, Entry::InUse { offset, generation });
            for (index, (_, at, _)) in (0..).zip(group) {
                let entry = Entry::Compressed { stream: *n, index };
                self.section.insert(at.number, entry);
            }
            self.next = self.next.max(n + 1);
        }
        Ok(())
    }
}

/// `pdf`, a document sheaf built in memory (blank pages, images as pages,
/// the stamp library's files), as a whole file through [`write_bodies`].
pub fn write_built(pdf: &crate::pdf::Document) -> Result<Vec<u8>> {
    let Some(version) = PdfVersion::parse(pdf.version.as_bytes()) else {
        bail!("{:?} isn't a PDF version", pdf.version);
    };
    let bodies = (pdf.objects.iter())
        .map(|(&id, obj)| Ok((id, Body::Object(Cow::Borrowed(obj), ObjKey::Built(id)))));
    let layout = Layout {
        trailer: &pdf.trailer,
        kind: XrefKind::Table,
        version,
        crypt: None,
        encrypt_at: ObjectId::new(0, 0),
        xref_from: 0,
        pack: None,
        to: None,
        from: None,
    };
    Ok(write_bodies(bodies, &layout, &in_progress::NoProgress)?.0)
}

#[allow(clippy::too_many_arguments)]
fn packed(
    out: &mut Vec<u8>,
    section: &mut Section,
    members: &[Member<'_>],
    uses: &HashMap<u32, Use>,
    reuse: &[u32],
    layout: &Layout<'_>,
    encrypting: Option<&Encrypting<'_>>,
    mut next: u32,
    p: &dyn Progress,
) -> Result<u32> {
    let by_number: HashMap<u32, &Member<'_>> = members.iter().map(|m| (m.0.number, m)).collect();
    let used = |n: &u32| uses.get(n).copied().unwrap_or(Use::None);
    let grouped = group::group(members.iter().map(|(id, ..)| (id.number, used(&id.number))));
    let taken = |n| section.lists(n) || by_number.contains_key(&n);
    let from = next.max(layout.xref_from);
    let Some(numbers) = numbers(grouped.len(), reuse, from, taken) else {
        for member in members {
            plain(out, section, member, encrypting)?;
        }
        return Ok(next);
    };
    let groups: Vec<(u32, Vec<&Member<'_>>)> = (numbers.into_iter().zip(grouped))
        .map(|(n, group)| (n, group.iter().map(|m| by_number[m]).collect()))
        .collect();
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get().min(8));
    let built = containers(&groups, threads, p)?;
    for ((n, group), container) in groups.iter().zip(built) {
        let Some(container) = container else {
            for member in group {
                plain(out, section, member, encrypting)?;
            }
            continue;
        };
        let cid = ObjectId::new(*n, 0);
        let offset = super::indirect_as(out, cid, &container, encrypting)?;
        section.insert(
            *n,
            Entry::InUse {
                offset,
                generation: 0,
            },
        );
        for (index, (id, ..)) in (0..).zip(group) {
            section.insert(id.number, Entry::Compressed { stream: *n, index });
        }
        next = next.max(n + 1);
    }
    Ok(next)
}

fn numbers(
    count: usize,
    reuse: &[u32],
    from: u32,
    taken: impl Fn(u32) -> bool,
) -> Option<Vec<u32>> {
    let mut given = Vec::with_capacity(count);
    let mut gave = HashSet::new();
    for n in reuse.iter().copied().chain(from..super::NUMBERS) {
        if given.len() == count {
            break;
        }
        if n != 0 && n < super::NUMBERS && !taken(n) && gave.insert(n) {
            given.push(n);
        }
    }
    let xref = given.iter().map(|&n| n + 1).fold(from, u32::max);
    (given.len() == count && xref < super::NUMBERS).then_some(given)
}

fn plain(
    out: &mut Vec<u8>,
    section: &mut Section,
    (id, _, obj, _): &Member<'_>,
    encrypting: Option<&Encrypting<'_>>,
) -> Result<()> {
    let offset = super::indirect_as(out, *id, obj, encrypting)?;
    section.insert(
        id.number,
        Entry::InUse {
            offset,
            generation: id.generation,
        },
    );
    Ok(())
}

/// Builds deflated `/ObjStm` container objects across up to `threads` worker
/// threads, checking `p` before each container and joining in deterministic
/// order. Encryption happens when writing each container into `out` via
/// `indirect_with` (or via `containers_encrypted` in tests).
pub(super) fn containers(
    groups: &[(u32, Vec<&Member<'_>>)],
    threads: usize,
    p: &dyn Progress,
) -> Result<Vec<Option<Object>>> {
    let build = |part: &[(u32, Vec<&Member<'_>>)]| -> Result<Vec<Option<Object>>> {
        (part.iter())
            .map(|(_, members)| {
                p.check()?;
                container_stream(members)
            })
            .collect()
    };
    let chunk = groups.len().div_ceil(threads.max(1)).max(4);
    if groups.len() <= chunk {
        return build(groups);
    }
    let build = &build;
    std::thread::scope(|s| {
        let parts: Vec<_> = (groups.chunks(chunk))
            .map(|part| s.spawn(move || build(part)))
            .collect();
        let mut built = Vec::with_capacity(groups.len());
        for part in parts {
            built.extend(part.join().expect("packing doesn't panic")?);
        }
        Ok(built)
    })
}

fn container_stream(members: &[&Member<'_>]) -> Result<Option<Object>> {
    let mut written = Vec::with_capacity(members.len());
    for (id, _, obj, raw) in members.iter().copied() {
        let bytes = match raw {
            Some(raw) => raw.to_vec(),
            None => {
                let mut bytes = Vec::new();
                crate::pdf::write::write_renumbered(
                    &mut bytes,
                    obj,
                    &crate::pdf::write::non_zero_ref,
                )
                .with_context(|| format!("writing member {id}"))?;
                bytes
            }
        };
        written.push((id.number, bytes));
    }
    Ok(objstm::container(&written).map(Object::from))
}

fn usable(n: u32) -> Result<()> {
    if n >= super::NUMBERS {
        bail!(
            "object {n} is past the {} numbers a PDF may use",
            super::NUMBERS
        );
    }
    Ok(())
}

/// Whether `obj` is one a full write leaves out: an object stream, a
/// cross-reference stream, or a linearization dictionary.
pub(crate) fn left_out(obj: &Object) -> bool {
    match obj.value() {
        Value::Stream(s) => matches!(s.dict.type_name(), Some(b"ObjStm" | b"XRef")),
        Value::Dict(d) => d.has(b"Linearized"),
        _ => false,
    }
}
