+++
id = "f4aee88c-d2ec-44aa-9fbd-c2a20c94dabc"
kind = "document"
title = "Omegon maintenance executable contract"
status = "decided"
tags = ["architecture", "maintenance", "recovery", "kernel", "cli"]
aliases = ["omegon-maintain"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
dependencies = ["selective-kernel-decomposition"]
open_questions = []
related = ["harness-architecture-parity"]
+++

# Omegon maintenance executable contract

## Status

This is the approved Slice-zero contract and durable architecture summary.
The independently runnable executable, recovery workflows, offline release
verification, and release-distribution integration are implemented in the
workspace. Normative behavior is owned by the OpenSpec maintenance
requirements. Remaining implementation is tracked by
`openspec/archive/2026-08-26-selective-kernel-decomposition/tasks.md`.

Package `omegon-maintenance-contracts` implements canonical v1 records/results,
domain-separated key and digest derivation, descriptor-relative Unix advisory
locks, crash reconciliation, and shared valid plus corruption fixtures. Package
`omegon-maintain` builds independently with diagnostics, contribution and session
quarantine, stale ownership pruning, durable audit workflows, and offline release
verification. Normal Omegon now enforces session-deny authority across startup,
headless, live interactive, and ACP resume paths, and normal contribution startup
enforces maintenance deny/exclusion authority. Runtime ownership writers and
release packaging/launch paths are integrated with public install/recovery guidance.

## Purpose

`omegon-maintain` is the independent recovery companion for Omegon. It must
remain runnable when normal Omegon startup, project configuration, contributions,
or optional runtime services are broken.

It is deliberately not a second agent harness and not a miniature package
manager. Slice zero provides data-only diagnostics plus bounded
denial/quarantine and proven-stale record pruning. Broader mutation remains
separately gated.

## Artifact topology

```text
package path:      core/crates/omegon-maintain
Cargo package:     omegon-maintain
executable:        omegon-maintain
shared contracts:  core/crates/omegon-maintenance-contracts
```

The package:

- shares the workspace version with `omegon`;
- does not depend on package `omegon`;
- shares only versioned deny, session-deny, ownership-record, exclusion-lock,
  transaction, audit, package-manifest, and canonical-key schemas through
  package `omegon-maintenance-contracts`;
- has its own dependency and binary-size budgets;
- carries its own version, commit, target, and artifact identity;
- refreshes commit and tracked dirty-state identity with the normal executable;
- is packaged beside `omegon` and signed where applicable in every supported
  release package;
- is independently launchable from source, linked development, direct install,
  Homebrew, Nix/OCI where supported, and release archives.

Every release archive contains `omegon.composition-lock.json` and
`omegon-maintain.composition-lock.json`. The signed package manifest binds each
lock and executable digest. Each resident entry records its identity, artifact
path and digest, protocol range, supported target, required or optional state,
and fallback. The lock also records the expected Sigstore workflow identity.
`release verify` validates the package-manifest Sigstore bundle before archive
composition, then validates every required lock as one fail-closed set. It does
not extract or execute optional contributions. Its evidence reports the verified
signing identity and result.

The maintenance lock contains no optional normal-runtime residents. Normal
optional domains use `typed_unavailable` fallback; absence is inventory, not a
claim that a domain was loaded.

## Slice 7 migration

Operators do not need to rename configuration fields or commands.
`disabled_tools`, persona manifests, child-runtime JSON, and existing
maintenance protocol v1 output remain compatibility contracts; normal startup
translates those names into capability IDs before admission. Existing archives
without resident locks are not upgraded in place and are not valid new release
packages. The only compatibility exception is the immutable signed legacy
verifier fixture identified by its exact tag, archive digest, record ID, and
two-member inventory in the release verifier. Any mutation to that tuple fails
closed. Reinstall the complete companion pair from one release. The canonical
`verify.release_verify` snippet is unchanged because the package manifest and
Sigstore bundle operands now carry the additional lock evidence.

The first crate should use only minimal CLI, serialization, filesystem,
cryptographic verification, locking, and deadline primitives. It does
not link the TUI, provider stack, memory, lifecycle, MCP, extension runtime, or
normal agent loop. Shared functionality must move into narrow owning crates or
maintenance-specific contracts rather than making maintenance depend on the
integration binary.

## Excluded startup inputs

Maintenance startup does not evaluate or initialize:

- normal TUI state;
- default loop or provider clients;
- project profiles or dynamic project configuration;
- project contribution code, prompts, hooks, or templates;
- MCP or extension subprocesses;
- mutable skills, prompts, personas, workflows, or catalog packs;
- memory or lifecycle stores;
- orchestration, Workbench, delegate, or cleave state;
- package build hooks or shell startup files.

This boundary is enforced in three ways: the maintenance Cargo graph rejects
normal-runtime, TUI, MCP, extension, memory, lifecycle, and orchestration
packages; black-box startup tests plant malformed or executable normal-runtime
inputs and require unchanged files with no sentinel execution; and release
companion validation requires the exact compiled maintenance profile and
exclusion set.

## Command tree

```text
omegon-maintain
  identity
  doctor

  home
    inspect
    recover

  composition
    inspect

  contribution
    list [--scope <user|project>] [--cursor <cursor>]
    inspect <selector> --scope <user|project>
    disable <selector> --scope <user|project>
    quarantine <selector> --scope <user|project>

  session
    list [--cursor <cursor>]
    inspect <session-id> --workspace <absolute-path>
    quarantine <session-id> --workspace <absolute-path>

  resource
    list --workspace <absolute-path> [--cursor <cursor>]
    prune-stale --workspace <absolute-path>

  release
    inspect
    verify --archive <path> --manifest <path> --bundle <path>

  audit
    inspect [--cursor <cursor>]
    verify [--cursor <cursor>]
```

No ambiguous aliases such as `fix`, `repair`, `clean`, or `update` are part of
Slice zero.

## Global options

```text
--json
--deadline <duration>
--home <absolute-path>
--config-home <absolute-path>
--workspace <absolute-path>
--dry-run
--request-id <uuid>
```

Rules:

- project contribution, selected-session, and resource commands require explicit
  `--workspace`;
- mutation targets are never inferred from Git, a profile, or contribution data;
- every mutation requires explicit `--deadline`;
- local read-only commands default to 30 seconds;
- offline release verification defaults to 5 minutes;
- accepted deadlines cannot exceed 10 minutes;
- one monotonic absolute deadline covers admission, locks, reads, verification,
  writes, fsync, audit settlement, and cleanup;
- progress never extends the deadline;
- `--dry-run` performs validation and locking but not final mutation;
- omitted request IDs are generated and returned.

Durations are unsigned integers followed by `ms`, `s`, or `m`; zero and overflow
are rejected. Root overrides are explicit operator authority grants. Each root
must be an existing absolute directory owned by the effective user, must not be
`/`, alias another granted root, end in a symlink, or change identity during the
command. Mutations reject group/other-writable roots and filesystems lacking the
required no-follow and atomic primitives.

## Structured output

On normal termination with writable stdout, `--json` makes stdout contain
exactly one JSON object:

```json
{
  "schema_version": 1,
  "command": "contribution.disable",
  "status": "success",
  "request_id": "uuid",
  "artifact": {
    "version": "0.x.y",
    "commit": "sha",
    "target": "aarch64-apple-darwin",
    "digest": "sha256:..."
  },
  "composition": {
    "profile": "maintenance",
    "generation": "sha256:...",
    "excluded_inputs": [
      "default_loop",
      "extension_runtime",
      "lifecycle",
      "mcp",
      "memory",
      "mutable_packs",
      "orchestration",
      "project_config",
      "project_contributions",
      "provider_clients",
      "tui"
    ]
  },
  "deadline": {
    "requested_ms": 10000,
    "elapsed_ms": 42,
    "expired": false
  },
  "diagnostics": [],
  "mutations": [],
  "errors": []
}
```

`status` is `success`, `failure`, or `degraded`.

Exit status:

- `0`: every requested operation completed and settled successfully;
- `1`: definite failure, refusal, invalid arguments, unsupported operation, or
  pre-dispatch timeout;
- `2`: completed degraded/partial diagnostic or mutation, unverifiable evidence,
  deadline after possible dispatch, unknown settlement, audit-settlement failure,
  or output failure after mutation.

After `--json` is recognized, argument/admission failures use the envelope and
logs use stderr only. Mutation entries expose `planned`, `prepared`, `dispatched`,
`applied`, `settled`, or `unknown` plus retry safety. A quarantine that settles
the deny record but cannot detach the entry is degraded with exit 2.

`--dry-run` may bootstrap/acquire maintenance-owned OS lock files and append a
dry-run audit record. It does not create transaction fences, deny/session-deny
records, quarantine entries, or ownership-record changes. Catchable cancellation before
dispatch fails cleanly; after dispatch, maintenance settles or records unknown
before honoring it. `SIGKILL`, abort, unwritable stdout, and uninterruptible
kernel I/O are reconciled from transaction state rather than covered by an
impossible output guarantee.

Paths in output are scope-labelled and redacted where home disclosure is not
needed. Errors have stable code, phase, retry-safety, and bounded message fields.
Untrusted bytes never become unbounded error text. List cursors bind the command,
admitted roots, effective scope or workspace, and last emitted diagnostic key;
changed or forged boundaries fail closed. Audit cursors are anchored through
validated successor segments to the current checkpoint.

## Offline release verification

Verify a downloaded release without discovery, network access, extraction, or
execution:

```sh
omegon-maintain --json release verify \
  --archive /absolute/path/omegon-<version>-<target>.tar.gz \
  --manifest /absolute/path/omegon-<version>-<target>.tar.gz.manifest.json \
  --bundle /absolute/path/omegon-<version>-<target>.tar.gz.manifest.sigstore.json
```

After authentication, an installer can additionally bind an already extracted
tree to the same signed inventory:

```sh
omegon-maintain --json release verify \
  --archive /absolute/path/omegon-<version>-<target>.tar.gz \
  --manifest /absolute/path/omegon-<version>-<target>.tar.gz.manifest.json \
  --bundle /absolute/path/omegon-<version>-<target>.tar.gz.manifest.sigstore.json \
  --extracted-root /absolute/path/to/staged-root
```

Verification accepts only the compiled `styrene-lab/omegon` release workflow
identity and issuer, Sigstore bundle v0.3 message signatures, one Rekor
`hashedrekord` v0.0.1 entry, and complete SET, inclusion-proof, and signed
checkpoint evidence. It evaluates the Fulcio certificate and trust material at
the authenticated Rekor integrated time, rejects timestamp-authority evidence,
and streams a bounded regular-file-only archive without extracting it. The
archive must exactly match the signed manifest and contain both root executables
with mode `0755`. With `--extracted-root`, the root must contain the exact
manifest inventory with matching regular-file digests, sizes, and modes; extra,
missing, linked, or special entries are rejected. All operand paths and the
optional extracted root must be absolute.

## Read authority

Maintenance may read only:

- its executable and immutable adjacent release metadata;
- explicit `--home`, defaulting to the resolved Omegon home;
- explicit `--config-home`, defaulting to the resolved Omegon config home;
- compiled allowlisted contribution parents below those roots;
- explicit canonical `--workspace`, limited to known `.omegon/` contribution
  and runtime paths;
- selected session snapshot/metadata entries under the config session root;
- explicit archive, signed package-manifest, and Sigstore-bundle operands to
  `release verify`.

It does not read arbitrary project source, `.git`, build files, hooks, `ai/`,
`docs/`, `openspec/`, memory, lifecycle, secrets, prompt/skill bodies, shell
startup files, or manifest-selected arbitrary paths.

## Mutation authority

Slice zero may mutate only:

```text
<home>/maintain/v1/
<home>/maintain/v1/audit/
<home>/maintain/v1/locks/
<home>/maintain/v1/deny/
<home>/maintain/v1/session-deny/
<allowlisted-contribution-parent>/.omegon-maintain-quarantine/
<workspace>/.omegon/runtime/  # proven stale ownership records only
```

A selected contribution directory entry may be atomically renamed into its
same-filesystem quarantine. A contribution-entry symlink may be unlinked, but
its target is never opened or modified. Contents are never edited or recursively
deleted.

Forbidden mutation roots include project source/configuration, `.git`, `ai/`,
`docs/`, `openspec/`, memory/lifecycle/secrets stores, session snapshot bytes,
package-manager-owned installation roots, symlink targets, and arbitrary paths
from contribution metadata.

## Path and write safety

- Trusted roots are opened once with no-follow semantics.
- Path components are traversed descriptor-relative with no-follow operations.
- Absolute children, parent traversal, separators, NUL, noncanonical IDs, and
  platform prefixes are rejected.
- Ancestor and target file identity are revalidated immediately before mutation.
- Symlinks and unexpected file types are rejected unless the command explicitly
  operates on the link entry itself.
- Writes use unique create-exclusive same-directory temporary files.
- Restrictive permissions are set before writing.
- Temporary files and parent directories are fsynced around atomic rename.
- No operation falls back to remove-then-create or copy-and-delete.
- Quarantine destinations are request-ID-derived, nonexisting, and installed
  with atomic no-replace semantics; source/destination identities are verified.
- Lock acquisition consumes the command deadline and never performs an
  unbounded final wait.
- Settlement-write failure fences later mutations and exits degraded rather
  than reporting success.

Every mutation first fsyncs a `Prepared` transaction with exact root/target
identities and a per-domain fence, then fsyncs `Dispatched` immediately before
the external mutation, followed by `Settled` or `Unknown`. Same-fingerprint
request-ID reuse reconciles deterministically; conflicting reuse is refused.
Audit records are sequence-numbered and hash-chained, but `audit verify` claims
structural continuity only, not authenticity against an external attacker.
Rotated segments contain at most 100,000 records. A bounded authenticated
frontier and request-keyed receipts let bootstrap, same-request replay, and
append recovery avoid rescanning aggregate history.

The deadline begins before root admission and is cooperative, not a hard
real-time promise. It is checked before each lock and potentially blocking
operation; no mutation dispatches without remaining budget, and an overrun
after dispatch is degraded/unknown. Slice-zero commands spawn no child process.

Version 1 limits include 1 MiB per inert/session metadata file, 4 KiB link text,
10,000 inert/session/resource entries per command, 4 MiB output, a 2 GiB archive, 4 GiB aggregate
uncompressed members, 100,000 members, 1 GiB per member, and 100,000 audit
records per verification. One-over-limit input fails closed or degrades an
aggregate diagnostic without emitting unbounded bytes.

## Command semantics

### Identity and composition

`identity` reports only maintenance artifact identity and compiled contract.
`composition inspect` reports the deterministic compiled maintenance profile and
explicit exclusions; it does not claim to represent the future live contribution
graph. `doctor` aggregates bounded read-only checks while retaining partial
findings.

### Installation home recovery

An installation stores the identity of its Omegon home directory. A mismatch
blocks guarded contribution loading. The core interface can still start and
reports affected scopes through `/status`.

Inspect the stored and observed identities:

```sh
omegon-maintain home inspect --json
```

Inspection does not modify authority records. It reports whether the directory
matches, whether recovery is pending, and whether recovery is eligible.

For a legacy device-number change, recovery requires the same canonical path and
inode. A conflicting stored volume identity prevents recovery. Recovery also
refuses active admission locks, unresolved transactions, and maintenance fences.
Close sessions that use the affected installation before applying recovery.

Recovery reserves descriptors for its bounded lock inventory by temporarily
raising the companion's soft file limit within the inherited hard limit. It
retains every lock and restores the limit afterward. If the hard limit cannot
support recovery, `home_recovery_descriptor_limit` reports a refusal before
recovery records change.

Preview the recovery without changing records:

```sh
omegon-maintain home recover --dry-run --deadline 30s --json
```

Apply recovery with a request UUID. Retain the result and request ID:

```sh
omegon-maintain home recover --deadline 30s --request-id <request-id> --json
```

If recovery is interrupted, repeat the command with the same request ID. Recovery
preserves installation identity, contribution restrictions, session restrictions,
and audit history. It records one recovery audit event. It does not delete
maintenance state or enable denied contributions.

On macOS filesystems that expose a volume UUID, recovery records volume and
directory continuity for subsequent device renumbering. Unsupported filesystems
retain strict identity checks and explicit recovery.

After recovery, restart the harness or reload the affected contributions. A
successful scan clears its loading failure from `/status`. Recovery does not
refresh provider credentials; authentication failures remain separate.

### Contributions

List and inspect parse bounded inert metadata only. They do not expand
environment variables, resolve commands, execute probes, follow entry symlinks,
fetch network data, or load prompts/skills.

Disable writes an idempotent maintenance-owned deny record that every normal
startup path consults before parsing contribution-controlled content. Malformed
deny state fails closed, and cached composition cannot bypass a newer deny
generation. It does not claim to stop an already-running process.

Normal startup holds a shared per-scope exclusion lock from before deny lookup
through parsing/activation. Quarantine holds the exclusive lock from before deny
preparation through detach settlement, uses a secure same-filesystem quarantine
and atomic no-replace rename, and reports unknown rather than overstating an
unbound identity race. Permanent purge, restore, and enable are deferred.

### Sessions

List and inspect validate existing snapshot/metadata framing, IDs, filenames,
stored normalized workspace identity, schema/version, types, sizes, and digests.
Selection requires explicit workspace and exactly one matching immutable pair;
it does not open project contents. Current session JSON is labelled an
LLM-facing snapshot, not semantic event truth.

Quarantine writes a resume-deny record while preserving every original byte.
Interactive, daemon, ACP, and stale-cache resume paths consult it before session
deserialization and fail closed on malformed state. Semantic recovery,
truncation, event synthesis, metadata rebuild, and completion classification are
outside Slice zero.

### Resources

Resource operations require explicit workspace and trust only Slice-zero
versioned Omegon ownership records carrying boot and process-start identity. PID
liveness and filenames are not ownership; legacy/malformed records are
inspect-only. Pruning removes records only when heartbeat expiry and dead process
identity are both proven. Recorded cross-boundary capability or history is
`best_effort` or `unverifiable`; Slice zero does not perform process cleanup.

### Releases

Release verification is offline. A Sigstore bundle supplies the certificate
chain, Rekor inclusion proof, and signed checkpoint verified against compiled
roots and versioned identity policy. Its signed package manifest binds archive,
target/version/tag/commit, build provenance, and both executable digests.
Verification streams but neither extracts nor executes members and performs no
discovery, download, installation, activation, update, switch, or rollback.

## Deferred operations

Slice zero does not provide:

- generic read, search, patch, or shell;
- project code/config edits or validators;
- Git, Cargo, hooks, package scripts, or dynamic probes;
- contribution enable, restore, purge, install, update, or build;
- process killing outside the current maintenance invocation;
- semantic session repair, event synthesis, snapshot rewrite, completion change,
  or invocation retry;
- network release discovery/download, install, activation, update, switch, or
  rollback;
- package-manager-owned mutation;
- trust based on project keys, endpoints, channels, or verification policy.

These operations remain outside Slice zero and are not implied by later
authority slices. Slices 1/5, 3, and 7 establish relevant prerequisites only;
any session-repair, generic mutation, update, switch, or rollback command needs
separate requirements, safety analysis, and implementation tasks.

## Packaging and documentation

Every supported package containing `omegon` also contains and exposes the
companion. Platform archives contain both
executables at their root:

```text
omegon
omegon-maintain
```

Both carry matching release identity, are signed where applicable, and are
independently launch-tested through source, linked development, direct install,
platform archive, Homebrew, Nix, and OCI paths supported by the repository. Each
path tests missing/incompatible-companion failure.

For linked development, `omegon-maintain --version` includes the same embedded
commit reported by `identity`. The shared launcher compares that commit with the
selected checkout, so a current pair reports `stale: no` for both executables.
The deterministic Slice-7 campaign is defined by
`fixtures/release-closeout-evidence-v1.json` and
`scripts/check_release_closeout.py`. Its release-signing claim is limited to the
checked-in signed fixture and verifier tests.

Public operator guidance is owned by the installation and Maintenance & Recovery
site pages. Their generated command examples come from canonical install,
maintenance, and verification snippets and are validated against the packaged
companion behavior before this lane exits.
