//! PDFs sheaf builds in memory (M10.5 §11.7, M18 slices 10–12), each a
//! `sheaf_doc::pdf::Document` graph numbered as it's built and written whole
//! by [`write_built`](super::full::write_built).

use std::collections::HashMap;

use anyhow::{Context as _, Result};

use crate::annot::{Annot, AnnotKind};
#[cfg(any(test, feature = "fixtures"))]
use crate::annot::{Emitted, emission};
use crate::content::sink::{self, Sink};
use crate::content::{Need, Res, ResBody, ResId, ResKind, Reserved, ResourceNeeds};
use crate::geom::{Affine, Rect};
use crate::materialize::copy::copy_object;
use crate::model::{FormRef, Mark};
use crate::pdf::annots::SynthesizedAp;
use crate::pdf::{
    Dictionary, Document as Pdf, Object, ObjectId, Stream, StreamExt, dictionary, resolve_fn,
};
use crate::source::Limits;
use crate::store::{BlobId, BlobRef};

#[derive(Debug, Default)]
pub struct ResObjects {
    written: HashMap<ResId, ObjectId>,
}

impl ResObjects {
    pub fn object(&mut self, pdf: &mut Pdf, res: &Res) -> ObjectId {
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

    pub fn bind(&mut self, pdf: &mut Pdf, needs: &ResourceNeeds, resources: &mut Dictionary) {
        crate::content::sink::bind(&mut Graph { pdf, res: self }, needs, resources);
    }
}

fn add(pdf: &mut Pdf, objects: &[Object]) -> ObjectId {
    let ids: Vec<ObjectId> = objects.iter().map(|_| pdf.new_object_id()).collect();
    for (object, &id) in objects.iter().zip(&ids) {
        let mut object = object.clone();
        object.renumber(|old| Some(ids[old.number as usize]));
        if let Some(stream) = object.as_stream_mut() {
            stream.compress();
        }
        pdf.objects.insert(id, object);
    }
    ids[0]
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

    fn reserved(&mut self, resources: Option<&Object>) -> Reserved {
        let objects = resolve_fn(|id| self.pdf.objects.get(&id));
        Reserved::read(objects, resources, &crate::content::sink::naming_limits())
    }
}

fn as_form(drawn: SynthesizedAp, pdf: &mut Pdf, res: &mut ResObjects) -> ObjectId {
    let mut graph = Graph { pdf, res };
    let form = sink::form(
        &mut graph,
        drawn.bbox,
        drawn.matrix,
        drawn.content,
        &drawn.needs,
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
                let drawn = crate::annot::appearance(&ap.inputs.annot(), page);
                let (bbox, matrix) = (Rect::from(drawn.bbox), Affine::from(drawn.matrix));
                let id = as_form(drawn, pdf, &mut self.res);
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

    pub fn form(&mut self, appearance: SynthesizedAp, pdf: &mut Pdf) -> ObjectId {
        as_form(appearance, pdf, &mut self.res)
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

#[cfg(any(test, feature = "fixtures"))]
pub fn to_pdf(a: &Annot, pdf: &mut Pdf) -> Written {
    write(a, None, None, pdf, &mut WriteCx::new())
}

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

    pub fn page(&mut self, media: Rect, content: Vec<u8>, needs: &ResourceNeeds) {
        let mut content = Stream::new(dictionary! {}, content);
        content.compress();
        let content = self.pdf.add_object(content);
        let mut resources = Dictionary::new();
        self.res.bind(&mut self.pdf, needs, &mut resources);
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
        self.page(media, b"/Stamp Do".to_vec(), &needs);
    }

    pub fn finish(mut self) -> Result<Vec<u8>> {
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
    use crate::content::{ContentWriter, DrawText, Std14};

    #[test]
    fn built_pages_share_their_fonts_and_read_back() {
        let made = || {
            let mut built = Built::new();
            for line in ["one", "two"] {
                let mut w = ContentWriter::new();
                w.begin_text();
                w.set_font(&Std14::Helvetica, 12.0);
                w.text_move(72.0, 700.0);
                w.show_text(&Std14::Helvetica, line);
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
