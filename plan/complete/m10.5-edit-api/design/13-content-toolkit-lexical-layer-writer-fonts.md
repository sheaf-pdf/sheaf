# M10.5 §13: The content toolkit: the lexical layer, the writer and fonts

Part of [M10.5: the design](README.md).


**Pulled forward from M11** into `sheaf-doc/src/content/`:

```rust
pub enum Tok<'a> { Op(&'a [u8]), Operand(&'a [u8]) /* raw text: numbers kept as written */, InlineImage(Range<usize>) }
pub fn lex<'a>(s: &'a [u8], res: &ResCx, lim: &Limits) -> impl Iterator<Item = Result<(Range<usize>, Tok<'a>), LexError>>;
pub struct Marked { pub tag: Name, pub props: Option<Props>, pub span: SpanAcross, pub depth: u8 }
pub fn marked(streams: &[&[u8]], props: &PropertiesDict, lim: &Limits) -> Result<Vec<Marked>, LexError>;
pub struct Balance { pub net: i32, pub min_prefix: i32 }
pub fn balance(streams: &[&[u8]]) -> Result<Balance, LexError>;
pub fn splice(streams: &[&[u8]], drop: &[SpanAcross]) -> Result<Vec<Vec<u8>>, SpliceError>;  // byte-preserving
pub fn names_used(streams: &[&[u8]], except: &[SpanAcross]) -> NameSet;    // for PageRes pruning
pub fn text_bag(streams: &[&[u8]], fonts: &FontDecoders) -> TextBag;        // decoded, NFC, word multiset
pub struct PageBlocks { pub marked: Arc<[Marked]>, pub pagination: Arc<[(PaginationKind, PagOrigin, SpanAcross)]>,
                        pub sheaf: Arc<[SheafBlockRef]> /* by stream marker or by marked-content marker */,
                        pub isolation: Arc<[ObjRef]>, pub legacy_ocr: Option<LegacyOcr /* the q + marked 'Q…' pair */>,
                        pub balance: Balance, pub nested_in_forms: bool }
```

**Lexing.**
- **Inline images** follow PDFium's `ReadInlineStream` rules:
  - an exact byte count for unfiltered images (W × H × BPC × components
    / 8);
  - EOD markers for `/AHx` and `/A85`;
  - the EOI for DCT;
  - `/L` or `/Length` where present (PDF 2.0);
  - only then the whitespace-delimited `EI` scan.

  An ignored corpus test compares the lexer's operator boundaries with
  PDFium's page-object enumeration, per page.
- **The logical concatenation.** Lexing works on the concatenation of a
  page's `/Contents`, because a BDC may open in one stream and close in
  the next.
- **Errors.** A stream that won't lex is an error the op reports ("page
  12's content can't be read; nothing was changed"). It is never skipped
  silently.

**Splicing is safe or refused.** A dropped range is accepted only in two
cases:
- its bytes are q/Q- and BT/ET-balanced on their own;
- or it lies wholly inside one BT…ET and contains no text-state or
  positioning operators.

Otherwise it is `EditError::Content`, naming the page. Removing a
running head drawn in the middle of a text object can therefore never
shift the body text. Pixel tests check the pixels outside each removed
artifact's bbox, in slices 6 and 16.

**Classification (`PagOrigin`):**
- **`Sheaf`:** a `/SheafBlock` stream, or a `/SheafGen` property.
- **`SheafLegacy`:** `/SheafStamp true`, `SheafSettings`, `/SheafHF`,
  `/SheafOcrLayer`.
- **`Acrobat`:** the range draws only a Form XObject whose `/PieceInfo`
  has `ADBE_CompoundType` with
  `/Private /Header|/Footer|/Watermark|/Background`.
- **`Authored`:** every other `/Artifact /Pagination`. Word, LibreOffice
  and InDesign mark their running heads this way, and sheaf never
  removes these unasked.

**Removal targets top-level page streams.** Artifacts nested inside
forms are reported through `nested_in_forms` as "left in place", never
skipped silently.

**`ContentWriter`** handles q/Q, cm, colors (gray, RGB, CMYK),
ExtGState, paths, text through a `Font`, images, and `place_form`
(honoring BBox, Matrix and the stamp page's `/Rotate`). It writes BDC
with **named** property lists only, and emits symbolic `ResourceNeeds`
that the emitter names and deduplicates.

**Fonts.**

```rust
trait Font { fn encode(&self, s: &str) -> (Vec<u8>, Unencoded); fn advance(&self, b: &[u8], size: f32) -> f32;
             fn wrap(...); fn covers(&self, c: char) -> bool; fn resource(&self) -> FontRes; }
```

- **Implementations.** `Std14`: AFM widths for all 14, with exact cp1252
  (WinAnsi), Symbol and ZapfDingbats encoders. `Glyphless`: for OCR.
- **What it replaces:** four encoders and three width tables
  (`stamps::win_ansi`/`pdf_literal`, `annot::content_literal`, the
  watermark's UTF-8-into-WinAnsi bug, and the summary's wrap).
- **Embedded fonts.** Any font sheaf embeds is GID-preserving (CID =
  GID).
- **Users.** Mark appearances, header/footer, watermark, OCR text,
  flatten placement, the comment summary and tapes all use it.

**Unencodable text (interim, until M10 slice 7)** is treated the same in
marks and generators (proposal 22). Characters the font can't encode
draw `'?'` in the appearance or block only. `/Contents`, specs and
recipes keep the exact text, and a one-time notice per document says so.
M10 slice 7's Noto Type0 fallback, behind the same trait, ends it.

**Never through lopdf's `Content::decode/encode`.** Per-crate
`clippy.toml` bans land in the slice that removes the last user (Lint
ledgers). `stamps::strip`, `watermark::read` and `has_artifacts`'
decompress-every-page scan are deleted, and the memoized `blocks` fact
replaces them.

**Stays in M11:**
- the semantic interpreter: graphics and text state, glyph boxes,
  foreign font metrics, Type3;
- Form XObject instancing per use;
- clipping;
- removal transforms;
- image editing;
- descriptors.

M11's interpreter consumes `Tok` spans, so nothing here is thrown away.
