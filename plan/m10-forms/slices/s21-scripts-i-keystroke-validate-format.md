# M10 slice 21: Scripts I: keystroke, validate, format

Status: open · Part of [M10: Forms](../README.md)

**Scripts I: keystroke, validate, format** (proposal 13).
- **On the edit API:** what keystroke, validate and format scripts
  set commits `Validate` into the focus session's Typing group, so a
  commit and what its scripts did are one undo step.
- The event model wired into the editor and commits, with Focus,
  Blur, Mouse Down, Mouse Up, Enter and Exit; document scripts at the
  first interaction, their writes folded into that commit;
  `!ADBE::` scripts skipped.
- Dialogs and their gate; the no-ops (`dirty`, `closeDoc` and the
  rest); `setFocus` once per user action; timers; the script log;
  `[forms] scripts` and Forms > Run Scripts.
- **Checks:**
  - PA-40's upper-casing keystroke; Ohio's digit filter; CA 540's
    date check on Blur shows one dialog per action; Georgia's SSN
    `printx` format;
  - an IRS form opens and fills with no dialog; a script error names
    its field and filling goes on;
  - on a form whose document scripts set values at start-up (Ohio's
    `today`, or a fixture like it), the first focus and commit are
    one undo step, and undoing it restores the start-up values; a
    first focus that leaves without a commit makes its own step,
    "Form set 2 fields";
  - a hostile test per "never" and no-op item (a submit,
    `launchURL`, `closeDoc`, `dirty = false`, `print` without a user
    action, `beep`, a `setFocus` loop…), each leaving the document,
    its modified flag and the disk as they were.
