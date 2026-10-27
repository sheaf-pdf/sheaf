# M18: what sheaf has for the crates

Part of [M18: the PDF layer](README.md): sheaf's code that pdf-lint's
agent can take into the crates, or work from, by crate and slice; and
checks pdf-lint could add from it. Surveyed 2026-10-27 at b33ee07 (Greg
asked: "is there work that sheaf has already done that pdf-lint can
extract or at least work from").

- **Paths** are sheaf's, under `crates/sheaf-doc/src/` unless they say
  otherwise; line counts are without tests.
- **Licence:** sheaf is MIT OR Apache-2.0, so its code moves into the
  family's crates (MIT OR Apache-2.0) and pdf-lint (Apache-2.0) as it
  is. `vendor/lopdf/` is lopdf's MIT code: a model, not code to copy,
  except the hunks marked `sheaf:`, which are sheaf's.
- **Coupled to** says what a module reaches besides the standard
  library: lopdf's types become pdf-cos's when it moves; sheaf's own
  modules (`materialize`, `source`, `annot`, `geom`) are what has to be
  untangled first.

## Object streams (#57): slice 7, and pdf-lint

| Module | Lines | What | Coupled to | Goes to |
|---|---|---|---|---|
| `serialize/objstm.rs` | 96 | Which objects may go in a container (`packable`: generation 0, not a stream, not an encryption, linearization or signature dictionary); the container (`/First`, the header of offsets, members as direct objects, deflated, its `/Length` direct); its cap tied to the reader's, so a file sheaf writes is one it reads | lopdf's types, `serialize/object.rs` | pdf-structure's writer ([slice 7](slices/s07-pdf-structure-writing.md)) |
| `serialize/group.rs` | 49 | Containers grouped by the pages that use their members, in Annex F's parts, page 1's apart | nothing | the linearization crate (between milestones, item 4); pdf-structure's writer can call it |
| `serialize/full.rs` | 463 | Containers built on threads, each encrypted once, whole; none where streams are left in the clear (`/StmF /Identity`) and strings aren't, since the members' strings would be in the clear (`Crypt::packs`, `serialize/crypt.rs`) | `materialize`, lopdf's `Document`, in-progress | its generic half to slice 7 |
| `serialize/xref.rs` | 255 | Type 2 entries (an object in a container), cross-reference streams under PNG Up | lopdf's types | slice 7 |

Keeping signature dictionaries out of containers is sheaf's caution (a
later signing tool finds one where it expects), not ISO's rule.

## The rest of the writer: slices 3, 6 and 7

