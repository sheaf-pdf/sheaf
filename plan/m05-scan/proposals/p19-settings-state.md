# M5 proposal 19: Settings and state

Part of [M5: Scan & OCR](../README.md) · [all proposals](README.md)

**Settings and state.**
- **`state.json`** (sheaf's, disposable): the scanner dialog's last
  device, source, resolution, mode, page size, cleanup rows, OCR,
  blank pages and separate options; the Separate dialog's rules,
  action, names and keep/drop; Insert Separator Sheets' where and
  what. `State::scan` splits into the two profiles' choices.
- **`settings.toml`** (yours, never written): a `[scanner]` section in
  the commented template:
  ```toml
  [scanner]
  # Kinds of scanner driver sheaf leaves out: "twain", "wia", "sane".
  # A driver that misbehaves can be left out for every scan.
  # skip = ["wia"]

  # Seconds a scanner may send nothing before sheaf gives up on it
  # (a flatbed's lamp warming up counts).
  # timeout = 60
  ```
  No separator rules in settings for now: the dialog remembers them.
  A firm that wants one shared rule list gets `[[separators.rule]]`
  when it asks.
