# Build or adopt 2: lopdf fixes upstream (the weekend's pull requests)

Area: Cross-cutting: the PDF object layer, writing, and Flate · Part of [build or adopt](README.md)

- **Need:** know which of sheaf's fixes still need to go upstream.
- **Where:** F13 "Upstream meanwhile" (`backlog.md:726-736`);
  `vendor/lopdf/SHEAF.md:316-324`.
- **Today's plan:** Greg sends them on the weekend of 2026-10-24/25,
  each confirmed first (`backlog.md:726-727`).
- **What upstream has now** (`J-F-Liu/lopdf` `main` at `d0b3abf`,
  2026-10-23; read in the code, not run):

  | sheaf fix (`backlog.md:729-736`) | Upstream |
  |---|---|
  | Strings in stream dictionaries (#603) | **In 0.46.0:** `2f497f8`, "encrypt and decrypt strings in stream dictionaries, and leave signature /Contents in clear (#603, #608)" |
  | V4+ filter names and `/Identity` | **Merged 2026-10-22** (#624, "Fix Identity defaults for V4 and V5 encryption filters"; `encryption.rs:713-715` on main). Whether an undeclared name is now an error: unverified |
  | The set in `traverse_objects` and `prune_objects` | **Merged 2026-10-23** (#630, "traverse and prune in linear time"). Whether it's the same fix: unverified |
  | Rebuilt file with no `trailer` keyword (SHEAF.md) | #613, "reconstruct files that only have cross-reference streams" (see below) |
  | Caps in object streams and predictors | **Merged 2026-10-22** (#625) |
  | `/EFF` | Not upstream (only a doc comment, `encryption.rs:189`) |
  | `/Encrypt null` | Not found upstream |
  | White space around `startxref` | Not upstream: `xref_start` still takes an optional single space before the end of line (`parser/mod.rs:717-726`) |
  | Each xref stream read once; lenient `/XRefStm` past EOF | unverified |
  | f64 reals, `LazyPdf` | Not upstream (`Real(f32)` in 0.46.0) |

  - **#613's timing:** the core section's table and change list put it
    in 0.46.0; its summary lists it among the merges since. Settle it
    when diffing.
  - **#584** (the owner password giving the wrong key) is fixed in the
    vendored code and in 0.46.0 (entry 29), though the issue still shows
    open.
- **Fit:** about half the list was done by others in the last 10 days;
  they're fixing the same corpus-style bugs. sheaf's local branch
  `fix/xref-correction-every-entry` (in `/home/greg/repos/lopdf`) is in
  flight too.
- **Recommendation:** **build on upstream.** Before writing each PR,
  compare the `sheaf:` hunks with upstream `main`, and send only what's
  missing: `/EFF`, `/Encrypt null`, `startxref`, and the
  xref-stream-once fix if it's still needed. Moving the vendored copy to
  0.46+ waits for between-milestone triage, as Greg ruled, and should
  drop the hunks upstream now has. High.
