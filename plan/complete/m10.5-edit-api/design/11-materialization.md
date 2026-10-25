# M10.5 §11: Materialization

Part of [M10.5: the design](README.md).


**Materialization** turns a `DocState` into bytes: a full file, an
appended section, a recovery section or an overlay.

#### 11.1 Stages

```rust
pub fn plan(input: &SaveInput, target: &SaveTarget) -> SavePlan;                    // §9.2: before any write
pub fn emit(st: &DocState, scope: EmitScope, base: &EmitBase, p: &Progress) -> Emission;   // the shared heart (pure)
pub fn rewrite_graph(e: &Emission, primary: &Src) -> Graph;    // primary loaded eagerly + emitted objects at stable numbers
pub fn serialize(e: Emission, g: Option<Graph>, how: How, prof: &Profile, p: &Progress) -> Result<Written>;
pub fn materialize(input: &SaveInput, plan: &SavePlan, target: SaveTarget, p: &Progress) -> Result<Output, SaveStop>;
pub struct Output { pub bytes: Written /* file path or in-memory bytes */, pub record: SaveRecord, pub notes: Vec<Note> }
pub enum SaveStop { Ask(Ask) /* FileChanged at write time; RemovalCheck veto on a signed document */, Failed(Error) }
pub enum EmitScope { All, Keys(Arc<ObjKeySet>) /* Append, recovery: closure under emit_deps */, Pages(Arc<[PageId]>) }
```

**The Rewrite path:**
1. emit;
2. build the graph: the primary loaded eagerly as a lopdf `Document`,
   plus the emitted objects at their stable numbers;
3. run `kept::keep_only` **unchanged**, except that it also returns the
   ids it modified;
4. apply `rules::plan`'s transforms;
5. serialize: objects neither emitted nor modified are raw-copied when
   the §11.6 conditions hold, and everything else is re-serialized.

`keep_only` is ported to a symbolic graph only if slice 9's Rewrite gate
fails on the heavy fixture.

**Bodies are produced while writing.** Built bodies that are large and
derived are `SymObject::Deferred`: generator blocks, appearance streams,
OCR streams and filtered streams. They are produced inside `serialize`,
one object at a time, written and dropped. Peak memory then stays near
the largest object plus `keep_only`'s structural graph, not the sum of
all derived bytes (gated, see Tests, fixtures and gates).

#### 11.2 Module layout: save.rs and incremental.rs taken apart

| Module | Holds | From |
|---|---|---|
| `materialize/plan.rs` | `SavePlan`, strategy, asks, `emit_deps` | save.rs `save_to_path_noting`, `signatures_broken` |
| `materialize/emit/{pages, content, annots, form, outline, catalog}.rs` | the emitter | `build_rewrite`'s loop, `write_page_annots`, `link_connectors`, `write_outline`, forms/merge.rs |
| `materialize/copy.rs` | cross-source closure copy | `copy_object`, `remap`, `is_page_node`, `inherited_attrs` |
| `materialize/graph.rs` | the Rewrite graph | `LoadedSource`, `file::load_source` |
| `materialize/verify.rs` | post-conditions (§11.9) | new |
| `kept.rs` | `keep_only`, unchanged, returning the ids it modified | — |
| `serialize/{object, xref, append, full, crypt, durable}.rs` | one writer | incremental.rs:478–550 (section framing) and :591–683 (`write_object`, `write_dict`, `write_name`), `write_atomic` + fsync |
| `export/{run, presets, transforms}.rs` | §12 | `subset`, `print_pdf`, `flatten_annots`, `export_*` |
| `content/` | §13 | stamps, watermark, text_layer, annotation installers |
| `recovery.rs` | v2 | |

Tests move with their code (save.rs :1736–5061) and are ported to ops.

#### 11.3 The emitter: objects derived from the definition

