# Build or adopt 36: JPX (JPEG 2000) decode

Area: M11: Security and redaction · Part of [build or adopt](README.md)

- **Candidates:**
  - [`hayro-jpeg2000`](https://crates.io/crates/hayro-jpeg2000) 0.4.1
    (2026-10-04; 11 releases since 2025-12; MIT OR Apache-2.0; LaurenzV;
    4.7M downloads). `#![forbid(unsafe_code)]`, but its default `simd`
    feature uses `fearless_simd`, which has unsafe; `no_std`. "tested
    against 20.000+ images scraped from random PDFs" and "a large part of
    the OpenJPEG test suite"; "expect … worse performance than OpenJPEG,
    but the difference gap should not be too large"
    ([README](https://github.com/LaurenzV/hayro/tree/main/hayro-jpeg2000)).
    **It yields 8-bit only**, so 16-bit JPX drops to 8-bit.
    `DecodeSettings::target_resolution` decodes at reduced size, useful
    for M14.
  - [`openjpeg-sys`](https://crates.io/crates/openjpeg-sys) 1.0.12
    (2025-04-16; BSD-2-Clause): C, built from source.
    [`jpeg2k`](https://crates.io/crates/jpeg2k) 0.10.1 (2025-07-29; MIT
    or Apache-2.0) wraps openjpeg-sys or `openjp2`.
    [`openjp2`](https://crates.io/crates/openjp2) 0.6.1 (2025-07-17;
    BSD-2-Clause): a c2rust port with heavy `unsafe` (count unverified).
  - [`jpeg2000`](https://crates.io/crates/jpeg2000) 0.3.0: **GPL-3.0,
    out**.
  - Encoders: only OpenJPEG through the C crates or openjp2; nothing in
    safe Rust.
- **Recommendation:** **adopt hayro-jpeg2000 for decoding.** Re-encode a
  redacted JPX as Flate (lossless; PDF/A-1 doesn't allow JPX anyway); for
  M14's lossy recompression, JPEG. No JPX encoder. High (decode); medium
  (no encoder).
