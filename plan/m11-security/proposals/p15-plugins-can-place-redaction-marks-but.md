# M11 proposal 15: Plugins can place redaction marks but never apply them

Part of [M11: Security & redaction](../README.md) · [all proposals](README.md)

**Plugins can place redaction marks but never apply them.** Applying
needs the user, through Export Redacted Copy, and is a `Transform`,
which no plugin edit can hold. Redaction marks join the WIT `annot`
kinds in the one API bump at the end of M11 (0.6, since M10 takes
0.5), and the ops plugins submit (a new `edit-schema` minor).
