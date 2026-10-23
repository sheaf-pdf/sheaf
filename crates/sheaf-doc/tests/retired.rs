//! The retired-symbols ledger (M10.5 §17): every name the edit API took
//! out, with the slice that took it out ([`RETIRED`]), and every name a
//! later milestone's slice took out, in a table of the milestone's own
//! ([`RETIRED_M5`]). Nothing under `crates/` may define one again or refer
//! to it. Each slice adds its rows in the commit that deletes the names.
//!
//! A row is a path from its crate's root, such as
//! `model::Document::marginal`. Every `.rs` file under `crates/` (but not
//! under a `target`) is parsed with `syn`, and a row fails on:
//! - **a definition at that path:** an item, an impl's or trait's item as
//!   `Type::item`, a field as `Type::field`, a variant as `Type::Variant`,
//!   a variant's field as `Type::Variant::field`.
//!   A file's module comes from its path, and inline `mod`s nest;
//! - **a path that names it:** one that holds the row's last two segments
//!   in a row, after a leading `crate`, `self`, `super` or `sheaf_*`, in
//!   code or in a `use`; for a row of one segment, an item at a crate's
//!   root, one that starts with it there (`w::OutlineItem` is another
//!   module's);
//! - **its last segment alone,** as a method, the function a call calls, a
//!   type, a struct literal's or pattern's path, or a field, unless that
//!   name is in [`QUALIFIED_ONLY`]. A plain value, such as a local
//!   variable, is never a hit.
//!
//! Macro bodies count where they read as expressions (`assert_eq!`,
//! `format!`).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use syn::punctuated::Punctuated;
use syn::visit::{self, Visit};
use syn::{Expr, Ident, ImplItem, Member, Pat, TraitItem, Type, UseTree};

