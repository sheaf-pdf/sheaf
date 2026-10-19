//! Annotations and forms through the emitter (M10.5 slice 9): marks
//! through their appearances, reopening as they were, companions known by
//! their group alone, replies following marks given fresh ids, pop-ups
//! going with their comments, connectors and tags; merged fields one
//! object a copy, instances apart, the form's owned keys, save policy and
//! the plan's asks; and the post-conditions failing closed.

use std::collections::{BTreeSet, HashSet};
use std::sync::Arc;

use in_progress::NoProgress;
use lopdf::{Dictionary, Document as Pdf, Object, ObjectId, dictionary};
use sheaf_core::Rotation;

use super::graph::Graph;
use super::tests::{
    add, bytes, file, insert, lazy, open, rewrite, save, source, source_of, span, target,
};
use super::*;
use crate::annot::{Annot, AnnotId, AnnotKind, Box2, Pt};
use crate::forms::fixture::{Form, every_mark_kind, row};
use crate::labels::{LabelSpec, Style};
use crate::model::{Document, Edit, PageId};
use crate::prim::{Key, Val};
use crate::serialize::fixture::holds;
use crate::store::Src;

/// A one-page form whose page holds `marks` as a save writes them, each
/// holding its stamp's page or attachment's file.
fn marked(marks: &[Annot]) -> Vec<u8> {
    let mut f = Form::new(1);
    let page = f.pages[0];
    let mut dict = f.dict(page).clone();
    let marks = crate::forms::fixture::holding(marks);
    crate::forms::fixture::write_marks(&mut f.pdf, &mut dict, &marks);
    *f.dict(page) = dict;
    f.bytes()
}

/// Page `index`'s `/Annots` in `src`, each entry's object and dictionary.
fn annots_on(src: &Src, index: u32) -> Vec<(Option<ObjectId>, Dictionary)> {
    let pdf = src.pdf();
    let page = pdf.get_dictionary(src.page(index).unwrap().id).unwrap();
    let Ok(list) = page.get(b"Annots") else {
        return Vec::new();
    };
    let list = pdf.dereference(list).unwrap().1.as_array().unwrap();
    (list.iter())
        .map(|e| {
            let (at, d) = pdf.dereference(e).unwrap();
            (at, d.as_dict().unwrap().clone())
        })
        .collect()
}

/// `d`'s marks on page `index`, by id.
fn marks(d: &Document, index: usize) -> Vec<Annot> {
    let page = d.page(index).unwrap();
    let mut out: Vec<Annot> = d.state().marks_on(page).cloned().collect();
    out.sort_by_key(|a| a.id);
    out
}

/// `key` of `d` as a name, if it is one.
fn name<'a>(d: &'a Dictionary, key: &[u8]) -> Option<&'a [u8]> {
    d.get(key).ok().and_then(|n| n.as_name().ok())
}

fn mark(id: u64, kind: AnnotKind) -> Annot {
    Annot {
        id: AnnotId(id),
        kind,
        color: [200, 30, 30],
        opacity: 1.0,
        stroke: 1.0,
        author: "me".into(),
        contents: "words".into(),
        modified: String::new(),
        created: String::new(),
        fill: None,
        dashed: false,
        cloudy: false,
        turn: Rotation::R0,
        name: String::new(),
    }
}

fn bx(x: f32, y: f32, w: f32, h: f32) -> Box2 {
    Box2 { x, y, w, h }
}

/// Every kind of mark, on a page brought in from another file, is written
/// through its appearance and reopens as the document held it: each a
/// mark, a page stamp holding its page and an attachment its file, the
/// reply and the status in their thread.
#[test]
fn every_kind_of_mark_reopens_as_written() {
    let mut d = open(&bytes(&file("A", 1, false)), None);
    let other = marked(&every_mark_kind());
    let s = add(&mut d, &other, "marks.pdf");
    let carried = d.source(s).unwrap().src.annots().marks();
    let edit = Edit::InsertPages {
        source: s,
        range: 0..1,
        before: None,
        outline: None,
        annots: carried.into_iter().collect(),
    };
    d.apply(edit, "insert").unwrap();
    let want = marks(&d, 1);
    assert_eq!(want.len(), every_mark_kind().len());
    let out = rewrite(&d, Profile::default());
    let back = open(&out.bytes, None);
    assert_eq!(marks(&back, 1), want);
    let page = back.page(1).unwrap();
    assert_eq!(back.state().foreign_on(page).count(), 0);
    // No companion is marked: a replacement's StrikeOut is its Caret's by
    // its group.
    assert!(!holds(&out.bytes, crate::annot::PART.as_bytes()));
}

/// A mark the document holds as it was read is the file's object as it is,
/// its appearance with it; an edited one is written anew at its number.
#[test]
fn a_mark_as_read_is_the_file_s_object_and_an_edited_one_keeps_its_number() {
    let every = every_mark_kind();
    let before = marked(&every);
    let mut d = open(&before, None);
    let was = lazy(&before, None);
    let read: Vec<(ObjectId, Dictionary)> = annots_on(&source(&before, None), 0)
        .into_iter()
        .map(|(at, d)| (at.unwrap(), d))
        .collect();
    let out = rewrite(&d, Profile::default());
    let now = lazy(&out.bytes, None);
    for (at, _) in &read {
        assert_eq!(
            span(&now, &out.bytes, *at),
            span(&was, &before, *at),
            "{at:?}"
        );
    }
    let page = d.page(0).unwrap();
    let mut note = marks(&d, 0).remove(1);
    note.contents = "Edited".into();
    let edited = note.name();
    d.apply(Edit::AnnotUpdate { page, annot: note }, "edit")
        .unwrap();
    let out = rewrite(&d, Profile::default());
    let now = lazy(&out.bytes, None);
    let (at, _) = (read.iter())
        .find(|(_, d)| crate::file::thread_name(d) == edited)
        .unwrap();
    assert_ne!(span(&now, &out.bytes, *at), span(&was, &before, *at));
    assert_eq!(
        marks(&open(&out.bytes, None), 0)[1].contents,
        "Edited",
        "the edit is at its number"
    );
    let untouched = read.iter().filter(|(a, _)| a != at).count();
    let kept = (read.iter())
        .filter(|(a, _)| a != at && span(&now, &out.bytes, *a) == span(&was, &before, *a))
        .count();
    assert_eq!(kept, untouched);
}

