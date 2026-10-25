# M10 slice 16: Form data: FDF and XFDF

Status: open · Part of [M10: Forms](../README.md)

**Form data: FDF and XFDF** (proposal 17; bug 11).
- **On the edit API:** importers produce `SetFieldValues` batches by
  name, resolved through calculation and committed `Validate`, as an
  import job; exporters are queries over a snapshot. Both sit beside
  comments in `sheaf_doc::exchange`.
- FDF `/Fields` and XFDF `<fields>`, read and written beside
  comments in the same files, values only; import from another
  filled PDF.
- Forms > Import Data…, Export Data > XFDF… and FDF….
- `sheaf form fields`, `export`, `import`, `fill` and `reset`.
- **Checks:** every field type round-trips through FDF and XFDF;
  unmatched names are reported; an FDF's scripts, flag changes and
  `/F` are reported and ignored; Import Data adds no comments; a fuzz
  target for both readers; `sheaf-cli` tests run the subcommands.
