+++
id = "f6644dc6-1ed2-4d1a-9aea-8146ac640ff3"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Extension Mind RPC Interface Design

## Overview

The RPC interface that extensions use to declare and manage their persistent knowledge system (mind).

## Core Methods

### `get_mind(query: String, limit: usize) -> MindResponse`

Query the extension's persistent knowledge. Called by:
- TUI startup (to load all facts)
- `memory_recall()` (augment search results)
- Agent context building (inject relevant facts)

**Request:**
```json
{
  "jsonrpc": "2.0",
  "id": "get-mind-1",
  "method": "get_mind",
  "params": {
    "query": "team communication",
    "limit": 10
  }
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "id": "get-mind-1",
  "result": {
    "facts": [
      {
        "id": "scribe-001",
        "section": "Engagement",
        "content": "Team prefers async communication over sync meetings",
        "tags": ["communication", "team-culture"],
        "confidence": 0.95,
        "reinforced": 3,
        "created_at": "2024-03-15T10:00:00Z",
        "last_accessed": "2024-03-31T14:00:00Z"
      }
    ],
    "episodes": [
      {
        "id": "scribe-ep-001",
        "title": "Team Kickoff Discussion",
        "created_at": "2024-03-15T10:00:00Z",
        "facts": ["scribe-001", "scribe-002"]
      }
    ],
    "total_facts": 47,
    "matched": 3
  }
}
```

### `load_mind() -> MindLoadResponse`

Load persisted mind from disk into extension's memory. Called by:
- Omegon when enabling extension
- Extension startup

**Request:**
```json
{
  "jsonrpc": "2.0",
  "id": "load-mind-1",
  "method": "load_mind",
  "params": {}
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "id": "load-mind-1",
  "result": {
    "loaded": true,
    "facts_loaded": 47,
    "episodes_loaded": 12,
    "checkpoint_path": "~/.omegon/extensions/scribe-rpc/mind/",
    "last_checkpoint": "2024-03-31T14:00:00Z"
  }
}
```

### `store_mind(facts: Vec<Fact>, checkpoint: bool) -> MindStoreResponse`

Persist mind to disk. Called by:
- Omegon periodically (every 30 mins)
- When disabling extension
- Session shutdown

**Request:**
```json
{
  "jsonrpc": "2.0",
  "id": "store-mind-1",
  "method": "store_mind",
  "params": {
    "facts": [
      {
        "id": "scribe-001",
        "section": "Engagement",
        "content": "Team prefers async communication",
        "tags": ["communication"],
        "confidence": 0.95,
        "reinforced": 3,
        "created_at": "2024-03-15T10:00:00Z",
        "last_accessed": "2024-03-31T14:00:00Z"
      }
    ],
    "checkpoint": true
  }
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "id": "store-mind-1",
  "result": {
    "stored": true,
    "facts_count": 47,
    "bytes_written": 52428,
    "checkpoint_path": "~/.omegon/extensions/scribe-rpc/mind/",
    "timestamp": "2024-03-31T14:30:00Z"
  }
}
```

### `add_fact(fact: Fact) -> AckResponse`

Add a single fact to the mind. Called by:
- Extension itself (discovery during conversation)
- Omegon (if agent wants to teach extension something)

**Request:**
```json
{
  "jsonrpc": "2.0",
  "id": "add-fact-1",
  "method": "add_fact",
  "params": {
    "section": "Patterns",
    "content": "This team uses trunk-based development",
    "tags": ["git", "workflow", "trunk-based"],
    "confidence": 0.88
  }
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "id": "add-fact-1",
  "result": {
    "id": "scribe-048",
    "stored": true,
    "total_facts": 48
  }
}
```

### `update_fact(id: String, fact: Fact) -> AckResponse`

Update an existing fact (reinforcement, tags, etc).

**Request:**
```json
{
  "jsonrpc": "2.0",
  "id": "update-fact-1",
  "method": "update_fact",
  "params": {
    "id": "scribe-001",
    "section": "Engagement",
    "content": "Team prefers async communication; Slack is primary channel",
    "confidence": 0.96,
    "tags": ["communication", "team-culture", "slack"]
  }
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "id": "update-fact-1",
  "result": {
    "id": "scribe-001",
    "updated": true
  }
}
```

