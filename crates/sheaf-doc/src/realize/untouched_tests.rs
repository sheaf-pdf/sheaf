//! Which pages draw as their base pages do, and where the realizer sends
//! each (M10.5 §10.1, slice 13): the model's predicate agrees with what an
//! overlay of each page alone writes, page by page, under every consumer's
//! inputs, after every kind of edit; an untouched page is viewed in its
//! base, an edited one in its group, in base order then copy, and a group
//! is another engine once a page of it draws otherwise, and only then; a
//! source PDFium refuses, every page in its groups, files of their own; the
//! deltas kept, and the inputs' digests.

use std::sync::Arc;

use lopdf::{Object, dictionary};
use sheaf_edit::{Rotation, Turn};

use super::fixture::{every_input, hiding, layer, their_comment};
use super::*;
use crate::forms::Value;
use crate::forms::fixture::Form;
use crate::materialize::header_tests::ACROBAT_HEADER;
use crate::materialize::others_tests::{ACROBAT, BODY, acrobat_file};
use crate::materialize::tests::{add, bytes, file, insert, open};
use crate::model::history_tests::{with_ours, with_theirs};
use crate::model::{FieldNode, Terminal};
use crate::prim::{Key, Prim, Val};
use crate::tx::fixture as fx;
use crate::tx::tests::opened;

/// Whether each page of `st` is untouched under `inputs`, in order,
/// asserting that an overlay of it alone says the same, and that its delta
/// writes what that overlay writes of it ([`delta_alike`]).
fn verdicts(st: &DocState, inputs: &ViewInputs, case: &str) -> Vec<bool> {
    (st.order().iter())
        .map(|&p| {
            let model = untouched(st, p, inputs);
            let emitted = emission_is_base(st, p, inputs).unwrap();
            assert_eq!(model, emitted, "{case}: {p:?} under {inputs:?}");
            delta_alike(st, p, inputs, case);
            model
        })
        .collect()
}

/// Page `page`'s delta ([`EmitScope::Delta`]) writes what the overlay of
/// it alone ([`EmitScope::Pages`]) writes of it without the frame, as the
/// realizer's deltas did before they skipped the frame: every object the
/// page reaches alike, at the same numbers, and nothing the page doesn't
/// reach that the overlay didn't write.
fn delta_alike(st: &DocState, page: PageId, inputs: &ViewInputs, case: &str) {
    let src = st.def(page).unwrap().base.src.clone();
    let overlay = Overlay {
        src,
        inputs,
        rotate: RotatePolicy::KeepBase,
        arts: None,
    };
    let part = |scope| {
        let e = emit::emit(st, scope, EmitBase::Overlay(&overlay), &[], &NoProgress).unwrap();
        Part::page(e, page).unwrap().reached(page)
    };
    let (whole, whole_unreached) = part(EmitScope::Pages([page].into()));
    let (delta, delta_unreached) = part(EmitScope::Delta(page));
    assert_eq!(delta, whole, "{case}: {page:?}'s delta under {inputs:?}");
    assert!(
        delta_unreached.is_subset(&whole_unreached),
        "{case}: {page:?}'s delta under {inputs:?} writes {:?}",
        delta_unreached
            .difference(&whole_unreached)
            .collect::<Vec<_>>()
    );
}

/// [`verdicts`] under every consumer's inputs: the screen's.
fn agree(st: &DocState, case: &str) -> Vec<bool> {
    let [screen, ..] = every_input().map(|inputs| verdicts(st, &inputs, case));
    screen
}

/// Three pages: [`every_kind`](crate::forms::fixture::every_kind)'s fields
/// on the first two, and other tools' comments and a link on the first.
fn theirs() -> Form {
    let mut f = crate::forms::fixture::every_kind_in(3);
    with_theirs(&mut f, 0, "theirs");
    f
}

