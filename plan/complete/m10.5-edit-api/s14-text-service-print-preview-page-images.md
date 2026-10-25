# M10.5 slice 14: Text service; print, preview and page images from bytes

Status: done · Part of [M10.5: the edit API](README.md)

**Text service; print, preview and page images from bytes** (M; bugs
22 and 24).
- **`TextService`,** with `TextOpts`, in PageSpace. Find, Patterns,
  selection, calculator picks, plugin text and blank detection all
  use it.
- **Print, print preview and page-image export** come from bytes in
  memory: GDI from memory, CUPS via `lp -`.
- **Blank detection** uses `content_only()`.
- **Deletes:**
  - `CharsKey::{Engine, Layer}`, `index_of_source_page`,
    `layer_hits` and `merge_source_hits`;
  - `text_layer::search`/`pattern_search`;
  - `PageChars::from_layer`;
  - primary-only Find;
  - the print and export temp files (docview.rs:3505; sheaf-ops
    export.rs:113);
  - the 16× device trick.
- **Checks:**
  - Find and Patterns hit text on inserted, duplicated, cropped and
    unsaved-OCR pages.
  - Print, preview and image export leave no temp files.
  - Blank detection ignores an unsaved header.
  - Char boxes lie within 1/16 pt of PDFium's.

## Notes

