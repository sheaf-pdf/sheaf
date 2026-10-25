# M10.5 slice 10: Append, the planner and the removal check

Status: done · Part of [M10.5: the edit API](README.md)

**Append, the planner and the removal check** (XL; proposals 9, 23,
32, 33 and 34; bugs 2, 13, 14 and 23).
- **Appending:**
  - in-place Append with the journal;
  - `Onto::CopyOf` for Save As of signed documents;
  - the full `FileStamp` (slice 7's plus the startxref window's
    hash) and `content_hash`.
- **The full planner:**
  - `classify` and `Effects` (§7.1), the planner being their first
    consumer;
  - path effects judged against saved values;
  - the `emit_deps` closure;
  - `present`, `BaseObjectsMissing`, `NoSaveRecord`,
    `TransformNeedsRewrite`;
  - Unchanged attaches the record.
- **Signatures:**
  - `SigImpact` with allow masks, FieldMDP All/Include/Exclude, and
    `/Lock /P`;
  - "Append anyway", with the form transforms in the section;
  - UR3 kept within its TransformParams;
  - the Catalog `/Version` bump on Append.
- **The page tree** is spliced in place, with node splits past
  max(128, size at open).
- **The Append tests ignored in slice 9** are re-enabled.
- **The removal check** runs on every Append:
  - the old side is read from the file on disk;
  - derived objects are judged on their inputs;
  - other objects are judged on decoded text;
  - a signed document asks.
- **Recovery:**
  - Append sections with transforms;
  - recovered documents chained;
  - a restored document appends its `pending_tail` on its first
    save.
- **Deletes:**
  - the always-Rewrite rule and Full-only recovery;
  - slice 9's "signed files ask Rewrite or Cancel": the signature
    ask now offers "Append anyway".
- **Checks:**
  - **Removed text, every feature × both strategies:** the removed
    strings are absent from the decoded bytes (a page, a mark, a
    reply, another tool's comment, a bookmark, replaced mark text, a
    replaced OCR layer, and a re-OCR over a layer saved in an
    earlier session).
  - **Appends after earlier saves:**
    - a mark color change after a Rewrite appends one object under
      the same number;
    - a bookmark rename gives a Rewrite with the old title gone;
    - delete pages, save, undo, save, reopen: `qpdf --check` passes
      and the pages show;
    - undoing to the saved state gives Unchanged and a clean
      document.
  - **Durability:**
    - three consecutive Appends, then a reopen, pass `qpdf --check`;
    - a torn append (killed by a hook) is truncated on the next
      open.
  - **The Append ≡ Rewrite property** (32 cases, default run; over
    `Edit` generators until slice 11 ports it to `Op`), including
    labels i–iii with page 4 moved before page 1, links to pages
    added in later Appends, and a live-token reorder.
  - **The removal check:**
    - a forced misclassification is vetoed and logged;
    - the same veto on a P2 fixture asks and never rewrites.
  - **Recovery:**
    - the sidecar of an encrypted file with an unsaved header
      contains no plaintext;
    - a same-length sidecar against a changed tail is rejected;
    - a restored signed file's first save appends when its sections
      removed nothing, and asks with "Append anyway" when they did.
  - **Signatures:**
    - a P3-certified fixture appends an annotation silently, and a
      P2 fixture asks;
    - a signed file with a deleted comment offers "Append anyway"
      and leaves the signed bytes untouched;
    - a page insert into a signed **hybrid** form offers "Append
      anyway", the section drops `/XFA`, and the signed bytes stay
      intact;
    - a P2-certified, UR3-enabled fixture with `/Annots /Create`
      appends a comment with UR3 and DocMDP byte-intact, and no ask;
    - Save As of a signed file copy-appends, and its byte ranges
      still cover their bytes.
  - **Files:** a change by another program before an Append gives
    `FileChanged`.
  - **Gate:** one mark's incremental save on the 500 MB scan takes
    under 50 ms.

## Notes

