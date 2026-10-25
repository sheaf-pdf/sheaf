# M10: Sources

Part of [M10: Forms](README.md).

## Research for this plan

Research for this plan, all on 2026-10-14:
- sheaf's code at `13cfeb4`, with throwaway probes, outside the repo,
  that ran sheaf's own save paths on real forms;
- a corpus of 40 fillable forms (29 IRS, 11 state forms from 9 states),
  scanned for field types, appearances, fonts and scripts, with every
  script parsed and the document-level ones run in boa;
- a wider survey of the scripts in 56 forms (IRS, state and federal
  non-tax), and pdf.js's form-script library run in boa;
- a boa_engine 0.22.0 spike: embedding, speed, limits and hostile
  scripts, natively and as a wasm guest in sheaf's wasmtime;
- a second spike: pdf.js's form-script library inside that guest, with
  its unit tests, the corpus's heaviest forms, time zones, the WASI
  imports and hostile scripts again;
- the prior-art notes (`docs/prior-art/acrobat.md` §1.6 and §2.6,
  `foxit.md` §3.5, `rust-ecosystem.md` on gpui-pdf's `forms.rs`);
- outside sources, listed at the end.


Checked 2026-10-14.
- Greg's corpus, `../pdf-corpus`: `forms/` and `manifest.tsv` (70 IRS
  forms, NY IT-201, CA 540, the I-9, bankruptcy form B 122A-2, VA
  21-526EZ and CA FL-100 at kickoff), and the SafeDocs pool. Tests use
  it from slice 1 (proposal 21).
