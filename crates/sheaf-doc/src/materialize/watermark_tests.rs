//! Watermarks and backgrounds as a save draws them (M10.5 slice 16): a
//! block of its own for each distinct layout, marked with its generator,
//! the page's own content isolated by its balance, the page's resources
//! one dictionary for each base page's, the catalog's `/SheafGenerators`,
//! the PDF version raised, and an Append of what changed.

use std::collections::{BTreeMap, BTreeSet};

use lopdf::xref::XrefType;
use lopdf::{Dictionary, Document as Pdf, Object, ObjectId, Stream, dictionary};
use sheaf_edit::{BlobSel, GridPos, Turn, WatermarkContent, WatermarkSpec, Z};

use super::append_tests::{appended_in_place, file, keys_at, planned_in_place, written};
use super::emit::PdfVersion;
use super::emit::content::needs_after;
use super::key::ObjKey;
use super::removal;
use super::removal_tests::{layer, opened, scratch, text_file};
use super::{How, StrategyKind, save_rewritten};
use crate::content::{Balance, Concat, GenId, ResCx};
use crate::model::{Document, PageId};
use crate::serialize::fixture::holds;
use crate::source::Limits;
use crate::store::{Blob, BlobKind};
use crate::tx::fixture as fx;

/// A watermark of `text`, over every page or `pages`.
fn watermark(text: &str, pages: Option<&[PageId]>) -> sheaf_edit::Op {
    fx::set_watermark(fx::watermark(text), Z::Over, pages)
}

/// A file of one page for each of `contents`, each its content; pages
/// `0..split` under a node whose resources are one dictionary, the rest
/// under another with its own (one node when `split` is all of them).
fn pages_file(contents: &[&[u8]], split: usize) -> Vec<u8> {
    let mut pdf = Pdf::with_version("1.7");
    let root = pdf.new_object_id();
    let font = pdf.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
    });
    let groups: Vec<&[&[u8]]> = match split < contents.len() {
        true => vec![&contents[..split], &contents[split..]],
        false => vec![contents],
    };
    let mut nodes = Vec::new();
    for (g, group) in groups.into_iter().enumerate() {
        let node = pdf.new_object_id();
        let resources = pdf.add_object(dictionary! {
            "Font" => dictionary! { "F1" => font },
            "ProcSet" => vec![Object::Name(b"PDF".to_vec()), Object::Integer(g as i64)],
        });
        let kids: Vec<Object> = (group.iter())
            .map(|content| {
                let content = pdf.add_object(Stream::new(dictionary! {}, content.to_vec()));
                let page =
                    dictionary! { "Type" => "Page", "Parent" => node, "Contents" => content };
                pdf.add_object(page).into()
            })
            .collect();
        let count = kids.len() as i64;
        pdf.objects.insert(
            node,
            Object::Dictionary(dictionary! {
                "Type" => "Pages", "Parent" => root, "Kids" => kids, "Count" => count,
                "Resources" => resources,
            }),
        );
        nodes.push(Object::Reference(node));
    }
    pdf.objects.insert(
        root,
        Object::Dictionary(dictionary! {
            "Type" => "Pages", "Kids" => nodes, "Count" => contents.len() as i64,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        }),
    );
    let catalog = pdf.add_object(dictionary! { "Type" => "Catalog", "Pages" => root });
    pdf.trailer.set("Root", catalog);
    let mut bytes = Vec::new();
    pdf.save_to(&mut bytes).unwrap();
    bytes
}

/// What a page of `pdf` names in its `/Contents`, in order.
fn contents(pdf: &Pdf, page: ObjectId) -> Vec<ObjectId> {
    let page = pdf.get_dictionary(page).unwrap();
    let entries = match page.get(b"Contents").unwrap() {
        Object::Array(a) => a.clone(),
        Object::Reference(r) => match pdf.get_object(*r).unwrap() {
            Object::Array(a) => a.clone(),
            _ => vec![Object::Reference(*r)],
        },
        other => panic!("contents {other:?}"),
    };
    entries.iter().map(|e| e.as_reference().unwrap()).collect()
}

/// Stream `id` of `pdf`, its dictionary and its data decoded.
fn stream(pdf: &Pdf, id: ObjectId) -> (Dictionary, Vec<u8>) {
    let s = pdf.get_object(id).unwrap().as_stream().unwrap();
    let data = s
        .decompressed_content()
        .unwrap_or_else(|_| s.content.clone());
    (s.dict.clone(), data)
}

/// A `/SheafBlock` marker's strings, by key.
type Strings = BTreeMap<Vec<u8>, Vec<u8>>;

