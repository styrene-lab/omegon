---
id: nex-substrate-tool-boundary
title: "Omegon Nex Substrate Tool Boundary"
status: exploring
tags: [nex, tools, substrate, devenv, secrets, policy]
open_questions:
  - "Should the public agent tool be named `nex_substrate`, `substrate_inspect`, or grouped under a future `runtime_profile` tool surface?"
  - "Should first-slice missing Nex behavior be a structured report with `nex_available=false`, or should headless/release modes fail the tool call outright?"
  - "Which policy findings become blockers in v1: `requires_review`, `unsupported`, `secret-value-runtime`, privileged/destructive safety tags, or only explicit release/headless mode checks?"
  - "Should Omegon call the external `nex` binary, an embedded library crate, or both with binary-first fallback-to-library later?"
related:
  - nex-deterministic-substrate-boundary
  - repo-agent-runtime-profile
  - secrets-pack-deterministic-credential-workflow
  - deterministic-devenv-workflow-assessment
---

# Omegon Nex Substrate Tool Boundary

## Overview

Define the first Omegon-side boundary for consuming Nex deterministic-substrate facts. The boundary is read-only and advisory: Omegon asks Nex to inspect project substrate inputs such as `devenv.nix`, `devenv.yaml`, `.envrc`, and `secretspec.toml`; Nex returns stable fact reports; Omegon overlays runtime policy interpretation without provisioning tools, exporting secrets, mutating profiles, or granting access.

This is deliberately separate from `nex_capability`. `nex_capability` answers "is this one named capability available or resolvable?" The new substrate boundary answers "what deterministic environment and credential contracts does this repository declare, and what should Omegon care about before running agents?"

## Design goals

- Consume Nex as the source of truth for substrate discovery, classification, and report schemas.
- Keep the first Omegon slice read-only and non-enforcing except for explicit tool-call error handling.
- Preserve raw Nex reports in machine-readable `details` so future UI/release/headless policies can build on the same evidence.
- Produce a small Omegon policy overlay that identifies findings relevant to agent autonomy, release preflight, headless execution, and secret grants.
- Degrade predictably when Nex is absent.

## Non-goals

- Do not parse `devenv.nix`, `devenv.yaml`, or `secretspec.toml` in Omegon.
- Do not install Nex, Nix, devenv, packages, extensions, or secrets providers.
- Do not inject secrets into agent processes.
- Do not treat Nex reports as authorization grants.
- Do not replace the existing `nex_capability` single-capability resolver.

## Public tool surface

Add a new read-only agent tool:

```text
nex_substrate
```

Initial schema:

```json
{
  "type": "object",
  "properties": {
    "action": {
      "type": "string",
      "enum": ["inspect"],
      "description": "Read-only substrate inspection action"
    },
    "path": {
      "type": "string",
      "description": "Project directory to inspect; defaults to the current workspace root"
    },
    "mode": {
      "type": "string",
      "enum": ["devenv"],
      "description": "Substrate report family to request from Nex; defaults to devenv for the first slice"
    }
  },
  "required": ["action"]
}
```

`mode` is intentionally narrow in the first slice. Future modes can include `profile`, `secrets`, `hardware`, or `all` once Nex has stable command/report contracts for those surfaces.

## Nex command mapping

First slice command:

```text
nex devenv inspect <path> --json
```

Expected Nex schema:

```text
io.styrene.nex.devenv-import-report.v1
```

Omegon must treat this report as an external fact contract. If the schema is unknown or absent, Omegon should still return the raw output under diagnostics but should mark policy confidence as degraded.

## Omegon report contract

Omegon returns a wrapper report:

```json
{
  "schema": "io.styrene.omegon.nex-substrate-report.v1",
  "source": "nex",
  "nex_available": true,
  "path": "/workspace/project",
  "mode": "devenv",
  "reports": {
    "devenv_import": {
      "schema": "io.styrene.nex.devenv-import-report.v1"
    }
  },
  "policy": {
    "summary": {
      "blockers": 0,
      "warnings": 2,
      "review_items": 1,
      "secret_contracts": 2
    },
    "findings": []
  },
  "diagnostics": []
}
```

When Nex is unavailable:

```json
{
  "schema": "io.styrene.omegon.nex-substrate-report.v1",
  "source": "nex",
  "nex_available": false,
  "path": "/workspace/project",
  "mode": "devenv",
  "reports": {},
  "policy": {
    "summary": {
      "blockers": 0,
      "warnings": 1,
      "review_items": 0,
      "secret_contracts": 0
    },
    "findings": [
      {
        "severity": "warning",
        "code": "nex_unavailable",
        "message": "Nex is not available on PATH; substrate inspection was skipped.",
        "source": null
      }
    ]
  },
  "diagnostics": ["install or expose `nex` to enable deterministic substrate inspection"]
}
```

For the initial interactive agent tool, missing Nex should be a structured warning, not a hard error. Release/headless preflight can later promote `nex_unavailable` to a blocker when a repo profile requires Nex substrate verification.

## Policy overlay

Omegon policy findings are derived from Nex facts. They do not alter the Nex report.

Finding shape:

```json
{
  "severity": "info|warning|blocker",
  "code": "requires_review|unsupported_substrate|secret_contract|secret_runtime_value|privileged_mutation|destructive_mutation|arbitrary_command|nex_unavailable|schema_unknown",
  "message": "human-readable summary",
  "source": {
    "report": "devenv_import",
    "item_id": "devenv.nix:enterShell",
    "file": "devenv.nix",
    "path": "enterShell"
  }
}
```

Initial mapping from Nex `io.styrene.nex.devenv-import-report.v1`:

| Nex evidence | Omegon finding | Initial severity | Why Omegon cares |
|---|---|---:|---|
| `item.review.required=true` | `requires_review` | warning | Agent/autonomous workflows should not silently accept migration-risk items. |
| `bucket=unsupported` | `unsupported_substrate` | warning | Determinism may be incomplete. |
| `kind=secret-contract` or safety `secret-contract` | `secret_contract` | info | Secret names/contracts affect preflight and grants but are not values. |
| safety `secret-value-runtime` | `secret_runtime_value` | warning | Runtime value flow must be scoped/redacted. |
| safety `arbitrary-command` | `arbitrary_command` | warning | Shell hooks/tasks can surprise autonomous agents. |
| safety `privileged-mutation` or `system-config-mutation` | `privileged_mutation` | warning | Needs explicit approval before any apply/provision path. |
| safety `destructive-disk-operation` or `hardware-driver-mutation` | `destructive_mutation` | blocker in headless/release, warning interactively | These are never safe to infer silently. |
| unknown Nex schema | `schema_unknown` | warning | Preserve raw report but reduce confidence. |

The first slice should not know whether the current prompt is release/headless. It should expose enough structured findings for later runtime policy to make that decision.

## Runtime behavior

1. Resolve `path` against the workspace boundary using existing path safety helpers where possible.
2. Locate `nex` via PATH.
3. If missing, return `nex_available=false` report.
4. Run `nex devenv inspect <path> --json` with bounded timeout and captured stdout/stderr.
5. Parse stdout as JSON.
6. Preserve the raw Nex JSON under `reports.devenv_import`.
7. Derive policy findings from `items`, `summary`, and `schema`.
8. Return a concise text summary plus full JSON details.

Process-spawn constraints:

- Use argument arrays, not shell interpolation.
- Do not inherit stdio.
- Bound execution time.
- Treat stderr as diagnostics, not as report data, unless JSON parsing fails.

## Text summary format

The tool's visible text should be compact:

```text
Nex substrate inspection: available
Path: /workspace/project
Report: io.styrene.nex.devenv-import-report.v1
Items: portable=5 project=1 machine=1 review=1 unsupported=0
Policy: 0 blockers, 3 warnings, 2 secret contracts
Warnings:
- requires_review: devenv.nix:enterShell requires operator review before migration
- arbitrary_command: devenv.nix:enterShell contains arbitrary command surface
```

If Nex is missing:

```text
Nex substrate inspection: unavailable
Path: /workspace/project
Policy: warning nex_unavailable — install or expose `nex` to enable deterministic substrate inspection
```

## Ownership decisions

### New tool boundary: `nex_substrate`, not `nex_capability`

**Status:** candidate

**Rationale:** Capability resolution and substrate inspection have different shapes. A capability check is a single-key availability query; substrate inspection is a project-wide report with nested evidence and policy findings. Keeping them separate avoids schema ambiguity and keeps future enforcement clearer.

### Binary-first integration for first slice

**Status:** candidate

**Rationale:** The sister project already exposes `nex devenv inspect --json`. Calling the binary keeps ownership in Nex, avoids vendoring unstable internals into Omegon, and exercises the same CLI/API boundary that UI and operators will use. A library crate can be considered later when Nex stabilizes a Rust API.

### Missing Nex is warning-only for interactive inspection

**Status:** candidate

**Rationale:** The first tool is exploratory/read-only. Failing hard when Nex is absent would make the tool brittle before repo profiles can declare that Nex substrate verification is required. Headless/release workflows can later promote this to a blocker through repo runtime policy.

### Omegon policy overlay must be derived and auditable

**Status:** candidate

**Rationale:** Omegon should never silently reinterpret substrate facts. The wrapper report keeps raw Nex evidence and links every policy finding back to report/item/source evidence.

## First implementation tasks

1. Add `core/crates/omegon/src/nex/substrate.rs` with wrapper report structs and policy mapping helpers.
2. Add `nex_substrate` to `tool_registry.rs` and `CoreTools::definitions()` as a read-only repo-inspection tool.
3. Implement `CoreTools::execute()` branch for `nex_substrate` using `nex devenv inspect <path> --json`.
4. Add unit tests for policy mapping using synthetic Nex import reports.
5. Add a missing-Nex test around the command runner boundary if command execution is injectable; otherwise keep first tests pure and cover execution with focused manual validation.
6. Update `CHANGELOG.md` `[Unreleased]` when behavior lands.

## Later slices

- Add repo runtime profile field that requires Nex substrate verification.
- Consume future `nex secrets check --json` once available.
- Promote selected findings to blockers in headless/release mode.
- Surface findings in session preflight and release preflight.
- Add A2A/child-agent grant policy derived from secret contracts.
