//! sheaf's private data (M10.5 §15): what every op family writes is
//! standard keys but [`private::KEYS`], and a file written without it
//! ([`Private::Strip`], with `RemovePrivateData`'s filters on its pages)
//! holds none, its marks named neutrally, uniquely and still threaded,
//! or isn't written at all.

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;

use in_progress::NoProgress;
use lopdf::{Document as Pdf, Object, ObjectId, dictionary};
use sheaf_edit::{BatesSpec, FlattenSet, Transform, Z};

use super::removal_tests::{opened, scratch};
use super::rewrite_tests::thread_mark;
use super::{AskAnswers, Encryption, How, Numbering, Private, Profile, Unheld};
use crate::annot::{AnnotKind, Box2, Pt};
use crate::export::{Export, ExportError, Out, PRINT_ANSWERS};
use crate::forms::fixture::Form;
use crate::model::{AnnotBody, Document, PageId};
use crate::private::{self, found, sheaf_names};
use crate::store::BlobKind;
use crate::tx::{Actor, fixture as fx};

/// A box at `x`, `y` on a page, 80 by 30.
fn at(x: f32, y: f32) -> Box2 {
    Box2 {
        x,
        y,
        w: 80.0,
        h: 30.0,
    }
}

/// A form of four pages, its text field on the first filled, and another
/// tool's square there with no `/NM`: opened, then drawn on with every op
/// family. On the first page a note, a reply to it and one to the square,
/// a highlight, a replacement and a dynamic stamp from the library; a
/// connector from there to the second page; a watermark and a header
/// with Bates numbers on every page; recognized text on the third; and the
/// fourth's comments flattened.
fn everything(dir: &Path) -> Document {
    let mut f = Form::new(4);
    let field = f.text("Name", 0, [72.0, 650.0, 272.0, 670.0]);
    f.fill(field, "Ada");
    let page = f.pages[0];
    let square = f.pdf.add_object(dictionary! {
        "Type" => "Annot", "Subtype" => "Square", "P" => page, "F" => 4,
        "Rect" => vec![400.into(), 400.into(), 500.into(), 450.into()],
        "Contents" => Object::string_literal("Theirs"),
    });
    let mut annots = f.annots(0);
    annots.push(square);
    f.set_annots(0, &annots);
    let (_, mut doc) = opened(dir, "a.pdf", &f.bytes());
    let pages: Vec<PageId> = doc.state().order().iter().copied().collect();
    let p0 = pages[0];
    let library = crate::stamp_lib::build_collection(
        "Mine",
        &[(
            "Paid".to_string(),
            Some("<<initials>> <<date>>".to_string()),
            dynamic_page(),
        )],
    )
    .unwrap();
    let stamp = crate::stamp_lib::one_page(&Pdf::load_mem(&library).unwrap(), 0).unwrap();
    assert!(String::from_utf8_lossy(&stamp).contains(crate::stamp_lib::TEXT_KEY));
    let held = (doc.store().intern(stamp.into(), BlobKind::stamp())).unwrap();
    let note = thread_mark(
        AnnotKind::Note {
            pos: Pt { x: 100.0, y: 500.0 },
        },
        "Why?",
    );
    doc.commit_ops(vec![fx::add(p0, &note)], "note").unwrap();
    let theirs = doc
        .state()
        .foreign_on(p0)
        .next()
        .unwrap()
        .1
        .summary
        .name
        .clone();
    let parent = doc.state().marks_on(p0).next().unwrap().name();
    let kinds = [
        (AnnotKind::Reply { parent }, "Because."),
        (AnnotKind::Reply { parent: theirs }, "Seen."),
        (
            AnnotKind::Highlight {
                rects: vec![at(72.0, 700.0)],
            },
            "",
        ),
        (
            AnnotKind::Replace {
                rects: vec![at(72.0, 600.0)],
                struck: "old".into(),
                at: crate::annot::caret_box(152.0, 600.0, 12.0),
            },
            "new",
        ),
        (
            AnnotKind::PageStamp {
                rect: at(300.0, 300.0),
                stamp: held.id().stem(),
                name: "Paid".into(),
                text: Some("GE 10/22/2026".into()),
            },
            "",
        ),
    ];
    let ops = (kinds.into_iter())
        .map(|(kind, text)| fx::add(p0, &thread_mark(kind, text)))
        .collect();
    doc.commit_ops(ops, "marks").unwrap();
    drop(held);
    let connector = |rect| {
        let kind = AnnotKind::Connector {
            rect,
            label: "A".into(),
            partner: None,
        };
        thread_mark(kind, "")
    };
    let (a, b) = (connector(at(72.0, 100.0)), connector(at(300.0, 100.0)));
    doc.commit_ops(vec![fx::connect((p0, &a), (pages[1], &b))], "connect")
        .unwrap();
    let mut header = fx::header("Page <<1>> <<bates>>");
    header.bates = Some(BatesSpec {
        prefix: "AB".into(),
        suffix: String::new(),
        digits: 6,
        start: 1,
    });
    let ops = vec![
        fx::set_watermark(fx::watermark("DRAFT"), Z::Over, None),
        fx::set_header(header, None),
    ];
    doc.commit_ops(ops, "generators").unwrap();
    let layer = Arc::new(crate::text_layer::fixture::layer());
    doc.commit_ops(fx::ocr([(pages[2], layer)]), "ocr").unwrap();
    let square = thread_mark(
        AnnotKind::Square {
            rect: at(72.0, 400.0),
        },
        "Flat",
    );
    let ops = vec![
        fx::add(pages[3], &square),
        fx::flatten(&[pages[3]], FlattenSet::All),
    ];
    doc.commit_ops(ops, "flatten").unwrap();
    doc
}

