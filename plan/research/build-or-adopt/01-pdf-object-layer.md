# Build or adopt 1: The PDF object layer (F13)

Area: Cross-cutting: the PDF object layer, writing, and Flate · Part of [build or adopt](README.md)

- **Need:** read objects from untrusted files lazily, under caps, with
  byte spans; an owned, mutable object model that keeps reals as f64 and
  dictionary order, so copied objects write back unchanged; decode
  streams.
- **Where:** F13 (`plan/backlog/README.md`), F15 (`:775-793`);
  `vendor/lopdf/SHEAF.md`.
- **Today's plan:** "Bring what sheaf uses of lopdf into sheaf's own
  crates and drop the dependency, while still sending upstream any fix
  found along the way" (`backlog.md:692-694`). Still lopdf's: "the object
  model; the object parser and cross-reference reading; object streams;
  stream decompression; encryption…; the eager `Document`…; its writer"
  (`:697-706`). "Deferred (Greg, 2026-10-22): revisit after more
  milestones" (`:756`). No move during a milestone.
- **Candidates:**

  | Crate | Latest | License | Maintenance | Rust or FFI, unsafe | wasm |
  |---|---|---|---|---|---|
  | [lopdf](https://crates.io/crates/lopdf) | 0.46.0, 2026-10-21 | MIT | 56 releases since 2016; 26.0M downloads; about 25 PRs merged 2026-10-12 to 23 (J-F-Liu, yhay81, LuisFigueroaG) | Pure Rust; `#![forbid(unsafe_code)]` (0.46.0 `src/lib.rs:2`) | Builds (a `wasm_js` feature for getrandom; unverified beyond that) |
  | [pdf](https://crates.io/crates/pdf) (pdf-rs) | 0.10.0, 2026-03-02 | MIT | 23 releases; 635k downloads; one active maintainer (s3bk), commits 2026-10-13 | Pure Rust; some `unsafe` (`pdf/src/any.rs`) | unverified |
  | [hayro-syntax](https://crates.io/crates/hayro-syntax) | 0.8.0, 2026-10-04 | Apache-2.0 OR MIT | 11 releases since 2025-06; 5.1M downloads; LaurenzV (of Typst); used by `typst-library`, `hayro-interpret`, `hayro-write` | Pure Rust; no `unsafe` unless its `unsafe` feature is on, besides smallvec's | `no_std` with alloc, so it should build (unverified) |
  | [pdf_oxide](https://crates.io/crates/pdf_oxide) | 0.3.78, 2026-09-08 | MIT OR Apache-2.0 on crates.io; README says "MIT licensed"; GitHub detects Apache-2.0 | 83 releases in 11 months; 1.35M downloads; 1,367 commits from one author | Pure Rust core, optional aws-lc crypto; about 96 dependency lines | Has a wasm package |
  | [oxidize-pdf](https://crates.io/crates/oxidize-pdf) | 5.5.1, 2026-10-23 | MIT now; 16 releases were GPL-3.0 and 20 AGPL-3.0 | 119 releases in 15 months, majors 1 to 5 | Pure Rust | unverified |
  | [qpdf](https://crates.io/crates/qpdf) (qpdf-rs) | 0.3.7, 2026-09-27; `vendored` builds qpdf 12.4.2 | Bindings MIT/Apache-2.0; [qpdf](https://github.com/qpdf/qpdf) Apache-2.0 | ancwrd1 alone; 58k downloads. qpdf: 5.5k stars, 12.4.2 on 2026-09-27 | FFI to C++ through `qpdf-c.h`; unsafe throughout | No; F12 plans a separate wasm build |
  | zpdf-parser, pdfpurr | 0.16.0 (2026-10-16), 0.4.0 (2026-03-20) | MIT; MIT OR Apache-2.0 | First published 2026-06 and 2026-03; under 3k downloads | Pure Rust | unverified |

- **Fit, part by part:**
  - **Object model.** None of these is an owned model sheaf could edit.
    - lopdf 0.46.0 still has `Object::Real(f32)` (`src/object.rs:83`).
      sheaf's f64 patch stays: SHEAF.md counts 55,861 reals in 578
      files that a round trip through f32 changed.
    - pdf-rs has `Primitive::Number(f32)` (`pdf/src/primitive.rs:21`);
      its README: "Modifying and writing PDFs is still experimental".
    - pdf_oxide keeps a dictionary as `HashMap<String, Object>`
      (`src/object.rs:23`), so key order is lost.
    - hayro-syntax's objects are borrowed (`Object<'a>`) and read-only:
      "you cannot directly use it to manipulate PDF files" (README).
    - In-housing means lopdf's own `Object`, `Dictionary` and `Stream`
      with the `sheaf:` changes. That keeps the API at the 446 lines in
      182 files that name `lopdf::` today (F13 says "about 400 import
      sites").
  - **Parser, cross-references and repair.**
    - hayro-syntax reads every xref form, xref streams, object streams,
      repairs broken tables, decodes every filter, and decrypts with a
      password (`Pdf::new_with_password` in 0.8.0; its README still says
      password-protected files aren't supported, which the code
      contradicts).
    - Its gaps: it needs the whole file as one slice (`PdfData` is an
      `Arc<dyn AsRef<[u8]>>`, so a memory map works, but there are no
      windowed reads like `FileBytes`). Caps like `ReadLimits` and
      per-object byte spans are unverified. Its crypto is hand-rolled
      (`crypto/aes.rs`, `md5.rs`, `rc4.rs`, `sha256.rs`…), and F13 keeps
      encryption on RustCrypto.
    - qpdf's repair is the best of the lot, but linking it brings C++ to
      three OSes, unsafe FFI, no wasm and qpdf's own object handles.
      sheaf already runs it as a CLI oracle.
  - **Object streams (reading):** small. `vendor/lopdf/src/object_stream.rs`
    is 339 lines, and `LazyPdf` already decodes each one once.
  - **Stream decompression:** keep [flate2](https://crates.io/crates/flate2)
    and weezl (LZW), which lopdf uses already; entry 4 names the Flate
    backend. ASCII85, ASCIIHex, RunLength and the PNG predictor are
    small. Image codecs are entry 33. (Core's note that "the image codecs
    are PDFium's" is superseded there.)
  - **Encryption:** entry 29.
  - **The eager `Document`** (the Rewrite graph, PDFs built in memory,
    `prune_objects`): F15's streaming Rewrite. No crate does a streaming
    copy with pruning over a lazy source.
  - **lopdf's writer** (`annot::tape_pdf`, tests, fixtures): move these
    to `serialize/`. Nothing to adopt.
- **F13's "Maybe published":** no published crate offers an owned, f64,
  order-keeping, lazy, capped object layer with byte spans, so a crate
  of that layer would have a real niche. The nearest are hayro-syntax
  (read-only) and lopdf (eager, f32).
- **Recommendation:** **build in-house** (F13), starting from lopdf's
  MIT code (its object model and reader, with sheaf's patch), on flate2
  and weezl. High.
  - hayro-syntax is the design model for zero-copy lazy parsing.
  - Consider it as a dev-only differential oracle beside
    `source::lazy_tests` (F23): Greg's "hayro as a test oracle", applied
    to the syntax crate. Medium that it's worth the upkeep: its repair
    differs from PDFium's, so every difference needs triage.
  - qpdf (`--check`, `--json`) stays the outside oracle; PDFium's
    `cpdf_parser.cpp` stays the reference for repair.
  - pdf-rs: no. Besides the f32 reals, its README rejects pull requests
    written with AI ("code submitted to this repository needs to be
    written by a human"), so sheaf couldn't send fixes upstream.
  - Every in-housed reader piece gets its fuzz body before it lands, as
    `source::fuzz` does now (entry 99).
