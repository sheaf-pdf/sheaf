# M10 slice 22: Scripts II: calculate

Status: open · Part of [M10: Forms](../README.md)

**Scripts II: calculate** (decision 3; proposals 9 and 13).
- **On the edit API:** the `/CO` cascade resolves on the worker and
  commits `Validate` into the user's Typing group; replay never runs
  it, and a save waits for a running cascade's group.
  `PageSource::Template` is the op a script's `spawn` lowers to
  (pages from `/Names /Templates`, which M10.5 keeps unmodeled);
  proposal 13 left spawning out of reach in M10, so it lands here
  only if a form needs it at kickoff (recommended: with `spawn`, not
  before).
- The `/CO` cascade after a commit, each calculated field's validate
  and format, no re-entry, and the results in the commit's one edit;
  a calculated value for the field being typed in waits for its
  commit.
- `sync` after every edit that doesn't come from the sandbox.
- Forms > Recalculate; `[forms] auto_calculate`; imports and resets
  recalculate.
- Property changes from scripts (`display`, `readonly`, colors) reach
  the model, as ops, and the appearance.
- Headless in `sheaf form fill` and `import`, with `--no-scripts`.
- **Checks:**
  - Georgia's `GA_Calc()` runs end to end within the deadline;
    Ohio's 319-field cascade lands as one edit with no script error,
    and its `dsRound()` rounds; a calculation that sets its own
    inputs doesn't loop;
  - undo takes a commit and its calculated values together; with B =
    A×2, commit A, undo, commit A again, and B is right; fill, delete
    a page holding an input, recalculate, and no stale value comes
    back;
  - a save while a cascade runs waits for it and saves its results;
  - `sheaf form fill` of NY IT-201 writes formatted appearances; the
    state forms' replays pass with scripts on.
