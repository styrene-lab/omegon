//! MemoryFeature — integrated memory system.
//!
//! This feature provides all 12 memory_* agent-callable tools and context injection
//! over the boot-captured managed memory generation.
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
use sha2::{Digest, Sha256};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use omegon_memory::{
    CreateEdge, DecayProfileName, EmbeddingService, FactPrecondition, MemoryMutation,
    MemoryMutationEffect, Section, StoreAction, StoreEpisode, StoreFact,
};

struct SessionEndTask {
    cancellation: tokio_util::sync::CancellationToken,
    handle: std::thread::JoinHandle<Result<(), String>>,
}

struct SessionEndTaskState {
    accepting: bool,
    tasks: Vec<SessionEndTask>,
    failures: Vec<String>,
}

impl Default for SessionEndTaskState {
    fn default() -> Self {
        Self {
            accepting: true,
            tasks: Vec::new(),
            failures: Vec::new(),
        }
    }
}

#[derive(Debug)]
struct MemoryFeatureInvokeError(
    ManagedServiceCallError<crate::memory_service::MemoryServiceErrorV1>,
);

impl MemoryFeatureInvokeError {
    fn code(&self) -> &'static str {
        match &self.0 {
            ManagedServiceCallError::Operation(error) => match error.code {
                crate::memory_service::MemoryServiceErrorCodeV1::Cancelled => "memory:cancelled",
                crate::memory_service::MemoryServiceErrorCodeV1::Unavailable
                | crate::memory_service::MemoryServiceErrorCodeV1::StoreUnavailable => {
                    "memory:unavailable"
                }
                crate::memory_service::MemoryServiceErrorCodeV1::OperationConflict => {
                    "memory:operation_conflict"
                }
                crate::memory_service::MemoryServiceErrorCodeV1::FactVersionConflict => {
                    "memory:fact_version_conflict"
                }
                _ => "memory:operation_failed",
            },
            ManagedServiceCallError::Cancelled => "memory:cancelled",
            ManagedServiceCallError::GenerationDraining
            | ManagedServiceCallError::GenerationDegraded
            | ManagedServiceCallError::GenerationRetired => "memory:unavailable",
            ManagedServiceCallError::Panicked => "memory:panicked",
        }
    }
}

impl std::fmt::Display for MemoryFeatureInvokeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}", self.code())
    }
}

impl std::error::Error for MemoryFeatureInvokeError {}

pub(crate) mod confirmation;
mod formation;
mod lifecycle;

/// Memory feature that provides all memory_* tools and context injection.
pub struct MemoryFeature {
    /// Renderer for context injection
    last_selection: Mutex<Option<omegon_memory::MemorySelectionReport>>,
    /// Mind identifier (normally `primensus` for automatic LLM memory).
    mind: String,
    /// Pinned fact IDs for working memory
    working_memory: Mutex<Vec<String>>,
    /// Set by execute() when a successful memory mutation/focus change should
    /// trigger a refreshed HarnessStatus snapshot after ToolEnd delivery.
    pending_status_refresh: AtomicBool,
    /// Optional embedding service for hybrid search + auto-embed on store.
    embed_service: Option<Arc<dyn EmbeddingService>>,
    /// Hash of the last rendered memory context. When content hasn't changed,
    /// `provide_context` returns None to skip re-injection — the existing
    /// injection persists via its TTL instead of being re-rendered.
    last_context_hash: Mutex<u64>,
    last_context_turn: Mutex<Option<u32>>,
    /// Set to true by memory mutation tools so the next provide_context()
    /// re-renders even if the hash would match (facts changed underneath).
    context_dirty: AtomicBool,
    /// Boot-captured managed owner for every durable memory operation.
    memory_binding: crate::memory_service::MemoryBinding,
    /// Host-routed extraction of pending candidates from attributed session evidence.
    extractor: Option<Arc<dyn formation::Extractor>>,
    session_binding: Option<crate::session_consumers::DeferredSessionViewBinding>,
    session_id: Mutex<Option<String>>,
    session_end_tasks: Arc<Mutex<SessionEndTaskState>>,
    status_root: std::path::PathBuf,
}

impl MemoryFeature {
    pub(crate) fn new(memory_binding: crate::memory_service::MemoryBinding, mind: String) -> Self {
        Self {
            last_selection: Mutex::new(None),
            mind,
            working_memory: Mutex::new(Vec::new()),
            pending_status_refresh: AtomicBool::new(false),
            embed_service: None,
            last_context_hash: Mutex::new(0),
            last_context_turn: Mutex::new(None),
            context_dirty: AtomicBool::new(true), // force initial render
            memory_binding,
            extractor: None,
            session_binding: None,
            session_id: Mutex::new(None),
            session_end_tasks: Arc::new(Mutex::new(SessionEndTaskState::default())),
            status_root: std::env::current_dir().unwrap_or_default(),
        }
    }

    pub fn with_extraction_model(mut self, model: String) -> Self {
        let model = model.trim();
        if model.is_empty()
            || model.len() > omegon_memory::formation::MAX_IDENTIFIER_BYTES
            || model.chars().any(char::is_control)
        {
            self.extractor = None;
            tracing::warn!("invalid memory extraction model configuration; extraction disabled");
        } else {
            self.extractor = Some(Arc::new(formation::ModelExtractor(model.into())));
        }
        self
    }

    pub(crate) fn with_capabilities(
        mut self,
        profile: &crate::settings::Profile,
        child: bool,
        embeddings: Option<Arc<dyn EmbeddingService>>,
    ) -> Self {
        self.extractor = None;
        self.embed_service = embeddings;
        if !child && profile.memory_extraction_enabled != Some(false) {
            let model = profile
                .memory_extraction_model
                .as_deref()
                .unwrap_or(formation::DEFAULT_EXTRACTION_MODEL)
                .trim();
            self = self.with_extraction_model(model.into());
        }
        self
    }

    pub(crate) fn with_session_binding(
        mut self,
        binding: crate::session_consumers::DeferredSessionViewBinding,
    ) -> Self {
        self.session_binding = Some(binding);
        self
    }

    pub(crate) fn with_status_root(mut self, root: std::path::PathBuf) -> Self {
        self.status_root = root;
        self
    }

    /// Attach an embedding service for hybrid search and auto-embed on store.
    pub fn with_embed_service(mut self, svc: Arc<dyn EmbeddingService>) -> Self {
        self.embed_service = Some(svc);
        self
    }

    fn parse_section_arg(section_str: &str) -> anyhow::Result<Section> {
        let normalized = match section_str {
            "architecture" => "Architecture",
            "decisions" => "Decisions",
            "constraints" => "Constraints",
            "known_issues" | "known issues" => "Known Issues",
            "patterns_conventions" | "patterns & conventions" => "Patterns & Conventions",
            "specs" => "Specs",
            other => other,
        };
        serde_json::from_value(Value::String(normalized.into())).map_err(|_| {
            anyhow::anyhow!(
                "invalid memory section '{section_str}'; expected one of Architecture, Decisions, Constraints, Known Issues, Patterns & Conventions, Specs"
            )
        })
    }

    /// Get the current mind identifier.
    pub fn mind(&self) -> &str {
        &self.mind
    }

    /// Replace an earlier injection when selection becomes empty. Returning None
    /// means "keep the live TTL", not "clear memory", to the context assembler.
    fn clear_memory_context(&self) -> Option<ContextInjection> {
        let mut last_hash = self.last_context_hash.lock().unwrap();
        if *last_hash == 0 {
            return None;
        }
        *last_hash = 0;
        *self.last_context_turn.lock().unwrap() = None;
        Some(ContextInjection {
            source: "memory".into(),
            content: String::new(),
            priority: 200,
            ttl_turns: 1,
        })
    }

    fn tool_operation_id(&self, call_id: &str, operation: &str) -> anyhow::Result<String> {
        let session = self
            .session_id
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| anyhow::anyhow!("memory session identity is unavailable"))?;
        Ok(format!("tool:{session}:{call_id}:{operation}"))
    }

    async fn invoke(
        &self,
        request: crate::memory_service::MemoryRequestV1,
    ) -> Result<crate::memory_service::MemoryPayloadV1, MemoryFeatureInvokeError> {
        self.memory_binding
            .invoke(request)
            .await
            .map(|response| response.payload)
            .map_err(MemoryFeatureInvokeError)
    }

    async fn apply_mutation(
        &self,
        operation_id: String,
        mutation: MemoryMutation,
        cancellation: tokio_util::sync::CancellationToken,
    ) -> anyhow::Result<omegon_memory::MemoryMutationOutcome> {
        match self
            .invoke(crate::memory_service::MemoryRequestV1::ApplyMutation {
                scope: crate::memory_service::MemoryScopeV1::Project,
                operation_id,
                mutation,
                cancellation,
            })
            .await?
        {
            crate::memory_service::MemoryPayloadV1::Mutation(outcome) => Ok(outcome),
            _ => anyhow::bail!("managed memory returned an unexpected mutation response"),
        }
    }

    async fn get_fact(
        &self,
        id: String,
        cancellation: tokio_util::sync::CancellationToken,
    ) -> anyhow::Result<Option<omegon_memory::Fact>> {
        match self
            .invoke(crate::memory_service::MemoryRequestV1::GetFact {
                scope: crate::memory_service::MemoryScopeV1::Project,
                id,
                cancellation,
            })
            .await?
        {
            crate::memory_service::MemoryPayloadV1::Fact(fact) => Ok(*fact),
            _ => anyhow::bail!("managed memory returned an unexpected fact response"),
        }
    }

    async fn refresh_status(&self) {
        crate::status::refresh_managed_memory_status_for_mind(
            &self.memory_binding,
            &self.status_root,
            &self.mind,
        )
        .await;
    }
}

async fn persist_embedding(
    embed_svc: &Arc<dyn EmbeddingService>,
    binding: &crate::memory_service::MemoryBinding,
    fact: FactPrecondition,
    content: String,
    operation_id: String,
    cancellation: tokio_util::sync::CancellationToken,
) {
    let generated = tokio::select! {
        _ = cancellation.cancelled() => return,
        result = tokio::time::timeout(std::time::Duration::from_secs(30), embed_svc.embed_identified(&content)) => result,
    };
    match generated {
        Ok(Ok(embedding)) => {
            if let Err(error) = binding
                .invoke(crate::memory_service::MemoryRequestV1::ApplyMutation {
                    scope: crate::memory_service::MemoryScopeV1::Project,
                    operation_id,
                    mutation: MemoryMutation::StoreIdentifiedEmbedding {
                        fact: fact.clone(),
                        embedding,
                    },
                    cancellation,
                })
                .await
            {
                tracing::warn!(fact_id = %fact.id, ?error, "auto-embed store failed");
            }
        }
        Ok(Err(error)) => {
            tracing::debug!(fact_id = %fact.id, %error, "auto-embed generation failed");
        }
        Err(_) => tracing::warn!(fact_id = %fact.id, "auto-embed generation timed out"),
    }
}

struct SessionEndPipelineInput {
    mind: String,
    memory_binding: crate::memory_service::MemoryBinding,
    extractor: Option<Arc<dyn formation::Extractor>>,
    evidence: omegon_memory::EpisodeFormation,
    session_id: String,
    status_root: std::path::PathBuf,
    turns: u32,
    tool_calls: u32,
    duration_secs: f64,
}

fn formation_episode_request(input: &SessionEndPipelineInput) -> (String, StoreEpisode) {
    let session_key = format!("{:x}", Sha256::digest(input.session_id.as_bytes()));
    let source_key = format!(
        "{:x}",
        Sha256::digest(
            serde_json::to_vec(&(
                &input.mind,
                &input.evidence.source,
                &input.evidence.evidence,
                input.evidence.version,
                input.evidence.truncated,
                input.extractor.as_ref().map(|extractor| extractor.model())
            ))
            .expect("source serialization")
        )
    );
    let mut evidence = input.evidence.clone();
    evidence.candidates.clear();
    evidence.rejected_candidates = 0;
    evidence.extraction = omegon_memory::ExtractionOutcome::Disabled;
    if let Some(extractor) = &input.extractor {
        evidence.extraction = omegon_memory::ExtractionOutcome::Pending {
            model: extractor.model().into(),
        };
    }

    let date = evidence
        .evidence
        .last()
        .and_then(|item| chrono::DateTime::parse_from_rfc3339(&item.recorded_at).ok())
        .map(|timestamp| timestamp.date_naive().to_string());
    (
        format!("session:{session_key}:formation-v2:{source_key}"),
        StoreEpisode {
            mind: input.mind.clone(),
            title: format!("Session memory: {}", input.session_id),
            narrative: evidence.narrative(),
            date,
            affected_nodes: vec![],
            affected_changes: vec![],
            files_changed: vec![],
            tags: vec!["auto".into(), "formation-v2".into()],
            tool_calls_count: None,
            formation: Some(Box::new(evidence)),
        },
    )
}

async fn run_session_end_pipeline(input: SessionEndPipelineInput) {
    const EPISODE_PHASE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
    const VAULT_PHASE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);
    tracing::debug!(
        turns = input.turns,
        tool_calls = input.tool_calls,
        duration_secs = input.duration_secs,
        "advisory session statistics for memory capture"
    );
    let (operation_id, request) = formation_episode_request(&input);
    let evidence = request
        .formation
        .as_deref()
        .expect("formation request")
        .clone();
    let episode_cancellation = tokio_util::sync::CancellationToken::new();
    let episode =
        input
            .memory_binding
            .invoke(crate::memory_service::MemoryRequestV1::ApplyMutation {
                scope: crate::memory_service::MemoryScopeV1::Project,
                operation_id: operation_id.clone(),
                mutation: MemoryMutation::StoreEpisode { request },
                cancellation: episode_cancellation.clone(),
            });
    let stored = match tokio::time::timeout(EPISODE_PHASE_TIMEOUT, episode).await {
        Ok(Ok(crate::memory_service::MemoryResponseV1 {
            payload: crate::memory_service::MemoryPayloadV1::Mutation(outcome),
            ..
        })) => Some(outcome),
        Ok(Err(error)) => {
            tracing::warn!(?error, "session episode storage failed");
            None
        }
        Err(_) => {
            episode_cancellation.cancel();
            tracing::warn!("session episode storage timed out");
            None
        }
        _ => None,
    };
    let Some(stored) = stored else {
        return;
    };
    if !stored.replayed
        && input.extractor.is_some()
        && let MemoryMutationEffect::EpisodeStored { episode_id } = stored.effect
    {
        let completed = formation::extract_candidates(evidence, input.extractor.as_ref()).await;
        let cancellation = tokio_util::sync::CancellationToken::new();
        let completion =
            input
                .memory_binding
                .invoke(crate::memory_service::MemoryRequestV1::ApplyMutation {
                    scope: crate::memory_service::MemoryScopeV1::Project,
                    operation_id: format!("{operation_id}:complete"),
                    mutation: MemoryMutation::CompleteFormation {
                        episode_id,
                        formation: Box::new(completed),
                    },
                    cancellation: cancellation.clone(),
                });
        match tokio::time::timeout(EPISODE_PHASE_TIMEOUT, completion).await {
            Ok(Ok(_)) => {}
            Ok(Err(error)) => tracing::warn!(
                ?error,
                "formation completion failed; source remains durable"
            ),
            Err(_) => {
                cancellation.cancel();
                tracing::warn!("formation completion timed out; source remains durable");
            }
        }
    }

    let vault_cancellation = tokio_util::sync::CancellationToken::new();
    match tokio::time::timeout(
        VAULT_PHASE_TIMEOUT,
        input
            .memory_binding
            .invoke(crate::memory_service::MemoryRequestV1::VaultSessionEnd {
                scope: crate::memory_service::MemoryScopeV1::Project,
                mind: input.mind.clone(),
                cancellation: vault_cancellation.clone(),
            }),
    )
    .await
    {
        Ok(Err(error))
            if !matches!(
                error,
                ManagedServiceCallError::Operation(ref error)
                    if matches!(
                        error.code,
                        crate::memory_service::MemoryServiceErrorCodeV1::Unavailable
                            | crate::memory_service::MemoryServiceErrorCodeV1::SyncNotConfigured
                    )
            ) =>
        {
            tracing::warn!(?error, "vault session-end synchronization failed");
        }
        Err(_) => {
            vault_cancellation.cancel();
            tracing::warn!("vault session-end synchronization timed out");
        }
        _ => {}
    }

    crate::status::refresh_managed_memory_status_for_mind(
        &input.memory_binding,
        &input.status_root,
        &input.mind,
    )
    .await;
}