/// Every case the predicate decides agrees with the emission, under every
/// consumer's inputs: a file as opened, its forms and other tools'
/// comments drawn as they are; pages that inherit their boxes and
/// resources, whose overlay writes them out under another parent; sheaf's
/// marks in the file, which the screen draws itself; a comment hidden or
/// deleted; a field's value, which no save writes yet; a header; recognized
/// text; a duplicate; a turned page, which the job turns; a watermark and a
/// background, which OCR's inputs don't draw, and the same found again in
/// the file, then moved and removed; a header of page numbers found again,
/// then its pages moved, and one replacing Acrobat's; a crop; fields
/// whose `/Parent` isn't the parent whose `/Kids` lists them;
/// a link to a page deleted.
#[test]
fn the_predicate_agrees_with_the_emission_in_every_case() {
    let all = |n: usize| vec![true; n];
    // As opened.
    let d = opened(theirs().bytes(), "form.pdf");
    assert_eq!(agree(d.state(), "opened"), all(3));
    // Only placed otherwise: what the pages inherit, written out.
    let d = open(&bytes(&file("A", 3, false)), None);
    assert_eq!(agree(d.state(), "inherited"), all(3));
    // sheaf's marks in the file, which the screen draws itself.
    let mut f = theirs();
    with_ours(&mut f);
    let d = opened(f.bytes(), "ours.pdf");
    assert_eq!(agree(d.state(), "ours"), [false, false, true]);
    assert_eq!(verdicts(d.state(), &ViewInputs::plugin(), "ours"), all(3));
    // A comment hidden, and one deleted.
    let mut d = opened(theirs().bytes(), "form.pdf");
    let p0 = d.page(0).unwrap();
    let comment = their_comment(d.state(), p0);
    let hidden = hiding(&[comment], ViewInputs::screen());
    assert_eq!(verdicts(d.state(), &hidden, "hidden"), [false, true, true]);
    let content = hiding(&[comment], ViewInputs::content_only());
    assert_eq!(verdicts(d.state(), &content, "hidden"), all(3));
    d.commit_ops(vec![fx::delete_annots(&[comment])], "Delete")
        .unwrap();
    assert_eq!(agree(d.state(), "deleted"), [false, true, true]);
    // A field's value, put as a fill would put it.
    let d = opened(theirs().bytes(), "form.pdf");
    let mut st = d.state().clone();
    let p0 = st.order()[0];
    let field = st.widgets_on(p0).next().unwrap().1.field;
    let node = st.field(field).unwrap().clone();
    let term = Terminal {
        value: Some(Value::Text("Bo".into())),
        ..Terminal::clone(node.term.as_ref().unwrap())
    };
    let filled = FieldNode {
        term: Some(Arc::new(term)),
        ..FieldNode::clone(&node)
    };
    let prim = Prim {
        key: Key::Field(field),
        before: Some(Val::Field(node)),
        after: Some(Val::Field(Arc::new(filled))),
    };
    st.apply(&prim).unwrap();
    assert_eq!(agree(&st, "field value"), all(3));
    // A header, recognized text, a duplicate and a turn.
    let mut d = opened(theirs().bytes(), "form.pdf");
    let [p0, p1, p2] = [0, 1, 2].map(|i| d.page(i).unwrap());
    let header = fx::set_header(fx::header("Draft"), Some(&[p0]));
    d.commit_ops(vec![header], "Header").unwrap();
    d.commit_ops(fx::ocr([(p1, layer("ocr"))]), "OCR").unwrap();
    d.commit_ops(vec![fx::duplicate(&[p2])], "Duplicate")
        .unwrap();
    d.commit_ops(vec![fx::rotate(&[p2], Turn::Quarter)], "Turn")
        .unwrap();
    assert_eq!(agree(d.state(), "edited"), [false, false, true, false]);
    let ocr = verdicts(d.state(), &ViewInputs::content_only(), "edited");
    assert_eq!(ocr, [true, true, true, false]);
    // A watermark and a background, which every consumer but OCR draws.
    let mut d = opened(theirs().bytes(), "form.pdf");
    let [p0, p1] = [0, 1].map(|i| d.page(i).unwrap());
    let set = |z, page| fx::set_watermark(fx::watermark("Draft"), z, Some(&[page]));
    let ops = vec![set(sheaf_edit::Z::Over, p0), set(sheaf_edit::Z::Under, p1)];
    d.commit_ops(ops, "Watermarks").unwrap();
    assert_eq!(agree(d.state(), "watermarked"), [false, false, true]);
    let ocr = verdicts(d.state(), &ViewInputs::content_only(), "watermarked");
    assert_eq!(ocr, all(3));
    // The same, saved and found again: drawn as the file draws them where
    // they're as they were, but under OCR's inputs, which set their blocks
    // aside; moved or removed, set aside where they were.
    let saved = crate::materialize::How::Rewritten.write(&d);
    let mut d = opened(saved, "found.pdf");
    assert_eq!(agree(d.state(), "found"), all(3));
    let ocr = verdicts(d.state(), &ViewInputs::content_only(), "found");
    assert_eq!(ocr, [false, false, true]);
    let [p0, p1] = [0, 1].map(|i| d.page(i).unwrap());
    let mut moved = fx::watermark("Draft");
    moved.position = sheaf_edit::GridPos::Top;
    let moved = fx::set_watermark(moved, sheaf_edit::Z::Over, Some(&[p0]));
    d.commit_ops(vec![moved], "Move").unwrap();
    assert_eq!(agree(d.state(), "moved"), [false, true, true]);
    let removed = fx::remove_watermark(Some(sheaf_edit::Z::Under), &[p1]);
    d.commit_ops(vec![removed], "Remove").unwrap();
    assert_eq!(agree(d.state(), "removed"), [false, false, true]);
    let ocr = verdicts(d.state(), &ViewInputs::content_only(), "removed");
    assert_eq!(ocr, [false, false, true]);
    // Acrobat's watermark on both pages, one under a watermark of the
    // document's, which replaces it where it's drawn, and so not under
    // OCR's inputs; the other's taken off by a filter, under any.
    let other: &[u8] = b"BT /F1 12 Tf 72 700 Td (other) Tj ET";
    let streams: [&[&[u8]]; 2] = [&[BODY, ACROBAT], &[other, ACROBAT]];
    let mut d = opened(acrobat_file(&streams), "acrobat.pdf");
    let [p0, p1] = [0, 1].map(|i| d.page(i).unwrap());
    let mine = fx::set_watermark(fx::watermark("Draft"), sheaf_edit::Z::Over, Some(&[p0]));
    d.commit_ops(vec![mine], "Watermark").unwrap();
    assert_eq!(agree(d.state(), "acrobat's replaced"), [false, true]);
    let ocr = verdicts(d.state(), &ViewInputs::content_only(), "acrobat's replaced");
    assert_eq!(ocr, all(2));
    let removed = fx::remove_watermark(Some(sheaf_edit::Z::Over), &[p1]);
    d.commit_ops(vec![removed], "Remove").unwrap();
    assert_eq!(agree(d.state(), "acrobat's removed"), [false, false]);
    let ocr = verdicts(d.state(), &ViewInputs::content_only(), "acrobat's removed");
    assert_eq!(ocr, [true, false]);
    // A header of page numbers, saved and found again: drawn as the file
    // draws it where it reads as it did, and anew where a move changed
    // where its pages are; Acrobat's header replaced under one.
    let mut d = open(&bytes(&file("A", 3, false)), None);
    let [p0, p1] = [0, 1].map(|i| d.page(i).unwrap());
    let numbers = fx::set_header(fx::header("Page <<1>> of <<N>>"), Some(&[p0, p1]));
    d.commit_ops(vec![numbers], "Header").unwrap();
    assert_eq!(agree(d.state(), "numbered"), [false, false, true]);
    let saved = crate::materialize::How::Rewritten.write(&d);
    let mut d = opened(saved, "numbered.pdf");
    assert_eq!(agree(d.state(), "numbered, found"), all(3));
    let ocr = verdicts(d.state(), &ViewInputs::content_only(), "numbered, found");
    assert_eq!(ocr, [false, false, true]);
    let [p0, p1] = [0, 1].map(|i| d.page(i).unwrap());
    d.commit_ops(vec![fx::move_pages(&[p1], Some(p0))], "Move")
        .unwrap();
    assert_eq!(agree(d.state(), "numbered, moved"), [false, false, true]);
    let streams: [&[&[u8]]; 2] = [&[BODY, ACROBAT_HEADER], &[other, ACROBAT_HEADER]];
    let mut d = opened(acrobat_file(&streams), "acrobat.pdf");
    let p0 = d.page(0).unwrap();
    let mine = fx::set_header(fx::header("Mine"), Some(&[p0]));
    d.commit_ops(vec![mine], "Header").unwrap();
    assert_eq!(agree(d.state(), "acrobat's header replaced"), [false, true]);
    let ocr = verdicts(d.state(), &ViewInputs::content_only(), "acrobat's header");
    assert_eq!(ocr, all(2));
    // A crop: the page's box isn't its base page's, which no job draws
    // (spike C failed), under every consumer's inputs.
    let to = crate::Box2 {
        x: 36.0,
        y: 36.0,
        w: 364.0,
        h: 464.0,
    };
    let mut d = open(&bytes(&file("A", 2, false)), None);
    let p1 = d.page(1).unwrap();
    d.commit_ops(vec![fx::crop_to(p1, to)], "Crop").unwrap();
    assert_eq!(agree(d.state(), "crop"), [true, false]);
    let ocr = verdicts(d.state(), &ViewInputs::content_only(), "crop");
    assert_eq!(ocr, [true, false]);
    let mut d = opened(theirs().bytes(), "form.pdf");
    let p1 = d.page(1).unwrap();
    d.commit_ops(vec![fx::crop_to(p1, to)], "Crop").unwrap();
    assert_eq!(agree(d.state(), "crop of widgets"), [true, false, true]);
    // A form inserted from a file whose defaults aren't the document's,
    // which its overlay pushes down onto its roots.
    let mut d = opened(theirs().bytes(), "form.pdf");
    let mut other = Form::new(1);
    other.text("plain", 0, crate::forms::fixture::row(0));
    let acroform = other.acroform;
    other.dict(acroform).set("Q", 1);
    let other = add(&d, &other.bytes(), "other.pdf");
    insert(&mut d, &other, 0);
    assert_eq!(agree(d.state(), "inserted form"), [true, true, true, false]);
    // Fields a parent's `/Kids` lists, one with no `/Parent` and one whose
    // `/Parent` is another: the model hangs both from the parent, as the
    // overlay writes them, so neither page is as its file has it where
    // widgets are drawn.
    let mut f = Form::new(3);
    let row = crate::forms::fixture::row;
    let [a, b] = [(0, "a"), (1, "b")].map(|(page, name)| f.text(name, page, row(0)));
    let elsewhere = f.text("c", 2, row(0));
    f.group("g", &[a, b]);
    f.dict(a).remove(b"Parent");
    f.dict(b).set("Parent", elsewhere);
    let d = opened(f.bytes(), "form.pdf");
    assert_eq!(agree(d.state(), "orphaned"), [false, false, true]);
    let ocr = verdicts(d.state(), &ViewInputs::content_only(), "orphaned");
    assert_eq!(ocr, all(3));
    // A link to the last page, which goes: the overlay leaves it out.
    let mut f = theirs();
    let last = f.pages[2];
    let link = f.pdf.add_object(dictionary! {
        "Type" => "Annot", "Subtype" => "Link",
        "Rect" => vec![200.into(), 200.into(), 300.into(), 220.into()],
        "Dest" => vec![last.into(), Object::Name(b"Fit".to_vec())],
        "Border" => vec![0.into(), 0.into(), 2.into()],
    });
    let mut annots = f.annots(0);
    annots.push(link);
    f.set_annots(0, &annots);
    let mut d = opened(f.bytes(), "form.pdf");
    assert_eq!(agree(d.state(), "link"), all(3));
    let [p0, p2] = [0, 2].map(|i| d.page(i).unwrap());
    let key = |d: &crate::Document, inputs| render_key(d.state(), p0, inputs).unwrap();
    let (screen, content) = (ViewInputs::screen(), ViewInputs::content_only());
    let was = [key(&d, &screen), key(&d, &content)];
    d.commit_ops(vec![fx::delete(&[p2])], "Delete").unwrap();
    assert_eq!(agree(d.state(), "dead link"), [false, true]);
    // The link isn't drawn now: the page's key says so where it's drawn.
    assert_ne!(key(&d, &screen), was[0]);
    assert_eq!(key(&d, &content), was[1]);
}

