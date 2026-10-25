# M10 slice 5: Forms survive page tools III: XFA, usage rights and signatures

Status: done · Part of [M10: Forms](../../m10-forms/README.md)

**Forms survive page tools III: XFA, usage rights and signatures**
(proposals 6–7; bugs 6–7).
- A hybrid form's XFA datasets read and compared with its `/V` values
  by binding, with the bar when they hold values the AcroForm lacks;
  the `<speak>` and `<toolTip>` text read for tooltips.
- `/XFA` goes when a save changes the field tree, with its tooltips
  copied into `/TU`, and only after asking when its datasets hold
  values.
- `/Perms /UR3` goes on any full rewrite, with `/SigFlags` where no
  signature field needs it; DocMDP stays.
- A save that would rewrite a signed or certified file asks first,
  which covers M9's removals today. Incoming signed fields are kept
  with the "no longer valid" notice; flattening them comes with
  slice 15.
- **Checks:** bugs 6–7's reproductions, by both strategies; a hybrid
  fixture whose datasets hold a value its `/V` lacks shows the bar and
  asks before dropping; a page delete on a signed fixture (built in
  code) asks, and Append leaves its signed bytes untouched; deleting a
  1040 page keeps the other page's tooltips in `/TU`.

## Notes

Done. A save takes out what described the form as it was, and asks
before a rewrite of a signed file.
- **`forms::xfa`** reads a static XFA copy: each field (or exclusion
  group) of the template by the AcroForm full name Designer gives it,
  the value its datasets bind to it, and its tooltip (`<toolTip>`, or
  else `<speak>`). Binding is XFA's normal one by name and order:
  `bind match="none"` is transparent, `global` binds by name anywhere,
  and `dataRef`s of `$.…`/`$record.…` by path. `Lookup` finds a field
  by name, or by its path without indexes when that's unique.
- **`FormFacts::xfa_values`**: fields whose XFA value the AcroForm lacks
  (text it doesn't have, a box checked there and not here; a check
  box's `0` and whitespace aren't values). Where a field has no `/TU`,
  `Field::tooltip` takes the XFA's.
- **The bar** for a hybrid form with such values: "This form's XFA part
  holds values sheaf doesn't show; Adobe Acrobat or Reader shows them.
  Saving a change to the form removes that part." A save that would
  remove it asks first ("Remove the form's XFA part?", Save and Remove
  It or Cancel, Cancel the default); the answer holds for the window's
  session.
- **`Document::form_changed`** (and `Snapshot::form_changed`): a field
  added, removed or changed, or a page's widgets not as the saved
  version has them (gone, or another file's page now). An extract or
  print subset counts the pages left out.
- **`forms::rules::take_out`**, at the end of every rewrite: Reader's
  usage rights (`/Perms /UR3`, `/UR`) go, `/Perms` with them when empty,
  and `/SigFlags` when no signed field or certification needs it; a
  certification stays. When the form changed, `/XFA` goes too, its
  tooltips copied into `/TU` first where a field has none. The save's
  notes say so.
- **The incremental writer's modes:** a save refuses a changed form
  that has an XFA copy or usage rights (so it's a rewrite: the earlier
  revision would keep them); an append (below) and a recovery sidecar
  take them out in their update.
- **Signed files** (`save::Signed`): `save_to_path_noting` takes `Ask`,
  `Rewrite` or `Append`. Asked, a save that would rewrite a file with a
  signed field or a certification fails with `WouldBreakSignatures`
  (how many, and whether it's encrypted), writing nothing. The app then
  asks: "Rewrite (breaks 1 signature)", "Append (earlier versions stay
  in the file)" (the default; not offered for an encrypted file until
  M11) or Cancel, and saves as chosen. `save_to_path`, for headless
  use, rewrites. An append writes an update even for what a full save
  takes out, which then stays in the earlier revision, as the prompt
  says.
