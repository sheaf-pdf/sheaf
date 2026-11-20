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

    /// Object `id`, its references followed as [`object`] follows them,
    /// borrowed; None where it can't be read. A source lends a stream's
    /// dictionary without reading or keeping its data, which [`read`]
    /// gives: what a font's objects are found by ([`Objects`]), so a
    /// font's program and CMaps aren't held once the font is read.
    ///
    /// [`object`]: ReadObjects::object
    /// [`read`]: ReadObjects::read
    fn peek(&self, id: ObjectId) -> Option<&Object>;
}

/// A source's or a document's objects as the PDF layer's crates read
/// them: [`Resolve`] by [`ReadObjects::peek`], so a reference to a
/// reference is followed as sheaf follows it everywhere, and [`Streams`]
/// by [`ReadObjects::read`] and sheaf's own decoding, a stream read once
/// and not kept. What pdf-fonts reads a font from, and what interprets a
/// page with it takes the same.
#[derive(Debug)]
pub struct Objects<'a, R: ?Sized>(pub &'a R);

impl<R: ?Sized> Clone for Objects<'_, R> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<R: ?Sized> Copy for Objects<'_, R> {}

impl<'a, R: ReadObjects + ?Sized> Resolve<'a> for Objects<'a, R> {
    fn resolve(&self, id: ObjectId) -> Option<&'a Object> {
        self.0.peek(id)
    }
}

impl<R: ReadObjects + ?Sized> Streams for Objects<'_, R> {
    /// Stream `id` decoded, as sheaf decodes every stream. Past `limit`
    /// it's [`Why::Limit`]; anything else that gives no data (no such
    /// object, not a stream, a filter that fails, a source that doesn't
    /// read) is [`Why::Damaged`], which a font reads as none.
    fn decoded(&self, id: ObjectId, limit: usize) -> Result<Cow<'_, [u8]>, Unreadable> {
        let read = self.0.read(id, |object| {
            let stream = object.as_stream()?;
            Some(stream.decompressed_content_with_limit(limit))
        });
        match read {
            Ok(Some(Ok(data))) => Ok(Cow::Owned(data)),
            Ok(Some(Err(crate::pdf::Error::Limit(crate::pdf::Limit::Decompressed)))) => {
                Err(Unreadable::new(id, Why::Limit, "decodes past the limit"))
            }
            Ok(Some(Err(error))) => Err(Unreadable::new(id, Why::Damaged, error.to_string())),
            Ok(None) => Err(Unreadable::new(id, Why::Damaged, "not a stream")),
            Err(error) => Err(Unreadable::new(id, Why::Damaged, error.to_string())),
        }
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

    fn peek(&self, id: ObjectId) -> Option<&Object> {
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

    fn peek(&self, id: ObjectId) -> Option<&Object> {
        self.get_object(id).ok()
    }
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
