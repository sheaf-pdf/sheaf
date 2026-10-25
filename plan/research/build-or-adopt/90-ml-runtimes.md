# Build or adopt 90: ML runtimes (UVDoc in use, DocRes pending)

Area: Pending · Part of [build or adopt](README.md)

- **Need:** run ONNX models: PP-OCR detection, recognition and
  orientation, and UVDoc (in use, through oar-ocr); DocRes (Pending,
  `ROADMAP.md:707-722`); layout detection for M17 tagging and the
  Markdown export (`:592-594`, `:686-705`).
- **Today:** `ort` 2.0.0-rc.13 with `load-dynamic`, loading Microsoft's
  ONNX Runtime 1.28.0 (`scripts/fetch-ocr-data.sh:18`), pinned by
  oar-ocr's `=2.0.0-rc.13` (`crates/sheaf-ocr/Cargo.toml:31-33`).
- **Candidates:**
  - [`ort`](https://crates.io/crates/ort) 2.0.0-rc.15 (2026-10-19), still
    a release candidate after 16 releases since 2024-02; MIT OR
    Apache-2.0; pykeio; 2.6k stars; FFI to ONNX Runtime (MIT).
  - [`tract-onnx`](https://crates.io/crates/tract-onnx) 0.23.9
    (2026-10-15; MIT OR Apache-2.0; Sonos; 3.1k stars): pure Rust, CPU,
    builds for wasm.
  - [`rten`](https://crates.io/crates/rten) 0.27.0 (2026-10-02; MIT OR
    Apache-2.0; Robert Knight): pure Rust, runs ocrs's models, builds for
    wasm.
  - [`candle-core`](https://crates.io/crates/candle-core) and
    [`candle-onnx`](https://crates.io/crates/candle-onnx) 0.11.0
    (2026-06-26; Hugging Face): partial operator coverage (unverified for
    these models).
  - [`burn`](https://crates.io/crates/burn) 0.22.1 and
    [`burn-onnx`](https://crates.io/crates/burn-onnx) 0.22.0 (Tracel AI):
    ONNX into Rust code at build time.
- **Fit:** a pure-Rust runtime would drop the native ONNX Runtime sheaf
  ships on each platform, and could run models inside plugins. Whether
  tract or rten has the operators and speed for PP-OCRv5 and v6, UVDoc
  and DocRes is unverified. oar-ocr is hard-wired to ort.
- **Recommendation:** **keep ort.** A switch is undecided: it would take
  a spike running PP-OCRv6 detection and recognition and UVDoc on tract
  and rten, measuring operator support, output equality and speed. High
  (keep); low (any switch).
