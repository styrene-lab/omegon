
+++
id = "5845a539-b0b9-4c34-a148-3ceced3d8116"
kind = "design_node"

[data]
title = "Omegon Backend Surfaces for Auspex"
status = "exploring"
issue_type = "epic"
priority = 2
dependencies = []
open_questions = []
+++

## Overview


Build the Omegon daemon backend substrate for Auspex and other rich clients. Auspex is the UI/control-plane product; Omegon should expose the runtime, lifecycle, capability, readiness, ACP/session, and assistant-run APIs that Auspex consumes.

The near-term work remains backend-first: stabilize headless HTTP/ACP/IPC surfaces before expanding Auspex UI screens. This node no longer tracks a separate pure-Omegon Dioxus console. The previous "Omegon Console" framing is retained only as historical shorthand for backend/API work now intended for Auspex.

Omegon should not become a Flynt clone, an Auspex clone, or a second orchestration shell. It should be the authoritative local agent runtime and semantic state source underneath Auspex.

## Position

Use a two-channel integration model:

```text
Auspex UI / Control Plane
  ├─ ACP-over-WebSocket for interactive Omegon agent sessions
  └─ Omegon-native HTTP/WebSocket API for runtime, lifecycle, settings, memory, capabilities, and project state

omegon serve
  ├─ ACP session bridge
  ├─ native API router
  ├─ lifecycle read/mutation services
  ├─ runtime/provider/extension/secrets surfaces
  ├─ memory/codebase projections
  └─ workspace/session orchestration
```

ACP is the agent conversation/session protocol. The native API is Omegon's runtime/control-plane API for Auspex.

## Why not ACP-only

ACP is ideal for:

- session creation
- prompt submission
- assistant/tool/plan streaming
- model/mode/thinking session config
- editor/client-mediated tool operations

ACP is not the best primary shape for:

- browser app auth/session management
- runtime health and daemon readiness
- lifecycle dashboards
- workspace lease inventory
- provider/extension/secrets settings screens
- memory/codebase browsing
- durable history and analytics
- REST/SSE/WebSocket-friendly dashboard refresh

ACP ext_methods remain useful for Zed/Flynt/editor compatibility. Auspex should use normal Omegon APIs for app/control-plane state and embed ACP only where agent-session semantics are needed.

## Repo strategy

Start in this repository.

Proposed initial layout:

```text
core/crates/omegon-web/        # daemon HTTP/WebSocket/API backend
auspex/                         # UI/control-plane consumer of these backend surfaces
```

If workspace constraints favor crates only, the first app can live under:

```text
core/crates/omegon-backend-surfaces/
```

Do not split into a separate repository yet. The API shape is still evolving and needs atomic backend/frontend iteration. Split only after:

1. `omegon serve` exposes a stable versioned API.
2. ACP-over-WebSocket behavior is stable.
3. DTOs are generated or published as a stable package/crate.
4. The UI can tolerate daemon version skew.
5. Release cadence/distribution needs diverge from core Omegon.

## Backend-first phases

### Phase 1: daemon runtime API foundation

Expose stable, headless-safe daemon endpoints:

```text
GET /api/healthz
GET /api/readyz
GET /api/runtime/status
GET /api/runtime/capabilities
GET /api/providers/status
GET /api/extensions
GET /api/workspaces/leases
```

Goals:

- browser can determine whether daemon is usable
- UI can render runtime/provider/extension readiness without scraping logs
- reuse existing ACP runtime/status/capability logic where possible

### Phase 2: lifecycle read API

Expose lifecycle projections from the services created by the lifecycle extraction pass:

```text
GET /api/lifecycle/snapshot
GET /api/lifecycle/design
GET /api/lifecycle/design/:id
GET /api/lifecycle/design/ready
GET /api/lifecycle/design/blocked
GET /api/lifecycle/design/frontier
```

Backends:

- `LifecycleReadHandle`
- `lifecycle::query`
- `design::read_node_sections`

The same DTO builders should also back ACP ext_methods:

```text
_lifecycle/snapshot
_lifecycle/design/list
_lifecycle/design/get
_lifecycle/design/ready
_lifecycle/design/blocked
_lifecycle/design/frontier
```

### Phase 3: ACP-over-WebSocket sessions

Expose browser-compatible ACP transport:

```text
WS /api/acp
```

or, if session preallocation is cleaner:

```text
POST /api/sessions
WS /api/sessions/:id/acp
```

