//! Stamps as PDF pages (M9 decision 6).
//!
//! A *collection* is a PDF with a stamp per page, the way Acrobat keeps its
//! custom stamps: each page is named in the file's `/Names /Pages` tree as
//! `#<id>=<name>`, and the file's `/Info /Title` names the collection. An
//! Acrobat stamp file reads the same way.
//!
//! A stamp placed in a document is its page alone, a one-page PDF named
//! by its blob's stem ([`BlobId::stem`](crate::store::BlobId::stem)),
//! which the mark holds ([`FormRef`](crate::model::FormRef)). Saved, its
//! appearance draws that page as a Form XObject ([`page_form`]), read from
//! the blob as a source, which the file holds from then on;
//! [`form_to_pdf`] makes a one-page PDF of it again, to show or copy. The
//! files made here are written through sheaf's serializer
//! ([`write_built`](crate::serialize::full::write_built)).
//!
//! A collection is the user's own file, read whole ([`read_collection`],
//! [`one_page`], [`build_collection`]), so what copies its pages copies
//! from a document loaded whole.

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result, bail};
use lopdf::{Dictionary, Document as Pdf, Object, ObjectId, Stream, dictionary};
use sheaf_edit::{PageSize, Rotation};

use crate::attachments::Objects;
use crate::content::PageContent;
use crate::geom::{Affine, Geom, Rect};
use crate::materialize::copy::remap;
use crate::serialize::built::{Built, StampForm};
use crate::store::Src;

/// Marks a stamp page's dynamic text template (`<<initials>> <<date>>`),
/// drawn over the page when it's placed.
pub const TEXT_KEY: &str = "SheafStampText";

/// A dynamic stamp's tokens filled in: `<<name>>`, `<<initials>>`,
/// `<<date>>` and `<<time>>`, as of now.
pub fn fill(template: &str, name: &str, initials: &str, date: &str, time: &str) -> String {
    template
        .replace("<<name>>", name)
        .replace("<<initials>>", initials)
        .replace("<<date>>", date)
        .replace("<<time>>", time)
}

/// One stamp in a collection.
#[derive(Debug, Clone, PartialEq)]
pub struct StampPage {
    pub name: String,
    /// A dynamic text template, drawn over the page.
    pub text: Option<String>,
    /// The page's size, for the stamp's proportions.
    pub size: PageSize,
}

/// A collection's title (when it has one) and stamps, in page order.
pub fn read_collection(pdf: &Pdf) -> (Option<String>, Vec<StampPage>) {
    let title = pdf
        .trailer
        .get(b"Info")
        .ok()
        .and_then(|i| pdf.dereference(i).ok())
        .and_then(|(_, i)| i.as_dict().ok())
        .and_then(|i| i.get(b"Title").ok())
        .map(crate::file::pdf_text)
        .filter(|t| !t.trim().is_empty());
    let names = page_names(pdf);
    let stamps = pdf
        .get_pages()
        .into_values()
        .enumerate()
        .map(|(i, id)| {
            let dict = pdf.get_dictionary(id).ok();
            let geom = geom(pdf, id);
            StampPage {
                name: names
                    .get(&id)
                    .cloned()
                    .unwrap_or_else(|| format!("Stamp {}", i + 1)),
                text: dict
                    .and_then(|d| d.get(TEXT_KEY.as_bytes()).ok())
                    .map(crate::file::pdf_text)
                    .filter(|t| !t.is_empty()),
                size: geom.shown(),
            }
        })
        .collect();
    (title, stamps)
}

/// Each page's name from the `/Names /Pages` tree: what follows `=` in
/// `#<id>=<name>`, else the key without its `#`.
fn page_names(pdf: &Pdf) -> HashMap<ObjectId, String> {
    let mut out = HashMap::new();
    let Some(root) = pdf
        .catalog()
        .ok()
        .and_then(|c| c.get(b"Names").ok())
        .and_then(|n| pdf.dereference(n).ok())
        .and_then(|(_, n)| n.as_dict().ok())
        .and_then(|n| n.get(b"Pages").ok())
        .and_then(|p| pdf.dereference(p).ok())
        .and_then(|(_, p)| p.as_dict().ok())
    else {
        return out;
    };
    let mut todo = vec![root.clone()];
    let mut seen = 0;
    while let Some(node) = todo.pop() {
        seen += 1;
        if seen > 10_000 {
            break;
        }
        if let Ok(names) = node.get(b"Names").and_then(Object::as_array) {
            for pair in names.chunks(2) {
                let [key, value] = pair else { continue };
                let Ok(id) = value.as_reference() else {
                    continue;
                };
                let key = crate::file::pdf_text(key);
                let name = match key.split_once('=') {
                    Some((_, n)) => n.to_string(),
                    None => key.trim_start_matches('#').to_string(),
                };
                out.insert(id, name);
            }
        }
        if let Ok(kids) = node.get(b"Kids").and_then(Object::as_array) {
            for k in kids {
                if let Ok((_, Object::Dictionary(d))) = pdf.dereference(k) {
                    todo.push(d.clone());
                }
            }
        }
    }
    out
}

