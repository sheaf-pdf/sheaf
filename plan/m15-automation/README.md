# M15: Automation

Status: sketched · CLI groundwork shipped

Proves: Acrobat Pro + PDFlyer parity · Depends on: Batch: each step's milestone (M10's `sheaf form` for fill and flatten, M11 for redact by pattern, M14 for optimize). Parity audit and stabilization: last, after everything else

## Summary

- Action Wizard-style batch actions: run a sequence of commands (OCR, Bates,
  optimize, redact by pattern, flatten, export) over files and folders.
- Built on the M6 plugin host API, which settles the README's Rhai vs Lua
  question: plugins first, with an optional Rhai front end for quick macros
  if users want one.
- Headless CLI batch mode (`sheaf batch …`). The split is in place: GUI-free
  operations live in `sheaf-ops` (with the `in-progress` crate for progress
  and cancellation), and `sheaf-cli` holds the subcommands. They're reachable
  as `sheaf <subcommand>` or through the GPUI-free `sheaf-cli` binary;
  `export`, `images-to-pdf` and `scan` ship today. Ops move into
  `sheaf-ops` as subcommands need them, and `batch` chains them.
- Parity audit: walk Acrobat Pro's menus and tools and PDFlyer's toolbar
  item by item; anything still missing becomes a slice here or is moved to
  Not planned with a reason.
- Stabilization: corpus round-trip tests, Windows bundle and CI
  workflows exercised, settings/keymap format declared stable.

## Plan files

- No plan yet: the milestone is broken into slices when it starts.
- [Build or adopt](../research/build-or-adopt/README.md): the M15 rows, and the [changes to make](../research/build-or-adopt/changes-to-make.md) at kickoff
- Open questions: [asks](../asks.md)