/// An untouched page is viewed in its source's base engine at its base
/// index, turned by its job; the edited pages of a group share its
/// engine, each at its place among them in base order, a duplicate after
/// its original; and a group whose page draws otherwise is another engine,
/// while the same state and inputs find the same one.
#[test]
fn views_send_untouched_pages_to_the_base_and_edited_ones_to_their_group() {
    let mut d = opened(theirs().bytes(), "form.pdf");
    let [p0, p1, p2] = [0, 1, 2].map(|i| d.page(i).unwrap());
    let blob = d.primary().id();
    let header = |text| fx::set_header(fx::header(text), Some(&[p2, p0]));
    d.commit_ops(vec![header("Draft")], "Header").unwrap();
    d.commit_ops(vec![fx::rotate(&[p1], Turn::Quarter)], "Turn")
        .unwrap();
    d.commit_ops(vec![fx::duplicate(&[p0])], "Duplicate")
        .unwrap();
    let copy = d.page(1).unwrap();
    let realizer = Realizer::new();
    let screen = ViewInputs::screen();
    let st = d.state();
    let view = |p| realizer.view(st, p, &screen).unwrap();
    let turned = view(p1);
    assert_eq!(turned.engine, EngineKey::Base(blob));
    assert_eq!(turned.index, 1);
    assert_eq!(turned.job.rotation, Rotation::R90);
    assert!(turned.annots && turned.forms);
    let group = GroupKey {
        source: blob,
        chunk: 0,
    };
    let [first, dup, last] = [p0, copy, p2].map(view);
    let EngineKey::Group(g, digest) = first.engine else {
        panic!("{first:?} isn't its group's");
    };
    assert_eq!(g, group);
    for (v, index) in [(first, 0), (dup, 1), (last, 2)] {
        assert_eq!(v.engine, first.engine);
        assert_eq!(v.index, index);
    }
    assert_eq!(view(p0), first);
    let ocr = realizer.view(st, p0, &ViewInputs::content_only()).unwrap();
    assert_eq!(ocr.engine, EngineKey::Base(blob));
    assert!(!ocr.annots && !ocr.forms);
    d.commit_ops(vec![header("Final")], "Header").unwrap();
    let again = realizer.view(d.state(), p0, &screen).unwrap();
    assert!(matches!(again.engine, EngineKey::Group(g, other) if g == group && other != digest));
}

