# M11 proposal 9: Verification, every time

Part of [M11: Security & redaction](../README.md) · [all proposals](README.md)

**Verification, every time.**
- sheaf reopens the copy it wrote with PDFium and checks that no
  character box intersects a mark.
- When a pattern made the marks, it checks that the pattern no longer
  matches inside them.
- The report says what was removed: text runs, images edited, paths
  clipped, annotations removed, leak-check changes. It also says what
  couldn't be checked.
- A failed check stops the export and names the page.
