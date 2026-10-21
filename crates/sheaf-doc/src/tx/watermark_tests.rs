//! Watermarks and backgrounds as `SetWatermark` and `RemoveWatermark` set
//! them (M10.5 slice 16): one generator for each z, which a later one of
//! that z replaces and keeps the id of; taken off pages by narrowing its
//! scope, and gone when it's on none; what it draws resolved and checked
//! before it's lowered; and the lines say what was done.

use std::collections::BTreeSet;

use in_progress::NoProgress;
use sheaf_edit::{BlobSel, GridPos, Op, PageSel, ScopeSel, WatermarkContent as Said, Z};

use super::fixture as fx;
use super::tests::{commit, doc_of};
use super::*;
use crate::content::PaginationKind;
use crate::model::{
    Document, GenSpec, Generator, ImageRef, PageForm, Scope, WatermarkContent, WatermarkGen,
};
use crate::prim::Key;
use crate::store::BlobKind;

/// The watermark at `z`, and its generator's id.
fn drawn(d: &Document, z: Z) -> Option<(GenId, WatermarkGen)> {
    let g = d.state().watermark(z)?;
    Some((g.id, WatermarkGen::clone(g.spec.watermark().unwrap())))
}

/// The pages `scope` names, or None for every page.
fn named(scope: &Scope) -> Option<Vec<PageId>> {
    match scope {
        Scope::All => None,
        Scope::Pages(pages) => Some(pages.iter().copied().collect()),
    }
}

/// A watermark of `content`, in the middle.
fn of(content: Said) -> sheaf_edit::WatermarkSpec {
    sheaf_edit::WatermarkSpec {
        content,
        position: GridPos::Center,
    }
}

/// An image watermark of `image`.
fn image(image: BlobSel, scale: f32) -> Said {
    Said::Image {
        image: Some(image),
        scale,
        opacity: 0.5,
    }
}

/// A watermark of page `page` of `pdf`.
fn page_of(pdf: BlobSel, page: u32) -> Said {
    Said::Page {
        pdf: Some(pdf),
        page,
        scale: 1.0,
        opacity: 1.0,
    }
}

/// A PNG's bytes, 4 by 2.
fn png() -> Vec<u8> {
    let rgb = image::RgbImage::from_pixel(4, 2, image::Rgb([200, 30, 30]));
    sheaf_image::encode::png(&rgb, 72).unwrap()
}

/// Each z has one generator: set again, it's replaced and keeps its id;
/// the other z's is one of its own. Each is one prim, and replaces the
/// artifacts of its own kind.
#[test]
fn a_watermark_is_one_generator_for_its_z() {
    let mut d = doc_of(3, &[]);
    let [_, p1, p2] = [0, 1, 2].map(|i| d.page(i).unwrap());
    let set =
        |text: &str, z, pages: Option<&[PageId]>| fx::set_watermark(fx::watermark(text), z, pages);
    let c = commit(&mut d, Actor::User, vec![set("DRAFT", Z::Over, None)]).unwrap();
    let (id, wm) = drawn(&d, Z::Over).unwrap();
    assert_eq!(*c.effects.keys, BTreeSet::from([Key::Generator(id)]));
    assert_eq!(c.lines[0].text, "Add a watermark to all pages");
    assert_eq!(wm.scope, Scope::All);
    assert_eq!(wm.replaces, BTreeSet::from([PaginationKind::Watermark]));
    assert!(matches!(&wm.content, WatermarkContent::Text { text, .. } if &**text == "DRAFT"));
    let c = commit(&mut d, Actor::User, vec![set("FINAL", Z::Over, None)]).unwrap();
    assert_eq!(c.lines[0].text, "Change the watermark on all pages");
    let (again, wm) = drawn(&d, Z::Over).unwrap();
    assert_eq!(again, id, "the one it replaced's id");
    assert!(matches!(&wm.content, WatermarkContent::Text { text, .. } if &**text == "FINAL"));
    let c = commit(
        &mut d,
        Actor::User,
        vec![set("Copy", Z::Under, Some(&[p2, p1]))],
    )
    .unwrap();
    assert_eq!(c.lines[0].text, "Add a background to pages 2, 3");
    let (behind, wm) = drawn(&d, Z::Under).unwrap();
    assert_ne!(behind, id);
    assert_eq!(named(&wm.scope), Some(vec![p1, p2]));
    assert_eq!(wm.replaces, BTreeSet::from([PaginationKind::Background]));
    assert_eq!(d.state().generators().len(), 2);
    assert_eq!(drawn(&d, Z::Over).unwrap().0, id, "the other z's stays");
    // Said again as it is, it changes nothing.
    let n = d.version_count();
    commit(&mut d, Actor::User, vec![set("FINAL", Z::Over, None)]).unwrap();
    assert_eq!(d.version_count(), n);
    d.undo().unwrap();
    assert!(drawn(&d, Z::Under).is_none());
    let generator = d.state().generators()[&id].clone();
    assert!(generator.covers(p1));
    assert_eq!((generator.z(), &*generator.scope()), (Z::Over, &Scope::All));
}

