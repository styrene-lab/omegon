# Resolved inference policy — verification record

## Scope and lifecycle

Implementation and the five review fixes are complete on `feat/resolved-inference-policy`, based on
`6b276f3b8fc7e610ff61dab961d3e21778114c00`. The user accepted all three policy
defaults and the boolean `Enabled` representation clarification. The change has
33 scenarios and 12 implementation tasks. All 12 tasks are checked after the
current-source regression, affected-crate, Clippy, and private headless checks
passed. Both original independent reviewers cleared all five P2 fixes against
the reviewed code hash below. Live Ghostty verification remains parent-owned
after local commits; this change is not archived.

Native task/test registration and ledger reconciliation are unavailable in this
harness. No native history was fabricated and no archival was performed.
Persisted session-v1 facts and replay payload meanings are unchanged. The live
projection DTO is additive. Optional credential identity metadata is described
in [design.md](design.md).

The existing Ghostty session has not been rebuilt, restarted, or driven. Final
same-terminal verification remains parent-owned. Automated reviews are complete.

## Automated evidence

Commands use the canonical checkout and its pinned Nix environment, with
`RUST_TEST_THREADS=1` and `CARGO_NET_OFFLINE=true`. Cargo gates run serially.

- Structural OpenSpec validation passed with all 12 tasks checked (`verifying`).
- The focused run in `resolved-policy-expanded-5.log` passed 26 tests.
- `just lint` passed (exit 0), including workspace check and strict all-targets
  Clippy. `just clippy-changed` subsequently passed (exit 0). The final-source
  post-review validation is recorded below.
- Unfiltered `just test-rust` exited 101: 5,411 main-crate tests passed, 11 were
  ignored, and only the pre-existing LikeC4 activation metadata test failed.
- The next workspace run excluded that single test. Main-crate tests passed
  (5,412 passed, 11 ignored, one excluded), as did its integration targets. The
  run later exited 101 at the separate pre-existing
  `omegon-skills::disclosure::tests::every_bundled_skill_declares_matchable_signals`:
  `ratatui-tui` declares unsupported signal `**/tui/**/*.rs`.
- The affected-crate landing command passed (exit 0), including integration
  targets and doctests, with the LikeC4 test explicitly excluded:
  `cargo test -p omegon -p omegon-memory -p omegon-native-extension-host -p omegon-secrets -p omegon-traits -p omegon-web -p styrene-work-model -p styrene-work-runtime --locked --offline -- --test-threads=1 --skip skills::tests::bundled_skills_declare_activation_metadata`.
  Live-provider opt-in environment flags were explicitly set to zero.
- A separate packaging owner subsequently fixed `skills/likec4/SKILL.md` and
  regenerated `content-pack.toml`. That owner reported the activation test and
  manifest check passing. These two files form a separate logical packaging
  change, not a policy semantic change.
- `pkl eval --format json pkl/Profile.pkl` and `git diff --check` passed.
- Fresh unfiltered `just test-commit` passed (exit 0) on the five-fix source:
  main-crate unit tests: 5,419 passed, zero failed, 11 ignored, zero excluded.
  All main integration targets and the `omegon-memory`,
  `omegon-native-extension-host`, `omegon-secrets`, and `omegon-traits` gates,
  including their doctests, passed. Log: `resolved-policy-final-test-commit.log`.
- Fresh `just clippy-changed` passed (exit 0), including formatting and strict
  all-targets Clippy for all five affected crates.
  Log: `resolved-policy-five-fixes-clippy.log`.
- Private headless acceptance passed (exit 0) against `target/debug/omegon`:
  two normal turns, subsequent permission interaction, resize, and shell return.
  The fixture handled four local requests, created zero GUI windows, and
  verified cleanup. Startup showed `usage unavailable`; the completed second
  turn showed `~14k / 124k context (12%)`, with `default/unknown` reasoning on the
  custom fixture route. Earlier broader workspace failures remain historical
  evidence; the final affected-crate gate has no excluded tests.

