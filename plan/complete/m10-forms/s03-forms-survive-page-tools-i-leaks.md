# M10 slice 3: Forms survive page tools I: leaks and copies

Status: done · Part of [M10: Forms](../../m10-forms/README.md)

**Forms survive page tools I: leaks and copies** (proposal 5; bugs 1
and 5, and bug 10's `clean_fields`).
- `derived_copy` carries the page's widgets, fields and `/DR`.
- Parent-tree entries that keep a removed page's widget go.
- An empty `/AcroForm` goes; direct-object fields are pruned too.
- **Checks:** bugs 1 and 5's reproductions as regression tests, by
  both strategies, with the form check clean on the output;
  `LEAKVALUE123` is nowhere in the bytes; a cropped form page still
  shows its value with appearances only.

## Notes

Done. A page keeps its form through the page tools that copy it, and a
removed page's widgets leave the file, values and all.
- **Derived copies know their origin.** `SourceInfo::derived`
  (`Derived { from, pages }`): crop, watermark (and its removal) and
  Remove Header/Footer record which page each copied page was made from.
  `save::origin` follows copies of copies.
- **`derived_copy` carries the form** (`save::copy_form`): the copied
  roots of `/Fields` and `/CO`, and `/DR`, `/DA`, `/Q` and
  `NeedAppearances`, so PDFium draws the copy's widgets. The XFA copy,
  usage rights and `/SigFlags` describe the whole file, so they stay
  behind. `keep_only` then drops the widgets of pages not copied, and
  the copy prunes what only they reached.
- **A save writes a derived page as the page it was made from,** with
  the copy's content: `copy_derived_page` takes every key but `/Annots`
  and `/Parent` from the copy, and `/Annots` are the original's. Both
  writers (`build_rewrite`, `incremental::build`). So the original's
  widgets keep their `/P` and their fields, and the copy's own widgets
  never reach the file. The original's object is reused, as for any
  page.
- **`kept::keep_only` lets go of a removed page's annotations**:
  - `dropped_annots`: those in a removed page's `/Annots`, or whose `/P`
    is a removed page (or null), and on no kept page. `sweep` cuts every
    reference to them, as it does to removed pages;
  - structure elements the parent tree or the ID tree reach and `/K`
    doesn't are pruned from their highest ancestor the walk hasn't seen
    (`topmost`); an element pruned empty loses its `/K`, so whatever
    still lists it reaches nothing;
  - a tree whose root is a direct dictionary (`/ParentTree`, `/IDTree`,
    `/Names /Dests`) is moved into an object first (`indirect`): the
    walks that clean trees only changed referenced roots, so a direct
    parent tree kept every entry;
  - `clean_fields` prunes direct-object fields and kids, looks through a
    node listed twice once, and removes an `/AcroForm` left with no
    fields (an XFA copy beside it goes with it);
  - page templates (`/Names /Templates`) count as kept, widgets and all.

Deviations from the plan:
- **"By both strategies" means the rewrite and the incremental writer,
  not two saves in place.** M9's rule stands: a page shown from a copy
  counts as removed content (`Document::removes_content`), so saving a
  cropped page in place is a rewrite, as is a delete. That's right for
  Remove Header/Footer and Remove Watermark, which take content out. The
  incremental writer still writes derived pages, for the recovery
  sidecar, so tests check its bytes directly. A header/footer change is
  never incremental (the writer refuses it), so that test checks the
  rewrite.
- **The corpus's CA 540 doesn't have bug 5's shape.** It's the 2025
  form, which tags each widget once, in `/K`; planning reproduced the
  leak on the 2024 one. `540-5007` is `540_form_5007` now. Its test
  (fill it, delete page 5, extract page 6) passes before and after the
  slice. The regression test is a fixture built in code (below), and a
  corpus test over every curated form (below) found real leaks.

Found here:
- **The I-9 keeps a deleted page's values through a second structure
  tree.** Its catalog's `/StructTreeRoot` holds one tree, and its parent
  tree points into another, whose root nothing but `/P` links reaches.
  Pruning removed the parent tree's entry for each widget's element, but
  left the element's `/K`, and its `/Figure` parent in the other tree
  still listed it. That tree is reachable from the kept pages' elements
  by `/P`, so the deleted page's widgets stayed, with their `/V`. Fixed
  by the `/K` clearing and `topmost` above. At HEAD, deleting the page
  with the most text fields kept a value in 3 of the 64 curated forms
  with more than one page (4506-C, 8832, I-9); now in none.
- **A direct `/ParentTree` was never cleaned** (above), which `Form::tag`
  writes; any element it listed kept its widget. From M9's slice 21.
- **Every rewrite dropped page templates.** `sweep` nulled references to
  pages outside the tree, which templates are: `/Names /Templates` came
  out as `[(t1) null]`, and their fields lost their widgets. From M9.
- **A reply kept a deleted page's comment:** a note on a kept page whose
  `/IRT` names one on a removed page kept it, contents and all. The
  sweep's cut covers it.
- Left for slice 4, as planned: duplicating a derived page shares its
  widgets (bug 4); a derived page made from an inserted file's page
  brings widgets whose fields aren't in `/Fields` (bug 2).

Checked by tests:
- `sheaf-ops/tests/forms_pages.rs`, on fixtures, each failing at HEAD:
  - a cropped page (bug 1): the copy has a form and PDFium draws its
    field (as the app does), only that page's fields came along, and the
    check is clean on it; the rewrite and the incremental writer keep
    all 4 fields and widgets, values, the crop box, the page's own 3
    widgets, a clean check, and the value with appearances only;
  - the same after a watermark, after a watermark taken off again (a
    copy of a copy, with no "DRAFT" left), and after Remove Header/Footer
    on a saved footer (none of it left, by rewrite);
  - bug 5 by delete (in place, a rewrite) and by extract: Acrobat's
    duplicate `/Form` elements with no `/P` in the parent tree; and the
    I-9's shape, a second tree with a `/Figure` whose alt text describes
    the page and an element only the ID tree reaches. `LEAKVALUE123`
    and the deleted page's alt text are nowhere in the bytes (streams
    decoded too), the other page's are, the check is clean, and no
    `/OBJR` leads nowhere;
  - a form whose fields are on page 1 only, by extract and delete: no
    `/AcroForm` (its XFA copy too), "no form"; a direct-object parent
    field whose widgets were on a deleted page goes from `/Fields`;
  - a page template with a field survives a delete, by both saves.
- Each piece of `keep_only` was taken out in turn, and a test failed
  without it (the `/K` clearing, `topmost`, the ID tree's orphans, the
  sweep's cut of dropped annotations, the templates).
- `kept::tests::a_reply_keeps_no_comment_of_a_deleted_page`.
- On the corpus (skip without it): every curated form with more than
  one page, its busiest page's text fields filled with markers, that
  page deleted: no marker and no dangling `/OBJR` in the rewrite (64
  forms, 3.9 s); CA 540 as above; the 1040's and W-9's first pages
  cropped, by both writers, keep their field and widget counts, the
  filled value, and no finding the input didn't have (but slice 5's).

Checked on Xwayland: a filled 1040, page 1 cropped by an inch from the
top through the dialog: "Cropped 1 page", 612×720 pt, and the value
still drawn (before, the cropped page lost every widget).
