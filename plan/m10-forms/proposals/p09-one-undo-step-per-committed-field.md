# M10 proposal 9: One undo step per committed field

Part of [M10: Forms](../README.md) · [all proposals](README.md)

**One undo step per committed field.**
- A commit is leaving a field (Tab, Enter, a click elsewhere), a
  click on a check box or radio button, or a choice picked. Typing
  inside a field isn't history; Esc reverts it.
- The edit, `Edit::FieldsSet`, holds the committed value and every
  value calculations changed, resolved. Replay never runs a script,
  so history stays exact (M9 decision 16), even for a script that
  reads the clock. Values document scripts set as they start are held
  and join the first commit's edit; if focus leaves with no commit,
  they become a step of their own, "Form set 2 fields" (proposal 13).
- Import, Clear Form and a Reset button are one step each, and so is
  a flatten. Authoring is one step per gesture ("Move 6 fields").
- History reads "Fill “Line 1a” on page 1", "Fill “Line 1a” (and 3
  calculated fields)", "Fill “Name” (and 2 set by the form)", "Import
  form data (24 fields)", "Clear form".
- **Pending commits.** A document's field events go through one
  queue, in order.
  - Any command that reads or writes the document (save, autosave,
    print, export, undo and redo, a History jump, page tools, close,
    plugin calls) first commits the focused field and waits for the
    queue to drain, up to the 1 s commit deadline.
  - A calculated value for the field being typed in waits until that
    field commits.
  - A rejection that lands after focus moved still restores the
    previous value.
- **Ctrl+Z in a field** undoes the field's own typing, and reverts it
  as Esc does when there's nothing left; Ctrl+Y and Ctrl+Shift+Z
  redo (both are document Redo today, main.rs:497-498). None of them
  reaches document Undo or Redo. Today Ctrl+Z would undo the previous edit
  and orphan the typed text (bug 11).