/// Every name M10.5 retired, with the slice that retired it.
const RETIRED: &[(&str, u8)] = &[
    ("model::Ids", 1),
    ("model::Step::ids", 1),
    ("model::Document::replay_step", 1),
    ("model::Document::rng", 1),
    ("model::Document::next_page", 1),
    ("model::Document::next_outline", 1),
    ("model::Document::next_field", 1),
    ("model::Document::fresh_page_id", 1),
    ("model::Document::fresh_outline_id", 1),
    ("model::Document::marginal", 1),
    ("model::Version::marginal_bytes", 1),
    ("history::HistoryStats::marginal_bytes", 1),
    ("model::SourceInfo::data", 3),
    ("model::SourceInfo::bytes", 3),
    ("model::SourceInfo::page_sizes", 3),
    ("model::SourceInfo::page_count", 3),
    ("model::SourceInfo::first_page", 3),
    ("model::SourceInfo::password", 3),
    ("model::SourceInfo::page_size", 3),
    ("model::SourceInfo::path", 3),
    ("model::SourceData", 3),
    ("model::Document::set_source_sizes", 3),
    ("file::source_bytes", 3),
    ("RenderHandle::page_sizes", 3),
    ("RenderHandle::open", 3),
    ("RenderHandle::data", 3),
    ("Renderer::page_sizes", 3),
    ("pdfium::PdfiumEngine::bytes", 3),
    ("Msg::PageSizes", 3),
    ("SizeWalk", 3),
    ("SIZE_CHUNK", 3),
    ("DocInfo::page_count", 3),
    ("DocInfo::first_page", 3),
    ("docview::DocView::fetch_page_sizes", 3),
    ("docview::DocView::fetch_source_sizes", 3),
    ("forms::read::MAX_DEPTH", 3),
    ("forms::read::MAX_NODES", 3),
    ("forms::read::MAX_ENTRIES", 3),
    ("forms::read::MAX_WIDGETS", 3),
    ("forms::read::MAX_NAME", 3),
    ("forms::read::MAX_NAMES", 3),
    ("forms::read::MAX_ITEMS", 3),
    ("forms::read::MAX_TEXT", 3),
    ("forms::read::MAX_TEXTS", 3),
    ("forms::read::MAX_DA", 3),
    ("forms::read::MAX_SCRIPT_ENTRIES", 3),
    ("forms::check::MAX_DA", 3),
    ("forms::check::MAX_XFA", 3),
    ("forms::xfa::MAX_XFA", 3),
    ("properties::properties_of", 3),
    ("properties::page_resources", 3),
    ("properties::linearized", 3),
    ("properties_dialog::PropertiesContext::password", 3),
    ("properties_dialog::PropertiesContext::bytes", 3),
    ("file::read_open", 4),
    ("file::facts_in", 4),
    ("file::FileFacts", 4),
    ("file::outline_styles", 4),
    ("file::OutlineStyle", 4),
    ("file::style_flat_items", 4),
    ("model::Document::set_initial_outline_styles", 4),
    ("docview::Doc::pending_outline_styles", 4),
    ("docview::Incoming::outline_styles", 4),
    ("docview::flat_item", 4),
    ("Renderer::outline", 4),
    ("pdfium::PdfiumEngine::outline", 4),
    ("RenderHandle::outline", 4),
    ("Msg::Outline", 4),
    ("OutlineItem", 4),
    ("open::open", 4),
    ("open::open_with_foreign", 4),
    ("open::describe", 4),
    ("model::Document::set_initial_outline", 4),
    ("model::Document::set_initial_labels", 4),
    ("model::Document::set_initial_header_footer", 4),
    ("model::Document::set_initial_annots", 4),
    ("model::Document::set_initial_fields", 4),
    ("model::Document::opened_state_mut", 4),
    ("docview::DocView::fetch_outline", 4),
    ("docview::DocView::load_sheaf_annots", 4),
    ("docview::Doc::outline_loaded", 4),
    ("docview::Doc::facts_loaded", 4),
    ("form_bar::Doc::take_form", 4),
    ("labels::page_labels", 4),
    ("incremental::apply_rotation", 5),
    ("annot::PageGeom", 5),
    ("annot::visible_box", 5),
    ("exchange::Geoms", 5),
    ("crop::unrotate", 5),
    ("comments::summary::rotate_box", 5),
    ("comments::summary::Picture::source", 5),
    ("comments::summary::Picture::rotation", 5),
    ("file::PdfFile::page_geometry", 5),
    ("model::Rebase::shift", 5),
    ("text_layer::TextLayer::shifted", 5),
    ("text_layer::TextLayer::page", 5),
    ("page_chars::PageChars::page", 5),
    ("text_layer::rotate_box", 5),
    ("PageFrame", 5),
    ("open::outline::Walk::frames", 5),
    ("open::outline::Walk::frame", 5),
    ("pdfium::bookmarks::frame_of", 5),
    ("job::OcrPage::rotation", 5),
    ("job::OcrPage::size", 5),
    ("job::PageSource::page_size", 5),
    ("RenderHandle::page_size", 5),
    ("Msg::PageSize", 5),
    ("markup_tools::rotate_pt", 5),
    ("markup_tools::unrotate_pt", 5),
    ("docview::rotate_rect", 5),
    ("text_layer::rotate_pt", 5),
    ("stamps::strip_ops", 6),
    ("stamps::has_artifacts", 6),
    ("stamps::is_kind", 6),
    ("stamps::contains", 6),
    ("stamps::HELVETICA", 6),
    ("stamps::HELVETICA_BOLD", 6),
    ("stamps::helvetica_advance", 6),
    ("stamps::helvetica_width", 6),
    ("annot::helv_width", 6),
    ("annot::helv_text_width", 6),
    ("stamps::win_ansi", 6),
    ("stamps::pdf_literal", 6),
    ("stamps::StampObjects", 6),
    ("stamps::objects", 6),
    ("text_layer::OcrObjects", 6),
    ("text_layer::ocr_objects", 6),
    ("text_layer::add_stream", 6),
    ("text_layer::utf16_hex", 6),
    ("text_layer::GLYPHLESS_TTF", 6),
    ("comments::summary::Sheet", 6),
    ("comments::summary::wrap", 6),
    ("comments::summary::width", 6),
    ("comments::summary::dingbat", 6),
    ("comments::summary::Sheet::use_form", 6),
    ("annot::content_literal", 6),
    ("annot::font_resources", 6),
    ("annot::wrap_lines", 6),
    ("annot::HELV", 6),
    ("annot::HELV_BOLD", 6),
    ("annot::ZADB", 6),
    ("annot::color_ops", 6),
    ("annot::stroke_ops", 6),
    ("annot::cloud_ops", 6),
    ("save::StampForms", 6),
    ("file::recover_cached", 6),
    ("file::annots_and_cached", 6),
    ("generated::store_as", 6),
    ("generated::store_blob_as", 6),
    ("generated::legacy_name_of", 6),
    ("stamp_preview::png_path", 6),
    ("stamp_preview::TAPES", 6),
    ("stamps_panel::Library::stems", 6),
    ("incremental::write_object", 7),
    ("incremental::write_dict", 7),
    ("incremental::write_name", 7),
    ("incremental::runs", 7),
    ("save::write_atomic", 7),
    ("images::write_atomic", 7),
    ("save::WouldBreakSignatures::encrypted", 7),
    ("model::PageRef", 8),
    ("model::OutlineNode", 8),
    // DocState's fields and accessors went in slice 8, which moved its
    // struct to model/state.rs: a field would come back there, and a
    // method in any of the files with an `impl DocState`.
    ("model::state::DocState::index_of", 8),
    ("model::state::DocState::page_positions", 8),
    ("model::state::DocState::text_layers", 8),
    ("model::state::DocState::text_layer", 8),
    ("model::state::DocState::has_text_layer", 8),
    ("model::state::DocState::outline", 8),
    ("model::state::DocState::outline_flat", 8),
    ("model::state::DocState::removed_comments", 8),
    ("model::state::DocState::widgets", 8),
    ("model::DocState::index_of", 8),
    ("model::DocState::page_positions", 8),
    ("model::DocState::text_layer", 8),
    ("model::DocState::has_text_layer", 8),
    ("model::DocState::outline_flat", 8),
    ("prim::DocState::index_of", 8),
    ("prim::DocState::page_positions", 8),
    ("prim::DocState::text_layer", 8),
    ("prim::DocState::has_text_layer", 8),
    ("prim::DocState::outline_flat", 8),
    ("model::Document::saved_outline", 8),
    ("model::Document::saved_labels", 8),
    ("model::Document::saved_annots", 8),
    ("model::Document::saved_removed", 8),
    ("model::Document::saved_fields", 8),
    ("model::Document::baked_header_footer", 8),
    ("model::Document::forms", 8),
    ("model::Document::set_source_form", 8),
    ("model::Document::assets", 8),
    ("model::Document::set_source_assets", 8),
    ("model::Document::set_saved_assets", 8),
    ("model::Snapshot::assets", 8),
    ("model::source_geom", 8),
    ("model::render_delta_in", 8),
    ("model::outline_find", 8),
    ("model::outline_update", 8),
    ("model::outline_remove", 8),
    ("model::subtree_contains", 8),
    ("model::strip_targets", 8),
    ("model::count_nodes", 8),
    ("model::collect_targets", 8),
    ("model::collect_ids", 8),
    ("model::import_marks", 8),
    ("prim::Key::Pages", 8),
    ("prim::Key::Outline", 8),
    ("prim::Key::AnnotsOf", 8),
    ("prim::Key::TextLayer", 8),
    ("prim::Key::RemovedOf", 8),
    ("prim::Key::HeaderFooter", 8),
    ("prim::Key::WidgetsOf", 8),
    ("prim::Val::Pages", 8),
    ("prim::Val::Outline", 8),
    ("prim::Val::Annots", 8),
    ("prim::Val::TextLayer", 8),
    ("prim::Val::Removed", 8),
    ("prim::Val::HeaderFooter", 8),
    ("prim::Val::Widgets", 8),
    ("open::Facts::marks", 8),
    ("open::Facts::renamed", 8),
    ("open::Facts::foreign", 8),
    ("open::Facts::assets", 8),
    ("open::ViewFacts::foreign", 8),
    ("file::AnnotsInFile::assets", 8),
    ("file::ForeignAnnot::page", 8),
    ("file::is_comment_subtype", 8),
    ("assets::MarkAssets", 8),
    ("assets::SourceAssets", 8),
    ("assets::Stamp", 8),
    ("assets::File", 8),
    ("generated::store_blob", 8),
    ("generated::blob_path", 8),
    ("annot::copy_stream", 8),
    ("attachments::AttachmentRef::Cached", 8),
    ("forms::field::Field::source_name", 8),
    ("forms::field::Field::has_scripts", 8),
    ("forms::fixture::mark_assets", 8),
    ("runtime::HostState::page_ref", 8),
    ("docview::Doc::page_ref", 8),
    ("docview::Doc::foreign_annots", 8),
    ("docview::Doc::foreign_generation", 8),
    ("docview::Doc::index_of_page_in", 8),
    ("docview::Incoming::marks", 8),
    ("docview::Incoming::renamed", 8),
    ("docview::Incoming::assets", 8),
    ("docview::Incoming::foreign", 8),
    ("docview::Incoming::form", 8),
    ("docview::reread_foreign", 8),
    ("form_bar::Forms::incoming", 8),
    ("outline_panel::locate", 8),
    ("stamp_preview::Tried", 8),
    ("stamp_preview::Previews::missing", 8),
    ("model::Saved", 9),
    ("save::SaveStop", 9),
    ("save::Ask", 9),
    ("save::LoadedSource", 9),
    ("file::load_source", 9),
    ("save::INHERITED", 9),
    ("save::inherited_attrs", 9),
    ("save::copy_page_dict", 9),
    ("save::is_page_node", 9),
    ("save::copy_object", 9),
    ("save::remap", 9),
    ("content::to_lopdf::resources_of", 9),
    ("content::to_lopdf::ResObjects::stream", 9),
    ("save::build_rewrite", 9),
    ("save::rewrite_with", 9),
    ("save::finish", 9),
    ("save::kept_encryption", 9),
    ("serialize::full::write", 9),
    ("model::Document::rebase_to_saved", 9),
    ("model::rebind_annots", 9),
    ("model::rebind_fields", 9),
    ("model::Document::mark_saved", 9),
    ("recovery::MAGIC", 9),
    ("recovery::materialize", 9),
    ("docview::DocView::save_signed", 9),
    ("docview::DocView::ask_how_to_save_signed", 9),
    ("docview::DocView::ask_file_changed", 9),
    ("docview::DocView::pending_signed", 9),
    ("docview::DocView::pending_changed", 9),
    ("docview::DocView::pending_xfa", 9),
    ("docview::Doc::layers_default", 9),
    ("form_bar::DocView::would_drop_xfa_values", 9),
    ("form_bar::DocView::ask_to_drop_xfa_values", 9),
    ("attachments_panel::DocView::refresh_facts", 9),
    ("layers_panel::Doc::layers_changed", 9),
    // Slice 9, step 7: the old writers, the old entry point, the legacy
    // projection, the dirty tracking and the renames table.
    ("incremental::incremental", 9),
    ("incremental::append", 9),
    ("incremental::incremental_section", 9),
    ("incremental::Mode", 9),
    ("save::save_to_path", 9),
    ("save::save_to_path_noting", 9),
    ("save::Strategy", 9),
    ("save::Signed", 9),
    ("save::WouldBreakSignatures", 9),
    ("save::IfChanged", 9),
    ("save::write_checked", 9),
    ("save::file_changed", 9),
    ("save::write_outline", 9),
    ("save::outline_dest", 9),
    ("save::write_page_annots", 9),
    ("save::ConnectorRef", 9),
    ("save::PageAnnotsWritten", 9),
    ("save::strip_sheaf_annots", 9),
    ("save::link_connectors", 9),
    ("save::is_identity", 9),
    ("save::reserve_pages", 9),
    ("save::reserve_page_objects", 9),
    ("save::copy_of", 9),
    ("save::origin", 9),
    ("save::origins", 9),
    ("save::needed", 9),
    ("save::copy_derived_page", 9),
    ("save::geom_of", 9),
    ("forms::merge::CopyMaps", 9),
    ("forms::merge::FormCopy", 9),
    ("forms::merge::page_copy", 9),
    ("forms::merge::copy_annots", 9),
    ("forms::merge::annots_on", 9),
    ("forms::merge::merge", 9),
    ("forms::rules::take_out", 9),
    ("forms::read::fields_of", 9),
    ("kept::drop_dead_links", 9),
    ("kept::prune_fields", 9),
    ("materialize::graph::Loaded::pdf", 9),
    ("materialize::graph::Loaded::from_pdf_owned", 9),
    ("file::page_geom", 9),
    ("file::read_annots", 9),
    ("file::read_annots_mem", 9),
    ("file::read_sheaf_annots", 9),
    ("file::read_sheaf_annots_mem", 9),
    ("file::removed_with_threads", 9),
    ("annot::WriteCx::owning", 9),
    ("annot::WriteCx::own", 9),
    ("serialize::append::Base::of", 9),
    ("model::LegacyDocState", 9),
    ("model::legacy::LegacyDocState", 9),
    ("model::legacy_field", 9),
    ("model::legacy::legacy_field", 9),
    ("model::Snapshot::legacy", 9),
    ("forms::field::Field::source", 9),
    ("forms::field::Field::copy", 9),
    ("forms::field::Field::under", 9),
    ("model::Document::baseline", 9),
    ("model::Document::removes_content", 9),
    ("model::Document::annots_dirty_pages", 9),
    ("model::Document::fields_dirty", 9),
    ("model::Document::header_footer_dirty", 9),
    ("model::Document::form_changed", 9),
    ("model::Document::widgets_changed", 9),
    ("model::Snapshot::outline_dirty", 9),
    ("model::Snapshot::labels_dirty", 9),
    ("model::Snapshot::header_footer_dirty", 9),
    ("model::Snapshot::annots_dirty", 9),
    ("model::Snapshot::fields_dirty", 9),
    ("model::Snapshot::removes_content", 9),
    ("model::Snapshot::form_changed", 9),
    ("model::state::DocState::annots_changed_from", 9),
    ("model::DocState::annots_changed_from", 9),
    ("prim::DocState::annots_changed_from", 9),
    ("model::state::DocState::fields_changed_from", 9),
    ("model::DocState::fields_changed_from", 9),
    ("prim::DocState::fields_changed_from", 9),
    ("model::state::DocState::outline_eq", 9),
    ("model::DocState::outline_eq", 9),
    ("prim::DocState::outline_eq", 9),
    ("model::state::DocState::labels_eq", 9),
    ("model::DocState::labels_eq", 9),
    ("prim::DocState::labels_eq", 9),
    ("model::state::DocState::keeps_annots_of", 9),
    ("model::DocState::keeps_annots_of", 9),
    ("prim::DocState::keeps_annots_of", 9),
    ("model::state::DocState::keeps_bookmarks_of", 9),
    ("model::DocState::keeps_bookmarks_of", 9),
    ("prim::DocState::keeps_bookmarks_of", 9),
    ("model::state::DocState::widgets_eq_on", 9),
    ("model::DocState::widgets_eq_on", 9),
    ("prim::DocState::widgets_eq_on", 9),
    ("model::Renames", 9),
    ("model::Snapshot::renamed", 9),
    ("model::Snapshot::renamed_on", 9),
    ("model::Document::renamed", 9),
    ("model::Document::set_source_renamed", 9),
    ("model::SourceInfo::stamp", 9),
    ("forms::rules::has", 9),
    // Moved to `materialize::plan`, which asks before a Rewrite breaks
    // them.
    ("save::signatures_broken", 9),
    // AES IVs keyed by the number an object is written at: an update's
    // section keys them by what it is.
    ("serialize::indirect", 10),
    ("serialize::crypt::Crypt::encrypt", 10),
    // A stamp in hex is the stamp's own (`FileStamp::hex`), which an
    // append's journal says it in too.
    ("recovery::stamp_hex", 10),
    ("recovery::parse_stamp", 10),
    // Every save a Rewrite, and a signed file's ask by a count: the
    // planner decides, and asks by each signature's impact.
    ("materialize::plan::Why::AlwaysRewrite", 10),
    ("materialize::plan::signatures_broken", 10),
    ("materialize::plan::Ask::Signatures::signatures", 10),
    // A restored document's file, and its being unsaved, are its record's:
    // the file's, with the recovery sidecar's sections pending.
    ("model::Document::disk_base", 10),
    ("model::Document::set_disk_base", 10),
    ("model::Document::mark_unsaved", 10),
    // An update frees nothing: an object it unlinks stays in use.
    ("serialize::xref::Entry::deleted", 10),
    // Ids and OCR text are sheaf-edit's types (M10.5 §6.1), which can't
    // hold what needs the model: a blob's id is the store's `blob_id`, and
    // a layer from an engine's words `text_layer::from_engine`.
    ("store::BlobId::of", 11),
    ("text_layer::TextLayer::from_engine", 11),
    // A source is its blob (M10.5 §3.1): nothing registers a file, and an
    // edit names its source by `BlobId`, the name it goes by beside it.
    // Each insert is a form instance of its own (§3.7), whose copy is
    // fixed when it's made, so an instance's objects never shift.
    ("model::SourceInfo", 11),
    ("model::Document::add_source", 11),
    ("model::Document::register_source", 11),
    ("model::Document::unregister", 11),
    ("model::Document::sources", 11),
    ("model::Document::source", 11),
    ("model::Document::next_source", 11),
    ("model::Document::unlogged_sources", 11),
    ("model::Document::debug_assert_ours", 11),
    ("model::Version::sources", 11),
    ("model::Snapshot::sources", 11),
    ("model::Snapshot::source", 11),
    ("model::PageBase::source", 11),
    ("model::Edit::InsertPages::range", 11),
    ("model::Edit::ReplacePages::range", 11),
    ("model::registered", 11),
    ("model::signed_from", 11),
    ("materialize::record::SaveInput::sources", 11),
    ("materialize::graph::load", 11),
    ("materialize::deps::Closure::form_shift", 11),
    ("materialize::append::Refused::FormShift", 11),
    ("materialize::key::FormCopies", 11),
    ("derive::DerivedSource::info", 11),
    ("combine::insert_runs", 11),
    ("model::Document::compute", 11),
    ("model::EditOutcome", 11),
    ("model::Document::form_notice", 11),
    ("model::Document::unencodable_notice", 11),
    ("model::Damaged", 11),
    ("model::NotReady", 11),
    ("model::Document::bring_annots", 11),
    ("model::Document::take_in_fields", 11),
    ("model::Document::copy_fields", 11),
    ("model::Document::script_sources", 11),
    ("model::Document::scripts_mixed", 11),
    ("model::Document::scripts_apart", 11),
    ("model::Document::set_author", 11),
    ("model::merge_outline", 11),
    ("model::cascade_delete", 11),
    ("model::name_apart", 11),
    ("model::next_copy", 11),
    ("model::Step::edit", 11),
    ("exchange::Imported::edits", 11),
    ("plugins::sign_marks", 11),
    ("plugins::keep_unseen", 11),
    ("plugins::Signer", 11),
    ("api::shared::edit_from_wit", 11),
    ("api::v02::annot_from_wit", 11),
    ("api::v03::annot_from_wit", 11),
    ("runtime::EditBatch::edits", 11),
    ("docview::Incoming", 11),
    ("docview::Incoming::edit_marks", 11),
    ("docview::DocView::start_annot_editor", 11),
    ("docview::DocView::pending_edit_last", 11),
    // Every change is an op lowered in a transaction (M10.5 §6), which
    // a caller commits: the old edits, and what only they used, are gone.
    ("model::Edit", 11),
    ("model::double_option", 11),
    ("model::Rebase", 11),
    ("model::NewOutlineItem", 11),
    ("model::Document::apply", 11),
    ("model::Document::apply_batch", 11),
    ("model::holds_same", 11),
    ("derive::DerivedSource::rebases", 11),
    // What compute's arms and the app's edit builders used: the ready
    // check is the transaction's (`EditError::NotReady`), and the
    // outline and mark helpers are lowering's, under their own names.
    ("model::Document::check_ready", 11),
    ("model::Document::tree_from_items", 11),
    ("model::Document::scripted", 11),
    ("model::outline_insert", 11),
    ("model::outline_unlink", 11),
    ("model::fresh_annot_id", 11),
    ("set_status::status_edits", 11),
    ("set_status::reset_edits", 11),
    ("docview::DocEvent::Edited::outcome", 11),
    // The job API: a group's commits amend its version, or continue it
    // after other edits (§8.4).
    ("model::JobId", 12),
    ("model::Version::job", 12),
    ("model::Document::next_job", 12),
    ("model::Document::begin_job", 12),
    ("model::Document::apply_in_job", 12),
    ("model::Document::job_is_current", 12),
    ("model::Document::job_is_undone", 12),
    ("tx::fixture::Document::commit_ops_in_job", 12),
    ("ocr_run::OcrRun::job", 12),
    // Pruning that kept the saved version and the branch holding it, the
    // root stopping there: the saved version detaches instead (§8.7).
    ("model::Document::prune", 12),
    ("model::Document::holds_protected", 12),
    ("model::Document::remove_subtree", 12),
    // History weighed in one pass, each version that keeps a snapshot
    // looked up once (`Document::weigh`).
    ("model::Document::droppable", 12),
    ("model::Document::stats_for", 12),
    // The app's funnels and refreshes: a view lowers apart from its one
    // commit (`DocView::commit`), and every change reaches `on_change`,
    // which lays the pages out again only when they go elsewhere (§14.1).
    ("docview::DocView::apply", 12),
    ("docview::DocView::apply_batch", 12),
    ("docview::DocView::submit", 12),
    ("docview::DocView::finish_edit", 12),
    ("docview::DocView::after_model_change", 12),
    ("docview::DocView::prune_selection", 12),
    ("docview::Doc::touched", 12),
    ("docview::Doc::version", 12),
    ("docview::Had", 12),
    ("docview::reveal_new_bookmarks", 12),
    ("page_insert::DocView::rebase", 12),
    // A plugin's own edits are told by the change's actor; a sidecar is
    // of a version, which never changes.
    ("docview::DocView::edit_source", 12),
    ("docview::DocView::edits", 12),
    ("docview::DocView::autosave_again", 12),
    // Planners and dialogs plan on a snapshot and commit on its version
    // (§14.1): the one-job-at-a-time runner was the planners' too, and
    // dialogs read the document as it was when they opened, not now.
    ("jobs::DocView::run_job_then", 12),
    ("page_insert::DocView::header_footer", 12),
    ("page_insert::DocView::has_page_labels", 12),
    // A plugin call reads the document from its cell at its first read,
    // and its batches land as one undo step (§14.3), not one transaction
    // each on the document as it is when the call returns.
    ("runtime::DocContext::snapshot", 12),
    ("plugins::plugin_tx", 12),
    // A thread and an engine per source, and its handle wherever a page was
    // drawn or read: one engine pool serves every engine (§10.3), and a
    // page is drawn from its view (`Views`). The impl of OCR's page source
    // for the handle goes with it, as `RenderHandle` names it.
    ("RenderHandle", 13),
    ("DocInfo", 13),
    ("SearchWalk", 13),
    ("worker_loop", 13),
    ("Msg::HideAnnots", 13),
    ("Msg::SetHidden", 13),
    ("job::OcrPage::source", 13),
    ("job::OcrPage::render", 13),
    ("render::PageRenderer::base", 13),
    ("print_dialog::PrintContext::handles", 13),
    ("plugins::EngineText::handle", 13),
    ("page_insert::PlannedInsert::handle", 13),
    ("page_insert::with_engines", 13),
    ("page_insert::Copies", 13),
    ("docview::Doc::handles", 13),
    ("docview::Doc::primary", 13),
    ("engines_on", 13),
    // Hiding by replaying it into each engine: what a page draws is data,
    // `ViewInputs.hidden` and `ocg_off` (§10.4), each consumer passing its
    // own, and the layers panel's choice a patch of the catalog, not a
    // decrypted copy written again.
    ("Renderer::hide_annots", 13),
    ("Renderer::set_annots_hidden", 13),
    ("pdfium::PdfiumEngine::hide_annots", 13),
    ("pdfium::PdfiumEngine::set_annots_hidden", 13),
    ("docview::Doc::take_in", 13),
    ("docview::Doc::drawn_marks", 13),
    ("docview::Doc::foreign_hidden", 13),
    ("docview::Doc::layers_generation", 13),
    ("docview::drawn_marks", 13),
    ("docview::marks_by_page", 13),
    ("docview::hide_renamed", 13),
    ("comments_panel::DocView::sync_foreign_visibility", 13),
    ("layers::set_hidden", 13),
    // The header and footer drawn by GPUI over each page: the realizer
    // draws it into the page's overlay.
    ("docview::header_footer_elements", 13),
    // Images dropped on a change and tiles invalidated for the whole
    // document: images are found by render key (§10.2), and a page keeps
    // its last drawn until its new one lands. The pane's layout holds no
    // source page, so a page shown from another source page lays nothing
    // out again.
    ("docview::Doc::drop_stale_images", 13),
    ("docview::Doc::invalidate_tiles", 13),
    ("docview::Doc::tiles_invalidated", 13),
    ("docview::Doc::shown", 13),
    ("pane::Shown::source", 13),
    ("pane::Shown::index", 13),
    ("pane::Shown::render", 13),
    // The emitter's save input: it reads the state, and a save's primary
    // and derived copies come with its base (§11.1).
    ("materialize::emit::Cx::input", 13),
    // What a render answered names its page and job: its request's format
    // and priority were never read (the review of slice 13).
    ("RenderRequest::format", 13),
    ("RenderRequest::priority", 13),
    // The app's own cache of a page's characters, keyed by its base
    // engine's page or its unsaved OCR layer: a page's text comes from the
    // text service (§10.5), read from its realization under the screen's
    // inputs and cached by render key.
    ("text_select::CharsKey", 14),
    ("text_select::CharsKey::Engine", 14),
    ("text_select::CharsKey::Layer", 14),
    ("text_select::CACHED_PAGES", 14),
    ("text_select::DocView::chars_key", 14),
    ("text_select::DocView::cache_chars", 14),
    ("docview::Doc::page_chars", 14),
    ("pool::EnginePool::page_chars", 14),
    ("page_chars::PageChars::from_layer", 14),
    // Find and Patterns over the primary's base engine by source index,
    // unsaved OCR layers searched beside it and merged in: a search walks
    // the document's order, each page read from its view's engine
    // (`Text::search` on `EnginePool::walk`). The new walk keeps the name
    // `Walk`, so only the old one's fields are rows.
    ("Matcher", 14),
    ("SearchEvent", 14),
    ("pool::Msg::Search", 14),
    ("pool::EnginePool::search", 14),
    ("pool::Walk::target", 14),
    ("pool::Walk::matcher", 14),
    ("pool::Worker::chunk", 14),
    ("docview::Doc::search", 14),
    ("docview::Doc::index_of_source_page", 14),
    ("docview::Doc::layer_hits", 14),
    ("docview::Doc::merge_source_hits", 14),
    // An unsaved OCR layer's own search, text and boxes: the realizer draws
    // the layer into its page, so its words are read as the page's. A hit
    // is `PageChars::find`'s now, in `page_chars`.
    ("text_layer::search", 14),
    ("text_layer::pattern_search", 14),
    ("text_layer::TextHit", 14),
    ("text_layer::PageText", 14),
    ("text_layer::page_text", 14),
    ("text_layer::fold", 14),
    ("text_layer::hit_from_span", 14),
    ("text_layer::line_of", 14),
    ("text_layer::unrotate_from_upright", 14),
    ("text_layer::TextLayer::text", 14),
    ("text_layer::TextLayer::boxes_in_rect", 14),
    // PDFium's text in the displayed frame through a 16× device, and the
    // engine's own search: characters come in PageSpace with their angles
    // (§5), and `PageChars::find` matches them. `sheaf_render::Rect` has
    // no row: a one-segment row matches a path's first segment, and
    // `geom::Rect::new` is called as `Rect::new`.
    ("SearchHit", 14),
    ("Renderer::search_page", 14),
    ("Renderer::pattern_search_page", 14),
    ("Renderer::text_boxes_in_rect", 14),
    ("pdfium::PdfiumEngine::search_page", 14),
    ("pdfium::PdfiumEngine::pattern_search_page", 14),
    ("pdfium::PdfiumEngine::text_boxes_in_rect", 14),
    ("pool::EnginePool::search_page", 14),
    ("pool::EnginePool::pattern_search_page", 14),
    ("pool::EnginePool::text_boxes_in_rect", 14),
    ("pool::EnginePool::page_text", 14),
    ("pool::EnginePool::page_text_len", 14),
    // A plugin's text from its source page's base engine, with OCR's
    // special cases beside it: plugins read the text service under their
    // own inputs, as the file will draw the page (§14.3).
    ("runtime::TextSource", 14),
    ("runtime::TextMatch", 14),
    ("api::shared::layer_matches", 14),
    ("api::shared::text_match_to_wit", 14),
    ("plugins::EngineText", 14),
    ("plugins::EngineText::hits", 14),
    ("plugins::to_box", 14),
    // A file opened by its path, print's last: the print job goes to GDI
    // and to CUPS from memory (§10.6), so PDFium opens only bytes.
    ("pdfium::PdfiumEngine::open", 14),
    // Crop's derived file: a crop is a box edit of the page's `Geom`
    // (`Op::SetPageBoxes`), so no copy is written, interned or opened, and
    // Trim White is a resolve stage that measures through the views
    // (`crop::trim_white`), not an engine of its own. Crop's `MIN_SIDE`
    // moved to `geom::MIN_SIDE`, so it has no row. The dialog takes the
    // page's size from its preview.
    ("save::cropped_copy", 15),
    ("crop::CroppedSource", 15),
    ("crop::CropSpec", 15),
    ("crop::CropSpec::Margins", 15),
    ("crop::CropSpec::TrimWhite", 15),
    ("crop::Margins", 15),
    ("crop::plan", 15),
    ("crop::content_margins", 15),
    ("engine_for", 15),
    ("tx::resolve::Derive::Crop", 15),
    ("crop_dialog::CropEvent::Crop", 15),
    ("crop_dialog::CropContext::size", 15),
    // The watermark and background as a derived copy with a mark drawn on
    // its pages: they are generators (`GenSpec::Watermark`), set and taken
    // off by ops (`SetWatermark`, `RemoveWatermark`) and drawn at emit
    // (`generate::layout`), so nothing writes `SheafSettings` or the
    // `/SheafWM*` names, nothing reads a watermark back from a page, and
    // no copy keeps a header for one. Remove Header/Footer keeps
    // `stamps::strip` for its own kinds until slice 17. The dialog starts
    // from the generator at its z; `crop_dialog::why` moved to
    // `dialog_preview::why`, so it has no row.
    ("derive::watermark", 16),
    ("derive::current_watermark", 16),
    ("derive::has_kinds", 16),
    ("watermark::Watermark", 16),
    ("watermark::Mark", 16),
    ("watermark::Anchor", 16),
    ("watermark::FONT", 16),
    ("watermark::install", 16),
    ("watermark::read", 16),
    ("tx::resolve::Derive::Watermark", 16),
    ("tx::resolve::Derive::RemoveWatermark", 16),
    ("save::HeaderKept", 16),
    ("stamps::WATERMARK", 16),
    ("stamps::BACKGROUND", 16),
    ("page_insert::DocView::apply_watermark", 16),
    ("workspace::Workspace::open_watermark", 16),
    ("watermark_dialog::WatermarkContext::behind", 16),
    ("watermark_dialog::WatermarkContext::current", 16),
    // The header and footer as a legacy installer with a catalog record of
    // its own: it is a generator (`GenSpec::HeaderFooter`), set and taken
    // off by ops, its Bates numbers fixed at apply (`Frozen`) and kept in
    // its blocks' `/N`, drawn at emit as `/SheafBlock` blocks and found
    // again from `/SheafGenerators`, so nothing writes or reads
    // `/SheafHeaderFooter`, counts Bates numbers at each save, or strips a
    // header by hand. Its tokens and lines moved to `generate::header`
    // (`TOKENS`, `Line` for `Placed`), and `Generator`'s `z`, `scope` and
    // `replaces` are its fields now, so they have no row; nor has
    // `GenSpec::same_place`, which is `Generator::same_place`.
    ("stamps::HeaderFooter", 17),
    ("stamps::Bates", 17),
    ("stamps::TOKENS", 17),
    ("stamps::PageFacts", 17),
    ("stamps::Placed", 17),
    ("stamps::Shared", 17),
    ("stamps::FONT", 17),
    ("stamps::FONT_NAME", 17),
    ("stamps::to_content", 17),
    ("stamps::install", 17),
    ("stamps::drawn_over", 17),
    ("stamps::base_draws", 17),
    ("stamps::HEADER_FOOTER", 17),
    ("stamps::stripped", 17),
    ("stamps::strip", 17),
    ("stamps::CATALOG_KEY", 17),
    ("stamps::to_catalog", 17),
    ("stamps::stored", 17),
    ("stamps::from_catalog", 17),
    ("source::Source::header_footer", 17),
    ("source::StoredHeaderFooter", 17),
    ("open::Facts::header_footer", 17),
    ("model::state::DocState::header_covers", 17),
    ("model::state::DocState::bates_of", 17),
    ("model::state::DocState::header_pages", 17),
    ("model::state::DocState::number_header_pages", 17),
    ("model::state::Indexes::header", 17),
    ("model::records::REPLACES_NONE", 17),
    ("model::records::CtxNeeds::any", 17),
    ("materialize::emit::Cx::bates", 17),
    ("materialize::emit::content::header_footer", 17),
    ("materialize::removal::watermarks_say", 17),
    ("materialize::deps::is_watermark", 17),
    ("materialize::deps::Deps::watermark", 17),
    ("materialize::plan::Why::LegacyHeaderFooter", 17),
    ("tx::content::narrow_watermarks", 17),
    ("header_footer_dialog::today", 17),
    ("header_footer_dialog::HeaderFooterContext::current", 17),
    ("header_footer_dialog::HeaderFooterContext::file", 17),
    ("header_footer_dialog::HeaderFooterContext::scope", 17),
    ("page_insert::header_footer_op", 17),
    ("page_insert::watermark_notice", 17),
    ("page_insert::DocView::indexes_of", 17),
    ("workspace::Workspace::open_header_footer", 17),
    // Derived sources: no flow makes a copy of a page any more, so there's
    // no copy to record, keep in the store, show in place of its page,
    // emit as the page it was made from, or rebase a page onto. The Remove
    // Header/Footer flow that made the last of them is an op
    // (`RemoveHeaderFooter`, by origin). `tx::Entry` is a struct of its
    // old `Op` variant's fields now.
    ("derive", 17),
    ("derive::DerivedSource", 17),
    ("derive::DerivedSource::note", 17),
    ("derive::DerivedSource::bases", 17),
    ("derive::copy_of", 17),
    ("derive::has_header", 17),
    ("derive::strip_stamps", 17),
    ("derive::strip_stamps_of", 17),
    ("by_source", 17),
    ("save::derived_copy", 17),
    ("save::copy_form", 17),
    ("model::Derived", 17),
    ("model::DerivedCopies", 17),
    ("model::Document::derived", 17),
    ("model::Document::add_derived", 17),
    ("model::Snapshot::derived", 17),
    ("materialize::record::SaveInput::derived", 17),
    ("store::MemoryKind::Derived", 17),
    ("tx::resolve::Derive", 17),
    ("tx::resolve::Derive::StripHeaderFooter", 17),
    ("tx::resolve::ResolvedOp::SetPageBase", 17),
    ("tx::Tx::set_page_base", 17),
    ("tx::Entry::Base", 17),
    ("tx::Entry::Op", 17),
    ("tx::pages::set_base", 17),
    ("tx::fixture::commit_page_base", 17),
    ("describe::derived", 17),
    ("materialize::emit::EmitBase::Rewrite::derived", 17),
    ("materialize::emit::EmitBase::Saved::derived", 17),
    ("materialize::emit::Shown::content", 17),
    ("materialize::emit::origin", 17),
    ("materialize::emit::annots::derived", 17),
    ("materialize::graph::Loaded", 17),
    ("materialize::graph::Loaded::from_pdf", 17),
    ("materialize::copy::INHERITED", 17),
    ("materialize::copy::inherited_attrs", 17),
    ("materialize::copy::reserve", 17),
    ("file::not_ours", 17),
    ("page_insert::DocView::show_copies", 17),
    ("page_insert::DocView::keep_copies", 17),
    // OCR as content: recognized text is a layer with a block id, drawn as
    // a `/SheafBlock` block of its own after the page's isolated content
    // (`text_layer::block`), its font and list under `/Sh…` names, and a
    // saved one is taken out by the stack's filters. The legacy installer
    // (`/SheafOcrLayer`, `/SheafOcr`), the emitter's balance around it, and
    // the sink's methods only it used are gone.
    ("model::ContentStack::with_ocr_text", 18),
    ("model::records::ContentStack::with_ocr_text", 18),
    ("text_layer::install", 18),
    ("text_layer::strip", 18),
    ("text_layer::has_layer", 18),
    ("text_layer::layer_entries", 18),
    ("text_layer::to_content", 18),
    ("text_layer::MARKER", 18),
    ("text_layer::FONT_NAME", 18),
    ("materialize::emit::content::fresh_balance", 18),
    ("content::sink::Sink::add", 18),
    ("content::sink::Sink::stream_dict", 18),
    // The plugin API's OCR override, the layer a page's stack holds:
    // recognized text is the page's content, saved or not, which `text`
    // reads, and the API has none.
    ("api::shared::HostState::page_text_layer", 18),
    // Memory sources: blank pages, images as pages and library stamps are
    // PDFs made in memory and interned as blobs, written by sheaf's
    // serializer (`memory`, `serialize::full::write_built`); nothing is
    // written to a cache, and a stamp nothing holds isn't found. The
    // generated cache, the stems it gave by path, combine's image file in
    // it and `find_stamp`'s cache arm are gone.
    ("generated", 19),
    ("generated::cache_dir", 19),
    ("generated::store", 19),
    ("generated::store_in", 19),
    ("generated::name_of", 19),
    ("generated::is_stem", 19),
    ("generated::path_of", 19),
    ("generated::stem_of", 19),
    ("generated::write_new", 19),
    ("generated::blank_pdf", 19),
    ("generated::blank_pages", 19),
    ("images::PageFit", 19),
    ("images::files_to_pdf", 19),
    ("model::held_stamp", 19),
    ("combine::page_source", 19),
    ("page_insert::DocView::insert_made", 19),
    // Private data: under `Private::Strip` a file holds none of
    // `private::KEYS`, legacy names included (an inline list loses
    // `private::LIST_KEYS`, derived from them); a page's markers, lists
    // and names of sheaf's go through its filters,
    // `RemovedKind::PrivateData`.
    ("model::LEGACY_KEYS", 19),
    ("effects::RemovedKind::OlderSettings", 19),
    // The old exports: every way out is `export::run`, an `ExportTx` of
    // transforms (`KeepPages`, `RemoveMarkup`, `RemovePrivateData`,
    // `FlattenAnnotations`) written by `materialize` with the save's
    // checks, and the presets name them. `subset`'s fake snapshot, the
    // post-pass on a Rewrite's graph (`flatten_annots`, `/SheafFlat1`,
    // the widget first-state fallback) and the markup strip are gone,
    // with the app's methods named after them.
    ("save::subset", 19),
    ("save::paired", 19),
    ("save::form_on", 19),
    ("save::prune_outline", 19),
    ("save::print_pdf", 19),
    ("save::extract_pages", 19),
    ("save::extract_bytes", 19),
    ("save::export_clean_copy", 19),
    ("save::export_flattened", 19),
    ("save::flatten_annots", 19),
    ("save::normal_appearance", 19),
    ("save::strip_markup", 19),
    ("save::strip_markup_annots", 19),
    ("save::is_link_or_field", 19),
    ("save::without_markup", 19),
    ("save::EXPORT", 19),
    ("save::CLEAN", 19),
    ("save::written", 19),
    ("save::is_the_file", 19),
    ("materialize::PostPass", 19),
    ("materialize::materialize_then", 19),
    ("docview::DocView::ask_before_saving", 19),
    ("docview::DocView::export_flattened_to", 19),
    ("docview::DocView::export_flattened_prompt", 19),
    ("docview::DocView::export_clean_copy_prompt", 19),
    ("print_dialog::PrintContext::snapshot", 19),
    // Every subcommand runs on a `Session`, which saves through
    // `materialize::save_with`, the save core; scan, the summary and blank
    // pages make their pages in memory, and nothing is opened or saved by
    // hand.
    ("materialize::save", 19),
    ("scan::add_text", 19),
    ("comments::summary_pdf", 19),
    // A page past its source's pages can't be: an open makes a page of
    // each page the source has, and an edit's of what the store holds.
    // The check of it had no caller.
    ("save::validate", 19),
    // One plugin world, `sheaf:plugin@0.4.0` (M10.5 §14.3): WIT 0.2 and
    // 0.3, their two bindings, the transitional adapter that said their
    // edits as ops, a plugin's world chosen per plugin, and the API before
    // the current one are gone. A call's edits are Op JSON lowered into
    // its one transaction, which lands as the call's group
    // (`sheaf_plugin::Landing`): no batches, no stems held for the host to
    // find (a plugin uploads its assets), no reads kept beside them.
    ("api::v02", 20),
    ("api::v03", 20),
    ("api::bindings_v02", 20),
    ("api::bindings_v03", 20),
    ("api::shared::Adapter", 20),
    ("api::shared::Said", 20),
    ("api::shared::More", 20),
    ("api::shared::page_frame", 20),
    ("api::shared::to_frame", 20),
    ("api::shared::dest_from_wit", 20),
    ("api::shared::rotation_from_wit", 20),
    ("api::shared::pt_from_wit", 20),
    ("api::shared::rgb_from_wit", 20),
    ("API_PREVIOUS", 20),
    ("runtime::World", 20),
    ("runtime::EditBatch", 20),
    ("runtime::Held", 20),
    ("runtime::CallOutput::batches", 20),
    ("runtime::CallOutput::reads", 20),
    ("runtime::CallOutput::base", 20),
    ("runtime::HostState::batches", 20),
    ("runtime::HostState::reads", 20),
    ("runtime::HostState::read", 20),
    ("runtime::HostState::page_base", 20),
    ("plugins::land", 20),
    ("plugins::Landed", 20),
    ("plugins::edited_label", 20),
    ("batch::Batch", 20),
    ("batch::annot", 20),
    ("batch::at", 20),
    ("batch::BLUE", 20),
    // A plugin is told what a change changed (`changed`, its scopes), not
    // its undo label.
    ("runtime::DocEvent::Edited", 20),
    ("manifest::EventKind::Edited", 20),
    // The app holds back a busy plugin's regions, latest wins, and its
    // changes, merged, in one kind of queue (`plugins::Coalescer`), and
    // sends what waited when a call from either ends (`queue_done`).
    ("plugins::Latest", 20),
    ("plugins::region_done", 20),
    // PDFs sheaf builds in memory are built by `serialize::built`: the
    // resources their content draws with (`ResObjects`), marks as the
    // writer makes them (`WriteCx`, `Held`, `StampForm`, and the tests'
    // `to_pdf`), and pages (`Built`), each written by `write_built`; the
    // marks' module builds dictionaries and content alone, and reads the
    // digest of what it wrote with no file. Exchange's comments are a
    // source, read as any PDF's are, not a document loaded whole.
    ("content::to_lopdf", 20),
    ("content::ResObjects", 20),
    ("content::sink::Lopdf", 20),
    ("annot::WriteCx", 20),
    ("annot::StampForm", 20),
    ("annot::Held", 20),
    ("annot::Written", 20),
    ("annot::to_pdf", 20),
    ("annot::to_pdf_in", 20),
    ("annot::NoObjects", 20),
    ("annot::Appearance::write", 20),
    ("materialize::copy::copy_page_dict", 20),
    ("exchange::Comments::pdf", 20),
    ("exchange::Comments::new", 20),
    // Model-level code names no lopdf `ObjectId` (rule 7,
    // tests/object_ids.rs): a bookmark as read from a file is the open's
    // fact (`open::FlatOutlineItem`), and a flatten reads what an
    // annotation shows through its source (`source::appearance`).
    ("model::FlatOutlineItem", 20),
    ("tx::flatten::Shows", 20),
    ("tx::flatten::read", 20),
    ("tx::flatten::shown", 20),
    ("tx::flatten::frame", 20),
    ("tx::flatten::unkept", 20),
];