/// A group realized over a sound base is the base's bytes and a section
/// after them, which numbers what isn't the source's own past its `/Size`;
/// opening an engine of a group whose pages changed since is refused.
#[test]
fn a_group_opens_as_the_base_and_its_section() {
    let mut d = opened(theirs().bytes(), "form.pdf");
    let p0 = d.page(0).unwrap();
    let header = |text| fx::set_header(fx::header(text), Some(&[p0]));
    d.commit_ops(vec![header("Draft")], "Header").unwrap();
    let realizer = Realizer::new();
    let screen = ViewInputs::screen();
    let view = realizer.view(d.state(), p0, &screen).unwrap();
    let opened = realizer.open(d.state(), view.engine, &screen).unwrap();
    let src = d.primary();
    assert_eq!(opened.segments.len(), 2);
    assert!(Arc::ptr_eq(&opened.segments[0], src.bytes()));
    let whole: Arc<[u8]> = opened.segments.concat().into();
    let pdf = lopdf::LazyPdf::open(whole, None, &lopdf::ReadLimits::DEFAULT).unwrap();
    assert!(pdf.xref().size > src.xref().size);
    assert_eq!(pdf.xref().sections, src.xref().sections + 1);
    d.commit_ops(vec![header("Final")], "Header").unwrap();
    assert!(realizer.open(d.state(), view.engine, &screen).is_err());
}

