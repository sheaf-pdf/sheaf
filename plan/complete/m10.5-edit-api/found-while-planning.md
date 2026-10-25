# M10.5: Found while planning

Part of [M10.5: the edit API](README.md).


Bugs and gaps in today's code that matter for M10.5. References are as
of `a0955fc`. "From the code" means read, not reproduced; "inferred"
means the survey reader concluded it without running it. The slice in
brackets fixes each.

1. **An edit made during a save is lost or misfiled.** From the code.
   - `DocView::save_to` snapshots the cursor and saves in the
     background, and edits stay enabled; only a second save is refused
     (docview.rs:1655).
   - When the save finishes, `rebase_to_saved` (model.rs:1208)
     re-expresses whatever version is the cursor *then* as the saved
     file: it clears that version's OCR layers, maps its pages by
     position onto the saved file's pages, and marks it saved.
   - So an OCR flush during the save loses its layers, a page move
     during the save shows the wrong saved pages in those positions, and
     any edit made during the save is marked saved without being in the
     file.

   [Slice 9.]
2. **A file changed on disk is overwritten without asking, and saves
   aren't durable.** From the code.
   - The only check is the file's length against what was opened
     (`SourceInfo.bytes`, save.rs:908). When it differs,
     `save_to_path_noting` falls through to a full rewrite and replaces
     the file (save.rs:923–946).
   - A change that keeps the length goes unnoticed, and the incremental
     save then appends to the bytes read at open (incremental.rs:65),
     not to the file on disk. Either way the other program's changes are
     discarded.
   - `write_atomic` (save.rs:954) uses one fixed temp name per file, so
     two saves collide, and it never fsyncs.

   [Slices 7 and 10.]
3. **The first copy in page order owns a page's object.**
   `build_rewrite` gives a page's original object to whichever copy
   comes first (`used.insert(src_page)`, save.rs:613). Move a duplicate
   before its original and save: the duplicate takes the original's
   object number, and the links, `/P` entries and structure that pointed
   at the original now point at the copy. From the code. [Slice 9.]
4. **Encryption is dropped on load, and lopdf's per-object crypto has
   gaps** (M11 bug 2).
   - `file::load_pdf_mem` and `load_pdf` remove `/Encrypt` (file.rs:258,
     :277), so every rewrite is plaintext, and the incremental writer
     refuses encrypted files (incremental.rs:73). The save notice says
     "password protection removed" (docview.rs:1749).
   - Vendored lopdf's `transform_object` (encryption.rs:734) exempts
     only xref streams and `/Metadata`, has no `/EFF` handling, and
     doesn't reach strings inside stream dicts (lopdf #603).

   [Slices 3 and 7.]
5. **An append to a file with bytes before `%PDF-` mixes offsets.**
   lopdf reads xref offsets relative to the first `%PDF-` (vendor
   reader.rs:686), and the incremental writer takes `/Prev` from it
   (incremental.rs:76, :495) but writes its new objects' offsets as
   absolute positions in the buffer (:484–546). It also copies `/ID`
   unchanged (:496–500). From the code. [Slice 7.]
6. **Replay isn't exact, and a failure is silent.**
   - `compute` (model.rs:2119) reads inputs no step records: the
     sources' page counts and sizes, the rng, `Document.forms` and path
     stems. `replay_step` (:1502) re-runs it.
   - `undo` and `redo` turn a replay failure into a log warning and do
     nothing (model.rs:1335, :1346).
   - The property test `replaying_the_log_rebuilds_every_version`
     (model.rs:3466) covers 7 of the 21 kinds.

   [Slice 1.]
7. **Versions change in place, and a no-op is found by deep
   comparison.**
   - `apply_in_job` (model.rs:1434–1463) amends the cursor version in
     place, guarded only by `saved != cursor`, and `rebase_to_saved`
     changes a version in place too. Whoever read version V can't tell
     that V changed.
   - No-op detection compares whole states (`state == *self.state()`,
     model.rs:1384, :1457). On stable Rust, im's `PartialEq` has no
     `ptr_eq` shortcut, so every apply compares every page, mark, OCR
     word, field and widget.

   [Slice 1.]
