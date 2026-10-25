# M10.5 §14: Consumers

Part of [M10.5: the design](README.md).


#### 14.1 The app: one door, one refresh

```rust
impl DocView {
    pub fn commit(&mut self, tx: Tx, opts: CommitOpts, cx: &mut Context<Self>) -> Result<Commit, EditError>;
    fn on_change(&mut self, ch: &ChangeSet, cx: &mut Context<Self>);     // the only refresh
}
pub struct ChangeSet {
    pub from: VersionId, pub to: VersionId,
    pub cause: Cause /* Commit | Amend | Undo | Redo | Jump | Saved | Rollback | Revert */,
    pub actor: Actor, pub depth: u8, pub keys: Arc<KeySet>,
    pub order: bool, pub pages_added: Vec<PageId>, pub pages_removed: Vec<PageId>,
    pub geometry: PageSet, pub content: PageSet /* render keys may differ */,
    pub annots: PageSet, pub fields: Vec<FieldId>, pub outline: bool, pub labels: bool, pub form: bool, pub generators: bool,
}
```

A **ChangeSet** says what one transition between versions changed, for
every consumer that has to follow it.

**One door.**
- **The three funnels become one:** `finish_edit`, the hand-written
  `touched()` + `after_model_change` pairs, and OCR's `apply_in_job`.
- **`DocView::commit` returns the error.** It shows the notice
  centrally, and plugin callers get it as `result`.
- **One refresh.** Undo, redo, jump, `record_save`, `rollback_group` and
  `revert_group` return a `ChangeSet` into the same `on_change`, which:
  - prunes selection, region, chars and text selection of removed ids;
  - relayouts only on `order` or `geometry`;
  - lets caches follow render keys;
  - schedules autosave;
  - publishes the new `Snapshot` and `LowerCx` to the document's plugin
    cell (an `ArcSwap`, §14.3);
  - emits one `DocEvent::Changed`.
- **New ids come from `Commit.created`.**

**Background planners.**
- **Concurrent short planners.** `insert_file`, blank and image inserts,
  blank review, comment import and Trim White build a `Tx` off-thread on
  a snapshot and commit with a base (policies in §8.5). They run
  concurrently with each other and with OCR.
- **OCR** keeps its own run, a long streaming producer.
- **One at a time.** `jobs::run_job`'s rule now covers only jobs that
  write files: exports and split. Saves are serialized per document by
  the seal and the second-save wait.
- The `NoProgress` paths and bare `background_executor().spawn` calls in
  page_insert.rs go.

**Dialogs** resolve indexes to ids against the version they opened on,
preview in the background, re-lower the preview on every `ChangeSet`,
and commit with `Base::At(opened)`.

**Menus** use `Tx::check`: no lowering, no ids.

**The clipboard** carries copied marks as JSON in the source page's view
frame. The blobs they reference travel in a process-wide, in-memory blob
cache keyed by `BlobId`. Paste in any window interns them into that
document's Store before lowering `AddMark`.

**Saving:**
1. `save_input(v)` seals v.
2. `plan` runs on a background thread.
3. Ask any `Ask`s.
4. `materialize` runs on a background thread.
5. `record_save`, or `abandon_save`.

Nothing is reopened, no tile is dropped, and edits stay enabled
throughout.

**Combine** builds the combined document (§14.2), saves it to the path
the dialog asked for, and hands **the same `Document`** to a new window.
Its history starts at the combined version (the first file's opened
state is pruned), its notices show there, and `FormDoc.scripts` follows.
Nothing is reopened.

**The `--selftest-*` and `--script` harnesses** keep working through
`commit`. Their `dump` gains `"version"`.

#### 14.2 Open, Session and the CLI

**`sheaf_doc::open::start(bytes, password, store) -> Loading`** gives
the page count and the first page's geometry in O(1), and rendering
starts immediately.

**`open::finish(loading, progress) -> Opened { doc: Document }`** runs
on a job thread and builds the complete root:
- geometry, annotations and the form;
- the outline with styles and closed state, and labels;
- generators from markers;
- the synthesized `SaveRecord`.

Until it finishes:
- the app grays out edit and plugin commands;
- `commit` returns `NotReady`;
- plugin calls are queued until ready, and plugins get `opened` only
  after.

If a file misses the 1 s budget, edits simply wait longer, with progress
shown. There are no late facts (proposal 28).

**One open for the app and headless.** `sheaf_ops::open` (open.rs) is
replaced.

```rust
pub struct Session { pub doc: Document }                       // sheaf-ops: no GPUI; EnginePool only when rendering
impl Session {
    pub fn open(path: &Path, pw: Option<&str>, p: &Progress) -> Result<Session>;   // returns after finish
    pub fn new_empty() -> Session;                              // scan, images-to-pdf, summary
    pub fn intern(&mut self, path: &Path, pw: Option<&str>) -> Result<Blob>;       // pinned by the Session
    pub fn name(&mut self, name: &str, blob: Blob);             // for BlobSel::With in JSON
    pub fn commit(&mut self, ops: Vec<Op>, label: &str) -> Result<Commit, EditError>;  // actor = Cli, base = cursor
    pub fn preview(&self, ops: &[Op]) -> Result<Preview, EditError>;
    pub fn save(&mut self, target: SaveTarget, answers: &AskAnswers, p: &Progress) -> Result<SaveReport, SaveStop>;
    pub fn export(&self, e: Export, answers: &AskAnswers, p: &Progress) -> Result<ExportReport, ExportError>;
}
```

