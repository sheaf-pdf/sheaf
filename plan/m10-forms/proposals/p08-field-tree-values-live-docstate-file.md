# M10 proposal 8: **The field tree and values live in `DocState`; the file stays

Part of [M10: Forms](../README.md) · [all proposals](README.md)

**The field tree and values live in `DocState`; the file stays
standard.**
- Read at open (`FileFacts`, `read_open`, file.rs:281, :399) and for
  inserted sources, with inheritance resolved: names, types, flags
  (`/Ff`), values (`/V`, `/DV`), `/DA`, `/Q`, `/MaxLen`, `/Opt`,
  `/TU`, actions (`/AA`, `/A`), and each widget's page, rect, flags,
  `/MK`, `/AS` and appearance states. Plus `/AcroForm`'s `/DR`, `/DA`,
  `/CO`, `/NeedAppearances`, `/SigFlags` and `/XFA`, and the catalog's
  `/Perms`.
- **Read defensively.** The tree comes from untrusted files and is
  read for every file at open, so a cycle would hang or overflow on
  any file, not just forms.
  - A visited set on `/Kids` and `/Parent`; depth at most 32; caps on
    field and widget counts, on `/Opt` length, and on the `/MaxLen`
    used for layout (1,000); rects, sizes and colors finite and
    clamped.
  - Over a cap, the form reads as "damaged: not fillable", with a
    notice, and the file still opens.
  - FDF and XFDF `<fields>` get the same limits.
- Kept in persistent `im` maps (`Send`), like marks. A field has a
  sheaf id,
  mapped to its source and object id; its full name is data. Widgets
  follow page ids, so moves and deletes carry them.
- Untouched fields' appearances, and keys sheaf doesn't read, stay in
  the source and are written back as they were.
- **No `/SheafField` JSON.** Fields aren't sheaf's marks: the standard
  keys hold everything, Acrobat round trips stay simple, and M11's
  sanitize has nothing extra to strip.
- Saving writes `/V` on the field, `/AS` on its widgets, and new
  appearances, by both strategies. An Append writes only the changed
  field and widget objects, and recovery sidecars carry values the
  same way (M10.5's emitter, §11).
