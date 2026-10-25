# M13: Content editing

Status: sketched · 

Proves: The hardest Acrobat feature · Depends on: M11 (image editing)

## Summary

- Edit text in place: reflow within a text block, font matching with
  substitution when the embedded font is subset, and a clear warning when
  a glyph is missing. The font work leans on skrifa (coverage, metrics
  and outlines; the content interpreter already reads embedded programs
  with it) and subsetter for the subsets written (Greg, 2026-10-24; M10
  slice 7 settles whether subsetter's renumbering is taken).
- Add text and image objects.
- Edit, replace, move, resize, crop images.
- Link editing: create, edit destination/action, delete; create links from
  URLs in text.
- Edit existing headers/footers (beyond the M8 update/remove).

Slices: object selection + move/delete → add text/image → image edit/replace →
text edit within a line → block reflow → links.

Builds on the content interpreter (M11's slice 3, landed early), and
on M11's image redaction for image editing.

## Plan files

- No plan yet: the milestone is broken into slices when it starts.
- [Build or adopt](../research/build-or-adopt/README.md): the M13 rows, and the [changes to make](../research/build-or-adopt/changes-to-make.md) at kickoff
- Open questions: [asks](../asks.md)