/// Stamp `index` of a collection alone, as a one-page PDF.
pub fn one_page(pdf: &Pdf, index: usize) -> Result<Vec<u8>> {
    let page = *pdf
        .get_pages()
        .values()
        .nth(index)
        .context("no such stamp")?;
    let mut out = Pdf::with_version("1.5");
    let pages_id = out.new_object_id();
    let mut map = HashMap::new();
    let mut dict = page_without_extras(pdf, page, &mut out, &mut map)?;
    // What the page inherits, it takes along.
    let geom = geom(pdf, page);
    dict.set("MediaBox", visible_array(&geom));
    if geom.rotate != Rotation::R0 {
        dict.set("Rotate", geom.rotate.degrees());
    }
    dict.set("Parent", pages_id);
    let page_id = map[&page];
    out.objects.insert(page_id, Object::Dictionary(dict));
    finish(&mut out, pages_id, vec![page_id], None, Vec::new())
}

/// A page's visible box as a PDF rectangle.
fn visible_array(geom: &Geom) -> Object {
    let v = geom.visible();
    Object::Array([v.x0, v.y0, v.x1, v.y1].map(Object::from).to_vec())
}

/// A page copied into `dst` without what a stamp doesn't need: its
/// annotations, and anything pointing back at the document it came from.
fn page_without_extras(
    src: &Pdf,
    page: ObjectId,
    dst: &mut Pdf,
    map: &mut HashMap<ObjectId, ObjectId>,
) -> Result<Dictionary> {
    // Copy from a trimmed clone, so the annotations aren't copied at all.
    let mut trimmed = src.get_dictionary(page)?.clone();
    for key in [b"Annots".as_slice(), b"B", b"StructParents", b"Thumb"] {
        trimmed.remove(key);
    }
    let mut scratch = src.clone();
    scratch
        .objects
        .insert(page, Object::Dictionary(trimmed.clone()));
    let mut dict = copy_page_dict(&scratch, page, dst, map)?;
    // Resources a page inherits from its tree.
    if !dict.has(b"Resources")
        && let Some(r) = inherited(src, page, b"Resources")
    {
        dict.set("Resources", remap(&scratch, &r, dst, map)?);
    }
    Ok(dict)
}

/// Clone a page dictionary from `src` into `dst`, deep-copying everything
/// it references (resources, content, annotations…) with fresh object
/// numbers. Other pages aren't copied (see [`remap`]): a reference to one
/// the caller is copying too goes through `map`, where the caller
/// reserved it.
fn copy_page_dict(
    src: &Pdf,
    page: ObjectId,
    dst: &mut Pdf,
    map: &mut HashMap<ObjectId, ObjectId>,
) -> Result<Dictionary> {
    let dict = src.get_dictionary(page)?.clone();
    // The page's own id must map to the object we're about to create so
    // annotations' /P back-references resolve; the caller inserts it.
    let new_id = match map.get(&page) {
        Some(&id) => id,
        None => {
            let id = dst.new_object_id();
            map.insert(page, id);
            id
        }
    };
    let mut out = Dictionary::new();
    for (k, v) in dict.iter() {
        if k == b"Parent" {
            continue;
        }
        out.set(k.clone(), remap(src, v, dst, map)?);
    }
    // Hold the id so later allocations don't collide; the caller may
    // replace this placeholder, or it stays as the page.
    dst.objects
        .entry(new_id)
        .or_insert_with(|| Object::Dictionary(out.clone()));
    Ok(out)
}

