# Nex capability resolver

## Summary

Nex should support agent-driven capability acquisition as a resolver over the primitives it already owns: host checks, declarative Nex profiles, Nix package overlays, OCI runtime materialization, and Omegon extensions. It should not become a generic package manager and should not introduce a broad operator command surface.

The harness/agent should be able to ask:

```text
Is capability X available?
If not, can an existing Nex profile, a project-local profile overlay, or an extension install satisfy it?
What should I ask the operator to approve?
```

## Existing Nex model

Current implementation lives under:

```text
core/crates/omegon/src/nex/profile.rs
core/crates/omegon/src/nex/manifest.rs
core/crates/omegon/src/nex/registry.rs
core/crates/omegon/src/nex/container.rs
core/crates/omegon/src/nex/spawn.rs
core/crates/omegon/src/nex/compose.rs
```

Nex already models declarative execution environments:

```rust
NexProfile {
    name,
    profile_hash,
    base_domain,
    overlays,
    resource_limits,
    capabilities,
    image_ref,
    signed_by,
}
```

Existing domains:

```text
chat
coding
coding-python
coding-node
coding-rust
infra
full
custom
```

Existing custom profile paths:

```text
~/.config/omegon/nex/*.toml
<project>/.omegon/nex/*.toml
```

Existing manifest overlay shape:

```toml
[profile]
name = "coding-d2"
base = "coding"

[overlays.diagramming]
packages = ["d2"]
```

This means the capability resolver should extend Nex's existing profile/overlay model, not create a second unrelated profile system.

## Non-goals

Do not add:

```text
omegon package ...
/package ...
omegon nex package ...
```

Do not implement a generic wrapper around:

```text
brew
cargo
pipx
npm
pnpm
uv
helm
```

Nex may materialize those tools as binaries inside a profile, but those tools remain responsible for their own ecosystems once present.

## Capability keys

Normalize agent requests into capability keys:

```text
binary:d2
binary:piper
binary:podman
binary:docker
extension:omegon-voice
extension:scratchpad
runtime:python
runtime:node
runtime:rust
service:ollama
```

Short forms may resolve to obvious binary keys:

```text
d2 -> binary:d2
piper -> binary:piper
omegon-voice -> extension:omegon-voice
```

## Resolver model

A capability may be available from several locations:

```rust
enum CapabilityLocation {
    HostPath { path: PathBuf, version: Option<String> },
    NexProfile { profile: String, image_ref: Option<String> },
    NexOverlaySuggestion { base: NexDomain, packages: Vec<String> },
    Extension { name: String },
}
```

Resolution returns evidence and recommendations:

```rust
struct CapabilityResolution {
    capability: CapabilityKey,
    status: CapabilityStatus,
    locations: Vec<CapabilityLocation>,
    recommendations: Vec<CapabilityRecommendation>,
    diagnostics: Vec<CapabilityDiagnostic>,
}

enum CapabilityRecommendation {
    UseProfile { profile: String },
    CreateProjectProfile { manifest: String },
    InstallExtension { name: String },
}
```

## Operations

MVP operations:

```text
nex.capability.check
nex.capability.resolve
```

`install` is deferred until the approval/execution seam is designed against HostAction policy. First pass can recommend profile manifests or extension installs without mutating the machine.

### check

Answers whether a capability is currently available.

Checks:

- host PATH for `binary:*`;
- active/selected Nex profile metadata for known included tools;
- installed extension registry for `extension:*`.

### resolve

Answers how a missing capability can be satisfied.

Resolution sources:

- already-available host path;
- available built-in Nex domain;
- custom Nex profile loaded by `NexRegistry`;
- project-local overlay manifest suggestion;
- installed extension or Armory extension suggestion.

## Catalog shape

Add a small static catalog that maps capabilities to Nex-native possibilities.

Suggested path:

```text
data/nex-capabilities.toml
```

Example:

