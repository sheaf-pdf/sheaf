# M5: Scan & OCR

Status: deferred · Done except scanner intake and separator sheets. The intake pipeline, untitled scan documents (#41) and reading codes (slice 3) land; the rest is deferred (Greg, 2026-10-23) until there are scanners to test against

Proves: Scanned binders become searchable · Depends on: Nothing unbuilt

## Summary

- ✅ OCR to an invisible, searchable text layer, per page or over a whole
  binder, in the background: `sheaf-ocr` with an `OcrEngine` trait and two
  engines, Tesseract 5 (static) and PaddleOCR PP-OCRv5 on ONNX Runtime
  (`oar-ocr`); Document > Recognize Text (OCR)…; one undo step; Find and
  Patterns search unsaved layers. Design in [`plan/complete/m05-ocr.md`](../complete/m05-ocr.md).
- ✅ Scan cleanup (`sheaf_image::clean`): deskew, despeckle, auto-rotate by detected
  text orientation (word shape, plus PaddleOCR's orientation model when
  present), black-and-white by Sauvola thresholding. Blank-page removal
  (M8, `sheaf_ops::blank` over `sheaf_image::blank`): "Leave out blank
  pages" when scanning, Document > Find Blank Pages… with a review before
  deleting, and `sheaf remove-blanks`.
- ✅ Camera capture: phone photos of paper. Find the page and correct the
  perspective, clear the background left at its edges, even out shadows,
  lighting and white balance, and optionally flatten curled pages (UVDoc,
  opt-in). File > Create > From Scans/Photos…, with a preview of each step,
  and `sheaf scan`. Design in [`plan/complete/cleanup-cli.md`](../complete/cleanup-cli.md).
- ✅ Create PDF from images (JPEG/PNG/TIFF/BMP/GIF/WebP; multi-page TIFF in;
  JPEG kept as is, black and white as CCITT G4): `sheaf_doc::images`,
  `sheaf images-to-pdf`.
- Scanner intake: WIA/TWAIN on Windows, SANE on Linux.
- Separator sheet → bookmark: a generic barcode/patch-code/keyword separator
  that splits or bookmarks a scanned batch. This replaces PDFlyer/CCH Scan
  "Target Sheets". Includes inserting separator sheets into an existing
  PDF above chosen pages, so it can be re-run (PDFlyer's Insert Target
  Sheets; moved here from Pending 2026-10-14).
- Scanner intake and separator sheets: design in
  the files linked below (planning).

Slices: OCR engine + text layer ✅ → batch OCR over binder ✅ → image import ✅
→ cleanup filters ✅ (with camera capture) → scanner intake → separator sheets.

**Deferred after slice 3 (Greg, 2026-10-23):** the plan is in
the files linked below. Its slices 1–2 (the intake
pipeline, and untitled documents that scans stream into; #41) and 3
(reading barcodes and patch codes) land. Slices 4–10 (separator sheets,
separating, the scan worker and SANE, scanning in the app, TWAIN, WIA, and
the docs) wait until there are physical scanners to test against, such as
the office's Fujitsu fi-6130s. Without them they aren't worth the time
now. Run-time loading of the LGPL TWAIN DSM and of libsane, which slices 6
and 8 need, is ruled fine ([build-or-adopt.md](../research/build-or-adopt/README.md),
D17).

**Decided at kickoff (2026-10-11):** OCR is a core feature, not a plugin
(Tesseract and ONNX Runtime are native code and can't run in wasmtime), with
both engines in-process behind a trait so they can be compared on scanned
tax forms and swapped later.

## Where it stands

Status: **deferred after slice 3** (Greg, 2026-10-23): "without physical
scanners to test against it's not a valuable use of time or tokens right
now". Slices 4 and 5 don't need a scanner but have little value without
one, so they wait too. Slices 1–2 landed in #41 and slice 3 in its own PR;
slices 4–10 resume once there are scanners to test against (the office's
Fujitsu fi-6130s). D17 (loading the TWAIN DSM and libsane at run time,
which slices 6 and 8 need) is ruled allowed. Planned 2026-10-23, one
commit per slice, with notes per slice at the end, as in M7–M10.5.

**Kickoff (2026-10-23):** the proposals and asks 1–11, 13 and 14 are
taken as recommended, under Greg's standing instruction to take a
recommended answer rather than wait, and stay open to his review until
the slice that depends on each starts. Ask 12 is answered (Greg,
2026-10-23): the reference scanners are the office's Fujitsu fi-6130s.

The rest of M5 is done: OCR ([`ocr.md`](../complete/m05-ocr.md)), scan cleanup and camera
capture ([`cleanup-cli.md`](../complete/cleanup-cli.md)), image import, blank pages
([`blanks.md`](../complete/m08-blanks.md)). This plan covers the two slices the roadmap
left: scanner intake, and separator sheets.

## Slices

Ordered by ask 2's recommendation: the shared groundwork (the pipeline
and untitled documents), then separators, then scanners. If ask 2 says
scanners first, slices 6–9 move before 3–5; nothing else changes.

| Slice | Title | Status | Depends on |
|---|---|---|---|
| [1](../complete/m05-scan/s01-intake-pipeline.md) | The intake pipeline | done | — |
| [2](../complete/m05-scan/s02-documents-with-no-file-yet.md) | Documents with no file yet | done | — |
| [3](../complete/m05-scan/s03-reading-codes.md) | Reading codes | done | — |
| [4](slices/s04-separator-sheets.md) | Separator sheets | deferred | — |
| [5](slices/s05-separate.md) | Separate | deferred | 4 |
| [6](slices/s06-scanners-worker-sane.md) | Scanners: the worker and SANE | deferred | 5 (--separate) |
| [7](slices/s07-scanning-app.md) | Scanning in the app | deferred | 5, 6 |
| [8](slices/s08-windows-twain.md) | Windows: TWAIN | deferred | 6, 7 |
| [9](slices/s09-windows-wia.md) | Windows: WIA | deferred | 6 |
| [10](slices/s10-docs-roadmap.md) | Docs and the roadmap | deferred | 4–9 (wrap-up) |

## Plan files

- [Proposals](proposals/README.md): one file each
- [Scope](scope.md)
- [What's in place](in-place.md)
- [Design](design.md)
- [Found while planning](found.md)
- [Asks](asks.md)
- [Risks](risks.md)
- [Not in M5](not-m5.md)
- [Sources](sources.md)
- Research: [Scanners](research/scanners.md), [Separators](research/separators.md), [Reading and writing codes](research/reading-writing-codes.md)
- Finished slices, with their notes: [complete/m05-scan](../complete/m05-scan/)
- [Build or adopt](../research/build-or-adopt/README.md): the M5 rows, and the [changes to make](../research/build-or-adopt/changes-to-make.md) at kickoff
- Open questions: [asks](../asks.md)
