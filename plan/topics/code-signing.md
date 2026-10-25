# Code signing for release builds

Status: **proposed** (2026-10-17). Nothing is signed and nothing here is
built. This memo records the research, the open decisions, and a first cut
of the slices.

The goal: CI builds, signs and publishes every release bundle
(`release.yml` on a `v*` tag), with no manual step and no signing key on
anyone's machine.

This is about signing sheaf's own executables. PDF digital signatures are
M12, a separate topic.

## Why

- **Windows.** An unsigned `sheaf.exe` from a downloaded zip gets
  SmartScreen's "Windows protected your PC" with "Unknown publisher".
  Smart App Control in Windows 11, where it's turned on, goes further. It
  checks every code module it loads, DLLs included, and blocks unsigned
  files that Microsoft's cloud service doesn't already trust.
- **Every platform.** Users, and the `sheaf-pdf` installer
  ([`installer.md`](installer.md)), need a way to check that a bundle came
  from our CI and wasn't changed afterwards.

## Where things stand

- `release.yml` builds on `ubuntu-latest` and `windows-latest`.
  `scripts/package.sh` builds, stages and archives in one run, so signing
  has no place to run between staging and zipping.
- The PE files in the Windows bundle:

  | File | Comes from | Signed? |
  |---|---|---|
  | `sheaf.exe`, `sheaf-cli.exe` | our build | no |
  | `pdfium.dll` | `bblanchon/pdfium-binaries` (community build) | to check; probably not |
  | `ocr/lib/onnxruntime.dll` | `microsoft/onnxruntime` releases | to check; probably by Microsoft |

  Plugins are WASM components, not PE files, so Authenticode doesn't apply
  to them (see [Not covered](#not-covered)).
- No `SHA256SUMS` and no attestations.
- Every job gets `contents: write`. Third-party actions are pinned by tag,
  not by commit. Release builds restore `rust-cache` and the Tesseract
  cache.

## Windows: where the key lives

Since June 2023 the CA/Browser Forum's rules require code-signing keys to
be generated and kept in hardware (an HSM or a token). CAs no longer issue
a `.pfx` that could sit in a CI secret, and GitHub's hosted runners can't
use a USB token. Signing in CI therefore has to go through a cloud signing
service.

Researched 2026-10-17 (sources at the end). Prices and eligibility change
often, so check them again before choosing.

### Azure Artifact Signing (formerly Trusted Signing)

- Microsoft validates the publisher and issues short-lived certificates
  that it rotates on its own, so every signature must be timestamped.
- Basic tier: $9.99 a month for 5,000 signatures. A release signs three or
  four files.
- Eligibility for publicly trusted certificates:
  - Organizations: the US, Canada, the EU, the UK, Australia, New Zealand,
    Japan, South Korea, Singapore, Switzerland, Norway and Israel. Older
    docs list fewer countries.
  - Individuals: the US and Canada only.
- CI: [`Azure/artifact-signing-action`](https://github.com/Azure/artifact-signing-action)
  on the Windows runner.
  - It logs in with GitHub's OIDC token through a federated credential on
    an Entra app registration, so no signing secret is stored anywhere.
  - The app needs the *Artifact Signing Certificate Profile Signer* role.
  - The job needs `id-token: write`.
  - The federated credential's subject can be limited to one environment:
    `repo:sheaf-pdf/sheaf:environment:release`.
- Publisher shown: the validated name, a person or an organization.

### SignPath Foundation (free for open source)

- The certificate belongs to SignPath Foundation, so users see **"SignPath
  Foundation"** as the publisher, not sheaf.
- Its [conditions](https://signpath.org/terms) include:
  - every component under an OSI-approved license, with no commercial
    dual-licensing, and no proprietary components (system libraries are
    fine);
  - no malware or potentially unwanted programs;
  - the project is actively maintained and has **already released** the
    software in the form to be signed;
  - the download page describes what the software does.
- Builds must be fully automated and use
  [origin verification](https://docs.signpath.io/origin-verification),
  which checks that each artifact came from our workflow at a known commit.
- Projects publish a code signing policy that lists team roles. Most also
  require a person to approve each signing request, as in
  [Super Productivity's policy](https://super-productivity.com/code-signing/).
  That conflicts with the "no manual step" goal. Our policy page would go
  on sheaf-pdf.com, in sheaf-web.
- To check: whether they'll sign `pdfium.dll`, which is open source but a
  third-party binary we don't build.

### OV certificate from a CA, with a cloud HSM

- SSL.com eSigner, DigiCert KeyLocker and others. They cost a few hundred
  dollars a year, and some also charge per signature.
- CI logs in with long-lived credentials kept in secrets, for example a
  password plus a TOTP seed, or an API key plus a client certificate.
- Works in any country the CA validates. The publisher is the validated
  name.

### Not worth it

- **EV certificates.** Since 2024 an EV certificate no longer earns
  immediate SmartScreen reputation. Most sources agree on this; ngrok's
  February 2026 post disagrees. Reputation now builds per publisher and per
  file hash, so EV costs more and buys us nothing.
- **Self-signed certificates.** Windows doesn't trust them, so users still
  see "Unknown publisher".

### Recommendation

- **Artifact Signing** if the publisher is eligible. It's the cheapest, it
  was built for CI, and the repo holds no signing secret.
- **SignPath Foundation** if cost matters more than the publisher's name,
  and the per-release approvals and the policy page are acceptable.
- **A CA certificate with a cloud HSM** otherwise.

### What signing won't fix

Signing is necessary but not sufficient.

- Every new build starts with no SmartScreen reputation and earns it as
  people download it, so early releases may still show a warning.
- Smart App Control has been reported to block signed files that few
  people have downloaded.
- False positives can be reported to Microsoft Security Intelligence.
- To keep reputation building: timestamp every signature, keep the
  publisher name the same from one release to the next, and never change a
  file after signing it.

## Open decisions

1. **Publisher.** The name appears in Windows' publisher prompts and in
   file properties. The `sheaf-pdf` GitHub organization isn't a legal
   entity, so signing as "sheaf" means forming one first, such as an LLC.
   Otherwise the certificate carries a person's name. The same choice
   comes up later for an Apple developer account, where an organization
   account needs a D-U-N-S number.
2. **Provider**, from the options above. It depends on 1 and on country.
3. **Third-party DLLs.** Should `pdfium.dll` be signed under our name?
   Recommended: yes, because we ship it and Smart App Control checks it.
   Leave `onnxruntime.dll` alone if Microsoft has already signed it.
4. **Approvals.** Fully automatic on a tag, or a required reviewer on the
   `release` environment? The goal is automatic, and the tag ruleset in
   slice 2 keeps that safe. SignPath may require approval regardless.

## Slices

Slices 1–3 don't depend on the provider.

### 1. Checksums and attestations

- Add a last job that downloads both bundles, writes `SHA256SUMS`, and
  publishes everything to the release. This is the "final job" option in
  [`installer.md`](installer.md#release-workflow-changes), which the
  installer needs anyway.
- Attest each bundle with [`actions/attest`](https://github.com/actions/attest).
  - `actions/attest-build-provenance` is now a wrapper around it.
  - The job needs `id-token: write` and `attestations: write`.
  - It's free for public repos. Attestations are signed through the public
    Sigstore instance.
  - Anyone can check a file with
    `gh attestation verify <file> --repo sheaf-pdf/sheaf`.
- On Linux this is all that's needed, since nothing checks signatures on a
  tarball. Flatpak or AppImage, if they come later, bring their own
  signing.

### 2. Harden `release.yml`

- Set permissions per job, with `contents: read` at the top level.
- Pin the third-party actions (`softprops/action-gh-release`,
  `Swatinem/rust-cache`, `dtolnay/rust-toolchain`) to commit SHAs. They run
  in the same workflow as signing.
- Drop caches from release builds. Caches are shared with other workflows,
  so a poisoned one could end up inside a signed binary. Releases get
  slower, but they're rare.
- Add a tag ruleset: only maintainers create `v*` tags, and nobody moves or
  deletes them.
- Add a `release` environment that only `v*` tags can deploy to.
  - It holds the signing settings (account and profile names) as
    variables.
  - Slice 4's federated credential points at it.

### 3. Split packaging

- Give `package.sh` a way to stop after staging `dist/<name>/`, and a way
  to archive an existing stage. The flag names are still to decide;
  `--no-build` is the precedent.
- The bundle layout stays the same, since it's part of the bundle contract
  in [`installer.md`](installer.md).
- The Windows build job uploads the staged folder as a workflow artifact
  instead of zipping it.

### 4. Sign on Windows (needs a provider)

- A separate `sign-windows` job, in the `release` environment, with
  `id-token: write`.
  - The build job doesn't get that permission. `cargo build` runs build
    scripts from every dependency, and none of them should be able to
    reach the signing service.
- It downloads the staged folder and signs `sheaf.exe`, `sheaf-cli.exe`
  and `pdfium.dll`, plus `onnxruntime.dll` if it isn't signed already. It
  uses SHA-256 digests and an RFC 3161 timestamp.
- It checks every PE file in the stage with `signtool verify /pa`, and
  fails if any file is unsigned.
- Then it archives the stage, and slice 1's job checksums and attests the
  signed bundle. Order: sign → zip → checksum → attest.

### 5. The website

- sheaf-web's Download page lists the checksums and shows how to verify a
  download.
- With SignPath, the code signing policy page goes there too.

## Later: macOS

macOS is tier 2, but its signing fits the same pipeline:

- It needs the Apple Developer Program, at $99 a year. An organization
  account needs a D-U-N-S number.
- It uses a Developer ID Application certificate. Unlike on Windows, this
  can be exported as a `.p12` and kept in a CI secret.
- Sign with the hardened runtime, then notarize with `notarytool` and an
  App Store Connect API key.
  - A notarization ticket can be stapled to an app bundle or a `.dmg`, but
    not to a zip or a bare binary. The macOS bundle will probably have to
    become a `.app`.
- It runs on a macOS runner, or from Linux with `rcodesign`.

## Not covered

- **PDF digital signatures.** These are M12 in
  [`ROADMAP.md`](../../ROADMAP.md).
- **Plugin signing.** `.sheafplugin` files aren't signed, and there's no
  plugin catalog yet ([`docs/plugins.md`](../../docs/plugins.md)). That's a
  separate trust question.

## Sources

Looked up 2026-10-17.

- Microsoft:
  - [Code signing options for Windows apps](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/code-signing-options)
  - [Artifact Signing quickstart](https://learn.microsoft.com/azure/artifact-signing/quickstart)
  - [Artifact Signing pricing](https://azure.microsoft.com/en-us/pricing/details/artifact-signing/)
  - [`Azure/artifact-signing-action`](https://github.com/Azure/artifact-signing-action)
- SignPath:
  - [Conditions](https://signpath.org/terms)
  - [Origin verification](https://docs.signpath.io/origin-verification)
- EV certificates and SmartScreen:
  - [ToDesktop](https://www.todesktop.com/blog/posts/windows-apps-psa-ev-certs-do-not-grant-immediate-reputation-anymore)
  - [ngrok, which disagrees](https://ngrok.com/blog/windows-ev-certificate)
- Smart App Control:
  - [textslashplain.com, 2026-04-28](https://textslashplain.com/2026/04/28/smart-app-control/)
  - [Microsoft Q&A: signed files still blocked](https://learn.microsoft.com/en-us/answers/questions/5791210/application-signed-and-blocked-by-smart-app-contro)
- GitHub:
  - [Artifact attestations](https://docs.github.com/actions/security-guides/using-artifact-attestations-to-establish-provenance-for-builds)
