# Backlog: closed entries

Entries move here when they close, saying where they went. IDs are never reused. The open ones: [the backlog](../backlog/README.md).

### B1: Calculator keys don't match PDFlyer's

- **Closed:** 2026-10-21. Not a bug: sheaf doesn't copy PDFlyer just
  for the sake of it (Greg). Keys are chosen on their merits, with
  [F4](../backlog/F4.md).
- **Found:** 2026-10-21, Claude, checking the Gemini summary against
  `docs/prior-art/pdflyer.md` §3
- **The differences**, from CCH's
  [Using10keyCalc](https://www.scanautoflow.com/ophelp/v22/en/Scan_WkStn/Content/PDFlyer/Using10keyCalc.htm)
  and the typed-key map in `crates/sheaf-app/src/calc_panel.rs`:

  | Key | PDFlyer | sheaf |
  |---|---|---|
  | `*` | multiply (the keypad's ×) | total (an adding machine's total key) |
  | G | MC | M+ |
  | H | MS | M− |
  | J | MR | MR |
  | K | M− | MC |
  | L | M+ | MS |
  | `T` | date and time into the note | nothing (Ctrl+T while editing a note) |

- **Notes:** M9 slice 13 chose the memory order without PDFlyer's table
  to hand (`plan/complete/m09-markup.md`).

### B5: A file whose signatures were broken still asks about them on a reorder

- **Closed:** 2026-10-23, fixed in the B5 PR. A
  signature a save would break or flag is checked first
  (`forms::sigs::integrity`, `Signature::integrity`): its `/ByteRange`
  hashed from the source's bytes against the digest its CMS signed (the
  messageDigest signed attribute, a document timestamp's messageImprint,
  or for no signed attributes the DigestInfo its RSA PKCS#1 v1.5 value
  opens to with the signer's certificate's key). One that no longer
  matches is `Verdict::AlreadyBroken` and doesn't ask; one that can't be
  checked (`Integrity::Unknown`: no signed attributes and not RSA, GOST,
  a CMS that can't be read) still asks. The form check reports one that
  fits its revision as `BrokenSignature`; `sheaf inspect --json` counts
  them (`signatures_broken`). Tests: sheaf-doc
  `forms::sigs::integrity::tests` (`a_signature_is_intact_until_a_byte_it_signs_changes`,
  `a_timestamp_is_checked_by_its_imprint`,
  `an_rsa_signature_with_no_signed_attributes_is_opened`,
  `what_can_t_be_checked_is_unknown_and_asks`,
  `byte_ranges_are_hashed_as_given_and_cut_at_the_end`,
  `a_byte_range_that_isn_t_pairs_is_unknown`, `bytes_before_the_header`,
  `ber_is_read`,
  `hostile_cms_is_unknown_not_a_crash`, `checks_are_shared_capped_and_lazy`,
  `a_broken_signature_asks_nothing`, and the first tier over
  `signatures/`, `the_corpus_s_signatures_agree_with_pyhanko`),
  `forms::tests::a_signature_whose_bytes_changed_is_found`,
  `materialize::plan_tests::a_section_writing_the_catalog_over_a_broken_signature_appends`;
  sheaf-ops `forms_saves::a_signature_a_rewrite_broke_isn_t_asked_about_again`
  (the repro); sheaf-cli `edit::a_signature_broken_already_isn_t_asked_about`
  (0008830, 0010094, and a file `--signatures break` wrote). The header
  tier on 62 signed forms (the first in `forms/`'s order with a signed
  field): 72 of the 74 reorders asked about the signatures (and were
  appended anyway), now 36. The 36 that stopped asking are 35 reorders
  after a forced Rewrite and 0008830's reorder of the file as it came
  (its signatures were broken already); the 36 that still ask reorder
  a file as it came, with its signatures intact.
  The mark tier: 359 ask, from 360; 7334180's signature, whose
  `/ByteRange` doesn't span its own `/Contents`, no longer asks before
  its Rewrite (`BaseUnsound`). The crop tier: 1104 ask, from 1121; the 17 that
  stopped (each signature stale or broken when it came) append, but
  7334180, rewritten (`BaseUnsound`). The form check's comparison
  (`Report::added_since`) counts a signature it found stale or broken
  before the save as nothing new, whatever the file's size after
  (`forms::check::tests::a_signature_broken_already_is_nothing_new`).

- **Found:** 2026-10-22, M10.5 slice 17's corpus tier (pre-existing)
- **Where:** `forms::sigs::sig_impact` (`crates/sheaf-doc/src/forms/sigs.rs:187`)
- **Severity:** annoyance
- **What happens:** a file rewritten with `break_signatures` (the user
  already agreed to break them) still gets the signature ask when its
  pages are reordered. `sig_impact` judges by `/V` and the permissions,
  not by whether the signed bytes still verify. This is 25 of the
  62-file sample's 100 reorder Appends.
- **Expected:** a signature whose digest no longer matches its bytes
  doesn't ask again.
- **Repro:** append_corpus's header tier (slice 17) on a signed form
  after a forced Rewrite, then reorder.
- **Notes:** probably needs the digest check pyHanko does
  (`../pdf-corpus/signatures/` has files changed after signing and
  digest mismatches to test against; see F9).
- **Also (M10.5 slice 19):** the exports' and the command line's
  signature ask reuse `sig_impact`, so (both fixed with it):
  - a signature already invalid when the file was opened still asks:
    `signatures/safedocs/0008830.pdf` and `0010094.pdf`, which pyHanko
    finds neither intact nor valid, exit 3 on a mark add (37
    unencrypted files in `signatures/` have such a signature). Both
    are signed with no signed attributes, so their digests are opened
    from their RSA values. Of `signatures/`, pyHanko gives a verdict of
    not intact to 17 signatures in 15 files, and sheaf finds exactly
    those broken; the manifest's other not-intact signatures are
    pyHanko's errors (59 document timestamps its run refuses, GOST
    digests, asn1crypto parse errors);
  - a file written with `sheaf … --signatures break` asks again on its
    next edit (`append_anyway: true`, "… removed since the last save").
