//! MemoryProvider — integrates MemoryBackend with the agent loop.
//!
//! Implements:
//! - `ToolProvider` — exposes memory_store, memory_recall, memory_query,
//!   memory_focus, memory_archive, memory_supersede, memory_connect tools
//! - `ContextProvider` — injects relevant facts into the system prompt per-turn
//! - `SessionHook` — loads facts on startup, persists on shutdown

use async_trait::async_trait;
use omegon_traits::*;
use serde_json::Value;
use std::sync::Mutex;

use crate::backend::{ContextRenderer, MemoryBackend};
use crate::types::*;

/// Wraps a MemoryBackend and provides tools, context, and session hooks
/// to the agent loop.
pub struct MemoryProvider<B: MemoryBackend, R: ContextRenderer> {
    backend: B,
    renderer: R,
    mind: String,
    /// Pinned fact IDs for working memory.
    working_memory: Mutex<Vec<String>>,
    applicability_context: Option<ApplicabilityContext>,
    operation_namespace: String,
    memory_token_cap: usize,
    last_selection: Mutex<Option<MemorySelectionReport>>,
    selection_cache: Mutex<crate::selection_cache::MemorySelectionCache>,
}

impl<B: MemoryBackend, R: ContextRenderer> MemoryProvider<B, R> {
    pub fn new(backend: B, renderer: R, mind: String) -> Self {
        Self {
            backend,
            renderer,
            mind,
            working_memory: Mutex::new(Vec::new()),
            applicability_context: None,
            operation_namespace: crate::util::gen_id(),
            memory_token_cap: crate::selection::DEFAULT_MEMORY_TOKEN_CAP,
            last_selection: Mutex::new(None),
            selection_cache: Mutex::new(Default::default()),
        }
    }

    pub fn backend(&self) -> &B {
        &self.backend
    }

    pub fn with_applicability_context(mut self, context: ApplicabilityContext) -> Self {
        self.applicability_context = Some(context);
        self
    }

    pub fn with_memory_token_cap(mut self, cap: usize) -> Self {
        self.memory_token_cap = cap.min(crate::selection::MAX_MEMORY_TOKEN_CAP);
        self
    }

    fn parse_section_arg(section_str: &str) -> anyhow::Result<Section> {
        serde_json::from_value(Value::String(section_str.into())).map_err(|_| {
            anyhow::anyhow!(
                "invalid memory section '{section_str}'; expected one of Architecture, Decisions, Constraints, Known Issues, Patterns & Conventions, Specs"
            )
        })
    }
}

// ─── Tool definitions ───────────────────────────────────────────────────────

