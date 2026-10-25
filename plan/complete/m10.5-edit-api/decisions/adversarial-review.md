# M10.5: The adversarial review

Part of [the M10.5 decisions](README.md).


The merged design (synthesis-design.md and synthesis-meta.md) went
through a six-lens adversarial review. The lenses were code-truth,
history-soundness, pdf-correctness, future-fit, delivery-simplicity and
consumers.

- **136 findings:** 5 blocker, 65 major, 66 minor.
- **136 rulings:** 101 accepted, 25 accepted-modified, 9 deferred, 1
  rejected.

revised-design.md built in every accepted ruling, and the plan was
written from revised-design.md.

| Lens | Findings | Blocker / major / minor | Accepted / modified / deferred / rejected |
|---|---|---|---|
| code-truth | 18 | 0 / 7 / 11 | 14 / 4 / 0 / 0 |
| history-soundness | 19 | 3 / 9 / 7 | 14 / 3 / 2 / 0 |
| pdf-correctness | 29 | 1 / 14 / 14 | 24 / 5 / 0 / 0 |
| future-fit | 22 | 0 / 10 / 12 | 11 / 4 / 6 / 1 |
| delivery-simplicity | 21 | 0 / 10 / 11 | 18 / 3 / 0 / 0 |
| consumers | 27 | 1 / 15 / 11 | 20 / 6 / 1 / 0 |
| **Total** | **136** | **5 / 65 / 66** | **101 / 25 / 9 / 1** |

**How to read the tables:**
- **Where the counts come from.** They come from the `rulings` array in
  review-result.json. The revision summary's own tally is different: 93
  accepted, 30 accepted-modified, 12 deferred and 1 rejected. One of its
  12 deferred items, "M10 slice 9: lowering refusal of locked fields",
  is an accepted-modified ruling in the array
  (future-fit/mdp-checked-at-save-only). The tables follow the array.
- **Problems** were written against the synthesis. Slice numbers in the
  findings refer to its older 18-slice plan, so the one-line problems
  below mostly leave them out. "M10 slice N" always means a slice of
  M10.
- **Notes** use the revised design's S-numbers, which the plan kept as
  slice numbers (S7 is slice 7, the serializer). The rulings also cite
  revised-design section letters. In the plan:
  - O became §20;
  - R became proposals 11 and 13–40;
  - S's three questions became proposals 8–10 and the first open Ask;
  - N.2, N.3 and N.4 became the Transitional code ledger, Lint ledgers,
    and Tests, fixtures and gates;
  - N.6 became §19.
- **Duplicates.** Several lenses found the same issue. Most of the later
  rulings point back to the first ("same fix as", "duplicate of",
  "see"); the two agent-socket rulings were made separately:
  - the amend/VersionId hole (three lenses);
  - analysis renders that include generators;
  - removal-check encoding;
  - the agent socket;
  - object-stream packing.
- **The critic pass.** review-result.json also holds a later 40-issue
  critic pass over the written plan (`critic_issues`, `fix_summary`).
  All 40 fixes were applied to the plan. They aren't tabled here, but
  where they changed a ruling's detail, the plan's text wins (see the
  last subsection).

### code-truth

