# M18: what sheaf takes from each crate

Part of [M18: the PDF layer](README.md): where sheaf stands, what each
crate replaces in sheaf and what sheaf needs from it, and what the
switch would show. The crates themselves are [crates.md](crates.md).
Written 2026-10-26 as a memo in `plan/topics/` (exploring), moved here
when Greg asked for the milestone's plan (2026-10-27). It builds on
[F13](../backlog/F13.md) and [build-or-adopt entry
1](../research/build-or-adopt/01-pdf-object-layer.md) (the object
layer's survey), and doesn't repeat them.

## Where sheaf stands

sheaf already copies unchanged objects as they were, within limits:

- **The rule** (`materialize/mod.rs:289-293`): a Rewrite copies a
  source object's bytes (`Body::Raw`) only when numbering is stable
  (`Numbering::Stable`), the source read soundly (`base_sound`) and its
  encryption is unchanged (`same_crypt`). An object the emitter or
  `keep_only` changed, or one that names a moved object, is written anew.
- **How much:** `materialize_corpus` copies 919,465 of the 2,269,688
  primary objects that survive, about 40%. The rest are re-serialized
  from lopdf's objects.
- **Where the other 60% go:**
  - **Spans come from a second parse.** `raw_span_whole`
    (`vendor/lopdf/src/lazy.rs:622-636`) parses each object again to find
    its range, on up to 8 threads (`materialize/mod.rs:572-599`).
  - **An object stream's members have no span,** so they're always
    re-serialized, though their bytes are plain once the container is
    decoded.
  - **Renumbering turns copying off.** A copied object names others by
    number, so a profile that renumbers writes everything anew.
  - **A stream whose `/Length` is a reference isn't copied:** pruning
    could drop the length object the bytes still name.
- **File access is already what the notes recommend.** Each source is
  one owned `Arc<[u8]>` (`Source::bytes`), named by its xxh3-128
  checksum (`sheaf_util::checksum`), and `store.rs` refuses two
  different files with the same checksum. An Append reads the file on
  disk by position (`lopdf::FileBytes`) and stops if its length changed
  since it was opened (`materialize/mod.rs:220-228`): changes are
  detected, not prevented ([file contention](../topics/file-contention.md)).
- **The notes' differential test is `source::lazy_tests`:** every object
  of the corpus's readable forms, read lazily, equals the eager load.

## What sheaf uses of lopdf (2026-10-27)

- **Reach:** 203 files name `lopdf::`, 101 of them outside tests,
  examples and fuzzing (141 in sheaf-doc, 29 sheaf-ops, 15
  sheaf-render, 11 sheaf-cli, 4 sheaf-app, 2 sheaf-ocr, 1
  sheaf-plugin); entry 1 counted 446 lines in 182 files. The most named:
  `ObjectId`, `Result`, `Object`, `Dictionary`, `Document`, `Stream`,
  `Error`, the `dictionary!` macro, `StringFormat`, `xref::XrefType`.
- **The eager `Document`:** 39 files outside tests, among them the
  Rewrite graph (`materialize/graph.rs`, `copy.rs`, `kept.rs`,
  `number.rs`, `verify.rs`), PDFs built in memory (`annot.rs`,
  `serialize/built.rs`, the `fixture.rs` modules, `stamp_lib.rs`),
  `recovery.rs`, and sheaf-app's OCR and scan runs.
- **Reading:** `LazyPdf`, `ByteSource`, `FileBytes`, `ReadLimits`,
  `XrefFacts`, and lopdf's errors (`InvalidPassword`, `ObjectNotFound`,
  `MissingXrefEntry`, `Decompress`), in `source/`.
- **Encryption:** `encryption::crypt_filters`, `decrypt_object`,
  `EncryptionState`, under `serialize/crypt.rs` and `source/`.
- **Content:** none in production. `content/` lexes on its own;
  `lopdf::content::Content::decode` is a test oracle
  (`content/corpus.rs`, `forms/check.rs`).
- **Its writer:** `annot::tape_pdf`, tests and fixtures.
- **Docs:** `docs/document-model.md` names lopdf on 47 lines.

## Crate by crate

### pdf-tokenizer

- **Replaces:** the scanning under lopdf's object parser, through
  pdf-cos, and under pdf-content's scanner if [slice 18](slices/s18-one-scanner.md)
  proves it.
- **Differences to absorb above it:** sheaf's content lexer follows
  PDFium, the tokenizer ISO.
  - A run of digits, signs and points is a number to PDFium (`--5`,
    `1.2.3`); the tokenizer makes those keywords.
  - A hex string runs to `>` in PDFium; the tokenizer ends it at a byte it
    can't hold.
  - sheaf's lexer stops with an error saying where (`LexError`), where
    the tokenizer marks a token unclosed.
