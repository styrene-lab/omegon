+++
id = "219bd3be-d23f-4676-a5b8-7877fe7fcd7a"
kind = "design_node"

[data]
title = "Skill Registry and Extension Integration"
status = "exploring"
issue_type = "architecture"
priority = 2
dependencies = []
open_questions = []
+++

## Overview

# Skill Registry and Extension Integration

## Overview

Omegon needs a single resolved skill inventory that accounts for both fluid plaintext skills and skills contributed by extensions. The goal is not to duplicate the extension registry; the skill registry is a projection over all skill providers, while the extension registry remains the owner of extension installation, versioning, discovery, and lifecycle.

The operator-facing lifecycle is: “I need a skill to do X” → agent assesses fit using a meta-skill → inventory existing skills across providers → create/update/import/override local plaintext skills when needed → reload the skill registry without recompilation.

## Current Evidence

- `core/crates/omegon/src/plugins/registry.rs` currently injects loaded skill directive strings into the system prompt. It loads from `~/.omegon/skills/` and `<cwd>/.omegon/skills/`.
- `core/crates/omegon/src/skills.rs` owns plaintext skill schema, parsing, listing, and bundled/user/project skill installation concepts.
- `core/crates/omegon/src/extension_registry.rs` owns armory extension discovery and installation. This should remain the extension distribution registry, not become the plaintext skill registry.
- `docs/extension-registry-design.md` describes extensions as curated/custom/local packages installed under `~/.omegon/extensions/` with their own publishing/versioning lifecycle.

## Design Decision

Create a unified skill registry/resolver whose inputs are multiple skill providers:

1. Bundled/internal skill provider.
2. User-local plaintext provider: `~/.omegon/skills/<name>/`.
3. Extension-provided skill provider: installed extension manifests exposing embedded or sidecar skills.
4. Project-local plaintext provider: `<project>/.omegon/skills/<name>/`.

Project-local plaintext skills have the highest precedence and are the default location for local overrides. Do not introduce a top-level `skills/` directory.

The extension registry remains the source of truth for extensions. Extension-provided skills are read-only skill contributions projected into the skill registry, not separately installed skills.

## Provider Ownership

| Provider | Owns | Editable by operator | Reload behavior |
|---|---|---:|---|
| Bundled skills | default guidance shipped with Omegon | no; override locally | reloadable from registry projection, but bundled content changes on upgrade |
| User plaintext skills | user-global guidance | yes | explicit skill reload |
| Extension skills | guidance packaged with installed extensions | no; create local override | extension reload/restart may be required for new extension package, but skill projection can refresh existing sidecar metadata |
| Project plaintext skills | project-specific overrides/custom skills | yes | explicit skill reload |

## Precedence

Resolution is by canonical skill name/alias after provider normalization:

```text
project plaintext: <project>/.omegon/skills/<name>/
    >
user plaintext: ~/.omegon/skills/<name>/
    >
extension-provided skill: ~/.omegon/extensions/<extension>/...
    >
bundled default skill
```

The resolver must retain shadow metadata rather than silently dropping lower-precedence entries.

Example projection:

```text
rust
  active source: project plaintext
  path: .omegon/skills/rust/SKILL.md
  shadows: user plaintext rust, bundled rust
  editable: yes
  reloadable: yes
```

## Extension Implications

Extensions may contribute skills, but they must not create a second skill lifecycle. The extension registry handles extension install/update/remove/version. The skill registry consumes extension skill descriptors as one provider source.

Extension-provided skills should be treated as immutable defaults. If the operator says “update this extension skill,” the correct action is usually:

1. inspect the extension-provided skill;
2. create a project-local override at `.omegon/skills/<name>/` or user-local override at `~/.omegon/skills/<name>/`;
3. preserve provenance that the override shadows `extension:<extension-id>/<skill-name>`;
4. reload skills.

Extension packages may expose skills by one of these mechanisms, in order of preference:

1. manifest-declared skill descriptors pointing to packaged guidance files;
2. conventional sidecar path inside the installed extension directory;
3. generated capability inventory converted to skill guidance only if the extension explicitly declares it.

Do not infer arbitrary extension README content as a skill without an explicit import/authoring step.

## Required Internals

### SkillDescriptor

The registry needs a structured descriptor rather than only `Vec<String>` prompt fragments:

```rust
struct SkillDescriptor {
    id: String,
    canonical_name: String,
    manifest: SkillManifest,
    body: String,
    source: SkillSource,
    provider_id: String,
    guidance_path: Option<PathBuf>,
    editable: bool,
    reloadable: bool,
    shadows: Vec<SkillRef>,
    shadowed_by: Option<SkillRef>,
    diagnostics: Vec<SkillDiagnostic>,
}
```

