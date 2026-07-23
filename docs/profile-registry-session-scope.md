+++
title = "Profile Registry and Session Scope"
tags = ["design","profiles","startup"]
+++

+++
id = "08de408f-c5d7-4ce5-8362-65236c76b941"
kind = "design_node"

[data]
title = "Profile Registry and Session Scope"
status = "implementing"
issue_type = "architecture"
priority = 2
dependencies = []
open_questions = []
+++

## Overview

# Profile Registry and Session Scope

# Profile Registry and Session Scope

## Context

Omegon currently treats runtime profile loading as a singleton lookup: project `.omegon/profile.json` overrides user `~/.omegon/profile.json`, otherwise built-in defaults apply. That is workable for one operator profile per workspace, but it is not mature enough for many-agent-one-machine scenarios.

The desired model should mirror skills: profiles are discoverable portable artifacts with explicit scope, and startup resolves one active profile into a session plan instead of letting surfaces infer state from ambient files.

## Goals

- Provide a centralized registry for profile artifacts across bundled, user, and project scopes.
- Preserve compatibility with existing singleton profile files.
- Make active profile selection explicit and inspectable.
- Keep profile inventory separate from active session state.
- Support many installed agents/profiles on one machine without dormant entries leaking into session surfaces.

## Non-goals

- Replacing agent manifests. Agent bundles remain executable/persona/workflow catalog entries.
- Removing `.omegon/profile.json` compatibility in the first slice.
- Building a full multi-active-agent merge model. A session has at most one active profile and optionally one active agent.

## Profile scopes

1. **Bundled profiles**
   - Shipped read-only with Omegon.
   - Serve as defaults/templates.
   - Scope label: `bundled`.

2. **User profiles**
   - Stored under `~/.omegon/profiles/<id>.json` initially.
   - Operator-scoped, machine/user preference.
   - Scope label: `user`.

3. **Project profiles**
   - Stored under `<project-root>/.omegon/profiles/<id>.json` initially.
   - Portable with the repository when committed.
   - Scope label: `project`.

4. **Legacy singleton profiles**
   - Existing `<project-root>/.omegon/profile.json` and `~/.omegon/profile.json`.
   - Exposed as synthetic registry entries such as `project-default` and `user-default`.
   - Scope label: `project-legacy` / `user-legacy` or `project` / `user` plus `source_kind = LegacySingleton`.

## Registry data model

```rust
pub enum ProfileScope {
    Bundled,
    User,
    Project,
}

pub enum ProfileSourceKind {
    RegistryFile,
    LegacySingleton,
    BuiltInDefault,
}

pub struct ProfileRegistryEntry {
    pub id: String,
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub scope: ProfileScope,
    pub source_kind: ProfileSourceKind,
    pub path: Option<PathBuf>,
    pub profile: Profile,
    pub editable: bool,
    pub portable: bool,
    pub shadows: Vec<String>,
    pub validation: ProfileValidation,
}

pub struct ProfileSelection {
    pub id: String,
    pub scope: Option<ProfileScope>,
}

pub struct ResolvedProfile {
    pub entry: ProfileRegistryEntry,
    pub selection_source: ProfileSelectionSource,
}
```

## Selection files

Add explicit active-profile pointers:

- Project: `<project-root>/.omegon/active-profile.json`
- User: `~/.omegon/active-profile.json`

Shape:

```json
{
  "id": "operator-default",
  "scope": "user"
}
```

Selection precedence:

1. Explicit CLI/session selection, once available.
2. Project active-profile pointer.
3. User active-profile pointer.
4. Legacy project `.omegon/profile.json`.
5. Legacy user `~/.omegon/profile.json`.
6. Built-in default.

Listing all profiles is separate from selecting one.

## Startup contract

Startup should move toward:

```rust
ProfileRegistry::discover(cwd)
    -> ProfileRegistry
ProfileRegistry::resolve_selection(cwd, cli_profile)
    -> ResolvedProfile
ResolvedProfile::apply_to(Settings)
    -> Settings
SessionPlan::new(resolved_profile, active_agent, settings, ...)
```

The first slice can keep the existing `Settings` mutation model but replace `Profile::load_with_source(cwd)` inside `bootstrap::initialize_shared_settings` with registry-backed selection resolution.

## SessionPlan target

```rust
pub struct SessionPlan {
    pub mode: SessionMode,
    pub workspace: PathBuf,
    pub profile: ResolvedProfile,
    pub active_agent: Option<AgentBundleSummary>,
    pub settings: Settings,
    pub enabled_extensions: Vec<ExtensionCapabilitySummary>,
    pub secret_scope: SessionSecretScope,
    pub materialized_artifacts: Vec<MaterializedArtifact>,
}
```

Surfaces such as `/secrets`, `/profiles`, `/agents`, ACP startup, and web startup should read from this plan instead of re-deriving identity from global inventory.