Auspex acts as an ACP client for conversation sessions. This should reuse existing ACP semantics rather than inventing a separate chat stream.

### Phase 4: daemon event stream

Add a non-ACP event stream for app dashboards:

```text
GET /api/events
```

Candidate events:

```text
runtime.status_changed
lifecycle.snapshot_changed
workspace.lease_changed
provider.status_changed
extension.status_changed
session.started
session.ended
```

ACP session updates remain scoped to active agent conversations. Daemon events are for app state.

### Phase 5: service-backed writes

Only after read DTOs and permission policy are stable, expose writes through domain services:

```text
POST  /api/lifecycle/design
PATCH /api/lifecycle/design/:id/status
POST  /api/lifecycle/design/:id/questions
POST  /api/lifecycle/design/:id/decisions
POST  /api/lifecycle/design/:id/implement
```

Backend:

- `LifecycleMutationService`

Do not call tool handlers from API endpoints.

## Future Auspex UI integration shape

Potential app routes:

```text
/sessions          # ACP conversation sessions
/lifecycle         # design tree dashboard
/lifecycle/:id     # design node detail
/runtime           # daemon/provider/extension status
/settings          # profile, secrets, extensions, providers
/memory            # memory/codebase mind browser
```

Potential app modules:

```text
auspex/src/
  app.rs
  routes/
    sessions.rs
    lifecycle.rs
    runtime.rs
    settings.rs
    memory.rs
  components/
    agent_panel.rs
    tool_call_card.rs
    lifecycle_node_card.rs
    status_badge.rs
  api/
    client.rs
    acp_ws.rs
    dto.rs
  state/
    session_store.rs
    runtime_store.rs
    lifecycle_store.rs
```

## Correctness constraints

- Headless APIs must not depend on TUI state.
- ACP remains the source for agent session streaming; do not duplicate conversation semantics in a bespoke chat protocol.
- Native API and ACP ext_methods must share DTO builders where they expose the same domain state.
- APIs must call read models/services, not provider-facing tool handlers.
- Secrets are write-only; never expose secret values.
- Mutating endpoints need permission/capability policy before product use.
- Browser-facing APIs need explicit auth/origin policy before remote exposure.

## First implementation slice

1. Create or extend the backend API module under `omegon-web`/`omegon serve`.
2. Add shared lifecycle read DTO builders.
3. Expose:

```text
GET /api/runtime/capabilities
GET /api/lifecycle/snapshot
GET /api/lifecycle/design/ready
GET /api/lifecycle/design/blocked
GET /api/lifecycle/design/frontier
```

4. Mirror the lifecycle read DTOs as ACP ext_methods for Zed/Flynt compatibility.
5. Add tests for DTO shape and endpoint/ext_method parity.

## Research

### Substrate evaluation: persistent agent control-plane pattern

Conceptual review of Hermes/OpenClaw-style systems shows the relevant product pattern is not chat UI or nomenclature; it is a long-lived harness becoming a persistent personal/organizational operator. The common substrate is: (1) daemonized agent runtime, (2) persistent memory, (3) durable/scheduled/event-driven work, (4) tool/capability management, (5) isolated subagents or execution backends, and (6) a management surface for sessions, tasks, runs, memory, and capabilities.

Omegon's advantage is that its Rust-native harness already has stronger primitives than these systems usually expose: lifecycle authority (design tree, OpenSpec, drift findings, mutation services), ACP session semantics, native daemon APIs, memory facts/episodes/codebase search, cleave/delegation, provider/tool/extension surfaces, and emerging autonomous tasking/sentry concepts. The console should therefore compete on operational depth rather than adapter breadth.

Architectural direction: use ACP-over-WebSocket strictly for interactive agent sessions and conversation semantics; use native HTTP/WebSocket daemon APIs for runtime, lifecycle, memory, settings, tasking, run history, and dashboard state. Durable autonomy should be modeled as persistent executable objectives: task becomes actionable via trigger, sentry claims it, agent executes with bounded spec/checkpoints/budget, results update lifecycle/memory/evidence, and Auspex shows truth.

Implications for this design node: first backend substrate work should stabilize read DTOs and control-plane APIs before Auspex UI integration. The near-term foundation remains ACP lifecycle read expansion plus native HTTP mirrors. The strategic follow-on is durable task/sentry APIs, not messaging-gateway breadth. Messaging, remote/container backends, and multimodal adapters are later ingress/egress plugins over the core substrate, not the core product.

