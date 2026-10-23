//! The document model: a virtual sequence of pages over immutable source
//! files, with an outline tree, marks and form fields, and undo and redo.
//! See `docs/document-model.md`.
//!
//! The files are blobs in a [`Store`], each PDF read lazily as a
//! [`Source`] with lopdf (vendored, `vendor/lopdf`). Saves are written by
//! sheaf's own [`serialize`], encrypted as their source was and replacing
//! a file durably. Nothing in here knows about PDFium: rendering lives in
//! other crates.
//!
//! The edit vocabulary is sheaf-edit's, with the types the records share
//! with it (ids, geometry, OCR text, a tape's rows), which are re-exported
//! where they were defined before (M10.5 §6.1).

// Tests may use what clippy.toml bans (the lint ledger, M10.5 slice 19).
#![cfg_attr(test, allow(clippy::disallowed_methods))]

pub mod annot;
pub mod attachments;
pub mod change;
pub mod content;
pub mod describe;
pub mod effects;
pub mod exchange;
pub mod export;
pub mod file;
pub mod forms;
pub mod generate;
pub mod geom;
pub mod history;
pub mod images;
pub mod import;
pub mod kept;
pub mod labels;
pub mod layers;
pub mod materialize;
pub mod measure;
pub mod memory;
pub mod model;
pub mod open;
pub mod page_chars;
pub mod portfolio;
pub mod prim;
pub mod private;
pub mod properties;
pub mod realize;
pub mod recovery;
pub mod save;
pub mod serialize;
pub mod source;
pub mod stamp_lib;
pub mod store;
pub mod text_layer;
pub mod tx;

pub use sheaf_edit::{DestView, PageSize, Rotation, tape};

pub use annot::{Annot, AnnotId, AnnotKind, Box2, Pt, default_author, pdf_date_now};
pub use change::{Cause, ChangeSet};
pub use history::{AllOf, FixedCount, HistoryPolicy, HistoryStats, MemoryBudget};
pub use model::{
    AnnotBody, AnnotRec, Bookmark, ContentStack, CtxNeeds, DocState, Document, FlatOutlineItem,
    Foreign, FormInst, FormRef, GenId, GenSpec, Generator, GroupId, GroupKind, Invariant, Layer,
    Mark, OutlineId, PageBase, PageDef, PageId, Snapshot, SourceId, Static, Step, Target,
    TextStyle, VersionId, VersionView,
};
pub use page_chars::{Glyph, PageChars, Query, TextHit, TextOpts, TextService};
pub use realize::{MarkDraw, OcgOverride, RenderKey, ViewInputs, render_key, render_reads};
pub use source::{ObjRef, Source};
pub use store::{Blob, BlobId, BlobRef, Src, Store};
pub use text_layer::{OcrWord, TextLayer};
pub use tx::{
    Actor, Base, Commit, CommitOpts, EditError, Grant, Identity, JobKind, Notice, Preview,
    ResolvedOp, Stale, Tx, notice_text,
};

/// What sheaf counts pages in, for progress. One spelling everywhere:
/// `Unit::Count` compares the noun, so "page" wouldn't match.
pub const PAGES: in_progress::Unit = in_progress::Unit::Count("pages");