**Combine** is `Session::open(first)` plus one `Tx` (proposal 35):
- `DeletePages` for the first file's unselected pages;
- one `InsertPages { pages }` per further file, with its file name;
- `AddBookmark` per file.

The first file stays the primary, so its catalog keys (`/Lang`,
`/OCProperties`, structure, `/ViewerPreferences`, `/PageMode`,
`/Metadata`, `/OutputIntents`, `/Names`, and the AcroForm's `/XFA`,
`/DR`, `/DA` and `/Perms`) flow through `keep_only` and the form rules
as today. Its encryption is kept by `Profile.encryption = Keep`, subject
to proposal 10. The CLI then saves to `-o`.

**`sheaf edit IN --edits FILE|-`.** Flags:

| Flag | Meaning |
|---|---|
| `--with NAME=PATH[:PASSWORD_ENV]`… | name inputs for `{"with": "NAME"}` |
| `--password PW` | the password for IN |
| `-o OUT [--append]` or `--in-place` | the target; `--append` copies the saved bytes and appends, which keeps signatures |
| `--signatures ask\|break\|append-anyway` | how to answer a signature ask |
| `--xfa ask\|drop` | how to answer `XfaHoldsValues` |
| `--dry-run` | print the preview's lines and consequences and the resolved pages |
| `--force` | **only** "replace an existing output", as in every other subcommand |
| `--schema` | print the schemars schema |

- **The input** is the envelope (§6.1), whose `label` names the
  version.
- **JSON addressing** has no session ids: pages by `{"pos": n}` (pre-op
  state, 1-based), bookmarks by `{"path": [...]}`, annotations by
  `{"nm": …}`, blobs by `{"with": "NAME"}`. M10 adds fields by full
  name. An `Id` ref (a bare number) is `Invalid`: "session ids aren't
  meaningful in a file".
- **Output.** It prints `{version, created, lines, strategy}`, where
  `created` gives positions, `/NM` names and title paths, never session
  ids.
- **Exit codes:**
  - 0, done;
  - 1, failed;
  - 2, usage;
  - **3, a decision is needed** (the `Ask` is printed as JSON on
    stdout);
  - 130, canceled.

**`sheaf inspect IN [--password PW] --json`** prints, for scripts and
M15's authors:
- pages: position, label, boxes, rotate;
- sheaf marks: `/NM`, page, kind;
- others' comments: `/NM`, page;
- bookmarks as title paths;
- generators: kind, scope;
- a form summary.

**Every existing subcommand becomes a thin translator over `Session`:**
combine, comments import (`AddMark`/`PatchMarks` with `AnnotRef::Nm`
and `imported`, §6.8), remove-blanks, scan, images-to-pdf, split, export
and form check.

#### 14.3 Plugins: `sheaf:plugin@0.4.0`, one world

```wit
package sheaf:plugin@0.4.0;
interface edits {
  /// `ops`: the JSON envelope {"v":"1.N","ops":[…]} of sheaf-edit Ops (schema in docs/plugins.md, by schemars).
  /// Lowered on the plugin thread into this call's transaction; returns JSON `Applied { created, lines }`
  /// (ids final once the call commits) or JSON `EditError`.
  submit: func(label: string, ops: string) -> result<string, string>;
  preview: func(ops: string) -> result<string, string>;           // lines, effects in words, caps
  /// Commit this call's transaction now (blocks while the UI thread commits); the version, or a conflict.
  commit: func() -> result<version, string>;
}
interface assets {
  /// Bytes the plugin supplies (a PNG for a stamp, a file to attach): a handle valid for this call only.
  upload: func(bytes: list<u8>, kind: asset-kind) -> result<asset, string>;
}
interface document {
  record version { instance: u64, id: u64 }
  version: func() -> version;                                     // the call's base
  pages: func() -> list<page-info>;          // id, media, crop (PageSpace), rotate (absolute), label, to-view, from-view
  annotations: func(page: page-id) -> list<annot-view>;   // sheaf marks, plus others' comments as read-only summaries;
                                                          // never widgets; author as a role unless `identity`
  …                                                       // outline, selection, text (no field text), render
}
// events: opened (when ready), changed(change-info { version, parent, cause, actor-kind, scopes }), saved, edit-refused
// scopes: { order: bool, geometry: list<page-id>, content: list<page-id>, annots: list<page-id>,
//           outline: bool, labels: bool, form: bool }     — semantic, mapped from ChangeSet; Key stays internal
```

