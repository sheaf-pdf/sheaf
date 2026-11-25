//! Where content's resources are written (M10.5 slice 9): a [`Sink`] reads
//! what a dictionary names and writes the resources a [`ContentWriter`]'s
//! content draws with, once however many draw with them, so a mark's
//! appearance form ([`form`], [`bind`]) serves both a PDF built in memory
//! ([`serialize::built`](crate::serialize::built)) and the emitter, whose
//! dictionaries refer to objects by key; the emitter reads a page's
//! `/Contents` and `/Resources` through it too.
//!
//! A dictionary and what a sink reads for it are in one space: a built
//! PDF's object numbers, or the emitter's placeholders.
//!
//! What content draws in the fallback font ([`Fallback`]) is joined where
//! its resources are bound ([`bind`], [`form`] and [`form_dict`] take its
//! [`Glyphs`] beside its needs and call [`Sink::fallback`] themselves), so
//! nothing binds the fallback's marker without joining what it draws: the
//! sink writes the subset once it has them all.
//!
//! [`ContentWriter`]: super::ContentWriter

use crate::pdf::content::Limits;
use crate::pdf::gfx::{Matrix, Rect};
use crate::pdf::{Dictionary, Object, ObjectId, Stream, StreamExt, Value, dictionary, resolve_fn};
use crate::source;

use super::{Fallback, Glyphs, Res, ResKind, Reserved, ResourceNeeds};

/// What content's resources are read through and written to.
pub trait Sink {
    /// The dictionary object `id` is, references followed: None for
    /// anything else, or what can't be read.
    fn dict(&mut self, id: ObjectId) -> Option<Dictionary>;

    /// The array object `id` is.
    fn array(&mut self, id: ObjectId) -> Option<Vec<Object>>;

    /// `res`'s object, written once however many pages draw with it. The
    /// fallback's marker ([`Fallback::res`]) is given the number its subset
    /// is written at, once every glyph is joined ([`Sink::fallback`]).
    fn resource(&mut self, res: &Res) -> ObjectId;

