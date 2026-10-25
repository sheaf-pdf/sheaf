# Building

```sh
# Linux (Debian/Ubuntu) system deps for GPUI — once:
sudo apt install build-essential clang lld cmake pkg-config libfontconfig-dev \
  libwayland-dev libx11-xcb-dev libxkbcommon-x11-dev libvulkan1 libasound2-dev \
  libglib2.0-dev libssl-dev libzstd-dev xdg-desktop-portal xdg-desktop-portal-gtk


scripts/fetch-pdfium.sh          # prebuilt libpdfium → vendor/pdfium/<os>-<arch>/
scripts/fetch-ocr-data.sh        # Tesseract language data + PaddleOCR models → vendor/ocr/
                                 #   (--uvdoc adds the curled-page model scan cleanup can use)
scripts/build-plugins.sh         # the bundled plugins (plugins/) → target/plugins/
cargo run -p sheaf-app           # the GPUI window
                                 #   (debug builds don't hand files to a running
                                 #   sheaf unless SHEAF_SINGLE_INSTANCE=1)
```

**Standard operations** are in the `justfile` (`cargo install just`; `just`
lists them):
- `just setup` makes a dev environment from a fresh checkout: the system
  packages (with sudo, on Debian and Ubuntu), the toolchain and targets,
  PDFium and the OCR data, the plugins, and the oracle tools
  ([docs/oracles.md](oracles.md)). `just doctor` says what's there.
- `just ci` runs what CI runs.
- `just precommit` runs what CI can't: the tests with the corpus,
  interop, the release properties, and clippy as macOS and Windows see
  sheaf-app.
- `just premerge` runs every ignored corpus tier, before a PR merges.
- `just gates` runs the 7k gates, waiting for a quiet machine first.

Tests run under a memory cap where Linux has a user systemd
(`scripts/test-capped.sh`; `SHEAF_UNCAPPED=1` turns it off).

