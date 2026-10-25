# M10 slice 14: Highlight Fields, the Fields panel, and values in search

Status: open · Part of [M10: Forms](../README.md)

**Highlight Fields, the Fields panel, and values in search**
(proposals 6, 14 and 24; bug 8).
- **On the edit API:** the panel is model reads (`FormDoc` and its
  fields); `TextOpts` gains `fields: true`, which merges widgets'
  display text into a page's text, for the app (M10.5 §10.5; plugins
  get it with 0.5's `fields`, slice 29).
- Shift+L highlights fields, required ones in their own color (from
  `[forms]`). A bar on a fillable form, "This document has fillable
  fields", offers it.
- The Fields panel (View > Panels > Fields, Ctrl+Alt+0): every field
  with its type and value, by tab order, page or name, with a filter;
  click to go there; the overflow marker. It's for reviewing a
  client's filled form. Tooltips from `/TU`, or the XFA `<speak>`
  text read in slice 5.
- Find, Patterns and text export include field values, merged the
  way OCR words are.
- **Checks:** a filled value is found by Find and by a Patterns
  preset, and is in `sheaf export -f text`; a plugin's page text
  still leaves it out.
