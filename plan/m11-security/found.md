# M11: Found while planning

Part of [M11: Security & redaction](README.md).


Bugs and gaps in today's code that matter for M11. File and function
references are as of `f61e7eb`. The slice in brackets fixes each one.

1. **Deleted pages stay in the saved file.** Moved to M9 as bug 14 and
   slice 21 (decision 1).
2. **Encryption is dropped without a word.**
   - Every load removes `/Encrypt` and clears `encryption_state`
     (`file::load_pdf`, `file::load_source`).
   - Incremental saves bail on encrypted files, and the rewrite writes
     plaintext.
   - When the user typed a password, the notice says "password
     protection removed".
   - For a file with only a permissions password, lopdf decrypts on its
     own (`Document::load` handles an empty user password), so there's
     no notice at all. The permissions are lost too.

   [Fixed in M10.5, slices 3 and 7: sources keep their encryption, and
   every save writes it.]
3. **Properties shows no security for those files.** It asks
   `is_encrypted()`, which is false once lopdf has decrypted
   (`properties.rs`). [Slice 1.]
4. **No crash protection for owner-only encrypted files.** Recovery
   is skipped only when a password was typed (`docview.rs`, the autosave
   path). For these files `incremental_section` bails, so every autosave
   just logs a warning. [Fixed in M10.5 by the encrypted sidecar, slice
   10.]
5. **Plaintext copies left on disk:**
   - the generated cache (`generated.rs`): decrypted crops, watermarks
     and stripped copies, never deleted (`m8.md`);
   - the recovered temp file `sheaf-recovered-<pid>-<ms>.pdf`, never
     removed (`docview.rs`);
   - attachments opened from the panel, written to
     `$TMP/sheaf-attachments-<pid>/` and never removed
     (`attachments_panel.rs`).

   [The generated cache and the recovered temp file: fixed in M10.5
   (slices 9 and 19). The attachments folder: slice 1.]
6. **Patterns searches only the primary source** (`pattern_panel.rs`,
   via `index_of_source_page`).
   - Inserted, cropped, watermarked, blank and image pages give no hits
     until saved and reopened.
   - Duplicated pages get hits on the first copy only.

   Pattern-driven redaction would miss them. [Fixed in M10.5 by the text
   service, slice 14, which reads every page as it draws.]
7. **Acrobat's redaction marks are treated as comments**
   (`is_comment_subtype` in `file.rs`).
   - Export Clean Copy deletes them.
   - Flatten burns in their outline and removes nothing.
   - Nothing says the file has unapplied redactions.

   [Slice 4.]
8. **Copies of page text live outside the content stream:**
   - a Replace mark's `struck` text and mark contents in `/SHEF_Annot`
     (`annot.rs`);
   - bookmark titles from "Bookmark matches" (`pattern_panel.rs`);
   - OCR layer streams (`/SheafOcrLayer`);
   - `/ActualText` and `/Alt` in the structure tree.

   A content rewriter alone wouldn't reach them. [Slices 5 and 7, by
   proposals 7 and 8.]
9. **Hidden data in saved files:**
   - watermark settings with the image's local path in content property
     lists (`watermark.rs`, `SheafSettings`);
   - header/footer settings in the catalog;
   - `/Info`, XMP and `/PieceInfo` kept as they were, with `ModDate` and
     `Producer` never updated.

   All are fine in a binder, but sanitizing must remove them. [Slice 9.
   M10.5 no longer writes the watermark settings or the image's local
   path (slice 16), and Clean Copy strips sheaf's private data,
   `private::KEYS` (slice 19).]
10. **Gaps in lopdf 0.45:**
    - strings inside stream dictionaries are neither decrypted nor
      encrypted (issue #603), for example an Indexed palette;
    - an owner password on an R2–R4 file silently gives the wrong key
      (#584);
    - `encrypt()` clears `encryption_state`, so encrypt a clone;
    - the README's "empty passwords only" is stale.

    Work around them in sheaf, and send fixes upstream. [Slice 1. #603
    is closed in M10.5 by its lazy reader and `serialize/crypt.rs`
    (slices 3 and 7); #584 is left for slice 1.]
11. **The incremental writer can't encrypt.** It writes plaintext objects
    and no `/Encrypt` in the trailer (`incremental.rs`, the hand-written
    object writer). [Fixed in M10.5, slices 3 and 7: one serializer
    writes every save, encrypted.]
12. **The legacy `PdfFile::open` / `save_as`** (`file.rs`) uses lopdf
    directly and would write plaintext too. Remove it, or route it
    through the save path. [Slice 1; M10.5 left it untouched
    (file.rs:10).]
13. **The only content rewriter is narrow.** `stamps::strip` removes
    tagged artifacts from page `/Contents`. It doesn't descend into Form
    XObjects or appearances and has no geometry. Crop only sets
    `/CropBox`, so content outside it stays. [Slice 3. M10.5 deleted
    `stamps::strip` (slice 17); its lexical splice in `content/` (slices
    6 and 16) is what slice 3's rewriter builds on. Slice 3 built the
    interpreter, with geometry and every form use, and its writer
    (`interp::encode`); rewriting with them is slices 5 and 6's, and
    content outside the crop box proposal 12's (slice 9).]

Not a disk leak, recorded so nobody looks again: `Document.sources` and
history keep unredacted bytes in memory for the session, for undo. That's
intended. The redacted copy is a separate file, built only from the marks'
output.
