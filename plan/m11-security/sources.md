# M11: Sources

Part of [M11: Security & redaction](README.md).

## Research for this plan

Research for this plan, all on 2026-10-14:
- sheaf's code at `f61e7eb`;
- the prior-art notes (`docs/prior-art/acrobat.md` §2.9–2.10,
  `foxit.md` §3.6, `pdflyer.md` §2.8);
- outside sources, listed at the end.


Checked 2026-10-14.
- lopdf 0.45:
  [`Document`](https://docs.rs/lopdf/latest/lopdf/struct.Document.html),
  [`EncryptionVersion`](https://docs.rs/lopdf/latest/lopdf/encryption/enum.EncryptionVersion.html),
  [encrypt example](https://github.com/J-F-Liu/lopdf/blob/master/examples/encrypt.rs),
  issues [#603](https://github.com/J-F-Liu/lopdf/issues/603),
  [#584](https://github.com/J-F-Liu/lopdf/issues/584),
  [#479](https://github.com/J-F-Liu/lopdf/issues/479).
- PDFium [`fpdf_edit.h`](https://pdfium.googlesource.com/pdfium/+/HEAD/public/fpdf_edit.h),
  [`cpdf_pagecontentgenerator.cpp`](https://pdfium.googlesource.com/pdfium/+/HEAD/core/fpdfapi/edit/cpdf_pagecontentgenerator.cpp),
  [`cpdf_parser.cpp`](https://pdfium.googlesource.com/pdfium/+/HEAD/core/fpdfapi/parser/cpdf_parser.cpp)
  (Standard handler only).
- How others remove content (for their behavior only; MuPDF and iText
  are AGPL):
  [MuPDF `page.h`](https://github.com/ArtifexSoftware/mupdf/blob/master/include/mupdf/pdf/page.h)
  and [`pdf-op-filter.c`](https://github.com/ArtifexSoftware/mupdf/blob/master/source/pdf/pdf-op-filter.c);
  [iText pdfSweep](https://github.com/itext/i7j-pdfsweep);
  [pdf_oxide text pruning](https://docs.rs/crate/pdf_oxide/0.3.76/source/src/redaction/text_prune.rs).
- ISO 32000-1 §12.5.6.23 (Redact annotations) and Table 22 (permissions):
  [PDF32000_2008.pdf](https://opensource.adobe.com/dc-acrobat-sdk-docs/pdfstandards/PDF32000_2008.pdf);
  PDF 2.0 deprecations from the
  [Arlington model](https://github.com/pdf-association/arlington-pdf-model/blob/master/tsv/latest/EncryptionStandard.tsv).
- Acrobat:
  [redactable data](https://helpx.adobe.com/acrobat/desktop/protect-documents/redact-pdfs/redactable-data.html),
  [redaction properties](https://helpx.adobe.com/acrobat/desktop/protect-documents/redact-pdfs/text-redaction-properties.html).
- Glyph positions leaking removed text:
  [PETS 2023](https://petsymposium.org/popets/2023/popets-2023-0069.php).
- Incremental updates keeping the original:
  [Computerworld, 2008](https://www2.computerworld.com.au/article/print/215187/recovering_pdf_redaction_/).
- Tax practice:
  [IRS Pub 4557](https://www.irs.gov/pub/irs-pdf/p4557.pdf),
  [16 CFR 314.4](https://www.law.cornell.edu/cfr/text/16/314.4),
  [26 CFR 301.6109-4](https://www.law.cornell.edu/cfr/text/26/301.6109-4).
- Acrobat's partial-word marks:
  [Adobe: Redact text](https://helpx.adobe.com/acrobat/desktop/protect-documents/redact-pdfs/redact-text.html),
  [a walkthrough of the settings](https://chrismenardtraining.com/post/redact-a-word-or-phrase-in-a-pdf-file-in-adobe-acrobat-pro-dc/).
