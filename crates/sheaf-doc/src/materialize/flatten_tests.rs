//! Annotations flattened (M10.5 slice 19): what each shows drawn into its
//! page as a layer of placed forms, `/SheafBlock << /K /Flat /B (b…) >>`,
//! after the page's isolated content, and the annotation gone. Print,
//! Hidden and NoView are honored, links stay, a widget draws the state its
//! `/AS` names and nothing without one, while another tool's comment draws
//! what PDFium shows, its `/V`'s state or its `/Off`, never its first; a
//! mark is drawn again from what it's pinned to, with nothing
//! of it left. Opened again, the block is the file's, kept in its slot,
//! and a second flatten adds another after it.

use in_progress::NoProgress;
use lopdf::{Dictionary, Document as Pdf, Object, ObjectId, Stream, dictionary};
use sheaf_edit::{FlattenSet, Rotation};

use super::How;
use super::recognized_tests::{contents, depth, isolates, pages, stream};
use super::removal_tests::{opened, scratch};
use crate::annot::{Annot, AnnotId, AnnotKind, Box2};
use crate::content::{BlockAt, BlockId, SheafBlock, SheafBlockRef};
use crate::effects::Flags;
use crate::forms::fixture::Form;
use crate::model::{Document, FormRef, PageId, PlaceTag, Placement, Static};
use crate::realize::{ViewInputs, emission_is_base, untouched};
use crate::tx::fixture as fx;
use crate::tx::{Actor, EditError};

/// What [`annotated`]'s first page holds, by object.
struct Held {
    /// A check box that's on, and one with states and no `/AS`.
    on: ObjectId,
    unnamed: ObjectId,
    /// A text field, filled.
    text: ObjectId,
    /// Another tool's squares: printed, not printed, hidden, never shown.
    printed: ObjectId,
    unprinted: ObjectId,
    hidden: ObjectId,
    unviewed: ObjectId,
    /// Another tool's comment whose appearance has states, `/Off` the
    /// second, and none named.
    stated: ObjectId,
    link: ObjectId,
}

/// A two-page form (tagged, when `tagged`) whose first page holds [`Held`]'s
/// annotations, each but the link with an appearance of its own.
fn annotated(tagged: bool) -> (Vec<u8>, Held) {
    let mut f = Form::new(2);
    let on = f.check_box("On", 0, [72.0, 600.0, 92.0, 620.0], true);
    let unnamed = f.check_box("Unnamed", 0, [100.0, 600.0, 120.0, 620.0], true);
    f.dict(unnamed).remove(b"AS");
    let text = f.text("Name", 0, [150.0, 600.0, 300.0, 620.0]);
    f.fill(text, "Ada");
    let page = f.pages[0];
    let form = |f: &mut Form, ops: &str| {
        let dict = dictionary! {
            "Type" => "XObject", "Subtype" => "Form",
            "BBox" => vec![0.into(), 0.into(), 50.into(), 25.into()],
        };
        f.pdf.add_object(Stream::new(dict, ops.as_bytes().to_vec()))
    };
    let annot = |f: &mut Form, name: &str, flags: i64, x: i64, ap: Dictionary| {
        let id = f.pdf.add_object(dictionary! {
            "Type" => "Annot", "Subtype" => "Square", "P" => page,
            "Rect" => vec![x.into(), 400.into(), (x + 100).into(), 450.into()],
            "NM" => Object::string_literal(name), "F" => flags, "AP" => ap,
        });
        let mut annots = f.annots(0);
        annots.push(id);
        f.set_annots(0, &annots);
        id
    };
    let square = |f: &mut Form, name: &str, flags: i64, x: i64| {
        let n = form(f, "0 0 1 rg 0 0 50 25 re f");
        annot(f, name, flags, x, dictionary! { "N" => n })
    };
    let printed = square(&mut f, "printed", 4, 72);
    let unprinted = square(&mut f, "unprinted", 0, 180);
    let hidden = square(&mut f, "hidden", 4 | 2, 290);
    let unviewed = square(&mut f, "unviewed", 4 | 32, 400);
    let [first, second] =
        ["1 0 0 rg 0 0 50 25 re f", "0 1 0 rg 0 0 50 25 re f"].map(|ops| form(&mut f, ops));
    let states = dictionary! { "Red" => first, "Off" => second };
    let stated = annot(&mut f, "stated", 4, 72, dictionary! { "N" => states });
    f.dict(stated)
        .set("Rect", vec![72.into(), 300.into(), 172.into(), 350.into()]);
    let link = f.pdf.add_object(dictionary! {
        "Type" => "Annot", "Subtype" => "Link", "F" => 4,
        "Rect" => vec![72.into(), 200.into(), 172.into(), 220.into()],
        "Dest" => vec![f.pages[1].into(), "Fit".into()],
    });
    let mut annots = f.annots(0);
    annots.push(link);
    f.set_annots(0, &annots);
    if tagged {
        f.tag();
    }
    let held = Held {
        on,
        unnamed,
        text,
        printed,
        unprinted,
        hidden,
        unviewed,
        stated,
        link,
    };
    (f.bytes(), held)
}