/// A replace-text mark saves with its StrikeOut grouped under its Caret,
/// marked no other way, and reopens as one mark; a file sheaf wrote with
/// `/SheafPart` on the StrikeOut opens as one mark too, and saves without
/// it.
#[test]
fn a_replacement_saves_with_no_sheaf_part_and_reopens_as_one_mark() {
    let at = bx(100.0, 600.0, 80.0, 14.0);
    let replace = mark(
        7,
        AnnotKind::Replace {
            rects: vec![at],
            struck: "old words".into(),
            at,
        },
    );
    let mut d = open(&bytes(&file("A", 1, false)), None);
    let page = d.page(0).unwrap();
    d.apply(
        Edit::AnnotAdd {
            page,
            annot: replace.clone(),
        },
        "replace",
    )
    .unwrap();
    let out = rewrite(&d, Profile::default());
    let list = annots_on(&source_of(&out), 0);
    assert_eq!(list.len(), 2);
    let (caret, strike) = (&list[0], &list[1]);
    assert_eq!(name(&caret.1, b"Subtype"), Some(b"Caret".as_slice()));
    assert_eq!(name(&strike.1, b"Subtype"), Some(b"StrikeOut".as_slice()));
    assert_eq!(
        strike.1.get(b"IRT").unwrap(),
        &Object::Reference(caret.0.unwrap())
    );
    assert_eq!(name(&strike.1, b"RT"), Some(b"Group".as_slice()));
    assert!(!strike.1.has(crate::annot::PART.as_bytes()));
    let back = open(&out.bytes, None);
    let page = back.page(0).unwrap();
    assert_eq!(back.state().annots_on(page).count(), 1);
    assert!(matches!(marks(&back, 0)[0].kind, AnnotKind::Replace { .. }));

    // As sheaf wrote it before: the StrikeOut marked as the Caret's part.
    let mut f = Form::new(1);
    let written = crate::annot::to_pdf(&replace, &mut f.pdf);
    let caret = f.pdf.add_object(written.main);
    let mut strike = written.companions.into_iter().next().unwrap();
    strike.set("IRT", caret);
    strike.set("RT", Object::Name(b"Group".to_vec()));
    strike.set(crate::annot::PART, true);
    let strike = f.pdf.add_object(strike);
    f.set_annots(0, &[caret, strike]);
    let old = f.bytes();
    let d = open(&old, None);
    let page = d.page(0).unwrap();
    assert_eq!(d.state().annots_on(page).count(), 1);
    assert_eq!(d.state().marks_on(page).count(), 1);
    // Unedited, it isn't copied as the file has it: it's written anew.
    let out = rewrite(&d, Profile::default());
    assert!(!holds(&out.bytes, crate::annot::PART.as_bytes()));
    let mut d = d;
    let mut edited = marks(&d, 0).remove(0);
    edited.contents = "newer words".into();
    d.apply(
        Edit::AnnotUpdate {
            page,
            annot: edited,
        },
        "edit",
    )
    .unwrap();
    let out = rewrite(&d, Profile::default());
    assert!(!holds(&out.bytes, crate::annot::PART.as_bytes()));
    let back = open(&out.bytes, None);
    assert_eq!(back.state().annots_on(back.page(0).unwrap()).count(), 1);
}

/// A mark as the file has it is copied as it is only while it says what
/// the mark is written with: its `/P` its page's, and no pop-up (sheaf's
/// marks have none). One with a pop-up, or another page's `/P`, is written
/// anew, as the built mark is.
#[test]
fn a_mark_with_a_pop_up_or_another_page_s_p_is_written_anew() {
    // A page holding a note, `key` set to the second page on its object.
    let made = |key: &str| {
        let mut f = Form::new(2);
        let page = f.pages[0];
        let mut dict = f.dict(page).clone();
        let pos = Pt { x: 100.0, y: 700.0 };
        let marks = crate::forms::fixture::holding(&[mark(9, AnnotKind::Note { pos })]);
        crate::forms::fixture::write_marks(&mut f.pdf, &mut dict, &marks);
        *f.dict(page) = dict.clone();
        let list = dict.get(b"Annots").unwrap().as_array().unwrap();
        let at = list[0].as_reference().unwrap();
        let other = f.pages[1];
        f.dict(at).set(key, other);
        (f.bytes(), at)
    };
    for key in ["Popup", "P"] {
        let (before, at) = made(key);
        let d = open(&before, None);
        assert_eq!(marks_on_first(&d), 1, "{key}");
        let out = rewrite(&d, Profile::default());
        let now = lazy(&out.bytes, None);
        let written = now.get_dictionary(at).unwrap();
        assert!(!written.has(b"Popup"), "{key}");
        let p = written.get(b"P").unwrap().as_reference().unwrap();
        assert_eq!(p, source_of(&out).page(0).unwrap().id, "{key}");
    }
}

/// How many of `d`'s marks its first page holds.
fn marks_on_first(d: &Document) -> usize {
    d.state().marks_on(d.page(0).unwrap()).count()
}

/// A file whose page holds a mark, and another tool's reply to it.
fn replied() -> Vec<u8> {
    let note = mark(
        3,
        AnnotKind::Note {
            pos: Pt { x: 100.0, y: 700.0 },
        },
    );
    let mut f = Form::new(1);
    let main = crate::annot::to_pdf(&note, &mut f.pdf).main;
    let main = f.pdf.add_object(main);
    let reply = f.pdf.add_object(dictionary! {
        "Type" => "Annot", "Subtype" => "Text", "IRT" => main, "NM" => Object::string_literal("r1"),
        "Rect" => vec![100.into(), 700.into(), 120.into(), 720.into()],
        "Contents" => Object::string_literal("their reply"),
    });
    f.set_annots(0, &[main, reply]);
    f.bytes()
}

/// Another tool's reply to a mark names the mark's object as written: on
/// a second insert of its page, whose marks are given fresh ids, the
/// reply there names the mark there.
#[test]
fn a_reply_follows_its_mark_given_a_fresh_id() {
    let mut d = open(&bytes(&file("A", 1, false)), None);
    let other = replied();
    let s = add(&mut d, &other, "replied.pdf");
    for _ in 0..2 {
        let carried = d.source(s).unwrap().src.annots().marks();
        let edit = Edit::InsertPages {
            source: s,
            range: 0..1,
            before: None,
            outline: None,
            annots: carried.into_iter().collect(),
        };
        d.apply(edit, "insert").unwrap();
    }
    let (first, second) = (marks(&d, 1), marks(&d, 2));
    assert_ne!(first[0].id, second[0].id, "a fresh id");
    let out = rewrite(&d, Profile::default());
    let now = source_of(&out);
    for index in [1, 2] {
        let list = annots_on(&now, index);
        assert_eq!(list.len(), 2, "page {index}");
        // An insert's marks come after the file's other annotations.
        let (reply, mark) = (&list[0], &list[1]);
        assert!(mark.1.has(b"SheafAnnot"));
        assert_eq!(
            reply.1.get(b"IRT").unwrap(),
            &Object::Reference(mark.0.unwrap()),
            "page {index}"
        );
    }
    let back = open(&out.bytes, None);
    for index in [1, 2] {
        let page = back.page(index).unwrap();
        let (_, reply) = back.state().foreign_on(page).next().unwrap();
        assert_eq!(
            reply.summary.parent.as_deref(),
            Some(marks(&back, index)[0].name().as_str())
        );
    }
}

