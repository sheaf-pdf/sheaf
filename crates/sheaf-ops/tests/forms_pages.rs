//! A page keeps its form through the page tools (M10 slice 3). A crop is
//! the page's box and a watermark is drawn on the page as a save writes
//! it, so either keeps the page's own fields, and nothing is made of the
//! page (M10.5 slices 15 and 16); Remove Header/Footer carries them into
//! the copy it makes; either way the page shows them, and a save writes
//! them back as they were (bug 1). A
//! removed page's widgets leave the file, values and all, even ones only
//! the structure tree's parent tree reaches (bug 5). A form left with no
//! fields goes, and direct-object fields are pruned too (bug 10). Each by
//! both save strategies, with the form check clean on the output.
//!
//! Needs libpdfium (`scripts/fetch-pdfium.sh`), like the app itself.

use std::collections::BTreeMap;

use futures::executor::block_on;
use lopdf::{Document as Pdf, Object, ObjectId, dictionary};
use sheaf_doc::forms::check;
use sheaf_doc::forms::corpus::Corpus;
use sheaf_doc::forms::fixture::Form;
use sheaf_doc::realize::ViewInputs;
use sheaf_doc::store::{BlobKind, MemoryKind, Origin};
use sheaf_doc::tx::fixture::{delete, remove_watermark, set_header, set_watermark, watermark};
use sheaf_doc::{Actor, CommitOpts, Derive, PageId};
use sheaf_edit::{Margins, Op, PageSel, Rotation, Z};
use sheaf_image::raster::PixelFormat;
use sheaf_render::pdfium::render_appearances;
use sheaf_render::{Budget, Job, PageIndex};

mod common;
use common::*;

/// A page's `/CropBox`.
fn crop_box(bytes: &[u8], page: usize) -> Vec<f64> {
    let pdf = Pdf::load_mem(bytes).unwrap();
    let id = pdf.get_pages()[&(page as u32 + 1)];
    pdf.get_dictionary(id)
        .unwrap()
        .get(b"CropBox")
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .map(|o| o.as_float().unwrap())
        .collect()
}

sheaf_doc::both_ways!(
    a_cropped_form_page_keeps_its_fields,
    a_cropped_form_page_keeps_its_fields_appended: a_cropped_form_page_keeps_its_fields_by
);

/// A cropped form page keeps its widgets and shows its value, with no copy
/// and no cache file: the page is still its file's, cropped, and on screen
/// the document's views draw its field (a copy once had no `/AcroForm`, and
/// the field was blank); nothing is made of it, in the store or the cache
/// ([`own_cache`]); and saved `how`, the file has the form, its values,
/// the crop, the value drawn, and the page's annotations as they were.
fn a_cropped_form_page_keeps_its_fields_by(how: How) {
    let test = match how {
        How::Rewritten => "a_cropped_form_page_keeps_its_fields",
        How::Appended => "a_cropped_form_page_keeps_its_fields_appended",
    };
    own_cache(test, || cropped_form_page(how));
}

