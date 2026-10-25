# M10 slice 23: Prepare Form I: the mode and the tools

Status: open · Part of [M10: Forms](../README.md)

**Prepare Form I: the mode and the tools** (proposals 14–15).
- **On the edit API:** `AddField` and `AddWidget` (a new field's
  widgets name it by its local), `MoveWidgets`, `DeleteFields` and
  `DeleteWidgets`.
- The mode and its key context: F and Shift+F, P, G, the arrow
  nudges, Esc and Done.
- A tool for every type, unsigned signature fields included, with
  Acrobat's default names ("Text1", "Check Box1"…).
- Move, resize, multi-select, copy, paste and delete through M9's
  handles; a deleted widget leaves the structure tree. Signed fields
  are locked (`EditError::Locked`), and deleting one asks.
- **Checks:** one of each type made, saved and reopened in sheaf and
  PDFium, with the check clean; a W-9 field moved and resized;
  deleting a field forces a full rewrite and leaves no structure
  reference to a missing object.
