//! What a `Do` draws, as PDFium draws it
//! (`CPDF_StreamContentParser::Handle_ExecuteXObject`): a Form XObject,
//! each use interpreted on its own, as PDFium parses a form afresh where
//! it's drawn (`AddForm`), its content read once however often it's
//! drawn; an image; and nothing for an XObject of another subtype, a
//! PostScript one among them. Images, drawn by `Do` or inline, are read
//! for what's said of them without their data decoded.

use std::ops::Range;
use std::sync::Arc;

use lopdf::{Dictionary, Object, ObjectId};

use super::text::Quad;
use crate::content::page::{ReadObjects, direct};
use crate::geom::{Affine, Rect};

/// A use of a form: where each `Do` is, from the page's content down to
/// the one that draws it, each its operator's offset in the content it's
/// in. The same bytes give the same ids.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct UseId(Arc<[u32]>);

impl UseId {
    pub(super) fn new(path: &[u32]) -> UseId {
        UseId(path.into())
    }

    /// Each `Do`'s offset, the page's first.
    pub fn path(&self) -> &[u32] {
        &self.0
    }

    /// How deep in forms it is: 1 for a form the page draws.
    pub fn depth(&self) -> usize {
        self.0.len()
    }
}

/// A form drawn, where it's drawn.
#[derive(Debug, Clone, Copy)]
pub struct FormUse<'e> {
    pub form: ObjectId,
    pub id: &'e UseId,
    /// Form space onto PageSpace: its `/Matrix`, then the CTM it's drawn
    /// with.
    pub matrix: Affine,
    /// Its `/BBox`, in form space, which clips what it draws: empty where
    /// it isn't four numbers.
    pub bbox: Option<Rect>,
    /// Its own `/OC`: the optional content it's in, by reference.
    pub oc: Option<ObjectId>,
}

/// Why a form isn't drawn where it's used; the rest of the content is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    /// It's drawn within itself: the page draws it, and it, or a form it
    /// draws, draws it again. PDFium's own check
    /// (`CPDF_StreamContentParser::Parse`) compares the buffers it reads
    /// each use's content into, which it makes afresh for each use of a
    /// stream read from a file, filtered or not, so it draws such a form
    /// within itself until it's too deep: what the interpreter reports
    /// is less than PDFium draws, and a consumer that must know all that's
    /// drawn fails closed on it.
    Cycle,
    /// It's deeper in forms than
    /// [`ContentLimits::form_depth`](crate::source::ContentLimits), PDFium's
    /// `kMaxFormLevel`, past which PDFium doesn't draw it either.
    Depth,
}

/// An image drawn.
#[derive(Debug, Clone, PartialEq)]
pub struct ImageDrawn {
    pub of: ImageOf,
    /// The unit square of its space through the CTM, as PDFium places an
    /// image (`FPDFPageObj_GetRotatedBounds`): its bottom-left,
    /// bottom-right, top-right and top-left as it's upright.
    pub quad: Quad,
    /// Whether it's an image mask (`/ImageMask`), its samples painting
    /// the fill color.
    pub stencil: bool,
    /// Its `/SMask`, the soft mask it's drawn through.
    pub smask: Option<ObjectId>,
    /// Its `/Mask` where that's an image, a stencil mask; a color key mask
    /// is the image's own samples.
    pub mask: Option<ObjectId>,
    /// Its `/OC`: the optional content it's in, by reference.
    pub oc: Option<ObjectId>,
}

/// Where an image's samples are.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageOf {
    XObject(ObjectId),
    /// An inline image, `BI` to `EI`: where its data is in the content
    /// it's in, as its [`Tok::InlineImage`](crate::content::Tok) says.
    Inline {
        data: Range<usize>,
    },
}

impl ImageDrawn {
    /// An image drawn with `ctm`.
    pub(super) fn new(of: ImageOf, facts: &ImageFacts, ctm: Affine) -> ImageDrawn {
        ImageDrawn {
            of,
            quad: Quad::of(Rect::new(0.0, 0.0, 1.0, 1.0), ctm),
            stencil: facts.stencil,
            smask: facts.smask,
            mask: facts.mask,
            oc: facts.oc,
        }
    }
}

/// What's said of an image, wherever it's drawn.
#[derive(Debug, Clone, Default)]
pub(super) struct ImageFacts {
    pub stencil: bool,
    pub smask: Option<ObjectId>,
    pub mask: Option<ObjectId>,
    pub oc: Option<ObjectId>,
}

