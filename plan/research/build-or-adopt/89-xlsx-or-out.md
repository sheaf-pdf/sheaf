# Build or adopt 89: XLSX in or out

Area: Pending · Part of [build or adopt](README.md)

- **Need:** none in the plans; only CSV that's "Excel-safe"
  (`m10.md:1014-1017`). Excel export appears only in prior-art notes
  (`docs/prior-art/acrobat.md:331`: "PDF-to-Excel table fidelity is the
  key competitive surface").
- **Candidates, for when it comes:**
  [rust_xlsxwriter](https://crates.io/crates/rust_xlsxwriter) 0.101.0
  (2026-10-20; MIT OR Apache-2.0; the author of Python's XlsxWriter; 129
  releases); [calamine](https://crates.io/crates/calamine) 0.37.0
  (2026-10-18; MIT; reads xlsx, xls and ods).
- **Recommendation:** **undecided.** A plan that exports tables
  (pdfplumber-core) or fills forms from a workbook in M15 decides it;
  then rust_xlsxwriter for output, calamine for input. High that nothing
  is needed now.