/// A realizer is shared by the UI thread, which asks for views, and the
/// engine pool's worker, which realizes them.
#[test]
fn a_realizer_is_send_and_sync() {
    fn shared<T: Send + Sync>() {}
    shared::<Realizer>();
    shared::<Realization>();
}

/// What `realizer` keeps of each delta: its page and key, the part and its
/// weight, oldest first.
fn kept(realizer: &Realizer) -> Vec<((PageId, RenderKey), Arc<Part>, usize)> {
    let deltas = realizer.deltas.lock().unwrap();
    let kept = (deltas.order.iter()).map(|at| {
        let (part, weight) = &deltas.by[at];
        (*at, part.clone(), *weight)
    });
    let kept: Vec<_> = kept.collect();
    assert_eq!(
        kept.len(),
        deltas.by.len(),
        "each delta kept is in the order once"
    );
    let weighed: usize = kept.iter().map(|(_, _, weight)| weight).sum();
    assert_eq!(deltas.bytes, weighed, "the bytes are what the kept weigh");
    assert!(
        deltas.bytes <= deltas.cap,
        "{} past {}",
        deltas.bytes,
        deltas.cap
    );
    kept
}

/// A group opened again takes each page's delta as it was kept, emitting
/// none; a realizer whose cap holds fewer than the group's deltas keeps
/// the newest that fit, letting the oldest go first, and its bytes are
/// what those weigh.
#[test]
fn deltas_are_kept_by_page_and_key_under_their_cap() {
    let mut d = opened(theirs().bytes(), "form.pdf");
    d.commit_ops(vec![fx::set_header(fx::header("Draft"), None)], "Header")
        .unwrap();
    let (st, screen) = (d.state(), ViewInputs::screen());
    let order: Vec<PageId> = st.order().iter().copied().collect();
    let realizer = Realizer::new();
    let engine = realizer.view(st, order[0], &screen).unwrap().engine;
    realizer.open(st, engine, &screen).unwrap();
    let first = kept(&realizer);
    let pages: Vec<PageId> = first.iter().map(|((page, _), _, _)| *page).collect();
    assert_eq!(pages, order);
    realizer.open(st, engine, &screen).unwrap();
    let again = kept(&realizer);
    assert_eq!(again.len(), first.len());
    for ((at, part, _), (was, kept, _)) in again.iter().zip(&first) {
        assert_eq!(at, was);
        assert!(Arc::ptr_eq(part, kept), "{at:?} emitted again");
    }
    // Room for the last two alone.
    let weights: Vec<usize> = first.iter().map(|(_, _, weight)| *weight).collect();
    let capped = Realizer::capped(weights[1..].iter().sum());
    capped.open(st, engine, &screen).unwrap();
    let left: Vec<PageId> = (kept(&capped).iter())
        .map(|((page, _), _, _)| *page)
        .collect();
    assert_eq!(left, order[1..]);
}

