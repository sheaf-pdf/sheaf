# Document transforms: plugins that take the file's bytes and return new ones

Status: findings, on the planning path after M10.5 (Greg, 2026-10-20);
no plan yet. Written 2026-10-20 from a design check
against M10.5 as of slice 13 (`2c13ca7`): five readers over the plan and
the code, two competing designs (byte-faithful and model-faithful) and
two adversarial critics (correctness, feasibility). Plan references are
to [`../m10.5-edit-api.md`](../m10.5-edit-api.md) by section; code
references are as of that commit.

## The idea (Greg, 2026-10-20)

A plugin kind that gets the bytes sheaf would save for a document, does
anything it likes to them, and hands back new bytes. The result lands
one of two ways:

1. **In place:** the result becomes the document, as one history node
   ("a batch transform of the entire PDF"), undoable like any edit.
2. **Export:** the result is written byte for byte to a new location,
   as Export Flattened writes its file. The document and its history
   don't change.

What it's for: signing and timestamping, linearizing, PDF/A conversion,
optimizers, and outside cleanup or redaction tools built for wasm.

## Decided (Greg)

- **Not in M10.5, but on the planning path** (2026-10-20). An
  export-only slice 21 was considered (M with a Rewrite input) and kept
  out for now. It lands in a later plugin API version.
  M10.5 §20 has M10 taking 0.5 and M11 taking 0.6, so transforms join
  whichever minor is current when they're built.
- **Both landings are wanted:** in place and export.

## Proposed (to confirm)

1. **One interface, two destinations.** The plugin does the same work
   either way: read the input, write the output. The manifest says which
   destinations a transform allows, and the user picks one when running
   it. `input.info()` tells the plugin which one it's running for, so a
   linearizer can refuse to run in place, for example. Two interfaces
   would double the SDK surface for no difference on the plugin's side.

   ```wit
   interface transform-io {
     resource input {
       len: func() -> u64;
       read: func(offset: u64, len: u32) -> result<list<u8>, string>;  // at most 8 MiB per read
       info: func() -> input-info;  // destination, signed, page count, PDF version
     }
     resource output {
       copy: func(offset: u64, len: u64) -> result<_, string>;  // from the input, host side
       write: func(bytes: list<u8>) -> result<_, string>;
     }
   }
   export run-transform: func(id: string, args-json: option<string>,
                              input: borrow<input>, out: borrow<output>)
                              -> result<string, string>;  // Ok: a summary line
   ```

   Manifest:
   `[[transforms]] { id, title, destinations = ["document", "export"], keeps = "prefix" | "any", memory = "1 GiB" }`,
   plus a `document-bytes` permission.
2. **Ship the export destination first, with a Rewrite-image input.**
   That fits M10.5's export design as it stands (below). The Save As
   input that signers want, and the in-place destination's structural
   change to the model, can follow in later minors.

## Shared by both destinations

- **The permission.** The input carries everything: authors' names in
  `/T`, field values, attachments, metadata and hidden data. The consent
  text says so, and names any network hosts the plugin also has. This is
  a deliberate exception, to be recorded with its scope:
  - M10.5 decided-while-planning 6: "Plugins and the agent get semantic
    edits only, at first";
  - §6.8's "a plugin never learns the user's identity" (proposal 20),
    and slice 20's check for it, which then apply only to plugins
    without `document-bytes`;
  - M11's "plugins never apply redactions" (§20, §6.7), since outside
    redaction tools are one of the uses. It holds by type today because
    `ApplyRedactions` is a `Transform`, which only an `ExportTx` takes.
- **The runner.** Each run gets a fresh instance on its own thread, with
  its own `Store` and the manifest's memory limit (capped by the host;
  wasm32 stops at 4 GiB), from a cached `Component`. Today every plugin
  has one thread and one `Store`, a trap unloads the whole plugin, and
  guest memory defaults to 256 MiB with a 30 s call timeout
  (`crates/sheaf-plugin/src/runtime.rs`). Wasm linear memory never
  shrinks. So a transform in the shared instance would block the
  plugin's other calls, keep its gigabyte after it finishes, and take
  the plugin down if it ran out.
  - Cancel is `is-cancelled` first, then an epoch interrupt that kills
    only that instance.
  - The timeout needs raising for transforms. Today only the long host
    calls are credited back against it (OCR, `show-form`, render, HTTP);
    the input and output calls would need the same.
  - The instance gets no write preopens and never sees a path.
- **Transport.** Host-held `input` and `output` resources, read and
  written in chunks. One `list<u8>` each way would copy the whole file
  into guest memory. `copy` lets a signer pass the input's prefix
  through without it entering wasm.
