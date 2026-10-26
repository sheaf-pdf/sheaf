# M14: Document processing

Status: sketched · 

Proves: Archival and output parity · Depends on: Nothing unbuilt

## Summary

- Compare documents: text diff and visual (pixel) diff, with a side-by-side
  report.
- Optimize / reduce file size: image downsampling and recompression, font
  subsetting (skrifa to read the programs, subsetter for TrueType and
  OpenType ones where renumbering their glyphs is acceptable; bare CFF
  and Type 1, which no crate subsets, built on read-fonts:
  `plan/research/build-or-adopt/README.md` entry 62),
  removing unused objects, and Fast Web View as a choice; packing every
  file into object streams whatever its source, classic-table files
  included, and recompressing streams. A Rewrite of a file that used
  object streams packs them again ([between milestones](../between/README.md),
  done: [object streams](../complete/object-streams.md)), and the
  linearization crate is between-milestones work too (2026-10-23).
- PDF/A convert and validate ❓.
- Preflight-lite (font embedding, color spaces, transparency checks) ❓.
- Accessibility and PDF/UA moved to [M17](../m17-accessibility/README.md).

**Decide at kickoff:** how far to go on the ❓ items. They matter for
archival requirements but not for daily binder work.

**Oracles** (Greg, 2026-10-23; [docs/oracles.md](../../docs/oracles.md#by-milestone)):
veraPDF for PDF/A, and OpenJPEG for JPX images sheaf re-encodes.

## Plan files

- No plan yet: the milestone is broken into slices when it starts.
- [Build or adopt](../research/build-or-adopt/README.md): the M14 rows, and the [changes to make](../research/build-or-adopt/changes-to-make.md) at kickoff
- Open questions: [asks](../asks.md)