### SkillSource

```rust
enum SkillSource {
    Bundled,
    UserPlaintext,
    ProjectPlaintext,
    Extension { extension_id: String, extension_version: Option<String> },
}
```

### SkillProvider

```rust
trait SkillProvider {
    fn provider_id(&self) -> &str;
    fn precedence(&self) -> SkillPrecedence;
    fn discover(&self, cwd: &Path) -> anyhow::Result<Vec<SkillDescriptor>>;
}
```

Provider discovery should be independent from prompt assembly. Prompt assembly should consume the resolved active descriptors.

### SkillRegistry

The skill registry should:

- discover descriptors from all providers;
- normalize names and aliases;
- validate manifests and activation metadata;
- resolve precedence and shadowing;
- expose active and shadowed entries;
- support explicit reload;
- provide a prompt-fragment view for `PluginRegistry`;
- provide a management/status view for `/skills` and future TUI modals.

### PluginRegistry Integration

`PluginRegistry` should not independently walk skill directories long-term. It should depend on the skill registry projection:

```text
SkillRegistry::reload(cwd)
  -> ResolvedSkillInventory
  -> PluginRegistry receives ordered prompt directives
```

This prevents duplicate handling between plaintext skill loading and extension-provided skill loading.

## Agent Lifecycle

The internal skill-authoring meta-skill should guide the agent:

1. Determine whether the operator request is skill-shaped.
2. Inventory resolved skills across bundled/user/extension/project providers.
3. Prefer use or override over duplicate creation.
4. Create local skills under `.omegon/skills/<name>/` by default.
5. Import external sources as editable local skills, not armory packages.
6. Treat extension skills as immutable unless the extension itself exposes an editable local development mode.
7. Reload skills after file mutation.
8. Report source, path, shadowing, and reload status.

## Cross-Agent Compatibility

There is no evidence of a single formal standards body for agent skills, but there is an emerging de facto shape across Claude Code/Claude Agent Skills, Codex, VS Code/Copilot, and parts of the Cursor ecosystem:

```text
<skill-name>/
  SKILL.md          # required: frontmatter + markdown guidance
  scripts/          # optional helper scripts
  resources/        # optional references, templates, examples
```

The portable core is deliberately small:

- a directory per skill;
- a required `SKILL.md`;
- YAML frontmatter with at least `name` and `description` for progressive disclosure;
- markdown body for full guidance;
- optional adjacent scripts/resources referenced from the guidance.

Claude Code documentation describes skills as `SKILL.md` files with YAML frontmatter and markdown content, loaded from skill directories, and notes that skills can function as slash commands. Anthropic Agent Skills documentation describes skills as directories containing instructions, executable code, and reference materials. OpenAI Codex documentation describes skills as reusable workflows and points distribution toward plugins when skills need broader packaging. Cursor's primary durable instruction affordance is `.cursor/rules/*.mdc` with frontmatter such as `description` and `globs`, while newer VS Code/Copilot agent skills documentation aligns more closely with the directory + `SKILL.md` model.

Design consequence: Omegon should keep the on-disk plaintext skill bundle compatible with the `SKILL.md` de facto format and put Omegon-specific fields behind optional extensions, not required schema. Prefer YAML frontmatter for maximum portability; tolerate TOML frontmatter for existing Omegon skills as a local extension.

Omegon-specific metadata should be optional and namespaced where possible:

```yaml
---
name: postmortem
description: Write and review incident postmortems
# portable fields above; Omegon extensions below
x-omegon:
  activation: intent_detected
  profile: [docs]
  project_signals: ["incidents/**/*.md"]
---
```

Existing top-level Omegon fields (`activation`, `profile`, `project_signals`, `trusted_paths`, etc.) can remain supported for compatibility, but export/import should be able to normalize to the portable core plus `x-omegon` extensions.

Cursor compatibility is not the same as native skill compatibility. Cursor rules are persistent instruction rules, not necessarily callable skill bundles. Omegon can import `.cursor/rules/*.mdc` as source material for a local skill, but should not treat Cursor's rule schema as the canonical skill schema.

## Skill Bundles, Scripts, and Callable Helpers

Claude/Codex-style skills are not only markdown guidance; they can also package scripts and reference resources beside `SKILL.md`. Omegon should support that shape for plaintext skills, but with a strict distinction between **guidance** and **executable capabilities**.

A plaintext skill directory may contain:

```text
.omegon/skills/<name>/
  SKILL.md
  plugin.toml
  scripts/
    helper.py
    transform.sh
  resources/
    template.md
    examples/
```

