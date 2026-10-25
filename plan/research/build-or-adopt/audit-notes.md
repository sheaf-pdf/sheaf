# Build or adopt: audit notes

Part of [build or adopt](README.md).

## Already acted on (2026-10-23)


- **wasmtime:** PR #33 updates wasmtime, wasmtime-wasi and
  wasmtime-wasi-http from 49.0.2 to 49.0.4 for
  [GHSA-mghf-wjfp-p5xm](https://github.com/bytecodealliance/wasmtime/security/advisories/GHSA-mghf-wjfp-p5xm)
  (critical). On aarch64, a module with a large `br_table` "could then
  jump to arbitrary other code … leading to arbitrary sandbox escapes":
  Apple Silicon Macs running any installed plugin. The core section also
  cited a second advisory fixed in the same releases,
  [GHSA-5qpp-g3j2-8frx](https://github.com/bytecodealliance/wasmtime/security/advisories/GHSA-5qpp-g3j2-8frx);
  it couldn't be verified afterwards, so it's unverified here. No backlog
  entry: #33 is the fix. Entry 100.
- **The content interpreter PR** (M11 slice 3): its wrap-up takes
  skrifa/read-fonts for tight glyph boxes (TrueType, CFF and Type 1),
  and hayro-cmap's 65 predefined CMaps, with Adobe's BSD-3 notice. It
  adds backlog F17, hayro-interpret as a second corpus oracle. Entries
  25 and 26 are adopted there.
- **lopdf, for the weekend's upstream pull requests** (Greg,
  2026-10-24/25): 0.46.0 (2026-10-21) has #598 and #603. #624, #630,
  #613 and #625 are merged since. Diff the vendored patch against
  upstream main before writing each PR. Entry 2 has the table, and the
  one discrepancy about #613.

## What the build already compiles


Adopting these adds no new crate, or only a new version. From the
sections' readings of `Cargo.lock` and `cargo tree`.
- **Fonts and text:**
  - ttf-parser (every sheaf-app build, through gpui-pre and usvg);
  - skrifa 0.40 and 0.44, read-fonts 0.37 and 0.41 (the Linux app,
    through cosmic-text and swash);
  - fontdb 0.23 (every platform: gpui-pre turns on resvg's `text` and
    `system-fonts`);
  - harfrust 0.5.2 and unicode-linebreak (Linux, through cosmic-text);
  - rustybuzz 0.20.1 (every build, through usvg; archived);
  - unicode-bidi (every platform through usvg, and Linux through
    cosmic-text);
  - unicode-normalization 0.1.25 (sheaf-doc), unicode-segmentation
    (sheaf-ui and sheaf-app), icu_normalizer 2.3.0 (through `url` and
    `idna`);
  - icu_segmenter, but only inside the boa wasm guest, not the host;
  - a Zed fork of font-kit (GPUI's Linux build).
- **Geometry:** kurbo 0.13.1 (through usvg 0.46); lyon_geom (through
  gpui-pre's lyon 1.0.19).
- **Crypto and transport:**
  - lopdf's aes 0.9.3, cbc 0.2.1, ecb 0.2.1, md-5 0.11.0, sha2 0.11.0,
    stringprep 0.1.5, rand 0.10 and getrandom 0.4;
  - sheaf-doc's hmac 0.13, sha2 0.11 and zeroize 1.9.1;
  - ring 0.17.14, hyper 1.12, tokio (1.53.2 in the lock) and rustls 0.23,
    through wasmtime-wasi and wasmtime-wasi-http;
  - security-framework 3.7.0 (in the lockfile); aws-lc-rs (in the
    lockfile, not in a normal build);
  - oo7 0.6.0 (gpui-pre-linux), on the older RustCrypto line (aes 0.8.4,
    cbc 0.1.2, md-5 0.10.6, sha2 0.10.9): compile time only;
  - uds_windows 1.2.1; windows 0.62.2 (gpui-pre-windows, accesskit) and
    0.61.3 (sheaf-instance, sheaf-print).
- **Images:** image 0.25.10, zune-jpeg 0.5.15, png 0.18, tiff 0.11.3,
  fax 0.2.7, flate2 1.1.10, weezl, imageproc 0.27, moxcms 0.8.1 (through
  image), libloading 0.8.9 and 0.9.0 (through ort, pdfium-render and
  wgpu). zopfli only as a build-time dependency of tesseract-rs.
- **App and tools:** quick-xml 0.41, schemars 1.2.2, accesskit 0.24.1
  with its adapters and accesskit_consumer 0.38, toml 0.9.12, clap 4.6.7,
  proptest 1.12.0.

## Backlog entries this audit added


In `plan/backlog/README.md`. B14–B18 (the `m5-intake` branch) and F17 (the
content interpreter PR) are held on other branches; B23 landed with #32
and B30 with #35.
- **B24:** G4 encoding truncates images wider than 65,535 pixels (entry
  38).
- **B25:** fax TIFFs (CCITT Group 3, Modified Huffman) are refused
  (entry 16).
- **B26:** the Flate backend depends on which crates are in the build
  (entry 4, D4).
- **B27:** nothing checks dependencies for advisories or licenses (D7).
- **B28:** printing may not work in the Flatpak build (entry 95).
- **B29:** search misses ligatures, compatibility forms and NFD text
  (entry 85).
- **F18:** notices for the data and code sheaf ships (entries 19, 21, 26,
  35, 63, 84).
- **F19:** wipe file keys, not only passwords (entries 29, 43).
- **F20:** upstream cms `CertificateChoices` alternatives (entry 44).
- **F21:** try PP-OCRv6 in the PaddleOCR engine (entry 13).
- **F22:** check what sheaf writes against the Arlington PDF Model
  (entry 94).
- **F23:** hayro-syntax as a second reader in the lazy-reader corpus
  test (entry 1).
- Lines added to B5, F9 and F13.
- The wasmtime bump the core section proposed as a bug isn't an entry:
  PR #33 is the fix.
