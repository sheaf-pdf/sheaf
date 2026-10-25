# M10 slice 4: Forms survive page tools II: merge and rename

Status: done · Part of [M10: Forms](../../m10-forms/README.md)

**Forms survive page tools II: merge and rename** (proposal 5; bugs
2–4).
- Insert, Replace and Combine merge the incoming form, by both save
  strategies: fields, `/DR`, `/DA` and `/Q`, `/CO`, document scripts.
  Only widgets on copied pages come along.
- A clash on any node renames the whole incoming form, and the rename
  is kept in the edit.
- Duplicate Pages gives the copy its own widgets and names.
- Scripts from more than one source are off by default.
- Notices for a renamed form, for one whose scripts name its old
  fields, and for scripts turned off.
- **Checks:** bugs 2–4's reproductions, by both strategies, with the
  check clean; Combine of the 1040, Schedule 1 and Schedule B gives
  unique full names; GA 500 combined with OH IT 1040 has scripts off.

## Notes

Done. Forms that come in with pages join the document's, and a
duplicated page's widgets are its own.
- **The model holds the incoming fields.** `Document::set_source_form`
  gives the model a source's form as read (the app's Insert and
  Replace, and Combine, call it beside `add_source`; `set_initial_fields`
  records the primary's). `InsertPages` and `ReplacePages` then bring
  that file's fields with widgets on the pages coming in, and its fields
  with no widget at all, with new ids and their widgets on the new
  pages. `DuplicatePages` gives the copies' widgets fields of their own.
