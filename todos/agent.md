# Agent panel: Claude and Codex in a sheaf window

Status: **proposed** (2026-10-14). Nothing is built. This memo records
the decisions so far, the research behind them, and a first cut of the
slices.
- **`sheaf mcp` and its tools are M16** (Greg, 2026-10-20; slices 1 and
  2 below). See [`ROADMAP.md`](../ROADMAP.md#m16-agent-tools-mcp).
- **The panel stays under Pending** (slices 3–6). See
  [`ROADMAP.md`](../ROADMAP.md#agent-panel-claude-and-codex).
- **M10.5 changes the tools' design:** edits become sheaf-edit `Op`
  JSON through `Document::plan` and `commit(plan)`, with version
  tokens. Its slice 20 updates this memo; until then, the M10.5 plan's
  §20 and "Changes to other plans" hold.

## The idea

A chat panel in the window where the user works with an AI agent on the
open document, the way Zed runs external agents in its agent threads. The
agent can:
- see the document: pages, text (OCR layers included), bookmarks, marks,
  the selection, and page images;
- see the edit history tree;
- list every command, built-in or from a plugin, with its help text;
- run commands and edits live, with the user's approval;
- analyze the document's state and history.

Foxit's AI Assistant does some of this (agentic editing through chat,
"Delete Page 1"), but only inside Foxit's subscription. See
[prior-art/foxit.md §3.13](../docs/prior-art/foxit.md#313-ai-assistant-ch-15-subscription-only).

## Decisions (Greg, 2026-10-14)

- **Agents only.** sheaf launches an agent program, and the agent handles
  sign-in and billing with the user's own subscription. sheaf takes no
  API keys and has no agent loop of its own.
- **Claude and Codex first.** Other ACP agents (Gemini CLI and the rest of
  the ACP Registry) can come later.
- **Commands run live.** Each approved tool call runs at once. The agent
  doesn't hand back a plan for the user to run.
- **No local models** for now. That's probably its own feature later.
- **Core, not a plugin.** `plugin-queue.md` skipped "Ask Claude" as a
  plugin on 2026-10-13 because AI should be core. Plugins also can't
  start processes or stream UI.
- **UI:** no gpui-kit dependency, per
  [`gpui-kit-eval.md`](../docs/gpui-kit-eval.md); a few small pieces are
  worth copying (see [gpui-kit](#gpui-kit) below). Markdown is our own.

## Signing in

Researched 2026-10-14. This constraint shapes the whole design.

**Claude.** As of 2026-10-14, the Claude Code
[Legal and compliance](https://code.claude.com/docs/en/legal-and-compliance)
page says:
- Third-party developers may not offer Claude.ai login in their own
  apps, or route requests through Free, Pro or Max plan credentials on
  their users' behalf.
- They may not collect, store or intermediate Claude.ai credentials or
  session tokens; sign-in must complete through Anthropic's own flow.
- Developers building on the Agent SDK should use API keys.
- It does allow "an end user signing in to the unmodified Claude Code
  binary with their own Claude subscription".

So sheaf never sees a credential: the user signs in inside the agent.

**Go/no-go for Claude.** Zed runs Claude through `claude-agent-acp`, an
adapter built on the Agent SDK. The terms don't say whether a
subscription sign-in through that adapter counts as the unmodified Claude
Code binary. Since sheaf takes no API keys, the answer decides whether
Claude can use the ACP path. **Ask Anthropic before building it.**

If the answer is no, drive the user's installed `claude` directly in its
headless mode, which is the unmodified binary:
- `claude -p --input-format stream-json --output-format stream-json`;
- sheaf's tools through `--mcp-config`;
- approval prompts back to sheaf through `--permission-prompt-tool`.

That means a second transport beside ACP, just for Claude.

**Codex.** Codex handles its own ChatGPT sign-in, and sheaf runs
`codex-acp`. OpenAI opened "Sign in with ChatGPT" to approved third-party
partners in October 2026, but that's for apps that call OpenAI
themselves, so it doesn't apply to an agents-only design.

## Architecture

```
sheaf chat panel ──ACP (stdio)──▶ claude-agent-acp / codex-acp ──▶ vendor
        ▲                                   │
        └────────── MCP: sheaf's tools ◀────┘
```

- **New crate `sheaf-agent`,** with no GPUI, like `sheaf-plugin`:
  - the ACP client over the agent's stdin and stdout (crate
    `agent-client-protocol`, Apache-2.0, 3.3.0 on 2026-10-14);
  - starting and stopping agents, sessions, streamed updates and
    permission requests;
  - the Claude headless transport, if the go/no-go goes that way.
- **sheaf's tools are an MCP server** (`rmcp`, Apache-2.0). There are two
  ways for the agent to reach it:
  - MCP over the ACP connection itself (`agent-client-protocol-rmcp`,
    behind its `unstable_mcp_over_acp` feature);
  - **`sheaf mcp`**, a stdio MCP server that reaches the running sheaf.
    How it does is in [Transport](#transport-researched-2026-10-20)
    below: a socket of its own is recommended over the single-instance
    socket this memo first planned on.
- **`sheaf mcp` is useful on its own.** Claude Code, Claude Desktop or
  Codex run from outside sheaf could use it with no panel and no sign-in
  questions, so it's the first slice.
- **Which agents:** Claude and Codex built in. A list in `settings.toml`
  (command, arguments, environment) adds or overrides them. The ACP
  Registry comes later.
- **The agent's own tools.** Claude Code and Codex come with shell and
  file-editing tools.
  - sheaf doesn't advertise ACP's `fs` and `terminal` client
    capabilities.
  - It limits each agent to sheaf's tools, through Claude Code's allowed
    and disallowed tools and Codex's sandbox and approval settings.
  - Not checked yet: how much of this each adapter lets a client set. If
    an agent can't be limited, its own tools need the same approval as
    sheaf's writes.

## Transport (researched 2026-10-20)

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
  `TMPDIR` (the TS SDK). `sheaf_instance::default_dir()` reads those, so
  `sheaf mcp` would look in `/tmp/sheaf-<uid>` while the GUI is in
  `/run/user/<uid>/sheaf`.
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
| 2 | The same over `instance.sock`, extended | Workable, no gain |
| 3 | The app hosts Streamable HTTP on 127.0.0.1 | Defer |
| 4 | Streamable HTTP over the socket | VS Code only; skip |
| 5 | A headless `sheaf mcp` that opens files itself | Not for M16 |
| 6 | MCP over ACP | The panel's, later |
| 7 | The relay speaks MCP; a private RPC to the app | Two protocols; no |

1. **Recommended.** Nothing listens on the network and browsers can't
   open Unix sockets. On Linux and macOS it gets `sheaf-instance`'s
   protections: a 0700 runtime folder the user must own, and a peer uid
   check on every connection. One MCP implementation lives in the
   process that owns the documents, undo and the approval card, and the
   same handler can later sit behind option 3 or 6. The instance
   socket's hand-off protocol is left alone. Unity ships this shape: a
   stdio relay to the Editor over a Unix socket or named pipe, with
   in-app approval of each client.
2. **The instance socket, extended.** A `{v: 2, kind: "mcp"}` first line
   switches the connection to MCP. A v2 owner can keep accepting v1
   opens, so hand-offs needn't break. But the listener is serial, with 2
   s timeouts and a 1 MiB request cap built for one-shot hand-offs, so it
   needs reworking for long-lived connections, and a second socket file
   costs nothing.
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
- **Find the runtime folder without the environment:** `/run/user/<uid>`
  when it exists and the user owns it, `confstr(_CS_DARWIN_USER_TEMP_DIR)`
  on macOS. Windows is fine (both lists keep `LOCALAPPDATA`).
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
- **Windows:** the socket has no peer check today, only the profile
  folder's ACL, and that has to change before document content flows.
  Either bridge `uds_windows` into tokio with a reader and a writer
  thread per connection, or use a per-user named pipe. A pipe name is
  machine-wide, so it needs the user's SID in the name,
  `FILE_FLAG_FIRST_PIPE_INSTANCE`, and a check of the server's SID by
  the relay before it sends anything (against another user squatting the
  name).
- **Other environments:** WSL2 (a Linux `sheaf` can't reach a Windows
  AF_UNIX socket, so docs point at `sheaf.exe mcp`), VS Code's sandbox
  for stdio servers, and Flatpak or Snap packaging all change what the
  relay can see.

### Decisions at kickoff

1. A socket of its own, or a new request on `instance.sock`?
   Recommended: its own.
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
  and §20.

## Tools

Most already exist as the plugin host API (`wit/sheaf.wit`). The MCP
tools translate them.

| Tool | Comes from |
|---|---|
| Document: path, pages, outline, marks per page, selection, current page | WIT `document` |
| Text: page text, search, pattern search, words, text boxes | WIT `text` (unsaved OCR layers included) |
| Page images, as PNG for models that can see | WIT `render`, with caps on size and count |
| History: versions with id, parent, label, author, time and edits in words; the branches | `VersionView`, `describe.rs`, `Document::edge` (M9 slice 4, done) |
| Commands: id, label, category, help, keywords, whether it can run now | `CommandRegistry`. Built-ins can't register without help (`command_info.rs`), nor can plugin commands (manifest loader). |
| Submit edits | `sheaf_doc::Edit` (serde) through `apply_batch`, one version. The same as the plugin `edits` interface. |
| Run a command | The registry's target: a built-in action or `plugin::Run` |
| Go to a version | `Document::jump` |

**Versions keep reads and writes honest.** Every tool returns the current
`VersionId`. Writes take the version they were based on and are refused
when the document has moved since, because the user edited in the
meantime. The agent then reads again.

## Running commands live

**The gap.**
- Built-in commands are mostly argument-free `actions!` that act on the
  selection or open a dialog (OCR, Bates, headers and footers, split…).
  An agent can't fill in a GPUI dialog.
- Plugin commands take `args-json` with no schema, and `show-form` waits
  for the user.

**The plan.**
- **With an argument schema,** a command runs without its dialog. The
  schema comes from schemars, as `plugin::Run`'s already does.
- **Without a schema,** the call opens the command's dialog, pre-filled
  from the arguments where the dialog supports it. It returns when the
  user closes the dialog, as done or cancelled.
  - The user is watching anyway, so this is acceptable.
  - Every command is usable from the first day.
- **Schemas are added over time.** M15 batch actions need the same
  schemas, so the two share that work. With the dialog fallback, this
  milestone doesn't have to wait for M15.
- **Commands that act on the selection** either take the pages in their
  schema, or the agent sets the selection first.
- **Plugins:** an optional `args` schema per command in the manifest.
  `show-form` is answered from the arguments when they cover every field.

**History.**
- Each approved call becomes one version, labeled "<agent>: <command or
  edits in words>".
- Its author is the agent on the user's behalf ("Claude for GE").
  `apply_batch` needs an author parameter for this; today every version
  takes the identity from `settings.toml`.
- **Revert this turn:** a button on each agent turn that jumps to the
  version before the turn's first edit. A jump isn't an edit, so it costs
  nothing and can be redone.

## Permissions

- **Reading the open document** is allowed after the user consents, the
  first time that document goes to a cloud agent.
  - Tax return information falls under IRC §7216, which limits disclosing
    it to third parties without the taxpayer's consent. Firms will ask
    about this first.
  - Open: the wording, and whether consent is per document, per client or
    per user.
- **`settings.toml`:** `[agent] enabled` and the allowed agents. A firm
  can ship settings with agents off. sheaf only reads that file.
- **Every write asks first:** Allow once, Always for this command in this
  thread, or Reject.
  - For sheaf's own tools, sheaf prompts in its MCP handler, so the rule
    holds whatever the agent does.
  - Tools the agent itself asks about arrive as ACP
    `session/request_permission`.
- **Never "always":** Save, Save As, exports and anything else that writes
  files; commands that touch other files (Combine, Split); opening URLs.
- **Plugin commands** run with the plugin's own permissions. The agent
  can't grant more.
- **A log of what was sent:** tool calls and their results, per thread,
  in the local state folder. Open: whether a firm needs to keep it.

## Panel

- A new `Panel::Agent` in the right dock, beside History.
- **Threads:** one per document (one document per window today), kept in
  the state folder, with a list of past threads.
- **Agent picker:** Claude or Codex, with each one's sign-in status.
  "Sign in" runs the agent's own login (ACP `authenticate`, or the agent's
  own login command).
- **Message list:** GPUI's `ListState` with `FollowMode::Tail`, which our
  pinned Zed revision already has (`gpui/src/elements/list.rs:113`); Zed's
  own agent panel uses the same thing. Plus a jump-to-latest button.
- **Replies:** our own Markdown renderer on `pulldown-cmark` (MIT):
  - paragraphs, emphasis, inline code, code blocks without highlighting,
    lists, tables and links;
  - page references ("p. 12") as links that go to the page;
  - while streaming, only the last block is parsed again;
  - a Copy button per reply. Drag-selection across replies can come
    later.
- **Tool-call cards:**
  - collapsible, showing the arguments, and the result in words
    (`describe.rs` for edits);
  - the approval buttons;
  - status: running, done, failed or rejected.
- **ACP `plan` updates** as a checklist.
- **The message box** is `text_area.rs`.
  - Enter sends, Shift+Enter starts a new line. Check against the note
    editor, where Ctrl+Enter confirms.
  - @-mentions of pages, bookmarks and marks use the command palette's
    list.
  - Chips attach the selection or a region (`region.rs`).
  - This is where porting gpui-kit's undo/redo and word movement into
    `text_area.rs` pays off most (the evaluation's first suggestion), if
    that isn't done by then.
- **Cancel** (`session/cancel`), and a working indicator.

## gpui-kit

Checked 2026-10-14 against gpui-kit 0.7.3.

**Licenses:**
- gpui-kit, gpui-component and gpui-base are Apache-2.0, so we can copy
  from them with attribution.
- Zed's `agent_ui`, `acp_thread`, `agent_servers` and `markdown` are
  GPL-3.0-or-later (checked in our Zed checkout). We can study how they
  behave but can't copy their code.

**Depending on it** is still the wrong trade. Even `gpui-base` alone
pulls in `lsp-types`, `html5ever`, `markdown` and `ropey`, and pins
`gpui-pre =0.3.9`.

| Piece | Size | Verdict |
|---|---|---|
| `message_scroller.rs` | 508 lines | Not needed. It wraps GPUI's `ListState` tail-follow mode, which we already have; it only adds a jump-to-latest button. |
| Markdown view, `gpui-base/src/text/` | ~22k lines, plus 4.7k for window-wide selection | The one real candidate: streaming append (`TextViewState::push_str`), re-parsing only what's new, fade-in (`stream_fade.rs`), tables, selection and copy. But it's tightly tied to kit's theme, animation and selection code. Use it as the reference for ours, and copy it only if drag-selection across replies is wanted. |
| `collapsible.rs` | 145 lines | Small; copy or rewrite for tool-call cards. |
| `clipboard.rs` | 163 lines | Small; copy or rewrite for Copy buttons. |
| `shimmer.rs` | 657 lines | Optional polish for "Running OCR…"; a spinner does the job. |
| `message.rs`, `bubble.rs` | ~1k lines | No. Layout slots with little logic, and the agent's output is full-width, not bubbles. |
| `diff/` | 8k lines | No. It shows text patches; sheaf shows an agent's changes as edits in words. |
| Attachments, speech, syntax highlighter | | No. |

## Slices (first cut)

1. **`sheaf mcp`:** read tools, commands and history, through a socket of
   its own ([Transport](#transport-researched-2026-10-20)). Test it with Claude Code and Codex run from a terminal.
2. **Writes:** edits and commands with version checks, the approval prompt
   in sheaf, versions authored by the agent, the dialog fallback.
3. **`sheaf-agent`:** the ACP client. Codex first, since it doesn't wait
   on the go/no-go; then Claude through ACP or through the headless
   fallback.
4. **The panel:** threads, streaming Markdown, tool-call cards, approvals,
   cancel, the message box.
5. **Review:** Revert this turn, page links, @-mentions and attachments.
6. **Policy:** consent, the settings, the send log, user docs.

About the size of M6 (plugins). The protocol crates make the plumbing
small; the panel, the approval and review UX, and argument schemas for
about 90 built-in commands are the bulk.

## Open

- Anthropic's answer on subscription sign-in through `claude-agent-acp`.
- How far each adapter lets sheaf limit the agent to sheaf's tools.
- MCP over ACP or `sheaf mcp` inside the panel: `sheaf mcp` until an
  adapter supports MCP over ACP ([Transport](#transport-researched-2026-10-20)).
- Threads: per document or per window, how long they're kept.
- §7216 consent: wording, scope, and whether the send log is kept.
- Page images: caps on size and count.
- Commands an agent should never run.
- Placement: after M15, or before it now that the dialog fallback removes
  the dependency.

## Sources

Checked 2026-10-14.
- [Claude Code: Legal and compliance](https://code.claude.com/docs/en/legal-and-compliance)
  (authentication and credential use)
- [Zed docs: External Agents](https://zed.dev/docs/ai/external-agents)
- [Zed blog: The ACP Registry is Live](https://zed.dev/blog/acp-registry)
- [ACP protocol overview](https://agentclientprotocol.com/protocol/overview)
- [`agent-client-protocol`](https://docs.rs/agent-client-protocol/) and
  [`agent-client-protocol-rmcp`](https://docs.rs/agent-client-protocol-rmcp/)
- [Which coding agents support ACP](https://agentrq.com/blog/which-coding-agents-support-acp-agent-client-protocol)
  (the `claude-agent-acp` and `codex-acp` adapters)
- [The New Stack: Sign in with ChatGPT](https://thenewstack.io/sign-in-with-chatgpt/)
  and [OpenAI help](https://help.openai.com/en/articles/20001542-using-your-chatgpt-plan-in-other-apps-and-sites)
- [The Register: Anthropic clarifies ban on third-party tool access](https://www.theregister.com/software/2026/02/20/anthropic-clarifies-ban-on-third-party-tool-access-to-claude/5014546)
