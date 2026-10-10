# Preservation verification — final acceptance

This record describes deterministic contract evidence and six bounded real-model
observations. It does not claim universal model obedience, OS confinement, or native
lifecycle closure. The accepted core body and hash remain unchanged from stage 1.

## Scoped owners

The following table accounts for every preservation-ledger row. The corresponding
common-policy scenarios remain the contract unless a loading/content-pack owner
is named explicitly. Test names below refer to the main `omegon` binary tests.

| Ledger rows | Current owner and disposition | Evidence |
|---|---|---|
| L1 | Evidence-backed disagreement in `prompt::CORE_POLICY`. | `core_preserves_reviewed_obligations_without_old_personas` |
| L2, L3 | Observations/inferences/uncertainty and honest claims merged in the core. | Same core test; exact approved-byte comparison |
| L4 | Smallest justified action satisfying the task; proportionate validation. Blanket 80% shortcut retired. | Core test and `base_commit_scope_requires_operator_or_authorized_workflow` |
| L5 | Ownership/interface respect in core; engineering roles remain optional persona/project content. Universal engineer identity retired. | Core test; registry persona-transition tests |
| L6 | Clear requests and standing authorization in core; meaningful decisions remain operator-owned. | Core test; configured permission-layer tests |
| L7 | Human interaction boundary in core; interactive procedure remains in the `terminal` contract. | `scoped_procedures_survive_production_compact_schema_output` |
| L8, L9 | One host constant, no pack override or registry policy input; family APIs removed. | `optional_pack_failures_empty_and_replacement_cannot_replace_host_policy`, registry ownership tests, removed-symbol search |
| H1 | Direct useful final response in core. | Approved-byte comparison; six bounded real-model outcomes in the final acceptance section |
| H2 | Authorized action in core; batch/parallel procedure remains in the applicable code-act skill and invocation owner. No universal batching mandate. | `code_act_skill_preserves_canonical_edit_validate_loop`; `skills/code-act/SKILL.md` execution/rules sections |
| H3, H4 | Concrete correction and actionable investigation in core. | `base_prompt_hardens_operator_frustration_recovery`, core test |
| H5 | Exact edits and narrow validation in actual `edit`/`validate` descriptions; no duplicate new prose. | Production compact-schema test |
| H6 | Evidence in core; file/line citations in applicable codebase-init guidance. | `extension_references_are_task_scoped_pack_assets_not_automatic_prompts` |
| H7 | Default commit timing retired; operator/workflow scope in core. Child finalization is separate. | Core commit-scope test; `cleave::context::tests::finalization_*` |
| H8 | `request_context` describes orientation and known-target direct reads; universal context-first ordering retired. | Production compact-schema test |
| H9 | Clickable Markdown URLs relocated to the existing output-style skill. | Pack-backed extension-reference test also checks style guidance |
| H10 | Scope and user-work preservation in core; no coding-only persona. | Core test; no-workflow-adoption test |
| H11 | Truthful state in core; detailed execution remains in `plan` and active intent. | Compact-schema test; `context::tests::active_plan_intent_is_visible_in_the_current_provider_prompt` |
| H12 | Repository architecture guidance remains in root/main AGENTS; shared semantic/runtime owners unchanged. Universal implementation appendix removed. | Source diff; `host_preserves_truthful_work_state_without_universal_harness_workflow` |
| H13 | Prompt/loop execution remains with `features/prompt.rs`, `runtime_prompt.rs`, and `prompts.rs`. | Existing prompt/queue tests; no production changes to these owners |
| H14 | Discovery/enable procedure remains with `manage_tools`. | Production compact-schema test |
| S1 | Admitted-tool rule in core; actual schemas and invocation checks remain authoritative. | Compact-schema and request-snapshot tests |
| S2 | Bounded delegate versus coordinated cleave selection moved into their descriptions. Universal assessment retired. | Compact-schema test; no-workflow-adoption test |
| S3 | Active-state truth stays with core, plan, and lifecycle owners. | Active-plan test; adopted OpenSpec guidance |
| S4 | Durability and exposed compaction remain in memory tool descriptions and runtime policy. | Compact-schema test; unchanged compaction/extraction tests |
| S5 | Tool-availability lifecycle adoption inference removed. Assumptions/focus/questions and OpenSpec checkpoints moved into actual contracts. | No-workflow-adoption and compact-schema tests |
| S6 | Current-state/source-pointer advice added to `memory_store`; durability, deduplication and superseding retained. Unconditional recall mandate retired. | Compact-schema test; finite memory replacement test |
| S7 | Design and OpenSpec procedures stay with their tools and adopted skill. No nonexistent cleave parameter is introduced. | Compact-schema test checks task/test registration, design questions, and absent `openspec_change_path` parameter |
| S8 | Local model's lack of parent context is now top-level contract text, along with conditional startup and verification. | Compact-schema test proves parameter descriptions are stripped while essentials remain |
| S9 | Automatic extension-based language/style/test commands retired; project/admitted language skills remain owners. | `file_extensions_do_not_override_project_language_and_test_policy` |
| S10 | Host reports observed filenames and source directory only. Lockfile/application and command inference retired. | `project_signals_are_observed_files_not_inferred_policy` |
| S11 | Static capability/redirect table removed from shipping and automatic assembly. Current tool schemas describe available capabilities; core requires honest limits. No opposite blanket capability claim is added. | No-workflow/extension-adoption test; manifest packaging test |
| S12 | Vox reply procedure now belongs to both host-produced ingress branches; exact original reply address and session key remain. | `extensions::vox_bridge::tests`, including leased result, object/string keys and quoted-marker cases |
| S13 | Scry essentials and versioned reference are scoped skill content. Generic `generate` name no longer triggers a host appendix. Manifest-bound installation preserves the reference when user scope overrides bundled scope. | `scoped_extension_skills_are_discoverable_retrievable_and_triggered_without_granting_tools`; `bundled_install_repairs_supporting_assets_with_unchanged_entries_and_user_precedence`; reference and no-adoption tests |
| S14 | Extension-authoring reference moved into an applicable skill bundle, pointing to the existing external SDK and host docs. Generic filename-triggered full appendix removed. `skills_get` exposes the reference base for both bundled and installed paths. | Same skill inventory/retrieval and installed-reference tests; content-pack archive checks |
| D1 | Request-local subagent facts come from the final logical tool snapshot and current automation settings. They are mandatory, replaced every composition, and captured after budgeting. | `request_policy_facts_follow_production_lean_lazy_and_admission_snapshots` |
| D2 | Scout/patch/verify preambles, configured restrictions, and limits unchanged. Bash remains exposed where configured; prose is not confinement. | `child_role_prose_and_actual_admitted_schemas_remain_distinct`, existing worker-profile tests |
| D3 | Cleave commit/guardrail/finalization owner unchanged. | Existing finalization tests |
| D4 | Registry supplies skills → tone → persona only; explicit subsets and empty replacement preserved. | Registry, persona switch/clear, and subset reload tests |
| D5 | Focused lifecycle state remains with its existing context feature; no new adoption inference. | Production owner unchanged; source diff and no-adoption test |
| D6 | Memory/session evidence and replacement semantics unchanged. | Finite-memory and persistent-provider replacement tests |
| D7 | Auth/catalog/learned-skill status owners unchanged. Availability is not action authority. | Source diff; core authority and actual-tool-snapshot tests |
| D8 | MCP/plugin/extension source and admission owners unchanged. Quoted content is not promoted by text matching. | Core byte check, quoted-policy registry test, quoted Vox marker test |
| D9 | HUD, intents, attachments, TTL, telemetry and mandatory selection retained. | Context tests, active plan, one-turn injection, explicit workflow TTL/telemetry test |
| D10 | Lex permission layer, configured denial and prompt precedence unchanged. | `permissions::tests::layered_policy*` |
| I1 | Complete, attributed, actionable global loader on the common host path. | Global instruction tests with synthetic sources |
| I2 | Existing root-to-cwd/worktree/symlink/canonical-dedup/non-Git discovery unchanged. | All `instruction_discovery_*` tests |
| I3 | Complete mandatory input, byte metrics, actual schema and generation reserves retained. | `complete_host_instructions_charge_oauth_prefix_schemas_and_generation_before_capture`; tiny-context selection test |
| I4 | No prompt-family selector or APIs. Resource/posture/UI/child fields unchanged apart from accurate comments/help. | Removed-symbol search; source diff; production lean/lazy test |
| I5 | Host constant has no optional-content dependency. References are pack-owned; retired automatic bundles are not shipped as prompts. Old Lex embedding exception removed. | Pack-failure matrix, manifest/embedding checks, packaging tests |
| I6 | Provider envelope and dialect implementations unchanged. OAuth prefix and actual schema costs are included before evidence. | Complete-input boundary test; unchanged provider source |
| I7 | Summary/extraction instructions stay purpose-specific, including no-tools checks and bounded failure handling. | Model-request no-tools tests; compaction prompt/generation and extraction tests |
| I8 | New core metadata lives in captured bytes; no persisted schema changes. Historical and in-flight captures are not rewritten. | `captured_host_policy_replays_after_instruction_sources_change_and_disappear`, existing capture and retry tests |

