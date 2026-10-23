use super::*;
use crate::annot::{Annot, AnnotKind, Pt};
use crate::model::Document;
use crate::prim::Key;
use crate::tx::fixture::add;
use crate::tx::{Actor, CommitOpts, EditError, JobKind, Stale};
use in_progress::NoProgress;
use lopdf::{Document as Pdf, Stream, dictionary};
use sheaf_edit::Rotation;

/// Comments a test makes, each on its page, as a data file holds them.
#[derive(Default)]
struct Making {
    built: Built,
    annots: Vec<(u32, ObjectId)>,
}

impl Making {
    fn add(&mut self, page: u32, d: Dictionary) -> ObjectId {
        let id = self.built.pdf().add_object(Object::Dictionary(d));
        self.annots.push((page, id));
        id
    }

    /// The comments made, on `file`.
    fn comments(self, file: Option<&str>) -> Comments {
        Comments::built(self.built, self.annots, file.map(str::to_string)).unwrap()
    }
}

/// Comments of most kinds, as another tool writes them.
fn sample() -> Comments {
    let mut c = Making::default();
    let r = |x: i64, y: i64| vec![x.into(), y.into(), (x + 20).into(), (y + 20).into()];
    let note = c.add(
        0,
        dictionary! {
            "Type" => "Annot", "Subtype" => "Text", "Rect" => r(100, 600), "NM" => Object::string_literal("n1"),
            "T" => Object::string_literal("Ann"), "Contents" => crate::save::text_string("Tie <&> “foot”"),
            "M" => Object::string_literal("D:20261001120000"), "C" => vec![1.into(), 1.into(), 0.into()],
            "F" => 28, "Name" => "Comment", "Open" => false,
        },
    );
    c.add(
        0,
        dictionary! {
            "Type" => "Annot", "Subtype" => "Text", "Rect" => r(100, 600), "NM" => Object::string_literal("r1"),
            "IRT" => note, "T" => Object::string_literal("Bo"), "Contents" => Object::string_literal("Done"),
        },
    );
    c.add(
        0,
        dictionary! {
            "Type" => "Annot", "Subtype" => "Text", "Rect" => r(100, 600), "NM" => Object::string_literal("s1"),
            "IRT" => note, "State" => Object::string_literal("Accepted"),
            "StateModel" => Object::string_literal("Review"),
        },
    );
    c.add(
        0,
        dictionary! {
            "Type" => "Annot", "Subtype" => "Highlight", "Rect" => r(100, 680),
            "NM" => Object::string_literal("h1"), "C" => vec![1.into(), 1.into(), 0.into()], "CA" => 0.5,
            "QuadPoints" => vec![100.into(), 700.into(), 200.into(), 700.into(), 100.into(), 688.into(), 200.into(), 688.into()],
        },
    );
    c.add(
        1,
        dictionary! {
            "Type" => "Annot", "Subtype" => "Line", "Rect" => r(10, 10), "NM" => Object::string_literal("l1"),
            "L" => vec![10.into(), 10.into(), 200.into(), 150.into()],
            "LE" => vec![Object::Name(b"None".to_vec()), Object::Name(b"OpenArrow".to_vec())],
            "C" => vec![1.into(), 0.into(), 0.into()], "BS" => dictionary! { "W" => 3, "S" => "D", "D" => vec![3.into(), 3.into()] },
        },
    );
    c.add(
        1,
        dictionary! {
            "Type" => "Annot", "Subtype" => "Ink", "Rect" => r(10, 10), "NM" => Object::string_literal("i1"),
            "InkList" => vec![Object::Array(vec![10.into(), 10.into(), 20.into(), 25.into(), 40.into(), 30.into()])],
        },
    );
    c.add(
        1,
        dictionary! {
            "Type" => "Annot", "Subtype" => "Polygon", "Rect" => r(300, 300), "NM" => Object::string_literal("p1"),
            "Vertices" => vec![300.into(), 300.into(), 400.into(), 310.into(), 350.into(), 400.into()],
            "BE" => dictionary! { "S" => "C", "I" => 1 }, "IC" => vec![0.into(), 0.into(), 1.into()],
        },
    );
    c.add(
        1,
        dictionary! {
            "Type" => "Annot", "Subtype" => "FreeText", "IT" => "FreeTextCallout",
            "Rect" => vec![200.into(), 200.into(), 400.into(), 260.into()],
            "RD" => vec![50.into(), 0.into(), 0.into(), 0.into()],
            "CL" => vec![205.into(), 205.into(), 230.into(), 230.into(), 250.into(), 230.into()],
            "DA" => Object::string_literal("/Helv 10 Tf 0 0 1 rg"), "LE" => "OpenArrow",
            "NM" => Object::string_literal("f1"), "Contents" => Object::string_literal("Why?"),
        },
    );
    c.comments(Some("review.pdf"))
}

