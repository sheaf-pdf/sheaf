# M10: what the finished slices recorded

Part of [M10: Forms](../../m10-forms/README.md).

content in the model, semantic edits that reduce to primitives, one path
for crop, watermark, header/footer, OCR layers, flatten and the exports).
Slices 6+ are built on it once it lands. What slices 3–5 left for it:
- **Derived copies** (`derived_copy`, `SourceInfo::derived`): a crop,
  watermark or header/footer change makes a new PDF per source only to
  change content, then a save routes everything else (the page object,
  `/Annots`, fields, structure parents) back to the original page. Content
  in the model would make that routing unnecessary.
- **`removes_content` can't tell a crop from a removal:** any page shown
  from another source forces a full rewrite, though a crop removes
  nothing; Remove Header/Footer and Remove Watermark really do.
- **The form merge needs per-copy identity:** `Field::copy` and
  `Field::under` stand in for "which instance of which file's form", and
  both writers key their copy maps by (source, copy). The rename is decided
  at apply time and kept in the model, not the edit.
- **`rebase_to_saved` only resets fields** (copy 0, no parent, the new
  primary); their objects in a rewritten file are stale until slice 9's
  re-binding.
- **Two writers with the same page logic** (`build_rewrite`,
  `incremental::build`), each now with derived pages, own objects for
  copies, the form merge and the form rules. The incremental writer can't
  take anything out, so removals, changed hybrid forms and signed-file
  appends each special-case it (`Mode`).
- **Notices** come from `EditOutcome` fields read by each caller
  (`forms_renamed`, `signatures_in`); Combine's can't reach the window it
  opens.

Paused here (2026-10-15): Greg added M10.5, a redesigned edit API (page
