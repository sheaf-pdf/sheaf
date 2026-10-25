# M5: Design

Part of [M5: Scan & OCR](README.md).


How the pieces fit, before the proposals that choose between ways of
building them.

```
 sheaf-scan-worker (a process): SANE | TWAIN | WIA | fake     image files (photos, a copier's TIFFs,
      │ RawPage { pixels, dpi, sheet, side }, over a pipe         │ a ScanSnap's PDF opened as a document)
      │                                                          │ decoded pages
      ▼                                                         ▼
 sheaf_ops::intake, a page at a time:
   clean (profile) → blank? → separator? → OCR? → a batch of pages
      │ every few pages: InsertPages of a Memory(Images) source + SetOcrText,
      │ one group, one undo step (GroupKind::Scan)
      ▼
 a document: the open one (Insert from Scanner), a new untitled one
 (Create from Scanner / Scans), or a Session (`sheaf scan`)
      │                                          any PDF, open or on the command line
      ▼                                                         │
 sheaf_ops::separate::find  ◄──────────────────────────────────┘
      │ sections: where each separator is, what it read, title, level
      ▼
 review → one commit: AddBookmark per section, DeletePages of the separators
        → and/or a split: Export::extract per section, written to a folder
```

- **New crate `sheaf-scanner`:** devices and nothing else. It lists
  scanners, reads what each can do, scans with settings, and hands back
  pages as pixels with their resolution. It knows no PDFs, no documents
  and no windows (a CI gate keeps it that way, as `sheaf-image`'s does).
  Its backends (`sane`, `twain`, `wia`, and `fake` for tests; `ica` if
  ask 4 says so) run only in its worker binary, `sheaf-scan-worker`;
  the app and the command line use its client, which starts the worker
  and talks to it over pipes (proposal 4).
- **`sheaf_image::codes` (new module):** reading barcodes and patch codes
  in a page image, and the geometry of a QR code to draw. Pixels only,
  inside `sheaf-image`'s gate.
- **`sheaf-ops`:** `intake` (from today's `scan.rs`: the page-at-a-time
  pipeline, for files and devices alike) and `separate` (find, plan,
  apply, split).
- **`sheaf-edit` / `sheaf-doc`:** one new `PageSource::Separator { sheet }`
  and `MemoryKind::Separator`, made by `memory::separator`, so a sheet is
  an insert like a blank page.
- **`sheaf-cli`:** `scan` gains devices; new `separate` and
  `separator-sheets`.
- **`sheaf-app`:** the Create from Scans dialog gains scanners as a
  source; Insert Pages > From Scanner; Document > Separate… with a review;
  Document > Insert Separator Sheets…; File > Create > Separator Sheets…;
  documents with no file yet (ask 1).
