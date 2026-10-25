# M10 proposal 14: The Select tool fills; Prepare Form is a mode with its own keys

Part of [M10: Forms](../README.md) · [all proposals](README.md)

**The Select tool fills; Prepare Form is a mode with its own keys.**
- With the Select tool, a click on a field focuses it. With a markup
  tool, a click still places a mark, as in Acrobat. Today any click
  on a field starts a text selection (bug 9).
- While a field has focus, keys are the field's. Letters never pick
  tools and Space never scrolls: the field's key context is left out
  of the document's single-key bindings, as `TextInput` is
  (main.rs:429).
- Keys, Acrobat's first (M9 decision 13). Tab, Shift+Tab, Ctrl+Tab,
  Shift+L, F, Shift+F and Ctrl+Alt+0 are free. Arrows and Space are
  DocView keys left out only for `TextInput` and `TextArea`
  (main.rs:464-469), and Enter and Esc are Find's in every DocView
  context (main.rs:545, :548), so the field's key context binds them
  itself.

  | Key | Does | From |
  |---|---|---|
  | Tab, Shift+Tab | commit, and go to the next or previous field, across pages; with no field focused, the first on the visible page; wraps at the end | Acrobat |
  | Enter | commit a single-line field; a new line in a multiline one; toggle a check box | Acrobat |
  | Keypad Enter | commit and leave the field | Acrobat |
  | Space | toggle a check box or radio button | Acrobat |
  | Arrows | previous or next radio button in a group | Acrobat |
  | Alt+Down | open a combo box's list | Windows |
  | Esc | revert the field and leave it | Acrobat |
  | Ctrl+Tab | a tab character in a text field | Acrobat |
  | Ctrl+Z; Ctrl+Y or Ctrl+Shift+Z | undo or redo typing in the field (proposal 9) | sheaf |
  | Shift+L | Highlight Fields | Acrobat |
  | F | Prepare Form, with the Text Field tool | Acrobat |
  | Shift+F | cycle the field tools | Acrobat |
  | Ctrl+Alt+0 | Fields panel (the last free panel key) | sheaf |

  Inside Prepare Form, in its own key context: P preview, G guides,
  L R T B align left, right, top or bottom, H V align horizontally or
  vertically, Shift+H and Shift+V center, Shift+N tab numbers, arrows
  nudge 1 pt and Shift+arrows 10 pt, and Esc or Done to leave.
- **A stays Arrow.** Acrobat uses A to enter and leave form editing
  (`docs/prior-art/README.md`: "Revisit at M10"), but its single keys
  are off by default there, and F enters the mode here.
- **Tab order** follows each page's `/Tabs`. `/S` (structure order)
  is on every page of 29 of the 40 files (26 IRS forms, CA 540, CA
  3519, VA 760), so it walks the structure tree, from each OBJR to
  its widget in `/K` order. `/R` (Colorado), `/C` and `/W` as named.
  With no `/Tabs`, or a missing or broken tree, the `/Annots` order.
- **Help while filling:**
  - the field's `/TU` tooltip on hover and focus, and its name and
    tooltip in the status bar (the only accessible name a field has);
  - a marker in the editor and the Fields panel when a value is cut
    off when printed (Acrobat's "+");
  - opt-in `[forms] auto_advance` (a should-have): moves to the next
    field when a `MaxLen` field fills, and spreads a paste across
    consecutive one-character fields in tab order. NJ-1040 has 740
    of them, and no `/TU`.
- **A Forms menu** between Tools and Plugins, as Acrobat's classic
  menu and Foxit's and PDF-XChange's Form tabs: Highlight Fields,
  Clear Form…, Import Data…, Export Data, Merge Data Files…, Flatten
  Fields…, Run Scripts, Recalculate, Script Log…, Prepare Form and
  Detect Fields….