/// A page with a comment that opens a pop-up, a screen annotation, a
/// widget outside the form and an entry of no subtype.
fn popped() -> Vec<u8> {
    let mut f = Form::new(1);
    let page = f.pages[0];
    let rect = || vec![100.into(), 100.into(), 150.into(), 150.into()];
    let comment = f.pdf.new_object_id();
    let popup = f.pdf.add_object(dictionary! {
        "Type" => "Annot", "Subtype" => "Popup", "Parent" => comment, "P" => page, "Rect" => rect(),
    });
    f.pdf.objects.insert(
        comment,
        dictionary! {
            "Type" => "Annot", "Subtype" => "Square", "P" => page, "Rect" => rect(),
            "NM" => Object::string_literal("c1"), "Contents" => Object::string_literal("THEIRNOTE"),
            "Popup" => popup,
        }
        .into(),
    );
    let screen = f.pdf.add_object(dictionary! {
        "Type" => "Annot", "Subtype" => "Screen", "P" => page, "Rect" => rect(),
    });
    let stray = f.pdf.add_object(dictionary! {
        "Type" => "Annot", "Subtype" => "Widget", "P" => page, "Rect" => rect(),
    });
    let bare = f.pdf.add_object(dictionary! { "Rect" => rect() });
    f.set_annots(0, &[comment, popup, screen, stray, bare]);
    f.bytes()
}

/// A pop-up goes with the comment that opens it, a duplicate's with the
/// duplicate's comment; what the records don't hold stays with its page,
/// a copy's its own; deleting the comment takes its pop-up out.
#[test]
fn pop_ups_go_with_their_comments_and_the_rest_stays() {
    let before = popped();
    let mut d = open(&before, None);
    let first = d.page(0).unwrap();
    // As read, every entry is the file's object as it is.
    let out = rewrite(&d, Profile::default());
    let was = annots_on(&source(&before, None), 0);
    let now = annots_on(&source_of(&out), 0);
    let objects = |l: &[(Option<ObjectId>, Dictionary)]| -> Vec<ObjectId> {
        l.iter().map(|(at, _)| at.unwrap()).collect()
    };
    assert_eq!(objects(&now), objects(&was));
    let (lazy_was, lazy_now) = (lazy(&before, None), lazy(&out.bytes, None));
    for at in objects(&was) {
        assert_eq!(
            span(&lazy_now, &out.bytes, at),
            span(&lazy_was, &before, at),
            "{at:?}"
        );
    }

    d.apply(Edit::DuplicatePages { ids: vec![first] }, "dup")
        .unwrap();
    let out = rewrite(&d, Profile::default());
    let now = source_of(&out);
    let copy = annots_on(&now, 1);
    let subtypes: Vec<Option<&[u8]>> = copy.iter().map(|(_, d)| name(d, b"Subtype")).collect();
    assert_eq!(
        subtypes,
        [
            Some(b"Square".as_slice()),
            Some(b"Popup"),
            Some(b"Screen"),
            Some(b"Widget"),
            None
        ]
    );
    let original = annots_on(&now, 0);
    let mine: HashSet<ObjectId> = objects(&original).into_iter().collect();
    assert!(objects(&copy).iter().all(|at| !mine.contains(at)));
    let (comment, popup) = (&copy[0], &copy[1]);
    assert_eq!(
        popup.1.get(b"Parent").unwrap(),
        &Object::Reference(comment.0.unwrap())
    );
    assert_eq!(
        comment.1.get(b"Popup").unwrap(),
        &Object::Reference(popup.0.unwrap())
    );
    let copy_page = now.page(1).unwrap().id;
    for (_, d) in &copy[..4] {
        assert_eq!(d.get(b"P").unwrap(), &Object::Reference(copy_page));
    }

    d.apply(
        Edit::CommentsDelete {
            page: first,
            names: vec!["c1".into()],
        },
        "delete",
    )
    .unwrap();
    let out = rewrite(&d, Profile::default());
    let left = annots_on(&source_of(&out), 0);
    let subtypes: Vec<Option<&[u8]>> = left.iter().map(|(_, d)| name(d, b"Subtype")).collect();
    assert_eq!(
        subtypes,
        [Some(b"Screen".as_slice()), Some(b"Widget"), None]
    );
    assert_eq!(annots_on(&source_of(&out), 1).len(), 5);
}

/// Each connector of a pair leads to the other's page, at its place.
#[test]
fn a_connector_leads_to_the_next_of_its_group() {
    let mut d = open(&bytes(&file("A", 2, false)), None);
    let (p0, p1) = (d.page(0).unwrap(), d.page(1).unwrap());
    let end = |id, y| {
        mark(
            id,
            AnnotKind::Connector {
                rect: bx(100.0, y, 20.0, 20.0),
                label: "1".into(),
                partner: None,
            },
        )
    };
    d.apply(
        Edit::ConnectorAdd {
            a: (p0, end(1, 500.0)),
            b: (p1, end(2, 300.0)),
        },
        "connect",
    )
    .unwrap();
    let out = rewrite(&d, Profile::default());
    let now = source_of(&out);
    let pages: Vec<ObjectId> = now.page_ids().collect();
    for (index, to) in [(0, pages[1]), (1, pages[0])] {
        let list = annots_on(&now, index);
        let dest = list[0].1.get(b"Dest").unwrap().as_array().unwrap();
        assert_eq!(dest[0], Object::Reference(to), "page {index}");
        assert_eq!(dest[1], Object::Name(b"XYZ".to_vec()));
    }
}

/// A tagged form whose page has another tool's comment with its
/// `/StructParent`, beside the form's tagged widget.
fn tagged_form() -> Vec<u8> {
    let mut f = Form::new(1);
    f.text("name", 0, row(0));
    let page = f.pages[0];
    let comment = f.pdf.add_object(dictionary! {
        "Type" => "Annot", "Subtype" => "Square", "P" => page,
        "Rect" => vec![300.into(), 300.into(), 350.into(), 350.into()],
        "Contents" => Object::string_literal("tagged comment"),
    });
    let mut list = f.annots(0);
    list.push(comment);
    f.set_annots(0, &list);
    f.tag();
    f.bytes()
}

/// A page of a tagged file inserted into a tagged file loses its
/// annotations' `/StructParent`, its widget's too, since its structure
/// tree isn't written; the file's own page keeps theirs.
#[test]
fn a_tagged_page_inserted_into_a_tagged_file_has_no_struct_parent_on_its_annotations() {
    let tagged = tagged_form();
    let mut d = open(&tagged, None);
    let s = add(&mut d, &tagged, "other.pdf");
    insert(&mut d, s, 0);
    let out = rewrite(&d, Profile::default());
    let now = source_of(&out);
    let own = annots_on(&now, 0);
    assert_eq!(own.len(), 2);
    assert!(own.iter().all(|(_, d)| d.has(b"StructParent")));
    let theirs = annots_on(&now, 1);
    assert_eq!(theirs.len(), 2);
    assert!(theirs.iter().all(|(_, d)| !d.has(b"StructParent")));
    assert!(
        !now.pdf()
            .get_dictionary(now.page(1).unwrap().id)
            .unwrap()
            .has(b"StructParents")
    );
}

