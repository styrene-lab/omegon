+++
title = "Profile-scoped capability policy"
tags = ["design", "skills", "profiles", "extensions", "capabilities"]
+++

# Profile-scoped capability policy

## Status

Proposed design. This document defines the intended architecture for disabling skills, extension-provided skills, extensions, and eventually tools for the operator profile currently in use without mutating shared project assets.

## Problem

Omegon currently has separate lifecycle concepts for profiles, skills, and extensions:

- Profiles are discoverable across user/project/built-in scopes and active selection is stored separately from profile inventory.
- Plaintext skills are discovered from user and project roots, and bundled skills are embedded in the binary.
- Extension lifecycle already supports global enable/disable/remove state for installed extensions.
- Skills are injected into the system prompt through `AugmentRegistry::load_skills` / `load_skills_subset`.

The missing capability is a reversible operator preference:

> Do not use this skill/extension while I am using the current profile.

This must not be implemented as deletion. In particular, project-local/repository skills under `.omegon/skills/**` are shared repository assets. Disabling one for a single operator/profile must not remove it from the repo or affect other operators who use the same project with different profiles.

## Design principle

**Capability availability is source-owned; capability activation is operator/profile-owned.**

Source ownership answers: where does this skill/extension/tool come from, and who may update or delete it?

Activation ownership answers: should this operator's current profile use it in this session?

The disable operation belongs to activation ownership. Therefore it is a policy overlay, not source mutation.

## Terms

| Term | Meaning |
|---|---|
| Source | The place that owns an artifact: bundled binary, user home, project repo, extension package. |
| Capability | A model-facing or runtime ability: skill, extension, extension-provided skill, tool, future prompt/template package. |
| Active profile | The resolved profile selected for the current session. |
| Profile policy | Operator-owned overlay saying which capabilities are suppressed for a profile. |
| Disable | Add suppression policy for a capability under a profile. Reversible. Non-destructive. |
| Enable | Remove suppression policy for a capability under a profile. |
| Delete/remove/uninstall | Physically remove an installed artifact from its owning source. Destructive. |
| Global extension disable | Existing extension installed-state disable; prevents extension startup independent of active profile. |

## Current evidence

Known implementation surfaces:

- `core/crates/omegon/src/skills.rs` documents skills as prompt-injected markdown loaded from `~/.omegon/skills/*/SKILL.md` and `<cwd>/.omegon/skills/*/SKILL.md`.
- `core/crates/omegon/src/skills.rs` embeds bundled skills in the `BUNDLED` constant.
- `core/crates/omegon/src/skills.rs::list_structured()` projects bundled, extension, user, and project skill entries.
- `core/crates/omegon/src/plugins/registry.rs::AugmentRegistry::load_skills_subset()` is the prompt-injection load path for user/project skill files.
- `core/crates/omegon/src/settings.rs` already has `ProfileRegistry`, `LoadedProfile`, and `ActiveProfileSelection` concepts.
- `core/crates/omegon/src/extension_cli.rs` already has global extension `enable`, `disable`, and `remove` behavior.
- `core/crates/omegon/src/control_runtime.rs::skill_delete_response()` currently physically deletes a project-local skill if it exists. That is acceptable only as an explicit project mutation, not as a profile preference.

## Goals

1. Let the operator disable a skill for the currently active profile.
2. Ensure disabling a project-local/repo skill never mutates `.omegon/skills/**`.
3. Keep disable/enable reversible and visible in `/skills` and tool projections.
4. Preserve physical deletion/removal only for explicit destructive commands.
5. Use the same policy model later for extensions and tools.
6. Record enough metadata to explain why a skill/extension is suppressed.
7. Support immediate skill reload in the current session where feasible.

## Non-goals

- Do not create a second skill registry.
- Do not make project-local skills private to one operator.
- Do not remove bundled skills from the binary.
- Do not replace global extension enable/disable in the first slice.
- Do not solve fine-grained per-tool authorization in the first slice.
- Do not add source-specific skill suppression unless a concrete need appears.

## Policy storage

Use an operator-owned policy store under `~/.omegon`, not under the repository.

Recommended first-slice path:

```text
~/.omegon/profile-policies/<profile-id>.toml
```

Rationale:

- It is explicitly operator-local.
- It does not mutate user or project profile definition files.
- It is easy to inspect and back up.
- It keeps profile artifact inventory separate from active policy overlays.
- It avoids writing profile-specific state into shared project repositories.

Alternative considered:

```text
~/.omegon/profiles/<profile-id>.policy.toml
```

This is colocated with user profiles but is awkward for project/built-in active profiles because the policy is still operator-local. `profile-policies/` makes the overlay nature clearer.

## Policy schema

First slice:

```toml
version = 1
profile_id = "coding"

[skills]
disabled = ["openspec", "flynt-design"]
```

Second slice:

```toml
version = 1
profile_id = "coding"

[skills]
disabled = ["openspec", "flynt-design"]

[extensions]
disabled = ["browser-automation"]

[tools]
disabled = ["browser_search", "reader_open"]
```

Potential future shape for source-specific suppression:

```toml
[[skills.disabled_entries]]
name = "openspec"
source = "project"
reason = "Use manual project process in coding profile"
```

Do not implement source-specific suppression initially. Source-agnostic suppression best matches operator intent: “do not use this capability in this profile,” regardless of whether it comes from bundled, user, project, or extension source.

## Canonical identity

Policy keys should be canonicalized names, not file paths.

For skills:

- canonical key: normalized skill name
- examples: `openspec`, `flynt-design`, `oci`
- aliases may resolve at command time, but policy should store canonical names

For extensions:

- canonical key: extension manifest name / installed directory name

For tools:

- canonical key: registered tool name, e.g. `browser_search`

The first implementation should avoid storing absolute paths because paths differ across machines and because repo-local path ownership is not the same as profile activation ownership.

## Profile resolution

The policy engine needs the active profile id. Resolution should reuse the existing profile registry instead of inventing a second mechanism.

Current profile concepts already include:

```rust
pub struct LoadedProfile {
    pub profile: Profile,
    pub source: ProfileSource,
}

pub struct ActiveProfileSelection {
    pub id: String,
    pub scope: Option<String>,
}
```

Recommended active policy id resolution:

1. If an active profile selection file resolved an explicit id, use that id.
2. Else if the loaded profile has `profile.name`, use that.
3. Else synthesize from source:
   - `project-default`
   - `user-default`
   - `built-in-default`
4. If everything fails, use `default`.

Expose this as one helper, not scattered logic:

```rust
pub fn active_profile_policy_id(cwd: &Path) -> String
```

Longer term, `LoadedProfile` should carry the selected registry entry id directly so policy identity does not have to infer it from profile content.

## Rust module shape

Add a module:

```text
core/crates/omegon/src/profile_policy.rs
```

Core types:

```rust
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProfileCapabilityPolicy {
    pub version: u32,
    pub profile_id: String,
    #[serde(default)]
    pub skills: SkillPolicy,
    #[serde(default)]
    pub extensions: ExtensionPolicy,
    #[serde(default)]
    pub tools: ToolPolicy,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SkillPolicy {
    #[serde(default)]
    pub disabled: BTreeSet<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExtensionPolicy {
    #[serde(default)]
    pub disabled: BTreeSet<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolPolicy {
    #[serde(default)]
    pub disabled: BTreeSet<String>,
}
```

API:

```rust
pub fn policy_path_for_profile(profile_id: &str) -> anyhow::Result<PathBuf>;
pub fn load_policy(profile_id: &str) -> anyhow::Result<ProfileCapabilityPolicy>;
pub fn save_policy(policy: &ProfileCapabilityPolicy) -> anyhow::Result<PathBuf>;

pub fn disable_skill(profile_id: &str, skill: &str) -> anyhow::Result<PolicyMutationSummary>;
pub fn enable_skill(profile_id: &str, skill: &str) -> anyhow::Result<PolicyMutationSummary>;
pub fn is_skill_disabled(profile_id: &str, skill: &str) -> bool;
pub fn disabled_skills(profile_id: &str) -> BTreeSet<String>;
```

`PolicyMutationSummary` should include:

