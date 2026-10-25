# M5 slice 1: The intake pipeline

Status: done · Part of [M5: Scan & OCR](../../m05-scan/README.md)

**The intake pipeline** (proposals 1 and 3; F14; findings 1 and 6).
- **Builds:** `sheaf_ops::intake` (`Pages`, batches of 8 pages or 2
  seconds as one job group, a `Landing` trait with the Session's);
  `sheaf_ops::scan` as intake over image files; pages encoded as
  they're cleaned (G4 for black and white); the two cleanup profiles
  and `sheaf scan --profile`; `JobKind::Scan` and `GroupKind::Scan`;
  `State::scan` split by profile.
- **Deletes:** `scan`'s collect-then-insert path and its
  `Vec<PageImage>`.
- **Checks:**
  - F14: `sheaf scan --ocr` on three images finds their words, one
    OCR block a page, one font, on every CI runner.
  - Memory: 500 synthetic black and white Letter pages at 300 dpi
    scan with a peak under 400 MB above the process's start (today's
    path would need 4.2 GB).
  - The same images make the same pages as before the slice: each
    page rendered and compared with the old path's at 0 pixels of
    difference, and the same text layers.
  - The scanner profile: a synthetic flatbed scan (a white page with
    text on a white lid, 3° of skew) comes out straightened and not
    cropped; with the photo profile, the slice notes say what it did
    (finding 6).
  - An intake is one undo step however many batches it took, and
    two if the user edits in between.

## Notes

