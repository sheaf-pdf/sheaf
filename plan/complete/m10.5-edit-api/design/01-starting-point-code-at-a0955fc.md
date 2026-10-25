# M10.5 §1: Starting point: the code at `a0955fc`

Part of [M10.5: the design](README.md).


The facts the design relies on, checked at `a0955fc`:

- **Branch state.** HEAD is `a0955fc` on `m10`: M10 slices 3, 4 and 5
  (`32723da`, `70257fc`, `e5468e8`), then the pause commit.
- **Vendored lopdf:**
  - `encrypt_object` (encryption.rs:828), `decrypt_object` (:833), and
    `transform_object` (:734), which exempts only xref streams and
    `/Metadata`;
  - no `/EFF` handling;
  - `Object::Real(f32)` (object.rs:83);
  - the `object_streams` cache (reader.rs:364) and `Reader::get_object`
    (:1095);
  - the reader slices the buffer at the first `%PDF-` and treats xref
    offsets as relative to it (reader.rs:686).
- **`kept.rs`** (1,144 lines, plus 657 lines of tests in
  `kept/tests.rs`):
  `keep_only(pdf: &mut Pdf, kept)` (:21) needs a mutable parsed graph.
  It keeps `/Names /Templates` pages, cuts references to removed pages'
  annotations, and prunes structure elements in place.
- **`save.rs`** (5,061 lines; tests from :1736):
  - `build_rewrite` (:379) gives a primary page object to the first page
    in document order that shows it (`used.insert(src_page)`, :613).
    That is a live ownership bug (Found while planning, bug 3).
  - Every rewrite ends with `rules::take_out(&mut pdf, form_changed)`
    (:707).
  - `subset` (:86) sets `form_changed` when it leaves out pages with
    widgets.
  - `export_clean_copy` (:34) and `export_flattened` (:182) refuse the
    open file. `extract_pages` (:72) and split don't.
  - `Signed::{Ask, Rewrite, Append}` (:869). `Append` means "append even
    what a full save takes out (a removed page, a changed form's XFA
    copy)".
  - `WouldBreakSignatures` (:884). `save_to_path_noting` (:899) tries an
    incremental save, then catches the failure. `write_atomic` (:954)
    has no fsync.
- **`incremental.rs`** (683 lines):
  - It imports
    `LoadedSource, copy_page_dict, geom_of, is_identity, link_connectors, strip_sheaf_annots, write_outline, write_page_annots`
    from save (:15–18).
  - It calls `file::page_geom`,
    `forms::merge::{merge, copy_annots, annots_on}`,
    `save::{reserve_page_objects, copy_of, origins, needed, copy_derived_page}`
    and `forms::rules::{has, take_out}`.
  - `Mode::{Save, Append, Sidecar}` (:47–59). Append and Sidecar take
    out XFA and usage rights inside the section (:427–436).
  - The section writer is at :478–550 and records offsets as absolute
    positions in the buffer. The object writer is
    `write_object`/`write_dict`/`write_name` at :591–683.
- **`model.rs`** (4,462 lines):
  - `SourceId(pub u32)` (:19) and `SourceInfo` (:32).
  - Today's `DocState` (:151–176): pages, outline, per-page sheaf
    `annots`, `text_layers`, labels, `header_footer`,
    `removed_comments`, terminal `fields` and per-page `widgets`.
  - `Ids { next_page, next_outline, next_field, seed }` (:536–541).
  - `Snapshot`, with seven dirty flags and the sources map (:606–630),
    computed in `snapshot()` (:1112–1125).
  - `add_source` (:785); `script_sources`, `scripts_mixed` and
    `form_notice` (:810–900); `rebase_to_saved` (:1208–1263).
  - `apply_in_job` (:1434–1463) amends the cursor version **in place**
    and recomputes `marginal_bytes`. Its only amend guard is
    `saved != cursor`.
  - `HistoryStats.undone_source_bytes` is hard-coded to 0 (:1715).
    `marginal` uses 256-byte guesses (:1798–1814).
  - `take_in_fields` (:1841–1910) joins a source's existing copy-0 form
    when its pages arrive in runs. `name_apart` (:2002) takes the stem
    from the source path.
  - Every insert registers a fresh `SourceId` (`add_source` at
    docview.rs:2705, page_insert.rs and combine.rs:169), so that join
    applies only within one registration's runs, and inserting identical
    bytes twice gives two forms
    (`corpus_a_form_combined_with_itself_is_two_forms`).
  - The `DocState` struct literals are at :735 and :2437. `signed_from`
    is at :2813 and `next_copy` at :2821.