/// A Form XObject, as interpreting it needs it.
#[derive(Debug)]
pub(super) struct Form {
    /// Its content, decompressed.
    pub data: Vec<u8>,
    pub matrix: Affine,
    pub bbox: Option<Rect>,
    /// Its own `/Resources`, where it has some.
    pub resources: Option<Object>,
    pub oc: Option<ObjectId>,
}

/// What reading an XObject found.
pub(super) enum XObject {
    Form(Form),
    Image(ImageFacts),
    /// A stream of another subtype, which PDFium doesn't draw: a
    /// PostScript XObject, or one of no subtype it knows.
    Other,
    /// Nothing: no stream, no such object.
    None,
    /// A form whose content is past what's left of the page's.
    TooLarge,
}

impl XObject {
    /// XObject `id`, a form's content decompressed within `budget` bytes.
    pub(super) fn read(
        objects: &impl ReadObjects,
        id: ObjectId,
        budget: usize,
    ) -> Result<XObject, lopdf::Error> {
        let read = objects.read(id, |object| {
            let Object::Stream(stream) = object else {
                return Ok(XObject::None);
            };
            let dict = &stream.dict;
            let reference = |key: &[u8]| dict.get(key).ok()?.as_reference().ok();
            let subtype = dict.get(b"Subtype").ok().and_then(|s| direct(objects, s));
            match subtype.and_then(|s| s.as_name().ok()) {
                Some(b"Form") => {}
                Some(b"Image") => return Ok(XObject::Image(image(objects, dict))),
                _ => return Ok(XObject::Other),
            }
            let data = match stream.decompressed_content_with_limit(budget) {
                Ok(data) => data,
                Err(lopdf::Error::Decompress(lopdf::DecompressError::MemoryLimitExceeded {
                    ..
                })) => return Ok(XObject::TooLarge),
                Err(e) => return Err(e),
            };
            Ok(XObject::Form(Form {
                data,
                matrix: array(objects, dict, b"Matrix", 6).map_or(Affine::IDENTITY, |n| Affine {
                    a: n[0],
                    b: n[1],
                    c: n[2],
                    d: n[3],
                    e: n[4],
                    f: n[5],
                }),
                bbox: array(objects, dict, b"BBox", 4).map(|n| Rect::new(n[0], n[1], n[2], n[3])),
                resources: dict
                    .get(b"Resources")
                    .ok()
                    .filter(|r| direct(objects, r).is_some_and(|r| r.as_dict().is_ok()))
                    .cloned(),
                oc: reference(b"OC"),
            }))
        });
        match read {
            Ok(read) => read,
            // An object that isn't there is null, as in PDFium.
            Err(lopdf::Error::MissingXrefEntry | lopdf::Error::ObjectNotFound(_)) => {
                Ok(XObject::None)
            }
            Err(e) => Err(e),
        }
    }
}

/// What image XObject `dict` says of itself: a mask whose `/Mask` is no
/// stream is a color key.
fn image(objects: &impl ReadObjects, dict: &Dictionary) -> ImageFacts {
    let reference = |key: &[u8]| dict.get(key).ok()?.as_reference().ok();
    let stencil = dict.get(b"ImageMask").ok().and_then(|m| direct(objects, m));
    let stream = |id: &ObjectId| {
        objects
            .read(*id, |o| o.as_stream().is_ok())
            .unwrap_or(false)
    };
    ImageFacts {
        stencil: stencil.and_then(|m| m.as_bool().ok()) == Some(true),
        smask: reference(b"SMask"),
        mask: reference(b"Mask").filter(stream),
        oc: reference(b"OC"),
    }
}

/// `dict`'s `key`, an array of numbers, as PDFium reads a matrix or a
/// rectangle: an array of `len` elements, each a finite number or else
/// 0; where it's an array of another length, the identity matrix or the
/// empty rectangle; None where it isn't an array.
pub(super) fn array(
    objects: &impl ReadObjects,
    dict: &Dictionary,
    key: &[u8],
    len: usize,
) -> Option<Vec<f32>> {
    let array = direct(objects, dict.get(key).ok()?)?.as_array().ok()?;
    if array.len() != len {
        return Some(match len {
            // PDFium's default matrix is the identity.
            6 => vec![1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            _ => vec![0.0; len],
        });
    }
    Some(
        array
            .iter()
            .map(|n| {
                direct(objects, n)
                    .and_then(|n| n.as_float().ok())
                    .map(|n| n as f32)
                    .filter(|n| n.is_finite())
                    .unwrap_or(0.0)
            })
            .collect(),
    )
}