8. **History descriptions are computed against the wrong state.**
   `VersionView` describes every step of a batch or job against the
   parent state, so later steps print the wrong page numbers, and falls
   back to counts when the parent's snapshot was trimmed. describe.rs
   prints "Change the content of …" for every `RebasePages`
   (describe.rs:113), whatever the feature. [Slices 1 and 11.]
9. **History memory is guessed.** `marginal` (model.rs:1798–1814) counts
   256 bytes per changed annotation, widget or field and ignores step
   payloads. `HistoryStats.undone_source_bytes` is hard-coded to 0
   (model.rs:1715), and sources are never freed. Inferred: at 20,000
   fields a commit costs about 5 MB by this count, so the 256 MB budget
   lasts about 50 commits. [Slices 1, 3 and 12.]
10. **File facts that arrive late are dropped.** `opened_state_mut`
    (model.rs:760) works only before the first edit, so six loaders log
    "… arrived after edits; ignored" and drop their facts
    (docview.rs:1208, :1858, :1867, :1872, :1915; form_bar.rs:71).
    Plugins get `opened` before those loads finish, so a plugin can win
    the race (inferred). The app and headless opens are two
    implementations (`sheaf_ops::open`). [Slice 4.]
11. **Unknown ids are ignored, and references are never validated.**
    Move, Rotate, Delete, Duplicate and ClearTextLayers skip ids they
    don't know (their `compute` arms at model.rs:2122, :2153, :2170,
    :2248, :2618). Bookmark targets, label anchors and the
    header/footer's page scope are never checked. [Slice 11.]
12. **Cascades are written per edit, and differ.**
    - DeletePages (model.rs:2170) leaves the fields
      (`form_fields_follow_their_pages` asserts 9 remain,
      model.rs:3080), the label anchors and the header/footer's page
      scope.
    - DuplicatePages (model.rs:2248) doesn't copy `removed_comments`, so
      another tool's comment deleted on the original reappears on the
      copy. From the code.
    - ReplacePages keeps `removed_comments` on the extras it deletes.
    - RebasePages shifts marks and OCR words, but not widgets or
      bookmark views.

    [Slices 5, 8 and 11.]
13. **Removal is detected by id, so replaced text survives incremental
    saves.**
    - `removes_content` (model.rs:1156–1195) checks for missing primary
      pages, mark ids, deleted comments and bookmark ids, nothing else.
    - Replaced mark text, a renamed bookmark, a changed label prefix and
      a deleted mark from an inserted file all slip through (the last
      inferred), so the old text stays in the earlier revision. A
      replaced OCR layer is caught only because the incremental writer
      bails (incremental.rs:268, :322).
    - It counts any primary page shown from another source as removed,
      so a crop forces a full rewrite though it removes nothing (the M10
      worker's note).

    [Slice 10.]
14. **The incremental writer can't express enough, and writes too
    much.** `incremental::build` (incremental.rs:62):
    - parses the whole primary eagerly (:67);
    - can't draw a header/footer (:71), so `/SheafHeaderFooter` and the
      drawn Bates numbers go stale after a reorder until a full save;
    - can't take anything out, so removals, changed hybrid forms and
      signed-file appends each special-case it (`Mode`, :47–59), and a
      saved OCR layer can't be cleared at all;
    - on any page tree that isn't the identity, rewrites every page dict
      into one flat `/Pages` node, so a crop or a reorder of 7,000 pages
      writes 7,000 dicts, and drops `/PageLabels`;
    - rewrites every mark of a dirty page as a new object on every save.

    [Slices 9, 10 and 18.]
15. **The outline is rebuilt on every rewrite** by `write_outline`
    (save.rs:1318), which keeps only Title, Dest, C, F and links: `/A`
    actions, `/SE`, named destinations and the closed state are lost.
    Labels the model didn't read are dropped on a reorder. [Slices 8 and
    9.]
16. **A derived source costs a file, a thread and a full rewrite per
    edit.**
    - Each crop, watermark or strip writes a whole PDF per source
      (`derived_copy`, save.rs:727; `cropped_copy`, :714) to the
      plaintext cache (`generated::store`, called at sheaf-ops
      derive.rs:71, :154 and crop.rs:150), form values included, and
      never deletes it.
    - Each opens its own PDFium thread (`RenderHandle::open`,
      sheaf-render lib.rs:1063), never freed.
    - The plans are applied to whatever state is current when they
      finish (page_insert.rs:319–326, :450–456, :562–569), with
      `NoProgress` and no cancel. Two overlapping crops can undo each
      other (inferred).

    [Slices 15, 16, 17 and 19.]
