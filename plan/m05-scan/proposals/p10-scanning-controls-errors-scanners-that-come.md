# M5 proposal 10: Scanning: controls, errors, and scanners that come and go

Part of [M5: Scan & OCR](../README.md) · [all proposals](README.md)

**Scanning: controls, errors, and scanners that come and go.**
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
