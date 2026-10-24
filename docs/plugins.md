# Writing sheaf plugins

Status: API `sheaf:plugin@0.4.0`, 2026-10-23. Design notes live in
[`todos/plugins.md`](../todos/plugins.md).

A sheaf plugin is a WebAssembly component plus a manifest. It can add
commands to the menus, the command palette and the keymap; read the open
document's pages, bookmarks, markup and text (including recognized text);
change the document with sheaf's own edits, its ops, a call's as one undo
step; ask the user questions with forms; list things in its own dock
panel; react to document events; keep a little data; and, if the user
allows it, read or write files in named folders or use the network.

It runs sandboxed, so one `.wasm` file works on every platform sheaf runs on.
A crash, a hang or runaway memory unloads the plugin and leaves the app
running.

sheaf ships its own plugins this way (the repo's [`plugins/`](../plugins)):
[`tax-form-bookmarks`](../plugins/tax-form-bookmarks) and
[`amount-crossref`](../plugins/amount-crossref) are good worked examples.

## Quick start (Rust)

```sh
rustup target add wasm32-wasip2
cargo new --lib my-plugin && cd my-plugin
```

`Cargo.toml`:

```toml
[package]
name = "my-plugin"
version = "0.1.0"
edition = "2024"

[lib]
crate-type = ["cdylib"]

[dependencies]
# Not on crates.io yet: build against a sheaf checkout.
sheaf-plugin-api = { path = ".../sheaf/crates/sheaf-plugin-api" }

[profile.release]
opt-level = "s"
lto = true
strip = true
```

`src/lib.rs`:

```rust
use sheaf_plugin_api::{Ops, Plugin, document, text};

struct MyPlugin;

impl Plugin for MyPlugin {
    fn run_command(id: String, _args: Option<String>) -> Result<(), String> {
        match id.as_str() {
            "bookmark-first-lines" => {
                let mut ops = Ops::new();
                for p in document::pages() {
                    let t = text::page_text(p.id)?;
                    if let Some(line) = t.lines().map(str::trim).find(|l| !l.is_empty()) {
                        ops = ops.bookmark(line, Some(p.id));
                    }
                }
                if !ops.is_empty() {
                    ops.submit("Bookmark first lines").map_err(|f| f.message)?;
                }
                Ok(())
            }
            other => Err(format!("unknown command {other}")),
        }
    }
}

sheaf_plugin_api::export!(MyPlugin);
```

`sheaf-plugin.toml`, next to `Cargo.toml`:

```toml
id = "my-plugin"
name = "My Plugin"
version = "0.1.0"
api = "^0.4"
edit-schema = "1.0"
homepage = "https://example.com/my-plugin"

[[commands]]
id = "bookmark-first-lines"
label = "Bookmark &First Lines"
icon = "icons/bookmark-first-lines.svg"
help = "Bookmark each page, titled with its first line of text."
menu = "Tools/My Plugin"
```

and `icons/bookmark-first-lines.svg` beside it (see [Icons](#icons)).

Build and install:

```sh
cargo build --release --target wasm32-wasip2
dest=~/.config/sheaf/plugins/my-plugin     # %APPDATA%\sheaf\plugins on Windows
mkdir -p "$dest"
cp -r sheaf-plugin.toml icons "$dest/"
cp target/wasm32-wasip2/release/my_plugin.wasm "$dest/plugin.wasm"
```

Restart sheaf, or use **Plugins → Manage Plugins… → Install from Folder…**
on a folder holding `sheaf-plugin.toml`, `plugin.wasm` and `icons/`. To hand
a plugin to someone else, zip those (at the top level or in one folder) and
rename the zip to `name.sheafplugin`; **Install from File…** takes it.

Other languages work too: anything that builds a WASI 0.2 component for the
`plugin` world in [`wit/sheaf.wit`](../wit/sheaf.wit) (C with wit-bindgen,
Go with TinyGo, JavaScript with ComponentizeJS, …). Edits are JSON, so a
plugin in another language writes the envelope [below](#the-edits-schema)
by hand or from the schema.

## The manifest

`sheaf-plugin.toml` is static data: menus, presets and marks appear without
running any plugin code.

| Key | Meaning |
|---|---|
| `id` | 1–64 lowercase letters, digits and dashes. Unique. Can't be a built-in action namespace (`docview`, `tools`, `workspace`, …). |
| `name`, `description` | Shown in the plugin manager. |
| `homepage` | Optional `https://` URL (docs, source, changelog), linked from the plugin manager. |
| `version` | Semver of the plugin. |
| `api` | Semver requirement on the API: `"^0.4"`. sheaf serves one minor of it and refuses a plugin whose range doesn't take it ([Versioning](#versioning)). |
| `edit-schema` | The version of the edits the plugin writes, `"1.0"`: the `v` of every envelope it submits. Required for a plugin that edits; leave it out for one that only reads. sheaf refuses, at load, a plugin built for a newer minor than it reads ("built for edits 1.1; this sheaf reads up to 1.0"). |
| `[[commands]]` | `id`; `label` (`&` marks the menu mnemonic); `icon`, an SVG in the plugin folder such as `"icons/scan.svg"` (see [Icons](#icons)); `help`, a sentence or two of plain text shown in tooltips and the command palette. Optional: `keywords` (more words the palette finds it by), `category` (palette prefix; defaults to `name`), `menu` (a path like `"Tools/Workpapers"`; menus and submenus are created as needed), `keys` (a default binding in `keymap.json` syntax; the user's keymap overrides it), `keys-context` (where `keys` applies, as a keymap context such as `"DocView && !TextInput"`; by default anywhere in the window, text fields included, so give single-key bindings a context), `when` (what must hold for it to run, all of: `"document"` a document is open, `"selection"` an annotation is selected, `"pages"` pages are selected in the thumbnails, `"region"` a region is Shift+dragged; menus and the palette grey the command out otherwise, and sheaf decides without running plugin code). `icon` and `help` are required. |
| `[[panels]]` | `id`, `label`. Each gets a dock tab and a **View → Plugin Panels** entry. |
| `events` | Document events to receive: any of `"opened"` (once the file has opened: its marks, form, bookmarks and the rest are read), `"changed"`, `"edit-refused"`, `"saved"`, `"selection-changed"`, `"closed"`, `"region"` (the region the user Shift+drags on a page, live while dragging, and its clearing). See [Events](#events). |
| `[[presets]]` | `name`, `regex`: pattern-search presets shown (read-only) in the Patterns panel. |
| `[marks]` | `tickmarks = [{ name, marks = ["Σ", …] }]` and `stamps = ["PBC", …]` for the markup toolbar. |
| `[permissions]` | `read = ["~/Documents/Clients"]`, `write = [...]`: absolute paths or `~/…`. `network = ["api.example.com"]`: hosts the plugin may make HTTP requests to (`"*.example.com"` includes subdomains, `"host:8080"` limits the port). `identity = true`: the user's name and initials ([Who made what](#who-made-what)). See [Trust model](#trust-model). |
| `preferences` | The keys the plugin reads from its section of `settings.toml` (see `storage`), e.g. `["target", "round"]`. A key there that isn't listed is reported in the plugin manager as a likely typo. Leave it out and nothing is checked. |

### Icons

Every command has one: it's what toolbars, the command palette and the
plugin manager show. Draw it in the style of sheaf's own
([`assets/icons/fns/README.md`](../assets/icons/fns/README.md)): a 24 × 24
viewBox, 1.75-unit rounded strokes, no fill, and `currentColor` for every
stroke, so it follows the theme and the button's state. It should read at
16–32px. sheaf checks when it loads the plugin that the file exists inside
the plugin folder, is valid SVG and is at most 64 KB; a command whose icon
fails is left out and the plugin manager says why.

Plugins don't put themselves on the toolbar; users do, from View >
Customize Toolbar… or by right-clicking the command in a menu or the
command palette. A plugin command on the toolbar is hidden while the
plugin is off and comes back with it.

A command runs as the action `plugin::Run`, so users can bind it themselves:

```json
[{ "bindings": { "ctrl-alt-b": ["plugin::Run", { "plugin": "my-plugin", "command": "bookmark-first-lines" }] } }]
```

The second element's `args` (any JSON) reaches `run_command` as a string.

## The API

All of it is in [`wit/sheaf.wit`](../wit/sheaf.wit), with each function
documented. The Rust SDK re-exports each interface as a module and adds
helpers: [`Ops`](../crates/sheaf-plugin-api/src/ops.rs) builds edits,
[`view`](../crates/sheaf-plugin-api/src/view.rs) maps positions to and
from the page as it shows, and `FormBuilder` and `progress` cover forms
and progress bars. It re-exports sheaf's edit vocabulary too: `Op` and
everything an op holds, from the `sheaf-edit` crate (`sheaf_edit`).

**Entry points** (the `Plugin` trait; only `run_command` is required):
`activate` once after loading, `run_command(id, args)`,
`panel_action(panel, row, action)` when the user clicks a row action, and
`on_event(event)` for subscribed events.

### PageSpace and the page as it shows

Every position the API gives or takes is in **PageSpace**: the page's own
user space, in points, y up, as the file has it before its `/Rotate` and
before any turn the user added in sheaf. A page's boxes, its text and
words, its marks, a render's `to-page`, OCR words, bookmark positions and
the ops you submit all use it, so a position read from one goes straight
into another. A page turned in sheaf keeps its PageSpace: what you placed
stays on the content it was placed on.

To place something where the user sees it ("12 points above the match,
as the page shows"), go through the page as it shows: points, y down,
from the top-left corner of what shows (the crop box, turned as it
shows). Each `page-info` carries the two maps: `to-view` (PageSpace to
the page as it shows) and `from-view` (back). The SDK's `view` module
applies them: `to_view` and `from_view` for points, `box_to_view` and
`box_from_view` for boxes, `dest_from_view` for a bookmark position
("this shown top, the viewer's own left"), and `shown_size`.

```rust
// A 40×20 box 100 points in from the left and 200 down, as shown.
let shown = Box2 { x: 100.0, y: 200.0, w: 40.0, h: 20.0 };
let rect = view::box_from_view(&page, shown);
```

A box (`box2`) is its corner nearest the origin and its size; in
PageSpace that corner is its bottom-left.

### Reading the document

**`document`**: `path`, `version`, `pages`, `outline`, `annotations(page)`,
`selection`, `current-page`. Ids are opaque and stable while the document
is open; they aren't page numbers. `path` is none for a document with no
file yet (a scan not saved). With no document open, `path` and `version`
are none and the lists are empty.
- `pages` gives each page's id, its media and crop boxes, how it's
  turned as it shows (`rotate`: the file's `/Rotate` with any turn added
  in sheaf), its label (its page label, or its number) and its two
  maps.
- `outline` lists the bookmarks in pre-order with their depth, each
  bookmark's page and where on it it lands (`dest-view`: `/XYZ` with a
  PageSpace point and zoom, or another PDF destination kind as the file
  has it).
- `version` is the call's base: `{instance, id}`. `id` names a state of
  document `instance` only. A document opened again is another instance,
  even at the same path, and ids kept from before mean nothing in it, so
  key anything you keep between calls by `instance` (as
  `plugins/amount-crossref` and `plugins/duplicate-pages` do).

**What a call reads is what it decides by.** A call reads one version
of the document, its base: the document as it is at the call's first
read or submit (a command that first shows a form reads the document as
the user left it). Each read is recorded: `pages` reads the order, each
page's content and boxes, and the labels; `outline` the top level and
each bookmark; `annotations(p)` page p's list of annotations and each one
shown; `text` and `render` what the page draws. When the call's edits
land, each must still hold as it was read, or they don't land
([One transaction per call](#one-transaction-per-call)). So read what
you decide by, and no more: listing the pages reads their order, so a
page inserted, moved or deleted anywhere since refuses the call's edits,
while an edit to a page the call never read lands beside them.

**Reads see the call's own edits.** After a submit, every read shows
what the submitted edits made (a bookmark in `outline`, a mark in
`annotations`, a turned page in `pages` and `render`), and what the call
itself changed isn't recorded as a read: its own edits can't refuse it.

### Marks and comments

**`annotations(page)`** lists a page's annotations in the file's order,
each an `annot-view`:
- **`mark`**: one of sheaf's marks, read whole (`annot`). Every kind sheaf
  has: highlight, underline, strike-out, squiggly, replace (a suggested
  replacement) and insert, note, reply and status (threaded by `name`),
  free-text and callout, square, ellipse, line (with `arrow-start`),
  polygon, polyline and ink, tickmark, connector, page-stamp, text-stamp
  and page-border, file-attachment, measure (distance, perimeter or area
  with its scale), and tape. Each has its `color`, `opacity`, `stroke`,
  `fill`, `dashed`, `cloudy`, `created`, `modified`, `author`, `contents`
  and `name` (its `/NM`, which replies and review states name it by).
- **`comment`**: another tool's comment, which sheaf shows and keeps but
  doesn't edit, as a read-only summary: its `subtype`, `rect`, `contents`,
  `author` and `name`.

A form field's widget is never listed, nor a link, nor an annotation the
file hides.

#### Who made what

A plugin never learns who the user is unless the user lets it. Authors
are told as roles: `you` for the user, `plugin <id>` for an installed
plugin, and `other` for anyone else. A mark a plugin makes is by the
plugin (its author is its id, whatever an op says), and `<<name>>` and
`<<initials>>` in its text (and in a text stamp's) stay as written;
`<<date>>` and `<<time>>` are filled in as dynamic stamps fill them.

A plugin that needs the user's name declares `identity = true` under
`[permissions]`, and sheaf asks the user ("Know your name"). Once
allowed, `<<name>>` and `<<initials>>` in its marks are filled in, and
authors are told by name (an installed plugin's still as `plugin <id>`).
No bundled plugin asks for it.

### Text, renders and OCR

**`text`**: `page-text`, `pattern-search(page, regex)`,
`search(page, query, match-case)`, `text-boxes(page, rect)` and
`words(page, rect?)` (every word with its box). A page's text is the page
as the file will draw it now: its own text, recognized (OCR) text and a
header or footer, saved or not, but never a form field's value or any
annotation's text. Words are apart by a space and lines by a newline; a
`search`'s words match where the page's are apart by any whitespace, so a
phrase wrapped onto the next line is found; it ignores case unless
`match-case`. Matches come with their boxes in PageSpace, one per line
they span, and pattern matches carry their capture groups. A call's text
read records what the page draws as text: an edit since to the page,
a header, footer or watermark on it, or its OCR text refuses the call's
edits, and a comment added to it doesn't.

**`render`**: read-only page images. `page-renderer(page)` draws the page
as it shows, turned as it shows (or a region of it, in PageSpace) at a
dpi you choose (up to 600), in `gray8` or `rgba8`, as the file will draw
it now: with sheaf's marks, a header or footer saved or not, and other
tools' comments and form fields' appearances. Set `without-markup` for
the page alone: no marks and no other tool's comments (form fields still
drawn), as a print without markup draws it. The pixels stay in sheaf: an
`image` gives its size, maps pixels to PageSpace (`to-page`), and hands
over rows or tiles as you read them. Handles last for one call.
Rendering time doesn't count against the call's limit, but each call
may render at most 400 million pixels and keep 16 images alive at once.
`plugins/duplicate-pages` uses it to fingerprint pages.

**`ocr`**: `recognize(page, options)` runs the user's OCR engine on one page
and returns its words with boxes in PageSpace. It reads the page's own
content, as sheaf's OCR does: no form fields, comments, markup, or
header, footer, watermark or background sheaf draws, saved or not, are
drawn into what it reads. It doesn't change the document, and a plugin
can't set a page's recognized text. Text a page has been recognized
with, saved or not, is the page's content: read it with `text`.

### One transaction per call

**`edits`**: `submit(label, ops)`, `preview(ops)` and `commit()`. Ops are
sheaf's own edits, as JSON: the envelope `{"v": "1.0", "ops": [...]}`,
`v` the manifest's `edit-schema` ([the schema](#the-edits-schema)). The
SDK's `Ops` builds them and reads the answers.

- **`submit`** lowers the ops into the call's transaction at once, on the
  plugin's thread: all of them, checked now, or none and why. It answers
  what they made (`Applied`: `created`, each op's new pages, bookmarks
  and marks in order; `locals`, what each local named; and `lines`, as
  History will say them) or why not (`Failure`: a `kind` such as
  `not-found`, `invalid`, `refused` or `conflict`, and the `message` the
  user would see). The ids it answers with are the ones the edits keep
  once they land. `label` is what undo calls the call's edits: the first
  submit's. An envelope with a label of its own, or another `v` than the
  manifest's, is `invalid`.
- **Later ops name what earlier ones made by locals:** an op gives
  `"local": "$1"` to what it makes, and the ops after it name it `"$1"`
  (a new heading's children, a note's reply). Locals last until the
  call's next `commit`; the ids in the answer last as long as the
  document is open.
- **`preview`** says what ops would do, applied now, with nothing applied:
  `lines`, `created`, `locals`, `caps` (what each op needs, in the words
  of the [list below](#what-a-plugin-may-do): a plugin's are `"plugin"`,
  since an op it may not make is refused, as `submit` would refuse it)
  and `removes` (whether they'd remove something that's there: a page, a
  comment, a bookmark). The ids it names are drawn, and never given
  again.
- **The call's edits land when it returns** (`Ok`), as one undo step,
  called as its first submit is. They're made on the call's base, and
  land on the document as it is then: its reads must still hold, and its
  ops are made again on the current version, making what they made at
  first under the same ids. So an edit the user made meanwhile to
  something the call didn't read lands beside them. If any read changed,
  or the ops would make something else, nothing lands: the user sees
  which change got in the way, and the plugin gets `edit-refused` if it
  subscribes.
- **`commit()`** lands the call's edits so far now, and waits while sheaf
  lands them on its UI thread; the waiting isn't counted against the
  call's limit. It answers the version they made, or why they didn't
  land (a conflict, say: those edits are gone, and the call's next edits
  are made on the document as it is). Edits after it are made on that
  version, and join it as one undo step unless the user edits in
  between, when History shows the call's parts with "Undo all of …".
  With nothing submitted, it answers the version the call reads. A call
  whose document's window has closed is told "its document was closed".
- **A call that fails, traps or is stopped** (`Err`, a panic, a timeout,
  or the user's Escape) changes nothing: what it submitted is dropped,
  and what it committed is taken back, leaving any edit the user made in
  between. Should the user have changed since what it committed, its
  edits stay, and the user is told so.
- **A call made on a document that has since been closed changes
  nothing.**

#### What a plugin may do

`Cap::Plugin`, which needs nothing more: move, turn, delete and duplicate
pages (`move-pages`, `rotate-pages`, `delete-pages`, `duplicate-pages`);
set the page labels (`set-page-labels`); add, change, move and delete
bookmarks (`add-bookmark`, `patch-bookmarks`, `move-bookmarks`,
`delete-bookmarks`); add, change and move sheaf's marks (`add-mark`,
`patch-marks`, `move-marks`), replies and review states among them, and
page stamps and attachments of what it uploads; add a connector pair
(`add-connector`); and delete sheaf's marks (`delete-annotations` of
marks). Bookmarks land at a position on their page (`view`), and a mark
made with the SDK's `Ops::mark` reads upright as its page shows.

What it may not (`refused`, and nothing is applied): what needs the
user's approval (`approval`), such as deleting another tool's comment or
a link, making another tool's comment a mark (`adopt-annotations`),
cropping (`set-page-boxes`), watermarks, backgrounds, headers and
footers, clearing OCR text and flattening (`flatten-annotations`); and
what only the user makes (`user`): inserting or replacing pages
(`insert-pages`, `replace-pages`), setting OCR text, and an imported
comment's own name and dates (`imported`). A plugin never names a blob by
id (`{"blob": …}` is refused); it uploads what its edits hold. No
manifest permission grants more in 0.4; `identity` changes what a
plugin's marks say, not what it may do.

### Assets

**`assets`**: `upload(bytes, kind)` hands sheaf bytes for the call's edits
to hold, and answers a handle that ops name as `{"asset": n}` (the SDK's
`upload` gives a `BlobSel`). Kinds:
- `image`, a PNG or JPEG, for a page stamp, drawn at 72 pixels to the
  inch;
- `pdf`, for a page stamp of its first page;
- `file`, for an attachment (`file-attachment`).

A handle names its upload until the call returns, through any `commit`.
A call's uploads hold at most 64 MiB together, an image counting its
bytes and its pixels at four bytes each (a 4,000 by 4,000 image takes
61 MiB), told by its header before it's decoded. What a stamp or
attachment holds is kept in the document as a paste's is, and is written
into the file when it's saved.

```rust
let dot = sheaf_plugin_api::upload(PNG, AssetKind::Image)?;
Ops::new()
    .page_stamp(&page, Box2 { x: 100.0, y: 100.0, w: 40.0, h: 40.0 }, dot, "Dot")
    .submit("Stamp")
    .map_err(|f| f.message)?;
```

### Forms, panels and the rest

**`ui`**: `show-form(form)` asks the user and blocks until they answer (none
if they cancel). Fields: text, number (with optional bounds), choice,
checkbox, file (open or save) and page range (answered as page ids).
`FormBuilder` in the SDK builds forms and reads answers. `notice(message)`
shows a line in the status area (an empty message clears it).
`set-panel(panel, rows)` replaces the rows of one of your panels for the
current document. Each row has a label, detail, badge, icon, indent, an
optional page (clicking the row goes there) and buttons that call
`panel-action`. `show-panel(panel)` brings a panel into view without
taking keyboard focus: one of yours, or sheaf's own `pages`, `bookmarks`,
`patterns`, `calc` or `comments` (say, to show the comments a command
just added). Setting a panel's rows also shows it.

**`patterns`**: `presets()` returns the user's Patterns presets, then those
plugins ship.

**`storage`**: `get`, `set`, `delete`, `keys` for a key/value store kept in
`<config>/plugin-data/<id>/store.json` (4 MiB per plugin), and `settings()`,
the JSON the user puts in `<config>/plugin-data/<id>/settings.json` (`"{}"`
when there is none). Uninstalling keeps this data.

`preferences()` is the plugin's own section of the user's
`settings.toml`, as JSON (`"{}"` when there is none):

```toml
[plugins.currency-convert]
target = "EUR"
round = 2
```

It's read when the call starts and stays the same for the whole call, even
if the user saves the file meanwhile. A plugin reads only its own section,
and needs no permission for it. Declare its keys in the manifest
(`preferences`) so a misspelled one is pointed out.

**`progress`**: for long commands. `task(label, total, unit)` shows
in the document's status bar as "label: 12 of 300 pages…" with a progress
strip; `set`, `inc` and `set-total` move it, `message` replaces the line,
and `child(label, weight)` makes a step covering part of it. Dropping a
task or step, or returning from the command, fills its part of the strip;
one the user stopped stays where it was. Make one task per command: the
strip follows the first, and a later one only changes the status line.
Escape asks the command to stop: `is-cancelled` turns true, and the plugin
should return (an error like "Stopped" is fine; its edits are taken back
either way). The call's time limit still applies. The SDK's
`progress::pages(label, n)` and `progress::check(&task)?` cover the usual
loop:

```rust
let task = progress::pages("Reading pages", pages.len() as u64);
for p in &pages {
    progress::check(&task)?;
    // … work on p …
    task.inc(1);
}
```

**`log`**: `log(level, message)` goes to sheaf's log with the plugin id.

**HTTP** is standard `wasi:http`: sheaf makes the request (and the TLS
connection) if the host is one the manifest lists and the user allowed;
anything else fails as denied. The SDK's `http::get(url)` reads a whole
response. Time spent waiting on the network doesn't count against the
call's limit. There are no raw sockets.

### Events

Events arrive like any other call, through `on_event`, for the events the
manifest subscribes to. Edits and forms work in them as in a command.

- **`opened`** comes once the file has opened: its marks, form, bookmarks
  and the rest are read.
- **`changed(change-info)`** comes for every change to the document but
  the plugin's own: what made it (`cause`: commit, amend, undo, redo,
  jump, rollback, revert or coalesced), who by kind (`actor-kind`: user,
  plugin, job or mixed; never a name), the versions it went from
  (`parent`) and to (`version`), and what it changed (`scopes`): the
  order; pages whose geometry, content or annotations changed; the
  outline, the labels, the form; and generators (a watermark,
  background, header or footer, which any page it's on may draw
  differently). Invalidate what you keep by the scopes your reads
  depend on: `plugins/subtotal` forgets a page's numbers when
  `scopes.content` names it, and all of them on a change to generators,
  order or labels.
- **Changes made while the plugin is busy wait, merged:** one
  `changed` per document, from the earliest `parent` to the latest
  `version`, with the union of their scopes, `coalesced` when their
  causes differ and `mixed` when their kinds do. A plugin whose last
  version is `parent` missed nothing; one whose isn't reads again. Only
  changes alike to answer merge (below): a user's edit made after an
  undo, or after another plugin's answer, comes as a `changed` of its
  own, so the plugin's answer to it isn't refused for what came before.
- **`edit-refused(why)`** says the plugin's edits didn't land, and why:
  something its call read changed since, or the edits answered a change
  that may not be answered (below).
- **`saved(path)`**, **`selection-changed`** and **`closed`**; `closed`
  comes only to a document that had `opened`.
- **`region(region)`** and **`region-cleared`**: the region the user
  Shift+drags on a page, live while dragging, in PageSpace. Region events
  are latest-wins: while a plugin is handling one, newer ones replace
  each other and only the newest is sent next. For live work, fetch a
  page's words once and filter them yourself, as `plugins/subtotal` does.

**Answers to events stop.** A plugin's edits made from an event are its
answer to it, and some are refused (the submit, preview or commit
answers `refused`, nothing lands, and the plugin gets `edit-refused` if
it subscribes):
- an answer to an undo, redo, History jump, rollback or "Undo all of"
  ("the user undid this"), so a plugin can't fight the user's undo;
- an answer to a change that was itself a plugin's answer to one ("it
  answers a plugin's answer to a change, and those stop there"), so two
  plugins can't answer each other forever: the user's edits, and a
  command's or a panel action's, can be answered once, and the answer
  can't;
- an answer to `edit-refused`, which isn't told again.

**While a file opens**, its pages show before sheaf has read the rest of
it, and a plugin's commands are greyed out. A call given meanwhile (a
command by its keys, a panel action, an event) waits: once the file is
read, `opened` comes first, then the calls that waited, in order, each
seeing the document as it is then. Of region events only the newest
waits. A file closed before it's read drops the calls that waited (a
recovery opens a file again).

### Limits

- **Time**: each call has 30 seconds of the plugin's own time. Waiting for
  the user (`show-form`), for OCR, for the network, for a render and for
  a `commit` doesn't count. Past the limit, the call traps.
- **Memory**: 256 MiB of linear memory.
- **Traps** (a panic, a timeout, running out of memory) unload the plugin
  for the rest of the session, and the call's edits are taken back. The
  user sees what happened, and the plugin manager shows the error.
  Returning `Err(message)` from an entry point is not a trap: the user
  sees the message, the call's edits are taken back, and the plugin stays
  loaded.
- **Threading**: one call at a time per plugin, in order, on the plugin's
  own thread. Global state in your module persists between calls until
  sheaf exits or the plugin is turned off. Keep anything that must
  outlive that in `storage`.
- **Uploads**: 64 MiB a call, an image's pixels counted. **Renders**: 400 million pixels a call, 16
  images alive. **Storage**: 4 MiB.

## The edits schema

The ops are sheaf-edit's `Op`, in JSON: the schema is
[`crates/sheaf-edit/schema/ops-1.0.json`](../crates/sheaf-edit/schema/ops-1.0.json),
generated from the Rust types and checked in CI, and `sheaf edit --schema`
prints it. Its descriptions say what each op and field does, and
[`docs/document-model.md`](document-model.md) explains the vocabulary
(how ops name pages, bookmarks and marks, and what each makes). A field
sheaf doesn't know, or a reference of the wrong kind, is `invalid` with
the place it's at, rather than ignored.

An envelope holds the edits' version and the ops, applied in order, each
on what the ones before it made. A plugin names pages, bookmarks and
marks by the ids the document reads give it (a page is its bare id), or
by position (`{"pos": 3}`, 1-based, against the document as each op
finds it), a bookmark by its titles (`{"path": ["Part II"]}`) and a
comment by its `/NM` (`{"nm": …}`). The examples below run in sheaf's
tests, each on a document of four Letter pages whose ids, as `pages`
gives them, are 0 to 3.

A heading at the end of the outline, with two bookmarks under it that
name it by its local; the second lands 72 points from the top of a
Letter page:

<!-- plugin-ops example -->
```json
{"v": "1.0", "ops": [
  {"op": "add-bookmark", "at": "last", "title": "Schedules", "target": 1, "local": "$1"},
  {"op": "add-bookmark", "parent": "$1", "at": "last", "title": "Schedule A", "target": 1},
  {"op": "add-bookmark", "parent": "$1", "at": "last", "title": "Schedule B", "target": 2,
   "view": {"Xyz": {"left": 0, "top": 720}}}
]}
```

A note and a reply to it, threaded by the note's local, and a tickmark
beside it:

<!-- plugin-ops example -->
```json
{"v": "1.0", "ops": [
  {"op": "add-mark", "page": 0, "local": "$1",
   "mark": {"kind": {"type": "note", "pos": {"x": 540, "y": 740}}, "contents": "Tie out to the K-1",
            "style": {"color": [255, 200, 0], "opacity": 1, "stroke": 1}}},
  {"op": "add-mark", "page": 0, "reply_to": "$1",
   "mark": {"kind": {"type": "reply"}, "contents": "Tied.",
            "style": {"color": [255, 200, 0], "opacity": 1, "stroke": 1}}},
  {"op": "add-mark", "page": 0,
   "mark": {"kind": {"type": "stamp", "rect": {"x": 500, "y": 700, "w": 14, "h": 14}, "glyph": "✓"},
            "style": {"color": [0, 120, 255], "opacity": 1, "stroke": 1.5}}}
]}
```

Tidy the pages: the last turned a quarter clockwise, the third moved to
the front, and a copy of the second, named by its local, moved to the
end:

<!-- plugin-ops example -->
```json
{"v": "1.0", "ops": [
  {"op": "rotate-pages", "pages": {"ids": [3]}, "by": 90},
  {"op": "move-pages", "pages": {"ids": [2]}, "before": 0},
  {"op": "duplicate-pages", "pages": {"ids": [1]}, "locals": ["$1"]},
  {"op": "move-pages", "pages": {"ids": ["$1"]}}
]}
```

## Versioning

**Until sheaf's first release: one world, an exact-minor check, and
`edit-schema`.**
- sheaf serves one plugin world, `sheaf:plugin@0.4.0`. A manifest's `api`
  must take its minor (`"^0.4"`); a plugin built for another is refused
  at load, with a message, and is rebuilt against the SDK of the sheaf
  it runs in. No earlier world is served beside it.
- Edits cross as JSON of sheaf's own `Op`, versioned on their own by
  `edit-schema`. A new op, or a new field of one, is a new minor of the
  edits (1.1) and needs no new world; a sheaf reads every earlier minor
  of its major, and refuses at load a plugin built for a newer one.
- Only a new read (a function or record of the WIT) changes the world,
  which bumps its minor while 0.x. The SDK crate's version is the WIT
  package's.

**0.4.0** (2026-10-23) is a new world, and sheaf serves no other: 0.2
and 0.3 plugins don't load, and are rebuilt against SDK 0.4.
- Edits are sheaf's ops as JSON (`edits.submit` with an envelope),
  lowered at once and answered with what they made; `preview` and
  `commit` are new. 0.3's `edit` variant and the SDK's `Batch` are gone:
  build ops with `Ops`.
- A call's edits are one transaction, landing when it returns (or at
  `commit`) on the document as it is then, and taken back when it fails.
- Positions are PageSpace, with each page's `to-view` and `from-view`;
  0.3's top-left frame of the page as the file displays it is gone, and
  renders draw the page as it shows.
- `annotations` gives sheaf's marks and other tools' comments as
  summaries; authors are roles; `document.version`; `page-info` has the
  page's boxes and label; `without-markup` renders; `assets.upload`.
- The `edited` event became `changed`, with semantic scopes, merged
  while the plugin is busy, and `edit-refused` is new.
- The manifest gained `edit-schema` and `permissions.identity`.

**0.3.0** (2026-10-15) had every kind of mark sheaf has, and their new
fields (`fill`, `dashed`, `cloudy`, `arrow-start`, `created`, `modified`,
`name`), and `storage.preferences`. sheaf served 0.2 beside it until
0.4. Before its release, 0.3 lost `ocr.page-text-layer`: recognized text
is the page's content, which `text` reads.

**0.2.1** (2026-10-14) added the `progress` interface. **0.2.0**
(2026-10-14) changed the manifest, not the WIT: every command needs
`icon` and `help`, and a plugin package is a folder (or archive) with its
`icons/`.

## Trust model

Plugins are untrusted code. The sandbox is the WebAssembly component
boundary (wasmtime) plus WASI with nothing granted by default:

- **Files.** None, unless the manifest lists directories under
  `[permissions]` *and* the user allows it. A granted directory is visible to
  the plugin at its own path (with `/` separators). `read` directories are
  read-only. `write` directories can be read and written.
- **Network.** HTTP only, to the hosts the manifest names, and only after
  the user allows it (the prompt lists each host). No raw sockets.
- **Identity.** Never, unless the manifest asks for `identity` and the
  user allows it: authors are roles, and `<<name>>` and `<<initials>>`
  stay as written ([Who made what](#who-made-what)). A plugin is told the
  document's path (`document.path`), though, which may hold the user's
  login name.
- **Consent.** When a plugin asks for permissions the user hasn't answered,
  it runs without them and sheaf asks once, in a prompt listing exactly what
  it wants. Answers are kept in `<config>/plugins.json`. A later version
  asking for more asks again. The plugin manager shows each plugin's
  permissions and can allow or revoke them at any time.
- **The document.** A plugin sees the documents the user runs it on (or that
  its events fire for), and changes them only through undoable edits of
  its own: a call is one undo step, and what needs the user's approval is
  refused. It never sees a form field's widget or value through
  `annotations` or `text`.
- **Not yet:** code signing and a plugin catalog. Install plugins from
  people you trust. sheaf's bundled plugins come from this repository.

Plugins can't draw their own UI. Forms and panels are described as data and
sheaf draws them, so a plugin can't imitate sheaf's own dialogs or read the
screen.

## Plugins that ship with sheaf

The `plugins/` directory holds plugins that are part of sheaf. They use the
same API as anyone's. `scripts/build-plugins.sh` builds and stages them;
release bundles carry them in `plugins/` beside the binary. On startup,
sheaf installs any it hasn't installed before into the user's plugin
directory. From then on they're ordinary plugins: the user can turn them off,
replace them or uninstall them. An uninstalled bundled plugin stays
uninstalled, and only untouched copies are upgraded when sheaf ships a newer
version. Debug builds read the staged set from `target/plugins`.

## Testing a plugin

Unit-test your logic on the host: the SDK compiles there (calls into sheaf
panic outside a plugin). Keep pure functions apart from the calls, as the
bundled plugins do, and test what a page's maps do with a `PageInfo` you
make (the bundled plugins test turned pages so). To run a plugin against
real documents, see `crates/sheaf-plugin/tests/host.rs`. It loads
components with fake text and UI sources, and its `Shared` harness stands
in for the app: it holds the document, publishes it as the app does, and
is the call's committer, so a `commit()` lands mid-call and the call's
edits land when it returns, as one undo step, as they would in sheaf.
`FakeUi` answers forms, and can make the user's own edit while a form is
open (`FakeUi::editing`), so a test can show what lands beside a call's
edits and what refuses them.