/// Each comment in `c` as the converter makes it, by name.
fn marks(c: &Comments) -> Vec<(u32, String)> {
    let mut out = Vec::new();
    for i in 0..c.len() {
        let d = c.dict(i).unwrap();
        let a = crate::import::convert(c.src.pdf(), d, Rotation::R0, Default::default()).unwrap();
        out.push((c.annots[i].0, serde_json::to_string(&a).unwrap()));
    }
    out
}

#[test]
fn xfdf_and_fdf_carry_every_kind_through() {
    let c = sample();
    let want = marks(&c);
    let xfdf = xfdf::write(&c).unwrap();
    let text = String::from_utf8(xfdf.clone()).unwrap();
    assert!(text.contains(r#"<text page="0""#), "{text}");
    assert!(
        text.contains("<contents>Tie &lt;&amp;&gt; “foot”</contents>"),
        "{text}"
    );
    assert!(text.contains(r#"inreplyto="n1""#) && text.contains(r#"<f href="review.pdf"/>"#));
    let back = xfdf::read(&xfdf).unwrap();
    assert_eq!(back.file.as_deref(), Some("review.pdf"));
    assert_eq!(marks(&back), want);

    let fdf = fdf::write(&c).unwrap();
    assert!(fdf.starts_with(b"%FDF-1.2"));
    let back = read(&fdf, None).unwrap();
    assert_eq!(back.file.as_deref(), Some("review.pdf"));
    assert_eq!(marks(&back), want);
}

#[test]
fn acrobat_xfdf_reads() {
    let xml = r##"<?xml version="1.0" encoding="UTF-8" ?>
<xfdf xmlns="http://ns.adobe.com/xfdf/" xml:space="preserve"><annots>
<text color="#FFFF00" creationdate="D:20261001120000-07'00'" flags="print,nozoom,norotate" date="D:20261001120500-07'00'" name="a1b2" icon="Comment" page="0" rect="100,600,120,620" subject="Sticky Note" title="Ann"><contents-richtext><body xmlns="http://www.w3.org/1999/xhtml"><p>Tie &amp; foot</p></body></contents-richtext><contents>Tie &amp; foot&#x2014;now</contents><popup flags="print,nozoom,norotate" open="no" page="0" rect="120,500,300,620"/></text>
<text color="#FFFF00" name="r1" inreplyto="a1b2" page="0" rect="100,600,120,620" title="Bo"><contents>Done</contents></text>
<ink color="#FF0000" width="2" page="1" name="i1" rect="0,0,50,50"><inklist><gesture>10,10;20,20;30,15</gesture></inklist></ink>
</annots><f href="workpapers.pdf"/><ids original="ABC" modified="DEF"/></xfdf>"##;
    let c = read(xml.as_bytes(), None).unwrap();
    assert_eq!(c.len(), 3);
    assert_eq!(c.file.as_deref(), Some("workpapers.pdf"));
    let note = crate::import::convert(
        c.src.pdf(),
        c.dict(0).unwrap(),
        Rotation::R0,
        Default::default(),
    )
    .unwrap();
    assert_eq!(note.contents, "Tie & foot—now");
    assert_eq!((note.name.as_str(), note.author.as_str()), ("a1b2", "Ann"));
    assert_eq!(note.color, [255, 255, 0]);
    assert_eq!(note.created, "D:20261001120000-07'00'");
    let reply = crate::import::convert(
        c.src.pdf(),
        c.dict(1).unwrap(),
        Rotation::R0,
        Default::default(),
    )
    .unwrap();
    assert!(matches!(reply.kind, AnnotKind::Reply { ref parent } if parent == "a1b2"));
    let ink = crate::import::convert(
        c.src.pdf(),
        c.dict(2).unwrap(),
        Rotation::R0,
        Default::default(),
    )
    .unwrap();
    assert_eq!(c.annots[2].0, 1);
    assert!(matches!(&ink.kind, AnnotKind::Ink { paths } if paths[0].len() == 3));
    assert_eq!(ink.stroke, 2.0);
}

/// Another PDF's comments come in from the file read as a source: every
/// annotation but links, widgets and popups, by page, sheaf's own as it
/// wrote it; an encrypted file's only with its password.
#[test]
fn another_pdf_s_comments_are_read_from_it() {
    use crate::serialize::fixture::{USER, encrypted, rc4_aes_128_and_aes_256};
    let mut f = crate::forms::fixture::Form::new(2);
    let r = || vec![100.into(), 600.into(), 120.into(), 620.into()];
    let note = f.pdf.add_object(dictionary! {
        "Type" => "Annot", "Subtype" => "Text", "Rect" => r(), "NM" => Object::string_literal("ann-1"),
        "T" => Object::string_literal("Ann"), "Contents" => Object::string_literal("Look"),
    });
    let reply = f.pdf.add_object(dictionary! {
        "Type" => "Annot", "Subtype" => "Text", "Rect" => r(), "NM" => Object::string_literal("bo-1"),
        "IRT" => note, "T" => Object::string_literal("Bo"), "Contents" => Object::string_literal("Done"),
    });
    let others = ["Popup", "Link", "Widget"].map(|subtype| {
        f.pdf
            .add_object(dictionary! { "Type" => "Annot", "Subtype" => subtype, "Rect" => r() })
    });
    let ours = crate::forms::fixture::note();
    let main = crate::serialize::built::to_pdf(&ours, &mut f.pdf).main;
    let ours_id = f.pdf.add_object(Object::Dictionary(main));
    let free = f.pdf.add_object(dictionary! {
        "Type" => "Annot", "Subtype" => "FreeText", "Rect" => r(), "NM" => Object::string_literal("ft-1"),
        "Contents" => Object::string_literal("On two"), "DA" => Object::string_literal("/Helv 10 Tf"),
    });
    f.set_annots(0, &[note, others[0], reply, others[1], others[2], ours_id]);
    f.set_annots(1, &[free]);
    let id = Object::string_literal("0123456789abcdef");
    f.pdf.trailer.set("ID", vec![id.clone(), id]);
    let plain = f.bytes();
    let [_, (_, aes), _] = rc4_aes_128_and_aes_256(&f.pdf);
    let [locked, _] = encrypted(&f.pdf, &aes, |_| {});
    let e = read(&locked, None).err().expect("it needs its password");
    assert!(crate::source::OpenError::is_password(&e), "{e:#}");
    for (bytes, password) in [(&plain, None), (&locked, Some(USER))] {
        let c = read(bytes, password).unwrap();
        let names: Vec<(u32, String)> = (0..c.len())
            .map(|i| (c.annots[i].0, thread_name(c.dict(i).unwrap())))
            .collect();
        let want = [(0, "ann-1"), (0, "bo-1"), (0, &*ours.name()), (1, "ft-1")];
        assert_eq!(names, want.map(|(page, nm)| (page, nm.to_string())));
        assert_eq!(c.parent_of(c.dict(1).unwrap()).as_deref(), Some("ann-1"));
        let entry = Object::Reference(c.annots[2].1);
        let entries = [entry.clone()];
        let annots = crate::file::PageAnnots::new(&entries);
        let back = crate::file::sheaf_mark(c.src.pdf(), &entry, Rotation::R0, &annots).unwrap();
        assert_eq!(
            (back.id, back.kind, back.contents),
            (ours.id, ours.kind.clone(), ours.contents.clone())
        );
    }
}

#[test]
fn not_a_data_file_says_so() {
    assert!(read(b"hello", None).is_err());
    assert!(xfdf::read(b"<html></html>").is_err());
}

/// A two-page Letter file with another tool's square ("sq") on page 1.
fn pdf_with_square(dir: &std::path::Path) -> std::path::PathBuf {
    let mut pdf = Pdf::with_version("1.5");
    let pages_id = pdf.new_object_id();
    let mut kids = Vec::new();
    for i in 0..2 {
        let content = pdf.add_object(Stream::new(dictionary! {}, Vec::new()));
        let mut page = dictionary! {
            "Type" => "Page", "Parent" => pages_id, "Contents" => content,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        };
        if i == 0 {
            let sq = pdf.add_object(dictionary! {
                "Type" => "Annot", "Subtype" => "Square", "NM" => Object::string_literal("sq"),
                "Rect" => vec![100.into(), 100.into(), 200.into(), 150.into()],
                "T" => Object::string_literal("Pat"), "Contents" => Object::string_literal("Box"),
            });
            page.set("Annots", vec![sq.into()]);
        }
        kids.push(Object::Reference(pdf.add_object(page)));
    }
    pdf.objects.insert(
        pages_id,
        dictionary! { "Type" => "Pages", "Kids" => kids, "Count" => 2 }.into(),
    );
    let catalog = pdf.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    pdf.trailer.set("Root", catalog);
    let path = dir.join("two.pdf");
    pdf.save(&path).unwrap();
    path
}

/// What `got` brings, applied to `doc` as the import job applies it.
fn applied(doc: &mut Document, got: Imported) {
    let mut tx = doc.tx(Actor::Job(JobKind::Import));
    let ops = got.ops_in(&mut tx);
    tx.apply_batch(ops, &NoProgress).unwrap();
    doc.commit(tx, CommitOpts::default()).unwrap();
}

/// `path` opened, with a note of ours on its first page, and the other
/// tools' comments in it.
fn opened(path: &std::path::Path) -> (Document, Vec<AnnotId>) {
    let mut doc = crate::open::file(path, None).unwrap().doc;
    let note = Annot {
        id: AnnotId(0),
        kind: AnnotKind::Note {
            pos: Pt { x: 50.0, y: 50.0 },
        },
        color: [255, 230, 0],
        opacity: 1.0,
        stroke: 1.0,
        author: "Greg".into(),
        contents: "ours".into(),
        modified: String::new(),
        created: String::new(),
        fill: None,
        dashed: false,
        cloudy: false,
        turn: Rotation::R0,
        name: String::new(),
    };
    let p0 = doc.page(0).unwrap();
    doc.commit_ops(vec![add(p0, &note)], "note").unwrap();
    let st = doc.state();
    let theirs = (st.order().iter())
        .flat_map(|&page| st.foreign_on(page).map(|(id, _)| id))
        .collect();
    (doc, theirs)
}

/// Two imports of the same comment planned on one version, as two
/// started together are, with a note added to another page meanwhile: the
/// first lands beside the note; the second, which found no comment by that
/// name there, is a Conflict over the page the first added it to, and
/// changes nothing, rather than adding it twice.
#[test]
fn imports_planned_together_conflict_over_what_they_matched_by() {
    let dir = std::env::temp_dir().join(format!("sheaf-exchange-twice-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = pdf_with_square(&dir);
    let (mut doc, _) = opened(&path);
    let (p0, p1) = (doc.page(0).unwrap(), doc.page(1).unwrap());
    let mut c = Making::default();
    c.add(
        1,
        dictionary! {
            "Subtype" => "Text", "Rect" => vec![100.into(), 100.into(), 120.into(), 120.into()],
            "NM" => Object::string_literal("new"), "Contents" => Object::string_literal("Hello"),
        },
    );
    let c = c.comments(None);
    let snap = doc.snapshot();
    let planned = || {
        let mut tx = Document::tx_at(&snap, doc.lower_cx(Actor::Job(JobKind::Import)));
        let ops = import(&snap, doc.store(), &c).ops_in(&mut tx);
        tx.apply_batch(ops, &NoProgress).unwrap();
        tx
    };
    let (first, second) = (planned(), planned());
    let ours = doc.state().marks_on(p0).next().unwrap().clone();
    let note = Annot {
        name: String::new(),
        ..ours
    };
    doc.commit_ops(vec![add(p0, &note)], "note").unwrap();
    let on = || CommitOpts::based(snap.version, Stale::Relower, "Import");
    doc.commit(first, on()).unwrap();
    assert_eq!(doc.state().annots_named(p1, "new").count(), 1);
    assert_eq!(doc.state().marks_on(p0).count(), 2);
    let at = doc.current_version();
    let e = doc.commit(second, on()).unwrap_err();
    assert!(
        matches!(e, EditError::Conflict { key: Key::PageAnnots(p), .. } if p == p1),
        "{e}"
    );
    assert_eq!(doc.current_version(), at);
    assert_eq!(doc.state().annots_named(p1, "new").count(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn imports_match_by_name_and_report_the_rest() {
    let dir = std::env::temp_dir().join(format!("sheaf-exchange-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = pdf_with_square(&dir);
    let (mut doc, foreign) = opened(&path);
    let snap = doc.snapshot();
    let p0 = doc.page(0).unwrap();
    let ours = doc.state().marks_on(p0).next().unwrap().clone();

    let mut c = Making::default();
    let r = || vec![100.into(), 100.into(), 120.into(), 120.into()];
    // Ours, changed elsewhere; theirs, changed elsewhere; a new one; one
    // sheaf can't hold; one past the last page.
    c.add(
        0,
        dictionary! { "Subtype" => "Text", "Rect" => r(), "NM" => crate::save::text_string(&ours.name()), "Contents" => Object::string_literal("ours, edited") },
    );
    c.add(
        0,
        dictionary! { "Subtype" => "Square", "Rect" => r(), "NM" => Object::string_literal("sq"), "Contents" => Object::string_literal("Box, edited") },
    );
    c.add(
        1,
        dictionary! { "Subtype" => "Text", "Rect" => r(), "NM" => Object::string_literal("new"), "Contents" => Object::string_literal("Hello") },
    );
    c.add(1, dictionary! { "Subtype" => "Sound", "Rect" => r() });
    c.add(1, dictionary! { "Subtype" => "Popup", "Rect" => r() });
    c.add(5, dictionary! { "Subtype" => "Text", "Rect" => r() });
    let got = import(&snap, doc.store(), &c.comments(None));
    assert_eq!((got.added, got.updated, got.off_the_end), (1, 2, 1));
    assert_eq!(got.skipped, BTreeMap::from([("Sound".to_string(), 1)]));
    assert_eq!(
        got.summary(),
        "1 added, 2 updated; skipped 1 Sound (sheaf can't hold them); 1 on pages this document doesn't have"
    );
    // Ours, by name; theirs made ours, by name, then updated.
    let named = |nm: &str| AnnotRef::Nm {
        nm: nm.into(),
        page: Some(PageRef::Id(p0)),
    };
    match &got.ops[..] {
        [
            Op::PatchMarks {
                marks,
                patch,
                imported: Some(mine),
            },
            Op::AdoptAnnotations { annots },
            Op::PatchMarks { marks: square, .. },
            Op::AddMark {
                imported: Some(new),
                ..
            },
        ] => {
            assert_eq!(*marks, [named(&ours.name())]);
            assert_eq!(patch.contents.as_deref(), Some("ours, edited"));
            assert_eq!(mine.nm, ours.name());
            assert_eq!((annots, square), (&vec![named("sq")], &vec![named("sq")]));
            assert_eq!(new.nm, "new");
        }
        other => panic!("{other:?}"),
    }

    // All of it, ours and theirs, out and back.
    let out = gather(&snap, &[ours.id], &foreign).unwrap();
    assert_eq!(out.file.as_deref(), Some("two.pdf"));
    let back = read(&fdf::write(&out).unwrap(), None).unwrap();
    let names: Vec<(u32, String)> = (0..back.len())
        .map(|i| (back.annots[i].0, thread_name(back.dict(i).unwrap())))
        .collect();
    assert_eq!(names, [(0, ours.name()), (0, "sq".to_string())]);
    // Ours come back exactly, from /SheafAnnot.
    let again = import(&snap, doc.store(), &back);
    assert_eq!((again.added, again.updated), (0, 2));

    // Applied: ours and theirs changed in place, under their names, and
    // the new one added on the second page.
    applied(&mut doc, got);
    let st = doc.state();
    let mark = |page: PageId, nm: &str| {
        let id = st.annots_named(page, nm).next().unwrap();
        st.annot(id).unwrap().mark().unwrap().clone()
    };
    assert_eq!(mark(p0, &ours.name()).contents, "ours, edited");
    assert_eq!(st.annots_named(p0, "sq").count(), 1);
    assert_eq!(mark(p0, "sq").contents, "Box, edited");
    assert_eq!(mark(doc.page(1).unwrap(), "new").contents, "Hello");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A page stamp's page and an attachment's file go out with our marks in
/// an FDF and come back with them into a document that has neither: the
/// file's bytes as they were, the stamp's page made a one-page PDF of the
/// form the FDF holds, and named by it.
#[test]
fn our_marks_bring_their_stamps_and_files() {
    use crate::forms::fixture::{MARK_FILE, MARK_FILE_BYTES, MARK_STAMP, every_mark_kind};
    let dir = std::env::temp_dir().join(format!(
        "sheaf-exchange-held-{}-{:x}",
        std::process::id(),
        fastrand::u64(..)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = pdf_with_square(&dir);
    let (mut doc, _) = opened(&path);
    let held = crate::forms::fixture::mark_blobs(doc.store());
    let p0 = doc.page(0).unwrap();
    let every = every_mark_kind();
    for annot in [&every[17], &every[23]] {
        doc.commit_ops(vec![add(p0, annot)], "mark").unwrap();
    }
    drop(held);
    // After the note.
    let ours: Vec<AnnotId> = doc.state().marks_on(p0).skip(1).map(|a| a.id).collect();
    let out = gather(&doc.snapshot(), &ours, &[]).unwrap();
    let back = read(&fdf::write(&out).unwrap(), None).unwrap();
    let (mut other, _) = opened(&path);
    assert!(other.find_stamp(&MARK_STAMP).is_none());
    assert!(
        other
            .find_file(&MARK_FILE, MARK_FILE_BYTES.len() as u64)
            .is_none()
    );
    let got = import(&other.snapshot(), other.store(), &back);
    assert_eq!(got.added, 2);
    applied(&mut other, got);
    let marks: Vec<&crate::model::Mark> = other.state().mark_records_on(p0).skip(1).collect();
    let AnnotKind::PageStamp { stamp, .. } = &marks[0].annot.kind else {
        panic!("a page stamp")
    };
    let page = marks[0].stamp.as_ref().unwrap().to_pdf().unwrap();
    assert_eq!(*stamp, crate::store::blob_id(&page).stem());
    assert_eq!(
        lopdf::Document::load_mem(&page).unwrap().get_pages().len(),
        1
    );
    assert!(
        matches!(&marks[1].annot.kind, AnnotKind::FileAttachment { file, .. } if *file == *MARK_FILE)
    );
    assert_eq!(&**marks[1].file.as_ref().unwrap().bytes(), MARK_FILE_BYTES);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A reply comes in after the comment it replies to, wherever the data
/// file lists it; one to a comment that's neither here nor brought in is
/// in no thread, and is left out and said.
#[test]
fn replies_come_in_after_their_comments() {
    let dir = std::env::temp_dir().join(format!(
        "sheaf-exchange-replies-{}-{:x}",
        std::process::id(),
        fastrand::u64(..)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = pdf_with_square(&dir);
    let (mut doc, _) = opened(&path);
    let p0 = doc.page(0).unwrap();
    let mut c = Making::default();
    let r = || vec![100.into(), 100.into(), 120.into(), 120.into()];
    let mut add = |d: Dictionary| {
        c.add(0, d);
    };
    // By name, as XFDF says what a reply is to: the reply first, then its
    // comment; a reply to the square here; one to nothing.
    add(dictionary! {
        "Subtype" => "Text", "Rect" => r(), "NM" => Object::string_literal("re"),
        "IRT" => Object::string_literal("why"), "Contents" => Object::string_literal("Because"),
    });
    add(dictionary! {
        "Subtype" => "Text", "Rect" => r(), "NM" => Object::string_literal("why"),
        "Contents" => Object::string_literal("Why?"),
    });
    add(dictionary! {
        "Subtype" => "Text", "Rect" => r(), "NM" => Object::string_literal("ok"),
        "IRT" => Object::string_literal("sq"), "Contents" => Object::string_literal("OK"),
    });
    add(dictionary! {
        "Subtype" => "Text", "Rect" => r(), "NM" => Object::string_literal("lost"),
        "IRT" => Object::string_literal("gone"), "Contents" => Object::string_literal("?"),
    });
    let got = import(&doc.snapshot(), doc.store(), &c.comments(None));
    assert_eq!((got.added, got.updated, got.orphans), (3, 0, 1));
    assert_eq!(
        got.summary(),
        "3 added, 0 updated; 1 replies left out (what they reply to isn't on their page)"
    );
    let order: Vec<&str> = (got.ops.iter())
        .map(|op| match op {
            Op::AddMark {
                imported: Some(i), ..
            } => i.nm.as_str(),
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(order, ["why", "ok", "re"]);
    applied(&mut doc, got);
    let st = doc.state();
    let parent = |nm: &str| {
        let id = st.annots_named(p0, nm).next().unwrap();
        st.annot(id).unwrap().parent().map(str::to_string)
    };
    assert_eq!(parent("re").as_deref(), Some("why"));
    assert_eq!(parent("ok").as_deref(), Some("sq"));
    assert_eq!(st.annots_named(p0, "lost").count(), 0);
    let _ = std::fs::remove_dir_all(&dir);
}
