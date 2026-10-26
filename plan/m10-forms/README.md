# M10: Forms

Status: paused · after slice 5; slices 6–17 resume after the [between-milestones work](../between/README.md): the linearization crate, F15, `sheaf search`, and roles and labels on sheaf-ui's widgets

Proves: Tax forms are filled and calculated in sheaf · Depends on: Nothing unbuilt (it waited, by a ruling, for all of M10.5: done)

## Summary

Started 2026-10-15, before M11 after all. The plan, with the decisions
made while planning and at kickoff and the bugs found, is in the files
linked below.

Paused after slice 5 (2026-10-15) for M10.5, the edit API; slices 6+
resume on it. Done so far: the corpus and form check, the field model,
and forms surviving page tools (crop, watermark, header/footer, delete,
extract, insert, replace, combine, duplicate), with the save rules for
hybrid XFA, usage rights and signed files.

- Fill AcroForms: text, checkbox, radio, combo/list, with correct appearance
  stream generation so values look the same in every viewer.
- Field calculations and format/validate/keystroke scripts. Run a
  sandboxed subset of Acrobat form JavaScript (AFNumber_Format,
  AFSimple_Calculate and friends, plus custom scripts), not a general JS
  runtime: pdf.js's form-script library in boa_engine, run as a
  WebAssembly guest in sheaf's wasmtime.
- Create / edit fields: all field types, properties, tab order, alignment
  tools.
- Automatic field detection on flat forms.
- Import / export form data: FDF, XFDF, CSV, tab-delimited text, JSON.
  Acrobat's XML later.
- Flatten form fields (Export Flattened Copy already keeps fields; add the
  option to flatten them).
- XFA: still not planned. Show a clear notice when a document is XFA-only.
- Forms survive page tools, and saves follow rules for hybrid XFA, usage
  rights and signed files.

Slices: corpus and form check → field model → page tools → appearances →
saving → filling → flatten → data import/export → AF functions → script
host → scripts → field authoring → auto-detect (the cut line) → plugin API.

## Where it stands

Status: **paused after slice 5** (2026-10-15), for M10.5, the edit API.
M10 resumes after all of M10.5 (slices 1–20; its proposal 11), with
slices 6+ rewritten on that API by M10.5 slice 20. In progress since
2026-10-15. Branch `m10`, cut from `main` at `13cfeb4` (which has all of
M9) and brought up to `3131eab`. M10 now comes before M11 (decision 2).
Three decisions were made while planning, and the proposals and asks
were settled at kickoff (both below). One commit per slice, with notes
per slice at the end, as in M7–M9.

## Slices

Ordered by proposal 4: groundwork and the page-tool fixes first, so
later slices build on forms that survive; scripts after the IRS forms
work; detection last, behind the cut line.

