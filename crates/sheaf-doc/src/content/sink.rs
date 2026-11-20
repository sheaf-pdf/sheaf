//! Where content's resources are written (M10.5 slice 9): a [`Sink`] reads
//! what a dictionary names and writes the resources a [`ContentWriter`]'s
//! content draws with, once however many draw with them, so a mark's
//! appearance form ([`bind`]) serves both a PDF built in memory
//! ([`serialize::built`](crate::serialize::built)) and the emitter, whose
//! dictionaries refer to objects by key; the emitter reads a page's
//! `/Contents` and `/Resources` through it too.
//!
//! A dictionary and what a sink reads for it are in one space: a built
//! PDF's object numbers, or the emitter's placeholders.
//!
//! [`ContentWriter`]: super::ContentWriter

use crate::pdf::content::Limits;
use crate::pdf::{Dictionary, Object, ObjectId, Value, resolve_fn};
use crate::source;

use super::{Res, ResKind, Reserved, ResourceNeeds};

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

/// The entries of `page`'s `/Contents`, an array's or the one stream's,
/// never decoded.
pub fn content_entries(sink: &mut dyn Sink, page: &Dictionary) -> Vec<Object> {
    match page.get(b"Contents") {
        None => Vec::new(),
        Some(object) => match object.value() {
            Value::Array(entries) => entries.clone(),
            Value::Ref(r) => sink.array(*r).unwrap_or_else(|| vec![Object::from(*r)]),
            _ => vec![object.clone()],
        },
    }
}

/// `page`'s `/Resources`, a copy to change: a page shares its dictionary
/// with others by reference.
pub fn resources(sink: &mut dyn Sink, page: &Dictionary) -> Dictionary {
    match page.get(b"Resources").map(Object::value) {
        Some(Value::Ref(r)) => sink.dict(*r).unwrap_or_default(),
        Some(Value::Dict(d)) => d.clone(),
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
        let mut category = match resources.get(key).map(Object::value) {
            Some(Value::Ref(r)) => sink.dict(*r).unwrap_or_default(),
            Some(Value::Dict(d)) => d.clone(),
            _ => Dictionary::new(),
        };
        category.set(need.name.clone(), id);
        resources.set(key, category);
    }
}

/// The caps a resources dictionary's names are read under
/// ([`Reserved::read`]): sheaf's defaults, as pdf-content-streams takes
/// them ([`ContentLimits::family`]). A sink reads for a file being
/// written, of pages of any source, so it has no one document's limits;
/// the read follows references a few in a row and no further.
///
/// [`ContentLimits::family`]: source::ContentLimits::family
pub fn naming_limits() -> Limits {
    source::Limits::DEFAULT.content.family()
}

