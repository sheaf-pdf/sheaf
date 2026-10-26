//! A whole file: the header, every object, and one cross-reference section
//! in the base's kind, with no `/Prev` or `/XRefStm`. Encrypted, each
//! object is encrypted under the number it's written at, an object
//! stream's members with it, and the source's `/Encrypt` and `/ID[0]` are
//! kept.
//!
//! [`write_bodies`] is the materializer's (M10.5 §11.7): an object is
//! either its source's bytes, copied at its own number (raw passthrough),
//! or written, its AES IVs keyed by its [`ObjKey`]; the header gives the
//! version the emission needs. Asked to ([`Pack`]), it packs what it
//! writes that may go in an object stream ([`objstm::packable`]) into
//! containers grouped by the pages that use them ([`group`]), which a
//! thread of their own finds while the rest is written; the containers
//! are built on a thread per core (8 at most), each encrypted once, as a
//! stream, under its own number. [`write_built`] is a PDF sheaf builds in
//! memory, written through it, unpacked.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::thread::ScopedJoinHandle;

use anyhow::{Context as _, Result, bail};
use in_progress::Progress;
use lopdf::{Dictionary, Object, ObjectId, XrefFacts, XrefKind};

use super::crypt::Crypt;
use super::group::{self, Use};
use super::xref::{Entry, Section};
use super::{object, objstm};
use crate::materialize::ObjKey;
use crate::materialize::emit::PdfVersion;

/// After the header, a comment of bytes past ASCII, so a tool that guesses
/// from the first lines takes the file for binary.
pub(crate) const BINARY: &[u8] = b"%\xE2\xE3\xCF\xD3\n";

/// Trailer keys the base's cross-reference sections had, which this
/// section writes anew or doesn't have: its links to other sections, a
/// cross-reference stream's dictionary (lopdf reads it as the trailer),
/// and the encryption and `/ID`.
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
    /// An object, which is the key: written, and encrypted when the file
    /// is, on its own, its IVs keyed by the key, or, when the layout packs
    /// it, with its object stream.
    Object(Cow<'a, Object>, ObjKey),
}

/// An object [`write_bodies`] packs: its id, what it is, and itself.
pub(super) type Member<'a> = (ObjectId, ObjKey, Cow<'a, Object>);

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
    /// [`Pack::reuse`] doesn't number: on a stable save, past every number
    /// the primary and the previous save used, so neither takes a number
    /// an object pruned from the primary had.
    pub xref_from: u32,
    /// Whether objects go in object streams, and how. Only a
    /// cross-reference stream can list their members, so a table's layout
    /// that packs is refused.
    pub pack: Option<Pack<'a>>,
}

/// How [`write_bodies`] packs what it writes anew that may go in an
/// object stream ([`objstm::packable`]).
pub struct Pack<'a> {
    /// Which pages use each object, by number, which its container is
    /// grouped by ([`group::group`]): one not listed is no page's. Called
    /// once, on a thread of its own while the rest is written, since a
    /// walk of every page's objects takes about as long as writing them.
    pub uses: &'a (dyn Fn() -> HashMap<u32, Use> + Sync),
    /// The numbers containers take first, in order: the source's own
    /// containers', now free. One the file lists, one given already, 0
    /// and one a file may not use are passed over.
    pub reuse: &'a [u32],
}

/// `bodies`, by number, as a whole file laid out as `layout` says, `p`
/// saying when to stop: the bytes, and what its cross-reference section
/// says. The `/ID` keeps the first string, the crypt's or else the
/// trailer's, and the second is a hash of what's written, so the same
/// bodies write the same bytes. An encrypted file with neither gets the
/// hash for both. A number the `/Encrypt` dictionary should go at that a
/// body has is refused, and so is any number a file may not use
/// ([`NUMBERS`](super::NUMBERS)): the section lists every number below the
/// last, so one past it would list billions.
///
/// Packed, what's written anew that may be goes in containers after the
/// `/Encrypt` dictionary, numbered from [`Pack::reuse`], then from past
/// every body and [`Layout::xref_from`], the cross-reference stream after
/// them. A container's members are written on their own instead when its
/// data would be more than the reader takes ([`objstm::fits`]), and all
/// of them when the cross-reference stream would be left no number, or
/// under an encryption that would leave their strings in the clear
/// ([`Crypt::packs`]).
pub fn write_bodies<'a>(
    bodies: impl IntoIterator<Item = Result<(ObjectId, Body<'a>)>>,
    layout: &Layout<'_>,
    p: &dyn Progress,
) -> Result<(Vec<u8>, XrefFacts)> {
    if layout.pack.is_some() && layout.kind == XrefKind::Table {
        bail!("a cross-reference table can't list objects in object streams");
    }
    let pack = (layout.pack.as_ref()).filter(|_| layout.crypt.is_none_or(Crypt::packs));
    std::thread::scope(|s| {
        let pack = pack.map(|pack| (pack.reuse, s.spawn(pack.uses)));
        file(bodies, layout, pack, p)
    })
}

