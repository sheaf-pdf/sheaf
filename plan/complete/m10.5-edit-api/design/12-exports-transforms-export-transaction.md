# M10.5 §12: Exports: transforms on an export transaction

Part of [M10.5: the design](README.md).


```rust
pub struct Export { pub from: Snapshot, pub transforms: Vec<Transform>, pub profile: Profile, pub out: Out }
pub struct Profile { pub encryption: Encryption /* Keep | None */, pub numbering: Numbering /* Stable | Dense */,
                     pub private: Private /* Keep | Strip */ }
pub enum Out { File { path: PathBuf, overwrite: bool }, Bytes }      // not `Target`: that is a bookmark's (§3.8)
pub fn preflight(snap: &Snapshot, e: &Export) -> Vec<Gate>;      // Proceed | Ask | Refuse
pub fn run(e: Export, asks: &AskAnswers, p: &Progress) -> Result<Exported, ExportError>;  // the ONLY writer besides Save
```

**What `run` does:**
1. Runs the preflight gates (§12.1).
2. Runs the resolve stage, for transforms that need engines. M11's
   partial-image redaction is its first producer; none exists in M10.5.
3. Lowers the transforms on an `ExportTx` over the snapshot, with the
   same lowering, rules and validation as the binder. It uses scratch
   ids and no history.
4. Runs `rules::plan` (target against base, strategy Rewrite) and
   applies its transforms. Every way out follows the form rules.
5. Materializes with `EmitScope::All` and the profile, with progress and
   cancel, and `Deferred` bodies.
6. Verifies (§11.9).
7. Writes atomically with fsync, or returns bytes.

A failure anywhere writes nothing. Exports run as jobs with progress and
cancel, stay out of the History panel and plugin events, and are `Send`.
Generator semantics match the binder: live tokens are evaluated against
the exported document, and frozen Bates numbers keep the binder's
numbers. A print is the exception (Greg, 2026-10-22): its pages are the
document's, so its live tokens read where the document has each page,
the count and the labels the document's (`KeepPages { numbering:
AsDocument }`), and pages 5 and 6 of 10 print "Page 5 of 10" and "Page 6
of 10", as print preview shows them; an extract's say "Page 1 of 2"
(`AsKept`).

#### 12.1 Preflight gates

- **Refuse the binder's own file** as a target, for every export. Today
  only Clean Copy and Export Flattened refuse; `extract_pages` and split
  don't.
- **Ask** before signatures in the exported content stop validating
  (`SigImpact` against the export's Rewrite).
- **Ask `XfaHoldsValues`** when the export drops XFA that holds values.
- **Check capability.**
- **M11** adds unapplied redactions here, as the one choke point for
  "every way out asks first".

#### 12.2 FlattenAnnotations

- It respects Print, Hidden and NoView.
- For widgets it uses `/AS` with **no first-state fallback** (M10 bug
  10). The fallback stays for non-widgets.
- It keeps links.
- It never merges or decodes streams. It adds a `Placements` layer per
  page whose forms are `FormRef::Base` (source appearances) or
  `FormRef::Pinned` (derived appearances with their inputs captured),
  and removes the annotations through the cascade rules.
- Its `removed_annots` go into `keep_only` for OBJR pruning.
- It is both a binder op and an export step, with identical semantics.

#### 12.3 Presets

| Preset | Transforms | Profile |
|---|---|---|
| Extract / Export Pages / Split part | `KeepPages { ids, Headings, labels: true, AsKept }` | Keep (proposal 10), Dense |
| Print / Save as PDF | `KeepPages { Drop, AsDocument }` + (`FlattenAnnotations { Printable }` or `RemoveMarkup`) (+ `FlattenFields` after M10 slice 15) | None, Bytes |
| Export Flattened | `FlattenAnnotations { All }` (+ `FlattenFields`) | Keep (proposal 10) |
| Clean Copy | `RemoveMarkup { keep: links, widgets }` + `RemovePrivateData` | Keep (proposal 10), Strip |
| Page images / text | the Print transforms, realized from bytes in memory | Bytes |
| Remove Blanks (CLI) | `KeepPages(kept)` and `KeepPages(rejects)`, two exports, each `AsKept` | Keep |

**Not exports:**
- **Save As** is a Save (§9.1).
- **Combine** is a binder flow: open the first file, apply one
  transaction, then save (§14.1, §14.2).
- **Scan and images-to-PDF** are `Session::new_empty` plus inserts, then
  a save.
