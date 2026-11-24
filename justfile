# sheaf's standard operations. `just` lists them; `just setup` makes a dev
# environment; `just ci` runs what CI runs; `just precommit` runs the
# checks CI can't (the corpus, interop, the release properties). See
# docs/oracles.md for the tools the checks lean on.

set shell := ["bash", "-euo", "pipefail", "-c"]

tools_dir := env("SHEAF_TOOLS", home_directory() / ".local/share/sheaf-tools")
pdf_lint := env("SHEAF_PDF_LINT", tools_dir / "pdf-lint/pdf-lint")
pdfjs := env("SHEAF_PDFJS", tools_dir / "pdfjs/node_modules/pdfjs-dist/legacy/build/pdf.mjs")
verapdf := env("SHEAF_VERAPDF", tools_dir / "verapdf/verapdf")
logs := justfile_directory() / "target/just"

# Versions the setup recipes install; docs/oracles.md says why each.
pdf_lint_version := "0.2.0-rc.1"
pdfjs_version := "6.4.299"
verapdf_version := "1.30.3"
verapdf_key := "13DD102B4DD69354D12DE5A83184863278B17FE7"

# The fuzz targets the nightly CI job runs (.github/workflows/fuzz.yml).
fuzz_targets := "lazy_reader page_tree sheaf_annot splice interp op_json sheaf_generators"

# List the recipes.
default:
    @just --list --unsorted

# ----- Setting up --------------------------------------------------------

# A dev environment from a fresh checkout: system packages (sudo), the Rust
# toolchain and targets, PDFium and the OCR data, the plugins, and the
# oracle tools. `just doctor` after, to see what's there.
[doc("A dev environment from a fresh checkout (system packages need sudo); then `just doctor`")]
[group('setup')]
setup: setup-system setup-rust fetch plugins-build setup-oracles doctor

# System packages: GPUI's build deps and the oracle tools (Debian and
# Ubuntu; elsewhere it prints what to install). Asks for sudo.
[doc("System packages: GPUI build deps and the oracle tools (apt; prints the list elsewhere)")]
[group('setup')]
setup-system:
    #!/usr/bin/env bash
    set -euo pipefail
    pkgs="build-essential clang lld cmake pkg-config libfontconfig-dev \
      libwayland-dev libx11-xcb-dev libxkbcommon-x11-dev libvulkan1 libasound2-dev \
      libglib2.0-dev libssl-dev libzstd-dev xdg-desktop-portal xdg-desktop-portal-gtk \
      qpdf poppler-utils mupdf-tools libtiff-tools default-jre-headless python3 curl unzip gpg"
    if command -v apt-get >/dev/null; then
      sudo apt-get install -y $pkgs
    else
      echo "Not Debian or Ubuntu: install the equivalents of"; echo "  $pkgs"
      echo "(docs/building.md has Windows; macOS needs Xcode's tools.)"
    fi

# The pinned toolchain (rust-toolchain.toml, with wasm32-wasip2), the
# Windows target for `just check-windows`, and nightly with cargo-fuzz for
# `just fuzz`.
[doc("The pinned toolchain, the Windows target, nightly and cargo-fuzz")]
[group('setup')]
setup-rust:
    rustup show active-toolchain || rustup toolchain install
    rustup target add x86_64-pc-windows-msvc
    rustup toolchain list | grep '^nightly' >/dev/null || rustup toolchain install nightly --profile minimal
    cargo fuzz --version >/dev/null 2>&1 || cargo install cargo-fuzz --locked

# PDFium and the OCR data (Tesseract's language data, PaddleOCR's models)
# into vendor/; `just fetch --uvdoc` adds the curled-page model.
[doc("PDFium and the OCR data into vendor/ (`just fetch --uvdoc` adds the curled-page model)")]
[group('setup')]
fetch *args:
    scripts/fetch-pdfium.sh
    scripts/fetch-ocr-data.sh {{args}}

# The oracle tools that aren't system packages, under $SHEAF_TOOLS
# (~/.local/share/sheaf-tools): pdf-lint, pdf.js, veraPDF, and uv for the
# corpus tools' Python.
[doc("pdf-lint, pdf.js, veraPDF and uv under $SHEAF_TOOLS (~/.local/share/sheaf-tools)")]
[group('setup')]
setup-oracles: setup-pdf-lint setup-pdfjs setup-verapdf setup-uv