/// A stamp's page whose dynamic text the library draws over it: a box.
fn dynamic_page() -> Vec<u8> {
    let mut pdf = Pdf::with_version("1.7");
    let pages = pdf.new_object_id();
    let content = lopdf::Stream::new(dictionary! {}, b"0 0 1 RG 4 w 2 2 196 76 re S".to_vec());
    let content = pdf.add_object(content);
    let page = pdf.add_object(dictionary! {
        "Type" => "Page", "Parent" => pages, "Contents" => content,
        "MediaBox" => vec![0.into(), 0.into(), 200.into(), 80.into()],
    });
    pdf.objects.insert(
        pages,
        dictionary! { "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1 }.into(),
    );
    let catalog = pdf.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
    pdf.trailer.set("Root", catalog);
    let mut bytes = Vec::new();
    pdf.save_to(&mut bytes).unwrap();
    bytes
}

/// `doc`'s current version exported without sheaf's private data
/// ([`Private::Strip`]), numbered from 1, its pages with
/// `RemovePrivateData`'s filters
/// ([`private_filters`](crate::tx::content::private_filters)) when
/// `filtered`.
fn stripped(doc: &Document, filtered: bool) -> Result<Vec<u8>, ExportError> {
    let transforms = match filtered {
        true => vec![Transform::RemovePrivateData],
        false => Vec::new(),
    };
    let e = Export {
        from: doc.export_from(Actor::User),
        transforms,
        profile: Profile {
            encryption: Encryption::Keep,
            numbering: Numbering::Dense,
            private: Private::Strip,
            unheld: Unheld::Keep,
        },
        out: Out::Bytes,
    };
    let exported = crate::export::run(e, &AskAnswers::default(), &NoProgress)?;
    Ok(exported.bytes.expect("bytes"))
}

/// Every annotation of `pdf`'s pages, in order, by object.
fn annots(pdf: &Pdf) -> Vec<(ObjectId, lopdf::Dictionary)> {
    (pdf.get_pages().into_values())
        .flat_map(|page| {
            let list = pdf.get_dictionary(page).unwrap().get(b"Annots").ok();
            let list = list.and_then(|a| a.as_array().ok()).cloned();
            list.unwrap_or_default()
        })
        .filter_map(|a| a.as_reference().ok())
        .map(|id| (id, pdf.get_dictionary(id).unwrap().clone()))
        .collect()
}

/// Each op family's output, saved both ways, names nothing `Sheaf…` but
/// [`private::KEYS`] (§15): not the library's dynamic text either, which
/// a placed stamp's form leaves out. What it does name, it writes.
#[test]
fn no_sheaf_name_but_the_private_keys_is_written() {
    let dir = scratch();
    let doc = everything(&dir);
    for how in [How::Rewritten, How::Appended] {
        let pdf = Pdf::load_mem(&how.write(&doc)).unwrap();
        let names = sheaf_names(&pdf);
        let other: Vec<String> = (names.iter())
            .filter(|n| !private::is_private(n))
            .map(|n| String::from_utf8_lossy(n).into_owned())
            .collect();
        assert_eq!(other, Vec::<String>::new(), "{how:?}");
        for written in [
            "SHEF_Annot",
            "SHEF_Generators",
            "SHEF_Block",
            "SHEF_Gen",
            "SHEF_OcrBlock",
        ] {
            assert!(names.contains(written.as_bytes()), "{how:?}: {written}");
        }
    }
}

/// The same document written without sheaf's private data holds none of
/// it: no name of [`private::KEYS`] and no mark named as sheaf names one.
/// Each annotation's `/NM` is its own; a reply still names what it
/// replies to, the replacement's StrikeOut its caret, and opened again,
/// each reply is in its thread; the connector still leads to its other
/// end. What's drawn is drawn: the header's Bates number, the
/// recognized words, what the flatten drew.
#[test]
fn a_file_without_private_data_holds_none() {
    let dir = scratch();
    let doc = everything(&dir);
    let bytes = stripped(&doc, true).unwrap();
    let pdf = Pdf::load_mem(&bytes).unwrap();
    assert_eq!(found(&pdf), []);
    assert_eq!(sheaf_names(&pdf), BTreeSet::new());
    let annots = annots(&pdf);
    let names: Vec<&[u8]> = (annots.iter())
        .filter_map(|(_, d)| d.get(b"NM").ok()?.as_str().ok())
        .collect();
    let unique: BTreeSet<&[u8]> = names.iter().copied().collect();
    assert_eq!(unique.len(), names.len(), "{names:?}");
    let named = |d: &lopdf::Dictionary, said: &str| {
        d.get(b"Contents").and_then(Object::as_str).ok() == Some(said.as_bytes())
    };
    let find = |said: &str| annots.iter().find(|(_, d)| named(d, said)).unwrap();
    let irt = |said: &str| find(said).1.get(b"IRT").unwrap().as_reference().unwrap();
    assert_eq!(irt("Because."), find("Why?").0);
    assert_eq!(irt("Seen."), find("Theirs").0);
    let strike = (annots.iter())
        .find(|(_, d)| d.get(b"Subtype").and_then(Object::as_name).ok() == Some(b"StrikeOut"));
    assert_eq!(
        strike
            .unwrap()
            .1
            .get(b"IRT")
            .unwrap()
            .as_reference()
            .unwrap(),
        find("new").0
    );
    let links = (annots.iter())
        .filter(|(_, d)| d.get(b"Subtype").and_then(Object::as_name).ok() == Some(b"Link"));
    assert_eq!(links.filter(|(_, d)| d.has(b"Dest")).count(), 2);
    let all = super::recognized_tests::everything(&pdf);
    for drawn in ["Page 3 AB000003", "<00530061007600650064>", "(DRAFT)"] {
        assert!(all.contains(drawn), "{drawn}");
    }
    let (_, again) = opened(&dir, "stripped.pdf", &bytes);
    let p0 = again.page(0).unwrap();
    let foreign: Vec<_> = (again.state().annots_on(p0))
        .filter_map(|(_, rec)| match &rec.body {
            AnnotBody::Foreign(f) => Some(f.summary.clone()),
            _ => None,
        })
        .collect();
    let by = |said: &str| foreign.iter().find(|f| f.contents == said).unwrap();
    assert_eq!(by("Because.").parent.as_deref(), Some(&*by("Why?").name));
    assert_eq!(by("Seen.").parent.as_deref(), Some(&*by("Theirs").name));
}

/// A file to be written without sheaf's private data that still holds
/// some isn't written (§11.9, failing closed): here an older sheaf's
/// watermark settings, its names and its header's, nothing having taken
/// them out of its pages' content.
#[test]
fn a_file_holding_private_data_is_not_written_without_it() {
    let dir = scratch();
    let (_, doc) = opened(&dir, "older.pdf", &super::header_tests::older_header_file());
    let Err(ExportError::Failed(why)) = stripped(&doc, false) else {
        panic!("written")
    };
    assert!(
        why.contains("it would hold sheaf's private data: Sheaf"),
        "{why}"
    );
    let pdf = Pdf::load_mem(&stripped(&doc, true).unwrap()).unwrap();
    assert_eq!(found(&pdf), []);
}

/// A page whose content can't be read whole (one of its streams can't be
/// decoded), another of its streams holding an older sheaf's watermark
/// settings: `RemovePrivateData`'s filters can't reach the page, so a
/// clean copy isn't written (failing closed), and print without markup
/// is, a note saying what's left (§16 row 27, `Private::Scrub`).
#[test]
fn private_data_a_page_s_content_hides_fails_a_clean_copy_alone() {
    let dir = scratch();
    let legacy: &[u8] = b"/Artifact <</Type /Pagination /Subtype /Watermark /SheafSettings \
        (settings) >> BDC q 0.5 g 100 100 50 50 re f Q EMC";
    let undecoded: &[u8] = b"% undecoded";
    let file =
        super::others_tests::acrobat_file(&[&[super::others_tests::BODY, legacy, undecoded]]);
    let mut pdf = Pdf::load_mem(&file).unwrap();
    for obj in pdf.objects.values_mut() {
        if let Ok(stream) = obj.as_stream_mut()
            && stream.content == undecoded
        {
            stream.dict.set("Filter", "JBIG2Decode");
        }
    }
    let mut bytes = Vec::new();
    pdf.save_to(&mut bytes).unwrap();
    let (_, doc) = opened(&dir, "a.pdf", &bytes);
    let Err(ExportError::Failed(why)) = stripped(&doc, true) else {
        panic!("written")
    };
    assert!(why.contains("SheafSettings"), "{why}");
    let page = doc.page(0).unwrap();
    let print = Export::print(doc.export_from(Actor::User), &[page], false);
    let printed = crate::export::run(print, &PRINT_ANSWERS, &NoProgress).unwrap();
    let left = "some of sheaf's private data couldn't be taken out: SheafSettings";
    let notes: Vec<&str> = printed.notes.iter().map(|n| n.0.as_str()).collect();
    assert!(notes.iter().any(|n| n.starts_with(left)), "{notes:?}");
}

/// A page flattened, whose block is made only as it's written, is read
/// all the same: an older sheaf's settings in its own content fail the
/// file, and taken out, it's written, its flattened mark drawn. Written
/// again, it's the same bytes.
#[test]
fn a_flattened_page_s_own_content_is_read() {
    let dir = scratch();
    let legacy: &[u8] = b"/Artifact <</Type /Pagination /Subtype /Watermark /SheafSettings \
        (settings) >> BDC q 0.5 g 100 100 50 50 re f Q EMC";
    let file = super::others_tests::acrobat_file(&[&[super::others_tests::BODY, legacy]]);
    let (_, mut doc) = opened(&dir, "a.pdf", &file);
    let page = doc.page(0).unwrap();
    let square = thread_mark(
        AnnotKind::Square {
            rect: at(72.0, 400.0),
        },
        "Flat",
    );
    let ops = vec![
        fx::add(page, &square),
        fx::flatten(&[page], FlattenSet::All),
    ];
    doc.commit_ops(ops, "flatten").unwrap();
    let Err(ExportError::Failed(why)) = stripped(&doc, false) else {
        panic!("written")
    };
    assert!(why.contains("SheafSettings"), "{why}");
    let bytes = stripped(&doc, true).unwrap();
    let pdf = Pdf::load_mem(&bytes).unwrap();
    assert_eq!(found(&pdf), []);
    let first = super::recognized_tests::pages(&pdf)[0];
    let drawn = super::recognized_tests::drawn(&pdf, first);
    assert!(drawn.contains(" Do\n"), "the flattened square: {drawn}");
    assert_eq!(stripped(&doc, true).unwrap(), bytes);
}
