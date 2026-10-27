# M18 slice 18: One scanner: PDFium's rules as a pdf-tokenizer mode

Status: open · Part of [M18: the PDF layer](../README.md) · in pdf-lint's workspace, by pdf-lint's agent

**What:** the family scans bytes one way (the notes: "don't fork the
parser"), with the policy a parameter: pdf-tokenizer gains PDFium's
rules as a mode, and pdf-content's scanner moves onto it.

- **The mode:** where sheaf's lexer and pdf-tokenizer differ
  ([sheaf.md](../sheaf.md#pdf-tokenizer)): a word of digits, signs and
  points is a number to PDFium (`--5`, `1.2.3`); a hex string runs to
  `>`; whatever else a corpus comparison finds. ISO's rules stay the
  default.
- **pdf-content on it:** its own scanner goes; inline images' data is
  still found by pdf-content, which then has the tokenizer go on past
  it.
- **pdf-objects can read under either mode,** so sheaf's model and its
  content are read by the same rules.
- **Taken only if proved:** the same tokens as pdf-content's own scanner
  on every page of sheaf's corpus, `tier-lexer` unchanged, and no slower
  (Greg's ruling on speed). If it isn't, pdf-content keeps its scanner
  and this slice's notes say why.

**Where:** `crates/pdf-tokenizer/`, `crates/pdf-content/`.

**Checked:**
- the token comparison over the corpus, and `tier-lexer` in sheaf;
- pdf-tokenizer's own tests in both modes, and its fuzz body in both;
- benchmarks of both scanners on the heavy 7k file's content.
