# M10.5 §16: Every feature on the new API

Part of [M10.5: the design](README.md).


| # | Feature | New expression | Slice |
|---|---|---|---|
| 1 | Move / reorder | `MovePages` → `Put Order`; Append rewrites `/Pages` nodes in place (+ labels by `emit_deps`) | 10, 11 |
| 2 | Rotate | absolute `Geom.rotate`; the render job carries the delta | 5 |
| 3 | Delete pages | `DeletePages` + rules page.annots, page.bookmarks, page.labels, page.generators, widget.field | 11 |
| 4 | Insert file | `InsertPages { pages, name }`, one form instance, notices, `Relower` | 11, 12 |
| 5 | Duplicate | `DuplicatePages` (copy + 1, private annotations, new form instance) | 11 |
| 6 | Replace | `ReplacePages` (same ids, base swap) | 11 |
| 7 | Crop / Trim White | `SetPageBoxes`; Trim White = content-only render job, `Validate` | 15 |
| 8 | Watermark / background | `SetWatermark` / `RemoveWatermark` generators; `ImageRef`; `replaces` | 16 |
| 9 | Header/footer + Bates | `SetHeaderFooter` / `RemoveHeaderFooter`; live tokens; frozen Bates per block | 17 |
| 10 | Strip ours and Acrobat's | filters by origin; `PageRes` pruning; authored only on request | 16, 17 |
| 11 | OCR layers | `SetOcrText` / `ClearOcrText`; blocks; content-only input; OCR group | 8, 18 |
| 12 | Sheaf marks | `AddMark` / `PatchMarks` / `MoveMarks` / `DeleteAnnotations`; v2 digest; stable objects | 5, 6, 10, 11 |
| 13 | Page / custom stamps | `Mark.stamp = FormRef::Blob`; `place_form`; library stamps as `Memory(Library)` | 6, 19 |
| 14 | Tapes | marks with derived AP; turned pictures in memory | 6 |
| 15 | Connectors | `AddConnector`; `/Dest` resolved symbolically at emit | 9, 11 |
| 16 | Measurements | unchanged kinds, in PageSpace | 5 |
| 17 | Attachments (marks) | `Mark.file = BlobRef`, embedded once per file | 6, 8 |
| 18 | Others' comments | `AnnotBody::Foreign`; `DeleteAnnotations`; `AdoptAnnotations` (keeps source keys, drops `/RC` on text change); hiding is a view input | 8, 11, 13 |
| 19 | Outline | per-item `Bookmark` records; emitted in place by provenance | 8, 9 |
| 20 | Page labels | `SetPageLabels`; anchors validated; re-anchor cascade | 9, 11 |
| 21 | Fields / widgets | `FieldNode` tree with `FormInst`, widgets as annotations (merged flag, props), `FormDoc`; M10 slices 4–5's rules as lowering + `rules::plan` | 8, 9, 10 |
| 22 | Blank pages | `InsertPages { from: Blank }` → `Memory(Blank)` | 19 |
| 23 | Images → PDF, scan | `InsertPages { from: Images }`; scan = Session + `SetOcrText` + save | 19 |
| 24 | Combine | open first + one Tx; the app hands the Document to a new window | 11, 19 |
| 25 | Extract / split | `KeepPages` presets | 19 |
| 26 | Flatten (export, print with markup) | `FlattenAnnotations` with `PinnedAp` | 19 |
| 27 | Clean copy, print without markup | `RemoveMarkup` + `RemovePrivateData` (with `StripLegacy`) | 19 |
| 28 | Print, page images/text, print preview | ExportTx → bytes → pool; preview realizes the print state | 14, 19 |
| 29 | Layers (OCG) | `ViewInputs.ocg_off` | 13 |
| 30 | Comments import (XFDF) | `AddMark` / `PatchMarks` with `AnnotRef::Nm` and `imported` (`/NM`, dates, author kept), `Relower` | 12, 19 |
| 31 | Remove blanks, comment summary | export presets; summary on ContentWriter as a `Memory(Summary)` source | 6, 19 |
| 32 | Recovery | Full (slice 9), then Append sections with form transforms and a removals flag, encrypted, chained | 9, 10 |
| 33 | Save in place | planner from effects; SaveRecord; in-place append; no rebase | 9, 10 |
| 34 | Stamp library | stays outside the edit API (user files); placing a stamp interns its form | 6 |
| 35 | Set Status, Color by Level, Bookmarks from Pages, paste, calculator | UI macros emitting op batches with locals through `Tx::reader` | 11 |
| 36 | Replace mark (Caret + StrikeOut) | companion recognized by `/IRT` + `/RT /Group` to a sheaf Caret | 9 |
| 37 | Plugins | WIT 0.4 JSON ops, assets, decision reads | 20 |
| 38 | Save As | a Save; copy-and-append for signed documents | 9, 10 |
