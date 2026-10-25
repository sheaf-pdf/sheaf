# M10 proposal 6: **A hybrid form loses its XFA copy on the first save that changes the

Part of [M10: Forms](../README.md) · [all proposals](README.md)

**A hybrid form loses its XFA copy on the first save that changes the
form.**
- 28 of the 29 IRS forms carry static XFA beside the AcroForm (all
  but 1040-ES); no state form does. Acrobat prefers the XFA copy, so
  after a fill of the AcroForm alone it shows stale or blank values
  (known behavior; not reproduced, since it needs Acrobat).
- So when a value or the field tree changes (a fill, an import, a
  page tool that adds or removes fields), the save removes `/XFA`
  from `/AcroForm` and says so once: "Removed the form's XFA copy, so
  Acrobat shows the values filled here." iText's `removeXfaForm` and
  pdftk's `drop_xfa` do the same.
- Removing it counts as removing content, so that save is a full
  rewrite (asking first on a signed file, proposal 7): XFA datasets
  can hold values Acrobat filled.
- The binding isn't a name mapping:
  `topmostSubform[0].Page1[0].f1_01[0]` binds to
  `topmostSubform/f1_01`, and check boxes hold `0` or `1`. sheaf
  resolves it to read the datasets; updating them instead of removing
  them is left for later.
- **Unless the datasets hold values the AcroForm lacks.** A static
  XFA form filled by an XFA-only tool can keep its values only in
  `<xfa:datasets>`; sheaf would show blanks, then delete the client's
  data on the first save. At open, dataset leaves are compared with
  `/V` by binding. Even a blank 1040 has 62 non-empty leaves, so "the
  datasets are empty" isn't a test. If they hold values the AcroForm
  lacks, a bar says "This form's XFA part holds values sheaf doesn't
  show", and dropping it asks first.
- **Tooltips move to `/TU`.** The IRS XFA layers hold the forms'
  tooltips (4,977 `<speak>` elements, against 30 `/TU` entries) and
  366 scripts no other viewer runs. When the XFA goes, `<speak>` and
  `<toolTip>` text is copied into `/TU` wherever the binding
  resolves, so the Fields panel, the CSV's tooltip row and screen
  readers keep them.