Done. Append is back, planned (proposals 9, 23, 32, 33 and 34; bugs
   2, 13, 14 and 23): `materialize::plan` judges a save's path against
   the saved state by value and writes the least that carries it, nothing
   (Unchanged), an Append onto the saved file in place with a journal or
   onto a verified copy of it for Save As of a signed file, or a Rewrite
   for its reasons, strict removal first among them. An Append is the
   path's keys closed over what depends on them, emitted onto the saved
   file through slice 9's emitter and numbered through its record, its
   page tree spliced in place, its IVs keyed by `ObjKey`; before a byte of
   it is written it's checked against the file on disk (the present
   check, then the removal check), and what can't follow the file is a
   Rewrite instead, or a signed document asks. Signatures are judged per
   signature by §9.3's table (`SigImpact`), and a save they don't allow,
   or one strict removal rewrites, offers Append anyway. Recovery
   sidecars are Append sections, chained, encrypted as the file is, and a
   restored document appends them on its first save. One mark's save on
   the 500 MB scanned 7k takes 21 ms, asserted under 50; the Rewrite
   writes the same bytes as before at 1.13× and 1.10× slice 8's writer.
   The always-Rewrite rule, Full-only recovery, `Document.disk_base`,
   number-keyed IVs and slice 9's Rewrite-or-Cancel ask are deleted, and
   every ignored twin runs by default.
   - **Effects** (`effects.rs`, §7.1, step 1). `Flags` (the §7.1 set,
 `bitflags`), `SigClass` (`FILL`, `ANNOT`, `OTHER`), `Effects { flags,
 sig }` and `classify(key, before, after, cx)`, `prim::same` first and
 total over the keys (a value a slot can't hold is every flag), with
 `ClassifyCx::new(saved, target)` holding a lazily built index of the
 target's shown pages and OCR layers. Every §7.1 row that exists today
 has its arm. `judge(saved, target, keys) -> PathEffects { flags, sig,
 removed, annots, fields, changed }` classifies each key once:
 `Removed { kind, count }` by `RemovedKind` (pages, comments, widgets,
 fields, bookmarks, OCR layers, comments' text, field values, field
 names or text, bookmark titles, label prefixes, the header and
 footer, XFA datasets, recovered), `AnnotOps` (create, modify,
 delete, for usage rights), the fields touched by full name (every
 node under one renamed or moved, a changed widget's field), and
 `changed`, the keys whose values differ.
   - **The path** (`SaveInput.path_keys`, `Document::path_keys`): the
 prim keys of every step on the tree path saved → where the branches
 meet → v, both sides; empty with no saved side and in `detached`.
 `prune` keeps the saved and sealed versions and any branch holding
 them, and the root stops at the saved version or the branch point,
 so the path is always in history and `keys_to_root` isn't needed (a
 doc comment says so).
   - **Signatures** (`forms/sigs.rs`, §9.3, steps 1 and 5), read once
 through `Source::signatures()`: each certification's DocMDP `/P`
 (2 when absent, 1 when out of range or unreadable), FieldMDP
 references and a field's `/Lock` (`All`, `Include`, `Exclude`, PDF
 2.0's `/Lock /P`), `/Perms /UR3` or `/UR` with `/Form /FillIn` and
 `/Annots`' create, modify and delete, all within `FormLimits` (a
 reference or lock that can't be read, or past the caps, locks every
 field). `sig_impact(sigs, Pending, class, fields) -> Vec<SigImpact {
 signature, verdict }>`, `Verdict { Allowed, Noted, Flagged, Invalid }`
 with `asks()` (`/P` 1 allows an Append of nothing, the review), and `UsageRights::allow(class, ops)`. Fixture:
 `certify(field, p)`, `usage_rights_for(form, annots)` and
 `sign_locking(field, action, fields, p)`.
   - **The planner** (`plan.rs`, step 5), in order: `CopyOf` for another
 path when the primary has signatures or usage rights, they hold and
 the saved stamp holds, else `OtherTarget`; `NoSaveRecord` (no record
 that knows its file); `FileChanged` with its ask; Unchanged (the same
 file, nothing found yet, `path.changed` empty, no pending tail),
 before `BaseUnsound` (`SaveRecord::takes_updates`) and
 `LegacyHeaderFooter`; `RecoveredRemovals`; `Removes` and
 `ReplacesText` (split by `RemovedKind::is_text`);
 `TransformNeedsRewrite` (DropXfa while the datasets hold values the
 path replaced); else `Append { onto }`. `Why` has a reader's
 `Display` (§7.3) and `SaveReport.why` carries it; the app's notice
 reads "Saved X (Rewrite: …)" or "Nothing to save". `rules::plan`
 takes `(src, target, primary, rec, strategy, class, ops)`: DropUr3
 always on a Rewrite, on an Append only when the rights refuse it;
 the Append's class counts `OTHER` for any transform and for a
 `/Version` bump (`emit::content::needs`/`needs_after`, shared with
 the emitter).
   - **Signature asks and Append anyway.** `Ask::Signatures { impact,
 append_anyway: Option<AppendAnyway { stays_readable: Vec<Removed> }>,
 reason }` while the record's signatures hold (`signatures_hold`: true
 when opened, false after a Rewrite, inherited by everything else; a
 document with no record counts as holding). Append anyway is offered
 on an Append the signatures don't allow and on a Rewrite whose every
 reason is `Removes`, `ReplacesText`, `TransformNeedsRewrite`,
 `RemovalCheck` or `RecoveredRemovals`, never for `BaseUnsound`,
 `BaseObjectsMissing`, `FileChanged`, `NoSaveRecord`, `OtherTarget`
 or the legacy header. `SavePlan::answered(input, target, answers)`:
 an Append or Unchanged plan answered Overwrite is
 `Rewrite { [FileChanged] }`, which asks while signatures hold, with
 no Append anyway; a Rewrite offering Append anyway, taken, is that
 Append (Saved or CopyOf); an Append the signatures don't allow,
 answered `break_signatures`, is `Rewrite { [Chosen] }`. The removal
 check reports only when the offer taken said something stays
 readable (`AskAnswers::take`, `stays_readable`, the review's finding
 8): an Append anyway to the signatures alone still stops on removed
 text and asks again.
   - **The closure** (`deps.rs`, steps 2–6): every path key whose value
 differs, closed over §7.2's table and what the property found
 missing from it (the module doc's table): links re-judged when the
 page written for their target changed (`emit::written_pages` over
 both states), bookmarks to a page now shown as another, the pages
 of a widget that stops or starts being its field's object, the
 catalog for scoped header pages and for labels that come or go, the
 AcroForm only when a field node came, went, moved or shows other
 widgets, the roots a transform the file hasn't had needs (and an XFA
 copy policy keeps again), and the splice's changed nodes and moved
 pages; and the sources whose pages are written otherwise
 (`relinked`), whose objects the file has are copied again (the
 review). `Closure { keys, parents, tree, form_shift, relinked }`.
   - **The emitter's Append** (`EmitScope::Keys`, `EmitBase::Saved {
 record, parents, tree }`, step 2): out-of-scope keys are placeholders
 resolved through the map, the primary's own number, or an owned key
 at a present number; Res, Block and Base keys the map has are
 referred to (`push_once`, `Closure::saved`); a clean-up pass
 (`reached`) drops what only left-out objects reached. The catalog is
 forced in for a later version (its `/Version` the later of the
 file's and the section's whenever it's written), for a form or
 outline that came or went, or roots without numbers of their own.
 The `All`/`Rewrite` path is unchanged: the corpus identity and
 four-times runs hold byte for byte.
   - **The spliced tree** (`emit/tree.rs`): pages keep their nodes, an
 inserted or moved page joins its predecessor's, a node past
 max(128, size at open) splits (the root into nodes under it), an
 emptied node leaves its parent, and node keys are found by number
 through the map, new ones past the highest, retired ones never
 reused. `tree::parents` answers without splicing when the order is
 unchanged and no page shows another page than it did (the review). `TreeNode.from` says which node's keys a node holds.
   - **Numbering and the section** (`number::appended`,
 `serialize::append::section`): map keys at their numbers, owned
 primary objects at theirs, the rest past every number the file uses
 (its cross-reference streams' too), the section's own stream after
 them; `section(base, &[(ObjectId, ObjKey, Object)], xref_number,
 crypt)` encrypts through `indirect_as`. An update frees nothing.
   - **The present check** (`append::append`, steps 2–4, 6):
 `verify::section` (copy-on-write, a primary object at its number
 only over itself, the widget merged with it now, keys moved
 elsewhere or records the target no longer has (`Moved`, built once a
 section, the review), every reference resolved), then
 `Refused::{Unsound, Missing(NotInFile), FormShift, Held, Removes,
 Unplanned}`; `Refused::why()` maps them to
 `BaseUnsound`, `BaseObjectsMissing` and `RemovalCheck { key }`, and
 `Unplanned` (Veto mode, signed fields' signatures holding, the
 section writing the catalog or the form's dictionary) to the ask an
 Append they don't allow makes. `write_file` turns a refusal into
 `plan::fallback`'s Rewrite with the Rewrite's asks; `Written` gains
 `strategy`, `wrote` and `record_in`. An Append's form check is
 `forms::check_latest` (what the newest revision reaches).
   - **The removal check** (`materialize/removal.rs`, step 4): the old
 side is a `LazyPdf` over the file on disk with the primary's
 password; it examines every number the section writes over, then
 what's unlinked (not `/Parent`, `/P`, `/IRT`; a page or node only as
 a kid; the catalog never). Derived objects are judged on record
 inputs: a saved mark's objects on `says()`, a block reached from a
 map-keyed `Page` on its OCR words and header text; Res, ResPart and
 Shared say nothing. Source objects are judged as `TextBag`s (strings;
 content through its fonts for pages with inherited resources, forms,
 tiling patterns and Type 3 CharProcs; `/JS`, `/RC`, `/XFA` and XMP as
 words; images and embedded files by hash), the old bag within the
 bag of everything written. Unreadable counts as removed. A derived
 veto logs `RemovalMisclassified { key }`; derived and source vetoes
 are counted apart (`removal::vetoes()`). `Mode::{Veto, Anyway,
 Report, Recovery}`. `TextBag` hashes the raw bytes of a string with a code
 its font doesn't map, or shown in no font (`Decoder::text ->
 Option`). `removal::of_rewrite` (fixtures) runs it as a test oracle
 on a Rewrite (step 8): the corpus's page deletes, the removed-text
 suite's Rewrites and the property's planned Rewrites (the review).
   - **In place, with a journal** (`serialize::append`, step 3):
 `append_in_place(path, base, section)` re-checks the stamp, keeping
 the tail it read, writes and syncs `<name>.pdf.sheaf-append`
 (`{pre_len, stamp, start}`) and its directory, opens to append,
 checks `pre_len`, writes and syncs, removes the journal and syncs
 the directory; the new stamp comes from the bytes written; an I/O
 failure cuts back at once. The review: the journal is written whole
 under a temporary name, locked (`File::lock`) and renamed into place,
 held until it goes, beside the file a link names (`durable::resolve`),
 and the new stamp's time is read after the file is closed. `undo_torn(path)` (in `open::file`,
 `Opened.torn`, and the app's `DocView::open` before
 `recovery::find`, with a `Torn` notice) cuts a torn append back and
 restores the time, drops a complete one's journal, leaves a shorter
 or changed file. A test hook tears the next append (`tear_next`).
   - **CopyOf** (`durable::copy_appended`): a streamed copy hashed as it
 goes (`sheaf_core::Checksum`, xxh3-128 by twox-hash's `RawHasher`
 with its static secret, no alloc feature), compared with
 `content_hash` when known, the saved stamp checked before and just
 before the rename, the copy's time read once it's in place.
   - **The full stamp and the records** (step 3): `FileStamp.window`, the
 xxh3-128 of up to 4 KiB at the last `startxref`'s target, cut at
 EOF, read by position when it's past the 64 KiB tail
 (`FileStamp::ending` behind `of`, `read` and `holds`;
 `FileStamp::hex`/`from_hex`, 96 hex digits); `SaveRecord` gains
 `content_hash`, `sound`, `signatures_hold`, `pdf_version` and
 `pending_tail`, `OutputRecord` `sound`, `signatures_hold` and
 `pdf_version`, `RootValues` `info`.
   - **Unchanged** (steps 3, 5): `write_file` writes nothing and gives the
 saved record for the version; `Document::record_unchanged` attaches
 it with no save note; `SaveReport.strategy` is
 `Option<StrategyKind>`.
   - **Recovery** (`recovery.rs`, step 7): format 2's `<append|full>`;
 `write(input, file, base, p)` makes an Append section through
 `append::append` in `Mode::Recovery` (nothing stops it, the signatures
 guard skipped), Keep, `plan::sidecar`'s transforms; flags `removes`,
 `fill`, `annot`, `other` and `field:<hex>`; Full when there's no
 record, it can't take an update, the legacy header is on or the
 section is refused or fails (the review); chained for a restored
 document; no sidecar for a
 section that changes nothing. `restore` returns `(SourceInfo, Came)`,
 `Came` moved into `materialize` (`Read(FileStamp)`, `Restored { base,
 pending }`), and the restored record is `SaveRecord::restored` with
 `PendingTail { bytes: Option<Arc<[u8]>>, flags: TailFlags { removes,
 sig, fields } }`; `append::append` joins the tail itself, so its
 output is tail then section. `is_dirty`/`VersionView.saved` follow a
 private `on_disk()`. The password autosave skip is gone.
   - **The app** (docview.rs): the Signatures prompt is Append Anyway (its
 detail says what stays readable), "Rewrite (breaks n signatures)" and
 Cancel; the save notice says why a save rewrote, or "Nothing to
 save"; a torn append's notice on open; the app's `Came` went to
 sheaf-doc.
   - **The command line** saves as the planner decides
 (`comments import`'s answers unchanged): a signed input saved
 elsewhere copy-appends when its signatures allow the change.
   - **Tests and the harness.** `appended(&Document)`,
 `How::write(&Document)`, `save_appended` (`plan::forced_append`,
 Append anyway, stopping on a refusal), `save_rewritten` (`plan::forced_rewrite`, also
 `recovery::write`'s until step 7), `Document::append_input` and
 `opened_from` for documents made in memory; sheaf-ops'
 `common::planned`, and `rewritten_and_saved` asserting a planned
 Rewrite. The Append ≡ Rewrite property
 (`materialize/append_rewrite_tests.rs`, `Run::over` over history's
 `Run`), the removed-text suite in kept/tests.rs, `plan_tests.rs`,
 `removal_tests.rs`, `append_tests.rs` and sheaf-ops'
 `tests/append_corpus.rs` (step 8).
   - **Deleted,** each confirmed dead by `cargo check` and clippy:
 `Why::AlwaysRewrite` and the every-save-a-Rewrite rule,
 `plan::signatures_broken`, the plan's `broken` rule and `policy`,
 `Ask::Signatures`' count, the app's Rewrite-or-Cancel prompt;
 `serialize::indirect` and `Crypt::encrypt`; `recovery::stamp_hex`
 and `parse_stamp` (now `FileStamp::hex`/`from_hex`); Full-only
 recovery, `Document.disk_base`, `set_disk_base`, `mark_unsaved` and
 the password autosave skip; `section`'s `deleted` parameter;
 `materialize_then`'s "Append comes in slice 10" and "Unchanged …"
 stops; every `#[ignore = "slice 10"]`; the 7k harness's
 `appended_one_mark`, its whole-file write and skipped rows
 (`Row::skipped`).
   - **Ledgers:** retired.rs gains eleven rows (`serialize::indirect`,
 `serialize::crypt::Crypt::encrypt`, `recovery::{stamp_hex,
 parse_stamp}`, `materialize::plan::Why::AlwaysRewrite`,
 `materialize::plan::signatures_broken`,
 `materialize::plan::Ask::Signatures::signatures`,
 `model::Document::{disk_base, set_disk_base, mark_unsaved}`,
 `serialize::xref::Entry::deleted`) and `QUALIFIED_ONLY` six names
 (`Removed`, `Widgets`, `indirect`, `encrypt`, `signatures`,
 `deleted`); its walker names a variant's field as
 `Type::Variant::field`, with a self-test, so the count field's row
 has teeth. The transitional ledger marks the IV, every-save-a-Rewrite,
 Full-only recovery and twins rows deleted (the harness kept, below),
 the FileStamp row's slice 10 part done and the `materialize::save`
 row's copy-append deviation, and adds the form-instance-shift
 refusal (10→11), Full sidecars while the legacy header is on (10→17)
 and §9.3's conservative mapping (10→the Acrobat cross-check). No lint
 ban lands.
   - **Docs:** `docs/document-model.md` gains "Appends, the planner and
 the removal check (M10.5 slice 10)" after "Saves are outputs"
 (effects, the path, the plan and its reasons, signatures and Append
 anyway, answers, the closure, the emitter's Append, the spliced
 tree, numbering, the present and removal checks, in place with the
 journal, CopyOf, Unchanged, the records, recovery sections, the
 command line, the tests, what went); Saving's flow and its Append
 paragraph, One writer (no deletes, ObjKey IVs, the full stamp and
 `content_hash`), slice 9's "until slice 10" lines, the Core idea's
 `Document`, Opening (a torn append undone first) and Undo/redo (a
 save judged by its path, Unchanged, a pending tail's dirtiness) say
 what changed. The README's save-model row and crash recovery
 describe planned Appends, Append anyway and Append sidecars.
   - **The 7k harness:** the incremental rows are one mark saved as the
 app saves it (`planned_and_put_back`: `save_input` → `plan`, asserted
 Append onto the file → `write_file`, asserted written as an Append →
 `abandon_save`, the file cut back by `set_len` and its time
 restored), the median of `INCREMENTAL_RUNS` (9), its section made in
 memory beside it (`section_in_memory`: `append::append` over the
 file, every check) and the rest, writing in place, as a time and a
 share; the scanned median asserted under `INCREMENTAL_GATE_MS` (50).
 The AES-256 rows do the same on the encrypted copy. The recovery
 sidecar row is one mark's Append section, its size in KB.

   Deviations from the plan (the planner's list first, as resolved):
   - **The path has no anchor:** `Version` has no `keys`, so `path_keys`
 is built in `Document::input(v)` from the steps on the tree path
 through where the branches meet; with pruning keeping the saved
 version, no `keys_to_root`.
   - **Records' facts the design leaves implicit:**
 `SaveRecord.pdf_version` (the file's version, for the `/Version`
 bump; `version` is the VersionId), `sound`, and `signatures_hold`,
 which replaces plan.rs's `broken` rule (wrong after a Rewrite then
 an Append). The
 literals build from `..SaveRecord::opened`.
   - **`Removed` is a kind and a count** (`RemovedKind`), defined here;
 `stays_readable` takes the path's `removed`, plus `XfaDatasets` when
 DropXfa applies and `Recovered` ("what the recovered changes
 removed") for a tail.
   - **`Why`** gained `BaseUnsound`, `Removes`, `ReplacesText`,
 `BaseObjectsMissing`, `TransformNeedsRewrite` (carrying the
 `SaveTransform`; only DropXfa makes one), `RemovalCheck { key }`,
 `RecoveredRemovals`, `FileChanged` (never offered Append anyway: an
 update can't follow bytes sheaf didn't write) and `Chosen` (the
 Rewrite a reader picks over an Append the signatures don't allow,
 and the answer to the write-time guard), not in the design.
   - **`SavePlan::answered` takes `(input, target, answers)`:** the
 Signatures ask needs the primary's signatures and
 `signatures_hold`, and Append anyway needs the target for Saved or
 CopyOf; it turns an Unchanged plan answered Overwrite into a Rewrite
 too, since writing nothing over another program's file while
 marking the document clean would be wrong.
   - **BaseObjectsMissing is found at write time:** the base objects an
 Append references are known only once its closure is emitted, so
 `append::append` checks after emitting, before a byte is written;
 unsigned documents rewrite, signed ones ask (Append anyway never
 offered). `BaseUnsound` is a write-time fallback too (a rebuilt
 cross-reference table, an unsound record, a tree that can't be
 spliced), and planned up front through `takes_updates`.
   - **A form instance's index shift** (`FormCopies`, until slice 11) is
 refused as `Refused::FormShift` (BaseObjectsMissing), a ledger row.
 The plan's scenario (a form inserted twice, the first instance
 deleted, saved and undone) can't shift it: deleting a page keeps its
 fields until slice 11's cascade, so the test registers one file
 twice and inserts the second registration first; the literal
 scenario found the `deps::field` bug below.
   - **A removal-check veto on a signed document offers Append anyway**
 (review finding append-anyway-narrowed), against §9.2's "Rewrite or
 Cancel", saying what stays readable: the path's removals and
 `RemovedKind::Found` ("removed text the removal check found"), so the
 prompt names it (the review's findings 1 and 10).
   - **The P2 + UR3 `/Annots /Create` Check, conservatively:** §9.3 lets
 P2 allow only filling in, so a P2 + UR3 `/Form /FillIn` fill appends
 with no ask, UR3 and DocMDP byte-intact; a P3 + UR3 `/Annots
 /Create` comment appends with no ask; a P2 + UR3 comment asks.
 Pending Greg's Acrobat check (a ledger row).
   - **`EmitBase::Saved { record, parents, tree }`** rather than
 `Saved(&SaveRecord)`, the Copy/Eq derives dropped; `append` takes
 `(input, plan, on_disk: Arc<dyn ByteSource>, mode, p)`, the record
 from `input.saved`, the file's bytes for its length, last byte, form
 check and read-back.
   - **Node keys by number, `TreeNode.from` added:** a node rewritten or
 split keeps the primary node's inheritable keys it held (`/Rotate`,
 boxes, `/Resources`), which a key alone would lose.
   - **The closure's extras:** the catalog for a version bump is decided
 after emission (the version needed is known only then); the AcroForm
 for a field change only when the node came, went, moved or shows
 other widgets (step 2 had it on every field change, which would make
 every fill on a P2 file trip the write-time guard); bookmarks: no
 rule for a `/Dest` page's written-ness in step 2 (a state invariant
 retargets a bookmark whose page goes), then step 6's
 `deps::retargeted` for a page that now shows another.
   - **`indirect` and `Crypt::encrypt` deleted;** `indirect` and
 `encrypt` are `QUALIFIED_ONLY` (`object::indirect` and lopdf's
 `Document::encrypt` remain).
   - **Appends write no deletes** (lopdf's readers ignore free entries,
 slice 7): an unlinked object stays in use, its number never reused,
 and the removal check judges it. `section`'s `deleted` went, and
 with the review `xref::Entry::deleted` (its xref tests write
 `Entry::Free` themselves). This closes slice
 7's "free entries before Append deletes" without a reader change, a
 deviation from §11.7.
   - **The startxref window** hashes up to 4 KiB at the number as written,
 taken as an absolute position (a header offset shifts the real
 target; every stamp applies the same rule, so it's still a
 deterministic function of the bytes); the new stamp after an Append
 comes from the old tail and the section.
   - **CopyOf** re-checks the saved stamp after the copy, append and sync,
 just before the rename (stricter than between copy and append), and
 the copy's length must equal the stamp's; a CopyOf of an opened file
 learns its checksum from the copy, which the stamps alone verify.
   - **The harness moved to `&Document`;** a document made in memory gets
 a record as opened from its primary's bytes (`append_input`,
 `opened_from`).
   - **`save_rewritten`** keeps the both-ways Rewrite halves Rewrites,
 and about 50 strategy assertions were judged one by one: saves that
 only add assert Append (`undo_across_saves_is_exact`, forms_merge's
 second save, the OCR layer test), saves after a removal assert the
 planned Rewrite with its exact reasons, tests meant to rewrite first
 use `save_rewritten`.
   - **Ledger row 5600's harness stays:** `both_ways!`, `How`,
 `appended`, `save_appended` and `common::appended` are the
 both-strategies harness the Checks need; only the ignores and the
 "until slice 10" wording went, and nothing of them is retired.
   - **Recovery's representation:** a restored document always has a
 record: `SaveRecord::restored`, `pending_tail.bytes` the sections, or
 None for a Full restore (`knows_file()` false: `NoSaveRecord`,
 `takes_updates()` false as a backstop); `signatures_hold` stays
 true, so a Rewrite of a signed restore still asks.
   - **Torn tails** are undone by `undo_torn` before `SourceInfo::read`
 in both `open::file` and the app's `DocView::open`. The journal
 holds the update's own `startxref`, so a complete file is told from
 a torn one without knowing the header offset, and a later append by
 another program is left alone.
   - **The 7k harness** times `save_input` → `plan` → `write_file` →
 `abandon_save`, the file put back by `set_len` and `set_modified`
 (built in step 5 for the informational row, made the gate in step
 8).
   - **Deferred, as planned:** `LIVE_REDRAW`, `allowed_drops` and
 `SaveRecord.drawn` (16–17); `Effects.pages`/`keys` and `Step.effects`
 (11–13). The header over a reorder stands for the live-token case, a
 LegacyHeaderFooter Rewrite until 17.
   - **An unreadable signature** is an approval signature judged as
 `/P` 1: it always asks (1263925; also a `/V` that can't be read, a
 `/V` dictionary without `/ByteRange`, a damaged form whose SigFlags
 says signatures exist).
   - **TextBag says when it fell back:** each string in codes its font
 doesn't map (or shown in no font, a TrueType or Type 3 font with no
 encoding, a Type0 font without ToUnicode) is its raw bytes' hash, so
 only the same bytes match it; slice 6's "bytes as they are" changed
 (`words_end_where_the_text_moves` now uses a font).
   - **The command line copy-appends** a signed input saved to another
 path when its signatures allow the change, rewriting (`Chosen`) when
 they don't, against the CLI table's `-o` (Rewrite) until slice 19's
 flags; `comments.rs`' comment and the `materialize::save` ledger row
 say so.
   - **forms_saves' `a_save_that_would_rewrite_a_signed_file_asks`:**
 after a Rewrite to a copy, saving over the original signed file no
 longer asks (`signatures_hold` is false) and rewrites with
 `[OtherTarget]`.
   - **`StrategyKind` has no Unchanged:** `SaveReport.strategy` is
 `Option<StrategyKind>` (None wrote nothing) and
 `Document::record_unchanged` attaches the record with no note, since
 `record_save` alone can't tell an Unchanged copy from a save.

   Deviations found while building:
   - **Effects (step 1):** `PathEffects.annots: AnnotOps`, since UR3
 grants create, modify and delete apart, which `SigClass::ANNOT` can't
 say; a field rename gives FIELD_TREE | REPLACES_TEXT (the conservative
 reading of §7.1's comment on `/T`), and a tooltip, items, scripts or
 a non-empty `/DV` replaced do too; a Page key None→Some or Some→None
 gives PAGE_ADD or PAGE_REMOVE (the table derives them only from
 `Order`), and REMOVES (Pages) when no target page shows the source
 page (a deleted duplicate doesn't); any layer difference is CONTENT,
 and REMOVES (plus REPLACES_TEXT for OCR) when no target page shows
 the old layer; a removed widget is FIELD_TREE | REMOVES (Widgets); a
 generator added is CONTENT, changed or removed REMOVES |
 REPLACES_TEXT ("drawn in the saved file" read as "present in the
 saved state" until 17's `drawn`). `Removed` is `QUALIFIED_ONLY`
 (slice 8 retired `prim::Val::Removed`).
   - **Signatures (step 1):** approval signatures with no MDP allow
 FILL | ANNOT (§9.3's "Append ⊆ allowed" row); FieldMDP matching is
 conservative (an included name locks the nodes over and under it, an
 excluded one frees only itself and those under it); a certification
 without a DocMDP reference is P 1, a DocMDP reference without
 `/Perms` still certifies, and a `/V` that resolves to no dictionary
 signs nothing (13 corpus forms pass an empty-string `/V` down to
 their signature fields; the first draft counted them unreadable).
 `usage_rights_for` ORs AppendOnly into `/SigFlags`.
   - **Emission (step 2):** out-of-scope annotations on a visited page are
 built and left out, so `/Annots` matches a Rewrite's; the form and
 outline are walked whole when any of their keys is in scope; an
 Append's Raw primary objects are written as the primary has them,
 each reference checked against the file; `number::appended` gives a
 widget its own number when the file had it merged with its field
 and it's written apart now. `RootValues.info` keeps the trailer's
 `/Info` out of the map: listing it there dropped the corpus identity
 counts to 2,265,344 and 917,488 (the test leaves mapped numbers
 out), the bytes the same. Every page an Append writes carries
 `/CropBox` (its media box when it has none), since a saved node may
 crop: PDFium cropped a page moved under a cropping node (§11.3 says
 "when it differs").
   - **What the twins found (step 3), in the Append emitter only:** a
 link to a page not written, or to null, is left out, as `keep_only`
 drops it; a reply's parent that is another tool's comment without
 `/NM` is put in scope, since it takes the name; marks the open gave
 fresh ids are written by the first Append; an attachment refers to
 the file's embedded stream when it holds the same bytes; a page whose
 turn didn't change keeps its own `/Rotate` (135 stays 135). An
 Append's form check is `forms::check_latest`, since an Append
 anyway's unlinked widgets stay in the file (an OffPage finding of
 the earlier revision). Fixtures and expectations changed with them:
 `outline_tree` counts only items that show; `appends_onto` and
 `bookmark_positions` take their bookmarks from a file;
 `deleted_comments_stay_out`'s forced Append is refused and the
 planned save writes them; `usage_rights_go` expects UR3 dropped (a
 turn is OTHER); the encrypted render twin has its own scratch dir
 (the halves raced on one); `a_cancel_and_an_append_stop_the_write`
 became `a_cancel_stops_the_write`.
   - **Writes (step 3):** a Rewrite's `content_hash` is taken in
 `write_file` over the output just before `durable::write`, outside
 the gate's `materialize` timer (about 2 ms on the heavy's 17.9 MB);
 `undo_torn` restores the file's time after cutting it, so the
 pre-append stamp and a sidecar written against it hold again; an I/O
 error while appending cuts the file back at once; a journal that
 can't be removed after a complete write is only logged; if
 `undo_torn` fails, `open::file` logs it and reads the file as it is.
 `append_in_place` (the test writer of step 2) went;
 `serialize::fixture::edited` returns the Document; an in-place
 Append requires `target.same_file`.
   - **The closure's `field()` returned early** whenever `Field(f)` was in
 the key set, which a widget's change puts there first, so the
 AcroForm and the ancestors were skipped: deleting a form page,
 saving by a Rewrite and undoing wrote the field back unlisted and
 `verify::form` failed the save. It has its own visited set (step 4,
 `a_field_whose_widget_comes_back_is_written_with_the_form`).
   - **The removal check, as built (step 4):** `linked` is every number
 the section writes or refers to, plus the trailer's catalog and
 `/Info`, broader than per-object unlinking, so an object moved into
 another written one isn't counted; derived marks are judged by bag
 inclusion of `says()`, written independently of classify, and what
 a mark's objects reach isn't followed; a block is judged as derived
 only when reached from a map-keyed page; widget appearances are
 source objects (the emitter copies them); another tool's comment
 made into a sheaf mark is a source object, whose old `/M` and other
 strings veto (fails closed, as the design accepts); `materialize()`
 with an Append plan vetoes as `SaveStop::Failed` (only `write_file`
 falls back), and nothing calls it so. `AskAnswers.append_anyway`
 came in step 4, its Report mode needing an answer.
   - **The planner (step 5):** `OtherTarget` is a reason only when the
 target can't be a copy (a Save As of a signed document stopped by
 removals is a Rewrite without it, so Append anyway onto the copy is
 still offered; `fallback` no longer adds it); CopyOf counts usage
 rights as a signature (`Signatures::is_empty`), so Save As of a
 UR3-only form keeps its rights when the change is within them;
 Removes and ReplacesText split by `is_text`, no empty `Removes([])`
 beside a text-only REMOVES; the `/Version` bump is judged over every
 target page (`needs_after`, free unless the record's version is
 below 1.2); the test hook is on `judge` (`effects::misjudged`); the
 write-time guard needs no stored class (a Veto-mode Append plan on a
 file whose signatures hold has, by construction, no Signatures ask)
 and covers signed fields only, not a UR3-only file.
 `forced_rewrite` is public (`recovery::write` used it until step 7).
   - **DropXfa on an unsigned hybrid form's Append is vetoed** (§11.9):
 the XFA streams' text is unlinked, and `allowed_drops` holds only
 live redraws, so the save is `Rewrite { [RemovalCheck { AcroForm }] }`
 (forms_saves' `a_form_brought_into_a_hybrid_one_is_saved_in_full`)
 and a signed one asks. Kept, failing closed; a decision for Greg
 (below), as is the same for DropUr3, which the corpus shows.
   - **The property's fixes (step 6),** each a named
 `append_rewrite_*` case that fails with its fix off: links re-judged
 by `emit::written_pages` (`emit::shown` takes `(st, sources)`, and
 `written_pages` came out of `emit()`, the Rewrite path doing the same
 work); bookmarks to a page shown as another (`deps::retargeted`);
 `number::stable` on the Rewrite path giving up a previous number that
 is a primary object it no longer is, unless a root (Rewrite, rebase,
 Rewrite failed `verify::graph`; `in_use` before numbering,
 `ObjKey::is_root`; the corpus bytes unchanged); the pages of a
 widget whose merge with its field changes; `verify::section`'s
 `moved_from`; the file's transforms plus the plan's for an Append;
 an XFA copy policy keeps again comes back (`rules::drops_xfa`), or
 the Append is refused when a Rewrite pruned its streams; a reorder
 writes the catalog for chosen header pages; labels that come or go
 write it; `Refused::Held` (a primary object that can't sit at its
 number, or whose number the file gives another record); and
 `model::adopt` (a mark made from another tool's comment on a copied
 page no longer takes the original's object).
   - **The property, as built:** in `append_rewrite_tests.rs` (every name
 holds `append_rewrite`), its forced Append through `write_file` with
 `forced_append` in Append anyway mode (falling back as the app does),
 comparing more than the plan lists (resources by value to depth 8,
 decoded content, every annotation and field property, the form's
 `/CO`, `/DA`, `/Q`, NeedAppearances, SigFlags and XFA state), over a
 six-page form whose XFA part each case decides; history_tests'
 `Run`, `Run::act`, `Run.d` and `TRIM` are `pub(crate)`, `Run::over`
 builds one over a file, and `forms::fixture::every_kind_in(pages)`
 came. The regression seeds were removed: each real failure is a
 named case, and the one left was from fault injection.
   - **Removed text (step 6):** the mark and OCR layer bool tests became
 `both_ways!` tests with the same names; the re-OCR over an earlier
 session's layer rewrites for RemovalCheck (step 4's veto), not
 ReplacesText.
   - **Recovery (step 7):** the sidecar's flags carry the sections'
 classes and fields (`TailFlags { removes, sig, fields }`), since a
 restored document's saved state already holds the tail's changes
 and its path can't see them; without them a comment under a P2
 certification would append unasked. The removal check runs on
 sidecar sections (`Mode::Recovery`, against "no present or removal
 check"), only to set `removes`, and a section writing the roots adds
 `other` (`Output.unjudged.writes_roots`), so the first save of a
 restored document never writes the tail unjudged; any refusal makes
 the sidecar Full, as §11.10 says (the step's "no present check"
 contradicted it). `append::append` takes the file on disk and joins
 the tail; `Output.unjudged: append::Unjudged`; an empty section
 leaves no sidecar; `restore` reads the disk file once and stamps the
 bytes it read; `blank_table` moved to `serialize::fixture`; model
 tests set `saved` directly; `take_facts` lost its `recovered`
 parameter; main.rs's `--password` check isn't an autosave skip and
 stays.
   - **The corpus tier (step 8)** is sheaf-ops `tests/append_corpus.rs`,
 its own file beside materialize_corpus.rs, whose counts stay as they
 were: every file's mark is planned and saved onto a copy of the file
 (an Append writes in place), deleted-means-deleted ran on every
 fourth file (the 1040 test's pick) until the review, each feature
 after a first save each way, and also looks for a 48-byte run of the
 deleted page's content only it has; the oracle adds `removal::of_rewrite`
 (fixtures), the whole Rewrite taken as a section.
   - **The gate's sidecar row** held two marks (the abandoned save's and
 another), as slice 9's did; it holds one now.

   The review (26 findings on the uncommitted slice, each checked against
   the code; the fixes below, each with a test that fails without it):
   - **Pages that trade primary objects** (finding 15, a blocker): with
 the order unchanged, `tree::parents` kept every page's node, but two
 rebased pages can trade the primary's page objects, each written at
 the other's number, so the saved node listed them the other way
 round. The fast path now also needs no page rebased; a rebase
 splices (`append_rewrite_pages_that_trade_their_objects_keep_their_order`).
   - **A copy's indirect link action** (finding 16): an object the file
 has was referred to, never copied again, though what its page
 references resolve to changes when a copy of the page comes or goes.
 `deps::links` records the sources whose pages are written otherwise
 (`Closure::relinked`), and the copy closure copies their objects
 again where the section reaches them (`Closure::relinks`), written
 over their own numbers (`append_rewrite_a_copy_s_link_action_follows_its_page`).
 Not done: a primary copy 0's action, which the review supposed stale
 the same way; `leads_nowhere` already leaves such a link out, as a
 Rewrite's `keep_only` does.
   - **Append anyway answers an offer** (finding 8, deviating from §9.2's
 single Append anyway): `AskAnswers.stays_readable`, set by
 `AskAnswers::take(offer)` when the offer says something stays
 readable, and `removal::Mode::Anyway`. Report mode needs that
 consent; an Append anyway to the signatures alone vetoes a removal
 the planner missed and asks again, its offer naming
 `RemovedKind::Found`, without the `Unplanned` guard
 (`an_append_anyway_the_signatures_asked_still_stops_on_removed_text`).
 The app takes the offer it showed. Findings 1 and 10 (the veto's
 empty `stays_readable`) are the same fix: `fallback` says the path's
 removals and `Found`.
   - **`verify::section`** (findings 17, 19, 21): a key at its map number
 is skipped before anything else, and what `moved_from` looked up per
 key is built once a section (`Moved`), so the check is linear (the
 review measured rotate-all saves after the first quadratic, about
 2.4 s at 7k). The merged-widget exemption is the emission's own
 alias of the field (`Section.aliases`); the property then found a
 replaced page whose field's merged widget is another `AnnotId` now,
 the old one gone from the state, so a key the target no longer holds
 may be written over too (`Section.target`; `section_tests`).
   - **`/P` 1 and an Append of nothing** (finding 11): `sig_impact` judges
 P = 1 by `within` like the rest, so Save As of an untouched P1 file
 copies it and asks nothing (284 corpus files are P1;
 `save_as_of_an_untouched_p1_file_copies_it_and_asks_nothing`). An
 Append's class counts nothing for marks the open gave fresh ids,
 which the first Append writes whatever its class (`deps::fresh`): a
 gap before the review too, left as it was.
   - **Durability** (findings 9, 12, 13): the in-place Append's and the
 copy's stamp take their time after the file is closed (and, for the
 copy, renamed), as the Rewrite's does, since NTFS sets a write's time
 as the handle closes (not checkable here; Windows CI will say); the
 journal is written whole under a temporary name, locked
 (`File::lock`) and renamed into place, held until it goes, and
 `undo_torn` leaves a journal it can't lock (another window or process
 still appending), and both resolve links first
 (`an_append_its_writer_holds_is_left_alone`,
 `a_torn_append_through_a_link_is_undone_by_the_file_s_own_name`).
 Rejected: skipping `undo_torn` in read-only commands. Undoing a torn
 append restores the file to its last complete save, which any open
 should see, and with the lock it never touches a live append.
   - **Recovery** (finding 14): a section that fails to be made for
 anything but a refusal, a file changed or a cancel now leaves a Full
 sidecar, logged, not none (`a_section_that_fails_leaves_a_full_sidecar`).
   - **The oracle** (finding 3): `removal::of_rewrite` also runs over the
 removed-text suite's Rewrites (each finds the removal) and the
 property's planned Rewrites, where it must find no derived object's
 text gone unless the plan says something was removed or replaced.
 Narrowed there, a deviation from §11.9's "every Rewrite": the files'
 own objects aren't judged in the property, since a deleted page's
 words are on other pages too (the oracle finds nothing) and a Rewrite
 drops what nothing reaches (it finds the form's dictionary).
   - **The corpus tier** (findings 2, 20): deleted means deleted runs over
 every file, six features (a page, a mark, a reply, a mark's text,
 another tool's comment, a bookmark) each way; the Append half's first
 save goes through `write_file` with `forced_append`, so a decline is
 only a Rewrite for `BaseUnsound` or `BaseObjectsMissing`, counted by
 reason and bounded (`DELETED`); any other failure fails.
   - **Smaller fixes:** finding 6 (the xref module doc; `Entry::deleted`
 retired), 7 and 22 (`removal::vetoes_here`, a thread's own count, and
 a test logger, `removal_tests::warned`, so the tests assert the count
 and the `RemovalMisclassified` warning of their own save), 23
 (`removal_tests::Scratch`, removed on drop, shared by append_tests
 and append_rewrite_tests with `opened`, `said`, `layer` and
 `rewrite_answers`; 3,258 leaked directories cleaned from /tmp), 24
 (`the_latest_revision_is_checked_alone`), 25
 (`serialize::append::section` borrows the section's map), 26 (three
 comments), 5 (the `materialize::save` ledger row named, not
 numbered).
   - **Recorded, not changed:** finding 4, `CopyOf` never reflinks: §9.1
 and §11.7 have it reflink where it can, but the copy reads every byte
 anyway, to check `content_hash` or learn it, so a reflink would save
 only the write, through a Linux-only ioctl. Finding 18, an Append
 that writes form keys runs `forms::check_latest` over the whole file
 and the section in memory: the 7k harness has a row for it now (a
 one-page form inserted), measured below; a check over only what the
 newest revision reaches, read lazily, is unbudgeted.

   Measured (AMD EPYC 7302P, 32 threads, release builds unless marked):
   - **The gate: one mark's incremental save** (the 7k harness's
 `gates_on_the_*_7k`, each process's time the median of nine saves;
 the scanned fixture in six processes, the others in two):

 | | Synthetic | Heavy | Scanned |
 |---|---|---|---|
 | incremental save, one mark | 20.1, 28.7 ms [slice 2 56.6] | 42.7, 42.4 ms [166] | 21.2 ms (20.8–26.0) [742], asserted < 50 |
 | its section, in memory | 16.9 ms | 39.2, 38.8 ms | 17.7–18.0 ms |
 | writing it in place (journal, four syncs) | 3.2, 11.8 ms (16%, 41%) | 3.5, 3.6 ms (8%) | 3.0–8.2 ms (14–31%) |
 | RSS added | 3.2 MB | 7.8 MB | 2.8 MB |
 | the same on the AES-256 copy | 26.3, 25.5 ms (22.6 in memory) | 46.4, 46.1 ms (43.1–43.3) | — |
 | recovery sidecar of one mark (Append section) | 25.7 ms, 1.11 KB | 52.4 ms, 1.24 KB | 26.7 ms, 1.23 KB |

 The gate holds with room: the scanned's median is 21.2 ms of 50, so
 it's asserted, not ignored. In memory, step 2's emission of the
 section took about 11 ms (the closure 3.3 ms, of which `shown`
 1.7); the removal check adds about 7 ms, nearly all of it
 `LazyPdf::open` over `FileBytes` (3 ms for the same parse in
 memory). A sync spiked to 150–210 ms now and then in step 4's
 runs, and one synthetic process's writing took 11.8 ms; the median
 of nine absorbs it (the first run after the whole workspace suite
 read 63.7 ms once). Slice 9's whole-file
 Rewrite of one mark was 1,062–1,192 ms on the scanned, and its Full
 sidecar 1.6–1.7 s and 474 MB.
   - **The Rewrite against slice 8's writer**
 (`rewrites_on_the_synthetic_and_heavy_7k`; seven processes of each
 writer, run alternately, slice 8's from its own build at
 `target/slice8-gate/`, each process's time the median of three runs):

 | | Synthetic | Heavy |
 |---|---|---|
 | slice 8's writer today | 133.6 ms (130.8–138.9) | 686 ms (581–707) |
 | `materialize`, slice 10 | 152 ms (149–157); 50.4–50.6 MB added | 781 ms (775–805); 253–254 MB added |
 | ratio to the gate's constant (134 and 707 ms) | 1.13× | 1.10× |
 | ratio to today's slice 8 median | 1.14× | 1.14× |
 | RSS added, to slice 8's 61.7 and 251 MB | 0.82× | 1.01× |

 No flag: both under 1.2×, the gate at 1.3× (Greg, 2026-10-18).
 Slice 9 left 146–147 and 770–791 ms; step 2 measured 150 and 781
 ms. The Rewrite path's only new work is `in_use` built before
 numbering (`number::stable`'s give-up rule) and the `emit::shown` /
 `written_pages` split, besides the content checksum `write_file`
 takes outside `materialize` (about 2 ms on the heavy's 17.9 MB).
   - **The rest of the 7k harness,** two whole runs (slice 9's in
 brackets): the Rewrite in the fixture's process 147–149 ms [147–151],
 696–712 [629–664] and 576–644 [533–593] on the synthetic, heavy and
 scanned; ready 53.9–54.5 [54.4–66.1], 345–354 [342–357] and 456–475
 ms [451–454]; Bates' Rewrite 396–412 [396–422], 939–1,026 [916–935]
 and 791–845 [823–829]; the heavy flatten 1,759–1,766 ms
 [1,711–1,720]. Nothing gated moved past its gate.
   - **The corpus** (../pdf-corpus): materialize_corpus is unchanged:
 identity 4,389 of 4,392 written, 2,269,722 primary objects
 surviving, 919,479 copied as they were, 436 changed by `keep_only`, 0
 failed; 2,901 forms without their first page, 0 failed (551 findings
 of the allowed kinds, 0 new by name); every page duplicated, 4,389, 0
 failed; inserted twice into the 1040, 1,098, 0 failed; four times,
 0 differ; the 2932995 logo, 0 of 2,037,600 bytes differ.
 encrypted_corpus: 573 encrypted files, the Rewrite keeps all 573 and
 the Append 572, one declined (0630899's rebuilt table). append_corpus:
 4,389 files marked as planned, 3,770 appended, 259 rewritten (211
 RemovalCheck, 34 BaseObjectsMissing, 14 BaseUnsound), 360 asked
 about signatures, 0 failed; removal-check vetoes 0 derived and 211
 source (4.8% of the files, every one a planned DropUr3); deleted
 means deleted on every file since the review, each feature each way,
 0 failed: a page 2,901 files (2,031 with a run of its content only it
 had after a Rewrite, 2,022 after an Append; the first Append declined
 by 28, 17 `BaseUnsound` and 11 `BaseObjectsMissing`), a mark, a reply
 and a mark's text 4,389 each (68 declined: 30 and 38), another tool's
 comment 30 (the files with one whose text no other object holds; 0
 declined), a bookmark 4,389 (30 declined, `BaseUnsound`); step 8's
 every fourth file had 728, 1,098 and 1,098. The oracle finds text
 gone from 2,899 of 2,901 page-delete Rewrites, 0 derived; the nine
 signed files as §9.3 says. Step 4's experiment, the twins' Append
 halves in Veto mode, vetoed 25, every one a true removal but another
 tool's comment made a sheaf mark (its dates). append_corpus takes
 453 s, nearly all of it deleted means deleted (434 s alone: 12 runs
 over 4,392 files; step 8's every fourth file took about 80 s). After
 the review, Greg moved deleted means deleted to the ignored tier
 (every file still; Verify and Commit run it), so the default run of
 append_corpus is the other three tests.
   - **Interop** (qpdf 12.3.2, node 24.22, pdfjs-dist 6.4.299): `qpdf
 --check` clean on 84 Rewrite outputs and 14 Append outputs, three
 Appends in a row onto each of 14 bases (42 checks), a delete saved
 and undone on each of 14 bases (a Rewrite for BaseObjectsMissing,
 every page back), and both records outputs; pdf.js reads the records
 file both ways (the Append half keeps the comment only its Rewrite
 half deletes), and the prefixed Append opens identically in qpdf and
 pdf.js (table and stream). Once, by hand: `qpdf --check` on all 3,770
 corpus one-mark Appends, none worse than its original (Found).
   - **Suites:** `cargo test --workspace` 1,085 passed and 30 ignored in
 62 binaries, 419 s (slice 9: 892 and 96), sheaf-doc's unit tests
 688 and 3 ignored in 31.3 s; fmt and clippy (`-D warnings`, all
 targets) are clean, and `cargo check --target x86_64-pc-windows-msvc
 -p sheaf-doc -p sheaf-ops --tests` passes. The ignored tier in
 release, the 7k harness apart (`-- --ignored --skip 7k --skip
 200_ocr`, with qpdf and pdf.js), 559 s: 15 passed (the replay
 property at 4,096 cases, Append ≡ Rewrite at 1,024, the interop
 tier, the OCR engines, the lexer-vs-PDFium differential, PDFium's
 frames), and the fuzz-seed writer failed as it does without
 `$SHEAF_FUZZ_SEEDS`; no test is ignored for slice 10 any more.
   - **After the review** (release): the scanned 7k's one mark 22.3 ms
 (its section 18.3 ms, writing it 4.0 ms), under 50; the Rewrite in
 ten processes each, 149 ms (144–152) and 779 ms (764–782), 1.11× and
 1.10× the gate's 134 and 707 ms, no flag (`Moved`, `relinked` and
 the tree's rebase rule touch only the Append; the closure's
 `relinks` is an empty set's lookup on the Rewrite path). The new row,
 a one-page form inserted into the scanned 7k as an Append, takes
 1,036 ms and adds 1,577 MB, nearly all of it `forms::check_latest`
 loading the whole 500 MB file in memory (finding 18). The review's
 rotate-all saves after the first, at 1,000, 2,000 and 4,000 pages,
 take 26, 56 and 120 ms (63, 207 and 770 before `Moved`). The corpus
 baselines hold (above), encrypted_corpus 573 kept and 572 appended,
 interop's qpdf and pdf.js rows pass (84 Rewrites and 14 Appends
 clean, three Appends on 14 bases, deletes saved and undone), the
 property passes 1,024 cases in release (73 s) with the oracle in it,
 and `cargo test --workspace` passes 1,095 with 30 ignored in 62
 binaries (783 s); fmt, clippy and `cargo check --target
 x86_64-pc-windows-msvc` are clean.
   - **The review:** no review step ran within the slice's eight steps;
 each step checked its own new checks by switching their behavior off
 (they failed, and passed with it back). A review of the whole slice
 followed, its 26 findings above; each fix's test was run with the
 fix switched off, and failed.

   Found:
   - **A widget that is its field's object, where the field has other
 widgets or the widget shows twice,** is written apart from its field
 by every save (`emit::form`'s split), so an opened file's map has no
 number for the widget's key and an Append that writes its page
 refers to an object the file hasn't got: 34 of the corpus's one-mark
 saves rewrite for BaseObjectsMissing (0003532's radio parent is
 itself a widget with two kids; 0218989's signature widget is listed
 on two pages). Correct, and only slower; an Append could refer to the
 merged object as the widget, which the Rewrite's split would then
 have to match (unbudgeted).
   - **Dropping usage rights on an Append is always vetoed:** a comment on
 a file whose rights allow only filling in (211 corpus files, with
 Reader's usage rights and no other signature) plans DropUr3, which
 unlinks the `/UR3` signature dictionary, whose strings the removal
 check finds gone; the save rewrites for RemovalCheck, as DropXfa's
 does. A decision for Greg (below).
   - **qpdf passes every one-mark Append of the corpus** (3,770 files,
 run once, not kept as a test): none checks worse than its original.
 2,107 check clean as their originals do; 1,546 originals warn,
 nearly all of their linearization hints, which an update makes moot,
 and their Appends check clean; 117 keep their originals' warnings
 (widgets not reachable from `/AcroForm` and the like).
   - **The oracle finds nothing in 2 of 2,901 page-delete Rewrites**
 (0016064 and 0112654): their cover pages' words are on other pages
 too (0016064's logo as well), so nothing of the page is gone from the
 file. The check is as sensitive as it should be there.
   - **Linearized originals** stop being linearized after an Append, as
 an update makes them; nothing reads them as linearized afterwards
 (sheaf, PDFium, qpdf, pdf.js).
   - **The section in memory is most of an Append's time** (about 18 of
 21 ms on the scanned 7k): the removal check's open of the file's
 reader is about 7 ms of it (3 ms for the same parse in memory);
 reading the newest cross-reference section in one window to EOF in
 lopdf's `XrefReader` would save about 4 ms with the same parse, not
 taken (the gate holds).

   Checked by tests:
   - **Removed text, every feature × both strategies** (kept/tests.rs,
 `both_ways!`): `a_deleted_page_is_gone_from_the_saved_bytes`,
 `a_deleted_mark_is_gone_…`, `a_deleted_reply_is_gone_…`,
 `another_tool_s_deleted_comment_is_gone_…`,
 `a_deleted_bookmark_is_gone_…`,
 `a_mark_s_replaced_text_is_gone_…`, `deleted_mark_gone`,
 `replacing_saved_ocr_text_takes_the_old_out` and
 `a_re_ocr_over_an_earlier_session_s_layer_takes_the_old_out`, each a
 forced first save, the removal, a planned Rewrite with its exact
 reasons, and the text unreadable raw, in UTF-16, hex of either,
 decoded streams and strings; the corpus tier's
 `deleted_means_deleted_both_ways`.
   - **Appends after earlier saves:**
 `a_mark_s_color_change_writes_the_mark_alone` (a Rewrite, then a
 style change: an Append rewriting the mark at the Rewrite map's
 number, plus only its dependents; planned through
 `planned_in_place`);
 `a_bookmark_renamed_rewrites_and_its_old_title_goes`;
 `pages_deleted_saved_and_undone_come_back_by_a_rewrite` (Append
 planned, BaseObjectsMissing at the write, every page back) and
 interop's `pages_deleted_saved_and_undone_pass_qpdf_check`;
 `back_at_the_saved_state_nothing_is_written` (bytes and mtime, a
 clean document, the legacy header on too) and
 `an_unchanged_plan_writes_nothing`.
   - **Durability:** `appends_one_after_another_reopen` (three Appends,
 the streamed checksum the one-shot one, the record's facts),
 sheaf-render's `three_appends_in_a_row_reopen` (LazyPdf and PDFium),
 interop's `three_appends_pass_qpdf_check`;
 `a_torn_append_is_undone_when_the_file_opens`,
 `a_torn_update_is_cut_back_when_the_file_opens`,
 `a_journal_undoes_only_its_own_torn_update`,
 `an_update_goes_after_the_file_and_leaves_no_journal`.
   - **The Append ≡ Rewrite property:** `append_rewrite_reopen_alike` (32
 cases, default run) and `append_rewrite_reopen_alike_1024`
 (ignored), with the fixed cases (labels i–iii with page 4 moved
 first, links to pages a later Append brings, a split then a reorder,
 the header over a reorder for the live-token case, and
 `append_rewrite_facts_tell_files_apart`) and the eleven fixes' named
 cases.
   - **The removal check:** `a_forced_misclassification_is_vetoed_and_counted`
 and `a_misjudged_removal_is_vetoed_through_the_plan` (vetoed, a
 Rewrite with RemovalCheck, the counter up);
 `the_veto_on_a_certified_file_asks_and_never_rewrites`;
 `an_unknown_encoding_counts_as_differing`,
 `a_re_subset_font_with_the_same_text_passes`,
 `an_overwritten_appearance_is_judged_on_its_inputs`,
 `an_unlinked_form_s_text_is_read`; the corpus tier's oracle,
 `the_removal_check_finds_what_a_page_delete_rewrite_took_out`, and
 `RemovalMisclassified` 0 for derived objects over the corpus.
   - **Recovery:** `an_encrypted_file_s_full_sidecar_holds_no_plaintext`
 (the unsaved header's sidecar, encrypted);
 `a_same_length_change_makes_a_sidecar_stale` (the tail and the
 startxref window, `find` and `restore` both);
 `a_restored_signed_file_appends_unless_its_sections_removed`;
 `a_second_crash_chains_the_sections`,
 `a_password_protected_document_s_sidecar_restores_with_the_password`
 (RC4-128, AES-128, AES-256),
 `a_restored_signed_file_saved_as_another_copies_and_appends`,
 `an_append_sidecar_of_one_mark_is_far_smaller_than_the_file`,
 `a_misjudged_section_still_flags_its_sidecar`.
   - **Signatures:** `a_certification_permits_what_its_p_says` (P3
 appends a comment silently, P2 asks with Append anyway);
 `a_comment_deleted_from_a_signed_file_offers_append_anyway` (taken:
 the signed bytes a prefix, the byte range fitting);
 `a_page_inserted_into_a_signed_hybrid_form_offers_append_anyway`
 (the section's AcroForm without `/XFA`);
 `usage_rights_stay_within_what_they_allow` (the three conservative
 cases); `save_as_of_a_signed_file_copies_and_appends` and forms_saves'
 `a_signed_file_saved_as_another_copies_and_appends`;
 `a_section_writing_the_catalog_unplanned_asks`; every cell of §9.3's
 table, FieldMDP and `/Lock /P` (`forms::sigs::tests`); the corpus
 tier's `a_comment_on_a_signed_file_appends_or_asks_by_what_its_signatures_allow`.
   - **Files:** `a_file_changed_before_an_append_asks_and_overwrite_rewrites`
 (a byte appended, a same-length change at the window of a file whose
 table is past 64 KiB), `overwrite_makes_an_append_a_rewrite`,
 `a_file_another_program_changed_is_left_alone`,
 `a_change_at_the_last_section_shows_however_far_back`,
 `a_copy_appends_only_to_the_file_it_was_stamped`.
   - **The gate:** `gates_on_the_scanned_7k` asserts the one-mark
 incremental save under 50 ms (ignored, release; Measured).
   - **The review's:** `append_rewrite_pages_that_trade_their_objects_keep_their_order`,
 `append_rewrite_a_copy_s_link_action_follows_its_page`,
 `an_append_anyway_the_signatures_asked_still_stops_on_removed_text`,
 `save_as_of_an_untouched_p1_file_copies_it_and_asks_nothing`,
 verify's `section_tests` (a key at its map number; only the field's
 merged widget, or a record gone, at its number),
 `an_append_its_writer_holds_is_left_alone`,
 `a_torn_append_through_a_link_is_undone_by_the_file_s_own_name`,
 `a_section_that_fails_leaves_a_full_sidecar`,
 `the_latest_revision_is_checked_alone`; the veto tests asserting
 their own thread's count and their `RemovalMisclassified` warning;
 the P2 veto test's non-empty `stays_readable`; the removal check as
 an oracle in the removed-text suite and the property; the corpus
 tier's six features over every file.
   - **Also:** the twins (every `_appended` test, by default); the model's
 `path_keys_run_through_where_the_branches_meet` and
 `the_saved_state_by_another_route_changes_nothing`; the history
 proptest's check that `judge(...).changed` is a brute-force diff of
 every key; effects' per-row tests and invariants; the tree's splice
 tests; and the ledger's `nothing_retired_is_back` with slice 10's
 rows and `a_variant_s_field_is_named_from_its_enum`. Each new check
 the steps name was run with its behavior switched off, failed, and
 passed with it back.

   For Greg:
   - **An Append that writes form keys checks the whole file's form**
 (finding 18): 1,036 ms and 1,577 MB added for a one-page form
 inserted into the scanned 7k, against one mark's 22 ms. The check
 is a real post-condition and stays; reading only what the newest
 revision reaches, lazily, is unbudgeted. Greg (2026-10-19): keep
 the check; a scoped one is a follow-up (Left for later).
   - **Deleted means deleted runs on every corpus file by default** (the
 review's finding 2, against step 8's every fourth file): 434 s, and
 `cargo test --workspace` went from 419 s to 783 s. Greg
 (2026-10-19): moved to the ignored tier, still over every file;
 each slice's Verify and Commit run it.
   - **§9.3's mapping is conservative** until the Acrobat cross-check:
 `/P` 2 allows only filling in (so a P2 + UR3 comment asks), an
 approval signature with no MDP allows filling in and comments, a
 signature that can't be read is `/P` 1, FieldMDP's includes lock the
 nodes over a name too. 284 corpus forms are GPO certifications with
 `/P` 1, so every save of them asks, Appends included (360 of the
 corpus's one-mark saves ask). The picked files are in Asks.
   - **Planned drops the removal check vetoes:** DropXfa on a hybrid
 form's Append and DropUr3 on a file whose rights a change exceeds
 unlink objects with text (the XFA streams, the `/UR3` signature), so
 the save rewrites (211 corpus files' comments) or a signed file asks.
 Should a planned transform's unlinked objects be allowed drops?
   - **The command line copy-appends** a signed input saved to another
 path when the signatures allow it (the `materialize::save` ledger
 row), until slice 19's flags.
   - **Save As over the original signed file after a Rewrite** no longer
 asks: the saved file's signatures don't hold.
   - **UR3-only files count as signed for CopyOf,** so Save As keeps their
 rights when a change is within them.
   - **Another tool's comment edited into a sheaf mark always rewrites:**
 its old dates and strings don't survive an Append.
   - **An unknown encoding** is the raw bytes' hash: the same text in
 another encoding counts as differing.
   - **A field rename counts as replaced text,** so it rewrites.
   - **An XFA copy comes back** on an Append when save policy keeps it
 again (the fields as the file had them), refused when a Rewrite
 pruned its streams.
   - **Held refusals** (a primary object whose number the file gives
 another record) rewrite rather than renumber in place.
   - **Recovery:** the removal check runs on sidecar sections only to set
 `removes`; sidecar flags carry classes and fields; a Full restore
 can't copy-append on Save As (NoSaveRecord: a Rewrite that asks); a
 restored file with an approval signature appends silently when its
 tail removed nothing, and asks with Append anyway when it did,
 saying only "what the recovered changes removed".
   - **Writes:** an I/O error while appending cuts the file back at once,
 and `undo_torn` restores the file's time, so a sidecar written
 against the file still holds. A forced Append (Append anyway) applies
 Append-mode transforms and checks only what the newest revision's
 form reaches: unlinked widgets are what "stays readable".

   Left for later slices:
   - **Slice 11:** `FormInst.src` a `BlobId`, ending the form-shift refusal
 (ledger 10→11); `Step.effects` and §7.1's `Effects.pages`/`keys`
 (PageTouch) for previews; the field cascade (a deleted page's fields).
   - **Slices 12–13:** what §7.3's other consumers read of effects
 (approvals, the realizer).
   - **Slices 16–17:** `LIVE_REDRAW`, `SaveRecord.drawn` and
 `Emission.allowed_drops` (live redraws), the header and footer as
 generators, ending `LegacyHeaderFooter` and its Full sidecar
 (ledger 10→17); live-token reorders appending.
   - **Slice 19:** the command line's `-o … --append` and `--signatures`
 flags (proposal 40), replacing `materialize::save`'s answers.
   - **M12, or once Greg has checked Acrobat:** §9.3's mapping (ledger).
   - **Follow-up (Greg kept the check, 2026-10-19):** a form check for
 an Append that writes form keys that reads only what the section
 wrote and what it reaches, lazily (today 1,036 ms and 1,577 MB added
 for one form page inserted into the scanned 7k, the review's finding
 18), before fills (M10 slice 9) make such Appends common.
   - **Unbudgeted:** an Append that
 refers to a widget merged with its field where the field has others
 (34 corpus Rewrites); a one-window newest-section read in lopdf
 (about 4 ms of an Append); planned transforms' drops in
 `allowed_drops`, if Greg says so.

   After the commit (a review of cabd8e8; each finding fixed in a
   follow-up, with a test that fails with the fix switched off):
   - **An Append's new numbers and a dangling reference** (finding 5):
 `number::appended` numbered new keys past the file's `/Size`, its
 cross-reference streams, its map and `present`, but not past the
 primary numbers the section names, as `number::stable`'s `named`
 does, so a primary object's reference to a number nothing is
 (another tool's damage, which `Saved::own` passes as the file's), at
 or past `/Size`, could be taken by a new object and name it; and
 `verify::section` passed a reference to any number the primary never
 used, a new one the section doesn't write too. Now the new numbers
 start past every primary number the section names: copy 0 of the
 map's identity blob among its keys and placeholders, what
 `Emission::owned` writes at, and what the primary's objects it
 writes as they are (`SymObject::Raw`, read before numbering now)
 refer to, none past `NUMBERS`. When only those leave too few numbers
 the Append is refused (`Refused::Unsound`), so the save is a
 Rewrite, which numbers it for itself. And `verify::section` fails a
 reference at or past the first new number (below `NUMBERS`) to what
 the section doesn't write, as `links`' `given` does. Tests:
 `a_dangling_reference_stays_dangling_past_the_new_numbers` (a page
 naming its `/Size` and four past it, a mark added and three pages of
 another file inserted, planned as an Append; and the Rewrite of the
 same edits), `a_reply_written_as_it_was_keeps_its_dangling_reference`
 (another tool's reply naming the `/Size`, written raw beside its
 mark's new appearance), `numbers_named_near_the_last_refuse_the_append`
 (the refusal, and the planned save's Rewrite stopping as its own
 numbering does, nothing written), and verify's
 `a_reference_to_a_number_the_section_gave_and_doesn_t_write_fails`.
 Switched off, each failed: without `named` the first two found their
 references naming new objects (`12 0` and `16 0`; `12 0`) and the
 third wasn't refused; without the raw objects' numbers only the
 reply's test failed; with a plain error for the refusal, the third;
 without `given`, the section's check passed `101 0`.
 Deviations and what's left: the raw objects' references count too,
 which `stable` doesn't count of a Rewrite's; the Rewrite is
 unchanged, its bytes the same. So what a number at or past `/Size`
 that nothing is can still come to name a new object from is what
 neither counts: a Rewrite's primary objects as they are, and the
 file's objects outside an Append's section (any update numbers from
 the file's `/Size`, and finding such references means reading every
 object); the Rewrite's half would change its bytes, so it's left for
 Greg. An object both strategies build that names a number so
 near 8,388,607 that the new objects don't fit past it now stops the
 save: the Append is refused and the Rewrite stops, as it always did
 (the Append used to write it). The corpus didn't move:
 materialize_corpus's identity 4,389 of 4,392 written, 2,269,722
 surviving, 919,479 copied, 436 changed by `keep_only`, 0 failed (the
 other five rows as before); append_corpus 3,770 appended, 259
 rewritten (34, 14 and 211), 360 asked, 0 failed; encrypted_corpus 573
 kept by the Rewrite, 572 by the Append and one declined; deleted
 means deleted (ignored tier) as before, the first Append declined by
 28, 68, 68, 68, 0 and 30, 0 failed.
   - **A crop's copy made copy 0 by a move** (finding 6): `emit::shown`
 makes the first in order of a page's derived copies its copy 0, so
 moving the duplicate of a cropped page before the original made the
 duplicate the page's copy 0, the owner of the primary object's
 number, while neither page was written (one node holds both). An
 Append numbers a key by the file's map first, so that number still
 held the original, and what names the page by number (the second
 page's link, which the section writes again; the catalog's
 `/OpenAction`, which it keeps) went to it, where a Rewrite claims the
 owned numbers first and writes the duplicate there. `append::held`
 checked only the keys the section writes. Now it checks every key of
 `Emission::owned`: one the section doesn't write is where the map has
 it (else at its own number, as `Saved::number` finds it), and one
 anywhere but its own number refuses the Append (`Refused::Held`,
 `BaseObjectsMissing`), as another key the file holds there that stays
 did. Tests: `append_rewrite_a_crop_s_copy_made_copy_0_by_a_move_rewrites`
 (the review's scenario, the second page's link going to the first)
 and `append_rewrite_an_open_action_to_a_crop_s_copy_made_copy_0_rewrites`
 (the catalog's `/OpenAction` going there instead), each reopened as a
 Rewrite of the same state after both saves; the reopen compares where
 the open action goes now (the property's files had none until the
 second round below gave them one). Switched off (only the keys
 written checked), the open action's test failed (the Append's going
 to the second place, the Rewrite's to the first); the link's still
 passed, as finding 7's check refuses it too, and with both switched
 off it failed the same way.
   - **Page references the relinking didn't judge again** (finding 7):
 finding 16's fix judged again only other tools' links with an object
 of their own, by their explicit destination. What else the file has
 that names a page of a source, which a copy's reference names
 otherwise now (a widget's action, a field's, a link written straight
 into `/Annots`, an annotation the records don't hold), kept the page
 it named when it was written. Now `deps::closure` finds such a holder
 the section doesn't write again (`Closure::stale`) and `append`
 refuses (`Refused::Stale`, `BaseObjectsMissing`), so the save is a
 Rewrite. `Relinking` keeps both states' written pages; `moved` says
 whether a reference resolves otherwise, as `copy::page_named` (the
 copy closure's, shared now) resolves it, or from the primary's copy 0
 by its number: it named a page then and another or none now (one that
 named none names the page that came, which `held` keeps at the
 number). The holders, each read as its emitter reads it: every page's
 own keys (as copy 0, but `/Parent`, `/Annots`, `/Contents`,
 `/Resources` and `/Thumb`), what its `/Annots` has that no record
 holds (as the page's copy, but a slot read as a record, sheaf's old
 marks and pop-ups), other tools' annotations, widgets, fields, and
 sheaf's marks read from a file. One written again names the page
 anew, and what it reaches of a relinked source is copied again, but
 not from the primary's copy 0, whose references are numbers; a link
 that leads nowhere now is left out only where its page is written
 again (`annots::leads_nowhere` takes what's written as a closure
 now). Each walk follows references as a copy does, never into a page
 or a document-level object, each object once a copy, from the
 holder's keys but those the emitter writes itself (`/P`, `/Parent`,
 `/IRT`, `/Popup`, a field's `/Kids`), and only for a source and copy
 a reference of which moves: a duplicate in a large primary reads no
 object. Tests: `append_rewrite_a_copy_s_inline_link_follows_its_page`
 and `append_rewrite_a_copy_s_screen_action_follows_its_page` (finding
 16's test with a link written into `/Annots`, and a screen annotation,
 each with a GoTo action), `append_rewrite_a_duplicate_s_button_action_follows_its_page`,
 and `append_rewrite_a_copy_s_button_of_the_first_instance_appends`,
 which stays an Append. The review's button scenario doesn't diverge:
 pages brought in again join the instance of their file the first
 brought (`take_in_fields`), so the copy's button is read as copy 0
 and goes to the second page's copy 0 under both; a duplicate's button
 is a new instance, copy 1, which does. Switched off (the refusal
 skipped), the three failed, their actions going to the third place
 against the Rewrite's fifth (the button's the third against the
 fourth). The property's reopen compares a widget's action and where
 each `/Annots` entry that goes somewhere goes, and the files' push
 buttons go to a page (the document's sixth, the other file's first):
 with both fixes switched off its 1,024 cases failed (a button whose
 page was deleted, appended anyway, named the page's old object where
 the Rewrite wrote null), and proptest kept the shrunk run of two
 actions in `proptest-regressions/materialize/`, which every run
 replays first; with them on they pass.
 What's left: it's conservative, so some Appends rewrite that needn't
 (a holder of the primary's copy 0 written again for another reason, a
 slot whose widget record went, a walk into a widget a copy writes as
 null). Not walked: a page's content and resources, which never name
 a page, and a page template's widgets, reached only through a field's
 `/Kids`. Found here, not in these findings: the catalog's
 `/OpenAction` and named destinations going to a primary page that
 went stay naming the page's old object, which a Rewrite's
 `keep_only` drops (a throwaway test of the open action: `[7 0 R
 /Fit]` against none). It isn't only a delete appended anyway: the
 second round below refuses it. Left for Greg: `leads_nowhere`
 doesn't resolve a link's named destination, which `keep_only` does.
 `leads_nowhere` and `page_named` moved without changing what's
 written. The corpus didn't move: materialize_corpus's identity 4,389
 of 4,392 written, 2,269,722 surviving, 919,479 copied, 436 changed by
 `keep_only`, 0 failed (the other five rows as before);
 append_corpus 3,770 appended, 259 rewritten (34, 14 and 211), 360
 asked, 0 failed; encrypted_corpus 573 kept by the Rewrite, 572 by the
 Append and one declined; deleted means deleted (ignored tier) as
 before, the first Append declined by 28, 68, 68, 68, 0 and 30, 0
 failed.
   - **An Append anyway's removals it didn't list** (finding 4): in
 `Mode::Report` (an offer that listed what the path removes, taken)
 the removal check reported whatever it found and never called
 `removal::veto`. A derived object's find is a misclassification
 unless the path said it, but one the path didn't say (a mark's
 replaced text that `judge` missed, beside the comment the offer
 listed) was written with the generic note. It wasn't logged as
 `RemovalMisclassified` or counted, and it wasn't asked about again;
 only an offer that listed nothing got that. Now `judge` keeps, slot
 by slot, what removes or replaces text (`PathEffects::removing`, what
 `removed` counts), and an Append anyway judges the path again and
 hands those slots to the check (`Section::said`). A mark is said
 when its slot is. What's drawn over a page is said when its page's
 slot is (the recognized text) and a generator's is (the header and
 footer). A find the path didn't say comes before one it did
 (`Found::said`, `Found::misclassified`). In `Report`, an unsaid find
 stops the Append, logged and counted (`removal::count`, which was
 `veto`). The save then asks again, offering the Append with
 `RemovedKind::Found` listed too, as the empty offer's path does. A
 find the path said is reported as before. The new
 `AskAnswers::found` answers that ask: `take` sets it for an offer
 that lists `Found`, and `takes` requires it of one (without that,
 the fallback took its own offer and the save failed). It picks the
 new `Mode::ReportAll`, which reports whatever it finds and still
 logs and counts a misclassification, so `Vetoes::derived` counts
 misclassifications whether or not they stopped an Append. The
 harnesses that append whatever the check finds say so: `found` in
 `save_appended`, append_corpus's `appended_or_not` and
 append_rewrite_tests' `forced`, and `ReportAll` in
 `saving::appended`. `removes_nothing` clears `removing` too. Tests:
 `an_append_anyway_that_listed_a_removal_still_stops_on_one_it_did_not`
 covers the review's scenario. On a P2-certified file, two comments
 are appended anyway; then one is deleted and the other's text
 replaced, and `judge` misses the replaced text. The offer lists 1
 comment. Taken, the save asks again, naming the other mark's key
 and listing `Found`; the misclassification is logged and counted
 once, and nothing is written. Taken again, the save appends and the
 reader is told; it's logged and counted again. In addition,
 `a_comment_deleted_from_a_signed_file_offers_append_anyway` now
 asserts that its listed comment's find is neither logged nor
 counted. Switched off, each part failed:
 - the Report refusal skipped: the first save appended with the
   generic note;
 - `takes` ignoring `found`: the save failed ("the update would leave
   removed text readable in Annot(…)") instead of asking;
 - the said slots empty in `Report`: the new test named the listed
   comment's key, and the listed-removal tests
   (`a_comment_deleted_…`,
   `the_veto_on_a_certified_file_asks_and_never_rewrites`) asked
   again instead of reporting;
 - ReportAll's count skipped: no warning on the second save.

 What's left: another's object (another tool's comment, a page's
 content) is judged on its text alone, which can't be matched to the
 offer's kinds, so `Report` still reports its find. A recovery
 sidecar's section (`Mode::Recovery`) still neither logs nor counts
 (what it finds flags the sidecar). `ReportAll` counts a
 misclassification again on the save that writes it, after the one
 that asked. §11.9's "Append anyway" bullet says this now. The
 emitter, numbering, verify and the Rewrite are untouched
 (materialize_corpus not run). The corpus didn't move: append_corpus
 3,770 appended, 259 rewritten (34, 14 and 211), 360 asked, 0 failed,
 vetoes 0 derived and 211 on the files' own objects;
 encrypted_corpus 573 kept by the Rewrite, 572 by the Append and one
 declined; deleted means deleted (ignored tier) the first Append
 declined by 28, 68, 68, 68, 0 and 30, 0 failed, and its Appends
 anyway (now `ReportAll`) counted no misclassification.
   - **An encrypted sidecar's page string** (finding 0): slice 10
 narrowed recovery's `an_encrypted_sidecar_holds_no_plaintext` to the
 mark's note, though the turn's section still writes the page's
 dictionary, whose `/PieceInfo` holds `PAGE_STRING` (serialize's
 `encrypted_files_append_encrypted` checks the same of an Append).
 The check is back: the page's string and the note are in the sidecar
 in the clear only when the file is. It held as soon as it was
 restored, so nothing else changed. Checked with the section writing a
 page's dictionary unencrypted (and the debug read-back, which stops
 that first, off): the restored check failed on `PAGE_STRING`. The
 narrowed test failed too, but only later, reading the notes back from
 the garbled page.
   - **Bug 13 after a Rewrite** (finding 1): `undo_past_a_save`'s
 non-appended half saved first as planned, which is an Append now (a
 move removes nothing), so both halves appended first and M9's "the
 first save was a full rewrite" didn't run. The first save is
 `How::save` now, a Rewrite or an Append by the half. The save after
 the undo runs twice, from a fresh file each time: planned (an Append
 onto the file the first save wrote) and by a Rewrite. Every save's
 strategy is asserted, and all four orders write A0 A1 A2. Checked
 with a Rewrite recording the previous record's page tree instead of
 the one it wrote: the Rewrite-then-planned run wrote A2 A0 A1 (bug
 13's symptom) and failed. It was the only failure in sheaf-doc's lib
 tests (708 passed), and the test as slice 10 left it passed.
   - **A comment made ours, as planned** (finding 2): the test said
 adopting takes nothing out, so a save in place appends, and checked
 that with `save_appended`'s forced Append. The planned save
 rewrites. `AnnotAdd` draws the mark's id afresh, so `judge` sees the
 square, the caret and the strikeout grouped under it go as comments
 (`Why::Removes([Comments: 3])`). The removal check, in Veto mode,
 refuses the Append too (`Refused::Removes` on the old strikeout, not
 derived): the replacement writes its own (`sheaf:<id>:strike`), and
 the old one's `/NM (c1-strike)` would stay readable. The test now
 asserts both, and that the Rewrite leaves no `c1-strike`. Its
 Append-anyway half stays, asserting the "stays readable" note and
 that the file was appended to. Either way, deleting the adopted
 square after that is a Rewrite with `Removes([Comments: 2])` (the
 square and its reply). The doc comment says so. Checked:
 - with `judge` not counting a foreign comment's removal, the planned
   save fell back on the removal check's veto (`Why::RemovalCheck`,
   the review's scenario), and the test failed;
 - with the removal check finding nothing, the Veto-mode Append went
   through, and it failed;
 - slice 10's version passed under both.

 Left for Greg: every save in place that adopts a comment now
 rewrites, where slice 8's `removes_content` counted an adopted
 comment as kept and appended. It fails closed, but the reader is
 told "3 comments removed since the last save" for two comments they
 made theirs.
   - **A form saved twice, the second by a Rewrite** (finding 3):
 forms_merge's `saved_twice` asserted its second save in place as
 planned (an Append) in both halves. So no second Rewrite numbered
 through the first save's record; the detached `rewrite_to_path`
 copy has none. Each half now runs twice from a fresh file: the
 second save planned (asserted an Append) and forced
 (`save_rewritten`, asserted a Rewrite). The copy is written beside
 each, and the file and the copy are checked clean with 5 fields and
 6 widgets. Only the forced second Rewrite numbers through a record
 here: 26 keys after a first Rewrite and 22 after a first Append
 (pages, the inserted form's objects, the duplicate's copy, widgets
 and fields), where every other Rewrite in the test has the opened
 file's 3. Checked with `number::stable` writing a field it reuses
 from the record one number lower, where the record has a widget:
 both halves failed ("the written form has damaged field trees").
 Slice 10's version and forms_saves passed (three sheaf-doc lib
 tests, the property among them, failed too). Mutations that only
 renumber (the give-up rule dropped, or `taken` not guarding the
 record's or the owned numbers) leave the output valid and the test
 passing, as they should.

 Findings 0 to 3 change tests only: the emitter, numbering, verify
 and the Rewrite are untouched (materialize_corpus not run).

 A second review of these fixes (each fix below with a test that
 fails with it switched off):
   - **A page that's no page's copy 0, named by the catalog**: the gap
 above isn't only a delete appended anyway. A page deleted whose
 duplicate (copy 1) stays removes nothing, so the plan is a plain
 Append, and when the page drew no text the removal check finds
 nothing either; `held` checks only owned keys and `stale` walked no
 document-level object, so the catalog's `/OpenAction`, `/AA`, the
 names' `/Dests`, the structure tree's `/Pg` and an article's beads
 kept naming the page's old object, outside the tree, where a
 Rewrite's `keep_only` prunes them. Now `Closure::stale` walks, last,
 the primary's catalog as copy 0 but `CATALOG_OWNED` (into its names,
 structure tree and actions, never into a page) and refuses
 (`Refused::Stale(ObjKey::Catalog)`, `BaseObjectsMissing`) when it
 reaches a page of the primary that was a page's copy 0 then and is
 none's now (`Relinking::went`). A page that's another page's copy 0
 now isn't refused here: `held` keeps it at the number, where the
 Rewrite writes it too. Tests: `a_page_deleted_after_its_duplicate`
 (four pages, the last blank; duplicated, then deleted, saved between
 and not), planned as an Append, refused as named, and the save a
 Rewrite that reopens as a fresh one, for the open action
 (`…_an_open_action_to_a_page_deleted_…`), a named destination and
 the structure tree (`…_names_of_a_page_deleted_…`). Switched off,
 the Append wrote the open action as `[11 0 R /Fit]` against none,
 and both tests' refusal was none. The property's document has an
 open action now (`files_opening`, to the second page; the fixed
 cases' files have none, as several append a delete, replacement or
 rebase of each page that this refuses): with the walk switched off
 its 1,024 cases failed (the open action, and a form check finding
 the old page's widgets reachable through it); on, they pass.
   - **`stale`'s other holders** (no test reached them): a page's own
 keys (`PAGE_OWN`, read as copy 0), a field apart from its widgets
 (the fields' loop) and a mark read from a file. Tests on the same
 helper: the first page's `/AA` (`…_a_page_s_action_…`), a radio
 group's own `/AA` (`…_a_field_s_action_…`) and a note of sheaf's
 with an action (`…_a_mark_s_action_…`), each going to the deleted
 page, refused as `Stale` of the page, the field and the mark. Each
 failed (no refusal) with its branch switched off and passed on.
   - **Which refusal the crop tests pin**: both asserted only
 `BaseObjectsMissing`, which `Missing`, `FormShift`, `Held` and
 `Stale` all give. The helper now runs the Append on the moved state
 (`refusal`, Veto mode) and asserts which: the open action's case is
 `Held(Page(copy))`, the review's link case `Stale(Annot(link))`, as
 the link is the primary's copy 0 and `stale` refuses it first. So
 reverting `held` to the emitted keys fails the open action's test.
   - **An OCR layer replaced, appended anyway** (finding 4's page
 branch): `a_page_recognized_again_on_a_signed_file_appends_anyway_as_listed`.
 On an approval-signed file a layer appended anyway, then recognized
 again: the plan is a Rewrite for `ReplacesText([OcrLayers: 1])`
 offering the Append anyway; taken (`Report`), the check's find (the
 page's block) is the page slot's, so it appends with the note, no
 warning and no count. With `Drawn::on`'s bags swapped, or the page's
 slot not counted as said, the save asked again with `Found`.
   - **The adopted comments' reason**: `a_comment_made_ours_is_saved_in_place`
 asserted `Removes([Comments: 3])` as if it were the design. It now
 asserts the Rewrite (the removal check's veto, asserted beside it,
 makes that right) and takes either `Removes(_)`, today's known
 miscount, or `RemovalCheck`, what the save gives once `judge`
 counts an adopted comment as kept; the doc comment says so.
   - **Notes and docs**: the open action note above said the property
 compared it, which was vacuous until now; `number::appended`'s and
 `numbers_named_near_the_last_refuse_the_append`'s docs said the
 fallback Rewrite numbers the file for itself, but there it stops
 too, and nothing can be saved (where new numbers start, past the
 highest named or around the named, is left for Greg). The proptest
 seed file is added explicitly (it was untracked).
 The Rewrite is untouched (the new walk runs only in an Append's
 closure), and the corpus didn't move: materialize_corpus's identity
 4,389 of 4,392 written, 2,269,722 surviving, 919,479 copied, 436
 changed by `keep_only`, 0 failed (the other five rows as before);
 append_corpus 3,770 appended, 259 rewritten (34, 14 and 211), 360
 asked, 0 failed; encrypted_corpus 573 kept by the Rewrite, 572 by
 the Append and one declined; deleted means deleted (ignored tier)
 the first Append declined by 28, 68, 68, 68, 0 and 30, 0 failed.