### Ecosystem substrate: Armory, skills, plugins, and local extensions

The existing ecosystem already provides a stronger assistant substrate than a monolithic assistant app. Armory discovery (`docs/armory-discovery.md`) treats `styrene-lab/omegon-armory` as the upstream catalog for extensions, plugins, skills, and agents, with installed-state detection across user and project scopes. Locally, `../omegon-extensions` shows concrete extension families: browser automation (`omegon-browser`), trusted package/devenv operations (`omegon-nex`), voice STT/TTS (`omegon-voice`), local image generation (`scry`), Rust/TypeScript extension SDKs/starter templates, and deterministic TDD/evidence tooling in this repo (`extensions/omegon-tdd-savepoint`).

This changes the assistant assessment: the superior assistant should not bake every capability into the core runtime. Core Omegon should provide the durable runtime, policy, lifecycle, memory, tasking, ACP sessions, and extension loading/control plane. Skills/plugins/extensions provide replaceable capabilities and domain expertise. Auspex should expose this ecosystem as an operator surface: discover installed/available assets, show capability health, configure secrets/settings, explain trust/permissions, enable/disable skills or extensions, and bind them to durable agent tasks or assistant profiles.

Observed capability classes to model explicitly:

- **Browser/workflow automation** — `omegon-browser` plus browser recording/recipe tools provide web search, page interaction, and repeatable browser workflows. Treat as guarded ingress/action capability, not core assistant logic.
- **Package/devenv substrate** — `omegon-nex` is a trusted package-provider extension with host-action permissions and Nex-backed helper installation/devenv inspection. This is how the assistant safely acquires local capabilities instead of instructing operators to install tools manually.
- **Voice/multimodal interface** — `omegon-voice` contributes streaming voice capability, microphone STT, speaker TTS, voice profiles, and voice-to-daemon-event bridging. Voice should be a modality plugin over the same session/task substrate, not a separate assistant.
- **Image generation/creative tools** — `scry` demonstrates heavyweight local generation as an extension with models, galleries, previews, secrets, and config. Auspex needs generic extension widget/config/status surfaces backed by Omegon APIs so this class does not require bespoke UI.
- **Evidence/TDD tooling** — `omegon-tdd-savepoint` and the `.omegon/evidence` substrate show deterministic evidence records as first-class assistant memory, useful for trustable autonomous work.
- **OpenAPI tool provider** — project-local OpenAPI specs can become tools without writing an extension; this is the lightweight integration path for APIs and should appear in capability inventory.
- **Skills and agents** — bundled skills (`rust`, `typescript`, `python`, `security`, `openspec`, `oci`, `git`, etc.) and Armory/catalog agents are declarative behavior/capability packages. The assistant should assemble profiles from these rather than treating prompts as the only specialization mechanism.
- **Extension minds** — `docs/extension-byom-system.md` defines per-extension persistent minds. This is important for a personal/org assistant: domain integrations can own cross-repo knowledge while Omegon composes recall and provenance.

Strategic implication: building our own superior assistant means composing a durable harness plus an inspectable capability graph. Auspex should answer through Omegon APIs: what can this assistant do, why is it allowed, what knowledge does it carry, what tasks is it running, what extensions/skills are active, what secrets/config are missing, and what evidence supports its claims. That is a stronger substrate than broad hard-coded adapters.

### Armory organization model for assistant composition

A local inspection of `../omegon-armory` shows that Armory is already more than a flat extension registry. It is the upstream composition catalog for assistant identities and capability bundles:

- `registry.toml` — extension discovery and install metadata, including categories such as media, comms, knowledge, automation, and remote execution.
- `catalog-registry.toml` plus `catalog/*/agent.toml` — deployable agent bundles with persona directives, optional mind facts, extension dependencies, model/settings defaults, workflows, secrets, and triggers.
- `profiles/*/profile.toml` — curated capability stacks such as `rust-shop`, `typescript-shop`, `python-shop`, `security-review`, `infra-operator`, and `docs-vault`; these already encode persona/tone/skill/extension dependencies and activation policy.
- `skills/*/plugin.toml` plus `SKILL.md` — reusable behavioral/craft modules.
- `personas/*/plugin.toml` and `tones/*/plugin.toml` — identity and output-style modules.
- `examples/*/plugin.toml`, `machine-profiles/*/armory.toml`, `materialization-payloads/*/armory.toml`, and `workstations/*` — installable examples, machine targets, payloads, and environment bundles.
- `dist/*.json` and OCI-style index directories — generated/distribution artifacts that indicate Armory is trending toward packaged capability indexes, not only raw GitHub traversal.