### Private runtime receipt

Command:

```sh
direnv exec . env TMPDIR=/private/var/folders/ln/r6np9wfn1wx8r6sdnvn07sxw0000gn/T/opencode \
  python3 scripts/tui_acceptance.py --binary target/debug/omegon \
  --output /private/var/folders/ln/r6np9wfn1wx8r6sdnvn07sxw0000gn/T/opencode/resolved-policy-e7976e47-pty \
  --tui inline --ui active
```

The evidence directory contains `manifest.json`, hashed captures, the driver,
source diff, and authority receipts. The binary SHA-256 is
`70f7d01648c3d263c8dac08b3caf45b613d774b1237030a34f002a8f63f73b76`.
The captured full source-diff SHA-256 is
`ad0f3aa8b034fd4cf14580312ad22c4d5a2971a503512a2cca529cf45dd4c211`.
This full diff includes documentation and the separate packaging correction;
the policy code-diff hash below remains stable across final evidence updates.

## Independent review fixes

The independent adapter/UI and core reviews identified five P2 findings. The
fix delta is limited to `providers.rs`, `providers/policy_tests.rs`,
`inference_policy.rs`, and `bootstrap.rs`:

1. Disabled reasoning on supported Codex models now validates the Responses
   `/reasoning/effort` field. Chat Completions validates `reasoning_effort`.
   An irrelevant matching field cannot mask a contradictory effective field.
   Regression: `resolved_policy_supported_codex_off_uses_responses_effort_field`.
2. Local and cloud GPT-OSS routes are categorical and cannot disable reasoning.
   Off rejects before evidence/transport and preserves stored intent. Minimal
   still sends low. Disable-capable Qwen-style routes still send explicit false.
   Regression: `resolved_policy_gpt_oss_off_rejects_but_boolean_routes_send_false`.
3. OpenRouter, Ollama Cloud, Copilot, and compatible wrappers now expose opaque
   identities for their configured transports. Custom endpoints exclude native
   metadata; token changes do not change endpoint identity.
   Regression: `resolved_policy_configured_endpoints_isolate_consent_and_ignore_token_rotation`.
4. Fact selection establishes the highest applicable source/publisher group
   before checking conflicts. Superseded conflicts no longer make selection
   depend on input order. Unresolved top conflicts still reject before evidence.
   Regression: `resolved_policy_top_revision_conflicts_are_order_independent_before_transport`.
5. Bootstrap restores saved raw reasoning independently of legacy enum parsing
   when there is no explicit CLI posture override. Both full/slim flag paths
   preserve structured and provider-default intent without rewriting profiles.
   Regression: `resolved_policy_bootstrap_preserves_raw_profile_intent_across_layout_postures`.

The final five-fix focused run passed: 35 passed, zero failed, exit 0, in
`resolved-policy-five-review-fixes.log`. Production is frozen at code-diff hash
`e7976e474af74d6d77b0fb36a0b1253b2feb26a75bed532ec9927859056502a8`, computed with
`git diff 6b276f3b8fc7e610ff61dab961d3e21778114c00 --binary -- core data pkl | shasum -a 256`.
The unfiltered `just test-commit` and `just clippy-changed` runs passed on this
hash. Both original reviewers subsequently inspected the final source and
cleared their findings: endpoint identity, fact supersession, bootstrap intent,
supported Codex Off, and non-disableable GPT-OSS Off. These were source rechecks,
not additional test runs. No Rust or policy source changed after clearance.
Only verification prose and the separate packaging changelog entry were updated
for the local commits. Final same-terminal rebuild and live verification remain
pending with the parent.

Logs are under the approved temporary directory, with prefix
`/private/var/folders/ln/r6np9wfn1wx8r6sdnvn07sxw0000gn/T/opencode/resolved-policy-`.
These paths are machine-local evidence, not portable repository artifacts.

## Scenario-to-test map

