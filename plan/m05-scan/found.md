# M5: Found while planning

Part of [M5: Scan & OCR](README.md).


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
