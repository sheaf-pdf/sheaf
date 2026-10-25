# Build or adopt 82: PDF/UA validation

Area: M17: Accessibility (and M7's text with layout) · Part of [build or adopt](README.md)

- **Need:** check PDF/UA's machine-checkable rules on a file, and support
  a review view for the human ones.
- **Where:** `ROADMAP.md:576-602`; F9.
- **Today's plan:** "The Matterhorn Protocol lists PDF/UA's failure
  conditions; veraPDF validates both standards, and is the reference to
  compare against" (`:581-584`).
- **Candidates:** veraPDF 1.30 (`ua1`, `ua2`, `wt1r`, `wt1a`; entry 68).
  The Matterhorn Protocol: 31 checkpoints and 136 failure conditions, 89
  that software can assess and 47 that need a person
  ([PDF Association](https://pdfa.org/?p=2162)); its edition is for
  PDF/UA-1, and a UA-2 edition is unverified. PAC (axes4): Windows
  freeware, closed, not usable in CI (a CLI is unverified). pdf_oxide's
  `src/compliance/pdf_ua.rs` (28 KB, not reviewed). oxidize-pdf makes no
  PDF/UA claim.
- **Fit:** as with PDF/A, no Rust validator is close to veraPDF. The
  human checks need sheaf's own review view.
- **Recommendation:** **build in-house** for the checks covering what
  sheaf writes and for the review view; **veraPDF in tests** (`ua1`,
  `ua2`) over `conformance/`. Medium. Whether it shares M14's validator
  ("(open)" in the dependency table) depends on whether M14 builds one; a
  common rule-table format, krilla style, serves both.
