//! What an annotation shows as a flatten reads it (M10.5 §12.2): its
//! dictionary read by pdf-annots ([`annots::read`]), its flags and the
//! state it names as the family reads them, the form of its normal
//! appearance that a flatten draws ([`Showing::form`]), and that form
//! placed on a box as ISO 32000-2 §12.5.5 places it, by the family's
//! placement ([`annots::placement`]). Which state another tool's comment
//! or a widget shows is sheaf's, over the family's [`AppearanceEntry`].
//!
//! Nothing read here is kept by the source: each object is read for the
//! asker and dropped with what one annotation read ([`showing`]), so a
//! flatten of thousands of widgets holds none of their dictionaries,
//! appearances or forms once it's done.

use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use crate::annot::Box2;
use crate::content::{Objects, ReadObjects, Unkept};
use crate::geom::{Affine, Rect};
use crate::pdf::annots::{self, AppearanceEntry};
use crate::pdf::{LazyPdf, Object, ObjectId, Resolve, Value, gfx};
use crate::store::Src;

use super::{Limits, ObjRef};

/// Where an annotation's dictionary is in its source: its object, or
/// written as a dictionary in slot `slot` of page `index`'s `/Annots`.
#[derive(Debug, Clone, Copy)]
pub(crate) enum AnnotAt<'a> {
    Object(&'a ObjRef),
    InSlot {
        src: &'a Src,
        index: u32,
        slot: usize,
    },
}

impl<'a> AnnotAt<'a> {
    /// The source it's in.
    fn src(self) -> &'a Src {
        match self {
            AnnotAt::Object(at) => &at.src,
            AnnotAt::InSlot { src, .. } => src,
        }
    }
}

/// A source's objects as a flatten reads an annotation's: each read for
/// the asker by [`LazyPdf::object_unkept`] (a stream with its data) and
/// kept nowhere in the reader (one the reader keeps already is copied
/// out), a chain of references followed to the reader's depth, and what
/// isn't there absent. Where a
/// chain it followed ended is noted, so that a form is named by the
/// stream's own object ([`Fresh::end`]), as the file has it.
struct Fresh<'a> {
    pdf: &'a LazyPdf,
    ends: RefCell<HashMap<ObjectId, ObjectId>>,
}

impl<'a> Fresh<'a> {
    fn new(pdf: &'a LazyPdf) -> Fresh<'a> {
        Fresh {
            pdf,
            ends: RefCell::default(),
        }
    }

    /// The object the chain of references from object `id` ended at, as
    /// [`peek`](ReadObjects::peek) followed it: `id` itself where it
    /// isn't a reference, or hasn't been read.
    fn end(&self, id: ObjectId) -> ObjectId {
        self.ends.borrow().get(&id).copied().unwrap_or(id)
    }
}

impl ReadObjects for Fresh<'_> {
    /// As the source reads and keeps it. The family's reading asks only
    /// [`peek`](ReadObjects::peek), through [`Objects`].
    fn object(&self, id: ObjectId) -> crate::pdf::Result<&Object> {
        self.pdf.get_object(id)
    }

    fn read<R>(&self, id: ObjectId, read: impl FnOnce(&Object) -> R) -> crate::pdf::Result<R> {
        ReadObjects::read(self.pdf, id, read)
    }

    /// Object `id`, its references followed, always owned: read by
    /// [`LazyPdf::object_unkept`], so nothing of it stays in the reader.
    fn peek(&self, id: ObjectId) -> crate::pdf::Result<Option<Cow<'_, Object>>> {
        use crate::pdf::Error;
        let mut at = id;
        for _ in 0..=Limits::DEFAULT.reader.max_depth {
            let object = match self.pdf.object_unkept(at) {
                Ok(object) => object,
                Err(Error::MissingXrefEntry | Error::ObjectNotFound(_)) => return Ok(None),
                Err(error) => return Err(error),
            };
            match object.as_reference() {
                Some(next) => at = next,
                None => {
                    if at != id {
                        self.ends.borrow_mut().insert(id, at);
                    }
                    return Ok(Some(Cow::Owned(Arc::unwrap_or_clone(object))));
                }
            }
        }
        Err(Error::ReferenceLimit)
    }
}

/// What a flatten reads of an annotation ([`showing`]): the family's
/// reading of its dictionary, with the objects it was read from.
pub(crate) struct Showing<'o> {
    src: &'o Src,
    fresh: &'o Fresh<'o>,
    unkept: &'o Unkept,
    annot: annots::Annot<'o>,
}

