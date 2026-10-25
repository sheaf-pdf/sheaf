# Build or adopt 10: Barcode reading

Area: M5: separator sheets · Part of [build or adopt](README.md)

- **Need:** read QR, Data Matrix, Code 39, Code 128 and PDF417 on page
  images at 150–200 dpi in 1-bit, with no false reads on real scans.
- **Where:** M5 separators (`ROADMAP.md:131`; `m5-intake.md` proposals
  14–15, ask 7); F16 (QR codes into links, a wasm plugin).
- **Today's plan:** "**rxing, behind a small trait in
  `sheaf_image::codes`**… the trait lets zxing-cpp in later"
  (`m5-intake.md:1243-1247`).
- **Candidates:**
  - [`rxing`](https://crates.io/crates/rxing) 0.9.3 (2026-09-16, the
    latest; four releases in 2026; Apache-2.0; hschimke alone; 736k
    downloads). Pure Rust, no `unsafe` in `src/`.
    [rxing-wasm](https://github.com/rxing-core/rxing-wasm) targets
    wasm32-unknown-unknown; wasm32-wasip2, which F16 needs, is
    unverified. Its `image` feature pins `image = "=0.25.8"` and pulls
    `imageproc ^0.25` (sheaf is on 0.27); `default-features = false`
    drops both, as the plan says.
  - [`zxing-cpp`](https://crates.io/crates/zxing-cpp) 0.5.3 (2026-07-29;
    Apache-2.0; 33k downloads): builds C++20 with CMake. FFI; not for
    wasip2.
  - [`rqrr`](https://crates.io/crates/rqrr) 0.11.0 (2026-09-02; (MIT OR
    Apache-2.0) AND ISC): QR only.
  - [`zbar-rust`](https://crates.io/crates/zbar-rust) 0.1.1 (LGPL-2.1)
    and [`zedbar`](https://crates.io/crates/zedbar) 0.7.0 (2026-10-02,
    LGPL-3.0-or-later): copyleft, so prior art only.
  - [`bardecoder`](https://crates.io/crates/bardecoder) 0.5.0 (2023): QR
    only, stale.
- **Fit:** as the plan's benchmark found; nothing has changed. For F16,
  read QR only: reading every format finds false codes. F16's host
  `barcodes` interface beside `ocr` (`backlog.md:978-980`) would let
  plugins reuse M5's reader.
- **Recommendation:** **adopt behind a trait** (rxing first, zxing-cpp
  second), as planned. High.