- **Signed fields brought in** with pages are kept, and the notice
  says "The signature from signed.pdf is no longer valid here"
  (`EditOutcome::signatures_in`; Combine's notice too).

Deviations from the plan:
- **Values are compared only where the AcroForm's is empty or off.**
  Two different values (the XFA's raw, the AcroForm's formatted) would
  flag every formatted amount.
- **Only the app asks** before dropping XFA values; a headless rewrite
  drops them (the save's notes say so).
- **Exports, Extract Pages, Split and print follow the rewrite rules**
  (usage rights out; the XFA copy out when the form changed) but don't
  ask about signatures: they write new files and leave the signed one
  as it was.
- Not fixed: a rewrite still drops an invisible signature whose widget
  is on no page (slice 2's 9 Aspose files), now only after the user
  chose Rewrite. An append to a file whose cross-references lopdf
  rebuilt fails, and says why ("Save failed: …").

Found here:
- **How Designer indexes AcroForm names,** learned from the corpus's 71
  hybrid government forms, which now all bind field for field: a
  same-named `<draw>` takes an index (the 2848's
  `RetentionRevocation[1]` follows a draw); an unnamed object is
  indexed by its kind among all its siblings of that kind (the
  4506-C's `#subform[9]` is page 1's tenth subform); an unnamed subform
  is transparent to names, so what's in it is numbered in its parent
  (the 990-EZ's `#subform[2].help[2]`); a page set isn't (the 4868's
  master page `Page1` doesn't take the body's index); and a dot in a
  name is `\.`. A blank 1040's datasets hold 62 values, all its check
  boxes' `0`; the 1120 has one `\r\n`.
- The tests' corpus fills were a full lopdf save, which broke the
  input's usage rights; they're appended now, as Reader saves a fill.

Checked by tests:
- `forms::xfa`: binding by name and order, `none`, `dataRef`, a draw
  and an unnamed subform taking indexes, transparency to names, dots,
  entities (`&amp;`), whitespace; what isn't XML reads as nothing;
- `read::tests::corpus_hybrid_forms_bind_their_xfa` (skips without the
  corpus): all 71 hybrid government forms bind every field, hold no
  XFA-only value, and more than half their fields get tooltips;
- `sheaf-ops/tests/forms_saves.rs`: a hybrid fixture with a page
  deleted loses `/XFA` by both writers, its tooltip in `/TU`; turned,
  it keeps it; with a form inserted, the incremental save refuses and
  the save in place is a rewrite without it, with the note; usage
  rights go with a rewrite and stay with an update, `/SigFlags` with
  them; an XFA value the AcroForm lacks is counted, and not once it's
  there; a signed fixture with a page deleted: asked, nothing written,
  then rewritten (signature broken, the value gone) or appended (the
  original's bytes intact, the signature fits, 1 page); an unsigned one
  isn't asked; a certified one is; signed fields brought in are kept,
  with the notice;
- on the corpus: the 1040 with page 2 deleted has no `/XFA`, `/Perms`
  or `/SigFlags`, the check clean, and nine in ten of its fields have
  `/TU` (`f1_01`'s starts "Page 1."); the W-9's page 2 extracted has no
  form and nothing of its XFA or usage rights (bug 6's case);
- slices 3 and 4's corpus tests now check every finding: combining the
  1040 and schedules, the 1040 with itself, a duplicated 1040 page,
  crops and deletes all come out clean;
- `form_bar::tests::xfa_values_the_acroform_lacks_get_the_bar`;
- each rule taken out in turn (the XFA drop, the rights drop, asking)
  fails some of these.

Checked on Xwayland:
- a signed fixture with a page deleted and saved: "“signed.pdf” is
  signed", Rewrite (breaks 1 signature), Append (the default) or
  Cancel, the file untouched; Append saved it incrementally, the
  original's bytes intact and the signature still fitting;
- a hybrid fixture holding an XFA-only value: the bar at open; a page
  deleted and saved: "Remove the form's XFA part?", the file untouched.