/// Every name M5 retired, with the slice of `todos/m5-intake.md` that
/// retired it.
const RETIRED_M5: &[(&str, u8)] = &[
    // A scan is `intake` over the files' frames, a page at a time: the
    // file-at-a-time cleanup had no caller left.
    ("scan::clean_one", 1),
    // A black and white page is held as its CCITT G4 bytes from the
    // moment it's made, not as 8-bit pixels until it's written.
    ("io::Pixels::Bilevel", 1),
];

/// The ledgers, with the words a hit names its slice by.
const LEDGERS: &[(&[(&str, u8)], &str)] = &[(RETIRED, "slice"), (RETIRED_M5, "M5 slice")];

/// Last segments only a path matches: names the new design uses again
/// (§17), and words common enough to name other things.
const QUALIFIED_ONLY: &[&str] = &[
    // A print, export or render request's job, and the app's file-writing
    // job: not the version's.
    "job",
    // The command palette's entries pruned (commands.rs): not history's.
    "prune",
    // How many snapshots the policy may drop (`HistoryStats`), and the
    // locals that hold them.
    "droppable",
    // The header and footer a document's state draws: not the view's.
    "header_footer",
    "Mode",
    "deleted",
    "Step",
    "PageRef",
    "LabelSpec",
    "Widget",
    "Ids",
    "ids",
    "rng",
    "next_page",
    "next_outline",
    "next_field",
    "data",
    "bytes",
    "page_count",
    "first_page",
    "password",
    "page_size",
    "open",
    "linearized",
    "path",
    "outline",
    "Outline",
    "OutlineItem",
    "describe",
    "page_labels",
    "shift",
    "page",
    "frame",
    "frames",
    "rotation",
    "size",
    "PageSize",
    "source",
    "Sheet",
    "wrap",
    "width",
    "contains",
    "objects",
    "runs",
    "encrypted",
    "index_of",
    "text_layers",
    "removed_comments",
    "widgets",
    "forms",
    "assets",
    "marks",
    "foreign",
    "renamed",
    "form",
    "TextLayer",
    "HeaderFooter",
    "Stamp",
    "File",
    "locate",
    "missing",
    "Pages",
    "Annots",
    "incoming",
    "has_scripts",
    "Saved",
    "SaveStop",
    "Ask",
    "inherited_attrs",
    "copy_page_dict",
    "is_page_node",
    "copy_object",
    "remap",
    "stream",
    "finish",
    "write",
    "materialize",
    "append",
    "Strategy",
    "Signed",
    "copy_of",
    "origin",
    "origins",
    "needed",
    "copy_annots",
    "annots_on",
    "merge",
    "fields_of",
    "pdf",
    "own",
    "of",
    "copy",
    "under",
    "baseline",
    "form_changed",
    "stamp",
    "signatures_broken",
    "has",
    // What a save's path removed (§9.2's `Removed`, and its kinds'
    // `Widgets`), not slice 8's removed slot value and widgets' slot.
    "Removed",
    "Widgets",
    // `serialize::object::indirect` writes an object's frame, and
    // lopdf's `Document::encrypt` encrypts a whole file.
    "indirect",
    "encrypt",
    // `Source::signatures` reads them; `Ask::Signatures`' count is gone.
    "signatures",
    // `PageChars::from_engine`, and the free function in its place.
    "from_engine",
    // The OCR job's and the store's sources, and fields of that name.
    "sources",
    // Other things of these names: ranges of every kind, an engine's
    // `info`, a file's `load`, a command's being `registered`.
    "range",
    "info",
    "load",
    "registered",
    // Another tool's edit box (stamps_panel.rs's `edit`), a form field
    // tree's damage (`Finding::Damaged`), and the error a refused edit
    // became (`EditError::NotReady`).
    "edit",
    "Damaged",
    "NotReady",
    // A version's lines (`VersionView::edits`), and the app's count of
    // them.
    "edits",
    // The History panel's rows of a version's lines (`Row::Edit`) and
    // `sheaf edit` (`Command::Edit`), and what a transaction and the app's
    // document view apply.
    "Edit",
    "apply",
    "apply_batch",
    // A plugin form's own submit and a print job's, Rec's touched keys and
    // a form's, and anything's version.
    "submit",
    "touched",
    "version",
    // Every other snapshot: the document's, a dialog's, a tracker's.
    "snapshot",
    // The file dialog's scripted answer (`file_dialog::scripted`), and
    // what anything else comes out as.
    "scripted",
    "outcome",
    // Other handles and the handles a markup kind draws, the OCR run's job
    // and a request's handler: not a source's render handle.
    "handle",
    "handles",
    // A document's primary source (`Document::primary`), and a primary
    // button.
    "primary",
    // An insert's form taken in (`tx::forms::take_in`).
    "take_in",
    // What every other plan and layout shows, and indices, renders and
    // bases of every other kind: a page's index in its source, a view's
    // in its engine, a page's base.
    "shown",
    "index",
    "render",
    "base",
    // Every text input, and a save's input.
    "input",
    // A pool request's, a raster's and every other format and priority:
    // not a render's answer's.
    "format",
    "priority",
    // Every other text, hit, target and search: a word's and a field's
    // text, Find's and the pattern panel's hits, every pool request's
    // target, `Text::search`, the search bar (`Search`) and the plugin
    // API's search and pattern search.
    "text",
    "hits",
    "target",
    "search",
    "Search",
    "pattern_search",
    // A `TextHit` is `PageChars::find`'s now, in `page_chars`; a
    // `TextMatch` the WIT's.
    "TextHit",
    "TextMatch",
    // PDFium's text through `Renderer` (`page_text`, `page_text_len`,
    // `page_chars`), the plugin API's `page_text` and the `page_chars`
    // module.
    "page_text",
    "page_text_len",
    "page_chars",
    // Other enums' engines and layers (OCR's, the layers panel's), the
    // realizer's chunks, `Iterator::fold`, and prefs.rs's `line_of`.
    "Engine",
    "Layer",
    "chunk",
    "fold",
    "line_of",
    // A save's plan (`materialize::plan`), split's (`split::plan`), the
    // form rules' (`forms::rules::plan`) and the Insert and Page Labels
    // dialogs' `plan`: not crop's.
    "plan",
    // The margins a crop or a header takes off a page (`sheaf_edit::Margins`,
    // and the variants `BoxSpec::Margins`, `CropEvent::Margins` and the
    // Crop dialog's `Request::Margins` that carry them): not sheaf-ops'
    // own or `CropSpec`'s.
    "Margins",
    // describe's `Boxes::Crop`, a `SetPageBoxes` that sets the crop alone:
    // not a derived flow's or the dialog's event.
    "Crop",
    // The generator's own: `PaginationKind::Watermark` and `GenSpec::
    // Watermark`, `Op::RemoveWatermark` and `ResolvedOp::RemoveWatermark`,
    // `DocState::watermark`, `GenSpec::watermark` and the fixtures'
    // `watermark`: not the old module's, `Derive`'s or derive.rs's.
    "Watermark",
    "RemoveWatermark",
    "watermark",
    // A mark of sheaf's (`model::Mark`), where a note's mark is anchored
    // (`sheaf_edit::Anchor`), the plugin tests' installer and the prompts'
    // (`sheaf_ui::prompts::install`), and every other read: not the old
    // watermark module's, nor the header's or the OCR layer's installers.
    "Mark",
    "Anchor",
    "install",
    "read",
    // Every other current thing: a theme's, a list's, the instance's.
    "current",
    // The isolation the emitter shares between pages (`content::Shared`),
    // a placed image or page (`ImageRef::Placed`, `PageForm::Placed`), a
    // source's page facts (`source::PageFacts`), and every other strip,
    // stripped page, stored entry and base that draws a page (a test's
    // `strip`, `/SheafGenerators`' `stored`, a render key's
    // `base_draws`). Nothing else is named `to_content`, so the header's
    // and the OCR layer's are matched bare.
    "Shared",
    "Placed",
    "PageFacts",
    "strip",
    "stripped",
    "stored",
    "base_draws",
    // Every other header, file, scope, content, check for anything and
    // Bates spec: a fixture's header, the dialogs' and ops' files and
    // scopes, a page's content, `Iterator::any`, `HeaderFooterSpec::bates`.
    "header",
    "file",
    "scope",
    "content",
    "any",
    "bates",
    // What else is derived (the removal check's `Found::derived` and
    // `Vetoes::derived`), a note of every other kind, the bases pages
    // show, a page's `Base` and every `Op`, the app's toolbar `Loaded`, the
    // exchange's `from_pdf` and `Vec::reserve`.
    "derived",
    "note",
    "bases",
    "Base",
    "Op",
    "Loaded",
    "from_pdf",
    "reserve",
    // Every other add: the fixtures' `tx::fixture::add` of a mark, a
    // builder's, a set's: not the sink's object added for the OCR layer's
    // installer.
    "add",
    // The document's store (`Document::store`, the `store` module), the
    // blank pages made in memory (`memory::blank_pages`) and the tests'
    // own blank files, a key's path (`model`'s tests), an export's file
    // stem (`sheaf_ops::export::stem_of`), a name for a thing (`name_of`)
    // and the realizer's generated content (`untouched::generated`): not
    // the generated cache's.
    "store",
    "blank_pages",
    "blank_pdf",
    "path_of",
    "stem_of",
    "name_of",
    "generated",
    // What a save or a test wrote (`append_tests::written`, the
    // materializer's `Written`), and a font's subset (`removal_tests`):
    // not the old exports' state and writer.
    "written",
    "subset",
    // A session's save (`sheaf_ops::session::Session::save`), the app's,
    // a test's `How::save` and lopdf's: not `materialize::save`, which
    // `save_with` replaced.
    "save",
    // A plugin manifest's check (`Manifest::validate`), a stale
    // transaction's (`Document::validate`, for `Stale::Validate`) and a
    // form's count of its validate scripts (`ScriptCounts::validate`): not
    // the save module's page check.
    "validate",
    // A plugin's transaction on what the document published to its calls
    // (`Published::plugin_tx`, M10.5 §14.3): not the app's, which made
    // one on the document as a call returned.
    "plugin_tx",
    // The plugin API's one world (`api::World`), not the choice of two; a
    // mark's held asset (`serialize::built::Held`) and private data's name
    // a walk found (`private::Said`), not the adapter's or the marks'
    // module's; a call's landing (`sheaf_plugin::Landed`, `Landing`), not
    // the app's batch loop's.
    "World",
    "Held",
    "Said",
    "Landed",
    "land",
    // A mark turned to the page it was copied from (`Annot::to_frame`),
    // not a page's API frame; what a lowering read (`Lower::reads`), not
    // a call's reads beside its batches.
    "to_frame",
    "reads",
    // A bookmark position, sheaf's blue and an annotation of every other
    // kind: the plugin API's own (`ops::at`, `ops::BLUE`) and every
    // `annot`, not `batch`'s.
    "at",
    "BLUE",
    "annot",
    // A plugin's render option (`render-options.without-markup`), not the
    // old save's.
    "without_markup",
    // What builds a PDF in memory (`serialize::built`'s `ResObjects`,
    // `WriteCx`, `StampForm`, `Written`, `to_pdf` and `to_pdf_in`), a
    // form's page as a one-page PDF (`FormRef::to_pdf`), and every type's
    // constructor: not the content toolkit's, the marks' module's or
    // exchange's.
    "ResObjects",
    "WriteCx",
    "StampForm",
    "Written",
    "to_pdf",
    "to_pdf_in",
    "new",
    // A bookmark as read from a file (`open::FlatOutlineItem`), and what
    // an annotation shows read through its source (`source::appearance`'s
    // `Shows` and `unkept`): not the model's or lowering's.
    "FlatOutlineItem",
    "Shows",
    "unkept",
    // A black and white page of a TIFF being written
    // (`encode::TiffPage::Bilevel`): not a page held as 8-bit pixels.
    "Bilevel",
];