**Slices 6+ wait for M10.5** (Greg, 2026-10-15): appearances, saving,
filling, flatten and the rest are built on the redesigned edit API once
it lands, not before. Slices 1–5 are merged to `main` first. M10.5
slice 20 rewrote slices 6–29 on that API
([`m10.5-edit-api.md`](../complete/m10.5-edit-api/README.md), "M10 slices 6+ on the new
API"): each keeps its number, proposals and checks, and says first what
it is on the API (**On the edit API**), with the plan's § where a name
is defined. Where a proposal names code M10.5 replaced
(`Edit::FieldsSet`, `build_rewrite`, the incremental writer,
`rebase_to_saved`, `kept.rs`), the slice says what replaces it.

and, with `spawn`, `PageSource::Template` (spawning pages from
templates; slice 22).
`FormsHost::resolve(&Snapshot, Vec<Op>) -> (Vec<Op>, reads)` runs on the
formjs worker, then commits `Validate` into the user's Typing group. The
sandbox syncs on `ChangeSet.fields` for transitions not made by
`FormScript`. Replay never runs scripts, and a save waits for a running
cascade's group.

`AddWidget`, `PatchFields`, `PatchWidgets`, `MoveWidgets`,
`DeleteFields`, `DeleteWidgets`, `RenameField`, `MoveField`,
`SetTabOrder` and `SetCalculationOrder`, over existing keys. **They add
`FieldNode.patch`, widget patches (`PatchValue`) and role rows**
(removing `/AA` or `/JS` → REMOVES; `/AP` on a widget → FILL).
Multi-select is one Tx with locals. Deleting is REMOVES, so a Rewrite.
Copies are untagged until M14.

| Slice | Title | Status | Depends on |
|---|---|---|---|
| [1](../complete/m10-forms/s01-forms-corpus-form-check.md) | The forms corpus and the form check | done | — |
| [2](../complete/m10-forms/s02-field-model-form-facts.md) | The field model and form facts | done | — |
| [3](../complete/m10-forms/s03-forms-survive-page-tools-i-leaks.md) | Forms survive page tools I: leaks and copies | done | — |
| [4](../complete/m10-forms/s04-forms-survive-page-tools-ii-merge.md) | Forms survive page tools II: merge and rename | done | — |
| [5](../complete/m10-forms/s05-forms-survive-page-tools-iii-xfa.md) | Forms survive page tools III: XFA, usage rights and signatures | done | — |
| [6](slices/s06-text-field-appearances.md) | Text field appearances | open | between milestones (ruling) |
| [7](slices/s07-unicode-fallback-font.md) | A Unicode fallback font | open | 6 |
| [8](slices/s08-check-box-radio-choice-appearances.md) | Check box, radio and choice appearances | open | 6 |
| [9](slices/s09-saving-values.md) | Saving values | open | 6, 7, 8 |
| [10](slices/s10-filling-i-focus-keys-tab-order.md) | Filling I: focus, keys and Tab order | open | 9 |
| [11](slices/s11-filling-ii-field-editor.md) | Filling II: the field editor | open | 6, 7, 9, 10 |
| [12](slices/s12-filling-iii-marks-drawn-by-pdfium.md) | Filling III: marks drawn by PDFium | open | 6, 8, 9 |
| [13](slices/s13-check-boxes-radios-lists-buttons-clear.md) | Check boxes, radios, lists and buttons; Clear Form | open | 8, 9, 10, 11 |
| [14](slices/s14-highlight-fields-fields-panel-values-search.md) | Highlight Fields, the Fields panel, and values in search | open | 9, 10, 11 |
| [15](slices/s15-flatten.md) | Flatten | open | 6, 8, 9 |
| [16](slices/s16-form-data-fdf-xfdf.md) | Form data: FDF and XFDF | open | 9, 13 |
| [17](slices/s17-form-data-csv-text-json.md) | Form data: CSV, text and JSON | open | 9, 10, 16 |
| [18](slices/s18-af-functions-rust.md) | The AF functions in Rust | open | 6, 9, 11 |
| [19](slices/s19-script-host-i-guest.md) | The script host I: the guest | open | — |
| [20](slices/s20-script-host-ii-host-worker-protocol.md) | The script host II: host, worker and protocol | open | 9, 10, 16, 18, 19 |
| [21](slices/s21-scripts-i-keystroke-validate-format.md) | Scripts I: keystroke, validate, format | open | 10, 11, 13, 20 |
| [22](slices/s22-scripts-ii-calculate.md) | Scripts II: calculate | open | 11, 13, 16, 17, 20, 21 |
| [23](slices/s23-prepare-form-i-mode-tools.md) | Prepare Form I: the mode and the tools | open | 6, 8, 9 |
| [24](slices/s24-prepare-form-ii-field-properties.md) | Prepare Form II: field properties | open | 13, 14, 23 |
| [25](slices/s25-prepare-form-iii-actions-scripts.md) | Prepare Form III: actions and scripts | open | 18, 22, 24 |
| [26](slices/s26-prepare-form-iv-order-layout.md) | Prepare Form IV: order and layout | open | 10, 23 |
| [27](slices/s27-detect-fields-i-born-digital-pages.md) | Detect fields I: born-digital pages | open | 15, 23 (cut line) |
| [28](slices/s28-detect-fields-ii-scans.md) | Detect fields II: scans | open | 27 (cut line) |
| [29](slices/s29-plugin-api-0-5-docs.md) | Plugin API 0.5, and docs | open | 6–26; 27–28 unless cut (wrap-up) |

**Slices 19–22, scripts, on the edit API.** They add `Actor::FormScript`
**Slices 23–26, Prepare Form, on the edit API.** `AddField`,

## Plan files

- [Decisions](decisions.md): ruled while planning and at kickoff
- [Proposals](proposals/README.md): one file each
- [Scope](scope.md)
- [Found while planning](found.md)
- [Not in M10](not-m10.md)
- [Sources](sources.md)
- Finished slices, with their notes: [complete/m10-forms](../complete/m10-forms/)
- [Build or adopt](../research/build-or-adopt/README.md): the M10 rows, and the [changes to make](../research/build-or-adopt/changes-to-make.md) at kickoff
- Open questions: [asks](../asks.md)
