# M10: Found while planning

Part of [M10: Forms](README.md).


Bugs and gaps in today's code that matter for M10. References are as of
`13cfeb4`. "Reproduced" means throwaway probes, outside the repo, ran
sheaf's own code on real forms (CA 540, NY IT-2104, and the IRS 1040,
W-4 and W-9) and checked the output. Slices 3–5 and 9 fix 1–7, and the
slice in brackets fixes each.

1. **Crop, watermark and Remove Header/Footer destroy a page's form.**
   Reproduced.
   - `derived_copy` makes a new catalog with no `/AcroForm`
     (save.rs:690). Crop (`cropped_copy`, save.rs:619), watermark and
     header/footer removal (sheaf-ops derive.rs:67, :142) all go through
     it.
   - **On screen,** every widget on the derived page disappears: PDFium
     draws widgets only in a document with an `/AcroForm`. A widget with
     a red appearance gave 2,392 red samples before and none after, and
     a filled value went too.
   - **On save,** the primary's fields lose their widgets and are pruned,
     and the copies are orphans: a cropped NY IT-2104 saved with `/Fields`
     empty and 30 orphan widgets.

   [Slice 3.]
2. **Insert, Combine and Replace never merge forms.** Reproduced.
   - Copied widgets keep their `/Parent` chain, but their fields never
     reach `/AcroForm /Fields` (save.rs:434-441, incremental.rs:115-125).
     The incoming `/DR`, `/DA`, `/CO`, `NeedAppearances`, `/XFA` and
     document scripts are dropped.
   - `sheaf-cli combine it2104.pdf 2024-540.pdf -o x.pdf` gives 180
     orphan widgets. NY with the 1040 gives 199, and the 1040's
     `/HelveticaLTStd-Bold` is missing from `/DR`. NY IT-2104 inserted
     into the W-9 loses its `/Arial`. An incremental insert does the
     same.
   - Copying one page of a form drags along the same fields' widgets
     from pages not copied, with `/P` null (8 strays in one case).

   [Slice 4.]
3. **Two copies of one form have no rule for duplicate names.**
   Reproduced.
   - Combining a form with itself leaves the second copy's 30 widgets
     orphaned, with the same names.
   - In sheaf, PDFium binds them to the first copy's fields by name (60
     widgets, 25 names), so filling one would fill both. Readers that
     follow the spec don't see them as fields at all.

   [Slice 4, by proposal 5.]
4. **Duplicate Pages shares widget objects between pages.** The copy
   reuses the same `/Annots` references (save.rs:539-547,
   incremental.rs:171-178). Duplicating NY IT-2104's page gave 30 widgets
   on more than one page, and 30 with the wrong `/P`. Reproduced, by
   both strategies. [Slice 4, by proposal 5.]
5. **A filled value from a removed page stays in the file.** Reproduced.
   - `prune_structure` walks only `/K`, and its `/ParentTree` check
     (kept.rs:562-583) keeps entries for structure elements outside the
     `/K` tree. Acrobat-tagged forms have duplicate `/S /Form` elements
     with no `/P`, and those keep a removed page's widget alive, with its
     `/V` and `/AA`.
   - Fill CA 540's field `540-5007` with `LEAKVALUE123`, then delete page
     5 or extract page 6: the output still holds `LEAKVALUE123`, in a
     stray widget with `/P` null.
   - A gap in M9's slice 21 (deleting means deleted).

   [Slice 3.]
6. **The XFA copy is kept when pages or fields change.** Reproduced.
   - Deleting a page of the 1040 leaves XFA for 2 pages in a 1-page file.
     Combining two 1040s gives 4 pages and XFA for 2. Extracting the
     W-9's page 2 (instructions) gives empty `/Fields` with the full XFA,
     document scripts and `/Perms`.
   - XFA datasets can hold values Acrobat filled, so this can leak too.
   - Once sheaf fills the AcroForm side, Acrobat would show the stale
     XFA data instead (known behavior; not reproduced, since it needs
     Acrobat).

   [Slices 5 and 9, by proposal 6.]
