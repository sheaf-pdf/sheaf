# M10 slice 17: Form data: CSV, text and JSON

Status: open · Part of [M10: Forms](../README.md)

**Form data: CSV, text and JSON** (proposal 17).
- **On the edit API:** as slice 16's: `SetFieldValues` batches by
  name, committed `Validate`; exports are queries.
- Forms > Merge Data Files… into one CSV, Excel-safe by default;
  tab-delimited text out, and in through a row picker; the versioned
  JSON both ways; the `#tooltip` row.
- `sheaf form merge`.
- **Checks:** three filled W-9s merge into three rows, with columns
  in tab order, hidden fields included and a BOM; a value starting
  with `=` is prefixed and imports back unchanged; a text row imports
  into the right fields; JSON round-trips; the tooltip row isn't read
  as data.
