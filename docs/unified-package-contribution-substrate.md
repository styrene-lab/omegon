+++
title = "Unified Package Contribution Substrate"
tags = ["packages","armory","skills","extensions","flynt","acp","architecture"]
+++

+++
title = "Unified Package Contribution Substrate"
tags = ["packages","armory","skills","extensions","acp","architecture"]
+++

# Unified Package Contribution Substrate

+++
id = "a7db3ce4-4835-4509-9f2c-eb61378eb6b4"
kind = "design_node"

[data]
title = "Unified Package Contribution Substrate"
status = "exploring"
issue_type = "architecture"
priority = 1
dependencies = []
open_questions = [
  "What exact ACP schemas should packages/install, packages/plan, packages/search, packages/list, packages/remove, and packages/update expose in v1?",
  "Should v1 require pre-install package inspection and approval, or allow install-first adapters for legacy CLI parity?",
  "What is the minimum contribution report required to support uninstall/update when one package materializes multiple contributions?",
  "What should the unified manifest be called: omegon.package.toml, package.toml, or an extension of plugin.toml?",
  "How should contribution risk classes be represented for prompt-only skills, scripts, MCP servers, SDK extensions, and HostAction policy?"
]
+++

## Overview

Omegon needs a composable ACP package substrate. The current codebase exposes historical implementation buckets as public install concepts: skills, legacy plugins, SDK extensions, catalog agents, and Armory entries. That is not a durable ACP model.

The stable domain model should be:

```text
Package
  ├── identity / source / provenance
  ├── install plan / approval metadata
  └── contributions[]
        ├── skill
        ├── sdk_extension
        ├── agent
        ├── persona
        ├── tone
        ├── script
        ├── mcp_server
        └── host_action_policy
```

A package is the acquisition, provenance, install, update, and uninstall unit. Contributions are the materialized runtime units.

This design is ACP-first and client-neutral. Flynt is only one consumer; the ACP substrate should be correct for any host/client.

## Current Codebase Anchors

- `core/crates/omegon/src/acp.rs` exposes ACP `ext_method` surfaces.
- `core/crates/omegon/src/armory.rs` owns registry-style browse/install for Armory entries.
- `core/crates/omegon/src/plugin_cli.rs` owns Git/local install of legacy `plugin.toml` packages.
- `core/crates/omegon/src/extension_cli.rs` owns install of SDK extensions with `manifest.toml`.
- `core/crates/omegon/src/skills.rs` owns installed skill materialization and plain skill management.
- `core/crates/omegon/src/catalog.rs` owns bundled/catalog agent install/list.
- `package-install-hostaction` frames host-mediated install approval, but this node owns the product/domain package substrate behind ACP and HostActions.

## Decisions

### Decision: ACP exposes package/contribution substrate, not client-specific aliases

Status: decided

ACP is the stable integration boundary. It should model the domain as packages that materialize contributions, rather than adding page- or client-specific methods such as `skills/plugin_install` as primary APIs.

### Decision: `packages/install` is the canonical ACP install entrypoint

Status: decided

A single package install entrypoint lets ACP clients install registry, Git, local, and future package sources while receiving a normalized package/contribution report. Existing `skills/*`, `extensions/*`, `armory/*`, and `catalog/*` surfaces can remain compatibility adapters.

### Decision: Package is the user-facing install unit

Status: decided

Omegon should not expose `plugin` as a primary product concept. A plugin is a legacy package manifest format. The user-facing concept is `package`, with one or more contribution types.

### Decision: SDK extension remains a contribution type, not the umbrella

Status: decided

Do not rename everything to extension. SDK extensions have stronger runtime/security semantics than prompt-only skills or passive package metadata. Treat SDK extension as one contribution kind inside a package.

### Decision: `plugin.toml` becomes a legacy package adapter

Status: decided

Existing `plugin.toml` Armory packages continue to work, but the new substrate treats them as one package format adapter. `plugin.toml` with `type = "skill"` contributes skill material, not an SDK extension.

### Decision: Package planning precedes long-term package mutation

Status: proposed

The correct substrate should support `packages/plan` before mutation so hosts can display provenance, contribution types, risk classes, files touched, and activation consequences. First patch slices may use existing installers, but the architecture should converge on plan-before-install.

## Canonical ACP Surface

### `packages/plan`

Input:

```json
{
  "source": "https://github.com/org/package",
  "kind_hint": "auto"
}
```

Output:

```json
{
  "ok": true,
  "package": {
    "id": "package",
    "source": "https://github.com/org/package",
    "source_kind": "git"
  },
  "contributions": [
    {
      "kind": "skill",
      "id": "example",
      "risk": "prompt_influence",
      "action": "install"
    }
  ],
  "warnings": []
}
```

### `packages/install`

Input:

```json
{
  "source": "https://github.com/org/package",
  "kind_hint": "auto",
  "approval_token": null
}
```

Output:

```json
{
  "ok": true,
  "package": {
    "id": "package",
    "source": "https://github.com/org/package"
  },
  "contributions": [
    {
      "kind": "skill",
      "id": "example",
      "path": "/home/user/.omegon/skills/example/SKILL.md",
      "status": "installed"
    }
  ],
  "warnings": []
}
```

### `packages/search`

Searches registries such as Armory, returning packages and contribution summaries rather than separate item kinds.

### `packages/list`

Lists installed packages, including their materialized contributions and source/provenance metadata.

### `packages/remove`

Removes an installed package and its recorded contributions where safe. If records are incomplete, returns a plan/warning rather than guessing.

### `packages/update`

Updates by package id/source and reports changed contributions.

## Internal Module Direction

Create a package substrate module:

```text
core/crates/omegon/src/packages/
  mod.rs
  detect.rs
  install.rs
  manifest.rs
  plan.rs
  report.rs
  source.rs
  adapters/
    armory.rs
    extension.rs
    legacy_plugin.rs
    skill.rs
    catalog.rs
```

Initial adapters should call existing code rather than rewrite everything:

- Git/local `plugin.toml` package → `plugin_cli::install`
- SDK extension package → `extension_cli::install`
- Armory registry target → `armory::install`
- bundled/catalog agents → `catalog` APIs
- existing skill install → `skills`/`armory` APIs

## Armory Relationship

Armory should remain central, but its role should shift from a parallel install surface to the canonical package registry/index.

Current Armory behavior mixes registry, package format, and installer routing. The unified substrate should separate those concerns:

```text
Armory
  = registry / index / discovery plane

Package substrate
  = source resolution / detection / planning / installation / contribution records

Contribution stores
  = skills, sdk extensions, agents, personas, tones, scripts, MCP servers
```

In the target model, Armory search returns packages with contribution summaries, not separate product buckets that compete with ACP methods:

```json
{
  "items": [
    {
      "id": "recro-omegon",
      "kind": "package",
      "source": "https://github.com/recro/recro-omegon",
      "contributions": ["skill"],
      "description": "Recro workflows for Omegon"
    }
  ]
}
```

`packages/search` should be able to query Armory by default and filter by contribution type:

```json
{
  "registry": "armory",
  "query": "recro",
  "contributes": ["skill"]
}
```

`packages/install` should accept Armory package IDs as one source kind:

```json
{
  "source": "armory:recro-omegon",
  "kind_hint": "auto"
}
```

Compatibility methods such as `armory/search` and `armory/install` can remain, but they should become adapters over `packages/search` and `packages/install`, not independent logic paths.

Armory also needs schema evolution: instead of indexing only historical roots such as `skills/`, `personas/`, `tones/`, and extension registries, it should index packages and their contribution manifests/provenance. Legacy Armory entries can be projected into this model.

## Package Detection

Detection should inspect source material after checkout/copy:

| Marker | Meaning |
|---|---|
| `omegon.package.toml` | future unified package manifest |
| `manifest.toml` | SDK extension contribution/package |
| `plugin.toml` | legacy package manifest adapter |
| `plugin.toml` + `type = "skill"` | skill contribution |
| `SKILL.md` | standalone skill contribution |
| catalog agent manifest | agent contribution |

Armory registry records should optionally predeclare the same detected contribution summary so hosts can show package risk/contents before checkout.

## Contribution Risk Classes

Contribution reports should classify risk so hosts can request appropriate approval:

| Contribution | Risk |
|---|---|
| skill/persona/tone | `prompt_influence` |
| script | `local_code_execution` |
| MCP server | `persistent_tool_execution` |
| SDK extension | `extension_runtime` |
| host action policy | `host_capability_request` |

## Relationship to `package.install@1` HostAction

The ACP package substrate owns package detection, planning, installation reports, and contribution records. `package.install@1` is a host-mediated approval/mutation primitive that should eventually call this substrate for dry-run and execution.

## Migration Strategy

1. Add `packages/plan` and `packages/install` in ACP as the canonical API.
2. Keep existing `extensions/install`, `skills/install`, `armory/install`, and catalog methods as compatibility adapters.
3. Route compatibility adapters through the packages substrate once the module exists.
4. Add package install records so uninstall/update can operate on packages instead of guessing from contribution stores.
5. Introduce `omegon.package.toml` for composite packages, while preserving `plugin.toml`, `manifest.toml`, and `SKILL.md` adapters.

## Non-goals

- Do not make SDK extension the umbrella term for all packages.
- Do not expose legacy `plugin` as the main product/API concept.
- Do not add client-specific ACP aliases as the primary path.
- Do not require all existing packages to migrate to a unified manifest before the substrate ships.
