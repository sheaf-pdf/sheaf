//! A page as it shows, for placing things where the user sees them. sheaf
//! keeps geometry in PageSpace: a page's own user space, in points, y up,
//! before its turn. A page's info carries the maps to and from the page as
//! it shows: points, y down, from the top-left corner of what shows, turned
//! as it shows. Text, renders and marks come in PageSpace, and ops take it.
//!
//! ```ignore
//! // A 40×20 box 100 points in from the left and 200 down, as shown.
//! let shown = Box2 { x: 100.0, y: 200.0, w: 40.0, h: 20.0 };
//! let rect = view::box_from_view(&page, shown);
//! ```

use sheaf_edit::{Box2, DestView, Pt};

use crate::types::{self, Affine, PageInfo, Rotation};

/// `p` mapped by `m`.
pub fn apply(m: &Affine, p: Pt) -> Pt {
    Pt {
        x: m.a * p.x + m.c * p.y + m.e,
        y: m.b * p.x + m.d * p.y + m.f,
    }
}

/// `b` mapped by `m`, by its corners: a page's maps turn by quarters, so
/// a box maps to a box.
pub fn apply_box(m: &Affine, b: Box2) -> Box2 {
    let near = apply(m, Pt { x: b.x, y: b.y });
    let far = apply(
        m,
        Pt {
            x: b.right(),
            y: b.bottom(),
        },
    );
    Box2::from_corners(near, far)
}

/// PageSpace point `p` as `page` shows it.
pub fn to_view(page: &PageInfo, p: Pt) -> Pt {
    apply(&page.to_view, p)
}

/// A point as `page` shows it, in PageSpace.
pub fn from_view(page: &PageInfo, p: Pt) -> Pt {
    apply(&page.from_view, p)
}

/// PageSpace box `b` as `page` shows it.
pub fn box_to_view(page: &PageInfo, b: Box2) -> Box2 {
    apply_box(&page.to_view, b)
}

/// A box as `page` shows it, in PageSpace.
pub fn box_from_view(page: &PageInfo, b: Box2) -> Box2 {
    apply_box(&page.from_view, b)
}

/// A bookmark position as `page` shows it: the shown point (`left`,
/// `top`) at the window's top-left, either left out to keep the viewer's.
/// A page turns by quarters, so each PageSpace coordinate comes from one
/// shown one, and is left out where that is.
pub fn dest_from_view(page: &PageInfo, left: Option<f32>, top: Option<f32>) -> DestView {
    let m = &page.from_view;
    let along = |k: f32, v: Option<f32>, offset: f32| v.map(|v| k * v + offset);
    let (left, top) = match m.a != 0.0 {
        true => (along(m.a, left, m.e), along(m.d, top, m.f)),
        false => (along(m.c, top, m.e), along(m.b, left, m.f)),
    };
    DestView::Xyz {
        left,
        top,
        zoom: None,
    }
}

/// How big `page` shows, in points: its crop box, turned as it shows.
pub fn shown_size(page: &PageInfo) -> (f32, f32) {
    let (w, h) = (page.crop.w, page.crop.h);
    match page.rotate {
        Rotation::R0 | Rotation::R180 => (w, h),
        Rotation::R90 | Rotation::R270 => (h, w),
    }
}

impl From<types::Pt> for Pt {
    fn from(p: types::Pt) -> Pt {
        Pt { x: p.x, y: p.y }
    }
}

impl From<Pt> for types::Pt {
    fn from(p: Pt) -> types::Pt {
        types::Pt { x: p.x, y: p.y }
    }
}

impl From<types::Box2> for Box2 {
    fn from(b: types::Box2) -> Box2 {
        Box2 {
            x: b.x,
            y: b.y,
            w: b.w,
            h: b.h,
        }
    }
}

impl From<Box2> for types::Box2 {
    fn from(b: Box2) -> types::Box2 {
        types::Box2 {
            x: b.x,
            y: b.y,
            w: b.w,
            h: b.h,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A Letter page the file turns 90°, its crop box off the origin, as
    /// sheaf describes it: (20, 30)–(600, 770), shown 740 wide and 580
    /// tall.
    fn turned() -> PageInfo {
        let to_view = Affine {
            a: 0.0,
            b: 1.0,
            c: 1.0,
            d: 0.0,
            e: -30.0,
            f: -20.0,
        };
        let from_view = Affine {
            a: 0.0,
            b: 1.0,
            c: 1.0,
            d: 0.0,
            e: 20.0,
            f: 30.0,
        };
        PageInfo {
            id: 1,
            media: types::Box2 {
                x: 0.0,
                y: 0.0,
                w: 612.0,
                h: 792.0,
            },
            crop: types::Box2 {
                x: 20.0,
                y: 30.0,
                w: 580.0,
                h: 740.0,
            },
            rotate: Rotation::R90,
            label: "1".into(),
            to_view,
            from_view,
        }
    }

    /// What's placed through `from_view` shows where it was placed, and
    /// the page shows its turned size.
    #[test]
    fn a_box_placed_as_shown_shows_there() {
        let page = turned();
        let shown = Box2 {
            x: 100.0,
            y: 200.0,
            w: 40.0,
            h: 20.0,
        };
        let kept = box_from_view(&page, shown);
        assert_eq!(
            kept,
            Box2 {
                x: 220.0,
                y: 130.0,
                w: 20.0,
                h: 40.0
            }
        );
        assert_eq!(box_to_view(&page, kept), shown);
        let p = Pt { x: 5.0, y: 7.0 };
        assert_eq!(to_view(&page, from_view(&page, p)), p);
        assert_eq!(shown_size(&page), (740.0, 580.0));
    }

    /// A bookmark landing 200 points down the turned page as it shows,
    /// keeping the viewer's other coordinate, keeps PageSpace's x: the
    /// page's shown y runs along it.
    #[test]
    fn a_shown_top_lands_on_the_turned_page_s_axis() {
        let page = turned();
        let xyz = |left, top| DestView::Xyz {
            left,
            top,
            zoom: None,
        };
        assert_eq!(
            dest_from_view(&page, None, Some(200.0)),
            xyz(Some(220.0), None)
        );
        assert_eq!(
            dest_from_view(&page, Some(100.0), Some(200.0)),
            xyz(Some(220.0), Some(130.0))
        );
    }
}
