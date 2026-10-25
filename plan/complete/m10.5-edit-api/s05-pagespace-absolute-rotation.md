# M10.5 slice 5: PageSpace and absolute rotation

Status: done · Part of [M10.5: the edit API](README.md)

**PageSpace and absolute rotation** (L; proposal 21; bugs 12 and
20).
- **`Geom::{visible, shown, to_view, from_view}`** is the only
  conversion. Marks, OCR words, bookmark `DestView`s, regions and
  connectors are stored in PageSpace, on today's DocState. The plugin
  0.3 adapter converts at the edge.
- **`/SheafAnnot` v2,** with the canonical staleness digest and tick
  ids, is written by the old writers:
  - v1 is converted from the standard keys;
  - duplicate `sheaf:<id>` names get fresh ids at open;
  - stale marks open as others' comments, in today's list.
- **Rotation** is absolute in today's `model::PageRef` struct, and
  render jobs get the delta.
- **Derived crops** keep user space, so `RebasePages`' shift is zero.
- **The clipboard** holds view-frame geometry, and paste converts
  through the target's frame.
- **`geom_of` and `page_geom`** become thin wrappers returning
  `Geom`, kept for the old writers until slice 9.
- **Deletes:**
  - `Rebase.shift` and its shifting of marks and OCR;
  - `TextLayer::shifted`;
  - `annot::PageGeom`;
  - the outline frame conversions in the writers;
  - the app's `rotate_pt`/`unrotate_pt` conventions;
  - `model::PageRef`'s rotation delta and its reset in
    `rebase_to_saved`;
  - `/SheafTick<id>`.
- **Checks:**
  - A property test over `/Rotate` × view rotation × CropBox offsets
    × MediaBox off the origin: `to_view ∘ from_view` is the identity
    within 1/16 pt.
  - Every mark kind round-trips on `/Rotate` 0, 90, 180 and 270 with
    offset boxes.
  - A highlight that is cropped, rotated, saved and reopened still
    covers the same glyphs.
  - A bookmark XYZ on a cropped page lands on the same content.
  - A v1 fixture's rects equal v2's within 0.01 pt.
  - A stale mark opens as another tool's comment and isn't reverted.
  - A mark re-serialized by another tool (reals reformatted,
    `/Contents` as UTF-16BE, `/Rect` corners swapped) stays a sheaf
    mark.
  - A page duplicated by another tool opens with distinct ids and one
    notice.
  - Widget hit testing equals today's on rotated corpus pages.
  - Paste between pages with different `/Rotate` and CropBox keeps
    the visual position.

## Notes

Done. Everything anchored to a page has one frame, PageSpace: the
page's default user space, points, y up, before `/Rotate`, whatever
its boxes. Marks, other tools' comments, OCR words, the engine's
characters, bookmark `DestView`s, regions and connectors are stored
in it on today's DocState, and `Geom::{to_view, from_view}` is the
one conversion, the old writers' and readers' included
(`save::geom_of` and `file::page_geom` return a `Geom`).
`PageRef.rotation` is absolute and a render takes the delta, a crop
moves nothing, and a mark has a `turn`, the rotation it reads upright
in, so the writer needs no page geometry. `/SheafAnnot` v2 holds the
PageSpace JSON with a canonical staleness digest: v1 converts from
the standard keys, a stale mark opens as another tool's comment,
copies of a mark get fresh ids with one notice, and ticks are
`/Tick`. The plugin 0.3 adapter converts at its edge, and the app at
its own: the pointer, drawing, PDFium's text, and a clipboard of
marks as their page showed them (proposal 21, bugs 12 and 20).
- **The frame** (`geom.rs`): `Affine`, a 2×3 f32 map in PDF's order
  with public `a`–`f` and `IDENTITY`, with `upright(r)` (`to_view`'s
  linear part for a page turned `r`), `apply`, `apply_box`,
  `apply_axes` (an `/XYZ` point whose missing coordinate stays
  missing; it debug-asserts a quarter-turn map), `then` and
  `inverse`. `Geom::to_view()` is `upright(rotate)` and a translation
  to the top-left of the visible box as shown, PDFium's
  page-to-device mapping at one pixel a point; `from_view()` is its
  inverse. `geom::quarter_turns(degrees)` is `Geom::read`'s rotation
  rule, factored out for the writers that read a dictionary's
  `/Rotate`. `Source::geom(i)` is new (Letter past the table), and
  `page_size` is its `shown()`.
- **Absolute rotation** (`model.rs`): `PageRef.rotation` is set from
  `src.geom(index).rotate` in `Document::loading`, `InsertPages` and
  both `ReplacePages` arms; `RotatePages` adds to it, `RebasePages`
  keeps it, and `rebase_to_saved` keeps it (its reset is gone).
  `Document` and `Snapshot` have `geom(page)`, the source page's
  boxes with the page's rotation, and `render_delta(page)`, the turn
  from the source page's rotation to the page's; `page_size` is
  `geom(page).shown()`. Every render that meant "the turn over the
  source" takes `render_delta`: the app's render jobs, keep sets,
  preview, tile and thumbnail keys, search reveal, print, blank
  review, OCR, crop's TrimWhite, and the plugins' `page_info` and OCR
  render. The watermark copy is drawn with the page's absolute
  rotation on the copy's `Geom` (derive.rs), and the comment summary
  maps its markers through the printed page's `Geom`. `build_rewrite`
  writes `pref.rotation` (no key at R0), as does incremental.rs's
  new page tree; in the tree it keeps, a page dictionary is
  rewritten when `render_delta` isn't R0, and then says its rotation,
  0 too, where `apply_rotation` added the delta to the key.
- **Marks in PageSpace** (`annot.rs`): `Annot.turn: Rotation` (serde
  default, skipped at R0); `AnnotKind::mapped(&Affine)` (points
  pointwise, boxes through `apply_box`, an icon's corner through
  `icon_box`; the old closure mapper is `map_with`, which
  `translated` and `resized` use);
  `Annot::{mapped, upright, page_bounds, to_page, to_frame}`, where
  `upright()` is the mark's shape (an `AnnotKind`, the mark itself
  not cloned) in the frame it reads upright in, PageSpace turned by
  `turn`, y down, which `AnnotKind::bounds` (moved from `Annot`),
  `outline_of`, `text_line_paths`, `caret_points`,
  `measure_label_box` and the tape and stamp layouts are written for;
  `Annot::hit` takes a point in that frame and lays the shape out
  itself. `Box2`'s `(x, y)` is its low corner, bottom-left in
  PageSpace. The writer lays out `a.upright()` and maps it into
  user space with `Affine::upright(turn).inverse()`, text turned
  `turn`; `cloud_ops` and `tape_content` take an `&Affine`, the page
  stamp's placement maps its box upright, and `tape_pdf` renders the
  tape as it reads. `write_page_annots` takes no geometry (a
  connector lands at its PageSpace top-left);
  `stamps::{to_content, install}` and `watermark::install` take a
  `&Geom` with the absolute rotation and draw through `from_view()`;
  the header and footer lay out on
  `geom_of(&dict, pref.rotation).shown()`; `stamp_lib::page_forms`
  returns `Geom`s. `import::convert` takes a turn and reads the
  standard keys as PageSpace, `ForeignAnnot.rect` and `Comment.rect`
  are PageSpace, exchange's `import` turns marks by `pref.rotation`
  and `gather` reads other tools' dictionaries through the lazy
  `Source`. `group_members` moved to `Document` and `Snapshot`
  (`group_members_in`) and orders a page's marks by `to_view` of
  their `page_bounds()`, top then left; `link_connectors` takes the
  `Snapshot`. The comment summary maps its markers through the
  printed page's `to_view`.