/// A square mark of sheaf's on page `page`, and a replacement of text,
/// which writes a struck-out companion beside its caret.
fn marks(doc: &mut Document, page: PageId) -> [AnnotId; 2] {
    let mark = |kind| Annot {
        id: AnnotId(0),
        kind,
        color: [200, 0, 0],
        opacity: 1.0,
        stroke: 2.0,
        author: "Me".into(),
        contents: "Look".into(),
        modified: String::new(),
        created: String::new(),
        fill: None,
        dashed: false,
        cloudy: false,
        turn: Rotation::R0,
        name: String::new(),
    };
    let square = mark(AnnotKind::Square {
        rect: Box2 {
            x: 300.0,
            y: 100.0,
            w: 80.0,
            h: 40.0,
        },
    });
    let words = Box2 {
        x: 100.0,
        y: 100.0,
        w: 60.0,
        h: 12.0,
    };
    let replace = mark(AnnotKind::Replace {
        rects: vec![words],
        struck: "old".into(),
        at: crate::annot::caret_box(160.0, 100.0, 12.0),
    });
    let ops = vec![fx::add(page, &square), fx::add(page, &replace)];
    let made = doc.commit_ops(ops, "marks").unwrap().created;
    let made: Vec<AnnotId> = made.annots().collect();
    [made[0], made[1]]
}

/// The objects page `page`'s annotations of `doc` were read as: None for a
/// mark of sheaf's made since.
fn objects_on(doc: &Document, page: PageId) -> Vec<Option<ObjectId>> {
    (doc.state().annots_on(page))
        .map(|(_, rec)| match &rec.body {
            crate::model::AnnotBody::Mark(m) => m.src.as_ref().map(|r| r.id),
            crate::model::AnnotBody::Foreign(f) => f.src.as_ref().map(|r| r.id),
            crate::model::AnnotBody::Widget(w) => w.src.as_ref().map(|r| r.id),
        })
        .collect()
}

/// Page `page`'s placements, its layers' in order.
fn placements(doc: &Document, page: PageId) -> Vec<Placement> {
    let def = doc.state().def(page).unwrap();
    (def.content.over.iter())
        .flat_map(|l| match &l.body {
            Static::Placements(placed) => placed.to_vec(),
            Static::OcrText(_) => Vec::new(),
        })
        .collect()
}

/// The source's form `p` places, when it places one.
fn base(p: &Placement) -> Option<ObjectId> {
    match &p.form {
        FormRef::Base(at) => Some(at.id),
        FormRef::Blob(_) | FormRef::Pinned(_) => None,
    }
}

/// The normal appearance form annotation `id` of `pdf` names, its state
/// `state`'s when it has states.
fn appearance(pdf: &Pdf, id: ObjectId, state: Option<&[u8]>) -> ObjectId {
    let ap = pdf.get_dictionary(id).unwrap().get(b"AP").unwrap();
    let n = ap.as_dict().unwrap().get(b"N").unwrap();
    match (n, state) {
        (Object::Reference(r), None) => *r,
        (Object::Dictionary(states), Some(state)) => {
            states.get(state).unwrap().as_reference().unwrap()
        }
        (n, _) => panic!("{n:?}"),
    }
}

