# Plugins: the whole picture

Status: overview, written 2026-10-19. It ties together the plugin work
that's done, planned and not yet planned. Each piece links to its own
plan where there is one.

## The goal

Anyone can write, test, package and share a sheaf plugin, in any
language that builds a WASI component, without building sheaf from
source. Rust is the reference. The plugin API is the WIT and the files
beside it, not any one language's crate.

## Where it stands (2026-10-19)

- **Shipped (M6, [`../plugins.md`](../plugins.md)):** the wasmtime host,
  the manifest, declarative forms and panels, events, storage,
  permissions with consent, the plugin manager and `.sheafplugin`
  installs. API `sheaf:plugin@0.3.0`, with 0.2 still served.
- **Six bundled plugins** in `plugins/`, built by
  `scripts/build-plugins.sh`. The queue they came from is
  [`../plugin-queue.md`](../plugin-queue.md); writing images is parked in
  [`../plugin-image-writes.md`](../plugin-image-writes.md).
- **The Rust SDK** (`crates/sheaf-plugin-api`, 0.3.0) isn't published.
  It can't be as it stands: its bindgen reads `../../wit`, which `cargo
  package` leaves out.
- **An outside author** has to build sheaf from source to run a plugin,
  because no release has been published. They write the manifest and
  copy files into the plugin folder by hand, nothing checks a plugin
  before sheaf loads it, and there's no way to test one without the GUI.
- **Being planned:** API 0.4 in M10.5 slice 20
  ([`../m10.5-edit-api.md`](../m10.5-edit-api.md) §14.3). It's the only
  world, ops cross the WIT as JSON of the Rust `Op`, each call is one
  transaction, and the manifest check is exact-minor until alpha.

## Decided (Greg)

- **Third-party authors, WASM components, declarative UI**
  (2026-10-11; [`../plugins.md`](../plugins.md)).
- **API 0.4 is the only world, and ops cross as JSON**
  (M10.5 proposal 18, decision 3).
- **An authoring tool, `cargo sheaf-plugin`** (2026-10-18). The crate is
  `cargo-sheaf-plugin` (2026-10-19). Plan:
  [`cargo-sheaf-plugin.md`](cargo-sheaf-plugin.md).
- **`sheaf-core` is split and renamed `sheaf-util` in M10.5 slice 11**
  (2026-10-19, §6.1).
  - `Rotation`, `PageSize` and `DestView` move to `sheaf-edit`, and
    `PageIndex` to `sheaf-render`.
  - `sheaf-util` keeps the checksum and file reading, and is never
    published.
  - Every crate outside the published set gets `publish = false`.
- **SDK order** (2026-10-19): Rust is the reference, then Python, then
  TypeScript. The Python and TS SDKs are bindings, not separate designs
  ("just bindings to the rust sdk").
- **Document transforms** (2026-10-20): a plugin kind that takes the
  bytes sheaf would save and returns new ones, landing in place as one
  history node or written out as an export. Not in M10.5; a later API
  version. Findings: [`document-transforms.md`](document-transforms.md).

## Proposed (to confirm)

1. **Anything with real logic lives behind the WIT.** This is how "just
   bindings" works. A Python or TS plugin can't link the Rust SDK:
   - one route is composing a Rust helper component into every plugin
     (`wac`);
   - the other is shipping the Rust SDK as a WASI wheel, which works for
     Python only.

   Both are heavy, and the Rust SDK's code is thin anyway. So:
   - Logic every language needs is a host function, which each
     language's bindgen picks up: checking ops (`edits.preview`), the
     view matrices `page-info` already carries, and later perhaps the
     amount parsing that subtotal, amount-crossref and currency-convert
     each do themselves.
   - Each SDK is generated bindings, types generated from the schema,
     and about a page of idiom that uses the Rust SDK's names.
   - The Rust SDK is the reference for names and for the JSON it
     produces, not a library the others link.
   - Review rule: a helper with real logic that only Rust has belongs
     in the WIT.
2. **Five layers:**

   | Layer | What it is | Shared or per language |
   |---|---|---|
   | 0. Contract | WIT `sheaf:plugin@0.4.0`; the edit schema (ops, `Applied`, `EditError`; edit-schema `1.N`); the manifest schema and the `.sheafplugin` layout; conformance scenarios with golden JSON | Shared, versioned 0.4 |
   | 1. Bindings | Generated from layer 0, never hand-edited: wit-bindgen, componentize-py, jco, and types from the schema (Rust re-exports `sheaf-edit`) | Per language |
   | 2. SDK | Entry points, the `Ops` and form builders, progress, a Python HTTP helper | Per language, thin |
   | 3. Tooling | `check` and `pack` for any language; `new` and `build` that drive cargo, componentize-py or jco | One tool |
   | 4. Host | One conformance suite, run against each SDK's fixture | Shared |

3. **The contract files aren't owned by Rust.** The WIT stays canonical
   at the root (`wit/`). The Rust crate ships a copy, and a test checks
   it matches; no symlink, since Windows checkouts often don't keep
   them. The edit schema is generated from `sheaf-edit` by schemars,
   committed, and fails CI when stale, and a change to it is reviewed
   as a contract change.
4. **Same version numbers.** An SDK's major.minor is the contract's
   version; patch numbers are each SDK's own.
5. **Equal footing:**
   - no SDK gets something the others can't (no channel only Rust can
     use);
   - every SDK passes the conformance suite before a release;
   - every SDK's `Ops` builder produces the golden JSON byte for byte.
6. **The SDK stays in this workspace** and is published from it, as Zed
   publishes `zed_extension_api`. Under 0.4 the contract includes
   `sheaf-edit`, which `sheaf-doc` also depends on.
7. **Rename `sheaf-plugin-api` to `sheaf-plugin-sdk`** in M10.5 slice
   20, which already rewrites its surface and every bundled plugin. The
   docs already call it the SDK, and the new name can't be confused with
   the host crate `sheaf-plugin`. The name was free on crates.io on
   2026-10-19.
8. **Layout:**
   ```
   wit/                  the contract's WIT (canonical)
   schema/               edit-1.N.json, sheaf-plugin.toml.json (generated, committed)
   conformance/          scenarios and golden op JSON
   sdk/rust/             → crates.io  sheaf-plugin-sdk
   sdk/python/           → PyPI
   sdk/ts/               → npm
   crates/sheaf-edit, crates/sheaf-plugin-manifest, crates/cargo-sheaf-plugin
   ```

## The pieces

| # | Piece | Plan | Status | Depends on |
|---|---|---|---|---|
| 1 | Plugin API 0.4 | [M10.5 slice 20](../m10.5-edit-api.md) | planned | M10.5 slices 11–19 |
| 2 | The contract files (WIT at the root, schemas, package format) | here, and M10.5 slice 20 for the schema | proposed | 1 |
| 3 | `sheaf-plugin-manifest` | [cargo-sheaf-plugin.md](cargo-sheaf-plugin.md) slice 1 | planned | — |
| 4 | Authoring tool, `cargo sheaf-plugin` | [cargo-sheaf-plugin.md](cargo-sheaf-plugin.md) | planned | 3; slices 4–5 on 1 |
| 5 | `sheaf plugin …` in sheaf-cli: install, list, enable, allow, uninstall, headless `run` | none yet | to plan | 3; [`../file-contention.md`](../file-contention.md) |
| 6 | The published set: the rename, the WIT copy, version rules, dry runs | [cargo-sheaf-plugin.md](cargo-sheaf-plugin.md) slice 4 | planned | 1, 2 |
| 7 | Conformance suite (scenarios, golden JSON, a fixture per SDK) | none yet | to plan | 1, 2 |
| 8 | Python SDK (componentize-py) | none yet | to plan | 6, 7 |
| 9 | TypeScript SDK (jco / ComponentizeJS) | none yet | to plan | 6, 7 |
| 10 | Precompiled plugins, so large components load fast | none yet | to plan | before 8 ships |
| 11 | WASI 0.3 for plugins | none yet | later API version | — |
| 12 | Prebuilt sheaf binaries; the installer | [`../installer.md`](../installer.md) | release workflow exists, nothing published | — |
| 13 | Signing and a catalog | [`../code-signing.md`](../code-signing.md):242 | out of scope for now | 12 |
| 14 | Author docs (`docs/plugins.md`) | M10.5 slice 20; cargo-sheaf-plugin slice 5 | planned | 1, 4 |
| 15 | Document transforms: bytes in, bytes out; export first, then in place | [document-transforms.md](document-transforms.md) | findings; after M10.5 | 1; M10.5 slice 19; in place also needs a versioned primary |

**Notes on the pieces:**
- **5** is the only way an outside author can test a plugin against a
  real document. The plugin install code (`install_dir`,
  `extract_archive`, consent, the bundled-plugin record) lives in the
  GPUI crate (`crates/sheaf-app/src/plugins.rs`) and moves out first.
- **7** generalizes `crates/sheaf-plugin/tests/host.rs`, which today
  runs against one Rust fixture.
- **10:** the host sets up wasmtime with only the component model and
  epoch interruption (`crates/sheaf-plugin/src/runtime.rs:258-259`), with
  no cache for compiled code. So every plugin is compiled by Cranelift
  at every startup, which is cheap for Rust but not for a CPython or JS
  engine component of several to tens of megabytes. Precompile when a
  plugin is installed, keyed by the plugin's checksum and the wasmtime
  version. Keep the result in sheaf's own cache folder, never the
  plugin's: loading precompiled code is `unsafe` in wasmtime, so sheaf
  only loads files it made itself.
- **11:** WASI 0.3 came out on 2026-06-11 (0.3.1 on 2026-08-11), and
  wasmtime has had it on by default since 46. sheaf is on wasmtime
  49.0.2 but links only `wasmtime_wasi::p2` (`runtime.rs:262-263`).
  Staying on 0.2 for API 0.4 is right, since wasi.dev still advises 0.2
  for every toolchain but Rust. `check` should name a 0.3 component
  clearly. 0.3's built-in async would suit sheaf's blocking calls
  (`show-form`, HTTP, OCR).
- **12** gates publicity: there's no point telling outside authors
  about the SDK before they can download a sheaf to run their plugin.

## Order

1. **Now, between M10.5 slices:** `cargo-sheaf-plugin` slices 1–3 (the
   manifest crate, `check` and `pack`, `build`). They fix two install
   bugs and put the bundled plugins on the tool. They never land while
   M10.5 slice 12 or 20 is in flight.
2. **M10.5 slice 11:**
   - `sheaf-edit` is created, and `sheaf-core` becomes `sheaf-util`;
   - the edit schema's shapes are settled so they generate cleanly into
     Python and TS;
   - the Rust semver of the spec structs is settled (Open).
3. **M10.5 slice 20:**
   - API 0.4;
   - the SDK's surface checked against Proposed 1;
   - the rename (Proposed 7);
   - the schema published.
4. **After M10.5:**
   - the conformance suite (7);
   - the published set (6);
   - `new` and the docs (cargo-sheaf-plugin slice 5);
   - the sheaf-cli commands (5).
5. **Before inviting outside authors:** a published sheaf release (12).
6. **Then** the Python SDK (8, with 10 just before it), then TS (9).

## Open

- **Rust semver for the published `sheaf-edit`.** `Op` is already
  `#[non_exhaustive]` (M10.5 §2), but the spec structs have public
  fields. An optional field added to one is backward compatible for
  JSON (edit-schema `1.N`) but breaks Rust code that builds the struct
  literally. Leaning:
  - `#[non_exhaustive]` on the spec structs, built through the `Ops`
    builder or constructors;
  - the SDK pins `sheaf-edit` exactly;
  - `cargo-semver-checks` on the published set in CI.

  Settle by M10.5 slice 11.