```rust
pub enum ObjKey {
    Catalog, PagesNode(u32), OutlinesRoot, AcroForm, Info, Names(NameTree), Generators /* /SheafGenerators */,
    Page(PageId),                                // a page dict (copy 0 is written at its own number)
    Base { src: SourceId, obj: ObjectId, copy: u32 },  // a source object (copied when not primary or copy ≥ 1)
    Annot(AnnotId), AnnotPart(AnnotId, Part /* Ap(state) | Companion | Popup | FileSpec */),
    Field(FieldId),                              // a merged widget's ObjKey::Annot ALIASES its terminal's Field key
    Bookmark(OutlineId), Labels,
    Block(u128 /* layout digest */),             // generator, OCR and flattened blocks; identical blocks share
    GenForm(u128), Filtered { stream: ObjRef, filter: u64 },
    PageRes { base_res: Option<ObjRef>, dropped: u64, added: u64 },
    Res(ResId),                                  // content-addressed font, image, form, ExtGState, property list
    Shared(SharedKind /* QOpen | QClose(n) | GlyphlessFont */),
}
pub struct Emission { pub objects: Vec<(ObjKey, SymObject)>, pub version: PdfVersion /* required, §11.7 */,
                      pub removed_annots: Vec<ObjRef>, pub allowed_drops: Vec<ObjKey> /* live redraws */ }
pub enum SymObject { Raw(ObjRef), Built(SymValue) /* refs are ObjKeys */, Deferred(DeferredBody) }
```

**Pages.** `emit/pages.rs` starts from the base page dict, read lazily,
and keeps unknown keys. Then:
- **Boxes and rotation.** `Geom` boxes and `/Rotate` are written only
  where they differ from what the page will inherit in its written
  position.
- **Inheritance.** When a page dict is placed under a node other than
  its source parent chain (Append inserts, duplicates, moves between
  nodes, overlays), all four inheritable keys are written explicitly
  from the page's effective values: `/Resources`, `/MediaBox`,
  `/CropBox` when it differs, and `/Rotate`, written even when it is 0.
- **Contents and resources.** `/Contents` per §11.4, `/Resources` per
  §11.5.
