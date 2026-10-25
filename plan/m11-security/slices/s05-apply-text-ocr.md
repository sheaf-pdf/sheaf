# M11 slice 5: Apply: text and OCR

Status: open · Part of [M11: Security & redaction](../README.md)

**Apply: text and OCR** (proposal 7).
- **On the content stack** (left by slice 3): `ContentStack.rewrite`,
  the new base (setting it absorbs the current filters, and later
  filters apply to its output), and `Static::Ops`, with
  `Rewrite::Redact { areas, policy }` evaluated per page at emit as
  their first producer (M10.5 §3.4, §11.4 and §20). The new type
  and `content::Rewrite` (a marked range's property list written
  again, `without_keys`) would share a name, so one is renamed then.
- Start from slice 3's sketch for this slice (its notes).
- Remove glyphs under marks and write survivors with absolute
  positions.
- Type3 glyph uses.
- OCR layer words.
- `/ActualText` and `/Alt`; `/ToUnicode` and `/Widths` for codes no
  longer used.
- Check: no character box under a mark, and every surviving glyph
  within 0.5 pt of where it was. A test copies the PETS 2023 setup
  against our own output.
