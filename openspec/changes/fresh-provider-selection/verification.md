# Verification

## Implementation and review

Production implementation: `001b7d0da29c2c44f274200422b08c7bf4bb85d8`.
Terminal acceptance readiness correction: `6db1a0c3` (test tooling only).

Independent review found that command dispatch, model status, and profile application
still inferred readiness from stored credentials. Regression tests failed before those
paths were changed to consume the serving route. Review also added typed withdrawal
handling, stale-route presentation checks, and exclusion of anonymous providers from
automatic account selection. Free selection does not submit the retained draft.

## Scenario coverage

| Requirement | Evidence |
| --- | --- |
| Unconfigured and expired startup | `fresh_provider_cli_has_no_implicit_model`, `fresh_provider_startup_without_model_never_probes_credentials`, disconnected composer/stale-route tests; both unconfigured terminal captures |
| CLI/profile precedence and registry defaults | `fresh_provider_cli_override_and_saved_selection`, noninteractive selection tests, registry default assertions |
| Unselected persistence | `fresh_provider_settings_are_unselected_and_not_persisted_as_a_model`, empty legacy profile and Zen round-trip tests |
| Preserve draft and attachments; cancel setup | `disconnected_submission_preserves_draft_and_attachments_without_starting_turn`, command availability and submit-once tests; both unconfigured terminal captures |
| Explicit free choices and terms | connection discovery projection tests, exact route admission tests, live catalog selection capture |
| Bounded catalog failure | invalid/oversized inventory tests, deadline test covering a stalled body; initial live timeout displayed the retry path |
| Ordinary streaming and tool calls | `zen_bridge_streams_text_and_tool_calls_with_public_credential`, managed route admission test; synthetic public streaming probe |
| Withdrawal before and during execution | route preparation, typed HTTP failure, loop-driver invalidation and late-failure tests; paid fallback credentials never used |
| Throttling | deterministic HTTP 429 fixture retains bounded failure and does not use paid credentials |

## Gates

- Test-first failures recorded for startup/defaults (five failures), disconnected
  composer/submission (two failures), and reviewed route readiness (three failures).
  Focused reruns passed after implementation.
- `env -u NO_COLOR -u OMEGON_ASCII_GLYPHS RUST_TEST_THREADS=1 just test-crate omegon`
  passed: 5,186 unit tests, zero failures, ten ignored, followed by the crate's
  integration checks. This is the crate gate, not a full-workspace test claim.
- `env -u NO_COLOR -u OMEGON_ASCII_GLYPHS just clippy-changed` passed.
- `cargo fmt --all --check`, `git diff --check`, `just test-dev-scripts`, and
  `python3 scripts/tests/test_tui_acceptance.py` passed.
- The existing user-profile capture test was found writing to the operator home.
  It now runs in a child process with isolated home/config paths. Its focused test
  passed with the operator profile still absent. The test-created file was backed
  up before removal; the checkout profile was unchanged.

## Runtime evidence

Local evidence is retained outside Git in the sibling directory
`omegon-provider-onboarding-evidence-01`. Its `build.json` records the clean source
revision, optimized build and frozen executable. Binary SHA-256:
`368803258127ff59c4154107c59214850cb42bc0012f4588687c172f829ef2eb`.

Each capture manifest records the binary hash, terminal command, dimensions,
timestamps, process identity, capture hashes and cleanup result. Tests used private
tmux servers and isolated configuration, with no GUI windows.

| Capture | Result |
| --- | --- |
| `om-unconfigured` | Passed: inline/Active, no model argument, no credentials/profile, draft retained after submit/cancel, zero inference or conversation turns |
| `omegon-unconfigured` | Passed: fullscreen/Full, same disconnected assertions |
| `om-connected-flow-02` | Passed: two replies published once, resize, project browsing, export, permission denial, four local fixture requests, clean exit |
| `omegon-connected-flow` | Passed: fullscreen/Full connected flow, four local fixture requests, clean exit |
| `om-free-connection-04` | Passed: live Zen catalog and data terms, anonymous MiMo selection persisted, draft retained, zero inference/turns, deliberate draft clearing and clean exit |

The first connected inline capture inspected output before the turn closed. The
runner now requires matching authority terminal facts, no working/publication
status, and exactly one reply marker. A failing regression test preceded the fix;
the same frozen binary passed the corrected capture. Original failures remain in
the evidence directory.

The first live catalog GET timed out before a TCP connection was logged. Subsequent
catalog requests and anonymous streaming succeeded with the same application and
dependency versions. This supports a transient network/resolver delay, but does
not establish its cause. The five-second deadline and retry behavior are retained.

The live probes also exposed an existing idle Ctrl+C ingress defect: the priority
interrupt relay drops idle interrupts before they reach the editor's clear/quit
handler. That cancellation-routing change has a separate OpenSpec follow-up.
Connection tests dismiss the command result panel, then use Ctrl+U and verify the
draft is cleared before entering `/quit`.

## Limits

Anonymous offerings are curated and intersected with live inventory. Availability
and data terms can change independently of Omegon releases. Public probes used
synthetic text in isolated workspaces; provider tool-call behavior, withdrawal and
throttling were verified with deterministic HTTP fixtures. Real GUI-terminal
compatibility was not rerun in this pass. The installed launcher and previous
WezTerm preview were not replaced; the frozen artifact above was tested directly.


## Native admission correction (2026-09-07)

A real /model action exposed valid Codex credentials being routed through generic
HTTP manifest admission. Native admission now checks registered native execution,
canonical provider/model identity, and field provenance. Embedded transport/adapter/
secret declarations retain native construction. Custom overrides retain manifest
validation. Discovery cannot overwrite a declared offering's endpoint or native
model identity, but can refresh observations or introduce new offerings.

Four focused native-admission tests passed. They exercise actual Codex, Anthropic,
and OpenAI bridge construction and controller switches using synthetic credentials;
embedded and discovery inventories; critical operator overrides before and after
discovery; declared identity preservation; new discovered identities; and ordinary
tool request validation for Fable 5.1 and Mythos 5.1. The two model declarations now
include verified tools evidence. No forced tool choice was added and the Anthropic
adapter already omits tool_choice. Source references are recorded in design.md.

Final validation passed all 5,264 Omegon tests (11 ignored), affected-target Clippy,
formatting, and spec validation. Private inline/fullscreen configured captures
confirmed Astra as serving without inference, OAuth refresh, browser launch, or
fixture auth mutation. The connection verification document records artifact
identity, accepted capture directories, cleanup and retained failed attempts.

The first native fixture used the wrong credential-source label and later assumed
Managed project overrides could reach route admission; both expectations were
corrected to match existing contracts. Its tools requirement also exposed the
Fable metadata omission; that failing result is retained before the data correction.
Raw logs live under `../omegon-connect-feedback-evidence-01/logs`.

This pass verifies current Fable 5.1/Mythos 5.1 tool declarations and Astra routing.
Other Anthropic catalog entries still need a broader tool-capability evidence audit;
this change does not infer tool support for every provider/model. No live provider
inference, account access, or model context-limit assertion is claimed.
