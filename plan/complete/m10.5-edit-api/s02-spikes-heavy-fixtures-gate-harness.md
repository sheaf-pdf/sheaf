# M10.5 slice 2: Spikes, heavy fixtures and the gate harness

Status: done · Part of [M10.5: the edit API](README.md)

**Spikes, heavy fixtures and the gate harness** (M; proposal 16).
- Its own commit; the spike code lives under `bench/`, never in
  `crates/`.
- **Spike A, encrypted overlay sections.** RC4-128, AES-128 and
  AES-256 bases, sections encrypted through `encrypt_object`, loaded
  through the two-segment `FPDF_LoadCustomDocument`.
- **Spike B, overlays on real files.** It covers:
  - the 402-file pool sample;
  - corpus forms with widgets, after a page is replaced in place;
  - xref-stream, hybrid, linearized, damaged and leading-junk bases.

  It measures open, warm and cold 1024 px renders, and RSS per
  engine.
- **Spike C, render-time box edits.** A crop inside the base's
  visible box, through a matrix and clip on the base engine.
- **Written criteria:**
  - A passes if all three render pixel-equal to a decrypted Full
    write.
  - B passes per base class if at least 99% of sampled pages are
    pixel-equal (appearances only) to a Full write, and RSS per
    engine stays within the cap it sets. A failing class realizes as
    a standalone PDF per group.
  - C passes if it is pixel-equal to a written crop on 100% of the
    sample. If it fails, crops realize as overlays.
- **Verdicts.** GROUP, the LRU count, the engine byte cap, the
  background budget and the thumbnail fill rate go into
  `docs/document-model.md` before slice 8.
- **Fixture generators** (`bench/gen.py`):
  - the **heavy 7k**: embedded subset fonts per section, an xref
    stream with object streams, about 20k annotations including
    widgets, 2k bookmarks, labels;
  - a **scanned 7k** of about 500 MB.
- **`tests/gate_7k.rs`** (ignored) records today's baselines on both
  fixtures and the synthetic one.
- **The planning measurements' scripts** are committed under
  `bench/`: `gen.py` (the synthetic file, which grows the heavy and
  scanned generators), and `bench.py`, `real.py`, `warm.py` and
  `form.py` (PDFium through ctypes), so proposal 16's numbers and the
  baselines can be re-run.
- **Deletes:** the spike code, at the end of the slice. The verdicts,
  fixtures, scripts and harness stay.
- **Checks:** the harness runs on all three fixtures and prints
  baselines, and the verdict section is present with every number
  filled in.

## Notes

Done. The overlay design is measured, and the five numbers slice 13
builds on are set: encrypted sections (A) pass, overlays on real files
(B) pass on every class but damaged bases (which realize standalone by
design), render-time crops (C) fail, and GROUP is 128 pages with
scoped group sections, the screen's LRU 6 group engines in 96 MB, the
background budget 2 engines in 32 MB, and the thumbnail fill rate
310 pages/s. The planning scripts, the heavy and scanned 7k
generators and the 7k gate harness with today's baselines are
committed; the spike code is deleted.
- **`bench/`:** `gen.py` makes the fixtures
  (`gen.py synthetic|heavy|scanned N out.pdf`, through argparse
  subcommands, written to `out.pdf.<pid>.tmp` and moved into place, so
  a killed run leaves no partial file and two runs share no temporary
  file). `bench.py`, `real.py`, `warm.py`
  and `form.py` time PDFium through ctypes, with the libpdfium to time
  as their first argument, and share `pdfium.py`'s bindings. Each
  docstring says what it measures and what the planning run measured.
  `pool-sample.txt` is the planning sample, corpus-relative, in its
  original order: 400 of the corpus's 1,935 pool PDFs of 4 MB or more
  (4.2–5.0 MB, up to 1,050 pages), plus `forms/safedocs/0174624.pdf`
  and `0043009.pdf`. `README.md` lists each file and fixture with its
  command, the corpus lookup and the harness command. Standard library
  only, run with `python3 -I` from the repo root; `real.py` finds the
  corpus as the tests do (`$SHEAF_CORPUS`, then `../pdf-corpus`) and
  says it skipped without one.
- **The synthetic 7k** is byte-identical to the planning run's, so
  proposal 16's numbers re-run on the same input: 3,510,145 bytes,
  sha256
  `297917810fe2447c611d20bf8de377184cf641a122f49ab5357be5fd2008142c`
  (with `img_every` 1, 4,123,353 bytes,
  `4ee9f637d3c96a9bd515c67bfeb77762d7ec504a6a47288797486d60847dfd9a`).
- **The heavy 7k** (`gen.py heavy N out.pdf [--font PATH]`): 70
  sections of 100 pages, each its own `/Pages` node under the root
  and its own embedded subset font (a simple `/TrueType` font,
  `/WinAnsiEncoding`, `/Widths`, a Flate `/FontFile2`, BaseFont like
  `AAAAAB+DejaVuSans`). Six section words in each heading vary the
  glyphs: the 70 subsets come in 58 glyf sizes. About 50 lines of
  Flate text a page and three annotations: a `/Link` to a page in
  another section (`/XYZ` or `/Fit`); a markup in turn `/Square`,
  `/Highlight` (with `/QuadPoints`) and `/Text`, each with `/AP /N`,
  `/NM`, `/T`, `/Contents`, `/M` and `/P`; and a merged field and
  widget, a `/Tx` whose `/AP /N` shows its `/V`, every tenth a check
  box with `/AS` and `/Yes` and `/Off` appearances. `/AcroForm` lists
  all 7,000 fields. One bookmark a section with 28 children,
  `/PageLabels` in three ranges, every non-stream object packed 100 to
  a Flate `/ObjStm`, one `/Type /XRef` stream and no table, under
  `%PDF-1.7`.
- **The scanned 7k** (`gen.py scanned N out.pdf`): one image XObject a
  page, 1275 × 1650 8-bit DeviceGray, Flate, copied from a seeded pool
  of 32 pseudo-scans (white paper, speckled rows, word-shaped ink;
  about 70 KB a page), drawn by `q 612 0 0 792 0 0 cm /Im0 Do Q`. One
  flat `/Pages`, a classic xref, no text layer, streamed to disk.
