# Build or adopt 63: PDF/A and preflight font checks; embedding the standard 14

Area: M14: Document processing · Part of [build or adopt](README.md)

- **Need:** check fonts are embedded, `/Widths` agree with the program's
  advances, and `CIDSet`/`CharSet` are present for PDF/A-1 subsets. With
  `Profile.fonts: EmbedAll`, embed substitutes for every standard 14
  font sheaf writes, ZapfDingbats check boxes among them.
- **Where:** M14 PDF/A and preflight-lite (`ROADMAP.md:456`);
  `m10.5-edit-api.md:3663-3668` ("`Profile.fonts: Std14Ok | EmbedAll` …
  Metric-compatible substitutes (Liberation) sit behind `Font`"). D10.
- **Candidates:** skrifa (advances); Liberation (Helvetica, Times and
  Courier only); **Foxit's PDFium substitutes,** FoxitSymbol.pfb and
  FoxitDingbats.pfb (Type 1, 16.7 KB and 29.5 KB; BSD-3, PDFium
  Authors), which pdf.js ships in
  [external/standard_fonts](https://github.com/mozilla/pdf.js/tree/master/external/standard_fonts)
  with Liberation Sans.
- **Fit:** **Liberation has no Symbol or ZapfDingbats.** The Foxit pair
  is what PDFium, and so sheaf's renderer, draws for them. Embedding a
  Type 1 is fine for PDF/A.
- **Recommendation:** **adopt** Foxit Symbol and Dingbats for EmbedAll;
  **build** the checks on skrifa. Medium.