| id | sev | problem | ruling | note |
|---|---|---|---|---|
| insert-runs-vs-form-instance | major | Keying form identity on (source, copy) with a single-span InsertPages can't express both of M10 slice 4's rules: a file's page runs join one form, while the same bytes inserted twice make two forms. | accepted-modified | Every insert gets a fresh SourceId today. InsertPages/ReplacePages take a RangeSet and make one FormInst per op. No Join variant, since nothing produces it. Rename stems come from `PageSource::File.name`. The SourceId→BlobId switch moves to slice 11. |
| old-writer-deps-deleted-early | major | incremental.rs lives on but imports symbols that earlier slices delete, so those slices wouldn't compile. | accepted | incremental.rs and everything it imports are deleted together in slice 9. The legacy projection types are a ledger row from slice 8 to 9. Each slice runs `cargo check` on its deletions. |
| append-anyway-narrowed | major | "Append anyway" was offered only for removals, a regression from M10 slice 5: a fill in a signed hybrid form would force a Rewrite through DropXfa with no choice. | accepted | Offered for Removes, ReplacesText, TransformNeedsRewrite (DropXfa, DropUr3, FixSigFlags), RemovalCheck and RecoveredRemovals. Never offered for BaseUnsound, BaseObjectsMissing, FileChanged or NoSaveRecord. |
| recovery-drops-form-transforms | major | Recovery's base is the recovered bytes, so `rules::plan` sees no form change, and a recovered fill of a hybrid form keeps stale XFA and UR3. | accepted | Recovery sections apply the `rules::plan` transforms; the strategy rules still don't apply to them. Slice 9 check. |
| export-skips-form-rules | major | `export::run` had no `rules::plan` step and no XfaHoldsValues ask, so exports keep stale XFA and broken UR3. Save As was both an export and a Save. | accepted | `export::run` runs `rules::plan`. Preflight asks about signatures and XfaHoldsValues. Save As becomes a Save. The every-way-out test is in slice 19. |
| reachability-vs-templates | major | `/Names /Templates` pages, which keep_only keeps, are reachable but not in the target, so the reachability post-condition fails every save of such a form. | accepted-modified | Target pages are the page tree plus template pages. Widgets on template pages aren't modeled: keep_only keeps them, and `Terminal.outside` counts them. |
| combine-new-empty-loses-primary | major | Building Combine on `new_empty` drops the first file's catalog keys (/Lang, StructTree, XFA, /OutputIntents and others), and write-then-reopen loses its notices and the scripts-off state. | accepted | Combine opens the first file and commits one Tx: DeletePages, then InsertPages for each further file. The app hands the same Document to the new window. |
| slice45-notices-unmapped | minor | The four form notices from M10 slices 4–5, and Combine's, had no home in the design, and their helpers appeared in no slice's Deletes. | accepted | `Notice::{FormRenamed, SignatureNotValidHere, ScriptsOff}` go in `Commit.notices`. The helpers are deleted in slice 11, and the exact notice texts are slice 11 checks. |
| rules-plan-signature | minor | Deciding /SigFlags from the primary's SourceForm alone misses signed fields merged in from other files, and a standalone FixSigFlags changes today's rule. | accepted | The signature becomes `rules::plan(base, target, path, record, strategy)`. FixSigFlags is decided from the target's signed terminals, and only alongside a UR removal. |
| widget-record-loses-fields | minor | The Widget record drops border, actions, `has_appearance` and slot, which scripts-off and later M10 slices need. | accepted | Widget gains `props: Arc<WidgetProps { border, actions, has_appearance }>`, plus `merged` and `inst`. |
| orphan-deletions | minor | SourceInfo, `add_source`, `register_source`, `Version.sources` and the SourceId(u32)→BlobId switch had no deleting slice, and two source registries coexisted with no ledger row. | accepted-modified | Every symbol has an owner: slice 3 (SourceInfo's bytes, sizes and password), slice 11 (the registration layer, ledger row 3→11), slice 9 (`copy_of`, `reserve_page_objects`). Not slice 2 or 4 as proposed: an early switch would merge forms. |
| stale-refs-and-claims | minor | Line references had drifted, `Ids` was missing `next_field`, two claims were wrong, and the edits to M10's proposals 8 and 10 misnamed them. | accepted | The basis is refreshed at a0955fc, the claims are corrected, and the edits to proposals 8 and 10 are rewritten. |
| ledger-grep-collisions | minor | A bare grep for retired names (`subset`, `read`, `Mode`, `Step`, lopdf's `renumber_objects`) gives false positives. | accepted | tests/retired.rs lists qualified paths, matched by a `syn` walk of crates/ that excludes vendor/. Names the design reuses match only when qualified. |
| private-names-left | minor | Writers that survive still emit /SheafTick, /SheafStamp and /SheafStampText. | accepted | Ticks write `/Name /Tick` (slice 5). Page-stamp XObjects get PageRes names (slice 6), and /SheafStamp becomes read-only. Stamp-library files are outside the scan. Legacy names are handled by StripLegacy. |
| clippy-temp-dir-ban | minor | A workspace-wide clippy ban on `temp_dir` breaks sheaf-instance's legitimate socket-path fallback and some test helpers. | accepted-modified | Per-crate clippy.toml files for sheaf-doc, ops, render and app only, with tests allowed. sheaf-instance gets none. recent.rs and the attachments panel get `#[allow]` with a reason. Each ban lands when its last user goes. Since then sheaf-instance is gone, replaced by the instance-handoff crate, so its exception went with it; the four crates' bans are unchanged. |
| plugin-surface-widened | minor | With one AnnotId space, a straight port lets plugins delete other tools' comments and links, and read widgets (/AS) and foreign comment text. | accepted | The cap is computed from the resolved targets. Deleting sheaf marks is Plugin; others' comments and links need Approval, which 0.4 doesn't grant. `annotations()` never returns widgets. |
| agent-socket-overbuilt | minor | Adding a long-lived edit RPC to the one-shot handoff socket builds for a consumer that isn't designed yet. | accepted | The socket and MCP exposure are dropped. Transport, approval UI, `Actor::Agent` and plan handles go to the agent milestone. |
| watermark-image-rebind | minor | After reopen, an image watermark has no blob in the session (the file holds a re-encoded XObject), so it can't be updated. | accepted | ImageRef and PageForm are `Blob` or `Placed(ObjRef)`, rebound at open through the recognized block's resources. Slice 16 check. |

### history-soundness

| id | sev | problem | ruling | note |
|---|---|---|---|---|
| amend-breaks-version-identity | blocker | An amend changes the state behind an unchanged VersionId, so base == cursor passes for stale Txs: a dialog committed during OCR drops OCR pages, stale calculated totals commit, and long OCR runs write no sidecar. | accepted-modified | Not a revision counter: each amend mints a new VersionId. Stale commits validate decision reads, lowering reads and write before-values by value, and a mismatch is a Conflict. Path-key unions and BaseGone go. Slice 12 check. |
| amend-during-inflight-save | blocker | An OCR flush or keystroke during a save amends the version being saved. `record_save` then marks it saved, so the edit is neither in the file nor dirty. | accepted | `save_input` seals the version, and amends refuse sealed versions. Prune and rollback never remove sealed or saved versions. `record_save` and `abandon_save` unseal. Slice 9 check. |
| blob-liveness-by-id | blocker | Records hold a Copy BlobId, not a handle, so a blob dies when its caller drops it (a watermark image, `--with` files), and re-interning the same bytes builds a second Source. | accepted | Records hold BlobRef/Src handles (Eq/Hash by id, serialized as the id), and Source holds its Blob. `Tx::pin` and `Session::intern` return pinned Blobs. Slice 8 check. |
| append-base-bytes-unspecified | major | The second Append's base was unspecified: built on a Source's bytes, the first Append's section is lost and the file is corrupt, and temp+rename copies the whole file on every Append. | accepted | The base is the saved file on disk. Appends go in place after a stamp re-check, with a journal. CopyOf uses a reflink or a verified copy. Slice 10 check: three Appends, then `qpdf --check`. |
| append-emit-scope-not-dependency-closed | major | An Append that emits only the changed keys leaves dependent objects stale: /PageLabels after a move, links to pages inserted later, live-token blocks, widget APs after a DA/DR change. | accepted | An `emit_deps` registry function with a dependency table, guarded by a property test that a default-run Append and a Rewrite reopen to the same facts. |
| ifdisjoint-phantoms | major | Key-level optimistic concurrency misses phantoms: a generator scoped to a page being deleted, dangling bookmarks, duplicate /NM. | accepted-modified | After a Validate merge, the cascade and uniqueness rules re-run over the written keys, and any violation is a Conflict. Relower avoids phantoms by lowering on the cursor. Synthetic read keys were rejected. Slice 12 checks. |
| plugin-and-ui-reads-unrecorded | major | WIT reads and UI-macro reads that drive decisions aren't in the read set, so a plugin's stale subtotal commits without a conflict. | accepted | Every WIT read records a decision read by value, and UI macros read through `Tx::reader`. The subtotal scenario is a slice 20 check. |
| render-derived-payload-reads | major | OCR and Trim White render pages with generators and widgets, so unsaved header or watermark text enters OCR layers and survives the generator's removal. | accepted-modified | Analysis renders use `content_only()`: base, filters and static under-layers. OCR uses the render-time transform, so it reads only `Page(p)`. |
| memory-budget-unenforceable | major | Prims pin their payload Arcs, so dropping snapshots frees little, and without a save the version line is never pruned (about 2.4 GB for 200 re-OCRs). | accepted | `snapshot_bytes` is split from `step_bytes`, and the byte budget prunes versions. A detached saved anchor lets the root pass the saved version. Moved to slice 12, with a 200-version check. |
| file-changed-on-disk-silently-overwritten | major | A Rewrite silently replaced a file another program had changed, because the stamp was checked only at plan time. | accepted | Ask::FileChanged offers Save As, Overwrite and Cancel. The stamp is re-checked just before the rename or in-place append. Temp names are unique and permissions are kept. Slices 7 and 10. |
| groupended-contradiction | major | The GroupEnded rule either stops OCR on any user edit or lets concurrent groups alternate into hundreds of versions. | accepted | GroupEnded holds when the group's latest version isn't an ancestor-or-self of the cursor. Interleaving fragments the group, and `revert_group` undoes the fragments in one step. Slice 12 test. |
| rollback-group-partial-and-saves | major | A rollback after a trap does nothing if a later commit sits on top, and it can remove a saved version or one being saved. | accepted | Truncate only a childless, unsaved, unsealed cursor. Otherwise revert with inverse prims validated by value, keeping the work on a conflict. Slices 12 and 20. |
| recovery-sidecar-lifecycle | minor | After `record_save` the stale sidecar is neither rewritten nor deleted, and a restored document writes none, so a second crash loses the work done after recovery. | accepted | `record_save` rewrites or deletes the sidecar synchronously. Autosave is keyed on the version token and waits for saves. Recovered documents chain Append sidecars on the same disk base. |
| saved-pins-and-lost-saved | minor | Every version ever saved pins its snapshot and SaveRecord forever, and prune can drop the saved version with no safe fallback stated. | accepted | Only saved and sealed versions are pinned, and older records shrink to SaveNotes. A detached anchor; NoSaveRecord means a safe Rewrite. |
| signing-outside-model | minor | The signature's /V, /AP, /Perms and /SigFlags are written by a save rather than a Put, so later Appends drop them and the planner can't see signatures made this session. | deferred-to-M12 | Nothing signs in M10.5, and the hooks were cut. §20 records the rule: a SignField commit, a reserved Append, a SetSignatureValue commit, and `SaveRecord.signatures` feeding SigImpact. |
| removal-check-encoding-and-marker | minor | Comparing raw operand bytes is blind to encoding: subset fonts cause false vetoes, and identical bytes can be different text. /D meant two different digests. | accepted | Derived objects are judged on their record inputs; source objects on decoded NFC word multisets, where an unknown encoding counts as different. Subsets keep GIDs. Separate /L and /R string digests. |
| net-identity-and-unchanged | minor | A batch or amend that nets to nothing leaves a dirty version, and an Unchanged save doesn't move `saved`. | accepted | Prims are coalesced per key at commit and at amend. An amend that nets to nothing removes its version. An Unchanged plan attaches the save record. |
| agent-revert-is-jump | minor | Reverting an agent's turn by jumping back also moves later user edits into a redo branch. | deferred-to-agent | Nothing in M10.5 produces it, but `revert_group` exists. §20 records: revert the turn's versions, and offer a jump only when nothing came after. |
| positions-and-lines-under-merge | minor | Positions-based dialog Txs conflict with every page move, because Order is one key, and Ids-only ops merged over a reorder keep wrong describe lines. | accepted | Dialogs resolve positions to ids when they open, so they never read Order. Lines are recomputed against the merged pre-state. |

### pdf-correctness

| id | sev | problem | ruling | note |
|---|---|---|---|---|
| objstm-packing-double-encrypts | blocker | Packing into object streams would encrypt member strings twice (ISO 32000-1 7.5.7), garbling encrypted xref-stream files, and the other 7.5.7 exclusions were unlisted. | accepted-modified | M10.5 writes no object streams; packing goes to M14 with `packable()` and the exclusions recorded. Members are re-serialized as plain objects, each encrypted once. Slice 7 adds an AES-256 xref-stream fixture. |
| raw-passthrough-conditions | major | Raw-byte passthrough is valid only with the same (num, gen), key and ID, and the primary source. Dense Extract/Split, unencrypted Print and objects keep_only edited would break. | accepted | Five conditions: Stable numbering; the primary source; the same Crypt key and ID[0]; untouched by emit and by keep_only (which now reports what it modified); not an ObjStm member, with matching gen. |
| q-isolation-formula | major | The q/Q isolation count was backwards, and a net count can't handle bases like `Q q`; the minimum prefix depth is needed. | accepted | `Balance { net, min_prefix }`: open = 1+d, close = 1+d+net. A base that can't be lexed fails the op. Pixel tests for `q…`, `Q q` and `Q`. |
| merged-field-widget-objkey | major | Merged field/widget dicts, the form most single-widget fields take, map to two ObjKeys at one object number, so a fill defines the object twice. Duplicate orphans the widget. | accepted | `Widget.merged`. The pair is emitted as one `ObjKey::Field` object with Annot aliased to it, and copies stay merged. Slice 9 check. |
| resources-left-after-artifact-removal | major | Splicing out Acrobat's `/Artifact … Do … EMC` leaves the Form XObject in /Resources, so the removed header or watermark text survives in the file. | accepted | PageRes leaves out names used only inside dropped ranges, and the unlinked objects go to the removal check. Slice 16 check against Acrobat's watermark. |
| pagination-artifacts-overmatch | major | Matching /Pagination Header/Footer artifacts by type alone deletes running heads authored in Word, LibreOffice and InDesign. | accepted | PagOrigin is Sheaf, SheafLegacy, Acrobat (by PieceInfo ADBE_CompoundType) or Authored. The Set ops drop only Acrobat's. The Remove ops report counts per origin and touch authored artifacts only with `include_authored`. |
| private-strip-impossible-in-content | major | /SheafGen and /SheafOCR sit inside content bytes that no filter can rewrite, so Clean Copy either loses the header or OCR text or fails the private-data check. | accepted | The markers move into named property lists (/ShP\<n\>). A StripLegacy filter rewrites legacy inline lists and resource names. All legacy names are in `private::KEYS`. |
| inheritance-on-reparent | major | A page placed under a new parent inherits that parent's /Rotate, /CropBox, /Resources or /MediaBox, and splitting at 128 kids conflicts with flat 7k-kid files. | accepted | All four inheritable keys are written explicitly when a page leaves its source parent chain, /Rotate 0 included. A node splits only past max(128, its size at open). Slice 9 fixture. |
| overlay-delta-numbering | major | Per-page overlay deltas cached independently allocate colliding object numbers and duplicate shared objects (/Pages, AcroForm), and RC4/AES ciphertext is bound to its number. | accepted | One session-wide overlay allocator per source. Cached deltas are symbolic and unencrypted, deduplicated by ObjKey, with one /Pages and one AcroForm per group. Encryption happens at assembly. Slice 13. |
| ur3-drop-on-append | major | Removing UR3 from /Perms in a form-fill Append is needless (TransformParams allow fills), and the Catalog change silently breaks certification. | accepted | DropUr3 applies on every Rewrite, but on an Append only when the path falls outside UR3's TransformParams. Catalog and AcroForm transforms count as OTHER, so certified files ask. Slice 10. |
| sigclass-pageannots-objectmap | minor | PageAnnots was unclassified (P3 files would ask on every comment), "appearance inputs" was vague, and FieldMDP missed All, Exclude and /Lock /P. | accepted-modified | PageAnnots is classified by its membership delta: ANNOT for marks and others' annotations, OTHER for widgets. A widget change is FILL only for /AS and value-derived APs. FieldMDP covers All, Include, Exclude and /Lock /P. Object-level page diffs go to M12. |
| block-marker-integers | major | Ids and digests up to 2^53 (and u128) written as PDF integers exceed Annex C's range and lose precision when PDFium re-saves. /D had two meanings. | accepted | Ids and digests are written as strings (/G, /L, /R, /N). An ignored slice 16 test re-saves through `FPDF_SaveAsCopy`. |
| annot-digest-canonical-and-nm | major | A digest over raw values breaks when another tool re-serializes. /NM is unique only per page, so copied marks collide. v1 conversion used the current /Rotate. | accepted | A canonical digest (1/64 pt, NFC, normalized /Rect). A duplicate `sheaf:<id>` gets a fresh id and a notice. `AnnotRef::Nm { nm, page? }` fails as Ambiguous on duplicates. v1 geometry comes from the standard keys. |
| foreign-text-edit-stale-rc-ap | major | Changing a foreign annotation's /Contents while keeping its /RC and /AP leaves the old text visible in Acrobat and in the file, and adoption drops /RC, /DS, /RD, /IT and /Measure. | accepted-modified | In M10.5, others' annotations can't have their text edited, only be deleted or adopted. Adopted and src-backed marks keep /DS, /RD, /IT and /Measure, drop /RC when /Contents changes, and always derive /AP. |
| generator-inputs-not-restorable | major | After reopen, a generator's image or PDF blob is gone, and a Pages scope can't be rebuilt because PageIds aren't in the file. | accepted | Images and page forms rebind through the recognized blocks (Placed). Pages scopes are rebuilt from blocks, with empty marked blocks on covered pages where nothing was drawn. Slice 16. |
| isolation-and-legacy-ocr-q-markers | minor | Unmarked QOpen/QClose streams become base content after reopen and nest one level deeper per cycle; the finding said legacy OCR strip leaves an orphan q. | accepted-modified | Isolation streams are marked (`/SheafBlock /K /Isolate`) and set aside before reassembly; slice 16 five-cycle check. The legacy claim is corrected: the legacy q stream is marked /SheafOcrLayer and is already stripped. |
| splice-range-safety | minor | Dropping a marked range inside BT…ET, or one holding unbalanced q or state operators, shifts or restyles the text that follows. | accepted | A drop is accepted only if it balances on its own, or lies inside one BT…ET with no text-state or positioning operators; otherwise `EditError::Content`. Pixel checks in slices 6 and 16. |
| inline-image-lexing | minor | Ending inline images by /L or ` EI ` differs from PDFium, so splices could silently corrupt pages. | accepted | The lexer follows PDFium's ReadInlineStream rules, with an ignored differential test against PDFium's page objects. |
| merged-streams-marked-sel | minor | Once another tool merges /Contents, StreamSel no longer finds sheaf's blocks, so ClearOcrText and generator updates fail or overreach. | accepted | MarkedSel gains `SheafGen(GenId)` and `SheafOcr`, and PageBlocks finds sheaf blocks by marker inside merged streams. |
| realizer-rotate-double | minor | A realized page's overlay dict carries the new /Rotate, and the rotation delta is applied again on top, so the page rotates twice. | accepted | Overlays use `RotatePolicy::KeepBase`. Slice 13 check. |
| removal-check-old-side | minor | Re-emitting the saved state isn't what's on disk (keep_only edits, font subsets), "still referenced" has no index, and the TextBag misses Form XObjects, /RC, JavaScript, XFA and XMP. | accepted | The old side is read from the saved file through LazyPdf, after the stamp check. A reference the check can't resolve counts as unlinked. The TextBag covers every reachable stream, and subsets keep GIDs. |
| header-offset | minor | In files with junk before `%PDF-`, offsets must be relative to the header, but appended offsets and `raw_span` were absolute. | accepted | XrefFacts records the header offset, appended offsets are written relative to it, and `raw_span` is translated by it. Fixtures in slices 3 and 7. |
| eff-crypt-filter | minor | /EFF, the crypt filter for embedded files, isn't supported, so attachment-only encryption is decrypted and encrypted wrongly. | accepted | /EFF is parsed into Crypt and used for EmbeddedFile streams in LazyPdf and crypt.rs. Slice 3 fixture. |
| conformance-claims | major | Stamping or watermarking a PDF/A or PDF/UA file breaks the claim while keeping it, and nothing checks claims. | accepted-modified | A `Source::conformance()` fact, and a planner Note when a change breaks a claim. Font embedding, the no-transparency rule and claim removal go to M14. |
| version-bump | minor | Emitted features (transparency, xref streams, AES, artifact /Subtype) can need a newer PDF version than the base declares. | accepted | The emitter accumulates the version its features need. A Rewrite writes it in the header; an Append sets Catalog /Version, which counts as OTHER for signatures. Slice 16. |
| xref-table-shape | minor | The xref shape of a full write and the generations of deleted entries were unspecified, and NaN, infinities and out-of-range reals could be written. | accepted | One [0, Size) subsection with a linked free list, or an xref stream with /Index. Objects deleted in an Append get generation + 1. The real writer refuses non-finite values and clamps magnitudes. |
| append-durability-cost | minor | Temp+rename copies the whole base on every Append (about 500 MB on a large scan), and hashing only the last 64 KiB misses changes mid-file. | accepted | Appends are written in place with a journal, and FileStamp adds a hash of the bytes at the startxref target. Gate: an incremental save under 50 ms on the 500 MB scan. |
| aes-iv-determinism | minor | Deriving the AES IV from the ObjKey alone reuses the same IV and key for different plaintexts across saves. | accepted | The IV is the first 16 bytes of HMAC-SHA256(file key, ObjKey ‖ plaintext). |
| copy-structparents-and-dup-kids | minor | Copied pages keep /StructParents, so two pages claim one ParentTree key, and a page object listed twice gives two copy-0 PageDefs. | accepted | /StructParents is dropped on copies and on pages from other structure trees, with a Note. A page listed twice opens as copy 0, 1, … with a notice. |

### future-fit

| id | sev | problem | ruling | note |
|---|---|---|---|---|
| amend-reuses-version-id | major | The same amend/VersionId hole: a crop during OCR drops the flushed pages, and M10's calculation cascades and plugin calls hit it too. | accepted-modified | Same fix as amend-breaks-version-identity: amends mint new ids rather than bumping an epoch, and commits validate by value. The commit-rule property test includes amends. |
| formref-ap-dangles-after-flatten | major | A flattened annotation's `FormRef::Ap` points at a record the flatten deletes, so emit, the realizer and recovery can't derive its appearance. | accepted | `FormRef::Pinned(Arc<PinnedAp>)` captures the appearance inputs at flatten. New invariant: no ContentStack references an AnnotId or FieldId. |
| keypatch-holds-object-numbers | major | KeyPatch values are lopdf Objects that can hold raw object numbers (stale under Dense, unable to name an ObjKey or PageId), and FieldNode has no patch at all. | accepted-modified | KeyPatch is removed from M10.5, since nothing produces it. M10 slice 10 adds it for /Tabs, with symbolic PatchValue and role rows. FieldNode patches arrive in M10 slices 23–26, `AnnotBody::Link` in M13. |
| signing-not-derivable-and-invisible | major | A signed file isn't a function of (state, sources), the planner can't see signatures sheaf made this session, and `Value::Signed` carries no data. | deferred-to-M12 | §20 records: two commits around one Append, `SaveRecord.signatures`, signatures with a payload, and signing only when the plan is an Append. |
| veto-rewrites-signed-without-ask | major | When the removal-check backstop vetoes an Append on a signed file, the fallback Rewrite breaks the signatures without the ask ever being shown. | accepted | On any document with a known signature, a veto stops at `Ask::Signatures { reason: RemovalCheck }`. Only unsigned documents fall back to a Rewrite, and the CLI exits 3. Slice 10 P2 check. |
| textbag-compares-encoded-operands | major | Comparing raw operands vetoes routine changes: re-wrapped FreeText, a fresh CJK subset per materialization, re-subsetting in M13. | accepted | Same fix as removal-check-encoding-and-marker. |
| transforms-not-streaming | major | Whole-document transforms (redaction, sanitize, optimize) hold every rewritten byte in memory twice, with no progress or cancel. | accepted-modified | M10.5 adds `SymObject::Deferred` bodies produced inside serialize, progress and cancel through lowering and emit, and a slice 19 peak-memory gate. `Rewrite::Redact`/`Edit` and PageSel on ApplyRedactions are recorded for M11. |
| root-key-ownership-and-undo | major | Root keys owned only while patched fall back to the written roots after an undo, so an undone /CO order or catalog patch is never saved. | accepted | One static ownership table per root object. Owned keys always come from the target state, unowned keys from the written roots. Slice 9 tests: set, Append, undo, Append. |
| save-as-breaks-signatures | major | Save As always rewrote, breaking signatures even when the file was unmodified or only form-filled. Acrobat copies the file and appends. | accepted | Save As of a signed document uses `Strategy::Append { onto: CopyOf }`. Slice 10. |
| leak-check-over-docstate | major | A text inventory over DocState misses most of what M11's leak check must cover: Info and XMP, names, URIs, /Alt, JavaScript, /SheafAnnot. | accepted | `text_inventory` leaves the registry list. M11's leak check runs over the output; a DocState pass (`modeled_text`) is only a fast pre-check. |
| struct-parents-on-inserted-pages | minor | Inserted or duplicated pages keep /StructParents, so their content is attributed to the wrong structure elements. | accepted | /StructParents and annotations' /StructParent are dropped on pages outside the written structure tree, with a Note. The M14 plan (`Key::StructElem`, `PlaceTag::Struct`) is recorded. |
| sigclass-total-order | minor | A totally ordered SigClass can't express signing (allowed at P2 and P3, not P1) or DSS and timestamps (allowed after P1). | accepted | SigClass becomes bitflags (FILL, ANNOT, OTHER) with per-P allow masks. M12 adds SIGN and LTV. |
| changes-since-signing-objkey | minor | ObjKeys exist only for this session's writes, so "changes since signing" must classify later revisions by their content. | deferred-to-M12 | A content-based role classifier over later revisions is recorded. The ObjKey mapping is only a test oracle for sheaf's own Appends. |
| security-two-homes | minor | A binder SetSecurity op gives encryption two sources of truth, spreads unzeroized passwords through history and JSON, and breaks recovery. | deferred-to-M11 | Rule recorded: `Key::Security` secrets are zeroized, redacted, and kept out of `Step.op`, describe lines and serde. Binder saves use FromDocument, New is for exports, and recovery is written in Full form when the encryption changed. |
| transforms-need-host-phase | minor | Some transforms need PDFium or search before they can lower (M11 image redaction and verification, M15 pattern selectors), and there's no stage to run them in. | accepted | A host resolve stage (Snapshot, EnginePool, Progress) produces resolved ops and decision reads before pure lowering, in Tx and in `export::run`. |
| m15-steps-and-caps | minor | M15's batch steps are jobs and exports, not Ops, so building them on the plugin API would cross the Transform type wall. | deferred-to-M15 | The Step schema (Op, Job or Export) and `Actor::Batch` are recorded. If ApplyRedactions later moves into Op, it gets `Cap::User`. |
| content-stack-rewrite-semantics | minor | How rewrites and filters compose, how an added image is placed, and how an M13 drag hides content were all undefined. | deferred-to-M11/M13 | Rules recorded: a rewrite is the new base and absorbs the current filters (M11); `Static::Image`, a unified object query and a content hide set (M13). |
| pdfa-fonts-not-profile-input | minor | Font choice is bound at derivation and isn't a Profile input, so a PDF/A export can't re-derive appearances with embedded fonts. | deferred-to-M14 | The font is already an explicit appearance input. `Profile.fonts` and Liberation substitutes are recorded for M14. |
| save-optimized-ends-history | minor | Optimized output can't be an Append base, so Save Optimized must reopen the file, which silently ends undo history. | accepted | §20 states it: Save Optimized writes an export and reopens it as a new session, with a notice that history is cleared. Otherwise only Save As Optimized is offered. |
| mdp-checked-at-save-only | minor | DocMDP and FieldMDP were checked only at save time, so locked fields stay editable until then. | accepted-modified | `Tx::preview` reports DocMDP consequences per op from M10.5. Lowering-time refusal of locked and signed fields comes with the first field-editing ops, in M10 slice 9. |
| incoming-signed-fields-and-templates | minor | A merged signed field shows as signed but is invalid, and spawning pages from templates has no PageSource. | rejected | Unsigning them contradicts M10 slice 5's committed, tested behaviour. They're kept with the SignatureNotValidHere notice and can be flattened with `FormBring::Flatten` (M10 slice 15). `PageSource::Template` goes to M10 slices 19–22. |
| m10-sequencing | minor | Several M10 slices depend on M10.5 slices that would trail, and the plan didn't say those land first. | accepted | M10 resumes only after all 20 slices, and nothing trails. This follows Greg's decided sequencing. |

### delivery-simplicity

| id | sev | problem | ruling | note |
|---|---|---|---|---|
| points-and-sizes | major | The slice table summed to 50 points, not the 46 claimed, and several slices were undersized; an honest total was about 58–62. | accepted | Re-sized to 20 slices and about 68 points. Records and ops are XL. The serializer is split from Rewrite, and the realizer from the text service. The cuts are applied. |
| projection-window | major | The legacy projection fed both save paths for six slices while the representation under it changed twice. | accepted-modified | Reordered beyond the proposal: serializer in slice 7, records 8, Rewrite 9, Append 10, ops 11. The projection and the records-adapted `rebase_to_saved` live one slice, and always-Rewrite saves one slice. |
| ledger-gaps | major | The transitional ledger left out: SourceInfo and the Store coexisting; `rebase_to_saved` and compute on v2; subset and print; the eager load; the WIT 0.2 adapter; spike code. | accepted | A ledger row for each, such as registrations 3→11 and the derived record 3→17. The SourceId switch is in slice 11. |
| ledger-and-gates-last | major | The retired-symbols ledger and the 7k gates arrived in the last slice, after everything they enforce. | accepted | tests/retired.rs lands in slice 1 and the gate harness in slice 2. Each slice updates its docs section. The M10 re-plan is written in slice 20, and old slice 18 is dissolved. |
| kept-port | major | Porting keep_only (1,144 lines) to a symbolic Graph is large and not shown to be needed: a lopdf rewrite of the 7k file takes 0.49 s. It also contradicts H.5. | accepted | keep_only runs unchanged on an eager graph and returns the ids it modified. H.5 is restated with the pruning exception. A symbolic port happens only if the slice 9 gate fails. |
| weak-7k-fixture | major | syn7000.pdf (3.5 MB, one font, no annotations, forms or object streams) can't exercise the risks the design names, and several gates had no threshold. | accepted | Slice 2 adds a heavy 7k fixture and a 500 MB scan, and every gate has a threshold. |
| whole-doc-render-invalidation | major | Any generator puts PageCtx in every covered page's render key, so a reorder re-realizes everything. The cost table was wrong and the thumbnail gate missing. | accepted | `Generator::needs()` limits render-key inputs. The cost table and checks are corrected, a thumbnail fill-rate gate is added, and spike C tests box edits on the base engine. |
| agent-socket-yagni | major | An edit endpoint on the instance socket means a protocol bump, a new local write surface, and no client to test against. | accepted | The socket and the agent checks are dropped. `Session::preview`/`commit` stay for the CLI. |
| open-questions-triage | major | Most of the 17 open questions were already settled by Greg's rules or the status quo; only three are his call. | accepted | Three questions remain: live or frozen page tokens, strict removal, and export encryption without a checkbox. The rest are settled as proposals. Fixtures move to a material-requested list. Marks and generators share one '?' interim rule. |
| tighter-plan | major | Taken together: the adapter lives as long as possible, enforcement comes last, about 6 points have no producer, and the gates can't fail. | accepted-modified | Its principles are kept: ledger and gates first, honest sizes, the cuts, a short projection. The order differs, and the plan is 20 slices and 68 points because the review added scope. |
| m12-hooks-yagni | minor | The M12 hooks (Reservation, `Output.holes`/`offsets`, `revisions()`) have no producer or consumer in M10.5. | accepted | H.11 is dropped. Revision reading stays only where forms' EarlierValues uses it, and M12's needs are recorded in §20. |
| shared-object-machinery | minor | The apply-time copy-on-write check is infeasible, because apply doesn't emit, and it's redundant. `Source::users` has no other consumer. | accepted | `Source::users`, the users index and the apply-time check are dropped. Copy-on-write is enforced as a post-condition. |
| objstm-packing | minor | Object-stream packing is a new capability, not a fix, and it would land in the biggest slice. | accepted | No object streams are written in M10.5; packing goes to M14. |
| snapshot-check-unpassable | minor | `snapshot()` computes dirty flags in O(pages + fields) until they're deleted, so the O(1) check can't pass earlier. | accepted | Snapshot is O(1) once slice 9 deletes the dirty flags. SaveInput is computed only on the save path, and the check is in slice 12. |
| groups-without-producers | minor | The Slider, Drag and Typing group kinds have no producer in M10.5. | accepted | GroupKind is Ocr or PluginCall, and M10 slice 11 adds Typing. The no-unproduced rule covers every enum. |
| unproduced-variants | minor | `BatesStart::Continue`, flatten-on-insert, FormScript, `text_inventory` and `Actor::Agent` have no M10.5 producer but add JSON surface. | accepted | Removed, along with `SourceRef::Named`. `BlobSel::With` stays, because the CLI's `--with` produces it (slice 19). |
| clippy-ban-timing | minor | When the clippy bans landed, their targets still had live users: `generated::*`, `temp_dir`, lopdf content round trips. | accepted | Each ban lands in the slice that removes its last user, through per-crate clippy.toml files. |
| m10-resume-trailing | minor | Letting M10.5 slices trail M10's resumption creates two dependent workstreams on main. | accepted | M10.5 finishes before M10 resumes; nothing trails. |
| spike-criteria | minor | The spikes had no pass/fail criteria, shared a commit with production code, and had no fallbacks for linearized, hybrid or xref-stream bases. | accepted | The spikes are their own commit under bench/, with written criteria and fallbacks per base class. The verdicts are recorded before the records slice (8). |
| open-builds-old-shape | minor | The open slice had nowhere to put foreign annotations, links or watermarks, and recognizing Acrobat's blocks at open would lex every page. | accepted | Slice 4 builds only today's shape. Foreign records, links and recognition come with records, and open never lexes content. |
| concept-count | minor | About 140 new named concepts against about 75 removed, and about 20 of the new ones have no M10.5 producer. | accepted-modified | A type ledger is added (plan §19), and the nothing-unproduced rule covers every public type and variant. No numeric target is set. |

### consumers

| id | sev | problem | ruling | note |
|---|---|---|---|---|
| amend-keeps-version-id | blocker | The amend hole as consumers see it: dialogs during OCR, M10 typing calculations, the agent's Refuse and plugin caches all see an unchanged id. | accepted-modified | Duplicate of amend-breaks-version-identity: amends mint new ids, and commits validate by value. |
| wit-reads-not-in-read-set | major | Plugin decisions made from WIT reads aren't in the read set, so stale values commit, and the agent's Refuse policy can't write during any background work. | accepted-modified | WIT reads record decision reads by value, and plugin calls commit with Relower after validating them. The agent part goes to its milestone: per-session decision reads, then Validate. |
| hot-keys-conflict-storms | major | Single keys (Order, OutlineRoots, Form, Labels, PageAnnots) make unrelated concurrent work conflict, so for example a slow insert is refused because of any thumbnail drag. | accepted-modified | `Stale::Relower` with pinned ids for host planners and plugin calls. Dialogs resolve positions when they open and re-lower previews on each ChangeSet. Three-way merges of ordered keys were rejected. |
| tx-mode-runtime-caps-in-commit | major | Export-only Transforms share the Tx type through a runtime mode, caps are checked after lowering against an actor the caller supplies, and `DocView::commit` throws the error away. | accepted | Tx accepts only Op. ExportTx accepts Transform and is consumed only by `export::run`. Caps are checked in `Tx::apply` before lowering, against the actor `Document::lower_cx` binds. `DocView::commit` returns Result. |
| plugin-rollback-interleaved | major | If the user edits during a plugin call, rollback after a trap either drops the user's edit or keeps the plugin's. The next Tx's base and the version's label were unspecified. | accepted | After a mid-call commit, the next Tx is based on that version. Rollback truncates or reverts, and the label is the first submit's. Slice 20 interleaved test. |
| plugin-tx-built-at-enqueue | minor | A queued plugin call builds its Tx when it's enqueued, so it starts from a stale base, worse after a `ui.show-form` wait. | accepted | The latest snapshot sits in an ArcSwap cell, and the call's Tx is built lazily at its first read or submit. |
| identity-guarantee-false | major | Expanding `<<name>>` under read-your-writes, plus author fields, leaks the user's identity to plugins while the test still passes. | accepted | Option (b): without the identity permission, tokens stay literal and authors appear as roles. Marks a plugin creates are authored by the plugin. Slice 20 tests. |
| r98-values-leak | major | In 0.4, renders, text, OCR and widget /AS reads expose every field value before the planned 0.5 capability exists. | accepted-modified | In 0.4, `annotations()` never returns widgets and text leaves out field display text. Render gating (`FormDraw::Redacted`) goes to 0.5's fields capability (M10 slice 29), and gating for the agent to the agent milestone. |
| blob-refs-in-plugin-ops | major | Raw BlobIds in plugin ops let a plugin probe the Store or re-embed blobs kept only by undo, and a paste in another window names blobs missing from that Store. | accepted | Plugins name blobs through call-scoped handles from `assets.upload`, and raw BlobIds from plugins are refused. The clipboard carries blobs in a process-wide cache, interned on paste. |
| caps-static-per-op | minor | Caps fixed per op let any plugin silently delete other tools' comments and links. | accepted | The cap is computed per (op, target kind), with a totality test. |
| changed-latest-wins-drops-scopes | major | Latest-wins coalescing of change events drops the scopes of the discarded events, so plugin caches go silently stale. | accepted | Coalescing merges events: earliest parent, latest version, union of scopes, and the cause "coalesced" when causes differ. |
| wit-exposes-internal-keys | minor | Putting Key in WIT change-info freezes the internal conflict granularity as public API. | accepted | change-info carries semantic scopes mapped from the ChangeSet, and Key stays internal. |
| undo-fighting-plugins | minor | Undo and redo are depth 0, so an event plugin that "fixes" the document re-applies its edit after every undo. | accepted | Edits submitted from `on_event` after an Undo, Redo, Jump, Rollback or Revert are refused with an edit-refused event. Slice 20 test. |
| cli-cannot-address | major | Session-only ids (PageId, OutlineId, FieldId, BlobId hashes) can't be named from the CLI or M15 JSON, so `sheaf edit` can only do whole-range page ops. | accepted | JSON addresses pages by position, bookmarks by title path, annotations by /NM and blobs by `--with` name. `sheaf inspect --json` is added, and created objects print as positions, /NM names and paths. |
| cli-force-overloaded-and-save-as-append | major | `--force` already means "overwrite", so reusing it to break signatures is dangerous. There were no XFA or password flags, and `-o` always rewrites a signed form. | accepted | `--force` means only overwrite. Added `--signatures`, `--xfa`, `--password` and `--with NAME=PATH[:ENV]`. Exit 3 means a decision is needed. `-o --append` copies and appends. |
| positions-against-base | minor | In a batch, later ops' positions refer to the base, not to the state after the earlier ops. | accepted | Positions resolve against the pre-op state, and `--dry-run` prints the resolved pages. |
| agent-entrypoint-miswired | major | Session hard-codes the Cli actor and preview keeps no Tx, so the agent's approval flow lowers twice and commits something other than what was approved. | deferred-to-agent | `Document::plan` and `commit(plan)`, with plans expiring on a conflict, are recorded in §20. Nothing agent-facing is built in M10.5. |
| run-job-one-at-a-time | major | Running hours-long OCR through `run_job` blocks every insert, header/footer, trim and import for the whole run. | accepted | OCR keeps its own run, and short planners run alongside it. The one-at-a-time rule applies only to jobs that write files; saves are serialized by the seal. |
| content-dialog-lowering-on-ui | major | Header and watermark dialogs re-lower on every keystroke, which needs PageBlocks for every page on the UI thread, and filters decided at apply time miss pages inserted later. | accepted | `Generator.replaces` is resolved at emit from the memoized blocks fact, so lowering is O(1) and later pages are covered. Previews lower in the background. |
| analysis-renders-include-generators | major | OCR, Trim White and blank detection see unsaved generators, which leaks removed header text into OCR layers and makes blank pages look non-blank. | accepted | Same fix as render-derived-payload-reads. |
| pagespace-at-the-edges | major | PageSpace leaks to the edges: pasting across differently framed pages, MoveMarks deltas, and plugin or agent placement all need /Rotate and CropBox math. | accepted | The clipboard stores geometry in the view frame, MoveMarks takes `Delta::View`, page-info carries view matrices, and plugin-api has helpers. |
| op-json-versioning-gaps | minor | WIT submit has no schema version, defaults for new fields are unstated, an untagged Nm swallows typos, Ref::Nm can be ambiguous, and a u128 BlobId is unsafe in JSON. | accepted | The manifest declares `edit-schema`, with an envelope. Later fields are always defaulted, enforced by a schemars snapshot. Refs are per kind with a custom deserializer, a duplicate match is Ambiguous, and BlobIds are hex strings. |
| renderkey-and-pool-churn | minor | Drag and filter state in render keys re-render pages for nothing, and background analysis evicts the visible pages' engines. | accepted | The ViewInputs digest covers only what the realization draws. Engines for visible pages are pinned, and background consumers get their own budget. |
| combine-still-reopens | minor | Combine still writes a file and reopens it, losing the rename and scripts-off notices and the undo history. | accepted-modified | The app saves the combined document and hands the same Document to a new window. Combine stays a binder flow on the first file (per combine-new-empty-loses-primary). |
| can-apply-full-lowering | minor | `can_apply` runs full lowering and allocates ids every time a menu re-renders. | accepted | `Tx::check` resolves references and checks caps only: no puts, cascades or ids. Slice 11 check. |
| live-token-unencodable-later | minor | A later label change can make a header's live token unencodable, so emit fails and the document can't be saved. | accepted-modified | Emit is total: unencodable glyphs draw '?' with a notice until M10 slice 7, so no cascade validation is needed. Slice 17 check. |
| loading-gaps | minor | A lazy /Annots fallback brings back late-arriving facts outside history, plugin commands during Loading get "no document", and there's no Document to return NotReady. | accepted | No lazy /Annots fallback: a slow file waits longer, with progress. Plugin calls are queued and plugin commands greyed until ready. `Session::open` returns after finish. |

### Deferred

**The nine deferred findings.** The plan records all of them in §20 (the
revised design's section O) and lists them under "Not in M10.5".

- **M12:**
  - history-soundness/signing-outside-model
  - future-fit/signing-not-derivable-and-invisible
  - future-fit/changes-since-signing-objkey
- **M11:** future-fit/security-two-homes
- **M11 and M13:** future-fit/content-stack-rewrite-semantics. The rule
  that a rewrite is the new base goes to M11. `Static::Image`, the
  object query and the hide set go to M13.
- **M14:** future-fit/pdfa-fonts-not-profile-input
- **M15:** future-fit/m15-steps-and-caps
- **The agent milestone:**
  - history-soundness/agent-revert-is-jump
  - consumers/agent-entrypoint-miswired

**Parts deferred inside other rulings, by destination:**
- **M10 slice 9:** lowering refuses locked and signed fields with
  `EditError::Locked` (mdp-checked-at-save-only).
- **M10 slice 10:** `KeyPatch` (keypatch-holds-object-numbers).
- **M10 slice 11:** `GroupKind::Typing` (groups-without-producers).
- **M10 slice 15:** `FormBring::Flatten` (unproduced-variants,
  incoming-signed-fields-and-templates).
- **M10 slices 19–22:** `PageSource::Template` and `Actor::FormScript`
  (incoming-signed-fields-and-templates, unproduced-variants).
- **M10 slices 23–26:** FieldNode and widget patches
  (keypatch-holds-object-numbers).
- **M10 slice 29 (plugin API 0.5):** `FormDraw::Redacted`
  (r98-values-leak).
- **M11:**
  - `Rewrite::Redact`/`Edit` and PageSel on ApplyRedactions
    (transforms-not-streaming);
  - the leak check over the output (leak-check-over-docstate).
- **M12:**
  - SIGN and LTV classes (sigclass-total-order);
  - object-level page diffs (sigclass-pageannots-objectmap);
  - signing hooks (m12-hooks-yagni).
- **M13:**
  - `AnnotBody::Link` (keypatch-holds-object-numbers);
  - `Source::users` and the users index (shared-object-machinery).
- **M14:**
  - object streams (objstm-packing-double-encrypts, objstm-packing);
  - font embedding, the no-transparency rule and claim removal
    (conformance-claims);
  - structure modeling (struct-parents-on-inserted-pages);
  - Save Optimized (save-optimized-ends-history).
- **M15:** `BatesStart::Continue` (unproduced-variants).
- **The agent milestone:**
  - transport and socket (agent-socket-overbuilt, agent-socket-yagni);
  - `Actor::Agent` (unproduced-variants);
  - the agent's decision reads (wit-reads-not-in-read-set);
  - value gating for the agent (r98-values-leak).
- **When a producer appears:** `FormBring::Join`
  (insert-runs-vs-form-instance).

### Rejected

- **future-fit/incoming-signed-fields-and-templates.** The finding asked
  to turn incoming signed fields into unsigned ones. That contradicts
  M10 slice 5's committed, tested behaviour
  (`signed_fields_brought_in_are_kept_and_said_so`). Instead, such
  fields are kept with the SignatureNotValidHere notice and can be
  flattened with `FormBring::Flatten` (M10 slice 15). The finding's
  template half was deferred: `PageSource::Template` goes to M10 slices
  19–22.

### Where the plan's final text differs from the rulings (the plan wins)

- **Ruling tally.** The revision summary's 93 / 30 / 12 / 1 doesn't
  match the rulings array (101 / 25 / 9 / 1). The counts above use the
  array.
- **Notices.** slice45-notices-unmapped named three notices. The plan
  adds a fourth, `Notice::CopiesRenamed`, from critic fix 18.
- **`temp_dir` allows.** clippy-temp-dir-ban allowed two uses, recent.rs
  and the attachments panel. The plan's lint ledger adds a third, the
  plugin installer's `install_archive` (plugins.rs:529).
- **OCR marker.** private-strip-impossible-in-content moved /SheafGen
  and /SheafOCR into /ShP\<n\> property lists. In the plan, the OCR
  range is tagged `/ShOCR /ShP<n>` with a `/SheafOcrBlock` key (critic
  fix 26).
- **Op envelope.** op-json-versioning-gaps said `{v, ops}`. The plan's
  envelope is `{ "v": "1.0", "label"?, "ops" }`. A plugin envelope that
  carries `label` is Invalid (critic fix 10).
- **Locked fields.** mdp-checked-at-save-only sent lowering-time refusal
  to M10 slice 9. The plan makes that concrete as
  `EditError::Locked { field }`, rather than a separate refusal enum
  (critic fix 8).
- **The three open questions** from open-questions-triage are the plan's
  proposals 8–10 and its first open Ask. The plan assumes the
  recommendations. The settled decisions became proposals 11 and 13–40.
