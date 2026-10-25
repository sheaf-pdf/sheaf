# Build or adopt 26: CMaps: predefined CJK CMaps and CID → Unicode

Area: M11: Security and redaction · Part of [build or adopt](README.md)

- **Need:** for a Type0 font whose `/Encoding` is a predefined CMap
  (90ms-RKSJ-H, UniJIS-UCS2-H, UniGB-UCS2-H, KSCms-UHC-H…), split strings
  into codes and map code → CID, for `/W` widths and glyph boxes. Map CID
  → Unicode through Adobe-\*-UCS2 when there's no `/ToUnicode`, for text,
  the removal check's `TextBag` and the leak check. M13 needs Unicode →
  CID to write such fonts.
- **Where:** M11 slices 3 and 5; M10.5's `FontDecoders`
  (`crates/sheaf-doc/src/content/text_bag.rs:146-230`); M13.
- **Today's plan:** none. sheaf parses `/ToUnicode` itself
  (`text_bag.rs:322-`: bfchar, bfrange, codespace ranges). A Type0 font
  with no ToUnicode decodes 2 bytes at a time (`:223-229`): its strings
  become hashes, consistent but not text. **Taken since:** done in the
  content interpreter PR (M11 slice 3, its wrap-up): hayro-cmap 0.1.0
  (with hayro-postscript 0.1.0 and brotli 8.0.4) for the 65 bundled
  maps, CIDs for `/W` and CID → Unicode where there's no ToUnicode;
  sheaf's ToUnicode parser kept, PDFium's code splitting kept in
  sheaf's own table while the codespace stays private (an upstream PR
  to expose it is for later: backlog F25). Adobe's notice ships as
  `packaging/licenses/LICENSE-cmap-resources`, brotli's as
  `LICENSE-brotli`.
- **Candidates:**
  - **[hayro-cmap](https://crates.io/crates/hayro-cmap) 0.1.0**
    (2026-04-12; [docs](https://docs.rs/hayro-cmap/0.1.0)):
    - Apache-2.0 OR MIT for the code. The bundled data is under Adobe's
      BSD-3 notice, "Copyright 1990-2023 Adobe" (`assets/LICENSE.txt`),
      which a binary distribution must reproduce (F18).
    - One release; 3.45M downloads, 3.24M of them in 90 days, riding on
      hayro-interpret. The hayro repo: 814 stars, pushed 2026-10-23.
    - Pure Rust, `forbid(unsafe)`, `no_std` without its default feature.
      Depends on hayro-postscript 0.1.0 and `brotli` 8 (BSD-3-Clause AND
      MIT, the full compressor; sheaf's lock has only
      `brotli-decompressor`).
    - **`load_embedded` bundles** (`src/bcmap/embedded.rs`) 65 CMaps in
      one 250,740-byte brotli blob: every name in ISO 32000-1 Table 118
      (61, Identity-H/V and H/V included) plus `Adobe-CNS1-UCS2`,
      `Adobe-GB1-UCS2`, `Adobe-Japan1-UCS2` and `Adobe-Korea1-UCS2`.
      Anything else returns `None`. The first use decompresses the whole
      bundle into one static `Vec`; its size is unverified.
    - Gaps: its codespace ranges are `pub(crate)` and unused ("Acrobat
      seems to ignore it"), so `lookup_cid_code(code, byte_len)` leaves
      the code length to the caller, by shortest match. `usecmap`
      recursion is capped (`MAX_NESTING_DEPTH`), but range counts aren't,
      beyond the input's size.
  - **Adobe cmap-resources**
    ([GitHub](https://github.com/adobe-type-tools/cmap-resources)): BSD-3;
    pushed 2023-11-15. Raw PostScript CMaps for every collection
    (Japan1-7, GB1-6, CNS1-7, Korea1/KR-9…). Data, not a crate.
  - **pdf.js bcmaps**
    ([external/bcmaps](https://github.com/mozilla/pdf.js/tree/master/external/bcmaps)):
    168 `.bcmap` files, 1,165,667 bytes, with Adobe's LICENSE, loaded on
    demand by URL. More than Table 118 (the UTF32 and 2004 variants), in
    Mozilla's own format. The approach to copy if Table 118 is too small.
- **Fit:** hayro-cmap supplies the data and the code → CID → Unicode
  lookups sheaf lacks, in safe Rust, already compressed. sheaf's own
  parser keeps `ToUnicode` and its `Limits` (hayro-cmap takes none).
  Mixed-width CMaps (RKSJ: 1- and 2-byte codes) work by shortest match; a
  code valid in the codespace but unmapped may split differently from
  PDFium, so corpus-check it.
- **Recommendation:** **build on hayro-cmap:** its bundle and CID →
  Unicode, sheaf's ToUnicode parser kept, and an upstream PR exposing
  codespace ranges, then revisit using it for both. Medium. What would
  change it: CJK files in the binders naming CMaps outside Table 118
  (then Adobe's full set, pdf.js-style).
