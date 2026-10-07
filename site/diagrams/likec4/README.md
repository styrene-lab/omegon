# LikeC4 tooling

This directory contains the conversational architecture model in
[context.c4](context.c4). Select a named view according to the question:

The [working agreement](WORKING-AGREEMENT.md) defines how observed behavior,
accepted intent, design decisions, implementation evidence, and shared runtime
review relate.

| View | C4 level | Question |
| --- | --- | --- |
| `systemContext` — **Omegon — System context** | Level 1: systems and people, with an environment annotation | Who directs Omegon, which external systems supply capabilities, and where does project work happen? |
| `omegonInternals` — **Omegon — Internals** | Level 2: applications and data stores | Which applications own session history, lifecycle state, memory, and indexing? |
| `hostComponents` — **Omegon host — Components** | Level 3: components within the host | How does supervised work use context assembly, inference, tool execution, approval, memory, lifecycle, native extensions, and MCP? |

The context view shows the Operator directing Omegon through its native interface
or an ACP-compatible editor, such as Zed. Omegon requests inference from an external
provider and retrieves web information. A configured secrets store supplies secrets.
Native extension-connected systems and optional MCP servers supply additional
external capabilities. The Project workspace identifies the user-owned files and
command-execution environment where Omegon performs project work.

## First model walkthrough

The file has three blocks:

1. `specification` defines the element kinds: `person`, `system`,
   `externalSystem`, `externalEnvironment`, `application`, `component`, and `dataStore`.
   The person shape identifies the human; cylinders identify data stores.
   The workspace rectangle has a **User-owned environment** label to distinguish
   this nonstandard context annotation from a software system.
2. `model` declares the elements and their labeled relationships.
   In `operator = person 'Operator'`, `operator` is the reference ID, `person` is
   the kind, and `'Operator'` is the displayed title. Each element also has a
   description.
3. `views` defines `systemContext`, `omegonInternals`, and `hostComponents`.
   The model defines architecture facts; each view selects which elements to show.
   Explicit `include` lists keep the selected elements stable as the model grows.
   `autoLayout LeftRight` arranges the diagrams horizontally.

LikeC4 automatically includes relationships between the included elements, so the
view does not repeat the arrows. Here, arrows indicate directed usage or
dependency, not a complete dataflow: responses and results can travel back.
An ACP-compatible editor is an optional external path through which the Operator
directs work and reviews results. Zed is one example. The editor connects to Omegon
through ACP, as described in the
[ACP surface documentation](../../../docs/acp-surface.md). The direct Operator →
Omegon relationship represents the native interface.
The provider is outside Omegon's system boundary whether inference runs locally
or through a hosted service.

The model attaches external relationships to their most specific known owners.
C1 hides the applications and components, so LikeC4 projects their endpoints onto
`omegon`. Operator → ACP-compatible editor remains a direct external relationship.
The three workspace relationships merge into one C1 edge. A view-only override
labels it **Reads and updates project artifacts; runs commands**, preserving the
owner-specific model endpoints without a duplicate system-level relationship.