    /// Joins `glyphs`, drawn in the fallback font by content whose
    /// resources are being bound, to the subset of it the sink writes: the
    /// font it gave the marker's number ([`Sink::resource`]). [`bind`]
    /// calls it with what it binds, so nothing binds the marker without
    /// joining what it draws.
    fn fallback(&mut self, glyphs: &Glyphs);

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
/// other pages share is never changed. `glyphs`, what the content draws
/// in the fallback font, are joined ([`Sink::fallback`]): content names
/// the fallback if and only if it draws a glyph in it.
pub fn bind(
    sink: &mut dyn Sink,
    needs: &ResourceNeeds,
    glyphs: &Glyphs,
    resources: &mut Dictionary,
) {
    debug_assert_eq!(
        (needs.0.iter()).any(|need| Fallback::SANS.is(&need.res)),
        !glyphs.is_empty(),
        "content names the fallback font if and only if it draws a glyph in it"
    );
    if !glyphs.is_empty() {
        sink.fallback(glyphs);
    }
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

/// An appearance's form XObject, made from its parts: its box (`/BBox`),
/// its matrix (`/Matrix`, where it isn't the identity), its content,
/// compressed, and what that draws with, `needs`, written through `sink`
/// once a file each and bound in its `/Resources` ([`bind`]), and
/// `glyphs`, what it draws in the fallback font, joined. Whoever drew the
/// appearance (`pdf::annots::SynthesizedAp`'s parts, a mark's, drawn by
/// sheaf or by the family), its form is made here.
pub fn form(
    sink: &mut dyn Sink,
    bbox: Rect,
    matrix: Matrix,
    content: Vec<u8>,
    needs: &ResourceNeeds,
    glyphs: &Glyphs,
) -> Stream {
    let mut form = Stream::new(form_dict(sink, bbox, matrix, needs, glyphs), content);
    form.compress();
    form
}

/// [`form`]'s dictionary alone, before its content is compressed and
/// held: what a form whose content is written later (the emitter's
/// deferred one) holds it with.
pub fn form_dict(
    sink: &mut dyn Sink,
    bbox: Rect,
    matrix: Matrix,
    needs: &ResourceNeeds,
    glyphs: &Glyphs,
) -> Dictionary {
    let mut resources = Dictionary::new();
    bind(sink, needs, glyphs, &mut resources);
    let numbers = |n: &[f32]| Object::from(n.iter().copied().map(Object::from).collect::<Vec<_>>());
    let mut dict = dictionary! {
        "Type" => Object::name("XObject"),
        "Subtype" => Object::name("Form"),
        "BBox" => numbers(&[bbox.x0, bbox.y0, bbox.x1, bbox.y1]),
    };
    if matrix != Matrix::IDENTITY {
        dict.set("Matrix", numbers(&matrix.to_array()));
    }
    dict.set("Resources", resources);
    dict
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
    use crate::pdf::Document as Pdf;
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
        let none = Glyphs::new();
        res.bind(&mut pdf, &needs, &none, &mut first);
        let count = pdf.objects.len();
        res.bind(&mut pdf, &needs, &none, &mut second);
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
        res.finish(&mut pdf);
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

        fn fallback(&mut self, _: &Glyphs) {
            unreachable!("nothing is written")
        }

        fn reserved(&mut self, resources: Option<&Object>) -> Reserved {
            reserved_shallow(self, resources)
        }
    }

    /// A sink over nothing that gives a resource already written its own
    /// number, and any other the next of its own.
    struct Numbers(u32);

    impl Sink for Numbers {
        fn dict(&mut self, _: ObjectId) -> Option<Dictionary> {
            None
        }

        fn array(&mut self, _: ObjectId) -> Option<Vec<Object>> {
            None
        }

        fn resource(&mut self, res: &Res) -> ObjectId {
            match res.body() {
                crate::content::ResBody::Written(id) => *id,
                _ => {
                    self.0 += 1;
                    ObjectId::new(self.0, 0)
                }
            }
        }

        /// Nothing is written, so there's no subset to join to.
        fn fallback(&mut self, _: &Glyphs) {}

        fn reserved(&mut self, resources: Option<&Object>) -> Reserved {
            reserved_shallow(self, resources)
        }
    }

    /// An appearance's form, from its parts: its box, its matrix after
    /// it only where that isn't the identity, then its needs bound by
    /// name; its content compressed; and its dictionary alone is the
    /// form's, but for what holding the content adds.
    #[test]
    fn a_form_is_made_from_an_appearances_parts() {
        let mut w = ContentWriter::new();
        w.draw(Res::written(ObjectId::new(40, 0)));
        let (content, needs) = w.finish(&Reserved::default());
        let bbox = Rect {
            x0: 10.0,
            y0: 20.0,
            x1: 110.5,
            y1: 70.25,
        };
        let turned = Matrix::new(0.0, 1.0, -1.0, 0.0, 70.25, 0.0);
        let numbers =
            |n: &[f32]| Object::from(n.iter().map(|&v| Object::from(v)).collect::<Vec<_>>());
        for matrix in [Matrix::IDENTITY, turned] {
            let none = Glyphs::new();
            let made = form(
                &mut Numbers(0),
                bbox,
                matrix,
                content.clone(),
                &needs,
                &none,
            );
            let keys: Vec<&[u8]> = made.dict.iter().map(|(k, _)| k).collect();
            let mut expected: Vec<&[u8]> = vec![b"Type", b"Subtype", b"BBox"];
            if matrix != Matrix::IDENTITY {
                expected.push(b"Matrix");
            }
            expected.push(b"Resources");
            assert_eq!(keys[..expected.len()], expected[..], "{matrix:?}");
            assert_eq!(
                made.dict.get(b"BBox"),
                Some(&numbers(&[10.0, 20.0, 110.5, 70.25]))
            );
            let written = made.dict.get(b"Matrix");
            assert_eq!(
                written,
                (matrix != Matrix::IDENTITY)
                    .then(|| numbers(&matrix.to_array()))
                    .as_ref()
            );
            let xobjects = made.dict.get(b"Resources").and_then(Object::as_dict);
            let xobjects = xobjects
                .and_then(|r| r.get(b"XObject"))
                .and_then(Object::as_dict);
            assert_eq!(
                xobjects.and_then(|x| x.get(&needs.0[0].name)),
                Some(&Object::from(ObjectId::new(40, 0)))
            );
            assert_eq!(made.decompressed_content().unwrap(), content);
            let alone = form_dict(&mut Numbers(0), bbox, matrix, &needs, &none);
            for (key, value) in alone.iter() {
                assert_eq!(made.dict.get(key), Some(value));
            }
            assert_eq!(alone.len(), expected.len());
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
