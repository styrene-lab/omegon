---
id: nex-deterministic-substrate-boundary
title: "Nex Deterministic Substrate Boundary"
status: exploring
tags: [nex, substrate, tooling, secrets, capabilities, architecture]
open_questions:
  - "Which existing Omegon host checks should move behind Nex first: release toolchain checks, package install HostActions, secrets-pack preflight, `nex_capability`, or provider/tool availability probes?"
  - "What structured contract should Nex return to Omegon: capability status only, remediation plans, exact versions/sources, export plans for secrets, or provenance/attestation data?"
  - "How should Omegon degrade when Nex is unavailable: direct local checks, warning-only mode, fail-fast for release/headless workflows, or install prompt?"
  - "Which policy decisions must remain in Omegon even if Nex can resolve/provision the substrate: approvals, scope enforcement, changelog/version release policy, delegation grants, and runtime lifecycle transitions?"
dependencies: []
related:
  - repo-agent-runtime-profile
  - deterministic-devenv-workflow-assessment
  - nex-substrate-tool-boundary
---

# Nex Deterministic Substrate Boundary

## Overview

Define the boundary between Nex and Omegon for deterministic host substrate management: toolchains, install plans, secrets-pack validation, environment profiles, and host capability checks should live in Nex, while Omegon retains runtime policy, lifecycle orchestration, approvals, and agent behavior.

## Decisions

### Nex owns capability/provisioning facts, not agent authorization decisions

**Status:** candidate

**Rationale:** Nex should report whether a host can satisfy a requirement and how to remediate it. Omegon should decide whether an agent action is allowed, whether a credential may be granted, and whether a workflow should proceed.

## Open Questions

- Which existing Omegon host checks should move behind Nex first: release toolchain checks, package install HostActions, secrets-pack preflight, `nex_capability`, or provider/tool availability probes?
- What structured contract should Nex return to Omegon: capability status only, remediation plans, exact versions/sources, export plans for secrets, or provenance/attestation data?
- How should Omegon degrade when Nex is unavailable: direct local checks, warning-only mode, fail-fast for release/headless workflows, or install prompt?
- Which policy decisions must remain in Omegon even if Nex can resolve/provision the substrate: approvals, scope enforcement, changelog/version release policy, delegation grants, and runtime lifecycle transitions?

## Research: Nex ownership evidence from sister project

Nex already has design and code that point to the right ownership split for deterministic substrate work:

- `docs/nex-devenv-parallels.md` frames Nex as borrowing devenv's module/options/tasks/info/test/explain patterns for machine and host profiles, while explicitly not becoming a generic project dev-environment manager.
- `docs/nex-secretspec-integration.md` adopts the SecretSpec separation of WHAT/HOW/WHERE for secrets and states that profiles should declare secret contracts, not secret values.
- `src/devenv_import.rs` already emits `io.styrene.nex.devenv-import-report.v1`, detects `secretspec.toml`, hashes source files, and classifies discovered items with safety tags such as `secret-contract`, `secret-value-runtime`, `arbitrary-command`, and mutation categories.
- `src/machine_profile.rs` already has `MachineProfileSecrets { required, optional }` and validates names as uppercase environment-style identifiers, which matches an early secrets-pack name contract.
- `docs/nex-cli-command-surface-map.md` defines planned `nex secrets list/check/generate/run` commands and stable outputs such as `io.styrene.nex.secrets-contract.v1` and `io.styrene.nex.secrets-check-report.v1`.

This means Omegon should avoid owning advanced substrate parsing and SecretSpec/devenv import semantics. Omegon should consume Nex reports and make runtime/policy decisions from them.

## Boundary implication

Nex should own:

- discovering devenv/secretspec inputs;
- parsing and classifying substrate facts;
- validating host capability and resolver availability;
- producing stable report schemas;
- offering remediation/provisioning plans;
- generating no-secret-value summaries for UI/agent consumption.

Omegon should own:

- whether a workflow is allowed to proceed;
- whether a secret/capability can be granted to a session, child agent, or A2A peer;
- approval routing and audit logs;
- lifecycle enforcement (`seed -> exploring -> decided -> implementing -> implemented`);
- repo agent runtime profile interpretation;
- conversion of Nex facts into agent prompt/tool/runtime policy.

## Next implementation slice

Add an Omegon-side read-only Nex substrate ingestion surface before enforcing behavior. The concrete boundary is designed in [[nex-substrate-tool-boundary|Omegon Nex Substrate Tool Boundary]] and should be a separate `nex_substrate` tool rather than another `nex_capability` action.

First slice command mapping:

```text
nex_substrate { action: "inspect", path: ".", mode: "devenv" }
```

Under the hood Omegon should call:

```text
nex devenv inspect <path> --json
```

The first slice should only ingest and display/report Nex facts. It should not yet mutate the runtime, install packages, enforce policy, or grant secrets.
