# Architecture

```
┌─────────────────────────────────────────────────────────┐
│  sheaf-app          GPUI application, views, commands;   │
│                     one process per user: the            │
│                     instance-handoff crate               │
├────────────────────────────┬────────────────────────────┤
│  sheaf-cli (no GUI)        │  sheaf-ops: operations     │
│  clap subcommands          │  shared by app and CLI;    │
│                            │  progress + cancel: the    │
│                            │  in-progress crate         │
├────────────────────────────┴────────────────────────────┤
│  sheaf-ui           Reusable widgets: page canvas, tile  │
│                     viewport, sidebars, command palette  │
├────────────────────────────┬────────────────────────────┤
│  sheaf-doc                 │  sheaf-render              │
│  Document model; ops       │  Page → tiles, cache,      │
│  lowered in transactions   │  background render pool    │
│  (pages, outline, annots), │                            │
│  undo/redo, planned saves  │  trait Renderer            │
│                            │   └ pdfium (MVP)           │
│  lopdf / own object layer  │   └ hayro (later, pure Rust)│
├────────────────────────────┴────────────────────────────┤
│  sheaf-edit         The edit vocabulary: ops, ids, refs, │
│                     specs; types and JSON, for plugins   │
│  sheaf-util         Byte helpers: checksum, file reads   │
└─────────────────────────────────────────────────────────┘
```

## Key decisions (and why)

| Decision | Choice | Rationale |
|---|---|---|
| UI framework | **GPUI** (`gpui` + `gpui_platform`, from gpui-pre) | GPU-accelerated, retained-mode, proven in Zed. Pre-1.0 with breaking changes; pin versions ([Updating GPUI](building.md#updating-gpui)). Linux backend is wgpu (Blade was removed Feb 2026). M0's spike passed ([the spikes](../plan/complete/m00-spikes.md)); Windows' launch time is [B60](../plan/backlog/B60.md). |
| Rasterizer | **PDFium** via `pdfium-render` | Chrome's engine: fast, BSD-licensed, handles encrypted files, forms, broken PDFs. Hidden behind a `Renderer` trait so a pure-Rust engine (hayro) can replace it once mature. |
| Document edits | **sheaf's own model and writer**, over **lopdf**'s objects | PDFium's write API is limited (esp. outlines). Every edit is an op (`sheaf_edit::Op`), lowered in a transaction into the document model's records, never into the file's object graph. sheaf's own emitter writes a save (`serialize/`, `materialize/`) as an incremental update or a rewrite, as the save is planned (below). lopdf supplies the object types files are read as, a source at a time, and parses what's still loaded whole: a Rewrite's graph, the form check, a stamp collection. An edited page renders from an overlay: an append section built in memory over the file's bytes, nothing written to disk or loaded again. |
| Rendering strategy | **Tiled, async, cached** | Viewport requests tiles at the current zoom; one render thread, the engine pool, renders them off the UI thread; images are cached by what the page draws (its render key) and the tile, so a reorder, a save or an edit to another page re-renders nothing, and a page shows its last image until its new one lands. Low-res preview tile first, then full-res. This is the single biggest "feels fast" lever. |
| Text / OCR | PDFium text extraction. OCR is a **core feature** behind an `OcrEngine` trait with two engines compiled in by default: **Tesseract 5** (built from source, linked statically) and **PaddleOCR PP-OCRv5** on ONNX Runtime (`oar-ocr`, with Microsoft's runtime library loaded on demand from `ocr/lib`). | Both engines are native C/C++, so OCR can't live in a WASM plugin; the trait keeps engines swappable, and plugins get OCR as a host capability. Judged on scanned tax forms, not purity. |
| Save model | Incremental update by default; Save As is a save to another file | Preserves originals, fast saves on large files (one mark on a 500 MB scan in about 21 ms). Clean rewrite available for size reduction / "Export Clean Copy". A save is an output: it writes the version on screen and records what it wrote, and the document, its undo history and what the screen shows stay as they were. sheaf plans each save: back at what the file holds, nothing is written; otherwise an incremental update, journaled so a crash mid-write is undone at the next open, unless something was removed (a page, a mark, a reply, a bookmark) or text the file already holds was replaced (a comment's text, a bookmark's title, OCR text; a regenerated page number aside), the file can't take one, or the update would leave removed text readable, when the file is rewritten whole, so what was removed or replaced isn't left in an earlier revision; the save says why. A file another program changed since sheaf opened or last saved it is never overwritten unasked: Save As…, Overwrite or Cancel, and a document recovered after a crash is checked against its file the same way. A signed or certified file keeps its signatures when what changed is within what they allow (a certification's permissions, Reader's usage rights), Save As included; otherwise sheaf asks: Append Anyway (an update that leaves the signed bytes as they were, what it removed still readable in the earlier revision), Rewrite (which breaks them) or Cancel. No save or copy carries a page it doesn't keep: links, named destinations, the open action, form fields, tags and article threads that pointed at one are dropped or pruned. |
| Undo | A tree of versions, each with the edits that made it | Every edit is an op (`sheaf_edit::Op`), lowered in a transaction into the changes it makes. A version keeps a snapshot (cheap, structure-shared) and those changes from its parent; undo, redo and jumps move between versions, an edit after undo starts a branch instead of discarding, and a trimmed snapshot is rebuilt by replaying them. An OCR run or a plugin command is one undo step, however many results it lands, and an edit planned in the background (an insert, a crop, a dialog's change) lands on whatever you did meanwhile, or tells you what it conflicts with and changes nothing. History is kept within 256 MB, the oldest versions going first. Per-document. |

## Efficiency principles

- Never render what isn't visible. Never render on the UI thread.
- Cache aggressively, bound strictly.
- Lazy-parse: open the xref and page tree, nothing else, until asked.
- No work at idle. If the window isn't focused and nothing changed, 0% CPU.
- Prefer boring data structures. Profile before optimizing.
