# M10.5: Sources

Part of [M10.5: the edit API](README.md).


Checked 2026-10-15.
- sheaf at `a0955fc`, read by the ten survey readers (model and history;
  save and materialize; content features; forms; the app and CLI;
  plugins and the agent; rendering; future milestones, from ROADMAP.md,
  [`m11.md`](../../m11-security/README.md), the README feature tables and `docs/prior-art/`;
  PDF prior art; editor prior art), and the M10 worker's notes after
  slices 3–5.
- Greg's corpus, `../pdf-corpus` (its `forms/` and `manifest.tsv`, the
  322 `/DocMDP` files, and the SafeDocs pool, of which 402 files of 4 MB
  or more were measured).
- Measurements: PDFium through ctypes, without sheaf's lock, on an AMD
  EPYC 7302P, on a synthetic 7,000-page file (3.5 MB) and the 402 pool
  files. Slice 2 commits the scripts under `bench/` (`gen.py`,
  `bench.py`, `real.py`, `warm.py`, `form.py`).
- PDF libraries (for their behavior; MuPDF, iText and pdfSweep are AGPL,
  read only):
  - [MuPDF](https://github.com/ArtifexSoftware/mupdf):
    `include/mupdf/pdf/interpret.h`, `page.h`, `document.h`, `object.h`,
    `clean.h`, `form.h`, `annot.h`; `source/pdf/pdf-clean.c`,
    `pdf-op-filter.c`, `pdf-xref.c`; PyMuPDF's
    [Page](https://pymupdf.readthedocs.io/en/latest/page.html),
    [journalling](https://pymupdf.readthedocs.io/en/latest/recipes-journalling.html)
    and [low-level
    interfaces](https://pymupdf.readthedocs.io/en/latest/recipes-low-level-interfaces.html);
    Ghostscript [bug
    708052](https://bugs.ghostscript.com/show_bug.cgi?id=708052).
  - [PDFium](https://pdfium.googlesource.com/pdfium):
    `public/fpdf_edit.h`, `public/fpdf_save.h`, `public/fpdf_annot.h`,
    `core/fpdfapi/edit/cpdf_pagecontentgenerator.cpp`,
    `cpdf_pagecontentmanager.cpp`, `cpdf_creator.cpp`.
  - [pdf.js](https://github.com/mozilla/pdf.js): `src/core/worker.js`,
    `document.js`, `writer.js`, `annotation.js`, `editor/pdf_editor.js`,
    `src/display/annotation_storage.js`, and
    [`editor/tools.js`](https://raw.githubusercontent.com/mozilla/pdf.js/master/src/display/editor/tools.js).
  - [iText](https://github.com/itext/itext-java) (`PdfObject`,
    [`PdfPage`](https://api.itextpdf.com/iText/java/9.5.0/com/itextpdf/kernel/pdf/PdfPage.html),
    [`PdfCanvas`](https://api.itextpdf.com/iText/java/9.5.0/com/itextpdf/kernel/pdf/canvas/PdfCanvas.html))
    and [pdfSweep](https://github.com/itext/i7j-pdfsweep) ([removing
    content](https://kb.itextpdf.com/itext/removing-content-with-pdfsweep)).
  - [PDFBox](https://github.com/apache/pdfbox): `PDPageContentStream`,
    `PDDocument`, `COSUpdateInfo`, `COSUpdateState`, `COSIncrement`,
    `ContentStreamWriter`, `PDFStreamParser`, `TextPosition`.
  - [qpdf](https://github.com/qpdf/qpdf) (`QPDFPageObjectHelper`,
    `QPDF`, `QPDFObjectHandle`, `QPDFWriter`, `QPDFPageDocumentHelper`,
    `QPDFAcroFormDocumentHelper`) and
    [pikepdf](https://github.com/pikepdf/pikepdf) (`_qpdf.pyi`,
    `_page.pyi`, `_parsers.pyi`, `_content_scopes.py`, `_page_copy.py`;
    [content
    streams](https://pikepdf.readthedocs.io/en/latest/topics/content_streams.html)).
  - [pdf-writer](https://docs.rs/pdf-writer/latest/pdf_writer/)
    ([`Chunk`](https://docs.rs/pdf-writer/latest/pdf_writer/struct.Chunk.html)),
    [krilla](https://github.com/LaurenzV/krilla),
    [hayro](https://github.com/LaurenzV/hayro) (`hayro-interpret`,
    `hayro-write`).
  - pdf_oxide:
    [redaction](https://pdf.oxide.fyi/docs/editing/redaction), releases
    [0.3.50](https://newreleases.io/project/github/yfedoseev/pdf_oxide/release/v0.3.50)
    and
    [0.3.76](https://newreleases.io/project/github/yfedoseev/pdf_oxide/release/v0.3.76),
    [`redaction/serialize.rs`](https://docs.rs/crate/pdf_oxide/0.3.77/source/src/redaction/serialize.rs).
  - [pdfium-render](https://docs.rs/pdfium-render/latest/pdfium_render/)
    ([`PdfPageContentRegenerationStrategy`](https://docs.rs/pdfium-render/latest/pdfium_render/prelude/enum.PdfPageContentRegenerationStrategy.html)).
  - lopdf, vendored at `39492cb` plus sheaf's xref patch; issues
    [#603](https://github.com/J-F-Liu/lopdf/issues/603) and
    [#584](https://github.com/J-F-Liu/lopdf/issues/584).
  - Apryse
    [`ElementWriter.WriteMode`](https://docs.apryse.com/api/PDFTronSDK/dotnet/pdftron.PDF.ElementWriter.WriteMode.html),
    and Foxit's [redaction
    guide](https://developers.foxit.com/documents/development-guide/pdf-sdk-ios/features/annotation/redact/).
- The standard and signatures:
  - ISO 32000-1 (Annex C limits; 7.5.7 object streams; 12.7.3.1 merged
    fields and widgets; Table 254) and ISO 32000-2 (Table 257, DocMDP):
    [PDF32000_2008.pdf](https://opensource.adobe.com/dc-acrobat-sdk-docs/pdfstandards/PDF32000_2008.pdf);
  - ABCpdf on DocMDP [transform
    parameters](https://www.websupergoo.com/helppdfnet/source/9-abcpdf.elements/12-interactive/1479-docmdptransformparameterselement/default.htm)
    and
    [`/P`](https://websupergoo.com/helppdfnet/source/9-abcpdf.elements/12-interactive/1479-docmdptransformparameterselement/2-properties/01-entryp.htm),
    and on [redaction
    annotations](https://www.websupergoo.com/helppdfnet/source/9-abcpdf.elements/12-interactive/1421-redactionannotationelement/default.htm);
  - shadow and incremental-saving attacks:
    [pdf-insecurity.org](https://pdf-insecurity.org/signature/isa.html),
    [NDSS
    2021](https://www.ndss-symposium.org/wp-content/uploads/ndss2021_1B-4_24117_paper.pdf),
    [iText on detecting
    them](https://pdfa.org/detecting-pdf-shadow-attacks-with-itext-7/);
  - LibPDF on [incremental
    saves](https://libpdf.documenso.com/docs/concepts/incremental-saves);
  - glyph positions leaking redacted text: [PETS
    2023](https://petsymposium.org/popets/2023/popets-2023-0069.php).
- Editor architectures:
  - ProseMirror: [transforms
    (guide)](https://prosemirror.net/docs/guide/#transform),
    [reference](https://prosemirror.net/docs/ref/#transform),
    [collaborative
    editing](https://marijnhaverbeke.nl/blog/collaborative-editing.html);
  - Slate:
    [operations](https://docs.slatejs.org/concepts/05-operations),
    [`operation.ts`](https://raw.githubusercontent.com/ianstormtaylor/slate/main/packages/slate/src/interfaces/operation.ts),
    [normalizing](https://docs.slatejs.org/concepts/11-normalizing),
    [PathRef](https://docs.slatejs.org/api/locations/path-ref);
  - Lexical: [editor
    state](https://lexical.dev/docs/concepts/editor-state),
    [transforms](https://lexical.dev/docs/concepts/transforms),
    [history](https://raw.githubusercontent.com/facebook/lexical/main/packages/lexical-history/src/index.ts);
  - tldraw: [store](https://tldraw.dev/sdk-features/store),
    [editor](https://tldraw.dev/docs/editor);
  - Figma:
    [multiplayer](https://www.figma.com/blog/how-figmas-multiplayer-technology-works/),
    [ordered
    sequences](https://www.figma.com/blog/realtime-editing-of-ordered-sequences/),
    [plugin
    typings](https://raw.githubusercontent.com/figma/plugin-typings/master/plugin-api.d.ts),
    [accessing the
    document](https://developers.figma.com/docs/plugins/accessing-document/),
    [InstanceNode](https://developers.figma.com/docs/plugins/api/InstanceNode/);
  - Xi:
    [retrospective](https://raphlinus.github.io/xi/2020/06/27/xi-retrospective.html),
    [issue
    1187](https://github.com/xi-editor/xi-editor/issues/1187#issuecomment-491473599);
  - Yjs: [undo manager](https://docs.yjs.dev/api/undo-manager),
    [relative positions](https://docs.yjs.dev/api/relative-positions);
    Automerge:
    [storage](https://automerge.org/docs/reference/under-the-hood/storage/),
    [binary
    format](https://automerge.org/automerge-binary-format-spec/);
  - Excalidraw:
    [history](https://raw.githubusercontent.com/excalidraw/excalidraw/master/packages/excalidraw/history.ts),
    [element
    types](https://raw.githubusercontent.com/excalidraw/excalidraw/master/packages/element/src/types.ts),
    [blobs](https://raw.githubusercontent.com/excalidraw/excalidraw/master/packages/excalidraw/data/blob.ts);
  - Photoshop's [Smart
    Objects](https://photographylife.com/photoshop-smart-objects-explained)
    and [History
    panel](https://helpx.adobe.com/nz/photoshop/desktop/get-started/set-up-toolbars-panels/history-panel-overview.html);
    Affinity's
    [saving](https://affinity.help/photo/en-US.lproj/pages/GetStarted/save.html);
  - Git's
    [objects](https://git-scm.com/book/en/v2/Git-Internals-Git-Objects)
    and the [LFS
    spec](https://github.com/git-lfs/git-lfs/blob/main/docs/spec.md);
  - CAD: OCAF's [user
    guide](https://occt3d.com/dev/doc/overview/html/occt_user_guides__ocaf.html)
    and
    [`TDocStd_Document`](https://occt3d.com/dev/doc/refman/html/class_t_doc_std___document.html);
    FreeCAD's [topological naming
    problem](https://wiki.freecad.org/Topological_naming_problem/en),
    [its fix in
    1.0](https://hackster.io/news/freecad-hits-1-0-after-two-decades-finally-sees-an-end-to-the-toponaming-problem-7e7ac2df3e63)
    and
    [`Transactions.h`](https://raw.githubusercontent.com/FreeCAD/FreeCAD/main/src/App/Transactions.h);
    Onshape's
    [collaboration](https://www.onshape.com/en/blog/under-the-hood-how-collaboration-works);
  - CodeMirror: [guide](https://codemirror.net/docs/guide/),
    [`change.ts`](https://raw.githubusercontent.com/codemirror/state/main/src/change.ts),
    [`history.ts`](https://raw.githubusercontent.com/codemirror/commands/main/src/history.ts);
    VS Code's
    [`vscode.d.ts`](https://raw.githubusercontent.com/microsoft/vscode/main/src/vscode-dts/vscode.d.ts);
    the [LSP 3.17
    specification](https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/).