- **Which helpers move behind the WIT** under Proposed 1. Amount
  parsing is the likeliest; it would need an API design of its own.
- **Python's HTTP helper:** a few lines over `wasi:http` in the SDK, or
  a host function so every language gets one-call HTTP.
- **The `cargo-sheaf-plugin` plan's open items:**
  - its proposals 1–5;
  - `publish = false` on the new crates until they're published;
  - the oldest Rust each published crate needs;
  - keeping its WASI import check loose (`wasi:*@0.2.*`);
  - the host running the same component check at load;
  - `new --ci github`.
- **Whether `../plugins.md`, `../plugin-queue.md` and
  `../plugin-image-writes.md` move into this folder.** They're linked
  from `README.md`, `ROADMAP.md`, `docs/plugins.md` and code comments.

## Languages that build components today

As of 2026-10-19, from [wasi.dev/languages](https://wasi.dev/languages).

| Stack | For a sheaf plugin |
|---|---|
| Rust (`wasm32-wasip2`, Tier 2 stable) | Ready; the reference |
| C / C++ (wasi-sdk, `wasm-tools component new`) | Works, low-level; the WIT and the schema only, no SDK |
| Python (componentize-py) | Works; embeds CPython (tens of MB); pure-Python packages only, unless a native one is built for WASI |
| JavaScript / TypeScript (jco / ComponentizeJS) | Works; embeds a JS engine (several MB); `fetch` over `wasi:http` |
| Go (TinyGo + go-modules) | Works with TinyGo's limits; upstream Go only builds WASI 0.1 modules |
| C# (componentize-dotnet) | Pre-release |
| MoonBit | Works; few users |
| Kotlin, Ruby, Zig | Modules only, no component toolchain |
| Java (GraalVM), Swift | Planned |