/// The thread finding which pages use each object ([`Pack::uses`]).
type Walk<'scope> = ScopedJoinHandle<'scope, HashMap<u32, Use>>;

/// [`write_bodies`]'s file, packed when `pack` gives the numbers
/// containers take first and the thread finding the pages that use each
/// object.
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
    // What goes in containers, written once they're numbered.
    let mut members: Vec<Member<'a>> = Vec::new();
    for body in bodies {
        p.check()?;
        let (id, body) = body?;
        usable(id.0)?;
        next = next.max(id.0 + 1);
        let pos = match body {
            Body::Raw(bytes) => {
                let pos = out.len();
                out.extend_from_slice(bytes);
                out.push(b'\n');
                pos
            }
            Body::Object(obj, key) if pack.is_some() && objstm::packable(id, &obj) => {
                members.push((id, key, obj));
                continue;
            }
            Body::Object(obj, key) => super::indirect_as(&mut out, id, &key, &obj, layout.crypt)?,
        };
        section.insert(
            id.0,
            Entry::InUse {
                pos,
                generation: id.1,
            },
        );
    }
    let mut trailer = layout.trailer.clone();
    for key in SECTION_KEYS {
        trailer.remove(key);
    }
    if let Some(crypt) = layout.crypt {
        let at = layout.encrypt_at;
        usable(at.0)?;
        if section.lists(at.0) || members.iter().any(|(id, ..)| id.0 == at.0) {
            bail!("object {} is the encryption dictionary's", at.0);
        }
        let dict = Object::Dictionary(crypt.dict().clone());
        let pos = object::indirect(&mut out, at, &dict)?;
        section.insert(
            at.0,
            Entry::InUse {
                pos,
                generation: at.1,
            },
        );
        trailer.set("Encrypt", Object::Reference(at));
        next = next.max(at.0 + 1);
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
            next,
            p,
        )?;
    }
    let kept = (layout.crypt)
        .and_then(Crypt::first_id)
        .map(<[u8]>::to_vec)
        .or_else(|| super::first_id(layout.trailer));
    let id = super::file_id(kept, layout.crypt.is_some(), &out);
    if let Some(id) = &id {
        trailer.set("ID", id.clone());
    }
    let pos = out.len();
    let (size, max_stream) = match layout.kind {
        XrefKind::Table => {
            let size = section.size();
            section.table(&mut out, trailer, pos)?;
            (size, None)
        }
        XrefKind::Stream => {
            let at = next.max(layout.xref_from);
            usable(at)?;
            section.stream(&mut out, at, trailer, pos)?;
            (at + 1, Some(at))
        }
    };
    let id = match id {
        Some(Object::Array(ids)) => match ids.as_slice() {
            [Object::String(a, _), Object::String(b, _)] => Some([a.clone(), b.clone()]),
            _ => None,
        },
        _ => None,
    };
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

/// `pdf`, a document sheaf built in memory (blank pages, images as pages,
/// the stamp library's files), as a whole file through [`write_bodies`]:
/// each object written anew at its number, unencrypted, with a
/// cross-reference table and the header `pdf`'s version gives. The same
/// document writes the same bytes, so it interns as the same blob.
pub fn write_built(pdf: &lopdf::Document) -> Result<Vec<u8>> {
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
        encrypt_at: (0, 0),
        xref_from: 0,
        pack: None,
    };
    Ok(write_bodies(bodies, &layout, &in_progress::NoProgress)?.0)
}

