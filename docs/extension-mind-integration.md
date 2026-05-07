+++
id = "ef2ac3c5-8615-490b-9163-c327be73fa9a"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Extension Mind Query Pipeline Integration

## Overview

Extension minds are integrated into Omegon's memory query pipeline. When `memory_recall(query)` is called, results include facts from project memory, episodic memory, AND active extension minds.

## Architecture

### Memory System Layers (Current)

```
Memory System
├── Project Memory (.git/omegon/memory/facts.jsonl)
│   ├── Per-repo
│   ├── Persistent (tracked in git optionally)
│   └── Repo-local knowledge
├── Episodic Memory (in-memory)
│   ├── Per-session
│   ├── Not persistent
│   └── Session events and learnings
└── Harness State (design tree, openspec, etc.)
    ├── Per-repo
    ├── Decision logs
    └── Lifecycle state
```

### Memory System Layers (With Extensions)

```
Memory System
├── Project Memory (.git/omegon/memory/facts.jsonl)
│   ├── Per-repo
│   ├── Persistent
│   └── Repo-local knowledge
├── Episodic Memory (in-memory)
│   ├── Per-session
│   ├── Not persistent
│   └── Session events
├── Extension Minds (NEW)
│   ├── ~/.omegon/extensions/{name}/mind/
│   ├── Cross-repo
│   ├── Persistent
│   └── Extension-managed knowledge
└── Harness State
    ├── Per-repo
    ├── Design tree, openspec
    └── Lifecycle
```

## Query Pipeline

### memory_recall(query: String) → Vec<Fact>

Called by:
- `/memory recall <query>` command
- Agent context injection (before LLM turn)
- Manual fact search

**Implementation:**

```rust
pub async fn memory_recall(&self, query: &str) -> Vec<Fact> {
    let mut results = vec![];
    
    // 1. Project memory search
    if let Ok(project_facts) = self.search_project_memory(query) {
        for fact in project_facts {
            results.push(Fact {
                source: "project".to_string(),
                ..fact
            });
        }
    }
    
    // 2. Episodic memory search
    if let Ok(episodic_facts) = self.search_episodic_memory(query) {
        for fact in episodic_facts {
            results.push(Fact {
                source: "episodic".to_string(),
                ..fact
            });
        }
    }
    
    // 3. Extension minds (NEW)
    for (ext_name, mind) in self.active_extension_minds.iter() {
        if let Ok(ext_facts) = mind.search(query) {
            for fact in ext_facts {
                results.push(Fact {
                    source: format!("extension:{}", ext_name),
                    ..fact
                });
            }
        }
    }
    
    // Rank by BM25 score + reinforcement count
    results.sort_by_key(|f| (-f.bm25_score, -f.reinforced as i32));
    
    // Return top K (default: 20)
    results.into_iter().take(self.recall_limit).collect()
}
```

### Result Tagging

Each fact includes a `source` field:

```rust
pub struct Fact {
    pub id: String,
    pub section: String,
    pub content: String,
    pub tags: Vec<String>,
    pub confidence: f32,
    pub reinforced: u32,
    pub source: String,              // "project", "episodic", "extension:scribe-rpc"
    pub created_at: String,
    pub last_accessed: String,
}
```

### Filtering by Source

User can filter recall by source:

```
/memory recall "team communication" --from project
/memory recall "team communication" --from episodic
/memory recall "team communication" --from extension:scribe-rpc
/memory recall "team communication" --from extension:*
```

## Agent Context Injection

### Current (without extensions)

When preparing agent context, inject relevant facts:

```rust
pub async fn prepare_context(&self) -> ContextBundle {
    let recall = self.memory_recall(&get_query_keywords()).await;
    
    let mut context = String::new();
    context.push_str("## Project Knowledge\n");
    for fact in recall {
        context.push_str(&format!("- {}\n", fact.content));
    }
    
    ContextBundle {
        facts_context: context,
        fact_count: recall.len(),
    }
}
```

### New (with extensions)

Include extension mind facts, tagged with source:

```rust
pub async fn prepare_context(&self) -> ContextBundle {
    let recall = self.memory_recall(&get_query_keywords()).await;
    
    let mut context = String::new();
    
    // Group by source
    let project_facts: Vec<_> = recall.iter()
        .filter(|f| f.source == "project")
        .collect();
    let episodic_facts: Vec<_> = recall.iter()
        .filter(|f| f.source == "episodic")
        .collect();
    let extension_facts: Vec<_> = recall.iter()
        .filter(|f| f.source.starts_with("extension:"))
        .collect();
    
    if !project_facts.is_empty() {
        context.push_str("## Project Knowledge\n");
        for fact in project_facts {
            context.push_str(&format!("- {}\n", fact.content));
        }
    }
    
    if !episodic_facts.is_empty() {
        context.push_str("## Session Insights\n");
        for fact in episodic_facts {
            context.push_str(&format!("- {}\n", fact.content));
        }
    }
    
    if !extension_facts.is_empty() {
        context.push_str("## Extension Knowledge\n");
        for (ext_name, facts) in extension_facts.group_by(|f| &f.source) {
            context.push_str(&format!("### {}\n", ext_name));
            for fact in facts {
                context.push_str(&format!("- {}\n", fact.content));
            }
        }
    }
    
    ContextBundle {
        facts_context: context,
        fact_count: recall.len(),
    }
}
```

