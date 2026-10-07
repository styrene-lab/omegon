# Verification

## Integration acceptance — 2026-10-07

[PR #249](https://github.com/styrene-lab/omegon/pull/249) merged at
`77996814af7720a45f15fd482ac8579093ebdd6f`. Its first parent is
`024a8616bfee6f4b3c3bf89fc9c32a2f540e8c48`, so combined main includes request
preparation #247, CI repair #248, provider repair #250, and quiet inline #249.
The bounded inline implementation is accepted. Landing and lifecycle and
architecture reconciliation are authorized.

### Evidence chain

| Evidence | Exact source or artifact | Result and boundary |
| --- | --- | --- |
| Request preparation author gates and independent review | PR #247, merge `13760f25` | Passed; scenario mapping and original test evidence remain in the [request-contract record](../unified-model-request-contract/verification.md). |
| Provider CI and hosted round trips after CI repair #248 | Provider head `a08d526106fe068cf573a12b70782b14a979e15a`, merged as #250 at `024a8616` | CI run `37663285447`: 30 jobs and 31 PR checks passed. Hosted run `37663276660`: Anthropic, OpenAI, and Ollama Cloud round trips passed. Local Ollama and issue reporting were skipped. |
| Inline PR CI | `f21cc8d6c9308bfe18026f56768552e127a1846c` | [Test run 37664057629](https://github.com/styrene-lab/omegon/actions/runs/37664057629): all 30 jobs passed. Lipstyk also passed. Rust 1.95 was used; inline and missing-AgentEnd regressions passed. The opt-in PTY detachment test actually ran: one passed, zero ignored. |
| Earlier private stress and resume | Frozen binary SHA-256 `5caf296bce616b05f77c127c5bc8f854b0a8aa29648fd2ee098e6eb526d9bc8d` | Passed with retained source/driver identity and cleanup evidence below. This is not a build of the combined merge. |
| Combined-main headless acceptance | Pristine `77996814af7720a45f15fd482ac8579093ebdd6f`, frozen binary SHA-256 `cf3210740a3f38fca9aa1f500a800952559d0e258a11d3fd7e6eda4e11758388` | Passed: six stress requests, one resume request, 43 captures, and verified owned-process cleanup. No GUI compatibility claim. |
| Exact-head post-merge CI | `77996814af7720a45f15fd482ac8579093ebdd6f` | [Run 37671759878](https://github.com/styrene-lab/omegon/actions/runs/37671759878) completed with 29 successful jobs and one cancelled Rust build. The 30-minute job limit expired during task-capsule release compilation. This is not an all-green gate or a code assertion failure. |

The integration owner supplied the completed CI and runtime results above. This documentation
update does not rerun source tests, provider calls, or runtime acceptance. The
original frozen binary reports `13760f2-dirty`; retained source comparisons bind
it to the reviewed inline changes. Its earlier stress/resume evidence remains
valid within that boundary and does not establish combined-main acceptance.

### Combined-main headless acceptance — 2026-10-07

The integration owner built pristine `77996814` with the locked Nix Rust 1.95
toolchain. The frozen binary reports `omegon 0.29.0-dev (7799681 2026-10-07)` and
hashes to `cf3210740a3f38fca9aa1f500a800952559d0e258a11d3fd7e6eda4e11758388`.
Private receipt: `integration-77996814/final-result.json` under the approved
OpenCode evidence directory. Its result is `runtime_pass_ci_blocked`.

The private headless tmux stress run passed six local provider requests and
retained 32 captures. It verified the quiet replay notice while the provider
was held, ordinary second submission, real denied write without file mutation,
cancellation and recovery, and inspector/draft preservation. The resume run
passed one local request and retained 11 captures. It verified actual saved-session
resume, borrowed-menu behavior, primary history, drafts, and thinking selection.
Both manifests report cleanup success, zero remaining owned groups, and zero
GUI windows. The fixture rejected external HTTP(S) probes to GitHub, OpenAI,
and Ollama; it is not an OS network sandbox.

Exact-head CI completed with 29 successful jobs and one cancelled job. The Rust
build exceeded `30m0s` in **Build and exercise task capsule release binary**.
Preceding build, composition, feature/Clippy, and contract steps passed. The
timeout leaves that gate blocked; it does not identify a failed code assertion.

This evidence belongs only to exact `77996814`. A later rebased TUI tip needs
its own validation. This documentation integration did not rebuild or rerun
the application.

### Remaining acceptance and administrative closure

Combined-main headless acceptance is complete. Exact-head CI remains blocked
by the Rust-build timeout above. Native GUI compatibility was not exercised.

All seven quiet-inline tasks remain checked. The OpenSpec artifact-derived stage
is `verifying`; the change is not archived. User authorization includes closure,
but native task/test registration, ledger reconciliation, and baseline-aware
archival remain blocked by the
[lifecycle closure gap](../../../docs/lifecycle-closure-reconciliation.md).
Standalone archive readiness is not a native lifecycle transition. Completed
tasks and guarded state have not been rewound to bypass this boundary.

At the closeout inspection, the session had no native lifecycle-management tool.
The installed launcher could not resolve its checkout, and the available debug
binary had no standalone management command. These historical operating
limitations do not establish a product requirement.

### Documentation validation — 2026-10-07

Read-only checks used an explicit `--root` pointing to this documentation
worktree's `openspec/`, after integrating merged main `77996814`:

| Change | `validate` | `archive --check` |
| --- | --- | --- |
| `unified-model-request-contract` | `OK (verifying)` | `ARCHIVE READY` |
| `quiet-inline-replay` | `OK (verifying)` | `ARCHIVE READY` |

Local links, heading anchors, and immutable source targets passed. Canonical
design fields, sections, and matching open-question lists were manually reviewed
against `design_artifacts.rs`. Task files are byte-identical to combined main:
12 checked request-contract tasks and seven checked quiet-inline tasks.
`git diff --check` passed. Source, baseline, and archive contents match combined
main; these documentation checks did not run builds or mutate lifecycle state.

## Historical source and environment — 2026-10-07, before merge

- Branch: `feat/quiet-inline-unification`.
- Base: `13760f256662de5cbb8089d38990f7db3c93d89d`, verified against `origin/main`
  and read-only `git ls-remote origin refs/heads/main`.
- Isolated worktree: the `omegon-inline-unification` checkout.
- Canonical environment: `nix develop`, Rust 1.95.0 / Cargo 1.95.0 from the
  locked Nix toolchain.
- Compilation uses this worktree's own `target/`; no shared artifact overwrite.
- Reference checkout was read-only and matched the requested commit and dirty-state boundary.

## Historical deterministic checks — 2026-10-07, before merge

| Check | Result |
| --- | --- |
| `cargo test -p omegon inline_splash_preserves --locked` before implementation | Failed at the expected disabled-replay notice assertion: original slash dispatch returned `Handled`. |
| Same regression after implementation | Passed (one test, idle/active × inline/borrowed fullscreen cases). |
| Authoritative completion/idle/next-turn, stale-advice, publication and priority-input regressions | Passed: 1 supervisor-completion test, 2 authoritative-idle tests, 1 stale-advice test, 32 publication tests, 4 priority-input tests. |
| `python3 scripts/tests/test_tui_acceptance.py` (Python 3.14.8) | Passed after final driver changes, including rejection of missing notice, alternate-screen takeover, hidden cancel control, and external HTTPS CONNECT. |
| `cargo fmt --all -- --check` through Nix | Passed |
| OpenSpec `validate quiet-inline-replay` | Passed; structural validation only. |
| `just clippy-changed` through Nix | Passed, including all-targets Clippy with warnings denied. |
| `RUST_TEST_THREADS=1 just test-crate omegon` through Nix | Passed on the completed sequential rerun: 5,366 unit tests passed, zero failed, plus all integration targets in the recipe. The repository's existing ignored tests were retained. Earlier incomplete attempts are recorded below. |
| `RUST_TEST_THREADS=1 just test-dev-scripts` through Nix | Passed on the completed sequential rerun, including optional-domain validation and final launcher tests. Earlier pre-main launch delay is recorded below. |

### Earlier broader-gate attempts

The crate gate reported these failures in unchanged extension owners:

- `extensions::conformance_tests::lif_001_lif_003_real_process_quiescent_replacement`
- `extensions::conformance_tests::replacement_enforces_tool_shape_and_preserves_stable_invocation`
- `extensions::conformance_tests::shutdown_settles_fixture_descendants`
- `extensions::conformance_tests::stale_generation_is_rejected_before_extension_invocation`
- `extensions::sdk_compat_spawn_tests::spawn_rejects_malformed_sdk_contract`
- `extensions::sdk_compat_spawn_tests::spawn_rejects_newer_unknown_sdk_contract`

No pristine-base reproduction or root-cause claim is made here. The gate was still
progressing through TUI tests when stopped after those failures. In particular,
`authoritative_decision_cleanup_releases_queue_and_next_submission` passed in that
run. Its temporary slow progress was not an established interaction deadlock.

The script gate completed 16 developer unit tests and 125 release-policy tests.
During `check_optional_domain_isolation.py`, PID 90878 was launched for
`conflicting_openspec_roots_reject_readiness` but had not entered the test harness
after about four minutes. Its sample contains only `_dyld_start`, with no Rust
frames. This is an indeterminate host-launch blocker, not a failed lifecycle
assertion. The initial script invocation had been interrupted by the harness
restart; that attempt also is not counted as a pass.

`crate-gate.log`, `developer-script-gate.log`, `focused-tests-build.log`, and
`clippy.log` are retained under the evidence root. The two sampling reports and
`gate-stop.json` record diagnosis and cleanup. Only groups verified to belong to
these worktree invocations were signalled: 18014, 19523, and detached descendant
89702. Twelve owned processes were captured before termination; the subsequent
process-table check found zero remaining processes in those groups. Other agents'
gates were left running. The landing-gate task remained open until the follow-up
below completed.

### Follow-up: bounded serial reproduction

The original aborted log retains test names but no panic reports. It therefore
cannot establish the exact failed assertion or error for those six historical
failures. Source locations below identify test definitions, not inferred panic
sites. No unrelated-baseline conclusion follows from the size of the inline patch.

The existing compiled test executable was reused without Cargo rebuilds:
`target/debug/deps/omegon-23bbd70b43d2da31`, SHA-256
`9f09db38383908c038f125753225c5d0e5ebe5079f8fa18c22818d5b6aa81074`.
Its on-disk code signature verified successfully. The Nix environment resolved
Rust 1.95.0 and Homebrew Python 3.14.8. The original gate used the same toolchain
and serialized libtest, but a separate developer-script gate was concurrently
rebuilding/relinking the same test executable. Exact historical resource pressure
and fixture errors were not recorded.

| Historical failure | Definition in current/base source | Individual serial reproduction |
| --- | --- | --- |
| `lif_001_lif_003_real_process_quiescent_replacement` | `extensions/conformance_tests.rs:652` | Passed, 2.26 s including runner cleanup |
| `replacement_enforces_tool_shape_and_preserves_stable_invocation` | `extensions/conformance_tests.rs:529` | Passed, 1.16 s |
| `shutdown_settles_fixture_descendants` | `extensions/conformance_tests.rs:949` | Passed, 0.85 s |
| `stale_generation_is_rejected_before_extension_invocation` | `extensions/conformance_tests.rs:567` | Passed, 0.85 s |
| `spawn_rejects_malformed_sdk_contract` | `extensions/mod.rs:2375` | Passed, 0.30 s |
| `spawn_rejects_newer_unknown_sdk_contract` | `extensions/mod.rs:2360` | Passed, 0.30 s |

All paths in the definition column are under `core/crates/omegon/src/`. Each test
ran with `--exact --nocapture --test-threads=1`, `OMEGON_NERD_FONT=1`, and a
90-second diagnostic deadline. No deadline expired. `narrow-repro/manifest.json`
binds artifact hash, environment, test identity, observed descendants, return
codes, and zero remaining owned processes. The previously stalled lifecycle test
also passed on that executable in 0.14 s.

Next, both whole extension modules ran serially in their own processes to include
in-module setup/order effects. Conformance: 12 passed and one pre-existing ignored
test. SDK compatibility: six passed. `module-repro/` retains their output and
cleanup records. No test or production source was changed for these probes.

The OS log query provides a more specific loader observation: at
`2026-10-07 12:30:03.899`, `syspolicyd` recorded execution scan/provenance handling
for the exact old PID 90878 and executable identifier. That is about 6m28s after
its recorded launch. `loader-system-log.txt` retains the matching records. Combined
with its `_dyld_start` sample and successful later launch of the same bytes, this
places the old stall before Rust execution, during macOS launch/provenance handling.
It does not identify why that processing was delayed or prove concurrent relinking
caused it. No provenance attributes, signatures, or OS security settings were changed.

A single sequential gate driver ran the crate gate first, then the script gate.
It retained uncaptured panic output and used `RUST_TEST_THREADS=1`,
`RUST_TEST_NOCAPTURE=1`, and `RUST_BACKTRACE=1` through canonical `nix develop`.
Both commands completed with exit status zero. They were awaited to completion;
neither was stopped or restarted. The prior aborted attempts are not waived or
counted as passing.

### Final gate results and classification

- `crate-serial.log`: 5,366 unit tests passed, zero failed, 11 existing ignored.
  All nine integration targets completed successfully; the existing terminal
  detachment opt-in test remained ignored under the normal recipe. Live upstream
  opt-ins were not enabled. All six named historical failures and the new inline
  replay regression explicitly report `ok` in this complete run.
- `developer-script-serial.log`: 16 developer unit tests, 125 release-policy
  tests, optional-domain validation, and final launcher checks completed. The
  previously delayed `conflicting_openspec_roots_reject_readiness` test reports `ok`.
- `serial-gate-status.txt`: `crate=0`, `scripts=0`.
- `serial-gate-cleanup.json`: owned process group 9553 and its unique Nix temporary
  directory have zero remaining processes. Diagnostic probe manifests likewise
  record zero remaining owned processes.

No fixture, production, harness, dependency, or toolchain changes were required
during this follow-up. The effective validation change was executing the gates
sequentially, so one gate could not relink the executable used by another. This
removes an observed source of interference but does not establish the historical
panic causes. A pristine-baseline execution was not needed to repair a reproduced
defect, because no defect reproduced and both required current-source gates passed.
No claim that the six failures were pre-existing baseline defects is made.

The earlier PTY evidence remains attributable: production and acceptance-driver
sources are unchanged, and the retained runtime binary still hashes to the value
below. `final-identity.json` records an exact comparison of the current Rust diff
with the retained PTY source diff, the matching driver hash, and the final on-disk
test executable hash after the script gate. All scoped tasks are now verified and
the patch is ready for review.
The parent-reported independent read-only review found no blocking code findings
and requested separate commits for fixture network isolation and quiet inline
behavior. The serial gates above satisfy the previously outstanding validation.
The fixture-only staged snapshot also passed its Python acceptance contract tests
before the first commit, with no replay helper or replay-test dependency included.
Commit and PR handoff are now operator-authorized. This evidence describes the
reviewed source and frozen artifact, not a newly built committed binary. OpenSpec
remains in verifying state; archival and installation have not been performed.

### Historical CI baseline integration for PR #249

The pending-CI wording in this historical handoff is superseded by the completed
PR-head results above. The exact combined-main runtime and CI results are now
recorded separately above.

Merged the exact CI baseline `031ef2b78729962370cb1151491800273e10e62d`
into the inline branch without rebasing its two logical commits. The only merge
conflict was the Unreleased changelog; all inline and CI entries were preserved.
The baseline changes CI, contributor guidance, its workflow contract test, and a
flake package export. The locked compiler remains Rust 1.95.0, confirmed by
`nix eval --raw --no-update-lock-file .#packages.x86_64-linux.rust-toolchain.version`.

Git object comparisons against inline head `f6309630` confirm identical `core/`,
`scripts/`, `Cargo.toml`, `Cargo.lock`, and `flake.lock` contents after integration.
The complete Rust source tree object remains
`7e70c899ddf078e3255d5b747d58b2367f1161e2`. The driver and frozen runtime hashes
also still match the retained PTY manifests. `ci-baseline-source-identity.json`
records these comparisons under the evidence root.

The earlier serialized local gates and PTY captures therefore remain evidence for
the same production and fixture contents. They are not presented as a new build
from the baseline-merge commit. Fresh hosted CI must validate the updated PR head
before normal merge. OpenSpec archival and combined-acceptance reconciliation
remain with the parent workflow.

## Historical runtime evidence ledger — 2026-10-07, before merge

Evidence root: the retained private `quiet-inline-vcvNwo` evidence directory.
Artifact names below are relative to that directory; machine-local paths are
omitted from this public record.

Build: `nix develop --command cargo build -p omegon --locked`, completed successfully.
Artifact: this worktree's `target/debug/omegon`, copied byte-for-byte to the evidence
root before broader gates could rebuild the working artifact. Frozen SHA-256:
`5caf296bce616b05f77c127c5bc8f854b0a8aa29648fd2ee098e6eb526d9bc8d`.
Version: `omegon 0.29.0-dev (13760f2-dirty 2026-10-06)`.
Rust implementation and test sources did not change after this build.

| Hypothesis | Changed input | Artifact / capture | Result |
| --- | --- | --- | --- |
| Inline replay cannot take over a held provider turn | Guarded slash dispatch; inline stress fixture sends `/splash` before inspection | `stress/`, frozen hash above | Checkpoint failed: readiness saw notice, but the next capture sampled native insertion before composer redraw. Cleanup passed. |
| Retaining the matched observation removes the capture race | Predicate returns the notice-plus-cancel frame; capture stores that exact frame | `stress-observed/`, same binary | Passed, six local inference requests. Logs exposed pre-existing external discovery with the fixture's dummy key. |
| Canonical resume retains history and restores terminal ownership | Existing `--menu-backdrop` fixture | `resume/`, same binary | Passed, one local inference request. |
| Project token alone can satisfy startup connectivity | Removed generic dummy provider key | `stress-isolated/`, `resume-isolated/`, same binary | Failed: startup reported missing provider-prefix credentials. Both owned groups cleaned up. No inference requests. |
| Local inference works with external HTTP(S) rejected | Restored compatibility key; child HTTP(S) proxy rejects external CONNECT locally | `stress-local/`, same binary | Passed, six local inference requests; blocked OpenAI, Ollama, and GitHub CONNECT attempts. |
| Canonical resume works with the isolated HTTP clients | Same rejecting proxy; existing resume fixture | `resume-local/`, same binary | Passed, one local inference request; history, drafts, thinking selection and terminal restoration verified. |

Final commands (with `TMPDIR` set to the approved private evidence parent):

```sh
python3 scripts/tui_acceptance.py --binary "$EVIDENCE/omegon" --tui inline --ui active --stress --output "$EVIDENCE/stress-local"
python3 scripts/tui_acceptance.py --binary "$EVIDENCE/omegon" --tui inline --ui active --menu-backdrop --output "$EVIDENCE/resume-local"
```

Here `EVIDENCE` is the root above. Each manifest binds the source diff, driver,
binary hash, command, process/start identity, capture hashes, dimensions and times.

The final stress run passed ordinary second submission, exactly-once automatic
publication before explicit export, real denied write with no file mutation,
cancellation while browsing, preserved drafts/Work selection, and post-cancel
recovery. `stress-quiet-replay.txt` shows the notice beside `Responding` and
`Ctrl+C cancel` while the fixture provider is held. `06-permission.txt` shows the
actual write approval. `stress-cancel-draft.txt` shows the terminal cancellation
notice and preserved draft. The retained authority journals contain six prepared
model requests, one tool call/result, five closed turns and one interruption.

The replay checkpoint was retained 44 ms after sending `/splash` text in this run
(one observation, not a performance benchmark). Its SHA-256 is
`59ea3adb09ce28909dbfb3490b8f1ebe0f6b600c5060fa08dec72314eee66102`.
The pre-export primary capture contains exactly one `TUI_FIXTURE_REPLY_1` and one
`TUI_FIXTURE_REPLY_2`. Final driver SHA-256:
`673e893b7b00bbfaafbabc2663bbc4fec75dc33730f8c708e207160d807465f7`.

The final resume run used a separate saved session from the same artifact. Its
journal contains one prepared model request. Explicit fullscreen inspection shows
the canonical prior history; borrowed menus do not replay it into primary output.

Both final runs exited with primary screen and mouse state `0:0`. Their manifests
report successful cleanup, with no forced cleanup and zero GUI windows. A final
process-table check found zero remaining owned groups, including the retired seed
session. Final groups were 63898 (stress) and 66002 (resumed session). All earlier
attempts also had zero remaining owned groups. `proof-summary.json` retains these
checks and journal event counts.

## Remaining scope

The lost-AgentEnd and authoritative-idle cases are deterministic regressions, not
fabricated PTY failures. Blocked native-output fault injection and fullscreen
splash input ownership are outside this inline command fix. No GUI portability
or complete project/work/evidence browser acceptance is claimed.

The deliberately long private temporary path exceeds the Unix socket path limit
for the optional IPC server. Captures exercise the native TUI and canonical journal,
not IPC. Final HTTP clients reject external metadata probes locally; earlier
pre-isolation runs made unauthenticated/dummy-key metadata requests. No run used
real credentials or external model inference.