#[test]
fn nothing_retired_is_back() {
    let root = workspace();
    let mut files = Vec::new();
    rust_files(&root.join("crates"), &mut files);
    assert!(
        files.iter().any(|f| f.ends_with("sheaf-doc/src/model.rs")),
        "the walk found the model"
    );
    let mut hits = Vec::new();
    for file in &files {
        let shown = file.strip_prefix(&root).unwrap().display();
        let src = std::fs::read_to_string(file).unwrap();
        let parsed = syn::parse_file(&src).unwrap_or_else(|e| panic!("{shown}: {e}"));
        for &(rows, slices) in LEDGERS {
            for (line, row, slice) in scan_file(&parsed, module_of(file), rows) {
                hits.push(format!(
                    "{shown}:{line}: {row} (retired in {slices} {slice})"
                ));
            }
        }
    }
    assert!(
        hits.is_empty(),
        "retired names are back:\n{}",
        hits.join("\n")
    );
}

#[test]
fn the_ledger_matches_definitions_calls_and_paths() {
    const ROWS: &[(&str, u8)] = &[
        ("model::Ids", 1),
        ("model::Document::replay_step", 1),
        ("model::Document::marginal", 1),
        ("model::Document::rng", 1),
        ("model::Version::marginal_bytes", 1),
        ("incremental::Mode", 9),
    ];
    let src = r#"
use crate::incremental::Mode;
struct Ids;
struct Version { marginal_bytes: usize }
impl Document {
    fn marginal(&self) {}
    fn undo(&mut self, mode: Mode) {
        self.replay_step();
        assert_eq!(self.marginal(), ());
        let marginal_bytes = self.rng;
        let _ = (marginal_bytes, sheaf_doc::incremental::Mode::Append);
    }
}
mod tests {
    struct Ids;
}
"#;
    let hits = scan(src, vec!["model".into()], ROWS).unwrap();
    // Not line 7's bare `Mode` or line 10's `rng` (qualified only), line
    // 10's and 11's local `marginal_bytes`, or line 15's `tests::Ids`.
    assert_eq!(
        hits,
        [
            (2, "incremental::Mode", 9),
            (3, "model::Ids", 1),
            (4, "model::Version::marginal_bytes", 1),
            (6, "model::Document::marginal", 1),
            (8, "model::Document::replay_step", 1),
            (9, "model::Document::marginal", 1),
            (11, "incremental::Mode", 9),
        ]
    );
}