```rust
pub struct PolicyMutationSummary {
    pub profile_id: String,
    pub capability_kind: String,
    pub capability_name: String,
    pub changed: bool,
    pub path: PathBuf,
}
```

Use `toml` if already available. If not, use JSON to avoid adding dependencies. The user-facing design does not depend on TOML specifically.

## Skill enforcement

### Enforcement point

Policy should be enforced before skill directive strings are injected into the prompt.

Current path:

```rust
AugmentRegistry::load_skills(cwd)
  -> load_skills_subset(cwd, allowed)
  -> load_from_dirs_filtered_with_policy(...)
  -> loaded_skills = result.skills
```

Recommended first-slice change:

```rust
pub fn load_skills_subset(&mut self, cwd: &Path, allowed: &[String]) {
    let profile_id = crate::profile_policy::active_profile_policy_id(cwd);
    let disabled = crate::profile_policy::disabled_skills(&profile_id);
    self.load_skills_subset_with_disabled(cwd, allowed, &disabled);
}
```

Add a testable internal path:

```rust
fn load_from_dirs_filtered_with_policy_and_suppression(
    dirs: &[PathBuf],
    allowed: &[String],
    conflict_policy: SkillConflictResolution,
    disabled: &BTreeSet<String>,
) -> PromptSkillLoadResult
```

Filtering should happen at candidate construction time:

```rust
let canonical = normalize_skill_name(&skill_name);
if disabled.contains(&canonical) {
    suppression_events.push(...);
    continue;
}
```

This prevents disabled skills from participating in conflict resolution, prompt injection, and activation events as if they were active.

### Suppression events

The existing `skill_activation_events` are about loaded/injected skills. Add suppressed events if the event type can represent them cleanly. If not, keep a separate registry field for the first slice:

```rust
skill_suppression_events: Vec<SkillActivationEvent>
```

Event fields should report:

- `active_ref`: skill ref
- `reason`: `profile_policy_disabled`
- `resolution`: `suppressed`
- `injected`: `false`
- `recommendation`: `enable_for_profile` or null

If adding suppressed events risks confusing existing consumers, do not include them in `skill_activation_events` yet. Instead expose disabled state through list/menu projections and add suppression events later with a semantic DTO update.

## Structured skill inventory

`skills::list_structured()` should annotate policy state.

If `omegon_skills::SkillEntry` can be changed safely, add fields:

```rust
pub disabled: bool,
pub disabled_for_profile: Option<String>,
```

If that crate boundary makes the change too broad, create a local projection wrapper:

```rust
pub struct SkillPolicyView {
    pub entry: SkillEntry,
    pub disabled: bool,
    pub disabled_for_profile: Option<String>,
}
```

Preferred: add fields to `SkillEntry` because ACP/tool surfaces already serialize it as the skill inventory DTO.

List behavior:

- bundled disabled skill still appears, marked disabled
- user disabled skill still appears, marked disabled
- project disabled skill still appears, marked disabled
- extension-provided disabled skill still appears, marked disabled
- hidden/shadowed skill behavior remains unchanged except disabled status applies to the resolved/canonical name

## Slash command UX

Add commands:

```text
/skills disable <name>
/skills enable <name>
/skills disabled
```

Update help:

```text
/skills disable <name>      suppress a skill for the current profile without deleting it
/skills enable <name>       remove current-profile suppression
/skills disabled            list skills disabled for the current profile
/skills delete <name>       delete a user-installed skill; project removal is explicit only
```

Command responses:

### Disable project-local skill

```text
Disabled skill `flynt-design` for profile `coding`.
The project skill file was not modified; other operators and profiles are unaffected.
Reloaded skills for future turns.
```

### Enable skill

```text
Enabled skill `flynt-design` for profile `coding`.
Removed profile policy suppression and reloaded skills for future turns.
```

### Disable unknown skill

Two viable options:

1. Strict: fail if the skill cannot be resolved.
2. Permissive: allow suppressing a future skill name.

Recommendation: strict for slash/menu actions, permissive only for direct policy file editing. Operator feedback is better if a typo does not create inert policy.

### Disable bundled skill

