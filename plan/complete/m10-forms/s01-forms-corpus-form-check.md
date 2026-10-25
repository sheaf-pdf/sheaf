# M10 slice 1: The forms corpus and the form check

Status: done · Part of [M10: Forms](../../m10-forms/README.md)

**The forms corpus and the form check** (proposal 21; bug 10).
- Corpus lookup (`$SHEAF_CORPUS`, then `../pdf-corpus`, else skip)
  and a reader for its `manifest.tsv`; a builder for form fixtures in
  code (fields of each type, widgets, `/DR`, `/XFA`, `/Perms`, a
  structure tree).
- **`sheaf_doc::forms::check`,** from the planning probe: widgets no
  field in `/Fields` reaches; widgets on more than one page, with the
  wrong `/P`, or with `/P` null; fonts named in `/DA` missing from
  `/DR`; duplicate full names; values with no appearance; earlier
  revisions whose field values differ from the current ones; `/XFA`,
  `/Perms` and `/SigFlags`, and whether they still fit the file. Every
  later slice asserts it on what it writes.
- **A PDFium check that renders appearances only** (form data off),
  so tests see what other viewers see (bug 10).
- `sheaf form check file.pdf` prints the check.
- ROADMAP.md and `m11.md` say M10 comes first, and ROADMAP's M10
  "Slices:" line takes this plan's order (decision 2), if that wasn't
  done at kickoff.
- **Checks:** every form under the corpus's `forms/` opens, and the
  check's findings on them become the baseline, recorded in the slice
  notes (1040-ES's fonts missing from `/DR`, HelveticaLTStd-Bold ×110
  and ArialMT ×4, should show); fixtures built in code with each
  problem are caught; with no corpus, the corpus tests skip.

## Notes

Done. The form check is `sheaf_doc::forms::check`, built from the
planning probe; fixtures and the corpus lookup sit beside it.
- **`check(bytes, password)`** (and `check_file`) gives a `Report`:
  pages, terminal fields, widgets on pages and on page templates, the
  XFA copy's packets (and whether it's dynamic, `/NeedsRendering`),
  `NeedAppearances`, usage rights, a certification, `/SigFlags`,
  signature fields and how many are signed, revisions, and
  `findings`. Findings about fields or widgets name them, a widget by
  its field's full name:
  - widgets no field in `/Fields` reaches; widgets in the file on no
    page (not those on page templates, `/Names /Templates`, where
    forms that spawn pages keep them); on more than one page; whose
    `/P` is another page, or null or not in the file (a missing `/P`
    is fine);
  - fields whose `/Kids` hold a field or widget whose `/Parent` isn't
    them (another field, or none): pdf.js names fields up `/Parent`,
    PDFium follows `/Kids`, so the two see different fields;
  - fonts a `/DA` names (the widget's, its field's, or the form's)
    that `/DR /Font` lacks, counted by widget;
  - full names more than one terminal field has;
  - values with no appearance on the widgets a viewer draws (on a
    page and not Hidden; `NoView` ones still print): a text or choice
    value with a widget that has no `/AP /N`, or a check box or radio
    value no widget shows (`/AS` in that state, with an appearance
    for it);
  - values an earlier revision holds that the current one doesn't
    (replaced or cleared). A revision ends at a `%%EOF` after
    `startxref n`, for an `n` no earlier `%%EOF` gave, where lopdf
    reads the file up to it without rebuilding its cross-references
    and finds pages. So a `%%EOF` in a stream (an attached PDF), a
    trailer written twice and a linearized file's first-page section
    aren't revisions. The last 256 are read. (lopdf's strict mode
    would also catch a rebuilt cut, but it refuses things real files
    do.);
  - an XFA copy that doesn't fit: the template's fields as SOM paths
    (containers by name, or `#subform` and the like when unnamed; an
    exclusion group as one field; nothing hidden, no prototypes)
    against the AcroForm's full names, without their `[n]` and with
    `\.` read as a dot. Not for a dynamic form;
  - a signature (UR3, DocMDP, a signed field) whose `/ByteRange` isn't
    `[0 a b c]` around a hex string, ending where a revision ends;
  - `/SigFlags` 1 with no signature field, or 2 with nothing signed
    (no signed field, UR3 or DocMDP);
  - a damaged tree: a node reached twice, more than 32 deep, past
    100,000 nodes, a full name longer than 64 KiB, or full names
    adding up to more than 64 MiB. The walk doesn't follow it, and
    reads the rest.
