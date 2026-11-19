//! Annotations' appearances as a source holds them, as a flatten reads
//! them (M10.5 §12.2): what an annotation's dictionary says it shows, the
//! form its normal appearance shows, and that form's frame. Each object is
//! read without keeping it, which a flatten of thousands of widgets reads
//! once each.

use std::sync::Arc;

use crate::pdf::{Dictionary, LazyPdf, Object, ObjectId, Value};

use super::{Limits, ObjRef, Source};
use crate::geom::{Affine, Rect};
use crate::store::Src;

/// What an annotation's dictionary says that a flatten reads: its flags
/// (`/F`), the state it shows (`/AS`), its value (`/V`, or its parent's),
/// and its normal appearance (`/AP /N`), as written.
pub(crate) struct Shows {
    pub(crate) flags: u32,
    pub(crate) state: Option<Vec<u8>>,
    value: Option<Vec<u8>>,
    normal: Option<Object>,
}

impl Shows {
    /// What annotation `at` says: None when it isn't a dictionary.
    pub(crate) fn read(at: &ObjRef) -> Option<Shows> {
        let pdf = at.src.pdf();
        let (_, obj) = unkept(pdf, at.id)?;
        let Value::Dict(dict) = obj.value() else {
            return None;
        };
        Some(Shows::of(pdf, dict))
    }

    /// What the annotation in slot `slot` of page `index`'s `/Annots` says,
    /// written there as a dictionary: None when it isn't one.
    pub(crate) fn in_slot(src: &Source, index: u32, slot: usize) -> Option<Shows> {
        let Value::Dict(dict) = src.page(index)?.annots.get(slot)?.value() else {
            return None;
        };
        Some(Shows::of(src.pdf(), dict))
    }

    /// What `dict`, an annotation of `pdf`, says. A state or a value is a
    /// name, or a string, as PDFium reads either; an empty one is none.
    fn of(pdf: &LazyPdf, dict: &Dictionary) -> Shows {
        let read = |dict: &Dictionary, key: &[u8]| -> Option<Object> {
            let entry = dict.get(key)?;
            match entry.as_reference() {
                Some(r) => Some((*unkept(pdf, r)?.1).clone()),
                None => Some(entry.clone()),
            }
        };
        let value = |key: &[u8]| read(dict, key);
        let named = |o: Object| -> Option<Vec<u8>> {
            let bytes = match o.value() {
                Value::Name(n) => n.as_bytes(),
                Value::String(s, _) => s.as_bytes()?,
                _ => return None,
            };
            (!bytes.is_empty()).then(|| bytes.to_vec())
        };
        let flags = value(b"F").and_then(|f| f.as_integer()).unwrap_or(0);
        let parent = || {
            let parent = value(b"Parent")?;
            let Value::Dict(dict) = parent.value() else {
                return None;
            };
            read(dict, b"V").and_then(named)
        };
        // `/N` alone, the appearance dictionary read in place.
        let n = |ap: &Object| match ap.value() {
            Value::Dict(d) => d.get(b"N").cloned(),
            _ => None,
        };
        let normal = match dict.get(b"AP") {
            Some(ap) => match ap.as_reference() {
                Some(r) => unkept(pdf, r).and_then(|(_, ap)| n(&ap)),
                None => n(ap),
            },
            None => None,
        };
        Shows {
            flags: u32::try_from(flags).unwrap_or(0),
            state: value(b"AS").and_then(named),
            value: value(b"V").and_then(named).or_else(parent),
            normal,
        }
    }

    /// The form its normal appearance, read from `src`, shows: the stream
    /// it names, or of a dictionary of states, the one `state` names. With
    /// none named and `by_value` (another tool's comment, never a widget,
    /// M10 bug 10), the one PDFium shows, the page view's engine: the state
    /// its value names, else `/Off`; never the first. None when there's
    /// none, which PDFium shows as nothing (it makes no appearance of its
    /// own where `/AP /N` is; pdf.js does, for some kinds).
    pub(crate) fn form(&self, src: &Src, state: Option<&[u8]>, by_value: bool) -> Option<ObjRef> {
        let pdf = src.pdf();
        let at = |id| ObjRef {
            src: src.clone(),
            id,
        };
        // A dictionary of states, read in place.
        let held;
        let states = match self.normal.as_ref()?.value() {
            Value::Ref(r) => {
                let (id, obj) = unkept(pdf, *r)?;
                if obj.as_stream().is_some() {
                    return Some(at(id));
                }
                held = obj;
                match held.value() {
                    Value::Dict(d) => d,
                    _ => return None,
                }
            }
            Value::Dict(states) => states,
            _ => return None,
        };
        let state = match state {
            Some(state) => state,
            None if by_value => match self.value.as_deref() {
                Some(value) if states.has(value) => value,
                _ => b"Off",
            },
            None => return None,
        };
        let id = states.get(state)?.as_reference()?;
        let (id, obj) = unkept(pdf, id)?;
        obj.as_stream().is_some().then(|| at(id))
    }
}

/// Form `form`'s `/BBox` and `/Matrix` (the identity when it has none):
/// None when it isn't a form with a box.
pub(crate) fn frame(form: &ObjRef) -> Option<(Rect, Affine)> {
    let pdf = form.src.pdf();
    let (_, obj) = unkept(pdf, form.id)?;
    let dict = &obj.as_stream()?.dict;
    let numbers = |key: &[u8], n: usize| -> Option<Vec<f32>> {
        let entry = dict.get(key)?;
        let array = match entry.as_reference() {
            Some(r) => (*unkept(pdf, r)?.1).clone(),
            None => entry.clone(),
        };
        let values: Vec<f32> = (array.as_array()?.iter())
            .map(|v| v.as_number().unwrap_or(0.0) as f32)
            .collect();
        (values.len() == n).then_some(values)
    };
    let bbox = numbers(b"BBox", 4)?;
    let matrix = match numbers(b"Matrix", 6).as_deref() {
        Some(&[a, b, c, d, e, f]) => Affine { a, b, c, d, e, f },
        _ => Affine::IDENTITY,
    };
    Some((Rect::new(bbox[0], bbox[1], bbox[2], bbox[3]), matrix))
}

/// Object `id` of `pdf`, its references followed, read without keeping
/// it, and the object it is.
fn unkept(pdf: &LazyPdf, id: ObjectId) -> Option<(ObjectId, Arc<Object>)> {
    let mut at = id;
    for _ in 0..Limits::DEFAULT.reader.max_depth {
        let obj = pdf.object_unkept(at).ok()?;
        match obj.as_reference() {
            Some(next) => at = next,
            None => return Some((at, obj)),
        }
    }
    None
}
