# M10 slice 9: Saving values

Status: open · Part of [M10: Forms](../README.md)

**Saving values** (proposals 6–9; bugs 6, 7 and 11). **It shrinks to
one commit.**
- **`SetFieldValues { values (by FieldId or full name), display,
  calculated }`,** an `Op` that lowers to `Put Field`/`Put Annot`: the
  field's value and display, its widgets' `/AS`, and the appearances
  slices 6–8 make for them. History says it ("Fill “Line 1a” on page
  1"). There's no dirty tracking of saved values: a save is an output,
  and dirty is the saved version not being the cursor (M10.5 §9.1).
- **Both strategies, recovery and identity after a save already
  exist** (M10.5 §9 and §11), so 9a's re-binding is deleted: the
  emitter writes `/V`, `/AS` and appearances by both strategies, an
  Append only the changed field and widget objects, and a recovery
  sidecar the same way. A fill clears `/RV` and keeps `/DS`; a
  password field's value isn't written to `/V`, so data export skips
  it.
- **The save rules are not per-writer code:**
  - a replaced value is REPLACES_TEXT, so a Rewrite (M10.5 proposal
    9);
  - filling an empty field appends, keeping UR3 within its
    `/TransformParams` (M10.5 proposal 34);
  - earlier-revision values are a fact of the source's form that
    forces a Rewrite on the first form change;
  - XFA (its tooltips to `/TU`, after `Ask::XfaHoldsValues`), `/UR3`,
    `/SigFlags` and `NeedAppearances` files' missing appearances are
    `forms::rules::plan` transforms (M10.5 §11.8);
  - signed and certified files go through §9.3's asks, with "Append
    anyway";
  - lowering refuses edits to fields locked by a known signature
    (FieldMDP), and to signed fields themselves, with the
    `EditError::Locked { field }` this slice adds;
  - the encryption notice is gone, because every save keeps
    encryption (M10.5).
- **Checks:**
  - the 1040 and W-9 filled through `SetFieldValues`, saved both ways
    and reopened: values read back, the check is clean, and PDFium
    shows them with appearances only; after a fill, `/XFA` and
    `/Perms` are gone; a replaced value is nowhere in the bytes;
  - fill, full rewrite, fill another field, incremental save,
    reopen: both values, one object per field; the same after Save
    As, and after undoing past a save then saving;
  - a fixture built in code (fill, save incrementally, clear, save
    incrementally, fill another field) has no trace of the first
    value after the next save;
  - a fill on a signed hybrid fixture offers "Append anyway" and
    doesn't rewrite unasked; a fill of a field a signature locks is
    `Locked`;
  - a fillable fixture with only a permissions password, and one with
    an open password, keep their encryption through a fill saved both
    ways and through recovery;
  - undoing back to the saved version makes the next save
    incremental again.