- **The gate harness,** `crates/sheaf-ops/tests/gate_7k.rs`: five
  ignored tests, run with
  `cargo test -p sheaf-ops --release --test gate_7k -- --ignored
  --nocapture --test-threads=1`.
  - `gates_on_the_synthetic_7k`, `_heavy_7k` and `_scanned_7k` find
    their fixture in `$SHEAF_GATE_DIR`, else `target/gate-7k/`, as
    `<kind>-7000.pdf`, and make one that is missing, older than
    `gen.py` or not the length below with `python3 -I bench/gen.py`,
    panicking with gen.py's stderr if that fails. A synthetic or
    scanned fixture gen.py then makes at another length fails ("take
    the baselines again"); a heavy one warns that its font isn't
    DejaVuSans. No fixture's sha256 is checked. The document is opened
    from `<kind>-7000-work.pdf`, a copy beside the fixture, so the
    incremental save writes its own file; crop's copies go to `cache/`
    (`SHEAF_CACHE_DIR`).
  - Each runs in two child processes of the test binary, the second
    for the flatten alone, and each child writes a file once its rows
    are done; the parent fails if one is missing, so a test renamed
    away from `--exact` can't pass with nothing run.
  - Each prints one table, a row per measurement: now, slice 2's
    baseline, the ratio and the gate a later slice sets on it. The
    rows, each timing what the app does: the geometry walk
    (`PdfiumEngine::open`, then a timed `page_sizes`); ready
    (`open_with_foreign`) and its RSS; the middle page at 1,024 px,
    cold (open + render) and warm; 200 previews at 320 px, in pages a
    second; the incremental save of one Square mark (`save_to_path`
    on the working copy, which writes the whole file, then the copy
    put back) and its RSS; Rewrite (the bytes only, as slice 9
    compares writers), its RSS and the bytes it writes; a crop of
    every page by 36 pt a side (`crop::plan` with the generated cache
    emptied first, so each run writes its copy, a `RenderHandle` on
    each copy, `add_source`, `RebasePages`) and the edit alone;
    Bates (`derive::strip_stamps`, an engine on each stripped copy,
    then `RebasePages` if any and `SetHeaderFooter` in one
    `apply_batch`, as `set_header_footer` in page_insert.rs does;
    then the Rewrite that draws it); and, in the second process,
    `export_flattened` on the file as opened, with its RSS beside
    1.5 × file + 200 MB. Every edit is undone after its rows.
  - Times are medians, of 3 runs for whole-file work and 5 for the
    warm render. RSS calls glibc's `malloc_trim(0)`, writes 5 to
    `/proc/self/clear_refs`, runs, and reads `VmHWM`: each
    measurement prints its peak and what it added to the RSS it
    started from. Off Linux the RSS rows read n/a.
  - `history_at_7k_pages_and_20k_marks` moved here from sheaf-doc,
    with its setup, medians and asserts (< 1 ms, < 2 ms), and
    `history_of_200_ocr_versions_at_1000_pages` records the history
    budget gate's baseline without asserting. It grows the process by
    about 4.7 GB until slice 12 bounds history, so it skips, saying
    why, with under 6 GB of `MemAvailable`.
  - The baselines are consts: `SYNTHETIC`, `HEAVY` and `SCANNED`
    (`Baseline`, a field a row), `MARKS_7K` and `OCR_200`, each value
    the median of three full release runs on the machine below.
- **The spikes** were a Rust package in `bench/spike/` (its own
  Cargo workspace and lock file, git-ignored by its own `.gitignore`,
  lopdf by path from `vendor/lopdf`, pdfium-render =0.9.4), deleted
  at the end of the slice with its target directory.
  - **Sections:** vendored lopdf's `IncrementalDocument` with the
    edit as a `new_from_prev` delta: the base's xref kind, `/Prev`,
    `/ID` kept, `/Encrypt` repeated and every new object encrypted by
    `encrypt_object`; new objects numbered above `/Size`; on leading
    junk, the section built against `base[h..]`, so offsets count
    from the header (§11.7). The reference was lopdf's Full write of
    the same delta.
  - **The edit:** three pages (the first, middle and last) replaced
    in place: `/Contents` became `[q, originals…, Q + a filled block
    and Helvetica text]`, `/Resources` resolved through inheritance
    plus the text's font, and `/Annots` the page's own plus a Square
    with `/AP /N`, scaled to the page's visible box. Two pages left
    alone were compared too.
  - **Loading:** `Segments`, a `Read + Seek` over
    `[base Arc, section Arc]`, through pdfium-render's
    `load_pdf_from_reader` (`FPDF_LoadCustomDocument`).
  - **Comparing:** BGRA pixels at 1,024 px, exactly, two ways:
    appearances only (each annotation's print flag set to whether it
    shows, PDFium's flatten, then a render, as sheaf-render's
    `render_appearances` does), which decided the criteria, and live
    (`FPDF_FFLDraw`, as the page view draws). PNG pairs of every
    mismatch were kept. Corpus bases ran in child processes, 16 at a
    time with a 60 s cap, so a PDFium crash or hang cost one base;
    the built bases ran alone.
- **Docs:** `docs/document-model.md` gains "Rendering edited pages:
  the spikes (M10.5 slice 2)" after "Saving" (it had no section on
  rendering edited pages): what an overlay is, how the spikes ran,
  each criterion and result, the fallback taken, the five verdicts
  with their rules, and what the spikes found. The history bench's
  reference in "Costs and how they're bounded" points at the harness.
  `bench/README.md` points at the section.

