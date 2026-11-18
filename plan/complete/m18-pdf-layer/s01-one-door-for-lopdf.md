# M18 slice 1: One door for lopdf

Status: done · Part of [M18: the PDF layer](../../m18-pdf-layer/README.md) · in sheaf

**What:** every sheaf crate reaches lopdf through one module,
`sheaf_doc::pdf`, so later slices change what it points at rather than
203 files. No behaviour changes.

- `sheaf_doc::pdf` re-exports what sheaf names of lopdf: `Object`,
  `Dictionary`, `Stream`, `ObjectId`, `StringFormat`, `Document`,
  `Error` and `Result`, the `dictionary!` macro, and the reading types
  (`LazyPdf`, `ByteSource`, `FileBytes`, `ReadLimits`, `XrefFacts`,
  `XrefKind`, `xref::XrefType`, the `encryption` items `source/` and
  `serialize/crypt.rs` use).
- sheaf-doc's own modules name it too, not `lopdf::`.
- sheaf-ops, sheaf-render, sheaf-cli, sheaf-app, sheaf-ocr and
  sheaf-plugin drop their `lopdf` dependency: each depends on sheaf-doc
  already ([sheaf.md](../../m18-pdf-layer/sheaf.md#what-sheaf-uses-of-lopdf-2026-10-27)).
- **The inventory:** every method, function and trait sheaf calls on
  lopdf's types, with how many call sites, in this slice's notes. It's
  what [slice 3](s03-pdf-objects-editing-and-writing.md)'s owned form must
  cover and what [slice 10](../../m18-pdf-layer/slices/s10-an-in-memory-document-of-sheaf-s-own.md)'s
  document must offer. Calls that a better API would remove are marked
  so, not kept for their own sake.

**Where:** `crates/sheaf-doc/src/lib.rs` (a `pdf` module), every
`use lopdf` in the workspace, the six crates' `Cargo.toml`.

**Checked:**
- the workspace builds, and every test passes unchanged;
- a CI step, like "sheaf-util stays byte helpers": nothing but
  sheaf-doc depends on lopdf (`cargo tree`);
- a test that no file outside `sheaf_doc::pdf` names `lopdf::`, so the
  door stays the only one.

## Notes

Done 2026-11-17:

- `crates/sheaf-doc/src/pdf.rs` re-exports everything sheaf names of `lopdf`,
  plus a `dictionary!` wrapper macro (`$crate::pdf::Dictionary`) so callers
  outside `sheaf-doc` need no direct `lopdf` dependency.
- Rewrote 200 `.rs` files across the workspace to reach `crate::pdf` (inside
  `sheaf-doc`) or `sheaf_doc::pdf` (everywhere else), and dropped
  `lopdf.workspace = true` from `sheaf-ops`, `sheaf-render`, `sheaf-cli`,
  `sheaf-app`, `sheaf-ocr`, and `sheaf-plugin`.
- Enforced by `crates/sheaf-doc/tests/pdf_door.rs` (`syn` AST walk over
  `crates/`) and `cargo tree` in `just boundaries` and `.github/workflows/ci.yml`.

### Call-site inventory on `sheaf_doc::pdf` types

- **Constructors and variants:** `Object::string_literal` (430),
  `Object::Dictionary` (318), `Object::Reference` (305), `Stream::new` (273),
  `Document::load_mem` (241), `Object::Array` (217),
  `Document::with_version` (181), `Object::Stream` (139), `Object::Name` (136),
  `Object::String` (102), `Document::load` (92), `Object::Integer` (92),
  `Object::from` (74), `Dictionary::new` (68), `Object::Null` (66),
  `Object::Real` (52), `LazyPdf::open` (51), `Document::new` (46),
  `StringFormat::Hexadecimal` (33), `XrefKind::Stream` (33),
  `StringFormat::Literal` (25), `XrefKind::Table` (24),
  `LoadOptions::default` (14), `ReadLimits::DEFAULT` (9), `Object::Boolean` (9),
  `LoadOptions::with_password` (6), `Document::load_mem_with_options` (6),
  `FileBytes::new` (4).
- **Object / Dictionary / Stream methods:** `.get()` (2,077 across dicts and
  collections), `.set()` (998), `.as_str()` (310), `.as_dict()` (309),
  `.as_reference()` (290), `.as_bytes()` (206), `.remove()` (197),
  `.as_array()` (189), `.has()` (152), `.as_name()` (99), `.as_stream()` (98),
  `.get_mut()` (63), `.as_i64()` (52), `.as_float()` (46),
  `.decompressed_content()` (39), `.content` / `.content()` (32),
  `.as_array_mut()` (17), `.as_dict_mut()` (17), `.compress()` (15),
  `.as_bool()` (9), `.as_f64()` (6), `.type_name()` (6), `.set_content()` (5),
  `.set_plain_content()` (3), `.as_stream_mut()` (3), `.is_null()` (3).
- **Document / LazyPdf methods:** `.add_object()` (774),
  `.get_dictionary()` (275), `.dereference()` (163), `.save()` (148),
  `.new_object_id()` (147), `.xref()` (141), `.get_pages()` (131),
  `.get_object()` (88), `.catalog()` (88), `.save_to()` (86),
  `.get_dictionary_mut()` (85), `.base_sound()` (66), `.ids()` (44),
  `.trailer()` (36), `.object_unkept()` (24), `.page_iter()` (23),
  `.raw_span()` (17), `.is_encrypted()` (14), `.get_object_mut()` (12),
  `.is_read()` (10), `.raw_span_whole()` (9), `.version()` (7), `.encrypt()` (7),
  `.catalog_mut()` (4), `.encryption()` (4), `.prune_objects()` (3),
  `.objects` (2), `.save_with_options()` (1).
