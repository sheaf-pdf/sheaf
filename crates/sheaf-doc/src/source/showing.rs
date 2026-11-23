//! What an annotation shows as a flatten reads it (M10.5 §12.2), by
//! pdf-annots: the state it names and the form of its normal appearance
//! that a flatten draws ([`Showing::form`]), from its appearances read
//! alone ([`annots::appearances`]: `/AP` and `/AS`, which no cap on the
//! rest of the annotation can refuse), and that form placed on a box as
//! ISO 32000-2 §12.5.5 places it, by the family's placement
//! ([`annots::placement`]). Another tool's comment is read whole
//! ([`annots::read`]) for its flags too ([`showing`]); a widget's flags
//! and state are its record's, so only its appearances are read
//! ([`appearing`]). Which state another tool's comment or a widget shows
//! is sheaf's, over the family's [`AppearanceEntry`].
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
use crate::geom::{Affine, Rect, family_box};
use crate::pdf::annots::{AppearanceEntry, Appearances, Error};
use crate::pdf::{Dictionary, LazyPdf, Object, ObjectId, Resolve, Value};
use crate::store::Src;

use super::{Limits, ObjRef, annots};

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

/// What a flatten reads of an annotation's appearances ([`showing`],
/// [`appearing`]): the family's reading of them, with the dictionary and
/// the objects they were read from.
pub(crate) struct Showing<'o> {
    src: &'o Src,
    fresh: &'o Fresh<'o>,
    unkept: &'o Unkept,
    dict: &'o Dictionary,
    appearances: Option<Appearances>,
}

/// The dictionary of annotation `at` and its object's id (none in a
/// slot), read by `fresh` and held by `unkept`: None where `at` holds no
/// dictionary.
fn dictionary<'o>(
    at: AnnotAt<'o>,
    fresh: &'o Fresh<'o>,
    unkept: &'o Unkept,
) -> Option<(&'o Dictionary, Option<ObjectId>)> {
    match at {
        AnnotAt::Object(r) => match Objects::new(fresh, unkept).resolve(r.id)?.value() {
            Value::Dict(dict) => Some((dict, Some(fresh.end(r.id)))),
            _ => None,
        },
        AnnotAt::InSlot { src, index, slot } => match src.page(index)?.annots.get(slot)?.value() {
            Value::Dict(dict) => Some((dict, None)),
            _ => None,
        },
    }
}