## Static limitation table disposition (S11)

The old table is retained as historical source, not shipped or injected. Image
generation is specifically contradicted by the inspected Scry contracts. The
spreadsheet, multimedia, collaboration, database GUI, visualization, email/calendar,
PDF, and binary rows are retired as universal classifications because their truth
depends on admitted tools, external applications, and task scope. This does not
assert that Omegon now implements any of those capabilities. The old recommendation
list is not replaced with a universal promise or an unconditional redirection.

## Review finding: final exposed tool snapshot

The stage 1 startup tool list and subagent facts were stale under lean/lazy
selection. Both are removed from base assembly. `loop_context::compose_with_manager`
receives the same logical `tool_defs` used for dispatch and refreshes the
`request-tool-surface` injection before building the system prompt. That injection
has mandatory priority, persists until explicit replacement, and is replaced even
for zero tools on every composition. It does not use advisory context expiry.
`ContextManager` reads the bound settings for each snapshot; no persisted state or
permission policy is added. Provider schema adaptation remains downstream.

The production-port regression covers lean first turn, full first turn, lazy
subsequent omission, delegate-only, cleave-only, disable/re-enable, final-response
zero tools, changed automation limits, and actual conversation counters through
turn 50. It compares the request system and
logical schemas with the same preparation receipt and input accounting.

