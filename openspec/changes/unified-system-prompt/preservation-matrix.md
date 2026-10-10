# Preservation ledger

## How to review this ledger

Source inspection is at `4733fb4b2c36ee7078911e4e6c5a540dbfb63c69`.
The long Full/Slim Rust literals were read in full with wrapped output; the ledger
does not rely on truncated line previews. The operator accepted these dispositions
when authorizing implementation. Completion evidence is recorded separately in
[implementation.md](implementation.md):

- **RETAIN**: preserve the semantic contract at its current owner.
- **MERGE**: express overlapping obligations once in the canonical core.
- **RELOCATE**: preserve useful guidance at a named, applicable owner before
  removing automatic injection. Applicability is not workflow authorization.
- **RETIRE**: deliberately remove the stated behavior, with rationale and a
  verification target. This does not imply deleting its entire source file.

Current destinations and executed evidence for every row are recorded in
[verification.md](verification.md) and the final gate record in
[implementation.md](implementation.md). The entries below retain the contract
targets and original source inventory. `C` refers
to [common-policy scenarios](specs/prompt/common-policy.md); `G` refers to
[guidance-loading scenarios](specs/harness-parity/opencode2.md); `CP` refers to
[content-pack scenarios](specs/runtime-contributions/content-packs.md). The titles are
stable review references. No row authorizes broader feature removal.

## Evidence index

