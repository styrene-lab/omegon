# Verification

## Upstream evidence

- [GPT-6 Astra model specification](https://developers.openai.com/api/docs/models/gpt-6-astra): exact ID, API context 1,050,000, output 128,000, reasoning low/medium/high/xhigh/max and extended-context pricing threshold.
- [Astra migration guidance](https://developers.openai.com/api/docs/guides/latest-model): tool calls require Responses; unsupported sampling/logprob fields are omitted; none/minimal effort maps to low.
- [Stateless reasoning continuation](https://developers.openai.com/api/docs/guides/reasoning#preserve-reasoning-without-stored-responses): preserve output items including encrypted reasoning and assistant phase when replaying tool work.
- [Fable 5.1 specification](https://platform.claude.com/docs/en/models/fable-5-1/overview): current model, context/output and adaptive-thinking behavior match existing support. The live Anthropic drift check reported no current models missing.
- Installed Codex catalog: exact Astra slug is listed and supported, with `context_window=272000` and `max_context_window=872000`. Sanitized model-only evidence is retained; credentials were not copied. The subscription route uses this separate maximum, not the API ceiling. Codex-specific ultra delegation behavior is not exposed as an API effort option.

## Test-first evidence

Logs are retained outside Git in the sibling `omegon-frontier-model-evidence-01` directory.

| Boundary | Initial failing evidence |
| --- | --- |
| Registry | Astra was missing from native OpenAI routes |
| Transport | Tool-bearing Astra request targeted Chat Completions |
| Eligibility | Codex Astra access failure lacked route-specific guidance |
| Reasoning | xhigh did not parse, max collapsed to high, Astra used the short stall allowance |
| Admission | Embedded OpenAI HTTP metadata incorrectly entered custom-manifest secret admission |
| Continuation | A second request dropped encrypted reasoning and assistant phase |

## Review decisions

Independent review found the native/manifest admission boundary and lossy Responses continuation. The native exception must honor embedded ownership of endpoint transport, adapter and secrets, plus offering endpoint/native identity; overrides retain manifest admission. Opaque output replay is restricted to the same provider and exact wire model. Request extras cannot attach a different remote conversation.

Fifteen Pkl fixtures passed: xhigh/max accepted and preserved, ultra rejected, across Profile, TaskSpec, both AgentManifest settings locations and CodexIntegration. Existing lower-cost grades and explicit profile selections are retained. Fresh disconnected startup remains connection-first.

## Final gates and runtime

- The complete Omegon crate gate passed after the default/effort expectations were updated: 5,200 unit tests passed, 10 ignored, plus its integration checks.
- Fourteen focused Astra tests passed, including actual two-request HTTP continuation, empty-output fallback, partial-output rejection, model picker visibility, route ownership and reasoning levels.
- A final small compatibility adjustment retains the old High mapping for Max on legacy OpenAI/manual-Anthropic adapters and clamps GPT-OSS effort to its supported high ceiling. All 139 provider tests passed after that adjustment.
- Changed-crate Clippy, formatting, registry validation, and the live Anthropic model drift check passed.
- The earlier preview's generated Sonnet selection was backed up and removed using an exact pre-recorded file hash. Other project preferences remain unchanged; no credentials were cleared.

## Installed artifact and captures

The release build is `omegon 0.29.0-dev (e6ca85d 2026-09-06)`, built from clean revision `e6ca85d2a72e4a6fe12c52726460b91ea11c93b2`. The artifact is `target/release/omegon`, SHA-256 `d5ca48a39a7dca4ec2e0ddcc896a6abf1e1a22431e23d1c247d2c743eaa67689`. Both installed launchers resolve to it; the default channel also resolves to it outside the checkout.

The private PTY capture in `installed-picker-01` passed against that artifact. The normal connected shortlist visibly includes Anthropic: Claude Fable 5.1, OpenAI: GPT-6 Astra, and OpenAI Codex: GPT-6 Astra. The reasoning selector includes xhigh and max. Synthetic credentials were confined to a temporary home. No provider requests or conversation turns occurred. The owned terminal process tree was cleaned up; no GUI windows were created. Artifact, process, capture hashes, dimensions and actions are recorded in its manifest.

`just link` built and installed the binary/companion, launchers, content pack and skills, then stopped at catalog installation: existing maintenance state records home device `16777231`, whereas the current device is `16777233` for the same path and inode. Guard state was preserved. The remaining `just install-codescan-extension` and `just install-default-extensions` steps passed separately. This is a partial link result, not a passing complete link gate. Paid upstream inference and account eligibility were not tested.

The real-home startup check in `operator-home-startup-01` reached Choose a connection and returned to the shell through /quit. It used a temporary project, preserved the absent global profile, and cleaned up the owned process tree. The home identity mismatch is not a core TUI startup blocker, but user skill/plugin scopes and extension discovery fail closed; this install is therefore limited for testing those contributions. Existing OAuth refresh also returned invalid_grant. Neither condition was bypassed or misrepresented as a successful provider connection. The current model routes can be tested after a successful /connect; restoring guarded contributions requires a separate maintenance-state recovery change.
