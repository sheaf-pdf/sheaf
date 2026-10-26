# Object streams, and B3

Status: done

The second item [between milestones](../between/README.md) (Greg, 2026-10-25:
"start object streams and B3"). Two slices, one PR. The linearization
crate (item 3) builds on slice 2's grouping.

## Why

A Rewrite writes no object streams. It re-serializes each object-stream
member as a plain object ([M10.5 §11.6](m10.5-edit-api/design/11-materialization.md#116-numbering-raw-passthrough-object-streams-the-page-tree)),
and it writes its cross-reference stream uncompressed (`serialize/xref.rs`).
Measured with qpdf on the corpus (2026-10-23), unpacking object streams
grows the IRS forms by a median 88%: all 70 use them, and `fw4.pdf`
grows by 178% (against qpdf's own `--object-streams=preserve` rewrite;
against the source, 74% and 155%, and sheaf's Rewrite 50% and 123%:
slice 1's notes). A sample of 377 SafeDocs files grows by 15%. Every Save As
to a new unsigned file pays this, and so does every save that removes
content or replaces text, and every export. An Append writes only what
changed, so it doesn't pay it.

[B3](backlog-closed.md#b3-properties-says-fast-web-view-yes-after-an-incremental-save):
Properties said "Fast Web View: Yes" after an incremental save, which
voided the linearization.

## Decided

Greg's standing rule applies: an open question with a recommended answer
is taken, and noted.

1. **Which files are packed.** A Rewrite or export packs when its source
   used object streams: a base whose cross-reference section is a stream,
   or a hybrid-reference file. A hybrid's output becomes a plain
   cross-reference-stream file; its table was only there for readers older
   than PDF 1.5. A classic-table base stays a table, unpacked. PDF/A-1
   forbids both object streams and cross-reference streams, and a table
   file is never made bigger by being left a table. Packing every file
   regardless of its source is Optimize's job (M14).
2. **What is packed.** Every object the writer writes anew
   (`Body::Object`) goes in a container if it is packable. An object
   copied raw as its source has it (`Body::Raw`, §11.6) stays where it is.
   The source's container members are never raw-copied, so they are
   always packed again. Plain objects the source wrote stay plain, and
   the 7k Rewrite keeps its memcpy path.
3. **Packable** (`packable`, ISO 32000-1 7.5.7, M10.5 §20). These are
   never packed:
   - streams;
   - an object whose generation isn't 0;
   - the `/Encrypt` dictionary;
   - a container's own `/Length`;
   - cross-reference streams;
   - the linearization dictionary;
   - signature dictionaries (`/Type /Sig`, or a dictionary with
     `/ByteRange` and `/Contents`), so a later signing tool finds them
     where it expects.
4. **Encryption.** Each container is encrypted once, as a stream, under
   its own number. Strings inside its members are never encrypted
   separately. Like every other AES object, the container's IVs are keyed
   hashes of what it holds, so the same document writes the same bytes.
5. **Grouping by the pages that use them**, so the linearization crate
   can reuse it:
   - Members are ordered in parts, as Annex F lays out a linearized file:
     1. objects only page 1 uses;
     2. objects that a single later page uses, in page order;
     3. objects several pages share;
     4. the rest (the catalog, the page tree, the AcroForm, outlines,
        names, anything no page reaches).
   - Containers are filled in that order, up to 100 members each (qpdf's
     default), and a container never spans the page-1 part's boundary.
   - The grouping is a pure function from (object, the pages that use
     it) to containers, with no lopdf or materializer types in its
     signature, so item 3 can move it into its crate.
6. **Numbers.** A container gets a number the way a stable save numbers
   anything new.
   - Numbers the source's own containers had, now free, are reused first,
     in order. Then numbers past `/Size` are used.
   - Saving the same document again must not grow `/Size` (a test).
   - Dense numbering (exports) numbers containers after the objects.
7. **The cross-reference stream is compressed**: Flate with the PNG Up
   predictor (`/Predictor 12`, `/Columns` the sum of `/W`), `/W` as narrow
   as the largest offset needs, and type-2 entries for members. qpdf,
   PDFium, pdf.js and sheaf's own reader must read it back.
8. **Append is unchanged.** An update stays plain objects in the base's
   kind. Its few objects gain little from packing, and a plain update is
   what signature validators read most easily. Logged in the backlog for
   later; not this item.
9. **B3's check is Annex F's minimum**: a file counts as linearized when
   its first object is a dictionary whose `/Linearized` is 1 (Annex F's
   version: a number whose floor is 1, as qpdf reads it) and its `/L`
   equals the file's length counted from the header
   (`XrefFacts::header_offset`). As built, the dictionary must end
   within the first 1,024 bytes too, as Annex F has it (slice 1's
   notes). An
   Append's output record says not linearized. The full check (hint
   tables, the first page's section) is the linearization crate's
   checker, item 3.

## Slices

| Slice | Status | Depends on |
|---|---|---|
| 1. Rewrite's size, measured; B3 | done | — |
| 2. Object streams on Rewrite and exports, grouped by page | done | 1 |

### Slice 1: Rewrite's size, measured; B3

**Builds:**
- **A size tier**, `crates/sheaf-ops/tests/rewrite_size.rs`, `#[ignore]`,
  and a justfile recipe `tier-size` that `premerge` runs:
  - It covers every IRS form (`forms/irs/`, 70 files) and every file in
    `bench/pool-sample.txt` (402; as built: only those that use object
    streams would leave the "without" group empty).
  - Each file gets an unsigned Save As through `Session`, the path a user
    takes.
  - It reports the output's size against the source's: the median, the
    90th percentile and the worst file, by group (IRS, pool), and
    separately for sources with and without object streams (decision
    1's: a stream or hybrid base, against a table).
  - Slice 1 asserts nothing about size. It records the baseline. It does
    assert that every output reopens in sheaf, and passes `qpdf --check`
    when qpdf is on PATH (skipped, and said, when it isn't). As built:
    no worse than its source (exit 0 where the source's is, 3 only where
    the source warns or fails, never 2 or a signal), since 166 pool
    sources warn already.
- **The baseline in the notes**, beside qpdf's numbers on the same files
  as a reference: `qpdf --object-streams=disable` (unpacked) and
  `--object-streams=generate` (repacked).
- **B3**:
  - Fix `vendor/lopdf/src/lazy.rs`'s `linearized` as decision 9 says:
    `/L` against the length from the header.
  - Make `crates/sheaf-doc/src/materialize/append.rs`'s output record say
    `linearized: false`.
  - Test it on a fixture linearized and then appended to, and on a
    linearized file from the pool sample, saved through sheaf. Properties
    then says No, and `qpdf --check-linearization` agrees. As built:
    - the pool files take a forced Append (`save_appended`), since a
      planned save asks on signed files and rewrites others;
    - qpdf agrees when it exits 0 or 3 without "is not linearized"; any
      other exit fails;
    - Properties' Fast Web View reads the saved record of the file the
      view shows, since a save never replaces the primary, and
      `Properties::linearized` (the source as opened) is retired;
    - the check refines decision 9: the dictionary is read whole within
      the first 1,024 bytes (Annex F), where qpdf asks only that the
      object start there, so a dictionary ending at byte 1,025 is
      linearized to qpdf and not to sheaf; item 3's checker keeps the
      rule or changes it on purpose;
    - without qpdf, the append tier checks sheaf's half alone.
  - Move B3 from `plan/backlog/` to `plan/complete/backlog-closed.md`.
- **realize_corpus's "linearized" row** now counts only files whose
  linearization still holds. Its per-row counts change, so record the new
  numbers.

**Checks:**
- The size tier runs on every file it picks. Its table goes in the notes.
- B3's tests pass. `qpdf --check-linearization` and sheaf agree on every
  linearized pool file, before and after an Append.
- fmt, clippy and the workspace tests pass, and `just plan-check` passes.

### Slice 2: Object streams on Rewrite and exports, grouped by page

**Builds:**
- **In `serialize/`**, a container writer: members as `n offset` pairs,
  `/N`, `/First`, Flate, `/Type /ObjStm`. Also:
  - `packable` (decision 3);
  - the grouping (decision 5);
  - type-2 cross-reference entries;
  - the compressed cross-reference stream (decision 7);
  - container encryption (decision 4).

  As built: `objstm.rs` (`packable(id, obj)`, since the writer sees
  lopdf's objects, not §20's `packable(ObjKey, &SymObject, gen)`; and
  `container`, which makes none whose data would pass the reader's
  256 MB `max_decompressed`), `group.rs` (`Use`, `group`, `MEMBERS`, no
  lopdf types), `Entry::Packed` (a table refuses one) and
  `ObjKey::Container(n)` (tag 22), which keys a container's IVs. A
  signature is told as lopdf's encryption walk tells one (`/Type`, else
  `/FT`, of `/Sig` or `/DocTimeStamp`; with neither, `/ByteRange` and
  `/Contents`), and so is an encryption dictionary (a `/Filter` name,
  `/O` and `/U`). Only a whole file's cross-reference stream is
  compressed: an update's isn't ([B62](../backlog/B62.md)).
- **`full::write_bodies` packs** when the layout asks for it (decision
  1):
  - Written objects that are packable go into containers.
  - Raw bodies stay plain.
  - The section lists members as type 2.
  - `left_out` still drops the source's containers.

  As built: `Layout.pack: Option<Pack { uses, reuse }>`, refused with a
  table. `uses` is a closure `write_bodies` runs on a thread of its own
  while it writes the rest; the containers are built on up to 8 threads,
  4 to a thread at least, and joined in order. Nothing is packed under
  an encryption whose `/StmF` is Identity and whose `/StrF` encrypts
  (`Crypt::packs`: the members' strings would be in the clear), nor when
  the cross-reference stream would be left no number a file may use.
- **The Rewrite path** (`materialize/mod.rs`) sets packing from the
  source (decision 1). It gives the writer each object's pages from the
  graph it already has, and numbers containers as decision 6 says. The
  header's version is at least 1.5 when it packs.

  As built: a source packs when its section is a stream or it's a
  hybrid (`XrefFacts::hybrid`: a trailer naming `/XRefStm`, a stale one
  too), and `Crypt::packs` holds. `graph::page_uses` walks from each
  page, each object at most twice, on its own stack. A stable save's
  containers take the source's `/ObjStm` numbers (from the eager load)
  first, but one the pruned graph still holds, so a reference to an old
  container keeps dangling. `/Size` holds across saves in a session;
  reopened, a Rewrite grows it by one, since `number::stable` reserves
  the primary's `/Size` and its cross-reference stream's number.
- **Exports** (dense numbering) pack under the same rule. Find where
  each export builds its `Layout`, `realize/assemble.rs` included, and
  say in the notes which ones pack and why.
- **The size tier asserts a bound.** For sources that used object
  streams, set it from what packing achieves. The target is median growth
  under 10% on the IRS forms, with no file more than 25% bigger. The
  numbers and the reason go in the notes. Files without object streams
  must not change size (decision 1).

  As built: the bounds, by nearest rank, are asserted when the whole
  tier ran, and each output's layout on every run (`laid_out`). "Must
  not change size": the 171 tables were diffed once against slice 1's
  rows, all the same size, and +0.5% is their asserted ceiling.
- **Docs:**
  - `docs/document-model.md`'s writer section;
  - the `serialize/mod.rs` and `full.rs` module docs, which say "no
    object streams" today;
  - the between card's item 2 status;
  - a backlog entry for packing on Append (decision 8).

  As built: also README, ROADMAP, the M10 card's status line, the parity
  checklist, the document-transforms memo, build-or-adopt entry 66,
  `docs/oracles.md` and the justfile. The entry is
  [B62](../backlog/B62.md), a feature under the B number it was given.

**Checks:**
- Unit tests in `serialize/tests.rs`:
  - every exclusion in decision 3;
  - a container's header and offsets;
  - type-2 entries read back by lopdf and by sheaf's lazy reader;
  - the grouping's part boundaries and the 100-member cap;
  - determinism: the same document writes the same bytes twice;
  - `/Size` stable across two saves.
- Encrypted Rewrites (RC4 40 and 128, AES-128, AES-256) with containers.
  sheaf, qpdf (`--check`, and `--decrypt` with the password) and PDFium
  read them back. An AES-256 xref-stream base whose strings live in
  object streams rewrites and reads back
  (`an_aes256_object_stream_base_rewrites`, which asserted no `/ObjStm`,
  now asserts object streams). As built: RC4-40 is
  `serialize::fixture::rc4_40`, chained before the other three.
- The materialize, append, realize, exports and encrypted corpus tiers
  hold, and so does the Arlington tier over what sheaf writes (its
  `ObjStm` and `XRefStream` rules).
- The interop tests read packed outputs with qpdf, PDFium and pdf.js.
- The 7k gate: the heavy fixture's Rewrite stays within 1.3× slice 8's
  writer and is flagged past 1.2×, as M10.5 set it (it has 30,317
  objects in 304 containers). Report the time with and without packing.
  As built: nothing writes unpacked any more, so "without" is a
  constant, 3cbb0a3's median, printed with no gate.
- fmt, clippy and the workspace tests pass, and `just plan-check` passes.

## Not in this item

- Packing on Append (decision 8; logged in the backlog).
- Packing classic-table files, or recompressing streams: Optimize, M14.
- Linearized output and its checker: item 3, the linearization crate.

## Slice notes

(Each slice adds its notes here: what was built, deviations and why,
measurements, hand checks for Greg, what review found, Found.)

1. Done. B3 is fixed and
   [closed](backlog-closed.md#b3-properties-says-fast-web-view-yes-after-an-incremental-save),
   and an unsigned Save As's size is measured beside qpdf's.
   - **The check** (vendored lopdf, decision 9): `LazyPdf::open` parses
     the first object within the 1,024 bytes it already read
     (`holds_linearization`); `XrefFacts::linearized` holds for a
     dictionary, whole within those bytes, whose `/Linearized` is 1 (a
     number whose floor is 1) and whose direct integer `/L` is the
     length from the header. It is the only change on a gated path, a
     parse of bytes already read: slice 1 owns no gate, and `gate_7k.rs`
     is unchanged.
   - **The record:** an Append's output record says `linearized: false`.
   - **Properties** (sheaf-app): `PropertiesContext::linearized` is the
     saved record's when its path is the view's file, else the primary's.
     sheaf-doc's `Properties::linearized`, the source as opened, which
     nothing showed, is retired (`retired.rs`'s new
     `RETIRED_OBJECT_STREAMS`).
   - **Tests:** sheaf-doc `lazy_tests::a_linearization_holds_while_its_length_is_the_file_s`
     (byte 1,024 both ways, `/Linearized` 1, 1.0 or 1.5 but not 0.9, 2,
     true, null or a name, junk before the header, `/L` off by one,
     missing, indirect or real, another object first, an update) and
     `append_tests::a_linearized_file_appended_to_is_linearized_no_more`;
     sheaf-app `properties_says_no_fast_web_view_once_a_save_appends`;
     interop's `a_linearized_file_appended_to_is_linearized_no_more`
     (`qpdf --linearize` on a sheaf Rewrite, then an Append and a
     Rewrite). Tiers: append_corpus's
     `linearization_agrees_with_qpdf_before_and_after_an_append`
     (`tier-append`) and `rewrite_size.rs` (`just tier-size`, in
     premerge). tests/common has `qpdf()` (from interop.rs),
     `qpdf_said`, `qpdf_linearized` and `pool_sample()`.

   Deviations from the plan (and the slice text's "As built"):
   - **The fixture is hand-written** (`serialize::fixture::linearized_file`,
     `linearized_file_after(lead)`): lopdf's writer drops any object
     with `/Linearized`. `/L` is padded, then patched; qpdf exits 3 on
     it (hint tables only). fuzz.rs mutates it too.
   - **All 402 pool files, decision 1's split:** with/without is the
     base's kind (stream or hybrid, against a table), not `/ObjStm` in
     the bytes, which is counted beside it. The sample's two
     `forms/safedocs/` paths are in the pool group.
   - **Asks are answered as they come,** each kind once. Medians and p90
     are by nearest rank, so each is a real file's.
   - **qpdf's rules** (`qpdf_linearized`, the `--check` rule), the forced
     Append and Properties' record: in the slice text.
   - **Stricter than qpdf on purpose:** sheaf wants the linearization
     dictionary to end within the first 1,024 bytes (Annex F: it is
     wholly contained in them); qpdf 12.3.2 asks only that the object
     start there. A dictionary ending at byte 1,025 is linearized to
     qpdf and not to sheaf (seen by hand; `lazy_tests`'s case says so).
     They agree on the whole pool sample, which has no such file. The
     slice text names it as decision 9's refinement, for item 3.

   Measured (qpdf 12.3.2; test profile):
   - **Size** (`tier-size`, 29 s), growth over the source's bytes,
     median / p90 / worst; qpdf at its defaults, which recompress
     streams:

     | Files | sheaf | qpdf unpacked | qpdf repacked |
     |---|---|---|---|
     | IRS, 70 (all streams) | +49.6 / +113.9 / +132.5% | +73.6 / +141.9 / +188.7% | −7.1 / −3.3 / −1.3% |
     | pool, 402 | +0.0 / +6.9 / +114.4% | −0.0 / +7.3 / +123.7% | −2.0 / −0.1 / +5.9% |
     | pool, used (209 stream, 22 hybrid) | +1.1 / +11.7 / +114.4% | +0.8 / +13.3 / +123.7% | −1.4 / −0.0 / +5.9% |
     | pool, didn't (171 tables) | −0.1 / −0.0 / +0.4% | −0.5 / +0.1 / +3.0% | −2.8 / −0.4 / −0.1% |

     The IRS worst is f941sb; fw4 is +122.6% (qpdf unpacked +155.4%).
     The Why's 88% and 178% are qpdf unpacked over its own
     `--object-streams=preserve` (+87.9%, +177.9% here). sheaf is under
     qpdf unpacked because its xref stream (22 KB on fw4) beats qpdf's
     table (71 KB) and its objects are unspaced (287 KB to 347 KB),
     though it copies streams raw (155 KB) where qpdf recompresses them
     (114 KB). Raw copying is why the pool's worst, 6335597, is +114% to
     qpdf's +85% (3.8 MB of streams, 1.7 MB recompressed): Optimize, M14.
   - **The size tier's checks:** 134 signature asks (pool only), 0 XFA,
     so signed sources are measured with their signatures broken.
     `qpdf --check`, source → output: 0→0 on 306, 3→0 on 150 (141 of
     them the linearized sources), 3→3 on 16 (each output's warnings are
     its source's: name trees, a corrupt JPEG, bad zlib), never 2. Every
     output reopens with its pages; none fails to open; qpdf wrote all.
   - **Linearization** (4 s): sheaf and qpdf both find 150 of the 402
     linearized, file by file, then 0 after 402 Appends; none stopped.
     The other 66 naming `/Linearized` early were updated after it (`/L`
     short; 65 have three or more `%%EOF`). With the old search put
     back, it failed on 216.
   - **realize_corpus's predicate rows:** linearized 2,523 files (4 of
     111,825 pages disagree) → 1,791 (0 of 75,875); every other row
     unchanged (all: 4,932, 4 of 242,055). It fails on B4 alone (0874785
     page 0), before and after.
   - **materialize_corpus:** 6 passed, the counts identical before and
     after (identity 4,389 of 4,392, 919,465 copied, 432 changed; 4,389
     × 4, 0 differ; 2,901 forms, 551 findings; 1,098; logo 0 bytes).
   - **Suites:** the workspace 1,955 passed, 68 ignored; interop with
     qpdf and pdf.js 19 passed; `just fuzz lazy_reader 60` twice, clean.

   Hand checks for Greg: open `pool/safedocs/7084/7084596.pdf`, File >
   Properties says Fast Web View: Yes; add a mark, save (an Append), and
   it says No without a reopen; `qpdf --check-linearization` on the
   saved file says it is not linearized.

   Review (the plan's) found: the dialog reads the primary, which a save
   never replaces, so the record alone left it saying Yes; a pool tier of
   object-stream files only empties the "without" group; qpdf's exit 3,
   and exit 0 for "is not linearized"; B3's links that plan-check follows.

   Review (the code's) found, all fixed:
   - `/Linearized` of any value counted (`true`, 2, null): now 1 as qpdf
     reads it (a number whose floor is 1), checked by hand against qpdf
     12.3.2 (1, 1.0 and 1.5 linearized; 0.9, 2, true, null and a name
     not);
   - `Properties::linearized` was read by nothing the user sees, and
     three tests checked it equal to the xref fact it was copied from:
     retired, and the tests say they check the cross-reference facts;
   - the append tier returned before its sheaf checks without qpdf: qpdf
     is optional there now, as in the size tier;
   - the tier's baseline was skipped silently if the pool sample changed
     length: the sample's length is asserted;
   - the 1,025-byte disagreement with qpdf was unrecorded: above, in B3's
     closed entry and in the between card's checker bullet;
   - the slice text's `qpdf --check` rule didn't match `no_worse`
     (3 where the source warns *or fails*): the text now says what the
     code does, since an output that only warns where its source failed
     is no worse.

   Found:
   - **Properties' other rows describe the file as opened:** after a save,
     File size, the dates, producer, version, pages, fonts and security
     still read the opened Source. Logged as [B61](../backlog/B61.md).

   Left for slice 2:
   - The bound goes in `rewrite_size.rs`, over its decision-1 groups,
     against the source's bytes; its per-file rows (`target/just/tier-size.log`
     after premerge, or `just tier-size`) are the baseline.
   - IRS has no "didn't" group. The pool's 171 tables move −0.1% median,
     +0.4% worst today, so "must not change size" needs a tolerance, or
     byte equality checked another way.

2. Done. A Rewrite and every export of a file that used object streams
   pack what they write anew into object streams again, grouped by the
   pages that use it, each container encrypted once; a whole file's
   cross-reference stream is deflated under the Up predictor. A table
   file stays a table, unpacked, and an Append is unchanged
   ([B62](../backlog/B62.md)). The IRS forms now shrink by a median 12.9%
   on an unsigned Save As (slice 1: +49.6%), and the heavy 7k's Rewrite
   takes 808 ms, 1.14× slice 8's writer.
   - **`serialize/`:** `objstm.rs` (`is_container`, `packable`,
     `MAX_DATA`, `fits`, `container`); `group.rs` (`Use`, `MEMBERS`, `group`: page 1's part
     cut apart, then each later page's in page order, the shared, the
     rest, 100 to a container); `xref.rs`'s `Entry::Packed` and a full
     section's stream deflated under `/Predictor 12` (`up`); `crypt.rs`'s
     `Crypt::packs`; and `full.rs`'s `Layout.pack`, `Pack`, `packed`,
     `numbers` and `containers`. `serialize::deflated` deflates the
     containers and the cross-reference stream at zlib's default level
     (images.rs keeps its own).
   - **`materialize/`:** `ObjKey::Container(n)` (tag 22);
     `graph::page_uses`; `TreeShape::pages`; `append::refs` is
     `pub(crate)`; `rewrite()` decides packing, the numbers to reuse and
     the page walk.
   - **Which writers pack.** `materialize::rewrite` is every Rewrite:
     a save's, Save As, `sheaf edit -o`, each export preset (Extract and
     split, print with and without markup, Clean Copy, Export Flattened,
     rejects) and recovery's whole sidecar. It packs by decision 1, an
     export numbering its containers after its objects (asserted by
     `exports_of_a_stream_file_pack_and_of_a_table_don_t`).
     `realize/assemble.rs`'s standalone never packs: it stays in memory,
     unencrypted, read by PDFium alone. `full::write_built` never packs:
     what it writes (blank pages, images to PDF, the stamp library, FDF,
     memory sources, `serialize/built.rs`'s sheets) never had object
     streams, and it writes a table. Combine and scans pack only from a
     stream or hybrid primary, as any Rewrite.
   - **Two speedups, the same bytes** (the gate, below): `object.rs`
     writes integers, references, names and hex strings byte by byte and
     a literal with nothing to escape as it is, a real alone through
     `write!` (`numbers_names_and_strings_write_as_formatting_would`
     checks every byte value and the integer edges against `format!`);
     and `Pack.uses` is a closure, so the page walk runs beside the
     bodies' loop. Both 7k outputs hashed the same before and after, and
     the size tier's 472 rows matched the run before them to the byte
     count.

   Deviations from the plan (and the slice text's "As built"):
   - **Decision 3's tests are lopdf's walk's,** so signature fields
     (`/FT /Sig`, no `/Type`) stay unpacked too, and an older
     revision's encryption dictionary.
   - **Decision 4, refined:** a file whose `/StmF` is Identity and whose
     `/StrF` encrypts isn't packed (`Crypt::packs`, in the writer, so no
     layout can put strings in the clear). An encrypted hybrid so
     encrypted rewrites as a table alone, its `/XRefStm` gone
     (`forms::fixture::hybrid_reference_encrypted`, in
     `identity_stmf_under_an_encrypting_strf_isn_t_packed`).
   - **Robustness, not in the plan:** a container past the reader's
     256 MB isn't made, its members written on their own, and with no
     number left below `NUMBERS` (8,388,607) for the cross-reference
     stream, nothing is packed. Numbering also passes over 0, members' numbers and repeats.
   - **A hybrid** is `XrefFacts::hybrid`, so a table whose trailer names
     a stale `/XRefStm` becomes a packed stream file too.
   - **materialize_corpus's identity check** (sheaf-ops) skips a
     source's object streams, whose numbers the Rewrite's containers now
     take, and fails if such a number holds anything else: it counted
     them as 18,489 objects "changed by keep_only".
   - **Packing is asserted by reading objects**
     (`serialize::fixture::object_streams_of`), not by `/ObjStm` in the
     bytes, which a string or name can hold; negative scans use
     `fixture::in_clear`, which inflates streams and looks for the text
     in hex too, as a string written `<…>` holds it.
   - **Arlington** holds every object no key names that reads as
     `/Type /ObjStm` as built, and checks it as ObjectStream from the
     section (path `xref`); sheaf-ops' `Checked.packs` asserts
     ObjectStream was checked exactly when a Rewrite or export packs (19
     tests), by the writer's rule: the file used object streams and the
     encryption written, if any, packs. No allowance was added.
   - **The size tier's bounds** are tighter than the plan's targets (IRS
     median ≤ −5%, worst ≤ +5%; pool median ≤ +1%, worst ≤ +5%; tables
     worst ≤ +0.5%), each commented with its reason.
   - **`a_container_past_what_the_reader_takes_is_written_plain`** was
     left out at first (4.0 s, before literals with nothing to escape
     were written as they are); measured again it takes 1.9 s (the
     out-of-numbers test 3.7 s), so it's in, unignored: a 256 MB literal
     member written on its own, its container's number left free, the
     other containers made.
   - **The gate's "without packing"** is a constant (3cbb0a3, median of
     5 processes), since a switch to write unpacked would be a shim.
   - **B62** is a feature under the B number it was given; F29 was next.

   Measured (release on the AMD EPYC 7302P for the gate; test profile
   otherwise; qpdf 12.3.2, pdf.js 6.4.299):
   - **The 7k gate** (`rewrites_on_the_synthetic_and_heavy_7k`, 5
     runs, each part in a process of its own):

     | | before packing (3cbb0a3) | packed | slice 8 |
     |---|---|---|---|
     | heavy, time | 774 ms | 808 ms (792–883) | 707 ms (1.14×) |
     | heavy, peak RSS / added | 474 / 253 MB | 478 / 256 MB | 468 / 251 MB |
     | heavy, file | 17.9 MB | 13.8 MB, 306 object streams | |
     | synthetic, time | 150 ms | 146 ms (140–150) | 134 ms (1.09×) |
     | synthetic, peak / added | 89.5 / 51.0 MB | 89.5 / 51.2 MB | 96.5 / 61.7 MB |

     An earlier 5 of the same writer gave 798 ms (791–806); the 883 was
     a busy moment, past the flag. Before the two speedups the heavy
     took 867–903 ms (1.23–1.28×). Timed once, packing cost: the page
     walk 33–40 ms (now beside the bodies), the containers 26 ms on 8
     threads (141 on one: 60 writing members, 80 deflating), the
     cross-reference stream's deflate 21 ms, grouping and numbering
     9 ms. `gates_on_the_heavy_7k` and `_synthetic_7k` pass once each
     (the heavy's Bates Rewrite 991 ms, flatten 1,185 ms, clean copy
     1,086 ms; proposal 8's accepted misses as before).
   - **Size** (`tier-size`, 472 of 472 files, 30 s), growth over the
     source:

     | Files | median / p90 / worst |
     |---|---|
     | IRS, 70 | −12.9 / −3.0 / −0.9% (fw2) |
     | pool, used object streams, 231 | −0.4 / −0.0 / +2.8% (2705083) |
     | pool, tables, 171 | −0.1 / −0.0 / +0.4% (7439653) |

     fw4 writes 190,669 bytes of 208,845 (slice 1: 464,884; qpdf's
     repack 188,577). Against slice 1's rows: all 171 tables the same
     size; all 301 that used object streams smaller, 1,106,261,752 bytes
     to 1,034,939,475 in all (their sources 1,073,395,899). The 301 (275
     streams holding `/ObjStm`, 4 without, 22 hybrids) write streams
     with 2 or more object streams, at 1.5 (20), 1.6 (88), 1.7 (192)
     and 2.0 (1); every table writes a table. `qpdf --check`, source →
     output: 0→0 on 306, 3→0 on 150, 3→3 on 16, as in slice 1.
   - **Slice 1's reading, corrected:** the IRS forms shrink because all
     70 are linearized and carry `/UR3` usage rights (a 13 KB
     signature), which a Rewrite drops; 6335597's +114% was the
     unpacking itself (it now shrinks 1.8%); the worst, 2705083, keeps
     its 12 signature dictionaries (13 KB each) on their own.
   - **Corpus:** materialize_corpus 6 passed, its counts slice 1's
     exactly (identity 4,389 of 4,392, 919,465 copied, 432 changed;
     4,389 × 4, 0 differ; 2,901 forms, 551 findings; 1,098; logo 0
     bytes). encrypted_corpus whole: 573 Rewrites kept; 572 Appends
     kept, 1 declined. Arlington on 10 forms: 171 object streams checked
     as ObjectStream, nothing failed. The full ignored tiers run at
     premerge.
   - **Interop** 21 passed: 60 packed outputs (every base of `form()`,
     RC4-40 among them, and `hybrid_reference`), `qpdf --check` on 60,
     `--decrypt` then `--check` on 24, pdf.js reading 60 alike and
     refusing the 24 encrypted ones without the password; PDFium in
     sheaf-render's `reopen_in_pdfium` and
     `an_aes256_object_stream_base_rewrites`.
   - **Suites:** the workspace 1,984 passed, 69 ignored (1,986 after
     the code's review); clippy, the non-Linux simulation and Windows
     clippy (sheaf-util, sheaf-doc, sheaf-ops) clean.

   Sites audited for scans packing would make vacuous (`holds` logged
   on `/ObjStm` bytes, suites run, then reverted): sheaf-doc's
   encrypted Rewrite tests, the field and removal vetoes and recovery's
   whole sidecar, and sheaf-render's `encrypted.rs`, now `in_clear` or
   object reads. sheaf-ops' `common::holds` reads every object through
   `Pdf::load_mem`, so its scans still check.

   Hand checks for Greg: Save As `forms/irs/fw4.pdf` to a new file: it's
   about 9% smaller than the source (it was more than twice the size);
   it opens in sheaf and another viewer with its fields as they were, and
   `qpdf --show-xref` lists its objects as compressed. Do the same with
   a password-protected file that uses object streams: it opens with
   the same password.

   Review (the plan's) found: §20's `packable` signature, no export
   building a `Layout`, a shared `Section::stream`, `/Size` growing by
   one on a reopen, Identity `/StmF`, lopdf's signature test, the
   256 MB limit, `ObjKey`'s next tag, chunking by container, no RC4-40
   fixture, pdf.js without a password, and the other texts this item
   makes false; all taken into the slice above. Review (the code's,
   each step's mutation checks) found: a test that passed with nothing
   packed (it now asserts packing); a new private `full::write`, a name
   slice 9 retired (`retired.rs` caught it; it's `file`).

   Review (the code's, of the whole slice) found, all fixed:
   - the M14 card read as if no packing was left for M14: it and
     parity.md's Optimize row now say packing every file, tables too,
     and recompressing streams stay in Optimize;
   - the size tier's `laid_out` and Arlington's `Checked.packs` asked
     for object streams from a file whose encryption doesn't pack (no
     such file in the corpus yet): `Measured.packs` (printed, explained)
     and arlington's `packs` follow `Crypt::packs`, an export's by the
     encryption it writes;
   - the encrypted hybrid had no test (above);
   - `in_clear` didn't look for a string written in hex: it does, in
     either case of digit (`in_clear_finds_a_string_written_in_hex`),
     and every scan that uses it still passes;
   - the oversized container's fallback had no test (above; deleting
     the loop fails it);
   - "is this an object stream" was written four times: one
     `objstm::is_container`, used by `materialize`, Arlington's hold and
     check, and `fixture::object_streams_of`.

   Found:
   - **[F12](../backlog/F12.md)** (a qpdf plugin) says it would pack
     object streams before M14: sheaf does now.
   - **Three copies of one reference walk:** `append::refs`, and private
     ones in `materialize/verify.rs` and `realize/assemble.rs`.
   - **sheaf-ops' `common::holds`** loads with the empty password only:
     on a file whose user password isn't empty it scans the bytes alone.
     Right for "not in the clear", but its doc doesn't say so.
   - **A full write near 8.4 million objects** lists every number, 1.2 s
     and about 0.5 GB to build the rows; a container past 256 MB writes
     its members twice. Both bounded, both rare.
   - **The gate deflates with miniz_oxide** (sheaf-ops builds flate2
     with its default backend), the shipped app with zlib-rs (B26).
     Deflating the cross-reference stream at level 1 would save about
     19 ms on the heavy for 21 KB more; that changes bytes, so it's
     Greg's call.
   - **A stable save's containers reuse old container numbers,** which
     `SaveRecord.present` doesn't list. Safe: an Append numbers past
     `/Size`, and `own()` refuses numbers the primary used that the file
     doesn't hold.
   - **The Arlington corpus tier** prints how many object streams it
     checked but asserts none per form; arlington's `packs` now knows an
     Identity `/StmF` form isn't packed, so it could. `hold.rs` reads
     every unkeyed object of a Rewrite to find them.

   Left for item 3 (the linearization crate): `serialize::group` is
   pure (numbers and `Use` in, containers out) and moves into the crate
   as its plan step's grouping; `materialize::graph::page_uses` (each
   object's use, from a walk bounded at two visits an object) is what
   sheaf hands it as "which pages use it". Linearizing needs page 1's
   objects first in the file, which `group` already keeps in containers
   of their own.
