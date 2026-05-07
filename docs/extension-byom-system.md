+++
id = "cd7565ff-8c37-45b6-af2c-0146d920ec4b"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Extension BYOM System Design

## Overview

BYOM = **Bring Your Own Mind**

Extensions can declare and provide their own persistent knowledge/memory system. Unlike project memory (per-repo) and episodic memory (per-session), extension minds are:

- **Persistent across repos** — survive switching between projects
- **Persistent across sessions** — survive closing and reopening Omegon
- **Queryable** — integrated into memory_recall() results
- **Optional** — extensions can opt out (not all extensions have minds)

## Motivation

Scribe-rpc (engagement tracking + timeline) will maintain a knowledge base of:
- Project patterns and conventions discovered
- Team communication style and preferences
- Historical decisions and their rationale
- Engagement milestones and progress

This knowledge should:
- Persist across projects (it's about team/engagement, not a single project)
- Be queryable like project memory (augment recall results)
- Be managed by the extension (scribe owns its knowledge model)
- Survive Omegon and scribe-rpc process restarts

## Architecture

### Memory Tier System

Current (three tiers):
1. **Project Memory** — per-repo, in `.git/omegon/memory/facts.jsonl`
2. **Episodic Memory** — per-session, in-memory only
3. **Harness State** — decision logs, design tree

New (addition):
4. **Extension Minds** — per-extension, persistent, cross-repo/cross-session

### Extension Mind Structure

Each extension mind (stored in `~/.omegon/extensions/{name}/mind/`) mirrors omegon-memory:

```
~/.omegon/extensions/scribe-rpc/mind/
├── facts.jsonl                 # Main fact storage
├── episodes.jsonl              # Episode recordings
└── metadata.json               # Mind metadata
```

**facts.jsonl:** Same format as project memory

```json
{
  "id": "ext-scribe-001",
  "section": "Engagement",
  "content": "Team prefers async communication over sync meetings",
  "reinforced": 2,
  "tags": ["communication", "team-culture"],
  "created_at": "2024-03-15T10:00:00Z",
  "last_accessed": "2024-03-31T14:00:00Z"
}
```

**metadata.json:**

```json
{
  "extension": "scribe-rpc",
  "extension_version": "0.2.0",
  "sdk_version": "0.15",
  "mind_version": 1,
  "created_at": "2024-03-15T10:00:00Z",
  "total_facts": 47,
  "total_episodes": 12,
  "last_updated": "2024-03-31T14:00:00Z"
}
```

**episodes.jsonl:**

```json
{
  "id": "ext-scribe-ep-001",
  "title": "Engagement Review: Project Alpha",
  "created_at": "2024-03-31T14:00:00Z",
  "facts": [
    "ext-scribe-001",
    "ext-scribe-002"
  ],
  "context": {
    "project": "alpha",
    "session_id": "sess-123"
  }
}
```

### Memory Query Integration

When `memory_recall(query)` is called:

```rust
pub async fn memory_recall(&self, query: &str) -> Vec<Fact> {
    let mut results = vec![];
    
    // 1. Project memory
    results.extend(self.project_memory.search(query));
    
    // 2. Episodic memory
    results.extend(self.episodic_memory.search(query));
    
    // 3. Extension minds (NEW)
    for (ext_name, mind) in self.active_extension_minds.iter() {
        let ext_results = mind.search(query);
        for mut fact in ext_results {
            fact.source = format!("extension:{}", ext_name);
            results.push(fact);
        }
    }
    
    // Rank by relevance and reinforcement
    results.sort_by_key(|f| (-f.score, -f.reinforced as i32));
    results
}
```

Results are tagged with source:
- `source: "project"` — from project memory
- `source: "episodic"` — from episodic memory
- `source: "extension:scribe-rpc"` — from scribe-rpc mind
- `source: "extension:python-analyzer"` — from python-analyzer mind

## RPC Interface

### Declaring a Mind

Extension declares it has a mind in manifest.toml:

```toml
[extension]
name = "scribe-rpc"
version = "0.2.0"

# NEW: declares this extension has a persistent mind
[mind]
enabled = true
# Optional: human-readable description
description = "Engagement tracking and decision history"
```

### RPC Methods

#### `get_mind()` - Query extension mind

Omegon calls this when:
- Loading extension minds on TUI startup
- User searches/recalls facts
- Building context for agent turns

**Request:**
```json
{
  "jsonrpc": "2.0",
  "id": "1",
  "method": "get_mind",
  "params": {
    "query": "team communication preferences",
    "limit": 10
  }
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "id": "1",
  "result": {
    "facts": [
      {
        "id": "ext-scribe-001",
        "section": "Engagement",
        "content": "Team prefers async communication",
        "tags": ["communication", "team-culture"],
        "confidence": 0.95,
        "reinforced": 2
      }
    ],
    "total": 47,
    "query_match_count": 3
  }
}
```

#### `store_mind()` - Update extension mind

Omegon calls this when:
- Disabling extension (to persist mind to disk)
- Periodically during TUI session (checkpoint)

**Request:**
```json
{
  "jsonrpc": "2.0",
  "id": "2",
  "method": "store_mind",
  "params": {
    "facts": [...],
    "episodes": [...],
    "checkpoint": true
  }
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "id": "2",
  "result": {
    "stored": true,
    "facts_count": 47,
    "checkpoint_path": "~/.omegon/extensions/scribe-rpc/mind/"
  }
}
```

#### `load_mind()` - Load persisted mind

Omegon calls this when enabling extension.

**Request:**
```json
{
  "jsonrpc": "2.0",
  "id": "3",
  "method": "load_mind",
  "params": {}
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "id": "3",
  "result": {
    "loaded": true,
    "facts_count": 47,
    "episodes_count": 12,
    "last_checkpoint": "2024-03-31T14:00:00Z"
  }
}
```

## Lifecycle

### TUI Startup

```
1. For each enabled extension:
   a. Spawn extension process
   b. Health check (ping_method)
   c. IF has mind (manifest.mind.enabled):
      - Call load_mind() RPC
      - Load facts from ~/.omegon/extensions/{name}/mind/
      - Register in active_extension_minds

2. When memory_recall() called:
   a. Search project + episodic + active extension minds
   b. Return results tagged with source
```

### During Session

```
1. Periodically (every 30 minutes or on /memory save):
   - For each active extension mind:
     - Call store_mind() to checkpoint mind state
     - Update metadata.json

2. When user disables extension:
   - Call store_mind() to persist any changes
   - Unload from active_extension_minds
   - Keep filesystem data (mind survives)

3. When user enables extension:
   - Call load_mind()
   - Load facts from disk
   - Register in active_extension_minds
```

### Session End

```
1. For each active extension mind:
   - Call store_mind() with checkpoint=true
   - Persist final state to ~/.omegon/extensions/{name}/mind/

2. Extension processes shut down normally
```

## Manifest Extension

```toml
[extension]
name = "scribe-rpc"
version = "0.2.0"
description = "Engagement tracking"

# NEW: optional mind declaration
[mind]
# Whether this extension has a persistent mind
enabled = true

# Description of the mind for UI/documentation
description = "Engagement tracking and decision history"

# Max facts to keep (optional, default: unlimited)
max_facts = 10000

# Retention policy (optional)
retention_days = 365           # Keep facts for 1 year
```

## Storage & Persistence

Extension minds stored in `~/.omegon/extensions/{name}/mind/`:

```
~/.omegon/extensions/scribe-rpc/
├── target/
├── src/
├── Cargo.toml
├── manifest.toml
└── mind/                       # NEW: mind directory
    ├── facts.jsonl             # Line-delimited facts
    ├── episodes.jsonl          # Line-delimited episodes
    ├── metadata.json           # Mind metadata
    └── .gitignore              # mind/ not tracked in repo
```

Each fact in facts.jsonl includes:
- `id`: Unique identifier (extension-scoped)
- `section`: Knowledge category (Architecture, Patterns, Decisions, etc.)
- `content`: Fact text
- `tags`: For discovery and organization
- `confidence`: 0.0-1.0 score
- `reinforced`: Access count
- `created_at`: Timestamp
- `last_accessed`: Timestamp

## Query Performance

Extension minds are loaded into memory on TUI startup:

```rust
pub struct MemorySystem {
    project: ProjectMemory,
    episodic: EpisodeMemory,
    active_extension_minds: HashMap<String, ExtensionMind>,
}

impl ExtensionMind {
    pub fn search(&self, query: &str) -> Vec<Fact> {
        // BM25 search over loaded facts (fast, in-memory)
        self.facts.search(query)
    }
}
```

If extension mind becomes very large (10k+ facts), it's still manageable:
- Facts are lazy-loaded from JSONL on first search
- Subsequent searches hit in-memory index (fast)
- Only loaded if extension is enabled

## Conflict Resolution

If two extensions declare overlapping knowledge:
- Results are both included, tagged with source
- User/agent can see what knowledge came from where
- Extensions don't interfere with each other

Example:
```
Query: "team communication preferences"

Results:
1. [extension:scribe-rpc] Team prefers async communication (conf: 0.95)
2. [extension:python-analyzer] Python style guide: use type hints (conf: 0.85)
3. [project] Our repo uses async/await patterns (conf: 0.90)
```

## Optional Implementation

Extensions can choose NOT to have a mind:

```toml
[mind]
enabled = false                # Default if section absent
```

If no mind declared, Omegon doesn't call get_mind/store_mind RPC.

## Migration Path

**Phase 1 (current):**
- Mind RPC methods optional
- Extensions without mind work normally
- No change needed to existing extensions

**Phase 2 (0.17):**
- New extensions can declare mind
- Scribe-rpc adds mind capability
- UI shows mind status

**Phase 3 (0.18+):**
- Mind management UI (/memory command integration)
- Export/import minds
- Mind search in TUI
- Mind-aware agent context building

## Example: Scribe-RPC Mind

```json
[facts.jsonl content]
{
  "id": "scribe-001",
  "section": "Team Dynamics",
  "content": "Code review turnaround is typically 24-48 hours on this team",
  "tags": ["process", "communication"],
  "confidence": 0.92,
  "reinforced": 5
}

{
  "id": "scribe-002",
  "section": "Architecture",
  "content": "Team uses event-driven patterns for async operations",
  "tags": ["architecture", "patterns"],
  "confidence": 0.88,
  "reinforced": 3
}

{
  "id": "scribe-003",
  "section": "Decisions",
  "content": "Chose Rust for performance-critical components to avoid C/C++",
  "tags": ["decision", "tech-stack"],
  "confidence": 0.99,
  "reinforced": 1,
  "decision_rationale": "Type safety + performance + package ecosystem"
}
```

When agent recalls `"how does the team approach code reviews?"`:
- Finds scribe fact: "Code review turnaround is 24-48 hours"
- Tagged as `source: "extension:scribe-rpc"`
- Included in context window for next turn
- Agent has engagement knowledge without it being in project memory
