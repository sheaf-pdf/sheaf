# M18 slice 7: pdf-structure, writing

Status: open · Part of [M18: the PDF layer](../README.md) · in pdf-lint's workspace, by pdf-lint's agent

**What:** pdf-structure writes what it reads: the generic half of
sheaf's `serialize/`, moved, MIT OR Apache-2.0 as sheaf's code is.

- **Cross-reference sections:** tables and streams (`serialize/xref.rs`).
- **Object-stream containers** (`serialize/objstm.rs`). Grouping by
  page (`serialize/group.rs`) goes to the linearization crate, as
  planned between milestones.
- **A whole file and an incremental section:** the assembly of
  `serialize/full.rs` and `append.rs`, given numbered objects.
- **Encryption on write:** what's encrypted and what isn't, by the
  rules the reader decrypts by (`serialize/crypt.rs`'s walk), through
  pdf-crypt.
- **Verbatim copies:** an object written as the bytes it was read from,
  under decision 3's rules in [crates.md](../crates.md#decisions-across-the-family):
  encryption by pdf-crypt's `Method`, references kept, `/Length` as a
  reference refused, recovered objects never copied, object-stream
  members' decoded bytes into new containers.
- **sheaf keeps the policy:** which objects, their numbers (`ObjKey`),
  the profiles, durable writes (`serialize/durable.rs`).

**From sheaf** ([from-sheaf.md](../from-sheaf.md#object-streams-57-slice-7-and-pdf-lint)):
object streams as #57 landed them (`objstm.rs`, `group.rs`, `full.rs`'s
containers, `Crypt::packs`), and `xref.rs`, `append.rs`, `crypt.rs`,
each with what it's coupled to.

**Where:** `crates/pdf-structure/`; from sheaf, `serialize/`'s generic
code and tests.

**Checked:**
- read, write, read again gives the same objects, whole files and
  incremental sections, encrypted or not;
- `qpdf --check`, pdf.js and PDFium open what it writes;
- the copy rules, each with a test that fails without it;
- `serialize/`'s tests that move, unchanged.
