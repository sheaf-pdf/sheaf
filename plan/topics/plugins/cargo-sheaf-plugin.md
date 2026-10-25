# `cargo sheaf-plugin`: the plugin authoring tool

Status: planned, not started. Written 2026-10-19, from a review of what
an outside plugin author needs (2026-10-18). Slices 1–3 can land between
M10.5 slices; 4 and 5 wait for M10.5 slice 20 (plugin API 0.4).

## Why

Today an author follows [`docs/plugins.md`](../../../docs/plugins.md) by hand:
add the WASI target, write `Cargo.toml` and `sheaf-plugin.toml`, draw an
icon, build, then copy three things into `<config>/plugins/<id>/`.
sheaf's own plugins use `scripts/build-plugins.sh`, which only works
inside this repo. Nothing checks a plugin before sheaf loads it, so
mistakes first show up in Plugins → Manage Plugins…. A `.sheafplugin` is
made by zipping the files and renaming the zip.

`cargo sheaf-plugin` does these steps:
- `new`: a plugin crate that builds and loads as it is.
- `build`: build for `wasm32-wasip2` and stage the plugin folder.
- `check`: what the host would refuse, before the host sees it.
- `pack`: the `.sheafplugin` file.

It's for authors, so it has to be light. `cargo install
cargo-sheaf-plugin` should take seconds and pull in no PDFium, Tesseract,
wasmtime or GPUI.

## Decided (Greg)

- sheaf supports `cargo sheaf-plugin` as the authoring tool (2026-10-18).
- The crate is `cargo-sheaf-plugin`, and it runs as the cargo subcommand
  `cargo sheaf-plugin` (2026-10-19).
- M10.5 slice 11 splits `sheaf-core` and renames it `sheaf-util`
  (2026-10-19; [`m10.5-edit-api.md`](../../complete/m10.5-edit-api/README.md) §6.1).
  - Its types move out: `Rotation`, `PageSize` and `DestView` go to
    `sheaf-edit`, and `PageIndex` goes to `sheaf-render`.
  - What stays (the checksum and file reading) is never published.
  - So nothing published depends on it, and its taken name doesn't
    matter. Every crate outside the published set gets `publish = false`.

## Proposed (to confirm)

1. **A workspace member, `crates/cargo-sheaf-plugin`, published to
   crates.io.** Greg's first thought was a crate outside the workspace.
   - What keeps it light is its dependencies, not where it lives. `cargo
     install` builds only the crate's own dependencies. CI enforces it
     the way it keeps sheaf-image pixels only (`ci.yml:65-71`): a `cargo
     tree` check (below).
   - It has to agree with the host on the manifest and on the package
     layout, and both are defined here. As a member, `cargo clippy
     --workspace` and `cargo test --workspace` build it against the
     current code on every commit, and the round-trip and template tests
     live next to the host's. Outside, it would be another loop that CI
     has to remember, like `plugins/*` (`ci.yml:84-92`).
   - sheaf uses it for its own plugins (slice 3), so the bundled plugins
     test the tool as they test the SDK.
   - `plugins/` is excluded from the workspace because it builds for
     `wasm32-wasip2` (`Cargo.toml:3-5`). The tool builds for the host,
     so that reason doesn't apply.
   - It doesn't take the workspace's `rust-version`; it states the oldest
     Rust it needs.
2. **The tool never touches the user's sheaf.** Installing, listing,
   turning on and off, permissions, uninstalling and the headless `run`
   go in sheaf-cli as `sheaf plugin …` (its own plan, not written yet).
   They share `plugins.json` and `.bundled.json` with a running app (see
   [`file-contention.md`](../file-contention.md)), and `run` needs the real
   host anyway. The tool's output is a folder or a file; the user
   installs it.
3. **The SDK stays in this workspace and is published from it.** Zed
   publishes `zed_extension_api` from its main repository the same way.
   Under 0.4 the contract is the WIT plus `sheaf-edit` (M10.5 §6.1,
   §14.3), and `sheaf-doc` depends on `sheaf-edit` too. A separate repo
   would have to release in lockstep with this one.
4. **One definition of a plugin package**, in a new
   `sheaf-plugin-manifest` crate, used by the host, the app, sheaf-cli
   and the tool alike (below).
5. **Versions follow the API.** The tool's major.minor is the
   `sheaf:plugin` version it targets, and the patch is its own. `cargo
   install cargo-sheaf-plugin@0.4` gets the tool for 0.4. The manifest
   crate follows the same rule. (The SDK's version is already the WIT
   package's.)

## Found while planning

1. **Icons that the manifest check accepts but installing drops.** The
   check (`manifest.rs:387-400`) accepts any `.svg` path inside the
   folder. But installing from a folder (`copy_tree`, `plugins.rs:166`)
   keeps only top-level files and the files directly in `icons/`, and
   installing from an archive (`extract_archive`, `plugins.rs:547`) also
   requires a plain file name (`is_icon_name`, `plugins.rs:588`).
   - `icon = "art/scan.svg"` or `"icons/sub/scan.svg"` loads from the
     source folder. After either kind of install the file is gone, and
     the command is left out.
   - `icon = "icons/My Scan.svg"` survives a folder install but not an
     archive.

   Fix: the check accepts only `icons/<plain name>.svg`, the one layout
   both installs keep (slice 1).
2. **Folder and archive installs keep different files.** A folder
   install copies every top-level file (`Cargo.toml`, `Cargo.lock`,
   anything). An archive keeps four names (`plugins.rs:548`). Both should
   use the shared package definition (slice 1).
3. **`build-plugins.sh` guesses the `.wasm` name** from `^name` in
   `Cargo.toml`, with dashes turned into underscores. A `[lib] name`
   would break it. `build` takes the artifact path from cargo's JSON
   messages instead (slice 3).
4. **The quick start's `crate-type = ["cdylib"]`** (`docs/plugins.md:39`)
   differs from the bundled plugins' `["cdylib", "rlib"]`, which they
   need for host-side tests. The template uses the bundled plugins' form
   (slice 5).
5. **`sheaf-core` is taken on crates.io** (0.2.0, MonumentalSystems/sheaf-rs,
   unrelated). 0.4's SDK depends on `sheaf-edit`, which M10.5 §6.1 built
   on `sheaf-core`. Settled: slice 11 splits it (Decided).

## Design

### `sheaf-plugin-manifest`

Pure data and checks, with no wasmtime and no `sheaf-doc`. Its
dependencies are serde, toml, semver, regex, anyhow, usvg (the minor
GPUI draws icons with, 0.46 today) and zip. It holds:
- What `crates/sheaf-plugin/src/manifest.rs` has now: `Manifest` and the
  declarations, `Permissions` and `Grants`, `RESERVED_IDS`, `FILE_NAME`,
  `WASM_NAME`, the checks and the icon check.
- `API_VERSION` (and, until slice 20 deletes it, `API_PREVIOUS`), moved
  from `crates/sheaf-plugin/src/lib.rs:31-34`.
- **`package`**, the one definition of a plugin package's files:
  `sheaf-plugin.toml`, `plugin.wasm`, `icons/<plain name>.svg`,
  `README.md` and `LICENSE`, plus the limits (64 MiB a file, 512 files).
  It provides:
  - `files(dir)`;
  - `copy(from, to)`, replacing the app's `copy_plugin`/`copy_tree`;
  - `unpack(archive, to)`, replacing `extract_archive`, still by name
    only, so entry paths never choose where anything is written;
  - `pack(dir, out)`.
- **`component`**: reads a `plugin.wasm` with `wasmparser` and reports:
  - whether it is a component (a core module means it was built for
    `wasm32-wasip1`);
  - its `sheaf:plugin` imports and their version;
  - any other import that isn't WASI 0.2 (`wasi:http` included);
  - whether the four entry points are exported.

  `HOST_IMPORTS` lists, as data, the interfaces the host links
  (`runtime.rs:261-268`). A host test keeps the list honest (see
  Verification).

`sheaf-plugin` and `sheaf-app` use it directly. `sheaf-plugin` stops
re-exporting `Manifest`, `Permissions`, `Grants` and `EventKind`
(`lib.rs:24`), and the app's `zip` dependency moves into this crate.

### `cargo-sheaf-plugin`

A binary named `cargo-sheaf-plugin`. Cargo runs it as `cargo-sheaf-plugin
sheaf-plugin <args>`, so the clap setup accepts and skips that first
argument. It runs cargo through `$CARGO`, which keeps the toolchain cargo
resolved.

**`check [dir | file.sheafplugin]`.** Errors are anything the host would
refuse. It uses the same code, so the two agree:
- the manifest, through `Manifest::parse` and the icon check, against
  the API version the tool was built for (it says which);
- the package layout;
- `plugin.wasm`: present, within the limit, and a component whose imports
  and exports pass `component` and whose `sheaf:plugin` version the
  manifest's `api` accepts.

Warnings:
- a single-key `keys` with no `keys-context` (`docs/plugins.md` asks for
  one);
- an icon whose viewBox isn't 24 × 24, or that uses colors other than
  `currentColor`.

A source folder with no `plugin.wasm` yet gets the manifest and icon
checks, plus a note that it isn't built. The exit status is nonzero on
errors, and on warnings too with `--deny-warnings`.

**`build [crate-dir …]`.**
- Runs `$CARGO build --release --target wasm32-wasip2
  --message-format=json-render-diagnostics` and takes the cdylib's path
  from cargo's messages. That respects `CARGO_TARGET_DIR`, workspaces and
  `[lib] name`.
- Stages `<target-dir>/sheaf-plugin/<id>/` with `package::copy`, then
  runs `check` on the staged folder.
- `--dev` builds the dev profile, so trap backtraces keep function names
  (the template's release profile strips them).
- `--out <dir>` and several crate folders cover sheaf's `plugins/*`.
- When the target is missing, it says `rustup target add wasm32-wasip2`.

**`pack [dir]`.** A staged folder is packed as it is; a crate folder is
built first. It writes `<id>-<version>.sheafplugin` and refuses when
`check` has errors. The zip is deterministic (entries sorted, fixed
timestamps), so one build always packs to the same bytes, which plugin
signing ([`code-signing.md`](../code-signing.md):242) can rely on later.

**`new <name>`.** A folder with:
- `Cargo.toml`: `["cdylib", "rlib"]`, and the release profile from the
  docs;
- `sheaf-plugin.toml`: one command, `when = ["document"]`;
- `src/lib.rs`: a 0.4 plugin with that one command, its logic in a pure
  function with a host-side unit test;
- `icons/<command>.svg`: a generic icon drawn to sheaf's icon rules;
- `README.md` and `.gitignore`.

The SDK comes from crates.io at the tool's API version. `--sdk-path
<dir>` points at a sheaf checkout instead (sheaf's template test uses
it). The template files are `include_str!`'d from the crate and tested
on every commit.

### The published set (slice 4)

- **The WIT moves into the SDK crate** (`crates/sheaf-plugin-api/wit/`),
  and the host's bindgen reads it there (`crates/sheaf-plugin/src/api/mod.rs:11`).
  `cargo package --list` shows no `wit/` today, so the SDK can't be
  published as it stands.
- **The set is** `sheaf-plugin-api`, `sheaf-edit`,
  `sheaf-plugin-manifest` and `cargo-sheaf-plugin`, released together
  under one tag (`plugin-sdk-v0.4.0`). All four names were free on
  2026-10-19. Names are first-come.
- **CI runs `cargo publish --dry-run`** for each, which catches a `../`
  path or a dependency that is only a path.

## Slices

Sizes as in M10.5: S = 1, M = 2, L = 3. About 9 points.

| # | Slice | Size | When |
|---|---|---|---|
| 1 | `sheaf-plugin-manifest`; the host and the app on it | M | between M10.5 slices |
| 2 | The tool: `check` and `pack` | M | after 1 |
| 3 | `build`; sheaf's own scripts on it | S | after 2 |
| 4 | The published set | M | after M10.5 slice 20 |
| 5 | `new`, and the docs around the tool | M | after 4 |

**On M10.5.** Slices 1–3 touch `crates/sheaf-plugin/src/manifest.rs` and
`crates/sheaf-app/src/plugins.rs`, which M10.5 slices 12 and 20 also
change. Each lands as its own commit between M10.5 slices, as the
in-progress swap did (`07550ca`), never while slice 12 or 20 is in
flight. Slice 20 then adds `edit-schema` and `permissions.identity` to
the manifest in its new crate, deletes `API_PREVIOUS` there, and makes
the `api` check exact-minor. The tool picks all of that up through the
shared code.

1. **`sheaf-plugin-manifest`.**
   - Move the manifest, the API constants and the archive code.
   - Add `package`.
   - Fix found-while-planning 1 and 2.
   - Move the manifest tests.
   - Delete the app's `copy_plugin`, `copy_tree`, `extract_archive` and
     `is_icon_name`.
   - Add the crate to the `cargo tree` check.
2. **`check` and `pack`.**
   - Add the `component` module and `HOST_IMPORTS`.
   - Add `crates/cargo-sheaf-plugin` with `check` and `pack`.
   - Add the negative fixtures (Verification).
   - Add the crate to the `cargo tree` check.
   - Works on today's 0.3 plugins.
3. **`build`.**
   - `scripts/build-plugins.sh` becomes a call to the tool built from the
     workspace, which `package.sh:37`, `ci.yml:92` and `release.yml` use.
   - The CI loop over `plugins/*` keeps its fmt, clippy and tests.
   - The staged `target/plugins` is byte-identical before and after the
     switch.
4. **The published set.**
   - Move the WIT into the SDK crate.
   - Check that nothing in the set depends on `sheaf-util`, or on any
     crate with `publish = false`.
   - Add the dry runs to CI.
   - Publish at 0.4.0.
5. **`new` and the docs.**
   - Add the template and its test.
   - Rewrite the quick start in `docs/plugins.md` around `new`, `build`,
     `check` and `pack`.
   - "Testing a plugin" points at `build --dev`, and at `sheaf plugin
     run` once sheaf-cli has it.
   - Add a line to the README's building notes.

## Verification

- **The manifest crate:**
  - its moved tests pass;
  - every icon path the check accepts survives both `package::copy` and
    `pack` → `unpack`;
  - `pack` → `unpack` → `Manifest::load` round-trips;
  - two `pack`s of one folder are byte-identical.
- **Light:** `cargo tree -e normal` for `sheaf-plugin-manifest` and
  `cargo-sheaf-plugin` shows none of the crates the sheaf-image check
  rejects, and no wasmtime crate. Record the time of a cold `cargo
  install --path crates/cargo-sheaf-plugin` in the slice notes.
- **Agrees with the host:**
  - every bundled plugin and the test fixture passes `check`, and the
    host loads it;
  - each negative fixture fails both: a core module (built for
    `wasm32-wasip1`), a component importing an interface the host
    doesn't link, and a bad icon path;
  - the host test loads each bundled plugin and checks its imports
    against `HOST_IMPORTS`.
- **The template:** `new` → `build --sdk-path <repo SDK>` → the host
  loads the plugin and runs its command on a fixture document, and
  `cargo test` passes in the new crate. Runs in CI.
- **Publishing:** `cargo publish --dry-run` passes for each crate in the
  set, and the SDK and `sheaf-edit` build for `wasm32-wasip2`.

## Open

- **The same component check in the host at load**, for a clearer
  message than wasmtime's link error. Leaning yes; it's shared code.
- **`new --ci github`**, a workflow that builds, checks and attaches the
  `.sheafplugin` to a release. Leaning yes, behind the flag.

## Out of scope

- `sheaf plugin install | list | enable | disable | allow | revoke |
  uninstall | run` in sheaf-cli: their own plan (Proposed 2).
- Plugin signing and a catalog
  ([`code-signing.md`](../code-signing.md):242).
- Templates for other languages. `check` and `pack` work on any
  component; `build` and `new` are for cargo only.
- Prebuilt sheaf binaries, the other half of what an outside author
  needs: `release.yml` exists, but no release has been published.

## Critical files

- `crates/sheaf-plugin/src/manifest.rs`, `crates/sheaf-plugin/src/lib.rs`
  (`:24`, `:31-34`), `crates/sheaf-plugin/src/api/mod.rs:11`,
  `crates/sheaf-plugin/src/runtime.rs:261-268`,
  `crates/sheaf-plugin/tests/host.rs`
- `crates/sheaf-app/src/plugins.rs` (`:153-185`, `:485`, `:529-595`),
  `crates/sheaf-app/Cargo.toml` (`zip`)
- `crates/sheaf-plugin-api/` (the WIT moves in), `wit/`
- `scripts/build-plugins.sh`, `scripts/package.sh:37`,
  `.github/workflows/ci.yml` (`:65-71`, `:84-92`),
  `.github/workflows/release.yml`
- `docs/plugins.md`, `README.md`
- New: `crates/sheaf-plugin-manifest/`, `crates/cargo-sheaf-plugin/`