/// What `/SheafBlock` marks stream `id` as: its `/K`, and the rest by key.
fn marker(pdf: &Pdf, id: ObjectId) -> Option<(Vec<u8>, Strings)> {
    let (dict, _) = stream(pdf, id);
    let marker = dict.get(b"SheafBlock").ok()?.as_dict().ok()?;
    let kind = marker.get(b"K").unwrap().as_name().unwrap().to_vec();
    let strings = (marker.iter())
        .filter(|(k, _)| k.as_slice() != b"K")
        .map(|(k, v)| (k.clone(), v.as_str().expect("a string").to_vec()))
        .collect();
    Some((kind, strings))
}

/// Every page of `pdf`, in order.
fn pages(pdf: &Pdf) -> Vec<ObjectId> {
    pdf.get_pages().into_values().collect()
}

/// The page's resources, as written: what it names, followed.
fn resources(pdf: &Pdf, page: ObjectId) -> (Option<ObjectId>, Dictionary) {
    let page = pdf.get_dictionary(page).unwrap();
    match page.get(b"Resources").unwrap() {
        Object::Reference(r) => (Some(*r), pdf.get_dictionary(*r).unwrap().clone()),
        Object::Dictionary(d) => (None, d.clone()),
        other => panic!("resources {other:?}"),
    }
}

/// The page's `q`/`Q` count through all its content, as PDFium runs it:
/// the lowest it goes, and where it ends.
fn balance(pdf: &Pdf, page: ObjectId) -> Balance {
    let data: Vec<Vec<u8>> = (contents(pdf, page).into_iter())
        .map(|id| stream(pdf, id).1)
        .collect();
    let streams: Vec<&[u8]> = data.iter().map(Vec::as_slice).collect();
    let lim = Limits::DEFAULT;
    crate::content::balance(&Concat::new(&streams), &ResCx::default(), &lim).unwrap()
}

/// A watermark on every page of a file whose pages are alike is one block
/// for them all: after the page's own content, isolated by a marked `q`
/// before it and a marked `Q` after it, its marker's values strings
/// naming its generator, its layout and its recipe, its property list
/// named in the page's `/Properties`, and the catalog's
/// `/SheafGenerators` saying what it is. Both ways.
fn markers(how: How) {
    let dir = scratch();
    let (_, mut doc) = opened(&dir, "a.pdf", &text_file(3));
    doc.commit_ops(vec![watermark("DRAFT", None)], "watermark")
        .unwrap();
    let id = doc.state().watermark(Z::Over).unwrap().id;
    let bytes = how.write(&doc);
    assert_eq!(how.write(&doc), bytes, "{how:?}: the same bytes every time");
    let pdf = Pdf::load_mem(&bytes).unwrap();
    let mut blocks = BTreeSet::new();
    for page in pages(&pdf) {
        let ids = contents(&pdf, page);
        assert_eq!(ids.len(), 4, "{how:?}: {ids:?}");
        let kinds: Vec<Option<Vec<u8>>> = (ids.iter())
            .map(|&id| marker(&pdf, id).map(|(k, _)| k))
            .collect();
        let isolate = Some(b"Isolate".to_vec());
        let want = [isolate.clone(), None, isolate, Some(b"Gen".to_vec())];
        assert_eq!(kinds, want, "{how:?}");
        assert_eq!(stream(&pdf, ids[0]).1, b"q\n");
        assert_eq!(stream(&pdf, ids[2]).1, b"Q\n");
        let (_, strings) = marker(&pdf, ids[3]).unwrap();
        assert_eq!(GenId::parse(&strings[&b"G".to_vec()]), Some(id), "{how:?}");
        assert_eq!(strings[&b"L".to_vec()].len(), 16);
        assert_eq!(strings[&b"R".to_vec()].len(), 16);
        blocks.insert(ids[3]);
        let (_, ops) = stream(&pdf, ids[3]);
        let text = String::from_utf8_lossy(&ops);
        let name = text
            .strip_prefix("/Artifact /")
            .unwrap()
            .split(' ')
            .next()
            .unwrap();
        let (_, res) = resources(&pdf, page);
        let props = res.get(b"Properties").unwrap().as_dict().unwrap();
        let list = props.get(name.as_bytes()).unwrap();
        let list = pdf.dereference(list).unwrap().1.as_dict().unwrap();
        assert_eq!(
            list.get(b"Subtype").unwrap().as_name().unwrap(),
            b"Watermark"
        );
        let gen_id = list.get(b"SheafGen").unwrap().as_str().unwrap();
        assert_eq!(GenId::parse(gen_id), Some(id));
        assert!(res.get(b"Font").unwrap().as_dict().unwrap().has(b"F1"));
    }
    assert_eq!(blocks.len(), 1, "{how:?}: one block");
    let catalog = pdf.catalog().unwrap();
    let json = catalog
        .get(b"SheafGenerators")
        .unwrap()
        .as_reference()
        .unwrap();
    let json = pdf.get_object(json).unwrap().as_str().unwrap();
    let json: serde_json::Value = serde_json::from_slice(json).unwrap();
    assert_eq!(json["generators"][0]["id"], id.to_string());
    assert_eq!(json["generators"][0]["spec"]["content"]["text"], "DRAFT");
}

