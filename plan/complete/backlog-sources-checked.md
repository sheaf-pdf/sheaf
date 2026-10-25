# Backlog: sources checked

Part of [the backlog](../backlog/README.md).

Outside material already compared against sheaf, so the comparison isn't
done twice.

- **2026-10-21: Gemini's summary of a PDFlyer demo video**
  ([share link](https://share.gemini.google/5pTedPNwgmKO)). It covers
  the calculator, sticky notes, status stamps, connectors and Note
  Summary. It produced B1 (closed) and F1–F3. F4 and F5 came out of
  discussing it.
  - **Already in sheaf from M9:**
    - placing, moving and re-editing tapes, and tape titles;
    - sticky notes;
    - Set Status and Reset Status, with Reset keeping the status stamp;
    - connector groups;
    - Note Summary and Stamp Summary as Comments presets, whose rows
      jump to the mark and select it.
  - **Secondhand, and partly at odds with CCH's help:**
    - In the report, Note Summary includes stamps. In CCH's help it
      covers notes and tapes, and stamps have their own Stamp Summary.
    - In the report, clicking a connector jumps to its target. In CCH's
      help, connectors are label groups navigated from the right-click
      menu.

    Prefer `docs/prior-art/pdflyer.md`, which cites CCH's help.
- **2026-10-22: OCR on pages that already have text.** Adobe's
  [renderable text](https://helpx.adobe.com/acrobat/kb/error-could-perform-recognition-acrobat.html)
  note, ocrmypdf's
  [advanced docs](https://ocrmypdf.readthedocs.io/en/stable/advanced.html)
  (skip, redo, force) and ABBYY's
  [incorrect text layer](https://support.abbyy.com/hc/en-us/articles/360020818320-Recognition-of-the-document-that-has-incorrect-text-layer)
  article. Foxit's documentation didn't say. It produced F10.
- **2026-10-23: QR codes to links.** Neither Acrobat nor Foxit appears
  to do it. Foxit
  [turns URLs in text into links](https://resource-cms.foxit.com/tutorials/how-add-hyperlinks-pdfs-seamless-navigation-and-interactivity),
  and an Adobe forum
  [question asking for this](https://community.adobe.com/t5/acrobat/make-clickable-hyperlinks-from-qr-or-zappar-codes-in-pdf-how/m-p/9981646)
  has no answer. It produced F16.
- **2026-10-23: an appearance with states and no `/AS`.** pdf.js's
  `src/core/annotation.js` (6.4.299) and PDFium, on hand-made files, and
  Acrobat's documentation, which doesn't say. It's in B12's notes.
- **2026-10-23: the build-or-adopt audit** ([build-or-adopt.md](../research/build-or-adopt/README.md)): crates.io, docs.rs
  and GitHub for every need in the milestones and the backlog, by four
  researchers. It produced B24–B29 and F18–F23, and lines on B5, F9 and
  F13.
