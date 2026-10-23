# Backlog: bugs and features found along the way

Status: **open log** (started 2026-10-21). Bugs and feature ideas that
turn up during milestone work, written down so they aren't lost. Nothing
here is scheduled. We don't fix these in the middle of a milestone
phase; they're triaged between phases.

## How to use it

- **Add, don't fix.** When something comes up that isn't part of the
  current slice, write an entry and keep going. Agents working a slice
  do the same.
- **One entry per item**, newest at the bottom of its section. IDs are
  never reused: `B1`, `B2`… for bugs, `F1`, `F2`… for features.
- **Check for overlap first.** A feature already in `ROADMAP.md` (a
  milestone, the parity checklist, Pending or Not planned) or in a
  `todos/` memo gets a line there, or a link here, not a duplicate.
- **Triage between phases.** Each entry moves to [Closed](#closed),
  saying where it went:
  - fixed, with the commit or PR;
  - a slice in a milestone plan;
  - a feature into `ROADMAP.md` Pending, where every feature proposal
    starts;
  - its own memo in `todos/`, if it needs research;
  - won't fix or not planned, with a reason.

### Bug entry

```markdown
### B<n>: <short title>

- **Found:** <date>, <who>, during <milestone/slice>
- **Where:** <crate, file:line, command or view>
- **Severity:** data loss | wrong output | crash | annoyance
- **What happens:** <observed>
- **Expected:** <what should happen>
- **Repro:** <steps, corpus file, or test>
- **Notes:** <suspected cause, related entries or memos>
```

### Feature entry

```markdown
### F<n>: <short title>

- **Found:** <date>, <who>, during <milestone/slice>
- **What:** <the feature>
- **Why:** <who needs it, what it unblocks>
- **Overlap:** <roadmap items or memos it touches, or none found>
- **Notes:** <prior art, rough size, open questions>
```

## Bugs

### B2: Scrolling over a dialog also scrolls the page behind it

- **Found:** 2026-10-21, Greg, in the command palette, during M10.5
- **Where:** `sheaf_ui::dialog::backdrop` (`crates/sheaf-ui/src/dialog.rs`)
- **Severity:** annoyance
- **What happens:** with the command palette open, the scroll wheel
  scrolls the palette's list and the page view underneath at the same
  time.
- **Expected:** a modal dialog takes the wheel. Only the dialog
  scrolls, or nothing does.
- **Repro:** open a document long enough to scroll, open the palette
  (Ctrl+Shift+K), and scroll over its list.
- **Notes:**
  - **Cause, from reading the code (not yet reproduced or tested):** the
    backdrop is a full-window `absolute().inset_0()` div with a
    mouse-down listener, but no `.occlude()`. In GPUI a scroll handler
    runs when its hitbox `should_handle_scroll`, and only a hitbox in
    front set to block the mouse (`occlude()`) stops that.
    `block_mouse_except_scroll` lets scrolling through on purpose
    (`gpui-pre` 0.3.9 `elements/div.rs`, `window.rs`). So the page
    view's `.on_scroll_wheel(… on_wheel …)` and its `overflow_scroll`
    both still fire, and so does any panel under the cursor.
  - **The fix is probably one line:** `.occlude()` on the backdrop. The
    palette's list draws above its own backdrop, so it keeps scrolling.
  - **It's every dialog, not just the palette:** 20 dialogs use the
    backdrop (export, print, OCR, scan, combine, split, insert and
    replace, crop, watermark, header/footer, labels, blanks, properties,
    Set Status, bookmark tools, toolbar, plugin manager, plugin forms).
    Overlays that don't use it (menus, popovers, context menus) mostly
    call `.occlude()` already; check the rest.
  - **Test:** a headless GPUI test that opens the palette, sends a
    scroll-wheel event over it, and checks that the page view's scroll
    offset hasn't changed. GPUI tests are Linux-only, so the helpers
    need `cfg(all(test, target_os = "linux"))`.

### B3: Properties says "Fast Web View: Yes" after an incremental save

- **Found:** 2026-10-22, Claude, while Greg and Claude discussed where
  linearization belongs (M14), during M10.5 slice 18
- **Where:** the vendored lopdf's `vendor/lopdf/src/lazy.rs:443`, which
  sets `XrefFacts.linearized`; `crates/sheaf-doc/src/materialize/append.rs:477`,
  which carries it into the output record; shown by
  `crates/sheaf-app/src/properties_dialog.rs:147`
- **Severity:** wrong output
- **What happens:** a file counts as linearized whenever `/Linearized`
  appears within 1,024 bytes of its header. An Append keeps the input's
  flag, and a reopen finds the same dictionary at the top. So after
  sheaf (or any tool) appends an update to a linearized file,
  Properties still says Fast Web View: Yes. The dictionary's `/L` and
  hint tables no longer describe the file, and readers drop the fast
  path.
- **Expected:** Yes only when the linearization dictionary still holds.
  At least `/L` must equal the file's length (ISO 32000-1 Annex F: an
  updated file whose `/L` doesn't match isn't treated as linearized).
  After an Append, the output record says not linearized.
- **Repro:** found by reading the code, not yet run. Take a linearized
  corpus file (the pool sample has 216), add a mark, save (an Append),
  and open Properties. `qpdf --check-linearization` on the saved file
  should disagree with sheaf.
- **Notes:**
  - The fix is small: compare `/L` with the file length where the
    flag is set, and write `false` in the Append's record.
  - realize_corpus groups its stats by this flag, so its "linearized"
    row includes files updated after linearizing.
  - Writing linearized files is M14's (Optimize: linearization as a
    writer option).

### B4: The realizer's predicate and the emission disagree on a page that can't be read

- **Found:** 2026-10-22, Claude, in PR #26's pre-merge run
  (`realize_corpus --ignored`), during M10.5 slice 18. Greg merged #26
  with it logged here.
- **Where:** `crates/sheaf-doc/src/realize/untouched.rs`
  (`untouched`); `crates/sheaf-doc/src/materialize/emit/content.rs`
  (`stack`)
- **Severity:** annoyance (extra work; the pixels are right)
- **What happens:** `the_predicate_agrees_with_the_emission_on_every_page`
  fails on `pool/safedocs/0874/0874785.pdf` page 0, under all four
  inputs (screen, plugin, print, print without markup): "the model says
  false, the emission true", 4 of 242,055 checks.
  - The tier's edit puts a header ("Realized <<1>> of <<N>>", with a
    Bates slot) and an over-watermark on the page.
  - Page 0's content can't be read: stream 49 doesn't inflate ("invalid
    distance too far back"). That's real damage, not a decryption bug;
    qpdf and poppler fail on it too.
  - The emitter can't read the page's blocks, so it writes the page as
    its base with the Note "No … header or footer is drawn on page 1,
    whose content can't be read." To the emitter, the page is untouched.
  - The model never reads content. It sees generators on an unmarked
    page and says the page changed.
- **Expected:** the two agree. The model should call such a page
  untouched, so it draws from the base engine.
- **Repro:** `SHEAF_REALIZE_ONLY=pool/safedocs/0874/0874785.pdf cargo
  test -p sheaf-ops --test realize_corpus -- --ignored
  the_predicate_agrees_with_the_emission_on_every_page`