/// An image watermark on every page of a group is made once a realizer:
/// each page's delta, emitted alone, draws the image the first made, and
/// so does a group made again for another edit. Another realizer makes it
/// once of its own.
#[test]
fn a_watermark_s_image_is_made_once_a_realizer() {
    use crate::materialize::emit::content::ARTS_MADE;
    use crate::materialize::watermark_tests::{image_watermark, png};
    let mut d = open(&bytes(&file("A", 3, false)), None);
    let (_png, spec) = image_watermark(&d, &png());
    let set = fx::set_watermark(spec, sheaf_edit::Z::Over, None);
    d.commit_ops(vec![set], "Watermark").unwrap();
    let screen = ViewInputs::screen();
    let first = d.page(0).unwrap();
    let realizer = Realizer::new();
    let made = ARTS_MADE.get();
    let opened = |realizer: &Realizer, d: &crate::Document| {
        let view = realizer.view(d.state(), first, &screen).unwrap();
        realizer.open(d.state(), view.engine, &screen).unwrap();
    };
    opened(&realizer, &d);
    assert_eq!(kept(&realizer).len(), 3);
    assert_eq!(ARTS_MADE.get() - made, 1);
    let background = fx::set_watermark(fx::watermark("Copy"), sheaf_edit::Z::Under, None);
    d.commit_ops(vec![background], "Background").unwrap();
    opened(&realizer, &d);
    assert_eq!(kept(&realizer).len(), 6);
    assert_eq!(ARTS_MADE.get() - made, 1);
    opened(&Realizer::new(), &d);
    assert_eq!(ARTS_MADE.get() - made, 2);
}

