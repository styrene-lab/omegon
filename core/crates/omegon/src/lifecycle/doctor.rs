//! Lifecycle reconciliation audit — flags suspicious design-tree states.
//!
//! This is intentionally heuristic. It does not mutate anything; it reports
//! nodes that look stale so release flow or operators can reconcile them.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

use omegon_opsx::ChangeState;

use super::design;
use super::types::{ChangeInfo, ChangeStage, DesignNode, NodeStatus};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuditKind {
    ImplementedHasOpenQuestions,
    ResolvedWithoutQuestions,
    SeedWithoutQuestions,
    ExploringWithoutQuestions,
    ParentImplementedWithActiveChildren,
    QuestionAppearsAnsweredByDecision,
    OpenSpecStateDrift,
}

impl AuditKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ImplementedHasOpenQuestions => "implemented_has_open_questions",
            Self::ResolvedWithoutQuestions => "resolved_without_questions",
            Self::SeedWithoutQuestions => "seed_without_questions",
            Self::ExploringWithoutQuestions => "exploring_without_questions",
            Self::ParentImplementedWithActiveChildren => "parent_implemented_with_active_children",
            Self::QuestionAppearsAnsweredByDecision => "question_appears_answered_by_decision",
            Self::OpenSpecStateDrift => "openspec_state_drift",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditFinding {
    pub node_id: String,
    pub title: String,
    pub kind: AuditKind,
    pub detail: String,
}

pub fn audit_repo(repo_root: &Path) -> Vec<AuditFinding> {
    let docs_dir = repo_root.join("docs");
    let nodes = design::scan_design_docs(&docs_dir);
    audit_nodes(&nodes)
}

pub fn audit_nodes(nodes: &HashMap<String, DesignNode>) -> Vec<AuditFinding> {
    let mut findings = Vec::new();

    let mut children_by_parent: HashMap<&str, Vec<&DesignNode>> = HashMap::new();
    for node in nodes.values() {
        if let Some(parent) = node.parent.as_deref() {
            children_by_parent.entry(parent).or_default().push(node);
        }
    }

    for node in nodes.values() {
        if matches!(node.status, NodeStatus::Implemented) && !node.open_questions.is_empty() {
            findings.push(AuditFinding {
                node_id: node.id.clone(),
                title: node.title.clone(),
                kind: AuditKind::ImplementedHasOpenQuestions,
                detail: format!(
                    "implemented node still has {} open question(s)",
                    node.open_questions.len()
                ),
            });
        }

        if matches!(node.status, NodeStatus::Resolved) && node.open_questions.is_empty() {
            findings.push(AuditFinding {
                node_id: node.id.clone(),
                title: node.title.clone(),
                kind: AuditKind::ResolvedWithoutQuestions,
                detail: "resolved node has no open questions; likely ready to advance".into(),
            });
        }

        if matches!(node.status, NodeStatus::Seed) && node.open_questions.is_empty() {
            findings.push(AuditFinding {
                node_id: node.id.clone(),
                title: node.title.clone(),
                kind: AuditKind::SeedWithoutQuestions,
                detail: "seed node has no open questions or assumptions; likely underspecified"
                    .into(),
            });
        }

        if matches!(node.status, NodeStatus::Exploring) && node.open_questions.is_empty() {
            findings.push(AuditFinding {
                node_id: node.id.clone(),
                title: node.title.clone(),
                kind: AuditKind::ExploringWithoutQuestions,
                detail: "exploring node has no open questions; likely stale or underspecified"
                    .into(),
            });
        }

        if matches!(node.status, NodeStatus::Implemented)
            && let Some(children) = children_by_parent.get(node.id.as_str())
        {
            let active: Vec<&DesignNode> = children
                .iter()
                .copied()
                .filter(|c| !matches!(c.status, NodeStatus::Implemented | NodeStatus::Deferred))
                .collect();
            if !active.is_empty() {
                findings.push(AuditFinding {
                    node_id: node.id.clone(),
                    title: node.title.clone(),
                    kind: AuditKind::ParentImplementedWithActiveChildren,
                    detail: format!(
                        "implemented parent still has active children: {}",
                        active
                            .iter()
                            .map(|c| c.id.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                });
            }
        }

        if let Some(sections) = design::read_node_sections(node) {
            let decided_titles: HashSet<String> = sections
                .decisions
                .iter()
                .filter(|d| d.status == "decided")
                .map(|d| normalize(&d.title))
                .collect();
            for q in &node.open_questions {
                let nq = normalize(q);
                if decided_titles.iter().any(|d| overlaps_meaningfully(&nq, d)) {
                    findings.push(AuditFinding {
                        node_id: node.id.clone(),
                        title: node.title.clone(),
                        kind: AuditKind::QuestionAppearsAnsweredByDecision,
                        detail: format!("open question appears answered by a decided section: {q}"),
                    });
                }
            }
        }
    }

    findings.sort_by(|a, b| {
        a.node_id
            .cmp(&b.node_id)
            .then(a.kind.as_str().cmp(b.kind.as_str()))
    });
    findings
}

pub fn audit_openspec_changes(
    changes: &[ChangeInfo],
    opsx_states: &HashMap<String, ChangeState>,
) -> Vec<AuditFinding> {
    let mut findings = Vec::new();

    for change in changes {
        let expected = opsx_state_for_stage(change.stage);
        let Some(actual) = opsx_states.get(&change.name).copied() else {
            findings.push(AuditFinding {
                node_id: change.name.clone(),
                title: change.name.clone(),
                kind: AuditKind::OpenSpecStateDrift,
                detail: "OpenSpec change exists on disk but has no omegon-opsx change record"
                    .into(),
            });
            continue;
        };

        if actual != expected {
            findings.push(AuditFinding {
                node_id: change.name.clone(),
                title: change.name.clone(),
                kind: AuditKind::OpenSpecStateDrift,
                detail: format!(
                    "OpenSpec file stage is {}, but omegon-opsx state is {}",
                    change.stage.as_str(),
                    actual.as_str()
                ),
            });
        }
    }

    findings.sort_by(|a, b| a.node_id.cmp(&b.node_id));
    findings
}

pub fn audit_openspec_archives(
    repo_root: &Path,
    opsx_states: &HashMap<String, ChangeState>,
) -> Vec<AuditFinding> {
    let archive_dir = repo_root.join("openspec/archive");
    let entries = match fs::read_dir(&archive_dir) {
        Ok(entries) => entries,
        Err(_) => return vec![],
    };

    let mut findings = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };

        match opsx_states.get(name).copied() {
            Some(ChangeState::Archived) => {}
            Some(state) => findings.push(AuditFinding {
                node_id: name.to_string(),
                title: name.to_string(),
                kind: AuditKind::OpenSpecStateDrift,
                detail: format!(
                    "OpenSpec content is archived on disk, but omegon-opsx state is {}",
                    state.as_str()
                ),
            }),
            None => findings.push(AuditFinding {
                node_id: name.to_string(),
                title: name.to_string(),
                kind: AuditKind::OpenSpecStateDrift,
                detail: "OpenSpec content is archived on disk but has no omegon-opsx change record"
                    .into(),
            }),
        }
    }

    findings.sort_by(|a, b| a.node_id.cmp(&b.node_id));
    findings
}

