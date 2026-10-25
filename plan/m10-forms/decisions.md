# M10 decisions

Ruled by Greg while planning and at kickoff. Part of [M10: Forms](README.md).

## Decided while planning (Greg, 2026-10-14)

1. **The form JavaScript engine is boa_engine.** The roadmap's "decide
   at kickoff: boa or QuickJS" is settled.
   - 0.22.0 is current (published 2026-08-28). It needs Rust 1.91;
     sheaf is on 1.98. It's licensed Unlicense OR MIT; 18 of its
     dependencies (ICU4X data) are Unicode-3.0.
   - It parsed all 477 distinct scripts in the corpus (66 ms in all),
     and ran every file's document-level and open-action scripts without
     an error against a stand-in for Acrobat's objects.
   - pdf.js's form-script library runs in it unchanged, natively and
     inside the wasm guest (proposal 12).
   - It runs as a WebAssembly guest (decision 3). What scripts may do is
     proposals 12–13.
2. **M10 comes before M11.** M11 was planned first, and ROADMAP.md
   still says "M10 is skipped for now" (ROADMAP.md:245, and
   [`m11.md`](../m11-security/README.md) lines 3–4). That's changed: M10 is next.
   - At kickoff (or in slice 1), ROADMAP.md's M10 section gets the
     usual "Started …" line, loses its "Decide at kickoff" paragraph
     (ROADMAP.md:240), and its "Slices:" line (:237) takes this plan's
     order. "CSV/XML" (:232) becomes CSV, with XML later. M11's section
     and `m11.md` say it follows M10.
   - M11's plugin API bump (its proposal 15) becomes 0.5, since M10
     takes 0.4. (Since M10.5, which took 0.4: M10 takes 0.5 and M11
     0.6.)