Done. Text service; print, preview and page images from bytes (bugs
   22 and 24): a page's text is read from what it draws, as its pixels
   are. PDFium's characters arrive in PageSpace with their angles, the
   16× device gone, and lines group in the direction the text runs. A
   `TextService` (`sheaf_ops::text::Text` over the views) reads each
   page's characters on the pool's worker from the engine and page its
   view renders from, under its caller's inputs, cached by render key,
   through a pool query that falls back as a render does; Find and
   Patterns walk the document's order page by page, each on its own
   engine. Find, Patterns, selection, calculator picks, text markup,
   plugin text, OCR's "skip pages with text" and blank detection all read
   through it, so inserted, duplicated, cropped, unsaved-OCR and
   unsaved-header pages have their text, and so does every page of a
   source PDFium refuses (slice 13's leftovers: its text, Find,
   selection, plugin text, OCR's check, printing and Remove Blank Pages
   now work; Trim White stays slice 15's). Printing sends its print file
   from memory (CUPS `lp -` on stdin, GDI `open_bytes`), and
   `PdfiumEngine::open(path)` is gone. The asserted 7k gates hold (the
   Rewrite 1.11× and 1.08× slice 8's writer), Find over 7,000 pages is
   measured (no gate), and the corpus counts hold.
   - **PageSpace text** (step 1, sheaf-render, sheaf-doc). `EngineChar {
 ch, rect: Option<Box2>, angle }` (`PageCharList`): PDFium's loose
 bounds as they come, PageSpace, no box for a generated character,
 the angle counter-clockwise. `PageChars::from_engine` takes `(ch,
 rect, angle)` with no `Geom`; `upright` is the quarter turn most
 boxed characters are drawn at (`most_drawn`: R0 with none, the
 lesser turn on a tie), lines grouped on `Affine::upright(upright)`.
 The `SCALE`/`points_to_pixels` code in `page_chars` is gone.
 `frames.rs`: `char_boxes_lie_within_a_sixteenth_of_a_point_of_
 pdfium_s` (every turned or offset corpus page) and
 `char_boxes_on_a_turned_page_off_the_origin_are_pdfium_s`
 (a lopdf `/Rotate 90` page, CropBox [36 48.5 560 740], a line across
 and one up the page), the three corpus drivers sharing `none_differ`.
   - **The text service** (step 2). sheaf-doc: `TextHit` moved to
 `page_chars` (the root re-export kept), `Query::{Literal, Pattern}`,
 `PageChars::find` and `text_len`, `TextService` and `TextOpts {
 inputs }`. sheaf-render: `EnginePool::query(target, page, fallback,
 f)` on a private `ask` (`Msg::Ask` carries the fallback; a free
 `fall_back` shared with `render`). sheaf-ops: `text::{Text,
 CHARS_KEPT}` (64 pages by render key, the first read the first let
 go), `Text::{new, chars, cached}`, `impl TextService for Text`;
 `Views::locate` (the render path's target, index and fallback, now
 shared), `Views::target` private, `views::answered`.
   - **Find and Patterns** (step 3). `EnginePool::walk(count, item, work,
 tx)` and `Walked::{Scanned, Done}` (`WALK_CHUNK` pages a turn
 between renders, each on its own engine, falling back as a query
 does, the engine of its last page held; a page whose item or work
 fails or panics passed over, logged), `EnginePool::opened`; `text::Found::{Hits, Scanned, Done}`
 and `Text::search`; the app's `Doc.text` (the screen's budget),
 `start_search`/`start_pattern_search` on it, `Hit::on(position,
 page, hits)`, `scanned` a count of positions. The gate harness's
 Find rows.
   - **Selection, calculator picks, markup, the notice** (step 4).
 `cached_chars` is `doc.text.cached`, `with_chars` asks `doc.text.chars`
 and reads a page again when its render key changed while it was
 read; `TextSel.chars` keeps the characters a selection was made in,
 which its paint, Copy, markup and Text Edits read (the review); the
 refused notice is "PDFium can't read this file as saved, so its
 pages are drawn from sheaf's copy".
   - **Plugins' text** (step 5). `DocContext.text: Arc<dyn TextService>`
 (`plugins::text_source`, a `Text` a call in the background's
 budget); `HostState::page_base(page, &ViewInputs)` records
 `render_reads` under the inputs given; `ViewInputs::plugin_text()`
 (the review); api/shared.rs's `drawn_text` (under `plugin_text()`,
 with `to_frame`) under `page_text`, `search`,
 `pattern_search`, `words` and `text_boxes`, and `matches_to_wit`;
 `recognize` under `content_only`. docs/plugins.md and the 0.3 WIT's
 text doc say what text is now.
   - **Blank detection and OCR's check** (step 6). `blank::find(snap,
 &Views, indexes, params, progress)` in document order: text through
 a `Text` (background) under `{ ocr_over: true, ..content_only() }`,
 pixels through `views.render` under `content_only` at about 60 dpi;
 `Views::start` (the CLI's and tests' own pool); `ViewPages` holds a
 `Text` and `text_len` is `PageChars::text_len` under `content_only`.
 The pool's background base engines grow into what the screen's
 engines leave of `SCREEN_BYTES` (the review).
   - **Print, preview and page images** (step 7). `sheaf_print::submit(job,
 Arc<[u8]>)`; cups.rs's `send` (`lp <options> -`, the job written on
 stdin from a thread while `lp`'s output is read) and `said`; gdi.rs on
 `open_bytes`; the app's `send_print`. `PdfiumEngine::open(path)`
 deleted, its 21 test and example callers on `open_bytes`
 (`sheaf_util::read_file`, or `engine_on` helpers); `sheaf-util` a
 dev-dependency of sheaf-render, and new to sheaf-ocr's; `tempfile`
 a dev-dependency of sheaf-print.
   - **Gates, ledgers, docs** (step 8). The gates run on a quiet machine
 with the new rows; retired.rs gains 51 rows and `QUALIFIED_ONLY` 16
 names, each with its reason, checked with probe files; the
 transitional row (14, search, deleted), the `temp_dir` lint row and
 slice 13's `read_render` row; docs/document-model.md's "Text and
 printing (M10.5 slice 14)" and its forward references (Coordinates,
 the consumers' table, the refused source, the pool, the app on views,
 Text layers' Search), README's Find and selection rows; the pool's
 budget docs (Trim White isn't the pool's until slice 15).
   - **Deleted,** each confirmed dead by `cargo check --workspace
 --all-targets` and clippy and listed in retired.rs: `CharsKey`
 (`Engine`, `Layer`), `chars_key`, `cache_chars`, `CACHED_PAGES`,
 `Doc.page_chars`, `EnginePool::page_chars`, `PageChars::from_layer`;
 `Matcher`, `SearchEvent`, `Msg::Search`, the old walk's `target` and
 `matcher`, `Worker::chunk`, `EnginePool::search`, `Doc::search`,
 `index_of_source_page`, `layer_hits`, `merge_source_hits` (primary-
 only Find); `text_layer::{search, pattern_search, PageText,
 page_text, fold, hit_from_span, line_of, unrotate_from_upright}` and
 its `TextHit` (moved), `TextLayer::{text, boxes_in_rect}`;
 `SearchHit`, `sheaf_render::Rect`, `Renderer::{search_page,
 pattern_search_page, text_boxes_in_rect}` and `PdfiumEngine`'s (the
 last of the 16× trick), `EnginePool::{search_page,
 pattern_search_page, text_boxes_in_rect, page_text,
 page_text_len}`; `runtime::{TextSource, TextMatch}`,
 `api::shared::{layer_matches, text_match_to_wit}`, `EngineText`
 (`hits`, `to_box`); blank detection's own engine and hand-appended
 OCR text; the print temp file; `PdfiumEngine::open`. The export temp
 file was already gone (below). sheaf-render's `regex` dependency went
 with the engine's search.

   Deviations from the plan (the planner's list first, as resolved):
   - **`TextOpts { inputs: ViewInputs }`**, not §10.5's `{ fields }`:
 what a page's text is read under is the inputs it's realized with
 (§10.4), and §10.5's `fields` (fields' display text) is M10 slice
 14's. Plugin privacy holds without it: PDFium's text page never reads
 a widget's appearance (`plugin_text_leaves_field_values_out`).
   - **The trait carries `chars` only,** and the document walk is
 `Text::search`, a stream: §10.5's trait has `search` too, but the one
 caller that searches a whole document, the app, can't block, and a
 blocking per-page trait method on the worker would wait for itself.
 Both `Text::chars` (a receiver, the UI's) and `TextService::chars`
 (blocking) are named `chars`.
   - **One cache, by `RenderKey`,** where §10.5 has untouched pages cached
 by (source, index) beside realized ones: an untouched page's key
 digests its base page, so one cache serves both, and a duplicate or
 a page in another state that draws the same is read once. It holds
 64 pages, the first read let go first (a hit doesn't refresh), and a
 search neither reads nor fills it, so a 7k walk never evicts the
 screen's selection characters.
   - **`EnginePool::query`,** generic and public, with a fallback: the
 pool's `ask` was private and sheaf-render can't name `PageChars`, so
 `Text` builds them on the worker in a closure. It's built on a
 private `ask(target, Option<PageIndex>, fallback, f)`, since
 `form_type` loads no page; `form_type` has no fallback, so a refused
 file still learns of it there (the notice).
   - **Blank detection's text is read with recognized text** (`{
 ocr_over: true, ..content_only() }`), its pixels under
 `content_only()`: §10.4 says `content_only()`, but unsaved OCR text
 counted before and is the page's (recognized text is invisible, so
 the pixels are the same either way). A failed text read still counts
 as no text, the pixels deciding; pages are walked in document order,
 and the render's width comes from the page as it shows
 (`snap.geom(id).shown()`), the same for an untouched page.
   - **Plugin text reads under `plugin_text()`,** not `content_only`:
 §14.3 says text "as the file will draw it", so a call's text has the
 page's own text, the unsaved header and recognized text. It has no
 annotation's text (marks', comments', fields'): PDFium's text page
 reads a page's content, never an annotation's appearance, so
 `plugin_text()` is `plugin()` without marks, widgets or other tools'
 annotations, and its reads are that page's `render_reads` under it
 (`Page`, every `Generator`, and the order where a page-numbering
 header calls for it). The step's code read under `plugin()` and
 recorded `PageAnnots` and each drawn `Annot` too, which nothing the
 call saw depended on (the review); `recognize` keeps `content_only`.
   - **`lp -` without `--`:** CUPS reads stdin for the file `-`; every
 option before it is generated and `-t`'s title a separate value, so
 no `--` is needed (cups.rs passed `-- <path>` before).
   - **The export temp file was already gone:** `export::flattened` opened
 `print_pdf`'s bytes with `open_bytes` before the slice (the plan's
 export.rs:113), and the only file export writes is its output's, the
 TIFF through a same-directory `.part`, which atomic writes may
 (§10.6). The Delete was met before the slice; its Check is
 `printing_previewing_and_exporting_write_no_temp_file`, and a fault
 sending `flattened` through a temp file fails it.
   - **`upright` from the characters' angles,** not the page's `/Rotate`
 (`PageChars::from_engine` took the base's turn): the characters are
 then independent of `/Rotate` and the crop box (they're in
 PageSpace; the render key, and so the cache, still has the boxes,
 and a turn only where a generator is laid out in the turned page),
 and text running down the page selects a line at a time, ending
 slice 5's limit ("selects,
 picks and underlines a character a line"). Pages whose text reads
 upright as shown behave as before (`Affine::upright(r)` is `to_view`'s
 linear part for every r). PDFium's `FPDFText_GetCharAngle` turned out
 to be clockwise (text drawn up the page by Tm `0 1 -1 0` came back as
 270), so `EngineChar.angle` is (360 − PDFium's) mod 360; the fixture
 test pins it (fault F3).
   - **Matching is `PageChars::find`'s,** not `FPDFText_Find`'s. A
 literal is one regex, compiled once per search by `Query::literal`:
 the query's words escaped and joined by `\s+`, case-insensitive
 unless it matches case. So its words match where the page's are
 apart by any whitespace (a line break, two spaces, a no-break
 space), as `FPDFText_Find`'s did (it splits the query into words and
 takes any run of space, CR, LF or no-break space between them), and
 a phrase wrapped onto the next line is found, with a rect on each
 line; matches don't overlap. A pattern runs over the text as it
 reads (words apart by a space, lines by a newline), its byte offsets
 mapped to characters with its groups; a literal takes the same
 path. The step's literal compared the query's spaces with the
 page's characters one by one, so a wrapped phrase wasn't found (the
 review); the OCR layers' own search had that rule. The fix first
 matched word by word by hand; Greg's regex form replaced it before
 the commit (less code, one path with Patterns, and the regex
 crate's case folding instead of a per-character lowercase).
 docs/plugins.md says what a search matches.
   - **`PdfiumEngine::open(path)` is deleted,** not kept for tests: its 21
 test and example callers read the file and call `open_bytes`, as
 `open` did inside; the gate's two cold opens time the same work.
   - **`Views::start()`,** new: the CLI (no sheaf-render dependency) and
 tests make views over a pool of their own for blank detection.
   - **The notice** drops "and Find and text selection don't work on it":
 "PDFium can't read this file as saved, so its pages are drawn from
 sheaf's copy".
   - **Crops read from their derived copy's base** (the plan-vs-code note
 said a derived page is never untouched): until slice 15 a crop is an
 untouched page of the derived file, viewed as `EngineKey::Base` of
 that source; its text is what that file draws, equal to the
 original's within 1/16 pt.
   - **`EnginePool::walk` takes the caller's sender** (`tx:
 UnboundedSender<T>`, `T: From<Walked>`) rather than returning a
 progress receiver, so a page's hits, `Scanned(n)` and `Done` come in
 order on one channel; `work` returns a `Result`, logged like a
 failed item. The new walk keeps the name `Walk` (`Msg::Walk`), so
 retired.rs retires the old one's fields, not the name.
 `EnginePool::opened` (engines opened, counted in `Look`) is new for
 the gate's reopen count.
   - **`with_chars` reads a page again** when its render key under the
 screen's inputs changed while it was read, rather than handing back
 a stale answer or `None` (a pick saying "No number there" after an
 edit mid-read).
   - **Text in draw order:** a page with recognized text and a header
 selects as the page's own text, then the recognized words, then the
 header ("page 5\nTotal 42\nDraft"); `from_layer` gave an OCR'd page
 the layer's words alone.
   - **A selection keeps the characters it was made in** (`TextSel.
 chars`): characters are cached by render key, and the step's
 selection read them from the cache, so on a page whose key changed
 without its text changing (a foreign comment on it hidden or edited,
 a field typed into, a page deleted under a "Page <<1>> of <<N>>"
 header; a turn only where a generator is laid out turned) it painted
 nothing, Copy fell through to copying marks, U picked the Highlight
 tool, and a Text Edit's replace did nothing, until a hover read the
 page again (the review). `CharsKey` changed only with the source,
 index or OCR layer. A selection on a page whose text did change (OCR
 recognized under it) keeps what was selected; one on a page deleted
 still goes.
   - Step-level choices, each in its step's report: the frames check
 counts and skips characters drawn FAR (2^17 pt) or more off the
 page, which PDFium's i32 device can't map (106 on
 forms/safedocs/4427660.pdf, about 2^31 pt up, which only the
 every-page variant the review dropped reached; the moved-pages test
 skips none); OCR words checked inside the layer's word box within a
 quarter point (the Glyphless font's loose box reaches about 0.21 pt
 above a 14 pt word); `Text::cached` computes the render key directly
 rather than viewing; `runtime::TextMatch` and `text_match_to_wit`
 deleted too (their only producer was `TextSource`), and
 `text_layer::line_of` (only `hit_from_span` used it);
 `HostState::page_base` returns `(&DocContext, &Snapshot, PageId)`;
 the fixture plugin's `find` reports boxes; sheaf-ocr's `pdftext`
 example searches through `PageChars::find`; the idle-CPU test runs on
 every platform, its tick check Linux-only; the gate's Find rows one
 run each (a walk takes seconds); blank detection's header test uses a
 real header and footer (a 10 pt "Draft" alone is Blank by its
 pixels); `a_source_pdfium_refuses_prints` in realize.rs beside the
 other refused-source tests, checking a mark prints too;
 `pixel_format.rs` builds its page in memory; the app test's `TMPDIR`
 is unwritable (0555), so a temp file made and deleted fails it too;
 cups.rs's `said` shared by `run` and `send`; a plugin's text reads
 count against its call's time limit, unlike its OCR and renders
 (text has no per-call cap, so crediting it would let a loop over
 `page-text` run forever); retired.rs has no
 `sheaf_render::Rect` row (a one-segment row matches a path's first
 segment, and `geom::Rect::new` is called as `Rect::new`).
   - **Slice 8's "search over `ContentStack`'s text"** is met by reading
 realizations: the realizer draws the stack (the unsaved OCR layer,
 the header), so its text is the page's.
   - **Slice 5's leftovers for 14** (PageSpace text ending the 16× trick
 and the conversions where PDFium's text arrives, the base-frame line
 grouping, `merge_source_hits`, the plugins' `TextSource` frame) are
 done.

   Found while building:
   - **PDFium's character angle is clockwise** (step 1), as above.
   - **forms/safedocs/4427660.pdf draws 106 characters about 2^31 pt up
 the page,** past what PDFium's device mapping can hold (step 1).
   - **A walk needs its fallback only for the first page it reads on a
 refused source** (step 3): that page's failed open marks the source
 refused and later items view their groups, so a test searching
 after the refusal was known passed with the walk's fallback switched
 off. The ops test now searches a word on every page of a refused
 document before anything has tried it.
   - **A 10 pt header alone is a blank page's pixels** (step 6): with
 only "Draft" as the header, the pixel fault (rendering under
 `screen()`) passed; the test now uses a real header and footer and
 checks its premise (the screen's render of the empty page isn't
 Blank, and the screen's text has the header).
   - **A small print job fits in the pipe** (step 7): a fake `lp` that
 ignores stdin isn't caught by `send` for a small job; the app test's
 parent checks what `lp` got, and the unit test sends 1 MiB.
   - **The pool's docs named Trim White as a background requester** (step
 8): it opens an engine of its own until slice 15; the module and
 `Budget` docs now name OCR, blank detection and plugins' text and
 scans, and Find's and selection's text for the screen.

   The review of the slice's changes (15 findings, each checked against
   the code; fixed unless said):
   - **Find missed a phrase wrapped onto the next line** (major): the
 literal now matches `FPDFText_Find`'s way (deviations, Matching).
   - **Find Blank Pages reopened its base engine every ~380 pages**
 (major): blank detection reads in the background's budget, which
 held a base engine to its 32 MB like its group engines; on the
 synthetic 7k 19 opens in 13.1 s. Measured first (the new gate test),
 then fixed in the pool rather than in blank detection: the
 background's base engines grow into what the screen's engines leave
 of `SCREEN_BYTES` and close past it by the base engines' rule
 (`close_heaviest_base`, shared with the screen's), never the
 screen's own; their group engines keep 32 MB, and 2 engines in all
 still bound the count. 6 opens, 12.3 s, against 10.4 s for the old
 path's one engine and `page_text` (11.3 s with `page_chars`, which
 the slice reads; a throwaway test, deleted). The review's estimate
 of 12 to 13 s on the heavy doesn't arise: every page of the heavy
 7k has annotations, so blank detection reads none. The memory bound
 stays about the screen's 96 MB and the background's 32 MB: what a
 background base engine grows into the screen's engines no longer
 use.
   - **Find thrashed past pinned engines** (major in effect, found by the
 review's reading): with `SCREEN_ENGINES` pinned, the walk's group
 engine closed after every page; 6,232 opens and 37.5 s (synthetic)
 and 88.0 s (heavy) over the headed 7k with six groups pinned. A walk
 now holds the engine of its last page (`Worker.held`, by walk id):
 `close_lru` skips it, as it does a pinned engine, until the walk
 reads elsewhere or ends, and its end settles the budget again. 49
 opens, 12.1 and 9.0 s, as unpinned. A walk past six pinned engines
 holds a seventh, as a pin would.
   - **A selection lost its characters on a key change** (minor): it
 keeps them now (deviations).
   - **Plugin text was read under `plugin()`** (minor, with the notes'
 and the code comment's claim that marks' text was in it): now under
 `plugin_text()`, with reads to match (deviations).
   - **The notes misstated the turn and the crop** in the render key
 (minor): reworded here and in docs/document-model.md.
   - **`realized_glyphs` skipped a file whose `ready` failed** (minor): a
 failed `ready` is a difference now, and the 780,238 characters are a
 floor on the full corpus (`REALIZED_GLYPHS`). A file sheaf's store
 doesn't open, or PDFium refuses, is still not compared.
   - **The ignored every-page frames variant** (390 s) is dropped: a box
 built from PDFium's own bounds checked against PDFium's own mapping
 re-tests `to_view` against PDFium's page-to-device map, which the
 every-page corners test checks; the moved-pages test, the
 off-origin fixture and the realized-glyph corpus check stay.
   - **Test helpers:** `opened`, `ready`, `file(n, kind)`, `views` and the
 `Blind` OCR engine are in sheaf-ops' tests/common/mod.rs, used by
 realize.rs, text.rs and blank.rs (text.rs's pages now draw
 realize.rs's shapes beside "page i").
   - **`Renderer::page_text_len`** is kept as the tests' oracle (PDFium's
 own count, which `PageChars::text_len` is checked against) and its
 doc says so; it no longer claims to pick OCR candidates.
   - **`SEARCH_CHUNK` is `WALK_CHUNK`** (private, no retired row), and
 `CallOutput.reads`' doc is rewrapped (two findings, one line).

   Measured (AMD EPYC 7302P, 32 threads, release builds unless marked):
   - **The gates** (`gate_7k`, release, every test once at a load of 1.6
 to 5.8, nothing else of this slice's running; the Rewrite in seven
 more processes of its own):

 | Row | Synthetic | Heavy | Scanned |
 |---|---|---|---|
 | realize + open + render 1,024 px, an edited page (asserted < 15 and < 25 ms) | 12.1 ms (2.3× the warm 5.36 ms) | 22.7 ms (4.6× the warm 4.97 ms) | 33.4 ms (1.5×) |
 | engines after 220 groups: weighed, RSS added, groups open (asserted ≤ 96 MB, ≤ 96 MB, ≤ 6) | 28.8 MB, 37.5 MB, 6 | 28.8 MB, 62.1 MB, 6 | 28.8 MB, 51.1 MB, 6 |
 | thumbnail strip after a crop and the header (asserted ≥ 310 pages/s, ≤ 96 MB) | 412 pages/s, 80.8 MB weighed, 32.5 MB added | 343 pages/s, 81.4 MB, 79.5 MB | 164 pages/s, 80.8 MB, 81.7 MB |
 | Find "Draft" over every page, as opened (new; no gate) | 11.4 s, 612 pages/s, 6 engines opened | 8.0 s, 874 pages/s, 6 | n/a |
 | Find over every page, a header on every page (new; 7,000 hits) | 12.2 s, 574 pages/s, 55 engines opened | 9.1 s, 773 pages/s, 55 | n/a |
 | Find, header on every page, 6 groups pinned (the review; `text_walks_on_the_synthetic_and_heavy_7k`, a load of 11 to 45) | 12.1 s, 49 engines opened (before the walk's hold: 37.5 s, 6,232) | 9.0 s, 49 (88.0 s, 6,232) | n/a |
 | Find Blank Pages over every page, a pool of its own (the review; same test) | 12.3 s, 6 engines opened (before: 13.1 s, 19) | 3.3 ms, 0 (every page has annotations, so none is read) | n/a |
 | one-mark Append on the scanned (asserted < 50 ms) | | | 20.7 ms |
 | snapshot, `Tx::check` of a delete of every page (asserted < 50 µs, < 1 ms) | 0.240 µs, 0.395 ms | 0.270 µs, 0.396 ms | 0.240 µs, 0.399 ms |

 The slice owns no new assertion: its Find rows are information. As
 opened, Find reads every page from the base engine, which opens six
 times (once, then again each time its loaded pages take the screen's
 engines past 96 MB, about every 1,200 pages); headed, each of the 55
 groups' engines opens once. The rest, as slice 13 left them: the
 geometry walk 48.3 and 77.0 ms (< 100 and < 250 ms); ready on the
 heavy 355 ms (< 1 s); 200 OCR versions of 1,000 pages at most 256 MB
 (12 versions, 235 MB of steps); AddMark 0.018 ms and a full reorder
 0.975 ms at 20,000 marks; 1,000 mark adds on the heavy 0.033 ms
 (blank 0.021 ms); ready on 20,000 sheaf marks 530 ms (< 1 s).
   - **Find's per-page cost against `FPDFText_Find`** (a throwaway test,
 since deleted: one engine, no pool, every page, twice): reading a
 page's characters with their boxes and angles and running `find`
 takes 11.1 s on the synthetic's 7,000 pages against `FPDFText_Find`'s
 9.0–9.3 s (1.19–1.23×), and 7.5 s against 6.2 s on the heavy (1.21×):
 four PDFium calls a character (28.7 million characters on the
 synthetic, 17.8 million on the heavy). The walk adds little beyond
 that (11.4 and 8.0 s in the pool).
   - **The Rewrite against slice 8's writer** (`rewrites_on_the_synthetic_
 and_heavy_7k` seven times plus the harness's own run, eight processes
 of the writer, each process's time the median of its runs, against
 the gate's constants from slice 8's build, 134 and 707 ms, which
 wasn't run again): synthetic 146–163 ms, median 149 ms, 1.11× (one
 process at 163 ms, 1.21×, past the flag; the rest 1.09–1.17×), RSS
 added 49.7–51.1 MB (0.81–0.83×); heavy 744–809 ms, median 763 ms,
 1.08× (1.05–1.14×), RSS added 251–253 MB (1.00–1.01×). The slice
 changed nothing on the writer's path.
   - **The corpus** (../pdf-corpus), every count as slice 13 left them:
 materialize_corpus's identity 4,389 of 4,392 written, 2,269,722
 primary objects surviving, 919,479 copied as they were, 436 changed
 by `keep_only`, 0 failed; 2,901 forms without their first page, 0
 failed; every page duplicated, 4,389, 0 failed; inserted twice into
 the 1040, 1,098, 0 failed; four times, 0 differ; the 2932995 logo, 0
 of 2,037,600 bytes differ. append_corpus: 4,389 marked, 3,770
 appended, 259 rewritten (211 RemovalCheck, 34 BaseObjectsMissing, 14
 BaseUnsound), 360 asked about signatures, 0 failed, 0 derived vetoes
 and 211 on the files' own objects; the oracle finds text gone from
 2,899 of 2,901 page-delete Rewrites (none from 0016064 and 0112654).
 encrypted_corpus: the Rewrite keeps 573 of 573 and the Append 572
 (0630899 declined). The ignored tiers (deleted means deleted,
 realize_corpus's two) wait for the pre-merge run (Greg, 2026-10-20);
 realize_corpus on five files by `SHEAF_REALIZE_ONLY` (4422502,
 4427660, 0106273, 0171994, 0630899; 43 s, debug): the predicate
 disagrees on 0 of 225 pages and inputs, and 2 of 23 pages differ from
 the Rewrite, both within the anti-aliasing allowance (0106273 page 0
 by 1 px, 0171994 page 12 by 4), 4422502 refused by PDFium and
 realized standalone.
   - **Corpus checks of the text:** frames.rs's check, 780,627 characters
 on every turned or offset corpus page, none apart (7 s); its
 every-page variant, 61,414,645 characters in 390 s, none apart, the
 106 far off the page on 4427660 counted, before the review dropped
 it; the realized-glyph check, 780,238 characters read from
 duplicates' group realizations against PDFium's on their sources,
 none apart (8 s; frames' count adds 389 boxed CR/LF characters in 37
 files, which `from_engine` drops), the count now asserted as a floor
 on the 4,392-file corpus.
   - **Suites:** `cargo test --workspace` 1,352 passed and 34 ignored
 (slice 13: 1,329 and 33; after steps 3 to 7: 1,345, 1,346, 1,342,
 1,345 and 1,348): the slice's new tests, less those that went with
 what they tested (text_layer's search and box tests,
 `ocr_words_become_characters`, `a_layer_reads_as_its_lines`, host.rs's
 `unsaved_ocr_text_wins_over_the_engine` and `search_and_text_boxes`);
 the review's four new tests; the new ignored one the gate's
 `text_walks_on_the_synthetic_and_heavy_7k`, where the every-page
 frames variant went (the review). fmt and clippy
 (`-D warnings`, all targets) are clean, and so is clippy with
 sheaf-app's, the pool's and gate_7k's Linux-only gates switched to
 `cfg(any())`, as macOS and Windows build them; `cargo check --target
 x86_64-pc-windows-msvc` passes for sheaf-doc, sheaf-edit and
 sheaf-util with their tests and for sheaf-render, sheaf-print,
 sheaf-ops and sheaf-ocr with all targets, and clippy (`-D warnings`)
 on that target for all seven (sheaf-print's GDI path included);
 sheaf-plugin, sheaf-app and sheaf-cli can't be checked here, as
 before. The release properties: Append ≡ Rewrite at 1,024 cases and
 the replay property at 4,096 pass (63 s together). The interop tier
 (qpdf; pdf.js 6.4.299 on node 24.22): 7 passed, `qpdf --check` clean
 on 84 Rewrite and 14 Append outputs, three Appends onto each of 14
 bases and a delete saved and undone on each of 14, pdf.js reading the
 records file both ways.
   - **The reviews:** the plan was reviewed before the steps started, and
 its findings are the deviations above, as resolved; the slice's
 changes were reviewed after the steps (above). Each step checked its
 new checks by switching their behavior off (below).

   Checked by tests:
   - **PageSpace text:** frames.rs's three; page_chars' unit tests
 (`each_rotate_with_its_content_reads_upright` over the four
 rotations, `text_running_down_the_page_reads_as_one_line`,
 `the_most_drawn_turn_reads_upright`, and the ported search tests
 `a_literal_spans_words_on_a_line`, `repeated_matches_follow_one_
 another`, `a_pattern_maps_its_bytes_to_characters`,
 `hits_on_turned_text_are_in_page_space`, `text_len_counts_what_s_
 drawn_but_whitespace`); ocr_layer.rs's `upright` against the layer's
 at every rotation, each PDFium word on the layer's box.
   - **Matching** (the review): page_chars' `a_literal_s_words_match_
 across_lines_and_spaces` (a phrase over a line break, a rect on each
 line, a double space and a no-break space) and "3 TOTAL" over the
 page's line onto the recognized words in `a_search_walks_the_
 document_s_order`.
   - **The service** (sheaf-ops tests/text.rs): `chars_come_from_each_
 page_s_realization` (an untouched page, an inserted `/Rotate 90` file
 with an offset CropBox reading one line up the page, a duplicate, a
 crop, and SetOcrText with an unsaved header; under `content_only` the
 header and OCR words absent and the copies equal to their originals
 within 1/16 pt), `a_refused_source_s_text_comes_from_its_groups`,
 `chars_are_cached_by_render_key`, `a_search_walks_the_document_s_
 order`, `realized_glyphs_lie_within_a_sixteenth_of_a_point_of_the_
 source_s`; the pool's `a_refused_query_reads_from_its_fallback`,
 `a_walk_reads_pages_of_many_engines_in_order_and_then_idles` (30
 positions over a base, a group and a refused engine, a panicking item
 and work, the exact stream, one open each, at most one CPU tick after
 `Done` on Linux: the idle CPU gate) and `a_dropped_receiver_ends_
 its_walk`; from the review, `a_walk_keeps_its_engine_past_pinned_
 ones` (six engines pinned, a walk over three groups opens each once)
 and `a_background_base_engine_grows_into_what_the_screen_leaves`.
   - **The app:** `find_and_patterns_search_through_the_pool` (an inserted
 page, a duplicate, a crop, an OCR page and a header: Find, a pattern,
 "draft" and "total 42" hitting in document order with PageSpace rects;
 `with_chars` on each), `a_file_pdfium_refuses_draws_its_pages` (Find,
 selection, the notice), `what_a_change_removes_leaves_the_selection`,
 `a_pick_reads_a_number_recognized_but_not_saved`,
 `a_page_changed_while_read_is_read_again`,
 `a_selection_outlives_a_change_to_how_its_page_draws` (the review: a
 page deleted under "Page <<1>> of <<N>>", the selection still copies
 with no read),
 `printing_previewing_and_exporting_write_no_temp_file` (a child
 process, `TMPDIR` empty and unwritable, a fake `lp` reading stdin:
 print, preview, PNG and TIFF export; `lp` got %PDF bytes PDFium
 opens).
   - **Plugins:** `plugin_text_reads_the_page_as_the_file_will_draw_it`
 (unsaved OCR and header from their groups, every page as the saved
 Rewrite's within 1/16 pt, a refused source through the fallback) and
 `plugin_text_leaves_field_values_out` (sheaf-ops; the page read with
 its widgets drawn reads the same, PDFium's premise);
 `text_comes_as_the_file_draws_it_in_the_page_s_frame` and the reads
 test (host.rs; a text read records no `PageAnnots` or mark, a mark on
 the page or not).
   - **Blank detection, OCR's check, printing:** `an_unsaved_header_
 leaves_a_blank_page_blank`, `unsaved_ocr_text_still_counts`,
 `remove_blank_pages_works_on_a_source_pdfium_refuses`,
 `ocr_skips_text_on_a_source_pdfium_refuses` (tests/blank.rs);
 `a_source_pdfium_refuses_prints` (tests/realize.rs);
 `lp_reads_the_job_from_stdin` (cups.rs, 1 MiB through a fake `lp`).
   - **Fault checks** (each step's, restored from its own copy and checked
 with `cmp`; no proptest seed added): step 1's 4 (the box from the top
 edge, w and h swapped, the angle unflipped, `from_engine` ignoring
 angles), step 2's 3 (queries ignoring their fallback, the cache keyed
 by `PageId`, angles dropped), step 3's (the walk ignoring
 fallbacks, Find on the primary only, and the walk reading each page
 from its source's base engine by index), step 4's 3 (`cached_chars`
 ignoring the key, selection from the base engine, no stale-answer
 check), step 5's 5 (reads under `content_only`, text under
 `content_only`, an OCR page without its text, no `to_frame`, the
 `text_boxes` rect not mapped back), step 6's 5 (text and pixels
 under `screen()`, no `ocr_over`, text and `text_len` from the base
 engine), step 7's 4 (a print temp file, a fake `lp` ignoring stdin in
 each test, export through a temp file): all caught, the walk's
 fallback once the test searched before the refusal was known (Found).
 The review's 5: the literal's gap a space alone (the unit test and
 the ops search), the walk holding nothing and the background's base
 engines held to 32 MB (the two pool tests), `selected_text` from the
 cache (the app test), plugin text under `plugin()` (host.rs's two):
 all caught. Step 8's: retired.rs's 51 rows each hit by a probe file
 defining it at its path, and the qualified-only names hit when
 qualified and not when bare (probe files in a directory cargo
 doesn't build, deleted).

   For Greg:
   - **The refused notice** now reads "PDFium can't read this file as
 saved, so its pages are drawn from sheaf's copy". Wording yours.
   - **An unsaved header is text:** Find, Patterns, selection and the
 calculator read what the screen draws, so a header or footer not yet
 saved is found and selectable, and an OCR'd, headed page selects as
 the page's text, then the recognized words, then the header.
   - **Plugin text conflicts with header edits:** a call's text reads
 record what of the page the file will draw as text
 (`plugin_text()`): the page, every generator and, under a numbering
 header, the order, so an unsaved header's edit conflicts with a call
 that read the page's text (§14.3's "as the file will draw it"); a
 comment's doesn't, since PDFium never reads an annotation as text
 (the step recorded `PageAnnots` and each `Annot` too; the review
 dropped them). docs/plugins.md says so. Slice 20's 0.4 reads may
 narrow the generators to those that can come onto the page.
   - **Find is about 1.2× slower a page** than `FPDFText_Find` (Measured):
 11.4 s over the synthetic 7k and 8.0 s over the heavy as opened, 12.2
 and 9.1 s with a header on every page, on the worker between renders.
 A faster read (one FFI call for a page's boxes, say) is a later perf
 pass's.
   - **Matching is `PageChars::find`'s** (a literal is its words joined
 by `\s+` in one regex, case-insensitive unless it matches case, so
 a phrase wrapped onto the next line is found, as `FPDFText_Find`
 found it), for Find and plugins alike.
   - **Blank detection counts unsaved OCR text** (deviations), where §10.4
 says `content_only()`; recognized text saying "intentionally left
 blank" still marks a page so.
   - **A selection keeps the characters it was made in:** after an edit
 that changes how its page draws (a comment hidden, a page deleted
 under a numbering header) it still paints, copies and marks up as
 it was; after one that changes the page's text (OCR recognized
 under it) it keeps what was selected until a new selection.
   - **The pool's budgets changed** (the review): the background's base
 engines grow into what the screen's engines leave of 96 MB rather
 than its own 32 MB, and a walk holds the engine it reads on past the
 screen's count (a seventh beside six pinned). Find Blank Pages opens
 its base engine 6 times over the synthetic 7k, not 19, and Find past
 six pinned groups 49 engines, not 6,232. Both rules yours to change.
   - **Real CUPS and GDI weren't run:** a fake `lp` shows the job on stdin
 and the trailing `-`; GDI only compiles (the Windows check and
 clippy). sheaf-app's and sheaf-plugin's Windows checks can't run here,
 as before.
   - **The Rewrite flag:** one of eight synthetic processes took 1.21×
 slice 8's writer (163 ms), past the 1.2× flag; the median is 1.11×
 and the slice didn't touch the writer.

   Left for later slices:
   - **Slice 15:** Trim White on a source PDFium refuses (`content_only`
 over the whole MediaBox through the views, off `engine_for`); crops
 read from their group, not a derived copy's base.
   - **Slice 19:** an `ExportTx` for print and text export; the
 `std::env::temp_dir` ban (the print temp file is gone, so the
 recovery and generated-cache users are what's left).
   - **M10 slice 14:** `TextOpts.fields`, fields' display text.
   - **Slice 20:** WIT 0.4's to-view and from-view matrices, so plugins'
 text needn't go through 0.3's frame.
   - **A later perf pass:** Find's per-page read (above), beside the
 realize gate's.
   - **Follow-ups:** the `Closure::stale` probe (slice 11's Unprobed),
 untouched: no step changed that walk.
