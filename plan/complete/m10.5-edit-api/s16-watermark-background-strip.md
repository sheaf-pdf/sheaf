# M10.5 slice 16: Watermark, background and strip

Status: done · Part of [M10.5: the edit API](README.md)

**Watermark, background and strip** (L; proposals 30, 31 and 39;
bugs 16–18 and 28).
- **The watermark/background `Generator`:**
  - `needs()`;
  - upright layout at emit;
  - shared resources once per file;
  - one block per distinct layout;
  - `ImageRef` and `PageForm`.
- **Markers:**
  - `/SheafGenerators`;
  - string `/SheafBlock` markers;
  - named property lists;
  - marked isolation streams.
- **Content assembly:** `Balance` isolation, and `PageRes` with name
  pruning.
- **Ops:** `replaces`, and `RemoveWatermark` by origin, with
  `include_authored`.
- **Copy-on-write splices.**
- **Dialog prefill** from the model.
- **Recognition after reopen,** including image rebinding.
- **Unbacked blocks** are kept.
- **Covered-but-empty markers.**
- **The PDF version** is accumulated.
- **A PDF/A or PDF/UA claim** gives a Note.
- **Deletes:**
  - derive::watermark and `current_watermark`;
  - `watermark::install` and `read`, and the per-page ExtGState,
    font and image;
  - `SheafSettings` and its local image path, now read only
    (`StripLegacy` strips it);
  - `/SheafWM*`;
  - `stamps::strip`'s watermark and background kinds (its call in
    derive::watermark); Remove Header/Footer keeps `stamps::strip`
    until slice 17;
  - `SetPageBase` for watermarks.
- **Checks:**
  - **Resources:**
    - an image watermark on 7,000 pages adds one image object;
    - the object census shows one font, one ExtGState and one
      Resources dict per distinct base Resources.
  - **Text:**
    - "Café" renders;
    - CJK text draws `'?'`, with a notice.
  - **Removing others':**
    - Acrobat's reference watermark is removed, its Form XObject
      unlinked, and its text absent from the decoded bytes after a
      Rewrite;
    - a Word-made PDF's running headers survive a sheaf watermark,
      and `RemoveWatermark` reports them as authored and leaves them
      unless `include_authored`.
  - **Ours after reopen:** recognized, updatable and removable,
    including an image watermark (which rebinds), with `Scope::All`
    reaching a page inserted after reopen. A PDFium
    `FPDF_SaveAsCopy` re-save keeps our blocks recognized (ignored
    test).
  - **Saves:**
    - a rotate keeps the watermark upright;
    - adding a watermark to a saved file appends, and removing it
      gives a Rewrite with no trace left.
  - **Content safety:**
    - a stream shared by blank pages stays intact;
    - a front watermark over the bases `q …`, `Q q` and `Q` leaks no
      graphics state (a pixel check);
    - five save/reopen/watermark cycles keep the q depth at 2 or
      less;
    - `forms::check` is clean on watermarked form pages.
  - **Versions and claims:**
    - a PDF 1.3 base is written as 1.4 (Rewrite), or gets a Catalog
      `/Version` (Append);
    - a PDF/A-2b file gets the Note.
  - **Clean Copy** of a file with a legacy watermark has no
    `SheafSettings`.
  - **Thumbnails** fill at the gate rate after watermarking all
    7,000 pages.

## Notes