crate::both_ways!(
    a_watermark_is_one_marked_block_after_isolated_content,
    a_watermark_is_one_marked_block_after_isolated_content_appended: markers
);

/// Pages that share their resources share one dictionary of the page's
/// own, and draw the watermark in one font, one graphics state and one
/// block; two groups of pages, two dictionaries, the same font, state and
/// block.
fn census(how: How) {
    let dir = scratch();
    let content: &[u8] = b"BT /F1 24 Tf 72 72 Td (page) Tj ET";
    for (split, groups) in [(4, 1), (2, 2)] {
        let (_, mut doc) = opened(
            &dir,
            &format!("{split}.pdf"),
            &pages_file(&[content; 4], split),
        );
        doc.commit_ops(vec![watermark("DRAFT", None)], "watermark")
            .unwrap();
        let bytes = how.write(&doc);
        let pdf = Pdf::load_mem(&bytes).unwrap();
        let own: BTreeSet<Option<ObjectId>> = (pages(&pdf).into_iter())
            .map(|p| resources(&pdf, p).0)
            .collect();
        assert_eq!(own.len(), groups, "{how:?}: {split}");
        assert!(own.iter().all(Option::is_some));
        let count = |test: &dyn Fn(&Dictionary) -> bool| {
            (pdf.objects.values())
                .filter(|o| match o {
                    Object::Dictionary(d) => test(d),
                    Object::Stream(s) => test(&s.dict),
                    _ => false,
                })
                .count()
        };
        let bold = |d: &Dictionary| {
            d.get(b"BaseFont").and_then(Object::as_name).ok() == Some(b"Helvetica-Bold")
        };
        assert_eq!(count(&bold), 1, "{how:?}: {split}");
        assert_eq!(count(&|d| d.has(b"ca")), 1, "{how:?}: {split}");
        let block = |d: &Dictionary| {
            (d.get(b"SheafBlock").and_then(Object::as_dict))
                .is_ok_and(|m| m.get(b"K").and_then(Object::as_name).ok() == Some(b"Gen"))
        };
        assert_eq!(count(&block), 1, "{how:?}: {split}");
    }
}

crate::both_ways!(
    pages_that_share_resources_share_what_a_watermark_adds,
    pages_that_share_resources_share_what_a_watermark_adds_appended: census
);

/// A PNG's bytes, 4 by 2.
pub(crate) fn png() -> Vec<u8> {
    let rgb = image::RgbImage::from_pixel(4, 2, image::Rgb([200, 30, 30]));
    sheaf_image::encode::png(&rgb, 72).unwrap()
}

/// `bytes`, an image file, interned in `doc`'s store (held while it's
/// named), and a watermark of it a quarter of the page wide.
pub(crate) fn image_watermark(doc: &Document, bytes: &[u8]) -> (Blob, WatermarkSpec) {
    let blob = (doc.store())
        .intern(bytes.into(), BlobKind::image(bytes).unwrap())
        .unwrap();
    let spec = WatermarkSpec {
        content: WatermarkContent::Image {
            image: Some(BlobSel::Id(blob.id())),
            scale: 0.25,
            opacity: 0.5,
        },
        position: GridPos::Center,
    };
    (blob, spec)
}

/// An image watermark under every page of a 7,000-page document of blank
/// pages adds one image to the file, and one block: both ways.
fn one_image(how: How) {
    const PAGES: usize = 7000;
    let dir = scratch();
    let blank: &[u8] = b"";
    let (_, mut doc) = opened(&dir, "blank.pdf", &pages_file(&[blank; PAGES], PAGES));
    let (_png, spec) = image_watermark(&doc, &png());
    let set = fx::set_watermark(spec, Z::Under, None);
    doc.commit_ops(vec![set], "watermark").unwrap();
    let bytes = how.write(&doc);
    let pdf = Pdf::load_mem(&bytes).unwrap();
    assert_eq!(pdf.get_pages().len(), PAGES);
    let images = (pdf.objects.values())
        .filter(|o| {
            (o.as_stream()).is_ok_and(|s| {
                s.dict.get(b"Subtype").and_then(Object::as_name).ok() == Some(b"Image")
            })
        })
        .count();
    assert_eq!(images, 1, "{how:?}");
    // The background, the page's isolation and its own content.
    let drawn = |page| {
        let mut ids = contents(&pdf, page);
        ids.remove(2);
        ids
    };
    let first = drawn(pages(&pdf)[0]);
    for page in pages(&pdf) {
        assert_eq!(drawn(page), first, "{how:?}");
    }
    let (_, block) = stream(&pdf, first[0]);
    let text = String::from_utf8_lossy(&block);
    assert!(text.contains(" Do\n"), "{text}");
    let (_, res) = resources(&pdf, pages(&pdf)[0]);
    let props = res.get(b"Properties").unwrap().as_dict().unwrap();
    let list = pdf.dereference(props.iter().next().unwrap().1).unwrap().1;
    let subtype = list.as_dict().unwrap().get(b"Subtype").unwrap();
    assert_eq!(subtype.as_name().unwrap(), b"Background");
}

