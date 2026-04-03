//! Core types — state enums, node/change/milestone data structures.

use serde::{Deserialize, Serialize};

/// Design node lifecycle states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeState {
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

impl std::fmt::Display for NodeState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl NodeState {
    /// Valid transitions from this state.
    pub fn valid_transitions(self) -> &'static [NodeState] {
        use NodeState::*;
        match self {
            Seed => &[Exploring, Deferred, Archived],
            Exploring => &[Resolved, Decided, Blocked, Deferred, Archived],
            Resolved => &[Decided, Exploring, Blocked, Deferred, Archived],
            Decided => &[Implementing, Exploring, Blocked, Deferred, Archived],
            Implementing => &[Implemented, Decided, Blocked, Deferred, Archived],
            // Implemented can reopen — "the implementation was wrong"
            Implemented => &[Exploring, Decided, Deferred, Archived],
            // Blocked can resume to where it was interrupted
            Blocked => &[Exploring, Decided, Implementing, Deferred, Archived],
            Deferred => &[Seed, Exploring, Archived],
            Archived => &[Seed, Exploring],
        }
    }

    /// Can transition to the target state?
    pub fn can_transition_to(self, target: NodeState) -> bool {
        self.valid_transitions().contains(&target)
    }

    pub fn as_str(self) -> &'static str {
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
        match s.to_lowercase().as_str() {
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
}

/// OpenSpec change lifecycle states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChangeState {
    Proposed,
    Specced,
    Planned,
    /// Testing — test stubs written from spec scenarios, all failing.
    /// TDD: tests exist before implementation code.
    Testing,
    Implementing,
    Verifying,
    Archived,
    /// Abandoned — proposal or change was dropped before completion.
    Abandoned,
}

impl std::fmt::Display for ChangeState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl ChangeState {
    pub fn valid_transitions(self) -> &'static [ChangeState] {
        use ChangeState::*;
        match self {
            Proposed => &[Specced, Abandoned],
            Specced => &[Planned, Proposed, Abandoned],
            Planned => &[Testing, Specced, Abandoned],
            Testing => &[Implementing, Planned, Abandoned],
            Implementing => &[Verifying, Testing, Abandoned], // can go back to Testing
            Verifying => &[Archived, Implementing, Abandoned],
            Archived => &[Proposed],
            Abandoned => &[Proposed],
        }
    }

    pub fn can_transition_to(self, target: ChangeState) -> bool {
        self.valid_transitions().contains(&target)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Proposed => "proposed",
            Self::Specced => "specced",
            Self::Planned => "planned",
            Self::Testing => "testing",
            Self::Implementing => "implementing",
            Self::Verifying => "verifying",
            Self::Archived => "archived",
            Self::Abandoned => "abandoned",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "proposed" => Some(Self::Proposed),
            "specced" => Some(Self::Specced),
            "planned" => Some(Self::Planned),
            "testing" => Some(Self::Testing),
            "implementing" => Some(Self::Implementing),
            "verifying" => Some(Self::Verifying),
            "archived" => Some(Self::Archived),
            "abandoned" => Some(Self::Abandoned),
            _ => None,
        }
    }
}

/// Milestone lifecycle states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MilestoneState {
    Open,
    Frozen,
    Released,
}

impl std::fmt::Display for MilestoneState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Open => f.write_str("open"),
            Self::Frozen => f.write_str("frozen"),
            Self::Released => f.write_str("released"),
        }
    }
}

/// Priority levels (1 = critical, 5 = trivial).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Priority(pub u8);

impl Priority {
    pub fn new(level: u8) -> Self {
        Self(level.clamp(1, 5))
    }
}

/// Issue classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IssueType {
    Epic,
    Feature,
    Task,
    Bug,
    Chore,
}

