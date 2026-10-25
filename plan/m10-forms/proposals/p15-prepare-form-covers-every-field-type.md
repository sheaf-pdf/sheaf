# M10 proposal 15: **Prepare Form covers every field type, and editing fields in

Part of [M10: Forms](../README.md) · [all proposals](README.md)

**Prepare Form covers every field type, and editing fields in
existing PDFs.**
- Tools: Text, Check Box, Radio Button, Dropdown, List Box, Button,
  and Signature (unsigned; signing is M12). Draw, move, resize,
  multi-select, copy, paste and delete, each one undo step, reusing
  M9's handles, properties bar, and copy and paste.
- Properties, in Acrobat's tabs: General (name, tooltip, visible or
  hidden, read-only, required), Appearance (border, fill, font, size,
  text color), Options (by type: alignment, default, multiline,
  scrolling, comb with its length, check style, export values,
  items), Actions (Reset Form, Print, Go to Page, Open URL,
  Show/Hide, JavaScript), Format, Validate and Calculate.
- Format, Validate and Calculate write the AF calls Acrobat writes,
  so Acrobat reads them back as its own choices. Calculate offers
  "value is the sum, product, average, minimum or maximum of" and
  simplified field notation. Acrobat stores the notation as
  JavaScript with markers around the expression (from memory; a
  reference file is in the files wanted, after Asks).
- Fields panel: the dotted-name hierarchy, rename, sort by page, name
  or tab order.
- Tab order by row, column or structure, or by hand; Show Tab
  Numbers. Written as `/Tabs` and the `/Annots` order.
- A calculation order (`/CO`) dialog.
- Align, center, distribute and same size; snapping to the grid and
  guides, generalized from M9's measurement snapping
  (measure_tools.rs:66-112).
- Should-haves: Create Multiple Copies (`name.0`, `name.1`…) and
  Duplicate Across Pages (same name, so the value is shared).
- Deleting a field forces a full rewrite, as deleting anything does,
  asks first on a signed file (proposal 7), and prunes its widgets
  from the structure tree as flatten does (proposal 18).
- Left out: image fields, page templates, rich text, barcodes and a
  document JavaScript editor (Not in M10).
