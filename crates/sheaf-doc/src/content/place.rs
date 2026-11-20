//! A form placed in content by sheaf's geometry: the writer takes plain
//! numbers, and where a form's box lands on a page is [`Affine`]'s to
//! say.

use super::{ContentWriter, Res};
use crate::annot::Box2;
use crate::geom::{Affine, Rect};

/// Form `form`, its `/BBox` `bbox` and `/Matrix` `matrix`, drawn by `w`
/// so the box its matrix makes of its bbox fills `target`. A form whose
/// box has no area draws nothing, and isn't drawn.
pub fn place_form(w: &mut ContentWriter, form: Res, bbox: Rect, matrix: Affine, target: Box2) {
    let Some(onto) = Affine::onto(matrix.bounds(bbox), target) else {
        return;
    };
    w.save();
    w.transform(onto.array());
    w.draw(form);
    w.restore();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::Reserved;
    use crate::pdf::ObjectId;

    /// A form is placed by its box as its matrix turns it: a page turned
    /// a quarter, its box off the origin, fills the target.
    #[test]
    fn place_form_honors_bbox_and_matrix() {
        let place = |bbox: Rect, matrix: Affine| {
            let mut w = ContentWriter::new();
            let target = Box2 {
                x: 100.0,
                y: 200.0,
                w: 50.0,
                h: 200.0,
            };
            place_form(
                &mut w,
                Res::written(ObjectId::new(5, 0)),
                bbox,
                matrix,
                target,
            );
            let (bytes, needs) = w.finish(&Reserved::default());
            let text = String::from_utf8(bytes).unwrap();
            match needs.0.first() {
                Some(form) => text.replace(&*String::from_utf8_lossy(&form.name), "F"),
                None => text,
            }
        };
        // 200 × 100 at (50, 50), as it is: half as wide, twice as tall.
        let bbox = Rect::new(50.0, 50.0, 250.0, 150.0);
        assert_eq!(
            place(bbox, Affine::IDENTITY),
            "q\n0.25 0 0 2 87.5 100 cm\n/F Do\nQ\n"
        );
        // Turned a quarter from the origin: (x, y) is (-y, x), so it's
        // 100 × 200 at (-150, 50), drawn at a half.
        let turned = Affine {
            a: 0.0,
            b: 1.0,
            c: -1.0,
            d: 0.0,
            ..Affine::IDENTITY
        };
        assert_eq!(place(bbox, turned), "q\n0.5 0 0 1 175 150 cm\n/F Do\nQ\n");
        // A box with no area isn't drawn.
        assert_eq!(place(Rect::new(0.0, 0.0, 0.0, 9.0), Affine::IDENTITY), "");
    }
}
