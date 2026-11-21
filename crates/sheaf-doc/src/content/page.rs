//! A page's content as one: the logical concatenation of its `/Contents`
//! streams. A marked range or a `q` may open in one stream and close in
//! the next, so a page's streams are lexed as PDFium reads them, joined
//! with a space between each ([`Concat`]), and what's found is mapped back
//! to a stream and a place in it ([`Pos`], [`SpanAcross`]). Those three
//! are pdf-content-streams', as is the join a page's streams are read
//! into.
//!
//! [`PageContent`] reads the streams through [`ReadObjects`]: a source,
//! which reads each without keeping it, or a document in memory (the old
//! writers' copies, a stamp collection). A stream that won't decode, or
//! streams that decode past [`ContentLimits::decompressed`] together, are
//! a [`ContentFault`], never skipped and never taken as raw bytes.
//!
//! [`ContentLimits::decompressed`]: crate::source::ContentLimits

use std::borrow::Cow;
use std::cell::{OnceCell, RefCell};
use std::collections::HashMap;

use crate::pdf::content::Join;
pub use crate::pdf::content::{Concat, Pos, SpanAcross};
use crate::pdf::{
    Dictionary, LazyPdf, Object, ObjectId, Resolve, Stream, StreamExt, Streams, Unreadable, Value,
    Why,
};

use super::lex::{self, LexError, ResCx};
use crate::source::Limits;

/// What a page's content is read from: a source, or a document in memory.
/// The toolkit's own, so it outlives the eager documents.
pub trait ReadObjects {
    /// Object `id`, its references followed, read once and kept where the
    /// reader keeps what it reads: resources, which pages share.
    fn object(&self, id: ObjectId) -> crate::pdf::Result<&Object>;

    /// Object `id`, its references followed, handed to `read`; a source
    /// reads it without keeping it: a page's streams, each read once to
    /// lex.
    fn read<R>(&self, id: ObjectId, read: impl FnOnce(&Object) -> R) -> crate::pdf::Result<R>;

    /// Object `id`, its references followed as [`object`] follows them:
    /// borrowed where the source keeps it, and where it doesn't, read for
    /// the asker and not kept. None where there's no such object, and the
    /// error where it's there and doesn't read. A source keeps what
    /// [`object`] keeps, but a stream it hasn't kept is read as its
    /// dictionary alone, its data neither read nor kept (which [`read`]
    /// gives), and handed over: what the PDF layer's crates find a page's
    /// and a font's objects by ([`Objects`]), so neither an XObject drawn
    /// nor a font's program or CMaps is held once a page is interpreted.
    ///
    /// [`object`]: ReadObjects::object
    /// [`read`]: ReadObjects::read
    fn peek(&self, id: ObjectId) -> crate::pdf::Result<Option<Cow<'_, Object>>>;
}

/// Why `error`, a source's failure to read an object, is: the reason the
/// PDF layer's crates are told ([`Unreadable`]). The source's I/O is
/// [`Why::Source`]; a password or decryption that fails, [`Why::Locked`];
/// one of the reader's caps, [`Why::Capped`]; anything else, the file's,
/// [`Why::Damaged`]: a loop of references among them
/// ([`ReferenceCycle`](crate::pdf::Error::ReferenceCycle)), which is the
/// same under any cap, where a chain that is only long is the cap's.
pub(crate) fn why(error: &crate::pdf::Error) -> Why {
    use crate::pdf::Error as E;
    match error {
        E::IO(_) => Why::Source,
        E::InvalidPassword | E::Decryption(_) => Why::Locked,
        E::Limit(_) | E::ReferenceLimit => Why::Capped,
        _ => Why::Damaged,
    }
}

/// `error`, reading `id`, as the PDF layer's crates are told it.
fn unreadable(id: ObjectId, error: &crate::pdf::Error) -> Unreadable {
    Unreadable::new(id, why(error), error.to_string())
}

/// A source's or a document's objects as the PDF layer's crates read
/// them, for as long as `unkept` lives: [`Resolve`] by
/// [`ReadObjects::peek`], so a reference to a reference is followed as
/// sheaf follows it everywhere, what the source keeps lent from it, and
/// what it doesn't (a stream's dictionary, its data unread) read once and
/// held by `unkept`, not the source ([`Unkept`]); and [`Streams`] by
/// [`ReadObjects::read`] and sheaf's own decoding, a stream read once and
/// not kept. An object that doesn't read, or a stream that doesn't
/// decode, has its reason: the source's I/O is [`Why::Source`], a
/// password or decryption that fails [`Why::Locked`], one of the reader's
/// caps [`Why::Capped`], and anything else, the file's, [`Why::Damaged`].
/// What pdf-fonts reads a font from, and pdf-gfx a page, each
/// interpretation with an `unkept` of its own ([`interp`](super::interp)),
/// so interpreting pages keeps nothing more of the source than its kept
/// objects.
#[derive(Debug)]
pub struct Objects<'a, R: ?Sized> {
    source: &'a R,
    unkept: &'a Unkept,
}