- **Notes:**
  - **Only conservative:** the page goes to its group's overlay, which
    writes it exactly as the base, so the screen never shows a stale
    page. The pixel tier passes on it.
  - **Since slice 17 (`bf7baf2`):** before, the legacy header installer
    wrapped the content without reading it, so the emission changed the
    page too (0 of 20 at `fa14236`, 4 of 20 at `bf7baf2`). The same gap
    has been there for a watermark alone since slice 16, hidden by the
    header.
  - **A proposed fix, about 60 lines in 4 files** (an investigation's;
    it passes the narrowed predicate and pixel tiers and the sheaf-doc
    lib):
    - `untouched` becomes `boxes_kept && annots_kept && (!(drawn_over
      || generated) || unread(def))`.
    - `unread(def)` is `(no markers || filters non-empty) &&
      !src.readable(index)`. That's exactly where `stack` decides by
      the page's blocks.
    - A new `Source::readable(index)` caches the answer either way,
      because `blocks()` doesn't cache a failure.
    - A test, `a_page_whose_content_cant_be_read_is_untouched_under_a_header`,
      builds an unclosed string beside a readable page and expects
      `[true, false]`.
  - **Its cost, for a decision:** the predicate would read a page's
    content, once per source page, on the UI thread (`view()`, and
    `find()` over up to 128 pages of a group the first time it's seen
    after a header is added). It's the same read the worker's delta does
    and they share the cache, so the total work doesn't change, but some
    of it moves to the UI thread. No fix keeps the predicate free of
    content reads and the tier strict: generator commits read no content
    (the Bates gate depends on it), and drawing over unreadable content
    isn't safe.
  - Render keys still include the generators for such a page: extra
    re-renders, never wrong pixels.

### B5: A file whose signatures were broken still asks about them on a reorder

- **Found:** 2026-10-22, M10.5 slice 17's corpus tier (pre-existing)
- **Where:** `forms::sigs::sig_impact` (`crates/sheaf-doc/src/forms/sigs.rs:187`)
- **Severity:** annoyance
- **What happens:** a file rewritten with `break_signatures` (the user
  already agreed to break them) still gets the signature ask when its
  pages are reordered. `sig_impact` judges by `/V` and the permissions,
  not by whether the signed bytes still verify. This is 25 of the
  62-file sample's 100 reorder Appends.
- **Expected:** a signature whose digest no longer matches its bytes
  doesn't ask again.
- **Repro:** append_corpus's header tier (slice 17) on a signed form
  after a forced Rewrite, then reorder.
- **Notes:** probably needs the digest check pyHanko does
  (`../pdf-corpus/signatures/` has files changed after signing and
  digest mismatches to test against; see F9).
- **Also (M10.5 slice 19):** the exports' and the command line's
  signature ask reuse `sig_impact`, so:
  - a signature already invalid when the file was opened still asks:
    `signatures/safedocs/0008830.pdf` and `0010094.pdf`, which pyHanko
    finds neither intact nor valid, exit 3 on a mark add (37
    unencrypted files in `signatures/` have such a signature);
  - a file written with `sheaf … --signatures break` asks again on its
    next edit (`append_anyway: true`, "… removed since the last save").

### B9: A radio group keeps its value when its chosen button's page is removed

