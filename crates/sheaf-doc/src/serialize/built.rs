//! PDFs sheaf builds in memory (M10.5 §11.7), each a lopdf graph numbered
//! as it's built and written whole by [`write_built`]: the comments a data
//! file holds, or a document's gathered to go out (`exchange::Comments`),
//! the comment summary's sheets, a stamp's page back from its form, a
//! flattened mark's page, a tape's picture, and the files tests build.
//!
//! - [`ResObjects`]: the resources content draws with, each written once a
//!   file and bound in the resources of what draws with it.
//! - [`WriteCx`]: marks written as the writer makes them
//!   ([`emission`](crate::annot::emission)), what they draw with in common,
//!   and the stamps' pages and attachments' files they hold ([`Held`]),
//!   each once a file.
//! - [`Built`]: a file of pages, each its box, its content and what that
//!   draws with.
//!
//! [`write_built`]: super::full::write_built

use std::collections::HashMap;

use anyhow::{Context as _, Result};
use lopdf::{Dictionary, Document as Pdf, Object, ObjectId, Stream, dictionary};

use crate::annot::{Annot, AnnotKind, Appearance};
#[cfg(any(test, feature = "fixtures"))]
use crate::annot::{Emitted, emission};
use crate::content::sink::Sink;
use crate::content::{Need, Res, ResBody, ResId, ResKind, Reserved, ResourceNeeds};
use crate::geom::{Affine, Rect};
use crate::materialize::copy::copy_object;
use crate::model::{FormRef, Mark};
use crate::source::Limits;
use crate::store::{BlobId, BlobRef};

/// The objects a file built in memory has written for resources, by
/// [`ResId`]: a font, an ExtGState or a property list that many pages or
/// marks draw with is one object a file. For one file only.
#[derive(Debug, Default)]
pub struct ResObjects {
    written: HashMap<ResId, ObjectId>,
}

impl ResObjects {
    /// `res`'s object in `pdf`, written the first time it's asked for.
    pub fn object(&mut self, pdf: &mut Pdf, res: &Res) -> ObjectId {
        match &res.body {
            ResBody::Written(id) => *id,
            ResBody::Objects(objects) => *self
                .written
                .entry(res.id)
                .or_insert_with(|| add(pdf, objects)),
        }
    }

    /// `needs` written into `pdf` and bound in `resources`, a resources
    /// dict about to be written: each under its name in its category. A
    /// category that's a reference is copied in first, so a dict others
    /// share is never changed.
    pub fn bind(&mut self, pdf: &mut Pdf, needs: &ResourceNeeds, resources: &mut Dictionary) {
        crate::content::sink::bind(&mut Graph { pdf, res: self }, needs, resources);
    }
}

/// `objects`, a resource's ([`ResBody::Objects`]), added to `pdf`, each
/// with a number of its own and its streams compressed; the first's
/// number.
fn add(pdf: &mut Pdf, objects: &[Object]) -> ObjectId {
    let ids: Vec<ObjectId> = objects.iter().map(|_| pdf.new_object_id()).collect();
    for (object, &id) in objects.iter().zip(&ids) {
        let mut object = object.clone();
        renumber(&mut object, &ids);
        if let Object::Stream(stream) = &mut object {
            let _ = stream.compress();
        }
        pdf.objects.insert(id, object);
    }
    ids[0]
}

/// `object` with each reference `(i, 0)` made `ids[i]`.
fn renumber(object: &mut Object, ids: &[ObjectId]) {
    match object {
        Object::Reference(id) => *id = ids[id.0 as usize],
        Object::Array(items) => items.iter_mut().for_each(|o| renumber(o, ids)),
        Object::Dictionary(dict) => dict.iter_mut().for_each(|(_, o)| renumber(o, ids)),
        Object::Stream(stream) => stream.dict.iter_mut().for_each(|(_, o)| renumber(o, ids)),
        _ => {}
    }
}

/// A file being built and the resources it has written: what an
/// appearance's resources are written through.
struct Graph<'a> {
    pdf: &'a mut Pdf,
    res: &'a mut ResObjects,
}

impl Sink for Graph<'_> {
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

/// `appearance` as a form in `pdf`, what it draws with written through
/// `res`, once a file each.
fn as_form(appearance: Appearance, pdf: &mut Pdf, res: &mut ResObjects) -> ObjectId {
    let form = appearance.form(&mut Graph { pdf, res });
    pdf.add_object(form)
}

