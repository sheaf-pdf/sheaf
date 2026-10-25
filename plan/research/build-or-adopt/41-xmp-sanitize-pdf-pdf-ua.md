# Build or adopt 41: XMP: sanitize (M11), PDF/A and PDF/UA (M14, M17)

Area: M11: Security and redaction · Part of [build or adopt](README.md)

Two sections, two needs, and a split recommendation (D12).
- **Needs:**
  - **M11 sanitize** (security): remove a file's XMP (the catalog's
    `/Metadata`, and page, image and Form XObject `/Metadata`), or write a
    minimal fresh packet that keeps PDF/A and PDF/UA claims
    (`pdfaid:part`/`conformance`, `pdfuaid:part`) and sets Producer and
    ModifyDate. Read the claims first.
  - **M14 and M17** (core): create or update the catalog's `/Metadata`:
    `pdfaid:part` and `conformance`, `pdfuaid:part` (and `rev` for
    UA-2), `dc:title` agreeing with Info, dates and Producer, keeping the
    other schemas a file has.
- **Where:** M11 slice 9 (proposal 12, bug 9); M14 PDF/A; M17 PDF/UA;
  Pending "Document properties and the initial view"
  (`ROADMAP.md:891-897`); B8 (closed by #35: the comment summary now
  writes its `/Info` title, Producer and CreationDate, with no op for
  document info yet).
- **Today's plan:** "metadata (Info, XMP, `/PieceInfo`)" (`m11.md:242`);
  bug 9: "`/Info`, XMP and `/PieceInfo` kept as they were, with `ModDate`
  and `Producer` never updated" (`:374-375`). The plan doesn't say what
  happens to a PDF/A file's claim. Nothing beyond mentions for M14.
- **Candidates:**
  - [quick-xml](https://crates.io/crates/quick-xml) 0.42.0 (2026-08-22;
    MIT; tafia, Mingun, dralley; 492M downloads; pure Rust). **sheaf-doc
    uses 0.41** for XFDF, XFA and the XMP conformance claims.
  - [xmp-writer](https://crates.io/crates/xmp-writer) 0.3.3 (2026-04-21;
    MIT OR Apache-2.0; typst's laurmaedje and reknih; 4.6M downloads; 6
    releases; no dependencies, no `unsafe`). Write-only, "a simple API to
    write XMP metadata": `pdfa_part`, `pdfa_conformance`, `pdfua_part`,
    `producer`, `modify_date` (`src/lib.rs:664-751`).
  - [xmp_toolkit](https://crates.io/crates/xmp_toolkit) 1.12.1
    (2026-03-19; MIT OR Apache-2.0 bindings; Adobe; 233k downloads):
    Adobe's **C++ XMP Toolkit SDK (BSD-3-Clause), with expat (MIT) and
    zlib bundled**, built with `cc`. 2.1 MB; a Windows CRT feature; no
    wasm.
- **Fit:**
  - Sanitizing deletes, or writes a fresh minimal packet; quick-xml plus
    xmp-writer cover both (security).
  - Conversion must update existing packets (Acrobat and Word put their
    own namespaces in them, which a conversion shouldn't drop), and the
    PDF/A and PDF/UA properties are a few dozen lines of XML. Adopting
    xmp-writer only for new packets means two code paths (core).
  - xmp_toolkit is C++ for a round-trip editor sheaf doesn't need.
- **Recommendation:** security: **build on quick-xml to read and
  xmp-writer to write** (high). Core: **build in-house on quick-xml**
  (read, patch, write), with xmp-writer's schema coverage as a checklist
  (medium). Both: not xmp_toolkit. This memo recommends core's single
  path (D12). Either way the plan needs a line on PDF/A claims.