/// Taken off pages, a watermark is narrowed to the pages it's left on
/// (one on every page to the others there are now), and gone when that's
/// none; with no z, both are. Off pages it isn't on, nothing changes.
#[test]
fn a_removed_watermark_is_narrowed_or_gone() {
    let mut d = doc_of(4, &[]);
    let [p0, p1, p2, p3] = [0, 1, 2, 3].map(|i| d.page(i).unwrap());
    let ops = vec![
        fx::set_watermark(fx::watermark("A"), Z::Over, None),
        fx::set_watermark(fx::watermark("B"), Z::Under, Some(&[p1, p2])),
    ];
    commit(&mut d, Actor::User, ops).unwrap();
    let scope = |d: &Document, z| drawn(d, z).map(|(_, wm)| named(&wm.scope));
    let c = commit(
        &mut d,
        Actor::User,
        vec![fx::remove_watermark(Some(Z::Over), &[p0])],
    )
    .unwrap();
    assert_eq!(c.lines[0].text, "Remove the watermark from page 1");
    assert_eq!(scope(&d, Z::Over), Some(Some(vec![p1, p2, p3])));
    assert_eq!(scope(&d, Z::Under), Some(Some(vec![p1, p2])));
    let c = commit(&mut d, Actor::User, vec![fx::remove_watermark(None, &[p1])]).unwrap();
    assert_eq!(
        c.lines[0].text,
        "Remove the watermark and background from page 2"
    );
    assert_eq!(scope(&d, Z::Over), Some(Some(vec![p2, p3])));
    assert_eq!(scope(&d, Z::Under), Some(Some(vec![p2])));
    commit(
        &mut d,
        Actor::User,
        vec![fx::remove_watermark(Some(Z::Under), &[p2])],
    )
    .unwrap();
    assert_eq!(scope(&d, Z::Under), None);
    // Off a page neither is on: nothing.
    let n = d.version_count();
    commit(&mut d, Actor::User, vec![fx::remove_watermark(None, &[p0])]).unwrap();
    assert_eq!(d.version_count(), n);
    let all = Op::RemoveWatermark {
        z: None,
        pages: PageSel::All,
        include_authored: false,
    };
    let c = commit(&mut d, Actor::User, vec![all]).unwrap();
    assert_eq!(
        c.lines[0].text,
        "Remove the watermark and background from pages 1–4"
    );
    assert!(d.state().generators().is_empty());
    // Narrowing one on every page reads the order: a page inserted since
    // isn't one it's left on.
    commit(
        &mut d,
        Actor::User,
        vec![fx::set_watermark(fx::watermark("A"), Z::Over, None)],
    )
    .unwrap();
    let mut tx = d.tx(Actor::User);
    tx.apply(fx::remove_watermark(Some(Z::Over), &[p0]), &NoProgress)
        .unwrap();
    assert!(tx.into_parts().seen.contains_key(&Key::Order));
}

