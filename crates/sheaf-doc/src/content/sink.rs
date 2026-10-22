//! Where content's resources are written (M10.5 slice 9): a [`Sink`] reads
//! what a dictionary names and writes the resources a [`ContentWriter`]'s
//! content draws with, once however many draw with them, so a mark's
//! appearance form ([`bind`]) serves both the lopdf document an old writer
//! builds ([`Lopdf`]) and the emitter, whose dictionaries refer to objects
//! by key; the emitter reads a page's `/Contents` and `/Resources` through
//! it too.
//!
//! A dictionary and what a sink reads for it are in one space: an old
//! writer's object numbers, or the emitter's placeholders.
//!
//! [`ContentWriter`]: super::ContentWriter

use lopdf::{Dictionary, Document as Pdf, Object, ObjectId};

use super::names::ResKind;
use super::page::ReadObjects;
use super::res::Res;
use super::to_lopdf::ResObjects;
use super::writer::{Reserved, ResourceNeeds};

/// What content's resources are read through and written to.
pub trait Sink {
    /// The dictionary object `id` is, references followed: None for
    /// anything else, or what can't be read.
    fn dict(&mut self, id: ObjectId) -> Option<Dictionary>;

    /// The array object `id` is.
    fn array(&mut self, id: ObjectId) -> Option<Vec<Object>>;

    /// `res`'s object, written once however many pages draw with it.
    fn resource(&mut self, res: &Res) -> ObjectId;

    /// The names a page's `resources` binds, which new content keeps
    /// clear of.
    fn reserved(&mut self, resources: Option<&Object>) -> Reserved;
}

/// The lopdf document an old writer builds, and the resources it has
/// written.
pub struct Lopdf<'a> {
    pub pdf: &'a mut Pdf,
    pub res: &'a mut ResObjects,
}

impl Sink for Lopdf<'_> {
    fn dict(&mut self, id: ObjectId) -> Option<Dictionary> {
        self.pdf.get_dictionary(id).ok().cloned()
    }

    fn array(&mut self, id: ObjectId) -> Option<Vec<Object>> {
        self.pdf.get_object(id).ok()?.as_array().ok().cloned()
    }

    fn resource(&mut self, res: &Res) -> ObjectId {
        self.res.object(self.pdf, res)
    }

    fn reserved(&mut self, resources: Option<&Object>) -> Reserved {
        Reserved::read(self.pdf, resources)
    }
}

/// The entries of `page`'s `/Contents`, an array's or the one stream's,
/// never decoded.
pub fn content_entries(sink: &mut dyn Sink, page: &Dictionary) -> Vec<Object> {
    match page.get(b"Contents").ok() {
        None => Vec::new(),
        Some(Object::Array(entries)) => entries.clone(),
        Some(Object::Reference(r)) => sink
            .array(*r)
            .unwrap_or_else(|| vec![Object::Reference(*r)]),
        Some(other) => vec![other.clone()],
    }
}

/// `page`'s `/Resources`, a copy to change: a page shares its dictionary
/// with others by reference.
pub fn resources(sink: &mut dyn Sink, page: &Dictionary) -> Dictionary {
    match page.get(b"Resources").ok() {
        Some(Object::Reference(r)) => sink.dict(*r).unwrap_or_default(),
        Some(Object::Dictionary(d)) => d.clone(),
        _ => Dictionary::new(),
    }
}

/// `needs` written through `sink` and bound in `resources`, a resources
/// dictionary about to be written: each under its name in its category.
/// A category that's a reference is copied in first, so a dictionary
/// other pages share is never changed.
pub fn bind(sink: &mut dyn Sink, needs: &ResourceNeeds, resources: &mut Dictionary) {
    for need in &needs.0 {
        let id = sink.resource(&need.res);
        let key = need.kind.key();
        let mut category = match resources.get(key) {
            Ok(Object::Reference(r)) => sink.dict(*r).unwrap_or_default(),
            Ok(Object::Dictionary(d)) => d.clone(),
            _ => Dictionary::new(),
        };
        category.set(need.name.clone(), id);
        resources.set(key, category);
    }
}

/// The names `resources` binds as far as `sink` reads them: the
/// dictionary and its categories followed, each name's binding as
/// written. What a binding that's a reference names isn't read, so a name
/// bound to one is kept clear of whatever it is.
pub fn reserved_shallow(sink: &mut dyn Sink, resources: Option<&Object>) -> Reserved {
    let mut dict = match resources {
        Some(Object::Reference(r)) => sink.dict(*r).unwrap_or_default(),
        Some(Object::Dictionary(d)) => d.clone(),
        _ => return Reserved::default(),
    };
    for kind in ResKind::ALL {
        if let Ok(&Object::Reference(r)) = dict.get(kind.key()) {
            let category = sink.dict(r).unwrap_or_default();
            dict.set(kind.key(), category);
        }
    }
    Reserved::read(&Unread, Some(&Object::Dictionary(dict)))
}

/// Objects none of which can be read.
struct Unread;

impl ReadObjects for Unread {
    fn object(&self, id: ObjectId) -> lopdf::Result<&Object> {
        Err(lopdf::Error::ObjectNotFound(id))
    }

    fn read<R>(&self, id: ObjectId, _: impl FnOnce(&Object) -> R) -> lopdf::Result<R> {
        Err(lopdf::Error::ObjectNotFound(id))
    }
}
