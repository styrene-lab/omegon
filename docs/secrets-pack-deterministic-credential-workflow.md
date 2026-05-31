---
id: secrets-pack-deterministic-credential-workflow
title: "Secrets-Pack Deterministic Credential Workflow"
status: exploring
tags: [secrets, nex, devenv, headless, a2a, security]
open_questions:
  - "[assumption] secrets-pack should be declarative policy only, never encrypted bulk value storage, so per-secret durable storage and least-privilege rotation remain intact."
  - "Which path/schema should define packs: `.omegon/secrets-pack.toml`, `.nex/secrets-pack.toml`, Pkl schema in `pkl/SecretsPack.pkl`, or profile-embedded pack references?"
  - "How should resolved secrets be transported to child/local/remote agents: no forwarding, explicit env allowlist, ephemeral file descriptor, local IPC, scoped token minting, or A2A grant envelope?"
  - "What should Nex validate versus Omegon enforce: availability and resolver health in Nex; grant/export authorization and runtime redaction in Omegon?"
dependencies: []
related:
  - session-secret-cache-preflight
  - repo-agent-runtime-profile
  - nex-deterministic-substrate-boundary
  - nex-substrate-tool-boundary
---

# Secrets-Pack Deterministic Credential Workflow

## Overview

Define secrets-pack as the credential analogue to devenv/Nex environment profiles: a declarative, commit-safe contract for required and optional secrets, resolution policy, export/scoping rules, headless behavior, and child/A2A grant semantics. Values remain in keychain, Vault, env, cmd resolvers, CI stores, or Styrene wallet; the pack declares policy and shape only.

## Decisions

### Secrets-pack declares credential contract; Nex resolves availability; Omegon enforces grants

**Status:** candidate

**Rationale:** This mirrors the larger substrate split. Nex can say whether required credentials are available and how they can be resolved. Omegon owns whether a runtime/session/subagent is authorized to receive or use a secret.

### Headless/autonomous workflows must preflight packs and fail before work begins

**Status:** candidate

**Rationale:** Surprise credential prompts during cleave, CI, release, or remote A2A execution break deterministic autonomy. Interactive sessions may prompt at startup; headless sessions must inherit scoped resolved grants or fail fast.

## Open Questions

- [assumption] secrets-pack should be declarative policy only, never encrypted bulk value storage, so per-secret durable storage and least-privilege rotation remain intact.
- Which path/schema should define packs: `.omegon/secrets-pack.toml`, `.nex/secrets-pack.toml`, Pkl schema in `pkl/SecretsPack.pkl`, or profile-embedded pack references?
- How should resolved secrets be transported to child/local/remote agents: no forwarding, explicit env allowlist, ephemeral file descriptor, local IPC, scoped token minting, or A2A grant envelope?
- What should Nex validate versus Omegon enforce: availability and resolver health in Nex; grant/export authorization and runtime redaction in Omegon?

## Research: Nex SecretSpec parity

Nex has already started adopting SecretSpec-style semantics in design and code:

- `docs/nex-secretspec-integration.md` names the core split as WHAT/HOW/WHERE and explicitly says profiles declare secret contracts, not values.
- Nex design favors `secretspec run -- command`-style runtime injection and warns against global shell export.
- Nex's planned command surface includes `nex secrets list`, `nex secrets check`, `nex secrets generate`, and `nex secrets run`.
- `src/devenv_import.rs` already detects `secretspec.toml` and emits `SecretContract` items into the devenv import report.
- `src/machine_profile.rs` already supports `required` and `optional` secret names for machine profiles.

These facts support treating Nex as the owner of secrets-pack availability and resolver-health checks. Omegon should not duplicate SecretSpec parsing unless Nex is unavailable and a minimal fallback is required.

## Ownership refinement

Secrets-pack should be a shared contract surface, but ownership should split this way:

- Nex: parse/import/check/generate/run substrate secrets; report missing or unhealthy providers; avoid Nix-store and global-env leakage.
- Omegon: decide grant policy, session cache policy, child/A2A forwarding, approval requirements, and redaction enforcement.

For headless work, Omegon should ask Nex for a no-values check report before work begins, then either fail fast or request an explicit operator grant.
