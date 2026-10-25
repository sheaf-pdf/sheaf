# M5: Scope

Part of [M5: Scan & OCR](README.md).


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
  sorting pages by form (see [Not in M5](not-m5.md)).