- **Names that clash are renamed** (`Document::name_apart`): when any
  incoming full name, or a parent in one, is a field or parent the
  document has, the whole incoming form goes under a new parent named
  for its file (`fw9`, then `fw9_2`; a dot in the name becomes `_`).
  Duplicate Pages uses the same rule, so a copy of `f1040`'s page is
  `f1040.topmostSubform[0]…`. Pages of a file whose form is already in
  the document join that form (Combine's `b.pdf:1,3` inserts two runs).
- **`Field::copy` and `Field::under`** say which copy of its source's
  form a field is in (0 for the source's own, another for each
  duplicate) and the parent it goes under. `Field::source_name` is its
  name in its own file.
- **Both writers** key their copy maps by (source, copy). A page whose
  widgets are in a later copy gets an object of its own, reserved
  before the pages are written, and all its annotations copied for it
  (`forms::merge::copy_annots`); a copy of the primary's form is copied
  from a clone of the primary as loaded. Then `forms::merge::merge` puts
  each copy's roots in `/Fields` (under a new parent when renamed),
  prunes widgets that aren't on the pages written
  (`kept::prune_fields`), merges `/DR` (adding what's missing; a font of
  the same name that isn't the same font comes in as `Helv_2`, and the
  copy's `/DA`s follow), pushes the incoming form's `/DA` and `/Q` down
  where they differ, appends its `/CO`, ORs `NeedAppearances`, and adds
  its document scripts (a clashing name becomes "Init 2"; Adobe's
  `!ADBE::` version checks aren't doubled). An incoming `/XFA`, `/Perms`
  and `/SigFlags` stay behind (slice 5).
- **The incremental writer writes only new objects something it writes
  reaches,** so what a copy dragged in (a widget of a page left behind,
  with its value) stays out, as a rewrite's pruning keeps it out.
- **Scripts:** `Document::script_sources` (sources whose fields, or
  whose document scripts, have scripts), and `scripts_mixed`, true when
  there's more than one, now or before a save this session. Nothing runs
  scripts yet; slice 21 reads it.
- **Notices** (`Document::form_notice`, from `EditOutcome::forms_renamed`):
  "Fields from fw9.pdf are under “fw9”, since their names were taken",
  "; its scripts still use the old names", "The copies' fields are under
  “f1040”, apart from the originals", and "Scripts are off: fields from
  more than one form have them". The app shows them after Insert,
  Replace and Duplicate Pages; `sheaf combine` prints them
  (`CombineReport::notice`).
- Combine (`sheaf_ops::combine`) loads the first file's fields and gives
  the model the others' forms.

Deviations from the plan:
- **The rename is kept in the model, not in the edit.** It's decided
  when the edit applies, from the document's names then, and written
  into the fields (`Field::under`); each step records the field id
  counter as it does page ids, so a replay gives the same fields, ids
  and names. The edits keep their shape, which M10.5 redesigns anyway.
- **A source's form is given beside the source** (`set_source_form`),
  not on `SourceInfo`, which has 25 places that build one.
- **Duplicating a page with widgets copies all its annotations,** links
  and comments too, so the copy owns each one. A page with no widgets
  duplicates as before (annotations shared by reference). Copies of
  the primary's form duplicate their appearance streams and resources
  rather than share them.
- **After a save, every field is the saved file's own** (copy 0, no
  parent to add): `rebase_to_saved` resets them, so the next save
  doesn't copy them again. Their objects in the saved file are stale
  after a rewrite; re-binding them is slice 9's (proposal 8).
- **Combine's notice isn't shown in the app yet.** Combine writes a file
  and opens it in a new window, and the notice would have to follow the
  open; `sheaf combine` prints it. And the opened file is one form to
  sheaf, so "scripts off" doesn't follow it: slice 21 should carry the
  hint to the window it opens in, or mark the file.
- The flatten offer for a renamed form with scripts waits for slice 15;
  the notice says its scripts still use the old names.
- Whether Acrobat links a duplicated page's fields (Asks) is still
  unchecked; sheaf renames, as decided.

Found here:
- **The incremental writer wrote every object a copy made,** referenced
  or not, so an insert of one page wrote the widgets (and values) of the
  incoming file's other pages that its fields' `/Kids` reached (bug 2's
  strays, in the incremental strategy). Fixed by the reachability rule.
- The incremental writer, which recovery sidecars use, can't take out a
  replaced page's widgets: their fields stay in `/Fields`, with widgets
  on no page. Saves rewrite then (M9 bug 14); a recovered file would
  have them. Not fixed.

Checked by tests (`sheaf-ops/tests/forms_merge.rs`; helpers shared with
slice 3's in `tests/common`), each by the rewrite and the incremental
writer unless said:
- an inserted form joins: 4 fields and 5 widgets, values, `/DR`'s
  added font, `/CO`, its value drawn with appearances only, and the save
  in place incremental;
- one page of a form brings only that page's widget of a field with
  widgets on two pages, and nothing of the other page;
- a form inserted into itself goes under "form", then "form_2", values
  in each, with the notice;
- a clash only on a parent (`top.x` and `top.y`) renames the whole
  form; a font of the same name that's another font comes in renamed,
  with its fields' `/DA`;
- a duplicated page gets its own field (`form.a`), drawn; a duplicate of
  an inserted page and of a cropped page too;
- a duplicate and an insert saved in place, then saved again both ways:
  each field once;
- Replace brings the new page's form; the rewrite drops the replaced
  page's field (the incremental writer's one finding is that widget, as
  above);
- undo takes the form out again;
- scripts from two forms are off, with the notice, and their document
  scripts kept apart ("Init", "Init 2", one version check); Combine says
  the same;
- a file's pages in two runs join one form, without its other page's;
- `form_notice`'s texts; `merge`'s `/DA` font renaming;
- the model's replay test with forms in both files, which fails if a
  replay doesn't restore the field counter;
- each part taken out in turn (the merge, own objects for copies,
  renaming, pruning incoming kids, `/DR`, the incremental writer's
  reachability rule) fails some of these;
- `sheaf-cli/tests/combine.rs`: a form combined with itself prints the
  notice, and the check is clean on the file.
- On the corpus (skip without it): the 1040, Schedule 1 and Schedule B
  combine to 344 fields and widgets (199 + 73 + 72) with unique names,
  the schedules under "f1040s1" and "f1040sb", and no finding but slice
  5's (the XFA copy, usage rights); the 1040 combined with itself, 398
  fields (bug 3); CA 540 and NY IT-201 combine to 383 fields and 417
  widgets, no problems, scripts off; the 1040's first page duplicated,
  by both writers, 128 more widgets and no finding but slice 5's.

Checked on Xwayland: the filled 1040's first page duplicated (Duplicate
Pages): 3 pages, the model's fields 199 + 128, and "Duplicate page. The
copies' fields are under “f1040-filled”, apart from the originals" in the
status bar.