impl<'a, R: ?Sized> Objects<'a, R> {
    /// `source`'s objects, what it doesn't keep held by `unkept`.
    pub fn new(source: &'a R, unkept: &'a Unkept) -> Objects<'a, R> {
        Objects { source, unkept }
    }
}

impl<R: ?Sized> Clone for Objects<'_, R> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<R: ?Sized> Copy for Objects<'_, R> {}

impl<'a, R: ReadObjects + ?Sized> Resolve<'a> for Objects<'a, R> {
    fn resolve(&self, id: ObjectId) -> Option<&'a Object> {
        self.try_resolve(id).ok().flatten()
    }

    /// Object `id` as [`ReadObjects::peek`] lends it, what the source
    /// doesn't keep held by [`Unkept`] and read once: none where there's
    /// no such object, and why where it doesn't read, as [`Objects`]
    /// says.
    fn try_resolve(&self, id: ObjectId) -> Result<Option<&'a Object>, Unreadable> {
        if let Some(held) = self.unkept.get(id) {
            return Ok(Some(held));
        }
        match self.source.peek(id) {
            Ok(Some(Cow::Borrowed(kept))) => Ok(Some(kept)),
            Ok(Some(Cow::Owned(read))) => Ok(Some(self.unkept.hold(id, read))),
            Ok(None) => Ok(None),
            Err(error) => Err(unreadable(id, &error)),
        }
    }
}

impl<R: ReadObjects + ?Sized> Streams for Objects<'_, R> {
    /// Stream `id` decoded, as sheaf decodes every stream. Past `limit`
    /// it's [`Why::Limit`]; what isn't a stream is [`Why::Damaged`]; and
    /// one the source doesn't read (no such object among them), or whose
    /// filters fail, has its error's reason, as [`Objects`] says.
    fn decoded(&self, id: ObjectId, limit: usize) -> Result<Cow<'_, [u8]>, Unreadable> {
        let read = self.source.read(id, |object| {
            let stream = object.as_stream()?;
            Some(stream.decompressed_content_with_limit(limit))
        });
        match read {
            Ok(Some(Ok(data))) => Ok(Cow::Owned(data)),
            Ok(Some(Err(crate::pdf::Error::Limit(crate::pdf::Limit::Decompressed)))) => {
                Err(Unreadable::new(id, Why::Limit, "decodes past the limit"))
            }
            Ok(Some(Err(error))) | Err(error) => Err(unreadable(id, &error)),
            Ok(None) => Err(Unreadable::new(id, Why::Damaged, "not a stream")),
        }
    }
}

/// What [`Objects`] lends that its source doesn't keep, such as an
/// XObject's or a font's stream's dictionary: each object read once, by
/// the id it was asked for by, and held here, not by the source, until
/// this is dropped. An interpretation has one of its own, so a page
/// interpreted holds nothing of the source once it's done, however many
/// pages are.
#[derive(Debug, Default)]
pub struct Unkept {
    /// Where each is among `held`, by the id it was asked for by.
    at: RefCell<HashMap<ObjectId, usize>>,
    held: Places,
}

impl Unkept {
    /// The object held for `id`, if one is.
    fn get(&self, id: ObjectId) -> Option<&Object> {
        let n = *self.at.borrow().get(&id)?;
        self.held.at(n).get()
    }

    /// `object`, read for `id`, held for as long as this lives; or the one
    /// held for it already.
    fn hold(&self, id: ObjectId, object: Object) -> &Object {
        let n = {
            let mut at = self.at.borrow_mut();
            let next = at.len();
            *at.entry(id).or_insert(next)
        };
        self.held.at(n).get_or_init(|| object)
    }
}

/// Places for objects, none moved once it's filled, so that each can be
/// lent for as long as they live: a run of them, then the next run, twice
/// as long, made when it's first needed.
#[derive(Debug, Default)]
struct Places {
    run: Box<[OnceCell<Object>]>,
    next: OnceCell<Box<Places>>,
}

impl Places {
    /// The `n`th place, counted across the runs.
    fn at(&self, mut n: usize) -> &OnceCell<Object> {
        let mut places = self;
        while n >= places.run.len() {
            n -= places.run.len();
            let len = (2 * places.run.len()).max(8);
            places = places.next.get_or_init(|| {
                Box::new(Places {
                    run: (0..len).map(|_| OnceCell::new()).collect(),
                    next: OnceCell::new(),
                })
            });
        }
        &places.run[n]
    }
}