/// Annotation `at` as a flatten reads it, handed to `see` while what it
/// read is held, then dropped: None where `at` holds no dictionary, and
/// the family's error where it refuses the one there.
pub(crate) fn showing<T>(
    at: AnnotAt<'_>,
    see: impl FnOnce(&Showing<'_>) -> T,
) -> Option<Result<T, annots::Error>> {
    let src = at.src();
    let fresh = Fresh::new(src.pdf());
    let unkept = Unkept::default();
    let (dict, id) = match at {
        AnnotAt::Object(r) => match Objects::new(&fresh, &unkept).resolve(r.id)?.value() {
            Value::Dict(dict) => (dict, Some(fresh.end(r.id))),
            _ => return None,
        },
        AnnotAt::InSlot { src, index, slot } => match src.page(index)?.annots.get(slot)?.value() {
            Value::Dict(dict) => (dict, None),
            _ => return None,
        },
    };
    let annot = match annots::read(&fresh, &unkept, dict, id) {
        Ok(annot) => annot,
        Err(why) => return Some(Err(why)),
    };
    let showing = Showing {
        src,
        fresh: &fresh,
        unkept: &unkept,
        annot,
    };
    Some(Ok(see(&showing)))
}

impl<'o> Showing<'o> {
    /// Its flags (`/F`), as the family reads them.
    pub(crate) fn flags(&self) -> u32 {
        self.annot.flags().bits()
    }

    /// The state it names (`/AS`), as the family reads it: a name, or a
    /// string, as PDFium reads either; none where it's empty.
    pub(crate) fn state(&self) -> Option<&[u8]> {
        let state = self.annot.appearances()?.state.as_deref()?;
        (!state.is_empty()).then_some(state)
    }

    /// The form its normal appearance shows in state `state` (none where
    /// it's empty): the one stream it names, or of a dictionary of states,
    /// the last of the name `state` names. With none named and `by_value`
    /// (another tool's comment, never a widget, M10 bug 10), the one
    /// PDFium shows, the page view's engine: the state its value names,
    /// else `/Off`; never the first. A widget with states and none named
    /// shows none. None where there's none, which PDFium shows as nothing
    /// (it makes no appearance of its own where `/AP /N` is; pdf.js does,
    /// for some kinds). The form is named by the stream's own object.
    pub(crate) fn form(&self, state: Option<&[u8]>, by_value: bool) -> Option<ObjRef> {
        let state = state.filter(|s| !s.is_empty());
        let id = match self.annot.appearances()?.normal.as_ref()? {
            AppearanceEntry::Single(id) => *id,
            entry @ AppearanceEntry::States(states) => {
                let shown;
                let state = match state {
                    Some(state) => state,
                    None if by_value => {
                        shown = match self.value() {
                            Some(value) if states.iter().any(|(name, _)| **name == *value) => value,
                            _ => b"Off".to_vec(),
                        };
                        shown.as_slice()
                    }
                    None => return None,
                };
                entry.resolve(Some(state), true)?
            }
        };
        Some(ObjRef {
            src: self.src.clone(),
            id: self.fresh.end(id),
        })
    }

    /// The state its value names (`/V`, or its parent's): a name, or a
    /// string, as PDFium reads either; none where it's empty.
    fn value(&self) -> Option<Vec<u8>> {
        let objects = Objects::new(self.fresh, self.unkept);
        let reached = |o: &'o Object| match o.as_reference() {
            Some(id) => objects.resolve(id),
            None => Some(o),
        };
        let named = |o: &'o Object| -> Option<Vec<u8>> {
            let bytes = match reached(o)?.value() {
                Value::Name(n) => n.as_bytes(),
                Value::String(s, _) => s.as_bytes()?,
                _ => return None,
            };
            (!bytes.is_empty()).then(|| bytes.to_vec())
        };
        let dict = self.annot.dict();
        dict.get(b"V").and_then(named).or_else(|| {
            let Value::Dict(parent) = reached(dict.get(b"Parent")?)?.value() else {
                return None;
            };
            parent.get(b"V").and_then(named)
        })
    }

    /// Form `form`, one [`form`](Showing::form) gave, placed on `rect`
    /// ([`annots::placement`]): its `/BBox`, its `/Matrix`, and the matrix
    /// that takes the box the second makes of the first onto `rect`. None
    /// where it isn't a form with a box, or that box has no area, which
    /// draws nothing.
    pub(crate) fn placed(&self, form: &ObjRef, rect: Box2) -> Option<(Rect, Affine, Affine)> {
        let on = gfx::Rect {
            x0: rect.x,
            y0: rect.y,
            x1: rect.x + rect.w,
            y1: rect.y + rect.h,
        };
        let (bbox, matrix, onto) = annots::placement(self.fresh, self.unkept, form.id, on)?;
        Some((Rect::from(bbox), Affine::from(matrix), Affine::from(onto)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pdf::{Document as Pdf, Stream, dictionary};

    /// A file with a catalog and no page yet (`lazily` gives it one).
    fn file() -> Pdf {
        let mut pdf = Pdf::with_version("1.7");
        let pages = pdf.add_object(dictionary! {
            "Type" => Object::name("Pages"), "Kids" => Vec::<Object>::new(), "Count" => 0,
        });
        let catalog =
            pdf.add_object(dictionary! { "Type" => Object::name("Catalog"), "Pages" => pages });
        pdf.trailer.set("Root", catalog);
        pdf
    }

    /// A form of box `[0 0 20 10]`, drawing `content`.
    fn form(pdf: &mut Pdf, content: &[u8]) -> ObjectId {
        pdf.add_object(Stream::new(
            dictionary! {
                "Type" => Object::name("XObject"), "Subtype" => Object::name("Form"),
                "BBox" => vec![0.into(), 0.into(), 20.into(), 10.into()],
            },
            content.to_vec(),
        ))
    }

    /// `/Rect [10 10 30 20]`, `/F 4` and `extra`, an annotation of subtype
    /// `subtype`.
    fn annot(subtype: &str, extra: Vec<(&str, Object)>) -> crate::pdf::Dictionary {
        let mut dict = dictionary! {
            "Type" => Object::name("Annot"), "Subtype" => Object::name(subtype),
            "Rect" => vec![10.into(), 10.into(), 30.into(), 20.into()], "F" => 4,
        };
        for (key, value) in extra {
            dict.set(key, value);
        }
        dict
    }

    /// The move by 10, 10: `[0 0 20 10]` onto the rect.
    const MOVED: Affine = Affine {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        e: 10.0,
        f: 10.0,
    };

    /// The rect `[10 10 30 20]` as a box.
    const RECT: Box2 = Box2 {
        x: 10.0,
        y: 10.0,
        w: 20.0,
        h: 10.0,
    };

    /// A widget's states by reference, its value its field's: what it
    /// shows is read, the form its field's value names placed on its
    /// rect, and nothing it read (the widget, its appearance dictionary,
    /// its field, its forms) is kept by the source once it's read.
    #[test]
    fn what_a_flatten_reads_is_kept_nowhere() {
        let mut pdf = file();
        let on = form(&mut pdf, b"0 0 20 10 re f");
        let off = form(&mut pdf, b"");
        let ap = pdf.add_object(dictionary! { "N" => dictionary! { "On" => on, "Off" => off } });
        let field =
            pdf.add_object(dictionary! { "FT" => Object::name("Btn"), "V" => Object::name("On") });
        let extra = vec![("AP", ap.into()), ("Parent", field.into())];
        let widget = pdf.add_object(annot("Widget", extra));
        let src = crate::open::tests::lazily(&mut pdf);
        let read = [widget, ap, field, on, off];
        assert!(read.iter().all(|&id| !src.pdf().is_kept(id)), "kept before");

        let at = ObjRef {
            src: src.clone(),
            id: widget,
        };
        let (flags, state, by_value, by_state, onto) = showing(AnnotAt::Object(&at), |s| {
            let by_value = s.form(s.state(), true);
            let onto = by_value.as_ref().and_then(|f| s.placed(f, RECT));
            (
                s.flags(),
                s.state().map(<[u8]>::to_vec),
                by_value,
                s.form(None, false),
                onto,
            )
        })
        .expect("a dictionary")
        .expect("read");
        assert_eq!((flags, state), (4, None));
        // By its value, its field's: `On`. By its state alone, none (a
        // widget with states and none named).
        assert_eq!(by_value.map(|f| f.id), Some(on));
        assert_eq!(by_state, None);
        let (bbox, matrix, onto) = onto.expect("placed");
        assert_eq!(bbox, Rect::new(0.0, 0.0, 20.0, 10.0));
        assert_eq!(matrix, Affine::IDENTITY);
        assert_eq!(onto, MOVED);
        let kept: Vec<ObjectId> = (read.into_iter())
            .filter(|&id| src.pdf().is_kept(id))
            .collect();
        assert_eq!(kept, [], "kept by the source after a flatten's read");
    }

    /// A normal appearance named through a reference to a reference is
    /// the stream's own object, and is placed; an empty `/AS` names no
    /// state; what isn't a dictionary is none.
    #[test]
    fn a_form_through_a_chain_of_references_is_named_by_its_stream() {
        let mut pdf = file();
        let shown = form(&mut pdf, b"0 0 20 10 re f");
        let between = pdf.add_object(Object::from(shown));
        let extra = vec![
            ("AS", Object::name("")),
            ("AP", dictionary! { "N" => between }.into()),
        ];
        let dict = annot("Square", extra);
        let square = pdf.add_object(dict);
        let number = pdf.add_object(Object::from(7));
        let src = crate::open::tests::lazily(&mut pdf);
        let at = ObjRef {
            src: src.clone(),
            id: square,
        };
        let (state, form, placed) = showing(AnnotAt::Object(&at), |s| {
            let form = s.form(s.state(), true);
            let placed = form.as_ref().and_then(|f| s.placed(f, RECT));
            (s.state().map(<[u8]>::to_vec), form, placed)
        })
        .expect("a dictionary")
        .expect("read");
        assert_eq!(state, None);
        assert_eq!(form.map(|f| f.id), Some(shown));
        assert_eq!(placed.map(|p| p.2), Some(MOVED));

        let at = ObjRef { src, id: number };
        assert!(showing(AnnotAt::Object(&at), |_| ()).is_none());
    }
}
