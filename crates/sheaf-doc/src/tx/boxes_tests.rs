//! Page boxes as `SetPageBoxes` sets them (M10.5 slice 15): margins off
//! the sides as the page shows, rectangles in PageSpace within the media
//! box, the crop cleared to the media box; what isn't a box, or leaves too
//! little of the page, fails the op; only the pages' boxes are written,
//! and only where they change; and the lines say what was done.

use in_progress::NoProgress;
use sheaf_edit::{BoxSpec, Margins, Op, Patch, Rotation, Turn};

use super::fixture as fx;
use super::tests::{doc_of, opened};
use super::*;
use crate::annot::Box2;
use crate::geom::{Geom, Rect};
use crate::model::Document;
use crate::prim::Key;

fn margins(top: f32, right: f32, bottom: f32, left: f32) -> Margins {
    Margins {
        top,
        bottom,
        left,
        right,
    }
}

fn all(by: f32) -> Margins {
    margins(by, by, by, by)
}

fn rect(x: f32, y: f32, w: f32, h: f32) -> Box2 {
    Box2 { x, y, w, h }
}

/// `pages`' boxes: `crop`, `trim`, `bleed` and `art` as given.
fn boxes(
    pages: &[PageId],
    crop: Patch<BoxSpec>,
    trim: Patch<BoxSpec>,
    bleed: Patch<BoxSpec>,
    art: Patch<BoxSpec>,
) -> Op {
    Op::SetPageBoxes {
        pages: fx::ids(pages),
        crop,
        trim,
        bleed,
        art,
    }
}

/// `pages`' crop cleared, to the media box.
fn uncrop(pages: &[PageId]) -> Op {
    boxes(pages, Patch::Clear, Patch::Keep, Patch::Keep, Patch::Keep)
}

/// Why `op` fails on `d`, as the reader is told it.
fn fails(d: &mut Document, op: Op) -> String {
    let (at, n) = (d.current_version(), d.version_count());
    let e = d.commit_ops(vec![op], "boxes").unwrap_err();
    assert!(matches!(e, EditError::Invalid { .. }), "{e}");
    assert_eq!((d.current_version(), d.version_count()), (at, n), "{e}");
    e.to_string()
}

/// Margins are taken off the sides as the page shows, turned by its
/// `/Rotate` and by sheaf: on a page the file turns 90°, the top margin
/// comes off its user space's left, and turned 90° more in sheaf, off its
/// bottom. A media box off the origin moves nothing.
#[test]
fn margins_crop_the_shown_sides() {
    let m = margins(10.0, 20.0, 30.0, 40.0);
    let mut f = crate::forms::fixture::Form::new(1);
    let page = f.pages[0];
    let media = [100.0f32, 50.0, 712.0, 842.0].map(lopdf::Object::from);
    f.dict(page).set("MediaBox", media.to_vec());
    f.dict(page).set("Rotate", 90);
    let bytes = f.bytes();
    for (by, want) in [
        (Rotation::R0, Rect::new(110.0, 90.0, 682.0, 822.0)),
        (Rotation::R90, Rect::new(120.0, 60.0, 672.0, 812.0)),
    ] {
        let mut d = opened(bytes.clone(), "t.pdf");
        let id = d.page(0).unwrap();
        if let Some(by) = Turn::by(by) {
            d.commit_ops(vec![fx::rotate(&[id], by)], "rotate").unwrap();
        }
        let was = d.geom(id);
        d.commit_ops(vec![fx::crop(&[id], m)], "crop").unwrap();
        let now = d.geom(id);
        assert_eq!(now.crop, want, "{by:?}");
        assert_eq!(
            Geom {
                crop: was.crop,
                ..now
            },
            was,
            "only the crop"
        );
        let (shown, cropped) = (was.shown(), now.shown());
        assert_eq!(cropped.width, shown.width - 60.0, "{by:?}");
        assert_eq!(cropped.height, shown.height - 40.0, "{by:?}");
    }
}

/// Margins come off the box the page shows, its crop, not its media box.
#[test]
fn margins_come_off_the_crop() {
    let mut d = doc_of(1, &[]);
    let p = d.page(0).unwrap();
    d.commit_ops(
        vec![fx::crop_to(p, rect(100.0, 100.0, 200.0, 300.0))],
        "crop",
    )
    .unwrap();
    assert_eq!(d.geom(p).crop, Rect::new(100.0, 100.0, 300.0, 400.0));
    d.commit_ops(vec![fx::crop(&[p], all(10.0))], "crop")
        .unwrap();
    assert_eq!(d.geom(p).crop, Rect::new(110.0, 110.0, 290.0, 390.0));
}

