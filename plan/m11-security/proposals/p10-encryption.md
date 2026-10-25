# M11 proposal 10: Encryption

Part of [M11: Security & redaction](../README.md) · [all proposals](README.md)

**Encryption.**
- Every save keeps a file's encryption exactly: rewrite, incremental,
  recovery, Save As. "Password protection removed" goes away.
  - It reuses the file's `/Encrypt` dictionary and file key, so no
    owner password is needed.
  - `/ID` doesn't change, since R2–R4 keys depend on it.
- **Kept on every save since M10.5** (slices 7, 9 and 10):
  `serialize/crypt.rs` writes each object with the file's key, and
  the recovery sidecar is encrypted as the file is.
- New encryption is AES-256 (R6) only. PDF 2.0 deprecates RC4 and
  AES-128.
- **New AES-256 encryption (V5, R6) writes `/Length 256` in
  `/Encrypt`,** in M11's encryption code or as a `vendor/lopdf`
  patch: lopdf's `Document::encrypt` leaves it out, and poppler 26.01
  then garbles every stream. Its tests check new AES-256 output with
  poppler (M10.5 slice 2's notes).
- All Standard-handler revisions can still be read.
- Encryption is removed only through Security…, or `--decrypt` in
  the CLI.
