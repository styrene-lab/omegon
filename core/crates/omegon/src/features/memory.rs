//! MemoryFeature — integrated memory system.
//!
//! This feature wraps MemoryBackend to provide all 12 memory_* agent-callable tools
//! and context injection. It holds an Arc<dyn MemoryBackend> received from setup.rs
//! and implements the Feature trait directly.
//!
//! Tools provided:
//! - memory_query (render full memory as markdown)
//! - memory_recall (semantic search by query string, return top-k)
//! - memory_store (add fact to section)
//! - memory_focus (pin fact IDs to working memory)
//! - memory_release (clear working memory)
//! - memory_episodes (search episode narratives)
//! - memory_compact (trigger compaction — delegate to existing auto_compact)
//! - memory_supersede (replace fact by ID)
//! - memory_archive (archive facts by ID)
//! - memory_connect (create edge between facts)
//! - memory_search_archive (search archived facts)
//! - memory_ingest_lifecycle (internal tool for lifecycle candidate ingestion)

use async_trait::async_trait;
use omegon_traits::*;
use serde_json::Value;
use std::sync::{Arc, Mutex};

use omegon_memory::{
    MemoryBackend, ContextRenderer, MarkdownRenderer,
    StoreFact, FactFilter, CreateEdge, Section, DecayProfileName, StoreAction,
};

/// Memory feature that provides all memory_* tools and context injection.
pub struct MemoryFeature {
    /// Memory backend for storage operations
    backend: Arc<dyn MemoryBackend>,
    /// Renderer for context injection
    renderer: MarkdownRenderer,
    /// Mind identifier (usually "default")
    mind: String,
    /// Pinned fact IDs for working memory
    working_memory: Mutex<Vec<String>>,
}

impl MemoryFeature {
    /// Create a new memory feature with the given backend and mind.
    pub fn new(backend: Arc<dyn MemoryBackend>, mind: String) -> Self {
        Self {
            backend,
            renderer: MarkdownRenderer,
            mind,
            working_memory: Mutex::new(Vec::new()),
        }
    }

    /// Get the backend for direct access (used by other features).
    pub fn backend(&self) -> &Arc<dyn MemoryBackend> {
        &self.backend
    }

    /// Get the current mind identifier.
    pub fn mind(&self) -> &str {
        &self.mind
    }
}

#[async_trait]
impl Feature for MemoryFeature {
    fn name(&self) -> &str {
        "memory"
    }