crate::both_ways!(
    an_image_on_every_page_is_one_image,
    an_image_on_every_page_is_one_image_appended: one_image
);

/// The page's own content is isolated by its balance (§3.4): `q … cm`
/// opens 1 and closes 2, `Q q` 2 and 2, `Q` 2 and 1; drawn through, the
/// count never goes below what the page started with and ends where it
/// started, so nothing the page leaves set reaches the watermark.
#[test]
fn isolation_is_counted_from_the_base_s_balance() {
    let dir = scratch();
    let cases: [(&[u8], usize, usize); 3] = [
        (b"q 1 0 0 1 200 200 cm 0 0 1 rg 0 0 50 50 re f", 1, 2),
        (b"Q q 0 0 1 rg 10 10 50 50 re f", 2, 2),
        (b"Q 0 0 1 rg 10 10 50 50 re f", 2, 1),
    ];
    for (base, open, close) in cases {
        let (_, mut doc) = opened(&dir, "a.pdf", &pages_file(&[base], 1));
        doc.commit_ops(vec![watermark("DRAFT", None)], "watermark")
            .unwrap();
        for how in [How::Rewritten, How::Appended] {
            let bytes = how.write(&doc);
            let pdf = Pdf::load_mem(&bytes).unwrap();
            let page = pages(&pdf)[0];
            let ids = contents(&pdf, page);
            let what = String::from_utf8_lossy(base);
            assert_eq!(
                stream(&pdf, ids[0]).1,
                b"q\n".repeat(open),
                "{what} {how:?}"
            );
            assert_eq!(
                stream(&pdf, ids[2]).1,
                b"Q\n".repeat(close),
                "{what} {how:?}"
            );
            let drawn = balance(&pdf, page);
            assert_eq!(
                drawn,
                Balance {
                    net: 0,
                    min_prefix: 0
                },
                "{what} {how:?}"
            );
        }
    }
}

/// A page with an OCR layer sheaf wrote before M10.5
/// (`text_layer::fixture::legacy_page`), given new text and a watermark:
/// the old layer is set aside with its `q`, so the isolation counts the
/// page's own content alone, which leaves a `q` open, and the new layer's
/// block goes after it, before the watermark's. Both ways.
fn legacy_ocr(how: How) {
    let dir = scratch();
    let mut bytes = Vec::new();
    crate::text_layer::fixture::legacy_page()
        .save_to(&mut bytes)
        .unwrap();
    let (_, mut doc) = opened(&dir, "a.pdf", &bytes);
    let page = doc.page(0).unwrap();
    let mut ops = fx::ocr([(page, layer("after"))]);
    ops.push(watermark("DRAFT", None));
    doc.commit_ops(ops, "ocr and watermark").unwrap();
    let bytes = how.write(&doc);
    let pdf = Pdf::load_mem(&bytes).unwrap();
    let page = pages(&pdf)[0];
    let ids = contents(&pdf, page);
    let old_layer = |id: ObjectId| {
        let marker = crate::text_layer::fixture::LEGACY_MARKER;
        stream(&pdf, id).0.has(marker)
    };
    assert!(!ids.iter().any(|&id| old_layer(id)), "{how:?}");
    let kinds: Vec<Option<Vec<u8>>> = (ids.iter())
        .map(|&id| marker(&pdf, id).map(|(k, _)| k))
        .collect();
    let isolate = Some(b"Isolate".to_vec());
    let want = [
        isolate.clone(),
        None,
        isolate,
        Some(b"Ocr".to_vec()),
        Some(b"Gen".to_vec()),
    ];
    assert_eq!(kinds, want, "{how:?}");
    assert_eq!(stream(&pdf, ids[0]).1, b"q\n");
    assert_eq!(stream(&pdf, ids[1]).1, crate::text_layer::fixture::BASE);
    assert_eq!(stream(&pdf, ids[2]).1, b"Q\nQ\n");
    assert_eq!(balance(&pdf, page), Balance::default(), "{how:?}");
}

crate::both_ways!(
    an_old_ocr_layer_replaced_is_not_isolated,
    an_old_ocr_layer_replaced_is_not_isolated_appended: legacy_ocr
);

/// Text is drawn in its face's WinAnsi, "Café" one byte a character, and
/// a character it hasn't got, as CJK, `?`: the block holds no other
/// bytes for them. Both ways.
fn encoded(how: How) {
    let dir = scratch();
    for (text, drawn) in [("Café", &b"(Caf\xe9) Tj"[..]), ("漢字", b"(??) Tj")] {
        let (_, mut doc) = opened(&dir, "a.pdf", &text_file(1));
        doc.commit_ops(vec![watermark(text, None)], "watermark")
            .unwrap();
        let bytes = how.write(&doc);
        let pdf = Pdf::load_mem(&bytes).unwrap();
        let ids = contents(&pdf, pages(&pdf)[0]);
        let (_, block) = stream(&pdf, ids[3]);
        assert!(
            block.windows(drawn.len()).any(|w| w == drawn),
            "{how:?}: {}",
            String::from_utf8_lossy(&block)
        );
    }
}