- **Found:** 2026-10-22, M10.5 slice 19's export corpus tier
  (pre-existing: `DeletePages`')
- **Where:** `DeletePages`' lowering (`crates/sheaf-doc/src/tx/`), and so
  an extract's `KeepPages`
- **Severity:** wrong output
- **What happens:** delete the page holding a radio group's chosen
  button (or extract the other pages) and the group keeps its `/V`,
  which no button left can show. `forms::check` then reports
  `NoAppearance`, and `StaleSigFlags` on the delete.
- **Expected:** what's left is consistent: the group's `/V` is a state
  one of its remaining buttons has, or `/Off`; the form check finds
  nothing new.
- **Repro:** `forms/safedocs/0070616.pdf`, field `Gruppieren4`: the
  export tier's `export_round_trips` asserts this as its one known
  failure.
- **Notes:** the same question for a check box group and for a
  choice's `/Opt`-bound value; worth checking what Acrobat does to `/V`
  on Delete Pages.

### B10: After a Combine, the "XFA part holds values" bar can stay up

- **Found:** 2026-10-22, M10.5 slice 19 (step 7, Combine's hand-off)
- **Where:** the app's Combine hand-off (`crates/sheaf-app/`), the
  adopted window's form facts
- **Severity:** annoyance
- **What happens:** Combine is one transaction on the first file's
  session, handed to a window as made, answered with `PRINT_ANSWERS`
  (which drop an XFA part's values). The adopted window keeps the first
  file's form facts as opened, and `xfa_values_dropped` isn't set. So a
  hybrid first file whose XFA held values can still show the bar after
  the combine dropped them.
- **Expected:** the bar reflects the combined document: gone, or saying
  the values were dropped.
- **Repro:** Combine with a hybrid XFA form holding values as the first
  file.
- **Notes:** none found in the corpus runs; read from the code.

### B11: Create from Scans still writes a file and reopens it

- **Found:** 2026-10-22, M10.5 slice 19
- **Where:** `Workspace::create_from_scans`
  (`crates/sheaf-app/src/workspace.rs`), `open_paths`
- **Severity:** annoyance
- **What happens:** scan builds a Session now, but the app writes its
  result to a file and opens that file, where Combine hands its
  Document to a window as made.
- **Expected:** the scan's Session handed to a window, as Combine does:
  no file until the user saves.
- **Repro:** Create from Scans in the app.
- **Notes:** M5's scanner intake (ROADMAP) reworks this path; do it
  there.

(B14–B18 are on the M5 branch, `m5-intake`, found by M5 slice 1.)

### B19: A bookmark selection isn't tracked, so a re-lowered edit can land on other pages

- **Found:** 2026-10-23, M10.5 slice 20
- **Where:** `PageSel::Bookmark`'s resolve (`crates/sheaf-doc/src/tx/`),
  which records only `Key::Order`, not the outline `bookmark_span` reads
- **Severity:** wrong output
- **What happens:** an edit that selects pages by a bookmark, re-lowered
  (`Stale::Relower`) after another edit moved that bookmark, resolves to
  the bookmark's new span with no conflict. Only a transaction that was
  handed out conflicts, and its Conflict names no edit. Decision reads
  of a bookmark selection have the same gap.
- **Expected:** a move of the bookmark (or of the next one, which ends
  its span) between the read and the commit conflicts, as other stale
  reads do.
- **Repro:** a plugin's edit on `{"bookmark": "Schedule C"}` while the
  user drags that bookmark to another page; the call ends and
  re-lowers.
- **Notes:** record the outline keys the span reads (the bookmark and
  its next sibling at each level).

### B20: A plugin's region events wait forever after a window closes mid-call

- **Found:** 2026-10-23, M10.5 slice 20 (pre-existing)
- **Where:** the app's plugin call queue (`crates/sheaf-app/src/plugins.rs`),
  the `regions` key
- **Severity:** annoyance (a stuck plugin)
- **What happens:** if a region call is in flight when its window
  closes, `ws.update_in` fails and the plugin's `regions` key stays
  busy, so its later region events in every window never run. Changes
  are keyed per view and aren't affected.
- **Expected:** a closed window ends its calls and frees the key.
- **Repro:** a plugin that handles regions slowly; select a region,
  close the window before it returns, then select a region in another
  window.

### B30: Images to PDF and scan outputs have no /Producer or /CreationDate

- **Found:** 2026-10-23, the backlog-fixes PR (beside B8)
- **Where:** sheaf-ops `images.rs` and `scan.rs`, whose sessions start
  from `Session::new_empty`; File > Create > From Images and From Scans,
  `sheaf images-to-pdf` and `sheaf scan`
- **Severity:** annoyance
- **What happens:** the output takes its `/Info` from its empty primary,
  which has none, so it has no `/Producer` or `/CreationDate`. Combine
  keeps its first file's `/Info`, which is right.
- **Expected:** `/Producer` sheaf and `/CreationDate` when it was made, as
  the comment summary has again (B8).
- **Repro:** `sheaf images-to-pdf a.png -o out.pdf`, then
  `qpdf --show-object=trailer out.pdf`: `<< /Root 3 0 R /Size 7 >>`.
- **Notes:** M5's intake rework (`m5-intake`) owns scan; leave scan.rs to
  it. B8's `Session::new_titled`, an empty primary with an `/Info`, is
  the likely fix. (B24–B29 are on another branch.)

### B23: A fuzz test's wall-clock budget fails when the machine is busy

- **Found:** 2026-10-23, the instance-handoff migration's workspace run
  (pre-existing)
- **Where:** `source::fuzz::tests::fuzz_bodies_survive_mutated_fixtures`
  (`crates/sheaf-doc/src/source/fuzz.rs:1394`), `took < 60 s`
- **Severity:** annoyance (a flaky test)
- **What happens:** at load average ~60 it took 70.9 s and failed; run
  alone it passes in 6.25 s. A slow CI runner could trip it too.
- **Expected:** the test bounds the work, not the wall clock: a cap on
  steps or bytes per mutated body (what it really guards against is
  unbounded work), or CPU time rather than elapsed time.
- **Notes:** the 7k gates measure time on a quiet machine on purpose;
  a unit test in the default suite shouldn't depend on load.

## Features

### F1: Basic calculator

- **Found:** 2026-10-21, Greg, from a Gemini summary of a PDFlyer video,
  during M10.5
- **What:** PDFlyer's second calculator. Entry is the usual `a + b =`
  rather than an adding machine's amount-then-operator, and `=` totals
  the section. Otherwise it's the same tape: sections, a title row,
  notes, memory, and To PDF. PDFlyer's Settings choose which calculators
  appear.
- **Why:** Not everyone touch-types 10-key. The report lists the two
  modes as a core feature, but matching PDFlyer isn't a reason on its
  own (Greg, 2026-10-21); this needs users who want `a + b =` entry.
- **Overlap:** M9 slice 13 left it out on purpose: "PDFlyer's separate
  Basic calculator (the `=` key) is left out unless asked for"
  (`todos/m9.md`). If [F4](#f4-the-calculator-as-a-popup-not-a-panel)
  goes ahead, decide this with it.
- **Notes:** probably an entry mode on the same panel and tape model, not
  a second panel. Source:
  [UsingBasicCalculator](https://www.scanautoflow.com/ophelp/v22/en/Scan_WkStn/Content/PDFlyer/UsingBasicCalculator.htm);
  `docs/prior-art/pdflyer.md` §2.3.

### F2: Percent key on the 10-key

- **Found:** 2026-10-21, Claude, while checking the same report
- **What:** a `%` key, as PDFlyer has ("calculates the percentage of
  the previously entered number"). sheaf's tape has add, subtract,
  multiply and divide only (`sheaf_edit::tape::Op`).
- **Why:** rates and allocations are common on workpapers.
- **Overlap:** M9 slice 13 took the other keys from PDFlyer; `%` isn't
  listed there, as taken or as left out.
- **Notes:** the behavior needs pinning down. CCH's own example
  (500 × 10% = 25000) is wrong, and adding machines differ. A new op
  is a new `Op` variant (in the ops JSON schema too) and a text form for
  `/Contents`, so other viewers show it. Tapes saved by earlier builds
  needn't read: sheaf is pre-release.

### F3: Put a calculator result on the page as text

- **Found:** 2026-10-21, Greg, from a Gemini summary of a PDFlyer video,
  during M10.5
- **What:** put a total or subtotal on the page as an editable text mark,
  or copy it to the clipboard, instead of placing the whole tape.
- **Why:** The report says PDFlyer does this ("results and subtotals can
  be pasted directly onto the workpaper as editable text"). CCH's help
  doesn't mention it; its calculators' only output is To PDF. Treat it
  as a sheaf extension, not parity.
- **Overlap:** M9 slice 14 (pick numbers from the page) is the reverse
  direction. The `FreeText` mark kind exists.
- **Notes:** a tape row's context menu ("Copy amount", "Place as text")
  would cover both.

### F4: The calculator as a popup, not a panel

- **Found:** 2026-10-21, Greg, during M10.5
- **What:** opening the calculator brings up a popup that takes focus.
  The user works the tape, then To PDF places it on the page as an
  annotation and the popup closes. Edit Tape on a placed tape opens the
  popup again with the tape loaded, and To PDF updates it where it is.
  This is PDFlyer's flow.
- **Why:** a calculation is an action the user takes. It should have
  focus while they do it and go away when they're done, with the result
  saved where they put it. Today's docked panel (`Panel::Calc`, opened
  by `workspace::ShowCalc`) stays on screen after the tape is placed,
  and looks and feels odd.
- **Overlap:**
  - M9 slices 13–14 built the panel: the tape format, Edit Tape, Place
    on the next page clicked, and Pick. The tape model
    (`sheaf_edit::tape`, the tape mark kind) stays; this changes the UI
    around it.
  - Plugins can open it: `show-panel` takes `"calc"` (`wit/sheaf.wit`;
    `BuiltinPanel::Calc` in `sheaf-plugin`). The call is fire-and-forget
    ("bring it into view"), which fits a popup as well as a panel, so
    the WIT needn't change. The one exception is its promise that it
    "doesn't take keyboard focus": either a plugin-opened calculator
    opens unfocused, or that line gets an exception for `"calc"`.
- **Open questions:**
  - **Pick and focus.** Pick (click a number on the page, or Shift+drag
    over a region) needs the page clickable while the calculator is
    open. So the popup can't be modal: it keeps keyboard focus while
    page clicks still reach the canvas.
  - **Where the tape lands.** PDFlyer puts it at the upper-left of the
    active page, and you drag it from there. sheaf's Place waits for a
    click on a page, which puts it in the right spot in one step. One
    option: To PDF closes the popup and the next click places the tape.
  - **Dismissing.** What Esc does to a tape that hasn't been placed:
    discard it, or keep it for next time (PDFlyer keeps the tape until
    New Tape). Today Esc clears the entry, then goes back to the
    document.
  - **Window or overlay.** A separate window can move to another
    monitor. A floating card inside the main window is simpler.
    `sheaf_ui::dialog` has only modal parts.
  - **Keys.** Choose them on their own merits. The differences from
    PDFlyer are in [B1](#b1-calculator-keys-dont-match-pdflyers), for
    reference.

### F5: Plugins ask for a command, not a panel; sheaf picks the view

- **Found:** 2026-10-21, Greg, discussing F4, during M10.5
- **What:** one host call that means "run this command; the user may
  want to set its parameters". It takes a command id, whatever arguments
  the caller has, and whether the user should see them before the
  command runs. sheaf decides what the user sees: a form generated from
  the command's schema, the command's own dialog, or a tool UI like the
  calculator popup ([F4](#f4-the-calculator-as-a-popup-not-a-panel)).
  It returns done or cancelled.
- **Why:**
  - It separates *what* from *how it looks*. `show-panel("calc")` names
    a view, but whether the calculator is a panel or a popup is sheaf's
    business, not the plugin's.
  - One path for every caller. Agents (M16), batch actions (M15), key
    bindings (`args-json`), the CLI and plugins all need commands with
    arguments, and one schema serves them all.
  - With a schema, sheaf can generate a form for any command, built-in
    or plugin, that has no dialog of its own.
- **Overlap:**
  - M16 and `todos/agent.md` ("Running commands live") plan most of
    this, for agents: a command with an argument schema runs without its
    dialog, and one without opens its dialog pre-filled and returns done
    or cancelled. The schemas are shared with M15. F5 extends that to
    plugins, and lets the caller ask for the user to review even when
    the arguments are complete.
  - The WIT today has `show-panel` (brings a view into view, without
    focus), `show-form` (shows the plugin's own form and waits for the
    answers), and the export `run-command` (sheaf calls a plugin
    command with `args-json`, which has no schema). A plugin has no
    import for running a command of sheaf's or another plugin's.
  - Not in M10.5: plugin API 0.4 (slice 20) doesn't touch these calls,
    and adding the import later only adds to the WIT.
- **Open questions:**
  - **Views stay.** Not every `show-panel` call is a tool:
    `currency-convert` calls `show-panel("comments")` to show its
    results. Panels that are views (pages, bookmarks, comments, a
    plugin's own panels) keep `show-panel`. Tools that end in an edit
    (the calculator, Set Status, Bates, OCR) go through the new call.
    The calculator is a tool that was filed as a panel.
  - **Example:** the `subtotal` plugin shows a region's sum in the
    status bar. With this call it could open the calculator pre-filled
    with those amounts, for the user to check and place.
  - **Permissions: permissive for now** (Greg, 2026-10-21: "while we are
    in pre-release I'm inclined to be permissive. we want to have low
    friction"). Any plugin may run any command, with or without the
    user seeing its arguments, and there's no per-command allowlist.
    Revisit before the first release:
    - which commands may run unprompted (M16 asks the same for agents:
      "commands an agent should never run");
    - whether running a command bypasses the manifest permissions some
      ops need (crop, watermark, flatten; M16 "Decide at kickoff").
  - **Blocking.** `show-form` holds the plugin's call until the user
    answers, outside its time limit, but a calculator session can be
    long. Keep that model, or return at once and report the result
    through an event?
  - **Return value:** done or cancelled, the version the command made,
    and perhaps the arguments the user ended with.
  - **Precedent:** Emacs's `interactive`, where a command says how to
    ask for its arguments, `call-interactively` asks, and a direct call
    doesn't. Also VS Code's `executeCommand(id, ...args)`.

### F6: Make Go to Page easy to find

- **Found:** 2026-10-21, Greg, during M10.5, who asked for "goto page#"
  as useful in large documents
- **What:** Go to Page exists (M7, `crates/sheaf-app/src/goto.rs`). It
  takes a page number or a label ("iv", "A-3"). But the only ways in
  are View > Page Navigation > Go to Page, Ctrl+Shift+N (Acrobat's key)
  and the command list. Make it easy to find:
  - **Click the page indicator.** The status bar's "page 4 / 120" is
    plain text inside one string (`DocView`'s status line). Clicking it,
    or making it an editable box, should open Go to Page, as page boxes
    do in Acrobat, Foxit, pdf.js and Evince.
  - **Bind Ctrl+G as well.** `docs/prior-art/README.md` recommended
    binding both, because Foxit users expect Ctrl+G and F3 already finds
    the next match. Its note that "sheaf has no go-to-page command yet"
    is out of date.
- **Why:** jumping to a page is a basic move in a large binder, and a
  command found only through a submenu and an Acrobat key is easy to
  miss. Greg didn't know it existed.
- **Overlap:** M7 built the command; `todos/toolbar.md` lists it among
  the View items. Nothing plans a page box.
- **Notes:** small. Check how long a label like "A-3 (57 of 1,240)"
  can get before it crowds the status bar.

### F7: Scrollbars on everything that scrolls

- **Found:** 2026-10-21, Greg, during M10.5
- **What:** a visible scrollbar on every area that scrolls, starting
  with the page view's panes. Each pane gets its own bars, vertical and,
  when zoomed wider than the view, horizontal. Drag the thumb to move;
  click the track to page.
- **Why:**
  - In a large document the thumb shows roughly where you are at a
    glance, without reading "page X of N".
  - With the view split, each pane can be at a different point in the
    document, and only one pane's position is in the status bar.
  - Today the wheel and keys are the only way to scroll. A thumb gives a
    coarse jump that the wheel can't.
- **Overlap:** nothing plans scrollbars. Neither sheaf nor GPUI draws
  one: GPUI only reserves space (`scrollbar_width`) and reports the
  platform's auto-hide setting (`App::should_auto_hide_scrollbars`). Go
  to Page ([F6](#f6-make-go-to-page-easy-to-find)) is the precise
  jump; this is the at-a-glance one.
- **Notes:**
  - **One component** in `sheaf-ui`, used by all of them: about 19 files
    have an `overflow_*_scroll` area (the page panes, the side panels,
    the palette, the dialogs' lists, the calculator tape). Virtualized
    lists know their full height already (`sheaf_ui::list::ListLayout`),
    and so do the page panes (their layout's `total_h` and `view_h`).
  - **Source:** the scrollbar itself is `gpui-base`'s `scrollbar.rs`
    (0.7.3, Apache-2.0): about 1,850 lines plus 1,040 of tests.
    `gpui-component`'s `scroll/scrollable.rs` only wraps it in a
    `.vertical_scrollbar(&handle)` extension. Worth reading:
    - its `ScrollbarHandle` trait, implemented for `ScrollHandle`,
      `UniformListScrollHandle` and `ListState`;
    - its three modes: show while scrolling, on hover, or always;
    - its thumb geometry.

    It's tied to kit's theme and animation modules, so using it as the
    reference for a smaller one of our own probably beats copying it.
    `todos/agent.md` cleared the kit crates for copying with
    attribution, though not as a dependency. Zed draws its own in its
    `ui` crate; check that crate's license before reading it for code,
    since the Zed crates `agent.md` checked are GPL-3.0.
  - **One page at a time.** In the single-page and single-spread modes,
    a pane only scrolls within the current spread, so a plain scrollbar
    would show position within the page. Acrobat's single-page bar spans
    the whole document and steps by page; consider the same.
  - **While dragging,** show the page it would land on ("iv (4 of
    120)"), as Acrobat and Preview do.
  - **Large files open lazily.** The page count can grow while a file
    is read ("120+ (searching…)"), so the thumb shrinks as it reads. It
    shouldn't jump under the user's drag.
  - **Overlay or always shown:** follow the platform's auto-hide setting.
  - **Later, maybe:** marks on the track for search hits, bookmarks or
    comments, like an editor's overview ruler.

### F9: Test against pdf-corpus's signatures/ and conformance/ collections

- **Found:** 2026-10-22, Greg, during M10.5 slice 18
- **What:** corpus tiers over `../pdf-corpus/signatures/` (531 files,
  checked with pyHanko: `intact`, `valid`, changed after signing,
  digest mismatches, several subfilters) and `conformance/` (3,524:
  PDF/A, PDF/UA and PDF/X claims, some damaged). Slice 18 added
  `Corpus::scans()` and tiers over `scans/`; these two aren't read yet.
- **Why:**
  - `signatures/`: Append must keep signatures that hold, and the
    signature asks must be right (B5). Today's signed-file tests use
    the 322 `/DocMDP` forms.
  - `conformance/`: the PDF/A save Note (M10.5 slice 16), PDF/A
    convert and validate (M14), PDF/UA (M17).
- **Overlap:** M10.5 slice 19's exports and signature asks; M12
  (digital signatures); M14; M17.
- **Notes:** each needs a `Corpus::` reader for its manifest, like
  `scans()`, and a tier with its own `SHEAF_…_ONLY` narrowing.
  - **Progress (M10.5 slice 19):** `Corpus::signatures()` reads
    `signatures/`'s manifest; sheaf-cli's `tests/edit.rs` uses it (an
    `--append` onto signed files keeps their signatures). No tier sweeps
    the collection yet, and `conformance/` is still unread.

### F13: In-house what sheaf uses of lopdf and drop the dependency

- **Found:** 2026-10-22, Greg and Claude, during M10.5 slice 19
- **What:** a long-term direction, not scheduled, and never mid-milestone
  (Greg, 2026-10-22). Bring what sheaf uses of lopdf into sheaf's own
  crates and drop the dependency, while still sending upstream any fix
  found along the way. It isn't either/or.
  - **Already sheaf's own:** the writer (`serialize/`), the content-stream
    code (`content/`) and the lazy-reading layer (`source/`).
  - **Still lopdf's:**
    - the object model;
    - the object parser and cross-reference reading;
    - object streams;
    - stream decompression;
    - encryption, which sheaf would keep on the RustCrypto crates and
      not hand-roll;
    - the eager `Document`: the Rewrite graph, PDFs built in memory,
      `prune_objects`;
    - its writer, used only by `annot::tape_pdf`, tests and fixtures.
  - About 9k of lopdf's 23.5k lines are off sheaf's production paths:
    text encodings and glyph tables, CMap parsing, outlines, bookmarks
    and TOC, fonts, dates, XObject helpers, IncrementalDocument.
- **Why:** sheaf carries a vendored, patched lopdf whose patches reshape
  its reader, encryption and object model. The vendored crate is outside
  the workspace, so sheaf's clippy `-D warnings` and lint bans don't run
  on it, and each new upstream commit means reapplying the patch.
- **A first step:** move `LazyPdf` (`vendor/lopdf/src/lazy.rs`, 989
  lines) into sheaf-doc's `source/`.
  - The byte sources and caps (`ByteSource`, `FileBytes`, `ReadLimits`,
    `parse_from`) stay in lopdf, whose shared `XrefReader` reads through
    them.
  - lopdf then makes public what LazyPdf reads today as crate-private:
    from reader.rs, `XrefReader`, `Resolved`, `decryption`, `encrypts`,
    `header_offset`, `length_value`, `normal_offsets`, `object_bound`;
    from the parser, `Resolve`, `header`, `indirect_object_in`,
    `stream_end`.
  - Keep `source::lazy_tests` identical across the move: every object of
    the corpus's readable forms, read lazily, equals the eager load.
- **Upstream meanwhile** (Greg: the weekend of 2026-10-24/25, each pull
  request confirmed first): `vendor/lopdf/SHEAF.md` lists fixes that
  stand on their own:
  - each xref stream read once;
  - lenient `/XRefStm` past EOF;
  - white space around `startxref`;
  - `/Encrypt null`;
  - strings in stream dictionaries (lopdf #603);
  - `/EFF`;
  - V4+ filter names and `/Identity`;
  - the set in `traverse_objects` and `prune_objects`.
- **Overlap:** `vendor/lopdf/SHEAF.md`, "Going back to crates.io";
  F15 (a Rewrite that doesn't load the file whole) would remove the
  largest use of the eager `Document`.
- **Notes:** to size at triage: the object model's reach (about 400
  import sites), and the parser's edge cases, which lopdf's parser
  fixes and the corpus comparisons encode (`source::lazy_tests`; the
  corpus runs).
- **Maybe published** (Greg asked, 2026-10-22): the in-housed lopdf parts,
  `LazyPdf`, sheaf-doc's writer (`serialize/`), its `source/` and
  `geom`, and the generic half of `content/` (lex, writer, splice) could
  become one layer crate, and later perhaps a published alternative to
  lopdf, as `in-progress` and `instance-handoff` were extracted.
  - **Production coupling today:**
    - `serialize` reaches `materialize` (`ObjKey`) and `source`;
    - `source` reaches `content`, `geom` and `store` (`BlobId`);
    - `content` reaches `annot` and `private`, which are sheaf's blocks
      and markers.
  - Make it an internal crate with a clean boundary first; decide on
    publishing after an ecosystem survey.
- **Deferred** (Greg, 2026-10-22): revisit after more milestones, since
  what sheaf needs from this layer may change as later scopes dig in.
  Upstream fixes go ahead meanwhile.

### F14: An automated test of `sheaf scan --ocr`

- **Found:** 2026-10-22, M10.5 slice 19 (step 5, scan as a Session)
- **What:** a CLI test that runs `sheaf scan --ocr` on a few images and
  checks the output's text layer: the words found, one OCR block a page,
  one font.
- **Why:** slice 19 moved scan onto a Session (an empty session plus an
  insert of the images, then OCR). Only a hand run checks the `--ocr`
  path; the OCR op itself is tested in sheaf-doc and the corpus tiers.
- **Overlap:** M5's scanner intake (ROADMAP), which reworks scan.
- **Notes:** the worry was that Tesseract may be missing in CI. Check
  what `--ocr` needs (sheaf-ocr's default engine vs the `tesseract`
  feature, used only for orientation) and either run it everywhere or
  install what it needs in CI. Don't make it skip silently.

### F15: A Rewrite that doesn't load the whole file (the flatten export's memory gate)

- **Found:** 2026-10-22, M10.5 slice 19 (the flatten gate)
- **What:** a Rewrite that copies the primary's objects without loading
  the file whole: a streaming copy, its pruning and `keep_only`
  included.
- **Why:** the plan's gate for Export Flattened on the heavy 7k test
  file (peak ≤ 1.5× the file + 200 MB, 220.5 MB, in the flatten's own
  process) can't be met by construction. Opening the heavy alone peaks
  at 221 MB, and the export peaks at 513–515 MB, most of it the
  Rewrite's eager load of the primary (`materialize::graph::eager`; a
  plain Rewrite adds 252 MB). The gate is measured on every run and
  asserted only with `SHEAF_GATE_FLATTEN`, like proposal 8's accepted
  misses. Every Rewrite-based save and export of a large file pays this.
- **Overlap:** M10.5 proposal 37 (a symbolic `keep_only`, unbudgeted);
  F13 (the eager `Document` is lopdf's); M14 (object-stream packing).
- **Notes:** the gate may also want restating as what the export adds
  over the open (293 MB today against 220.5) rather than the process's
  peak; decide with the design. Slice 19's notes have the five runs.

### F10: Redo OCR: replace another tool's text layer

- **Found:** 2026-10-22, Greg, comparing sheaf's OCR with Acrobat's and
  ocrmypdf's, during M10.5
- **What:** a third choice in Recognize Text (OCR)…, beside skipping
  pages with text and recognizing every page: redo. On each page, take
  out the invisible text another tool's OCR left (text render mode 3:
  Acrobat's Searchable Image, ocrmypdf, ABBYY, a scanner's own), keep
  the visible text, and write sheaf's layer in its place.
- **Why:**
  - With skip off, sheaf's layer goes on top of the old one, so the page
    draws its words twice, invisibly: Find hits each word twice, and
    selection, copy and plugins' page text are doubled. A plugin's
    `recognize` always runs with skip off
    (`crates/sheaf-app/src/plugins.rs:898`).
  - With skip on (the default), a page whose layer came from a weak
    engine or a scanner can't be improved. Acrobat's only way is to
    rasterize the page, which loses its real text.
  - A page under `HAS_TEXT_MIN_CHARS` (20, `crates/sheaf-ocr/src/job.rs`)
    is recognized even with skip on, so a short foreign layer or a saved
    Bates number comes out twice.
- **Overlap:**
  - [F8](#f8-a-command-to-remove-recognized-text) removes sheaf's own
    layer on its own; redo is that plus recognizing again, for any
    tool's layer. [B7](#b7-clear-ocr-text-silently-does-nothing-on-a-damaged-old-layer)
    is the case where even sheaf's own old layer can't be taken out.
  - Sheaf already replaces its own layer: `text_layer::install` strips
    streams marked `/SheafOcrLayer` before writing
    (`crates/sheaf-doc/src/text_layer.rs`), and slice 18's OCR blocks
    (`/SheafOcrBlock`, `todos/m10.5-edit-api.md` decision 26) keep that a
    dict edit. This entry is the foreign case, which has no marker, so
    the page's own streams must be decoded and rewritten. Sheaf never
    does that today.
  - M11's true content removal and Remove Hidden Information
    (`todos/m11.md`) take text out of a page's streams. Redo needs the
    same operation for invisible text alone, so it probably follows M11
    or shares its machinery.
- **Notes:**
  - **Prior art:** ocrmypdf's `--redo-ocr` (now `--mode redo`) strips
    invisible text and recognizes again, keeping visible text;
    `--force-ocr` rasterizes instead, and `--skip-text` is sheaf's
    default. Acrobat refuses a page with "renderable text" (hidden text
    counts) and has no redo. ABBYY has a setting to force OCR. Foxit's
    behavior is unconfirmed.
  - **Which invisible text is OCR:** not all of it is. Authors use
    render mode 3 for search-only or copy-only text too. Strip every
    mode-3 run, or only those over a page-sized image?
  - **Pages with visible text:** the page's render shows that text, so
    OCR reads it again. Either recognize only outside PDFium's
    character boxes, or drop recognized words that overlap them.
    Check how ocrmypdf's redo handles this.

### F11: Plugin memory and time limits in settings.toml

- **Found:** 2026-10-22, Greg, discussing a wasm build of qpdf as a
  repair plugin, during M10.5
- **What:** the plugin limits, today fixed at 256 MiB of linear memory
  and 30 s a call (`Limits::default()`,
  `crates/sheaf-plugin/src/runtime.rs:37`), set in `settings.toml` for
  every plugin, and possibly overridden plugin by plugin.
- **Why:**
  - A plugin that works on a whole file, such as a qpdf repair or
    another document transform, needs more than 256 MiB for a large
    binder, and more than 30 s.
  - A user on a small machine may want them lower.
  - Today, changing them means rebuilding sheaf.
- **Overlap:**
  - `todos/plugins/document-transforms.md`, Open: may a transform
    declare raised limits (up to about 3.5 GiB and 15 minutes), shown
    at consent? That is the plugin asking. This entry is the user
    setting them, and the two need one rule for which wins.
  - `docs/plugins.md`, Limits, documents today's fixed values.
- **Notes:**
  - **Where per-plugin values go:** not in `[plugins.<id>]`. That
    section is the plugin's own: it's handed to the plugin as JSON
    (`Prefs::plugin_json`), and keys its manifest doesn't name are
    reported as typos. A separate table such as `[plugin-limits]` with
    `[plugin-limits.<id>]` keeps the host's settings out of the
    plugin's reach.
  - **Plumbing:** the runtime is made once with one `Limits`
    (`crates/sheaf-app/src/plugins.rs:358`), but each plugin has its own
    store (`Instance::start`, `runtime.rs`), so a per-plugin value can
    come in through `LoadOptions`.
  - **Changes while running:** settings are re-read when the file
    changes. A timeout can apply from the next call; a memory limit is
    set when the store is made, so it applies when the plugin is
    reloaded.
  - **Bounds:** wasm32 memory can't pass 4 GiB. A value out of range is
    clamped and reported, as other settings problems are, never fatal.
  - The timeout already leaves out time spent waiting on the user and
    on OCR. Keep that.

### F12: A qpdf plugin

- **Found:** 2026-10-22, Greg, discussing PDF repair, during M10.5
- **What:** qpdf built for wasm as a plugin of its own, kept in its own
  repository and installed as an archive (Plugins > Install, through
  `install_archive`), not bundled with sheaf. Commands such as:
  - check a file (`--check`), as a second opinion beside sheaf's reader;
  - linearize, and pack object streams, before M14 does them natively;
  - rewrite a damaged file, as Save As does today, by qpdf's rules;
  - dump the file's objects (`--qdf`, `--json`) for debugging.
- **Why:** it brings qpdf's checks and writer options to sheaf users
  now. It would also be the first plugin built outside this
  repository, and the first real user of the document-transform API:
  if a qpdf plugin works end to end, that API is probably right.
- **Overlap:**
  - `todos/plugins/document-transforms.md`, whose "What can't work"
    says a tool like qpdf needs a wasm build. It also records the `gc`
    requirement below.
  - M14 Optimize (`ROADMAP.md`): linearization and object streams,
    which make the plugin's versions a bridge, not the long-term path.
    M14's Fast Web View entry rules out a transform plugin as the way
    sheaf linearizes, since a plugin parses and writes the whole file
    again; that cost is acceptable for a stopgap.
  - [F11](#f11-plugin-memory-and-time-limits-in-settingstoml).
- **Notes:**
  - **Needs, in order:**
    1. wasmtime's `gc` feature in sheaf's build. qpdf uses C++
       exceptions throughout, and wasm exception handling needs `gc` in
       wasmtime 49. Without it the component is rejected at load.
    2. The document's bytes. Today's API has no way to hand a plugin
       the whole file; the document-transform API would add it. Until
       then, a file field in a plugin form returns a path, and the
       plugin can open it only inside the folders its manifest's
       read/write permissions grant (preopened in
       `crates/sheaf-plugin/src/runtime.rs`). So it can work on files
       on disk, not on the open document.
    3. Higher memory and time limits (F11): 256 MiB and 30 s won't hold
       a large file.
  - **Why not in this repository:** the build needs a C++ wasm
    toolchain (wasi-sdk, libc++ built with exceptions, and zlib and
    libjpeg built for wasm). Sheaf's CI and `package.sh` build only
    Rust plugins.
  - **License:** qpdf is Apache-2.0, so the plugin ships qpdf's NOTICE.

### F16: Turn QR codes into links

- **Found:** 2026-10-23, Greg, during M10.5
- **What:** a plugin command that finds the QR codes on a document's
  pages, decodes them, and puts a link over each one that holds a web
  address, so clicking the code opens what scanning it would. Before
  adding anything, it lists what it found (the page, the decoded text,
  and whether it will become a link) for the user to choose from.
  - `http`, `https` and `mailto` addresses become links. Other schemes
    (`javascript:`, `file:`, app links) are listed and skipped.
  - Other content (plain text, a contact card, Wi-Fi details, a payment
    code) doesn't become a link. The plugin can offer to put it in a
    note mark beside the code instead.
- **Why:** QR codes on invoices, statements, letters and tax notices
  lead to payment portals and sign-in pages. On screen, the only way to
  follow one is to point a phone at the monitor. A link helps
  screen-reader users too, who can't scan a code at all. And seeing the
  decoded address before it's linked shows a preparer where a code
  really goes before trusting it.
- **Overlap:**
  - M13 link editing (`ROADMAP.md`), whose "create links from URLs in
    text" is this feature's text-based sibling.
  - Pending's "Follow links in the page view". Sheaf doesn't follow
    links yet, so until it does, these work only in other viewers.
  - M5's separator sheets read barcodes. `todos/m5-intake.md` ("Reading
    and writing codes", on the `m5-intake` branch) benchmarked rxing and
    zxing-cpp; both read QR codes with 0.34 mm modules in every
    1-bit case tested at 200 and 150 dpi.
- **Notes:**
  - **How:** render each page in gray through the plugin API's `render`
    interface, decode QR codes only with rxing (pure Rust, Apache-2.0,
    builds for wasm), and turn each code's corners into the link's
    rectangle with `image.to-page`. Read QR only: in M5's bench,
    reading every format over a whole page found false codes on about
    22 of 150 real scans. Check the false-read rate with QR alone.
  - **Needs, in order:**
    1. An op that creates a link. The edit vocabulary has none: plugins
       add marks and connectors, and a link is only something they may
       not delete. A plugin-made link that opens an address may need the
       user's approval (`approval`), or the plugin's own confirmation
       list may be enough. Decide when the op is designed, probably
       with M13's link editing.
    2. Seeing the links already there. `annotations()` never lists a
       link, so a second run can't tell which codes are already linked
       and would stack duplicates. Either the document interface lists
       links (rectangle and target), or the op skips a link with the
       same target over the same area.
  - **Where the decoder lives:** in the plugin, or, once M5 puts a
    reader in sheaf, in a host `barcodes` interface beside `ocr`, which
    saves every plugin from bundling rxing.
  - **Home:** an in-repo plugin beside `subtotal` and `amount-crossref`.
    rxing is pure Rust, so the build needs nothing new.
  - **Size:** the plugin is small; the link op is the real work.
  - **When:** revisit after M13 lands (Greg, 2026-10-23).

## Closed

### B1: Calculator keys don't match PDFlyer's

- **Closed:** 2026-10-21. Not a bug: sheaf doesn't copy PDFlyer just
  for the sake of it (Greg). Keys are chosen on their merits, with
  [F4](#f4-the-calculator-as-a-popup-not-a-panel).
- **Found:** 2026-10-21, Claude, checking the Gemini summary against
  `docs/prior-art/pdflyer.md` §3
- **The differences**, from CCH's
  [Using10keyCalc](https://www.scanautoflow.com/ophelp/v22/en/Scan_WkStn/Content/PDFlyer/Using10keyCalc.htm)
  and the typed-key map in `crates/sheaf-app/src/calc_panel.rs`:

  | Key | PDFlyer | sheaf |
  |---|---|---|
  | `*` | multiply (the keypad's ×) | total (an adding machine's total key) |
  | G | MC | M+ |
  | H | MS | M− |
  | J | MR | MR |
  | K | M− | MC |
  | L | M+ | MS |
  | `T` | date and time into the note | nothing (Ctrl+T while editing a note) |

- **Notes:** M9 slice 13 chose the memory order without PDFlyer's table
  to hand (`todos/m9.md`).

### B6: sheaf-app's clippy fails without default features

- **Closed:** 2026-10-23. Fixed in the backlog-fixes PR:
  `Contributions::add_panel` is built only with the `plugins` feature,
  as only a plugin's manifest declares a panel. `cargo clippy -p
  sheaf-app --all-targets --no-default-features -- -D warnings` is clean;
  still no CI job builds it.
- **Found:** 2026-10-22, M10.5 slice 18 (pre-existing)
- **Where:** `crates/sheaf-app/src/contrib.rs:158`,
  `Contributions::add_panel`
- **Severity:** annoyance (build)
- **What happens:** `cargo clippy -p sheaf-app --all-targets
  --no-default-features -- -D warnings` fails: `add_panel` is dead
  without the `plugins` feature.
- **Expected:** clean, or the configuration isn't supported.
- **Repro:** the command above.
- **Notes:** no CI configuration builds it. Gate the fn on the feature,
  or decide the no-plugins build isn't supported and say so.

### B7: Clear OCR text silently does nothing on a damaged old layer

- **Closed:** 2026-10-23. Fixed in the backlog-fixes PR:
  `PageBlocks::legacy_ocr_stays` says a layer stream is outside the
  pair. On a page with nothing else to take out, a Clear (or a Set)
  adds no filter, changes nothing, and the commit says the layer stays
  (`Notice::OcrStays`, the save's Note's words); a page with text taken
  out keeps the save's Note, so neither says it twice. Tests:
  `a_lone_legacy_layer_without_its_q_stays_and_says_so`
  (materialize/ocr_tests.rs) and, through the app's command (F8),
  `removing_recognized_text_says_what_stays`.
- **Found:** 2026-10-22, M10.5 slice 18
- **Where:** `ClearOcrText`'s lowering (`crates/sheaf-doc/src/tx/`), the
  legacy `/SheafOcrLayer` selector (`StreamSel::SheafOcr`)
- **Severity:** annoyance
- **What happens:** an older sheaf's OCR layer is a `q` plus a
  `/SheafOcrLayer` stream. If another tool lost the `q` (no pair, no
  block), a Clear adds no filter: it changes nothing and says nothing.
- **Expected:** a Note that the old layer stays, as other kept artifacts
  get ("… stays on page 3: taking it out would change what else the
  page draws").
- **Repro:** a page whose only legacy OCR stream has no leading `q`;
  ClearOcrText; save.
- **Notes:** taking it out without the `q` isn't safe (`3 Tr` would
  leak), so keeping it is right; only the silence is wrong.

### B8: The comment summary PDF has no title, producer or creation date

- **Closed:** 2026-10-23. Fixed in the backlog-fixes PR: the summary's
  primary is `memory::titled`'s (`Session::new_titled`), a PDF of no
  pages whose `/Info` has the title, `/Producer` sheaf and
  `/CreationDate`, which a save keeps. No op sets document info: that's
  for the Properties editor's op (Pending) to design. Test: sheaf-cli's
  `the_summary_has_its_title_producer_and_date` (`-o`, both layouts).
- **Found:** 2026-10-22, Claude, during M10.5 slice 19 (step 5, the
  summary as a Session)
- **Where:** sheaf-ops `comments::summary`, the CLI's `sheaf comments
  summary` and the app's Comments panel
- **Severity:** annoyance
- **What happens:** the summary is now a Session made with `new_empty`
  plus an insert of the `Memory(Summary)` blob. A session's output takes
  its `/Info` from its primary, which is the empty document, and no op
  sets document info. So the saved summary has no `/Title`, `/Producer`
  or `/CreationDate`.
- **Expected:** as before slice 19: `/Title` is the summary's title,
  `/Producer` is sheaf, `/CreationDate` is when it was made. A viewer's
  title bar and Properties show them.
- **Repro:** `sheaf comments summary in.pdf -o out.pdf`, then
  `qpdf --show-object=trailer out.pdf` (or Properties): no `/Info`
  entries.
- **Notes:** Greg: "log it" (2026-10-22). The fix is an op (or a Session
  option) that sets document info, which a Properties editor would also
  want. Until then the summary could set it on its `new_empty` primary.

### B12: Flatten draws a comment's first appearance state when it has no `/AS`

- **Closed:** 2026-10-23. Fixed in the backlog-fixes PR: a non-widget
  annotation whose `/AP /N` has states draws what PDFium shows, the page
  view's engine: `/AS`'s state, else the one `/V` (or its parent's)
  names, else `/Off`, else nothing; never its first, and nothing made of
  its border or fill. A name or a string names a state. Widgets keep
  their rule (M10 bug 10). It's one choice (`source::appearance`'s
  `Shows::form`), so Export Flattened, print with markup, print preview
  and page images all follow it. pdf.js differs where the two disagree:
  it never takes `/V` or `/Off`, and for a Square, Circle, Ink, the text
  markups and FreeText with a border or a fill it draws an appearance of
  its own. Tests: `a_comment_with_states_draws_the_one_pdfium_shows`
  (materialize/flatten_tests.rs: states and no `/AS`, with a border, with
  a fill, an `/Off`, a `/V`, a parent's `/V`, a `/V` naming none, an
  `/AS` naming none, a string `/AS`), `flatten_takes_what_shows_and_leaves_the_rest`
  and `printable_leaves_what_isnt_printed` (their comment draws its
  `/Off`), sheaf-cli's `a_comment_with_states_exports_as_the_page_view_shows_it`,
  and sheaf-render's `the_page_view_shows_a_comment_s_value_s_state_or_its_off`,
  which pins what PDFium shows of the same file.
- **Found:** 2026-10-22, M10.5 slice 19 (step 2, `FlattenAnnotations`)
  (pre-existing: the old flatten did the same)
- **Where:** `FlattenAnnotations`' choice of appearance for a
  non-widget (`crates/sheaf-doc/src/tx/flatten.rs`)
- **Severity:** wrong output
- **What happens:** a comment (not a widget) whose `/AP /N` has states
  and no `/AS` is flattened with its first state. PDFium shows nothing
  for it (it falls back to `/V`, then `/Off`), so flattening draws what
  no viewer showed.
- **Expected:** flatten draws what the viewer showed, nothing here.
  Widgets already have no first-state fallback.
- **Repro:** a Square annotation with `/AP << /N << /A … /B … >> >>` and
  no `/AS`; Export Flattened.
- **Notes:** rare in the wild; check pdf.js and Acrobat agree with PDFium
  before changing it.
  - **Checked (2026-10-23, the backlog-fixes PR):** pdf.js and PDFium
    agree on nothing for the repro, but not on the cases beside it.
    Neither ever draws the first state.
    - **pdf.js** (6.4.299, `Annotation.setAppearance` in
      src/core/annotation.js): with `/N` a dictionary and no `/AS` name,
      no appearance at all, never `/V` or `/Off`. Square, Circle, Line,
      Polygon, Ink, the text markups and FreeText then draw one pdf.js
      makes from `/Rect`, `/C`, `/IC` and the border, if the border has
      width or there's a fill; a Stamp draws nothing.
    - **PDFium** (sheaf's, the page view's engine): `/AS`, else `/V` (the
      annotation's or its parent's), else `/Off`; nothing when that
      state isn't there, and no appearance of its own while `/AP /N` is.
    - **Hand-made files** (one Square, states A and B, no `/AS`): both
      draw nothing; with `/BS /W 4` or an `/IC`, PDFium nothing and
      pdf.js its own square; with states A and `/Off` instead, PDFium
      draws Off and pdf.js nothing (its own square with a border). A
      Circle is the Square's case; a Stamp draws nothing in both.
    - **Acrobat:** not found in its docs. ISO 32000-1 12.5.5 requires
      `/AS` when `/AP` has subdictionaries, so such a file isn't
      conforming.
    - **Not only Export Flattened:** print with markup, print preview
      and Export Pages' images draw through the same flatten
      (`FlattenSet::Printable`), so they show the first state where the
      page view shows nothing (`sheaf export` drew state A on the repro).
    - **The choice:** PDFium's, what the page view shows (the
      coordinator, under Greg's standing instruction to take the
      recommended answer), over drawing nothing as pdf.js does but for
      its own appearances.

### B13: A print file with markup renumbers its header when reopened in sheaf

- **Closed:** 2026-10-23. Fixed in the backlog-fixes PR: print with
  markup leaves sheaf's private data out as print without markup does
  (`RemovePrivateData`, `Private::Scrub`), so what sheaf drew is plain
  content and nothing in the file is live. Chosen over writing an
  `AsDocument` keep's generator blocks as plain content: one rule for
  both kinds of print, through what's already there, and a print file
  has no use for the rest of sheaf's private data either (its marks are
  drawn into its pages). Test:
  `a_print_file_opened_again_says_the_document_s_numbers`
  (materialize/header_tests.rs: pages 5–6 of 10, of a document saved
  and not, the file and a save of it reopened say "Page 5 of 10").
- **Found:** 2026-10-23, M10.5 slice 19's wrap-up (the print numbering
  fix)
- **Where:** `Export::print` with markup (`crates/sheaf-doc/src/export.rs`),
  which keeps sheaf's private data (`Private::Keep`)
- **Severity:** wrong output
- **What happens:** printing pages 5–6 of 10 with markup writes "Page 5
  of 10", but the file keeps the header live in `/SheafGenerators`. Open
  the print preview's Save as PDF output in sheaf and the header is
  numbered against that file's own pages, "Page 1 of 2", and a save of
  it writes that. Other viewers show "Page 5 of 10". Print without
  markup writes the header as plain content, so it stays.
- **Expected:** the file reads "Page 5 of 10" in sheaf too.
- **Repro:** a 10-page document with a "Page <<1>> of <<N>>" header;
  print preview of pages 5–6 with markup, Save as PDF; open it in sheaf.
- **Notes:** before the fix the print file said "Page 1 of 2"
  everywhere. Candidate fixes: print with markup leaves out sheaf's
  private data as print without markup does (Scrub), or an `AsDocument`
  keep writes its generators' blocks as plain content.

### B21: `layers::LayerId` is a lopdf `ObjectId` under another name

- **Closed:** 2026-10-23. Fixed in the backlog-fixes PR: `LayerId` is
  sheaf's own, a type over the group's object that only layers.rs reads
  (not an `ObjRef`: the facts are read from a `&Source`, with no `Src`
  to hold, and the view's override is keyed by source already).
  `tests/object_ids.rs` sees through a type alias that names `ObjectId`,
  or names such an alias, and catches a path through one; its probe
  `an_alias_of_an_object_id_is_seen_through` tests that. With the alias
  back, `only_a_source_s_readers_and_writers_name_an_object_id` fails
  on realize/mod.rs and realize/inputs.rs.
- **Found:** 2026-10-23, M10.5 slice 20
- **Where:** `crates/sheaf-doc/src/layers.rs` (`pub type LayerId =
  ObjectId`), used by `ViewInputs::off` and the app's layers panel
- **Severity:** annoyance (a design leak)
- **What happens:** an optional content layer is named by its file
  object id outside the `ObjectId` allowlist. `tests/object_ids.rs`
  can't see through the alias.
- **Expected:** a layer id of sheaf's own, as pages and marks have, or
  an `ObjRef`.
- **Notes:** also make the grep test catch type aliases of `ObjectId`.

### B22: `FlatOutlineItem` derives Serialize and Deserialize that nothing uses

- **Closed:** 2026-10-23. Fixed in the backlog-fixes PR: the derives
  and their `serde` attributes are gone; nothing read or wrote it.
- **Found:** 2026-10-23, M10.5 slice 20 (pre-existing)
- **Where:** `crates/sheaf-doc/src/open/outline.rs`
- **Severity:** annoyance (dead code)
- **What happens:** the derives have no serializer anywhere in the
  workspace, and they pin the type's shape for no reader.
- **Expected:** drop them, or say who reads it.

### F8: A command to remove recognized text

- **Closed:** 2026-10-23. Done in the backlog-fixes PR: Document >
  Remove Recognized Text… (`docview::RemoveRecognizedText`,
  `ocr_clear.rs`) asks whether the selected pages or every page, then
  runs `ClearOcrText` as a planner, one undo step, the notice its line,
  what stays (B7), or that there was none. The command line's form is
  `sheaf edit`'s `clear-ocr-text`, which README now says. Tests:
  `removing_recognized_text_is_one_undo_step`,
  `removing_recognized_text_says_what_stays` and
  `it_asks_about_the_selected_pages_or_every_page`.
- **Found:** 2026-10-22, M10.5 slice 18
- **What:** an app command (and a CLI form) that runs `ClearOcrText`
  on the selected pages or all pages, with the usual Notes.
- **Why:** slice 18 made saved OCR layers removable (bug 14), but only
  tests, the API and fuzz call `ClearOcrText`. A user who wants a bad
  OCR layer gone can only re-OCR over it.
- **Overlap:** the OCR dialog (`crates/sheaf-app/src/ocr_*`); M10.5
  slice 19's `sheaf edit` takes any op as JSON, so the CLI form may come
  free.
- **Notes:** small: a command and a confirmation. The op, its Notes
  and the Rewrite it plans exist.

## Sources checked

Outside material already compared against sheaf, so the comparison isn't
done twice.

- **2026-10-21: Gemini's summary of a PDFlyer demo video**
  ([share link](https://share.gemini.google/5pTedPNwgmKO)). It covers
  the calculator, sticky notes, status stamps, connectors and Note
  Summary. It produced B1 (closed) and F1–F3. F4 and F5 came out of
  discussing it.
  - **Already in sheaf from M9:**
    - placing, moving and re-editing tapes, and tape titles;
    - sticky notes;
    - Set Status and Reset Status, with Reset keeping the status stamp;
    - connector groups;
    - Note Summary and Stamp Summary as Comments presets, whose rows
      jump to the mark and select it.
  - **Secondhand, and partly at odds with CCH's help:**
    - In the report, Note Summary includes stamps. In CCH's help it
      covers notes and tapes, and stamps have their own Stamp Summary.
    - In the report, clicking a connector jumps to its target. In CCH's
      help, connectors are label groups navigated from the right-click
      menu.

    Prefer `docs/prior-art/pdflyer.md`, which cites CCH's help.
- **2026-10-22: OCR on pages that already have text.** Adobe's
  [renderable text](https://helpx.adobe.com/acrobat/kb/error-could-perform-recognition-acrobat.html)
  note, ocrmypdf's
  [advanced docs](https://ocrmypdf.readthedocs.io/en/stable/advanced.html)
  (skip, redo, force) and ABBYY's
  [incorrect text layer](https://support.abbyy.com/hc/en-us/articles/360020818320-Recognition-of-the-document-that-has-incorrect-text-layer)
  article. Foxit's documentation didn't say. It produced F10.
- **2026-10-23: QR codes to links.** Neither Acrobat nor Foxit appears
  to do it. Foxit
  [turns URLs in text into links](https://resource-cms.foxit.com/tutorials/how-add-hyperlinks-pdfs-seamless-navigation-and-interactivity),
  and an Adobe forum
  [question asking for this](https://community.adobe.com/t5/acrobat/make-clickable-hyperlinks-from-qr-or-zappar-codes-in-pdf-how/m-p/9981646)
  has no answer. It produced F16.
- **2026-10-23: an appearance with states and no `/AS`.** pdf.js's
  `src/core/annotation.js` (6.4.299) and PDFium, on hand-made files, and
  Acrobat's documentation, which doesn't say. It's in B12's notes.