- **`/SheafAnnot` v2**: `to_pdf_in` sets it last, on the finished
  dictionary: `Payload<M> { v, std, #[serde(flatten)] mark }` with
  `VERSION = 2` and `std = staleness(pdf, &dict, threaded)`. The
  digest's keys, in order: `/Subtype`, `/Rect` (corners normalized),
  `/Contents`, `/T`, `/M`, `/NM`, `/C`, `/IC`, `/CA` (1 when absent),
  `/QuadPoints`, `/InkList`, `/L`, `/Vertices`, `/CL`, `/RD`; numbers
  cast to f32, then counted in 1/64 pt (geometry) or 1/1000 (colors
  and `/CA`), which `annot::on_grid` puts the writer's numbers on
  first (`/RD` as the inset to an inner box on the steps); text
  strings through `file::text_string` and NFC
  (`unicode-normalization = "0.1"`, sheaf-doc's new dependency);
  `sheaf_core::checksum_hex` of the canonical bytes. A text string,
  the JSON's included, is written as is only where PDFDocEncoding
  reads it back so (`file::is_pdf_doc_ascii`: ASCII but 0x18–0x1F
  and 0x7F), else UTF-16BE, by annot.rs's `pdf_text_string` and
  `save::text_string` alike. `annotation()`
  no longer writes `/SheafAnnot`, so the StrikeOut companion no
  longer removes it; a tick's stamp is `/Name /Tick`.
  `AnnotId::sheaf_names(id)` gives `sheaf:<id>` and
  `sheaf:<id>:strike`. The outline's text decoder moved to file.rs as
  `pub fn text_string` (BOMs, PDFDocEncoding), and the outline's
  `title` is it plus PDFium's control-characters-as-spaces rule.
- **Reading marks** (`file::sheaf_mark(pdf, entry, turn, annots)`,
  `annots` a `file::PageAnnots`, the page's `/Annots` with its
  StrikeOuts indexed by `/IRT` the first time a v1 replacement asks):
  v2 counts while its digest is the dictionary's; v1 while its
  `modified` is `/M`, its geometry read by `from_standard_keys` with
  import.rs's readers, now shared (`numbers`, `user_box`, `inset`,
  and the new `quads`, `points`, `ink`): a box is `/Rect` less `/RD`
  or less the old writer's pad, text markup its `/QuadPoints`, lines
  and paths their `/L`, `/Vertices`, `/InkList` and `/CL`, a note's
  or an attachment's corner its icon box's, a replacement's `at` its
  Caret's and its struck rects its grouped StrikeOut's; `turn` is the
  page's `/Rotate` and the rest is the JSON's. Another version isn't
  read. `is_ours(pdf, entry, annots)` calls `sheaf_mark`, so it can't
  disagree with the open; `annots_and_cached` passes each page's turn
  and `/Annots`, and puts a stale mark in `foreign` with the mark
  adopting it would make; exchange's import and the attachments
  listing use the same reader.
- **Copied marks** (`open::fresh_ids`, in `finish`): the first mark
  with an id, in page and then `/Annots` order, keeps it, and each
  later one gets a new id below 2^53 that no mark in the file has,
  from `fastrand::Rng::with_seed` of the source's `BlobId` hash.
  `Facts.renamed: Vec<open::Renamed { page, was, id }>`, the copies
  whose `/NM` changes, becomes `ViewFacts.renamed: Vec<(PageId,
  AnnotId)>` in `opened`, which drops those pages from
  `saved_annots`, so either save writes their new `/NM`;
  `follow_renamed` names a copy by its new name in the `parent` of
  another tool's comment in its thread. `Document::set_source_renamed`
  keeps each source's renames by page (`ready` for the file, the app
  for an inserted or replacing file), `Snapshot::renamed_on(source,
  index)` gives a page's, and `write_page_annots` takes them: the name
  the file gives each of our marks and the name it has now, so
  another tool's reply to a copy is re-pointed at the copy's new
  object, and goes with it when it's deleted. The app hides both old
  names on those pages at ready and on the insert and replace path
  (`marks_by_page(marks, renamed)`, `hide_renamed`, by maps from page
  to index and to position), and `ready` shows one notice
  (`renamed_notice`).
- **A fuzz target for v2**, as slice 3's:
  `source::fuzz::sheaf_annot`, `fuzz/fuzz_targets/sheaf_annot.rs` and
  the fuzz.yml matrix, where `SHEAF_FUZZ_TARGET` picks the seeds
  `write_fuzz_seeds` writes; `mark_fixtures` (every kind in v2 and a
  v1-shaped copy) seed it and `fuzz_bodies_survive_mutated_fixtures`;
  `pdf.dict` has the annotation keys and JSON tokens.
- **OCR and PageChars:** `OcrWord.rect` is PageSpace and
  `TextLayer.upright` absolute.
  `TextLayer::from_engine(words, &Geom, engine)` groups lines in the
  view frame, then maps each box through `from_view()` and sets
  `upright = geom.rotate`; `to_content(layer)` and
  `install(pdf, page, layer, objects)` take no geometry and place the
  text through `Affine::upright(upright).inverse()`, turned
  `upright`. `PageChars::from_engine(chars, base: &Geom)` maps the
  engine's boxes through the base page's `from_view()` with
  `upright = base.rotate`, and `from_layer` reads the layer as it is.
  sheaf-ocr's `OcrPage { id, source, index, geom, render }` is
  captured when the job starts;
  `layer_from_words(words, &geom, raster_w, raster_h, engine)` scales
  pixels to `geom.shown()`, and the job renders at its width turned
  `render`. The plugins' `OcrSource::recognize` takes
  `(page, &Geom, render, language, dpi)`, and scan.rs's `add_text`
  takes each image page's `doc.geom(p)`.
- **Crops and bookmarks:** `RebasePages` only re-points pages:
  `Rebase { id, source, index }`,
  `DerivedSource.pages: Vec<(PageId, u32)>`, and crop builds the crop
  box from the kept area as shown through `from_view()`.
  `DestView::Xyz` is PageSpace, as the file has it: open/outline.rs
  reads `/XYZ` as is (nulls None, a zoom of 0 none),
  `outline_dest(page, view)` and
  `write_outline(pdf, nodes, page_obj)` take no frames,
  `build_rewrite`'s frames map and source `/Rotate` read and
  incremental.rs's frames and `original_rot` are gone, and
  sheaf-render's reference walker gives PDFium's user-space position
  (`view_of`).
