# M11 slice 1: Encrypted files: what's left

Status: open · Part of [M11: Security & redaction](../README.md)

**Encrypted files: what's left** (decisions 2 and 3; proposals 10–11;
bugs 3, 5, 10 and 12). **It shrinks:** keeping encryption on every
save (proposal 10) landed in M10.5 (slices 7, 9 and 10), with the
encrypted recovery sidecar, owner-only files' recovery, sources kept
in memory and lopdf #603's workaround (bugs 2, 4, 5's cache and
temp file, 10's #603 and 11). What's left:
- Owner-only files get a one-time notice and correct Properties.
- Work around lopdf #584 for reading.
- Certificate-encrypted files get a clear refusal.
- Temp files are cleaned up: attachments opened from the panel.
- The legacy `PdfFile::open` / `save_as` (bug 12) goes, or goes
  through the save path.
- Check: an owner password on an R2–R4 file gives the right key;
  Properties shows an owner-only file's restrictions; no attachment
  opened from the panel is left in the temp folder after close; a
  certificate-encrypted file is refused with its reason; and the
  round trips this slice first planned, over M10.5's saves where its
  encrypted corpus tier doesn't already cover them: RC4-128, AES-128
  and AES-256 files saved both ways, each opened in PDFium with its
  password afterwards, owner-only files keeping their `/P`.
