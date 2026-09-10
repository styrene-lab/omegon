//! Memory system types — mirrors api-types.ts exactly.
//!
//! Field names are snake_case matching the TypeScript interfaces.
//! Any deviation from api-types.ts is a bug.

use serde::{Deserialize, Serialize};

// ─── Section names ──────────────────────────────────────────────────────────

/// Memory sections — the top-level organizational categories.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Section {
    #[serde(rename = "Architecture")]
    Architecture,
    #[serde(rename = "Decisions")]
    Decisions,
    #[serde(rename = "Constraints")]
    Constraints,
    #[serde(rename = "Known Issues")]
    KnownIssues,
    #[serde(rename = "Patterns & Conventions")]
    PatternsConventions,
    #[serde(rename = "Specs")]
    Specs,
    #[serde(rename = "Recent Work")]
    RecentWork,
}

impl Section {
    pub fn all() -> &'static [Section] {
        &[
            Section::Architecture,
            Section::Decisions,
            Section::Constraints,
            Section::KnownIssues,
            Section::PatternsConventions,
            Section::Specs,
            Section::RecentWork,
        ]
    }
}

// ─── Fact status ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FactStatus {
    Pending,
    Active,
    Dormant,
    Archived,
    Superseded,
}

// ─── Core records ───────────────────────────────────────────────────────────

/// A memory fact. Mirrors FactRecord in api-types.ts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fact {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lifecycle_inference: Option<Box<LifecycleInference>>,
    pub id: String,
    pub mind: String,
    pub content: String,
    pub section: Section,
    pub status: FactStatus,
    pub confidence: f64,
    pub reinforcement_count: u32,
    pub decay_rate: f64,
    pub decay_profile: DecayProfileName,
    pub last_reinforced: String, // ISO 8601
    pub created_at: String,      // ISO 8601
    #[serde(default)]
    pub version: u64, // Lamport clock
    #[serde(skip_serializing_if = "Option::is_none")]
    pub superseded_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// Content hash for deduplication (16-char truncated sha256 hex).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_hash: Option<String>,
    /// Set when this fact was last accessed by a recall/search operation.
    /// Used for soft decay timer reset.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_accessed: Option<String>,
    /// Session ID that created this fact.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_session: Option<String>,
    /// When the fact was superseded (ISO 8601).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub superseded_at: Option<String>,
    /// When the fact was archived (ISO 8601).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub archived_at: Option<String>,
    /// jj change ID that created this fact (permanent, survives rebase).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jj_change_id: Option<String>,
    /// Persona ID that owns this fact. NULL = project fact (default).
    /// When a persona is active and a fact is stored into its mind layer,
    /// this records which persona owns it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub persona_id: Option<String>,
    /// Memory layer: 'project' (default), 'persona', 'working'.
    /// Controls injection priority and lifecycle.
    #[serde(default = "default_layer")]
    pub layer: String,
    /// Searchable tags for domain classification (e.g. ["pcb", "thermal"]).
    /// Used by persona mind stores for filtered queries.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
}

fn default_layer() -> String {
    "project".into()
}

/// Decay profile discriminant — persisted in DB.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum DecayProfileName {
    #[default]
    Standard,
    Global,
    RecentWork,
}

/// Recorded provenance class; not a permission or execution proof.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProvenanceBasis {
    LegacyUnknown,
    ExplicitArtifact,
    UnconfirmedInference,
    OperatorConfirmedInference,
    InvalidMetadata,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceAvailability {
    NotChecked,
    NoReference,
    SnapshotMatches,
    SnapshotChanged,
    ReadableUnverified,
    Unavailable,
    UnsupportedReference,
}

/// Read-only fact/provenance projection. Excerpts are explicitly bounded previews.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FactInspection {
    pub id: String,
    pub mind: String,
    pub section: Section,
    pub status: FactStatus,
    pub version: u64,
    pub content_excerpt: String,
    pub content_truncated: bool,
    pub content_sha256: String,
    pub created_at: String,
    pub created_session: Option<String>,
    pub last_reinforced: String,
    pub reinforcement_count: u32,
    pub confidence: f64,
    pub supersedes: Option<String>,
    pub superseded_at: Option<String>,
    pub archived_at: Option<String>,
    pub basis: ProvenanceBasis,
    pub artifact: Option<Box<LifecycleConclusionSource>>,
    pub inference: Option<Box<LifecycleInference>>,
    pub source_excerpt: Option<String>,
    pub source_truncated: bool,
    pub evidence_availability: EvidenceAvailability,
    pub diagnostic: Option<String>,
}