/// A stamp's page as a form in the file: its number (or the emitter's
/// placeholder for it), what the content that draws it names it by,
/// `/BBox` and `/Matrix`.
#[derive(Debug, Clone, Copy)]
pub struct StampForm {
    pub id: ObjectId,
    /// Its number's digest ([`Res::written`]), or in a pinned appearance
    /// its form's, which no save's numbering moves.
    pub res: ResId,
    pub bbox: Rect,
    pub matrix: Affine,
}

impl StampForm {
    /// Form `id`, named by its number.
    pub fn written(id: ObjectId, bbox: Rect, matrix: Affine) -> StampForm {
        StampForm {
            id,
            res: Res::written(id).id,
            bbox,
            matrix,
        }
    }

    /// The resource content draws it as: its object, by its name.
    pub fn resource(&self) -> Res {
        Res {
            id: self.res,
            body: ResBody::Written(self.id),
        }
    }
}

/// What a mark draws and embeds that isn't its own, where the file being
/// written holds it: a page stamp's page as a form, an attachment's
/// embedded file stream. None draws a stamp without its page and writes
/// an attachment without its file.
#[derive(Debug, Clone, Copy, Default)]
pub struct Held {
    pub stamp: Option<StampForm>,
    pub file: Option<ObjectId>,
}

/// What the marks written into one PDF built in memory share: the fonts
/// and ExtGStates their appearances draw with, each one object a file,
/// and the stamps' pages and attachments' files the marks hold, each
/// written once.
#[derive(Default)]
pub struct WriteCx {
    res: ResObjects,
    /// Each stamp's page as a form in the file, by the form a mark holds:
    /// None when it can't be had.
    stamps: HashMap<FormRef, Option<StampForm>>,
    /// Each attachment's file in the file, by its blob.
    files: HashMap<BlobId, ObjectId>,
    /// What's been copied from each source: its objects' numbers in the
    /// file.
    copied: HashMap<BlobId, HashMap<ObjectId, ObjectId>>,
}

impl WriteCx {
    /// For marks written into a file of their own.
    pub fn new() -> WriteCx {
        WriteCx::default()
    }

    /// Stamp `stem`'s page, the library's `name`, held as `form`, as a form
    /// in `pdf`: copied from the source that holds it, made a form from
    /// its one-page PDF, or drawn from what it's pinned to. None, said
    /// once, when it can't be had.
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
                let drawn = crate::annot::appearance(&ap.inputs.annot(), page);
                let bbox = drawn.bbox();
                let id = as_form(drawn, pdf, &mut self.res);
                Ok(StampForm::written(id, bbox, Affine::IDENTITY))
            }
        };
        let made = made
            .inspect_err(|e| log::warn!("stamp {name} ({stem}) is drawn without its page: {e:#}"))
            .ok();
        self.stamps.insert(form.clone(), made);
        made
    }

    /// What mark `m` holds, in `pdf`: its stamp's page as a form, its
    /// file as an embedded file stream, each written once. What it can't
    /// have is said, and it's drawn without it.
    pub fn held(&mut self, m: &Mark, pdf: &mut Pdf) -> Held {
        held(&m.annot, m.stamp.as_ref(), m.file.as_ref(), pdf, self)
    }

    /// `appearance` as a form in `pdf`, what it draws with written once.
    pub fn form(&mut self, appearance: Appearance, pdf: &mut Pdf) -> ObjectId {
        as_form(appearance, pdf, &mut self.res)
    }

    /// The attachment's file `file` as an embedded file stream in `pdf`,
    /// made from its bytes once.
    fn file(&mut self, file: &BlobRef, pdf: &mut Pdf) -> ObjectId {
        *self.files.entry(file.id()).or_insert_with(|| {
            let mut stream = Stream::new(
                dictionary! {
                    "Type" => "EmbeddedFile",
                    "Params" => dictionary! { "Size" => file.bytes().len() as i64 },
                },
                file.bytes().to_vec(),
            );
            let _ = stream.compress();
            pdf.add_object(Object::Stream(stream))
        })
    }
}

/// What `a` holds, as `stamp` and `file`, written into `pdf` through
/// `cx` where `a` names them: its stamp's page as a form, its file as an
/// embedded file stream. What it can't have is said.
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

/// A mark as PDF objects: its annotation dictionary, with `/AP` set, and
/// any companions. Objects they point at (appearance streams, images,
/// embedded files) are already in the document. The writer gives each
/// companion `/IRT` to the main annotation and `/RT /Group`.
#[cfg(any(test, feature = "fixtures"))]
pub struct Written {
    pub main: Dictionary,
    pub companions: Vec<Dictionary>,
}

