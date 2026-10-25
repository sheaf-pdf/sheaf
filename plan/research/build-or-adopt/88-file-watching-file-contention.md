# Build or adopt 88: File watching and file contention

Area: Pending · Part of [build or adopt](README.md)

- **Need:** notice that a file changed on disk while open (the banner
  when the window comes to the front; the lock file's "Notify Me"); M15's
  watched folders (entry 74).
- **Where:** Pending "File contention" (`ROADMAP.md:768-782`);
  `plan/topics/file-contention.md`, `:129-131`, `:133-150`.
- **Today's plan:** layer 1 ("detect changes before saving") is done in
  M10.5, with a `FileStamp`. "When a window comes to the front, check the
  file" (`file-contention.md:111`). On a network share, the lock holder
  is found through OS locks, or by a heartbeat where locks don't reach
  the server (`:133-150`). `settings.toml` is reloaded by an mtime stamp
  (`crates/sheaf-app/src/prefs.rs:73-75`).
- **Candidates:** [notify](https://crates.io/crates/notify) 8.2.0
  (2025-08-03, the latest stable; 9.0.0-rc.6 on 2026-10-17). The `notify`
  crate is CC0-1.0; notify-types and the debouncers are MIT OR
  Apache-2.0. 179M downloads; inotify, FSEvents and
  ReadDirectoryChangesW, with a polling backend. Its docs: "Network
  mounted filesystems like NFS may not emit any events … A workaround is
  the `PollWatcher` backend" (`notify/src/lib.rs:37-42`). For lock files,
  [fs4](https://crates.io/crates/fs4) 1.1.0 (MIT OR Apache-2.0) or
  [fd-lock](https://crates.io/crates/fd-lock) 4.0.4; std's `File::lock`,
  stable in recent Rust (unverified for sheaf's 1.98), may make both
  unnecessary.
- **Fit:** the case the design cares about is a colleague on a file
  server, and events from another SMB or NFS client are exactly what
  notify can't promise, so polling the `FileStamp` (on focus, and every
  few seconds while focused) is the reliable base. notify adds instant
  updates for local and sync-client changes. CC0 is a public-domain
  dedication, compatible with sheaf's licenses; some legal reviews flag
  that it waives no patents, a small cost.
- **Recommendation:** **build** the check on `FileStamp` polling;
  **adopt notify** behind a small trait, with `PollWatcher` as the
  fallback, when Notify Me or watched folders get a slice. Check whether
  std's file locking covers the lock file before adding fs4. Medium.
