# M5 proposal 22: Testing without a scanner

Part of [M5: Scan & OCR](../README.md) · [all proposals](README.md)

**Testing without a scanner.** Every layer has a test that needs no
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
