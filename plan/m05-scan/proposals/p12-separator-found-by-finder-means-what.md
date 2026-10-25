# M5 proposal 12: A separator is found by a finder and means what its rule says

Part of [M5: Scan & OCR](../README.md) · [all proposals](README.md)

**A separator is found by a finder and means what its rule says.**
```rust
pub enum Finder {
    /// sheaf's own sheet: a QR code whose text starts SHEAFSEP.
    Sheet,
    /// Other software's sheets: a barcode of these symbologies whose
    /// text matches `pattern` (none: any text).
    Barcode { symbologies: Vec<Symbology>, pattern: Option<Regex> },
    /// Kodak patch codes of these types.
    Patch { codes: Vec<PatchCode> },
    /// A page with little else on it whose text matches `pattern`.
    Keyword { pattern: Regex },
}
pub struct Rule {
    pub finder: Finder,
    pub action: Action,            // Bookmark | Split | Both
    pub title: Title,              // the code's text | a template ("{value}", "{n}", "{date}")
    pub level: Level,              // Top | UnderLast (top while there's no top one yet)
    pub separator: Keep,           // Drop | Keep
    pub place: Place,              // Starts (the section after it; the default) | Ends (the section before it)
    pub when: When,                // Each (every page found) | Changes (only when the code's text changes)
}
```
- **sheaf's sheets are always looked for**, and carry their own
  action, title, level and keep/drop (proposal 13); no rule is needed
  to read one, on any machine, so a sheet printed at one desk is read
  the same at every other.
- **Rules for other separators** come from the Separate dialog (its
  last ones in `state.json`) or `sheaf separate --find …`. A firm
  that has a stack of PaperStream patch sheets, or its own "SEPARATOR"
  page made in Word, sets a rule once.
- **`When::Changes`** is for codes printed on each document's own
  pages rather than on a sheet (a label on every page of a
  document): a new section starts only when the text differs from
  the last page's, and the page is kept (Kofax's and PaperStream's
  "separate on change").
- **`Place::Ends`** is for trailer separators: a sheet put after each
  document rather than before it, as Kofax Scan Client allows
  (header and trailer separators, each kept or dropped). A trailer
  ends the section before it, and pages after the last trailer are a
  section of their own titled "End". sheaf's own sheets always start
  a section; `Ends` is for other software's sheets, and starting is
  every rule's default.
- **"Any barcode" is never a default.** Tax documents carry barcodes
  of their own: the corpus's forms have Code 39 form ids, Code 128s,
  a Data Matrix and a QR (the bench found them), and W-2s and K-1s
  carry PDF417 (UltraTax had a utility that read them; unverified). A
  barcode rule has a pattern unless the user clears it, and the
  review shows what each separator read.
- **A keyword page must be mostly empty:** the match plus at most 60
  other letters, as Find Blank Pages' "intentionally blank" check
  does (`blank::text_says`), so a page that mentions the word isn't a
  separator. The page's text is its own, its OCR text, or, while
  scanning with OCR on, what OCR read. A page with no text can't
  match; the review counts those pages and says to run OCR.
- Alternative: separators by configuration only, as CCH Scan,
  PaperStream and Kofax do (a sheet means what the job profile says).
  Self-describing sheets are better on the merits: no profile to keep
  alike across desks, and the meaning travels with the paper.