Done. Watermark, background and strip (proposals 30, 31 and 39; bugs
   16–18 and 28): a watermark or background is a generator, set and taken
   off by ops, laid out upright on each page's own boxes when a save
   writes the page or a view draws it, written as marked blocks around
   the page's isolated content, and found again when the file opens. It
   replaces what Acrobat, an older sheaf or another file drew of its kind
   and leaves the author's, and Remove Watermark says what it found by
   who drew it. A Clean Copy takes an older sheaf's settings out of what
   it drew. The derived copy, `watermark::install` and `read`, the
   per-page resources, the `/SheafWM*` names and `SheafSettings` writing
   are gone. A text watermark and an image background on 7,000 pages add
   one image, one font and one graphics state to the file and a
   resources dictionary for each distinct one the pages had (asserted on
   every 7k fixture); the strip fills at 413 and 344 pages a second with
   a watermark on every page (asserted ≥ 310), and the corpus counts
   hold.
   - **The ops and records** (step 1, sheaf-edit, sheaf-image,
 sheaf-doc). sheaf-edit: `FontChoice` beside `Color` in mark.rs (the
 12 Std14 text faces, kebab-case, default helvetica-bold); the new
 watermark.rs's `Z::{Under, Over}`, `GridPos`'s nine cells (default
 center), `WatermarkSpec { content, position }` and
 `WatermarkContent::{Text { text, font, size, color, opacity, angle },
 Image { image, scale, opacity }, Page { pdf, page, scale, opacity }}`
 (tagged by `type`, `deny_unknown_fields`); `Op::SetWatermark { spec,
 z, scope }` and `Op::RemoveWatermark { z: Option<Z>, pages,
 include_authored }` after `ClearOcrText`; four `every_op` fixtures;
 `schema/ops-1.0.json` regenerated (insertions, and step 4's two
 relaxations); fuzz/op.dict's names; tests/ops.rs's newer-version op
 is `flatten-annotations`. sheaf-image: `io::sniff(bytes)`, the six
 types `read` takes, which `read_bytes` and `is_image_path` share.
 sheaf-doc: `BlobKind::Image(ImageFormat)` and `BlobKind::image`;
 `content::blocks::GenId` the one `GenId` (with `draw`), re-exported
 from `model`; `Scope { All, Pages(im::OrdSet<PageId>) }`,
 `ImageRef`, `PageForm`, `WatermarkGen { content, position, z,
 scope, replaces }` (`kind_at(z)`), `GenSpec::Watermark` and
 `GenSpec::watermark()`; `Generator::{covers, z, scope, replaces}`
 (`needs()` empty for a watermark, `on_pages_left` narrowing a
 `Pages` scope); `DocState::watermark(z)`; `check_invariants`' one
 watermark a z, scope pages that exist, no empty `Pages`;
 `resolves_after` with scope pages. tx: `ResolvedOp::SetWatermark`
 and `RemoveWatermark`; resolving an image blob, a PDF blob's page
 (0 Invalid, past the end NotFound) or Invalid for a stamp grant;
 caps (`Approval`); tx/content.rs's `set_watermark` (the values
 checked, the replaced generator's id kept, `replaces` by z,
 `Notice::Unencodable` with the text kept) and `remove_watermark`
 (narrowing, `All` → `Pages(order − pages)` reading `Order`); the
 DuplicatePages copy joining a `Pages` scope (pages.rs `join_scope`);
 effects (added and layout-only `CONTENT`, the rest `REMOVES`, and
 `REPLACES_TEXT` for text, as `RemovedKind::{Watermark, Background}
 { text }`); describe's lines and runs; prim.rs's heap sizes (an
 `im::OrdSet` impl); realize/key.rs's `Feed` of `WatermarkGen` (not
 its scope) and `Drawn::of`; `fx::{watermark, set_watermark,
 remove_watermark}`; the replay property's set-watermark and
 remove-watermark; fuzz.rs's `applying()`.
   - **Drawn on save** (step 2). The new `generate.rs`: `layout(id, wm,
 geom, art, reserved) -> Drawn { ops, res, layout_digest }`, `Art::
 {Image, Form}`, `recipe_digest` (the render key's Feed,
 `realize::watermark_digest`) and `catalog_json`, the placement
 ported from `watermark::install` (36 pt edge, the rotated bounding
 box, `geom.from_view()`, the cells, a JPEG's quarter turns), drawn
 through `ContentWriter` (`named(&self)` and `Reserved::take` added;
 `GenId` Displays as `g<hex>`). materialize/key.rs: `ObjKey::
 Generators` (of the state, a root), `ObjKey::PageRes { base: u128,
 dropped: u64, added: u64 }`, `Shared::QOpen(u32)` and
 `QClose(u32)` for the unused unit `QOpen`, `bytes()` tags 17, 18
 and 13·0/1·n, `size_of::<ObjKey>() == 48` asserted; `ObjMap::opened`
 numbers the catalog's `/SheafGenerators`. emit/content.rs:
 `generators_on` (early out with no generators), the assembly
 `[under…, QOpen(1 + d), base′…, QClose(1 + d + net), over…]` with
 the memoized balance (`fresh_balance` once an old OCR layer is taken
 out), blocks under `ObjKey::Block` (the uncompressed stream's
 `ResId`) marked `/SheafBlock << /K /Gen /G (g…) /L <…> /R <…> >>`,
 isolation `/SheafBlock << /K /Isolate >>`, empty blocks where the art
 can't be had, `PageRes` from the base Resources through `sink::bind`
 (`Cx.page_res`, placeholders for the legacy installers, pushed only
 when still named), one Note for pages whose blocks fact errors,
 `/Thumb` dropped; `needs_after` counting a watermark under 1.4
 (Catalog `/Version`, plan.rs `OTHER`); `Cx.arts` (an image decoded
 once a blob, a page made a form once a page; `blob_form` takes a page
 index). The catalog's `/SheafGenerators`, an indirect literal string
 (`emit::appended` pushes it in scope or when the catalog is written
 unnumbered). deps.rs: `Generator(g)` of a watermark writes
 `Generators`, the catalog when the first comes or the last goes, and
 the pages covered then or now (a scope-only change, those that
 changed); the header keeps its path. removal.rs: `PageRes` silent,
 a block judged by `Drawn::on` (each covering watermark's text or
 art hash), the old `Generators` object on every watermark's recipe
 text saved against target. `append_tests`' helpers `pub(super)`.
   - **Drawn on screen, the PDF/A Note, gates** (step 3). `page` draws
 generators when `view.is_none_or(|v| v.generators)` (pages.rs'
 `/Thumb` too); `generators()` takes out an old OCR layer only when
 the new text is drawn; untouched.rs's `generated()`; `content::Arts`
 (at most 4, by `(BlobId, Option<page>)`), reaching emissions through
 `Overlay.arts`. plan.rs: `told(input, path, transforms, asks)` and
 `claim_broken`'s Note (a Rewrite for every generator it draws, an
 Append for those its path changes), `joined`; `PdfA` Displays.
 gate_7k: the strip's `SetWatermark(text, All)` between the crop and
 the header, "watermark all pages, commit", a text watermark and a
 600×400 PNG background's Rewrite, file and Append
 (`planned_and_put_back`), and the census, asserted.
   - **Found again after reopen** (step 4). `Source::marked()` (the
 catalog's `/SheafGenerators`, or a Derived blob), `Source::
 markers(i)` (each `/Contents` stream's dictionary, cached a stream),
 `Source::generators()` (`generate::read` → `Saved`, each entry alone,
 bad ones logged and passed over; `Saved::generator(src, page_of)`);
 `Facts.generators` → `Document::opened` → `DocState.
 opened_generators`, `DocState::backs(g)`, the header's id drawn clear
 of them. base′ in emit/content.rs: `content::intact` (the emitter's
 no-change path, `untouched` and the key's `base_draws`), `rebuilt()`
 (one decompress and lex, `content::kept`): backed blocks set aside,
 isolation set aside unless an unbacked block is kept, `DropStreams`
 and `DropMarked` filters spliced into `ObjKey::Filtered { stream:
 u128, filter: u64 }` copies, base blocks of equal `/L` and isolation
 of equal counts reused, dropped names pruned from `PageRes`
 (`Reserved::forget`; a name re-added to an identical object keeps the
 file's binding, `Cx::read` crate-visible). `ContentStack.filters:
 Vector<Filter>` (`DropMarked(MarkedSel::SheafGen)`,
 `DropStreams(StreamSel::SheafGen)`) in the digest, `Feed`, heap size
 and `with_filter`; effects' `RemovedKind::Filtered`.
 `RemoveWatermark` forcing each page's blocks (a read of `Page(p)`)
 and adding `DropStreams` and `DropMarked` (`PageBlocks::kind_of`).
 The removal check's arm for a reopened block. The fuzz target
 `sheaf_generators` (body, target, Cargo entry, nightly row, seeds,
 pdf.dict) and `generator_fixtures`; the Append ≡ Rewrite property's
 `marked` dimension and watermarks in `Reopened`;
 `PdfiumEngine::save_copy`; gate_7k's "ready, found again".
   - **Others' watermarks** (step 5). `MarkedSel::Pagination { kinds,
 origins }`; `PagOrigin::REPLACED`; `PageBlocks::selected` and
 `Pagination::selected_by`, the one rule; `Pagination::generator`;
 `content::splice_each`. emit/content.rs's `replacing()` (only where
 the blocks hold such a range), cuts by `Pagination::cuts` into
 `Filtered` copies, another file's block of a replaced kind left out
 whole, names only the cuts used pruned, refused ranges counted in one
 Note. effects' replaces row (`judge` reads the blocks). Remove's
 Pagination filter by origin and `tx::OthersFound`, in
 `ResolvedOp::RemoveWatermark.others` and its line; `lower_op` takes
 `&mut ResolvedOp`, the line described after lowering.
 content/corpus.rs's strip loop on `PageBlocks::selected`.
 append_corpus's ignored `watermark_round_trips`
 (`SHEAF_WATERMARK_ONLY`).
   - **An older sheaf's settings** (step 6). `Filter::StripLegacy(
 LegacySel::Marked(MarkedSel))`, `LEGACY_KEYS`; `lex::
 without_entries` and `content::without_keys` (`SpliceError::List`);
 `Spliced { taken, cuts }` with `takes` and `streams(content,
 rewrites)`; `rebuilt`'s `older_ranges`, `Cx.settings_kept` and its
 Note, `Stayed { ranges, settings }`; save.rs's `without_markup`
 adding the filter; effects' `RemovedKind::OlderSettings`. The content
 corpus test's rewrite pass, the fuzz splice target's rewrite check
 and a lopdf-style legacy fixture.
   - **The app on ops** (step 7). watermark_dialog.rs rewritten: the op's
 fields, the family and style rows, `Art::{Image, Pdf, Kept}`,
 `read_art`, `Start::at(state, z)`, `WatermarkEvent::{Apply(Box<Op>,
 Option<Blob>), Remove { include_authored }, Dismiss}`, the preview on
 `Preview<Op>` and `Previewer::page_in` (`draw_preview`, `follow`,
 `relower`, `release`); `crop_dialog::why` → `dialog_preview::why`.
 workspace.rs's `toggle_watermark(z)`, `watch()` and
 `close_dialogs_of_another`. page_insert.rs's `set_watermark` and the
 planner `remove_watermark`, `watermark_notice`. Docs of
 page_insert.rs, jobs.rs, docview.rs's planners and the command
 descriptions.
   - **Gates, interop, ledgers, docs** (step 8). gate_7k run whole; the
 interop pair `a_watermark_s_blocks_read_in_pdfjs(_appended)`;
 realize_corpus's edit draws a watermark beside the header on its
 three pages; retired.rs's 18 rows and 8 qualified-only names, each
 with its reason; the ledgers (above); docs/document-model.md's
 "Watermarks and backgrounds as generators (M10.5 slice 16)", the op
 listing and caps, and its forward references (the records'
 `Generator` and `DocState`, bytes in memory, the toolkit's last
 lopdf users, `PagOrigin::Sheaf`, the header strip, Unencodable,
 the page base's line, callers, planners and dialogs, the writers
 lopdf keeps, emission, the consumers' table, the overlay, slice 15's
 derived flows, OCR flushes); README's Watermarks / backgrounds row.
   - **Deleted,** each confirmed dead by `cargo check --workspace
 --all-targets` and clippy, the public names listed in retired.rs:
 sheaf-ops'
 `derive::watermark`, `current_watermark` and `has_kinds` (the
 header-only `has_header` now); all of sheaf-doc's watermark.rs
 (`Watermark`, `Mark`, `Anchor`, `FONT`, `install`, `read`,
 `extent`, the per-page ExtGState, font and image and the `/SheafWM*`
 names); `Derive::Watermark` and `RemoveWatermark`, `set_base`'s
 Unencodable arm and their describe arms; `save::HeaderKept`,
 `derived_copy`'s `header` and `&Geom` parameters and the per-page
 box read that fed them; `stamps::WATERMARK` and `BACKGROUND`
 (`strip` and `stripped` the header and footer's alone); the app's
 `DocView::apply_watermark`, `Workspace::open_watermark` and
 `WatermarkContext::{behind, current}`. Nothing writes
 `SheafSettings` or a local image path; the classifier still reads
 them (`PagOrigin::SheafLegacy`), and Clean Copy strips them.

   Deviations from the plan (the planner's list first, as resolved):
   - **The watermark carries its own z, `Scope` and `replaces`**
 (`GenSpec::Watermark(Arc<WatermarkGen>)`), and `Generator`'s
 accessors read them as §3.5's fields would be: hoisting them now
 meant rewriting about 40 header sites slice 17 rewrites anyway.
 `scope()` is a `Cow<Scope>` (the header's made from
 `HeaderFooter.pages`), `z()` is `Over` for the header, and
 `replaces()` none for it, its derived copies still stripping. A
 transitional ledger row (16 → 17) says so.
   - **An op names its file by `BlobSel`, optionally.** An op can't hold
 a `BlobRef`, so `Image { image }` and `Page { pdf, page }` take a
 `BlobSel`, resolved to `ImageRef::Blob` / `PageForm::Blob` as
 `NewMark.asset` is; a wrong kind is Invalid, a page past the end
 NotFound. Since step 4 both are `Option<BlobSel>` (`page` defaulting
 to 1): none keeps what the watermark at that z draws now, the only
 way to move a reopened image without embedding it again; nothing
 there is Invalid ("no image is drawn at its z to keep"). The schema
 changed by relaxations only.
   - **`FontChoice`, `GridPos` and `Z`,** named but never defined, are
 sheaf-edit's: `Z` and `GridPos` in watermark.rs, `FontChoice`
 beside `Color` (slice 17's `HeaderFooterSpec` takes it too), all
 `deny_unknown_fields`. Defaults: center, helvetica-bold, angle 0;
 an image's or page's scale is of the page's width (the legacy rule),
 in (0, 2].
   - **One `GenId`,** `content::blocks`', re-exported from `model`, so
 both paths stay valid and retired.rs has no row for it.
   - **1.4, not 1.7** (for Greg): §11.7 says artifact subtypes need 1.7,
 the slice's Check and `PdfVersion::ARTIFACTS` 1.4; the code follows
 the Check. ISO 32000-1 Table 330 marks `/Subtype` as PDF 1.7.
   - **`replaces` and Remove take Acrobat's, an older sheaf's and another
 file's** ({Acrobat, SheafLegacy, Sheaf}, `PagOrigin::REPLACED`), not
 §11.4's Acrobat alone: the old derived flow stripped every
 non-authored artifact of its kind, and an inserted sheaf file's
 unbacked watermark would draw beside ours. Backed blocks are set
 aside anyway (`Pagination::generator` keeps them out of counts).
   - **A page whose content can't be read gets no watermark** (for
 Greg): §3.4 fails the op that needs isolation, but SetWatermark
 lowers in O(1) and `All` covers later pages, so the page is written
 and realized without it, with one save Note naming the pages, and
 emit stays total. A `replaces` cut the splice refuses stays, with a
 Note too.
   - **The backed rule:** a block is backed when its `/G` is a
 generator of the state that covers its page, or, on a page of the
 opened primary (or a derived copy, until slice 17), a generator of
 the state or one recognized from the primary (`DocState::backs(g,
 page)`; the page-aware half is the review's). Backed blocks are
 always set aside, so a generator gone by Remove, a cascade, an
 extract, a Clean Copy or a print leaves none; unbacked ones
 (another file's, inserted, an earlier save of the same file's among
 them, since a `GenId` lasts across saves) are kept until a filter
 drops them, and isolation stays on pages that keep one.
   - **Recognition reads more than stream markers** (the review): a
 page with no block in a stream of its own, whose resources name a
 list with `/SheafGen`, has its ranges read (`Source::blocks`) for a
 block another tool merged into a stream, so a `pages` scope keeps
 that page and an Update re-lays it (§3.5); only such pages are
 read. An `all` generator that a page whose content can be read
 holds no block of (a page another tool added) is recognized as
 `Pages` of the pages that hold one, with a log line, so the screen,
 an Append and a Rewrite agree; a page that can't be read keeps
 `All`, as sheaf never drew on it. An image or page still binds
 only from a block in a stream of its own.
   - **SetWatermark keeps the replaced generator's `GenId`,** as
 `set_header_footer` does, so recognized blocks stay backed and an
 equal `/L` is used again.
   - **Effects by recipe:** a change of text, image or page, or a
 removal, is `REMOVES` (`REPLACES_TEXT` for text); a layout-only
 change is `CONTENT` and may append. Beyond §7.1, a watermark that
 leaves a page still in the document is `REMOVES` too, as Remove's
 narrowing is; `All` → `Pages` counts as narrowed when a page of the
 saved order is off the new scope; a page that left by being deleted
 isn't counted; a z change is layout. `RemovedKind` gained
 `Watermark { text }` and `Background { text }`, a text one reading
 as the header does ("1 watermark changed").
   - **StripLegacy came early** (for Greg): Clean Copy is slice 19's
 RemovePrivateData in the design, but the Check needs a Clean Copy
 without `SheafSettings`. `Filter::StripLegacy(LegacySel::Marked)`
 rewrites the inline lists of an older sheaf's watermark and
 background ranges without `LEGACY_KEYS`, lexically (comments,
 spacing and nested dicts kept byte for byte) and safe or refused (the
 result must read back as the list less the keys); a named list, one
 across streams, a non-name key or non-object value, or no `BDC` is
 refused and kept, with a Note. `LegacySel::Stream`, named lists'
 `/Properties` and renaming `/SheafWM*` are slice 19's. Print and
 page images without markup strip them too (`without_markup`, §16
 row 27). Adding the filter is `CONTENT | REMOVES` as
 `RemovedKind::OlderSettings`, so an Append can't leave the path in
 an earlier revision. A clean copy's former legacy range reads as
 `Authored` afterwards.
   - **`HeaderKept` and `derived_copy`'s `header` went,** their one live
 caller `derive::watermark`; the header-keeping copy tests ported to
 plain copies, and materialize/tests.rs's derived-copy case of
 `the_header_is_drawn_once_and_not_again_where_the_base_draws_it`
 (with `copied`) dropped, since no flow makes such a copy. `fix` lost
 its `&Geom` too.
   - **`stamps::strip` and `stripped` are header/footer-only;**
 strip_artifacts.rs, rewrite_tests.rs and content/corpus.rs ported,
 the last onto `PageBlocks::selected`, which `MarkedSel::Pagination`
 matches through.
   - **Keys** as their writers came: `ObjKey::Generators` (a root, as
 Labels and AcroForm are), `PageRes { base: u128, dropped: u64,
 added: u64 }` (the plan's `Option<ObjRef>` isn't `Copy`; three
 u128s made `ObjKey` 64 bytes and the synthetic Rewrite 1.16–1.18×,
 so the size is asserted at 48), `Filtered { stream: u128, filter:
 u64 }`, `Block` by the uncompressed stream's `ResId` (compressed once
 a file; legacy `Sink::add` blocks keep the compressed digest), and
 `Shared::QOpen(n)`/`QClose(n)` in place of an unused unit `QOpen`:
 the legacy installers' unmarked `q` is an `ObjKey::Res` by digest,
 not `Shared::QOpen`, as the plan said.
   - **`BlobKind::Image(ImageFormat)`,** liveness by `BlobRef`.
   - **PageForm through a PDF:** the chooser takes a PDF and draws its
 page 1 (`Art::Pdf`); `model::PageForm` and stamp_lib's are
 qualified apart.
   - **`a_derived_flow_keeps_the_crop`'s watermark half:** a watermark
 planned before a later crop now lands (it reads no `Geom`), keeps
 the crop and is centred on it; a further crop and save re-centre it.
 The `strip_stamps` half stays until 17. The two docview tests use
 `Derive::StripHeaderFooter` (label "Strip"), any `Page(p)` write
 serving the blank-review conflict.
   - **Remove is a planner** (`run_planner`, `Actor::User`, `Validate` on
 the dialog's version), not a job; its per-origin counts and
 unreadable pages are computed at lowering into
 `ResolvedOp::RemoveWatermark.others` (`tx::OthersFound { acrobat,
 other_files, older_sheaf, authored, authored_left, refused,
 in_forms, unreadable }`; `older_sheaf`, since retired.rs flags
 `legacy`), and read by its line and the app's notice.
   - **Layout from `Geom`** with the generator; `PageCtx` comes with
 slice 17. The recipe digest is spec plus z.
   - Step-level choices, each in its step's report:
 - Step 1: an empty `Pages` scope is Invalid ("the watermark is on no
   page") and an invariant; text is trimmed before the empty check;
   `remove_watermark` reads `Order` when it narrows `All`
   (`remove_header_footer` has the same hole, slice 17's); the replay
   property redirects remove-header-footer when there's no header;
   `read_bytes` refuses a type it doesn't read before decoding.
 - Step 2: the property list is named `/Sh<base32>` like every added
   resource (`ContentWriter::fresh`), not `/ShP<n>`; `/L` and `/R`
   are 16-byte hex strings, `/G` a literal `g<hex>`; the JSON escapes
   all non-ASCII; added names clear of every name the base binds (a
   superset, pruning narrowing it from step 4); `Cx.arts` keyed by
   `(BlobId, Option<index>)`, not `Cx.stamps`; one Note for all
   unreadable pages (`describe::pages_text`, crate-visible);
   `SaveRecord.drawn` stays empty until slice 17.
 - Step 3: the PDF/A Note counts the legacy header too, a Rewrite
   every generator, an Append those its path changes; an overlay
   drawing generators but not OCR keeps the file's OCR layer; a
   covered page whose content can't be read isn't untouched; the
   census uses an Over text and an Under image at one opacity, so one
   ExtGState is checked across both; `source::tests` and
   `materialize::watermark_tests` crate-visible for their helpers.
 - Step 4: `Source::generators()` returns `&Arc<[Saved]>` and never
   fails (a source makes neither `ObjRef`s of itself nor `PageId`s),
   `Saved::generator` making the record at ready; `opened_generators`
   in the state, read through `backs`, and the key feeding the backed
   ids a base holds; `Placed { at, res, turns }`, the JSON `{"placed":
   "<ResId>", "turns": n}` (`v` stays 1; without turns a Placed JPEG
   drew sideways, without the id an unchanged image never laid out
   alike); covered-but-empty checked with a hand-emptied block;
   `intact` the shared rule, the realizer never decoding a store image
   for a digest; content_only views set backed blocks aside too;
   Remove's `DropMarked(SheafGen)` for merged ranges either way,
   `DropStreams` for unbacked streams; filters compared only while
   the base is the same; markers read for Derived blobs too.
 - Step 5: the cut goes with drawing the generators, so not under
   content_only (OCR can read a replaced Acrobat watermark's text);
   the classify row reads the blocks in `judge` only, so a step's
   History badge doesn't flag a replacement, the save plan does;
   another file's block of a replaced kind is left out whole; step
   4's refused `DropMarked` ranges count toward the Note; `in_forms`
   may over-report (the fact isn't per kind); Remove's origins only
   where a splice takes a range; the `foreign` recognized test uses a
   background, and a `replaced` test covers no doubling; the ignored
   tier allows StaleSignature, StaleSigFlags, XfaMismatch and
   EarlierValues, which these saves are told to cause.
 - Step 6: `LegacySel` has only `Marked`; the rewrite is lexical with
   a read-back check; `Spliced` changed shape (`taken`, `takes`,
   `streams`); `LEGACY_KEYS` in `model::records`; `StripLegacy` with
   `MarkedSel::SheafGen` handled so the type is total; Clean Copy
   reads every page's blocks (off the UI thread, ungated).
 - Step 7: `Apply(Box<Op>, Option<Blob>)` carries the chosen file so
   the transaction pins it; Remove is always offered, removes from
   every page, and says "No watermark to remove" when nothing was;
   a family row and a style row for the faces; rotation for text
   only; the menu item toggles the dialog; notices are the commit's
   line and notices.
 - Step 8: the interop pair starts from a page with Acrobat's
   watermark and writes four files a way (as is, set, moved after a
   reopen, removed); realize_corpus's edit draws a watermark too, so
   its tiers cover the realizer's watermark on real files.

   Found while building:
   - **A 64-byte `ObjKey`** (step 2): `PageRes` with three u128s grew
 every key and pushed the synthetic Rewrite to 1.16–1.18× of slice
 8's; two u64 name digests keep it 48 bytes, asserted.
   - **The old `/SheafGenerators` vetoed layout changes** (step 2): its
 JSON's words read as text gone; it's judged on record inputs now.
   - **Every delta decoded the image** (step 3): `Cx.arts` lasts one
 emission, so a group's 128 deltas decoded an image background 128
 times; `content::Arts` keeps them a realizer.
   - **A reopened JPEG drew sideways** (step 4), and an unchanged image
 watermark never laid out alike: `Placed` holds the turns and the
 `ResId` it was drawn as.
   - **The corpus checkout was restructured** (2026-10-21, outside this
 slice): `../pdf-corpus/manifest.tsv` is `forms/manifest.tsv` now and
 `index/safedocs-inventory.tsv.gz` is `safedocs-forms-inventory.tsv.gz`,
 so `Corpus::find` skips every corpus test and realize_corpus can't
 read its inventory. Every corpus run here went through a scratch
 root of symlinks (`manifest.tsv`, `forms`, `pool`, `index` with the
 inventory under its old name).
   - **A list split across streams** (step 6): one Word list in the corpus
 (0837070 p1) crosses two streams and can't be rewritten; 7,026 of
 7,027 pagination lists rewrite and read back, and 2 named lists are
 refused.
   - **A moved watermark beside an unchanged background takes a second
 font and ExtGState** (step 8, the interop test): after a reopen the
 background's block keeps the file's names, so the moved watermark's
 resources take new names bound to new, identical objects: one more
 font and graphics state a file, not a page, and bounded (its names
 are pruned and reused at the next move). The census, on a first
 set, finds one; binding a need to a base name already bound to an
 identical object would make it one again (a later perf pass).
   - **origin/m10.5-p5's `cd6ed01`** isn't in this worktree: without it
 the non-Linux clippy simulation fails on docview's `use
 sheaf_ops::crop`, as on origin before that fix; applied for the
 simulation only.

   Measured (AMD EPYC 7302P, 32 threads, release builds unless marked):
   - **The gates** (`gate_7k`, release, each test in its own run at a
 load of 2.0 to 2.9 (another session's corpus fetch on one core); the
 Rewrite in five more processes of its own):

 | Row | Synthetic | Heavy | Scanned |
 |---|---|---|---|
 | thumbnail strip, crop + watermark + header, 320 px (asserted ≥ 310 pages/s, ≤ 96 MB weighed and added) | 413 pages/s, 80.7 MB, 26.2 MB (slice 15, no watermark: 446) | 344 pages/s, 80.7 MB, 63.0 MB (372) | 162 pages/s, 80.7 MB, 77.6 MB (167) |
 | realize + open + render 1,024 px, an edited page (asserted < 15 and < 25 ms) | 12.1 ms (2.3× the warm 5.25 ms) | 22.2 ms (4.4× the warm 5.06 ms) | 32.7 ms (1.5×) |
 | census of a text watermark and a PNG background on every page (asserted): images, fonts, graphics states, forms added; page resources; property lists | 1, 1, 1, 0; 1 of 1; 2 | 1, 1, 1, 0; 70 of 70; 2 | 1, 1, 1, 0; 7,000 of 7,000; 2 |
 | that Rewrite, its file, the Append onto the file as opened (new; no gate) | 343 ms, 3.97 MB, 483 ms | 890 ms, 18.5 MB, 681 ms | 1,028 ms, 475 MB, 691 ms |
 | that file opened again, ready with both found (new; no gate; ready as opened) | 92.7 ms (66.8 ms) | 496 ms (366 ms) | 536 ms (462 ms) |
 | watermark all pages, the commit (new; no gate) | 0.014 ms | 0.014 ms | 0.015 ms |
 | crop all pages, and validated after a delete (asserted < 200 ms) | 12.7, 15.8 ms | 11.8, 15.0 ms | 9.66, 15.4 ms |
 | engines after 220 groups: weighed, RSS added, open (asserted ≤ 96 MB, ≤ 96 MB, ≤ 6) | 28.8 MB, 37.5 MB, 6 | 28.8 MB, 62.2 MB, 6 | 28.8 MB, 51.1 MB, 6 |
 | one-mark Append on the scanned (asserted < 50 ms) | | | 26.0 ms |
 | snapshot, `Tx::check` of a delete of every page (asserted < 50 µs, < 1 ms) | 0.241 µs, 0.397 ms | 0.271 µs, 0.395 ms | 0.241 µs, 0.404 ms |

 The strip's watermark costs it about 0.2 ms a page on the heavy (a
 block, its layout and digest and a `PageRes` each page's delta, and
 a transparent draw). The rest as slice 15 left them: the geometry
 walk 48.2 and 74.6 ms (< 100 and < 250 ms); ready on the heavy 366
 ms (< 1 s); 200 OCR versions of 1,000 pages at most 236 MB (≤ 256,
 12 versions); AddMark 0.020 ms and a full reorder 0.995 ms at 20,000
 marks; 1,000 mark adds on the heavy 0.033 ms (blank 0.022 ms); ready
 on 20,000 sheaf marks 534 ms (< 1 s); Bates apply 153, 210 and 79.0
 ms (slice 17's < 50 ms, unasserted). Trim White and the text walks
 (information only) weren't run again.
   - **The Rewrite against slice 8's writer** (the harness's run and
 `rewrites_on_the_synthetic_and_heavy_7k` five more times, each in a
 process of its own, against slice 8's 134 and 707 ms): synthetic
 146–162 ms, the five's median 154 ms, 1.15× (1.09–1.21×; one run
 of the five past the 1.2× flag, the median not), RSS added
 50.3–51.3 MB (0.82–0.83×); heavy 777–815 ms, median 806 ms, 1.14×
 (1.10–1.15×), RSS added 252–255 MB (1.00–1.02×). Slice 15 had 150 and 764 ms
 (1.12× and 1.08×; the heavy 714–831). A save of a document with no
 generator takes the same calls as before (`generators_on`'s early
 out; markers read only for a marked source); the step-5 run measured
 1.14× and 1.09×. Below the flag; not chased.
   - **The corpus** (through the scratch root, above), every count as
 slice 15 left them: materialize_corpus's identity 4,389 of 4,392
 written, 2,269,722 primary objects surviving, 919,479 copied as they
 were, 436 changed by `keep_only`, 0 failed; 2,901 forms without their
 first page, 0 failed; every page duplicated, 4,389, 0 failed;
 inserted twice into the 1040, 1,098, 0 failed; four times, 0 differ;
 the logo, 0 of 2,037,600 bytes differ. append_corpus: 4,389 marked,
 3,770 appended, 259 rewritten (211 RemovalCheck, 34
 BaseObjectsMissing, 14 BaseUnsound), 360 asked, 0 failed, 0 derived
 vetoes and 211 on the files' own objects; the oracle finds text gone
 from 2,899 of 2,901 (none from 0016064 and 0112654).
 encrypted_corpus: the Rewrite keeps 573 of 573 and the Append 572
 (0630899 declined). realize_corpus, its edit with the watermark: the
 predicate tier over every file it picks (4,935: 4,392 corpus forms,
 402 pool sample, 50 encrypted, 50 leading junk, 50 startxref
 nowhere; 3 don't open) disagrees on 0 of 242,055 pages and inputs
 (15 s); the pixel tier on eight files by `SHEAF_REALIZE_ONLY`
 (4422502, 0043199 and 0112530 with Acrobat watermarks, 0128827
 Word's, the 1040, 0106273, 0171994, 0630899): 3 of 28 pages differ,
 all within the anti-aliasing allowance (0106273 page 0 by 1 px,
 0171994 pages 12 and 23 by 3 and 4), 4422502 refused by PDFium and
 realized standalone; the whole pixel tier ran past ten minutes here
 and is the pre-merge run's. The watermark tier by
 `SHEAF_WATERMARK_ONLY` on five files (0043199, 0112530, 0128827, the
 1040, 0630899): 5 of 5 watermarked both ways, found again and
 removed, Acrobat's on their pages 4 the two ways together, 2 left
 (0112530's, a splice refusing them), 0 failed, one Append a Rewrite
 for BaseUnsound. The full ignored tiers (deleted means deleted,
 realize_corpus's pixels, the crop and watermark tiers) wait for the
 pre-merge run (Greg, 2026-10-20).
   - **Suites:** `cargo test --workspace` 1,478 passed and 42 ignored
 (slice 15: 1,379 and 38; ignored +4: the SaveAsCopy test, the
 watermark corpus tier and the interop pair). fmt and clippy (`-D
 warnings`, all targets) are clean, and so is clippy with
 sheaf-app's, the pool's and gate_7k's Linux-only gates switched to
 `cfg(any())`, `cd6ed01`'s hunk applied for it (above); clippy (`-D
 warnings`) on `x86_64-pc-windows-msvc` passes for sheaf-doc,
 sheaf-edit, sheaf-util and sheaf-image with their tests and for
 sheaf-render, sheaf-print, sheaf-ops and sheaf-ocr with all targets
 (sheaf-plugin, sheaf-app and sheaf-cli can't be checked here, as
 before). The release properties: Append ≡ Rewrite at 1,024 cases
 (with the `marked` dimension) and the replay property at 4,096 pass
 with set-watermark and remove-watermark (65 s together). The interop
 tier (qpdf; pdf.js 6.4.299 on node 24.22): its 10 ignored tests pass
 (slice 15: 8; 11 and 9 with the default one),
 `qpdf --check` clean on every output; pdf.js reads Acrobat's page as
 `Page 1` then `ACROBAT` in an `Artifact`, and the set, moved and
 removed pages both ways as `COPY` (in its `Artifact`), `Page 1`,
 `DRAFT` (in its), then `Page 1` alone, each loading its operator
 list (31, 32 and 6 operators).
   - **The reviews:** the plan was reviewed adversarially before the
 steps started, and its findings are the deviations above, as
 resolved. Each step checked its new checks by switching their
 behavior off (below).

   The review of the slice's changes (19 findings, each checked against
   the code; fixed unless said, each fix's test failing with the fix
   switched off):
   - **A file held as an attachment never drew as a watermark** (major):
 `Store::intern` hands back a live blob with the kind it was first
 interned as, and resolve judged that kind, so a PNG or a PDF already
 attached was Invalid. An image is an image, and a page a PDF's, by
 their bytes now (`BlobKind::image`; the PDF parsed).
 `a_file_held_as_an_attachment_draws_as_a_watermark`.
   - **A watermark found and removed came back in copies** (major):
 `subset` and `without_markup` built their states without the opened
 generators, so Clean Copy, extracts, prints and page images kept
 its blocks. `Builder::opened_as(st)` carries them.
 `a_found_watermark_removed_is_gone_from_copies`.
   - **A page another tool added under an `all` watermark** drew it on
 screen and in a Rewrite but not in an Append (minor): recognized as
 `Pages` of the pages holding a block (Deviations).
 `a_page_another_tool_added_has_no_watermark`.
   - **A merged block dropped its page from a `pages` scope** (minor), so
 an Update left the old drawing there: found by its ranges now
 (Deviations). `a_merged_block_keeps_its_page_in_scope` (both ways).
   - **An earlier save's page lost its watermark** (minor): `backs`
 matched the id alone, and a `GenId` lasts across saves; it reads the
 page now (Deviations). `a_page_from_an_earlier_save_keeps_its_
 background` (both ways).
   - **`replaces` was taken from the JSON unchecked** (minor): an entry
 whose `replaces` isn't its z's kind is passed over, and a found
 generator's is built from its z. A case in
 `what_sheaf_wouldn_t_write_isn_t_found`.
   - **An isolating stream was trusted by its dictionary** (minor, two
 findings): the rebuild set a stream marked `/Isolate` aside and the
 removal check said nothing of it whatever it held. One that holds
 more than `q`s or `Q`s (`content::isolates`, moved from the
 emitter) is the page's own content in both now.
 `an_isolating_stream_that_draws_is_the_page_s` (both ways) and
 removal_tests' `an_isolating_stream_that_draws_is_read`.
   - **A file whose catalog lost `/SheafGenerators`** (minor): the
 rebuild took its markers from `Source::markers`, which reads none
 for such a file, so a replacing watermark wrote its old block as an
 empty `Filtered` copy and nested the content once more. The
 rebuild reads each stream's marker from the dictionary it decodes
 anyway; `stack` and `intact` keep the catalog's O(1) gate.
 `blocks_a_catalog_doesn_t_name_are_left_out_whole` (both ways).
   - **Every Append decoded the image** (minor, two findings):
 `/SheafGenerators`' JSON, which needs an image's digest, was made
 before the Append knew it wrote it; it's made only then, the
 decision taken from whether any watermark is set. The bytes written
 are the same. `an_append_that_draws_nothing_makes_no_image`
 (`ARTS_MADE` 0 for a comment's Append, 1 for a move's).
   - **Background… with the Watermark dialog open closed it and opened
 nothing** (minor): the menu item toggles only its own z's dialog,
 and the other's closes as this one opens. No test: no headless
 test builds a `Workspace`.
   - **Setting the same watermark again said "Watermark"** (minor): it
 says "No change to the watermark" ("… background"), in
 `a_watermark_is_one_undo_step`.
   - **A rebuild digested a reopened page's layout three times**
 (minor): the rebuild uses `intact`'s digest; `drawn`'s layout is
 left for the perf pass (Left for later slices).
   - Quality: `a_pdfium_resave_keeps_the_watermark_found` removes its
 scratch directory (`watermark-resave`); untouched.rs' and
 generate.rs' docs re-wrapped; the older sheaf's watermark and
 background fixture is one, `generate::fixture` (the `fixtures`
 feature), for others_tests and sheaf-ops' watermark.rs (its image
 now red, green, blue and black, the pixel test unchanged);
 emit/content.rs' `told` is `model_layout_digest`, and the recipe's
 digest has one name, `recipe_digest` (realize/key.rs, re-exported
 by `generate`).
   - **Not done, for Greg:** dialogs show no counts (For Greg).
   - **After the fixes:** `cargo test --workspace` 1,491 passed and 42
 ignored (+13, the review's tests); fmt and clippy (`-D warnings`,
 all targets) clean; the corpus counts all as above
 (materialize_corpus, append_corpus, encrypted_corpus, through the
 scratch root); the release properties (Append ≡ Rewrite at 1,024,
 the replay at 4,096) pass; gate_7k on the synthetic and heavy
 (load 2.4 to 4.6) all pass: realize + open + render 12.1 and 22.5 ms,
 the strip 415 and 349 pages/s, the Rewrite against slice 8's 1.12×
 and 1.08× (one run each), crop 9.67 ms, ready with both found again
 93.8 and 498 ms. Each fix's test was checked by switching the fix
 off (11 fault runs, each restored from this review's own copy and
 checked with `cmp`; no proptest seed added).

   Checked by tests:
   - **The ops** (sheaf-doc tx/watermark_tests.rs, 10): one Generator
 prim an op; the same z keeps its id, the other z sits beside it;
 Remove narrowing `All` and `Pages` and clearing; the delete and
 duplicate cascades; every Invalid and NotFound message (an Image
 naming a PDF blob, a Page an image blob, a stamp grant); a kept
 image or page holding its blob and index; CJK text's Unencodable,
 the text kept; run lines; the invariants. Elsewhere:
 `a_watermark_naming_no_file_keeps_its_image_or_page`; effects'
 watermarks-and-backgrounds test, `filters_added_and_taken_off`,
 `a_watermark_replacing_others_takes_them_off`, `OlderSettings`
 apart from `Filtered`, and the every-pair samples; stale_tests (two
 watermarks of one z conflict, a background beside them doesn't, one
 on a page deleted since conflicts); tx/tests.rs' 26 kinds;
 sheaf-edit's JSON forms, defaults, misspelled and extra fields, and
 kept images and pages.
   - **Drawn on save** (generate/tests.rs, 5; materialize/
 watermark_tests.rs, 22, seven of them both ways): upright and centred
 at four turns and the corners at 90°; the block's shape, list,
 ExtGState, `Caf\xe9` and `??`; the digest blind to names; image
 proportions and turns; the JSON; one block for alike pages, the
 list bound in `/Properties`, the same bytes twice; the census (1
 font, 1 ExtGState, 1 block, a `PageRes` a Resources group); one image
 on a generated 7,000-page document; isolation counts for `q … cm`
 (1/2), `Q q` (2/2) and `Q` (2/1) with the running count ≥ 0 and
 ending at 0; an old OCR layer, new OCR and a watermark; "Café" and
 CJK; the first watermark's Append keys, a turn's one page and block,
 a scope change's page and `Generators`; 1.3 → `%PDF-1.4` and
 `/Version /1.4`; the unreadable-page Note; layout appending with no
 derived veto and text rewriting; removing the last leaving no
 `/SheafGenerators` or block; AES-128; one form for a page watermark;
 the reopened record numbering `Generators`. sheaf-ops tests/
 watermark.rs (PDFium): "Café" read back, a 90° page upright (char
 angle 90, x rising along one line), front watermarks over `q … cm`,
 `Q q` and `Q` pixel-identical to the clean base, an old OCR layer,
 forms checked clean with their values and fields drawn, all both
 ways.
   - **On screen:** untouched_tests' predicate-vs-emission cases for
 watermarks, backgrounds, reopened, moved, removed and Acrobat pages
 under every input, and `a_watermark_s_image_is_made_once_a_
 realizer`; key_tests (a reorder changes no watermarked page's key;
 each filter its own key); sheaf-ops realize.rs's
 `unsaved_watermarks_render_as_saved` (text, image background and a
 turned page with both, 0 px from the Rewrite in appearances, live,
 R0 and R90 panes; untouched pages from the base; one object number
 and one font in a group section); plan_tests' `a_claim_what_sheaf_
 draws_breaks_gets_a_note` (PDF/A-2b, PDF/UA-1, both, none, unreadable;
 a later comment-only Append none).
   - **Found again** (materialize/recognized_tests.rs, 33, thirteen
 cases both ways; the review's ten among them): found again; written as it is and untouched under screen,
 not content_only; changed in place with one block a page; removed
 with a Rewrite and no `DRAFT`, `SheafBlock`, `SheafGen` or `Isolate`
 left; gone by two removes, a delete cascade and an extract; an image
 found as `Placed`, moved without re-embedding, removed; `All`
 reaching a page inserted after reopen; five cycles at q depth ≤ 2;
 a hand-emptied block keeping its page; another file's block kept,
 isolated, then filtered; a merged block cut; a shared blank stream
 copied for the filtered page only; Appends after reopen (a comment
 its page, a turn its page and block, a move its pages, blocks and
 `Generators`; 0 derived vetoes); malformed entries passed over;
 `replaced` (an inserted file's watermark replaced, no doubling).
 sheaf-ops: `a_found_watermark_changed_or_removed_draws_as_set_
 afresh`, the ignored `a_pdfium_resave_keeps_the_watermark_found`
 (passes by hand); the fuzz fixtures found.
   - **Others' and older sheaf's** (materialize/others_tests.rs, 13):
 Acrobat's replaced after the content and in Acrobat's own layout,
 its form and graphics state off the page and out of the Rewrite,
 `Removes([Filtered 1])`, both ways; the other kind leaving it; Remove
 in one line for two ops; a Word-style author's header, footer and
 watermark kept, and with `include_authored` the one a splice takes
 gone and the other counted; an older sheaf's replaced and counted; a
 refused cut's Note and Remove's counts with an unreadable page; a
 shared blank stream; Clean Copy of a legacy text watermark and image
 background (no `SheafSettings`, `SheafStamp` or path; still `Tj`,
 `Do` and both `BDC`s; balanced; shared copies), a shared legacy
 stream both ways, a named list's settings kept with the Note.
 blocks_tests' rewrites and refusals; sheaf-ops: pixels outside
 Acrobat's box unchanged after Remove, Set over Acrobat's drawing as
 on the page without it, a Clean Copy's legacy page pixel-identical,
 and `corpus_files_with_others_watermarks` (0043199, 0128827);
 content/corpus.rs's rewrite pass; append_corpus's ignored
 `watermark_round_trips`.
   - **The app:** watermark_dialog's preview equal to applying (text,
 image, PDF page), `read_art` refusing a file that's neither, a kept
 image with none drawn, a stale preview dropped, the prefill before
 and after a save and reopen; docview's (Linux) `a_watermark_is_one_
 undo_step` and `remove_watermark_is_planned_and_counts_the_author_s`;
 forms_pages' watermark on ops, both ways, in `own_cache` (no derived
 blob, fields on screen, the form check clean, 4 fields and widgets,
 values, `/P`, appearances); crop.rs's `a_derived_flow_keeps_the_crop`;
 the thread-count test.
   - **Interop:** `a_watermark_s_blocks_read_in_pdfjs(_appended)`
 (above).
   - **Fault checks** (each restored from its step's own copy and
 checked with `cmp`; no proptest seed added): step 2's isolation
 ignoring `d` (the pixel test fails); step 3's memo off (3 decodes,
 not 1); step 4's `opened_generators` left out of `backs()`
 (`a_found_watermark_gone_any_way_leaves_no_block` fails), and the
 emitter's intact shortcut off, which changes nothing (the rebuild
 writes the same; the predicate-vs-emission test guards that the two
 agree); step 5's `replacing` off (8 sheaf-doc and 2 sheaf-ops tests
 fail); step 6's rewrite off (3 tests fail); step 8's `replacing` off
 again (both interop tests read `ACROBAT`), and retired.rs's 18 rows
 each hit by a probe file defining it at its path and naming it, the
 8 qualified-only names hit when qualified and not when bare (probe
 files in a package cargo doesn't build, deleted).

   For Greg:
   - **1.4, not 1.7:** a watermark needs `%PDF-1.4` here, as the slice's
 Check and `PdfVersion::ARTIFACTS` say; ISO 32000-1 Table 330 marks an
 artifact list's `/Subtype` as 1.7 (Deviations).
   - **`replaces` takes Acrobat's, an older sheaf's and another file's**
 of its kind, not Acrobat's alone (Deviations).
   - **Notes a save gives, wording yours:** "No watermark or background
 is drawn on page 3, whose content can't be read."; "1 watermark or
 background other programs or files drew stays on page 1: taking it
 out would change what else the page draws."; "1 watermark or
 background an older sheaf drew on page 1 keeps the settings it kept
 in it, which can't be taken out of it as it's written."; "This file
 claims PDF/A-2b; the watermark sheaf draws on it isn't made to
 conform, so the saved file no longer does".
   - **The backed rule** (Deviations): another file's sheaf watermark,
 inserted, stays until a filter or a `replaces` of its kind takes it,
 and so does an earlier save's of the same file on a page the
 document's watermark of that id doesn't cover; a page inserted from
 the very file the document opened is the document's, its blocks
 backed, as a duplicate's are.
   - **Dialogs show no counts** (proposal 31 and D61 say they do; the
 review): Remove's counts of others' artifacts are said only in its
 notice once it lands, and Set's never before the save, where a
 replacement shows as `Removes([Filtered n])` (a step's judging
 reads no blocks, so its commit says nothing of them). Counting the
 selected pages' replaced artifacts off the UI thread for the
 dialog, as Remove's planner does, is left; recommended with slice
 17's header dialog, which needs the same count.
   - **A page watermark comes from a PDF's page 1** in the dialog; the op
 takes any page.
   - **StripLegacy came early** for Clean Copy's Check; after a clean
 copy an older sheaf's range reads as the author's, so `replaces` and
 Remove leave it unless asked; one Word list in the corpus can't be
 rewritten (it crosses streams), and an older sheaf's always can.
   - **Remove's line and the app's notices:** "Remove the watermark from
 pages 1, 2 (2 made in Acrobat; 12 the author's left in place)",
 "No watermark to remove"; the Remove button always shown, removing
 from every page; the menu item toggling the dialog; the ported
 conflict test's "Strip" label.
   - **OCR reads a replaced Acrobat watermark:** the `replaces` cut goes
 with drawing generators, so under `content_only` an Acrobat
 watermark ours replaces is still the page's, and OCR can recognize
 its text.
   - **The manual Acrobat round trip** (Asks): open a file sheaf saved
 with a watermark and a background in Acrobat, see both drawn, edit
 Acrobat's own watermark there, save, and reopen in sheaf; whether
 Acrobat keeps or drops our marked blocks and `/SheafGenerators` is
 still inferred (PDFium's SaveAsCopy keeps them, the ignored test).
   - **The corpus checkout's new layout** needs `Corpus::find` (and
 realize_corpus's inventory name) taught it, or the old names back;
 until then every corpus test skips (Found).

   Left for later slices:
   - **Slice 17:** hoists `scope`, `z` and `replaces` onto `Generator`
 for the header and footer too, with `frozen` and `PageCtx` (the 16 →
 17 ledger row); `LIVE_REDRAW`, `SaveRecord.drawn` and
 `Emission.allowed_drops` (live tokens; a watermark has none);
 RemoveHeaderFooter's `include_authored` (and its `Order` read when
 it narrows `All`); `stamps::strip`, derived copies and
 `Derive::StripHeaderFooter`, with derive.rs and `Source::marked`'s
 Derived arm. No op of this slice fails on content (Remove counts an
 unreadable page, Set reads none), so slice 11's `EditError::Content`
 for a `ContentError` waits for 17–18.
   - **Slice 19:** RemovePrivateData: `LegacySel::Stream`, named lists'
 settings in `/Properties`, and renaming the `/SheafWM*` names in old
 files.
   - **A later perf pass:** the strip's per-page block, layout, digest
 and `PageRes` in overlays (memoize by generator, geom and base
 resources); an image's stream cloned into each page's delta;
 `generate::layout_digest` per key on reopened pages, and on the
 emitter's rebuild of one, where `intact`'s digest is used again but
 `drawn`'s layout digests a third time (memoize by generator and
 geometry an emission; the review); recognition at open (about 26 ms
 synthetic and 130 ms heavy over ready as opened);
 a need bound to a base name already bound to an identical object
 (Found).
   - **Follow-ups:** the `Closure::stale` probe (slice 11's Unprobed),
 untouched: no step changed that walk.