3. **boa runs as a WebAssembly guest in sheaf's wasmtime,** not in
   sheaf's process and not as a process of its own. Greg had assumed it
   would need a sandbox and pictured a separate process, but "a wasm
   sandbox is much cleaner and for the scale of scripts that I expect us
   to deal with the performance should not be an issue".
   - **In-process boa can't be made safe against a hostile PDF.** In
     the spike, on an 8 MiB stack:
     - about 330 bytes of `(`, closed or not, crash the process: boa's
       parser overflows the stack between 300 and 330 levels, and
       upstream issues #4397 and #4311 are open;
     - `JSON.stringify` of an object nested 20,000 deep aborts too;
     - a backtracking regex (`/(a+)+$/` on 28 characters) ran 20.4 s,
       and `new Array(1e8).join(…)` peaked at 7.07 GB, each inside one
       built-in call that nothing can stop;
     - there's no memory limit, and a failed allocation aborts;
     - boa has no interrupt API (#3442, open since 2023). Its loop limit
       is per call frame: 100 calls of about a million iterations each
       ran under a limit of a million. Callbacks from built-ins aren't
       counted: a nested `forEach` ran 42.6 s. Its async budget doesn't
       reach built-in callbacks, and through pdf.js's `eval` path a
       250 ms deadline was ignored (the script ran 7.2 s).
   - **In wasmtime every one of those trapped,** and the host ran on:
     the same boa built for wasm32-wasip2 (with `annex-b` alone) and run
     by wasmtime 49.0.2, the version `sheaf-plugin` uses, with an epoch
     deadline, a memory cap and a 64 MiB wasm stack. A fresh instance
     then ran normally.
     - The second spike ran them again on the build sheaf would ship
       (`annex-b` and `xsum`, pdf.js loaded), with a 1 s deadline and a
       256 MiB cap. Up to 4,000 nested parentheses parse (557 ms). From
       5,000 the 64 MiB stack overflows (about 14 KiB a level), which
       traps cleanly because the stack comes first in the guest's
       memory.
     - Endless loops (inside `try` too), the backtracking regex, the
       nested `forEach`, a 20,000-deep `JSON.stringify`, and
       `while(true){}` as a field's Validate action through pdf.js's
       `eval`, all trapped at about 1,065 ms (the ticker's drift).
     - `new Array(1e8).join` and string doubling were refused memory at
       the cap (196.9 MiB) and trapped, with the allocation failure in
       the captured stderr. `f(){f()}` hit boa's recursion limit, an
       error scripts can catch.
     - A store that trapped refuses to run again
       (`CannotEnterComponent`), so it's dropped. A fresh one then
       committed normally every time.
   - **The cost is about 2x** (boa alone here; with pdf.js, 1.5–2x,
     proposal 12):

     | | Native | wasm |
     |---|---|---|
     | Context, form objects and AF library | 0.74 ms | 1.10 ms |
     | A SUM calculation, called as a function | 39 µs | 78 µs |
     | `AFNumber_Format` | 28 µs | 58 µs |
     | 500 calculations in one cascade | 5.2 ms | 11.0 ms |

     With pdf.js in it, compiling the guest takes 0.44 s on 32 threads
     and 6.9 s on one. The compiled form (16.9 MB) is cached and loads
     in 0.4 ms with `deserialize_file` (mmap), or about 11 ms from
     bytes. An instance takes 0.03 ms (0.15 ms the first).
     - Compiling inside sheaf's process leaves a 280–305 MB high-water
       mark from Cranelift, so it happens once, off the UI thread, and
       the cache is what normally loads. From the cache, the spike's
       host process started at 8 MB and peaked at 39.5 MB through
       Ohio's form.
   - **It also keeps boa out of sheaf's build.** Natively, boa adds
     8.0 MB to the binary, 131 crates (37 not in the lock) and 141 s to
     a clean release build, 91 s of it in `boa_engine`. As a guest with
     pdf.js it's a 6.64 MB `.wasm` (1.75 MB gzipped), built in its own
     workspace like the bundled plugins: 119–124 s clean, mostly
     `boa_engine`, and 34–38 s after a change to the guest's own code.
     It's built as a `cdylib` only, as measured; adding `rlib` changes
     what thin LTO produces.
   - **The guest gets almost nothing.** The spike's guest imports
     sheaf's own `sheaf:formjs/host` (the time zone, below) and 17 WASI
     0.2.9 interfaces: `io` poll, error and streams; `clocks`
     monotonic-clock and wall-clock; `random` random and insecure-seed;
     and `cli` stdout, stderr, stdin, environment, exit,
     terminal-input, terminal-output, terminal-stdin, terminal-stdout
     and terminal-stderr. No filesystem, sockets or `wasi:http`.
     - **Real, because they're called:** the wall clock (`Date`); the
       monotonic clock (boa's clock reads it as a context is built; a
       custom boa `Clock` would drop it); insecure-seed (hash seeding
       at load); random (`Math.random`); stdout and stderr through io
       streams, captured into 64 KiB memory pipes and sent to the script
       log, written only for a panic or out-of-memory message.
     - **Real but empty:** the environment, read only on that panic
       path, for `RUST_BACKTRACE`. Trapping it loses the message.
     - **Real though unused:** io poll and error, whose resource types
       the clocks and streams share.
     - **Trap stubs, never called:** stdin, exit and the five
       `terminal-*` interfaces.
     - The `WasiCtx` has no preopened folder, environment or arguments,
       and TCP, UDP and name lookup are off. Everything in the spike ran
       in it.
     - The plugin host isn't copied: its `wasi_ctx` inherits stderr and
       preopens granted folders (runtime.rs:561-581), and its linker
       adds `wasi:http` (runtime.rs:258-259).
     - **Not `Linker::define_unknown_imports_as_traps`** after
       wasmtime-wasi's `add_to_linker`. wasmtime-wasi 49 registers WASI
       0.2.12 and the guest imports 0.2.9, so the stubs' exact 0.2.9
       names shadow the version-compatible match, and every WASI call
       traps. Stubs are per interface, each resource defined only in
       the interface that owns it.
     - A test checks that the guest's import list equals this list.
   - **Threads:** one forms worker thread (a 128 MiB stack, reserved
     rather than committed) holds a wasmtime store for each document
     that has scripts. The UI sends events and gets back resolved
     changes; it never runs JavaScript.
     - **Set up at open, run at first use.** When a document with
       scripts opens, the worker loads pdf.js and sets up its sandbox
       from the field model in the background: 0.1–0.3 s in the guest,
       and no form JavaScript runs. The document's own scripts start at
       the first interaction (proposal 13). Set up at first focus
       instead, it would stall that focus 0.1–0.3 s.
     - A keystroke waits for its answer up to a short limit (in the
       guest a keystroke takes 0.25 ms, and a commit with five sums
       about 6 ms). Past the limit the key goes through, and the
       commit's own check decides.
     - A commit and its calculations run without blocking the UI and
       land as one edit. What happens when the user acts first is
       proposal 9.
     - A host call that waits for the user (a dialog) pushes the
       deadline back, as plugin calls do (runtime.rs:495-505).
     - **The sandbox follows the model.** pdf.js keeps its own copy of
       every field's value and properties. After any edit that changes
       them without coming from the sandbox (undo, redo, a History jump,
       an import, Clear Form, a plugin fill, a page tool, a rename, a
       Prepare Form edit), the host sends `sync {values, props}`. When
       the field tree changes, the store is rebuilt from the model's
       values.
     - **Idle stores go.** Linear memory never shrinks and a store may
       grow to the cap, so ten open binders could hold gigabytes. (The
       measured forms peaked at 65–77 MiB, nearly all of it the mostly
       untouched stack.) A store idle for 5 minutes, or beyond 4 live,
       is dropped. A store past 128 MiB is rebuilt. A dropped store is
       rebuilt from the model's values in the background when its
       window gains focus; an interaction that beats the rebuild waits
       for it (0.1–0.3 s, plus up to 70 ms of document scripts).
   - **Limits:** an epoch deadline of about 250 ms per keystroke and
     1 s per commit with its calculations; 256 MiB of memory; a 64 MiB
     wasm stack, with the guest linked for a 64 MiB stack (Rust's
     default 1 MiB traps at 100 nested parentheses). boa's own limits
     stay as a backstop: recursion 512, and a loop limit set from the
     corpus.
     - The 256 MiB includes the 64 MiB stack, so scripts get about
       192 MiB of heap.
     - The worker runs the guest synchronously on its 128 MiB thread.
       Deadlines are wall-clock, checked in the epoch callback as the
       plugin host does (runtime.rs:495-505); a ticker thread only
       advances the epoch every 1 ms so the checks happen. (The spike's
       ticker alone drifted 6.5%: a 1 s deadline trapped at 1,065 ms.)
     - In the spike the slowest steady commit was about 100 ms (a
       synthetic 300-field cascade), and Ohio's real one 70 ms. So the
       deadlines leave room, once pdf.js's slow first commit is patched
       (proposal 12).
   - **After a trap,** the instance is dropped and made again with the
     model's values, and a notice says "Form scripts stopped: took
     longer than 1 s". After three in one document, its scripts stay off
     until it's reopened.
   - **Features:** `default-features = false`, with `annex-b` (old
     scripts use `substr` and `escape`) and `xsum` (pdf.js needs
     `Math.sumPrecise`, aform.js:357). No `intl` (+16 MB), no
     `temporal`, and never `fuzz` (it gives `Context::default()` a
     budget of zero). boa supports only its latest 0.x release, so
     sheaf keeps up with its releases.
     - The second spike built this set (`annex-b` and `xsum`, without
       `temporal` or `float16`) with pdf.js. pdf.js loaded unchanged,
       and `Map.prototype.getOrInsertComputed`, which it uses, is there.
       `Math.sumPrecise` gives exact sums: `[0.1,0.2,0.3]` gives 0.6,
       and `[1e20,0.1,-1e20]` gives 0.1. The guest's size above and the
       timings in proposal 12 are for this set; the table above is boa
       alone with `annex-b`.
   - **Under wasm the local time zone is UTC,** so the host supplies the
     offset. The guest's `HostHooks::local_timezone_offset_seconds`
     calls a host import, `local-offset(unix-seconds) -> s32`, backed
     by `chrono::Local`.
     - Checked in the spike under Los Angeles, New York, Kolkata
       (`getTimezoneOffset()` -330), Lord Howe (a half-hour DST shift:
       -660 in January, -630 in July) and UTC. `printd` and `scand`
       follow it.
     - Without it, boa falls back to the `time` crate, which gives UTC
       under wasm.
     - A date expression calls the host a few times (about 15 calls for
       6 date operations).
   - **The compiled guest is cached, and checked.** It loads through
     wasmtime's unsafe deserialize, so a tampered or stale file would
     run as native code. The cache is keyed on the guest's sha256,
     wasmtime's version and a hash of the engine config, kept in sheaf's
     cache folder with 0600 permissions, and recompiled on any mismatch
     or deserialize error. Deserializing checks only the engine and
     version, not which `.wasm` the file came from (spike), so the
     guest's hash in the key is what catches a stale cache.
   - **A Cargo feature, `forms-js`, on by default,** in sheaf-app beside
     `plugins` (sheaf-app Cargo.toml:18), since both need wasmtime, and
     in sheaf-cli, which brings wasmtime and Cranelift into the CLI for
     the first time (it has only the OCR features, sheaf-cli
     Cargo.toml:12-16).
     - Without it scripts don't run, and the Rust AF path (proposal 12)
       still formats and checks values; `sheaf form fill` runs that path
       only.
     - A CI check like sheaf-image's (ci.yml:64-70) keeps wasmtime out
       of sheaf-doc and sheaf-ops.
     - Tests build the guest on first use under `target/formjs` (about
       2 minutes clean), and CI caches it.

## Settled at kickoff (Greg, 2026-10-15)

- **Signed files:** a save that would rewrite a signed or certified file
  asks first (proposal 7).
- **Encryption:** settled at kickoff as "M10 only warns", with keeping
  encryption on save left to M11 slice 1 (proposal 22). Moot since
  M10.5: every save keeps encryption, so there's no warning.
- **Fonts:** Liberation Sans, Serif and Mono for the field editor, Noto
  Sans for characters they and the field's font lack, and a character no
  bundled font covers is refused at commit (proposal 11).
- **Everything else takes the recommended answer.** Greg: "make a best
  guess if there are decisions with a recommendation". So the proposals
  below stand as written, and each ask's recommendation is taken (Asks).
- **Real forms come from Greg's corpus,** `../pdf-corpus`, not a fetch
  script, and no PDF is committed to sheaf (proposal 21, rewritten). The
  corpus was still being built at kickoff, so nothing is assumed about
  what it holds, calculation forms included.

## Asks (answered at kickoff, 2026-10-15)

Greg answered the first three; the rest take the recommended answer.

- **Signed files** (proposal 7): ask before any rewrite. Greg: yes.
- **Encryption before v0.6** (proposal 22): just warn; M11 slice 1 stays
  in M11. Greg: "just warn". (Moot since M10.5, whose saves keep
  encryption.)
- **Fonts** (proposal 11): Liberation for the editor, Noto Sans for the
  fallback, refuse what no bundled font covers. Greg: "sure".
- **pdf.js's `scripting_api` under Apache-2.0** (proposal 12): yes, with
  the Rust AF path beside it and proposal 12's patches.
- **Fixtures** (proposal 21): no PDF in sheaf; real forms from
  `../pdf-corpus` (changed at kickoff, from committing two IRS forms).
- **A hybrid form's XFA copy** (proposal 6): removed on the first change,
  with a notice, asking first when its datasets hold values.
- **Clashing field names** (proposal 5): renamed, never linked, for
  Insert, Combine and Duplicate Pages alike.
- **Scripts on by default** (proposal 13): yes, sandboxed, with
  `[forms] scripts = false` as the firm-wide switch.
- **Per-document script choices** (proposals 13 and 24): the session
  only; nothing in `state.json` keyed by a client file's path.
- **gpui-pdf's `forms.rs`** (proposal 20): a reference, not vendored.

Files that would settle open points:

- **Reference files from Acrobat:**
  - an IRS form filled and saved by Acrobat;
  - sheaf's fill of the same form (XFA removed), opened in Acrobat and
    Reader, to check proposals 6–7 and Reader's usage-rights message;
  - `AFNumber_Format(2,…)` of 1.005, to see how Acrobat rounds;
  - a Calculate script that reads `event.value`, `AFTime_Format(1)` of
    "13:05", and "13/45/2026" typed into an `mm/dd/yyyy` field, to
    confirm proposal 12's patches;
  - a simplified-field-notation calculation;
  - fields from Create Multiple Copies, and a page with fields
    duplicated in Acrobat;
  - Acrobat's FDF, XFDF, text and merged CSV exports.
- **Forms with calculations from practice:** a client organizer (Drake,
  SafeSend or CCH) and firm-made Acrobat forms (an engagement letter, a
  review checklist). Only Georgia's form calculated in earnest in the
  planning corpus; the SafeDocs pool may have more once it's searched.
- **A dynamic XFA form,** for the notice.
- **A static XFA form filled by an XFA-only tool,** with its values only
  in the datasets, for proposal 6's check.
- **A form with a signed signature field,** for the signed-file rule.

## Oracles (Greg, 2026-10-23)

Applied when the slices that need them start
([docs/oracles.md](../../docs/oracles.md#by-milestone)):
- **pdf.js's scripting** for the AF functions (slice 18), on the same
  inputs.
- **PDFium's form drawing** (FFLDraw) beside the appearances sheaf
  generates (slices 6–8).
- **OTS and fontTools** on the fallback font sheaf embeds (slice 7):
  well formed as browsers require, and what the subset holds.
