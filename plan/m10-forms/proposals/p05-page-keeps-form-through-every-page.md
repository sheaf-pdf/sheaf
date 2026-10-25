# M10 proposal 5: A page keeps its form through every page tool

Part of [M10: Forms](../README.md) · [all proposals](README.md)

**A page keeps its form through every page tool.**
- Crop, watermark and Remove Header/Footer carry the page's fields
  into the derived copy (bug 1).
- Insert Pages, Replace Pages and Combine add the incoming fields to
  `/AcroForm /Fields`, and merge what they need: `/DR` fonts (renamed
  on a clash), `/DA` and `/Q` (pushed down onto the fields where they
  differ), `/CO`, and document-level scripts (name-tree keys renamed
  on a clash) (bug 2).
- Copying some of a form's pages copies only the widgets on those
  pages. Today a field's widgets on the pages left behind come along
  with a null `/P` (bug 2).
- **Names that clash are renamed, never merged** (bug 3).
  - A clash on any node of the incoming tree, terminal or not,
    counts: every IRS form's root is `topmostSubform[0]`, with
    `Page1[0]` under it. The incoming form's fields then all go under
    a new parent field named for its file (`fw9.topmostSubform[0]…`,
    then `fw9_2`).
  - Nothing is grafted under an existing node, which would change the
    kids' inherited `/FT`, `/Ff`, `/DA` and `/Q`. Merging would link
    the two copies' values, which is the known complaint about
    Acrobat's merge.
  - The rename is decided when the edit runs and kept in it, so the
    names sheaf shows are the names it saves.
  - A renamed form's own scripts still name the old fields. When the
    incoming form has scripts, the notice says so and suggests
    flattening it first (Insert and Combine offer that; slice 15).
- **Scripts from more than one source don't mix.** Document scripts
  share one global scope, so renaming name-tree keys doesn't separate
  them: GA 500 and OH IT 1040 both define `DS_Init`, `DS_Arrays` and
  `UFOOOOOxa`, from different Ultraforms versions. When more than one
  source in the result brings document or field scripts, scripts are
  off for that document by default, with a notice offering to flatten
  the incoming forms or keep only the first form's scripts.
- **Duplicate Pages gives the copy its own widgets** (bug 4), renamed
  by the same rule, so the copy is a separate form. Acrobat may link
  them instead (to check; see Asks).
