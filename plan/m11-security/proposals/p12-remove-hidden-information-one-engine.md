# M11 proposal 12: Remove Hidden Information… is one engine

Part of [M11: Security & redaction](../README.md) · [all proposals](README.md)

**Remove Hidden Information… is one engine.**
- A dialog of categories that writes a copy. Acrobat's list, plus
  sheaf's own:
  - metadata (Info, XMP, `/PieceInfo`);
  - attachments;
  - bookmarks;
  - comments and markup;
  - form fields (removed or flattened);
  - hidden text that isn't OCR;
  - hidden layers (their content removed through `/OC`);
  - content outside the crop box (redacted as the area outside it);
  - links, actions and JavaScript (`/OpenAction`, `/AA`,
    `/Names /JavaScript`, Launch, URIs);
  - embedded search indexes;
  - thumbnails;
  - sheaf's private data (`SheafSettings`, which holds a watermark
    image's local path; `/SheafHeaderFooter`; `/SHEF_Annot`
    payloads).
- Unreferenced objects always go, by a full rewrite.
- Export Redacted Copy offers it as a checkbox, on by default, with
  the categories remembered in `state.json`.
- Export Clean Copy becomes a preset with its current behavior,
  since PDFlyer users expect it.
- Not in M11: Acrobat's "overlapping objects" (objects hidden behind
  others). It needs occlusion analysis; ❓ for M14.