| Module | Lines | What | Coupled to | Goes to |
|---|---|---|---|---|
| `serialize/object.rs` | 257 | Objects as syntax: a real in its shortest form that reads back (past i64's range too), names and strings byte by byte, for speed | lopdf's types | pdf-cos ([slice 3](slices/s03-pdf-cos-editing-and-writing.md)) |
| `serialize/xref.rs` | 255 | Tables and streams; free entries linked from entry 0; offsets counted from `%PDF-`, as PDFium, pdf.js and qpdf count them; an update's section lists only what it writes | lopdf's types | pdf-structure (slice 7) |
| `serialize/crypt.rs` | 128 | What a save encrypts: every string, a stream dictionary's included, and every stream, an embedded file's by `/EFF` and a stream's own `/Crypt`; not the encryption dictionary, a cross-reference stream, metadata under `/EncryptMetadata false`, a signature's `/Contents`. That's the list pdf-crypt leaves to its caller. The IVs, a keyed hash of the object's key and its plaintext (HMAC-SHA-256), stay sheaf's | `materialize` (`ObjKey`), lopdf's encryption, hmac, sha2 | the list to pdf-structure (slices 4 and 7); the IVs are why [slice 6](slices/s06-pdf-filters-and-pdf-crypt-for-writing.md) keeps the IV the caller's |
| `serialize/append.rs` | 700 | An incremental section after a file's last: what it writes, its cross-reference section, `/Prev` | `materialize`, `source`, `serialize/durable.rs` | its generic half to slice 7 |

## Reading: slice 5's models

| Module | Lines | What | Goes to |
|---|---|---|---|
| `holds_linearization` (`vendor/lopdf/src/lazy.rs:994`, a `sheaf:` hunk) | | B3's check: whether a linearization dictionary still holds, `/L` against the file's length | pdf-structure's facts ([slice 5](slices/s05-pdf-structure-lazy-reads.md)); a pdf-lint rule (below) |
| `source/limits.rs` | 148 | Every cap a reader runs under, one value | pdf-structure's limits value |
| `source/fuzz.rs` | 1,747 | The fuzz bodies of `fuzz/`: open and read lazily, any bytes, under the caps | pdf-structure's fuzz body |
| `source/lazy_tests.rs` | 1,248 | Every object of the corpus's readable forms read lazily equals the eager load | slice 5's "whole and lazy agree" |

## For pdf-lint itself

Not M18's slices, but sheaf already has them; pdf-lint's agent and
Greg decide.

- **`ObjectStream`:** pdf-lint 0.1.0 doesn't check object streams.
  sheaf's tests stand in (`crates/sheaf-ops/tests/lint/mod.rs`, from
  line 172: each container sheaf packs, walked against the model's
  `ObjectStream` table). It can become pdf-lint's own check, with or
  after [slice 2](slices/s02-pdf-cos-and-arlington-cut-from-pdf-lint.md)'s
  cut of arlington. PR #58 listed it among the gaps it found.
- **ISO 32000-1 7.5.7's list,** on read: no stream, no object of
  generation other than 0, no encryption dictionary and no container's
  `/Length` object in an object stream; in a linearized file, not the
  catalog, the linearization dictionary or a page object either.
  `objstm::packable` is the writer's side of the first part; the
  linearized part is the linearization crate's to write.
- **Strings left in the clear by an object stream:** a container in a
  file whose streams are `/Identity` and whose strings are encrypted
  holds its members' strings unencrypted. `Crypt::packs` is the
  writer's side.
- **A linearization that no longer holds,** after an incremental save:
  B3's check, above.
- **Content checks,** once the lexer moves (below): unbalanced
  `q`/`Q`, marked content that doesn't end, a resource name content
  uses that `/Resources` lacks, and resources nothing uses.

## Content: pdf-content, slices 16 to 19

