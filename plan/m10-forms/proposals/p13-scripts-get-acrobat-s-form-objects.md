# M10 proposal 13: Scripts get Acrobat's form objects, and nothing that reaches out

Part of [M10: Forms](../README.md) · [all proposals](README.md)

**Scripts get Acrobat's form objects, and nothing that reaches out.**
- **In:** the AF functions; `event` (`value`, `rc`, `change`,
  `willCommit`, `target`, `selStart`, `selEnd`…); `this` as the
  document (`getField`, `numFields`, `getNthFieldName`, `resetForm`,
  `calculateNow`, `print`); Field values and properties (`display`,
  `readonly`, colors, `charLimit`, items, `setFocus`); `app.alert`,
  `beep` and `response`; `util.printf`, `printd`, `scand` and
  `printx`; `console.println`; `color` and `display`.
- **Harmless no-ops,** never left undefined (an undefined method
  throws and stops the rest of the script): submitting and mailing,
  URLs and launching, data objects, importing and exporting data,
  `saveAs`, `closeDoc`, `app.quit`, setting `dirty`, opening
  documents, privileges, menus and toolbars, page templates
  (`spawn`), adding or removing fields and pages. Each one is
  logged. None touches sheaf's modified flag (Ohio's open action
  sets `dirty = false`).
- **Never:** network, files, fullscreen (`app.fs`), password prompts
  (`app.response` with `bPassword` returns nothing), silent printing.
- **What the guest sends back is untrusted.** Field ids are checked
  against the model; a value is at most 64 KiB and a list at most
  10,000 items; updates per event are capped, and the rest dropped
  and logged. pdf.js's page-num command is honored only during a
  user action; zoom and layout are ignored and logged.
- **Dialogs** are sheaf's, titled with the document's name and
  saying they come from it. One per user action within 5 s, as
  pdf.js gates them (app.js:427-430), with "Don't show more from this
  document". `print` needs a user action and always opens the print
  dialog.
- **Focus:** a script's `setFocus` is honored once per user action,
  so a validate that refocuses its field can't trap the user, and Esc
  always leaves.
- **Timers** (`setTimeOut`, `setInterval`; pdf.js needs one for its
  activation window) are capped in number and interval, paused while
  the window is hidden, and stopped at close.
- **`eval` stays on.** Georgia's and Ohio's generated libraries use
  it, and it can't leave the guest.
- **Adobe's version checks never run.** Every IRS form has three
  `!ADBE::` document scripts. Depending on `app.viewerType` and
  `viewerVersion` they call `app.alert`, `app.response`,
  `app.launchURL`, `app.findComponent` or `getURL`; under the corpus
  run's stand-in (version 0) every IRS form reached `app.response()`
  on open. sheaf skips `!ADBE::*` scripts and sets
  `ADBE.Reader_Value_Asked` and `Viewer_Value_Asked`, as pdf.js
  does.
- **When scripts run:** at open, the worker sets up the document's
  sandbox in the background (decision 3), and no form JavaScript runs.
  The document's own scripts start at the first interaction with one
  of its fields, with pdf.js's Open event, where document-level
  scripts run (7 ms for NY IT-201, 70 ms for GA 500, in the guest).
  Until then the stored appearances show.
  - Values the document scripts set as they start (Ohio's `DS_Init`
    sets `today` and `UPC_ScanDate`) are held and join that first
    commit's edit. If focus leaves with no commit, they become their
    own step, "Form set 2 fields" (proposal 9).
  - When a store is rebuilt (after a trap, or evicted), the model's
    values are pushed in and the scripts' start-up writes are
    dropped: the model is the source of truth.
  - `/OpenAction` JavaScript, page open and close actions, and
    document events (WillSave, DidSave, WillPrint, DidPrint) don't
    run in M10. They're logged.
- **Triggers:** keystroke, validate, calculate and format; Focus,
  Blur, Mouse Down, Mouse Up, Enter and Exit (Ohio has 3 Mouse Down
  actions, Georgia 1 Exit).
- **Order,** as Acrobat documents it and pdf.js and PDFium implement
  it: keystroke on each key; keystroke with `willCommit`; validate;
  the value commits; every field in `/CO` calculates, each running
  its own validate and format; then the field's format. Format
  changes only the appearance, and `/V` stays raw. A calculation
  that sets fields doesn't start another round. A value typed into a
  calculated field lasts until the next calculation, as in Acrobat.
- **Where implementations differ,** sheaf does what the forms expect
  of Acrobat:
  - a rejected commit (`rc = false`) restores the previous value, as
    Acrobat's reference says and PDFium does (pdf.js clears the
    field);
  - validate may rewrite `event.value` (pdf.js; PDFium drops it). CA
    DE-4 and the I-9 depend on it;
  - a Calculate event starts with the field's current value (pdf.js
    starts it at `null`, and Ohio's scripts throw; proposal 12's
    patch);
  - `AFTime_Format` reads 24-hour values, and date and time
    keystrokes check the range (pdf.js does neither; proposal 12);
  - numbers round as `toFixed` does (pdf.js: 1.005 to two places is
    `1.00`; PDFium adds 1e-15 and gives `1.01`). Acrobat's own answer
    is in the Asks.
- **Errors** name the field and trigger in a script log (Forms >
  Script Log…), and filling goes on.
- **`[forms] scripts = false`** in `settings.toml` turns scripts off
  firm-wide; Forms > Run Scripts turns them off for one document, for
  the session, and "Don't show more from this document" lasts the
  session too. Nothing is kept by a client file's path. The Rust AF
  path still runs.
- **Out of reach in M10:** Georgia's and Ohio's long tail (spawning
  page templates with renamed fields, `execDialog`, `buttonSetIcon`,
  a print barcode built through `eval`, moving fields at run time).
  Those forms still fill and print without them.
