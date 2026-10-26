<img src="assets/icons/sheaf-f2-transparent.svg" alt="" width="96" align="right">

# sheaf

A fast, native PDF editor. Rust + GPUI.

Sheaf aims for feature parity with Adobe Acrobat Pro's editing tools, plus the
workpaper tooling from CCH ProSystem fx **PDFlyer** (tickmarks, connectors,
calculator tapes, bookmark-driven page management) — without the bloat. It's
built first for tax and audit preparers working in large scanned binders.

> **Status:** pre-alpha, with no releases yet. Daily binder work (viewing,
> page tools, bookmarks, markup, comments, OCR, export, print) works today;
> filling forms, redaction and signatures are still to come. See
> [What works today](#what-works-today) and [Progress](#progress).
> Settings files and the command line may still change.

## Why

Acrobat has become slow enough that routine work — opening a 400-page scanned
workpaper binder, reordering pages, dropping a tickmark — is painful,
especially on older hardware. PDFlyer makes the workflow tolerable but is
Windows-only and runs inside a paid copy of Acrobat Standard or Pro.

Sheaf's bet is that a PDF editor written in Rust, with a GPU-accelerated UI and
a render pipeline designed around tiling and caching, can be an order of
magnitude snappier than Acrobat on the same machine.

## Goals

1. **Fast.** The real test is whether it feels obviously snappier than
   Acrobat on the same (older) machine. Directional targets, not release
   gates:
   - Cold start to empty window: **< 300 ms**
   - Open a 1,000-page scanned PDF to first painted page: **< 500 ms**
   - Page flip (cached): **< 1 frame**; (uncached): first tile visible < 100 ms
   - Idle CPU: **0%**. No background daemons, no updater, no telemetry.
   - Memory: a bounded tile cache, not "load every page bitmap and hope".
2. **Correct.** Round-trip arbitrary PDFs without corrupting them. Incremental
   saves by default so the original bytes are preserved.
3. **Acrobat Pro parity** for the editing feature set (see
   [the feature matrix](docs/features.md)).
4. **PDFlyer parity** for the tax/audit workpaper workflow.
5. **Keyboard-first.** Every action has a binding; the mouse is optional.
6. **Native, single binary.** No Electron, no browser engine, no runtime.

### Non-goals (for now)

- Cloud / collaboration / "Studio Sessions"-style review.
- Mobile.
- Being a PDF *library* for other programs (though the core crates may be
  reusable).
- Pixel-perfect rendering of every exotic PDF on day one. We lean on a
  mature renderer for that.

## Platforms

| Platform | Tier | Notes |
|---|---|---|
| Windows (x64) | **1** | Primary daily-driver target. Must be fast on older hardware. |
| Linux (x64, Wayland + X11) | **1** | Development platform. |
| macOS (arm64) | 2 | Should mostly fall out of the Linux work. Not blocking releases. |

Tier 1 platforms gate every release. So far sheaf is used and tested day to
day on Linux; CI builds and tests it on Linux, Windows and macOS.

## One document per window

Every open PDF gets its own window; there are no document tabs. Switching
and arranging documents is left to the OS window manager.

- **File → Open…** (`Ctrl+O`) opens one or more PDFs, each in its own window
  (the first fills the current window if it's empty). A file that's already
  open brings its window forward instead.
- **File → New Window** (`Ctrl+N`) opens an empty window.
- **File → Close** (`Ctrl+W`) closes the document; the last window stays
  open, empty. **Close Window** (`Ctrl+Shift+W`) always closes the window,
  and `Ctrl+Q` quits. Unsaved documents prompt first.

`sheaf one.pdf two.pdf` opens two windows. While sheaf is running, opening
a file from the file manager or with `sheaf file.pdf` hands it to the
running sheaf instead of starting another. `sheaf --new-instance` runs a
separate one beside it.

## What works today

A summary; [docs/features.md](docs/features.md) has every feature and its
status.

- **Viewing.** Large and scanned PDFs, encrypted ones too (with a password
  prompt). Single page, two-up and continuous; zoom, rotate view, split
  view, full screen and read mode; rulers, grid and guides. Thumbnails,
  bookmarks, attachments, layers and history panels. PDF Portfolios open
  for browsing and extracting. Recent files, Document Properties, and a
  toolbar you can customize.
- **Finding text.** Find in document, and a Patterns panel for regex
  searches (SSNs, EINs, amounts, dates, and presets of your own). Select
  and copy text.
- **Pages.** Reorder, rotate, delete, extract and duplicate pages. Insert
  another PDF with its bookmarks merged, blank pages or images. Replace
  pages, crop, find and remove blank pages. Combine Files, and split by
  page count, bookmark, size or ranges. Page labels. Headers and footers,
  Bates numbering, watermarks and backgrounds, written as Acrobat writes
  them, so either can update or remove them later.
- **Bookmarks.** Build and nest them, move a bookmark with its pages, color
  them, PDFlyer's Set Status, and bookmarks made from page ranges or
  pattern matches.
- **Markup.** Notes; highlight, underline, strikethrough and squiggly;
  Acrobat-style text edits; free text, callouts, shapes, clouds and ink.
  Move, resize, restyle, copy and paste marks. Stamps: built-in, your own
  libraries (from images or PDF pages), and dynamic stamps with your name
  and the date. PDFlyer's tickmarks, connectors and calculator tape, which
  can pick numbers straight off the page. Measuring distance, perimeter and
  area to scale. File attachments.
- **Review.** A Comments panel with threads, replies, status and
  checkmarks, filters and sorting (PDFlyer's Stamp and Note Summary
  included). Other tools' comments can be edited. Comment summaries as PDF,
  CSV or text; import and export as FDF or XFDF.
- **Scanning and OCR.** Phone photos or a scanner's pages to a clean PDF:
  page found and straightened, turned upright, deskewed, shadows evened out,
  in color, gray or black and white. Images to PDF. OCR with Tesseract or
  PaddleOCR into an invisible, searchable text layer, over a whole binder
  in the background. Scanning straight from a scanner isn't there yet.
- **Forms, so far.** Forms open and display, and survive the page tools (crop, watermark, insert, extract, combine and the rest).
  Saves respect hybrid XFA forms, Reader usage rights and signatures.
  `sheaf form check` reports what other viewers would trip on. Filling
  fields comes next.
- **Saving.** Incremental saves in milliseconds that keep the original
  bytes. What you delete or replace is gone from the saved file, not left
  in an earlier revision. Encrypted files stay encrypted. A signed file
  keeps its signatures, or sheaf asks before breaking them. A file another
  program changed is never overwritten unasked. A file sheaf writes whole
  (a Save As, an export) packs its objects into object streams when the
  original did, so it doesn't grow. Crash recovery, and an edit history
  you can step back through, branches included.
- **Output.** Export pages to PNG, JPEG, TIFF (black-and-white G4,
  grayscale or color) or text. Export a flattened copy, or a clean copy
  without comments and marks. Print with sheaf's own preview (CUPS on
  Linux and macOS, GDI on Windows).
- **Plugins.** WebAssembly plugins with their own commands, forms and
  panels, and only the access you allow. Bundled: Tax Form Bookmarks,
  Amount Cross-Reference, Find Duplicate Pages, Subtotal, Currency Convert
  and Bookmark from Pattern. To write one, see
  [docs/plugins.md](docs/plugins.md).
- **Command line.** `sheaf <subcommand>`, or the GUI-less `sheaf-cli`,
  runs work headless: edits from a JSON file, inspect, combine, split,
  export, scan, images to PDF, remove blanks, comments and form check. See
  [docs/cli.md](docs/cli.md).
- **Keyboard and settings.** A command palette, your own key bindings, and
  a settings file. See [docs/configuration.md](docs/configuration.md).

Not yet: filling or creating form fields, redaction, setting passwords,
digital signatures, editing page text and images, compare and optimize.
[Progress](#progress) says when.

## Progress

Work is grouped in milestones. There are no dates or version numbers:
everything is pre-release, and the order follows what each piece needs
first. [ROADMAP.md](ROADMAP.md) has the detail.

**Done**

| Milestone | What it brought |
|---|---|
| M0–M4: The fast workpaper binder | Viewer, page and bookmark organizer, markup with PDFlyer's tickmarks, connectors and calculator tape, comments, clean copy, command palette, key bindings, crash recovery |
| M5: Scan & OCR | OCR, scan cleanup, phone photos, images to PDF. Scanning from a scanner and separator sheets are deferred until there are scanners to test against |
| M6: Plugins | WebAssembly plugins, a plugin manager, permissions you approve |
| M7: Viewing & output | Page modes, split view, full screen, rulers, attachments, portfolios, layers, printing, image, TIFF and text export |
| M8: Page tools | Combine, split, insert, replace, crop, blank pages, page labels, headers and footers, Bates numbering, watermarks |
| M9: Markup completion | Editing marks, the remaining annotation types, custom and dynamic stamps, replies and status, Set Status, FDF/XFDF, comment summaries, measurement, edit history |
| M10, part 1: Forms groundwork | The form check; forms that survive page tools and saves |
| M10.5: The edit API | One set of edits behind the app, `sheaf edit` and plugins. For you: saves that really remove what you delete and keep encryption, and the same edits from the command line and plugins |
| The content interpreter (from M11) | Reading what each page draws: the groundwork for redaction, content editing and accessible documents. Nothing new on screen yet |
| Object streams | Smaller files when sheaf rewrites a file (a Save As, an export, a save that removes something): an IRS form now shrinks by about an eighth, where it grew by half. File > Properties says whether Fast Web View still holds after a save |

**Next, in this order**

| Work | What it brings |
|---|---|
| Fast Web View | Linearized files, which show page 1 before the rest arrives over a network; a choice in Save As |
| Leaner rewrites | Rewriting a large file without loading all of it into memory |
| `sheaf search` | Text search across files and folders from the command line |
| Screen-reader labels | sheaf's buttons, fields and dialogs announced to screen readers (the first part of M17) |
| M10, part 2: Filling forms | Filling text, check box, radio and choice fields so they look right in every viewer; flattening fields; form data in and out (FDF, XFDF, CSV, JSON) |
| Fill & Sign (from M12) | Typed, drawn or image signatures and initials, saved for reuse |
| M11: Security & redaction | True redaction, by hand or by pattern; removing hidden information; setting, changing and removing passwords |

**Later**

| Milestone | What it brings |
|---|---|
| M10, part 3: The rest of forms | Calculations and form scripts, creating and editing fields, detecting fields on flat forms |
| M5, the rest | Scanning from a scanner (WIA, TWAIN, SANE) and separator sheets that bookmark or split a scanned batch |
| M12: Digital signatures | Signing with certificates, verifying, timestamps, certifying |
| M13: Content editing | Editing text in place, adding and editing images, links |
| M14: Document processing | Compare, optimize and reduce file size, PDF/A |
| M15: Automation | Batch actions over files and folders; a final parity audit against Acrobat Pro and PDFlyer |
| M16: Agent tools (MCP) | An AI agent (Claude Code, Codex or another MCP client) reads a document and proposes edits, which you approve before they're made |
| M17: Accessibility | sheaf usable with a screen reader and the keyboard; accessible (PDF/UA) documents |

Not planned: XFA forms, Office ↔ PDF conversion (use LibreOffice), creating
portfolios, cloud features and mobile.

## Getting sheaf

There are no releases yet, so build from source.
[docs/building.md](docs/building.md) has the system packages for Linux and
Windows and the full steps; after them it comes down to:

```sh
scripts/fetch-pdfium.sh          # the PDFium renderer
scripts/fetch-ocr-data.sh        # OCR language data and models
scripts/build-plugins.sh         # the bundled plugins
cargo run -p sheaf-app
```

Then:
- [docs/cli.md](docs/cli.md): the command line, its subcommands and exit
  codes.
- [docs/configuration.md](docs/configuration.md): settings, key bindings,
  where sheaf keeps its files, and crash recovery.
- [docs/plugins.md](docs/plugins.md): writing plugins.

For developers: [docs/architecture.md](docs/architecture.md) (crates and key
decisions), [docs/building.md](docs/building.md) (tests, the test corpus,
updating GPUI, packaging), [docs/document-model.md](docs/document-model.md),
[docs/oracles.md](docs/oracles.md) and [ROADMAP.md](ROADMAP.md).

## Name

A *sheaf* is a bundle of papers. It's also short, unclaimed on crates.io
and most package managers, and not an Adobe trademark.

## License

Dual-licensed under MIT or Apache-2.0, at your option. Third-party engines:
PDFium (BSD-3), Tesseract and its glyphless `pdf.ttf` (Apache-2.0),
Leptonica (BSD-2), ONNX Runtime (MIT), PaddleOCR models and `oar-ocr`
(Apache-2.0); see `packaging/licenses/`. Barcodes are read and QR codes made
with rxing (Apache-2.0). MuPDF is deliberately not used (AGPL). lopdf (MIT)
is vendored with a patch in `vendor/lopdf`. The Arlington PDF Model's TSVs
(PDF Association, Apache-2.0) are vendored in `vendor/arlington-pdf-model`
for tests only, and parts of the test-only checker that reads them are
adapted from its TestGrammar (Apache-2.0; see
`crates/sheaf-doc/src/arlington/check.rs`). Text in the standard 14 fonts
is laid out with Adobe's Core 14 AFM metrics (Adobe's notice) and the Adobe
Glyph List (BSD-3), in `crates/sheaf-doc/assets`; text in fonts over
predefined CJK CMaps with Adobe's CMaps (BSD-3), as hayro-cmap
(Apache-2.0/MIT) bundles them, decompressed by the brotli crates
(BSD-3/MIT): `LICENSE-cmap-resources` and `LICENSE-brotli` in
`packaging/licenses/`. Embedded font programs are read following FreeType's
routines, which libpdfium also builds in: portions of this software are
copyright © 2026 The FreeType Project (www.freetype.org), under the
FreeType License (`LICENSE-freetype`).