# pdf-lint {{pdf_lint_version}} from sheaf-pdf/pdf-lint, verified against its
# release SHA-256, for the Arlington tests (tests/arlington.rs).
[doc("pdf-lint, verified by SHA-256, for the Arlington tests")]
[group('setup')]
setup-pdf-lint:
    #!/usr/bin/env bash
    set -euo pipefail
    if [ -x "{{pdf_lint}}" ] && [[ "$("{{pdf_lint}}" --version 2>/dev/null | head -1)" == "pdf-lint {{pdf_lint_version}}" ]]; then
      echo "pdf-lint {{pdf_lint_version}} is installed"; exit 0
    fi
    case "$(uname -s)-$(uname -m)" in
      Linux-x86_64)   plat="linux-x86_64" ;;
      Linux-aarch64)  plat="linux-aarch64" ;;
      Darwin-arm64)   plat="macos-arm64" ;;
      Darwin-x86_64)  plat="macos-x86_64" ;;
      *) echo "unsupported platform for setup-pdf-lint: $(uname -s)-$(uname -m)"; exit 1 ;;
    esac
    name="pdf-lint-{{pdf_lint_version}}-$plat"
    file="$name.tar.gz"
    base="https://github.com/sheaf-pdf/pdf-lint/releases/download/pdf-lint-v{{pdf_lint_version}}"
    work=$(mktemp -d); trap 'rm -rf "$work"' EXIT
    curl -sSfL -o "$work/$file" "$base/$file"
    curl -sSfL -o "$work/$file.sha256" "$base/$file.sha256"
    want=$(awk '{print $1}' "$work/$file.sha256")
    if command -v sha256sum >/dev/null 2>&1; then got=$(sha256sum "$work/$file" | awk '{print $1}')
    else got=$(shasum -a 256 "$work/$file" | awk '{print $1}'); fi
    [ "$got" = "$want" ] || { echo "$file SHA-256 $got != $want"; exit 1; }
    tar -xzf "$work/$file" -C "$work"
    rm -rf "{{tools_dir}}/pdf-lint"
    mkdir -p "{{tools_dir}}"
    mv "$work/$name" "{{tools_dir}}/pdf-lint"
    "{{pdf_lint}}" --version

# pdfjs-dist {{pdfjs_version}} for the interop tests (needs node and npm,
# e.g. through nvm).
[doc("pdfjs-dist for the interop tests (needs node and npm)")]
[group('setup')]
setup-pdfjs:
    #!/usr/bin/env bash
    set -euo pipefail
    nvm_bin=$(ls -d "$HOME"/.nvm/versions/node/*/bin 2>/dev/null | tail -1 || true)
    [ -n "$nvm_bin" ] && export PATH="$nvm_bin:$PATH"
    command -v npm >/dev/null || { echo "npm missing: install node (nvm, or the nodejs package) first"; exit 1; }
    mkdir -p "{{tools_dir}}/pdfjs"
    npm install --prefix "{{tools_dir}}/pdfjs" --ignore-scripts --no-audit --no-fund "pdfjs-dist@{{pdfjs_version}}"
    ls "{{pdfjs}}"

# veraPDF {{verapdf_version}} from software.verapdf.org, its signature checked
# against the release key in a throwaway keyring, installed unattended.
[doc("veraPDF, its signature verified, installed unattended (needs java)")]
[group('setup')]
setup-verapdf:
    #!/usr/bin/env bash
    set -euo pipefail
    if [ -x "{{verapdf}}" ] && [[ "$("{{verapdf}}" --version 2>/dev/null)" == *"veraPDF {{verapdf_version}}"* ]]; then
      echo "veraPDF {{verapdf_version}} is installed"; exit 0
    fi
    command -v java >/dev/null || { echo "java missing: just setup-system"; exit 1; }
    work=$(mktemp -d); trap 'rm -rf "$work"' EXIT
    name=verapdf-greenfield-{{verapdf_version}}-installer.zip
    base=https://software.verapdf.org/releases/$(echo {{verapdf_version}} | cut -d. -f1-2)
    curl -sSfL -o "$work/$name" "$base/$name"
    curl -sSfL -o "$work/$name.asc" "$base/$name.asc"
    export GNUPGHOME="$work/gnupg"; mkdir -m 700 "$GNUPGHOME"
    curl -sSf "https://keyserver.ubuntu.com/pks/lookup?op=get&search=0x{{verapdf_key}}" | gpg -q --import
    status=$(gpg --status-fd 1 --verify "$work/$name.asc" "$work/$name" 2>/dev/null || true)
    [[ "$status" == *"VALIDSIG {{verapdf_key}}"* ]] \
      || { echo "veraPDF's signature doesn't verify with key {{verapdf_key}}"; exit 1; }
    (cd "$work" && unzip -q "$name")
    jar=$(find "$work" -name 'verapdf-izpack-installer-*.jar' | head -1)
    printf 'INSTALL_PATH=%s\n' "$(dirname "{{verapdf}}")" > "$work/install.properties"
    java -Djava.awt.headless=true -jar "$jar" -options "$work/install.properties"
    "{{verapdf}}" --version | head -1

# uv, which runs the corpus tools with their own Python packages (pyHanko,
# pikepdf, pypdfium2).
[doc("uv, which runs the corpus tools with their Python packages")]
[group('setup')]
setup-uv:
    command -v uv >/dev/null || cargo install --locked uv

# What this machine has: toolchains, vendor files, oracle tools, the corpus.
[doc("What this machine has: toolchains, vendor files, oracle tools, the corpus")]
[group('setup')]
doctor:
    #!/usr/bin/env bash
    set -uo pipefail
    have() { if command -v "$1" >/dev/null 2>&1; then printf '  %-16s %s\n' "$1" "$("$@" 2>&1 | head -1)"; else printf '  %-16s MISSING\n' "$1"; fi; }
    echo "Rust:"; have rustc --version; have cargo --version
    printf '  %-16s %s\n' "targets" "$(rustup target list --installed | tr '\n' ' ')"
    printf '  %-16s %s\n' "nightly" "$(rustup toolchain list | grep -c nightly) installed"
    have cargo-fuzz --version
    echo "Vendor:"
    for d in vendor/pdfium vendor/ocr; do printf '  %-16s %s\n' "$d" "$( [ -e "$d" ] && echo present || echo 'MISSING (just fetch)')"; done
    echo "Oracles (docs/oracles.md):"
    have qpdf --version; have pdftotext -v; have mutool -v; have tiffinfo; have java -version
    if [ -x "{{pdf_lint}}" ]; then printf '  %-16s %s\n' "pdf-lint" "$("{{pdf_lint}}" --version 2>/dev/null | head -1) ({{pdf_lint}})"
    elif command -v pdf-lint >/dev/null 2>&1; then printf '  %-16s %s\n' "pdf-lint" "$(pdf-lint --version 2>/dev/null | head -1)"
    else printf '  %-16s %s\n' "pdf-lint" "MISSING (just setup-pdf-lint)"; fi
    nvm_bin=$(ls -d "$HOME"/.nvm/versions/node/*/bin 2>/dev/null | tail -1 || true)
    [ -n "$nvm_bin" ] && export PATH="$nvm_bin:$PATH"
    have node --version
    printf '  %-16s %s\n' "pdf.js" "$( [ -e "{{pdfjs}}" ] && echo "{{pdfjs}}" || echo 'MISSING (just setup-pdfjs)')"
    printf '  %-16s %s\n' "veraPDF" "$( [ -x "{{verapdf}}" ] && "{{verapdf}}" --version 2>/dev/null | head -1 || echo 'MISSING (just setup-verapdf)')"
    have uv --version
    echo "Corpus:"
    corpus=${SHEAF_CORPUS:-$(dirname "{{justfile_directory()}}")/pdf-corpus}
    if [ -e "$corpus/forms/manifest.tsv" ]; then printf '  %-16s %s\n' "pdf-corpus" "$corpus"
    else printf '  %-16s %s\n' "pdf-corpus" "MISSING at $corpus: the corpus tests need it (fetch-pdf-corpus fetch --out $corpus; docs/building.md)"; fi
    echo "Test memory cap:"
    if [ "$(uname -s)" = Linux ] && systemctl --user show-environment >/dev/null 2>&1; then echo "  on (scripts/test-capped.sh, MemoryMax ${SHEAF_TEST_MEMORY_MAX:-48G})"; else echo "  off (no user systemd)"; fi