/// Write `a`, a mark that holds no stamp's page or file, into `pdf`. Its
/// geometry is PageSpace, which is the page's user space, so it needs
/// nothing of the page. The caller adds [`Written::main`] (and the
/// companions) to the page's `/Annots`. A page stamp is drawn without its
/// page, an attachment without its file. What tests build their files
/// with.
#[cfg(any(test, feature = "fixtures"))]
pub fn to_pdf(a: &Annot, pdf: &mut Pdf) -> Written {
    write(a, None, None, pdf, &mut WriteCx::new())
}

/// [`to_pdf`], for one of the marks written into `pdf` through `cx`, with
/// what it holds: what they draw with in common is in the file once, and
/// so is each stamp's page and file.
#[cfg(any(test, feature = "fixtures"))]
pub fn to_pdf_in(m: &Mark, pdf: &mut Pdf, cx: &mut WriteCx) -> Written {
    write(&m.annot, m.stamp.as_ref(), m.file.as_ref(), pdf, cx)
}

/// `a` written into `pdf` through `cx`, drawing the page `stamp` and
/// embedding `file` where it names them.
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

/// A PDF of pages built in memory: its page tree numbered first, then
/// what the caller puts in its graph ([`pdf`](Self::pdf)) and the pages
/// as they're added, each page's content compressed and what that draws
/// with written once a file. The same pages make the same bytes.
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
    /// `pdf`, a graph made in memory (a data file's, read), to add pages
    /// to: its page tree numbered past what it has.
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
    /// A file of no pages yet.
    pub fn new() -> Built {
        Built::default()
    }

    /// Its graph, for what its pages draw that's put in by hand: a page or
    /// a form copied in, a mark written.
    pub fn pdf(&mut self) -> &mut Pdf {
        &mut self.pdf
    }

    /// A page of `media`, drawing `content` with what `needs` names.
    pub fn page(&mut self, media: Rect, content: Vec<u8>, needs: &ResourceNeeds) {
        let mut content = Stream::new(dictionary! {}, content);
        let _ = content.compress();
        let content = self.pdf.add_object(content);
        let mut resources = Dictionary::new();
        self.res.bind(&mut self.pdf, needs, &mut resources);
        let media = [media.x0, media.y0, media.x1, media.y1].map(Object::from);
        let page = self.pdf.add_object(dictionary! {
            "Type" => "Page", "Parent" => self.tree, "Contents" => content,
            "MediaBox" => media.to_vec(), "Resources" => resources,
        });
        self.kids.push(page.into());
    }

    /// A page that draws `form`, the box its `/Matrix` makes of its
    /// `/BBox`, which is what it draws: a stamp's page, back from its form.
    pub fn form_page(&mut self, form: StampForm) {
        let needs = ResourceNeeds(vec![Need {
            kind: ResKind::XObject,
            name: b"Stamp".to_vec(),
            res: form.resource(),
        }]);
        let media = form.matrix.bounds(form.bbox);
        self.page(media, b"/Stamp Do".to_vec(), &needs);
    }

    /// The file: its page tree and catalog, written through sheaf's
    /// serializer.
    pub fn finish(mut self) -> Result<Vec<u8>> {
        let count = self.kids.len() as i64;
        self.pdf.objects.insert(
            self.tree,
            Object::Dictionary(dictionary! {
                "Type" => "Pages", "Count" => count, "Kids" => self.kids,
            }),
        );
        let catalog = self.pdf.add_object(dictionary! {
            "Type" => "Catalog", "Pages" => self.tree,
        });
        self.pdf.trailer.set("Root", catalog);
        super::full::write_built(&self.pdf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::{ContentWriter, Std14};

    /// Pages built in memory share what they draw with: two pages in one
    /// font hold one font object, the file reads back as a source of its
    /// pages, and the same pages make the same bytes.
    #[test]
    fn built_pages_share_their_fonts_and_read_back() {
        let made = || {
            let mut built = Built::new();
            for line in ["one", "two"] {
                let mut w = ContentWriter::new();
                w.begin_text();
                w.font(&Std14::Helvetica, 12.0);
                w.text_move(72.0, 700.0);
                w.text(line);
                w.end_text();
                let (content, needs) = w.finish(&Reserved::default());
                built.page(Rect::new(0.0, 0.0, 612.0, 792.0), content, &needs);
            }
            built.finish().unwrap()
        };
        let bytes = made();
        assert_eq!(made(), bytes);
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
}