/// A rectangle within the media box is the box; one that strays past it by
/// float noise is clipped to it; one partly off the page fails, never
/// repaired. A cleared crop is the media box again.
#[test]
fn a_rect_is_taken_within_the_media_box_alone() {
    let mut d = doc_of(2, &[]);
    let [p0, p1] = [0, 1].map(|i| d.page(i).unwrap());
    let media = d.geom(p0).media;
    assert_eq!(media, Rect::LETTER);
    d.commit_ops(
        vec![fx::crop_to(p0, rect(512.0, 0.0, 100.005, 100.0))],
        "noise",
    )
    .unwrap();
    assert_eq!(d.geom(p0).crop, Rect::new(512.0, 0.0, 612.0, 100.0));
    let off = fails(&mut d, fx::crop_to(p1, rect(600.0, 0.0, 100.0, 100.0)));
    assert_eq!(off, "set-page-boxes: the box isn't on page 2");
    let below = fails(&mut d, fx::crop_to(p1, rect(0.0, -1.0, 100.0, 100.0)));
    assert_eq!(below, "set-page-boxes: the box isn't on page 2");
    d.commit_ops(vec![uncrop(&[p0])], "uncrop").unwrap();
    assert_eq!(d.geom(p0).crop, media);
}

/// What isn't a box, or leaves less than a quarter inch of the page on a
/// side, fails the op, which changes nothing, saying which page; a box of
/// a quarter inch is one.
#[test]
fn a_box_that_leaves_too_little_or_isnt_one_fails() {
    let mut d = doc_of(3, &[]);
    let [p0, p1, p2] = [0, 1, 2].map(|i| d.page(i).unwrap());
    let both = [p0, p1];
    for (op, why) in [
        (
            fx::crop(&both, margins(400.0, 0.0, 380.0, 0.0)),
            "those margins leave nothing of page 1",
        ),
        (
            fx::crop(&[p1], margins(0.0, 600.0, 0.0, 0.0)),
            "those margins leave nothing of page 2",
        ),
        (
            fx::crop(&[p2], margins(0.0, 0.0, 0.0, 700.0)),
            "those margins leave nothing of page 3",
        ),
        (
            fx::crop(&both, margins(0.0, -1.0, 0.0, 0.0)),
            "a margin can't be less than 0",
        ),
        (
            fx::crop(&both, margins(f32::NAN, 0.0, 0.0, 0.0)),
            "a margin isn't a number",
        ),
        (
            fx::crop(&both, margins(0.0, 0.0, f32::INFINITY, 0.0)),
            "a margin isn't a number",
        ),
        (
            fx::crop_to(p1, rect(10.0, 10.0, 10.0, 100.0)),
            "the box leaves nothing of page 2",
        ),
        (
            fx::crop_to(p1, rect(10.0, 10.0, 100.0, -50.0)),
            "the box leaves nothing of page 2",
        ),
        (
            fx::crop_to(p0, rect(f32::NAN, 10.0, 100.0, 100.0)),
            "the box isn't a number",
        ),
    ] {
        assert_eq!(fails(&mut d, op), format!("set-page-boxes: {why}"));
    }
    // Every box is held to it, not only the crop.
    let trim = boxes(
        &[p0],
        Patch::Keep,
        Patch::Set(BoxSpec::Rect(rect(0.0, 0.0, 17.0, 100.0))),
        Patch::Keep,
        Patch::Keep,
    );
    assert_eq!(
        fails(&mut d, trim),
        "set-page-boxes: the box leaves nothing of page 1"
    );
    // A quarter inch left is enough.
    d.commit_ops(
        vec![fx::crop(&[p0], margins(387.0, 0.0, 387.0, 0.0))],
        "thin",
    )
    .unwrap();
    assert_eq!(d.geom(p0).crop, Rect::new(0.0, 387.0, 612.0, 405.0));
}

