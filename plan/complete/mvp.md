# MVP (v0.1): "the fast workpaper binder"

The MVP is **not** an Acrobat clone. It is the smallest thing that lets a tax or
audit preparer stop opening Acrobat for daily binder work, and it is the
vehicle for proving the performance thesis.

A user should be able to:

1. Open a 1,000-page scanned binder and be scrolling in under half a second.
2. Navigate by bookmarks and thumbnails without stutter.
3. Reorder, rotate, delete, and insert pages — including inserting another PDF
   and having its bookmarks merged sensibly.
4. Build and reorganize the bookmark tree, moving pages along with bookmarks.
5. Drop tickmarks and connectors from the keyboard, add notes and highlights,
   and run a calculator tape onto the page.
6. Search text.
7. Save incrementally in milliseconds, and export a clean copy.

Everything marked 🎯 in the [feature tables](../../docs/features.md) is in scope. Nothing else is.

## Explicitly deferred from MVP
Content (text/image) editing, OCR, forms, redaction, signatures, TIFF export,
measurement, compare, batch. These are all real Acrobat parity items and all
come after v0.1.

After v0.1 come Scan & OCR (M5) and plugins (M6), which together make v0.2, then the
rest of the path to parity in [ROADMAP.md](../../ROADMAP.md).

## Milestones

| # | Milestone | Proves |
|---|---|---|
| M0 | **Spikes**: GPUI window on Windows and Linux (both tier 1); pdfium-render renders a page; lopdf round-trips a PDF. *All three pass on Linux and Windows (2026-10-11).* | The stack is viable |
| M1 | **Viewer**: tiled async rendering, continuous scroll, zoom, thumbnails, outline, multiple windows, search. *Done 2026-10-11 (Linux).* | The performance thesis |
| M2 | **Organizer**: page reorder/rotate/delete/insert, bookmark tree editing, incremental save, undo; pattern (regex) search as its own tool. *Done 2026-10-11 (Linux).* | The document model |
| M3 | **Markup**: standard annotations, stamps, tickmarks, connectors, calc tape, comments panel, clean copy export. *Done 2026-10-11 (Linux).* | The PDFlyer workflow |
| M4 | **Polish → v0.1**: unsaved-changes prompts, settings with persisted layout, command palette, user keymap, crash-recovery autosave, packaging + CI. *Done 2026-10-11 (Linux; Windows bundle and CI workflows not yet exercised).* | Daily-driver readiness |
| M5 | **Scan & OCR**: `OcrEngine` trait with Tesseract and PaddleOCR engines, batch OCR over a binder with progress and cancel, invisible text layer written on save, search over OCR text. *OCR slices done 2026-10-12 (Linux; Windows build not yet exercised). Scan cleanup, image import, scanner intake and separator sheets still planned — see [`m05-ocr.md`](m05-ocr.md).* | Scanned binders become searchable |
| M6 | **Plugins**: WASM components (wasmtime, WIT), command registry, declarative forms and panels, document events, storage, OCR as a host capability, permissions with consent, plugin manager, bundled plugins (tax-form bookmarks, amount cross-reference). *Slices done 2026-10-13 (Linux; Windows build not yet exercised) — see [`docs/plugins.md`](../../docs/plugins.md) and [`m06-plugins.md`](m06-plugins.md).* | Extensibility without forking |
| M7 | **Viewing & output**: page modes and rotate view, split view, full screen / read mode, rulers / grid / guides, page labels, attachments, portfolios, layers, document properties, recent files, print with preview (`sheaf-print`: CUPS, GDI), export to PNG / JPEG / TIFF (G4) / text. *Slices done 2026-10-14 (Linux; printing tested against stand-in CUPS tools, the Windows print backend only type-checked) — see [`m07-viewing.md`](m07-viewing.md).* | Viewing and printing never need Acrobat |

M8 onward (viewing and output through v1.0 parity) is laid out in [ROADMAP.md](../../ROADMAP.md).