/// Annotation `at` as a flatten reads another tool's comment: read whole
/// by the family, its flags (`/F`) as the family reads them and what it
/// shows handed to `see` while what it read is held, then dropped. None
/// where `at` holds no dictionary, and the family's error where it
/// refuses the one there (a cap on a part the flatten doesn't draw, too:
/// the open lists a comment by the same reading, so none it listed is
/// refused here).
pub(crate) fn showing<T>(
    at: AnnotAt<'_>,
    see: impl FnOnce(u32, &Showing<'_>) -> T,
) -> Option<Result<T, Error>> {
    let src = at.src();
    let fresh = Fresh::new(src.pdf());
    let unkept = Unkept::default();
    let (dict, id) = dictionary(at, &fresh, &unkept)?;
    let annot = match annots::read(&fresh, &unkept, dict, id) {
        Ok(annot) => annot,
        Err(why) => return Some(Err(why)),
    };
    let showing = Showing {
        src,
        fresh: &fresh,
        unkept: &unkept,
        dict,
        appearances: annot.appearances().cloned(),
    };
    Some(Ok(see(annot.flags().bits(), &showing)))
}

/// Annotation `at` as a flatten reads a widget: its appearances alone,
/// what it shows handed to `see` while what it read is held, then
/// dropped. Nothing else of it is read (its flags and its state are its
/// field's record's), so a widget whose actions or `/DA` are past one of
/// the family's caps is still drawn. None where `at` holds no dictionary;
/// an error only where an object can't be read, which sheaf's sources
/// give as absent.
pub(crate) fn appearing<T>(
    at: AnnotAt<'_>,
    see: impl FnOnce(&Showing<'_>) -> T,
) -> Option<Result<T, Error>> {
    let src = at.src();
    let fresh = Fresh::new(src.pdf());
    let unkept = Unkept::default();
    let (dict, id) = dictionary(at, &fresh, &unkept)?;
    let appearances = match annots::appearances(&fresh, &unkept, dict, id) {
        Ok(appearances) => appearances,
        Err(why) => return Some(Err(why)),
    };
    let showing = Showing {
        src,
        fresh: &fresh,
        unkept: &unkept,
        dict,
        appearances,
    };
    Some(Ok(see(&showing)))
}

impl<'o> Showing<'o> {
    /// The state it names (`/AS`), as the family reads it: a name, or a
    /// string, as PDFium reads either; none where it's empty.
    pub(crate) fn state(&self) -> Option<&[u8]> {
        let state = self.appearances.as_ref()?.state.as_deref()?;
        (!state.is_empty()).then_some(state)
    }

    /// The form its normal appearance shows in state `state` (none where
    /// it's empty): the one stream it names, or of a dictionary of states,
    /// the last of the name `state` names. With none named and `by_value`
    /// (another tool's comment, never a widget, M10 bug 10), the one
    /// PDFium shows, the page view's engine: the state its value names
    /// where the states have an entry of that name, whatever the entry is
    /// (PDFium asks whether the key is there, then for its stream: one
    /// that's null, a dictionary or a reference to nothing shows
    /// nothing), else `/Off`; never the first. A widget with states and
    /// none named
    /// shows none. None where there's none, which PDFium shows as nothing
    /// (it makes no appearance of its own where `/AP /N` is; pdf.js does,
    /// for some kinds). The form is named by the stream's own object.
    pub(crate) fn form(&self, state: Option<&[u8]>, by_value: bool) -> Option<ObjRef> {
        let state = state.filter(|s| !s.is_empty());
        let id = match self.appearances.as_ref()?.normal.as_ref()? {
            AppearanceEntry::Single(id) => *id,
            entry @ AppearanceEntry::States(_) => {
                let shown;
                let state = match state {
                    Some(state) => state,
                    None if by_value => {
                        shown = match self.value() {
                            Some(value) if self.has_state(&value) => value,
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

    /// `o`, or the object it refers to: none where that isn't there.
    fn reached(&self, o: &'o Object) -> Option<&'o Object> {
        match o.as_reference() {
            Some(id) => Objects::new(self.fresh, self.unkept).resolve(id),
            None => Some(o),
        }
    }

    /// Whether its normal appearance's states (`/AP /N`, a dictionary)
    /// have an entry named `name`, whatever it is: the family's
    /// [`AppearanceEntry`] keeps only the entries that are streams, and
    /// PDFium's rule for a state none names asks for the key alone.
    fn has_state(&self, name: &[u8]) -> bool {
        let states = || -> Option<&'o Dictionary> {
            let Value::Dict(ap) = self.reached(self.dict.get(b"AP")?)?.value() else {
                return None;
            };
            match self.reached(ap.get(b"N")?)?.value() {
                Value::Dict(states) => Some(states),
                _ => None,
            }
        };
        states().is_some_and(|states| states.has(name))
    }

    /// The state its value names (`/V`, or its parent's): a name, or a
    /// string, as PDFium reads either; none where it's empty.
    fn value(&self) -> Option<Vec<u8>> {
        let reached = |o: &'o Object| self.reached(o);
        let named = |o: &'o Object| -> Option<Vec<u8>> {
            let bytes = match reached(o)?.value() {
                Value::Name(n) => n.as_bytes(),
                Value::String(s, _) => s.as_bytes()?,
                _ => return None,
            };
            (!bytes.is_empty()).then(|| bytes.to_vec())
        };
        let dict = self.dict;
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
        let on = family_box(rect);
        let (bbox, matrix, onto) = annots::placement(self.fresh, self.unkept, form.id, on)?;
        Some((Rect::from(bbox), Affine::from(matrix), Affine::from(onto)))
    }
}

#[cfg(test)]
pub(crate) mod tests {
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
        let (flags, state, by_value, by_state, onto) = showing(AnnotAt::Object(&at), |flags, s| {
            let by_value = s.form(s.state(), true);
            let onto = by_value.as_ref().and_then(|f| s.placed(f, RECT));
            (
                flags,
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
        let (state, form, placed) = showing(AnnotAt::Object(&at), |_, s| {
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
        assert!(showing(AnnotAt::Object(&at), |_, _| ()).is_none());
        assert!(appearing(AnnotAt::Object(&at), |_| ()).is_none());
    }

    /// A chain of actions one past pdf-semantics' cap, each the next's
    /// `/Next`: its first, which an annotation's `/A` names.
    pub(crate) fn long_chain(pdf: &mut Pdf) -> ObjectId {
        let cap = annots::limits().semantics.action_chain;
        let mut next = None;
        for _ in 0..=cap {
            let mut action =
                dictionary! { "S" => Object::name("Named"), "N" => Object::name("NextPage") };
            if let Some(next) = next {
                action.set("Next", next);
            }
            next = Some(pdf.add_object(action));
        }
        next.expect("at least one")
    }

    /// An annotation whose action chain is past the family's cap is
    /// refused read whole (another tool's comment: the open, reading it
    /// the same way, lists none such), and its appearances alone still
    /// read: a widget's, which a flatten draws by them, is placed as one
    /// with no action is.
    #[test]
    fn a_cap_on_its_actions_refuses_it_whole_and_not_its_appearances() {
        let mut pdf = file();
        let on = form(&mut pdf, b"0 0 20 10 re f");
        let off = form(&mut pdf, b"");
        let chain = long_chain(&mut pdf);
        let states: Object = dictionary! { "On" => on, "Off" => off }.into();
        let extra = |action: Option<ObjectId>| {
            let mut extra = vec![
                ("AS", Object::name("On")),
                ("AP", dictionary! { "N" => states.clone() }.into()),
            ];
            extra.extend(action.map(|a| ("A", a.into())));
            extra
        };
        let plain = pdf.add_object(annot("Widget", extra(None)));
        let chained = pdf.add_object(annot("Widget", extra(Some(chain))));
        let src = crate::open::tests::lazily(&mut pdf);
        let at = |id| ObjRef {
            src: src.clone(),
            id,
        };

        let whole = showing(AnnotAt::Object(&at(chained)), |_, _| ()).expect("a dictionary");
        assert_eq!(
            whole.expect_err("refused").to_string(),
            "exceeded cap on chained actions"
        );
        let drawn = |id| {
            appearing(AnnotAt::Object(&at(id)), |s| {
                let form = s.form(s.state(), false).expect("a form");
                (form.id, s.placed(&form, RECT).map(|p| p.2))
            })
            .expect("a dictionary")
            .expect("read")
        };
        assert_eq!(drawn(chained), (on, Some(MOVED)));
        assert_eq!(drawn(chained), drawn(plain));
        // And whole, the one with no action reads the same appearances.
        let whole = showing(AnnotAt::Object(&at(plain)), |flags, s| {
            (flags, s.form(s.state(), false).map(|f| f.id))
        });
        assert_eq!(whole.expect("a dictionary").expect("read"), (4, Some(on)));
    }

    /// Another tool's comment with states and none named shows the state
    /// its value names where the states have an entry of that name,
    /// whatever the entry is: one that isn't a stream (a dictionary, a
    /// reference to nothing) shows nothing, as PDFium shows it, never
    /// `/Off` in its place. A value no entry has shows `/Off`; a widget
    /// with none named shows none.
    #[test]
    fn a_value_naming_a_state_that_is_no_stream_shows_nothing() {
        let mut pdf = file();
        let off = form(&mut pdf, b"0 0 20 10 re f");
        let gone: ObjectId = (9_999, 0).into();
        let comment = |pdf: &mut Pdf, on: Object, value: &str| {
            let states = dictionary! { "On" => on, "Off" => off };
            let extra = vec![
                ("AP", dictionary! { "N" => states }.into()),
                ("V", Object::name(value)),
            ];
            pdf.add_object(annot("Stamp", extra))
        };
        let not_a_stream = comment(&mut pdf, crate::pdf::Dictionary::new().into(), "On");
        let dangling = comment(&mut pdf, Object::from(gone), "On");
        let unnamed = comment(&mut pdf, crate::pdf::Dictionary::new().into(), "Other");
        let src = crate::open::tests::lazily(&mut pdf);
        let shown = |id, by_value| {
            let at = ObjRef {
                src: src.clone(),
                id,
            };
            showing(AnnotAt::Object(&at), |_, s| s.form(s.state(), by_value))
                .expect("a dictionary")
                .expect("read")
                .map(|f| f.id)
        };
        assert_eq!(shown(not_a_stream, true), None);
        assert_eq!(shown(dangling, true), None);
        assert_eq!(shown(unnamed, true), Some(off));
        assert_eq!(shown(not_a_stream, false), None);
    }
}
