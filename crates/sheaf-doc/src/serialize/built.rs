//! PDFs sheaf builds in memory (M10.5 §11.7, M18 slices 10–12), each a
//! `sheaf_doc::pdf::Document` graph numbered as it's built and written whole
//! by [`write_built`](super::full::write_built).
//!
//! What content draws with is written once a PDF by a [`ResObjects`]: a
//! [`Built`]'s own, and a [`WriteCx`]'s, which writes marks into a PDF
//! another owns. The fallback font ([`Fallback`]) is written last, as the
//! subset of every glyph content bound through one drew, at the number it
//! gave the fallback's marker: so every owner finishes
//! ([`ResObjects::finish`], [`WriteCx::finish`], [`Built::finish`]) before
//! its PDF is written, and two owners writing into one PDF write a subset
//! each.

use std::collections::HashMap;

use anyhow::{Context as _, Result};

use crate::annot::{Annot, AnnotKind, Drawing};
#[cfg(any(test, feature = "fixtures"))]
use crate::annot::{Emitted, emission};
use crate::content::sink::{self, Sink};
use crate::content::{
    Fallback, Glyphs, Need, Res, ResBody, ResId, ResKind, Reserved, ResourceNeeds, Uses,
};
use crate::geom::{Affine, Rect};
use crate::materialize::copy::copy_object;
use crate::model::{FormRef, Mark};
use crate::pdf::annots::SynthesizedAp;
use crate::pdf::{
    Dictionary, Document as Pdf, Object, ObjectId, Stream, StreamExt, dictionary, resolve_fn,
};
use crate::source::Limits;
use crate::store::{BlobId, BlobRef};

/// The resources content draws with, written into a PDF once each,
/// however much content draws with them, and the fallback font's subset
/// of every glyph content drew in it, written by [`ResObjects::finish`].
/// Dropped with the fallback bound and its subset unwritten, a debug build
/// panics: a reference to the subset would dangle.
#[derive(Debug, Default)]
pub struct ResObjects {
    written: HashMap<ResId, ObjectId>,
    fallback: Subset,
}

/// The fallback font's subset a [`ResObjects`] writes.
#[derive(Debug, Default)]
struct Subset {
    /// Its font's number, reserved the first time content binds the
    /// fallback.
    at: Option<ObjectId>,
    /// Every glyph joined: what content bound through it draws.
    joined: Glyphs,
    /// What was written at `at`, and each of its objects' numbers, `at`
    /// first: none until [`ResObjects::finish`].
    written: Option<(Glyphs, Vec<ObjectId>)>,
}

impl Subset {
    /// Whether nothing reserved is left unwritten.
    fn finished(&self) -> bool {
        self.at.is_none()
            || self
                .written
                .as_ref()
                .is_some_and(|(g, _)| *g == self.joined)
    }
}

impl ResObjects {
    pub fn object(&mut self, pdf: &mut Pdf, res: &Res) -> ObjectId {
        if Fallback::SANS.is(res) {
            return *self.fallback.at.get_or_insert_with(|| pdf.new_object_id());
        }
        match res.body() {
            ResBody::Written(id) => *id,
            ResBody::Objects(objects) => *self
                .written
                .entry(res.id())
                .or_insert_with(|| add(pdf, objects)),
            // A kind of resource made after this was written: its name
            // binds nothing, written once.
            body => *self.written.entry(res.id()).or_insert_with(|| {
                log::warn!("a resource of an unknown kind is written as null: {body:?}");
                pdf.add_object(Object::null())
            }),
        }
    }

    pub fn bind(
        &mut self,
        pdf: &mut Pdf,
        needs: &ResourceNeeds,
        glyphs: &Glyphs,
        resources: &mut Dictionary,
    ) {
        crate::content::sink::bind(&mut Graph { pdf, res: self }, needs, glyphs, resources);
    }

