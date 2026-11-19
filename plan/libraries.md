# Libraries

Work that's reusable on its own is published as a crate, as
`in-progress` (progress and cancellation), `instance-handoff`
(single-instance launching) and `pdf-lint` (sheaf's Arlington checker,
with the `pdf-tokenizer` it reads with; sheaf's tests check with it
since [pdf-lint](complete/pdf-lint.md)) were. A candidate stays where the
plan has it. What changes is that its API keeps sheaf's types out from
the start.
Candidates, most valuable first (2026-10-23; crate facts in
[build-or-adopt](research/build-or-adopt/README.md)):

1. **Linearization** ([between milestones](between/README.md), item
   4). Scheduled, as a crate of the PDF family
   ([M18](m18-pdf-layer/crates.md#linearization-a-crate-of-its-own)).
2. **The PDF layer** (F13's "Maybe published", now [M18](m18-pdf-layer/README.md)):
   `pdf-tokenizer`, `pdf-objects`, `pdf-filters`, `pdf-crypt`,
   `pdf-structure`, `pdf-content-streams`, `pdf-semantics` and
   `pdf-arlington` in [sheaf-pdf/pdf-lint](https://github.com/sheaf-pdf/pdf-lint)
   (MIT OR Apache-2.0 for the reusable library crates; `pdf-lint` 0.2.0-rc.1
   released as a standalone CLI binary). `sheaf` now reads and writes PDFs
   through `pdf-objects`, `pdf-structure`, `pdf-filters` (`zlib-rs`) and
   `pdf-crypt`, and `lopdf` is gone (M18 slices 1, 8–13); crates.io
   publishing of the new crates and migrating `sheaf`'s content/semantics
   modules (`s14`, `s17`, `s19`, `s21`) follow.
3. **Form scripts** (M10 slices 18–22): the AF functions and pdf.js's
   form-script library on boa, as a WebAssembly guest (entry 24). Any
   Rust program that fills PDF forms needs this.
4. **PAdES signing and LTV** (M12). No Rust crate does it; pyHanko
   (Python, MIT) is the oracle (entry 54). It waits on RustCrypto's cms
   0.3 round (D2). Plan it as a crate at M12's kickoff.

Deferred: **scanner access** (M5's `sheaf-scanner`) and **Kodak patch
codes**, with M5's scanner work (Greg, 2026-10-23: no physical scanners
to test with). No maintained Rust crate does SANE or TWAIN (entries
5–8), and none reads patch codes (entry 12), so both are candidates
again when M5 resumes.

Not candidates:
- the durable file writes (crates for atomic replacement exist);
- the plugin host (it's sheaf's command registry and UI);
- the OCR engine trait (thin over the engines' own crates).
