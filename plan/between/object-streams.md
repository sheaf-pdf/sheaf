# Object streams, and B3

Status: in progress

The second item [between milestones](README.md) (Greg, 2026-10-25:
"start object streams and B3"). Two slices, one PR. The linearization
crate (item 3) builds on slice 2's grouping.

## Why

A Rewrite writes no object streams. It re-serializes each object-stream
member as a plain object ([M10.5 §11.6](../complete/m10.5-edit-api/design/11-materialization.md#116-numbering-raw-passthrough-object-streams-the-page-tree)),
and it writes its cross-reference stream uncompressed (`serialize/xref.rs`).
Measured with qpdf on the corpus (2026-10-23), unpacking object streams
grows the IRS forms by a median 88%: all 70 use them, and `fw4.pdf`
grows by 178%. A sample of 377 SafeDocs files grows by 15%. Every Save As
to a new unsigned file pays this, and so does every save that removes
content or replaces text, and every export. An Append writes only what
changed, so it doesn't pay it.

[B3](../backlog/B3.md): Properties says "Fast Web View: Yes" after an
incremental save, which voided the linearization.

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
   `/Linearized` is in its first object and its `/L` equals the file's
   length counted from the header (`XrefFacts::header_offset`). An
   Append's output record says not linearized. The full check (hint
   tables, the first page's section) is the linearization crate's
   checker, item 3.

## Slices

| Slice | Status | Depends on |
|---|---|---|
| 1. Rewrite's size, measured; B3 | open | — |
| 2. Object streams on Rewrite and exports, grouped by page | open | 1 |

### Slice 1: Rewrite's size, measured; B3

**Builds:**
- **A size tier**, `crates/sheaf-ops/tests/rewrite_size.rs`, `#[ignore]`,
  and a justfile recipe `tier-size` that `premerge` runs:
  - It covers every IRS form (`forms/irs/`, 70 files) and the files in
    `bench/pool-sample.txt` that use object streams.
  - Each file gets an unsigned Save As through `Session`, the path a user
    takes.
  - It reports the output's size against the source's: the median, the
    90th percentile and the worst file, by group (IRS, pool), and
    separately for sources with and without object streams.
  - Slice 1 asserts nothing about size. It records the baseline. It does
    assert that every output reopens in sheaf, and passes `qpdf --check`
    when qpdf is on PATH (skipped, and said, when it isn't).
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
    then says No, and `qpdf --check-linearization` agrees.
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
- **`full::write_bodies` packs** when the layout asks for it (decision
  1):
  - Written objects that are packable go into containers.
  - Raw bodies stay plain.
  - The section lists members as type 2.
  - `left_out` still drops the source's containers.
- **The Rewrite path** (`materialize/mod.rs`) sets packing from the
  source (decision 1). It gives the writer each object's pages from the
  graph it already has, and numbers containers as decision 6 says. The
  header's version is at least 1.5 when it packs.
- **Exports** (dense numbering) pack under the same rule. Find where
  each export builds its `Layout`, `realize/assemble.rs` included, and
  say in the notes which ones pack and why.
- **The size tier asserts a bound.** For sources that used object
  streams, set it from what packing achieves. The target is median growth
  under 10% on the IRS forms, with no file more than 25% bigger. The
  numbers and the reason go in the notes. Files without object streams
  must not change size (decision 1).
- **Docs:**
  - `docs/document-model.md`'s writer section;
  - the `serialize/mod.rs` and `full.rs` module docs, which say "no
    object streams" today;
  - the between card's item 2 status;
  - a backlog entry for packing on Append (decision 8).

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
  (`an_aes256_object_stream_base_rewrites` today asserts no `/ObjStm`;
  update it).
- The materialize, append, realize, exports and encrypted corpus tiers
  hold, and so does the Arlington tier over what sheaf writes (its
  `ObjStm` and `XRefStream` rules).
- The interop tests read packed outputs with qpdf, PDFium and pdf.js.
- The 7k gate: the heavy fixture's Rewrite stays within 1.3× slice 8's
  writer and is flagged past 1.2×, as M10.5 set it (it has 30,317
  objects in 304 containers). Report the time with and without packing.
- fmt, clippy and the workspace tests pass, and `just plan-check` passes.

## Not in this item

- Packing on Append (decision 8; logged in the backlog).
- Packing classic-table files, or recompressing streams: Optimize, M14.
- Linearized output and its checker: item 3, the linearization crate.

## Slice notes

(Each slice adds its notes here: what was built, deviations and why,
measurements, hand checks for Greg, what review found, Found.)
