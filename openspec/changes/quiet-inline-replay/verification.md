# Verification

## Source and environment

- Branch: `feat/quiet-inline-unification`.
- Base: `13760f256662de5cbb8089d38990f7db3c93d89d`, verified against `origin/main`
  and read-only `git ls-remote origin refs/heads/main`.
- Isolated worktree: `/Users/wilson/workspace/styrene-lab/omegon-inline-unification`.
- Canonical environment: `nix develop`, Rust 1.95.0 / Cargo 1.95.0 from
  `/nix/store/an3nv5smkv3d0kp4wm545hps90fgmn4g-rust-default-1.95.0`.
- Compilation uses this worktree's own `target/`; no shared artifact overwrite.
- Reference checkout was read-only and matched the requested commit and dirty-state boundary.

## Deterministic checks

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

## Runtime evidence ledger

Evidence root:
`/private/var/folders/ln/r6np9wfn1wx8r6sdnvn07sxw0000gn/T/opencode/quiet-inline-vcvNwo`.

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
