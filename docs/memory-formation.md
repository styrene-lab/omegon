# Evidence-backed memory formation

The Rust memory feature forms session episodes from validated semantic replay.
It retains bounded source references and excerpts, then asks an independently
configured extractor for classified candidates. These candidates are pending
inferences, not active facts.

## Configure extraction

Add these keys to the project's `.omegon/profile.json`:

```json
{
  "memoryExtractionEnabled": true,
  "memoryExtractionModel": "anthropic:claude-haiku-4-5-20251001"
}
```

The model above is the current Rust default. Requests use existing host completion
routing. Configuration does not establish provider or credential availability.
Embedding discovery does not enable or disable extraction.

The formation envelope records the configured model spec. Actual serving-route
provenance remains in the host's route lease.

Set `memoryExtractionEnabled` to `false` to disable inference while retaining
evidence capture and ordinary memory tools. Child sessions keep automatic extraction
disabled. Configuration takes effect when a new memory feature is constructed.

Blank model specs, control characters, and specs exceeding 512 UTF-8 bytes disable
only the extractor with a diagnostic that does not include the invalid value.
Source evidence capture remains available.

## Evidence and outcomes

The optional episode `formation` envelope records source session/stream/frontier
identity, attributed excerpts, pending candidates, and extraction state (disabled,
pending, complete, or unavailable).

Assistant reports do not count as independent tool verification. Thinking content
and restricted continuity are excluded. Missing or replaced semantic sources are
reported as unavailable; uncommitted SessionEnd prompt/outcome strings are not used
as fallback evidence. Mixed legacy sessions expose only their supported suffix.

Capture retains at most 64 items, 1 KiB per excerpt, and 32 KiB of excerpt text.
It preserves the first retained user goal and a recent suffix, with explicit
truncation indicators. Original content remains in the canonical session log.
Extraction accepts at most 32 candidates and rejects malformed candidates,
unsupported references, and model-supplied authority fields. Domain constants are
authoritative for these limits.

The memory provider collector enforces a 64 KiB text budget while receiving deltas.
It requires a terminal `Done` event; valid JSON followed by EOF is insufficient.
An unreadable or oversized assistant chunk ends the excerpt rather than joining
text across the missing content.

## Durability and inspection

During a session, every eight `TurnEnd` notifications make an evidence snapshot due.
The host captures from validated committed replay through the session-view binding;
turn telemetry itself is not evidence. One capture worker can run per feature.
Further turns while it is busy coalesce into a due flag instead of another task.
Finished workers are reaped on subsequent turn notifications. A failed capture is
retried on a later turn; unavailable sources create no checkpoint episode.

Interval capture only persists evidence. The recovery scheduler performs extraction.
With extraction disabled, snapshots remain available with disabled extraction state.
The interval resets at SessionStart. SessionEnd retains its finalization behavior.
Identical interval/final snapshots share the existing capture-policy v2 identity
and replay their receipt. Different snapshots may overlap and retain the existing
evidence-item and byte bounds, including explicit truncation. A source frontier
identifies the observed replay boundary; it does not claim every prior event was
retained in the bounded excerpt. The canonical session log remains the source owner.

Before pressure, overflow, feature-requested, or manual compaction, the host awaits
an optional feature checkpoint hook. Aggressive decay also invokes it before
tightening the context window. Memory can persist a snapshot immediately, without
waiting for the eight-turn interval or running inference. The host gives published
features a shared ten-second wait budget and propagates cancellation.

Memory reports `persisted` only after the storage acknowledgment, `not_applicable`
when there is no substantive evidence, or `unavailable` with a bounded reason.
An occupied interval-worker slot reports `capture_busy` rather than allocating more
work. Missing sources, storage failure, and timeout do not veto optional-memory
compaction or claim successful capture. Canonical session records remain retained.
Timed-out capture stays owned until it settles; synchronous replay cannot be preempted.

Source evidence is committed before inference. A separate atomic
`CompleteFormation` mutation updates candidates and extraction status without
replacing evidence. Episode search, stale-vector invalidation, and the operation
receipt update in the same transaction.

Cancellation during inference leaves a durable pending record. On session startup,
the hosted memory feature starts one background recovery worker. Each pass resumes
up to eight pending episodes for its mind and exact configured extraction model,
oldest first. Each pass has a two-minute budget and the existing per-extraction
timeout. It uses stored evidence excerpts, so the original session need not be reopened.

Recovery follows the existing extraction enable/disable and child-session policy.
Changing the configured model leaves checkpoints for the previous model pending.
Disabled, complete, and unavailable outcomes are not automatically retried by this
worker. After a successful pass, observed backlog schedules another pass after one
second. An empty queue is checked again after 60 seconds, including work committed
after startup. Pass failures or timeouts use exponential backoff from 60 seconds
to a 15-minute maximum. Success resets backoff. Pending work survives restart;
the scheduler's timers and failure streak are process-local and restart immediately.
Incremental coverage and broader queue-admission backpressure remain planned.

