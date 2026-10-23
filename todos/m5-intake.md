# M5 remainder: scanner intake and separator sheets

Status: **in progress** (planned 2026-10-23). Branch `m5-intake`, a
worktree cut from M10.5 slice 19 (`3e1b9f0`, on `m10.5-p6`), to be
rebased onto `main` once PR 6 squash-merges. One commit per slice, with
notes per slice at the end, as in M7–M10.5.

**Kickoff (2026-10-23):** the proposals and asks 1–11, 13 and 14 are
taken as recommended, under Greg's standing instruction to take a
recommended answer rather than wait, and stay open to his review until
the slice that depends on each starts. Ask 12 (the scanners Greg can
check by hand, and what the pilot offices use) is his to answer before
slice 6's hand checks; slices 1–5 need no hardware.

The rest of M5 is done: OCR ([`ocr.md`](ocr.md)), scan cleanup and camera
capture ([`cleanup-cli.md`](cleanup-cli.md)), image import, blank pages
([`blanks.md`](blanks.md)). This plan covers the two slices the roadmap
left: scanner intake, and separator sheets.

Research for this plan, all on 2026-10-23:
- sheaf's code at `3e1b9f0`: slice 19's `Session`, memory sources,
  exports, `sheaf edit` and the Op vocabulary, the scan path and the app's
  Create from Scans, Find Blank Pages' planner and review;
- this machine's SANE (sane-backends 1.2.1 libraries, libsane1 1.4.0,
  sane-airscan 0.99.36, hplip), with the `test` backend turned on in a
  private config directory and driven by `scanimage`;
- a benchmark of barcode reading in Rust on synthetic 300 dpi pages
  (a scratch crate outside the repo);
- the 709 scanned PDFs in `../pdf-corpus/scans/`;
- the prior-art notes (`docs/prior-art/pdflyer.md` §2.7, §4–§6,
  `acrobat.md` §2.11, `README.md`);
- outside sources, listed in [Sources](#sources).

## Scope

The roadmap's two lines, plus what planning turned up:

- **Scanner intake.** Scan straight into sheaf: File > Create > From
  Scanner and Document > Insert Pages > From Scanner in the app, and
  `sheaf scan --device` on the command line. Pages go through the cleanup,
  blank-page and OCR steps sheaf already has, into a document.
  - Linux: SANE. Windows: TWAIN and WIA (ask 3); the desk scanners tax
    software names and certifies are USB devices with TWAIN drivers, and
    tax software asks for TWAIN.
    macOS: the roadmap names nothing (ask 4).
  - Network scanners and copiers come through each system's own path
    (sane-airscan on Linux, Windows' built-in WSD and eSCL drivers under
    WIA), so sheaf speaks no network scan protocol of its own
    (proposal 5).
  - **Added:** scanner drivers run in a worker process, never in the
    app: drivers crash, hang and corrupt memory, and TWAIN's 32-bit
    drivers can't load into a 64-bit app (proposal 4).
  - **Added:** scanners sheaf can't drive. A ScanSnap has no TWAIN, WIA
    or ISIS driver at all; ScanSnap Home hands each scan to sheaf as a
    file instead, and so does a copier's scan-to-folder (proposal 11).
- **Separator sheets.** A page between documents in a scanned batch
  that says where one document ends and the next begins.
  - **Separate:** find the separators in a document (one just scanned,
    or any scanned PDF, such as a copier's scan-to-folder file) and
    bookmark there, split into files there, or both, with the separator
    pages dropped or kept, after a review. This replaces CCH Scan's
    Target Sheets.
  - **Separators of every kind:** sheaf's own sheets, which say what
    they mean in a QR code; other software's barcode sheets, by
    symbology and pattern; Kodak patch codes; and a keyword on an
    otherwise empty page.
  - **Insert Separator Sheets:** sheaf's sheets put into an existing PDF
    above chosen pages (above each top-level bookmark, named after it, by
    default), so a binder can be printed, rescanned and separated again
    the way it was (PDFlyer's Insert Target Sheets). Also a stack of
    sheets to print (File > Create > Separator Sheets).
- **Added, because the planned items need them:**
  - **A scanner profile for cleanup.** `ScanParams`' defaults are a phone
    photo's (find the page, even out the lighting); a feeder's pages need
    neither, and finding a page that fills the image can crop it wrongly
    (proposal 3).
  - **Scanned pages stream into a document,** an untitled one for Create
    (ask 1). That fixes backlog **B11** (Create from Scans writes a file
    and opens it again).
  - **Backlog F14,** an automated test of `sheaf scan --ocr`, since this
    plan reworks that path (slice 1).
- **Not in M5:** watched folders, reading scanners' own barcode and patch
  results through TWAIN, CCH's target sheets by their own encoding, and
  sorting pages by form (see [Not in M5](#not-in-m5)).

## What's in place

Slice 19 (`3e1b9f0`) rebuilt the paths this plan uses. Read from the code:

- **`Session`** (`crates/sheaf-ops/src/session.rs`): a document with no
  window, which every subcommand runs on. `Session::new_empty()` is a
  document of no pages made in memory, which a scan inserts its pages
  into; `commit`/`commit_as(actor, ops, label)` commit ops; `save(SaveTo,
  answers, p)` saves through `materialize::save_with` (`-o` a Rewrite,
  `-o --append`, `--in-place` as planned); `export` runs an `Export`. A
  save or export that stops at an ask is a `NeedsDecision` (exit 3).
- **Memory sources** (`crates/sheaf-doc/src/memory.rs`): blank pages,
  images as pages, and the empty document are small PDFs made in memory
  and interned as `Memory(kind)` blobs (`MemoryKind::{Library, Recovered,
  Blank, Images, Summary}`), which `InsertPages` brings in as a file's
  pages. The same inputs make the same bytes, so the same blob: an insert
  lowered again finds the pages it made. `PageSource` (sheaf-edit
  `op.rs`) is `File | Blank { size, count } | Images { images, fit }`,
  and `tx/resolve.rs` makes the memory source when the resolver builds.
- **`sheaf scan`** (`crates/sheaf-ops/src/scan.rs`): image files in,
  each cleaned (`sheaf_image::clean`, with the orientation votes and
  UVDoc), blank ones left out if asked, OCRed if asked, then one
  `InsertPages` of a `MemoryKind::Images` source into `Session::new_empty`
  (`made_pages`) and one `SetOcrText` per page as `Actor::Job(Ocr)`, then
  a Rewrite to `-o`. All pages are held until the end.
- **The app's Create from Scans** (`workspace.rs` `create_from_scans`,
  `scan_dialog.rs`): pick images, set options with a per-step preview,
  Create asks for a path, runs `sheaf_ops::scan`, then `open_paths` opens
  the written file (B11). Its choices persist in `state.json`
  (`State::scan`).
- **Combine's hand-off** (`workspace.rs` `show_made`, `DocView::adopt`):
  Combine saves its Session to the path the dialog asked for, then hands
  the same `Document` to a window (empty, or a new one); nothing is read
  again. `DocView.path` is a `PathBuf`: there is no document without a
  file.
- **Exports** (`crates/sheaf-doc/src/export.rs`): every way out besides
  a save is an `Export` (a version, transforms, a profile, an `Out`),
  written by `export::run`. `Export::extract(from, pages, out)` is a split
  part (`Transform::KeepPages`, headings kept, numbered as kept);
  `export::check` asks first without writing.
- **Split** (`crates/sheaf-ops/src/split.rs`): `SplitBy::{EveryN, Ranges,
  TopBookmarks, MaxBytes}`; parts are named `{stem}_{title}` with
  `file_safe` and `_2` for repeats, `{stem}_Start` for pages before the
  first bookmark; every part is checked before any is written.
- **The op vocabulary** (`crates/sheaf-edit/src/op.rs`): `InsertPages`,
  `DeletePages`, `AddBookmark { parent, at, title, target, … }`,
  `MoveBookmarks { with_pages }`, `SetOcrText`. Ops are serde, schemars,
  and plugins' and `sheaf edit`'s JSON.
- **Find Blank Pages** (`crates/sheaf-ops/src/blank.rs`,
  `crates/sheaf-app/src/blank_review.rs`): a planner over a snapshot reads
  each page as its views draw it (text first, then pixels at 60 dpi
  through `Views::render` in the background budget), then a review sheet
  with thumbnails and checkboxes commits one delete on the version it
  measured (`Base::At`, a stale page conflicts). This is the pattern for
  Separate.
- **Groups** (M10.5 §8.4, `GroupKind::{Ocr, PluginCall}`): a job's
  commits amend one version, one undo step, unless the user edits or
  saves in between; a job whose group the user undid is told
  (`GroupEnded`) and stops. `JobKind::{Ocr, Trim, Import, Insert}` name
  the actors.
- **Fonts for generated pages** (`sheaf-doc/src/content/font`): the
  standard 14 with exact widths; text a font can't encode draws `?` until
  M10 slice 7's fallback font.
- **Settings** (`sheaf_ops::prefs`): `settings.toml` is read, never
  written (`[identity]`, `[ocr]`, `[stamps]`, `[forms]`, `[plugins.*]`);
  dialogs' last choices are in `state.json`.
- **CI** builds and tests on Ubuntu, Windows and macOS runners, builds
  Tesseract and fetches the OCR data (so `--ocr` can be tested anywhere),
  and has dependency gates: `sheaf-image` stays pixels only, no OCR
  library is dynamically linked, macOS links only system libraries.
- **Plugins** (M10.5 slice 20, `sheaf:plugin@0.4.0`): reads of text and
  rendered pixels; edits as Op JSON, `Cap::Plugin` ops first. Inserting
  pages "comes later, with manifest permissions" (§14.3).

## Research

What the proposals rest on. Links are in [Sources](#sources); claims
that couldn't be confirmed are marked (unverified).

### Scanners

- **What tax offices scan with** (what tax software names and
  certifies; no survey of offices was found). Desk ADF scanners, mostly
  USB: Fujitsu/Ricoh fi-7160 and fi-8170 (PaperStream IP, a TWAIN and ISIS
  driver), Canon imageFORMULA DR (TWAIN and ISIS, some WIA), Epson DS
  (TWAIN, WIA, ISIS, ICA, SANE), Brother ADS (TWAIN, WIA, ISIS, SANE),
  Kodak S2000 (TWAIN, ISIS, WIA), and ScanSnaps. Tax software asks for
  TWAIN: Drake Documents, Thomson Reuters FileCabinet CS, TaxDome (WIA or
  TWAIN; it lists ScanSnap iX100–iX1600 as having neither), Scan123 (which
  calls ScanSnap "completely incompatible").
- **ScanSnap** "does not support TWAIN drivers or ISIS drivers" (PFU's
  FAQ), and no WIA driver exists. ScanSnap Home profiles can **Send to**
  any application by its executable. A ScanSnap SDK exists for Windows
  and macOS, with registration. On Linux, SANE's fujitsu backend rates
  the iX1600 "good" over USB.
- **SANE.** A small C API (`sane_init` … `sane_exit`), no thread-safety
  promise (calls on one handle mustn't overlap; `sane_cancel` is safe
  asynchronously). Batch scanning isn't in the standard: frontends call
  `sane_start` per page until `NO_DOCS`, which some backends return from
  `sane_read`. Option names are standard; the feeder's and duplex's
  values aren't (`ADF Duplex`, `ADF Front`, `Automatic Document Feeder`,
  separate `adf-mode` or `duplex` options). License: backends GPL-2.0+
  with the SANE exception, `sane.h` public domain. The one Rust crate
  (`sane-scan` 0.1.2, 2023) links at build time; ruscan (MIT) `dlopen`s.
  NAPS2 runs SANE in a worker; libinsane runs it in its own process
  because some backends corrupt memory and crash.
- **SANE's `test` backend, here.** `libsane-test.so` comes with
  libsane1 but is commented out of `/etc/sane.d/dll.conf`; a private
  `SANE_CONFIG_DIR` whose `dll.conf` says `test` turns it on without
  root. It simulates a feeder (empty after 10 sheets), gray and color,
  1/8/16-bit, unknown height, short reads, and injected statuses (jam,
  cover open, no documents, I/O error). No duplex; its area stops at 200
  mm. `scanimage -L` with this machine's own configuration took 10.7 s
  with no scanner attached.
- **eSCL** (AirScan, Mopria) is HTTP and XML, found by mDNS
  (`_uscan._tcp`). Network document scanners and copiers speak it; the
  USB desk scanners above don't (an fi-series has no eSCL; HP's USB
  ScanJets speak it only over IPP-over-USB). Each system reaches eSCL
  already: sane-airscan on Linux; Windows' built-in drivers, WSD's as
  WIA devices and, on Windows 11, eSCL's (a 24H2 bug broke discovery of
  USB eSCL devices; that every eSCL copier appears under WIA is
  unverified); macOS's AirScan through Image Capture. Mopria's specification is free to download under a
  license for internal implementation only, with no redistribution.
  sane-airscan's source lists the quirks a client must handle (503
  retries, Brother's false "ADF empty", Xerox's jam that means empty).
- **WIA 2.0** is in the `windows` crate (`IWiaDevMgr2`, `IWiaItem2`,
  `IWiaTransfer`); feeder and duplex by `WIA_IPS_DOCUMENT_HANDLING_SELECT`,
  `WIA_IPS_PAGES = 0` for the whole feeder; errors by code (paper empty,
  jam, multi-feed, cover open, warming up). WIA 1.0 and WinRT's scanner
  API scan only to files. No virtual WIA device installs without a
  person.
- **TWAIN.** Windows ships only the old 32-bit `TWAIN_32.DLL`;
  `TWAINDSM.dll` (LGPL-2.1) comes with drivers. A data source loads only
  into a process of its own bitness; NAPS2 runs TWAIN in a 32-bit worker
  because drivers are "generally 32-bit only", though PaperStream IP,
  Epson Scan 2 and newer Canon drivers have 64-bit TWAIN. TWAIN wants a
  window handle and a message loop. `twain.h` defines patch-code and
  barcode detection (`ICAP_PATCHCODEDETECTIONENABLED`, `TWEI_PATCHCODE`,
  `TWEI_BARCODETEXT`); which drivers fill them in is unverified. The TWAIN
  Working Group's sample data source is a virtual scanner (Modified BSD,
  MSI installers); a silent install on GitHub's runners is unverified.
  Rust: the `twain` crate is one 2021 release; RSWebTWAIN (Apache-2.0,
  alpha) has the 64-bit app plus 32-bit sidecar shape.