/// An explicit embedding-space identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmbeddingSpace {
    pub model: String,
    pub revision: String,
    pub preprocessing: String,
    pub dimensions: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IdentifiedEmbedding {
    pub space: EmbeddingSpace,
    pub values: Vec<f32>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct VectorDiagnostics {
    pub compatible: usize,
    pub legacy: usize,
    pub incompatible: usize,
    pub stale: usize,
    pub identity_unavailable: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VectorSearchReport {
    pub results: Vec<ScoredFact>,
    pub diagnostics: VectorDiagnostics,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EmbeddingIndexState {
    Missing,
    Legacy,
    Incompatible,
    Stale,
    Ready,
}

/// A fact with search scoring attached.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoredFact {
    #[serde(flatten)]
    pub fact: Fact,
    /// Legacy raw cosine (-1.0–1.0), lexical, or proximity value. Prefer named scores.
    pub similarity: f64,
    /// Ranking value, potentially fused or graph-derived. Not a probability.
    pub score: f64,
    #[serde(default)]
    pub scores: RetrievalScores,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub graph_evidence: Vec<GraphEvidence>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RetrievalScores {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lexical: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cosine: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rrf: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub graph: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GraphRelationKind {
    Related,
    Support,
    Contradiction,
    Supersession,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphEvidence {
    pub edge_id: String,
    pub other_fact_id: String,
    pub relation: String,
    pub outgoing: bool,
    pub kind: GraphRelationKind,
}

impl ScoredFact {
    pub fn new(fact: Fact, similarity: f64, score: f64) -> Self {
        Self {
            fact,
            similarity,
            score,
            scores: Default::default(),
            graph_evidence: vec![],
        }
    }
}

/// A session episode narrative.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Episode {
    pub id: String,
    pub mind: String,
    pub date: String,
    pub title: String,
    pub narrative: String,
    pub created_at: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub affected_nodes: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub affected_changes: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files_changed: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls_count: Option<u32>,
    /// jj change ID that created this episode (permanent, survives rebase).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jj_change_id: Option<String>,
    /// Bounded source-linked formation evidence. Candidates are not admitted facts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub formation: Option<Box<EpisodeFormation>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum FormationSource {
    Available {
        session_id: String,
        stream_id: String,
        sequence: u64,
        event_id: String,
    },
    Unavailable {
        session_id: String,
        reason: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceKind {
    UserStatement,
    AssistantReport,
    ToolResult,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceOutcome {
    Succeeded,
    Failed,
    Denied,
    NotDispatched,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FormationEvidence {
    pub event_id: String,
    pub sequence: u64,
    pub recorded_at: String,
    pub kind: EvidenceKind,
    pub excerpt: String,
    pub truncated: bool,
    pub outcome: Option<EvidenceOutcome>,
}

/// An unadmitted model inference supported by references into the bounded evidence set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryCandidate {
    pub content: String,
    pub section: Section,
    pub evidence_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ExtractionOutcome {
    Disabled,
    Pending { model: String },
    Complete { model: String },
    Unavailable { model: String, reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EpisodeFormation {
    pub version: u16,
    pub source: FormationSource,
    pub evidence: Vec<FormationEvidence>,
    pub candidates: Vec<MemoryCandidate>,
    pub extraction: ExtractionOutcome,
    pub truncated: bool,
    pub rejected_candidates: usize,
}

/// A directional relationship between two facts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Edge {
    pub id: String,
    pub source_id: String,
    pub target_id: String,
    pub relation: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Edge confidence (1.0 = full). Replaces legacy `weight` field.
    #[serde(alias = "weight")]
    pub confidence: f64,
    pub created_at: String,
}

// ─── Request/response types ─────────────────────────────────────────────────

/// Request to store a new fact.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StoreFact {
    pub mind: String,
    pub content: String,
    pub section: Section,
    pub decay_profile: DecayProfileName,
    pub source: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MaintenanceReport {
    pub mind: String,
    pub active_facts: usize,
    pub candidates: Vec<MaintenanceCandidate>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MaintenanceCandidate {
    pub id: String,
    pub effective_confidence: f64,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MaintenanceApplyResult {
    pub requested: usize,
    pub transitioned: usize,
}

/// Result of storing a fact — what happened.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoreResult {
    pub fact: Fact,
    pub action: StoreAction,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum StoreAction {
    Stored,
    Reinforced,
    Deduplicated,
}

/// Filter for listing facts.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FactFilter {
    pub section: Option<Section>,
    pub status: Option<FactStatus>,
}

/// Retrieval population. Historical evidence remains searchable regardless of decay.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchIntent {
    #[default]
    Current,
    Historical,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchFilter {
    #[serde(default)]
    pub intent: SearchIntent,
    #[serde(default)]
    pub section: Option<Section>,
}

impl SearchFilter {
    pub fn matches(&self, fact: &Fact) -> bool {
        let status_matches = match self.intent {
            SearchIntent::Current => fact.status == FactStatus::Active,
            SearchIntent::Historical => matches!(
                fact.status,
                FactStatus::Archived | FactStatus::Dormant | FactStatus::Superseded
            ),
        };
        status_matches
            && self
                .section
                .as_ref()
                .is_none_or(|section| section == &fact.section)
    }

    pub fn score(&self, relevance: f64, fact: &Fact) -> Option<f64> {
        if !self.matches(fact) {
            return None;
        }
        match self.intent {
            SearchIntent::Current => crate::decay::ambient_score(relevance, fact),
            SearchIntent::Historical => Some(relevance),
        }
    }
}

/// One bounded keyset page over facts present at the first page's Lamport
/// watermark. Facts inserted after that watermark are intentionally deferred
/// to a later scan; status changes may remove facts but cannot duplicate them.
#[derive(Debug, Clone)]
pub struct FactPage {
    pub facts: Vec<Fact>,
    pub next_cursor: Option<String>,
    pub total: usize,
}

/// Request for context injection rendering.
#[derive(Debug, Clone)]
pub struct ContextRequest {
    pub mind: String,
    pub query: Option<String>,
    pub working_memory: Vec<String>,
    pub max_chars: usize,
    pub episodes: usize,
    pub include_global: bool,
}

impl Default for ContextRequest {
    fn default() -> Self {
        Self {
            mind: String::new(),
            query: None,
            working_memory: Vec::new(),
            max_chars: 12_000,
            episodes: 1,
            include_global: false,
        }
    }
}

/// Pre-rendered context block ready for system prompt injection.
#[derive(Debug, Clone)]
pub struct RenderedContext {
    pub markdown: String,
    pub facts_injected: usize,
    pub episodes_injected: usize,
    pub char_count: usize,
    pub budget_exhausted: bool,
}

/// Request to create an edge.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CreateEdge {
    pub source_id: String,
    pub target_id: String,
    pub relation: String,
    pub description: Option<String>,
}

/// Request to store an episode.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StoreEpisode {
    pub mind: String,
    pub title: String,
    pub narrative: String,
    pub date: Option<String>,
    pub affected_nodes: Vec<String>,
    pub affected_changes: Vec<String>,
    pub files_changed: Vec<String>,
    pub tags: Vec<String>,
    pub tool_calls_count: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub formation: Option<Box<EpisodeFormation>>,
}

/// Stats from a JSONL import.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImportStats {
    pub imported: usize,
    pub reinforced: usize,
    pub skipped: usize,
    pub errors: usize,
}

/// Entity-specific optimistic precondition for a targeted mutation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FactPrecondition {
    pub id: String,
    pub expected_version: u64,
}

/// A durable memory mutation whose stable operation identity is supplied by
/// [`MemoryBackend::apply_mutation`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MemoryMutation {
    ConfirmLifecycleCandidate {
        candidate: FactPrecondition,
        snapshot_hash: String,
        session_id: String,
        request_id: String,
        surface: ConfirmationSurface,
        supersedes: Option<FactPrecondition>,
    },
    StoreLifecycleConclusion {
        request: StoreFact,
        source: Box<crate::lifecycle::LifecycleConclusionSource>,
        supersedes: Option<FactPrecondition>,
    },
    StoreLifecycleInference {
        request: StoreFact,
        inference: Box<LifecycleInference>,
    },
    ImportJsonl {
        jsonl: String,
    },
    StoreFact {
        request: StoreFact,
    },
    ReinforceFact {
        fact: FactPrecondition,
    },
    ReinforceFactOnce {
        fact_id: String,
    },
    TransitionFacts {
        facts: Vec<FactPrecondition>,
        status: FactStatus,
    },
    SupersedeFact {
        fact: FactPrecondition,
        replacement: StoreFact,
    },
    SupersedeFactWithExisting {
        fact: FactPrecondition,
        replacement: FactPrecondition,
    },
    StoreEmbedding {
        fact: FactPrecondition,
        model_name: String,
        embedding: Vec<f32>,
    },
    StoreIdentifiedEmbedding {
        fact: FactPrecondition,
        embedding: IdentifiedEmbedding,
    },
    CreateEdge {
        mind: String,
        request: CreateEdge,
    },
    StoreEpisode {
        request: StoreEpisode,
    },
    CompleteFormation {
        episode_id: String,
        formation: Box<EpisodeFormation>,
    },
}

/// Compact durable effect recorded for operation replay. Fact content and
/// vectors are not duplicated into the operation receipt table.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MemoryMutationEffect {
    JsonlImported {
        imported: usize,
        reinforced: usize,
        skipped: usize,
        errors: usize,
    },
    FactStored {
        fact_id: String,
        version: u64,
        action: StoreAction,
    },
    FactReinforced {
        fact_id: String,
        version: u64,
        reinforcement_count: u32,
    },
    FactsTransitioned {
        facts: Vec<FactPrecondition>,
        status: FactStatus,
    },
    FactSuperseded {
        original: FactPrecondition,
        replacement: FactPrecondition,
    },
    EmbeddingStored {
        fact_id: String,
        model_name: String,
        dims: u32,
    },
    EdgeCreated {
        edge_id: String,
    },
    EpisodeStored {
        episode_id: String,
    },
    FormationCompleted {
        episode_id: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryMutationOutcome {
    pub effect: MemoryMutationEffect,
    pub replayed: bool,
}

// ─── JSONL wire format ──────────────────────────────────────────────────────

/// A single line in the JSONL git-sync format.
/// Discriminated on `_type` (not `type`) to match the existing JSONL files on disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "_type")]
pub enum JsonlRecord {
    #[serde(rename = "fact")]
    Fact(JsonlFact),
    #[serde(rename = "episode")]
    Episode(Episode),
    #[serde(rename = "edge")]
    Edge(Edge),
    #[serde(rename = "mind")]
    Mind(MindRecord),
}

/// Fact transport. Legacy records omit operational state; modern exports retain it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonlFact {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lifecycle_inference: Option<Box<LifecycleInference>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operational: Option<Box<FactOperationalState>>,
    pub id: String,
    pub mind: String,
    pub content: String,
    pub section: Section,
    pub status: FactStatus,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_hash: Option<String>,
    /// In the JSONL, `supersedes` means "this fact supersedes fact Y".
    /// Mapped to `Fact.superseded_by` (inverse perspective) on import.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supersedes: Option<String>,
    /// Lamport version for conflict resolution. Default 0 for legacy files.
    #[serde(default)]
    pub version: u64,
    /// Decay profile — additive field, default "standard" for legacy facts.
    #[serde(default)]
    pub decay_profile: DecayProfileName,
    /// Persona ID that owns this fact. NULL = project fact.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub persona_id: Option<String>,
    /// Memory layer: 'project' (default), 'persona', 'working'.
    #[serde(default = "default_layer")]
    pub layer: String,
    /// Searchable tags for domain classification.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
}

/// Supported classes of explicit structured lifecycle conclusion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LifecycleConclusionKind {
    Decision,
    Constraint,
    Specification,
}

/// Portable attribution to the exact artifact snapshot parsed by the host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LifecycleConclusionSource {
    pub kind: LifecycleConclusionKind,
    pub artifact_path: String,
    pub artifact_id: Option<String>,
    pub artifact_sub: String,
    pub artifact_sha256: String,
    pub statement_sha256: String,
}

/// Attribution supplied with an unconfirmed lifecycle summary, not verified evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LifecycleInference {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confirmation: Option<CandidateConfirmation>,
    pub source_kind: String,
    pub artifact_ref_type: Option<String>,
    pub artifact_ref_path: Option<String>,
    pub artifact_ref_sub: Option<String>,
    /// Proposed correction only; admission must not apply it before confirmation.
    pub proposed_supersedes: Option<String>,
}

impl LifecycleInference {
    pub fn validate(&self) -> crate::backend::Result<()> {
        if let Some(confirmation) = &self.confirmation {
            confirmation.validate()?;
        }
        for value in [
            Some(&self.source_kind),
            self.artifact_ref_type.as_ref(),
            self.artifact_ref_path.as_ref(),
            self.artifact_ref_sub.as_ref(),
            self.proposed_supersedes.as_ref(),
        ]
        .into_iter()
        .flatten()
        {
            if value.trim().is_empty() || value.len() > 2048 || value.chars().any(char::is_control)
            {
                return Err(crate::MemoryError::InvalidMutation(
                    "invalid lifecycle inference reference".into(),
                ));
            }
        }
        Ok(())
    }
}

pub(crate) fn validate_inference_status(
    status: &FactStatus,
    inference: Option<&LifecycleInference>,
    content: &str,
) -> crate::backend::Result<()> {
    if (*status == FactStatus::Pending)
        != inference.is_some_and(|inference| inference.confirmation.is_none())
    {
        return Err(crate::MemoryError::InvalidMutation(
            "lifecycle inferences must remain pending".into(),
        ));
    }
    if let Some(inference) = inference {
        inference.validate()?;
        if inference.confirmation.as_ref().is_some_and(|confirmation| {
            confirmation.content_sha256 != crate::retrieval::raw_content_hash(content)
        }) {
            return Err(crate::MemoryError::InvalidMutation(
                "confirmed content does not match operator review".into(),
            ));
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfirmationSurface {
    /// Shared native agent-event approval channel; not a specific frontend identity.
    NativeEvent,
    Acp,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateConfirmation {
    pub reviewed_version: u64,
    pub snapshot_sha256: String,
    pub session_id: String,
    pub request_id: String,
    pub surface: ConfirmationSurface,
    pub confirmed_at: String,
    pub content_sha256: String,
}

impl CandidateConfirmation {
    pub fn validate(&self) -> crate::backend::Result<()> {
        for value in [&self.session_id, &self.request_id] {
            if value.is_empty() || value.len() > 2048 || value.chars().any(char::is_control) {
                return Err(crate::MemoryError::InvalidMutation(
                    "invalid confirmation provenance".into(),
                ));
            }
        }
        if self.reviewed_version > i64::MAX as u64
            || self.snapshot_sha256.len() != 64
            || !self
                .snapshot_sha256
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
            || chrono::DateTime::parse_from_rfc3339(&self.confirmed_at).is_err()
            || self.content_sha256.len() != 64
            || !self
                .content_sha256
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(crate::MemoryError::InvalidMutation(
                "invalid confirmation timestamp or digest".into(),
            ));
        }
        Ok(())
    }
}

pub(crate) fn validate_inference_import(
    existing: Option<&LifecycleInference>,
    incoming: Option<&LifecycleInference>,
    status: &FactStatus,
) -> crate::backend::Result<()> {
    if let Some(existing) = existing {
        if existing.confirmation.is_none()
            && (*status != FactStatus::Pending
                || incoming.is_some_and(|inference| inference.confirmation.is_some()))
        {
            return Err(crate::MemoryError::InvalidMutation(
                "transport cannot confirm lifecycle inference".into(),
            ));
        }
        if existing.confirmation.is_some() && incoming != Some(existing) {
            return Err(crate::MemoryError::InvalidMutation(
                "transport cannot rewrite operator confirmation attribution".into(),
            ));
        }
    }
    Ok(())
}

/// Persisted operational state, transported without reinforcement or clock resets.
/// Absence on a legacy record means unknown, not a request to reset existing state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FactOperationalState {
    pub confidence: f64,
    pub reinforcement_count: u32,
    pub decay_rate: f64,
    pub last_reinforced: String,
    pub last_accessed: Option<String>,
    pub created_session: Option<String>,
    pub superseded_at: Option<String>,
    pub archived_at: Option<String>,
    pub jj_change_id: Option<String>,
}

impl From<&Fact> for FactOperationalState {
    fn from(fact: &Fact) -> Self {
        Self {
            confidence: fact.confidence,
            reinforcement_count: fact.reinforcement_count,
            decay_rate: fact.decay_rate,
            last_reinforced: fact.last_reinforced.clone(),
            last_accessed: fact.last_accessed.clone(),
            created_session: fact.created_session.clone(),
            superseded_at: fact.superseded_at.clone(),
            archived_at: fact.archived_at.clone(),
            jj_change_id: fact.jj_change_id.clone(),
        }
    }
}

impl FactOperationalState {
    pub fn validate(&self) -> crate::backend::Result<()> {
        if !self.confidence.is_finite()
            || !(0.0..=1.0).contains(&self.confidence)
            || !self.decay_rate.is_finite()
            || self.decay_rate < 0.0
        {
            return Err(crate::MemoryError::InvalidMutation(
                "invalid fact operational numeric state".into(),
            ));
        }
        for time in [
            Some(&self.last_reinforced),
            self.last_accessed.as_ref(),
            self.superseded_at.as_ref(),
            self.archived_at.as_ref(),
        ]
        .into_iter()
        .flatten()
        {
            if chrono::DateTime::parse_from_rfc3339(time).is_err() {
                return Err(crate::MemoryError::InvalidMutation(
                    "invalid fact operational timestamp".into(),
                ));
            }
        }
        Ok(())
    }

    pub(crate) fn apply_to(self, fact: &mut Fact) {
        fact.confidence = self.confidence;
        fact.reinforcement_count = self.reinforcement_count;
        fact.decay_rate = self.decay_rate;
        fact.last_reinforced = self.last_reinforced;
        fact.last_accessed = self.last_accessed;
        fact.created_session = self.created_session;
        fact.superseded_at = self.superseded_at;
        fact.archived_at = self.archived_at;
        fact.jj_change_id = self.jj_change_id;
    }
}

/// Mind record in the JSONL transport.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MindRecord {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
}

// ─── Embedding metadata ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingMetadata {
    pub model_name: String,
    pub dims: u32,
    pub inserted_at: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jsonl_fact_round_trip() {
        let fact = JsonlFact {
            lifecycle_inference: None,
            operational: None,
            id: "abc123".into(),
            mind: "default".into(),
            content: "Some architecture fact".into(),
            section: Section::Architecture,
            status: FactStatus::Active,
            created_at: "2026-03-18T00:00:00Z".into(),
            source: Some("extraction".into()),
            content_hash: Some("1234567890abcdef".into()),
            supersedes: None,
            version: 0,
            decay_profile: DecayProfileName::Standard,
            persona_id: None,
            layer: "project".into(),
            tags: vec![],
        };
        let record = JsonlRecord::Fact(fact);
        let json = serde_json::to_string(&record).unwrap();
        assert!(
            json.contains(r#""_type":"fact"#),
            "should use _type: {json}"
        );

        let parsed: JsonlRecord = serde_json::from_str(&json).unwrap();
        match parsed {
            JsonlRecord::Fact(f) => {
                assert_eq!(f.id, "abc123");
                assert_eq!(f.section, Section::Architecture);
            }
            _ => panic!("expected Fact variant"),
        }
    }

    #[test]
    fn jsonl_deserializes_real_file_format() {
        // This is the actual format from .omegon/memory/facts.jsonl
        let line = r#"{"_type":"fact","id":"scQZ59OF3fPW","mind":"default","section":"Architecture","content":"Some fact","status":"active","created_at":"2026-03-04T05:30:13.976Z","source":"extraction","content_hash":"497f84b1d8aecb70","supersedes":"JngamqHkF69o"}"#;
        let record: JsonlRecord = serde_json::from_str(line).unwrap();
        match record {
            JsonlRecord::Fact(f) => {
                assert_eq!(f.id, "scQZ59OF3fPW");
                assert_eq!(f.supersedes, Some("JngamqHkF69o".into()));
                assert_eq!(f.version, 0); // default for missing field
                assert_eq!(f.decay_profile, DecayProfileName::Standard); // default
            }
            _ => panic!("expected Fact"),
        }
    }

    #[test]
    fn jsonl_mind_record() {
        let line = r#"{"_type":"mind","name":"project-x","description":"A test project"}"#;
        let record: JsonlRecord = serde_json::from_str(line).unwrap();
        match record {
            JsonlRecord::Mind(m) => assert_eq!(m.name, "project-x"),
            _ => panic!("expected Mind"),
        }
    }

    #[test]
    fn section_serde_preserves_display_names() {
        let s = Section::KnownIssues;
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(json, r#""Known Issues""#);
        let parsed: Section = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, Section::KnownIssues);
    }

    #[test]
    fn decay_profile_name_defaults_to_standard() {
        let name: DecayProfileName = Default::default();
        assert_eq!(name, DecayProfileName::Standard);
    }

    #[test]
    fn fact_status_snake_case() {
        let s = FactStatus::Active;
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(json, r#""active""#);
    }
}
