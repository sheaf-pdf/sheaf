# sheaf's Arlington checks on pdf-lint

Status: in progress

An item [between milestones](README.md), next after object streams
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
    [B45](../backlog/B45.md)'s, `/DA` on button fields.
  - Its bytes reader finds what a writer can get wrong below the object
    level: a `/Length` that doesn't end at `endstream`, cross-reference
    rows that aren't 20 bytes, a key written twice, a last line that
    isn't `%%EOF`.
  - It checks against the model's grammar the way the PDF Association's
    TestGrammar does.
- **CI can check every platform.** sheaf's outputs hold object streams
  since [object streams](../complete/object-streams.md), and pdf-lint
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
     has.
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
     keep the backlog entry as a column or in the reason.
   - **Annex E's name classes**, which `private.rs`'s test uses: from
     pdf-lint if it exposes them, else a few lines in that test.
5. **What goes:**
   - `crates/sheaf-doc/src/arlington/` whole, and the `fixtures`
     feature's re-export of it;
   - `vendor/arlington-pdf-model`, and its mentions in `about.toml`,
     `deny.toml` and the licence notes, if any;
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
   don't.

## Slices

| Slice | Status | Depends on |
|---|---|---|
| 1. The tests on pdf-lint; sheaf's checker and the vendored model gone | open | — |

### Slice 1: The tests on pdf-lint; sheaf's checker and the vendored model gone

**Builds:**
- **First, the baseline:** the default Arlington tests' printed counts,
  and `just tier-arlington` (tables checked, findings, carried, allowed,
  per form), recorded before anything changes.
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
- Checking more of what sheaf writes than the tests check today.

## Slice notes

(Each slice adds its notes here: what was built, deviations and why,
measurements, hand checks for Greg, what review found, Found.)
