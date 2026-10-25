# Build or adopt 78: The local transport (relay socket, Windows pipe)

Area: M16: Agent tools (MCP), and the agent panel (Pending) · Part of [build or adopt](README.md)

- **Need:** `sheaf mcp` relays to `agent.sock`. On Windows the plan
  chooses between bridge threads and a per-user named pipe with a peer
  check.
- **Where:** `agent.md:189-305` (decisions 1–3): "Windows: bridge
  threads, or a per-user named pipe?" (`:292-293`). D16.
- **Candidates:** tokio's `net::windows::named_pipe`, in the build
  already; [interprocess](https://crates.io/crates/interprocess) 2.4.6
  (2026-10-16; 0BSD OR Apache-2.0; 16.5M downloads; local sockets on
  every OS, with tokio support); Greg's
  [instance-handoff](https://crates.io/crates/instance-handoff)
  0.1.0-alpha.1, whose socket code is the precedent (entry 96).
- **Fit:** tokio's named pipes cover the pipe option with no new
  dependency; the SID checks are sheaf's own either way (entry 80).
  interprocess would unify the Unix and Windows paths, but adds a
  dependency for what instance-handoff already solved once.
- **Recommendation:** **build on tokio** (named pipes on Windows, Unix
  sockets elsewhere), sharing the folder and peer checks with
  instance-handoff. Medium. Entries 79–80 have the checks' detail, from
  the security section.