/// An attribute a page inherits from its ancestors.
fn inherited(pdf: &Pdf, page: ObjectId, key: &[u8]) -> Option<Object> {
    inherited_as(pdf, page, key, |v| Some(v.clone()))
}

/// The first of `page`'s and then its ancestors' `key`s that `read`
/// reads.
fn inherited_as<T>(
    pdf: &Pdf,
    page: ObjectId,
    key: &[u8],
    read: impl Fn(&Object) -> Option<T>,
) -> Option<T> {
    let mut id = page;
    for _ in 0..64 {
        let d = pdf.get_dictionary(id).ok()?;
        if let Some(v) = d.get(key).ok().and_then(&read) {
            return Some(v);
        }
        id = d.get(b"Parent").ok()?.as_reference().ok()?;
    }
    None
}

/// Page `page`'s boxes and `/Rotate`, each its own or what it inherits,
/// read as the page table reads a source's (`source::pages`), so a stamp
/// is sized as its form draws it: an entry that resolves is the page's,
/// read or not; a box's numbers through references, a real `/Rotate`
/// cut to a whole number.
fn geom(pdf: &Pdf, page: ObjectId) -> Geom {
    let found = |key: &[u8]| {
        inherited_as(pdf, page, key, |o| {
            pdf.dereference(o).ok().map(|(_, o)| o.clone())
        })
    };
    let number = |o: &Object| match pdf.dereference(o).map(|(_, o)| o) {
        Ok(Object::Integer(i)) => *i as f32,
        Ok(Object::Real(r)) => *r as f32,
        _ => 0.0,
    };
    let boxed = |key: &[u8]| match found(key)? {
        Object::Array(a) if a.len() == 4 => {
            let [a, b, c, d] = [0, 1, 2, 3].map(|i| number(&a[i]));
            Some(Rect::new(a, b, c, d))
        }
        _ => None,
    };
    let rotate = match found(b"Rotate") {
        Some(Object::Integer(i)) => i,
        Some(Object::Real(r)) => r as i64,
        _ => 0,
    };
    Geom::read(boxed(b"MediaBox"), boxed(b"CropBox"), rotate)
}

/// A collection titled `title` of `stamps`: each a one-page PDF, with its
/// name and dynamic text.
pub fn build_collection(
    title: &str,
    stamps: &[(String, Option<String>, Vec<u8>)],
) -> Result<Vec<u8>> {
    let mut out = Pdf::with_version("1.5");
    let pages_id = out.new_object_id();
    let mut kids = Vec::new();
    let mut names = Vec::new();
    for (name, text, bytes) in stamps {
        let src = Pdf::load_mem(bytes).context("reading a stamp")?;
        let page = *src
            .get_pages()
            .values()
            .next()
            .context("a stamp with no page")?;
        let mut map = HashMap::new();
        let mut dict = page_without_extras(&src, page, &mut out, &mut map)?;
        dict.set("MediaBox", visible_array(&geom(&src, page)));
        dict.set("Parent", pages_id);
        dict.remove(TEXT_KEY.as_bytes());
        if let Some(t) = text.as_ref().filter(|t| !t.is_empty()) {
            dict.set(TEXT_KEY, text_string(t));
        }
        let id = map[&page];
        out.objects.insert(id, Object::Dictionary(dict));
        kids.push(id);
        names.push((format!("#{}={name}", random_id()), id));
    }
    finish(&mut out, pages_id, kids, Some(title), names)
}

/// Close off a document: its page tree, page names and title.
fn finish(
    out: &mut Pdf,
    pages_id: ObjectId,
    kids: Vec<ObjectId>,
    title: Option<&str>,
    names: Vec<(String, ObjectId)>,
) -> Result<Vec<u8>> {
    if kids.is_empty() {
        bail!("a collection needs at least one stamp");
    }
    out.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Count" => kids.len() as i64,
            "Kids" => kids.iter().map(|k| Object::Reference(*k)).collect::<Vec<_>>(),
        }),
    );
    let mut catalog = dictionary! { "Type" => "Catalog", "Pages" => pages_id };
    if !names.is_empty() {
        // A name tree's keys are sorted.
        let mut names = names;
        names.sort_by(|a, b| a.0.cmp(&b.0));
        let arr: Vec<Object> = names
            .into_iter()
            .flat_map(|(k, id)| [text_string(&k), Object::Reference(id)])
            .collect();
        catalog.set(
            "Names",
            dictionary! { "Pages" => dictionary! { "Names" => arr } },
        );
    }
    let catalog = out.add_object(catalog);
    out.trailer.set("Root", catalog);
    if let Some(t) = title {
        let info = out.add_object(dictionary! { "Title" => text_string(t) });
        out.trailer.set("Info", info);
    }
    crate::serialize::full::write_built(out)
}

