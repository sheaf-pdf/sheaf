# Build or adopt 94: Checking what sheaf writes against the Arlington PDF Model

Area: Backlog items · Part of [build or adopt](README.md)

- **Need:** check every object sheaf builds (annotations, fields,
  StructElems, output intents) against PDF 2.0's model.
- **Where:** F22; F9; M14; M17.
- **Candidates:** the Arlington model's TSVs (Apache-2.0; entry 68). No
  Rust checker exists; TestGrammar (C++) and veraPDF's Arlington service
  do. krilla runs Arlington in CI.
- **Recommendation:** **build** a small test-only reader of the TSVs
  (F22).
