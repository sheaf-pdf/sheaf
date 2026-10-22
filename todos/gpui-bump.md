# GPUI bump: gpui-pre 0.3.9 → next snapshot

Status: **planned** (2026-10-21). Waiting for the next gpui-pre release;
nothing in the tree has changed yet. This memo records what the bump
needs, checked against Zed main, and the steps. The procedure itself is
the README's "Updating GPUI".

## Where things stand

- crates.io's newest gpui-pre is still **0.3.9** (2026-10-12,
  zed@`93d7456`), the version sheaf pins.
- Longbridge's weekly run on 2026-10-19 failed on Zed commit
  `56934ec`. Zed's workspace now takes `wgpu` from git, and crates.io
  can't publish a git dependency. gpui-kit #3600 fixed the script, and
  the re-run (Zed `b4ecc45`) only "resumed" 0.3.9: it published the one
  crate still missing at that version, `gpui-pre-ios`. sheaf doesn't
  build iOS, so it changes nothing here, but `gpui-pre-ios` 0.3.9 comes
  from a different Zed commit than the other 0.3.9 crates.
- The next scheduled run is Sunday 2026-10-26 (about 00:35 UTC). It
  should publish **0.3.10** from Zed main as of then.

## What's in it

Checked against Zed main `5adae44` (2026-10-21): 125 commits past
`93d7456`, 17 of them in the GPUI crates (+5.8k/−0.3k lines, about 3.9k
of that the new iOS backend). Before bumping, read the commits between
`5adae44` and the snapshot's rev too.

### What sheaf has to change