- **Needs:** nothing more. The policy belongs in the layers above.

### pdf-cos

- **Replaces:** lopdf's `Object`, `Dictionary` and `Stream` (with
  sheaf's patch: f64 reals, dictionary order), its object parser, and
  `serialize/object.rs`.
- **The notes' two stream extents** (the `/Length` declared, which may
  be an unresolved reference, and the end found by scanning for
  `endstream`) are what sheaf's two spans tell apart today:
  `raw_span_whole` refuses a stream whose `/Length` is a reference or
  doesn't fit its data. Recorded at the first parse, that check is free.
- **Needs:**
  - **An owned, editable form** that replaces `lopdf::Object`
    throughout sheaf, the migration's real cost. pdf-lint's `object.rs`
    is its start, each value owned with its span; what it lacks is
    what an edit does to a span (a changed value has none, so it's
    written anew), and the calls sheaf makes on lopdf's types
    ([slice 1](slices/s01-one-door-for-lopdf.md)'s inventory).
  - Reals at f64, dictionary order kept, a key written twice visible
    (`object.rs` keeps every entry; a lookup takes the last, as
    readers do).
  - Serializing as `serialize/object.rs` does, byte for byte, so
    sheaf's output doesn't move.
  - Caps on depth and size, and a fuzz body.

### pdf-filters

- **Replaces:** lopdf's filters, and the predictor code under them.
- **Needs:**
  - **Encoding,** in scope by the ruling. A save deflates object streams
    and content, and a whole file's cross-reference stream under the PNG
    Up predictor (`serialize/xref.rs`).
  - **Both Flate backends** (Greg, 2026-10-27), zlib-rs and
    miniz_oxide, the caller naming one
    ([slice 6](slices/s06-pdf-filters-and-pdf-crypt-for-writing.md)):
    sheaf probably zlib-rs, for speed, pdf-lint probably miniz_oxide.
    [D4](../research/build-or-adopt/decisions.md#d4-one-flate-backend-for-the-workspace)
    stays sheaf's: one backend in every sheaf build, so the same input
    gives the same bytes ([entry 4](../research/build-or-adopt/04-flate-one-backend-every-build.md)).
  - The output cap it has, wired to sheaf's `ReadLimits`.
  - Image filters stay out: [entry
    33](../research/build-or-adopt/33-pdf-image-samples-image-s-own.md)
    owns them.

### pdf-crypt

- **Built:** revisions 2 to 6 (5 as Adobe's Extension Level 3) on
  RustCrypto's primitives; crypt filters by name, `/Identity` included;
  every fault said, what decrypted kept; the keys kept in the `Handler`;
  a `writer` feature that encrypts for test files, its salts fixed.
- **Replaces:** lopdf's security handler (vendored, with sheaf's fixes:
  V4+ filter names and `/Identity`, `/EFF`, `/Encrypt null`), and the
  key handling under `serialize/crypt.rs`.
- **Against sheaf's needs** ([entry
  29](../research/build-or-adopt/29-standard-security-handler-read-r2-r6.md)):
  - R2 to R6 read, on the RustCrypto crates (F13): built.
  - Crypt filters as the file names them, an Identity `/StmF` under an
    encrypting `/StrF` included (object streams' `Crypt::packs`):
    `Handler::streams`, `strings` and `named`.
  - **The `Method`, not the keys.** Whether a copy stays valid comes
    from the `Method`: `Rc4` and `Aes128` key each object by its number
    and generation, `Aes256` uses the file's key. The keys stay in.
  - **Encrypting, the gap.** A save of an encrypted file encrypts what
    it rewrites with the source's handler, so sheaf needs
    `encrypt_string` and `encrypt_stream`. They take the IV from the
    caller, as sheaf's keyed-hash IVs need (M10.5 §11.7), but are behind
    the `writer` feature. By the ruling, the split:
    - the two encrypt functions without the feature;
    - a dictionary builder taking its random bytes (salts, and
      AES-256's file key) from the caller, for M11's new passwords;
    - `writer::dictionary`, its salts fixed, for tests.

### pdf-structure

- **Replaces:** lopdf's `reader.rs`, `xref.rs` and `object_stream.rs`,
  `LazyPdf` ([crates.md](crates.md#where-lazypdf-goes-pdf-structure-not-pdf-semantics)),
  and the generic half of sheaf's `serialize/xref.rs`, `objstm.rs`,
  `full.rs`, `append.rs` and `crypt.rs`.
- **Streams live here** (the notes): decoding needs the dictionary, the
  security handler and the object's id together, and encryption covers
  strings too, with exceptions. sheaf's `serialize/crypt.rs` lists those
  exceptions as lopdf's walk decides them.
- **Needs:**
  - **Lazy reads under caps** (`ReadLimits`), over a byte source read
    by position, so a huge file needn't be read whole. pdf-lint's reader
    reads a file whole into a `Document`;
  - **each object's span from its first parse,** so no second parse;
  - **where each object came from,** object-stream members included;
  - the facts as written that sheaf reads today (`XrefFacts`,
    `base_sound`, hybrids), and the linearization check (B3's
    `holds_linearization`);
  - writing: cross-reference tables and streams, object-stream
    containers, a whole file and an incremental section, encrypted and
    compressed, with the copy rules of [crates.md](crates.md#decisions-across-the-family).
- **Builds on it later:** the linearization crate (between milestones,
  item 4) is independent of any object model, and F15's Rewrite that
  doesn't load the file whole reads through the lazy resolver.

### pdf-content

- **Replaces:** `content/lex.rs` and its `steps`, `balance.rs`,
  `marked.rs`, `names.rs` and `page.rs`'s joining of a page's streams
  ([slices 16](slices/s16-pdf-content-operators.md) and
  [17](slices/s17-sheaf-s-content-on-pdf-content.md)); then
  `writer.rs`, `res.rs` and `splice.rs` ([slice 19](slices/s19-pdf-content-writing.md)).
- **A correction to the notes:** they say content is shared by the
  linter and "Sheaf rendering". PDFium renders sheaf's pages. sheaf's
  lexer feeds its interpreter (what a page holds, what an edit changes),
  which is why its rules must be PDFium's: the model has to agree with
  the pixels.
- **Needs:**
  - PDFium's rules, held by `tier-lexer`, as a policy beside ISO's;
  - inline images' ends by PDFium's rules, from the color spaces' components
    sheaf gives (`ResCx`);
  - operands as written, parsed only when asked for, and typed operators
    the interpreter reads;
  - its caps and errors (`LexErrorKind`);
  - no slower than sheaf's lexer: the interpreter, blocks and text bags
    lex every page they read.
- **Stays sheaf's:** the interpreter, which draws as PDFium draws.

### pdf-semantics

- **Replaces** ([slice 21](slices/s21-sheaf-on-pdf-semantics.md)):
  sheaf's text strings (one reader, three writers), its dates, its name
  and number trees, the page table's walk and the conformance claims.
- **Needs:**
  - the page tree walked as PDFium walks it, as a policy, on the open
    path: the 7k gate's open walks it;
  - where a value came from (inherited, defaulted), which Properties
    and form reading want;
  - writing as well as reading: sheaf writes text strings, dates and
    number trees.

## The notes' speed seams, against sheaf today

| Seam (the notes) | sheaf today |
|---|---|
| Borrowed bytes, lazy unescaping, interned names | Owned `lopdf::Object`s; names decoded on parse |
| `memchr` or SIMD for delimiters, `endstream`, `startxref` | No `memchr`: lopdf's parser and `content/lex.rs` scan a byte at a time |
| Skip to an object's end without building it | `raw_span_whole` builds the object to find its end |
| Object cache; object streams decoded once | `LazyPdf` keeps each object read and decodes each object stream once |
| A budgeted cache of decoded streams | None in `source/` or `LazyPdf` |
| Linearization fast path | The linearization crate (between milestones) |
| Streaming or capped decode | Capped (`ReadLimits`), not streaming |
| Per-page parallelism; the resolver `Sync` or sharded | `LazyPdf` is shared across threads (`raw_spans`' 8) |
| Writer: unmodified streams passed through | `Body::Raw` for about 40% of objects |
| Incremental append; parallel compression | Append is sheaf's; object-stream containers are built on threads (`serialize/full.rs:388`) |

## What sheaf would gain

| Change | What it would show | Measured by |
|---|---|---|
| Spans from the first parse | No second parse of every copied object | The 7k gate's Rewrite (808 ms at #57) |
| Object-stream members copied as decoded | More objects copied, identical output where nothing changed | `materialize_corpus`'s copied count |
| References rewritten in place on renumbering | Copies under any numbering, not only `Numbering::Stable` | The same count, on renumbering profiles |
| Spans on every value | Errors that point at bytes | sheaf's errors and the Arlington tests |
| No vendored lopdf | clippy and the lint bans on the whole PDF layer; no patch to reapply | the workspace |

## pdf-lint without qpdf

pdf-lint 0.2.0 (unreleased) decodes and decrypts through pdf-filters and
pdf-crypt, and keeps qpdf for its own tests; its archives hold none.
Once it's out, a bump of sheaf's dev-dependency lets sheaf's tests read
through pdf-lint alone, and drops `PDF_LINT_QPDF` and the macOS
`OPENSSL_MODULES` step PR #58 added (the bundled qpdf loads OpenSSL's
legacy provider only from the Homebrew path it was built against). It
needs no M18 slice and waits only for the release and the pause.