- **Asks come before the plugin starts.** The plugin sees the bytes, so
  "every way out asks first" (§12.1) has to run before it: signatures,
  `XfaHoldsValues`, and M11's unapplied redactions. The one exception is
  a signature ask that depends on the output (the plugin dropped a
  prefix whose signatures held). That's only known after the run, so it
  needs a stop that holds the output and resumes, like `SaveStop::Ask`;
  `export::run` takes its answers up front today.
- **The output is checked before it's used:** a size cap enforced as
  it's written, a `%PDF-` header, and an open through the `Store`.
  - Of §11.9's post-conditions, only the removal check and copy-on-write
    need sheaf's own emission.
  - Reachability, `forms::check`, the `private::KEYS` scan and M11's
    leak check (which already reads materialized bytes) can run on the
    reopened output, as Notes at least.
  - Slice 19's every-way-out test asserts `forms::check` is clean and a
    clean copy has no `private::KEYS`. Plugin output can't promise
    either, so the test reruns those checks on it or records an
    exemption.
- **What can't work:**
  - a wasm plugin can't run qpdf or Ghostscript; it needs a wasm build
    of the tool, or the bytes go out under a network grant;
    - **a C++ build needs wasm exceptions, which sheaf's runtime can't
      load yet.** qpdf throws and catches throughout, its repair path
      included, so a WASI build of it uses the exception-handling
      proposal. Wasmtime 49 supports that only when built with its `gc`
      feature (`Config::wasm_exceptions` is behind `cfg(feature =
      "gc")`, and exceptions are on by default when it is). sheaf builds
      wasmtime with `default-features = false` and no `gc`
      (`crates/sheaf-plugin/Cargo.toml`), so such a component is
      rejected at load. Enabling `gc` should be enough on the host side;
      its cost in binary size and build time isn't measured. The
      plugin's side needs a wasi-sdk whose libc++ supports
      `-fwasm-exceptions` (which release first did is unchecked).
      Emscripten's builds of qpdf don't apply: sheaf runs WASI
      components with no JavaScript host;
    - qpdf's other needs fit: zlib and libjpeg are plain C, its native
      crypto avoids OpenSSL, it reads and writes buffers in memory, and
      it's Apache-2.0. Ghostscript also builds for wasm but is AGPL;
      backlog F12 is a qpdf plugin;
  - files near 4 GiB can't be loaded whole by a plugin.

## The export destination

With a Rewrite-image input it fits §12 without changes to the model.
`export::run` (slice 19) gains one step between "verifies (§11.9)" and
"writes atomically": hand the materialized bytes to the transform, then
write the plugin's bytes with the same atomic write. Everything else in
§12 already holds:
- the binder's own file is refused as a target;
- the preflight gates run, the signature ask against the export's
  Rewrite included;
- it's a job with progress and cancel;
- it stays out of History and plugin events;
- it works on a `Snapshot` and is `Send`, so the user keeps editing.

It needs none of the in-place destination's hard parts: no versioned
file, no id matching, no merge question, no history budget and no
save-after story.

- **Layering:** `export::run` lives in sheaf-doc, and sheaf-plugin
  depends on sheaf-doc. So the plugin step is a hook passed into
  `Export`/`run` by the caller, a small API change.
- **Where the output goes:** streamed into durable's temp file in the
  target folder, or into memory, never `temp_dir`. Slice 19 tests that
  nothing is written to the temp or cache directories.
- **Input, v1: the export's own Rewrite image.** It can also be a
  preset's image (Export Flattened, Clean Copy), so a plugin can run
  after sheaf's own transforms, for example Clean Copy and then PDF/A. A
  signed file gets the usual "signatures will stop validating" ask up
  front, so signers can't add to an already-signed file in v1.
- **Input, later: what Save As to a new path would write,** so a signer
  gets a signed file whose signatures still hold. That isn't an export
  product today:
  - Save As is copy-and-append only when every Append condition holds:
    signatures that hold, a matching stamp, a sound base, and no
    Removes or ReplacesText. Otherwise it's a Rewrite with the
    signature ask or "Append anyway" (§9).
  - Its copy is a materialized section that `durable::copy_appended`
    writes onto the saved file, so there are no input bytes in memory.
  - `export::run` plans only Rewrites, and §12 calls it "the ONLY writer
    besides Save".

  So it needs an Append/copy strategy for exports with a bytes form of
  the copy, plus the stop-and-resume signature ask above. Running the
  transform on a Save without `record_save` would be a third writer.
- **Encrypted documents:** refused in v1. Print exports with
  `Encryption::None` because its bytes stay in memory (proposal 10). A
  decrypted result written to disk is what proposal 10 and M11's "no
  plaintext on disk" guard against. Allowing it later needs an explicit
  ask that the result is written unencrypted. For a signed file the
  decrypted input is a Rewrite, so the same ask has to say the
  signatures break.