fn opsx_state_for_stage(stage: ChangeStage) -> ChangeState {
    match stage {
        ChangeStage::Proposed => ChangeState::Proposed,
        ChangeStage::Specified => ChangeState::Specced,
        ChangeStage::Planned => ChangeState::Planned,
        ChangeStage::Implementing => ChangeState::Implementing,
        ChangeStage::Verifying => ChangeState::Verifying,
        ChangeStage::Archived => ChangeState::Archived,
    }
}

fn normalize(s: &str) -> String {
    s.to_lowercase()
        .replace("should ", "")
        .replace("what is ", "")
        .replace("what are ", "")
        .replace("decision:", "")
        .replace(['?', '"', '\'', '—', '-', ':', ',', '.', '(', ')'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn overlaps_meaningfully(a: &str, b: &str) -> bool {
    let aw: HashSet<&str> = a.split_whitespace().filter(|w| w.len() > 3).collect();
    let bw: HashSet<&str> = b.split_whitespace().filter(|w| w.len() > 3).collect();
    let overlap = aw.intersection(&bw).count();
    overlap >= 3
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lifecycle::types::DesignNode;
    use std::path::PathBuf;

    fn node(id: &str, status: NodeStatus) -> DesignNode {
        DesignNode {
            id: id.into(),
            title: id.into(),
            status,
            parent: None,
            tags: vec![],
            dependencies: vec![],
            related: vec![],
            open_questions: vec![],
            branches: vec![],
            openspec_change: None,
            issue_type: None,
            priority: None,
            archive_reason: None,
            superseded_by: None,
            archived_at: None,
            file_path: PathBuf::from(format!("docs/{id}.md")),
        }
    }

    #[test]
    fn flags_resolved_without_questions() {
        let mut nodes = HashMap::new();
        nodes.insert("n1".into(), node("n1", NodeStatus::Resolved));
        let findings = audit_nodes(&nodes);
        assert!(
            findings
                .iter()
                .any(|f| f.kind == AuditKind::ResolvedWithoutQuestions)
        );
    }

    #[test]
    fn flags_seed_without_questions() {
        let mut nodes = HashMap::new();
        nodes.insert("n1".into(), node("n1", NodeStatus::Seed));
        let findings = audit_nodes(&nodes);
        assert!(
            findings
                .iter()
                .any(|f| f.kind == AuditKind::SeedWithoutQuestions)
        );
    }

    #[test]
    fn flags_implemented_parent_with_active_child() {
        let parent = node("parent", NodeStatus::Implemented);
        let mut child = node("child", NodeStatus::Exploring);
        child.parent = Some("parent".into());
        let mut nodes = HashMap::new();
        nodes.insert(parent.id.clone(), parent);
        nodes.insert(child.id.clone(), child);
        let findings = audit_nodes(&nodes);
        assert!(
            findings
                .iter()
                .any(|f| f.kind == AuditKind::ParentImplementedWithActiveChildren)
        );
    }
}