/// A page that's deleted leaves a watermark's pages, and the watermark is
/// gone once it's on none; a page's copy joins them when the page is
/// among them. One on every page takes neither.
#[test]
fn a_watermark_s_pages_follow_deletes_and_duplicates() {
    let mut d = doc_of(3, &[]);
    let [p0, p1, p2] = [0, 1, 2].map(|i| d.page(i).unwrap());
    let ops = vec![
        fx::set_watermark(fx::watermark("A"), Z::Over, None),
        fx::set_watermark(fx::watermark("B"), Z::Under, Some(&[p0, p1])),
    ];
    commit(&mut d, Actor::User, ops).unwrap();
    let c = commit(&mut d, Actor::User, vec![fx::delete(&[p0])]).unwrap();
    let dropped = Cascade {
        rule: Rule::PageGenerators,
        objects: 1,
    };
    assert_eq!(c.cascades, [dropped]);
    let under = |d: &Document| drawn(d, Z::Under).map(|(_, wm)| named(&wm.scope).unwrap());
    assert_eq!(under(&d), Some(vec![p1]));
    let c = commit(&mut d, Actor::User, vec![fx::duplicate(&[p1, p2])]).unwrap();
    let copies: Vec<PageId> = c.created.pages().collect();
    let mut want = vec![p1, copies[0]];
    want.sort();
    assert_eq!(under(&d), Some(want));
    assert_eq!(drawn(&d, Z::Over).unwrap().1.scope, Scope::All);
    assert_eq!(d.state().check_invariants(), Ok(()));
    commit(&mut d, Actor::User, vec![fx::delete(&[p1, copies[0]])]).unwrap();
    assert_eq!(under(&d), None);
    assert!(drawn(&d, Z::Over).is_some());
}

/// What isn't drawable fails the op, saying what's wrong with it, and
/// nothing is lowered.
#[test]
fn an_undrawable_watermark_is_invalid() {
    let mut d = doc_of(2, &[]);
    let text = |text: &str, size: f32, opacity: f32, angle: f32| Said::Text {
        text: text.into(),
        font: Default::default(),
        size,
        color: [0, 0, 0],
        opacity,
        angle,
    };
    let png = d
        .store()
        .intern(png().into(), BlobKind::image(&png()).unwrap())
        .unwrap();
    let cases = [
        (text(" \n", 72.0, 0.3, 45.0), "the text is empty"),
        (text("A", 0.0, 0.3, 45.0), "the size is 0, not above 0"),
        (text("A", -1.0, 0.3, 45.0), "the size is -1, not above 0"),
        (
            text("A", f32::NAN, 0.3, 45.0),
            "the size is NaN, not above 0",
        ),
        (
            text("A", f32::INFINITY, 0.3, 45.0),
            "the size is inf, not above 0",
        ),
        (text("A", 72.0, 1.5, 45.0), "the opacity is 1.5, not 0 to 1"),
        (
            text("A", 72.0, -0.1, 45.0),
            "the opacity is -0.1, not 0 to 1",
        ),
        (
            text("A", 72.0, f32::NAN, 45.0),
            "the opacity is NaN, not 0 to 1",
        ),
        (
            text("A", 72.0, 0.3, f32::NAN),
            "the angle is NaN, not a number",
        ),
        (
            text("A", 72.0, 0.3, f32::NEG_INFINITY),
            "the angle is -inf, not a number",
        ),
        (
            image(BlobSel::Id(png.id()), 0.0),
            "the scale is 0, not above 0 up to 2",
        ),
        (
            image(BlobSel::Id(png.id()), 2.5),
            "the scale is 2.5, not above 0 up to 2",
        ),
        (
            image(BlobSel::Id(d.primary().id()), 1.0),
            "the watermark's image isn't an image file",
        ),
        (
            page_of(BlobSel::Id(png.id()), 1),
            "the watermark's page isn't a PDF's",
        ),
        (
            page_of(BlobSel::Id(d.primary().id()), 0),
            "the watermark's page is 1-based",
        ),
    ];
    let ids = d.ids_next();
    for (content, why) in cases {
        let op = fx::set_watermark(of(content), Z::Over, None);
        let e = commit(&mut d, Actor::User, vec![op]).unwrap_err();
        assert!(matches!(e, EditError::Invalid { .. }), "{e}");
        assert_eq!(e.to_string(), format!("set-watermark: {why}"));
    }
    let past = page_of(BlobSel::Id(d.primary().id()), 3);
    let e = commit(
        &mut d,
        Actor::User,
        vec![fx::set_watermark(of(past), Z::Over, None)],
    );
    assert_eq!(
        e.unwrap_err(),
        EditError::not_found("set-watermark", "page 3 of the file's 2")
    );
    let nowhere = Op::SetWatermark {
        spec: fx::watermark("A"),
        z: Z::Under,
        scope: ScopeSel::Pages(fx::ids(&[])),
    };
    let e = commit(&mut d, Actor::User, vec![nowhere]).unwrap_err();
    assert_eq!(e.to_string(), "set-watermark: the watermark is on no page");
    // A stamp's page, granted, isn't a file.
    let mut tx = d.tx(Actor::User);
    let stamp = tx.grant(Grant::Stamp {
        stem: "s".into(),
        form: crate::model::FormRef::Blob(png.clone().into()),
    });
    let op = fx::set_watermark(of(image(stamp, 1.0)), Z::Over, None);
    let e = tx.apply(op, &NoProgress).unwrap_err();
    assert_eq!(e.to_string(), "set-watermark: a stamp's page isn't a file");
    assert_eq!(d.ids_next(), ids, "nothing was lowered");
    assert!(d.state().generators().is_empty());
}

