//! sheaf's edit vocabulary (M10.5 §6.1): the ops a caller states an edit
//! in ("delete pages 3–5"), the references they name things by, and the
//! payloads they carry, as types and as JSON. The app, the command line
//! and plugins all edit a document by handing it ops; sheaf-doc lowers
//! them into changes of its model, and re-exports the types its records
//! share with them (the ids, geometry, the OCR text and a tape's rows).
//!
//! Types only, with the pure helpers that go with them: serde and
//! schemars, and nothing of sheaf's, so it builds for wasm32-wasip2 and a
//! plugin can use it.
//!
//! Op JSON is a file format, versioned by its [`Envelope`]. An op's own
//! specs refuse fields they don't know, so a misspelled one is an error;
//! a field added after 1.0 has a default, which a test holds the schema
//! to against the committed `schema/ops-1.0.json`. Positions are 1-based,
//! as the UI shows them, and geometry is in PageSpace (the page's default
//! user space: points, y up, before `/Rotate`).

pub mod envelope;
#[cfg(feature = "fixtures")]
pub mod fixture;
pub mod geom;
pub mod header_footer;
pub mod ids;
pub mod labels;
pub mod mark;
pub mod measure;
pub mod op;
pub mod page;
pub mod patch;
pub mod refs;
pub mod tape;
pub mod text_layer;
pub mod watermark;

pub use envelope::{Envelope, VERSION};
pub use geom::{Box2, Pt, Vec2};
pub use header_footer::{BatesSpec, HeaderFooterSpec};
pub use ids::{AnnotId, BlobId, FieldId, Local, NotALocal, OutlineId, PageId, SourceId};
pub use labels::{LabelStyle, PageLabelRange};
pub use mark::{Anchor, Color, FontChoice, Imported, MarkKind, MarkPatch, NewMark, Style};
pub use op::{BookmarkPatch, Bring, Delta, FormBring, Op, OutlineBring, PageSource, Sibling, Turn};
pub use page::{BoxSpec, DestView, Margins, PageSize, Rotation, TextStyle};
pub use patch::Patch;
pub use refs::{
    AnnotRef, BlobSel, BookmarkRef, PageRef, PageSel, RangeSet, RangeSetError, ScopeSel,
};
pub use text_layer::{OcrWord, TextLayer};
pub use watermark::{GridPos, WatermarkContent, WatermarkSpec, Z};