/// The objects `from` reaches in `pdf`, but `skip`.
fn reached(pdf: &Pdf, from: ObjectId, skip: &HashSet<ObjectId>) -> BTreeSet<ObjectId> {
    let mut seen = BTreeSet::new();
    let mut stack = vec![from];
    while let Some(id) = stack.pop() {
        if skip.contains(&id) || !seen.insert(id) {
            continue;
        }
        let mut refs = Vec::new();
        fn walk(o: &Object, out: &mut Vec<ObjectId>) {
            match o {
                Object::Reference(r) => out.push(*r),
                Object::Array(a) => a.iter().for_each(|o| walk(o, out)),
                Object::Dictionary(d) => d.iter().for_each(|(_, o)| walk(o, out)),
                Object::Stream(s) => s.dict.iter().for_each(|(_, o)| walk(o, out)),
                _ => {}
            }
        }
        if let Ok(o) = pdf.get_object(id) {
            walk(o, &mut refs);
        }
        stack.extend(refs);
    }
    seen
}

/// Duplicating a page with a field merged with its one widget gives one
/// object a copy, field and widget alike, its page's; each widget key is
/// its field's number in the record; and the form checks clean.
#[test]
fn a_duplicated_merged_field_is_one_object_a_copy() {
    let mut f = Form::new(1);
    let field = f.text("name", 0, row(0));
    f.fill(field, "Ada");
    let mut d = open(&f.bytes(), None);
    let first = d.page(0).unwrap();
    d.apply(Edit::DuplicatePages { ids: vec![first] }, "dup")
        .unwrap();
    let out = rewrite(&d, Profile::default());
    let now = source_of(&out);
    let pages: Vec<ObjectId> = now.page_ids().collect();
    let mut objects = Vec::new();
    for index in 0..2 {
        let list = annots_on(&now, index);
        assert_eq!(list.len(), 1, "page {index}");
        let (at, dict) = &list[0];
        assert_eq!(name(dict, b"FT"), Some(b"Tx".as_slice()), "a field");
        assert_eq!(
            name(dict, b"Subtype"),
            Some(b"Widget".as_slice()),
            "a widget"
        );
        assert!(!dict.has(b"Kids"));
        assert_eq!(
            dict.get(b"P").unwrap(),
            &Object::Reference(pages[index as usize])
        );
        objects.push(at.unwrap());
    }
    assert_ne!(objects[0], objects[1]);
    assert_eq!(objects[0], field, "copy 0 keeps its number");
    let st = d.state();
    for (_, rec) in st.annots() {
        let w = rec.widget().unwrap();
        let map = &out.record.map;
        let id = st.annot_ids_on(rec.page).unwrap()[0];
        assert_eq!(
            map.get(&ObjKey::Annot(id)),
            map.get(&ObjKey::Field(w.field)).or(Some(field)),
        );
    }
    let report = crate::forms::check(&out.bytes, None).unwrap();
    assert!(report.is_clean(), "{:?}", report.findings);
    assert_eq!(report.fields, 2);
}

/// One form file inserted twice is two instances: each its own fields,
/// widgets and appearances, sharing only the form's resources; and the
/// form checks clean.
#[test]
fn one_form_inserted_twice_is_two_instances_sharing_nothing_of_their_own() {
    let mut f = Form::new(1);
    let text = f.text("name", 0, row(0));
    f.fill(text, "Ada");
    f.radio("pick", 0, &[row(1), row(2)], Some(1));
    let form = f.bytes();
    let mut d = open(&bytes(&file("A", 1, false)), None);
    for name in ["f1.pdf", "f2.pdf"] {
        let s = add(&mut d, &form, name);
        insert(&mut d, s, 0);
    }
    let out = rewrite(&d, Profile::default());
    let report = crate::forms::check(&out.bytes, None).unwrap();
    assert!(report.is_clean(), "{:?}", report.findings);
    assert_eq!(report.fields, 4);
    let pdf = Pdf::load_mem(&out.bytes).unwrap();
    let pages: Vec<ObjectId> = pdf.get_pages().into_values().collect();
    // What the form's resources and the pages hold is shared; past those,
    // each page's annotations reach objects of their own.
    let acroform = pdf
        .catalog()
        .unwrap()
        .get(b"AcroForm")
        .unwrap()
        .as_reference()
        .unwrap();
    let dr = pdf
        .get_dictionary(acroform)
        .unwrap()
        .get(b"DR")
        .unwrap()
        .clone();
    let mut skip: HashSet<ObjectId> = reached(&pdf, acroform, &HashSet::new())
        .into_iter()
        .filter(|&id| {
            let shared = Object::Dictionary(dictionary! { "DR" => dr.clone() });
            reached_in(&pdf, &shared).contains(&id)
        })
        .collect();
    skip.extend(pages.iter().copied());
    let reach = |page: ObjectId| -> BTreeSet<ObjectId> {
        let list = pdf
            .get_dictionary(page)
            .unwrap()
            .get(b"Annots")
            .unwrap()
            .as_array()
            .unwrap();
        (list.iter())
            .flat_map(|a| reached(&pdf, a.as_reference().unwrap(), &skip))
            .collect()
    };
    let (a, b) = (reach(pages[1]), reach(pages[2]));
    assert!(!a.is_empty() && !b.is_empty());
    assert!(
        a.is_disjoint(&b),
        "shared: {:?}",
        a.intersection(&b).collect::<Vec<_>>()
    );
}

/// The objects `obj` reaches in `pdf`.
fn reached_in(pdf: &Pdf, obj: &Object) -> BTreeSet<ObjectId> {
    let mut out = BTreeSet::new();
    fn walk(pdf: &Pdf, o: &Object, out: &mut BTreeSet<ObjectId>) {
        match o {
            Object::Reference(r) => {
                if out.insert(*r)
                    && let Ok(o) = pdf.get_object(*r)
                {
                    walk(pdf, o, out);
                }
            }
            Object::Array(a) => a.iter().for_each(|o| walk(pdf, o, out)),
            Object::Dictionary(d) => d.iter().for_each(|(_, o)| walk(pdf, o, out)),
            Object::Stream(s) => s.dict.iter().for_each(|(_, o)| walk(pdf, o, out)),
            _ => {}
        }
    }
    walk(pdf, obj, &mut out);
    out
}

/// A field whose widgets, its `/Kids`, were all on a deleted page goes,
/// its tooltip with it, and so does the parent it leaves with no fields,
/// as `keep_only` takes such fields out of a file (bug 14), though the
/// state keeps the nodes until slice 11's cascade. A field no page ever
/// showed stays, and so does the form; extracting the other page leaves
/// the same.
#[test]
fn a_field_left_with_no_widgets_goes() {
    let mut f = Form::new(2);
    f.text("kept", 0, row(0));
    let radio = f.radio("gone", 1, &[row(0), row(1)], Some(1));
    f.dict(radio).set("TU", Object::string_literal("SHEAFGONE"));
    f.group("parent", &[radio]);
    let hidden = f.pdf.add_object(dictionary! {
        "FT" => "Tx", "T" => Object::string_literal("hidden"),
        "V" => Object::string_literal("SHEAFHIDDEN"),
    });
    let fields: Vec<ObjectId> = f.fields().into_iter().chain([hidden]).collect();
    f.set_fields(&fields);
    let mut d = open(&f.bytes(), None);
    let names = |bytes: &[u8]| -> BTreeSet<String> {
        let d = open(bytes, None);
        let st = d.state();
        (st.fields())
            .filter_map(|(id, _)| st.full_name(id))
            .collect()
    };
    let all = ["hidden", "kept", "parent", "parent.gone"].map(String::from);
    assert_eq!(names(&f.bytes()), BTreeSet::from(all));
    let first = d.page(0).unwrap();
    let extracted = crate::save::extract_bytes(&d.snapshot(), &[first]).unwrap();
    let second = d.page(1).unwrap();
    d.apply(Edit::DeletePages { ids: vec![second] }, "delete")
        .unwrap();
    let out = rewrite(&d, Profile::default());
    for (bytes, what) in [(&out.bytes, "deleted"), (&extracted, "extracted")] {
        assert!(!holds(bytes, b"SHEAFGONE"), "{what}");
        assert!(holds(bytes, b"SHEAFHIDDEN"), "{what}");
        let left = ["hidden", "kept"].map(String::from);
        assert_eq!(names(bytes), BTreeSet::from(left), "{what}");
        let report = crate::forms::check(bytes, None).unwrap();
        assert_eq!(report.findings, [], "{what}");
    }
}