**Ops cross WIT as JSON of the exact Rust type** (proposal 18).
- sheaf-plugin-api re-exports `sheaf_edit::Op`, an `Ops` builder, and
  the `to_view`/`from_view` helpers. There is no hand mirror.
- **Versioning.** The manifest declares `edit-schema = "1.N"`. The host
  refuses a plugin whose minor is newer than its own at load, and the
  submit envelope carries the same version.
- Growth is additive: a new `Op` needs no WIT bump. Only new *read*
  functions bump the minor.

**One transaction per call.**
- **The base.** The host builds the call's `Tx` lazily, at the call's
  first document read or submit, from the document's `ArcSwap` cell. A
  queued call, or a command that first shows a form, therefore starts
  from a fresh base.
- **Reads.** Every WIT read records **decision reads** in the call's Tx:
  - `pages()` → `Order` and each listed page's `Page` and `Geom`;
  - `annotations(p)` → `PageAnnots(p)` and each `Annot`;
  - outline reads → `OutlineRoots` and the bookmarks visited;
  - `text`/`render(p)` → `read_render`.

  Reads see the plugin's own writes.
- **`submit`** lowers on the plugin thread (it needs only
  `Arc<LowerCx>`), returns the allocated ids and validates
  synchronously. Raw blob ids are refused; assets come through `upload`.
- **Committing.** `commit()` (optional) and the end-of-call `Ok` commit
  on the UI thread with `Stale::Relower`:
  - the decision reads are validated by value;
  - the ops are re-lowered on the cursor, with pinned ids;
  - if the re-lowered `created` differs from what `submit` returned, it
    is a Conflict.

  So a user edit to an unrelated page during a call doesn't refuse the
  call, while a change to anything the plugin read does.
- **After a mid-call `commit()`,** the call's next Tx is based on the
  version it committed. The version's label is the first submit's label.
- **On `Err`, a trap or cancel,** the uncommitted Tx is dropped, and
  `rollback_group` truncates or reverts (§8.4).
- **A conflict** surfaces as `commit`'s error or, at the end of the
  call, as a notice plus an `edit-refused` event.
- **One undo step.** A call's commits form one undo step, unless the
  user edits in between (§8.4, proposal 19).

**Reads are "as the file will draw it now"** for content: render and
text come from realizations with `MarkDraw::ByEngine`, unsaved header
and OCR included, with a `without-markup` option. Coordinates are
PageSpace, and the view matrices are provided.

**Privacy in 0.4:**
- `annotations()` never returns widgets.
- Text excludes field display text (`TextOpts { fields: false }`).
- Authors are roles, and identity tokens stay literal (§6.8).
- Renders draw appearances, as they do today. Gating field values in
  renders (`FormDraw::Redacted`) arrives with 0.5's fields capability
  (M10 slice 29).

**Events.**
- **`changed`** is coalesced per plugin by **merging**: the earliest
  `parent`, the latest `version`, the union of scopes, and cause
  `coalesced` when causes differ. A plugin can trust
  `parent == my last version`, or else re-read.
- **No echoes.** A plugin isn't sent its own changes. A causal depth
  stops edits made from `on_event` at depth ≥ 2.
- **No undo fights.** Edits submitted from an `on_event` whose
  triggering change was an Undo, Redo, Jump, Rollback or Revert are
  refused with an `edit-refused` event ("the user undid this"), so an
  event plugin can't fight undo.

**First cut:** `Cap::Plugin` ops. These are today's 13 WIT 0.3 edit
cases (wit/sheaf.wit:242–256: move, rotate, delete and duplicate pages;
the five outline edits; annot add, update and delete; connector add),
plus labels, replies and review states through locals, bookmark style,
page stamps and attachments through uploaded assets, and deleting
**sheaf marks**.
Header/footer, OCR, insert-pages and deleting others' comments come
later, with manifest permissions.

**One world.**
- **Deleted:** `wit/0.2`, `api/v02.rs`, `api/v03.rs` (0.4's host code
  replaces it), the double bindgen and `include!(shared.rs)`, the
  `World` enum and per-plugin world selection, `API_PREVIOUS`, the
  164,420-byte 0.2 fixture wasm,
  `keep_unseen`, `sign_marks`, `edit_source`'s batch loop, and
  `EditBatch`/`CallOutput.batches`.
- The manifest check is exact-minor until alpha.

**The bundled plugins are rebuilt, and their versions bumped:**
- tax-form-bookmarks appends instead of folding in reverse;
- bookmark-from-pattern uses locals;
- amount-crossref and duplicate-pages key `PENDING` by
  `version.instance`;
- subtotal invalidates by `scopes.annots`.

#### 14.4 The agent

Nothing is built for the agent in M10.5. The API leaves room for it to
expand later without reshaping:
- `Actor` is `#[non_exhaustive]`;
- version tokens are `{ instance, version }`;
- commits validate decision reads by value;
- `revert_group` exists;
- Op JSON is versioned.

§20 records the rules for its milestone.
