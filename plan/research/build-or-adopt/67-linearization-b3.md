# Build or adopt 67: Linearization (Fast Web View) and B3

Area: M14: Document processing · Part of [build or adopt](README.md)

- **Need:** write linearized files as an option on a full rewrite (page
  1's objects first, the linearization dictionary, hint tables); report
  linearization truthfully after an Append (B3).
- **Where:** `ROADMAP.md:438-454`; B3 (`backlog.md:90-121`).
- **Today's plan:** "A writer option, beside object streams … Not an edit
  and not a transform plugin … Checked with `qpdf
  --check-linearization`" (`ROADMAP.md:442-454`).
- **Candidates:** no Rust crate writes linearized files; lopdf 0.46's
  `SaveOptions.linearize` is only a flag (nothing in `src/` acts on it).
  qpdf (`--linearize`; C++, Apache-2.0) does; locally qpdf is 12.3.2,
  the latest 12.4.2 (2026-09-27).
- **Fit:** qpdf's `QPDF_linearization.cc` and `QPDFWriter` (Apache-2.0,
  readable), with ISO 32000-1 Annex F, are the reference.
- **Recommendation:** **build in-house** on the serializer, as planned;
  test with `qpdf --check-linearization` on Linux CI, where qpdf is a
  test tool already. B3's fix is sheaf's own (`/L` against the file
  length). High that no crate exists; medium on size (hint tables are
  the bulk).
