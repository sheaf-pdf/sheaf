# M10 slice 2: The field model and form facts

Status: done · Part of [M10: Forms](../../m10-forms/README.md)

**The field model and form facts** (proposals 6, 8, 19 and 24).
- Reading the field tree and `/AcroForm` with inheritance, within
  proposal 8's limits; the form's class (AcroForm, hybrid, XFA-only);
  whether it has scripts, usage rights, signatures or a
  certification. `form_type` joins the `Renderer` trait.
- `FileFacts` and inserted sources (`Incoming`) carry it, and
  `DocState` holds it (`set_initial_fields`), with saved values for
  dirty tracking.
- Properties gains a Form row: the class, the number of fields,
  scripts, usage rights, signatures.
- The XFA-only bar.
- `[forms]` in `settings.toml`, in the commented template.
- **Checks:** each corpus file's field counts match its manifest row
  (`fields`, `field_types`, `field_scripts`, `kind`); an XFA-only
  fixture built in code shows the bar; cyclic `/Kids`, a field that is
  its own parent and a 10,000-deep chain each open as "damaged: not
  fillable"; a fuzz or proptest target for the field reader.

## Notes

Done. The field model's types are `sheaf_doc::forms::field`, and
`forms::read` reads a file's form and the facts sheaf shows about it.
The document model holds the fields; the app shows the facts.
- **`forms::read(pdf)`** gives a `FileForm`:
  - terminal fields in tree order (`Field`): an id, the source and
    object, the full name, `FieldKind`, `/Ff`, `/V` and `/DV` as a
    `Value` (text, a state, items, or signed), `/DA`, `/Q`, `/MaxLen`,
    `/Opt` as `Choice`s, `/TU`, and the field's own `/AA`;
  - their widgets by page index (`Widget`): the object, its slot in
    `/Annots`, `/Rect`, `/F`, its own `/DA`, `/MK` (colors in gray, RGB
    or CMYK, rotation, caption), `/BS` or `/Border`, `/AS`, the states
    `/AP /N` has, and `/A` with its own `/AA`;
  - `FormFacts`.
- `/FT`, `/Ff`, `/V`, `/DV`, `/DA`, `/Q`, `/MaxLen` and `/Opt` are
  inherited; `/TU` and `/AA` aren't. A value read before the field's
  type is known is fitted to it (a string `/V` on a check box is its
  state). Actions are their `/S` and, for JavaScript, the script, from
  a string or a stream; `/Next` isn't followed, and nothing runs them.
- **`FormFacts`:** the class (`FormClass`: none, AcroForm, hybrid, or
  XFA-only, meaning dynamic or XFA with no AcroForm fields; the form
  check's `Report::class` now uses it too), fields by kind, widgets,
  fields with scripts by trigger and with any script, document scripts
  with Adobe's `!ADBE::` version checks counted apart, `/XFA` and
  `/NeedsRendering`, `NeedAppearances`, `/SigFlags`, usage rights, a
  certification, signature fields and how many are signed, the form's
  `/DA`, `/Q` and `/DR` fonts, `/CO` as field ids, and why it's
  damaged. It prints as Properties' line.
- **Limits** (proposal 8): a visited set and the path from the root;
  at most 32 deep, 100,000 nodes, 200,000 entries in `/Fields` and the
  `/Kids` arrays the tree reaches, 100,000 widgets on pages; full
  names of at most 64 KiB each and 64 MiB in all (the check's limits,
  now shared); `/Opt` and list values of at most 10,000 items; values,
  scripts and other text of at most 1 MiB each (streams decoded with
  that limit) and 64 MiB in all. Past one, or on a cycle, the form is
  damaged: no fields or widgets are kept, the facts say why, and the
  file opens. A damaged form's class counts it as having fields, so a
  damaged hybrid form is hybrid, not XFA-only. Rects are ordered,
  finite and within ±1,000,000 pt, colors within 0–1, border widths
  within 0–100, `/MK /R` a multiple of 90. Values, `/Opt` arrays and
  actions many fields share are read once and shared (`Arc`). The
  document scripts' name tree reads each node and array once, and
  stops counting after 100,000 entries.
- **The model:** `DocState::fields` (`im::OrdMap<FieldId, Field>`)
  and `DocState::widgets` (`im::HashMap<PageId, Vector<Widget>>`).
  - `Document::set_initial_fields` takes the primary file's form after
    open, before any edit like the other `set_initial_*`, with widgets
    put on page ids by index, and keeps the saved fields;
    `fields_dirty` (and `Snapshot::fields_dirty`) compares with them,
    and `rebase_to_saved` resets them.
  - Moving a page carries its widgets; deleting or replacing it drops
    them, and undo brings them back. Fields stay when their widgets go.
    History's memory estimate counts both.
- `FileFacts::form` is read in `facts_in`, so at open and for inserted
  files; `Incoming::form` carries it; the app's `Doc::forms` keeps the
  primary's facts and each inserted source's. `sheaf_ops::open` loads
  the fields too. Both load them only when there are fields or
  widgets, like the other `set_initial_*` calls, so a file with no form
  edited before its facts arrive logs no warning.
- **`Renderer::form_type`** (`FormType`: none, AcroForm, XFA
  foreground, XFA full), from PDFium's `FPDF_GetFormType`, and
  `DocInfo::form_type` at open.
- **The XFA-only bar** (`form_bar.rs`): "This is an XFA form. sheaf
  can't show or fill it; Adobe Acrobat or Reader can.", with Dismiss,
  above the pages. It shows from PDFium's form type (XFA full) as soon
  as the file opens, then from the field model's class once that's
  read, which also catches static XFA with no AcroForm fields. A damaged
  form gets a notice, "This form can't be filled: a field is its own
  ancestor in /Kids".
- **Properties** has a Form row, for the 1040 "Hybrid (AcroForm and
  XFA); 199 fields; no scripts; Reader usage rights".