```text
Disabled bundled skill `oci` for profile `coding`.
Bundled skills cannot be removed from this binary; suppression is profile-local and reversible.
```

### Delete project-local skill

Current behavior physically deletes project-local skills. This should be changed.

Recommended first-slice behavior:

```text
`flynt-design` is project-local at .omegon/skills/flynt-design/SKILL.md.
Use `/skills disable flynt-design` to hide it for profile `coding`.
Removing it from the repository would affect other operators; use `/skills remove-from-project flynt-design` for an explicit project mutation.
```

Add a separate destructive command only if needed:

```text
/skills remove-from-project <name>
```

Do not expose this as the default menu action.

## TUI/menu UX

The `/skills` menu should show disabled state in row badges.

Example row states:

```text
oci            bundled    disabled: coding
rust           project    active
flynt-design   project    disabled: coding
```

Suggested row actions:

| Key/action | Meaning | Availability |
|---|---|---|
| Enter | inspect | all skills |
| i | install/refresh | bundled/user where applicable |
| d | disable for current profile | active skills |
| e | enable for current profile | disabled skills |
| g | full inspect | all skills |
| x | delete local install | user-installed only |

Do not show `x` for project-local skills in the normal menu. If project removal is eventually supported, label it explicitly as `remove from project`, not `delete`, and require a confirmation surface.

## Agent-callable tools

Current skill tools include:

- `skills_list`
- `skills_get`
- `skills_create`
- `skills_import`
- `skills_install`
- `skills_delete`
- `skills_reload`

Add:

- `skills_disable`
- `skills_enable`
- `skills_disabled`

Tool descriptions must be explicit:

```text
skills_disable: Disable a resolved skill for the active operator profile. This writes operator-local policy only and never deletes project-local skill files.
```

`skills_delete` should be narrowed:

```text
Delete a user-installed external skill. Project-local skills require an explicit project-removal command and bundled/extension skills cannot be deleted.
```

## Delete/remove semantics

Adopt this matrix:

| Source | Disable for profile | Enable for profile | Delete/remove |
|---|---:|---:|---:|
| Bundled | yes | yes | no |
| User plaintext | yes | yes | yes, user-local deletion |
| Project plaintext | yes | yes | explicit project-removal only |
| Extension-provided skill | yes | yes | no; remove/disable owning extension |
| Extension | second slice | second slice | existing remove |

Important: disabling an extension-provided skill suppresses only that skill. Disabling an extension for profile suppresses the extension process/tools/widgets/skills and belongs to the second slice.

## Extension policy second slice

Profile-scoped extension disable should not reuse the existing global extension state. It should layer on top:

1. Global extension state says whether an extension is installed-enabled at all.
2. Profile policy says whether this active profile suppresses it.
3. Runtime loader starts extension only if both allow it.

Commands:

```text
/extension disable-for-profile <name>
/extension enable-for-profile <name>
```

Keep existing commands:

```text
/extension disable <name>    # global installed-state disable
/extension enable <name>     # global installed-state enable
/extension remove <name>     # physical uninstall
```

List/menu should show:

- `enabled`
- `disabled globally`
- `disabled for profile coding`

Runtime enforcement belongs before extension process spawn/tool registration, not after tool exposure.

## Prompt/session reload semantics

Skill disable/enable should attempt immediate reload of prompt skill state for future turns.

Flow:

1. Validate skill exists and resolve canonical name.
2. Write profile policy.
3. Call the same registry reload path used by `/skills reload`.
4. Return a response that says reload happened.

If a surface cannot reload immediately, response must say so explicitly:

```text
Policy updated. Run `/skills reload` or start a new session for prompt changes to apply.
```

For extensions, immediate unload/reload may be harder because processes may already be running. Second-slice behavior can require `/extension refresh` or restart while still writing policy immediately.

## Security and safety

