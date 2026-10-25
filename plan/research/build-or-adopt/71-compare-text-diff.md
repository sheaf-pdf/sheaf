# Build or adopt 71: Compare: text diff

Area: M14: Document processing · Part of [build or adopt](README.md)

Both the images and core sections; the images section's detail stands,
and core's benchmark is the check.
- **Need:** a word-level diff of two documents' text, with positions so
  changes can be drawn; page matching between versions; bounded at
  7,000 pages.
- **Where:** `ROADMAP.md:434-435` ("Compare documents: text diff and
  visual (pixel) diff, with a side-by-side report"; no crate named).
- **Candidates:**
  - [`similar`](https://crates.io/crates/similar) 3.2.0 (2026-08-17;
    Apache-2.0; mitsuhiko; 228M downloads; no `unsafe`): word, character
    and grapheme tokenizers; Myers, patience and LCS; **deadline
    variants** ("algorithms periodically check it and may fall back",
    `algorithms/mod.rs:51-92`); wasm support (`wasm32_web_time`).
  - [`imara-diff`](https://crates.io/crates/imara-diff) 0.2.0
    (2025-06-14; Apache-2.0; 35M downloads): faster, lower-level
    (interned tokens), 10 `unsafe`; used by gitoxide (unverified).
  - [`dissimilar`](https://crates.io/crates/dissimilar) 1.0.11
    (2026-03-15; Apache-2.0; dtolnay): character-level diff-match-patch
    with semantic cleanup. [`diff`](https://crates.io/crates/diff)
    0.1.13 (2022): stale.
- **Fit:** similar's deadline keeps the work bounded, and the same crate
  can match pages by diffing a sequence of page fingerprints. Page
  matching and the report are sheaf's.
- **Recommendation:** **adopt similar;** imara-diff if 7,000-page runs
  are too slow, decided by a benchmark at M14's kickoff. High (images);
  medium (core, pending the benchmark).