- **After writing:** offer "Open the result". That's close to M14's
  "Save As Optimized" alternative (write an export to a new path, then
  open it), and it covers many in-place uses before the in-place
  destination exists.
- **CLI:** slice 19's `sheaf edit -o out.pdf` is the closer home, for
  example `--transform <plugin>/<id>`. Today `sheaf export` writes page
  images or text into a folder, and no CLI command exports a PDF. It
  needs the plugins README's piece 5 (`sheaf plugin …` in sheaf-cli).
- **Batch:** M15's `Step` would need a new variant or field. A byte step
  runs after verify, so it isn't a `Transform`, which is a closed enum
  lowered on an `ExportTx` before materializing.
- **Size:**
  - **M with the Rewrite input,** on top of slices 19 and 20. The runner
    is about a third of it, then the WIT, manifest and SDK, the export
    hook, the UI, and two fixture plugins with their gates.
  - **L with the Save As input,** for the Append/copy export strategy
    and the stop-and-resume ask.

## The in-place destination (findings)

**Verdict:** possible, with one structural change. It's too large to
add to slice 20, which is already L and all plugin surface; about two
thirds of this is sheaf-doc model and save-planner work.

**Why the existing ops can't land it.**
- `ReplacePages` keeps the old file's catalog, duplicates marks (it
  keeps the model's and re-imports the file's copies), brings the form
  in renamed apart, and ignores the outline and labels.
- `SetPageBase` onto a derived copy is deleted in slice 17, forces a
  Rewrite over the old file, and loses catalog-level state too.

**The change: the file a document is becomes versioned state.**
- Today `Document.primary` sits outside history. It's the source a
  Rewrite writes onto, and it supplies every catalog key sheaf doesn't
  model: `/Names` (embedded files, JavaScript, named destinations),
  `/OCProperties`, `/Metadata`, `/Lang`, the structure tree,
  OutputIntents, the AcroForm's `/XFA` and `/Perms`, encryption and
  `/Info`. No version can change it.
- It becomes one key, `Key::Primary`, through the registry rule
  (classify, emit_deps, `ChangeSet`, `HeapSize`, `check_invariants`,
  `render_key`). D4's objection to reserved keys doesn't apply, because
  the transform produces it.
- Every reader follows: about 120 uses in sheaf-doc (`Snapshot`,
  `SaveInput`, `LowerCx`, `instance_copy`, the budget's kept set,
  recovery, the planner), plus sheaf-app's panels (layers, attachments,
  patterns, script, form bar) and sheaf-ops `combine.rs`. Slice 19 adds
  more (`Session`, Combine, `export::run`).

**The op.** A host-only `ResolvedOp::ReplaceDocument`, with no public
`Op`, like `Revert`. It's lowered off the UI thread from
`Document::opened`, refactored to take any source and an id matcher.
The state it builds is the one opening the file would give, diffed
against the base into `Put` prims plus the primary's `Put`. Undo, redo
and jump need nothing new: they're prim swaps, and nothing re-lowers.

**Identity.** Ids are bound back so selections, dialogs and plugins'
cached ids survive:
- **pages:** by object number and generation through the input's
  `ObjMap`, then by position only when the page counts agree, otherwise
  fresh. No `PageId` may be bound twice, enforced in release builds
  (`check_invariants` is only a `debug_assert`);
- **marks:** free, from `/SheafAnnot` and `/NM`, as open already does;
- **generators:** free after slices 16–17, from `/SheafGenerators` and
  `/SheafBlock`;
- **fields:** by full name; **bookmarks:** by pre-order when the title
  matches;
- **other tools' comments:** by `/NM`, then number, then subtype and
  rect. These are best effort.

Unsaved content stacks come back as base content, as after a save and
reopen.

**Concurrency.** There's no merge, and a byte transform reads
everything.
- It commits only if the cursor is still the version it started from,
  checked by version token.
- Editing, undo, redo and jump pause while it runs.
- It's unavailable while OCR or another group job runs, since their
  held flushes would conflict on every page.

**Input.** The save's own image: when Save would Append, that's the
file on disk plus the section. So a signer appends after the existing
signatures instead of getting a Rewrite that breaks them.

**Output.**
- It's interned as `Origin::Memory(MemoryKind::Transformed)`.
- Unreadable or encrypted output is refused. Repaired output
  (`base_sound = false`) is a policy choice; see Open.
- Output that PDFium refuses still draws, through slice 13's
  refused-base path.
- Output identical to the input (by `BlobId`) is a no-op with a notice.

**Saving afterwards.**
- **A save straight after** writes the plugin's bytes verbatim ("Adopt")
  and records them as an opened file.