impl ReadObjects for LazyPdf {
    fn object(&self, id: ObjectId) -> crate::pdf::Result<&Object> {
        self.get_object(id)
    }

    fn read<R>(&self, id: ObjectId, read: impl FnOnce(&Object) -> R) -> crate::pdf::Result<R> {
        let mut object = self.object_unkept(id)?;
        let mut chain = 0;
        while let Some(next) = object.as_reference() {
            chain += 1;
            if chain > Limits::DEFAULT.reader.max_depth {
                return Err(crate::pdf::Error::Limit(crate::pdf::Limit::Depth));
            }
            object = self.object_unkept(next)?;
        }
        Ok(read(&object))
    }

    fn peek(&self, id: ObjectId) -> crate::pdf::Result<Option<Cow<'_, Object>>> {
        LazyPdf::peek(self, id)
    }
}

impl ReadObjects for crate::pdf::Document {
    fn object(&self, id: ObjectId) -> crate::pdf::Result<&Object> {
        self.get_object(id)
    }

    fn read<R>(&self, id: ObjectId, read: impl FnOnce(&Object) -> R) -> crate::pdf::Result<R> {
        self.get_object(id).map(read)
    }

    /// Object `id`, which a document in memory keeps, as every object. A
    /// chain of references that comes back to an object of its own is a
    /// loop, the document's damage
    /// ([`ReferenceCycle`](crate::pdf::Error::ReferenceCycle)), where one
    /// that is only long is the limit's.
    fn peek(&self, id: ObjectId) -> crate::pdf::Result<Option<Cow<'_, Object>>> {
        use crate::pdf::Error;
        match self.get_object(id) {
            Ok(object) => Ok(Some(Cow::Borrowed(object))),
            Err(Error::ObjectNotFound(_)) => Ok(None),
            Err(Error::ReferenceLimit) => Err(match looped(self, id) {
                Some(at) => Error::ReferenceCycle(at),
                None => Error::ReferenceLimit,
            }),
            Err(error) => Err(error),
        }
    }
}

/// Where the chain of references from object `id` of `document` comes
/// back to an object of its own; None where it ends, however long. Asked
/// once a chain has passed the document's limit, so one within it pays
/// nothing.
fn looped(document: &crate::pdf::Document, id: ObjectId) -> Option<ObjectId> {
    let mut chain = vec![id];
    while let Some(next) = (document.objects.get(&chain[chain.len() - 1]))?.as_reference() {
        if chain.contains(&next) {
            return Some(next);
        }
        chain.push(next);
    }
    None
}

/// Why a page's content can't be read.
#[derive(Debug, thiserror::Error)]
pub enum ContentFault {
    #[error("stream {} {} R can't be read: {error}", .stream.number, .stream.generation)]
    Stream {
        stream: ObjectId,
        error: crate::pdf::Error,
    },
    #[error("its streams decode to more than {0} bytes")]
    TooLarge(usize),
    /// `error`'s offset is into `stream`.
    #[error("stream {} {} R: {error}", .stream.number, .stream.generation)]
    Lex { stream: ObjectId, error: LexError },
}

/// A page's content: its streams, decoded, as one.
#[derive(Debug, Clone)]
pub struct PageContent {
    /// Each stream's object, in order.
    ids: Box<[ObjectId]>,
    /// Each stream's dictionary: what marks it as one of sheaf's.
    dicts: Box<[Dictionary]>,
    concat: Concat<'static>,
}

impl PageContent {
    /// The streams `contents` names, decoded: what [`PageFacts::contents`]
    /// holds, where a reference to an array is that array's streams. What
    /// isn't a stream (null, a missing object) draws nothing, as in
    /// PDFium.
    ///
    /// [`PageFacts::contents`]: crate::source::PageFacts
    pub fn read(
        objects: &impl ReadObjects,
        contents: &[ObjectId],
        lim: &Limits,
    ) -> Result<PageContent, ContentFault> {
        let cap = lim.content.decompressed;
        let mut ids = Vec::new();
        let mut dicts = Vec::new();
        let mut join = Join::new(&lim.content.family());
        let mut take = |id: ObjectId, stream: &Stream| -> Result<(), ContentFault> {
            let data = stream
                .decompressed_content_with_limit(join.room())
                .map_err(|e| match e {
                    crate::pdf::Error::Limit(crate::pdf::Limit::Decompressed) => {
                        ContentFault::TooLarge(cap)
                    }
                    e => ContentFault::Stream {
                        stream: id,
                        error: e,
                    },
                })?;
            join.push(&data).map_err(|_| ContentFault::TooLarge(cap))?;
            ids.push(id);
            dicts.push(stream.dict.clone());
            Ok(())
        };
        for &id in contents {
            let entries = objects
                .read(id, |object| match object.value() {
                    Value::Stream(stream) => take(id, stream).map(|()| Vec::new()),
                    Value::Array(entries) => {
                        Ok(entries.iter().filter_map(Object::as_reference).collect())
                    }
                    _ => Ok(Vec::new()),
                })
                .or_else(|e| missing(id, e).map(|()| Ok(Vec::new())))??;
            for entry in entries {
                objects
                    .read(entry, |object| match object.value() {
                        Value::Stream(stream) => take(entry, stream),
                        _ => Ok(()),
                    })
                    .or_else(|e| missing(entry, e).map(Ok))??;
            }
        }
        Ok(PageContent {
            ids: ids.into(),
            dicts: dicts.into(),
            concat: join.finish(),
        })
    }

