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
    Active,
    Archived,
    Superseded,
}

// ─── Core records ───────────────────────────────────────────────────────────

/// A memory fact. Mirrors FactRecord in api-types.ts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fact {
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


/// A fact with search scoring attached.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoredFact {
    #[serde(flatten)]
    pub fact: Fact,
    /// Raw cosine similarity (0.0–1.0), or FTS5 rank score.
    pub similarity: f64,
    /// Combined score: similarity × decay-adjusted confidence.
    pub score: f64,
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
    pub weight: f64,
    pub created_at: String,
}

// ─── Request/response types ─────────────────────────────────────────────────

/// Request to store a new fact.
#[derive(Debug, Clone)]
pub struct StoreFact {
    pub mind: String,
    pub content: String,
    pub section: Section,
    pub decay_profile: DecayProfileName,
    pub source: Option<String>,
}

/// Result of storing a fact — what happened.
#[derive(Debug, Clone)]
pub struct StoreResult {
    pub fact: Fact,
    pub action: StoreAction,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreAction {
    Stored,
    Reinforced,
    Deduplicated,
}

/// Filter for listing facts.
#[derive(Debug, Clone, Default)]
pub struct FactFilter {
    pub section: Option<Section>,
    pub status: Option<FactStatus>,
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
#[derive(Debug, Clone)]
pub struct CreateEdge {
    pub source_id: String,
    pub target_id: String,
    pub relation: String,
    pub description: Option<String>,
}

/// Request to store an episode.
#[derive(Debug, Clone)]
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
}

/// Stats from a JSONL import.
#[derive(Debug, Clone, Default)]
pub struct ImportStats {
    pub imported: usize,
    pub reinforced: usize,
    pub skipped: usize,
    pub errors: usize,
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

/// Minimal fact representation in the JSONL transport format.
/// The JSONL contains a subset of the full Fact fields — DB-only fields
/// (confidence, reinforcement_count, decay_rate, etc.) are NOT in the JSONL.
/// These are reconstructed from defaults on import.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonlFact {
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
        };
        let record = JsonlRecord::Fact(fact);
        let json = serde_json::to_string(&record).unwrap();
        assert!(json.contains(r#""_type":"fact"#), "should use _type: {json}");

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
        // This is the actual format from .pi/memory/facts.jsonl
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
