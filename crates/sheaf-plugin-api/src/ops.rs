//! Edits as ops: sheaf's own edit vocabulary ([`Op`]), handed to sheaf as
//! JSON and lowered into the call's edits at once. What a submit made is
//! answered with ids that stay true once the call's edits land; later ops
//! in the same submit name what earlier ones made by locals (`"$1"`).
//!
//! ```ignore
//! let applied = Ops::new()
//!     .bookmark("Form 1040", Some(page))
//!     .stamp(&info, Box2 { x: 10.0, y: 10.0, w: 24.0, h: 24.0 }, "✓")
//!     .submit("Bookmark forms")
//!     .map_err(|f| f.message)?;
//! ```

use serde::de::DeserializeOwned;
use sheaf_edit::{
    Anchor, Applied, BlobSel, BookmarkRef, Box2, DestView, Envelope, Failure, FailureKind, Local,
    MarkKind, NewMark, Op, PageId, PageRef, PageSel, Previewed, Rotation, Sibling, Style,
};

use crate::assets::AssetKind;
use crate::types::{PageInfo, Version};
use crate::{assets, edits};

/// sheaf's blue.
pub const BLUE: [u8; 3] = [0, 120, 255];

/// A mark's look in `color`, as sheaf draws one by default: opaque, a
/// 1.5-point line, no fill.
pub fn style(color: [u8; 3]) -> Style {
    Style {
        color,
        opacity: 1.0,
        stroke: 1.5,
        fill: None,
        dashed: false,
        cloudy: false,
    }
}

/// Page `id`, as the API names it.
pub fn page(id: u64) -> PageRef {
    PageRef::Id(PageId(id))
}

/// A bookmark position: this PageSpace point at the window's top-left.
pub fn at(left: f32, top: f32) -> DestView {
    DestView::Xyz {
        left: Some(left),
        top: Some(top),
        zoom: None,
    }
}

/// Ops to submit together, each made on what the ones before it made.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Ops {
    ops: Vec<Op>,
}

impl Ops {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(mut self, op: Op) -> Self {
        self.ops.push(op);
        self
    }

    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }

    pub fn len(&self) -> usize {
        self.ops.len()
    }

    pub fn ops(&self) -> &[Op] {
        &self.ops
    }

    /// A top-level bookmark at the end of the outline, landing on
    /// `target`.
    pub fn bookmark(self, title: &str, target: Option<u64>) -> Self {
        self.bookmark_in(None, title, target, None, None)
    }

    /// A bookmark at the end of `parent`'s children (the top level's with
    /// none), landing at `view` on `target`, and named `local` for later
    /// ops to put others under ([`BookmarkRef::New`]).
    pub fn bookmark_in(
        self,
        parent: Option<BookmarkRef>,
        title: &str,
        target: Option<u64>,
        view: Option<DestView>,
        local: Option<Local>,
    ) -> Self {
        self.push(Op::AddBookmark {
            parent,
            at: Sibling::Last,
            title: title.to_string(),
            target: target.map(page),
            view,
            color: None,
            style: Default::default(),
            local,
        })
    }

    /// A mark of `kind` on page `on`, reading upright as the page shows,
    /// in `style`, saying `contents`.
    pub fn mark(self, on: &PageInfo, kind: MarkKind, style: Style, contents: &str) -> Self {
        self.push(Op::AddMark {
            page: page(on.id),
            mark: NewMark {
                kind,
                style,
                contents: contents.to_string(),
                author: None,
                turn: Rotation::from(on.rotate),
                asset: None,
            },
            reply_to: None,
            local: None,
            imported: None,
        })
    }

    /// A tickmark showing `glyph` in `rect` on page `on`, in blue.
    pub fn stamp(self, on: &PageInfo, rect: Box2, glyph: &str) -> Self {
        let glyph = glyph.to_string();
        self.mark(on, MarkKind::Stamp { rect, glyph }, style(BLUE), "")
    }

    /// A page stamp of `asset`, an image or a PDF uploaded ([`upload`]),
    /// drawn in `rect` on page `on`, called `name`.
    pub fn page_stamp(self, on: &PageInfo, rect: Box2, asset: BlobSel, name: &str) -> Self {
        let kind = MarkKind::PageStamp {
            rect,
            name: name.to_string(),
            text: None,
        };
        let mut ops = self.mark(on, kind, style(BLUE), "");
        if let Some(Op::AddMark { mark, .. }) = ops.ops.last_mut() {
            mark.asset = Some(asset);
        }
        ops
    }

    /// A numbered connector pair: a square in `rect` on each page, linked.
    pub fn connector(self, a: (u64, Box2), b: (u64, Box2), label: &str) -> Self {
        let end = |(id, rect): (u64, Box2)| Anchor {
            page: page(id),
            rect,
        };
        self.push(Op::AddConnector {
            a: end(a),
            b: end(b),
            label: label.to_string(),
            style: style(BLUE),
            locals: Vec::new(),
        })
    }

    /// Pages `ids` deleted.
    pub fn delete_pages(self, ids: impl IntoIterator<Item = u64>) -> Self {
        let pages = PageSel::ids(ids.into_iter().map(PageId));
        self.push(Op::DeletePages { pages })
    }

    /// The JSON sheaf takes them as: an envelope in this crate's version of
    /// the edits, which the manifest's `edit-schema` names.
    pub fn to_json(&self) -> String {
        let envelope = Envelope::new(None, self.ops.clone());
        serde_json::to_string(&envelope).expect("ops are JSON")
    }

    /// Their edits, lowered into the call's now, all or none, as `label`
    /// (the call's first submit names its edits): what they made, which
    /// the edits keep once they land, or why not.
    pub fn submit(&self, label: &str) -> Result<Applied, Failure> {
        reply(edits::submit(label, &self.to_json()))
    }

    /// What they'd do, applied now, with nothing applied.
    pub fn preview(&self) -> Result<Previewed, Failure> {
        reply(edits::preview(&self.to_json()))
    }
}

