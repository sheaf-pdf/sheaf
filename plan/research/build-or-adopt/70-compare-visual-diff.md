# Build or adopt 70: Compare: visual (pixel) diff

Area: M14: Document processing · Part of [build or adopt](README.md)

- **Need:** pages rendered at the same dpi, compared with tolerance for
  anti-aliasing, the changed regions boxed.
- **Where:** `ROADMAP.md:434-435`.
- **Today's plan:** none.
- **Candidates:**
  - [`dssim`](https://crates.io/crates/dssim) and
    [`dssim-core`](https://crates.io/crates/dssim-core) 3.5.1
    (2026-08-29): **AGPL-3.0. Out.**
  - [`image-compare`](https://crates.io/crates/image-compare) 0.5.0
    (2025-08-18; MIT; ChrisRega alone; 2.1M downloads; no `unsafe`; on
    image, itertools and rayon): SSIM, RMS, a "hybrid" score, difference
    images.
  - [`blazediff`](https://crates.io/crates/blazediff) 6.0.1 (2026-08-27;
    19 releases since 2026-01; MIT; teimurjan; 533k downloads in 90
    days; 352 stars): pixelmatch-compatible YIQ delta and anti-alias
    detection; SIMD (63 `unsafe`); `default-features = false` drops its
    command line and its PNG and JPEG code.
  - [`dify`](https://crates.io/crates/dify) 0.8.0 (MIT): a command-line
    tool. [`pixelmatch`](https://crates.io/crates/pixelmatch) 0.1.0
    (2021): stale. pixelmatch's own algorithm (mapbox, ISC) is about 200
    lines and easy to port.
- **Fit:** the kernel is the easy part. Matching pages across inserted
  or deleted pages, and grouping changed pixels into boxes (imageproc's
  connected components, already used), are sheaf's either way.
- **Recommendation:** **build on PDFium renders and imageproc,** with
  blazediff behind a small trait or a credited port of pixelmatch's
  algorithm. image-compare only if a single similarity score is wanted.
  Medium.
