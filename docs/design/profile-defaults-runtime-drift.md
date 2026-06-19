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
  "[assumption] Project-local .omegon/profile.json remains the primary project override target, while user-level ~/.omegon/profile.json remains the primary user defaults target.",
  "[assumption] Bare /profile save should write to the active loaded source when that source is project or user, and require an explicit target when the source is built-in defaults.",
  "[assumption] A single active repo/user profile source is enough for the first implementation; named profile catalogs can come later.",
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

- `/profile capture` — existing command; should be the compatibility spelling for saving current runtime over an explicit target.
- `/profile save` — proposed primary spelling.
- `/profile save --project` — write the project override at `<project-root>/.omegon/profile.json`.
- `/profile save --user` — write the user default at `~/.omegon/profile.json`.
- `/profile save --active` — write back to the active loaded source when that source is project or user.
- `/profile save as <name>` — future named-profile catalog; not required for v1.

Default target policy must be explicit in output. Recommended v1 policy:

- If the active profile source is project, bare `/profile save` writes project.
- If the active profile source is user, bare `/profile save` writes user.
- If the active source is built-in defaults, bare `/profile save` should either require `--project|--user` or choose project with a clear message. Prefer requiring a target to avoid accidentally creating repo policy.

`/think`, `/context`, `/model`, `/provider`, `/skills`, and similar live commands should mutate runtime and mark drift, not persist by default.

### Decision: profile source and save target are first-class

**Status:** proposed

Current implementation has distinct paths but incomplete semantics:

- Project profile: `<project-root>/.omegon/profile.json` via `project_profile_path(cwd)`.
- User profile: preferred `~/.omegon/profile.json`, falling back to config-dir `omegon/profile.json` only when home is unavailable.
- `Profile::load(cwd)` currently loads project if present, otherwise user, otherwise default. It does not merge user + project.
- `Profile::save(cwd)` always writes project.
- `Profile::save_global()` writes user.
- Existing `/profile capture` loads whichever profile is active, captures live settings, then calls `save(cwd)`, which means a user profile can be silently materialized into a project override.

That last behavior is the problem to fix.

Introduce explicit source and target concepts:

```rust
enum ProfileSource {
    Project(PathBuf),
    User(PathBuf),
    BuiltInDefault,
}

enum ProfileSaveTarget {
    ActiveSource,
    Project,
    User,
}
```

`/profile view` should show the active source:

```text
Profile: project (.omegon/profile.json)
Profile: user (~/.omegon/profile.json)
Profile: built-in defaults
```

`/profile save` must respect user/project distinction. It must not silently turn user-level defaults into repo-local policy unless the operator chooses the project target.

Longer-term, profile resolution should probably become layered merge:

```text
built-in defaults
→ user profile
→ project profile
→ CLI/env overrides
→ live runtime drift
```

But that is a larger semantic change. V1 may preserve current project-overrides-user load behavior if source and save target are made explicit.

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
    save_target: Option<ProfileSaveTarget>,
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

### Gap: `/profile capture` currently collapses user/project distinction

Current `/profile capture` effectively does:

```rust
let mut profile = settings::Profile::load(cwd);
profile.capture_from(&settings);
profile.save(cwd);
```

Because `Profile::save(cwd)` always writes the project profile, a session that loaded user defaults from `~/.omegon/profile.json` can silently create or overwrite `<project-root>/.omegon/profile.json`. That turns personal defaults into project-local policy without an explicit target choice.

The fix is to make profile load return source metadata and make profile save target explicit. `/profile save --user` should preserve user-level defaults; `/profile save --project` should intentionally create/update project policy; bare `/profile save` should follow the active source only when that is unambiguous.

### Gap: no active profile baseline is modeled

The system can load/apply a profile, but it does not appear to preserve “this was the baseline at load time” as a first-class runtime object. Without that, drift has to compare runtime against disk, which can be wrong if the profile file changes externally during a session.

V1 may compare against current `Profile::load(cwd)` for simplicity, but the intended architecture is a loaded-profile snapshot.

## Proposed implementation slices

### Slice 1 — schema, source tracking, and startup correctness

- Add profile support for requested context class.
- Add profile load source tracking for project/user/built-in defaults.
- Add explicit save targets for active source, project, and user.
- Ensure `/profile save` and `/profile capture` respect user/project target semantics instead of always writing project.
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

## Cutover and TDD plan

This work is cross-cutting enough that each semantic cutover needs tests landed before behavior flips. The tests should pin the shared contract first, then let UI/chrome work consume that contract without re-litigating persistence semantics.

### Cutover 1 — profile schema, source tracking, and startup precedence

Purpose: make the profile capable of representing the runtime axes we intend to compare while preserving user/project persistence boundaries.

Pre-cutover tests:

- `settings.rs`: profile serde round-trip accepts and emits requested context class.
- `settings.rs`: `Profile::load_with_source` returns `Project` when `<project-root>/.omegon/profile.json` exists.
- `settings.rs`: `Profile::load_with_source` returns `User` when no project profile exists and `~/.omegon/profile.json` exists.
- `settings.rs`: `Profile::load_with_source` returns `BuiltInDefault` when neither profile exists.
- `settings.rs`: explicit project save target writes only `<project-root>/.omegon/profile.json`.
- `settings.rs`: explicit user save target writes only `~/.omegon/profile.json`.
- `settings.rs`: active-source save writes user when the loaded baseline came from the user profile and project when it came from the project profile.
- `settings.rs`: built-in-default active-source save behavior is explicit: either rejected until `--project|--user` is supplied, or writes project with a clear message.
- `settings.rs`: `Profile::apply_to()` restores requested context class without changing actual model-derived `context_class`/`context_window`.
- `settings.rs`: `Profile::capture_from()` captures explicit requested context class and omits or normalizes unset/default state according to the final policy.
- `bootstrap.rs`: slim/full posture defaults do not erase explicit profile thinking or requested context class.
- `bootstrap.rs`: CLI posture override remains stronger than profile posture defaults, while explicit profile resource fields are restored only where policy says they should be.

Cutover event:

- Add the profile field and apply/capture behavior.
- Add source-aware profile loading and target-aware profile saving.
- Keep existing `/think` and `/context` persistence semantics unchanged until drift projection tests exist.

### Cutover 2 — pure drift projection

Purpose: create the semantic source of truth before changing any operator-facing behavior.

Pre-cutover tests:

- `surfaces/profile.rs` or equivalent: clean profile/runtime pair yields `dirty = false`, `changed_count = 0`, no drift rows.
- Thinking drift yields one row with stable key, display label, profile value, runtime value, `LiveOnly` persistence, and non-error severity.
- Requested-context-class drift yields one row and labels the value as requested working-set policy, not actual model window.
- Multiple drifted axes produce stable ordering for snapshot-style assertions.
- Ephemeral runtime-only values such as provider connection state and token pressure are excluded.
- Drift projection actions include view/save/apply affordances only when they are relevant.

Cutover event:

- Introduce `ProfileDriftProjection` and use it in tests only or behind `/profile view` while raw JSON remains available through `/profile export`.

### Cutover 3 — `/profile view`, `/profile save`, and `/profile revert`

Purpose: give operators a reliable drift readout before runtime commands stop auto-saving.

Pre-cutover tests:

- `control_runtime.rs`: `/profile view` renders clean state without a drift warning and includes active source/target information.
- `control_runtime.rs`: `/profile view` renders drift rows for thinking/context with save/apply actions.
- `control_runtime.rs`: `/profile export` remains raw JSON/tooling-friendly.
- TUI parser tests: `/profile save` maps to source-aware capture behavior.
- TUI parser tests: `/profile save --project` maps to explicit project save target.
- TUI parser tests: `/profile save --user` maps to explicit user save target.
- TUI parser tests: `/profile revert` maps to existing apply behavior.
- Daemon/control tests: aliases route through the same control requests, not TUI-only branches.

Cutover event:

- Make `/profile view` human-first.
- Add save/revert aliases.
- Keep capture/apply as stable compatibility spellings.

### Cutover 4 — runtime-only `/think` and `/context`

Purpose: separate tactical runtime mutation from persistence.

Pre-cutover tests:

- `control_runtime.rs`: `set_thinking_response` changes live settings but does not write profile by default.
- `control_runtime.rs`: `set_context_class_response` changes live requested context class but does not write profile by default.
- `control_runtime.rs`: failed profile-save rollback tests are removed or replaced with runtime-only success tests plus explicit `/profile save` failure tests.
- `/profile view` after `/think high` shows thinking drift.
- `/profile save` after `/think high` clears thinking drift by updating the profile.
- `/profile apply` after `/think high` restores runtime from profile and clears drift.
- Equivalent context-class tests cover requested context class.

Cutover event:

- Remove eager `profile.capture_from(&s)` from `/think` and `/context` handlers.
- Update command output copy to say “live override” or “runtime only” where appropriate.

### Cutover 5 — settings menu row integration

Purpose: let the settings overhaul consume drift semantics row-by-row.

Pre-cutover tests:

- `surfaces/settings.rs`: runtime settings rows can carry optional profile/default metadata.
- `surfaces/settings.rs`: thinking/context rows render clean when runtime matches profile baseline.
- `surfaces/settings.rs`: thinking/context rows render drift metadata when runtime differs.
- `tui/settings_menu.rs`: row navigation/edit dispatch remains unchanged when drift metadata is present.
- Settings projection tests assert persistence labels: live-only, saved default, next-startup-only.

Cutover event:

- Wire profile drift metadata into `SettingsSurfaceProjection`.
- Keep the TUI renderer responsible only for visual treatment, filtering, navigation, and edit dispatch.

### Cutover 6 — slash popup and chrome integration

Purpose: make drift visible at command-discovery and ambient-status layers without duplicating logic.

Pre-cutover tests:

- `surfaces/command_menu.rs`: command rows can carry persistence semantics.
- Command-menu projection labels `/think <level>` and `/context <class>` rows as live overrides.
- Command-menu projection promotes `/profile view`, `/profile save`, and `/profile apply` when drift exists.
- TUI autocomplete/slash popup tests prove persistence metadata survives filtering.
- Chrome/statusline tests render neutral profile cue when clean and amber/delta cue when dirty.
- ACP/IPC command-discovery tests either preserve existing output or explicitly include the new metadata without breaking compatibility.