/// A view asked for again and again under inputs hiding comments digests
/// them once, while the same sets are asked of; inputs made again are
/// digested again. A comment hidden on one group's page leaves another
/// group's engine as it was.
#[test]
fn inputs_are_digested_once_and_a_hide_changes_only_its_group() {
    let mut f = crate::forms::fixture::every_kind_in(GROUP as usize + 2);
    with_theirs(&mut f, 0, "theirs");
    let mut d = opened(f.bytes(), "form.pdf");
    d.commit_ops(vec![fx::set_header(fx::header("Draft"), None)], "Header")
        .unwrap();
    let st = d.state();
    let (first, last) = (st.order()[0], *st.order().last().unwrap());
    let comment = their_comment(st, first);
    let realizer = Realizer::new();
    let screen = ViewInputs::screen();
    let [was_first, was_last] = [first, last].map(|p| realizer.view(st, p, &screen).unwrap());
    let hidden = hiding(&[comment], ViewInputs::screen());
    let digests = DIGESTS.get();
    for _ in 0..20 {
        for page in [first, last] {
            realizer.view(st, page, &hidden).unwrap();
        }
    }
    assert_eq!(DIGESTS.get() - digests, 1);
    realizer.view(st, last, &hidden.clone()).unwrap();
    assert_eq!(DIGESTS.get() - digests, 1, "a clone holds the same sets");
    realizer
        .view(st, last, &hiding(&[comment], screen))
        .unwrap();
    assert_eq!(DIGESTS.get() - digests, 2);
    let [now_first, now_last] = [first, last].map(|p| realizer.view(st, p, &hidden).unwrap());
    assert_ne!(now_first.engine, was_first.engine);
    assert_eq!(now_last.engine, was_last.engine);
}

/// Once PDFium has refused a sound base, each of its groups is a file of
/// its own, and another engine than its section over the base was, though
/// its pages are the same: an engine that failed to open stays failed
/// under its key. (Its untouched pages joining their groups is
/// `a_base_pdfium_refuses_draws_every_page_from_its_realization`'s.)
#[test]
fn a_refused_base_s_groups_are_files_of_their_own() {
    let mut d = opened(theirs().bytes(), "form.pdf");
    d.commit_ops(vec![fx::set_header(fx::header("Draft"), None)], "Header")
        .unwrap();
    let (st, screen) = (d.state(), ViewInputs::screen());
    let realizer = Realizer::new();
    let first = st.order()[0];
    let section = realizer.view(st, first, &screen).unwrap();
    let r = realizer.open(st, section.engine, &screen).unwrap();
    assert_eq!(r.segments.len(), 2);
    assert!(d.primary().base_sound());
    d.primary().refuse_base();
    let alone = realizer.view(st, first, &screen).unwrap();
    assert_ne!(alone.engine, section.engine);
    assert_eq!(alone.index, section.index);
    let r = realizer.open(st, alone.engine, &screen).unwrap();
    assert_eq!((r.segments.len(), r.password), (1, None));
}
