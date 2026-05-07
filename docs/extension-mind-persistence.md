+++
id = "beaec53b-2864-45eb-bfaa-daf17977760c"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Extension Mind Persistence Design

## Overview

Extension minds persist to `~/.omegon/extensions/{name}/mind/` in JSONL format (matching omegon-memory structure). Minds survive process restarts and repo switching.

## Directory Structure

```
~/.omegon/extensions/scribe-rpc/
├── Cargo.toml
├── Cargo.lock
├── src/
│   └── main.rs
├── target/
│   └── release/
│       └── scribe-rpc
├── manifest.toml
└── mind/                           # NEW: mind directory
    ├── facts.jsonl                 # Main knowledge base
    ├── episodes.jsonl              # Episode recordings
    ├── metadata.json               # Mind metadata
    └── .gitignore                  # mind/ not tracked in git
```

The `mind/` directory is **not tracked in git** (similar to project memory `.git/omegon/`).

## File Formats

### facts.jsonl

Line-delimited JSON. One fact per line, no array wrapper.

```jsonl
{"id":"scribe-001","section":"Engagement","content":"Team prefers async communication","tags":["communication","team-culture"],"confidence":0.95,"reinforced":3,"created_at":"2024-03-15T10:00:00Z","last_accessed":"2024-03-31T14:00:00Z"}
{"id":"scribe-002","section":"Patterns","content":"Trunk-based development workflow","tags":["git","workflow"],"confidence":0.88,"reinforced":2,"created_at":"2024-03-15T11:00:00Z","last_accessed":"2024-03-31T14:00:00Z"}
```