- **The plugin 0.3 adapter** (`api/shared.rs`): a page's frame is its
  source page's `Geom` (`page_frame`; `to_frame`, identity for a page
  not in the document). Out, through `to_view`: `annotations()`,
  `layer_to_wit`, the OCR-layer branches of `search`,
  `pattern_search` and `words` (`layer_matches`), `text_boxes` (its
  rect in through the inverse), `outline_item` and `dest_to_wit` with
  `apply_axes`, and a region's rect (`event_to_wit`). In, through
  `from_view`: `edit_from_wit`'s AnnotAdd, AnnotUpdate and
  ConnectorAdd with `turn` the frame's rotation, and the bookmark
  edits' views in their target's frame (OutlineEdit's in the new
  target's, else the current one's). PDFium's text from the
  `TextSource` passes as it is; `page_info` is unchanged.
  `World::run` builds WIT events from the document's snapshot, and
  the app's `keep_unseen` keeps a mark's `turn` on a plugin's update.
- **The app's frames:** `Doc::pane_geom(i)` (the page with the pane's
  rotate view; `to_view` times the zoom is the screen) and
  `Doc::edit_geom(i)` (without it; where gestures build and edit
  marks), `Doc::reveal(i, box)` for Find, Patterns, the comments
  panel's jump and `jump_to_annot`, and `DocView::page_space`. The
  pointer is pane → PageSpace → edit frame (`page_point_k`);
  `mark_at` hits each mark laid out upright, `handle_at` gives the
  edit-frame mark, `foreign_at` maps the PageSpace `/Rect`, and
  `measure_point` snaps to the grid and guides as the pane draws
  them. `annot_elements`, `outline_elements` and `caret_element` lay
  out `a.upright()` and map it through
  `Affine::upright(a.turn).inverse().then(to_pane)`; Find and pattern
  hits, the selection, selection outlines, handles, the editor's
  place, other tools' outlines, the region overlay and the print
  preview's markup draw through the pane frame. `Hit.rects` is
  `Vec<Box2>` in PageSpace, `merge_source_hits` maps PDFium's boxes
  through the source page's `from_view`, and the text selection,
  hover, `over_text` and `pick_number_at` take PageSpace. The outline
  panel's `current_view` and `go_to_bookmark` go through
  `apply_axes`, `bookmark_pattern_hits` maps a hit's top as shown
  less 12 pt back, `Region` is PageSpace, and the script hooks'
  `window_point` keeps the file's frame.
- **Making and editing marks:** `Annot::to_page(&frame)` maps a mark
  made in a view of its page through `from_view()` and adds the
  frame's rotation to its turn; `to_frame` is its inverse. The app,
  the plugin adapter and markup_appearance's and comments.rs's
  `placed` use them. `DocView::add_annot` takes a mark in the page's
  edit frame through `to_page`, for every tool (free text, square,
  ellipse, line, arrow, ink, note, tick, stamp, attach file,
  connector, callout, polygon, polyline, cloud, the measure tools,
  calc's tape and the selftest hooks), and `add_page_annot` takes
  marks already in PageSpace (text markups, replies, statuses).
  `Drag.moving` and `Resize.orig` hold edit-frame copies, `dragged`
  translates one, and a move, a resize and an adopting drag commit
  through `to_page`, which their previews draw through too. Text
  markups and the Replace and Insert carets take `PageChars.upright`
  as their turn, and a highlight
  dragged where there's no text the edit frame's. Calc's picked ticks
  are laid out in the text's reading frame, and Update Tape resizes
  in the tape's upright frame.
  `set_status::status_edits(st, geom, id, spec, stamp_text, height, sign)`
  lays out the border and the stamp on the page as it shows.
- **The clipboard, version 2** (`COPIED_MARKS`):
  `copied(marks, &Geom)` holds each mark in its page's edit frame
  (`to_frame`), and `pasted(marks, &Geom, shift)` moves them `shift`
  points down and right as seen and takes them in through `to_page`;
  `copy_marks` copies each through its own page's frame,
  `paste_marks` pastes through the current page's, and a version 1
  clipboard isn't pasted.
- **Deleted:** `annot::{PageGeom, visible_box}`, exchange's `Geoms`,
  crop's `unrotate`, the summary's `rotate_box` and
  `Picture.{source, rotation}`, `PdfFile::page_geometry`;
  `Rebase.shift`, `TextLayer::{shifted, page}`, `PageChars.page`,
  `text_layer::{rotate_box, rotate_pt}`; sheaf-core's `PageFrame`
  with its tests and sheaf_doc's re-export, `Walk::{frames, frame}`,
  sheaf-render's `frame_of`; `OcrPage.{rotation, size}`,
  `PageSource::page_size`, `RenderHandle::page_size` and
  `Msg::PageSize`; markup_tools' `rotate_pt` and `unrotate_pt`,
  docview's `rotate_rect`; incremental's `apply_rotation`;
  `/SheafTick<id>`, which `rg SheafTick crates` finds only in the v1
  test.
- **The ledger:** 27 rows for slice 5, every name above but the
  format (`annot::PageGeom`, `annot::visible_box`, `exchange::Geoms`,
  `crop::unrotate`,
  `comments::summary::{rotate_box, Picture::source, Picture::rotation}`,
  `file::PdfFile::page_geometry`, `model::Rebase::shift`,
  `text_layer::TextLayer::{shifted, page}`,
  `page_chars::PageChars::page`,
  `text_layer::{rotate_box, rotate_pt}`, `PageFrame`,
  `open::outline::Walk::{frames, frame}`,
  `pdfium::bookmarks::frame_of`, `job::OcrPage::{rotation, size}`,
  `job::PageSource::page_size`, `RenderHandle::page_size`,
  `Msg::PageSize`, `markup_tools::{rotate_pt, unrotate_pt}`,
  `docview::rotate_rect`, `incremental::apply_rotation`).
  `QUALIFIED_ONLY` gains `shift`, `page`, `frame`, `frames`,
  `rotation`, `size`, `PageSize` and `source`. Run over `crates/` as
  of c4432c7 (slice 4), every row hits (204 hits, no earlier
  slice's).
- **Docs:** `docs/document-model.md` has a new section, "Coordinates:
  PageSpace": the one conversion, the pane, edit and upright frames,
  absolute rotation and `render_delta`, a crop moving nothing,
  `turn`, `/SheafAnnot` v2 (the digest's keys, its normalizing and
  what it leaves out, and why), v1 from the standard keys, stale
  marks, copied marks, OCR's `upright`, `/XYZ`, regions, the plugin
  0.3 frame, PDFium's text until slice 14, and the clipboard. The
  Core idea's `PageRef` sketch, "Rendering never waits", "What the
  viewer changes" (the Job's delta), Saving (`/Rotate` written
  absolute, marks in v2, rotation kept after a save), the Decisions
  bullet on `PageRef.rotation` (absolute, and why the original is
  free since slice 3), the open's bookmark positions and the Text
  layers section (PageSpace words, absolute `upright`, no rotation
  deltas to reset) are current. `docs/plugins.md` says 0.3's frame is
  unchanged. The plan's transitional ledger names the wrappers' users
  (5 to 9), where stale marks go (5 to 8) and the copies' renames the
  old writers follow (5 to 9).

Deviations from the plan:
- **Mark `turn` isn't in the slice's bullets,** but §3.6's `Mark` has
  it and §5's paste "derives `turn`". Without it a mark's text would
  turn with every rotate, and once `PageGeom` went the writer had no
  frame left to draw it in. It's on today's `Annot`, serde default
  R0, written into the v2 JSON. A new mark's turn is its page's
  rotation (`to_page`), a text markup's its text's
  (`PageChars.upright`), and a mark a plugin adds the frame's
  rotation.
- **Widget hit testing has no app counterpart yet** (M10 slice 10
  adds one; form_bar.rs only shows the XFA bar), so the Check
  compares `Geom::to_view` with PDFium's own page-to-device mapping
  of each widget's centre on turned corpus pages, and PDFium's
  device-to-page of that point lands in its `/Rect` within 1/16 pt.
  The PDFium wrappers in `pdfium::frames` are `page_to_device` and
  `device_to_page`: clippy's `wrong_self_convention` rejects a
  private `from_*(&self)`.
- **The corpus `to_view` check runs on 404 pages by default,** those
  whose frame is more than the plain y flip (`/Rotate` not 0, or a
  visible box off the origin): loading all 24,251 pages under the one
  PDFium lock takes about 100 s. The every-page variant is ignored,
  as history_tests' 4,096 cases are, and passes (Measured).
- **The digest leaves out `/IRT`, `/RT`, a reply's or a status's
  `/Rect`, and `/AP`.** `write_page_annots` and `exchange::gather`
  set `/IRT` and `/RT` after the mark is written, and `/IRT` is a
  reference a rewrite renumbers; a reply's and a status's `/Rect` is
  copied from the parent then too (save.rs). So `staleness` takes
  `threaded: bool`: the writer can't tell a reply from the dictionary
  when it computes the digest, and the reader knows from the JSON's
  kind. `/AP` isn't the mark's: other tools regenerate it.
- **The writer puts the digest's numbers on its steps** (found in
  review): the digest rounds each number to 1/64 pt or 1/1000, and
  the writer wrote whatever f32 it computed, so one near the middle of
  two steps moved into the next when another tool wrote it again with
  fewer digits, and the mark opened as that tool's (15 of 200 random
  squares at three decimals, every one at two; 356 of 9,200 marks of
  every kind at three). `annot::on_grid`, run in `to_pdf_in` before
  the digest, snaps `/Rect`, `/QuadPoints`, `/InkList`, `/L`,
  `/Vertices` and `/CL` to 1/64 pt and `/C`, `/IC` and `/CA` to
  1/1000, and makes `/RD` the inset from the snapped `/Rect` to the
  snapped inner box, so `/Rect` less `/RD` is within half a step and a
  v1 reading stays within 0.01 pt. A value on the grid needs at most
  six decimals and comes back to its step from any rewrite to two
  decimals (geometry) or three (colors); the digest and the grid share
  `steps_per_unit(key)` and `steps`.
