//! Shared lifecycle types — design nodes, openspec changes, specs.
//!
//! These types mirror the TypeScript definitions in:
//! - extensions/design-tree/types.ts
//! - extensions/openspec/types.ts
//!
//! Phase 1a: read-only structs for parsing. Phase 1b: mutation methods.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// ─── Design-Tree Types ──────────────────────────────────────────────────────

/// Status of a design node in the exploration lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeStatus {
    Seed,
    Exploring,
    Resolved,
    Decided,
    Implementing,
    Implemented,
    Blocked,
    Deferred,
}

impl NodeStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Seed => "seed",
            Self::Exploring => "exploring",
            Self::Resolved => "resolved",
            Self::Decided => "decided",
            Self::Implementing => "implementing",
            Self::Implemented => "implemented",
            Self::Blocked => "blocked",
            Self::Deferred => "deferred",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "seed" => Some(Self::Seed),
            "exploring" => Some(Self::Exploring),
            "resolved" => Some(Self::Resolved),
            "decided" => Some(Self::Decided),
            "implementing" => Some(Self::Implementing),
            "implemented" => Some(Self::Implemented),
            "blocked" => Some(Self::Blocked),
            "deferred" => Some(Self::Deferred),
            _ => None,
        }
    }

    pub fn icon(&self) -> &'static str {
        match self {
            Self::Seed => "◌",
            Self::Exploring => "◐",
            Self::Resolved => "◉",
            Self::Decided => "●",
            Self::Implementing => "⚙",
            Self::Implemented => "✓",
            Self::Blocked => "✕",
            Self::Deferred => "◑",
        }
    }
}

/// Issue type classification for a design node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IssueType {
    Epic,
    Feature,
    Task,
    Bug,
    Chore,
}

impl IssueType {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "epic" => Some(Self::Epic),
            "feature" => Some(Self::Feature),
            "task" => Some(Self::Task),
            "bug" => Some(Self::Bug),
            "chore" => Some(Self::Chore),
            _ => None,
        }
    }
}

/// A design node parsed from a markdown document with YAML frontmatter.
#[derive(Debug, Clone)]
pub struct DesignNode {
    pub id: String,
    pub title: String,
    pub status: NodeStatus,
    pub parent: Option<String>,
    pub tags: Vec<String>,
    pub dependencies: Vec<String>,
    pub related: Vec<String>,
    pub open_questions: Vec<String>,
    pub branches: Vec<String>,
    pub openspec_change: Option<String>,
    pub issue_type: Option<IssueType>,
    pub priority: Option<u8>,
    pub file_path: PathBuf,
}

/// A decision recorded in a design document.
#[derive(Debug, Clone)]
pub struct DesignDecision {
    pub title: String,
    pub status: String, // "exploring", "decided", "rejected"
    pub rationale: String,
}

/// A research entry in a design document.
#[derive(Debug, Clone)]
pub struct ResearchEntry {
    pub heading: String,
    pub content: String,
}

/// Parsed structured sections from a design document.
#[derive(Debug, Clone, Default)]
pub struct DocumentSections {
    pub overview: String,
    pub research: Vec<ResearchEntry>,
    pub decisions: Vec<DesignDecision>,
    pub open_questions: Vec<String>,
    pub impl_file_scope: Vec<FileScope>,
    pub impl_constraints: Vec<String>,
}

/// File scope entry from Implementation Notes.
#[derive(Debug, Clone)]
pub struct FileScope {
    pub path: String,
    pub description: String,
    pub action: Option<String>, // "new", "modified", "deleted"
}

// ─── OpenSpec Types ─────────────────────────────────────────────────────────

/// Lifecycle stage of an OpenSpec change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChangeStage {
    Proposed,
    Specified,
    Planned,
    Implementing,
    Verifying,
    Archived,
}

impl ChangeStage {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Proposed => "proposed",
            Self::Specified => "specified",
            Self::Planned => "planned",
            Self::Implementing => "implementing",
            Self::Verifying => "verifying",
            Self::Archived => "archived",
        }
    }
}

/// A Given/When/Then scenario.
#[derive(Debug, Clone)]
pub struct Scenario {
    pub title: String,
    pub given: String,
    pub when: String,
    pub then: String,
    pub and_clauses: Vec<String>,
}

/// A requirement grouping scenarios.
#[derive(Debug, Clone)]
pub struct Requirement {
    pub title: String,
    pub description: String,
    pub scenarios: Vec<Scenario>,
}

/// A parsed spec file.
#[derive(Debug, Clone)]
pub struct SpecFile {
    pub domain: String,
    pub file_path: PathBuf,
    pub requirements: Vec<Requirement>,
}

/// Full status of an OpenSpec change.
#[derive(Debug, Clone)]
pub struct ChangeInfo {
    pub name: String,
    pub path: PathBuf,
    pub stage: ChangeStage,
    pub has_proposal: bool,
    pub has_design: bool,
    pub has_specs: bool,
    pub has_tasks: bool,
    pub total_tasks: usize,
    pub done_tasks: usize,
    pub specs: Vec<SpecFile>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_status_round_trip() {
        for s in &["seed", "exploring", "resolved", "decided", "implementing", "implemented", "blocked", "deferred"] {
            let status = NodeStatus::parse(s).unwrap();
            assert_eq!(status.as_str(), *s);
        }
    }

    #[test]
    fn node_status_from_invalid() {
        assert!(NodeStatus::parse("invalid").is_none());
    }

    #[test]
    fn issue_type_from_str() {
        assert_eq!(IssueType::parse("epic"), Some(IssueType::Epic));
        assert_eq!(IssueType::parse("bug"), Some(IssueType::Bug));
        assert!(IssueType::parse("unknown").is_none());
    }
}
