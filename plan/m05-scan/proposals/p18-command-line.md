# M5 proposal 18: The command line

Part of [M5: Scan & OCR](../README.md) · [all proposals](README.md)

**The command line.**
```
sheaf scan --list-devices [--json]
sheaf scan --device ID|NAME [--source feeder|duplex|flatbed] [--dpi N]
           [--mode color|gray|bw] [--page letter|legal|a4|auto]
           [--ocr] [--drop-blanks] [--separate [--find …]] [cleanup flags] -o OUT.pdf
sheaf scan IMAGES… [--profile photo|scanner] … -o OUT.pdf          (as today)

sheaf separate IN [--find sheet|patch:2,3,T|code39[:REGEX]|code128[:REGEX]
                          |qr[:REGEX]|datamatrix[:REGEX]|pdf417[:REGEX]|keyword:REGEX]…
                  [--action bookmark|split|both] [--title TEMPLATE]
                  [--keep-separators] [--split-dir DIR] [--names title|stem-title]
                  (-o OUT [--append] | --in-place | --dry-run) [--json]

sheaf separator-sheets IN (--above-bookmarks [--with-children] | --above 3,10,25)
                       [--titles FILE] [--action …] [--keep] -o OUT
sheaf separator-sheets --titles "W-2" "1099" … [--copies N] [--page letter|a4] -o sheets.pdf
```
- `--find sheet` is always on; each `--find` adds a rule with
  `--action`, `--title` and `--keep-separators` as given before it
  (or the defaults: bookmark, the code's text, drop).
- `separate --dry-run --json` prints the sections: where each
  separator is, what it read, the title, the pages. A split's ask
  (signatures, an XFA part's values) exits 3, as every subcommand's
  does.
- `--device` takes an id from `--list-devices` or a name's unique
  prefix; `--device first` takes the first one found.
- All three are thin translators over `Session` and the ops, as every
  subcommand is since slice 19.
