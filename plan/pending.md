# Pending

Features we probably want but haven't placed yet. Some are provisional,
some aspirational. **Every new feature proposal starts here.**

When adding an entry:
1. Check the milestones, the parity checklist and Not planned for overlap.
   Note any partial overlap or clear duplicate in the entry. A clear
   duplicate goes into the existing item, not here.
2. Link the research behind it (`docs/prior-art/`, a `plan/` memo).

When reviewing, each entry ends up in one of three places:
- folded into an existing milestone, which usually means a new slice;
- a new milestone, if several pending entries share a theme;
- Not planned, with a reason.

Entries leave this section when they're placed.

### Export to Markdown

Export a document, or a page range, as Markdown: headings, paragraphs,
lists and tables in reading order. This makes binder content usable in
notes, wikis and LLM tools. Bookmarks could supply the heading structure.

- **Overlap:**
  - M7 Export text (plain done; "with layout ❓" left open). This would be
    another format in that same export dialog and `sheaf export`.
  - M17 accessibility (tags tree, reading order). Tagged PDFs give the
    structure directly, and both need a reading-order model.
  - M5 OCR. Scans need a text layer first.
- **Approach:** born-digital pages use PDFium's text plus heuristics
  (font size for headings, x-alignment for columns and tables). Scans can
  use the structure pipeline in `oar-ocr`, which `sheaf-ocr` already
  depends on: layout detection and table recognition, with
  `to_markdown()`. See
  [prior-art/vision-models.md §2](../docs/prior-art/vision-models.md#2-page-parsing-layout-tables-markdown).
- **Open:** which milestone. It fits as an M7 follow-up slice (text-only
  heuristics) or M14 (once reading order exists).

### ML document restoration (DocRes)

Opt-in model-based cleanup for photos and degraded scans: deblurring,
better binarization of faded copies, and shadow and appearance correction
where background division leaves artifacts. One 15 M-parameter model
covers all of these.
- **Overlap:** M5 scan cleanup and camera capture (done). This extends
  them; UVDoc already covers dewarping.
- **Research:** [prior-art/vision-models.md §1](../docs/prior-art/vision-models.md#1-docres).
- **Open:**
  - CPU time per page;
  - whether it improves OCR accuracy, not just looks;
  - the license of the weights;
  - we'd have to make the ONNX export ourselves.

  If it's worth it, it's an M5 follow-up slice beside UVDoc.

### PDFlyer parity gaps

PDFlyer tools that sheaf lacks or does differently, from
[prior-art/pdflyer.md §6](../docs/prior-art/pdflyer.md#6-corrections-and-implications-for-sheafs-readme-and-roadmap)
and [prior-art/README.md](../docs/prior-art/README.md#what-the-prior-art-says-about-useful)
(researched 2026-10-13). Multi-page TIFF with marks rendered in is left
out: M7's TIFF export already renders sheaf's marks.

**Placed 2026-10-14** (M9 planning):
- Into M9: connector groups; Set Status (replacing sign-off stamps); Stamp
  Summary and Note Summary; Recent and Favorite stamps, sticky stamp mode,
  user stamps shared across staff; the tape's section/row format, editing
  a placed tape, rotating and resizing it.
- Into M5: Insert Target Sheets, as part of separator sheets.

What's left is bookmark and page work. Each row was checked against sheaf
on 2026-10-14:

| Gap | sheaf today | Size |
|---|---|---|
| Split Bookmark: split a multi-page bookmark into same-level bookmarks, moving pages between them | Nothing. Split Document splits files. The model already has the edits it needs and the bookmark page-span helper (`DocState::bookmark_pages`). | Medium: a new dialog |
| Delete Bookmark *with its pages* | Delete removes the bookmark and its children; the pages stay. Shift+drag moves a bookmark with its pages. | Small: an option, one undo step (delete the bookmark first, then the pages) |
| Delete Page re-attaches its bookmark to the next page | Every bookmark pointing at a deleted page is removed and its children move up a level; nothing is re-pointed. The test `delete_cascades_and_promotes_children` asserts this, so it changes too. | Small: in `cascade_delete`, point the bookmark at its first surviving page |
| Hide/Show empty bookmarks; colorize by populated vs empty | No hide. The panel dims bookmarks with no destination, which isn't the same as no pages under them. Color Bookmarks by Level colors by depth. | Small to medium. "Empty" is undefined in PDFlyer's help; probably "neither it nor anything under it points at a page". |
| Insert PDF modes: merge by parent bookmarks / append as one bookmark at a chosen level / one bookmark per page / none; offer to delete the source | No dialog. From File… opens a picker and always merges: matching top-level titles take the new children, but the pages aren't moved under them. | Medium: a dialog; modes 2 and 3 are existing edits, mode 1 with real repagination is new |
| **Read existing PDFlyer workpapers** (stamps, connectors, tapes) | No sample. Third-party marks are read-only records that keep only page, subtype, author, text, date and rect. | Large; blocked on a PDFlyer-marked sample. Highest value. M9 slice 3 reads bookmark colors, a prerequisite. |

**Open:**
- These rows fit one short "PDFlyer binder" milestone better than M9.
- The doc corrections from the same §6 were applied to README.md and
  this file on 2026-10-14.

### Follow links in the page view

Clicking a link annotation does nothing today. That covers internal
destinations, URLs (which should get a warning before opening, like the
attachments panel's) and links in files from other tools. Connectors jump
only through sheaf's own code. Found while planning M9.
- **Shift+click follows a link** (Greg, 2026-10-23). A plain click
  stays a click in the text (placing, selecting), so following a link is
  never an accident. Hovering a link shows its target and "Shift+click to
  open"; the right-click menu has Open Link and Copy Link.
- **URLs in the page's text are links too, without changing the file,**
  as Acrobat's "Create links from URLs" preference makes them (added
  2026-10-23). Web addresses and email addresses in the text, OCR text
  included, follow with Shift+click like a real link. PDFium finds them
  (`FPDFLink_LoadWebLinks`: each URL and its rectangles; pdfium-render
  has only the raw binding), or a pattern over the page's words does.
  `settings.toml` can turn it off.
- **What Shift+click does today:**
  - **Shift+drag** selects a region with any tool. A Shift+click without
    a drag doesn't, so it's free on a link.
  - **With the Select tool, Shift+click on a mark** adds it to the
    selection. A highlight over a URL is common, so the mark keeps
    Shift+click and the link opens from the right-click menu.
  - Shift+click doesn't extend a text selection in sheaf, so nothing is
    lost there; it would rule that convention out later.
- **Keyboard:** reaching and opening links without a mouse belongs to
  M17.
- **Overlap:** M13 link editing (create, edit, delete) builds on this,
  and its "create links from URLs in text" writes real links where this
  only shows them. M9 connector groups write `/Dest` cycles that Acrobat
  follows; sheaf doesn't follow them.
- **Open:** where it lands: a small change on its own, or M13's first
  link slice.

### File contention: more than one process editing a PDF

**Mostly fixed in M10.5; the rest deferred.** Suppose another process
changes a PDF while sheaf has it open: a colleague's sheaf on a file
server, Acrobat, or a sync client. Since M10.5 (its proposal 33), sheaf
re-checks the file before every write. When another program changed it,
nothing is written, and the save asks: Save As…, Overwrite or Cancel. A
recovery file is offered only while its PDF is the one it was written
against.
- **What's left,** in [`plan/topics/file-contention.md`](topics/file-contention.md):
  - the check when a window comes to the front, keeping the rest of what
    the original file had (`ReplaceFileW` on Windows, its group on Linux
    and macOS), and saying plainly that another program has the file
    open;
  - a lock file that warns the second person before they start;
  - merging, later.
- **Overlap:** M9 slice 1 (single instance) removes one user's two sheaf
  processes. Slice 2 creates the local state folder the recovery files
  would move to. Slice 3 makes mark ids unique. Slice 4 keeps every
  version's commands back to the last save, which a merge would replay.
  Slice 18's comment import is a manual merge path.

### Agent panel (Claude and Codex)

A chat panel where the user works with an AI agent on the open document,
like Zed's external agents. sheaf launches the agent (Claude Code or
Codex), and the agent handles sign-in with the user's own subscription;
sheaf takes no API keys and never sees a credential. The agent works
through [M16](m16-agent/README.md)'s tools: it sees the document, its
edit history, and every command with its help text, plugin commands
included, and it runs commands and transactions with the user's
approval. The panel adds threads, streamed replies, tool-call cards with
the approvals, and launching the agent over ACP. Decisions and research
(2026-10-14) are in [`plan/m16-agent/README.md`](m16-agent/README.md) (its slices 3–6).
- **Placement:** after M16, whose tools it hosts.
- **Overlap:**
  - M16: the reads, transactions, approvals, version checks and
    "revert this turn" are its tools.
  - M9 slice 4 edit history (done): versions, edits in words, jumps.
  - M15 batch actions. Both need commands that take arguments instead of
    a dialog. Until a command has them, the agent opens its dialog
    pre-filled, so this needn't wait for M15.
  - `plan/topics/plugins/queue.md` skipped "Ask Claude" as a plugin (2026-10-13)
    because AI belongs in core; this is that feature.
  - Foxit's AI Assistant runs commands from chat
    ([prior-art/foxit.md §3.13](../docs/prior-art/foxit.md#313-ai-assistant-ch-15-subscription-only)).
- **Open:**
  - Whether Anthropic allows subscription sign-in through the
    `claude-agent-acp` adapter. If not, Claude runs through Claude Code's
    own headless mode instead of ACP.
  - Limiting each agent to sheaf's tools: both come with shell and file
    tools of their own.
  - Client data leaving the machine: IRC §7216 consent and a firm-wide
    off switch in `settings.toml`.

### Search across files

Find and Patterns over files and folders that aren't open, as Acrobat's
Advanced Search does: "which of this client's PDFs has this EIN?"
- **The CLI first, and soon** (Greg, 2026-10-23): `sheaf search` over
  files and folders, by text, regular expression or a Patterns preset
  (`patterns.json`). It prints the file, page and matched text, or JSON,
  and exits as grep does (0 found, 1 none, 2 error).
  - Each file opens as a `Session` and runs `Text::search` (M10.5 slice
    19), so it searches what sheaf shows, OCR text layers included.
  - Pages with no text are counted, not OCR'd.
  - Encrypted files are listed and skipped unless a password is given.
- **The GUI is open.** With one document per window, the results need a
  window of their own: hits grouped by file, each opening its file at the
  page. Open: where it starts (File menu, palette, recent files), and
  whether it also searches open windows' unsaved changes.
- **Overlap:** M15's batch actions (a search's files as a batch's input)
  and M11's redact by pattern, both over many files; M16 (an agent could
  call it).
- **Where:** the CLI is placed in the
  [between-milestones work](between/README.md), item 5 (2026-10-23).
  The GUI stays here.

### Spell check

Underline misspelled words as they're typed, with suggestions and "Add
to dictionary" on right-click, as Acrobat checks comments and form
fields. Notes, comments and filled forms go to clients.
- **Where:** sheaf-ui's `TextInput` and `TextArea` (every panel and
  dialog), notes, replies and free-text marks, and form fields once M10
  fills them on the page. Not number fields.
- **Engines:**
  - Windows' `ISpellChecker` and macOS's `NSSpellChecker`: the user's
    own languages and dictionary, for free.
  - spellbook (Helix's pure-Rust, Hunspell-compatible checker; MPL-2.0,
    alpha, no phonetic suggestions) with Hunspell dictionaries: the same
    on every system, and the only choice on Linux, where
    `/usr/share/hunspell` has the system's dictionaries.
  - zspell (pure Rust); its license and suggestion API are unchecked.
- **Dictionaries:** which to ship, each with its own license, plus a
  word list of tax and accounting terms (EIN, 1099-NEC, K-1, form names)
  so they aren't flagged.
- **Open:**
  - Where "Add to dictionary" writes. Settings are hand-edited files
    sheaf never writes, and `state.json` is disposable, so a user's word
    list is a third kind of file. A firm may want one list for everyone.
  - Which milestone: dialogs and notes could have it before M10's form
    filling does.

### Drag pages between windows

Drag thumbnails from one window to another to copy pages, or with a
modifier to move them. Nice but not a priority (Greg, 2026-10-23).
- **Why:** with one document per window, it's the direct way to move
  pages between binders. The prior art's list of what's useful asks for
  it, after Preview
  ([prior-art/README.md](../docs/prior-art/README.md)).
- **Overlap:** M8's insert from file and Combine (the ops exist), and
  M2's thumbnail drag within a window.
- **Open:** drags between GPUI windows, and to and from the file manager,
  are unexplored. Both windows are in one sheaf process, so the pages
  can come from the source's current state, unsaved edits included.

### A binder index page

Make an index page from the bookmarks: titles with page numbers or
labels, each a link, kept current as pages move, as headers and footers
are. Very low priority, and probably a plugin rather than core (Greg,
2026-10-23).
- **Overlap:** M13's links; M8's header and footer fields, which update
  as pages move.
- **As a plugin, it needs:** a way to insert pages (in M10.5's ops,
  `insert-pages` is the user's alone) and an op that creates links (none
  yet; see backlog F16).

### Document properties and the initial view

Edit the title, author, subject and keywords, and set how the file
opens: bookmarks panel shown, page layout and zoom, and the title in the
window bar instead of the file name. File > Properties is read-only
today (M7). Revisit after M17's PDF/UA work, which sets the title shown
and the language anyway (Greg, 2026-10-23).