    /// Each stream's object, in order.
    pub fn ids(&self) -> &[ObjectId] {
        &self.ids
    }

    /// Each stream's dictionary, in order.
    pub fn dicts(&self) -> &[Dictionary] {
        &self.dicts
    }

    /// The streams, decoded, as one.
    pub fn concat(&self) -> &Concat<'static> {
        &self.concat
    }

    /// The joined bytes, to draw as they are: a copy, as
    /// pdf-content-streams' [`Concat`] lends its bytes and doesn't give
    /// them up.
    pub fn into_bytes(self) -> Vec<u8> {
        self.concat.bytes().to_vec()
    }

    /// The fault `error`, from lexing [`PageContent::concat`], is: in
    /// which stream, and where in it.
    pub fn fault(&self, error: LexError) -> ContentFault {
        let pos = self.concat.pos(error.at);
        ContentFault::Lex {
            stream: self
                .ids
                .get(pos.stream as usize)
                .copied()
                .unwrap_or(ObjectId::new(0, 0)),
            error: LexError {
                at: pos.at as usize,
                ..error
            },
        }
    }
}

/// A read of `id` that failed: Ok when there's no such object, which
/// reads as null.
fn missing(id: ObjectId, e: crate::pdf::Error) -> Result<(), ContentFault> {
    match e {
        crate::pdf::Error::MissingXrefEntry | crate::pdf::Error::ObjectNotFound(_) => Ok(()),
        error => Err(ContentFault::Stream { stream: id, error }),
    }
}

/// The streams a page's `/Contents` names: one, or a direct array's. A
/// reference to an array stays that one reference, since telling the two
/// apart reads the stream it usually is.
pub fn contents(page: &Dictionary) -> Vec<ObjectId> {
    match page.get(b"Contents").map(Object::value) {
        Some(Value::Ref(id)) => vec![*id],
        Some(Value::Array(streams)) => streams.iter().filter_map(Object::as_reference).collect(),
        _ => Vec::new(),
    }
}

impl ResCx {
    /// The named color spaces of `resources`, a page's or a form's
    /// `/Resources` as written.
    pub fn read(objects: &impl ReadObjects, resources: Option<&Object>) -> ResCx {
        let spaces = resources
            .and_then(|r| direct(objects, r))
            .and_then(Object::as_dict)
            .and_then(|r| direct(objects, r.get(b"ColorSpace")?))
            .and_then(Object::as_dict);
        ResCx::new(spaces.into_iter().flat_map(|spaces| {
            spaces
                .iter()
                .map(|(name, space)| (name.to_vec(), components(objects, space)))
        }))
    }
}

/// The components a color space in the resources has, as PDFium counts
/// them, 3 where it can't make one out.
fn components(objects: &impl ReadObjects, space: &Object) -> u32 {
    let direct = |o| direct(objects, o);
    let Some(space) = direct(space) else {
        return 3;
    };
    let parts = match space.value() {
        Value::Name(family) => return lex::family_components(family.as_bytes()),
        Value::Array(parts) => parts,
        _ => return 3,
    };
    let family = parts.first().and_then(direct).and_then(Object::as_name);
    let second = || parts.get(1).and_then(direct);
    match family {
        Some(b"ICCBased") => second()
            .and_then(Object::as_stream)
            .and_then(|s| s.dict.get(b"N")?.as_integer())
            .filter(|n| matches!(n, 1 | 3 | 4))
            .map_or(3, |n| n as u32),
        Some(b"DeviceN") => second()
            .and_then(Object::as_array)
            .map(|names| names.len() as u32)
            .filter(|&n| n > 0)
            .unwrap_or(3),
        Some(family) => lex::family_components(family),
        None => 3,
    }
}

/// `object`, or what the reference it is names.
pub(super) fn direct<'a>(objects: &'a impl ReadObjects, object: &'a Object) -> Option<&'a Object> {
    match object.as_reference() {
        Some(id) => objects.object(id).ok(),
        None => Some(object),
    }
}

#[cfg(test)]
mod tests;