crate::both_ways!(
    text_is_drawn_in_its_face_s_encoding,
    text_is_drawn_in_its_face_s_encoding_appended: encoded
);

/// The keys the section a save of `doc` appended to its file wrote, by
/// kind: how many pages, blocks, isolating streams, page resources,
/// resources, and whether the generators and the catalog.
pub(super) fn kinds(keys: &BTreeSet<ObjKey>) -> BTreeMap<&'static str, usize> {
    let mut out = BTreeMap::new();
    for key in keys {
        let kind = match key {
            ObjKey::Page(_) => "page",
            ObjKey::Block(_) => "block",
            ObjKey::Shared(_) => "isolation",
            ObjKey::PageRes { .. } => "page resources",
            ObjKey::Res(_) => "resource",
            ObjKey::Generators => "generators",
            ObjKey::Catalog => "catalog",
            other => panic!("{other:?}"),
        };
        *out.entry(kind).or_default() += 1;
    }
    out
}

/// A watermark added to a saved file appends its pages' dictionaries, the
/// block, the isolating streams, the pages' resources and what they add,
/// what the generators are and the catalog that names them. Turning a
/// page appends its dictionary and its new block, and the same watermark
/// on one more page appends that page and what the generators are.
#[test]
fn a_watermark_appends_what_it_draws_and_a_turn_its_page_and_block() {
    let dir = scratch();
    let (path, mut doc) = opened(&dir, "a.pdf", &text_file(3));
    let p0 = doc.page(0).unwrap();
    doc.commit_ops(vec![watermark("DRAFT", None)], "watermark")
        .unwrap();
    let (base, section) = planned_in_place(&mut doc);
    let keys = keys_at(&doc, &written(base.len(), &section, &path));
    let want = BTreeMap::from([
        ("page", 3),
        ("block", 1),
        ("isolation", 2),
        ("page resources", 1),
        ("resource", 3),
        ("generators", 1),
        ("catalog", 1),
    ]);
    assert_eq!(kinds(&keys), want);

    doc.commit_ops(vec![fx::rotate(&[p0], Turn::Quarter)], "turn")
        .unwrap();
    let (base, section) = planned_in_place(&mut doc);
    let keys = keys_at(&doc, &written(base.len(), &section, &path));
    assert_eq!(kinds(&keys), BTreeMap::from([("page", 1), ("block", 1)]));
    assert!(keys.contains(&ObjKey::Page(p0)));

    let (path, mut doc) = opened(&dir, "b.pdf", &text_file(3));
    let [p0, p1] = [0, 1].map(|i| doc.page(i).unwrap());
    doc.commit_ops(vec![watermark("DRAFT", Some(&[p0]))], "watermark")
        .unwrap();
    planned_in_place(&mut doc);
    doc.commit_ops(vec![watermark("DRAFT", Some(&[p0, p1]))], "watermark")
        .unwrap();
    let (base, section) = planned_in_place(&mut doc);
    let keys = keys_at(&doc, &written(base.len(), &section, &path));
    assert_eq!(keys, BTreeSet::from([ObjKey::Page(p1), ObjKey::Generators]));
    assert_eq!(removal::vetoes_here().derived, 0);
}

/// A PDF 1.3 file watermarked is written as 1.4 by a Rewrite, and an
/// Append onto it says `/Version /1.4` in the catalog it writes.
#[test]
fn a_watermark_raises_the_version_to_1_4() {
    let dir = scratch();
    let bytes = file(&[2], XrefType::CrossReferenceTable, "1.3");
    let (path, mut doc) = opened(&dir, "a.pdf", &bytes);
    assert!(!needs_after(doc.state(), PdfVersion::new(1, 3)));
    doc.commit_ops(vec![watermark("DRAFT", None)], "watermark")
        .unwrap();
    assert!(needs_after(doc.state(), PdfVersion::new(1, 3)));
    assert!(!needs_after(doc.state(), PdfVersion::ARTIFACTS));
    let rewritten = How::Rewritten.write(&doc);
    assert!(rewritten.starts_with(b"%PDF-1.4"));
    let catalog = doc.saved_record().unwrap().map.keys[&ObjKey::Catalog];
    let (base, section) = appended_in_place(&mut doc).unwrap();
    assert!(written(base.len(), &section, &path).contains(&catalog));
    let pdf = Pdf::load(&path).unwrap();
    let version = pdf.catalog().unwrap().get(b"Version").unwrap();
    assert_eq!(version.as_name().unwrap(), b"1.4");
    let record = doc.saved_record().unwrap();
    assert_eq!(record.pdf_version, PdfVersion::ARTIFACTS);
}

