+++
id = "f1cba9f2-a8e8-4794-8aa4-861f162c87df"
kind = "document"
title = "Provider API drift detection — daily live verification against reviewed expectations"
status = "implemented"
tags = ["ci", "providers", "api-drift", "upstream", "rust", "testing"]
aliases = ["provider-api-drift"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
open_questions = []
+++

# Provider API drift detection — daily live verification against reviewed expectations

## Overview

Omegon keeps runtime routing honest by separating **reviewed local expectations** from
**live upstream verification**.

The runtime consumes checked-in provider expectations. A dedicated daily GitHub Actions
workflow (`.github/workflows/provider-drift.yml`) exercises the opt-in
`core/crates/omegon/tests/live_upstream_smoke.rs` suite against limited-budget provider
credentials and treats failures as control-plane signals, not release blockers.

That split matters:

- release publication must stay available even when upstream providers are flaky;
- nightly builds should keep surfacing signal, but they should not become the only place drift is noticed;
- drift triage needs durable logs, a stable fingerprint, and deduplicated issue handling.

## Workflow design

The daily workflow runs on a schedule and on manual dispatch.

1. Check out the repo and install the Rust toolchain.
2. Run the live upstream smoke suite with `OMEGON_RUN_LIVE_UPSTREAM_TESTS=1`.
3. Capture the raw log as an artifact.
4. Convert the log into a deterministic JSON report via `scripts/provider_drift_issue.py`.
5. Upload both the raw log and the derived report.
6. Create or update a GitHub issue only when the report identifies true drift.

The helper script extracts failing test names and key failure snippets, then classifies the
failure shape as one of:

- `transient` — network/runtime/provider-outage style failures;
- `auth_or_quota` — invalid credentials, exhausted quota, billing disabled, or similar;
- `likely_drift` — failures that are neither transient nor credential-related and should be treated as probable contract/behavior drift.

It then computes a short fingerprint from that normalized failure shape. GitHub issue
behavior uses that fingerprint:

- same fingerprint → comment on the existing open drift issue;
- different fingerprint → close older open drift issues and create a new one;
- clean run, transient failure, or auth/quota failure → upload artifacts and summary, but do not open a drift issue.

## Secrets and budget

The workflow prefers dedicated low-budget secrets when present:

- `ANTHROPIC_DRIFT_API_KEY`
- `OPENAI_DRIFT_API_KEY`
- `OLLAMA_DRIFT_API_KEY`

It falls back to the standard provider secrets if the dedicated drift secrets are not yet
configured. The intended steady state is to provision separate drift-only credentials with
low spend ceilings and no production coupling.

Local Ollama verification is **disabled by default** in GitHub Actions. The daily workflow
checks hosted upstream providers plus Ollama Cloud; local Ollama smoke remains opt-in via
`OMEGON_RUN_OLLAMA_LOCAL_LIVE_TEST=1` on an environment that actually hosts an Ollama API.

## Non-blocking policy

The drift workflow is the canonical detector. Release and nightly stay non-blocking with
respect to provider drift:

- `release.yml` keeps the live smoke job `continue-on-error: true` and publishes even if the smoke job fails;
- `nightly.yml` no longer gates tag creation on live upstream smoke.

That is deliberate. Provider outages and upstream contract changes are operational signals,
not reasons to jam the binary release pipeline.

## Artifacts and triage

Each run uploads a `provider-drift-report` artifact containing:

- `live-upstream-smoke.log` — full raw cargo test output;
- `report.json` — normalized machine-readable summary;
- `step-summary.md` — the rendered human summary posted to the workflow summary / issue comments.

Triage starts with the artifact, then the failing tests, then the reviewed expectation
matrix or runtime implementation.

## Open Questions

*No open questions.*