#[test]
fn a_variant_s_field_is_named_from_its_enum() {
    const ROWS: &[(&str, u8)] = &[("plan::Ask::Signatures::count", 10)];
    let src = r#"
enum Ask {
    Signatures { count: usize },
    Other { count: usize },
}
"#;
    let hits = scan(src, vec!["plan".into()], ROWS).unwrap();
    // Not line 4's, another variant's.
    assert_eq!(hits, [(3, "plan::Ask::Signatures::count", 10)]);
}

#[test]
fn a_root_item_is_named_from_its_root() {
    const ROWS: &[(&str, u8)] = &[("OutlineItem", 4)];
    let src = r#"
use sheaf_render::OutlineItem;
fn read(item: w::OutlineItem) -> api::types::OutlineItem {
    let _ = crate::OutlineItem::default();
    w::OutlineItem { depth: 0 }
}
"#;
    let hits = scan(src, Vec::new(), ROWS).unwrap();
    // Not lines 3's and 5's, other modules' `OutlineItem`s.
    assert_eq!(hits, [(2, "OutlineItem", 4), (4, "OutlineItem", 4)]);
}

#[test]
fn a_file_s_module_comes_from_its_path() {
    let crates = workspace().join("crates");
    for (file, module) in [
        ("sheaf-doc/src/lib.rs", ""),
        ("sheaf-doc/src/model.rs", "model"),
        (
            "sheaf-doc/src/model/history_tests.rs",
            "model::history_tests",
        ),
        ("sheaf-doc/src/forms/mod.rs", "forms"),
        ("sheaf-doc/tests/retired.rs", ""),
        ("sheaf-ops/tests/common/mod.rs", "common"),
        ("sheaf-cli/src/bin/sheaf-cli.rs", ""),
        ("sheaf-app/build.rs", ""),
        ("sheaf-plugin/tests/fixtures/test-plugin/src/lib.rs", ""),
    ] {
        assert_eq!(module_of(&crates.join(file)).join("::"), module, "{file}");
    }
}

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/sheaf-doc is two below the workspace")
        .to_path_buf()
}