/// An image watermark holds its image file, and a page watermark the PDF
/// with the page's index; a plugin names them by its grants.
#[test]
fn a_watermark_holds_its_image_or_page() {
    let mut d = doc_of(2, &[]);
    let bytes = png();
    let png = d
        .store()
        .intern(bytes.clone().into(), BlobKind::image(&bytes).unwrap())
        .unwrap();
    assert!(matches!(
        png.kind(),
        BlobKind::Image(image::ImageFormat::Png)
    ));
    assert!(BlobKind::image(b"%PDF-1.7").is_none());
    let mut tx = d.tx(Actor::User);
    let seal = tx.grant_named("seal", Grant::Blob(png.clone().into()));
    let pdf = tx.grant(Grant::Blob(d.primary().blob().clone().into()));
    let ops = vec![
        fx::set_watermark(of(image(seal, 0.5)), Z::Over, None),
        fx::set_watermark(of(page_of(pdf, 2)), Z::Under, None),
    ];
    tx.apply_batch(ops, &NoProgress).unwrap();
    d.commit(tx, CommitOpts::default()).unwrap();
    let (_, over) = drawn(&d, Z::Over).unwrap();
    let WatermarkContent::Image {
        image: ImageRef::Blob(held),
        scale,
        ..
    } = &over.content
    else {
        panic!("{:?}", over.content);
    };
    assert_eq!((held.id(), *scale), (png.id(), 0.5));
    let (_, under) = drawn(&d, Z::Under).unwrap();
    let WatermarkContent::Page {
        page: PageForm::Blob { pdf, index },
        ..
    } = &under.content
    else {
        panic!("{:?}", under.content);
    };
    assert_eq!((pdf.id(), *index), (d.primary().id(), 1));
}