- **No `"tick": id` in the JSON.** A tick has no identity beyond its
  `AnnotId`, which every mark's JSON carries as `id`, as v1's did,
  and `/SheafTick<id>` was its only other copy, which nothing read.
  Ticks write `/Name /Tick`.
- **PDFium's text stays in the source page's displayed frame until
  slice 14** (`page_chars`, `search_page`, `pattern_search_page`,
  `text_boxes_in_rect`, the 16× device trick). The app converts it
  where it arrives, through the source page's `from_view`
  (`PageChars::from_engine`, `merge_source_hits`, pattern_panel,
  calc_panel); the plugins' `TextSource` passes it on unchanged,
  since that frame is 0.3's.
- **The plugin 0.3 frame is "the page as the file displays it"**
  (docs/plugins.md): the source page's `Geom`, with its own `/Rotate`
  and nothing sheaf adds. Bug 20's WIT half (page-info's size
  includes sheaf's rotation, its coordinates don't) stays until 0.4's
  matrices (slice 20). The adapter's helper is `page_frame`, not
  `frame_of`, which the ledger retired; `edit_from_wit` and
  `event_to_wit` take an `Option<&Snapshot>`, since a call can run
  with no document (nothing is mapped then).
- **Bug 20's last convention, the tape's turned frame, ends as a
  style:** a tape is stored in PageSpace with its `turn` like every
  mark, and lays out in its upright frame. Its `look.rotation`
  (`tape_frame`, `tape_to_shown`, `resized`'s `along`) is PDFlyer's
  option to turn the tape's text a quarter within its box, which the
  user sets (groups.rs's turn), not a frame anything is stored in, so
  it stays: a tape turned on a turned page has both, the page's in
  `turn` and its own in the look.
- **The outline frame conversions had readers too:**
  open/outline.rs's `Walk::{frames, frame}` and sheaf-render's
  reference walker (`frame_of`). With the writers' gone, sheaf-core's
  `PageFrame` and sheaf_doc's re-export had no user, and went in the
  same step.
- **The rotate conventions reached past the app:** text_layer.rs's
  `rotate_pt` and `rotate_box` (page_chars.rs and sheaf-render's
  ocr_layer test used the second), the summary's `rotate_box`,
  docview's `rotate_rect` and crop's `unrotate`; and a third geometry
  reader bug 20 doesn't list, `PdfFile::page_geometry`, used only by
  examples/roundtrip.rs, which now prints `Source::geom`. All are
  gone. markup_tools' `rotate_pt` and `unrotate_pt` and docview's
  `rotate_rect` went in step 6, not 7: nothing used them once the
  app's view was converted, and clippy fails dead functions in the
  app.
- **Geometry follows PDFium's rules now:** `geom_of` and `page_geom`
  read through `Geom::read`, so a crop box outside the media box
  shows nothing, as PDFium shows it, where `visible_box` fell back to
  the media box (`geometry_follows_the_crop_box` asserts it). And the
  model and both saves use quarter turns: a `/Rotate 135` reads as
  90, and a rewrite, or an incremental save that writes a new page
  tree, writes 90 where it wrote 135; an incremental save that keeps
  the tree writes a page's `/Rotate` only where its rotation isn't its
  source page's, so an untouched 135 stays, its marks rewritten or
  not.
- **A turned page says its `/Rotate`, 0 too** (a fix beyond the
  bullets): an incremental save that keeps the page tree used to
  remove `/Rotate` from a page turned back upright, which then
  inherited its parent's. Deciding that from the attributes it pulls
  down (step 1's `parent_turns`) missed a page with a `/Rotate` of its
  own under a parent with another, and removed the key from any page
  whose marks changed, so a page's own `/Rotate 0` under a turned
  parent turned with a mark added (9 pages of the corpus's signed
  safedocs/0047234.pdf are that shape). Now a page whose rotation
  isn't its source page's says it, 0 included, and any other keeps
  the key it has, its own or the one pulled down; only the new tree,
  whose root has none, leaves R0 out
  (`a_page_turned_upright_stays_upright_through_a_save`, four cases,
  fails either way).
- **The writers' interim arithmetic is gone:** step 1 wrote the
  header and footer's turn as
  `pref.rotation.degrees() − geom.rotate`, since there's no
  `Rotation::from(i64)` and `from_degrees(135)` is R0 where
  `Geom::read` makes it 90; step 2 replaced it with a `Geom`
  (`geom_of(&dict, pref.rotation)`), so stamps and watermarks draw
  through `from_view()` with the absolute rotation.
- **Fresh ids come from an rng seeded with the source's `BlobId`,**
  not fastrand's global one, so the app's open and the command line's
  give the same ids (`the_app_opens_as_the_command_line_does`
  compares them). §4's per-source seeded rng for other tools'
  annotations stays slice 8's. Beyond the bullets: a renamed copy
  that has another tool's `/NM` keeps it (only its id changes), and
  isn't in `renamed` or the notice, since nothing in the file
  changes; a reply or status copied with its parent on the same page
  follows the parent's new name; `opened` drops the renamed pages
  from `saved_annots`, or an incremental save would skip them and
  keep the old `/NM`; the app hides `sheaf:<old>` and
  `sheaf:<old>:strike` on those pages, at ready and when inserting or
  replacing pages; and the notice shows only when no other is
  showing, so a damaged-form notice keeps priority.
- **Another tool's reply to a copy follows it** (found in review):
  the old writers knew our marks by the file's `/NM`, so a save
  counted a copy, which the file names `sheaf:<old>`, as deleted and
  took out its other-tool replies with it, by either save kind. The
  document keeps each source's renames (`set_source_renamed`, by
  `SourceId` and page index, so a crop's derived page and a version
  from before a save find theirs, and a rebased primary has none),
  and `write_page_annots` knows each of our marks by the file's name
  and the name it has now: replies are re-pointed through the second,
  and a deleted copy's replies go by the first. The open names the
  copy by its new name in those replies' `parent` (`follow_renamed`),
  so the panel threads them under it; where a mark on the page still
  has the old name (a copy beside its original), both leave them
  with that one.
- **`file::sheaf_mark` takes the `/Annots` entry, not the
  dictionary,** so a v1 Replace finds its StrikeOut by `/IRT` to the
  entry's object, with `/RT /Group` and Subtype StrikeOut rather than
  the PART key, since `exchange::gather` sets no PART. A v1 mark
  whose standard keys can't give its geometry (a Replace with no
  StrikeOut) is another tool's comment. `is_ours` takes the page's
  `/Annots` and asks `sheaf_mark`, so the old writers can't strip a
  mark the open didn't read; `strip_sheaf_annots`,
  `write_page_annots`' before-map, `annots_and_cached` and the
  attachments listing call it, and the listing now shows a stale
  attachment mark as another tool's (it skipped anything with
  `/SheafAnnot`). The page's `/Annots` comes as a `file::PageAnnots`,
  which indexes the page's StrikeOuts once, when a v1 replacement
  first asks (found in review: a search of the page per replacement
  made a page of N replacements without StrikeOuts O(N²) to open and
  to save, 22 s at 12,000, and `/Annots` has no cap).