Deviations from the plan:
- **The harness is `crates/sheaf-ops/tests/gate_7k.rs`,** not
  `tests/gate_7k.rs`: the root `Cargo.toml` is a virtual manifest
  (slice 1's note), and sheaf-ops already depends on sheaf-doc and
  sheaf-render and has open and crop.
- **The history bench moved into it** (slice 1's "left for slice
  2"). sheaf-doc's private helpers `src` and `square` became
  `SourceInfo` and `Annot` struct literals, and `history_tests.rs`
  lost the test, its `Duration`/`Instant` import and the module doc's
  "And, ignored, the timing at 7,000 pages." The 200-OCR-versions
  row is measurable today, model only, so it was added as a baseline
  without an assert.
- **Baselines are consts in the harness,** printed beside each run
  with a ratio, and below, naming the machine. The plan didn't say
  where; slice 9 deletes today's writer, yet its gate is relative to
  it. Each is the median of three full runs: single runs differed.
- **Each fixture test runs in processes of its own** (the test
  binary re-runs itself for that test, `--exact`, once for the edit
  rows and once for the flatten, with `SHEAF_GATE_FIXTURE_PROCESS`
  naming the file the child writes when done and `SHEAF_GATE_PART`
  the rows; the model-only tests stay in the parent). In one process
  the memory a fixture freed stayed resident and fragmented: the
  synthetic's ready peaked at 47 MB alone and at 160 MB after the
  heavy and scanned tests. With a process each, full and
  single-fixture runs agree within 1–2%. `malloc_trim(0)` before
  resetting `VmHWM` keeps a row from counting what earlier rows freed
  (the heavy's flatten started at 806 MB without it, 248 MB with).
  The flatten got its own process after review: behind the crop and
  Bates rows the heavy's started at 248–279 MB, and on the file
  opened again in the same process still at 240–293 MB (heap the
  edits fragmented, which `malloc_trim` can't return); in its own
  process it starts at the document as opened, 139 MB.
- **The table has a gate column** beyond now, baseline and ratio,
  with the Gates table's threshold and the slice that asserts it.
  Render and crop print two lines each (cold and warm; total and the
  edit alone), and Rewrite prints its output size as a row of its
  own.
- **Each RSS measurement prints two rows,** its peak and what it
  added to the RSS it started from, and both are baselines. The
  plan's "≤ 1.5× its RSS" for Rewrite is read as what the Rewrite
  adds: most of today's peak is the eagerly opened document, which
  slices 3 and 8 shrink, so a ratio of peaks would let the new writer
  use twice today's writer's own memory, and slice 9 deletes today's
  writer, so its share can't be measured again later. Slice 19's
  limit stays on the flatten's peak, as the Gates table words it.
- **Rows time what the app does,** not only the sheaf-doc call: the
  incremental save writes the file (`save_to_path` over the copy the
  document was opened from, which `same_file` needs), crop opens a
  `RenderHandle` on its copy as page_insert.rs does and writes the
  copy each run (the generated cache is emptied first; `store` would
  otherwise read the old copy back and compare it), and Bates runs
  `set_header_footer`'s `strip_stamps` and `apply_batch`, which load
  every source with lopdf. A first version timed the bytes, the cache
  hit and the bare `SetHeaderFooter` (0.007 ms against < 50 ms).
- **Gate rows with no counterpart today aren't in the harness:**
  realize + open + render an edited page, engines after 200 groups
  and the thumbnail strip (slice 13), the live-token reorder (17),
  and idle CPU (the app's, 12 and 13). The module doc names the slice
  that adds each.
- **No rows in the retired ledger,** and no `cargo check` for deleted
  symbols: `crates/sheaf-doc/tests/retired.rs` walks `crates/` only,
  the spike code never entered `crates/` or git, and moving a test is
  not a retirement. Nothing under `crates/` referred to `bench/spike`.
- **The scripts:** `bench/pdfium.py` is new (the four timing scripts
  repeated about 25 lines of ctypes declarations each; their timings
  are unchanged), and `bench/.gitignore` ignores `__pycache__/`.
  `bench.py` keeps the planning run's row letters (A load, with the
  size walk under it; B render; C text; D a concatenated section; E
  two segments; F a scratch PDF; G `GenerateContent`). `gen.py` lost
  a dead `leaves = []`, its local imports moved to the top (output
  byte-identical), it takes a subcommand, and it refuses N < 1, which
  made a broken file. Its summary line is now
  `<fixture>, N pages: B bytes in S s` and counts, for the synthetic
  too (it printed `3510145 bytes 7000 pages`). `real.py` reports files
  it couldn't read or PDFium couldn't load (none in the sample)
  instead of skipping them.
- **A standard-library TrueType subsetter, not fontTools:** this
  machine has no fontTools, pikepdf or qpdf, and gen.py stays stdlib
  only. It keeps glyph ids, empties the glyphs a section doesn't use
  (following composites), and drops `fpgm`, `prep` and `cvt` and also
  `GDEF`, `GPOS`, `GSUB`, `MATH`, `kern`, `FFTM` and `gasp`, which a
  PDF never reads; `post` becomes format 3. Glyph instructions are
  stripped too, since without `fpgm` they would call functions that
  no longer exist. It reads a short or long `loca` and writes a long
  one. The subset tag is 0-based (section 1 is `AAAAAA+`) and the name
  is the font's PostScript name. A font it can't read exits 2 naming
  `--font`.
- **The heavy 7k's bytes depend on the font,** by default
  `/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf` (sha256
  `b4c632e3cdf9acc7f28758fb5a323c8524d7fc6660d46904d9b6cbe2809c419c`),
  so the harness only warns when its length differs. `/AcroForm /DR`
  has `/ZaDb` beside `/Helv`, for the check boxes' `/DA`. Bookmarks:
  every fifth section's entry is open, some children have `/C` or
  `/F`, and non-ASCII titles are UTF-16BE. Labels: roman for 12 pages,
  decimal from 1, and the last N/35 pages `A-1` on (no appendix under
  35 pages).
- **The scanned 7k's images are Flate gray,** not JPEG or CCITT: gen.py
  can't depend on PIL, the only imaging library here. Its header is
  `%PDF-1.4`. Both new fixtures carry `/Info` and an `/ID` (the md5 of
  the fixture's name and N); the synthetic still has none.
- **`IncrementalDocument` as planned, adjusted:**
  - lopdf's reader drops `/XRefStm` from the loaded trailer and
    `/Encrypt` from a decrypted document's, so `new_from_prev` clones
    neither. The spike removed both anyway, and a stale
    `/DecodeParms` an xref-stream base's trailer carries.
  - The synthetic base got an `/ID` before `Document::encrypt`, since
    RC4 and AES-128 keys depend on `/ID[0]`; the previous document was
    loaded with its password, since `IncrementalDocument` refuses an
    encrypted one; and the Full write took `/Encrypt` out of the
    trailer before `save_to`.
  - The AES-256 base got `/Length 256` in its `/Encrypt`, as Acrobat
    writes it; without it poppler can't read lopdf's AES-256 file at
    all (found below). PDFium and lopdf don't care.
  - New objects are numbered above the largest of `/Size` − 1,
    lopdf's `max_id` and the xref's highest entry (found below).
- **Appearances only is a flatten, not `render_form_data(false)`:**
  with `FPDF_ANNOT` and no `FPDF_FFLDraw`, PDFium draws no widgets at
  all, form environment or not (checked on the heavy 7k's page 0), so
  a plain render leaves them out. The spikes flattened what shows, as
  `render_appearances` does, and compared live renders beside it;
  live ran first, since the flatten changes the loaded page.
- **Checks the criteria don't name:** an edited page must render
  differently from the base (in at least one mode, since a widget over
  a whole page hides an edit live) and an untouched one identically,
  or a pass where neither side shows the edit would count; spike A's
  section must hold neither of the edit's strings in plain text (the
  Full write is checked to hold them); and where `pdftoppm` is
  installed, poppler renders spike A's overlay and Full write alike
  (PDF interoperability is a fixed constraint, and this found the
  AES-256 `/Length`).
- **Spike A also ran on 50 encrypted corpus files:** inventory rows
  with `encrypted=1` and status ok, in file order, at most 15 an
  algorithm, stopping at 50 files (75 rows), each decrypted with the
  empty password.
- **Spike B:**
  - Writer artifacts are separated per page: where overlay and Full
    write differ, the base is compared with lopdf's write of it with
    no edit, on that page and in that mode, and if those differ too
    the page is the writer's, not the overlay's.
  - Classes: classic table and xref stream are the newest section's
    kind (lopdf's `cross_reference_type`, the kind the section is
    written in); hybrid means `/XRefStm` in the bytes; linearized,
    `/Linearized` within 1,024 bytes of the header; damaged, lopdf's
    `xref_start` is 0 or lopdf and PDFium count different pages;
    encrypted, lopdf decrypted it with the empty password.
  - An untestable base (lopdf can't load it or write a section, or
    PDFium can't load the base) is listed and left out. If PDFium
    can't load or render the overlay, all its sampled pages count
    against the overlay; if the Full write fails, against the writer.
  - Memory was weighed on every base, not just the pool sample and
    the 7k fixtures; the cap's rule uses the pool sample's p99, and a
    class passes on memory if its p99 is within the cap.
  - Cold is the first render in a fresh engine, in a process that has
    already rendered the base once (PDFium's global fonts are
    loaded: the app is never cold); warm is the next render. Opens
    are medians of 5.
  - The synthetic and scanned 7k were compared, not only weighed, so
    slice 13's synthetic gate got an early read. The SafeDocs forms
    were drawn from 3,976, leaving out the one already in the pool
    sample. The sample already had 22 hybrid and 216 linearized
    files, so no pool picks were needed for those.
  - **A top-up run for damaged and leading junk,** 3 and 2 bases in
    the first run, mostly built: 90 pool files outside the sample, in
    inventory order, the first 50 with `%PDF-` past offset 0 and the
    first 50 whose `startxref` leads to no xref (by a scan of each
    file's head and tail; 10 are both), through spike B's driver with
    only those bases. The class rows are both runs' together. The
    review's reruns (this, the fill and the budget walks) rebuilt the
    spike from its sources outside the repo; `bench/spike/` stays
    gone.
- **Spike C** compared two more edits, the crop with a 90° turn and
  the turn alone through today's rotate parameter, since §10.1's job
  transform carries rotation deltas too; the criterion was judged on
  all three. The matrix was solved to compose bit for bit in PDFium's
  f32 arithmetic (`CFX_Matrix`'s product as chromium/7881 computes
  it), nudging each translation by ulps; a plain f64 inverse is
  mostly an ulp off. The written page went through
  `FPDF_RenderPageBitmap` at the page view's bitmap size, not
  pdfium-render's matrix path, which scales differently.
  Appearances-only comparisons were re-solved against the flattened
  pages' boxes (PDFium's flatten moves inherited boxes, found below),
  and pages were reloaded after the flatten (pdfium-render's stale
  handle, found below). Failures were sorted into writer, flatten
  moved the box, widgets live, inexact matrix, edge rounding,
  annotations outside and rotation.
- **The fallbacks taken:** C failed, so crops realize as overlays
  (§10.1). B failed on damaged bases only (the top-up below), which
  realize standalone, as §10.1 has them by design. A needs none.
- **What the spikes found stays in these notes,** not in "Found while
  planning": that list is the bugs planning found, each with the slice
  that fixes it, and these are measurements and rules for later
  slices, listed under "Left for later slices" by the slice that uses
  each. The one for another milestone, lopdf's AES-256 `/Length`, went
  into "Changes to other plans → m11.md", and `plan/m11-security/README.md` waits for
  slice 20 as that section says.
- **GROUP measures a scoped variant, a departure from §10.1:** the
  section's root `/Pages` lists only the group's pages and its
  AcroForm only the top-level fields with a widget on them. §10.1 has
  edited pages keep "their page index, AcroForm, OCProperties,
  inherited attributes and shared resources"; scoped, they keep the
  last three. In place no G fits the rule, because the heavy's engine
  loads all 7,000 fields. What scoping gives up, which slice 13 takes
  on (the docs list it too): the page tree is flattened, each page's
  `/Parent` still naming its own node, whose `/Kids` and `/Count`
  aren't rewritten (inherited keys resolve because PDFium walks
  `/Parent`; or write them out and re-parent, as §11.6 does for
  duplicates); `PageView.index` is the page's place in its group's
  engine, so whatever reads an edited page from its realization,
  §10.5's text included, goes through it; destinations into other
  groups resolve to no page in the engine (sheaf reads links from the
  file, not from engines); and a field whose widgets span groups is
  listed in each, with `/CO` and scripts blind to fields outside the
  group. Spike B gained a scoped pass and was re-run; its in-place
  results matched the first run's exactly. G was measured up to 256,
  so the rule's upper bound is measured; "near the base open" is
  within 0.5 ms, and an engine fits the cap with every page of its
  group previewed (the strip's state), not one.
- **The LRU count counts group engines only, pinned ones included,**
  for both panes of a split window. §10.3 caps "others", the
  unpinned, by count, but a count read from what is visible sizes the
  pinned set, so the verdict caps them together: two groups a pane,
  two panes, two for the strip. That never closes a visible group's
  engine, and with one pane keeps the 2 groups last left. The first
  verdict, 4 engines in 64 MB, left out the split view; the fill and
  the budget walk were run again at 6 engines and 96 MB with two
  panes pinned. Each visible source's base engine is pinned and
  outside the count (and grows without bound, found below).
- **The thumbnail fill rate is defined here,** since the plan doesn't
  define it; the review that added it said "the thumbnail strip fills
  at N or more pages per second within the engine byte cap". It is
  320 px previews (`PREVIEW_W`, docview.rs:70) of all 7,000 pages in
  order, every page behind a scoped group overlay with a header, a
  watermark and the crop in the section, through the screen's LRU,
  over the time spent in engine opens and renders: lopdf's section
  building is not slice 13's assembly. Slice 13's strip gate counts
  its own assembly in the time, which at 310 pages/s leaves about
  10 ms a group of 128 on the heavy. The scanned 7k isn't held to it
  (about 150 pages/s on the bare base, decoding its images).
- **The background budget's rule is two consumers at once.** The
  first rule, one OCR walk that opens no group twice, held for 1
  engine as well, so it didn't pick 2. Two walks taking turns (from
  the first page and from the middle, 1,024 pages each) reopen at
  every turn with 1 engine and never with 2, which is what 2 is for;
  three at once would share it. A document runs OCR, a job, a plugin
  run and a Trim White crop each on its own, so more than two can
  run, and 2 is recorded as chosen for two, not measured enough for
  more. The walks render at 300 dpi, sheaf-ocr's default, while the
  screen holds 6 engines pinned (groups 26 and 27 in one pane, 12
  and 13 in the other, 0 and 1 in the strip); groups the screen
  holds serve the walks without a second open.

Measured (AMD EPYC 7302P, 32 threads, release builds, vendored
libpdfium chromium/7881, pdfium-render 0.9.4, Python 3.14.4):
- **`bench.py … 7000`:** built in 454 ms. A: load, page count and
  close, median 2.06 ms; the size walk 59.36 ms (8.5 µs a page). B:
  page 3,500 at 1,024 px 5.06 ms, at 320 px 1.82 ms. C: text page
  1.27 ms. D: original + a 370-byte section, concatenated and loaded,
  median 2.40 ms (min 1.92; the copy 0.16 ms), the edited page
  reading "EDITED PAGE". E: two segments, median 2.18 ms (min 2.16).
  F: a scratch PDF, 0.04 ms, 1,096 bytes, 5.60 ms to load and render.
  G: `GenerateContent` after inserting a text object, 96.06 ms. These
  reproduce proposal 16 (A about 2 ms, D 2.0–2.4 ms, E 2.16 ms).
- **`real.py … pool-sample.txt`** (about 40 s): 402 files, median
  load 0.37 ms, max 16.59 ms; median page-0 render 31.54 ms, max
  1.94 s (the brief: 0.38 ms, 15.7 ms, 31.5 ms, 1.94 s).
- **`warm.py`, page 0:** f1040 first 23.1 ms, again 12.8, the next
  page 11.3, a fresh document 17.9; 2025-540: 13.9, 5.0, 9.9 and
  11.0 ms.
- **`form.py`:** f1040 load 0.70 ms, form environment 0.03 ms, render
  with `FPDF_FFLDraw` 21.81 ms; 2025-540 0.49, 0.01 and 15.05 ms. Over
  all government forms the form environment takes at most 0.35 ms
  (`uscis/i-9.pdf`), not the planning run's 0.14 ms.
- **The fixtures** (N = 7,000):

  | Fixture | Bytes | sha256 | Made in |
  |---|---|---|---|
  | synthetic | 3,510,145 | `297917810fe2447c611d20bf8de377184cf641a122f49ab5357be5fd2008142c` | 0.45 s |
  | heavy | 14,364,550 | `cfe374eee27177512c978b7ad78fab4951e502be688e17263824f5952c34b218` | 2.24 s (2.31 s wall), 86 MB peak RSS |
  | scanned | 496,665,679 | `198bb7fc386450396150667ab0af0a4d5471de6134b3192ed91e45054ff42361` | 1.51 s (1.56 s wall), 30 MB peak RSS |

  gen.py's docstring and `bench/README.md` pin them too. The heavy:
  70 sections and fonts (each subset about 77 KB, 22 KB compressed);
  21,000 annotations: 7,000 links, 7,000 markups (2,334 squares,
  2,333 highlights, 2,333 notes) and 7,000 widgets (6,300 text
  fields, 700 check boxes); 2,030 bookmarks; 3 label ranges; 52,392
  objects, 30,317 of them in 304 object streams. The scanned: images
  of 70,481 bytes on average. Page 0 renders in 19.6 ms (heavy) and
  19.8 ms (scanned) through `real.py`.
- **Baselines** (`SYNTHETIC` / `HEAVY` / `SCANNED`; ms unless
  marked), each the median of three runs of the final harness, and
  the ratio of one more run of the same build to them:

  | Row | Synthetic | Heavy | Scanned | Last run |
  |---|---|---|---|---|
  | geometry walk | 46.6 | 59.4 | 53.0 | 0.98–1.10 |
  | ready | 77.1 | 724 | 853 | 0.98–1.02 |
  | ready, peak RSS / added | 47.2 / 40.4 MB | 320 / 312 MB | 1,011 / 1,005 MB | 1.00–1.01 |
  | render 1,024 px, cold | 30.0 | 733 | 365 | 0.99–1.00 |
  | render 1,024 px, warm | 5.26 | 4.95 | 16.5 | 0.99–1.01 |
  | 200 previews at 320 px | 522/s | 171/s | 152/s | 1.00–1.02 |
  | incremental save, one mark (the file written) | 56.6 | 166 | 742 | 1.03–1.08 |
  | incremental save, peak RSS / added | 61.5 / 35.5 MB | 374 / 216 MB | 1,032 / 1,004 MB | 0.97–1.01 |
  | Rewrite (the bytes) | 300 | 3,082 | 1,201 | 0.95–0.99 |
  | Rewrite, peak RSS / added | 78.0 / 50.5 MB | 417 / 248 MB | 1,078 / 1,021 MB | 0.99–1.02 |
  | Rewrite, the file written | 3.27 MB | 17.7 MB | 473 MB | 1.00 |
  | crop all pages (copy written, engine opened) | 445 | 2,326 | 2,256 | 0.95–0.99 |
  | crop, the edit alone | 1.65 | 1.70 | 1.71 | 1.00–1.06 |
  | Bates apply (strip_stamps + apply_batch) | 114 | 270 | 428 | 0.95–0.99 |
  | Bates, the Rewrite | 750 | 3,979 | 1,708 | 0.94–0.97 |
  | flatten (markups only: links and widgets stay) | 336 | 3,489 | 1,495 | 0.88–1.02 |
  | flatten, peak RSS / added | 74.2 / 63.3 MB | 420 / 281 MB | 1,057 / 1,041 MB | 0.99–1.00 |

  History at 7k with 20k marks: AnnotAdd 0.686 ms, a full reorder
  0.945 ms (last run 0.697 and 0.946). 200 OCR versions of 1,000
  pages: 201 versions, 2 snapshots kept holding 0.016 MB, steps
  4,076 MB by estimate, RSS growth 4,738 MB (last run the same). The
  heavy's walk (59 ms) and ready (724 ms) already pass their gates;
  crop (445–2,326 ms against < 200), Bates apply (114–428 ms against
  < 50), the scanned's incremental save (742 against < 50) and the
  heavy's flatten RSS (420 MB against 221 MB) fail, as expected. The
  heavy's flatten is today's, which keeps the 7,000 widgets: slice
  19's `FlattenAnnotations { All }` flattens 7,000 more appearances
  than its 420 MB covers.
- **The first baselines were redone.** Review found that the Bates
  row timed only the `SetHeaderFooter` apply (0.007 ms), the
  incremental save only built its bytes (424 ms on the scan, now
  742 with the 497 MB write), crop timed a cache hit after the first
  run and opened no engine (1,722 ms on the heavy, now 2,326), and
  the heavy's flatten started from what the rows before it left
  (544 MB, now 420 MB in its own process). All three fixtures' rows
  were measured again on the final harness.
- **Code placement:** the rows that run today's writer (Rewrite,
  Bates' Rewrite, flatten, crop's copy) move across builds of
  gate_7k.rs and hold steady within one, with no library rebuilt
  between. The heavy's Rewrite has read 2.43–2.50 s, 2.93–3.09 s and
  3.49–3.56 s in three builds, Bates' Rewrite 3.17–3.25, 3.74–4.06
  and 4.53–4.62 s, flatten 2.94–2.95, 3.41–3.56 and 3.94–4.05 s, and
  the synthetic's Rewrite 253–254, 287–304 and 329–332 ms; review
  reports a build without LTO in the slowest. lopdf's load and save of
  the heavy take 40 and 115 ms of a Rewrite; the rest is sheaf-doc's
  own loops. perf is unavailable (`perf_event_paranoid` 4), so the
  loop isn't pinned down. The baselines are this build's, the middle
  placement, and its own reruns read 0.94–1.02 on those rows but the
  synthetic flatten's 0.88.
- **Spike A** (57 s, 491 MB peak; the docs have the table): 0 pixels
  differ on all five pages for RC4-128, AES-128 and AES-256, both
  ways. Overlay / base / Full write opens: 1.52 / 1.49 / 1.35 ms
  (RC4-128), 1.52 / 1.49 / 1.38 (AES-128), 8.14 / 8.09 / 1.37
  (AES-256); sections 2,840, 3,181 and 3,180 bytes. Corpus: 50 of 50
  pass (AES-128 15, AES-256 10, RC4-128 11, RC4-128 under V4 6,
  RC4-40 8; 30 xref-stream bases; median overlay opens 0.22–3.71 ms,
  within 0.02–0.09 ms of the base's). No AES-256 R5 file was seen.
- **Spike B** (883 bases in 222 s): 2,573 of 2,574 pages pixel-equal
  (99.96%), the one a lopdf writer artifact; every class at least
  99.40%, and no page differed because of the overlay (the top-up
  below changed that for damaged bases). Per engine:
  median 1.55 MB, p99 10.87 MB over 881 bases; the pool sample's p99
  10.87 MB sets the 16 MB cap. The 7k bases: synthetic 5.34 MB, open
  1.52 → 1.50 ms, cold render 23.0 → 24.1 ms; heavy 35.42 MB (over
  the cap), 8.66 → 9.36 ms, 738 → 751 ms; scanned 7.20 MB, 3.66 →
  3.67 ms, 37.3 → 63.8 ms. The class table is in the docs.
- **Spike B's top-up** (90 pool picks in 8 s): leading junk, with the
  first run's 2, 52 bases and 130 of 130 pages; of the 50 picks 37
  start with a UTF-8 byte-order mark, 18 are damaged too, and 32 load
  without a rebuilt xref. Damaged, with the
  first run's 3, 56 bases and 107 of 133 pages (80.45%): 26 pages of
  24 bases differ, none a writer artifact, every one damaged (22
  signed files lose page 0's visible signature stamp in the overlay,
  one a seal on a page left alone, and one shows no edit). Five picks
  were untestable (lopdf: two with no pages, three bad xrefs). Per
  engine: median 0.96 and 0.11 MB, p99 5.35 and 5.34 MB.
- **Spike C** (1,267 pages in 61 s): the crop is pixel-equal on 590
  (46.60%) live and 1,266 (99.92%) appearances only; with a 90° turn,
  549 and 1,167; the turn alone, 1,228 and 1,232. All 221 pages
  without widgets pass the crop both ways. Matrices exact in f32:
  3,364 of 3,801.
- **GROUP** (273 s; engine MB with every page of the group
  previewed, in place → scoped):

  | G | Synthetic | Heavy | Scanned |
  |---|---|---|---|
  | 8 | 5.01 → 0.94 | 35.16 → 4.02 | 7.19 → 1.82 |
  | 16 | 5.11 → 1.05 | 35.48 → 4.41 | 7.75 → 2.39 |
  | 32 | 5.32 → 1.28 | 36.20 → 5.21 | 8.88 → 3.54 |
  | 64 | 5.75 → 1.73 | 37.63 → 6.77 | 11.18 → 5.87 |
  | 128 | 6.62 → 2.68 | 40.44 → 9.89 | 15.74 → 10.51 |
  | 256 | 8.44 → 4.57 | 46.27 → 16.32 | 24.91 → 19.80 |

  Scoped opens are 1.36–1.60, 7.70–7.90 and 2.02–2.28 ms against the
  bare bases' 1.35–1.42, 8.58–8.73 and 3.59–3.69; scoped cold renders
  9.0–9.2, 8.8–12.8 and 28.2–28.7 ms against in place 23.7–24.9,
  736–744 and 47.4–48.8. A scoped section of 128 is 55–58 KB, built by
  lopdf in 22, 178 and 451 ms. Scoped renders matched in place on
  every preview, and the Full write on 1,982 of 1,982 corpus pages.
- **The verdicts:** GROUP = 128, scoped; LRU count 6 group engines,
  pinned ones included; engine byte cap 96 MB (16 MB an engine);
  background budget 2 engines and 32 MB (two walks taking turns: 14
  engines with 2, none twice, at most 16.8 MB, 106.9 s; 1,538 with 1,
  1,524 reopened, 136.3 s; one walk of 7,000 pages at 300 dpi in
  364.5 s, 49 engines, none twice, at most 20.6 MB; none of the
  screen's closed); thumbnail fill rate 310 pages/s (heavy 318,
  synthetic 389, through 6 engines and 96 MB with two panes pinned;
  55 engines opened, none twice, the screen's at most 48.5 MB; at 4
  engines and 64 MB it was 320 and 319 over two runs, at most
  33.7 MB). The rules are in the docs.

Found:
- **PDFium draws widgets only through `FPDF_FFLDraw` or a flatten,**
  and `FPDF_FFLDraw` takes no matrix: a render-time box edit can't
  draw live widgets (C's main failure, 676 of 1,046 widget pages).
  This bears on §10.1's `FormDraw`.
- **Object numbers start above `/Size`, not lopdf's `max_id`:** on
  `pool/safedocs/5801/5801985.pdf`, lopdf's `max_id` and merged xref
  stop at 76, `/Size` is 78 and the newest xref stream is object 77.
  Numbered from 77, the section's content stream was read by PDFium
  as the base's xref stream, and page 0 rendered live without its
  text (3,152 px). Slice 13's allocator and slice 10's Append numbering
  must start above `/Size` and every xref stream's own number.
- **A two-segment reader must fill each read across the boundary:**
  pdfium-render hands each PDFium block request to one `Read::read`,
  and PDFium takes any nonzero count as the whole block.
- **An AES-256 (R6) source pays its key derivation on every load:**
  8.1–8.2 ms against 1.5 ms at 7k, so a group-engine reopen on one is
  about 8 ms, not proposal 16's 2 ms.
- **What makes a fresh engine slow:** the first `FPDF_LoadPage(k)` in
  a document loads every page dictionary up to k (19.8 ms at page
  3,499 of the synthetic, 26 ms on the heavy; neighbours take
  0.13–0.18 ms), and the first `FORM_OnAfterLoadPage` loads every
  field (702–712 ms and 25 MB on the heavy: 35.41 MB an engine with
  the form environment, 10.39 MB without). pdfium-render makes a form
  environment on every load. Scoped group sections avoid both.
- **Slice 13's heavy realize gate can't pass as written:** scoped,
  the heavy's open and cold render take 7.86 + 10.1 = 18.0 ms, 2.9×
  its warm render in that run (6.12 ms) and 3.6× the harness's
  4.95 ms, against ≤ 1.7×; its bare open alone, 8.6 ms, is about 1.7×.
  The synthetic's open and cold render take 1.49 + 9.1 = 10.6 ms
  scoped (25.6 in place), which leaves 4.4 ms of its < 15 ms for the
  realize itself, the section's assembly included; the spike didn't
  measure that.
- **A damaged base's overlay isn't its Full write:** in the top-up,
  22 damaged, signed files lose page 0's visible signature stamp in
  the overlay, one loses a seal on a page left alone, and on one the
  edit doesn't show. §10.1 already realizes damaged bases
  standalone.
- **The base engine isn't bounded by the LRU:** after previewing
  every page it holds 91.4 MB (synthetic) and 338.5 MB (heavy).
- **A rotation delta through the render's rotate parameter isn't the
  written `/Rotate`** on 38 of 1,267 pages (images resampled; on 6
  forms, widgets drawn differently by `FPDF_FFLDraw`). With
  `RotatePolicy::KeepBase` the job carries the delta, so an overlay
  of a turned page won't be pixel-equal to the saved file there.
- **lopdf's writer recolours page 0's logo of
  `pool/safedocs/2932/2932995.pdf`** (AES-128, xref stream,
  linearized) with no edit at all; the overlay matches the base.
- **lopdf's AES-256 (V5) `/Encrypt` has no `/Length`,** and poppler
  26.01 then garbles every stream, of the base alone and of an
  overlay; with `/Length 256` it reads both. Acrobat-made AES-256
  files in the corpus have it. Overlay sections are unaffected (they
  reference the source's `/Encrypt`).
- **PDFium's `FPDFPage_Flatten` writes `/MediaBox` and `/CropBox`
  from the page's own keys only:** a page that inherits a box other
  than letter comes out 612 × 792 (`forms/safedocs/0163162.pdf` page
  2: 419.5 × 595.3), on 9 of 3,801 transforms in C.
  `render_appearances` (sheaf-render) sizes its render from the
  flattened page, so it renders such pages in the wrong box.
- **pdfium-render 0.9.4's `PdfPage::flatten`** reloads the page under
  a new handle but `boundaries()` keeps the closed one: reading a box
  after a flatten is a use-after-free (a SIGSEGV on 25 of 880 bases
  until the spike reloaded the page).
- **lopdf can't load two pool files:** `0676477` (0 pages; PDFium
  finds 47) and `2289457` ("invalid start value" in the xref).
- **Edit placement:** a widget over a whole page (5588122, page 765)
  hides any edit in a live render, and a 28 pt-wide page (2494340,
  page 60) put a fixed 36 pt offset off the page; the spike scaled
  its edit to the visible box and checked both modes.
- **Today's eager open of the scanned 7k** peaks at 1,012 MB, the
  whole file, and a Rewrite of the heavy spends all but about
  0.16 s of its 2.4–3.6 s outside lopdf's load and save.
- **The history budget can't hold 200 OCR versions today:** 4,076 MB
  of steps by estimate and 4.7 GB of RSS growth, with 2 snapshots
  kept, since `MemoryBudget` can only drop snapshots (slice 1's
  note).

Checked:
- The harness on all three fixtures, once more at the slice's end on
  the build the baselines came from: every row prints with its
  baseline and ratio, 0.88–1.10; the history bench's asserts pass.
  The regenerated fixtures kept their sha256s.
- The verdict section has every number filled in: no TBD, `?` or
  placeholder in it.
- `bench/spike/` is gone, sources, `Cargo.toml`, lock file and its
  565 MB target directory; `git status` shows only `bench/` (the
  scripts, `pdfium.py`, the sample, the README and `.gitignore`),
  `gate_7k.rs`, `history_tests.rs`, `docs/document-model.md` and this
  plan.
- `python3 -m py_compile bench/*.py`, `cargo fmt --all -- --check`,
  `cargo clippy --workspace --all-targets -- -D warnings` and
  `cargo test --workspace` (554 passed, 16 ignored) pass. CI needed
  no change: the harness is ignored.

Left for later slices:
- **Slice 3:** assert the geometry walk (< 100 ms synthetic, 46 ms
  today; < 250 ms heavy, 59 ms). The lazy reader should load the two
  pool files lopdf can't (`0676477`, `2289457`) and bring the
  scanned's ready RSS (1,011 MB) down.
- **Slice 4:** assert ready < 1 s on the heavy (724 ms today).
- **Slice 8:** the linear scans that likely make most of today's
  Rewrite time (all but 0.16 s of the heavy's 2.4–3.6 s is sheaf-doc's
  loops).
- **Slice 9:** assert Rewrite ≤ 1.3× (flag past 1.2×)
  `HEAVY.rewrite_ms` and `SYNTHETIC.rewrite_ms`, and the RSS it adds ≤ 1.5×
  `rewrite_added_mb` (248 and 50.5 MB), minding code placement: a
  build of the harness can read 0.8–1.2× with nothing changed, so
  time both writers in one build before today's is deleted. Check
  whether the new emitter keeps 2932995's logo.
- **Slice 10:** assert the incremental save of one mark < 50 ms on
  the scan (742 ms today, writing the whole file). Number Append
  objects above `/Size` and every xref stream's own number.
- **Slice 12:** assert 200 OCR versions within 256 MB (4,076 MB of
  steps today).
- **Slice 13:**
  - overlays with these numbers: GROUP 128, group sections scoped
    (root `/Pages` and AcroForm listing only the group), the screen's
    6 group engines and 96 MB, pinned ones included, 16 MB an engine,
    a background budget of 2 engines and 32 MB;
  - scoping departs from §10.1, whose edited pages keep their page
    index and AcroForm: `PageView.index` is the page's place in its
    group's engine, and every read of an edited page goes through it;
    each page's `/Parent` names a node the section doesn't rewrite
    (write the inheritable keys out and re-parent, or rely on PDFium
    walking `/Parent`); destinations into other groups resolve to
    nothing in the engine; a field whose widgets span groups is
    listed in each, and `/CO` or scripts can't see fields outside;
  - sections encrypted with the source's key (A passed: no decrypted
    image); standalone groups for damaged bases only (B failed on
    those alone, as §10.1 expects); crops realized as overlays (C
    failed: no box edits on the base engine);
  - `Segments` filling reads across the boundary, the allocator
    above `/Size`, and a bound on the base engine (count it against
    the byte cap, or reopen it: 1.4–8.6 ms);
  - the gates: realize + open + render < 15 ms synthetic, where open
    and cold render took 10.6 ms, leaving 4.4 ms for the realize and
    its assembly; restate the heavy's ≤ 1.7× before asserting it,
    which no design that opens an engine meets (an absolute bound
    such as < 25 ms fits the 18.0 ms measured, with the realize
    still to add); engines after 200 groups within 96 MB; the
    thumbnail strip ≥ 310 pages/s within the cap on the synthetic
    and heavy, with the sections' assembly counted in its time
    (about 10 ms a group of 128 is what the rate leaves);
  - the corpus "overlay ≡ saved file" check takes in leading-junk and
    damaged files (the top-up's picks), the latter through the
    standalone path;
  - the "overlay ≡ saved file" check on turned pages will see the
    rotation-delta differences, and its appearances-only renders
    need `render_appearances` to keep inherited boxes (render at the
    page's box from before the flatten).
- **Slice 15:** assert crop commit < 200 ms (445–2,326 ms today).
  C failed, so a crop is geometry that realizes as an overlay, not a
  box-only render on the base engine.
- **Slice 17:** assert Bates apply < 50 ms (114–428 ms today, nearly
  all of it `strip_stamps` loading every source), and add the
  live-token reorder row.
- **Slice 19:** assert the flattened export's peak RSS ≤ 1.5× file +
  200 MB, in the flatten's own process (the heavy: 420 MB against
  221 MB, 281 MB of it the export's), and switch the row to
  `FlattenAnnotations { All }`, which flattens the 7,000 widgets
  today's flatten keeps.
- **Idle CPU** (slices 12 and 13) is the app's, with no harness row.
- **M11:** new AES-256 encryption writes `/Length 256` in V5's
  `/Encrypt`, now a bullet under "Changes to other plans → m11.md",
  which slice 20 applies.
- **Any slice:** code that reads a box after pdfium-render's `flatten`
  must load the page again.
