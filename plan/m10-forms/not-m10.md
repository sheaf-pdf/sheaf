# M10: Not in M10

Part of [M10: Forms](README.md).


- **XFA forms:** showing or filling dynamic XFA, and FormCalc, stay not
  planned. Updating a hybrid form's XFA datasets instead of removing
  them: Pending.
- **Signing, certifying, verifying, and Fill & Sign:** M12. M10 places
  unsigned signature fields, protects signed ones, and asks before
  rewriting a signed file.
- **Encryption kept on save:** done in M10.5, so M10 doesn't warn.
- **Removing scripts when sanitizing, and redacting fields:** M11.
- **Batch filling and flattening, and a filled copy per CSV row:** M15,
  on top of `sheaf form`.
- **Auto-complete:** Pending. Remembered entries are client data and
  would cross clients. If built: text only, off by default, in its own
  file in the state folder, with a Clear command.
- **Installed fonts** for characters no bundled font covers (found with
  `fontdb`, embedded where the font allows): later. Until then they're
  refused at commit.
- **Right-to-left text and complex shaping in fields:** later. A notice
  says so.
- **Spell check, rich-text fields, image fields, page templates
  (`spawn`):** later. **Barcode fields:** not planned.
- **A JavaScript console and debugger, and a document JavaScript
  editor** (Acrobat's Ctrl+J and Shift+D): later.
- **Open, page and document-event scripts** (`/OpenAction`, page open
  and close, WillSave, DidSave, WillPrint, DidPrint): logged, not run.
  Later, if forms turn out to need them.
- **Submitting forms, emailing form data, form tracking:** not planned.
  A Submit button offers to save XFDF instead.
- **General document JavaScript and folder-level scripts:** not planned
  (ROADMAP.md, Not planned).
- **Acrobat's XML form data:** later.
- **Detecting fields on open, or with a layout model** (oar-ocr): later.
- **Unicode in stamps, headers and footers:** no longer out: slice 7's
  fallback ends M10.5's `?` interim in marks and generators (M10.5
  proposal 22). **The comment summary's:** Pending (M9 decision 11); it
  draws through the same `Font` trait, so the fallback reaches it with
  little more.
- **Following links in the page view** stays in Pending. Buttons' Go To
  and URI actions are in M10.
