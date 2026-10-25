# Configuration

sheaf keeps what you choose in the platform config directory:
`~/.config/sheaf` on Linux, `%APPDATA%\sheaf` on Windows (which roams with
your profile), `~/Library/Application Support/sheaf` on macOS. What it
remembers for itself goes in a local state directory: `~/.local/state/sheaf`,
`%LOCALAPPDATA%\sheaf\state`, `~/Library/Application Support/sheaf/state`.
`SHEAF_CONFIG_DIR` overrides both (state goes in its `state/`).

- `settings.toml` — your preferences, edited by hand; there's no settings
  view yet. Edit > Open Settings File opens it, writing one with every
  setting explained if there isn't one. sheaf only reads it, again whenever a
  window comes forward; a mistake shows in a banner with its line, and only
  that setting falls back to its default.
  - `[identity]`: `name` (the author of your comments and marks; your user
    name otherwise) and `initials`.
  - `[ocr]`: what the Recognize Text dialog doesn't choose:
    `tessdata_dir` for your own Tesseract data, `det`, `rec`, `dict`,
    `textline_ori` and `doc_ori` for your own PaddleOCR models, `psm`, and
    `use_doc_orientation`.
  - `[stamps]`: `size` (how tall a stamp goes down, in points; 40), and
    `date_format` and `time_format` for dynamic stamps (strftime codes;
    `%m/%d/%Y` and `%-I:%M %p`).
  - `[plugins.<id>]`: a plugin's own settings, whatever it documents; only
    that plugin reads them (as JSON, through API 0.4). A key its manifest
    doesn't name is pointed out in Plugins → Manage Plugins….
- `state.json` (state directory) — written by the app: window placement,
  dock layout (which panels sit where, tab order, sizes, open state), and
  each dialog's last choices, among them the OCR engine, `dpi`, language and
  scope; recent stamps; the comment CSV's columns; the last measuring scale;
  each markup tool's color (`tool_colors`, by tool: `"strikeout": [0, 0, 0]`).
  Safe to delete; a copy that doesn't parse is set aside as
  `state.json.bad`. Only the sheaf that owns the instance writes it.
- `recent.json` (state directory) — File > Open Recent.
- `recovery/` (state directory) — the crash-recovery sidecars of documents
  no file holds yet (see Crash recovery, below), with an empty lock file
  for each one open and `.lock`. Deleting one loses its unsaved pages.
- `patterns.json` — the Patterns panel's presets, seeded from the built-ins on
  first run. Edit, delete, or add them from the panel (right-click a chip;
  "+ Save…" stores the current pattern) or by hand.
- `stamps/` — your stamp collections, a PDF each with a stamp per page
  (an Acrobat custom-stamp file is the same shape), and `favorites.json`.
  Recent stamps are in `state.json`.
- `marks.json` — extra tickmark sets and stamps for the markup toolbar:
  `{ "tickmarks": [{ "name": "Audit", "marks": ["Σ", "◊"] }], "stamps": ["PBC"] }`.
- `plugins/` — installed plugins, one folder each (see
  [`docs/plugins.md`](plugins.md)); `plugins.json` records which are
  turned off and what each may access; `plugin-data/<id>/` holds each
  plugin's storage and the `settings.json` you can give it. Manage them from
  Plugins → Manage Plugins….
- `toolbar.json` — the toolbar's buttons and where it sits, written when you
  customize or move it (View > Customize Toolbar…, or drag its grip to an
  edge or over the page). Items are command ids as the palette shows them,
  `"separator"`, the `"toolbar::Colors"` and `"toolbar::Tickmarks"` widgets,
  or `["plugin::Run", { "plugin": "<id>", "command": "<id>" }]`. A plugin's
  buttons hide while it's off and come back with it.

```json
{ "toolbars": [{ "name": "Main", "dock": "top",
    "items": ["docview::Save", "separator", "tools::ToolHighlight", "toolbar::Colors"] }] }
```

- `keymap.json` — your key bindings, loaded after the defaults so they win.
  Zed's format: a list of `{ "context", "bindings" }` entries. Actions are
  named as the command palette (Ctrl+Shift+K) shows them; `null` unbinds.

```json
[
  { "context": "DocView", "bindings": { "ctrl-shift-k": "tools::ToolTick", "f9": null } },
  { "bindings": { "ctrl-shift-p": "workspace::CommandPalette" } }
]
```

Contexts: `DocView` (the page view), `Outline`, `Comments`, `Palette`,
`OcrDialog`, `PluginForm`, `PluginManager`, `TextInput`, `Workspace` (anywhere in the window), or none for global.
Plugin commands are `plugin::Run` with `{ "plugin": "<id>", "command": "<id>" }`.
Bad entries are reported on stderr at startup and skipped.

**Crash recovery.** A few seconds after each edit, sheaf writes the edits
since the file was saved to `<name>.pdf.sheaf-recovery` next to the file:
an update to the file, encrypted when the file is (a password-protected
file's too), about a kilobyte for a mark on a 500 MB scan. The file
itself is never touched until you save. If sheaf didn't exit cleanly, the
next open offers to recover, as long as the file is still the one the
sidecar was written against, and the recovered document's first save
checks the file again and appends the recovered edits with its own, or
rewrites the file when they removed or replaced text it holds. When no
update can follow the file (it can't take one, or the edits need what a
save pruned from it), the sidecar holds the whole document instead, and
its first save is a rewrite. Saving, or choosing Don't Save, removes the
sidecar. A document no file holds yet (Create from Scans makes one)
keeps its sidecar, the whole document, in the state directory's
`recovery/` (see Configuration), its write started as each batch of
pages comes in (or as soon as the write before it ends), so a crash
mid-scan loses at most the pages that came in since the last write
finished; the next launch offers each one back. Every sheaf keeps its
own untitled documents' sidecars there, a second one started beside a
running sheaf too, each claimed by its name while the document is open,
so none is offered back while its sheaf still runs.