/// Flattened, every annotation shown on the first page but the link draws
/// its appearance there and goes: sheaf's marks drawn again from what
/// they're pinned to, a replacement's caret and struck text both; the
/// form's widgets by the state their `/AS` names, the one with states and
/// none named drawing nothing; another tool's comment with states and none
/// named by its `/Off`, as PDFium shows it, not its first. The hidden and
/// the never shown stay, as does the link, and nothing on the other page
/// changes. Fields left with no widgets go.
#[test]
fn flatten_takes_what_shows_and_leaves_the_rest() {
    let dir = scratch();
    let (bytes, held) = annotated(false);
    let (_, mut doc) = opened(&dir, "a.pdf", &bytes);
    let [p0, p1] = [0, 1].map(|i| doc.page(i).unwrap());
    let [square, replace] = marks(&mut doc, p0);
    let commit =
        (doc.commit_ops(vec![fx::flatten(&[p0, p1], FlattenSet::All)], "flatten")).unwrap();
    assert_eq!(commit.lines[0].text, "Flatten 8 annotations on pages 1, 2");
    assert!(commit.lines[0].removes);
    let flags = commit.effects.effects.flags;
    assert!(flags.contains(Flags::CONTENT | Flags::REMOVES), "{flags:?}");
    let left = [held.hidden, held.unviewed, held.link].map(Some);
    assert_eq!(objects_on(&doc, p0), left);
    let st = doc.state();
    assert!(st.annot(square).is_none() && st.annot(replace).is_none());
    let names: Vec<String> = (st.fields())
        .map(|(id, _)| st.full_name(id).unwrap())
        .collect();
    assert!(names.is_empty(), "{names:?}");
    // In the page's annotations' order: the widgets, the squares, the
    // comment with states, then sheaf's marks, a replacement's two.
    let pdf = Pdf::load_mem(&bytes).unwrap();
    let placed = placements(&doc, p0);
    let forms: Vec<Option<ObjectId>> = placed.iter().map(base).collect();
    let want = [
        Some(appearance(&pdf, held.on, Some(b"Yes"))),
        Some(appearance(&pdf, held.text, None)),
        Some(appearance(&pdf, held.printed, None)),
        Some(appearance(&pdf, held.unprinted, None)),
        Some(appearance(&pdf, held.stated, Some(b"Off"))),
        None,
        None,
        None,
    ];
    assert_eq!(forms, want);
    let unnamed = [b"Yes", b"Off"].map(|state| appearance(&pdf, held.unnamed, Some(state)));
    assert!(forms.iter().flatten().all(|f| !unnamed.contains(f)));
    assert!(placed.iter().all(|p| p.tag == PlaceTag::Untagged));
    // Each form's box onto its annotation's: the squares' 50 by 25 onto
    // 100 by 50 at x, 400; a mark's appearance drawn where it is.
    let square_at = |x: f32| crate::geom::Affine {
        a: 2.0,
        b: 0.0,
        c: 0.0,
        d: 2.0,
        e: x,
        f: 400.0,
    };
    assert_eq!(placed[2].matrix, square_at(72.0));
    assert_eq!(placed[3].matrix, square_at(180.0));
    assert!(
        placed[5..]
            .iter()
            .all(|p| p.matrix == crate::geom::Affine::IDENTITY)
    );
    let kinds: Vec<&str> = (placed[5..].iter())
        .map(|p| match &p.form {
            FormRef::Pinned(ap) => match ap.inputs.kind {
                AnnotKind::Square { .. } => "Square",
                AnnotKind::Replace { .. } => "Replace",
                AnnotKind::StrikeOut { .. } => "StrikeOut",
                _ => "another",
            },
            _ => panic!("a mark's is pinned"),
        })
        .collect();
    assert_eq!(kinds, ["Square", "Replace", "StrikeOut"]);
    assert!(placements(&doc, p1).is_empty());
}

/// Only what's printed is flattened under Printable: another tool's
/// comment without the Print flag stays, with the hidden, the never shown
/// and the link.
#[test]
fn printable_leaves_what_isnt_printed() {
    let dir = scratch();
    let (bytes, held) = annotated(false);
    let (_, mut doc) = opened(&dir, "a.pdf", &bytes);
    let p0 = doc.page(0).unwrap();
    let commit =
        (doc.commit_ops(vec![fx::flatten(&[p0], FlattenSet::Printable)], "print")).unwrap();
    assert_eq!(
        commit.lines[0].text,
        "Flatten 5 printable annotations on page 1"
    );
    let left = [held.unprinted, held.hidden, held.unviewed, held.link].map(Some);
    assert_eq!(objects_on(&doc, p0), left);
    let pdf = Pdf::load_mem(&bytes).unwrap();
    let forms: Vec<Option<ObjectId>> = placements(&doc, p0).iter().map(base).collect();
    let want = [
        appearance(&pdf, held.on, Some(b"Yes")),
        appearance(&pdf, held.text, None),
        appearance(&pdf, held.printed, None),
        appearance(&pdf, held.stated, Some(b"Off")),
    ]
    .map(Some);
    assert_eq!(forms, want);
}