Test paths below are relative to `core/crates/omegon/src/`. A mapping identifies
the regression owner; it is not a claim that every final gate has passed.
Names beginning `resolved_policy_` are runnable with one Cargo filter.

| Scenario | Regression owner |
| --- | --- |
| API facts cannot fill an OAuth route by model name | `inference_policy_tests.rs`: `resolved_policy_api_codex_isolation_and_legacy_fallback` |
| Unknown route uses labeled conservative assumptions | Same test; `providers/policy_tests.rs`: `resolved_policy_unknown_default_and_omitted_controls_are_not_disabled` |
| Stale evidence retains its bounds | `inference_policy_tests.rs`: `resolved_policy_discovery_supersedes_stale_and_unordered_conflicts_reject` |
| Contradictory applicable facts block preparation | Same test; `resolved_policy_exhaustion_and_capacity_semantics` |
| New applicable metadata supersedes a declared snapshot | `resolved_policy_inventory_merge_preserves_scope_and_metadata_generation` |
| Maximum does not become the default working window | `resolved_policy_stable_key_generation_and_legacy_nonconsent`; `resolved_policy_configured_metadata_cannot_raise_declared_capacity` |
| Explicit larger target and narrower caps | `resolved_policy_target_precedence_invalid_winner_and_narrowing` |
| Highest priority numeric target wins or fails without fallback | Same test |
| Maximum action must materialize a known numeric target | `resolved_policy_numeric_maximum_does_not_follow_refresh`; `session_settings_commands.rs`: `resolved_policy_context_control_and_saved_off_preserve_profile_intent` |
| Invalid overrides fail without silent clamping | Same control test; `providers/policy_tests.rs`: `resolved_policy_invalid_intent_and_late_overrides_never_open_transport` |
| Unknown capacity does not disguise a configured target | `resolved_policy_numeric_maximum_does_not_follow_refresh` |
| History makes otherwise valid fixed overhead overflow | `providers/policy_tests.rs`: `resolved_policy_history_overflow_repair_and_independent_auxiliary` |
| Shared output and cached input are counted once | `providers.rs`: Responses continuation deduplication regressions; `providers/policy_tests.rs`: `resolved_policy_late_provider_usage_stays_with_its_request`; `loop_context.rs`: memory-attribution regression |
| Enforced output cap reduces total-window input fit | `model_request.rs`: `resolved_policy_generation_cap_validates_complete_input_boundary` |
| Genuine input-only bound does not subtract output | `resolved_policy_exhaustion_and_capacity_semantics` |
| Unspecified window basis uses conservative host accounting | Same test |
| Heuristic conservatism and incomplete enforcement stay inspectable | Same test; `providers/policy_tests.rs`: `resolved_policy_output_enforcement_and_final_wire_guard` |
| Named configured threshold controls compaction | `loop_context.rs`: `resolved_policy_configured_threshold_controls_real_loop_compaction` |
| Portable Minimal normalizes visibly to low | `providers/policy_tests.rs`: `resolved_policy_prepared_minimal_matches_wire_and_retry_capture`; `tui/tests.rs`: `resolved_policy_composer_uses_captured_full_input_and_invalidates_on_switch` |
| Saved Off requires resolution on Astra | `model_request.rs`: `resolved_policy_rejects_saved_off_before_evidence_or_transport`; profile/control regression |
| Unsupported concrete effort cannot hide behind an override | `resolved_policy_invalid_intent_and_late_overrides_never_open_transport` |
| Noncategorical and unknown-default modes retain their meaning | `resolved_policy_boolean_and_anthropic_modes_match_adapter_fields`; `resolved_policy_unknown_default_and_omitted_controls_are_not_disabled` |
| Boolean reasoning is distinct from categorical effort and omission | Same adapter tests, including contradictory `think` override rejection |
| Connection and metadata changes do not mutate in-flight policy | `resolved_policy_incompatible_generation_rejects_before_bridge_stream`; existing request-capture/lease tests |
| Routine token refresh preserves intent for the stable route | `auth.rs`: `resolved_policy_credential_refresh_preserves_connection_without_fabricating_legacy_identity`; `resolved_policy_stable_key_generation_and_legacy_nonconsent` |
| Incompatible generation cannot silently retarget dispatch | `resolved_policy_incompatible_generation_rejects_before_bridge_stream`; `provider_route_service.rs`: `stale_contribution_generation_is_rejected_before_recording` |
| Compaction and sessionless auxiliary models have independent policy | `resolved_policy_history_overflow_repair_and_independent_auxiliary`; existing `prepared_compaction_*`, `prepared_auxiliary_*`, and `auxiliary_preparation_rejects_session_append_authority` tests |
| Retry and repair preserve their distinct identities | `resolved_policy_prepared_minimal_matches_wire_and_retry_capture`; `resolved_policy_history_overflow_repair_and_independent_auxiliary`; existing semantic request/repair authority tests |
| Late protocol override cannot contradict a captured policy | `resolved_policy_output_enforcement_and_final_wire_guard`; `resolved_policy_invalid_intent_and_late_overrides_never_open_transport` |
| A smaller assembly window cannot acquire a registry denominator | `surfaces/inference_policy.rs`: `resolved_policy_projection_has_one_denominator_including_mandatory_input`; actual composer regression |
| Startup and model switch do not fabricate current usage | `resolved_policy_startup_and_old_usage_do_not_fabricate_current_percentage`; actual composer regression |
| Provider usage does not overwrite another request estimate | `resolved_policy_late_provider_usage_stays_with_its_request` |
| Operator can explain a normalized request and early compaction | Semantic status projection, profile/control, and real-loop threshold regressions |

