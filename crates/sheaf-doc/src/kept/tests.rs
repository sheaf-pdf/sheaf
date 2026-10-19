//! Bug 14's reproductions and their variants: what a save, an extract, a
//! crop or a stamp writes holds no page it doesn't keep.

use std::path::{Path, PathBuf};

use lopdf::content::{Content, Operation};
use lopdf::{Document as Pdf, Object, ObjectId, Stream, dictionary};
use sheaf_edit::Rotation;

use crate::annot::{Annot, AnnotId, AnnotKind, Pt};
use crate::effects::{Removed, RemovedKind};
use crate::materialize::{
    AskAnswers, How, StrategyKind, Why, forced_rewrite, materialize, removal, save,
};
use crate::model::Document;
use crate::save::rewrite;
use crate::tx::fixture as fx;
use in_progress::NoProgress;

fn tempdir() -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let d = std::env::temp_dir().join(format!(
        "sheaf-kept-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A file of `n` pages saying "MARK <tag> <i>", with `extra` adding to it
/// (links, names…) given the page ids.
fn file(
    dir: &Path,
    name: &str,
    tag: &str,
    n: usize,
    extra: impl FnOnce(&mut Pdf, &[ObjectId]),
) -> PathBuf {
    let mut pdf = Pdf::with_version("1.5");
    let pages_id = pdf.new_object_id();
    let font = pdf.add_object(
        dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica" },
    );
    let mut ids = Vec::new();
    for i in 0..n {
        let content = Content {
            operations: vec![
                Operation::new("BT", vec![]),
                Operation::new("Tf", vec!["F1".into(), 24.into()]),
                Operation::new("Td", vec![72.into(), 700.into()]),
                Operation::new(
                    "Tj",
                    vec![Object::string_literal(format!("MARK {tag} {i}"))],
                ),
                Operation::new("ET", vec![]),
            ],
        };
        let c = pdf.add_object(Stream::new(dictionary! {}, content.encode().unwrap()));
        ids.push(pdf.add_object(dictionary! {
            "Type" => "Page", "Parent" => pages_id, "Contents" => c,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "Resources" => dictionary! { "Font" => dictionary! { "F1" => font } },
        }));
    }
    pdf.objects.insert(
        pages_id,
        dictionary! {
            "Type" => "Pages", "Count" => n as i64,
            "Kids" => ids.iter().map(|&i| Object::Reference(i)).collect::<Vec<_>>(),
        }
        .into(),
    );
    let catalog = pdf.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    pdf.trailer.set("Root", catalog);
    extra(&mut pdf, &ids);
    let path = dir.join(name);
    pdf.save(&path).unwrap();
    path
}

/// A link on `from` to `to`.
fn link(pdf: &mut Pdf, from: ObjectId, to: ObjectId) {
    let l = pdf.add_object(dictionary! {
        "Type" => "Annot", "Subtype" => "Link",
        "Rect" => vec![72.into(), 72.into(), 144.into(), 96.into()],
        "Dest" => vec![to.into(), "Fit".into()],
    });
    let page = pdf.get_dictionary_mut(from).unwrap();
    let mut annots = page
        .get(b"Annots")
        .and_then(|a| a.as_array())
        .cloned()
        .unwrap_or_default();
    annots.push(l.into());
    page.set("Annots", annots);
}

fn catalog(pdf: &mut Pdf) -> &mut lopdf::Dictionary {
    let id = pdf.trailer.get(b"Root").unwrap().as_reference().unwrap();
    pdf.get_dictionary_mut(id).unwrap()
}

/// The file at `path`, opened as the app opens it: its form, labels and
/// the rest read, which a save writes from the state.
fn opened(path: &Path) -> Document {
    crate::open::file(path, None).unwrap().doc
}

/// Whether any stream in the file says `text`, decompressed.
fn says(bytes: &[u8], text: &str) -> bool {
    let pdf = Pdf::load_mem(bytes).unwrap();
    pdf.objects.values().any(|o| match o {
        Object::Stream(s) => {
            let data = s
                .decompressed_content()
                .unwrap_or_else(|_| s.content.clone());
            String::from_utf8_lossy(&data).contains(text)
        }
        _ => false,
    })
}

fn page_annots(bytes: &[u8], page: u32) -> usize {
    let pdf = Pdf::load_mem(bytes).unwrap();
    let id = pdf.get_pages()[&(page + 1)];
    pdf.get_dictionary(id)
        .unwrap()
        .get(b"Annots")
        .ok()
        .and_then(|a| pdf.dereference(a).ok())
        .and_then(|(_, a)| a.as_array().ok().map(|a| a.len()))
        .unwrap_or(0)
}

/// How a test saves.
#[derive(Debug, Clone, Copy)]
enum Way {
    /// A save, to the file itself.
    Save,
    /// The bytes a save to another file writes ([`rewrite`]).
    Rewrite,
}

const WAYS: [Way; 2] = [Way::Save, Way::Rewrite];

/// `model` written as `way` says, `path` the file it opened.
fn saved(model: &mut Document, path: &Path, way: Way) -> Vec<u8> {
    match way {
        Way::Save => {
            save(model, path, &AskAnswers::default(), &NoProgress).unwrap();
            std::fs::read(path).unwrap()
        }
        Way::Rewrite => rewrite(&model.snapshot()).unwrap(),
    }
}

#[test]
fn a_deleted_page_leaves_the_file_whatever_points_at_it() {
    for way in WAYS {
        let dir = tempdir();
        let path = file(&dir, "a.pdf", "A", 3, |pdf, p| {
            // Page 1 links to page 3, by a link, a name, the old /Dests
            // and the open action.
            link(pdf, p[0], p[2]);
            let names = pdf.add_object(dictionary! {
                "Names" => vec![Object::string_literal("end"), Object::Array(vec![p[2].into(), "Fit".into()]),
                                Object::string_literal("start"), Object::Array(vec![p[0].into(), "Fit".into()])],
            });
            let dests =
                pdf.add_object(dictionary! { "last" => vec![Object::from(p[2]), "Fit".into()] });
            let named = pdf.add_object(dictionary! {
                "Type" => "Annot", "Subtype" => "Link",
                "Rect" => vec![72.into(), 200.into(), 144.into(), 220.into()],
                "Dest" => Object::string_literal("end"),
            });
            let c = catalog(pdf);
            c.set("Names", dictionary! { "Dests" => names });
            c.set("Dests", dests);
            c.set("OpenAction", vec![Object::from(p[2]), "Fit".into()]);
            let page = pdf.get_dictionary_mut(p[0]).unwrap();
            let mut a = page.get(b"Annots").unwrap().as_array().unwrap().clone();
            a.push(named.into());
            page.set("Annots", a);
        });
        let mut model = opened(&path);
        let third = model.page(2).unwrap();
        model.commit_ops(vec![fx::delete(&[third])], "del").unwrap();
        let bytes = saved(&mut model, &path, way);
        assert!(!says(&bytes, "MARK A 2"), "{way:?}");
        assert!(says(&bytes, "MARK A 0") && says(&bytes, "MARK A 1"));
        // Both links led to it: they're gone. So are the name and the open
        // action; the name for page 1 stays.
        assert_eq!(page_annots(&bytes, 0), 0, "{way:?}");
        let pdf = Pdf::load_mem(&bytes).unwrap();
        let c = pdf.catalog().unwrap();
        assert!(c.get(b"OpenAction").is_err(), "{way:?}");
        let names = pdf
            .dereference(c.get(b"Names").unwrap())
            .unwrap()
            .1
            .as_dict()
            .unwrap()
            .get(b"Dests")
            .unwrap();
        let tree = pdf.dereference(names).unwrap().1.as_dict().unwrap();
        assert_eq!(tree.get(b"Names").unwrap().as_array().unwrap().len(), 2);
        let dests = pdf
            .dereference(c.get(b"Dests").unwrap())
            .unwrap()
            .1
            .as_dict()
            .unwrap();
        assert!(dests.is_empty());
        assert_eq!(super::page_count_reachable(&pdf), 2);
    }
}

/// Whether the file holds `text` anywhere: in its bytes, or a stream's.
fn holds(bytes: &[u8], text: &str) -> bool {
    bytes.windows(text.len()).any(|w| w == text.as_bytes()) || says(bytes, text)
}

#[test]
fn a_reply_keeps_no_comment_of_a_deleted_page() {
    // A note on page 2, and a reply to it on page 1: a reply's /IRT is the
    // one way left to reach the note once its page goes.
    for way in WAYS {
        let dir = tempdir();
        let path = file(&dir, "a.pdf", "A", 2, |pdf, p| {
            let note = pdf.add_object(dictionary! {
                "Type" => "Annot", "Subtype" => "Text", "P" => p[1],
                "Rect" => vec![72.into(), 72.into(), 96.into(), 96.into()],
                "Contents" => Object::string_literal("SHEAFLEAK note"),
            });
            pdf.get_dictionary_mut(p[1])
                .unwrap()
                .set("Annots", vec![note.into()]);
            let reply = pdf.add_object(dictionary! {
                "Type" => "Annot", "Subtype" => "Text", "P" => p[0], "IRT" => note,
                "Rect" => vec![72.into(), 72.into(), 96.into(), 96.into()],
                "Contents" => Object::string_literal("a reply"),
            });
            pdf.get_dictionary_mut(p[0])
                .unwrap()
                .set("Annots", vec![reply.into()]);
        });
        let mut model = opened(&path);
        let second = model.page(1).unwrap();
        model
            .commit_ops(vec![fx::delete(&[second])], "del")
            .unwrap();
        let bytes = saved(&mut model, &path, way);
        assert!(!holds(&bytes, "SHEAFLEAK"), "{way:?}");
        assert!(holds(&bytes, "a reply"), "{way:?}");
        assert_eq!(page_annots(&bytes, 0), 1, "{way:?}");
    }
}

crate::both_ways!(
    inserting_one_page_brings_only_that_page,
    inserting_one_page_brings_only_that_page_appended: inserting_one_page
);

fn inserting_one_page(how: How) {
    let dir = tempdir();
    let a = file(&dir, "a.pdf", "A", 1, |_, _| {});
    let b = file(&dir, "b.pdf", "B", 3, |pdf, p| {
        link(pdf, p[0], p[2]);
        link(pdf, p[0], p[1]);
    });
    let mut model = opened(&a);
    let (sb, _) = model.store().read_file(&b, None).unwrap();
    // B's first and third pages: the link between them stays, the one
    // to its second goes, and the second isn't in the file.
    let insert = fx::insert(&sb, [0, 2], "b.pdf", None);
    model.commit_ops(vec![insert], "ins").unwrap();
    let bytes = how.write(&model);
    assert!(!says(&bytes, "MARK B 1"), "{how:?}");
    assert!(says(&bytes, "MARK B 0") && says(&bytes, "MARK B 2"));
    let pdf = Pdf::load_mem(&bytes).unwrap();
    let pages = pdf.get_pages();
    let b0 = pages[&2];
    let annots = pdf
        .get_dictionary(b0)
        .unwrap()
        .get(b"Annots")
        .unwrap()
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(annots.len(), 1, "{how:?}");
    let link = pdf.dereference(&annots[0]).unwrap().1.as_dict().unwrap();
    let to = link.get(b"Dest").unwrap().as_array().unwrap()[0]
        .as_reference()
        .unwrap();
    assert_eq!(to, pages[&3]);
    assert_eq!(super::page_count_reachable(&pdf), 3);
}

#[test]
fn fields_tags_and_threads_let_go_of_a_deleted_page() {
    let dir = tempdir();
    let path = file(&dir, "a.pdf", "A", 3, |pdf, p| {
        // A field with a widget on pages 1 and 3, and one only on page 3.
        let widget = |pdf: &mut Pdf, page: ObjectId, parent: Option<ObjectId>| {
            let mut w = dictionary! {
                "Type" => "Annot", "Subtype" => "Widget", "P" => page,
                "Rect" => vec![72.into(), 300.into(), 200.into(), 320.into()],
            };
            if let Some(parent) = parent {
                w.set("Parent", parent);
            } else {
                w.set("FT", "Tx");
                w.set("T", Object::string_literal("only3"));
            }
            let id = pdf.add_object(w);
            let d = pdf.get_dictionary_mut(page).unwrap();
            let mut a = d
                .get(b"Annots")
                .and_then(|a| a.as_array())
                .cloned()
                .unwrap_or_default();
            a.push(id.into());
            d.set("Annots", a);
            id
        };
        let both = pdf.new_object_id();
        let w0 = widget(pdf, p[0], Some(both));
        let w2 = widget(pdf, p[2], Some(both));
        pdf.objects.insert(
            both,
            dictionary! { "FT" => "Tx", "T" => Object::string_literal("both"), "Kids" => vec![w0.into(), w2.into()] }.into(),
        );
        let only = widget(pdf, p[2], None);
        // Tags: a paragraph on each page, under a document element.
        let root = pdf.new_object_id();
        let doc = pdf.new_object_id();
        let para = |pdf: &mut Pdf, page: ObjectId| {
            pdf.add_object(dictionary! { "Type" => "StructElem", "S" => "P", "P" => doc, "Pg" => page, "K" => 0 })
        };
        let e0 = para(pdf, p[0]);
        let e2 = para(pdf, p[2]);
        pdf.objects.insert(
            doc,
            dictionary! { "Type" => "StructElem", "S" => "Document", "P" => root, "K" => vec![e0.into(), e2.into()] }.into(),
        );
        let parents = pdf.add_object(dictionary! {
            "Nums" => vec![0.into(), Object::Array(vec![e0.into()]), 1.into(), Object::Array(vec![e2.into()])],
        });
        pdf.objects.insert(
            root,
            dictionary! { "Type" => "StructTreeRoot", "K" => doc, "ParentTree" => parents }.into(),
        );
        // An article thread through pages 2 and 3.
        let thread = pdf.new_object_id();
        let (b1, b2) = (pdf.new_object_id(), pdf.new_object_id());
        let r = || vec![0.into(), 0.into(), 100.into(), 100.into()];
        pdf.objects.insert(
            b1,
            dictionary! { "T" => thread, "N" => b2, "V" => b2, "P" => p[1], "R" => r() }.into(),
        );
        pdf.objects.insert(
            b2,
            dictionary! { "N" => b1, "V" => b1, "P" => p[2], "R" => r() }.into(),
        );
        pdf.objects
            .insert(thread, dictionary! { "Type" => "Thread", "F" => b1 }.into());
        pdf.get_dictionary_mut(p[1])
            .unwrap()
            .set("B", vec![Object::from(b1)]);
        pdf.get_dictionary_mut(p[2])
            .unwrap()
            .set("B", vec![Object::from(b2)]);
        for (i, page) in p.iter().enumerate() {
            pdf.get_dictionary_mut(*page)
                .unwrap()
                .set("StructParents", i as i64);
        }
        let c = catalog(pdf);
        c.set(
            "AcroForm",
            dictionary! { "Fields" => vec![both.into(), only.into()] },
        );
        c.set("StructTreeRoot", root);
        c.set("MarkInfo", dictionary! { "Marked" => true });
        c.set("Threads", vec![Object::from(thread)]);
    });
    let mut model = opened(&path);
    let third = model.page(2).unwrap();
    model.commit_ops(vec![fx::delete(&[third])], "del").unwrap();
    let_go_of_the_third_page(&rewrite(&model.snapshot()).unwrap());
}

/// What [`fields_tags_and_threads_let_go_of_a_deleted_page`] checks of a
/// file written without its third page.
fn let_go_of_the_third_page(bytes: &[u8]) {
    let bytes = bytes.to_vec();
    assert!(!says(&bytes, "MARK A 2"));
    let pdf = Pdf::load_mem(&bytes).unwrap();
    assert_eq!(super::page_count_reachable(&pdf), 2);
    let c = pdf.catalog().unwrap();
    // The field on both keeps page 1's widget; the one only on page 3 is gone.
    let form = pdf
        .dereference(c.get(b"AcroForm").unwrap())
        .unwrap()
        .1
        .as_dict()
        .unwrap();
    let fields = form.get(b"Fields").unwrap().as_array().unwrap();
    assert_eq!(fields.len(), 1);
    let both = pdf.dereference(&fields[0]).unwrap().1.as_dict().unwrap();
    assert_eq!(both.get(b"Kids").unwrap().as_array().unwrap().len(), 1);
    // The tags keep page 1's paragraph; page 3's is gone from the tree and
    // the parent tree.
    let root = pdf
        .dereference(c.get(b"StructTreeRoot").unwrap())
        .unwrap()
        .1
        .as_dict()
        .unwrap();
    let doc = pdf
        .dereference(root.get(b"K").unwrap())
        .unwrap()
        .1
        .as_dict()
        .unwrap();
    assert_eq!(doc.get(b"K").unwrap().as_array().unwrap().len(), 1);
    let parents = pdf
        .dereference(root.get(b"ParentTree").unwrap())
        .unwrap()
        .1
        .as_dict()
        .unwrap();
    assert_eq!(parents.get(b"Nums").unwrap().as_array().unwrap().len(), 2);
    // The thread through page 3 is dropped, and page 2's bead of it.
    assert!(c.get(b"Threads").is_err());
    let page2 = pdf.get_dictionary(pdf.get_pages()[&2]).unwrap();
    assert!(page2.get(b"B").is_err());
}

#[test]
fn copies_hold_only_the_pages_copied() {
    let dir = tempdir();
    let path = file(&dir, "a.pdf", "A", 3, |pdf, p| {
        link(pdf, p[0], p[2]);
        link(pdf, p[0], p[1]);
    });
    let model = opened(&path);
    let (p0, p1) = (model.page(0).unwrap(), model.page(1).unwrap());
    // Extract pages 1 and 2: page 3 doesn't come, nor does the link to it.
    let bytes = crate::save::extract_bytes(&model.snapshot(), &[p0, p1]).unwrap();
    assert!(!says(&bytes, "MARK A 2") && says(&bytes, "MARK A 1"));
    assert_eq!(page_annots(&bytes, 0), 1);
    // A crop is a copy of the page alone.
    let src = model.snapshot().primary;
    let bytes = crate::save::cropped_copy(&src, &[(0, [0.0, 0.0, 300.0, 300.0])]).unwrap();
    assert!(!says(&bytes, "MARK A 1") && !says(&bytes, "MARK A 2"));
    assert_eq!(page_annots(&bytes, 0), 0);
    // So is a stamp made from a page, and the forms the summary draws.
    let pdf = Pdf::load(&path).unwrap();
    let stamp = crate::stamp_lib::one_page(&pdf, 0).unwrap();
    assert!(!says(&stamp, "MARK A 1") && !says(&stamp, "MARK A 2"));
    let mut dst = Pdf::with_version("1.5");
    let first = pdf.get_pages()[&1];
    crate::stamp_lib::page_forms(&pdf, &[first], &mut dst).unwrap();
    let mut out = Vec::new();
    dst.save_to(&mut out).unwrap();
    assert!(says(&out, "MARK A 0") && !says(&out, "MARK A 2"));
}

fn note(text: &str) -> Annot {
    Annot {
        id: AnnotId(0),
        kind: AnnotKind::Note {
            pos: Pt { x: 100.0, y: 100.0 },
        },
        color: [255, 230, 0],
        opacity: 1.0,
        stroke: 1.0,
        author: "Greg".into(),
        contents: text.into(),
        modified: String::new(),
        created: String::new(),
        fill: None,
        dashed: false,
        cloudy: false,
        turn: Rotation::R0,
        name: String::new(),
    }
}

// ----- Removed text, every feature both ways (M10.5 slice 10) ---------------

/// The bytes `text` may be written as: itself, and in UTF-16 (a text
/// string's other encoding, and the codes OCR text is shown in), each also
/// as a hex string's digits, in either case.
fn written_as(text: &str) -> Vec<Vec<u8>> {
    let utf16: Vec<u8> = text.encode_utf16().flat_map(u16::to_be_bytes).collect();
    let mut out = Vec::new();
    for bytes in [text.as_bytes().to_vec(), utf16] {
        let hex: String = bytes.iter().map(|b| format!("{b:02X}")).collect();
        out.push(hex.to_lowercase().into_bytes());
        out.push(hex.into_bytes());
        out.push(bytes);
    }
    out
}

/// Whether `text` can be read in the file `bytes`, as any of the bytes
/// [`written_as`] gives: in the file as it is, in a stream decoded (an
/// object stream's too) or in a string any object holds, decoded. Every
/// object of the file's newest revision, which is all of a Rewrite's.
fn readable(bytes: &[u8], text: &str) -> bool {
    let forms = written_as(text);
    let found = |hay: &[u8]| (forms.iter()).any(|f| hay.windows(f.len()).any(|w| w == &f[..]));
    found(bytes) || (Pdf::load_mem(bytes).unwrap().objects.values()).any(|o| holds_in(o, &found))
}

/// Whether `found` finds what it looks for in `o`: in its strings, and its
/// stream's data decoded.
fn holds_in(o: &Object, found: &dyn Fn(&[u8]) -> bool) -> bool {
    match o {
        Object::String(s, _) => found(s),
        Object::Array(items) => items.iter().any(|o| holds_in(o, found)),
        Object::Dictionary(d) => d.iter().any(|(_, o)| holds_in(o, found)),
        Object::Stream(s) => {
            let data = (s.decompressed_content()).unwrap_or_else(|_| s.content.clone());
            found(&data) || s.dict.iter().any(|(_, o)| holds_in(o, found))
        }
        _ => false,
    }
}

/// `model` saved to `path` `how`, whatever the plan would decide: `text`
/// can be read in the file.
fn saved_first(how: How, model: &mut Document, path: &Path, text: &str) {
    let first = how.save(model, path);
    assert_eq!(first.strategy, Some(how.strategy()));
    assert!(
        readable(&std::fs::read(path).unwrap(), text),
        "{how:?}: {text}"
    );
}

/// `model` saved to `path` as the plan says, after `text` went from it: a
/// Rewrite, which the removal check, run over it as a test oracle (§11.9),
/// finds took text out of the file, and after which `text` can't be read
/// in the file. Why it rewrote.
fn saved_without(model: &mut Document, path: &Path, text: &str) -> Vec<Why> {
    let v = model.current_version();
    let input = model.save_input(v).unwrap();
    let target = model.save_target(path);
    // The Rewrite the save writes, whether planned or the Append the
    // removal check stops.
    let rewrite = forced_rewrite(&input, &target);
    let out = materialize(&input, &rewrite, target, &NoProgress).unwrap();
    let file = std::fs::read(path).unwrap().into();
    let found = removal::of_rewrite(&input, &out, file).unwrap();
    assert!(found.is_some(), "the removal check finds {text} gone");
    model.abandon_save(v);
    let report = save(model, path, &AskAnswers::default(), &NoProgress).unwrap();
    assert_eq!(report.strategy, Some(StrategyKind::Rewrite));
    assert!(!readable(&std::fs::read(path).unwrap(), text), "{text}");
    report.why
}

/// `count` of `kind`, as a save's reasons say what went.
fn gone(kind: RemovedKind, count: usize) -> Vec<Removed> {
    vec![Removed { kind, count }]
}

crate::both_ways!(
    a_deleted_page_is_gone_from_the_saved_bytes,
    a_deleted_page_is_gone_from_the_saved_bytes_appended: deleted_page_gone
);

/// A page brought in from another file and saved, then deleted: the next
/// save is a Rewrite, and what the page said is gone from the file.
fn deleted_page_gone(how: How) {
    let dir = tempdir();
    let path = file(&dir, "a.pdf", "A", 2, |_, _| {});
    let other = file(&dir, "b.pdf", "PAGESECRET", 1, |_, _| {});
    let mut model = opened(&path);
    let (other, _) = model.store().read_file(&other, None).unwrap();
    let insert = fx::insert(&other, [0], "b.pdf", None);
    model.commit_ops(vec![insert], "insert").unwrap();
    saved_first(how, &mut model, &path, "MARK PAGESECRET");
    let page = model.page(2).unwrap();
    model
        .commit_ops(vec![fx::delete(&[page])], "delete")
        .unwrap();
    let why = saved_without(&mut model, &path, "MARK PAGESECRET");
    assert_eq!(why, [Why::Removes(gone(RemovedKind::Pages, 1))]);
}

crate::both_ways!(
    a_deleted_mark_is_gone_from_the_saved_bytes,
    a_deleted_mark_is_gone_from_the_saved_bytes_appended: deleted_mark_gone
);

/// A mark deleted after a save is gone from the next save's bytes: the
/// second save is a Rewrite, whatever the first was.
fn deleted_mark_gone(how: How) {
    let dir = tempdir();
    let path = file(&dir, "a.pdf", "A", 1, |_, _| {});
    let mut model = opened(&path);
    let p0 = model.page(0).unwrap();
    let add = fx::add(p0, &note("SECRET-NOTE-TEXT"));
    model.commit_ops(vec![add], "note").unwrap();
    saved_first(how, &mut model, &path, "SECRET-NOTE-TEXT");
    let id = model.state().marks_on(p0).next().unwrap().id;
    model
        .commit_ops(vec![fx::delete_annots(&[id])], "delete")
        .unwrap();
    let why = saved_without(&mut model, &path, "SECRET-NOTE-TEXT");
    assert_eq!(why, [Why::Removes(gone(RemovedKind::Comments, 1))]);
}

crate::both_ways!(
    a_deleted_reply_is_gone_from_the_saved_bytes,
    a_deleted_reply_is_gone_from_the_saved_bytes_appended: deleted_reply_gone
);

/// A reply to a note, saved, then deleted: gone from the next save, the
/// note it answered kept.
fn deleted_reply_gone(how: How) {
    let dir = tempdir();
    let path = file(&dir, "a.pdf", "A", 1, |_, _| {});
    let mut model = opened(&path);
    let p0 = model.page(0).unwrap();
    let add = fx::add(p0, &note("the note"));
    model.commit_ops(vec![add], "note").unwrap();
    let parent = model.state().marks_on(p0).next().unwrap().name();
    let reply = Annot {
        kind: AnnotKind::Reply { parent },
        ..note("SECRET-REPLY-TEXT")
    };
    let add = fx::add(p0, &reply);
    model.commit_ops(vec![add], "reply").unwrap();
    saved_first(how, &mut model, &path, "SECRET-REPLY-TEXT");
    let id = (model.state().marks_on(p0))
        .find(|a| a.contents == "SECRET-REPLY-TEXT")
        .unwrap()
        .id;
    model
        .commit_ops(vec![fx::delete_annots(&[id])], "delete")
        .unwrap();
    let why = saved_without(&mut model, &path, "SECRET-REPLY-TEXT");
    assert_eq!(why, [Why::Removes(gone(RemovedKind::Comments, 1))]);
    assert!(readable(&std::fs::read(&path).unwrap(), "the note"));
}

crate::both_ways!(
    another_tool_s_deleted_comment_is_gone_from_the_saved_bytes,
    another_tool_s_deleted_comment_is_gone_from_the_saved_bytes_appended: deleted_theirs_gone
);

/// Another tool's comment, in the file when it opened, kept by a save of
/// a note beside it, then deleted: gone from the next save.
fn deleted_theirs_gone(how: How) {
    let dir = tempdir();
    let path = file(&dir, "a.pdf", "A", 1, |pdf, p| {
        let theirs = pdf.add_object(dictionary! {
            "Type" => "Annot", "Subtype" => "Text", "NM" => Object::string_literal("theirs"),
            "Rect" => vec![72.into(), 72.into(), 96.into(), 96.into()],
            "Contents" => Object::string_literal("THEIR-SECRET-TEXT"),
        });
        pdf.get_dictionary_mut(p[0])
            .unwrap()
            .set("Annots", vec![theirs.into()]);
    });
    let mut model = opened(&path);
    let p0 = model.page(0).unwrap();
    let add = fx::add(p0, &note("ours"));
    model.commit_ops(vec![add], "note").unwrap();
    saved_first(how, &mut model, &path, "THEIR-SECRET-TEXT");
    let delete = fx::delete_named(p0, &["theirs"]);
    model.commit_ops(vec![delete], "delete").unwrap();
    let why = saved_without(&mut model, &path, "THEIR-SECRET-TEXT");
    assert_eq!(why, [Why::Removes(gone(RemovedKind::Comments, 1))]);
}

crate::both_ways!(
    a_deleted_bookmark_is_gone_from_the_saved_bytes,
    a_deleted_bookmark_is_gone_from_the_saved_bytes_appended: deleted_bookmark_gone
);

/// A bookmark saved, then deleted: gone from the next save.
fn deleted_bookmark_gone(how: How) {
    let dir = tempdir();
    let path = file(&dir, "a.pdf", "A", 2, |_, _| {});
    let mut model = opened(&path);
    let insert = fx::bookmark(None, None, "SECRET-BOOKMARK", Some(model.page(1).unwrap()));
    model.commit_ops(vec![insert], "bookmark").unwrap();
    saved_first(how, &mut model, &path, "SECRET-BOOKMARK");
    let id = model.state().bookmarks_preorder()[0].1;
    model
        .commit_ops(vec![fx::delete_bookmark(id)], "delete")
        .unwrap();
    let why = saved_without(&mut model, &path, "SECRET-BOOKMARK");
    assert_eq!(why, [Why::Removes(gone(RemovedKind::Bookmarks, 1))]);
}

crate::both_ways!(
    a_mark_s_replaced_text_is_gone_from_the_saved_bytes,
    a_mark_s_replaced_text_is_gone_from_the_saved_bytes_appended: replaced_mark_text
);

/// A note saved, then its text replaced: the next save is a Rewrite, with
/// the new text and without the old.
fn replaced_mark_text(how: How) {
    let dir = tempdir();
    let path = file(&dir, "a.pdf", "A", 1, |_, _| {});
    let mut model = opened(&path);
    let p0 = model.page(0).unwrap();
    let add = fx::add(p0, &note("OLD-NOTE-TEXT"));
    model.commit_ops(vec![add], "note").unwrap();
    saved_first(how, &mut model, &path, "OLD-NOTE-TEXT");
    let annot = Annot {
        contents: "NEW-NOTE-TEXT".into(),
        ..model.state().marks_on(p0).next().unwrap().clone()
    };
    model.commit_ops(vec![fx::update(&annot)], "edit").unwrap();
    let why = saved_without(&mut model, &path, "OLD-NOTE-TEXT");
    assert_eq!(why, [Why::ReplacesText(gone(RemovedKind::CommentText, 1))]);
    assert!(readable(&std::fs::read(&path).unwrap(), "NEW-NOTE-TEXT"));
}

/// OCR text saying `text`.
fn ocr_layer(text: &str) -> std::sync::Arc<crate::text_layer::TextLayer> {
    use crate::text_layer::OcrWord;
    std::sync::Arc::new(crate::text_layer::from_engine(
        vec![OcrWord {
            text: text.into(),
            rect: crate::annot::Box2 {
                x: 72.0,
                y: 100.0,
                w: 60.0,
                h: 12.0,
            },
            confidence: 0.9,
        }],
        &crate::geom::Geom::read(None, None, 0),
        "test",
    ))
}

/// How many streams of the file `pdf` hold an OCR layer's text (not the
/// `q` that sets it apart).
fn ocr_layers(pdf: &[u8]) -> usize {
    (Pdf::load_mem(pdf).unwrap().objects.values())
        .filter(|o| {
            o.as_stream().is_ok_and(|s| {
                s.dict.has(b"SheafOcrLayer")
                    && s.decompressed_content()
                        .unwrap_or_else(|_| s.content.clone())
                        != b"q\n"
            })
        })
        .count()
}

crate::both_ways!(
    replacing_saved_ocr_text_takes_the_old_out,
    replacing_saved_ocr_text_takes_the_old_out_appended: replaced_ocr_text
);

/// A page recognized again after a save has one layer in the next save,
/// the new one: the old text is gone from the bytes.
fn replaced_ocr_text(how: How) {
    let dir = tempdir();
    let path = file(&dir, "a.pdf", "A", 1, |_, _| {});
    let mut model = opened(&path);
    let p0 = model.page(0).unwrap();
    let ocr = |text: &str| fx::ocr([(p0, ocr_layer(text))]);
    model.commit_ops(ocr("OCRFIRSTWORD"), "ocr").unwrap();
    saved_first(how, &mut model, &path, "OCRFIRSTWORD");
    assert_eq!(ocr_layers(&std::fs::read(&path).unwrap()), 1);
    // Recognized again: the old text would stay in an earlier revision,
    // so it's a full save.
    model.commit_ops(ocr("OCRSECONDWORD"), "ocr").unwrap();
    let why = saved_without(&mut model, &path, "OCRFIRSTWORD");
    assert_eq!(why, [Why::ReplacesText(gone(RemovedKind::OcrLayers, 1))]);
    let now = std::fs::read(&path).unwrap();
    assert_eq!(ocr_layers(&now), 1);
    assert!(readable(&now, "OCRSECONDWORD"));
}

crate::both_ways!(
    a_re_ocr_over_an_earlier_session_s_layer_takes_the_old_out,
    a_re_ocr_over_an_earlier_session_s_layer_takes_the_old_out_appended: re_ocr_reopened
);

/// A layer saved in an earlier session, part of the page's own content
/// once the file opens again, recognized over: the removal check finds
/// the old words in what the Append would unlink, so the save is a
/// Rewrite, which takes them out.
fn re_ocr_reopened(how: How) {
    let dir = tempdir();
    let path = file(&dir, "a.pdf", "A", 1, |_, _| {});
    let mut model = opened(&path);
    let layer = |model: &Document, text: &str| fx::ocr([(model.page(0).unwrap(), ocr_layer(text))]);
    model
        .commit_ops(layer(&model, "OCROLDWORD"), "ocr")
        .unwrap();
    saved_first(how, &mut model, &path, "OCROLDWORD");
    let mut model = opened(&path);
    model
        .commit_ops(layer(&model, "OCRNEWWORD"), "ocr")
        .unwrap();
    let why = saved_without(&mut model, &path, "OCROLDWORD");
    assert!(matches!(why[..], [Why::RemovalCheck { .. }]), "{why:?}");
    let now = std::fs::read(&path).unwrap();
    assert_eq!(ocr_layers(&now), 1);
    assert!(readable(&now, "OCRNEWWORD"));
}

/// `keep_only` names every object it changed, and those it added, and
/// no other: what a Rewrite can't copy as the file had it (M10.5 §11.6).
/// Over a file whose third page goes, with a link, a named destination,
/// the open action, fields, tags, an article thread and a stray reference
/// to it.
#[test]
fn keep_only_names_what_it_changed() {
    let dir = tempdir();
    let path = file(&dir, "a.pdf", "A", 3, |pdf, p| {
        link(pdf, p[0], p[2]);
        let widget = |pdf: &mut Pdf, page: ObjectId, parent: ObjectId| {
            let id = pdf.add_object(dictionary! {
                "Type" => "Annot", "Subtype" => "Widget", "P" => page, "Parent" => parent,
                "Rect" => vec![72.into(), 300.into(), 200.into(), 320.into()],
            });
            let d = pdf.get_dictionary_mut(page).unwrap();
            let mut a = (d.get(b"Annots").and_then(|a| a.as_array()))
                .cloned()
                .unwrap_or_default();
            a.push(id.into());
            d.set("Annots", a);
            id
        };
        let (both, only) = (pdf.new_object_id(), pdf.new_object_id());
        let kids = vec![
            widget(pdf, p[0], both).into(),
            widget(pdf, p[2], both).into(),
        ];
        pdf.objects.insert(
            both,
            dictionary! { "FT" => "Tx", "T" => Object::string_literal("both"), "Kids" => kids }
                .into(),
        );
        let kids = vec![widget(pdf, p[2], only).into()];
        pdf.objects.insert(
            only,
            dictionary! { "FT" => "Tx", "T" => Object::string_literal("only"), "Kids" => kids }
                .into(),
        );
        let form = pdf.add_object(dictionary! { "Fields" => vec![both.into(), only.into()] });
        let (root, doc) = (pdf.new_object_id(), pdf.new_object_id());
        let para = |pdf: &mut Pdf, page: ObjectId| {
            pdf.add_object(dictionary! { "Type" => "StructElem", "S" => "P", "P" => doc, "Pg" => page, "K" => 0 })
        };
        let (e0, e2) = (para(pdf, p[0]), para(pdf, p[2]));
        pdf.objects.insert(
            doc,
            dictionary! { "Type" => "StructElem", "S" => "Document", "P" => root, "K" => vec![e0.into(), e2.into()] }.into(),
        );
        let parents = pdf.add_object(dictionary! {
            "Nums" => vec![0.into(), Object::Array(vec![e0.into()]), 2.into(), Object::Array(vec![e2.into()])],
        });
        pdf.objects.insert(
            root,
            dictionary! { "Type" => "StructTreeRoot", "K" => doc, "ParentTree" => parents }.into(),
        );
        let thread = pdf.new_object_id();
        let (b1, b2) = (pdf.new_object_id(), pdf.new_object_id());
        let r = || vec![0.into(), 0.into(), 100.into(), 100.into()];
        pdf.objects.insert(
            b1,
            dictionary! { "T" => thread, "N" => b2, "V" => b2, "P" => p[1], "R" => r() }.into(),
        );
        pdf.objects.insert(
            b2,
            dictionary! { "N" => b1, "V" => b1, "P" => p[2], "R" => r() }.into(),
        );
        pdf.objects
            .insert(thread, dictionary! { "Type" => "Thread", "F" => b1 }.into());
        for (page, bead) in [(p[1], b1), (p[2], b2)] {
            pdf.get_dictionary_mut(page)
                .unwrap()
                .set("B", vec![Object::from(bead)]);
        }
        let stray = pdf.add_object(dictionary! { "Page" => p[2] });
        let dests = dictionary! { "Names" => vec![Object::string_literal("third"), vec![p[2].into(), "Fit".into()].into()] };
        let c = catalog(pdf);
        c.set("AcroForm", form);
        c.set("StructTreeRoot", root);
        c.set("Threads", vec![Object::from(thread)]);
        c.set("Names", dictionary! { "Dests" => dests });
        c.set("OpenAction", vec![p[2].into(), "Fit".into()]);
        c.set("Stray", stray);
    });
    let mut pdf = Pdf::load(&path).unwrap();
    let pages: Vec<ObjectId> = pdf.get_pages().into_values().collect();
    // The tree as a Rewrite leaves it: the first two pages.
    let tree = catalog(&mut pdf)
        .get(b"Pages")
        .unwrap()
        .as_reference()
        .unwrap();
    let kids: Vec<Object> = pages[..2].iter().map(|&p| p.into()).collect();
    let node = pdf.get_dictionary_mut(tree).unwrap();
    node.set("Kids", kids);
    node.set("Count", 2);
    let before = pdf.objects.clone();
    let kept = super::keep_only(&mut pdf, &pages[..2].iter().copied().collect());
    let changed: std::collections::BTreeSet<ObjectId> = (pdf.objects.iter())
        .filter(|(id, o)| before.get(id) != Some(o))
        .map(|(id, _)| *id)
        .collect();
    assert_eq!(kept.modified, changed);
    // What changed: the first page (its link and its widget's field stay,
    // the link goes), the second (its bead), the catalog, the form, the
    // field on both, the tags, and the stray reference; the names moved
    // into objects of their own, which were added.
    let catalog_id = pdf.trailer.get(b"Root").unwrap().as_reference().unwrap();
    for id in [pages[0], pages[1], catalog_id] {
        assert!(kept.modified.contains(&id), "{id:?}");
    }
    assert!(
        kept.modified.iter().any(|id| !before.contains_key(id)),
        "added"
    );
    // What didn't: the pages' content.
    let content = |p: ObjectId| {
        before[&p]
            .as_dict()
            .unwrap()
            .get(b"Contents")
            .unwrap()
            .as_reference()
            .unwrap()
    };
    assert!(!kept.modified.contains(&content(pages[0])));
    let _ = std::fs::remove_dir_all(&dir);
}
