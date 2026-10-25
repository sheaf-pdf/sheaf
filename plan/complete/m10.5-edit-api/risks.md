# M10.5: Risks

Part of [M10.5: the edit API](README.md).


- **Scope.** The plan is 20 slices, about 68 points, with M10 paused
  throughout.
  - *Mitigations:* slices 1–4 bring value on their own (exact replay,
    measured risks, the Store, one open). M10's slices 6–9 shrink by
    about one slice (the spike, the re-binding and the per-writer save
    rules go).
- **The lazy reader must be exact** on damaged, hybrid, encrypted and
  hostile files.
  - *Mitigations:* corpus-wide lazy-against-eager equality, the eager
    fallback, fuzzing and one `Limits`.
- **Overlays on real files are unmeasured.** That covers PDFium with
  in-memory append sections on encrypted, xref-stream, hybrid,
  linearized, damaged and leading-junk bases, and widgets after a page
  is replaced in place.
  - *Mitigation:* slice 2's written criteria, with fallbacks (a
    decrypted image per source, standalone groups) decided before any
    slice depends on them.
- **The change to f64 reals** touches about 56 sites and may shift
  output formatting.
  - *Mitigation:* the save suites and a corpus round trip in slice 3.
- **The legacy projection and the adapted `rebase_to_saved`** (slice 8)
  feed the old writers while records exist.
  - *Mitigations:* they live for one slice only and cross no coordinate
    or appearance change; the replay proptest and every save suite run
    through them; both are deleted in slice 9.
- **No-rebase saves and in-place Appends** depend on the `SaveRecord`
  (`present`, owned roots, `tree`, stamps) being right across
  alternating Append, Rewrite, Save As and recovery. A mistake writes a
  broken file.
  - *Mitigations:* the Append ≡ Rewrite property in the default run, the
    delete/save/undo/save/reopen chain, `qpdf --check`, the journal, and
    stamp re-checks before every write.
- **False positives in the removal check** can force rewrites, or asks
  on signed files.
  - *Mitigations:* derived objects are judged on their inputs (exact);
    decoding goes through ToUnicode; subsets are GID-preserving; the
    corpus run counts vetoes per class.
  - *Remaining exposure:* vector geometry is out of scope, and an
    unknown encoding errs toward a Rewrite.
- **Value-validated commits plus `Relower`** are new concurrency
  machinery.
  - *Mitigations:* the commit rule is one function with a property test
    (a random Tx, a concurrent commit and an amend, then commit: the
    result is either a Conflict or a state containing both changes, and
    replay equals the snapshot), plus an explicit test for each policy
    row.
- **Live page tokens** rely on deterministic layout. A font or layout
  code change between sessions turns a reopen-and-reorder into a
  re-layout of every covered page. It still appends as `LIVE_REDRAW`,
  but it surprises.
  - *Mitigation:* the byte-determinism test and the planner's
    explanation. Proposal 8 lets Greg choose frozen tokens instead.
- **`keep_only` and the Rewrite path** still parse every reachable dict
  structurally, and 500 MB scans are unmeasured until slice 2.
  - *Mitigation:* slice 9's Rewrite gate. The port to a symbolic graph
    happens only if that gate fails.
- **The PageSpace switch** (slice 5) touches every gesture, drawing and
  hit-test site.
  - *Mitigations:* the `to_view ∘ from_view` property, and selftests on
    rotated, cropped pages.
- **Strict removal** sends more saves down the Rewrite path and asks
  more often on signed files.
  - *Mitigations:* raw passthrough keeps rewrites near I/O speed,
    "Append anyway" covers removals and form transforms, and the gate
    records Rewrite time.
- **The DocMDP, FieldMDP and UR3 mapping** rests on ISO 32000 and vendor
  docs.
  - *Mitigation:* warnings stay conservative until the Acrobat
    cross-check (Asks).
- **Interop is inferred.** Acrobat's handling of our named-property
  `/Artifact` blocks, and the `/PieceInfo` rule for its own, need an
  Acrobat round trip (slices 16 and 17).
- **Engine memory.** Each group engine parses its own xref and object
  cache over the same base.
  - *Mitigations:* LRU by measured bytes, a separate background budget,
    and geometry-only pages that need no overlay if spike C passes.
- **JSON ops over WIT** lose compile-time typing for non-Rust plugins.
  - *Mitigations:* `deny_unknown_fields`, per-kind refs with clear
    errors, the published schema and the `edit-schema` manifest check.
- **The `/SheafAnnot` v2 digest** must be computed identically on write
  and on read.
  - *Mitigations:* the canonical form, and a round-trip test for every
    mark kind, by both strategies and after a re-serialization by
    another tool.
- **Sources stay alive for the session.** Opening a huge file, deleting
  most of it and continuing keeps the whole file in memory until the
  document is closed.
  - *Accepted, and documented.*
