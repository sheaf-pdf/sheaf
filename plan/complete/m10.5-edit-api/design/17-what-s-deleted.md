# M10.5 §17: What's deleted

Part of [M10.5: the design](README.md).


The slice that deletes each item is in parentheses.

**sheaf-doc model.rs:**
- `Edit` and `compute` (11);
- `Ids { next_page, next_outline, next_field, seed }`, step seeds and
  `replay_step` (1);
- `double_option` and `EditOutcome` (11);
- `Rebase` and `Derived` (17);
- `SourceInfo`'s `page_sizes`, `bytes` and `data`, and
  `set_source_sizes` (3); `SourceInfo` itself, with `derived` moved to a
  side table (11);
- `unlogged_sources`, `next_source`, `add_source`/`register_source`,
  `Version.sources` and `Document::edge`'s source list (11);
- the six `set_initial_*` and `opened_state_mut` (4);
- the `saved_*` copies and `baked_header_footer` (8, folded into one
  baseline state, `Document.baseline`); the baseline,
  `annots_dirty_pages`, `fields_dirty`, `header_footer_dirty`,
  `form_changed`, `removes_content`, and the `Snapshot` dirty flags and
  sources map (9);
- `rebase_to_saved` (9);
- `begin_job`, `apply_in_job`, `job_is_current`, `job_is_undone` and
  `Version.job` (12);
- `cascade_delete`, `bring_annots` (slice 8's, which `import_marks`
  became) and `merge_outline` as compute arms (11);
- `Document.forms` and `set_source_form` (8);
- `script_sources`, `scripts_mixed`, `form_notice`, `signed_from` and
  `next_copy` (11);
- `take_in_fields`, `copy_fields` and `name_apart` (11; slice 8 folded
  `bring_fields` into `take_in_fields`);
- `DocState.{text_layers, header_footer, removed_comments}` and the
  widgets-by-page map (8);
- `Field.copy`, `Field.under`, and the ObjectIds in `Field` and
  `Widget`, which live on in the legacy projection (9);
- the `DocState` struct literals (:735, :2437) (8), and the `Snapshot`
  literal with its dirty flags (9);
- `marginal`'s guesses (1).

**Other sheaf-doc files:**
- describe.rs's 21-arm match (11).
- **save.rs's code half:**
  - the rewrite path: `build_rewrite`, `rewrite_with`, `finish`, the
    `renumber_objects` use (9);
  - exports: `subset`, `prune_outline`, `print_pdf`,
    `extract_pages`/`extract_bytes`, `export_clean_copy`,
    `export_flattened`, `flatten_annots`, `normal_appearance`'s widget
    fallback (19);
  - derived copies: `cropped_copy` (15); `derived_copy` and `copy_form`
    (17); `origin`/`origins`, `needed`, `copy_derived_page` and
    `copy_of`, whose only callers are the two old writers (9);
  - page reservation: `is_identity`, `reserve_pages`,
    `reserve_page_objects` (9);
  - moved code: `LoadedSource`/`inherited_attrs` and `link_connectors`
    (9);
  - annotation and outline writers: `write_page_annots`,
    `strip_sheaf_annots` and `write_outline` (9); `strip_markup_annots`
    (19);
  - geometry: `geom_of` (9);
  - saving: `write_atomic` and `save_to_path_noting`'s length-only disk
    check (7); its try-then-catch, `Signed`, and `WouldBreakSignatures`
    (now `Ask::Signatures`) (9).
- incremental.rs entirely, with its replaced-layer bails (9). Its writer
  moves to `serialize/` (7).
- recovery v1, its temp primary and its never-deleted temp file (9).
- generated.rs entirely (19).
- **file.rs:** `recover_cached` (6), `page_geom` (9), the full-parse
  `load_source` (moved into `materialize/graph.rs`, 9), and the
  decrypt-and-strip in `load_pdf`/`load_pdf_mem` (3 for the read-only
  paths, 7 for the rest).
- **stamps.rs:** `strip_ops` and `has_artifacts` (6); `install`, `strip`
  and `to_catalog`/`from_catalog` (17). The encoders move to
  `content/font` (6).
