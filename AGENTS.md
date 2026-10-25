# Agents: start here

sheaf is a native PDF editor in Rust (GPUI, PDFium for rendering, its own
writer). This file says where the plan lives, how to read it without
spending tokens on finished work, and how to keep it that way.

## Reading the plan

Read top down, and stop as soon as you have what you need:

1. **[ROADMAP.md](ROADMAP.md):** the index. What's next, every milestone's
   status and dependencies, and links to everything else.
2. **A milestone's card,** `plan/<milestone>/README.md`: its summary, where
   it stands, its slice table (status and what each slice depends on) and
   links to its plan files.
3. **A slice's file,** `plan/<milestone>/slices/sNN-*.md`: what to build,
   where in the code, and how it's checked. Slice text cites decisions
   ("decision 2"), proposals ("proposal 11", one file each in
   `proposals/`), bugs found while planning ("bug 10", in `found.md`) and
   build-or-adopt entries ("entry 17", in `plan/research/build-or-adopt/`).
   Open only the ones the slice names.
4. **[plan/complete/](plan/complete/):** finished milestones and slices,
   with their notes, and closed backlog entries. Read a file there only
   when you need the history; `rg` skips the folder (`.ignore`), so search
   it with `rg --no-ignore-dot`. M10.5's design (`plan/complete/m10.5-edit-api/design/`,
   cited as §N) describes the edit API that everything since builds on.

Also in `plan/`: [asks.md](plan/asks.md) (questions waiting on Greg),
[the backlog](plan/backlog/README.md), [Pending](plan/pending.md),
[the parity checklist](plan/parity.md), [libraries](plan/libraries.md),
[not planned](plan/not-planned.md), `topics/` (plans that aren't
milestones) and `research/`.

For users the docs are [README.md](README.md) (what works today, and the
progress table) and [docs/](docs/) (features, CLI, configuration,
building, architecture).

## Names

- Milestones are `M<n>`; slices are "M10 slice 7", in
  `plan/m10-forms/slices/s07-*.md`. Numbers never change.
- Backlog entries are `B<n>` (bugs) and `F<n>` (features), one file each,
  `plan/backlog/B<n>.md`. IDs are never reused.
- Build-or-adopt entries are numbered (`plan/research/build-or-adopt/NN-*.md`);
  its decisions are `D<n>`.

## Keeping it small

- **Finished work moves out in the PR that finishes it.** A finished slice's
  file moves to `plan/complete/<milestone>/`, with `Status: done` and its
  notes under `## Notes`; the card's row links there and says `done`. A
  finished milestone's card and files move to `plan/complete/` and its
  ROADMAP row says `Done`. Update README's "What works today" and
  "Progress", and `docs/features.md`, when users get something new.
- **Out-of-scope finds go in the backlog,** not into the current slice: a
  new `B<n>.md` or `F<n>.md` from the templates in
  [the backlog's README](plan/backlog/README.md), and its row there. A
  closed entry moves to `plan/complete/backlog-closed.md`.
- **Questions for Greg** go in [asks.md](plan/asks.md), one line each, with
  a link to where they're argued. When one is ruled, record the ruling
  there and delete the line.
- **Status lines.** Every card, slice and backlog entry has
  `Status: <word>` on its third line: `open`, `in progress`, `done`,
  `deferred` or `blocked` for slices and entries; `planned`, `sketched`,
  `paused`, `deferred`, `in progress` or `proposed` for cards. The tables
  that list them must say the same.
- **Size budgets:** ROADMAP.md 12 KB, a card 12 KB, the backlog's README
  16 KB, any other plan file 24 KB (`plan/complete/` has none). Split a
  file that outgrows its budget.
- **`just plan-check`** checks all of this: the tables against the files,
  the status lines, every link, and the budgets. It runs in
  `just precommit`.

## Working in the repo

- Build, test and the `just` recipes: [docs/building.md](docs/building.md).
  Before a commit, `just precommit` (what CI can't run); before a PR
  merges, `just premerge`.
- Search with `rg`, not `grep -r`.
- Never weaken a test or a gate, and never skip corpus files to make one
  pass. The test corpus (`../pdf-corpus`) is read-only.
- sheaf is pre-alpha: redesign cleanly rather than shim old sheaf APIs or
  formats. PDF interoperability is still required.
- GPL, LGPL and AGPL code may be studied and cited, never copied,
  translated or linked; running such a tool as a separate process in
  tests is fine.
