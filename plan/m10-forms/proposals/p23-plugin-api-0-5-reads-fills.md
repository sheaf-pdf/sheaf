# M10 proposal 23: **Plugin API 0.5 reads and fills fields; plugins never run document

Part of [M10: Forms](../README.md) · [all proposals](README.md)

**Plugin API 0.5 reads and fills fields; plugins never run document
JavaScript.** (Renumbered from 0.4 by M10.5, which took 0.4.)
- A WIT `fields` interface: each field's name, type, flags, value,
  options, page, rect and tooltip; and read-only form facts (class,
  scripts, usage rights). The edits are field ops in the JSON `Op`,
  which needs no WIT bump: setting values and resetting are a
  plugin's (`Cap::Plugin`); a flatten needs approval, as
  `FlattenAnnotations` does.
- Not `form`: the `ui` interface already has `form`, `field`,
  `field-kind`, `field-value` and `field-answer` for plugin dialogs.
- A plugin's fill goes through the same calculation as the user's
  when scripts are on.
- JavaScript isn't a host capability. Creating fields from a plugin
  can come later if asked for.
- One bump at the end (M9 decision 12): 0.5 replaces 0.4, and no 0.3
  or 0.4 is served (one world, exact-minor until alpha; M10.5
  proposal 18).
