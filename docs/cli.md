# Command line

Some work runs without a window. `sheaf <subcommand> …` runs it headless,
and so does `sheaf-cli <subcommand> …`, a separate binary with no GUI at
all, for servers and CI. `--help` on either lists the options. A progress
bar shows on stderr when it's a terminal, and a line per step when it
isn't (`-q` hides both); Ctrl-C stops the work cleanly.

A subcommand that writes a PDF never decides for you what the file loses:
when its output would break a signature or drop a form's XFA values, it
stops before writing anything, exits 3 and prints the question as JSON on
stdout. `--signatures ask|break|append-anyway` and `--xfa ask|drop`
answer it (`ask` is the default); `-o out.pdf --append` copies the input
and appends the changes, which keeps a signed file's signatures.
`--force` only ever means "replace an output that's there". Exit status:
0 done, 1 failed, 2 bad arguments, 3 a decision is needed, 130 cancelled.

```sh
sheaf edit binder.pdf --edits tidy.json -o tidy.pdf
sheaf edit signed.pdf --edits note.json -o noted.pdf --append
sheaf inspect binder.pdf --json
sheaf export binder.pdf -o pngs/ --pages "1-3, 10-" --dpi 200
sheaf export binder.pdf -f tiff --tiff bw -o binder.tif
sheaf-cli export binder.pdf -f text -o binder.txt
sheaf scan photos/*.jpg -o receipts.pdf --mode bw --ocr
sheaf remove-blanks scans.pdf -o clean.pdf --rejects removed.pdf
sheaf comments summary binder.pdf -l pages --summary stamps
sheaf comments csv binder.pdf -c number,page,author,text --author "Pat Smith" > pat.csv
```

| Subcommand | Does |
|---|---|
| `edit` | Edits from a JSON file (`--edits FILE`, or `-` for stdin) applied as one undoable version and saved: `-o out.pdf` (written whole), `-o out.pdf --append`, or `--in-place`. The edits are sheaf's ops (`--schema` prints their JSON schema) and name things as you see them: pages by position (`{"pos": 3}`), bookmarks by their titles (`{"path": ["Part I", "Ch. 2"]}`), comments by `/NM` (`{"nm": "…"}`), and files by `--with NAME=PATH` (`{"with": "NAME"}`). Prints what it did as JSON; `--dry-run` says what it would do and writes nothing. Examples in [docs/document-model.md](document-model.md) |
| `inspect` | `--json`: a PDF's pages (labels, boxes, rotation), sheaf's marks and other tools' comments by `/NM`, bookmarks by title path, headers, footers and watermarks, and a form summary: what `edit` needs to name things |
| `form` | `check`: a PDF's form as the file has it, for what other viewers would trip on or a save would carry where it shouldn't. Prints its kind (AcroForm, hybrid, XFA-only) and how many fields, widgets and pages it has, its XFA, NeedAppearances, Reader usage rights, certification and signatures, then each problem: widgets no field reaches, on no page, on more than one or with the wrong `/P`, kids whose `/Parent` is elsewhere, fonts a `/DA` names that `/DR` lacks, full names held twice, values with no appearance, values only an earlier revision holds, an XFA copy that differs from the fields, and signatures or `/SigFlags` that no longer fit the file; `--password` for an encrypted one |
| `combine` | PDFs and images into one PDF in order; `file:range` takes some pages (`a.pdf:1-3,7`). A bookmark per file (`--no-file-bookmarks` to skip), `--image-page` |
| `export` | Pages to PNG, JPEG, TIFF or text, as File > Export Pages. Prints the files written; refuses to overwrite without `--force` |
| `split` | One PDF into several: `--every N`, `--bookmarks` (a file per top-level bookmark), `--max-size 10MB`, or `--ranges "1-3; 4-10; 11-"` |
| `remove-blanks` | Blank, and "intentionally left blank", pages out of a PDF, as Document > Find Blank Pages…. Prints each page found and whether it goes; `--dry-run` writes nothing; `--rejects` keeps the removed pages in a file of their own, bookmarked "p. 4 — blank". Nearly blank pages (a signature) stay unless `--nearly-blank`; `--sensitivity strict\|normal\|lenient`, `--keep-intentional`, `--pages` |
| `scan` | Photos of paper, or a scanner's or copier's pages, to a clean PDF, a page each (a TIFF's pages each a page). Photos (`--profile photo`, the default): finds the page in the photo and straightens it, turns it upright, levels the text, evens out shadows and color casts. A scanner's pages (`--profile scanner`): turned upright and levelled, the whole image kept and its light left as it is. `--mode color\|gray\|bw`, `--page letter\|a4\|original`, `--dpi`, `--ocr` (searchable text that lines up with the page), `--crop`/`--no-crop`, `--flatten`/`--no-flatten`, `--no-orient` and `--no-deskew` to change a step, `--drop-blanks` to leave out blank pages, `--flatten-curl` for curled or bent pages (UVDoc model; `scripts/fetch-ocr-data.sh --uvdoc`) |
| `images-to-pdf` | JPEG, PNG, TIFF (every page), BMP, GIF and WebP files to one PDF, a page each: `--page original\|letter\|a4`, `--margin`, `--dpi` |
| `comments` | A PDF's comments, as File > Comments: `summary` (a PDF; `-l pages` puts each page beside its numbered list), `csv` (`-c` picks the columns), `text`, or `export` (XFDF or FDF, by `-o`'s extension or `--format`), to stdout unless `-o`. `import file.pdf data.xfdf…` brings comments from FDF, XFDF or other PDFs into a copy (`-o`; default "<name> - imported.pdf"), updating those it has by name. Filter and sort as the Comments panel does: `--pages 3-10`, `--type`, `--author`, `--status` (each repeats), `--checked`/`--unchecked`, `--summary stamps\|notes`, `--search`, `--sort page\|author\|date\|type\|color` |
