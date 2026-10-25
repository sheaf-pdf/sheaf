# Build or adopt 52: RFC 3161 timestamps

Area: M12: Signatures · Part of [build or adopt](README.md)

- **Need:** build a TimeStampReq, POST it (`application/timestamp-query`),
  parse the TimeStampResp, check its token (the TSA's chain with
  id-kp-timeStamping, the messageImprint, the nonce); embed the token as
  a signature timestamp (PAdES B-T) or a `/DocTimeStamp` (ETSI.RFC3161,
  B-LTA); verify existing tokens (the spike matched 53 imprints).
- **Where:** M12 slice 5 ("RFC 3161 timestamps; LTV (embed revocation
  info)", `ROADMAP.md:401`). D13.
- **Candidates:**
  - [x509-tsp](https://crates.io/crates/x509-tsp) 0.1.0 (2023-06-26;
    Apache-2.0 OR MIT; RustCrypto): types only, on the old line (der
    0.7, cms 0.2). 0.2.0-pre on cms 0.3.0-rc.0 is on master,
    unpublished.
  - **[tsp-ltv](https://crates.io/crates/tsp-ltv)** 0.6.0 (2026-10-16;
    BSD-2-Clause; Kushal Das alone; created 2026-03; 228k downloads; 2★;
    no `unsafe`): a TSA client and pool, OCSP and CRL clients, chain
    building, trust stores. On the RustCrypto 0.2 line, with reqwest
    0.12, tokio and rustls; async, with blocking wrappers.
  - cryptographic-message-syntax 0.28 has a TSP client (MPL-2.0; entry
    44).
  - **HTTP:** sheaf-app already links hyper 1.12, tokio and rustls 0.23
    (through wasmtime-wasi-http). The installer plan picked ureq
    (`plan/topics/installer.md`). TSAs, OCSP and CRLs are mostly plain HTTP.
- **Recommendation:** **build on x509-tsp 0.2 and cms 0.3** once
  published, with a small synchronous, bounded HTTP client. Medium. Use
  tsp-ltv as prior art and a test oracle; adopt it only if it moves to
  the 0.3 line and lets callers supply the transport (kickoff).
