+++
title = "Profile defaults and runtime drift"
tags = ["architecture","profiles","settings","posture","runtime","tui"]
+++

+++
id = "4cc6255c-0a0f-4cdb-817a-8753075e41e1"
kind = "design_node"

[data]
title = "Profile defaults and runtime drift"
status = "exploring"
issue_type = "architecture"
priority = 2
parent = "5f83a73e-d20a-4e3b-bf0a-384915e91136"
dependencies = []
open_questions = [
  "[assumption] Project-local .omegon/profile.json remains the primary persistence target for current-profile capture.",
  "[assumption] A single active repo profile is enough for the first implementation; named profile catalogs can come later.",
  "[assumption] Runtime drift can initially compare only settings::Profile fields that already round-trip through capture_from/apply_to.",
  "[assumption] Settings menu and slash command menu projections can consume the same profile-drift DTO without renderer-specific duplication."
]
related = [
  "docs/operator-profile.md",
  "docs/runtime-profile-status-contract.md",
  "docs/runtime-identity-persona-posture-stack.md",
  "docs/design/operator-capability-profile.md",
  "docs/design/authority-envelope-runtime.md",
  "docs/modern-command-palettes.md"
]
+++

# Profile defaults and runtime drift

## Overview

Omegon already has a profile surface. Do not invent a second one.

Current implementation evidence:

- `settings::Profile` persists repo/user defaults in `.omegon/profile.json` and `~/.omegon/profile.json`.
- `/profile view`, `/profile export`, `/profile capture`, and `/profile apply|load` already exist in the slash/control path.
- Existing profile fields cover model, model intent, thinking level, max turns, provider order, fallback providers, default posture, permissions, automation, update behavior, tool detail, sandbox, terminal tool, persona, tone, integrations, and extension load policy.
- `profile.capture_from(&Settings)` already implements “save current runtime settings into profile”.
- `profile.apply_to_with_posture(&mut Settings, cwd)` already implements “load profile defaults into runtime”.
- `docs/modern-command-palettes.md` already establishes that slash/menu surfaces should come from renderer-neutral projections, not TUI-local duplicate registries.

The missing design is not basic profile persistence. The missing design is the contract between saved profile defaults, live runtime drift, and the operator surfaces that make that drift legible.

## Existing surfaces to preserve

### Repo/user profile persistence

`settings::Profile::load(cwd)` already resolves profile state by loading the project profile first and falling back to the user profile. `Profile::save(cwd)` writes the project-local profile. `Profile::save_global()` writes the user-level profile.

This remains the storage spine.

### Slash/control profile commands

Existing commands should be treated as the base command family:

- `/profile view` — show live runtime plus saved profile document.
- `/profile export` — export profile data.
- `/profile capture` — capture live runtime into the profile.
- `/profile apply` / `/profile load` — apply saved profile defaults to live runtime.
- `/profile mqtt ...` — mutate integration defaults.
- `/profile extension allow|deny|clear` — mutate extension load defaults.
- `/profile persona ...` and `/profile tone ...` — mutate persona/tone defaults.

New work should refine these commands, not create a separate `/defaults` or parallel profile system.

### Settings menu and slash popup surfaces

This design should land with the settings menu overhaul and slash command popup polish, not as backend-only plumbing.

The settings menu is where the operator should understand each setting row as one of:

- inherited from profile default,
- live override / unsaved drift,
- saved project default,
- saved user default,
- next-startup-only policy.

The slash command popup is where commands that cause runtime drift should be honest about persistence. A row such as `/think high` or `/context massive` should communicate “apply live” rather than implying “save default”. Profile save/revert actions should be nearby when drift exists.

This pairs with the command palette design rule in [[modern-command-palettes]]: menu rows should be renderer-neutral semantic projections, not hidden TUI allowlists.

### Runtime stack

This design extends [[runtime-identity-persona-posture-stack]] rather than replacing it. Profiles seed the behavior/resource layers, but they are not identity or authorization.

Profiles may store defaults for:

```text
Persona -> Posture -> Resource Envelope
```

Profiles may also store operator preferences and permission policy, but runtime identity and authority envelopes remain separate concepts.

## Problem statement

The current profile path blurs two operations:

1. Change runtime now.
2. Persist this as a future default.

Commands like `/think` and `/context` currently trend toward immediate persistence by calling `profile.capture_from(&s)` after mutating runtime. That makes live tactical changes sticky by accident.

The desired semantics are:

> Profile defaults seed runtime. Runtime may intentionally drift. Drift is visible, reversible, and only persists when the operator explicitly saves it.

## Decision model

### Decision: profile values are defaults, not shackles

**Status:** proposed

