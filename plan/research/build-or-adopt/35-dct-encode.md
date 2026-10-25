# Build or adopt 35: DCT (JPEG) encode

Area: M11: Security and redaction · Part of [build or adopt](README.md)

- **Need:** re-encode a redacted or downsampled JPEG in gray, RGB or CMYK
  (Adobe-inverted CMYK included), ideally with the original's
  quantization tables and subsampling, to limit generation loss.
- **Candidates:**
  - image's `JpegEncoder` (in use): **only `L8` and `Rgb8`**; every other
    type is `Unsupported` (`image-0.25.10/src/codecs/jpeg/encoder.rs:494-510`).
  - [`jpeg-encoder`](https://crates.io/crates/jpeg-encoder) 0.7.1
    (2026-07-27; vstroebel alone; 9M downloads). License `(MIT OR
    Apache-2.0) AND IJG`: the IJG terms ask the docs to say the software
    is "based in part on the work of the Independent JPEG Group" (F18).
    `no_std`, no `unsafe` unless `simd`. 1-, 3- and 4-component color
    (CMYK, YCCK), custom quantization tables, chroma subsampling,
    progressive, optimized Huffman tables; no dependencies.
  - [`mozjpeg`](https://crates.io/crates/mozjpeg) 0.10.13 (2025-02-18;
    IJG) on `mozjpeg-sys` 2.2.3 (IJG AND Zlib AND BSD-3-Clause): C, needs
    nasm for SIMD; compresses better.
  - [`turbojpeg`](https://crates.io/crates/turbojpeg) 1.5.1 (2026-07-25;
    Unlicense OR MIT): C (libjpeg-turbo), with lossless transforms.
  - No crate does a DCT-domain wipe of a region (jpegtran-style), which
    would leave the rest untouched; `dct-io` 0.1.1 only reads and writes
    coefficients.
- **Fit:** jpeg-encoder covers CMYK and can reuse the source's tables, but
  it's one person's crate. Re-encoding must not copy APPn segments: an
  EXIF thumbnail, XMP or a Photoshop resource can hold a preview of the
  unredacted image.
- **Recommendation:** **adopt jpeg-encoder** for M11, M13 and M14
  re-encoding. Keep image's encoder where it's used today (the
  byte-identical rule). mozjpeg only if M14 measures a large saving.
  Medium-high.