Managed shutdown cancels and joins the recovery task. Cancellation before completion
leaves pending evidence intact. A completed extraction and its receipt commit
atomically; recovery does not admit candidates as facts or reinforce existing facts.
Concurrent owners can both perform inference, but only one pending-to-terminal
transition can commit. A completion conflict stops that recovery pass and preserves
the winning result and remaining pending work. Provider execution is not exactly-once.
Discarding a feature also signals cancellation; managed shutdown remains the path
that joins its owned threads.

Hosted `memory_query` includes `details.formation_recovery` with enablement, mind,
and scheduler state. The state reports phase, completed pass attempts, consecutive
pass failures, scheduled delay, a pending-queue sample, and a bounded failure code.
The sample covers only the configured model/mind at the last completed pass.
When `pending_sample_capped` is true, the count is a lower bound. It is unknown while
a pass runs or fails. The delay is the scheduled interval, not a live countdown.
These observations do not establish full component readiness or successful candidate
admission. Normal session-end formation can run alongside recovery; atomic completion
resolves races.

Hosted `memory_query` also exposes `details.evidence_checkpoint`: the event interval,
capped turns-since-request counter, whether capture is due, and whether its worker
is running. These are admission observations, not durable-completion acknowledgments.
Its `last_pre_eviction` field reports the last memory-hook outcome. A persisted
outcome acknowledges a bounded snapshot, not complete historical coverage. A null
value means no completed observation is available for that attempt; it must not be
interpreted as successful capture. This field is not a ledger of every compaction.
The capture worker participates in managed cancellation/join and feature-drop
cancellation. Capture requests use the managed storage boundary and cancel outstanding
requests when dropped. Synchronous replay validation is not a preemptible operation.

Capture-policy v2 binds operation identities to the mind, retained source evidence,
and configured extractor. Retry-time counters and wall-clock dates are excluded
from the bound payload. Episode dates come from retained evidence when available;
unknown dates are assigned by storage on the first write. Advisory statistics
remain runtime diagnostics rather than evidence metadata.

Formation envelopes remain wire version 1; the database schema evolves independently
and is currently v13.
Existing capture-policy v1 records and receipts are retained. The first v2 capture
of an earlier source can create a new policy-versioned episode; repeats within v2
are replay-safe.

Read narratives with `memory_episodes`. The complete typed envelope is preserved
by JSONL export/import and in SQLite's `episodes.formation` column. Pending-to-complete
transport updates verify unchanged source evidence. Corrupt formation metadata
produces an error rather than silently disappearing.

## Schema compatibility

Schema v9 introduced the nullable formation column; schema v10 retains it and adds
vector identity metadata. Schema v11 adds pending lifecycle-inference attribution.
Schema v12 permits operator-confirmed inferences to become active while retaining
their original attribution. Schema v13 adds recorded applicability.
Initialized project stores on schemas v5–v12 migrate
through the existing backup/verification workflow before startup opens
them. Legacy episodes retain absent evidence as unknown. Separately managed stores
must use the explicit migration workflow before opening with a v13 backend; older
binaries cannot open a v13 store. Confirmation was a semantic migration: older v11 readers
reject active facts that carry inference metadata.

## Inferred lifecycle summaries

`memory_ingest_lifecycle` with `authority: "inferred"` retains a pending candidate.
The response includes its ID and `status: "pending"`. Candidate metadata preserves
declared artifact references and proposed supersession without applying a correction.
These references are attribution, not verified execution evidence.

Pending candidates are retained in JSONL and can be inspected through the backend's
pending-status inventory. They do not enter recall, pinned context, embedding indexes,
or vault fact publication. Retrying the same operation replays its receipt.
Changing JSONL status cannot confirm a retained inference.

## Operator confirmation

Call `memory_confirm` with `candidate_id` to request review. The tool accepts no
approval flag. The runtime sends the candidate content, declared attribution, mind,
version, and any proposed correction to the TUI or ACP permission channel.

An affirmative per-request response permits an internal runtime invocation. The
internal commit tool is not exposed on the public tool surface. Denial, cancellation
while waiting, timeout, or an unavailable operator surface does not admit the candidate.
The review wait is bounded to 120 seconds. This uses the client's existing identity
and permission-response boundary; it is not physical-human attestation.

Before committing, storage checks the candidate's version and exact snapshot hash.
A proposed supersession also requires the reviewed target version and the same mind.
Confirmation and correction commit atomically with their operation receipt. A stale
snapshot or target rejects the operation rather than confirming changed content.

