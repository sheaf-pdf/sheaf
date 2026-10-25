# Build or adopt 14: Scan cleanup

Area: M5 (done): OCR, scan cleanup, image import · Part of [build or adopt](README.md)

- **Need:** deskew, despeckle, Sauvola binarization, page finding,
  lighting.
- **Where:** M5, done (`ROADMAP.md:116-119`).
- **Today:** `sheaf_image::clean` uses imageproc 0.27 for blur, Canny,
  morphology, contours, warps, median filters and connected components.
  sheaf's own: the projection-profile deskew (`clean.rs:490`), Sauvola
  on integral images (`:675`), despeckle over imageproc's labelling
  (`:815`).
- **Candidates:**
  - [`imageproc`](https://crates.io/crates/imageproc) 0.27.0
    (2026-06-02; MIT; image-rs): `adaptive_threshold`, Otsu, Kapur and
    Hough, but **no Sauvola and no deskew** (its 0.27.0 `contrast.rs` and
    `hough.rs`).
  - Searches for "sauvola", "deskew" and "binarization" found only small
    apps and one-off crates.
  - Leptonica has all of these. sheaf links it statically inside
    Tesseract (BSD-2), but tesseract-rs exposes no Leptonica API;
    [`leptonica-sys`](https://crates.io/crates/leptonica-sys) 0.4.9
    (2024) is a separate C link.
- **Recommendation:** **build in-house** (as done): nothing better
  exists, and sheaf's code is a few hundred lines. High.
