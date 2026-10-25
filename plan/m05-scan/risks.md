# M5: Risks

Part of [M5: Scan & OCR](README.md).


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
