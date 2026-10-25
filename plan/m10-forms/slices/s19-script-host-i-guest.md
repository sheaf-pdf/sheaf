# M10 slice 19: The script host I: the guest

Status: open · Part of [M10: Forms](../README.md)

**The script host I: the guest** (decisions 1 and 3; proposal 12).
- The second spike already showed pdf.js runs acceptably in the
  guest (proposal 12), so this slice builds the real one on what it
  learned.
- **The guest:** boa 0.22 (`annex-b`, `xsum`) and pdf.js's
  `scripting_api` at a pinned commit with sheaf's patches (proposal
  12), compiled in with `include_str!` and served by an in-memory
  `ModuleLoader`; the glue behind pdf.js's `callExternalFunction`;
  the `local-offset` host import behind `HostHooks`; the `Date.parse`
  shim, run in each sandbox after pdf.js loads. Linked for a 64 MiB
  stack, and built as a `cdylib` only.
- Built by the plugin build script and shipped beside the bundled
  plugins; tests build it on first use under `target/formjs`, and CI
  caches it.
- `NOTICE-forms-js.txt`, generated from the guest workspace's
  `cargo metadata` (its license fields).
- A minimal test host (the spike's: wasmtime, decision 3's linker and
  stubs, the `local-offset` import) under the guest's tests, so this
  slice's checks run before slice 20; slice 20 replaces it with
  `sheaf-formjs`.
- A benchmark over the spike's cases: load; set-up of NY IT-201, GA
  500 and Ohio; a keystroke; a commit; Ohio's cascade and the
  300-field ones, first commits included. Its numbers go in the slice
  notes beside proposal 12's.
- **Checks:** pdf.js's 58 unit tests and the 12 added cases pass
  inside the guest, 70 of 70; `Math.sumPrecise([1e20,0.1,-1e20])` is
  0.1; `getTimezoneOffset()` follows the host under
  `TZ=Asia/Kolkata` and `TZ=Australia/Lord_Howe`; Ohio's commit logs
  no `TypeError`; the first commit on the 300-field cascade stays far
  inside 1 s (110–120 ms in the spike).