/// Another tool's comment whose appearance has states draws the one PDFium
/// shows, as the page view does: its `/AS`'s, else the one its `/V` (or
/// its parent's) names, else its `/Off`, else nothing; never its first,
/// nor one made of its border or fill, as pdf.js makes. A string names a
/// state as a name does, and an `/AS` naming none draws nothing.
#[test]
fn a_comment_with_states_draws_the_one_pdfium_shows() {
    let mut f = Form::new(1);
    let page = f.pages[0];
    let cases: [(&[&str], Dictionary, Option<&str>); 9] = [
        (&["Red", "Green"], dictionary! {}, None),
        (
            &["Red", "Green"],
            dictionary! { "BS" => dictionary! { "W" => 4 } },
            None,
        ),
        (
            &["Red", "Green"],
            dictionary! { "IC" => vec![0.into(), 1.into(), 0.into()] },
            None,
        ),
        (&["Red", "Off"], dictionary! {}, Some("Off")),
        (
            &["Red", "Green"],
            dictionary! { "V" => "Green" },
            Some("Green"),
        ),
        (
            &["Red", "Green"],
            dictionary! { "Parent" => dictionary! { "V" => "Green" } },
            Some("Green"),
        ),
        (&["Red", "Off"], dictionary! { "V" => "Blue" }, Some("Off")),
        (&["Red", "Off"], dictionary! { "AS" => "Blue" }, None),
        (
            &["Red", "Green"],
            dictionary! { "AS" => Object::string_literal("Green") },
            Some("Green"),
        ),
    ];
    let mut want = Vec::new();
    for (i, (states, extra, shows)) in cases.into_iter().enumerate() {
        let mut ap = Dictionary::new();
        for (n, state) in states.iter().enumerate() {
            let dict = dictionary! {
                "Type" => "XObject", "Subtype" => "Form",
                "BBox" => vec![0.into(), 0.into(), 50.into(), 25.into()],
            };
            let ops = format!("{n} 0 0 rg 0 0 50 25 re f");
            let form = f.pdf.add_object(Stream::new(dict, ops.into_bytes()));
            ap.set(*state, form);
            if shows == Some(*state) {
                want.push(Some(form));
            }
        }
        let y = 700 - 60 * i as i64;
        let mut dict = dictionary! {
            "Type" => "Annot", "Subtype" => "Square", "P" => page, "F" => 4,
            "Rect" => vec![72.into(), y.into(), 172.into(), (y + 50).into()],
            "NM" => Object::string_literal(format!("square{i}")),
            "AP" => dictionary! { "N" => ap },
        };
        for (k, v) in extra.iter() {
            dict.set(k.clone(), v.clone());
        }
        let id = f.pdf.add_object(dict);
        let mut annots = f.annots(0);
        annots.push(id);
        f.set_annots(0, &annots);
    }
    let dir = scratch();
    let (_, mut doc) = opened(&dir, "a.pdf", &f.bytes());
    let p0 = doc.page(0).unwrap();
    let commit = (doc.commit_ops(vec![fx::flatten(&[p0], FlattenSet::All)], "flatten")).unwrap();
    assert_eq!(commit.lines[0].text, "Flatten 9 annotations on page 1");
    let forms: Vec<Option<ObjectId>> = placements(&doc, p0).iter().map(base).collect();
    assert_eq!(forms, want);
}

/// Undone, the annotations are back and the page draws nothing over it;
/// done again, the same state.
#[test]
fn undo_and_redo_put_the_annotations_back_and_take_them_again() {
    let dir = scratch();
    let (bytes, _) = annotated(false);
    let (_, mut doc) = opened(&dir, "a.pdf", &bytes);
    let p0 = doc.page(0).unwrap();
    marks(&mut doc, p0);
    let before = doc.state().clone();
    doc.commit_ops(vec![fx::flatten(&[p0], FlattenSet::All)], "flatten")
        .unwrap();
    let after = doc.state().clone();
    doc.undo().unwrap();
    assert_eq!(*doc.state(), before);
    assert!(placements(&doc, p0).is_empty());
    doc.redo().unwrap();
    assert_eq!(*doc.state(), after);
}