- **`/StructParents`** is kept only on copy 0 of a page whose source
  structure tree is written (the primary's). It is dropped on copies and
  on pages inserted from other files, and `/StructParent` is dropped
  from their annotations, with a Note when the source was tagged. M14
  models structure.
- **`/Annots`** comes from `page_annots`:
  - marks through their derived appearances, with source keys kept
    (§3.6);
  - others' annotations as the source dict, references remapped,
    copy-on-write;
  - widgets the same way. A merged widget is its terminal's one object.
  - **Links.** Links among copied pages are remapped. Links whose target
    page isn't written are dropped, by both strategies; `emit_deps`
    keeps this right across Appends.
- **`/Thumb`** is dropped whenever content changed.
- **Template pages** (`/Names /Templates`) are kept raw by `keep_only`
  on Rewrite and untouched by Append. They count as target pages for
  reachability (§11.9).

**Forms.** `emit/form.rs` is forms/merge.rs generalized:
- **Field dicts** are copied with their changed keys (`/V`, `/AS`,
  `/DA`, `/T`, `/Parent`, `/Kids`, `/P`). A `copy ≥ 1` instance gets its
  own objects.
- **A merged field and widget** (ISO 32000-1 12.7.3.1) is emitted as
  **one** object under `ObjKey::Field`, with the field's and the
  widget's changes combined. Its `ObjKey::Annot` aliases that key in the
  `ObjMap`, so a fill never defines one number twice. A copy of a merged
  pair is still one object, with no `/Parent`, so it is never orphaned.
- **`/DR` and inherited keys.** `/DR` is merged with the renames decided
  at lowering. `/DA` and `/Q` are pushed down. The owned AcroForm keys
  come from `FormDoc`, and document scripts are carried over.

**Outline.** Emitted in place by provenance. Original items keep `/A`,
`/SE` and the sign of `/Count`, and only changed items are written. A
full rebuild happens only for a new tree. `write_outline`'s
always-rebuild goes.

**Labels and generators.** `/PageLabels` comes from the `LabelSpec`s,
and `/SheafGenerators` from the generators (specs only, no paths).

**Connectors.** `/Dest` cycles resolve symbolically after the page keys
exist, so `reserve_pages`' id dance goes.

**Appearances** are derived at emit time by
`appearance(mark | widget | pinned, inputs, font) -> FormRes`. The
function is pure and memoized by input digest. The font is an explicit
input, never a global. M10 slice 7's fallback font is a late-bound
`Res::Font(Fallback)`: one GID-preserving subset per materialization or
overlay.

#### 11.4 Content assembly

```
/Contents [ gen-under… static-under…  QOpen^open  base′…  QClose^close  static-over…  gen-over… ]
```

- **`base′`** is the source page's `/Contents` minus sheaf's own
  isolation streams and blocks (recognized by marker), with any filters
  applied.
  - The original streams are referenced and **never re-encoded**.
  - With a filter, a stream becomes a lexical splice (§13), written
    under a `Filtered` key: copy-on-write, so a stream shared by blank
    pages stays intact on unfiltered pages.
- **The `replaces` rule.** A generator's `replaces` becomes
  `DropMarked(Pagination { kinds, origins: {Acrobat} })` on each covered
  page at emit. It is resolved from the memoized `blocks` fact, so it
  also covers pages inserted later.
- **Isolation** is emitted only when the page has layers or generators.
  The `open` and `close` counts come from `Balance` (§3.4). `QOpen` and
  `QClose(n)` are shared, marked streams, written once per file.
- **Every sheaf block is its own stream** with a `/SheafBlock` dict.
  - Pagination content is wrapped in `/Artifact /ShP<n> BDC … EMC`, with
    a named property list (§3.5).
  - OCR blocks are untagged invisible text wrapped in
    `/ShOCR /ShP<n> BDC … EMC`. Their named property list
    `<< /SheafOcrBlock (b<hex>) >>` lets a merged stream still classify,
    and Strip still be a dict edit. The key differs from the legacy font
    name `/SheafOcr` by more than case.

**The `/SheafBlock` dict, per kind.** Every value but `/K` is a string.

| Kind | `/SheafBlock` | Inside the stream |
|---|---|---|
| Generator | `<< /K /Gen /G (g<hex>) /L <layout hex> /R <recipe hex> /N (000123) >>`; `/N` only with Bates | `/Artifact /ShP<n> BDC … EMC`, property list `<< /Type /Pagination /Subtype … /SheafGen (g<hex>) >>` |
| OCR | `<< /K /Ocr /B (b<hex>) >>`: the layer's `BlockId` | `/ShOCR /ShP<n> BDC … EMC`, property list `<< /SheafOcrBlock (b<hex>) >>` |
| Flattened | `<< /K /Flat /B (b<hex>) >>`: the layer's `BlockId` | the placements, tagged per `PlaceTag` |
| Isolation | `<< /K /Isolate >>` | `q`s (`QOpen`) or `Q`s (`QClose(n)`) |

#### 11.5 Copy-on-write and resources

**The writer never writes a source object's number with different
content,** except in two cases:
- **Model-owned identities:**
  - page dicts (copy 0);
  - annotation, field and outline dicts that came from the file;
  - the Catalog, `/Pages` nodes, AcroForm and Info.
- **Rewrite pruning:** objects `keep_only` edits in place, such as
  structure elements' `/K`, `/ParentTree` nodes and `/Names` subtrees.

`verify.rs` enforces this as a post-condition (§11.9): it compares each
written source number's content against the source, unless its `ObjKey`
is model-owned or `keep_only` reported it.

**Shared objects are never modified in place.**
- Shared streams, Form XObjects, images, fonts and Resources dicts are
  left alone.
- A transformed stream is a new key, memoized by
  `(source object, transform)`.
- "All uses" edits are M13 and M14 work, which adds `Source::users` and
  an in-model users index with its first producer.

**Pages get a `PageRes`,** a new dict built from the base page's
resources.
- **One level down.** It copies the base's entries one level down in
  each category it changes.
- **Added names** are collision-free: `/Sh<base32(ResId)[..6]>`, bumped
  if the base already uses the name.
- **Dropped names.** After filters, the names still used by the
  remaining content are computed (`content/names`). Names used **only**
  inside dropped ranges are omitted. The Form XObject an Acrobat
  watermark draws, and its ExtGState and fonts, are then no longer
  reachable from that page. The unlinked objects go to the removal
  check.
- **Sharing.** Pages that share one base Resources dict, one
  dropped-name set and one added-name set share one `PageRes`. A
  watermark over 7,000 pages that share a Resources dict therefore
  writes **one** Resources dict, one font, one ExtGState and one image.

**Cross-source copy** never follows into a page it isn't copying; that
reference becomes Null. Links among copied pages are kept. Copied
annotations and fields are private to their copy.

#### 11.6 Numbering, raw passthrough, object streams, the page tree

**Rewrite numbering is stable,** for in-place saves and Save As:
- every surviving primary object keeps its number, and removed ones
  become free entries;
- keys already in the previous `SaveRecord.map` keep their numbers;
- new keys are numbered from `size` in a deterministic order: pages in
  order, then annotations by id, fields by id, resources by digest;
- `renumber_objects` is no longer used.

**Raw passthrough.** An object is copied as its original bytes only when
**all** of these hold. Everything else is re-serialized and re-encrypted
under its final `(num, gen)`.
1. The numbering is `Stable`.
2. The object comes from the primary source.
3. The target's `Crypt` equals the source's: the same algorithm, file
   key and `/ID[0]`. For R ≤ 4 the per-object key depends on number and
   generation.
4. The object is untouched by emit and by `keep_only`.
5. The object isn't an object-stream member, and its xref generation
   equals its `n g obj` header.

Under these conditions, a Rewrite at 7,000 pages is mostly memcpy plus
`keep_only`'s structural parse.

**Object streams.**
- Object-stream containers are never raw-copied.
- Their surviving members are re-serialized as **plain objects**,
  encrypted individually when the target is encrypted. That matches
  today, so it is no regression.
- M10.5 writes no object streams, in Rewrite or Append. Packing is M14's
  (§20, proposal 37), with ISO 32000-1 7.5.7's exclusions and the rule
  that strings inside an object stream are not encrypted separately.

**Dense numbering for exports,** with byte-identical stream
deduplication.

**The page tree.**
- **Append** keeps the saved file's `TreeShape` and rewrites changed
  `/Pages` nodes **in place** under their own numbers.
  - A page stays in its node, and inserted pages join their
    predecessor's node.
  - A node splits only once it grows past max(128, its size at open), so
    a flat 7,000-kid tree stays flat.
  - Only nodes whose `/Kids` or `/Count` changed are written, plus the
    page dicts that changed node (with explicit inheritable keys).
  - A reorder in a flat tree writes one object.
- **Rewrite** writes a balanced tree with fanout 32.

#### 11.7 The serializer, encryption and durability

- **`serialize/object.rs`** is the hand-written object writer (lopdf's
  Writer is private). Reals are written in shortest round-trip form. A
  non-finite real is refused, and magnitudes are clamped to ±3.4e38
  (Annex C).
- **`serialize/xref.rs`:**
  - **Full writes** produce one subsection `[0, Size)` with free entries
    linked in a list, or an xref stream with `/Index` when the base used
    xref streams.
  - **Appends** write runs. An object deleted in an Append becomes a
    free entry with generation + 1.
  - **The header offset.** Appended xref offsets are written relative to
    the header offset `h` recorded in `XrefFacts` (position − h), so
    files with leading junk work in PDFium, pdf.js and qpdf.
- **`serialize/append.rs`.** An Append never uses a `Source`'s bytes as
  its base. Its base is **the saved file on disk**, whose identity is
  the `SaveRecord`.
  - **In place.** The section is assembled in memory (Appends are
    small), and the removal check (§11.9) runs on it before any byte
    reaches the file. Then: re-check the stamp; write a journal
    `<name>.pdf.sheaf-append` beside the recovery sidecar, holding
    `{ pre_len, stamp }`, and fsync it and the directory; append the
    section to the file (in the base's xref kind, `/Prev` at the real
    start, a stale `/XRefStm` ignored, `/ID[0]` kept and `/ID[1]`
    updated, `/Encrypt` repeated in the trailer); fsync; remove the
    journal.
  - **A torn tail.** On open, a journal whose file is longer than
    `pre_len` and doesn't end in a complete section is our own torn
    write. The file is truncated back to `pre_len`, with a notice.
  - **`Onto::CopyOf`** reflinks (FICLONE) where the filesystem supports
    it, or else stream-copies and verifies against `content_hash`, to a
    temp file in the target directory. It appends there and renames.
  - **Empty sections.** Nothing changed means nothing is written.
- **`serialize/full.rs`** writes no `/Prev` or `/XRefStm`.
- **`serialize/crypt.rs`** encrypts every written string and stream with
  the target's `Crypt` through `encrypt_object`, including strings in
  stream dicts (lopdf #603).
  - It honors `/EFF` for embedded files.
  - It skips `/Encrypt`, xref streams, and `/Metadata` when
    EncryptMetadata is false.
  - **IVs.** The AES-CBC IV is the first 16 bytes of HMAC-SHA256(file
    key, ObjKey ‖ plaintext). Output stays deterministic, and no IV is
    reused for different plaintexts of one object across saves.
- **Encryption by profile.**
  - `Profile.encryption = Keep` is the default for saves, Save As and
    recovery.
  - `None` is for print and page images.
  - New encryption is M11's first producer.
  - Loads stop stripping `/Encrypt`, which was the root of M11 bug 2.
- **The PDF version.** The emitter accumulates the version its features
  need: ExtGState `/ca` 1.4, xref streams 1.5, AES 1.6, artifact
  subtypes 1.7.
  - A Rewrite writes the header at that version.
  - An Append sets Catalog `/Version` only when it is higher. That is a
    Catalog change, so it adds OTHER (§9.3).
- **`serialize/durable.rs`** writes a full file to a unique temp name
  (`.<name>.<random>.sheaf-tmp`) in the target directory, preserving
  permissions. It fsyncs the file, re-checks the target's stamp
  (aborting to `FileChanged`), renames, and fsyncs the directory.
- **Determinism.** The same `(state, sources, previous map, profile)`
  gives the same bytes. Only ordered maps are used in emission, and the
  IV rule is deterministic.

#### 11.8 Save-time form transforms: policy, not history

`forms/rules.rs` is rewritten as a pure function run at plan time:

```rust
pub fn plan(base: &SourceForm, target: &DocState /* FormDoc + nodes */, path: &PathEffects,
            rec: &SaveRecord, strategy: &Strategy) -> Vec<SaveTransform>;
pub enum SaveTransform { DropXfa { tooltips_to_tu: bool }, DropUr3, FixSigFlags, FillAppearances }
```

- **`DropXfa`** applies when the field tree or values differ from the
  base (after `XfaHoldsValues`, when the datasets hold values).
  - Tooltips are copied onto the **target's** fields, inserted forms
    included.
  - It rides an Append when the XFA datasets hold no value the target
    has replaced. Otherwise it is `TransformNeedsRewrite(DropXfa)`, so
    the old values don't survive in the earlier revision.
- **`DropUr3`** is decided per strategy (proposal 34).
  - **On a Rewrite** it always applies, because UR3's signature covers
    the original bytes.
  - **On an Append** it applies only when the path's `SigClass` isn't
    within UR3's `/TransformParams`: `/Form [/FillIn …]` allows FILL,
    and `/Annots [/Create /Delete /Modify …]` allows ANNOT. Reader
    validates later revisions against those, so a permitted fill or
    comment keeps Reader's rights.
- **`FixSigFlags`** applies only alongside a UR removal, as today. It
  decides from the signed terminals and certification in the **target**
  (signed fields brought in from other files count), never from the
  primary's form alone.
- **`FillAppearances`** creates appearances for valued fields in a
  NeedAppearances file (M10's producer).

**Who applies the transforms.** The emitter applies them to the Catalog
and AcroForm emission on every path: Rewrite, Append (allowed or
"anyway"), recovery sections (§11.10) and exports (§12).
- `SaveRecord.transforms` and `roots` remember what the file already
  lost, so an Append never repeats a drop.
- Undo across such a save restores nothing in the file, because the
  model never held the change. The next save re-applies the policy
  against the base, idempotently.

#### 11.9 Post-conditions on every output: fail closed

**1. The removal check** runs on every Append, and as a test oracle on
every Rewrite.
- **What it examines.** It looks at each object the section overwrites,
  and each subgraph that becomes unlinked. A subgraph is unlinked when
  its references are no longer present in the new versions of the
  objects that held them. Whether some other, untouched object still
  references a target is unknown, so it counts as unlinked: the check
  errs toward a Rewrite.
- **The old side is the saved file itself.** The check opens the target
  through `LazyPdf` over positional reads after the stamp check, and
  reads only the objects it needs.
- **Derived objects are judged on their record inputs.** These are the
  objects sheaf derives: mark and widget appearances, generator blocks,
  OCR blocks, isolation streams and `PageRes`. The check compares their
  text inputs between the saved state and the target: contents, `/RC`,
  struck text, tape rows, widget value and display, recipe text, OCR
  words. This is exact, and needs no font decoding.
- **Objects from a source are judged on decoded text.** For them the
  check builds a `TextBag` from:
  - string objects;
  - content and appearance string operands, decoded through each font's
    encoding or `/ToUnicode` to NFC, as whitespace-normalized word
    multisets;
  - every stream reachable from the unlinked or overwritten objects:
    Form XObjects, CharProcs, `/RC`, JavaScript, XFA, XMP;
  - hashes of image and embedded-file streams.

  An unknown encoding hashes the raw bytes and counts as differing.
- **GID-preserving subsets.** Every font sheaf embeds uses
  `CIDToGIDMap /Identity` with CID = original GID, so encoded strings
  depend only on font and text, and re-subsetting never looks like a
  removal.
- **A veto.** Anything in the old bag that is missing from the new one,
  and not in `Emission.allowed_drops` (live redraws whose `/R` is
  unchanged), vetoes the Append.
  - On a document with any known signature, the save stops at
    `Ask::Signatures { reason: RemovalCheck }`.
  - Otherwise it becomes a Rewrite.
  - `RemovalMisclassified { key }`, an internal diagnostic and not an
    `EditError`, is logged and counted. The corpus run asserts the count
    is zero for derived objects, and reports vetoes on source objects
    for tuning.
- **"Append anyway".** When the user chose it, the check reports instead
  of vetoing what the offer listed. A derived object's find the save's
  path didn't say is still a misclassification, logged and counted: it
  vetoes, and the save asks again, offering the Append with what the
  check found listed too; once that's agreed to, it's reported (slice
  10's finding 4).

**2. Reachability.**
- Every reference resolves.
- The target pages are the page tree plus the pages reachable only
  through the written `/Names /Templates`; no other page is reachable.
- Every `Res` and blob is present.

**3. Forms.** `forms::check` reports no finding the input didn't already
have.

**4. Private data.** With `Private::Strip`, no `private::KEYS` name
appears (§15).

**5. Encryption.** In debug builds, with `Keep`, every string and stream
is encrypted.

**6. Copy-on-write** (§11.5).

#### 11.10 Recovery

**Sidecar v2.** The format is:

```
sheaf-recovery 2\n<append|full>\n<base len>\n<base stamp hex>\n<flags>\n<bytes>
```

- **Append sections** come from the same append serializer, from the
  saved record to the cursor, with `Profile::Recovery`.
  - **No strategy rules.** A section may "remove" by omission, since it
    is never the final file.
  - **Form transforms do apply** (`rules::plan`), so a crash-recovered
    fill of a hybrid form has no stale XFA.
- **Full** is used when base objects are missing, the base is unsound,
  or there is no save record.
- **Flags.** `removes` is set when the saved → cursor path the section
  carries has `REMOVES` or `REPLACES_TEXT`, or a transform that needs a
  Rewrite.
- **Encryption.** The sidecar is encrypted for encrypted documents, so
  owner-only and password files get recovery, and no plaintext is
  written.

**The lifecycle.**
- **Autosave** runs 3 s after the last edit and at least every 30 s. It
  is skipped while the cursor's version token is the one the last
  sidecar wrote, and it waits for an in-flight save.
- **`record_save`** synchronously writes a new sidecar against the new
  record when the cursor differs from the saved version, and deletes the
  sidecar otherwise. A stale sidecar never outlives its base.
- **Restoring** verifies the base's length and stamp. It opens
  `disk base + sections` from memory as a `Memory(Recovered)` source.
  Its synthesized record has the original path and
  `pending_tail = { bytes: sections, removes }` from the sidecar's
  flags. A Full sidecar restores with no usable base for the disk file
  (`NoSaveRecord`), so its first save is a Rewrite.
  - **The first save** appends `pending_tail + new section` to the file
    in place when the tail's `removes` flag is clear, so signed files
    stay appendable.
  - **When `removes` is set,** the plan adds `Why::RecoveredRemovals`: a
    Rewrite, or on a signed document the ask with "Append anyway". A
    crash never leaves removed text readable in an earlier revision
    without asking.
  - **If the plan is a Rewrite,** the Rewrite simply replaces the file.
  - **A second crash** offers the chained sections: the recovered
    document writes its own Append sidecars against the same disk base.