The skill guidance may instruct the agent to use these bundled resources. The skill registry should index the skill root and expose resource metadata, but bundled scripts are not automatically promoted to first-class tools.

### Callable helper policy

There are three levels of executable support:

1. **Reference scripts** — files the agent may run through normal shell/tool surfaces after reading the skill. This is the default for local plaintext skills.
2. **Declared helper actions** — scripts declared in skill metadata with command, args schema, working-directory policy, and safety metadata. These may be surfaced as reviewed helper affordances later.
3. **Extension tools** — durable callable capabilities owned by the extension/tool registry. These remain the right home for stable, reusable APIs.

Do not duplicate the extension tool registry inside the skill registry. If a helper needs durable tool identity, argument schema, permissioning, streaming, or cross-session distribution, it should graduate into an extension tool or command registry entry.

### Security and provenance

Skill-bundled scripts are executable code and need stronger handling than markdown guidance:

- The registry must preserve skill root provenance so scripts/resources resolve relative to the owning skill.
- The agent should read/inspect local scripts before running them unless already trusted by policy.
- Script execution should use existing process/tool surfaces with workspace and approval checks; skills do not bypass sandboxing.
- Extension-packaged helper scripts are immutable package resources. Updating them means updating the extension or creating a local override.
- Local/project skill helper scripts are editable and reloadable with the skill.

### Manifest extension sketch

`SkillManifest` may eventually grow an optional helpers/resources section:

```toml
[[helpers]]
name = "extract-text"
description = "Extract text from an input document"
command = "python3"
args = ["scripts/extract_text.py", "{input}"]
inputs = [{ name = "input", kind = "path", required = true }]
safety = "read_only"

[[resources]]
name = "postmortem-template"
path = "resources/postmortem-template.md"
kind = "template"
```

Initial implementation can defer declared helper actions and simply support skill-root resource discovery plus guidance that references scripts. This preserves the Claude Code affordance without prematurely building a second tool registry.

## Open Questions

- [assumption] Installed extension manifests can expose enough metadata for extension-provided skill discovery without launching the extension process.
- [assumption] Prompt assembly can preserve current skill injection order after moving from raw `Vec<String>` to structured descriptors.
- [assumption] Existing `/skills` output can be backed by the unified registry without breaking operator workflows.
- Should extension-provided skills require an explicit manifest field, or is a conventional sidecar path acceptable as a compatibility fallback?
- Should user-local plaintext skills outrank extension-provided skills, or should extension skills outrank user-local but remain below project overrides? Current decision: user-local outranks extension because operator-owned guidance should override packaged defaults.

## Metadata Format Decision

Omegon-authored `SKILL.md` files use YAML frontmatter as the canonical format:

```yaml
---
name: postmortem
description: Write and review incident postmortems
x-omegon:
  activation: intent_detected
  profile: [docs]
---
```

TOML frontmatter remains supported as a compatibility format for existing Omegon skills and local operator preference:

```toml
+++
name = "postmortem"
description = "Write and review incident postmortems"

[x-omegon]
activation = "intent_detected"
profile = ["docs"]
+++
```

Rationale: YAML is the de facto `SKILL.md` frontmatter convention across Claude/Codex/VS Code-style agent skills and Markdown-adjacent rule systems. TOML remains preferable for machine-owned Omegon config such as `plugin.toml`, but `SKILL.md` optimizes for cross-agent portability and human authoring. Export/import should preserve portable `name` and `description` fields and place Omegon-specific metadata under `x-omegon` when producing portable bundles. Existing top-level Omegon metadata fields remain accepted for backward compatibility.

## Implementation Notes

Likely file scope:

- `core/crates/omegon/src/skills.rs`: descriptor/source/provider/resolver types, plaintext provider discovery, list/status output.
- `core/crates/omegon/src/plugins/registry.rs`: replace directory walking with resolved skill prompt fragments.
- `core/crates/omegon/src/extension_registry.rs` and/or extension manifest parsing module: expose installed extension skill descriptors without assuming extension registry owns skill lifecycle.
- `core/crates/omegon/src/control_runtime.rs`: `/skills` output should show provider/source/editability/shadowing.
- Tests for precedence, shadowing, project override path, extension skill projection, and reload.

## Non-Goals

- Do not move local project skills to top-level `skills/`.
- Do not publish every local/imported skill to armory.
- Do not make the extension registry responsible for plaintext skill mutation.
- Do not infer arbitrary extension docs as skills.
- Do not require recompilation to update plaintext skills.

## Open Questions