/// Canceled, a flatten leaves the transaction as it was, and can go on.
#[test]
fn a_canceled_flatten_leaves_the_transaction_as_it_was() {
    let dir = scratch();
    let (bytes, _) = annotated(false);
    let (_, doc) = opened(&dir, "a.pdf", &bytes);
    let pages = [0, 1].map(|i| doc.page(i).unwrap());
    let mut tx = doc.tx(Actor::User);
    let state = tx.state().clone();
    let canceled = in_progress::Tracker::new();
    canceled.cancel();
    let flatten = fx::flatten(&pages, FlattenSet::All);
    assert_eq!(
        tx.apply(flatten.clone(), &canceled).map(|_| ()),
        Err(EditError::Cancelled)
    );
    assert_eq!(*tx.state(), state);
    assert!(tx.preview().lines.is_empty());
    tx.apply(flatten, &NoProgress).unwrap();
    assert_eq!(placements_in(tx.state(), pages[0]), 5);
}

/// How many forms page `page` of `st` places.
fn placements_in(st: &crate::DocState, page: PageId) -> usize {
    let def = st.def(page).unwrap();
    (def.content.over.iter())
        .map(|l| match &l.body {
            Static::Placements(placed) => placed.len(),
            Static::OcrText(_) => 0,
        })
        .sum()
}

/// The block stream `id` of `pdf` is a flattened layer's, by its id.
fn flat_block(pdf: &Pdf, id: ObjectId) -> Option<BlockId> {
    let (dict, _) = stream(pdf, id);
    let marker = dict.get(b"SheafBlock").ok()?.as_dict().ok()?;
    if marker.get(b"K").and_then(Object::as_name).ok()? != b"Flat" {
        return None;
    }
    BlockId::parse(marker.get(b"B").ok()?.as_str().ok()?)
}

/// What each of page `page`'s streams of `pdf` is: `Isolate`, `Flat`, or
/// `-`, the page's own.
fn kinds(pdf: &Pdf, page: ObjectId) -> Vec<&'static str> {
    (contents(pdf, page).into_iter())
        .map(|c| match (isolates(pdf, c), flat_block(pdf, c)) {
            (true, _) => "Isolate",
            (_, Some(_)) => "Flat",
            _ => "-",
        })
        .collect()
}

/// The XObject page `page` of `pdf` names `name`.
fn xobject(pdf: &Pdf, page: ObjectId, name: &[u8]) -> ObjectId {
    let (res, ids) = pdf.get_page_resources(page).unwrap();
    let res = match res {
        Some(d) => d.clone(),
        None => pdf.get_dictionary(ids[0]).unwrap().clone(),
    };
    let xo = pdf.dereference(res.get(b"XObject").unwrap()).unwrap().1;
    xo.as_dict()
        .unwrap()
        .get(name)
        .unwrap()
        .as_reference()
        .unwrap()
}

/// The names a flattened block draws, in order.
fn drawn_names(ops: &[u8]) -> Vec<Vec<u8>> {
    let text = String::from_utf8_lossy(ops);
    (text.lines())
        .filter_map(|l| l.strip_suffix(" Do"))
        .map(|n| n.trim_start_matches('/').as_bytes().to_vec())
        .collect()
}

crate::both_ways!(
    a_flattened_page_draws_its_layer_after_its_content,
    a_flattened_page_draws_its_layer_after_its_content_appended: drawn_after
);

