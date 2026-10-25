# M5 slice 7: Scanning in the app

Status: deferred · Part of [M5: Scan & OCR](../README.md)

**Scanning in the app** (proposals 2, 10 and 11).
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