/// The `.rs` files under `dir`, in order, leaving out build output.
fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            if !path.ends_with("target") {
                rust_files(&path, out);
            }
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// The module `file` is, from its crate's root: `src/lib.rs` and
/// `src/main.rs` are the root, `src/a.rs` and `src/a/mod.rs` are `a`, and
/// `src/a/b.rs` is `a::b`. A file directly in `tests/`, `examples/`,
/// `benches/` or `src/bin/`, or a build script, is a crate root itself.
fn module_of(file: &Path) -> Vec<String> {
    let package = file
        .ancestors()
        .skip(1)
        .find(|dir| dir.join("Cargo.toml").is_file())
        .expect("the file is in a package");
    let mut parts: Vec<String> = file
        .strip_prefix(package)
        .unwrap()
        .with_extension("")
        .iter()
        .map(|c| c.to_string_lossy().into_owned())
        .collect();
    let top: Vec<&str> = parts.iter().take(2).map(String::as_str).collect();
    let (dir, crate_a_file) = match top[..] {
        ["src", "bin"] => (2, true),
        ["src", ..] => (1, false),
        ["tests" | "examples" | "benches", ..] => (1, true),
        _ => return Vec::new(),
    };
    parts.drain(..dir);
    let root = if crate_a_file {
        parts.len() == 1
    } else {
        parts == ["lib"] || parts == ["main"]
    };
    if root {
        parts.clear();
    }
    if parts.last().is_some_and(|p| p == "mod") {
        parts.pop();
    }
    parts
}

