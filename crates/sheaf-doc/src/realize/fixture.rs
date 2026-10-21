//! What the realizer's and the overlay's tests share (M10.5 §10): every
//! consumer's inputs, inputs hiding or dragging annotations, another tool's
//! comment, and recognized text.

use std::sync::Arc;

use sheaf_edit::Rotation;

use super::ViewInputs;
use crate::annot::AnnotId;
use crate::model::{DocState, PageId};
use crate::text_layer::{OcrWord, TextLayer};

/// The screen's inputs, a plugin's, print preview's with and without
/// markup, and OCR's.
pub(crate) fn every_input() -> [ViewInputs; 5] {
    [
        ViewInputs::screen(),
        ViewInputs::plugin(),
        ViewInputs::print(true),
        ViewInputs::print(false),
        ViewInputs::content_only(),
    ]
}

/// `inputs` hiding `ids`.
pub(crate) fn hiding(ids: &[AnnotId], inputs: ViewInputs) -> ViewInputs {
    ViewInputs {
        hidden: Arc::new(ids.iter().copied().collect()),
        ..inputs
    }
}

/// `inputs` with `ids` being dragged.
pub(crate) fn dragging(ids: &[AnnotId], inputs: ViewInputs) -> ViewInputs {
    ViewInputs {
        dragging: Arc::new(ids.iter().copied().collect()),
        ..inputs
    }
}

/// The first of other tools' comments on `page` (not a link).
pub(crate) fn their_comment(st: &DocState, page: PageId) -> AnnotId {
    let (id, _) = (st.foreign_on(page))
        .find(|(_, f)| !f.summary.is_link())
        .expect("a comment of theirs");
    id
}

/// Recognized text of one word, told apart by `engine`.
pub(crate) fn layer(engine: &str) -> Arc<TextLayer> {
    let word = OcrWord {
        text: "word".into(),
        rect: crate::annot::Box2 {
            x: 10.0,
            y: 10.0,
            w: 30.0,
            h: 10.0,
        },
        confidence: 0.9,
    };
    Arc::new(TextLayer {
        words: vec![word],
        lines: std::iter::once(0..1).collect(),
        upright: Rotation::R0,
        engine: engine.into(),
    })
}