#[async_trait]
impl Feature for MemoryFeature {
    fn name(&self) -> &str {
        "memory"
    }

    fn runtime_contribution_generation_id(&self) -> Option<RuntimeContributionGenerationId> {
        Some(
            RuntimeContributionGenerationId::new(crate::memory_service::MEMORY_GENERATION)
                .expect("static generation id is valid"),
        )
    }

    fn runtime_lifecycle_policy(&self) -> Option<RuntimeLifecyclePolicy> {
        Some(crate::memory_service::memory_lifecycle_policy())
    }

    fn runtime_transition_policy(&self) -> Option<RuntimeCompositionTransitionPolicy> {
        Some(crate::memory_service::memory_transition_policy())
    }

    fn tools(&self) -> Vec<ToolDefinition> {
        vec![
            ToolDefinition {
                name: crate::tool_registry::memory::MEMORY_STORE.into(),
                label: "memory_store".into(),
                description: "Store a durable fact in Omegon runtime memory. Facts persist across sessions. \
Use this for stable architectural decisions, constraints, bug patterns, project conventions, and durable tradeoffs. \
Before storing, prefer memory_recall to check whether an active fact already covers the point; use memory_supersede for stale facts \
and rely on reinforcement for exact duplicates instead of storing paraphrases. Do not store transient observations, generic task chatter, \
or facts better represented as Flynt/project documents.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "required": ["section", "content"],
                    "properties": {
                        "applicability": omegon_memory::applicability::constraints_schema(),
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
                capabilities: vec![omegon_traits::ToolCapability::StateChanging],
            },
            ToolDefinition {
                name: crate::tool_registry::memory::MEMORY_RECALL.into(),
                label: "memory_recall".into(),
                description: "Search project memory for facts relevant to a query. Returns ranked results. \
Use this PROACTIVELY at the start of any non-trivial task to surface relevant context before acting. \
Also use it when you notice a gap — if you're unsure whether something was already decided, search first.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "required": ["query"],
                    "properties": {
                        "context": omegon_memory::applicability::context_schema(),
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
                capabilities: vec![
                    omegon_traits::ToolCapability::Orientation,
                    omegon_traits::ToolCapability::BroadOrientation,
                ],
            },
            ToolDefinition {
                name: crate::tool_registry::memory::MEMORY_QUERY.into(),
                label: "memory_query".into(),
                description: "Read a capped inventory of active facts from Omegon runtime memory. This is broad and can be noisy in mature projects; prefer memory_recall for targeted retrieval and use memory_query only for inventory, hygiene, or debugging.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {}
                }),
                capabilities: vec![omegon_traits::ToolCapability::Orientation],
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
                capabilities: vec![omegon_traits::ToolCapability::StateChanging],
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
                capabilities: vec![omegon_traits::ToolCapability::StateChanging],
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
                capabilities: vec![omegon_traits::ToolCapability::StateChanging],
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
                capabilities: vec![omegon_traits::ToolCapability::StateChanging],
            },
            ToolDefinition {
                name: crate::tool_registry::memory::MEMORY_RELEASE.into(),
                label: "memory_release".into(),
                description: "Clear working memory — release all pinned facts.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {}
                }),
                capabilities: vec![omegon_traits::ToolCapability::StateChanging],
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
                capabilities: vec![omegon_traits::ToolCapability::Orientation],
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
                capabilities: vec![omegon_traits::ToolCapability::StateChanging],
            },
            ToolDefinition {
                name: crate::tool_registry::memory::MEMORY_SEARCH_ARCHIVE.into(),
                label: "memory_search_archive".into(),
                description: "Search archived, dormant, and superseded project facts. Results are historical evidence, not current guidance.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "required": ["query"],
                    "properties": {
                        "context": omegon_memory::applicability::context_schema(),
                        "query": {
                            "type": "string",
                            "description": "Search terms"
                        }
                    }
                }),
                capabilities: vec![omegon_traits::ToolCapability::Orientation],
            },
            ToolDefinition {
                name: crate::tool_registry::memory::MEMORY_INGEST_LIFECYCLE.into(),
                label: "memory_ingest_lifecycle".into(),
                description: "Ingest lifecycle conclusions. Inferred summaries remain pending. Explicit conclusions require a matching decided design artifact or baseline/archived spec; provide artifact_ref_type, artifact_ref_path, and artifact_ref_sub.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "required": ["source_kind", "authority", "section", "content"],
                    "properties": {
                        "source_kind": { "type": "string" },
                        "authority": { "type": "string", "enum": ["explicit", "inferred"] },
                        "section": { "type": "string" },
                        "content": { "type": "string" },
                        "supersedes": { "type": "string" },
                        "supersedes_version": { "type": "integer", "minimum": 0, "description": "Required with supersedes; expected target fact version." },
                        "artifact_ref_type": { "type": "string" },
                        "artifact_ref_path": { "type": "string" },
                        "artifact_ref_sub": { "type": "string", "description": "Decision/requirement title, or Implementation Notes/Constraints for an exact constraint. Explicit decision/spec content uses Title: statement." }
                    }
                }),
                capabilities: vec![omegon_traits::ToolCapability::StateChanging],
            },
            ToolDefinition {
                name:crate::tool_registry::memory::MEMORY_SELECTION.into(),label:"memory_selection".into(),
                description:"Inspect the last ambient memory selection: selected handles, exclusion reasons, token accounting, and degradation. Explicit request_context packs return their own selection reports.".into(),
                parameters:serde_json::json!({"type":"object","properties":{},"additionalProperties":false}),capabilities:vec![omegon_traits::ToolCapability::Orientation],
            },
            ToolDefinition {
                name:crate::tool_registry::memory::MEMORY_SET_APPLICABILITY.into(),label:"memory_set_applicability".into(),
                description:"Record applicability constraints for one fact at its expected version. Does not reinforce, confirm, or change lifecycle status. An empty constraint object records unknown applicability.".into(),
                parameters:serde_json::json!({"type":"object","required":["fact_id","expected_version","applicability"],"additionalProperties":false,"properties":{
                    "fact_id":{"type":"string"},"expected_version":{"type":"integer","minimum":0},"applicability":omegon_memory::applicability::constraints_schema()}}),
                capabilities:vec![omegon_traits::ToolCapability::StateChanging],
            },
            ToolDefinition {
                name:crate::tool_registry::memory::MEMORY_INSPECT.into(),label:"memory_inspect".into(),
                description:"Read-only inspection of one active, historical, or pending fact, its recorded provenance, and bounded artifact availability. Does not reinforce or reactivate memory.".into(),
                parameters:serde_json::json!({"type":"object","required":["fact_id"],"additionalProperties":false,"properties":{"fact_id":{"type":"string"}}}),
                capabilities:vec![omegon_traits::ToolCapability::Orientation],
            },
            ToolDefinition {
                name: crate::tool_registry::memory::MEMORY_CONFIRM.into(),label:"memory_confirm".into(),
                description:"Request operator review of a pending lifecycle candidate. Only an interactive operator response can confirm it; no approval flag is accepted.".into(),
                parameters:serde_json::json!({"type":"object","required":["candidate_id"],"additionalProperties":false,"properties":{"candidate_id":{"type":"string"}}}),
                capabilities:vec![omegon_traits::ToolCapability::StateChanging],
            },
        ]
    }

    async fn execute(
        &self,
        tool_name: &str,
        call_id: &str,
        args: Value,
        cancel: tokio_util::sync::CancellationToken,
    ) -> anyhow::Result<ToolResult> {
        match tool_name {
            crate::tool_registry::memory::MEMORY_STORE => {
                let content = args["content"].as_str().unwrap_or("").to_string();
                let section_str = args["section"].as_str().unwrap_or("Architecture");
                let section = Self::parse_section_arg(section_str)?;

                let source = args["source"].as_str().unwrap_or("manual");
                let request = StoreFact {
                    mind: self.mind.clone(),
                    content: content.clone(),
                    section,
                    decay_profile: DecayProfileName::Standard,
                    source: Some(source.into()),
                };
                let mutation = match omegon_memory::applicability::constraints_arg(&args)? {
                    Some(constraints) => MemoryMutation::StoreApplicableFact {
                        request,
                        constraints: Box::new(constraints),
                    },
                    None => MemoryMutation::StoreFact { request },
                };
                let outcome = self
                    .apply_mutation(
                        self.tool_operation_id(call_id, "store")?,
                        mutation,
                        cancel.clone(),
                    )
                    .await?;
                let MemoryMutationEffect::FactStored {
                    fact_id,
                    version,
                    action,
                } = outcome.effect
                else {
                    anyhow::bail!("managed memory returned an unexpected store effect");
                };
                // A replay is rendered from its durable receipt even if the
                // fact has since been archived or superseded.
                if !outcome.replayed
                    && matches!(action, StoreAction::Stored)
                    && let Some(ref embed_svc) = self.embed_service
                {
                    persist_embedding(
                        embed_svc,
                        &self.memory_binding,
                        FactPrecondition {
                            id: fact_id.clone(),
                            expected_version: version,
                        },
                        content.clone(),
                        self.tool_operation_id(call_id, &format!("embedding:{fact_id}"))?,
                        cancel.clone(),
                    )
                    .await;
                }

                let msg = match action {
                    StoreAction::Stored => format!("Stored in {}: {}", section_str, content),
                    StoreAction::Reinforced => format!("Reinforced existing fact: {}", content),
                    StoreAction::Deduplicated => "Duplicate — fact already exists".to_string(),
                };
                self.pending_status_refresh.store(true, Ordering::Relaxed);
                self.context_dirty.store(true, Ordering::Relaxed);
                self.refresh_status().await;
                Ok(ToolResult {
                    content: vec![ContentBlock::Text { text: msg }],
                    details: serde_json::json!({ "id": fact_id, "action": format!("{:?}", action) }),
                })
            }
            crate::tool_registry::memory::MEMORY_RECALL => {
                let query = args["query"].as_str().unwrap_or("").trim().to_string();
                if query.is_empty() {
                    return Ok(ToolResult {
                        content: vec![ContentBlock::Text {
                            text: "memory_recall requires a non-empty query.".into(),
                        }],
                        details: serde_json::json!({ "is_error": true }),
                    });
                }
                let k = usize::try_from(args["k"].as_u64().unwrap_or(10))
                    .unwrap_or(10_000)
                    .min(10_000);
                let fetch_k = k.saturating_mul(2).min(10_000); // over-fetch for RRF merge headroom
                let filter = omegon_memory::SearchFilter {
                    context: omegon_memory::applicability::context_arg(&args)?,
                    section: args["section"]
                        .as_str()
                        .map(Self::parse_section_arg)
                        .transpose()?,
                    ..Default::default()
                };

                let query_vector = if let Some(ref embed_svc) = self.embed_service {
                    tokio::select! {
                        _ = cancel.cancelled() => return Err(MemoryFeatureInvokeError(ManagedServiceCallError::Cancelled).into()),
                        result = tokio::time::timeout(std::time::Duration::from_secs(30), embed_svc.embed_identified(&query)) => match result {
                            Ok(Ok(query_embedding)) => Some(query_embedding),
                            _ => None,
                        },
                    }
                } else {
                    None
                };
                let (query_space, query_vector) = match query_vector {
                    Some(embedding) => (Some(embedding.space), Some(embedding.values)),
                    None => (None, None),
                };
                let crate::memory_service::MemoryPayloadV1::RecallReport(report) = self
                    .invoke(crate::memory_service::MemoryRequestV1::HybridSearch {
                        scope: crate::memory_service::MemoryScopeV1::Project,
                        mind: self.mind.clone(),
                        query,
                        query_vector,
                        query_space,
                        include_diagnostics: true,
                        filter,
                        limit: k,
                        fetch_limit: fetch_k,
                        min_similarity: 0.1,
                        cancellation: cancel,
                    })
                    .await?
                else {
                    anyhow::bail!("managed memory returned an unexpected search response");
                };

                let results = report.results;
                let diagnostic =
                    omegon_memory::renderer::vector_diagnostic_label(&report.diagnostics);

                if results.is_empty() {
                    return Ok(ToolResult {
                        content: vec![ContentBlock::Text {
                            text: format!("No matching facts found.\n{diagnostic}"),
                        }],
                        details: serde_json::json!({"count":0,"vector":report.diagnostics}),
                    });
                }

                let mut lines = vec![diagnostic];
                for (i, sf) in results.iter().enumerate() {
                    let section = serde_json::to_string(&sf.fact.section).unwrap_or_default();
                    let section = section.trim_matches('"');
                    let content = if sf.fact.content.len() > 200 {
                        crate::util::truncate(&sf.fact.content, 197)
                    } else {
                        sf.fact.content.clone()
                    };
                    lines.push(format!(
                        "{}. [{}] ({}, {}) {}",
                        i + 1,
                        sf.fact.id,
                        section,
                        omegon_memory::renderer::recall_score_label(sf),
                        content,
                    ));
                }
                Ok(ToolResult {
                    content: vec![ContentBlock::Text {
                        text: lines.join("\n"),
                    }],
                    details: serde_json::json!({ "count": results.len(), "vector": report.diagnostics,
                        "scores":results.iter().map(|result| serde_json::json!({"id":result.fact.id,"scores":result.scores,"graph_evidence":result.graph_evidence})).collect::<Vec<_>>() }),
                })
            }
            crate::tool_registry::memory::MEMORY_QUERY => {
                let mut facts = Vec::new();
                let mut cursor = None;
                loop {
                    let payload = self
                        .invoke(crate::memory_service::MemoryRequestV1::ListFactsPage {
                            scope: crate::memory_service::MemoryScopeV1::Project,
                            mind: self.mind.clone(),
                            filter: omegon_memory::FactFilter::default(),
                            limit: 1_000,
                            cursor,
                            cancellation: cancel.clone(),
                        })
                        .await?;
                    let crate::memory_service::MemoryPayloadV1::FactPage(page) = payload else {
                        anyhow::bail!("managed memory returned an unexpected fact page");
                    };
                    facts.extend(page.facts);
                    cursor = page.next_cursor;
                    if cursor.is_none() {
                        break;
                    }
                }

                if facts.is_empty() {
                    return Ok(ToolResult {
                        content: vec![ContentBlock::Text {
                            text: "No facts in memory.".into(),
                        }],
                        details: serde_json::json!({ "count": 0 }),
                    });
                }

                // Group by section. Large stores are inventory-only to avoid turning memory_query
                // into a noisy context dump; use memory_recall for targeted retrieval.
                let mut sections: std::collections::BTreeMap<String, Vec<&omegon_memory::Fact>> =
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

                let large_store_threshold = 200;
                if facts.len() > large_store_threshold {
                    lines.push(format!(
                        "Large memory store detected (>{large_store_threshold} facts). Showing section counts only; use memory_recall for targeted retrieval."
                    ));
                    lines.push(String::new());
                    for (section, section_facts) in &sections {
                        lines.push(format!("## {} ({} facts)", section, section_facts.len()));
                    }
                } else {
                    let max_per_section = 8;
                    for (section, section_facts) in &sections {
                        lines.push(format!("## {} ({} facts)", section, section_facts.len()));
                        for fact in section_facts.iter().take(max_per_section) {
                            // Truncate long facts to keep output manageable
                            let content = if fact.content.len() > 120 {
                                crate::util::truncate(&fact.content, 117)
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
                }

                Ok(ToolResult {
                    content: vec![ContentBlock::Text {
                        text: lines.join("\n"),
                    }],
                    details: serde_json::json!({ "count": facts.len(), "sections": sections.len(), "inventory_only": facts.len() > large_store_threshold }),
                })
            }
            crate::tool_registry::memory::MEMORY_ARCHIVE => {
                let ids: Vec<String> = args["fact_ids"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();
                let outcome = self
                    .invoke(crate::memory_service::MemoryRequestV1::ApplyToolMutation {
                        scope: crate::memory_service::MemoryScopeV1::Project,
                        operation_id: self.tool_operation_id(call_id, "archive")?,
                        mutation: crate::memory_service::MemoryToolMutationV1::Archive {
                            mind: self.mind.clone(),
                            fact_ids: ids,
                        },
                        cancellation: cancel.clone(),
                    })
                    .await?;
                let crate::memory_service::MemoryPayloadV1::Mutation(outcome) = outcome else {
                    anyhow::bail!("managed memory returned an unexpected archive effect");
                };
                let MemoryMutationEffect::FactsTransitioned { facts, .. } = outcome.effect else {
                    anyhow::bail!("managed memory returned an unexpected archive effect");
                };
                let count = facts.len();
                self.pending_status_refresh.store(true, Ordering::Relaxed);
                self.context_dirty.store(true, Ordering::Relaxed);
                self.refresh_status().await;
                Ok(ToolResult {
                    content: vec![ContentBlock::Text {
                        text: format!("Archived {count} fact(s)."),
                    }],
                    details: serde_json::json!({ "archived": count }),
                })
            }
            crate::tool_registry::memory::MEMORY_SUPERSEDE => {
                let fact_id = args["fact_id"].as_str().unwrap_or("").to_string();
                let content = args["content"].as_str().unwrap_or("").to_string();
                let section_str = args["section"].as_str().unwrap_or("Architecture");
                let section = Self::parse_section_arg(section_str)?;

                let outcome = self
                    .invoke(crate::memory_service::MemoryRequestV1::ApplyToolMutation {
                        scope: crate::memory_service::MemoryScopeV1::Project,
                        operation_id: self.tool_operation_id(call_id, "supersede")?,
                        mutation: crate::memory_service::MemoryToolMutationV1::Supersede {
                            fact_id: fact_id.clone(),
                            replacement: StoreFact {
                                mind: self.mind.clone(),
                                content: content.clone(),
                                section,
                                decay_profile: DecayProfileName::Standard,
                                source: Some("manual".into()),
                            },
                        },
                        cancellation: cancel.clone(),
                    })
                    .await?;
                let crate::memory_service::MemoryPayloadV1::Mutation(outcome) = outcome else {
                    anyhow::bail!("managed memory returned an unexpected supersede effect");
                };
                let MemoryMutationEffect::FactSuperseded { replacement, .. } = outcome.effect
                else {
                    anyhow::bail!("managed memory returned an unexpected supersede effect");
                };

                // Replays are rendered entirely from the durable receipt. The replacement may
                // have transitioned again since the original supersede committed.
                if !outcome.replayed
                    && let Some(ref embed_svc) = self.embed_service
                {
                    let new_fact = self
                        .get_fact(replacement.id.clone(), cancel.clone())
                        .await?
                        .ok_or_else(|| anyhow::anyhow!("replacement memory fact is unavailable"))?;
                    persist_embedding(
                        embed_svc,
                        &self.memory_binding,
                        replacement.clone(),
                        content,
                        self.tool_operation_id(call_id, &format!("embedding:{}", new_fact.id))?,
                        cancel.clone(),
                    )
                    .await;
                }

                self.pending_status_refresh.store(true, Ordering::Relaxed);
                self.context_dirty.store(true, Ordering::Relaxed);
                self.refresh_status().await;
                Ok(ToolResult {
                    content: vec![ContentBlock::Text {
                        text: format!("Superseded {} → new fact {}", fact_id, replacement.id),
                    }],
                    details: serde_json::json!({ "old_id": fact_id, "new_id": replacement.id }),
                })
            }
            crate::tool_registry::memory::MEMORY_CONNECT => {
                let source_id = args["source_fact_id"].as_str().unwrap_or("").to_string();
                let target_id = args["target_fact_id"].as_str().unwrap_or("").to_string();
                let relation = args["relation"].as_str().unwrap_or("").to_string();
                let outcome = self
                    .apply_mutation(
                        self.tool_operation_id(call_id, "connect")?,
                        MemoryMutation::CreateEdge {
                            mind: self.mind.clone(),
                            request: CreateEdge {
                                source_id: source_id.clone(),
                                target_id: target_id.clone(),
                                relation: relation.clone(),
                                description: args["description"].as_str().map(String::from),
                            },
                        },
                        cancel.clone(),
                    )
                    .await?;
                let MemoryMutationEffect::EdgeCreated { edge_id } = outcome.effect else {
                    anyhow::bail!("managed memory returned an unexpected edge effect");
                };
                self.pending_status_refresh.store(true, Ordering::Relaxed);
                self.context_dirty.store(true, Ordering::Relaxed);
                self.refresh_status().await;
                Ok(ToolResult {
                    content: vec![ContentBlock::Text {
                        text: format!("Connected {} → {} ({})", source_id, target_id, relation),
                    }],
                    details: serde_json::json!({ "edge_id": edge_id }),
                })
            }
            crate::tool_registry::memory::MEMORY_FOCUS => {
                let ids: Vec<String> = args["fact_ids"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();
                let count = ids.len();
                {
                    let mut current = self.working_memory.lock().unwrap();
                    if current.len().saturating_add(count) > crate::memory_service::MAX_CONTEXT_PINS
                    {
                        anyhow::bail!(
                            "memory focus exceeds the {} pin limit",
                            crate::memory_service::MAX_CONTEXT_PINS
                        );
                    }
                    current.extend(ids);
                }
                self.pending_status_refresh.store(true, Ordering::Relaxed);
                self.context_dirty.store(true, Ordering::Relaxed);
                self.refresh_status().await;
                Ok(ToolResult {
                    content: vec![ContentBlock::Text {
                        text: format!("Pinned {count} fact(s) to working memory."),
                    }],
                    details: Value::Null,
                })
            }
            crate::tool_registry::memory::MEMORY_RELEASE => {
                self.working_memory.lock().unwrap().clear();
                self.pending_status_refresh.store(true, Ordering::Relaxed);
                self.context_dirty.store(true, Ordering::Relaxed);
                self.refresh_status().await;
                Ok(ToolResult {
                    content: vec![ContentBlock::Text {
                        text: "Working memory cleared.".into(),
                    }],
                    details: Value::Null,
                })
            }
            crate::tool_registry::memory::MEMORY_EPISODES => {
                let query = args["query"].as_str().unwrap_or("").to_string();
                let k = usize::try_from(args["k"].as_u64().unwrap_or(5))
                    .unwrap_or(10_000)
                    .min(10_000);
                let crate::memory_service::MemoryPayloadV1::Episodes(episodes) = self
                    .invoke(crate::memory_service::MemoryRequestV1::SearchEpisodes {
                        scope: crate::memory_service::MemoryScopeV1::Project,
                        mind: self.mind.clone(),
                        query,
                        limit: k,
                        cancellation: cancel,
                    })
                    .await?
                else {
                    anyhow::bail!("managed memory returned an unexpected episode response");
                };
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
                let crate::memory_service::MemoryPayloadV1::ScoredFacts(results) = self
                    .invoke(crate::memory_service::MemoryRequestV1::FtsSearch {
                        scope: crate::memory_service::MemoryScopeV1::Project,
                        mind: self.mind.clone(),
                        query,
                        limit: 20,
                        filter: omegon_memory::SearchFilter {
                            context: omegon_memory::applicability::context_arg(&args)?,
                            intent: omegon_memory::SearchIntent::Historical,
                            section: None,
                        },
                        cancellation: cancel,
                    })
                    .await?
                else {
                    anyhow::bail!("managed memory returned an unexpected archive search response");
                };
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
                        omegon_memory::renderer::recall_score_label(scored)
                    ));
                }
                Ok(ToolResult {
                    content: vec![ContentBlock::Text {
                        text: lines.join("\n"),
                    }],
                    details: Value::Null,
                })
            }
            crate::tool_registry::memory::MEMORY_SELECTION => {
                let report = self.last_selection.lock().unwrap().clone();
                Ok(ToolResult {
                    content: vec![ContentBlock::Text {
                        text: serde_json::to_string_pretty(&report)?,
                    }],
                    details: serde_json::to_value(report)?,
                })
            }
            crate::tool_registry::memory::MEMORY_SET_APPLICABILITY => {
                let id = args["fact_id"]
                    .as_str()
                    .filter(|id| !id.is_empty())
                    .ok_or_else(|| anyhow::anyhow!("fact_id is required"))?
                    .to_string();
                let version = args["expected_version"]
                    .as_u64()
                    .ok_or_else(|| anyhow::anyhow!("expected_version is required"))?;
                let constraints = omegon_memory::applicability::constraints_arg(&args)?
                    .ok_or_else(|| anyhow::anyhow!("applicability is required"))?;
                let record = self
                    .invoke(crate::memory_service::MemoryRequestV1::GetFactRecord {
                        scope: crate::memory_service::MemoryScopeV1::Project,
                        mind: self.mind.clone(),
                        id: id.clone(),
                        cancellation: cancel.clone(),
                    })
                    .await?;
                if !matches!(record,crate::memory_service::MemoryPayloadV1::Fact(record) if record.is_some())
                {
                    anyhow::bail!("fact not found in this mind");
                }
                let outcome = self
                    .apply_mutation(
                        self.tool_operation_id(call_id, "applicability")?,
                        MemoryMutation::SetFactApplicability {
                            fact: FactPrecondition {
                                id,
                                expected_version: version,
                            },
                            constraints: Box::new(constraints),
                        },
                        cancel,
                    )
                    .await?;
                self.context_dirty.store(true, Ordering::Relaxed);
                self.pending_status_refresh.store(true, Ordering::Relaxed);
                self.refresh_status().await;
                Ok(ToolResult {content:vec![ContentBlock::Text {text:"Recorded applicability constraints without reinforcement or lifecycle change.".into()}],details:serde_json::to_value(outcome)?})
            }
            crate::tool_registry::memory::MEMORY_INSPECT => {
                let id = omegon_memory::inspection::fact_id(&args)?.to_string();
                let payload = self
                    .invoke(crate::memory_service::MemoryRequestV1::GetFactRecord {
                        scope: crate::memory_service::MemoryScopeV1::Project,
                        mind: self.mind.clone(),
                        id,
                        cancellation: cancel.clone(),
                    })
                    .await?;
                let crate::memory_service::MemoryPayloadV1::Fact(fact) = payload else {
                    anyhow::bail!("unexpected fact inspection response");
                };
                let fact = (*fact).ok_or_else(|| anyhow::anyhow!("fact not found in this mind"))?;
                let inspection = omegon_memory::FactInspection::from_fact(&fact);
                let root = self.status_root.clone();
                let inspection = tokio::select! {
                    _ = cancel.cancelled() => return Err(MemoryFeatureInvokeError(ManagedServiceCallError::Cancelled).into()),
                    result=tokio::task::spawn_blocking(move || lifecycle::inspect_source(&root,inspection)) => result?,
                };
                Ok(ToolResult {
                    content: vec![ContentBlock::Text {
                        text: omegon_memory::inspection::render(&inspection)?,
                    }],
                    details: serde_json::to_value(inspection)?,
                })
            }
            crate::tool_registry::memory::MEMORY_CONFIRM => {
                let object = args
                    .as_object()
                    .ok_or_else(|| anyhow::anyhow!("invalid confirmation request"))?;
                if object.len() != 1 {
                    anyhow::bail!(
                        "confirmation accepts only candidate_id; approval is supplied by the operator surface"
                    );
                }
                let id = args["candidate_id"]
                    .as_str()
                    .filter(|id| !id.is_empty())
                    .ok_or_else(|| anyhow::anyhow!("candidate_id is required"))?
                    .to_string();
                let request_id = self.tool_operation_id(call_id, "confirmation")?;
                let session_id = self
                    .session_id
                    .lock()
                    .unwrap()
                    .clone()
                    .ok_or_else(|| anyhow::anyhow!("memory session identity is unavailable"))?;
                let payload = self
                    .invoke(crate::memory_service::MemoryRequestV1::GetPendingFact {
                        scope: crate::memory_service::MemoryScopeV1::Project,
                        mind: self.mind.clone(),
                        id: id.clone(),
                        cancellation: cancel.clone(),
                    })
                    .await?;
                let crate::memory_service::MemoryPayloadV1::Fact(candidate) = payload else {
                    anyhow::bail!("unexpected candidate response");
                };
                let Some(candidate) = *candidate else {
                    if let Some(fact) = self.get_fact(id.clone(), cancel).await?
                        && fact.mind == self.mind
                        && fact
                            .lifecycle_inference
                            .as_ref()
                            .and_then(|inference| inference.confirmation.as_ref())
                            .is_some_and(|confirmation| confirmation.request_id == request_id)
                    {
                        return Ok(ToolResult {
                            content: vec![ContentBlock::Text {
                                text: format!(
                                    "Candidate [{id}] was already confirmed by this operator request."
                                ),
                            }],
                            details: serde_json::json!({"id":id,"status":"active","replayed":true}),
                        });
                    }
                    anyhow::bail!("pending candidate not found in this mind");
                };
                if candidate.content.len() > 65_536
                    || candidate.id.len() > 2048
                    || candidate.mind.len() > 2048
                {
                    anyhow::bail!("candidate exceeds operator review bounds");
                }
                let inference = candidate
                    .lifecycle_inference
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("candidate attribution is unavailable"))?;
                let mut correction = String::new();
                let supersedes = if let Some(id) = &inference.proposed_supersedes {
                    let target = self
                        .get_fact(id.clone(), cancel.clone())
                        .await?
                        .filter(|fact| fact.mind == self.mind)
                        .ok_or_else(|| {
                            anyhow::anyhow!("correction target is not active in this mind")
                        })?;
                    if target.content.len() > 65_536 || target.id.len() > 2048 {
                        anyhow::bail!("correction target exceeds operator review bounds");
                    }
                    correction = format!(
                        "\nReplace [{}] version {}:\n{}\nTarget applicability: {}",
                        confirmation::readable(&target.id),
                        target.version,
                        confirmation::readable(&target.content),
                        serde_json::to_string(&target.applicability)?
                    );
                    Some(FactPrecondition {
                        id: target.id,
                        expected_version: target.version,
                    })
                } else {
                    None
                };
                let attribution = serde_json::to_string(inference)?;
                let prompt = format!(
                    "Confirm this inference as project memory? This does not verify execution outcomes.\nMind: {}\nCandidate [{}] version {}:\n{}\nDeclared attribution: {}\nCandidate applicability: {}{}",
                    confirmation::readable(&candidate.mind),
                    confirmation::readable(&candidate.id),
                    candidate.version,
                    confirmation::readable(&candidate.content),
                    attribution,
                    serde_json::to_string(&candidate.applicability)?,
                    correction
                );
                return Err(confirmation::MemoryConfirmationRequired {
                    prompt,
                    request: confirmation::ConfirmationRequest {
                        snapshot_hash: omegon_memory::lifecycle::candidate_snapshot_hash(
                            &candidate,
                        )?,
                        candidate: FactPrecondition {
                            id: candidate.id,
                            expected_version: candidate.version,
                        },
                        session_id,
                        request_id,
                        supersedes,
                    },
                }
                .into());
            }
            crate::tool_registry::memory::MEMORY_APPLY_CONFIRMATION => {
                let request: confirmation::ConfirmationRequest =
                    serde_json::from_value(args["request"].clone())?;
                let surface: omegon_memory::ConfirmationSurface =
                    serde_json::from_value(args["surface"].clone())?;
                let outcome = self
                    .apply_mutation(
                        request.request_id.clone(),
                        MemoryMutation::ConfirmLifecycleCandidate {
                            candidate: request.candidate,
                            snapshot_hash: request.snapshot_hash,
                            session_id: request.session_id,
                            request_id: request.request_id,
                            supersedes: request.supersedes,
                            surface,
                        },
                        cancel,
                    )
                    .await?;
                self.context_dirty.store(true, Ordering::Relaxed);
                self.pending_status_refresh.store(true, Ordering::Relaxed);
                self.refresh_status().await;
                Ok(ToolResult {
                    content: vec![ContentBlock::Text {
                        text: "Operator-confirmed candidate admitted to active memory.".into(),
                    }],
                    details: serde_json::json!({"status":"active","outcome":outcome}),
                })
            }
            crate::tool_registry::memory::MEMORY_INGEST_LIFECYCLE => {
                // Lifecycle fact ingestion — stores with source metadata
                let content = args["content"].as_str().unwrap_or("").to_string();
                let section_str = args["section"].as_str().unwrap_or("Architecture");
                let section = Self::parse_section_arg(section_str)?;
                let authority = args["authority"].as_str().unwrap_or("inferred");
                let source_kind = args["source_kind"].as_str().unwrap_or("unknown");

                if !matches!(authority, "explicit" | "inferred") {
                    anyhow::bail!("invalid lifecycle authority");
                }
                if authority == "inferred" {
                    let reference = |key: &str| -> anyhow::Result<Option<String>> {
                        match args.get(key) {
                            None | Some(Value::Null) => Ok(None),
                            Some(Value::String(value)) => Ok(Some(value.clone())),
                            _ => anyhow::bail!("invalid lifecycle reference field {key}"),
                        }
                    };
                    let outcome = self
                        .apply_mutation(
                            self.tool_operation_id(call_id, "lifecycle")?,
                            MemoryMutation::StoreLifecycleInference {
                                request: StoreFact {
                                    mind: self.mind.clone(),
                                    content,
                                    section,
                                    decay_profile: DecayProfileName::Standard,
                                    source: Some(format!("lifecycle:{source_kind}")),
                                },
                                inference: Box::new(omegon_memory::LifecycleInference {
                                    confirmation: None,
                                    source_kind: source_kind.into(),
                                    artifact_ref_type: reference("artifact_ref_type")?,
                                    artifact_ref_path: reference("artifact_ref_path")?,
                                    artifact_ref_sub: reference("artifact_ref_sub")?,
                                    proposed_supersedes: reference("supersedes")?,
                                }),
                            },
                            cancel,
                        )
                        .await?;
                    let MemoryMutationEffect::FactStored { fact_id, .. } = outcome.effect else {
                        anyhow::bail!(
                            "managed memory returned an unexpected lifecycle candidate effect"
                        );
                    };
                    return Ok(ToolResult {content:vec![ContentBlock::Text {
                        text:"Retained lifecycle inference pending confirmation; excluded from established knowledge.".into(),
                    }],details:serde_json::json!({"id":fact_id,"status":"pending","replayed":outcome.replayed})});
                }

                let path = args["artifact_ref_path"]
                    .as_str()
                    .ok_or_else(|| {
                        anyhow::anyhow!("explicit lifecycle admission requires artifact_ref_path")
                    })?
                    .to_string();
                let reference_type = args["artifact_ref_type"]
                    .as_str()
                    .ok_or_else(|| {
                        anyhow::anyhow!("explicit lifecycle admission requires artifact_ref_type")
                    })?
                    .to_string();
                let sub = args["artifact_ref_sub"]
                    .as_str()
                    .ok_or_else(|| {
                        anyhow::anyhow!("explicit lifecycle admission requires artifact_ref_sub")
                    })?
                    .to_string();
                let supersedes = match args.get("supersedes") {
                    None | Some(Value::Null) => None,
                    Some(Value::String(id)) if !id.trim().is_empty() => Some(FactPrecondition {
                        id: id.clone(),
                        expected_version: args["supersedes_version"].as_u64().ok_or_else(|| {
                            anyhow::anyhow!("supersedes requires supersedes_version")
                        })?,
                    }),
                    _ => anyhow::bail!("invalid supersedes target"),
                };
                let root = self.status_root.clone();
                let source_kind_owned = source_kind.to_string();
                let section_for_validation = section.clone();
                let validated = tokio::select! {
                    _ = cancel.cancelled() => return Err(MemoryFeatureInvokeError(ManagedServiceCallError::Cancelled).into()),
                    result = tokio::task::spawn_blocking(move || lifecycle::validate(&root,&path,&source_kind_owned,&reference_type,&sub,&section_for_validation,&content)) => result??,
                };
                let content = validated.content;
                let conclusion_source = validated.source;
                let outcome = self
                    .apply_mutation(
                        self.tool_operation_id(call_id, "lifecycle")?,
                        MemoryMutation::StoreLifecycleConclusion {
                            request: StoreFact {
                                mind: self.mind.clone(),
                                content: content.clone(),
                                section,
                                decay_profile: DecayProfileName::Standard,
                                source: Some(format!("lifecycle:{source_kind}")),
                            },
                            source: Box::new(conclusion_source.clone()),
                            supersedes,
                        },
                        cancel.clone(),
                    )
                    .await?;
                let (fact_id, version, action) = match outcome.effect {
                    MemoryMutationEffect::FactStored {
                        fact_id,
                        version,
                        action,
                    } => (fact_id, version, action),
                    MemoryMutationEffect::FactSuperseded { replacement, .. } => (
                        replacement.id,
                        replacement.expected_version,
                        StoreAction::Stored,
                    ),
                    _ => anyhow::bail!(
                        "managed memory returned an unexpected lifecycle store effect"
                    ),
                };

                // Auto-embed newly ingested lifecycle facts
                if matches!(action, StoreAction::Stored)
                    && let Some(ref embed_svc) = self.embed_service
                {
                    persist_embedding(
                        embed_svc,
                        &self.memory_binding,
                        FactPrecondition {
                            id: fact_id.clone(),
                            expected_version: version,
                        },
                        content.clone(),
                        self.tool_operation_id(call_id, &format!("embedding:{fact_id}"))?,
                        cancel.clone(),
                    )
                    .await;
                }

                let msg = match action {
                    StoreAction::Stored => format!(
                        "Ingested ({authority}/{source_kind}): {}",
                        content.chars().take(80).collect::<String>()
                    ),
                    StoreAction::Reinforced => "Reinforced lifecycle fact".to_string(),
                    StoreAction::Deduplicated => {
                        "Duplicate lifecycle fact — already exists".to_string()
                    }
                };
                self.pending_status_refresh.store(true, Ordering::Relaxed);
                self.context_dirty.store(true, Ordering::Relaxed);
                self.refresh_status().await;
                Ok(ToolResult {
                    content: vec![ContentBlock::Text { text: msg }],
                    details: serde_json::json!({ "action": format!("{:?}", action), "id": fact_id, "version":version,
                        "status":"active", "source":conclusion_source, "replayed":outcome.replayed }),
                })
            }
            _ => anyhow::bail!("Unknown memory tool: {tool_name}"),
        }
    }

    fn on_event(&mut self, event: &BusEvent) -> Vec<BusRequest> {
        match event {
            BusEvent::SessionStart { session_id, .. } => {
                *self.session_id.lock().unwrap() = Some(session_id.clone());
                Vec::new()
            }
            BusEvent::ToolEnd { name, is_error, .. }
                if !is_error
                    && matches!(
                        name.as_str(),
                        crate::tool_registry::memory::MEMORY_STORE
                            | crate::tool_registry::memory::MEMORY_ARCHIVE
                            | crate::tool_registry::memory::MEMORY_SUPERSEDE
                            | crate::tool_registry::memory::MEMORY_CONNECT
                            | crate::tool_registry::memory::MEMORY_FOCUS
                            | crate::tool_registry::memory::MEMORY_RELEASE
                            | crate::tool_registry::memory::MEMORY_INGEST_LIFECYCLE
                            | crate::tool_registry::memory::MEMORY_CONFIRM
                            | crate::tool_registry::memory::MEMORY_APPLY_CONFIRMATION
                            | crate::tool_registry::memory::MEMORY_SET_APPLICABILITY
                    )
                    && self.pending_status_refresh.swap(false, Ordering::Relaxed) =>
            {
                vec![BusRequest::RefreshHarnessStatus]
            }

            BusEvent::SessionEnd {
                turns,
                tool_calls,
                duration_secs,
                ..
            } if *turns > 0 && self.memory_binding.available() => {
                let mind = self.mind.clone();
                let memory_binding = self.memory_binding.clone();
                let extractor = self.extractor.clone();
                let session_binding = self.session_binding.clone();
                let target = session_binding
                    .as_ref()
                    .and_then(|binding| binding.snapshot());
                let Some(session_id) = self.session_id.lock().unwrap().clone() else {
                    self.session_end_tasks
                        .lock()
                        .unwrap()
                        .failures
                        .push("session-end event had no stable session identity".into());
                    return vec![];
                };
                let status_root = self.status_root.clone();
                let (t, tc, dur) = (*turns, *tool_calls, *duration_secs);
                let cancellation = tokio_util::sync::CancellationToken::new();
                let worker_cancellation = cancellation.clone();
                let handle = std::thread::Builder::new()
                    .name(format!("memory-session-end-{session_id}"))
                    .spawn(move || {
                        let runtime = tokio::runtime::Builder::new_current_thread()
                            .enable_all()
                            .build()
                            .map_err(|error| error.to_string())?;
                        runtime.block_on(async {
                            tokio::select! {
                                _ = worker_cancellation.cancelled() => {}
                                _ = async {
                                    run_session_end_pipeline(SessionEndPipelineInput {
                                        mind,
                                        memory_binding,
                                        extractor,
                                        evidence: formation::capture(session_binding.as_ref(), target.as_ref(), &session_id),
                                        session_id,
                                        status_root,
                                        turns: t,
                                        tool_calls: tc,
                                        duration_secs: dur,
                                    })
                                    .await;
                                } => {}
                            }
                        });
                        Ok(())
                    });
                let mut tasks = self.session_end_tasks.lock().unwrap();
                if !tasks.accepting {
                    cancellation.cancel();
                    if let Ok(handle) = handle {
                        tasks.tasks.push(SessionEndTask {
                            cancellation,
                            handle,
                        });
                    }
                    tasks
                        .failures
                        .push("session-end work arrived after shutdown admission closed".into());
                } else {
                    match handle {
                        Ok(handle) => tasks.tasks.push(SessionEndTask {
                            cancellation,
                            handle,
                        }),
                        Err(error) => tasks
                            .failures
                            .push(format!("failed to spawn session-end task: {error}")),
                    }
                }
                vec![]
            }

            _ => Vec::new(),
        }
    }

    async fn prepare_managed_shutdown(&mut self) -> anyhow::Result<()> {
        let (tasks, mut failures) = {
            let mut state = self.session_end_tasks.lock().unwrap();
            state.accepting = false;
            let tasks = std::mem::take(&mut state.tasks);
            let failures = std::mem::take(&mut state.failures);
            (tasks, failures)
        };
        for task in &tasks {
            task.cancellation.cancel();
        }
        for task in tasks {
            match tokio::task::spawn_blocking(move || task.handle.join()).await {
                Ok(Ok(Ok(()))) => {}
                Ok(Ok(Err(error))) => failures.push(error),
                Ok(Err(_)) => failures.push("session-end task panicked".into()),
                Err(error) => failures.push(format!("session-end join task failed: {error}")),
            }
        }
        if failures.is_empty() {
            Ok(())
        } else {
            anyhow::bail!(failures.join("; "))
        }
    }

    fn provide_context(&self, signals: &ContextSignals<'_>) -> Option<ContextInjection> {
        // Run async in a blocking context since provide_context is sync
        let mind = self.mind.clone();
        let wm_ids = self.working_memory.lock().unwrap().clone();

        let binding = self.memory_binding.clone();
        let turn_number = signals.turn_number;
        if signals.context_budget_tokens == 0 {
            *self.last_selection.lock().unwrap() =
                Some(omegon_memory::selection::empty_report(None));
            return self.clear_memory_context();
        }

        std::thread::scope(|scope| {
            scope
                .spawn(|| {
                    let runtime = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .ok()?;
                    runtime.block_on(async {
                        let response = match binding
                            .invoke(crate::memory_service::MemoryRequestV1::SelectContext {
                                scope: crate::memory_service::MemoryScopeV1::Project,
                                mind,
                                query: signals.user_prompt.to_string(),
                                pins: wm_ids,
                                host_budget: signals.context_budget_tokens,
                                intent: omegon_memory::MemorySelectionIntent::Ambient,
                                fetch_limit: omegon_memory::selection::MAX_CANDIDATES,
                                cancellation: tokio_util::sync::CancellationToken::new(),
                            })
                            .await
                        {
                            Ok(response) => response,
                            Err(_) => {
                                *self.last_selection.lock().unwrap() =
                                    Some(omegon_memory::selection::empty_report(Some(
                                        "memory_selection_failed".into(),
                                    )));
                                return self.clear_memory_context();
                            }
                        };
                        let crate::memory_service::MemoryPayloadV1::Selection(selected) =
                            response.payload
                        else {
                            *self.last_selection.lock().unwrap() =
                                Some(omegon_memory::selection::empty_report(Some(
                                    "memory_selection_invalid_response".into(),
                                )));
                            return self.clear_memory_context();
                        };

                        *self.last_selection.lock().unwrap() = Some(selected.report);
                        if selected.markdown.is_empty() {
                            return self.clear_memory_context();
                        }

                        // Hash the rendered content to detect changes
                        use std::hash::{Hash, Hasher};
                        let mut hasher = std::collections::hash_map::DefaultHasher::new();
                        selected.markdown.hash(&mut hasher);
                        let content_hash = hasher.finish();

                        // Skip re-injection if content is unchanged and no mutation occurred
                        let dirty = self
                            .context_dirty
                            .swap(false, std::sync::atomic::Ordering::Relaxed);
                        let mut last_hash = self.last_context_hash.lock().unwrap();
                        let mut last_turn = self.last_context_turn.lock().unwrap();
                        let injection_alive =
                            last_turn.is_some_and(|last| turn_number.saturating_sub(last) < 3);
                        if !dirty
                            && *last_hash == content_hash
                            && content_hash != 0
                            && injection_alive
                        {
                            return None; // existing injection persists via TTL
                        }
                        *last_hash = content_hash;
                        *last_turn = Some(turn_number);

                        Some(ContextInjection {
                            source: "memory".into(),
                            content: selected.markdown,
                            priority: 200, // high — memory is important context
                            ttl_turns: 3,  // persist for 3 turns, then refresh
                        })
                    })
                })
                .join()
                .ok()?
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio_util::sync::CancellationToken;

    async fn managed_feature() -> (MemoryFeature, crate::bus::EventBus, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let binding = crate::memory_service::MemoryBinding::default();
        let mut bus = crate::bus::EventBus::new();
        bus.register(Box::new(crate::memory_service::MemoryDeclarationFeature));
        let candidate =
            crate::memory_service::start_candidate(crate::memory_service::MemoryWorkerConfig {
                workspace_root: Some(dir.path().to_path_buf()),
                memory_token_cap: None,
                project_memory_root: dir.path().to_path_buf(),
                project_db_path: dir.path().join("facts.db"),
                project_jsonl_path: dir.path().join("facts.jsonl"),
                global_db_path: None,
                vault: None,
                startup_sync_enabled: false,
            })
            .await
            .unwrap();
        bus.stage_managed_generation("memory", candidate).unwrap();
        bus.try_finalize_managed().await.unwrap();
        binding.capture(&bus).unwrap();
        let mut feature =
            MemoryFeature::new(binding, "test".into()).with_status_root(dir.path().to_path_buf());
        feature.on_event(&BusEvent::SessionStart {
            session_id: "fixture-session".into(),
            cwd: dir.path().to_path_buf(),
        });
        (feature, bus, dir)
    }

    #[tokio::test]
    async fn initial_wave_live_tool_contracts() {
        let (mut feature, mut bus, _dir) = managed_feature().await;
        feature.mind = "wave".into();
        feature
            .apply_mutation(
                "wave-fixture".into(),
                MemoryMutation::ImportJsonl {
                    jsonl: include_str!("../../../omegon-memory/tests/fixtures/retrieval.jsonl")
                        .into(),
                },
                CancellationToken::new(),
            )
            .await
            .unwrap();
        let archived = feature
            .execute(
                "memory_search_archive",
                "archive-check",
                serde_json::json!({"query":"zircon"}),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        let recalled = feature
            .execute(
                "memory_recall",
                "section-check",
                serde_json::json!({"query":"zircon", "section":"Constraints", "k":1}),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        assert!(
            bus.shutdown_managed_services()
                .await
                .all_resources_settled()
        );
        let archive_text = archived.content[0].as_text().unwrap();
        let recall_text = recalled.content[0].as_text().unwrap();
        assert!(
            archive_text.contains("[obsolete]") && !archive_text.contains("[current]"),
            "{archive_text}"
        );
        assert!(recall_text.contains("[constraint]"), "{recall_text}");
    }

    #[tokio::test]
    async fn wave4_recall_reports_incompatible_vectors_and_named_scores() {
        struct IdentifiedService(&'static str);
        #[async_trait]
        impl EmbeddingService for IdentifiedService {
            async fn embed(&self, _: &str) -> Result<Vec<f32>, omegon_memory::EmbedError> {
                panic!("memory must use identified generation");
            }
            fn model_name(&self) -> &str {
                self.0
            }
            async fn embed_identified(
                &self,
                _: &str,
            ) -> Result<omegon_memory::IdentifiedEmbedding, omegon_memory::EmbedError> {
                Ok(omegon_memory::IdentifiedEmbedding {
                    space: omegon_memory::EmbeddingSpace {
                        model: self.0.into(),
                        revision: "fixture-v1".into(),
                        preprocessing: "raw-v1".into(),
                        dimensions: 2,
                    },
                    values: vec![1.0, 0.0],
                })
            }
        }
        let (feature, mut bus, _dir) = managed_feature().await;
        let stored = feature
            .apply_mutation(
                "wave4-fact".into(),
                MemoryMutation::StoreFact {
                    request: StoreFact {
                        mind: "test".into(),
                        content: "zircon identified recall".into(),
                        section: Section::Constraints,
                        source: None,
                        decay_profile: Default::default(),
                    },
                },
                CancellationToken::new(),
            )
            .await
            .unwrap();
        let MemoryMutationEffect::FactStored {
            fact_id, version, ..
        } = stored.effect
        else {
            panic!("fact");
        };
        let embedding = IdentifiedService("model-a")
            .embed_identified("unused")
            .await
            .unwrap();
        feature
            .apply_mutation(
                "wave4-index".into(),
                MemoryMutation::StoreIdentifiedEmbedding {
                    fact: FactPrecondition {
                        id: fact_id,
                        expected_version: version,
                    },
                    embedding,
                },
                CancellationToken::new(),
            )
            .await
            .unwrap();
        let feature = feature.with_embed_service(Arc::new(IdentifiedService("model-b")));
        let degraded = feature
            .execute(
                "memory_recall",
                "wrong-space",
                serde_json::json!({"query":"zircon"}),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        let feature = feature.with_embed_service(Arc::new(IdentifiedService("model-a")));
        let compatible = feature
            .execute(
                "memory_recall",
                "same-space",
                serde_json::json!({"query":"zircon"}),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        feature
            .execute(
                "memory_store",
                "automatic-index",
                serde_json::json!({"section":"Architecture","content":"newborn auto indexed fact"}),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        let automatic = feature
            .execute(
                "memory_recall",
                "automatic-recall",
                serde_json::json!({"query":"newborn"}),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        assert!(
            bus.shutdown_managed_services()
                .await
                .all_resources_settled()
        );
        assert_eq!(degraded.details["vector"]["incompatible"], 1);
        assert!(degraded.content[0].as_text().unwrap().contains("lexical="));
        assert!(degraded.details["scores"][0]["scores"]["cosine"].is_null());
        let text = compatible.content[0].as_text().unwrap();
        for label in ["lexical=", "cosine=", "rrf="] {
            assert!(text.contains(label), "{text}");
        }
        assert!(!text.contains('%'));
        assert_eq!(automatic.details["vector"]["compatible"], 2);
    }

    #[tokio::test]
    async fn initial_wave_empty_task_selection_replaces_live_ttl() {
        let (feature, mut bus, _dir) = managed_feature().await;
        feature
            .execute(
                "memory_store",
                "ttl-fact",
                serde_json::json!({
                    "section":"Constraints", "content":"zircon requires atomic migration"
                }),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        let signals = ContextSignals {
            user_prompt: "atomic migration",
            recent_tools: &[],
            recent_files: &[],
            lifecycle_phase: &LifecyclePhase::Idle,
            turn_number: 1,
            context_budget_tokens: 200,
        };
        let first = feature.provide_context(&signals);
        let second = feature.provide_context(&ContextSignals {
            user_prompt: "unrelatedquartz",
            turn_number: 2,
            ..signals
        });
        assert!(
            bus.shutdown_managed_services()
                .await
                .all_resources_settled()
        );
        assert!(first.unwrap().content.contains("atomic migration"));
        let replacement = second.expect("explicit empty replacement must retire prior live memory");
        assert_eq!(replacement.source, "memory");
        assert!(replacement.content.is_empty());
    }

    fn adversarial_input() -> SessionEndPipelineInput {
        SessionEndPipelineInput {
            mind: "test".into(),
            memory_binding: Default::default(),
            extractor: None,
            evidence: formation::sample_evidence(),
            session_id: "synthetic-session".into(),
            status_root: Default::default(),
            turns: 1,
            tool_calls: 1,
            duration_secs: 1.0,
        }
    }

    #[test]
    fn adversarial_capture_replay_ignores_advisory_statistics() {
        let mut input = adversarial_input();
        let first = formation_episode_request(&input);
        input.turns = 500;
        input.tool_calls = 900;
        input.duration_secs = 3600.0;
        let second = formation_episode_request(&input);
        assert_eq!(first.0, second.0);
        assert_eq!(
            first.1, second.1,
            "same capture identity must bind the same payload"
        );
    }

    #[test]
    fn adversarial_capture_date_comes_from_evidence_and_minds_are_isolated() {
        let mut input = adversarial_input();
        input.evidence.evidence[0].recorded_at = "2001-01-01T00:00:00Z".into();
        let first = formation_episode_request(&input);
        assert_eq!(first.1.date.as_deref(), Some("2001-01-01"));
        input.mind = "other".into();
        assert_ne!(
            first.0,
            formation_episode_request(&input).0,
            "mind scopes cannot share a capture receipt"
        );
    }

    #[tokio::test]
    async fn adversarial_stable_capture_payload_replays_through_managed_storage() {
        let (feature, mut bus, _dir) = managed_feature().await;
        let mut input = adversarial_input();
        let (id, request) = formation_episode_request(&input);
        assert!(id.contains(":formation-v2:"));
        let first = feature
            .apply_mutation(
                id.clone(),
                MemoryMutation::StoreEpisode { request },
                CancellationToken::new(),
            )
            .await
            .unwrap();
        assert!(!first.replayed);
        input.turns = 100;
        input.tool_calls = 200;
        input.duration_secs = 86400.0;
        let (_, request) = formation_episode_request(&input);
        let replay = feature
            .apply_mutation(
                id,
                MemoryMutation::StoreEpisode { request },
                CancellationToken::new(),
            )
            .await
            .unwrap();
        assert!(replay.replayed);
        assert_eq!(first.effect, replay.effect);
        input.mind = "other".into();
        let (id, request) = formation_episode_request(&input);
        assert!(
            !feature
                .apply_mutation(
                    id,
                    MemoryMutation::StoreEpisode { request },
                    CancellationToken::new()
                )
                .await
                .unwrap()
                .replayed
        );
        assert!(
            bus.shutdown_managed_services()
                .await
                .all_resources_settled()
        );
    }

    #[tokio::test]
    async fn adversarial_invalid_model_does_not_discard_source_evidence() {
        let (feature, mut bus, _dir) = managed_feature().await;
        let profile = crate::settings::Profile {
            memory_extraction_model: Some("m".repeat(513)),
            ..Default::default()
        };
        let feature = feature.with_capabilities(&profile, false, None);
        let mut input = adversarial_input();
        input.memory_binding = feature.memory_binding.clone();
        input.extractor = feature.extractor.clone();
        run_session_end_pipeline(input).await;
        let payload = feature
            .invoke(crate::memory_service::MemoryRequestV1::ListEpisodes {
                scope: crate::memory_service::MemoryScopeV1::Project,
                mind: "test".into(),
                limit: 1,
                cancellation: CancellationToken::new(),
            })
            .await
            .unwrap();
        assert!(
            bus.shutdown_managed_services()
                .await
                .all_resources_settled()
        );
        let crate::memory_service::MemoryPayloadV1::Episodes(episodes) = payload else {
            panic!("episodes");
        };
        assert_eq!(
            episodes.len(),
            1,
            "invalid optional model must not prevent durable capture"
        );
        assert!(feature.extractor.is_none());
    }

    #[test]
    fn wave3_extraction_is_configured_without_embeddings() {
        let feature = MemoryFeature::new(Default::default(), "wave3".into()).with_capabilities(
            &Default::default(),
            false,
            None,
        );
        assert!(
            feature.extractor.is_some(),
            "embedding discovery must not disable extraction"
        );
        assert!(feature.embed_service.is_none());
    }

    #[test]
    fn wave3_profile_override_disable_and_child_policy_are_independent() {
        let mut profile: crate::settings::Profile = serde_json::from_value(serde_json::json!({
            "memoryExtractionModel":"fixture:cheap", "memoryExtractionEnabled":true
        }))
        .unwrap();
        let feature = MemoryFeature::new(Default::default(), "wave3".into())
            .with_capabilities(&profile, false, None);
        assert_eq!(feature.extractor.unwrap().model(), "fixture:cheap");
        profile.memory_extraction_enabled = Some(false);
        let disabled = MemoryFeature::new(Default::default(), "wave3".into())
            .with_extraction_model("previous".into())
            .with_capabilities(&profile, false, None);
        assert!(disabled.extractor.is_none());
        profile.memory_extraction_enabled = Some(true);
        let child = MemoryFeature::new(Default::default(), "wave3".into())
            .with_capabilities(&profile, true, None);
        assert!(child.extractor.is_none());
    }

    #[tokio::test]
    async fn wave3_pipeline_persists_pending_candidates_without_creating_facts() {
        struct Fake;
        #[async_trait]
        impl formation::Extractor for Fake {
            fn model(&self) -> &str {
                "fixture-model"
            }
            async fn extract(&self, _: &str) -> anyhow::Result<String> {
                Ok(r#"[{"content":"Migration must be atomic.","section":"Constraints","evidence_ids":["event-4"]}]"#.into())
            }
        }
        let (feature, mut bus, dir) = managed_feature().await;
        for _ in 0..2 {
            run_session_end_pipeline(SessionEndPipelineInput {
                mind: "test".into(),
                memory_binding: feature.memory_binding.clone(),
                extractor: Some(Arc::new(Fake)),
                evidence: formation::sample_evidence(),
                session_id: "synthetic-session".into(),
                status_root: dir.path().into(),
                turns: 2,
                tool_calls: 1,
                duration_secs: 1.0,
            })
            .await;
        }
        let payload = feature
            .invoke(crate::memory_service::MemoryRequestV1::ListEpisodes {
                scope: crate::memory_service::MemoryScopeV1::Project,
                mind: "test".into(),
                limit: 10,
                cancellation: CancellationToken::new(),
            })
            .await
            .unwrap();
        let crate::memory_service::MemoryPayloadV1::Episodes(episodes) = payload else {
            panic!("episodes");
        };
        assert_eq!(episodes.len(), 1);
        assert_eq!(
            episodes[0].formation.as_ref().unwrap().candidates[0].section,
            Section::Constraints
        );
        let payload = feature
            .invoke(crate::memory_service::MemoryRequestV1::Stats {
                scope: crate::memory_service::MemoryScopeV1::Project,
                mind: "test".into(),
                cancellation: CancellationToken::new(),
            })
            .await
            .unwrap();
        let crate::memory_service::MemoryPayloadV1::Stats(stats) = payload else {
            panic!("stats");
        };
        assert_eq!(stats.total_facts, 0);
        assert!(
            bus.shutdown_managed_services()
                .await
                .all_resources_settled()
        );
    }

    #[tokio::test]
    async fn wave3_cancellation_keeps_source_and_pending_extraction_durable() {
        struct Waiting(Arc<tokio::sync::Notify>);
        #[async_trait]
        impl formation::Extractor for Waiting {
            fn model(&self) -> &str {
                "fixture-waiting"
            }
            async fn extract(&self, _: &str) -> anyhow::Result<String> {
                self.0.notify_one();
                std::future::pending().await
            }
        }
        let (feature, mut bus, dir) = managed_feature().await;
        let started = Arc::new(tokio::sync::Notify::new());
        let task = tokio::spawn(run_session_end_pipeline(SessionEndPipelineInput {
            mind: "test".into(),
            memory_binding: feature.memory_binding.clone(),
            extractor: Some(Arc::new(Waiting(started.clone()))),
            evidence: formation::sample_evidence(),
            session_id: "synthetic-session".into(),
            status_root: dir.path().into(),
            turns: 1,
            tool_calls: 0,
            duration_secs: 1.0,
        }));
        tokio::time::timeout(std::time::Duration::from_secs(5), started.notified())
            .await
            .unwrap();
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        let payload = feature
            .invoke(crate::memory_service::MemoryRequestV1::ListEpisodes {
                scope: crate::memory_service::MemoryScopeV1::Project,
                mind: "test".into(),
                limit: 1,
                cancellation: CancellationToken::new(),
            })
            .await
            .unwrap();
        let crate::memory_service::MemoryPayloadV1::Episodes(episodes) = payload else {
            panic!("episodes");
        };
        let evidence = episodes[0].formation.as_ref().unwrap();
        assert_eq!(evidence.evidence.len(), 1);
        assert!(matches!(
            evidence.extraction,
            omegon_memory::ExtractionOutcome::Pending { .. }
        ));
        assert!(
            bus.shutdown_managed_services()
                .await
                .all_resources_settled()
        );
    }

    #[tokio::test]
    async fn feature_exposes_public_memory_tools_without_internal_confirmation() {
        let feature = MemoryFeature::new(Default::default(), "test".into());
        let tools = feature.tools();
        assert_eq!(tools.len(), 16, "public memory tool inventory");

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
        assert!(names.contains(&"memory_confirm"));
        assert!(names.contains(&"memory_inspect"));
        assert!(names.contains(&"memory_set_applicability"));
        assert!(names.contains(&"memory_selection"));
        assert!(!names.contains(&"memory_apply_confirmation"));
    }

    #[test]
    fn durable_memory_mutations_are_declared_state_changing() {
        let feature = MemoryFeature::new(Default::default(), "test".into());
        let tools = feature.tools();

        for name in [
            "memory_store",
            "memory_archive",
            "memory_supersede",
            "memory_connect",
            "memory_focus",
            "memory_release",
            "memory_compact",
            "memory_ingest_lifecycle",
            "memory_confirm",
            "memory_set_applicability",
        ] {
            let tool = tools.iter().find(|tool| tool.name == name).unwrap();
            assert!(
                tool.capabilities
                    .contains(&omegon_traits::ToolCapability::StateChanging),
                "{name} must declare mutation authority"
            );
        }
        for name in [
            "memory_recall",
            "memory_query",
            "memory_episodes",
            "memory_search_archive",
        ] {
            let tool = tools.iter().find(|tool| tool.name == name).unwrap();
            assert!(
                !tool
                    .capabilities
                    .contains(&omegon_traits::ToolCapability::StateChanging),
                "{name} must remain read-only"
            );
        }
    }

    #[tokio::test]
    async fn store_and_query_integration() {
        let (feature, mut bus, _dir) = managed_feature().await;
        let cancel = tokio_util::sync::CancellationToken::new();

        // Store a fact
        let result = feature.execute(
            "memory_store", "c1",
            serde_json::json!({"section": "Architecture", "content": "System uses microservices"}),
            cancel.clone(),
        ).await.unwrap();
        assert!(result.content[0].as_text().unwrap().contains("Stored"));

        // Query all facts
        let result = feature
            .execute("memory_query", "c2", serde_json::json!({}), cancel.clone())
            .await
            .unwrap();
        let text = result.content[0].as_text().unwrap();
        assert!(
            text.contains("microservices"),
            "query should return stored fact: {text}"
        );
        assert!(
            bus.shutdown_managed_services()
                .await
                .all_resources_settled()
        );
    }

    #[tokio::test]
    async fn recall_search() {
        let (feature, mut bus, _dir) = managed_feature().await;
        let cancel = tokio_util::sync::CancellationToken::new();

        // Store a fact
        feature.execute(
            "memory_store", "c1",
            serde_json::json!({"section": "Architecture", "content": "Authentication uses OAuth2 with PKCE flow"}),
            cancel.clone(),
        ).await.unwrap();

        // Search for it
        let result = feature
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
        assert!(
            bus.shutdown_managed_services()
                .await
                .all_resources_settled()
        );
    }

    #[tokio::test]
    async fn recall_requires_non_empty_query() {
        let feature = MemoryFeature::new(Default::default(), "test".into());
        let cancel = tokio_util::sync::CancellationToken::new();

        let result = feature
            .execute(
                "memory_recall",
                "c1",
                serde_json::json!({"query": "   "}),
                cancel,
            )
            .await
            .unwrap();

        let text = result.content[0].as_text().unwrap();
        assert!(text.contains("requires a non-empty query"));
        assert_eq!(result.details["is_error"], true);
    }

    #[tokio::test]
    async fn memory_store_rejects_invalid_section() {
        let feature = MemoryFeature::new(Default::default(), "test".into());
        let cancel = tokio_util::sync::CancellationToken::new();

        let err = feature
            .execute(
                "memory_store",
                "c1",
                serde_json::json!({"section": "Notes", "content": "System uses microservices"}),
                cancel,
            )
            .await
            .unwrap_err();

        assert!(err.to_string().contains("invalid memory section 'Notes'"));
    }

    #[tokio::test]
    async fn memory_supersede_rejects_invalid_section() {
        let (feature, mut bus, _dir) = managed_feature().await;
        let cancel = tokio_util::sync::CancellationToken::new();

        let stored = feature
            .execute(
                "memory_store",
                "c1",
                serde_json::json!({"section": "Architecture", "content": "System uses microservices"}),
                cancel.clone(),
            )
            .await
            .unwrap();
        let fact_id = stored.details["id"].as_str().unwrap();

        let err = feature
            .execute(
                "memory_supersede",
                "c2",
                serde_json::json!({"fact_id": fact_id, "section": "Notes", "content": "System uses services"}),
                cancel,
            )
            .await
            .unwrap_err();

        assert!(err.to_string().contains("invalid memory section 'Notes'"));
        assert!(
            bus.shutdown_managed_services()
                .await
                .all_resources_settled()
        );
    }

    #[tokio::test]
    async fn memory_ingest_lifecycle_rejects_invalid_section() {
        let feature = MemoryFeature::new(Default::default(), "test".into());
        let cancel = tokio_util::sync::CancellationToken::new();

        let err = feature
            .execute(
                "memory_ingest_lifecycle",
                "c1",
                serde_json::json!({
                    "source_kind": "design-tree",
                    "authority": "inferred",
                    "section": "Notes",
                    "content": "Lifecycle fact"
                }),
                cancel,
            )
            .await
            .unwrap_err();

        assert!(err.to_string().contains("invalid memory section 'Notes'"));
    }

    #[tokio::test]
    async fn inspection_reports_unavailable_declared_source_without_mutation() {
        let (feature, mut bus, _dir) = managed_feature().await;
        let stored=feature.execute("memory_ingest_lifecycle","candidate",serde_json::json!({"source_kind":"design-tree","authority":"inferred","section":"Decisions","content":"zircon unverified conclusion",
            "artifact_ref_type":"design","artifact_ref_path":"docs/design/missing.md"}),CancellationToken::new()).await.unwrap();
        let id = stored.details["id"].as_str().unwrap().to_string();
        let pending = || crate::memory_service::MemoryRequestV1::GetPendingFact {
            scope: crate::memory_service::MemoryScopeV1::Project,
            mind: "test".into(),
            id: id.clone(),
            cancellation: CancellationToken::new(),
        };
        let before = serde_json::to_value(feature.invoke(pending()).await.unwrap()).unwrap();
        let inspected = feature
            .execute(
                "memory_inspect",
                "inspect",
                serde_json::json!({"fact_id":id}),
                CancellationToken::new(),
            )
            .await;
        let after = serde_json::to_value(feature.invoke(pending()).await.unwrap()).unwrap();
        assert!(
            bus.shutdown_managed_services()
                .await
                .all_resources_settled()
        );
        let inspected = inspected.unwrap();
        assert_eq!(inspected.details["status"], "pending");
        assert_eq!(inspected.details["basis"], "unconfirmed_inference");
        assert_eq!(inspected.details["evidence_availability"], "unavailable");
        assert_eq!(before, after);
    }

    #[tokio::test]
    async fn applicability_tracks_head_changes_and_retires_same_turn_context() {
        let (feature, mut bus, dir) = managed_feature().await;
        let repo = git2::Repository::init(dir.path()).unwrap();
        let signature = git2::Signature::now("test", "test@example.invalid").unwrap();
        let tree_id = repo.index().unwrap().write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        let first = repo
            .commit(Some("HEAD"), &signature, &signature, "first", &tree, &[])
            .unwrap();
        let revision = format!("git:{first}");
        feature.execute("memory_store","scoped",serde_json::json!({"section":"Constraints","content":"zircon revision rule","applicability":{"revisions":[revision]}}),CancellationToken::new()).await.unwrap();
        let signals = ContextSignals {
            user_prompt: "zircon",
            recent_tools: &[],
            recent_files: &[],
            lifecycle_phase: &LifecyclePhase::Idle,
            turn_number: 1,
            context_budget_tokens: 500,
        };
        let initial = feature.provide_context(&signals).unwrap();
        assert!(initial.content.contains("zircon revision rule"));
        std::fs::write(dir.path().join("untracked.txt"), "dirty workspace").unwrap();
        let context = crate::memory_service::applicability_context(Some(dir.path()));
        assert_eq!(context.revision.as_deref(), Some(revision.as_str()));
        assert_eq!(
            context.workspace,
            Some(crate::workspace::runtime::workspace_id_from_path(
                &dir.path().canonicalize().unwrap()
            ))
        );
        let parent = repo.find_commit(first).unwrap();
        repo.commit(
            Some("HEAD"),
            &signature,
            &signature,
            "second",
            &tree,
            &[&parent],
        )
        .unwrap();
        let cleared = feature
            .provide_context(&signals)
            .expect("known mismatch must replace a live injection even within the same turn");
        assert!(cleared.content.is_empty());
        let recall = feature
            .execute(
                "memory_recall",
                "recall",
                serde_json::json!({"query":"zircon"}),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(recall.details["count"], 0);
        assert!(
            bus.shutdown_managed_services()
                .await
                .all_resources_settled()
        );
    }

    #[tokio::test]
    async fn applicability_update_expires_context_without_reinforcement() {
        let (feature, mut bus, _dir) = managed_feature().await;
        let stored = feature
            .execute(
                "memory_store",
                "store",
                serde_json::json!({"section":"Constraints","content":"zircon expires"}),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        let id = stored.details["id"].as_str().unwrap().to_string();
        let before = feature
            .get_fact(id.clone(), CancellationToken::new())
            .await
            .unwrap()
            .unwrap();
        let signals = ContextSignals {
            user_prompt: "zircon",
            recent_tools: &[],
            recent_files: &[],
            lifecycle_phase: &LifecyclePhase::Idle,
            turn_number: 1,
            context_budget_tokens: 500,
        };
        assert!(
            feature
                .provide_context(&signals)
                .unwrap()
                .content
                .contains("applicability unknown")
        );
        let args = serde_json::json!({"fact_id":id,"expected_version":before.version,"applicability":{"valid_until":"2000-01-01T00:00:00Z"}});
        feature
            .execute(
                "memory_set_applicability",
                "expire",
                args.clone(),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        let replay = feature
            .execute(
                "memory_set_applicability",
                "expire",
                args,
                CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(replay.details["replayed"], true);
        assert!(
            feature
                .provide_context(&signals)
                .unwrap()
                .content
                .is_empty()
        );
        let after = feature
            .get_fact(id, CancellationToken::new())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(after.status, omegon_memory::FactStatus::Active);
        assert_eq!(after.reinforcement_count, before.reinforcement_count);
        assert_eq!(after.last_reinforced, before.last_reinforced);
        assert!(
            bus.shutdown_managed_services()
                .await
                .all_resources_settled()
        );
    }

    #[tokio::test]
    async fn applicability_revalidation_failure_retires_injection_but_preserves_facts() {
        let (feature, mut bus, dir) = managed_feature().await;
        feature
            .execute(
                "memory_store",
                "store",
                serde_json::json!({"section":"Constraints","content":"zircon remains stored"}),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        let signals = ContextSignals {
            user_prompt: "zircon",
            recent_tools: &[],
            recent_files: &[],
            lifecycle_phase: &LifecyclePhase::Idle,
            turn_number: 1,
            context_budget_tokens: 500,
        };
        assert!(
            feature
                .provide_context(&signals)
                .unwrap()
                .content
                .contains("zircon remains stored")
        );
        let connection = rusqlite::Connection::open(dir.path().join("facts.db")).unwrap();
        connection.execute_batch("DROP TABLE facts_fts;").unwrap();
        assert!(
            feature
                .provide_context(&signals)
                .unwrap()
                .content
                .is_empty()
        );
        assert_eq!(
            connection
                .query_row("SELECT COUNT(*) FROM facts", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert!(
            bus.shutdown_managed_services()
                .await
                .all_resources_settled()
        );
    }

    #[tokio::test]
    async fn token_selection_never_exceeds_conservative_host_budget() {
        let (feature, mut bus, _dir) = managed_feature().await;
        feature.execute("memory_store","large",serde_json::json!({"section":"Constraints","content":format!("zircon {}","漢".repeat(100))}),CancellationToken::new()).await.unwrap();
        feature
            .execute(
                "memory_store",
                "small",
                serde_json::json!({"section":"Constraints","content":"zircon small fact"}),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        let signals = ContextSignals {
            user_prompt: "zircon",
            recent_tools: &[],
            recent_files: &[],
            lifecycle_phase: &LifecyclePhase::Idle,
            turn_number: 1,
            context_budget_tokens: 200,
        };
        let injected = feature.provide_context(&signals).unwrap();
        assert!(
            bus.shutdown_managed_services()
                .await
                .all_resources_settled()
        );
        assert!(
            injected.content.len() <= 200,
            "conservative accounting includes UTF-8 bytes and formatting"
        );
        assert!(injected.content.contains("zircon small fact"));
    }

    #[tokio::test]
    async fn token_selection_standalone_and_hosted_ambient_agree() {
        let (feature, mut bus, dir) = managed_feature().await;
        feature
            .execute(
                "memory_store",
                "fact",
                serde_json::json!({"section":"Constraints","content":"zircon shared evidence"}),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        let standalone = omegon_memory::MemoryProvider::new(
            omegon_memory::SqliteBackend::open(&dir.path().join("facts.db")).unwrap(),
            omegon_memory::MarkdownRenderer,
            "test".into(),
        );
        let signals = ContextSignals {
            user_prompt: "zircon",
            recent_tools: &[],
            recent_files: &[],
            lifecycle_phase: &LifecyclePhase::Idle,
            turn_number: 1,
            context_budget_tokens: 900,
        };
        let hosted = feature.provide_context(&signals).unwrap();
        let direct =
            omegon_traits::ContextProvider::provide_context(&standalone, &signals).unwrap();
        assert_eq!(hosted.content, direct.content);
        let host_report = feature
            .execute(
                "memory_selection",
                "report",
                serde_json::json!({}),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        let direct_report = omegon_traits::ToolProvider::execute(
            &standalone,
            "memory_selection",
            "report",
            serde_json::json!({}),
            CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(host_report.details, direct_report.details);
        assert!(
            bus.shutdown_managed_services()
                .await
                .all_resources_settled()
        );
    }

    #[tokio::test]
    async fn inspection_tracks_artifact_availability_without_reactivating_history() {
        let (feature, mut bus, dir) = managed_feature().await;
        std::fs::create_dir_all(dir.path().join("docs/design")).unwrap();
        let path = dir.path().join("docs/design/zircon.md");
        std::fs::write(&path, lifecycle::TEST_DESIGN).unwrap();
        let stored=feature.execute("memory_ingest_lifecycle","explicit",serde_json::json!({"source_kind":"design-tree","authority":"explicit","section":"Decisions",
            "content":"Use transactions: Keep corrections atomic.","artifact_ref_type":"design","artifact_ref_path":"docs/design/zircon.md","artifact_ref_sub":"Use transactions"}),CancellationToken::new()).await.unwrap();
        let id = stored.details["id"].as_str().unwrap().to_string();
        feature
            .execute(
                "memory_archive",
                "archive",
                serde_json::json!({"fact_ids":[id]}),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        let record = || crate::memory_service::MemoryRequestV1::GetFactRecord {
            scope: crate::memory_service::MemoryScopeV1::Project,
            mind: "test".into(),
            id: id.clone(),
            cancellation: CancellationToken::new(),
        };
        let before = serde_json::to_value(feature.invoke(record()).await.unwrap()).unwrap();
        for (step, expected) in [
            (0, "snapshot_matches"),
            (1, "snapshot_changed"),
            (2, "unavailable"),
        ] {
            if step == 1 {
                std::fs::write(&path, lifecycle::TEST_DESIGN.replace("atomic", "guarded")).unwrap();
            }
            if step == 2 {
                std::fs::remove_file(&path).unwrap();
            }
            let inspected = feature
                .execute(
                    "memory_inspect",
                    "inspect",
                    serde_json::json!({"fact_id":id}),
                    CancellationToken::new(),
                )
                .await
                .unwrap();
            assert_eq!(inspected.details["evidence_availability"], expected);
            if step == 0 {
                let mut projection: omegon_memory::FactInspection =
                    serde_json::from_value(inspected.details.clone()).unwrap();
                let artifact = projection.artifact.as_mut().unwrap();
                artifact.artifact_sha256 = artifact.artifact_sha256.to_uppercase();
                assert_eq!(
                    lifecycle::inspect_source(dir.path(), projection).evidence_availability,
                    omegon_memory::EvidenceAvailability::SnapshotMatches
                );
            }
            assert_eq!(inspected.details["status"], "archived");
            assert_eq!(inspected.details["basis"], "explicit_artifact");
        }
        #[cfg(unix)]
        {
            let outside = tempfile::tempdir().unwrap();
            let target = outside.path().join("secret.md");
            std::fs::write(&target, lifecycle::TEST_DESIGN).unwrap();
            std::os::unix::fs::symlink(target, &path).unwrap();
            let inspected = feature
                .execute(
                    "memory_inspect",
                    "symlink",
                    serde_json::json!({"fact_id":id}),
                    CancellationToken::new(),
                )
                .await
                .unwrap();
            assert_eq!(inspected.details["evidence_availability"], "unavailable");
        }
        let after = serde_json::to_value(feature.invoke(record()).await.unwrap()).unwrap();
        assert!(
            bus.shutdown_managed_services()
                .await
                .all_resources_settled()
        );
        assert_eq!(before, after);
    }

    #[tokio::test]
    async fn inspection_does_not_validate_declared_references_or_follow_unsupported_paths() {
        let (feature, mut bus, dir) = managed_feature().await;
        std::fs::create_dir_all(dir.path().join("docs/design")).unwrap();
        std::fs::write(
            dir.path().join("docs/design/plain.md"),
            "plain text, not an explicit decision",
        )
        .unwrap();
        for (path, expected) in [
            ("docs/design/plain.md", "readable_unverified"),
            ("../outside.md", "unsupported_reference"),
        ] {
            let stored=feature.execute("memory_ingest_lifecycle",path,serde_json::json!({"source_kind":"design-tree","authority":"inferred","section":"Decisions","content":"unverified","artifact_ref_type":"design","artifact_ref_path":path}),CancellationToken::new()).await.unwrap();
            let inspected = feature
                .execute(
                    "memory_inspect",
                    "inspect",
                    serde_json::json!({"fact_id":stored.details["id"]}),
                    CancellationToken::new(),
                )
                .await
                .unwrap();
            assert_eq!(inspected.details["evidence_availability"], expected);
            assert_eq!(inspected.details["basis"], "unconfirmed_inference");
        }
        assert!(
            bus.shutdown_managed_services()
                .await
                .all_resources_settled()
        );
    }

    #[tokio::test]
    async fn lifecycle_explicit_claim_requires_artifact_evidence() {
        let (feature, mut bus, _dir) = managed_feature().await;
        let result = feature
            .execute(
                "memory_ingest_lifecycle",
                "unsupported",
                serde_json::json!({
                    "source_kind":"design-tree", "authority":"explicit", "section":"Decisions",
                    "content":"zircon tests passed"
                }),
                CancellationToken::new(),
            )
            .await;
        assert!(
            bus.shutdown_managed_services()
                .await
                .all_resources_settled()
        );
        assert!(
            result.is_err(),
            "the authority string alone cannot establish explicit evidence"
        );
    }

    #[tokio::test]
    async fn lifecycle_explicit_tool_corrects_with_artifact_attribution_and_replay() {
        let (feature, mut bus, dir) = managed_feature().await;
        std::fs::create_dir_all(dir.path().join("docs/design")).unwrap();
        std::fs::write(
            dir.path().join("docs/design/zircon.md"),
            lifecycle::TEST_DESIGN,
        )
        .unwrap();
        let stored = feature
            .execute(
                "memory_store",
                "old",
                serde_json::json!({"section":"Decisions","content":"old zircon decision"}),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        let old_id = stored.details["id"].as_str().unwrap().to_string();
        let old = feature
            .get_fact(old_id.clone(), CancellationToken::new())
            .await
            .unwrap()
            .unwrap();
        let args = serde_json::json!({"source_kind":"design-tree","authority":"explicit","section":"Decisions",
            "content":"Use transactions: Keep corrections atomic.","artifact_ref_type":"design",
            "artifact_ref_path":"docs/design/zircon.md","artifact_ref_sub":"Use transactions",
            "supersedes":old_id,"supersedes_version":old.version});
        let result = feature
            .execute(
                "memory_ingest_lifecycle",
                "correct",
                args.clone(),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        let replay = feature
            .execute(
                "memory_ingest_lifecycle",
                "correct",
                args,
                CancellationToken::new(),
            )
            .await
            .unwrap();
        let original = feature
            .get_fact(old.id, CancellationToken::new())
            .await
            .unwrap();
        assert!(
            bus.shutdown_managed_services()
                .await
                .all_resources_settled()
        );
        assert!(original.is_none());
        assert_eq!(result.details["id"], replay.details["id"]);
        assert_eq!(replay.details["replayed"], true);
        assert_eq!(result.details["source"]["artifact_id"], "zircon");
        assert!(result.details["version"].is_u64());
    }

    #[tokio::test]
    async fn lifecycle_inference_is_pending_and_not_recalled_as_knowledge() {
        let (feature, mut bus, _dir) = managed_feature().await;
        let result = feature.execute("memory_ingest_lifecycle", "inferred", serde_json::json!({
            "source_kind":"design-tree", "authority":"inferred", "section":"Decisions",
            "content":"zircon inferred success", "artifact_ref_type":"design",
            "artifact_ref_path":"docs/design/zircon.md", "artifact_ref_sub":"unverified-summary"
        }), CancellationToken::new()).await.unwrap();
        let recall = feature
            .execute(
                "memory_recall",
                "recall",
                serde_json::json!({"query":"zircon"}),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        assert!(
            bus.shutdown_managed_services()
                .await
                .all_resources_settled()
        );
        assert_eq!(result.details["status"], "pending");
        assert_eq!(recall.details["count"], 0);
    }

    #[tokio::test]
    async fn confirmation_tool_cannot_accept_agent_approval_flags_or_expose_internal_commit() {
        let (feature, mut bus, _dir) = managed_feature().await;
        let candidate=feature.execute("memory_ingest_lifecycle","candidate",serde_json::json!({"source_kind":"design-tree","authority":"inferred","section":"Decisions","content":"zircon inference"}),CancellationToken::new()).await.unwrap();
        let id = candidate.details["id"].as_str().unwrap();
        assert!(
            feature
                .execute(
                    "memory_confirm",
                    "review",
                    serde_json::json!({"candidate_id":id,"approved":true}),
                    CancellationToken::new()
                )
                .await
                .is_err()
        );
        let error = feature
            .execute(
                "memory_confirm",
                "review",
                serde_json::json!({"candidate_id":id}),
                CancellationToken::new(),
            )
            .await
            .unwrap_err();
        let review = error
            .downcast::<confirmation::MemoryConfirmationRequired>()
            .unwrap();
        assert!(
            feature
                .get_fact(id.into(), CancellationToken::new())
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            !feature
                .tools()
                .iter()
                .any(|tool| tool.name == crate::tool_registry::memory::MEMORY_APPLY_CONFIRMATION)
        );
        let mut public_bus = crate::bus::EventBus::new();
        public_bus.register(Box::new(MemoryFeature::new(
            Default::default(),
            "test".into(),
        )));
        public_bus.register_internal_tool(
            crate::tool_registry::memory::MEMORY_APPLY_CONFIRMATION,
            "memory",
        );
        assert!(
            public_bus
                .execute_tool(
                    crate::tool_registry::memory::MEMORY_APPLY_CONFIRMATION,
                    "forged",
                    serde_json::json!({}),
                    CancellationToken::new()
                )
                .await
                .is_err()
        );
        feature
            .execute(
                crate::tool_registry::memory::MEMORY_APPLY_CONFIRMATION,
                "kernel",
                serde_json::json!({"request":review.request,"surface":"native_event"}),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        assert!(
            feature
                .get_fact(id.into(), CancellationToken::new())
                .await
                .unwrap()
                .is_some()
        );
        let replay = feature
            .execute(
                "memory_confirm",
                "review",
                serde_json::json!({"candidate_id":id}),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(replay.details["replayed"], true);
        assert!(
            bus.shutdown_managed_services()
                .await
                .all_resources_settled()
        );
    }

    #[tokio::test]
    async fn confirmation_internal_dispatch_requires_runtime_principal() {
        let dir = tempfile::tempdir().unwrap();
        let binding = crate::memory_service::MemoryBinding::default();
        let mut feature = MemoryFeature::new(binding.clone(), "test".into());
        feature.on_event(&BusEvent::SessionStart {
            session_id: "fixture-session".into(),
            cwd: dir.path().into(),
        });
        let mut bus = crate::bus::EventBus::new();
        bus.register(Box::new(feature));
        bus.register_internal_tool(
            crate::tool_registry::memory::MEMORY_APPLY_CONFIRMATION,
            "memory",
        );
        let candidate =
            crate::memory_service::start_candidate(crate::memory_service::MemoryWorkerConfig {
                workspace_root: Some(dir.path().to_path_buf()),
                memory_token_cap: None,
                project_memory_root: dir.path().into(),
                project_db_path: dir.path().join("facts.db"),
                project_jsonl_path: dir.path().join("facts.jsonl"),
                global_db_path: None,
                vault: None,
                startup_sync_enabled: false,
            })
            .await
            .unwrap();
        bus.stage_managed_generation("memory", candidate).unwrap();
        bus.try_finalize_managed().await.unwrap();
        binding.capture(&bus).unwrap();
        let stored=bus.execute_tool("memory_ingest_lifecycle","candidate",serde_json::json!({"source_kind":"design-tree","authority":"inferred","section":"Decisions","content":"zircon inference"}),CancellationToken::new()).await.unwrap();
        let error = bus
            .execute_tool(
                "memory_confirm",
                "review",
                serde_json::json!({"candidate_id":stored.details["id"]}),
                CancellationToken::new(),
            )
            .await
            .unwrap_err();
        let review = error
            .downcast::<confirmation::MemoryConfirmationRequired>()
            .unwrap();
        let args = serde_json::json!({"request":review.request,"surface":"native_event"});
        assert!(
            bus.invoke_internal(
                crate::tool_registry::memory::MEMORY_APPLY_CONFIRMATION,
                "forged",
                args.clone(),
                CancellationToken::new(),
                Default::default()
            )
            .await
            .is_err()
        );
        let result = bus
            .invoke_internal(
                crate::tool_registry::memory::MEMORY_APPLY_CONFIRMATION,
                "kernel",
                args,
                CancellationToken::new(),
                crate::invocation_service::InvocationScope {
                    principal: "kernel:memory-confirmation".into(),
                    principal_class: omegon_traits::RuntimePrincipalClass::Internal,
                    surface: omegon_traits::RuntimeSurface::Internal,
                    ..Default::default()
                },
            )
            .await;
        assert!(
            bus.shutdown_managed_services()
                .await
                .all_resources_settled()
        );
        assert_eq!(result.unwrap().details["status"], "active");
    }

    #[tokio::test]
    async fn memory_query_large_store_reports_inventory_only() {
        let (feature, mut bus, _dir) = managed_feature().await;
        let cancel = tokio_util::sync::CancellationToken::new();

        for i in 0..201 {
            feature
                .execute(
                    "memory_store",
                    &format!("store-{i}"),
                    serde_json::json!({
                        "section": "Architecture",
                        "content": format!("Large store fact {i}")
                    }),
                    cancel.clone(),
                )
                .await
                .unwrap();
        }

        let result = feature
            .execute("memory_query", "query", serde_json::json!({}), cancel)
            .await
            .unwrap();

        let text = result.content[0].as_text().unwrap();
        assert!(text.contains("Large memory store detected"));
        assert!(text.contains("## Architecture (201 facts)"));
        assert!(!text.contains("Large store fact 0"));
        assert_eq!(result.details["inventory_only"], true);
        assert!(
            bus.shutdown_managed_services()
                .await
                .all_resources_settled()
        );
    }

    #[tokio::test]
    async fn working_memory_focus_release() {
        let (feature, mut bus, _dir) = managed_feature().await;
        let cancel = tokio_util::sync::CancellationToken::new();
        let mut ids = Vec::new();
        for index in 1..=3 {
            let stored = feature
                .execute(
                    "memory_store",
                    &format!("focus-store-{index}"),
                    serde_json::json!({"section": "Architecture", "content": format!("Focus fact {index}")}),
                    cancel.clone(),
                )
                .await
                .unwrap();
            ids.push(stored.details["id"].as_str().unwrap().to_string());
        }

        // Focus some fact IDs
        feature
            .execute(
                "memory_focus",
                "c1",
                serde_json::json!({"fact_ids": ids}),
                cancel.clone(),
            )
            .await
            .unwrap();

        {
            let wm = feature.working_memory.lock().unwrap();
            assert_eq!(wm.len(), 3);
        }

        // Release working memory
        feature
            .execute(
                "memory_release",
                "c2",
                serde_json::json!({}),
                cancel.clone(),
            )
            .await
            .unwrap();

        {
            let wm = feature.working_memory.lock().unwrap();
            assert!(wm.is_empty());
        }
        assert!(
            bus.shutdown_managed_services()
                .await
                .all_resources_settled()
        );
    }

    #[tokio::test]
    async fn memory_store_requests_harness_refresh_on_tool_end() {
        let (mut feature, mut bus, _dir) = managed_feature().await;
        let cancel = tokio_util::sync::CancellationToken::new();

        feature
            .execute(
                "memory_store",
                "c1",
                serde_json::json!({"section": "Architecture", "content": "System uses microservices"}),
                cancel,
            )
            .await
            .unwrap();

        let requests = feature.on_event(&BusEvent::ToolEnd {
            id: "c1".into(),
            name: crate::tool_registry::memory::MEMORY_STORE.into(),
            result: ToolResult {
                content: vec![],
                details: Value::Null,
            },
            is_error: false,
        });
        assert!(matches!(
            requests.as_slice(),
            [BusRequest::RefreshHarnessStatus]
        ));
        assert!(
            bus.shutdown_managed_services()
                .await
                .all_resources_settled()
        );
    }

    #[tokio::test]
    async fn memory_focus_requests_harness_refresh_on_tool_end() {
        let (mut feature, mut bus, _dir) = managed_feature().await;
        let cancel = tokio_util::sync::CancellationToken::new();
        let stored = feature
            .execute(
                "memory_store",
                "focus-refresh-store",
                serde_json::json!({"section": "Architecture", "content": "Focus refresh fact"}),
                cancel.clone(),
            )
            .await
            .unwrap();

        feature
            .execute(
                "memory_focus",
                "c1",
                serde_json::json!({"fact_ids": [stored.details["id"]]}),
                cancel,
            )
            .await
            .unwrap();

        let requests = feature.on_event(&BusEvent::ToolEnd {
            id: "c1".into(),
            name: crate::tool_registry::memory::MEMORY_FOCUS.into(),
            result: ToolResult {
                content: vec![],
                details: Value::Null,
            },
            is_error: false,
        });
        assert!(matches!(
            requests.as_slice(),
            [BusRequest::RefreshHarnessStatus]
        ));
        assert!(
            bus.shutdown_managed_services()
                .await
                .all_resources_settled()
        );
    }

    #[tokio::test]
    async fn memory_archive() {
        let (feature, mut bus, _dir) = managed_feature().await;
        let cancel = tokio_util::sync::CancellationToken::new();

        // Store a fact first
        let store_result = feature
            .execute(
                "memory_store",
                "c1",
                serde_json::json!({"section": "Architecture", "content": "Test fact to archive"}),
                cancel.clone(),
            )
            .await
            .unwrap();

        // Extract fact ID from store result
        let fact_id = store_result.details["id"].as_str().unwrap();

        // Archive it
        let archive_result = feature
            .execute(
                "memory_archive",
                "c2",
                serde_json::json!({"fact_ids": [fact_id]}),
                cancel.clone(),
            )
            .await
            .unwrap();

        assert!(
            archive_result.content[0]
                .as_text()
                .unwrap()
                .contains("Archived 1 fact(s)")
        );
        assert!(
            bus.shutdown_managed_services()
                .await
                .all_resources_settled()
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn ambient_context_preserves_pin_order_hash_dirty_ttl_and_priority() {
        let (feature, mut bus, _dir) = managed_feature().await;
        let cancel = tokio_util::sync::CancellationToken::new();
        let first = feature
            .execute(
                "memory_store",
                "context-first",
                serde_json::json!({"section": "Architecture", "content": "First ambient context fact"}),
                cancel.clone(),
            )
            .await
            .unwrap();
        let second = feature
            .execute(
                "memory_store",
                "context-second",
                serde_json::json!({"section": "Decisions", "content": "Second ambient context fact"}),
                cancel.clone(),
            )
            .await
            .unwrap();
        feature
            .execute(
                "memory_focus",
                "context-focus",
                serde_json::json!({"fact_ids": [second.details["id"], first.details["id"]]}),
                cancel.clone(),
            )
            .await
            .unwrap();

        let signals = ContextSignals {
            user_prompt: "",
            recent_tools: &[],
            recent_files: &[],
            lifecycle_phase: &LifecyclePhase::Idle,
            turn_number: 1,
            context_budget_tokens: 100_000,
        };
        let injection = feature.provide_context(&signals).expect("memory context");
        assert_eq!(injection.priority, 200);
        assert_eq!(injection.ttl_turns, 3);
        let second_position = injection
            .content
            .find("Second ambient context fact")
            .unwrap();
        let first_position = injection
            .content
            .find("First ambient context fact")
            .unwrap();
        assert!(second_position < first_position);
        assert!(feature.provide_context(&signals).is_none());

        let expired_signals = ContextSignals {
            turn_number: 4,
            ..signals
        };
        let reinjected = feature
            .provide_context(&expired_signals)
            .expect("unchanged memory reinjects after TTL");
        assert_eq!(reinjected.content, injection.content);

        feature
            .execute("memory_release", "context-release", Value::Null, cancel)
            .await
            .unwrap();
        let refreshed = feature
            .provide_context(&signals)
            .expect("dirty context refresh");
        assert!(!refreshed.content.contains("## Working Memory (pinned)"));
        assert!(
            bus.shutdown_managed_services()
                .await
                .all_resources_settled()
        );
    }

    #[tokio::test]
    async fn archive_and_supersede_replay_exact_outcomes_and_conflict_on_changed_payload() {
        let (mut feature, mut bus, _dir) = managed_feature().await;
        feature.on_event(&BusEvent::SessionStart {
            session_id: "replay-session".into(),
            cwd: std::path::PathBuf::from("."),
        });
        let cancel = tokio_util::sync::CancellationToken::new();
        let archived = feature
            .execute(
                "memory_store",
                "archive-store",
                serde_json::json!({"section": "Architecture", "content": "Replay archive fact"}),
                cancel.clone(),
            )
            .await
            .unwrap();
        let archive_args = serde_json::json!({"fact_ids": [archived.details["id"]]});
        let first = feature
            .execute(
                "memory_archive",
                "archive-call",
                archive_args.clone(),
                cancel.clone(),
            )
            .await
            .unwrap();
        let replay = feature
            .execute(
                "memory_archive",
                "archive-call",
                archive_args,
                cancel.clone(),
            )
            .await
            .unwrap();
        assert_eq!(first.content[0].as_text(), replay.content[0].as_text());
        assert_eq!(first.details, replay.details);
        let conflict = feature
            .execute(
                "memory_archive",
                "archive-call",
                serde_json::json!({"fact_ids": []}),
                cancel.clone(),
            )
            .await
            .unwrap_err();
        assert_eq!(conflict.to_string(), "memory:operation_conflict");

        let stored = feature
            .execute(
                "memory_store",
                "store-replay-call",
                serde_json::json!({"section": "Architecture", "content": "Exact store replay"}),
                cancel.clone(),
            )
            .await
            .unwrap();
        feature
            .execute(
                "memory_archive",
                "store-replay-archive",
                serde_json::json!({"fact_ids": [stored.details["id"]]}),
                cancel.clone(),
            )
            .await
            .unwrap();
        let replay = feature
            .execute(
                "memory_store",
                "store-replay-call",
                serde_json::json!({"section": "Architecture", "content": "Exact store replay"}),
                cancel.clone(),
            )
            .await
            .unwrap();
        assert_eq!(stored.content[0].as_text(), replay.content[0].as_text());
        assert_eq!(stored.details, replay.details);

        let original = feature
            .execute(
                "memory_store",
                "supersede-store",
                serde_json::json!({"section": "Architecture", "content": "Replay supersede original"}),
                cancel.clone(),
            )
            .await
            .unwrap();
        let supersede_args = serde_json::json!({
            "fact_id": original.details["id"],
            "section": "Decisions",
            "content": "Replay supersede replacement"
        });
        let first = feature
            .execute(
                "memory_supersede",
                "supersede-call",
                supersede_args.clone(),
                cancel.clone(),
            )
            .await
            .unwrap();
        let replacement_id = first.details["new_id"].as_str().unwrap().to_string();
        feature
            .execute(
                "memory_archive",
                "archive-replacement-call",
                serde_json::json!({"fact_ids": [replacement_id]}),
                cancel.clone(),
            )
            .await
            .unwrap();
        let replay = feature
            .execute(
                "memory_supersede",
                "supersede-call",
                supersede_args,
                cancel.clone(),
            )
            .await
            .unwrap();
        assert_eq!(first.content[0].as_text(), replay.content[0].as_text());
        assert_eq!(first.details, replay.details);
        let conflict = feature
            .execute(
                "memory_supersede",
                "supersede-call",
                serde_json::json!({
                    "fact_id": original.details["id"],
                    "section": "Decisions",
                    "content": "Changed replacement"
                }),
                cancel,
            )
            .await
            .unwrap_err();
        assert_eq!(conflict.to_string(), "memory:operation_conflict");
        assert!(
            bus.shutdown_managed_services()
                .await
                .all_resources_settled()
        );
    }

    #[tokio::test]
    async fn identical_call_ids_are_isolated_by_session_identity() {
        let (mut feature, mut bus, _dir) = managed_feature().await;
        let cancel = CancellationToken::new();
        feature.on_event(&BusEvent::SessionStart {
            session_id: "session-one".into(),
            cwd: ".".into(),
        });
        feature
            .execute(
                "memory_store",
                "same-call",
                serde_json::json!({"content": "first session", "section": "Architecture"}),
                cancel.clone(),
            )
            .await
            .unwrap();
        feature.on_event(&BusEvent::SessionStart {
            session_id: "session-two".into(),
            cwd: ".".into(),
        });
        feature
            .execute(
                "memory_store",
                "same-call",
                serde_json::json!({"content": "second session", "section": "Architecture"}),
                cancel,
            )
            .await
            .unwrap();
        assert!(
            bus.shutdown_managed_services()
                .await
                .all_resources_settled()
        );
    }

    #[tokio::test]
    async fn unavailable_and_cancelled_calls_preserve_typed_evidence() {
        let unavailable = MemoryFeature::new(Default::default(), "test".into())
            .execute(
                "memory_query",
                "absent",
                Value::Null,
                CancellationToken::new(),
            )
            .await
            .unwrap_err();
        assert_eq!(unavailable.to_string(), "memory:unavailable");

        let (feature, mut bus, _dir) = managed_feature().await;
        let cancellation = CancellationToken::new();
        cancellation.cancel();
        let cancelled = feature
            .execute("memory_query", "cancelled", Value::Null, cancellation)
            .await
            .unwrap_err();
        assert_eq!(cancelled.to_string(), "memory:cancelled");
        assert!(
            bus.shutdown_managed_services()
                .await
                .all_resources_settled()
        );
    }

    #[tokio::test]
    async fn committed_mutation_refresh_uses_independent_cancellation() {
        let (feature, mut bus, dir) = managed_feature().await;
        let original = CancellationToken::new();
        feature
            .apply_mutation(
                "refresh-race-store".into(),
                MemoryMutation::StoreFact {
                    request: StoreFact {
                        mind: "test".into(),
                        content: "Committed before caller cancellation".into(),
                        section: Section::Architecture,
                        decay_profile: DecayProfileName::Standard,
                        source: None,
                    },
                },
                original.clone(),
            )
            .await
            .unwrap();
        original.cancel();
        feature.refresh_status().await;
        let snapshot = crate::status::managed_memory_status_snapshot_for(dir.path());
        assert!(snapshot.available);
        assert_eq!(snapshot.status.total_facts, 1);
        assert!(
            bus.shutdown_managed_services()
                .await
                .all_resources_settled()
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn ambient_context_honors_render_budget_and_focus_limit() {
        let (feature, mut bus, _dir) = managed_feature().await;
        let cancel = CancellationToken::new();
        feature
            .execute(
                "memory_store",
                "budget-store",
                serde_json::json!({"section": "Architecture", "content": "A long ambient fact that must be bounded by the supplied context budget"}),
                cancel.clone(),
            )
            .await
            .unwrap();
        let signals = ContextSignals {
            user_prompt: "ambient fact",
            recent_tools: &[],
            recent_files: &[],
            lifecycle_phase: &LifecyclePhase::Idle,
            turn_number: 1,
            context_budget_tokens: 200,
        };
        let injection = feature.provide_context(&signals).expect("bounded context");
        assert!(injection.content.len() <= 200);

        let tiny_signals = ContextSignals {
            turn_number: 2,
            context_budget_tokens: 1,
            ..signals
        };
        let cleared = feature
            .provide_context(&tiny_signals)
            .expect("smaller budget must retire the previous live injection");
        assert!(cleared.content.is_empty());
        assert!(feature.provide_context(&tiny_signals).is_none());

        feature
            .execute(
                "memory_focus",
                "unresolved-pin",
                serde_json::json!({"fact_ids": ["session-local-unresolved"]}),
                cancel.clone(),
            )
            .await
            .unwrap();
        assert_eq!(
            feature.working_memory.lock().unwrap().as_slice(),
            ["session-local-unresolved"]
        );
        feature.working_memory.lock().unwrap().clear();

        let too_many = (0..=crate::memory_service::MAX_CONTEXT_PINS)
            .map(|index| format!("fact-{index}"))
            .collect::<Vec<_>>();
        let error = feature
            .execute(
                "memory_focus",
                "too-many-pins",
                serde_json::json!({"fact_ids": too_many}),
                cancel,
            )
            .await
            .unwrap_err();
        assert!(error.to_string().contains("pin limit"));
        assert!(feature.working_memory.lock().unwrap().is_empty());
        assert!(
            bus.shutdown_managed_services()
                .await
                .all_resources_settled()
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn concurrent_focus_updates_enforce_pin_limit_atomically() {
        let (feature, mut bus, _dir) = managed_feature().await;
        let feature = Arc::new(feature);
        let request = |prefix: &str| {
            (0..600)
                .map(|index| format!("{prefix}-{index}"))
                .collect::<Vec<_>>()
        };
        let first = {
            let feature = feature.clone();
            let ids = request("first");
            tokio::spawn(async move {
                feature
                    .execute(
                        "memory_focus",
                        "concurrent-focus-first",
                        serde_json::json!({"fact_ids": ids}),
                        CancellationToken::new(),
                    )
                    .await
            })
        };
        let second = {
            let feature = feature.clone();
            let ids = request("second");
            tokio::spawn(async move {
                feature
                    .execute(
                        "memory_focus",
                        "concurrent-focus-second",
                        serde_json::json!({"fact_ids": ids}),
                        CancellationToken::new(),
                    )
                    .await
            })
        };
        let (first, second) = tokio::join!(first, second);
        let outcomes = [first.unwrap(), second.unwrap()];
        assert_eq!(outcomes.iter().filter(|result| result.is_ok()).count(), 1);
        assert_eq!(outcomes.iter().filter(|result| result.is_err()).count(), 1);
        assert_eq!(feature.working_memory.lock().unwrap().len(), 600);
        assert!(
            bus.shutdown_managed_services()
                .await
                .all_resources_settled()
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn session_end_pipeline_completes_on_current_thread_runtime() {
        let (mut feature, mut bus, _dir) = managed_feature().await;
        feature.on_event(&BusEvent::SessionStart {
            session_id: "current-thread-session".into(),
            cwd: std::path::PathBuf::from("."),
        });
        let start = std::time::Instant::now();
        feature.on_event(&BusEvent::SessionEnd {
            turns: 1,
            tool_calls: 2,
            duration_secs: 3.0,
            initial_prompt: Some("uncommitted-advisory-prompt".into()),
            outcome_summary: Some("uncommitted-advisory-outcome".into()),
        });
        assert!(start.elapsed() < std::time::Duration::from_millis(100));
        let mut episode_count = 0;
        for _ in 0..50 {
            let payload = feature
                .invoke(crate::memory_service::MemoryRequestV1::ListEpisodes {
                    scope: crate::memory_service::MemoryScopeV1::Project,
                    mind: "test".into(),
                    limit: 10,
                    cancellation: CancellationToken::new(),
                })
                .await
                .unwrap();
            let crate::memory_service::MemoryPayloadV1::Episodes(episodes) = payload else {
                panic!("episode payload");
            };
            episode_count = episodes.len();
            if episode_count == 1 {
                assert!(!episodes[0].narrative.contains("uncommitted-advisory"));
                assert!(matches!(
                    episodes[0].formation.as_ref().unwrap().source,
                    omegon_memory::FormationSource::Unavailable { .. }
                ));
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        assert_eq!(episode_count, 1);
        feature.prepare_managed_shutdown().await.unwrap();
        assert!(
            bus.shutdown_managed_services()
                .await
                .all_resources_settled()
        );
    }

    #[test]
    fn feature_retains_only_managed_binding_and_mind() {
        let feature = MemoryFeature::new(Default::default(), "test".into());
        assert_eq!(feature.mind(), "test");
        assert!(!feature.memory_binding.available());
    }
}