/// `members` written after the rest of `out`, grouped by `uses` and
/// numbered from `reuse` first ([`Pack`]), and listed in `section`, whose
/// numbers are below `next`: each container, which lists its members, and
/// on their own those no container takes. What's next past the
/// containers.
#[allow(clippy::too_many_arguments)]
fn packed(
    out: &mut Vec<u8>,
    section: &mut Section,
    members: &[Member<'_>],
    uses: &HashMap<u32, Use>,
    reuse: &[u32],
    layout: &Layout<'_>,
    mut next: u32,
    p: &dyn Progress,
) -> Result<u32> {
    let by_number: HashMap<u32, &Member<'_>> = members.iter().map(|m| (m.0.0, m)).collect();
    let used = |n: &u32| uses.get(n).copied().unwrap_or(Use::None);
    let grouped = group::group(members.iter().map(|(id, ..)| (id.0, used(&id.0))));
    let taken = |n| section.lists(n) || by_number.contains_key(&n);
    let from = next.max(layout.xref_from);
    let Some(numbers) = numbers(grouped.len(), reuse, from, taken) else {
        for member in members {
            plain(out, section, member, layout.crypt)?;
        }
        return Ok(next);
    };
    let groups: Vec<(u32, Vec<&Member<'_>>)> = (numbers.into_iter().zip(grouped))
        .map(|(n, group)| (n, group.iter().map(|m| by_number[m]).collect()))
        .collect();
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get().min(8));
    let built = containers(&groups, layout.crypt, threads, p)?;
    for ((n, group), container) in groups.iter().zip(built) {
        let Some(container) = container else {
            for member in group {
                plain(out, section, member, layout.crypt)?;
            }
            continue;
        };
        let pos = object::indirect(out, (*n, 0), &container)?;
        section.insert(*n, Entry::InUse { pos, generation: 0 });
        for (index, (id, ..)) in (0..).zip(group) {
            let entry = Entry::Packed {
                container: *n,
                index,
            };
            section.insert(id.0, entry);
        }
        next = next.max(n + 1);
    }
    Ok(next)
}

/// The numbers `count` containers take: `reuse`'s first, in order, but 0,
/// one a file may not use, one `taken` and one given already; then from
/// `from` up. None when there aren't so many, or the cross-reference
/// stream after them would take a number a file may not use.
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

/// `member` written on its own after the rest of `out`, and listed in
/// `section`.
fn plain(
    out: &mut Vec<u8>,
    section: &mut Section,
    (id, key, obj): &Member<'_>,
    crypt: Option<&Crypt>,
) -> Result<()> {
    let pos = super::indirect_as(out, *id, key, obj, crypt)?;
    section.insert(
        id.0,
        Entry::InUse {
            pos,
            generation: id.1,
        },
    );
    Ok(())
}

/// Each of `groups`, a container's number and its members, as an object
/// stream ([`objstm::container`]), encrypted with `crypt` when given,
/// under its number, its IVs keyed by [`ObjKey::Container`] and what it
/// holds: None for one whose data would be more than the reader takes.
/// Built on up to `threads` threads, 4 containers to one at least, `p`
/// checked before each, and joined in order, so the same groups build the
/// same containers however many threads build them.
pub(super) fn containers(
    groups: &[(u32, Vec<&Member<'_>>)],
    crypt: Option<&Crypt>,
    threads: usize,
    p: &dyn Progress,
) -> Result<Vec<Option<Object>>> {
    let build = |part: &[(u32, Vec<&Member<'_>>)]| -> Result<Vec<Option<Object>>> {
        (part.iter())
            .map(|(n, members)| {
                p.check()?;
                container(*n, members, crypt)
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

/// `members` as object stream `n`, encrypted with `crypt` when given: None
/// when its data wouldn't [`fit`](objstm::fits).
fn container(n: u32, members: &[&Member<'_>], crypt: Option<&Crypt>) -> Result<Option<Object>> {
    let mut written = Vec::with_capacity(members.len());
    for (id, _, obj) in members.iter().copied() {
        let mut bytes = Vec::new();
        object::write(&mut bytes, obj).map_err(|e| e.within(*id))?;
        written.push((id.0, bytes));
    }
    let Some(stream) = objstm::container(&written) else {
        return Ok(None);
    };
    let stream = Object::Stream(stream);
    let Some(crypt) = crypt else {
        return Ok(Some(stream));
    };
    let encrypted = (crypt.encrypt_as((n, 0), &ObjKey::Container(n), &stream))
        .with_context(|| format!("encrypting object {n} 0"))?;
    Ok(Some(encrypted))
}

/// Whether object number `n` is one a file may use.
fn usable(n: u32) -> Result<()> {
    if n >= super::NUMBERS {
        bail!(
            "object {n} is past the {} numbers a PDF may use",
            super::NUMBERS
        );
    }
    Ok(())
}

/// Whether `obj` is one a full write leaves out: an object stream (its
/// members are written anew, on their own or in the write's containers),
/// a cross-reference stream or a linearization dictionary.
pub(crate) fn left_out(obj: &Object) -> bool {
    match obj {
        Object::Stream(s) => matches!(s.dict.get_type(), Ok(t) if t == b"ObjStm" || t == b"XRef"),
        Object::Dictionary(d) => d.has(b"Linearized"),
        _ => false,
    }
}