/// A node whose `/Kids` another node holds (their `/Parent` names that
/// one, and its read took them), as some writers leave a form, never
/// showed anything as sheaf read it: a save keeps it, as the file has it,
/// with or without a page deleted.
#[test]
fn a_node_whose_kids_another_holds_stays() {
    let mut f = Form::new(2);
    let kept = f.text("kept", 0, row(0));
    f.group("real", &[kept]);
    f.text("other", 1, row(0));
    let stray = f.pdf.add_object(dictionary! {
        "T" => Object::string_literal("stray"), "Kids" => vec![kept.into()],
    });
    let fields: Vec<ObjectId> = f.fields().into_iter().chain([stray]).collect();
    f.set_fields(&fields);
    let names = |bytes: &[u8]| -> BTreeSet<String> {
        let d = open(bytes, None);
        let st = d.state();
        (st.fields())
            .filter_map(|(id, _)| st.full_name(id))
            .collect()
    };
    let all = ["other", "real", "real.kept", "stray"].map(String::from);
    assert_eq!(names(&f.bytes()), BTreeSet::from(all.clone()));
    let mut d = open(&f.bytes(), None);
    let same = rewrite(&d, Profile::default());
    assert_eq!(names(&same.bytes), BTreeSet::from(all));
    let second = d.page(1).unwrap();
    d.apply(Edit::DeletePages { ids: vec![second] }, "delete")
        .unwrap();
    let out = rewrite(&d, Profile::default());
    let left = ["real", "real.kept", "stray"].map(String::from);
    assert_eq!(names(&out.bytes), BTreeSet::from(left));
}

/// A three-page form with labels (i, ii, iii), an outline of two
/// bookmarks, and two fields that calculate, in order.
fn owned_keys_form() -> Vec<u8> {
    let mut f = Form::new(3);
    let a = f.text("a", 0, row(0));
    let b = f.text("b", 1, row(0));
    let acroform = f.acroform;
    f.dict(acroform).set("CO", vec![a.into(), b.into()]);
    let catalog = f.catalog;
    f.dict(catalog).set(
        "PageLabels",
        dictionary! { "Nums" => vec![0.into(), dictionary! { "S" => "r" }.into()] },
    );
    let pages = f.pages.clone();
    let item = |title: &str, page: ObjectId| {
        dictionary! {
            "Title" => Object::string_literal(title),
            "Dest" => vec![page.into(), "Fit".into()],
        }
    };
    crate::forms::fixture::outline_tree(
        &mut f,
        vec![(0, item("One", pages[0])), (0, item("Two", pages[1]))],
    );
    f.bytes()
}

/// What a file says of the keys the state owns: its labels, its
/// bookmarks' titles, and its fields that calculate, by name.
fn owned_keys(bytes: &[u8]) -> (Vec<String>, Vec<String>, Vec<String>) {
    let d = open(bytes, None);
    let st = d.state();
    let labels = st.page_labels().unwrap_or_default();
    let titles = (st.bookmarks_preorder().into_iter())
        .map(|(_, _, b)| b.title.to_string())
        .collect();
    let order = (st.form().calc_order.iter())
        .filter_map(|&f| st.full_name(f))
        .collect();
    (labels, titles, order)
}

/// Each key the state owns, set, saved, undone and saved again, reopens
/// as the file had it: the labels, the outline root's (its bookmarks) and
/// the form's `/CO`.
#[test]
fn owned_keys_survive_set_save_undo_save_reopen() {
    let original = owned_keys_form();
    let want = owned_keys(&original);
    assert_eq!(want.0, ["i", "ii", "iii"]);
    assert_eq!(want.1, ["One", "Two"]);
    assert_eq!(want.2, ["a", "b"]);
    type Set = Box<dyn Fn(&mut Document)>;
    let sets: Vec<(&str, Set)> = vec![
        (
            "labels",
            Box::new(|d: &mut Document| {
                let first = d.page(0).unwrap();
                let ranges = vec![LabelSpec {
                    start: first,
                    style: Style::Decimal,
                    prefix: "P-".into(),
                    first: 1,
                }];
                d.apply(Edit::SetPageLabels { ranges }, "labels").unwrap();
            }),
        ),
        (
            "outline root",
            Box::new(|d: &mut Document| {
                let page = d.page(2).unwrap();
                d.apply(
                    Edit::OutlineInsert {
                        parent: None,
                        after: None,
                        title: "Zero".into(),
                        target: Some(page),
                        view: None,
                    },
                    "bookmark",
                )
                .unwrap();
            }),
        ),
        (
            "/CO",
            Box::new(|d: &mut Document| {
                let mut form = crate::model::FormDoc::clone(d.state().form());
                let order: Vec<_> = form.calc_order.iter().rev().copied().collect();
                form.calc_order = order.into();
                let edit = Edit::SetPageLabels { ranges: Vec::new() };
                d.put_for_test(edit, Key::Form, Some(Val::Form(Arc::new(form))), "order");
            }),
        ),
    ];
    for (key, set) in sets {
        let mut d = open(&original, None);
        set(&mut d);
        let first = save(&mut d);
        assert_ne!(owned_keys(&first.bytes), want, "{key}: set");
        d.undo().unwrap();
        let second = save(&mut d);
        assert_eq!(owned_keys(&second.bytes), want, "{key}");
    }
}

/// A hybrid form with Reader's usage rights and `/SigFlags`.
fn hybrid() -> Form {
    let mut f = Form::new(2);
    f.text("a", 0, row(0));
    f.text("b", 1, row(0));
    f.xfa();
    f.usage_rights();
    let acroform = f.acroform;
    f.dict(acroform).set("SigFlags", 1);
    f
}

