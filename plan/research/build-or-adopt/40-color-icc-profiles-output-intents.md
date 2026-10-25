# Build or adopt 40: Color: ICC profiles and output intents

Area: M11: Security and redaction · Part of [build or adopt](README.md)

- **Need:** convert a redaction fill or stamp color into an image's
  ICCBased or CMYK space (M11, M13); write a PDF/A output intent profile
  (M14, `ROADMAP.md:455`); read profile headers for preflight-lite's
  color checks (`:456`).
- **Today's plan:** none written.
- **Candidates:**
  - [`moxcms`](https://crates.io/crates/moxcms) 0.9.1 (2026-09-15; 36
    releases since 2025-02; BSD-3-Clause OR Apache-2.0; awxkee alone;
    107M downloads; used by image 0.25.10, hayro-interpret,
    typst-library and jxl-oxide). `forbid(unsafe_code)` unless its SIMD
    features are on. Reads and writes profiles (`writer.rs`'s `encode`);
    builds sRGB, Display P3, BT.2020 and gray profiles (`defaults.rs`).
    **Already in the lockfile** (0.8.1, through image).
  - [`lcms2`](https://crates.io/crates/lcms2) 6.2.0 (2026-08-26; MIT) on
    `lcms2-sys` 4.0.7 (MIT): C, Little CMS, the reference; PDFium has its
    own copy for rendering.
  - [`qcms`](https://crates.io/crates/qcms) 0.3.0 (2024-01-09; MIT;
    Firefox): narrower.
- **Fit:** re-encoding in the image's own space needs no color
  management; only the fill color does. moxcms can generate an sRGB
  output-intent profile itself. Whether it does RGB-to-CMYK through a
  profile's B2A tables is unverified.
- **Recommendation:** **adopt moxcms** (already there); lcms2 only if
  preflight needs exact Little CMS behavior. Medium. The output intent's
  profile is D14: the core section would ship CC0 profiles instead
  (entry 68).