**The test corpus.** The corpus tests read about 16,000 real-world and
test-suite PDFs from `../pdf-corpus` beside this checkout, or
`$SHEAF_CORPUS`, and skip without them. The PDFs aren't in this
repository: [fetch-pdf-corpus](https://github.com/sheaf-pdf/fetch-pdf-corpus)
rebuilds the corpus byte for byte from where each file was published,
checking each one's sha256:

```sh
cargo install --locked --git https://github.com/sheaf-pdf/fetch-pdf-corpus --tag v0.1.1
fetch-pdf-corpus fetch --out ../pdf-corpus        # 4.3 GB
fetch-pdf-corpus fetch --out ../pdf-corpus pool   # 1.9 GB more: the pool files the
                                                  #   ignored realize and lexer tiers
                                                  #   and bench/real.py read
```

A rerun resumes, and `fetch-pdf-corpus verify --dir ../pdf-corpus`
checks a folder. `parsing/unsafe/` holds files made to break parsers:
read them as bytes, or in a process you can lose.

**Plugins.** sheaf runs WebAssembly plugins (the `plugins` feature, on by
default; `--no-default-features` leaves out wasmtime too). The bundled
plugins build for `wasm32-wasip2`, which `rust-toolchain.toml` installs;
debug builds pick them up from `target/plugins`. The API is
`sheaf:plugin` 0.4, the only one served: plugins edit with sheaf's own
ops (`sheaf_edit::Op`, as JSON) and place things in each page's own
space. A plugin call's edits are one undo step, landing when it returns
on whatever you did meanwhile that it didn't read, and taken back if it
fails. A plugin isn't told who wrote what: authors are "you", "plugin
<id>" or "other", and `<<name>>` stays as written, unless you allow it
(its manifest asks for `identity`). It is told the document's path,
which may hold your login name. Writing your own:
[`docs/plugins.md`](plugins.md).

**OCR engines and build time.** Both engines are on by default. The first
build compiles Tesseract 5 and Leptonica from source (CMake and a C++17
compiler; a few minutes, cached in `~/.tesseract-rs`) and needs network
access once. `cargo build -p sheaf-app --no-default-features` skips the
engines for a quick build (the Recognize Text dialog then says so). The
PaddleOCR engine does not link ONNX Runtime: it loads Microsoft's generic
build from `vendor/ocr/lib` (or `ocr/lib` beside a packaged binary) the
first time it is used, so the app starts on any x86-64 CPU and the
`ort` crate's AVX2-only static binaries are never involved. CI runs the
binary under `qemu -cpu Nehalem` to keep it that way.

On Linux, file dialogs use the desktop portal when available and fall back to
Zenity if the portal fails. WSLg installations without a desktop portal need
`sudo apt install zenity` for Open, Insert PDF, Save As, and export dialogs.

On Windows, install Visual Studio 2022 (or the Build Tools) with the
"Desktop development with C++" workload and a Windows 10/11 SDK, plus rustup
with the default `x86_64-pc-windows-msvc` host. No other system deps. Run the
fetch script from Git Bash; it detects the host and installs `pdfium.dll`:

```sh
bash scripts/fetch-pdfium.sh     # → vendor/pdfium/windows-x86_64/pdfium.dll
bash scripts/fetch-ocr-data.sh   # → vendor/ocr/
cargo run -p sheaf-app
```

Visual Studio ships the CMake the Tesseract build needs; run the first
build from a Developer PowerShell (or with `VSINSTALLDIR` set) so it uses
the NMake generator.

The toolchain is pinned in `rust-toolchain.toml`. It has to be new enough
for the pinned GPUI snapshot: gpui-pre publishes no `rust-version`, so Zed's
own `rust-toolchain.toml` at the snapshot's commit is the guide.

## Updating GPUI

GPUI comes from [gpui-pre](https://crates.io/crates/gpui-pre): Longbridge's
weekly crates.io snapshot of Zed main, its crates renamed `gpui-pre-*`. The
sources are Zed's at the snapshot's commit plus a few packaging edits marked
`Modified for gpui-pre`. sheaf keeps no patches against Zed and lets
gpui-pre do the snapshotting. gpui-pre doesn't follow semver, so the root
`Cargo.toml` pins one snapshot exactly, and moving to a newer one is a
deliberate review (once a milestone or so):

1. Find the newest version and the Zed commit it snapshots. Inside this
   workspace `cargo info gpui-pre` describes the locked version and shows
   the newest only as "(latest X)"; `cargo info gpui-pre@X` then ends its
   description "snapshot of zed@<rev>" (the full rev is `zed-rev` under
   `[package.metadata.gpui-pre]` in its `Cargo.toml`).
2. Bump both exact pins (`gpui-pre` and `gpui-pre-platform` share a
   version) and the rev in the comment above them.
3. Read gpui-pre's own edits: `rg -l 'Modified for gpui-pre'
   ~/.cargo/registry/src/*/gpui-pre*-<new>/` (after a `cargo fetch`), and
   read each one that is new or changed. They are where gpui-pre's code
   departs from Zed's, and they're written for gpui-kit; keep the list in
   the `Cargo.toml` comment current.
4. Keep `[patch.crates-io]` in step with Zed at that rev: the
   `[patch.crates-io]` of Zed's `Cargo.toml` for the crates GPUI uses, and
   for `xim-ctext` the xim-rs rev in Zed's `crates/gpui_linux/Cargo.toml`.
   Published crates can't carry Zed's patches, so sheaf repeats them; drop
   any that cargo warns are not used. If Zed now needs a newer Rust (its
   `rust-toolchain.toml`), move ours with it.
5. Read the `Cargo.lock` diff. Besides the `gpui-pre-*` versions, expect only
   the crates gpui-pre's published manifests take from crates.io where Zed
   builds from git (`zed-font-kit`, `zed-xim`, `zed-scap`, `wasm_thread`) and
   their own dependencies (at 0.3.9 `xim-parser`, and `dirs` 5 where Zed has
   6); explain anything else. Diff each crate swapped to crates.io, and each
   of its dependencies that resolves differently, against the source at
   Zed's git rev, and wherever the code differs in behavior add a
   `[patch.crates-io]` to that rev, as for `xim-ctext`.
   [Prior art](prior-art/rust-ecosystem.md#gpui-pre) has what each
   differed in last time.
6. Run `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D
   warnings` and `cargo test --workspace`, then a UI smoke test: open a
   corpus PDF with `--timing --selftest-save out.pdf` and with
   `--selftest-annot out.pdf` (a `--script` ending in `{"quit": true}`
   closes the window), and look at the window. Put the timings in the
   bump's commit message and compare them with the last bump's (`git log
   -- Cargo.toml`). Fix what broke in the same change; Windows CI covers
   the Windows backend.

## Packaging

```sh
scripts/package.sh               # → dist/sheaf-<version>-<os>-<arch>.tar.gz (zip on Windows)
```

The bundle is the release binaries (`sheaf` and the GUI-less `sheaf-cli`)
next to the PDFium library, the OCR data
in `ocr/` (about 25 MB of language files and models, plus the ONNX Runtime
library in `ocr/lib`), the bundled plugins in `plugins/`, the licenses, and
(on Linux) a `.desktop` entry; unpack it anywhere and run `sheaf`. The
binary looks for `libpdfium` and `ocr/` beside itself first, so no install
step is needed. Pushing a `v*` tag runs `.github/workflows/release.yml`, which builds
the Linux and Windows bundles and attaches them to a GitHub release; `ci.yml`
runs fmt, clippy, tests and a build on every push.

Versions are alpha pre-releases (`0.1.0-alpha.N`) until the app is ready for
daily use. To release, bump `version` in the root `Cargo.toml` and push the
tag `v<version>`. The workflow refuses a tag that doesn't match `Cargo.toml`
and marks tags with a `-` suffix as pre-releases.

## Optional: faster builds (sccache + mold)

The GPUI dependency tree is ~700 crates; a cold build is 10+ minutes on a
modest machine. Two tools cut iteration time substantially. Both are
configured in your **user-level** `~/.cargo/config.toml`, not in this repo, so
nothing here forces them on anyone.

**sccache** caches compiled dependency artifacts across `cargo clean`s,
toolchain bumps, and GPUI version bumps.

```sh
cargo binstall sccache        # or: cargo install sccache
```

**mold** replaces the system linker. Link time dominates incremental
rebuilds of a GPUI binary, and mold is several times faster than `ld`/`lld`.

```sh
# Linux: grab the release binary (mold isn't on crates.io)
curl -fsSL https://github.com/rui314/mold/releases/download/v3.0.0/mold-3.0.0-x86_64-linux.tar.gz \
  | tar -xz --strip-components=2 -C ~/.local/bin mold-3.0.0-x86_64-linux/bin/mold
# or: sudo apt install mold
```

Then in `~/.cargo/config.toml`:

```toml
[build]
rustc-wrapper = "sccache"

[target.x86_64-unknown-linux-gnu]
linker = "clang"
# Use an absolute path if ~/.local/bin isn't on PATH.
rustflags = ["-C", "link-arg=-fuse-ld=mold"]
```

Notes:
- Changing `rustflags` or `rustc-wrapper` invalidates every artifact; expect
  one full rebuild after setup.
- sccache can't cache incremental compiles, so the `sheaf-*` workspace crates
  always compile locally. That's fine — incremental is faster for code you're
  editing. The win is the dependency tree.
- `sccache --show-stats` to confirm hits; `sccache --zero-stats` to reset.
- On Windows, mold doesn't apply; `lld-link` (bundled with the MSVC toolchain
  via `rust-lld`) is the equivalent lever. sccache works unchanged.