- **`[forms]` in `settings.toml`** (`sheaf_ops::prefs::FormPrefs`):
  `scripts` (true), `auto_calculate` (true), `auto_advance` (false),
  and the `highlight` and `required` colors, written `"#RRGGBB"`
  (Acrobat's `#CCD7FF` and `#FF0000`). In the commented template; a bad
  color is a problem on its line, like any bad value. Nothing reads them
  yet.
- For scripts, `{"dump": "form"}` and a `{"scroll": [x, y, dy]}` step
  (the Properties dialog's last rows needed it).
- Small: `file::pdf_text_bytes`; the fixtures' `every_kind` and `row`
  moved into `forms::fixture`; proptest as a sheaf-doc dev-dependency
  (adds `proptest`, `rand_xorshift` and `unarray` to the lock).

Deviations from the plan:
- **A field or widget listed twice isn't damage; a cycle is.** The
  reader first treated anything reached twice as damage, as the check
  does, which made 12 SafeDocs files unfillable. None has a cycle: a
  field in `/Fields` twice, a kid in `/Fields` as well as under its
  parent, one widget twice in a `/Kids`, a kid under two parents. Now
  a node is read where it's first reached, and roots that are
  someone's kid are read last, so they get their parent's name. Only a
  field that's its own ancestor is damage. The check still reports
  sharing.
- **So the work is bounded by entries, not only nodes.** A node is
  read once, but each list is looked through wherever it's reached, so
  fields that share one `/Kids` array cost its length each time. The
  walk that finds the roots that are kids counts every entry of
  `/Fields` and of the `/Kids` arrays it reaches, and past 200,000 the
  form is damaged; the read looks through no list that walk didn't.
  10,000 fields sharing a `/Kids` array of 10,000 (a 1.2 MB file) took
  26 s to read before, and read as damaged in 15 ms now. Document
  scripts had the same trouble with shared `/Names` arrays (4,000
  nodes on one array of 100,000 pairs counted 400,000,000), and a
  direct node in a `/Kids` array that listed the array again looped
  forever.
- **A loop through `/Parent` alone isn't damage.** The reader goes
  down `/Kids` and never follows `/Parent`, so a field that's its own
  parent, but not its own kid, reads as a field. The plan's "a field
  that is its own parent" is tested as one that's its own kid too.
  Only the form check walks a field's `/Parent` (to name a widget the
  tree doesn't reach), with a visited set and at most 32 steps. Later
  code that walks it (naming fields as pdf.js does, or looking up an
  inherited key from a widget) must bound it the same way.
- **Caps beyond the plan's list:** text (1 MiB each, 64 MiB in all),
  which bounds decompression bombs and a shared `/AP` read per widget,
  widgets, and entries (above). `/MaxLen` past 1,000 isn't damage:
  it's kept, and `Field::layout_len` caps what layout uses, since a
  long free-text field's character limit is real.
- **"Each corpus file matches its manifest row" holds for the curated
  forms, and for 99% of the SafeDocs sample,** which is what the test
  asserts (no curated form may be damaged, and a damaged sample file
  counts against the 1%); the differences are listed (below). The kind
  is compared by the manifest's rule (XFA and no fields is
  `xfa-only`); sheaf also calls dynamic XFA with AcroForm fields
  XFA-only (5 SafeDocs files), as proposal 19 says.
- Widgets on no page aren't in the model: nothing could show or fill
  them. Fields with no widget are.
- Left for later slices, as planned: an inserted file's fields (slice
  4), Duplicate Pages' copies (slice 4), widgets of a cropped page
  (slice 3), re-binding fields after a save (slice 9). Direct-object
  fields and kids aren't read (none in the corpus).