- **Forms.**
  - `forms::Widget` (forms/field.rs:259) carries
    `object, slot, rect, flags, da, mk, border, state, states, has_appearance, actions`.
  - `Field.copy` and `Field.under` are at :208 and :212. `has_scripts`
    reads widget actions.
  - `forms/rules.rs`: `drop_xfa` copies tooltips onto the output's own
    fields, and `drop_rights` decides `/SigFlags` from the output's own
    signed fields, only when `/UR3` or `/UR` was present.
- **sheaf-ops `combine`.**
  - The first file becomes the primary through `Document::new` and
    `set_initial_fields` (combine.rs:147–152).
  - `insert_runs` inserts a file's non-contiguous selection as
    contiguous runs that join one form (:208–245).
  - Each later file gets a fresh `add_source`. The app writes the result
    and opens it in a new window (combine_dialog.rs:296–300).
- **App and plugins.**
  - `jobs::run_job` runs one job at a time (jobs.rs:3, :45). OCR is its
    own run (ocr_run.rs).
  - `Latest<T>` is latest-wins (plugins.rs:1415). `sign_marks` is at
    :1039 and `keep_unseen` at :1066.
  - The hide replays are at docview.rs:298, :1722 and :1901. The
    header/footer overlay is at :5196 and `annot_elements` at :5329.
  - The layers panel writes a decrypted temp copy (layers_panel.rs:84).
    The attachments panel opens attachments from a `temp_dir` folder
    (attachments_panel.rs:449), the one user-requested temp file.
- **sheaf-instance.** `PROTOCOL = 1`, a 2 s timeout and a 1 MiB request
  (lib.rs:31–40). `socket_path` falls back to `temp_dir` (:356).
  `recent.rs` legitimately compares paths with `temp_dir`.
- **Lint config.** There is no `clippy.toml`.
- **WIT 0.3.** `annot-delete(annot-ref)` reaches only sheaf marks, and
  `annotations()` returns only sheaf marks, with `author`.
- **Private names written today:**
  - `/SheafAnnot`;
  - `/SheafStamp`, both as an artifact property and as a stamp XObject
    name (annot.rs:1161–1168, read back at file.rs:509);
  - `/SheafStampText` (the stamp library, stamp_lib.rs:22);
  - `/SheafOcr`, the OCR layer font's resource name (text_layer.rs:396);
  - `/SheafOcrLayer`, the marker on the OCR layer's streams (:398);
  - `/SheafSettings`, which holds a local image path inside an inline
    property list (watermark.rs:216);
  - `/SheafHeaderFooter`, `/SheafHF` and `/SheafPart`;
  - `/SheafTick<id>` as a stamp `/Name` (annot.rs:2276);
  - the resource names `/SheafFlat1`, `/SheafWMf`, `/SheafWMgs` and
    `/SheafWMim`.
- **The OCR layer's shape.** Today's OCR layer is
  `[q, original…, layer starting "Q"]` (text_layer.rs:584–603, :526).
  Both added streams carry `/SheafOcrLayer true`; the `q` stream is
  shared per file (:494), and `strip` removes both (:625–651).
  `/SheafOcr` is the glyphless font's resource name (:396), used by `Tf`
  inside the layer.
- **Recovery v1** writes `<name>.pdf.sheaf-recovery` next to the file
  (recovery.rs:2, :17) and checks only the base's length (:58–70).
- **Measured.** A full rewrite of the 3.5 MB synthetic 7k file takes
  0.49 s and 69 MB (1.54 s and 131 MB at 14k). The synthetic file has
  one font, one shared Resources dict, a classic xref and no
  annotations.