# ----- What CI runs -------------------------------------------------------

# Everything .github/workflows/ci.yml checks, but the Linux binary checks.
[doc("Everything CI checks but the Linux binary checks")]
[group('ci')]
ci: fmt-check locks boundaries clippy test ocr-smoke plugins no-defaults

# Format the code.
[group('ci')]
fmt:
    cargo fmt --all

[doc("cargo fmt --check")]
[group('ci')]
fmt-check:
    cargo fmt --all -- --check

# Both lockfiles match their manifests: the workspace's, and the fuzz
# workspace's (fuzz/Cargo.lock), which the nightly fuzz job regenerates
# without saying so when a dependency changes.
[doc("Both lockfiles (the workspace's and fuzz/'s) are current")]
[group('ci')]
locks:
    cargo metadata --locked --format-version 1 > /dev/null
    cargo metadata --locked --format-version 1 --manifest-path fuzz/Cargo.toml > /dev/null

# The crate boundaries CI holds: sheaf-image pixels only, sheaf-util byte
# helpers, sheaf-edit types only (and building for WASI).
[doc("The crate boundaries CI holds (sheaf-image, sheaf-util, sheaf-edit)")]
[group('ci')]
boundaries:
    #!/usr/bin/env bash
    set -euo pipefail
    bad=$(cargo tree -p sheaf-image -e normal --prefix none | awk '{print $1}' | sort -u \
      | grep -E '^(lopdf|pdf-.*|pdfium-render|gpui-pre|tesseract|oar-ocr|ort|sheaf-(doc|render|ocr|ops|ui|app|plugin|print))$' || true)
    [ -z "$bad" ] || { echo "sheaf-image depends on: $bad"; exit 1; }
    deps=$(cargo tree -p sheaf-util -e normal --prefix none | awk '{print $1}' | sort -u | xargs)
    [ "$deps" = "sheaf-util twox-hash" ] || { echo "sheaf-util depends on: $deps"; exit 1; }
    bad=$(cargo tree -p sheaf-edit -e normal --prefix none | awk '{print $1}' | sort -u \
      | grep -E '^sheaf-' | grep -vx 'sheaf-edit' || true)
    [ -z "$bad" ] || { echo "sheaf-edit depends on: $bad"; exit 1; }
    for pkg in pdf-objects pdf-filters pdf-crypt pdf-structure pdf-content-streams pdf-fonts pdf-gfx pdf-forms pdf-semantics; do
      bad=$(cargo tree --workspace -e all -i "$pkg" --prefix depth 2>/dev/null | awk '/^1sheaf-/ {print $1}' | sed 's/^1//' | sort -u \
        | grep -vx 'sheaf-doc' || true)
      [ -z "$bad" ] || { echo "crates outside sheaf-doc depend on $pkg directly: $bad"; exit 1; }
    done
    lopdf_present=$(cargo tree --workspace -e all --prefix none | awk '{print $1}' | grep -x 'lopdf' || true)
    [ -z "$lopdf_present" ] || { echo "lopdf is still in the dependency tree"; exit 1; }
    insecure=$(cargo tree --workspace -e normal -f '{p} {f}' | grep -F 'insecure-test-writer' || true)
    [ -z "$insecure" ] || { echo "insecure-test-writer enabled in normal dependencies: $insecure"; exit 1; }
    cargo build -p sheaf-edit --target wasm32-wasip2

