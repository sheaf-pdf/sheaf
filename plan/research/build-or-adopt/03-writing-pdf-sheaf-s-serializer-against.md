# Build or adopt 3: Writing PDF: sheaf's serializer against pdf-writer and krilla

Area: Cross-cutting: the PDF object layer, writing, and Flate · Part of [build or adopt](README.md)

- **Need:** write built and copied objects, as an Append or a full file,
  in the base's xref kind, encrypted object by object, byte-faithfully.
- **Where:** M10.5 slices 7, 9 and 10 (built); M14 Optimize and PDF/A;
  M17 tagging.
- **Today's plan:** "sheaf's own (from incremental.rs's writer), with
  append and full framings, the base's xref kind, a real `/Prev`,
  per-object encryption and fsync … Rejected: lopdf's
  `IncrementalDocument`" (`plan/complete/m10.5-edit-api/decisions/README.md`).
- **Candidates:**
  - [pdf-writer](https://crates.io/crates/pdf-writer) 0.15.0
    (2026-05-27; MIT OR Apache-2.0; Typst's; 6.2M downloads;
    `forbid(unsafe)`; builds for wasm). Typed step-by-step writers; xref
    streams (`finish_with_xref_stream`). Its numbers are f32
    (`buf.rs:51` `push_float(f32)`), and no object-stream builder was
    found.
  - [krilla](https://crates.io/crates/krilla) 0.8.2 (2026-06-04; MIT OR
    Apache-2.0; LaurenzV; 3.1M downloads; `typst-pdf` 0.15.1 builds on
    it). New documents only, with tagging, PDF/A-1 to 4 and PDF/UA-1.
    Encryption and signatures are "out-of-scope" (README).
  - `hayro-write` 0.8.0: "an internal crate and not meant for external
    use" (README). It turns existing pages into XObjects for pdf-writer.
- **Fit:**
  - Neither crate can write a copied object unchanged (f32), append to
    a file, or encrypt.
  - The new files sheaf makes (B8's comment summary, `tape_pdf`, scans to
    PDF, the print PDF) already go through sheaf's Session and emitter
    (M10.5 slice 19). A second writer would break the one-serializer
    rule.
  - **Worth taking as references:**
    - krilla's `crates/krilla/src/configure/PDF_A1.md` to `PDF_A4.md`
      and `PDF_UA1.md`, which list each standard clause by clause, each
      marked enforced, documented, not applicable or not enforced. With
      them, `validate.rs` (47 KB), which enforces the rules as it
      writes, and its CI checks against veraPDF and the Arlington model
      (README, "Testing");
    - pdf-writer's `structure.rs` (81 KB of typed StructElem, role-map
      and namespace writers) and `attributes.rs`, for key names in M14
      and M17.
- **Recommendation:** **build in-house** (settled): keep `serialize/`,
  adopt neither. Model M14's PDF/A and M17's PDF/UA rules on krilla's
  clause tables. High.