/// A file is an image or a PDF by its bytes: one the store holds as an
/// attachment already, chosen again, is the attachment's blob, and draws
/// as a watermark or a background all the same.
#[test]
fn a_file_held_as_an_attachment_draws_as_a_watermark() {
    let mut d = doc_of(2, &[]);
    let bytes = png();
    let png = d
        .store()
        .intern(bytes.clone().into(), BlobKind::Attachment)
        .unwrap();
    let pdf_bytes = crate::generated::blank_pages(300.0, 200.0, 3).unwrap();
    let pdf = (d.store())
        .intern(pdf_bytes.clone().into(), BlobKind::Attachment)
        .unwrap();
    let image_again = (d.store())
        .intern(bytes.clone().into(), BlobKind::image(&bytes).unwrap())
        .unwrap();
    let pdf_again = (d.store())
        .intern(pdf_bytes.into(), BlobKind::stamp())
        .unwrap();
    assert!(matches!(image_again.kind(), BlobKind::Attachment));
    assert!(matches!(pdf_again.kind(), BlobKind::Attachment));
    let mut tx = d.tx(Actor::User);
    let image_sel = tx.grant(Grant::Blob(image_again.into()));
    let pdf_sel = tx.grant(Grant::Blob(pdf_again.into()));
    let ops = vec![
        fx::set_watermark(of(image(image_sel, 0.5)), Z::Over, None),
        fx::set_watermark(of(page_of(pdf_sel, 3)), Z::Under, None),
    ];
    tx.apply_batch(ops, &NoProgress).unwrap();
    d.commit(tx, CommitOpts::default()).unwrap();
    let (_, over) = drawn(&d, Z::Over).unwrap();
    let WatermarkContent::Image {
        image: ImageRef::Blob(held),
        ..
    } = &over.content
    else {
        panic!("{:?}", over.content);
    };
    assert_eq!(held.id(), png.id());
    let (_, under) = drawn(&d, Z::Under).unwrap();
    let WatermarkContent::Page {
        page: PageForm::Blob { pdf: held, index },
        ..
    } = &under.content
    else {
        panic!("{:?}", under.content);
    };
    assert_eq!((held.id(), *index), (pdf.id(), 2));
}

/// An image or a page op that names no file keeps the one drawn at its z,
/// moved or scaled; with none drawn there, it's invalid.
#[test]
fn a_watermark_naming_no_file_keeps_its_image_or_page() {
    let mut d = doc_of(2, &[]);
    let kept = |z, image: bool| {
        let content = match image {
            true => Said::Image {
                image: None,
                scale: 0.75,
                opacity: 0.25,
            },
            false => Said::Page {
                pdf: None,
                page: 1,
                scale: 0.75,
                opacity: 0.25,
            },
        };
        let spec = sheaf_edit::WatermarkSpec {
            content,
            position: GridPos::Top,
        };
        fx::set_watermark(spec, z, None)
    };
    for (z, image, why) in [
        (Z::Over, true, "no image is drawn at its z to keep"),
        (Z::Under, false, "no page is drawn at its z to keep"),
    ] {
        let e = commit(&mut d, Actor::User, vec![kept(z, image)]).unwrap_err();
        assert_eq!(e.to_string(), format!("set-watermark: {why}"));
    }
    let bytes = png();
    let png = (d.store())
        .intern(bytes.clone().into(), BlobKind::image(&bytes).unwrap())
        .unwrap();
    let pdf = BlobSel::Id(d.primary().id());
    let ops = vec![
        fx::set_watermark(of(image(BlobSel::Id(png.id()), 0.5)), Z::Over, None),
        fx::set_watermark(of(page_of(pdf, 2)), Z::Under, None),
    ];
    commit(&mut d, Actor::User, ops).unwrap();
    let (was_over, was_under) = (drawn(&d, Z::Over).unwrap(), drawn(&d, Z::Under).unwrap());
    commit(
        &mut d,
        Actor::User,
        vec![kept(Z::Over, true), kept(Z::Under, false)],
    )
    .unwrap();
    for (z, was) in [(Z::Over, was_over), (Z::Under, was_under)] {
        let (id, now) = drawn(&d, z).unwrap();
        assert_eq!(id, was.0);
        assert_eq!(now.position, GridPos::Top);
        let file = |c: &WatermarkContent| match c {
            WatermarkContent::Image { image, scale, .. } => (Some(image.clone()), None, *scale),
            WatermarkContent::Page { page, scale, .. } => (None, Some(page.clone()), *scale),
            WatermarkContent::Text { .. } => panic!("{c:?}"),
        };
        let ((a, b, _), (c, e, scale)) = (file(&was.1.content), file(&now.content));
        assert_eq!((a, b, 0.75), (c, e, scale), "{z:?}");
    }
}

