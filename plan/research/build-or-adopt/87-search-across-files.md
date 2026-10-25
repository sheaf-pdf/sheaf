# Build or adopt 87: Search across files

Area: Pending · Part of [build or adopt](README.md)

- **Need:** `sheaf search` over files and folders, and later a GUI.
- **Where:** `ROADMAP.md:817-837`.
- **Today's plan:** "Each file opens as a `Session` and runs
  `Text::search` … Pages with no text are counted, not OCR'd. Encrypted
  files are listed and skipped unless a password is given" (`:825-829`).
- **Candidates:** for walking,
  [walkdir](https://crates.io/crates/walkdir) 2.5.0 (2024-03-01;
  Unlicense OR MIT; 711M downloads) or
  [ignore](https://crates.io/crates/ignore) 0.4.33 (2026-08-04). For an
  index, if ever: [tantivy](https://crates.io/crates/tantivy) 0.27.0
  (2026-10-17; MIT; 16k stars; pure Rust, heavy), or SQLite FTS5 through
  [rusqlite](https://crates.io/crates/rusqlite) 0.40.2 (MIT; C SQLite
  bundled).
- **Fit:** the CLI needs no index; regex and Patterns are in place. An
  index of client PDFs would be a plaintext copy of their text on disk,
  which runs into M11's "No plaintext copies of an encrypted document on
  disk" (`m11.md:229`), and it's client data at rest besides.
- **Recommendation:** **build in-house** on `Session`, with walkdir (or
  `std::fs`). Defer any index; if the GUI needs repeat searches over one
  client's folder, decide between tantivy and FTS5 then, with the index's
  encryption and location as the deciding constraints. High for the CLI.
