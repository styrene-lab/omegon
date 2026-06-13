+++
id = "release-0-27-0-workstream-auth-integrity"
kind = "document"
title = "0.27.0 workstream — auth/store integrity and provider-route correctness"
status = "exploring"
tags = ["release", "0.27.0", "workstream", "auth", "provider-routing", "hardening"]
aliases = ["0.27 auth integrity", "auth store integrity workstream"]
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# 0.27.0 workstream — auth/store integrity and provider-route correctness

## Owner

Primary owner: **omegon-quartus**.

## Branch

`release/0.27-auth-integrity`

Branch created from current `main` HEAD: `07027739 fix(auth): prefer refreshable oauth credentials`.

## Mission

Make provider authentication and provider-route state reliable enough for the 0.27.0 release. The main release risk is not lack of features; it is an operator being forced to re-login or seeing route/model status that does not match actual credential and bridge state.

## Inputs

- [[release-0.27.0-exploration|0.27.0 release exploration]]
- `core/crates/omegon/src/auth.rs`
- `core/crates/omegon/src/setup.rs`
- `core/crates/omegon/src/route.rs`
- `core/crates/omegon/src/providers.rs`
- `core/crates/omegon/src/main.rs`
- auth/route tests in `auth.rs`, `setup.rs`, `route.rs`, `main.rs`

## Current findings

Observed bug:

- Startup selected `openai-codex:gpt-5.5`.
- `auth.json` existed but did not contain the `openai-codex` provider entry at startup.
- `/login openai-codex` restored the entry and route resolution immediately worked.
- External Codex CLI auth did not exist, so fallback import could not rescue the missing entry.

Current hardening already applied in the active working tree:

- Auth write paths trace provider key-set deltas.
- Auth writes refuse to replace an unparsable existing auth store with a partial credential file.
- Regression tests cover preservation of existing `openai-codex` credentials while writing another provider and refusal to overwrite malformed auth JSON.

Important candidate root cause now addressed:

- Previous auth update code used `serde_json::from_str(...).unwrap_or(json!({}))` in multiple write paths. A malformed/transiently unreadable store could therefore become `{}` plus the provider currently being written, dropping unrelated credentials such as `openai-codex`.

## Scope

### In scope

- Finish and validate auth-store integrity hardening.
- Verify all auth write paths preserve unrelated provider entries.
- Add concurrent write preservation coverage if feasible.
- Confirm `openai-codex` accountId survives refresh/login/adoption paths.
- Confirm startup env hydration reads valid stored Codex auth and does not require `/login` after relaunch.
- Confirm route state cannot claim connected when credential ledger reports missing/expired.
- Confirm `/login`, `/logout`, `/model`, and model-tier/offline switches route through `RouteController` where applicable.

### Out of scope

- UI copy/layout polish except where needed for correct route truthfulness.
- Release script and changelog mechanics.
- New provider integrations.
- Full auth backup/restore workflow unless required to close a release blocker.

## Acceptance criteria

- Existing `openai-codex` credentials cannot be dropped by normal auth writes for another provider.
- Malformed existing `auth.json` is not replaced by a partial provider store.
- Focused auth tests pass.
- Relaunch with stored `openai-codex` credentials reaches connected route state without `/login`.
- `/auth status` reports `openai-codex` authenticated when the stored entry exists and is valid.
- If the selected provider is missing/expired, startup route state is disconnected or explicit fallback — never silent fallback.
- Findings and any remaining risks are recorded in [[release-0.27.0-exploration]].

## Suggested task breakdown

1. Review current auth diff in `core/crates/omegon/src/auth.rs`.
2. Run focused tests:
   - `cargo test -p omegon credential_write_refuses_to_replace_unparsable_auth_json`
   - `cargo test -p omegon writing_one_provider_preserves_existing_codex_credentials`
   - `cargo test -p omegon refreshed_codex_entry_preserves_existing_account_id`
3. Add any missing tests for concurrent writes or adoption/refresh preservation if low risk.
4. Run `just link` from the patched branch.
5. Relaunch with `openai-codex:gpt-5.5`; confirm no `/login` required.
6. Inspect `~/.config/omegon/omegon.log` for auth key-delta traces and startup route resolution.
7. Commit auth integrity patch.

## Risks

- Logging provider key names is acceptable; never log credential values.
- Backups/repair are tempting but may be too large for this release branch. Trace + parse-refusal may be the right 0.27.0-sized fix.
- Route correctness spans multiple files. Avoid duplicating route logic outside `RouteController`; test invariants instead.

## Coordination notes

- Coordinate with `release/0.27-ui-polish` for operator-facing wording around missing credentials and route remediation.
- Coordinate with `release/0.27-mechanics-validation` before final validation because this workstream owns the highest-risk release bug.
