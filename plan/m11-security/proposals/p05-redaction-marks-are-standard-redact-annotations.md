# M11 proposal 5: Redaction marks are standard `/Redact` annotations

Part of [M11: Security & redaction](../README.md) · [all proposals](README.md)

**Redaction marks are standard `/Redact` annotations.**
- Written with `QuadPoints`, `IC` (fill), `OverlayText`, `DA`, `Q` and
  `Repeat`, plus a generated `/RO` and `/AP`. So Acrobat can review
  and apply sheaf's marks, and the reverse.
- A third-party `/Redact` opens as an editable sheaf redaction mark,
  not as a comment (bug 7).
- Marks also keep the `/SheafAnnot` JSON, as every sheaf mark does
  (M9 decision 1).