/// [`a_cropped_form_page_keeps_its_fields_by`]'s case, in its process.
fn cropped_form_page(how: How) {
    let dir = scratch(&format!("crop-{how:?}"));
    let path = write(&dir, "form.pdf", &mut two_page_form());
    let input = std::fs::read(&path).unwrap();
    let mut doc = open(&path);
    let page: PageId = doc.state().order()[0];
    // 100 pt off the bottom: the fields stay in view.
    let margins = Margins {
        top: 0.0,
        left: 0.0,
        right: 0.0,
        bottom: 100.0,
    };
    crop(&mut doc, &[page], margins);
    let base = &doc.state().def(page).unwrap().base;
    assert_eq!(base.src, *doc.primary());
    assert!(doc.derived().is_empty(), "no copy");
    let copies = (doc.store().sources().into_iter()).filter(|src| {
        matches!(
            src.blob().kind(),
            BlobKind::Pdf {
                origin: Origin::Memory(MemoryKind::Derived),
                ..
            }
        )
    });
    assert_eq!(copies.count(), 0, "no copy in the store");
    let job = Job::Page {
        width: 612,
        rotation: Rotation::R0,
    };
    let (format, budget) = (PixelFormat::Rgba, Budget::Screen);
    let screen = ViewInputs::screen();
    let rx = views().render(doc.state(), page, &screen, job, format, 0, budget);
    let r = block_on(rx).unwrap().unwrap().raster;
    assert!(
        blue_in(&r, 0, 792.0) > HALF_A_ROW,
        "the page draws the field"
    );
    let (strategy, bytes) = saved(&mut doc, &path, how);
    let what = format!("{strategy:?}");
    assert_eq!(strategy, how.strategy());
    let report = clean(&bytes, &what);
    assert_eq!((report.fields, report.widgets), (4, 4), "{what}");
    let v = values(&bytes);
    assert_eq!(
        (v["a"].as_str(), v["b"].as_str()),
        ("Alpha", "Beta"),
        "{what}"
    );
    assert_eq!(
        crop_box(&bytes, 0),
        vec![0.0, 100.0, 612.0, 792.0],
        "{what}"
    );
    // Other viewers see the value: its appearance is on the page.
    let r = render_appearances(&bytes, None, PageIndex(0), 612).unwrap();
    assert!(
        blue_in(&r, 0, 792.0) > HALF_A_ROW,
        "{what}: the value shows"
    );
    // The page's annotations are its own widgets, as they were: the same
    // objects after an Append, the same fields at the same places after a
    // Rewrite.
    let [was, now] = [&input, &bytes].map(|b| annots_of(b, 0));
    assert_eq!(now.len(), 3, "{what}");
    let what_each = |a: &[(ObjectId, String)]| a.iter().map(|(_, a)| a.clone()).collect::<Vec<_>>();
    assert_eq!(what_each(&now), what_each(&was), "{what}");
    if how == How::Appended {
        assert_eq!(now, was, "{what}: the same objects");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// Page `page`'s annotations in `bytes`: each object, and what it is: its
/// subtype, field name and `/Rect`.
fn annots_of(bytes: &[u8], page: usize) -> Vec<(ObjectId, String)> {
    let pdf = Pdf::load_mem(bytes).unwrap();
    let id = pdf.get_pages()[&(page as u32 + 1)];
    let annots = pdf.get_dictionary(id).unwrap().get(b"Annots").unwrap();
    let annots = pdf.dereference(annots).unwrap().1.as_array().unwrap();
    (annots.iter())
        .map(|a| {
            let at = a.as_reference().unwrap();
            let d = pdf.get_dictionary(at).unwrap();
            let subtype = String::from_utf8_lossy(d.get(b"Subtype").unwrap().as_name().unwrap());
            let name = d.get(b"T").and_then(|t| t.as_str()).unwrap_or_default();
            let rect = d.get(b"Rect").unwrap().as_array().unwrap();
            let rect: Vec<f64> = rect.iter().map(|v| v.as_float().unwrap()).collect();
            let name = String::from_utf8_lossy(name);
            (at, format!("{subtype} {name:?} {rect:?}"))
        })
        .collect()
}

/// Each widget on `bytes`' pages has its page as its `/P`.
fn on_their_pages(bytes: &[u8]) -> bool {
    let pdf = Pdf::load_mem(bytes).unwrap();
    pdf.get_pages().into_values().all(|page| {
        let annots = pdf.get_dictionary(page).unwrap().get(b"Annots");
        let annots = annots
            .and_then(|a| pdf.dereference(a))
            .map(|(_, a)| a.clone());
        let annots = annots.ok().and_then(|a| a.as_array().ok().cloned());
        (annots.unwrap_or_default().iter()).all(|a| {
            let d = pdf.get_dictionary(a.as_reference().unwrap()).unwrap();
            d.get(b"Subtype").unwrap().as_name().unwrap() != b"Widget"
                || d.get(b"P").and_then(Object::as_reference).ok() == Some(page)
        })
    })
}

/// A watermark on both pages, and with `remove`, taken off again, written
/// `how`: edits of the document, so nothing is made of the pages, in the
/// store or the cache ([`own_cache`]); on screen the document's views draw
/// the fields; and the file has the form, its values, each widget on its
/// page, the values drawn, and the mark, or not, the form check clean.
fn watermarked(remove: bool, how: How) {
    let name = if remove { "unmarked" } else { "marked" };
    let dir = scratch(&format!("{name}-{how:?}"));
    let path = write(&dir, "form.pdf", &mut two_page_form());
    let mut doc = open(&path);
    let pages: Vec<PageId> = doc.state().order().iter().copied().collect();
    let set = set_watermark(watermark("DRAFT"), Z::Over, None);
    doc.commit_ops(vec![set], "Watermark").unwrap();
    if remove {
        let removed = remove_watermark(Some(Z::Over), &pages);
        doc.commit_ops(vec![removed], "Remove watermark").unwrap();
    }
    assert!(doc.derived().is_empty(), "no copy");
    let copies = (doc.store().sources().into_iter()).filter(|src| {
        matches!(
            src.blob().kind(),
            BlobKind::Pdf {
                origin: Origin::Memory(MemoryKind::Derived),
                ..
            }
        )
    });
    assert_eq!(copies.count(), 0, "no copy in the store");
    let job = Job::Page {
        width: 612,
        rotation: Rotation::R0,
    };
    let (format, budget, screen) = (PixelFormat::Rgba, Budget::Screen, ViewInputs::screen());
    for &page in &pages {
        let rx = views().render(doc.state(), page, &screen, job, format, 0, budget);
        let r = block_on(rx).unwrap().unwrap().raster;
        assert!(
            blue_in(&r, 0, 792.0) > HALF_A_ROW,
            "{page:?} draws its field"
        );
    }
    let bytes = written(&doc, how);
    let what = format!("{name}, {how:?}");
    let report = clean(&bytes, &what);
    assert_eq!((report.fields, report.widgets), (4, 4), "{what}");
    let v = values(&bytes);
    assert_eq!(
        (v["a"].as_str(), v["b"].as_str()),
        ("Alpha", "Beta"),
        "{what}"
    );
    assert!(on_their_pages(&bytes), "{what}");
    for page in 0..2 {
        let r = render_appearances(&bytes, None, PageIndex(page), 612).unwrap();
        assert!(blue_in(&r, 0, 792.0) > HALF_A_ROW, "{what}: page {page}");
    }
    assert_eq!(holds(&bytes, b"DRAFT"), !remove, "{what}");
    let _ = std::fs::remove_dir_all(&dir);
}

sheaf_doc::both_ways!(
    a_watermarked_form_keeps_its_fields,
    a_watermarked_form_keeps_its_fields_appended: marked_by
);

fn marked_by(how: How) {
    let test = match how {
        How::Rewritten => "a_watermarked_form_keeps_its_fields",
        How::Appended => "a_watermarked_form_keeps_its_fields_appended",
    };
    own_cache(test, || watermarked(false, how));
}

sheaf_doc::both_ways!(
    a_form_with_its_watermark_removed_keeps_its_fields,
    a_form_with_its_watermark_removed_keeps_its_fields_appended: unmarked_by
);

fn unmarked_by(how: How) {
    let test = match how {
        How::Rewritten => "a_form_with_its_watermark_removed_keeps_its_fields",
        How::Appended => "a_form_with_its_watermark_removed_keeps_its_fields_appended",
    };
    own_cache(test, || watermarked(true, how));
}

/// Tag `form` as Acrobat tags some forms: beside each widget's `/Form`
/// element in the tree, a second one with no `/P`, which only the parent
/// tree reaches (M10 bug 5).
fn tag_with_strays(form: &mut Form) {
    form.tag();
    let root = form
        .pdf
        .catalog()
        .unwrap()
        .get(b"StructTreeRoot")
        .unwrap()
        .as_reference()
        .unwrap();
    let nums = form
        .dict(root)
        .get(b"ParentTree")
        .unwrap()
        .as_dict()
        .unwrap()
        .get(b"Nums")
        .unwrap()
        .as_array()
        .unwrap()
        .clone();
    let mut strays = Vec::new();
    for pair in nums.chunks(2) {
        let element = pair[1].as_reference().unwrap();
        let k = form.dict(element).get(b"K").unwrap().clone();
        let page = form.dict(element).get(b"Pg").unwrap().clone();
        let stray = form.pdf.add_object(dictionary! {
            "Type" => "StructElem", "S" => "Form", "Pg" => page, "K" => k,
        });
        strays.push(pair[0].clone());
        strays.push(Object::Reference(stray));
    }
    form.dict(root)
        .set("ParentTree", dictionary! { "Nums" => strays });
}

#[test]
fn a_form_with_its_header_and_footer_removed_keeps_its_fields() {
    let dir = scratch("header-footer");
    let path = write(&dir, "form.pdf", &mut two_page_form());
    // A footer, saved into the file.
    let mut doc = open(&path);
    let hf = sheaf_doc::stamps::HeaderFooter {
        texts: [
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            "SHEAFFOOTER <<1>>".into(),
            String::new(),
        ],
        ..Default::default()
    };
    doc.commit_ops(vec![set_header(hf.spec(), None)], "Header & footer")
        .unwrap();
    let footed = dir.join("footed.pdf");
    sheaf_doc::save::rewrite_to_path(&doc.snapshot(), &footed).unwrap();
    assert!(holds(&std::fs::read(&footed).unwrap(), b"SHEAFFOOTER"));

    // Removed, as the app does it.
    let mut doc = open(&footed);
    let derived =
        sheaf_ops::derive::strip_stamps(doc.store(), &doc.snapshot(), &sheaf_ops::NoProgress)
            .unwrap();
    assert_eq!(derived.len(), 1);
    derived[0].note(&mut doc);
    let mut tx = doc.tx(Actor::User);
    let pages = derived[0].bases().collect();
    tx.set_page_base(pages, Derive::StripHeaderFooter).unwrap();
    let remove = Op::RemoveHeaderFooter {
        pages: PageSel::All,
    };
    tx.apply(remove, &sheaf_ops::NoProgress).unwrap();
    doc.commit(tx, CommitOpts::labelled("Remove header & footer"))
        .unwrap();
    // The footer's gone from the file: a save is a Rewrite.
    let saved = rewritten_and_saved(&mut doc, &footed);
    assert_eq!(saved[1].0, StrategyKind::Rewrite);
    for (strategy, bytes) in saved {
        let what = format!("{strategy:?}");
        assert!(!holds(&bytes, b"SHEAFFOOTER"), "{what}");
        let report = clean(&bytes, &what);
        assert_eq!((report.fields, report.widgets), (4, 4), "{what}");
        let v = values(&bytes);
        assert_eq!(
            (v["a"].as_str(), v["b"].as_str()),
            ("Alpha", "Beta"),
            "{what}"
        );
        for page in 0..2 {
            let r = render_appearances(&bytes, None, PageIndex(page), 612).unwrap();
            assert!(blue_in(&r, 0, 792.0) > HALF_A_ROW, "{what}: page {page}");
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// Two tagged pages whose widgets the parent tree reaches only through
/// strays; on the second, a field holding `LEAKVALUE123`.
fn leaky_form() -> Form {
    let mut f = Form::new(2);
    blue(&mut f, "keep", 0, 0, "KEPT");
    blue(&mut f, "gone", 1, 0, "LEAKVALUE123");
    tag_with_strays(&mut f);
    f
}

/// Tag `form`, then point its parent tree into a second tree the root
/// doesn't hold, as the I-9's is: a page element per page over a
/// `/Figure` (with alt text, "SHEAFALT <page>") over a `/Form` element per
/// widget. An element only the ID tree reaches ("SHEAFID <page>") points
/// at each page's widgets too.
fn tag_with_a_second_tree(form: &mut Form) {
    form.tag();
    let root = form
        .pdf
        .catalog()
        .unwrap()
        .get(b"StructTreeRoot")
        .unwrap()
        .as_reference()
        .unwrap();
    let stale = form.pdf.new_object_id();
    let (mut tops, mut nums, mut ids) = (Vec::new(), Vec::new(), Vec::new());
    for (i, page) in form.pages.clone().into_iter().enumerate() {
        let top = form.pdf.new_object_id();
        let figure = form.pdf.new_object_id();
        let mut kids = vec![Object::Integer(0)];
        let mut objrs = Vec::new();
        for w in form.annots(i) {
            let key = form.dict(w).get(b"StructParent").unwrap().as_i64().unwrap();
            let objr = dictionary! { "Type" => "OBJR", "Obj" => w, "Pg" => page };
            objrs.push(Object::Dictionary(objr.clone()));
            let element = form.pdf.add_object(dictionary! {
                "Type" => "StructElem", "S" => "Form", "P" => figure, "K" => objr,
            });
            kids.push(Object::Reference(element));
            nums.push(Object::Integer(key));
            nums.push(Object::Reference(element));
        }
        form.pdf.objects.insert(
            figure,
            dictionary! {
                "Type" => "StructElem", "S" => "Figure", "P" => top, "Pg" => page, "K" => kids,
                "Alt" => Object::string_literal(format!("SHEAFALT {i}")),
            }
            .into(),
        );
        form.pdf.objects.insert(
            top,
            dictionary! { "Type" => "StructElem", "S" => "Part", "P" => stale, "K" => vec![figure.into()] }
                .into(),
        );
        tops.push(Object::Reference(top));
        let only = form.pdf.add_object(dictionary! {
            "Type" => "StructElem", "S" => "Form", "ID" => Object::string_literal(format!("id{i}")),
            "Alt" => Object::string_literal(format!("SHEAFID {i}")), "K" => objrs,
        });
        ids.push(Object::string_literal(format!("id{i}")));
        ids.push(Object::Reference(only));
    }
    form.pdf.objects.insert(
        stale,
        dictionary! { "Type" => "StructTreeRoot", "K" => tops }.into(),
    );
    let r = form.dict(root);
    r.set("ParentTree", dictionary! { "Nums" => nums });
    r.set("IDTree", dictionary! { "Names" => ids });
}

#[test]
fn a_deleted_pages_value_leaves_a_second_tree() {
    let dir = scratch("second-tree");
    let mut f = Form::new(2);
    blue(&mut f, "keep", 0, 0, "KEPT");
    blue(&mut f, "gone", 1, 0, "LEAKVALUE123");
    tag_with_a_second_tree(&mut f);
    let path = write(&dir, "form.pdf", &mut f);
    let mut doc = open(&path);
    let second = doc.state().order()[1];
    doc.commit_ops(vec![delete(&[second])], "Delete").unwrap();
    for (strategy, bytes) in rewritten_and_saved(&mut doc, &path) {
        let what = format!("{strategy:?}");
        assert!(!holds(&bytes, b"LEAKVALUE123"), "{what}: the value stayed");
        // What described the page went with it; the first page's stays.
        assert!(!holds(&bytes, b"SHEAFALT 1"), "{what}");
        assert!(!holds(&bytes, b"SHEAFID 1"), "{what}");
        assert!(
            holds(&bytes, b"SHEAFALT 0") && holds(&bytes, b"SHEAFID 0"),
            "{what}"
        );
        assert_eq!(dangling_structure(&bytes), 0, "{what}");
        let report = clean(&bytes, &what);
        assert_eq!(
            (report.pages, report.fields, report.widgets),
            (1, 1, 1),
            "{what}"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_deleted_pages_value_leaves_the_file() {
    let dir = scratch("delete");
    let path = write(&dir, "form.pdf", &mut leaky_form());
    let mut doc = open(&path);
    let second = doc.state().order()[1];
    doc.commit_ops(vec![delete(&[second])], "Delete").unwrap();
    let saved = rewritten_and_saved(&mut doc, &path);
    // A removal is never an Append (M9 bug 14).
    assert_eq!(saved[1].0, StrategyKind::Rewrite);
    for (strategy, bytes) in saved {
        let what = format!("{strategy:?}");
        assert!(!holds(&bytes, b"LEAKVALUE123"), "{what}: the value stayed");
        let report = clean(&bytes, &what);
        assert_eq!(
            (report.pages, report.fields, report.widgets),
            (1, 1, 1),
            "{what}"
        );
        assert_eq!(values(&bytes)["keep"], "KEPT", "{what}");
        assert_eq!(dangling_structure(&bytes), 0, "{what}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_extracted_page_brings_no_other_pages_value() {
    let dir = scratch("extract");
    let path = write(&dir, "form.pdf", &mut leaky_form());
    let doc = open(&path);
    let first = doc.state().order()[0];
    let bytes = sheaf_doc::save::extract_bytes(&doc.snapshot(), &[first]).unwrap();
    assert!(!holds(&bytes, b"LEAKVALUE123"));
    let report = clean(&bytes, "extracted");
    assert_eq!((report.pages, report.fields, report.widgets), (1, 1, 1));
    assert_eq!(dangling_structure(&bytes), 0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_form_left_with_no_fields_goes() {
    // Fields on the first page only, as on the W-9 (its second page is
    // instructions).
    let dir = scratch("empty");
    let mut f = Form::new(2);
    blue(&mut f, "a", 0, 0, "Alpha");
    f.check_box("box", 0, row(1), true);
    f.xfa();
    let path = write(&dir, "form.pdf", &mut f);
    let mut doc = open(&path);
    let (first, second) = (doc.state().order()[0], doc.state().order()[1]);
    let bytes = sheaf_doc::save::extract_bytes(&doc.snapshot(), &[second]).unwrap();
    assert_eq!(fields_of(&bytes), None, "extracted");
    assert!(!holds(&bytes, b"Alpha"), "extracted");
    let report = clean(&bytes, "extracted");
    assert_eq!(report.class(), "no form");

    doc.commit_ops(vec![delete(&[first])], "Delete").unwrap();
    for (strategy, bytes) in rewritten_and_saved(&mut doc, &path) {
        let what = format!("{strategy:?}");
        assert_eq!(fields_of(&bytes), None, "{what}");
        assert!(!holds(&bytes, b"Alpha"), "{what}");
        assert_eq!(clean(&bytes, &what).class(), "no form", "{what}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn direct_object_fields_are_pruned() {
    // `/Fields` holds a parent field as a direct dictionary, whose kids
    // are widgets on the first page; a text field is on the second.
    let dir = scratch("direct");
    let mut f = Form::new(2);
    let a = blue(&mut f, "a", 0, 0, "LEAKVALUE123");
    let b = f.text("b", 0, row(1));
    let keep = blue(&mut f, "keep", 1, 0, "KEPT");
    for id in [a, b] {
        f.dict(id).remove(b"T");
    }
    let direct = dictionary! {
        "T" => Object::string_literal("group"),
        "Kids" => vec![Object::Reference(a), Object::Reference(b)],
    };
    f.dict(f.acroform).set(
        "Fields",
        vec![Object::Dictionary(direct), Object::Reference(keep)],
    );
    let path = write(&dir, "form.pdf", &mut f);
    let mut doc = open(&path);
    let first = doc.state().order()[0];
    doc.commit_ops(vec![delete(&[first])], "Delete").unwrap();
    for (strategy, bytes) in rewritten_and_saved(&mut doc, &path) {
        let what = format!("{strategy:?}");
        let fields = fields_of(&bytes).unwrap();
        assert_eq!(fields.len(), 1, "{what}: {fields:?}");
        assert!(fields[0].as_reference().is_ok(), "{what}");
        assert!(!holds(&bytes, b"LEAKVALUE123"), "{what}");
        assert_eq!(values(&bytes)["keep"], "KEPT", "{what}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn page_templates_stay() {
    // A form that spawns pages keeps a template page outside the page
    // tree, with its own field.
    let dir = scratch("templates");
    let mut f = two_page_form();
    let template = f.pdf.add_object(dictionary! {
        "Type" => "Page", "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
    });
    let widget = f.pdf.add_object(dictionary! {
        "Type" => "Annot", "Subtype" => "Widget", "FT" => "Tx", "P" => template,
        "T" => Object::string_literal("spawned"),
        "Rect" => vec![100.into(), 700.into(), 300.into(), 730.into()],
    });
    f.dict(template)
        .set("Annots", vec![Object::Reference(widget)]);
    let mut fields = f.fields();
    fields.push(widget);
    f.set_fields(&fields);
    let catalog = f.catalog;
    f.dict(catalog).set(
        "Names",
        dictionary! { "Templates" => dictionary! {
            "Names" => vec![Object::string_literal("extra"), Object::Reference(template)],
        } },
    );
    let path = write(&dir, "form.pdf", &mut f);
    let mut doc = open(&path);
    let second = doc.state().order()[1];
    doc.commit_ops(vec![delete(&[second])], "Delete").unwrap();
    // The new writer's post-conditions count the template as a page the
    // file keeps.
    let saved = rewritten_and_saved(&mut doc, &path)
        .into_iter()
        .map(|(strategy, bytes)| (format!("{strategy:?}"), bytes));
    for (what, bytes) in saved {
        let report = clean(&bytes, &what);
        assert_eq!((report.template_widgets, report.fields), (1, 4), "{what}");
        let pdf = Pdf::load_mem(&bytes).unwrap();
        let names = pdf.catalog().unwrap().get(b"Names").unwrap();
        let names = pdf.dereference(names).unwrap().1.as_dict().unwrap();
        let templates = pdf.dereference(names.get(b"Templates").unwrap()).unwrap().1;
        let entry = &templates
            .as_dict()
            .unwrap()
            .get(b"Names")
            .unwrap()
            .as_array()
            .unwrap()[1];
        let page = pdf.get_dictionary(entry.as_reference().unwrap()).unwrap();
        assert_eq!(
            page.get(b"Annots").unwrap().as_array().unwrap().len(),
            1,
            "{what}"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// Bug 5 as planning reproduced it: CA 540's `540-5007` (on the 2025
/// form, `540_form_5007`) filled with `LEAKVALUE123`, then page 5 deleted,
/// or page 6 extracted.
#[test]
fn corpus_ca_540_leaks_no_removed_value() {
    let Some(form) = corpus_form("forms/ca-ftb/2025-540.pdf") else {
        return;
    };
    let dir = scratch("ca540");
    let path = dir.join("540.pdf");
    std::fs::write(&path, filled(&form, "540_form_5007", "LEAKVALUE123")).unwrap();
    let mut doc = open(&path);
    let pages: Vec<PageId> = doc.state().order().iter().copied().collect();
    let extracted = sheaf_doc::save::extract_bytes(&doc.snapshot(), &[pages[5]]).unwrap();
    assert!(!holds(&extracted, b"LEAKVALUE123"), "extracted");
    assert_eq!(findings(&extracted), vec![], "extracted");

    doc.commit_ops(vec![delete(&[pages[4]])], "Delete").unwrap();
    for (strategy, bytes) in rewritten_and_saved(&mut doc, &path) {
        let what = format!("{strategy:?}");
        assert!(!holds(&bytes, b"LEAKVALUE123"), "{what}");
        assert_eq!(findings(&bytes), vec![], "{what}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

sheaf_doc::both_ways!(
    corpus_cropped_forms_keep_their_fields,
    corpus_cropped_forms_keep_their_fields_appended: corpus_cropped_forms_keep_their_fields_by
);

/// Bug 1 on real forms: the 1040's first page and the W-9's first page
/// cropped, saved both ways, keep every field and its value.
fn corpus_cropped_forms_keep_their_fields_by(how: How) {
    for (file, field) in [
        ("forms/irs/f1040.pdf", "topmostSubform[0].Page1[0].f1_14[0]"),
        ("forms/irs/fw9.pdf", "topmostSubform[0].Page1[0].f1_01[0]"),
    ] {
        let Some(form) = corpus_form(file) else {
            return;
        };
        let dir = scratch(&format!("corpus-crop-{how:?}"));
        let path = dir.join("form.pdf");
        std::fs::write(&path, filled(&form, field, "Filled In")).unwrap();
        let input = std::fs::read(&path).unwrap();
        let before = check(&input, None).unwrap();
        // The fill has no appearance yet (slice 9 makes them).
        let expected = vec![sheaf_doc::forms::Finding::NoAppearance(vec![
            field.to_string(),
        ])];
        let mut doc = open(&path);
        let page = doc.state().order()[0];
        let margins = Margins {
            top: 10.0,
            left: 10.0,
            right: 10.0,
            bottom: 10.0,
        };
        crop(&mut doc, &[page], margins);
        let (strategy, bytes) = (how.strategy(), written(&doc, how));
        let what = format!("{file}, {strategy:?}");
        let report = check(&bytes, None).unwrap();
        assert_eq!(
            (report.fields, report.widgets),
            (before.fields, before.widgets),
            "{what}"
        );
        assert_eq!(findings(&bytes), expected, "{what}");
        assert_eq!(values(&bytes)[field], "Filled In", "{what}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// Bug 5 on every curated form with more than one page: each text field
/// whose widgets are all on the page with most of them is filled with a
/// marker, the page is deleted, and no marker is left in the rewrite.
#[test]
fn corpus_deleted_pages_leave_no_values() {
    let Some(corpus) = Corpus::or_skip() else {
        return;
    };
    let dir = scratch("corpus-delete");
    let (mut tried, mut failed) = (0, Vec::new());
    for entry in corpus.manifest().unwrap() {
        if entry.path.starts_with("forms/safedocs/") || entry.pages < 2 {
            continue;
        }
        let bytes = std::fs::read(corpus.path(&entry)).unwrap();
        let mut pdf = Pdf::load_mem(&bytes).unwrap();
        let form = form_in(&bytes);
        // Each text field's pages.
        let mut pages: BTreeMap<_, Vec<u32>> = BTreeMap::new();
        for (&page, widgets) in &form.widgets {
            for w in widgets {
                pages.entry(w.field).or_default().push(page);
            }
        }
        let pages = &pages;
        let alone = |page: u32| {
            form.fields.iter().filter(move |f| {
                f.kind == sheaf_doc::forms::FieldKind::Text
                    && pages
                        .get(&f.id)
                        .is_some_and(|p| p.iter().all(|&q| q == page))
            })
        };
        let Some(page) = (0..entry.pages).max_by_key(|&p| alone(p).count()) else {
            continue;
        };
        let objects: Vec<ObjectId> = alone(page).map(|f| f.object).collect();
        if objects.is_empty() {
            continue;
        }
        for (i, &object) in objects.iter().enumerate() {
            pdf.get_dictionary_mut(object)
                .unwrap()
                .set("V", Object::string_literal(format!("SHEAFLEAK{i}")));
        }
        let path = dir.join("form.pdf");
        pdf.save(&path).unwrap();
        let mut doc = open(&path);
        let id = doc.state().order()[page as usize];
        doc.commit_ops(vec![delete(&[id])], "Delete").unwrap();
        let bytes = sheaf_doc::save::rewrite(&doc.snapshot()).unwrap();
        let dangling = dangling_structure(&bytes);
        if holds(&bytes, b"SHEAFLEAK") || dangling > 0 {
            // Kept, to look at.
            let name = entry.path.replace('/', "_");
            std::fs::write(dir.join(&name), &bytes).unwrap();
            failed.push(format!(
                "{}: page {} deleted, a value stayed or {dangling} structure references \
                 lead nowhere (in {})",
                entry.path,
                page + 1,
                dir.join(&name).display()
            ));
        }
        tried += 1;
    }
    println!("{tried} forms, a page of each deleted");
    assert!(failed.is_empty(), "{failed:#?}");
    let _ = std::fs::remove_dir_all(&dir);
}