/// Save policy: a Rewrite of a hybrid form with usage rights drops the
/// rights, and `/SigFlags` with them when nothing signed needs it; the XFA
/// copy stays while the fields are as read, and goes once a page with a
/// field does, its tooltips onto the fields. The plan says so.
#[test]
fn save_policy_drops_the_rights_and_a_changed_form_s_xfa() {
    let before = hybrid().bytes();
    let mut d = open(&before, None);
    let tgt = target(Profile::default());
    let p = plan(&SaveInput::detached(&d.snapshot()), &tgt);
    assert_eq!(
        p.transforms,
        [SaveTransform::DropUr3, SaveTransform::FixSigFlags]
    );
    assert_eq!(p.notes, [Note(crate::forms::rules::RIGHTS_REMOVED.into())]);
    let out = materialize(
        &SaveInput::detached(&d.snapshot()),
        &p,
        tgt.clone(),
        &NoProgress,
    )
    .unwrap();
    let now = source_of(&out);
    let catalog = now.pdf().catalog().unwrap();
    assert!(!catalog.has(b"Perms"));
    let form = now.form();
    assert!(form.facts.xfa, "the XFA copy stays");
    assert_eq!(form.facts.sig_flags, 0);
    assert_eq!(&*out.record.transforms, p.transforms.as_slice());

    let second = d.page(1).unwrap();
    d.apply(Edit::DeletePages { ids: vec![second] }, "delete")
        .unwrap();
    let input = SaveInput::detached(&d.snapshot());
    let p = plan(&input, &tgt);
    assert_eq!(
        p.transforms,
        [
            SaveTransform::DropXfa {
                tooltips_to_tu: true
            },
            SaveTransform::DropUr3,
            SaveTransform::FixSigFlags
        ]
    );
    let out = materialize(&input, &p, tgt, &NoProgress).unwrap();
    let now = source_of(&out);
    assert!(!now.form().facts.xfa);
    let report = crate::forms::check(&out.bytes, None).unwrap();
    assert!(report.is_clean(), "{:?}", report.findings);
}