| Model endpoint | External interaction | Source evidence under `core/crates/omegon/src/` |
| --- | --- | --- |
| `omegon.host.interfaces` | Receives native Operator work and ACP editor tasks. | [tui/ui_actions.rs](../../../core/crates/omegon/src/tui/ui_actions.rs) (`SubmitPrompt`), [acp.rs](../../../core/crates/omegon/src/acp.rs) (`prompt` and streamed session updates) |
| `omegon.host.inference` | Requests model inference. | [provider_route_service.rs](https://github.com/styrene-lab/omegon/blob/13760f256662de5cbb8089d38990f7db3c93d89d/core/crates/omegon/src/provider_route_service.rs) (`ResolvedProviderRoute::stream`), [providers.rs](https://github.com/styrene-lab/omegon/blob/13760f256662de5cbb8089d38990f7db3c93d89d/core/crates/omegon/src/providers.rs) (provider bridge resolution) |
| `omegon.host.tools` | Finds and retrieves web information through built-in web tools. | [tools/web_search.rs](../../../core/crates/omegon/src/tools/web_search.rs) (search and fetch clients), [setup.rs](../../../core/crates/omegon/src/setup.rs) (`web-search` registration) |
| `omegon.host.extensions` | Requests additional external capabilities through native extensions. | [extensions/mod.rs](../../../core/crates/omegon/src/extensions/mod.rs) (native capability adaptation and RPC) |
| `omegon.host.mcp` | Discovers and invokes optional MCP capabilities. | [plugins/mcp.rs](../../../core/crates/omegon/src/plugins/mcp.rs) (`McpFeature`, discovery, tool adaptation, and server connections) |

These endpoints express logical responsibility. The web edge includes the
built-in clients that initiate search and fetch requests, rather than tool routing
alone. The extension edge identifies host-side integration; an extension may
perform the external interaction. It does not assert that every extension uses
generic `execute_tool` dispatch.

At the C1 system-context level, **Web information sources** groups search providers,
content extraction services, and websites by their shared purpose: supplying web
information to Omegon. The box represents a grouped external role, not a single
concrete deployed service. This view deliberately omits separate search and fetch
paths. Transport details, such as HTTP, do not establish system boundaries.

**Secrets store** represents a category of alternative backing stores: an OS
keyring, Vault, or another configured secrets engine. Omegon retrieves configured
secrets from the selected store. This model deliberately places the backing
secrets store outside Omegon's system boundary, whether the store is local or
remote. Secret resolution and secrets-engine adapters remain inside Omegon.

The secrets relationship deliberately starts at `omegon.host`: secret resolution
is a host-wide, cross-cutting capability shared by several modeled responsibilities.
Evidence includes provider credential resolution in
[providers.rs](https://github.com/styrene-lab/omegon/blob/13760f256662de5cbb8089d38990f7db3c93d89d/core/crates/omegon/src/providers.rs)
(`resolve_provider_binding_with_secrets`), search-key resolution in
[tools/web_search.rs](../../../core/crates/omegon/src/tools/web_search.rs)
(`resolve_key`), and extension secret resolution in
[setup.rs](../../../core/crates/omegon/src/setup.rs) (`resolve_extension_secrets`).
No single existing component owns all these clients. C1 projects this host-level
exception onto Omegon, preserving the selected external backing-store boundary.

**Extension-connected systems** is a generic category for additional independent
systems not already shown. Omegon requests their capabilities through extensions.
If an extension connects to a secrets store, web information source, inference
provider, or explicitly modeled MCP server, that system belongs to the existing
category and is not counted again here.
This intentionally broad category represents neither a single deployable system
nor a separate extension runtime. Refine it when distinct responsibilities matter
in a later view.

The [extension runtime guide](../../../docs/extensions.md#authoring-workflow)
documents the SDK's `execute_tool` contract as an integration mechanism. That
mechanism does not determine the C1 ownership boundary or guarantee networking
capabilities or permissions. The SDK, adapters, and release-coupled first-party
extensions, such as codescan, remain inside the Omegon C1 box. The internals view
expands that boundary to show applications and data stores.

**MCP servers** is a separate external role for independently supplied local or
remote capabilities exposed through Model Context Protocol. These optional servers
connect through **MCP integration**, a host component distinct from native
**Extension integration**. Local transport alone does not make an MCP server an
Omegon-owned application.

**Project workspace** (`workspace`, kind `externalEnvironment`) is a nonstandard
environment annotation added to C1. It represents user-owned repository/project
files and their command-execution environment, rather than a software system,
Docker container, or deployment node. The muted rectangle and explicit
**User-owned environment** label identify this role.

Omegon-owned session history, lifecycle ledger, durable memory, and derived index
remain separate logical stores, even when colocated with project files on disk.
The authored design and change documents belong to the workspace; the lifecycle
ledger records their managed state and transitions. The external Secrets store
retains its own role, including when backed by a local OS keyring.

| Model relationship to `workspace` | Source evidence |
| --- | --- |
| `omegon.host.tools` — Reads and changes project files; runs commands | Built-in [read](../../../core/crates/omegon/src/tools/read.rs), [write](../../../core/crates/omegon/src/tools/write.rs), and [edit](../../../core/crates/omegon/src/tools/edit.rs) tools, plus [bash.rs](../../../core/crates/omegon/src/tools/bash.rs) (`Command::current_dir`) |
| `omegon.host.lifecycle` — Reads and updates design and change artifacts | [lifecycle/design.rs](../../../core/crates/omegon/src/lifecycle/design.rs) (`update_node`) and [lifecycle/spec.rs](../../../core/crates/omegon/src/lifecycle/spec.rs) (`read_change`, `propose_change`) |
| `omegon.codescan` — Reads code and knowledge sources | The [codescan process](../../../extensions/omegon-codescan/src/main.rs) passes its workspace to `Indexer::run_with_cancel`; [indexer.rs](../../../core/crates/omegon-codescan/src/indexer.rs) reads both source kinds before indexing |

Only C1 includes the workspace and MCP servers. These relationships describe
current implementation responsibilities, not an exhaustive inventory of workspace
access or a claim that optional integrations are active in this installation.

LikeC4 can also generate an `index` overview when no `index` view is authored.
Select one of the named views above to use an explicitly selected diagram.

## Internals walkthrough

The Omegon system contains two `application` elements and four `dataStore` elements.
These are C4 containers: applications and logical stores within a software system.
The containment follows **software system → container**, not recursive component
nesting. A C4 container does not imply a Docker container.

- **Omegon host** (`omegon.host`) is the common Rust runtime for sessions, the
  agent loop, tool and provider integrations, adapters, and supervision.
- **Codescan service** (`omegon.codescan`) is an optional first-party Rust process
  for indexing and search. It runs only when enabled and available. The host
  requests its capabilities through JSON-RPC over stdio.
- **Session history** (`omegon.sessions`) stores local durable conversations and
  resume checkpoints as JSONL and JSON. The host persists and resumes sessions.
- **Lifecycle ledger** (`omegon.lifecycle`) stores repository-local lifecycle
  state in JSON files. The host records lifecycle transitions.
- **Durable memory** (`omegon.memory`) stores retained project knowledge in
  SQLite. The host stores and retrieves that knowledge.
- **Code and knowledge index** (`omegon.index`) stores derived searchable code and
  knowledge in SQLite. The Codescan service owns, builds, and queries this index.

The cylinders represent logical stores, not separate database servers. Their
arrows identify application ownership and access. Retained memory and the derived
codescan index have different owners and purposes.

The view is mode-neutral. Interactive, ACP, headless, and daemon execution are
host modes or adapters, not separate application boxes. Internal threads, workers,
SDKs, and crates belong to deeper implementation detail.

The explicit selection shows only the Omegon boundary and these six children.
External systems and the project workspace are omitted from this internal view.
External relationships attach to host owners in the model and project onto Omegon
only in C1. Their external endpoints are absent from both internal views.

Relationships start at the smallest known responsibility endpoint. Session
orchestration owns the session-history relationship, Design and change lifecycle
owns the ledger relationship, Project memory owns retained-knowledge access, and
Extension integration owns the codescan transport relationship. C2 hides these
components, so LikeC4 projects their relationships onto the visible ancestor,
`omegon.host`. These implied ancestor edges preserve the five C2 relationships
without duplicate host-origin model edges. The codescan-to-index edge stays direct.

These are source-level architecture statements, not assertions about a running
installation or which optional services are currently active. The source evidence
includes mode dispatch in [main.rs](https://github.com/styrene-lab/omegon/blob/13760f256662de5cbb8089d38990f7db3c93d89d/core/crates/omegon/src/main.rs),
composition in [setup.rs](../../../core/crates/omegon/src/setup.rs), session
persistence in [session_authority.rs](../../../core/crates/omegon/src/session_authority.rs),
the ledger in [lifecycle_service.rs](../../../core/crates/omegon/src/lifecycle_service.rs),
memory ownership in [memory_service.rs](../../../core/crates/omegon/src/memory_service.rs),
and index ownership in the [codescan process](../../../extensions/omegon-codescan/src/main.rs).

The syntax follows the official [tutorial](https://likec4.dev/tutorial/),
[specification](https://likec4.dev/dsl/specification/),
[model](https://likec4.dev/dsl/model/), and
[view predicates](https://likec4.dev/dsl/views/predicates/) documentation.

## Incremental host components

Open [`hostComponents`](http://127.0.0.1:5173/view/hostComponents/) in the live
preview to explore the incremental C3 view. It expands **Omegon host** into
nine peer components, all directly inside the host container:

| Component | Responsibility | Source evidence under `core/crates/omegon/src/` |
| --- | --- | --- |
| **Operator interfaces** (`interfaces`) | Adapts native and ACP prompts, commands, approvals, and progress. | [operator_commands.rs](../../../core/crates/omegon/src/operator_commands.rs), [surfaces/conversation.rs](../../../core/crates/omegon/src/surfaces/conversation.rs) |
| **Session orchestration** (`orchestration`) | Supervises session work and drives the inference-tool cycle. | [runtime_supervisor.rs](../../../core/crates/omegon/src/runtime_supervisor.rs), [loop.rs](https://github.com/styrene-lab/omegon/blob/13760f256662de5cbb8089d38990f7db3c93d89d/core/crates/omegon/src/loop.rs) |
| **Context assembly** (`context`) | Builds per-turn instructions, conversation history, and contributed context. | [loop_context.rs](../../../core/crates/omegon/src/loop_context.rs), [context.rs](../../../core/crates/omegon/src/context.rs) |
| **Inference gateway** (`inference`) | Resolves the provider and model route and normalizes response streams. | [provider_route_service.rs](https://github.com/styrene-lab/omegon/blob/13760f256662de5cbb8089d38990f7db3c93d89d/core/crates/omegon/src/provider_route_service.rs), [bridge.rs](../../../core/crates/omegon/src/bridge.rs) |
| **Tool execution and policy** (`tools`) | Admits calls through permissions and approvals, executes tools, and returns results. | [loop_driver.rs](https://github.com/styrene-lab/omegon/blob/13760f256662de5cbb8089d38990f7db3c93d89d/core/crates/omegon/src/loop_driver.rs), [invocation_service.rs](../../../core/crates/omegon/src/invocation_service.rs), [loop_permission.rs](../../../core/crates/omegon/src/loop_permission.rs) |
| **Project memory** (`memory`) | Recalls and maintains retained project knowledge and contributes relevant context. | [features/memory.rs](../../../core/crates/omegon/src/features/memory.rs), [memory_service.rs](../../../core/crates/omegon/src/memory_service.rs) |
| **Design and change lifecycle** (`lifecycle`) | Manages project design and specification transitions and contributes active-work context. | [features/lifecycle.rs](https://github.com/styrene-lab/omegon/blob/13760f256662de5cbb8089d38990f7db3c93d89d/core/crates/omegon/src/features/lifecycle.rs), [lifecycle_service.rs](../../../core/crates/omegon/src/lifecycle_service.rs) |
| **Extension integration** (`extensions`) | Adapts native extension capabilities and supervises extension processes and RPC. | [extensions/mod.rs](../../../core/crates/omegon/src/extensions/mod.rs), including `ExtensionFeature` and `ExtensionSupervisor` |
| **MCP integration** (`mcp`) | Discovers and adapts MCP tools, resources, and prompts; manages server connections. | [plugins/mcp.rs](../../../core/crates/omegon/src/plugins/mcp.rs), including `McpFeature` and `McpSupervisor` |

Read the arrows from Operator interfaces to Session orchestration, then to
Context assembly, Inference gateway, and Tool execution and policy. The approval
arrow returns from Tool execution and policy to Operator interfaces when an
operator decision is required. Results can return along a request relationship
without a separate reverse arrow.

Tool execution and policy executes memory operations, executes design and change
operations, invokes native extension-owned tools, and invokes discovered MCP tools
through the supporting components.
Context assembly requests relevant retained knowledge from Project memory and
focused design and active-change context from Design and change lifecycle.
These two context relationships represent contract-mediated interactions:
`loop_context.rs` collects contributions through the feature bus and
`Feature::provide_context`. They do not assert direct calls from Context assembly
to the managed memory or lifecycle services.

**Project memory** (`omegon.host.memory`) is functional behavior inside the host.
**Durable memory** (`omegon.memory`) is the logical SQLite store in C2.
The component groups recall, maintenance, and context contributions through the
memory feature and its managed service. Its model relationship targets the
SQLite persistence represented by `omegon.memory`; the component is not the store.
C2 projects that relationship onto the host. Similarly, **Design and change
lifecycle** (`omegon.host.lifecycle`) provides functional behavior, while
**Lifecycle ledger** (`omegon.lifecycle`) represents stored lifecycle state.

**Extension integration** covers native extension adaptation, process supervision,
and RPC, including generic `execute_tool` and specialized native capabilities.
The codescan relationship represents the latter: [setup.rs](../../../core/crates/omegon/src/setup.rs)
captures an extension RPC handle for [CodescanBinding](../../../core/crates/omegon/src/codescan_service.rs),
whose client sends specialized codescan requests. The
[codescan process](../../../extensions/omegon-codescan/src/main.rs) does not
advertise generic tools. This edge does not assert codescan `execute_tool`
dispatch. Extension integration does not represent all MCP integration.

**MCP integration** publishes discovered tools through `McpFeature::tools` and
contributes cached resource, resource-template, and prompt listings through
`provide_context`. Thus **Collects MCP resource and prompt context** represents
feature-bus context contributions, not direct retrieval of every resource or prompt.
The separate `mcp_read_resource` and `mcp_get_prompt` tools retrieve content and
expand prompts. See [plugins/mcp.rs](../../../core/crates/omegon/src/plugins/mcp.rs)
(`tools`, `provide_context`, and execution handlers).

Native extension and MCP lifecycles have separate supervisors.
[setup.rs](../../../core/crates/omegon/src/setup.rs) transfers them through
`own_extension` and `own_mcp`, respectively. Their shared contribution lifecycle
does not imply identical process supervision or RPC contracts.

These components are architectural groupings of related functionality behind
interfaces, rather than a one-to-one inventory of files or crates. Session
orchestration coordinates the illustrated cycle; it is not a universal gateway
for every host interaction. The nine components and thirteen internal relationships
are an observed, source-backed incremental slice, not an exhaustive account of
host responsibilities or automatically accepted architecture norms.

The containment follows **software system → container → component**. Each
component is a peer under `omegon.host`; there are no nested components. The
explicit view includes only the host boundary and these nine children. External
actors, stores, the Codescan service, and the feature bus have no separate boxes
in this view.

The existing [manual C3 layout](.likec4/hostComponents.likec4.snap) retains the prior
component positions and edge paths, with space added for MCP and its two edges.
The memory-extraction edge adds routing within that layout without moving nodes
or replacing existing edge records.
Keep this snapshot aligned with the selected model; full validation detects drift.

## Prior model audit: PR #247

Assessed immutable revision:
[`13760f256662de5cbb8089d38990f7db3c93d89d`](https://github.com/styrene-lab/omegon/commit/13760f256662de5cbb8089d38990f7db3c93d89d),
the PR #247 merge, compared with its first parent
`1828d6c5a6f78eceae9e811e9ba39e4ccf4f59d4` (which includes the Vault ownership fix).
The clean implementation worktree at `5318381d4e7f5ee84642ee81e3206982248d24c7`
had the same tree as the assessed merge. At that checkpoint, the diagram branch
was based on `7b2da402073d84e54f3caffbb13845a007161c62`; its local Rust files were
not the post-merge source authority. Immutable links below retain that evidence,
including `model_request.rs`, which was absent from the diagram branch.

Before consolidation, turn and repair paths repeated capture/dispatch assembly.
Compaction and auxiliary completion already used the shared route infrastructure
with distinct evidence, tool, budget, and completion policies. After consolidation,
[`PreparedModelRequest`](https://github.com/styrene-lab/omegon/blob/13760f256662de5cbb8089d38990f7db3c93d89d/core/crates/omegon/src/model_request.rs#L1-L95)
provides a validated, immutable request value. Flow adapters supply inputs and
owner evidence before streaming. This contract does not establish a separately
responsible service or justify another C3 box.

| Sample | Post-merge evidence and architectural effect |
| --- | --- |
| Initial and repaired turns | [TurnRequestPreparation and dispatch capture](https://github.com/styrene-lab/omegon/blob/13760f256662de5cbb8089d38990f7db3c93d89d/core/crates/omegon/src/loop_session.rs#L24-L162) consolidate assembly. [The loop](https://github.com/styrene-lab/omegon/blob/13760f256662de5cbb8089d38990f7db3c93d89d/core/crates/omegon/src/loop.rs#L494-L642) retains orchestration and repair policy. |
| Route admission and compaction | [Resolved route preparation](https://github.com/styrene-lab/omegon/blob/13760f256662de5cbb8089d38990f7db3c93d89d/core/crates/omegon/src/provider_route_service.rs#L404-L456), [compaction](https://github.com/styrene-lab/omegon/blob/13760f256662de5cbb8089d38990f7db3c93d89d/core/crates/omegon/src/provider_route_service.rs#L1317-L1505), and [turn dispatch](https://github.com/styrene-lab/omegon/blob/13760f256662de5cbb8089d38990f7db3c93d89d/core/crates/omegon/src/provider_route_service.rs#L1508-L1562) use the value while retaining route and evidence ownership. Compaction now rejects structured upstream failure immediately. |
| Bounded memory extraction | [ModelExtractor](https://github.com/styrene-lab/omegon/blob/13760f256662de5cbb8089d38990f7db3c93d89d/core/crates/omegon/src/features/memory/formation.rs#L18-L38) calls [quick_completion_bounded](https://github.com/styrene-lab/omegon/blob/13760f256662de5cbb8089d38990f7db3c93d89d/core/crates/omegon/src/providers.rs#L756-L805), which prepares a no-tools, boot-bound, sessionless step request. Memory retains evidence selection, deadline, parsing, cancellation, and publication. |

Preparation is shared across these adapters. Neither Context assembly nor
Inference gateway exclusively owns the complete envelope lifecycle. Existing
context selection, invocation permission, session authority, and execution owners
retain their scopes. C1 remains nine nodes and nine edges. C2 remains six children
plus the system boundary and five edges. No additional ancestor self-edge is intended.

C3 now includes `omegon.host.memory → omegon.host.inference`:
**Requests bounded memory extraction**. This corrects a model omission exposed
by the review, not a production dependency introduced by PR #247. The
[pre-merge helper](https://github.com/styrene-lab/omegon/blob/1828d6c5a6f78eceae9e811e9ba39e4ccf4f59d4/core/crates/omegon/src/providers.rs#L756-L805)
already dispatched this request, and `formation.rs` is unchanged by the merge.

This selected sample covers turns/repairs, compaction, and memory extraction.
In the sampled [loop context adapter](https://github.com/styrene-lab/omegon/blob/13760f256662de5cbb8089d38990f7db3c93d89d/core/crates/omegon/src/loop_context.rs)
and [compaction planner](https://github.com/styrene-lab/omegon/blob/13760f256662de5cbb8089d38990f7db3c93d89d/core/crates/omegon/src/context_compaction_service.rs),
context selects and applies plans; loop/control owners request inference through
the route service. These paths do not establish a direct Context assembly →
Inference gateway edge. Other direct bridge callers are outside this sample.

The [design concern](../../../docs/unified-model-request-contract.md) and all five
[OpenSpec change artifacts](../../../openspec/changes/unified-model-request-contract/proposal.md)
were synchronized byte-for-byte from the assessed merge at this checkpoint.
Their pre-merge handoff statements describe that historical checkpoint: the
parent draft was replaced, PR #247 merged, and the memory edge was reconciled.
The current copies include the later factual closeout records below. Historical
research links in `design.md` refer to its recorded `7b2da402` source; implementation
paths and test symbols in the synchronized artifacts refer to assessed `13760f25`.
At that checkpoint, OpenSpec remained **verifying**, with 12 completed tasks,
and the concern retained its canonical `implementing` status. Tool-backed
task/test registration and final lifecycle acceptance were pending; merge alone
does not advance those states. This reassessment is source and diagram evidence,
not a new Omegon runtime test.

## Incremental architecture verification after landing

Assessed canonical-main revision:
[`77996814af7720a45f15fd482ac8579093ebdd6f`](https://github.com/styrene-lab/omegon/commit/77996814af7720a45f15fd482ac8579093ebdd6f).
This bounded assessment compares the three subsequent merges with the prior
`13760f25` model audit above. It checks their effects on the existing C1, C2,
and C3 responsibilities, not the entire codebase. The immutable source links
below identify the landed implementation independently of the checkout used
to read this assessment.

| Landed change | Evidence at the assessed revision | Architecture assessment |
| --- | --- | --- |
| [PR #248](https://github.com/styrene-lab/omegon/pull/248), CI toolchain baseline (`031ef2b7`) | [Test workflow](https://github.com/styrene-lab/omegon/blob/77996814af7720a45f15fd482ac8579093ebdd6f/.github/workflows/test.yml) resolves the locked Rust baseline exposed by [flake.nix](https://github.com/styrene-lab/omegon/blob/77996814af7720a45f15fd482ac8579093ebdd6f/flake.nix). | Development and validation configuration; no runtime component or dependency is added. |
| [PR #250](https://github.com/styrene-lab/omegon/pull/250), native Ollama Cloud tools (`024a8616`) | [Provider contributions](https://github.com/styrene-lab/omegon/blob/77996814af7720a45f15fd482ac8579093ebdd6f/core/crates/omegon/src/provider_contributions.rs) declares the provider's tool contract; the [model registry](https://github.com/styrene-lab/omegon/blob/77996814af7720a45f15fd482ac8579093ebdd6f/data/model-registry.json) declares model capability. [providers.rs](https://github.com/styrene-lab/omegon/blob/77996814af7720a45f15fd482ac8579093ebdd6f/core/crates/omegon/src/providers.rs#L4567-L4849) serializes tool schemas and normalizes native streamed calls and failures. | Provider capability and transport implementation remain under **Inference gateway** (`omegon.host.inference`). Tool invocation, permissions, and execution remain the separate **Tool execution and policy** responsibility (`omegon.host.tools`). |
| [PR #249](https://github.com/styrene-lab/omegon/pull/249), quiet inline/splash handling (`77996814`) | [Slash handling](https://github.com/styrene-lab/omegon/blob/77996814af7720a45f15fd482ac8579093ebdd6f/core/crates/omegon/src/tui/slash_commands.rs#L1232-L1243) disables splash replay when the session base is inline. [TUI test source](https://github.com/styrene-lab/omegon/blob/77996814af7720a45f15fd482ac8579093ebdd6f/core/crates/omegon/src/tui/tests.rs#L3783-L3808) covers draft and borrowed-inspector preservation. | Presentation behavior remains within **Operator interfaces** (`omegon.host.interfaces`). Inline and fullscreen remain host presentation choices, with no new application or mode boxes. |

**Architecture effect: none at the modeled levels.** These changes require no
C1/C2/C3 topology changes. The earlier memory → inference correction remains
part of the model. Model source, named views, manual geometry, snapshots, and
theme are unchanged by this documentation reconciliation.

**Combined-main headless runtime acceptance: PASS at exact `77996814`.** The
integration owner's pristine Nix Rust 1.95 build has frozen SHA-256
`cf3210740a3f38fca9aa1f500a800952559d0e258a11d3fd7e6eda4e11758388`.
Stress and resume passed with six plus one local provider requests and 43 captures.
The checks covered denied writes without mutation, ordinary second submission,
cancellation and recovery, inspector/draft preservation, resume, and owned-process
cleanup. External HTTP(S) was rejected by the fixture; no GUI windows were created.

**Exact-head CI is blocked, not all green.**
[Run 37671759878](https://github.com/styrene-lab/omegon/actions/runs/37671759878)
completed with 29 successful jobs and one cancelled Rust build. The build exceeded
its 30-minute limit during task-capsule release compilation; no code assertion
failure was reported. The [integration verification record](../../../openspec/changes/quiet-inline-replay/verification.md#combined-main-headless-acceptance--2026-10-07)
retains the evidence boundary. These results do not validate a later rebased TUI
tip, which needs separate gates.

The design and OpenSpec copies now include factual closeout records while retaining
historical verification. Both changes remain **verifying**, with task files
unchanged. Native registration, ledger reconciliation, and baseline-aware archival
remain blocked by the [closure concern](../../../docs/lifecycle-closure-reconciliation.md).
This artifact integration reruns no Omegon runtime checks and adds no preview
acceptance evidence. Model source, theme, and manual geometry are preserved.

## Diagram theme

[likec4.config.json](likec4.config.json) defines the diagram palette: teal for
Omegon, its applications, and host components, copper for the Operator, and warm charcoal for external
systems, the external workspace environment, and internal data stores. Kind-level
styles assign these roles. Names, containment, the environment label, the person
shape, and data-store cylinders distinguish their meanings.
Solid copper arrows with vee heads retain the directed-usage meaning above.
Explicit pale title and description colors contrast with the element fills;
relationship labels use pale text on charcoal backgrounds with viewer-applied opacity.

The standard viewer still controls light/dark mode. This config does not set the
canvas background or force dark mode. For a dark-default static build, run
`npm run diagrams:build -- --theme dark` from `site/`; this flag does not affect
`diagrams:dev` or override a saved viewer preference.
See the official [theme configuration](https://likec4.dev/dsl/config/#styles-customization).

## Prerequisites

- Node.js **>=22.22.3**, as required by the pinned LikeC4 **1.59.4** CLI.
- npm. CI uses Node 22, which must meet the minimum patch version above.

LikeC4 includes Graphviz WASM for layout. A system Graphviz installation is not
required. Playwright browsers are needed only for PNG/JPEG export and are not
installed by this setup.

## Commands

From the repository root, install the locked site dependencies:

```sh
cd site
npm ci
```

Validate the model and start the live preview with:

```sh
npm run diagrams:validate
npm run diagrams:dev
```

The shared preview listens on `127.0.0.1:5173` and uses the canonical checkout
after promotion. Follow the [working agreement](WORKING-AGREEMENT.md#shared-runtime-review)
for source identity, controlled restarts, and operator ownership.

Build the standalone diagram website with:

```sh
npm run diagrams:build
```

Build output goes to the ignored `site/dist/likec4/` directory. The ordinary Astro
build also uses `site/dist/` and can replace this output. Diagram commands are
opt-in.

See the [official CLI documentation](https://likec4.dev/tooling/cli/) for additional commands.