/// Saved `how`, a flattened page draws its own content isolated, then its
/// layer's block, which places each form in a `q` of its own through its
/// matrix, each an artifact on a page of a tagged file: another tool's
/// appearance as the file has it, a mark's drawn again. Its annotations
/// are the link, the hidden and the never shown; no field is left, and the
/// structure tree no longer reaches what went.
fn drawn_after(how: How) {
    let dir = scratch();
    for tagged in [false, true] {
        let (bytes, held) = annotated(tagged);
        let (_, mut doc) = opened(&dir, &format!("{tagged}.pdf"), &bytes);
        let p0 = doc.page(0).unwrap();
        marks(&mut doc, p0);
        doc.commit_ops(vec![fx::flatten(&[p0], FlattenSet::All)], "flatten")
            .unwrap();
        let id = doc.state().def(p0).unwrap().content.over[0].id;
        let out = Pdf::load_mem(&how.write(&doc)).unwrap();
        let at = format!("{how:?}, tagged {tagged}");
        let page = pages(&out)[0];
        assert_eq!(
            kinds(&out, page),
            ["Isolate", "-", "Isolate", "Flat"],
            "{at}"
        );
        let block = contents(&out, page)[3];
        assert_eq!(flat_block(&out, block), Some(id), "{at}");
        let ops = stream(&out, block).1;
        let text = String::from_utf8_lossy(&ops).into_owned();
        let names = drawn_names(&ops);
        assert_eq!(names.len(), 8, "{at}: {text}");
        let tags = text.matches("/Artifact BMC").count();
        assert_eq!(tags, if tagged { 8 } else { 0 }, "{at}: {text}");
        assert!(
            text.starts_with(if tagged { "/Artifact BMC\nq\n" } else { "q\n" }),
            "{at}"
        );
        assert!(text.contains("q\n2 0 0 2 72 400 cm\n/Sh"), "{at}: {text}");
        // Others' appearances are the file's own objects, at their numbers.
        let read = Pdf::load_mem(&bytes).unwrap();
        let theirs = [
            appearance(&read, held.on, Some(b"Yes")),
            appearance(&read, held.text, None),
            appearance(&read, held.printed, None),
        ];
        for (name, form) in names.iter().zip(theirs) {
            assert_eq!(xobject(&out, page, name), form, "{at}");
        }
        // A mark's, drawn again: the square's stroke.
        let square = xobject(&out, page, &names[5]);
        let (dict, data) = stream(&out, square);
        assert_eq!(dict.get(b"Subtype").unwrap().as_name().unwrap(), b"Form");
        let data = String::from_utf8_lossy(&data).into_owned();
        assert!(data.contains(" re\nS\n"), "{at}: {data}");
        let annots = (out
            .get_page_annotations(page)
            .unwrap_or_default()
            .into_iter())
        .map(|d| d.get(b"NM").ok().map(|n| n.as_str().unwrap().to_vec()))
        .collect::<Vec<_>>();
        let left: Vec<Option<Vec<u8>>> =
            vec![Some(b"hidden".to_vec()), Some(b"unviewed".to_vec()), None];
        assert_eq!(annots, left, "{at}");
        // An Append keeps what went in the revision before it.
        if how == How::Appended {
            continue;
        }
        let everything = super::recognized_tests::everything(&out);
        assert!(!everything.contains("Unnamed"), "{at}");
        assert!(!everything.contains("printed"), "{at}");
        // The structure tree's elements each name an annotation by an
        // object reference: those of the annotations gone went with them.
        if tagged {
            let elements = (out.objects.values())
                .filter_map(|o| o.as_dict().ok())
                .filter(|d| d.get(b"S").and_then(Object::as_name).ok() == Some(b"Form"))
                .count();
            assert_eq!(
                elements, 3,
                "{at}: the hidden, the never shown and the link"
            );
        }
    }
}

crate::both_ways!(
    a_flattened_layer_reopens_in_its_slot,
    a_flattened_layer_reopens_in_its_slot_appended: reopened_in_slot
);

/// Opened again, a flattened page's block is found among its blocks by
/// its marker (the file names no generator) and kept where it is: the
/// page is intact, rendered from its base, and a save writes its content
/// as it is. Flattened again, a mark made since draws in a second layer
/// after the first, and nesting doesn't grow.
fn reopened_in_slot(how: How) {
    let dir = scratch();
    let (bytes, _) = annotated(false);
    let (_, mut doc) = opened(&dir, "a.pdf", &bytes);
    let p0 = doc.page(0).unwrap();
    marks(&mut doc, p0);
    doc.commit_ops(vec![fx::flatten(&[p0], FlattenSet::All)], "flatten")
        .unwrap();
    let id = doc.state().def(p0).unwrap().content.over[0].id;
    // Flattened and unsaved, it's drawn over its base, under any inputs.
    for inputs in crate::realize::fixture::every_input() {
        assert!(!untouched(doc.state(), p0, &inputs), "{how:?}: {inputs:?}");
        assert!(!emission_is_base(doc.state(), p0, &inputs).unwrap());
    }
    let saved = how.write(&doc);
    let (_, mut again) = opened(&dir, "b.pdf", &saved);
    let file = Pdf::load_mem(&saved).unwrap();
    let block = contents(&file, pages(&file)[0])[3];
    let src = again.primary();
    let p0 = again.page(0).unwrap();
    let found = src.blocks(0).unwrap().sheaf.to_vec();
    let want = SheafBlockRef {
        block: SheafBlock::Flat(id),
        at: BlockAt::Stream(block),
    };
    assert_eq!(found, [want], "{how:?}");
    assert!(!src.marked(), "{how:?}: nothing names a generator");
    let st = again.state();
    for inputs in crate::realize::fixture::every_input() {
        assert!(untouched(st, p0, &inputs), "{how:?}: {inputs:?}");
        assert!(emission_is_base(st, p0, &inputs).unwrap(), "{how:?}");
    }
    let rewritten = Pdf::load_mem(&How::Rewritten.write(&again)).unwrap();
    let page = pages(&rewritten)[0];
    assert_eq!(contents(&rewritten, page), contents(&file, pages(&file)[0]));
    let deep = depth(&file, pages(&file)[0]);
    marks(&mut again, p0);
    again
        .commit_ops(vec![fx::flatten(&[p0], FlattenSet::All)], "again")
        .unwrap();
    let out = Pdf::load_mem(&how.write(&again)).unwrap();
    let page = pages(&out)[0];
    let at = format!("{how:?}");
    assert_eq!(
        kinds(&out, page),
        ["Isolate", "-", "Isolate", "Flat", "Flat"],
        "{at}"
    );
    assert_eq!(contents(&out, page)[3], block, "{at}");
    assert_eq!(depth(&out, page), deep, "{at}");
}