**Context injection example:**

```
## Project Knowledge
- Repository uses async/await patterns
- Tests are in separate test/ directory
- CI/CD via GitHub Actions

## Session Insights
- User working on authentication feature
- Reviewed 3 PRs today

## Extension Knowledge
### extension:scribe-rpc
- Team prefers async communication over sync meetings
- Code review turnaround is typically 24-48 hours
- Chose Rust for performance-critical components
```

## Relevance Ranking

Facts are ranked by:
1. **BM25 score** (text relevance to query)
2. **Reinforcement count** (how many times fact was used/verified)
3. **Confidence** (extension-provided confidence score)

```rust
pub struct Fact {
    pub bm25_score: f32,          // Computed during search
    pub reinforced: u32,          // Incrementing counter
    pub confidence: f32,          // Extension-provided (0.0-1.0)
}

impl Fact {
    pub fn combined_score(&self) -> f32 {
        (self.bm25_score * 0.5) +           // 50% text relevance
        (self.reinforced as f32 * 0.3) +    // 30% reinforcement
        (self.confidence * 0.2)             // 20% confidence
    }
}
```

**Example ranking:**

```
Query: "how does the team approach code reviews?"

Results (ranked):
1. [extension:scribe-rpc] "Code review turnaround is 24-48 hours"
   BM25: 0.85, reinforced: 5, confidence: 0.95 → score: 0.89

2. [project] "Code reviews happen async in GitHub"
   BM25: 0.82, reinforced: 2, confidence: 0.90 → score: 0.82

3. [extension:scribe-rpc] "Team prefers detailed feedback over nitpicking"
   BM25: 0.70, reinforced: 3, confidence: 0.85 → score: 0.74

4. [episodic] "User mentioned code review tooling preferences"
   BM25: 0.65, reinforced: 0, confidence: 0.80 → score: 0.59
```

## Memory Telemetry

Track how memory is used:

```rust
pub struct MemoryTelemetry {
    pub total_facts: usize,           // Sum of all sources
    pub facts_by_source: HashMap<String, usize>,
    pub avg_reinforcement: f32,
    pub search_latency_ms: f32,
    pub context_injection_size: usize,
}

// Example output
{
    "total_facts": 127,
    "facts_by_source": {
        "project": 47,
        "episodic": 45,
        "extension:scribe-rpc": 35
    },
    "avg_reinforcement": 2.3,
    "search_latency_ms": 12.4,
    "context_injection_size": 3421
}
```

Available via: `omegon /memory stats`

## Conflict Resolution

If two sources have conflicting facts:

```
Query: "what's the team's preferred testing framework?"

Results:
1. [project] "Team uses pytest for Python"
2. [extension:scribe-rpc] "Team is experimenting with Hypothesis"
3. [episodic] "User mentioned pytest in conversation"
```

Both facts are returned. Agent/user resolves based on context.

## Extension Mind Disabling

When extension is disabled:
- Its mind facts are removed from active search
- Persisted mind data stays on disk
- When re-enabled, facts are loaded again

```rust
pub async fn disable_extension(&mut self, ext_name: &str) {
    if let Some(mind) = self.active_extension_minds.remove(ext_name) {
        // Persist to disk
        mind.store_to_disk().await.ok();
    }
    // Remove from recall results
}

pub async fn enable_extension(&mut self, ext_name: &str) {
    if let Some(mind) = self.load_extension_mind(ext_name).await {
        self.active_extension_minds.insert(ext_name.to_string(), mind);
        // Now included in recall results
    }
}
```

## Performance Considerations

### Index Sizes

Typical indices:
- Project memory: 47 facts → BM25 index ~50 KB
- Episodic memory: 45 facts → BM25 index ~40 KB
- Extension mind: 35 facts → BM25 index ~35 KB
- **Total: ~125 KB** (negligible)

### Search Latency

Expected search latencies:
- Project: 0.5 ms
- Episodic: 0.3 ms
- Single extension: 0.3 ms
- **Total for 3 sources: ~1 ms** (fast)

For large minds (10k facts):
- Index: ~10 MB
- Search: ~5 ms (still acceptable)

### Parallelization

Search across sources can be parallelized:

```rust
let (project, episodic, ext1, ext2) = tokio::join!(
    self.search_project_memory(query),
    self.search_episodic_memory(query),
    self.active_extension_minds["ext1"].search(query),
    self.active_extension_minds["ext2"].search(query),
);
```

## Future: Smart Consolidation

Omegon could eventually:
- Detect duplicate facts across project and extension
- Flag conflicting facts for review
- Suggest fact consolidation
- Merge related facts from multiple sources

Example:
```
[project] "Uses async/await patterns"
[extension:scribe-rpc] "Team uses async patterns for performance"
→ Suggestion: merge into one consolidated fact with both perspectives
```

Not in Phase 1, but architecture supports it.
