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
  needs a serde-compatible addition to the tape's stored lines and a text
  form for `/Contents`.

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