/// The same flattened document writes the same bytes each time: the
/// layer's block and the marks' forms, made as they're written, alike.
#[test]
fn a_flattened_document_writes_the_same_bytes_twice() {
    let dir = scratch();
    let (bytes, _) = annotated(true);
    let (_, mut doc) = opened(&dir, "a.pdf", &bytes);
    let p0 = doc.page(0).unwrap();
    marks(&mut doc, p0);
    doc.commit_ops(vec![fx::flatten(&[p0], FlattenSet::All)], "flatten")
        .unwrap();
    for how in [How::Rewritten, How::Appended] {
        assert_eq!(how.write(&doc), how.write(&doc), "{how:?}");
    }
}

/// A page whose content can't be read takes no flattened layer: the op
/// says so, and nothing is changed.
#[test]
fn a_page_that_cant_be_read_isnt_flattened() {
    let dir = scratch();
    let (bytes, _) = annotated(false);
    let mut pdf = Pdf::load_mem(&bytes).unwrap();
    let page = pdf.get_pages()[&1];
    let content = pdf.get_dictionary(page).unwrap().get(b"Contents").unwrap();
    let content = content.as_reference().unwrap();
    let bad = Stream::new(Dictionary::new(), b"0 0 9 9 re f EMC".to_vec());
    pdf.objects.insert(content, Object::Stream(bad));
    let mut bytes = Vec::new();
    pdf.save_to(&mut bytes).unwrap();
    let (_, mut doc) = opened(&dir, "a.pdf", &bytes);
    let p0 = doc.page(0).unwrap();
    let before = doc.state().clone();
    let err = doc
        .commit_ops(vec![fx::flatten(&[p0], FlattenSet::All)], "flatten")
        .unwrap_err();
    assert!(matches!(err, EditError::Content { .. }), "{err:?}");
    assert_eq!(*doc.state(), before);
}

/// A header set over a reopened flattened page, saved as planned: an
/// Append, which rebuilds the page around its own content and keeps the
/// layer's block, the file's own object, last, over the header's, as the
/// annotations showed over it. The removal check judges what it reads of
/// the page as the file's own, and vetoes nothing.
#[test]
fn an_append_over_a_saved_flattened_block_vetoes_nothing() {
    let dir = scratch();
    let (bytes, _) = annotated(false);
    let (_, mut doc) = opened(&dir, "a.pdf", &bytes);
    let p0 = doc.page(0).unwrap();
    marks(&mut doc, p0);
    doc.commit_ops(vec![fx::flatten(&[p0], FlattenSet::All)], "flatten")
        .unwrap();
    let saved = How::Rewritten.write(&doc);
    let was = Pdf::load_mem(&saved).unwrap();
    let block = contents(&was, pages(&was)[0])[3];
    let (_, mut again) = opened(&dir, "b.pdf", &saved);
    let p0 = again.page(0).unwrap();
    let set = fx::set_header(fx::header("Draft"), Some(&[p0]));
    again.commit_ops(vec![set], "header").unwrap();
    let before = super::removal::vetoes_here();
    let (_, section) = super::append_tests::planned_in_place(&mut again);
    assert!(!section.is_empty());
    assert_eq!(super::removal::vetoes_here(), before);
    let path = again.file().unwrap().0.to_path_buf();
    let pdf = Pdf::load(&path).unwrap();
    let page = pages(&pdf)[0];
    let now = contents(&pdf, page);
    assert_eq!(now.len(), 5, "{now:?}");
    assert!(super::recognized_tests::block_of(&pdf, now[3]).is_some());
    assert_eq!(now[4], block);
    // Opened again, the page is written as the header and the layer left
    // it.
    let (_, reopened) = opened(&dir, "c.pdf", &std::fs::read(&path).unwrap());
    let rewritten = Pdf::load_mem(&How::Rewritten.write(&reopened)).unwrap();
    assert_eq!(contents(&rewritten, pages(&rewritten)[0]), now);
}

