# M18 slice 15: Wrap-up

Status: open · Part of [M18: the PDF layer](../README.md) · in sheaf

**What:** docs and plan for a sheaf without lopdf.

- **Docs:** `docs/architecture.md` (the PDF layer and its crates),
  `docs/document-model.md` (it names lopdf on 47 lines, and tests read
  it), `docs/building.md`, `docs/oracles.md` where lopdf was an oracle.
- **Plan:** [F13](../../backlog/F13.md) closed; [Libraries](../../libraries.md)'
  item 2 says what's published and what waits; build-or-adopt entries
  1, 2 and 29 updated; the backlog entries that named lopdf's
  behaviour rechecked.
- **pdf-lint:** if 0.2.0 or later is out, sheaf's dev-dependency moves
  to it, and `PDF_LINT_QPDF` and the macOS `OPENSSL_MODULES` step go
  ([sheaf.md](../sheaf.md#pdf-lint-without-qpdf)).
- **What's later** ([scope](../scope.md#later)) stays listed there, each
  with what it waits for.

**Checked:** `just plan-check`, `just precommit`, `just premerge`.