fn tool_defs() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name:"memory_selection".into(),label:"memory_selection".into(),description:"Inspect the last ambient memory selection report: evidence handles, exclusions, and token accounting.".into(),
            parameters:serde_json::json!({"type":"object","properties":{},"additionalProperties":false}),capabilities:vec![ToolCapability::Orientation],
        },
        ToolDefinition {
            name:"memory_set_applicability".into(),label:"memory_set_applicability".into(),description:"Record version-checked applicability without reinforcement or lifecycle change.".into(),
            parameters:serde_json::json!({"type":"object","required":["fact_id","expected_version","applicability"],"properties":{"fact_id":{"type":"string"},"expected_version":{"type":"integer"},"applicability":crate::applicability::constraints_schema()}}),
            capabilities:vec![ToolCapability::StateChanging],
        },
        ToolDefinition {
            name:"memory_inspect".into(),label:"memory_inspect".into(),
            description:"Inspect one fact across active, historical, or pending states without reinforcement. Reports recorded provenance; artifact availability is not checked by the standalone provider.".into(),
            parameters:serde_json::json!({"type":"object","required":["fact_id"],"additionalProperties":false,"properties":{"fact_id":{"type":"string"}}}),
            capabilities:vec![ToolCapability::Orientation],
        },
        ToolDefinition {
            name: "memory_store".into(),
            label: "memory_store".into(),
            description: "Store a durable fact in Omegon runtime memory. Facts persist across sessions. Check existing facts first; supersede stale facts instead of storing paraphrases.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "required": ["section", "content"],
                "properties": {
                    "applicability": crate::applicability::constraints_schema(),
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
            capabilities: vec![ToolCapability::Orientation],
        },
        ToolDefinition {
            name: "memory_recall".into(),
            label: "memory_recall".into(),
            description: "Search project memory for facts relevant to a query. Returns ranked results.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "required": ["query"],
                "properties": {
                    "context": crate::applicability::context_schema(),
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
            capabilities: vec![ToolCapability::Orientation, ToolCapability::BroadOrientation],
        },
        ToolDefinition {
            name: "memory_query".into(),
            label: "memory_query".into(),
            description: "Read a capped inventory of active facts from Omegon runtime memory. Broad/noisy in mature projects; prefer memory_recall for targeted retrieval.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {}
            }),
            capabilities: vec![ToolCapability::Orientation],
        },
        ToolDefinition {
            name: "memory_archive".into(),
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
            capabilities: vec![ToolCapability::Orientation],
        },
        ToolDefinition {
            name: "memory_supersede".into(),
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
            capabilities: vec![ToolCapability::Orientation],
        },
        ToolDefinition {
            name: "memory_connect".into(),
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
            capabilities: vec![ToolCapability::Orientation],
        },
        ToolDefinition {
            name: "memory_focus".into(),
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
            capabilities: vec![ToolCapability::Orientation],
        },
        ToolDefinition {
            name: "memory_release".into(),
            label: "memory_release".into(),
            description: "Clear working memory — release all pinned facts.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {}
            }),
            capabilities: vec![ToolCapability::Orientation],
        },
        ToolDefinition {
            name: "memory_episodes".into(),
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
            capabilities: vec![ToolCapability::Orientation],
        },
        ToolDefinition {
            name: "memory_compact".into(),
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
            capabilities: vec![ToolCapability::Orientation],
        },
        ToolDefinition {
            name: "memory_ingest_lifecycle".into(),
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
            capabilities: vec![ToolCapability::Orientation],
        },
        ToolDefinition {
            name: "memory_search_archive".into(),
            label: "memory_search_archive".into(),
            description: "Search archived, dormant, and superseded project facts. Results are historical evidence, not current guidance.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "required": ["query"],
                "properties": {
                    "context": crate::applicability::context_schema(),
                    "query": {
                        "type": "string",
                        "description": "Search terms"
                    }
                }
            }),
            capabilities: vec![ToolCapability::Orientation],
        },
    ]
}

// ─── ToolProvider ────────────────────────────────────────────────────────────