**Rationale:**
- Append-only efficient (adds are O(1))
- Streaming-friendly (read line-by-line)
- Mergeable in git (if tracked, which we don't)
- Same format as omegon-memory `facts.jsonl`

### episodes.jsonl

Line-delimited JSON. Episode recordings (groupings of facts by context).

```jsonl
{"id":"scribe-ep-001","title":"Team Kickoff Discussion","created_at":"2024-03-15T10:00:00Z","facts":["scribe-001","scribe-002"],"context":{"project":"alpha"}}
{"id":"scribe-ep-002","title":"Architecture Review","created_at":"2024-03-16T14:00:00Z","facts":["scribe-003","scribe-004","scribe-005"],"context":{"branch":"feature/async"}}
```

### metadata.json

Singleton file with mind metadata.

```json
{
  "extension": "scribe-rpc",
  "extension_version": "0.2.0",
  "sdk_version": "0.15",
  "mind_version": 1,
  "created_at": "2024-03-15T10:00:00Z",
  "last_updated": "2024-03-31T14:30:00Z",
  "total_facts": 47,
  "total_episodes": 12,
  "bytes_on_disk": 52428,
  "last_checkpoint": "2024-03-31T14:30:00Z"
}
```

## Lifecycle

### TUI Startup

```
1. Extension spawned
2. Health check passes
3. Omegon calls load_mind() RPC
4. Extension reads from ~/.omegon/extensions/{name}/mind/:
   - Opens facts.jsonl
   - Loads facts into in-memory index (BM25)
   - Opens episodes.jsonl
   - Loads episode metadata
   - Loads metadata.json (validate mind_version)
5. Extension ready for queries
```

### During Session

**Every 30 minutes (configurable):**
```
1. Omegon calls store_mind(facts, checkpoint=true)
2. Extension serializes current facts
3. Writes to ~/.omegon/extensions/{name}/mind/facts.jsonl
4. Updates metadata.json (last_updated, total_facts)
5. Continues operation
```

**On manual `/memory save` command:**
```
1. Omegon calls store_mind(facts, checkpoint=true)
2. Same as periodic checkpoint
```

**When extension is disabled:**
```
1. Omegon calls store_mind(facts, checkpoint=true)
2. Extension performs final write
3. mind/ directory persists on disk
4. Extension process shuts down
```

### Extension Re-enable

```
1. User clicks [enable] on disabled extension
2. Omegon spawns extension process
3. Health check passes
4. Omegon calls load_mind() RPC
5. Extension loads facts from ~/.omegon/extensions/{name}/mind/
6. Extension mind restored exactly as it was before disable
```

### Session Shutdown (Omegon Exit)

```
1. For each active extension with mind:
   - Call store_mind(facts, checkpoint=true)
   - Extension writes final state

2. Send SIGTERM to extension processes
3. Wait graceful shutdown timeout
4. Send SIGKILL if needed

5. All minds persisted to ~/.omegon/extensions/{name}/mind/
```

### Repository Switch

```
User: omegon /project/alpha
  [work in alpha, interact with scribe-rpc]
  Scribe learns about alpha's patterns

User: omegon /project/beta
  [switch projects]
  [scribe-rpc extension still loaded]
  [scribe still has alpha knowledge]
  [adds beta knowledge on top]
  [mind grows cross-repo]

User: omegon /project/alpha
  [return to alpha]
  [scribe still has both alpha and beta knowledge]
  [but search can be project-filtered if desired]
```

## Size Management

### Automatic Truncation

If mind becomes too large, apply retention policy from manifest:

```toml
[mind]
max_facts = 10000           # Keep only latest 10k facts
retention_days = 365        # Delete facts older than 1 year
```

**On startup or periodically:**
```
1. Load metadata.json
2. If total_facts > max_facts:
   - Sort facts by last_accessed
   - Delete oldest facts until under limit
3. If any fact > retention_days old:
   - Delete it
4. Rewrite facts.jsonl
5. Update metadata.json
```

### Size Monitoring

In metadata.json, track bytes on disk:

```json
{
  "total_facts": 47,
  "bytes_on_disk": 52428
}
```

If `bytes_on_disk > 100MB` (configurable):
- Warn extension during load
- Suggest running compaction
- Continue anyway

## Corruption Recovery

If facts.jsonl is corrupted (partial write, etc):

```
1. Omegon calls load_mind()
2. Extension attempts to read facts.jsonl
3. If JSON parse error on a line:
   - Log warning: "Corrupted fact at line X"
   - Skip the line
   - Continue reading rest of file
4. Load recovers what it can
5. On next store_mind(), corrupted fact is gone
```

Alternative: backup on write

```
1. Omegon calls store_mind()
2. Before overwriting facts.jsonl:
   - Move facts.jsonl → facts.jsonl.bak
3. Write to new facts.jsonl
4. On success: delete facts.jsonl.bak
5. On error: restore from facts.jsonl.bak
```

## Disk Usage Example

Typical extension mind:
- 1000 facts: ~500 KB (fact = ~500 bytes)
- 100 episodes: ~50 KB
- metadata.json: ~2 KB
- **Total: ~550 KB per extension**

For 10 extensions:
- **5.5 MB total** (negligible)

Even with 10k facts per extension:
- 10 extensions × 5 MB = **50 MB** (acceptable)

## Migration

If mind format changes (mind_version bump):

```json
{
  "mind_version": 1,  // Current
  "created_at": "2024-03-15T10:00:00Z"
}
```

To migrate to mind_version 2:

```rust
if metadata.mind_version < 2 {
    migrate_v1_to_v2(&facts).await?;
}
```

SDK provides migration helpers:
```rust
pub mod mind_migration {
    pub fn v1_to_v2(facts: Vec<Fact>) -> Vec<Fact> {
        // Transform facts to new schema
    }
}
```

## Thread Safety

Extension mind accessed from multiple threads:
1. RPC handler (get_mind, add_fact, etc.)
2. Periodic checkpoint task
3. Load/store operations

Use Arc<RwLock<>>:

```rust
struct ExtensionMind {
    facts: Arc<RwLock<Vec<Fact>>>,
    episodes: Arc<RwLock<Vec<Episode>>>,
}

async fn get_mind(&self, query: &str) -> Result<Vec<Fact>> {
    let facts = self.facts.read().await;
    Ok(facts.search(query))
}

async fn store_mind(&self) -> Result<()> {
    let facts = self.facts.read().await;
    let episodes = self.episodes.read().await;
    persist_to_disk(&facts, &episodes).await?;
    Ok(())
}
```

## Optional: Cloud Sync

Future (not designed yet):
- Extension minds can opt into cloud sync
- `~/.omegon/extensions/scribe-rpc/mind/` synced to cloud storage
- Sync across multiple machines
- Same mind, different omegon instances

Configuration:
```toml
[mind]
enabled = true
sync = "disabled"  # or "s3", "gcs", "dropbox", "icloud"
sync_location = "s3://my-bucket/scribe-rpc-mind"
```

For now: local persistence only.