    /// Writes the fallback font's subset, of every glyph joined, at the
    /// number content binds it by, where content bound it: its owner calls
    /// it once its content is bound, before its PDF is written. Called
    /// again, it writes nothing, unless more was joined since: then the
    /// subset is written again at the same numbers. The fallback bound with
    /// no glyph joined (content that missed its join, a bug) is said, and
    /// its subset written of none, so nothing dangles; glyphs joined with
    /// nothing bound are said, and nothing written.
    pub fn finish(&mut self, pdf: &mut Pdf) {
        let sub = &mut self.fallback;
        let Some(at) = sub.at else {
            if !sub.joined.is_empty() {
                log::error!(
                    "{} glyphs drawn in the fallback font were joined, but nothing binds it: \
                     no subset is written",
                    sub.joined.len()
                );
            }
            return;
        };
        if sub.finished() {
            return;
        }
        if sub.joined.is_empty() {
            log::error!(
                "the fallback font is bound, but no glyph drawn in it was joined: \
                 its subset is written with none"
            );
        }
        let subset = Fallback::SANS.subset(&sub.joined);
        let ResBody::Objects(objects) = subset.body() else {
            unreachable!("a subset is its objects")
        };
        let mut ids = sub.written.take().map_or_else(|| vec![at], |(_, ids)| ids);
        for gone in ids.drain(objects.len().min(ids.len())..) {
            pdf.objects.remove(&gone);
        }
        while ids.len() < objects.len() {
            ids.push(pdf.new_object_id());
        }
        add_at(pdf, objects, &ids);
        sub.written = Some((sub.joined.clone(), ids));
    }
}

/// A [`ResObjects`] dropped with the fallback bound and its subset not
/// written, or not of every glyph joined since, is a reference left
/// dangling: a debug build panics (not while panicking already).
impl Drop for ResObjects {
    fn drop(&mut self) {
        if cfg!(debug_assertions) && !std::thread::panicking() {
            assert!(
                self.fallback.finished(),
                "the fallback font was bound in a built PDF, and its subset never written: \
                 its owner didn't finish"
            );
        }
    }
}

fn add(pdf: &mut Pdf, objects: &[Object]) -> ObjectId {
    let ids: Vec<ObjectId> = objects.iter().map(|_| pdf.new_object_id()).collect();
    add_at(pdf, objects, &ids);
    ids[0]
}

/// `objects`, a resource's, written at `ids`, one each: each reference
/// `(i, 0)` among them to the `i`th's number, each stream compressed.
fn add_at(pdf: &mut Pdf, objects: &[Object], ids: &[ObjectId]) {
    for (object, &id) in objects.iter().zip(ids) {
        let mut object = object.clone();
        object.renumber(|old| Some(ids[old.number as usize]));
        if let Some(stream) = object.as_stream_mut() {
            stream.compress();
        }
        pdf.objects.insert(id, object);
    }
}

struct Graph<'a> {
    pdf: &'a mut Pdf,
    res: &'a mut ResObjects,
}

impl Sink for Graph<'_> {
    fn dict(&mut self, id: ObjectId) -> Option<Dictionary> {
        self.pdf.get_dictionary(id).ok().cloned()
    }

    fn array(&mut self, id: ObjectId) -> Option<Vec<Object>> {
        self.pdf
            .get_object(id)
            .ok()?
            .as_array()
            .map(<[Object]>::to_vec)
    }

    fn resource(&mut self, res: &Res) -> ObjectId {
        self.res.object(self.pdf, res)
    }

    fn fallback(&mut self, glyphs: &Glyphs) {
        self.res.fallback.joined.merge(glyphs);
    }

    fn reserved(&mut self, resources: Option<&Object>) -> Reserved {
        let objects = resolve_fn(|id| self.pdf.objects.get(&id));
        Reserved::read(objects, resources, &crate::content::sink::naming_limits())
    }
}

/// `drawn`'s form, drawing `glyphs` in the fallback font, written into
/// `pdf`, its resources through `res`.
fn as_form(drawn: SynthesizedAp, glyphs: &Glyphs, pdf: &mut Pdf, res: &mut ResObjects) -> ObjectId {
    let mut graph = Graph { pdf, res };
    let form = sink::form(
        &mut graph,
        drawn.bbox,
        drawn.matrix,
        drawn.content,
        &drawn.needs,
        glyphs,
    );
    pdf.add_object(form)
}

#[derive(Debug, Clone, Copy)]
pub struct StampForm {
    pub id: ObjectId,
    pub res: ResId,
    pub bbox: Rect,
    pub matrix: Affine,
}

impl StampForm {
    pub fn written(id: ObjectId, bbox: Rect, matrix: Affine) -> StampForm {
        StampForm {
            id,
            res: Res::written(id).id(),
            bbox,
            matrix,
        }
    }

