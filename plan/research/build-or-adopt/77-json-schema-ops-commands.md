# Build or adopt 77: JSON Schema for ops and commands

Area: M16: Agent tools (MCP), and the agent panel (Pending) · Part of [build or adopt](README.md)

- **Need:** argument schemas shared by MCP tools, M15 steps, plugins and
  key bindings.
- **Where:** `ROADMAP.md:516-518`; F5 (`backlog.md:512-572`);
  `agent.md:375`.
- **Today's plan:** "schema comes from schemars, as `plugin::Run`'s
  already does" (`agent.md:375`).
- **Candidates:** [schemars](https://crates.io/crates/schemars) 1.2.3
  (2026-10-12; MIT; 574M downloads); sheaf locks 1.2.2, in `sheaf-edit`,
  `sheaf-app` and `sheaf-cli`.
- **Recommendation:** **adopt** (already adopted). High.
