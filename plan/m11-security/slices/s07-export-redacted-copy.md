# M11 slice 7: Export Redacted Copy

Status: open · Part of [M11: Security & redaction](../README.md)

**Export Redacted Copy** (File > Export Redacted Copy…; decision 4;
proposals 8, 9 and 16).
- **On the edit API:** applying is a `Transform` on an `ExportTx`
  (`ApplyRedactions`, taking a `PageSel` so Preview lowers only the
  visible pages), so plugins can't apply redactions by type; the leak
  check runs over the output (proposal 8).
- The dialog, the `_Redacted` name, opening the copy, the report, the
  leak check, and the verification.
- Guard rails on every way out of the binder: a gate of the export
  preflight (M10.5 §12.1) and of Save As.
- `sheaf redact`.
