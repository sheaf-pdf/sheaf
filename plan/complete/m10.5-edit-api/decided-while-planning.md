# M10.5: Decided while planning (Greg, 2026-10-15)

Part of [M10.5: the edit API](README.md).


1. **M10.5 designs and builds a clean edit API** for M11 onward:
   redaction, sanitize, encryption, signatures, in-place content
   editing, optimize, PDF/A, accessibility and batch. It's built from
   this plan and sliced like M9 and M10: one commit per slice, each with
   its checks.
2. **M10 pauses after slice 5 and resumes on the new API.** M10's PR
   merges to `main`, M10.5 is built from `main`, and M10 then resumes
   with slices 6+ on the new API (M10 slices 6+ on the new API, below;
   when exactly is proposal 11).
3. **Clean and correct beats compatibility,** since sheaf is pre-alpha.
   - No shims. The `Edit` enum, the WIT edit variant (sheaf stops
     serving 0.2 and 0.3), the plugin-api `Batch` and sheaf's private
     file data may all be redesigned.
   - No migration code for sheaf's own older formats unless it's nearly
     free.
   - Fixed: PDF interoperability (Acrobat, PDFium, pdf.js, and Acrobat
     round trips), the snapshot undo guarantees, and speed at about
     7,000 pages.
4. **Standard PDF keys first.** Private data only where PDF has no place
   for it.
5. **The generated-file cache is retired for derived sources.** Page
   content becomes part of the model.
6. **Plugins and the agent get semantic edits only, at first.** The API
   must be able to expand later.
7. **Existing features are re-expressed on the new API, and the old
   paths deleted,** within the milestone.
