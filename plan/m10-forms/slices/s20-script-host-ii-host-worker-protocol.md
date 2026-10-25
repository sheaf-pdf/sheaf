# M10 slice 20: The script host II: host, worker and protocol

Status: open · Part of [M10: Forms](../README.md)

**The script host II: host, worker and protocol** (decision 3;
proposals 13 and 20). Lands as two commits: 20a the engine, cache,
linker, limits and restart; 20b the worker, stores, eviction and the
protocol with `sync` and message checks.
- **On the edit API:** `Actor::FormScript`, and
  `FormsHost::resolve(&Snapshot, Vec<Op>) -> (Vec<Op>, reads)` on the
  worker: what a script sets comes back as ops with the decision
  reads it made, committed `Validate`. `sync` follows
  `ChangeSet.fields` for every transition `FormScript` didn't make.
- **The host,** `sheaf-formjs`: the wasmtime engine and its keyed
  compile cache, compiled off the UI thread and loaded with
  `deserialize_file`; a linker with exactly decision 3's list, its
  stubs per interface; the worker thread, a store per document, the
  set-up at open, and eviction with the rebuild on focus;
  wall-clock deadlines checked in the epoch callback, as the plugin
  host does, with a 1 ms ticker; the memory cap; the protocol
  (pdf.js's init payload, events in, updates out, `sync`), with the
  guest's messages checked (proposal 13); the restart after a trap;
  the guest's captured stdout and stderr in the script log.
- `forms-js` in sheaf-app and sheaf-cli; the CI check that sheaf-doc
  and sheaf-ops don't depend on wasmtime.
- **Checks:**
  - the spike's hostile scripts each trap, the store is dropped, and
    a fresh instance commits: 5,000 and 100,000 nested parentheses,
    and 100,000 unclosed (4,000 parse); an endless loop, inside
    `try` too; a 20,000-deep `JSON.stringify`; the backtracking
    regex; a nested `forEach`; `while(true){}` as a Validate action;
    `Array(1e8).fill`, a huge `join`, string doubling, and a loop
    keeping 1 MB strings;
  - `f(){f()}` is an error the script can catch; an out-of-memory
    message reaches the script log;
  - the guest's import list equals decision 3's list;
  - a tampered or stale compile cache is recompiled, not loaded;
  - an update naming a field that doesn't exist, or a value over
    64 KiB, is dropped and logged;
  - a form with scripts is set up at open with no script run, and an
    evicted store comes back with the model's values;
  - an undo, a plugin's fill and an import reach the sandbox through
    `sync`; a script's own commit doesn't come back to it;
  - the Rust AF path and pdf.js agree on every case.