/// Trim, bleed and art are set as the crop is, from what the page shows
/// before the op, and cleared to none; cleared, the crop is the media box.
#[test]
fn every_box_is_set_and_cleared() {
    let mut d = doc_of(1, &[]);
    let p = d.page(0).unwrap();
    let set = boxes(
        &[p],
        Patch::Set(BoxSpec::Margins(all(36.0))),
        Patch::Set(BoxSpec::Rect(rect(18.0, 18.0, 576.0, 756.0))),
        Patch::Set(BoxSpec::Margins(all(0.0))),
        Patch::Set(BoxSpec::Margins(margins(72.0, 0.0, 0.0, 0.0))),
    );
    d.commit_ops(vec![set], "boxes").unwrap();
    let g = d.geom(p);
    assert_eq!(g.crop, Rect::new(36.0, 36.0, 576.0, 756.0));
    assert_eq!(g.trim, Some(Rect::new(18.0, 18.0, 594.0, 774.0)));
    assert_eq!(g.bleed, Some(Rect::LETTER));
    assert_eq!(g.art, Some(Rect::new(0.0, 0.0, 612.0, 720.0)));
    let clear = boxes(&[p], Patch::Clear, Patch::Clear, Patch::Keep, Patch::Clear);
    d.commit_ops(vec![clear], "clear").unwrap();
    let g = d.geom(p);
    assert_eq!((g.crop, g.trim, g.art), (Rect::LETTER, None, None));
    assert_eq!(g.bleed, Some(Rect::LETTER), "kept");
}

/// A crop writes each page's boxes and nothing else, and none where they
/// stay as they are: a crop to what a page shows makes no version.
#[test]
fn a_crop_writes_the_boxes_it_changes_alone() {
    let mut d = doc_of(3, &[]);
    let [p0, p1, p2] = [0, 1, 2].map(|i| d.page(i).unwrap());
    let c = d
        .commit_ops(vec![fx::crop(&[p0, p2], all(36.0))], "crop")
        .unwrap();
    let keys: Vec<Key> = c.change.keys.iter().copied().collect();
    assert_eq!(keys, [Key::Geom(p0), Key::Geom(p2)]);
    let same = vec![
        fx::crop(&[p1], all(0.0)),
        fx::crop_to(p1, rect(0.0, 0.0, 612.0, 792.0)),
        uncrop(&[p1]),
        boxes(&[p1], Patch::Keep, Patch::Clear, Patch::Clear, Patch::Clear),
    ];
    let at = d.current_version();
    let c = d.commit_ops(same, "the same").unwrap();
    assert_eq!(c.version, None);
    assert_eq!(d.current_version(), at);
}

/// A crop's line names its pages; a run of crops counts them; a crop
/// cleared is said so, and other boxes are "page boxes".
#[test]
fn crops_read_as_crops() {
    let d = doc_of(4, &[]);
    let [p0, p1, p2, p3] = [0, 1, 2, 3].map(|i| d.page(i).unwrap());
    let lines = |ops: Vec<Op>| {
        let mut tx = d.tx(Actor::User);
        tx.apply_batch(ops, &NoProgress).unwrap();
        let lines = tx.preview().lines;
        lines.into_iter().map(|l| l.text).collect::<Vec<_>>()
    };
    let trim = || {
        boxes(
            &[p0],
            Patch::Keep,
            Patch::Set(BoxSpec::Margins(all(9.0))),
            Patch::Keep,
            Patch::Keep,
        )
    };
    assert_eq!(lines(vec![fx::crop(&[p2], all(9.0))]), ["Crop page 3"]);
    assert_eq!(
        lines(vec![fx::crop(&[p0, p1, p2], all(9.0))]),
        ["Crop pages 1–3"]
    );
    assert_eq!(lines(vec![uncrop(&[p1])]), ["Remove the crop from page 2"]);
    assert_eq!(lines(vec![trim()]), ["Change the page boxes of page 1"]);
    let three = [p0, p2, p3].map(|p| fx::crop(&[p], all(9.0)));
    assert_eq!(lines(three.to_vec()), ["Crop 3 pages"]);
    let mixed = vec![fx::crop(&[p1, p2], all(9.0)), trim()];
    assert_eq!(lines(mixed), ["Change the page boxes of 3 pages"]);
    let both = vec![uncrop(&[p1]), uncrop(&[p2, p3])];
    assert_eq!(lines(both), ["Remove the crop from 3 pages"]);
}