Cutover event:

- Add persistence semantics to command-menu rows.
- Add `prof:<label> ΔN` to chrome from the same profile drift projection.

### Regression suite shape

The suite should be layered by contract:

1. **Profile persistence tests** in `settings.rs` own serde/apply/capture behavior.
2. **Startup precedence tests** in `bootstrap.rs` own profile/posture/CLI/env ordering.
3. **Projection tests** in `surfaces/profile.rs`, `surfaces/settings.rs`, and `surfaces/command_menu.rs` own renderer-neutral semantics.
4. **Control-runtime tests** own slash/control command effects and persistence boundaries.
5. **TUI parser/render tests** own command parsing, selector dispatch, row filtering, and chrome treatment.
6. **ACP/IPC/Web tests** own compatibility for non-TUI command/status consumers.

Do not test profile drift by scraping final TUI pixels first. Test the semantic projection first, then add one or two renderer smoke tests per surface.

## Future product line — Armory-published portable profiles

This is explicitly **not** a 0.27.0 line item. It depends on the profile-source, drift, save-target, schema, and trust foundations described above.

Longer-term, Omegon should be able to publish and install portable profile artifacts through the Armory alongside skills and extensions. Skills and extensions will likely remain repo-shaped artifacts because they contain documentation, code, tests, assets, and versioned package structure. Profiles are different: they are small, singular, portable configuration artifacts.

A portable Armory profile could represent:

- a review posture/profile for conservative code audit work,
- a release-manager profile,
- a local-first low-cost profile,
- a high-context architecture profile,
- a hardened daemon/headless profile,
- a project-family default profile for a team or organization.

The product goal is not “copy someone else's settings blindly.” The goal is signed, inspectable, portable defaults that an operator can import, diff against current runtime/project/user settings, apply selectively, and save into the appropriate user/project target.

### Trust requirement: signed profiles

GPG or an equivalent signing foundation is mandatory before Armory-published profiles are trusted product surface.

Minimum trust requirements:

- profile artifacts are signed by a maintainer/publisher key,
- Omegon can verify the signature before install/apply,
- the operator can inspect the profile contents before applying,
- profile provenance is visible in `/profile view` or an equivalent profile-detail surface,
- imported profiles cannot silently grant authority or bypass permissions,
- profile signatures are separated from runtime identity/authorization claims.

GPG is the likely starting point because it is familiar and portable, but the design should leave room for future Sigstore/minisign/SSH-signature support if the Armory trust model evolves.

### Required foundations before this can ship

This depends on work that does not exist yet:

- a stable profile schema with explicit user/project/source/target semantics,
- profile drift and projection infrastructure,
- named/imported profile identity separate from active user/project profile files,
- Armory artifact metadata for profile artifacts,
- a key-management and trust-store story for publisher keys,
- signature verification plumbing,
- an install/apply flow that previews diffs and supports selective adoption,
- policy that prevents imported profiles from acting as authorization grants.

### Product shape sketch

Possible future commands:

```text
/armory profile search review
/armory profile inspect styrene/release-manager
/armory profile install styrene/release-manager
/profile import armory:styrene/release-manager
/profile apply imported:styrene/release-manager --preview
/profile save --user --from imported:styrene/release-manager
```

Possible profile detail fields:

```text
Profile: styrene/release-manager
Source: armory
Publisher: Styrene Labs
Signature: verified GPG key ABCD1234
Applies to: user/project/selective
Contains: posture, thinking, context, model intent, tool detail, extension policy
Excludes: secrets, credentials, runtime identity, authority grants
```

The first implementation should bias toward preview/diff/apply rather than automatic activation. Portable profiles are defaults and preferences, not authority envelopes.

## Non-goals for v1

- Named profile catalog and `/profile save as <name>`.
- Cross-machine profile synchronization.
- Full extension hot-reload semantics.
- Treating profile as authorization identity.
- Replacing authority envelopes, route state, or provider health state.
- Armory-published portable profiles or signed profile import.

## Open questions

- [assumption] Project-local `.omegon/profile.json` remains the primary project override target, while user-level `~/.omegon/profile.json` remains the primary user defaults target.
- [assumption] Bare `/profile save` should write to the active loaded source when that source is project or user, and require an explicit target when the source is built-in defaults.
- [assumption] A single active repo/user profile source is enough for the first implementation; named profile catalogs can come later.
- [assumption] Runtime drift can initially compare only fields that already round-trip through `Profile::capture_from()` and `Profile::apply_to()`.
- [assumption] Settings menu and slash command menu projections can consume the same profile-drift DTO without renderer-specific duplication.
- Should `/think high --save` and `/context massive --save` be supported for fast one-shot persistence, or should all persistence route through `/profile save`?
- Should profile drift compare against the disk profile live, or against a startup/apply snapshot? The snapshot is architecturally cleaner; disk comparison is simpler but can lie after external edits.