- Skill names must reject path traversal: `/`, `\`, `..`, null bytes, absolute path forms.
- Policy file writes must create parent directories under `~/.omegon` only.
- Do not follow operator-provided paths for policy writes.
- Do not delete files as part of disable.
- Use atomic-ish write pattern if available: write temp file in same directory then rename.
- If policy write fails, do not mutate in-memory skill state silently.

## Migration behavior

No migration is required for existing installations. Absence of a policy file means no disabled capabilities.

Existing project-local skills remain active unless disabled by the operator.

`/skills delete <project-local>` currently behaves destructively. Migration action:

- Change `/skills delete` to refuse project-local deletion by default.
- Add explicit `/skills remove-from-project` only if there is a clear operator need.
- Mention `/skills disable` in the refusal message.

This is a behavior change and must be recorded in `CHANGELOG.md` when implemented.

## Testing plan

### Unit tests: policy store

- Load missing policy returns empty policy for profile.
- Disable adds canonical skill once.
- Disable is idempotent.
- Enable removes canonical skill.
- Enable absent skill is idempotent.
- Invalid skill names are rejected.
- Policy path remains under operator home.

### Unit tests: skill prompt injection

Use temp dirs and `load_from_dirs_filtered_with_policy_and_suppression`.

Cases:

1. Disabled user skill is not present in `loaded_skills`.
2. Disabled project skill is not present in `loaded_skills`.
3. Disabled skill does not participate in conflict resolution.
4. `allowed` filter and disabled filter compose by intersection.
5. Enabling restores prompt injection.

### Unit tests: project-local non-mutation

1. Create temp repo with `.omegon/skills/foo/SKILL.md`.
2. Disable `foo` for active profile.
3. Assert policy file changed under `~/.omegon/profile-policies/**`.
4. Assert `.omegon/skills/foo/SKILL.md` content is byte-for-byte unchanged.
5. Assert prompt injection excludes `foo`.
6. Assert structured list still includes `foo` with disabled state.

### Control runtime tests

- `/skills disable foo` queues/executes correct control request.
- `/skills enable foo` queues/executes correct control request.
- `/skills disabled` reports profile id and disabled names.
- `/skills delete project-local` refuses and recommends disable.
- `/skills delete user-local` still deletes user-local skill.
- `/skills delete bundled` refuses and recommends disable.

### Tool tests

- `skills_disable` writes policy and reloads registry.
- `skills_enable` removes policy and reloads registry.
- `skills_list` includes disabled metadata.
- `skills_delete` refuses project-local skills.

### Regression invariant

Add an explicit test named like:

```rust
project_local_skill_disable_never_deletes_repo_skill_file
```

This is the design's central safety contract.

## Rollout plan

### Slice 1: skill policy

- Add `profile_policy.rs`.
- Add active profile policy id helper.
- Add skill disable/enable/list policy functions.
- Enforce policy in `AugmentRegistry` skill loading.
- Annotate structured skill inventory with disabled state.
- Add slash commands and tool functions.
- Make `/skills delete` refuse project-local deletion by default.
- Add tests and changelog.

### Slice 2: extension policy

- Reuse policy schema `[extensions] disabled`.
- Add commands `disable-for-profile` / `enable-for-profile`.
- Gate extension spawn/registration.
- Show profile-disabled extension state in extension list/menu.
- Add tests.

### Slice 3: tool policy

- Reuse policy schema `[tools] disabled`.
- Gate command/tool registry projection.
- Decide how profile-disabled tools interact with hidden tool groups and `manage_tools`.
- Add tests for unavailable tool exposure and command routing.

## Open questions

- Should policy keys use active profile selection id or `Profile.name` when they differ?
- Should disabled skill suppression produce `SkillActivationEvent` entries with `injected = false`, or should disabled state stay only in inventory projections for now?
- Should `/skills disable <unknown>` be strict or allow future suppression? Recommendation: strict.
- Should project-local deletion be supported as `/skills remove-from-project`, or should operators edit/delete repo files through normal file operations?
- Should user-local policy be syncable across machines, or remain per-machine by default?

## Decisions captured

- Disabling a skill is profile policy, not deletion.
- Project-local skills are repository assets and must not be removed by profile disable.
- Suppression should be source-agnostic by canonical skill name in the first implementation.
- Existing extension global disable remains distinct from profile-scoped extension suppression.
- `/skills delete` should no longer silently delete project-local skills as the default deletion behavior.