    fn tools(&self) -> Vec<ToolDefinition> {
        vec![
            ToolDefinition {
                name: crate::tool_registry::memory::MEMORY_STORE.into(),
                label: "memory_store".into(),
                description: "Store a fact in project memory. Facts persist across sessions.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "required": ["section", "content"],
                    "properties": {
                        "section": {
                            "type": "string",
                            "enum": ["Architecture", "Decisions", "Constraints", "Known Issues", "Patterns & Conventions", "Specs"],
                            "description": "Memory section"
                        },
                        "content": {
                            "type": "string",
                            "description": "Fact content (single bullet point, self-contained)"
                        }
                    }
                }),
            },
            ToolDefinition {
                name: crate::tool_registry::memory::MEMORY_RECALL.into(),
                label: "memory_recall".into(),
                description: "Search project memory for facts relevant to a query. Returns ranked results.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "required": ["query"],
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "Natural language query"
                        },
                        "k": {
                            "type": "number",
                            "description": "Number of results (default: 10)"
                        },
                        "section": {
                            "type": "string",
                            "description": "Optional section filter"
                        }
                    }
                }),
            },
            ToolDefinition {
                name: crate::tool_registry::memory::MEMORY_QUERY.into(),
                label: "memory_query".into(),
                description: "Read all active facts from project memory.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {}
                }),
            },
            ToolDefinition {
                name: crate::tool_registry::memory::MEMORY_ARCHIVE.into(),
                label: "memory_archive".into(),
                description: "Archive one or more facts by ID.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "required": ["fact_ids"],
                    "properties": {
                        "fact_ids": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "Fact IDs to archive"
                        }
                    }
                }),
            },
            ToolDefinition {
                name: crate::tool_registry::memory::MEMORY_SUPERSEDE.into(),
                label: "memory_supersede".into(),
                description: "Replace an existing fact with an updated version.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "required": ["fact_id", "section", "content"],
                    "properties": {
                        "fact_id": { "type": "string" },
                        "section": { "type": "string" },
                        "content": { "type": "string" }
                    }
                }),
            },
            ToolDefinition {
                name: crate::tool_registry::memory::MEMORY_CONNECT.into(),
                label: "memory_connect".into(),
                description: "Create a relationship between two facts.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "required": ["source_fact_id", "target_fact_id", "relation", "description"],
                    "properties": {
                        "source_fact_id": { "type": "string" },
                        "target_fact_id": { "type": "string" },
                        "relation": { "type": "string" },
                        "description": { "type": "string" }
                    }
                }),
            },
            ToolDefinition {
                name: crate::tool_registry::memory::MEMORY_FOCUS.into(),
                label: "memory_focus".into(),
                description: "Pin facts into working memory so they persist across the session.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "required": ["fact_ids"],
                    "properties": {
                        "fact_ids": {
                            "type": "array",
                            "items": { "type": "string" }
                        }
                    }
                }),
            },
            ToolDefinition {
                name: crate::tool_registry::memory::MEMORY_RELEASE.into(),
                label: "memory_release".into(),
                description: "Clear working memory — release all pinned facts.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {}
                }),
            },
            ToolDefinition {
                name: crate::tool_registry::memory::MEMORY_EPISODES.into(),
                label: "memory_episodes".into(),
                description: "Search session episode narratives for past work context.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "required": ["query"],
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "What you're looking for in past sessions"
                        },
                        "k": {
                            "type": "number",
                            "description": "Number of results (default: 5)"
                        }
                    }
                }),
            },
            ToolDefinition {
                name: crate::tool_registry::memory::MEMORY_COMPACT.into(),
                label: "memory_compact".into(),
                description: "Trigger context compaction to free up context window space.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "instructions": {
                            "type": "string",
                            "description": "Optional focus instructions for compaction"
                        }
                    }
                }),
            },
            ToolDefinition {
                name: crate::tool_registry::memory::MEMORY_SEARCH_ARCHIVE.into(),
                label: "memory_search_archive".into(),
                description: "Search archived project memories from previous months.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "required": ["query"],
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "Search terms"
                        }
                    }
                }),
            },
            ToolDefinition {
                name: crate::tool_registry::memory::MEMORY_INGEST_LIFECYCLE.into(),
                label: "memory_ingest_lifecycle".into(),
                description: "Internal tool for lifecycle candidate ingestion. Used by design-tree, openspec, and cleave extensions.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "required": ["source_kind", "authority", "section", "content"],
                    "properties": {
                        "source_kind": { "type": "string" },
                        "authority": { "type": "string", "enum": ["explicit", "inferred"] },
                        "section": { "type": "string" },
                        "content": { "type": "string" },
                        "supersedes": { "type": "string" },
                        "artifact_ref_type": { "type": "string" },
                        "artifact_ref_path": { "type": "string" },
                        "artifact_ref_sub": { "type": "string" }
                    }
                }),
            },
        ]
    }

    async fn execute(
        &self,
        tool_name: &str,
        _call_id: &str,
        args: Value,
        _cancel: tokio_util::sync::CancellationToken,
    ) -> anyhow::Result<ToolResult> {
        match tool_name {
            crate::tool_registry::memory::MEMORY_STORE => {
                let content = args["content"].as_str().unwrap_or("").to_string();
                let section_str = args["section"].as_str().unwrap_or("Architecture");
                let section: Section = serde_json::from_value(Value::String(section_str.into()))
                    .unwrap_or(Section::Architecture);

                let result = self.backend.store_fact(StoreFact {
                    mind: self.mind.clone(),
                    content: content.clone(),
                    section,
                    decay_profile: DecayProfileName::Standard,
                    source: Some("manual".into()),
                }).await.map_err(|e| anyhow::anyhow!("{e}"))?;

                let msg = match result.action {
                    StoreAction::Stored => format!("Stored in {}: {}", section_str, content),
                    StoreAction::Reinforced => format!("Reinforced existing fact: {}", content),
                    StoreAction::Deduplicated => "Duplicate — fact already exists".to_string(),
                };
                Ok(ToolResult {
                    content: vec![ContentBlock::Text { text: msg }],
                    details: serde_json::json!({ "id": result.fact.id, "action": format!("{:?}", result.action) }),
                })
            }
            crate::tool_registry::memory::MEMORY_RECALL => {
                let query = args["query"].as_str().unwrap_or("").to_string();
                let k = args["k"].as_u64().unwrap_or(10) as usize;

                // Use FTS search (vector search requires embeddings which may not be available)
                let results = self.backend.fts_search(&self.mind, &query, k)
                    .await.map_err(|e| anyhow::anyhow!("{e}"))?;

                if results.is_empty() {
                    return Ok(ToolResult {
                        content: vec![ContentBlock::Text { text: "No matching facts found.".into() }],
                        details: Value::Null,
                    });
                }

                let mut lines = Vec::new();
                for (i, sf) in results.iter().enumerate() {
                    let section = serde_json::to_string(&sf.fact.section).unwrap_or_default();
                    let section = section.trim_matches('"');
                    // Truncate very long facts in recall results
                    let content = if sf.fact.content.len() > 200 {
                        format!("{}…", &sf.fact.content[..197])
                    } else {
                        sf.fact.content.clone()
                    };
                    lines.push(format!(
                        "{}. [{}] ({}, {:.0}%) {}",
                        i + 1, sf.fact.id, section, sf.similarity * 100.0, content,
                    ));
                }
                Ok(ToolResult {
                    content: vec![ContentBlock::Text { text: lines.join("\n") }],
                    details: serde_json::json!({ "count": results.len() }),
                })
            }
            crate::tool_registry::memory::MEMORY_QUERY => {
                let facts = self.backend.list_facts(&self.mind, FactFilter::default())
                    .await.map_err(|e| anyhow::anyhow!("{e}"))?;

                if facts.is_empty() {
                    return Ok(ToolResult {
                        content: vec![ContentBlock::Text { text: "No facts in memory.".into() }],
                        details: serde_json::json!({ "count": 0 }),
                    });
                }

                // Group by section, show counts + sample facts (capped to avoid overwhelming the model)
                let mut sections: std::collections::BTreeMap<String, Vec<&omegon_memory::Fact>> = std::collections::BTreeMap::new();
                for fact in &facts {
                    let section = serde_json::to_string(&fact.section).unwrap_or_default();
                    let section = section.trim_matches('"').to_string();
                    sections.entry(section).or_default().push(fact);
                }

                let mut lines = Vec::new();
                lines.push(format!("{} facts across {} sections:\n", facts.len(), sections.len()));

                let max_per_section = 8;
                for (section, section_facts) in &sections {
                    lines.push(format!("## {} ({} facts)", section, section_facts.len()));
                    for fact in section_facts.iter().take(max_per_section) {
                        // Truncate long facts to keep output manageable
                        let content = if fact.content.len() > 120 {
                            format!("{}…", &fact.content[..117])
                        } else {
                            fact.content.clone()
                        };
                        lines.push(format!("  [{}] {}", fact.id, content));
                    }
                    if section_facts.len() > max_per_section {
                        lines.push(format!("  … +{} more (use memory_recall for targeted search)",
                            section_facts.len() - max_per_section));
                    }
                    lines.push(String::new());
                }

                Ok(ToolResult {
                    content: vec![ContentBlock::Text { text: lines.join("\n") }],
                    details: serde_json::json!({ "count": facts.len(), "sections": sections.len() }),
                })
            }
            crate::tool_registry::memory::MEMORY_ARCHIVE => {
                let ids: Vec<String> = args["fact_ids"].as_array()
                    .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                    .unwrap_or_default();
                let id_refs: Vec<&str> = ids.iter().map(|s| s.as_str()).collect();
                let count = self.backend.archive_facts(&id_refs)
                    .await.map_err(|e| anyhow::anyhow!("{e}"))?;
                Ok(ToolResult {
                    content: vec![ContentBlock::Text { text: format!("Archived {count} fact(s).") }],
                    details: serde_json::json!({ "archived": count }),
                })
            }
            crate::tool_registry::memory::MEMORY_SUPERSEDE => {
                let fact_id = args["fact_id"].as_str().unwrap_or("").to_string();
                let content = args["content"].as_str().unwrap_or("").to_string();
                let section_str = args["section"].as_str().unwrap_or("Architecture");
                let section: Section = serde_json::from_value(Value::String(section_str.into()))
                    .unwrap_or(Section::Architecture);

                let new_fact = self.backend.supersede_fact(&fact_id, StoreFact {
                    mind: self.mind.clone(),
                    content,
                    section,
                    decay_profile: DecayProfileName::Standard,
                    source: Some("manual".into()),
                }).await.map_err(|e| anyhow::anyhow!("{e}"))?;
                Ok(ToolResult {
                    content: vec![ContentBlock::Text {
                        text: format!("Superseded {} → new fact {}", fact_id, new_fact.id)
                    }],
                    details: serde_json::json!({ "old_id": fact_id, "new_id": new_fact.id }),
                })
            }
            crate::tool_registry::memory::MEMORY_CONNECT => {
                let edge = self.backend.create_edge(CreateEdge {
                    source_id: args["source_fact_id"].as_str().unwrap_or("").into(),
                    target_id: args["target_fact_id"].as_str().unwrap_or("").into(),
                    relation: args["relation"].as_str().unwrap_or("").into(),
                    description: args["description"].as_str().map(String::from),
                }).await.map_err(|e| anyhow::anyhow!("{e}"))?;
                Ok(ToolResult {
                    content: vec![ContentBlock::Text {
                        text: format!("Connected {} → {} ({})", edge.source_id, edge.target_id, edge.relation)
                    }],
                    details: serde_json::json!({ "edge_id": edge.id }),
                })
            }
            crate::tool_registry::memory::MEMORY_FOCUS => {
                let ids: Vec<String> = args["fact_ids"].as_array()
                    .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                    .unwrap_or_default();
                let count = ids.len();
                self.working_memory.lock().unwrap().extend(ids);
                Ok(ToolResult {
                    content: vec![ContentBlock::Text { text: format!("Pinned {count} fact(s) to working memory.") }],
                    details: Value::Null,
                })
            }
            crate::tool_registry::memory::MEMORY_RELEASE => {
                self.working_memory.lock().unwrap().clear();
                Ok(ToolResult {
                    content: vec![ContentBlock::Text { text: "Working memory cleared.".into() }],
                    details: Value::Null,
                })
            }
            crate::tool_registry::memory::MEMORY_EPISODES => {
                let query = args["query"].as_str().unwrap_or("").to_string();
                let k = args["k"].as_u64().unwrap_or(5) as usize;
                let episodes = self.backend.search_episodes(&self.mind, &query, k).await
                    .map_err(|e| anyhow::anyhow!("{e}"))?;
                if episodes.is_empty() {
                    return Ok(ToolResult {
                        content: vec![ContentBlock::Text { text: "No matching episodes found.".into() }],
                        details: Value::Null,
                    });
                }
                let mut lines = Vec::new();
                for ep in &episodes {
                    lines.push(format!("### {}: {}", ep.date, ep.title));
                    lines.push(ep.narrative.chars().take(500).collect::<String>());
                    lines.push(String::new());
                }
                Ok(ToolResult {
                    content: vec![ContentBlock::Text { text: lines.join("\n") }],
                    details: Value::Null,
                })
            }
            crate::tool_registry::memory::MEMORY_COMPACT => {
                // Context compaction is handled at the conversation level, not memory level.
                // Signal the caller that compaction was requested.
                Ok(ToolResult {
                    content: vec![ContentBlock::Text {
                        text: "Context compaction requested. The agent loop will compact older conversation history.".into()
                    }],
                    details: serde_json::json!({ "action": "compact_requested" }),
                })
            }
            crate::tool_registry::memory::MEMORY_SEARCH_ARCHIVE => {
                let query = args["query"].as_str().unwrap_or("").to_string();
                // Search archived facts using FTS - for now this searches all facts,
                // we'd need to update the backend to filter for archived specifically
                let results = self.backend.fts_search(&self.mind, &query, 20).await
                    .map_err(|e| anyhow::anyhow!("{e}"))?;
                if results.is_empty() {
                    return Ok(ToolResult {
                        content: vec![ContentBlock::Text { text: "No matching archived facts found.".into() }],
                        details: Value::Null,
                    });
                }
                let mut lines = Vec::new();
                for scored in &results {
                    let f = &scored.fact;
                    lines.push(format!("[{}] ({:?}) {}", f.id, f.section, f.content));
                }
                Ok(ToolResult {
                    content: vec![ContentBlock::Text { text: lines.join("\n") }],
                    details: Value::Null,
                })
            }
            crate::tool_registry::memory::MEMORY_INGEST_LIFECYCLE => {
                // Lifecycle fact ingestion — stores with source metadata
                let content = args["content"].as_str().unwrap_or("").to_string();
                let section_str = args["section"].as_str().unwrap_or("Architecture");
                let section: Section = serde_json::from_value(Value::String(section_str.into()))
                    .unwrap_or(Section::Architecture);
                let authority = args["authority"].as_str().unwrap_or("inferred");
                let source_kind = args["source_kind"].as_str().unwrap_or("unknown");

                let result = self.backend.store_fact(StoreFact {
                    mind: self.mind.clone(),
                    content: content.clone(),
                    section,
                    decay_profile: DecayProfileName::Standard,
                    source: Some(format!("lifecycle:{source_kind}")),
                }).await.map_err(|e| anyhow::anyhow!("{e}"))?;

                let msg = match result.action {
                    StoreAction::Stored => format!("Ingested ({authority}/{source_kind}): {}", content.chars().take(80).collect::<String>()),
                    StoreAction::Reinforced => "Reinforced lifecycle fact".to_string(),
                    StoreAction::Deduplicated => "Duplicate lifecycle fact — already exists".to_string(),
                };
                Ok(ToolResult {
                    content: vec![ContentBlock::Text { text: msg }],
                    details: serde_json::json!({ "action": format!("{:?}", result.action), "id": result.fact.id }),
                })
            }
            _ => anyhow::bail!("Unknown memory tool: {tool_name}"),
        }
    }

    fn provide_context(&self, _signals: &ContextSignals<'_>) -> Option<ContextInjection> {
        // Run async in a blocking context since provide_context is sync
        let mind = self.mind.clone();
        let wm_ids = self.working_memory.lock().unwrap().clone();

        // Use tokio::runtime::Handle to block on async backend calls
        let handle = tokio::runtime::Handle::try_current().ok()?;
        let backend = &self.backend;
        let renderer = &self.renderer;

        std::thread::scope(|scope| {
            scope.spawn(|| {
                handle.block_on(async {
                    let facts = backend.list_facts(&mind, FactFilter::default()).await.ok()?;
                    let episodes = backend.list_episodes(&mind, 1).await.ok()?;

                    // Resolve working memory facts
                    let mut wm_facts = Vec::new();
                    for id in &wm_ids {
                        if let Ok(Some(f)) = backend.get_fact(id).await {
                            wm_facts.push(f);
                        }
                    }

                    let rendered = renderer.render_context(&facts, &episodes, &wm_facts, 12_000);
                    if rendered.markdown.is_empty() {
                        return None;
                    }

                    Some(ContextInjection {
                        source: "memory".into(),
                        content: rendered.markdown,
                        priority: 200, // high — memory is important context
                        ttl_turns: 1,  // re-injected every turn
                    })
                })
            }).join().ok()?
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use omegon_memory::{InMemoryBackend, MemoryBackend};
    use std::sync::Arc;

    #[tokio::test]
    async fn feature_exposes_12_tools() {
        let backend: Arc<dyn MemoryBackend> = Arc::new(InMemoryBackend::new());
        let feature = MemoryFeature::new(backend, "test".into());
        let tools = feature.tools();
        assert_eq!(tools.len(), 12, "Should have exactly 12 memory tools");
        
        let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"memory_store"));
        assert!(names.contains(&"memory_recall"));
        assert!(names.contains(&"memory_query"));
        assert!(names.contains(&"memory_archive"));
        assert!(names.contains(&"memory_supersede"));
        assert!(names.contains(&"memory_connect"));
        assert!(names.contains(&"memory_focus"));
        assert!(names.contains(&"memory_release"));
        assert!(names.contains(&"memory_episodes"));
        assert!(names.contains(&"memory_compact"));
        assert!(names.contains(&"memory_search_archive"));
        assert!(names.contains(&"memory_ingest_lifecycle"));
    }

    #[tokio::test]
    async fn store_and_query_integration() {
        let backend: Arc<dyn MemoryBackend> = Arc::new(InMemoryBackend::new());
        let feature = MemoryFeature::new(backend, "test".into());
        let cancel = tokio_util::sync::CancellationToken::new();

        // Store a fact
        let result = feature.execute(
            "memory_store", "c1",
            serde_json::json!({"section": "Architecture", "content": "System uses microservices"}),
            cancel.clone(),
        ).await.unwrap();
        assert!(result.content[0].as_text().unwrap().contains("Stored"));

        // Query all facts
        let result = feature.execute(
            "memory_query", "c2",
            serde_json::json!({}),
            cancel.clone(),
        ).await.unwrap();
        let text = result.content[0].as_text().unwrap();
        assert!(text.contains("microservices"), "query should return stored fact: {text}");
    }

    #[tokio::test]
    async fn recall_search() {
        let backend: Arc<dyn MemoryBackend> = Arc::new(InMemoryBackend::new());
        let feature = MemoryFeature::new(backend, "test".into());
        let cancel = tokio_util::sync::CancellationToken::new();

        // Store a fact
        feature.execute(
            "memory_store", "c1",
            serde_json::json!({"section": "Architecture", "content": "Authentication uses OAuth2 with PKCE flow"}),
            cancel.clone(),
        ).await.unwrap();

        // Search for it
        let result = feature.execute(
            "memory_recall", "c2",
            serde_json::json!({"query": "OAuth authentication"}),
            cancel.clone(),
        ).await.unwrap();
        let text = result.content[0].as_text().unwrap();
        assert!(text.contains("OAuth2"), "recall should find auth fact: {text}");
    }

    #[tokio::test]
    async fn working_memory_focus_release() {
        let backend: Arc<dyn MemoryBackend> = Arc::new(InMemoryBackend::new());
        let feature = MemoryFeature::new(backend, "test".into());
        let cancel = tokio_util::sync::CancellationToken::new();

        // Focus some fact IDs
        feature.execute(
            "memory_focus", "c1",
            serde_json::json!({"fact_ids": ["f1", "f2", "f3"]}),
            cancel.clone(),
        ).await.unwrap();

        {
            let wm = feature.working_memory.lock().unwrap();
            assert_eq!(wm.len(), 3);
            assert!(wm.contains(&"f1".to_string()));
            assert!(wm.contains(&"f2".to_string()));
            assert!(wm.contains(&"f3".to_string()));
        }

        // Release working memory
        feature.execute(
            "memory_release", "c2",
            serde_json::json!({}),
            cancel.clone(),
        ).await.unwrap();

        {
            let wm = feature.working_memory.lock().unwrap();
            assert!(wm.is_empty());
        }
    }

    #[tokio::test]
    async fn memory_archive() {
        let backend: Arc<dyn MemoryBackend> = Arc::new(InMemoryBackend::new());
        let feature = MemoryFeature::new(backend, "test".into());
        let cancel = tokio_util::sync::CancellationToken::new();

        // Store a fact first
        let store_result = feature.execute(
            "memory_store", "c1",
            serde_json::json!({"section": "Architecture", "content": "Test fact to archive"}),
            cancel.clone(),
        ).await.unwrap();

        // Extract fact ID from store result
        let fact_id = store_result.details["id"].as_str().unwrap();

        // Archive it
        let archive_result = feature.execute(
            "memory_archive", "c2",
            serde_json::json!({"fact_ids": [fact_id]}),
            cancel.clone(),
        ).await.unwrap();

        assert!(archive_result.content[0].as_text().unwrap().contains("Archived 1 fact(s)"));
    }

    #[test]
    fn backend_accessor() {
        let backend: Arc<dyn MemoryBackend> = Arc::new(InMemoryBackend::new());
        let feature = MemoryFeature::new(backend.clone(), "test".into());
        
        // Should be able to access the backend
        let _backend_ref = feature.backend();
        assert_eq!(feature.mind(), "test");
    }
}