A loaded profile establishes starting defaults. The runtime may depart from those defaults during work because the operator or harness needs a different execution envelope.

Examples of legitimate live drift:

- `/think high` for a design review.
- `/context massive` for a large-codebase pass.
- `/model ...` for provider-specific behavior.
- enabling a skill or extension for a task.
- switching from slim/explorator posture to full/architect mode.

These changes are valid runtime state even if they are unsaved.

### Decision: profile capture is explicit

**Status:** proposed

Live runtime changes should not silently overwrite the profile. Saving defaults should flow through profile commands:

- `/profile capture` — existing command; should be the canonical “save current runtime over active profile”.
- `/profile save` — proposed alias for `/profile capture`.
- `/profile save --global` — optional future affordance for `Profile::save_global()`.
- `/profile save as <name>` — future named-profile catalog; not required for v1.

`/think`, `/context`, `/model`, `/provider`, `/skills`, and similar live commands should mutate runtime and mark drift, not persist by default.

### Decision: drift is computed as runtime minus loaded profile snapshot

**Status:** proposed

At profile load/startup, store or reconstruct a baseline snapshot of the effective profile defaults after posture resolution. Runtime settings are compared against that baseline.

Initial drift fields should be limited to axes already represented in `settings::Profile` and `Settings`:

- model / model intent / exact override
- thinking level
- requested context class once it is added to profile capture/apply
- max turns
- automation level
- provider order and fallback providers
- tool detail
- sandbox
- terminal tool
- persona/tone where observable
- extension load policy, with the caveat that load policy may apply only on next startup

Do not include ephemeral fields:

- token pressure
- actual model context window
- provider connected/auth health
- session id
- runtime bridge instance
- transient cooldowns
- workbench/delegate/cleave state

### Decision: profile drift is a shared projection consumed by settings, slash popup, chrome, and `/profile view`

**Status:** proposed

Do not compute profile drift separately inside each UI surface. Introduce a renderer-neutral projection that can be reused by:

- `/profile view` text output,
- settings menu rows,
- slash command popup/menu rows,
- title/status chrome,
- ACP/IPC/Web status surfaces later.

Suggested DTO shape:

```rust
struct ProfileDriftProjection {
    profile_label: String,
    source: ProfileSource,
    dirty: bool,
    changed_count: usize,
    rows: Vec<ProfileDriftRow>,
    actions: Vec<ProfileDriftAction>,
}

struct ProfileDriftRow {
    key: String,
    label: String,
    profile_value: String,
    runtime_value: String,
    persistence: PersistenceSemantics,
    severity: DriftSeverity,
}

enum PersistenceSemantics {
    LiveOnly,
    SavedDefault,
    AppliesNextStartup,
    EphemeralRuntime,
}
```

This keeps producer/provenance separate from visual form. TUI, ACP, and future UI clients render the same semantic state differently.

### Decision: surface drift subtly in chrome and explicitly in profile/settings/palette surfaces

**Status:** proposed

A compact title/status cue should indicate that runtime differs from profile defaults without distracting from work.

Preferred form:

```text
prof:architect Δ3
```

or, if no named profile exists yet:

```text
prof:project Δ3
```

Rendering rule:

- no delta / neutral color: runtime matches loaded profile baseline.
- amber/dim gold profile marker: runtime has unsaved drift.
- optional `ΔN`: count of changed axes.

`/profile view` should move from raw JSON-only output toward a human projection:

```text
Profile: project (.omegon/profile.json)
Runtime drift: Δ3 unsaved changes

  Thinking       medium → high
  Context class  extended → massive
  Model          auto/S → anthropic:claude-sonnet-4-6

Actions:
  /profile save       Save current runtime as profile defaults
  /profile apply      Revert runtime to profile defaults
```

The raw JSON can remain available through `/profile export`.

### Decision: settings menu rows expose default/live/persistence state

**Status:** proposed

The settings menu overhaul should make profile drift visible at row level. Each setting row should be able to display:

- the saved/default value,
- the live/runtime value,
- whether the row is clean or drifted,
- whether edits apply immediately or on next startup,
- available row actions: apply live, save to profile, revert to profile.

Example row projection:

```text
Thinking        high        Δ profile: medium      live only
Context class   massive     Δ profile: extended    live only
Extensions      +flynt      next startup            profile policy
```

This avoids a separate “profile page” becoming the only place to discover unsaved runtime changes.

### Decision: slash command popup rows explain runtime-only mutation

**Status:** proposed

The slash popup/menu polish should include persistence semantics for commands that mutate settings. Rows should distinguish:

- runtime-only mutation,
- profile/default mutation,
- action that reverts live state to profile,
- action that saves drift to profile.

Examples:

```text
/think high          Apply high reasoning now          live override
/context massive     Use massive working set now       live override
/profile save        Save current drift as defaults    persists
/profile apply       Revert runtime to profile         destructive/live
```

When drift exists, related actions should be promoted or grouped:

```text
Profile drift Δ3
  /profile view
  /profile save
  /profile apply
```

This should use the shared command-menu projection rather than TUI-local special cases.

### Decision: startup precedence must protect explicit profile fields from posture defaults

**Status:** proposed

Posture should establish defaults, then explicit profile fields should override those defaults.

Current `Profile::apply_to_with_posture()` already follows this ordering internally: apply default posture first, then `apply_to()`.

Startup code must not later call `set_posture()` in a way that overwrites explicit profile resource fields without restoring them. In particular, slim/full/child runtime posture overrides should be reconciled against explicit saved fields such as thinking level and requested context class.

## Existing gaps found

### Gap: requested context class is runtime-only

`Settings` has `requested_context_class: Option<ContextClass>`, but the current `Profile` schema does not expose a saved requested-context-class field in the portions inspected. `capture_from()` also does not capture it.

This explains why `/context <class>` can persist only accidentally or not at all depending on current command path. A proper profile field should be added:

```rust
pub requested_context_class: Option<String>
```

or a better named profile field:

```rust
pub context_class: Option<String>
```

The field should represent the operator-requested working-set policy, not the actual provider/model window.

### Gap: `/think` and `/context` currently save too eagerly

`set_thinking_response()` and `set_context_class_response()` mutate runtime and then capture/save the profile. That prevents the runtime/profile drift model from being visible, because the baseline is overwritten immediately.

These commands should become runtime-only by default once drift is implemented.

### Gap: `/profile view` emits raw JSON

The existing view already includes both `live` and `profile`; this is the correct data source. It needs a projection layer that computes and renders drift.

### Gap: no active profile baseline is modeled

The system can load/apply a profile, but it does not appear to preserve “this was the baseline at load time” as a first-class runtime object. Without that, drift has to compare runtime against disk, which can be wrong if the profile file changes externally during a session.

V1 may compare against current `Profile::load(cwd)` for simplicity, but the intended architecture is a loaded-profile snapshot.

## Proposed implementation slices

### Slice 1 — schema and startup correctness

- Add profile support for requested context class.
- Ensure `Profile::apply_to()` applies requested context class after posture defaults.
- Ensure `Profile::capture_from()` captures requested context class only when explicitly set or non-default by policy.
- Fix startup ordering so slim/full posture defaults do not erase explicit profile resource fields.

### Slice 2 — drift projection

- Add a small `ProfileDriftProjection` comparing live `Settings` to profile/baseline.
- Include changed field key, display label, profile value, runtime value, persistence note, and severity.
- Use it in `/profile view`.
- Keep `/profile export` raw for tooling.

### Slice 3 — command semantics

- Make `/profile save` an alias for existing `/profile capture`.
- Make `/profile revert` an alias for existing `/profile apply`.
- Change `/think` and `/context` to runtime-only by default.
- Optionally add `--save` flags later if compatibility demands it.

### Slice 4 — settings menu integration

- Extend `SettingsSurfaceProjection` rows with optional profile-default/runtime-drift metadata.
- Show row-level clean/drift/next-startup status in the settings menu overhaul.
- Provide row-level actions for save/revert where safe.
- Keep TUI-local code responsible only for navigation, filtering, selection, and input dispatch.

### Slice 5 — slash popup/chrome integration

- Extend command-menu rows with persistence semantics and related profile actions.
- Promote `/profile view`, `/profile save`, and `/profile apply` when drift exists.
- Add the compact `prof:<label> ΔN` cue to status/title chrome.
- Render drift in amber/dim gold using the existing chrome style vocabulary.

## Non-goals for v1

- Named profile catalog and `/profile save as <name>`.
- Cross-machine profile synchronization.
- Full extension hot-reload semantics.
- Treating profile as authorization identity.
- Replacing authority envelopes, route state, or provider health state.

## Open questions

- [assumption] Project-local `.omegon/profile.json` remains the primary persistence target for current-profile capture.
- [assumption] A single active repo profile is enough for the first implementation; named profile catalogs can come later.
- [assumption] Runtime drift can initially compare only fields that already round-trip through `Profile::capture_from()` and `Profile::apply_to()`.
- [assumption] Settings menu and slash command menu projections can consume the same profile-drift DTO without renderer-specific duplication.
- Should `/think high --save` and `/context massive --save` be supported for fast one-shot persistence, or should all persistence route through `/profile save`?
- Should profile drift compare against the disk profile live, or against a startup/apply snapshot? The snapshot is architecturally cleaner; disk comparison is simpler but can lie after external edits.
