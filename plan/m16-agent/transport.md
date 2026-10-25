# M16: Transport (researched 2026-10-20)

Part of [the agent design](README.md).


For M16. Five researchers (the MCP spec, clients, the Rust SDK, sheaf's
socket code, prior art), a synthesis, and an adversarial fact-check that
re-verified the claims against current sources. Facts are as of
2026-10-20; MCP, its SDKs and its clients change monthly, so recheck at
kickoff.

There are two links: the agent's client to `sheaf mcp`, and `sheaf mcp`
to the running sheaf, which holds the open documents, their undo history
and the approval card.

### Facts that shape it

- **MCP's current revision is 2026-07-28.** It defines exactly two
  transports: stdio, and Streamable HTTP (one POST endpoint; the reply is
  JSON or an SSE stream for that request). Custom transports over a byte
  stream such as a Unix socket SHOULD reuse stdio's framing, which "works
  unchanged over Unix domain sockets". WebSocket (SEP-1288) was closed as
  dormant on 2026-06-26.
- **It's stateless.** There's no initialize handshake and no protocol
  session: every request carries its protocol version and capabilities in
  `_meta`, and a server "must not treat connection or process identity
  as a proxy for conversation or session continuity". State that spans
  requests is an explicit handle the client passes back. Over stdio the
  server sends no requests of its own, only responses and notifications.
- **Claude Desktop launches local servers only over stdio.** Its URL
  "custom connectors" are opened from Anthropic's cloud, so they can't
  reach localhost. Local servers in `claude_desktop_config.json` aren't
  available in Cowork or claude.ai.
- **Clients:** Claude Code, the Codex CLI and app, Cursor, VS Code and
  Zed all take a stdio command (`claude mcp add sheaf -- sheaf mcp`,
  `codex mcp add sheaf -- sheaf mcp`). Zed is on 2025-11-25, and Codex
  has 2026-07-28 only behind an opt-in flag, so sheaf serves both eras.
- **Client limits:** Codex kills a tool call after 60 s by default and
  fails a server that hasn't started in 10 s. Claude Code's idle limit is
  30 min on stdio and 5 min on HTTP. On Windows, Claude Code kills a stdio
  server's whole process tree 300 ms after closing stdin; on Unix it
  sends SIGINT, SIGTERM after 100 ms, then SIGKILL.
- **Stripped environments:** Codex and the MCP TypeScript SDK pass stdio
  servers a minimal environment, without `XDG_RUNTIME_DIR` (both) or
  `TMPDIR` (the TS SDK). sheaf-instance read those, so `sheaf mcp` would
  have looked in `/tmp/sheaf-<uid>` while the GUI was in
  `/run/user/<uid>/sheaf`. instance-handoff, which replaced it, doesn't:
  its app folder is `/run/user/<uid>/sheaf` on Linux with systemd, and
  the per-user temporary folder from `confstr` on macOS, whatever the
  environment. What can still differ is the state folder the instance
  folder is named after: `sheaf_ops::dirs::state_dir()` follows
  `XDG_STATE_HOME` (and `SHEAF_CONFIG_DIR`), which a stripped
  environment drops, so a relay that computes another state folder finds
  no sheaf.
- **Local HTTP has a record.** The spec requires Origin validation (403
  when invalid), and says local servers SHOULD bind to 127.0.0.1 and
  SHOULD authenticate. SDK defaults got this wrong: CVE-2025-66414 and
  CVE-2025-66416 (the TypeScript and Python SDKs), CVE-2025-49596 (MCP
  Inspector, CVSS 9.4) and CVE-2025-52882 (Claude Code's IDE extension,
  CVSS 8.8). rmcp 3.6.0's `allowed_origins` is empty by default, which
  turns Origin validation off. A loopback port is reachable by every user
  on an RDS or Citrix host.