The only break is `ScrollWheelEvent` → `ScrollEvent` (Zed #65805). The old
names stay as deprecated aliases, and `-D warnings` makes those errors:

- `crates/sheaf-app/src/docview.rs`: the import (line 25), `on_wheel`'s
  parameter type (3494), and `.on_scroll_wheel(` → `.on_scroll(` (4652).
- `crates/sheaf-app/src/script.rs`: the import (45),
  `PlatformInput::ScrollWheel` → `PlatformInput::Scroll` (228), and the
  new required field `source: ScrollSource::Pointer`.

That's about 10 lines after `cargo fmt`. sheaf-ui needs nothing.

**Trial build** (a throwaway clone at `ac4d401`, GPUI as a git
dependency on Zed `5adae44`, rust 1.98.0):

- Before the fix: exactly those 7 errors.
- After it: `cargo clippy --workspace --all-targets -- -D warnings` is
  clean, and `cargo test -p sheaf-app -p sheaf-ui` passes (118 + 11).

The git dependency builds Zed's own wgpu rev, not gpui-pre's crates.io
one, so rerun all of this on the real 0.3.10.

### wgpu 29.0.4 → 30.0.1, without Zed's fix

- Zed #65649 moved Zed to wgpu git rev `3b3506e` for gfx-rs/wgpu#10230.
  That fix is for a Vulkan panic on surface teardown: "Trying to destroy
  a SwapchainAcquireSemaphore that is still in use by a SurfaceTexture".
  Zed's PR says the panic is in registry 29.0.4 (what sheaf ships today)
  and still in 30.0.1.
- gpui-pre publishes against crates.io **30.0.1**. `bump-gpui.ts`
  (`adaptWgpuRelease`) rewrites `wgpu_context.rs`, `wgpu_atlas.rs` and
  `wgpu_renderer.rs` in gpui-pre-wgpu. It drops `default_queue:
  wgpu::QueueDescriptor::default()` and turns `.has_srgb_suffix()` into
  `.is_srgb()`. These are new `Modified for gpui-pre` edits.
- **The README's parity patch doesn't work here.** wgpu at `3b3506e` is
  versioned 30.0.0, so a `[patch.crates-io] wgpu = { git, rev }` would
  apply, and gpui-pre-wgpu's rewritten sources wouldn't compile against
  it (no `default_queue`). The only way round would be patching
  gpui-pre-wgpu, which sheaf doesn't do.
- **Decision: accept the gap and document it.** It's no regression from
  0.3.9. gpui-pre asks for `^30.0.1`, so once a 30.x release ships the
  fix, `cargo update -p wgpu` picks it up with no gpui-pre bump. Check
  the wgpu releases on each later bump.

### Unchanged

- Zed's `[patch.crates-io]` block, and the xim-rs rev in
  `crates/gpui_linux/Cargo.toml`. sheaf's `[patch.crates-io]` stays as
  it is.
- Zed's `rust-toolchain.toml` (1.98.1). sheaf's 1.98.0 builds it.
- The 0.3.9 packaging edits carry over: `$crate` paths in `actions!`,
  the gpui-kit path facade in the proc macros (now 19 entry points), and
  gpui-pre-apple's vendored sources (5 files).
- Other packaging differences in the 2026-10-19 log don't affect sheaf:
  - http_client drops `async-tar` and the `github-download` feature;
  - exact `=` pins are relaxed for hot reload's optional dioxus and
    subsecond crates (off);
  - `gpui-pre-reqwest` replaces zed-reqwest.

### Expected `Cargo.lock` changes

Besides the `gpui-pre-*` versions:

- the wgpu family moves 29.0.4 → 30.0.1 (`wgpu`, `wgpu-core`, the
  `wgpu-core-deps-*` crates, `wgpu-hal`, `wgpu-types`,
  `wgpu-naga-bridge`, `naga`), and `naga-types` appears;
- `bit-set` 0.10 comes in, and `glow` likely moves to 0.18;
- `gpu-descriptor` and `hexf-parse` drop out;
- possibly `drm` 0.15 through wgpu-hal, Linux only;
- `gpui-pre-ios`, `objc2-ui-kit` and `objc2-core-text` appear (iOS
  only, never built).

That's roughly 25 entries and about 300 lines. The trial lockfile came
from git sources, so it shows git-versus-crates.io artifacts (`dirs` 6,
`xim-parser` 0.2.1, the zed-* crates) that the real bump won't have.
Anything outside this list needs explaining (README step 5).

### Behaviour changes to watch in the smoke test

- Sprites are grouped by atlas texture, and their instance layout
  changed (Zed `b4ecc45`). PDF pages draw as image sprites, so check
  pages, thumbnails and glyphs.
- Translucent layers composite source-over (Zed #65571). The wgpu paths
  pipeline now uses `PREMULTIPLIED_ALPHA_BLENDING`. This matters only
  for alpha over transparent windows; sheaf's windows are opaque.
- Focus-handle release was reworked (Zed #65177): a dropped-handle
  list, and a new assert on over-release. sheaf has about 114
  `FocusHandle` sites.
- App borrow panics now name the borrow that conflicted (Zed #65863).
  Only the messages change.
- Windows ignores other apps' `WM_USER` messages (Zed #65628). Windows
  CI covers it.
- macOS can opt into Display P3 through `WindowOptions::use_display_p3`.
  It defaults to false, and sheaf builds `WindowOptions` with
  `..Default::default()`.
- New and unused by sheaf:
  - the iOS backend and hot reload (`hot-reload` feature, off);
  - `App::on_app_lifecycle` and `App::on_memory_warning`;
  - `on_back_gesture`;
  - `ScrollEvent::source` (`ScrollSource::Pointer` or `Touch`);
  - `Window::insets` and `notify_text_input_changed`/`_selection_changed`;
  - `EntityInputHandler::selection_bounds_for_range`, which has a default.

## Steps

1. When 0.3.10 is out, `cargo info gpui-pre@0.3.10`: read `zed-rev` and
   check that it's past `5adae44`. Diff the GPUI crates from `5adae44` to
   that rev for anything new.
2. Work on a branch in a worktree with its own `target/`. Bump both
   exact pins (`gpui-pre`, `gpui-pre-platform`) and the rev in the
   `Cargo.toml` comment.
3. `cargo fetch`, then `rg -l 'Modified for gpui-pre'
   ~/.cargo/registry/src/*/gpui-pre*-0.3.10/`. Read the new gpui-pre-wgpu
   edits and anything else new or changed. Update the list in the
   `Cargo.toml` comment.
4. `[patch.crates-io]`: confirm it's unchanged and that cargo prints no
   "patch not used" warnings.
5. Review the `Cargo.lock` diff against the list above.
6. Apply the scroll rename in `docview.rs` and `script.rs`.
7. Local checks (CI covers fmt, clippy, tests and the other OSes):
   - `cargo test -p sheaf-app -p sheaf-ui` under the capped
     `systemd-run` runner;
   - the README smoke test on Wayland and on X11, with
     `--timing --selftest-save` and `--selftest-annot`;
   - open and close dialogs and resize windows, which exercises the
     Vulkan surface-teardown path;
   - put the timings in the commit message and compare them with the
     0.3.9 switch.
8. Docs:
   - the `Cargo.toml` comment: 0.3.10's Zed rev, the edits list, and the
     wgpu 30.0.1 gap, with why no patch;
   - README "Updating GPUI" step 5: wgpu as the case a parity patch
     can't cover, and the "at 0.3.9" examples;
   - `docs/prior-art/rust-ecosystem.md`, the gpui and gpui-pre sections:
     the rev, the edits, the parity notes;
   - `todos/agent.md:460` if it should name the new pin.
9. PR, then watch macOS and Windows CI.

## Estimate

One small PR, one slice:

| Part | Size |
|---|---|
| Code | 2 files, about 10 lines |
| `Cargo.toml` | 2 pins and about 10 lines of comment |
| `Cargo.lock` | about 25 entries, about 300 lines |
| Docs | about 30–50 lines |

Most of the time goes to the smoke test and CI. The 2026-10-26 snapshot
will include a few days more of Zed main than this check covered. At
current activity (about 2 GPUI commits a day) it shouldn't be much
bigger.

## Trial leftovers

`~/.cache/sheaf-gpui-eval`, about 15 GB in all, holds:

- `zed`: a blobless Zed clone back to 2026-10-03;
- `sheaf-trial`: the patched clone, with `vendor/pdfium` and
  `vendor/ocr` symlinked from the real checkout;
- `target/`: its own target dir, about 15 GB.

Delete it after the bump, or reuse it to repeat the trial on 0.3.10.
