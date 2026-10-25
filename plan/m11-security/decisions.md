# M11 decisions

Ruled by Greg while planning and at kickoff. Part of [M11: Security & redaction](README.md).

## Decided while planning (Greg, 2026-10-14)

1. **Deleting has to delete, and that's fixed in M9.** Today a deleted
   page stays in the saved file whenever anything still points at it.
   Inserting one page from another file can bring that whole file along.
   An incremental save keeps everything removed since the last save in
   the earlier revision.
   - Reproduced on `f61e7eb`; the details are bug 14 in
     [`m9.md`](../complete/m09-markup.md).
   - Greg added it to M9 as slice 21, so v0.5 doesn't ship it. Done in
     `109823d`.
   - Redaction builds on it: a redacted page replaces the original, and
     the original must not survive in the file.
2. **Permissions are shown and kept, not enforced.**
   - A file with only a permissions password (bank statements that
     forbid editing or copying) opens as today, and you can still mark
     it up.
   - Properties lists the restrictions, and a one-time notice says the
     file has them.
   - Saves keep them, and Security… can remove them once you give the
     permissions password.
   - ISO 32000 says permissions are advisory, and qpdf and pdf.js don't
     enforce them either.
3. **Certificate encryption (`/Adobe.PubSec`) comes after M12.**
   - PDFium only opens the standard (password) handler, so sheaf would
     have to decrypt these files itself.
   - It also needs certificate stores (PKCS#12, the Windows store,
     PKCS#11), which M12 builds.
   - Until then, sheaf says clearly that a file is certificate-encrypted
     and can't be opened.
4. **Applying redactions writes a copy; the binder keeps its marks.**
   Greg leaned toward exporting, but worried that people would believe
   they'd redacted something and send the original. Exporting comes with
   guard rails aimed at exactly that, and he agreed to them:
   - **Marks never look applied.** An unapplied mark is drawn as a
     hatched outline labeled "Redaction (not applied)", never as a filled
     box. A Preview toggle shows the page as the copy will look, with a
     banner while it's on. (Acrobat also shows outlines until you apply.)
   - **The document says so.** While a document has unapplied marks, a
     bar above the page says "3 redactions not applied", with an Export
     Redacted Copy… button.
   - **Every way out of the binder asks first:** Save As, the exports
     (pages, text, flattened, clean copy), Print, Extract, Split and
     Combine. "This sends the text under 3 redaction marks. Export a
     redacted copy instead?" The default button is the redacted copy.
     It's a gate of the export preflight (M10.5 §12.1), which every
     export goes through, and of Save As's plan.
   - **Applying is a `Transform` on an `ExportTx`** (M10.5 §12, §20):
     only an export takes one, and `Document::commit` takes a `Tx`,
     which can't hold one, so applying in place, by a plugin or
     anyone, is impossible by type.
   - **The copy is unmistakable.**
     - It's named `<name>_Redacted.pdf` by default (Acrobat's suffix).
     - It opens in its own window when written, with "Redacted copy" by
       the title.
     - It has no unapplied marks.
     - It comes with a report (proposal 9).
   - **The file itself warns.** The binder saves its marks as standard
     `/Redact` annotations, so a recipient's Acrobat or sheaf shows them
     as unapplied redactions, not as black boxes.
   - **No apply-in-place in M11,** so there's one model to learn. It can
     be added later, using the same engine.

## Oracles (Greg, 2026-10-23)

The leak check's verification reads each redacted copy with several
readers at once: PDFium, pdf.js's text layer and poppler's `pdftotext`,
plus a byte search of `qpdf --qdf` output. Text one reader's quirks hide
is found by another ([docs/oracles.md](../../docs/oracles.md#by-milestone)).