Done. The intake pipeline (proposals 1 and 3; F14; findings 1 and
6): scanned pages go into a document a page at a time through
`sheaf_ops::intake`, from a `Pages` stream (image files here, a TIFF's
pages one by one through the new `sheaf_image::io::frames`; devices
from slice 6), through cleanup, blank pages and OCR. Each page is
encoded the moment it's cleaned (black and white as CCITT G4, held so
from construction as `Pixels::G4`; color and gray as JPEG) and its
pixels dropped. Pages land in batches (`Batching`: 8 pages, or 2 s
since the last landing; the command line by count only) through a
`Landing`. The Session's commits each batch, an `InsertPages` of a
`Memory(Images)` blob plus its pages' `SetOcrText`, as one commit of
`Actor::Job(JobKind::Scan)` in one `GroupKind::Scan` group, so an
intake is one undo step however many batches it took, and undoing it
stops it. `sheaf scan` is intake over files into a Session, then a
Rewrite, and a multi-page TIFF now gives all its pages. Cleanup has
two profiles, Photo (today's) and Scanner (no page finding, no light
evening), with `sheaf scan --profile photo|scanner` and the
`--crop`/`--flatten` pairs, `State::scan` split by profile and the
Create from Scans dialog's "Pages from" row. F14 is a CLI test of
`scan --ocr` on every runner (backlog: Closed). 500 black and white
Letter pages at 300 dpi scan with under 400 MB added in both profiles
(the old path: 4.2 GB); the same images make the same pages as before
at 0 px with the same text and the same word boxes; every asserted
gate holds, and the corpus counts are #26's. Whatever stops an intake
but its landing lands the pages it had ready first.
- **G4 from construction, `io::frames`** (step 1). sheaf-image io.rs:
  `Pixels::G4(Vec<u8>)` in place of `Pixels::Bilevel`, encoded once by
  `PageImage::from_image` for an all-0/255 gray image (`encode::g4`);
  sheaf-doc's `images::image_stream` passes the bytes through under
  the same `CCITTFaxDecode` dictionary. `pub fn frames(path) ->
  Result<Frames>` (`Frames: Iterator<Item = Result<RgbImage>>`): the
  kind sniffed from the first 16 bytes; a TIFF's first page
  `load(path)`'s image, and its later ones read an image a `next()`
  by a `tiff` decoder over a `BufReader<File>` (boxed), each turned
  upright by its Orientation tag and made RGB as `load` does; anything
  else one frame, `load(path)`, read on the first `next()`; an error
  ends it. `tiff_pages`' per-image decode moved into `tiff_image<R:
  Read + Seek>`, which both use. `tempfile` a dev-dependency (CI's
  pixels-only gate checks normal dependencies). Tests:
  `a_black_and_white_page_is_held_as_g4` (a 2550x3300 page's G4 bytes
  `g4(raw)`, under 1% of its 8-bit size),
  `a_tiff_gives_its_pages_one_at_a_time`,
  `any_other_image_is_one_frame_as_load_reads_it`; encode.rs'
  `tiff_round_trips_through_the_reader` and images.rs'
  `png_gray_and_bilevel` on `Pixels::G4`. (The review's fixes
  replaced step 1's `a_tiff_frame_is_what_load_reads`: Fixes after
  review.)
- **The scan job and its group** (step 2). caps.rs: `JobKind::Scan`,
  which `actor_allows` lets make `InsertPages` and `SetOcrText` and
  nothing else, `actor_exports` false; the arms kept apart from the
  plugin's (slice 20 edits the same file). model.rs: `GroupKind::Scan`,
  which takes `Job(Scan)`'s commits only (`takes`). fixture.rs'
  `commit_ops_in_group` commits a Scan group's ops as the scan job.
  history_tests.rs: an `unreachable!` arm (Deviations). Tests:
  `caps_cover_every_op_target_and_actor` with `Job(Scan)`;
  group_tests.rs' `a_scan_group_takes_the_scan_job_s_batches` (three
  batches by `PageRef::New` and `PageRef::Pos`, the first not amending
  and the next two amending, one version, a `Job(Ocr)` commit in the
  group a bug, a `DeletePages` by the scan job `Refused(Cap::Plugin)`,
  one undo back to the start), the catch_unwind check shared with
  `a_commit_in_a_group_by_another_actor_is_a_bug` as
  `commit_is_a_bug`.
- **The intake** (step 3). sheaf-ops intake.rs (new): `RawPage { image,
  name }`; `trait Pages { next(p), count() }`; `Files::new(&[PathBuf])`
  ("no images given" for none; a TIFF's later pages named "…, page
  k"; progress by file, by page since the review); `Batching {
  pages, wait }`, its `DEFAULT` and the pure `due(pages,
  since_last)`, an empty batch never due; `Batch { pages, words, fit,
  engine }`; `trait Landing`; `SessionLanding {
  new, session, finish }`, the group begun by `new` and ended by
  `finish`, whose `land` applies the insert, takes the `Made::Page`
  ids and their geoms from `tx.state()`, applies a `SetOcrText` a page
  on the same Tx and commits once (`CommitOpts { group: Some(g),
  ..labelled }`), a `GroupEnded` coming back as that error;
  `intake(pages, opts, engine, landing, batching, p)`: check, clean,
  the turned and blank messages as before, OCR of the cleaned image,
  then the page encoded (black and white through `from_image`, which
  takes the image by value) and its pixels dropped, a batch landed
  when `due` says and the rest at the end, a landing's error stopping
  it before another page is read (and, since the review, any other
  error landing the rest first). `made_pages` (pub(crate); images.rs
  calls it there), `to_page` and `JPEG_QUALITY` moved there from
  scan.rs. scan.rs' `scan()`: `Files`, the engine built as before,
  `Session::new_empty`, `SessionLanding::new(…, "Scan")`, `intake` by
  `BATCHING`, `finish`, "every page came out blank" when none landed,
  the Rewrite to `out`. Tests (sheaf-ops tests/intake.rs, new: seeded
  850x1100 pages, a fake engine `Lines` reading a word a run of dark
  rows, orientation off, `wait: None`):
  `batches_make_the_pages_one_batch_makes`,
  `an_intake_is_one_undo_step`,
  `an_edit_between_batches_makes_two_undo_steps`,
  `undoing_an_intake_stops_it`, `a_multi_page_tiff_scans_page_by_page`;
  intake.rs' `a_batch_is_due_at_its_count_or_after_its_wait`.
  docs/document-model.md's scan bullet.
- **Two cleanup profiles** (step 4). scan.rs: `Profile::{Photo,
  Scanner}` (kebab-case, Photo the default, proposal 3's table in its
  doc), `Profile::options()` (Photo `ScanOptions::default()`, Scanner
  the same with `crop` and `flatten` off), `ScanChoices { profile,
  photo, scanner }` (`#[serde(default)]`, `get`, `set`). sheaf-cli
  scan.rs: `--profile photo|scanner` (photo by default), `--crop`/
  `--no-crop` and `--flatten`/`--no-flatten` (`overrides_with`: the
  last given wins, and with neither the profile decides), `--mode`,
  `--page`, `--dpi` and `--threshold` as `Option`s, all through
  `options(&ScanArgs)`; the "N cropped from the photo" count printed
  only when crop is on; the help for both kinds of pages. App:
  `State::scan` a `ScanChoices`; the dialog's first row, "Pages from"
  ("Photos", "A scanner or copier"), a switch keeping the shown
  profile's edits for a switch back, loading the other's (`usable()`
  turning off OCR and curl where they can't run here) and refreshing
  the preview; `ScanEvent::Created { path, profile, opts }`, which
  saves that profile's options and the profile. Tests:
  `the_scanner_profile_neither_crops_nor_evens_the_light`,
  `each_profile_keeps_its_own_choices`, the CLI's
  `the_profile_decides_what_no_flag_does` and
  `of_a_flag_and_its_no_the_last_wins`, tests/intake.rs'
  `the_scanner_profile_keeps_a_white_page_on_a_white_lid_whole`;
  state.rs' sample with the Scanner profile. README's `scan` and
  "Create PDF from images" rows.
- **The memory gate** (step 5). gate_7k.rs:
  `scan_of_500_black_and_white_pages` (ignored, release),
  `LetterScans` (a `Pages` making each seeded page as it's asked for)
  and `letter_scan(seed)` (2550x3300, off-white paper with noise, 36
  lines of words, up to 1° of skew, no two pages or batches alike);
  each profile's row inside `with_mem`: a Session, a `SessionLanding`,
  `intake` without OCR by `scan::BATCHING`, `finish`, the Rewrite to
  `target/gate-7k/`; after it, outside `with_mem`, the report's 500
  pages and lopdf's read of 500 pages in the file. Rows: pages,
  pages/s (cleaned, landed and written), peak RSS, RSS added (< 400
  MB, asserted for both on every run), MB written; n/a off Linux.
  `scan::BATCHING` public. bench/README.md's harness paragraph.
- **F14** (step 6). sheaf-cli tests/scan.rs (new):
  `scan_ocr_finds_the_words_in_one_block_a_page_and_one_font`. lopdf
  writes three Letter pages (36 pt Helvetica headings, "Invoice 4471
  Paid", "Receipt Lumber Yard" and "Mileage Log March", over 8–11
  lines of ordinary text at 14–18 pt); `export --format png --dpi 300`
  makes three PNGs; `scan -q --ocr --profile scanner` scans them,
  orientation on. Checked: 3 pages; every heading word on its page in
  `export --format text`; exactly one `Marker::Ocr` stream in each
  page's /Contents; one font object across the pages' resources, its
  /BaseFont GlyphLessFont. It runs in `cargo test --workspace` on all
  three runners (sheaf-cli's default features build Tesseract, and CI
  fetches the OCR data), and without the data the scan fails, never
  skips. F14 moved to the backlog's Closed.
- **Wrap-up** (step 7). retired.rs: `RETIRED_M5` (`scan::clean_one`,
  `io::Pixels::Bilevel`) beside M10.5's `RETIRED`, both in `LEDGERS`
  with the words a hit names its slice by ("retired in M5 slice 1"),
  each file parsed once for both (`scan_file`); "Bilevel" in
  `QUALIFIED_ONLY`; the module doc. docs/document-model.md: `Job(… |
  Scan)` and what the scan job may do, `GroupKind::Scan` in the code
  block and the Groups paragraph, the history overview, the scan
  bullet's G4. This plan's text: proposals 1 and 3 and findings 1 and
  6, as built.
- **Deleted,** confirmed dead by `cargo check --workspace
  --all-targets` and clippy, the names in retired.rs' `RETIRED_M5`:
  `scan()`'s collect-then-insert path (its `Vec<PageImage>` and
  `Vec<Vec<OcrWord>>` held to the end, the one `made_pages` of every
  page, and the second commit, `commit_as(Job(Ocr), …, "Recognize
  Text")`); `scan::clean_one`; `Pixels::Bilevel`. `rg` finds neither
  name outside retired.rs, and "Recognize Text" in sheaf-ops only in
  prefs.rs' doc of the OCR dialog's settings.

Deviations from the plan (the planner's list first, as resolved):
- **Multi-page TIFFs:** proposal 1's "a TIFF's pages one by one"
  wasn't so: today's scan read each input with `io::load`, the first
  image alone (Found). `io::frames` gives a TIFF's first page as
  `load(path)`'s image, so every TIFF `scan` read before (and so every
  single-page one) makes the same page, byte for byte, whatever its
  kind (CMYK8, CMYK16, float RGB, planar, an Orientation tag that
  isn't one value, up to image's 512 MiB); its later pages, which the
  old path lost, are decoded one at a time from the file through
  `tiff_pages`' own decode (`tiff_image`). Those equal `load` of the
  same image alone exactly for RGB, gray, G4, 16-bit gray and every
  Orientation; CMYK8 within 1 (`tiff_pages`' integer conversion,
  which must stay byte for byte, and image's float one differ by −1
  on 807 of the 65,536 (c, k) pairs, computed exhaustively); CMYK16,
  float RGB and planar later pages are an error, never a wrong image
  (Found). Step 1 had read the first page through `tiff_image` too,
  which refused those kinds and pages past tiff's own 256 MiB limit,
  and differed by 1 on CMYK8: the review's findings 5, 6, 7, 8, 10
  and 15 (Fixes after review).
- **`tiff_pages` and the Orientation tag:** it does ignore it (Found);
  its output is unchanged here, and frames match `load`, as the test
  pins.
- **`intake::RawPage { image: RgbImage, name }`,** not proposal 9's
  `RawPage { image: PageBytes, dpi, sheet, side }`, which belongs to
  sheaf-scanner (slice 6) and doesn't exist yet; the name is for
  messages ("x.png: blank, left out"). Slice 6 maps a device's pages
  into it and may add dpi, sheet and side then.
- **The 2-second rule:** `Pages::next` is a blocking pull, so a timer
  can only be read between pages: a batch lands once it holds `pages`
  pages, or once `wait` has passed since the last landing (the
  intake's start, for the first), checked as each page is done. A slow
  device's page lands right after it's cleaned. The rule is the pure
  `Batching::due(pages, since_last)`, unit-tested.
- **Determinism: the command line batches by count only.** The saves
  aren't byte-identical across batchings: a Rewrite numbers what an
  insert brought by its source key (`ObjKey::Base { blob, .. }`, the
  blob a content hash; materialize/number.rs, "the rest by key"), so
  the same pages inserted in other batches are written in another
  object order. By design, not a bug. So `scan::BATCHING` is
  `Batching { pages: 8, wait: None }`, and the same images make the
  same file (OCR's block ids aside, drawn at random, M10.5 §4);
  `Batching::DEFAULT` (8 pages or 2 s) is the app's and the devices'
  (slices 2, 6 and 7), unused but by its unit test until then. The
  batch test compares PDFium renders (0 px), page text, character
  boxes and OCR layers, and asserts the same batching twice writes the
  same bytes (without OCR). Proposal 1's text says so now.
- **One commit a batch:** today's scan set its text in a second
  commit by `Job(Ocr)` ("Recognize Text"), two undo steps. A batch's
  `InsertPages` and `SetOcrText` are one commit of `Job(Scan)`, which
  `actor_allows` lets make those two ops alone. `BlobSel::Id` resolves
  for job actors (only plugins are refused, resolve.rs).
- **The text and its pages in one transaction:** the insert applied
  first, its pages taken from `OpResult.made` (`Made::Page`) and their
  geoms from `tx.state()`, the `SetOcrText`s then applied on the same
  Tx, one commit. A page OCR finds no words on still gets an empty
  layer, as before.
- **G4 at construction, for every reader** (scan, images-to-pdf,
  `PageSource::Images`, black and white PNG and TIFF reads, stamp
  images): the bytes are the same function's, so nothing written
  changes (step 1's cmp of the baseline, step 3's comparison).
  `encode::TiffPage::Bilevel` stays, so "Bilevel" is in
  `QUALIFIED_ONLY`.
- **Intake takes a built engine** (`Option<&mut dyn OcrEngine>`;
  `scan()` builds it from `OcrConfig` as before): `cargo test -p
  sheaf-ops` has no OCR engine, while `cargo test --workspace` (CI)
  unifies sheaf-cli's features in, so sheaf-ops' tests there have
  Tesseract's OSD and the orientation model. The sheaf-ops tests use a
  fake engine with orientation off; the real OCR check is sheaf-cli's
  (F14).
- **history_tests.rs** matches `group.kind` exhaustively, so
  `GroupKind::Scan` has an arm there, `unreachable!` ("the runs made
  here are OCR's and plugins'"). The generator stays OCR-or-plugin:
  adding Scan would change the property's random stream, and the
  existing cases, the 4,096-case release property
  (`prims_rebuild_every_version_4096`) and the saved seed would no
  longer test what they did. Scan groups are covered by group_tests.rs
  and tests/intake.rs.
- **The memory gate has no env var:** the plan names none, and it's a
  peak RSS, which needs no quiet machine, so it's asserted on every
  run, as history's budget is. It's in gate_7k (release, ignored)
  because 1,000 Letter cleanups take half an hour, and it's asserted
  under both profiles: the plan's 4.2 GB is today's `sheaf scan --mode
  bw`, the Photo profile (the command's default), and the Scanner
  profile is the devices'. It lands pages as the command line does
  (`scan::BATCHING`, public for it).
- **retired.rs' M5 table:** the ledger is M10.5's, `(&str, u8)` rows
  whose hits print "retired in slice N", so M5's rows would read as
  M10.5's slices. They're a second table, `RETIRED_M5`, printed
  "retired in M5 slice N", scanned by the same test; the module doc
  says so.
- **`State::scan` by profile, with a dialog row:** the dialog had no
  profile, so the scanner choices would have been dead state until
  slice 7. A minimal "Pages from: Photos | A scanner or copier" row
  swaps in each profile's remembered options. Created saves the
  profile it created with and its options; edits to the other profile
  while the dialog is open last only while it's open. The labels "As
  photographed" (`Paper::Original`) and the "Photo" preview view stay
  under the scanner profile (For Greg).
- **Blank pages and duplex:** files have no duplex backs, so neither
  profile leaves out blank pages for files; proposal 3's "on with
  duplex" is slice 6's device settings' (the plan's table says so
  now).
- **state.json:** an older flat `scan` object parses as `ScanChoices`'
  defaults, its keys ignored (pre-alpha and disposable: no shim). A
  partial `scanner` object fills its missing fields from
  `ScanOptions::default()`, the Photo profile's, so a hand-edited
  `"scanner": {"dpi": 200}` would get crop and flatten on; sheaf
  always writes the whole object.
- **Crash safety on the command line:** the Session is in memory, so a
  cancelled `sheaf scan` of files writes nothing, as before; the
  images are still on disk. A device's pages are another matter
  (slice 6, Left). The intake itself keeps the pages so far on any
  stop but its landing's own (Fixes after review), which the app's
  landing (slice 2) and the devices (slices 6 and 7) build on.
- **A cancel cuts the page in flight short:** cleanup and OCR check
  the cancel, so the page a cancel comes during is left out and the
  pages before it land. For files that loses nothing (the image is on
  disk); for a device, whose page in flight is already scanned,
  proposal 10's "stops after the page in flight" may mean finishing
  it first (slices 6 and 7, Left).
- **`made_pages`** moved to intake.rs as pub(crate), and no public API
  nothing calls was added (the app's landing, slice 2, adds what it
  needs); `scan::BATCHING` is public for the gate, the only addition
  of the kind.
- **The intake tests' cleanup:** crop, flatten and deskew off as well
  as orientation (`Paper::Original` at 100 dpi). In the dev profile a
  cleanup with them takes about 1.8 s an 850x1100 page, without them
  about 50 ms, and the batch test cleans 270 pages; the tests are
  about landing, and the flatbed test covers deskew.
- **The two-undo-steps test** starts from one blank page of the user's
  (a `Cli` insert) and turns it between batches 2 and 3, so the
  history reads "Blank", "Scan", "Rotate", "Scan (continued)", and
  `revert_group` takes back every scanned page and leaves the turn
  (turning a scanned page and reverting would take the page itself).
  The group comes from the timeline (`VersionView.group`), as
  History's "Undo all of" finds it: no public API added for it.
- **Progress counts pages:** `Files::new` opens each file and counts a
  TIFF's pages from its headers (`io::count_frames`, no image
  decoded), so a file that can't be opened or whose headers can't be
  read is an error before any page is read; the intake sets its count
  after each page, in `PAGES`. Step 3 counted files, so a multi-page
  TIFF's bar ran to the end on its first page (review finding 11).
- **`SessionLanding` has no Drop:** an intake that fails without
  `finish()` leaves its group open until the session goes, and
  `scan()` drops the session on an error, so nothing sees it today.
  Slice 2's landing should end its group on failure (Left).
- **The CLI's other options** (`--mode`, `--page`, `--dpi`,
  `--threshold`) became `Option`s, their defaults in their help
  ("(default: color)", as `--engine` already was), so the profile
  decides whatever isn't given; the two profiles agree on them today,
  so nothing written changes. A repeated flag (`--crop --crop`) is
  refused, as any repeated flag already was; `--crop --no-crop
  --crop` is accepted, the last winning.
- **The flatbed test's light assertion:** step 4's fault check (crop
  back on in the Scanner profile) can't fail the flatbed test, since
  crop does nothing on that scan (finding 6, Measured). It fails the
  profile's unit test, which asserts `!crop`, and the flatbed test
  also asserts the light is left as scanned (paper within 3 of 247,
  lid within 3 of 252, the paper darker), which fails with flatten
  on (254, 254) and with deskew off (3° left).
- **F14's font fault** was made in emit/mod.rs' `Sink::resource`
  (each call's first object keyed anew), the function that writes a
  drawn resource once a file; content/font's `Glyphless::resource`
  would have made a font a `Tf`, a word, not a page. (c) was shown to
  fail under the first fault with (b)'s loop skipped for one run, as
  the test stops at its first failing assert. (d) also checks the
  font is GlyphLessFont.
- **Docs in their steps:** steps 3 and 4 changed
  docs/document-model.md's scan bullet and README's rows in the step
  that changed what they described; step 5 bench/README.md's.
- **The memory gate's output for `qpdf --check`:** the gate removes
  its file, so the wrap-up's gate run carried one temporary line
  copying each row's output aside before the removal (after the
  measurement and outside `with_mem`); gate_7k.rs was restored from
  its copy and checked with cmp, and the release binary rebuilt.
- **The extra checks the steps added:** the scan job making any op
  (step 2's second fault check); the gate's two sanity reads (500
  pages reported, 500 in the file).

Found while building:
- **A multi-page TIFF lost every page but its first** in `sheaf scan`
  and Create from Scans (`io::load` decodes one image), silently:
  fixed here, through `io::frames`.
- **`tiff_pages` ignores the TIFF Orientation tag,** so images-to-pdf,
  Combine and `PageSource::Images` show an oriented TIFF unturned,
  while `load`, `decoded` and now `frames` turn it. Not changed here.
  Backlog B14.
- **The Combine dialog's background `inspect`** (combine_dialog.rs:119)
  decodes every image just to count its pages, and now G4-encodes the
  black and white ones too; it could count a TIFF's IFDs instead.
  Backlog B17.
- **Some TIFF kinds `load` reads are refused by `tiff_image`**
  (CMYK16, 32-bit float RGB and RGBA, planar), in images-to-pdf,
  Combine and `PageSource::Images` already, and in a scan for a
  TIFF's later pages, which the old scan never read (Deviations).
  `tiff_image`'s `GrayA` arms are never reached: tiff 0.11 reports a
  two-sample gray image as `Multiband`, which neither it nor `load`
  reads (the review's finding 5 counted GrayA16 among what `load`
  reads; it isn't). Backlog B15.
- **`sheaf-cli`'s `progress::run` hangs after a panic in its work:**
  the panic unwinds before `done.store(true)`, so `thread::scope`
  waits forever for the drawing thread, at 0% CPU (the review's
  finding 7 found it through a debug build's overflow, fixed here);
  `done` could be set by a drop guard. Backlog B16.
- **`images_to_pdf` reads every image before it writes** (`read` →
  `Vec<PageImage>`): G4 at construction shrinks its black and white
  pages, but gray and RGB pages (a PNG's pixels) are still held raw.
  Backlog B18.
- **The photo profile's cleanup is slow:** about 3.2 s a 300 dpi
  Letter page in release, on one thread, 2.3 s of it finding the page
  and evening the light (`find_page`, `flatten_light`); the scanner
  profile about 0.8 s. 500 pages take about 26 minutes under Photo and
  7.5 under Scanner, so a fast feeder (slices 6 and 7) would outpace
  Photo's cleanup. Slice 6's scan-speed gate takes it up.
- **A Rewrite's bytes follow how a document's pages were split across
  inserts** (the same pages in another object order; Deviations): by
  design, noted for whoever compares saves.

What review found and fixed: the plan was reviewed adversarially
before the steps started, and its findings are the deviations above,
as resolved (the planner's list). Each step checked its new checks by
switching their behavior off (Checked by tests). The wrap-up read the
slice's whole diff again against the plan and the rules for every
slice, and found nothing to fix in the code; the plan's text was
behind it in proposals 1 and 3 and findings 1 and 6, now as built.

Fixes after review (the slice's code review, 16 findings; each checked
against the code first):
- **An intake that stops keeps its pages** (finding 1): a cancel, a
  page that can't be read, cleaned or recognized threw away the batch
  not yet landed (up to 7 pages). The loop now runs in a closure, and
  whatever it returns, a batch it left holding pages lands after it
  (a landing that failed took its batch, so it's never landed twice),
  reporting to no progress so a cancel doesn't stop it; the read's
  error comes back first. Tests:
  `a_page_that_can_t_be_read_keeps_the_pages_before_it`,
  `a_cancel_keeps_the_pages_so_far`. `scan()` still writes nothing on
  an error (its Session is dropped).
- **A batch's text by its pages** (findings 2 and 14, the same): the
  landing takes the `Made::Page` ids before pairing them with the
  batch's word lists, and a count that differs is an error, not a
  `debug_assert`. No insert today makes anything but pages, so no test
  can reach the error.
- **Word boxes against the old path** (finding 3): checked, equal
  (Measured).
- **F14's body words** (finding 4): nine in ten of each page's body
  words (distinct lowercased alphanumeric tokens: 62, 78 and 39) must be
  found on it, besides every heading word as before (case kept).
- **A TIFF's first page is `load`'s** (findings 5, 6, 7, 8, 10 and
  15, one regression seen six ways): `Frames` gives `load(path)` for
  a TIFF's first page and keeps its decoder at the first image for
  the later ones, so `scan` reads every TIFF it read before, to the
  same page. Later pages: the Orientation tag read as image reads it
  (a tag that isn't one value from 1 to 8 ignored, not an error); the
  decoder within image's 512 MiB (`TIFF_LIMIT`, `tiff_decoder`, also
  `tiff_pages`'); `tiff_image`'s `w * h` in `usize`, not `u32` (a
  70000x70000 header panicked a debug build); its ResolutionUnit read
  leniently as well (a malformed one an error before, the same class
  as finding 6; images-to-pdf only reads more files for it). Tests:
  `a_tiff_s_first_page_is_load_s_image` (11 kinds, among them CMYK8,
  CMYK16, float RGB, planar RGB and the tag as two values and as a
  signed one: each `load`'s exactly, and one page counted),
  `a_tiff_s_later_pages_read_as_load_reads_them` (each kind second
  after a gray page: exactly `load` of it alone, CMYK8 within 1,
  CMYK16, float and planar an error), `a_tiff_page_past_256_mib_is_read`
  (a white 9500x9500 RGB second page, 271 MB, PackBits),
  `a_tiff_page_too_big_is_refused` (a 70000x70000 header, through
  `frames` and through images-to-pdf's read). The review's files,
  with the fixed debug binary: its planar TIFF scans to the old
  binary's bytes, its two-valued Orientation scans to the old
  output's bytes, and its 70000x70000 header fails as the old one
  did ("Memory limit exceeded"). The four baseline runs: a and d
  byte for byte the wrap-up's, b and c equal once masked.
- **Thumbnails aren't pages** (finding 9): `next_page` skips images
  marked reduced-resolution (NewSubfileType bit 0) after the first,
  in `frames`, `count_frames` and images-to-pdf's `tiff_pages`
  (images-to-pdf, Combine and `PageSource::Images` lose a TIFF's
  thumbnail pages too; the first image is always a page, as `load`
  reads it). Test: `a_tiff_s_thumbnail_is_not_a_page`.
- **Progress counts pages** (finding 11): Deviations. Test:
  `progress_counts_a_tiff_s_pages` (a two-page TIFF and a PNG: the
  count 3; done 0, 1, 2 and fraction 0, ⅓, ⅔ as each page is asked
  for; 3 of 3 and 1.0 at the end).
- **The photo profile judged** (finding 12): the flatbed test asserts
  what finding 6 recorded (no quad, the whole scan fitted, under 0.3°
  left, 28 lines, paper and lid both at least 250), in place of the
  print.
- **The clock in the intake** (finding 13):
  `a_batch_lands_once_its_wait_has_passed` (a wait of 0.3 s, room for
  100 pages, small pages taking 0.9 s to come before pages 1 and 5:
  landings of 1, 4 and 3 pages).
- **Leftovers** (finding 16): group_tests.rs' module doc rewrapped; the
  TIFF test's repeated `crop = false` gone.
- Rejected: none outright. Finding 5's GrayA16 isn't read by `load`
  either (Found); its other kinds are.

Measured (AMD EPYC 7302P, 32 threads, release builds unless marked):
- **The gates** (`gate_7k`, release, `--ignored --test-threads=1` in
  one run, started once the 1-minute load was under 4 on two checks 30
  s apart: load 1.8 at its start and under 3.5 through the timed
  gates, its first eight minutes; 11 of 11 asserted gates pass, and
  both memory rows):

  | Row | Synthetic | Heavy | Scanned |
  |---|---|---|---|
  | geometry walk (asserted < 100 and < 250 ms); ready (asserted < 1 s on the heavy) | 48.1 ms; 54.6 ms | 77.3 ms; 352 ms | 60.5 ms; 471 ms |
  | realize + open + render 1,024 px, an edited page (asserted < 15 and < 25 ms) | 11.9 ms (slice 19: 11.8) | 21.3 ms (21.2) | 33.0 ms (31.7) |
  | thumbnail strip, crop + watermark + header, 320 px (asserted ≥ 310 pages/s) | 425 pages/s | 348 pages/s | 163 pages/s |
  | recognized text on every page: census (asserted 1 font, one block a page); the Append | 1, 7,000; 10.11 s | 1, 7,000; 10.16 s | 1, 7,000; 9.98 s |
  | one-mark Append on the scanned (asserted < 50 ms) | | | 25.6 ms (slice 19: 26.4) |
  | the Rewrite against slice 8's writer, the gates' own part (fails past 1.3×, flagged past 1.2×) | 151 ms, 1.13× | 780 ms, 1.10× | |

  The rest as slice 19 left them: history's 200 OCR versions of 1,000
  pages 236 MB (≤ 256 MB, asserted); snapshot 0.24–0.28 µs and
  `Tx::check` of a delete of every page 0.417 ms; engines after 220
  groups 28.9–29.0 MB weighed, ≤ 6 open; crop all pages 11.5–13.4 ms;
  Bates apply 0.62–0.63 ms, one font; AddMark 0.021 ms and a full
  reorder 1.01 ms at 20,000 marks; ready on 20,000 sheaf marks 559 ms;
  "Page n of N" moved after a save, the Append 469–675 ms (≤ 1 s),
  sections 1.36–3.33 MB (≤ 8 MB). Proposal 8's accepted misses
  (`SHEAF_GATE_PROPOSAL_8`) where slice 19 left them: synthetic 21.6,
  20.3, 18.9 and 28.6 ms, heavy 37.8, 35.2, 32.1 and 42.1 ms, the
  Appends on the file opened again 1,068 and 1,448 ms (the scanned's
  889 ms, asserted). The flatten export's RSS (`SHEAF_GATE_FLATTEN`):
  the heavy's peak 513 MB (slice 19: 514), the synthetic's 88.5 MB,
  the scanned's 1,077 MB. This slice's paths don't reach them. The
  Rewrite against slice 8's writer in the processes of
  `rewrites_on_the_synthetic_and_heavy_7k`: synthetic 155 ms (1.16×),
  heavy 737 ms (1.04×), under the 1.2× flag; this slice doesn't touch
  the writer, so no five-run median was taken (slice 19's medians:
  1.09× and 1.09×).
- **The memory gate** (`scan_of_500_black_and_white_pages`, in the
  run above; from its tenth minute the machine was busy again, load
  24–45, which moves the pages/s rows only):

  | Profile | Cleaned, landed and written | Peak RSS | RSS added (asserted < 400 MB) | Written |
  |---|---|---|---|---|
  | Photo (the command's default) | 0.301 pages/s | 262 MB | 210 MB | 10.4 MB |
  | Scanner | 0.970 pages/s | 245 MB | 189 MB | 10.4 MB |

  Step 5's run (a process of its own, the machine shared): Photo 0.316
  pages/s, 251 MB peak, 246 MB added; Scanner 1.12 pages/s, 218 MB,
  185 MB; 2,032 s for the test (this run's 12 tests: 3,140 s). Where it
  goes (step 5's one-page probe, deleted): one Photo page alone peaks
  160–179 MB above its start in 3.15–3.19 s (finding the page and
  evening the light 2.31–2.34 s, straightening 0.41 s, fitting and
  black and white, Sauvola and despeckling, 0.44 s; G4 9 ms for about
  21.4 KB), one Scanner page 130 MB in 0.81 s. So a row's peak is one
  page's cleanup (Sauvola's two u64 integral images, the light's
  copies) plus 55–85 MB over 500 pages (the store's batch PDFs, the
  Rewrite, the allocator's slack), and nothing grows with the count.
  The fault check (every cleaned image held to the end) adds 4,206 and
  4,214 MB: the plan's 4.2 GB. Both 500-page files pass `qpdf
  --check`, 500 pages each.
- **The same pages as before** (step 1's baseline, step 3's and the
  wrap-up's runs; debug sheaf-cli builds, the old one at `3e1b9f0`).
  Inputs: 300 dpi PNGs of pages 1–2 of three scan-kind corpus files
  (scans/safedocs/0000932, 1-bit at 300 dpi; 0002156, gray at 300 dpi;
  0000577, color at 150 dpi), page 1 of forms/irs/f1040.pdf, and two
  two-page TIFFs of scans/safedocs/0002786 (black and white, gray).
  Four runs over the nine files: defaults; `--mode bw --ocr`; `--mode
  gray --no-crop --no-flatten --ocr --page original`; `--mode bw
  --drop-blanks`. The old binary twice: the runs without OCR
  byte-identical, those with OCR identical once the random block ids
  (`/B (b…)`, `/SheafOcrBlock (b…)`, the `/Sh…` names drawn from them)
  are masked and the files compared as QDF without their xref. G4 at
  construction (step 1): the black and white runs byte-identical to
  the old. The intake (step 3): each run has 11 pages, 9 before, since
  each TIFF's second page is now there; the 9 pages that line up (each
  TIFF's first against the old run's one) render identically at 150
  dpi in all four runs (36 of 36 pages equal byte for byte), and
  `export --format text` is the old text (16 bytes in the runs
  without OCR, 20,314 and 24,854 in those with); the two new pages
  are the TIFFs' second pages. The wrap-up's final binary: its four
  outputs equal step 3's (the runs without OCR byte for byte, those
  with OCR once masked). `qpdf --check`: no errors on all four. The
  text layers' word boxes (review finding 3), by `pdftotext -bbox`
  (poppler) on the two runs with OCR, the 9 pages that line up: every
  word and its box (to the 6 decimals it prints) equal, 3,320 words in
  the black and white run and 4,096 in the gray one; the review's
  fixed binary's outputs equal the wrap-up's (a and d byte for byte, b
  and c masked).
- **Batching** (`batches_make_the_pages_one_batch_makes`, ten pages):
  batches of 1, 3 and 8 make the pages one batch makes (0 px at 50
  dpi, the same PDFium text and character boxes, the same OCR layers)
  in every mode, with OCR and without, but not the same bytes:
  batches of 1 differed from one batch in every mode (another object
  order), and 3 and 8 matched it in Color by chance. The same batching
  twice: the same bytes (without OCR).
- **Finding 6** (the flatbed test's scan, orientation off): the
  Scanner profile kept the whole scan (no quad, 2550x3279 fitted),
  took out the 3° (under 0.3° left), kept 28 of 28 lines where they
  were drawn and the light as scanned (paper 246, lid 251). The Photo
  profile found no page either (quad None) and made the same
  2550x3279, took out 3.00° (0.000° left), kept 28 of 28 lines (ink
  over columns 386–2136 and rows 464–2848, as the Scanner's), and
  evened the light out: paper and lid both 254. Two throwaway scans
  (a 12 px dark strip along the glass's top and left edges; a page
  filling the scan with a dark band across its top) gave no page
  either. So the photo profile doesn't crop a white page on a white
  lid wrongly here: both tones are on the bright side of Otsu's level,
  the bright pass's outline is the whole scan, and `find_page` takes
  that as the photo being the page already. What separates the
  profiles on a scanner's pages is the light.
- **The corpus** (`../pdf-corpus`, `SHEAF_CORPUS` unset; no "skipped:
  no corpus" line), every count as #26 left it: materialize_corpus's
  identity 4,389 of 4,392 written, 2,269,688 primary objects
  surviving, 919,465 copied as they were, 432 changed by `keep_only`,
  0 failed; 2,901 forms without their first page, 0 failed; every page
  duplicated, 4,389, 0 failed; inserted twice into the 1040, 1,098, 0
  failed; four times, 0 differ; the logo, 0 of 2,037,600 bytes differ.
  append_corpus: 4,389 marked, 3,770 appended, 259 rewritten (211
  RemovalCheck, 34 BaseObjectsMissing, 14 BaseUnsound), 360 asked, 0
  failed, 0 derived vetoes and 211 on the files' own objects; the
  oracle finds text gone from 2,899 of 2,901 (none from 0016064 and
  0112654). encrypted_corpus: the Rewrite keeps 573 of 573 and the
  Append 572 (0630899 declined). source_corpus: 4,392 files, 24,251
  pages compared, 0 differ; forms_corpus passes. The ignored tiers
  (the export tier, deleted means deleted, realize_corpus, the scans'
  OCR) are the pre-merge run's (Greg, 2026-10-20); this slice adds and
  changes none.
- **Suites:** `cargo test --workspace` 1,682 passed, 0 failed and 57
  ignored after the review's fixes (1,674 before them; slice 19: 1,654
  and 56 at its verify, plus its wrap-up's three print numbering
  tests; this slice's 25, and the memory gate ignored), no "skipped:
  no corpus"; F14's test about 14 s in the dev profile. The review's
  fixes left fmt, clippy (`-D warnings`, all targets), the non-Linux
  clippy simulation and Windows clippy for sheaf-image and sheaf-ops
  clean; they don't touch the timed gates' paths, and the memory
  gate's intake loop holds what it held (the same batch, now landed
  on an error too), so the gates weren't run again. The interop
  tier (qpdf; pdf.js 6.4.299 on node 24.22): 19 of 19 with
  `--include-ignored`. `qpdf --check` clean on the baseline's four
  outputs, on F14's (made by hand with the test's commands; pdf.js
  reads its 255 words, 88, 103 and 64 a page) and on both 500-page
  files. No proptest seed added. fmt and clippy (`-D
  warnings`, all targets) clean, and clippy with sheaf-app's
  Linux-only gates switched off (`nonlinux-clippy.sh`); clippy (`-D
  warnings`) on `x86_64-pc-windows-msvc` passes for sheaf-image and
  sheaf-doc with their tests, for sheaf-ops with all targets, and for
  sheaf-cli with all targets without its OCR engines
  (`--no-default-features`: oar-ocr's tokenizers build oniguruma with
  a C compiler); sheaf-app can't be checked for Windows here, as
  before.

Checked by tests:
- **G4 and frames:** sheaf-image's
  `a_black_and_white_page_is_held_as_g4`,
  `a_tiff_gives_its_pages_one_at_a_time` (two images: sizes, order,
  pixels), `a_tiff_s_first_page_is_load_s_image`,
  `a_tiff_s_later_pages_read_as_load_reads_them`,
  `a_tiff_s_thumbnail_is_not_a_page`,
  `a_tiff_page_past_256_mib_is_read`, `a_tiff_page_too_big_is_refused`
  (Fixes after review),
  `any_other_image_is_one_frame_as_load_reads_it` (a PNG),
  `tiff_round_trips_through_the_reader`, `png_gray_and_bilevel`.
- **The scan job and its group:**
  `caps_cover_every_op_target_and_actor`,
  `a_scan_group_takes_the_scan_job_s_batches`.
- **The intake:** tests/intake.rs'
  `batches_make_the_pages_one_batch_makes` (per mode, with OCR and
  without, batchings of 1, 3 and 8 against one of every page: each
  page's render at 50 dpi at 0 px, PDFium's page text and character
  boxes, the state's OCR layers; the same batching twice the same
  bytes), `an_intake_is_one_undo_step` (20 pages in
  batches of 3: one version labelled "Scan" after the empty one, by
  `Job(Scan)`; one undo leaves no pages and nothing to undo),
  `an_edit_between_batches_makes_two_undo_steps`,
  `undoing_an_intake_stops_it` (`GroupEnded` after exactly 9 pages
  read, no pages left), `a_multi_page_tiff_scans_page_by_page` (a
  two-page TIFF and a PNG through `scan()`: three pages, MediaBoxes
  612x792, 792x612, 576x720);
  `a_page_that_can_t_be_read_keeps_the_pages_before_it`,
  `a_cancel_keeps_the_pages_so_far`,
  `a_batch_lands_once_its_wait_has_passed`,
  `progress_counts_a_tiff_s_pages` (Fixes after review);
  `a_batch_is_due_at_its_count_or_after_its_wait`; scan_blanks.rs
  unchanged.
- **The profiles:**
  `the_scanner_profile_neither_crops_nor_evens_the_light`,
  `each_profile_keeps_its_own_choices` (set and get, a JSON round
  trip, `{}` both profiles' defaults),
  `the_profile_decides_what_no_flag_does`,
  `of_a_flag_and_its_no_the_last_wins`,
  `the_scanner_profile_keeps_a_white_page_on_a_white_lid_whole` (a
  1400x1800 lid of about 252 holding a 1275x1650 page of about 247
  with 28 lines of text bars, turned 3°; the Scanner profile with
  orientation off: no quad, the whole scan fitted, 2550x3279, skew
  under 0.3° on a 1000 px copy, all 28 ink rows within 3·scale px of
  where they were drawn and the ink's columns spanning theirs, the
  light as scanned; the Photo profile no quad, the scan fitted, under
  0.3° left, 28 lines, the light evened to at least 250).
- **Memory:** gate_7k's `scan_of_500_black_and_white_pages` (Measured).
- **F14:** `scan_ocr_finds_the_words_in_one_block_a_page_and_one_font`
  (every heading word, nine in ten of each body's).
- **Fault checks,** each file restored from the step's own copy and
  checked with cmp, and no proptest seed added (only
  materialize/append_rewrite_tests.txt is there):
  - frames ignoring the Orientation tag fails
    `a_tiff_frame_is_what_load_reads` at its equality; `from_image`
    holding the raw pixels (`Pixels::Gray`) fails
    `a_black_and_white_page_is_held_as_g4` and
    `tiff_round_trips_through_the_reader`;
  - the Scan group taking any job's commits fails
    `a_scan_group_takes_the_scan_job_s_batches` ("panics in debug
    only"); the scan job making any op fails
    `caps_cover_every_op_target_and_actor` ("Job(Scan) making
    move-pages") and the group test at its `Refused`;
  - committing without the group fails `an_intake_is_one_undo_step` (8
    versions, not 2); reading on past `GroupEnded` fails
    `undoing_an_intake_stops_it` (20 pages read, not 9); a TIFF's first
    image alone fails `a_multi_page_tiff_scans_page_by_page` (2 pages,
    not 3); dropping the last batch's `SetOcrText` fails the batch test
    ("every page has a layer"); `due` ignoring `wait` fails its unit
    test ("one page, the wait passed");
  - crop on in the Scanner profile fails its unit test (not the
    flatbed test: Deviations); flatten on fails the flatbed test
    ("paper and lid (254, 254), scanned 247 and 252"); deskew off
    fails it ("3° of skew left");
  - the intake holding every cleaned image to its end makes the
    memory gate fail: the Photo profile added 4,206 MB and the Scanner
    4,214 MB ("the photo profile added 4206 MB to the RSS"), which is
    the plan's 4.2 GB;
  - dropping the `SetOcrText` ops in `SessionLanding::land` fails F14's
    (b) ('page 1 has no "Invoice"') and, with (b) skipped, (c) ("page 1
    draws 0 OCR blocks"); a fresh glyphless font a page fails (d) ("the
    pages draw in {(22, 0), (23, 0), (24, 0)}");
  - "Bilevel" taken out of `QUALIFIED_ONLY`, with a `clean_one()` call
    put in scan.rs, fails `nothing_retired_is_back` with five hits,
    each "(retired in M5 slice 1)": the four `TiffPage::Bilevel` uses
    and the call.
  - The review's fixes, each switched off alone: a TIFF's first page
    through `tiff_frame` in place of `load` fails
    `a_tiff_s_first_page_is_load_s_image`; the Orientation tag read
    strictly again fails the later-pages test ("tag has invalid
    type"); `TIFF_LIMIT` at tiff's 256 MiB fails
    `a_tiff_page_past_256_mib_is_read`; `(w * h) as usize` panics
    `a_tiff_page_too_big_is_refused` ("attempt to multiply with
    overflow"); thumbnails kept fails `a_tiff_s_thumbnail_is_not_a_page`
    (3 pages, not 2); the rest not landed on an error fails both
    keep-the-pages tests (3 pages, not 4); the rest landed with the
    cancelled progress fails the cancel test (3, not 4); `Files`
    counting files fails `progress_counts_a_tiff_s_pages` (`Some(2)`,
    not 3), and the intake not setting its count fails it at its
    per-page check; the clock not reset after a landing fails the wait
    test (`[1, 1, 1, 1, 1, 1, 1, 1]`), and the wait never read fails
    it (`[8]`); the Photo profile with the Scanner's options fails the
    flatbed test's photo half; each page's text cut to its first 12
    words fails F14's body check.

For Greg:
- **Hand checks** (no tests reach these):
  - Create from Scans' "Pages from" row, by Tab and Left/Right and by
    clicking its chips: a switch loads the other profile's steps and
    refreshes the preview; edits to one profile are still there after
    switching away and back; Create remembers the profile and its
    options, and the dialog opens on that profile next time; the "As
    photographed" page size and the "Photo" preview label still read
    well under the scanner profile.
  - A real copier's or scanner's multi-page TIFF (scan-to-folder),
    with `sheaf scan --profile scanner` and with the dialog's "A
    scanner or copier": every page there, in order, upright and
    straight, the whole image kept and its light as scanned; and once
    with `--mode bw`.
- **The photo profile's speed** (Found): about 3.2 s a 300 dpi Letter
  page, against 0.8 s for the scanner profile; worth knowing before
  slices 6 and 7 put a feeder behind it.
- **Ask 12** (the scanners Greg can check by hand) is still open, for
  slice 6.

Left for later slices:
- **Slice 2:** the app's `Landing` (a document's planner: `plan_made`,
  `Base::At`, so the user can keep working), untitled documents, and
  B11 (Create from Scans still runs `scan()` to a file and opens it).
  The app's landing should end its group when an intake fails
  (`SessionLanding` has no Drop), and may need intake on a thread of
  its own (`Pages::next` blocks).
- **Slice 6:** sheaf-scanner's `RawPage` (bytes, dpi, sheet, side)
  mapped into `intake::RawPage` (which may gain dpi, sheet and side);
  the Scanner profile's blank rule with duplex ("on with duplex"),
  part of the device's settings; a `sheaf scan --device` that stops on
  a hang, a crash or Ctrl-C must keep the pages already scanned (the
  command line's Session is in memory today, and a scan of files
  loses nothing, its images being on disk); the 2 s rule with a
  blocking device (`Batching::DEFAULT`, read as each page is done);
  the photo profile's speed against a feeder (Found).
- **Slice 7:** pages streaming into the open document (Insert from
  Scanner), through slice 2's landing, with `Batching::DEFAULT`.
- **Slices 6 and 7:** whether a cancel finishes a device's page in
  flight (already scanned) before it stops, as proposal 10's "stops
  after the page in flight" may mean; the intake cuts it short today
  (Deviations).
- **For the backlog** (Found): `tiff_pages` and the Orientation tag;
  Combine's `inspect` decoding every image to count pages (it could
  use `io::count_frames` now); `images_to_pdf` holding every image;
  the TIFF kinds `tiff_image` refuses (Deviations) and its unreachable
  `GrayA` arms; `progress::run` hanging after a panic.
