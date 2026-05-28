# First extraction hardening pass

## Intent

Make the Rust SDK extraction unambiguous in the Omegon host baseline now that the SDK lives in `omegon-extension-rs` and is published as `omegon-extension = "0.25"`.

This pass focuses on small, high-signal fixes rather than deleting the internal crate immediately. The internal source directory still exists as historical source while the host branch stabilizes, but the host must stop depending on it and must stop prompting contributors to edit it as the active SDK.

## Fixes in this pass

1. Host and bundled browser extension consume the crates.io SDK.
2. Browser extension reports `SDK_CONTRACT_VERSION` instead of a stale literal.
3. Prompt extension-authoring detection recognizes standalone SDK repositories and no longer depends on `core/crates/omegon-extension` being present.
4. Extension authoring documentation points to `omegon-extension-rs` as canonical and says host-owned code lives in the Omegon repo.
5. Legacy internal SDK references are reduced to historical/design notes only.

## Deliberate deferrals

- Delete `core/crates/omegon-extension` in a separate commit after path-reference search and docs updates are clean.
- Implement host compatibility policy based on `SDK_CONTRACT_VERSION` in a separate runtime-focused pass.
- Classify remaining external first-party consumers such as `aether` and `lipstyk` separately.