    pub fn resource(&self) -> Res {
        Res::named(self.res, ResBody::Written(self.id))
            .expect("an object already written is a resource, whatever it holds")
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Held {
    pub stamp: Option<StampForm>,
    pub file: Option<ObjectId>,
}

#[derive(Default)]
pub struct WriteCx {
    res: ResObjects,
    stamps: HashMap<FormRef, Option<StampForm>>,
    files: HashMap<BlobId, ObjectId>,
    copied: HashMap<BlobId, HashMap<ObjectId, ObjectId>>,
}

impl WriteCx {
    pub fn new() -> WriteCx {
        WriteCx::default()
    }

    pub(crate) fn stamp(
        &mut self,
        form: &FormRef,
        stem: &str,
        name: &str,
        pdf: &mut Pdf,
    ) -> Option<StampForm> {
        if let Some(&made) = self.stamps.get(form) {
            return made;
        }
        let made = match form {
            FormRef::Base(at) => {
                let frame = crate::stamp_lib::form_frame(at.src.pdf(), at.id);
                frame
                    .context("a form with no box")
                    .and_then(|(bbox, matrix)| {
                        let copied = self.copied.entry(at.src.id()).or_default();
                        let id = copy_object(at.src.pdf(), at.id, pdf, copied)?;
                        let id = id.context("a page, not a form")?;
                        Ok(StampForm::written(id, bbox, matrix))
                    })
            }
            FormRef::Blob(page) => (page.blob().source(&Limits::DEFAULT))
                .map_err(anyhow::Error::from)
                .and_then(|src| crate::stamp_lib::page_form(&src, pdf))
                .map(|f| StampForm::written(f.id, f.bbox, f.matrix)),
            FormRef::Pinned(ap) => {
                let page = match (&ap.inputs.kind, &ap.inputs.stamp) {
                    (AnnotKind::PageStamp { stamp, name, .. }, Some(page)) => {
                        self.stamp(page, stamp, name, pdf)
                    }
                    _ => None,
                };
                let Drawing { ap: drawn, glyphs } =
                    crate::annot::appearance(&ap.inputs.annot(), page);
                let (bbox, matrix) = (Rect::from(drawn.bbox), Affine::from(drawn.matrix));
                let id = as_form(drawn, &glyphs, pdf, &mut self.res);
                Ok(StampForm::written(id, bbox, matrix))
            }
        };
        let made = made
            .inspect_err(|e| log::warn!("stamp {name} ({stem}) is drawn without its page: {e:#}"))
            .ok();
        self.stamps.insert(form.clone(), made);
        made
    }

    pub fn held(&mut self, m: &Mark, pdf: &mut Pdf) -> Held {
        held(&m.annot, m.stamp.as_ref(), m.file.as_ref(), pdf, self)
    }

    /// A mark's `appearance` as a form written into `pdf`, what it draws
    /// with through this writer, what it draws in the fallback font joined
    /// to the subset [`WriteCx::finish`] writes.
    pub fn form(&mut self, appearance: Drawing, pdf: &mut Pdf) -> ObjectId {
        let Drawing { ap, glyphs } = appearance;
        as_form(ap, &glyphs, pdf, &mut self.res)
    }

    /// Writes the fallback font's subset of what its marks drew
    /// ([`ResObjects::finish`]), into `pdf`, the PDF it wrote them into:
    /// before that's written.
    pub fn finish(&mut self, pdf: &mut Pdf) {
        self.res.finish(pdf);
    }

    fn file(&mut self, file: &BlobRef, pdf: &mut Pdf) -> ObjectId {
        *self.files.entry(file.id()).or_insert_with(|| {
            let mut stream = Stream::new(
                dictionary! {
                    "Type" => Object::name("EmbeddedFile"),
                    "Params" => dictionary! { "Size" => file.bytes().len() as i64 },
                },
                file.bytes().to_vec(),
            );
            stream.compress();
            pdf.add_object(Object::from(stream))
        })
    }
}

fn held(
    a: &Annot,
    stamp: Option<&FormRef>,
    file: Option<&BlobRef>,
    pdf: &mut Pdf,
    cx: &mut WriteCx,
) -> Held {
    let stamp = match (&a.kind, stamp) {
        (AnnotKind::PageStamp { stamp, name, .. }, Some(form)) => cx.stamp(form, stamp, name, pdf),
        (AnnotKind::PageStamp { stamp, name, .. }, None) => {
            log::warn!("stamp {name} ({stamp}) is drawn without its page: it holds none");
            None
        }
        _ => None,
    };
    let file = match (&a.kind, file) {
        (AnnotKind::FileAttachment { .. }, Some(file)) => Some(cx.file(file, pdf)),
        (
            AnnotKind::FileAttachment {
                file: stem, name, ..
            },
            None,
        ) => {
            log::warn!("attachment {name} ({stem}) is written without its file: it holds none");
            None
        }
        _ => None,
    };
    Held { stamp, file }
}

#[cfg(any(test, feature = "fixtures"))]
pub struct Written {
    pub main: Dictionary,
    pub companions: Vec<Dictionary>,
}

/// Mark `a` written into `pdf`, what it draws with too, its own
/// [`WriteCx`] finished.
#[cfg(any(test, feature = "fixtures"))]
pub fn to_pdf(a: &Annot, pdf: &mut Pdf) -> Written {
    let mut cx = WriteCx::new();
    let written = write(a, None, None, pdf, &mut cx);
    cx.finish(pdf);
    written
}

/// Mark `m` written into `pdf` through `cx`, which its caller finishes
/// ([`WriteCx::finish`]) once every mark is written.
#[cfg(any(test, feature = "fixtures"))]
pub fn to_pdf_in(m: &Mark, pdf: &mut Pdf, cx: &mut WriteCx) -> Written {
    write(&m.annot, m.stamp.as_ref(), m.file.as_ref(), pdf, cx)
}

#[cfg(any(test, feature = "fixtures"))]
fn write(
    a: &Annot,
    stamp: Option<&FormRef>,
    file: Option<&BlobRef>,
    pdf: &mut Pdf,
    cx: &mut WriteCx,
) -> Written {
    let held = held(a, stamp, file, pdf, cx);
    let Emitted {
        mut main,
        appearance,
        companions,
    } = emission(a, held);
    let ap = cx.form(appearance, pdf);
    main.set("AP", dictionary! { "N" => ap });
    let companions = companions
        .into_iter()
        .map(|(mut dict, appearance)| {
            let ap = cx.form(appearance, pdf);
            dict.set("AP", dictionary! { "N" => ap });
            dict
        })
        .collect();
    Written { main, companions }
}

pub struct Built {
    pdf: Pdf,
    res: ResObjects,
    tree: ObjectId,
    kids: Vec<Object>,
}

impl Default for Built {
    fn default() -> Built {
        Built::from(Pdf::with_version("1.5"))
    }
}

impl From<Pdf> for Built {
    fn from(mut pdf: Pdf) -> Built {
        let tree = pdf.new_object_id();
        Built {
            pdf,
            res: ResObjects::default(),
            tree,
            kids: Vec::new(),
        }
    }
}

impl Built {
    pub fn new() -> Built {
        Built::default()
    }

    pub fn pdf(&mut self) -> &mut Pdf {
        &mut self.pdf
    }

    /// A page `media` big drawing `content`, which draws with `uses`.
    pub fn page(&mut self, media: Rect, content: Vec<u8>, uses: &Uses) {
        let mut content = Stream::new(dictionary! {}, content);
        content.compress();
        let content = self.pdf.add_object(content);
        let mut resources = Dictionary::new();
        let Uses { needs, glyphs } = uses;
        self.res.bind(&mut self.pdf, needs, glyphs, &mut resources);
        let media = [media.x0, media.y0, media.x1, media.y1].map(Object::from);
        let page = self.pdf.add_object(dictionary! {
            "Type" => Object::name("Page"), "Parent" => self.tree, "Contents" => content,
            "MediaBox" => media.to_vec(), "Resources" => resources,
        });
        self.kids.push(page.into());
    }

    pub fn form_page(&mut self, form: StampForm) {
        let needs = ResourceNeeds(vec![Need {
            kind: ResKind::XObject,
            name: b"Stamp".to_vec(),
            res: form.resource(),
        }]);
        let media = form.matrix.bounds(form.bbox);
        self.page(media, b"/Stamp Do".to_vec(), &needs.into());
    }

    /// The PDF, written: the fallback font's subset of what its pages drew
    /// first ([`ResObjects::finish`]); a [`WriteCx`] that wrote into it
    /// finishes before.
    pub fn finish(mut self) -> Result<Vec<u8>> {
        self.res.finish(&mut self.pdf);
        let count = self.kids.len() as i64;
        self.pdf.objects.insert(
            self.tree,
            Object::from(dictionary! {
                "Type" => Object::name("Pages"), "Count" => count, "Kids" => self.kids,
            }),
        );
        let catalog = self.pdf.add_object(dictionary! {
            "Type" => Object::name("Catalog"), "Pages" => self.tree,
        });
        self.pdf.trailer.set("Root", catalog);
        super::full::write_built(&self.pdf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::fixture::{FALLBACK_TEXT, fallback_page, subsets_hold};
    use crate::content::{ContentWriter, DrawText, Std14};
    use crate::pdf::fonts::Program;

    /// Two Letter pages of Helvetica text, "one" and "two".
    fn helvetica_pages() -> Vec<u8> {
        let mut built = Built::new();
        for line in ["one", "two"] {
            let mut w = ContentWriter::new();
            w.begin_text();
            w.set_font(&Std14::Helvetica, 12.0);
            w.text_move(72.0, 700.0);
            w.show_text(&Std14::Helvetica, 12.0, line);
            w.end_text();
            let (content, needs) = w.finish(&Reserved::default());
            built.page(Rect::new(0.0, 0.0, 612.0, 792.0), content, &needs.into());
        }
        built.finish().unwrap()
    }

    /// A built PDF that draws nothing in the fallback font is the bytes
    /// it was before there was one (M10 slice 7): a page of Helvetica
    /// text, a FreeText saying "plain" pinned and made a one-page PDF (a
    /// flattened mark's page, two owners' resources in it), and a tape's
    /// picture, each its length and checksum as the code before the
    /// fallback wrote it. The literals were taken from that code, and
    /// never change.
    #[test]
    fn built_pdfs_without_the_fallback_are_the_bytes_they_were() {
        use std::sync::Arc;

        use crate::annot::Box2;
        use crate::model::{MarkAp, PinnedAp};

        let free_text = MarkAp {
            kind: AnnotKind::FreeText {
                rect: Box2 {
                    x: 10.0,
                    y: 20.0,
                    w: 120.0,
                    h: 40.0,
                },
                size: 12.0,
            },
            color: [200, 30, 30],
            opacity: 1.0,
            stroke: 1.0,
            contents: "plain".into(),
            fill: None,
            dashed: false,
            cloudy: false,
            turn: sheaf_edit::Rotation::R0,
            stamp: None,
        };
        let pinned = crate::annot::pinned_pdf(&Arc::new(PinnedAp::new(free_text))).unwrap();
        let tape = (crate::forms::fixture::every_mark_kind().into_iter())
            .find(|a| matches!(a.kind, AnnotKind::Tape { .. }))
            .unwrap();
        let tape = crate::annot::tape_pdf(&tape).unwrap();
        let sums =
            [helvetica_pages(), pinned, tape].map(|b| (b.len(), sheaf_util::checksum_hex(&b)));
        let sums = sums.each_ref().map(|(n, sum)| (*n, sum.as_str()));
        assert_eq!(
            sums,
            [
                (909, "be5a3ee6f27ac7f7028abba4b019bf92"),
                (960, "c955350ab0fb4287979ff9f874c2ead9"),
                (894, "ed0c93ad2ad5751a3a1e50185de445fc"),
            ]
        );
    }

    #[test]
    fn built_pages_share_their_fonts_and_read_back() {
        let bytes = helvetica_pages();
        assert_eq!(helvetica_pages(), bytes);
        let src = crate::forms::fixture::source(bytes);
        assert_eq!(src.page_count(), 2);
        let font = |index: u32| {
            let resources = src.page(index).unwrap().resources.clone().unwrap();
            let fonts = resources.as_dict().unwrap().get(b"Font").unwrap();
            let (_, font) = fonts.as_dict().unwrap().iter().next().unwrap();
            font.as_reference().unwrap()
        };
        assert_eq!(font(0), font(1));
        assert_eq!(src.page_size(1).width, 612.0);
    }

    /// Content saying `text` in the fallback font at 12 pt, 2 in and 4 up,
    /// and what it draws with.
    fn in_fallback(text: &str) -> (Vec<u8>, Uses) {
        let sans = Fallback::SANS;
        let mut glyphs = Glyphs::new();
        let codes = glyphs.add(sans.program(), text).unwrap();
        let mut w = ContentWriter::new();
        w.begin_text();
        w.font(sans.res(), 12.0);
        w.text_move(2.0, 4.0);
        w.show(&codes);
        w.end_text();
        let (content, needs) = w.finish(&Reserved::default());
        (content, Uses { needs, glyphs })
    }

    /// Each Type 0 font of `pdf`, by its number.
    fn type0s(pdf: &Pdf) -> Vec<(ObjectId, &Dictionary)> {
        (pdf.objects.iter())
            .filter_map(|(&id, object)| Some((id, object.as_dict()?)))
            .filter(|(_, d)| d.get(b"Subtype").and_then(Object::as_name) == Some(&b"Type0"[..]))
            .collect()
    }

    /// Every reference `bytes` holds, its trailer's too, that names no
    /// object it has.
    fn dangling(bytes: &[u8]) -> Vec<ObjectId> {
        let pdf = Pdf::load_mem(bytes).unwrap();
        let mut refs = Vec::new();
        let trailer = Object::from(pdf.trailer.clone());
        for object in pdf.objects.values().chain([&trailer]) {
            object.clone().renumber(|r| {
                refs.push(r);
                Some(r)
            });
        }
        refs.retain(|r| !pdf.objects.contains_key(r));
        refs
    }

    /// `object`'s stream, `pdf`'s object it names: its bytes as written
    /// and decoded.
    fn stream_of(pdf: &Pdf, object: Option<&Object>) -> (Vec<u8>, Vec<u8>) {
        let (_, object) = pdf.dereference(object.unwrap()).unwrap();
        let stream = object.as_stream().unwrap();
        (
            stream.raw_bytes().to_vec(),
            stream.decompressed_content().unwrap(),
        )
    }

    /// A page drawn by hand in the fallback embeds one Type 0 font, its
    /// subset: widths in `/W`, a `/CIDToGIDMap` stream, a `/ToUnicode`,
    /// and a program cut to the glyphs drawn, the map taking each code to
    /// its glyph in the cut; the same bytes every time.
    #[test]
    fn a_page_in_the_fallback_embeds_the_glyphs_it_draws() {
        let bytes = fallback_page(FALLBACK_TEXT);
        assert_eq!(fallback_page(FALLBACK_TEXT), bytes);
        assert!(dangling(&bytes).is_empty());
        assert_eq!(subsets_hold(&bytes), FALLBACK_TEXT.chars().count());
        let pdf = Pdf::load_mem(&bytes).unwrap();
        let [(_, type0)] = type0s(&pdf)[..] else {
            panic!("one Type 0 font")
        };
        let name = type0.get(b"BaseFont").and_then(Object::as_name).unwrap();
        assert_eq!(&name[6..], b"+NotoSans-Regular");
        let descendant = type0.get(b"DescendantFonts").unwrap();
        let descendant = pdf.dereference(descendant).unwrap().1.as_array().unwrap()[0].clone();
        let descendant = pdf.dereference(&descendant).unwrap().1.as_dict().unwrap();
        assert!(descendant.get(b"W").and_then(Object::as_array).is_some());
        let (to_unicode, _) = stream_of(&pdf, type0.get(b"ToUnicode"));
        assert!(!to_unicode.is_empty());
        let (flated, map) = stream_of(&pdf, descendant.get(b"CIDToGIDMap"));
        let descriptor = descendant.get(b"FontDescriptor").unwrap();
        let descriptor = pdf.dereference(descriptor).unwrap().1.as_dict().unwrap();
        let (_, cut) = stream_of(&pdf, descriptor.get(b"FontFile2"));
        let cut = Program::open(cut, 0).unwrap();

        let program = Fallback::SANS.program();
        let mut drawn = Glyphs::new();
        drawn.add(program, FALLBACK_TEXT).unwrap();
        assert_eq!(drawn.len(), 17);
        // The cut holds .notdef, the 17 glyphs drawn and the 4 that two of
        // them are made of (Ö of O and ¨, ç of c and ¸): it has the glyphs
        // drawn and what draws them, and nothing else.
        assert_eq!(cut.glyph_count(), 1 + 17 + 4);
        // Two bytes a CID, to the last glyph drawn, each glyph the cut holds
        // mapped from its number in the whole program to its own there.
        let last = drawn.iter().map(|(glyph, _)| glyph).max().unwrap();
        assert_eq!(map.len(), 2 * (usize::from(last) + 1));
        let to_cut = |glyph: u16| {
            let at = 2 * usize::from(glyph);
            u16::from_be_bytes([map[at], map[at + 1]])
        };
        let mapped: std::collections::BTreeSet<u16> = (0..=last).map(to_cut).collect();
        assert_eq!(mapped, (0..cut.glyph_count()).collect());
        for (glyph, _) in drawn.iter() {
            assert_ne!(to_cut(glyph), 0, "glyph {glyph} is in the cut");
            assert_eq!(cut.width(to_cut(glyph)), program.width(glyph));
        }
        println!(
            "{} glyphs drawn of {}; the cut holds {}; the map is {} bytes, {} after Flate",
            drawn.len(),
            program.glyph_count(),
            cut.glyph_count(),
            map.len(),
            flated.len()
        );
    }

    /// A [`WriteCx`] writing a form into a [`Built`]'s PDF and the built
    /// PDF's own page each write the subset of what they drew, and every
    /// reference the file holds names an object it has.
    #[test]
    fn two_owners_in_one_pdf_each_write_their_subset() {
        let mut built = Built::new();
        let mut cx = WriteCx::new();
        let (content, uses) = in_fallback("\u{141}ukasz");
        let bbox = crate::pdf::gfx::Rect {
            x0: 0.0,
            y0: 0.0,
            x1: 80.0,
            y1: 20.0,
        };
        let mut graph = Graph {
            pdf: built.pdf(),
            res: &mut cx.res,
        };
        let identity = crate::pdf::gfx::Matrix::IDENTITY;
        let form = sink::form(
            &mut graph,
            bbox,
            identity,
            content,
            &uses.needs,
            &uses.glyphs,
        );
        let form = built.pdf().add_object(form);
        let frame = (Rect::new(0.0, 0.0, 80.0, 20.0), Affine::IDENTITY);
        built.form_page(StampForm::written(form, frame.0, frame.1));
        let (content, uses) = in_fallback("\u{D6}l\u{E7}er");
        built.page(Rect::new(0.0, 0.0, 80.0, 20.0), content, &uses);
        cx.finish(built.pdf());
        let bytes = built.finish().unwrap();
        assert_eq!(dangling(&bytes), []);
        let pdf = Pdf::load_mem(&bytes).unwrap();
        assert_eq!(type0s(&pdf).len(), 2, "a subset each");
        assert_eq!(subsets_hold(&bytes), 6 + 5);
    }

    /// A subset is written once its owner finishes, again at the same
    /// numbers only once more is joined; bound with nothing joined, it's
    /// written of no glyph rather than left dangling.
    #[test]
    fn a_subset_is_written_when_its_owner_finishes() {
        let sans = Fallback::SANS;
        let mut pdf = Pdf::with_version("1.7");
        let mut res = ResObjects::default();
        let at = res.object(&mut pdf, &sans.res());
        assert_eq!(res.object(&mut pdf, &sans.res()), at, "one number");
        assert!(!pdf.objects.contains_key(&at), "reserved, not written");
        res.finish(&mut pdf);
        let empty = pdf.get_dictionary(at).unwrap().clone();
        let count = pdf.objects.len();
        res.finish(&mut pdf);
        assert_eq!(
            (pdf.objects.len(), pdf.get_dictionary(at).unwrap()),
            (count, &empty)
        );
        let mut glyphs = Glyphs::new();
        glyphs.add(sans.program(), "\u{141}").unwrap();
        Graph {
            pdf: &mut pdf,
            res: &mut res,
        }
        .fallback(&glyphs);
        res.finish(&mut pdf);
        assert_eq!(pdf.objects.len(), count, "the same numbers");
        assert_ne!(pdf.get_dictionary(at).unwrap(), &empty);
    }

    /// An owner dropped with the fallback bound and its subset never
    /// written is a reference left dangling: a debug build says so.
    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "its owner didn't finish")]
    fn an_unfinished_owner_panics_in_a_debug_build() {
        let mut pdf = Pdf::with_version("1.7");
        let mut res = ResObjects::default();
        res.object(&mut pdf, &Fallback::SANS.res());
        drop(res);
    }
}
