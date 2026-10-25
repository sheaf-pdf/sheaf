# Build or adopt 37: JBIG2 decode

Area: M11: Security and redaction · Part of [build or adopt](README.md)

- **Candidates:**
  - [`hayro-jbig2`](https://crates.io/crates/hayro-jbig2) 0.3.1
    (2026-10-04; four releases since 2026-01; MIT OR Apache-2.0;
    LaurenzV). `forbid(unsafe_code)`; `simd` through `fearless_simd`
    only for halftones; `no_std`. `Image::new_embedded(data, globals)`
    takes PDF's `/JBIG2Globals`. Its README: "This crate is currently
    under development."
  - [`jbig2dec`](https://crates.io/crates/jbig2dec) 0.3.1 (2020): the
    crate is GPL-3.0 and jbig2dec itself AGPL. **Out.**
  - Encoders: [`jbig2enc`](https://crates.io/crates/jbig2enc) 0.1.2
    (Apache-2.0, 301 downloads) and
    [`jbig2enc-rust`](https://crates.io/crates/jbig2enc-rust) 0.5.4 (MIT
    OR Apache-2.0, 530 downloads), both first released in 2026: too
    young. Lossy symbol matching is also what swapped digits in Xerox
    scanners' PDFs (2013), unacceptable on tax documents.
- **Fit, and a redaction detail:** `/JBIG2Globals` is a symbol dictionary
  several images can share; 15 of the corpus's 127 JBIG2 scans have one.
  Re-encoding the redacted image as G4 leaves the globals in the file
  whenever another image still points at them, and their symbols can
  include glyphs only the redacted area used.
- **Recommendation:** **adopt hayro-jbig2 for decoding; re-encode as G4;
  never encode JBIG2.** Drop a globals stream nothing else refers to;
  otherwise decide whether keeping it leaks, or re-encode every image
  that shares it (D5). Medium (the crate's maturity).
