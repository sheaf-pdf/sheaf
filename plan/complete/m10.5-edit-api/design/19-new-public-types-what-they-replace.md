# M10.5 §19: New public types: what they replace, what produces them

Part of [M10.5: the design](README.md).


| Types | Replace | Produced by in M10.5 |
|---|---|---|
| `Store`, `Blob`, `BlobRef`, `Src`, `BlobId` | `SourceInfo`, `add_source`, the generated cache | open, insert, intern, upload |
| `Source`, `LazyPdf`, `XrefFacts`, facts | eager `load_source` per use, six late loaders | open, insert |
| `DocState`, `Key`, `Val`, `Prim` | DocState v1, `Edit`'s implicit effects | every op |
| `PageDef`, `PageBase`, `Geom` | today's `model::PageRef` struct, `Rebase`, `PageGeom`, `geom_of` | open, page ops |
| `ContentStack`, `Layer`, `Static`, `Filter`, `MarkedSel`, `StreamSel`, `LegacySel`, `PagOrigin`, `Placement`, `FormRef`, `PinnedAp` | derived copies, `text_layers`, `stamps::strip` | OCR, flatten, remove/replace, `RemovePrivateData` |
| `Generator`, `GenSpec`, `Scope`, `Frozen`, `ImageRef`, `PageForm`, `CtxNeeds` | `stamps::HeaderFooter`, derive::watermark | Set/Remove Watermark and HeaderFooter |
| `AnnotRec`, `Mark`, `Foreign`, `Widget`, `WidgetProps` | per-page `Annot` lists, `foreign_annots`, `removed_comments`, `forms::Widget`'s ObjectId/slot | open, mark ops |
| `FieldNode`, `Terminal`, `FormDoc`, `FormInst`, `ScriptState` | `Field { copy, under, object }`, `Document.forms`, `scripts_mixed` | open, insert, duplicate |
| `Bookmark`, `Target`, `LabelSpec` | `OutlineNode`, today's `LabelSpec` | open, bookmark and label ops |
| `Op`, `Transform`, `PageRef` (the op reference enum), `BookmarkRef`, `AnnotRef`, `BlobSel`, `PageSel`, `Patch`, `NewMark`, `MarkPatch`, `Imported`, `Bring`, `Delta` | `Edit`, the WIT edit variant, plugin-api `Batch` | app, CLI, plugins |
| `Tx`, `ExportTx`, `LowerCx`, `HostAttrs`, `Notice`, `EditError`, `Created`, `Preview` | `compute`, `EditOutcome`, `sign_marks` | every caller |
| cascade rules | `cascade_delete`, `bring_annots` (was `import_marks`), `merge_outline` | lowering |
| `Effects`, `Flags`, `SigClass`, `classify`, `emit_deps` | `removes_content`, dirty flags, `saved_*` | commits, saves |
| `Version`, `Step`, `Line`, `GroupId`, `GroupKind`, `ChangeSet`, `Commit`, `Base`, `Stale` | in-place versions, jobs, three funnels | every transition |
| `SaveRecord`, `SaveInput`, `SavePlan`, `Strategy`, `Onto`, `Why`, `Ask`, `AppendAnyway`, `SigImpact`, `FileStamp`, `ContentHash`, `PendingTail`, `SaveTransform` | `Signed`, `WouldBreakSignatures`, `rebase_to_saved`, `rules::take_out` | every save |
| `ObjKey`, `SymObject`, `Emission`, `EmitScope`, `ObjMap` | `build_rewrite`/incremental internals | materialize, realize |
| `Export`, `Profile`, `Out`, presets | `export_*`, `print_pdf`, `subset` | exports |
| `ViewInputs`, `RenderKey`, `EngineKey`, `PageView`, `Realizer`, `EnginePool` | `RenderHandle` per source, `hide_annots`, `CharsKey` | screen, print, OCR, plugins |
| `TextService`, `TextOpts`, `PageChars` | `layer_hits`, `merge_source_hits` | Find, plugins, blank detection |
| `Tok`, `Marked`, `Balance`, `PageBlocks`, `TextBag`, `ContentWriter`, `Font` | `Content::decode/encode` use, four encoders | content features, removal check |
| `Session` | `sheaf_ops::open`, per-command flows | CLI, combine, scan |
