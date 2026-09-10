# Wave 5 operator confirmation

## Scope

Branch: `fix/memory-wave5-operator-confirmation`.
Implementation and tests: `10ca0293`.
Pending lifecycle inferences can be accepted through a snapshot-bound operator
review. This uses the existing TUI/ACP permission response channel and internal
runtime dispatch. No model-supplied approval flag is accepted.

Schema v12 changes the inference/status compatibility rule while retaining the
existing metadata column. Migration preserves legacy unknowns and pending state.
Full applicability and later Wave 5 scheduling/selection work remain open.

## Behavioral tests

`tests/candidate_confirmation.rs` initially failed because the typed confirmation
mutation was unsupported. It now covers:

- Confirmation, same-operation replay, active recall, and historical transport.
- Exact candidate digests and optimistic correction-target versions.
- Receipt-failure rollback and reopen.
- Content/attribution tampering rejection.
- Corrupt confirmed inventory aborting vault publication without removing its note.
- Schema-v11 migration without fabricated operator confirmation.

Host tests cover per-request TUI approval, denial, cancellation, ACP responses,
public approval-flag rejection, hidden commit-tool exposure, and runtime-principal
requirements for internal dispatch. Candidate and target content are bounded for
review; control characters are escaped in the prompt.

The real finalized-bus test rejects a model principal on internal dispatch and
accepts the runtime principal. Permission-wait tests retain the frontend sender
while cancelling or dropping the receiver future, proving that the wait does not
leave a blocking receive task behind.

## Review

Same-executor review; no independent reviewer is claimed. Reviewed model-provided
approval fields, direct public invocation of the commit operation, stale candidate
and correction snapshots, mutable confirmation metadata on import, corrupt rows
disappearing during vault materialization, repeated confirmation, and abandoned
permission receiver tasks. The async bridge now owns its receiver directly, so a
timeout or cancellation releases it even if the frontend retains the sender.

The stored record identifies the approval channel/session/request and reviewed
snapshot. `native_event` denotes the shared native event channel, not a claim that
the responder was specifically the TUI rather than a web client. `acp` denotes the
host proxy. Neither claims physical-human attestation or successful execution.
Cold-store transport retains originating attribution; it does not perform a local
confirmation. Domain callers remain responsible for authorized mutation access.

## Gates

Focused domain and frontend tests passed, including the real internal-dispatch
check and all eight permission-bridge tests. The memory feature matrix passed.
The first affected-crate run found a stale global tool inventory count; it was
updated for the public review and internal commit routes. The affected-crate gate
passed after that correction and permission-receiver cleanup: 5,307 main-crate unit
tests, default integration suites, and memory tests. All five portable memory
campaigns passed.

The final channel-label refinement passed the full memory feature matrix, 20
confirmation-related host tests, all eight permission-bridge tests, and Clippy for
both affected crates/all targets. Default/all-features memory ran 93 unit and 47
integration tests; no-default-features ran 89 unit and 42 integration tests. The
schema generator remains ignored in routine gates. Named OpenSpec validation and
whitespace checks passed.

This operator-confirmation slice is accepted. No blocking finding remains in its
same-executor review. The parent corpus remains implementing; applicability and
other Wave 5 foundations are not marked complete.

Local transcripts under
`~/.local/share/opencode/shell/9153e6c974f56bff7f08cdf3cb1bb0749fa67fbe/`:

- `sh_08cf8a388001iZuFUVEstkzoDs.out`: focused domain, schema, and frontend checks.
- `sh_08cff0c0c001FWVGC1OnOYlk6m.out`: finalized-bus internal-principal boundary.
- `sh_08d05356e001oK1ANslQTyobFX.out`: permission receiver cleanup tests.
- `sh_08d05dae7001nEUzQ8L8EDJnL6.out`: passing affected-crate gate, campaigns, and Clippy.
- `sh_08d0c564c001AtUZSQUMe5hTDp.out`: final feature matrix, channel-label/frontend checks, and Clippy.
