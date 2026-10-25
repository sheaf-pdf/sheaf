# M10 slice 10: Filling I: focus, keys and Tab order

Status: open · Part of [M10: Forms](../README.md)

**Filling I: focus, keys and Tab order** (proposals 9 and 14; bug
9).
- **On the edit API:** hit tests on widgets in PageSpace, in
  `page_annots` order, through each page's view maps (no app
  counterpart yet: M10.5 slice 5 checked the maps against PDFium's
  own). **It adds `KeyPatch` with symbolic `PatchValue`s, and
  `PageDef.keys` for `/Tabs`, with a classify role row (`/Tabs` →
  FIELD_TREE, OTHER).** The `/S` order comes from
  `materialize::kept`'s structure walker over source facts.
- The Select tool focuses fields; a field cursor.
- Tab order across pages by `/Tabs` (proposal 14); Tab with no field
  focused, and the wrap at the end.
- Enter, keypad Enter, Esc and Ctrl+Tab; the field's key context.
- **Pending commits:** commands commit the focused field first
  (proposal 9), and pending UI commits flush before any plugin call.
- Read-only fields don't take focus; required ones are marked. The
  `/TU` tooltip on hover and focus, and the name and tooltip in the
  status bar.
- `{"dump": "fields"}` for scripts.
- **Checks:** the W-9's and the 1040's Tab sequence follows their
  structure (the XFA template's order as a proxy for Acrobat's);
  Colorado's follows its rows; typing "v" in a field doesn't pick the
  Select tool; a widget on a page turned in sheaf is hit where it
  shows; a plugin command run while a field is focused sees its
  value.