Confirmation retains the original inference attribution and records the review
version, snapshot/content hashes, session, request ID, approval channel, and recorded time.
The `surface` field identifies `native_event` or `acp`; the shared native channel
does not identify a specific TUI or web responder.
It initializes the active confidence prior and reinforcement count as an explicit
admission action. It does not fabricate successful execution evidence. Confirmed
facts use normal active retrieval and vault publication; optional vector repair
remains available through embedding backfill.

JSONL transports confirmation attribution, including historical confirmed records.
It cannot promote an existing pending candidate or rewrite a retained confirmation.
Cold-store imports carry the originating record's attribution rather than asserting
that the local operator performed a new review.

## Provenance inspection

Inspection also reports declared applicability, its assessment, and the evaluation
context. See [memory applicability](memory-applicability.md) for rule and query semantics.

Use `memory_inspect` with `fact_id` to read a fact's recorded provenance and source
availability. It also accepts IDs of pending, archived, dormant, and superseded
records in the current mind. Historical records are returned as stored, without
substituting a current replacement.

The shared inspection projection reports lifecycle status, version, stored
timestamps, confidence, reinforcement count, and attribution. Content previews are
limited to 2,048 characters and source-label previews to 512 characters. Truncation
is explicit, and `content_sha256` always identifies the full stored content.

The provenance `basis` distinguishes explicit artifact attribution, unconfirmed
inferences, operator-confirmed inferences, invalid metadata, and `legacy_unknown`.
The last category includes any record with only an unstructured source label,
regardless of its creation date. A free-form label cannot establish operator
approval or successful execution.

Hosted inspection checks supported repository artifact references with the existing
bounded reader. `evidence_availability` reports:

| Value | Meaning |
|---|---|
| `no_reference` | No usable structured reference was recorded |
| `not_checked` | A reference exists but this adapter has not checked it |
| `snapshot_matches` | Current bytes match the recorded artifact hash |
| `snapshot_changed` | Current bytes differ from that hash |
| `readable_unverified` | A declared reference is readable but has no validated artifact hash |
| `unavailable` | The configured reader cannot currently read the source |
| `unsupported_reference` | The reference type or path is outside the supported lifecycle scope |

The standalone `MemoryProvider` reports recorded metadata without a filesystem
binding. Inspection does not establish execution success or current applicability.
Source availability never changes the fact's status, confidence, or reinforcement.

## Explicit lifecycle conclusions

`authority: "explicit"` requires a matching structured artifact. The authority
string alone is insufficient. Supported references are:

| Source kind / reference type | Path | Section / subreference |
|---|---|---|
| `design-tree` / `design` | `docs/design/*.md` or a nested design path | `Decisions` / decision title |
| `design-tree` / `design` | Same design scope | `Constraints` / `Implementation Notes/Constraints` |
| `openspec` / `spec` | `openspec/baseline/**/*.md` or `openspec/archive/YYYY-MM-DD-name/specs/**/*.md` | `Specs` / requirement title |

Design nodes must be decided, implementing, or implemented and parse without
diagnostics. Decision status must be `decided` and include a rationale. Specification
conclusions require an explicit `### Requirement:` heading. Proposal paths and open
questions are excluded.

For decisions, supply `content` as `Title: rationale`. For specifications, use
`Title: requirement description`. For constraints, supply the constraint itself.
Matching normalizes whitespace but preserves case. For example, a decision titled
`Use transactions` with rationale `Keep corrections atomic.` uses:

```json
{
  "source_kind": "design-tree",
  "authority": "explicit",
  "section": "Decisions",
  "content": "Use transactions: Keep corrections atomic.",
  "artifact_ref_type": "design",
  "artifact_ref_path": "docs/design/zircon.md",
  "artifact_ref_sub": "Use transactions"
}
```

Artifact reads are limited to 1 MiB and traverse repository-relative directories
without following symlinks. The stored source identifies the exact snapshot parsed.
It does not assert that the file remains unchanged or that a test execution occurred.

To correct an active fact, include `supersedes` and `supersedes_version`. Recall
displays the fact's version. A version mismatch or cross-mind target rejects the
whole correction. The response includes the replacement ID, version, and source
attribution. Retrying against the same source snapshot replays the operation;
missing or changed artifacts produce an error rather than a new write.

Explicit attribution uses a versioned JSON envelope in `Fact.source`, decoded by
`Fact::lifecycle_conclusion()`. It survives database reopen and JSONL transport
without an additional schema change. Without a correction target,
exact content/source duplicates reuse existing memory. Different evidence remains separately attributed for later
reconciliation.

Inspect the project migration state with:

```bash
omegon memory migrate --status --path ai/memory/facts.db
```

Use `.omegon/memory/facts.db` instead when that is the selected legacy root.
Follow the existing recovery workflow rather than overwriting newer writes with
an old backup.

Owners under `core/crates/`: `omegon-memory/src/formation.rs`,
`omegon/src/features/memory/formation.rs`, and `omegon/src/features/memory.rs`.
