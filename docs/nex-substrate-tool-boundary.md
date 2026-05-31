---
id: nex-substrate-tool-boundary
title: "Omegon Nex Substrate Tool Boundary"
status: exploring
tags: [nex, tools, substrate, devenv, secrets, policy]
open_questions:
  - "Should the public agent tool be named `nex_substrate`, `substrate_inspect`, or grouped under a future `runtime_profile` tool surface?"
  - "Should first-slice missing Nex behavior be a structured report with `nex_available=false`, or should headless/release modes fail the tool call outright?"
  - "Which policy findings become blockers in v1: `requires_review`, `unsupported`, `secret-value-runtime`, privileged/destructive safety tags, or only explicit release/headless mode checks?"
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

- Consume Nex as the source of truth for substrate discovery, classification, and report schemas when a Nex provider is explicitly available.
- Preserve Omegon's single-binary functional default: core startup, normal chat/coding, and default validation workflows must work without Nex, devenv, Nix, or `omegon-nex` installed.
- Keep the first Omegon slice read-only and non-enforcing except for explicit tool-call error handling.
- Preserve raw Nex reports in machine-readable `details` so future UI/release/headless policies can build on the same evidence.
- Produce a small Omegon policy overlay that identifies findings relevant to agent autonomy, release preflight, headless execution, and secret grants.
- Degrade predictably when Nex is absent.

## Non-goals

- Do not create default-operation, startup, release, or build dependencies on Nex or Nix.
- Do not parse `devenv.nix`, `devenv.yaml`, or `secretspec.toml` in Omegon.
- Do not install Nex, Nix, devenv, packages, extensions, or secrets providers.
- Do not inject secrets into agent processes.
- Do not treat Nex reports as authorization grants.
- Do not replace the existing `nex_capability` single-capability resolver.

## Core dependency invariant

Omegon core must remain a single-binary functional runtime by default. A fresh operator must be able to install and run `omegon` without installing Nex, devenv, Nix, package managers beyond the host baseline, `omegon-nex`, MCP servers, ACP peers, or any optional provider. Optional integrations can improve determinism, provisioning, and substrate awareness, but they cannot become implicit prerequisites for startup, chat/coding operation, ordinary tool use, or default validation.

This implies a hard routing rule:

```text
If a capability requires an external dependency, it must be one of:

1. extension-owned / ACP-owned / MCP-owned provider behavior;
2. explicit user-invoked core tool with graceful degraded result;
3. repo/workflow-policy-gated requirement that fails only after explicit opt-in.
```

It must not be:

```text
core startup requirement
unconditional session preflight
unconditional release preflight
compile-time Rust dependency on Nex internals
silent background probe that changes default behavior
```

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

For the initial interactive agent tool, missing Nex should be a structured warning, not a hard error. Release/headless preflight can later promote `nex_unavailable` to a blocker only when a repo profile explicitly requires Nex substrate verification. Without that explicit policy, missing Nex remains advisory.

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

## Delegation/extension routing

The long-term owner for Nex CLI delegation actions is the `omegon-nex` extension, not core. Core may retain a tiny advisory fallback for explicit `nex_substrate` tool calls, but any workflow that requires Nex execution beyond optional inspection should go through an extension/ACP/MCP provider boundary.

Ownership split:

```text
omegon-nex extension
  - fixed Nex command wrappers
  - host-action install/apply plans
  - raw Nex report retrieval
  - provider health/degraded status

Omegon core
  - optional tool registration
  - policy overlay derivation
  - repo/runtime/release enforcement only after explicit policy opt-in
  - no hard dependency on Nex binaries, crates, or services
```

If `omegon-nex` is installed, core should prefer it for Nex operation delegation once an extension-to-core substrate provider path exists. If it is not installed, core must continue to operate normally and may return `nex_unavailable` for explicit substrate inspections.

## Runtime behavior

1. Resolve `path` against the workspace boundary using existing path safety helpers where possible.
2. Locate a Nex provider:
   - future preferred path: installed `omegon-nex` extension/ACP/MCP provider;
   - first-slice fallback: direct `nex` binary on PATH for explicit user-invoked inspection only.
3. If no provider is available, return `nex_available=false` report.
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

### Binary fallback is allowed only for explicit advisory inspection

**Status:** candidate

**Rationale:** Calling `nex devenv inspect --json` directly from core is acceptable as a narrow first-slice fallback because it is explicit, read-only, bounded, and advisory. It must not grow into a general core dependency on Nex. Any required or mutating Nex-backed workflow should be routed through the extension system or another provider boundary.

### Core remains single-binary functional by default

**Status:** candidate

**Rationale:** Omegon's core value proposition is a self-contained agent harness. Deterministic substrate integrations are valuable, but they cannot impose Nex/devenv/Nix/extension installation on operators who are using ordinary Omegon workflows. Missing optional providers are degraded capabilities unless a repo/workflow policy explicitly opts into requiring them.

### Missing Nex is warning-only for interactive inspection

**Status:** candidate

**Rationale:** The first tool is exploratory/read-only. Failing hard when Nex is absent would make the tool brittle before repo profiles can declare that Nex substrate verification is required. Headless/release workflows can later promote this to a blocker through repo runtime policy.

### Omegon policy overlay must be derived and auditable

**Status:** candidate

**Rationale:** Omegon should never silently reinterpret substrate facts. The wrapper report keeps raw Nex evidence and links every policy finding back to report/item/source evidence.

## First implementation tasks

1. Keep the current core `nex_substrate` implementation narrow: explicit `inspect`, `devenv` mode only, advisory report only.
2. Add tests/guards that missing Nex remains a degraded report and never affects startup/default operation.
3. Extend `omegon-nex` with `nex_devenv_inspect` as the preferred long-term Nex delegation provider.
4. Add an extension/provider bridge so core policy code can consume raw Nex reports from `omegon-nex` when installed.
5. Keep pure policy mapping helpers in core so runtime/release/headless policy does not live in the extension.
6. Update `CHANGELOG.md` when behavior changes.

## Later slices

- Add repo runtime profile field that requires Nex substrate verification.
- Consume future `nex secrets check --json` once available.
- Promote selected findings to blockers in headless/release mode, but only when repo/workflow policy explicitly requires Nex substrate verification.
- Surface findings in session preflight and release preflight.
- Add A2A/child-agent grant policy derived from secret contracts.