17. **Watermarks.**
    - One new ExtGState and one new font or image per page
      (watermark.rs:242, :250, :256): an image watermark on 7,000 pages
      embeds 7,000 copies.
    - Text is written as UTF-8 into a WinAnsi font
      (watermark.rs:182–196), so "é" shows as "Ã©".
    - The settings, including a local image path, go into the file as
      `/SheafSettings` (watermark.rs:174, :216).

    [Slices 6 and 16.]
18. **Removing headers and watermarks is lossy.** `stamps::strip`
    (stamps.rs:457):
    - round-trips through lopdf's `Content` (f32 numbers, inline
      images);
    - skips a stream it can't decode without a word (:478), so a header
      can end up drawn twice (inferred);
    - works one stream at a time, so it misses a marked range that spans
      two streams;
    - replaces the stream object in place (:485), so every page that
      shares the stream changes (blank pages share one).

    `has_artifacts` (stamps.rs:495) decompresses every page's content to
    look. [Slices 6 and 16.]
19. **Mark appearances and stamps.**
    - `content_literal` (annot.rs:1515) writes everything outside ASCII
      and U+00A0–U+00FF as `?`, curly quotes, dashes and € included.
      `wrap_lines` (:1535) uses an average width.
    - Each mark gets its own inline fonts (`font_resources`, :1561) and
      ExtGState.
    - `to_pdf_in` (annot.rs:1120) re-imports page stamps from cache
      stems on every save, with a labeled box when the cache file is
      gone, and re-embeds attachments from the cache on every write.
    - Stamp placement ignores the form's BBox origin and the stamp
      page's `/Rotate` (inferred).

    [Slice 6.]
20. **Five coordinate conventions.**
    - Marks and OCR words are in source-displayed space
      (`annot::PageGeom`); widgets are in user space before `/Rotate`
      (forms/field.rs); bookmark XYZ is in the displayed frame and isn't
      shifted by a crop (inferred); header/footer and watermark layout
      is in the shown frame at write time; tapes have their own turned
      frame.
    - There are two geometry readers, `save::geom_of` and
      `file::page_geom`.
    - WIT's `page-info` width and height include sheaf's rotation, while
      its coordinates don't.

    [Slice 5.]
21. **Exports are a separate pipeline.**
    - `subset` (save.rs:86) fakes a `Snapshot` and keeps every field.
      Its outline rule (a parent becomes a heading) differs from
      DeletePages' (children are promoted), and an unsaved Bates header
      is renumbered per subset.
    - `extract_pages` (save.rs:72) and split don't refuse the open file,
      though Clean Copy (:34) and Export Flattened (:182) do.
    - `flatten_annots` (save.rs:200) ignores the Print flag, merges the
      page's content into one new stream, keeps widgets live, falls back
      to the first `/AP /N` state (`normal_appearance`, :354; M10 bug 10)
      and doesn't prune structure references.
    - Clean Copy keeps `/SheafHeaderFooter` and `SheafSettings`.

    [Slice 19.]
22. **Document content in temp and cache files.**
    - The layers panel writes a decrypted temp copy of the primary
      (layers_panel.rs:84).
    - Recovery writes `sheaf-recovered-<pid>-<ms>.pdf` and never deletes
      it (docview.rs:1024).
    - Print and export write temp files (docview.rs:3505; sheaf-ops
      export.rs:113).
    - Tape and stamp pictures go to the cache, and the generated cache
      (generated.rs) is never cleaned.

    [Slices 6, 9, 13, 14 and 19.]
23. **Recovery is limited to what the incremental writer can express.**
    The sidecar is `incremental_section` (incremental.rs:39), so a
    document with an unsaved header/footer or an encrypted file gets
    none. It's checked only by the file's length (recovery.rs:58), and
    every autosave re-parses the whole primary. [Slices 9 and 10.]
