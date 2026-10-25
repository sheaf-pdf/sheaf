# M10.5 slice 6: The content toolkit and fonts

Status: done · Part of [M10.5: the edit API](README.md)

**The content toolkit and fonts** (L; proposals 17, 22 and 29; bugs
17–19 and 22).
- **`content/{lex, marked, balance, splice, names, text_bag}`:**
  inline images by PDFium's rules, ranges across `/Contents` entries,
  `Balance`, safe splices, names used, and decoded TextBags.
- **PageBlocks:** the origins `Sheaf`, `SheafLegacy`, `Acrobat` and
  `Authored`; sheaf blocks found by stream marker and by
  marked-content marker; isolation streams; the legacy OCR pattern;
  nested-in-forms.
- **ContentWriter,** with named property lists and `place_form`.
- **Fonts:** the `Font` trait, Std14 AFM, the exact encoders and
  `Glyphless`. Embedded fonts are GID-preserving.
- **Writers moved onto the toolkit:** mark appearances, OCR stream
  writing, tapes, stamps and the comment summary (still called by the
  old writers).
- **Readers moved onto the toolkit:** `forms::check`'s `da_font`
  (forms/check.rs:760) lexes `/DA` with `content::lex`, and the stamp
  library's page-to-form (stamp_lib.rs:408) takes the page's
  `/Contents` through the toolkit's concatenation, not
  `get_page_content`.
- **`'?'` plus a notice** for unencodable text, in marks and
  generators.
- **Names:** page-stamp XObjects get `PageRes`-style names.
- Empty per-crate `clippy.toml` files are added; their bans land per
  the lint ledger.
- **Deletes:**
  - `annot::content_literal`'s encoder, `font_resources` and
    `wrap_lines`' average width;
  - `stamps::win_ansi`/`pdf_literal` (moved);
  - `stamps::strip_ops`' lopdf `Content` round trip (strip uses
    splice now);
  - `has_artifacts`' decompress scan;
  - the summary's own writer and wrap;
  - the cache-stem stamp import and its labeled-box fallback;
  - attachments re-embedded from the cache on every write;
  - `file::recover_cached`, and `stamp_preview`'s cache writes (the
    tape PDFs and the PNG pictures, stamp_preview.rs:31, :68 and
    :118–121);
  - `da_font`'s `Content::decode` and stamp_lib.rs's
    `get_page_content`;
  - `/SheafStamp` as a written XObject name.
- **Checks:**
  - Lexing plus a no-op splice is byte-identical on every corpus
    content stream.
  - An inline image containing `EI` survives a strip.
  - An ignored differential test compares the lexer's boundaries with
    PDFium's page objects over the pool.
  - A marked range spanning two streams is removed.
  - A malformed stream is an error naming the page.
  - A drop inside BT…ET that would remove a `Tf` or `Td` is refused,
    and the pixels outside a removed artifact's bbox are unchanged.
  - Classification: Acrobat's reference files give `Acrobat`, Word-
    and LibreOffice-tagged PDFs give `Authored`, and sheaf-written
    files give `Sheaf` or `SheafLegacy`.
  - `Balance` matches a reference count.
  - `é € — “ ”` in a FreeText reads back through PDFium.
  - Widths equal the AFM.
  - Identical inputs give byte-identical streams.
  - The same text in two different subsets gives equal TextBags.
  - A stamp from a cropped, rotated page lands where PDFium shows it.
  - A library stamp made from a page with several `/Contents` streams
    draws as that page does.
  - `forms::check` finds the same `/DA` fonts on every corpus form.
  - Stamps and attachments survive a reopen and re-save with an empty
    cache directory.
  - CJK mark text draws `'?'`, keeps the exact `/Contents`, and gives
    one notice.

## Notes

Done. Page content is read and written through sheaf-doc's own
content toolkit, brought forward from M11 (`content/`): a lexer that
keeps every byte and ends inline images where PDFium does, a page's
`/Contents` as one concatenation, marked ranges, `Balance`, splices
that are safe or refused, the resource names content uses, the
memoized per-page `Source::blocks` fact with who drew each pagination
artifact, fonts (the `Font` trait, the 14 standard fonts from Adobe's
AFMs with exact encoders, `Glyphless`), decoded `TextBag`s and
`ContentWriter`. The old writers draw through it (mark appearances,
tapes, the header and footer, OCR layers, the watermark's text and
the comment summary), the `/DA` check and the stamp library read
through it, text a font can't draw is a `?` with a one-time notice,
and stamps and attachments come from the document, not the generated
cache, under `Sh` names (proposals 17, 22 and 29; bugs 17's encoding
half, 18, 19 and 22's pictures).
- **The lexer** (`content/lex.rs`): `Tok { Op, Operand,
  InlineImage(data range) }` and `lex(s, &ResCx, &Limits)`, yielding
  `(Range, Tok)`. Numbers, names, nested literal strings with
  escapes, hex strings, arrays and dictionaries are one operand each,
  their nesting capped; comments are skipped; a stray closing
  delimiter is a one-byte operand, which PDFium skips too. An inline
  image is one token, `BI` to `EI`, its data found by PDFium's rules
  in order: the exact count for unfiltered data (rows padded; no
  `/CS`, or a name the resources don't have, is 1 bit of 1 component,
  as in PDFium); the end-of-data marker for `/AHx`, `/A85` and `/RL`;
  the JPEG end of image for DCT, the search remembered so it stays
  linear; `/L` or `/Length`; then a scan for `EI` with white-space
  around it. After the first four the image ends at the next `EI`
  operator. Abbreviated and full keys and filter names are read.
  `ResCx` holds named color spaces' component counts (`ResCx::read`:
  device and Cal names, ICCBased `/N`, Indexed, Separation, DeviceN
  and Lab; anything else 3, as in PDFium; `none()` for a `/DA`).
  `LexError { at, kind }`: a string, hex string, array, dictionary,
  inline image or marked content that doesn't end, an `EMC` that ends
  nothing, and each cap; lexing stops at an error and never panics or
  loops. Also `name()` (`#` escapes decoded), `object(operand,
  depth)` (an operand as a lopdf `Object`), and `Step`/`steps()`,
  operators with their operands read from the last, as PDFium reads
  them, with `Step::args()`. `object` reads an operand in one pass,
  recursing into arrays and dictionaries in order, so a deep operand
  costs what a flat one of its size does.