/// A page whose content can't be read gets no watermark, and a note says
/// which; the save goes on, the other pages watermarked.
#[test]
fn a_page_that_cant_be_read_is_saved_without_its_watermark() {
    let dir = scratch();
    let good: &[u8] = b"0 0 1 rg 10 10 50 50 re f";
    let bad: &[u8] = b"0 0 1 rg 10 10 50 50 re f EMC";
    for how in [How::Rewritten, How::Appended] {
        let (path, mut doc) = opened(&dir, "a.pdf", &pages_file(&[good, bad], 2));
        doc.commit_ops(vec![watermark("DRAFT", None)], "watermark")
            .unwrap();
        let report = how.save(&mut doc, &path);
        let notes: Vec<String> = report.notes.iter().map(|n| n.to_string()).collect();
        let note = "No recognized text, watermark, background, header or footer is drawn on \
                    page 2, whose content can't be read.";
        assert!(notes.iter().any(|n| n == note), "{how:?}: {notes:?}");
        let pdf = Pdf::load(&path).unwrap();
        let [first, second] = pages(&pdf)[..] else {
            panic!("two pages");
        };
        assert_eq!(contents(&pdf, first).len(), 4, "{how:?}");
        assert_eq!(contents(&pdf, second).len(), 1, "{how:?}");
    }
}

/// A watermark's text changed is text the file held that the document no
/// longer does: the save is a Rewrite. Moved, it's the same text: an
/// Append, the old block judged on what the watermark drew.
#[test]
fn a_changed_text_rewrites_and_a_moved_watermark_appends() {
    let dir = scratch();
    let (path, mut doc) = opened(&dir, "a.pdf", &text_file(2));
    doc.commit_ops(vec![watermark("DRAFT", None)], "watermark")
        .unwrap();
    planned_in_place(&mut doc);
    let moved = WatermarkSpec {
        position: GridPos::TopLeft,
        ..fx::watermark("DRAFT")
    };
    doc.commit_ops(vec![fx::set_watermark(moved, Z::Over, None)], "move")
        .unwrap();
    let before = removal::vetoes_here();
    let (base, section) = planned_in_place(&mut doc);
    let keys = keys_at(&doc, &written(base.len(), &section, &path));
    assert_eq!(
        kinds(&keys),
        BTreeMap::from([("page", 2), ("block", 1), ("generators", 1)])
    );
    assert_eq!(removal::vetoes_here(), before);
    doc.commit_ops(vec![watermark("FINAL", None)], "change")
        .unwrap();
    let report = super::save(
        &mut doc,
        &path,
        &Default::default(),
        &in_progress::NoProgress,
    )
    .unwrap();
    assert_eq!(report.strategy, Some(StrategyKind::Rewrite));
}

/// An Append that writes neither the catalog nor `/SheafGenerators`, a
/// comment's on a page the background isn't on, makes nothing of the
/// image the background draws: what the generators are, which needs its
/// digest, is made only when it's written. The background moved, it is.
#[test]
fn an_append_that_draws_nothing_makes_no_image() {
    use super::emit::content::ARTS_MADE;
    let dir = scratch();
    let (path, mut doc) = opened(&dir, "a.pdf", &text_file(3));
    let [p0, p2] = [0, 2].map(|i| doc.page(i).unwrap());
    let (_png, spec) = image_watermark(&doc, &png());
    let set = fx::set_watermark(spec.clone(), Z::Under, Some(&[p0]));
    doc.commit_ops(vec![set], "background").unwrap();
    save_rewritten(&mut doc, &path).unwrap();
    let made = ARTS_MADE.get();
    let square = super::append_tests::square(1);
    doc.commit_ops(vec![fx::add(p2, &square)], "comment")
        .unwrap();
    let (base, section) = planned_in_place(&mut doc);
    let keys = keys_at(&doc, &written(base.len(), &section, &path));
    assert!(keys.contains(&ObjKey::Page(p2)), "{keys:?}");
    assert!(!keys.contains(&ObjKey::Generators) && !keys.contains(&ObjKey::Catalog));
    assert_eq!(ARTS_MADE.get() - made, 0);
    let moved = WatermarkSpec {
        position: GridPos::TopLeft,
        ..spec
    };
    let set = fx::set_watermark(moved, Z::Under, Some(&[p0]));
    doc.commit_ops(vec![set], "move").unwrap();
    let (base, section) = planned_in_place(&mut doc);
    let keys = keys_at(&doc, &written(base.len(), &section, &path));
    assert!(keys.contains(&ObjKey::Generators), "{keys:?}");
    assert_eq!(ARTS_MADE.get() - made, 1);
}

