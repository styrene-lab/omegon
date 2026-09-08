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

Source evidence is committed before inference. A separate atomic
`CompleteFormation` mutation updates candidates and extraction status without
replacing evidence. Episode search, stale-vector invalidation, and the operation
receipt update in the same transaction.

Cancellation during inference leaves a durable pending record. Automatic restart
scheduling remains later-wave work. Repeated completed operations replay their
recorded outcome rather than duplicating episodes or reinforcing facts.

Capture-policy v2 binds operation identities to the mind, retained source evidence,
and configured extractor. Retry-time counters and wall-clock dates are excluded
from the bound payload. Episode dates come from retained evidence when available;
unknown dates are assigned by storage on the first write. Advisory statistics
remain runtime diagnostics rather than evidence metadata.

The database remains schema v9 and formation envelopes remain wire version 1.
Existing capture-policy v1 records and receipts are retained. The first v2 capture
of an earlier source can create a new policy-versioned episode; repeats within v2
are replay-safe.

Read narratives with `memory_episodes`. The complete typed envelope is preserved
by JSONL export/import and in SQLite's `episodes.formation` column. Pending-to-complete
transport updates verify unchanged source evidence. Corrupt formation metadata
produces an error rather than silently disappearing.

## Schema compatibility

Schema v9 adds the nullable formation column. Initialized project stores on schemas
v5–v8 migrate through the existing backup/verification workflow before startup opens
them. Legacy episodes retain absent evidence as unknown. Separately managed stores
must use the explicit migration workflow before opening with a v9 backend; older
binaries cannot open a v9 store.

Inspect the project migration state with:

```bash
omegon memory migrate --status --path ai/memory/facts.db
```

Use `.omegon/memory/facts.db` instead when that is the selected legacy root.
Follow the existing recovery workflow rather than overwriting newer writes with
an old backup.

Owners under `core/crates/`: `omegon-memory/src/formation.rs`,
`omegon/src/features/memory/formation.rs`, and `omegon/src/features/memory.rs`.
