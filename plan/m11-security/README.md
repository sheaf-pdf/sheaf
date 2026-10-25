# M11: Security & redaction

Status: planned · after M10 slices 6–17 and Fill & Sign (2026-10-23)

Proves: Client data can be sent out safely · Depends on: M10 slice 15, for flattening in Remove Hidden Information; M10 slices 23–26 if sanitize removes field scripts rather than whole fields (open)

## Summary

Planned 2026-10-14. It needs M10.5, and M10's flatten (slice 15) for
Remove Hidden Information; see the table.
The plan, with the decisions made while planning, the proposals for
kickoff and the bugs found, is in the files linked below.

- Redaction: mark areas, text or whole pages as standard `/Redact`
  annotations, then export a copy with **true content removal** (text, OCR
  text, images, line art and annotations under the marks), optional fill
  and overlay text, a leak check and verification. Guard rails keep the
  unredacted binder from being sent by mistake.
- Pattern-driven redaction: Patterns panel hits (OCR text included) become
  redaction marks (the shared engine planned since M2), covering whole words
  or chosen characters (Acrobat's partial-word marks).
- Sanitize / remove hidden information: metadata, attachments, comments,
  form fields, hidden layers, content outside the crop box, links, actions
  and JavaScript, search indexes, thumbnails, sheaf's private data,
  unreferenced objects. Export Clean Copy becomes one preset.
- Passwords and permissions: set, change, remove; AES-256. Permissions are
  shown and kept, not enforced.
- Encryption kept on save: done in M10.5 (slices 7, 9 and 10). Every
  save, Append, Rewrite, Save As and recovery sidecar keeps the file's
  encryption; what's left is [slice 1](slices/s01-encrypted-files-what-s-left.md)'s.
- Certificate encryption: moved to after M12 (decided 2026-10-14), which
  builds the certificate stores it needs.

Slices: encryption kept → Security dialog → content interpreter ✅
(landed early, between milestones, 2026-10-23) → redaction marks →
apply: text and OCR → apply: images, line art, annotations → Export
Redacted Copy → search and redact → remove hidden information → plugin
API and docs.

Deleted pages that stayed in saved files, found while planning, are fixed
in M9 (deleting means deleted) rather than here.

**Shared infrastructure:** M10.5 slice 6 built the content lexer and
writer. The content interpreter on them, this plan's slice 3, landed
early, in a PR of its own between milestones (2026-10-23), with a
wrap-up that put the removal check's `text_bag` on it and read fonts'
embedded programs (skrifa) and the predefined CMaps (hayro-cmap): it's
the base for M11's redaction, M13 content editing, M14 optimize and
M17's tagged documents, none of which waits on the rest of M11 for it.
Each may change it as real use finds what it needs.

## Where it stands

Status: **planned** (2026-10-14); slice 3, the content interpreter,
landed early (2026-10-23), in a PR of its own between milestones, after
M10.5 and before M10 resumed, since M13, M14 and M17 build on it too;
its wrap-up (2026-10-24), a second commit in that PR, moved `text_bag`
onto it, made it faster and adopted skrifa and hayro-cmap for font
programs and predefined CMaps. The rest isn't started. It comes after M10, which was skipped at first
and then started 2026-10-15. A few decisions were made while planning
(below); the rest are proposals to settle at kickoff. Proposal 6 (our
own interpreter and rewriter) was taken as recommended with slice 3.
One commit per slice, with notes per slice at the end, as in M7–M9.

M10.5, the edit API ([`m10.5-edit-api.md`](../complete/m10.5-edit-api/README.md)), came
between M10's slices 5 and 6 and took on part of this plan: encryption
kept on every save, and several of the bugs below. Each place says what
it did (M10.5 slice 20 updated this plan, as its "Changes to other
plans" lists).

## Slices

Ordered so that the bug fixes, which don't need the content rewriter,
come first.

| Slice | Title | Status | Depends on |
|---|---|---|---|
| [1](slices/s01-encrypted-files-what-s-left.md) | Encrypted files: what's left | open | M10 slices 6–17, M12 slice 1 (ruling) |
| [2](slices/s02-security.md) | Security… | open | 1 |
| [3](../complete/m11-security/s03-content-interpreter.md) | Content interpreter | done | — |
| [4](slices/s04-redaction-marks.md) | Redaction marks | open | M10 slices 6–17, M12 slice 1 (ruling) |
| [5](slices/s05-apply-text-ocr.md) | Apply: text and OCR | open | 4 |
| [6](slices/s06-apply-images-line-art-annotations.md) | Apply: images, line art, annotations | open | 4, 5 |
| [7](slices/s07-export-redacted-copy.md) | Export Redacted Copy | open | 4, 5, 6 |
| [8](slices/s08-search-redact.md) | Search and redact | open | 4, 7 |
| [9](slices/s09-remove-hidden-information.md) | Remove Hidden Information… | open | 5, 6, 7, M10 slice 15; M10 slices 23–26 if it removes field scripts (open) |
| [10](slices/s10-plugin-api-0-6-docs.md) | Plugin API 0.6, and docs | open | 1–9 (wrap-up), M10 slice 29 (WIT 0.5 first) |

## Plan files

- [Decisions](decisions.md): ruled while planning and at kickoff
- [Proposals](proposals/README.md): one file each
- [Scope](scope.md)
- [Found while planning](found.md)
- [Asks](asks.md)
- [Not in M11](not-m11.md)
- [Sources](sources.md)
- Finished slices, with their notes: [complete/m11-security](../complete/m11-security/)
- [Build or adopt](../research/build-or-adopt/README.md): the M11 rows, and the [changes to make](../research/build-or-adopt/changes-to-make.md) at kickoff
- Open questions: [asks](../asks.md)