/// The lines of `src`, the file of `module`, that hit a row, with the row
/// and its slice.
fn scan(
    src: &str,
    module: Vec<String>,
    rows: &'static [(&'static str, u8)],
) -> syn::Result<Vec<(usize, &'static str, u8)>> {
    Ok(scan_file(&syn::parse_file(src)?, module, rows))
}

/// [`scan`] of a file already parsed.
fn scan_file(
    file: &syn::File,
    module: Vec<String>,
    rows: &'static [(&'static str, u8)],
) -> Vec<(usize, &'static str, u8)> {
    let mut scan = Scan {
        rows,
        module,
        hits: BTreeSet::new(),
    };
    scan.visit_file(file);
    scan.hits.into_iter().collect()
}

struct Scan {
    rows: &'static [(&'static str, u8)],
    /// The module being read, from the crate root.
    module: Vec<String>,
    hits: BTreeSet<(usize, &'static str, u8)>,
}

impl Scan {
    fn hit(&mut self, at: &Ident, row: &'static str, slice: u8) {
        self.hits.insert((at.span().start().line, row, slice));
    }

    /// A definition of `name`, in `within` (a type or trait, and a
    /// variant of it).
    fn defines(&mut self, within: &[&Ident], name: &Ident) {
        let path: Vec<String> = self
            .module
            .iter()
            .cloned()
            .chain(within.iter().map(|i| i.to_string()))
            .chain([name.to_string()])
            .collect();
        for &(row, slice) in self.rows {
            if row.split("::").eq(path.iter().map(String::as_str)) {
                self.hit(name, row, slice);
            }
        }
    }