fn text_string(s: &str) -> Object {
    if s.is_ascii() {
        Object::string_literal(s)
    } else {
        let mut b = vec![0xFE, 0xFF];
        for u in s.encode_utf16() {
            b.extend(u.to_be_bytes());
        }
        Object::String(b, lopdf::StringFormat::Hexadecimal)
    }
}

/// 22 letters and digits, the length of Acrobat's stamp ids.
fn random_id() -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    (0..22)
        .map(|_| ALPHABET[fastrand::usize(..ALPHABET.len())] as char)
        .collect()
}

/// An image file as a one-page stamp PDF at 72 pixels to the inch, its
/// transparency kept as a soft mask (signatures, logos).
pub fn image_stamp(path: &Path) -> Result<Vec<u8>> {
    let img = image::open(path).with_context(|| format!("reading {}", path.display()))?;
    stamp_of(img)
}

/// An image's bytes as [`image_stamp`] makes a file's, and the bytes its
/// pixels take made a stamp (four a pixel): what a plugin uploads for a
/// stamp, its format told by the bytes. Those pixels must fit in `budget`,
/// told by the image's header before it's decoded, and the decoder may
/// take no more, so a small file of a huge image is refused, not made.
pub fn image_stamp_bytes(bytes: &[u8], budget: u64) -> Result<(Vec<u8>, u64)> {
    let reader = || {
        let reader = image::ImageReader::new(std::io::Cursor::new(bytes));
        reader.with_guessed_format().context("reading the image")
    };
    let (w, h) = reader()?.into_dimensions().context("reading the image")?;
    let pixels = u64::from(w) * u64::from(h) * 4;
    if pixels > budget {
        bail!(
            "a {w}×{h} image takes {} MiB made a stamp, more than the {} MiB left",
            pixels >> 20,
            budget >> 20
        );
    }
    let mut reader = reader()?;
    let mut limits = image::Limits::default();
    (limits.max_image_width, limits.max_image_height) = (Some(w), Some(h));
    limits.max_alloc = Some(budget);
    reader.limits(limits);
    let img = reader.decode().context("reading the image")?;
    Ok((stamp_of(img)?, pixels))
}

/// `img` as a one-page stamp PDF ([`image_stamp`]).
fn stamp_of(img: image::DynamicImage) -> Result<Vec<u8>> {
    let img = img.to_rgba8();
    let (w, h) = img.dimensions();
    if w == 0 || h == 0 {
        bail!("the image is empty");
    }
    let mut rgb = Vec::with_capacity((w * h * 3) as usize);
    let mut alpha = Vec::with_capacity((w * h) as usize);
    for p in img.pixels() {
        rgb.extend_from_slice(&p.0[..3]);
        alpha.push(p.0[3]);
    }
    let mut out = Pdf::with_version("1.5");
    let pages_id = out.new_object_id();
    let mut image = dictionary! {
        "Type" => "XObject", "Subtype" => "Image",
        "Width" => w as i64, "Height" => h as i64,
        "ColorSpace" => "DeviceRGB", "BitsPerComponent" => 8,
    };
    if alpha.iter().any(|a| *a < 255) {
        let mask = Stream::new(
            dictionary! {
                "Type" => "XObject", "Subtype" => "Image",
                "Width" => w as i64, "Height" => h as i64,
                "ColorSpace" => "DeviceGray", "BitsPerComponent" => 8,
            },
            alpha,
        );
        let mask = out.add_object(compressed(mask));
        image.set("SMask", mask);
    }
    let image = out.add_object(compressed(Stream::new(image, rgb)));
    let (pw, ph) = (w as f32, h as f32);
    let content = out.add_object(Stream::new(
        dictionary! {},
        format!("q {pw} 0 0 {ph} 0 0 cm /Im Do Q").into_bytes(),
    ));
    let page = out.add_object(dictionary! {
        "Type" => "Page", "Parent" => pages_id, "Contents" => content,
        "MediaBox" => vec![0.into(), 0.into(), Object::from(pw), Object::from(ph)],
        "Resources" => dictionary! { "XObject" => dictionary! { "Im" => image } },
    });
    finish(&mut out, pages_id, vec![page], None, Vec::new())
}

