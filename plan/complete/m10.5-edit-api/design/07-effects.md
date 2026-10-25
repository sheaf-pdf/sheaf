# M10.5 §7: Effects

Part of [M10.5: the design](README.md).


#### 7.1 `classify`: the one place that knows what a change means

```rust
bitflags! { pub struct Flags: u32 {
    const PAGE_ADD = 1<<0; const PAGE_REMOVE = 1<<1; const PAGE_ORDER = 1<<2; const GEOMETRY = 1<<3;
    const CONTENT = 1<<4;          // a layer or generator output added or changed (visible inputs)
    const REMOVES = 1<<5;          // content-bearing data that existed is gone
    const REPLACES_TEXT = 1<<6;    // a text-bearing value replaced or cleared (contents, /RC, struck text, tape rows,
                                   // /T, title, label prefix, field value non-empty → other, OCR words, recipe text)
    const LIVE_REDRAW = 1<<7;      // a generator's live tokens re-laid out under an unchanged recipe (appendable)
    const ANNOTS = 1<<8; const FIELD_VALUES = 1<<9; const FIELD_TREE = 1<<10; const FORM = 1<<11;
    const OUTLINE = 1<<12; const LABELS = 1<<13;
}}
bitflags! { pub struct SigClass: u8 { const FILL = 1; const ANNOT = 2; const OTHER = 4; } }   // M12 adds SIGN, LTV
pub struct Effects { pub flags: Flags, pub sig: SigClass, pub pages: PageTouch, pub keys: Arc<KeySet> }
pub fn classify(key: &Key, before: Option<&Val>, after: Option<&Val>, cx: &ClassifyCx) -> Effects;  // pure, total
```

**Effects** are what a change means for saving, signatures, rendering
and the app. `SigClass` is a **set**, not a ladder. DocMDP and UR3
permissions are allow masks over it (§9.3), so M12's SIGN and LTV
classes fit without reordering anything.

| Change | Flags | Sig |
|---|---|---|
| `Annot` None → Mark or Foreign | ANNOTS | ANNOT |
| `Annot` Some → None (mark, foreign, link) | ANNOTS \| REMOVES | ANNOT |
| Mark contents, struck text, tape rows or kind changed | ANNOTS \| REPLACES_TEXT | ANNOT |
| Mark geometry or style only | ANNOTS | ANNOT |
| `PageAnnots` membership or order delta of marks and others' annotations | ANNOTS | ANNOT |
| `PageAnnots` membership delta of widgets | FIELD_TREE | OTHER |
| `Geom` changed | GEOMETRY | OTHER |
| `Order` changed | PAGE_ORDER (+ PAGE_ADD / PAGE_REMOVE by diff) | OTHER |
| `Page` content: over-layer added | CONTENT | OTHER |
| `Page` content: filter added, or a layer that existed removed or changed | CONTENT \| REMOVES (OCR words: \| REPLACES_TEXT) | OTHER |
| `Field` value None or empty → v | FIELD_VALUES | FILL |
| `Field` value v → v′ | FIELD_VALUES \| REPLACES_TEXT | FILL |
| `Field` node added, removed, renamed or reparented | FIELD_TREE (removed: \| REMOVES) | OTHER |
| Widget `/AS`, or appearance inputs derived from the value, only | FIELD_VALUES | FILL |
| Widget rect, `/MK`, `/DA` or flags | FIELD_TREE | OTHER |
| `Form` changed | FORM | OTHER |
| `Generator` added | CONTENT over scope | OTHER |
| `Generator` added with `replaces`, where covered pages of the saved file hold Acrobat-added artifacts of those kinds | CONTENT \| REMOVES | OTHER |
| `Generator` recipe text changed or removed where it was drawn in the saved file | REMOVES \| REPLACES_TEXT | OTHER |
| `Generator` live tokens re-laid out, same recipe | LIVE_REDRAW | OTHER |
| `Bookmark` title changed | OUTLINE \| REPLACES_TEXT | OTHER |
| `Bookmark` removed | OUTLINE \| REMOVES | OTHER |
| `Labels` prefix that disappears | LABELS \| REPLACES_TEXT | OTHER |

`ClassifyCx` gives `classify` the layout context (positions before and
after, `needs()`) and a pure accessor to source facts (`blocks`, for the
`replaces` row). It is still a pure function of its inputs.

#### 7.2 Aggregation, judged by content against the saved version

- **Per step,** the union of prim effects is stored (`Step.effects`).
  Previews, approvals and ChangeSets use it.
- **For a save,** effects are **re-judged** against the saved version.
  - `path_keys(saved, target)` is the union of `Version.keys` along the
    tree path saved → LCA → target. When the saved version has been
    pruned (§8.7), the saved anchor's `keys_to_root` joins the union.
  - For each key in it, `classify(key, saved_value, target_value)` runs
    once, with `ptr_eq` first.
- **The result is content-based.** Replaced mark text, a renamed
  bookmark, a changed header text, a replaced field value, a replaced
  OCR layer and a deleted mark from an inserted file all count. Undoing
  back to the saved version empties the path.
- **This replaces** `removes_content`, `annots_dirty_pages`,
  `fields_dirty`, `header_footer_dirty`, `outline_dirty`,
  `labels_dirty`, `form_changed`, and the baseline state slice 8 put in
  place of the five `saved_*` copies and `baked_header_footer`.

**An Append's emission scope is closed over its dependencies.**
`emit_deps(key, saved, target) -> ObjKeySet` is a registry-rule
function, exhaustive over `Key`. The Append scope is the closure of
`path_keys` under it.

| Key | Also emits |
|---|---|
| `Order` | changed `/Pages` nodes; `/PageLabels` when labels exist; pages whose links target a page whose written-ness changed (a links-by-target index); pages covered by a live-token generator whose layout digest ≠ `SaveRecord.drawn` |
| `Geom(p)` | `p`'s page dict; generator blocks on `p` whose layout changed |
| `Page(p)` | `p`'s page dict, `/Contents`, blocks, `PageRes` |
| `PageAnnots(p)` | `p`'s page dict (`/Annots`) |
| `Annot(a)` | its annotation objects (`AnnotPart`s); for a merged widget, its field object |
| `Field(f)` | its dict (or merged widget); its widgets' appearances when the value changed; the parent's `/Kids`; AcroForm `/Fields` for a root |
| `Form` | the AcroForm owned keys; appearances of widgets that inherit `/DA` or `/DR` |
| `Bookmark(b)` | its item; its parent's `/First`, `/Last`, `/Count`; its neighbors' `/Prev`, `/Next` |
| `OutlineRoots` | the Outlines root and the first and last items |
| `Labels` | `/PageLabels`; pages with `<<label>>` tokens whose layout changed |
| `Generator(g)` | `/SheafGenerators`; blocks on covered pages whose layout changed; those pages' dicts |

A property test in the default run checks the closure: after every save
in a random sequence of ops, Appends, Rewrites, Save As, undo, redo and
jump, the reopen facts of the written file equal the reopen facts of a
fresh Rewrite of the same state (Tests, fixtures and gates).

#### 7.3 Consumers

| Consumer | Uses |
|---|---|
| Save planner and its notice ("Rewriting: the text of 2 comments changed since the last save") | `SavePlan` |
| Signed-file prompt | `SigImpact`, `AppendAnyway` |
| Plugin approval and dialog consequences | `Preview.effects`, `Preview.consequences` in words |
| Menus (`can_apply`) | `Tx::check` (no lowering) |
| History panel | a badge on steps with `REMOVES` |
| M11 leak check | over the **output** (§20), not DocState |