- **Output that extends the disk file** (a signer, P = input ‖ tail):
  the tail goes through recovery's existing `PendingTail` and
  `Came::Restored` path, so later edits Append in place.
- **Otherwise,** the first save that has to Rewrite onto the plugin's
  file loses object streams, xref streams, linearization and the
  plugin's signatures.
- **Whole file plus a section in one write** (`Onto::Base`) reverses
  D28 and would need its own decision.

**Rendering.** Nothing new. Every render key changes, the visible pages
re-render from a new base engine, and the old images stay cached for
undo.

**Size and shape:** L + M. A model half (versioned primary, the op, id
matching, save and recovery paths), tested with a Rust fixture
transform. Then the plugin half: the destination in the manifest, the
land branch, and two fixture plugins, one that appends and one that
rewrites.

## Problems the critics found

Any in-place slice must fix these. For the export destination, items 3
and 7 always apply, and items 1 and 2 apply once its input is the Save
As image (the host then serves the disk file during the run).

1. **Signatures.** Ask before landing or Adopting whenever the input's
   signatures hold and the output doesn't keep the input as a byte
   prefix, judged against the saved file's base (§9). Record
   `signatures_hold = false` when the user agrees.
   `SaveRecord::opened` hard-codes `true` today.
2. **A torn input.** The disk part of the input is read lazily (`pread`
   on a held fd), and the stamp is checked only when the save is
   planned. A file rewritten in place during a long run would hand the
   plugin one set of bytes while `copy` writes another. Check the
   `FileStamp` again at the end of the run, or hash the copied prefix.
3. **Hidden text.** An incremental "redact" or "clean" plugin can leave
   the text readable in the earlier revision. Run the removal check over
   the plugin's tail and set `TailFlags.removes` from it; it must never
   be silently false. Adopt runs the same check. For the export
   destination it's at least a Note.
4. **Memory.** A superseded primary that's still brought by a version on
   the line is never counted, so three transforms of the 474 MB scanned
   7k hold about 1.9 GB that nothing weighs. Counting it while the saved
   version or its anchor still pins it is no better: pruning drops the
   transform's undo and frees nothing. Count only what pruning can free,
   and keep the newest transform's undo.
5. **The step's summary.** Pages re-based together with the primary
   change are a re-expression, not 7,000 removed pages. Otherwise
   History shows "removes 7,000 pages" with the Removes badge.
6. **Recovery.** A Full sidecar on every autosave after a transform
   takes 1.6 s, writes 474 MB and peaks at about 2 GB RSS on the scanned
   7k. It needs a bounded design, without a hash-named second copy of
   the file beside the user's (rule 11).
7. **Gates.**
   - An identity transform keeps every id and reopens to the same facts.
   - Transform-and-land time and peak RSS on the heavy 7k.
   - A peak-RSS bound on the scanned 7k: the input image, the output
     buffer, the old file and the new one are all resident.
   - Append after a tail-only transform passes `qpdf --check` and keeps
     its signatures.
   - Autosave cost after a transform.

**Costs to expect, from M10.5's measurements:**
- **Heavy 7k (17.9 MB):** about one open to read the result back
  (342–357 ms, +200 MB RSS), and a Rewrite-image input in 629–664 ms
  (+237 MB). Every page re-renders, and opening the new base engine
  costs about a 0.7 s pause.
- **Scanned 7k (about 474 MB):** a Rewrite-image input peaks at 1.6 GB
  RSS.

## Open

- **Destinations:** declared per transform in the manifest
  (recommended), or every transform offered both ways?
- **The export input:** the Rewrite image in v1, with a choice of preset
  (recommended), and the Save As image when signers need it?
- **Unreadable export output:** refuse (recommended), or ask before
  writing?
- **Other output types** (Markdown, images, EPUB) by a declared media
  type, skipping the PDF check. Later.
- **Encrypted documents:** refused in both destinations at first. Export
  can allow them later with an explicit "written unencrypted" ask; in
  place waits for M11's `Key::Security`.
- **Edits during an in-place run:** paused (recommended), or the first
  edit stops the plugin?
- **Large files:** may a transform declare raised memory and time limits
  (up to about 3.5 GiB and 15 minutes), shown at consent?
- **`document-bytes` together with network hosts** in one plugin:
  allowed, with the consent naming the hosts? No bundled plugin should
  have both.
- **Repaired output** (lopdf rebuilt the xref): refuse it, or accept it
  as sheaf accepts repaired files at open, at the cost of never
  appending to it?
- **Byte-faithfulness after more edits:** is a Note naming the transform
  enough when a later save has to Rewrite, or should that save ask?
- **When the in-place destination is built,** its model half lands
  before or with whatever next adds readers of the primary.