fn compressed(mut s: Stream) -> Stream {
    let _ = s.compress();
    s
}

/// The page of a one-page stamp PDF, read as a source, as a Form XObject
/// in `dst`. Its content is the page's, its resources copied over; it
/// draws the page as shown, from the origin ([`PageForm`]).
pub fn page_form(stamp: &Src, dst: &mut Pdf) -> Result<PageForm> {
    if stamp.page_count() == 0 {
        bail!("a stamp with no page");
    }
    Ok(page_forms(stamp, &[0], dst)?.remove(0))
}

/// A page made a Form XObject: its `/BBox` is the page's visible box, in
/// the page's user space, and its `/Matrix` turns and moves that as the
/// page shows, so the form draws the page as shown with its bottom-left
/// corner at the origin, [`Geom::shown`] across.
#[derive(Debug, Clone, Copy)]
pub struct PageForm {
    pub id: ObjectId,
    pub bbox: Rect,
    pub matrix: Affine,
    /// The page's boxes and rotation.
    pub geom: Geom,
}

/// The matrix that draws a page of `geom` as it shows: [`Geom::to_view`],
/// with y turned up from the bottom.
fn shown_matrix(geom: &Geom) -> Affine {
    geom.to_view().then(Affine {
        d: -1.0,
        f: geom.shown().height,
        ..Affine::IDENTITY
    })
}

/// Pages `pages` of `src`, each by its index, as Form XObjects in `dst`
/// ([`PageForm`]): each page's streams as one, as a viewer draws them, and
/// its resources (its own, or what it inherits) copied over, what the
/// pages share, fonts and images, once. A page with none makes a form
/// whose `/Resources` is empty: PDF 2.0 requires a form's (the Arlington
/// model's `XObjectFormType1`), and a reader before it may draw a form
/// without them with the resources of the page it's on (ISO 32000-1
/// 7.8.3). Nothing else of a page is copied: not its annotations, nor
/// what points back at its document.
pub fn page_forms(src: &Src, pages: &[u32], dst: &mut Pdf) -> Result<Vec<PageForm>> {
    let mut map = HashMap::new();
    let mut out = Vec::with_capacity(pages.len());
    for &index in pages {
        let page =
            (src.page(index)).with_context(|| format!("the file has no page {}", index + 1))?;
        let geom = page.geom;
        let resources = (page.resources.as_deref())
            .map(|r| remap(src.pdf(), r, dst, &mut map))
            .transpose()?;
        let content =
            PageContent::read(src.pdf(), &page.contents, src.limits()).with_context(|| {
                format!("reading the content of page {} {} R", page.id.0, page.id.1)
            })?;
        // Adding 0 makes a -0 0, the matrix as it's written.
        let Affine { a, b, c, d, e, f } = shown_matrix(&geom);
        let matrix = Affine {
            a: a + 0.0,
            b: b + 0.0,
            c: c + 0.0,
            d: d + 0.0,
            e: e + 0.0,
            f: f + 0.0,
        };
        let mut form = dictionary! {
            "Type" => "XObject",
            "Subtype" => "Form",
            "BBox" => visible_array(&geom),
        };
        if matrix != Affine::IDENTITY {
            let Affine { a, b, c, d, e, f } = matrix;
            form.set("Matrix", [a, b, c, d, e, f].map(Object::from).to_vec());
        }
        form.set(
            "Resources",
            resources.unwrap_or_else(|| Dictionary::new().into()),
        );
        let form = Stream::new(form, content.into_bytes());
        out.push(PageForm {
            id: dst.add_object(compressed(form)),
            bbox: geom.visible(),
            matrix,
            geom,
        });
    }
    Ok(out)
}

/// A one-page PDF of the Form XObject `form` in `src`: a placed stamp's
/// page, back from its saved appearance. The page is the box the form's
/// `/Matrix` makes of its `/BBox`, which is what it draws.
pub fn form_to_pdf<P: Objects + ?Sized>(src: &P, form: ObjectId) -> Result<Vec<u8>> {
    let (bbox, matrix) = form_frame(src, form).context("a form with no box")?;
    let mut built = Built::new();
    let copied =
        crate::materialize::copy::copy_object(src, form, built.pdf(), &mut HashMap::new())?;
    built.form_page(StampForm::written(
        copied.context("a page, not a form")?,
        bbox,
        matrix,
    ));
    built.finish()
}

