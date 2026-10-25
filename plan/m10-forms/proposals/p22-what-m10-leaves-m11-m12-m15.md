# M10 proposal 22: What M10 leaves to M11, M12 and M15

Part of [M10: Forms](../README.md) · [all proposals](README.md)

**What M10 leaves to M11, M12 and M15.**
- **M11.** Keeping encryption on save was M11's, and M10 was to warn
  until then (kickoff). Moot since M10.5: every save keeps a file's
  encryption, recovery sidecars are encrypted, and owner-only files
  get recovery, so M10 adds no notice and no warning.
  - Sanitize removes field and document scripts and offers to
    flatten, reusing slice 15. Redaction removes the widgets and
    values under a mark, and its leak check searches field values
    (slice 14 provides them). The "fill in forms" permission is
    shown, not enforced (M11 decision 2).
- **M12:** signing, the Signed tab (`/Lock`, `/SV`), certifying
  (DocMDP that allows filling), verification, and Fill & Sign for
  flat forms, which can reuse M10's detection.
- **M15:** batch fill, flatten and export, and a filled copy per CSV
  row. The forms commands get argument schemas from the start, so
  batch actions and the agent can drive them.
