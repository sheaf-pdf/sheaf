# M16: Agent tools (MCP)

Status: sketched · 

Proves: Agents work on documents through sheaf's own ops · Depends on: Nothing unbuilt

## Summary

Added 2026-10-20. An MCP server through which an AI agent works on a
document the way a plugin does. It reads and inspects the document, and
it builds transactions from any combination of sheaf's ops, which the
user approves before they commit. The design so far is in
[`agent.md`](agent.md) (its slices 1 and 2) and in the M10.5
plan's §20 ("The agent milestone"). The chat panel that hosts an agent
inside sheaf stays under [Pending](../pending.md#agent-panel-claude-and-codex).

- **`sheaf mcp`:** a stdio MCP server (`rmcp`) that Claude Code, Claude
  Desktop, Codex or any other MCP client runs. It needs no panel in
  sheaf, and sheaf never handles the agent's sign-in.
- **Reads, as a plugin's:**
  - the document's pages, outline and marks, and the selection;
  - text, OCR layers included;
  - page images, with caps on size and count;
  - the edit history;
  - every command with its help text, plugin commands included.

  Every read returns the version it saw.
- **Transactions of any ops:**
  - The agent submits sheaf-edit `Op` JSON, the vocabulary of plugin
    API 0.4.
  - `Document::plan` lowers a batch into one transaction with a
    preview. The user approves the preview in the sheaf window, and
    `commit(plan)` commits exactly what was shown, as one version
    authored by the agent on the user's behalf.
  - A plan expires if the document changed underneath it. Reads are
    tracked per agent session, so a user edit elsewhere doesn't refuse a
    plan. MCP itself has no sessions since its 2026-07-28 revision, so
    the session is a handle sheaf issues and the agent passes back.
- **Commands:** a command with an argument schema runs without its
  dialog; one without opens its dialog pre-filled. M15's batch actions
  need the same schemas.
- **Revert this turn,** and a log of what was sent.
- **Consent and policy:**
  - consent before a document first goes to a cloud agent (IRC §7216);
  - field values behind per-document consent;
  - `[agent] enabled` in `settings.toml`, so a firm can switch agents
    off.

**Decide at kickoff:**
- **Which ops an approved plan may hold** beyond a plugin's. Crop,
  watermark and flatten need a manifest permission for plugins, and
  inserting and replacing pages is the user's alone. Also whether
  "semantic edits only, at first" (M10.5 decision 6) still holds.
- **How `sheaf mcp` reaches the running sheaf.** `agent.md` recommends
  a stdio relay to sheaf's own `agent.sock` (its option 1), not the
  instance-handoff socket, which carries only launches (the crate's ADRs
  0001 and 0005: no general IPC, no replies before 0.2). M10.5 left the
  transport to this milestone (proposal 36).
- **Commands an agent should never run,** and writes that never get
  "always allow": saves, exports and anything else that writes files.

Builds on M10.5 (ops, version tokens, plugin API 0.4's reads), M6's
command registry and M9's edit history, so it can start once M10.5 is
done.

Slices: read tools, history and commands → plans and approval → command
schemas and the dialog fallback → consent, settings and the send log →
docs.

## Plan files

- [The agent design](agent.md): `sheaf mcp` (slices 1 and 2) and the agent panel (Pending)
- [Transport](transport.md): researched 2026-10-20
- No plan yet: the milestone is broken into slices when it starts.
- [Build or adopt](../research/build-or-adopt/README.md): the M16 rows, and the [changes to make](../research/build-or-adopt/changes-to-make.md) at kickoff
- Open questions: [asks](../asks.md)
