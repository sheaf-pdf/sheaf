# Build or adopt 96: Single instance and progress (Greg's crates)

Area: Infrastructure that's built (confirmed) · Part of [build or adopt](README.md)

- **in-progress** 0.1.0 (2026-10-17; MIT OR Apache-2.0) is used:
  `in-progress.workspace = true` in sheaf-doc, sheaf-ops, sheaf-cli and
  sheaf-plugin (`Cargo.toml:73`).
- **instance-handoff** 0.1.0-alpha.1 (2026-10-21; MIT OR Apache-2.0) is
  published but not used on origin/main, which still has the in-tree
  `crates/sheaf-instance` (libc, uds_windows, windows). PR #32 makes the
  switch: it deletes `crates/sheaf-instance`, and updates `plan/m16-agent/README.md`,
  which cites `sheaf_instance::default_dir()` at `:164` and
  `crates/sheaf-instance/src/lib.rs` at `:328`.
