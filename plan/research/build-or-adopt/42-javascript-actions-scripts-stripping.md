# Build or adopt 42: JavaScript, actions and scripts: stripping

Area: M11: Security and redaction · Part of [build or adopt](README.md)

- **Need:** remove `/OpenAction`; `/AA` on the catalog, pages,
  annotations and fields; `/Names /JavaScript`; JavaScript, Launch, URI,
  SubmitForm, ImportData, GoToR, GoToE and RichMedia actions, following
  their `/Next` chains; field scripts; XFA `<script>` elements in the
  template packet. No dangling references.
- **Where:** M11 slice 9; M10's handoff.
- **Today's plan:** build. "links, actions and JavaScript (`/OpenAction`,
  `/AA`, `/Names /JavaScript`, Launch, URIs)" (`m11.md:250-251`);
  "Sanitize removes field and document scripts and offers to flatten"
  (`m10.md:1141-1142`).
- **Candidates:** none over lopdf objects. pdf_oxide's
  `redaction/sanitize.rs` works in its own object model: prior art only.
- **Recommendation:** **build in-house** over the M10.5 emission walk,
  with quick-xml for XFA scripts. High.