### `reinforce_fact(id: String) -> AckResponse`

Increment reinforcement count (fact was useful/verified).

**Request:**
```json
{
  "jsonrpc": "2.0",
  "id": "reinforce-1",
  "method": "reinforce_fact",
  "params": {
    "id": "scribe-001"
  }
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "id": "reinforce-1",
  "result": {
    "id": "scribe-001",
    "reinforced": 4
  }
}
```

### `delete_fact(id: String) -> AckResponse`

Remove a fact from the mind.

**Request:**
```json
{
  "jsonrpc": "2.0",
  "id": "delete-fact-1",
  "method": "delete_fact",
  "params": {
    "id": "scribe-001"
  }
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "id": "delete-fact-1",
  "result": {
    "deleted": true,
    "total_facts": 46
  }
}
```

## Type Definitions

### Fact

```json
{
  "id": "ext-{extension}-{number}",
  "section": "Architecture|Decisions|Patterns|Specs|Constraints|Known Issues",
  "content": "Human-readable fact text",
  "tags": ["tag1", "tag2"],
  "confidence": 0.95,
  "reinforced": 3,
  "created_at": "2024-03-15T10:00:00Z",
  "last_accessed": "2024-03-31T14:00:00Z"
}
```

### Episode

```json
{
  "id": "ext-{extension}-ep-{number}",
  "title": "Human-readable episode title",
  "description": "Optional longer description",
  "created_at": "2024-03-15T10:00:00Z",
  "facts": ["scribe-001", "scribe-002"],
  "context": {
    "project": "optional-project-name",
    "session_id": "optional-session-id",
    "branch": "optional-git-branch"
  }
}
```

## Lifecycle Hooks

### Before Disable

```json
{
  "jsonrpc": "2.0",
  "method": "set_enabled_state",
  "params": {
    "enabled": false,
    "reason": "user disabled"
  }
}
```

Extension should prepare for shutdown (persist state).

### Before Reload

```json
{
  "jsonrpc": "2.0",
  "method": "set_enabled_state",
  "params": {
    "enabled": false,
    "reason": "reload"
  }
}
```

## Error Handling

If mind operations fail, extensions return typed errors:

```json
{
  "jsonrpc": "2.0",
  "id": "get-mind-1",
  "error": {
    "code": "InternalError",
    "message": "Failed to load mind: database corruption detected"
  }
}
```

Omegon handles errors gracefully:
- `get_mind()` error → log warning, continue without extension mind
- `load_mind()` error → disable extension, log error
- `store_mind()` error → log warning, continue (next checkpoint retry)

## Optional Implementation

Extensions can implement a minimal mind:

```rust
async fn handle_rpc(&self, method: &str, params: Value) -> Result<Value> {
    match method {
        "get_mind" => {
            // Simple: just return empty mind
            Ok(json!({"facts": [], "total_facts": 0, "matched": 0}))
        }
        "load_mind" => {
            Ok(json!({"loaded": true, "facts_loaded": 0}))
        }
        "store_mind" => {
            // Extensions without real mind: no-op
            Ok(json!({"stored": true, "facts_count": 0}))
        }
        _ => Err(Error::method_not_found(method)),
    }
}
```

Or implement full mind with persistence:

```rust
struct ScribeExtension {
    mind: Arc<Mutex<ScribeMind>>,
}

impl ScribeExtension {
    async fn handle_rpc(&self, method: &str, params: Value) -> Result<Value> {
        match method {
            "get_mind" => {
                let mind = self.mind.lock().await;
                let facts = mind.search(&params["query"].as_str().unwrap_or(""));
                Ok(json!({"facts": facts, "total_facts": mind.len()}))
            }
            "store_mind" => {
                let mind = self.mind.lock().await;
                mind.persist_to_disk().await?;
                Ok(json!({"stored": true}))
            }
            // ... other methods
        }
    }
}
```

## Versioning

Mind format versioning stored in `metadata.json`:

```json
{
  "extension": "scribe-rpc",
  "extension_version": "0.2.0",
  "sdk_version": "0.15",
  "mind_version": 1,
  "created_at": "2024-03-15T10:00:00Z"
}
```

If `mind_version` changes, migration logic needed:
- `mind_version=1` vs `mind_version=2` → migrate on load
- Extensions can define migration functions in SDK
