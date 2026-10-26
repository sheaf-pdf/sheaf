# sheaf's Arlington checks on pdf-lint

Status: done

An item [between milestones](../between/README.md), next after object streams
(Greg, 2026-10-26). sheaf's Arlington checker was extracted and published
as [pdf-lint](https://github.com/sheaf-pdf/pdf-lint) (crates.io 0.1.0,
Apache-2.0). sheaf moves to it and drops its own copy. After this item,
the work between milestones pauses until Greg says.

## Why

- **Duplication.** `crates/sheaf-doc/src/arlington/` is about 7,500
  lines, and `vendor/arlington-pdf-model` is 2.5 MB. Both are test-only,
  never in a shipped build, and pdf-lint now maintains the same
  checker.
- **pdf-lint checks more.**
  - Its embedded model carries corrections where the model contradicts
    ISO 32000-2. One of them is
    [B45](backlog-closed.md#b45-arlington-requires-da-on-button-fields)'s,
    `/DA` on button fields.
  - Its bytes reader finds what a writer can get wrong below the object
    level: a `/Length` that doesn't end at `endstream`, cross-reference
    rows that aren't 20 bytes, a key written twice, a last line that
    isn't `%%EOF`.
  - It checks against the model's grammar the way the PDF Association's
    TestGrammar does.
- **CI can check every platform.** sheaf's outputs hold object streams
  since [object streams](object-streams.md), and pdf-lint
  decodes them with qpdf. `sheaf-pdf/setup-pdf-lint@v1` installs pdf-lint
  and its pinned, checksummed qpdf on Linux, macOS and Windows runners.

## Decided

Greg's standing rule applies: an open question with a recommended answer
is taken, and noted.

1. **A dev-dependency on the library,** `pdf-lint = "=0.1.0"`, for the
   tests that hold what sheaf writes (`crates/sheaf-ops/tests/arlington.rs`
   and its corpus tier).
   - A test needs what only the library has:
     - sheaf's record of which objects a save wrote, built or merged;
     - `Report::beyond` over every file a document read (Combine reads
       several);
     - the allowances held for backlog entries;
     - assertions on which tables were checked.
   - The command line takes one `--baseline` file, so it can't express
     these.
   - As built: in `crates/sheaf-ops/tests/lint/`, which `arlington.rs`
     uses: `read`, `options`, `check` and `written` (slice 1's notes).
   - pdf-lint is Apache-2.0 only. As a dev-dependency it never reaches a
     shipped build, so sheaf stays MIT OR Apache-2.0.
2. **Read with pdf-lint's bytes reader and its qpdf decoder,** not a
   `Source` over sheaf's lopdf reader.
   - The bytes reader is what checks a writer's syntax.
   - A lopdf `Source` would lose `/Length` as written, the trailer's
     `/Encrypt`, warnings and byte spans, which pdf-lint's README lists.
3. **qpdf 11.9 or later** decodes object streams, compressed
   cross-reference streams and encryption.
   - Locally it is `/usr/bin/qpdf` (12.3.2).
   - In CI, `setup-pdf-lint` puts pdf-lint, with its bundled qpdf, on
     each runner, and `PDF_LINT_QPDF` points the library at that qpdf.
     It is pinned to `version: 0.1.0`, the version the dev-dependency
     has. As built: that qpdf is 12.4.2.
   - A test that needs qpdf and finds none fails, saying why. It never
     passes vacuously. Locally, qpdf comes from the system, as the
     interop tests already need it.
4. **What sheaf keeps** (in sheaf-ops' tests, or a small test-support
   module):
   - **The hold:** the record's map to pdf-lint's hold, the
     objects sheaf wrote, built or merged. This is today's
     `Hold::written`.
   - **Allowances:** one TSV, still each row held for an open backlog
     entry. `every_allowance_for_any_file_is_needed` and
     `every_allowance_names_an_open_backlog_entry` keep passing. Use
     pdf-lint's allowance format and API where it has them (its
     `--allow` TSV has table, key, rule, file, version and reason), and
     keep the backlog entry as a column or in the reason. As built: in
     the reason, first (`B40: …`), since the format has six fields and
     no entry.
   - **Annex E's name classes**, which `private.rs`'s test uses: from
     pdf-lint if it exposes them, else a few lines in that test. As
     built: `pdf_lint::Class`, a dev-dependency of sheaf-doc.
5. **What goes:**
   - `crates/sheaf-doc/src/arlington/` whole, and the `fixtures`
     feature's re-export of it;
   - `vendor/arlington-pdf-model`, and its mentions in `about.toml`,
     `deny.toml` and the licence notes, if any. As built: neither file
     exists; its mentions were `.gitattributes`' rule and README's
     licence note;
   - their public names, retired in `crates/sheaf-doc/tests/retired.rs`
     as earlier items did.
6. **Findings that change:**
   - pdf-lint's corrections and readers may find less (B45's `/DA`) or
     more (a key written twice, an xref row's width).
   - Each new finding is fixed at its cause in sheaf's writer when it is
     small. Otherwise it is logged as a backlog entry and allowed for it.
     It is never allowed without an entry.
   - An allowance no longer needed goes. Its backlog entry closes when
     pdf-lint's correction is what fixed it, as B45's likely is. B40 and
     B46 are checked against pdf-lint's model and `beyond`.
7. **CI.** Add `sheaf-pdf/setup-pdf-lint@v1` to `ci.yml`'s test job on
   all three platforms, so the Arlington tests that run there decode
   packed and encrypted outputs everywhere. Whether CI also runs the
   `pdf-lint` command line on a few files sheaf writes (`sheaf edit -o`)
   is the plan's call: do it only if it checks something the tests
   don't. As built: not run. It checks nothing the tests don't, and it
   can't hold what a save wrote, carry a source's faults or allow.

## Slices

| Slice | Status | Depends on |
|---|---|---|
| 1. The tests on pdf-lint; sheaf's checker and the vendored model gone | done | — |

### Slice 1: The tests on pdf-lint; sheaf's checker and the vendored model gone

**Builds:**
- **First, the baseline:** the default Arlington tests' printed counts,
  and `just tier-arlington` (tables checked, findings, carried, allowed,
  per form), recorded before anything changes. As built: the tier
  printed totals only, so `SHEAF_ARLINGTON_SAVES` writes a line a save
  first, on sheaf's checker.
- **`pdf-lint = "=0.1.0"`** as a dev-dependency of sheaf-ops, and of
  sheaf-doc if `private.rs`'s test keeps a use.
- **The hold, the allowances and the check helpers moved** onto
  pdf-lint's types (decision 4): `arlington.rs` reads each file with
  pdf-lint's reader and qpdf decoder, holds what the save record says,
  sets it beside the files read (`beyond`), and applies the allowances.
- **Every test in `arlington.rs` kept,** each still naming the tables it
  must have checked, and still checking `ObjectStream` exactly when a
  save packs. The corpus tier runs on every form, and its counts are
  asserted as before (`FORMS`).

  As built: pdf-lint 0.1.0 never checks object streams (its walk leaves
  them out), so `lint::check` holds each one the hold calls built to
  `ObjectStream` beside it. A field merged with its widget is checked as
  both tables, pdf-lint's default, where sheaf's checker took one.
- **The checker and the model deleted** (decision 5). rg finds no
  `sheaf_doc::arlington` and no `arlington-pdf-model` outside history.
- **CI** (decision 7): `setup-pdf-lint` on ubuntu, macOS and Windows,
  `PDF_LINT_QPDF` set from it.
- **Docs:**
  - `docs/oracles.md`'s Arlington row becomes pdf-lint;
  - `docs/building.md`, if the tests' qpdf needs saying;
  - `plan/libraries.md` lists pdf-lint as published;
  - the between card and ROADMAP;
  - the backlog entries whose `Where` names sheaf's checker;
  - `justfile`'s `tier-arlington` comment.

**Checks:**
- Every Arlington test passes on pdf-lint. Each table it named is still
  checked. The corpus tier's form and save counts hold.
- For each difference from the baseline, the notes list the finding,
  why (a correction, the bytes reader, a reading difference) and what
  was done: fixed, allowed for a new entry, or an allowance and entry
  closed.
- The allowance tests pass: every row needed, every entry open.
- Without qpdf, a test that needs it fails, saying so. Shown once by hand
  with `PDF_LINT_QPDF` pointing nowhere, then put back.
- fmt, clippy and the workspace tests pass, and so do the non-Linux
  clippy simulation and `just plan-check`. CI passes on all three
  platforms.

## Not in this item

- Changes to pdf-lint itself. A gap or a bug in pdf-lint found here is
  written up in the notes for Greg (it's his to change upstream), and
  worked around in sheaf only if it's small.
- Checking more of what sheaf writes than the tests check today. Read
  as what's held, not how strictly it's checked: pdf-lint's stricter
  checks of what is held, two tables among them, are taken, as Why
  welcomes.

## Slice notes

(Each slice adds its notes here: what was built, deviations and why,
measurements, hand checks for Greg, what review found, Found.)

1. Done. sheaf's Arlington tests check what sheaf writes with pdf-lint
   0.1.0, a dev-dependency: each file is read from its bytes, and qpdf
   decodes what's packed or encrypted. sheaf's checker and the vendored
   model are gone, and CI installs pdf-lint's qpdf on all three
   platforms. Every test still checks the tables it names, and
   `ObjectStream` exactly where it did. The tier's form and save counts
   hold. In the tier, pdf-lint carries 120,775 of the forms' faults
   where sheaf's checker carried 18,669. Its corrections closed B45, and
   it found B63 (its own bug) and two damaged forms (F29).
   - **The dependency:** `pdf-lint = "=0.1.0"` in the workspace, a
     dev-dependency of sheaf-ops and of sheaf-doc (`private.rs`'s test
     asserts sheaf's private names are `pdf_lint::Class::Second`).
     Cargo.lock gains pdf-lint and pdf-tokenizer 0.1.0.
   - **`crates/sheaf-ops/tests/lint/`,** a folder like `tests/common/`:
     - `read(what, bytes, password)`: `pdf_lint::bytes::read_with` and
       a private `Decoder` that mirrors the binary's. qpdf runs only
       when the reader asks for it. The first ask checks `Qpdf::ready()`
       once a process, and fails the test with what needed qpdf and how
       to get one. The bytes go to a numbered file in one scratch
       directory a process (`common::scratch`, which recreates its
       directory on every call), removed when the decoder drops. A file
       all in plain view, as lopdf's fixtures are, runs no qpdf.
     - `options()`: deprecations are findings, as they were in sheaf's
       checker; pdf-lint's default only counts them.
     - `check(doc, hold, options)`: `pdf_lint::check`, then a stand-in
       for what pdf-lint leaves out. Each `/Type /ObjStm` the hold calls
       built is held to every cell of `ObjectStream` through pdf-lint's
       public predicates (unlisted, missing, too-new, deprecated, type,
       indirect, direct, value, special, duplicate, unresolved), and so
       is what it links to, as sheaf's checker walked it: an array's
       elements, a dictionary or stream, decode parameters by the filter
       they pair with, and a reference to an object held as built, once
       (no longer unreached), to 8 links. All counted as pdf-lint
       counts. No caller can check without it.
     - `written(map, st, store, pdf, appended)`: `Hold::written`, ported
       whole as a free function on sheaf-doc's public paths, with
       private copies of `object_header` and `each_reference`.
     - `own_structure(report, from)`: the reader's findings of no object
       or the trailer in bytes sheaf wrote (an Append's from its
       section) held as built, so a source's like fault can't carry
       them. pdf-lint holds them as merged.
     - Tests: `lint::tests::an_object_streams_faults_are_found` (13
       faults in four hand-built objects, every rule the table can
       raise and what it links to; held as merged, not checked),
       `an_object_stream_before_1_5_is_too_new`,
       `what_the_version_deprecates_is_a_finding` (with `options()`, not
       pdf-lint's defaults), `the_object_streams_a_rewrite_packs_hold`,
       and the hold's four (`lint::hold::tests`), rebuilt on public
       fixtures with every assertion kept.
   - **`arlington.rs`:** `checked`, `save_checked`, `last_saved`,
     `exported` and `summary_held` read with `lint::read`, hold with
     `lint::written` (a lopdf `LazyPdf` still gives it the sections and
     ids) and check with `lint::check`. `files_read` checks each file
     the store read as merged, at the written file's version
     (`pdf_lint::Version`). `answered` (`Report::beyond`) and the
     allowances apply as before. All 38 tests are kept, with their tables
     and `packs`; `merged_fields` asserts its two-table count (12, 8).
     B46's pin (`a_merged_objects_finding_is_carried_from_any_object_of_a_source`)
     moved here from the checker's tests, on `answered`, which B46's fix
     changes. `a_fault_in_the_structure_sheaf_writes_is_never_carried`
     cuts xref rows to 19 bytes in a form and in what sheaf wrote.
   - **Allowances:** `arlington_allowed.tsv` is in pdf-lint's six
     columns, each reason starting with its entry (`B40: …`).
     `every_allowance_names_an_open_backlog_entry` fails a reason that
     doesn't. 29 rows became 34: B45's and 13 of B44's went; B63's 17
     and F29's 2 came.
   - **The tier** prints, beside its old counts, the keys deprecated and
     those readers ignore (by table and key, the 20 largest) and the
     objects checked as two tables. `SHEAF_ARLINGTON_SAVES` writes a TSV
     line a save: file, save, objects, keys, findings, carried, allowed,
     left, then `table=n` pairs.
   - **Deleted:** `crates/sheaf-doc/src/arlington/` (10 files, 7,604
     lines) and the `fixtures` feature's re-export of it;
     `vendor/arlington-pdf-model` (616 files, 4,845 lines), its
     `.gitattributes` rule and README's licence lines; `export.rs`'s
     link to `Hold::written`. rg finds no `sheaf_doc::arlington` or
     `arlington-pdf-model` outside `plan/`.
   - **Retired** (`retired.rs`'s `RETIRED_PDF_LINT`): `arlington`,
     `arlington::check_at`, `arlington::DIR` and
     `arlington::Hold::written`. pdf-lint reuses the module's other
     names (`Hold`, `Report`, `check` and the rest), which a bare last
     segment would find in the tests' own uses; `written` is
     qualified-only, so `lint::written` doesn't hit. Probed: the old
     uses hit, the new ones don't.
   - **CI:** `sheaf-pdf/setup-pdf-lint@v1` (`version: 0.1.0`) in the
     matrix job before Format, and `PDF_LINT_QPDF` set to its qpdf
     (`.exe` on Windows). The three archives' checksums and layouts
     were checked, and the default tests pass on the Linux one's qpdf.
   - **Docs:** `docs/oracles.md` (the Arlington row is pdf-lint; the qpdf
     row names it), `docs/building.md` (the tests need qpdf 11.9: apt's
     from Debian 13 and Ubuntu 24.04, else pdf-lint's), `just doctor`
     (that qpdf's version), the justfile's `tier-arlington`, `plan/libraries.md`, build-or-adopt
     94, README's licence note, B40, B44 and B46.

   Deviations from the plan (its text above now says what was built):
   - **`ObjectStream` by a stand-in.** pdf-lint's `left_out` skips
     `/Type /ObjStm`, and nothing else reaches it. The stand-in reads the
     table's cells, not a list of keys, so it holds more than sheaf's
     checker did (SinceVersion, duplicates). It reads an
     IndirectReference of FALSE as asking nothing, as both checkers do.
     fn:NoCycle on `/Extends` can't be told and is counted as not
     checked; sheaf writes no `/Extends`. Of several linked tables it
     takes the filter's, not TestGrammar's scores (pdf-lint's are
     private); one it can't tell is counted as not checked.
   - **Structure findings held as built:** `own_structure` (above).
   - **Two tables** (taken in the plan's review): a field merged with
     its widget is checked as both, pdf-lint's default, where sheaf's
     checker took one, as TestGrammar does. "Not in this item: checking
     more" was read as what's held, not how strictly. `one_table` was
     the fallback had a named table stopped being checked; none did.
   - **The entry in the reason:** `Allowance::parse` takes six fields.
   - **sheaf's own decoder:** the library has none that says qpdf was
     needed and isn't ready (the binary's `Decoder` is private), and
     `Qpdf::new` takes a path, so in-memory bytes go through a scratch
     file. sheaf-ops' clippy.toml disallows `std::env::temp_dir`.
   - **No `about.toml` or `deny.toml`** to change.
   - **The per-save dump,** added for the baseline "per form".
   - **Decision 7's command line** isn't run in CI (above).
   - **The damaged forms are F29,** a research entry under "broken files
     aren't targets" (F27's precedent), not a bug.
   - **sheaf-doc's checker tests went with it:** 56 (check_tests 29,
     predicate_tests 18, tests 5, hold_tests 4). 46 of those names are in
     pdf-lint 0.1.0's suite, B46's pin among them. Of the other 10, the
     hold's 4 moved here, the 2 object-stream tests are the stand-in's,
     the allowance format's is `every_allowance_names_an_open_backlog_entry`,
     deprecations as findings is `what_the_version_deprecates_is_a_finding`,
     arithmetic is pdf-lint's, and 1 tested the vendored commit.

   Measured (qpdf 12.3.2; test profile):
   - **Default tests:** 48 passed, 1 ignored (38 before, plus the lint
     module's 4, the hold's 4, B46's pin and the structure test); 1.5 s
     → 2.2 s wall, as
     qpdf now decodes packed and encrypted outputs. Their 55 printouts,
     compared one by one (object numbers and dates set aside):
     - checks not made fell to 0 in 51, since pdf-lint evaluates `mod`,
       fn:InNameTree, fn:RequiredValue and others. Left: `@Filter` in an
       image stamp's soft mask, and `StdCF::@AuthEvent` in the AES
       Rewrites;
     - two tables: Combine's 4 fields, each encrypted Rewrite's 1,
       merged_fields' 12 (Rewrite) and 8 (Append), whose `/Opt` arrays
       are now walked. The widget's DA, FT, V and Opt are no longer
       unlisted;
     - a count of keys readers ignore, in the AES Rewrites;
     - one new finding, carried: the AES-256 Rewrites' merged `/Encrypt`
       has R 6 at PDF 1.7, which the model allows from 2.0. The source
       has it too.
     - No reader, duplicate or deprecated finding, nothing left, no
       allowance needed.
   - **The tier** (`just tier-arlington`): 93.1 s → 77.5 s wall. `FORMS`
     held (4,389 of 4,392, 3 unread; Append 2,406, Rewrite 1,983, a page
     deleted 2,901, a page to a copy 1,488). A rerun matched the first
     run save for save, but for the saves B63's and F29's rows allowed.
     After review (the walk, `own_structure`), every count the same
     (76 s); the 6 reader findings carried are the forms' own.

     | | sheaf's checker | pdf-lint |
     |---|---|---|
     | objects (tables) | 2,092,060 (129) | 2,266,473 (131), 431,267 as two |
     | `ObjectStream` | 24,696 | 24,696 |
     | keys | 11,486,688 | 12,859,831 |
     | checks not made | 154,799 | 1,335 |
     | keys not listed | 1,190,103 | 102,372 |
     | deprecated; readers ignore | (not counted) | 103,871; 1,523 |
     | two contexts; ties | 2,373; 368 | 693; 398 |
     | carried; allowed; left | 18,669; 1,287; 0 | 120,775; 1,402; 0 |
   - **Suites:** the workspace 1,940 passed, 69 ignored (object streams
     left 1,986: 56 went, 10 came); materialize_corpus 6 passed, its
     counts the baseline's exactly (identity 4,389 of 4,392, 919,465
     copied, 432 changed; 4,389 × 4, 0 differ; 2,901 forms, 551
     findings; 1,098; logo 0 bytes); fmt, clippy, the non-Linux
     simulation, Windows clippy and plan-check clean.
   - **No gate:** only test code changed, no writer, so `gate_7k.rs` is
     unchanged.

   Every difference from the baseline in the tier. 5,574 of 8,778 saves
   moved, in 3,894 forms, each for a cause below. No save lost a table
   but `Encoding` (now `EncodingMap`), and no `ObjectStream` count moved.
   - **Two tables** (5,377 saves): the field tables rise (FieldTx 2,825
     → 251,494), with what their keys lead to (AddActionFormField, and
     Resource and FontMap through `/DR`), and the widget's FT, DA, Q and
     V are no longer unlisted. Carried: the fields' own `/Ff` bits (+954
     special), FieldTx `/DA` and `/FT` (+170 missing), `/V` and `/DA`
     types (+68),
     FieldTx `/DS` (+5 too-new). None done: they're the forms'.
   - **pdf-lint's corrections:** FieldBtn* `/DA` not required, so 72
     carried and B45's 2 allowed findings went: B45's row went and
     [B45 closed](backlog-closed.md#b45-arlington-requires-da-on-button-fields).
     FieldTx and FieldChoice `/DR` (deprecated in 1.5): +52,998 carried.
     Resource `/Encoding` links EncodingMap: `Encoding` 2,082 → 10,
     EncodingMap 50,651, and with fields' `/DR` walked, `/Encoding`
     deprecated 2,104 → 50,702 carried.
   - **Predicates evaluated:** PageObject `/StructParents` +29 missing;
     the forms' AES-256 `/U` and `/O` +69 special each (127 bytes, the
     model asks 48) and `/R` 6 at 1.7 +73 value. What's still not
     checked: string-ascii on what qpdf decrypted (CIDSystemInfo, 638
     twice), `@AuthEvent` (58) and one `/StructParents`.
   - **Extensions honored** (180 saves, 98 forms): pdf-lint reads a
     catalog's `/Extensions` (ADBE level 3 and up, ESIC), so −96
     too-new (`/DSS` and its keys, `/Enforce`) and −811 value
     (PageObject `/Tabs /W`, encryption under ADBE level 8). qpdf
     confirmed that each of those forms declares them.
   - **The bytes reader,** carried: 98 keys written twice (three
     Rewrites, a widget the form wrote twice and sheaf copied as it was)
     and 6 reader findings (six Appends, the forms' own cross-reference
     warnings).
   - **Two contexts** (B44, 0117296): a key is unlisted only when no
     table that reaches the object lists it, so 13 of B44's 19 rows went;
     B44 stays open on 6.
   - **[B63](../backlog/B63.md), new, a pdf-lint bug:** `render` cuts a
     byte string at 80 bytes but a text string at 80 characters. In an
     encrypted file, the reader puts qpdf's plaintext in plain-view
     objects and tokenizes an object stream's strings as bytes, so a
     40-character UTF-16 `/DocumentID` shows two ways and doesn't match
     the form's when a save moves its page into or out of an object
     stream. 43 findings in 19 saves of 17 forms, held by 17 rows, one
     per form. Reproduced with qpdf and the `pdf-lint` binary alone.
   - **[F29](../backlog/F29.md), new:** 1819139 and 4422502's
     `startxref` names nothing, so pdf-lint reads little of them, while
     sheaf opens them by lopdf's recovery: their faults in what sheaf
     merges (78 FieldTx `/DR` deprecated, 9 `/Tabs` too-new) aren't
     carried. Two rows, one per form, the counts in their reasons.
   - **[B40](../backlog/B40.md) and [B46](../backlog/B46.md) stay open,**
     rewritten: no correction in pdf-lint covers B40, and pdf-lint's
     `beyond` has B46's any-object rule (its `since` and `by_path` are
     ways to fix it).

   Hand checks for Greg:
   - CI green on ubuntu, macOS and Windows with setup-pdf-lint. The macOS
     and Windows qpdf couldn't be run here.
   - Done here: `PDF_LINT_QPDF=/nonexistent/qpdf cargo test -p sheaf-ops
     --test arlington encrypted_files_hold` fails both its tests with
     "Rewrite needs qpdf to decode what's compressed or encrypted in it,
     and /nonexistent/qpdf isn't found: install qpdf 11.9 or later, or
     name one in PDF_LINT_QPDF" (and "Append needs …").
     `a_pdf_1_3_file_holds` passes with it set (a table file, nothing
     packed), and both pass again without it.

   Review (the plan's) found, all taken into the slice above: object
   streams never checked; no `check_at` (the version is in `Options`);
   `Qpdf` wants a path and no library decoder fails loudly;
   `common::scratch` recreating its directory; deprecations only counted
   and fields as two tables by default; reader warnings held as merged;
   the two new rules in the tier's counts; the hold's helpers private to
   the module deleted; the six-field allowance; no `about.toml` or
   `deny.toml`; B46's pin; the names pdf-lint reuses; the per-save dump.

   Found, for Greg (pdf-lint and setup-pdf-lint are his):
   - pdf-lint 0.1.0 never holds object streams to `ObjectStream`; the
     stand-in covers it until it does.
   - No library decoder says qpdf was needed and isn't ready, and
     `Qpdf::new` needs a file, so in-memory bytes go through one.
   - A reader warning with no object is held as merged (`made(None)`),
     so a Rewrite's own file-level warnings (cross-reference rows,
     `%%EOF`) would be carried when a source has a like one. Worked
     around here (`own_structure`).
   - B40 is a candidate for pdf-lint's corrections; B63 is its bug.
   - `pdf-lint --version | head -1` panics on the closed pipe.
   - The AES-256 fixture (`serialize::fixture`) is 1.7 with R 6, as
     Acrobat writes it under ADBE level 8, which the model has no
     extension for.
   - sheaf copies a primary's object nothing changed as it was
     (`SymObject::Raw`), so a key a form wrote twice stays twice.
   - The Extract's and the 2.0 Append's object numbers differ from one
     test process to the next (37 0 R, 41 0 R); within a process they
     are deterministic.
   - `ci.yml`'s paths filter still names `!todos/**` (gone since #53),
     and its `!*.md` matches only the root's, so a change to `plan/`
     alone runs CI: [B64](../backlog/B64.md).

   Review (the slice's) found, all fixed:
   - B46's pin tested pdf-lint's `beyond`, which no fix changes: the
     rule is now `answered`, and the pin tests it.
   - The stand-in followed only a direct array, where sheaf's checker
     walked every link: it walks them now (above), and the test holds
     every rule the table raises (Type, the old test's, among them).
   - File-level reader findings in sheaf's bytes carried from a like
     source's: `own_structure`, and a test that fails without it.
   - No test pinned deprecations as findings or the two tables: one
     each.
   - `docs/building.md` promised apt's qpdf would do; `just doctor`
     didn't check its version.
   - The plan stayed in `plan/between/` done: moved to `plan/complete/`.

   Left for later: none, as this is the item's only slice. The work
   between milestones pauses until Greg says.
