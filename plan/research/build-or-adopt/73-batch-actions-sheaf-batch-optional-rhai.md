# Build or adopt 73: Batch actions, `sheaf batch`, and the optional Rhai front end

Area: M15: Automation · Part of [build or adopt](README.md)

- **Need:** a versioned step schema over `Op`, a host runner, a CLI, and
  maybe a macro language for non-programmers.
- **Where:** `ROADMAP.md:462-479`; M10.5 §20 "M15 (batch)"
  (`m10.5-edit-api.md:3681-3686`); README's open question
  (`README.md:365-366`) and its feature row (`:304`).
- **Today's plan:** "Built on the M6 plugin host API, which settles the
  README's Rhai vs Lua question: plugins first, with an optional Rhai
  front end for quick macros if users want one" (`ROADMAP.md:466-468`).
- **Candidates:** [rhai](https://crates.io/crates/rhai) 1.27.0
  (2026-10-21; MIT OR Apache-2.0; 106 releases since 2016; 13.4M
  downloads; pure Rust; builds for wasm). `Engine::set_max_operations`
  stops runaway scripts
  ([Rhai book](https://rhai.rs/book/safety/max-operations.html)), and
  "Rhai is designed to not bring down the host system"
  ([Safety](https://rhai.rs/book/safety/)).
  [mlua](https://crates.io/crates/mlua) 0.12.2 (2026-10-03; MIT): Lua or
  LuaJIT in C through FFI.
- **Fit:** a step list is serde and schemars data, like `Op`, and needs
  no crate. Rhai fits a macro front end that calls the same commands.
  Macros are the user's own (or the firm's), not something a PDF
  carries, so running them in process with operation limits is
  defensible. If macros are shared across a firm, the boa approach (a
  wasm guest) applies too.
- **Recommendation:** **build** the runner and step schema in-house. For
  macros, **undecided:** if users ask for macros beyond plugins (the
  roadmap already says "if users want one"), adopt Rhai. Medium.
