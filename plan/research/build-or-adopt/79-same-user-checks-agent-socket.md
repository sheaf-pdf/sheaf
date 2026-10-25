# Build or adopt 79: Same-user checks on the agent socket (Unix)

Area: M16: Agent tools (MCP), and the agent panel (Pending) · Part of [build or adopt](README.md)

- **Need:** refuse any peer on `agent.sock` that isn't the same user, on
  Linux and macOS.
- **Where:** M16: "a 0700 runtime folder the user must own, and a peer
  uid check on every connection" (`agent.md:198-202`).
- **Today:** sheaf-instance calls libc's `SO_PEERCRED` on Linux and
  `getpeereid` on macOS, with `unsafe`
  (`crates/sheaf-instance/src/lib.rs:398-439`). PR #32 replaces
  sheaf-instance with instance-handoff.
- **Candidates:** [tokio](https://crates.io/crates/tokio) 1.54.1 (MIT):
  `UnixStream::peer_cred()` returns a `UCred` with uid, gid and pid
  (`tokio-1.54.0/src/net/unix/stream.rs:981`); safe, and rmcp runs on
  tokio. std's `UnixStream::peer_cred` is **still nightly-only**
  (`peer_credentials_unix_socket`,
  [#42839](https://github.com/rust-lang/rust/issues/42839);
  [docs](https://doc.rust-lang.org/std/os/unix/net/struct.UnixStream.html)).
  interprocess 2.4.6's `PeerCreds` (euid on Unix).
- **Recommendation:** **adopt tokio's `peer_cred`** for the agent socket.
  High.
