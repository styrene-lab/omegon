//! Lifecycle ContextProvider — injects design-tree and openspec context
//! into the system prompt based on signals.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use omegon_traits::{ContextInjection, ContextProvider, ContextSignals};

use super::design;
use super::spec;
use super::types::*;

/// Provides lifecycle context (design nodes + openspec changes) to the agent.
pub struct LifecycleContextProvider {
    /// All design nodes, keyed by id.
    nodes: HashMap<String, DesignNode>,
    /// Parsed sections cache (lazy-loaded).
    sections_cache: HashMap<String, DocumentSections>,
    /// Active openspec changes.
    changes: Vec<ChangeInfo>,
    /// Currently focused node id (if any).
    focused_node: Option<String>,
    /// The repo root for re-scanning.
    repo_path: PathBuf,
}

impl LifecycleContextProvider {
    /// Initialize by scanning docs/ and openspec/ directories.
    pub fn new(repo_path: &Path) -> Self {
        let docs_dir = repo_path.join("docs");
        let nodes = design::scan_design_docs(&docs_dir);
        let changes = spec::list_changes(repo_path);

        tracing::info!(
            nodes = nodes.len(),
            changes = changes.len(),
            "Lifecycle context initialized"
        );

        Self {
            nodes,
            sections_cache: HashMap::new(),
            changes,
            focused_node: None,
            repo_path: repo_path.to_path_buf(),
        }
    }

    /// Set the focused design node.
    pub fn set_focus(&mut self, node_id: Option<String>) {
        self.focused_node = node_id;
    }

    /// Get the focused node ID (if any).
    pub fn focused_node_id(&self) -> Option<&str> {
        self.focused_node.as_deref()
    }

    /// Get a design node by id.
    pub fn get_node(&self, id: &str) -> Option<&DesignNode> {
        self.nodes.get(id)
    }

    /// Get all design nodes.
    pub fn all_nodes(&self) -> &HashMap<String, DesignNode> {
        &self.nodes
    }

    /// Get all active changes.
    pub fn changes(&self) -> &[ChangeInfo] {
        &self.changes
    }

    /// Refresh by re-scanning (call after mutations from TS side).
    pub fn refresh(&mut self) {
        let docs_dir = self.repo_path.join("docs");
        self.nodes = design::scan_design_docs(&docs_dir);
        self.changes = spec::list_changes(&self.repo_path);
        self.sections_cache.clear();
    }

    fn get_sections(&mut self, node_id: &str) -> Option<&DocumentSections> {
        if !self.sections_cache.contains_key(node_id)
            && let Some(node) = self.nodes.get(node_id)
                && let Some(sections) = design::read_node_sections(node) {
                    self.sections_cache.insert(node_id.to_string(), sections);
                }
        self.sections_cache.get(node_id)
    }
}

impl ContextProvider for LifecycleContextProvider {
    fn provide_context(&self, _signals: &ContextSignals<'_>) -> Option<ContextInjection> {
        let mut parts = Vec::new();

        // 1. Focused design node context
        if let Some(ref node_id) = self.focused_node
            && let Some(node) = self.nodes.get(node_id) {
                // Read sections (can't use get_sections due to &self)
                if let Some(sections) = design::read_node_sections(node) {
                    let injection = design::build_context_injection(node, &sections);
                    if !injection.is_empty() {
                        parts.push(injection);
                    }
                }
            }

        // 2. Active openspec changes (if any are implementing/verifying)
        let active: Vec<_> = self.changes.iter()
            .filter(|c| matches!(c.stage, ChangeStage::Implementing | ChangeStage::Verifying))
            .collect();
        if !active.is_empty() {
            let injection = spec::build_context_injection(&active.iter().copied().cloned().collect::<Vec<_>>());
            if !injection.is_empty() {
                parts.push(injection);
            }
        }

        if parts.is_empty() {
            return None;
        }

        Some(ContextInjection {
            source: "lifecycle".into(),
            content: parts.join("\n\n"),
            priority: 150, // Between base prompt (200) and memory facts
            ttl_turns: 3,  // Refresh every few turns
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_returns_none_when_empty() {
        let provider = LifecycleContextProvider {
            nodes: HashMap::new(),
            sections_cache: HashMap::new(),
            changes: vec![],
            focused_node: None,
            repo_path: PathBuf::from("/nonexistent"),
        };

        let signals = ContextSignals {
            user_prompt: "hello",
            recent_tools: &[],
            recent_files: &[],
            lifecycle_phase: &omegon_traits::LifecyclePhase::Idle,
            turn_number: 1,
            context_budget_tokens: 4000,
        };

        assert!(provider.provide_context(&signals).is_none());
    }

    #[test]
    fn provider_injects_focused_node() {
        let mut nodes = HashMap::new();
        let tmp = std::env::temp_dir().join("omegon-lifecycle-test");
        let _ = std::fs::create_dir_all(&tmp);
        let doc_path = tmp.join("test.md");
        std::fs::write(&doc_path, "---\nid: test\ntitle: Test\nstatus: decided\n---\n\n# Test\n\n## Overview\n\nTest overview.\n\n## Decisions\n\n### Use X\n\n**Status:** decided\n\n**Rationale:** Because Y.\n").unwrap();

        let fm = design::parse_frontmatter(&std::fs::read_to_string(&doc_path).unwrap()).unwrap();
        let node = design::node_from_frontmatter(&fm, doc_path).unwrap();
        nodes.insert("test".to_string(), node);

        let provider = LifecycleContextProvider {
            nodes,
            sections_cache: HashMap::new(),
            changes: vec![],
            focused_node: Some("test".to_string()),
            repo_path: tmp.clone(),
        };

        let signals = ContextSignals {
            user_prompt: "hello",
            recent_tools: &[],
            recent_files: &[],
            lifecycle_phase: &omegon_traits::LifecyclePhase::Idle,
            turn_number: 1,
            context_budget_tokens: 4000,
        };

        let injection = provider.provide_context(&signals).unwrap();
        assert!(injection.content.contains("● test — Test"));
        assert!(injection.content.contains("Test overview"));
        assert!(injection.content.contains("Use X"));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn provider_injects_active_changes() {
        let provider = LifecycleContextProvider {
            nodes: HashMap::new(),
            sections_cache: HashMap::new(),
            changes: vec![ChangeInfo {
                name: "my-change".into(),
                path: PathBuf::new(),
                stage: ChangeStage::Implementing,
                has_proposal: true,
                has_design: true,
                has_specs: true,
                has_tasks: true,
                total_tasks: 8,
                done_tasks: 5,
                specs: vec![],
            }],
            focused_node: None,
            repo_path: PathBuf::from("/nonexistent"),
        };

        let signals = ContextSignals {
            user_prompt: "hello",
            recent_tools: &[],
            recent_files: &[],
            lifecycle_phase: &omegon_traits::LifecyclePhase::Idle,
            turn_number: 1,
            context_budget_tokens: 4000,
        };

        let injection = provider.provide_context(&signals).unwrap();
        assert!(injection.content.contains("my-change"));
        assert!(injection.content.contains("5/8"));
    }
}