This changes the backend shape again: the upstream discovery API should not expose only `extensions | plugins | skills | agents`. The assistant app needs a normalized **capability catalog** with at least these classes:

```text
extension       executable tool/widget integration
skill           procedural/craft guidance
persona         identity + mind seed + tool/skill policy
tone            output style modifier
agent_bundle    runnable assistant template with settings/workflows/triggers
profile         composable capability stack for project/domain work
machine_profile deploy target/environment profile
payload         materialization/deployment payload
workstation     development/runtime environment bundle
openapi_tool    project-local API-derived toolset
```

Profiles are especially important. They are closer to the product primitive we need than raw agents: a profile is a reusable capability recipe (`defaults` + dependency graph + activation policy), while an agent bundle is a runnable specialization (`persona` + settings + extensions + secrets + workflows + triggers). The future AssistantProfile should probably be an Omegon-native refinement of both:

```text
AssistantProfile = Armory profile dependencies
                 + optional catalog agent persona/mind/workflow defaults
                 + local project overlays
                 + installed extension/skill health
                 + secrets/config readiness
                 + trust/permission policy
                 + evidence/tasking budgets
```

Required organization for the backend substrate:

1. Keep the installed-extension read model as the first local capability health slice.
2. Add an Armory catalog projection that normalizes upstream items into capability nodes, preserving source path, kind, category/domain, version, install hint, installed state, and dependency edges when available.
3. Treat `profiles/*/profile.toml` as first-class discovery input; current `ArmoryKind` does not include profiles, but assistant composition depends on them.
4. Treat catalog agents as templates, not the only assistant model. Their triggers/workflows/secrets should seed AssistantProfile and durable task defaults.
5. Add readiness projection: for any profile/agent/template, show installed/missing dependencies, missing secrets/config, disabled/degraded extensions, and permission posture before execution.
6. Surface Armory distribution/index artifacts later so Auspex can browse cached/signed catalog snapshots instead of live GitHub traversal only.

## Decisions

### Decision: Backend-first, Auspex UI later

**Status:** decided
**Rationale:** Auspex UI screens should consume stable Omegon daemon APIs. Building UI against file formats or internal tool handlers would recreate product-boundary duplication.

### Decision: Use ACP plus native API

**Status:** decided
**Rationale:** ACP is the right session protocol; native API is the right application control plane for Auspex and other rich clients.

### Decision: Keep the app in-repo initially

**Status:** decided
**Rationale:** The API and DTOs are still evolving. In-repo development enables atomic backend/frontend changes and avoids version skew.

### Decision: Model the assistant as a profile plus capability graph, not only a session UI

**Status:** decided
**Rationale:** Durable autonomous work needs an execution identity richer than a prompt. Assistant profiles should bind model/posture defaults, skills, extensions, OpenAPI-generated tools, extension minds, secrets/config requirements, permission/trust policy, evidence expectations, and tasking budgets. Sentry-style durable tasks should execute configured assistant profiles, not raw prompts with ad hoc tool access.

### Decision: Separate assistant composition/operations from Auspex deployment supervision

**Status:** decided
**Rationale:** The Omegon assistant app should answer what an assistant is, what it can do, what it knows, what work/evidence it owns, and what trust/policy envelope constrains it. Auspex should answer where and how that assistant is running, whether the process/pod fleet is healthy, and how deployment lifecycle actions such as spawn, restart, drain, scale, logs, and upgrade are performed. The shared boundary should be an AssistantProfile/AgentBundle artifact: compose and inspect locally in the assistant app, then hand off to Auspex for durable/fleet deployment when needed.

## Adversarial Assessment

### Gap: The current backend plan still over-indexes on runtime/lifecycle reads and under-specifies capability inventory

The Phase 1/2 endpoint list covers runtime, providers, extensions, workspaces, and lifecycle projections, but it does not yet define a single capability graph that unifies installed extensions, Armory-available assets, skills, plugins, catalog agents, OpenAPI-generated tools, extension minds, widgets, trust classifications, permissions, config, secrets, and health. Without that graph, Auspex can show daemon state but cannot answer the core assistant question: what can this assistant do, why is it allowed, and what is missing?