- **macOS.** ImageCaptureCore (`ICScannerDevice`, a feeder unit with
  duplex) is wrapped by `objc2-image-capture-core` 0.3.2; it reaches ICA
  drivers and AirScan devices in one API. TWAIN on macOS is gone (the
  TWAIN Bridge left in 10.9). Apple's VirtualScanner sample runs by hand,
  not in CI (unverified). Direct network scanning from an app is behind
  the Local Network privacy prompt since macOS 15.
- **Sandboxes.** No scanner portal (xdg-desktop-portal #218, open since
  2018). Flathub's Simple Scan bundles SANE and asks for every device and
  the network.
- **NAPS2** (GPL-2.0+, SDK LGPL-2.1+): WIA, TWAIN (32-bit worker), SANE
  and Apple's API in workers, and eSCL; its issues are the best list of
  driver quirks (a jam that persists after clearing; Kodak's backend
  defaulting to duplex; WIA's `0x210001`). Its CI runs no scanner; its
  scanner tests are a manual project ("put the test page in your
  scanner").

### Separators

- **CCH Scan's target sheets** are generic, reusable barcode sheets
  (single-page, multi-page, forced classification with a class, form or
  custom bookmark name); the sheets and cover pages are **dropped** from
  the output; the cover page's second barcode names the PDF. The
  symbology isn't documented. CCH's 2011 guide: print, don't photocopy
  ("copies tend to degrade"). PDFlyer's Insert Target Sheets puts a
  sheet "above the selected page", and its Remove only takes a sheet off
  the dialog's list before OK. Scan Funnel inserts above or below, or a
  sheet before every file automatically.
- **SurePrep's** target sheets are one Letter page each with a Code 128
  barcode of the category's name (decoded); the workpaper takes the
  sheet's name, numbered on repeats.
- **Capture software** (Kofax/Tungsten, PaperStream Capture, Kodak
  Capture Pro, Epson Document Capture Pro, ABBYY, DocuWare): separate by
  barcode (type plus pattern), patch code, blank page, page count, text;
  delete the separator or keep it (an option everywhere; Kodak keeps
  patch 2 and drops 3 and T); "separate when the value changes"; file
  names from the barcode's value and tokens. Kofax prints separator
  sheets with a patch code and an optional barcode of the form name.
- **Desktop PDF editors** (Acrobat, Foxit, PDF-XChange, Power PDF) split
  by pages, size, bookmarks, blank pages or text; none splits on
  barcodes or inserts separator sheets. Evermap's AutoSplit plug-in
  splits Acrobat files by text and names them from it.
- **Open source.** Paperless-ngx splits on a separator barcode (default
  text `PATCHT`, any symbology, the page dropped unless asked), keeps ASN
  and tag barcode pages, and reads with zxing-cpp; it dropped zbar as
  unreliable on skewed and low-contrast codes. NAPS2's "Patch-T"
  separation reads only a Code 39 `PATCHT` with ZXing.NET and drops the
  sheet. Stirling-PDF splits on its own QR divider, drops it (and the
  next page in duplex mode), and decodes a page's image before drawing
  it. ecoDMS prints its "T" sheet on both sides.
- **Nobody but CCH inserts separator sheets into an existing PDF.** The
  print, rescan and re-run loop is PDFlyer's, and worth keeping.

### Reading and writing codes

- **Rust readers.** rxing 0.9.3 (Apache-2.0, a pure-Rust port of ZXing
  3.5.1 with zxing-cpp's QR and Data Matrix readers; every common 1D
  and 2D symbology; builds for wasm). zxing-cpp's Rust wrapper 0.5.3
  (Apache-2.0; the C++20 library, built with CMake). rqrr and bardecoder
  read QR only. zbar-rust is LGPL-2.1 with no Data Matrix or PDF417;
  zedbar, a Rust port of ZBar, is LGPL-3.0, awkward to link statically
  into a Rust binary. rxing's default features pin `image = "=0.25.8"` (sheaf
  is on 0.25.10): it's used with `default-features = false` and the
  features `decoders, encoders, encoding_rs, oned, qrcode, datamatrix`.
  zxing-cpp's author's benchmark (so perhaps partial) found rxing slower
  and less thorough on QR, and reading codes in pages with none when
  every format was on.
- **The bench** (a scratch crate, one thread, EPYC 7302P): synthetic
  300 dpi Letter pages of text, rules and a table with one code (Code
  128, Code 39, QR, Data Matrix, PDF417, each at two sizes), in 17
  degradations (turns of 90° and 180°, skew to 3°, noise, blur, salt and
  pepper, low contrast, JPEG at 40, black and white at 300, 200 and 150
  dpi, and an "office" mix of blur, skew, noise and 1-bit at 200 or 150).

  | Reader | Read (of 170) | No code, 150 dpi | No code, 200 dpi 1-bit | Real scans with junk reads |
  |---|---|---|---|---|
  | rxing, one decode, separator symbologies, `TryHarder` | 149 | 44 ms (49 reduced from 300) | 70 ms | 0 of 150 |
  | rxing, whole-page multi-code, all formats | 149 | 66 ms | 94 ms | about 22 of 150 |
  | rxing without `TryHarder` | 73 | 17 ms | 28 ms | — |
  | zxing-cpp, separator symbologies | 159 | 30 ms | 47 ms | 0 of 150 |
  | rqrr (QR only) | 25 of 34 | 51 ms | 93 ms | — |

  Both main readers read QR and Data Matrix at 0.34 mm modules in every
  1-bit 200 and 150 dpi case; 10 mil 1D codes and small PDF417 broke at
  150 dpi. A vector page of each symbology at three sizes, rasterized
  and read back, says 1D codes need 20 mil bars at 200 dpi in 1-bit.
- **Real pages carry codes.** Of 200 form pages and 150 scans from the
  corpus, the separator-limited readers found real codes on 8 forms and
  2 scans: Code 39 form ids, a Data Matrix `$REG32608$`, a QR saying "DO
  NOT READ", Code 128s.
- **Writers.** rxing encodes every symbology it reads (one module a
  pixel with no margin); `qrcode` 0.14.1 and `datamatrix` 0.3.3 (whose
  `Bitmap::path()` is a vector outline) are the alternatives;
  `barcoders` is 1D only. rxing's and barcoders' Code 39 is 2:1, below
  USS-39's 2.2–3.0 for thin bars, should sheaf ever print one.
- **Patch codes.** Kodak's A-61599 (2006; Alliance hosts a copy):
  wide bars 0.20 in, narrow bars and spaces 0.08 in, 0.80 in in all, at
  least 2 in long, at least 0.25 in from the edges, within 3.75 in of
  the leading edge; bars under 20% reflectance on paper over 65%; no
  photocopies. Kodak's "Patches for Printing" sheets (Letter and A4)
  carry the bars on all four edges plus a Code 39 `PATCHn`. No
  open-source detector was found (zxing-cpp has none; NAPS2 reads only
  the Code 39 text).
  A prototype in the bench (75 dpi, Otsu, lines across each edge band,
  bars classed wide over 0.14 in) read 204 of 204 synthetic pages and
  60 of 60 renders of Kodak's sheets, found nothing on 204 synthetic
  and 350 real pages without codes, in about 5 ms a page. Bars
  perpendicular to the leading edge (Kodak's i280 layout) aren't read.
- **Drivers' own readers.** TWAIN has patch and barcode detection
  (`TWEI_PATCHCODE`, `TWEI_BARCODETEXT`; QR among its barcode types,
  Data Matrix not); SANE's fujitsu, kodak and canon_dr backends expose
  none. Reading the pixels is the one way that works everywhere.

## Design

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

## Proposals (to confirm at kickoff)

### Intake

1. **One intake pipeline for files and devices.** `sheaf_ops::intake`
   takes pages from a stream and lands them in a document a batch at a
   time. Today's `sheaf_ops::scan` becomes the same pipeline fed from
   image files.
   - **The stream:** `trait Pages { fn next(&mut self, p) ->
     Result<Option<RawPage>> }`. Files give a decoded image a page (a
     TIFF's pages one by one); a scanner gives what it scans as it scans
     it, through the worker's client (proposal 4).
   - **A page at a time:** clean (proposal 3), then blank (leave out,
     with `sheaf_image::blank` as today), then separators (proposal 15,
     only when asked: a separator is never OCRed), then OCR (one engine,
     built once, as today).
   - **Batches:** every 8 pages, or 2 seconds, whichever comes first,
     the batch becomes one `MemoryKind::Images` source (`made_pages`)
     and one commit: an `InsertPages`, plus `SetOcrText` for its pages.
     The commits of one intake are one group (a new `GroupKind::Scan`,
     made by `Actor::Job(JobKind::Scan)`), so they're one undo step, as an
     OCR run's flushes are; undoing it mid-scan stops the scan, as it
     stops OCR.
   - **Where it lands** is a trait too: a `Session` (the command line),
     or a document's planner in the app (the open document for Insert,
     a new one for Create). The app's batches commit as page inserts do
     (`plan_made`, `Base::At`), so the user can keep working while pages
     arrive.
   - **Memory stays flat.** A page is encoded as soon as it's cleaned
     (JPEG for color and gray, CCITT G4 for black and white) and its
     pixels dropped. Today every page is held to the end, and a black and
     white page is held as 8-bit pixels (`Pixels::Bilevel`, 8.4 MB for
     Letter at 300 dpi): 500 pages hold 4.2 GB (found while planning).
   - Alternatives: keep `scan` as it is and write a second path for
     devices (two pipelines to keep alike); land everything at the end
     (nothing to see until the feeder is empty, and nothing recoverable
     if sheaf dies at page 400).

2. **Pages land where the user is.**
   - **Insert from Scanner** (Document > Insert Pages > From Scanner…,
     Acrobat's Insert from Scanner): pages go into the open document
     before the page chosen, or at the end, as they're scanned.
   - **Create from Scanner** and Create from Scans (photos): ask 1.
     Recommended: a new window with an untitled document opens at once
     and pages stream into it; nothing is written until Save, which asks
     where (B11's fix: no file written and opened again). The other
     answer is Combine's: ask for the path first, scan in the dialog,
     save, hand the Session to a window.
   - **Untitled documents** (if ask 1 says so) need, in the app:
     - `DocView.path` becomes optional (15 uses in `docview.rs`); the
       title says "Untitled scan" (numbered);
     - Save of an untitled document is Save As; closing one with pages
       asks, as a changed one does;
     - its crash-recovery sidecar lives in the state folder
       (`sheaf_ops::dirs::state_dir()/recovery/`) as a Full sidecar
       (the format already holds a whole document,
       `MemoryKind::Recovered`), and the next launch offers it, as
       recovery does beside a file today. A long scan is then safe from
       a crash after its first batch.
   - **The command line** lands in a `Session`, saved to `-o` at the end
     (as now).

3. **A scanner profile for cleanup.** `ScanParams::default()` is a phone
   photo's: find the page in the frame, even out the lighting. A
   scanner's page needs neither: the page is the image, the light is
   even, and the page finder can crop a white page on a white lid wrongly
   (to check in slice 1). So cleanup gets two profiles, each row still
   switchable:

   | Step | Photo (files; today's default) | Scanner (devices) |
   |---|---|---|
   | Find the page, clear its edges | on | off |
   | Flatten curl (UVDoc) | off | off |
   | Even out lighting, white paper | on | off |
   | Turn upright | on | on |
   | Straighten | on | on |
   | Leave out blank pages | off | on with duplex, else off |
   | Black and white | Sauvola on the cleaned page | the same: sheaf asks the scanner for gray and thresholds it, so every scanner's black and white looks alike |

   `sheaf scan --profile photo|scanner` (photo for files, scanner for a
   device). A folder of TIFFs from a copier's scan-to-folder is a
   scanner's pages: `--profile scanner` says so.
   - Alternative: one profile and let the user switch steps. Every
     scanner user would have to turn the same two off.
   - Asking a scanner for gray and thresholding in sheaf costs 8 times
     the bytes of 1-bit over USB or the network (about 8 MB a Letter page
     at 300 dpi, raw). That's well within USB 2's and gigabit's rate
     at a feeder's 30–60 pages a minute, but slow on Wi-Fi; a later
     option can ask for the scanner's own black and white.

4. **Scanner drivers run in a worker process.** `sheaf-scan-worker`, a
   small binary of `sheaf-scanner` alone (no GPUI, PDFium or OCR),
   shipped beside `sheaf` and `sheaf-cli`. The app and the command line
   start it when they need a scanner and talk to it over its standard
   input and output.
   - **Why:** a scanner driver is someone else's native code in sheaf's
     address space, beside documents with unsaved changes. libinsane
     runs SANE in its own process because some backends (Brother, old HP)
     corrupt memory and crash; NAPS2 runs SANE and Apple's in workers "for
     added stability"; `scanimage` hung on this machine after an injected
     jam. A worker that crashes or hangs is killed and restarted, and the
     pages it already sent are kept. TWAIN needs it anyway: a 32-bit
     driver loads only into a 32-bit process, and TWAIN wants a window
     and a message loop, which the worker owns.
   - **The protocol:** requests as JSON lines in (`list`, `caps`, `scan`,
     `cancel`, `quit`), events as JSON lines out (`device`, `caps`,
     `sheet`, `page`, `status`, `error`, `done`), each `page` followed by
     its bytes (raw pixels with width, height, format and resolution, or
     an encoded image as the driver gave it). Versioned by a `hello`
     line; the worker and the app ship together, so a mismatch is an
     error, not a negotiation.
   - **Watchdog:** a scan that sends nothing for 60 seconds (a flatbed's
     lamp warm-up included; `[scanner] timeout` changes it) is ended: the
     worker is killed, the reader is told "the scanner stopped
     answering", and the pages so far stay. Cancel is a message; a
     worker that hasn't stopped 10 seconds later is killed.
   - **Windows:** `sheaf-scan-worker.exe` (64-bit) and
     `sheaf-scan-worker-x86.exe` (32-bit, for 32-bit TWAIN drivers; ask
     3). Each lists the TWAIN sources of its own bitness; the client
     merges the lists and uses the worker the source needs.
   - **One driver per worker,** started for the dialog and kept while it's
     open (opening a SANE device can take seconds); a `list` has its own
     short-lived worker, so a slow probe never holds a scan up.
   - Alternative: in process, on a thread of their own (simple-scan,
     gscan2pdf). Simpler, but a driver's crash takes sheaf with it, and
     32-bit TWAIN can't load at all.

5. **Which driver API on which system.**

   | System | Driver API | Covers | Not covered |
   |---|---|---|---|
   | Linux | SANE (`libsane.so.1`, the system's) | USB scanners with a SANE backend (fujitsu covers fi-series and ScanSnap iX1600, canon_dr, epsonds, avision, kodak), vendors' own backends, network scanners through sane-airscan (eSCL, WSD) and hplip | scanners with no backend |
   | Windows | **TWAIN** (TWAINDSM, 64- and 32-bit), then **WIA 2.0** | TWAIN: every office document scanner's own driver (PaperStream IP, Canon's, Epson Scan 2, Kodak's, Brother's); WIA: scanners with a WIA driver, and network scanners and copiers through Windows' built-in WSD and eSCL drivers | ScanSnap (no driver API; proposal 11) |
   | macOS | ask 4 | — | — |

   - **No network scan protocol of sheaf's own.** eSCL looked like a
     common path for all three systems, but the scanners tax offices put
     on desks are USB-only (fi-7160 and fi-8170, Canon DR-C225, ScanSnap),
     and each system already reaches network scanners: sane-airscan on
     Linux, WIA's built-in drivers on Windows, AirScan through Image
     Capture on macOS. Mopria's eSCL specification is licensed for
     internal implementation only, with no redistribution. A direct
     eSCL client stays an option for later.
   - **Tax software asks for TWAIN.** Drake Documents, FileCabinet CS,
     TaxDome and Scan123 all name TWAIN; PaperStream IP (fi-series) is a
     TWAIN and ISIS driver. WIA covers fewer office scanners but comes
     with Windows, and is how a network copier appears.
   - Order on Windows: the device list shows both, marked by driver,
     with TWAIN first when one scanner has both.

6. **SANE: loaded at run time, sheaf's own bindings.**
   - `libsane.so.1` is `dlopen`ed by the worker (`libloading`), as PDFium
     and ONNX Runtime are by the app. Without it, sheaf starts and runs as
     ever, and the scanner dialog says "Scanning on Linux needs SANE
     (the libsane1 package)". `SHEAF_LIBSANE` names another library, for
     tests (slice 6).
   - The bindings are sheaf's: the 15 functions and their structs, from
     SANE's public-domain `sane.h`. The one crate (`sane-scan` 0.1.2,
     2023) links libsane at build time and runs bindgen on the system's
     headers, and has no cancel from another thread.
   - **Licenses:** SANE's backends are GPL-2.0+ with the SANE exception
     ("linking … does not by itself cause the resulting executable to be
     covered by the GPL"), and `sane.h` is public domain. sheaf neither
     links nor ships SANE: it loads the system's, so nothing of SANE is
     distributed with it.
   - **What every frontend learned, built in:**
     - one thread per handle, nothing overlapping; `sane_cancel` from the
       watchdog's thread is allowed (the API calls it safe
       asynchronously);
     - `sane_cancel` after every batch and after any error status;
     - a feeder: `sane_start` per page until `SANE_STATUS_NO_DOCS`, which
       on the first page means "the feeder is empty" and later means
       done, and which some backends return from `sane_read`, not
       `sane_start`;
     - a flatbed: one page per Scan press (some flatbeds give the same
       page forever);
     - parameters that make no sense (Brother's `2500x0`) end the batch;
     - `lines = -1` reads to EOF; padded rows; 1-bit rows are 1 = black,
       first pixel in the high bit; 16-bit samples in native order;
       three-pass frames assembled;
     - options by their standard names (`resolution`, `mode`, `source`,
       `tl-x`…`br-y`, `page-width`/`page-height`), and the feeder and
       duplex by what backends actually call them: `source` values `ADF
       Front`/`ADF Back`/`ADF Duplex` (fujitsu, canon_dr, avision),
       `ADF`/`ADF Duplex` (airscan), `Automatic Document Feeder` (test),
       vendor strings with "Duplex" in them, or a separate `adf-mode`,
       `duplex` or `ScanMode` option (simple-scan keeps the same list);
     - `page-height` set after `source`, since the fujitsu backend only
       enables it for the feeder; descriptors read again whenever an
       option says so, or `sane_start` returns `SANE_STATUS_INVAL`.
   - **Finding scanners:** `sane_get_devices` in a short-lived worker
     with its own deadline (it took 10.7 s here with nothing attached);
     the last device is opened by its name directly, without a list.

7. **TWAIN on Windows.**
   - The worker loads `TWAINDSM.dll` (the system's, installed by drivers;
     else sheaf's bundled copy of the TWAIN Working Group's, LGPL-2.1,
     unmodified, loaded at run time), opens it with a hidden window it
     owns, and pumps that window's messages through `MSG_PROCESSEVENT`.
   - Scans without the driver's window by default (`ShowUI = false`),
     with sheaf's settings: `ICAP_XRESOLUTION`, `ICAP_PIXELTYPE`,
     `CAP_FEEDERENABLED`, `CAP_DUPLEXENABLED`, `ICAP_SUPPORTEDSIZES` or a
     frame, `CAP_XFERCOUNT = -1`; native or memory transfer.
   - **"Scanner settings…"** opens the driver's own window
     (`CAP_ENABLEDSUIONLY`, where the driver has it), for what sheaf
     doesn't set: PaperStream's own image processing, a dropout color,
     a driver's double-feed sensitivity. Those then apply to sheaf's scans
     until changed.
   - Errors: `TWCC_PAPERJAM`, `TWCC_PAPERDOUBLEFEED`, `TWCC_NOMEDIA`,
     `CAP_FEEDERLOADED` before starting, cover open.
   - Bindings: sheaf's own, from `twain.h` (the `twain` crate is a single
     2021 release that ships DLLs inside it). RSWebTWAIN (Apache-2.0,
     alpha: a 64-bit app with a 32-bit JSON-lines sidecar and a typestate
     TWAIN state machine) is the reference.

8. **WIA 2.0 on Windows.**
   - Through the `windows` crate (`Win32::Devices::ImageAcquisition`:
     `IWiaDevMgr2`, `IWiaItem2`, `IWiaTransfer`,
     `IWiaTransferCallback`), on the worker's own STA thread.
   - The feeder is the root's `Feeder` child; duplex is
     `WIA_IPS_DOCUMENT_HANDLING_SELECT = FEEDER | DUPLEX`;
     `WIA_IPS_PAGES = 0` scans until the feeder is empty, a sheet being
     two pages in duplex.
   - Errors by code: `WIA_ERROR_PAPER_EMPTY` (at the start, "the feeder
     is empty"; later, done), `PAPER_JAM`, `MULTI_FEED`, `COVER_OPEN`,
     `WARMING_UP` (wait and retry), and the undocumented `0x210001` (no
     more pages), as NAPS2 learned.
   - WIA 1.0 and the WinRT scanner API aren't used: both scan only to
     files.

9. **The device model.**
   ```rust
   pub struct DeviceInfo {
       pub id: DeviceId,             // "sane:fujitsu:fi-7160:12345", "twain64:PaperStream IP fi-7160", "wia:{…}"
       pub name: String,             // as the driver names it
       pub maker: String, pub model: String,
       pub driver: Driver,           // Sane | Twain { bits } | Wia | Fake
   }
   pub struct Caps {
       pub sources: Vec<Source>,     // Flatbed | Feeder { duplex: bool }
       pub resolutions: Vec<u32>,    // or a range, listed
       pub modes: Vec<Mode>,         // Color | Gray | Bw
       pub max_area: SizeMm,
   }
   pub struct Settings {
       pub source: Source, pub dpi: u32, pub mode: Mode,
       pub paper: Paper,             // Auto (the driver's own size detection) | Letter | Legal | A4 | SizeMm
   }
   pub struct RawPage { pub image: PageBytes, pub dpi: (f32, f32), pub sheet: u32, pub side: Side /* Front | Back */ }
   pub enum ScanError { FeederEmpty, Jammed, DoubleFeed, CoverOpen, Busy, Gone, Stopped /* watchdog */, Driver(String) }
   pub enum Done { FeederEmpty, OnePage /* flatbed */, Cancelled }
   ```
   - Settings are clamped to the device's range (the SANE `test`
     backend's area stops at 200 mm), and what was clamped is said.
   - The client is synchronous and runs on a background thread, with
     events on a channel, as the OCR job's are (`OcrEvent`).
   - `fake` is a backend scripted in tests: pages, sides, a jam at
     sheet 4, a hang, a crash.

10. **Scanning: controls, errors, and scanners that come and go.**
    - **Feeder:** Scan runs until the feeder is empty, then offers "Scan
      More" (load the next stack; pages continue) and "Done". **Flatbed:**
      one page per Scan, then Scan More or Done.
    - **Scan backs** (Acrobat's Reverse Sides): for a simplex feeder, scan
      the fronts, turn the stack over, scan the backs; sheaf interleaves
      them (the backs come in reverse order). Blank backs then go as any
      blank page does.
    - **A jam, a double feed, an open cover:** the scan stops with what
      happened in words ("Paper jam at sheet 14: clear it, put sheet 14
      and the rest back, then Continue"), the pages so far kept;
      Continue goes on, Done ends.
    - **Cancel** (Escape): stops after the page in flight; pages so far
      stay (they're already in the document, one undo step).
    - **Progress:** sheets and pages counted (a feeder's total is
      unknown), the page in flight's bytes as a bar, and each page shown
      in the document as its batch lands.
    - **Scanners that come and go:** the device list fills in the
      background when the dialog opens and has a Refresh button; there
      are no hot-plug events worth relying on (SANE has none). A
      scanner unplugged mid-scan is `Gone`: pages kept, the dialog says
      so.
    - **The last scanner** (its id in `state.json`) is selected and
      opened at once, without waiting for the list.

11. **Scanners sheaf can't drive hand it files.**
    - **ScanSnap** has no TWAIN, ISIS or WIA driver (PFU's FAQ); on
      Linux, SANE's fujitsu backend drives the iX1600 over USB. On
      Windows and macOS, a ScanSnap Home profile can **Send to** any
      application, which it starts with the scanned file. With sheaf as
      that application, each scan opens in the running sheaf
      (single instance hands it over), where Document > Separate does the
      rest. The docs gain a page on setting that profile up.
    - **Copiers' scan-to-folder** and scan-to-email files are opened, or
      inserted, as any PDF; Separate works on them the same way.
    - **Later, not M5:** ScanSnap's SDK (Windows and macOS, registration
      required) and watched folders (M15).

### Separators

12. **A separator is found by a finder and means what its rule says.**
    ```rust
    pub enum Finder {
        /// sheaf's own sheet: a QR code whose text starts SHEAFSEP.
        Sheet,
        /// Other software's sheets: a barcode of these symbologies whose
        /// text matches `pattern` (none: any text).
        Barcode { symbologies: Vec<Symbology>, pattern: Option<Regex> },
        /// Kodak patch codes of these types.
        Patch { codes: Vec<PatchCode> },
        /// A page with little else on it whose text matches `pattern`.
        Keyword { pattern: Regex },
    }
    pub struct Rule {
        pub finder: Finder,
        pub action: Action,            // Bookmark | Split | Both
        pub title: Title,              // the code's text | a template ("{value}", "{n}", "{date}")
        pub level: Level,              // Top | UnderLast (top while there's no top one yet)
        pub separator: Keep,           // Drop | Keep
        pub when: When,                // Each (every page found) | Changes (only when the code's text changes)
    }
    ```
    - **sheaf's sheets are always looked for**, and carry their own
      action, title, level and keep/drop (proposal 13); no rule is needed
      to read one, on any machine, so a sheet printed at one desk is read
      the same at every other.
    - **Rules for other separators** come from the Separate dialog (its
      last ones in `state.json`) or `sheaf separate --find …`. A firm
      that has a stack of PaperStream patch sheets, or its own "SEPARATOR"
      page made in Word, sets a rule once.
    - **`When::Changes`** is for codes printed on each document's own
      pages rather than on a sheet (a label on every page of a
      document): a new section starts only when the text differs from
      the last page's, and the page is kept (Kofax's and PaperStream's
      "separate on change").
    - **"Any barcode" is never a default.** Tax documents carry barcodes
      of their own: the corpus's forms have Code 39 form ids, Code 128s,
      a Data Matrix and a QR (the bench found them), and W-2s and K-1s
      carry PDF417 (UltraTax had a utility that read them; unverified). A
      barcode rule has a pattern unless the user clears it, and the
      review shows what each separator read.
    - **A keyword page must be mostly empty:** the match plus at most 60
      other letters, as Find Blank Pages' "intentionally blank" check
      does (`blank::text_says`), so a page that mentions the word isn't a
      separator. The page's text is its own, its OCR text, or, while
      scanning with OCR on, what OCR read. A page with no text can't
      match; the review counts those pages and says to run OCR.
    - Alternative: separators by configuration only, as CCH Scan,
      PaperStream and Kofax do (a sheet means what the job profile says).
      Self-describing sheets are better on the merits: no profile to keep
      alike across desks, and the meaning travels with the paper.

13. **sheaf's separator sheet.** One page, the document's page size
    (Letter or A4 by default), black on white, mostly empty so it's
    easy to see in a stack:
    - **Top:** "SEPARATOR SHEET" in small capitals, and what it does:
      "Starts a new file", "Starts a bookmark", "Starts a bookmark under
      the one before".
    - **Middle:** the title, large and bold (Helvetica-Bold, shrunk to
      fit three lines; characters it can't draw show as `?` until M10
      slice 7's fallback font, while the code keeps the title exactly).
    - **Two QR codes** with the same text, top right and bottom left,
      modules of 0.75 mm (4.4 pixels at 150 dpi; the bench read modules
      half that size in every 1-bit 150 and 200 dpi case), error
      correction level M, at least 10 mm of white around each. A typical
      title makes a code about 30 mm square; the longest (120 bytes)
      about 40 mm. Either code survives a staple, a hole punch, a fold or
      a pen mark, and QR reads at any rotation, so a sheet fed upside down
      or sideways works.
    - **Optionally, patch T bars** along the edges, in Kodak's layout
      (off by default): a sheaf sheet then also separates in PaperStream,
      Kofax or a scanner's own patch reader. The QR code still says what
      sheaf does with it.
    - **Bottom:** "Put this sheet face up in front of the pages it names.
      sheaf finds it when the stack is scanned. Print it at 100%; don't
      photocopy it." and the code's text in small type, for a person.
      (CCH and Alliance give the same printing advice: copies degrade.)
    - **The code's text, version 1:** `SHEAFSEP1|a=split|l=1|p=drop|t=W-2
      Acme Corp`. Fields are `|`-separated `key=value`; `t` (the title,
      UTF-8, up to 120 bytes) is always last and may contain `|`;
      unknown keys are ignored, so later versions can add fields (client,
      year) without breaking older readers; a newer major version
      (`SHEAFSEP2`) is reported in the review, not guessed at. `a` is
      `bookmark`, `split` or `both`; `l` is `1` (top) or `2` (under the
      last top one, or top while there's none); `p` is `drop` or `keep`.
    - **Made in memory** (`memory::separator`, `MemoryKind::Separator`)
      from the sheet's spec, deterministically: the same spec makes the
      same bytes, as blank pages do, so an insert lowered again or
      replayed finds its pages. Drawn with the content writer: text in
      the standard 14, each QR module a filled rectangle (crisp at any
      printer resolution, a few kilobytes a page). The modules come from
      the reader library's own QR encoder (rxing's `BitMatrix`), runs
      merged into rectangles.
    - The symbology is ask 8: QR is recommended (TWAIN's barcode types
      have QR and no Data Matrix, should a driver's reader ever be used).

14. **Patch codes and other software's barcodes are read; sheaf prints
    only its own sheet.**
    - **Kodak patch codes** are four bars along a sheet's edge, wide
      (0.20 in) and narrow (0.08 in), set in from the edge (Kodak's
      A-61599 says at least 0.25 in; Alliance's printed sheet, citing
      Kodak's A-61580, 0.5 in). They carry no data, a type only:
      1 is WWNN, 2 WNNW, 3 WNWN, 4 NWWN, 6 NNWW, T NWNW, read from the
      edge inward, which also tells 3 from T and 1 from 6 at any
      rotation. sheaf reads them itself (no barcode library does;
      proposal 15). Defaults by Kodak Capture Pro's convention, each
      changeable in the rule: **3** (a batch) starts a top-level section,
      a new file when splitting, and is dropped; **T** starts a section
      under it and is dropped; **2** does the same as T and is kept;
      **1, 4 and 6** (image controls) are listed in the review and do
      nothing. With no 3 in a batch, T and 2 sections are top-level.
    - **The Code 39 text Kodak prints on its patch sheets** (`PATCH1`…
      `PATCHT`) is read as the same type, for a sheet whose bars a scan
      lost; the bars come first (in the bench, rxing missed Kodak's own
      `PATCHT` at 200 dpi in black and white, and the bars were found).
    - **Code 39 `PATCHT` alone**, the only thing NAPS2 and Paperless-ngx
      look for, is read as a patch T too, so a firm with their sheets
      needs no new ones. In the dialog it's named apart from real patch
      bars.
    - **Other software's barcode sheets** by symbology and pattern, with
      the code's text as the title: SurePrep's target sheets are Code 128
      with the category's name ("Schedule A", "Rent and Royalty"), so
      `code128:^(Schedule [ACEF]|Business Income|Rent and Royalty|Farm
      Income|State|Taxpayer Information)$` bookmarks a SurePrep-sheeted
      batch by category.
    - **sheaf prints none of them:** patch sheets come from scanner makers
      and Alliance as printable PDFs, and sheaf's own sheet does more.

15. **Finding separators.**
    - **In a document** (`separate::find(snap, views, rules, pages, p)`),
      as Find Blank Pages reads pages: each page drawn through its views
      with its content alone, in the background budget, in gray at **150
      dpi** (sheaf's QR modules are 4.4 pixels there; patch bars 12), or
      at **200 dpi** when a rule reads 1D codes (a 20 mil bar is 4
      pixels; thinner ones need the scan's own resolution), and its text
      read with its OCR text for keyword rules.
    - **While scanning,** on the cleaned image itself, before OCR,
      reduced 2:1 by a box filter when it was scanned at 300 dpi (about
      5 ms).
    - **What's read, in order:** patch bars along the four edges
      (sheaf's own reader, about 5 ms a page); then one barcode decode
      for sheaf's sheet and every rule's symbologies at once (rxing's
      `MultiFormatReader`, `TryHarder` on, its formats limited to QR and
      the rules' symbologies); then keywords. A code counts only if its
      text matches: `SHEAFSEP` for sheaf's sheet, the rule's pattern for
      others. One page is one separator at most, sheaf's sheet before
      any rule.
    - **Never "every symbology, every code on the page":** the bench's
      whole-page multi-code search with all formats was 5–6 times slower
      and read junk retail codes (UPC-E, EAN-8, ITF, Codabar) off about
      15% of the corpus's real scans; limited to the separator
      symbologies, it read none.
    - **Speed:** most pages aren't separators, so a page with none is
      what counts. In the bench (one thread, a 300 dpi Letter page of
      text, rules and a table), a page with no code took 44 ms decoded at
      150 dpi, 49 ms reduced from 300 (26 for a blank page), and 70 ms at
      200 dpi in black and white. A page with a code: 28–50 ms. rxing without `TryHarder` was
      twice as fast and missed every Data Matrix and small 1D code, so
      it isn't used.
    - **Parallel:** renders on the engine pool's workers (Budget
      Background), decoding on a rayon pool. A long scan's search is
      then about its renders' time (gate in slice 5); the fallback, if
      renders are the bottleneck, is decoding a scanned page's own image
      instead of drawing it, as Stirling-PDF does.

16. **Applying a separation.** `separate::plan(found, pages)` turns
    separators into **sections**: each starts at the first page after
    its separator (and after the separator's back, below) and runs to the
    next separator; pages before the first are a section titled "Start"
    unless there are none. Then:
    - **Bookmark:** one commit, labelled "Separate", on the version the
      search read (`Base::At`, as Find Blank Pages commits): an
      `AddBookmark` per section (level 1 at the top, `Sibling::Last`;
      level 2 under the last level 1), and one `DeletePages` of the
      separators that are dropped. One undo step.
    - **Split:** one file per top-level section, each an extract
      (`Export::extract`) of its pages, separators left out, written to a
      folder as `sheaf split` writes parts: every part checked before any
      is written, an existing file replaced only when told. Level-2
      sections are bookmarks inside their part. The document isn't
      changed.
    - **Both:** the commit, then the split of the version it made.
    - **Names:** a part is named by its title alone (`W-2 Acme Corp.pdf`,
      `file_safe`, then `_2` for repeats), as PaperStream and CCH name
      them from the sheet; `{stem}_{title}` like Split Document's parts
      is the other choice, set in the dialog (remembered).
    - **The sheet's back:** in a duplex scan, the back of a separator
      sheet is part of the sheet. While scanning, the scanner says which
      side a page is. In a PDF, a page right after a separator that
      `sheaf_image::blank` calls blank is taken as the back and goes with
      the sheet; the review shows it.
    - **Two separators in a row** make an empty section: dropped, with a
      line in the review.
    - **The review** (a sheet over the document, as Find Blank Pages'):
      a row per section with the separator's thumbnail, what it read
      ("QR: SHEAFSEP1 …", "Patch T", "Code 39: SEP-W2"), the title
      (editable), level, pages, and whether the separator is dropped.
      Apply makes the commit and the split. "N pages have no text"
      shows when a keyword rule couldn't read them.
    - Alternative: split only, as most capture software does. A binder
      that stays one file with bookmarks is how CCH Scan and PDFlyer
      users work, so bookmarking stays the default for other software's
      separators, and sheaf's own sheets say which.

17. **Insert Separator Sheets.** `InsertPages { from:
    PageSource::Separator { sheet }, before: Some(page) }`, one per
    place, one commit ("Insert separator sheets"), one undo step.
    - **Where (Document > Insert Separator Sheets…):** above each
      top-level bookmark, titled with its title (the default); with its
      children too, as level 2 sheets; above the selected pages; or above
      pages typed in ("3, 10, 25"), titled "Document 1", "Document 2"…;
      each title editable in a list before inserting.
    - **What they say:** split, bookmark or both, and drop (default) or
      keep.
    - The bookmarks keep pointing at their pages, so after a print and a
      rescan, Separate rebuilds the same bookmarks, or the same files.
    - **Removing them:** undo, before saving. Afterwards, Separate with
      neither bookmarks nor a split ("Remove separators only") takes them
      out, as PDFlyer's Remove does.
    - **A stack to print:** File > Create > Separator Sheets… makes a new
      document of sheets from a list of titles (one a line, or none for a
      plain "Split here" sheet, with copies), to print and keep by the
      scanner. `sheaf separator-sheets --titles … -o sheets.pdf` too.
    - Alternative: ship fixed printable sheets as PDFs, as PaperStream
      and Kodak do. Sheets made from the binder's own bookmarks are the
      point of Insert Target Sheets, and a list of titles covers the
      generic stack.

### For both

18. **The command line.**
    ```
    sheaf scan --list-devices [--json]
    sheaf scan --device ID|NAME [--source feeder|duplex|flatbed] [--dpi N]
               [--mode color|gray|bw] [--page letter|legal|a4|auto]
               [--ocr] [--drop-blanks] [--separate [--find …]] [cleanup flags] -o OUT.pdf
    sheaf scan IMAGES… [--profile photo|scanner] … -o OUT.pdf          (as today)

    sheaf separate IN [--find sheet|patch:2,3,T|code39[:REGEX]|code128[:REGEX]
                              |qr[:REGEX]|datamatrix[:REGEX]|pdf417[:REGEX]|keyword:REGEX]…
                      [--action bookmark|split|both] [--title TEMPLATE]
                      [--keep-separators] [--split-dir DIR] [--names title|stem-title]
                      (-o OUT [--append] | --in-place | --dry-run) [--json]

    sheaf separator-sheets IN (--above-bookmarks [--with-children] | --above 3,10,25)
                           [--titles FILE] [--action …] [--keep] -o OUT
    sheaf separator-sheets --titles "W-2" "1099" … [--copies N] [--page letter|a4] -o sheets.pdf
    ```
    - `--find sheet` is always on; each `--find` adds a rule with
      `--action`, `--title` and `--keep-separators` as given before it
      (or the defaults: bookmark, the code's text, drop).
    - `separate --dry-run --json` prints the sections: where each
      separator is, what it read, the title, the pages. A split's ask
      (signatures, an XFA part's values) exits 3, as every subcommand's
      does.
    - `--device` takes an id from `--list-devices` or a name's unique
      prefix; `--device first` takes the first one found.
    - All three are thin translators over `Session` and the ops, as every
      subcommand is since slice 19.

19. **Settings and state.**
    - **`state.json`** (sheaf's, disposable): the scanner dialog's last
      device, source, resolution, mode, page size, cleanup rows, OCR,
      blank pages and separate options; the Separate dialog's rules,
      action, names and keep/drop; Insert Separator Sheets' where and
      what. `State::scan` splits into the two profiles' choices.
    - **`settings.toml`** (yours, never written): a `[scanner]` section in
      the commented template:
      ```toml
      [scanner]
      # Kinds of scanner driver sheaf leaves out: "twain", "wia", "sane".
      # A driver that misbehaves can be left out for every scan.
      # skip = ["wia"]

      # Seconds a scanner may send nothing before sheaf gives up on it
      # (a flatbed's lamp warming up counts).
      # timeout = 60
      ```
      No separator rules in settings for now: the dialog remembers them.
      A firm that wants one shared rule list gets `[[separators.rule]]`
      when it asks.

20. **Separation is core; so is intake. Plugins see the results.**
    - Scanner APIs are native (C, COM, Objective-C), which wasm plugins
      can't call, and intake is the scan path the app and the command
      line share. Core, like OCR (M5's 2026-10-11 ruling).
    - Separation could be a plugin's (a plugin can render pages and read
      barcodes with its own wasm build of a decoder), but it needs what
      0.4 doesn't give plugins: inserting pages (§14.3: "insert-pages …
      later, with manifest permissions"), a new page source, the scan
      path and `sheaf separate`. And it's the PDFlyer parity row, so it
      belongs with the binder tools.
    - **What plugins get:** `PageSource::Separator` is an op's, so it
      reaches plugins and `sheaf edit` when inserts open to them; the
      bookmarks and pages a separation makes are ordinary ones, which
      `tax-form-bookmarks` and others read; Document > Separate is a
      command, which M15's batch and M16's agent can run.

21. **Packaging and gates.**
    - **Every bundle** gains `sheaf-scan-worker` beside `sheaf` and
      `sheaf-cli` (`scripts/package.sh`), found as PDFium is: beside the
      executable, then `target/` under test.
    - **Linux:** nothing else. SANE is the system's, loaded at run time;
      a new CI gate keeps it so: `! ldd target/debug/sheaf-scan-worker |
      grep libsane`, beside the OCR libraries' gate.
    - **Windows:** `sheaf-scan-worker-x86.exe` (if ask 3 keeps 32-bit
      TWAIN), built for `i686-pc-windows-msvc` (the worker alone: no
      GPUI, PDFium or Tesseract, so a short build), and `TWAINDSM.dll` in
      both bitnesses with its LGPL-2.1 text in `packaging/licenses/`.
      [`code-signing.md`](code-signing.md)'s table of PE files grows by
      four.
    - **macOS:** if ask 4 brings ImageCaptureCore, it's a system
      framework, which the "only system libraries" gate allows.
    - **A crate gate:** `sheaf-scanner` stays devices only: no
      `sheaf-doc`, `sheaf-render`, `sheaf-ocr`, `lopdf`, PDFium or GPUI
      under it (the shape of `sheaf-image`'s gate).
    - **Sandboxes, for when packaging comes** (not M5): there is no
      scanner portal (xdg-desktop-portal #218, open since 2018). Simple
      Scan's Flatpak bundles sane-backends, sane-airscan, hplip and avahi
      and asks for `--device=all` and `--share=network`; NAPS2's adds
      `--filesystem=host` to reach the host's backends. A Snap needs
      `raw-usb`, connected by hand.

22. **Testing without a scanner.** Every layer has a test that needs no
    hardware; what only hardware shows is a checklist done by hand.
    - **The `fake` backend** (every system, in CI): intake and the
      worker's protocol with scripted pages and sides, a jam at sheet 4
      then Continue, a double feed, an open cover, a feeder empty at the
      start, a hang (the watchdog kills the worker; pages kept), a crash
      (pages kept, the exit said), and a cancel mid-page.
    - **sheaf's fake libsane** (Linux, in CI): a small `cdylib` exporting
      the SANE API, loaded through `SHEAF_LIBSANE`, drives the real SANE
      bindings through what SANE's own test backend can't do: each
      backend's names for the feeder and duplex, `NO_DOCS` from
      `sane_read`, Brother's `2500x0`, `lines = -1` from a feeder, a jam
      mid-batch, 1-bit packing, padded rows, three-pass frames.
    - **SANE's `test` backend** (Linux, in CI): the runner installs
      `libsane1`, which carries `libsane-test.so`; `SANE_CONFIG_DIR`
      points at a temporary folder whose `dll.conf` says `test` (no root
      needed: checked here, `test:0` and `test:1` listed in 0.07 s). It
      covers a feeder of 10 sheets that then reports empty, gray, color,
      1- and 16-bit, unknown height (hand scanner), tiny reads, and
      injected `JAMMED`, `COVER_OPEN`, `NO_DOCS` and `IO_ERROR`. A jam
      must end the scan (finding 5).
    - **TWAIN's sample data source** (Windows): the TWAIN Working Group's
      `Twain_DS_sample01` (Modified BSD) is a virtual scanner that reads
      images from disk, with 32- and 64-bit MSI installers in the
      `twain-samples` v2.5.0 release; `CAP_XFERCOUNT` sets how many
      pages. Whether it installs silently (`msiexec /qn`) and scans with
      a hidden window on `windows-latest` is unverified: slice 8 starts
      with that spike. If it works, CI scans through TWAIN in both
      bitnesses; if not, by hand.
    - **WIA** has no virtual device that installs without a person
      (Microsoft's sample drivers need the WDK; `fakescan` needs an Add
      by hand). Its property and error mapping is tested against a trait
      over the WIA calls; scanning is checked by hand.
    - **By hand, on real scanners** (ask 12 asks which): a stack of 50
      mixed sheets through the feeder, duplex, a forced jam, an open
      cover, unplugging mid-scan, cancel, the driver's settings window,
      and the same through a network copier. The results go in the slice
      notes, a row per scanner and driver.
    - **F14:** `sheaf scan --ocr` on a few images, checking the text layer
      (the words, one OCR block a page, one font). CI builds Tesseract and
      fetches its data, so it runs on every runner and never skips.
    - **Separators** are tested end to end without a printer: sheets made
      by `memory::separator`, rendered by PDFium, degraded as a scan
      degrades them (the bench's 17 degradations, kept as a test
      helper), then found; Kodak's printed patch layout drawn the same
      way. The corpus's scans are the negative set: no sheaf code and no
      patch found on any of them.

## Found while planning

1. **`sheaf scan` holds every page's pixels until it writes.** Pages
   collect in a `Vec<PageImage>` until the end, and a black and white
   page is held as 8-bit pixels (`Pixels::Bilevel`; G4 happens in
   `images::to_pdf`): 8.4 MB a Letter page at 300 dpi, 4.2 GB for 500
   pages. Proposal 1's batches fix it (slice 1).
2. **Backlog B11** (Create from Scans writes a file and opens it again):
   proposal 2 and ask 1 (slices 1–2).
3. **Finding scanners is slow.** `scanimage -L` took 10.7 s on this
   machine with no scanner attached (sane-airscan's discovery and
   hplip's probe). A dialog must never wait for it: the last device is
   opened by its id at once, the list fills in the background, and it's
   kept for the session (proposals 6 and 10).
4. **The SANE `test` backend's area stops at 200 mm,** narrower than
   Letter (215.9 mm). Settings are clamped to the device's range, and
   the tests ask for the device's own maximum.
5. **`scanimage` hung after an injected jam** (`--read-return-value
   SANE_STATUS_JAMMED`) until `timeout` killed it, though a cover-open
   status ended it. sheaf calls `sane_cancel` after any error status
   and never reads again; a test pins that a jam ends the scan (slice
   6).
6. **The photo profile on scanner pages** (to check in slice 1): with
   `crop` on, a white page on a flatbed's white lid has no edge to find,
   and evening out the light whitens what the scanner already made
   white. Proposal 3.
7. **README says ❓ for "Target sheets / auto-bookmark from scan
   separators"**, while ROADMAP's parity table has placed it in M5 since
   2026-10-14. The docs slice fixes the row.

## Asks for Greg's kickoff

Each with its options, the recommended answer first.

1. **Where does Create from Scanner (and from Scans) put its pages?**
   (proposal 2)
   - **An untitled document in a new window, pages streaming in;
     nothing written until Save, which asks where; its recovery sidecar in
     the state folder.** Recommended: you see pages as they come, a crash
     after the first batch loses nothing, and it fixes B11.
   - Ask for the path first, scan inside the dialog, save, then hand the
     Session to a window (Combine's way). Less app work; nothing visible
     until the end, nothing recoverable before it.
2. **Which comes first, separators or scanners?**
   - **Separators first** (slices 3–5), then scanners (6–9).
     Recommended: separators need no hardware, serve every way scans
     arrive today (a copier's PDFs, a ScanSnap's files through Send to,
     image files), and carry the PDFlyer parity row; the scanner slices
     carry the platform risk and need checks by hand.
   - Scanners first, as the roadmap lists them.
3. **Which driver APIs on Windows?** (proposals 5, 7, 8)
   - **TWAIN and WIA, TWAIN first, with a 32-bit worker for 32-bit
     TWAIN drivers.** Recommended: office document scanners and tax
     software use TWAIN; WIA comes with Windows and is how network
     copiers appear; NAPS2 found TWAIN drivers "generally 32-bit only",
     and the worker makes a 32-bit build cheap.
   - TWAIN and WIA, 64-bit TWAIN only: one less build and four fewer
     files, and PaperStream IP, Epson Scan 2 and newer Canon drivers have
     64-bit TWAIN; older drivers then don't work.
   - WIA only (the roadmap's first word): fewest moving parts, but no
     PaperStream, and most office scanners' best driver unused.
   - TWAIN only: no network copiers without a TWAIN driver.
4. **macOS?** (the roadmap names nothing; macOS is tier 2)
   - **Not in M5: a Pending entry, "Scanning on macOS
     (ImageCaptureCore)".** Recommended: tax offices scan on Windows,
     nothing here can check a Mac scanner, and the dialog says scanning
     isn't on macOS yet (files, ScanSnap's Send to and separators all
     work).
   - An ImageCaptureCore slice after Windows, behind a cut line, through
     `objc2-image-capture-core` (0.3.2, which has `ICScannerDevice` and
     the feeder unit): it reaches Mac scanner drivers and AirScan copiers
     in one API, checked in CI only as far as it builds, and by hand on a
     Mac.
   - sheaf's own eSCL client: network scanners only, a Local Network
     privacy prompt on macOS 15 and later, and Mopria's specification
     licensed for internal implementation only.
5. **Drivers in a worker process?** (proposal 4)
   - **Yes, always.** Recommended: a driver's crash or hang never takes
     unsaved documents with it, and 32-bit TWAIN needs it.
   - In process on a thread of its own: simpler; a driver crash is
     sheaf's crash; no 32-bit TWAIN.
6. **libsane linked or loaded at run time?** (proposal 6)
   - **Loaded at run time** (`dlopen` of `libsane.so.1`), with sheaf's
     own bindings. Recommended: sheaf starts without SANE installed, as
     it does without ONNX Runtime, and nothing of SANE is shipped.
   - Linked: a build-time dependency on SANE's headers and a binary that
     won't start without libsane1.
7. **Which barcode library?** (proposal 15)
   - **rxing, behind a small trait in `sheaf_image::codes`.**
     Recommended: pure Rust (no new C++ build), Apache-2.0, reads and
     writes every symbology needed, and it read every sheaf-sized QR in
     the bench's worst cases; the trait lets zxing-cpp in later if real
     scans show misses, as the OCR trait let two engines in.
   - zxing-cpp: more thorough (159 of 170 against 149) and about 1.6
     times faster, but a C++20 library built with CMake in every build
     (CI already builds Tesseract that way).
   - zbar: LGPL, no Data Matrix or PDF417, and dropped by Paperless-ngx
     as unreliable on skewed and faint codes.
8. **What symbology does sheaf's own sheet use?** (proposal 13)
   - **QR, two copies, 0.75 mm modules, error correction M; patch T bars
     optional, off.** Recommended: reads at any rotation, holds a UTF-8
     title, reads in 1-bit at 150 dpi, and is the 2D symbology TWAIN
     drivers know.
   - Data Matrix: smaller for the same text and as robust, but no TWAIN
     driver reads it.
   - Code 128 (SurePrep's): read by every scanner's firmware, but one
     line of bars can't hold a long title at a size that reads at 200
     dpi (25 characters at 28 mil is wider than Letter).
   - Patch T on by default: every sheet then splits in other capture
     software too, which may not be what the firm wants.
9. **Which other separators are read?** (proposals 12, 14)
   - **Patch codes of every type by sheaf's own reader (T, 2 and 3 act
     by Kodak's convention, 1, 4 and 6 are listed only), Kodak's and
     NAPS2's Code 39 `PATCH…` text, and rules for Code 39, Code 128, QR,
     Data Matrix and PDF417 by pattern, and keywords.** Recommended:
     covers every sheet the research found in use (PaperStream's,
     Kofax's, Kodak's, Alliance's, SurePrep's, a firm's own), and
     patterns keep content codes out.
   - Patch T only, and sheaf's own sheets: smaller, but firms with other
     sheets keep their other software.
   - Also ITF, Codabar and the retail codes: the bench read junk of
     those off real scans; none is a separator anyone prints.
10. **What does sheaf's sheet encode?** (proposal 13)
    - **The title, the action (bookmark, split, both), the level (top, or
      under the last top one), and whether the sheet is dropped.**
      Recommended: what the separation needs, and the format takes more
      fields later without breaking older readers.
    - Also a client, a year and a document type, for naming files
      (`{client} {year} {title}`). Useful once sheaf has client fields
      to fill them from; until then the title carries them.
    - A one-time id, as GoFileRoom's cover sheets have: needs a
      database, which a desktop editor doesn't have.
11. **Is separation core or a plugin?** (proposal 20)
    - **Core,** with plugins seeing its results and its sheets as an op
      when inserts open to them. Recommended: it needs page inserts and a
      new page source, which 0.4 doesn't give plugins, and it's part of
      the scan path and the command line.
    - A bundled plugin, after a plugin API version that can insert
      pages: waits on that API; can't run inside a scan.
12. **Which scanners can be checked by hand, and what do the pilot
    offices use?** Recommended: name at least one TWAIN document scanner
    (an fi-series with PaperStream IP is what tax software certifies),
    one network copier, and whether any office uses ScanSnaps. These
    decide the hand-check matrix (proposal 22), and whether 32-bit TWAIN
    (ask 3) matters to anyone yet.
13. **CI for scanners.** (proposal 22)
    - **Linux runners install `libsane1` and run SANE's test backend
      and sheaf's fake libsane; the TWAIN slice starts with a spike to
      run TWAIN's sample data source on the Windows runner; WIA is
      checked by hand.** Recommended.
    - Fakes only: faster CI, but the real SANE library and a real TWAIN
      source never run outside a person's machine.
14. **What do other software's separators do by default, and how are
    split files named?** (proposal 16)
    - **Bookmark, separator dropped; split files named by their titles
      alone.** Recommended: a binder with bookmarks is how CCH and
      PDFlyer users work; capture software names files from the sheet.
    - Split by default; files named `{stem}_{title}` like Split
      Document's parts.

## Slices

Ordered by ask 2's recommendation: the shared groundwork (the pipeline
and untitled documents), then separators, then scanners. If ask 2 says
scanners first, slices 6–9 move before 3–5; nothing else changes.

1. **The intake pipeline** (proposals 1 and 3; F14; findings 1 and 6).
   - **Builds:** `sheaf_ops::intake` (`Pages`, batches of 8 pages or 2
     seconds as one job group, a `Landing` trait with the Session's);
     `sheaf_ops::scan` as intake over image files; pages encoded as
     they're cleaned (G4 for black and white); the two cleanup profiles
     and `sheaf scan --profile`; `JobKind::Scan` and `GroupKind::Scan`;
     `State::scan` split by profile.
   - **Deletes:** `scan`'s collect-then-insert path and its
     `Vec<PageImage>`.
   - **Checks:**
     - F14: `sheaf scan --ocr` on three images finds their words, one
       OCR block a page, one font, on every CI runner.
     - Memory: 500 synthetic black and white Letter pages at 300 dpi
       scan with a peak under 400 MB above the process's start (today's
       path would need 4.2 GB).
     - The same images make the same pages as before the slice: each
       page rendered and compared with the old path's at 0 pixels of
       difference, and the same text layers.
     - The scanner profile: a synthetic flatbed scan (a white page with
       text on a white lid, 3° of skew) comes out straightened and not
       cropped; with the photo profile, the slice notes say what it did
       (finding 6).
     - An intake is one undo step however many batches it took, and
       two if the user edits in between.
2. **Documents with no file yet** (proposal 2, ask 1; B11).
   - **Builds:** `DocView` with no path ("Untitled scan 1", "Untitled
     scan 2"); Save is Save As there; closing asks; the recovery sidecar
     in the state folder as a Full sidecar, offered at the next launch;
     Create from Scans streams its pages into an untitled document in a
     new or empty window, as they're cleaned.
   - **Deletes:** `create_from_scans`' save-and-`open_paths` (B11), the
     dialog's ask-for-a-path step.
   - **Checks** (headless app tests, Linux only, as the GPUI tests are):
     - Create from Scans of three images: an untitled window, three
       pages, one undo step, no file anywhere; Save As writes the file,
       and the window takes its name and path.
     - A process killed after the first batch of a 20-image scan: the
       next launch offers the untitled document's recovery with those
       pages.
     - Closing an untitled document with pages asks; one with none
       doesn't.
     - B11 moves to Closed in the backlog, with this slice.
3. **Reading codes** (proposals 14 and 15).
   - **Builds:** `sheaf_image::codes`: barcodes through rxing (ask 7;
     `default-features = false`, so its `image` pin doesn't hold
     sheaf's back), one decode a page for sheaf's sheet and the rules'
     symbologies at once (`TryHarder`, formats limited); sheaf's own
     patch-code reader (types 1, 2, 3, 4, 6 and T: bars along an edge,
     by wide and narrow widths); a QR code's modules for the writer; the
     bench's degradations as a test helper.
   - **Checks:**
     - Each symbology at 150, 200 and 300 dpi, in gray and thresholded
       black and white, turned 90°, 180° and 270°, skewed ±3°, with noise
       and blur, read back; Alliance's patch-T sheet and Kodak's spec's
       bars, drawn and degraded the same way, read as their types.
     - A blank page, a page of text, and a page of table rules read as
       nothing.
     - **Speed gate:** a Letter page with no code, decoded for sheaf's
       sheet plus patch codes at 150 dpi, takes no more than 60 ms on one
       thread in a release build (the bench: 44 ms plus 5); the gate
       measures on every run and asserts with `SHEAF_GATE_CODES`, as the
       other gates do.
     - **Corpus:** every page of the 709 files under `scans/` (5,697
       pages), drawn at 150 dpi: no sheaf code and no patch code found
       (any found is a false positive to fix), and the other barcodes
       found are listed as the baseline in the slice notes (they're
       content, like the forms' Code 39 ids).
4. **Separator sheets** (proposals 13 and 17).
   - **Builds:** `PageSource::Separator { sheet }`,
     `MemoryKind::Separator`, `memory::separator`; Document > Insert
     Separator Sheets… (above top-level bookmarks with or without their
     children, above the selected pages, above typed pages; titles in an
     editable list); File > Create > Separator Sheets… (a new untitled
     document of sheets); `sheaf separator-sheets`.
   - **Checks:**
     - The same sheet makes the same bytes; a sheet's code reads back
       from its own render at 150, 200 and 300 dpi, thresholded, at four
       turns.
     - Insert above every top-level bookmark of a forms-corpus file with
       bookmarks keeps each bookmark on its page, one undo step; an
       Append save and a Rewrite of it reopen alike.
     - A title with characters outside WinAnsi draws `?` and its code
       keeps the title exactly.
     - `sheaf edit` inserts a sheet from JSON; its schema lists it.
5. **Separate** (proposals 12, 15, 16 and 18).
   - **Builds:** `separate::{find, plan, apply}`; Document > Separate…
     with its rules and the review sheet; the split to a folder;
     `sheaf separate`.
   - **Checks:**
     - **The round trip:** a file with top-level bookmarks gets sheets
       inserted above them; every page is drawn to an image at 200 dpi
       and degraded as a scan (skew, noise, black and white); `sheaf scan
       --profile scanner` makes a PDF of the images; `sheaf separate`
       gives back the same titles at the same pages, and with `--action
       split`, files of the same page counts.
     - Sheets fed upside down; a blank back after each sheet (duplex);
       two sheets in a row; a sheet on page 1; repeated titles (`_2`);
       pages before the first sheet ("Start").
     - A keyword rule doesn't take a page that mentions the keyword in a
       paragraph; a patch-T rule drops T sheets and a patch-2 rule keeps
       its page, as configured.
     - A split of a signed file stops with exit 3 (`NeedsDecision`), and
       nothing is written.
     - **Speed gate:** finding separators in the corpus's longest scan
       (`scans/safedocs/0055624.pdf`, 297 pages) takes no longer than
       drawing its pages at 150 dpi plus 60 ms a page divided by the
       decode threads.
6. **Scanners: the worker and SANE** (proposals 4, 5, 6, 9 and 22).
   - **Builds:** `sheaf-scanner` (model, protocol, client, `fake`, `sane`),
     `sheaf-scan-worker`, sheaf's fake libsane (tests), `sheaf scan
     --list-devices` and `--device` (intake from a scanner into a
     Session), `--separate`; `[scanner]` in the settings template; CI
     installs `libsane1`; the `ldd` and crate gates.
   - **Checks:** the `fake` backend's scripts (proposal 22) on every
     runner; the fake libsane's cases and SANE's test backend on Linux;
     a hang is killed by the watchdog with pages kept; a crash keeps
     pages and says the exit; `--list-devices` with SANE missing says
     why and exits 1; by hand, a SANE scanner (ask 12).
7. **Scanning in the app** (proposals 2, 10 and 11).
   - **Builds:** scanners as a source in the Create from Scans dialog
     (File > Create > From Scanner… opens it on that source); Document >
     Insert Pages > From Scanner…; streaming into the document; Scan
     More, Scan Backs, Continue after a jam, Cancel; the device list in
     the background with Refresh; the last scanner at once; "Separate
     as it scans"; the docs' ScanSnap Home Send to page.
   - **Checks** (headless app tests with the `fake` backend): pages
     stream into an untitled document and into the open one before page
     3, one undo step each; a jam at sheet 4, then Continue, gives every
     page once in order; Scan Backs interleaves; Cancel keeps the pages
     so far. By hand on each system with a real scanner.
8. **Windows: TWAIN** (proposal 7; asks 3 and 13).
   - **Builds:** first the spike: TWAIN's sample data source installed
     silently on `windows-latest`. Then the `twain` backend, the 32-bit
     worker (if ask 3 keeps it), `TWAINDSM.dll` bundled, Scanner
     settings… (the driver's window).
   - **Checks:** with the spike working, CI scans 10 images through the
     sample source in both bitnesses; the error mapping in unit tests;
     by hand, the TWAIN scanners from ask 12.
9. **Windows: WIA** (proposal 8).
   - **Builds:** the `wia` backend on its STA thread, the feeder and
     duplex, the errors by code.
   - **Checks:** the property and error mapping against a fake of the
     WIA calls; by hand, a WIA scanner and a network copier.
10. **Docs and the roadmap.** README's rows (Insert from scanner, Target
    sheets: finding 7), its subcommand table (`scan`'s device flags,
    `separate`, `separator-sheets`), ROADMAP's M5 as done and the
    Pending entry for macOS (if ask 4 says so), `docs/` pages on
    scanning (each system; ScanSnap Home's Send to) and on separator
    sheets (printing them: 100% scale, from the PDF, not a photocopy).

## Risks

- **Every scanner's driver is different,** and CI sees none of them. The
  hand matrix (ask 12) is the only check of real drivers; until a
  scanner has been through it, its row says untested.
- **TWAIN in a worker with a hidden window.** Some drivers insist on
  their own window, or misbehave without a visible owner; the fallback
  is the driver's window (`ShowUI`), which sheaf can always offer.
- **The 32-bit worker** adds a target to the Windows release build and
  four signed files.
- **WIA's quirks** (NAPS2's list: feeders that need a different
  download loop, undocumented codes) show only on real devices.
- **Reading codes on poor scans.** A 1-bit 200 dpi scan of a photocopied
  sheet is the worst case. The sheet is designed for it (two codes,
  0.75 mm modules, level M, black on white) and the review shows every
  separator, so a miss is seen before anything is split. zxing-cpp is
  the second engine if rxing misses on real scans (ask 7).
- **Barcodes in content.** Tax forms carry barcodes. sheaf's sheets need
  the `SHEAFSEP` prefix; other software's need a pattern; the review
  shows what each read.
- **Untitled documents** touch recovery, recent files, window titles and
  "is this file open" (`find_open`), all of which assume a path today.
- **Speed on big binders** is the renders' (Separate draws every page);
  decoding a scanned page's own image instead of drawing it (Stirling
  PDF does) is the fallback if the gate isn't met.
- **Licenses.** sheaf ships no SANE; TWAINDSM is LGPL-2.1, shipped
  unmodified and loaded at run time; Mopria's eSCL specification isn't
  used; NAPS2 (GPL-2.0+, its SDK LGPL-2.1+) is read for what it learned,
  never copied.

## Not in M5

- **Watched folders** (sheaf picking up whatever a copier drops into a
  folder, separating and filing it): M15's batch actions, on `sheaf
  separate`.
- **Scanners' own barcode and patch-code results** (TWAIN's extended
  image info, PaperStream's job separation): later, if a firm's scanner
  does better than sheaf's own reading. sheaf reads the codes in the
  pixels, the same on every scanner.
- **CCH target sheets by their own encoding:** CCH's sheets are its own
  barcodes, read by CCH Scan's engine. If their text is a standard
  symbology, a barcode rule reads them; a mapping of CCH's catalog codes
  to titles is not planned.
- **Sorting and naming pages by form** (W-2, 1099, K-1) without
  separators: that's classification, which the `tax-form-bookmarks`
  plugin does from text; CCH Scan's catalog and AutoFlow's data
  extraction are not planned.
- **Scanner settings beyond the common ones** (gamma, a driver's own
  dropout color, its own deskew and crop): not exposed. sheaf does its
  own cleanup, the same on every scanner.
- **A scanner preview scan** (a quick low-resolution flatbed pass to
  pick an area): later, if flatbed users ask.
- **Flatpak, Snap and the Microsoft Store:** sheaf ships bundles today
  ([`installer.md`](installer.md)). What each sandbox needs for scanners
  is under proposal 21, for when packaging comes.

## Sources

Checked 2026-10-23.

- **This machine and the code:** sheaf at `3e1b9f0`; sane-backends'
  `scanimage` 1.2.1 with libsane1 1.4.0, sane-airscan 0.99.36 and
  hplip 3.24.4; the `test` backend through a private `SANE_CONFIG_DIR`;
  `../pdf-corpus/scans/` (709 files, `manifest.tsv`).
- **SANE:**
  [the API](https://sane-project.gitlab.io/standard/api.html),
  [image data](https://sane-project.gitlab.io/standard/environment.html),
  [`saneopts.h`](https://gitlab.com/sane-project/backends/-/blob/master/include/sane/saneopts.h),
  [LICENSE](https://gitlab.com/sane-project/backends/-/blob/master/LICENSE),
  [`dll.c`'s exception](https://gitlab.com/sane-project/backends/-/blob/master/backend/dll.c),
  [`scanimage.c`](https://gitlab.com/sane-project/backends/-/blob/master/frontend/scanimage.c),
  [fujitsu.desc](https://gitlab.com/sane-project/backends/-/blob/master/doc/descriptions/fujitsu.desc),
  sane-devel on [threads](https://lists.alioth.debian.org/pipermail/sane-devel/2002-December/045285.html),
  [ADF](https://lists.alioth.debian.org/pipermail/sane-devel/2002-December/045351.html)
  and [fujitsu's page-height](https://lists.alioth.debian.org/pipermail/sane-devel/2011-June/028762.html);
  [`sane-scan`](https://crates.io/crates/sane-scan),
  [ruscan](https://github.com/iruizr7/ruscan);
  simple-scan's [`scanner.vala`](https://gitlab.gnome.org/GNOME/simple-scan/-/blob/master/src/scanner.vala),
  gscan2pdf's [`Image_Sane.pm`](https://github.com/carygravel/gscan2pdf/blob/master/lib/Gscan2pdf/Frontend/Image_Sane.pm),
  libinsane's [`workarounds.h`](https://gitlab.gnome.org/World/OpenPaperwork/libinsane/-/blob/master/subprojects/libinsane/include/libinsane/workarounds.h)
  and [scan APIs](https://gitlab.gnome.org/World/OpenPaperwork/libinsane/-/blob/master/subprojects/libinsane/doc/scan_apis.dox).
- **eSCL and network scanning:**
  [Debian wiki](https://wiki.debian.org/eSCL),
  sane-airscan's [README](https://github.com/alexpevzner/sane-airscan),
  [`airscan-escl.c`](https://github.com/alexpevzner/sane-airscan/blob/master/airscan-escl.c),
  [`airscan-mdns.c`](https://github.com/alexpevzner/sane-airscan/blob/master/airscan-mdns.c),
  [`airscan.h`](https://github.com/alexpevzner/sane-airscan/blob/master/airscan.h),
  [LICENSE](https://github.com/alexpevzner/sane-airscan/blob/master/LICENSE)
  and [sane-airscan(5)](https://github.com/alexpevzner/sane-airscan/blob/master/sane-airscan.5.md);
  [Mopria's specification download](https://mopria.org/spec-download);
  [a PWG post on the job flow](https://pwg.org/archives/ipp/2019/020120.html);
  [WS-Scan](https://learn.microsoft.com/en-us/windows-hardware/drivers/image/wsd-scan-service-operation-elements);
  [Windows 11's eSCL issue](https://www.theregister.com/2024/11/26/microsoft_escl_issue/);
  [OpenPrinting on eSCL replacing SANE](https://lists.linuxfoundation.org/pipermail/printing-architecture/2021/003989.html);
  [ipp-usb](https://github.com/OpenPrinting/ipp-usb);
  test servers: [go-mfp](https://github.com/OpenPrinting/go-mfp),
  [AirSane](https://github.com/SimulPiscator/AirSane);
  crates: [`escl`](https://crates.io/crates/escl),
  [`mdns-sd`](https://crates.io/crates/mdns-sd).
- **Windows:**
  [WIA in the `windows` crate](https://microsoft.github.io/windows-docs-rs/doc/windows/Win32/Devices/ImageAcquisition/index.html),
  [feeder item trees](https://learn.microsoft.com/en-us/windows-hardware/drivers/image/implementing-feeder-scanner-item-trees),
  [duplex](https://learn.microsoft.com/en-us/windows-hardware/drivers/image/simple-duplex-capable-document-feeder),
  [`WIA_IPS_PAGES`](https://learn.microsoft.com/en-us/windows-hardware/drivers/image/wia-ips-pages),
  [WIA error codes](https://learn.microsoft.com/en-us/previous-versions/windows/desktop/wia/-wia-error-codes),
  [WinRT's ImageScanner](https://learn.microsoft.com/en-us/uwp/api/windows.devices.scanners.imagescanner.scanfilestofolderasync),
  [sample WIA drivers](https://learn.microsoft.com/en-us/windows-hardware/drivers/image/sample-wia-drivers),
  [fakescan](https://github.com/b1tninja/fakescan);
  TWAIN: [twain-dsm](https://github.com/twain/twain-dsm) and its
  [license](https://github.com/twain/twain-dsm/blob/master/TWAIN_DSM/license.txt),
  [the sample data source](https://github.com/twain/twain-samples/blob/master/TWAIN-Samples/Twain_DS_sample01/README.txt)
  and its [v2.5.0 installers](https://github.com/twain/twain-samples/releases/tag/v2.5.0),
  twain.org forum threads on [TWAINDSM vs TWAIN_32](https://twain.org/forums/topic/does-twaindsm-dll-replace-twain_32-dll/),
  [twain_32 and twain_64](https://www.twain.org/forums/topic/question-get-datasource-file-from-twain_64-folder/)
  and [the message loop](https://twain.org/forums/topic/twain-application-windows-c-callbacks-win-message-loop/),
  [TWAIN Direct](https://www.twaindirect.org/specification/),
  [the `twain` crate](https://crates.io/crates/twain),
  [RSWebTWAIN](https://github.com/seancampbell3161/RSWebTWAIN).
- **Scanners and drivers:**
  [PaperStream IP TWAIN x64](https://www.pfu.ricoh.com/global/scanners/fi/dl/setup/psip-twain64-3300.html),
  [fi-series drivers](https://www.pfu.ricoh.com/global/scanners/fi/dl/win-10-fi-8x70.html),
  [PFU's multi-OS drivers](https://www.pfu.ricoh.com/fi/software/multios-driver/),
  [Epson Scan 2's bitness](https://www.epson.eu/en_EU/faq/KA-01076/contents),
  Canon [DR-M260](https://asia.canon/en/support/0100967919) and
  [DR-C240](https://canon.jp/support/software/os/select/dr/DR-C240_Driver_V141171218001SP4_Windows),
  [Canon DR-S250N](https://www.canon-europe.com/business/products/scanners/document-scanners/imageformula-dr-s250n/specifications),
  [Brother ADS-4700W](https://brother-usa.com/-/media/brother/product-catalog-media/documents/ads-4700w-2-page-brochure.pdf),
  [Kodak S2000](https://alarisworld.com/solutions/document-scanners/desktop/s2080w-scanner);
  ScanSnap: [no TWAIN or ISIS](https://scansnap-faq.pfu.ricoh.com/hc/en-us/articles/27610858674713-Does-the-ScanSnap-Series-support-TWAIN-or-ISIS),
  [no WIA](https://www.technibble.com/forums/threads/wia-driver-for-fujitsu-scansnap-ix1600.87977),
  [its SDK](https://www.pfu.ricoh.com/scansnap/support/sdk/info-sdk.html),
  ScanSnap Home's [applications](https://www.pfu.ricoh.com/imaging/downloads/manual/ss_webhelp/en/help/webhelp/topic/ope_appli_mgr.html)
  and [profiles' Send to](https://www.pfu.ricoh.com/imaging/downloads/manual/ss_webhelp/en/help/webhelp/topic/ope_profile_make.html),
  [ScanSnap Home's split](https://scansnap-faq.pfu.ricoh.com/hc/en-us/articles/21000580939801-How-can-I-scan-multiple-documents-and-save-them-as-separated-PDF-files-in-a-batch);
  tax software: [FileCabinet CS](https://www.thomsonreuters.com/en-us/help/filecabinet-cs/send-to-fcs/determining-if-a-scanner-is-twain-compliant.html),
  [Drake Documents](https://kb.drakesoftware.com/KB/Drake-Documents/11416.htm) (search excerpt only),
  [TaxDome](https://help.taxdome.com/article/563-scanners-troubleshooting),
  [Scan123](https://knowledge.scan123.com/recommended-scanners).
- **macOS:**
  [`ICScannerDevice`](https://developer.apple.com/documentation/imagecapturecore/icscannerdevice),
  [the feeder unit](https://developer.apple.com/documentation/imagecapturecore/icscannerfunctionalunitdocumentfeeder),
  [`objc2-image-capture-core`](https://crates.io/crates/objc2-image-capture-core),
  [ScannerBrowser](https://developer.apple.com/library/archive/samplecode/ScannerBrowser/Listings/AppController_m.html),
  [a forum thread on finding no USB scanners](https://developer.apple.com/forums/thread/771629),
  [ICA bypassed for AirScan](https://support.lexmark.com/content/support/guides/en/v56544321/other-troubleshooting/software-and-solutions/image-capture-application-ica-bypasses-custom-driv.html),
  [TN3179, Local Network privacy](https://developer.apple.com/documentation/technotes/tn3179-understanding-local-network-privacy),
  [TWAIN on macOS](https://support.alarisworld.com/ps50-ps80-driver-for-mac),
  [VirtualScanner](https://developer.apple.com/library/archive/samplecode/VirtualScanner/Introduction/Intro.html).
- **Sandboxes:**
  [xdg-desktop-portal #218](https://github.com/flatpak/xdg-desktop-portal/issues/218),
  [Simple Scan's Flatpak](https://github.com/flathub/org.gnome.SimpleScan/blob/master/org.gnome.SimpleScan.json),
  [NAPS2's Flatpak](https://github.com/cyanfish/naps2/blob/master/NAPS2.Setup/config/linux/com.naps2.Naps2.yml),
  [Snap's simple-scan](https://forum.snapcraft.io/t/auto-connect-request-for-simple-scan/36173).
- **NAPS2:** [repository](https://github.com/cyanfish/naps2),
  [SDK README](https://github.com/cyanfish/naps2/blob/master/NAPS2.Sdk/README.md),
  [`SaneScanDriver.cs`](https://github.com/cyanfish/naps2/blob/master/NAPS2.Sdk/Scan/Internal/Sane/SaneScanDriver.cs),
  [`TwainScanDriver.cs`](https://github.com/cyanfish/naps2/blob/master/NAPS2.Sdk/Scan/Internal/Twain/TwainScanDriver.cs),
  [`WiaScanDriver.cs`](https://github.com/cyanfish/naps2/blob/master/NAPS2.Sdk/Scan/Internal/Wia/WiaScanDriver.cs),
  [`ScanBridgeFactory.cs`](https://github.com/cyanfish/naps2/blob/master/NAPS2.Sdk/Scan/Internal/ScanBridgeFactory.cs),
  [`BarcodeDetector.cs`](https://github.com/cyanfish/naps2/blob/HEAD/NAPS2.Sdk/Scan/BarcodeDetector.cs),
  [`SaveSeparatorHelper.cs`](https://github.com/cyanfish/naps2/blob/HEAD/NAPS2.Lib/ImportExport/SaveSeparatorHelper.cs),
  [batch scan](https://naps2.com/doc/batch-scan),
  [troubleshooting](https://www.naps2.com/doc/troubleshooting),
  issues [#449](https://github.com/cyanfish/naps2/issues/449),
  [#581](https://github.com/cyanfish/naps2/issues/581),
  [#904](https://github.com/cyanfish/naps2/issues/904),
  [#916](https://github.com/cyanfish/naps2/issues/916),
  [its CI](https://github.com/cyanfish/naps2/blob/master/.github/workflows/dotnet.yml).
- **Separators, tax software:** CCH Scan Workstation help:
  [Using target sheets](https://www.scanautoflow.com/ophelp/v22/en/Scan_WkStn/Content/Working/UsingTargetSheets.htm),
  [sheets](https://www.scanautoflow.com/ophelp/v22/en/Scan_WkStn/Content/OP_and_SaaS_SharedTopics/sheets.htm),
  [separator sheets](https://www.scanautoflow.com/ophelp/v22/en/Scan_WkStn/Content/OP_and_SaaS_SharedTopics/separator_sheet.htm),
  [Insert Target Sheets](https://www.scanautoflow.com/ophelp/v22/en/Scan_WkStn/Content/PDFlyer/Insert_Target_Sheets.htm),
  [Funnel's inserts](https://www.scanautoflow.com/ophelp/v22/en/Scan_WkStn/Content/ScanFunnel/SF-Insert-Cover-Pages-Target-Sheets.htm)
  and [tips](https://www.scanautoflow.com/ophelp/v22/en/Scan_WkStn/Content/ScanFunnel/SF-Target-Sheet-Tips.htm),
  [the 2011 Scan user guide](https://manualzilla.com/doc/5813161/--support),
  [Scan 17.0's notes](https://support.cch.com/updates/Scan/pdf/ReleaseNotesScan17.0.pdf);
  SurePrep's [target sheets](https://www.thomsonreuters.com/en-us/help/sureprep/scanners/target-sheets.html),
  [their PDF](https://www.thomsonreuters.com/content/dam/helpandsupp/en-us/Topics/sureprep-family/files/Target-Sheets/all-target-sheets.pdf)
  and [scanning for SPbinder](https://www.thomsonreuters.com/en-us/help/sureprep/scanners/scanning-documents-for-spbinder.html);
  GoFileRoom ScanFlow's [cover sheets](https://www.thomsonreuters.com/en-us/help/gofileroom/scanflow/use/generate-scanflow-barcode-cover-sheets)
  and [scanning](https://www.thomsonreuters.com/en-us/help/gofileroom/scanflow/use/scan-documents-with-scanflow).
- **Separators, capture software:** Kofax Capture's
  [Separator Sheet Utility](https://docshield.tungstenautomation.com/KC/en_US/11.1.0-40hy9nfk91/help/main/KC_separatorsheetutility/c_sepsheetutil.html)
  and [separation profiles](https://docshield.tungstenautomation.com/KC/en_US/11.1.0-40hy9nfk91/help/main/KC_administration/c_customsepformidprofilewin.html),
  [TotalAgility](https://docshield.tungstenautomation.com/KTA/en_US/7.8.0-dpm5ap0jk8/help/TA/All_Shared/Capture/t_profilesepforadocument.html),
  [ReadSoft Scan Client](https://docshield.tungstenautomation.com/RSXBSC/en_US/5.10/help/XBSC/Content/procedures/setup/Using_barcodes_for_document_separation.htm),
  [Kofax Express](https://docshield.tungstenautomation.com/express/en_us/3.3.0-8vueggfgfv/help/express_help/Express_help/c_barcodeindexing.html),
  [Power PDF's split](https://docshield.tungstenautomation.com/PowerPDF/en_US/5.0.0-3uoz7ssq2b/help/PowerPDF_help/c_splitdocument.html),
  Kodak Capture Pro's [patch codes](https://resources.kodakalaris.com/support/Capture%20Pro%20Help/Content/About_patch_codes.htm)
  and [bar code zones](https://resources.kodakalaris.com/support/Capture%20Pro%20Help/Content/Separating_documents_with_bar_code_zones.htm),
  [PaperStream Capture](https://www.pfu.ricoh.com/global/scanners/fi/paperstreamcapture/)
  and [its change history](https://www.pfu.ricoh.com/global/scanners/fi/dl/update/history/psc-301-change.pdf),
  [Epson Document Capture Pro](https://files.support.epson.com/htmldocs/wfds50k/wfds50kug/scan2_2.htm),
  ABBYY [FineReader Server](https://help.abbyy.com/en-us/finereaderserver/14/help/idd_dialogseparationoptions/)
  and [FlexiCapture](https://help.abbyy.com/en-us/flexicapture/12/scanning_station/batch_type_separation/),
  [DocuWare](https://support.docuware.com/en-US/knowledgebase/article/KBA-37139),
  [capturebites on sheet design](https://www.capturebites.com/metaserver/help/separator-sheet-design).
- **Separators, editors and open source:**
  [Evermap AutoSplit](https://evermap.com/AutoSplit-Split.asp),
  [Foxit's split](https://resource-cms.foxit.com/foxit-pdf-editor-for-windows/split-pdfs-multiple-files),
  [PDF-XChange's split](https://help.pdf-xchange.com/pdfxt8/split-document_ed.html),
  Paperless-ngx's [barcode settings](https://docs.paperless-ngx.com/configuration/#barcodes),
  [`barcodes.py`](https://github.com/paperless-ngx/paperless-ngx/blob/dev/src/documents/barcodes.py)
  and [v3 migration notes](https://github.com/paperless-ngx/paperless-ngx/blob/dev/docs/migration-v3.md),
  [Stirling-PDF's auto split](https://github.com/Stirling-Tools/Stirling-PDF/blob/main/app/core/src/main/java/stirling/software/SPDF/controller/api/misc/AutoSplitPdfController.java),
  [scanprep](https://github.com/baltpeter/scanprep),
  [pss](https://github.com/kduma-OSS/CLI-pdf-scan-splitter),
  [ecoDMS's separator sheet](https://www.ecodms.de/en/download/ecodms-archive/separator-sheet).
- **Patch codes:** [Alliance's patch-T sheet](https://www.alliancegroup.co.uk/downloads/patch_t_us-letter.pdf)
  and [notes](https://www.alliancegroup.co.uk/patch-codes.htm),
  [Dynamsoft's patch code patterns](https://www.dynamsoft.com/barcode-reader/barcode-types/patch-code/)
  (a vendor page), [zxing-cpp's formats](https://github.com/zxing-cpp/zxing-cpp).
- **Barcode libraries:** [rxing](https://crates.io/crates/rxing)
  ([repository](https://github.com/rxing-core/rxing), its
  [Cargo.toml's `image` pin](https://docs.rs/crate/rxing/0.9.3/source/Cargo.toml),
  [`one_d_reader.rs`](https://github.com/rxing-core/rxing/blob/main/src/oned/one_d_reader.rs),
  [rxing-wasm](https://github.com/rxing-core/rxing-wasm)),
  [zxing-cpp](https://github.com/zxing-cpp/zxing-cpp) and
  [its crate](https://crates.io/crates/zxing-cpp),
  [rqrr](https://crates.io/crates/rqrr),
  [bardecoder](https://crates.io/crates/bardecoder),
  [zbar-rust](https://crates.io/crates/zbar-rust),
  [zedbar](https://github.com/eventualbuddha/zedbar),
  [zxing-bench](https://github.com/axxel/zxing-bench/blob/main/rust/README.md)
  (by zxing-cpp's author);
  writers [barcoders](https://crates.io/crates/barcoders),
  [qrcode](https://crates.io/crates/qrcode),
  [datamatrix](https://crates.io/crates/datamatrix)
  ([its PDF example](https://github.com/jannschu/datamatrix-rs/blob/main/examples/pdf.rs));
  [USS-39](https://live.expresscorp.com/uploads/specifications/47/USS-39.pdf),
  [QR](https://en.wikipedia.org/wiki/QR_code) and
  [Code 39](https://en.wikipedia.org/wiki/Code_39)'s character sets.
  The bench: a scratch crate (rxing 0.9.3, zxing-cpp 0.5.3, rqrr 0.11.0,
  qrcode 0.14.1, datamatrix 0.3.3, image 0.25.10), its results kept in
  the planning session's scratchpad, not in the repo.
- **Patch codes and scanned barcodes:** Kodak A-61599,
  [Patch Code Information](http://www.alliancegroup.co.uk/downloads/patch-codes.pdf)
  (Alliance's copy), [Kodak's Patches for Printing](http://www.alliancegroup.co.uk/downloads/patches-for-printing-on-us-letter-paper.pdf),
  [Patch Code](https://en.wikipedia.org/wiki/Patch_Code) on Wikipedia,
  [twain.h](https://github.com/twain/twain-dsm/blob/master/TWAIN_DSM/src/twain.h),
  Kofax Scan Client's [supported barcodes](https://docshield.tungstenautomation.com/RSXBSC/en_US/5.10/help/XBSC/Content/other/Supported_barcodes.htm),
  [ABBYY's sample separator](https://help.abbyy.com/en-us/finereaderserver/14/admin_guide/howto_configuredocumentseparation/),
  [PaperStream Capture's datasheet](https://www.fujitsu.com/us/imagesgig5/PaperStream_Capture_Datasheet_v2.pdf),
  [CaptureOnTouch Pro](https://canon-europe.com/business/products/software/captureontouch-pro/specifications),
  [Kodak's barcode tips](https://resources.kodakalaris.com/support/Capture%20Pro%20Help/Content/Tips_for_using_bar_codes.htm),
  [SimpleOCR's sizing guidance](https://www.simpleocr.com/?p=6085) (third-party).

## Slice notes

None yet.