- **`text_string` is the outline's decoder without `title`'s
  control-characters rule,** which is PDFium's `GetTitle`'s alone;
  the `/SheafAnnot` JSON decodes through it too (it was `pdf_text`).
  `from_json` returns `Payload<Annot>`; `Payload<M>` is public, and
  the writer serializes it with `M = &Annot`. So the writers no
  longer write a string as is merely because it's ASCII (found in
  review): PDFDocEncoding reads 0x18–0x1F as accents and 0x7F as
  undefined, so a mark whose text had a DEL read back with a NUL in
  its JSON, which serde rejects, and opened as another tool's
  comment, and other viewers showed a U+0018 as a breve. Those
  strings are UTF-16BE now (`file::is_pdf_doc_ascii`), in both
  encoders; the payload keeps decoding as PDFium decodes text.
- **The v1 Check has two off-by-a-rotation cases,** `/Rotate 90` with
  the JSON's frame 0 and `/Rotate 0` with 270, beside each `/Rotate`
  alone. A v1 mark's turn is the page's `/Rotate`: on a page sheaf
  had turned, its old appearance was drawn upright in the old frame,
  so its text now reads in the file's.
- **The fuzz input is the annotation objects themselves**
  (`n 0 obj … endobj`, numbered from 4) after a fixed catalog, pages
  node and page turned 90° whose `/Annots` lists one entry per
  object, with no cross-reference table (LazyPdf rebuilds it by
  scanning). Its seeds are lopdf-written slices of every kind in v2
  and a v1-shaped copy, `marks-N`. fuzz/Cargo.lock also picked up
  slice 4's missing `sheaf-progress` edge.