Required correction: add a capability-profile substrate before or alongside durable tasking. The backend should expose capability inventory DTOs and assistant-profile bindings rather than scattering capability status across unrelated runtime, extension, skill, and settings pages.

### Gap: "Assistant" is not yet a first-class persisted domain object

The document describes sessions, lifecycle, runtime, settings, memory, and future tasking, but it does not define the persisted assistant/profile unit that a session or task runs as. That risks rebuilding the same weak pattern seen in broad assistant systems: prompt + tool bag + cron trigger. Omegon's advantage is policy-aware composition, so the execution unit must include capability selection, trust posture, budgets, memory/mind attachments, and evidence requirements.

Required correction: introduce an assistant profile schema and make ACP session creation and sentry task execution reference a profile id.

### Gap: Durable tasking is mentioned strategically but not connected to lifecycle authority

The substrate evaluation says durable objectives should update lifecycle/memory/evidence, but the endpoint plan has no sentry/task/run history API and no explicit lifecycle binding model for autonomous work. If omitted, autonomous tasks become external job records rather than lifecycle-aware work items.

Required correction: durable task/run DTOs must include design node/OpenSpec/task references, assistant profile id, trigger source, budget ledger, checkpoint/session id, evidence outputs, and completion/failure reconciliation policy.

### Gap: Extension UI contribution is acknowledged indirectly but not made a backend contract

Local extensions already declare config, secrets, capabilities, and widgets. The current plan says the future app has settings/memory/runtime pages, but it does not define generic extension widget/config/status endpoints. Without this, every serious extension becomes bespoke UI work and the capability ecosystem will not scale.

Required correction: define generic extension contribution DTOs for tools, config schema, required/optional secrets, widgets, health, trust classification, host-action permissions, and extension minds.

### Gap: Trust and permission policy is deferred too broadly

The document correctly says mutating endpoints need permission/capability policy before product use, but a persistent assistant cockpit needs trust surfaced even for read/setup flows. Installing or enabling browser, package-manager, voice, image-generation, and host-action extensions changes the assistant's authority envelope before any mutation endpoint is called.

Required correction: capability inventory and assistant profiles must carry permission/trust summaries from the beginning. The UI should make authority legible: local-only vs networked, read vs mutate, host-action capable, secret-bound, voice/microphone capable, browser-action capable, and trusted-provider vs standard-extension.

### Gap: ACP/native API boundary is correct but incomplete for profile-scoped sessions

The document correctly keeps ACP for conversation/session semantics and native APIs for app control-plane state. Missing detail: ACP session creation needs a way to select an assistant profile and receive the profile/capability envelope that shaped the session. Otherwise Auspex cannot explain why a session had particular tools, skills, memory, or restrictions.

Required correction: either session creation (`POST /api/sessions`) or ACP initialize metadata should include `assistant_profile_id`, resolved capability set, and policy summary.

### Gap: Evidence is present in the ecosystem but absent from console success criteria

The local `.omegon/evidence` substrate and deterministic TDD savepoint extension suggest a stronger trust model for autonomous work. The current node does not require Auspex to show evidence records, claim provenance, or verification artifacts. That would waste a differentiator against adapter-heavy systems.

Required correction: add evidence summary/read APIs and require autonomous task results to link to evidence artifacts where available.

### Potential misunderstanding: Armory is not merely install UX

Treating Armory as a marketplace/search page would undersell it. Armory plus local installed-state detection is the catalog substrate for assembling assistant profiles. It should feed capability graph resolution, not only a browse/install screen.

### Potential misunderstanding: Extensions are not just tools

The local extension ecosystem includes config schemas, secrets, widgets, host actions, streaming modalities, extension minds, and trust classifications. A tool-only projection would flatten the ecosystem and force bespoke side channels later.

### Potential misunderstanding: More integrations are not the winning move

The goal is not to match broad assistant systems adapter-for-adapter. The winning move is to make every integration inspectable, policy-bound, profile-scoped, lifecycle-aware, and evidence-producing. Adapter breadth should remain plugin/Armory work over that substrate.

## Open Questions

- [assumption] `omegon-web` is the right crate for long-lived daemon HTTP/WebSocket APIs rather than introducing a new backend crate.
- [resolved] Do not build a separate Omegon Dioxus app under `apps/omegon-console/`; Auspex is the UI/control-plane product and consumes Omegon backend surfaces.
- [assumption] Browser auth/origin policy can start local-only and harden before remote daemon exposure.

## Open Questions