crate::both_ways!(
    a_flattened_layer_draws_over_the_watermark,
    a_flattened_layer_draws_over_the_watermark_appended: over_the_watermark
);

/// Saved `how`, a page flattened under a watermark over its content draws
/// the layer last, as the annotations showed over the watermark; recognized
/// text stays before the watermark. Flattened again, the second layer
/// follows the first, both after the watermark's block.
fn over_the_watermark(how: How) {
    let dir = scratch();
    let (bytes, _) = annotated(false);
    let (_, mut doc) = opened(&dir, "a.pdf", &bytes);
    let p0 = doc.page(0).unwrap();
    marks(&mut doc, p0);
    let wm = fx::set_watermark(fx::watermark("Draft"), sheaf_edit::Z::Over, None);
    let ocr = fx::ocr([(p0, crate::realize::fixture::layer("t"))]);
    doc.commit_ops([vec![wm], ocr].concat(), "set").unwrap();
    doc.commit_ops(vec![fx::flatten(&[p0], FlattenSet::All)], "flatten")
        .unwrap();
    let out = Pdf::load_mem(&how.write(&doc)).unwrap();
    let page = pages(&out)[0];
    // Each stream's `/SheafBlock` kind, or `-`, the page's own.
    let kinds = |pdf: &Pdf, page: ObjectId| -> Vec<String> {
        (contents(pdf, page).into_iter())
            .map(|c| {
                let (dict, _) = stream(pdf, c);
                let kind = (dict.get(b"SheafBlock").and_then(Object::as_dict))
                    .and_then(|m| m.get(b"K").and_then(Object::as_name));
                kind.map_or("-".into(), |k| String::from_utf8_lossy(k).into_owned())
            })
            .collect()
    };
    let at = format!("{how:?}");
    assert_eq!(
        kinds(&out, page),
        ["Isolate", "-", "Isolate", "Ocr", "Gen", "Flat"],
        "{at}"
    );
    // Opened again, the page is in place, and written as it is.
    let saved = how.write(&doc);
    let (_, mut again) = opened(&dir, "b.pdf", &saved);
    let file = Pdf::load_mem(&saved).unwrap();
    let rewritten = Pdf::load_mem(&How::Rewritten.write(&again)).unwrap();
    let was = contents(&file, pages(&file)[0]);
    assert_eq!(contents(&rewritten, pages(&rewritten)[0]), was, "{at}");
    let p0 = again.page(0).unwrap();
    for inputs in [ViewInputs::screen(), ViewInputs::print(true)] {
        assert!(untouched(again.state(), p0, &inputs), "{at}: {inputs:?}");
    }
    marks(&mut again, p0);
    again
        .commit_ops(vec![fx::flatten(&[p0], FlattenSet::All)], "again")
        .unwrap();
    let out = Pdf::load_mem(&how.write(&again)).unwrap();
    let page = pages(&out)[0];
    assert_eq!(
        kinds(&out, page),
        ["Isolate", "-", "Isolate", "Ocr", "Gen", "Flat", "Flat"],
        "{at}"
    );
}

/// A pinned appearance as a one-page PDF, to show or copy: its page draws
/// the form the mark's appearance is, drawn from what it's pinned to.
#[test]
fn a_pinned_appearance_is_a_page_of_its_own() {
    let dir = scratch();
    let (bytes, _) = annotated(false);
    let (_, mut doc) = opened(&dir, "a.pdf", &bytes);
    let p0 = doc.page(0).unwrap();
    marks(&mut doc, p0);
    doc.commit_ops(vec![fx::flatten(&[p0], FlattenSet::All)], "flatten")
        .unwrap();
    let square = &placements(&doc, p0)[5].form;
    assert!(matches!(square, FormRef::Pinned(_)));
    let pdf = Pdf::load_mem(&square.to_pdf().unwrap()).unwrap();
    let page = pages(&pdf)[0];
    let (_, ops) = stream(&pdf, contents(&pdf, page)[0]);
    assert_eq!(ops, b"/Stamp Do");
    let form = xobject(&pdf, page, b"Stamp");
    let drawn = String::from_utf8_lossy(&stream(&pdf, form).1).into_owned();
    assert!(drawn.contains(" re\nS\n"), "{drawn}");
}