## Portability rules

Portable profile fields:

- model intent / provider preference
- thinking level
- requested context class
- posture name
- tool detail
- extension policy by id/name
- integration enablement flags without machine secrets

Potentially non-portable fields:

- absolute trusted directories
- mount identities
- local extension paths
- machine-specific URLs
- secret names that imply local-only providers

Registry validation should mark non-portable fields and allow the operator to keep them intentionally.

## Relationship to skills

Profiles should mirror skill discovery semantics:

- bundled first as read-only defaults,
- user-installed profiles,
- project-local profiles that may shadow broader entries,
- explicit import/export/create commands,
- registry/list surface with scope badges and validation.

Unlike skills, profile activation must be singular for a session.

## Relationship to agents

Profiles are runtime/operator policy. Agents are executable/persona/workflow bundles.

A profile may specify a default agent id in the future, but active agent resolution should remain explicit in `SessionPlan`. Dormant catalog agents must remain inventory only.

## ACP editor profile contract

ACP editor clients consume the profile registry through a portable select option, not by
reconstructing profile identity from the effective settings label. The contract is:

1. `active_profile_selection(cwd)` is the authoritative persisted selection using project,
   legacy-project, user, legacy-user, and built-in precedence.
2. ACP option values are always scope-qualified (`project:<id>`, `user:<id>`, or
   `built-in:<id>`).
3. The selector's `currentValue` exactly matches one advertised option value. Clients must
   never receive a bare display name that they render as `Unknown`.
4. Selecting a profile persists the project pointer, applies the resolved profile to the live
   worker, and emits one complete `ConfigOptionUpdate` because model, thinking, and context may
   all change together.
5. Profile inventory remains read-only through the selector. Editing, copying, importing, and
   deletion are explicit operator commands or richer first-party surfaces; generic ACP clients
   are not expected to synthesize forms from inventory metadata.
6. Built-in profiles remain immutable. A future portable edit flow opens editable project/user
   files and copies built-ins before modification.

This keeps ACP, TUI, CLI, and web surfaces pointed at the same selection authority while leaving
presentation client-owned.

## Implementation slices

### Slice 1 — Registry discovery and compatibility

- Add `ProfileRegistry` discovery in `settings.rs` or a new `profile_registry.rs`.
- Discover:
  - project `.omegon/profiles/*.json`,
  - user `~/.omegon/profiles/*.json`,
  - legacy project/user singleton profiles,
  - built-in default.
- Add unit tests for precedence, shadowing, and invalid JSON handling.

### Slice 2 — Selection resolution

- Add active-profile pointer files.
- Resolve active selection using precedence order.
- Keep `Profile::load_with_source` as compatibility wrapper over registry resolution.
- Update profile source metadata to identify registry vs legacy singleton.

### Slice 3 — Startup integration

- Change `bootstrap::initialize_shared_settings` to use registry-backed selected profile.
- Preserve existing CLI posture/slim/full/max-turn override ordering.
- Publish selected profile id/source into settings/status surfaces.

### Slice 4 — Operator surfaces

- Add `/profiles` or extend `/profile` with registry list/use/create/import/export.
- Show scope, source kind, portability warnings, and active selection.
- Keep `/profile capture` writing legacy singleton until registry create/use UX exists, then default to selected registry entry.

### Slice 5 — SessionPlan

- Introduce `SessionPlan` as the startup contract.
- Bind selected profile and optional active agent before TUI/web/ACP surfaces are built.
- Feed `/secrets` and launch readiness from `SessionPlan`, not catalog inventory.

### Slice 6 — ACP editor profile identity

- Publish scope-qualified profile option values and derive `currentValue` from the authoritative
  active-profile selection.
- Guarantee that the current value exists in the option inventory, including compatibility
  fallbacks.
- Keep profile application aggregate: persist selection, apply live settings, and push the full
  option set.
- Add regression coverage for selected-value membership so Zed/VS Code cannot regress to
  `Unknown`.

## Open questions

- Should profile registry files be JSON only initially, or support Pkl alongside JSON from the start?
- Should project profiles be portable by default, with explicit markers for machine-local fields?
- Should a profile be allowed to select a default agent id, or should agent selection remain entirely separate?
- What should the CLI spelling be: `omegon profile use <id>` or `omegon profiles use <id>`?
- Should active-profile pointer files be committed for project defaults, or should project default selection live in the profile registry itself?

## Initial decision recommendations

- Start JSON-only for the first implementation slice to reuse existing `Profile` serde.
- Treat legacy singleton profiles as synthetic registry entries rather than migrating files immediately.
- Keep agent selection separate from profile selection for now.
- Use explicit pointer files for active selection; do not infer activity from inventory presence.
- Mirror skills scope labels and shadowing behavior for operator familiarity.

## Open Questions