- **The digest check, spiked** (2026-10-23, the build-or-adopt audit;
  [build-or-adopt.md](../research/build-or-adopt/README.md), entry 93):
  - cms 0.3.0-rc.0, der 0.8 (`ber`), sha1 and sha2, pinned exactly:
    about 40 lines at the core. The stable cms 0.2.3 can't read the 138
    of the corpus's 593 signatures that are BER.
  - 546 of the 593 are checkable. It agrees with pyHanko on
    `0008830.pdf` and `0010094.pdf`: neither is intact.
  - Unknown algorithms, no signed attributes (28) and parse failures
    keep asking; only a clear mismatch counts as broken.
  - Hash lazily, once per source, cancellable: N signatures cost N
    hashes of nearly the whole file.
  - The fix went further than the spike: with no signed attributes, an
    RSA PKCS#1 v1.5 value is opened to its DigestInfo, as 0008830 and
    0010094 need; other algorithms still keep asking.

### B6: sheaf-app's clippy fails without default features

- **Closed:** 2026-10-23. Fixed in the backlog-fixes PR:
  `Contributions::add_panel` is built only with the `plugins` feature,
  as only a plugin's manifest declares a panel. `cargo clippy -p
  sheaf-app --all-targets --no-default-features -- -D warnings` is clean;
  still no CI job builds it.
- **Found:** 2026-10-22, M10.5 slice 18 (pre-existing)
- **Where:** `crates/sheaf-app/src/contrib.rs:158`,
  `Contributions::add_panel`
- **Severity:** annoyance (build)
- **What happens:** `cargo clippy -p sheaf-app --all-targets
  --no-default-features -- -D warnings` fails: `add_panel` is dead
  without the `plugins` feature.
- **Expected:** clean, or the configuration isn't supported.
- **Repro:** the command above.
- **Notes:** no CI configuration builds it. Gate the fn on the feature,
  or decide the no-plugins build isn't supported and say so.

### B7: Clear OCR text silently does nothing on a damaged old layer

- **Closed:** 2026-10-23. Fixed in the backlog-fixes PR:
  `PageBlocks::legacy_ocr_stays` says a layer stream is outside the
  pair. On a page with nothing else to take out, a Clear (or a Set)
  adds no filter, changes nothing, and the commit says the layer stays
  (`Notice::OcrStays`, the save's Note's words); a page with text taken
  out keeps the save's Note, so neither says it twice. Tests:
  `a_lone_legacy_layer_without_its_q_stays_and_says_so`
  (materialize/ocr_tests.rs) and, through the app's command (F8),
  `removing_recognized_text_says_what_stays`.
- **Found:** 2026-10-22, M10.5 slice 18
- **Where:** `ClearOcrText`'s lowering (`crates/sheaf-doc/src/tx/`), the
  legacy `/SheafOcrLayer` selector (`StreamSel::SheafOcr`)
- **Severity:** annoyance
- **What happens:** an older sheaf's OCR layer is a `q` plus a
  `/SheafOcrLayer` stream. If another tool lost the `q` (no pair, no
  block), a Clear adds no filter: it changes nothing and says nothing.
- **Expected:** a Note that the old layer stays, as other kept artifacts
  get ("… stays on page 3: taking it out would change what else the
  page draws").
- **Repro:** a page whose only legacy OCR stream has no leading `q`;
  ClearOcrText; save.
- **Notes:** taking it out without the `q` isn't safe (`3 Tr` would
  leak), so keeping it is right; only the silence is wrong.

### B8: The comment summary PDF has no title, producer or creation date

- **Closed:** 2026-10-23. Fixed in the backlog-fixes PR: the summary's
  primary is `memory::titled`'s (`Session::new_titled`), a PDF of no
  pages whose `/Info` has the title, `/Producer` sheaf and
  `/CreationDate`, which a save keeps. No op sets document info: that's
  for the Properties editor's op (Pending) to design. Test: sheaf-cli's
  `the_summary_has_its_title_producer_and_date` (`-o`, both layouts).
- **Found:** 2026-10-22, Claude, during M10.5 slice 19 (step 5, the
  summary as a Session)
- **Where:** sheaf-ops `comments::summary`, the CLI's `sheaf comments
  summary` and the app's Comments panel
- **Severity:** annoyance
- **What happens:** the summary is now a Session made with `new_empty`
  plus an insert of the `Memory(Summary)` blob. A session's output takes
  its `/Info` from its primary, which is the empty document, and no op
  sets document info. So the saved summary has no `/Title`, `/Producer`
  or `/CreationDate`.
- **Expected:** as before slice 19: `/Title` is the summary's title,
  `/Producer` is sheaf, `/CreationDate` is when it was made. A viewer's
  title bar and Properties show them.
- **Repro:** `sheaf comments summary in.pdf -o out.pdf`, then
  `qpdf --show-object=trailer out.pdf` (or Properties): no `/Info`
  entries.
- **Notes:** Greg: "log it" (2026-10-22). The fix is an op (or a Session
  option) that sets document info, which a Properties editor would also
  want. Until then the summary could set it on its `new_empty` primary.

### B12: Flatten draws a comment's first appearance state when it has no `/AS`

- **Closed:** 2026-10-23. Fixed in the backlog-fixes PR: a non-widget
  annotation whose `/AP /N` has states draws what PDFium shows, the page
  view's engine: `/AS`'s state, else the one `/V` (or its parent's)
  names, else `/Off`, else nothing; never its first, and nothing made of
  its border or fill. A name or a string names a state. Widgets keep
  their rule (M10 bug 10). It's one choice (`source::appearance`'s
  `Shows::form`), so Export Flattened, print with markup, print preview
  and page images all follow it. pdf.js differs where the two disagree:
  it never takes `/V` or `/Off`, and for a Square, Circle, Ink, the text
  markups and FreeText with a border or a fill it draws an appearance of
  its own. Tests: `a_comment_with_states_draws_the_one_pdfium_shows`
  (materialize/flatten_tests.rs: states and no `/AS`, with a border, with
  a fill, an `/Off`, a `/V`, a parent's `/V`, a `/V` naming none, an
  `/AS` naming none, a string `/AS`), `flatten_takes_what_shows_and_leaves_the_rest`
  and `printable_leaves_what_isnt_printed` (their comment draws its
  `/Off`), sheaf-cli's `a_comment_with_states_exports_as_the_page_view_shows_it`,
  and sheaf-render's `the_page_view_shows_a_comment_s_value_s_state_or_its_off`,
  which pins what PDFium shows of the same file.
- **Found:** 2026-10-22, M10.5 slice 19 (step 2, `FlattenAnnotations`)
  (pre-existing: the old flatten did the same)
- **Where:** `FlattenAnnotations`' choice of appearance for a
  non-widget (`crates/sheaf-doc/src/tx/flatten.rs`)
- **Severity:** wrong output
- **What happens:** a comment (not a widget) whose `/AP /N` has states
  and no `/AS` is flattened with its first state. PDFium shows nothing
  for it (it falls back to `/V`, then `/Off`), so flattening draws what
  no viewer showed.
- **Expected:** flatten draws what the viewer showed, nothing here.
  Widgets already have no first-state fallback.
- **Repro:** a Square annotation with `/AP << /N << /A … /B … >> >>` and
  no `/AS`; Export Flattened.
- **Notes:** rare in the wild; check pdf.js and Acrobat agree with PDFium
  before changing it.
  - **Checked (2026-10-23, the backlog-fixes PR):** pdf.js and PDFium
    agree on nothing for the repro, but not on the cases beside it.
    Neither ever draws the first state.
    - **pdf.js** (6.4.299, `Annotation.setAppearance` in
      src/core/annotation.js): with `/N` a dictionary and no `/AS` name,
      no appearance at all, never `/V` or `/Off`. Square, Circle, Line,
      Polygon, Ink, the text markups and FreeText then draw one pdf.js
      makes from `/Rect`, `/C`, `/IC` and the border, if the border has
      width or there's a fill; a Stamp draws nothing.
    - **PDFium** (sheaf's, the page view's engine): `/AS`, else `/V` (the
      annotation's or its parent's), else `/Off`; nothing when that
      state isn't there, and no appearance of its own while `/AP /N` is.
    - **Hand-made files** (one Square, states A and B, no `/AS`): both
      draw nothing; with `/BS /W 4` or an `/IC`, PDFium nothing and
      pdf.js its own square; with states A and `/Off` instead, PDFium
      draws Off and pdf.js nothing (its own square with a border). A
      Circle is the Square's case; a Stamp draws nothing in both.
    - **Acrobat:** not found in its docs. ISO 32000-1 12.5.5 requires
      `/AS` when `/AP` has subdictionaries, so such a file isn't
      conforming.
    - **Not only Export Flattened:** print with markup, print preview
      and Export Pages' images draw through the same flatten
      (`FlattenSet::Printable`), so they show the first state where the
      page view shows nothing (`sheaf export` drew state A on the repro).
    - **The choice:** PDFium's, what the page view shows (the
      coordinator, under Greg's standing instruction to take the
      recommended answer), over drawing nothing as pdf.js does but for
      its own appearances.

### B13: A print file with markup renumbers its header when reopened in sheaf

- **Closed:** 2026-10-23. Fixed in the backlog-fixes PR: print with
  markup leaves sheaf's private data out as print without markup does
  (`RemovePrivateData`, `Private::Scrub`), so what sheaf drew is plain
  content and nothing in the file is live. Chosen over writing an
  `AsDocument` keep's generator blocks as plain content: one rule for
  both kinds of print, through what's already there, and a print file
  has no use for the rest of sheaf's private data either (its marks are
  drawn into its pages). Test:
  `a_print_file_opened_again_says_the_document_s_numbers`
  (materialize/header_tests.rs: pages 5–6 of 10, of a document saved
  and not, the file and a save of it reopened say "Page 5 of 10").
- **Found:** 2026-10-23, M10.5 slice 19's wrap-up (the print numbering
  fix)
- **Where:** `Export::print` with markup (`crates/sheaf-doc/src/export.rs`),
  which keeps sheaf's private data (`Private::Keep`)
- **Severity:** wrong output
- **What happens:** printing pages 5–6 of 10 with markup writes "Page 5
  of 10", but the file keeps the header live in `/SheafGenerators`. Open
  the print preview's Save as PDF output in sheaf and the header is
  numbered against that file's own pages, "Page 1 of 2", and a save of
  it writes that. Other viewers show "Page 5 of 10". Print without
  markup writes the header as plain content, so it stays.
- **Expected:** the file reads "Page 5 of 10" in sheaf too.
- **Repro:** a 10-page document with a "Page <<1>> of <<N>>" header;
  print preview of pages 5–6 with markup, Save as PDF; open it in sheaf.
- **Notes:** before the fix the print file said "Page 1 of 2"
  everywhere. Candidate fixes: print with markup leaves out sheaf's
  private data as print without markup does (Scrub), or an `AsDocument`
  keep writes its generators' blocks as plain content.

### B21: `layers::LayerId` is a lopdf `ObjectId` under another name

- **Closed:** 2026-10-23. Fixed in the backlog-fixes PR: `LayerId` is
  sheaf's own, a type over the group's object that only layers.rs reads
  (not an `ObjRef`: the facts are read from a `&Source`, with no `Src`
  to hold, and the view's override is keyed by source already).
  `tests/object_ids.rs` sees through a type alias that names `ObjectId`,
  or names such an alias, and catches a path through one; its probe
  `an_alias_of_an_object_id_is_seen_through` tests that. With the alias
  back, `only_a_source_s_readers_and_writers_name_an_object_id` fails
  on realize/mod.rs and realize/inputs.rs.
- **Found:** 2026-10-23, M10.5 slice 20
- **Where:** `crates/sheaf-doc/src/layers.rs` (`pub type LayerId =
  ObjectId`), used by `ViewInputs::off` and the app's layers panel
- **Severity:** annoyance (a design leak)
- **What happens:** an optional content layer is named by its file
  object id outside the `ObjectId` allowlist. `tests/object_ids.rs`
  can't see through the alias.
- **Expected:** a layer id of sheaf's own, as pages and marks have, or
  an `ObjRef`.
- **Notes:** also make the grep test catch type aliases of `ObjectId`.

### B22: `FlatOutlineItem` derives Serialize and Deserialize that nothing uses

- **Closed:** 2026-10-23. Fixed in the backlog-fixes PR: the derives
  and their `serde` attributes are gone; nothing read or wrote it.
- **Found:** 2026-10-23, M10.5 slice 20 (pre-existing)
- **Where:** `crates/sheaf-doc/src/open/outline.rs`
- **Severity:** annoyance (dead code)
- **What happens:** the derives have no serializer anywhere in the
  workspace, and they pin the type's shape for no reader.
- **Expected:** drop them, or say who reads it.

### F8: A command to remove recognized text

- **Closed:** 2026-10-23. Done in the backlog-fixes PR: Document >
  Remove Recognized Text… (`docview::RemoveRecognizedText`,
  `ocr_clear.rs`) asks whether the selected pages or every page, then
  runs `ClearOcrText` as a planner, one undo step, the notice its line,
  what stays (B7), or that there was none. The command line's form is
  `sheaf edit`'s `clear-ocr-text`, which README now says. Tests:
  `removing_recognized_text_is_one_undo_step`,
  `removing_recognized_text_says_what_stays` and
  `it_asks_about_the_selected_pages_or_every_page`.
- **Found:** 2026-10-22, M10.5 slice 18
- **What:** an app command (and a CLI form) that runs `ClearOcrText`
  on the selected pages or all pages, with the usual Notes.
- **Why:** slice 18 made saved OCR layers removable (bug 14), but only
  tests, the API and fuzz call `ClearOcrText`. A user who wants a bad
  OCR layer gone can only re-OCR over it.
- **Overlap:** the OCR dialog (`crates/sheaf-app/src/ocr_*`); M10.5
  slice 19's `sheaf edit` takes any op as JSON, so the CLI form may come
  free.
- **Notes:** small: a command and a confirmation. The op, its Notes
  and the Rewrite it plans exist.

### F14: An automated test of `sheaf scan --ocr`

- **Closed:** 2026-10-23. Fixed in M5 slice 1 (`4481573`):
  `crates/sheaf-cli/tests/scan.rs`. It runs in `cargo test --workspace`
  on every CI runner (sheaf-cli's default features build Tesseract, and
  CI fetches the OCR data); without them the scan fails, never skips.
- **Found:** 2026-10-22, M10.5 slice 19 (step 5, scan as a Session)
- **What:** a CLI test that runs `sheaf scan --ocr` on a few images and
  checks the output's text layer: the words found, one OCR block a page,
  one font.
- **Why:** slice 19 moved scan onto a Session (an empty session plus an
  insert of the images, then OCR). Only a hand run checks the `--ocr`
  path; the OCR op itself is tested in sheaf-doc and the corpus tiers.
- **Overlap:** M5's scanner intake (ROADMAP), which reworks scan.
- **Notes:** the worry was that Tesseract may be missing in CI. Check
  what `--ocr` needs (sheaf-ocr's default engine vs the `tesseract`
  feature, used only for orientation) and either run it everywhere or
  install what it needs in CI. Don't make it skip silently.

### B11: Create from Scans still writes a file and reopens it

- **Closed:** 2026-10-23. Fixed in M5 slice 2: Create from Scans streams
  its pages into an untitled document in a new or empty window; nothing
  is written until Save, which is Save As.
- **Found:** 2026-10-22, M10.5 slice 19
- **Where:** `Workspace::create_from_scans`
  (`crates/sheaf-app/src/workspace.rs`), `open_paths`
- **Severity:** annoyance
- **What happens:** scan builds a Session now, but the app writes its
  result to a file and opens that file, where Combine hands its
  Document to a window as made.
- **Expected:** the scan's Session handed to a window, as Combine does:
  no file until the user saves.
- **Repro:** Create from Scans in the app.
- **Notes:** M5's scanner intake (ROADMAP) reworks this path; do it
  there.

### F22: Check what sheaf writes against the Arlington PDF Model

- **Closed:** 2026-10-24. Done in the Arlington checks PR: the model's
  `tsv/latest` at `d148fee` (2026-10-21) is vendored in
  `vendor/arlington-pdf-model` with its LICENSE, NOTICE.txt and a
  SOURCE.md, and read by the test-only `sheaf_doc::arlington` (behind
  `fixtures`): the TSVs; their predicates, 19 functions implemented
  (4,515 of the model's 4,729 calls), `fn:Deprecated` marking what it
  wraps and still allowing it, any other counted as a check not made;
  and a walk from the trailer as written that follows the Link column,
  each object's table chosen by TestGrammar's scoring (ported,
  Apache-2.0). Its rules: types, required entries (inherited ones too),
  too new for the file's version, deprecated, allowed values, direct and
  indirect, SpecialCase, unresolved, unreached, malformed trees, and in
  an object sheaf built whole, a key the model doesn't list that's no
  second- or third-class name nor sheaf's private key; every unlisted key
  is counted by its kind of name. `Hold::written` holds only what a save
  or export wrote (the objects an Append's cross-reference section lists,
  read or not; the objects a Rewrite derived; never a copy of a file's),
  each built whole or merged by its key; `Report::beyond` carries a
  finding in a merged object that the file read has too, in any of its
  objects (per object is B46), and
  `crates/sheaf-ops/tests/arlington_allowed.tsv` holds the rest for a
  backlog entry (B40–B45), by file and by version. Tests: sheaf-doc's
  `arlington::` units (54, each rule fault-checked);
  `crates/sheaf-ops/tests/arlington.rs`'s 38 default tests, every
  save-side writer both ways (`every_mark_kind_holds` to
  `a_pdf_2_0_file_holds`), the exports, Combine, the summary, images, a
  scan and the stamps, and the allowances (each `*` row needed, each
  entry open, B43 only from 2.0); and
  `what_sheaf_writes_on_every_form_holds_to_arlington` (`just
  tier-arlington`, in `premerge`): 4,389 of 4,392 forms (3 don't open),
  2,062,906 objects as 127 tables, 11,345,111 keys, 150,604 checks not
  made, 18,670 findings carried from the forms, 1,289 allowed, none
  left, in a minute; its form and save counts are asserted. Two writer
  fixes: a form `stamp_lib::page_forms` makes of a page without
  resources gets an empty `/Resources` (library stamps, page stamps,
  page watermarks and the summary's page pictures), and a file of no
  pages has no content stream.
- **Found:** 2026-10-23, the build-or-adopt audit
- **What:** a test-only checker that reads the Arlington PDF Model's TSV
  files
  ([pdf-association/arlington-pdf-model](https://github.com/pdf-association/arlington-pdf-model),
  Apache-2.0) and checks every object sheaf builds: annotations,
  appearance streams' resource dictionaries, form fields, the catalog
  and trailer keys it writes, and later StructElems and output intents.
  It checks keys, types, required entries and version predicates. Run it
  over the emission in the existing save tiers.
- **Why:** PDF interoperability is required, and viewers are lenient, so
  a malformed key can pass PDFium and pdf.js and still fail Acrobat or a
  validator. krilla runs Arlington in CI for the same reason.
- **Overlap:** F9 (`conformance/`); M14 PDF/A; M17 PDF/UA; the interop
  tests with qpdf and pdf.js.
- **Notes:** no Rust implementation exists; TestGrammar (C++) and
  veraPDF's Arlington service do. The TSVs are data: vendor a pinned
  copy under the test fixtures with its license, or read them from
  `../pdf-corpus/tools`. Start with the objects sheaf creates, not every
  object it copies. [build-or-adopt.md](../research/build-or-adopt/README.md), entry 94.

### F24: sheaf's private names as second- or third-class names

- **Closed:** 2026-10-25, done: the five private names sheaf writes are
  second-class names with the prefix `SHEF_` (Greg chose `SHEF`):
  `/SHEF_Annot`, `/SHEF_Block`, `/SHEF_Generators`, `/SHEF_Gen` and
  `/SHEF_OcrBlock`. The Arlington checker's exemption for them is gone,
  and a test holds every name sheaf writes to the prefix. Registering it
  waits for the first release ([F28](../backlog/F28.md)).
- **Found:** 2026-10-24, the Arlington checks (F22)
- **What:** rename the keys `/SheafAnnot` (a mark), `/SheafBlock` (a
  block's stream) and `/SheafGenerators` (the catalog), and the
  marked-content names `/SheafGen` and `/SheafOcrBlock`
  (`private::KEYS`), to a registered second-class prefix (e.g.
  `SHEF_Annot`) or third-class `XX` names, or move the page and form data
  under `/PieceInfo`, the spec's place for an application's private data.
  Or keep them.
- **Why:** ISO 32000-2 Annex E keeps first-class names for the spec. A
  validator could flag them, or a future spec key could collide.
  Arlington counts them as unlisted "other"; the checker exempts them by
  name from the `unlisted` rule, and a renamed set would need no
  exemption.
- **Overlap:** M10.5's private data (`private.rs`); the sidecar format
- **Notes:** pre-alpha, so a rename has no compatibility cost (clean over
  compat). A decision for Greg. Registering a prefix is with the PDF
  Association's registry.

### B3: Properties says "Fast Web View: Yes" after an incremental save

- **Closed:** 2026-10-26, fixed in the object-streams PR, slice 1
  ([object-streams.md](object-streams.md), decision 9). The
  vendored lopdf's `LazyPdf::open` counts a file linearized only when its
  first object, read whole within 1,024 bytes of the header, is a
  dictionary whose `/Linearized` is 1 (a number whose floor is 1, as
  qpdf reads it) and whose `/L`, a direct integer, is the file's length
  from the header (`holds_linearization`, `XrefFacts::linearized`);
  an Append's output record says not linearized; and File > Properties'
  Fast Web View reads the saved record when it describes the file shown
  (`PropertiesContext::linearized`), since a save never replaces the
  primary. Tests: sheaf-doc
  `source::lazy_tests::a_linearization_holds_while_its_length_is_the_file_s`
  and `materialize::append_tests::a_linearized_file_appended_to_is_linearized_no_more`
  (on `serialize::fixture::linearized_file`); sheaf-app
  `docview::tests::properties_says_no_fast_web_view_once_a_save_appends`
  (the repro, without a reopen); sheaf-ops interop's
  `a_linearized_file_appended_to_is_linearized_no_more` (`qpdf
  --linearize`, then a sheaf Append) and append_corpus's
  `linearization_agrees_with_qpdf_before_and_after_an_append`: sheaf and
  `qpdf --check-linearization` agree on each of the pool sample's 402
  files, 150 linearized before a forced Append and 0 after (the old flag
  said 216 and 216). realize_corpus's linearized row went from 2,523
  files to 1,791. They agree on the pool, not on every file: sheaf
  follows Annex F and wants the whole dictionary within the first 1,024
  bytes, while qpdf 12.3.2 asks only that the object start there, so a
  dictionary ending at byte 1,025 is linearized to qpdf and not to
  sheaf. No pool file is such; the linearization crate's checker (item
  3) keeps the rule or changes it on purpose.
- **Found:** 2026-10-22, Claude, while Greg and Claude discussed where
  linearization belongs (M14), during M10.5 slice 18
- **Where:** the vendored lopdf's `vendor/lopdf/src/lazy.rs:443`, which
  sets `XrefFacts.linearized`; `crates/sheaf-doc/src/materialize/append.rs:477`,
  which carries it into the output record; shown by
  `crates/sheaf-app/src/properties_dialog.rs:147`
- **Severity:** wrong output
- **What happens:** a file counts as linearized whenever `/Linearized`
  appears within 1,024 bytes of its header. An Append keeps the input's
  flag, and a reopen finds the same dictionary at the top. So after
  sheaf (or any tool) appends an update to a linearized file,
  Properties still says Fast Web View: Yes. The dictionary's `/L` and
  hint tables no longer describe the file, and readers drop the fast
  path.
- **Expected:** Yes only when the linearization dictionary still holds.
  At least `/L` must equal the file's length (ISO 32000-1 Annex F: an
  updated file whose `/L` doesn't match isn't treated as linearized).
  After an Append, the output record says not linearized.
- **Repro:** found by reading the code, not yet run. Take a linearized
  corpus file (the pool sample has 216), add a mark, save (an Append),
  and open Properties. `qpdf --check-linearization` on the saved file
  should disagree with sheaf.
- **Notes:**
  - The fix is small: compare `/L` with the file length where the
    flag is set, and write `false` in the Append's record.
  - realize_corpus groups its stats by this flag, so its "linearized"
    row includes files updated after linearizing.
  - Writing linearized files is M14's (Optimize: linearization as a
    writer option).

### B45: Arlington requires `/DA` on button fields

- **Closed:** 2026-10-26. pdf-lint's embedded model corrects the
  FieldBtn* tables' /DA (corrections.tsv; ISO 32000-2 Table 228); the
  allowance row went. Done in the pdf-lint item's slice 1
  ([pdf-lint.md](pdf-lint.md)): `just tier-arlington` on
  pdf-lint 0.1.0 finds no `/DA` missing in 0976876's radio field, and
  the corpus tier's 72 such findings carried from other forms (50
  FieldBtnRadio, 20 FieldBtnCheckbox, 2 FieldBtnPush) are gone too.
- **Found:** 2026-10-24, the Arlington checks (F22)
- **Where:** the model's FieldBtn* tables
- **Severity:** annoyance (a question for the model)
- **What happens:** a radio field that a save writes apart from its
  widget (BaseObjectsMissing), where neither the field nor the form has
  `/DA`, is a `missing` finding (0976876).
- **Expected:** ISO 32000-2 Table 228 asks `/DA` only of variable-text
  fields.
- **Repro:** `just tier-arlington` on `forms/safedocs/0976876.pdf`
  without the allowance row
- **Notes:** could go upstream with B40. Held by a row scoped to
  0976876.

### F13: In-house what sheaf uses of lopdf and drop the dependency

- **Closed:** 2026-10-27, became [M18](../m18-pdf-layer/README.md), the
  PDF layer, which started that day. Greg: "dropping lopdf isn't
  exactly the goal here. a coherent pdf crate family is." lopdf goes
  as part of that family, split from pdf-lint.
- **Found:** 2026-10-22, Greg and Claude, during M10.5 slice 19
- **What:** a long-term direction, not scheduled, and never mid-milestone
  (Greg, 2026-10-22). Bring what sheaf uses of lopdf into sheaf's own
  crates and drop the dependency, while still sending upstream any fix
  found along the way. It isn't either/or.
  - **Already sheaf's own:** the writer (`serialize/`), the content-stream
    code (`content/`) and the lazy-reading layer (`source/`).
  - **Still lopdf's:**
    - the object model;
    - the object parser and cross-reference reading;
    - object streams;
    - stream decompression;
    - encryption, which sheaf would keep on the RustCrypto crates and
      not hand-roll;
    - the eager `Document`: the Rewrite graph, PDFs built in memory,
      `prune_objects`;
    - its writer, used only by `annot::tape_pdf`, tests and fixtures.
  - About 9k of lopdf's 23.5k lines are off sheaf's production paths:
    text encodings and glyph tables, CMap parsing, outlines, bookmarks
    and TOC, fonts, dates, XObject helpers, IncrementalDocument.
- **Why:** sheaf carries a vendored, patched lopdf whose patches reshape
  its reader, encryption and object model. The vendored crate is outside
  the workspace, so sheaf's clippy `-D warnings` and lint bans don't run
  on it, and each new upstream commit means reapplying the patch.
- **A first step:** move `LazyPdf` (`vendor/lopdf/src/lazy.rs`, 989
  lines) into sheaf-doc's `source/`.
  - The byte sources and caps (`ByteSource`, `FileBytes`, `ReadLimits`,
    `parse_from`) stay in lopdf, whose shared `XrefReader` reads through
    them.
  - lopdf then makes public what LazyPdf reads today as crate-private:
    from reader.rs, `XrefReader`, `Resolved`, `decryption`, `encrypts`,
    `header_offset`, `length_value`, `normal_offsets`, `object_bound`;
    from the parser, `Resolve`, `header`, `indirect_object_in`,
    `stream_end`.
  - Keep `source::lazy_tests` identical across the move: every object of
    the corpus's readable forms, read lazily, equals the eager load.
- **Upstream meanwhile** (Greg: the weekend of 2026-10-24/25, each pull
  request confirmed first): `vendor/lopdf/SHEAF.md` lists fixes that
  stand on their own:
  - each xref stream read once;
  - lenient `/XRefStm` past EOF;
  - white space around `startxref`;
  - `/Encrypt null`;
  - strings in stream dictionaries (lopdf #603);
  - `/EFF`;
  - V4+ filter names and `/Identity`;
  - the set in `traverse_objects` and `prune_objects`.
- **Upstream since** (2026-10-23, the build-or-adopt audit, read in
  `J-F-Liu/lopdf` main at `d0b3abf`, not run; [build-or-adopt.md](../research/build-or-adopt/README.md), entry 2):
  - 0.46.0 (2026-10-21) has #598 and the stream-dictionary strings
    (#603, `2f497f8`).
  - Merged since: V4+ `/Identity` defaults (#624) and
    `traverse_objects` and `prune_objects` in linear time (#630), both
    still to confirm as the same fixes as sheaf's; caps in object
    streams and predictors (#625); rebuilding files that have only
    cross-reference streams (#613, which the audit placed both in 0.46.0
    and after it).
  - Still to send: `/EFF`, `/Encrypt null`, white space around
    `startxref`, and each xref stream read once and lenient `/XRefStm`,
    if still needed.
  - Diff each `sheaf:` hunk against upstream main before writing its PR.
- **Overlap:** `vendor/lopdf/SHEAF.md`, "Going back to crates.io";
  F15 (a Rewrite that doesn't load the file whole) would remove the
  largest use of the eager `Document`.
- **Notes:** to size at triage: the object model's reach (about 400
  import sites), and the parser's edge cases, which lopdf's parser
  fixes and the corpus comparisons encode (`source::lazy_tests`; the
  corpus runs).
- **Maybe published** (Greg asked, 2026-10-22): the in-housed lopdf parts,
  `LazyPdf`, sheaf-doc's writer (`serialize/`), its `source/` and
  `geom`, and the generic half of `content/` (lex, writer, splice) could
  become one layer crate, and later perhaps a published alternative to
  lopdf, as `in-progress` and `instance-handoff` were extracted.
  - **Production coupling today:**
    - `serialize` reaches `materialize` (`ObjKey`) and `source`;
    - `source` reaches `content`, `geom` and `store` (`BlobId`);
    - `content` reaches `annot` and `private`, which are sheaf's blocks
      and markers.
  - Make it an internal crate with a clean boundary first; decide on
    publishing after an ecosystem survey.
  - **The survey** (2026-10-23, the build-or-adopt audit): no published
    crate offers an owned, f64, order-keeping, lazy, capped object layer
    with byte spans, so a crate of that layer would have a real niche.
    The nearest are hayro-syntax (read-only, borrowed objects) and lopdf
    (eager, f32).
- **Deferred** (Greg, 2026-10-22): revisit after more milestones, since
  what sheaf needs from this layer may change as later scopes dig in.
  Upstream fixes go ahead meanwhile.
