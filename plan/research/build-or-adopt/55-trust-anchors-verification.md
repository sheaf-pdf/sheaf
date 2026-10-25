# Build or adopt 55: Trust anchors for verification

Area: M12: Signatures · Part of [build or adopt](README.md)

- **Need:** what to trust: certificates the user imports; the OS stores
  (Windows ROOT, macOS trust settings); the Adobe Approved Trust List;
  the EU Trusted Lists. Linux has no store for document-signing roots,
  and webpki-roots is a TLS list.
- **Where:** M12 slice 3; the roadmap's "validation status" implies it.
  D13.
- **Candidates:** the windows crate and security-framework read the OS
  stores (entries 50–51). No Rust crate verifies the EU list's XAdES
  signature maturely; Kushal Das's bergshamra and uppsala are new and
  weren't checked. Whether third parties may use the Adobe list is
  unverified.
- **Recommendation:** **undecided;** decide at kickoff. A reasonable
  start: user-trusted certificates plus the OS stores, the EU list later.
  Low.
