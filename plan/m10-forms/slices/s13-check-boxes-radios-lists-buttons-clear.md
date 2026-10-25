# M10 slice 13: Check boxes, radios, lists and buttons; Clear Form

Status: open · Part of [M10: Forms](../README.md)

**Check boxes, radios, lists and buttons; Clear Form** (proposal
14).
- **On the edit API:** `ResetFields` as one Tx; Hide is
  `PatchWidgets { flags }`.
- Click and Space toggle; arrows move in a radio group; a combo's
  list (Alt+Down opens it), and typing in an editable one; list boxes
  with Shift and Ctrl selection; `CommitOnSelChange` (10 fields)
  commits on a pick.
- Button actions, by an allowlist: Reset Form; Named (Print opens
  M7's print dialog; first, last, next and previous page); Hide; Go
  To; URI, with the attachments panel's warning; JavaScript (slice
  21). Submit never submits: it shows where it would have gone and
  offers to save XFDF instead. `/Launch`, `/GoToR` and `/ImportData`
  never run; any other type (`/SetOCGState`, `/Rendition`,
  `/Thread`…) is logged and ignored. `/SetOCGState` may later map to
  M7's layers panel.
- Forms > Clear Form…, with a confirmation, as one undo step.
- **Checks:** script runs over each kind; IL-1040's Reset button
  clears it as one step; PA-40's Print button opens the print dialog;
  a `/Launch` button only logs.