[doc("Workspace clippy, -D warnings")]
[group('ci')]
clippy:
    cargo clippy --workspace --all-targets -- -D warnings

# The workspace's tests, each binary under the memory cap; arguments go to
# cargo test (`just test -p sheaf-doc forms::`).
[doc("The tests under the memory cap: the workspace, or what the arguments name (`just test -p sheaf-doc forms::`)")]
[group('ci')]
test *args:
    scripts/test-capped.sh cargo test {{ if args == "" { "--workspace" } else { args } }}

# The OCR engines' smoke tests.
[group('ci')]
ocr-smoke:
    scripts/test-capped.sh cargo test -p sheaf-ocr --features tesseract,oar -- --ignored

# The bundled plugins and the host's test plugin: fmt, clippy for WASI,
# their unit tests, and the staged build.
[doc("The plugins: fmt, WASI clippy, unit tests and the staged build")]
[group('ci')]
plugins:
    #!/usr/bin/env bash
    set -euo pipefail
    for dir in plugins/*/ crates/sheaf-plugin/tests/fixtures/test-plugin/; do
      (cd "$dir" && cargo fmt -- --check && cargo clippy --target wasm32-wasip2 -- -D warnings)
    done
    for dir in plugins/*/; do (cd "$dir" && cargo test); done
    scripts/build-plugins.sh

# Build the bundled plugins into target/plugins, where debug builds find
# them.
[doc("Build the bundled plugins into target/plugins")]
[group('ci')]
plugins-build:
    scripts/build-plugins.sh

# sheaf-app without the OCR engines and plugins: it builds, and clippy is
# clean (backlog B6).
[doc("sheaf-app without OCR engines or plugins: builds, clippy clean")]
[group('ci')]
no-defaults:
    cargo check -p sheaf-app --no-default-features
    cargo clippy -p sheaf-app --all-targets --no-default-features -- -D warnings

# ----- Other platforms, from Linux ---------------------------------------

# Clippy sheaf-app's tests as macOS and Windows see them (its GPUI tests
# are Linux-only). Edits files and restores them: nothing else may edit
# sheaf-app meanwhile.
[doc("Clippy sheaf-app's tests as macOS and Windows see them (edits and restores files)")]
[group('platforms')]
clippy-nonlinux:
    scripts/nonlinux-clippy.sh

# Clippy for Windows the crates with no C build dependencies (sheaf-cli,
# sheaf-app and sheaf-plugin need a Windows toolchain: CI checks those).
[doc("Windows clippy for the crates without C build dependencies")]
[group('platforms')]
check-windows:
    rustup target list --installed | grep -x x86_64-pc-windows-msvc >/dev/null || rustup target add x86_64-pc-windows-msvc
    cargo clippy --target x86_64-pc-windows-msvc -p sheaf-util -p sheaf-doc -p sheaf-ops --all-targets -- -D warnings

# ----- What CI can't run --------------------------------------------------

# Before a commit, the checks CI can't run: no conflict markers, the plan's
# links and tables, fmt, the workspace with the corpus, interop, the
# release properties, no new proptest seeds, and the macOS/Windows clippy.
# Gates run apart, on a quiet machine (`just gates`). Logs and a summary in
# target/just/.
[doc("Before a commit: the checks CI can't run (logs in target/just/); gates apart")]
[group('local')]
precommit: (_steps "precommit" "markers plan-check fmt-check locks corpus interop properties seeds clippy-nonlinux check-windows")

# Before a PR merges: the ignored corpus tiers, in full (append_corpus's,
# the export tier, realize_corpus's pixels and predicate, PDFium's resave
# of a watermark, content's tokens and the content interpreter against
# PDFium, codes over scans/, Arlington over what sheaf writes, and the
# size of an unsigned Save As beside qpdf's, and every text field's
# appearance beside PDFium's). Two hours or more, the interpreter's tiers
# about one of them.
[doc("Before a PR merges: every ignored corpus tier (two hours or more)")]
[group('local')]
premerge: (_steps "premerge" "tier-append tier-exports tier-realize tier-watermark tier-lexer tier-interp tier-codes tier-arlington tier-size tier-fields tier-pages _corpus-ran")

[doc("The plan agrees with itself: tables against files, status lines, links, size budgets (AGENTS.md)")]
[group('local')]
plan-check:
    python3 scripts/check-plan.py

[doc("No merge-conflict markers in tracked or untracked files (git grep; binaries and ignored files skipped)")]
[group('local')]
markers:
    ! git grep -nIE --untracked '^(<<<<<<<|=======$|>>>>>>>)'

