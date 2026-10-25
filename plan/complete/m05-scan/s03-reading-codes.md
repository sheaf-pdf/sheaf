# M5 slice 3: Reading codes

Status: done · Part of [M5: Scan & OCR](../../m05-scan/README.md)

**Reading codes** (proposals 14 and 15).
- **Builds:** `sheaf_image::codes`: barcodes through rxing (ask 7;
  `default-features = false`, so its `image` pin doesn't hold
  sheaf's back), one decode a page for sheaf's sheet and the rules'
  symbologies at once (`TryHarder`, formats limited); sheaf's own
  patch-code reader (types 1, 2, 3, 4, 6 and T: bars along an edge,
  by wide and narrow widths); a QR code's modules for the writer; the
  bench's degradations as a test helper.
  - As built: rxing's features add `pdf417` (Research); the one
    decode is one binarized bitmap decoded for each symbology alone in
    a fixed order, past codes the caller's text match turns down
    (proposal 15, as built); `codes::read` reads a page's patch code
    and its barcode together; and the test helper is `codes::fixture` behind
    sheaf-image's new `fixtures` feature (`cfg(any(test, feature =
    "fixtures"))`, as sheaf-doc's), since sheaf-ops' tests in slices 4
    and 5 draw and degrade codes too.
- **Checks:**
  - Each symbology at 150, 200 and 300 dpi, in gray and thresholded
    black and white, turned 90°, 180° and 270°, skewed ±3°, with noise
    and blur, read back; Alliance's patch-T sheet and Kodak's spec's
    bars, drawn and degraded the same way, read as their types.
    - As built: noise is a scanner's, Gaussian σ 10 gray levels, and
      blur σ 1.5 px, each made at 300 dpi before the page is resampled
      to 200 or 150 and thresholded at 128 (the bench's σ 25 at 300 dpi
      failed every reader's 1D and PDF417 codes, and its office mix
      used σ 8). The sizes, fixed before the first run: QR at sheaf's
      0.75 mm (9 px at 300 dpi), Data Matrix at 0.68 mm (8 px), Code
      39 and Code 128 with 20 mil narrow bars (6 px) 0.5 in tall,
      PDF417 with 4 px modules in the encoder's 4-module rows. The
      patch layouts are drawn from their measurements (no PDFs in the
      repo): Kodak's label (one edge, A-61599's geometry), Kodak's
      printed sheet (all four edges, the first bar 0.62 in in) and
      Alliance's patch-T sheet (all four edges, 0.63 in in), each with
      the type as Code 39 in the middle. The bench's 17 degradations
      are kept as `fixture::BENCH`.
  - A blank page, a page of text, and a page of table rules read as
    nothing.
    - As built: sheaf-image has no font, so the fixture's text is
      glyph-like strokes; real glyphs come from PDFium in sheaf-ops: a
      lopdf-built Helvetica page with a ruled table in CI
      (`a_page_of_real_text_reads_as_nothing`), and the 7k fixtures'
      pages in the gate.
  - **Speed gate:** a Letter page with no code, decoded for sheaf's
    sheet plus patch codes at 150 dpi, takes no more than 60 ms on one
    thread in a release build (the bench: 44 ms plus 5); the gate
    measures on every run and asserts with `SHEAF_GATE_CODES`, as the
    other gates do.
    - As built: `codes_on_a_page_with_none` asserts four rows, each the
      median of 21 reads: the stroke page at 150 dpi in gray and in
      black and white, and the synthetic and heavy 7k's first pages
      drawn as separators are found (their content alone, 150 dpi
      gray); it measures every symbology at 150 gray and 200 in black
      and white, and the patch bars alone.
  - **Corpus:** every page of the 709 files under `scans/` (5,697
    pages), drawn at 150 dpi: no sheaf code and no patch code found
    (any found is a false positive to fix), and the other barcodes
    found are listed as the baseline in the slice notes (they're
    content, like the forms' Code 39 ids).
    - As built: sheaf-ops' `scans_hold_no_separators` (ignored,
      `SHEAF_CODES_ONLY`) draws each page with PDFium directly, its
      content alone in gray (the picture `separate::find` will draw
      through `Views` in slice 5, without a Session), and reads it for
      patch bars, for every symbology taking only a sheaf code or a
      patch sheet's Code 39 (each looked for past any other code), and
      for every symbology taking the first code (the baseline). Until
      slices 4
      and 5 define them, the sheet's `SHEAFSEP` prefix and the Code 39
      `PATCH…` text (a patch, proposal 14) are literals in the test.

## Notes

Done. Reading codes (proposals 14 and 15; asks 7–9): sheaf-image has
a new pixels-only module, `sheaf_image::codes`, which reads what can
make a page a separator sheet and makes the QR code sheaf's own sheet
will carry. Barcodes are read through rxing 0.9.3 (Apache-2.0,
without its default features, `pdf417` added for ask 9) behind
sheaf's own object-safe `Reader` trait, so zxing-cpp can come in
beside it (ask 7): a page is binarized once and decoded for each
symbology asked for alone, with `TryHarder` on, in a fixed order (QR,
Data Matrix, PDF417, Code 39, Code 128), until a code the caller's
text match takes, so sheaf's sheet comes first, a page reads the same
way every time, and a code of the page's content never hides a
separator's code of another symbology. Kodak patch
codes (types 1, 2, 3, 4, 6 and T) are read by sheaf's own reader,
bars along each edge read from the edge inward by their widths.
`codes::qr(text)` gives a QR code's modules (level M, UTF-8 with its
ECI, no quiet zone, the same modules for the same text), and
`codes::read` reads a page's patch code and its barcode together.
The bench's pages, its code drawing and its 17 degradations are
`codes::fixture`, behind sheaf-image's new `fixtures` feature, for
this crate's tests and slices 4 and 5's in sheaf-ops. Every symbology
reads back in every cell of its matrix (480 of 480), every patch
type in every layout scanned every way, blank, text and rules pages
(strokes, and a real Helvetica page drawn by PDFium in CI) read as
nothing, and the corpus's 709 scans (5,697 pages) hold no sheaf code
and no patch. The speed gate's four rows take 13–14 ms of the 60
allowed, every other asserted gate holds, and the corpus counts are
the baseline's. Nothing is deleted, no public name is retired, and
sheaf-doc, sheaf-edit, sheaf-app and the CLI don't change.
- **rxing behind `Reader`; QR modules; codes drawn** (step 1).
  sheaf-image Cargo.toml: `rxing = { version = "0.9.3",
  default-features = false, features = ["decoders", "encoders",
  "encoding_rs", "oned", "qrcode", "datamatrix", "pdf417"] }`, and a
  `fixtures` feature (commented as sheaf-doc's); the description and
  lib.rs' bullet and boundary (a barcode library, pixels only) say
  barcodes. codes.rs (new): the module doc (what's read, and why
  `TryHarder`, limited formats and a fixed order, with the bench's
  figures); `Symbology { Qr, DataMatrix, Code39, Code128, Pdf417 }`
  (Debug, Clone, Copy, PartialEq, Eq, Hash) with `ALL`; `Code {
  symbology, text }`; `trait Reader: Send + Sync { fn read(&self,
  page: &GrayImage, symbologies: &[Symbology]) -> Option<Code> }`;
  `Rxing`, which reads one `BinaryBitmap` (a `HybridBinarizer` over a
  `Luma8Source`) with a `decode_with_hints` call for each of QR, Data
  Matrix and PDF417 asked for, then one for the 1D symbologies asked
  for together, each with `TryHarder` and only those formats, a result
  of a format not asked for counting as none and any rxing error as
  None (nothing asked, nothing read); `Modules { width, height }` with
  `dark(x, y)`; `qr(text) -> Result<Modules>` (rxing's
  `qrcode_encoder::encode_with_hints`, level M, `CharacterSet` UTF-8).
  fixture.rs (new): `LETTER_300`, `white_page`, `encode(symbology,
  text) -> Modules` (QR through `qr`, the rest through
  `MultiFormatWriter` with no margin, cropped to their dark modules;
  a 1D code one row, a PDF417 code unturned in its 4-module rows) and
  `paste(img, modules, x, y, module_px_x, module_px_y)`. Tests: 8
  (Checked by tests).
- **Pages, degradations and the symbology matrix** (step 2).
  fixture.rs: `blank_page()`; `text_page(seed, clear)`, the bench's
  300 dpi Letter page in glyph-like strokes (22 letter shapes at an
  x-height of 22 px, a header over a rule, a 5 × 9 ruled table of
  digit shapes, a footer; whatever would cross `clear` left out);
  `rules_page()` (a border, W-2-style boxes, a table, lines to write
  on); `code_page(symbology, text, module_px, row_px)` (the code on
  text page 42 with its quiet zone cleared); `enum Degradation {
  Skew, Turn, Noise, SaltPepper, Blur, LowContrast, Jpeg, Threshold,
  Dpi }` and `degrade(img, from_dpi, steps) -> (GrayImage, u32)`, all
  seeded (a private xorshift64*); `BENCH`, the bench's 17 variants
  with its parameters; `VARIATIONS` (as is, turned 90°, 180° and 270°,
  skewed ±3°, noise, blur) and `scanned_every_way(page)`, 48 scans: each
  variation made at 300 dpi, resampled to 300, 200 and 150, in gray and
  thresholded at 128. Tests: five matrix tests, the negatives and
  `every_degradation_changes_the_page`.
- **The patch-code reader** (step 3). codes/patch.rs (new, its names
  re-exported from codes): the module doc (A-61599's geometry, the
  patterns from the edge inward, how it reads, the bench's figures);
  `PatchCode { One, Two, Three, Four, Six, T }` with `ALL` and
  `bars()`; `Edge`; `Patch { code, edge, inset, length }` (inches);
  `patches(page, dpi) -> Vec<Patch>` (the page box-reduced to about
  75 dpi and split at its Otsu level; lines 0.04 in apart across a
  band 4.7 in deep or half the page along each edge; a line reads a
  patch where light, four bars and three spaces of a patch's widths
  and light follow from the edge in; lines of one type within 3 lines
  of each other and 0.08 in of the last one's inset make a stretch, a
  patch at 0.6 in and 8 lines); `patch(page, dpi)`, the type read
  along the most length. fixture.rs: `PatchLayout { KodakLabel,
  KodakSheet, AllianceT }`, `patch_page(code, layout)`,
  `with_patch_bars(page, code, layout)`, `bordered_page()`,
  `edge_rule_page()`, `SHEET`, `SAMPLES` (the matrix's codes, moved
  from the tests) and `Scan { label, dpi, image }`. Tests: 8.
- **A page's codes, real text, the speed gate** (step 4). codes.rs:
  `PageCodes { patch, code }` and `read(page, dpi, symbologies,
  reader)`, the patch bars then the decode, both every time. sheaf-ops
  Cargo.toml: sheaf-image with `fixtures` as a dev-dependency. sheaf-ops
  tests/codes.rs (new): `real_text_page()` (a lopdf Letter page:
  Helvetica-Bold header over a rule, 36 lines of Helvetica at 11 pt,
  a 5 × 9 table of amounts with 0.75 pt rules, an 8 pt footer, its
  content written as bytes), `drawn(engine, page, dpi)` (content
  alone through `render_with`, then `to_gray()`), and
  `a_page_of_real_text_reads_as_nothing`. gate_7k.rs:
  `codes_on_a_page_with_none` with `CODES` (`SHEAF_GATE_CODES`),
  `CODES_GATE` (60 ms), `CODES_RUNS` (21), `read_timed` and
  `content_at_150(kind)`, and the module doc's M5 slice 3 paragraph.
  sheaf-image tests: `a_sheet_with_patch_bars_reads_both`,
  `a_page_with_nothing_reads_nothing`.
- **The corpus tier** (step 5). tests/codes.rs:
  `scans_hold_no_separators` (ignored; `SHEAF_CODES_ONLY` through
  `common::narrowed`), `scan_codes(corpus, entry)` on `each_entry`'s
  threads (PDFium's page count checked against the manifest's, each
  page drawn at 150 dpi and read for patch bars, for QR alone and for
  every symbology), `SCANS` (709, 5,697), and the literals `SHEET`
  (`SHEAFSEP`) and `PATCH_TEXT` (`PATCH`); `drawn` returns a Result,
  so a file that doesn't open or draw is a failure listed with its
  page. Run in full in the slice (Measured).
- **Wrap-up** (step 6). README's License paragraph names rxing. This
  plan, as built: Research's rxing features (`pdf417`), proposal 15's
  one decode, slice 3's Builds and Checks, and the Risks' codes on
  poor scans and barcodes in content. No docs/ page changes: nothing
  a user reaches reads codes yet (separator docs are slice 10's).
- **Review fixes** (after step 6; What review found). codes.rs:
  `Symbology`'s variants and `ALL` in the order a page is decoded
  (QR, Data Matrix, PDF417, Code 39, Code 128); `Reader::read(page,
  symbologies, accept)` and `codes::read(page, dpi, symbologies,
  accept, reader)`, `Rxing` decoding each symbology alone and going
  on past a code `accept` turns down (`TWO_D`, `linear()` and `asked`
  gone, `of` in their place); `Modules`' sides private behind
  `width()` and `height()`, and `dark` asserting its column and row;
  the docs say binarized once, not decoded once. patch.rs: `QUIET`
  0.18 in (was 0.08), its own `otsu_level` in u64 (imageproc's counts
  in u32), the module doc's bar length as A-61599 gives it. fixture.rs:
  the getters, and the generator's doc says why it isn't fastrand's.
  sheaf-ops: `drawn` moved to tests/common/mod.rs, which gate_7k now
  includes (`content_at_150` calls it); the corpus tier reads every
  symbology twice, for a separator past other codes and for the
  baseline. Tests: 7 new, and `codes_asked_for_read_in_a_fixed_order`
  covers every neighbor in the order.
- **Deleted:** nothing; slice 3 has no Deletes list and renames no
  public name, so retired.rs gets no rows (`git diff`: no sheaf-doc,
  sheaf-edit, sheaf-app or sheaf-cli file changed, and the only lines
  removed from sheaf-image and sheaf-ops are a description, a doc
  sentence and two `use` lists, each rewritten with more).

Deviations from the plan (the planner's list first, as resolved):
- **rxing's features add `pdf417`:** the plan listed `decoders,
  encoders, encoding_rs, oned, qrcode, datamatrix`, but ask 9 makes
  PDF417 a rule's symbology and it's a feature of its own in 0.9.3.
  `multi_barcode_readers` stays off (one decode for the first code).
  Research says so now.
- **"One decode a page" is one bitmap decoded for each symbology
  alone, in a fixed order, past codes the caller turns down:** rxing's
  `MultiFormatReader` walks the 2D formats of its `PossibleFormats` in
  a `HashSet`'s order, random for each reader
  (multi_format_reader.rs:180), so with two 2D symbologies asked a
  page with both read as either one, and sheaf's QR didn't come first.
  And any call stops at its first code, while proposal 15 counts a
  code only if its text matches: with QR always first, a vendor's QR
  code (8 of the corpus's scan pages carry one) would hide a rule's
  Code 128 every time, and in one 1D call a form's Code 39 id (6
  pages) would hide a rule's Code 128 on the rows met first. So
  `Rxing::read` calls `decode_with_hints` for each symbology asked
  alone, in `Symbology::ALL`'s order (QR, Data Matrix, PDF417, Code
  39, Code 128), on one `BinaryBitmap`, whose binarization is cached
  (binary_bitmap.rs:39), and `Reader::read` takes `accept`, the
  caller's match: a code it turns down lets the next symbology's
  decode run (review findings 1 and 4; the step 1 to 6 version did
  the 1D symbologies in one call and returned the first code of any
  pass). rxing gives one code a decode, so a second code of the same
  symbology is still hidden by the first (Left). Proposal 15 says so
  now. rxing's own `Reader` trait is imported as `use rxing::Reader
  as _`.
- **`Modules` is W × H,** not square: the fixture draws 1D and PDF417
  codes through it too (QR's are square).
- **The matrix's noise and blur, defined:** the bench's σ 25 noise at
  300 dpi failed Code 39, Code 128 and PDF417 in every reader, so the
  matrix's noise is a scanner's, σ 10 (seed 11; the bench's office mix
  used 8), and its blur σ 1.5 px, both made at 300 dpi before the
  resample and the threshold. All 17 bench variants are kept in
  `fixture::BENCH`, as proposal 22 says.
- **The codes' sizes, fixed before the first run** (the check gives
  none): QR at sheaf's 0.75 mm (9 px at 300 dpi), Data Matrix at the
  bench's 0.68 mm (8 px), Code 39 and Code 128 with 20 mil narrow bars
  (6 px) 150 px tall, PDF417 with 4 px modules in the encoder's own
  4-module rows (16 px). The Data Matrix and PDF417 texts were chosen
  here ("DOC=W-2;CLIENT=1042;N=0001", "Form W-2 2025|Copy B|Acme
  Corp|12-3456789"); Code 128's is SurePrep's "Schedule A".
- **The bench's degradations aren't test-only:** slices 4 and 5 use them
  from sheaf-ops' tests, so they're `codes::fixture` behind the new
  `fixtures` feature (`cfg(any(test, feature = "fixtures"))`, as
  sheaf-doc's and sheaf-edit's). Workspace clippy unifies sheaf-ops'
  dev-dependency feature into the library, so clippy also runs on
  sheaf-image alone with no features.
- **The fixture's text is strokes, real glyphs come from PDFium:** the
  bench drew DejaVu through ab_glyph and imageproc's `text` feature;
  sheaf-image has no font (imageproc without `text`), and CI's macOS
  and Windows runners have no DejaVu. The real-text checks are
  sheaf-ops': the lopdf-built Helvetica page in CI (PDFium carries the
  standard 14), the synthetic and heavy 7k's first pages in the gate,
  and the corpus tier. Strokes are drawn in gray 15 (the bench's text
  was 20), with a stroke header, footer and digit shapes in place of
  its fixed strings.
- **The patch sheets are drawn, not kept:** no PDFs go in the repo, so
  the layouts are drawn from their measurements: Kodak's label
  (A-61599: one edge, bars 0.20 and 0.08 in, 0.80 in in all, 2.5 in
  long, 0.5 in in, 3 in along the top), Kodak's printed sheet (all four
  edges 0.62 in in, from 0.5 to 10.5 in down the sides and 1.5 to 7 in
  along the top and bottom) and Alliance's patch-T sheet (measured at
  100 dpi: all four edges, the first bar 0.63 in in, N W N W, the
  sides' bars from 0.63 to about 9.5 in, the top and bottom's about
  1.55 to 6.96 in). Both
  sheets carry what the real ones do in the middle: a title, lines of
  text and their type as Code 39 (`PATCH1`…`PATCHT`, 20 mil).
  `with_patch_bars` asserts Alliance's is drawn only as a T.
- **The corpus tier draws with PDFium directly:** proposal 15's
  `separate::find` draws through `Views` (slice 5), so the tier calls
  `PdfiumEngine::open_bytes` and `render_with(…, Job::Page { width:
  page width × 150/72, R0 }, Rgba, Draw { annots: false, forms: false
  })` and `to_gray()`, the same content-only picture with no Session.
  PDFium draws under its global lock, so only the reads run in
  parallel. Pages with `/Rotate` (0025212, 0001706, 0028220, probed)
  draw in the turned frame at 150 dpi on both axes.
- **"No patch code" counts Code 39 `PATCH…` too** (proposal 14), read
  with every symbology, and a page counts as a sheaf code if either
  read gives a QR starting `SHEAFSEP`: both are literals in the test
  until slices 4 and 5 define the sheet's text and the patch mapping.
- **The speed gate's rows:** "the bench: 44 ms plus 5" is the bench's
  text page (DejaVu glyphs) decoded at 150 dpi in black and white,
  plus the patches. Asserted under `SHEAF_GATE_CODES`, each the median
  of 21 reads with [Qr] plus patches: the stroke page at 150 dpi in
  gray and in 1-bit, and the synthetic and heavy 7k's first pages drawn
  content-only at 150 dpi in gray. Measured only: every symbology at
  150 gray and at 200 in 1-bit, and the patches alone (`codes::read`
  with no symbologies). Each row first reads the page and must find
  nothing. The measured rows' baselines are the bench's 70 ms
  (text200bw) and the prototype's 5 ms.
- **Proposal 12's names:** `Symbology` and `PatchCode` derive Debug,
  Clone, Copy, PartialEq, Eq and Hash only; serde and the CLI's tokens
  (`--find code39`, `patch:2,3,T`) are slice 5's.
- **Not built here:** proposal 15's 2:1 box reduce while scanning,
  decoding on a rayon pool, and 200 dpi for 1D rules (slices 5 and 7;
  Left).
- **The CI page's content is bytes:** sheaf-ops' clippy.toml bans
  `Content::encode`, so `BT /F1 11 Tf … Tj ET` and `re f` are written
  as a stream's bytes.
- **The full corpus tier ran in the slice** (Measured), not only at
  the pre-merge run. It takes about 10 minutes in debug, too close to a
  foreground shell's limit, so step 5 ran it in the background and
  waited on its PID (`tail --pid`).
- **The UTF-8 test, renamed:** the plan's
  `a_title_outside_latin_1_reads_back_exactly` would pass without the
  `CharacterSet` hint (rxing's encoder already falls back to UTF-8
  with its ECI outside Latin-1), so its fault check couldn't fail. It's
  `titles_outside_ascii_read_back_as_utf_8`, with a Latin-1 title
  ("Müller & Söhne ½", written as ISO-8859-1 without the hint) beside
  the CJK one, and it checks the raw bytes are the text's UTF-8.
- **Added beyond the step list:** `the_same_text_makes_the_same_modules`
  pins the 21 × 21 picture of `qr("SHEAFSEP1|t=")` (byte mode with its
  ECI), so a change in rxing's encoder that would change every sheet's
  bytes shows across builds, not only within one process;
  `every_degradation_changes_the_page` (a degradation made a no-op
  would leave the matrix green);
  `bars_shorter_than_a_patch_read_as_none` (no test pinned the patch's
  length rule); `patch_sheets_carry_their_type_as_code_39`; the
  turned-labels test covers all four turns and checks the edge,
  inset and length; the patch negatives are skewed ±3° too; the real
  page asserts it has ink (9.4% at 150 dpi), so a page whose text
  didn't draw can't pass by reading nothing.
- **The prototype's patch reader, changed in two places:** lines are
  gathered by type and place along the edge, each inset compared with
  the last agreeing line's (the prototype grouped by inset and compared
  with the first), so a skewed edge's bars can drift; and a line's runs
  are matched with `windows(9)` (the prototype's loop skipped a patch
  whose light after it was the band's last run). `Patch.inset` is the
  agreeing lines' mean. Lines are `0.05 in × dpi` pixels apart rounded
  down, 0.04 in at 75 dpi, as in the prototype; the doc says 0.04.
- **Smaller changes:** `scanned_every_way`, `VARIATIONS`, `SHEET` and
  `SAMPLES` are pub in the fixture, so the patch tests and slices 4 and
  5 share the matrix; `code_page` takes `row_px` (a 1D code's bar
  height, a 2D code's module); `fixture::encode` panics on an encoder
  error (a fixture); the real page has 36 body lines at 11 pt, not
  about 55 (55 don't fit a Letter page with a header and a table); the
  sheet test (`a_sheet_with_patch_bars_reads_both`) puts the Kodak
  sheet's bars over the text page as instructed, the QR 24 px from the
  left bars, inside its 36 px quiet zone, and it still reads both;
  `a_page_with_nothing_reads_nothing` reads 9 scans (300 gray, 150
  gray and 1-bit), the full grid being steps 2 and 3's; the gate's
  slow rows are collected and asserted after the table prints.
- **No guard for tiny pages beyond "nothing asked":** step 1 probed 0
  × 0, 1 × 1, 0 × 10, 3 × 3 and 40 × 40, black and white, through
  `Rxing` with every symbology, and none panicked; the patch reader's
  Otsu level is 0 on an empty image.
- **The quiet run is wider than a space** (review finding 9): `QUIET`
  was 0.08 in, a printed space, while `SPACE` takes up to 0.14 in, so
  a line through more than four bars of a patch's widths read a patch
  from its middle and print 0.1 in after a label's bars didn't stop
  it; with the rule removed nothing failed. It's 0.18 in now, above
  `SPACE` and under A-61599's 0.25 in from other print (every layout
  and degradation test still passes at 0.24).
- **The patch reader's Otsu level is its own** (review finding 7):
  imageproc 0.27's `otsu_level` counts in u32, which overflows on more
  than about 16.8 M pixels of one gray (a panic in debug, a wrong
  level in release); the reader's work image is sized by `dpi` alone,
  so a large page or a low `dpi` reached it. `patch::otsu_level` is
  the same arithmetic in u64, its level imageproc's on every page
  tested (`the_otsu_level_is_imageproc_s`).
- **The fixture keeps its own generator** (review finding 10, kept):
  the workspace's tests seed `fastrand`, but the codes' tests were
  settled on these exact pages and slices 4 and 5 build on them, so a
  dependency update mustn't draw different pages; its doc says so.
- **One `drawn`** (review finding 12): the codes tests' and the gate's
  content-only draw is `common::drawn` in sheaf-ops' tests, which
  gate_7k now includes; slice 5's `separate::find` draws the same
  picture through `Views`.

New crates (each license from its own Cargo.toml; `cargo tree -p
sheaf-image -e normal` before and after; none pinned to a
pre-release): new to the workspace, rxing 0.9.3 (Apache-2.0),
rxing-one-d-proc-derive 0.9.1 (Apache-2.0) and codepage-437 0.1.0
(MIT), and in Cargo.lock only, as codepage-437's build-dependencies,
csv 1.4.0 and csv-core 0.1.13 (Unlicense/MIT). Already in Cargo.lock
but new to sheaf-image's tree: aho-corasick 1.1.5 and memchr 2.8.3
(Unlicense OR MIT), chrono 0.4.45, iana-time-zone 0.1.65, num-bigint
0.4.8, once_cell 1.21.4, regex 1.13.1, regex-automata 0.4.18,
regex-syntax 0.8.11, scopeguard 1.2.0, simdutf8 0.1.5, thiserror and
thiserror-impl 2.0.21 and unicode-segmentation 1.13.3 (MIT OR
Apache-2.0), core_detect 1.0.0 (MIT/Apache-2.0), multiversion_no_op
1.0.0 (Apache-2.0 OR MIT), encoding_rs 0.8.42 ((Apache-2.0 OR MIT)
AND BSD-3-Clause); iana-time-zone's platform dependencies were in
Cargo.lock already, and num and syn in sheaf-image's tree (imageproc,
nalgebra). rxing ships no NOTICE file. README's License paragraph
names rxing; `packaging/licenses/` carries the engines' and models'
licenses, no crate's (sheaf's own LICENSE-APACHE is the same text).
The CI pixels-only check passes: no lopdf, PDFium, OCR, gpui or other
sheaf crate under sheaf-image.

Found while building:
- **imageproc 0.27's `otsu_level` multiplies in u32**
  (`threshold as u32 * hist_count`, and `width * height`): on an image
  with more than about 16.8 M pixels of one gray level it panics in
  debug and gives a wrong level in release. The patch reader no longer
  calls it (review finding 7: its own `otsu_level` counts in u64);
  clean.rs still does, on reduced images as far as step 3 saw (not
  every caller traced). Upstream.

What review found and fixed: the slice plan was reviewed adversarially
before the steps started, and its findings are the deviations above,
as resolved (the planner's list). Each step checked its new checks by
switching their behavior off (Checked by tests). The wrap-up read the
codes module, the patch reader, the fixture and both test files again
against the plan and found nothing to fix in the code; the plan's
text was behind it in Research's rxing features, proposal 15's one
decode, slice 3's Builds and Checks and the Risks' codes, now as built.

The code review of the slice's changes then found 13 things; 12 were
fixed and one kept with a reason:
- **A turned-down code hid a later one** (findings 1 and 4, major):
  `Rxing::read` returned the first code of any pass, so a vendor's QR
  code hid a rule's Code 128 on the same page every time (a probe: a
  QR URL and a Code 128 read for [Qr, Code128] gave the QR). `Reader`
  takes `accept` now and each symbology is decoded alone (Deviations);
  `a_code_turned_down_never_hides_a_later_one` pins it.
- **The corpus tier looked for a patch sheet's Code 39 only on the
  first code** (findings 1, 5 and 8): on the 16 baseline pages whose
  first code was 2D, no Code 39 was ever read. The tier reads every
  symbology taking only a sheaf code or a `PATCH…` Code 39 now, so
  both are looked for past every other code on every page.
- **The quiet run and the 8-line rule were pinned by nothing**
  (findings 2 and 9): `QUIET` is 0.18 in (Deviations), and
  `bars_without_light_round_them_read_as_none` (print 0.1 in after a
  label's bars; seven bars in a row) and
  `bars_too_few_lines_cross_read_as_none` (dashed bars on every third
  line, 0.72 in long but 7 lines; on every line, a patch) pin them.
- **Docs** (findings 3 and 11): the sheets' insets (Kodak's 0.62 in,
  Alliance's 0.63) in slice 3's Checks; A-61599's bar length (0.75 in,
  2 in for Kodak's Patch-Reader scanners) in patch.rs and Research;
  "decoded once" is "binarized once" in codes.rs.
- **`Modules`' sides were public beside its private modules** (finding
  6): `width()` and `height()` now, and `dark` asserts its column and
  row (`a_module_past_the_code_s_side_panics`: `dark(21, 0)` of a 21 ×
  21 code read row 1's first module).
- **imageproc's Otsu level overflows** (finding 7): the patch reader's
  own (Deviations); `a_page_too_big_for_imageproc_s_otsu_level_reads`
  (4,200 × 4,200 white at 75 dpi) panics with imageproc's.
- **Two copies of the content-only draw** (finding 12): one, in
  common (Deviations).
- **Untested choices** (finding 13):
  `the_type_read_along_the_most_length_is_the_page_s` (`strongest`:
  the most length summed over edges, the first of `ALL` on a tie) and
  `codes_asked_for_read_in_a_fixed_order` over every neighbor in the
  order (QR and Data Matrix, Data Matrix and PDF417, PDF417 and Code
  39, Code 39 and Code 128, the later named first and drawn where a 1D
  read meets it first).
- **Kept: the fixture's own generator** (finding 10; Deviations).

Measured (AMD EPYC 7302P, 32 threads, release builds unless marked):
- **The codes gate** (`codes_on_a_page_with_none`, the first test of
  the gate run below, at a 1-minute load of 3.5; each row the median
  of 21 reads after a first that found nothing):

  | Row | Now | The bench |
  |---|---|---|
  | stroke page, 150 dpi gray, QR + patches (asserted ≤ 60 ms) | 14.2 ms | 44 ms + 5 |
  | stroke page, 150 dpi 1-bit, QR + patches (asserted) | 13.4 ms | 44 ms + 5 |
  | the synthetic 7k's page 1 (Helvetica), content alone at 150 dpi gray, QR + patches (asserted) | 13.5 ms | 44 ms + 5 |
  | the heavy 7k's page 1 (embedded DejaVu), the same (asserted) | 13.0 ms | 44 ms + 5 |
  | stroke page, 150 dpi gray, every symbology + patches | 56.1 ms | 44 ms + 5 |
  | stroke page, 200 dpi 1-bit, every symbology + patches | 80.9 ms | 70 ms + 5 |
  | stroke page, 150 dpi gray, patches alone | 6.25 ms | about 5 ms |

  Real glyphs cost no more than the strokes. Read for sheaf's sheet
  alone, a page with no code takes about a third of the bench's
  figure, which was for every separator symbology; read for every
  symbology with its patches, the stroke page takes 56 and 81 ms,
  against the bench's one call's 44 and 70 (plus 5) on its own page
  (DejaVu, 1-bit). The fixed order itself costs 2–3%: a temporary test,
  timed in release at a load of 13 and removed, read every symbology
  in 50.0, 48.2 and 76.2 ms (150 dpi gray, 150 1-bit, 200 1-bit) in
  the fixed order and 49.0, 47.0 and 74.4 ms in one call with the five
  formats; Data Matrix's reader is the dearest, 22 ms at 150 dpi and 51
  at 200 (QR's 8–10 and 23). Step 4's first run, at a load of 27:
  13.1–14.5 ms asserted, 56.5, 80.3 and 6.23 ms measured.
  After review (each symbology decoded alone, past codes turned down;
  the patch reader's own Otsu level and `QUIET` 0.18), with
  `SHEAF_GATE_CODES=1` once the load was under 4 on two checks 30 s
  apart (2.9–3.0): asserted 15.0, 13.7, 13.9 and 13.4 ms; every
  symbology 60.4 ms at 150 gray and 87.8 at 200 in 1-bit; patches
  alone 6.28 ms. The rows read for QR alone are unchanged (one pass
  either way). Decoding Code 39 and Code 128 apart is the correctness
  fix's price on a page with no code: back to back at a load of
  3.5–6, the 1D symbologies in one call took 56.0 and 81.5 ms and
  apart 60.6 and 88.2 (8%), taken, since in one call a form's Code 39
  id hides a rule's Code 128.
- **The gates** (`gate_7k`, release, `--ignored --test-threads=1` in
  one run with `SHEAF_GATE_CODES=1`, started once the 1-minute load
  was under 4 on two checks 30 s apart: 3.5–6.1 through the heavy's
  timed gates, 4.3–5.0 through the scanned's, 3.6–4.3 through the
  synthetic's and under 3 through the history gates; 13 of 13 tests
  pass, every asserted gate, both memory rows; 2,933 s):

  | Row | Synthetic | Heavy | Scanned |
  |---|---|---|---|
  | geometry walk (asserted < 100 and < 250 ms); ready (asserted < 1 s on the heavy) | 48.8 ms; 60.3 ms | 80.6 ms; 348 ms | 74.1 ms; 466 ms |
  | realize + open + render 1,024 px, an edited page (asserted < 15 and < 25 ms) | 12.0 ms (slice 2: 12.0) | 21.5 ms (22.0) | 32.7 ms |
  | thumbnail strip, crop + watermark + header, 320 px (asserted ≥ 310 pages/s) | 420 pages/s | 352 pages/s | 163 pages/s |
  | recognized text on every page: census (asserted 1 font, one block a page); the Append | 1, 7,000; 9.89 s | 1, 7,000; 10.09 s | 1, 7,000; 9.92 s |
  | one-mark Append on the scanned (asserted < 50 ms); reorder Append on the file opened again (asserted ≤ 1 s) | | | 25.7 ms; 877 ms |
  | the Rewrite against slice 8's writer, in the gates' own process and in `rewrites_on_the_synthetic_and_heavy_7k`'s (fails past 1.3×, flagged past 1.2×) | 151 ms, 1.12×; 153 ms, 1.14× | 746 ms, 1.05×; 739 ms, 1.05× | |

  The rest: history's 200 OCR versions of 1,000 pages 236 MB (≤ 256
  MB, asserted); snapshot 0.24–0.28 µs and `Tx::check` of a delete of
  every page 0.42–0.63 ms; engines after 220 groups 28.9–29.0 MB
  weighed, ≤ 6 open; crop all pages 9.97–13.3 ms; Bates apply
  0.62–0.64 ms, one font; AddMark 0.020 ms and a full reorder 0.97 ms
  at 20,000 marks; ready on 20,000 sheaf marks 614 ms (< 1 s; slice
  2's run 519); "Page n of N" moved after a save, the Append 362–810
  ms (≤ 1 s), sections 1.36–3.33 MB (≤ 8 MB). The scan memory gate
  (`scan_of_500_black_and_white_pages`, load 2–39 through it, the
  photo profile's own threads among it): photo 0.308 pages/s, 232 MB
  peak, 177 MB added (< 400 MB, asserted); scanner 1.15 pages/s, 239
  MB peak, 186 MB added; the untitled sidecar rows as slice 2 left
  them (10.4 MB in 28.0 and 211 ms; the color sample's 6.10 MB in 196
  ms). Proposal 8's accepted misses (`SHEAF_GATE_PROPOSAL_8`):
  synthetic 22.4, 20.4, 18.8 and 28.5 ms, heavy 38.3, 32.3, 33.5 and
  41.3 ms (slice 2: 22.7, 20.4, 19.2, 29.8; 36.3, 32.4, 32.4, 42.5),
  the Appends on the file opened again 895 and 1,361 ms (1,073 and
  1,517). The flatten export's RSS (`SHEAF_GATE_FLATTEN`): the
  heavy's peak 517 MB (slice 2: 515), the synthetic's 88.8 MB (88.7),
  the scanned's 1,077 MB (1,076). This slice's code reaches none of
  these paths (sheaf-image's codes module is called only by tests and
  the gate's codes rows), so the differences are the machine's.
- **Debug times** (sheaf-image at opt-level 1): in the workspace run,
  sheaf-image's 58 unit tests took 50.4 s on 32 threads beside the
  other test binaries. Step 3's whole sheaf-image suite pinned to 4
  cores (`taskset`, `--test-threads=4`, standing in for CI's runners)
  took 67.5 s with a 1.14 GB peak, against 25.9 s after step 2: about
  a minute more on each CI system's test step. Each matrix test takes
  8.5–11.9 s alone and each patch layout test about 44 s, nearly all
  of it the degradations (a page's 48 scans about 7 s: the Triangle
  resample about 250 ms a 300 dpi page, the blur 420 ms, the noise 290
  ms); the patch reader takes about 9 ms a page, the codes tests' peak
  is 1.7 GB on 32 threads (about 230 MB a matrix test, holding its 48
  scans). No cell was cut.
- **The corpus tier** (`scans_hold_no_separators`, debug, in step 5,
  at a load of 7–23 from the other checkout): 709 files and 5,697
  pages drawn at 150 dpi and read in 577 s (9 min 37 s wall), a 1.54
  GB peak; 0 sheaf codes, 0 patches (bars or Code 39 `PATCH…`), no
  rxing panic, no file failed, no "skipped: no corpus". The patch
  reader needed no fix on real scans: no feeder border, rule or column
  of text read as a patch. The baseline, the first code each page
  gives read for every symbology, 25 on 19 files (all under
  `scans/safedocs/`; pages from 1; the texts as read):

  | File | Page | Symbology | Text |
  |---|---|---|---|
  | 0000504 | 1 | QR | `https://www.igbce.de/igbce/mitglied-werden/beitrittsformular/` |
  | 0000574 | 1 | Code 128 | `2061622` |
  | 0001676 | 1, 2 | Data Matrix | `000112213493100000004770010177000007`, `…4767010174000008` |
  | 0002101 | 1 | QR | `http://www.pizzafaenza.com/` |
  | 0002505 | 2 | QR | `http://www.hannover.de/zurschule-flyer` |
  | 0003187 | 1 | Code 128 | `ES002441219T3` |
  | 0004119 | 14 | PDF417 | `M_DG#06400802208#PG#24/08/2018#0004000.E` |
  | 0004944 | 1 | QR | a plant-co.jp campaign URL |
  | 0005707 | 1 | QR | `みゆきデイサービスセンター\r\nhttp://care-support.link\r\n` |
  | 0010929 | 1 | Code 39 | `US009782084B2` |
  | 0011731 | 1 | Code 39 | `US005464133A ` |
  | 0012942 | 1, 3, 5, 7, 9 | Data Matrix | `0205485000128`, `0305486000128`, `0405487000128`, `0505488000128`, `0105484000128` |
  | 0019069 | 2 | QR | `http://www.marukasa.com/` |
  | 0020336 | 2 | QR | a Mexican report's stamp (`ente: Isla\r\nrpt: Proyecciones de Egresos - LDF …`) |
  | 0020822 | 1 | Code 128 | `0000000000044306` |
  | 0026808 | 1 | Code 39 | `4428056894` |
  | 0049724 | 1 | QR | a TÜV certificate URL ending in five NULs (the code's own padding, as far as can be told) |
  | 0051867 | 1, 2 | Code 39 | `0810` (short enough to be a junk read, but read with the formats limited, so listed), `SR97006307` |
  | 0067466 | 1 | Code 39 | `US008359275B2` |

  By symbology: QR 8, Data Matrix 7, Code 39 6, Code 128 3, PDF417 1.
  Run again after review (debug, load about 5–7), the separator read
  now looking for a sheaf code and a `PATCH…` Code 39 past every other
  code on every page: 709 files, 5,697 pages in 497 s (8 min 17 s
  wall), a 1.53 GB peak; 0 sheaf codes, 0 patches, no file failed, no
  "skipped: no corpus"; the baseline the same 25 codes on the same
  pages. Its two fault checks, narrowed to 0049724 and 0010929, still
  fire (Checked by tests).
- **The corpus** (`../pdf-corpus`, `SHEAF_CORPUS` unset; no "skipped:
  no corpus" line), every count the baseline's: materialize_corpus's
  identity 4,389 of 4,392 written, 2,269,688 primary objects surviving,
  919,465 copied as they were, 432 changed by `keep_only`, 0 failed;
  2,901 forms without their first page, 0 failed; every page
  duplicated, 4,389, 0 failed; inserted twice into the 1040, 1,098, 0
  failed; four times, 0 differ; the logo, 0 of 2,037,600 bytes differ.
  append_corpus: 4,389 marked, 3,770 appended, 260 rewritten (211
  RemovalCheck, 34 BaseObjectsMissing, 15 BaseUnsound), 359 asked, 0
  failed, 0 derived vetoes and 211 on the files' own objects; the
  oracle finds text gone from 2,899 of 2,901 (none from 0016064 and
  0112654). encrypted_corpus: the Rewrite keeps 573 of 573 and the
  Append 572 (0630899 declined). The signatures tier
  (`the_corpus_s_signatures_agree_with_pyhanko`): 413 signed files,
  0 disagree. The other ignored tiers are the pre-merge run's (Greg,
  2026-10-20): this slice touches neither the writer nor the realizer.
- **Suites:** `cargo test --workspace` 1,796 passed, 0 failed and 60
  ignored (the baseline's 1,770 and 58; this slice's 26 tests, and the
  gate test and the corpus tier ignored), no "skipped: no corpus", 11
  min wall, a 4.95 GB peak. fmt clean; clippy (`-D warnings`, all
  targets) clean on the workspace and on sheaf-image alone with no
  features; clippy with sheaf-app's Linux-only gates switched off
  (`nonlinux-clippy.sh`) clean; on `x86_64-pc-windows-msvc`, `cargo
  check -p sheaf-image --all-targets --features fixtures` and clippy
  (`-D warnings`, all targets) for sheaf-image with and without
  `fixtures` and for sheaf-ops pass. CI's pixels-only check finds
  nothing under sheaf-image. The interop tier and the release
  properties weren't run: the slice writes no file and doesn't touch
  the writer. No binary calls `codes` yet, so the release `sheaf`
  binary's size is slice 5's to record, when `sheaf separate` links
  rxing in.
  After review: sheaf-image's suite 65 of 65 (the 58 and 7 new),
  sheaf-ops' codes test and its tier (above); fmt and workspace clippy
  (`-D warnings`, all targets) clean, sheaf-image's clippy clean with
  and without `fixtures`, and on `x86_64-pc-windows-msvc` clippy for
  sheaf-image (with `fixtures`) and sheaf-ops (all targets, gate_7k
  now including common) passes. The workspace suite wasn't run again:
  only sheaf-image and sheaf-ops' tests changed, and clippy compiled
  every sheaf-ops test crate that includes common. No proptest seed
  was added. The final verification ran it all again: `cargo test
  --workspace` 1,803 passed (the 1,796 and review's 7), 0 failed, 60
  ignored, no "skipped: no corpus"; fmt, workspace clippy,
  `nonlinux-clippy.sh` and the Windows check and clippy of sheaf-image
  and sheaf-ops clean; the corpus counts the baseline's, as above.

Checked by tests:
- **rxing and QR modules** (codes/tests.rs):
  `a_qr_code_s_modules_read_back_as_their_text` (at 4 and 9 px a
  module, a 4-module quiet zone added),
  `titles_outside_ascii_read_back_as_utf_8` (Latin-1 and CJK titles,
  their raw bytes the text's UTF-8),
  `qr_codes_are_level_m` (rxing's error correction metadata "M"),
  `the_same_text_makes_the_same_modules` (and the pinned 21 × 21
  picture), `the_longest_title_fits_version_9` (the 34-byte prefix and
  a 120-byte UTF-8 title: square, at most 53 modules),
  `only_the_symbologies_asked_for_are_read`,
  `a_code_not_asked_for_never_hides_one_asked_for` (a Data Matrix and
  a Code 128 on one page, read for Code 128),
  `codes_asked_for_read_in_a_fixed_order` (each neighbor pair in the
  order on one page, the later drawn where a 1D read meets it first:
  20 reads for both, the later named first, give the earlier every
  time, and the later read alone gives it),
  `a_code_turned_down_never_hides_a_later_one` (a URL's QR code before
  a Code 128, a Data Matrix before a Code 39 `PATCHT`, a Code 39 id
  before a Code 128: read taking any code, the first; taking the later
  one's text, the later; taking none, nothing),
  `a_module_past_the_code_s_side_panics`.
- **The symbology matrix:** `qr_…`, `data_matrix_…`, `code_39_…`,
  `code_128_…` and `pdf417_reads_back_scanned_every_way`, each 48 scans
  read for its own symbology and for every one, exactly its text, every
  miss listed: 480 of 480 read.
  `blank_text_and_rules_pages_read_as_nothing` (each page as is and
  skewed ±3°, at 300, 200 and 150 dpi, gray and 1-bit, read for every
  symbology: 54 of 54 None).
  `every_degradation_changes_the_page` (every variation and bench
  entry changes a crop and ends at its dpi; a turn swaps the sides;
  the threshold leaves only 0 and 255).
- **Patch codes** (codes/patch/tests.rs):
  `kodak_label_patches_read_as_their_types_scanned_every_way` and
  `kodak_sheet_…` (6 types × 48 scans, 288 each),
  `alliance_t_reads_as_t_scanned_every_way` (48),
  `the_bench_s_degradations_keep_patch_types` (6 × 17),
  `turned_labels_tell_3_from_t_and_1_from_6` (every turn: the type,
  the edge it came to, its inset 0.5 ± 0.05 in and length 2.25–2.55
  in), `bars_shorter_than_a_patch_read_as_none` (0.4 in bars none, 0.75
  in a T, at every dpi and tone),
  `patch_sheets_carry_their_type_as_code_39` (200 dpi 1-bit),
  `pages_without_patches_read_as_none` (blank, text,
  rules, a black feeder border, a letterhead rule and the five sample
  code pages, as is and skewed ±3°, at three dpi in gray and 1-bit: 180
  of 180 none), `bars_without_light_round_them_read_as_none` (print
  0.1 in after a label's bars, and seven bars in a row, at three dpi
  in gray and 1-bit), `bars_too_few_lines_cross_read_as_none` (dashed
  bars on every third line: none; on every line: a T; at 300 and 150
  dpi, gray and 1-bit),
  `the_type_read_along_the_most_length_is_the_page_s`,
  `the_otsu_level_is_imageproc_s`,
  `a_page_too_big_for_imageproc_s_otsu_level_reads`.
- **A page's codes:** `a_sheet_with_patch_bars_reads_both` (a QR
  sheet with Kodak sheet T bars: T and the sheet's text at 300 gray,
  150 gray and 150 1-bit), `a_page_with_nothing_reads_nothing`;
  sheaf-ops' `a_page_of_real_text_reads_as_nothing` (PDFium's
  Helvetica page at 150 and 200 dpi, gray and 1-bit, every symbology:
  nothing, with ink on more than 2% of it).
- **The gate and the tier:** `codes_on_a_page_with_none` (each row
  must read nothing first) and `scans_hold_no_separators` (Measured).
- **Fault checks,** each file restored from the step's own copy and
  checked with cmp, and no proptest seed added (only
  materialize/append_rewrite_tests.txt is there):
  - no `CharacterSet` hint fails
    `titles_outside_ascii_read_back_as_utf_8` (the Latin-1 title's ü
    came out as byte 252) and the pinned picture; level L fails
    `qr_codes_are_level_m` and the picture;
    `PossibleFormats` None in the 1D call fails
    `a_code_not_asked_for_never_hides_one_asked_for` (the Data Matrix
    found first and filtered out); one call with every format asked
    fails `codes_asked_for_read_in_a_fixed_order` (DataMatrix where Qr
    was wanted);
  - `TryHarder` off fails the Code 39, Code 128 and Data Matrix
    matrices (96 of 96 missed each; QR and PDF417 read every cell
    without it, the code sitting away from the page's middle rows) and
    the fixed-order test; `Turn(180)` a no-op fails
    `every_degradation_changes_the_page`;
  - the bars read from the far side fails each Kodak layout test (192
    of 288: types 1, 3, 6 and T; 2 and 4 read the same either way),
    Alliance's (48 of 48), the bench test (68 of 102) and the turned
    labels; the patch's least length removed fails
    `bars_shorter_than_a_patch_read_as_none` (0.4 in bars read T at
    every dpi and tone). Not pinned alone in the steps: the quiet-run
    rule (with it and the stretch rules both off, 21 of 180 negatives
    read a patch) and the 8-line rule; after review, `QUIET` at 0
    fails `bars_without_light_round_them_read_as_none` on all 12
    reads, and at the steps' 0.08 on the 6 reads of print after a
    label; `LINES` at 0 fails `bars_too_few_lines_cross_read_as_none`
    (a T on every third line, at both dpi and tones);
  - after review, too: `accept` ignored fails
    `a_code_turned_down_never_hides_a_later_one`; the steps' one call
    for the 1D symbologies fails it and the fixed-order test; PDF417
    before Data Matrix, or Code 128 before Code 39, in `ALL` fails the
    fixed-order test; `dark` without its assert fails
    `a_module_past_the_code_s_side_panics`; imageproc's Otsu level in
    the patch reader panics `a_page_too_big_…` (contrast.rs:61,
    overflow); `strongest` taking the first type found, or the last
    on a tie, fails `the_type_read_along_the_most_length_…`;
  - `CODES_GATE` at 1 ms with `SHEAF_GATE_CODES` set fails the gate,
    naming its four asserted rows, and passes with it unset; one decode
    with every format, `TryHarder` and no "asked for" filter reads a
    junk Codabar "$0" off the stroke page at 150 gray (failing
    `blank_text_and_rules_pages_read_as_nothing` at 1 of 54 and
    `a_page_with_nothing_reads_nothing`, and the gate's first-read
    check on two rows), but nothing off the real Helvetica page or the
    7k's pages, as the bench found;
  - the tier's `SHEAFSEP` check off fires on 0049724 p.1's TÜV QR;
    every Code 39 counted as a patch fires on 0010929 p.1's
    "US009782084B2" (each narrowed to those two files).

For Greg:
- **Hand checks:** none reachable yet; no command reads codes until
  slice 5's `sheaf separate`. Then: printed Alliance patch-T and Kodak
  patch sheets, and SurePrep-style Code 128 sheets, scanned on the
  office's fi-6130s at 150, 200 and 300 dpi in 1-bit and gray (the
  bench's prototype read 60 of 60 renders of Kodak's sheets, but this
  slice's tests draw the layouts from geometry).
- **No gate decision:** the four asserted rows take 13–14 ms of the
  60 allowed, on real glyphs as on strokes. Read for every symbology a
  page takes 50–56 ms at 150 dpi and 81 at 200 (Measured), which is
  what slice 5's rules will cost a page beside sheaf's sheet.
- **CI time:** the matrix and the patch layouts add about a minute to
  each CI system's test step (debug, 4 cores), nearly all of it
  degrading the drawn pages; no cell was cut, as the plan asked.

Left for later slices:
- **Slice 4:** sheaf's sheet draws `codes::qr`'s modules as rectangles,
  runs merged; it defines the `SHEAFSEP1` text and its prefix constant
  (tests/codes.rs' `SHEET` literal then uses it); the fixture's
  degradations run on PDFium renders of the sheet. The longest title
  fits version 9 (53 modules, 40 mm at 0.75 mm), as proposal 13's
  layout assumed; `the_longest_title_fits_version_9` pins it.
- **Slice 5:** the Code 39 `PATCHn`/`PATCHT` text read as patch types
  (tests/codes.rs' `PATCH_TEXT` then uses it); serde and the CLI's
  tokens for `Symbology` and `PatchCode`; 200 dpi when a rule reads 1D
  codes; decoding on a rayon pool; the 297-page gate that builds on
  this slice's per-page time. `codes::read` and its `Rxing` read are
  what `separate::find` calls; `PageCodes` gives both, and the caller
  chooses, and `accept` is its match: `SHEAFSEP` for the sheet, each
  rule's pattern, `PATCH…` for a patch sheet's Code 39. Two limits to
  know. rxing gives one code a decode, so a turned-down code hides a
  second code of the same symbology on the page (a vendor's QR beside
  sheaf's sheet; rxing's multi readers, off with
  `multi_barcode_readers`, are the way if real pages show it). And
  decoding each 1D symbology alone costs a page with no code about 5
  ms at 150 dpi and 7 at 200 when both are asked (Measured), which
  the 297-page gate carries. A known limit of the patch reader, from a
  probe at the step 3 `QUIET` of 0.08 in: a Code 39 at 80 mil (bars
  as wide as a patch's) along an edge, about 8 in long, read as
  patches (Code 128 at 30–60 mil, Code 39 at 40 mil and every sample
  size read nothing, and the corpus had none); the review shows every
  separator, and the quiet width (0.18 in now, under A-61599's 0.25
  in from other print) is the first knob if real pages show it.
- **Slices 5 and 7:** the 2:1 box reduce of a 300 dpi page while
  scanning, before the read.
- **F16** (QR codes to links): a host barcodes interface could reuse
  `Reader`.
- **The pre-merge run** of the ignored tiers should add `cargo test -p
  sheaf-ops --test codes -- --ignored --nocapture` (about 10 minutes,
  nearly all PDFium drawing pages one at a time under its lock; release
  helps little, PDFium being prebuilt).