- The planning corpus, fetched 2026-10-14 into a scratch folder that
  isn't kept (sizes and sha256 prefixes were recorded):
  - IRS, public domain under 17 USC 105:
    `https://www.irs.gov/pub/irs-pdf/<form>.pdf` for f1040, f1040s1,
    f1040s2, f1040s3, f1040sa, f1040sb, f1040sc, f1040sd, f1040se,
    f1040sse, f4562, f8829, f8949, f941, f940, fw4, fw9, f1099nec, fw2,
    f1120s, f1065, f1065sk1, f8879, f2848, f8821, f4868, f1040es, f7004
    and f8962.
  - States, tax year 2025:
    [CA 540](https://www.ftb.ca.gov/forms/2025/2025-540.pdf),
    [CA 3519](https://www.ftb.ca.gov/forms/2025/2025-3519.pdf),
    [NY IT-201](https://www.tax.ny.gov/pdf/current_forms/it/it201_fill_in.pdf),
    [NY IT-2105](https://www.tax.ny.gov/pdf/current_forms/it/it2105_fill_in.pdf),
    [IL-1040](https://tax.illinois.gov/content/dam/soi/en/web/tax/forms/incometax/documents/currentyear/individual/il-1040.pdf),
    [NJ-1040](https://www.nj.gov/treasury/taxation/pdf/current/1040.pdf),
    [CO DR 0104](https://tax.colorado.gov/sites/tax/files/documents/DR0104_2025.pdf),
    [VA 760](https://www.tax.virginia.gov/sites/default/files/taxforms/individual-income-tax/2025/760-2025.pdf),
    [GA 500](https://apps.dor.ga.gov/FillableForms/webpdf/examples/2025GA500.pdf),
    [OH IT 1040](https://dam.assets.ohio.gov/image/upload/tax.ohio.gov/forms/ohio_individual/individual/2025/1040-bundle-original-fi.pdf),
    [PA-40](https://www.pa.gov/content/dam/copapwp-pagov/en/revenue/documents/formsandpublications/formsforindividuals/pit/documents/2025/2025_pa-40.pdf).
  - Not fetched: MI-1040 (404 behind a bot check) and MA Form 1 (403).
  - The script survey also read NY IT-2104, CA DE-4, KS K-40, UT TC-40,
    the NJ, MO, OR, AZ and SC W-4s, and the I-9, I-765, SF-86, SF-424,
    VA 21-526EZ and SS-4; the bug probes used CA 540 for 2024 and NY
    IT-2104.
- boa:
  [releases](https://github.com/boa-dev/boa/releases),
  [v0.22 notes](https://github.com/boa-dev/boa/releases/tag/v0.22),
  [versions on crates.io](https://crates.io/api/v1/crates/boa_engine/versions);
  issues [#3442](https://github.com/boa-dev/boa/issues/3442) (interrupt),
  [#4397](https://github.com/boa-dev/boa/issues/4397) (parser stack
  overflow), [#4515](https://github.com/boa-dev/boa/issues/4515), and
  #4311, #4402, #4398, #5351, #4275;
  [stack overflow issues](https://api.github.com/search/issues?q=repo:boa-dev/boa+%22stack+overflow%22),
  [open panic issues](https://api.github.com/search/issues?q=repo:boa-dev/boa+is:issue+is:open+panic);
  [RustSec](https://rustsec.org/packages/boa_engine.html),
  [security policy](https://github.com/boa-dev/boa/security),
  [fuzz targets](https://github.com/boa-dev/boa/tree/main/tests/fuzz).
- Form JavaScript:
  - [pdf.js](https://github.com/mozilla/pdf.js) `src/scripting_api` at
    `43c5e0d`, with `test/unit/scripting_spec.js` and
    `test/integration/scripting_spec.mjs` (Apache-2.0);
  - [PDFium `fxjs`](https://pdfium.googlesource.com/pdfium/+/HEAD/fxjs)
    at `fe163fd`, and `testing/resources/javascript` (BSD-3);
  - MuPDF `source/pdf/pdf-js.c` (AGPL) and Okular `core/script` (GPL),
    read for their behavior only;
  - Adobe's JavaScript reference, for example
    [Field properties](https://opensource.adobe.com/dc-acrobat-sdk-docs/acrobatsdk/html2015/Acro12_MasterBook/JS_API_AcroJS/Field_properties.htm);
  - ISO 32000-1 §12.6.3 (trigger events) and §12.7 (interactive forms):
    [PDF32000_2008.pdf](https://opensource.adobe.com/dc-acrobat-sdk-docs/pdfstandards/PDF32000_2008.pdf);
    ISO 21757-1:2020 (ECMAScript for PDF).
- Acrobat:
  [forms basics and preferences](https://helpx.adobe.com/acrobat/current/pdf-forms-basics.html),
  [Enterprise Toolkit form preferences](https://www.adobe.com/devnet-docs/acrobatetk/tools/PrefRef/Windows/FormPrefs.html),
  [field properties](https://helpx.adobe.com/acrobat/using/pdf-form-field-properties.html)
  (403 when fetched; search snippets only),
  [copy fields](https://helpx.adobe.com/acrobat/desktop/work-with-pdf-forms/create-forms/copy-fields.html),
  [tab order](https://helpx.adobe.com/acrobat/desktop/work-with-pdf-forms/customize-form-fields/set-navigation.html),
  [collecting form data](https://helpx.adobe.com/acrobat/current/collecting-pdf-form-data.html).
- Acrobat users:
  [auto field detection](https://acrobatusers.com/tutorials/designing-forms-auto-field-detection-adobe-acrobat/),
  [Create Multiple Copies naming](https://pdfautomationstation.substack.com/p/create-multiple-copies),
  [importing text rows](https://community.adobe.com/t5/acrobat-discussions/import-text-file-to-fillable-pdf-form/m-p/14261368/highlight/true),
  [flattening](https://community.adobe.com/t5/acrobat-discussions/how-to-flatten-in-adobe-acrobat-pro-dc/m-p/7986871/highlight/true).
- Foxit
  [forms preferences](https://www.foxit.com/resource-hub/user-guide/foxit-pdf-editor-for-windows/forms/)
  and [Designer Assistant](https://help.foxit.com/csh/index.php/front/index/q/product/phantom/id/Form_Recogntion_DesignerAsistant/version/9.2.0);
  PDF-XChange's [Form tab](https://help.pdf-xchange.com/pdfxe7c/form_ed.html).
- XFA:
  [iText: is it safe to remove XFA](https://kb.itextpdf.com/itext/is-it-safe-to-remove-xfa),
  [Anvil: why XFA forms fill blank](https://www.useanvil.com/ask-anvil/pdfs/why-xfa-pdf-forms-fill-blank/).
- pdftk's [man page](https://www.pdflabs.com/docs/pdftk-man-page/)
  (`need_appearances`, `drop_xfa`).
- Tax practice:
  [IRS: Forms 2848 and 8821 online](https://www.irs.gov/node/89666),
  [Form 8879](https://www.irs.gov/pub/irs-pdf/f8879.pdf),
  [Accounting Today on e-signatures](https://www.accountingtoday.com/news/irs-revises-guidance-on-electronic-signatures),
  [SafeSend organizer FAQ](https://safesendorganizers.zendesk.com/hc/en-us/articles/1500000448301-Organizer-FAQ),
  [an Intuit organizer thread](https://accountants.intuit.com/community/proseries-product-discussions-9/how-does-a-client-complete-a-2020-tax-organizer-to-be-imported-to-1040-78593),
  [Drake brochure](https://www.drakesoftware.com/sharedassets/pdf/2019_brochure.pdf).
