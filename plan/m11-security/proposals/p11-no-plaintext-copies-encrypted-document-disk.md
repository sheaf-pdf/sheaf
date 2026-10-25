# M11 proposal 11: No plaintext copies of an encrypted document on disk

Part of [M11: Security & redaction](../README.md) · [all proposals](README.md)

**No plaintext copies of an encrypted document on disk.** Mostly met
by M10.5: generated sources are memory blobs (the generated cache is
gone), and the recovery sidecar is encrypted. What's left is the
attachments folder (slice 1).
- Its generated sources (crops, watermarks, stripped copies) stay in
  memory as `SourceData`; slice 4 of M9 already keeps every source's
  bytes there.
- Its recovery section is encrypted with the document's key.
- Temporary files are deleted when the document closes: the
  recovered copy, and attachments opened from the panel.