- **`group_members` moved off DocState** to `Document` and `Snapshot`
  (the plan's second option): it sorted by `bounds().y`, which in
  PageSpace runs bottom-up, so it orders by the page's view and needs
  the pages' geometry. Not in the slice's list; it keeps connector
  links and Previous/Next in reading order.
- **"Derived crops keep user space" was true already:**
  `cropped_copy` and `derived_copy` copy the page dictionary and set
  only `/CropBox`, keeping its `/Rotate`. The work was deleting the
  shift (bug 12's half: nothing shifts now, so widgets and bookmark
  views no longer differ from marks) and building the crop box
  through `from_view`. Step 2 kept an interim OCR-only shift (the
  words' source-frame translation) until step 4 put the words in
  PageSpace.
- **The OCR job's engine `page_size` check went:** its `Geom` comes
  from the page table, which is PDFium's sizing (slice 3, and step
  1's frames tests), so there was nothing left to correct; with it
  went `PageSource::page_size`, its `RenderHandle` impl,
  `RenderHandle::page_size` and `Msg::PageSize`, the job their only
  user. `real_page_size_wins_over_the_apps_guess` became
  `a_turned_page_is_read_as_it_shows`. `OcrSource::recognize` takes
  the page's `Geom` and the render delta, which the host builds from
  the snapshot, since the app's `EngineOcr` holds only render
  handles.
- **`Annot::upright` gives the shape, not the mark** (found in
  review): it cloned the whole mark (author, text, dates, a tape's
  rows) to lay out its geometry, per mark on every frame
  (`annot_elements`), every click (`mark_at`) and every
  `page_bounds()` (groups, the comments list). It maps the kind alone
  now; `bounds` moved to `AnnotKind`, `Annot::hit` takes a point in
  the upright frame and lays out the shape itself, and the app's
  `outline_elements` takes the shape beside the mark (whether it's
  closed now comes from the shape).
- **The app converted moves, resizes and the adopting drag in step
  6,** with the view, not step 7: `drag_preview` is the view's, and
  `handle_at` gives the edit-frame mark, so the commit had to map
  back the same way for the preview and the result to agree.
  `mark_at` hits a mark laid out upright, not `a.mapped(&edit)`, so a
  measurement's label and a callout's end are hit where they're drawn
  when the mark's turn isn't the page's. `Hit.rects` changed type to
  PageSpace `Box2`. `current_view` keeps recording the left edge with
  the top, as Acrobat records an `/XYZ`. `bookmark_pattern_hits` maps
  PageIds to indexes once rather than searching per hit. The editor
  sits below a mark as shown, where it sat below it in the source
  frame.
- **`Annot::to_page` and `to_frame` are sheaf-doc's,** not a `Doc`
  method or the app's functions, so the app's gestures, the paste
  functions and Set Status, the plugin adapter and the tests' `placed`
  share one conversion. An edit-frame copy's turn is relative to the
  frame (`to_frame` subtracts the frame's rotation, `to_page` adds
  it), which is what the clipboard holds too. Calc's picked ticks are
  laid out in the text's reading frame (`picked` takes `upright`), not
  the edit frame, and Update Tape resizes in the tape's upright frame:
  the edit frame agrees with either only when sheaf hasn't turned the
  page, and neither frame is a `Geom`, so they map through
  `Affine::upright` themselves. `status_edits` takes the page
  geometry and the signing closure and returns finished edits, since
  `to_page` needs the whole signed mark.
- **The paste Check runs both ways,** A to B and B to A, within 1/64
  pt: with B at `/Rotate` 0 the one-way test couldn't catch a dropped
  turn (by mutation).
- **The crop/highlight Check saves incrementally with
  `incremental::incremental`,** as a recovery sidecar does: a page
  shown from a derived copy counts as removed content (M9 bug 14), so
  a save in place would be a rewrite. Its page's media box is off the
  origin, the highlight is made as the app makes one
  (`PageChars::from_engine` and `rects`), and the glyphs are found
  among PDFium's characters on the saved page, not through
  `PageChars`, which groups lines in the base frame and so reads the
  turned text a character a line. It fails when
  `PageChars::from_engine` drops its `from_view`.
- **Test names differ from the plan's here and there:**
  `a_missing_axis_stays_missing`;
  `words_come_back_through_to_view_for_every_frame` for
  `rotation_round_trips_for_every_frame`,
  `search_in_a_turned_frame_returns_page_space_rects`,
  `content_turns_with_the_upright_frame` and
  `positions_are_in_page_space`; the crop-leaves-OCR check is
  sheaf-ops' `a_crop_leaves_ocr_text_on_its_words`, which also has
  PDFium find the saved word. The every-kind round trip is
  open/tests.rs's and writes through `save::write_page_annots`,
  companions and threads included, checking each `/Rect` against the
  PageSpace bounds padded by the stroke. markup_appearance places
  every mark through the page's `from_view` and adds `/Rotate` 180
  and 270; ocr_layer.rs gives its page an offset crop box in every
  case.
- **Smaller choices:** `exchange::import` lost its unused `sources`;
  a private `stamp_lib::visible_array`, not `visible_box`, which the
  ledger matches by its last segment; `scan::add_text` lost its
  `pages` and `fit`; sheaf-plugin has lopdf as a dev-dependency for
  the turned fixture; the ledger rows
  `comments::summary::Picture::{source, rotation}` (with `source`
  qualified-only), since the summary's source frame was a convention
  too.

Measured (AMD EPYC 7302P, 32 threads, release builds unless marked):
- **The property test:** 256 cases (`/Rotate` × the pane's rotation ×
  a media box anywhere in ±5,000 and 1 to 14,400 pt × an inset crop
  box × points in ±20,000, both compositions, within 1/16 pt) in
  under 10 ms.
- **`to_view` against PDFium on the corpus** (4,392 files): 404 pages
  whose frame isn't the plain one and 3,242 widgets on turned pages,
  both in 2.0 s (the test profile); every one of the 24,251 pages in
  102.7 s (ignored; run again on the final code). Both default tests
  fail when `upright(R90)` is wrong.
- **The 7k harness,** one full run at the end (slice 4's in brackets;
  heavy ready 377–426 ms in three runs):

  | Row | Synthetic | Heavy | Scanned |
  |---|---|---|---|
  | geometry walk | 50.3 ms [48.2] | 73.7 ms [72.5], asserted | 61.3 ms [61.4] |
  | ready | 49.6 ms [50.8] | 426 ms [391; 416], asserted | 446 ms [539; 449] |
  | ready, peak RSS / added | 28.6 / 21.8 MB [29.1 / 22.0] | 278 / 263 MB [278 / 264] | 514 / 507 MB [514 / 507] |
  | render 1024 px, cold / warm | 29.5 / 5.18 ms [30.1 / 5.49] | 749 / 4.87 ms [771 / 4.96] | 366 / 21.6 ms [387 / 21.7] |
  | 200 previews at 320 px | 529 pages/s [523] | 169 [168] | 151 [151] |
  | incremental save, one mark | 75.6 ms [58.7] | 141 ms [140] | 965 ms [992] |
  | Rewrite | 248 ms [285] | 2,530 ms [2,922] | 777 ms [865] |
  | crop all pages / the edit | 339 / 1.25 ms [398 / 1.56] | 1,726 / 1.26 ms [2,197 / 1.58] | 1,356 / 1.17 ms [1,605 / 1.63] |
  | Bates apply / its Rewrite | 113 / 633 ms [112 / 716] | 258 / 3,147 ms [256 / 3,731] | 56.6 / 1,166 ms [87.4 / 1,445] |
  | flatten | 265 ms [291] | 2,994 ms [3,617] | 1,279 ms [1,470] |

  Every RSS row is within 3% of slice 4's (the Rewrite's peak, the
  heavy's 557 MB [555], the scanned's 1,582 [1,619]). History:
  AnnotAdd 0.700 ms [0.695] and a full reorder 0.940 ms [0.950],
  asserted; 200 OCR versions as before (4,074 MB of steps). The
  synthetic incremental save is noise, not slice 5: 66–79 ms in four
  runs, and c4432c7, built from `git archive` and run the same hour,
  61–79 ms in three. The writers' rows are faster (Rewrite 10–13%,
  crop 15–21%), part of it code placement, which the harness's note
  puts at up to 1.46×, and part work gone: crop's plan no longer
  loads each source eagerly for `page_geom`, and the writers no
  longer read each page's source `/Rotate` or build outline frames.
  No gated row regressed; the ready gate's fixtures have no sheaf
  marks, so ready on them gets a row of its own (next). Two heavy runs
  after review: ready 425 and 402 ms, the incremental save 143 and
  146 ms, crop 1,708 and 1,703 ms, and the writer rows back near
  slice 2's (Rewrite 3,037 and 3,175 ms, Bates' Rewrite 3,939 and
  3,992, flatten 3,658 and 3,538), with nothing on their path changed
  (the heavy has no sheaf marks, so no page reaches `on_grid` or the
  renames): the build-to-build swing the harness's note describes.
