# Build or adopt 64: Image downsampling

Area: M14: Document processing · Part of [build or adopt](README.md)

- **Need:** resample page images to a target ppi in gray, RGB, CMYK and
  16-bit, then re-encode.
- **Where:** `ROADMAP.md:436-437` ("image downsampling and
  recompression").
- **Today:** sheaf resizes with `image::imageops::resize` (`clean.rs:251`
  and others, `scan.rs:135`).
- **Candidates:** image's `resize` (pure Rust, safe, slow; figures
  unverified); [`fast_image_resize`](https://crates.io/crates/fast_image_resize)
  6.1.0 (2026-07-21; MIT OR Apache-2.0; Cykooz alone; 22M downloads;
  SIMD with about 680 `unsafe` in `src/`; U8 to U8x4, U16 and F32 pixels,
  so CMYK works as U8x4; a wasm32 SIMD path; rayon optional);
  [`pic-scale`](https://crates.io/crates/pic-scale) 0.7.12 (2026-09-10;
  BSD-3-Clause OR Apache-2.0; awxkee; used by hayro); hayro-jpeg2000's
  `target_resolution`, which decodes JPX at reduced size.
- **Recommendation:** **adopt fast_image_resize** for M14's new paths,
  after measuring it against image's resize on corpus scans. Leave
  cleanup's existing calls alone (byte-identical rule). Medium.