/// An encrypted file watermarked saves encrypted: what the generators are
/// and its blocks' markers are strings the file's key encrypts, which its
/// password reads back. Both ways.
fn encrypted(how: How) {
    use crate::serialize::fixture::{USER, encrypted, form, rc4_aes_128_and_aes_256};
    let dir = scratch();
    let plain = form();
    let state = &rc4_aes_128_and_aes_256(&plain)[1].1;
    let [bytes, _] = encrypted(&plain, state, |_| {});
    let path = dir.join("enc.pdf");
    std::fs::write(&path, bytes).unwrap();
    let mut doc = crate::open::file(&path, Some(USER)).unwrap().doc;
    doc.commit_ops(vec![watermark("MARKSECRET", None)], "watermark")
        .unwrap();
    let id = doc.state().watermark(Z::Over).unwrap().id;
    let out = how.write(&doc);
    assert!(!holds(&out, b"MARKSECRET"), "{how:?}");
    assert!(!holds(&out, id.to_string().as_bytes()), "{how:?}");
    let lim = &Limits::DEFAULT.reader;
    let pdf = lopdf::LazyPdf::open(std::sync::Arc::<[u8]>::from(out), Some(USER), lim).unwrap();
    let json = pdf.catalog().unwrap().get(b"SheafGenerators").unwrap();
    let json = pdf.dereference(json).unwrap().1.as_str().unwrap().to_vec();
    assert!(
        String::from_utf8(json).unwrap().contains("MARKSECRET"),
        "{how:?}"
    );
    let root = pdf
        .catalog()
        .unwrap()
        .get(b"Pages")
        .unwrap()
        .as_reference()
        .unwrap();
    let kids = pdf.get_dictionary(root).unwrap().get(b"Kids").unwrap();
    let first = kids.as_array().unwrap()[0].as_reference().unwrap();
    let page = pdf.get_dictionary(first).unwrap();
    let last = page
        .get(b"Contents")
        .unwrap()
        .as_array()
        .unwrap()
        .last()
        .unwrap();
    let block = pdf
        .get(last.as_reference().unwrap())
        .unwrap()
        .as_stream()
        .unwrap();
    let marker = block.dict.get(b"SheafBlock").unwrap().as_dict().unwrap();
    assert_eq!(
        GenId::parse(marker.get(b"G").unwrap().as_str().unwrap()),
        Some(id)
    );
}

crate::both_ways!(
    an_encrypted_file_s_watermark_is_encrypted,
    an_encrypted_file_s_watermark_is_encrypted_appended: encrypted
);

/// The last watermark removed and the document appended anyway: the
/// catalog it writes names no generators, and the pages it was on are
/// their own content again.
#[test]
fn the_last_watermark_removed_leaves_no_generators() {
    let dir = scratch();
    let (path, mut doc) = opened(&dir, "a.pdf", &text_file(2));
    let pages = [0, 1].map(|i| doc.page(i).unwrap());
    doc.commit_ops(vec![watermark("DRAFT", None)], "watermark")
        .unwrap();
    planned_in_place(&mut doc);
    let remove = fx::remove_watermark(Some(Z::Over), &pages);
    doc.commit_ops(vec![remove], "remove").unwrap();
    assert!(doc.state().generators().is_empty());
    let (base, section) = appended_in_place(&mut doc).unwrap();
    let keys = keys_at(&doc, &written(base.len(), &section, &path));
    assert_eq!(kinds(&keys), BTreeMap::from([("page", 2), ("catalog", 1)]));
    let pdf = Pdf::load(&path).unwrap();
    assert!(!pdf.catalog().unwrap().has(b"SheafGenerators"));
    for page in self::pages(&pdf) {
        assert_eq!(contents(&pdf, page).len(), 1);
        assert!(marker(&pdf, contents(&pdf, page)[0]).is_none());
    }
}

/// A file saved with a watermark opens with what its generators are
/// numbered in its record, as the catalog's other roots are: the object
/// a save writes it over.
#[test]
fn a_saved_file_s_generators_are_numbered_when_it_opens() {
    let dir = scratch();
    let (path, mut doc) = opened(&dir, "a.pdf", &text_file(1));
    doc.commit_ops(vec![watermark("DRAFT", None)], "watermark")
        .unwrap();
    save_rewritten(&mut doc, &path).unwrap();
    let doc = crate::open::file(&path, None).unwrap().doc;
    let pdf = Pdf::load(&path).unwrap();
    let at = pdf.catalog().unwrap().get(b"SheafGenerators").unwrap();
    let map = &doc.saved_record().unwrap().map;
    assert_eq!(
        map.keys.get(&ObjKey::Generators),
        Some(&at.as_reference().unwrap())
    );
}

