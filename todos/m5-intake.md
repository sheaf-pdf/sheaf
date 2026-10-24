# M5 remainder: scanner intake and separator sheets

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
  features `decoders, encoders, encoding_rs, oned, qrcode, datamatrix,
  pdf417` (as built: PDF417 is a feature of its own in 0.9.3, and ask 9
  makes it a rule's symbology; `multi_barcode_readers` stays off: a page
  is decoded for one code of each symbology asked for).
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
  least 0.75 in long (2 in for Kodak's Patch-Reader scanners; as built,
  sheaf's reader asks 0.6 in, for a skewed or clipped sheet), at least
  0.25 in from the edges and other print, within 3.75 in of the leading
  edge; bars under 20% reflectance on paper over 65%; no
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
     - As built (slice 1): `Pages` also has `count()`, how many pages
       the stream has when it knows (image files count a TIFF's from
       its headers, `io::count_frames`), for progress, which counts
       pages; and intake's page is `intake::RawPage { image: RgbImage,
       name }`, the name for messages ("fax.tif, page 2: blank, left
       out"). Proposal 9's `RawPage` (bytes, dpi, sheet, side) is
       sheaf-scanner's, slice 6's, which maps a device's pages into
       intake's and may add what it needs then. A TIFF is read an image
       at a time from the file (`sheaf_image::io::frames`): its first
       page is `io::load`'s image, as `scan` read every file before, and
       its later ones are decoded as images-to-pdf decodes them;
       reduced-resolution images (thumbnails) aren't pages.
     - As built (slice 1): whatever stops an intake but its landing (a
       cancel, a page that can't be read, cleaned or recognized), the
       pages it had ready land first, so the pages so far stay (one undo
       step), and its error is returned. A cancel cuts short the page
       it comes during (that page's cleanup or OCR), which is left out.
   - **A page at a time:** clean (proposal 3), then blank (leave out,
     with `sheaf_image::blank` as today), then separators (proposal 15,
     only when asked: a separator is never OCRed), then OCR (one engine,
     built once, as today).
   - **Batches:** every 8 pages, or 2 seconds, whichever comes first,
     the batch becomes one `MemoryKind::Images` source (`made_pages`)
     and one commit: an `InsertPages`, plus `SetOcrText` for its pages.
     - As built (slice 1): the 2 seconds are since the last landing (the
       intake's start, for the first), and `Pages::next` is a blocking
       pull, so the clock is read as each page is done
       (`Batching::due(pages, since_last)`): a slow device's page lands
       right after it's cleaned, and a fast one waits for the page after
       it at most. `Batching::DEFAULT` is the app's and the devices'
       (slices 2, 6 and 7). The command line batches by count only
       (`scan::BATCHING`, 8 pages and no clock): a Rewrite numbers what
       an insert brought by its source, each batch's own, so batches cut
       by a clock would write the same pages in another object order;
       by count, the same images make the same file (recognized text's
       block ids aside, which are drawn at random).
     The commits of one intake are one group (a new `GroupKind::Scan`,
     made by `Actor::Job(JobKind::Scan)`), so they're one undo step, as an
     OCR run's flushes are; undoing it mid-scan stops the scan, as it
     stops OCR.
   - **Where it lands** is a trait too: a `Session` (the command line),
     or a document's planner in the app (the open document for Insert,
     a new one for Create). The app's batches commit as page inserts do
     (`plan_made`, `Base::At`), so the user can keep working while pages
     arrive.
     - As built (slice 2): every landing makes a batch's source in its
       document's store (`intake::batch_source`, `made_pages` before) and
       puts its insert at the end and its pages' text into a transaction
       it gives (`intake::apply_batch`), so a Session's landing and the
       app's share them; `plan_made` makes the insert alone.
     - As built (slice 2): the app's landing (`scan_run.rs`) plans each
       batch on the intake's own thread, on the version the view hands
       it with the scan job's lowering, and the view commits it with
       `Base::At` and `Stale::Relower` in the scan's group. Planned in a
       background task of its own, as a planner's edit is, a batch would
       need a free background thread while the intake's waits for the
       answer: with as many scans running as there are threads, none
       would be. An insert at the end reads no order (a sheaf-doc test
       pins it), so no edit of the user's conflicts with a batch, and
       none is planned again.
   - **Memory stays flat.** A page is encoded as soon as it's cleaned
     (JPEG for color and gray, CCITT G4 for black and white) and its
     pixels dropped. Today every page is held to the end, and a black and
     white page is held as 8-bit pixels (`Pixels::Bilevel`, 8.4 MB for
     Letter at 300 dpi): 500 pages hold 4.2 GB (found while planning).
     - As built (slice 1): a black and white page is held as its G4
       bytes from the moment it's made, by every reader
       (`Pixels::G4`, `PageImage::from_image`). The gate
       (`scan_of_500_black_and_white_pages` in gate_7k) asserts under
       400 MB added under both profiles: Photo, the command's default
       and the path the 4.2 GB was measured on, and Scanner.
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
       - As built (slice 2): the field is private to `docview.rs`, and
         only the open, recovery and save code reads it. Its other
         readers (about 20 in `docview.rs`, plus export.rs, jobs.rs,
         comments_panel.rs, attachments_panel.rs and workspace.rs's
         `shows`) go through `path() -> Option<&Path>`, `folder()` and
         `stem()`. The plugin host's `DocContext.path` and Properties'
         `PropertiesContext.path` are `Option`s, and WIT's
         `document.path()` was already `option<string>`.
       - As built (slice 2): the field is a `Place`, a file or an
         `Untitled` (`untitled.rs`: its name, the folder Save As starts
         in, its images', and the claim on its name and sidecar). The
         name is "Untitled scan" and the first number from 1 that no view
         claims, in any sheaf process, and no sidecar has;
         `file_name()`, the title and `stem()` give it, and `path()` is
         None. Save is Save As there, suggesting "<name>.pdf", and a save
         makes the place the file. Its model goes by the same name
         (`Session::new_empty_named`, and a recovered one's), so a
         header's `<<file>>` says it until it's saved.
     - Save of an untitled document is Save As; closing one with pages
       asks, as a changed one does;
     - its crash-recovery sidecar lives in the state folder
       (`sheaf_ops::dirs::state_dir()/recovery/`) as a Full sidecar
       (the format already holds a whole document,
       `MemoryKind::Recovered`), and the next launch offers it, as
       recovery does beside a file today. A long scan is then safe from
       a crash after its first batch.
       - As built (slice 2): a third kind of format 2, `untitled`
         (`sheaf-recovery 2\nuntitled\n`, the folder Save As starts in,
         in hex, and the whole document, written as Full's is), since a
         Full sidecar's head names a base file and an untitled document
         has none: `recovery::write_untitled`, `find_untitled` (the
         folder's unheld ones, by name, read as far as their heads),
         `restore_untitled`, and `Document::never_saved` for the
         restored document, so it's dirty and its first save is a
         Rewrite. `find` beside a file takes no untitled sidecar. Each
         name is claimed (`recovery::Claim`: a lock file's lock, held by
         the view and each write in flight), so two sheaf processes, or
         a closed view's last write and a new document, never share a
         sidecar, and a launch never offers a live one.
       - As built (slice 2): the app starts its write as each batch
         lands, or as soon as the write in flight ends, not on
         autosave's clock (batches every 2 s would keep putting that
         off), and on autosave after other edits; it's removed when
         nothing is unsaved, with Don't Save, when the view closes with
         nothing unsaved, and once the document is saved to a file. A
         write in flight then removes what it wrote (a flag the view
         shares with it), even once the view is gone.
         A new document's name skips the names claimed and those
         sidecars there have. At launch the process that owns sheaf's
         state offers each no live view holds in its first window
         ("Recover the unsaved scan …?", Recover or Discard), not over a
         `--script` run's; a recovered one opens in an empty window or a
         new one, as a file opens (its pages first), its Save As
         starting in the folder its sidecar says.
   - **The command line** lands in a `Session`, saved to `-o` at the end
     (as now).

3. **A scanner profile for cleanup.** `ScanParams::default()` is a phone
   photo's: find the page in the frame, even out the lighting. A
   scanner's page needs neither: the page is the image, the light is
   even, and the page finder can crop a white page on a white lid wrongly
   (to check in slice 1; it didn't on slice 1's flatbed scan, where the
   light evened out is the difference: finding 6). So cleanup gets two
   profiles, each row still switchable:

   | Step | Photo (files; today's default) | Scanner (devices) |
   |---|---|---|
   | Find the page, clear its edges | on | off |
   | Flatten curl (UVDoc) | off | off |
   | Even out lighting, white paper | on | off |
   | Turn upright | on | on |
   | Straighten | on | on |
   | Leave out blank pages | off | on with duplex, else off (files: off) |
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
        pub place: Place,              // Starts (the section after it; the default) | Ends (the section before it)
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
    - **`Place::Ends`** is for trailer separators: a sheet put after each
      document rather than before it, as Kofax Scan Client allows
      (header and trailer separators, each kept or dropped). A trailer
      ends the section before it, and pages after the last trailer are a
      section of their own titled "End". sheaf's own sheets always start
      a section; `Ends` is for other software's sheets, and starting is
      every rule's default.
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
      - As built (slice 3): `codes::read(page, dpi, symbologies, accept,
        reader)` reads the patch bars, then the barcode, both every time.
        The page is binarized once (`BinaryBitmap` caches it) and decoded
        for each symbology asked for alone, in a fixed order (QR, Data
        Matrix, PDF417, Code 39, Code 128), each with `TryHarder`, until a
        code `accept` takes: the caller's text match ("a code counts only
        if its text matches"), so a code turned down lets the next
        symbology's decode run. One `MultiFormatReader` call over several
        formats walks the 2D ones in a `HashSet`'s order, random for each
        reader, so a page with two codes read as either one and sheaf's QR
        didn't come first; and any call stops at its first code, so one
        decode for the first code would let a vendor's QR code or a form's
        Code 39 id hide a rule's code every time. rxing gives one code a
        decode, so a second code of the same symbology is still hidden by
        the first (Slice notes, Left for later slices).
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
    unless there are none. A trailer (`Place::Ends`, proposal 12) instead
    ends the section that runs up to it, and takes its title from the
    trailer's code. Then:
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
   pages. Proposal 1's batches fix it (slice 1: fixed, the gate's 500
   pages under 400 MB added in both profiles).
2. **Backlog B11** (Create from Scans writes a file and opens it again):
   proposal 2 and ask 1 (slices 1–2). Slice 2: fixed. Create from Scans
   streams its pages into an untitled document in a new or empty window,
   and nothing is written until Save, which is Save As (backlog: Closed).
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
   - Checked in slice 1, on a synthetic flatbed scan (a page of about
     247 on a lid of about 252, 3° of skew): the photo profile finds no
     page and keeps the whole scan, as the scanner profile does (both
     tones fall on the bright side of Otsu's level, so the outline is
     the whole image, which `find_page` takes as "the photo is the page
     already"), and straightens it as well; what differs is the light,
     paper and lid both 254 under the photo profile. Cropping is
     harmless there; the light evened out is what the scanner profile
     saves.
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
    offices use?** **Answered (Greg, 2026-10-23):** none at home; at
    work, several Fujitsu fi-6130 duplex color scanners, "old, but very
    good". Older hardware like this often works better than newer
    models, if sheaf can talk to it, so old drivers are a main case, not
    a fallback.
    - **The fi-6130** (2008; 40 ppm, 50-sheet feeder, ultrasonic
      double-feed detection) predates PaperStream IP. Its Windows driver
      is FUJITSU TWAIN32 (9.21.x), a 32-bit TWAIN source, so the 32-bit
      worker (ask 3) is needed for the office's own scanners. The
      fi-6130Z (2012) has PaperStream IP TWAIN x64 up to 3.10 only; 3.20
      dropped the Z models. Still to learn: which model, and which driver
      is installed.
    - **SANE's fujitsu backend** rates the fi-6130 and fi-6130Z
      complete, so a Linux laptop at work covers slice 6's hand checks.
    - **Later models:** fi-7140/7160 (PaperStream IP, 64-bit TWAIN),
      fi-8170 (70 ppm, 140 ipm duplex, USB 3.2 and LAN; SANE "good",
      Ethernet not supported). The scan-speed gate (slice 6) is sized for
      the fi-8170.
    - **Open:** whether a development build may run on a work PC (IT and
      unsigned programs; `todos/code-signing.md`), which decides when the
      Windows hand checks can happen.
13. **CI for scanners.** (proposal 22)
    - **Linux runners install `libsane1` and run SANE's test backend
      and sheaf's fake libsane; the TWAIN slice starts with a spike to
      run TWAIN's sample data source on the Windows runner; WIA is
      checked by hand.** Recommended.
    - Fakes only: faster CI, but the real SANE library and a real TWAIN
      source never run outside a person's machine.
14. **What do other software's separators do by default, and how are
    split files named?** (proposal 16)
    - **Bookmark, separator dropped, separators starting their section
      (not ending it); split files named by their titles alone.**
      Recommended: a binder with bookmarks is how CCH and PDFlyer users
      work; capture software names files from the sheet; trailers
      (`Place::Ends`) are a rule's option for firms whose sheets go
      after each document.
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
     - As built: the sidecar is a kind of its own, `untitled`, the whole
       document as Full's is, since a Full sidecar's head names a base
       file (proposal 2, and the slice notes).
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
     - As built: rxing's features add `pdf417` (Research); the one
       decode is one binarized bitmap decoded for each symbology alone in
       a fixed order, past codes the caller's text match turns down
       (proposal 15, as built); `codes::read` reads a page's patch code
       and its barcode together; and the test helper is `codes::fixture` behind
       sheaf-image's new `fixtures` feature (`cfg(any(test, feature =
       "fixtures"))`, as sheaf-doc's), since sheaf-ops' tests in slices 4
       and 5 draw and degrade codes too.
   - **Checks:**
     - Each symbology at 150, 200 and 300 dpi, in gray and thresholded
       black and white, turned 90°, 180° and 270°, skewed ±3°, with noise
       and blur, read back; Alliance's patch-T sheet and Kodak's spec's
       bars, drawn and degraded the same way, read as their types.
       - As built: noise is a scanner's, Gaussian σ 10 gray levels, and
         blur σ 1.5 px, each made at 300 dpi before the page is resampled
         to 200 or 150 and thresholded at 128 (the bench's σ 25 at 300 dpi
         failed every reader's 1D and PDF417 codes, and its office mix
         used σ 8). The sizes, fixed before the first run: QR at sheaf's
         0.75 mm (9 px at 300 dpi), Data Matrix at 0.68 mm (8 px), Code
         39 and Code 128 with 20 mil narrow bars (6 px) 0.5 in tall,
         PDF417 with 4 px modules in the encoder's 4-module rows. The
         patch layouts are drawn from their measurements (no PDFs in the
         repo): Kodak's label (one edge, A-61599's geometry), Kodak's
         printed sheet (all four edges, the first bar 0.62 in in) and
         Alliance's patch-T sheet (all four edges, 0.63 in in), each with
         the type as Code 39 in the middle. The bench's 17 degradations
         are kept as `fixture::BENCH`.
     - A blank page, a page of text, and a page of table rules read as
       nothing.
       - As built: sheaf-image has no font, so the fixture's text is
         glyph-like strokes; real glyphs come from PDFium in sheaf-ops: a
         lopdf-built Helvetica page with a ruled table in CI
         (`a_page_of_real_text_reads_as_nothing`), and the 7k fixtures'
         pages in the gate.
     - **Speed gate:** a Letter page with no code, decoded for sheaf's
       sheet plus patch codes at 150 dpi, takes no more than 60 ms on one
       thread in a release build (the bench: 44 ms plus 5); the gate
       measures on every run and asserts with `SHEAF_GATE_CODES`, as the
       other gates do.
       - As built: `codes_on_a_page_with_none` asserts four rows, each the
         median of 21 reads: the stroke page at 150 dpi in gray and in
         black and white, and the synthetic and heavy 7k's first pages
         drawn as separators are found (their content alone, 150 dpi
         gray); it measures every symbology at 150 gray and 200 in black
         and white, and the patch bars alone.
     - **Corpus:** every page of the 709 files under `scans/` (5,697
       pages), drawn at 150 dpi: no sheaf code and no patch code found
       (any found is a false positive to fix), and the other barcodes
       found are listed as the baseline in the slice notes (they're
       content, like the forms' Code 39 ids).
       - As built: sheaf-ops' `scans_hold_no_separators` (ignored,
         `SHEAF_CODES_ONLY`) draws each page with PDFium directly, its
         content alone in gray (the picture `separate::find` will draw
         through `Views` in slice 5, without a Session), and reads it for
         patch bars, for every symbology taking only a sheaf code or a
         patch sheet's Code 39 (each looked for past any other code), and
         for every symbology taking the first code (the baseline). Until
         slices 4
         and 5 define them, the sheet's `SHEAFSEP` prefix and the Code 39
         `PATCH…` text (a patch, proposal 14) are literals in the test.
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
     - A trailer rule: patch-T sheets after each document make the same
       sections as the same sheets before them under a starting rule,
       with "End" for pages after the last.
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
     why and exits 1; by hand, an fi-6130 through SANE's fujitsu
     backend on a Linux laptop at the office (ask 12).
   - **Scan-speed gate** (Greg, 2026-10-23): the intake keeps pace with a
     fi-8170, 140 images a minute duplex (2.3 a second) at 300 dpi, in
     color and in black and white, fed by the `fake` backend through the
     worker's protocol. Cleanup and encoding keep up with the feeder
     (pages cleaned and encoded at least as fast as they arrive, measured
     over 200 images after the first 20); OCR may fall behind and finish
     after the feeder stops, with memory bounded while it does (the
     waiting pages held encoded, not as pixels). The gate fixes the
     worker threads to an office PC's count (4 cores; the notes say the
     CPU it ran on), is measured on every run and asserted with
     `SHEAF_GATE_SCAN_SPEED`. Slice 1 measured cleanup at about 0.8 s a
     300 dpi Letter page on one thread under Scanner (3.2 s under Photo),
     so meeting 2.3 a second on 4 cores needs pages cleaned in parallel
     (a few in flight, landed in order), or a faster cleanup.
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
     so far; at slice 6's gate rate (2.3 images a second, 200 images)
     pages stream into the window with no UI frame over 50 ms on their
     account. By hand on each system with a real scanner.
8. **Windows: TWAIN** (proposal 7; asks 3 and 13).
   - **Builds:** first the spike: TWAIN's sample data source installed
     silently on `windows-latest`. Then the `twain` backend, the 32-bit
     worker (needed: the office's fi-6130s have a 32-bit TWAIN driver,
     ask 12), `TWAINDSM.dll` bundled, Scanner settings… (the driver's
     window).
   - **Checks:** with the spike working, CI scans 10 images through the
     sample source in both bitnesses; the error mapping in unit tests;
     by hand, an fi-6130 on FUJITSU TWAIN32 through the 32-bit worker,
     duplex, with a jam and a double feed (and on PaperStream IP x64 if
     the office's are fi-6130Z), once a build may run on a work PC (ask
     12).
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
  - As built (slice 3): rxing read every drawn cell of slice 3's matrix
    (every symbology at 150, 200 and 300 dpi, gray and 1-bit, turned,
    skewed, with noise and blur); real printed sheets are slice 5's hand
    checks.
- **Barcodes in content.** Tax forms carry barcodes. sheaf's sheets need
  the `SHEAFSEP` prefix; other software's need a pattern; the review
  shows what each read.
  - As built (slice 3): the corpus's scans read 25 codes on 19 files
    (QR, Data Matrix, Code 39, Code 128, PDF417; the slice notes list
    them) and no patch bars; none starts `SHEAFSEP`, and no Code 39
    `PATCH…` is found past any of them. A code whose text doesn't match
    never hides another symbology's code: the read goes on past it.
- **Untitled documents** touch recovery, recent files, window titles and
  "is this file open" (`find_open`), all of which assume a path today.
  - As built (slice 2): recovery has the `untitled` kind in the state
    folder; recent files take a view only once it has a file (`install`
    and `adopt` add a file's place alone, and a save adds its file as
    before); the title is the place's name; `find_open` matches a view by
    its `path()`, so an untitled one never matches a path opened.
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

1. Done. The intake pipeline (proposals 1 and 3; F14; findings 1 and
   6): scanned pages go into a document a page at a time through
   `sheaf_ops::intake`, from a `Pages` stream (image files here, a TIFF's
   pages one by one through the new `sheaf_image::io::frames`; devices
   from slice 6), through cleanup, blank pages and OCR. Each page is
   encoded the moment it's cleaned (black and white as CCITT G4, held so
   from construction as `Pixels::G4`; color and gray as JPEG) and its
   pixels dropped. Pages land in batches (`Batching`: 8 pages, or 2 s
   since the last landing; the command line by count only) through a
   `Landing`. The Session's commits each batch, an `InsertPages` of a
   `Memory(Images)` blob plus its pages' `SetOcrText`, as one commit of
   `Actor::Job(JobKind::Scan)` in one `GroupKind::Scan` group, so an
   intake is one undo step however many batches it took, and undoing it
   stops it. `sheaf scan` is intake over files into a Session, then a
   Rewrite, and a multi-page TIFF now gives all its pages. Cleanup has
   two profiles, Photo (today's) and Scanner (no page finding, no light
   evening), with `sheaf scan --profile photo|scanner` and the
   `--crop`/`--flatten` pairs, `State::scan` split by profile and the
   Create from Scans dialog's "Pages from" row. F14 is a CLI test of
   `scan --ocr` on every runner (backlog: Closed). 500 black and white
   Letter pages at 300 dpi scan with under 400 MB added in both profiles
   (the old path: 4.2 GB); the same images make the same pages as before
   at 0 px with the same text and the same word boxes; every asserted
   gate holds, and the corpus counts are #26's. Whatever stops an intake
   but its landing lands the pages it had ready first.
   - **G4 from construction, `io::frames`** (step 1). sheaf-image io.rs:
     `Pixels::G4(Vec<u8>)` in place of `Pixels::Bilevel`, encoded once by
     `PageImage::from_image` for an all-0/255 gray image (`encode::g4`);
     sheaf-doc's `images::image_stream` passes the bytes through under
     the same `CCITTFaxDecode` dictionary. `pub fn frames(path) ->
     Result<Frames>` (`Frames: Iterator<Item = Result<RgbImage>>`): the
     kind sniffed from the first 16 bytes; a TIFF's first page
     `load(path)`'s image, and its later ones read an image a `next()`
     by a `tiff` decoder over a `BufReader<File>` (boxed), each turned
     upright by its Orientation tag and made RGB as `load` does; anything
     else one frame, `load(path)`, read on the first `next()`; an error
     ends it. `tiff_pages`' per-image decode moved into `tiff_image<R:
     Read + Seek>`, which both use. `tempfile` a dev-dependency (CI's
     pixels-only gate checks normal dependencies). Tests:
     `a_black_and_white_page_is_held_as_g4` (a 2550x3300 page's G4 bytes
     `g4(raw)`, under 1% of its 8-bit size),
     `a_tiff_gives_its_pages_one_at_a_time`,
     `any_other_image_is_one_frame_as_load_reads_it`; encode.rs'
     `tiff_round_trips_through_the_reader` and images.rs'
     `png_gray_and_bilevel` on `Pixels::G4`. (The review's fixes
     replaced step 1's `a_tiff_frame_is_what_load_reads`: Fixes after
     review.)
   - **The scan job and its group** (step 2). caps.rs: `JobKind::Scan`,
     which `actor_allows` lets make `InsertPages` and `SetOcrText` and
     nothing else, `actor_exports` false; the arms kept apart from the
     plugin's (slice 20 edits the same file). model.rs: `GroupKind::Scan`,
     which takes `Job(Scan)`'s commits only (`takes`). fixture.rs'
     `commit_ops_in_group` commits a Scan group's ops as the scan job.
     history_tests.rs: an `unreachable!` arm (Deviations). Tests:
     `caps_cover_every_op_target_and_actor` with `Job(Scan)`;
     group_tests.rs' `a_scan_group_takes_the_scan_job_s_batches` (three
     batches by `PageRef::New` and `PageRef::Pos`, the first not amending
     and the next two amending, one version, a `Job(Ocr)` commit in the
     group a bug, a `DeletePages` by the scan job `Refused(Cap::Plugin)`,
     one undo back to the start), the catch_unwind check shared with
     `a_commit_in_a_group_by_another_actor_is_a_bug` as
     `commit_is_a_bug`.
   - **The intake** (step 3). sheaf-ops intake.rs (new): `RawPage { image,
     name }`; `trait Pages { next(p), count() }`; `Files::new(&[PathBuf])`
     ("no images given" for none; a TIFF's later pages named "…, page
     k"; progress by file, by page since the review); `Batching {
     pages, wait }`, its `DEFAULT` and the pure `due(pages,
     since_last)`, an empty batch never due; `Batch { pages, words, fit,
     engine }`; `trait Landing`; `SessionLanding {
     new, session, finish }`, the group begun by `new` and ended by
     `finish`, whose `land` applies the insert, takes the `Made::Page`
     ids and their geoms from `tx.state()`, applies a `SetOcrText` a page
     on the same Tx and commits once (`CommitOpts { group: Some(g),
     ..labelled }`), a `GroupEnded` coming back as that error;
     `intake(pages, opts, engine, landing, batching, p)`: check, clean,
     the turned and blank messages as before, OCR of the cleaned image,
     then the page encoded (black and white through `from_image`, which
     takes the image by value) and its pixels dropped, a batch landed
     when `due` says and the rest at the end, a landing's error stopping
     it before another page is read (and, since the review, any other
     error landing the rest first). `made_pages` (pub(crate); images.rs
     calls it there), `to_page` and `JPEG_QUALITY` moved there from
     scan.rs. scan.rs' `scan()`: `Files`, the engine built as before,
     `Session::new_empty`, `SessionLanding::new(…, "Scan")`, `intake` by
     `BATCHING`, `finish`, "every page came out blank" when none landed,
     the Rewrite to `out`. Tests (sheaf-ops tests/intake.rs, new: seeded
     850x1100 pages, a fake engine `Lines` reading a word a run of dark
     rows, orientation off, `wait: None`):
     `batches_make_the_pages_one_batch_makes`,
     `an_intake_is_one_undo_step`,
     `an_edit_between_batches_makes_two_undo_steps`,
     `undoing_an_intake_stops_it`, `a_multi_page_tiff_scans_page_by_page`;
     intake.rs' `a_batch_is_due_at_its_count_or_after_its_wait`.
     docs/document-model.md's scan bullet.
   - **Two cleanup profiles** (step 4). scan.rs: `Profile::{Photo,
     Scanner}` (kebab-case, Photo the default, proposal 3's table in its
     doc), `Profile::options()` (Photo `ScanOptions::default()`, Scanner
     the same with `crop` and `flatten` off), `ScanChoices { profile,
     photo, scanner }` (`#[serde(default)]`, `get`, `set`). sheaf-cli
     scan.rs: `--profile photo|scanner` (photo by default), `--crop`/
     `--no-crop` and `--flatten`/`--no-flatten` (`overrides_with`: the
     last given wins, and with neither the profile decides), `--mode`,
     `--page`, `--dpi` and `--threshold` as `Option`s, all through
     `options(&ScanArgs)`; the "N cropped from the photo" count printed
     only when crop is on; the help for both kinds of pages. App:
     `State::scan` a `ScanChoices`; the dialog's first row, "Pages from"
     ("Photos", "A scanner or copier"), a switch keeping the shown
     profile's edits for a switch back, loading the other's (`usable()`
     turning off OCR and curl where they can't run here) and refreshing
     the preview; `ScanEvent::Created { path, profile, opts }`, which
     saves that profile's options and the profile. Tests:
     `the_scanner_profile_neither_crops_nor_evens_the_light`,
     `each_profile_keeps_its_own_choices`, the CLI's
     `the_profile_decides_what_no_flag_does` and
     `of_a_flag_and_its_no_the_last_wins`, tests/intake.rs'
     `the_scanner_profile_keeps_a_white_page_on_a_white_lid_whole`;
     state.rs' sample with the Scanner profile. README's `scan` and
     "Create PDF from images" rows.
   - **The memory gate** (step 5). gate_7k.rs:
     `scan_of_500_black_and_white_pages` (ignored, release),
     `LetterScans` (a `Pages` making each seeded page as it's asked for)
     and `letter_scan(seed)` (2550x3300, off-white paper with noise, 36
     lines of words, up to 1° of skew, no two pages or batches alike);
     each profile's row inside `with_mem`: a Session, a `SessionLanding`,
     `intake` without OCR by `scan::BATCHING`, `finish`, the Rewrite to
     `target/gate-7k/`; after it, outside `with_mem`, the report's 500
     pages and lopdf's read of 500 pages in the file. Rows: pages,
     pages/s (cleaned, landed and written), peak RSS, RSS added (< 400
     MB, asserted for both on every run), MB written; n/a off Linux.
     `scan::BATCHING` public. bench/README.md's harness paragraph.
   - **F14** (step 6). sheaf-cli tests/scan.rs (new):
     `scan_ocr_finds_the_words_in_one_block_a_page_and_one_font`. lopdf
     writes three Letter pages (36 pt Helvetica headings, "Invoice 4471
     Paid", "Receipt Lumber Yard" and "Mileage Log March", over 8–11
     lines of ordinary text at 14–18 pt); `export --format png --dpi 300`
     makes three PNGs; `scan -q --ocr --profile scanner` scans them,
     orientation on. Checked: 3 pages; every heading word on its page in
     `export --format text`; exactly one `Marker::Ocr` stream in each
     page's /Contents; one font object across the pages' resources, its
     /BaseFont GlyphLessFont. It runs in `cargo test --workspace` on all
     three runners (sheaf-cli's default features build Tesseract, and CI
     fetches the OCR data), and without the data the scan fails, never
     skips. F14 moved to the backlog's Closed.
   - **Wrap-up** (step 7). retired.rs: `RETIRED_M5` (`scan::clean_one`,
     `io::Pixels::Bilevel`) beside M10.5's `RETIRED`, both in `LEDGERS`
     with the words a hit names its slice by ("retired in M5 slice 1"),
     each file parsed once for both (`scan_file`); "Bilevel" in
     `QUALIFIED_ONLY`; the module doc. docs/document-model.md: `Job(… |
     Scan)` and what the scan job may do, `GroupKind::Scan` in the code
     block and the Groups paragraph, the history overview, the scan
     bullet's G4. This plan's text: proposals 1 and 3 and findings 1 and
     6, as built.
   - **Deleted,** confirmed dead by `cargo check --workspace
     --all-targets` and clippy, the names in retired.rs' `RETIRED_M5`:
     `scan()`'s collect-then-insert path (its `Vec<PageImage>` and
     `Vec<Vec<OcrWord>>` held to the end, the one `made_pages` of every
     page, and the second commit, `commit_as(Job(Ocr), …, "Recognize
     Text")`); `scan::clean_one`; `Pixels::Bilevel`. `rg` finds neither
     name outside retired.rs, and "Recognize Text" in sheaf-ops only in
     prefs.rs' doc of the OCR dialog's settings.

   Deviations from the plan (the planner's list first, as resolved):
   - **Multi-page TIFFs:** proposal 1's "a TIFF's pages one by one"
     wasn't so: today's scan read each input with `io::load`, the first
     image alone (Found). `io::frames` gives a TIFF's first page as
     `load(path)`'s image, so every TIFF `scan` read before (and so every
     single-page one) makes the same page, byte for byte, whatever its
     kind (CMYK8, CMYK16, float RGB, planar, an Orientation tag that
     isn't one value, up to image's 512 MiB); its later pages, which the
     old path lost, are decoded one at a time from the file through
     `tiff_pages`' own decode (`tiff_image`). Those equal `load` of the
     same image alone exactly for RGB, gray, G4, 16-bit gray and every
     Orientation; CMYK8 within 1 (`tiff_pages`' integer conversion,
     which must stay byte for byte, and image's float one differ by −1
     on 807 of the 65,536 (c, k) pairs, computed exhaustively); CMYK16,
     float RGB and planar later pages are an error, never a wrong image
     (Found). Step 1 had read the first page through `tiff_image` too,
     which refused those kinds and pages past tiff's own 256 MiB limit,
     and differed by 1 on CMYK8: the review's findings 5, 6, 7, 8, 10
     and 15 (Fixes after review).
   - **`tiff_pages` and the Orientation tag:** it does ignore it (Found);
     its output is unchanged here, and frames match `load`, as the test
     pins.
   - **`intake::RawPage { image: RgbImage, name }`,** not proposal 9's
     `RawPage { image: PageBytes, dpi, sheet, side }`, which belongs to
     sheaf-scanner (slice 6) and doesn't exist yet; the name is for
     messages ("x.png: blank, left out"). Slice 6 maps a device's pages
     into it and may add dpi, sheet and side then.
   - **The 2-second rule:** `Pages::next` is a blocking pull, so a timer
     can only be read between pages: a batch lands once it holds `pages`
     pages, or once `wait` has passed since the last landing (the
     intake's start, for the first), checked as each page is done. A slow
     device's page lands right after it's cleaned. The rule is the pure
     `Batching::due(pages, since_last)`, unit-tested.
   - **Determinism: the command line batches by count only.** The saves
     aren't byte-identical across batchings: a Rewrite numbers what an
     insert brought by its source key (`ObjKey::Base { blob, .. }`, the
     blob a content hash; materialize/number.rs, "the rest by key"), so
     the same pages inserted in other batches are written in another
     object order. By design, not a bug. So `scan::BATCHING` is
     `Batching { pages: 8, wait: None }`, and the same images make the
     same file (OCR's block ids aside, drawn at random, M10.5 §4);
     `Batching::DEFAULT` (8 pages or 2 s) is the app's and the devices'
     (slices 2, 6 and 7), unused but by its unit test until then. The
     batch test compares PDFium renders (0 px), page text, character
     boxes and OCR layers, and asserts the same batching twice writes the
     same bytes (without OCR). Proposal 1's text says so now.
   - **One commit a batch:** today's scan set its text in a second
     commit by `Job(Ocr)` ("Recognize Text"), two undo steps. A batch's
     `InsertPages` and `SetOcrText` are one commit of `Job(Scan)`, which
     `actor_allows` lets make those two ops alone. `BlobSel::Id` resolves
     for job actors (only plugins are refused, resolve.rs).
   - **The text and its pages in one transaction:** the insert applied
     first, its pages taken from `OpResult.made` (`Made::Page`) and their
     geoms from `tx.state()`, the `SetOcrText`s then applied on the same
     Tx, one commit. A page OCR finds no words on still gets an empty
     layer, as before.
   - **G4 at construction, for every reader** (scan, images-to-pdf,
     `PageSource::Images`, black and white PNG and TIFF reads, stamp
     images): the bytes are the same function's, so nothing written
     changes (step 1's cmp of the baseline, step 3's comparison).
     `encode::TiffPage::Bilevel` stays, so "Bilevel" is in
     `QUALIFIED_ONLY`.
   - **Intake takes a built engine** (`Option<&mut dyn OcrEngine>`;
     `scan()` builds it from `OcrConfig` as before): `cargo test -p
     sheaf-ops` has no OCR engine, while `cargo test --workspace` (CI)
     unifies sheaf-cli's features in, so sheaf-ops' tests there have
     Tesseract's OSD and the orientation model. The sheaf-ops tests use a
     fake engine with orientation off; the real OCR check is sheaf-cli's
     (F14).
   - **history_tests.rs** matches `group.kind` exhaustively, so
     `GroupKind::Scan` has an arm there, `unreachable!` ("the runs made
     here are OCR's and plugins'"). The generator stays OCR-or-plugin:
     adding Scan would change the property's random stream, and the
     existing cases, the 4,096-case release property
     (`prims_rebuild_every_version_4096`) and the saved seed would no
     longer test what they did. Scan groups are covered by group_tests.rs
     and tests/intake.rs.
   - **The memory gate has no env var:** the plan names none, and it's a
     peak RSS, which needs no quiet machine, so it's asserted on every
     run, as history's budget is. It's in gate_7k (release, ignored)
     because 1,000 Letter cleanups take half an hour, and it's asserted
     under both profiles: the plan's 4.2 GB is today's `sheaf scan --mode
     bw`, the Photo profile (the command's default), and the Scanner
     profile is the devices'. It lands pages as the command line does
     (`scan::BATCHING`, public for it).
   - **retired.rs' M5 table:** the ledger is M10.5's, `(&str, u8)` rows
     whose hits print "retired in slice N", so M5's rows would read as
     M10.5's slices. They're a second table, `RETIRED_M5`, printed
     "retired in M5 slice N", scanned by the same test; the module doc
     says so.
   - **`State::scan` by profile, with a dialog row:** the dialog had no
     profile, so the scanner choices would have been dead state until
     slice 7. A minimal "Pages from: Photos | A scanner or copier" row
     swaps in each profile's remembered options. Created saves the
     profile it created with and its options; edits to the other profile
     while the dialog is open last only while it's open. The labels "As
     photographed" (`Paper::Original`) and the "Photo" preview view stay
     under the scanner profile (For Greg).
   - **Blank pages and duplex:** files have no duplex backs, so neither
     profile leaves out blank pages for files; proposal 3's "on with
     duplex" is slice 6's device settings' (the plan's table says so
     now).
   - **state.json:** an older flat `scan` object parses as `ScanChoices`'
     defaults, its keys ignored (pre-alpha and disposable: no shim). A
     partial `scanner` object fills its missing fields from
     `ScanOptions::default()`, the Photo profile's, so a hand-edited
     `"scanner": {"dpi": 200}` would get crop and flatten on; sheaf
     always writes the whole object.
   - **Crash safety on the command line:** the Session is in memory, so a
     cancelled `sheaf scan` of files writes nothing, as before; the
     images are still on disk. A device's pages are another matter
     (slice 6, Left). The intake itself keeps the pages so far on any
     stop but its landing's own (Fixes after review), which the app's
     landing (slice 2) and the devices (slices 6 and 7) build on.
   - **A cancel cuts the page in flight short:** cleanup and OCR check
     the cancel, so the page a cancel comes during is left out and the
     pages before it land. For files that loses nothing (the image is on
     disk); for a device, whose page in flight is already scanned,
     proposal 10's "stops after the page in flight" may mean finishing
     it first (slices 6 and 7, Left).
   - **`made_pages`** moved to intake.rs as pub(crate), and no public API
     nothing calls was added (the app's landing, slice 2, adds what it
     needs); `scan::BATCHING` is public for the gate, the only addition
     of the kind.
   - **The intake tests' cleanup:** crop, flatten and deskew off as well
     as orientation (`Paper::Original` at 100 dpi). In the dev profile a
     cleanup with them takes about 1.8 s an 850x1100 page, without them
     about 50 ms, and the batch test cleans 270 pages; the tests are
     about landing, and the flatbed test covers deskew.
   - **The two-undo-steps test** starts from one blank page of the user's
     (a `Cli` insert) and turns it between batches 2 and 3, so the
     history reads "Blank", "Scan", "Rotate", "Scan (continued)", and
     `revert_group` takes back every scanned page and leaves the turn
     (turning a scanned page and reverting would take the page itself).
     The group comes from the timeline (`VersionView.group`), as
     History's "Undo all of" finds it: no public API added for it.
   - **Progress counts pages:** `Files::new` opens each file and counts a
     TIFF's pages from its headers (`io::count_frames`, no image
     decoded), so a file that can't be opened or whose headers can't be
     read is an error before any page is read; the intake sets its count
     after each page, in `PAGES`. Step 3 counted files, so a multi-page
     TIFF's bar ran to the end on its first page (review finding 11).
   - **`SessionLanding` has no Drop:** an intake that fails without
     `finish()` leaves its group open until the session goes, and
     `scan()` drops the session on an error, so nothing sees it today.
     Slice 2's landing should end its group on failure (Left).
   - **The CLI's other options** (`--mode`, `--page`, `--dpi`,
     `--threshold`) became `Option`s, their defaults in their help
     ("(default: color)", as `--engine` already was), so the profile
     decides whatever isn't given; the two profiles agree on them today,
     so nothing written changes. A repeated flag (`--crop --crop`) is
     refused, as any repeated flag already was; `--crop --no-crop
     --crop` is accepted, the last winning.
   - **The flatbed test's light assertion:** step 4's fault check (crop
     back on in the Scanner profile) can't fail the flatbed test, since
     crop does nothing on that scan (finding 6, Measured). It fails the
     profile's unit test, which asserts `!crop`, and the flatbed test
     also asserts the light is left as scanned (paper within 3 of 247,
     lid within 3 of 252, the paper darker), which fails with flatten
     on (254, 254) and with deskew off (3° left).
   - **F14's font fault** was made in emit/mod.rs' `Sink::resource`
     (each call's first object keyed anew), the function that writes a
     drawn resource once a file; content/font's `Glyphless::resource`
     would have made a font a `Tf`, a word, not a page. (c) was shown to
     fail under the first fault with (b)'s loop skipped for one run, as
     the test stops at its first failing assert. (d) also checks the
     font is GlyphLessFont.
   - **Docs in their steps:** steps 3 and 4 changed
     docs/document-model.md's scan bullet and README's rows in the step
     that changed what they described; step 5 bench/README.md's.
   - **The memory gate's output for `qpdf --check`:** the gate removes
     its file, so the wrap-up's gate run carried one temporary line
     copying each row's output aside before the removal (after the
     measurement and outside `with_mem`); gate_7k.rs was restored from
     its copy and checked with cmp, and the release binary rebuilt.
   - **The extra checks the steps added:** the scan job making any op
     (step 2's second fault check); the gate's two sanity reads (500
     pages reported, 500 in the file).

   Found while building:
   - **A multi-page TIFF lost every page but its first** in `sheaf scan`
     and Create from Scans (`io::load` decodes one image), silently:
     fixed here, through `io::frames`.
   - **`tiff_pages` ignores the TIFF Orientation tag,** so images-to-pdf,
     Combine and `PageSource::Images` show an oriented TIFF unturned,
     while `load`, `decoded` and now `frames` turn it. Not changed here.
     Backlog B14.
   - **The Combine dialog's background `inspect`** (combine_dialog.rs:119)
     decodes every image just to count its pages, and now G4-encodes the
     black and white ones too; it could count a TIFF's IFDs instead.
     Backlog B17.
   - **Some TIFF kinds `load` reads are refused by `tiff_image`**
     (CMYK16, 32-bit float RGB and RGBA, planar), in images-to-pdf,
     Combine and `PageSource::Images` already, and in a scan for a
     TIFF's later pages, which the old scan never read (Deviations).
     `tiff_image`'s `GrayA` arms are never reached: tiff 0.11 reports a
     two-sample gray image as `Multiband`, which neither it nor `load`
     reads (the review's finding 5 counted GrayA16 among what `load`
     reads; it isn't). Backlog B15.
   - **`sheaf-cli`'s `progress::run` hangs after a panic in its work:**
     the panic unwinds before `done.store(true)`, so `thread::scope`
     waits forever for the drawing thread, at 0% CPU (the review's
     finding 7 found it through a debug build's overflow, fixed here);
     `done` could be set by a drop guard. Backlog B16.
   - **`images_to_pdf` reads every image before it writes** (`read` →
     `Vec<PageImage>`): G4 at construction shrinks its black and white
     pages, but gray and RGB pages (a PNG's pixels) are still held raw.
     Backlog B18.
   - **The photo profile's cleanup is slow:** about 3.2 s a 300 dpi
     Letter page in release, on one thread, 2.3 s of it finding the page
     and evening the light (`find_page`, `flatten_light`); the scanner
     profile about 0.8 s. 500 pages take about 26 minutes under Photo and
     7.5 under Scanner, so a fast feeder (slices 6 and 7) would outpace
     Photo's cleanup. Slice 6's scan-speed gate takes it up.
   - **A Rewrite's bytes follow how a document's pages were split across
     inserts** (the same pages in another object order; Deviations): by
     design, noted for whoever compares saves.

   What review found and fixed: the plan was reviewed adversarially
   before the steps started, and its findings are the deviations above,
   as resolved (the planner's list). Each step checked its new checks by
   switching their behavior off (Checked by tests). The wrap-up read the
   slice's whole diff again against the plan and the rules for every
   slice, and found nothing to fix in the code; the plan's text was
   behind it in proposals 1 and 3 and findings 1 and 6, now as built.

   Fixes after review (the slice's code review, 16 findings; each checked
   against the code first):
   - **An intake that stops keeps its pages** (finding 1): a cancel, a
     page that can't be read, cleaned or recognized threw away the batch
     not yet landed (up to 7 pages). The loop now runs in a closure, and
     whatever it returns, a batch it left holding pages lands after it
     (a landing that failed took its batch, so it's never landed twice),
     reporting to no progress so a cancel doesn't stop it; the read's
     error comes back first. Tests:
     `a_page_that_can_t_be_read_keeps_the_pages_before_it`,
     `a_cancel_keeps_the_pages_so_far`. `scan()` still writes nothing on
     an error (its Session is dropped).
   - **A batch's text by its pages** (findings 2 and 14, the same): the
     landing takes the `Made::Page` ids before pairing them with the
     batch's word lists, and a count that differs is an error, not a
     `debug_assert`. No insert today makes anything but pages, so no test
     can reach the error.
   - **Word boxes against the old path** (finding 3): checked, equal
     (Measured).
   - **F14's body words** (finding 4): nine in ten of each page's body
     words (distinct lowercased alphanumeric tokens: 62, 78 and 39) must be
     found on it, besides every heading word as before (case kept).
   - **A TIFF's first page is `load`'s** (findings 5, 6, 7, 8, 10 and
     15, one regression seen six ways): `Frames` gives `load(path)` for
     a TIFF's first page and keeps its decoder at the first image for
     the later ones, so `scan` reads every TIFF it read before, to the
     same page. Later pages: the Orientation tag read as image reads it
     (a tag that isn't one value from 1 to 8 ignored, not an error); the
     decoder within image's 512 MiB (`TIFF_LIMIT`, `tiff_decoder`, also
     `tiff_pages`'); `tiff_image`'s `w * h` in `usize`, not `u32` (a
     70000x70000 header panicked a debug build); its ResolutionUnit read
     leniently as well (a malformed one an error before, the same class
     as finding 6; images-to-pdf only reads more files for it). Tests:
     `a_tiff_s_first_page_is_load_s_image` (11 kinds, among them CMYK8,
     CMYK16, float RGB, planar RGB and the tag as two values and as a
     signed one: each `load`'s exactly, and one page counted),
     `a_tiff_s_later_pages_read_as_load_reads_them` (each kind second
     after a gray page: exactly `load` of it alone, CMYK8 within 1,
     CMYK16, float and planar an error), `a_tiff_page_past_256_mib_is_read`
     (a white 9500x9500 RGB second page, 271 MB, PackBits),
     `a_tiff_page_too_big_is_refused` (a 70000x70000 header, through
     `frames` and through images-to-pdf's read). The review's files,
     with the fixed debug binary: its planar TIFF scans to the old
     binary's bytes, its two-valued Orientation scans to the old
     output's bytes, and its 70000x70000 header fails as the old one
     did ("Memory limit exceeded"). The four baseline runs: a and d
     byte for byte the wrap-up's, b and c equal once masked.
   - **Thumbnails aren't pages** (finding 9): `next_page` skips images
     marked reduced-resolution (NewSubfileType bit 0) after the first,
     in `frames`, `count_frames` and images-to-pdf's `tiff_pages`
     (images-to-pdf, Combine and `PageSource::Images` lose a TIFF's
     thumbnail pages too; the first image is always a page, as `load`
     reads it). Test: `a_tiff_s_thumbnail_is_not_a_page`.
   - **Progress counts pages** (finding 11): Deviations. Test:
     `progress_counts_a_tiff_s_pages` (a two-page TIFF and a PNG: the
     count 3; done 0, 1, 2 and fraction 0, ⅓, ⅔ as each page is asked
     for; 3 of 3 and 1.0 at the end).
   - **The photo profile judged** (finding 12): the flatbed test asserts
     what finding 6 recorded (no quad, the whole scan fitted, under 0.3°
     left, 28 lines, paper and lid both at least 250), in place of the
     print.
   - **The clock in the intake** (finding 13):
     `a_batch_lands_once_its_wait_has_passed` (a wait of 0.3 s, room for
     100 pages, small pages taking 0.9 s to come before pages 1 and 5:
     landings of 1, 4 and 3 pages).
   - **Leftovers** (finding 16): group_tests.rs' module doc rewrapped; the
     TIFF test's repeated `crop = false` gone.
   - Rejected: none outright. Finding 5's GrayA16 isn't read by `load`
     either (Found); its other kinds are.

   Measured (AMD EPYC 7302P, 32 threads, release builds unless marked):
   - **The gates** (`gate_7k`, release, `--ignored --test-threads=1` in
     one run, started once the 1-minute load was under 4 on two checks 30
     s apart: load 1.8 at its start and under 3.5 through the timed
     gates, its first eight minutes; 11 of 11 asserted gates pass, and
     both memory rows):

     | Row | Synthetic | Heavy | Scanned |
     |---|---|---|---|
     | geometry walk (asserted < 100 and < 250 ms); ready (asserted < 1 s on the heavy) | 48.1 ms; 54.6 ms | 77.3 ms; 352 ms | 60.5 ms; 471 ms |
     | realize + open + render 1,024 px, an edited page (asserted < 15 and < 25 ms) | 11.9 ms (slice 19: 11.8) | 21.3 ms (21.2) | 33.0 ms (31.7) |
     | thumbnail strip, crop + watermark + header, 320 px (asserted ≥ 310 pages/s) | 425 pages/s | 348 pages/s | 163 pages/s |
     | recognized text on every page: census (asserted 1 font, one block a page); the Append | 1, 7,000; 10.11 s | 1, 7,000; 10.16 s | 1, 7,000; 9.98 s |
     | one-mark Append on the scanned (asserted < 50 ms) | | | 25.6 ms (slice 19: 26.4) |
     | the Rewrite against slice 8's writer, the gates' own part (fails past 1.3×, flagged past 1.2×) | 151 ms, 1.13× | 780 ms, 1.10× | |

     The rest as slice 19 left them: history's 200 OCR versions of 1,000
     pages 236 MB (≤ 256 MB, asserted); snapshot 0.24–0.28 µs and
     `Tx::check` of a delete of every page 0.417 ms; engines after 220
     groups 28.9–29.0 MB weighed, ≤ 6 open; crop all pages 11.5–13.4 ms;
     Bates apply 0.62–0.63 ms, one font; AddMark 0.021 ms and a full
     reorder 1.01 ms at 20,000 marks; ready on 20,000 sheaf marks 559 ms;
     "Page n of N" moved after a save, the Append 469–675 ms (≤ 1 s),
     sections 1.36–3.33 MB (≤ 8 MB). Proposal 8's accepted misses
     (`SHEAF_GATE_PROPOSAL_8`) where slice 19 left them: synthetic 21.6,
     20.3, 18.9 and 28.6 ms, heavy 37.8, 35.2, 32.1 and 42.1 ms, the
     Appends on the file opened again 1,068 and 1,448 ms (the scanned's
     889 ms, asserted). The flatten export's RSS (`SHEAF_GATE_FLATTEN`):
     the heavy's peak 513 MB (slice 19: 514), the synthetic's 88.5 MB,
     the scanned's 1,077 MB. This slice's paths don't reach them. The
     Rewrite against slice 8's writer in the processes of
     `rewrites_on_the_synthetic_and_heavy_7k`: synthetic 155 ms (1.16×),
     heavy 737 ms (1.04×), under the 1.2× flag; this slice doesn't touch
     the writer, so no five-run median was taken (slice 19's medians:
     1.09× and 1.09×).
   - **The memory gate** (`scan_of_500_black_and_white_pages`, in the
     run above; from its tenth minute the machine was busy again, load
     24–45, which moves the pages/s rows only):

     | Profile | Cleaned, landed and written | Peak RSS | RSS added (asserted < 400 MB) | Written |
     |---|---|---|---|---|
     | Photo (the command's default) | 0.301 pages/s | 262 MB | 210 MB | 10.4 MB |
     | Scanner | 0.970 pages/s | 245 MB | 189 MB | 10.4 MB |

     Step 5's run (a process of its own, the machine shared): Photo 0.316
     pages/s, 251 MB peak, 246 MB added; Scanner 1.12 pages/s, 218 MB,
     185 MB; 2,032 s for the test (this run's 12 tests: 3,140 s). Where it
     goes (step 5's one-page probe, deleted): one Photo page alone peaks
     160–179 MB above its start in 3.15–3.19 s (finding the page and
     evening the light 2.31–2.34 s, straightening 0.41 s, fitting and
     black and white, Sauvola and despeckling, 0.44 s; G4 9 ms for about
     21.4 KB), one Scanner page 130 MB in 0.81 s. So a row's peak is one
     page's cleanup (Sauvola's two u64 integral images, the light's
     copies) plus 55–85 MB over 500 pages (the store's batch PDFs, the
     Rewrite, the allocator's slack), and nothing grows with the count.
     The fault check (every cleaned image held to the end) adds 4,206 and
     4,214 MB: the plan's 4.2 GB. Both 500-page files pass `qpdf
     --check`, 500 pages each.
   - **The same pages as before** (step 1's baseline, step 3's and the
     wrap-up's runs; debug sheaf-cli builds, the old one at `3e1b9f0`).
     Inputs: 300 dpi PNGs of pages 1–2 of three scan-kind corpus files
     (scans/safedocs/0000932, 1-bit at 300 dpi; 0002156, gray at 300 dpi;
     0000577, color at 150 dpi), page 1 of forms/irs/f1040.pdf, and two
     two-page TIFFs of scans/safedocs/0002786 (black and white, gray).
     Four runs over the nine files: defaults; `--mode bw --ocr`; `--mode
     gray --no-crop --no-flatten --ocr --page original`; `--mode bw
     --drop-blanks`. The old binary twice: the runs without OCR
     byte-identical, those with OCR identical once the random block ids
     (`/B (b…)`, `/SheafOcrBlock (b…)`, the `/Sh…` names drawn from them)
     are masked and the files compared as QDF without their xref. G4 at
     construction (step 1): the black and white runs byte-identical to
     the old. The intake (step 3): each run has 11 pages, 9 before, since
     each TIFF's second page is now there; the 9 pages that line up (each
     TIFF's first against the old run's one) render identically at 150
     dpi in all four runs (36 of 36 pages equal byte for byte), and
     `export --format text` is the old text (16 bytes in the runs
     without OCR, 20,314 and 24,854 in those with); the two new pages
     are the TIFFs' second pages. The wrap-up's final binary: its four
     outputs equal step 3's (the runs without OCR byte for byte, those
     with OCR once masked). `qpdf --check`: no errors on all four. The
     text layers' word boxes (review finding 3), by `pdftotext -bbox`
     (poppler) on the two runs with OCR, the 9 pages that line up: every
     word and its box (to the 6 decimals it prints) equal, 3,320 words in
     the black and white run and 4,096 in the gray one; the review's
     fixed binary's outputs equal the wrap-up's (a and d byte for byte, b
     and c masked).
   - **Batching** (`batches_make_the_pages_one_batch_makes`, ten pages):
     batches of 1, 3 and 8 make the pages one batch makes (0 px at 50
     dpi, the same PDFium text and character boxes, the same OCR layers)
     in every mode, with OCR and without, but not the same bytes:
     batches of 1 differed from one batch in every mode (another object
     order), and 3 and 8 matched it in Color by chance. The same batching
     twice: the same bytes (without OCR).
   - **Finding 6** (the flatbed test's scan, orientation off): the
     Scanner profile kept the whole scan (no quad, 2550x3279 fitted),
     took out the 3° (under 0.3° left), kept 28 of 28 lines where they
     were drawn and the light as scanned (paper 246, lid 251). The Photo
     profile found no page either (quad None) and made the same
     2550x3279, took out 3.00° (0.000° left), kept 28 of 28 lines (ink
     over columns 386–2136 and rows 464–2848, as the Scanner's), and
     evened the light out: paper and lid both 254. Two throwaway scans
     (a 12 px dark strip along the glass's top and left edges; a page
     filling the scan with a dark band across its top) gave no page
     either. So the photo profile doesn't crop a white page on a white
     lid wrongly here: both tones are on the bright side of Otsu's level,
     the bright pass's outline is the whole scan, and `find_page` takes
     that as the photo being the page already. What separates the
     profiles on a scanner's pages is the light.
   - **The corpus** (`../pdf-corpus`, `SHEAF_CORPUS` unset; no "skipped:
     no corpus" line), every count as #26 left it: materialize_corpus's
     identity 4,389 of 4,392 written, 2,269,688 primary objects
     surviving, 919,465 copied as they were, 432 changed by `keep_only`,
     0 failed; 2,901 forms without their first page, 0 failed; every page
     duplicated, 4,389, 0 failed; inserted twice into the 1040, 1,098, 0
     failed; four times, 0 differ; the logo, 0 of 2,037,600 bytes differ.
     append_corpus: 4,389 marked, 3,770 appended, 259 rewritten (211
     RemovalCheck, 34 BaseObjectsMissing, 14 BaseUnsound), 360 asked, 0
     failed, 0 derived vetoes and 211 on the files' own objects; the
     oracle finds text gone from 2,899 of 2,901 (none from 0016064 and
     0112654). encrypted_corpus: the Rewrite keeps 573 of 573 and the
     Append 572 (0630899 declined). source_corpus: 4,392 files, 24,251
     pages compared, 0 differ; forms_corpus passes. The ignored tiers
     (the export tier, deleted means deleted, realize_corpus, the scans'
     OCR) are the pre-merge run's (Greg, 2026-10-20); this slice adds and
     changes none.
   - **Suites:** `cargo test --workspace` 1,682 passed, 0 failed and 57
     ignored after the review's fixes (1,674 before them; slice 19: 1,654
     and 56 at its verify, plus its wrap-up's three print numbering
     tests; this slice's 25, and the memory gate ignored), no "skipped:
     no corpus"; F14's test about 14 s in the dev profile. The review's
     fixes left fmt, clippy (`-D warnings`, all targets), the non-Linux
     clippy simulation and Windows clippy for sheaf-image and sheaf-ops
     clean; they don't touch the timed gates' paths, and the memory
     gate's intake loop holds what it held (the same batch, now landed
     on an error too), so the gates weren't run again. The interop
     tier (qpdf; pdf.js 6.4.299 on node 24.22): 19 of 19 with
     `--include-ignored`. `qpdf --check` clean on the baseline's four
     outputs, on F14's (made by hand with the test's commands; pdf.js
     reads its 255 words, 88, 103 and 64 a page) and on both 500-page
     files. No proptest seed added. fmt and clippy (`-D
     warnings`, all targets) clean, and clippy with sheaf-app's
     Linux-only gates switched off (`nonlinux-clippy.sh`); clippy (`-D
     warnings`) on `x86_64-pc-windows-msvc` passes for sheaf-image and
     sheaf-doc with their tests, for sheaf-ops with all targets, and for
     sheaf-cli with all targets without its OCR engines
     (`--no-default-features`: oar-ocr's tokenizers build oniguruma with
     a C compiler); sheaf-app can't be checked for Windows here, as
     before.

   Checked by tests:
   - **G4 and frames:** sheaf-image's
     `a_black_and_white_page_is_held_as_g4`,
     `a_tiff_gives_its_pages_one_at_a_time` (two images: sizes, order,
     pixels), `a_tiff_s_first_page_is_load_s_image`,
     `a_tiff_s_later_pages_read_as_load_reads_them`,
     `a_tiff_s_thumbnail_is_not_a_page`,
     `a_tiff_page_past_256_mib_is_read`, `a_tiff_page_too_big_is_refused`
     (Fixes after review),
     `any_other_image_is_one_frame_as_load_reads_it` (a PNG),
     `tiff_round_trips_through_the_reader`, `png_gray_and_bilevel`.
   - **The scan job and its group:**
     `caps_cover_every_op_target_and_actor`,
     `a_scan_group_takes_the_scan_job_s_batches`.
   - **The intake:** tests/intake.rs'
     `batches_make_the_pages_one_batch_makes` (per mode, with OCR and
     without, batchings of 1, 3 and 8 against one of every page: each
     page's render at 50 dpi at 0 px, PDFium's page text and character
     boxes, the state's OCR layers; the same batching twice the same
     bytes), `an_intake_is_one_undo_step` (20 pages in
     batches of 3: one version labelled "Scan" after the empty one, by
     `Job(Scan)`; one undo leaves no pages and nothing to undo),
     `an_edit_between_batches_makes_two_undo_steps`,
     `undoing_an_intake_stops_it` (`GroupEnded` after exactly 9 pages
     read, no pages left), `a_multi_page_tiff_scans_page_by_page` (a
     two-page TIFF and a PNG through `scan()`: three pages, MediaBoxes
     612x792, 792x612, 576x720);
     `a_page_that_can_t_be_read_keeps_the_pages_before_it`,
     `a_cancel_keeps_the_pages_so_far`,
     `a_batch_lands_once_its_wait_has_passed`,
     `progress_counts_a_tiff_s_pages` (Fixes after review);
     `a_batch_is_due_at_its_count_or_after_its_wait`; scan_blanks.rs
     unchanged.
   - **The profiles:**
     `the_scanner_profile_neither_crops_nor_evens_the_light`,
     `each_profile_keeps_its_own_choices` (set and get, a JSON round
     trip, `{}` both profiles' defaults),
     `the_profile_decides_what_no_flag_does`,
     `of_a_flag_and_its_no_the_last_wins`,
     `the_scanner_profile_keeps_a_white_page_on_a_white_lid_whole` (a
     1400x1800 lid of about 252 holding a 1275x1650 page of about 247
     with 28 lines of text bars, turned 3°; the Scanner profile with
     orientation off: no quad, the whole scan fitted, 2550x3279, skew
     under 0.3° on a 1000 px copy, all 28 ink rows within 3·scale px of
     where they were drawn and the ink's columns spanning theirs, the
     light as scanned; the Photo profile no quad, the scan fitted, under
     0.3° left, 28 lines, the light evened to at least 250).
   - **Memory:** gate_7k's `scan_of_500_black_and_white_pages` (Measured).
   - **F14:** `scan_ocr_finds_the_words_in_one_block_a_page_and_one_font`
     (every heading word, nine in ten of each body's).
   - **Fault checks,** each file restored from the step's own copy and
     checked with cmp, and no proptest seed added (only
     materialize/append_rewrite_tests.txt is there):
     - frames ignoring the Orientation tag fails
       `a_tiff_frame_is_what_load_reads` at its equality; `from_image`
       holding the raw pixels (`Pixels::Gray`) fails
       `a_black_and_white_page_is_held_as_g4` and
       `tiff_round_trips_through_the_reader`;
     - the Scan group taking any job's commits fails
       `a_scan_group_takes_the_scan_job_s_batches` ("panics in debug
       only"); the scan job making any op fails
       `caps_cover_every_op_target_and_actor` ("Job(Scan) making
       move-pages") and the group test at its `Refused`;
     - committing without the group fails `an_intake_is_one_undo_step` (8
       versions, not 2); reading on past `GroupEnded` fails
       `undoing_an_intake_stops_it` (20 pages read, not 9); a TIFF's first
       image alone fails `a_multi_page_tiff_scans_page_by_page` (2 pages,
       not 3); dropping the last batch's `SetOcrText` fails the batch test
       ("every page has a layer"); `due` ignoring `wait` fails its unit
       test ("one page, the wait passed");
     - crop on in the Scanner profile fails its unit test (not the
       flatbed test: Deviations); flatten on fails the flatbed test
       ("paper and lid (254, 254), scanned 247 and 252"); deskew off
       fails it ("3° of skew left");
     - the intake holding every cleaned image to its end makes the
       memory gate fail: the Photo profile added 4,206 MB and the Scanner
       4,214 MB ("the photo profile added 4206 MB to the RSS"), which is
       the plan's 4.2 GB;
     - dropping the `SetOcrText` ops in `SessionLanding::land` fails F14's
       (b) ('page 1 has no "Invoice"') and, with (b) skipped, (c) ("page 1
       draws 0 OCR blocks"); a fresh glyphless font a page fails (d) ("the
       pages draw in {(22, 0), (23, 0), (24, 0)}");
     - "Bilevel" taken out of `QUALIFIED_ONLY`, with a `clean_one()` call
       put in scan.rs, fails `nothing_retired_is_back` with five hits,
       each "(retired in M5 slice 1)": the four `TiffPage::Bilevel` uses
       and the call.
     - The review's fixes, each switched off alone: a TIFF's first page
       through `tiff_frame` in place of `load` fails
       `a_tiff_s_first_page_is_load_s_image`; the Orientation tag read
       strictly again fails the later-pages test ("tag has invalid
       type"); `TIFF_LIMIT` at tiff's 256 MiB fails
       `a_tiff_page_past_256_mib_is_read`; `(w * h) as usize` panics
       `a_tiff_page_too_big_is_refused` ("attempt to multiply with
       overflow"); thumbnails kept fails `a_tiff_s_thumbnail_is_not_a_page`
       (3 pages, not 2); the rest not landed on an error fails both
       keep-the-pages tests (3 pages, not 4); the rest landed with the
       cancelled progress fails the cancel test (3, not 4); `Files`
       counting files fails `progress_counts_a_tiff_s_pages` (`Some(2)`,
       not 3), and the intake not setting its count fails it at its
       per-page check; the clock not reset after a landing fails the wait
       test (`[1, 1, 1, 1, 1, 1, 1, 1]`), and the wait never read fails
       it (`[8]`); the Photo profile with the Scanner's options fails the
       flatbed test's photo half; each page's text cut to its first 12
       words fails F14's body check.

   For Greg:
   - **Hand checks** (no tests reach these):
     - Create from Scans' "Pages from" row, by Tab and Left/Right and by
       clicking its chips: a switch loads the other profile's steps and
       refreshes the preview; edits to one profile are still there after
       switching away and back; Create remembers the profile and its
       options, and the dialog opens on that profile next time; the "As
       photographed" page size and the "Photo" preview label still read
       well under the scanner profile.
     - A real copier's or scanner's multi-page TIFF (scan-to-folder),
       with `sheaf scan --profile scanner` and with the dialog's "A
       scanner or copier": every page there, in order, upright and
       straight, the whole image kept and its light as scanned; and once
       with `--mode bw`.
   - **The photo profile's speed** (Found): about 3.2 s a 300 dpi Letter
     page, against 0.8 s for the scanner profile; worth knowing before
     slices 6 and 7 put a feeder behind it.
   - **Ask 12** (the scanners Greg can check by hand) is still open, for
     slice 6.

   Left for later slices:
   - **Slice 2:** the app's `Landing` (a document's planner: `plan_made`,
     `Base::At`, so the user can keep working), untitled documents, and
     B11 (Create from Scans still runs `scan()` to a file and opens it).
     The app's landing should end its group when an intake fails
     (`SessionLanding` has no Drop), and may need intake on a thread of
     its own (`Pages::next` blocks).
   - **Slice 6:** sheaf-scanner's `RawPage` (bytes, dpi, sheet, side)
     mapped into `intake::RawPage` (which may gain dpi, sheet and side);
     the Scanner profile's blank rule with duplex ("on with duplex"),
     part of the device's settings; a `sheaf scan --device` that stops on
     a hang, a crash or Ctrl-C must keep the pages already scanned (the
     command line's Session is in memory today, and a scan of files
     loses nothing, its images being on disk); the 2 s rule with a
     blocking device (`Batching::DEFAULT`, read as each page is done);
     the photo profile's speed against a feeder (Found).
   - **Slice 7:** pages streaming into the open document (Insert from
     Scanner), through slice 2's landing, with `Batching::DEFAULT`.
   - **Slices 6 and 7:** whether a cancel finishes a device's page in
     flight (already scanned) before it stops, as proposal 10's "stops
     after the page in flight" may mean; the intake cuts it short today
     (Deviations).
   - **For the backlog** (Found): `tiff_pages` and the Orientation tag;
     Combine's `inspect` decoding every image to count pages (it could
     use `io::count_frames` now); `images_to_pdf` holding every image;
     the TIFF kinds `tiff_image` refuses (Deviations) and its unreachable
     `GrayA` arms; `progress::run` hanging after a panic.

2. Done. Documents with no file yet (proposal 2, ask 1; B11): Create
   from Scans no longer writes a file and opens it again. The dialog hands
   its images and choices to the window (`ScanEvent::Scan`), which shows a
   new untitled document ("Untitled scan 1", "Untitled scan 2": the first
   number no view in any sheaf claims and no sidecar has) in the current window
   when it's empty, else in a new one, and the pages stream in as they're
   cleaned: batches of `Batching::DEFAULT` in one `GroupKind::Scan` group,
   one undo step, each planned on the intake's thread on a version the
   view hands it and committed on the UI thread with `Base::At` and
   `Stale::Relower`. Save on an untitled document is Save As, suggesting
   "<name>.pdf" in the images' folder, and afterwards the window takes the
   file's name and path. Closing asks only when it's dirty, which means it
   has pages. Its crash-recovery sidecar is a third kind of format 2,
   `untitled` (its Save As folder and the whole document), in
   `state_dir()/recovery/`, claimed by name while its view lives, its
   write started as each batch lands (or as the write in flight ends) and
   removed when nothing is unsaved; the process that owns sheaf's state
   offers each one no live view holds at the next launch. The dialog's
   ask-for-a-path step, its own scan run and the save-and-`open_paths`
   hand-off are gone. Every asserted gate holds, the corpus counts are
   the baseline's, and B11 is in the backlog's Closed.
   - **Groundwork** (step 1). sheaf-doc recovery.rs: the `untitled` kind
     (`sheaf-recovery 2\nuntitled\n` and the whole document), written by
     `write_untitled(input, sidecar, p) -> Result<usize>` (its target
     `same_file: false` with no stamp, through `durable::write`, the
     folder made first) through a private `whole(input, target, p)`
     (`forced_rewrite` plus `materialize`) that Full's arm of `write`
     shares; `untitled_path(dir, name)` (`<dir>/<name>.sheaf-recovery`);
     `pub struct Untitled { sidecar, name, bytes }`; `find_untitled(dir)`
     (sorted by name; a `*.sheaf-recovery` not of the kind removed;
     unreadable files, `.sheaf-tmp` temps and other files left; no folder,
     none); `restore_untitled(store, rec)` (`BlobKind::recovered(None)`)
     and `discard_untitled(sidecar)`; `parse` beside a file still refuses
     the kind (a comment on its arm); the module doc. model.rs:
     `Document::never_saved()` (`saved = None`), and the `saved` field's
     doc corrected (a memory-made document opens saved with no record;
     None is `never_saved`, or a memory document after pruning took its
     saved version). sheaf-ops: `Session::recovered(&Untitled, p)` (a new
     store, `restore_untitled`, `open::finish`, `open::start` under the
     sidecar's name, `ready` keeping its ViewFacts, then `never_saved`);
     intake.rs' `batch_source(store, pages, fit) -> Result<Src>` and
     `apply_batch(tx, src, words, engine, p)` (the insert at the end, then
     each page's `SetOcrText`, with the count check) over a pub(crate)
     `insert_at_end(src, name)`, which `SessionLanding::land` and images.rs
     use; `made_pages` deleted; scan.rs' `pub fn engine(opts, ocr)`, which
     `scan()` uses. Tests: recovery.rs' `an_untitled_sidecar_round_trips`,
     `find_untitled_removes_what_it_can_t_read`,
     `find_beside_a_file_takes_no_untitled_sidecar`; tests/intake.rs'
     `an_intake_s_untitled_sidecar_comes_back_as_a_session`.
     docs/document-model.md's recovery entry, Images to PDF, the scan
     entry and Sessions.
   - **The view's file behind accessors** (step 2, no change in
     behavior). docview.rs: the field private (it was `pub(crate)`), read
     outside the open, recovery and save code through `file_name()`,
     `path() -> Option<&Path>`, `folder()` and `stem()` (`stem_of`), and
     `title()` through `file_name()`; their readers in export.rs, jobs.rs,
     comments_panel.rs, attachments_panel.rs and workspace.rs' `shows`
     switched. properties_dialog.rs' `PropertiesContext.path` an `Option`,
     with a `name` for File when there's no path (Location "Not saved
     yet"). sheaf-plugin's `DocContext.path` an `Option` (WIT's
     `document.path()` was already `option<string>`; its doc and
     docs/plugins.md now say so). Test: host.rs'
     `plugin_sees_the_call_context` with a document of no file
     (`path=None`).
   - **Create from Scans streams into an untitled document** (step 3,
     B11). untitled.rs (new): `Untitled { name, dir, sidecar }`,
     `next(kind, cx)` over a pure `first_free`, and `for_scan(files, cx)`
     (the first image's folder for Save As, the sidecar at
     `untitled_path(dirs::recovery_dir(), name)`). sheaf-ops dirs.rs:
     `recovery_dir()`. docview.rs: `pub(crate) enum Place { File,
     Untitled }` in place of the field (`unopened(path, ..)` keeps its
     signature over a private `placed`); the accessors by place;
     `open`, `recover` and `offer_recovery` for a file only, and
     `recent::add` from `install` and `adopt` for a file only; `save()`
     returning `Task<bool>`, `save_to` for a file and `save_as()` (now
     resolving to whether it saved) for an untitled document;
     `finish_save` making the place the file; `adopt`/`adopting` taking a
     `Place`; `DocView::untitled(u, who, scale, timing, launched, cx)`,
     which reads no global; a `scan: Option<ScanRun>` field, with
     `cancel_scan` in Escape's chain after `cancel_ocr` and
     `scan_progress` for the status bar. scan_run.rs (new):
     `DocView::scan_untitled` (the workspace's and the tests' way in) and
     `start_scan(open, opts, ocr, batching, cx)`: `begin_group(Scan)`, the
     intake through `drive` (`open()`, `scan::engine`, `intake` with a
     `ViewLanding`), each batch's source made by `batch_source`, planned
     on the intake's thread on the version and the scan job's lowering
     the view hands it (`Request::Base`: `tx_at`, `pin`, `apply_batch`),
     and committed by the view (`Request::Commit`, `land_batch`) with
     `Base::At(on)`, `Stale::Relower` and the group; the group ended and
     `scan` cleared whatever ends `drive`; the notices "Scanned N pages
     (k left out as blank) — Ctrl+Z undoes", "Every page came out blank",
     "Scan stopped: its pages were undone" and `jobs::ended("Scan")`, and
     "Scanning page k of N — Esc stops" while it runs. scan_dialog.rs:
     its save-path step, `run()`, `running`, `status`, `error`, the bar,
     the Stop states and their imports deleted; the button "Create".
     workspace.rs: the handler's `show_untitled_scan(origin, files, opts,
     cx)` (shaped like `show_made`) through `Workspace::scan_untitled`,
     its `open_paths` gone. The comment summary of an untitled document
     is titled with its name. Tests (headless, Linux only):
     `create_from_scans_streams_into_an_untitled_document`,
     `closing_an_untitled_document_asks_only_with_pages`,
     `undoing_a_scan_in_the_view_stops_it`; untitled.rs'
     `untitled_names_take_the_first_free_number` (every platform);
     stale_tests.rs'
     `a_relowered_insert_at_the_end_lands_beside_an_insert_a_delete_and_a_move`.
     README's "Create PDF from images" row, docs/document-model.md's
     Create from Scans paragraph, scan.rs' module doc.
   - **Untitled recovery** (step 4). docview.rs: a
     `sidecar_unwanted: Arc<AtomicBool>` shared with the writes;
     `write_recovery` (now pub(crate)) through
     `write_untitled_recovery(sidecar, cx)` for an untitled place (nothing
     once unwanted; discarded and `sidecar_of` cleared when clean;
     rescheduled through autosave while a save or a write runs; skipped
     when `sidecar_of` is the current version; else written in the
     background with the `sidecar_writing` guard, the write removing what
     it wrote when the flag is set by then, and the completion keeping it
     only while the place is still that sidecar, the flag clear and the
     document dirty, removing it and calling `write_recovery` again
     otherwise); `discard_recovery`'s untitled arm setting the flag first;
     `show_session` (`adopting`'s shared part); `recovered_untitled(rec,
     ..)` (place `Untitled { name, dir: PathBuf::new(), sidecar }`,
     `Session::recovered` in the background, "Recovered unsaved pages —
     Ctrl+S to save them", `State::Failed("… (its recovery data is
     kept)")` on failure); `placed`'s `cx.on_release` calling `released()`,
     which discards a ready, clean untitled document's sidecar;
     `test_support`'s `sidecar_writing`, `sidecar_of`,
     `sidecar_unwanted`. scan_run.rs: `land_batch` calls `write_recovery`
     after `on_change`. untitled.rs: `next` skips a name a sidecar in
     `recovery_dir()` has (`kept`). workspace.rs:
     `offer_untitled_recoveries(first, cx)` (nothing unless
     `instance::owns_state()`; `find_untitled` in the background, then
     "Recover the unsaved scan \"<name>\"?", Recover or Discard, one at a
     time in `first`), `show_recovered` (an empty window, else a new one,
     through `Workspace::recovered_untitled`); main.rs calls it after the
     launch windows open, but not in a `--script` run. Tests:
     `a_batch_s_sidecar_is_written_as_it_lands`,
     `a_scan_killed_after_its_first_batch_is_offered_at_the_next_launch`,
     `an_untitled_document_s_sidecar_follows_it`; the create test and the
     names test extended. docs/document-model.md's untitled recovery,
     README's Crash recovery paragraph.
   - **Wrap-up** (step 5). gate_7k.rs: after each profile's row of
     `scan_of_500_black_and_white_pages`, outside `with_mem`, the untitled
     sidecar of the document scanned written once (timed, then removed)
     and its size, and what sidecars written as each batch lands would
     write in all (`sidecars_in_all`: the sum over batches of the
     document's size so far, its pages' share of the whole sidecar), in
     the command line's batches of 8 and in the app's at the row's pace
     (`app_batch`: as many pages as come within `Batching::DEFAULT`'s 2 s,
     up to 8); then the same for `COLOR_SAMPLE` (8) pages in color under
     the scanner profile (JPEG at the intake's quality of 80), scaled to
     500 pages; no gate on either; `scan_into`, the landing both use;
     the module doc and bench/README.md's harness paragraph. The backlog:
     B11 to the bottom of Closed, F14's Closed line with `4481573`. Docs:
     README's Configuration (`recovery/`), docs/document-model.md's dirty
     tracking (`never_saved`) and the app's saves (`Place`, Save As, the
     window taking the name), dirs.rs' doc (`recovery/` isn't safe to
     delete), this plan's finding 2, slice 2's Builds and the Risks'
     untitled documents, as built.
   - **Deleted,** confirmed dead by `cargo check --workspace
     --all-targets` and clippy, the names in retired.rs' `RETIRED_M5`
     with comments: `intake::made_pages` (`batch_source` and
     `apply_batch`); `scan_dialog::ScanEvent::Created` (`ScanEvent::Scan`),
     "Created" in `QUALIFIED_ONLY` for Combine's `CombineEvent::Created`;
     the dialog's `file_dialog::save` step and its `run()` of
     `sheaf_ops::scan::scan` with `running`, `status`, `error`, the bar
     and the Stop states (`scan::scan` stays, the command line's); the
     workspace's `open_paths` of the scan's file; the `DocView.path`
     field (`place`). `rg` finds `made_pages` and `ScanEvent::Created`
     only in retired.rs, no `scan::scan`, `file_dialog::save`, `Tracker`
     or `error` field in scan_dialog.rs, and no `path:` field in
     `DocView`.

   Deviations from the plan (the planner's list first, as resolved):
   - **The sidecar is a kind of its own:** the plan said "a Full
     sidecar", but Full's head names a base file (its length, stamp and
     flags), and an untitled document has none. `untitled` keeps format 2
     and writes the body as Full's is; `find` beside a file never takes
     one (it's removed as malformed there, Found). Proposal 2 and slice
     2's Builds say so now. Added beyond the step list:
     `recovery::untitled_path`, so the naming `find_untitled` parses back
     is in one place, and `write_untitled` making its folder, so
     `recovery/` needn't exist before the first batch.
   - **`never_saved`:** model.rs said `saved` is None for a document made
     in memory, but `Document::loading` sets it to the first version and
     `detach` to None when pruning takes a version with no record. A
     restored untitled document needs it None, so `never_saved()`; the
     field's doc is corrected. Revert to saved exists only in the docs
     (`cursor = saved`); every match on `self.saved` reads None as no
     saved side: `saved_at` None, `input()` no saved side (so a Rewrite,
     `NoSaveRecord`), `saved_record`/`file` None, pruning's `_ => {}`,
     `attach` setting `At(v)`. On a file-backed document it would leave
     the saved version's record unused; it's for memory-made documents
     only. A long-edited untitled document reads dirty after pruning,
     which is harmless.
   - **A recovered untitled document opens as a file does** (review): step
     1's `Session::recovered` (every page's facts read, then `ready`,
     before anything showed) is gone, never committed. `recovered_untitled`
     reads the body in the background (`restore_untitled`), then shows it
     through `show_opening` (`install`'s shared part, taking who and the
     scale, since `install` reads `prefs::get` and `state::get`, globals no
     headless test sets): its pages first, `finish_open` after them, and
     `ready` calls `never_saved` for an untitled place, with "Recovered
     unsaved pages — Ctrl+S to save them". While it's still reading,
     `released()` and `write_untitled_recovery` leave its sidecar alone.
     tests/intake.rs' test opens the sidecar the same way at sheaf-doc's
     level (renamed `an_intake_s_untitled_sidecar_comes_back`).
   - **`batch_source` and `apply_batch` in place of `plan_made`:**
     `plan_made` (page_insert.rs) makes the insert alone, while a batch
     needs its `SetOcrText`s in the same transaction. The two are shared
     by `SessionLanding`, images.rs and the app; `plan_made` isn't used.
     `apply_batch` names its insert "scan".
   - **Planned on the intake's thread:** the plan had each batch planned
     in a background task the UI starts. Planning in a second background
     task needs a free executor thread while the intake's blocks on the
     reply, and GPUI starts max(2, cores) threads, so with as many scans
     running as threads none would be free. The landing asks the view for
     the version and the lowering (`Request::Base`), plans there, and
     sends the planned Tx back (`Request::Commit`); the view commits on
     the UI thread as the plan said. It plans with `NoProgress`, so a
     cancel never cuts a batch's landing short (`SessionLanding` passes
     its progress). Proposal 1 says so.
   - **No Conflict re-plan:** the plan kept a bounded re-plan if an end
     insert read the order. It doesn't: the new sheaf-doc test commits an
     end insert planned before a user's insert, delete and move with
     `Relower`, and it lands beside each; an insert that recorded an
     order read makes it a Conflict (its fault check). So no edit of the
     user's conflicts with a batch, and none is planned again.
   - **Checks at the view:** headless GPUI opens no windows and has no
     Workspace test, so "an untitled window" is checked at the view the
     workspace builds, through the same constructor and naming code
     (`untitled::for_scan`, `DocView::scan_untitled`); the window choice,
     the prompts and the real picker are hand checks. The create test
     runs in a process of its own (`alone`, through `test_support`) with
     `SHEAF_CONFIG_DIR` and `SHEAF_SCRIPT_SAVE` in a `sheaf-` temporary
     directory, so `save()` itself runs as Save As, and no test touches
     the user's recent.json or state folder.
   - **"No file anywhere":** read as nothing written beside the images
     and nothing where it's saved until Save As, and (from step 4) only
     its sidecar in the recovery folder, none after.
   - **The sidecar at landing, not by autosave:** autosave writes 3 s
     after the last change, or at once once the changes began 30 s ago, so
     batches every 2 s would keep putting it off. `land_batch` starts the
     write as each batch lands, and `a_batch_s_sidecar_is_written_as_it_lands`
     checks it by state in the same update (`sidecar_writing`, or
     `sidecar_of` the current version), not by a deadline. A batch that
     lands while a write runs (review) sets `sidecar_due`
     (`write_recovery_at_once`), and the write's completion starts the
     next at once, not on autosave's 3 s timer, which each landing put
     off again: a crash loses at most the pages landed since the last
     write finished, which the module doc, `land_batch`'s and README's
     Crash recovery now say (they said none). Only the untitled
     completion does so; a file's sidecar has no landing yet (slice 7,
     Left).
   - **The dialog's run moved to the window:** the dialog ran
     `sheaf_ops::scan::scan` itself after its save step, with `running`,
     `status`, the bar, the Stop states and `error` (whose only setters
     were in `run()`). All of it went; the view's job shows the progress
     and the outcome, and the dialog closes once it emits
     `ScanEvent::Scan`. `ScanRun` keeps the task answering the intake
     (`_answers`): dropped with the view, it leaves the intake's question
     unanswered, which stops it ("the document was closed").
   - **No retired row for `DocView::path`:** the ledger treats `impl
     DocView { fn path() }` as a definition of `DocView::path`, so a row
     for the old field would fail against the new accessor. The field is
     private and gone; the name lives on as the accessor.
   - **The field private, not `pub(crate)`,** so the compiler, not only
     `rg`, keeps other modules off it. `offer_recovery` still reads it
     (its Discard drops the file's sidecar; the offer half of recover).
     The dialogs' suggestions now come from `stem()` (`stem_of`), so a
     file whose stem isn't UTF-8 gets "document - pages.pdf" and the like
     where it got none. `locked_card` takes the name, worked out before
     `render` borrows the state.
   - **The owner-only offer:** `main.rs` reaches `app.run` in Standalone
     processes too (another sheaf running, `--new-instance`), so the
     launch offer runs only when `instance::owns_state()` (`workspace::
     offered(dir, owns)`), leaving the state folder, discards included,
     to the owner. Every process writes its own untitled documents'
     sidecars there, a Standalone one's too, so a crash never loses their
     pages; claims keep them apart (Names are claimed, below), and
     instance.rs' doc says so. The plan's risk
     note was wrong in part: `Mode::Separate` (debug builds, the dev
     flags) owns the state when no other sheaf is running (sheaf-instance
     lib.rs:191 goes standalone only beside a running one), so a debug
     build run alone does offer them.
   - **The offer in the first window,** one sidecar at a time, and not
     over a `--script` run's window, whose keys a prompt would take (those
     wait for the next unscripted launch). A recovered document opens in
     an empty window or a new one, its Save As starting in the images'
     folder, which the sidecar's head keeps (review: a third head line,
     the path as UTF-8 hex, empty when it isn't UTF-8); an untitled
     document with no folder starts Save As and the exports in the home
     folder (`dirs::home`, now pub), not the process's working one.
   - **Orphan sidecars,** beyond the step list: the view's release
     (`cx.on_release`) discards a ready, clean untitled document's
     sidecar, so a scan undone to nothing and closed while a write runs
     leaves none; and a write's completion removes what it wrote when the
     document became clean meanwhile. A dirty view is always asked about
     before it closes, and a recovered view still loading (or failed)
     keeps its sidecar.
   - **Names are claimed** (review; it was "names skip sidecars", by
     open views' names and the sidecars on disk): `recovery::Claim`, a
     lock (`File::try_lock`: flock, LockFileEx, so per open file, in this
     process too) on `recovery/.<name>.sheaf-lock`, taken and let go under
     the folder's `.lock` so a lock file is removed (as its claim is let
     go) only while no one else has it open. `untitled::next(kind)` gives
     the first name it can claim that has no sidecar once claimed (no
     window is read, so none may be being updated), the claim kept in
     `Untitled.claim` (an `Arc`, cloned into each write in flight, so a
     closed view's last write holds the name until it's done) and None,
     with a warning, when the folder can't take claims (no sidecar
     then). `write_untitled` and `discard_untitled` take the claim, not a
     path; `find_untitled` claims what it returns, so a live document's
     sidecar, in this sheaf or another, is never offered, discarded or
     recovered twice, and two processes never share a name. The lock
     files stay only while their names are held (or after a crash, until
     the next `find_untitled`); `.lock` stays.
   - **The model goes by the place's name** (review; it was
     `Session::new_empty`'s "Untitled" as planned, while a recovered one
     went by its sidecar's name): `Session::new_empty_named(name)`, which
     `new_empty` calls with "Untitled", so a header's `<<file>>`
     (`lower_cx`) says "Untitled scan 1" before a crash and after its
     recovery alike.
   - **`DocContext.path` and `PropertiesContext.path` as `Option`s,** a
     public struct of sheaf-plugin the host tests build (seven literals
     in tests/host.rs, none in plugins.rs); `PropertiesContext` gained a
     `name` for File with no path.
   - **Step order:** README's row and Crash recovery paragraph and
     document-model.md's paragraphs changed in the steps that made them
     untrue (3 and 4), not in the wrap-up.
   - **Test helpers:** `ALONE`, `alone` and `shown` moved into
     `#[cfg(all(test, target_os = "linux"))] pub(crate) mod test_support`
     in docview.rs rather than making docview's `tests` pub(crate):
     `nonlinux-clippy.sh` switches off Linux-only tests only inside
     `#[cfg(test)]\nmod tests {`, and a `pub(crate) mod tests` fell
     outside it. scan_run.rs' test module is gated whole.
   - **The kill test's child** calls `DocView::untitled` plus `start_scan`
     with the gated pages (`Gated::new(&files, 8)`), since `scan_untitled`
     takes files only and is exactly those two calls over `Files`; the
     parent's `find_untitled` runs before its headless app, and the
     pages are compared by their image XObjects' bytes in the saved
     files (lopdf), against a `SessionLanding` intake of the same 8
     images.
   - **The gate rows:** the untitled sidecar is measured once a profile,
     outside `with_mem`, and what per-batch sidecars write in all is
     computed (each the whole sidecar's share by pages), not written 63
     or 500 times over. The color figure comes from an 8-page sample
     scaled to 500 pages. Each row's Session now outlives its
     measurement for the sidecar write, so the pages/s row no longer
     counts the Session's drop (microseconds against minutes).

   Found while building:
   - **An untitled sidecar is never encrypted:** a memory-made primary has
     none, so pages inserted from an encrypted PDF into an untitled
     document sit in the clear in the state folder, as a Save As would
     write them.
   - **`find` beside a file removes an untitled sidecar** as malformed, so
     opening a file named `state_dir()/recovery/Untitled scan 1` (no
     extension) would delete that recovery. An extreme edge case.
   - **A file's sidecar can outlive an undo to its saved version** (before
     this slice): in `write_recovery`'s file path, the clean-document
     discard runs before the `sidecar_writing` check, and the completion
     keeps a sidecar while its base is unchanged, so a write taking over
     3 s that lands after the document was undone to its saved version
     (or closed clean) stays, and the next open offers undone edits. The
     untitled path checks dirtiness when a write completes.
   - **`find` beside a file reads the whole sidecar** at open and holds
     it until its prompt is answered (before this slice); `find_untitled`
     now reads heads only (review), so only a file's large Full sidecar
     still costs that.
   - **Scratch folders in /tmp:** sheaf-doc's model.rs tests
     (`sheaf-model-<pid>-<n>`) and rewrite_tests (`sheaf-save-…`) leave
     theirs on every run: 7,041 `sheaf-model-*` and 38,367 `sheaf-*`
     entries at the wrap-up.
   - **`fuzz_bodies_survive_mutated_fixtures` asserts 60 s of wall time,**
     which a loaded machine misses (8 s alone).
   - **A stray doc comment** in workspace.rs (before this slice): `///
     Document > Recognize Text (OCR)…: open the options card…` sits above
     `open_recent`'s own.
   - **Not new:** `contrib.rs`' `add_panel` dead under
     `--no-default-features` is backlog B6.
   - **Tooling, not sheaf:** `nonlinux-clippy.sh` recognizes only
     `#[cfg(test)]\nmod tests {`, so a `pub(crate) mod tests` of
     Linux-only tests would fall outside its simulation (Deviations); and
     sheaf-plugin can't be checked for Windows here (wasmtime's fiber and
     ring build C with `lib.exe`).

   What review found and fixed: the slice plan was reviewed adversarially
   before the steps started, and its findings are the deviations above,
   as resolved (the planner's list). Each step checked its new checks by
   switching their behavior off (Checked by tests). The wrap-up read the
   app's landing, the untitled sidecar's writes and the launch offer again
   against the plan and found nothing to fix in the code; the plan's text
   was behind it in finding 2, slice 2's Builds and the Risks' untitled
   documents, now as built. The slice's review (20 findings) then found,
   all fixed but one rejected in part:
   - **Untitled sidecars were tied to their documents by name alone**
     (findings 1, 6, 7, 15, and 2): a Standalone sheaf wrote into the
     owner's `recovery/` with names only its own windows reserved, so two
     processes could share a sidecar and remove each other's, a
     relaunched owner could offer (and Discard) a live Standalone's, and
     a closed view's write still in flight could write over and remove a
     new document's of the same name; and `next` skipped a window being
     updated without a word. Now names are claimed (Deviations, "Names
     are claimed"): the claim, not the window list, says a name is
     taken, so `next` reads no window at all. Standalone processes keep
     their sidecars (a crash never loses their pages) and the owner alone
     offers and discards. Finding 15's "unique file names" option wasn't
     needed: the lock gives the same isolation and keeps the names.
   - **The crash-safety wording overstated it** (3, 13): a batch landing
     during a write waited for autosave, so the docs' "loses none" was
     wrong. The next write now starts as the last ends, and the docs say
     a crash loses at most the pages since the last finished write.
   - **`<<file>>` said "Untitled" before a crash and "Untitled scan 1"
     after it** (4): both go by the place's name now.
   - **No test ran the launch offer** (5): its choice is `offered(dir,
     owns)`, which the kill test runs on both sides of the kill and for a
     non-owner; the prompt, Recover's window choice and main.rs' call stay
     hand checks (headless GPUI has no windows to prompt in).
   - **Untitled sidecars were read whole at launch and held through every
     prompt** (8, 11, 18), with a second copy: `find_untitled` reads heads
     only, and `restore_untitled` reads the body once Recover is chosen,
     straight into the shared buffer (`sheaf_util::read_shared`, which
     `read_file_and_metadata` now uses too), no second copy.
   - **Each sidecar write held the document twice** (9, 12): the head and
     body go out in turn through `durable::write_parts`, which `write`
     now calls with one part; Full's arm too, its bytes step 1's
     baseline's (cmp).
   - **A recovered document's Save As started in the working folder**
     (10): the head keeps the folder, and a document with none starts in
     the home folder.
   - **A recovered document showed nothing until every page's facts were
     read** (14): it opens as a file does now (Deviations); no gate row
     was added, since it's the staged path the `ready` gate measures.
   - **Save at close mid-scan dropped the pages landing during the save**
     (16): `save_to_close` stops the scan and waits for it to end, then
     saves, and closes only if nothing is unsaved after.
   - **The close flow had no test** (17): rejected in part. The decision
     is `is_dirty()` itself, shared by both paths, so no `must_ask` was
     factored out; the Save arm is tested through `save_to_close` (a
     mid-scan save, and a cancelled Save As, which `SHEAF_SCRIPT_SAVE`
     set empty now scripts); the prompt and its wiring are hand checks,
     and the closing test's doc says so.
   - **A crashed write's temp file stayed forever** (19): `find_untitled`
     removes an unheld name's temp files (and a stale lock file).
   - **The scan tests' helpers were `pub(crate)`** (20): private now.

   Measured (AMD EPYC 7302P, 32 threads, release builds unless marked):
   - **The gates** (`gate_7k`, release, `--ignored --test-threads=1` in
     one run, started once the 1-minute load was under 4 on two checks 30
     s apart: 2.9 at its start and 3.0–3.5 through the heavy's timed gates;
     a spike to 17.8 during the scanned's, whose thumbnail strip isn't
     asserted, and 10.3 then 5.6 during the synthetic's, every asserted row
     passing with room; 12 of 12 tests pass, every asserted gate, both
     memory rows):

     | Row | Synthetic | Heavy | Scanned |
     |---|---|---|---|
     | geometry walk (asserted < 100 and < 250 ms); ready (asserted < 1 s on the heavy) | 49.3 ms; 55.1 ms | 79.3 ms; 368 ms | 62.2 ms; 468 ms |
     | realize + open + render 1,024 px, an edited page (asserted < 15 and < 25 ms) | 11.9 ms (slice 1: 11.9) | 22.1 ms (21.3) | 33.3 ms (33.0) |
     | thumbnail strip, crop + watermark + header, 320 px (asserted ≥ 310 pages/s) | 423 pages/s | 352 pages/s | 141 pages/s (the load's spike) |
     | recognized text on every page: census (asserted 1 font, one block a page); the Append | 1, 7,000; 9.86 s | 1, 7,000; 9.97 s | 1, 7,000; 10.18 s |
     | one-mark Append on the scanned (asserted < 50 ms) | | | 24.4 ms (slice 1: 25.6) |
     | the Rewrite against slice 8's writer, the gates' own part (fails past 1.3×, flagged past 1.2×) | 149 ms, 1.11× | 803 ms, 1.14× | |

     The rest where slice 1 left them: history's 200 OCR versions of 1,000
     pages 236 MB (≤ 256 MB, asserted); snapshot 0.24–0.28 µs and
     `Tx::check` of a delete of every page 0.42 ms; engines after 220
     groups 28.9–29.0 MB weighed, ≤ 6 open; crop all pages 10.4–13.1 ms;
     Bates apply 0.62–0.70 ms, one font; AddMark 0.021 ms and a full
     reorder 0.97 ms at 20,000 marks; ready on 20,000 sheaf marks 519 ms;
     "Page n of N" moved after a save, the Append 471–686 ms (≤ 1 s),
     sections 1.36–3.33 MB (≤ 8 MB). Proposal 8's accepted misses
     (`SHEAF_GATE_PROPOSAL_8`): synthetic 22.7, 20.4, 19.2 and 29.8 ms,
     heavy 36.3, 32.4, 32.4 and 42.5 ms (slice 1: 21.6, 20.3, 18.9, 28.6;
     37.8, 35.2, 32.1, 42.1), the Appends on the file opened again 1,073
     and 1,517 ms (1,068 and 1,448; the scanned's 888 ms, asserted). The
     flatten export's RSS (`SHEAF_GATE_FLATTEN`): the heavy's peak 515 MB
     (slice 1: 513), the synthetic's 88.7 MB, the scanned's 1,076 MB. This
     slice's paths reach none of them, and the differences are the
     machine's. The Rewrite against slice 8's writer in the processes of
     `rewrites_on_the_synthetic_and_heavy_7k`: synthetic 137 ms (1.03×),
     heavy 786 ms (1.11×), under the 1.2× flag; this slice doesn't touch
     the writer, so no five-run median was taken.
   - **The memory gate** (`scan_of_500_black_and_white_pages`, in the run
     above; load 5–13 through it): Photo 0.306 pages/s, 233 MB peak, 197
     MB added (asserted < 400 MB); Scanner 1.12 pages/s, 236 MB peak, 198
     MB added; both 10.4 MB written. The test's 2,963 s run (12 tests).
   - **The untitled sidecars** (the gate's rows; no gate, the plan names
     none):

     | Scan | Untitled sidecar, written once | Sidecars in all, a batch of 8 | The app's in all (its batch at that pace) |
     |---|---|---|---|
     | 500 black and white pages, photo profile (0.306 pages/s) | 10.4 MB in 217 ms | 334 MB | 2,597 MB (1 a batch) |
     | 500 black and white pages, scanner profile (1.12 pages/s) | 10.4 MB in 33.2 ms | 334 MB | 872 MB (3 a batch) |
     | 500 color pages, scanner profile, JPEG 80, from 8 (1.54 pages/s) | 382 MB (the 8 pages' 6.10 MB in 107 ms, scaled) | 12,304 MB | 24,037 MB (4 a batch) |

     Each figure is one write (the two black and white times, of the same
     10.4 MB, vary with the disk). The app writes no sidecar while one is
     being written, so its figures are at most these: a color document's
     whole sidecar near its end takes about 6.7 s at the sample's rate, so
     later batches share writes, but the writes run back to back. In black
     and white a whole sidecar a batch is cheap; in color it writes 12–24
     GB over a 500-page scan. An incremental untitled sidecar is slice 7's
     (Left).
   - **Step 1's baselines** (debug sheaf-cli, the old binary at
     `4481573`): `scan` of a PNG, of a two-page TIFF, `--mode bw`, `--mode
     gray`, and two images-to-pdf runs, all six outputs equal (cmp) old and
     new; the Full sidecar, written by a temporary test with the file's
     mtime pinned, for a memory-made document with no record and for one
     `opened_from` a file with its record unsound, equal old and new (the
     test removed, its file restored and checked with cmp). The wrap-up's
     final debug binary: the same six outputs equal the old's (cmp).
   - **The corpus** (`../pdf-corpus`, `SHEAF_CORPUS` unset; no "skipped:
     no corpus" line), every count the baseline's: materialize_corpus's
     identity 4,389 of 4,392 written, 2,269,688 primary objects surviving,
     919,465 copied as they were, 432 changed by `keep_only`, 0 failed;
     2,901 forms without their first page, 0 failed; every page
     duplicated, 4,389, 0 failed; inserted twice into the 1040, 1,098, 0
     failed; four times, 0 differ; the logo, 0 of 2,037,600 bytes differ.
     append_corpus: 4,389 marked, 3,770 appended, 259 rewritten (211
     RemovalCheck, 34 BaseObjectsMissing, 14 BaseUnsound), 360 asked, 0
     failed, 0 derived vetoes and 211 on the files' own objects; the
     oracle finds text gone from 2,899 of 2,901 (none from 0016064 and
     0112654). encrypted_corpus: the Rewrite keeps 573 of 573 and the
     Append 572 (0630899 declined). source_corpus: 4,392 files, 24,251
     pages compared, 0 differ; forms_corpus passes. The ignored tiers are
     the pre-merge run's (Greg, 2026-10-20); this slice adds and changes
     none, and touches neither the writer nor the realizer (the Full
     sidecar's body is the same function's, step 1's cmp).
   - **Suites:** `cargo test --workspace` 1,694 passed, 0 failed and 57
     ignored (slice 1: 1,682 and 57; this slice's 12 new tests), no
     "skipped: no corpus". fmt and clippy (`-D warnings`, all targets)
     clean, and clippy with sheaf-app's Linux-only gates switched off
     (`nonlinux-clippy.sh`); clippy (`-D warnings`, all targets) on
     `x86_64-pc-windows-msvc` passes for sheaf-doc and sheaf-ops;
     sheaf-plugin can't be built for Windows here, neither clippy nor
     check (wasmtime-internal-fiber and ring need `lib.exe`), and its
     change is a field made an `Option`; sheaf-app can't be checked for
     Windows here, as before. The interop tier (qpdf; pdf.js 6.4.299 on
     node 24.22): 19 of 19 with `--include-ignored`. `qpdf --check`: no
     errors on a Save As output (the create test's `scans.pdf`, 3 pages)
     and on an untitled sidecar's body with its two head lines stripped
     (the same test's sidecar before Save As, 3 pages), both copied aside
     by two temporary lines in the test, which was restored from its copy
     and checked with cmp. No proptest seed added.
   - **After the review's fixes:** `cargo test --workspace` 1,698 passed,
     0 failed, 57 ignored (four new tests; tests/intake.rs' renamed), no
     "skipped: no corpus" (`fuzz_bodies_survive_mutated_fixtures` missed
     its 60 s once at a load of 71, then passed, Found); fmt, clippy
     (`-D warnings`, all targets), the non-Linux simulation, and Windows
     clippy for sheaf-util, sheaf-doc and sheaf-ops clean; materialize,
     append and encrypted corpus counts the baseline's, word for word;
     interop 19 of 19 with `--include-ignored`. The gates weren't run
     again then (the 1-minute load was 34–71 throughout, the other
     checkout's runs): `durable::write_parts` is now under every save's
     write (one `write_all` a part, the same bytes), and the untitled
     rows' sidecar goes to a `recovery/` beside the fixtures under a
     claim.
   - **The final check's gate run** (after the review's fixes; release,
     `--ignored --test-threads=1` in one run, started at a load of 2.8
     after two checks under 4, 30 s apart): 11 of 12 pass. The miss is
     the scanned's reorder Append on the file opened again, 1,071 ms
     (asserted ≤ 1 s; 888 ms before the fixes), which `write_parts` can't
     reach (an Append in place goes on the file through
     `append_in_place`, not `durable::write`); `gates_on_the_scanned_7k`
     alone, three more times at loads under 4: 976, 838 and 889 ms, all
     passing, within slice 17's 0.89–1.07 s spread on the synthetic's
     same row. The rest: geometry walk 47.7 and 75.4 ms (< 100 and <
     250); ready on the heavy 360 ms (< 1 s); realize + open + render
     12.0 and 22.0 ms (< 15 and < 25); thumbnail strip 425 and 349
     pages/s (≥ 310); recognized text census 1 font, 7,000 blocks, the
     heavy's Append 9.95 s; the scanned's one-mark Append 39.2 ms in the
     run, 22.6 ms alone (< 50); history's 200 OCR versions 236 MB (≤ 256);
     the Rewrite against slice 8's writer 1.14× and 1.18× synthetic, 1.10×
     and 1.08× heavy (one run each, under the 1.2× flag; no five-run
     median, the writer's bytes unchanged); the scan memory gate 178 MB
     (photo) and 181 MB (scanner) added (< 400 MB); the untitled
     sidecars' rows 153 ms (photo), 41.8 ms (scanner) and 39.8 ms (the
     color sample), 10.4 MB each for 500 pages.

   Checked by tests:
   - **Untitled sidecars:** recovery.rs' `an_untitled_sidecar_round_trips`
     (the same page sizes and page 2's OCR word, `Memory(Recovered)`,
     dirty with nothing to undo, no record or file, still dirty after
     `forget_before_cursor`, a first save that's a Rewrite and leaves it
     clean), `find_untitled_removes_what_it_can_t_read`,
     `find_beside_a_file_takes_no_untitled_sidecar` (a real one, and an
     untitled head over lines that read as the file's own base);
     tests/intake.rs' `an_intake_s_untitled_sidecar_comes_back_as_a_session`
     (a 10-page gray OCR intake, its sidecar found and recovered: the
     name, 10 pages, dirty with nothing to undo, a Rewrite save, every page
     equal to the intake's save as PDFium reads it: render, text and
     character boxes).
   - **No file yet in the plugin host:** `plugin_sees_the_call_context`
     (`path=None` with its selection and current page).
   - **The end insert and the user's edits:**
     `a_relowered_insert_at_the_end_lands_beside_an_insert_a_delete_and_a_move`.
   - **The app** (headless, Linux only):
     `create_from_scans_streams_into_an_untitled_document` (the title
     "Untitled scan 1 •", `path()` None, Properties with no path and the
     name, 3 pages, the empty version and one "Scan" by `Job(Scan)`, undo
     to 0 pages and clean and redo; nothing beside the images or in
     `out/`, the sidecar alone in the recovery folder; `save()` resolving
     true as Save As, 3 pages in the file, `path()`, `model.file()` and
     the title "scans.pdf" following it, clean, the recovery folder
     empty); `closing_an_untitled_document_asks_only_with_pages`
     (`is_dirty()` false with no pages, true after a scan, false after
     the undo; `file_name()` "Untitled scan 1");
     `undoing_a_scan_in_the_view_stops_it` (20 gated images, exactly 4
     pages, the undone notice); `a_batch_s_sidecar_is_written_as_it_lands`
     (9 images gated after 8: the write started in the read that first
     sees 8 pages, then the sidecar holding 8, and 9 once the gate opens;
     then a write in flight and an edit landed as a batch lands: no look
     finds no write running with the sidecar behind, until it's current);
     `a_scan_killed_after_its_first_batch_is_offered_at_the_next_launch`
     (while the child runs with its 8-page sidecar, the parent can't claim
     "Untitled scan 1" and `offered(dir, true)` is empty; the child
     SIGKILLed, signal 9 asserted; `offered(dir, false)` empty, and
     `offered(dir, true)` exactly ["Untitled scan 1"] with the images'
     folder; `recovered_untitled` read with 8 pages, "Untitled scan 1 •",
     dirty, no path, `folder()` the images'; saved, the sidecar gone; the
     image XObjects' bytes a `SessionLanding` intake's);
     `saving_to_close_mid_scan_keeps_every_page` (in a process of its own:
     20 gated images, `save_to_close` at 3 pages, then the gate opened:
     it resolves true, the scan over short of 20, the saved file holding
     every page the view does, clean); `a_cancelled_save_to_close_keeps_it_open`
     (`SHEAF_SCRIPT_SAVE` empty, which now cancels a save picker: false,
     untitled, dirty, the sidecar kept);
     `an_untitled_document_of_no_folder_saves_from_home` (`folder()` the
     home folder);
     `an_untitled_document_s_sidecar_follows_it` (there after a scan,
     gone after undo, back after redo; a write in flight, undo twice and
     the view dropped: none once the write ends, and its name claimable
     only once the write is done; a write in flight, Don't Save and the
     view dropped: the same; `save_to`: the recovery folder empty but for
     claims, and an edit writes the file's own sidecar); untitled.rs'
     `untitled_names_take_the_first_free_number` (every platform, real
     claims in a temp folder: gaps once a claim is let go, other kinds
     and files take none, a crash's sidecar takes its name, as a claim
     does); recovery.rs' `a_claimed_name_is_no_one_else_s` (a second claim
     refused while one's held, its sidecar and temp file not found then;
     let go, its lock file gone, found with no folder, the dead write's
     temp gone, claimed by the find until it's dropped).
     `an_untitled_sidecar_round_trips` now writes with a claim and a
     folder ("images ü") and reads it back, not found while claimed;
     `find_untitled_removes_what_it_can_t_read` adds a head cut short, a
     folder not hex, a body that isn't a PDF (found by its head, failing
     only at `restore_untitled`), an unheld name's temp and lock file
     (removed) and a temp whose name was cut short (left).
   - **Fault checks,** each file restored from the step's own copy and
     checked with cmp, and no proptest seed added (only
     materialize/append_rewrite_tests.txt is there):
     - `never_saved` a no-op fails `an_untitled_sidecar_round_trips` and
       the session test at their dirty asserts; `parse` taking
       `"full" | UNTITLED` fails `find_beside_a_file_takes_no_untitled_sidecar`
       at its like-a-base case;
     - `path()` giving `Some("")` with no path fails the host test
       (`path=Some("")`);
     - committing without the group fails the create test (a timeline of
       4, not 2); the landing always replying Ok fails the undo test
       ("Scanned 20 pages…", not the undone notice); an untitled save
       written to `<dir>/<name>.pdf` fails the create test's path assert;
       `finish_save` leaving the place untitled fails it (`path()` None);
       `title()` ignoring the place fails it ("? •"); an end insert
       recording an order read makes the stale test a Conflict;
     - no `write_recovery` in `land_batch` (autosave alone) fails only the
       landing test ("the sidecar is written as the batch lands"); no
       `never_saved` in `Session::recovered` fails the kill test at the
       restored title ("Untitled scan 1", not "Untitled scan 1 •");
       `finish_save` not discarding fails the follow test ("saved, it
       keeps none"), the create test and the kill test; no flag store in
       `discard_recovery` fails the follow test ("none once it's
       written"), and so do no flag check in the background write and no
       release hook (its first case);
     - `made_pages` put back (a `pub(crate) fn made_pages() {}` in
       intake.rs) fails `nothing_retired_is_back` with
       "crates/sheaf-ops/src/intake.rs:432: intake::made_pages (retired in
       M5 slice 2)"; a `Created` variant put back in `ScanEvent` fails it
       with "scan_dialog::ScanEvent::Created (retired in M5 slice 2)".

   - **Review's fault checks** (each file restored from the review's own
     copy and checked with cmp; no proptest seed added): a claim that
     ignores a held lock fails `a_claimed_name_is_no_one_else_s`,
     `an_untitled_sidecar_round_trips`, the names test, the kill test
     ("the live child's") and the follow test ("claimed while its write
     runs"); no temp sweep fails the claim test and the find test; an
     empty folder line fails the round trip, the find test and the kill
     test (its folder); no chained write fails the batch test ("the next
     write starts as the last ends"); no `stop_scan` in `save_to_close`
     fails the mid-scan test ("stopped"); `save_to_close` resolving true
     whatever the save did fails the cancelled test ("it stays open");
     `offered` ignoring `owns` fails the kill test; no `never_saved` in
     `ready` fails the kill test (its title); no home fallback fails the
     home test. The Full sidecar, through `write_parts` now, equals step
     1's baseline (the temporary test again, its file restored and
     checked with cmp). Not checked by a fault: `released()` and
     `write_untitled_recovery` leaving a recovered sidecar alone while
     it's still reading (closing in that window is a race no test can
     hold), and `save_to_close`'s dirtiness check after a save (an edit
     can't be put between a scripted picker and the save's end).

   For Greg:
   - **Hand checks** (a release build, or `SHEAF_SINGLE_INSTANCE=1` with
     a debug one, so the process owns sheaf's state; no test reaches
     these):
     - File > Create > From Scans/Photos… from an empty window: the pages
       stream into that window, titled "Untitled scan 1 •"; from a window
       showing a document: a new window. The status line reads "Scanning
       page k of N — Esc stops", with the bar, and ends "Scanned N pages —
       Ctrl+Z undoes".
     - Two scans: "Untitled scan 1" and "Untitled scan 2", each in its own
       window.
     - Ctrl+S on an untitled document opens Save As in the images' folder
       suggesting "Untitled scan 1.pdf"; Cancel keeps it untitled (title,
       no file); a save makes the title the file's name, and the file is
       in Open Recent.
     - Closing the window or quitting asks about an untitled document only
       when it has pages, naming "Untitled scan 1"; its Save goes to Save
       As (mid-scan, it stops the scan first, then asks where; a
       cancelled Save As keeps the window open, and a quit stops); Don't Save closes it and leaves nothing under
       `~/.local/state/sheaf/recovery/`; a scan whose every page came out
       blank closes without asking.
     - Esc mid-scan stops it and keeps the pages so far; Ctrl+Z mid-scan
       undoes it and stops it ("Scan stopped: its pages were undone");
       closing the window mid-scan stops it.
     - Properties on an untitled document: File "Untitled scan 1",
       Location "Not saved yet".
     - Kill -9 sheaf mid-scan, then start it again: "Recover the unsaved
       scan \"Untitled scan 1\"?" in the first window. Recover shows it in
       an empty window or a new one, "Untitled scan 1 •", with "Recovered
       unsaved pages — Ctrl+S to save them", its pages those landed before
       the kill; Ctrl+S opens the picker in the images' folder. Discard
       removes it. With several, the prompts come one at a time; a fully
       undone scan closed leaves none to offer; a second sheaf started
       beside a running one (`--new-instance`) offers nothing, and its
       own scan is "Untitled scan 2" while the first sheaf's "Untitled
       scan 1" is open (before its first batch too); quit the first and
       start sheaf again while the second runs: the second's scan isn't
       offered.
     - An OCR scan with the OCR data missing: the scan stops at once and
       the notice says why (`jobs::ended("Scan", …)`), with the untitled
       window left empty.
   - **The untitled sidecar's cost on a long scan** (Measured): a whole
     document a batch is cheap in black and white and large in color;
     slice 7 is where an incremental untitled sidecar would go (Left).

   Left for later slices:
   - **Slice 4:** File > Create > Separator Sheets… makes a new untitled
     document: `untitled::next(kind)` takes the kind ("Untitled
     separators", say) and gives the name's claim, and a constructor like
     `for_scan` gives it a folder (none: the sheets come from no files,
     so Save As starts in the home folder);
     `DocView::untitled` and the recovery code take any `Untitled`. The
     launch offer's text says "scan" today ("Recover the unsaved scan …"),
     which a second kind should change.
   - **Slice 7:** Insert from Scanner streams into the open document:
     `start_scan(open, opts, ocr, batching, cx)` lands at the end only
     (`apply_batch`'s `insert_at_end`); a `before` page means an insert
     that reads the order, so the user's delete or move of that page meets
     a `Relower` commit, and the Conflict re-plan this slice didn't need
     comes back. `land_batch` already calls `write_recovery_at_once`, so a
     file's sidecar is written as each batch lands too, the Append section
     of every page scanned so far; but only the untitled write's
     completion starts the next write at once (`sidecar_due`), so a
     file's completion should do the same once its batches land. And the untitled sidecar's cost: each
     write is the whole document, so over a long color scan the writes
     add up to many gigabytes (Measured); an incremental untitled sidecar
     (the batches since the last one, as a file's Append section is) is
     the fix if the figures stand.
   - **Slices 6 and 7:** a device's scan into an untitled document is
     recoverable after its first batch, through this slice's sidecar; a
     device's page in flight at a crash is lost, as slice 1 left it.

3. Done. Reading codes (proposals 14 and 15; asks 7–9): sheaf-image has
   a new pixels-only module, `sheaf_image::codes`, which reads what can
   make a page a separator sheet and makes the QR code sheaf's own sheet
   will carry. Barcodes are read through rxing 0.9.3 (Apache-2.0,
   without its default features, `pdf417` added for ask 9) behind
   sheaf's own object-safe `Reader` trait, so zxing-cpp can come in
   beside it (ask 7): a page is binarized once and decoded for each
   symbology asked for alone, with `TryHarder` on, in a fixed order (QR,
   Data Matrix, PDF417, Code 39, Code 128), until a code the caller's
   text match takes, so sheaf's sheet comes first, a page reads the same
   way every time, and a code of the page's content never hides a
   separator's code of another symbology. Kodak patch
   codes (types 1, 2, 3, 4, 6 and T) are read by sheaf's own reader,
   bars along each edge read from the edge inward by their widths.
   `codes::qr(text)` gives a QR code's modules (level M, UTF-8 with its
   ECI, no quiet zone, the same modules for the same text), and
   `codes::read` reads a page's patch code and its barcode together.
   The bench's pages, its code drawing and its 17 degradations are
   `codes::fixture`, behind sheaf-image's new `fixtures` feature, for
   this crate's tests and slices 4 and 5's in sheaf-ops. Every symbology
   reads back in every cell of its matrix (480 of 480), every patch
   type in every layout scanned every way, blank, text and rules pages
   (strokes, and a real Helvetica page drawn by PDFium in CI) read as
   nothing, and the corpus's 709 scans (5,697 pages) hold no sheaf code
   and no patch. The speed gate's four rows take 13–14 ms of the 60
   allowed, every other asserted gate holds, and the corpus counts are
   the baseline's. Nothing is deleted, no public name is retired, and
   sheaf-doc, sheaf-edit, sheaf-app and the CLI don't change.
   - **rxing behind `Reader`; QR modules; codes drawn** (step 1).
     sheaf-image Cargo.toml: `rxing = { version = "0.9.3",
     default-features = false, features = ["decoders", "encoders",
     "encoding_rs", "oned", "qrcode", "datamatrix", "pdf417"] }`, and a
     `fixtures` feature (commented as sheaf-doc's); the description and
     lib.rs' bullet and boundary (a barcode library, pixels only) say
     barcodes. codes.rs (new): the module doc (what's read, and why
     `TryHarder`, limited formats and a fixed order, with the bench's
     figures); `Symbology { Qr, DataMatrix, Code39, Code128, Pdf417 }`
     (Debug, Clone, Copy, PartialEq, Eq, Hash) with `ALL`; `Code {
     symbology, text }`; `trait Reader: Send + Sync { fn read(&self,
     page: &GrayImage, symbologies: &[Symbology]) -> Option<Code> }`;
     `Rxing`, which reads one `BinaryBitmap` (a `HybridBinarizer` over a
     `Luma8Source`) with a `decode_with_hints` call for each of QR, Data
     Matrix and PDF417 asked for, then one for the 1D symbologies asked
     for together, each with `TryHarder` and only those formats, a result
     of a format not asked for counting as none and any rxing error as
     None (nothing asked, nothing read); `Modules { width, height }` with
     `dark(x, y)`; `qr(text) -> Result<Modules>` (rxing's
     `qrcode_encoder::encode_with_hints`, level M, `CharacterSet` UTF-8).
     fixture.rs (new): `LETTER_300`, `white_page`, `encode(symbology,
     text) -> Modules` (QR through `qr`, the rest through
     `MultiFormatWriter` with no margin, cropped to their dark modules;
     a 1D code one row, a PDF417 code unturned in its 4-module rows) and
     `paste(img, modules, x, y, module_px_x, module_px_y)`. Tests: 8
     (Checked by tests).
   - **Pages, degradations and the symbology matrix** (step 2).
     fixture.rs: `blank_page()`; `text_page(seed, clear)`, the bench's
     300 dpi Letter page in glyph-like strokes (22 letter shapes at an
     x-height of 22 px, a header over a rule, a 5 × 9 ruled table of
     digit shapes, a footer; whatever would cross `clear` left out);
     `rules_page()` (a border, W-2-style boxes, a table, lines to write
     on); `code_page(symbology, text, module_px, row_px)` (the code on
     text page 42 with its quiet zone cleared); `enum Degradation {
     Skew, Turn, Noise, SaltPepper, Blur, LowContrast, Jpeg, Threshold,
     Dpi }` and `degrade(img, from_dpi, steps) -> (GrayImage, u32)`, all
     seeded (a private xorshift64*); `BENCH`, the bench's 17 variants
     with its parameters; `VARIATIONS` (as is, turned 90°, 180° and 270°,
     skewed ±3°, noise, blur) and `scanned_every_way(page)`, 48 scans: each
     variation made at 300 dpi, resampled to 300, 200 and 150, in gray and
     thresholded at 128. Tests: five matrix tests, the negatives and
     `every_degradation_changes_the_page`.
   - **The patch-code reader** (step 3). codes/patch.rs (new, its names
     re-exported from codes): the module doc (A-61599's geometry, the
     patterns from the edge inward, how it reads, the bench's figures);
     `PatchCode { One, Two, Three, Four, Six, T }` with `ALL` and
     `bars()`; `Edge`; `Patch { code, edge, inset, length }` (inches);
     `patches(page, dpi) -> Vec<Patch>` (the page box-reduced to about
     75 dpi and split at its Otsu level; lines 0.04 in apart across a
     band 4.7 in deep or half the page along each edge; a line reads a
     patch where light, four bars and three spaces of a patch's widths
     and light follow from the edge in; lines of one type within 3 lines
     of each other and 0.08 in of the last one's inset make a stretch, a
     patch at 0.6 in and 8 lines); `patch(page, dpi)`, the type read
     along the most length. fixture.rs: `PatchLayout { KodakLabel,
     KodakSheet, AllianceT }`, `patch_page(code, layout)`,
     `with_patch_bars(page, code, layout)`, `bordered_page()`,
     `edge_rule_page()`, `SHEET`, `SAMPLES` (the matrix's codes, moved
     from the tests) and `Scan { label, dpi, image }`. Tests: 8.
   - **A page's codes, real text, the speed gate** (step 4). codes.rs:
     `PageCodes { patch, code }` and `read(page, dpi, symbologies,
     reader)`, the patch bars then the decode, both every time. sheaf-ops
     Cargo.toml: sheaf-image with `fixtures` as a dev-dependency. sheaf-ops
     tests/codes.rs (new): `real_text_page()` (a lopdf Letter page:
     Helvetica-Bold header over a rule, 36 lines of Helvetica at 11 pt,
     a 5 × 9 table of amounts with 0.75 pt rules, an 8 pt footer, its
     content written as bytes), `drawn(engine, page, dpi)` (content
     alone through `render_with`, then `to_gray()`), and
     `a_page_of_real_text_reads_as_nothing`. gate_7k.rs:
     `codes_on_a_page_with_none` with `CODES` (`SHEAF_GATE_CODES`),
     `CODES_GATE` (60 ms), `CODES_RUNS` (21), `read_timed` and
     `content_at_150(kind)`, and the module doc's M5 slice 3 paragraph.
     sheaf-image tests: `a_sheet_with_patch_bars_reads_both`,
     `a_page_with_nothing_reads_nothing`.
   - **The corpus tier** (step 5). tests/codes.rs:
     `scans_hold_no_separators` (ignored; `SHEAF_CODES_ONLY` through
     `common::narrowed`), `scan_codes(corpus, entry)` on `each_entry`'s
     threads (PDFium's page count checked against the manifest's, each
     page drawn at 150 dpi and read for patch bars, for QR alone and for
     every symbology), `SCANS` (709, 5,697), and the literals `SHEET`
     (`SHEAFSEP`) and `PATCH_TEXT` (`PATCH`); `drawn` returns a Result,
     so a file that doesn't open or draw is a failure listed with its
     page. Run in full in the slice (Measured).
   - **Wrap-up** (step 6). README's License paragraph names rxing. This
     plan, as built: Research's rxing features (`pdf417`), proposal 15's
     one decode, slice 3's Builds and Checks, and the Risks' codes on
     poor scans and barcodes in content. No docs/ page changes: nothing
     a user reaches reads codes yet (separator docs are slice 10's).
   - **Review fixes** (after step 6; What review found). codes.rs:
     `Symbology`'s variants and `ALL` in the order a page is decoded
     (QR, Data Matrix, PDF417, Code 39, Code 128); `Reader::read(page,
     symbologies, accept)` and `codes::read(page, dpi, symbologies,
     accept, reader)`, `Rxing` decoding each symbology alone and going
     on past a code `accept` turns down (`TWO_D`, `linear()` and `asked`
     gone, `of` in their place); `Modules`' sides private behind
     `width()` and `height()`, and `dark` asserting its column and row;
     the docs say binarized once, not decoded once. patch.rs: `QUIET`
     0.18 in (was 0.08), its own `otsu_level` in u64 (imageproc's counts
     in u32), the module doc's bar length as A-61599 gives it. fixture.rs:
     the getters, and the generator's doc says why it isn't fastrand's.
     sheaf-ops: `drawn` moved to tests/common/mod.rs, which gate_7k now
     includes (`content_at_150` calls it); the corpus tier reads every
     symbology twice, for a separator past other codes and for the
     baseline. Tests: 7 new, and `codes_asked_for_read_in_a_fixed_order`
     covers every neighbor in the order.
   - **Deleted:** nothing; slice 3 has no Deletes list and renames no
     public name, so retired.rs gets no rows (`git diff`: no sheaf-doc,
     sheaf-edit, sheaf-app or sheaf-cli file changed, and the only lines
     removed from sheaf-image and sheaf-ops are a description, a doc
     sentence and two `use` lists, each rewritten with more).

   Deviations from the plan (the planner's list first, as resolved):
   - **rxing's features add `pdf417`:** the plan listed `decoders,
     encoders, encoding_rs, oned, qrcode, datamatrix`, but ask 9 makes
     PDF417 a rule's symbology and it's a feature of its own in 0.9.3.
     `multi_barcode_readers` stays off (one decode for the first code).
     Research says so now.
   - **"One decode a page" is one bitmap decoded for each symbology
     alone, in a fixed order, past codes the caller turns down:** rxing's
     `MultiFormatReader` walks the 2D formats of its `PossibleFormats` in
     a `HashSet`'s order, random for each reader
     (multi_format_reader.rs:180), so with two 2D symbologies asked a
     page with both read as either one, and sheaf's QR didn't come first.
     And any call stops at its first code, while proposal 15 counts a
     code only if its text matches: with QR always first, a vendor's QR
     code (8 of the corpus's scan pages carry one) would hide a rule's
     Code 128 every time, and in one 1D call a form's Code 39 id (6
     pages) would hide a rule's Code 128 on the rows met first. So
     `Rxing::read` calls `decode_with_hints` for each symbology asked
     alone, in `Symbology::ALL`'s order (QR, Data Matrix, PDF417, Code
     39, Code 128), on one `BinaryBitmap`, whose binarization is cached
     (binary_bitmap.rs:39), and `Reader::read` takes `accept`, the
     caller's match: a code it turns down lets the next symbology's
     decode run (review findings 1 and 4; the step 1 to 6 version did
     the 1D symbologies in one call and returned the first code of any
     pass). rxing gives one code a decode, so a second code of the same
     symbology is still hidden by the first (Left). Proposal 15 says so
     now. rxing's own `Reader` trait is imported as `use rxing::Reader
     as _`.
   - **`Modules` is W × H,** not square: the fixture draws 1D and PDF417
     codes through it too (QR's are square).
   - **The matrix's noise and blur, defined:** the bench's σ 25 noise at
     300 dpi failed Code 39, Code 128 and PDF417 in every reader, so the
     matrix's noise is a scanner's, σ 10 (seed 11; the bench's office mix
     used 8), and its blur σ 1.5 px, both made at 300 dpi before the
     resample and the threshold. All 17 bench variants are kept in
     `fixture::BENCH`, as proposal 22 says.
   - **The codes' sizes, fixed before the first run** (the check gives
     none): QR at sheaf's 0.75 mm (9 px at 300 dpi), Data Matrix at the
     bench's 0.68 mm (8 px), Code 39 and Code 128 with 20 mil narrow bars
     (6 px) 150 px tall, PDF417 with 4 px modules in the encoder's own
     4-module rows (16 px). The Data Matrix and PDF417 texts were chosen
     here ("DOC=W-2;CLIENT=1042;N=0001", "Form W-2 2025|Copy B|Acme
     Corp|12-3456789"); Code 128's is SurePrep's "Schedule A".
   - **The bench's degradations aren't test-only:** slices 4 and 5 use them
     from sheaf-ops' tests, so they're `codes::fixture` behind the new
     `fixtures` feature (`cfg(any(test, feature = "fixtures"))`, as
     sheaf-doc's and sheaf-edit's). Workspace clippy unifies sheaf-ops'
     dev-dependency feature into the library, so clippy also runs on
     sheaf-image alone with no features.
   - **The fixture's text is strokes, real glyphs come from PDFium:** the
     bench drew DejaVu through ab_glyph and imageproc's `text` feature;
     sheaf-image has no font (imageproc without `text`), and CI's macOS
     and Windows runners have no DejaVu. The real-text checks are
     sheaf-ops': the lopdf-built Helvetica page in CI (PDFium carries the
     standard 14), the synthetic and heavy 7k's first pages in the gate,
     and the corpus tier. Strokes are drawn in gray 15 (the bench's text
     was 20), with a stroke header, footer and digit shapes in place of
     its fixed strings.
   - **The patch sheets are drawn, not kept:** no PDFs go in the repo, so
     the layouts are drawn from their measurements: Kodak's label
     (A-61599: one edge, bars 0.20 and 0.08 in, 0.80 in in all, 2.5 in
     long, 0.5 in in, 3 in along the top), Kodak's printed sheet (all four
     edges 0.62 in in, from 0.5 to 10.5 in down the sides and 1.5 to 7 in
     along the top and bottom) and Alliance's patch-T sheet (measured at
     100 dpi: all four edges, the first bar 0.63 in in, N W N W, the
     sides' bars from 0.63 to about 9.5 in, the top and bottom's about
     1.55 to 6.96 in). Both
     sheets carry what the real ones do in the middle: a title, lines of
     text and their type as Code 39 (`PATCH1`…`PATCHT`, 20 mil).
     `with_patch_bars` asserts Alliance's is drawn only as a T.
   - **The corpus tier draws with PDFium directly:** proposal 15's
     `separate::find` draws through `Views` (slice 5), so the tier calls
     `PdfiumEngine::open_bytes` and `render_with(…, Job::Page { width:
     page width × 150/72, R0 }, Rgba, Draw { annots: false, forms: false
     })` and `to_gray()`, the same content-only picture with no Session.
     PDFium draws under its global lock, so only the reads run in
     parallel. Pages with `/Rotate` (0025212, 0001706, 0028220, probed)
     draw in the turned frame at 150 dpi on both axes.
   - **"No patch code" counts Code 39 `PATCH…` too** (proposal 14), read
     with every symbology, and a page counts as a sheaf code if either
     read gives a QR starting `SHEAFSEP`: both are literals in the test
     until slices 4 and 5 define the sheet's text and the patch mapping.
   - **The speed gate's rows:** "the bench: 44 ms plus 5" is the bench's
     text page (DejaVu glyphs) decoded at 150 dpi in black and white,
     plus the patches. Asserted under `SHEAF_GATE_CODES`, each the median
     of 21 reads with [Qr] plus patches: the stroke page at 150 dpi in
     gray and in 1-bit, and the synthetic and heavy 7k's first pages drawn
     content-only at 150 dpi in gray. Measured only: every symbology at
     150 gray and at 200 in 1-bit, and the patches alone (`codes::read`
     with no symbologies). Each row first reads the page and must find
     nothing. The measured rows' baselines are the bench's 70 ms
     (text200bw) and the prototype's 5 ms.
   - **Proposal 12's names:** `Symbology` and `PatchCode` derive Debug,
     Clone, Copy, PartialEq, Eq and Hash only; serde and the CLI's tokens
     (`--find code39`, `patch:2,3,T`) are slice 5's.
   - **Not built here:** proposal 15's 2:1 box reduce while scanning,
     decoding on a rayon pool, and 200 dpi for 1D rules (slices 5 and 7;
     Left).
   - **The CI page's content is bytes:** sheaf-ops' clippy.toml bans
     `Content::encode`, so `BT /F1 11 Tf … Tj ET` and `re f` are written
     as a stream's bytes.
   - **The full corpus tier ran in the slice** (Measured), not only at
     the pre-merge run. It takes about 10 minutes in debug, too close to a
     foreground shell's limit, so step 5 ran it in the background and
     waited on its PID (`tail --pid`).
   - **The UTF-8 test, renamed:** the plan's
     `a_title_outside_latin_1_reads_back_exactly` would pass without the
     `CharacterSet` hint (rxing's encoder already falls back to UTF-8
     with its ECI outside Latin-1), so its fault check couldn't fail. It's
     `titles_outside_ascii_read_back_as_utf_8`, with a Latin-1 title
     ("Müller & Söhne ½", written as ISO-8859-1 without the hint) beside
     the CJK one, and it checks the raw bytes are the text's UTF-8.
   - **Added beyond the step list:** `the_same_text_makes_the_same_modules`
     pins the 21 × 21 picture of `qr("SHEAFSEP1|t=")` (byte mode with its
     ECI), so a change in rxing's encoder that would change every sheet's
     bytes shows across builds, not only within one process;
     `every_degradation_changes_the_page` (a degradation made a no-op
     would leave the matrix green);
     `bars_shorter_than_a_patch_read_as_none` (no test pinned the patch's
     length rule); `patch_sheets_carry_their_type_as_code_39`; the
     turned-labels test covers all four turns and checks the edge,
     inset and length; the patch negatives are skewed ±3° too; the real
     page asserts it has ink (9.4% at 150 dpi), so a page whose text
     didn't draw can't pass by reading nothing.
   - **The prototype's patch reader, changed in two places:** lines are
     gathered by type and place along the edge, each inset compared with
     the last agreeing line's (the prototype grouped by inset and compared
     with the first), so a skewed edge's bars can drift; and a line's runs
     are matched with `windows(9)` (the prototype's loop skipped a patch
     whose light after it was the band's last run). `Patch.inset` is the
     agreeing lines' mean. Lines are `0.05 in × dpi` pixels apart rounded
     down, 0.04 in at 75 dpi, as in the prototype; the doc says 0.04.
   - **Smaller changes:** `scanned_every_way`, `VARIATIONS`, `SHEET` and
     `SAMPLES` are pub in the fixture, so the patch tests and slices 4 and
     5 share the matrix; `code_page` takes `row_px` (a 1D code's bar
     height, a 2D code's module); `fixture::encode` panics on an encoder
     error (a fixture); the real page has 36 body lines at 11 pt, not
     about 55 (55 don't fit a Letter page with a header and a table); the
     sheet test (`a_sheet_with_patch_bars_reads_both`) puts the Kodak
     sheet's bars over the text page as instructed, the QR 24 px from the
     left bars, inside its 36 px quiet zone, and it still reads both;
     `a_page_with_nothing_reads_nothing` reads 9 scans (300 gray, 150
     gray and 1-bit), the full grid being steps 2 and 3's; the gate's
     slow rows are collected and asserted after the table prints.
   - **No guard for tiny pages beyond "nothing asked":** step 1 probed 0
     × 0, 1 × 1, 0 × 10, 3 × 3 and 40 × 40, black and white, through
     `Rxing` with every symbology, and none panicked; the patch reader's
     Otsu level is 0 on an empty image.
   - **The quiet run is wider than a space** (review finding 9): `QUIET`
     was 0.08 in, a printed space, while `SPACE` takes up to 0.14 in, so
     a line through more than four bars of a patch's widths read a patch
     from its middle and print 0.1 in after a label's bars didn't stop
     it; with the rule removed nothing failed. It's 0.18 in now, above
     `SPACE` and under A-61599's 0.25 in from other print (every layout
     and degradation test still passes at 0.24).
   - **The patch reader's Otsu level is its own** (review finding 7):
     imageproc 0.27's `otsu_level` counts in u32, which overflows on more
     than about 16.8 M pixels of one gray (a panic in debug, a wrong
     level in release); the reader's work image is sized by `dpi` alone,
     so a large page or a low `dpi` reached it. `patch::otsu_level` is
     the same arithmetic in u64, its level imageproc's on every page
     tested (`the_otsu_level_is_imageproc_s`).
   - **The fixture keeps its own generator** (review finding 10, kept):
     the workspace's tests seed `fastrand`, but the codes' tests were
     settled on these exact pages and slices 4 and 5 build on them, so a
     dependency update mustn't draw different pages; its doc says so.
   - **One `drawn`** (review finding 12): the codes tests' and the gate's
     content-only draw is `common::drawn` in sheaf-ops' tests, which
     gate_7k now includes; slice 5's `separate::find` draws the same
     picture through `Views`.

   New crates (each license from its own Cargo.toml; `cargo tree -p
   sheaf-image -e normal` before and after; none pinned to a
   pre-release): new to the workspace, rxing 0.9.3 (Apache-2.0),
   rxing-one-d-proc-derive 0.9.1 (Apache-2.0) and codepage-437 0.1.0
   (MIT), and in Cargo.lock only, as codepage-437's build-dependencies,
   csv 1.4.0 and csv-core 0.1.13 (Unlicense/MIT). Already in Cargo.lock
   but new to sheaf-image's tree: aho-corasick 1.1.5 and memchr 2.8.3
   (Unlicense OR MIT), chrono 0.4.45, iana-time-zone 0.1.65, num-bigint
   0.4.8, once_cell 1.21.4, regex 1.13.1, regex-automata 0.4.18,
   regex-syntax 0.8.11, scopeguard 1.2.0, simdutf8 0.1.5, thiserror and
   thiserror-impl 2.0.21 and unicode-segmentation 1.13.3 (MIT OR
   Apache-2.0), core_detect 1.0.0 (MIT/Apache-2.0), multiversion_no_op
   1.0.0 (Apache-2.0 OR MIT), encoding_rs 0.8.42 ((Apache-2.0 OR MIT)
   AND BSD-3-Clause); iana-time-zone's platform dependencies were in
   Cargo.lock already, and num and syn in sheaf-image's tree (imageproc,
   nalgebra). rxing ships no NOTICE file. README's License paragraph
   names rxing; `packaging/licenses/` carries the engines' and models'
   licenses, no crate's (sheaf's own LICENSE-APACHE is the same text).
   The CI pixels-only check passes: no lopdf, PDFium, OCR, gpui or other
   sheaf crate under sheaf-image.

   Found while building:
   - **imageproc 0.27's `otsu_level` multiplies in u32**
     (`threshold as u32 * hist_count`, and `width * height`): on an image
     with more than about 16.8 M pixels of one gray level it panics in
     debug and gives a wrong level in release. The patch reader no longer
     calls it (review finding 7: its own `otsu_level` counts in u64);
     clean.rs still does, on reduced images as far as step 3 saw (not
     every caller traced). Upstream.

   What review found and fixed: the slice plan was reviewed adversarially
   before the steps started, and its findings are the deviations above,
   as resolved (the planner's list). Each step checked its new checks by
   switching their behavior off (Checked by tests). The wrap-up read the
   codes module, the patch reader, the fixture and both test files again
   against the plan and found nothing to fix in the code; the plan's
   text was behind it in Research's rxing features, proposal 15's one
   decode, slice 3's Builds and Checks and the Risks' codes, now as built.

   The code review of the slice's changes then found 13 things; 12 were
   fixed and one kept with a reason:
   - **A turned-down code hid a later one** (findings 1 and 4, major):
     `Rxing::read` returned the first code of any pass, so a vendor's QR
     code hid a rule's Code 128 on the same page every time (a probe: a
     QR URL and a Code 128 read for [Qr, Code128] gave the QR). `Reader`
     takes `accept` now and each symbology is decoded alone (Deviations);
     `a_code_turned_down_never_hides_a_later_one` pins it.
   - **The corpus tier looked for a patch sheet's Code 39 only on the
     first code** (findings 1, 5 and 8): on the 16 baseline pages whose
     first code was 2D, no Code 39 was ever read. The tier reads every
     symbology taking only a sheaf code or a `PATCH…` Code 39 now, so
     both are looked for past every other code on every page.
   - **The quiet run and the 8-line rule were pinned by nothing**
     (findings 2 and 9): `QUIET` is 0.18 in (Deviations), and
     `bars_without_light_round_them_read_as_none` (print 0.1 in after a
     label's bars; seven bars in a row) and
     `bars_too_few_lines_cross_read_as_none` (dashed bars on every third
     line, 0.72 in long but 7 lines; on every line, a patch) pin them.
   - **Docs** (findings 3 and 11): the sheets' insets (Kodak's 0.62 in,
     Alliance's 0.63) in slice 3's Checks; A-61599's bar length (0.75 in,
     2 in for Kodak's Patch-Reader scanners) in patch.rs and Research;
     "decoded once" is "binarized once" in codes.rs.
   - **`Modules`' sides were public beside its private modules** (finding
     6): `width()` and `height()` now, and `dark` asserts its column and
     row (`a_module_past_the_code_s_side_panics`: `dark(21, 0)` of a 21 ×
     21 code read row 1's first module).
   - **imageproc's Otsu level overflows** (finding 7): the patch reader's
     own (Deviations); `a_page_too_big_for_imageproc_s_otsu_level_reads`
     (4,200 × 4,200 white at 75 dpi) panics with imageproc's.
   - **Two copies of the content-only draw** (finding 12): one, in
     common (Deviations).
   - **Untested choices** (finding 13):
     `the_type_read_along_the_most_length_is_the_page_s` (`strongest`:
     the most length summed over edges, the first of `ALL` on a tie) and
     `codes_asked_for_read_in_a_fixed_order` over every neighbor in the
     order (QR and Data Matrix, Data Matrix and PDF417, PDF417 and Code
     39, Code 39 and Code 128, the later named first and drawn where a 1D
     read meets it first).
   - **Kept: the fixture's own generator** (finding 10; Deviations).

   Measured (AMD EPYC 7302P, 32 threads, release builds unless marked):
   - **The codes gate** (`codes_on_a_page_with_none`, the first test of
     the gate run below, at a 1-minute load of 3.5; each row the median
     of 21 reads after a first that found nothing):

     | Row | Now | The bench |
     |---|---|---|
     | stroke page, 150 dpi gray, QR + patches (asserted ≤ 60 ms) | 14.2 ms | 44 ms + 5 |
     | stroke page, 150 dpi 1-bit, QR + patches (asserted) | 13.4 ms | 44 ms + 5 |
     | the synthetic 7k's page 1 (Helvetica), content alone at 150 dpi gray, QR + patches (asserted) | 13.5 ms | 44 ms + 5 |
     | the heavy 7k's page 1 (embedded DejaVu), the same (asserted) | 13.0 ms | 44 ms + 5 |
     | stroke page, 150 dpi gray, every symbology + patches | 56.1 ms | 44 ms + 5 |
     | stroke page, 200 dpi 1-bit, every symbology + patches | 80.9 ms | 70 ms + 5 |
     | stroke page, 150 dpi gray, patches alone | 6.25 ms | about 5 ms |

     Real glyphs cost no more than the strokes. Read for sheaf's sheet
     alone, a page with no code takes about a third of the bench's
     figure, which was for every separator symbology; read for every
     symbology with its patches, the stroke page takes 56 and 81 ms,
     against the bench's one call's 44 and 70 (plus 5) on its own page
     (DejaVu, 1-bit). The fixed order itself costs 2–3%: a temporary test,
     timed in release at a load of 13 and removed, read every symbology
     in 50.0, 48.2 and 76.2 ms (150 dpi gray, 150 1-bit, 200 1-bit) in
     the fixed order and 49.0, 47.0 and 74.4 ms in one call with the five
     formats; Data Matrix's reader is the dearest, 22 ms at 150 dpi and 51
     at 200 (QR's 8–10 and 23). Step 4's first run, at a load of 27:
     13.1–14.5 ms asserted, 56.5, 80.3 and 6.23 ms measured.
     After review (each symbology decoded alone, past codes turned down;
     the patch reader's own Otsu level and `QUIET` 0.18), with
     `SHEAF_GATE_CODES=1` once the load was under 4 on two checks 30 s
     apart (2.9–3.0): asserted 15.0, 13.7, 13.9 and 13.4 ms; every
     symbology 60.4 ms at 150 gray and 87.8 at 200 in 1-bit; patches
     alone 6.28 ms. The rows read for QR alone are unchanged (one pass
     either way). Decoding Code 39 and Code 128 apart is the correctness
     fix's price on a page with no code: back to back at a load of
     3.5–6, the 1D symbologies in one call took 56.0 and 81.5 ms and
     apart 60.6 and 88.2 (8%), taken, since in one call a form's Code 39
     id hides a rule's Code 128.
   - **The gates** (`gate_7k`, release, `--ignored --test-threads=1` in
     one run with `SHEAF_GATE_CODES=1`, started once the 1-minute load
     was under 4 on two checks 30 s apart: 3.5–6.1 through the heavy's
     timed gates, 4.3–5.0 through the scanned's, 3.6–4.3 through the
     synthetic's and under 3 through the history gates; 13 of 13 tests
     pass, every asserted gate, both memory rows; 2,933 s):

     | Row | Synthetic | Heavy | Scanned |
     |---|---|---|---|
     | geometry walk (asserted < 100 and < 250 ms); ready (asserted < 1 s on the heavy) | 48.8 ms; 60.3 ms | 80.6 ms; 348 ms | 74.1 ms; 466 ms |
     | realize + open + render 1,024 px, an edited page (asserted < 15 and < 25 ms) | 12.0 ms (slice 2: 12.0) | 21.5 ms (22.0) | 32.7 ms |
     | thumbnail strip, crop + watermark + header, 320 px (asserted ≥ 310 pages/s) | 420 pages/s | 352 pages/s | 163 pages/s |
     | recognized text on every page: census (asserted 1 font, one block a page); the Append | 1, 7,000; 9.89 s | 1, 7,000; 10.09 s | 1, 7,000; 9.92 s |
     | one-mark Append on the scanned (asserted < 50 ms); reorder Append on the file opened again (asserted ≤ 1 s) | | | 25.7 ms; 877 ms |
     | the Rewrite against slice 8's writer, in the gates' own process and in `rewrites_on_the_synthetic_and_heavy_7k`'s (fails past 1.3×, flagged past 1.2×) | 151 ms, 1.12×; 153 ms, 1.14× | 746 ms, 1.05×; 739 ms, 1.05× | |

     The rest: history's 200 OCR versions of 1,000 pages 236 MB (≤ 256
     MB, asserted); snapshot 0.24–0.28 µs and `Tx::check` of a delete of
     every page 0.42–0.63 ms; engines after 220 groups 28.9–29.0 MB
     weighed, ≤ 6 open; crop all pages 9.97–13.3 ms; Bates apply
     0.62–0.64 ms, one font; AddMark 0.020 ms and a full reorder 0.97 ms
     at 20,000 marks; ready on 20,000 sheaf marks 614 ms (< 1 s; slice
     2's run 519); "Page n of N" moved after a save, the Append 362–810
     ms (≤ 1 s), sections 1.36–3.33 MB (≤ 8 MB). The scan memory gate
     (`scan_of_500_black_and_white_pages`, load 2–39 through it, the
     photo profile's own threads among it): photo 0.308 pages/s, 232 MB
     peak, 177 MB added (< 400 MB, asserted); scanner 1.15 pages/s, 239
     MB peak, 186 MB added; the untitled sidecar rows as slice 2 left
     them (10.4 MB in 28.0 and 211 ms; the color sample's 6.10 MB in 196
     ms). Proposal 8's accepted misses (`SHEAF_GATE_PROPOSAL_8`):
     synthetic 22.4, 20.4, 18.8 and 28.5 ms, heavy 38.3, 32.3, 33.5 and
     41.3 ms (slice 2: 22.7, 20.4, 19.2, 29.8; 36.3, 32.4, 32.4, 42.5),
     the Appends on the file opened again 895 and 1,361 ms (1,073 and
     1,517). The flatten export's RSS (`SHEAF_GATE_FLATTEN`): the
     heavy's peak 517 MB (slice 2: 515), the synthetic's 88.8 MB (88.7),
     the scanned's 1,077 MB (1,076). This slice's code reaches none of
     these paths (sheaf-image's codes module is called only by tests and
     the gate's codes rows), so the differences are the machine's.
   - **Debug times** (sheaf-image at opt-level 1): in the workspace run,
     sheaf-image's 58 unit tests took 50.4 s on 32 threads beside the
     other test binaries. Step 3's whole sheaf-image suite pinned to 4
     cores (`taskset`, `--test-threads=4`, standing in for CI's runners)
     took 67.5 s with a 1.14 GB peak, against 25.9 s after step 2: about
     a minute more on each CI system's test step. Each matrix test takes
     8.5–11.9 s alone and each patch layout test about 44 s, nearly all
     of it the degradations (a page's 48 scans about 7 s: the Triangle
     resample about 250 ms a 300 dpi page, the blur 420 ms, the noise 290
     ms); the patch reader takes about 9 ms a page, the codes tests' peak
     is 1.7 GB on 32 threads (about 230 MB a matrix test, holding its 48
     scans). No cell was cut.
   - **The corpus tier** (`scans_hold_no_separators`, debug, in step 5,
     at a load of 7–23 from the other checkout): 709 files and 5,697
     pages drawn at 150 dpi and read in 577 s (9 min 37 s wall), a 1.54
     GB peak; 0 sheaf codes, 0 patches (bars or Code 39 `PATCH…`), no
     rxing panic, no file failed, no "skipped: no corpus". The patch
     reader needed no fix on real scans: no feeder border, rule or column
     of text read as a patch. The baseline, the first code each page
     gives read for every symbology, 25 on 19 files (all under
     `scans/safedocs/`; pages from 1; the texts as read):

     | File | Page | Symbology | Text |
     |---|---|---|---|
     | 0000504 | 1 | QR | `https://www.igbce.de/igbce/mitglied-werden/beitrittsformular/` |
     | 0000574 | 1 | Code 128 | `2061622` |
     | 0001676 | 1, 2 | Data Matrix | `000112213493100000004770010177000007`, `…4767010174000008` |
     | 0002101 | 1 | QR | `http://www.pizzafaenza.com/` |
     | 0002505 | 2 | QR | `http://www.hannover.de/zurschule-flyer` |
     | 0003187 | 1 | Code 128 | `ES002441219T3` |
     | 0004119 | 14 | PDF417 | `M_DG#06400802208#PG#24/08/2018#0004000.E` |
     | 0004944 | 1 | QR | a plant-co.jp campaign URL |
     | 0005707 | 1 | QR | `みゆきデイサービスセンター\r\nhttp://care-support.link\r\n` |
     | 0010929 | 1 | Code 39 | `US009782084B2` |
     | 0011731 | 1 | Code 39 | `US005464133A ` |
     | 0012942 | 1, 3, 5, 7, 9 | Data Matrix | `0205485000128`, `0305486000128`, `0405487000128`, `0505488000128`, `0105484000128` |
     | 0019069 | 2 | QR | `http://www.marukasa.com/` |
     | 0020336 | 2 | QR | a Mexican report's stamp (`ente: Isla\r\nrpt: Proyecciones de Egresos - LDF …`) |
     | 0020822 | 1 | Code 128 | `0000000000044306` |
     | 0026808 | 1 | Code 39 | `4428056894` |
     | 0049724 | 1 | QR | a TÜV certificate URL ending in five NULs (the code's own padding, as far as can be told) |
     | 0051867 | 1, 2 | Code 39 | `0810` (short enough to be a junk read, but read with the formats limited, so listed), `SR97006307` |
     | 0067466 | 1 | Code 39 | `US008359275B2` |

     By symbology: QR 8, Data Matrix 7, Code 39 6, Code 128 3, PDF417 1.
     Run again after review (debug, load about 5–7), the separator read
     now looking for a sheaf code and a `PATCH…` Code 39 past every other
     code on every page: 709 files, 5,697 pages in 497 s (8 min 17 s
     wall), a 1.53 GB peak; 0 sheaf codes, 0 patches, no file failed, no
     "skipped: no corpus"; the baseline the same 25 codes on the same
     pages. Its two fault checks, narrowed to 0049724 and 0010929, still
     fire (Checked by tests).
   - **The corpus** (`../pdf-corpus`, `SHEAF_CORPUS` unset; no "skipped:
     no corpus" line), every count the baseline's: materialize_corpus's
     identity 4,389 of 4,392 written, 2,269,688 primary objects surviving,
     919,465 copied as they were, 432 changed by `keep_only`, 0 failed;
     2,901 forms without their first page, 0 failed; every page
     duplicated, 4,389, 0 failed; inserted twice into the 1040, 1,098, 0
     failed; four times, 0 differ; the logo, 0 of 2,037,600 bytes differ.
     append_corpus: 4,389 marked, 3,770 appended, 260 rewritten (211
     RemovalCheck, 34 BaseObjectsMissing, 15 BaseUnsound), 359 asked, 0
     failed, 0 derived vetoes and 211 on the files' own objects; the
     oracle finds text gone from 2,899 of 2,901 (none from 0016064 and
     0112654). encrypted_corpus: the Rewrite keeps 573 of 573 and the
     Append 572 (0630899 declined). The signatures tier
     (`the_corpus_s_signatures_agree_with_pyhanko`): 413 signed files,
     0 disagree. The other ignored tiers are the pre-merge run's (Greg,
     2026-10-20): this slice touches neither the writer nor the realizer.
   - **Suites:** `cargo test --workspace` 1,796 passed, 0 failed and 60
     ignored (the baseline's 1,770 and 58; this slice's 26 tests, and the
     gate test and the corpus tier ignored), no "skipped: no corpus", 11
     min wall, a 4.95 GB peak. fmt clean; clippy (`-D warnings`, all
     targets) clean on the workspace and on sheaf-image alone with no
     features; clippy with sheaf-app's Linux-only gates switched off
     (`nonlinux-clippy.sh`) clean; on `x86_64-pc-windows-msvc`, `cargo
     check -p sheaf-image --all-targets --features fixtures` and clippy
     (`-D warnings`, all targets) for sheaf-image with and without
     `fixtures` and for sheaf-ops pass. CI's pixels-only check finds
     nothing under sheaf-image. The interop tier and the release
     properties weren't run: the slice writes no file and doesn't touch
     the writer. No binary calls `codes` yet, so the release `sheaf`
     binary's size is slice 5's to record, when `sheaf separate` links
     rxing in.
     After review: sheaf-image's suite 65 of 65 (the 58 and 7 new),
     sheaf-ops' codes test and its tier (above); fmt and workspace clippy
     (`-D warnings`, all targets) clean, sheaf-image's clippy clean with
     and without `fixtures`, and on `x86_64-pc-windows-msvc` clippy for
     sheaf-image (with `fixtures`) and sheaf-ops (all targets, gate_7k
     now including common) passes. The workspace suite wasn't run again:
     only sheaf-image and sheaf-ops' tests changed, and clippy compiled
     every sheaf-ops test crate that includes common. No proptest seed
     was added. The final verification ran it all again: `cargo test
     --workspace` 1,803 passed (the 1,796 and review's 7), 0 failed, 60
     ignored, no "skipped: no corpus"; fmt, workspace clippy,
     `nonlinux-clippy.sh` and the Windows check and clippy of sheaf-image
     and sheaf-ops clean; the corpus counts the baseline's, as above.

   Checked by tests:
   - **rxing and QR modules** (codes/tests.rs):
     `a_qr_code_s_modules_read_back_as_their_text` (at 4 and 9 px a
     module, a 4-module quiet zone added),
     `titles_outside_ascii_read_back_as_utf_8` (Latin-1 and CJK titles,
     their raw bytes the text's UTF-8),
     `qr_codes_are_level_m` (rxing's error correction metadata "M"),
     `the_same_text_makes_the_same_modules` (and the pinned 21 × 21
     picture), `the_longest_title_fits_version_9` (the 34-byte prefix and
     a 120-byte UTF-8 title: square, at most 53 modules),
     `only_the_symbologies_asked_for_are_read`,
     `a_code_not_asked_for_never_hides_one_asked_for` (a Data Matrix and
     a Code 128 on one page, read for Code 128),
     `codes_asked_for_read_in_a_fixed_order` (each neighbor pair in the
     order on one page, the later drawn where a 1D read meets it first:
     20 reads for both, the later named first, give the earlier every
     time, and the later read alone gives it),
     `a_code_turned_down_never_hides_a_later_one` (a URL's QR code before
     a Code 128, a Data Matrix before a Code 39 `PATCHT`, a Code 39 id
     before a Code 128: read taking any code, the first; taking the later
     one's text, the later; taking none, nothing),
     `a_module_past_the_code_s_side_panics`.
   - **The symbology matrix:** `qr_…`, `data_matrix_…`, `code_39_…`,
     `code_128_…` and `pdf417_reads_back_scanned_every_way`, each 48 scans
     read for its own symbology and for every one, exactly its text, every
     miss listed: 480 of 480 read.
     `blank_text_and_rules_pages_read_as_nothing` (each page as is and
     skewed ±3°, at 300, 200 and 150 dpi, gray and 1-bit, read for every
     symbology: 54 of 54 None).
     `every_degradation_changes_the_page` (every variation and bench
     entry changes a crop and ends at its dpi; a turn swaps the sides;
     the threshold leaves only 0 and 255).
   - **Patch codes** (codes/patch/tests.rs):
     `kodak_label_patches_read_as_their_types_scanned_every_way` and
     `kodak_sheet_…` (6 types × 48 scans, 288 each),
     `alliance_t_reads_as_t_scanned_every_way` (48),
     `the_bench_s_degradations_keep_patch_types` (6 × 17),
     `turned_labels_tell_3_from_t_and_1_from_6` (every turn: the type,
     the edge it came to, its inset 0.5 ± 0.05 in and length 2.25–2.55
     in), `bars_shorter_than_a_patch_read_as_none` (0.4 in bars none, 0.75
     in a T, at every dpi and tone),
     `patch_sheets_carry_their_type_as_code_39` (200 dpi 1-bit),
     `pages_without_patches_read_as_none` (blank, text,
     rules, a black feeder border, a letterhead rule and the five sample
     code pages, as is and skewed ±3°, at three dpi in gray and 1-bit: 180
     of 180 none), `bars_without_light_round_them_read_as_none` (print
     0.1 in after a label's bars, and seven bars in a row, at three dpi
     in gray and 1-bit), `bars_too_few_lines_cross_read_as_none` (dashed
     bars on every third line: none; on every line: a T; at 300 and 150
     dpi, gray and 1-bit),
     `the_type_read_along_the_most_length_is_the_page_s`,
     `the_otsu_level_is_imageproc_s`,
     `a_page_too_big_for_imageproc_s_otsu_level_reads`.
   - **A page's codes:** `a_sheet_with_patch_bars_reads_both` (a QR
     sheet with Kodak sheet T bars: T and the sheet's text at 300 gray,
     150 gray and 150 1-bit), `a_page_with_nothing_reads_nothing`;
     sheaf-ops' `a_page_of_real_text_reads_as_nothing` (PDFium's
     Helvetica page at 150 and 200 dpi, gray and 1-bit, every symbology:
     nothing, with ink on more than 2% of it).
   - **The gate and the tier:** `codes_on_a_page_with_none` (each row
     must read nothing first) and `scans_hold_no_separators` (Measured).
   - **Fault checks,** each file restored from the step's own copy and
     checked with cmp, and no proptest seed added (only
     materialize/append_rewrite_tests.txt is there):
     - no `CharacterSet` hint fails
       `titles_outside_ascii_read_back_as_utf_8` (the Latin-1 title's ü
       came out as byte 252) and the pinned picture; level L fails
       `qr_codes_are_level_m` and the picture;
       `PossibleFormats` None in the 1D call fails
       `a_code_not_asked_for_never_hides_one_asked_for` (the Data Matrix
       found first and filtered out); one call with every format asked
       fails `codes_asked_for_read_in_a_fixed_order` (DataMatrix where Qr
       was wanted);
     - `TryHarder` off fails the Code 39, Code 128 and Data Matrix
       matrices (96 of 96 missed each; QR and PDF417 read every cell
       without it, the code sitting away from the page's middle rows) and
       the fixed-order test; `Turn(180)` a no-op fails
       `every_degradation_changes_the_page`;
     - the bars read from the far side fails each Kodak layout test (192
       of 288: types 1, 3, 6 and T; 2 and 4 read the same either way),
       Alliance's (48 of 48), the bench test (68 of 102) and the turned
       labels; the patch's least length removed fails
       `bars_shorter_than_a_patch_read_as_none` (0.4 in bars read T at
       every dpi and tone). Not pinned alone in the steps: the quiet-run
       rule (with it and the stretch rules both off, 21 of 180 negatives
       read a patch) and the 8-line rule; after review, `QUIET` at 0
       fails `bars_without_light_round_them_read_as_none` on all 12
       reads, and at the steps' 0.08 on the 6 reads of print after a
       label; `LINES` at 0 fails `bars_too_few_lines_cross_read_as_none`
       (a T on every third line, at both dpi and tones);
     - after review, too: `accept` ignored fails
       `a_code_turned_down_never_hides_a_later_one`; the steps' one call
       for the 1D symbologies fails it and the fixed-order test; PDF417
       before Data Matrix, or Code 128 before Code 39, in `ALL` fails the
       fixed-order test; `dark` without its assert fails
       `a_module_past_the_code_s_side_panics`; imageproc's Otsu level in
       the patch reader panics `a_page_too_big_…` (contrast.rs:61,
       overflow); `strongest` taking the first type found, or the last
       on a tie, fails `the_type_read_along_the_most_length_…`;
     - `CODES_GATE` at 1 ms with `SHEAF_GATE_CODES` set fails the gate,
       naming its four asserted rows, and passes with it unset; one decode
       with every format, `TryHarder` and no "asked for" filter reads a
       junk Codabar "$0" off the stroke page at 150 gray (failing
       `blank_text_and_rules_pages_read_as_nothing` at 1 of 54 and
       `a_page_with_nothing_reads_nothing`, and the gate's first-read
       check on two rows), but nothing off the real Helvetica page or the
       7k's pages, as the bench found;
     - the tier's `SHEAFSEP` check off fires on 0049724 p.1's TÜV QR;
       every Code 39 counted as a patch fires on 0010929 p.1's
       "US009782084B2" (each narrowed to those two files).

   For Greg:
   - **Hand checks:** none reachable yet; no command reads codes until
     slice 5's `sheaf separate`. Then: printed Alliance patch-T and Kodak
     patch sheets, and SurePrep-style Code 128 sheets, scanned on the
     office's fi-6130s at 150, 200 and 300 dpi in 1-bit and gray (the
     bench's prototype read 60 of 60 renders of Kodak's sheets, but this
     slice's tests draw the layouts from geometry).
   - **No gate decision:** the four asserted rows take 13–14 ms of the
     60 allowed, on real glyphs as on strokes. Read for every symbology a
     page takes 50–56 ms at 150 dpi and 81 at 200 (Measured), which is
     what slice 5's rules will cost a page beside sheaf's sheet.
   - **CI time:** the matrix and the patch layouts add about a minute to
     each CI system's test step (debug, 4 cores), nearly all of it
     degrading the drawn pages; no cell was cut, as the plan asked.

   Left for later slices:
   - **Slice 4:** sheaf's sheet draws `codes::qr`'s modules as rectangles,
     runs merged; it defines the `SHEAFSEP1` text and its prefix constant
     (tests/codes.rs' `SHEET` literal then uses it); the fixture's
     degradations run on PDFium renders of the sheet. The longest title
     fits version 9 (53 modules, 40 mm at 0.75 mm), as proposal 13's
     layout assumed; `the_longest_title_fits_version_9` pins it.
   - **Slice 5:** the Code 39 `PATCHn`/`PATCHT` text read as patch types
     (tests/codes.rs' `PATCH_TEXT` then uses it); serde and the CLI's
     tokens for `Symbology` and `PatchCode`; 200 dpi when a rule reads 1D
     codes; decoding on a rayon pool; the 297-page gate that builds on
     this slice's per-page time. `codes::read` and its `Rxing` read are
     what `separate::find` calls; `PageCodes` gives both, and the caller
     chooses, and `accept` is its match: `SHEAFSEP` for the sheet, each
     rule's pattern, `PATCH…` for a patch sheet's Code 39. Two limits to
     know. rxing gives one code a decode, so a turned-down code hides a
     second code of the same symbology on the page (a vendor's QR beside
     sheaf's sheet; rxing's multi readers, off with
     `multi_barcode_readers`, are the way if real pages show it). And
     decoding each 1D symbology alone costs a page with no code about 5
     ms at 150 dpi and 7 at 200 when both are asked (Measured), which
     the 297-page gate carries. A known limit of the patch reader, from a
     probe at the step 3 `QUIET` of 0.08 in: a Code 39 at 80 mil (bars
     as wide as a patch's) along an edge, about 8 in long, read as
     patches (Code 128 at 30–60 mil, Code 39 at 40 mil and every sample
     size read nothing, and the corpus had none); the review shows every
     separator, and the quiet width (0.18 in now, under A-61599's 0.25
     in from other print) is the first knob if real pages show it.
   - **Slices 5 and 7:** the 2:1 box reduce of a 300 dpi page while
     scanning, before the read.
   - **F16** (QR codes to links): a host barcodes interface could reuse
     `Reader`.
   - **The pre-merge run** of the ignored tiers should add `cargo test -p
     sheaf-ops --test codes -- --ignored --nocapture` (about 10 minutes,
     nearly all PDFium drawing pages one at a time under its lock; release
     helps little, PDFium being prebuilt).
