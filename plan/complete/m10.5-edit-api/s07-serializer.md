# M10.5 slice 7: The serializer

Status: done · Part of [M10.5: the edit API](README.md)

**The serializer** (L; proposals 23 and 33; bugs 2, 4 and 5).
- `serialize/{object, xref, append, full, crypt, durable}`:
  - f64 reals, with non-finite values refused, and clamping;
  - one-subsection full xrefs with a free list, and appended runs
    with generation + 1 on deletes;
  - offsets relative to the header offset;
  - per-object encryption, stream-dict strings and `/EFF` included,
    with keyed-hash IVs;
  - `/Encrypt` and `/ID` kept;
  - unique temp names, preserved permissions, fsync of the file and
    the directory;
  - a minimal `FileStamp { len, mtime, tail hash }`, captured at open
    and after each save on the primary's registration
    (`SourceInfo`), and its re-check before the rename or append.
    A mismatch returns `SaveStop::Ask(Ask::FileChanged)` through the
    old entry point, as an error the app downcasts the way it does
    `WouldBreakSignatures` today, and the app asks Save As…,
    Overwrite or Cancel (proposal 33). Slice 10 adds the startxref
    window's hash and `content_hash`.
- The old writers (`finish`, and incremental.rs's section) write
  through it, loads stop stripping `/Encrypt`, and encrypted files
  save encrypted by both old strategies.
- **Deletes:**
  - lopdf's `save_to`/`compress` in `finish`;
  - incremental.rs's `write_object`/`write_dict`/`write_name`/`runs`
    (moved);
  - the `/Encrypt` strip in `load_pdf`/`load_pdf_mem`;
  - `write_atomic`;
  - the "encrypted: only Rewrite" restriction (docview.rs:1635,
    `WouldBreakSignatures.encrypted`);
  - `save_to_path_noting`'s length-only disk check and the Append
    path's "changed on disk" bail, which the stamp check replaces
    (moved here from slice 9; slice notes).
- **Checks:**
  - RC4-128, AES-128 and AES-256 files save encrypted by both
    strategies and reopen in PDFium with their password, keeping `/P`
    and `/ID[0]`.
  - An AES-256 xref-stream base whose strings live in object streams
    rewrites, and PDFium reads its `/T`, `/V` and outline titles.
  - A file with a 100-byte prefix appends and opens identically in
    PDFium, pdf.js and qpdf (ignored).
  - Reals round-trip at f64, and NaN is refused.
  - A file modified by another program between the save starting and
    the rename gives `FileChanged`, and nothing is overwritten.
  - Concurrent saves get distinct temp names.
  - Permissions are preserved.
  - `qpdf --check` passes on the outputs (ignored).

## Notes

Done. sheaf-doc writes PDF itself (`serialize/`), and both old
writers go through it before records arrive: a Rewrite (`finish`:
saves, Save As, extract, Clean Copy, Export Flattened, Combine, print)
is `serialize::full`, and incremental.rs's update and the recovery
sidecar are `serialize::append`. Reals are f64, a non-finite one
refused; a whole file has one cross-reference subsection with its
free entries linked, an update has runs, positions count from the
header and `/ID[0]` is kept. An encrypted file saves encrypted by both
strategies, each object encrypted on its own with the source's key
and keyed-hash IVs, its `/Encrypt` (and so `/P`) and `/ID[0]` kept;
exports keep the encryption and print drops it. Files are replaced
durably (a unique temp name, permissions kept, the file and its
directory fsynced), and a save in place checks a minimal
`FileStamp` before writing and again before the rename: a file
another program changed stops the save with
`SaveStop::Ask(Ask::FileChanged)`, and the app asks Save As…,
Overwrite or Cancel. A signed encrypted file can now append
(proposals 10, 23 and 33; bugs 2, 4 and 5).
- **The object writer** (`serialize/object.rs`): `write(out, obj)`, a
  direct object, and `indirect(out, id, obj) -> usize`, `n g obj` to
  `endobj` and where it starts; either leaves nothing in `out` when
  it fails, and `indirect`'s error names the object. Reals are
  written at f64 in their shortest round-trip digits, always with a
  point and never an exponent: NaN and the infinities are refused
  (`serialize::Error::NonFinite { id }`), magnitudes past 3.4e38 are
  clamped and nonzero ones under 1.175e-38 written 0 (Annex C), and
  -0 is `0.0`. Strings keep their format (literals escaped as lopdf
  escapes them), names escape `#`, white-space, delimiters and bytes
  outside `!`–`~`, and a stream's `/Length` is set to its data's.
  content/writer.rs's private `write_name` was the same escaping and
  now calls `object::name`, so there's one name writer.
- **Cross-references** (`serialize/xref.rs`): `Entry::{InUse { pos,
  generation }, Free { generation }}` and `Entry::deleted(g)`, a free
  entry at the next generation (capped at 65,535). `Section::full(size)`
  is one subsection `[0, Size)`, every number not listed free at
  generation 0, the free entries linked from entry 0 (generation
  65,535); `Section::append(size, header)` writes runs of the numbers
  listed, entry 0 only when it frees something. `/Size` grows past any
  number inserted. `table(out, trailer, pos)` and `stream(out, num,
  trailer, pos)` write the section, its trailer and `startxref`, every
  offset as position − header; the stream lists itself, with `/Index`
  pairs and `/W [1 n 2]`, n as narrow as the largest field allows,
  uncompressed. An offset longer than a table's ten digits is refused
  (`Error::TooFar`) rather than written as a broken table.
- **A whole file** (`serialize/full.rs`, `save::finish`):
  `full::write(objects, base, kind, version, crypt) ->
  anyhow::Result<Vec<u8>>` writes `%PDF-<version>`, a fixed
  binary-mark line, every object in ascending id order and one
  `Section::full` in the base's kind; object streams, cross-reference
  streams and linearization dictionaries are left out, so a source's
  object-stream members come out as objects of their own. The trailer
  is the base's without its sections' keys (`/Prev`, `/XRefStm`, an
  xref stream's dictionary keys, `/Size`), `/Encrypt` and `/ID`, which
  it writes anew; with a `Crypt` the stored `/Encrypt` dictionary is
  the object after the last, written in the clear, and the xref stream
  the one after that. `finish(pdf, crypt)` keeps lopdf's prune and
  renumber and calls it; lopdf's `compress` and `save_to` are gone from
  it, so a stream the save makes is compressed where it's made
  (`annot::Appearance::write` and `flatten_annots`' page content now
  do; every other producer already did) and a source's streams are
  written as stored, never re-encoded (§11.4).
- **`/ID`** (`serialize::file_id`, shared): the first string kept
  (the `Crypt`'s, else the base's), the second 16 bytes of xxh3-128 of
  everything written before the section, so the same objects give the
  same bytes; an encrypted file with no `/ID` (only AES-256 opens
  without one) gets the hash twice, a plain one gets none, as before.
  The first string is read by `serialize::first_id(trailer)` as
  lopdf's key derivation reads it: the array's first element when
  it's a string, however many the array has.
- **Appends** (`serialize/append.rs`, incremental.rs): `Base { len,
  ends_line, header, prev, size, kind, first_id, root, info }`, from
  the Source by `Base::of(&Source)` (`XrefFacts`' header offset, start
  and kind; `first_id` by `serialize::first_id`; `size` the greater of
  `/Size` and the largest cross-reference stream number + 1, refused
  past the reader's cap, "no object number left for an update";
  `/Root` and `/Info` from the newest trailer as stored). `append::section(base, objects, deleted,
  xref_number, crypt)` is empty with nothing to write or free;
  otherwise it ends the base's last line if it's open, writes each
  object at base length + its place, frees each deleted id at
  generation + 1, and writes a trailer with `/Root`, `/Prev` at the
  real start, `/Info`, `/Encrypt` (the base's reference) and the new
  `/ID`, then a table or a stream numbered `xref_number`. incremental.rs
  loads the primary through `file::load_source`, with its password,
  where it took `Pdf::load_mem` and refused any encrypted file; asks
  `base_sound()` where it asked `xref_start == 0`; raises the eager
  document's `max_id` to `size − 1` before numbering anything; and
  `build` returns only the section (`incremental_section` no longer
  copies the original). The encrypt-then-write step is
  `serialize::indirect`, shared with full.rs. A file with bytes before
  `%PDF-` appends with its positions counted from the header (bug 5).
  Updates are smaller: literal strings stay literal (the old writer
  wrote every string hex), reals are full precision, dictionaries
  compact and `/W` narrower; not measured. A sidecar of a file whose
  cross-references were rebuilt still has `/Prev` 0, as before.
- **Encryption** (`serialize/crypt.rs`, `Source::crypt()`): `Crypt {
  state, dict, at, first_id, ivs }` holds the `EncryptionState` the
  password opened, the `/Encrypt` dictionary as stored (read through
  `LazyPdf`, which never decrypts it), its number, the trailer's
  `/ID[0]` and an HMAC-SHA256 state keyed once with the file key.
  `Crypt::of(&LazyPdf) -> lopdf::Result<Option<Crypt>>`, `state()`,
  `dict()`, `at() -> ObjectId`, `first_id() -> Option<&[u8]>` and
  `encrypt(id, &obj)`, which encrypts a
  clone under `id` through lopdf's walk: every string, stream
  dictionaries' included, every stream, an embedded file by `/EFF`'s
  filter and a stream naming its own `/Crypt` by that, but not an
  encryption dictionary, a cross-reference stream, the metadata when
  `/EncryptMetadata` is false or a signature's `/Contents`. An AES IV
  is the first 16 bytes of HMAC(file key, num ‖ gen ‖ payload), num
  and gen big-endian. Encrypted strings keep their format.
  `Source::open` builds it (a failure to read the dictionary fails the
  open, `OpenError::Pdf`) and `Source::crypt()` gives it. Deps: `hmac`
  0.13 and `sha2` 0.11.
- **The vendor patch** (vendor/lopdf/SHEAF.md):
  `CryptFilter::encrypt_with_iv(key, plaintext, iv: &mut dyn
  FnMut(&[u8]) -> [u8; 16])`, which only the AES filters override
  (their `encrypt` goes through it with a shared `random_iv`);
  `encryption::encrypt_object_with(state, id, obj, iv)`, the same walk
  as `encrypt_object` with the IVs given (`Direction::Encrypt { iv }`;
  `encrypt_object` and `Document::encrypt` keep random IVs); and
  `LazyPdf::encryption() -> Option<(&EncryptionState, ObjectId)>`,
  its `crypt` and `encrypt` fields one `Option<(EncryptionState,
  ObjectId)>`, and an open that refuses a direct `/Encrypt` up front
  (`InvalidPassword`, what the security handler's failure gave).
- **Loads keep encryption** (file.rs): `load_pdf` and `load_pdf_mem`
  no longer remove `/Encrypt` from the trailer or set
  `encryption_state = None`. lopdf's eager reader still takes the
  dictionary out of the document it loads, so the writers take it from
  `Source::crypt()`; the docs of `load_pdf` and `load_source` say so.
- **Which outputs keep it** (`save::kept_encryption`, proposal 10's
  recommended answer): saves, Save As, `rewrite`, `rewrite_to_path`,
  extract and split, Clean Copy (so Combine, blanks and scan) and
  Export Flattened pass the primary's `Crypt` to `finish`;
  `print_pdf` passes None (§12.3), so print, page images and the
  comment summary are plain. Encryption follows the primary: pages
  from an encrypted file inserted into a plain one come out plain, and
  a plain file combined into an encrypted one comes out encrypted with
  the primary's password. Sidecars of encrypted files opened with an
  empty user password are now encrypted sections; password-protected
  documents still skip autosave (`self.password.is_some()`, slice 10).
- **Durable writes** (`serialize/durable.rs`, bug 2): `write(path,
  bytes, expect: Option<&FileStamp>) -> Result<(), WriteError>`
  (`Changed { path }`, `Io { path, source }`) follows symlinks to the
  real file, creates `.<name>.<16 hex>.sheaf-tmp` beside it with
  create-new (retrying on AlreadyExists, 16 times; the name cut at a
  character boundary so the whole fits `NAME_MAX`, 255), writes,
  copies the target's permissions (Unix), `sync_all`s, checks
  `expect` (a mismatch removes the temp and is `Changed`), renames and
  fsyncs the directory (Unix; EINVAL and ENOTSUP, from filesystems
  that can't, are ignored, and any other failure is a `log::warn!`,
  not a failed write, since the file is replaced by then). Any failure
  removes the temp file. Saves, Save As, Clean Copy, extract, Export
  Flattened, `rewrite_to_path`, recovery's sidecar and `materialize`,
  and sheaf-ops' image PDFs, scans and stamp collections
  (`stamps::write`) write through it; `save::write_atomic` and
  `images::write_atomic` are gone.
- **The file stamp** (`durable::FileStamp { len, mtime, tail }`, the
  tail xxh3-128 of the last 64 KiB): `FileStamp::of(meta, bytes)`
  stamps what an open read, `read(path)` reads the tail by Seek, and
  `holds(path)` says whether the file is still the stamped one (a
  missing file isn't). `SourceInfo.stamp: Option<FileStamp>`, taken
  by `SourceInfo::read` from the same open handle through the new
  `sheaf_core::read_file_and_metadata` (`read_file` calls it); None
  for blank, memory and derived sources. The app's re-read after a
  save stamps the saved file.
- **SaveStop and the old entry point** (save.rs): `SaveStop { Ask(Ask)
  }` and `Ask { FileChanged { path: Arc<Path> } }`, thiserror types
  beside `WouldBreakSignatures` ("<path> changed on disk since sheaf
  read it"), and `IfChanged { Ask, Overwrite }`, the new fourth
  argument of `save_to_path_noting` (`save_to_path` passes `Ask`). A
  save to the primary's own file with `Ask` and a stamp checks it up
  front, returning the SaveStop with nothing written, and writes
  through `write_checked(path, bytes, expect)`, which passes it to
  `durable::write` and makes its `Changed` the same SaveStop; an
  incremental save that hits it doesn't fall back to a Rewrite.
  `same_file` holds only when both paths resolve, and the exports'
  same-file refusals use it. The length-only `unchanged` check and the
  Append path's "changed on disk" bail are gone. `sheaf-cli comments`,
  which saves in place, stops with the FileChanged message.
  `WouldBreakSignatures.encrypted` is gone, and `signatures_broken`
  asks only about signatures.
- **The app** (docview.rs): the saved file is re-read with the
  document's password (`SourceInfo::read` and
  `RenderHandle::open_bytes`) and `view.password` is kept; the "—
  password protection removed" notice is gone (proposal 23).
  `ask_how_to_save_signed` always offers Rewrite (breaks n
  signatures), Append and Cancel; its encrypted branch and "until M11"
  are gone. `save_signed(path, signed, if_changed, cx)`; the Err arm
  downcasts `SaveStop::Ask(Ask::FileChanged { .. })` into
  `pending_changed: Option<(PathBuf, Signed)>`, and `render` calls
  `ask_file_changed`: a warning, "“name” changed on disk", "Another
  program changed it since sheaf read it.", Save As… (the default),
  Overwrite (`save_signed` again with `IfChanged::Overwrite`) and
  Cancel. `pending_signed` carries the `IfChanged` too. Save As… starts
  a fresh save (`Signed::Ask`, `IfChanged::Ask`), so a signed document
  is asked about its signatures again for the new path.
- **Fixtures** (`sheaf_doc::serialize::fixture`, cfg(test) or the
  `fixtures` feature): `OWNER`, `USER`, `rc4_aes_128_and_aes_256`,
  `attachment_only` and `encrypted`, moved from `source::lazy_tests`
  so other crates can use them; `form()` (a page of "Page 1", the
  text field `applicant` = "Ann Lee", the bookmark "Chapter One", an
  attachment, an `/ID`) with `FIELD`, `VALUE`, `BOOKMARK`, `ATTACHED`
  and `FORM_SECRETS`; `written(pdf)` (lopdf's writer, table and
  stream); `encrypt_dict(state)` (AES-256's with `/Length 256`); and
  `object_streams_encrypted(pdf, state)`: every non-stream object
  plain in one `/ObjStm` encrypted whole, streams encrypted under
  their own numbers, the `/Encrypt` dictionary in the clear and a
  hand-written xref stream with type-2 rows, the shape lopdf's
  `save_modern` gets wrong. From the review: `one_id(pdf)` (its `/ID`
  cut to one string), and the tests' shared `PREFIX`, `holds` (moved
  from `source::lazy_tests`) and `edited(path, password)` (a turn and
  a note, which sheaf-render's and sheaf-ops' tests each had).
- **The gate harness** (`gate_7k.rs`): the doc says the writer rows
  write through `serialize/` from slice 7 (crop's copy through lopdf's
  until 15) and the incremental save and the flatten replace their
  file durably; "incremental save, one mark, AES-256" and "Rewrite,
  AES-256" on synthetic and heavy, no baseline or gate, on
  `<kind>-7000-aes256.pdf`, made by lopdf (`Document::encrypt` with
  the shared AES-256 state and `encrypt_dict`, then its writer, an
  `/ID` added when the fixture has none) when missing or older than
  the fixture, opened with `USER` and asserted encrypted; the work
  copy is put back with the time it was made, synced
  (`work_copy`), so the next save finds the file it read and its
  fsync doesn't write out the harness's copy.
- **Ledgers:** the retired ledger has 7 rows for slice 7
  (`incremental::{write_object, write_dict, write_name, runs}`,
  `save::write_atomic`, `images::write_atomic`,
  `save::WouldBreakSignatures::encrypted`), and `QUALIFIED_ONLY`
  gains `runs` (six unrelated `runs` calls in annot.rs and the summary)
  and `encrypted` (the `encrypted(..)` fixture and a counter field,
  16 hits). The transitional ledger has rows for `SaveStop`, `Ask`
  and `IfChanged` on the old entry point, `SourceInfo.stamp`,
  `kept_encryption` and `finish`'s `Option<&Crypt>`, IVs keyed by the
  written number, `Base::of(&Source)`, and the whole-file in-place
  write. No lint ban lands.
- **Docs:** `docs/document-model.md`'s Saving has the serializer (a
  section "One writer (M10.5 slice 7)": objects and reals,
  cross-references and the header offset, whole files, updates,
  `/ID`, encryption kept per object with keyed IVs and `/EFF`, which
  outputs keep it, durable writes) and "A file another program
  changed"; both strategies' paragraphs say they write through it and
  that an encrypted file appends; "Both strategies use lopdf for
  object serialization" is gone, and the Core idea's `SourceInfo` has
  its stamp. sheaf-doc's crate doc, save.rs's and incremental.rs's
  module docs, vendor/lopdf/SHEAF.md (the IV seam, `encryption()`)
  and the README (Security: saves, Save As and exports stay encrypted,
  printing doesn't; the save model: a changed file asks) are current.

Deviations from the plan:
- **Line numbers had moved since a0955fc;** the work followed the
  symbols (incremental.rs's framing at :452–529 and writers at
  :554–648, `write_atomic` at save.rs:924, docview's encrypted branch
  at ~:1731 and the notice at ~:1858, file.rs's strip at :492–521).
- **IVs are keyed by the written `(num, gen)`, not ObjKey,** which
  doesn't exist until slice 9: HMAC-SHA256(file key, num ‖ gen ‖
  plaintext). Still deterministic, and no IV serves two plaintexts;
  `Crypt::iv`'s doc says so.
- **The IV seam needed a vendor patch:** lopdf's AES filters drew a
  random IV with no way in. `encrypt_with_iv` takes an IV source, not
  a computed IV, so RC4 and Identity never hash a stream for an IV
  they don't use (an eager IV would have about doubled RC4 saves'
  encryption cost).
- **`Crypt` API shapes:** `Crypt::of` returns `lopdf::Result<Option<
  Crypt>>` (failing to read `/Encrypt` must not drop the encryption)
  and `encrypt` a `lopdf::Result<Object>` (`DecryptionError` is
  neither Clone nor PartialEq, and `serialize::Error` is Copy); there's
  no `p()` (`/P` is in `dict()`), and `/ID[0]` is `first_id()`. The
  plan's `id0()` began as `id() -> Option<&[Vec<u8>; 2]>`, from
  `XrefFacts`' `/ID`, which is None unless the array is exactly two
  strings; and `at()` as an `Option`, None for a direct `/Encrypt`,
  which no encrypted file that opens has. The review changed both
  (below).
- **The `/Encrypt` strip was mostly lopdf's own:** its eager reader
  removes `/Encrypt` from the trailer and drops the dictionary on
  load. Deleting sheaf's two lines (the redundant trailer remove and
  `encryption_state = None`) doesn't give the writers the dictionary
  as stored, so they take a `Crypt` from the Source (§3.1's
  `Source::crypt()`, with the `LazyPdf` accessor slice 3 left for 7).
- **Removing lopdf's `compress()` from `finish`** stopped it
  Flate-encoding every unfiltered stream, sources' too. The two new
  streams that relied on it compress themselves
  (`annot::Appearance::write`, `flatten_annots`' page content); every
  other `Stream::new` on a writer path already did. A Rewrite of an
  uncompressed source no longer shrinks it (Measured).
- **`SaveStop` has only `Ask`, and `Ask` only `FileChanged`:** slice 7
  produces no `SaveStop::Failed`, `Ask::Signatures` or
  `Ask::XfaHoldsValues`. They're thiserror types in save.rs, deriving
  Clone and PartialEq so the app and tests compare the downcast;
  slice 9 moves them to `materialize/` and adds the rest. The app's
  Err arm matches `FileChanged` explicitly, so later variants fall
  through to "Save failed" until they're handled.
- **`IfChanged` is new:** proposal 33's Overwrite needs the old entry
  point to skip the stamp check, and the plan names no parameter.
  Overwrite skips both checks; with a signed document and Append
  chosen it appends to the bytes sheaf read. `pending_signed` carries
  it, or choosing Overwrite and then answering the signatures question
  would ask FileChanged again, in a loop.
- **The length-only check and the Append path's "changed on disk"
  bail went here, not in slice 9** (§17): the stamp check subsumes
  them, and after it they'd only be false when the reader chose
  Overwrite.
- **`same_file` holds only when both paths resolve:** it compared
  `canonicalize(a).ok() == canonicalize(b).ok()`, true when both fail
  (a deleted primary and a new Save As path), which would have asked
  FileChanged wrongly.
- **No `Progress` in the serializer:** §11.1's `serialize` takes
  `&Progress`, and slice 6's note says the serializer needs
  `was_cancelled`, but no caller of the old entry points has a
  tracker. Progress and cancel arrive with `materialize` (slice 9).
- **No `Profile` yet:** `finish` takes `Option<&Crypt>` through
  `kept_encryption`. Saves, Save As, extract and split, Clean Copy,
  Export Flattened and Combine keep the encryption (proposal 10, open
  for Greg, takes its recommended answer per the standing
  instruction); print passes None (§12.3).
- **The app had to re-read with the password:** it read the saved
  file back with `SourceInfo::read(.., None)` and
  `RenderHandle::open_bytes(.., None)` and dropped `view.password` with
  the notice. Once saves keep encryption, a password-protected
  document would fail to reopen after saving; the plan didn't mention
  it, and it landed with the Rewrite step.
- **sheaf-ops' `images::write_atomic` went too** (a fixed `.part`
  name and no fsync, the same defect, called by images.rs and
  scan.rs): through `durable::write`, with a retired row.
- **Symlinks, past the plan:** `write_atomic` replaced a symlinked
  target with a regular file. `durable::write` follows `read_link` up
  to 40 levels, as Linux does, rather than `canonicalize`, so a link
  whose target doesn't exist yet gets its target created and stays a
  link. `Changed { path }` names the path the caller gave.
- **Windows:** CI runs windows-latest. The tail is read by Seek, and
  rename-over works with std; permissions are copied and the
  directory fsynced on Unix only (Windows' one permission, read-only,
  would still fail the rename and leave a temp `remove_file` can't
  delete). A Windows (msvc) `cargo check -p sheaf-doc --lib --tests`
  passed in step 5; the permission and symlink tests are cfg(unix).
- **Smaller choices:** `Entry`'s field is `generation` (`gen` is
  reserved in edition 2024); the header offset lives on the `Section`
  (`Section::append(size, header)`), and `table`/`stream` take only
  the position the section starts at; positions are `usize`, as
  `XrefFacts` has them, u64 only in the rows; `full::write` takes the
  `BTreeMap` lopdf holds and returns `anyhow::Result`; the trailer also
  drops `/F`, `/FFilter`, `/FDecodeParms` and `/DL`, the rest of a
  stream dictionary's keys (ISO 32000-1 Table 5); the binary mark is a
  fixed line, not lopdf's per-document one; `Base` has `ends_line` (the
  old writer checked only LF) and is built by `Base::of(&Source)`;
  `append::section` returns nothing to write as an empty Vec (§11.7's
  empty sections, at the serializer); `FileStamp::holds` is shared by
  both checks; `WriteError::Io` names the file; the message shows the
  full path, as the other save errors do; Save As… is the prompt's
  default button and its title uses the document's own name.
- **Tests, past the plan:** the serializer tests write their files
  with the serializer itself, so every read-back checks the table it
  wrote; `an_encrypted_object_stream_base_rewrites` (RC4, AES-128 and
  AES-256, lazy and eager reads) beside the PDFium one; the serialize
  tests' `turned()` opens as the app does (`open::start` and `ready`),
  since `Document::new(.., &[])` left the outline empty and every
  rewrite dropped the bookmarks. AES-256 fixtures carry `/Length 256`
  (`encrypt_dict`), as Acrobat writes them: lopdf's V5 encode leaves it
  out (M11), and qpdf warns on the base and on every output, since
  saves keep `/Encrypt` as stored; `encrypted()` now encrypts once and
  writes both kinds from that document.
- **The PDFium checks read text through `page_chars`,** not
  `Renderer::page_text`, which loses text on turned pages (Found); the
  edit is a turn and `forms::fixture::note()`.
- **qpdf was installed** (12.3.2, /usr/bin/qpdf), contrary to the
  planner's note, so the ignored qpdf checks ran and pass. node and
  pdfjs-dist 6.4.299 arrived before the commit, and the pdf.js half of
  `a_prefixed_append_opens_identically_in_pdfjs_and_qpdf` then ran and
  passed: both xref kinds read "pages 1", page 1's text "Page 1" and
  2 annotations, identically before and after the append, in pdf.js
  and qpdf (`qpdf --check`: 28 outputs, 0 fail). The tools are
  `$SHEAF_QPDF` or PATH, and node with `$SHEAF_PDFJS` (pdfjs-dist's
  legacy/build/pdf.mjs); a missing one prints why and passes. The
  interop bases go past the list: every encryption with a
  table and a stream, plain object streams (`save_modern`), both
  prefixed files and an Append onto the AES-256 object-stream fixture,
  28 outputs.
- **The corpus check is its own test file** (`encrypted_corpus.rs`),
  and an Append declined on damaged cross-references counts as
  declined, as for a plain file.
- **Recovery:** sidecars of encrypted files are encrypted once the
  incremental writer encrypts; the app's autosave skip for
  password-protected documents stays until slice 10's encrypted
  sidecar.
- **The gate harness syncs the copy it puts back:** fsync latency on
  this machine swings widely (Measured), and the harness's own copy of
  the fixture was left dirty for the next save's fsync.
- **From the review,** each with a test that fails without it:
  - **`/ID[0]` is read as the key is made from it:** lopdf makes
    RC4's and AES-128's keys from the `/ID` array's first string,
    whatever the array's length, but the writers kept `/ID[0]` only
    when the array was exactly two strings (`XrefFacts::id`). A file
    whose `/ID` was one string saved with a new `/ID[0]`, its key no
    longer matching its `/O`, `/U` and objects: a Rewrite in place
    replaced it with a file nobody could decrypt, and an Append left
    its newest trailer so. `Crypt::of`, `Base::of` and `full::write`
    read it through `serialize::first_id`, and such a file gets a
    second string. `rewrites_stay_encrypted` and
    `encrypted_files_append_encrypted` also run on
    `one_id(plaintext())`. No backstop refuses a V<5 write without
    `/ID[0]`: the open makes the key from it and fails without one, so
    no such `Crypt` exists.
  - **`Crypt::at()` is an `ObjectId`:** `append::section`'s branch
    for a direct `/Encrypt` couldn't be reached. The vendored open
    refuses a direct dictionary itself, with the error it gave anyway,
    so `LazyPdf::encryption()` always has the number.
  - **Encrypted strings keep their format:** writing each one hex
    doubled its bytes for nothing, since `object::string` escapes a
    literal of any bytes so it reads back
    (`strings_names_and_streams_round_trip`). The vendor walk no
    longer marks them (`Direction::Encrypt { iv }`), so its patch is
    smaller. Measured below.
  - **An update needs numbers left:** `Base::of` added 1 to the
    largest cross-reference stream number unchecked, so a hostile file
    whose stream is numbered 4,294,967,295 (LazyPdf takes it) panicked
    an incremental save or a sidecar in debug builds and wrapped to 0
    in release. A `size` past the reader's cap (`max_objects`, Annex
    C's 8,388,607) is now "no object number left for an update", so a
    save in place rewrites, which renumbers; below the cap no update
    runs out of numbers. `Section::insert` saturates. Test:
    `a_base_numbered_to_the_last_number_isnt_appended_to`.
  - **A directory that can't be synced isn't a failed write:** the
    rename had replaced the file by then, but the error made the app
    say "Save failed", skip the rebase and ask FileChanged about its
    own write at the next save; a `-wx` directory, where `File::open`
    fails, did it every time. It's a `log::warn!` now. Test:
    `a_directory_that_cant_be_synced_still_saves`.
  - **Temp names fit:** `.<name>.<16 hex>.sheaf-tmp` adds 28 bytes
    (the old `.sheaf-tmp`, 11), so names of 228–254 bytes failed every
    save with ENAMETOOLONG. The name is cut at a character boundary so
    the whole fits 255 bytes, which also bounds NTFS's UTF-16 units; a
    name's bytes that aren't UTF-8 become U+FFFD in its temp's. Test:
    `a_long_name_gets_a_temp_name_that_fits` (250 bytes of ASCII, 253
    of three-byte characters).
  - **The check before the rename has its own test:** the closure in
    `save_to_path_noting` is `write_checked`, and
    `a_file_changed_while_saving_asks` checks that a stale stamp gives
    the SaveStop and leaves the file, with no temp. The bite check
    below that turned "the up-front stamp check off" didn't bite
    `a_file_changed_since_open_asks`, as the step said: with only that
    check off, the durable re-check gives the same SaveStop and the
    test passes (rerun in the review).
  - **The stamp library writes durably:** `stamps::write` had the
    fixed-name, unsynced `.pdf.part` rename `images::write_atomic` was
    retired for, and calls `durable::write` now. The collection's
    bytes are still lopdf's writer's (Found).
  - **The README's changed-file promise is scoped:** a recovered
    document's primary is its temp file, so a save to the original
    isn't checked (Left for later slices, slice 9). The save-model row
    and document-model.md say so.
  - **The corpus check decrypts:** it compared `/P`, `/ID[0]`, page
    counts and that PDFium opens, none of which reads an encrypted
    payload, so objects encrypted with a wrong key would have passed.
    Measured below.

Measured (AMD EPYC 7302P, 32 threads, release builds unless marked):
- **The 7k harness,** the writer rows and slice 7's, two runs at the
  end (the first before the harness synced its copies; slice 6's in
  brackets, slice 2's baseline after them where the table has one):

  | Row | Synthetic | Heavy | Scanned |
  |---|---|---|---|
  | incremental save, one mark | 62.9–83.5 ms [51.3; 56.6] | 288–292 ms [144; 166] | 890–1,017 ms [675; 742] |
  | incremental save, RSS added | 36.9 MB [41.4; 35.5] | 211–213 MB [—; 216] | 1,014–1,015 MB [—; 1,004] |
  | Rewrite | 123–126 ms [129; 300] | 568–585 ms [771; 3,082] | 512–523 ms [556; 1,201] |
  | Rewrite, peak / added RSS | 99–102 / 52.9–53.2 MB [—; 78.0 / 50.5] | 547–548 / 246–250 MB [563 / —; 417 / 248] | 1,569–1,579 / 1,009–1,011 MB [1,590 / —; 1,078 / 1,021] |
  | Rewrite, the file written | 3.27 MB [3.27] | 17.7 MB [17.7] | 473 MB [473] |
  | Bates, the Rewrite that draws it | 361–363 ms [470; 750] | 797–816 ms [1,075; 3,979] | 837–844 ms [1,007; 1,708] |
  | flatten | 189–284 ms [142; 336] | 1,400–1,510 ms [1,587; 3,489] | 1,166–1,424 ms [1,260; 1,495] |
  | incremental save, one mark, AES-256 | 285–317 ms | 582–665 ms | — |
  | Rewrite, AES-256 | 213–214 ms | 1,075–1,144 ms | — |

  A dash is a number slice 6 didn't record (it gave the RSS rows as
  within 3–4% of slice 2's). The rows slice 7 doesn't touch are within
  a few percent of slice 6's: the geometry walk 48.9–49.5, 75.8–75.9
  and 61.2–61.4 ms; ready 49.9–50.1, 377–386 and 446 ms; classify
  144–146, 202 and 77.3–79.4 ms; the scanned crop 1,179–1,269 ms
  (1,159, and 1,250 in slice 6's review rerun). History:
  AnnotAdd 0.699 ms and a full reorder 0.950 ms, asserted; 200 OCR
  versions 4,074 MB of steps and 4,738 MB of RSS. Ready on 20,000
  sheaf marks 544 ms [531–540]; their Rewrite 1,126 ms [1,118–1,122]
  and 228 MB added [227–228].
- **The Rewrites are 2–26% faster** (Bates' 16–26%) without lopdf's
  `compress` pass and writer, and their RSS is as before; the
  fixtures' streams were already compressed, so their files are the
  same size.
- **The incremental save and the flatten now fsync the whole file.**
  Step 4 measured the incremental save before durable writes at
  48.7, 141 and 832 ms; the heavy's grows by about 150 ms, the fsync
  of a 14 MB file in the gate (about 12 ms on its own, in step 5).
  fsync latency here is erratic: the same 3.3 MB file took 1–185 ms
  across consecutive saves under strace, and 4–340 ms without, so the
  incremental and flatten rows swing between runs (the synthetic
  flatten 189 and 284 ms). Until slice 10's in-place append, a save in
  place writes and fsyncs the whole file: about 1 s on the scanned 7k.
- **AES-256** (the copies are 3,605,538 and 20,292,788 bytes, against
  3,510,145 and 14,364,550, lopdf's writer putting the heavy's
  object-stream members out as objects of their own): the incremental
  save costs 285 ms against 62.9 on the synthetic and 582 against 288
  on the heavy, the Rewrite 213 against 126 and 1,144 against 568.
  Most of it is lopdf's eager load decrypting every object, which the
  old writers do and slice 9's and 10's don't: `file::load_source`
  73.7 ms against 11.6, and 473 against 63.5. Encrypting every object
  of the document through `Crypt::encrypt` takes 17.6 and 124 ms.
  (Measured with a throwaway probe, since deleted.)
- **Output sizes, the old writer against the new** (step 3, a
  temporary harness over the same `build_rewrite` output): 615 corpus
  forms (77 curated and every 8th SafeDocs form; 73 encrypted), 277.8
  MB as originals, 289.0 MB by the old writer, 296.6 MB by the new
  (×1.026). 593 grew; the worst ×1.87 (safedocs/1630364: 135 kB old,
  254 kB new, 248 kB original): source streams stored unfiltered are
  no longer re-encoded, so outputs come back to about the original's
  size. Encrypted forms also grew by their strings while those were
  written hex (0719618: 171 kB → 299 kB, 227 kB original); keeping
  each string's format (review), 0719618 rewrites to 226,026 bytes
  against 299,209 with hex (a scratch harness, release, `save::rewrite`
  of the file as opened; the totals above weren't rerun). `finish`
  over all 615: 1.60 s → 1.67 s.
- **The encrypted corpus** (`encrypted_corpus`, test profile, 27 s,
  6.4 s before the review's checks): of the manifest's 4,392 files,
  573 are encrypted and open with the empty user password (34 V1 R2
  RC4, 135 V2 R3 RC4, 311 V4 R4 AESV2, 31 V4 R4 RC4, 62 V5 R6 AESV3).
  Rewrite kept the encryption on all 573, Append on 572, one declined
  as a plain file would be (forms/safedocs/0630899.pdf, damaged
  cross-references). Every output reopens with no password, still
  encrypted, with the same `/P`, `/ID[0]` and page count and page 1
  turned; page 1's content streams (their data as stored), the
  strings in its dictionary and those in `/Info` decrypt to what the
  original's do; and PDFium opens it and reads page 1's characters as
  it read the original's. PDFium orders them, spaces them and marks a
  hyphen ending a line (U+0002) by where they are, which the turn
  changes (1,067 outputs differed until they were compared sorted,
  without white space, U+0002 as a hyphen). Encrypting under the
  wrong generation fails 511 Rewrites (every one but AES-256's, whose
  key isn't per object) and 75 Appends (those whose update holds a
  string; the rest encrypt nothing).
- **qpdf --check** (12.3.2): the ignored `qpdf_checks_every_output`
  passes on all 28 outputs. An ad-hoc run over the corpus test's 1,145
  outputs (a temporary harness, since deleted): 1,096 clean, no errors;
  49 warn only where their originals warn of their own content (48
  widgets not reachable from `/AcroForm`, 10 empty name-tree nodes, 6
  Flate data that doesn't decode, 4 unsorted name trees, 2 duplicate
  `/Annots` entries, 1 `/Size` kept from the base); 544 outputs of
  originals that warn (mostly linearization hint tables) pass clean.
- **Suites:** `cargo test --workspace` 782 passed, 23 ignored, in 2 m
  27 s with a 31 s build (751 and 21 at slice 6); sheaf-doc's unit
  tests 435 in 34.3 s (401 in 27.8 s). After the review, 786 passed
  and 23 ignored in 3 m 0 s, its build included; sheaf-doc's unit
  tests 439 in 30.1 s; the ignored qpdf tier passes again (28
  outputs), pdf.js still skipped.
- **The app by hand** (Xwayland and Gdk grabs, debug build): an
  RC4-128 file with user password "user" opened at the prompt; after
  a page turn Ctrl+S reported "Saved … (Incremental)", the file
  starting with the original bytes, holding no plaintext, keeping
  `/Encrypt 11 0 R` and `/ID[0]` with a new `/ID[1]`, read back with
  the password; a fresh run reopened it, page 1 turned. Another
  program's append to a plain file mid-session, then Ctrl+S, asked:
  Cancel left the file byte for byte, Overwrite saved Incremental onto
  the bytes sheaf read, and after a `touch` (mtime only) Save As…
  wrote a Rewrite to the picked path and left the original and its
  mtime alone. No temp files were left.

Found:
- **`Renderer::page_text` loses text on pages turned 90 or 270:**
  pdfium-render's `PdfPageText::all` bounds the text by the page's
  turned size in unturned user space, so on a Letter page with
  `/Rotate 90` text at y = 740 reads "". It affects Export Text
  (sheaf-ops export.rs:152), blank-page detection (blank.rs:143) and
  plugins' `page_text`. Not fixed; it needs a slice to own it.
- **lopdf's readers ignore free entries:** neither the table nor the
  stream parser records them, and `Xref::merge` keeps the older
  section's entry, so an object an append frees still reads from the
  earlier revision in lopdf and in sheaf, though PDFium and qpdf honour
  the free entry. Slice 7 writes no deletes; slice 10's Append deletes
  need it fixed.
- **PDFium rebuilds cross-references it can't follow,** so a PDFium
  check can't catch wrong offsets: with the header offset forced to 0
  the PDFium prefix test passed while LazyPdf's and both qpdf checks
  failed. Its doc comment says so.
- **lopdf's eager reader strips `/Encrypt` itself** (above), and its V5
  encode still leaves out `/Length` (M11), which qpdf warns of.
- **`write_atomic` replaced a symlink with a regular file,** and
  sheaf-ops had its own copy of the fixed-name, unsynced writer.
- **Writes still outside the serializer or durable writes:** lopdf's
  writer writes the comment summary and FDF files; the derived copies
  in memory; PDFs made from images and blank pages
  (`sheaf_doc::images::to_pdf`, `generated::blank_pages`), which are
  Images to PDF and a scan without OCR at the reader's path (durably,
  sheaf-ops images.rs:86 and scan.rs:236) and inserted image and blank
  pages and Combine's image items through the generated cache
  (page_insert.rs:83 and :131, combine.rs:101); the stamp library's
  collection, one-page and image-stamp files (`stamp_lib`'s `finish`,
  stamp_lib.rs:268); and pictures PDFium draws from memory (a tape,
  the layers panel's copy). Print's Save as PDF, page images, comment
  exports and the CLI's outputs are `std::fs::write`s (slice 19's
  exports), and the generated cache keeps its own temp-and-rename
  (generated.rs `write_new`, unsynced, gone with the cache in 19).
- **rustdoc finds six broken intra-doc links** in forms/ and open/
  (not CI-checked); the two slice 7 touched (save.rs's `Document`,
  object.rs's ambiguous `write`) are fixed.

Checked by tests:
- RC4-128, AES-128 and AES-256 files save encrypted by both
  strategies and reopen in PDFium with their password, keeping `/P`
  and `/ID[0]`: sheaf-render
  `pdfium::encrypted::encrypted_files_reopen_in_pdfium_by_both_strategies`
  (table and stream, a turn and a note, Rewrite and Append: refused
  without the password, no form text in the clear, and with it what
  the same edit of the plain file shows: pages, size, a pixel hash,
  text, annotations, field values and bookmark titles), backed by
  sheaf-doc's `rewrites_stay_encrypted` and
  `encrypted_files_append_encrypted` (each also with an `/ID` of one
  string; and `/EFF`, its strings in the clear as expected), `an_eff_file_rewrites_with_only_its_attachment_encrypted`
  and save's `encrypted_source_saves_encrypted` (Rewrite and in place,
  `Strategy::Incremental`), sheaf-ops'
  `encrypted_corpus_files_save_encrypted_by_both_strategies` (573
  corpus files) and
  `an_encrypted_signed_file_appends_and_keeps_its_signature`, and
  recovery's `an_encrypted_sidecar_holds_no_plaintext`;
- an AES-256 xref-stream base whose strings live in object streams
  rewrites, and PDFium reads its `/T`, `/V` and outline titles:
  `an_aes256_object_stream_base_rewrites` (no `/ObjStm` and no
  plaintext in the output, and what PDFium shows for the base), with
  sheaf-doc's `an_encrypted_object_stream_base_rewrites` (RC4,
  AES-128, AES-256);
- a file with a 100-byte prefix appends and opens identically in
  PDFium, pdf.js and qpdf: `a_file_with_a_100_byte_prefix_appends`
  (byte-identical past the prefix to the unprefixed append, read by
  LazyPdf), `a_prefixed_file_appends_as_pdfium_reads_it`, the ignored
  `a_prefixed_append_opens_identically_in_pdfjs_and_qpdf` (qpdf and
  pdf.js, both run and passing) and `positions_count_from_the_header`;
- reals round-trip at f64, and NaN is refused:
  `reals_round_trip_at_f64` and `nan_and_infinities_are_refused`;
- a file modified by another program between the save starting and
  the rename gives `FileChanged`, and nothing is overwritten:
  `durable::a_file_changed_before_the_rename_is_left_alone` (appended,
  same length with a new tail, mtime only; a removed file isn't
  recreated; no temp left), save's `a_file_changed_while_saving_asks`
  (the check before the rename gives the SaveStop the app downcasts,
  the file theirs and no temp) and `a_file_changed_since_open_asks`
  (Ask, Rewrite and Append downcast to the SaveStop and leave the
  file; Save As writes elsewhere; Overwrite appends to the bytes
  read);
- concurrent saves get distinct temp names:
  `concurrent_saves_get_distinct_temp_names` (16 threads, 1 MB+ each,
  one input whole, no temps left);
- permissions are preserved: `permissions_are_preserved` (0o640 and
  0o600 kept, a new file the umask default), with
  `a_symlinked_file_stays_a_symlink`,
  `a_long_name_gets_a_temp_name_that_fits` and
  `a_directory_that_cant_be_synced_still_saves`;
- `qpdf --check` passes on the outputs: the ignored
  `qpdf_checks_every_output` (28 outputs);
- also: `strings_names_and_streams_round_trip`,
  `a_full_table_links_its_free_entries`,
  `appended_runs_free_deleted_objects_with_the_next_generation`,
  `an_appended_free_entry_takes_the_next_generation`,
  `an_xref_stream_lists_its_index`,
  `a_table_refuses_offsets_past_ten_digits`,
  `objects_encrypt_as_the_lazy_reader_decrypts`, `ivs_are_keyed`
  (twice and on two threads; another value, number or generation gets
  another IV), `xref_streams_metadata_and_the_encrypt_dict_stay_plain`,
  `full_writes_are_deterministic`, `print_is_unencrypted`,
  `a_full_write_keeps_the_trailer_but_the_bases_sections`,
  `an_encrypted_source_keeps_how_it_is_encrypted`,
  `a_base_numbered_to_the_last_number_isnt_appended_to`, forms_saves'
  `a_save_that_would_rewrite_a_signed_file_asks`, and the ledger's
  `nothing_retired_is_back` with the 7 rows. Each step checked its
  tests bite (the header offset forced to 0, the crypt forced to None,
  random IVs, a fixed temp name), and put the code back; so did the
  review (the two-string `/ID` rule, the unchecked `size`, the
  directory sync failing the write, no cut in the temp name,
  `write_checked`'s `Changed` as a plain error, and a wrong
  generation in the corpus).

Left for later slices:
- **Slice 9:** ObjKey in the IV; `SaveStop` and `Ask` moved to
  `materialize/`, with `SaveStop::Failed`, `Ask::Signatures` and
  `Ask::XfaHoldsValues`, and `IfChanged` replaced by the plan's ask;
  `Profile` (`Keep`/`None`) in place of `kept_encryption` and
  `finish`'s `Option<&Crypt>`; the emitter's PDF version (full.rs
  writes the document's); raw passthrough only when the `Crypt` is
  the same (§11.6); the Unchanged plan writing nothing (the old entry
  point writes the bytes read back when there's no section);
  `Progress` and cancel in `serialize` (`was_cancelled`); the old
  writers' use of `serialize/` with them (transitional ledger); the
  recovered document's temp primary, stamped on its temp file, so a
  save to the original path is a Rewrite with no stamp check (the
  README's save-model row says so until then); and a primary with no
  stamp (blank, memory, derived) saved to a path of the same name,
  which nothing checks (`NoSaveRecord`). Its Deletes no longer
  include `save_to_path_noting`'s length-only disk check, gone here
  (the plan's slice table, §17 and slice 9's Deletes now say so).
- **Slice 10:** the full `FileStamp` (the window at the last
  `startxref` and `content_hash`); the in-place Append with its
  journal and torn-tail recovery, so a save in place stops rewriting
  and fsyncing the whole file; `Onto::CopyOf`; encrypted sidecars for
  password-protected documents, ending the autosave skip, and
  recovery's `find`/`materialize`, which still compare only the
  original's length; Append numbering above `max_stream` from the
  SaveRecord (the old writer already numbers past `Base.size`); free
  entries that lopdf's readers honour, before Append deletes.
- **Slices 15 and 17:** crop's copy and the other derived copies, still
  written by lopdf's writer.
- **Slice 19:** export presets and their profiles; print, page
  images, comment exports and the comment summary through the
  serializer and durable writes; Images to PDF, scans and the image
  and blank pages that are memory sources there, so
  `images::to_pdf`, `files_to_pdf` and `generated::blank_pages` stop
  using lopdf's writer; and the stamp library's files (`stamp_lib`'s
  `finish`) through `full::write`, which no slice of the plan owned
  (they're the reader's own, outside the edit API, §15).
- **M11:** new encryption (setting, changing and removing passwords
  and permissions, a per-export choice, proposal 10), lopdf's V5
  `/Length`, and zeroizing keys (`Crypt` and `LazyPdf` each hold the
  file key, unzeroized).
- **A slice to own** `Renderer::page_text` on turned pages (Found).
- **docs/prior-art/rust-ecosystem.md:149** still says saves come out
  unencrypted; it's a dated survey and was left as written.