/// Form `form`'s `/BBox` and `/Matrix` (the identity when it has none);
/// None when it isn't a form with a box.
pub(crate) fn form_frame<P: Objects + ?Sized>(src: &P, form: ObjectId) -> Option<(Rect, Affine)> {
    let Ok(Object::Stream(s)) = src.get_object(form) else {
        return None;
    };
    let numbers = |key: &[u8], n: usize| -> Option<Vec<f32>> {
        let (_, array) = src.dereference(s.dict.get(key).ok()?).ok()?;
        let values: Vec<f32> = array
            .as_array()
            .ok()?
            .iter()
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::annot::Pt;

    fn square(color: &str) -> Vec<u8> {
        let mut out = Pdf::with_version("1.5");
        let pages_id = out.new_object_id();
        let content = out.add_object(Stream::new(
            dictionary! {},
            format!("{color} rg 0 0 100 50 re f").into_bytes(),
        ));
        let page = out.add_object(dictionary! {
            "Type" => "Page", "Parent" => pages_id, "Contents" => content,
            "MediaBox" => vec![0.into(), 0.into(), 100.into(), 50.into()],
        });
        finish(&mut out, pages_id, vec![page], None, Vec::new()).unwrap()
    }

    #[test]
    fn a_collection_keeps_names_text_and_order() {
        let bytes = build_collection(
            "Review",
            &[
                (
                    "Approved".into(),
                    Some("<<initials>> <<date>>".into()),
                    square("0 1 0"),
                ),
                ("Rejected".into(), None, square("1 0 0")),
            ],
        )
        .unwrap();
        let pdf = Pdf::load_mem(&bytes).unwrap();
        let (title, stamps) = read_collection(&pdf);
        assert_eq!(title.as_deref(), Some("Review"));
        let names: Vec<&str> = stamps.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["Approved", "Rejected"]);
        assert_eq!(stamps[0].text.as_deref(), Some("<<initials>> <<date>>"));
        assert_eq!(stamps[1].size.width, 100.0);
        // One stamp alone, and back into a collection, unchanged.
        let one = one_page(&pdf, 1).unwrap();
        let again = build_collection("Review", &[("Rejected".into(), None, one)]).unwrap();
        let (_, stamps) = read_collection(&Pdf::load_mem(&again).unwrap());
        assert_eq!(stamps[0].name, "Rejected");
    }

    #[test]
    fn an_acrobat_style_name_reads_as_its_name() {
        let bytes = build_collection("x", &[("Paid".into(), None, square("0 0 1"))]).unwrap();
        let pdf = Pdf::load_mem(&bytes).unwrap();
        // What build_collection writes is Acrobat's shape: #<id>=<name>.
        let names = pdf
            .catalog()
            .unwrap()
            .get(b"Names")
            .unwrap()
            .as_dict()
            .unwrap()
            .get(b"Pages")
            .unwrap()
            .as_dict()
            .unwrap()
            .get(b"Names")
            .unwrap()
            .as_array()
            .unwrap()
            .clone();
        let key = crate::file::pdf_text(&names[0]);
        assert!(key.starts_with('#') && key.ends_with("=Paid"), "{key}");
        assert_eq!(read_collection(&pdf).1[0].name, "Paid");
    }

    /// A stamp page is sized as its form draws it: its geometry read as
    /// the page table reads it, a real `/Rotate` and a box's number by
    /// reference among them, in the collection and the page alone.
    #[test]
    fn a_stamp_is_sized_as_its_form_draws_it() {
        let mut pdf = Pdf::with_version("1.5");
        let pages_id = pdf.new_object_id();
        let content = pdf.add_object(Stream::new(dictionary! {}, b"0 0 100 50 re f".to_vec()));
        let width = pdf.add_object(100);
        let page = pdf.add_object(dictionary! {
            "Type" => "Page", "Parent" => pages_id, "Contents" => content,
            "MediaBox" => vec![0.into(), 0.into(), width.into(), 50.into()],
            "Rotate" => Object::Real(90.0),
        });
        let bytes = finish(&mut pdf, pages_id, vec![page], None, Vec::new()).unwrap();
        let built = build_collection("x", &[("Turned".into(), None, bytes)]).unwrap();
        let collection = Pdf::load_mem(&built).unwrap();
        let size = read_collection(&collection).1[0].size;
        assert_eq!((size.width, size.height), (50.0, 100.0));
        let alone = crate::forms::fixture::source(one_page(&collection, 0).unwrap());
        let form = page_form(&alone, &mut Pdf::with_version("1.5")).unwrap();
        let shown = form.geom.shown();
        assert_eq!((shown.width, shown.height), (size.width, size.height));
    }

    /// A page with no resources becomes a form whose `/Resources` is
    /// empty, never one without: what it draws uses none, and not the
    /// page's it's placed on.
    #[test]
    fn a_page_with_no_resources_is_a_form_with_none() {
        let mut pdf = Pdf::with_version("1.5");
        let pages_id = pdf.new_object_id();
        let content = pdf.add_object(Stream::new(dictionary! {}, b"0 0 10 10 re f".to_vec()));
        let page = pdf.add_object(dictionary! {
            "Type" => "Page", "Parent" => pages_id, "Contents" => content,
            "MediaBox" => vec![0.into(), 0.into(), 100.into(), 50.into()],
        });
        let bytes = finish(&mut pdf, pages_id, vec![page], None, Vec::new()).unwrap();
        let stamp = crate::forms::fixture::source(bytes);
        let mut dst = Pdf::with_version("1.5");
        let form = page_form(&stamp, &mut dst).unwrap();
        let dict = &dst.get_object(form.id).unwrap().as_stream().unwrap().dict;
        assert_eq!(
            dict.get(b"Resources").unwrap(),
            &Object::Dictionary(Dictionary::new())
        );
    }

    /// A page cropped off the origin and turned a quarter becomes a form
    /// that draws it as it shows from the origin: its matrix takes the
    /// visible box to the shown page's, and the page made back from the
    /// form is the shown size.
    #[test]
    fn a_cropped_turned_page_is_a_form_drawn_as_it_shows() {
        let mut pdf = Pdf::with_version("1.5");
        let pages_id = pdf.new_object_id();
        let content = pdf.add_object(Stream::new(
            dictionary! {},
            b"0 0 1 rg 60 70 10 10 re f".to_vec(),
        ));
        let page = pdf.add_object(dictionary! {
            "Type" => "Page", "Parent" => pages_id, "Contents" => content,
            "MediaBox" => vec![0.into(), 0.into(), 300.into(), 200.into()],
            "CropBox" => vec![50.into(), 60.into(), 250.into(), 160.into()],
            "Rotate" => 90,
        });
        let bytes = finish(&mut pdf, pages_id, vec![page], None, Vec::new()).unwrap();
        let stamp = crate::forms::fixture::source(bytes);
        let mut dst = Pdf::with_version("1.5");
        let form = page_form(&stamp, &mut dst).unwrap();
        let size = form.geom.shown();
        assert_eq!((size.width, size.height), (100.0, 200.0));
        assert_eq!(form_frame(&dst, form.id), Some((form.bbox, form.matrix)));
        let form = form.id;
        let s = dst.get_object(form).unwrap().as_stream().unwrap();
        let numbers = |key: &[u8]| -> Vec<f32> {
            let array = s.dict.get(key).unwrap().as_array().unwrap();
            array.iter().map(|v| v.as_float().unwrap() as f32).collect()
        };
        assert_eq!(numbers(b"BBox"), [50.0, 60.0, 250.0, 160.0]);
        let [a, b, c, d, e, f] = numbers(b"Matrix")[..] else {
            panic!("a matrix")
        };
        let m = Affine { a, b, c, d, e, f };
        // Turned a quarter clockwise, the box's bottom-left is the shown
        // top-left, and its top-right the shown bottom-right.
        let at = |x, y| m.apply(Pt { x, y });
        assert_eq!(at(50.0, 60.0), Pt { x: 0.0, y: 200.0 });
        assert_eq!(at(250.0, 160.0), Pt { x: 100.0, y: 0.0 });
        let back = Pdf::load_mem(&form_to_pdf(&dst, form).unwrap()).unwrap();
        let page = back.get_pages()[&1];
        let media = back.get_dictionary(page).unwrap().get(b"MediaBox").unwrap();
        let media: Vec<f32> = media
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_float().unwrap() as f32)
            .collect();
        assert_eq!(media, [0.0, 0.0, 100.0, 200.0]);
    }

    /// A page made a form takes its content and the resources it
    /// inherits, and nothing else of the page: not its annotations, nor
    /// its metadata.
    #[test]
    fn a_page_form_copies_its_resources_alone() {
        let mut pdf = Pdf::with_version("1.5");
        let pages_id = pdf.new_object_id();
        let font = pdf.add_object(dictionary! {
            "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
        });
        let content = pdf.add_object(Stream::new(
            dictionary! {},
            b"BT /F1 12 Tf 10 10 Td (Hi) Tj ET".to_vec(),
        ));
        let metadata = pdf.add_object(Stream::new(
            dictionary! { "Type" => "Metadata", "Subtype" => "XML" },
            b"<x/>".to_vec(),
        ));
        let note = pdf.add_object(dictionary! {
            "Type" => "Annot", "Subtype" => "Text",
            "Rect" => vec![0.into(), 0.into(), 10.into(), 10.into()],
        });
        let page = pdf.add_object(dictionary! {
            "Type" => "Page", "Parent" => pages_id, "Contents" => content,
            "Metadata" => metadata, "Annots" => vec![note.into()],
        });
        pdf.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1,
                "MediaBox" => vec![0.into(), 0.into(), 100.into(), 50.into()],
                "Resources" => dictionary! { "Font" => dictionary! { "F1" => font } },
            }),
        );
        let catalog = pdf.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        pdf.trailer.set("Root", catalog);
        let src = crate::forms::fixture::source(crate::serialize::full::write_built(&pdf).unwrap());
        let mut dst = Pdf::with_version("1.5");
        let form = page_form(&src, &mut dst).unwrap();
        assert_eq!(dst.objects.len(), 2, "the form and its font");
        let Object::Stream(s) = dst.get_object(form.id).unwrap() else {
            panic!("a form")
        };
        let content = String::from_utf8(s.decompressed_content().unwrap()).unwrap();
        assert!(content.contains("(Hi) Tj"), "{content}");
        let fonts = s.dict.get(b"Resources").unwrap().as_dict().unwrap();
        let f1 = fonts.get(b"Font").unwrap().as_dict().unwrap().get(b"F1");
        let font = dst
            .get_dictionary(f1.unwrap().as_reference().unwrap())
            .unwrap();
        assert_eq!(
            font.get(b"BaseFont").unwrap().as_name().unwrap(),
            b"Helvetica"
        );
    }

    #[test]
    fn a_stamp_page_becomes_a_form_and_comes_back() {
        let stamp = crate::forms::fixture::source(square("1 0 0"));
        let mut dst = Pdf::with_version("1.5");
        let form = page_form(&stamp, &mut dst).unwrap();
        let size = form.geom.shown();
        assert_eq!((size.width, size.height), (100.0, 50.0));
        let form = form.id;
        let Object::Stream(s) = dst.get_object(form).unwrap() else {
            panic!()
        };
        assert_eq!(s.dict.get(b"Subtype").unwrap().as_name().unwrap(), b"Form");
        let content = String::from_utf8(s.decompressed_content().unwrap()).unwrap();
        assert!(content.contains("re f"), "{content}");
        // From the form, a one-page PDF again.
        let back = form_to_pdf(&dst, form).unwrap();
        let back = Pdf::load_mem(&back).unwrap();
        assert_eq!(back.get_pages().len(), 1);
    }

    /// An image's pixels must fit the budget, told before it's decoded.
    #[test]
    fn an_image_stamp_is_made_within_its_budget() {
        let mut png = Vec::new();
        let img = image::RgbaImage::from_pixel(30, 20, image::Rgba([0, 0, 255, 128]));
        img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let err = image_stamp_bytes(&png, 30 * 20 * 4 - 1).unwrap_err();
        assert!(format!("{err:#}").contains("30×20"), "{err:#}");
        let (bytes, pixels) = image_stamp_bytes(&png, 30 * 20 * 4).unwrap();
        assert_eq!(pixels, 30 * 20 * 4);
        let stamp = crate::forms::fixture::source(bytes);
        let form = page_form(&stamp, &mut Pdf::with_version("1.5")).unwrap();
        let size = form.geom.shown();
        assert_eq!((size.width, size.height), (30.0, 20.0));
    }
}