24. **Rendering.**
    - Hiding is engine state, replayed on every open (docview.rs:298,
      :1722, :1901; layers_panel.rs:110; comments_panel.rs:621), each
      time followed by a global invalidate. Print preview, OCR and
      plugins share those handles, so they inherit the view's filters
      (inferred).
    - Caches keyed by `(source, index)` miss a content edit that keeps
      the source, and every save drops all tiles.
    - The unsaved header/footer is a GPUI overlay (docview.rs:5196) that
      isn't drawn under a view rotation, and it computes every page's
      Bates numbers for each page it draws (:5210).
    - Find and Patterns search only the primary engine plus unsaved OCR
      layers (`index_of_source_page`, docview.rs:350; M11 bug 6).

    [Slices 13, 14 and 17.]
25. **Ids are guessed after apply, and drags overwrite.**
    - The new mark is "the last mark on the page" (docview.rs:1965,
      :2476); paste uses `skip(before)` (markup_tools.rs:1684); adoption
      finds the mark by `/NM` (adopt.rs:87); a new bookmark is the next
      one in pre-order (outline_panel.rs:206), which is wrong when the
      selected bookmark has children (inferred); combine finds its pages
      after the old count.
    - sheaf-ops `combine` applies one `AnnotAdd` version per mark
      (combine.rs:175).
    - Drags write back whole-record clones taken at mouse-down, so a
      plugin or OCR change made during the drag is lost (inferred).

    [Slices 11 and 12.]
26. **Three apply funnels, and a relayout per edit.** `finish_edit`
    (docview.rs:1259); hand-written `touched()` and `after_model_change`
    pairs in `insert_file` (docview.rs:2643), page_insert.rs and
    blank_review.rs; and OCR's `apply_in_job`. `touched()`
    (docview.rs:419) clears every pane's layout on every edit, undo or
    jump. [Slice 12.]
27. **Plugins.**
    - No version check anywhere: a batch is applied to the document as
      it is when the call returns, up to 30 s later. Batches apply only
      on `Ok`, each as its own version, and are dropped otherwise.
    - `edits.submit` returns nothing (wit/sheaf.wit:407), so a batch
      can't refer to what it creates.
    - `sign_marks` (plugins.rs:1039) fills `<<name>>`, `<<initials>>`
      and an empty author with the user's identity, which the plugin can
      then read back. `keep_unseen` (plugins.rs:1066) repairs
      whole-record updates from the current mark.
    - `annotations()` returns only sheaf marks, with author names, and
      `annot-delete` reaches only sheaf marks (wit/sheaf.wit:254).
    - amount-crossref and duplicate-pages keep session PageIds in
      `PENDING` statics keyed by path
      (plugins/amount-crossref/src/lib.rs:120,
      plugins/duplicate-pages/src/lib.rs:168), so after a reopen they
      can act on the wrong pages (inferred).
    - Only the plugin that made an edit is skipped for its event, so two
      plugins can ping-pong (inferred).

    [Slices 11 and 20.]
28. **Hot spots at 7,000 pages.** `index_of` (model.rs:268) is linear
    inside AnnotAdd, Insert and CommentsDelete; `page_positions` (:302)
    builds a HashMap per call; `fresh_annot_id` (:2801) scans every mark
    through `find_annot` (:217); crop looks up each id linearly; the
    header/footer's `covers()` is `Vec::contains`; `has_artifacts`
    decompresses every page; `strip_sheaf_annots` parses every
    `/SheafAnnot` on every rewrite. [Slices 8, 15, 16 and 17.]
29. **What M10 slices 3–5 left for this** (the worker's notes, also at
    the end of `m10.md`).
    - Derived copies route the page object, `/Annots`, fields and
      structure parents back to the original at save time
      (`SourceInfo::derived`).
    - `Field::copy` and `Field::under` stand in for "which instance of
      which file's form", and both writers key their copy maps by
      (source, copy).
    - `rebase_to_saved` only resets fields to the new primary (copy 0,
      no parent); their objects in a rewritten file are stale.
    - Two writers carry the same page logic, derived pages, own objects
      for copies, the form merge and the form rules.
    - Notices travel as `EditOutcome` fields (`forms_renamed`,
      `signatures_in`) that each caller reads, and Combine's can't reach
      the window it opens.

    [Slices 8, 9, 11, 17 and 19.]
30. **The docs have drifted.** `docs/document-model.md` describes a
    linear `Vec` history with a `usize` cursor (:41–42), edits as pure
    functions (:46), `Arc<Vec<PageRef>>` (:80), and says lopdf doesn't
    write incremental updates. [Every slice updates its section; slice
    20 makes the final pass.]