/// A watermark of a PDF's page draws that page as a form on every page,
/// one form in the file: both ways.
fn page_form(how: How) {
    let dir = scratch();
    let (_, mut doc) = opened(&dir, "a.pdf", &text_file(3));
    let spec = WatermarkSpec {
        content: WatermarkContent::Page {
            pdf: Some(BlobSel::Id(doc.primary().id())),
            page: 2,
            scale: 0.5,
            opacity: 0.4,
        },
        position: GridPos::BottomRight,
    };
    doc.commit_ops(vec![fx::set_watermark(spec, Z::Over, None)], "watermark")
        .unwrap();
    let bytes = how.write(&doc);
    let pdf = Pdf::load_mem(&bytes).unwrap();
    let forms: Vec<ObjectId> = (pdf.objects.iter())
        .filter(|(_, o)| {
            (o.as_stream()).is_ok_and(|s| {
                s.dict.get(b"Subtype").and_then(Object::as_name).ok() == Some(b"Form")
            })
        })
        .map(|(&id, _)| id)
        .collect();
    assert_eq!(forms.len(), 1, "{how:?}");
    let (_, drawn) = stream(&pdf, forms[0]);
    assert!(holds(&drawn, b"(page 1)"), "{how:?}: the second page");
    for page in pages(&pdf) {
        let (_, block) = stream(&pdf, contents(&pdf, page)[3]);
        assert!(holds(&block, b" Do\n"), "{how:?}");
        let (_, res) = resources(&pdf, page);
        let xobjects = res.get(b"XObject").unwrap().as_dict().unwrap();
        let named: Vec<ObjectId> = (xobjects.iter())
            .map(|(_, r)| r.as_reference().unwrap())
            .collect();
        assert_eq!(named, forms, "{how:?}");
    }
}

crate::both_ways!(
    a_page_watermark_is_one_form,
    a_page_watermark_is_one_form_appended: page_form
);

/// Whether `text` is in any of `pdf`'s objects: its strings, and its
/// streams' data decoded (an object stream's objects are read out).
fn reads(pdf: &Pdf, text: &[u8]) -> bool {
    fn within(o: &Object, text: &[u8]) -> bool {
        match o {
            Object::String(s, _) => holds(s, text),
            Object::Array(items) => items.iter().any(|o| within(o, text)),
            Object::Dictionary(d) => d.iter().any(|(_, o)| within(o, text)),
            Object::Stream(s) => {
                let data = (s.decompressed_content()).unwrap_or_else(|_| s.content.clone());
                holds(&data, text) || s.dict.iter().any(|(_, o)| within(o, text))
            }
            _ => false,
        }
    }
    pdf.objects.values().any(|o| within(o, text))
}

/// A field whose `/Kids` lists the file's catalog, as 0092321's
/// `Page2[0]` does (a `WrongParent` the file has): a watermark appended,
/// the file opened again and the watermark taken off, the Rewrite leaves
/// none of its text, though the catalog the Append wrote, the file's now,
/// held it in `/SheafGenerators`. No field's kid is a catalog, and the
/// form check finds nothing the file didn't have.
#[test]
fn a_watermark_taken_off_isn_t_kept_by_a_field_s_stray_kid() {
    use crate::forms::fixture::{Form, row};
    let mut f = Form::new(1);
    let a = f.text("a", 0, row(0));
    let group = f.group("g", &[a]);
    let catalog = f.catalog;
    f.dict(group).set("Kids", vec![a.into(), catalog.into()]);
    let bytes = f.bytes();
    let was = crate::forms::check(&bytes, None).unwrap();
    let dir = scratch();
    let (path, mut doc) = opened(&dir, "a.pdf", &bytes);
    doc.commit_ops(vec![watermark("SHEAFSTRAYKID", None)], "watermark")
        .unwrap();
    appended_in_place(&mut doc).unwrap();
    assert!(reads(&Pdf::load(&path).unwrap(), b"SHEAFSTRAYKID"));
    let mut doc = crate::open::file(&path, None).unwrap().doc;
    let page = doc.page(0).unwrap();
    let remove = fx::remove_watermark(Some(Z::Over), &[page]);
    doc.commit_ops(vec![remove], "remove").unwrap();
    let answers = super::AskAnswers::default();
    let report = super::save(&mut doc, &path, &answers, &in_progress::NoProgress).unwrap();
    assert_eq!(report.strategy, Some(StrategyKind::Rewrite));
    let out = std::fs::read(&path).unwrap();
    let pdf = Pdf::load_mem(&out).unwrap();
    assert!(!reads(&pdf, b"SHEAFSTRAYKID"));
    let catalogs = |kids: &Object| {
        (kids.as_array().into_iter().flatten())
            .filter_map(|k| pdf.get_dictionary(k.as_reference().ok()?).ok())
            .filter(|d| {
                d.get(b"Type")
                    .and_then(Object::as_name)
                    .is_ok_and(|t| t == b"Catalog")
            })
            .count()
    };
    for o in pdf.objects.values() {
        if let Ok(kids) = o.as_dict().and_then(|d| d.get(b"Kids")) {
            assert_eq!(catalogs(kids), 0, "a field's kid is a catalog");
        }
    }
    let now = crate::forms::check(&out, None).unwrap();
    assert_eq!(now.added_since(&was), []);
}
