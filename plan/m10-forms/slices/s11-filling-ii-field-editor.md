# M10 slice 11: Filling II: the field editor

Status: open · Part of [M10: Forms](../README.md)

**Filling II: the field editor** (proposals 9, 11 and 14; bug 11).
- **It adds `GroupKind::Typing`:** one group per focus session, each
  commit a `SetFieldValues` amend into it (M10.5 §8.4). The editor's
  text stays uncommitted, with its own Ctrl+Z. `ViewInputs` hides the
  focused widget from the realization while the editor draws it.
- A `FieldEditor` element in sheaf-ui, over the widget at the page's
  zoom. It lays text out with slice 6's code (alignment, comb cells,
  auto size, wrapping, the `DoNotScroll` limit) and paints it by
  proposal 11's editor-font rule: Liberation Sans, Serif or Mono for
  what the field's font encodes, Noto Sans for what the appearance
  draws in the fallback.
  A hook before each change can veto, replace or limit it, and
  consults that layout (`MaxLen` now, scripts later).
- On a rotated page or a `/MK /R` widget, an upright editor beside
  the field, with a live preview.
- Password fields masked; FileSelect fields take plain text and
  never open a file dialog; Ctrl+Z and Ctrl+Y on the editor's text;
  the overflow marker; `auto_advance` (a should-have).
- The Liberation fonts' license in `packaging/licenses`.
- **Checks:** a script run that clicks, types, tabs onto the next
  page, reverts with Esc and undoes; Ctrl+S while typing saves the
  typed value; Ctrl+Z while typing changes no other field; one focus
  session's commits are one undo step; on Xwayland, the typed value
  and the saved appearance line up, right-aligned and comb fields
  included.
