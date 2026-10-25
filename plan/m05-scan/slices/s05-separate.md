# M5 slice 5: Separate

Status: deferred · Part of [M5: Scan & OCR](../README.md)

**Separate** (proposals 12, 15, 16 and 18).
- **Builds:** `separate::{find, plan, apply}`; Document > Separate…
  with its rules and the review sheet; the split to a folder;
  `sheaf separate`.
- **Checks:**
  - **The round trip:** a file with top-level bookmarks gets sheets
    inserted above them; every page is drawn to an image at 200 dpi
    and degraded as a scan (skew, noise, black and white); `sheaf scan
    --profile scanner` makes a PDF of the images; `sheaf separate`
    gives back the same titles at the same pages, and with `--action
    split`, files of the same page counts.
  - A trailer rule: patch-T sheets after each document make the same
    sections as the same sheets before them under a starting rule,
    with "End" for pages after the last.
  - Sheets fed upside down; a blank back after each sheet (duplex);
    two sheets in a row; a sheet on page 1; repeated titles (`_2`);
    pages before the first sheet ("Start").
  - A keyword rule doesn't take a page that mentions the keyword in a
    paragraph; a patch-T rule drops T sheets and a patch-2 rule keeps
    its page, as configured.
  - A split of a signed file stops with exit 3 (`NeedsDecision`), and
    nothing is written.
  - **Speed gate:** finding separators in the corpus's longest scan
    (`scans/safedocs/0055624.pdf`, 297 pages) takes no longer than
    drawing its pages at 150 dpi plus 60 ms a page divided by the
    decode threads.