/// Text the font can't draw is kept as it was said, and the commit says
/// it shows as `?`; text it can draw says nothing.
#[test]
fn unencodable_text_is_kept_with_a_notice() {
    let mut d = doc_of(1, &[]);
    let set = |text: &str| fx::set_watermark(fx::watermark(text), Z::Over, None);
    let c = commit(&mut d, Actor::User, vec![set("Café")]).unwrap();
    assert!(c.notices.is_empty(), "{:?}", c.notices);
    let c = commit(&mut d, Actor::User, vec![set("機密 Café")]).unwrap();
    assert_eq!(c.notices, [Notice::Unencodable]);
    let (_, wm) = drawn(&d, Z::Over).unwrap();
    assert!(matches!(&wm.content, WatermarkContent::Text { text, .. } if &**text == "機密 Café"));
}

/// A run of watermark ops is one line: the last's when they're of one z,
/// both named when they aren't.
#[test]
fn a_run_of_watermark_ops_is_one_line() {
    let mut d = doc_of(4, &[]);
    let [p0, p1, ..] = [0, 1, 2, 3].map(|i| d.page(i).unwrap());
    let set = |text: &str, z| fx::set_watermark(fx::watermark(text), z, None);
    let c = commit(
        &mut d,
        Actor::User,
        vec![set("A", Z::Over), set("B", Z::Over)],
    )
    .unwrap();
    assert_eq!(c.lines[0].text, "Change the watermark on all pages");
    let c = commit(
        &mut d,
        Actor::User,
        vec![set("C", Z::Over), set("D", Z::Under)],
    )
    .unwrap();
    assert_eq!(c.lines[0].text, "Change the watermark and background");
    let ops = vec![
        fx::remove_watermark(Some(Z::Under), &[p0]),
        fx::remove_watermark(Some(Z::Under), &[p1]),
    ];
    let c = commit(&mut d, Actor::User, ops).unwrap();
    assert_eq!(c.lines[0].text, "Remove the background from 2 pages");
    assert!(c.lines[0].removes);
    let ops = vec![
        fx::remove_watermark(Some(Z::Under), &[p0]),
        fx::remove_watermark(Some(Z::Over), &[p1]),
    ];
    let c = commit(&mut d, Actor::User, ops).unwrap();
    assert_eq!(
        c.lines[0].text,
        "Remove the watermark and background from 2 pages"
    );
}

/// A watermark's scope names pages there are, never none, and a document
/// has one of each z.
#[test]
fn watermarks_keep_the_invariants() {
    let mut d = doc_of(2, &[]);
    let p0 = d.page(0).unwrap();
    commit(
        &mut d,
        Actor::User,
        vec![fx::set_watermark(fx::watermark("A"), Z::Over, Some(&[p0]))],
    )
    .unwrap();
    let st = d.state().clone();
    let (id, wm) = drawn(&d, Z::Over).unwrap();
    let with = |id: u64, scope: Scope| {
        let wm = WatermarkGen {
            scope,
            ..wm.clone()
        };
        let g = Generator {
            id: GenId(id),
            spec: GenSpec::Watermark(Arc::new(wm)),
        };
        (
            Key::Generator(GenId(id)),
            Some(crate::prim::Val::Generator(Arc::new(g))),
        )
    };
    let pages = |ids: &[PageId]| Scope::Pages(ids.iter().copied().collect());
    for ((key, val), says) in [
        (
            with(id.0, pages(&[p0, PageId(99)])),
            "is on PageId(99), which isn't a page",
        ),
        (with(id.0, pages(&[])), "is on no page"),
        (with(id.0 + 1, Scope::All), "2 draw the Over watermark"),
    ] {
        let mut st = st.clone();
        st.set(&key, val).unwrap();
        let why = st.check_invariants().unwrap_err().to_string();
        assert!(why.contains(says), "{why}");
    }
}
