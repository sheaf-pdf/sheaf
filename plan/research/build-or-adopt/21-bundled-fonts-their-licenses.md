# Build or adopt 21: The bundled fonts and their licenses

Area: M10: Forms · Part of [build or adopt](README.md)

- **Need:** ship Noto Sans (the embedded fallback) and Liberation Sans,
  Serif and Mono (the editor; later metric-compatible substitutes), with
  correct notices.
- **Where:** M10 slices 7 and 11 (`m10.md:1469`, `:1597`); M10.5's M14
  notes (`m10.5-edit-api.md:3667`).
- **Today's plan:** "Noto Sans, under the SIL OFL (kickoff)"
  (`m10.md:603`); "The Liberation fonts' license in `packaging/licenses`"
  (`:1597`).
- **Facts:**
  - **Noto Sans:** latest NotoSans-v2.015, 2024-11-20
    ([repo](https://github.com/notofonts/latin-greek-cyrillic)). Its
    OFL.txt names no Reserved Font Name. Latin, Greek and Cyrillic only;
    Arabic, Hebrew, Indic and CJK are other families, which matches the
    plan's refusal. Unhinted Regular is 431,364 bytes (hinted 621,572).
  - **Liberation 2.1.5** (2021-09-30;
    [repo](https://github.com/liberationfonts/liberation-fonts)): OFL
    1.1, with Reserved Font Names "Liberation" and Google's "Arimo,
    Tinos and Cousine". 1.x was GPL with a font exception, so take 2.x.
    Last pushed 2022-12, fine for finished fonts.
  - **[OFL-FAQ](https://openfontlicense.org/ofl-faq/):** 1.12, embedding
    in documents is allowed "either in full or a subset", and the
    modification restrictions "do not apply"; 1.20, bundling in an app
    needs the copyright, license notice and license text. So subsetting
    either family into a PDF is fine. Ship Liberation unmodified in the
    app (don't subset or rename the bundled copy), or the RFNs apply.
- **Recommendation:** **adopt** both, unmodified, with `LICENSE-noto`
  and `LICENSE-liberation` in `packaging/licenses` (F18). Ship Noto
  unhinted. Bundle only the faces the editor rule uses. High.
