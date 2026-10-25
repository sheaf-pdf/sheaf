# M10 proposal 18: **Flatten draws what would print, then removes the fields:

Part of [M10: Forms](../README.md) · [all proposals](README.md)

**Flatten draws what would print, then removes the fields:
`FlattenFields`, beside `FlattenAnnotations`** (M10.5 §12.2).
- Export Flattened Copy gains "Form fields too", off by default since
  today's output keeps them, and remembered in `state.json`: it adds
  `FlattenFields` to the export's transforms, as
  `FlattenAnnotations` is there now.
- Forms > Flatten Fields… is the op `FlattenFields`: all fields, a
  page range, or the fields selected in Prepare Form or the Fields
  panel. One undo step; the save after it is then a full rewrite
  (REMOVES; asking first on a signed file, proposal 7).
- A widget is drawn when it would print: Print flag set, and not
  Hidden. Its normal appearance follows `/AS`. With no `/AS` on a
  widget that has several states, nothing is drawn: no first-state
  fallback. `FlattenAnnotations` leaves widgets alone, so only other
  annotations reach it (bug 10).
- A field with a value and no appearance gets one first.
- Then its widgets are drawn into a `Placements` layer of
  `FormRef::Pinned` and removed through `widget.field`, fields left
  with none go from the `FormDoc`, and `/Fields` and `/CO` are
  written from what's left. An `/AcroForm` left empty goes, with
  `/XFA`, `/Perms /UR3` and `NeedAppearances` (`forms::rules::plan`).
  Document scripts stay; removing them is M11's sanitize.
- **The structure tree loses them too.** 38 of the 40 corpus files
  are tagged. `keep_only` (`materialize::kept`) takes the kept pages
  and builds its own page-based set of what's gone, so it takes
  `removed_annots` too, and drops their OBJRs and their `/ParentTree`
  and `/IDTree` entries. Today only dropped pages are pruned. The
  flattened content is marked as an artifact or tagged under the old
  `/Form` element.
- **Printing flattens.** Print is `KeepPages` plus
  `FlattenAnnotations` (or `RemoveMarkup`); it adds `FlattenFields`
  always, as it flattens markup, so CUPS (`lp`), GDI and the print
  dialog's Save as PDF get the same bytes with no live form, and no
  `/XFA` or `/UR3` the save rules remove. Image export renders with
  PDFium's printing flag (bug 10).
- Insert Pages and Combine offer "Flatten form fields" for incoming
  forms (proposal 5).