| Module | Lines | What | Coupled to | Slice |
|---|---|---|---|---|
| `content/lex.rs` | 967 | Tokens as PDFium's `CPDF_StreamParser` ends them; an inline image's end by PDFium's `ReadInlineStream` rules, from the resources' color spaces (`ResCx`) and the filters' end markers; caps (`ContentLimits`); errors that say where; `steps`, an operator at a time. `tier-lexer` holds it to PDFium: no page differed over 23,628 | `source::Limits`, lopdf's `Object` (operands) | 16 |
| `content/page.rs`'s `Concat` | | A page's streams joined as PDFium reads them, every position mapped back to its stream | `ReadObjects` (the streams are the caller's in pdf-content) | 16 |
| `content/interp/op.rs` | 251 | The operators, each told by its name once, `Other` for one it doesn't know | the lexer | 16 |
| `content/balance.rs` | 40 | A page's `q`/`Q` balance | `source::Limits` | 16 |
| `content/marked.rs` | 195 | The ranges `BMC` and `BDC` open and `EMC` ends | `source::Limits`, lopdf's types | 16 |
| `content/names.rs` | 189 | The resource names content uses, and what `/Resources` must keep | `source::Limits` | 16 |
| `content/splice.rs` | 554 | Ranges taken out of a page's streams, every other byte as it was, only where that can't change what's drawn after | the lexer, `Concat`, `source::Limits` | 19 |
| `content/writer.rs` | 710 | Content written the same way every time, resources named by digest when it's finished | sheaf's fonts, `annot`, `geom`, `ReadObjects`, `serialize/object.rs`, lopdf's types | 19 |
| `content/res.rs` | 141 | Resources by what they are, a digest each | lopdf's types | 19 |

- The lexer's rules become pdf-content's PDFium policy
  ([sheaf.md](sheaf.md#pdf-tokenizer) has where they differ from the
  tokenizer's ISO rules), and slice 18 moves its scanner onto
  pdf-tokenizer if that's proved. `balance`, `marked` and `names` need
  only the lexer, and give pdf-lint its content checks.
- `splice` and `writer` are pdf-content's writing half, which pdf-repair
  would want too (taking a broken operator out without decoding the
  stream into operations). `writer` is untangled from sheaf's fonts and
  geometry first.
- **The interpreter** (`content/interp/`, 3,566 lines) draws as PDFium
  draws and is tied to sheaf's geometry: it stays sheaf's, on
  pdf-content.

## pdf-semantics: slices 20 and 21

| What | sheaf | pdf-lint |
|---|---|---|
| Text strings (7.9.2.2) | read: `file.rs`'s `text_string`; written in three places: `save.rs`, `stamp_lib.rs`, `annot.rs` | `text.rs`: decoded, every departure said |
| Dates (7.9.4) | written: `annot::pdf_date_now` | checked: `check::is_date` |
| Name trees (7.9.6) | `attachments::name_tree`; the scripts' tree in `materialize/emit/form.rs` | `predicate::Trees` |
| Number trees (7.9.7) | `labels.rs`: read (`collect_nums`) and written (`number_tree`) | the checker's walk |
| The page tree (7.7.3) | `source/pages.rs` (428 lines): `/Kids` depth first, inherited keys up `/Parent`, stopping at `/Count` where PDFium does | the checker's walk, as ISO has it |
| XMP claims (14.3.2) | `source/conformance.rs` (149 lines): PDF/A and PDF/UA, on quick-xml | `extension.rs`: the extension subsets |

- pdf-lint could report where PDFium's pages differ from the tree's
  (a `/Count` that's wrong, a page listed twice), once both walks are
  pdf-semantics' policies.
- A PDF/A profile in pdf-lint needs the conformance claim.

## Fonts, later

| Module | Lines | What | Coupled to |
|---|---|---|---|
| `content/font/encoding.rs` | 254 | Simple fonts' encodings, code to glyph name, glyph name to text | nothing |
| `content/font/afm.rs` | 101 | The standard 14 fonts' metrics (`assets/afm`, Adobe's licence in `assets/LICENSE-afm`) | nothing |
| `content/font/type1.rs` | 451 | A Type 1 program's tokens | nothing |
| `content/font/predefined.rs` | 277 | The predefined CMaps a Type 0 font's `/Encoding` names | hayro-cmap |

The rest of sheaf's font reading (`foreign.rs`, `program.rs` on skrifa)
is tied to its interpreter. A simple font's `/Widths` against the
standard 14's metrics would build on these, when pdf-lint wants font
checks ([scope](scope.md#weighed-for-the-family)).

## Test infrastructure

- **The oracles** sheaf tests against ([docs/oracles.md](../../docs/oracles.md)):
  qpdf, pdf.js, PDFium, veraPDF, MuPDF, poppler. poppler, MuPDF and
  veraPDF are GPL or AGPL: run as separate processes, never linked or
  copied from.
- **`tier-lexer`** (`crates/sheaf-render/tests/content_lexer.rs`): the
  lexer against PDFium, page by page.
- **[fetch-pdf-corpus](https://github.com/sheaf-pdf/fetch-pdf-corpus)**,
  public already: both repositories can test on the same corpus locks.

## Not to take

- `materialize/`: what a save writes and why is sheaf's policy.
- `serialize/durable.rs`: replacing a file durably; crates for atomic
  writes exist ([Libraries](../libraries.md)).
- The content interpreter (above).
