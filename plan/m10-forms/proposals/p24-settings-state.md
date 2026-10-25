# M10 proposal 24: Settings and state

Part of [M10: Forms](../README.md) · [all proposals](README.md)

**Settings and state.** `settings.toml` gains `[forms]`: `scripts`
(on), `auto_calculate` (on), `auto_advance` (off; proposal 14), and
the `highlight` and `required` colors. `state.json` keeps the
Highlight Fields toggle, Prepare Form's last tool, the last export
format and CSV choices (Excel-safe included), and the flatten and
detection choices. Choices for one document (Run Scripts off, "Don't
show more from this document") last the session; nothing is kept by
a client file's path. No settings view (M9 decision 14).
