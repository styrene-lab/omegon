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
    Archived,
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
            Self::Archived => "archived",
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
            "archived" => Some(Self::Archived),
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
            Self::Archived => "🗄",
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
    pub archive_reason: Option<String>,
    pub superseded_by: Option<String>,
    pub archived_at: Option<String>,
    pub file_path: PathBuf,
}

impl DesignNode {
    /// Count of open questions tagged as [assumption].
    pub fn assumption_count(&self) -> usize {
        self.open_questions
            .iter()
            .filter(|q| q.starts_with("[assumption]"))
            .count()
    }

    /// Count of regular (non-assumption) open questions.
    pub fn question_count(&self) -> usize {
        self.open_questions.len() - self.assumption_count()
    }
}

impl DocumentSections {
    /// Readiness score: decisions / (decisions + open_questions).
    /// Returns 1.0 when all unknowns are resolved, 0.0 when nothing is decided.
    /// Open questions include both regular questions and [assumption]-tagged ones.
    pub fn readiness_score(&self) -> f32 {
        let decided = self
            .decisions
            .iter()
            .filter(|d| d.status == "decided")
            .count();
        let total = decided + self.open_questions.len();
        if total == 0 {
            return 0.0; // no data = not ready (seed nodes)
        }
        decided as f32 / total as f32
    }

    /// Count of open questions tagged as [assumption].
    pub fn assumption_count(&self) -> usize {
        self.open_questions
            .iter()
            .filter(|q| q.starts_with("[assumption]"))
            .count()
    }

    /// Count of regular (non-assumption) open questions.
    pub fn question_count(&self) -> usize {
        self.open_questions.len() - self.assumption_count()
    }
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
        for s in &[
            "seed",
            "exploring",
            "resolved",
            "decided",
            "implementing",
            "implemented",
            "blocked",
            "deferred",
        ] {
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

    #[test]
    fn readiness_score_all_decided() {
        let sections = DocumentSections {
            decisions: vec![
                DesignDecision {
                    title: "A".into(),
                    status: "decided".into(),
                    rationale: "".into(),
                },
                DesignDecision {
                    title: "B".into(),
                    status: "decided".into(),
                    rationale: "".into(),
                },
            ],
            open_questions: vec![],
            ..Default::default()
        };
        assert!((sections.readiness_score() - 1.0).abs() < 0.01);
    }

    #[test]
    fn readiness_score_with_questions() {
        let sections = DocumentSections {
            decisions: vec![DesignDecision {
                title: "A".into(),
                status: "decided".into(),
                rationale: "".into(),
            }],
            open_questions: vec![
                "How does X work?".into(),
                "[assumption] The operator has git installed".into(),
            ],
            ..Default::default()
        };
        // 1 decided / (1 + 2) = 0.333
        assert!((sections.readiness_score() - 0.333).abs() < 0.01);
        assert_eq!(sections.assumption_count(), 1);
        assert_eq!(sections.question_count(), 1);
    }

    #[test]
    fn readiness_score_empty_is_zero() {
        let sections = DocumentSections::default();
        assert_eq!(sections.readiness_score(), 0.0);
    }

    #[test]
    fn readiness_score_rejected_decisions_not_counted() {
        let sections = DocumentSections {
            decisions: vec![
                DesignDecision {
                    title: "A".into(),
                    status: "decided".into(),
                    rationale: "".into(),
                },
                DesignDecision {
                    title: "B".into(),
                    status: "rejected".into(),
                    rationale: "".into(),
                },
            ],
            open_questions: vec!["?".into()],
            ..Default::default()
        };
        // 1 decided / (1 + 1) = 0.5 (rejected doesn't count as a known-known)
        assert!((sections.readiness_score() - 0.5).abs() < 0.01);
    }

    #[test]
    fn assumption_count_on_node() {
        let node = DesignNode {
            id: "test".into(),
            title: "Test".into(),
            status: NodeStatus::Exploring,
            parent: None,
            tags: vec![],
            dependencies: vec![],
            related: vec![],
            open_questions: vec![
                "Regular question".into(),
                "[assumption] Git is installed".into(),
                "[assumption] Vault is reachable".into(),
            ],
            branches: vec![],
            openspec_change: None,
            issue_type: None,
            priority: None,
            archive_reason: None,
            superseded_by: None,
            archived_at: None,
            file_path: std::path::PathBuf::from("test.md"),
        };
        assert_eq!(node.assumption_count(), 2);
        assert_eq!(node.question_count(), 1);
    }
}