- **Ready with 20,000 sheaf marks:** first measured one-off
  (`open::file`, median of 7, throwaway crates): the history bench's
  7,000 pages and 20,000 squares saved by Rewrite (17.7 MB) opened in
  594 ms, 508 ms on c4432c7's code (built from `git archive`) and 62
  ms with no marks, so slice 5 added 86 ms, 4.3 µs a mark. Alone, on
  that file: the annotation pass 425 ms, the digest 61 ms (3.1 µs a
  mark) and the JSON 42 ms; the rest of the pass is LazyPdf reading
  20,000 dictionaries, as before. 10,000 of the 20,000 being copies
  (ids repeated on other pages) added 9 ms for renaming them (603
  ms). A v1 mark reads in about 3.4 µs (step 3). After review it's a
  gate row, `ready_at_7k_pages_and_20k_sheaf_marks`: that document,
  the last 10,000 copies of the first, written by today's Rewrite
  into the gate directory each run (16.8 MB, its numbers on the
  digest's grid) and opened to ready, median of three, asserted under
  1 s: 513, 499 and 491 ms in three runs, so 499 ms is its slice 5
  baseline.
- **Fuzzing:** `sheaf_annot` ran 2 minutes on the final code from
  step 3's corpus, 3,367 runs, its corpus 806 to 942 inputs, no
  crash.
- **Suites:** `cargo test --workspace` 655 passed, 19 ignored, in 88
  s (625 and 17 at slice 4); sheaf-doc's unit tests 324 in 20.7 s
  (308 in 19.1 s). The ledger walks 242 files in 1.6 s; over
  c4432c7's `crates/` (a `git archive` copy, the test built in a
  scratch crate, debug) 8.1 s.
- **The app by hand** (Xwayland, the `--script` driver and Gdk window
  grabs; fixtures built by throwaway scripts, not in the tree): on
  pages at `/Rotate` 90 with a crop box, a media box at (−37, 52),
  `/Rotate` 270 with a crop box, a plain page and one turned in
  sheaf, each with sheaf marks, another tool's square and `/XYZ`
  bookmarks, with and without rotate view: marks, other tools'
  comments, Find hits, a text selection, a region and bookmark jumps
  landed where expected; a corner resize grew as shown; every tool
  drew on all four pages, and the incremental and rewritten saves
  render pixel-identical in `pdftoppm` and match the screen;
  reopened, every mark was where it was drawn; three marks copied
  from the `/Rotate` 90 page and pasted, in place on the offset and
  `/Rotate` 270 pages and shifted on their own page, landed where
  they were seen and read the same way; Set Status, calculator picks
  and tapes read upright on every page. Not covered: the Stamps
  panel's tool (the tick's path), Update Tape and a Replace caret
  (the driver can't double-click).

Found:
- **Before this slice a mark on a page sheaf had turned moved through
  a save** (inferred from the code, then pinned): `rebase_to_saved`
  pointed the pages at the saved file, whose `/Rotate` included
  sheaf's turn, and reset the delta, but never converted the marks,
  which were in the old displayed frame; and the v1 JSON on such a
  page was in the base frame while the file's `/Rotate` was base plus
  the turn. `a_mark_on_a_rotated_page_stays_put_through_a_save`
  failed until step 2 put marks in PageSpace, and the v1 reader takes
  geometry from the standard keys, so such files read right.
- **An incremental save could turn a page back** (fixed, deviations):
  removing `/Rotate` from a page turned upright let it inherit its
  parent's, and step 1's fix still missed a page with its own
  `/Rotate` under a parent with another, and any page whose marks
  changed.
- **`measure_point` snapped in the frame without rotate view**
  (fixed): in a view-rotated pane the grid and guides were off.
- **A cloudy polygon's scallops are clipped on their outer side in
  the saved file:** the writer pads `/Rect` by the stroke plus 1
  around the outline, as it did before; sheaf's own overlay draws
  them whole. It belongs to the appearance code (`annotation()`'s
  padding).
- **Limits, unchanged:** GPUI can't turn text, so a free text or
  stamp whose turn isn't the pane's draws its text unturned in its
  turned box; `PageChars::from_engine` groups lines in the base
  page's view frame, so text that runs down the page as shown
  selects, picks and underlines a character a line (slice 14's text
  service); selecting a mark doesn't clear the selected other tool's
  comment.
- **The digest's limits:** it doesn't cover a replacement's StrikeOut
  companion, so another tool's edit of the companion alone is
  overwritten at the next write; and with the numbers on its steps, a
  tool that writes geometry with one decimal or colors with two, or
  truncates geometry to two, can still move a number across a step,
  so its re-save reads as an edit.
- **A page the tree lists twice shares its `/Annots`,** so its second
  listing's marks now get fresh ids and a notice at open (slice 8's
  copy 0 and copy 1).
- **A recovery sidecar from before this slice** holds `TextLayer`
  JSON with `page` and in the displayed frame, and its words would
  read in the wrong frame. No shim (pre-alpha); slice 9 changes the
  sidecar.
