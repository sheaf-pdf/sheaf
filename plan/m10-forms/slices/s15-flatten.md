# M10 slice 15: Flatten

Status: open · Part of [M10: Forms](../README.md)

**Flatten** (proposals 7 and 18; bug 10).
- **`FlattenFields` shares `FlattenAnnotations`' machinery:**
  - ensure appearances;
  - draw printable widgets by `/AS`, with no first-state fallback,
    into a `Placements` layer of `FormRef::Pinned`, `PinnedAp`
    gaining a widget's inputs (value, display, state, `/DA`, `/Q`,
    `/DR`; M10.5 slice 19 left `MarkAp` marks-only for this);
  - remove widgets through `widget.field`;
  - empty the `FormDoc` of what's flattened;
  - pass `removed_annots` to `keep_only`, which prunes their
    structure entries.

  The rest of the clean-up follows: `/Fields` and `/CO` are written
  from the emptied `FormDoc` (an `/AcroForm` left empty goes), and
  `/XFA` and `/Perms /UR3` go by `forms::rules::plan`, as the form
  changed. Document scripts stay; removing them is M11's sanitize.
- **It adds `FormBring::Flatten`** for Insert and Combine, and fields
  whose signature isn't valid there (`SignatureNotValidHere`) may
  flatten there.
- Export Flattened Copy's "Form fields too" adds `FlattenFields` to
  its transforms; Forms > Flatten Fields… is the op; print adds it to
  `KeepPages` and `FlattenAnnotations` (M10.5 §12's print row);
  image export renders with PDFium's printing flag.
- `sheaf form flatten`.
- **Checks:**
  - a filled, flattened 1040 renders as before (appearances only),
    has no widgets, `/AcroForm` or `/XFA`, and passes `qpdf --check`
    with no structure reference to a missing object; no removed
    value survives in the bytes;
  - a hidden or non-printing field isn't drawn; a box with no `/AS`
    draws nothing; undo before saving keeps the form;
  - print of a filled 1040 has no widgets and shows the value with
    form drawing off;
  - one parametrized test over every output path (proposal 7) on a
    filled hybrid 1040, with the check clean on each.