/// The names `resources` binds as far as `sink` reads them: the
/// dictionary and its categories followed, each name's binding as
/// written. What a binding that's a reference names isn't read, so a name
/// bound to one is kept clear of whatever it is.
pub fn reserved_shallow(sink: &mut dyn Sink, resources: Option<&Object>) -> Reserved {
    let mut dict = match resources.map(Object::value) {
        Some(Value::Ref(r)) => sink.dict(*r).unwrap_or_default(),
        Some(Value::Dict(d)) => d.clone(),
        _ => return Reserved::default(),
    };
    for kind in ResKind::ALL {
        if let Some(r) = dict.get(kind.key()).and_then(Object::as_reference) {
            let category = sink.dict(r).unwrap_or_default();
            dict.set(kind.key(), category);
        }
    }
    let nothing = resolve_fn(|_: ObjectId| None::<&Object>);
    Reserved::read(nothing, Some(&Object::from(dict)), &naming_limits())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::{ContentWriter, DrawFont, DrawText, Glyphless};
    use crate::pdf::{Document as Pdf, Stream, StreamExt, dictionary};
    use crate::serialize::built::ResObjects;

    fn list(subtype: &str) -> Dictionary {
        dictionary! { "Type" => Object::name("Pagination"), "Subtype" => Object::name(subtype) }
    }

    fn props(subtype: &str) -> Res {
        Res::dict(list(subtype)).unwrap()
    }

    /// The needs written into a document once a save, and bound in each
    /// resources dict: a category by reference is copied first.
    #[test]
    fn needs_are_written_once_and_bound() {
        let mut w = ContentWriter::new();
        w.set_font(&Glyphless, 10.0);
        w.begin_marked("Artifact", props("Header"));
        w.end_marked();
        let (_, needs) = w.finish(&Reserved::default());
        let mut pdf = Pdf::with_version("1.7");
        let shared_fonts =
            pdf.add_object(dictionary! { "F1" => Object::from(ObjectId::new(1, 0)) });
        let mut res = ResObjects::default();
        let mut first = dictionary! { "Font" => shared_fonts };
        let mut second = Dictionary::new();
        res.bind(&mut pdf, &needs, &mut first);
        let count = pdf.objects.len();
        res.bind(&mut pdf, &needs, &mut second);
        assert_eq!(pdf.objects.len(), count, "nothing written twice");
        assert_eq!(first.get(b"Font").unwrap().as_dict().unwrap().len(), 2);
        assert_eq!(
            pdf.get_dictionary(shared_fonts).unwrap().len(),
            1,
            "the shared dict is left alone"
        );
        for key in [&b"Font"[..], b"Properties"] {
            let category = |r: &Dictionary| r.get(key).unwrap().as_dict().unwrap().clone();
            let [font, list] = &needs.0[..] else { panic!() };
            let name = if key == b"Font" {
                &font.name
            } else {
                &list.name
            };
            assert_eq!(
                category(&first).get(name).unwrap(),
                category(&second).get(name).unwrap()
            );
        }
        // Glyphless is its six objects, its program compressed.
        let font = res.object(&mut pdf, &Glyphless.res());
        let type0 = pdf.get_dictionary(font).unwrap();
        let to_unicode = type0.get(b"ToUnicode").unwrap().as_reference().unwrap();
        let cmap = pdf.get_object(to_unicode).unwrap().as_stream().unwrap();
        assert!(
            cmap.decompressed_content()
                .unwrap()
                .windows(21)
                .any(|w| w == b"<0000> <FFFF> <0000>\n")
        );
        // A stream once a save.
        let q = || Res::of([Stream::new(dictionary! {}, b"q\n".to_vec()).into()]).unwrap();
        let q = (res.object(&mut pdf, &q()), res.object(&mut pdf, &q()));
        assert_eq!(q.0, q.1);
        let q = q.0;
        let q = pdf.get_object(q).unwrap().as_stream().unwrap();
        assert_eq!(q.decompressed_content().unwrap(), b"q\n");
    }

    /// A sink over a document's objects that writes nothing.
    struct Reads<'a>(&'a Pdf);

    impl Sink for Reads<'_> {
        fn dict(&mut self, id: ObjectId) -> Option<Dictionary> {
            self.0.get_dictionary(id).ok().cloned()
        }

        fn array(&mut self, _: ObjectId) -> Option<Vec<Object>> {
            None
        }

        fn resource(&mut self, _: &Res) -> ObjectId {
            unreachable!("nothing is written")
        }

        fn reserved(&mut self, resources: Option<&Object>) -> Reserved {
            reserved_shallow(self, resources)
        }
    }

    /// Read shallowly, the resources and a category of them are followed
    /// through the sink, but not what a name binds by reference: a name
    /// bound to an equal list directly is used again, one bound to the
    /// very object a resource is written at is too, and one bound to any
    /// other reference is kept clear of, an equal list behind it or not.
    #[test]
    fn a_shallow_read_follows_categories_not_bindings() {
        let mut w = ContentWriter::new();
        w.begin_marked("Artifact", props("Header"));
        w.draw(Res::written(ObjectId::new(40, 0)));
        let (_, plain) = w.named(&Reserved::default());
        let (header, form) = (plain.0[0].name.clone(), plain.0[1].name.clone());

        let mut pdf = Pdf::with_version("1.7");
        let same = pdf.add_object(list("Header"));
        let names = |pdf: &Pdf, resources: Object| {
            let reserved = reserved_shallow(&mut Reads(pdf), Some(&resources));
            let (_, needs) = w.named(&reserved);
            (needs.0[0].name.clone(), needs.0[1].name.clone())
        };
        let by = |bound: Object| dictionary! { header.clone() => bound };
        // Direct, and equal: the name again.
        let direct = Object::from(dictionary! { "Properties" => by(list("Header").into()) });
        assert_eq!(names(&pdf, direct).0, header);
        // Direct, and another list: counted past.
        let other = Object::from(dictionary! { "Properties" => by(list("Other").into()) });
        assert_ne!(names(&pdf, other).0, header);
        // By reference to an equal list: unread, so counted past.
        let referred = Object::from(dictionary! { "Properties" => by(same.into()) });
        assert_ne!(names(&pdf, referred).0, header);
        // The resources and the category by reference are followed.
        let category = pdf.add_object(by(list("Other").into()));
        let resources = pdf.add_object(dictionary! { "Properties" => category });
        assert_ne!(names(&pdf, Object::from(resources)).0, header);
        // A form's name bound to the object it's written at: its own.
        let xobjects = |at: u32| {
            Object::from(dictionary! {
                "XObject" => dictionary! { form.clone() => Object::from(ObjectId::new(at, 0)) },
            })
        };
        assert_eq!(names(&pdf, xobjects(40)).1, form);
        assert_ne!(names(&pdf, xobjects(41)).1, form);
        // No resources at all, or none that's a dictionary.
        assert_eq!(names(&pdf, Object::null()), (header.clone(), form.clone()));
        let none = reserved_shallow(&mut Reads(&pdf), None);
        assert_eq!(w.named(&none).1, plain);
    }
}
