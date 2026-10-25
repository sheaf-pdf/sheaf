# M11 slice 10: Plugin API 0.6, and docs

Status: open · Part of [M11: Security & redaction](../README.md)

**Plugin API 0.6, and docs** (proposal 15).
- WIT 0.6 (one world, exact-minor; 0.5 not served): the redaction
  mark kind, and read-only security facts (is it encrypted, the
  permissions). Placing redaction marks is an op, `Cap::Plugin`;
  applying them is an export's, which no plugin can make.
- User docs, `docs/redaction.md`:
  - what's removed;
  - the limits: a gap shows the length, shadings, overlapping
    objects, and font subsets still holding outlines of removed
    characters until M14 re-subsets fonts.
- README rows; ROADMAP marked done.