/// The call's edits so far landed now, waiting while sheaf lands them: the
/// version they made, which the call's next edits are made on, or why
/// they didn't land.
pub fn commit() -> Result<Version, Failure> {
    edits::commit().map_err(failure)
}

/// `bytes` handed to sheaf as `kind` for this call's edits to hold: how
/// ops name them until the call returns.
pub fn upload(bytes: &[u8], kind: AssetKind) -> Result<BlobSel, String> {
    assets::upload(bytes, kind).map(BlobSel::Asset)
}

/// An answer of sheaf's: what it says, or why it failed.
fn reply<T: DeserializeOwned>(answer: Result<String, String>) -> Result<T, Failure> {
    let json = answer.map_err(failure)?;
    serde_json::from_str(&json).map_err(|e| Failure {
        kind: FailureKind::Internal,
        message: format!("reading sheaf's answer: {e}"),
    })
}

/// Why sheaf refused something, from its JSON.
fn failure(json: String) -> Failure {
    serde_json::from_str(&json).unwrap_or(Failure {
        kind: FailureKind::Internal,
        message: json,
    })
}

impl From<crate::types::Rotation> for Rotation {
    fn from(r: crate::types::Rotation) -> Rotation {
        use crate::types::Rotation as R;
        match r {
            R::R0 => Rotation::R0,
            R::R90 => Rotation::R90,
            R::R180 => Rotation::R180,
            R::R270 => Rotation::R270,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ops cross as an envelope in this crate's version of the edits, and
    /// read back as sheaf reads them.
    #[test]
    fn ops_cross_as_an_envelope() {
        let r = Box2 {
            x: 1.0,
            y: 2.0,
            w: 3.0,
            h: 4.0,
        };
        let ops = Ops::new()
            .bookmark("Form 1040", Some(7))
            .connector((7, r), (9, r), "3")
            .delete_pages([9]);
        assert_eq!(ops.len(), 3);
        let read: Envelope = serde_json::from_str(&ops.to_json()).unwrap();
        assert_eq!(
            (read.v.as_str(), read.label, &read.ops[..]),
            (sheaf_edit::VERSION, None, ops.ops())
        );
        let Op::AddConnector { a, b, label, .. } = &ops.ops()[1] else {
            panic!("{:?}", ops.ops()[1])
        };
        assert_eq!(
            (&a.page, &b.page, label.as_str()),
            (&page(7), &page(9), "3")
        );
        assert!(Ops::new().is_empty());
    }

    /// A failure is read from its JSON, and anything else sheaf says is
    /// kept as it said it.
    #[test]
    fn failures_read_from_their_json() {
        let said = r#"{"kind": "refused", "message": "the user undid this"}"#;
        assert_eq!(
            failure(said.into()),
            Failure {
                kind: FailureKind::Refused,
                message: "the user undid this".into(),
            }
        );
        assert_eq!(failure("trap".into()).kind, FailureKind::Internal);
        let answer: Result<Applied, _> = reply(Ok("{}".into()));
        assert_eq!(answer.unwrap_err().kind, FailureKind::Internal);
    }
}