Found here:
- **lopdf read an earlier revision of 9 SafeDocs files**: Office
  files that Aspose.PDF later signed in an incremental update. Word
  writes a hybrid-reference file (ISO 32000 7.5.8.4) as two sections:
  a full `xref` table with no `/XRefStm`, which lists the objects in
  object streams as in use at offset 0, then an empty `xref 0 0`
  section whose trailer has the `/XRefStm` and a `/Prev` to the full
  table. Aspose's update adds a table and another such pair. lopdf
  never read Word's stream, on the older empty section, so it took
  those entries at offset 0, and the first object in the file, Word's
  catalog without `/AcroForm`, was loaded over the update's. It read
  no signature field, and a save through lopdf would have lost the
  update.

  The cause, in lopdf 0.45.0's `Reader::resolve_xref_and_trailer`: it
  read an `/XRefStm` only inside the `/Prev` loop, so a file with no
  `/Prev` never had its stream read. It merged the stream after the
  previous section's table, and `Xref::merge` keeps the first entry,
  so that table's stale entries won. And it only read the newest
  trailer's (it took the key from `trailer`, not `prev_trailer`), so
  older sections' streams were left out: what broke the 9. ISO 32000
  reads each section's stream right after its table, before its
  `/Prev`.

  Fixed before slice 3 by vendoring lopdf's main branch at 39492cb
  (2026-10-12), which has the fix (#598), in `vendor/lopdf` with a
  patch of sheaf's; the root `Cargo.toml`'s `[patch.crates-io]` points
  lopdf there. `vendor/lopdf/SHEAF.md` says what's there and how to go
  back to crates.io; lopdf's MIT license is in `packaging/licenses`.
  It's still version 0.45.0 with the same dependencies, so nothing
  else in the lock moved. #598 reads each section's stream right after
  its table, and drops `/XRefStm` from the trailer, so a rewrite no
  longer writes a stale offset. But it reads a stream again for each
  section that points at it: a crafted file of 1,002 small sections
  sharing one stream of a million entries took 21 s to load, and the
  form check, which loads each earlier revision, takes longer still.
  sheaf's patch reads each stream once, however the offsets reach it:
  the file loads in under 0.1 s. It also ignores an `/XRefStm` offset
  past the end of the file in lenient mode, as #598 ignores an
  unreadable stream. At 39492cb such a file fails to load, lopdf
  rebuilds its cross-references by scanning, and the trailer keeps its
  stale `/Prev` and `/XRefStm`; lopdf 0.45.0 left such an offset in the
  files it rewrote from one-section hybrid files, saves by sheaf among
  them.

  In sheaf, for files lopdf rebuilds for other reasons: an incremental
  save fails over to a rewrite when lopdf rebuilt the file's
  cross-references (`xref_start` 0), which an update's `/Prev` can't
  point at; and a rewrite drops `/Prev` and `/XRefStm` from the
  trailer.

  The commit brings the 22 others since 0.45.0 too. Most are parser
  fixes. One (#587: `%%EOF` looked for in the last 1,024 bytes, not
  512) reads 28 of the 36 files slice 1 found lopdf couldn't: XFA-only
  forms with about 590 bytes of a web page after `%%EOF`. Others
  change more than parsing: c8dd179, "Refactor and simplify code",
  reworks the reader, writer, encryption, object and object stream
  code (1,447 lines added, 1,653 removed); #583 keeps a page-tree kid
  lopdf can't load as a page, where 0.45.0 dropped it; #573 and #586
  change how the file key is derived when decrypting; #578 decodes
  each object stream once. What covers them: lopdf's 321 tests, which
  pass with sheaf's patch, and sheaf's (the save, encryption, forms
  and comments tests, and the corpus runs). For Greg: the other way is
  0.45.0 with only #598's change and sheaf's patch, which leaves all
  that out and the 28 files unread.

  Since the fix (the baseline below is slice 2's): 4,359 read, 3 that
  lopdf can't; 4,345 as their manifest rows, 1 damaged, 13 that differ
  (the 9 updates match now). 3,782 AcroForm, 262 hybrid, 314
  XFA-only, 1 with no form. Over all 4,384 files lopdf reads, usage
  rights in 822 and signed fields in 1,116; the rest as below. The
  form check's tallies are as before, and the 28 it reads now have no
  problems.

  Tests that fail on 0.45.0:
  - `fixture::hybrid_reference` writes a hybrid-reference file by
    hand, since lopdf can't. One section with no `/Prev`: a table for
    the catalog, pages, page and an appearance, and the `/AcroForm`
    and two filled text fields in an object stream only its
    `/XRefStm` lists. `hybrid_update` appends a section with its own
    table (the page) and `/XRefStm`: a new catalog, which the first
    table lists too, and in a new object stream one field refilled, a
    signature field and a new `/AcroForm`. The other field is only in
    the first section's object stream, so the update reads whole only
    if the older section's `/XRefStm` is read, the fault that broke
    the 9. (It isn't Word's two-section layout.)
  - `read::tests::hybrid_reference_files_read_their_newest_revision`:
    the first file's fields, then the update's catalog, values and
    signature field.
  - `forms::tests::a_hybrid_reference_update_is_a_revision`: the check
    counts 2 revisions, with the old value in the first.
  - `forms_open.rs`'s `a_hybrid_reference_update_opens_and_saves_whole`:
    it opens with all three fields and keeps them through a rewrite
    and an incremental save.
  - `read::tests::corpus_hybrid_reference_files_read_their_newest_revision`
    (skips without the corpus): each file with an `/XRefStm` that
    lopdf reads, undamaged (172), has its manifest's fields by type,
    allowing for pypdf as `forms_corpus.rs` does (every curated form,
    99% of the sample). On 0.45.0 the 9 differ.

  All four also fail with a lopdf that reads only the newest section's
  `/XRefStm`. Tests that fail at 39492cb without sheaf's patch:
  - `read::tests::a_cross_reference_stream_many_sections_share_is_read_once`:
    1,002 sections point at one stream of a million entries, through
    the white-space, comments and zeros before its header and the
    digits of its number. It must load in under a second.
  - `save::tests::a_stale_xrefstm_is_ignored`: a table file whose
    trailer has an `/XRefStm` past the end. An update's `/Prev` is
    the table, and a rewrite has no `/XRefStm`.
  And `save::tests::a_file_lopdf_rebuilds_is_rewritten`, for the save
  changes: a file whose `startxref` points past the end, with a
  `/Prev` and `/XRefStm` in its trailer, is rewritten when saved in
  place, without them.

  Not fixed: a rewrite still drops these 9 files' signature field. Its
  widget is on no page (an invisible signature), and
  `kept::clean_fields` keeps only fields with a widget on a kept page.
  For slice 5, whose rewrite of a signed file asks first.
- **Not fixed:** one file whose catalog lopdf can't find (a broken
  cross-reference table) and one where it drops an object it can't
  parse (an encrypted `/TU`). And lopdf can't read 8 files at all (3
  AcroForm, 5 `unparsed`; slice 1 had 36).

