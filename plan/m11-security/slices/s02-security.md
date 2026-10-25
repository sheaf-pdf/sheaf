# M11 slice 2: Security…

Status: open · Part of [M11: Security & redaction](../README.md)

**Security…** (Document > Security…; proposal 14).
- Open password and permissions password.
- Permissions: printing (none, low resolution, high resolution),
  changes (none, assemble, fill in forms, comment and fill, any),
  copying, and accessibility.
- AES-256, written with `/Length 256` (proposal 10); change or
  remove. Changing the encryption is a save that can't append (M10.5
  §9.2: "encryption hasn't changed" is an Append's condition).
- Properties shows the result.
- `sheaf protect` and `--decrypt`.
- Check: Acrobat, Chrome (PDFium) and Firefox (pdf.js) open the
  result. macOS Preview is unconfirmed for R6, so test it on a Mac.
