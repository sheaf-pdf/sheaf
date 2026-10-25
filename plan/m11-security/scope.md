# M11: Scope

Part of [M11: Security & redaction](README.md).


The roadmap's first cut, plus what planning turned up:

- **Redaction:**
  - Mark areas, text or whole pages.
  - Export a copy with **true content removal**: text, OCR text, images,
    line art, and annotations under the marks. Optional fill color and
    overlay text.
- **Search and redact:** Patterns hits, word lists and OCR text become
  redaction marks, covering whole words or the characters you choose,
  as Acrobat does.
- **Remove hidden information:** metadata, attachments, comments, form
  fields, hidden layers, content outside the crop box, links, actions and
  JavaScript, search indexes, thumbnails, and sheaf's own private data.
  Export Clean Copy becomes one preset of it.
- **Encryption kept on save.** Done in M10.5 (slices 7, 9 and 10): every
  save, Append, Rewrite, Save As and recovery sidecar, keeps the file's
  encryption. What's left is slice 1's.
- **Passwords and permissions:** set, change and remove. New encryption is
  AES-256.
- **Moved out:**
  - Certificate encryption moves to after M12 (decision 3).
  - The leak through deleted pages is fixed in M9 (decision 1).