```toml
[capabilities."binary:d2"]
commands = ["d2"]

[[capabilities."binary:d2".overlays]]
base = "coding"
packages = ["d2"]
profile_name = "coding-d2"

[capabilities."binary:piper"]
commands = ["piper"]

[[capabilities."binary:piper".overlays]]
base = "coding-python"
packages = ["piper-tts"]
profile_name = "voice-piper"

[capabilities."extension:omegon-voice"]
extension = "omegon-voice"
armory = true
```

Keep the first catalog small. Initial candidates:

```text
binary:d2
binary:piper
binary:podman
binary:docker
extension:omegon-voice
extension:scratchpad
```

## Project-local profile recommendation

If `binary:d2` is missing, a resolver may return a manifest suggestion:

```toml
[profile]
name = "coding-d2"
base = "coding"

[overlays.diagramming]
packages = ["d2"]
```

Operator-facing summary:

```text
Capability binary:d2 is missing.
Nex can satisfy it with a project-local profile overlay:
  profile: coding-d2
  base: coding
  packages: d2

This would create .omegon/nex/coding-d2.toml after approval.
```

## Agent-facing tool

Use one agent tool rather than a new slash surface:

```text
nex_capability
```

Schema:

```json
{
  "action": "check|resolve",
  "capability": "binary:d2",
  "profile": "optional-profile-name"
}
```

No install action in MVP.

## Operator surface

No new top-level slash command.

No `omegon package` CLI.

Optional diagnostic CLI later, if needed:

```text
omegon nex capability check d2
omegon nex capability resolve d2
```

This is debugging only, not primary UX.

## Approval and mutation boundary

MVP is read-only except for explicit future profile creation. Mutating actions must route through host approval.

Future mutation candidates:

- create project-local `.omegon/nex/*.toml` profile from a recommendation;
- install extension from Armory;
- pull/build OCI image for a Nex profile.

All must show:

```text
capability
profile/extension affected
files or runtime state changed
commands/images involved
risk/scope
```

## Package manager boundary

Nex may include local tooling such as `cargo`, `pipx`, `uv`, or `pnpm` in a profile. After they are available as binaries, they own their ecosystems.

Good:

```text
Nex profile overlay includes pipx.
Agent later uses pipx under normal host/container execution policy.
```

Bad:

```text
Nex becomes a pipx/npm/cargo package search and install frontend.
```

MVP provider vocabulary should therefore avoid package-manager ecosystem providers. Use Nex overlays and extension installs first.

## Implementation plan

### Phase 1 — Design and catalog

- Add this design document.
- Add `data/nex-capabilities.toml` with a minimal catalog.
- Add parser/types for capability catalog entries.
- Test catalog load and capability key normalization.

### Phase 2 — Check operation

- Implement host PATH binary checks using argument arrays/no shell.
- Implement installed-extension checks if existing registry APIs are available.
- Include selected Nex profile metadata in responses where possible.

### Phase 3 — Resolve operation

- Use `NexRegistry` to find matching built-in/custom profiles.
- Use catalog overlay entries to generate project-local profile suggestions.
- Use extension/Armory metadata to suggest extension installation.
- Return diagnostics when no route exists.

### Phase 4 — Agent tool

- Add `nex_capability` with `check` and `resolve`.
- Keep responses structured and concise.
- Add tests for missing binary, available binary, and overlay recommendation.

### Phase 5 — Future mutation seam

Only after read-only resolution is useful:

- add approved project-profile creation;
- add approved extension install recommendation execution;
- add OCI image materialization diagnostics.

## Acceptance criteria

- Agent can ask whether `binary:d2` is available.
- Resolver detects host binary availability.
- Resolver can suggest a Nex project overlay profile when a catalog mapping exists.
- Resolver can check whether an extension capability is already installed.
- Resolver can suggest a scratchpad/voice extension install without implementing a generic package manager.
- No `omegon package` command is added.
- No new slash command is added.
- Package managers are treated as profile-contained binaries, not ecosystems Nex owns.

## MVP task plan

### 1. Catalog data and parser

Files:

```text
data/nex-capabilities.toml
core/crates/omegon/src/nex/capabilities.rs
core/crates/omegon/src/nex/mod.rs
```

Tasks:

- Define `CapabilityKey` with `kind` and `name`, plus normalization from short strings such as `d2` and `omegon-voice`.
- Define catalog entry structs for host commands, Nex overlay recommendations, and extension recommendations.
- Add `data/nex-capabilities.toml` with a deliberately small first catalog:

```text
binary:d2
binary:piper
binary:podman
binary:docker
extension:omegon-voice
extension:scratchpad
```

- Load the catalog with `include_str!` so resolution works in installed binaries.
- Add unit tests for catalog load, key normalization, and missing/unknown capability behavior.

### 2. Host and registry check operation

Files:

```text
core/crates/omegon/src/nex/capabilities.rs
```

Tasks:

- Implement `CapabilityResolver::check(capability, context)`.
- For `binary:*`, check host PATH using `std::env::split_paths` and executable-file detection. Do not spawn a shell.
- For `extension:*`, inspect installed extension metadata if a suitable host API exists; otherwise return `Unknown` plus an explicit diagnostic that extension-registry integration is not wired yet.
- Accept an optional selected/profile name and use `NexRegistry::resolve` to report matching Nex profile evidence. First pass should not infer every binary inside an OCI image; only report explicit catalog/profile/overlay matches.
- Add tests using temporary PATH directories for available/missing binaries.

### 3. Resolve operation and project profile suggestions

Files:

```text
core/crates/omegon/src/nex/capabilities.rs
```

Tasks:

- Implement `CapabilityResolver::resolve(capability, context)`.
- Return existing host/profile evidence when available.
- For catalog overlay entries, generate a project-local Nex manifest suggestion string.
- For extension entries, generate an `InstallExtension` recommendation without executing it.
- Include clear diagnostics when no route exists.
- Add tests for `binary:d2` producing a `coding-d2` project-profile suggestion when missing.

### 4. Agent tool surface

Files:

```text
core/crates/omegon/src/tools/mod.rs
core/crates/omegon/src/tools/nex_capability.rs
```

Tasks:

- Add tool `nex_capability` with schema:

```json
{
  "action": "check|resolve",
  "capability": "binary:d2",
  "profile": "optional-profile-name"
}
```

- Return structured JSON text with status, evidence locations, recommendations, and diagnostics.
- Refuse mutation actions such as `install`, `create`, or `apply` with a clear message: MVP is read-only.
- Add tests for tool schema and read-only refusal if the tool framework has narrow test hooks.

### 5. CLI diagnostic hook, optional but useful

Files:

```text
core/crates/omegon/src/main.rs
```

Tasks:

- Add only if implementation friction is low:

```text
omegon nex capability check <capability>
omegon nex capability resolve <capability>
```

- Keep it explicitly diagnostic. Do not add slash command equivalents.
- Output human-readable by default; allow JSON only if existing Nex command patterns already support it.

### 6. Documentation and lifecycle state

Files:

```text
design/nex-capability-resolver.md
docs/internal/slash-cli-command-map.md
CHANGELOG.md
```

Tasks:

- Update this design with implementation notes after Phase 1-4.
- Note in the slash/CLI map that Nex capability resolution is agent/tool-first, not slash-first.
- Add changelog entry only when behavior/tooling changes land.

## MVP cut line

Ship MVP when these are true:

- `nex_capability { action: "check", capability: "binary:d2" }` reports host availability accurately.
- `nex_capability { action: "resolve", capability: "binary:d2" }` recommends a Nex overlay profile when missing.
- `extension:scratchpad` can produce a non-mutating extension install recommendation.
- Unknown capabilities produce explicit diagnostics.
- No machine mutation happens.
- No new slash command is visible.
- Tests cover catalog load, host binary detection, overlay suggestion, extension recommendation, and unknown capability diagnostics.

## MVP risks

- Extension-registry access may not have a clean API yet. Do not block MVP on that; return a diagnostic and recommendation path.
- Inferring actual contents of OCI images is out of scope. Treat catalog mappings as recommendations, not proof.
- Nix package names may vary by platform/channel. Keep initial catalog small and conservative.
- Host PATH checks can lie under sandboxed execution. Include the checked PATH in diagnostics when useful.
