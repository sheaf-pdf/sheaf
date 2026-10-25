# M10: Scope

Part of [M10: Forms](README.md).


The roadmap's first cut, plus what planning turned up:

- **Fill** AcroForms: text (comb, multiline, auto size), check boxes,
  radio buttons, combo and list boxes, and button actions. sheaf writes
  the appearance of every value it changes, so values look the same in
  every viewer.
- **Scripts:** a sandboxed subset of Acrobat form JavaScript in boa, run
  as a WebAssembly guest (decisions 1 and 3): the AF functions,
  keystroke, validate, calculate and format, and document-level
  functions. Not a general JS runtime.
- **Create and edit fields** (Prepare Form): every field type,
  properties, tab order, calculation order, alignment tools.
- **Form data** in and out: FDF, XFDF, CSV, tab-delimited text and JSON.
  Acrobat's XML moves to later.
- **Flatten form fields,** in Export Flattened Copy and as an edit.
- **XFA:** a clear notice for XFA-only forms. Hybrid forms (an AcroForm
  with an XFA copy beside it) are filled through the AcroForm.
- **Automatic field detection** on flat forms, born-digital and scanned.
- **Added, because the planned items need them:**
  - **A field model.** Today sheaf carries widgets through saves without
    reading them; nothing knows the field tree.
  - **Forms that survive page tools.** Crop, watermark and Remove
    Header/Footer destroy a page's form today, and Insert, Combine and
    Replace leave the incoming fields orphaned (bugs 1–5). Filling is
    wasted if the next page tool loses it.
  - **Save rules for hybrid XFA, usage rights and signed files** (bugs
    6–7), on every way out of the binder. Without them Acrobat shows the
    stale XFA copy instead of what sheaf filled, Reader's usage-rights
    signature no longer matches the file, and a save can break a
    signature without asking.
  - **A Unicode font for appearances, and an editor that matches
    them.** M9 decision 11 left the font to M10: today any character
    above U+00FF is written as `?`.
  - **A forms corpus and a form check** (slice 1), used by every later
    slice's tests.
  - **Field values in Find, Patterns and text export** (bug 8). M11's
    leak check searches form values too.
- **Reordered** (proposal 4): fill first, then flatten and data, then
  scripts, authoring and detection. The roadmap says IRS and state forms
  depend on calculations; none of the 29 IRS forms has a field script,
  and only Georgia's of the 11 state forms does real arithmetic. So IRS
  forms work fully before any JavaScript does.
- **Cut line:** field detection (slices 27–28). If M10 runs long, it
  moves to a later milestone without holding up v0.6.
- **Moved out:** signing and Fill & Sign to M12, batch filling and a
  copy per CSV row to M15, removing scripts when sanitizing to M11
  (proposal 22).
