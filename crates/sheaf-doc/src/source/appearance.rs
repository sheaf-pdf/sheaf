//! Annotations' appearances as a source holds them, as a flatten reads
//! them (M10.5 §12.2): what an annotation's dictionary says it shows, the
//! form its normal appearance shows, and that form's frame. Each object is
//! read without keeping it, which a flatten of thousands of widgets reads
//! once each.

use std::sync::Arc;

use lopdf::{Dictionary, LazyPdf, Object, ObjectId};

use super::{Limits, ObjRef, Source};
use crate::geom::{Affine, Rect};
use crate::store::Src;

/// What an annotation's dictionary says that a flatten reads: its flags
/// (`/F`), the state it shows (`/AS`) and its normal appearance (`/AP
/// /N`), as written.
pub(crate) struct Shows {
    pub(crate) flags: u32,
    pub(crate) state: Option<Vec<u8>>,
    normal: Option<Object>,
}

impl Shows {
    /// What annotation `at` says: None when it isn't a dictionary.
    pub(crate) fn read(at: &ObjRef) -> Option<Shows> {
        let pdf = at.src.pdf();
        let (_, obj) = unkept(pdf, at.id)?;
        Some(Shows::of(pdf, obj.as_dict().ok()?))
    }

    /// What the annotation in slot `slot` of page `index`'s `/Annots` says,
    /// written there as a dictionary: None when it isn't one.
    pub(crate) fn in_slot(src: &Source, index: u32, slot: usize) -> Option<Shows> {
        let Object::Dictionary(dict) = src.page(index)?.annots.get(slot)? else {
            return None;
        };
        Some(Shows::of(src.pdf(), dict))
    }

    /// What `dict`, an annotation of `pdf`, says.
    fn of(pdf: &LazyPdf, dict: &Dictionary) -> Shows {
        let value = |key: &[u8]| -> Option<Object> {
            match dict.get(key).ok()? {
                Object::Reference(r) => Some((*unkept(pdf, *r)?.1).clone()),
                direct => Some(direct.clone()),
            }
        };
        let flags = value(b"F").and_then(|f| f.as_i64().ok()).unwrap_or(0);
        // `/N` alone, the appearance dictionary read in place.
        let n = |ap: &Object| ap.as_dict().ok()?.get(b"N").ok().cloned();
        let normal = match dict.get(b"AP").ok() {
            Some(Object::Reference(r)) => unkept(pdf, *r).and_then(|(_, ap)| n(&ap)),
            Some(direct) => n(direct),
            None => None,
        };
        Shows {
            flags: u32::try_from(flags).unwrap_or(0),
            state: value(b"AS").and_then(|s| s.as_name().ok().map(<[u8]>::to_vec)),
            normal,
        }
    }

    /// The form its normal appearance, read from `src`, shows: the stream
    /// it names, or of a dictionary of states, the one `state` names; with
    /// none named, the first when `first` says so (another tool's comment,
    /// never a widget, M10 bug 10). None when there's none.
    pub(crate) fn form(&self, src: &Src, state: Option<&[u8]>, first: bool) -> Option<ObjRef> {
        let pdf = src.pdf();
        let at = |id| ObjRef {
            src: src.clone(),
            id,
        };
        // A dictionary of states, read in place.
        let held;
        let states = match self.normal.as_ref()? {
            Object::Reference(r) => {
                let (id, obj) = unkept(pdf, *r)?;
                if obj.as_stream().is_ok() {
                    return Some(at(id));
                }
                held = obj;
                held.as_dict().ok()?
            }
            Object::Dictionary(states) => states,
            _ => return None,
        };
        let state = match state {
            Some(state) => state,
            None if first => states.iter().next()?.0.as_slice(),
            None => return None,
        };
        let id = states.get(state).ok()?.as_reference().ok()?;
        let (id, obj) = unkept(pdf, id)?;
        obj.as_stream().is_ok().then(|| at(id))
    }
}

/// Form `form`'s `/BBox` and `/Matrix` (the identity when it has none):
/// None when it isn't a form with a box.
pub(crate) fn frame(form: &ObjRef) -> Option<(Rect, Affine)> {
    let pdf = form.src.pdf();
    let (_, obj) = unkept(pdf, form.id)?;
    let dict = &obj.as_stream().ok()?.dict;
    let numbers = |key: &[u8], n: usize| -> Option<Vec<f32>> {
        let array = match dict.get(key).ok()? {
            Object::Reference(r) => (*unkept(pdf, *r)?.1).clone(),
            direct => direct.clone(),
        };
        let values: Vec<f32> = (array.as_array().ok()?.iter())
            .map(|v| v.as_float().unwrap_or(0.0) as f32)
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
        match *obj {
            Object::Reference(next) => at = next,
            _ => return Some((at, obj)),
        }
    }
    None
}