/// A design node — the fundamental unit of the design tree.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DesignNode {
    pub id: String,
    pub title: String,
    pub state: NodeState,
    pub parent: Option<String>,
    pub tags: Vec<String>,
    pub priority: Option<Priority>,
    pub issue_type: Option<IssueType>,
    pub open_questions: Vec<String>,
    pub decisions: Vec<Decision>,
    pub overview: String,
    /// Bound OpenSpec change name (if implementing).
    pub bound_change: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// A design decision.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Decision {
    pub title: String,
    pub status: DecisionStatus,
    pub rationale: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DecisionStatus {
    Exploring,
    Decided,
    Rejected,
}

/// An OpenSpec change — tracks a spec-driven implementation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Change {
    pub name: String,
    pub title: String,
    pub state: ChangeState,
    pub bound_node: Option<String>,
    pub specs: Vec<String>, // spec domain names
    /// Test file paths — registered when test stubs are written (TDD).
    #[serde(default)]
    pub test_files: Vec<String>,
    pub tasks_total: usize,
    pub tasks_done: usize,
    pub created_at: String,
    pub updated_at: String,
}

/// A release milestone.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Milestone {
    pub name: String,
    pub state: MilestoneState,
    pub nodes: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// An audit log entry — records every state transition with reason.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    pub timestamp: String,
    pub entity_type: String, // "node", "change", "milestone"
    pub entity_id: String,
    pub from_state: String,
    pub to_state: String,
    /// Why this transition happened. Required for force_transition.
    pub reason: Option<String>,
    /// True if this was a forced override bypassing FSM validation.
    pub forced: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_state_transitions() {
        assert!(NodeState::Seed.can_transition_to(NodeState::Exploring));
        assert!(NodeState::Exploring.can_transition_to(NodeState::Decided));
        assert!(NodeState::Decided.can_transition_to(NodeState::Implementing));
        assert!(!NodeState::Seed.can_transition_to(NodeState::Implemented));
        // Implemented CAN reopen now
        assert!(NodeState::Implemented.can_transition_to(NodeState::Exploring));
        assert!(NodeState::Implemented.can_transition_to(NodeState::Decided));
    }

    #[test]
    fn blocked_can_resume_to_implementing() {
        assert!(NodeState::Blocked.can_transition_to(NodeState::Implementing));
        assert!(NodeState::Blocked.can_transition_to(NodeState::Decided));
        assert!(NodeState::Blocked.can_transition_to(NodeState::Exploring));
        assert!(NodeState::Deferred.can_transition_to(NodeState::Archived));
        assert!(NodeState::Archived.can_transition_to(NodeState::Exploring));
    }

    #[test]
    fn change_state_transitions() {
        assert!(ChangeState::Proposed.can_transition_to(ChangeState::Specced));
        assert!(ChangeState::Planned.can_transition_to(ChangeState::Testing));
        assert!(ChangeState::Testing.can_transition_to(ChangeState::Implementing));
        assert!(ChangeState::Implementing.can_transition_to(ChangeState::Verifying));
        assert!(!ChangeState::Proposed.can_transition_to(ChangeState::Archived));
        // Can't skip Testing
        assert!(!ChangeState::Planned.can_transition_to(ChangeState::Implementing));
        // Abandoned from any active state
        assert!(ChangeState::Proposed.can_transition_to(ChangeState::Abandoned));
        assert!(ChangeState::Planned.can_transition_to(ChangeState::Abandoned));
        // Archived can reopen
        assert!(ChangeState::Archived.can_transition_to(ChangeState::Proposed));
        // Abandoned can revive
        assert!(ChangeState::Abandoned.can_transition_to(ChangeState::Proposed));
    }

    #[test]
    fn node_state_parse_roundtrip() {
        for state in [
            NodeState::Seed,
            NodeState::Exploring,
            NodeState::Decided,
            NodeState::Implementing,
            NodeState::Implemented,
            NodeState::Blocked,
            NodeState::Archived,
        ] {
            assert_eq!(NodeState::parse(state.as_str()), Some(state));
        }
    }
}