| Key | Inspected owner |
|---|---|
| P | [prompt.rs](../../../core/crates/omegon/src/prompt.rs), especially 27–183, 210–269, 274–313, 316–447, 464–483 |
| L | [lex-imperialis.md](../../../data/lex-imperialis.md), all six axes |
| K | [lex-capabilities.md](../../../data/lex-capabilities.md), including literal leading `+` characters in the current asset |
| T | [tool-limitations.md](../../../data/tool-limitations.md), all task/alternative rows and suggestions |
| A | [registry.rs](../../../core/crates/omegon/src/plugins/registry.rs#L751-L833), [persona.rs](../../../core/crates/omegon/src/features/persona.rs#L351-L372), [setup.rs](../../../core/crates/omegon/src/setup.rs#L1057), [TUI construction](../../../core/crates/omegon/src/tui/mod.rs#L939-L941) |
| X | [context.rs](../../../core/crates/omegon/src/context.rs#L360-L555), tool-group/file-type injections and mandatory selection |
| W | [conversation.rs](../../../core/crates/omegon/src/conversation.rs#L1132), active-plan execution contract |
| D | [delegate.rs](../../../core/crates/omegon/src/features/delegate.rs#L472-L482), [cleave/context.rs](../../../core/crates/omegon/src/cleave/context.rs#L442-L466), [autonomy.rs](../../../core/crates/omegon/src/autonomy.rs) |
| E | [Vox](../../../data/vox-extension-context.md), [Scry (relocated)](../../../skills/scry/references/usage.md), [extension authoring (relocated)](../../../skills/extension-authoring/references/authoring.md) |
| R | [permissions.rs](../../../core/crates/omegon/src/permissions.rs#L259-L334), [invocation_service.rs](../../../core/crates/omegon/src/invocation_service.rs), [model_request.rs](../../../core/crates/omegon/src/model_request.rs) |
| B | [content manifest generator](../../../scripts/content_pack_manifest.py#L21-L27), [embedding invariant](../../../scripts/check_no_embedded_content.py#L14-L44), [content-pack.toml](../../../content-pack.toml) |

The [pinned prompt source](https://github.com/styrene-lab/omegon/blob/4733fb4b2c36ee7078911e4e6c5a540dbfb63c69/core/crates/omegon/src/prompt.rs)
identifies the immutable observation. Relative links locate the corresponding
owners in this checkout. Uncommitted Git concern/model documents are local review
artifacts, not claimed to exist at that public pin.

## Constitutional semantics and repeated behavior

| ID | Existing rule and repeated locations | Disposition and reason | Owner / verification |
|---|---|---|---|
| L1 | Anti-sycophancy: challenge weak reasoning, evidence-based disagreement, no reflexive praise. L I; P condensed Lex; Full “disagree.” | **MERGE** all into one evidence-backed challenge obligation. | Host core; C “Six Lex categories remain accounted for.” |
| L2 | Evidence: distinguish known/suspected/guessed, test and revise, qualify uncertainty. L II; condensed Lex; Full/Slim grounding. | **MERGE** with cognitive honesty. Keep observations/inferences/uncertainty distinct. | Core plus evidence owners; C “Six Lex categories remain accounted for.” |
| L3 | Cognitive honesty: no confabulation, acknowledge limits, disclose unverified intuition and changed conclusions. L V. | **MERGE** with L2; do not require a universal introspective narrative or invented uncertainty percentage. | Core; honest outcomes and correction trial. |
| L4 | Proportionality: simplest working solution, iterate, avoid overarchitecture, distinguish prototype from implementation. L III; Full/Slim bounded exploration. | **MERGE** smallest justified action satisfying the task and proportionate validation. **RETIRE** blanket “80% solution now” quality shortcut. | Core; C “Six Lex categories remain accounted for”; complete-task review. |
| L5 | Systems framing: systems/interfaces/constraints/tradeoffs; owners and dependency costs; every operator presumed engineer. L IV; K opening; P coding/OM identities. | **RETAIN** ownership/interface respect in core. **RELOCATE** engineering framing to an applicable role, project, or skill. **RETIRE** universal engineer persona and OM-mode identity. | Core and scoped content; no-Git/general-task fixture under C “One core across entry points and resource postures.” |
| L6 | Agency: perform available work, request decisions rather than menial tasks, respect choices after disagreement. L VI; condensed Lex; Full/Slim approval language. | **MERGE** clear requests and standing authorization into core; no repeated “proceed?” once authorized. | Core; C “Git scope follows operator and workflow authority.” |
| L7 | Human checkpoints: interactive terminal for OAuth/browser waits; identify waiting state and checkpoint. L VI. | **RELOCATE** procedure to terminal/auth tools; **RETAIN** meaningful human interaction boundary in core. No blanket block on authorized actions. | `tools/terminal.rs`, auth/command surfaces; C “Scoped tool procedures survive relocation.” |
| L8 | Immutable core cannot be overridden by operator/project/persona/tone. L heading; P wrappers; A registry invariant. | **RETAIN** host ownership; **RETIRE** treating packed operational guidance as immutable simply because appended beneath Lex. | Host composition/admission; C “Persona cannot duplicate or replace host policy” and CP “Kernel builds without shipped content.” |
| L9 | Condensed three-axiom family plus full six-axiom family. P mode selection; A independent full bundle. | **RETIRE** families and duplicate injection after L1–L8 replacements exist. No grade-based shadow policy. | Setup, prompt, registry; C identity and persona scenarios, CP packless startup. |

## Full/Slim behavior and harness guidance

| ID | Existing rule and repeated locations | Disposition and reason | Owner / verification |
|---|---|---|---|
| H1 | Always answer after tools; terse concrete responses. P Full/Slim. | **MERGE** direct useful final response. Keep necessary evidence and limits. | Core; payload review and empirical task completion. |
| H2 | Act rather than narrate next-turn intent; combine independent calls. P Full/Slim. | **MERGE** action on clear requests. **RELOCATE** batching details to tool execution guidance; never require unsafe ordering. | Core/tool contracts; C “Scoped tool procedures survive relocation.” |
| H3 | Frustration is correction, not profanity to mirror; no apologetic/self-critical process monologue; if blocked identify exact decision. P Full/Slim. | **MERGE** concrete course correction without hostility or process narration; preserve actionable blockers. **RETIRE** universal bans on every apology/explanation. | Core; correction and blocked-task empirical fixtures. |
| H4 | Stop exploring once reversible next step is justified; archaeology only for actionable evidence/blockers. P Full/Slim. | **MERGE** bounded evidence gathering. Do not turn “smallest action” into partial task completion. | Core; bounded-progress review. |
| H5 | Read before edit; exact-current-text anchor; smallest replacement; canonical `edit`, narrow `validate`; every nontrivial change needs tests. P Full/Slim. | **MERGE** read/evidence/proportionate validation. **RELOCATE** edit preconditions and validation procedures to actual tools and applicable project/testing guidance. Preserve honest distinction between skipped, attempted, and passed checks. | `tools/edit.rs`, `tools/validate.rs`; C “Scoped tool procedures survive relocation.” |
| H6 | Cite file/line evidence; do not assert unread code. P Full/Slim, condensed Lex, L II/V. | **MERGE** evidence/honesty; **RELOCATE** surface-specific citation conventions when applicable. | Core and output/tool evidence; C semantic review. |
| H7 | “Commit when done”; “Do not push automatically … if asked, do it.” P Full/Slim, inherited by Constrained. | **RETIRE** default commit timing/publication prohibition as unconditional workflow. Replace with operator/applicable authorized workflow scope, without repeated approval. | Core; C Git scope scenario. Mechanical Git changes excluded. |
| H8 | Prefer `request_context` before exploratory calls unless exact target known. Full only. | **RELOCATE** orientation use cases to the actual request-context tool surface, preserving direct target reads. | Context-request feature/tool definition; C tool-contract scenario. |
| H9 | Render clickable URLs as Markdown links. P Full/Slim. | **RELOCATE** presentation rule to applicable output/surface guidance; preserve usability, not a universal workflow block. | Response-format/project/skill guidance; interface review. |
| H10 | Stay in local coding loop; do not add lifecycle/orchestration unless requested or necessary; do not widen small edits casually. Slim. | **MERGE** scope/proportionality; **RETAIN** operator agency and no mandatory workflow from mere availability. Strengthen scope with the review-requested explicit preservation of existing user work and unrelated changes; this is not verbatim legacy prompt text. **RETIRE** universal coding-only identity. | Core plus workflow admission; C “Six Lex categories remain accounted for” and “Available tools and files do not authorize workflow.” |
| H11 | Workbench is operational; update immediately, advance/complete/skip/clear accurately; no “nothing pending” with active/todo work. Full/Slim; K; W. | **MERGE** truthful state into core. **RETAIN** active-plan execution contract; **RELOCATE** detailed state transitions to `plan` tool and scoped active-plan instructions. | Conversation/plan owner; C “An inherited active plan remains operational.” |
| H12 | Separate producer/provenance from content form; semantic projections; shared TUI/CLI/ACP/IPC registry, no renderer shortcuts; prompt IDs are data. Full/Slim harness sections. | **RELOCATE** Omegon implementation architecture guidance to project/engineering skill scope. Runtime provenance/projection/registry contracts themselves **RETAIN**. | Root/main AGENTS and semantic owners; C protocol/metrics regression. |
| H13 | Prompt templates/loops are executable instructions; preserve provenance, preview/validate, explicit repeated-loop safety. Full harness section. | **RETAIN** contracts at `features/prompt.rs`, `runtime_prompt.rs`, `prompts.rs` and command/tool scope. **RELOCATE** explanatory prose; never discard because Slim omitted it. | C “Prompt and loop execution retains its safety boundary.” |
| H14 | Hidden tool groups: list then enable via `manage_tools`; Slim lists delegation/lifecycle/persona/secrets examples. Full/Slim. | **RELOCATE** discovery procedure to actual tool definitions; **RETAIN** admitted tool facts and permission boundary. Enabling availability does not grant workflow authority. | Tool-surface manager; C availability and tool-contract scenarios. |

## Capability assets, lifecycle, languages, and extensions

| ID | Existing rule and repeated locations | Disposition and reason | Owner / verification |
|---|---|---|---|
| S1 | Only current-schema tools; exact argument contracts. K. | **MERGE** core admitted-tool rule; **RETAIN** schemas and execution validation. | Core/tool owner; C tool-contract scenario. |
| S2 | Assess nontrivial work; delegate one side quest versus cleave coordinated scopes. K; P lifecycle; P subagent guidance; X cleave. | **RELOCATE** selection advice to applicable orchestration skill/tool. **RETIRE** universal assessment mandate. | Delegate/cleave contracts; C workflow and child scenarios. |
| S3 | Design/Workbench decisions, questions, status track evidence; do not claim spec verification/archive readiness prematurely. K; H11; X lifecycle. | **RETAIN** truthful active state and verification boundaries; **RELOCATE** ceremony to applicable lifecycle scope. | Lifecycle and plan owners; C active-plan scenario; existing reconciliation baseline. |
| S4 | Store durable decisions/constraints/verified patterns; monitor headroom and compact using exposed controls. K; X memory. | **RELOCATE** procedures to memory/compaction tools and applicable workflow. **RETAIN** actual context pressure facts and runtime compaction policy. | Memory/context owners; auxiliary preservation scenario. |
| S5 | Tool presence declares project structured workflow; design questions/assumptions/readiness/status recipe; OpenSpec design→spec→tasks→register→tests→implement→assess→archive. P `detect_lifecycle_context`. | **RETIRE** inference from availability (`_cwd` unused). **RELOCATE** valid procedures to actual lifecycle tools/applicable skills, keeping reconciliation requirements. | Lifecycle contracts; C availability scenario. |
| S6 | X memory: targeted recall, exposed inventory, conclusions rather than investigation steps, supersede/check existing facts, pointer facts. | **RELOCATE** to memory tool contracts/applicable memory guidance. No loss of deduplication or evidence semantics. | `features/memory.rs`; C tool-contract and quote scenarios. |
| S7 | X design: node/frontier/branch/focus and transitions. X cleave: score suggestion, conditional API parameter, reconcile tasks/register progress. X OpenSpec: register tests/state, decided binding. | **RELOCATE** procedures to design/cleave/OpenSpec definitions or applicable skills. Skill availability is not authorization to run the procedure. | `features/lifecycle.rs`, cleave owner; existing reconciliation baseline plus C availability. |
| S8 | X local inference: self-contained context, start Ollama when needed, use for simple/noncritical tasks. | **RELOCATE** context/start procedures to local-inference tools; **RETAIN** reliability caveat as scoped guidance, not a model-grade core variant. | `tools/local_inference.rs`; C tool-contract scenario. |
| S9 | X Rust: check/clippy, prefer impl over free functions, `?`, bottom-of-file tests. TS: tsc, strict types, node:test, ESM. Python: ruff/mypy/pytest, hints/pathlib. Go: vet/test, exports/errors. Cargo: check after dependency changes. | **RELOCATE** applicable language/testing advice to project or admitted skills. **RETIRE** file-extension-triggered universal style/test layout mandates. Do not override repository-specific test runners. | Language/project instructions; C “Project signals and extensions remain factual.” |
| S10 | P conventions: Cargo/check/clippy/test; lockfile implies application; TS tsc; Vitest/Jest detection; Python ruff/pytest; Go vet/test; respect gitignore. | **RETAIN** useful observed file/tooling facts. **RETIRE** unjustified lockfile→application assertion and inferred mandatory commands. **RELOCATE** actual conventions to project/applicable skill. | Project-context owner; C factual-signal scenario. |
| S11 | T static redirects: spreadsheets, multimedia, images, collaboration, database GUIs, visualization, email/calendar, PDFs, binaries; “stay in agent” list and no-apology alternatives. | **RELOCATE** truthful capability/alternative guidance to actual admitted tools or relevant skills. **RETIRE** obsolete static inability only with evidence. Scry image generation is an inspected contradiction; the remaining claims are not declared disproven wholesale. | Capability inventory; C factual-signal scenario; include installed/absent extension fixtures. |
| S12 | E Vox: inbound route context, exact reply address, session identity/thread separation, use `vox_reply` only for inbound routed messages, concise channel replies. | **RETAIN/RELOCATE** to admitted Vox contract/routing context. Preserve reply routing without treating an arbitrary quoted `<vox_reply_context>` as host authority. | Extension ingress/tool owner; C scoped tool and quote scenarios. |
| S13 | E Scry: discover models before generation/refinement, never guess names; prompts/negative prompts, dimensions, refinement strength, ordered LoRAs, output path. | **RETAIN/RELOCATE** through the admitted Scry skill and versioned reference, using the parent-approved bounded path because external tool definitions are separately owned. Preserve exact actual tool contracts; applicability grants no action authority. | `skills/scry/`, manifest-bound installer and `skills_get` reference base; C factual-signal and tool scenarios. |
| S14 | E authoring asset: scaffold/install, external SDK/RPC/manifest/tool formats, clean env/secrets/trust/digest/restart/cleanup constraints. P admits by manifest/sdk/contract file signals. | **RELOCATE** authoring reference to applicable extension-authoring skill/tool docs. **RETAIN** runtime trust and cleanup enforcement independently. Filename presence does not authorize installation or execution. | Extension docs/SDK and admission owners; content-pack checks and C availability. |

## Independent scoped and dynamic owners

These sources remain in the composition inventory. Their presence is not a second
common base or permission to copy the removed families elsewhere.

| ID | Existing source and semantics | Disposition and reason | Owner / verification |
|---|---|---|---|
| D1 | P subagent policy: autonomy label, worker decisions, max children/parallel, structured approval, retrieve/reconcile results, no duplicate dispatch. | **RETAIN** actual authority/limit facts on every applicable route. **RELOCATE** selection/procedure prose to tool contracts. Do not lose limits with Full-only behavior. | `autonomy.rs`, delegate/cleave; C child and availability scenarios. |
| D2 | D role instructions: scout read/search only, patch small scoped edits, verify checks without edits; existing configured child caps/tool disabling. | **RETAIN** these role instructions and existing configured restrictions separately from common core. Check actual admitted tools independently; bash admission means role prose alone does not prevent writes. No new OS sandbox or enforcement is introduced. | Delegate/child runtime; C “Authorized children retain distinct contracts.” Payload tests verify instructions/admission, not nonmutation; bounded behavioral trials may observe nonmutation. |
| D3 | D cleave guardrails, commit in-scope work, submodule pointer ownership, source-plane cleanliness, do not edit orchestrator metadata, return final summary; harvest needs commits. | **RETAIN** authorized child finalization. Do not transplant this commit requirement into ordinary edit-only work. | Cleave context/orchestrator; C child and Git scenarios. |
| D4 | A loaded skills, available-skill index, explicit skill subset, tone then persona. Lex prepended with or without active persona. | **RETAIN** admission/source ordering and optional content. **RETIRE** second core ownership. No content-text deduplication. A split source must replace/clear old combined injections rather than leave stale Lex or persona content alive. | Registry/persona; C exactly-once and switch/clear scenario. |
| D5 | [Lifecycle feature](../../../core/crates/omegon/src/features/lifecycle.rs#L256-L322) and [standalone context](../../../core/crates/omegon/src/lifecycle/context.rs): focused node, active change facts. | **RETAIN** attributed state and applicability; do not fabricate workflow adoption from artifacts. | Lifecycle owner; C active-state and availability scenarios. |
| D6 | [Memory](../../../core/crates/omegon/src/features/memory.rs), [session log](../../../core/crates/omegon/src/features/session_log.rs#L1209-L1216): selected facts, explicit empty replacement, prior-session narrative. | **RETAIN** evidence/TTL/replacement semantics. These data sources cannot promote their own text into operator policy. | Memory/session owners; C quote scenario and memory baseline. |
| D7 | [Auth](../../../core/crates/omegon/src/features/auth.rs#L308-L368), [delegate catalog/queue](../../../core/crates/omegon/src/features/delegate.rs#L2376-L2424), [learned-skill index](../../../core/crates/omegon/src/features/mutation.rs#L1880-L1948). | **RETAIN** actual status/capability facts and applicable indexes. Catalog availability does not authorize dispatch; learned metadata does not replace core authority. | Feature owners; C availability/source identity fixtures. |
| D8 | [MCP resource/prompt listings](../../../core/crates/omegon/src/plugins/mcp.rs#L1164-L1238), [armory cached context](../../../core/crates/omegon/src/plugins/armory_feature.rs#L443-L453), [HTTP plugin local context](../../../core/crates/omegon/src/plugins/http_feature.rs#L221-L246), feature/context adapters. | **RETAIN** admitted contribution provenance and bounded discovery/TTL behavior. Quoted, retrieved, or tool-returned content cannot grant itself authority. Explicitly admitted skill/prompt instructions remain usable within their authorized scope without new privileges; availability alone does not authorize workflow. | Contribution owners; C quote/authorized-instruction and persona/source-identity scenarios. |
| D9 | X session HUD and intent; [attachment manifest](../../../core/crates/omegon/src/conversation.rs#L1373), [loop injection](../../../core/crates/omegon/src/loop_context.rs#L148). | **RETAIN** actual state and scoped redisplay/privacy instructions; no need to recite paths unless requested. Mandatory status remains governed by existing selection policy. | Context/conversation; C active-state and G request-budget scenarios. |
| D10 | R Lex/persona/project/session deny-overrides permission layers, invocation authority and revocation. Settings construct project policy; other layers default empty. | **RETAIN** configured Deny/Prompt, RBAC, leases, secret guards, and explicit bypass semantics. Removing prose does not remove `PermissionLayer::Lex`; the six prose axioms are not thereby mechanically enforced. | Permission/invocation owners; C resource-controls scenario with explicitly configured denial. |

## Loading, packaging, transport, and evidence

| ID | Observed surface | Disposition and rationale | Owner / verification |
|---|---|---|---|
| I1 | P Full-only global instructions, 3,000-byte truncation, `if let Ok` ignores errors. | **RETAIN** separate global operator owner; **RETIRE** omission/truncation/error swallowing. Proposed behavior change: complete UTF-8 on all routes, actionable failure when existing source cannot be read. | Global loader; all G scenarios. |
| I2 | P complete attributed project chain, worktree/gitfile bound, canonical dedupe including explicit symlinks, missing versus unreadable distinction. | **RETAIN** all established behavior; no semantic precedence rewrite. | Project loader; G inherited project scenarios. |
| I3 | P metrics and X mandatory base/priority ≥190; request/resource accounting. | **RETAIN** section evidence and byte-based estimates, complete mandatory input, existing reserves/prefix/schema accounting. No silent downgrade. | Existing context/request owner; G budget and C metrics/history scenarios. |
| I4 | Setup model grade→mode; compact schemas always true; CLI “reduce prompt” help; non-prompt `is_slim()` consumers. | **RETIRE** selector/family APIs and stale help claims. **RETAIN** posture, thinking, caps, tool surfaces, child limits, persisted/Pkl fields and UI aliases. | Setup/settings/CLI; C route matrix and controls. |
| I5 | B capability/limitation/extension assets are manifest-owned content; constitutional Lex has a narrow script embedding exception that the broad baseline does not explicitly describe. | **RETAIN** discovery/admission/versioning. **RELOCATE/RETIRE** individual assets only after consumer/manifest/invariant review. Amend the baseline with a narrow host-core exception; never re-embed optional content under “core.” Digests are not publisher authentication. | Pack/script owners; all CP scenarios, including absent/corrupt/missing-asset/valid-empty fixtures; manifest/invariant checks. |
| I6 | [Provider adapters](../../../core/crates/omegon/src/providers.rs): OAuth prefix, Anthropic caching, Gemini system instruction, Responses envelope. | **RETAIN** protocol composition and cache boundaries; one core is not one complete wire string. | Providers/tool schema; C controls and budget scenarios. |
| I7 | [Unified model preparation](../unified-model-request-contract/specs/inference/preparation.md): purpose-specific compaction/extraction and authority/no-tools/deadline/collector constraints. | **RETAIN** unchanged. Do not replace auxiliary prompts with the common agent base. | Existing flow/request owners; C auxiliary scenario. |
| I8 | Existing request/session evidence, section metrics, immutable captured input and retries; `loop_session.rs` materializes exact provider-visible bytes and a source event. | **RETAIN** old meaning/history even after source files change or disappear; path labels alone are insufficient provenance. Identify current core in new preparation via existing fields where possible. No default new persisted schema. | Request/session owners; C history and metrics scenario. |

## Completion rule for the ledger

Before implementation removes an injection, its row must have an inspected
destination and concrete verification. A missing tool-contract description blocks
that removal until repaired. Newly discovered consumers become explicit ledger
entries. Neither a shorter prompt nor a passing fake-bridge snapshot proves
behavioral preservation. Bounded model trials remain separate evidence.
