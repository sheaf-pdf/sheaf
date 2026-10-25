# Build or adopt 80: Same-user checks on Windows (named pipes or AF_UNIX)

Area: M16: Agent tools (MCP), and the agent panel (Pending) · Part of [build or adopt](README.md)

- **Need:** only the same user may connect; the relay checks the
  server's SID before sending anything, against another user squatting
  the name; remote clients are rejected, and the pipe is created as the
  first instance.
- **Where:** M16: `agent.md:274-282`, and decision 2 at `:291-292`
  ("bridge threads, or a per-user named pipe? And which peer check lands
  before client data flows?"). D16.
- **Candidates:**
  - **tokio's named pipes:** `ServerOptions::first_pipe_instance`,
    `reject_remote_clients` and `create_with_security_attributes_raw`
    (`unsafe`; `tokio-1.54.0/src/net/windows/named_pipe.rs:2057-2310`).
  - **interprocess 2.4.6** (606★): named pipes and local sockets with
    security descriptors, `FILE_FLAG_FIRST_PIPE_INSTANCE` and
    `PIPE_REJECT_REMOTE_CLIENTS`; `client_process_id`,
    `server_process_id`, `client_session_id` and `impersonate_client`
    (`src/os/windows/named_pipe/stream/impl.rs:57-132`). Its
    cross-platform `PeerCreds` gives only a pid on Windows.
  - **uds_windows** 1.2.1 (already used) plus the `SIO_AF_UNIX_GETPEERPID`
    ioctl, defined in mingw-w64's
    [`afunix.h`](https://mingw.googlesource.com/mingw-w64/+/609a2498c115ccc355ade1638499d10ed36a940e/mingw-w64-headers/include/afunix.h)
    and used by [GLib](https://src.opensuse.org/luc14n0/glib/src/tag/2.72.0/gio/giowin32-afunix.h);
    Microsoft's documentation for it wasn't found. It would keep today's
    socket.
  - **The SID comparison has no crate:** OpenProcess → OpenProcessToken →
    GetTokenInformation(TokenUser) → EqualSid, or
    ImpersonateNamedPipeClient → OpenThreadToken. About 50 lines on the
    windows crate, which sheaf already depends on.
- **Recommendation:** **build the SID check on the windows crate,** over
  tokio's named pipes; interprocess if a synchronous API is wanted.
  Medium.