- **A page's content** (`content/page.rs`): `Concat`, the streams
  joined with a space between each, as PDFium's `CPDF_ContentParser`
  joins them, borrowed when there's one; `pos`/`span` map an offset
  back to (stream, offset) (`Pos`, `SpanAcross`, with `Display`) and
  `offset`/`offsets` the other way. `PageContent::read(objects,
  contents, lim)` decodes each stream within the page's
  `Limits.content.decompressed`, follows a `/Contents` reference to
  an array, reads a missing or non-stream entry as nothing, and gives
  `ContentFault::{Stream, TooLarge, Lex}`, never raw bytes; `ids()`,
  `concat()`, `dicts()` (where sheaf's stream markers are),
  `into_bytes()`, and `fault(LexError)`, which names the stream and
  the offset in it. `ReadObjects` is the toolkit's own seam for
  reading a stream by id, for `LazyPdf` and `lopdf::Document`:
  `object` reads and keeps, `read` doesn't (`LazyPdf::object_unkept`).
  `content::contents(dict)`, the page's `/Contents` walk, is shared
  with source/pages.rs.
- **Marked content, balance and names** (`marked.rs`, `balance.rs`,
  `names.rs`): `Marked { tag, props, span, depth }`,
  `Props::{Inline, Named}` (a named list looked up through
  `PropertiesDict`, the resources' `/Properties`), and `marked()` on
  the `Nest` scanner `blocks.rs` shares; unbalanced marked content is
  an error, nesting capped by `ContentLimits.depth`. `Balance { net,
  min_prefix }` and `balance()`. `ResKind` (with `ALL`), `NameSet`
  and `names_used()`: `Tf`, `Do`, `gs`, `sh`, `cs`/`CS` (device spaces
  left out), an inline image's `/CS`, `scn`/`SCN` patterns and
  `BDC`/`DP` named lists.
- **The splice** (`splice.rs`): `splice(content, res, lim, drop) ->
  Result<Vec<Cow<[u8]>>, SpliceError>`, `SpliceError::{Lex,
  OutOfRange, Overlap, Unsafe}`. Every byte outside the dropped ranges
  is kept, a stream nothing is taken from is borrowed, and a space
  goes where the bytes kept either side of a cut, or of cuts that
  touch, are neither white-space. §13's safety rules, and more
  (Deviations).
- **PageBlocks and the blocks fact** (`blocks.rs`,
  `source/mod.rs`): `PaginationKind`, `PagOrigin`, `GenId` and
  `BlockId` (parsed from `(g<hex>)`/`(b<hex>)`), `SheafBlock::{Gen,
  Ocr, Flat}`, `BlockAt::{Stream, Marked}`, `SheafBlockRef`,
  `LegacyOcr { q, layer }`, `Pagination { kind, origin, span }`, and
  `PageBlocks { marked, pagination, sheaf, isolation, legacy_ocr,
  balance, nested_in_forms }` with `read` and `of`, generic over
  `ReadObjects`. The origins are §13's: `Sheaf` in a `/SheafBlock`
  stream or with `/SheafGen` in its list; `SheafLegacy` with
  `/SheafStamp` or `/SheafSettings` in its list, drawing in
  `/SheafHF` or in a `/SheafOcrLayer` stream; `Acrobat` when the range
  paints nothing but forms, at least one, each with `/PieceInfo
  /ADBE_CompoundType /Private` a pagination kind; `Authored` else.
  `ContentError::{Read, Unsafe}` names the page 1-based ("page 12's
  content can't be read; nothing was changed"; "page 12's content
  can't be changed without changing what else it draws; nothing was
  changed"), with `page()` and `on_page(n)`.
  `Source::blocks(i) -> Result<Arc<PageBlocks>, ContentError>` keeps
  one `OnceLock` a page, reads the streams without keeping them, and
  is never called at open. `PageBlocks::read` and `of` take a
  `DrawnForms`, what's known of the XObjects pages draw by object
  (whether each is Acrobat's form, whether one has an artifact
  inside), which a source keeps for its pages; forms are read within
  the page's cap (Deviations). `Pagination::cuts` is what a strip
  takes out of a range: its span, but for a run of `Q`s right after
  its `BDC`, which stays.
- **Strip on the splice** (`stamps.rs`, sheaf-ops `derive.rs`):
  `stamps::strip(pdf, &mut page, index, kinds) -> Result<bool,
  ContentError>` splices the page's streams, writes each it changed as
  a new object and rewrites `/Contents` (bug 18: no `Content` round
  trip, no stream skipped silently, ranges across streams, nothing
  changed in place under a page that shares it). It removes the
  artifacts `stamps::stripped(&blocks, kinds)` gives, sheaf's and
  Acrobat's, never an author's (proposal 31, found in review;
  Deviations), each by its `Pagination::cuts`.
  `HEADER_FOOTER`, `WATERMARK`, `BACKGROUND` and `Watermark::kinds`
  are `&[PaginationKind]`. `derive::strip_stamps` and `watermark`
  pick their pages by `info.src.blocks(index)` and `stripped`, so a
  source with no header is never loaded, and rename an error to the
  page's place in the document. `strip_ops`, `has_artifacts`,
  `contains` and `is_kind` are gone.
- **Fonts** (`content/font/`): `afm.rs` reads an AFM's `C`/`WX`/`N`
  lines; `encoding.rs` has Standard (Helvetica.afm's codes), WinAnsi
  (Annex D's names, exact cp1252 text, `cp1252(code)`), MacRoman and
  Symbol's and ZapfDingbats' own (their AFMs), and `glyph_text`
  (glyph names to text by the AGL's rules: suffixes dropped, `_`
  parts, the ZapfDingbats list, the AGL, `uniXXXX`, `uXXXX[XX]`), word
  lists parsed once; `std14.rs` has `Std14` (all 14, `ALL`, `name()`),
  each font's widths and character-to-code map built once from its
  AFM, `encode` composing to NFC first and drawing `?` for what it
  can't encode, counted, and `resource()`, built once; `glyphless.rs`
  has `Glyphless` (moved from text_layer: `PROGRAM` is assets/pdf.ttf,
  with the ToUnicode CMap and CIDToGIDMap), each UTF-16 code unit a
  2-byte CID at 500/1000 em, every character covered, its six-object
  resource built once. The object-safe `Font: Send + Sync` has
  `encode`, `advance`, `width` and `wrap` (provided), `covers` and
  `resource`; `wrap` is `content::wrap_with(s, width, measure)`
  (breaks at spaces, a run counting one, inside a word longer than the
  line, and a paragraph at each `\n`). `Unencoded(usize)`.
  `FontRes { id: ResId, objects }`.
- **The data** (crates/sheaf-doc/assets/): Adobe's 14 Core14 AFMs
  (`afm/`), from Apache PDFBox's copies, kerning and the blank lines
  between lines removed, each saying so in a `Comment` on its second
  line; `LICENSE-afm` (Adobe's MustRead.html paragraph, word for
  word, after a note on where the files came from and how they were
  changed); `glyphlist.txt` and `zapfdingbats.txt` from
  adobe-type-tools/agl-aglfn, unchanged, and `LICENSE-agl` (BSD-3).
  Both licences are in packaging/licenses/ too, and the README's
  License paragraph names them.
- **Text bags** (`text_bag.rs`): `TextBag`, an NFC word count (`add`,
  `words`, `len`, `is_empty`); `FontDecoders::read(objects,
  resources, lim)`, each font's codes read through its ToUnicode CMap
  (bfchar, bfrange by string or array, codespace ranges for a
  composite font not encoded Identity-H/V; the CMap read with the
  lexer's steps), else a simple font's base encoding and
  `/Differences`, else Symbol's or ZapfDingbats' own by `/BaseFont`,
  else the code as it is; `text_bag(&Concat, &ResCx, &Limits,
  &FontDecoders)`, which tracks `Tf` through `q`/`Q`, decodes `Tj`,
  `TJ`, `'` and `"`, and ends a word at white-space, `BT`/`ET` and
  each text move.
- **ContentWriter** (`writer.rs`): `q`/`Q` and `cm`; gray, RGB and
  CMYK fill and stroke; an ExtGState (`GState { fill_alpha,
  stroke_alpha, blend }`, every PDF blend mode); line width, dash,
  cap and join; paths, their painting and clipping; text (`BT`/`ET`,
  `Tf` through `&dyn Font`, `Tm`, `Td`, `T*`, `Tr` with every
  `TextMode`, `Tz`, `TL`), `text()` returning `Unencoded`; `draw(Res)`
  and `image(Res, Box2)`; `place_form(form, bbox, matrix, target)`,
  which maps the form's Matrix·BBox onto `target`; and
  `begin_marked(tag, props)`, a named list only, and `end_marked()`.
  Numbers have at most four decimals, trailing zeros trimmed, no
  exponent and no `-0` (NaN and infinity write 0); strings are
  literal and escaped, or hex when a byte is a control code
  (`write_string`). Names are bound at `finish(&Reserved, given) ->
  (Vec<u8>, ResourceNeeds)`: a given name as given (`/SheafHF`,
  `/SheafOcr`), reserved first and never made for anything else; any
  other `/Sh` and six base32 characters (the top 30 bits of the
  `ResId`), counted up past a name `Reserved` binds to something else
  or one this finish used, and reused where `Reserved` binds it to the
  same resource (`Reserved::read(objects, resources)`: by number, or
  by content for an object that reaches no other). `ResourceNeeds` is
  one `Need { kind, name, res }` a `ResId`, in first-use order.
- **Resources** (`res.rs`, `to_lopdf.rs`): `ResId(u128)`, the xxh3-128
  digest of a canonical walk of a resource's objects (kinds and
  lengths tagged, dictionary keys sorted); `Res { id, body:
  ResBody::{Objects, Written(ObjectId)} }` with `Res::of`, `dict` and
  `written`, and `From<FontRes>`. The transitional
  `content/to_lopdf.rs` (6 to 9): `ResObjects`, one object a `ResId`
  a save (`object()`, `stream()` for the legacy isolating `q`s, and
  `bind(pdf, needs, resources)`, which copies a category that's a
  reference before changing it), and `resources_of(pdf, page)`.
- **The generators' writers on the toolkit:** the header and footer
  (`stamps::to_content(placed, color, geom, &Reserved)` in Helvetica
  under `/SheafHF`, its `/Type /Pagination … /SheafStamp true` list
  named in `/Properties`; `install(…, &mut ResObjects)`, the leading
  `Q` and the shared `q` kept; `win_ansi`, `pdf_literal`,
  `StampObjects` and `objects()` gone); OCR (`text_layer::to_content`
  in `Glyphless` under `/SheafOcr`, `TextMode::Invisible`, the legacy
  shape kept, `OcrObjects` and `ocr_objects` gone); the watermark's
  text, encoded by `Std14::HelveticaBold` and written by
  `write_string`, its font dictionary Std14's (bug 17's encoding
  half); and the comment summary (sheaf-ops), each sheet a
  `ContentWriter` drawn through a private `Draw` trait, `runs()`
  splitting text into Helvetica or Helvetica-Bold and ZapfDingbats
  (✓ and ✗ drawn as ✔ and ✘), lines wrapped by `wrap_with`, the
  printed page placed by `place_form`, one `ResObjects` for the file;
  `Sheet`, `wrap`, `width`, `dingbat` and `use_form` gone. save.rs and
  incremental.rs keep one `ResObjects` a save.
- **Marks and tapes on the toolkit** (annot.rs): every kind's drawing
  goes through `ContentWriter` (rects, ellipses, clouds by
  `cloud_path`, arrowheads, attachment icons, carets, lines, notes,
  the text stamp's frame, tapes). All text comes from one
  `runs(a, shown) -> Vec<Run { font, size, at, text }>` (FreeText and
  Callout wrapped by `Font::wrap` at the box less its inset, the
  measure and connector labels, a page stamp's text, a text stamp's
  lines, ticks in ZapfDingbats, a tape's rows by `tape_runs`), drawn
  by `draw_runs`; `Annot::unencodable()` reads the same runs.
  `annotation(a, stamp) -> Appearance { bbox, content, needs }`, and
  `Appearance::write` binds the needs through the save's
  `ResObjects`, so a save's appearances share their fonts and
  ExtGStates (bug 19). `/DA` is unchanged (`/Helv`). `annot::WriteCx {
  res, assets, … }` goes through `to_pdf_in`,
  `save::write_page_annots`, `build_rewrite`, incremental.rs and
  `exchange::gather`, with the text layer and the header and footer
  on its `res`; `save::StampForms` and
  `annot::{content_literal, font_resources, wrap_lines, HELV,
  HELV_BOLD, ZADB, color_ops, stroke_ops, cloud_ops}` are gone.
- **`?` and its notice:** `EditOutcome.unencodable` from `AnnotAdd`,
  `AnnotUpdate`, `ConnectorAdd`, the marks `InsertPages` and
  `ReplacePages` bring (`import_marks`) and `SetHeaderFooter`
  (`HeaderFooter::unencodable()`, the first page's slots expanded),
  ORed by `apply_batch`; `Watermark::unencodable()` for the app's
  watermark flow; `stamps::FONT` and `watermark::FONT` tie each
  writer to its check. `Document::unencodable_notice(&outcome)` gives
  "Some characters can't be drawn in the built-in fonts and show as
  “?” in the file; the text itself is kept" once a document
  (`told_unencodable`, outside history). The app shows it after an
  edit (`DocView::finish_edit`), beside `form_notice` in the insert
  and replace flows, and in the header and footer and watermark
  flows.
- **Stamps and attachments from the document** (`assets.rs`, file.rs,
  annot.rs, stamp_lib.rs): `MarkAssets`, a document's table by stem,
  outside history and shared with its snapshots (`Document::{assets,
  set_source_assets, set_saved_assets}`, `Snapshot.assets`):
  `Stamp::{Pdf(Blob), Form { src, form }}` and `File::{Source { src,
  stream }, Blob(Blob)}`, with `register`, `intern_stamp` and
  `intern_file` (keeping what was known), `register_saved` (the file
  a save wrote, replacing it), `generation` (counted at each change),
  `stamp`, `file`, `stamp_pdf` (a form made a PDF once) and
  `file_bytes`; a stem none of those has is read from the generated
  cache by path, once, and kept. `file::annots_in`
  (which absorbed `annots_and_cached`) returns `AnnotsInFile.assets`
  (`SourceAssets { stamps, files }`); `Facts.assets` is registered by
  `Document::ready`, combine and the app's insert and replace, and
  with `register_saved` by `reread_foreign` after a save. `sheaf_mark` checks that a mark holds
  what it names (`holds`: a page stamp's `/AP /N` resources name
  `/SheafStamp` or exactly one XObject; an attachment's `/FS /EF /F`
  or `/UF` is a reference), unread. `WriteCx::new(assets)` resolves
  each stamp and file once a save: a PDF through
  `stamp_lib::page_form`, a form copied with `copy_object`, one copy
  map a source, and with `WriteCx::owning(primary)` the primary's
  referenced in place; a file from a source is copied as stored, one
  from bytes embedded, and two marks of one file share its stream.
  The page is drawn with `place_form` under an `Sh` name. New store
  kinds `BlobKind::Attachment` and `MemoryKind::Library`.
  `attachments::{read_attachment, attachment_bytes}` take
  `&MarkAssets`; `exchange::{gather, import}` take the `Snapshot`, and
  import brings a comment file's stamp pages and files into the table.
  `file::recover_cached` and `generated::{store_as, store_blob_as,
  legacy_name_of}` are gone.
- **The stamp library** (`stamp_lib.rs`): `page_forms` takes the
  page's content through `PageContent` (the streams as one; an error
  names the page object) and returns `PageForm { id, bbox, matrix,
  geom }`, its `/Matrix` the page's `to_view` then a flip to y up, so
  the form draws its page as it shows, from the origin; `page_form`
  returns a `PageForm` sized as shown; `form_to_pdf` sizes its page by
  Matrix·BBox (`Affine::bounds`, which `place_form` shares) through
  the new crate-private `form_frame`.
- **The app:** `stamp_preview` renders a stamp's page and a turned
  tape with `PdfiumEngine::open_bytes` and `render_transparent` into
  an `Arc<RenderImage>` kept in memory for the session, by stem or by
  what the tape is, remembering what it couldn't make: from bytes
  given, for good; from a document's stamps, until their
  `generation` changes. `png_path` and the tape store are gone, and
  nothing goes to disk. The Stamps panel
  keeps each stamp's stem and bytes (`StampPage`), renders its tiles
  from them and interns a placed stamp into the document;
  `copy_marks` and `paste_marks` carry stamp PDFs and attachment bytes
  in a process-wide `CopiedAssets`, which a paste interns;
  `annot_elements` takes the document's assets.
- **The `/DA` reader** (`forms::check::da_font`): the last `Tf`'s
  first operand, decoded, by lexing; `Content::decode` gone from it.
- **Limits:** `Limits.content`, `ContentLimits { decompressed: 256
  MiB, operands: 65,536, depth: 512, inline_image: 64 MiB }`,
  re-exported from `source`.
- **Vendor patches** (each marked `sheaf:`, in vendor/lopdf/SHEAF.md):
  `LazyPdf::object_unkept` (lazy.rs), which reads an object without
  keeping it unless it was kept already, what it resolves on the way
  and failures the bytes decide kept as `get` keeps them; and
  `traverse_objects` (document.rs) and `prune_objects`
  (processor.rs), which keep the references found as a set beside
  their list, so pruning and renumbering are no longer quadratic.
- **Fuzzing:** `lexer` and `splice` bodies in source/fuzz.rs (the
  splice body splices each top-level marked range, all of them at
  once widened over the white-space around them so they touch
  (`touching`), and the pagination artifacts as a strip cuts them;
  it asserts that the kept content lexes as exactly the tokens kept,
  and returns how many splices it made, which the stable test checks
  the marked fixtures reach), `fuzz/fuzz_targets/lexer.rs` and
  `splice.rs`, `[[bin]]`s in fuzz/Cargo.toml, the fuzz.yml matrix,
  content tokens in pdf.dict, 11 content fixtures seeding `lexer` and
  18 `splice` (Acrobat's layout and ranges that touch among them),
  and `fuzz_bodies_survive_mutated_fixtures` mutating them (600 cases
  each) with their own token list.
- **The gate harness** (`gate_7k.rs`): "classify every page (blocks),
  cold", on a fresh Store and Source each run, with no baseline or
  gate (a missing baseline prints n/a); "Bates apply" on the file
  opened afresh each run (untimed), so each is a first apply that
  classifies every page cold (found in review: on one document, runs
  2 and 3 found the blocks known); "Rewrite, 20,000 sheaf marks"
  and its RSS added (`rewrite_of_7k_pages_and_20k_sheaf_marks`, a
  fifth FreeText and a fifth text stamps; baselines 2,314 ms and 346
  MB, taken before any code changed); `sheaf_marks()` shared with the
  ready row.
- **Lint and ledgers:** empty `clippy.toml` files in sheaf-doc,
  sheaf-ops, sheaf-render and sheaf-app, a one-line comment pointing
  at the lint ledger (clippy reads each: an unknown key in one fails
  its crate). The retired ledger has 42 rows for slice 6
  (`stamps::{strip_ops, has_artifacts, is_kind, contains, HELVETICA,
  HELVETICA_BOLD, helvetica_advance, helvetica_width, win_ansi,
  pdf_literal, StampObjects, objects}`,
  `annot::{helv_width, helv_text_width, content_literal,
  font_resources, wrap_lines, HELV, HELV_BOLD, ZADB, color_ops,
  stroke_ops, cloud_ops}`,
  `text_layer::{OcrObjects, ocr_objects, add_stream, utf16_hex,
  GLYPHLESS_TTF}`, `comments::summary::{Sheet, wrap, width, dingbat,
  Sheet::use_form}`, `save::StampForms`,
  `file::{recover_cached, annots_and_cached}`,
  `generated::{store_as, store_blob_as, legacy_name_of}`,
  `stamp_preview::{png_path, TAPES}`, `stamps_panel::Library::stems`);
  `QUALIFIED_ONLY` gains `Sheet`, `wrap`, `width`, `contains` and
  `objects`. Run over `crates/` as of 6415c8a (slice 5), every row
  hits (150 hits, no earlier slice's). The transitional ledger has
  rows for `assets.rs` (6 to 8, its cache fallback 8 for attachments
  and 19 for stamps), `content/to_lopdf.rs` with `WriteCx` (6 to 9)
  and the legacy names the namer is given (`/SheafHF` to 17,
  `/SheafOcr` to 18); the generated-cache, stale-marks and
  Objects-seam rows say what slice 6 changed.
- **Docs:** `docs/document-model.md` has a new section, "Page
  content: the toolkit": lexing and the concatenation, the
  inline-image rules, marked content, balance and names, splice
  safety, the blocks fact and the four origins (with the corpus's
  counts), strip on the splice, `ContentWriter` and its names, the
  fonts, `?` and its notice, text bags, stamps and attachments from
  the document, and pictures in memory. The Core idea's `Limits`
  (the content caps) and bytes-in-memory bullets, the open (stamps
  and files found in the file; content never read at open), Saving
  (the writer, `Sh` names, resources one object a save, stamps and
  files written once from the document) and Text layers (`Glyphless`
  through the writer) are current.

Deviations from the plan:
- **Line numbers are as of a0955fc and had moved;** the work followed
  the symbols (`da_font` at check.rs:762, `get_page_content` inside
  `page_forms`, annot.rs's `to_pdf_in` and the rest).
- **No `EditError::Content` until slice 11.** The toolkit's errors are
  `content::{LexError, SpliceError, ContentError}`; `ContentError` is
  an enum, `Read { page, fault }` and `Unsafe { page, error }`, not `{
  page, fault }`, because a splice can be refused as well as a stream
  unread, and its `Display` names the page 1-based as §13's message
  does. `on_page(n)` renames a source's page to its place in the
  document, as derive needs. Slice 11 maps it.
- **§13's names that don't exist:** `ObjRef` is `ObjectId` (the fact is
  a source's); `GenId` and `BlockId` are `u64` newtypes in
  `content::blocks`, parsed from `(g<hex>)` and `(b<hex>)`, until slice
  11's ids. Nothing writes `/SheafBlock` or `/SheafGen` until slices 16
  to 18, so `Sheaf` is tested on hand-built pages.
- **The toolkit takes `&Concat` (with `&ResCx` and `&Limits`), not
  `streams: &[&[u8]]`.** Lexing an inline image needs the resources'
  color spaces and the caps; `Concat::new(&streams)` builds one,
  borrowing a single stream, and `PageContent` holds one, so blocks
  and strip never join the streams twice. The design's "one buffer
  per stream" is one joined buffer, each stream a slice of it
  (`stream(i)`, `range(i)`), avoiding a second copy. `splice` returns
  `Vec<Cow<[u8]>>`, so strip writes a new object only for a stream it
  changed; `names_used` and `text_bag` return `Result`s (content that
  won't lex has no names to prune by and no words); an `except` span
  that doesn't map onto the content is ignored.
- **`Marked.depth` is `u16`,** since `ContentLimits.depth` (512) caps
  marked content as well as arrays and dictionaries; `q` isn't
  capped, only counted (PDFium has no cap, and a real page may nest
  deeply). `PageBlocks.pagination` holds `Pagination` structs, not
  tuples.
- **`PageBlocks.marked` keeps the artifact ranges and sheaf's marked
  blocks, not every structure range,** to bound the memoized fact: on
  the corpus 144,250 ranges over 24,256 pages, at most 773 on a page;
  a tagged page has an MCID range for every run of text.
- **A pagination artifact is an `/Artifact` range whose `/Subtype` is
  one of the four kinds, whatever its `/Type`, the outermost such
  range** (one nested in other marked content is still found).
  `/Type /Pagination` alone gives no kind to remove it by and isn't
  listed.
- **Splice safety goes past §13's two cases,** so that "can't change
  what's drawn after it" holds:
  - a range may set no graphics state outside a `q`…`Q` of its own
    (`cm`, color and color space, line style, `ri`, `i`, `gs`, `W` and
    `W*`, text state and `"`). Found in review: §13's balance rules
    let `1 0 0 rg` or `2 0 0 2 0 0 cm` go and leak onto what follows,
    and a test asserted that color alone could go;
  - inside `BT`…`ET` it moves no line, even inside a `q`, which
    doesn't save the text matrix; and a range that shows text is
    refused if body text is shown after it in the same object before
    a line move (`Td TD Tm T* ' "`) or `ET`, since `Tj` and `TJ` move
    the text on. Found in review: setting text state (a `Tf` or a
    `Tc` between) cleared the guard as a move does;
  - it starts and ends outside a path being made, which what follows
    would paint;
  - marked content may open in one range and close in a later one,
    with no marked content kept between and none of it optional
    (`/OC` hides what it marks), so a strip can keep Acrobat's
    leading `Q` (below);
  - it starts and ends between steps, never inside a token or a
    comment or between operands and their operator;
  - a space is inserted where the bytes kept either side of a cut, or
    of cuts that touch, are neither white-space. Found in review:
    with two cuts that touched, the bytes the neighbour took decided,
    and `0 g/A Do  /B Do 0 0 1 1 re f` became `0 g0 0 1 1 re f`.
- **Acrobat's classification, read exactly:** the range paints nothing
  but XObjects (path painting, text showing, `sh` and inline images
  paint; `re W n` clips don't), draws at least one, and every one is
  a form whose `/PieceInfo /ADBE_CompoundType /Private` names any of
  the four kinds, not necessarily the range's `/Subtype`. In 6 of the
  47 corpus files whose bytes say Acrobat drew a header or
  watermark, its forms are drawn with no marked content (the files
  were distilled again), so there's no artifact to classify; the test
  counts them as unmarked and asserts that every range of a kind
  Acrobat named in a file is `Acrobat`.
- **The legacy OCR layer is found by its `/SheafOcrLayer` stream
  marker:** `q` is the first such stream that's just `q`, `layer` the
  last other one; a pagination range in such a stream is
  `SheafLegacy`. `nested_in_forms` counts forms the page draws outside
  any pagination range (one inside goes with it), read one level
  deep; one that can't be read draws nothing, as in PDFium.
- **Forms are read once a source, within the page's cap** (found in
  review: `nested` read and decompressed the form behind every name,
  so one Flate bomb drawn under thousands of names took minutes on a
  page, and a form every page draws was read 7,000 times). Names are
  resolved to objects and deduplicated; `DrawnForms`, kept by the
  source, remembers by object whether each is Acrobat's form and
  whether one with resources of its own has an artifact inside (one
  without draws with its page's, so it's read for each page); and the
  forms a page reads take their decompressed size from what its
  streams leave of `ContentLimits.decompressed`. A form past the cap,
  or met once the page's share is spent, counts as having an artifact
  inside, where it used to count as unreadable and having none. An
  image XObject is still read whole, once a source, to learn it isn't
  a form: reading only its dictionary would need another vendor
  patch. Classifying the 7k fixtures cold: 141, 203 and 76.8 ms, from
  133, 199 and 74.
- **`stamps::strip` and `watermark::read` stay, as the transitional
  ledger says** (§13 lists them among the deletes): strip on the
  splice until 17, `read` on `Content::decode` until 16, so only
  `has_artifacts`' scan and `strip_ops`' round trip went. Its signature
  is `(pdf, &mut page, index, kinds)`, `index` naming the page in
  errors. In derive's `watermark`, a page in scope isn't classified
  (its `draw` check comes first); its copy is still stripped, and
  strip names the page on an error.
- **Strip takes proposal 31's selection now, not in slices 16 and 17**
  (found in review): it removes sheaf's and Acrobat's artifacts of the
  asked-for kinds and leaves an author's (`stamps::stripped`), where
  the step kept today's "whoever drew them". With a refused range
  failing the whole op, Document > Header & Footer (add, Bates,
  remove) failed on 421 of the 689 corpus files with a header or
  footer, Acrobat's among them, where `strip_ops` had removed every
  range and never failed. Authored ranges mostly can't go safely (on
  the corpus 2,443 of 6,837 splice out alone: Word sets a font and a
  position at the top level of the body's text object, and nests `q`
  across its marked content), so leaving them is the only safe
  choice, and it's slices 16 and 17's rule anyway. Until slice 17's
  `include_authored`, Remove Header/Footer and the watermark flow
  leave an author's running heads and watermarks where they are, and
  the app says nothing of them (slice 17's dialog shows the counts).
- **Acrobat's first artifact keeps the `Q` it opens with** (found in
  review): Acrobat puts a `q` before the page's content and opens its
  first artifact with the `Q` that ends it (`/Artifact BDC Q q … Do Q
  EMC`), so the range doesn't balance. `Pagination::cuts` keeps a run
  of `Q`s right after the `BDC` and cuts either side, the marked
  content one cut opens closed by the other, which leaves the page as
  sheaf's own header does, its `Q` outside its range. Cut so, 191 of
  the corpus's 192 Acrobat ranges splice out alone (as one span, 100
  of 117 headers and 43 of 45 watermarks did). The one left, a
  watermark on page 1 of forms/safedocs/0112530.pdf, has the body's
  `q BT … Tf Tm` inside its range, closed after it, which no splice
  can take without the body; the corpus test asserts exactly that
  page, and a watermark over that file fails naming it.
- **The step's "one Limits"** (§3.1) had no content caps:
  `Limits.content` is new, with values chosen here (256 MiB of a
  page's streams, matching the reader's stream cap; 65,536 operands;
  512 deep, PDFium's array cap; 64 MiB of inline image).
- **`LazyPdf::get` keeps every object it reads** (slice 3), so the
  blocks fact would have kept 7,000 pages' streams: the vendor patch
  `object_unkept` reads without keeping. It still keeps what it
  resolves on the way (an indirect `/Length`) and a failure the bytes
  alone decide; an object-stream member is read as `object` reads it,
  since streams are never members. In `PageContent::read` a missing
  object (no xref entry, or not found) reads as null and draws
  nothing, as in PDFium; any other read error is a fault.
- **Inline images, past §13's list:** `/RL` joins `/AHx` and `/A85` at
  the end-of-data rule (PDFium finds RL's end by decoding). An AHx,
  A85 or RL image with no end marker, and an unfiltered one whose
  count runs past the end, are errors; PDFium loses the rest of the
  page then. A DCT image with no `FF D9` falls through to `/L` and the
  scan, as PDFium does. Flate, LZW and CCF images aren't decoded to
  find their end, which PDFium does; they take `/L`, then the scan, as
  the plan lists. The pool differential found no desync from it. The
  color-space rule follows PDFium rather than reading `/IM` (an image
  mask has no `/CS`, so it gets 1 bit anyway).
- **`da_font` keeps a `Tf` found before a lex error,** as the old
  `Content::decode` did on `/Helv 12 Tf (open`, rather than
  returning None. The corpus check walks every `/DA` in every object
  of each file through the check's own `da_of`, a superset of the
  strings the check reads.
- **The fonts' data came from where the plan said:** Apache PDFBox
  trunk's copies of Adobe's Core14 AFMs with MustRead.html, the
  kerning and PDFBox's double spacing stripped and noted in each
  file, Adobe's paragraph unmodified in `LICENSE-afm`; the AGL and the
  ZapfDingbats list from adobe-type-tools/agl-aglfn master, whose
  LICENSE.md has no copyright line, so `LICENSE-agl` takes "Copyright
  2002-2019 Adobe" from the files' header. The URW base35 AFMs on
  this machine (AGPL) weren't used.
- **Font choices:** `Std14::encode` composes to NFC first, so a
  decomposed "é" is 0xE9 and `covers` agrees with `encode`; WinAnsi
  has no glyphs for the control codes, 0x7F and cp1252's five empty
  codes, so those draw `?`, and cp1252 is checked both ways for every
  code with a glyph. ZapfDingbats has no `?`, so a character it can't
  encode is left out and counted. The AGL reads Symbol's Delta, Omega
  and mu as ∆, Ω (NFC folds it to U+03A9) and µ; Symbol's encoder
  also maps the Greek Δ Ω μ to those codes, and decoding keeps the
  AGL's reading. WinAnsi decodes as exact cp1252 (0xA0 is U+00A0,
  0xAD U+00AD). MacRoman has the Mac OS symbols Annex D leaves out,
  as pdf.js does, but not the Apple logo; MacExpertEncoding isn't
  read. `Font` gained a provided `width` (the advance of `encode`)
  and `Send + Sync`, and stays object-safe.
- **"Embedded fonts are GID-preserving":** the only font sheaf embeds
  in slice 6 is `Glyphless`, its program unchanged and every CID drawn
  by its one glyph, so the rule is written in the `Font` trait's docs
  for M10 slice 7's Noto subsets, with Glyphless as the exception.
- **Ticks keep their look:** with exact encoders ✓ (U+2713) is
  ZapfDingbats a19 and ✗ (U+2717) a23, where ticks and the summary
  drew a20 and a24; marks and the summary map ✓ to ✔ (U+2714) and ✗
  to ✘ (U+2718). The summary's ✗ measures 677, its AFM width, where
  the old table said 778. Text stamp and tape boxes change slightly
  with exact widths.
- **Text bags, decided here:** a word ends at white-space, `BT`/`ET`
  and text moves (`Td TD Tm T* ' "`), not at `TJ`'s adjustments;
  `q`/`Q` save and restore the font; a font the resources don't have
  reads each byte as its code; a `/Differences` name that doesn't
  read gives the code as is. Default base encodings: a `/BaseFont`
  starting Symbol, or ZapfDingbats or Dingbats (subset tag stripped),
  uses that font's own; another Type1 or MMType1 Standard; TrueType
  or Type3 with none, the code as is. A string bfrange counts its last
  character up by the whole offset, as PDFium does (the spec says the
  last byte); ranges are kept, not expanded; a CMap that stops lexing
  is read up to the error. §11.9's "an unknown encoding hashes the raw
  bytes" isn't done: the decoder reads such codes as they are, and
  slice 10 may need it to say when it fell back.
- **`FontRes` and `ResId` were undefined in the design:** `FontRes {
  id, objects: Arc<[Object]> }` is an object table (the font's
  dictionary first, `(i, 0)` references indexing it, streams held
  decoded); `ResId` is `content::ResId(u128)`. `Res`/`ResBody` is the
  general resource, `FontRes` converts into it, and writing objects
  into lopdf lives only in `to_lopdf.rs` (`ResObjects::object`).
- **The emitter (slice 9) names and deduplicates `ResourceNeeds`;
  until then** the old writers bind names at `ContentWriter::finish`
  and write the needs through `ResObjects`, one object a `ResId` a
  save (transitional ledger, 6 to 9). `ResObjects` also memoizes the
  header's and OCR's isolating `q` streams, so `StampObjects` and
  `OcrObjects` went and the installers take `&mut ResObjects`.
  `Reserved` reuses a name already bound to the same resource:
  otherwise a page re-saved after its header was stripped, whose
  `Sh`-named list stayed in `/Properties`, would gain a new bumped
  name every save. Bug 19's sharing is within a save; across saves
  it's the emitter's (slices 9 and 10).
- **The header's list is named; the watermark keeps its inline list**
  and `Content::encode`, per-page objects and `SheafSettings` until
  16; only its text encoding, extent (now Helvetica-Bold, its font)
  and font dictionary moved. The header's named list with
  `/SheafStamp` still classifies `SheafLegacy`, and strip still finds
  it.
- **The summary's text mixes fonts,** so it can't wrap through one
  `Font::wrap`: the algorithm is `content::wrap_with(s, width,
  measure)`, which `Font::wrap` calls and the summary calls with its
  run-aware `measure`. Its helpers are a private `Draw` trait (`label`
  where `Sheet::text` was, since `ContentWriter` has a `text()`), and
  its measure is `measure`, since the ledger retires
  `comments::summary::width`. To keep the `stamps::win_ansi` row off
  the encoder, encoding.rs's `win_ansi(code)` is `cp1252(code)`.
- **Writer details:** `F` is never written (`Paint::Fill` is `f`);
  `T*` was added for `TL`; `text()` with no font draws nothing and
  counts every character unencoded; the writer follows the font
  through `q`/`Q`, and a `Q` with no `q` of its own (the legacy
  leading `Q`) is allowed. `place_form` writes `q cm /Name Do Q`
  without the summary's old `re W n`: the BBox mapped exactly onto
  the target clips the same area. A matrix entry is written with `-0`
  made 0. Connector labels are centred by their Helvetica-Bold width,
  not 0.3 em a character, matching the app. FreeText and Callout wrap
  at the box width less 4 pt, as before; `wrap_with` breaks a word
  wider than the line and collapses runs of spaces, so
  `to_pdf_round_trips_through_json` keeps its text on one line. The
  unused `TL` is no longer written in marks.
- **A tape's minus (U+2212) and subtotal sign (◊), which its row's
  font lacks, draw in Symbol** rather than as `?`, or every tape with
  a subtraction or a subtotal would raise the notice. `Run::unencodable`
  counts what `encode` reports, not `covers`, so decomposed text
  counts as drawn. `HeaderFooter::unencodable` checks the first
  page's expansion (pages differ only in digits). The header and
  footer's writer ignores `text()`'s count; the notice comes from
  `HeaderFooter::unencodable` at apply. The notice also shows beside
  `form_notice` in docview's insert and page_insert's replace flows,
  whose incoming marks set it; combine.rs doesn't show it.
- **Stamps and attachments, where the ledger said attachments stay
  cache stems until 8:** marks keep their stems (no `/SheafAnnot`
  change), and the transitional `MarkAssets` table says where each
  stem's bytes are, so slice 6 could delete the per-write cache
  re-embedding and `recover_cached` while stamps and attachments
  survive an empty cache; slice 8's `Mark.stamp: FormRef` and
  `Mark.file: BlobRef` replace it. `intern` is two methods over two
  tables, since a stamp's entry may be a form with no original bytes
  and one PDF may be both. `register` and `intern_*` keep what's
  known, so a file whose pages come in (insert, replace, combine), a
  paste or a crafted stem can't replace a document's own stamp or
  file. Found in review: `register` replaced, so inserting pages from
  a file whose mark named one of the document's stems with another
  form (or an older sheaf's form, with no `/Matrix`) changed what the
  document's own marks drew at the next save, and undoing the insert
  didn't bring it back; it had deleted `store_blob_as`' guard with
  nothing in its place. Only `register_saved`, for the file a save
  wrote, replaces, every entry that file holds: the save wrote each
  from the table, so it holds the same bytes; the old file and placed
  stamps aren't pinned for them, and the next incremental save
  references the saved file's forms in place. Stems aren't checked
  against bytes: a stamp's form isn't the PDF its stem names, so
  there's nothing to check it by until slice 8's `FormRef`.
  Unregistering a source doesn't drop its entries, since a pasted mark
  can outlive it: sources stay pinned by the table until slice 8.
- **`WriteCx::owning(primary)`, past the plan:** the savers hold the
  primary's objects under their own numbers, so a form or embedded
  file the primary holds is referenced in place, not copied, and an
  incremental save no longer appends another copy each time (asserted:
  the ids stay on append). Every other source's forms go through
  `copy_object`; an attachment from another source is copied by
  `copy_stream` (an unkept read, then `save::remap` of its dict), so a
  large file isn't kept in that source's `LazyPdf`.
- **A new rule, from the fixtures' "absent" stems:** a sheaf page
  stamp whose appearance draws no page, or an attachment with no
  embedded file, opens as another tool's comment, as a stale mark
  does (slice 5's `Facts.foreign`), decided at open without reading
  the form or the file (`holds`, inside `sheaf_mark`, so `is_ours`
  agrees and a save keeps it as another tool's). A stem that can't be
  resolved at write writes the mark without its page or file, with a
  log line; refusing it at apply is slice 11's validation (§6.4).
  `forms::fixture`'s marks name `MARK_STAMP` and `MARK_FILE`
  ("fixture-stamp", "fixture-file", which the cache never names) and
  `mark_assets()` holds them; the fuzz mark fixtures keep the stamp's
  appearance and the attachment's `/FS`, without the page or the file.
  `legacy_name_of` went with the FNV stem check, and save's legacy
  attachment variant with it.
- **Exchange import, past the plan:** `exchange::import` brings a
  comment file's stamp pages (via `form_to_pdf`) and attachment bytes
  into the table when it has none, so an FDF or PDF comment import
  from another machine keeps its stamps on save; XFDF carries no
  `/SheafAnnot` and is unaffected.
- **Proposal 29's "placing a stamp interns its form":** placing
  interns the stamp's one-page PDF (`MemoryKind::Library`), and a save
  makes the form; slice 8 holds the form itself. The Stamps panel
  still writes library stamps to the generated cache (slice 19's
  delete), so a stem a plugin or an exchange import names resolves
  through the fallback; attach.rs keeps its cache write and the
  fallback reads it at first use. `BlobKind::Attachment` is a unit
  variant: the design's `{ mime }` has no reader yet.
- **Bug 19's placement:** `page_forms` bakes the page's rotation and
  visible-box origin into the form's `/Matrix`, `place_form` honors
  BBox and Matrix, and `form_to_pdf` sizes its page by Matrix·BBox,
  which import.rs's foreign stamp import shares. `page_forms` returns
  `PageForm`s; the summary's `Picture` holds one and draws through
  `place_form`, its markers still through the `Geom`.
- **`stamp_preview` remembers pictures it couldn't make,** so a
  missing stamp isn't asked for every frame, as the PNG path was:
  from bytes given (a library tile, a tape), for good; from a
  document's stamps, by the document and its
  `MarkAssets::generation` then. Found in review: one document's
  failure blocked the stem everywhere, the Stamps panel included,
  for the session, even after a paste interned it.
- **The corpus and PDFium checks live in crate tests** (the root
  tests/ isn't a package): sheaf-doc's corpus tests through
  `forms::corpus`, sheaf-render's tests/ for PDFium, gate_7k in
  sheaf-ops. The PDFium differential binds pdfium-render itself, since
  `PdfiumEngine` doesn't expose page objects, and two bindings can't
  share a test process: it's tests/content_lexer.rs, and the stamp
  tests are tests/library_stamps.rs. The lex and splice identity runs
  every page and drawn form in the default suite (about 8 s, so no
  sampling and no ignored variant), and also reads every page's fonts
  and text bag. Beyond the plan, the differential fails on any count
  difference, not only an unread page. The identity check (found in
  review: it compared a stream with itself, and pages that didn't
  read or lex were listed, never asserted) checks that the tokens
  tile the joined bytes, only `blank` (tests.rs) between and after
  them, compares each stream of a splice of nothing with that
  stream's own bytes read apart from the concatenation, and asserts
  that no page or form is unread and none unbalanced. It tiles the
  joined bytes, not each stream with `assert_tiles`, since the joined
  bytes are what's lexed (a string may run from one stream into the
  next) and `assert_tiles` passes content that stops lexing.
- **`lex::object` reads an operand in one pass** (found in review: it
  scanned a nested element's whole range again at each level, so an
  inline property list 500 deep around 16 MiB took 6.7 s to read). An
  element nested past `depth`, or one that doesn't end, makes the
  operand no object; the lexer's operands, nested within the same
  cap, never are.
- **The vendor traversal patch, past the plan:** sharing appearance
  fonts and ExtGStates per save made the 20k-mark Rewrite 1.8× slower
  (4.1 s), lopdf's `Vec::contains` making prune (1.5 s) and renumber
  (1.6 s) quadratic in shared references; the mark writer itself took
  about 0.4 s. The patch keeps a set beside the list (36 and 65 ms).
  Side effect: every writer row is now far under slice 2's baselines
  (Measured), so slice 9's "≤ 1.2× baseline" Rewrite gate is much
  looser than intended.
- **Smaller choices:** the labeled box drawn when a stamp was missing
  moved into the `PageStamp` arm in step 5 and went in step 6;
  `text_layer::utf16_hex` became `glyphless_hex`; `stamps::is_kind`,
  `contains`, `objects`, text_layer's `add_stream` and the summary's
  `use_form` went with their callers and have ledger rows, as do
  `annot::{color_ops, stroke_ops, cloud_ops}` (now `stroke_style` and
  `cloud_path`); `annot::STAMP_PAGE`, added in step 5 and deleted in
  step 6, never reached a commit and has no row; derive.rs has a test
  module of its own (pages picked by their blocks, an unreadable page
  named by its place after a move); a stray doc line above save.rs's
  header test went; `gate_7k`'s ready-on-marks setup is the shared
  `sheaf_marks(mark)`, with `SHEAF_MARKS` and `SHEAF_COPIES`.

Measured (AMD EPYC 7302P, 32 threads, release builds unless marked):
- **The 7k harness,** one full run at the end (slice 5's in brackets):

  | Row | Synthetic | Heavy | Scanned |
  |---|---|---|---|
  | geometry walk | 49.2 ms [50.3] | 75.6 ms [73.7], asserted | 61.8 ms [61.3] |
  | classify every page (blocks), cold | 133 ms | 199 ms | 74.0 ms |
  | ready | 50.7 ms [49.6] | 375 ms [426], asserted | 444 ms [446] |
  | ready, peak RSS / added | 29.5 / 22.9 MB [28.6 / 21.8] | 278 / 258 MB [278 / 263] | 515 / 509 MB [514 / 507] |
  | render 1024 px, cold / warm | 34.1 / 5.35 ms [29.5 / 5.18] | 751 / 5.01 ms [749 / 4.87] | 380 / 22.0 ms [366 / 21.6] |
  | 200 previews at 320 px | 533 pages/s [529] | 169 [169] | 151 [151] |
  | incremental save, one mark | 51.3 ms [75.6] | 144 ms [141] | 675 ms [965] |
  | Rewrite | 129 ms [248] | 771 ms [2,530] | 556 ms [777] |
  | crop all pages / the edit | 270 / 1.33 ms [339 / 1.25] | 828 / 1.20 ms [1,726 / 1.26] | 1,159 / 1.20 ms [1,356 / 1.17] |
  | Bates apply / its Rewrite | 0.557 / 470 ms [113 / 633] | 1.33 / 1,075 ms [258 / 3,147] | 0.655 / 1,007 ms [56.6 / 1,166] |
  | Bates apply, first (cold), after review | 141 ms | 202 ms | 78.4 ms |
  | flatten | 142 ms [265] | 1,587 ms [2,994] | 1,260 ms [1,279] |

  The writer rows (Rewrite, Bates' Rewrite, crop's copies, flatten)
  are the vendor traversal patch: a quarter to a half of slice 2's
  baselines (heavy Rewrite 0.25×, flatten 0.45×). What the Rewrite and
  the flatten add to the RSS is within 4% of slice 2's, and the
  incremental save's within 3% on the heavy and scanned fixtures and
  41.4 MB against 35.5 on the synthetic; the peaks (the heavy
  Rewrite's 563 MB [557], the scanned's 1,590 [1,582]) are their
  process's, the new classify row's included. **Bates apply is now
  lazy,** and the table's 0.557–1.33 ms was the warm cost: that
  median of three runs on one document hit the memoized blocks. After
  review each run is a first apply on the file opened afresh, which
  classifies every page cold: 141, 202 and 78.4 ms against slice 2's
  114, 270 and 428 (1.24×, 0.75×, 0.18×), so slice 17's < 50 ms gate
  is a classify-cost problem, not a strip one. None of the three
  fixtures has a header to strip. The review's rerun (the gates
  alone, after the fixes) put the other rows within 6% of the table
  but three whose paths the fixes don't touch: the heavy geometry
  walk (84.4 ms against 75.6, asserted under 250), the scanned crop
  (1,250 against 1,159) and the scanned incremental save (873 against
  675, and 965 at slice 5); classify read 141, 203 and 76.8 ms. History: AnnotAdd 0.686 ms and a
  full reorder 0.948 ms, asserted; 200 OCR versions 4,074 MB of steps
  and 4,738 MB of RSS, as before.
- **20,000 sheaf marks:** the Rewrite (a fifth FreeText, a fifth text
  stamps) 1,118–1,122 ms in three runs against 2,314 ms (0.48×), RSS
  added 227–228 MB against 346 (0.66×). Before the traversal patch,
  on the new writer: 4.1 s. The ready file is 11.7 MB (16.8 MB at
  slice 5), its squares' appearances sharing one ExtGState; ready on
  it 531–540 ms in five runs against slice 5's 499 ms, under the 1 s
  gate. That's the harness's build-to-build swing, not slice 6:
  6415c8a's code, built from `git archive` the same hour, opened its
  own file in 485–564 ms and this slice's file in 604–618; this
  slice's code with a one-line change to keep a given file opened
  slice 5's file in 619–633 ms and its own in 580–613, and without it
  its own in 531–540.
- **Step 5's Rewrite rows,** before and after marks were drawn
  through the toolkit and the traversal patch: the 20k-mark Rewrite
  2,314 → 1,130 ms (346 → 238 MB added), synthetic Rewrite 300 → 131,
  Bates' 750 → 462, heavy Rewrite 3,082 → 801, Bates' 3,979 → 1,086,
  heavy flatten 3,489 → 1,594; AnnotAdd 0.686 → 0.693 ms.
- **Classifying 7,000 pages** (`Source::blocks` on a fresh source,
  median of three): 133, 199 and 74 ms (step 2's throwaway run: 138,
  200 and 72); with `DrawnForms` and the forms' share of the cap,
  after review, 141, 203 and 76.8.
- **The forms corpus** (`corpus_content_lexes_splices_and_classifies`,
  test profile, optimized with debug info): 24,256 pages (575 MB of
  content) and 5,273 drawn forms in 4,392 files in 9.3 s beside the
  `/DA` test (6.8 s alone; 8.0 s after review, which tiles every
  stream, reads each apart and splices every pagination artifact). 0
  pages unread, 0 with unbalanced marked content, 0 streams changed by
  an empty splice from their own bytes, 0 Balance differences on
  24,055 pages lopdf counts (no inline images), each asserted;
  10,111,137 words drawn; 144,250 marked ranges kept, at most 773 a
  page; 77 pages with artifacts nested in forms. Origins: headers 117
  Acrobat and 3,122 Authored, footers 3,638 Authored, watermarks 45
  Acrobat and 70 Authored, backgrounds 30 Acrobat and 7 Authored; 47
  files Acrobat drew in, 41 as marked and 6 with its forms unmarked;
  246 Word or LibreOffice files mark 2,323 artifacts, all Authored.
  Each pagination artifact spliced out alone, as a strip cuts it
  (after review): Acrobat's 191 made and 1 refused (asserted, that
  one page), Authored 2,443 made and 4,394 refused, none `Sheaf` or
  `SheafLegacy` in the corpus; a page's strip of each kind group,
  `stamps::stripped`, is refused on that one page only (asserted).
  Before the review's fixes, Remove Header/Footer was refused on 421
  of the 689 files with a header or footer.
  The `/DA` check: 275,686 strings in 4,392 forms, 0 differ from
  `Content::decode`'s, 7.5 s (4.6 s alone).
- **The lexer against PDFium** (ignored, `content_lexer`, test
  profile): 23,628 pages of the pool's 402 files in 37.4 s; on every
  page the lexer's top-level painting operators equal PDFium's page
  objects by kind: text 3,069,476, path 2,425,355, image 99,685
  (inline included), form 15,844, shading 10,625; no page the lexer
  can't read. The first runs differed on a few pages' paths, from how
  PDFium makes path objects, not the lexer losing its place: a move
  after a move replaces it, a line before any move is dropped, `b*`
  always closes, and a closed lone move stroked with round caps draws
  a dot. Counted that way, nothing differs. The pool sample's pages
  also classify with 0 errors, so unbalanced marked content as an
  error costs nothing there.
- **Fuzzing** (60 s each, from earlier steps' corpora): `lexer`
  676,335 runs, its corpus 1,217 to 1,515; `splice` 241,748 runs,
  1,178 to 1,584; no crash (step 1's and 2's runs: 747k and 246,716).
  After review, on the final code, seeded with the fixtures: `lexer`
  624,521 runs, 2,911 to 4,220; `splice` 234,185 runs, 3,114 to
  4,557; no crash.
- **Suites:** `cargo test --workspace` 742 passed, 21 ignored, in 1 m
  45 s with the build (655 and 19 at slice 5), 751 after review; sheaf-doc's unit tests
  401 in 27.8 s (324 in 20.7 s). The ledger walks 269 files; over
  6415c8a's `crates/` (a `git archive` copy, the test in a scratch
  crate, debug) 9.7 s.
- **The app by hand** (Xwayland, `--script` and a Gdk window grab,
  with a scratch cache directory): a file of every mark kind written
  by a scratch tool from `every_mark_kind()` and `mark_assets()`
  opened with its page stamp's picture (the stamp page's blue square,
  transparent around it) and the turned tape's picture (red, turned)
  where poppler draws the saved appearances, in the right colors; the
  cache directory stayed empty. Not covered: placing from the Stamps
  panel, the clipboard across windows, the notice in the dialogs.

Found:
- **sheaf-render's `render_appearances` draws at 1 px/pt whatever
  width it's asked:** asked for 600 px wide on a 300 pt page, it
  returned a 600×800 raster with the page in its corner. Every caller
  passes the page width in points, so nothing shows it; the placement
  test renders at 1 px/pt. Not fixed.
- **lopdf's traversal was quadratic** in a document's objects and
  shared references (fixed by the vendor patch; the old writers'
  Rewrite, crop and flatten rows dropped to a quarter to a half).
- **Slice 2's writer baselines are now loose:** slice 9's Rewrite gate
  (≤ 1.2× today's writer) compares with numbers 2–4× the writer's
  time now.
- **Text a font couldn't draw was lost silently before:** `?` for
  anything outside Latin-1 in marks (€ “ ” – …), and ten cp1252
  characters (Š š Ž ž Œ œ Ÿ ƒ ˆ ˜) in headers; the watermark wrote
  UTF-8.
- **In review** (each fixed, and a Deviation above): a strip refused
  on most files with running heads, Acrobat's own layout among them,
  failing Header & Footer and the watermark flow; ranges that set a
  color or the CTM, or showed text before a `Tf`, passing as safe;
  cuts that touch gluing tokens; `lex::object` and `nested` doing
  unbounded work on crafted input; incoming files replacing the
  document's stamps; a stamp picture's failure blocking it for the
  session; a corpus identity check that couldn't fail; and the Bates
  row reporting the warm cost.
- **Header & Footer and the watermark leave an author's own running
  heads** until slice 17's `include_authored`, and say nothing of
  them (slice 17's counts).

Checked by tests:
- lexing plus a no-op splice is byte-identical on every corpus
  content stream:
  `content::corpus::corpus_content_lexes_splices_and_classifies`
  (tokens that tile each page's and form's bytes, each stream's own
  bytes read apart, nothing unread or unbalanced, all asserted),
  with `a_splice_of_nothing_is_the_streams_as_they_are` and the
  property `tokens_tile_any_content` (512 cases);
- an inline image containing `EI` survives a strip:
  `stamps::tests::an_inline_image_holding_ei_survives_a_strip`, with
  `unfiltered_data_is_its_exact_size`,
  `filtered_data_ends_at_its_marker`,
  `an_image_ends_at_the_next_ei_past_its_data` and
  `unended_images_dont_search_again` (50,000 DCT images, linear);
- the ignored differential:
  `content_lexer::lexer_boundaries_match_pdfium_page_objects`
  (sheaf-render);
- a marked range spanning two streams is removed:
  `a_range_across_streams_is_taken_out_of_both` and
  `a_header_across_two_streams_goes`;
- a malformed stream is an error naming the page:
  `a_pages_blocks_are_read_once_and_name_the_page`,
  `content_that_cant_be_read_is_an_error_naming_the_page` and derive's
  `a_page_that_cant_be_read_is_named`, with `errors_say_where`,
  `a_lex_error_names_its_stream`, `a_stream_that_wont_decode_is_a_fault`
  and `streams_past_the_cap_are_a_fault`;
- a drop inside BT…ET that would remove a `Tf` or `Td` is refused, and
  the pixels outside a removed artifact's box are unchanged:
  `a_splice_inside_a_text_object_keeps_the_text_where_it_was` (a
  color, and text state between the dropped text and the body's,
  refused), `a_range_that_doesnt_balance_is_refused`,
  `a_range_that_sets_state_is_refused`,
  `a_range_inside_a_path_is_refused`,
  `marked_content_may_open_in_one_range_and_close_in_another`
  (Acrobat's leading `Q` kept; optional content refused),
  `ranges_must_lie_between_steps`, `ranges_must_lie_between_tokens`,
  `tokens_either_side_of_a_range_stay_apart` (cuts that touch among
  them) and `a_header_that_cant_go_safely_is_refused`, and
  sheaf-render's
  `strip_artifacts::a_stripped_header_leaves_the_rest_of_the_page`
  (an Acrobat header drawn between the body's text objects) and
  `acrobats_header_keeps_the_q_it_opens_with` (Acrobat's own layout,
  a `q` stream first and drawing after the header); the corpus test
  splices out every pagination artifact and asserts Acrobat's go but
  for the one page above;
- classification: the corpus test (Acrobat's ranges where its bytes
  name the kind, Word and LibreOffice files all Authored, counts
  printed), `sheafs_own_from_before_are_legacy` (the header as save
  draws it, an OCR layer, `/SheafHF` alone), the watermark's
  `text_watermark_in_front_and_read_back` and
  `background_goes_first`, save's
  `header_footer_is_drawn_stored_and_strippable`,
  `sheafs_blocks_are_found_by_their_markers`
  (hand-built `/SheafBlock` and `/SheafGen`),
  `acrobats_draw_its_forms_only`,
  `artifacts_in_forms_are_reported_left_in_place`, and
  `strip_removes_its_kinds_and_nothing_else` (an author's header
  left), `a_shared_stream_is_never_changed_in_place`,
  `a_header_installed_then_stripped_leaves_the_page_as_it_was` and
  derive's `pages_are_picked_by_their_blocks` (not a page with only an
  author's header);
- `Balance` matches a reference count: the corpus test against
  lopdf's `Content::decode` (test-only), and `balance_is_the_q_count`
  (§3.4's bases);
- `é € — “ ”` in a FreeText reads back through PDFium:
  `markup_appearance::free_text_reads_back_from_its_appearance`;
- widths equal the AFM: `font::tests::widths_equal_the_afm` (every
  code of all 14, the AFM re-read independently, spot values), with
  `win_ansi_is_cp1252_both_ways`,
  `symbol_and_zapf_dingbats_encode_their_own`,
  `what_a_font_cant_encode_draws_a_question_mark_and_is_counted`,
  `slots_are_as_wide_as_helvetica_draws_them` and
  `a_text_stamp_fits_its_widest_line`;
- identical inputs give byte-identical streams, twice and on two
  threads: `identical_marks_write_identical_bytes` (every kind in the
  mark fixtures, a turned tape among them, and each turned with its
  page), `identical_headers_write_identical_bytes`,
  `identical_layers_write_identical_bytes`, sheaf-ops'
  `the_summary_writes_the_same_pages_every_time`, and
  `identical_calls_give_identical_bytes` for the writer;
- the same text in two subsets gives equal TextBags:
  `the_same_text_in_two_subsets_gives_equal_bags` (a simple TrueType
  subset with `/Differences`, a Type0 Identity-H one with ToUnicode),
  `composed_and_decomposed_text_give_equal_bags`,
  `codespace_ranges_split_mixed_codes`;
- a stamp from a cropped, rotated page lands where PDFium shows it:
  `library_stamps::a_stamp_from_a_cropped_turned_page_lands_where_it_shows`,
  with `a_cropped_turned_page_is_a_form_drawn_as_it_shows` and
  `place_form_honors_bbox_and_matrix`;
- a library stamp of several `/Contents` streams draws as its page:
  `library_stamps::a_stamp_of_several_streams_draws_as_its_page`,
  with `streams_lex_as_one` and `a_pages_streams_read_as_one`;
- `forms::check` finds the same `/DA` fonts on every corpus form:
  `corpus_da_fonts_are_as_decoded`, with
  `da_fonts_are_the_last_tf_names`;
- stamps and attachments survive a reopen and re-save with an empty
  cache: `stamps_and_attachments_survive_an_empty_cache` (Rewrite and
  incremental, the ids kept on append), with
  `a_page_stamp_is_written_once_under_a_name_of_its_own`,
  `two_attachments_of_one_file_embed_it_once`,
  `a_mark_without_its_page_or_file_is_another_tools_comment`,
  exchange's `our_marks_bring_their_stamps_and_files`, combine's
  `marks_bring_what_they_hold`, the app's
  `copied_marks_take_what_they_hold`, and assets.rs's
  `a_file_answers_for_what_its_marks_hold`,
  `what_is_known_stays_but_for_a_saved_file` and
  `the_cache_answers_last`; save's
  `a_file_whose_pages_come_in_cant_replace_a_stamp` (pages inserted
  from a file whose mark names the document's stem with another page
  leave its mark drawing its own) and the app's
  `a_picture_that_cant_be_made_is_asked_for_once_it_may_be`;
- CJK mark text draws `?`, keeps the exact `/Contents` and gives one
  notice: `cjk_text_draws_question_marks_and_is_told_once` (and the
  header and footer);
- also: `marks_in_a_save_share_their_resources` (three fonts and two
  ExtGStates for every kind),
  `a_tape_draws_its_symbols_in_a_font_that_has_them`,
  `ticks_draw_and_measure_in_zapf_dingbats`,
  `text_is_encoded_for_its_font` (the watermark's é is 0xE9),
  `a_header_goes_out_in_win_ansi_with_a_named_list`, the writer's
  `names_are_bound_at_the_end` and
  `needs_are_written_once_and_bound`,
  `an_unkept_read_keeps_only_what_it_met`,
  `fuzz_bodies_survive_mutated_fixtures` (which caught the cuts that
  touch, put back), `deep_operands_read_in_one_pass`,
  `forms_are_read_once_and_within_the_pages_cap` (2,000 names of one
  8 MiB form on 50 pages read once; past the cap, and past the page's
  share), `bounds_hold_every_corner`, and the ledger's
  `nothing_retired_is_back` with the 42 rows.

Left for later slices:
- **Slice 8:** `Mark.stamp: FormRef` and `Mark.file: BlobRef` replace
  the asset table and its attachment fallback; stale marks and marks
  without their page or file as `Foreign` records; sources no longer
  pinned by the table.
- **Slice 9:** the emitter names and deduplicates `ResourceNeeds`
  across saves; `content/to_lopdf.rs`, `WriteCx` and the
  `attachments::Objects` seam go with the old writers. Re-baselining
  the Rewrite gate against the patched writer is worth deciding then.
- **Slice 10:** `TextBag` in the removal check, where an unknown
  encoding may need the decoder to say it fell back (§11.9).
- **Slice 11:** `ContentError` mapped to `EditError::Content`; stems
  that resolve to nothing refused at apply (§6.4).
- **Slice 13:** stamp and tape pictures through the realizer.
- **Slices 16 and 17:** proposal 31's origin rules in the ops
  (`stamps::stripped` has the selection; `include_authored` adds the
  author's, most of which a splice refuses, so those ops report them
  rather than fail), PageRes pruning with
  `names_used`, the watermark onto the writer (its named list,
  per-page objects and `SheafSettings`), and `stamps::strip`,
  `watermark::read` and `/SheafHF` deleted.
- **Slice 18:** OCR's legacy shape (`/SheafOcrLayer`, `/SheafOcr`).
- **Slice 19:** the `Content::{decode, encode}` and
  `get_page_content` bans in the clippy.toml files, and the generated
  cache, with the asset table's stamp fallback and the Stamps panel's
  and attach.rs's cache writes.
- **M10 slice 7:** the Noto fallback behind `Font`, ending `?`.
- **M10 slice 12:** `stamp_preview` and the tape pictures.