7. **Usage rights are kept through a rewrite.** Reproduced. After a
   rewrite, the 1040's UR3 `/ByteRange [0 206928 219198 1039]` still
   covers the 220,237-byte original, inside a 392,163-byte file. Nothing
   in the code knows `/Perms`, UR3 or `/SigFlags`. Incremental saves keep
   the original bytes intact. [Slices 5 and 9, by proposal 7.]
8. **Field values can't be found.** Find, Patterns, text export and text
   selection use PDFium's page text (sheaf-render lib.rs:471-529,
   :577-580; sheaf-ops export.rs:145-160), which leaves annotations out.
   A filled value gave no search hits and wasn't in `sheaf-cli export -f
   text`. Reproduced. [Slice 14.]
9. **Clicking a field doesn't fill it.** `on_page_mouse_down` checks
   marks, then other tools' comments (which never include widgets), then
   starts a text selection (markup_tools.rs:610, :767). With a markup
   tool, the click puts a mark on the field. The `Renderer` trait has no
   widget hit test, field cursor or Tab order (lib.rs:100-152), and
   nothing reads `/Tabs`. [Slices 10 and 13.]
10. **Smaller ones:**
    - sheaf's screen draws a value that has no appearance in the file
      (PDFium's `FFLDraw` reads `/V`), so it hides missing appearances.
      Reproduced. [Slice 1: tests render appearances only.]
    - `normal_appearance` falls back to the first `/AP /N` state when
      `/AS` is missing (save.rs:360). Today flatten keeps widgets
      (save.rs:215-218), so only other annotations reach it, but flatten
      must not reuse it for widgets. From the code. [Slice 15.]
    - `clean_fields` always keeps direct-object fields and kids
      (kept.rs:442-445, :508-511), and leaves an empty `/AcroForm`
      behind. From the code. [Slice 3.]
    - `print_pdf` keeps live fields ("Form fields print either way",
      save.rs:127-138), and that file goes to `lp` (cups.rs:85-87) and
      to the print dialog's Save as PDF. Print on Windows (sheaf-print
      gdi.rs:329) and image export (sheaf-ops export.rs:129-143) render
      without PDFium's printing flag, so a widget's Print and NoView
      flags act as they do on screen. From the code. [Slice 15.]
11. **Gaps:**
    - No notice for XFA-only forms, though pdfium-render's `form_type()`
      tells them apart (it gave Acrobat or XfaForeground on the corpus).
      [Slice 2.]
    - FDF `/Fields` and XFDF `<fields>` are ignored without a word
      (fdf.rs:41-47, xfdf.rs:6-7). [Slice 16.]
    - No script runs, so values show unformatted. The vendored PDFium
      (bblanchon build 7881, scripts/fetch-pdfium.sh:8) has no JavaScript
      or XFA, and pdfium-render passes no JavaScript platform. [Slices
      18–22.]
    - Generated text is Helvetica and WinAnsi only: `content_literal`
      writes anything above U+00FF as `?` (annot.rs:1515). [Slices 6–7.]
    - Once values can change, an incremental save would keep a replaced
      value in the earlier revision: `Snapshot::removes_content`
      (model.rs:918) covers pages, marks, comments and bookmarks only.
      [Slice 9, by proposal 7.]
    - A removal on a file with a signed field forces a full rewrite (M9
      slice 21) that breaks the signature without asking:
      `removes_content` has no signature check. From the code. [Slice 5,
      by proposal 7.]
    - The editor for a mark's text is a fixed 300 px box under it
      (docview.rs:4146-4180), and `TextInput` has no hook to veto or
      filter a change, and no maximum length (text_input.rs:352). Fields
      need an editor over the widget at the page's zoom, and keystroke
      scripts need the veto. Ctrl+Z in a text field reaches document
      Undo: it's bound for `DocView || Outline || Comments`
      (main.rs:496), and `TextInput` binds no undo of its own (sheaf-ui
      lib.rs:39-42). [Slices 10–11.]

Fine today, recorded so nobody looks again: an incremental save keeps a
form byte for byte, reordering pages keeps it, and deleting or
extracting pages prunes fields and `/CO` correctly (apart from bugs 5–6).