#[async_trait]
impl<B: MemoryBackend + 'static, R: ContextRenderer + 'static> ToolProvider
    for MemoryProvider<B, R>
{
    fn tools(&self) -> Vec<ToolDefinition> {
        tool_defs()
    }

    async fn execute(
        &self,
        tool_name: &str,
        _call_id: &str,
        args: Value,
        cancel: tokio_util::sync::CancellationToken,
    ) -> anyhow::Result<ToolResult> {
        match tool_name {
            "memory_selection" => {
                let report = self.last_selection.lock().unwrap().clone();
                Ok(ToolResult {
                    content: vec![ContentBlock::Text {
                        text: serde_json::to_string_pretty(&report)?,
                    }],
                    details: serde_json::to_value(report)?,
                })
            }
            "memory_set_applicability" => {
                let id = args["fact_id"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("fact_id is required"))?;
                if self
                    .backend
                    .get_fact_record(&self.mind, id)
                    .await?
                    .is_none()
                {
                    anyhow::bail!("fact not found in this mind");
                }
                let constraints = crate::applicability::constraints_arg(&args)?
                    .ok_or_else(|| anyhow::anyhow!("applicability is required"))?;
                let version = args["expected_version"]
                    .as_u64()
                    .ok_or_else(|| anyhow::anyhow!("expected_version is required"))?;
                let outcome = self
                    .backend
                    .apply_mutation(
                        &format!(
                            "provider:{}:{_call_id}:applicability",
                            self.operation_namespace
                        ),
                        MemoryMutation::SetFactApplicability {
                            fact: FactPrecondition {
                                id: id.into(),
                                expected_version: version,
                            },
                            constraints: Box::new(constraints),
                        },
                    )
                    .await?;
                Ok(ToolResult {
                    content: vec![ContentBlock::Text {
                        text: "Recorded applicability without reinforcement or lifecycle change."
                            .into(),
                    }],
                    details: serde_json::to_value(outcome)?,
                })
            }
            "memory_inspect" => {
                let id = crate::inspection::fact_id(&args)?;
                if cancel.is_cancelled() {
                    return Err(crate::MemoryError::Cancelled.into());
                }
                let fact = self
                    .backend
                    .get_fact_record(&self.mind, id)
                    .await?
                    .ok_or_else(|| crate::MemoryError::FactNotFound(id.into()))?;
                let mut inspection = FactInspection::from_fact(&fact);
                let filter = SearchFilter {
                    context: self.applicability_context.clone(),
                    ..Default::default()
                }
                .resolved()?;
                inspection.applicability_status = filter.applicability_status(&fact);
                inspection.applicability_context = filter.context.expect("resolved context");
                Ok(ToolResult {
                    content: vec![ContentBlock::Text {
                        text: crate::inspection::render(&inspection)?,
                    }],
                    details: serde_json::to_value(inspection)?,
                })
            }
            "memory_store" => {
                let content = args["content"].as_str().unwrap_or("").to_string();
                let section_str = args["section"].as_str().unwrap_or("Architecture");
                let section = Self::parse_section_arg(section_str)?;

                let request = StoreFact {
                    mind: self.mind.clone(),
                    content: content.clone(),
                    section,
                    decay_profile: DecayProfileName::Standard,
                    source: Some("manual".into()),
                };
                let (fact_id, action) =
                    if let Some(constraints) = crate::applicability::constraints_arg(&args)? {
                        let outcome = self
                            .backend
                            .apply_mutation(
                                &format!("provider:{}:{_call_id}:store", self.operation_namespace),
                                MemoryMutation::StoreApplicableFact {
                                    request,
                                    constraints: Box::new(constraints),
                                },
                            )
                            .await?;
                        let MemoryMutationEffect::FactStored {
                            fact_id, action, ..
                        } = outcome.effect
                        else {
                            anyhow::bail!("unexpected scoped store effect");
                        };
                        (fact_id, action)
                    } else {
                        let result = self.backend.store_fact(request).await?;
                        (result.fact.id, result.action)
                    };

                let msg = match action {
                    StoreAction::Stored => format!("Stored in {}: {}", section_str, content),
                    StoreAction::Reinforced => format!("Reinforced existing fact: {}", content),
                    StoreAction::Deduplicated => "Duplicate — fact already exists".to_string(),
                };
                Ok(ToolResult {
                    content: vec![ContentBlock::Text { text: msg }],
                    details: serde_json::json!({ "id": fact_id, "action": format!("{:?}", action) }),
                })
            }
            "memory_recall" => {
                let query = args["query"].as_str().unwrap_or("").to_string();
                let k = args["k"].as_u64().unwrap_or(10).min(10_000) as usize;
                let filter = SearchFilter {
                    context: crate::applicability::context_arg(&args)?
                        .or_else(|| self.applicability_context.clone()),
                    section: args["section"]
                        .as_str()
                        .map(Self::parse_section_arg)
                        .transpose()?,
                    ..Default::default()
                };

                // Use FTS search (vector search requires embeddings which may not be available)
                let results = self
                    .backend
                    .fts_search_filtered(
                        &self.mind,
                        &query,
                        k.saturating_mul(2).min(10_000),
                        &filter,
                    )
                    .await
                    .map_err(|e| anyhow::anyhow!("{e}"))?;

                let results = crate::service::expand_edges_filtered_checked(
                    &self.backend,
                    &self.mind,
                    results,
                    k,
                    &filter,
                    &|| cancel.is_cancelled(),
                )
                .await?;

                if results.is_empty() {
                    return Ok(ToolResult {
                        content: vec![ContentBlock::Text {
                            text: "No matching facts found.".into(),
                        }],
                        details: Value::Null,
                    });
                }

                let mut lines = Vec::new();
                for (i, sf) in results.iter().enumerate() {
                    let section = serde_json::to_string(&sf.fact.section).unwrap_or_default();
                    let section = section.trim_matches('"');
                    // Truncate very long facts in recall results
                    let content = if sf.fact.content.len() > 200 {
                        format!("{}…", sf.fact.content.chars().take(197).collect::<String>())
                    } else {
                        sf.fact.content.clone()
                    };
                    lines.push(format!(
                        "{}. [{}] ({}, {}) {}",
                        i + 1,
                        sf.fact.id,
                        section,
                        crate::renderer::recall_score_label(sf),
                        content,
                    ));
                }
                Ok(ToolResult {
                    content: vec![ContentBlock::Text {
                        text: lines.join("\n"),
                    }],
                    details: serde_json::json!({ "count": results.len() }),
                })
            }
            "memory_query" => {
                let facts = self
                    .backend
                    .list_facts(&self.mind, FactFilter::default())
                    .await
                    .map_err(|e| anyhow::anyhow!("{e}"))?;

                if facts.is_empty() {
                    return Ok(ToolResult {
                        content: vec![ContentBlock::Text {
                            text: "No facts in memory.".into(),
                        }],
                        details: serde_json::json!({ "count": 0 }),
                    });
                }

                // Group by section, show counts + sample facts (capped to avoid overwhelming the model)
                let mut sections: std::collections::BTreeMap<String, Vec<&Fact>> =
                    std::collections::BTreeMap::new();
                for fact in &facts {
                    let section = serde_json::to_string(&fact.section).unwrap_or_default();
                    let section = section.trim_matches('"').to_string();
                    sections.entry(section).or_default().push(fact);
                }

                let mut lines = Vec::new();
                lines.push("Stored active inventory; applicability is not filtered here. Use memory_recall for current guidance.".into());
                lines.push(format!(
                    "{} facts across {} sections:\n",
                    facts.len(),
                    sections.len()
                ));

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
                        lines.push(format!(
                            "  … +{} more (use memory_recall for targeted search)",
                            section_facts.len() - max_per_section
                        ));
                    }
                    lines.push(String::new());
                }

                Ok(ToolResult {
                    content: vec![ContentBlock::Text {
                        text: lines.join("\n"),
                    }],
                    details: serde_json::json!({ "count": facts.len(), "sections": sections.len() }),
                })
            }
            "memory_archive" => {
                let ids: Vec<String> = args["fact_ids"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();
                let id_refs: Vec<&str> = ids.iter().map(|s| s.as_str()).collect();
                let count = self
                    .backend
                    .archive_facts(&id_refs)
                    .await
                    .map_err(|e| anyhow::anyhow!("{e}"))?;
                Ok(ToolResult {
                    content: vec![ContentBlock::Text {
                        text: format!("Archived {count} fact(s)."),
                    }],
                    details: serde_json::json!({ "archived": count }),
                })
            }
            "memory_supersede" => {
                let fact_id = args["fact_id"].as_str().unwrap_or("").to_string();
                let content = args["content"].as_str().unwrap_or("").to_string();
                let section_str = args["section"].as_str().unwrap_or("Architecture");
                let section = Self::parse_section_arg(section_str)?;

                let new_fact = self
                    .backend
                    .supersede_fact(
                        &fact_id,
                        StoreFact {
                            mind: self.mind.clone(),
                            content,
                            section,
                            decay_profile: DecayProfileName::Standard,
                            source: Some("manual".into()),
                        },
                    )
                    .await
                    .map_err(|e| anyhow::anyhow!("{e}"))?;
                Ok(ToolResult {
                    content: vec![ContentBlock::Text {
                        text: format!("Superseded {} → new fact {}", fact_id, new_fact.id),
                    }],
                    details: serde_json::json!({ "old_id": fact_id, "new_id": new_fact.id }),
                })
            }
            "memory_connect" => {
                let edge = self
                    .backend
                    .create_edge(CreateEdge {
                        source_id: args["source_fact_id"].as_str().unwrap_or("").into(),
                        target_id: args["target_fact_id"].as_str().unwrap_or("").into(),
                        relation: args["relation"].as_str().unwrap_or("").into(),
                        description: args["description"].as_str().map(String::from),
                    })
                    .await
                    .map_err(|e| anyhow::anyhow!("{e}"))?;
                Ok(ToolResult {
                    content: vec![ContentBlock::Text {
                        text: format!(
                            "Connected {} → {} ({})",
                            edge.source_id, edge.target_id, edge.relation
                        ),
                    }],
                    details: serde_json::json!({ "edge_id": edge.id }),
                })
            }
            "memory_focus" => {
                let ids: Vec<String> = args["fact_ids"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();
                let count = ids.len();
                self.working_memory.lock().unwrap().extend(ids);
                Ok(ToolResult {
                    content: vec![ContentBlock::Text {
                        text: format!("Pinned {count} fact(s) to working memory."),
                    }],
                    details: Value::Null,
                })
            }
            "memory_release" => {
                self.working_memory.lock().unwrap().clear();
                Ok(ToolResult {
                    content: vec![ContentBlock::Text {
                        text: "Working memory cleared.".into(),
                    }],
                    details: Value::Null,
                })
            }
            "memory_episodes" => {
                let query = args["query"].as_str().unwrap_or("").to_string();
                let k = args["k"].as_u64().unwrap_or(5) as usize;
                let episodes = self
                    .backend
                    .search_episodes(&self.mind, &query, k)
                    .await
                    .map_err(|e| anyhow::anyhow!("{e}"))?;
                if episodes.is_empty() {
                    return Ok(ToolResult {
                        content: vec![ContentBlock::Text {
                            text: "No matching episodes found.".into(),
                        }],
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
                    content: vec![ContentBlock::Text {
                        text: lines.join("\n"),
                    }],
                    details: Value::Null,
                })
            }
            "memory_compact" => {
                // Context compaction is handled at the conversation level, not memory level.
                // Signal the caller that compaction was requested.
                Ok(ToolResult {
                    content: vec![ContentBlock::Text {
                        text: "Context compaction requested. The agent loop will compact older conversation history.".into()
                    }],
                    details: serde_json::json!({ "action": "compact_requested" }),
                })
            }
            "memory_ingest_lifecycle" => {
                // Lifecycle fact ingestion — stores with source metadata
                let content = args["content"].as_str().unwrap_or("").to_string();
                let section_str = args["section"].as_str().unwrap_or("Architecture");
                let section = Self::parse_section_arg(section_str)?;
                let authority = args["authority"].as_str().unwrap_or("inferred");
                let source_kind = args["source_kind"].as_str().unwrap_or("unknown");

                let result = self
                    .backend
                    .store_fact(StoreFact {
                        mind: self.mind.clone(),
                        content: content.clone(),
                        section,
                        decay_profile: DecayProfileName::Standard,
                        source: Some(format!("lifecycle:{source_kind}")),
                    })
                    .await
                    .map_err(|e| anyhow::anyhow!("{e}"))?;

                let msg = match result.action {
                    StoreAction::Stored => format!(
                        "Ingested ({authority}/{source_kind}): {}",
                        content.chars().take(80).collect::<String>()
                    ),
                    StoreAction::Reinforced => "Reinforced lifecycle fact".to_string(),
                    StoreAction::Deduplicated => {
                        "Duplicate lifecycle fact — already exists".to_string()
                    }
                };
                Ok(ToolResult {
                    content: vec![ContentBlock::Text { text: msg }],
                    details: serde_json::json!({ "action": format!("{:?}", result.action), "id": result.fact.id }),
                })
            }
            "memory_search_archive" => {
                let query = args["query"].as_str().unwrap_or("").to_string();
                let filter = SearchFilter {
                    intent: SearchIntent::Historical,
                    section: None,
                    context: crate::applicability::context_arg(&args)?,
                };
                let results = self
                    .backend
                    .fts_search_filtered(&self.mind, &query, 20, &filter)
                    .await
                    .map_err(|e| anyhow::anyhow!("{e}"))?;
                if results.is_empty() {
                    return Ok(ToolResult {
                        content: vec![ContentBlock::Text {
                            text: "No matching archived facts found.".into(),
                        }],
                        details: Value::Null,
                    });
                }
                let mut lines = Vec::new();
                for scored in &results {
                    let f = &scored.fact;
                    lines.push(format!(
                        "[{}] ({:?}, {:?}) {}\n  {}",
                        f.id,
                        f.section,
                        f.status,
                        f.content,
                        crate::renderer::recall_score_label(scored)
                    ));
                }
                Ok(ToolResult {
                    content: vec![ContentBlock::Text {
                        text: lines.join("\n"),
                    }],
                    details: Value::Null,
                })
            }
            _ => anyhow::bail!("Unknown memory tool: {tool_name}"),
        }
    }
}

// ─── ContextProvider ────────────────────────────────────────────────────────

impl<B: MemoryBackend + 'static, R: ContextRenderer + 'static> ContextProvider
    for MemoryProvider<B, R>
{
    fn provide_context(&self, signals: &ContextSignals<'_>) -> Option<ContextInjection> {
        // Run async in a blocking context since ContextProvider is sync
        let mind = self.mind.clone();
        let wm_ids = self.working_memory.lock().unwrap().clone();

        // For now: use tokio::runtime::Handle to block on async backend calls
        // This is acceptable because provide_context runs once per turn and the
        // backend operations are fast (<10ms for in-memory, <50ms for sqlite).
        let empty = || ContextInjection {
            source: "memory".into(),
            content: String::new(),
            priority: 200,
            ttl_turns: 1,
        };
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            *self.last_selection.lock().unwrap() = Some(crate::selection::empty_report(Some(
                "runtime_unavailable".into(),
            )));
            return Some(empty());
        };
        let backend = &self.backend;
        let renderer = &self.renderer;

        std::thread::scope(|scope| {
            scope
                .spawn(|| {
                    handle.block_on(async {
                        let filter = SearchFilter {
                            context: self.applicability_context.clone(),
                            ..Default::default()
                        }
                        .resolved()
                        .ok()?;
                        let mut cache = std::mem::take(&mut *self.selection_cache.lock().unwrap());
                        let selected = cache
                            .select(
                                backend,
                                &MemorySelectionRequest {
                                    mind,
                                    query: signals.user_prompt.into(),
                                    pins: wm_ids,
                                    context: filter.context.expect("resolved context"),
                                    intent: MemorySelectionIntent::Ambient,
                                    host_budget: signals.context_budget_tokens,
                                    memory_cap: self.memory_token_cap,
                                    fetch_limit: crate::selection::MAX_CANDIDATES,
                                },
                                &crate::selection::ConservativeUtf8Counter,
                                renderer,
                            )
                            .await
                            .ok()?;
                        *self.selection_cache.lock().unwrap() = cache;
                        *self.last_selection.lock().unwrap() = Some(selected.report);
                        Some(ContextInjection {
                            source: "memory".into(),
                            content: selected.markdown,
                            priority: 200, // high — memory is important context
                            ttl_turns: 3,  // persist for 3 turns; re-rendered on mutation
                        })
                    })
                })
                .join()
                .ok()?
        })
        .or_else(|| {
            *self.last_selection.lock().unwrap() = Some(crate::selection::empty_report(Some(
                "memory_selection_failed".into(),
            )));
            Some(empty())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inmemory::InMemoryBackend;

    struct NoopRenderer;
    impl ContextRenderer for NoopRenderer {
        fn render_context(
            &self,
            facts: &[Fact],
            _episodes: &[Episode],
            _wm: &[Fact],
            _max_chars: usize,
        ) -> crate::types::RenderedContext {
            crate::types::RenderedContext {
                markdown: if facts.is_empty() {
                    String::new()
                } else {
                    format!("{} facts loaded", facts.len())
                },
                facts_injected: facts.len(),
                episodes_injected: 0,
                char_count: 0,
                budget_exhausted: false,
            }
        }
    }

    #[tokio::test]
    async fn tool_provider_exposes_inspection_with_memory_tools() {
        let provider = MemoryProvider::new(InMemoryBackend::new(), NoopRenderer, "test".into());
        let tools = provider.tools();
        assert_eq!(tools.len(), 15);
        assert!(tools.iter().any(|tool| tool.name == "memory_inspect"));
        let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"memory_store"));
        assert!(names.contains(&"memory_recall"));
        assert!(names.contains(&"memory_query"));
        assert!(names.contains(&"memory_archive"));
        assert!(names.contains(&"memory_supersede"));
        assert!(names.contains(&"memory_connect"));
        assert!(names.contains(&"memory_focus"));
        assert!(names.contains(&"memory_release"));
    }

    #[tokio::test]
    async fn store_and_query_via_tools() {
        let provider = MemoryProvider::new(InMemoryBackend::new(), NoopRenderer, "test".into());
        let cancel = tokio_util::sync::CancellationToken::new();

        // Store
        let result = provider.execute(
            "memory_store", "c1",
            serde_json::json!({"section": "Architecture", "content": "System uses microservices"}),
            cancel.clone(),
        ).await.unwrap();
        assert!(result.content[0].as_text().unwrap().contains("Stored"));

        // Query
        let result = provider
            .execute("memory_query", "c2", serde_json::json!({}), cancel.clone())
            .await
            .unwrap();
        let text = result.content[0].as_text().unwrap();
        assert!(
            text.contains("microservices"),
            "query should return stored fact: {text}"
        );
    }

    #[tokio::test]
    async fn recall_via_tool() {
        let provider = MemoryProvider::new(InMemoryBackend::new(), NoopRenderer, "test".into());
        let cancel = tokio_util::sync::CancellationToken::new();

        provider.execute(
            "memory_store", "c1",
            serde_json::json!({"section": "Architecture", "content": "Authentication uses OAuth2 with PKCE flow"}),
            cancel.clone(),
        ).await.unwrap();

        let result = provider
            .execute(
                "memory_recall",
                "c2",
                serde_json::json!({"query": "OAuth authentication"}),
                cancel.clone(),
            )
            .await
            .unwrap();
        let text = result.content[0].as_text().unwrap();
        assert!(
            text.contains("OAuth2"),
            "recall should find auth fact: {text}"
        );
    }

    #[tokio::test]
    async fn focus_and_release() {
        let provider = MemoryProvider::new(InMemoryBackend::new(), NoopRenderer, "test".into());
        let cancel = tokio_util::sync::CancellationToken::new();

        provider
            .execute(
                "memory_focus",
                "c1",
                serde_json::json!({"fact_ids": ["f1", "f2"]}),
                cancel.clone(),
            )
            .await
            .unwrap();

        {
            let wm = provider.working_memory.lock().unwrap();
            assert_eq!(wm.len(), 2);
        }

        provider
            .execute(
                "memory_release",
                "c2",
                serde_json::json!({}),
                cancel.clone(),
            )
            .await
            .unwrap();

        let wm = provider.working_memory.lock().unwrap();
        assert!(wm.is_empty());
    }
}
