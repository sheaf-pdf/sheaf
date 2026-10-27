# M18 slice 17: sheaf's content on pdf-content

Status: open · Part of [M18: the PDF layer](../README.md) · in sheaf

**What:** sheaf's content reads through pdf-content
([slice 16](s16-pdf-content-operators.md)), and its own lexer goes.

- `content/lex.rs`, `lex::steps`, `balance.rs`, `marked.rs` and
  `names.rs` go; `page.rs` keeps reading a page's `/Contents` and hands
  the decoded streams to pdf-content.
- What reads content moves onto its iterator and typed operators: the
  interpreter (`content/interp/`), blocks (`blocks.rs`), `text_bag`,
  the fonts' embedded CMaps (`content/font/foreign.rs`), the splice.
- The interpreter stays sheaf's: it draws as PDFium draws, in sheaf's
  geometry.

**Where:** `crates/sheaf-doc/src/content/`.

**Checked:**
- `tier-lexer`: no page's tokens differ from PDFium's;
- the interpreter's, blocks' and text bags' tests, unchanged
  (`tier-interp` too);
- `materialize_corpus`: counts and output bytes as before, since
  content assembly and splicing read the same steps;
- the 7k gate within its flags.
