# Build or adopt 76: The ACP client (the agent panel)

Area: M16: Agent tools (MCP), and the agent panel (Pending) · Part of [build or adopt](README.md)

- **Need:** launch Claude Code or Codex adapters over ACP: sessions,
  streamed updates, permission requests.
- **Where:** Pending "Agent panel" (`ROADMAP.md:784-815`);
  `agent.md:93-97`.
- **Today's plan:** "crate `agent-client-protocol`, Apache-2.0, 3.3.0 on
  2026-10-14" (`agent.md:94-95`).
- **Candidates:** [agent-client-protocol](https://crates.io/crates/agent-client-protocol)
  3.5.0 (2026-10-22; Apache-2.0; 5.5M downloads; six minor releases since
  3.0.0 on 2026-10-06, majors 1 to 3 since July);
  [agent-client-protocol-rmcp](https://crates.io/crates/agent-client-protocol-rmcp)
  4.0.5 (2026-10-22). The SDK workspace also has `-conductor`, `-http`,
  `-polyfill` and `-trace-viewer` crates (unreviewed).
- **Fit:** the protocol's own SDK; the churn is the cost.
- **Recommendation:** **adopt**, pinned exactly; take the version at
  kickoff. High.
