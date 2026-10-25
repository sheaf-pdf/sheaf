# arlington-pdf-model

The PDF Association's Arlington PDF Model: the grammar of PDF's objects
(ISO 32000-2 with its errata), as TSV, a table per object.

- From: https://github.com/pdf-association/arlington-pdf-model at d148fee5a0cb6dcacadf58c09ed842e32847024d (2026-10-21, "Resolve #180")
- Fetched: 2026-10-24, from https://codeload.github.com/pdf-association/arlington-pdf-model/tar.gz/d148fee5a0cb6dcacadf58c09ed842e32847024d
- Taken: `LICENSE`, `NOTICE.txt`, `tsv/latest/*.tsv` (613 files, 493,082 bytes): one grammar for every PDF version, whose SinceVersion and DeprecatedIn columns and version predicates say which version each key, type and value belongs to
- Left out: `tsv/1.0` to `tsv/2.0` (the same grammar cut per version), `TestGrammar`, `scripts`, `gcxml`, `xml`, `Jupyter`, `3dvisualize`, `resources`, the docs (`README.md`, `INTERNAL_GRAMMAR.md` and the rest) and the spreadsheets
- License: Apache-2.0, as `LICENSE` and `NOTICE.txt` say (`CITATION.cff` too). `NOTICE.txt` puts the repository's other documentation under CC-BY-4.0: none of it is here
- Used by: tests only, through `sheaf_doc::arlington` (behind the `fixtures` feature, which only dev-dependencies turn on). Never shipped

The files are as the tarball has them, byte for byte (`.gitattributes`
keeps every checkout from changing their line ends).

To update: fetch a newer commit the same way, into a scratch directory;
replace `LICENSE`, `NOTICE.txt` and `tsv/latest` with its own; update the
commit here and in `sheaf_doc::arlington::COMMIT`; rerun
`just test -p sheaf-doc arlington::`, `just test -p sheaf-ops --test arlington`
and `just tier-arlington`; and record in the PR what moved.
