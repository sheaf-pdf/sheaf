# Build or adopt 75: The MCP server (`sheaf mcp`)

Area: M16: Agent tools (MCP), and the agent panel (Pending) · Part of [build or adopt](README.md)

- **Need:** a stdio MCP server, relayed to the running app, serving both
  protocol eras.
- **Where:** `ROADMAP.md:491-494`; `plan/m16-agent/README.md`, `:123-305`.
- **Today's plan:** "a stdio MCP server (`rmcp`)" (`ROADMAP.md:491`); rmcp
  "shipped breaking majors … so pin it exactly" (`agent.md:175-176`).
- **Candidates:** [rmcp](https://crates.io/crates/rmcp) 3.7.0
  (2026-10-22, two days after the 3.6.0 the memo checked; Apache-2.0;
  39M downloads; the official SDK,
  [modelcontextprotocol/rust-sdk](https://github.com/modelcontextprotocol/rust-sdk)).
  3.7.0's notes list auth additions and two fixes, plus a "Merge commit
  from fork" (possibly a security fix; unverified). Its
  `ProtocolVersion` has `V_2026_07_28` as `LATEST` and `V_2025_11_25`
  (`crates/rmcp/src/model.rs:167-174`); MCP's current revision is still
  [2026-07-28](https://modelcontextprotocol.io/specification/versioning).
  Needs tokio; its schemars support is schemars 1.0.
- **Fit:** tokio is in sheaf's default build already: wasmtime-wasi 49
  and wasmtime-wasi-http 49 depend on it (tokio 1.53.2 in the lock),
  through sheaf-plugin, which the default `plugins` feature brings. So
  option 7's "Keeps rmcp and tokio out of the GUI" (`agent.md:234`)
  saves less than it suggests. schemars 1.x matches `sheaf-edit`'s.
- **Recommendation:** **adopt rmcp**, pinned exactly, as planned; recheck
  its version and the `allowed_origins` default at kickoff. High.