    /// A path of two segments or more: a hit on each row whose last two
    /// it holds in a row, past its anchor.
    fn qualified<'i>(&mut self, path: impl IntoIterator<Item = &'i Ident>) {
        let path: Vec<&Ident> = path.into_iter().collect();
        if path.len() < 2 {
            return;
        }
        let anchors = path
            .iter()
            .take_while(|i| {
                let i = i.to_string();
                matches!(i.as_str(), "crate" | "self" | "super") || i.starts_with("sheaf_")
            })
            .count();
        let path = &path[anchors..];
        for &(row, slice) in self.rows {
            let row_path: Vec<&str> = row.split("::").collect();
            if let [name] = row_path[..] {
                if path.first().is_some_and(|first| *first == name) {
                    self.hit(path[0], row, slice);
                }
                continue;
            }
            let tail = &row_path[row_path.len() - 2..];
            if let Some(run) = path.windows(tail.len()).find(|w| w.iter().eq(tail)) {
                self.hit(run[0], row, slice);
            }
        }
    }

    /// A name on its own: a hit on each row it ends, unless that name is
    /// qualified-only.
    fn bare(&mut self, name: &Ident) {
        for &(row, slice) in self.rows {
            let last = row.rsplit("::").next().expect("a row has a name");
            if name == last && !QUALIFIED_ONLY.contains(&last) {
                self.hit(name, row, slice);
            }
        }
    }

    fn bare_last(&mut self, path: &syn::Path) {
        if let Some(last) = path.segments.last() {
            self.bare(&last.ident);
        }
    }

    /// Each path a `use` brings in.
    fn uses(&mut self, tree: &UseTree, prefix: &mut Vec<Ident>) {
        match tree {
            UseTree::Path(p) => {
                prefix.push(p.ident.clone());
                self.uses(&p.tree, prefix);
                prefix.pop();
            }
            UseTree::Name(n) if n.ident == "self" => self.qualified(prefix.iter()),
            UseTree::Name(n) => self.qualified(prefix.iter().chain([&n.ident])),
            UseTree::Rename(r) => self.qualified(prefix.iter().chain([&r.ident])),
            UseTree::Glob(_) => self.qualified(prefix.iter()),
            UseTree::Group(g) => {
                for t in &g.items {
                    self.uses(t, prefix);
                }
            }
        }
    }
}

impl<'ast> Visit<'ast> for Scan {
    fn visit_item_mod(&mut self, m: &'ast syn::ItemMod) {
        self.defines(&[], &m.ident);
        self.module.push(m.ident.to_string());
        visit::visit_item_mod(self, m);
        self.module.pop();
    }

    fn visit_item_struct(&mut self, s: &'ast syn::ItemStruct) {
        self.defines(&[], &s.ident);
        for field in &s.fields {
            if let Some(name) = &field.ident {
                self.defines(&[&s.ident], name);
            }
        }
        visit::visit_item_struct(self, s);
    }

    fn visit_item_enum(&mut self, e: &'ast syn::ItemEnum) {
        self.defines(&[], &e.ident);
        for variant in &e.variants {
            self.defines(&[&e.ident], &variant.ident);
            for field in &variant.fields {
                if let Some(name) = &field.ident {
                    self.defines(&[&e.ident, &variant.ident], name);
                }
            }
        }
        visit::visit_item_enum(self, e);
    }

    fn visit_item_fn(&mut self, f: &'ast syn::ItemFn) {
        self.defines(&[], &f.sig.ident);
        visit::visit_item_fn(self, f);
    }

    fn visit_item_const(&mut self, c: &'ast syn::ItemConst) {
        self.defines(&[], &c.ident);
        visit::visit_item_const(self, c);
    }

    fn visit_item_static(&mut self, s: &'ast syn::ItemStatic) {
        self.defines(&[], &s.ident);
        visit::visit_item_static(self, s);
    }

    fn visit_item_type(&mut self, t: &'ast syn::ItemType) {
        self.defines(&[], &t.ident);
        visit::visit_item_type(self, t);
    }

    fn visit_item_trait(&mut self, t: &'ast syn::ItemTrait) {
        self.defines(&[], &t.ident);
        for item in &t.items {
            let name = match item {
                TraitItem::Fn(f) => &f.sig.ident,
                TraitItem::Const(c) => &c.ident,
                TraitItem::Type(t) => &t.ident,
                _ => continue,
            };
            self.defines(&[&t.ident], name);
        }
        visit::visit_item_trait(self, t);
    }

    fn visit_item_impl(&mut self, i: &'ast syn::ItemImpl) {
        if let Type::Path(ty) = &*i.self_ty
            && let Some(ty) = ty.path.segments.last()
        {
            for item in &i.items {
                let name = match item {
                    ImplItem::Fn(f) => &f.sig.ident,
                    ImplItem::Const(c) => &c.ident,
                    ImplItem::Type(t) => &t.ident,
                    _ => continue,
                };
                self.defines(&[&ty.ident], name);
            }
        }
        visit::visit_item_impl(self, i);
    }

    fn visit_item_use(&mut self, u: &'ast syn::ItemUse) {
        self.uses(&u.tree, &mut Vec::new());
    }

    fn visit_path(&mut self, p: &'ast syn::Path) {
        self.qualified(p.segments.iter().map(|s| &s.ident));
        visit::visit_path(self, p);
    }

    fn visit_expr_method_call(&mut self, m: &'ast syn::ExprMethodCall) {
        self.bare(&m.method);
        visit::visit_expr_method_call(self, m);
    }

    fn visit_expr_call(&mut self, c: &'ast syn::ExprCall) {
        if let Expr::Path(callee) = &*c.func {
            self.bare_last(&callee.path);
        }
        visit::visit_expr_call(self, c);
    }

    fn visit_type_path(&mut self, t: &'ast syn::TypePath) {
        self.bare_last(&t.path);
        visit::visit_type_path(self, t);
    }

    fn visit_expr_struct(&mut self, s: &'ast syn::ExprStruct) {
        self.bare_last(&s.path);
        for field in &s.fields {
            if let Member::Named(name) = &field.member {
                self.bare(name);
            }
        }
        visit::visit_expr_struct(self, s);
    }

    fn visit_pat(&mut self, p: &'ast Pat) {
        match p {
            Pat::Path(unit) => self.bare_last(&unit.path),
            Pat::TupleStruct(tuple) => self.bare_last(&tuple.path),
            Pat::Struct(s) => {
                self.bare_last(&s.path);
                for field in &s.fields {
                    if let Member::Named(name) = &field.member {
                        self.bare(name);
                    }
                }
            }
            _ => {}
        }
        visit::visit_pat(self, p);
    }

    fn visit_expr_field(&mut self, f: &'ast syn::ExprField) {
        if let Member::Named(name) = &f.member {
            self.bare(name);
        }
        visit::visit_expr_field(self, f);
    }

    fn visit_macro(&mut self, m: &'ast syn::Macro) {
        if let Ok(args) = m.parse_body_with(Punctuated::<Expr, syn::Token![,]>::parse_terminated) {
            for arg in &args {
                self.visit_expr(arg);
            }
        }
        visit::visit_macro(self, m);
    }
}