- **Bounded on files from the wild.** A parent's name is in each of
  its kids' names, so full names stop at 64 KiB each and 64 MiB in all
  (the corpus's longest `/T` is 1,608 bytes), and the XFA paths the
  same. Values are compared by a hash of their text, read once per
  object however many fields share it, not copied per field. A `/DA`
  over 1 KiB isn't read, and the names shown for widgets outside the
  tree stop at 256 bytes. One root with a 1 MB value (or name) and
  2,000 kids took 8.7 s and 3.9 GB before this, and takes 0.03 s and
  16 MB; a 1 MB XFA container name around 3,000 fields, 10.9 s and
  5.9 GB before, 0.01 s.
- Names are compared as bytes (a button's state with `/AS`, a `/DA`'s
  font with `/DR`), and text from the file prints escaped, so a field
  name can't send control sequences to the terminal.
- The report prints as `sheaf form check file.pdf [--password]` shows
  it (`crates/sheaf-cli/src/form.rs`; the app hands `form` over too).
  `Report::class` says AcroForm, hybrid, XFA-only (dynamic, or no
  AcroForm fields) or no form; `Finding::kind` is for tallies.
- **`sheaf_render::pdfium::render_appearances(bytes, password, page,
  width)`**: a page as a viewer that draws only appearance streams
  shows it. PDFium flattens a private copy (each annotation's Print
  flag set to whether it shows on screen, since pdfium-render's
  flatten keeps what prints), then renders it. A check box or radio
  button with no `/AS` draws nothing: the flatten would draw its first
  state, the fallback bug 10 rules out (poppler draws the Off state,
  pdf.js the state of the field's value).
- **Fixtures** (`forms::fixture`; other crates' tests turn on
  sheaf-doc's `fixtures` feature): `Form::new(pages)` with `/Helv`
  and `/ZaDb` in `/DR`; `text`, `check_box`, `radio`, `choice` (combo
  or list), `push_button`, `signature`; `fill`; `group` (a parent
  field); `xfa`, a template and datasets naming the fields as they
  are, as Designer would (values escaped, names that must be XML
  names, an exclusion group only for a radio group), and `xfa_only`;
  `usage_rights`, `sign`, `certify`; `tag`, a `/Form` element per
  widget in a parent tree and `/Tabs /S`; `bytes`, which fills in
  each signature's `/ByteRange` around zeroed `/Contents`; and
  `append_revision`, an incremental update through lopdf's
  `IncrementalDocument`.
- **Corpus** (`forms::corpus`): `Corpus::find` (`$SHEAF_CORPUS`, then
  `../pdf-corpus` beside the workspace, each only with a
  `manifest.tsv`), `or_skip` ("skipped: no corpus …"), and the
  manifest read by its header's column names, so new columns don't
  break it.
- `file::load_pdf_mem` (`load_pdf` for bytes); `load_source` uses it.
- ROADMAP.md and `m11.md` were brought up at kickoff (990eb6e), with
  this plan's slice order. Nothing left to change.

Deviations from the plan:
- **Appearances only isn't "form data off".** With form drawing off,
  PDFium draws no widgets at all, so `render_appearances` flattens a
  copy instead. It ignores `NeedAppearances`, as other viewers that
  don't build appearances do.
- **The corpus grew during the slice:** from the 76 government forms
  of kickoff to 4,392 files, 4,316 of them a sample of the SafeDocs
  pool. So the corpus test opens every file but draws only the curated
  ones (all of them would take most of a minute), and the checks run
  on other threads while PDFium opens files on one (see "Found here"
  below). The test takes 9 s here.
- **The 4 files PDFium can't open** are the ones the manifest marks
  `unparsed` (pypdf can't read them either). The test lists them;
  any other file that doesn't open fails it.
- **"Whether `/XFA` still fits" is by field paths.** They match field
  for field on all 71 hybrid government forms; two IRS templates have
  fields with no widget (a hidden cover sheet's links in 4506-C, a
  prototype in 8949), which the rules leave out. A static XFA copy
  with no AcroForm fields at all is reported (bug 6's extracted W-9
  page).
- **Problems aren't an exit status:** `sheaf form check` exits 0
  whenever it could read the file.
- Direct-object fields and kids aren't followed (none in the corpus).

Found here:
- **PDFium failed to load files when several threads used it.** With
  pdfium-render 0.9.4 (its `thread_safe` feature) and the vendored
  libpdfium, loading the 73 forms under `forms/{irs,uscis,va,
  ca-ftb}` failed 0 times from one thread, and 544 and 550 of 584
  times from 8 threads in two runs (`FormatError`; 553 through
  `load_pdf_from_byte_vec`). The same loads through ctypes, with one
  lock around each call, didn't fail. The app opens each document on
  a render thread of its own (`RenderHandle::open`: docview.rs:779,
  :1015, :1589, :2574; page_insert.rs:166, :296, :427, :535;
  layers_panel.rs:92), so opening two files at once, or inserting
  from one while another renders, could fail the same way.

  The cause: pdfium-render's lock doesn't cover every call. 185 of
  the 467 functions its `thread_safe` wrapper binds take no lock,
  among them `FPDFDOC_InitFormFillEnvironment`, which every load
  makes (`PdfDocument::from_pdfium`), `FPDF_GetPageSizeByIndexF`, the
  text calls and `FPDFBitmap_FillRect`. So PDFium parsed on two
  threads at once. Its parser keeps its depth in a process global,
  `CPDF_SyntaxParser::s_CurrentRecursionDepth` (in `.bss`, not
  thread-local): `GetObjectBodyInternal` saves it, adds one, fails
  past 64, and puts the saved value back. Interleaved parses put back
  stale values and it drifts up. It read 63 after an 8-thread run,
  and from then on every load failed, from one thread too (73 of 73).
  A lock around loads alone isn't enough: loads under one still
  failed (285 of 292) while other threads sized pages and read text.

  Fixed before slice 3 by one lock in sheaf-render (`pdfium::lock`),
  held across every use of PDFium: each call into a `PdfiumEngine`, its
  load and its drop, and `render_appearances`. `pdfium()` is private
  now, so nothing reaches PDFium around it. Documents on different
  threads take turns a call into the engine at a time (a page's render
  or text, a page of a search). A render's drawing was already one
  PDFium call under pdfium-render's lock, so renders of different
  documents interleave as before. The two whole-document walks, page
  sizes and the outline, take the lock a page or a bookmark at a time.
  The lock is fair (parking_lot's `FairMutex`). With std's mutex a
  thread that locked again at once kept winning: a 20,000-page size
  walk took 20 s to over 400 s while another document rendered in a
  loop (0.15 s alone), and a document opening during a search waited up
  to 228 ms. With the fair lock the walk takes 0.9 to 2.1 s and the
  open 0.3 ms.

  `sheaf-render/tests/threads.rs` opens, sizes and reads 5 forms built
  in code, 4 times over, and up to 16 of the corpus's, once, from 8
  threads, each thread drawing an eighth of them; then it opens them
  again from one. Without the lock, 127 to 153 of the 160 uses of the
  built forms failed in 24 runs on 2, 4 and 32 cores, and 263 to 277 of
  288 with the corpus in 6. On one core the threads hardly overlap and
  it passes. With a lock around loads only, the test (then one round)
  failed all 6 runs. With the fix it passes, in 0.2 to 0.4 s without
  the corpus and 3 s with it. Slice 1's corpus test, changed to open
  from 16 threads, opened 4,388 of the 4,392 files, as from one (not
  kept: it's no faster, since PDFium takes turns). Six IRS forms opened
  in the app at once didn't show the bug before the fix either (3
  runs); after it they open and draw.
- lopdf can't read 36 files of the SafeDocs sample (below), so the
  check can't run on them and sheaf couldn't save them. Not fixed.
  With the lopdf vendored before slice 3 (slice 2's note) it can't
  read 8; the 28 it reads now have a web page after `%%EOF`.

The baseline, on the corpus at 2026-10-15 (4,392 files):
- PDFium opens 4,388, with the manifest's page count on every one.
- lopdf can't read 36 (damaged cross-reference tables or trailers: 28
  XFA-only, 3 AcroForm, 5 `unparsed`), all in the SafeDocs sample.
- **Government forms (76):** all but 1040-ES are clean. 1040-ES:
  fonts missing from `/DR`, HelveticaLTStd-Bold ×110 and ArialMT ×4,
  as the probe found. 71 are hybrid, with an XFA copy that fits; 71
  have usage rights that fit, with SigFlags 2; 70 have earlier
  revisions (two each, f2848 and f9465 three), none holding other
  values. Their field counts match the manifest.
- **SafeDocs sample (4,280 read):** 3,404 clean. Files with each
  problem: values with no appearance 114 (10 of them set
  `NeedAppearances`; leaving out Hidden widgets took 128 files off the
  first count of 267, and comparing button states as bytes 25),
  widgets on no page 221 (3 more keep widgets on page templates
  only), values in earlier revisions 223 (spot-checked: replaced
  values), fonts missing from `/DR` 163, signatures that don't fit
  125 (101 usage rights after a rewrite, as in bug 7), SigFlags with
  nothing to flag 52, widgets no field reaches 32, duplicate names
  21, fields with kids whose `/Parent` is elsewhere 14 (one looked
  at: a field `name` whose kids are widgets named `name` with no
  `/Parent`), damaged trees 14 (13 shared kids; one tree 33 deep,
  which PDFium's own limit of 32 also refuses), an XFA copy that
  doesn't fit 13 (master-page fields, a signature field added to the
  AcroForm alone, static XFA with no AcroForm fields or just one), a
  null `/P` 9, widgets on two pages 6, the wrong `/P` 4.

Checked by tests:
- 24 in `sheaf_doc::forms` without the corpus: a form of every kind
  (with XFA, usage rights and a structure tree) is clean; then one for
  each problem: widgets no field reaches, on two pages, with a wrong,
  null or dangling `/P` (and none for a missing one), on no page (bug
  5's stray) but not on a page template; kids whose `/Parent` is
  another field or missing; fonts from a widget's, a parent field's
  and the form's `/DA`; names compared as bytes (a Latin-1 check box
  state and font); duplicate names; values with no appearance (a
  check box showing off; none for a Hidden widget, one for a `NoView`
  one); earlier revisions (replaced and cleared values; filling an
  empty field leaves none), and none from an attached PDF's `%%EOF`
  or a trailer written twice; an XFA copy after a removed field, an
  added one, and no fields, and the fixture's datasets escaping a
  value; a dynamic XFA-only form; usage rights after a lopdf rewrite
  (bug 7), a signature surviving an append, a certification (one
  signature, in `/Perms` and its field) not surviving a rewrite;
  SigFlags with nothing to flag; a cycle, a field that's its own kid,
  a 10,000-deep chain and 100,001 fields; a full name over 64 KiB,
  names over 64 MiB in all (a 65,000-byte parent name over 1,100
  kids), and an XFA path over 64 KiB, which isn't compared; text from
  the file printed escaped; an encrypted form with its password, a
  wrong one and none; the manifest parser; and the lookup (no
  `manifest.tsv`, no corpus, so the corpus tests skip). The skip is
  checked through `Corpus::first_of`: where `../pdf-corpus` exists,
  `Corpus::find` finds it whatever `$SHEAF_CORPUS` says.
- 2 on the corpus, which skip without it: 1040-ES's findings (its
  missing fonts, nothing else) and field count (`sheaf_doc::forms`),
  and every file opened and checked
  (`sheaf-ops/tests/forms_corpus.rs`): all but the `unparsed` ones
  open, with the manifest's page counts, and lopdf reads every
  curated form. It prints the tallies above with `--nocapture`;
- `sheaf-render/tests/form_appearances.rs`: appearances only, by
  pixels: a value with no appearance is blank (the page view draws
  it), a check box shows its state, radio buttons with no `/AS` show
  none, hidden and `NoView` widgets don't show, one without the Print
  flag does;
- `sheaf-cli/tests/form.rs`: `form check` prints a clean form and a
  broken one, and fails on a missing file.
- Not tested: an incremental update to an encrypted file, which the
  fixtures can't write.

No UI in this slice, so no on-screen check.
