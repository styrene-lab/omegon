# Resolved inference policy — verification record

## Scope and lifecycle

Implementation and the five review fixes are complete on `feat/resolved-inference-policy`, based on
`6b276f3b8fc7e610ff61dab961d3e21778114c00`. The user accepted all three policy
defaults and the boolean `Enabled` representation clarification. The change has
33 scenarios and 12 implementation tasks. All 12 tasks are checked after the
current-source regression, affected-crate, Clippy, and private headless checks
passed. Both original independent reviewers cleared all five P2 fixes against
the reviewed code hash below. Same-window live Ghostty verification subsequently
passed on local commit `de0e18821acce3ec7e5a241601eb75d0c3597cd1`, as recorded
below. This change is not archived.

Native task/test registration and ledger reconciliation are unavailable in this
harness. No native history was fabricated and no archival was performed.
Persisted session-v1 facts and replay payload meanings are unchanged. The live
projection DTO is additive. Optional credential identity metadata is described
in [design.md](design.md).

At the automated-review handoff, the existing Ghostty session had not been
rebuilt, restarted, or driven. The subsequent live check preserved that session
through `/quit`, then rebuilt and launched a fresh session in the same window.

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
for the local commits. The subsequent same-terminal rebuild and live verification
used this unchanged policy code hash.

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

The post-review procedure was to preserve operator work, rebuild in the same
window, verify executable identity, then inspect startup usage, Minimal
normalization, normal replies, capacity controls, and unsupported Off feedback.
The live results below complete that procedure.

The existing terminal receipt identifies Ghostty terminal
`D7020F2F-64D9-4B2C-9A90-DEFEF4F0AEF4`, window `tab-group-76fdb92c0`, and
`/dev/ttys004`. Its `.git/dev-terminal/helpers.zsh` defines `odev` as
`direnv exec . just run --tui inline --ui active --fresh --no-splash`.
The helper does not forward arguments. The live check therefore used the exact
canonical command with an explicit Astra model, as recorded below. Process IDs
are historical evidence and must be revalidated before subsequent control.

## Same-window live verification — 2026-10-08

### Source, build, and ownership

After all automated reviews cleared, the owned window showed an idle Astra
session with an empty composer. `/quit` saved that session and returned to the
existing shell, PID `14782`, on `/dev/ttys004`. The old Omegon PID `15016` and its
`just` parent `14970` exited. No force termination was used.

The following command ran once in that same shell:

```sh
direnv exec . just run --tui inline --ui active --fresh --no-splash --model openai-codex:gpt-6-astra
```

The native terminal reported a successful `dev-release` build in 2m 09s. The
running executable was verified through its process identity and executable
mapping as `/Users/wilson/workspace/styrene-lab/omegon/target/dev-release/omegon`:

- Source: `de0e18821acce3ec7e5a241601eb75d0c3597cd1` on
  `feat/resolved-inference-policy`; reviewed policy code hash unchanged.
- Binary SHA-256:
  `bd7d4eb6a00a8158c35d93fafefcd2e06f0c657f0b7029934cbfa5c4bef3dde1`.
- Artifact modification time: `2026-10-08T20:57:17Z`.
- Omegon PID `95800`, PGID `95753`, TTY `ttys004`. Its IPC startup timestamp is
  `2026-10-08T20:57:20.010436Z`; the PID existed earlier as the build wrapper.
- Ghostty terminal `D7020F2F-64D9-4B2C-9A90-DEFEF4F0AEF4`, window
  `tab-group-76fdb92c0`, native window `598`, owner PID `694`, Ghostty `1.3.1`.
  The application changed the window title to `Ω omegon ✦`.

Only that window was captured. The established system-SDK capture procedure
worked without permission changes. Status receipts came from the same live
process through its existing IPC socket, with `server_pid == 95800` verified.
Session-setting commands and both prompts were submitted through the owned
Ghostty terminal. No replacement terminal, global installation, or site process
was created or changed.

### Observed policy and live results

| Check | Observed result |
| --- | --- |
| Fresh startup | Effective reasoning `low`; usage and percentage unavailable until request preparation. Advertised default `272000` and maximum `872000` remained distinct. |
| Actual host budget | Existing posture and working-set class each capped the working window at `131072`. Generation heuristic was `4096 + 2000 = 6096`, giving input budget `124976`. These narrower settings explain the difference from the full-reply fixture above. |
| `/context reasoning minimal` | Reported `Session reasoning: low`. Status preserved requested `minimal`, normalization to low, and the 2,000-token host heuristic. Codex output coverage remained explicitly unenforced. |
| First live prompt | `Reply exactly: policy-ready` returned `policy-ready`. Prepared input was `24850 / 124976` tokens, `19.883818%`; the composer showed approximately `24k / 124k context (20%)`. |
| `/context reasoning provider-default` | Cleared explicit requested reasoning; declared default still resolved to low. Current request estimate became unavailable, while prior provider usage retained its original snapshot identity. |
| `/context capacity 600000` | Stored session target `600000` for the captured OAuth route. Narrower host caps retained budget `124976`; no enlarged-context provider request was sent. |
| `/context capacity maximum` | Materialized numeric target `872000` with metadata revision `2fdf047c9631c9ed01a31b62efb7891718a931a8`. Host caps still applied. |
| `/context capacity reset` | Cleared the session target and restored lower-priority default selection. |
| `/context reasoning off` | Rejected at command validation: `reasoning needs resolution: off is unsupported`; the diagnostic stated that saved intent was unchanged. No Off prompt was submitted. |
| Restoration and second turn | Restored Minimal, with capacity still reset. `Reply exactly: policy-ready-again` returned `policy-ready-again`. The new prepared snapshot measured `24908 / 124976` tokens, `19.930227%`, with effective low. |

The first and second prepared snapshot IDs were
`d90124ae-6cf5-4f2e-a2a6-3e85d1807d69` and
`f92578af-2847-4162-b198-bf816f2b0cc6`. Their corresponding provider measurements
were 19,995 input / 6 output tokens and 20,016 input / 8 output tokens. Provider
measurements remained separate from local prepared-input estimates.

Final authoritative state was `busy: false`, `active_turn: idle`, queue depth
zero, and zero tool calls. Both replies and the empty, reusable composer were
visually verified. One observation script initially waited for `turns >= 2` and
timed out: that field remained `1`. The second reply, distinct prepared snapshot,
provider usage, and authoritative idle state established completion without
resubmitting the prompt or restarting the app.

The check performed two successful small live prompts, one unsupported-Off
rejection, and one capacity reset. Project/user profile and active-profile
hashes or absence markers were unchanged. Authentication worked without another
login. The two contribution-scope warnings matched the pre-existing baseline.

### Private evidence and retained state

Evidence is under `.git/dev-terminal/resolved-policy-20261008T205427Z/`, with
directory mode `0700` and receipt/capture mode `0600`. It includes source and
artifact identities, native window binding, per-control semantic status, profile
hash comparisons, and captures. Key captures are `03-build-result.png`,
`06-first-reply.png`, `11-off-rejected.png`, and `13-second-observation.png`.
`13-final-status.txt` and `13-final-session.json` record the final policy and
idle state. Screenshots and runtime conversations remain machine-local.

The app and dedicated window were intentionally left running. Production source
was unchanged. Live startup created the untracked runtime cursor
`.omegon/audit-consumer-cursor-v2.json`; it was left unstaged, alongside the
pre-existing personal `opencode-resume` file. This evidence update changes only
this verification document and does not claim archival or native ledger updates.
