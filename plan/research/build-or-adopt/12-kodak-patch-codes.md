# Build or adopt 12: Kodak patch codes

Area: M5: separator sheets · Part of [build or adopt](README.md)

- **Need:** find patch bars (types 1, 2, 3, 4, 6, T) along a page's
  edges.
- **Where:** `m5-intake.md` proposal 14 (`:868-876`).
- **Today's plan:** build. "sheaf reads them itself (no barcode library
  does…)".
- **Candidates:** none. crates.io ("patch code", "patchcode", "kodak
  patch") and GitHub have no detector, and zxing-cpp has none.
- **Recommendation:** **build in-house**, as planned. The prototype read
  204/204 synthetic pages and 60/60 renders of Kodak's sheets. High.