The baseline, on the corpus at 2026-10-15 (4,392 files):
- 4,331 read (30 `unparsed` left out, 31 lopdf can't read): 4,308 as
  their manifest rows, 1 damaged (a tree 33 deep), 22 that differ:
  - 10 list a field twice, which pypdf counts each time (1,807 fields
    where sheaf reads 1,345, in one);
  - 9 hybrid-reference updates and 2 other lopdf misreadings (above);
  - 1 has a parent field with two unnamed fields between it and its
    text field; pypdf counts the parent as a field of no type, and
    sheaf finds the text field.
- Classes: 3,773 AcroForm, 262 hybrid, 286 XFA-only (5 dynamic with
  AcroForm fields, 54 static with none), 10 with no form.
- Over all 4,356 files lopdf reads: field scripts in 1,587, document
  scripts in 513, usage rights in 794, signed fields in 1,107, a
  certification in 320, `NeedAppearances` in 119. Of the government
  forms, CA 540 (scripts on 87 fields, and 2 at document level), NY
  IT-201 (122), the I-9 (23) and B 122A-2 (81) have scripts; the IRS
  forms have only Adobe's version checks.
- PDFium's form type agrees with the reader's XFA reading on every file
  both read.
- Reading takes at most 10 ms a file (1,544 fields), 1.4 ms for the
  1040, 2.4 s for all 4,356 on one thread, after lopdf's load.

Checked by tests:
- 14 in `sheaf_doc::forms::read` on fixtures: every kind of field with
  its kind, value, id, widgets, slots and states, and the facts;
  inheritance (and what isn't inherited; a shared value is one string);
  the form's own keys (`NeedAppearances`, `/Q`, `/CO` as ids); the
  classes (AcroForm, hybrid, dynamic with fields, XFA-only, static with
  no fields, none, no `/AcroForm`, and a damaged hybrid form read no
  further than its first root); scripts by trigger, in a stream, on
  widgets, a non-JavaScript action, document scripts and version
  checks; document scripts in a shared `/Names` array counted once, and
  a `/Kids` array that lists itself; usage rights, a signature, a
  certification; values of every
  shape (one object under 1,000 fields, items, a state as a string, a
  name in a text field, UTF-16, a stream, a number); widgets clamped
  (rects, colors, rotation, border widths, `/Border`, a radio kid's own
  `/DA`, slots after a link); damaged trees, each with no fields and not
  fillable: a cycle, a field that's its own kid and parent, a widget
  that's its own kid, 1,000 fields sharing a `/Kids` array of 1,000, a
  chain 10,000 deep, 100,001 nodes, 100,001 widgets on a page, a
  70,000-byte name, names over 64 MiB in all (a 65,000-byte parent name
  over 1,100 kids), 10,001 `/Opt` items, a list value of 10,001 items,
  a string value over 1 MiB, one that inflates past 1 MiB, and 65
  values of 1 MiB; fields and widgets listed twice read once; a field
  that's only its own parent read as a field; `/MaxLen` kept and capped
  for layout. And the proptest `any_field_tree_reads`: 256 random trees of
  up to 19 nodes, with random kids (themselves and missing objects
  too), parents, types, flags, values, rects, colors and `/Opt`; no
  panic, distinct fields, each widget one field's, every number finite
  and in range.
- `model::tests::form_fields_follow_their_pages`: loaded at open,
  carried by a move, dropped by a delete and back on undo, dropped by a
  replace, not dirty, and not loadable after an edit;
  `fields_dirty_compares_with_the_saved_form`: a changed, a removed and
  an added field are dirty, in the snapshot too, and not after a save.
- `sheaf-ops/tests/forms_open.rs`: a fixture opens headless with its
  fields and widgets per page; PDFium's form type and the class agree
  for AcroForm, hybrid, XFA-only and no form; cyclic, own-parent and
  10,000-deep trees open with their page, no fields, damaged and not
  fillable.
- `sheaf-ops/tests/forms_corpus.rs`: `every_form_reads_as_its_manifest_row`
  (skips without the corpus): every curated form reads, undamaged, as
  its row, and at most 1% of the SafeDocs sample differs or is
  damaged; and slice 1's test now compares PDFium's form type with the
  reader on every file.
- `form_bar::tests`: an XFA-only fixture shows the bar, and so does
  static XFA with no fields; PDFium's type stands in before the read;
  hybrid and AcroForm forms don't; a damaged hybrid form gets the
  notice and no bar.
- `prefs::tests::forms_settings_read_and_default`, and the template
  still parses to the defaults.

Checked on Xwayland, with a scratch config:
- the XFA-only fixture opens with the bar above its pages, and
  Properties' Form row reads "XFA only, which sheaf can't fill";
- a SafeDocs XFA-only form (Ontario's EAP e-Forms) shows the bar over
  its fallback pages;
- the 1040's Properties row, and `{"dump": "form"}`: 199 fields, 199
  widgets, no bar;
- the cyclic fixture opens, with "This form can't be filled: a field is
  its own ancestor in /Kids" in the status bar;
- the XFA-only fixture inserted into the hybrid one (`--selftest-insert`)
  is listed by the dump as source 1, XFA-only.