/// The plan asks before it breaks or flags signatures that hold: an
/// approval signature allows a comment after it, and flags a turn, which
/// an Append anyway may still write; a Rewrite breaks it. It asks when the
/// saved file changed on disk, which is a Rewrite's reason, and when the
/// XFA it drops holds values. Its reasons are the target's, the record's
/// and the header's.
#[test]
fn the_plan_asks_before_it_rewrites() {
    let dir = std::env::temp_dir().join(format!("sheaf-plan-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("doc.pdf");
    let mut f = Form::new(1);
    let sig = f.signature("sig", 0, row(0));
    f.sign(sig);
    std::fs::write(&path, f.bytes()).unwrap();
    let store = crate::store::Store::new();
    let (info, stamp) = crate::model::SourceInfo::read(&store, &path, None).unwrap();
    let mut d = Document::new(
        store,
        info,
        Some(Came::Read(stamp)),
        &[],
        Box::new(crate::history::FixedCount(10)),
    );
    let page = d.page(0).unwrap();
    d.apply(
        Edit::AnnotAdd {
            page,
            annot: mark(
                1,
                AnnotKind::Note {
                    pos: Pt { x: 9.0, y: 9.0 },
                },
            ),
        },
        "note",
    )
    .unwrap();
    let v = d.current_version();
    let input = d.save_input(v).unwrap();
    let same = d.save_target(&path);
    assert!(same.same_file && same.stamp.is_some());
    let p = plan(&input, &same);
    assert_eq!(p.strategy, Strategy::Append { onto: Onto::Saved });
    assert_eq!(p.asks, []);
    d.abandon_save(v);
    let turn = Edit::RotatePages {
        ids: vec![page],
        by: Rotation::R90,
    };
    d.apply(turn, "turn").unwrap();
    let v = d.current_version();
    let input = d.save_input(v).unwrap();
    let p = plan(&input, &same);
    let impact = |verdict| {
        vec![crate::forms::sigs::SigImpact {
            signature: "sig".into(),
            verdict,
        }]
    };
    assert_eq!(p.strategy, Strategy::Append { onto: Onto::Saved });
    let anyway = AppendAnyway {
        stays_readable: Vec::new(),
    };
    assert_eq!(
        p.asks,
        [Ask::Signatures {
            impact: impact(crate::forms::sigs::Verdict::Flagged),
            append_anyway: Some(anyway),
            reason: None
        }]
    );
    std::fs::write(&path, b"another program's").unwrap();
    let p = plan(&input, &same);
    assert_eq!(
        p.strategy,
        Strategy::Rewrite {
            why: vec![Why::FileChanged]
        }
    );
    assert_eq!(
        p.asks,
        [
            Ask::FileChanged {
                path: same.path.clone()
            },
            Ask::Signatures {
                impact: impact(crate::forms::sigs::Verdict::Invalid),
                append_anyway: None,
                reason: Some(Why::FileChanged)
            }
        ]
    );
    d.abandon_save(v);
    let other = SaveTarget {
        same_file: false,
        ..same
    };
    let p = plan(&SaveInput::detached(&d.snapshot()), &other);
    assert_eq!(
        p.strategy,
        Strategy::Rewrite {
            why: vec![Why::OtherTarget, Why::NoSaveRecord]
        }
    );
    let _ = std::fs::remove_dir_all(&dir);

    // Dropping XFA that holds a value the fields don't asks first: the
    // XFA copy holds `a`'s, which the AcroForm lost.
    let mut f = Form::new(2);
    let a = f.text("a", 0, row(0));
    f.text("b", 1, row(0));
    f.fill(a, "FROMXFA");
    f.xfa();
    f.dict(a).remove(b"V");
    let mut d = open(&f.bytes(), None);
    assert_eq!(d.state().form().xfa.values, 1);
    let second = d.page(1).unwrap();
    d.apply(Edit::DeletePages { ids: vec![second] }, "delete")
        .unwrap();
    let p = plan(
        &SaveInput::detached(&d.snapshot()),
        &target(Profile::default()),
    );
    assert_eq!(p.asks, [Ask::XfaHoldsValues { fields: 1 }]);
    assert!(matches!(p.transforms[0], SaveTransform::DropXfa { .. }));
}

/// A signed page whose signature's `/Reference` names its file's
/// catalog (`/Data`), and whose other widget has a stray key naming the
/// form: duplicated, and inserted into another file, its copies bring
/// neither the catalog nor the form along, and the form checks clean. The
/// copies once brought a second catalog and form, whose `/Fields` held
/// copies of the widgets on no page, which the post-condition refused.
#[test]
fn a_copy_doesnt_bring_its_file_s_catalog_or_form() {
    let mut f = Form::new(1);
    let sig = f.signature("Signature1", 0, row(0));
    let text = f.text("a", 0, row(1));
    f.certify(sig, 2);
    let v = f.dict(sig).get(b"V").unwrap().as_reference().unwrap();
    let catalog = f.catalog;
    let reference = f.dict(v).get_mut(b"Reference").unwrap();
    let reference = &mut reference.as_array_mut().unwrap()[0];
    reference.as_dict_mut().unwrap().set("Data", catalog);
    let form = f.acroform;
    f.dict(text).set("Parrent", form);
    let before = f.bytes();
    let catalogs = |out: &Output| {
        let pdf = lazy(&out.bytes, None);
        (pdf.ids())
            .filter(|&id| (pdf.get_dictionary(id).ok()).is_some_and(|d| d.has_type(b"Catalog")))
            .count()
    };
    let check = |out: &Output| {
        let report = crate::forms::check(&out.bytes, None).unwrap();
        let widgets = (report.findings.iter()).filter(|f| {
            matches!(
                f,
                crate::forms::Finding::OffPage(_) | crate::forms::Finding::Orphans(_)
            )
        });
        assert_eq!(widgets.count(), 0, "{:?}", report.findings);
    };

    let mut d = open(&before, None);
    let first = d.page(0).unwrap();
    d.apply(Edit::DuplicatePages { ids: vec![first] }, "dup")
        .unwrap();
    let out = rewrite(&d, Profile::default());
    assert_eq!(catalogs(&out), 1);
    check(&out);
    // The copy's /Data is the written catalog, its stray key the form.
    let now = source_of(&out);
    let copy = annots_on(&now, 1);
    let pdf = now.pdf();
    let written = pdf.trailer().get(b"Root").unwrap().as_reference().unwrap();
    let data = (copy.iter())
        .filter_map(|(_, w)| pdf.dereference(w.get(b"V").ok()?).ok())
        .filter_map(|(_, v)| v.as_dict().ok()?.get(b"Reference").ok()?.as_array().ok())
        .filter_map(|r| r[0].as_dict().ok()?.get(b"Data").ok()?.as_reference().ok())
        .collect::<Vec<_>>();
    assert_eq!(data, [written]);
    let form = pdf.catalog().unwrap().get(b"AcroForm").unwrap();
    let parrent = (copy.iter()).filter_map(|(_, w)| w.get(b"Parrent").ok());
    assert!(parrent.into_iter().all(|p| p == form));

    let mut d = open(&bytes(&file("A", 1, false)), None);
    let other = add(&mut d, &before, "signed.pdf");
    insert(&mut d, other, 0);
    insert(&mut d, other, 0);
    let out = rewrite(&d, Profile::default());
    assert_eq!(catalogs(&out), 1);
    check(&out);
}

/// The post-conditions fail closed: an emitted object over a primary
/// number it isn't, a reference to nothing, and a page the file shouldn't
/// reach each stop the write; a reference the input had to nothing, and a
/// page template, don't.
#[test]
fn the_post_conditions_fail_closed() {
    let mut pdf = Pdf::with_version("1.7");
    let pages = pdf.new_object_id();
    let page = pdf.add_object(dictionary! { "Type" => "Page", "Parent" => pages });
    let stray = pdf.new_object_id();
    pdf.objects.insert(
        pages,
        dictionary! { "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1 }.into(),
    );
    let catalog = pdf.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
    pdf.trailer.set("Root", catalog);
    let tree: HashSet<ObjectId> = [pages, page].into();
    let in_use: HashSet<u32> = pdf.objects.keys().map(|id| id.0).collect();
    let graph = |pdf: &Pdf, keys: Vec<(ObjectId, ObjKey)>| Graph {
        pdf: pdf.clone(),
        keys: keys.into_iter().collect(),
        deferred: Default::default(),
        followed: Default::default(),
        moved: 0,
    };
    let reads = |_| true;
    let primary = verify::Primary {
        in_use: &in_use,
        reads: &reads,
        start: in_use.iter().max().unwrap() + 1,
    };
    let check = |g: &Graph, owned: &std::collections::BTreeMap<ObjKey, ObjectId>| {
        verify::graph(g, &primary, owned, &tree, true, None)
    };
    let none = std::collections::BTreeMap::new();
    assert!(check(&graph(&pdf, Vec::new()), &none).is_ok());
    // A page's key at its own number is fine only when the page owns it.
    let key = ObjKey::Page(PageId(1));
    let over = graph(&pdf, vec![(page, key)]);
    let e = check(&over, &none).unwrap_err();
    assert!(e.to_string().contains("written over"), "{e}");
    assert!(check(&over, &[(key, page)].into()).is_ok());
    // A reference to a number the save gave that nothing has; one to a
    // number the primary never used was its own.
    let mut damaged = pdf.clone();
    (damaged.get_dictionary_mut(page).unwrap()).set("Annots", vec![Object::Reference((0, 0))]);
    assert!(check(&graph(&damaged, vec![(page, key)]), &[(key, page)].into()).is_ok());
    let mut dangling = pdf.clone();
    (dangling.get_dictionary_mut(page).unwrap()).set("Annots", vec![Object::Reference((99, 0))]);
    let e = check(&graph(&dangling, vec![(page, key)]), &[(key, page)].into()).unwrap_err();
    assert!(e.to_string().contains("hasn't got"), "{e}");
    // A page the tree doesn't keep, reached: what nothing reaches was
    // pruned before the checks.
    let mut reaching = pdf.clone();
    (reaching.objects).insert(stray, dictionary! { "Type" => "Page" }.into());
    (reaching.get_dictionary_mut(catalog).unwrap()).set("OpenAction", vec![stray.into()]);
    let e = check(&graph(&reaching, Vec::new()), &none).unwrap_err();
    assert!(e.to_string().contains("doesn't keep"), "{e}");
    // Unless it's a page template.
    (reaching.get_dictionary_mut(catalog).unwrap()).set(
        "Names",
        dictionary! { "Templates" => dictionary! {
            "Names" => vec![Object::string_literal("t"), stray.into()],
        } },
    );
    assert!(check(&graph(&reaching, Vec::new()), &none).is_ok());
}

/// A widget no form reaches whose field names another widget, on no
/// page: a duplicate of its page copies the widget it shows, and not the
/// other, which would be a copy on no page.
#[test]
fn a_copy_brings_no_widget_its_page_doesnt_show() {
    let mut f = Form::new(1);
    let page = f.pages[0];
    let field = f.pdf.new_object_id();
    let widget = |f: &mut Form, at: usize| {
        let rect: Vec<Object> = row(at).into_iter().map(Object::from).collect();
        f.pdf.add_object(dictionary! {
            "Type" => "Annot", "Subtype" => "Widget", "Rect" => rect, "Parent" => field,
        })
    };
    let shown = widget(&mut f, 0);
    f.dict(shown).set("P", page);
    let unshown = widget(&mut f, 1);
    f.pdf.objects.insert(
        field,
        dictionary! {
            "FT" => "Tx", "T" => Object::string_literal("stray"),
            "Kids" => vec![shown.into(), unshown.into()],
        }
        .into(),
    );
    f.set_annots(0, &[shown]);
    let before = f.bytes();
    let widgets = |bytes: &[u8]| {
        let pdf = lazy(bytes, None);
        (pdf.ids())
            .filter(|&id| {
                (pdf.get_dictionary(id).ok()).is_some_and(|d| {
                    d.get(b"Subtype").and_then(Object::as_name).ok() == Some(b"Widget".as_slice())
                })
            })
            .count()
    };
    assert_eq!(widgets(&before), 2);
    let mut d = open(&before, None);
    let first = d.page(0).unwrap();
    d.apply(Edit::DuplicatePages { ids: vec![first] }, "dup")
        .unwrap();
    let out = rewrite(&d, Profile::default());
    assert_eq!(widgets(&out.bytes), 3, "the two, and the shown one's copy");
}

/// Copy-on-write fails closed on the primary's own objects too: one the
/// graph holds otherwise than its source reads stops the write, unless a
/// step of the Rewrite says it changed it, or it's emitted (whose number
/// the ownership check judges).
#[test]
fn copy_on_write_compares_the_primary_s_own_objects_with_their_source() {
    let src = source(&bytes(&file("A", 1, false)), None);
    let in_use: HashSet<u32> = src.pdf().ids().map(|id| id.0).collect();
    let graph = |pdf: Pdf, keys: Vec<(ObjectId, ObjKey)>| Graph {
        pdf,
        keys: keys.into_iter().collect(),
        deferred: Default::default(),
        followed: Default::default(),
        moved: 0,
    };
    let none = |_| false;
    let loaded = super::graph::eager(&src).unwrap();
    assert!(verify::copied(&graph(loaded.clone(), Vec::new()), &src, &in_use, &none).is_ok());
    let info = (loaded.trailer.get(b"Info").unwrap().as_reference()).unwrap();
    let mut edited = loaded.clone();
    (edited.get_dictionary_mut(info).unwrap()).set("Title", Object::string_literal("B"));
    let e = verify::copied(&graph(edited.clone(), Vec::new()), &src, &in_use, &none).unwrap_err();
    assert!(e.to_string().contains("other content"), "{e}");
    let said = |id| id == info;
    assert!(verify::copied(&graph(edited.clone(), Vec::new()), &src, &in_use, &said).is_ok());
    let emitted = graph(edited, vec![(info, ObjKey::Info)]);
    assert!(verify::copied(&emitted, &src, &in_use, &none).is_ok());
}

/// The written form's check fails closed by what each finding names: a
/// widget on no page that no input had stops the write, though an input
/// had another on no page; one an input had passes, under its name or
/// under a parent sheaf gave its form; and a form the save didn't write
/// anew isn't checked.
#[test]
fn the_form_post_condition_fails_closed_by_name() {
    // `names`' fields, each a widget on no page, under `parent` when it's
    // given.
    let form = |names: &[&str], parent: Option<&str>| {
        let mut f = Form::new(1);
        let fields: Vec<ObjectId> = (names.iter().enumerate())
            .map(|(i, name)| f.text(name, 0, row(i)))
            .collect();
        if let Some(parent) = parent {
            f.group(parent, &fields);
        }
        f.set_annots(0, &[]);
        f.bytes()
    };
    let input = crate::forms::fixture::source(form(&["x"], None));
    let check = |bytes: &[u8], form_changed: bool| {
        let g = Graph {
            pdf: crate::file::load_pdf_mem(bytes, None).unwrap(),
            keys: Default::default(),
            deferred: Default::default(),
            followed: Default::default(),
            moved: 0,
        };
        let out = verify::Written {
            bytes,
            form_changed,
            password: None,
            encrypted: false,
        };
        verify::output(&out, &g, &[&input])
    };
    assert!(check(&form(&["x"], None), true).is_ok());
    assert!(check(&form(&["x"], Some("P")), true).is_ok());
    let both = form(&["x", "y"], None);
    let e = check(&both, true).unwrap_err().to_string();
    assert!(e.contains("widgets on no page (\"y\")"), "{e}");
    assert!(check(&both, false).is_ok());
}

/// Reading back fails closed: an object written in the clear into an
/// encrypted file (as a raw copy of an unencrypted one would be) doesn't
/// read back as the graph has it, and stops the write.
#[cfg(debug_assertions)]
#[test]
fn an_object_written_in_the_clear_doesnt_read_back() {
    use crate::serialize::fixture::{USER, encrypted, rc4_aes_128_and_aes_256};
    use crate::serialize::full::{Body, Layout, write_bodies};
    use std::borrow::Cow;

    let plain = crate::serialize::fixture::form();
    let [(_, state), ..] = rc4_aes_128_and_aes_256(&plain);
    let [table, _] = encrypted(&plain, &state, |_| {});
    let src = source(&table, Some(USER));
    let g = Graph {
        pdf: super::graph::eager(&src).unwrap(),
        keys: Default::default(),
        deferred: Default::default(),
        followed: Default::default(),
        moved: 0,
    };
    let clear = bytes(&plain);
    let in_clear = lazy(&clear, None);
    // An object with a string, which encrypting changes: the attachment's
    // file specification.
    let spec = (plain.objects.iter())
        .find(|(_, o)| o.as_dict().is_ok_and(|d| d.has_type(b"Filespec")))
        .map(|(&id, _)| id)
        .filter(|&id| in_clear.raw_span_whole(id).is_some())
        .expect("a file specification");
    let crypt = src.crypt().unwrap();
    let write = |raw: bool| {
        let bodies = (g.pdf.objects.iter()).map(|(&id, obj)| {
            let key = g.key(id, src.id());
            Ok(match raw && id == spec {
                true => (id, Body::Raw(&clear[in_clear.raw_span_whole(id).unwrap()])),
                false => (id, Body::Object(Cow::Borrowed(obj), key)),
            })
        });
        let layout = Layout {
            trailer: &g.pdf.trailer,
            kind: src.xref().kind,
            version: super::emit::PdfVersion::new(1, 7),
            crypt: Some(crypt),
            encrypt_at: crypt.at(),
            xref_from: 1,
        };
        write_bodies(bodies, &layout, &NoProgress).unwrap().0
    };
    let check = |bytes: &[u8]| {
        let out = verify::Written {
            bytes,
            form_changed: false,
            password: Some(USER),
            encrypted: true,
        };
        verify::output(&out, &g, &[])
    };
    assert!(check(&write(false)).is_ok());
    let e = check(&write(true)).unwrap_err();
    assert!(
        format!("{e:#}").contains("wasn't encrypted as written"),
        "{e:#}"
    );
}

/// A form the state doesn't model (a document made without reading its
/// form, as `Document::new` makes one) is kept as the file has it: its
/// fields in `/Fields`, its widgets on their pages, the form checking
/// clean.
#[test]
fn a_form_the_state_doesnt_model_is_kept_as_the_file_has_it() {
    let mut f = Form::new(2);
    let a = f.text("a", 0, row(0));
    f.fill(a, "Ada");
    f.radio("pick", 1, &[row(1), row(2)], Some(0));
    let bytes = f.bytes();
    let store = crate::store::Store::new();
    let kind = crate::store::BlobKind::file(std::path::Path::new("f.pdf"), None);
    let primary = crate::model::SourceInfo {
        src: store.open(bytes.clone().into(), kind).unwrap(),
        name: "f.pdf".into(),
        derived: None,
    };
    let d = Document::new(
        store,
        primary,
        None,
        &[],
        Box::new(crate::history::FixedCount(10)),
    );
    assert_eq!(d.state().field_count(), 0, "not modeled");
    let out = rewrite(&d, Profile::default());
    let report = crate::forms::check(&out.bytes, None).unwrap();
    assert!(report.is_clean(), "{:?}", report.findings);
    assert_eq!((report.fields, report.widgets), (2, 3));
    let now = source_of(&out);
    assert_eq!(annots_on(&now, 0).len(), 1);
    assert_eq!(annots_on(&now, 1).len(), 2);
}