- **rmcp** shipped breaking majors on 2026-03-03, 2026-06-29 and
  2026-07-28 (3.6.0 on 2026-10-20), so pin it exactly. Tokio has no
  AF_UNIX on Windows (tokio#2201 is still open).
- **Elicitation** (the server asking the user through the client) is no
  approval gate. Claude Code's Elicitation hook and Codex's auto-approve
  can answer it without a person, and Claude Desktop and Zed don't
  support it.
- **MCP over ACP:** the RFD is a draft, `claude-agent-acp` and
  `codex-acp` don't advertise it, and ACP went from 3.0.0 to 3.4.0 in ten
  days. Every ACP agent MUST support stdio MCP servers, so a panel can
  pass `sheaf mcp` to its agent.

### Options

| # | Option | Verdict |
|---|---|---|
| 1 | **stdio relay plus a socket of its own:** `sheaf mcp` relays to `agent.sock` in the runtime folder; rmcp runs in the app | **Recommended** |
| 2 | The same over `instance.sock`, extended | Gone: that socket is instance-handoff's |
| 3 | The app hosts Streamable HTTP on 127.0.0.1 | Defer |
| 4 | Streamable HTTP over the socket | VS Code only; skip |
| 5 | A headless `sheaf mcp` that opens files itself | Not for M16 |
| 6 | MCP over ACP | The panel's, later |
| 7 | The relay speaks MCP; a private RPC to the app | Two protocols; no |

1. **Recommended.** Nothing listens on the network and browsers can't
   open Unix sockets. `agent.sock` goes in the runtime folder
   instance-handoff makes and checks: `Builder::runtime_dir()` names it
   (private, the user's alone, no links on the way), and
   `Builder::probe()` says whether a sheaf serves there before the relay
   connects. The crate exports no listener and no peer check, so the
   same-user check on every connection to `agent.sock` is M16's work:
   on Unix a few lines (`SO_PEERCRED`, `getpeereid`, as the crate checks
   its own socket); on Windows new work (the Windows amendment below).
   One MCP implementation lives in the process that owns the documents,
   undo and the approval card, and the same handler can later sit behind
   option 3 or 6. The hand-off protocol is left alone. Unity ships this
   shape: a stdio relay to the Editor over a Unix socket or named pipe,
   with in-app approval of each client.
2. **The instance socket, extended.** No longer open: since sheaf moved
   to instance-handoff, the hand-off socket is the crate's. Its scope
   leaves general IPC out (its ADR 0001) and puts replies beyond "taken"
   off to 0.2 (ADR 0005), and each connection carries one payload, with
   a commit point, not a stream.
3. **Localhost HTTP.** No relay for clients that speak HTTP, and several
   can connect at once (JetBrains, Figma and Sketch host localhost
   servers; JetBrains' `McpStdioRunner` bridges stdio to it). But it puts
   a network listener in front of client data, other users on a terminal
   server can reach it, sheaf has to get Origin, Host and token checks
   right where SDKs failed, clients time out sooner, Claude Desktop still
   needs a stdio bridge, and users have to configure a URL plus a token.
   If it's ever built: off by default in `settings.toml`, a per-launch
   token in a 0600 file, and Origin and Host allowlists.
4. **HTTP over the socket.** VS Code connects to `unix://` and `pipe://`
   directly; no other client surveyed does.
5. **Headless.** The simplest transport, and it works without the GUI.
   But two processes would edit one PDF (the GUI's next save asks
   "changed on disk", and Overwrite drops the agent's work), it has its
   own undo, and no approval card the user can trust. It could come later
   as a batch mode that writes new files.
6. **MCP over ACP.** No listener at all, and the agent's own tools can be
   approved in sheaf too, but nothing ships it yet. Write option 1's
   handler without assumptions about its transport, and the panel can
   add this without changing M16.
7. **A private RPC.** Keeps rmcp and tokio out of the GUI and lets the
   relay answer discovery before sheaf starts, but every MCP feature is
   translated twice and the panel can't reuse the handler.

### Amendments to option 1 (from the fact-check)

- **A connection isn't a session.** Claude Desktop starts each local
  server once, when it launches, so one relay serves every chat. sheaf
  issues agent-session handles, binds them to the connection, and takes
  them as tool arguments. "Revert this turn" and "Always for this
  command in this thread" need a turn and a thread that sheaf defines,
  for example per session handle or per committed plan.
- **Find the runtime folder without the environment:**
  instance-handoff's `Builder::runtime_dir()` does (`/run/user/<uid>`
  with systemd, `confstr(_CS_DARWIN_USER_TEMP_DIR)` on macOS; Windows is
  fine, both lists keep `LOCALAPPDATA`), as long as the relay names the
  GUI's state folder (Stripped environments, above).
- **The relay survives a missing or restarting sheaf.** A pure byte
  relay can't answer discovery when sheaf isn't running, and Claude Code
  doesn't reconnect stdio servers. So the relay is slightly MCP-aware: it
  answers discovery and a legacy client's initialize itself, reconnects
  lazily, and turns in-flight requests into errors when sheaf goes away.
- **Starting sheaf from the relay is hard:** the stripped environment
  has no `DISPLAY`, `WAYLAND_DISPLAY` or `DBUS_SESSION_BUS_ADDRESS`, so
  it has to launch through the session (`systemd-run --user`, D-Bus or
  desktop-file activation; `open -a` on macOS), detached with `setsid`
  or `CREATE_BREAKAWAY_FROM_JOB`, within Codex's 10 s.
- **Approvals don't block.** `plan` returns the plan's handle and
  preview at once and puts the card up; `await_plan` returns "pending"
  after a bounded wait, well under 60 s, with progress, and the agent
  polls. MCP's Tasks extension names approval gates as a use case and
  rmcp implements it, but client support is uneven. Don't tag commit with
  `anthropic/requiresUserInteraction`: Claude Code's `dontAsk` mode and
  `-p` with a permission-prompt tool turn it into a deny.
- **Connection consent.** Any process running as the user can reach the
  socket, so the window asks before an agent's first connection, as
  Unity's Pending Connections and JetBrains' first-call consent do. That
  is on top of the per-document §7216 consent and per-plan approval.
- **The approval card needs a queue per window.** Today a second plugin
  form cancels the first (`Workspace::plugin_ui`).
- **Cancellation and shutdown** (`notifications/cancelled`, stdin EOF,
  a closed socket) withdraw any open card.
- **Windows:** sheaf-instance's socket had no peer check, only the
  profile folder's ACL. instance-handoff's pipe has a user-only access
  list, an integrity label and a token check on both ends, but that's
  the crate's own, not exported, so `agent.sock` needs new work before
  document content flows. Either bridge `uds_windows` into tokio with a
  reader and a writer thread per connection, or use a per-user named
  pipe. A pipe name is machine-wide, so it needs the user's SID in the
  name, `FILE_FLAG_FIRST_PIPE_INSTANCE`, and a check of the server's SID
  by the relay before it sends anything (against another user squatting
  the name). The crate's `src/sys/windows.rs` does all of this for its
  pipe (a random name published in a labelled folder instead of the
  SID), and is the reference.
- **Other environments:** WSL2 (a Linux `sheaf` can't reach a Windows
  AF_UNIX socket, so docs point at `sheaf.exe mcp`), VS Code's sandbox
  for stdio servers, and Flatpak or Snap packaging all change what the
  relay can see.

### Every open document, and opening more (Greg, 2026-10-22)

Greg's direction for M16, not settled design. Every sheaf window shares
one process with the MCP server (option 1: `sheaf mcp` is a stdio relay
to sheaf's own `agent.sock`, not the instance-handoff socket), so the
agent can reach **all open documents**, and **open more**, which come
back to it as handles in its MCP session. One process holds the
documents, their undo and the approval cards, so two processes never
edit one file (what sank option 5).

Recommendations that go with it, not yet decided:
- **Handles.** Tools take a document handle and a version token, and
  `list_documents` lists the handles.
- **Consent per document, the listing included.** §7216 consent stays
  per document, even for the listing, since file names reveal clients:
  `list_documents` shows only the documents shared with the session,
  plus a count of the rest. Grants belong to the agent session (the
  handle sheaf issues), not the connection.
- **`open_document`** goes down the path handed-over files take
  (`workspace::open_from_launch`), its path resolved against the
  relay's working directory, as a launch's is against its own. It opens
  a visible window without taking focus, and nothing auto-saves or
  auto-closes it.
- **Opening a path needs its own approval card,** never "always":
  otherwise a sandboxed agent (Codex, Claude Code) reads files outside
  its sandbox through sheaf. Under Flatpak the path has to come through
  the document portal.

**Threads: an avenue to explore, not a decision** (Greg's words).
Rather than one thread per document or per window, maybe a pool like
the engine pool (`sheaf-render/src/pool.rs`):
- a thread is a conversation plus its granted documents, and any window
  can show it;
- live agent processes are pooled per agent kind (one process, many ACP
  sessions), pinned while a panel shows one of theirs and while a turn
  or a plan is open, and closed least recently used past a budget;
- a closed thread reopens from its transcript (ACP `session/load` where
  the agent offers it);
- MCP clients own their own threads: for them sheaf keeps only the
  grants, keyed by the agent-session handle.

### Decisions at kickoff

1. A socket of its own: settled by the move to instance-handoff, whose
   socket can't carry MCP (option 2). Still open: the folder
   `runtime_dir()` names, or one beside it, and how the relay finds it
   in a stripped environment.
2. Windows: bridge threads, or a per-user named pipe? And which peer
   check lands before client data flows?
3. With no sheaf running, does `sheaf mcp` start one, or fail with a
   clear message? Recommended: fail first; starting it is the hard part
   above.
4. Ever offer localhost HTTP? Recommended: only when a client needs a
   URL, and then off by default.
5. Serve both protocol eras (2026-07-28 and 2025-11-25)? Recommended:
   yes, while Zed and Codex need the older one.
6. Client setup: print a snippet per client, or also ship a signed
   `.mcpb` for Claude Desktop? Writing other apps' config files cuts
   against sheaf never writing settings.
7. How the card gets attention when the window can't bring itself
   forward (an MCP request carries no activation token): a taskbar flash
   and a notification, or more.

### Sources (checked 2026-10-20)

- MCP 2026-07-28: [transports](https://modelcontextprotocol.io/specification/2026-07-28/basic/transports),
  [stdio](https://modelcontextprotocol.io/specification/2026-07-28/basic/transports/stdio),
  [Streamable HTTP](https://modelcontextprotocol.io/specification/2026-07-28/basic/transports/streamable-http),
  [statelessness](https://modelcontextprotocol.io/specification/2026-07-28/basic/index#statelessness),
  [elicitation](https://modelcontextprotocol.io/specification/2026-07-28/client/elicitation),
  [security best practices](https://modelcontextprotocol.io/docs/2026-07-28/tutorials/security/security_best_practices),
  [the release post](https://blog.modelcontextprotocol.io/posts/2026-07-28/),
  [Tasks](https://modelcontextprotocol.io/extensions/tasks/overview).
- [GHSA-w48q-cv73-mx4w](https://github.com/modelcontextprotocol/typescript-sdk/security/advisories/GHSA-w48q-cv73-mx4w)
  (CVE-2025-66414), [CVE-2025-66416](https://osv.dev/CVE-2025-66416).
- Claude Code: [MCP](https://code.claude.com/docs/en/mcp). Claude
  Desktop: [local servers](https://modelcontextprotocol.io/docs/develop/connect-local-servers),
  [custom connectors](https://support.claude.com/en/articles/11175166-about-custom-connectors-using-remote-mcp).
- Codex: [config reference](https://learn.chatgpt.com/docs/config-file/config-reference),
  and in `openai/codex`: `codex-rs/rmcp-client/src/utils.rs` (default
  environment), `codex-rs/features/src/lib.rs` (the 2026-07-28 flag),
  `codex-rs/codex-mcp/src/elicitation.rs`.
- [Unity MCP](https://docs.unity3d.com/Packages/com.unity.ai.assistant@2.0/manual/unity-mcp-overview.html);
  JetBrains' [`McpStdioRunner.kt`](https://github.com/JetBrains/intellij-community/blob/master/plugins/mcp-server/mcpserver.stdio/src/McpStdioRunner.kt).
- sheaf: `crates/sheaf-instance/src/lib.rs` (`PROTOCOL`, the timeouts,
  `default_dir`, `serve`, `hand_off`), and the M10.5 plan's proposal 36
  and §20. sheaf-instance has since been replaced by instance-handoff
  0.1.0-alpha.1 (`Builder::runtime_dir`, `Builder::probe`; its
  `docs/design` ADRs 0001 and 0005).