- **watermark.rs:** `install`, `read`, the per-page
  ExtGState/font/image, `SheafSettings` and the `/SheafWM*` names (16).
- **text_layer.rs:** `shifted` (5); `search` and `pattern_search` (14);
  `install`, `strip` and `has_layer` (18).
- **annot.rs:** `PageGeom` and `/SheafTick<id>` (5); `content_literal`,
  `font_resources`, `wrap_lines`' average width, the cache-stem stamp
  import with its labeled-box fallback, per-write attachment
  re-embedding, and `/SheafStamp` as a written XObject name (6).
- **forms:** `rules::take_out` (rewritten as `rules::plan`) and
  forms/merge.rs's per-writer copy maps (moved into `emit/form.rs`)
  (9).

**sheaf-ops:**
- derive.rs entirely (`watermark` and `current_watermark` in 16, the
  rest in 17);
- crop.rs's derived path (the trim-white measurement is kept) (15);
- open.rs (4);
- the hand-rolled flows in combine.rs (`insert_runs`, one version per
  mark, position lookups) (11); scan.rs, blank.rs, export.rs and
  split.rs (19).

**sheaf-render:** `RenderHandle`'s thread per source and
`open(PathBuf)`, `hide_annots` and `set_annots_hidden` (13); search by
source index (14).

**sheaf-app:**
- **docview.rs:**
  - the old edit funnel:
    `apply`/`apply_batch`/`finish_edit`/`after_model_change` and its
    hand copies, `touched` and `reveal_new_bookmarks` (12);
    `drop_stale_images` and the post-edit `invalidate_tiles` (13);
  - the old drawing and hiding: `Doc.foreign_annots` (8);
    `header_footer_elements`, `drawn_marks`, `foreign_hidden` and
    `Doc.handles` per source (13);
  - the old text paths: `index_of_source_page`, `layer_hits`,
    `merge_source_hits` (14);
  - the async fact loaders and `load_sheaf_annots` (4);
  - `save_to`'s reopen, re-hide and re-read, and the `Signed::Append`
    offer path (9);
  - every id-guess site (11);
  - `edit_source`, which plugins.rs sets (20).
- page_insert.rs's derived crop (15), watermark (16) and header/footer
  (17) flows, and the synchronous blank and image cache writes (19).
- layers_panel.rs's temp copy (13), and the print and export temp files
  (14).
- text_select.rs's `CharsKey` (14).
- plugins.rs's `sign_marks` and `keep_unseen` (11), and the batch loop
  (20).
- workspace.rs's `current_watermark` file read (16).
- combine_dialog.rs's write-then-reopen (19).

**sheaf-plugin and sheaf-plugin-api:**
- wit/0.2, `api/v02.rs` and `api/v03.rs` (included at api/mod.rs:38)
  (20);
- the double bindgen and `World` (20);
- `API_PREVIOUS` and the 0.2 fixture wasm (20);
- `annot_from_wit`'s filling (11); `edit_from_wit` and the 0.3 edit
  variant (20);
- `EditBatch` and `Batch` (20);
- the OCR special cases in `api/shared.rs` (18).

**Formats:**
- written no more: `/SheafTick<id>` and `/SheafAnnot` v1 (5);
  `/SheafStamp` as a stamp XObject name (6); `/SheafPart` and recovery
  sidecar v1 (9); `SheafSettings`, `/SheafWM*` and `/SheafStamp` as a
  watermark's artifact property (16); `/SheafHeaderFooter`, `/SheafHF`
  and `/SheafStamp` as a header's artifact property (17);
  `/SheafOcrLayer` and `/SheafOcr` as the OCR font's resource name
  (18); `/SheafFlat1` (19);
- the legacy ones are read for recognition, and `RemovePrivateData`
  strips them.

**Enforcement.** `tests/retired.rs` (from slice 1) lists every retired
identifier as a qualified path, such as `save::subset`,
`text_layer::search` or `incremental::Mode`, with its slice, the same
one as above. It matches with a `syn` walk of `crates/`, excluding
`vendor/` and the identifiers the new design reuses: `Mode`, `Step`,
`PageRef`, `LabelSpec` and `Widget` are matched only qualified. Each
slice adds its own rows in the same commit.
