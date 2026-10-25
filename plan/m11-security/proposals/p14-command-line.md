# M11 proposal 14: Command line

Part of [M11: Security & redaction](../README.md) · [all proposals](README.md)

**Command line.**
- `sheaf redact` applies the marks in a file, or a pattern, to a
  copy.
- `sheaf sanitize` removes hidden information into a copy.
- `sheaf protect` sets passwords and permissions; `--decrypt` removes
  them.
- Each prints the same report as the app. M15's folder batches build
  on these.
