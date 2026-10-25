# Build or adopt 97: Settings (toml)

Area: Infrastructure that's built (confirmed) · Part of [build or adopt](README.md)

- **Today:** `toml = "0.9"` in sheaf-ops and sheaf-plugin (0.9.12
  locked). sheaf never writes settings, so read-only serde is right;
  toml_edit isn't needed.
- [toml](https://crates.io/crates/toml) 1.0.0 came out on 2026-02-11 and
  is at 1.1.8 (2026-10-09; MIT OR Apache-2.0). The lock also has 1.1.8
  (through embed-resource) and 0.8.23 (through cbindgen).
- **Recommendation:** **adopt** (keep), and move to 1.x in a dependency
  refresh, before M15's "settings/keymap format declared stable"
  (`ROADMAP.md:477-478`). High.
