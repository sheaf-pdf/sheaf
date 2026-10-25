# M17: Accessibility

Status: sketched · its first slice (roles and labels on sheaf-ui's widgets) is between-milestones work (2026-10-23)

Proves: People who use assistive technology can use sheaf and read what it writes · Depends on: The app: nothing unbuilt. Documents: M14's PDF/A validator, if PDF/UA's rules share it (open)

## Summary

Added 2026-10-23, taking over M14's accessibility item. It has two parts
that can run apart: the app, which needs nothing unbuilt, and the
documents, which build on the content interpreter (M11's slice 3,
landed early).

**The app, with a screen reader and the keyboard**
- **Screen readers.** GPUI (gpui-pre 0.3.9) reports its UI through
  AccessKit on macOS, Windows and Linux (AT-SPI). An element with an id
  and a role (`.role(Role::Button)`) is reported with its label; one
  without a role isn't. sheaf-app sets no roles today, so a screen
  reader finds nothing in it.
  - Roles, labels and states for every control: sheaf-ui's widgets
    (`TextInput`, `TextArea`, buttons, menus, lists, dialogs, the
    palette), the panels and the toolbar.
  - Announce what changes out of sight: progress, a finished OCR or
    save, an error.
  - **The page view** is drawn, so it needs a tree of its own: the
    page's text in reading order (from the tags tree when the file has
    one), marks as objects with their contents, and form fields as
    controls. A screen-reader user reads the PDF through it.
- **Keyboard only:** every command, panel and dialog reachable without a
  mouse, with a visible focus. The hard parts are placing and moving
  marks and selecting text on a page.
- **Seeing:** a high-contrast theme, and following the system's text
  size and reduced-motion settings.
- **Checked** against WCAG 2.2 as applied to software (Section 508, EN
  301 549), with NVDA and Narrator on Windows, VoiceOver on macOS and
  Orca on Linux. A firm that answers to Section 508 may ask for a
  conformance report (a VPAT).
- **Open:** how to test it. GPUI has an `a11y` debug module, and
  AccessKit trees can be read back; neither is explored yet.

**Documents: PDF/UA** (ISO 14289: UA-1 on PDF 1.7, UA-2 on PDF 2.0) as
the target ❓
- **Structure:** the tags tree in the model (`Key::StructElem` and
  `/ParentTree`, the M10.5 plan's §20), alt text, reading order, table
  headers, the document's language and its title shown. The content
  interpreter (M11's slice 3) ties content to tags: it reports each
  marked range with its MCID, in each form use.
- **Validate** PDF/UA's rules that software can check, as M14 does
  PDF/A's. The Matterhorn Protocol lists PDF/UA's failure conditions;
  veraPDF validates both standards, and is the reference to compare
  against.
- **Review what only a person can judge:** whether alt text says what
  the figure shows, whether the reading order makes sense, whether a
  heading is tagged as one. Software can't check these, so they need a
  view of their own: the tags tree beside the page, the reading order
  drawn over it, the text as a screen reader would read it, and the
  manual checks to tick off. Acrobat has its Reading Order tool and
  Tags panel; PAC has a screen-reader preview.
- **Tag an untagged file:** layout detection with reading order on each
  page (oar-ocr's PP-DocLayoutV2 orders the regions it finds), then a
  person checks it in the review view. Scans get it after OCR.
- **Keep a tagged file tagged:** what sheaf writes into a PDF/UA file
  (marks, headers and footers, watermarks, OCR text, form fields) is
  tagged or marked as an artifact, so an edit keeps the claim. Today
  M10.5 adds a Note when a change breaks a PDF/A or PDF/UA claim
  (proposal 39).
- **Tests:** `../pdf-corpus/conformance/` has PDF/UA claims (backlog
  F9). veraPDF is the oracle for UA-1 and UA-2, with PAC's Matterhorn
  checks by hand ([docs/oracles.md](../../docs/oracles.md#by-milestone)).
- **Open:** the review as a panel in the main window, or a review mode
  of its own, as Acrobat's Reading Order tool is.

**Its first slice goes early** (2026-10-23): roles and labels on
sheaf-ui's widgets are [between-milestones work](../between/README.md),
so the dialogs M10 and M11 add reach screen readers from the start.

**Decide at kickoff:** which part goes first (the app's can run beside
other milestones), and how far to go on PDF/UA.

Slices: roles and labels on sheaf-ui's widgets → panels, dialogs and
keyboard paths → the page view's tree → the tags tree in the model →
validation → the review view → tagging untagged files.

---

## Plan files

- No plan yet: the milestone is broken into slices when it starts.
- [Build or adopt](../research/build-or-adopt/README.md): the M17 rows, and the [changes to make](../research/build-or-adopt/changes-to-make.md) at kickoff
- Open questions: [asks](../asks.md)
