# M11 proposal 13: Search and redact through the Patterns panel

Part of [M11: Security & redaction](../README.md) · [all proposals](README.md)

**Search and redact through the Patterns panel.**
- "Mark for Redaction" on the hits, beside "Bookmark matches": Check
  All or one at a time.
- Word and phrase lists, alongside regular expressions.
- **Whole or partial words, as Acrobat offers them** (decided by Greg,
  2026-10-14).
  - Acrobat's Redaction Mark Options: whole words, or partial words
    with a settings dialog for how many characters to redact and
    where they sit in the word.
  - sheaf does the same: a general setting for any search, not one
    tied to a kind of number. For example, all but four characters
    at the end, or the first five.
  - The mark covers just those characters' boxes, from PDFium's
    per-character boxes, or the OCR layer's for scanned pages.
  - Whole words is the default, and the last setting is remembered in
    `state.json`.
  - The docs say what a partial mark leaves visible. Leaving the last
    four digits of an SSN is common (26 CFR 301.6109-4 allows it on
    payee statements). But for SSNs issued before the 2011
    randomization, Acquisti and Gross (PNAS, 2009) showed the first
    five digits can often be predicted from birth date and state. So
    the last four next to a date of birth come close to the whole
    number.
- New presets: email, credit card (with a Luhn check), ITIN.
- It searches OCR text, which Acrobat's Search & Redact can't
  (image-only text).