# The workspace's tests with ../pdf-corpus (or $SHEAF_CORPUS): fails if a
# corpus test skipped.
[doc("The workspace tests with the corpus; fails if a corpus test skipped")]
[group('local')]
corpus:
    #!/usr/bin/env bash
    set -uo pipefail
    mkdir -p "{{logs}}"
    scripts/test-capped.sh cargo test --workspace --no-fail-fast 2>&1 | tee "{{logs}}/corpus-tests.log"
    rc=${PIPESTATUS[0]}
    if grep -q 'skipped: no corpus' "{{logs}}/corpus-tests.log"; then echo "a corpus test skipped: no corpus found"; exit 1; fi
    exit "$rc"

# The interop tests: sheaf's output read by qpdf and pdf.js, and its text
# field appearances drawn by poppler's pdftoppm ($SHEAF_PDFTOPPM, else on
# the PATH). A test whose tool is missing says so ("skipped ...") and
# passes: read the output on a machine that may lack one.
[group('local')]
interop:
    #!/usr/bin/env bash
    set -euo pipefail
    nvm_bin=$(ls -d "$HOME"/.nvm/versions/node/*/bin 2>/dev/null | tail -1 || true)
    [ -n "$nvm_bin" ] && export PATH="$nvm_bin:$PATH"
    SHEAF_PDFJS="{{pdfjs}}" scripts/test-capped.sh cargo test -p sheaf-ops --test interop -- --include-ignored --nocapture

# The release-build property tests: Append and Rewrite reopen alike over
# 1,024 runs; history rebuilds every version over 4,096.
[doc("The release-build property tests")]
[group('local')]
properties:
    scripts/test-capped.sh cargo test -p sheaf-doc --release --lib -- --ignored append_rewrite_reopen_alike_1024 prims_rebuild_every_version_4096

# No proptest seed beyond the one real past failure kept on purpose: a
# seed is what a failed property (or a deliberate fault check) leaves.
[doc("No new proptest seeds")]
[group('local')]
seeds:
    ! git status --short -uall -- '*proptest-regressions*' | grep -v 'append_rewrite_tests.txt' | grep .

# Regenerate the CLI's golden files after a deliberate change, then show
# what moved.
[doc("Regenerate the CLI golden files, then show what moved")]
[group('local')]
golden-update:
    SHEAF_GOLDEN_UPDATE=1 scripts/test-capped.sh cargo test -p sheaf-cli --test golden
    git diff --stat -- crates/sheaf-cli/tests/golden

# ----- Corpus tiers (ignored by default) ----------------------------------

# append_corpus's tiers: deleted means deleted, crop, watermark, header and
# footer, OCR over the forms and over scans/, and the pool sample's
# linearization against qpdf before and after an Append.
[doc("append_corpus's tiers (deleted means deleted, crop, watermark, header, OCR, linearization)")]
[group('corpus')]
tier-append:
    scripts/test-capped.sh cargo test -p sheaf-ops --test append_corpus -- --ignored --nocapture

# Every form extracted, clean-copied and flattened, and opened again.
[group('corpus')]
tier-exports:
    scripts/test-capped.sh cargo test -p sheaf-ops --test exports -- --ignored --nocapture

# An unsigned Save As of every IRS form and every pool sample file: each
# output's size against its source's, beside qpdf's with object streams
# unpacked and packed again, a row per file and the growth by group (the
# log, target/just/tier-size.log, keeps the rows), sheaf's held to bounds
# by group; each output is packed or not as its source used object
# streams, reopens and passes qpdf --check no worse than its source.
# About 30 s.
[doc("An unsigned Save As's size against the source's and qpdf's, bounded by group, IRS forms and the pool sample (SHEAF_SIZE_ONLY narrows it)")]
[group('corpus')]
tier-size:
    scripts/test-capped.sh cargo test -p sheaf-ops --test rewrite_size -- --ignored --nocapture

# realize_corpus: the predicate and pixels over the pool, forms and scans/.
[doc("realize_corpus: the predicate and pixels over the pool, forms and scans/")]
[group('corpus')]
tier-realize:
    scripts/test-capped.sh cargo test -p sheaf-ops --test realize_corpus -- --ignored --nocapture

# Every corpus form's text fields filled (M10 slice 6): each widget's
# appearance made or unmade by a kind, an overlay's forms the save's, and
# each made one drawn as PDFium draws its own for the same value, inside
# its box, or a row of a class whose count the test holds, with how many
# it filled, made and compared. In release;
# SHEAF_FIELDS_ONLY=<paths, comma-separated> narrows it, its counts then
# printed, not held.
[doc("Every corpus text field filled: its appearance beside PDFium's, each difference classed (SHEAF_FIELDS_ONLY narrows it)")]
[group('corpus')]
tier-fields:
    scripts/test-capped.sh cargo test -p sheaf-ops --release --test fields_corpus -- --ignored --nocapture

# Every page of the scans, signatures and conformance suites and the pool
# sample as the page table has it, against PDFium's count and sizes (M18
# slice 21), beside source_corpus's test of forms/, which runs every
# time: the files PDFium counts past their trees and those it numbers
# otherwise are held by name, each to PDFium's own answer. A few minutes.
[doc("The page table against PDFium's counts and sizes over the scans, signatures and conformance suites and the pool sample")]
[group('corpus')]
tier-pages:
    scripts/test-capped.sh cargo test -p sheaf-ops --test source_corpus -- --ignored --nocapture

# PDFium's resave of a watermark.
[group('corpus')]
tier-watermark:
    scripts/test-capped.sh cargo test -p sheaf-ops --test watermark -- --ignored --nocapture

# Content's tokens, as sheaf reads a page through pdf-content-streams,
# against PDFium's page objects over the pool sample.
[group('corpus')]
tier-lexer:
    scripts/test-capped.sh cargo test -p sheaf-render --test content_lexer -- --ignored --nocapture

# Every page of scans/ drawn at 150 dpi and read for codes: no sheaf
# separator and no patch code; the other barcodes listed (M5 slice 3).
[doc("Codes over scans/: no separator or patch code found (SHEAF_CODES_ONLY narrows it)")]
[group('corpus')]
tier-codes:
    scripts/test-capped.sh cargo test -p sheaf-ops --test codes -- --ignored --nocapture

# What sheaf writes on every form held to the Arlington PDF Model with
# pdf-lint (backlog F22): a mark, a header and footer and recognized text,
# saved as planned, then a Rewrite, each save's objects checked; the
# form's own aren't. SHEAF_ARLINGTON_ONLY=<paths, comma-separated> narrows
# it to a few forms; SHEAF_ARLINGTON_SAVES=<file> writes each save's
# counts there, a line each, to set one run beside another.
[doc("pdf-lint over what sheaf writes on every form: a mark, a header and footer, recognized text, then a Rewrite (SHEAF_ARLINGTON_ONLY narrows it, SHEAF_ARLINGTON_SAVES writes each save's counts)")]
[group('corpus')]
tier-arlington:
    scripts/test-capped.sh cargo test -p sheaf-ops --test arlington -- --ignored --nocapture

# The content interpreter against PDFium over every form, the pool sample,
# scans/ and the PDF/UA files: every page interpreted within the caps,
# each glyph's box against PDFium's character, and every page written
# again rendering the same. In release, about 55 minutes;
# SHEAF_INTERP_ONLY=<paths, comma-separated> narrows it to a few files.
# Not here: the standard 14 widths test, ignored too, holds only where
# PDFium draws in its own standard 14 faces, as it does where the system
# has no font named Helvetica, Arial or Times New Roman (a bare Linux
# box). Run it by hand there: `cargo test -p sheaf-render --test
# content_interp -- --ignored --exact
# standard_14_widths_are_pdfium_s_faces_but_where_guessed`.
[doc("The content interpreter's tiers against PDFium, in release (about 55 min)")]
[group('corpus')]
tier-interp:
    scripts/test-capped.sh cargo test -p sheaf-render --release --test content_interp -- --ignored --nocapture --exact every_page_is_interpreted_within_the_caps glyph_boxes_are_pdfium_characters_on_every_page pages_written_again_render_the_same

# What every page of the files tier-interp reads shows, a line a page (its
# glyphs counted, a checksum of each glyph's code, text and box, and one
# of the page's text bag), written to `out`. It asserts nothing: it is
# for setting a change beside what was there. Run it before and after a
# change to what reads fonts or interprets a page (a move of the PDF
# layer's pin too), over the same corpus, and `diff` the two files: a
# line that differs is a page whose glyphs or text moved, and
# SHEAF_GLYPH_DUMP_ONLY=<a corpus path> prints that file's records in
# full, to read the two runs side by side. Two runs over the same code
# write the same file. In release, under a minute once built.
[doc("What every corpus page shows, a line a page, written to a file: diff two runs across a change to fonts or the interpreter")]
[group('corpus')]
glyph-dump out:
    SHEAF_GLYPH_DUMP="{{ absolute_path(out) }}" scripts/test-capped.sh cargo test -p sheaf-doc --release --lib -- --ignored --nocapture --exact content::corpus::glyphs_and_text_dump

# What every page of the files glyph-dump reads does as it's interpreted,
# a line a page, written to `out`: its interpretation's summary, field by
# field; a checksum of every event (each operator, path, image, shading,
# form, marked range and string shown, by the fields it names, in the
# state it's in, but a show's string and glyphs, which glyph-dump has);
# one of the page written again; `ok` or the class of what failed
# (`content`, `form-stream`, `limit:<cap>`, `not-finite`, `unwritable:<why>`,
# `unreadable:<why>`, `damaged` where forms are drawn through a damaged
# object, `page-stream`, `capped-inline`, `other`; `encode:` and one of
# those where only writing it again did; `unread` or `unopened` where
# there was nothing to interpret); and whether the page written again,
# interpreted again, draws the same (`same`, `differs`, or `-` where it
# isn't written again). It asserts nothing:
# run it before and after a change to the interpreter, over the same
# corpus, and `diff` the two files; SHEAF_EVENTS_DUMP_ONLY=<a corpus
# path> prints that file's records in full. Two runs over the same code
# write the same file.
[doc("What every corpus page's interpretation does, a line a page, written to a file: diff two runs across a change to the interpreter")]
[group('corpus')]
events-dump out:
    SHEAF_EVENTS_DUMP="{{ absolute_path(out) }}" scripts/test-capped.sh cargo test -p sheaf-doc --release --lib -- --ignored --nocapture --exact content::corpus::events_dump

# What every page of the files glyph-dump reads, and every form XObject a
# page draws (once a file), reads as content, a line each, written to
# `out`, numbers and checksums only: `ok` or the class of what stopped its
# tokens (`unread:stream`, `unread:too-large` or `unopened` where nothing
# was read, `resources` where its color spaces weren't); its tokens, as
# pdf-content-streams reads them, their ranges, its operators, its `q`/`Q`
# balance, the resource names it uses,
# its marked ranges, the names and dictionaries read from its operands,
# and what the private names' walk hears of it; and for a page, its
# blocks, field by field, and what's cut and written: each pagination
# artifact's cuts, the bytes a splice of them makes, its list written
# again, and the sets a strip takes out together. It asserts nothing: run
# it before and after a change to what reads content, over the same
# corpus, and `diff` the two files; SHEAF_CONTENT_DUMP_ONLY=<a corpus
# path> prints that file's records in full. Two runs over the same code
# write the same file.
[doc("What every corpus page's and form's content reads as, a line each, written to a file: diff two runs across a change to what reads content")]
[group('corpus')]
content-dump out:
    SHEAF_CONTENT_DUMP="{{ absolute_path(out) }}" scripts/test-capped.sh cargo test -p sheaf-doc --release --lib -- --ignored --nocapture --exact content::corpus::content_dump

# Every annotation of the files glyph-dump reads, as sheaf reads, flattens
# and writes it, written to `out`, numbers and checksums only: a line a
# file, and for each slot of a page's `/Annots`, three. What the open
# reads of it (`read`): one of sheaf's marks (its JSON), a mark's
# companion (`part`), another tool's annotation as pdf-annots reads it
# (`foreign`: every field it's listed by, texts as checksums, and the mark
# it converts to, as a checksum of its JSON, or why it doesn't), one
# pdf-annots refused (`refused`: why), or one passed over (`passed`: what
# the entry is). What a flatten reads of it through pdf-annots
# (`flatten`): its flags, the state it shows, and the form it shows by its
# value and by its state alone, each with its box, its matrix and the
# matrix that places it. And what a save writes of the mark it is
# (`make`): its dictionary, its appearance's box, the resources it binds
# and its content, and each companion's. It asserts nothing: run it
# before and after a change to what reads, flattens or draws annotations,
# or a move of the PDF layer's pin, over the same corpus, and `diff` the
# two files; SHEAF_ANNOTS_DUMP_ONLY=<a corpus path> prints that file's
# records in full. Two runs over the same code write the same file.
[doc("Every corpus annotation as sheaf reads, flattens and writes it, a line each, written to a file: diff two runs across a change to annotations")]
[group('corpus')]
annots-dump out:
    SHEAF_ANNOTS_DUMP="{{ absolute_path(out) }}" scripts/test-capped.sh cargo test -p sheaf-doc --release --lib -- --ignored --nocapture --exact annot::corpus::annots_dump

# Every text widget of every form in the corpus's manifest, as sheaf reads
# it and as pdf-forms' read layer does, written to `out`, numbers, names
# and checksums only: a line a file (its form's class, and how many text
# widgets the two read alike, read otherwise, or only one of them reads),
# and one a text widget, by its page, slot and object: the plain data
# sheaf's model gives of it (its value's length and checksum, never the
# value; flags, `/MaxLen`, justification, box, `/MK` colors and rotation,
# border, and its effective `/DA` parsed), `=` where the family's reading
# is the same or the parts that differ and the family's of each, the font
# its `/DA` names (by reference, direct, missing and the face it falls
# back to; its kind, program, `/BaseFont` and `/Encoding`), and the
# appearance made of it for a sample value from the family's reading and
# from sheaf's (its size, whether it overflowed, its content's length and
# checksum, or why none is made), with what of the two differs. It
# asserts nothing: run it before and after a change to what
# reads a form or makes a text field's appearance, or a move of the PDF
# layer's pin, over the same corpus, and `diff` the two files;
# SHEAF_FIELDS_DUMP_ONLY=<a corpus path> prints that file's lines. Two
# runs over the same code write the same file.
[doc("Every corpus text widget as sheaf and pdf-forms read it, and the appearance made of it, a line each, written to a file: diff two runs across a change to forms")]
[group('corpus')]
fields-dump out:
    SHEAF_FIELDS_DUMP="{{ absolute_path(out) }}" scripts/test-capped.sh cargo test -p sheaf-doc --release --lib -- --ignored --nocapture --exact forms::dump::fields_dump

# What sheaf reads of the common structures of every file of the forms,
# scans, signatures and conformance manifests and the pool's sample,
# written to `out`, counts and checksums only: a line a file, a column a
# structure, each `name=count:checksum`. The page table (each page's
# object, repeat, boxes, rotation, user unit, resources, contents,
# annotation entries, struct parents and parents), `/PageLabels`' ranges,
# the attachments, the whole `/Names /Dests` tree, the bookmarks, the
# layers, a portfolio's fields, the form's terminal fields (full names,
# tooltips and their values' lengths, never a value), the document scripts
# as a combine copies them, and what its XMP claims. A file that doesn't
# open is one line, and why. It asserts nothing: run it before and after
# a change to what reads page or name trees, text strings or XMP, or a
# move of the PDF layer's pin, over the same corpus, and `diff` the two
# files; SHEAF_STRUCTURES_DUMP_ONLY=<a corpus path> prints that file's
# records in full. Two runs over the same code write the same file.
[doc("What sheaf reads of every corpus file's page tree, labels, attachments, dests, outline, layers, portfolio, fields, scripts and claims, a line each, written to a file: diff two runs across a change to them")]
[group('corpus')]
structures-dump out:
    SHEAF_STRUCTURES_DUMP="{{ absolute_path(out) }}" scripts/test-capped.sh cargo test -p sheaf-doc --release --lib -- --ignored --nocapture --exact source::dump::structures_dump

# ----- Gates ---------------------------------------------------------------

# Wait until the machine is quiet: the 1-minute load under
# $SHEAF_QUIET_LOAD (4) on two checks 30 s apart. Timing gates need it.
[doc("Wait until the 1-minute load is under $SHEAF_QUIET_LOAD (4) twice, 30 s apart")]
[group('gates')]
quiet:
    #!/usr/bin/env bash
    set -euo pipefail
    limit=${SHEAF_QUIET_LOAD:-4}
    while true; do
      a=$(cut -d' ' -f1 /proc/loadavg); sleep 30; b=$(cut -d' ' -f1 /proc/loadavg)
      if awk -v a="$a" -v b="$b" -v l="$limit" 'BEGIN { exit !(a < l && b < l) }'; then echo "quiet: load $a, $b"; exit 0; fi
      echo "waiting for quiet: load $a, $b (under $limit)"
    done

# The 7k gates (crates/sheaf-ops/tests/gate_7k.rs), in release, one at a
# time on a quiet machine; names narrow them (`just gates
# gates_on_the_heavy_7k`), each after its own quiet check. Fixtures are made
# in $SHEAF_GATE_DIR, else target/gate-7k/, when missing.
[doc("The 7k gates in release on a quiet machine; names narrow them")]
[group('gates')]
gates *names:
    #!/usr/bin/env bash
    set -uo pipefail
    cargo test -p sheaf-ops --release --test gate_7k --no-run
    if [ -z "{{names}}" ]; then
      just quiet
      exec scripts/test-capped.sh cargo test -p sheaf-ops --release --test gate_7k -- --ignored --nocapture --test-threads=1
    fi
    fail=0
    for name in {{names}}; do
      just quiet
      scripts/test-capped.sh cargo test -p sheaf-ops --release --test gate_7k -- --ignored --nocapture --exact "$name" || fail=1
    done
    exit "$fail"

# ----- Fuzzing -------------------------------------------------------------

# A fuzz target for `secs` seconds, as the nightly CI job runs it: seeded
# from the stable test's fixtures, its corpus kept in fuzz/corpus/. Targets:
# lazy_reader page_tree sheaf_annot splice interp op_json
# sheaf_generators.
[doc("A fuzz target for `secs` seconds, as nightly CI runs it")]
[group('fuzz')]
fuzz target secs="300":
    SHEAF_FUZZ_SEEDS="{{justfile_directory()}}/fuzz/corpus/{{target}}" SHEAF_FUZZ_TARGET={{target}} cargo test -p sheaf-doc --lib -- --ignored --exact source::fuzz::tests::write_fuzz_seeds
    cargo +nightly fuzz run --target x86_64-unknown-linux-gnu {{target}} -- -max_total_time={{secs}} -rss_limit_mb=2048 -timeout=10 -dict=fuzz/{{ if target == "op_json" { "op.dict" } else { "pdf.dict" } }}

# Every fuzz target for `secs` seconds each.
[group('fuzz')]
fuzz-all secs="60":
    for t in {{fuzz_targets}}; do just fuzz "$t" {{secs}}; done

# ----- Release and housekeeping --------------------------------------------

# A release bundle for this platform (scripts/package.sh).
[group('release')]
package *args:
    scripts/package.sh {{args}}

# How much target/ holds, by profile.
[group('release')]
target-size:
    du -sh target target/* 2>/dev/null | sort -h | tail -8

# Fails if a corpus tier's log in target/just/ says it skipped for want of
# a corpus.
_corpus-ran:
    ! grep -l 'skipped: no corpus' {{logs}}/tier-*.log

# Runs recipes in order, keeping on past a failure, each one's output in
# target/just/<recipe>.log, and a summary at the end.
_steps name recipes:
    #!/usr/bin/env bash
    set -uo pipefail
    mkdir -p "{{logs}}"
    summary="{{logs}}/{{name}}-summary.txt"; : > "$summary"; failed=0
    for r in {{recipes}}; do
      start=$(date +%s)
      if just "$r" > "{{logs}}/$r.log" 2>&1; then rc=0; else rc=$?; failed=1; fi
      echo "$r rc=$rc $(( $(date +%s) - start ))s" | tee -a "$summary"
    done
    [ "$failed" = 0 ] && echo "{{name}}: all passed" || echo "{{name}}: failures above; logs in {{logs}}/"
    exit "$failed"
