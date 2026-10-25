# M10 proposal 17: **Form data: FDF and XFDF are musts; CSV, text and JSON are

Part of [M10: Forms](../README.md) · [all proposals](README.md)

**Form data: FDF and XFDF are musts; CSV, text and JSON are
should-haves.**
- **FDF** `/Fields` (read today only for `/Annots`, fdf.rs:41-47) and
  **XFDF** `<fields>` (xfdf.rs:6-7 keeps the place for it). Matched by
  full name; fields that don't match are reported; `NoExport` fields
  are left out; one undo step. With scripts on, an import
  recalculates.
- **Values only.** Import reads `/V`, and `/Opt` where it matches the
  field's. An FDF's own JavaScript (`/JavaScript` with `/Before`,
  `/After` and `/Doc`), its flag changes (`/SetFf`, `/ClrFf`,
  `/SetF`, `/ClrF`, which could unhide a field or clear ReadOnly) and
  its target file (`/F`) are reported and ignored. Import Data never
  adds comments from the same file.
- **From another filled PDF,** Acrobat's Import Data File
  (`exchange::read` already tells PDFs from FDF and XFDF).
- **Many files merged into one CSV:** a row per file, columns in tab
  order, hidden fields included, UTF-8 with a BOM. Users report that
  Acrobat's version drops hidden fields and garbles UTF-8.
- **Excel-safe,** on by default for Merge Data Files: a value starting
  with `=`, `+`, `-` or `@` gets a leading apostrophe, which import
  takes off. Staff open these files in Excel, where such a value runs
  as a formula.
- **Tab-delimited text** out, and in through a row picker.
- **JSON** for the CLI and plugins is a file format with a version:
  `{"sheaf_form_data": 1, "fields": [{name, type, value, export?}]}`,
  changed only by adding, as M9 decision 1 treats `/SHEF_Annot`, and
  documented in `docs/forms.md`.
- IRS field names are opaque (`topmostSubform[0].Page1[0].f1_01[0]`),
  so a CSV can carry each field's tooltip as a second header row,
  marked by a first cell of `#tooltip` so import can tell it from
  data.
- **`sheaf form`:** `fields`, `check`, `export`, `import`, `fill`,
  `reset`, `flatten` and `merge`, in `sheaf_ops::forms` (M9 decision
  10). A filled copy per CSV row is M15's.
- Acrobat's XML format comes later.
