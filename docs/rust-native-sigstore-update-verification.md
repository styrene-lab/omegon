---
id: rust-native-sigstore-update-verification
title: "Rust-native Sigstore verification for self-update"
status: exploring
parent: update-check-restart
tags: [release, security, update, rust]
open_questions:
  - "Does `sigstore-verification` support verifying detached blob signatures + certificate identity constraints for GitHub Release assets directly, or do we need to use lower-level `sigstore` APIs?"
dependencies: []
related: []
---

# Rust-native Sigstore verification for self-update

## Overview

Replace external cosign dependency in the self-updater with in-process Rust verification of GitHub Release archive signatures and certificates. The updater must fail closed, enforce GitHub Actions identity policy for styrene-lab/omegon release artifacts, and avoid platform-level tool dependencies.

## Research

### Rust dependency reconnaissance

`cargo search sigstore` shows viable Rust libraries in crates.io. `cargo info sigstore --locked` reports `sigstore 0.13.0` from `sigstore-rs`, marked 'An experimental crate to interact with sigstore' with verify/fulcio/rekor features. `cargo info sigstore-verification --locked` reports `sigstore-verification 0.2.1`, described as 'Sigstore, Cosign, and SLSA attestation verification library'. This suggests a Rust-native path is viable without shelling out to `cosign`, but the narrower `sigstore-verification` crate may be a better fit than the broader experimental `sigstore` crate.

### Source inspection: sigstore-verification limits

Inspected downloaded `sigstore-verification 0.2.1` source under cargo registry. The crate is real and exposes `verify_cosign_signature(...)`, but `verifiers/cosign.rs` contains explicit incomplete-verification shortcuts in the keyless path, including comments like 'For full verification, we would...' and 'For now, we verify the basic structure and digest match'. That is not strong enough to serve as Omegon's security boundary for self-update verification.

## Decisions

### Do not use sigstore-verification as the updater trust boundary

**Status:** decided

**Rationale:** Although the crate is useful for attestation-oriented workflows, source inspection shows its keyless Cosign verification path is intentionally incomplete and not appropriate as the sole cryptographic verification layer for a self-updating binary. Omegon should keep the current fail-closed cosign bridge until a stronger Rust-native verifier is implemented using lower-level APIs.

### Defer full Rekor/trust-root verification beyond this RC

**Status:** decided

**Rationale:** For the next RC, the priority is shipping an updater that works in-process and can carry future upgrades through `/update` without external platform dependencies. Rust-native blob signature verification plus certificate identity policy is sufficient for this release candidate. Full Rekor/trust-root verification remains important, but is deferred to a follow-up hardening pass rather than blocking the updater milestone.

## Open Questions

- Does `sigstore-verification` support verifying detached blob signatures + certificate identity constraints for GitHub Release assets directly, or do we need to use lower-level `sigstore` APIs?
