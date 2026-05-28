# Internal Rust SDK source removal plan

## Intent

Finish the host-side Rust SDK extraction by removing Omegon's internal ownership of
`core/crates/omegon-extension`. The Omegon host now consumes the published
`omegon-extension = "0.25"` crate, so keeping the internal source creates drift
and contributor confusion.

## Preconditions already satisfied

- `omegon-extension-rs` exists as the canonical standalone Rust SDK source.
- `omegon-extension v0.25.0` is published on crates.io.
- The standalone SDK passes protocol smoke, contract drift, package, and publish dry-run validation.
- Python and TypeScript SDKs carry the same `sdk-contract.json` artifact and expose matching contract constants.
- Omegon host depends on `omegon-extension = "0.25"` from crates.io.
- Bundled `omegon-browser` depends on `omegon-extension = "0.25"` and reports `SDK_CONTRACT_VERSION`.

## Cutover tasks

### 1. Host compatibility audit

Search active host code for SDK version handling:

```bash
grep -R "sdk_version\|SDK_CONTRACT_VERSION\|check_sdk_version\|VersionMismatch" -n \
  core extensions \
  --exclude-dir=target
```

Required result:

- Any install/runtime compatibility check treats `extension_info.sdk_version` / manifest `sdk_version` as `SDK_CONTRACT_VERSION`, not as the Rust crate SemVer or Omegon host version.
- If host-side compatibility policy is missing, add a small explicit helper that accepts contract `0.24` for this milestone and rejects unknown newer contracts.

### 2. Delete internal SDK source

Remove:

```text
core/crates/omegon-extension/
```

This source is now owned by `omegon-extension-rs`.

### 3. Path-reference sweep

Run:

```bash
grep -R "core/crates/omegon-extension\|../omegon-extension\|../../core/crates/omegon-extension" -n . \
  --exclude-dir=.git \
  --exclude-dir=target
```

Allowed remaining matches:

- historical design notes that explicitly describe the old pre-extraction path;
- archived OpenSpec history where changing the text would erase historical context.

Forbidden remaining matches:

- active `Cargo.toml` dependency paths;
- prompt/context docs that tell contributors to edit the old path;
- build/install scripts;
- examples or manifests.

### 4. Validation

Required:

```bash
cargo check -p omegon --manifest-path Cargo.toml
cargo check --manifest-path extensions/omegon-browser/Cargo.toml
```

Preferred if runtime tests are available and not too broad:

```bash
cargo test -p omegon --manifest-path Cargo.toml
```

### 5. Commit

Commit message:

```text
refactor(extension-sdk): remove internal Rust SDK source
```

## Non-blocking follow-ups

- Migrate or explicitly waive stale downstream consumers:
  - `aether` currently had evidence of hard-coded `sdk_version = "0.16.0"`.
  - `lipstyk` currently had evidence of old `omegon-extension v0.15.26`.
- Add packaging validation for Python SDK wheels.
- Add TypeScript `dist/` drift CI.
- Decide whether `sdk-contract.json` remains canonical or whether Pkl becomes the generated source of truth.
