# M18 slice 16: pdf-content-streams: operators

Status: open · Part of [M18: the PDF layer](../README.md) · in pdf-lint's workspace, by pdf-lint's agent

**What:** a page's content as an iterator of operators, from sheaf's
lexer, which `tier-lexer` holds to PDFium: no page's tokens differed
over 23,628 (Greg, 2026-10-27: "an iterator consuming operators").

- **`Operations`, a standard `Iterator`:** each item an operator (or an
  inline image), where it is, and its operands' ranges, held inline in
  a small array, so there's no allocation per operator. sheaf's
  `lex::steps` hands its steps to a callback only because their
  operands borrow one buffer that's reused.
- **Operands as written,** parsed when asked for: a number, a name, a
  string, or a pdf-objects object for an array or a dictionary. Operand `i`
  from the last, as PDFium takes them (`Step::arg`).
- **Typed operators** over it, as sheaf's interpreter has them
  (`content/interp/op.rs`): each told by its name once, with `Other`
  for one it doesn't know.
- **A page as one stream:** the caller gives the page's `/Contents`
  streams, decoded; they're joined with a space, as PDFium reads them,
  and every position maps back to its stream (`content/page.rs`'s
  `Concat`). pdf-content-streams needs no resolver.
- **Inline images** end by PDFium's `ReadInlineStream` rules, from the
  color spaces' components the caller gives (`ResCx`) and the filters'
  end markers.
- **Caps** (`ContentLimits`), errors that say where, and every place
  ISO's rules would read the bytes otherwise said as a fault, which a
  validator wants.
- **The operator table,** from ISO 32000-2's Annex A: each operator,
  its operands' count and types, and the graphics-object states it's
  allowed in (§8.2), as data for the typed operators and pdf-lint's
  checks.
- **Over it:** `q`/`Q` balance, marked content's ranges and the
  resource names content uses (`balance.rs`, `marked.rs`, `names.rs`).

**From sheaf** ([from-sheaf.md](../from-sheaf.md#content-pdf-content-streams-slices-16-to-19)):
`content/lex.rs`, `page.rs`'s `Concat`, `balance.rs`, `marked.rs`,
`names.rs`, `interp/op.rs`'s operators, and their tests.

**Where:** `crates/pdf-content-streams/`.

**Checked:**
- sheaf's lexer tests (`content/tests.rs`) and those of the modules
  that move, moved, unchanged;
- every page of sheaf's corpus: the same operators and operands as
  sheaf's lexer, run in sheaf until [slice 17](s17-sheaf-s-content-on-pdf-content-streams.md);
- a fuzz body: any bytes, no panic, under the caps;
- time beside sheaf's lexer on the heavy 7k file: no slower.

pdf-lint's content checks are pdf-lint's to add on it
([from-sheaf.md](../from-sheaf.md#for-pdf-lint-itself)).