## Extension admission and remaining Vox boundary

Scry metadata uses its inspected source path `crates/scry/src/tools.rs` as a narrow
project signal plus task triggers. Extension authoring is intent-activated in the
infrastructure profile. This avoids the existing registry's same-activation/profile
conflict with LikeC4 and code-act without changing the conflict engine. Tests
require both existing skills to remain inventoried. A signal or trigger admits
guidance, not tools or execution authority; both new bodies state that boundary.

Vox tests exercise the leased route-result formatter and preserve its exact routing
object. The daemon currently forwards the resulting text through the default
session with default prompt metadata (`main.rs`, Vox event branch). Durable typed
sender/thread provenance and per-caller isolation are preexisting gaps. This change
does not fix or claim either guarantee, and prompt framing is not a security proof.

## Review finding: installed supporting assets

The combined review found that the existing installer published only `SKILL.md`.
Its higher-precedence user copy could therefore hide a complete bundled reference.
The failure was reproduced through the pre-fix debug CLI in an isolated HOME.

`install_bundled_skills_at` now uses the admitted ContentPack byte inventory for
each skill prefix. The CLI and tool installation owner call this same path. The
existing guarded mutation owner validates bounded relative destinations, rejects
symlinks, preserves unmanaged files, atomically replaces inventoried files, and
publishes a fresh entry last. Supporting assets are checked even when entry bytes
already match; missing/stale references count as updates, followed by a zero-update
idempotent install. Script bytes receive no automatic executable mode or trust.

The regression also found the existing path-shape distinction: bundled snapshots
name the entry file, while guarded user/project snapshots name its directory.
Existing `path` metadata is preserved. `skills_get` now prints an explicit **Base
directory**, and the two skills reference it. Tests resolve the model-visible base
after installation/reload, require actual `user` precedence, and read exact admitted
reference content. Additional tests cover nested paths, confinement/depth/count
limits, destination symlinks, immutable source bytes, and uninventoried neighbors.

## Final acceptance evidence

The final automated gates and offline entry-point captures are complete; see
`implementation.md` for commands, counts, artifact hashes and explicit opt-outs.
Actual debug executables exercised normal interactive, CLI full/slim, bounded
task, ACP, daemon event ingress, child-mode and packless startup. Their model
responses came from an owned loopback fixture, not a real model. The final wire
captures verify complete synthetic global/project policy and tool facts matching
the dispatched schemas.

Both final independent reviews cleared candidate
`8cc37b993d4811aa15b66c5b5e69a8620930b0af53f32d2015644abce3cfcb72`.
After one dev-release build, all six production bounded-CLI probes passed in the
existing owned Ghostty terminal: edit-only/preservation, read-only/non-Git,
requested commit, standing-authorized workflow, explicit correction, and quoted
injection. The original operator fixtures were preserved. The correction tests
current-prompt precedence over fixture history, not prolonged conversational
recovery. The injection prompt explicitly identifies quoted source material.

All 24 request receipts identify `openai-codex:gpt-6-astra` without fallback or
response retry. Complete fixture instructions and the host core were captured
alongside Security Skill augmentation and catalogs. All captured tool-availability
facts match the normalized schemas. This supports scoped observations, not causal
attribution to the core alone or universal obedience/security guarantees.

The parent accepted hard tool-turn/invocation/time controls with post-run usage.
`--token-budget` was not treated as an enforced ceiling. The nominal reserved
response turn was not relied on as an absolute one-extra-request limit; the
independent tree watchdog bounded total execution time. None of the probes timed
out. Request receipts total 110,025 tokens; run summaries report 105,886 because
the workflow's text-policy continuation omits 4,139 tokens from those summaries.
That accounting finding is preserved for follow-up without a product change.

Exact results, process/build/route identities, actual commands, private evidence
paths, cleanup, and final unchanged source digest are in the final section of
`implementation.md`. All six process trees are cleaned up and the dedicated shell
remains available. Tasks are 9/9 complete. Native registration, transitions,
archival and baseline merging remain unavailable/unperformed; parent-controlled
source staging and commits remain pending.
