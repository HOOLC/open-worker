# Android certificate verifier compatibility patch

Source: rustls/rustls-platform-verifier v0.7.0, commit `2c158c5edca13283e301a26af946d8545e2d505a`.
The JNI ABI corresponds to Rust verifier 0.7.0 / Android bridge 0.1.1. MIT license is included.

Upstream issues: https://github.com/rustls/rustls-platform-verifier/issues/221 and https://github.com/rustls/rustls-platform-verifier/pull/179.

Local changes:

- Library-only `BuildConfig.TEST` is explicitly false. No upstream test-only verification bypass is enabled.
- Only a certificate with CRL Distribution Points, without an OCSP access-method OID, and without stapled OCSP selects CRL-first/no-OCSP-fallback checking. Existing chain, validity, EKU, hostname (Rust) and revocation validation remain enabled; a revoked CRL result remains an error.
- Verification is serialized, as the shared certificate factory and trust-anchor cache are not thread safe.
- Failed trust/revocation results have a bounded 30-second monotonic cooldown (64 entries), keyed by server, authentication method, EKUs, full chain and OCSP staple. A changed certificate/staple is checked immediately. Foreground resume and explicit reconnect clear failures. Successful results are never cached; a trust-store update can delay recovery from a prior failure by at most 30 seconds.
- Revocation reuses only the trust anchor just accepted by the platform and excludes that anchor from the certification path. It no longer enumerates the entire Android keystore per handshake. Platform chain validation still runs on each non-cooled-down attempt; no positive trust decision is cached.
- Other certificates and explicit OCSP staples retain upstream validation. Existing best-effort SOFT_FAIL policy is unchanged.

The application compiles this source instead of the packaged AAR classes. The build script checks the pinned Rust/bridge versions so a dependency update cannot silently change its JNI ABI. Remove this copy after adopting an upstream release with equivalent CRL-only support.

Current coordinated device evidence and ownership: send investigation（本地生成的验收记录）.