- **Costs not measured, each linear:** the adapter's `page_frame`
  searches the pages for each call (per `annotations(page)`, per
  outline item with a view, about 7 million comparisons for the
  heavy's 2,030 bookmarks; per region event; per edit), `copy_marks`
  and `add_annot` search once per mark, and the writer clones each
  mark once per write, on the Rewrite gate's path, which didn't move.
- **Found in review, fixed:** a turned tape's picture was cached by
  its kind, color and text, not its turn, which `tape_pdf` lays it
  out by, so two tapes alike but for their turn shared one picture
  (stamp_preview.rs's key has `turn` now); and the app's ready looked
  up each renamed mark's page and its place in the hidden names by
  searching the pages, O(renamed × pages) on the UI thread (maps now).
- **Not this slice's:** deleting a page's last mark leaves its page
  with no marks (`AnnotsOf` none), so both writers only strip ours
  from it (`strip_sheaf_annots`), and another tool's reply to the
  deleted mark stays, its `/IRT` at an object the page no longer
  lists; `write_page_annots` takes such replies out only while the
  page keeps a mark. Slice 8's `annot.thread` cascade, over others'
  annotations as records, covers it.
- **Under the `--script` driver only:** after a tool change clears
  the selection, the properties bar hides but the pane's fit-page
  layout catches up on the next render after a press, so the first
  scripted drag after it ended a few points off; and a note's editor,
  opened at mouse-down, doesn't take focus (a free text's, opened at
  mouse-up, does).

Checked by tests:
- `to_view ∘ from_view` is the identity within 1/16 pt over `/Rotate`
  × the pane's rotation × crop boxes × media boxes off the origin:
  `geom::tests::to_view_and_from_view_undo_each_other` (256 cases),
  with `visible_corners_land_on_the_shown_page` (each turn's corners,
  media box at (−37, 52), an offset crop box),
  `a_missing_axis_stays_missing`, and sheaf-render's
  `pdfium::frames::to_view_is_pdfium_s_page_to_device` (and its
  every-page variant);
- every mark kind round-trips on `/Rotate` 0, 90, 180 and 270 with
  offset boxes: `every_mark_kind_round_trips_on_every_rotate` over
  `forms::fixture::every_mark_kind()` (exhaustive over `AnnotKind`;
  within 1/64 pt, `turn` kept, nothing foreign, each `/Rect` the
  bounds padded), with `a_turned_mark_lays_out_as_it_reads`,
  markup_appearance's
  `text_markup_draws_where_it_should_turned_further` and
  `a_mark_on_a_rotated_page_stays_put_through_a_save`;
- a highlight cropped, rotated, saved and reopened covers the same
  glyphs: sheaf-ops'
  `a_cropped_rotated_highlight_still_covers_its_glyphs` (Rewrite and
  incremental; rects within 1/16 pt, PDFium's glyph boxes on the
  saved page inside them through its `Geom`), with
  `margins_crop_and_marks_follow` and `margins_crop_the_shown_sides`;
- a bookmark `/XYZ` on a cropped page lands on the same content:
  `a_bookmark_on_a_cropped_page_lands_on_the_same_content` (unchanged
  after a save, and `to_view` of it on the square's top-left in
  PDFium's render within a pixel), with `positions_are_in_page_space`
  and `bookmark_positions_survive_saves_and_our_rotation`;
- a v1 mark's rects equal v2's within 0.01 pt:
  `a_v1_mark_reads_as_its_v2_self` (every kind at each `/Rotate`,
  with a perimeter and an area beside the distance, so `/Vertices`
  is read for measurements too, and two JSON frames off by a sheaf
  turn);
- a stale mark opens as another tool's comment and isn't reverted:
  `a_stale_mark_is_another_tools_comment` (version 2's digest and
  version 1's `/M`, each with an adoptable mark, absent from the
  marks; Rewrite and incremental keep the edited dictionary);
- a mark re-serialized by another tool stays a mark:
  `a_reserialized_mark_stays_a_mark` (marks off the grid by 0.0077
  pt, reals written to three decimals, UTF-16BE `/Contents`, `/Rect`
  corners swapped; it fails without `on_grid`), with
  `a_mark_s_text_reads_back_as_written` (a DEL and 0x18 in `/Contents`
  and `/T`);
- a page of v1 replacements reads in linear time:
  `a_page_of_v1_replacements_reads_in_linear_time` (10,000, open and
  strip within 5 s; 0.4 s, and 29 s when the index is built per
  replacement);
- a page duplicated by another tool opens with distinct ids and one
  notice: `a_page_copied_by_another_tool_gets_fresh_ids` (the first
  keeps its id, one `renamed` entry each, the copied reply's thread,
  another tool's reply to the copy in its thread at the open and
  after either save, its `/IRT` at the copy, the same ids on a second
  open, the new `/NM` after either save, and the reply gone with the
  copy) and the app's `renamed_marks_are_hidden_by_their_old_names`;
- widgets land where PDFium puts them on turned corpus pages:
  `widgets_land_where_pdfium_puts_them` (asserting it compared some);
- a paste keeps the place as seen across `/Rotate` and crop:
  `paste_keeps_the_place_as_seen_across_rotate_and_crop` (both ways,
  in place and 10 pt down and right, the relative turn kept);
- absolute rotation: `an_inserted_page_keeps_its_own_rotate`,
  `a_rotation_survives_a_save`,
  `a_page_turned_upright_stays_upright_through_a_save` (no `/Rotate`
  under a parent's 90 turned upright, its own 90 under a parent's 180
  turned upright, its own 0 under a parent's 90 and its own 135, each
  only marked and keeping its key in an incremental save) and
  `rebase_after_save_keeps_ids_and_history`;
- OCR and text in PageSpace:
  `words_come_back_through_to_view_for_every_frame`,
  `search_in_a_turned_frame_returns_page_space_rects`,
  `content_turns_with_the_upright_frame`, page_chars'
  `a_turned_page_reads_as_it_shows`, sheaf-ocr's
  `layer_lands_in_page_space` and
  `a_turned_page_is_read_as_it_shows`, ocr_layer.rs and
  `a_crop_leaves_ocr_text_on_its_words`;
- the plugin frame:
  `a_plugin_works_in_the_page_as_the_file_shows_it`,
  `a_region_arrives_as_the_file_shows_the_page` and
  `page_info_is_the_turn_sheaf_added_and_the_size_shown`;
- the app's gestures: `a_drag_moves_a_mark_as_the_page_shows`,
  `a_handle_reshapes_a_mark_as_the_page_shows`,
  `a_mark_drawn_on_a_turned_page_reads_as_drawn` and
  `a_status_is_laid_out_on_the_page_as_it_shows`, each on all four
  turns with offset boxes; groups in reading order:
  `a_group_runs_by_page_then_down_the_page_as_it_shows`;
- ticks: `a_tick_is_named_tick`; the fuzz seeds:
  `the_mark_fixtures_read_as_marks` and
  `fuzz_bodies_survive_mutated_fixtures`; ready on 20,000 sheaf marks
  at 7,000 pages, the 7k harness's
  `ready_at_7k_pages_and_20k_sheaf_marks` (ignored, as the gates are);
- the ledger: `nothing_retired_is_back` with the 27 rows.

Left for later slices:
- **Slice 8:** `Mark` records with `turn`; stale marks as `Foreign`
  records (the transitional row, 5 to 8); §4's per-source seeded ids
  for other tools' annotations; copy 0 and copy 1 for a page listed
  twice, whose second listing's marks are renamed today.
- **Slice 9:** the `geom_of` and `page_geom` wrappers (5 to 9), with
  the old writers; the recovery sidecar's format.
- **Slice 11:** Op JSON in PageSpace; `MoveMarks`' `Delta`.
- **Slice 14:** the text service in PageSpace, ending the 16× trick
  and the conversions where PDFium's text arrives
  (`PageChars::from_engine`'s base-frame grouping,
  `merge_source_hits`, the plugins' `TextSource` frame).
- **Slice 20:** WIT 0.4's to-view and from-view, and bug 20's
  page-info half.