## Operator controls and expected behavior

- `/context status` reports policy inputs, selected and configured facts,
  superseded observations, route identity, generations, reserve equations,
  output coverage, normalization, thresholds, and source age.
- `/context capacity 600000` sets a live-session numeric target for the captured
  serving route. Narrower class/posture limits remain visible.
- `/context capacity maximum` materializes the current known numeric maximum.
  An unknown maximum requires a number. `/context capacity reset` removes the
  session target and exposes lower-priority profile/default selection.
- `/context reasoning minimal` requests portable Minimal. Astra resolves to low.
  `/context reasoning provider-default` removes explicit session reasoning.
- Profile `routeContextTargets` uses the full route object shown by inspection.
  A project target takes priority over a user target. Stored legacy
  `contextWindow` does not authorize enlargement.

For the reviewed Codex Astra fixture, default `W=272000` and maximum `872000`
remain distinct. Portable Minimal preserves the 2,000-token heuristic, so a
full-reply host allocation is `G_h=8192+2000=10192`. Codex emits no total-output
cap. With no narrower host cap or input safety reserve, the unspecified-basis
host input budget is `272000-10192=261808`. Actual schemas are counted in input,
not reserved a second time. This is a local estimate, not account entitlement or
provider-exact tokenization.

After review, the parent should preserve active operator work, rebuild through
the established same-window procedure, verify executable identity, then inspect
startup usage, Minimal normalization, one normal reply, capacity controls, and
unsupported Off feedback in the recorded Ghostty terminal. Existing controls are
listed above; no new terminal or window is needed. This record does not claim
that live Ghostty verification occurred.

The existing terminal receipt identifies Ghostty terminal
`D7020F2F-64D9-4B2C-9A90-DEFEF4F0AEF4`, window `tab-group-76fdb92c0`, and
`/dev/ttys004`. Its `.git/dev-terminal/helpers.zsh` defines `odev` as
`direnv exec . just run --tui inline --ui active --fresh --no-splash`.
After preserving work and reaching the shell with `/quit`, the parent can use
that existing helper. Validate current process identity before using recorded
PIDs; the receipt is historical. No command has been sent to that terminal by
this implementation session.
