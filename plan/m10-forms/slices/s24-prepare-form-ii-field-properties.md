# M10 slice 24: Prepare Form II: field properties

Status: open · Part of [M10: Forms](../README.md)

**Prepare Form II: field properties** (proposal 15).
- **On the edit API:** `PatchFields`, and `PatchWidgets` beyond
  slice 13's flags, through `FieldNode.patch` and widget patches;
  `RenameField` and `MoveField` for the hierarchy.
- The properties dialog, built like the plugin form card
  (`plugin_form.rs`), with its General, Appearance and Options tabs.
- The Fields panel's hierarchy and rename.
- **Checks:** each tab's choices reach the file as Acrobat writes
  them and read back; a rename keeps the value.
